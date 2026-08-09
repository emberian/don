use don_sim::systems::ammo::{
    ammo_init_cruise_spline, ammo_init_nuke_spline_high_arc, ammo_init_nuke_spline_terrain,
    ammo_step_cruise_spline, Ammo, AmmoPool, AmmoSplineChecksumError, NukeSplineEnv,
    NukeTerrainSample, RetailSpline, SplineBuildError, SplineVec3, FLAG_ALIVE, FLAG_FLYING,
    TRAJ_SPLINE,
};
use std::cell::RefCell;

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
fn fixed_nuke_arm_installs_the_high_arc_and_exact_custom_knot_header() {
    let mut ammo = Ammo::default();
    let start = SplineVec3::new(100.0, 200.0, 300.0);
    let end = SplineVec3::new(900.0, 1_000.0, 500.0);
    let mut spline = ammo_init_nuke_spline_high_arc(&mut ammo, start, end).unwrap();

    assert_eq!(spline.control_verts.checksum_header(), (12, 16, -1, 0));
    assert_eq!(spline.control_verts[0], start);
    assert_eq!(
        spline.control_verts[1],
        SplineVec3::new(100.0, 200.0, 550.0)
    );
    assert_eq!(
        spline.control_verts[5],
        SplineVec3::new(300.0, 400.0, 20_350.0)
    );
    assert_eq!(
        spline.control_verts[6],
        SplineVec3::new(500.0, 600.0, 20_400.0)
    );
    assert_eq!(spline.control_verts[11], end);
    assert_eq!(spline.knots.checksum_header(), (17, 17, -1, 0));
    assert_eq!(&spline.knots.as_slice()[..4], &[100.0, 30.0, 15.0, 10.0]);
    assert_eq!(spline.spline_knots.checksum_header(), (16, 16, -1, 0));
    assert_eq!(spline.spline_verts.len(), 121);
    assert_eq!(spline.spline_normals.len(), 121);
    assert_eq!(ammo.w.total_time, 120);
    assert_eq!(spline.total_spline_length.to_bits(), 0x4718_900c);
    assert_eq!(spline.walk_checksum(1), 0x545d_b134);
}

struct TerrainFixture {
    calls: RefCell<Vec<(i32, i32)>>,
    flags: [u16; 4],
    missing: Option<(i32, i32)>,
}

impl TerrainFixture {
    fn complete(flags: [u16; 4]) -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            flags,
            missing: None,
        }
    }
}

impl NukeSplineEnv for TerrainFixture {
    fn nuke_terrain(&self, x: i32, y: i32) -> Option<NukeTerrainSample> {
        let ordinal = self.calls.borrow().len();
        self.calls.borrow_mut().push((x, y));
        if self.missing == Some((x, y)) {
            return None;
        }
        Some(NukeTerrainSample {
            z: 1_000 + ordinal as i32 * 10,
            flags: self.flags[ordinal],
        })
    }
}

#[test]
fn terrain_nuke_arm_queries_truncated_points_and_applies_flag_priority() {
    let env = TerrainFixture::complete([0, 0x4003, 0x30, 0x4033]);
    let mut ammo = Ammo::default();
    let start = SplineVec3::new(100.9, 200.9, 300.0);
    let end = SplineVec3::new(3_500.9, 3_000.9, 500.0);
    let mut spline = ammo_init_nuke_spline_terrain(&mut ammo, &env, start, end).unwrap();

    assert_eq!(
        env.calls.into_inner(),
        [(780, 760), (1_460, 1_320), (2_140, 1_880), (2_820, 2_440)]
    );
    assert_eq!(spline.control_verts.checksum_header(), (8, 8, -1, 0));
    assert_eq!(spline.control_verts[0], start);
    assert_eq!(
        spline.control_verts[1],
        SplineVec3::new(100.9, 200.9, 550.0)
    );
    assert_eq!(
        spline.control_verts[2],
        SplineVec3::new(780.0, 760.0, 1_250.0)
    );
    assert_eq!(
        spline.control_verts[3],
        SplineVec3::new(1_460.0, 1_320.0, 2_260.0)
    );
    assert_eq!(
        spline.control_verts[4],
        SplineVec3::new(2_140.0, 1_880.0, 1_770.0)
    );
    assert_eq!(
        spline.control_verts[5],
        SplineVec3::new(2_820.0, 2_440.0, 2_280.0)
    );
    assert_eq!(
        spline.control_verts[6],
        SplineVec3::new(3_500.9, 3_000.9, 750.0)
    );
    assert_eq!(spline.control_verts[7], end);
    assert_eq!(spline.knots.checksum_header(), (13, 13, -1, 0));
    assert!(spline.knots.as_slice().iter().all(|&knot| knot == 30.0));
    assert_eq!(spline.spline_knots.checksum_header(), (12, 16, -1, 0));
    assert_eq!(spline.depth, 70);
    assert_eq!(spline.spline_verts.len(), 71);
    assert_eq!(spline.spline_normals.len(), 71);
    assert_eq!(ammo.w.traj, TRAJ_SPLINE);
    assert_eq!(ammo.w.total_time, 70);
    assert!(ammo.has_spline);
    assert_eq!(spline.total_spline_length.to_bits(), 0x45ba_0b3d);
    assert_eq!(spline.walk_checksum(1), 0xa8a8_206e);

    let changed_tier = TerrainFixture::complete([0, 0x4003, 0x30, 0x33]);
    let mut changed_ammo = Ammo::default();
    let mut changed =
        ammo_init_nuke_spline_terrain(&mut changed_ammo, &changed_tier, start, end).unwrap();
    assert_ne!(
        changed.walk_checksum(1),
        0xa8a8_206e,
        "clearing terrain bit 14 changes the fourth control height and walked path"
    );
}

#[test]
fn missing_nuke_terrain_fails_before_ammo_installation() {
    let env = TerrainFixture {
        calls: RefCell::new(Vec::new()),
        flags: [0; 4],
        missing: Some((1_460, 1_320)),
    };
    let mut ammo = Ammo::default();
    ammo.w.total_time = 77;
    let before = ammo;
    assert_eq!(
        ammo_init_nuke_spline_terrain(
            &mut ammo,
            &env,
            SplineVec3::new(100.9, 200.9, 300.0),
            SplineVec3::new(3_500.9, 3_000.9, 500.0),
        ),
        Err(SplineBuildError::MissingTerrain { x: 1_460, y: 1_320 })
    );
    assert_eq!(ammo, before);
    assert_eq!(env.calls.into_inner(), [(780, 760), (1_460, 1_320)]);
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
