use don_sim::command::{build, Bridge, ObjectTable, Package, QueuePos, Slot};
use don_sim::order::{MoveOrderState, Order, OrderIndex};
use don_sim::systems::{order_dispatch, save_load};
use don_sim::tick::Sim;

fn mutated_move_state() -> MoveOrderState {
    MoveOrderState {
        angle: 0x0102_0304,
        dest: 2,
        pause: 3,
        retry: 4,
        attempts: 5,
        timer: 6,
        facing: 0x1112_1314,
        dest_x: 701,
        dest_y: 702,
        last_x: 703,
        last_y: 704,
        coll_x: 705,
        coll_y: 706,
        orig_x: 707,
        orig_y: 708,
        off_x: -709,
        off_y: 710,
        group_oxx: 711,
        group_whose: 7,
        group_id: 712,
        group_form_id: 713,
        group_angle: 714,
        in_group: 1,
    }
}

/// The real wire receiver and the production tick/save owner used to be two disconnected
/// green-test islands. This test crosses that seam deliberately: opcode bytes install the
/// executable node, `publish` hands it to `World`, DoNSave restores it, and a real frame
/// resumes with the restored node still owned by the tick world.
#[test]
fn wire_move_order_survives_tick_owned_save_reload_and_resume() {
    let mut sim = Sim::new(0x1234_5678, 4);
    let actor = sim.spawn_unit(0, 17, 96, 96, 4).unwrap();
    let row = sim.world.row_of(actor).unwrap();

    let mut host = ObjectTable::new(1);
    host.put(0, 0, Slot::unit(0x4242, 96, 96));
    let mut bridge = Bridge::new();
    let mut package = Package::new(0, 0);
    let mut packet = build::group(0, &[0]);
    packet.extend_from_slice(&build::move_to(900, 1000, QueuePos::New, 0));
    bridge
        .process_all(&mut package, &packet, &mut host)
        .unwrap();

    let executable = host.get_mut(0, 0).unwrap().orders.current_mut().unwrap();
    assert_eq!(executable.kind, OrderIndex::MoveTo);
    let expected = mutated_move_state();
    executable.angle = expected.angle;
    executable.dest = expected.dest;
    executable.pause = expected.pause;
    executable.retry = expected.retry;
    executable.attempts = expected.attempts;
    executable.timer = expected.timer;
    executable.facing = expected.facing;
    executable.dest_x = expected.dest_x;
    executable.dest_y = expected.dest_y;
    executable.last_x = expected.last_x;
    executable.last_y = expected.last_y;
    executable.coll_x = expected.coll_x;
    executable.coll_y = expected.coll_y;
    executable.orig_x = expected.orig_x;
    executable.orig_y = expected.orig_y;
    executable.off_x = expected.off_x;
    executable.off_y = expected.off_y;
    executable.group_oxx = expected.group_oxx;
    executable.group_whose = expected.group_whose;
    executable.group_id = expected.group_id;
    executable.group_form_id = expected.group_form_id;
    executable.group_angle = expected.group_angle;
    executable.in_group = expected.in_group;

    order_dispatch::publish(&host.get(0, 0).unwrap().orders, sim.world.orders_mut(row));
    let installed = sim.world.orders(row).current().unwrap().clone();
    assert_eq!(installed.kind, OrderIndex::MoveTo);
    assert_eq!(installed.move_state, Some(expected));

    let bytes = save_load::save_sim(&sim).unwrap();
    let mut resumed = save_load::load_sim(&bytes).unwrap();
    let restored = resumed.world.orders(row).current().unwrap().clone();
    assert_eq!(restored, installed);

    // The actor's default zero speed makes this a deterministic held movement frame. It still
    // enters Sim's production MOVE_TO arm; no live-collision sidecar is invented after load.
    let before_frame = resumed.world.frame;
    resumed.do_frame();
    assert_eq!(resumed.world.frame, before_frame.wrapping_add(1));
    assert_eq!(resumed.world.orders(row).current(), Some(&restored));

    let resumed_bytes = save_load::save_sim(&resumed).unwrap();
    let resumed_again = save_load::load_sim(&resumed_bytes).unwrap();
    assert_eq!(resumed_again.world.orders(row).current(), Some(&restored));
}

#[test]
fn save_rejects_a_foreign_typed_move_payload_before_mutating_its_owner() {
    let mut sim = Sim::new(7, 2);
    let actor = sim.spawn_unit(0, 1, 0, 0, 0).unwrap();
    let row = sim.world.row_of(actor).unwrap();
    let malformed = Order {
        kind: OrderIndex::Attack,
        move_state: Some(mutated_move_state()),
        ..Order::default()
    };
    sim.world.orders_mut(row).replace(malformed);
    let before = sim.world.orders(row).clone();
    let before_frame = sim.world.frame;
    let before_rng = sim.world.random.state();

    assert_eq!(
        save_load::save_sim(&sim),
        Err(save_load::SaveError::Invalid(
            "movement payload on foreign order kind"
        ))
    );
    assert_eq!(sim.world.orders(row), &before);
    assert_eq!(sim.world.frame, before_frame);
    assert_eq!(sim.world.random.state(), before_rng);
}
