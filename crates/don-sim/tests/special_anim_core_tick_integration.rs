// SPDX-License-Identifier: GPL-3.0-or-later

//! Real step-14 reachability for the recovered SPECIAL_ANIM dispatcher.

use don_sim::order::{Order, OrderIndex, SpecialAnimType};
use don_sim::objects::{BUILD_BAND_BASE, WALL_BAND_BASE};
use don_sim::systems::movement::PathData;
use don_sim::systems::production::{self, BuildData};
use don_sim::systems::special_anim_executor::AIRBASE_TYPE;
use don_sim::systems::save_load::{load_sim, save_sim};
use don_sim::systems::walls::WallState;
use don_sim::tick::{Sim, StepRun};

fn actor_with_order(sim: &mut Sim, order: Order) -> (don_sim::world::Handle, usize) {
    let actor = sim.spawn_unit(0, 1, 192, 192, 1).unwrap();
    assert!(sim.issue(actor, order));
    let row = sim.world.row_of(actor).unwrap();
    (actor, row)
}

fn exit_from_target(target_o: i32) -> Order {
    let mut order = Order::special_anim(SpecialAnimType::Exit, 9, 10);
    let payload = order.special_anim.as_mut().unwrap();
    payload.ox = target_o;
    payload.whom = 0;
    payload.data3 = 384;
    payload.data4 = 576;
    order
}

fn savable_build(uid: u16) -> BuildData {
    let mut build = BuildData {
        flags: production::flag::VALID,
        uid,
        gather_down: -1,
        city: -1,
        city_down: -1,
        wonder: -1,
        dock: -1,
        attack_ox: -1,
        attack_whom: -1,
        ..BuildData::default()
    };
    build.other[production::off::OBJECT_ID..production::off::OBJECT_ID + 2]
        .copy_from_slice(&(BUILD_BAND_BASE as i16).to_le_bytes());
    build.other[0x28..0x2a].copy_from_slice(&(-1_i16).to_le_bytes());
    build
}

#[test]
fn real_frame_commits_object_free_exit_as_one_queue_path_transaction() {
    let mut sim = Sim::new(0x25_5880, 16);
    sim.activate(0);
    let (_, row) = actor_with_order(&mut sim, Order::special_anim(SpecialAnimType::Exit, 9, 10));
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
    let (_, row) = actor_with_order(&mut sim, Order::special_anim(SpecialAnimType::Unit, 9, 10));
    let before = *sim.world.orders(row).current().unwrap();

    sim.do_frame();

    assert_eq!(sim.world.orders(row).current(), Some(&before));
    assert_eq!(sim.cover.special_anim_working, 1);
    assert_eq!(sim.cover.special_anim_host_refused, 0);
}

#[test]
fn canonical_build_target_refuses_without_the_non_strict_type_relation_owner() {
    let mut sim = Sim::new(0x25_5880, 16);
    sim.activate(0);
    // Keep the ordinary market phase off the shared RNG so this branch proves it consumes none.
    sim.world.frame = 1;
    sim.market.cycle = 1;

    let build_row = sim.spawn_build(
        0,
        BuildData {
            flags: 1,
            uid: 0x4711,
            ..BuildData::default()
        },
    );
    sim.production_runtime.register_build(build_row, 0x120);

    let (_, row) = actor_with_order(&mut sim, exit_from_target(BUILD_BAND_BASE as i32));
    sim.world.orders_mut(row).push(Order {
        kind: OrderIndex::Guard,
        ..Order::default()
    });
    sim.paths[row].push(PathData {
        to_x: 768,
        to_y: 960,
        tolerance: 24,
        flags: 0,
    });
    let order_before = sim.world.orders(row).clone();
    let path_before = sim.paths[row].clone();
    let rng_before = sim.world.random.state();

    sim.do_frame();

    assert_eq!(sim.world.orders(row), &order_before);
    assert_eq!(sim.paths[row], path_before);
    assert_eq!(sim.world.random.state(), rng_before);
    assert_eq!(sim.cover.special_anim_completed, 0);
    assert_eq!(sim.cover.special_anim_host_refused, 1);
}

#[test]
fn canonical_unit_and_wall_targets_also_refuse_without_relation_rows() {
    let mut unit_sim = Sim::new(0x25_5880, 16);
    unit_sim.activate(0);
    unit_sim.world.frame = 1;
    unit_sim.market.cycle = 1;
    let target = unit_sim.spawn_unit(0, 0x121, 192, 192, 1).unwrap();
    let target_row = unit_sim.world.row_of(target).unwrap();
    let target_o = i32::from(unit_sim.world.units.o()[target_row]);
    let (_, actor_row) = actor_with_order(&mut unit_sim, exit_from_target(target_o));
    let unit_order_before = *unit_sim.world.orders(actor_row).current().unwrap();

    unit_sim.do_frame();

    assert_eq!(
        unit_sim.world.orders(actor_row).current(),
        Some(&unit_order_before)
    );
    assert_eq!(unit_sim.cover.special_anim_completed, 0);
    assert_eq!(unit_sim.cover.special_anim_host_refused, 1);

    let mut wall_sim = Sim::new(0x25_5880, 16);
    wall_sim.activate(0);
    wall_sim.world.frame = 1;
    wall_sim.market.cycle = 1;
    wall_sim.spawn_wall(
        0,
        WallState {
            flags: 1,
            uid: 0x301,
            ptype: Some(0x122),
            ..WallState::default()
        },
    );
    let (_, actor_row) = actor_with_order(&mut wall_sim, exit_from_target(WALL_BAND_BASE as i32));
    let wall_order_before = *wall_sim.world.orders(actor_row).current().unwrap();

    wall_sim.do_frame();

    assert_eq!(
        wall_sim.world.orders(actor_row).current(),
        Some(&wall_order_before)
    );
    assert_eq!(wall_sim.cover.special_anim_completed, 0);
    assert_eq!(wall_sim.cover.special_anim_host_refused, 1);
}

#[test]
fn reached_canonical_airbase_exit_refuses_before_any_local_or_rng_write() {
    let mut sim = Sim::new(0x25_5880, 16);
    sim.activate(0);
    sim.world.frame = 1;
    sim.market.cycle = 1;
    let build_row = sim.spawn_build(
        0,
        BuildData {
            flags: 1,
            uid: 0x1bf,
            ..BuildData::default()
        },
    );
    sim.production_runtime
        .register_build(build_row, AIRBASE_TYPE);

    let (_, row) = actor_with_order(&mut sim, exit_from_target(BUILD_BAND_BASE as i32));
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

#[test]
fn target_type_missing_after_save_load_refuses_instead_of_defaulting_non_airbase() {
    let mut original = Sim::new(0x25_5880, 16);
    original.world.frame = 1;
    original.market.cycle = 1;
    let build_row = original.spawn_build(0, savable_build(0x5225));
    original.production_runtime.register_build(build_row, 0x120);
    let (_, actor_row) = actor_with_order(
        &mut original,
        exit_from_target(BUILD_BAND_BASE as i32),
    );
    let order_before = *original.world.orders(actor_row).current().unwrap();
    let bytes = save_sim(&original).unwrap();

    let mut loaded = load_sim(&bytes).unwrap();
    assert!(loaded.production_runtime.build_types.is_empty());
    let rng_before = loaded.world.random.state();

    loaded.do_frame();

    assert_eq!(loaded.world.orders(actor_row).current(), Some(&order_before));
    assert_eq!(loaded.world.random.state(), rng_before);
    assert_eq!(loaded.cover.special_anim_completed, 0);
    assert_eq!(loaded.cover.special_anim_host_refused, 1);
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
    let walked = original
        .world
        .orders(original_row)
        .current()
        .unwrap()
        .special_anim;
    let bytes = save_sim(&original).unwrap();
    let mut loaded = load_sim(&bytes).unwrap();
    let loaded_row = 0;
    assert_eq!(
        loaded
            .world
            .orders(loaded_row)
            .current()
            .unwrap()
            .special_anim,
        walked
    );

    loaded.activate(0);
    loaded.do_frame();

    assert!(loaded.world.orders(loaded_row).is_empty());
    assert_eq!(loaded.cover.special_anim_completed, 1);
}
