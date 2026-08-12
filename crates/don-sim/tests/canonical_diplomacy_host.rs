// SPDX-License-Identifier: GPL-3.0-or-later

// The production module export is intentionally mounted with the coordinated command/tick/save
// patch.  Keep this exclusive lane executable in the meantime by recreating the same sibling
// module topology used below `systems` in the library.
pub mod command {
    pub use don_sim::command::*;
}

#[path = "../src/systems/canonical_diplomacy_host.rs"]
mod canonical_diplomacy_host;
#[path = "../src/systems/diplomacy_accept_host.rs"]
mod diplomacy_accept_host;
#[path = "../src/systems/diplomacy_deal_callbacks.rs"]
mod diplomacy_deal_callbacks;
#[path = "../src/systems/diplomacy_declare_host.rs"]
mod diplomacy_declare_host;
#[path = "../src/systems/leader_set_diplo.rs"]
mod leader_set_diplo;

use canonical_diplomacy_host::{
    commit_diplomacy_transaction, decode_diplomacy_for_save, decode_diplomacy_payload,
    encode_diplomacy_payload, prepare_diplomacy_transaction, CanonicalDiplomacyAuthority,
    CommitDiplomacyError, DiplomacyCodecError, DiplomacyInstalledFacts, DiplomacyOwnerImage,
    DiplomacyPersistentState, DIPLOMACY_PAYLOAD_LEN, DIPLOMACY_SAVE_FORMAT_VERSION,
    PRE_DIPLOMACY_SAVE_FORMAT_VERSION,
};
use command::setup_diplomacy::{PlayerSetup, PLAYER_PRESENT};
use diplomacy_accept_host::{AcceptAuthority, AcceptOutcome};
use diplomacy_declare_host::DeclareStep;
use leader_set_diplo::{Relation, DIPLO_SLOTS, NUM_GOODS};

// Exact retail corpus packets from re/fixtures/retail-diplomacy-command-packages-v1.json.
const RETAIL_DECLARE_2_5_WAR: [u8; 13] = [38, 2, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0];
const RETAIL_ACCEPT_2_3: [u8; 9] = [41, 2, 0, 0, 0, 3, 0, 0, 0];

fn active_owner(relation: Relation) -> DiplomacyOwnerImage {
    let mut owner = DiplomacyOwnerImage::default();
    owner.setup.frame = 700;
    owner.local_who = 2;
    for who in 0..DIPLO_SLOTS {
        owner.setup.players[who] = PlayerSetup {
            flags: PLAYER_PRESENT,
            who: who as u8,
            team: who as i8,
        };
        owner.setup.leaders[who].who = who as i32;
        owner.setup.leaders[who].leader_flags = 1;
        owner.setup.leaders[who].diplos = [relation as i32; DIPLO_SLOTS];
        owner.setup.leaders[who].diplos[who] = Relation::Ally as i32;
        owner.resources[who] = [1_000 + who as i32; NUM_GOODS];
    }
    owner
}

fn complete_facts() -> DiplomacyInstalledFacts {
    DiplomacyInstalledFacts {
        type_available: [[Some(true); NUM_GOODS]; DIPLO_SLOTS],
        declaration_costs: [[Some(10); NUM_GOODS]; DIPLO_SLOTS],
        tribute_scale_percent: [Some(100); DIPLO_SLOTS],
        war_allowed: Some(true),
        has_shared_vision_preq: [Some(false); DIPLO_SLOTS],
        console_treaty_one: [Some(false); DIPLO_SLOTS],
        global_shared_vision: Some(false),
        is_neutral: [Some(false); DIPLO_SLOTS],
        team_members_mode_one: [Some(1); DIPLO_SLOTS],
        num_allies: [Some(0); DIPLO_SLOTS],
        tribute_econ: [[Some(4); NUM_GOODS]; DIPLO_SLOTS],
    }
}

#[test]
fn retail_opcode_38_projects_resources_relations_armies_objects_and_commits_once() {
    let mut owner = active_owner(Relation::Peace);
    let before = owner.clone();
    let prepared =
        prepare_diplomacy_transaction(&owner, &complete_facts(), &RETAIL_DECLARE_2_5_WAR).unwrap();
    assert!(prepared.planned_authority.is_empty());
    assert!(prepared.required_external_authority.is_empty());
    let canonical_diplomacy_host::PreparedDiplomacyPlan::Declare(plan) = &prepared.plan else {
        panic!("wrong plan")
    };
    assert!(plan.steps.iter().any(|step| matches!(
        step,
        DeclareStep::IncrementTargetStatistic {
            sender: 2,
            target: 5
        }
    )));

    commit_diplomacy_transaction(&mut owner, &prepared, &[]).unwrap();
    assert_eq!(owner.resources[2], [before.resources[2][0] - 10; NUM_GOODS]);
    assert_eq!(owner.setup.leaders[2].diplos[5], Relation::War as i32);
    assert_eq!(owner.setup.leaders[5].diplos[2], Relation::War as i32);
    assert_eq!(owner.leader_diplomacy[2].declaration_by_target[5], 1);
    assert!(owner.interface_dirty);
    // Empty canonical rosters generated no hidden object/army calls or mutations.
    assert_eq!(owner.valid_armies, before.valid_armies);
    assert_eq!(owner.ejection_units, before.ejection_units);
}

#[test]
fn retail_opcode_41_rolls_back_all_owners_until_exact_authority_is_acknowledged() {
    let mut owner = active_owner(Relation::War);
    for (from, to) in [(2usize, 3usize), (3, 2)] {
        let proposal = &mut owner.retained.leaders[from].proposals[to];
        proposal.agreement_pending = 1;
        proposal.proposal_open = 1;
        proposal.treaty = Relation::Peace as i32;
    }
    owner.retained.leaders[2].proposals[3].offers[0] = 25;
    owner.retained.leaders[2].reserved_resources[0] = 25;
    owner.resources[2][0] -= 25;
    let before = owner.clone();
    let prepared =
        prepare_diplomacy_transaction(&owner, &complete_facts(), &RETAIL_ACCEPT_2_3).unwrap();
    let canonical_diplomacy_host::PreparedDiplomacyPlan::Accept(plan) = &prepared.plan else {
        panic!("wrong plan")
    };
    assert_eq!(plan.outcome, AcceptOutcome::Accepted);
    assert_eq!(prepared.planned_authority.len(), NUM_GOODS * 2 + 2);
    assert!(matches!(
        prepared.planned_authority[1],
        CanonicalDiplomacyAuthority::Accept(AcceptAuthority::ConsiderTribute {
            receiver: 3,
            sender: 2,
            good: 0,
            raw: 25,
        })
    ));
    assert_eq!(
        &prepared.planned_authority[NUM_GOODS * 2..],
        &[
            CanonicalDiplomacyAuthority::Accept(AcceptAuthority::NotifyDeal {
                leader: 2,
                other: 3,
                treaty: 1,
            }),
            CanonicalDiplomacyAuthority::Accept(AcceptAuthority::NotifyDeal {
                leader: 3,
                other: 2,
                treaty: 1,
            }),
        ]
    );
    // `consider_tribute` and `notify_deal` are now internal: no ordinary-accept simulation
    // authority remains for this peace transaction.
    assert!(prepared.required_external_authority.is_empty());
    assert_eq!(prepared.consider_tribute.len(), NUM_GOODS * 2);
    assert_eq!(prepared.notify_deal.len(), 1); // only local leader 2's body presents

    commit_diplomacy_transaction(&mut owner, &prepared, &[]).unwrap();
    assert_eq!(owner.setup.leaders[2].diplos[3], Relation::Peace as i32);
    assert_eq!(owner.setup.leaders[3].diplos[2], Relation::Peace as i32);
    assert_eq!(owner.resources[3][0], before.resources[3][0] + 25);
    assert_eq!(owner.retained.leaders[2].reserved_resources[0], 0);
    assert_eq!(owner.retained.leaders[2].sent_raw, 25);
    assert_eq!(owner.retained.leaders[3].received_scaled, 25);
    assert_eq!(owner.leader_diplomacy[2].peace_frames[3], 700);
    assert_eq!(owner.leader_diplomacy[3].peace_frames[2], 700);
    assert_eq!(owner.leader_diplomacy[2].tribute_stamp[3], 700);
    assert_eq!(owner.leader_diplomacy[3].gift_stamp[2], 700);
    assert_eq!(
        owner.retained.leaders[2].proposals[3],
        command::diplomacy_command_plans::DiplomacyProposal::default()
    );
}

#[test]
fn stale_owner_rejects_without_publishing_the_prepared_after_image() {
    let owner = active_owner(Relation::Peace);
    let prepared =
        prepare_diplomacy_transaction(&owner, &complete_facts(), &RETAIL_DECLARE_2_5_WAR).unwrap();
    let mut current = owner.clone();
    current.resources[2][0] += 1;
    let stale = current.clone();
    assert_eq!(
        commit_diplomacy_transaction(&mut current, &prepared, &[]),
        Err(CommitDiplomacyError::StaleOwner)
    );
    assert_eq!(current, stale);
}

#[test]
fn v14_payload_roundtrips_every_retained_field_and_is_canonical() {
    let mut state = DiplomacyPersistentState::default();
    let leader = &mut state.leaders[7];
    leader.proposals[6].agreement_pending = 1;
    leader.proposals[6].proposal_open = 9;
    leader.proposals[6].treaty = 2;
    leader.proposals[6].offers = [1, 2, 3, 4, 5, 6];
    leader.proposals[6].declaration_costs = [6, 5, 4, 3, 2, 1];
    leader.proposals[6].attacks[5] = 77;
    leader.reserved_resources = [10, 20, 30, 40, 50, 60];
    leader.repeated_targets = 8;
    leader.sent_raw = i32::MAX;
    leader.received_scaled = i32::MIN;

    let bytes = encode_diplomacy_payload(&state);
    assert_eq!(bytes.len(), DIPLOMACY_PAYLOAD_LEN);
    let decoded = decode_diplomacy_payload(&bytes).unwrap();
    assert_eq!(decoded, state);
    assert_eq!(encode_diplomacy_payload(&decoded), bytes);
}

#[test]
fn v13_absence_restores_constructor_state_while_v14_requires_the_leaf() {
    assert_eq!(
        decode_diplomacy_for_save(PRE_DIPLOMACY_SAVE_FORMAT_VERSION, None).unwrap(),
        DiplomacyPersistentState::default()
    );
    assert_eq!(
        decode_diplomacy_for_save(DIPLOMACY_SAVE_FORMAT_VERSION, None),
        Err(DiplomacyCodecError::MissingForCurrentFormat)
    );
    let payload = encode_diplomacy_payload(&DiplomacyPersistentState::default());
    assert_eq!(
        decode_diplomacy_for_save(PRE_DIPLOMACY_SAVE_FORMAT_VERSION, Some(&payload)),
        Err(DiplomacyCodecError::UnexpectedForLegacyFormat)
    );
}

#[test]
fn diplomacy_payload_rejects_truncation_trailing_bytes_and_header_drift() {
    let bytes = encode_diplomacy_payload(&DiplomacyPersistentState::default());
    assert_eq!(
        decode_diplomacy_payload(&bytes[..bytes.len() - 1]),
        Err(DiplomacyCodecError::Truncated)
    );
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert_eq!(
        decode_diplomacy_payload(&trailing),
        Err(DiplomacyCodecError::TrailingBytes)
    );
    let mut bad_slots = bytes;
    bad_slots[2] = 7;
    assert_eq!(
        decode_diplomacy_payload(&bad_slots),
        Err(DiplomacyCodecError::WrongSlotCount(7))
    );
}
