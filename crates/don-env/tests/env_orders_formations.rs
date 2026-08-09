use don_env::state::{EnvWorld, Rules};
use don_sim::command::{build, Bridge, Package, QueuePos};
use don_sim::order::OrderIndex;
use don_sim::systems::groups_guys::Formation;

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
}
