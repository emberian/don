// SPDX-License-Identifier: GPL-3.0-or-later
//
// Path imports keep this recovery lane out of the shared module list until convergence.
#[path = "../src/systems/diplomacy_command_plans.rs"]
mod diplomacy_command_plans;
#[path = "../src/systems/setup_diplomacy.rs"]
mod setup_diplomacy;

use diplomacy_command_plans::*;
use setup_diplomacy::{LeaderTeamState, PlayerSetup, DIPLO_ALLY, SETUP_SLOTS};

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
    state.local_who = 0;
    state
}

fn wire9(opcode: u8, sender: i32, target: i32) -> Vec<u8> {
    let mut bytes = vec![opcode];
    bytes.extend_from_slice(&sender.to_le_bytes());
    bytes.extend_from_slice(&target.to_le_bytes());
    bytes
}

fn wire13(opcode: u8, sender: i32, target: i32, value: i32) -> Vec<u8> {
    let mut bytes = wire9(opcode, sender, target);
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

fn wire17(opcode: u8, sender: i32, target: i32, a: i32, b: i32) -> Vec<u8> {
    let mut bytes = wire13(opcode, sender, target, a);
    bytes.extend_from_slice(&b.to_le_bytes());
    bytes
}

fn applied(decision: DiplomacyPlanDecision) -> DiplomacyCommandPlan {
    match decision {
        DiplomacyPlanDecision::Apply(plan) => plan,
        DiplomacyPlanDecision::Boundary(boundary) => panic!("unexpected boundary: {boundary:?}"),
    }
}

#[test]
fn fixed_wire_decode_preserves_signed_fields_and_demand_uses_wrapping_negation() {
    assert_eq!(
        decode_diplomacy_command(&wire13(TREATY_OPCODE, 2, 5, -7)),
        Ok(DiplomacyWireCommand::Treaty {
            sender: 2,
            target: 5,
            treaty: -7,
        })
    );
    assert_eq!(
        decode_diplomacy_command(&wire17(DEMAND_TRIBUTE_OPCODE, 1, 3, 4, i32::MIN)),
        Ok(DiplomacyWireCommand::Offer {
            sender: 1,
            target: 3,
            good: 4,
            amount: i32::MIN,
            demand: true,
        })
    );
    assert_eq!(
        decode_diplomacy_command(&wire9(TREATY_OPCODE, 1, 2)),
        Err(DiplomacyDecodeError::WrongWireLength {
            opcode: TREATY_OPCODE,
            expected: 13,
            actual: 9,
        })
    );
}

#[test]
fn treaty_runs_both_clear_agree_refunds_before_the_symmetric_treaty_write() {
    let mut before = state();
    let mine = &mut before.leaders[1];
    mine.reserved_resources[0] = 7;
    mine.buckets[0] = 10;
    mine.proposals[3].agreement_pending = 1;
    mine.proposals[3].declaration_costs[0] = 4;
    mine.proposals[3].offers[0] = 8;
    before.leaders[3].proposals[1].agreement_pending = 1;
    before.leaders[3].proposals[1].declaration_costs[1] = 5;
    before.leaders[3].reserved_resources[1] = 3;

    let plan = applied(plan_diplomacy_command(&before, &wire13(TREATY_OPCODE, 1, 3, 6)).unwrap());
    assert_eq!(plan.state.leaders[1].reserved_resources[0], 0);
    assert_eq!(plan.state.leaders[1].buckets[0], 17);
    assert_eq!(plan.state.leaders[3].reserved_resources[1], 0);
    assert_eq!(plan.state.leaders[3].buckets[1], 3);
    assert_eq!(plan.state.leaders[1].proposals[3].declaration_costs, [0; 6]);
    assert_eq!(plan.state.leaders[1].proposals[3].offers[0], 8);
    assert_eq!(plan.state.leaders[1].proposals[3].proposal_open, 0);
    assert_eq!(plan.state.leaders[1].proposals[3].treaty, 6);
    assert_eq!(plan.state.leaders[3].proposals[1].treaty, 6);
    assert_eq!(
        plan.steps.last(),
        Some(&DiplomacyStep::WriteTreaty {
            sender: 1,
            target: 3,
            treaty: 6,
        })
    );
}

#[test]
fn tribute_and_demand_write_opposite_directional_deltas_after_capacity_gate() {
    let mut before = state();
    before.leaders[1].buckets[2] = 100;
    before.leaders[1].proposals[4].offers[2] = 10;

    let tribute =
        applied(plan_diplomacy_command(&before, &wire17(TRIBUTE_OPCODE, 1, 4, 2, 20)).unwrap());
    assert_eq!(tribute.state.leaders[1].proposals[4].offers[2], 30);
    assert_eq!(tribute.state.leaders[4].proposals[1].offers[2], -20);

    let demand = applied(
        plan_diplomacy_command(&tribute.state, &wire17(DEMAND_TRIBUTE_OPCODE, 1, 4, 2, 7)).unwrap(),
    );
    assert_eq!(demand.state.leaders[1].proposals[4].offers[2], 23);
    assert_eq!(demand.state.leaders[4].proposals[1].offers[2], -13);
}

#[test]
fn unaffordable_positive_tribute_is_an_exact_noop_with_local_feedback_only() {
    let mut before = state();
    before.local_who = 1;
    before.leaders[1].buckets[2] = 12;
    before.leaders[1].proposals[4].offers[2] = 10;
    let plan =
        applied(plan_diplomacy_command(&before, &wire17(TRIBUTE_OPCODE, 1, 4, 2, 3)).unwrap());
    assert_eq!(plan.state, before);
    assert!(plan
        .steps
        .contains(&DiplomacyStep::RefuseInsufficientTribute {
            sender: 1,
            target: 4,
            good: 2,
            amount: 3,
        }));
    assert!(plan.steps.contains(&DiplomacyStep::LocalNotice {
        who: 1,
        notice: LocalNotice::InsufficientTribute,
    }));
}

#[test]
fn clear_tributes_preserves_treaty_and_attacks_while_clear_all_restores_every_sentinel() {
    let mut before = state();
    before.local_who = 3;
    before.leaders[1].proposals[3].treaty = 4;
    before.leaders[1].proposals[3].offers[0] = 9;
    before.leaders[1].proposals[3].attacks[6] = 1;
    before.leaders[3].proposals[1].offers[0] = -9;

    let tributes =
        applied(plan_diplomacy_command(&before, &wire9(CLEAR_TRIBUTES_OPCODE, 1, 3)).unwrap());
    assert_eq!(tributes.state.leaders[1].proposals[3].treaty, 4);
    assert_eq!(tributes.state.leaders[1].proposals[3].attacks[6], 1);
    assert_eq!(tributes.state.leaders[1].proposals[3].offers, [0; 6]);
    assert!(tributes.steps.contains(&DiplomacyStep::LocalNotice {
        who: 3,
        notice: LocalNotice::TributesCleared,
    }));

    let all = applied(plan_diplomacy_command(&before, &wire9(CLEAR_ALL_OPCODE, 1, 3)).unwrap());
    assert_eq!(
        all.state.leaders[1].proposals[3],
        DiplomacyProposal::default()
    );
    assert_eq!(
        all.state.leaders[3].proposals[1],
        DiplomacyProposal::default()
    );
    assert!(all.steps.contains(&DiplomacyStep::LocalNotice {
        who: 3,
        notice: LocalNotice::ProposalsCleared,
    }));
}

#[test]
fn negative_propose_attack_only_leaves_the_open_marker_but_nonnegative_writes_both_rows() {
    let before = state();
    let negative = applied(
        plan_diplomacy_command(&before, &wire17(PROPOSE_ATTACK_OPCODE, 2, 5, -1, 99)).unwrap(),
    );
    assert_eq!(negative.state.leaders[2].proposals[5].proposal_open, 1);
    assert_eq!(negative.state.leaders[2].proposals[5].attacks, [0; 8]);

    let normal = applied(
        plan_diplomacy_command(&before, &wire17(PROPOSE_ATTACK_OPCODE, 2, 5, 7, -3)).unwrap(),
    );
    assert_eq!(normal.state.leaders[2].proposals[5].proposal_open, 0);
    assert_eq!(normal.state.leaders[2].proposals[5].attacks[7], -3);
    assert_eq!(normal.state.leaders[5].proposals[2].attacks[7], -3);
}

#[test]
fn pending_agreement_reject_fast_path_clears_counters_and_only_the_sender_agreement() {
    let mut before = state();
    before.local_who = 4;
    before.leaders[1].response_314[4] = 8;
    before.leaders[1].response_334[4] = 9;
    before.leaders[1].proposals[4].agreement_pending = 1;
    before.leaders[1].proposals[4].declaration_costs[0] = 3;
    before.leaders[1].reserved_resources[0] = 3;

    let plan = applied(plan_diplomacy_command(&before, &wire9(REJECT_OPCODE, 1, 4)).unwrap());
    assert_eq!(plan.state.leaders[1].response_314[4], 0);
    assert_eq!(plan.state.leaders[1].response_334[4], 0);
    assert_eq!(plan.state.leaders[1].proposals[4].agreement_pending, 0);
    assert_eq!(plan.state.leaders[1].buckets[0], 3);
    assert_eq!(
        plan.state.leaders[4].proposals[1],
        before.leaders[4].proposals[1]
    );
    assert!(plan.steps.contains(&DiplomacyStep::LocalNotice {
        who: 4,
        notice: LocalNotice::Rejected,
    }));
}

#[test]
fn only_declare_and_accept_expose_external_diplomacy_boundaries() {
    let mut before = state();
    before.setup.leaders[1].diplos[4] = DIPLO_ALLY;
    before.setup.leaders[4].diplos[1] = DIPLO_ALLY;
    let declare = plan_diplomacy_command(&before, &wire13(DECLARE_OPCODE, 1, 4, 0)).unwrap();
    assert!(matches!(
        declare,
        DiplomacyPlanDecision::Boundary(DiplomacyBoundaryPlan {
            boundary: DiplomacyBoundary::DeclarationResourceAndDiploChange {
                requested_treaty: 0,
                effective_treaty: 1,
                ..
            },
            ..
        })
    ));

    assert!(matches!(
        plan_diplomacy_command(&before, &wire9(ACCEPT_OPCODE, 1, 4)).unwrap(),
        DiplomacyPlanDecision::Boundary(DiplomacyBoundaryPlan {
            boundary: DiplomacyBoundary::AcceptTransferAndDiploChange { .. },
            ..
        })
    ));
}

#[test]
fn nonpending_reject_refunds_reverse_agreement_and_clears_both_records_without_dow() {
    let mut before = state();
    before.local_who = 4;
    before.setup.leaders[1].leader_flags |= 4;
    before.setup.leaders[1].diplos[4] = DIPLO_ALLY;
    before.setup.leaders[4].diplos[1] = DIPLO_ALLY;
    before.leaders[1].response_314[4] = 8;
    before.leaders[1].response_334[4] = 9;
    before.leaders[1].proposals[4].agreement_pending = 0;
    before.leaders[1].proposals[4].proposal_open = 1;
    before.leaders[1].proposals[4].attacks[3] = 1;
    before.leaders[4].proposals[1].agreement_pending = 1;
    before.leaders[4].proposals[1].proposal_open = 1;
    before.leaders[4].proposals[1].declaration_costs[0] = 4;
    before.leaders[4].proposals[1].offers[0] = 3;
    before.leaders[4].reserved_resources[0] = 9;
    before.leaders[4].buckets[0] = 5;

    let plan = applied(plan_diplomacy_command(&before, &wire9(REJECT_OPCODE, 1, 4)).unwrap());

    assert_eq!(plan.state.leaders[1].response_314[4], 0);
    assert_eq!(plan.state.leaders[1].response_334[4], 0);
    assert_eq!(plan.state.leaders[4].reserved_resources[0], 2);
    assert_eq!(plan.state.leaders[4].buckets[0], 12);
    assert_eq!(
        plan.state.leaders[1].proposals[4],
        DiplomacyProposal::default()
    );
    assert_eq!(
        plan.state.leaders[4].proposals[1],
        DiplomacyProposal::default()
    );
    assert_eq!(plan.state.setup.leaders[1].diplos[4], DIPLO_ALLY);
    assert_eq!(plan.state.setup.leaders[4].diplos[1], DIPLO_ALLY);
    assert_eq!(
        &plan.steps[..2],
        &[
            DiplomacyStep::ClearResponseCounter {
                sender: 1,
                target: 4,
                field: ResponseCounterField::TributeDemanded334,
            },
            DiplomacyStep::ClearResponseCounter {
                sender: 1,
                target: 4,
                field: ResponseCounterField::Counteroffer314,
            },
        ]
    );
    assert!(plan
        .steps
        .contains(&DiplomacyStep::RejectCounterproposalSelfGate {
            sender: 1,
            target: 4,
            candidate: 3,
        }));
    assert!(plan.steps.windows(2).any(|steps| {
        steps
            == [
                DiplomacyStep::ClearProposalRecord {
                    leader: 4,
                    target: 1,
                },
                DiplomacyStep::ClearProposalRecord {
                    leader: 1,
                    target: 4,
                },
            ]
    }));
    assert_eq!(
        &plan.steps[plan.steps.len() - 2..],
        &[
            DiplomacyStep::LocalNotice {
                who: 4,
                notice: LocalNotice::CounterproposalRejected,
            },
            DiplomacyStep::Sound { category: 0x18 },
        ]
    );
}

#[test]
fn declaration_enemy_and_no_rush_gates_finish_before_the_external_cost_boundary() {
    let mut before = state();
    before.setup.leaders[1].diplos[4] = 0;
    let enemy = applied(plan_diplomacy_command(&before, &wire13(DECLARE_OPCODE, 1, 4, 0)).unwrap());
    assert_eq!(enemy.state, before);
    assert_eq!(
        enemy.steps,
        vec![DiplomacyStep::DeclarationAlreadyEnemy {
            sender: 1,
            target: 4
        }]
    );

    before.setup.leaders[1].diplos[4] = 1;
    before.setup.leaders[4].diplos[1] = 1;
    before.frame = 100;
    before.no_rush_frames = 10;
    before.leaders[1].declaration_frame[4] = 95;
    before.local_who = 1;
    let no_rush =
        applied(plan_diplomacy_command(&before, &wire13(DECLARE_OPCODE, 1, 4, 0)).unwrap());
    assert_eq!(no_rush.state, before);
    assert!(no_rush.steps.contains(&DiplomacyStep::LocalNotice {
        who: 1,
        notice: LocalNotice::NoRushDeclaration,
    }));
}

#[test]
fn get_diplo_and_runtime_team_reuse_exact_symmetric_and_setup_seams() {
    let mut before = state();
    before.setup.leaders[1].diplos[2] = 2;
    before.setup.leaders[2].diplos[1] = 1;
    assert_eq!(before.get_diplo(1, 2), Ok(1));
    before.setup.leaders[2].diplos[1] = 0;
    assert_eq!(before.get_diplo(1, 2), Ok(0));

    before.setup.players[1].team = 3;
    before.setup.players[2].team = 3;
    before.setup.frame = 0;
    assert_eq!(before.is_runtime_team(1, 2), Ok(true));
    before.setup.frame = 1;
    assert_eq!(before.is_runtime_team(1, 2), Ok(false));
}

#[test]
fn absent_flags_do_not_invent_a_gate_but_invalid_identity_and_indices_fail_closed() {
    let mut before = state();
    before.setup.leaders[1].leader_flags = 0;
    assert!(matches!(
        plan_diplomacy_command(&before, &wire13(TREATY_OPCODE, 1, 2, 0)),
        Ok(DiplomacyPlanDecision::Apply(_))
    ));

    before.setup.leaders[1].who = 7;
    assert_eq!(
        plan_diplomacy_command(&before, &wire13(TREATY_OPCODE, 1, 2, 0)),
        Err(DiplomacyPlanError::LeaderIdentityMismatch { slot: 1, who: 7 })
    );
    let before = state();
    assert_eq!(
        plan_diplomacy_command(&before, &wire17(TRIBUTE_OPCODE, 1, 2, 6, 1)),
        Err(DiplomacyPlanError::GoodOutOfRange { value: 6 })
    );
    assert_eq!(
        plan_diplomacy_command(&before, &wire17(PROPOSE_ATTACK_OPCODE, 1, 2, 8, 1)),
        Err(DiplomacyPlanError::AttackLeaderOutOfRange { value: 8 })
    );
}

#[test]
fn applied_receipt_recomputes_every_wire_and_state_field_while_boundaries_cannot_apply() {
    let before = state();
    let wire = wire13(TREATY_OPCODE, 1, 2, 4);
    let plan = applied(plan_diplomacy_command(&before, &wire).unwrap());
    let request = DiplomacyCommandRequest {
        before: before.clone(),
        wire: wire.clone(),
    };
    let receipt = DiplomacyCommandReceipt {
        request: request.clone(),
        status: DiplomacyTransactionStatus::Applied,
        plan: Some(plan.clone()),
    };
    assert!(receipt.validates(&request));

    let mut changed = request.clone();
    changed.wire[9..13].copy_from_slice(&5i32.to_le_bytes());
    let forged = DiplomacyCommandReceipt {
        request: changed.clone(),
        status: DiplomacyTransactionStatus::Applied,
        plan: Some(plan),
    };
    assert!(!forged.validates(&changed));

    let boundary_request = DiplomacyCommandRequest {
        before,
        wire: wire9(ACCEPT_OPCODE, 1, 2),
    };
    let forged_boundary = DiplomacyCommandReceipt {
        request: boundary_request.clone(),
        status: DiplomacyTransactionStatus::Applied,
        plan: receipt.plan,
    };
    assert!(!forged_boundary.validates(&boundary_request));
    assert!(
        DiplomacyCommandReceipt::unavailable(boundary_request.clone()).validates(&boundary_request)
    );

    let malformed_request = DiplomacyCommandRequest {
        before: state(),
        wire: vec![ACCEPT_OPCODE],
    };
    assert!(
        !DiplomacyCommandReceipt::unavailable(malformed_request.clone())
            .validates(&malformed_request)
    );
}
