use don_env::state::{EnvWorld, Rules};
use don_sim::command::{build, Bridge, Package, QueuePos};
use don_sim::order::OrderIndex;
use don_sim::systems::groups_guys::Formation;
use don_sim::systems::order_dispatch::OrderRec;

#[test]
fn product_env_form_uses_runtime_type_facts_and_installs_group_move() {
    let (rules, caps_real, _) = Rules::load(None, None);
    if !caps_real {
        eprintln!("SKIP: schema/live/env-typecaps.bin absent (run gen/gen_spec.py)");
        return;
    }
    assert!(
        rules.formation_cap(50).is_some(),
        "this product test requires captured postload formation facts"
    );

    let mut world = EnvWorld::new(rules, 8, 1, 64, 64);
    let handles: Vec<_> = (0..3)
        .map(|_| {
            world
                .spawn(0, 50, 4_800, 9_600)
                .expect("Citizen spawn must fit")
        })
        .collect();
    let rows: Vec<_> = handles
        .iter()
        .map(|&handle| world.sim.row_of(handle).expect("spawned handle is live"))
        .collect();
    let objects: Vec<_> = rows.iter().map(|&row| world.sim.units.o()[row]).collect();

    for &row in &rows {
        assert_eq!(world.sim.units.form()[row], Formation::Mob as i8);
        assert_eq!(world.sim.units.form_mod()[row], -1);
        world.sim.units.angle_mut()[row] = 0;
    }

    let mut bridge = Bridge::new();
    let mut package = Package::new(0, 0);
    bridge
        .process_all(&mut package, &build::group(0, &objects), &mut world)
        .unwrap();
    bridge
        .process_all(
            &mut package,
            &build::form(Formation::Line as i32, 0, QueuePos::Last),
            &mut world,
        )
        .unwrap();

    // Citizen is runtime category 10. Unlike category 0's alternating half-column,
    // the recovered category rule keeps these three captains on the same lateral axis.
    let expected_destinations = [(100, 200), (100, 203), (100, 206)];
    for ((&row, &object), destination) in rows.iter().zip(objects.iter()).zip(expected_destinations)
    {
        assert_eq!(world.sim.units.form()[row], Formation::Line as i8);
        let order = world.orders[row]
            .current()
            .expect("FORM installs one executable order node");
        assert_eq!(order.kind, OrderIndex::GroupMove);
        assert_eq!((order.x, order.y), destination);
        assert_eq!(order.flags, 5);
        assert_eq!(order.group_oxx, i32::from(objects[0]));
        assert_eq!(order.group_whose, 0);
        assert_eq!(order.group_id, 0);
        assert_eq!(order.group_form_id, 0);
        assert_eq!(order.group_angle, 0);
        assert_eq!(order.in_group, 0);
        assert_eq!((order.orig_x, order.orig_y), (4_800, 9_600));
        assert_eq!(world.sim.units.o()[row], object);
    }

    let group = bridge
        .groups
        .get(package.group)
        .expect("selection is interned");
    assert_eq!(group.form, Formation::Line as i32);
    assert_eq!(group.order_num, 1);

    let before: Vec<_> = rows
        .iter()
        .map(|&row| (world.sim.pos_x()[row], world.sim.pos_y()[row]))
        .collect();
    world.frame();
    for (&row, old) in rows.iter().zip(before) {
        assert_ne!(
            (world.sim.pos_x()[row], world.sim.pos_y()[row]),
            old,
            "FORM's installed GROUP_MOVE must execute while its leader/group/order facts are live"
        );
        assert_eq!(
            world.orders[row].front().map(|order| order.kind),
            Some(OrderIndex::GroupMove),
            "the distant formation destination must remain queued after one frame"
        );
    }

    for _ in 0..2_048 {
        if rows.iter().all(|&row| world.orders[row].is_empty()) {
            break;
        }
        world.frame();
    }
    for ((&row, _), destination) in rows.iter().zip(objects.iter()).zip(expected_destinations) {
        assert!(
            world.orders[row].is_empty(),
            "FORM movement must retire after reaching its installed member destination"
        );
        assert_eq!(
            (world.sim.pos_x()[row], world.sim.pos_y()[row]),
            destination
        );
    }

    bridge
        .process_all(
            &mut package,
            &build::move_to(1_200, 1_400, QueuePos::New, 2),
            &mut world,
        )
        .unwrap();
    for &row in &rows {
        assert_eq!(
            world.orders[row].front().map(|order| order.kind),
            Some(OrderIndex::GroupAttackTo),
            "attack-move on the live line formation must install GROUP_ATTACK_TO"
        );
    }

    // Env has no authoritative fight/do_attack_to host, so valid grouped nodes stay intact.
    // The ungrouped retail branch is wholly local and converts without asking for combat
    // state, but Citizen's resulting ATTACK_TO still requires target-selection capability.
    let detached = rows[2];
    world.sim.units.group_mut()[detached] = -1;
    let detached_before = (world.sim.pos_x()[detached], world.sim.pos_y()[detached]);
    world.frame();
    assert_eq!(
        world.orders[detached].front().map(|order| order.kind),
        Some(OrderIndex::AttackTo)
    );
    assert_eq!(
        (world.sim.pos_x()[detached], world.sim.pos_y()[detached]),
        detached_before,
        "ungroup conversion returns before ordinary ATTACK_TO movement"
    );
    let attack_gaps_before = world.unimplemented.unit[don_env::generated::uv::ATTACK];
    let move_gaps_before = world.unimplemented.unit[don_env::generated::uv::MOVE_TO];
    world.frame();
    assert_eq!(
        (world.sim.pos_x()[detached], world.sim.pos_y()[detached]),
        detached_before,
        "combat-capable ATTACK_TO must not move before find_melee_target is available"
    );
    assert_eq!(
        world.orders[detached].front().map(|order| order.kind),
        Some(OrderIndex::AttackTo),
        "the unsupported ordinary node must remain intact"
    );
    assert_eq!(
        world.unimplemented.unit[don_env::generated::uv::ATTACK],
        attack_gaps_before + 1,
        "the detached attacker stays visibly unsupported"
    );
    assert_eq!(
        world.unimplemented.unit[don_env::generated::uv::MOVE_TO],
        move_gaps_before + 2,
        "both grouped movement wrappers stay visibly unsupported"
    );
    for &row in &rows[..2] {
        assert_eq!(
            world.orders[row].front().map(|order| order.kind),
            Some(OrderIndex::GroupAttackTo),
            "grouped attack-move must fail closed without fight/do_attack_to callbacks"
        );
    }
}

#[test]
fn product_env_group_attack_converts_locally_or_fails_closed() {
    let (rules, _, _) = Rules::load(None, None);
    let mut world = EnvWorld::new(rules, 8, 1, 64, 64);
    let actor = world
        .spawn(0, 50, 4_800, 9_600)
        .expect("actor spawn must fit");
    let target = world
        .spawn(1, 50, 5_000, 9_600)
        .expect("target spawn must fit");
    let actor_row = world.sim.row_of(actor).expect("actor is live");
    let target_row = world.sim.row_of(target).expect("target is live");
    let actor_o = world.sim.units.o()[actor_row];
    let target_o = world.sim.units.o()[target_row];
    let target_uid = world.sim.units.uid()[target_row] as u16;

    let grouped = OrderRec {
        kind: OrderIndex::GroupAttack,
        flags: 4,
        target_o: i32::from(target_o),
        target_who: 1,
        target_uid,
        attack_def_x: 111,
        attack_def_y: 222,
        attack_mandatory: 2,
        attack_defensive: 1,
        attack_in_range: 1,
        attack_ever_in_range: 1,
        attack_new_ord: 1,
        group_oxx: i32::from(actor_o),
        group_whose: 0,
        group_id: 7,
        group_angle: 0x1234_5678,
        group_attack_temporary: 9,
        group_attack_oxxx: 17,
        group_attack_whosoever: 4,
        ..OrderRec::default()
    };

    world.sim.units.group_mut()[actor_row] = -1;
    world
        .install_order(actor_row, grouped.clone(), QueuePos::New)
        .expect("ordinary installation is admitted");
    world.frame();
    let ordinary = world.orders[actor_row]
        .front()
        .expect("ungrouped conversion retains an ATTACK");
    assert_eq!(ordinary.kind, OrderIndex::Attack);
    assert_eq!(world.target[actor_row], target);
    assert_eq!(world.sim.units.angle()[actor_row], 0x1234_5678);
    assert_eq!(
        (
            ordinary.flags,
            ordinary.target_o,
            ordinary.target_who,
            ordinary.target_uid,
            ordinary.attack_def_x,
            ordinary.attack_def_y,
            ordinary.attack_mandatory,
            ordinary.attack_defensive,
            ordinary.attack_in_range,
            ordinary.attack_ever_in_range,
            ordinary.attack_new_ord,
        ),
        (
            4,
            i32::from(target_o),
            1,
            target_uid,
            111,
            222,
            2,
            1,
            1,
            1,
            1
        )
    );
    assert_eq!(ordinary.group_id, -1);
    assert_eq!(ordinary.group_attack_temporary, 0);

    world.sim.units.group_mut()[actor_row] = 3;
    world.sim.units.angle_mut()[actor_row] = 77;
    world.sim.units.unit_masks_mut()[actor_row] = 0x40;
    world
        .install_order(actor_row, grouped.clone(), QueuePos::New)
        .expect("grouped installation is admitted");
    let before = world.orders[actor_row].clone();
    let before_angle_state = (
        world.sim.units.angle()[actor_row],
        world.sim.units.unit_masks()[actor_row],
    );
    let before_count = world.unimplemented.unit[don_env::generated::uv::ATTACK];
    world.frame();
    assert_eq!(world.orders[actor_row], before);
    assert_eq!(
        (
            world.sim.units.angle()[actor_row],
            world.sim.units.unit_masks()[actor_row],
        ),
        before_angle_state,
        "host failure must precede Unit::set_angle publication"
    );
    assert_eq!(world.target[actor_row], target);
    assert_eq!(
        world.unimplemented.unit[don_env::generated::uv::ATTACK],
        before_count + 1,
        "missing fight/range/Groups facts must be visible and fail closed"
    );
}
