use don_sim::systems::ammo::{
    ammo_init_cruise_spline, ammo_step_cruise_spline, Ammo, AmmoPool, AmmoSplineChecksumError,
    RetailSpline, SplineBuildError, SplineVec3, FLAG_ALIVE, FLAG_FLYING, TRAJ_SPLINE,
};

fn path_args() -> (f32, SplineVec3, SplineVec3, SplineVec3, SplineVec3) {
    (
        10.0,
        SplineVec3::new(100.0, 200.0, 300.0),
        SplineVec3::new(300.0, 250.0, 400.0),
        SplineVec3::new(900.0, 300.0, 600.0),
        SplineVec3::new(600.0, 100.0, 500.0),
    )
}

fn build() -> (Ammo, RetailSpline) {
    let mut ammo = Ammo::default();
    let (minimum, start, control, end, optional) = path_args();
    let spline = ammo_init_cruise_spline(&mut ammo, minimum, start, control, end, optional)
        .expect("finite positive fixture");
    (ammo, spline)
}

#[test]
fn cruise_constructor_generates_every_walked_array_with_retail_capacities() {
    let (ammo, mut spline) = build();

    assert!(ammo.has_spline);
    assert_eq!(ammo.w.traj, TRAJ_SPLINE);
    assert_eq!(ammo.w.total_time, 17);
    assert_eq!((spline.degree, spline.flags), (3, 0x10));
    assert_eq!(spline.max_control_depth_ratio.to_bits(), 4.0_f32.to_bits());
    assert_eq!(spline.control_verts.checksum_header(), (4, 4, -1, 0));
    assert_eq!(spline.knots.checksum_header(), (0, 0, -1, 0));
    assert_eq!(spline.weights.checksum_header(), (0, 0, -1, 0));
    assert_eq!(spline.spline_knots.checksum_header(), (8, 8, -1, 0));
    assert_eq!(spline.spline_verts.checksum_header(), (17, 32, -1, 0));
    assert_eq!(spline.spline_normals.checksum_header(), (17, 32, -1, 0));
    assert_eq!(
        spline.spline_knots.as_slice(),
        &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]
    );
    assert_eq!(spline.spline_verts[0], spline.control_verts[0]);
    assert_eq!(
        spline.spline_verts[spline.spline_verts.len() - 1],
        spline.control_verts[spline.control_verts.len() - 1]
    );
    assert_eq!(spline.spline_normals.len(), spline.spline_verts.len());

    assert_eq!(
        spline.depth, 95,
        "set_min_seg_length stores the unclamped depth"
    );
    assert_eq!(spline.total_spline_length.to_bits(), 0x445b_12ea);
    assert_eq!(spline.walk_checksum(1), 0xa0e7_bfdd);
}

#[test]
fn zero_optional_control_selects_the_quadratic_constructor_arm() {
    let mut ammo = Ammo::default();
    let (_, start, control, end, _) = path_args();
    let mut spline =
        ammo_init_cruise_spline(&mut ammo, 10.0, start, control, end, SplineVec3::default())
            .unwrap();

    assert_eq!(spline.degree, 2);
    assert_eq!(spline.control_verts.checksum_header(), (3, 4, -1, 0));
    assert_eq!(
        spline.spline_knots.as_slice(),
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
    );
    assert_eq!(spline.spline_verts.checksum_header(), (13, 16, -1, 0));
    assert_eq!(spline.spline_normals.checksum_header(), (13, 16, -1, 0));
    assert_eq!(ammo.w.total_time, 13);
    assert_eq!(spline.walk_checksum(1), 0x0600_9686);
}

#[test]
fn ammo_channel_walks_the_spline_immediately_after_its_non_null_byte() {
    let (mut ammo, spline) = build();
    ammo.w.flags = FLAG_ALIVE | FLAG_FLYING;
    let pool = AmmoPool {
        slots: vec![ammo],
        ammo_index: 1,
    };
    assert_eq!(
        pool.checksum_with_splines(&mut []),
        Err(AmmoSplineChecksumError::MissingSpline { slot: 0 })
    );

    let mut paths = vec![Some(spline)];
    let checksum = pool
        .checksum_with_splines(&mut paths)
        .expect("slot-aligned path");
    assert_eq!(checksum, 0xdabf_bff4);

    paths[0].as_mut().unwrap().spline_verts[8].z =
        f32::from_bits(paths[0].as_ref().unwrap().spline_verts[8].z.to_bits() + 1);
    assert_ne!(
        pool.checksum_with_splines(&mut paths).unwrap(),
        checksum,
        "the combined ammo channel includes generated spline payloads"
    );
}

#[test]
fn spline_walk_is_float_capacity_and_flag_mutation_sensitive() {
    let (_, mut baseline) = build();
    let expected = baseline.walk_checksum(1);

    let mut float_mutation = baseline.clone();
    float_mutation.spline_verts[8].x =
        f32::from_bits(float_mutation.spline_verts[8].x.to_bits() + 1);
    assert_ne!(float_mutation.walk_checksum(1), expected);

    let mut visible_flag = baseline.clone();
    visible_flag.control_verts.set_flags(1);
    assert_ne!(visible_flag.walk_checksum(1), expected);

    let mut transient_flag = baseline;
    transient_flag.control_verts.set_flags(0x40);
    assert_eq!(transient_flag.walk_checksum(1), expected);
    assert_eq!(transient_flag.control_verts.flags(), 0);
}

#[test]
fn cruise_step_uses_cur_time_index_and_keeps_the_last_sample_for_impact() {
    let (mut ammo, spline) = build();
    ammo.w.cur_time = 1;
    let point = ammo_step_cruise_spline(&mut ammo.w, &spline).expect("interior sample");
    assert_eq!(point, spline.spline_verts[1]);
    assert_eq!(
        (ammo.w.ex, ammo.w.ey, ammo.w.ez),
        (point.x as i32, point.y as i32, point.z as i32)
    );

    ammo.w.cur_time = spline.spline_verts.len() as u32 - 1;
    let before = (ammo.w.ex, ammo.w.ey, ammo.w.ez);
    assert_eq!(ammo_step_cruise_spline(&mut ammo.w, &spline), None);
    assert_eq!((ammo.w.ex, ammo.w.ey, ammo.w.ez), before);
}

#[test]
fn invalid_spline_facts_fail_before_ammo_mutation() {
    let mut ammo = Ammo::default();
    ammo.w.total_time = 77;
    let before = ammo;
    let (_, start, control, end, optional) = path_args();
    assert_eq!(
        ammo_init_cruise_spline(&mut ammo, 0.0, start, control, end, optional),
        Err(SplineBuildError::NonPositiveSegmentLength)
    );
    assert_eq!(ammo, before);
    assert_eq!(
        ammo_init_cruise_spline(
            &mut ammo,
            10.0,
            start,
            control,
            end,
            SplineVec3::new(f32::NAN, 0.0, 0.0)
        ),
        Err(SplineBuildError::NonFiniteInput)
    );
    assert_eq!(ammo, before);
}
