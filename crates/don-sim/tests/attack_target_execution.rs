//! Production-tick consumption of an exact ATTACK target identity.

use don_sim::order::{Order, OrderIndex, OrderTargetIdentity};
use don_sim::tick::Sim;
use don_sim::Handle;

const RECHARGE_SENTINEL: u8 = 7;

fn sim_with_attack_pair() -> (Sim, Handle, usize, OrderTargetIdentity) {
    let mut sim = Sim::new(0xa77a_c2026, 4);
    sim.activate(0);
    sim.activate(1);
    let actor = sim.spawn_unit(0, 50, 768, 768, 4).unwrap();
    let target = sim.spawn_unit(1, 51, 800, 800, 4).unwrap();
    let actor_row = sim.world.row_of(actor).unwrap();
    let target_row = sim.world.row_of(target).unwrap();
    sim.world.units.set_recharging(actor_row, RECHARGE_SENTINEL);
    let target = OrderTargetIdentity {
        handle: target,
        who: sim.world.units.get_who(target_row) as i8,
        o: sim.world.units.o()[target_row],
        uid: sim.world.units.get_uid(target_row),
    };
    (sim, actor, actor_row, target)
}

#[test]
fn exact_identity_is_consumed_before_the_existing_recharge_gate() {
    let (mut sim, actor, actor_row, target) = sim_with_attack_pair();
    assert!(sim.issue(actor, Order::attack_exact(target)));

    sim.do_frame();

    assert_eq!(sim.world.orders(actor_row).order_type(), OrderIndex::Attack);
    assert_eq!(
        sim.world
            .orders(actor_row)
            .current()
            .and_then(Order::exact_target_identity),
        Some(target)
    );
    assert_eq!(
        sim.world.units.get_recharging(actor_row),
        RECHARGE_SENTINEL - 1
    );
}

#[test]
fn identity_mismatches_retire_before_recharge_or_position_mutates() {
    let corruptions: [(&str, fn(OrderTargetIdentity) -> OrderTargetIdentity); 4] = [
        ("uid", |mut target: OrderTargetIdentity| {
            target.uid ^= 1;
            target
        }),
        ("handle generation", |mut target: OrderTargetIdentity| {
            target.handle.generation = target.handle.generation.wrapping_add(1);
            target
        }),
        ("owner", |mut target: OrderTargetIdentity| {
            target.who = 0;
            target
        }),
        ("object index", |mut target: OrderTargetIdentity| {
            target.o = target.o.wrapping_add(1);
            target
        }),
    ];

    for (label, corrupt) in corruptions {
        let (mut sim, actor, actor_row, target) = sim_with_attack_pair();
        let actor_pos = (
            sim.world.units.x_internal()[actor_row],
            sim.world.units.y_internal()[actor_row],
        );
        assert!(
            sim.issue(actor, Order::attack_exact(corrupt(target))),
            "{label}"
        );

        sim.do_frame();

        assert_eq!(
            sim.world.orders(actor_row).order_type(),
            OrderIndex::None,
            "{label}"
        );
        assert_eq!(
            sim.world.units.get_recharging(actor_row),
            RECHARGE_SENTINEL,
            "{label}"
        );
        assert_eq!(
            (
                sim.world.units.x_internal()[actor_row],
                sim.world.units.y_internal()[actor_row]
            ),
            actor_pos,
            "{label}"
        );
    }
}

#[test]
fn legacy_attack_keeps_its_non_authoritative_compatibility_path() {
    let (mut sim, actor, actor_row, target) = sim_with_attack_pair();
    assert!(sim.issue(actor, Order::attack(target.who, target.o)));

    sim.do_frame();

    assert_eq!(sim.world.orders(actor_row).order_type(), OrderIndex::Attack);
    assert_eq!(
        sim.world
            .orders(actor_row)
            .current()
            .and_then(Order::exact_target_identity),
        None
    );
    assert_eq!(
        sim.world.units.get_recharging(actor_row),
        RECHARGE_SENTINEL - 1
    );
}
