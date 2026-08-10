// SPDX-License-Identifier: GPL-3.0-or-later

use don_sim::command::diplomacy_command_plans::{
    DiplomacyCommandRequest, DiplomacyCommandState, DiplomacyStep, DiplomacyTransactionStatus,
    LocalNotice, REJECT_OPCODE,
};
use don_sim::command::setup_diplomacy::{LeaderTeamState, PlayerSetup, DIPLO_ALLY, SETUP_SLOTS};
use don_sim::command::{Bridge, Fleet, ObjectTable, Package};

fn state() -> DiplomacyCommandState {
    let mut state = DiplomacyCommandState::default();
    for slot in 0..SETUP_SLOTS {
        state.setup.players[slot] = PlayerSetup {
            flags: 1,
            who: slot as u8,
            team: slot as i8,
        };
        state.setup.leaders[slot] = LeaderTeamState {
            leader_flags: 1,
            who: slot as i32,
            diplos: [1; SETUP_SLOTS],
        };
        state.setup.leaders[slot].diplos[slot] = DIPLO_ALLY;
    }
    state.local_who = 2;
    state
}

fn reject(sender: i32, target: i32) -> Vec<u8> {
    let mut wire = vec![REJECT_OPCODE];
    wire.extend_from_slice(&sender.to_le_bytes());
    wire.extend_from_slice(&target.to_le_bytes());
    wire
}

#[test]
fn pending_reject_commits_the_exact_host_after_image_and_receipt() {
    let mut before = state();
    before.leaders[1].response_314[2] = 7;
    before.leaders[1].response_334[2] = 9;
    before.leaders[1].reserved_resources[0] = 6;
    before.leaders[1].buckets[0] = 10;
    before.leaders[1].proposals[2].agreement_pending = 1;
    before.leaders[1].proposals[2].proposal_open = 1;
    before.leaders[1].proposals[2].declaration_costs[0] = 4;

    let wire = reject(1, 2);
    let mut host = ObjectTable::new(0);
    host.set_diplomacy_command_state(before);
    let mut bridge = Bridge::new();
    let mut package = Package::new(0, 0);
    bridge.process_all(&mut package, &wire, &mut host).unwrap();

    let after = host.diplomacy_command_state_ref().unwrap();
    assert_eq!(after.leaders[1].response_314[2], 0);
    assert_eq!(after.leaders[1].response_334[2], 0);
    assert_eq!(after.leaders[1].reserved_resources[0], 2);
    assert_eq!(after.leaders[1].buckets[0], 14);
    assert_eq!(after.leaders[1].proposals[2].agreement_pending, 0);
    assert_eq!(after.leaders[1].proposals[2].proposal_open, 0);
    assert_eq!(after.leaders[1].proposals[2].declaration_costs, [0; 6]);

    let receipts = host.take_diplomacy_command_receipts();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].status, DiplomacyTransactionStatus::Applied);
    assert!(receipts[0].validates(&receipts[0].request));
}

#[test]
fn nonpending_reject_commits_atomically_without_recursive_diplomacy() {
    let mut before = state();
    before.setup.leaders[1].leader_flags |= 4;
    before.setup.leaders[1].diplos[2] = DIPLO_ALLY;
    before.setup.leaders[2].diplos[1] = DIPLO_ALLY;
    before.leaders[1].proposals[2].attacks[3] = 1;
    before.leaders[2].proposals[1].agreement_pending = 1;
    before.leaders[2].proposals[1].declaration_costs[0] = 4;
    before.leaders[2].reserved_resources[0] = 4;
    let wire = reject(1, 2);
    let mut host = ObjectTable::new(0);
    host.set_diplomacy_command_state(before);
    let mut bridge = Bridge::new();
    let mut package = Package::new(0, 0);
    bridge.process_all(&mut package, &wire, &mut host).unwrap();

    let after = host.diplomacy_command_state_ref().unwrap();
    assert_eq!(after.setup.leaders[1].diplos[2], DIPLO_ALLY);
    assert_eq!(after.setup.leaders[2].diplos[1], DIPLO_ALLY);
    assert_eq!(after.leaders[2].buckets[0], 4);
    assert_eq!(after.leaders[1].proposals[2], Default::default());
    assert_eq!(after.leaders[2].proposals[1], Default::default());
    let receipts = host.take_diplomacy_command_receipts();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].status, DiplomacyTransactionStatus::Applied);
    assert!(receipts[0].validates(&receipts[0].request));
    let steps = &receipts[0].plan.as_ref().unwrap().steps;
    assert!(steps
        .iter()
        .any(|step| matches!(step, DiplomacyStep::RejectCounterproposalSelfGate { .. })));
    assert_eq!(
        &steps[steps.len() - 2..],
        &[
            DiplomacyStep::LocalNotice {
                who: 2,
                notice: LocalNotice::CounterproposalRejected,
            },
            DiplomacyStep::Sound { category: 0x18 },
        ]
    );
}

#[test]
fn stale_snapshot_is_refused_before_planned_state_can_commit() {
    let mut requested = state();
    requested.leaders[1].proposals[2].agreement_pending = 1;
    let mut current = requested.clone();
    current.frame = 1;
    let request = DiplomacyCommandRequest {
        before: requested,
        wire: reject(1, 2),
    };
    let mut host = ObjectTable::new(0);
    host.set_diplomacy_command_state(current.clone());

    let receipt = host.apply_diplomacy_command_transaction(request);

    assert_eq!(receipt.status, DiplomacyTransactionStatus::Unavailable);
    assert_eq!(host.diplomacy_command_state_ref(), Some(&current));
    assert!(host.take_diplomacy_command_receipts().is_empty());
}
