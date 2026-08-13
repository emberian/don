// SPDX-License-Identifier: GPL-3.0-or-later

use don_sim::command::diplomacy_command_plans::DiplomacyProposal;
use don_sim::order::Order;
use don_sim::systems::canonical_diplomacy_host::{
    DiplomacyInstalledFacts, ExternalDiplomacyAuthority, PreparedDiplomacyPlan,
};
use don_sim::systems::canonical_diplomacy_runtime::{
    CanonicalDiplomacyRuntimeError, CanonicalDiplomacyStatus,
};
use don_sim::systems::defeat_cleanup;
use don_sim::systems::diplomacy_accept_host::AcceptOutcome;
use don_sim::systems::diplomacy_ejection_authority::{
    ContainedEjectionAnswer, ContainedEjectionAuthority,
};
use don_sim::systems::groups_guys::{GroupData, Groups};
use don_sim::systems::leader_set_diplo::{Relation, SetDiploAuthority};
use don_sim::systems::movement::PathData;
use don_sim::systems::player_setup::ManualPlayerSetup;
use don_sim::systems::production::runtime::LiveProductionType;
use don_sim::systems::save_load::{load_sim, save_sim};
use don_sim::systems::victory_score::{game_sem, leader_flag};
use don_sim::tick::lifecycle_host::PlayerTable;
use don_sim::tick::Sim;

const RETAIL_DECLARE_2_5_WAR: [u8; 13] = [38, 2, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0];
const RETAIL_ACCEPT_2_3: [u8; 9] = [41, 2, 0, 0, 0, 3, 0, 0, 0];

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
        tribute_econ: [[Some(4); 6]; 8],
        contained_ejection: Default::default(),
    }
}

fn configured_sim_with_mask(active_mask: u8) -> Sim {
    let mut sim = Sim::new(0x38_41_14, 8);
    let mut setup = ManualPlayerSetup {
        active_mask,
        team_style: 0,
        local_player_setup_slot: active_mask.trailing_zeros() as usize,
        ..ManualPlayerSetup::default()
    };
    let teams = [0, 1, 2, 3, 0, 1, 2, 3];
    setup.teams = std::array::from_fn(|who| {
        if active_mask & (1u8 << who) != 0 {
            teams[who]
        } else {
            8 // setup_diplomacy::TEAM_AUTO
        }
    });
    sim.start_manual_player_setup(setup).unwrap();
    let mut players = PlayerTable::new();
    for who in 0..8 {
        if active_mask & (1u8 << who) != 0 {
            players.seat(who, 1, who as u8, who as i8);
        }
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

fn configured_sim() -> Sim {
    configured_sim_with_mask(0xff)
}

fn configured_accept_sim() -> Sim {
    let mut sim = configured_sim();
    sim.players.as_mut().unwrap().console_who = 2;
    for (from, to) in [(2usize, 3usize), (3, 2)] {
        sim.vic_leaders.slots[from].diplos[to] = Relation::War as i32;
        let proposal = &mut sim.diplomacy.leaders[from].proposals[to];
        proposal.agreement_pending = 1;
        proposal.proposal_open = 1;
        proposal.treaty = Relation::Peace as i32;
    }
    sim.diplomacy.leaders[2].proposals[3].offers[0] = 25;
    sim.diplomacy.leaders[2].reserved_resources[0] = 25;
    sim.vic_leaders.slots[2].economy.bucket[0] -= 25;
    sim.leaders[2].econ.stockpile[0] -= 25;
    sim
}

fn configured_alliance_victory_sim() -> Sim {
    let mut sim = configured_sim_with_mask((1 << 2) | (1 << 3));
    sim.players.as_mut().unwrap().console_who = 2;
    for (from, to) in [(2usize, 3usize), (3, 2)] {
        let proposal = &mut sim.diplomacy.leaders[from].proposals[to];
        proposal.agreement_pending = 1;
        proposal.proposal_open = 1;
        proposal.treaty = Relation::Ally as i32;
    }
    sim
}

fn configured_alliance_with_defeated_opponents_sim() -> Sim {
    let mut sim = configured_sim();
    sim.players.as_mut().unwrap().console_who = 2;
    for candidate in 0..8 {
        if candidate == 2 || candidate == 3 {
            continue;
        }
        sim.vic_leaders.slots[candidate].diplos[3] = Relation::Ally as i32;
        sim.vic_leaders.slots[3].diplos[candidate] = Relation::Ally as i32;
    }
    for (from, to) in [(2usize, 3usize), (3, 2)] {
        let proposal = &mut sim.diplomacy.leaders[from].proposals[to];
        proposal.agreement_pending = 1;
        proposal.proposal_open = 1;
        proposal.treaty = Relation::Ally as i32;
    }
    sim
}

fn install_defeated_ground_unit(sim: &mut Sim) -> usize {
    let type_index = 100;
    sim.production_runtime
        .install_type(LiveProductionType::ordinary_unit(type_index, 1, 1));
    let unit = sim.spawn_unit(4, type_index, 111, 222, 4).unwrap();
    let row = sim.world.row_of(unit).unwrap();
    sim.world
        .orders_mut(row)
        .replace(Order::move_to(900, 901, 48));
    sim.paths[row].push(PathData::default());
    sim.world
        .units
        .set_unit_masks(row, defeat_cleanup::DEFEAT_UNIT_MASK | 0x0400_0000 | 0x20);
    row
}

fn install_defeated_ground_army(sim: &mut Sim) -> (usize, usize) {
    let row = install_defeated_ground_unit(sim);
    let object_id = sim.world.units.o()[row];
    sim.world
        .units
        .set_unit_masks(row, sim.world.units.get_unit_masks(row) | 0x0000_0100);
    let group_id = Groups::index(4, 3);
    let mut group = GroupData {
        id: group_id as i32,
        army: 5,
        who: 4,
        form: 4,
        disband: 7,
        ..GroupData::default()
    };
    assert!(group.add(object_id, 4, false, 0, 0));
    sim.groups.list[group_id] = group;
    sim.armies.lists[4][5].valid = 1;
    sim.armies.lists[4][5].army = 5;
    sim.armies.lists[4][5].who = 4;
    sim.armies.lists[4][5].num_groups = 1;
    sim.armies.lists[4][5].list[0] = group_id as i32;
    (row, group_id)
}

fn install_contained_ejection_roster(sim: &mut Sim) -> [ContainedEjectionAnswer; 3] {
    for type_index in [100, 101, 102, 200] {
        sim.production_runtime
            .install_type(LiveProductionType::ordinary_unit(type_index, 1, 1));
    }
    let carrier = sim.spawn_unit(5, 200, 333, 444, 5).unwrap();
    let carrier_row = sim.world.row_of(carrier).unwrap();
    let carrier_object = sim.world.units.o()[carrier_row];
    for type_index in [100, 101, 102] {
        let unit = sim.spawn_unit(2, type_index, 111, 222, 2).unwrap();
        let row = sim.world.row_of(unit).unwrap();
        sim.world.units.inside_up_who_mut()[row] = 5;
        sim.world.units.inside_up_mut()[row] = carrier_object;
    }
    [
        ContainedEjectionAnswer {
            owner: 2,
            object_id: 0,
            come_out_return: 0,
            domain: Some(0),
            has_air_patrol_order: None,
        },
        ContainedEjectionAnswer {
            owner: 2,
            object_id: 1,
            come_out_return: 1,
            domain: None,
            has_air_patrol_order: None,
        },
        ContainedEjectionAnswer {
            owner: 2,
            object_id: 2,
            come_out_return: 0,
            domain: Some(2),
            has_air_patrol_order: Some(false),
        },
    ]
}

fn install_ejection_authority(sim: &mut Sim, answers: &[ContainedEjectionAnswer]) {
    let authority =
        ContainedEjectionAuthority::capture(&sim.world, sim.channel_digest(), answers).unwrap();
    let mut facts = complete_facts();
    facts.contained_ejection = authority;
    sim.replace_diplomacy_authority(facts);
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
fn retail_packet_runs_bridge_sim_current_load_and_resumed_packet_identically() {
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

        let checkpoint = save_sim(&uninterrupted).expect("applied declaration is savable");
        assert_eq!(
            u32::from_le_bytes(checkpoint[24..28].try_into().unwrap()),
            18
        );
        let mut resumed = load_sim(&checkpoint).expect("declaration state reloads");

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
        sim.armies.lists[2][3].army = 3;
        sim.armies.lists[2][3].who = 2;
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
fn armies_off_force_process_executes_exact_entry_arm_and_resumes_with_v17_armies_owner() {
    with_large_stack(|| {
        let mut uninterrupted = configured_sim();
        uninterrupted.vic_leaders.slots[2].leader_flags |= 0x40;
        let army = &mut uninterrupted.armies.lists[2][3];
        army.valid = 1;
        army.army = 3;
        army.who = 2;
        army.human_frame = 9;
        let checkpoint = save_sim(&uninterrupted).expect("armies-off force target is savable");
        assert_eq!(
            u32::from_le_bytes(checkpoint[24..28].try_into().unwrap()),
            18
        );
        let mut resumed = load_sim(&checkpoint).expect("armies-off force target reloads");
        resumed.replace_diplomacy_authority(complete_facts());

        let resumed_receipt = resumed
            .process_diplomacy_package(2, 0x2635, &RETAIL_DECLARE_2_5_WAR)
            .unwrap();
        let uninterrupted_receipt = uninterrupted
            .process_diplomacy_package(2, 0x2635, &RETAIL_DECLARE_2_5_WAR)
            .unwrap();
        for receipt in [&resumed_receipt, &uninterrupted_receipt] {
            assert_eq!(receipt.status, CanonicalDiplomacyStatus::Applied);
            assert!(receipt.validates(&receipt.request));
            assert_eq!(receipt.completed_authority.len(), 1);
            assert!(matches!(
                receipt.completed_authority[0],
                ExternalDiplomacyAuthority::Declare(SetDiploAuthority::ForceArmyProcess {
                    owner: 2,
                    army_slot: 3,
                    forced: 1,
                })
            ));
            assert_eq!(receipt.army_process_receipts.len(), 1);
            let army = &receipt.army_process_receipts[0];
            assert!(army.validates());
            assert_eq!(army.before.human_frame, 9);
            assert_eq!(army.after.human_frame, 8);
            assert!(receipt.victory_receipts.is_empty());
            assert!(receipt.defeat_cleanup.is_none());
        }
        assert_eq!(resumed_receipt, uninterrupted_receipt);
        assert_eq!(resumed.armies.lists[2][3].human_frame, 8);
        assert_eq!(uninterrupted.armies.lists[2][3].human_frame, 8);
        assert_eq!(resumed.vic_leaders.slots[2].diplos[5], Relation::War as i32);
        assert_eq!(
            save_sim(&resumed).unwrap(),
            save_sim(&uninterrupted).unwrap()
        );
        assert_eq!(resumed.channel_digest(), uninterrupted.channel_digest());
        let reloaded = load_sim(&save_sim(&resumed).unwrap()).expect("forced Army result reloads");
        assert_eq!(save_sim(&reloaded).unwrap(), save_sim(&resumed).unwrap());

        let mut forged = resumed_receipt.clone();
        forged.army_process_receipts[0].after.human_frame = 7;
        assert!(!forged.validates(&forged.request));
    });
}

#[test]
fn accept_with_reached_army_authority_rolls_back_resources_relations_and_records() {
    with_large_stack(|| {
        let mut sim = configured_accept_sim();
        sim.armies.lists[2][3].valid = 1;
        sim.armies.lists[2][3].army = 3;
        sim.armies.lists[2][3].who = 2;
        let before = save_sim(&sim).unwrap();
        let receipt = sim
            .process_diplomacy_package(2, 0x2902, &RETAIL_ACCEPT_2_3)
            .unwrap();
        assert_eq!(receipt.status, CanonicalDiplomacyStatus::Unavailable);
        assert!(receipt.validates(&receipt.request));
        assert!(matches!(
            receipt.error,
            Some(CanonicalDiplomacyRuntimeError::ExternalAuthority(ref calls))
                if !calls.is_empty()
        ));
        assert_eq!(save_sim(&sim).unwrap(), before);
    });
}

#[test]
fn retail_accept_packet_resumes_and_commits_the_whole_authority_empty_transaction() {
    with_large_stack(|| {
        let mut uninterrupted = configured_accept_sim();
        let checkpoint = save_sim(&uninterrupted).expect("pending reciprocal offer is savable");
        let mut resumed = load_sim(&checkpoint).expect("pending reciprocal offer reloads");

        let without_facts = resumed
            .process_diplomacy_package(2, 0x2902, &RETAIL_ACCEPT_2_3)
            .unwrap();
        assert_eq!(without_facts.status, CanonicalDiplomacyStatus::Unavailable);
        assert!(matches!(
            without_facts.error,
            Some(CanonicalDiplomacyRuntimeError::MissingNoRushFrames)
        ));
        assert_eq!(save_sim(&resumed).unwrap(), checkpoint);

        resumed.replace_diplomacy_authority(complete_facts());
        let resumed_receipt = resumed
            .process_diplomacy_package(2, 0x2902, &RETAIL_ACCEPT_2_3)
            .unwrap();
        let uninterrupted_receipt = uninterrupted
            .process_diplomacy_package(2, 0x2902, &RETAIL_ACCEPT_2_3)
            .unwrap();
        assert_eq!(resumed_receipt.status, CanonicalDiplomacyStatus::Applied);
        assert!(resumed_receipt.validates(&resumed_receipt.request));
        assert!(uninterrupted_receipt.validates(&uninterrupted_receipt.request));
        let PreparedDiplomacyPlan::Accept(plan) = &resumed_receipt.prepared.as_ref().unwrap().plan
        else {
            panic!("opcode 41 returned a non-accept plan")
        };
        assert_eq!(plan.outcome, AcceptOutcome::Accepted);
        assert!(resumed_receipt
            .prepared
            .as_ref()
            .unwrap()
            .required_external_authority
            .is_empty());
        assert_eq!(
            resumed.vic_leaders.slots[2].diplos[3],
            Relation::Peace as i32
        );
        assert_eq!(
            resumed.vic_leaders.slots[3].diplos[2],
            Relation::Peace as i32
        );
        assert_eq!(resumed.leaders[3].econ.stockpile[0], 1_028);
        assert_eq!(resumed.diplomacy.leaders[2].sent_raw, 25);
        assert_eq!(resumed.diplomacy.leaders[3].received_scaled, 25);
        assert_eq!(
            resumed.diplomacy.leaders[2].proposals[3],
            DiplomacyProposal::default()
        );
        assert_eq!(
            resumed.vic_leaders.slots[2].init_diplomacy.made_peace[3],
            700
        );
        assert_eq!(
            resumed.vic_leaders.slots[3].init_diplomacy.made_peace[2],
            700
        );
        assert_eq!(
            resumed.vic_leaders.slots[2].init_diplomacy.tribute_stamp[3],
            700
        );
        assert_eq!(
            resumed.vic_leaders.slots[3].init_diplomacy.gift_stamp[2],
            700
        );
        assert_eq!(
            save_sim(&resumed).unwrap(),
            save_sim(&uninterrupted).unwrap()
        );
        assert_eq!(resumed.channel_digest(), uninterrupted.channel_digest());
    });
}

#[test]
fn retail_alliance_accept_stages_generic_victory_and_resumes_identically() {
    with_large_stack(|| {
        let mut uninterrupted = configured_alliance_victory_sim();
        let checkpoint = save_sim(&uninterrupted).expect("pending alliance is savable");
        let mut resumed = load_sim(&checkpoint).expect("pending alliance reloads");
        resumed.replace_diplomacy_authority(complete_facts());

        let resumed_receipt = resumed
            .process_diplomacy_package(2, 0x2902, &RETAIL_ACCEPT_2_3)
            .unwrap();
        let uninterrupted_receipt = uninterrupted
            .process_diplomacy_package(2, 0x2902, &RETAIL_ACCEPT_2_3)
            .unwrap();
        assert_eq!(resumed_receipt.status, CanonicalDiplomacyStatus::Applied);
        assert!(resumed_receipt.validates(&resumed_receipt.request));
        assert!(uninterrupted_receipt.validates(&uninterrupted_receipt.request));
        assert_eq!(resumed_receipt.completed_authority.len(), 1);
        assert_eq!(resumed_receipt.victory_receipts.len(), 1);
        let mut incomplete_receipt = resumed_receipt.clone();
        incomplete_receipt.victory_receipts.clear();
        assert!(!incomplete_receipt.validates(&incomplete_receipt.request));
        assert_eq!(
            resumed.vic_leaders.slots[2].diplos[3],
            Relation::Ally as i32
        );
        assert_eq!(
            resumed.vic_leaders.slots[3].diplos[2],
            Relation::Ally as i32
        );
        assert_ne!(
            resumed.vic_leaders.slots[2].leader_flags & leader_flag::WON,
            0
        );
        assert_ne!(
            resumed.vic_leaders.slots[3].leader_flags & leader_flag::WON,
            0
        );
        assert_eq!(
            resumed.vic_match.semaphore & (1u32 << game_sem::VICTORY_RESOLVED),
            1u32 << game_sem::VICTORY_RESOLVED
        );
        assert_eq!(
            save_sim(&resumed).unwrap(),
            save_sim(&uninterrupted).unwrap()
        );
        assert_eq!(resumed.channel_digest(), uninterrupted.channel_digest());

        let reloaded = load_sim(&save_sim(&resumed).unwrap()).expect("victory state reloads");
        assert_eq!(save_sim(&reloaded).unwrap(), save_sim(&resumed).unwrap());
    });
}

#[test]
fn alliance_victory_executes_vacuous_defeated_owner_cleanup_and_resumes_identically() {
    with_large_stack(|| {
        let mut uninterrupted = configured_alliance_with_defeated_opponents_sim();
        let checkpoint = save_sim(&uninterrupted).expect("pending coalition alliance is savable");
        let mut resumed = load_sim(&checkpoint).expect("pending coalition alliance reloads");
        resumed.replace_diplomacy_authority(complete_facts());

        let resumed_receipt = resumed
            .process_diplomacy_package(2, 0x2902, &RETAIL_ACCEPT_2_3)
            .unwrap();
        let uninterrupted_receipt = uninterrupted
            .process_diplomacy_package(2, 0x2902, &RETAIL_ACCEPT_2_3)
            .unwrap();
        assert_eq!(resumed_receipt.status, CanonicalDiplomacyStatus::Applied);
        assert!(resumed_receipt.validates(&resumed_receipt.request));
        assert!(uninterrupted_receipt.validates(&uninterrupted_receipt.request));
        let cleanup = resumed_receipt.defeat_cleanup.as_ref().unwrap();
        assert_ne!(cleanup.owners, 0);
        assert_eq!(
            cleanup.per_owner.len(),
            cleanup.owners.count_ones() as usize
        );
        for cleanup in &cleanup.per_owner {
            assert_eq!(cleanup.armies_stopped, 0);
            assert_eq!(cleanup.slots_visited, 0);
        }
        let mut forged_cleanup = resumed_receipt.clone();
        forged_cleanup.defeat_cleanup.as_mut().unwrap().per_owner[0].owner = 8;
        assert!(!forged_cleanup.validates(&forged_cleanup.request));
        assert_eq!(
            save_sim(&resumed).unwrap(),
            save_sim(&uninterrupted).unwrap()
        );
        assert_eq!(resumed.channel_digest(), uninterrupted.channel_digest());
        let reloaded = load_sim(&save_sim(&resumed).unwrap()).expect("coalition victory reloads");
        assert_eq!(save_sim(&reloaded).unwrap(), save_sim(&resumed).unwrap());
    });
}

#[test]
fn alliance_victory_cleans_a_live_ground_unit_atomically_and_resumes_identically() {
    with_large_stack(|| {
        let mut uninterrupted = configured_alliance_with_defeated_opponents_sim();
        let row = install_defeated_ground_unit(&mut uninterrupted);
        let checkpoint = save_sim(&uninterrupted).expect("pending ground cleanup is savable");
        let mut resumed = load_sim(&checkpoint).expect("pending ground cleanup reloads");
        resumed.replace_diplomacy_authority(complete_facts());

        let without_type = resumed
            .process_diplomacy_package(2, 0x2902, &RETAIL_ACCEPT_2_3)
            .unwrap();
        assert_eq!(without_type.status, CanonicalDiplomacyStatus::Unavailable);
        assert!(matches!(
            without_type.error,
            Some(CanonicalDiplomacyRuntimeError::VictoryDefeatCleanup(
                defeat_cleanup::DefeatCleanupError::UnsupportedUnitType {
                    owner: 4,
                    object_id: 0,
                    type_index: 100,
                }
            ))
        ));
        assert_eq!(save_sim(&resumed).unwrap(), checkpoint);

        resumed
            .production_runtime
            .install_type(LiveProductionType::ordinary_unit(100, 1, 1));
        let resumed_receipt = resumed
            .process_diplomacy_package(2, 0x2902, &RETAIL_ACCEPT_2_3)
            .unwrap();
        let uninterrupted_receipt = uninterrupted
            .process_diplomacy_package(2, 0x2902, &RETAIL_ACCEPT_2_3)
            .unwrap();
        assert_eq!(resumed_receipt.status, CanonicalDiplomacyStatus::Applied);
        assert!(resumed_receipt.validates(&resumed_receipt.request));
        assert!(uninterrupted_receipt.validates(&uninterrupted_receipt.request));

        let cleanup = resumed_receipt
            .defeat_cleanup
            .as_ref()
            .unwrap()
            .per_owner
            .iter()
            .find(|cleanup| cleanup.owner == 4)
            .unwrap();
        assert_eq!(cleanup.slots_visited, 1);
        assert_eq!(cleanup.invalid_skipped, 0);
        assert_eq!(cleanup.orders_closed, 1);
        assert_eq!(cleanup.unit_masks_cleared, 1);
        assert_eq!(cleanup.planes_killed, 0);

        assert!(resumed.world.orders(row).is_empty());
        assert!(resumed.paths[row].is_empty());
        assert_eq!(
            resumed.world.units.get_unit_masks(row)
                & (defeat_cleanup::DEFEAT_UNIT_MASK | 0x0400_0000),
            0
        );
        assert_eq!(
            (
                resumed.world.units.orders_x()[row],
                resumed.world.units.orders_y()[row],
                resumed.world.units.dest_angle()[row],
            ),
            (
                resumed.world.units.x_internal()[row],
                resumed.world.units.y_internal()[row],
                resumed.world.units.angle()[row],
            )
        );
        assert_eq!(
            save_sim(&resumed).unwrap(),
            save_sim(&uninterrupted).unwrap()
        );
        assert_eq!(resumed.channel_digest(), uninterrupted.channel_digest());

        let reloaded = load_sim(&save_sim(&resumed).unwrap()).expect("ground cleanup reloads");
        assert_eq!(save_sim(&reloaded).unwrap(), save_sim(&resumed).unwrap());
    });
}

#[test]
fn alliance_victory_with_a_live_defeated_plane_stays_unavailable_and_atomic() {
    with_large_stack(|| {
        let mut sim = configured_alliance_with_defeated_opponents_sim();
        let type_index = 101;
        sim.production_runtime
            .install_type(LiveProductionType::hosted_air_unit(type_index, 1, 1));
        sim.spawn_unit(4, type_index, 111, 222, 4).unwrap();
        let before = save_sim(&sim).unwrap();
        let receipt = sim
            .process_diplomacy_package(2, 0x2902, &RETAIL_ACCEPT_2_3)
            .unwrap();
        assert_eq!(receipt.status, CanonicalDiplomacyStatus::Unavailable);
        assert!(receipt.validates(&receipt.request));
        assert!(matches!(
            receipt.error,
            Some(
                CanonicalDiplomacyRuntimeError::VictoryNeedsPlaneDefeatCleanup {
                    owner: 4,
                    object_id: 0,
                }
            )
        ));
        assert_eq!(save_sim(&sim).unwrap(), before);
    });
}

#[test]
fn alliance_victory_stops_a_standing_ground_army_and_resumes_identically() {
    with_large_stack(|| {
        let mut uninterrupted = configured_alliance_with_defeated_opponents_sim();
        let (row, group_id) = install_defeated_ground_army(&mut uninterrupted);
        let army_before = uninterrupted.armies.lists[4][5].clone();
        let checkpoint =
            save_sim(&uninterrupted).expect("standing Army is savable through the v17 owner");
        assert_eq!(
            u32::from_le_bytes(checkpoint[24..28].try_into().unwrap()),
            18
        );
        let mut resumed = load_sim(&checkpoint).expect("standing Army reloads");
        resumed.replace_diplomacy_authority(complete_facts());
        resumed
            .production_runtime
            .install_type(LiveProductionType::ordinary_unit(100, 1, 1));

        let resumed_receipt = resumed
            .process_diplomacy_package(2, 0x2902, &RETAIL_ACCEPT_2_3)
            .unwrap();
        let uninterrupted_receipt = uninterrupted
            .process_diplomacy_package(2, 0x2902, &RETAIL_ACCEPT_2_3)
            .unwrap();
        assert_eq!(resumed_receipt.status, CanonicalDiplomacyStatus::Applied);
        assert!(resumed_receipt.validates(&resumed_receipt.request));
        assert!(uninterrupted_receipt.validates(&uninterrupted_receipt.request));

        let cleanup = resumed_receipt
            .defeat_cleanup
            .as_ref()
            .unwrap()
            .per_owner
            .iter()
            .find(|cleanup| cleanup.owner == 4)
            .unwrap();
        assert_eq!(cleanup.armies_stopped, 1);
        assert_eq!(cleanup.groups_stopped, 1);
        assert_eq!(cleanup.army_members_halted, 1);
        assert_eq!(cleanup.orders_closed, 1);
        assert_eq!(resumed.armies.lists[4][5], army_before);
        assert_eq!(resumed.groups.list[group_id].form, -1);
        assert_eq!(resumed.groups.list[group_id].disband, 0);
        assert!(resumed.world.orders(row).is_empty());
        assert!(resumed.paths[row].is_empty());
        assert_eq!(
            resumed.world.units.get_unit_masks(row)
                & (defeat_cleanup::DEFEAT_UNIT_MASK | 0x0400_0000 | 0x0000_0100),
            0
        );
        assert_eq!(
            save_sim(&resumed).unwrap(),
            save_sim(&uninterrupted).unwrap()
        );
        assert_eq!(resumed.channel_digest(), uninterrupted.channel_digest());
        let reloaded = load_sim(&save_sim(&resumed).unwrap()).expect("stopped Army reloads");
        assert_eq!(save_sim(&reloaded).unwrap(), save_sim(&resumed).unwrap());
    });
}

#[test]
fn alliance_revocation_projects_the_full_contained_ejection_cone_after_current_save_resume() {
    with_large_stack(|| {
        let mut uninterrupted = configured_sim();
        for (from, to) in [(2usize, 5usize), (5, 2)] {
            uninterrupted.vic_leaders.slots[from].diplos[to] = Relation::Ally as i32;
        }
        let answers = install_contained_ejection_roster(&mut uninterrupted);
        let checkpoint = save_sim(&uninterrupted).expect("contained Unit roster is savable");
        assert_eq!(
            u32::from_le_bytes(checkpoint[24..28].try_into().unwrap()),
            18
        );
        let mut resumed = load_sim(&checkpoint).expect("contained Unit roster reloads");
        assert_eq!(resumed.channel_digest(), uninterrupted.channel_digest());
        resumed.replace_diplomacy_authority(complete_facts());

        let missing = resumed
            .process_diplomacy_package(2, 0x2602, &declare(2, 5, Relation::Peace as i32))
            .unwrap();
        assert_eq!(missing.status, CanonicalDiplomacyStatus::Unavailable);
        assert!(matches!(
            missing.error,
            Some(CanonicalDiplomacyRuntimeError::ContainedEjectionAuthority(
                don_sim::systems::diplomacy_ejection_authority::ContainedEjectionAuthorityError::MissingAnswer {
                    owner: 2,
                    object_id: 0,
                }
            ))
        ), "unexpected missing-authority receipt: {missing:#?}");
        assert_eq!(save_sim(&resumed).unwrap(), checkpoint);

        install_ejection_authority(&mut resumed, &answers);
        install_ejection_authority(&mut uninterrupted, &answers);
        let wire = declare(2, 5, Relation::Peace as i32);
        let resumed_receipt = resumed.process_diplomacy_package(2, 0x2602, &wire).unwrap();
        let uninterrupted_receipt = uninterrupted
            .process_diplomacy_package(2, 0x2602, &wire)
            .unwrap();
        let expected = vec![
            ExternalDiplomacyAuthority::Declare(SetDiploAuthority::ComeOut {
                owner: 2,
                object_id: 0,
            }),
            ExternalDiplomacyAuthority::Declare(SetDiploAuthority::ComeOut {
                owner: 2,
                object_id: 1,
            }),
            ExternalDiplomacyAuthority::Declare(SetDiploAuthority::KillContainedUnit {
                owner: 2,
                object_id: 1,
                reason: 0,
            }),
            ExternalDiplomacyAuthority::Declare(SetDiploAuthority::ComeOut {
                owner: 2,
                object_id: 2,
            }),
            ExternalDiplomacyAuthority::Declare(SetDiploAuthority::AddAirStrafeOrder {
                owner: 2,
                object_id: 2,
                queue_pos: 2,
            }),
        ];
        for receipt in [&resumed_receipt, &uninterrupted_receipt] {
            assert_eq!(receipt.status, CanonicalDiplomacyStatus::Unavailable);
            assert!(receipt.validates(&receipt.request));
            assert!(receipt.installed_facts.is_some());
            assert!(receipt.prepared.is_some());
            assert!(receipt.completed_authority.is_empty());
            assert_eq!(
                receipt.error,
                Some(CanonicalDiplomacyRuntimeError::ExternalAuthority(
                    expected.clone()
                ))
            );
        }
        let mut forged = resumed_receipt.clone();
        let Some(CanonicalDiplomacyRuntimeError::ExternalAuthority(calls)) = &mut forged.error
        else {
            unreachable!()
        };
        calls.swap(0, 1);
        assert!(!forged.validates(&forged.request));
        assert_eq!(resumed_receipt, uninterrupted_receipt);
        assert_eq!(save_sim(&resumed).unwrap(), checkpoint);
        assert_eq!(save_sim(&uninterrupted).unwrap(), checkpoint);
        assert_eq!(resumed.channel_digest(), uninterrupted.channel_digest());

        let mut stale = resumed.diplomacy_authority.clone();
        stale.contained_ejection.units[0].uid ^= 1;
        resumed.replace_diplomacy_authority(stale);
        let stale_receipt = resumed.process_diplomacy_package(2, 0x2603, &wire).unwrap();
        assert!(matches!(
            stale_receipt.error,
            Some(CanonicalDiplomacyRuntimeError::ContainedEjectionAuthority(
                _
            ))
        ));
        assert_eq!(save_sim(&resumed).unwrap(), checkpoint);
    });
}

#[test]
fn non_diplomacy_opcode_is_not_admitted_by_the_production_mount() {
    let mut sim = configured_sim();
    assert_eq!(
        sim.process_diplomacy_package(2, 1, &[42]),
        Err(CanonicalDiplomacyRuntimeError::UnsupportedOpcode(Some(42)))
    );
}
