// SPDX-License-Identifier: GPL-3.0-or-later

//! Mutation-sensitive integration gates for the recovered naval/air transaction seams.
//!
//! These expectations are finite transcription checks, not retail differential evidence.
//! Their purpose is to ensure integration callers cannot bypass mandatory allocation,
//! capacity, host-search, order, or RNG inputs while the Tier-C ports are being wired.

use don_sim::rng::Random;
use don_sim::systems::air::{
    air_turn_speed, check_fuel_order_transaction, num_aircraft_here, select_nearest_air_host,
    AirBuildingHostCandidate, AirOrderWalk, AirTypeData, AirUnitHostCandidate,
    AircraftQueueAccounting, FuelVerdict, HostedAircraft, DOMAIN_AIR, DOMAIN_LAND,
    MIN_AIR_TURN_STEP,
};
use don_sim::systems::naval::{
    board_order_transaction, BoardStep, Dock, DockGullDestroyEffect, DockGullInitEffect,
    DockGullSpawnRequest, Docks, LeaderNaval, TargetRef, DOCK_FLAG_IN_USE, DOCK_GULL_TYPE_INDEX,
    NATURE_OWNER_SLOT, TILE,
};

#[test]
fn dock_init_sequences_registry_spawn_rng_and_gull_calls() {
    let mut docks = Docks::new();
    let mut leader = LeaderNaval::default();
    let mut stale = Dock::default();
    stale.dock_flags = 0xfe;
    docks.lists[0].add(stale);
    leader.dock_mark = 1;

    let seed = 0x1234_5678;
    let mut rng = Random::new(seed);
    let mut spawn_seen: Option<DockGullSpawnRequest> = None;
    let mut finish_seen: Option<DockGullInitEffect> = None;
    let slot = docks.init_dock_transaction(
        &mut leader,
        0,
        17,
        (1_920, 3_840),
        6,
        &mut rng,
        |request| {
            spawn_seen = Some(request);
            23
        },
        |effect| finish_seen = Some(effect),
    );

    assert_eq!(slot, 0, "the first free marked slot must be reused");
    assert_eq!(leader.reg_docks[6], 1);
    let dock = docks.lists[0].get(0).unwrap();
    assert_eq!(dock.dock_flags, DOCK_FLAG_IN_USE, "init is mov 1, not OR 1");
    assert_eq!((dock.o, dock.reg, dock.gull_o, dock.who), (17, 6, 23, 0));
    assert_eq!(
        spawn_seen,
        Some(DockGullSpawnRequest {
            who: NATURE_OWNER_SLOT,
            type_index: DOCK_GULL_TYPE_INDEX,
            x: 1_920 - TILE,
            y: 3_840 - TILE,
            arg5: -1,
            arg6: -1,
            arg7: -1,
        })
    );
    let finish = finish_seen.expect("successful spawn must finish the gull");
    assert_eq!(finish.gull_o, 23);
    assert_eq!((finish.angle_steps, finish.angle_mode), (7, 0));
    assert_eq!((finish.building_o, finish.building_who), (17, 0));
    assert_eq!((finish.target_o, finish.target_who), (-1, -1));
    assert_eq!(
        (finish.strafe_flag, finish.queue_pos, finish.trailing),
        (1, 2, 0)
    );
    assert_eq!(
        rng.state(),
        seed.wrapping_mul(Random::MUL).wrapping_add(Random::ADD),
        "a successful gull consumes exactly one main-stream draw"
    );
}

#[test]
fn failed_gull_allocation_is_a_zero_draw_transaction() {
    let mut docks = Docks::new();
    let mut leader = LeaderNaval::default();
    let mut rng = Random::new(91);
    let mut finished = false;
    let slot = docks.init_dock_transaction(
        &mut leader,
        1,
        8,
        (TILE, TILE),
        3,
        &mut rng,
        |_| -1,
        |_| finished = true,
    );
    assert_eq!(slot, 0);
    assert_eq!(rng.state(), 91);
    assert!(!finished);
    assert_eq!(docks.lists[1].get(0).unwrap().gull_o, -1);
    assert_eq!(
        leader.reg_docks[3], 1,
        "registry mutation precedes allocation"
    );
}

#[test]
fn dock_close_requires_the_measured_building_active_input() {
    let mut docks = Docks::new();
    let mut leader = LeaderNaval::default();
    let mut rng = Random::new(7);
    let slot =
        docks.init_dock_transaction(&mut leader, 2, 31, (500, 700), 9, &mut rng, |_| 12, |_| {});
    let mut destroyed: Option<DockGullDestroyEffect> = None;
    docks.close_dock_transaction(&mut leader, 2, slot, false, |effect| {
        destroyed = Some(effect)
    });
    assert_eq!(
        leader.reg_docks[9], 1,
        "retail does not decrement when the building active-bit probe fails"
    );
    assert_eq!(
        destroyed,
        Some(DockGullDestroyEffect {
            who: NATURE_OWNER_SLOT,
            gull_o: 12,
            args: (0, -1, 0),
        })
    );
    let dock = docks.lists[2].get(slot as usize).unwrap();
    assert_eq!((dock.o, dock.reg, dock.who), (-1, 0, 0xff));
    assert_eq!(leader.dock_mark, 0);
}

#[test]
fn board_executor_exposes_ordered_kill_then_containment_effects() {
    let ship = TargetRef {
        ox: 5,
        whom: 2,
        uid: 77,
    };
    let rendezvous = board_order_transaction(ship, true, true);
    assert_eq!(rendezvous.step, BoardStep::Rendezvous);
    assert_eq!(rendezvous.set_anim, (0, 0, 1));
    assert_eq!(rendezvous.kill_current_order, None);
    assert_eq!(rendezvous.go_inside, None);

    let load = board_order_transaction(ship, false, true);
    assert_eq!(load.step, BoardStep::GoInside);
    assert_eq!(load.kill_current_order, Some(0));
    assert_eq!(load.go_inside, Some((ship, 0)));

    let refused = board_order_transaction(ship, false, false);
    assert_eq!(refused.step, BoardStep::Abandon);
    assert_eq!(refused.kill_current_order, Some(0));
    assert_eq!(refused.go_inside, None);
}

#[test]
fn aircraft_queue_accounting_uses_both_measured_counter_operands() {
    let objects = [
        HostedAircraft {
            active: true,
            domain: DOMAIN_AIR,
            home_base_o: 4,
            home_base_who: 1,
        },
        HostedAircraft {
            active: true,
            domain: DOMAIN_LAND,
            home_base_o: 4,
            home_base_who: 1,
        },
    ];
    assert_eq!(
        num_aircraft_here(
            4,
            1,
            &objects,
            AircraftQueueAccounting::Build {
                also_count_queue: true,
                count_kind2_type0: 8,
                count_kind1_helicopter: 3,
            },
        ),
        6
    );
    assert_eq!(
        num_aircraft_here(
            4,
            1,
            &objects,
            AircraftQueueAccounting::Build {
                also_count_queue: true,
                count_kind2_type0: 8,
                count_kind1_helicopter: 4,
            },
        ),
        5,
        "mutating the second retail operand must change the capacity result"
    );
    assert_eq!(
        num_aircraft_here(
            4,
            1,
            &objects,
            AircraftQueueAccounting::Build {
                also_count_queue: false,
                count_kind2_type0: 800,
                count_kind1_helicopter: -300,
            },
        ),
        1,
        "also_count_queue short-circuits the build counter arm"
    );
}

#[test]
fn fuel_host_scan_preserves_retail_list_and_tie_order() {
    let buildings = [
        AirBuildingHostCandidate {
            x: 100,
            y: 0,
            host_ready: true,
            can_carry_aircraft: true,
        },
        AirBuildingHostCandidate {
            x: 10,
            y: 0,
            host_ready: false,
            can_carry_aircraft: true,
        },
    ];
    let units = [AirUnitHostCandidate {
        x: 100,
        y: 0,
        active: true,
        can_carry_aircraft: true,
    }];
    let selected = select_nearest_air_host(3, (0, 0), &buildings, &units).unwrap();
    assert_eq!(
        (selected.o, selected.who, selected.distance),
        (2000, 3, 100)
    );

    let mut mutated = buildings;
    mutated[0].host_ready = false;
    let selected = select_nearest_air_host(3, (0, 0), &mutated, &units).unwrap();
    assert_eq!(
        selected.o, 0,
        "the equal-distance unit wins only after the building is vetoed"
    );
}

#[test]
fn fuel_order_transaction_latches_and_repoints_walked_state() {
    let bomber = AirTypeData {
        domain: DOMAIN_AIR,
        mana: 500,
        ..AirTypeData::default()
    };
    let mut order = AirOrderWalk {
        oxx: 9,
        whose: 1,
        returning: 0,
        ..AirOrderWalk::default()
    };
    let buildings = [AirBuildingHostCandidate {
        x: 48,
        y: 48,
        host_ready: true,
        can_carry_aircraft: true,
    }];
    let verdict =
        check_fuel_order_transaction(&bomber, &mut order, 0, false, 1, (0, 0), &buildings, &[]);
    assert_eq!(verdict, FuelVerdict::ReturnTo { o: 2000, who: 1 });
    assert_eq!((order.returning, order.oxx, order.whose), (1, 2000, 1));

    let mut no_host = AirOrderWalk {
        oxx: -1,
        whose: -1,
        returning: 1,
        ..AirOrderWalk::default()
    };
    assert_eq!(
        check_fuel_order_transaction(&bomber, &mut no_host, 0, false, 1, (0, 0), &[], &[]),
        FuelVerdict::Crash
    );
}

#[test]
fn air_turn_speed_keeps_the_measured_sign_gate_and_helicopter_branch() {
    let raw = 0x1000_0000;
    let base = (raw >> 8) * 256;
    assert_eq!(
        air_turn_speed(raw, 256, false, 1, true, 0),
        base,
        "the bypass arm returns the raw scaled type speed"
    );
    assert_eq!(
        air_turn_speed(raw, 256, false, 1, false, -2),
        (base / 55) * 2
    );
    assert_eq!(
        air_turn_speed(raw, 256, false, -1, false, -2),
        MIN_AIR_TURN_STEP,
        "opposing bank and requested-turn signs take the literal early return"
    );
    assert_eq!(
        air_turn_speed(45 << 8, 256, true, 1, true, 0),
        45 * 256,
        "helicopters bypass the final fixed-wing minimum clamp"
    );
    assert_eq!(
        air_turn_speed(45 << 8, 256, false, 1, true, 0),
        MIN_AIR_TURN_STEP
    );
}
