// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../src/systems/leaders_diplomacy_opening_frontier.rs"]
mod subject;

use subject::*;

fn active(who: i32, score: i32) -> GlobalLeaderSlot {
    GlobalLeaderSlot {
        flags: LEADER_PLAYING,
        who,
        score,
        ..GlobalLeaderSlot::default()
    }
}

fn state(owner_who: i32) -> DiplomacyOpeningState {
    let mut state = DiplomacyOpeningState::default();
    state.owner.who = owner_who;
    for index in 0..RETAIL_LEADER_SLOTS {
        state.leaders[index].who = index as i32;
    }
    state
}

#[test]
fn pdb_extent_layout_and_instruction_frontier_are_frozen() {
    assert_eq!(LEADER_DIPLOMACY_VA, 0x006b_c950);
    assert_eq!(LEADER_DIPLOMACY_PDB_SIZE, 20_348);
    assert_eq!(
        (OPENING_BEGIN_VA, OPENING_END_VA),
        (0x006b_c96e, 0x006b_cb2c)
    );
    assert_eq!(
        (TARGET_LOOP_CONTINUE_VA, FUNCTION_RETURN_VA),
        (0x006b_e98a, 0x006b_e99a)
    );
    assert_eq!(LEADER_IS_ALLY_VA, 0x006e_db50);
    assert_eq!(LEADER_STRIDE, 0x6eec);
    assert_eq!(leader_va(0), 0x00e3_a390);
    assert_eq!(leader_va(7), 0x00e6_ac04);
    assert_eq!(
        (
            LEADER_FLAGS_OFFSET,
            LEADER_WHO_OFFSET,
            LEADER_SCORE_OFFSET,
            LEADER_DIPLOS_OFFSET,
            LEADER_TREATIES_OFFSET,
            LEADER_AGENDAS_OFFSET,
            LEADER_COUNTEROFFER_OFFSET,
            LEADER_TRIBUTE_DEMANDED_OFFSET,
            LEADER_DIP_OFFSET,
            DIPLOMACY_STRIDE,
        ),
        (0, 8, 0x18, 0x74, 0x94, 0xb4, 0x314, 0x334, 0x692c, 0x5c)
    );
}

#[test]
fn initial_gates_read_global_owner_and_preserve_machine_order() {
    let mut human = state(3);
    human.leaders[3].flags = LEADER_HUMAN;
    let before = human.clone();
    let trace = execute_diplomacy_opening(
        &mut human,
        GameFacts {
            check_victory_mode: true,
            ai_off: true,
            ..GameFacts::default()
        },
    )
    .unwrap();
    assert_eq!(trace.gate, GateOutcome::HumanOwner);
    assert_eq!(human, before);

    human.leaders[3].flags = 0;
    let trace = execute_diplomacy_opening(
        &mut human,
        GameFacts {
            check_victory_mode: true,
            ai_off: true,
            ..GameFacts::default()
        },
    )
    .unwrap();
    assert_eq!(trace.gate, GateOutcome::CheckVictoryMode);

    let trace = execute_diplomacy_opening(
        &mut human,
        GameFacts {
            ai_off: true,
            ..GameFacts::default()
        },
    )
    .unwrap();
    assert_eq!(trace.gate, GateOutcome::AiDisabled);
}

#[test]
fn ally_scan_is_mutual_counts_strictly_stronger_and_later_maximum_ties_win() {
    let mut state = state(2);
    state.leaders[0] = active(0, 90);
    state.leaders[1] = active(1, 120);
    state.leaders[2] = active(2, 100);
    state.leaders[3] = active(3, 120);
    state.leaders[4] = active(4, -5);
    state.leaders[5].flags = LEADER_VALID;

    state.leaders[0].diplos[2] = DIPLO_ALLIED;
    state.leaders[2].diplos[0] = DIPLO_ALLIED;
    state.leaders[1].diplos[2] = DIPLO_ALLIED;
    state.leaders[2].diplos[1] = 1;

    let trace = execute_diplomacy_opening(
        &mut state,
        GameFacts {
            frame: 1,
            ..GameFacts::default()
        },
    )
    .unwrap();
    let scan = trace.scan.unwrap();
    assert_eq!(scan.ally_reads.len(), 5);
    assert_eq!(scan.non_allies, 3);
    assert_eq!(scan.stronger_than_owner, 2);
    assert_eq!((scan.strongest_score, scan.strongest_slot), (120, 3));
    assert_eq!(
        scan.ally_reads[0].reason,
        AllyReason::MutualAllied {
            forward: 2,
            reciprocal: 2
        }
    );
    assert_eq!(
        scan.ally_reads[1].reason,
        AllyReason::NotMutual {
            forward: 2,
            reciprocal: Some(1)
        }
    );
    assert_eq!(scan.ally_reads[2].reason, AllyReason::SameWho);

    // One reciprocal relation bit is load-bearing: changing it changes only the ally count.
    state.leaders[2].diplos[1] = DIPLO_ALLIED;
    let scan = execute_diplomacy_opening(
        &mut state,
        GameFacts {
            frame: 1,
            ..GameFacts::default()
        },
    )
    .unwrap()
    .scan
    .unwrap();
    assert_eq!(scan.non_allies, 2);
    assert_eq!(scan.stronger_than_owner, 2);
}

#[test]
fn invalid_active_identity_fails_before_any_agenda_mutation() {
    let mut state = state(0);
    state.leaders[0] = active(0, 10);
    state.leaders[1] = active(8, 20);

    // The forward relation is retail's short circuit: with a non-allied value the invalid
    // reciprocal index is never owned or read.
    let short_circuit = execute_diplomacy_opening(
        &mut state,
        GameFacts {
            frame: 1,
            ..GameFacts::default()
        },
    )
    .unwrap();
    assert_eq!(short_circuit.scan.unwrap().non_allies, 1);

    // Changing only that directed value reaches the reciprocal read and must fail closed.
    state.leaders[1].diplos[0] = DIPLO_ALLIED;
    state.owner.treaties[1] = TREATY_ELIGIBLE;
    state.owner.agendas[1] = AGENDA_PENDING | 0x20;
    let before = state.clone();
    assert_eq!(
        execute_diplomacy_opening(&mut state, GameFacts::default()),
        Err(OpeningError::ActiveLeaderWhoOutsideArray {
            leader_index: 1,
            who: 8,
        })
    );
    assert_eq!(state, before);
}

#[test]
fn target_gate_order_separates_process_owner_and_treaty_bits() {
    let mut state = state(1);
    state.leaders[1] = active(1, 0);
    state.leaders[2].flags = LEADER_VALID;
    state.leaders[3] = active(3, 0);
    state.leaders[4] = active(4, 0);
    state.owner.treaties[4] = TREATY_ELIGIBLE | TREATY_BLOCKED;

    let trace = execute_diplomacy_opening(
        &mut state,
        GameFacts {
            frame: 1,
            ..GameFacts::default()
        },
    )
    .unwrap();
    assert_eq!(
        trace.targets[1].outcome,
        TargetOutcome::Skipped(TargetSkip::OwnerSlot)
    );
    assert_eq!(
        trace.targets[2].outcome,
        TargetOutcome::Skipped(TargetSkip::ProcessFlagClear)
    );
    assert_eq!(
        trace.targets[3].outcome,
        TargetOutcome::Skipped(TargetSkip::TreatyInactive)
    );
    assert_eq!(
        trace.targets[4].outcome,
        TargetOutcome::Skipped(TargetSkip::TreatyBlocked)
    );
    assert!(trace.local_mutations.is_empty());
    assert!(trace.residual.is_none());
}

#[test]
fn period_selection_pins_agree_tribute_counteroffer_and_agenda_bit_eight() {
    assert_eq!(target_period(1, 1, 1, AGENDA_SLOW_CADENCE), 0x800);
    assert_eq!(target_period(0, 0, 0, AGENDA_SLOW_CADENCE), 0x800);
    assert_eq!(target_period(0, 1, 0, AGENDA_SLOW_CADENCE), 0x2000);
    assert_eq!(target_period(0, 1, 1, AGENDA_SLOW_CADENCE), 0x4000);
    assert_eq!(target_period(0, 1, 1, 0), 0x1000);
    assert_eq!(target_period(0, 0, 1, 0), 0x400);
    assert_eq!(target_period(1, 1, 1, 0), 0x200);
}

#[test]
fn cadence_uses_owner_target_phase_and_wrapping_dword_addition() {
    assert_eq!(target_phase_word(0, 2, 3, 0x800), 19 * 32);
    assert_eq!(target_phase_word(u32::MAX - 31, 0, 1, 0x800), 0);

    let mut state = state(2);
    state.leaders[2] = active(2, 0);
    state.leaders[3] = active(3, 0);
    state.owner.treaties[3] = TREATY_ELIGIBLE;
    state.owner.dip_agree[3] = 1;
    state.owner.agendas[3] = AGENDA_SLOW_CADENCE;

    let miss = execute_diplomacy_opening(
        &mut state,
        GameFacts {
            frame: 1,
            ..GameFacts::default()
        },
    )
    .unwrap();
    assert!(matches!(
        miss.targets[3].outcome,
        TargetOutcome::Skipped(TargetSkip::CadenceMiss { period: 0x800, .. })
    ));

    let due_frame = 0u32.wrapping_sub(19 * 32);
    let due = execute_diplomacy_opening(
        &mut state,
        GameFacts {
            frame: due_frame,
            ..GameFacts::default()
        },
    )
    .unwrap();
    assert!(matches!(
        due.targets[3].outcome,
        TargetOutcome::ReachedPolicy {
            trigger: PolicyTrigger::CadenceDue {
                period: 0x800,
                phase_word: 0
            },
            ..
        }
    ));
}

#[test]
fn pending_agenda_clears_exact_bit_then_stops_at_typed_policy_residual() {
    let mut state = state(0);
    state.leaders[0] = active(0, 10);
    state.leaders[2] = active(2, 20);
    state.owner.treaties[2] = TREATY_ELIGIBLE;
    state.owner.agendas[2] = AGENDA_PENDING | AGENDA_SLOW_CADENCE | 0x20;

    let trace = execute_diplomacy_opening(&mut state, GameFacts::default()).unwrap();
    assert_eq!(state.owner.agendas[2], AGENDA_SLOW_CADENCE | 0x20);
    assert_eq!(
        trace.targets[2].outcome,
        TargetOutcome::ReachedPolicy {
            trigger: PolicyTrigger::PendingAgenda,
            agenda_before: 0x2c,
            agenda_after: 0x28,
        }
    );
    assert_eq!(
        trace.local_mutations,
        vec![AgendaMutationReceipt {
            instruction_va: 0x006b_cb2a,
            target_index: 2,
            before: 0x2c,
            after: 0x28,
        }]
    );
    assert_eq!(
        trace.residual,
        Some(OpenResidual::DownstreamPolicy {
            target_index: 2,
            first_unowned_va: 0x006b_cb2c,
        })
    );
    assert_eq!(
        trace.targets.len(),
        3,
        "later targets cannot run ahead of the residual"
    );
}

#[test]
fn clearing_the_pending_bit_changes_reachability_without_mutating_on_a_miss() {
    let mut pending = state(0);
    pending.leaders[0] = active(0, 0);
    pending.leaders[1] = active(1, 0);
    pending.owner.treaties[1] = TREATY_ELIGIBLE;
    pending.owner.agendas[1] = AGENDA_PENDING | AGENDA_SLOW_CADENCE;

    let mut ordinary = pending.clone();
    ordinary.owner.agendas[1] &= !AGENDA_PENDING;
    let ordinary_before = ordinary.clone();
    let ordinary_trace = execute_diplomacy_opening(
        &mut ordinary,
        GameFacts {
            frame: 1,
            ..GameFacts::default()
        },
    )
    .unwrap();
    assert!(matches!(
        ordinary_trace.targets[1].outcome,
        TargetOutcome::Skipped(TargetSkip::CadenceMiss { .. })
    ));
    assert_eq!(ordinary, ordinary_before);

    let pending_trace = execute_diplomacy_opening(
        &mut pending,
        GameFacts {
            frame: 1,
            ..GameFacts::default()
        },
    )
    .unwrap();
    assert!(matches!(
        pending_trace.targets[1].outcome,
        TargetOutcome::ReachedPolicy {
            trigger: PolicyTrigger::PendingAgenda,
            ..
        }
    ));
    assert_eq!(pending.owner.agendas[1], AGENDA_SLOW_CADENCE);
}
