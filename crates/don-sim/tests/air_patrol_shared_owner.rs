// SPDX-License-Identifier: GPL-3.0-or-later
//! Canonical AIR_PATROL Order -> dispatcher -> DoNSave v13 ownership.
//!
//! This deliberately stops before claiming opcode-11/36 execution: `Sim::unit_work` still has
//! no production AirPatrolHost, so the installed node must remain unchanged by a frame.

use don_sim::order::{Order, OrderIndex};
use don_sim::systems::{
    air_runtime_authority::{AirOrderPayload, AirPatrolOrderPayload, WalkedCoordArray},
    order_dispatch,
    save_load::{load_sim, save_sim, SaveError},
};
use don_sim::tick::Sim;

fn payload() -> AirPatrolOrderPayload {
    AirPatrolOrderPayload {
        x: WalkedCoordArray {
            increment: -7,
            flags: 0x08,
            values: vec![100, 200, 300],
        },
        y: WalkedCoordArray {
            increment: 11,
            flags: 0x10,
            values: vec![-400, 500, 600],
        },
        waypoint: 2,
        air: AirOrderPayload {
            home_o: 2_015,
            home_who: 0,
            cruising_alt: 0x640,
            sharp_turn: 1,
            old: 2,
            returning: 0,
        },
    }
}

#[test]
fn dispatcher_and_v13_round_trip_every_walked_air_patrol_field() {
    let mut sim = Sim::new(0x1234, 4);
    let handle = sim.spawn_unit(0, 17, 96, 96, 4).unwrap();
    let row = sim.world.row_of(handle).unwrap();
    let installed = Order::air_patrol(payload(), true).unwrap();
    sim.world.orders_mut(row).replace(installed.clone());

    let executable = order_dispatch::adopt(sim.world.orders(row));
    let mut published = don_sim::order::OrderList::new();
    order_dispatch::publish(&executable, &mut published);
    assert_eq!(published.current(), Some(&installed));

    let bytes = save_sim(&sim).unwrap();
    let mut loaded = load_sim(&bytes).unwrap();
    let mut control = load_sim(&bytes).unwrap();
    control.world.orders_mut(row).clear();
    assert_eq!(loaded.world.orders(row).current(), Some(&installed));
    assert_eq!(save_sim(&loaded).unwrap(), bytes);

    // This is an explicit red integration gate, not simulated physics: until tick owns the
    // mandatory air host, a frame cannot mutate or retire the canonical AIR_PATROL payload.
    loaded.do_frame();
    control.do_frame();
    assert_eq!(loaded.world.orders(row).current(), Some(&installed));
    assert_eq!(loaded.world.random.state(), control.world.random.state());
}

#[test]
fn save_rejects_missing_or_foreign_air_patrol_payload_without_mutation() {
    let mut sim = Sim::new(7, 2);
    let handle = sim.spawn_unit(0, 1, 0, 0, 0).unwrap();
    let row = sim.world.row_of(handle).unwrap();

    let missing = Order {
        kind: OrderIndex::AirPatrol,
        ..Order::default()
    };
    sim.world.orders_mut(row).replace(missing.clone());
    assert_eq!(
        save_sim(&sim),
        Err(SaveError::Invalid("missing AIR_PATROL payload"))
    );
    assert_eq!(sim.world.orders(row).current(), Some(&missing));

    let foreign = Order {
        kind: OrderIndex::Attack,
        air_patrol: Some(payload()),
        ..Order::default()
    };
    sim.world.orders_mut(row).replace(foreign.clone());
    assert_eq!(
        save_sim(&sim),
        Err(SaveError::Invalid(
            "AIR_PATROL payload on foreign order kind"
        ))
    );
    assert_eq!(sim.world.orders(row).current(), Some(&foreign));
}
