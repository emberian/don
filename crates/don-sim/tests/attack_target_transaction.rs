//! Exact ATTACK target identity survives every owner before the production consumer.

use don_sim::order::{Order, OrderList, OrderTargetIdentity};
use don_sim::systems::order_dispatch;
use don_sim::systems::save_load::{load_sim, save_sim};
use don_sim::tick::Sim;
use don_sim::Handle;

#[test]
fn exact_attack_target_survives_issue_queue_reset_publish_and_save_load() {
    let mut sim = Sim::new(0x51a7_2026, 4);
    sim.map.world.seed = 0x51a7_2026;
    let actor = sim.spawn_unit(0, 50, 768, 768, 4).unwrap();
    let target = sim.spawn_unit(1, 51, 800, 800, 4).unwrap();
    let actor_row = sim.world.row_of(actor).unwrap();
    let target_row = sim.world.row_of(target).unwrap();
    let identity = OrderTargetIdentity {
        handle: target,
        who: sim.world.units.get_who(target_row) as i8,
        o: sim.world.units.o()[target_row],
        uid: sim.world.units.get_uid(target_row),
    };

    assert!(sim.issue(actor, Order::attack_exact(identity)));
    assert_eq!(
        sim.world
            .orders(actor_row)
            .current()
            .and_then(Order::exact_target_identity),
        Some(identity)
    );

    // The executable queue owns the retail UID and additive Handle while its cursor moves.
    let mut queue = order_dispatch::adopt(sim.world.orders(actor_row));
    queue.push_back(order_dispatch::OrderRec::move_to(900, 900, 48));
    assert!(queue.advance());
    queue.reset();
    let current = queue.current().unwrap();
    assert_eq!(current.target_uid, identity.uid);
    assert_eq!(current.target_handle, Some(identity.handle));
    let mut published = OrderList::new();
    order_dispatch::publish(&queue, &mut published);
    assert_eq!(
        published.current().and_then(Order::exact_target_identity),
        Some(identity)
    );

    // DoNSave v7 is the persistent owner; load/resave cannot narrow the target transaction.
    let bytes = save_sim(&sim).unwrap();
    let loaded = load_sim(&bytes).unwrap();
    assert_eq!(save_sim(&loaded).unwrap(), bytes);
    assert_eq!(
        loaded
            .world
            .orders(actor_row)
            .current()
            .and_then(Order::exact_target_identity),
        Some(identity)
    );
}

#[test]
fn legacy_attack_does_not_invent_a_handle_but_keeps_the_retail_uid_on_conversion() {
    let mut legacy = Order::attack(1, 7);
    legacy.target_uid = 23;
    let widened = order_dispatch::OrderRec::from(legacy);
    assert_eq!(widened.target_uid, 23);
    assert_eq!(widened.target_handle, None);
    let narrowed = Order::from(widened);
    assert_eq!(narrowed.target_uid, 23);
    assert_eq!(narrowed.exact_target_identity(), None);
}

#[test]
fn save_rejects_an_out_of_world_exact_target_handle() {
    let mut sim = Sim::new(7, 4);
    sim.map.world.seed = 7;
    let actor = sim.spawn_unit(0, 50, 768, 768, 4).unwrap();
    assert!(sim.issue(
        actor,
        Order::attack_exact(OrderTargetIdentity {
            handle: Handle {
                id: u32::MAX,
                generation: 0,
            },
            who: 1,
            o: 0,
            uid: 1,
        })
    ));
    assert!(matches!(
        save_sim(&sim),
        Err(don_sim::systems::save_load::SaveError::World(_))
    ));
}
