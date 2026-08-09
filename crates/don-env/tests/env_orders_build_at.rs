use don_env::generated as g;
use don_env::state::{EnvWorld, Rules};
use don_sim::command::QueuePos;
use don_sim::order::OrderIndex;
use don_sim::systems::order_dispatch::OrderRec;

#[test]
fn product_build_at_preserves_identity_and_progress_until_the_mandatory_host_exists() {
    let (rules, _, _) = Rules::load(None, None);
    let mut world = EnvWorld::new(rules, 8, 1, 64, 64);
    let builder = world
        .spawn(0, g::UNIT_TYPE_BASE as u16, 4_800, 9_600)
        .expect("builder spawn must fit");
    let target = world
        .spawn(0, g::BUILD_TYPE_BASE as u16, 5_000, 9_600)
        .expect("target spawn must fit");
    let builder_row = world.sim.row_of(builder).expect("builder is live");
    let target_row = world.sim.row_of(target).expect("target is live");
    let mut order = OrderRec::of_kind(OrderIndex::BuildAt);
    order.target_who = i32::from(world.sim.owner()[target_row]);
    order.target_o = i32::from(world.sim.units.o()[target_row]);
    order.target_uid = world.sim.units.uid()[target_row] as u16;
    world
        .install_order(builder_row, order.clone(), QueuePos::New)
        .expect("BUILD_AT installation is admitted");
    let before_pos = (
        world.sim.pos_x()[builder_row],
        world.sim.pos_y()[builder_row],
    );
    let before_hits = world.sim.hits()[target_row];
    let before_gap = world.unimplemented.unit[g::uv::BUILD];

    world.frame();

    assert_eq!(
        (
            world.sim.pos_x()[builder_row],
            world.sim.pos_y()[builder_row],
        ),
        before_pos,
        "BUILD_AT must not cross the missing adjacency/reswarm boundary"
    );
    assert_eq!(world.sim.hits()[target_row], before_hits);
    assert_eq!(
        world.orders[builder_row].front(),
        Some(&order),
        "the target identity and complete order payload must survive fail-closed"
    );
    assert_eq!(world.unimplemented.unit[g::uv::BUILD], before_gap + 1);

    // A visibly invalid address is not locally retireable either: retail can immediately
    // enter build_done and mutate the worker's next assignment.
    let mut invalid = OrderRec::of_kind(OrderIndex::BuildAt);
    invalid.target_who = -1;
    invalid.target_o = -1;
    invalid.target_uid = u16::MAX;
    world
        .install_order(builder_row, invalid.clone(), QueuePos::New)
        .expect("malformed save/import payload remains representable");
    let before_gap = world.unimplemented.unit[g::uv::BUILD];
    world.frame();
    assert_eq!(world.orders[builder_row].front(), Some(&invalid));
    assert_eq!(world.unimplemented.unit[g::uv::BUILD], before_gap + 1);
}
