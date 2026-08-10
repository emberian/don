// SPDX-License-Identifier: GPL-3.0-or-later

//! Real step-14 reachability for the recovered SPECIAL_ANIM dispatcher.

use don_sim::order::{Order, OrderIndex, SpecialAnimType};
use don_sim::systems::movement::PathData;
use don_sim::systems::save_load::{load_sim, save_sim};
use don_sim::tick::{Sim, StepRun};

fn actor_with_order(sim: &mut Sim, order: Order) -> (don_sim::world::Handle, usize) {
    let actor = sim.spawn_unit(0, 1, 192, 192, 1).unwrap();
    assert!(sim.issue(actor, order));
    let row = sim.world.row_of(actor).unwrap();
    (actor, row)
}

#[test]
fn real_frame_commits_object_free_exit_as_one_queue_path_transaction() {
    let mut sim = Sim::new(0x25_5880, 16);
    sim.activate(0);
    let (_, row) = actor_with_order(
        &mut sim,
        Order::special_anim(SpecialAnimType::Exit, 9, 10),
    );
    sim.world.orders_mut(row).push(Order {
        kind: OrderIndex::Guard,
        x: 384,
        y: 576,
        ..Order::default()
    });
    sim.paths[row].push(PathData {
        to_x: 768,
        to_y: 960,
        tolerance: 24,
        flags: 0,
    });

    let trace = sim.do_frame();

    assert_eq!(trace.steps[14], StepRun::Executed);
    assert_eq!(sim.world.orders(row).order_type(), OrderIndex::Guard);
    assert!(sim.paths[row].is_empty());
    assert_eq!(sim.cover.special_anim_completed, 1);
    assert_eq!(sim.cover.special_anim_host_refused, 0);
    assert_eq!(sim.cover.special_anim_malformed, 0);
}

#[test]
fn real_frame_reaches_the_host_free_special_unit_no_op() {
    let mut sim = Sim::new(0x25_5880, 16);
    sim.activate(0);
    let (_, row) = actor_with_order(
        &mut sim,
        Order::special_anim(SpecialAnimType::Unit, 9, 10),
    );
    let before = *sim.world.orders(row).current().unwrap();

    sim.do_frame();

    assert_eq!(sim.world.orders(row).current(), Some(&before));
    assert_eq!(sim.cover.special_anim_working, 1);
    assert_eq!(sim.cover.special_anim_host_refused, 0);
}

#[test]
fn reached_external_tails_refuse_before_order_path_or_rng_mutation() {
    for mut order in [
        Order::special_anim(SpecialAnimType::Enter, 9, 10),
        Order::special_anim(SpecialAnimType::Exit, 9, 10),
    ] {
        if order.special_anim.unwrap().special_type == SpecialAnimType::Exit {
            let payload = order.special_anim.as_mut().unwrap();
            payload.ox = 3;
            payload.whom = 0;
            payload.data3 = 384;
            payload.data4 = 576;
        }
        let mut sim = Sim::new(0x25_5880, 16);
        sim.activate(0);
        // Keep the ordinary market roll from consuming the shared stream; with cycle 1 none
        // of the six resources is on its modulo-8 phase, so any change below would belong to
        // this reached SPECIAL_ANIM arm.
        sim.world.frame = 1;
        sim.market.cycle = 1;
        let (_, row) = actor_with_order(&mut sim, order);
        sim.paths[row].push(PathData {
            to_x: 768,
            to_y: 960,
            tolerance: 24,
            flags: 0,
        });
        let order_before = *sim.world.orders(row).current().unwrap();
        let path_before = sim.paths[row].clone();
        let rng_before = sim.world.random.state();

        sim.do_frame();

        assert_eq!(sim.world.orders(row).current(), Some(&order_before));
        assert_eq!(sim.paths[row], path_before);
        assert_eq!(sim.world.random.state(), rng_before);
        assert_eq!(sim.cover.special_anim_host_refused, 1);
        assert_eq!(sim.cover.special_anim_completed, 0);
    }
}

#[test]
fn saved_walked_payload_is_the_one_the_real_frame_executes() {
    let mut original = Sim::new(0x25_5880, 16);
    let (_, original_row) = actor_with_order(
        &mut original,
        Order::special_anim(SpecialAnimType::Exit, 41, 43),
    );
    let walked = original.world.orders(original_row).current().unwrap().special_anim;
    let bytes = save_sim(&original).unwrap();
    let mut loaded = load_sim(&bytes).unwrap();
    let loaded_row = 0;
    assert_eq!(loaded.world.orders(loaded_row).current().unwrap().special_anim, walked);

    loaded.activate(0);
    loaded.do_frame();

    assert!(loaded.world.orders(loaded_row).is_empty());
    assert_eq!(loaded.cover.special_anim_completed, 1);
}
