// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../src/systems/leaders_end_process_step17.rs"]
mod subject;

use subject::*;

fn product(frame: i32) -> ProductFacts<'static> {
    ProductFacts {
        frame,
        console_who: 0,
        console_play: 0,
        pop_limit_index: 0,
        population_limits: &SHIPPED_POP_LIMITS,
    }
}

fn active(who: i32) -> LeaderSlot {
    LeaderSlot {
        flags: LEADER_PROCESS_FLAG,
        who,
        ..LeaderSlot::default()
    }
}

#[test]
fn pdb_layout_and_machine_loop_cardinality_are_pinned() {
    assert_eq!(
        (END_PROCESS_ALL_VA, END_PROCESS_ALL_SIZE),
        (0x006e_d070, 549)
    );
    assert_eq!(RETAIL_LEADER_SLOTS, 8);
    assert_eq!(leader_va(0), 0x00e3_a390);
    assert_eq!(leader_va(7), 0x00e6_ac04);
    assert_eq!(leader_va(8) + LEADER_WHO_OFFSET, 0x00e7_1af8);
    assert_eq!(LEADER_STRIDE, 0x6eec);
    assert_eq!(
        (
            LEADER_WHO_OFFSET,
            LEADER_POP_CAP_OFFSET,
            LEADER_POP_ISSUES_OFFSET,
        ),
        (0x08, 0x7e4, 0x7e8)
    );
    assert_eq!(
        (
            GAME_PLAYER_ARRAY_OFFSET,
            PLAYER_STRIDE,
            PLAYER_POP_CAP_FRAME_OFFSET,
            PLAYER_FLAGS_OFFSET,
            PLAYER_WHO_OFFSET,
        ),
        (0x44, 0x8c, 0x2c, 0x30, 0x33)
    );
    assert_eq!(static_player_va(0), 0x00e3_7f04);
    assert_eq!(static_player_va(7), 0x00e3_82d8);
    assert_eq!(
        (GAME_POP_LIMIT_INDEX_OFFSET, GAME_FRAME_OFFSET),
        (0x31, 0x550)
    );
    assert_eq!((CONSOLE_WHO_OFFSET, CONSOLE_PLAY_OFFSET), (0x298, 0x2a0));
    assert_eq!(
        (
            POP_LIMITS_LIST_OFFSET,
            CATEGORY_STRIDE,
            CATEGORY_DATA_OFFSET
        ),
        (0x10, 0x58, 0x3c)
    );
}

#[test]
fn visits_all_eight_slots_in_address_order_even_when_every_gate_is_clear() {
    let mut state = Step17State::default();
    let trace = execute_step17(&mut state, product(123));
    assert_eq!(trace.visits.len(), 8);
    for (index, visit) in trace.visits.iter().enumerate() {
        assert_eq!(visit.leader_index, index);
        assert_eq!(visit.leader_va, leader_va(index));
        assert_eq!(visit.outcome, LeaderOutcome::ProcessFlagClear);
    }
    assert_eq!(trace.total_sequenced_effects, 0);
}

#[test]
fn zero_issues_scans_players_zero_through_seven_and_stores_masked_flags() {
    let mut state = Step17State::default();
    state.leaders[3] = active(5);
    state.players[0] = PlayerRecord {
        flags: PLAYER_VALID_FLAG | PLAYER_POP_CAP_WARNING_FLAG | 0x4000,
        who: 5,
        ..PlayerRecord::default()
    };
    state.players[1] = PlayerRecord {
        flags: PLAYER_POP_CAP_WARNING_FLAG,
        who: 5,
        ..PlayerRecord::default()
    };
    state.players[2] = PlayerRecord {
        flags: PLAYER_VALID_FLAG | 0x2000,
        who: 5,
        ..PlayerRecord::default()
    };
    state.players[7] = PlayerRecord {
        flags: PLAYER_VALID_FLAG | PLAYER_POP_CAP_WARNING_FLAG,
        who: 4,
        ..PlayerRecord::default()
    };

    let trace = execute_step17(&mut state, product(0));
    assert_eq!(
        trace.visits[3].outcome,
        LeaderOutcome::WarningScan { stores: 2 }
    );
    assert_eq!(
        trace.local_mutations,
        vec![
            LocalMutationReceipt {
                sequence: 0,
                mutation: LocalMutation::PlayerFlags {
                    leader_index: 3,
                    player_index: 0,
                    before: 0x4801,
                    after: 0x4001,
                },
            },
            LocalMutationReceipt {
                sequence: 1,
                mutation: LocalMutation::PlayerFlags {
                    leader_index: 3,
                    player_index: 2,
                    before: 0x2001,
                    after: 0x2001,
                },
            },
        ]
    );
    assert_eq!(state.players[0].flags, 0x4001);
    assert_eq!(state.players[1].flags, 0x0800);
    assert_eq!(state.players[2].flags, 0x2001);
    assert_eq!(state.players[7].flags, 0x0801);
}

#[test]
fn nonlocal_population_issues_have_no_mutation_or_host_tail() {
    let mut state = Step17State::default();
    state.leaders[0] = LeaderSlot {
        pop_issues: 1,
        ..active(7)
    };
    let trace = execute_step17(
        &mut state,
        ProductFacts {
            console_who: 6,
            ..product(10_000)
        },
    );
    assert_eq!(
        trace.visits[0].outcome,
        LeaderOutcome::NonLocalPopulationIssue
    );
    assert!(trace.local_mutations.is_empty());
    assert!(trace.product_reads.is_empty());
    assert!(trace.host_tails.is_empty());
}

#[test]
fn signed_wrapping_frame_gate_skips_449_and_runs_at_450() {
    let mut early = Step17State::default();
    early.leaders[0] = LeaderSlot {
        pop_issues: 1,
        ..active(0)
    };
    early.players[0].pop_cap_frame = 100;
    let early_trace = execute_step17(&mut early, product(549));
    assert_eq!(
        early_trace.visits[0].outcome,
        LeaderOutcome::FeedbackRateLimited { elapsed: 449 }
    );
    assert_eq!(early.players[0].pop_cap_frame, 100);

    let mut due = early.clone();
    let due_trace = execute_step17(&mut due, product(550));
    assert_eq!(due.players[0].pop_cap_frame, 550);
    assert_eq!(
        due_trace.local_mutations[0],
        LocalMutationReceipt {
            sequence: 0,
            mutation: LocalMutation::PlayerPopCapFrame {
                leader_index: 0,
                player_index: 0,
                before: 100,
                after: 550,
            },
        }
    );

    let mut wrapped = Step17State::default();
    wrapped.leaders[0] = LeaderSlot {
        pop_issues: 1,
        ..active(0)
    };
    wrapped.players[0].pop_cap_frame = i32::MAX;
    let wrapped_trace = execute_step17(&mut wrapped, product(i32::MIN));
    assert_eq!(
        wrapped_trace.visits[0].outcome,
        LeaderOutcome::FeedbackRateLimited { elapsed: 1 }
    );
}

#[test]
fn due_frame_is_stamped_before_missing_product_category_is_reported() {
    let mut state = Step17State::default();
    state.leaders[2] = LeaderSlot {
        pop_issues: 3,
        pop_cap: 0,
        ..active(9)
    };
    state.players[4].pop_cap_frame = 10;
    let trace = execute_step17(
        &mut state,
        ProductFacts {
            frame: 460,
            console_who: 9,
            console_play: 4,
            pop_limit_index: 7,
            population_limits: &SHIPPED_POP_LIMITS,
        },
    );
    assert_eq!(state.players[4].pop_cap_frame, 460);
    assert_eq!(trace.local_mutations[0].sequence, 0);
    assert_eq!(
        trace.residuals,
        vec![ResidualReceipt {
            sequence: 1,
            leader_index: 2,
            residual: OpenResidual::PopulationLimitRowUnavailable {
                index: 7,
                supplied_rows: 6,
            },
        }]
    );
    assert_eq!(
        trace.visits[2].outcome,
        LeaderOutcome::PopulationLimitUnavailable
    );
    assert!(trace.host_tails.is_empty());
}

#[test]
fn cap_compare_happens_after_stamp_and_product_read() {
    let mut state = Step17State::default();
    state.leaders[0] = LeaderSlot {
        pop_issues: 1,
        pop_cap: 75,
        ..active(0)
    };
    let trace = execute_step17(
        &mut state,
        ProductFacts {
            frame: 450,
            pop_limit_index: 1,
            ..product(450)
        },
    );
    assert_eq!(state.players[0].pop_cap_frame, 450);
    assert_eq!(trace.local_mutations[0].sequence, 0);
    assert_eq!(
        trace.product_reads,
        vec![ProductReadReceipt {
            sequence: 1,
            leader_index: 0,
            category_index: 1,
            value: 75,
        }]
    );
    assert_eq!(
        trace.visits[0].outcome,
        LeaderOutcome::AtOrAbovePopulationLimit { limit: 75 }
    );
    assert!(trace.host_tails.is_empty());
}

#[test]
fn below_cap_host_tail_has_exact_calls_arguments_and_global_sequence() {
    let mut state = Step17State::default();
    state.leaders[0] = LeaderSlot {
        pop_issues: 1,
        pop_cap: 74,
        ..active(0)
    };
    state.leaders[1] = active(1);
    state.players[1] = PlayerRecord {
        flags: PLAYER_VALID_FLAG | PLAYER_POP_CAP_WARNING_FLAG,
        who: 1,
        ..PlayerRecord::default()
    };
    let trace = execute_step17(
        &mut state,
        ProductFacts {
            frame: 450,
            pop_limit_index: 1,
            ..product(450)
        },
    );

    assert_eq!(trace.local_mutations[0].sequence, 0);
    assert_eq!(trace.product_reads[0].sequence, 1);
    assert_eq!(
        trace.host_tails,
        vec![
            HostTailReceipt {
                sequence: 2,
                leader_index: 0,
                host_tail: HostTail::CopyLocalizedString {
                    constructor_va: 0x00a1_d590,
                    text_offset: 0xc878,
                },
            },
            HostTailReceipt {
                sequence: 3,
                leader_index: 0,
                host_tail: HostTail::MessageWinAddFeedback {
                    call_va: 0x007e_9ab0,
                    duration: -1,
                    color_va: 0x00c8_d248,
                },
            },
            HostTailReceipt {
                sequence: 4,
                leader_index: 0,
                host_tail: HostTail::SoundGlobalPlay {
                    call_va: 0x0097_f770,
                    category: 0x5b,
                },
            },
        ]
    );
    assert_eq!(trace.local_mutations[1].sequence, 5);
    assert_eq!(
        trace.local_mutations[1].mutation,
        LocalMutation::PlayerFlags {
            leader_index: 1,
            player_index: 1,
            before: 0x0801,
            after: 0x0001,
        }
    );
    assert_eq!(trace.total_sequenced_effects, 6);
}

#[test]
fn invalid_console_play_fails_typed_without_inventing_a_player() {
    let mut state = Step17State::default();
    state.leaders[5] = LeaderSlot {
        pop_issues: 1,
        ..active(3)
    };
    let before = state.clone();
    let trace = execute_step17(
        &mut state,
        ProductFacts {
            console_who: 3,
            console_play: -1,
            ..product(999)
        },
    );
    assert_eq!(state, before);
    assert_eq!(
        trace.residuals,
        vec![ResidualReceipt {
            sequence: 0,
            leader_index: 5,
            residual: OpenResidual::ConsolePlayOutsidePlayerArray { play: -1 },
        }]
    );
    assert_eq!(
        trace.visits[5].outcome,
        LeaderOutcome::LocalPlayerUnavailable
    );
}

#[test]
fn shipped_population_rows_and_host_constants_are_exact() {
    assert_eq!(SHIPPED_POP_LIMITS, [50, 75, 100, 125, 150, 200]);
    assert_eq!(POP_WARNING_MIN_ELAPSED, 450);
    assert_eq!(PLAYER_POP_CAP_WARNING_FLAG, 0x0800);
    assert_eq!(POP_WARNING_SOUND_CATEGORY, 0x5b);
    assert_eq!(POP_WARNING_TEXT_OFFSET, 0xc878);
    assert_eq!(RED_COLOR_VA, 0x00c8_d248);
}
