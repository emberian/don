use don_sim::command::{build, Bridge, ObjectTable, Package, QueuePos, Slot};
use don_sim::order::OrderIndex;
use don_sim::systems::groups_guys::{formation_order_coord, Formation, FormationMember, GroupData};

fn captain_profile() -> FormationMember {
    FormationMember {
        category: 0,
        x_spacing: 96,
        y_spacing: 144,
        formation_size: 1,
        guy_spacing: 48,
        modern_infantry: false,
        width: 50,
        angle: 0,
    }
}

fn three_captains() -> GroupData {
    let mut group = GroupData {
        ox: 4_800,
        oy: 9_600,
        ..GroupData::default()
    };
    for o in 0..3 {
        assert!(group.add(o, 1, false, 0, 0));
    }
    group
}

#[test]
fn compute_form_places_captains_and_preserves_dead_tail_bytes() {
    let mut group = three_captains();
    group.off_x[3] = 0x1122_3344;
    group.off_y[3] = -0x1020_3040;
    group.curr_x[3] = 0x5566_7788;
    group.curr_y[3] = -0x5060_7080;
    group.angles[3] = 0x31;
    group.list[3] = 0x2345;

    let profiles = [captain_profile(); 3];
    let (layout, angle) = group
        .compute_form(
            &profiles,
            4_800,
            9_600,
            Formation::Line as i32,
            50,
            false,
            0,
            4_800,
            9_600,
            false,
        )
        .expect("Line has a fully recovered captain path");

    assert_eq!(angle, 0);
    assert_eq!(layout.leader_index, 0);
    assert_eq!(&layout.to_x[..3], &[4_800, 4_848, 4_800]);
    assert_eq!(&layout.to_y[..3], &[9_600, 9_744, 9_888]);
    assert_eq!(&layout.off_x[..3], &[0, 48, 0]);
    assert_eq!(&layout.off_y[..3], &[0, -144, -288]);
    assert_eq!(&group.off_x[..3], &[0, 1, 0]);
    assert_eq!(&group.off_y[..3], &[0, -3, -6]);
    assert_eq!(group.form_num, 3);

    assert_eq!(group.off_x[3], 0x1122_3344);
    assert_eq!(group.off_y[3], -0x1020_3040);
    assert_eq!(group.curr_x[3], 0x5566_7788);
    assert_eq!(group.curr_y[3], -0x5060_7080);
    assert_eq!(group.angles[3], 0x31);
    assert_eq!(group.list[3], 0x2345);
    assert_eq!(layout.to_x[3], 0, "the transient Form object is cleared");
    assert_eq!(layout.to_y[3], 0, "the transient Form object is cleared");
}

#[test]
fn unsupported_form_is_transactional() {
    let mut group = three_captains();
    group.form = 4;
    group.form_num = 17;
    group.off_x[0] = 19;
    group.off_y[2] = -23;
    group.angles[1] = -0x20;
    let before = group.clone();

    assert!(group
        .compute_form(
            &[captain_profile(); 3],
            4_800,
            9_600,
            Formation::Wedge as i32,
            50,
            false,
            0,
            4_800,
            9_600,
            false,
        )
        .is_none());
    assert_eq!(group, before, "failure must not leave a partial formation");
}

#[test]
fn every_form_hotkey_shape_has_the_recovered_offset_and_angle_pattern() {
    let cases = [
        (Formation::Line, [0, -144, -288], [0, 0, 0]),
        (Formation::Refused, [0, -192, -288], [0, 0x20, 0]),
        (Formation::Envelop, [0, -96, -288], [0, -0x20, 0]),
        (Formation::EchelonRight, [0, -192, -288], [0x20, 0x20, 0x20]),
        (
            Formation::EchelonLeft,
            [0, -96, -288],
            [-0x20, -0x20, -0x20],
        ),
    ];

    for (form, expected_forward, expected_angles) in cases {
        let mut group = three_captains();
        let (layout, _) = group
            .compute_form(
                &[captain_profile(); 3],
                4_800,
                9_600,
                form as i32,
                50,
                false,
                0,
                4_800,
                9_600,
                false,
            )
            .unwrap_or_else(|| panic!("{form:?} must stay on the exact hotkey path"));
        assert_eq!(&layout.off_x[..3], &[0, 48, 0], "{form:?} lateral");
        assert_eq!(&layout.off_y[..3], &expected_forward, "{form:?} forward");
        assert_eq!(&group.angles[..3], &expected_angles, "{form:?} angles");
    }
}

#[test]
fn form_command_installs_distinct_retail_captain_destinations() {
    let mut fleet = ObjectTable::new(8);
    for o in 0..3 {
        let mut slot = Slot::unit(100 + o as u16, 4_800, 9_600);
        let mut profile = captain_profile();
        // `get_form_mod_option` excludes -1 rather than averaging it. The two
        // contributors therefore resolve to width 75, retaining the one-row pattern.
        profile.width = if o == 0 { -1 } else { 75 };
        slot.formation_member = Some(profile);
        fleet.put(1, o, slot);
    }

    let mut bridge = Bridge::new();
    let mut package = Package::new(1, 0);
    bridge
        .process_all(&mut package, &build::group(1, &[0, 1, 2]), &mut fleet)
        .unwrap();
    {
        let group = bridge.groups.get_mut(package.group).unwrap();
        group.off_x[3] = 0x1122_3344;
        group.curr_y[3] = -0x5060_7080;
        group.angles[3] = 0x31;
    }

    bridge
        .process_all(
            &mut package,
            &build::form(Formation::Line as i32, 0, QueuePos::Last),
            &mut fleet,
        )
        .unwrap();

    let expected = [(100, 200), (101, 203), (100, 206)];
    for (o, destination) in expected.into_iter().enumerate() {
        let orders: Vec<_> = fleet.get(1, o as i16).unwrap().orders.iter().collect();
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].kind, OrderIndex::MoveTo);
        assert_eq!((orders[0].x, orders[0].y), destination);
        assert_eq!(orders[0].facing, 1);
    }
    assert_eq!(formation_order_coord(4_800), 100);
    assert_eq!(
        formation_order_coord(-1),
        -1,
        "the retail table floors negatives"
    );

    let group = bridge.groups.get(package.group).unwrap();
    assert_eq!(&group.curr_x[..3], &[0, 48, 0]);
    assert_eq!(&group.curr_y[..3], &[0, 144, 288]);
    assert_eq!(group.off_x[3], 0x1122_3344);
    assert_eq!(group.curr_y[3], -0x5060_7080);
    assert_eq!(group.angles[3], 0x31);
}
