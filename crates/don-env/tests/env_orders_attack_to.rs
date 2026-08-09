use don_env::generated as g;
use don_env::state::{EnvWorld, Rules};
use don_sim::command::QueuePos;
use don_sim::order::OrderIndex;
use don_sim::systems::order_dispatch::OrderRec;

fn attack_to(x: i32, y: i32) -> OrderRec {
    let mut order = OrderRec::move_to(x, y, 0);
    order.kind = OrderIndex::AttackTo;
    order
}

#[test]
fn product_attack_to_moves_only_when_future_combat_branches_are_authoritative() {
    let (rules, caps_real, _) = Rules::load(None, None);
    if !caps_real {
        eprintln!("SKIP: schema/live/env-typecaps.bin absent (run gen/gen_spec.py)");
        return;
    }
    let unarmed_type = (g::UNIT_TYPE_BASE as u16..g::GAIA_TYPE_BASE as u16)
        .find(|&type_id| {
            let cap = rules.caps.get(type_id);
            cap.attack == 0 && cap.move_rate > 0
        })
        .expect("shipped unit table must contain a mobile unarmed type");
    let attacker_type = (g::UNIT_TYPE_BASE as u16..g::GAIA_TYPE_BASE as u16)
        .find(|&type_id| {
            let cap = rules.caps.get(type_id);
            cap.attack > 0 && cap.move_rate > 0
        })
        .expect("shipped unit table must contain a mobile attacker");

    let mut world = EnvWorld::new(rules, 8, 1, 64, 64);
    let unarmed = world
        .spawn(0, unarmed_type, 4_800, 9_600)
        .expect("unarmed spawn must fit");
    let attacker = world
        .spawn(0, attacker_type, 4_800, 10_000)
        .expect("attacker spawn must fit");
    let unarmed_row = world.sim.row_of(unarmed).expect("unarmed actor is live");
    let attacker_row = world.sim.row_of(attacker).expect("attacker is live");
    world
        .install_order(unarmed_row, attack_to(6_000, 9_600), QueuePos::New)
        .expect("unarmed ATTACK_TO install is admitted");
    world
        .install_order(attacker_row, attack_to(6_000, 10_000), QueuePos::New)
        .expect("attacker ATTACK_TO install is admitted");
    let unarmed_before = (
        world.sim.pos_x()[unarmed_row],
        world.sim.pos_y()[unarmed_row],
    );
    let attacker_before = (
        world.sim.pos_x()[attacker_row],
        world.sim.pos_y()[attacker_row],
    );
    let before_count = world.unimplemented.unit[g::uv::ATTACK];

    world.frame();

    assert_ne!(
        (
            world.sim.pos_x()[unarmed_row],
            world.sim.pos_y()[unarmed_row],
        ),
        unarmed_before,
        "ungrouped zero-attack actors have an exact no-search/no-pause branch"
    );
    assert_eq!(
        (
            world.sim.pos_x()[attacker_row],
            world.sim.pos_y()[attacker_row],
        ),
        attacker_before,
        "an attacker must not move before find_melee_target capability is preflighted"
    );
    assert_eq!(
        world.orders[attacker_row].front().map(|order| order.kind),
        Some(OrderIndex::AttackTo)
    );
    assert_eq!(world.unimplemented.unit[g::uv::ATTACK], before_count + 1);

    world.sim.units.group_mut()[unarmed_row] = 3;
    world
        .install_order(unarmed_row, attack_to(7_000, 9_600), QueuePos::New)
        .expect("grouped ATTACK_TO install is admitted");
    let grouped_before = (
        world.sim.pos_x()[unarmed_row],
        world.sim.pos_y()[unarmed_row],
    );
    let before_count = world.unimplemented.unit[g::uv::ATTACK];
    world.frame();
    assert_eq!(
        (
            world.sim.pos_x()[unarmed_row],
            world.sim.pos_y()[unarmed_row],
        ),
        grouped_before,
        "Group pause/army facts are mandatory even for a zero-attack actor"
    );
    assert_eq!(
        world.unimplemented.unit[g::uv::ATTACK],
        before_count + 2,
        "both grouped-unarmed and attacking nodes remain visibly unsupported"
    );
}
