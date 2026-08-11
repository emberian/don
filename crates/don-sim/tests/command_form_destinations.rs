use don_sim::command::group_action_entry::formation_order_destination;
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

    // `Form::to_x/to_y` reach `Unit::add_group_move_order` `0x005E4710` as the UCoord cells
    // 100/200, 101/203 and 100/206, and that constructor stores `cell * 0x30 + 0x18` — the
    // centred Coord — into both `x`/`y` and `dest_x`/`dest_y` [measured, `005e4710.c`].
    let expected = [(4_824, 9_624), (4_872, 9_768), (4_824, 9_912)];
    for (o, destination) in expected.into_iter().enumerate() {
        let orders: Vec<_> = fleet.get(1, o as i16).unwrap().orders.iter().collect();
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].kind, OrderIndex::GroupMove);
        assert_eq!((orders[0].x, orders[0].y), destination);
        assert_eq!(orders[0].flags, 5, "allocated + nonzero form flags");
        assert_eq!(orders[0].facing, 0);
        assert_eq!(orders[0].group_oxx, 0);
        assert_eq!(orders[0].group_whose, 1);
        assert_eq!(orders[0].group_id, 6_400);
        assert_eq!(orders[0].group_form_id, 0);
        assert_eq!(orders[0].group_angle, 0);
        assert_eq!(orders[0].in_group, 0);
        assert_eq!((orders[0].orig_x, orders[0].orig_y), (4_800, 9_600));
    }
    assert_eq!(formation_order_coord(4_800), 100);
    assert_eq!(
        formation_order_coord(-1),
        -1,
        "the retail table floors negatives"
    );
    assert_eq!(
        formation_order_destination(4_800),
        4_824,
        "the stored destination is the centre of the cell, not the cell index"
    );

    let group = bridge.groups.get(package.group).unwrap();
    assert_eq!(&group.curr_x[..3], &[0, 48, 0]);
    assert_eq!(&group.curr_y[..3], &[0, 144, 288]);
    assert_eq!(group.off_x[3], 0x1122_3344);
    assert_eq!(group.curr_y[3], -0x5060_7080);
    assert_eq!(group.angles[3], 0x31);
    assert_eq!(group.order_num, 1);
}

#[test]
fn group_move_promotion_applies_each_recovered_member_gate() {
    let mut fleet = ObjectTable::new(8);
    for o in 0..5 {
        let mut slot = Slot::unit(200 + o as u16, 4_800, 9_600);
        slot.formation_member = Some(captain_profile());
        fleet.put(1, o, slot);
    }
    fleet
        .get_mut(1, 1)
        .unwrap()
        .formation_member
        .as_mut()
        .unwrap()
        .modern_infantry = true;
    fleet.get_mut(1, 2).unwrap().domain = 1;
    fleet.get_mut(1, 3).unwrap().role = 0x10;
    fleet.get_mut(1, 4).unwrap().unit_masks = 4;

    let mut bridge = Bridge::new();
    bridge.frame = 7;
    let mut package = Package::new(1, 7);
    bridge
        .process_all(&mut package, &build::group(1, &[0, 1, 2, 3, 4]), &mut fleet)
        .unwrap();
    bridge
        .process_all(
            &mut package,
            &build::move_to(8_000, 9_000, QueuePos::Last, 2),
            &mut fleet,
        )
        .unwrap();

    assert_eq!(
        fleet.get(1, 0).unwrap().orders.current().unwrap().kind,
        OrderIndex::GroupAttackTo
    );
    assert_eq!(
        fleet.get(1, 0).unwrap().orders.current().unwrap().group_id,
        13_400
    );
    assert_eq!(
        fleet.get(1, 1).unwrap().orders.current().unwrap().kind,
        OrderIndex::AttackTo
    );
    assert_eq!(
        fleet.get(1, 2).unwrap().orders.current().unwrap().kind,
        OrderIndex::AttackTo
    );
    assert_eq!(
        fleet.get(1, 3).unwrap().orders.current().unwrap().kind,
        OrderIndex::AttackTo
    );
    assert_eq!(
        fleet.get(1, 4).unwrap().orders.current().unwrap().kind,
        OrderIndex::AttackTo
    );

    // Role bit 0x10 admits GROUP_MOVE only while the exact instance latch is present;
    // installation then clears the separate 0x400 pending bit and leaves this bit alone.
    fleet.get_mut(1, 3).unwrap().unit_masks = 0x0004_0400;
    let mut disembark_move = build::move_to(8_500, 9_500, QueuePos::Last, 1);
    disembark_move[21] = 1;
    bridge
        .process_all(&mut package, &disembark_move, &mut fleet)
        .unwrap();
    let admitted = fleet.get(1, 3).unwrap();
    assert_eq!(
        admitted.orders.iter().last().unwrap().kind,
        OrderIndex::GroupMove
    );
    assert_eq!(admitted.orders.iter().last().unwrap().flags, 0x21);
    assert_eq!(admitted.orders.iter().last().unwrap().group_id, 13_401);
    assert_eq!(admitted.unit_masks, 0x0004_0000);
}

#[test]
fn unsupported_form_command_changes_nothing() {
    for unsupported in [Formation::Square, Formation::Wedge, Formation::Mob] {
        let mut fleet = ObjectTable::new(4);
        for o in 0..2 {
            let mut slot = Slot::unit(300 + o as u16, 4_800, 9_600);
            slot.form = Formation::Line as i8;
            slot.formation_member = Some(captain_profile());
            fleet.put(1, o, slot);
        }
        let mut bridge = Bridge::new();
        let mut package = Package::new(1, 0);
        bridge
            .process_all(&mut package, &build::group(1, &[0, 1]), &mut fleet)
            .unwrap();
        let before = bridge.groups.get(package.group).unwrap().clone();

        bridge
            .process_all(
                &mut package,
                &build::form(unsupported as i32, 0, QueuePos::Last),
                &mut fleet,
            )
            .unwrap();

        assert_eq!(bridge.groups.get(package.group).unwrap(), &before);
        for o in 0..2 {
            let slot = fleet.get(1, o).unwrap();
            assert_eq!(slot.form, Formation::Line as i8);
            assert!(slot.orders.is_empty());
        }
    }
}
