// SPDX-License-Identifier: GPL-3.0-or-later

use don_sim::systems::canonical_diplomacy_host::DiplomacyInstalledFacts;
use don_sim::systems::canonical_diplomacy_runtime::{
    CanonicalDiplomacyRuntimeError, CanonicalDiplomacyStatus,
};
use don_sim::systems::leader_set_diplo::Relation;
use don_sim::systems::player_setup::ManualPlayerSetup;
use don_sim::systems::save_load::{load_sim, save_sim};
use don_sim::tick::lifecycle_host::PlayerTable;
use don_sim::tick::Sim;

const RETAIL_DECLARE_2_5_WAR: [u8; 13] = [38, 2, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0];

fn declare(sender: i32, target: i32, treaty: i32) -> [u8; 13] {
    let mut wire = [0; 13];
    wire[0] = 38;
    wire[1..5].copy_from_slice(&sender.to_le_bytes());
    wire[5..9].copy_from_slice(&target.to_le_bytes());
    wire[9..13].copy_from_slice(&treaty.to_le_bytes());
    wire
}

fn complete_facts() -> DiplomacyInstalledFacts {
    DiplomacyInstalledFacts {
        no_rush_frames: Some(0),
        type_available: [[Some(true); 6]; 8],
        declaration_costs: [[Some(10); 6]; 8],
        tribute_scale_percent: [Some(100); 8],
        war_allowed: Some(true),
        has_shared_vision_preq: [Some(false); 8],
        console_treaty_one: [Some(false); 8],
        global_shared_vision: Some(false),
        is_neutral: [Some(false); 8],
        team_members_mode_one: [Some(1); 8],
        num_allies: [Some(0); 8],
        tribute_econ: [[Some(0); 6]; 8],
    }
}

fn configured_sim() -> Sim {
    let mut sim = Sim::new(0x38_41_14, 8);
    let mut setup = ManualPlayerSetup {
        active_mask: 0xff,
        team_style: 0,
        local_player_setup_slot: 0,
        ..ManualPlayerSetup::default()
    };
    setup.teams = [0, 1, 2, 3, 0, 1, 2, 3];
    sim.start_manual_player_setup(setup).unwrap();
    let mut players = PlayerTable::new();
    for who in 0..8 {
        players.seat(who, 1, who as u8, who as i8);
    }
    players.console_play = 0;
    players.console_who = 0;
    sim.players = Some(players);
    sim.world.frame = 700;
    sim.vic_match.frame = 700;
    for who in 0..8 {
        sim.vic_leaders.slots[who].diplos = [Relation::Peace as i32; 8];
        sim.vic_leaders.slots[who].diplos[who] = Relation::Ally as i32;
        sim.vic_leaders.slots[who].economy.bucket = [1_000 + who as i32; 6];
        sim.leaders[who].econ.stockpile = [1_000 + who as i32; 6];
    }
    sim.replace_diplomacy_authority(complete_facts());
    sim
}

fn with_large_stack(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn retail_packet_runs_bridge_sim_v14_load_and_resumed_packet_identically() {
    with_large_stack(|| {
        let mut uninterrupted = configured_sim();
        let first = uninterrupted
            .process_diplomacy_package(2, 0x2602, &RETAIL_DECLARE_2_5_WAR)
            .unwrap();
        assert_eq!(
            first.status,
            CanonicalDiplomacyStatus::Applied,
            "{:?}",
            first.error
        );
        assert!(first.validates(&first.request));
        assert_eq!(uninterrupted.vic_leaders.slots[2].diplos[5], 0);
        assert_eq!(uninterrupted.vic_leaders.slots[5].diplos[2], 0);
        assert_eq!(uninterrupted.leaders[2].econ.stockpile, [992; 6]);

        let checkpoint = save_sim(&uninterrupted).expect("applied declaration is v14-savable");
        assert_eq!(
            u32::from_le_bytes(checkpoint[24..28].try_into().unwrap()),
            14
        );
        let mut resumed = load_sim(&checkpoint).expect("v14 declaration state reloads");

        let peace = declare(2, 5, Relation::Peace as i32);
        let without_facts = resumed
            .process_diplomacy_package(2, 0x2603, &peace)
            .unwrap();
        assert_eq!(without_facts.status, CanonicalDiplomacyStatus::Unavailable);
        assert!(matches!(
            without_facts.error,
            Some(CanonicalDiplomacyRuntimeError::MissingNoRushFrames)
        ));
        assert_eq!(save_sim(&resumed).unwrap(), checkpoint);

        resumed.replace_diplomacy_authority(complete_facts());
        let resumed_receipt = resumed
            .process_diplomacy_package(2, 0x2603, &peace)
            .unwrap();
        let uninterrupted_receipt = uninterrupted
            .process_diplomacy_package(2, 0x2603, &peace)
            .unwrap();
        assert_eq!(resumed_receipt.status, CanonicalDiplomacyStatus::Applied);
        assert!(resumed_receipt.validates(&resumed_receipt.request));
        assert!(uninterrupted_receipt.validates(&uninterrupted_receipt.request));
        assert_eq!(
            save_sim(&resumed).unwrap(),
            save_sim(&uninterrupted).unwrap()
        );
        assert_eq!(resumed.channel_digest(), uninterrupted.channel_digest());
    });
}

#[test]
fn reached_army_authority_is_unavailable_and_rolls_back_every_owner() {
    with_large_stack(|| {
        let mut sim = configured_sim();
        sim.armies.lists[2][3].valid = 1;
        let before = save_sim(&sim).unwrap();
        let receipt = sim
            .process_diplomacy_package(2, 9, &RETAIL_DECLARE_2_5_WAR)
            .unwrap();
        assert_eq!(receipt.status, CanonicalDiplomacyStatus::Unavailable);
        assert!(receipt.validates(&receipt.request));
        assert!(
            matches!(
                receipt.error,
                Some(CanonicalDiplomacyRuntimeError::ExternalAuthority(ref calls))
                    if !calls.is_empty()
            ),
            "{:?}",
            receipt.error
        );
        assert_eq!(save_sim(&sim).unwrap(), before);
    });
}

#[test]
fn opcode_41_is_not_admitted_by_the_op38_production_mount() {
    let mut sim = configured_sim();
    let accept = [41, 2, 0, 0, 0, 5, 0, 0, 0];
    assert_eq!(
        sim.process_diplomacy_package(2, 1, &accept),
        Err(CanonicalDiplomacyRuntimeError::UnsupportedOpcode(Some(41)))
    );
}
