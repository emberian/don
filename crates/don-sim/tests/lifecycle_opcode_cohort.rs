// SPDX-License-Identifier: GPL-3.0-or-later

use don_sim::command::tail_command_transactions::{
    decode_tail_command,
    lifecycle::{LifecycleCall, LifecycleEffect, PLAYER_LEAVE_SCAN_REQUIRED, PLAYER_PRESENT},
};
use don_sim::command::{Bridge, Fleet, ObjectTable, Package};
use don_sim::systems::tech_cities::CityRecord;
use don_sim::systems::victory_score::{leader_flag, Elimination};
use don_sim::tick::lifecycle_host::PlayerTable;
use don_sim::tick::lifecycle_host::{SimTailError, SimTailOutcome};
use don_sim::tick::lifecycle_opcode_cohort::{
    find_capital, CapitalIdentity, CapitalPass, FindCapitalError, FindCapitalRequest,
    LifecycleFleet, CITY_CAPITAL, CITY_VALID,
};
use don_sim::tick::Sim;

fn sim() -> Sim {
    let mut sim = Sim::new(0x7071, 8);
    sim.activate(0);
    sim.activate(1);
    sim
}

fn active_city(flags: u16, former: u8) -> CityRecord {
    CityRecord {
        city_flags: CITY_VALID | flags,
        was_capital_flags: former,
        ..CityRecord::default()
    }
}

fn seated_capital_sim(seed: u64, departing: usize) -> Sim {
    let mut sim = Sim::new(seed, 8);
    sim.activate(0);
    sim.activate(1);
    let mut players = PlayerTable::new();
    let seated = PLAYER_PRESENT | PLAYER_LEAVE_SCAN_REQUIRED;
    players.seat(0, seated, 0, 0);
    players.seat(1, seated, 1, 1);
    players.console_play = 0;
    players.console_who = 0;
    sim.players = Some(players);
    sim.vic_match.options.elimination = Elimination::Capital as u8;
    sim.vic_leaders.slots[departing].lost_capital_timer = 7;
    sim
}

fn resign_wire(play: i32) -> Vec<u8> {
    let mut wire = vec![70];
    wire.extend_from_slice(&play.to_le_bytes());
    wire
}

fn quit_wire(play: i32, replay: u8, system_quit: u8) -> Vec<u8> {
    let mut wire = vec![71];
    wire.extend_from_slice(&play.to_le_bytes());
    wire.extend_from_slice(&[replay, system_quit]);
    wire
}

#[test]
fn own_capital_precedes_every_former_capital_and_honours_the_pair_skip() {
    let mut sim = sim();
    sim.cities.city_mark[1] = 3;
    sim.cities.slots[1][1] = active_city(CITY_CAPITAL, 0);
    sim.cities.slots[1][2] = active_city(CITY_CAPITAL, 0);
    sim.cities.city_mark[0] = 1;
    sim.cities.slots[0][0] = active_city(0, 1 << 1);

    let first = find_capital(
        &sim.cities,
        &sim.vic_leaders,
        FindCapitalRequest {
            who: 1,
            skip_city: -1,
            skip_who: -1,
        },
    )
    .unwrap();
    assert_eq!(first.pass, CapitalPass::Own);
    assert_eq!(first.result, CapitalIdentity { city: 1, who: 1 });
    assert_eq!(first.visited, 2);
    assert!(first.validates());

    let skipped = find_capital(
        &sim.cities,
        &sim.vic_leaders,
        FindCapitalRequest {
            who: 1,
            skip_city: 1,
            skip_who: 1,
        },
    )
    .unwrap();
    assert_eq!(skipped.result, CapitalIdentity { city: 2, who: 1 });
}

#[test]
fn former_capital_pass_is_slot_ordered_and_uses_the_departing_leader_guard() {
    let mut sim = sim();
    sim.cities.city_mark[0] = 1;
    sim.cities.city_mark[2] = 2;
    sim.cities.slots[0][0] = active_city(0, 1 << 1);
    sim.cities.slots[2][0] = active_city(0, 0);
    sim.cities.slots[2][1] = active_city(0, 1 << 1);

    let receipt = find_capital(
        &sim.cities,
        &sim.vic_leaders,
        FindCapitalRequest {
            who: 1,
            skip_city: 0,
            skip_who: 0,
        },
    )
    .unwrap();
    assert_eq!(receipt.pass, CapitalPass::Former);
    assert_eq!(receipt.result, CapitalIdentity { city: 1, who: 2 });
    assert!(receipt.departing_leader_active_guard);

    // Capstone proves retail rereads leaders[departing], not leaders[candidate], inside
    // the candidate loop. Making candidate 2 invalid therefore cannot suppress the hit.
    sim.vic_leaders.slots[2].leader_flags = 0;
    assert_eq!(
        find_capital(&sim.cities, &sim.vic_leaders, receipt.request)
            .unwrap()
            .result,
        receipt.result
    );

    sim.vic_leaders.slots[1].leader_flags = 0;
    let fallback = find_capital(&sim.cities, &sim.vic_leaders, receipt.request).unwrap();
    assert_eq!(fallback.pass, CapitalPass::Fallback);
    assert_eq!(fallback.result, CapitalIdentity { city: -1, who: 1 });
}

#[test]
fn malformed_city_mark_refuses_before_lookup() {
    let mut sim = sim();
    sim.cities.city_mark[6] = 21;
    assert_eq!(
        find_capital(
            &sim.cities,
            &sim.vic_leaders,
            FindCapitalRequest {
                who: 1,
                skip_city: -1,
                skip_who: -1,
            }
        ),
        Err(FindCapitalError::CityMarkOutOfRange {
            who: 6,
            mark: 21,
            installed: 20,
        })
    );
}

#[test]
fn capital_elimination_boundary_resolves_to_the_exact_defeat_argument() {
    let mut sim = sim();
    let mut players = PlayerTable::new();
    let seated = PLAYER_PRESENT | PLAYER_LEAVE_SCAN_REQUIRED;
    players.seat(0, seated, 0, 0);
    players.seat(1, seated, 1, 1);
    players.console_play = 0;
    players.console_who = 0;
    sim.players = Some(players);
    sim.vic_match.options.elimination = Elimination::Capital as u8;
    sim.vic_leaders.slots[1].lost_capital_timer = 7;
    sim.cities.city_mark[3] = 1;
    sim.cities.slots[3][0] = active_city(0, 1 << 1);

    let mut wire = [0u8; 5];
    wire[0] = 70;
    wire[1..].copy_from_slice(&1i32.to_le_bytes());
    let request = decode_tail_command(&wire).unwrap();
    let facts = sim.tail_command_facts(&request);
    let decision =
        don_sim::command::tail_command_transactions::plan_tail_command(&request, &facts).unwrap();
    let don_sim::command::tail_command_transactions::TailDecision::Boundary(boundary) = decision
    else {
        panic!("capital elimination must reach the typed boundary");
    };
    let don_sim::command::tail_command_transactions::TailOpenBoundary::PlayerLifecycle {
        request: lifecycle_request,
        plan,
    } = boundary.boundary
    else {
        panic!("expected lifecycle boundary");
    };
    let resolved = don_sim::tick::lifecycle_opcode_cohort::resolve_capital_boundary(
        &sim.cities,
        &sim.vic_leaders,
        match &facts {
            don_sim::command::tail_command_transactions::TailCommandFacts::PlayerLifecycle {
                image,
                ..
            } => image,
            _ => unreachable!(),
        },
        lifecycle_request,
        &plan,
    )
    .unwrap();
    assert!(resolved.validates());
    assert!(resolved.resolved.boundary.is_none());
    assert!(matches!(
        resolved.resolved.effects.last(),
        Some(LifecycleEffect::Call(LifecycleCall::LeaderDefeat {
            who: 1,
            defeat_type: 1,
            arg: 3,
            instant: 0,
        }))
    ));
    assert!(!sim.vic_leaders.slots[1].flag(leader_flag::DEFEATED));
}

#[test]
fn real_bridge_resign_discharges_a_present_former_capital_and_recomputes_receipt() {
    let mut sim = seated_capital_sim(0x7000, 1);
    sim.cities.city_mark[3] = 1;
    sim.cities.slots[3][0] = active_city(0, 1 << 1);
    let mut objects = ObjectTable::new(8);
    let mut bridge = Bridge::new();
    let mut package = Package::new(1, 0);
    {
        let mut fleet = LifecycleFleet::new(&mut objects, &mut sim);
        bridge
            .process_all(&mut package, &resign_wire(1), &mut fleet)
            .unwrap();
    }

    let records = bridge.take_discharged_tail_command_receipts();
    assert_eq!(records.len(), 1);
    assert!(records[0].valid);
    assert!(records[0].observed.validates());
    assert!(sim.vic_leaders.slots[1].flag(leader_flag::DEFEATED));
    assert_eq!(sim.vic_leaders.slots[1].defeated_by, 3);
    assert_eq!(sim.vic_leaders.slots[1].defeat_type, 1);

    let mut forged = records[0].observed.clone();
    let SimTailOutcome::Committed(committed) = &mut forged.outcome else {
        panic!("real Bridge must retain a committed Sim receipt");
    };
    committed
        .capital_resolution
        .as_mut()
        .unwrap()
        .lookup
        .result
        .who = 2;
    assert!(
        !forged.validates(),
        "tampered capital proof must not recompute"
    );
}

#[test]
fn tampered_capital_proof_rolls_back_before_any_lifecycle_mutation() {
    let mut sim = seated_capital_sim(0x7002, 1);
    sim.cities.city_mark[3] = 1;
    sim.cities.slots[3][0] = active_city(0, 1 << 1);
    let request = decode_tail_command(&resign_wire(1)).unwrap();
    let mut forged = find_capital(
        &sim.cities,
        &sim.vic_leaders,
        FindCapitalRequest {
            who: 1,
            skip_city: -1,
            skip_who: -1,
        },
    )
    .unwrap();
    forged.result.who = 2;

    let before_players = sim.players.clone();
    let mut before_leaders = Vec::new();
    sim.vic_leaders.walk_bytes(&mut before_leaders);
    let mut before_match = Vec::new();
    sim.vic_match.walk_bytes(&mut before_match);
    let receipt = sim.apply_tail_command_transaction_with_capital_lookup(&request, forged);
    assert_eq!(
        receipt.outcome,
        SimTailOutcome::Refused(SimTailError::CapitalProofMismatch)
    );
    assert_eq!(sim.players, before_players);
    let mut after_leaders = Vec::new();
    sim.vic_leaders.walk_bytes(&mut after_leaders);
    let mut after_match = Vec::new();
    sim.vic_match.walk_bytes(&mut after_match);
    assert_eq!(after_leaders, before_leaders);
    assert_eq!(after_match, before_match);
    assert!(!sim.vic_leaders.slots[1].flag(leader_flag::DEFEATED));
}

#[test]
fn real_bridge_quit_discharges_the_no_capital_fallback() {
    let mut sim = seated_capital_sim(0x7100, 0);
    let mut objects = ObjectTable::new(8);
    let mut bridge = Bridge::new();
    let mut package = Package::new(0, 0);
    {
        let mut fleet = LifecycleFleet::new(&mut objects, &mut sim);
        bridge
            .process_all(&mut package, &quit_wire(0, 1, 1), &mut fleet)
            .unwrap();
    }
    let records = bridge.take_discharged_tail_command_receipts();
    assert_eq!(records.len(), 1);
    assert!(records[0].valid);
    assert_eq!(sim.vic_leaders.slots[0].defeated_by, 0);
    assert_eq!(sim.players.as_ref().unwrap().playing, 0);
}

#[test]
fn changed_city_pool_between_bridge_facts_and_commit_rolls_back() {
    let mut sim = seated_capital_sim(0x7001, 1);
    sim.cities.city_mark[3] = 1;
    sim.cities.slots[3][0] = active_city(0, 1 << 1);
    let before_players = sim.players.clone();
    let mut before_leaders = Vec::new();
    sim.vic_leaders.walk_bytes(&mut before_leaders);
    let mut before_match = Vec::new();
    sim.vic_match.walk_bytes(&mut before_match);
    let request = decode_tail_command(&resign_wire(1)).unwrap();
    let mut objects = ObjectTable::new(8);
    let mut fleet = LifecycleFleet::new(&mut objects, &mut sim);
    let facts = Fleet::tail_command_facts(&fleet, &request).unwrap();

    // This is a valid alternate CityPool, not corruption. The CAS must still refuse it:
    // choosing a different former capital from facts captured by the Bridge would make the
    // command proof describe a before-image it did not execute.
    fleet.sim.cities.slots[3][0].was_capital_flags = 0;
    fleet.sim.cities.city_mark[4] = 1;
    fleet.sim.cities.slots[4][0] = active_city(0, 1 << 1);
    let receipt =
        Fleet::apply_discharged_tail_command_transaction(&mut fleet, request, facts).unwrap();
    assert_eq!(
        receipt.outcome,
        SimTailOutcome::Refused(SimTailError::StaleCapitalImage)
    );
    assert_eq!(fleet.sim.players, before_players);
    let mut after_leaders = Vec::new();
    fleet.sim.vic_leaders.walk_bytes(&mut after_leaders);
    let mut after_match = Vec::new();
    fleet.sim.vic_match.walk_bytes(&mut after_match);
    assert_eq!(after_leaders, before_leaders);
    assert_eq!(after_match, before_match);
    assert!(!fleet.sim.vic_leaders.slots[1].flag(leader_flag::DEFEATED));
}
