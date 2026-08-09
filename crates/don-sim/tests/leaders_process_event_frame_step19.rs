// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../src/systems/leaders_process_event_frame_step19.rs"]
mod subject;

use subject::*;

fn encrypted_age(age: i32) -> u32 {
    (age as u32) ^ AGES_XOR_KEY
}

fn product(frame: i32) -> ProductFacts {
    ProductFacts {
        frame,
        console_who: -1,
        team_scores: [Some(0); RETAIL_LEADER_SLOTS],
        encrypted_ages: [Some(encrypted_age(0)); RETAIL_LEADER_SLOTS],
    }
}

fn active(who: i32) -> LeaderSlot {
    LeaderSlot {
        flags: LEADER_IN_GAME_FLAG,
        who,
        ..LeaderSlot::default()
    }
}

fn team_score_reads(trace: &Step19Trace) -> Vec<(usize, usize, i32)> {
    trace
        .product_reads
        .iter()
        .filter_map(|receipt| match receipt.read {
            ProductRead::TeamScore {
                owner_leader_index,
                queried_leader_index,
                value,
                ..
            } => Some((owner_leader_index, queried_leader_index, value)),
            ProductRead::EncryptedAge { .. } => None,
        })
        .collect()
}

#[test]
fn pdb_layout_dispatcher_and_exact_eight_record_walk_are_pinned() {
    assert_eq!(
        (PROCESS_EVENT_FRAME_VA, PROCESS_EVENT_FRAME_SIZE),
        (0x006e_c180, 918)
    );
    assert_eq!(
        (DISPATCHER_VA, DISPATCH_CALL_VA),
        (0x0059_24a0, 0x0059_24ac)
    );
    assert_eq!(RETAIL_LEADER_SLOTS, 8);
    assert_eq!(LEADER_STRIDE, 0x6eec);
    assert_eq!(leader_va(0), 0x00e3_a390);
    assert_eq!(leader_va(7), 0x00e6_ac04);
    assert_eq!(leader_va(8), LEADERS_END_VA);
    assert_eq!(LEADER_IN_GAME_FLAG, 1);
    assert_eq!(
        (
            LEADER_WHO_OFFSET,
            LEADER_DIPLOS_OFFSET,
            LEADER_FRAME_BATTLE_OFFSET,
            LEADER_AVERAGE_DEATH_RATE_OFFSET,
            LEADER_AVERAGE_KILL_RATE_OFFSET,
            LEADER_AVERAGE_DAMAGE_RATE_OFFSET,
            LEADER_AVERAGE_HIT_RATE_OFFSET,
        ),
        (0x08, 0x74, 0x0a4c, 0x0a50, 0x0a52, 0x0a54, 0x0a56)
    );
    assert_eq!(
        (
            LEADER_CURRENT_EVENTS_OFFSET,
            LEADER_FIFTEEN_SECOND_EVENTS_OFFSET,
            LEADER_DATA_ENCRYPTED_OFFSET,
            ENCRYPTED_AGES_OFFSET,
        ),
        (0x0a58, 0x0a60, 0x6eb8, 0x00dc)
    );
    assert_eq!((GAME_FRAME_OFFSET, CONSOLE_WHO_OFFSET), (0x550, 0x298));
}

#[test]
fn visits_all_eight_slots_in_address_order_and_separates_gate_from_cadence() {
    let mut clear = Step19State::default();
    let clear_trace = execute_step19(&mut clear, product(0));
    for (index, visit) in clear_trace.visits.iter().enumerate() {
        assert_eq!(visit.leader_index, index);
        assert_eq!(visit.leader_va, leader_va(index));
        assert_eq!(visit.outcome, LeaderOutcome::InGameFlagClear);
    }
    assert_eq!(clear_trace.total_sequenced_effects, 0);

    let mut not_due = Step19State::default();
    not_due.leaders[4] = active(4);
    not_due.leaders[4].event_queue.deaths_current_frame = 9;
    let before = not_due.clone();
    let not_due_trace = execute_step19(&mut not_due, product(-1));
    assert_eq!(not_due_trace.visits[4].outcome, LeaderOutcome::FrameNotDue);
    assert_eq!(not_due, before);
    assert_eq!(not_due_trace.total_sequenced_effects, 0);
}

#[test]
fn due_fold_accumulates_raw_events_wraps_words_decays_and_clears_last() {
    let mut state = Step19State::default();
    state.leaders[0] = active(0);
    state.leaders[0].event_queue = EventQueueState {
        average_death_rate: 10,
        average_kill_rate: 8,
        average_damage_rate: 100,
        average_hit_rate: 16,
        deaths_current_frame: 1,
        kills_current_frame: 0,
        hits_current_frame: 16_384,
        damage_current_frame: u16::MAX,
        deaths_fifteen_seconds: u16::MAX,
        kills_fifteen_seconds: 2,
        hits_fifteen_seconds: 60_000,
        damage_fifteen_seconds: 1,
        ..EventQueueState::default()
    };

    let trace = execute_step19(&mut state, product(0));
    let LocalMutation::FoldEventQueue { before, after, .. } = trace.local_mutations[0].mutation
    else {
        panic!("first mutation must be the queue fold");
    };
    assert_eq!(before.current_events(), [1, 0, 16_384, u16::MAX]);
    assert_eq!(after.current_events(), [100, 0, 0, 65_436]);
    assert_eq!(after.fifteen_second_events(), [0, 2, 10_848, 0]);
    assert_eq!(
        (
            after.average_death_rate,
            after.average_kill_rate,
            after.average_hit_rate,
            after.average_damage_rate,
        ),
        (55, 7, 14, 32_768)
    );
    assert_eq!(state.leaders[0].event_queue.current_events(), [0; 4]);
    assert_eq!(
        trace.local_mutations.last().unwrap().mutation,
        LocalMutation::ClearCurrentEvents {
            leader_index: 0,
            before: [100, 0, 0, 65_436],
            after: [0; 4],
        }
    );
    assert_eq!(
        trace.product_reads[0],
        ProductReadReceipt {
            sequence: 1,
            read: ProductRead::EncryptedAge {
                owner_leader_index: 0,
                who_index: 0,
                encrypted: encrypted_age(0),
                xor_key: AGES_XOR_KEY,
                decoded: 0,
            },
        }
    );
    assert_eq!(trace.total_sequenced_effects, 3);
}

#[test]
fn quiet_dead_band_and_active_boundaries_match_unsigned_combat_sum() {
    let mut quiet = Step19State::default();
    quiet.leaders[0] = active(0);
    quiet.music.current_mood = mood::WINNING;
    quiet.music.next_mood = 9;
    let mut quiet_product = product(0);
    quiet_product.console_who = 0;
    let quiet_trace = execute_step19(&mut quiet, quiet_product);
    assert_eq!(quiet.music.next_mood, mood::QUIET);
    assert_eq!(
        quiet_trace.host_tails,
        vec![HostTailReceipt {
            sequence: 2,
            leader_index: 0,
            host_tail: HostTail::JukeBoxSetNextMood {
                call_va: JUKEBOX_SET_NEXT_MOOD_VA,
                requested_mood: mood::QUIET,
                force: true,
            },
        }]
    );

    let mut dead_band = Step19State::default();
    dead_band.leaders[0] = active(0);
    dead_band.leaders[0].event_queue.hits_current_frame = 6;
    dead_band.music.next_mood = 7;
    let mut dead_band_product = product(0);
    dead_band_product.console_who = 0;
    let dead_band_trace = execute_step19(&mut dead_band, dead_band_product);
    assert_eq!(dead_band.leaders[0].event_queue.average_hit_rate, 300);
    assert_eq!(dead_band.music.next_mood, 7);
    assert!(dead_band_trace.host_tails.is_empty());
    assert_eq!(team_score_reads(&dead_band_trace), vec![]);

    let mut active_state = Step19State::default();
    active_state.leaders[0] = active(0);
    active_state.leaders[0].event_queue.hits_current_frame = 12;
    let mut active_product = product(0);
    active_product.console_who = 0;
    let active_trace = execute_step19(&mut active_state, active_product);
    assert_eq!(active_state.leaders[0].event_queue.average_hit_rate, 600);
    assert_eq!(team_score_reads(&active_trace), vec![(0, 0, 0)]);
    assert_eq!(active_state.music.next_mood, mood::WINNING);
}

#[test]
fn same_current_mood_still_receives_global_store_but_suppresses_jukebox_call() {
    let mut state = Step19State::default();
    state.leaders[0] = active(0);
    state.music.current_mood = mood::QUIET;
    state.music.next_mood = 17;
    let mut facts = product(0);
    facts.console_who = 0;
    let trace = execute_step19(&mut state, facts);

    assert_eq!(state.music.next_mood, mood::QUIET);
    assert!(trace.host_tails.is_empty());
    assert!(trace.local_mutations.iter().any(|receipt| {
        receipt.mutation
            == LocalMutation::NextMusicMood {
                leader_index: 0,
                before: 17,
                after: mood::QUIET,
            }
    }));
}

#[test]
fn sequential_dispatch_observes_folded_earlier_enemy_but_not_later_enemy() {
    let mut state = Step19State::default();
    state.leaders[0] = active(0);
    state.leaders[0].event_queue.hits_current_frame = 2;
    state.leaders[1] = active(1);
    state.leaders[1].event_queue.hits_current_frame = 12;
    state.leaders[2] = active(2);
    state.leaders[2].event_queue.hits_current_frame = 2;

    let mut facts = product(0);
    facts.console_who = 1;
    facts.team_scores[0] = Some(50);
    facts.team_scores[1] = Some(10);
    facts.team_scores[2] = Some(100);
    let trace = execute_step19(&mut state, facts);

    assert_eq!(team_score_reads(&trace), vec![(1, 1, 10), (1, 0, 50)]);
    assert_eq!(state.leaders[0].event_queue.average_hit_rate, 100);
    assert_eq!(state.leaders[2].event_queue.average_hit_rate, 100);
}

#[test]
fn reciprocal_diplomacy_indexes_leaders_by_who_not_current_record_identity() {
    let mut state = Step19State::default();
    state.leaders[0] = active(0);
    state.leaders[0].event_queue.average_hit_rate = 10;
    state.leaders[0].diplos[1] = 1;

    // Record 3 is the local dispatcher object, but retail addresses leaders[who == 1]
    // for the reciprocal relation. Make those two records intentionally disagree.
    state.leaders[1].who = 7;
    state.leaders[1].diplos[0] = 1;
    state.leaders[3] = active(1);
    state.leaders[3].diplos[0] = 0;
    state.leaders[3].event_queue.hits_current_frame = 12;

    let mut facts = product(0);
    facts.console_who = 1;
    facts.team_scores[0] = Some(900);
    facts.team_scores[3] = Some(100);
    let trace = execute_step19(&mut state, facts);

    assert_eq!(team_score_reads(&trace), vec![(3, 3, 100)]);
    assert_eq!(state.music.next_mood, mood::WINNING);
}

#[test]
fn score_bias_uses_exact_four_thirds_and_three_quarters_boundaries() {
    fn run(own_score: i32) -> i32 {
        let mut state = Step19State::default();
        state.leaders[0] = active(0);
        state.leaders[0].event_queue.hits_current_frame = 12;
        state.leaders[0].event_queue.damage_current_frame = 16;
        state.leaders[1] = active(1);
        state.leaders[1].event_queue.average_hit_rate = 1;
        let mut facts = product(0);
        facts.console_who = 0;
        facts.team_scores[0] = Some(own_score);
        facts.team_scores[1] = Some(100);
        execute_step19(&mut state, facts);
        state.music.next_mood
    }

    // Folded local balance is 600 - 800 = -200. At 4/3 of 100, +200 reaches zero.
    assert_eq!(run(133), mood::WINNING);
    assert_eq!(run(132), mood::LOSING);
    // 3/4 is inclusive and subtracts another 200.
    assert_eq!(run(75), mood::LOSING);
}

#[test]
fn leaving_quiet_for_two_thousand_rate_sets_force_on_the_typed_tail() {
    let mut state = Step19State::default();
    state.leaders[0] = active(0);
    state.leaders[0].event_queue.hits_current_frame = 39;
    state.leaders[0].event_queue.damage_current_frame = 1;
    state.music.current_mood = mood::QUIET;
    let mut facts = product(0);
    facts.console_who = 0;
    let trace = execute_step19(&mut state, facts);

    assert_eq!(
        trace.host_tails[0].host_tail,
        HostTail::JukeBoxSetNextMood {
            call_va: JUKEBOX_SET_NEXT_MOOD_VA,
            requested_mood: mood::WINNING,
            force: true,
        }
    );
}

#[test]
fn age_is_decrypted_before_threshold_and_battle_tail_precedes_sentinels_and_clear() {
    let mut state = Step19State::default();
    state.leaders[0] = active(0);
    state.leaders[0].event_queue.deaths_current_frame = 4;
    state.leaders[0].event_queue.kills_current_frame = 2;
    let trace = execute_step19(&mut state, product(50));

    assert_eq!(
        trace.product_reads[0],
        ProductReadReceipt {
            sequence: 1,
            read: ProductRead::EncryptedAge {
                owner_leader_index: 0,
                who_index: 0,
                encrypted: encrypted_age(0),
                xor_key: AGES_XOR_KEY,
                decoded: 0,
            },
        }
    );
    assert_eq!(
        trace.host_tails[0],
        HostTailReceipt {
            sequence: 2,
            leader_index: 0,
            host_tail: HostTail::AchieveAddEvent {
                call_va: ACHIEVE_ADD_EVENT_VA,
                kind: BattleEventKind::DeathsOverKills,
                who: 0,
                empty_string_va: EMPTY_STRING_VA,
            },
        }
    );
    assert_eq!(trace.local_mutations[1].sequence, 3);
    assert_eq!(
        trace.local_mutations[1].mutation,
        LocalMutation::BattleRateSentinels {
            leader_index: 0,
            before: [200, 100],
            after: [BATTLE_RATE_SENTINEL; 2],
        }
    );
    assert_eq!(trace.local_mutations[2].sequence, 4);
    assert_eq!(
        trace.local_mutations[2].mutation,
        LocalMutation::BattleFrameStamp {
            leader_index: 0,
            before: 0,
            after: 50,
        }
    );
    assert_eq!(trace.local_mutations[3].sequence, 5);
    assert_eq!(
        state.leaders[0].event_queue.average_death_rate,
        BATTLE_RATE_SENTINEL
    );
    assert_eq!(
        state.leaders[0].event_queue.average_kill_rate,
        BATTLE_RATE_SENTINEL
    );
    assert_eq!(state.leaders[0].event_queue.frame_battle, 50);
}

#[test]
fn signed_wrapping_cooldown_is_strictly_less_than_eighteen_hundred() {
    fn run(previous: i32) -> Option<BattleEventKind> {
        let mut state = Step19State::default();
        state.leaders[0] = active(0);
        state.leaders[0].event_queue.frame_battle = previous;
        state.leaders[0].event_queue.deaths_current_frame = 4;
        state.leaders[0].event_queue.kills_current_frame = 2;
        let trace = execute_step19(&mut state, product(2_000));
        match trace.visits[0].outcome {
            LeaderOutcome::Due { battle_event, .. } => battle_event,
            outcome => panic!("unexpected outcome: {outcome:?}"),
        }
    }

    assert_eq!(run(201), None);
    assert_eq!(run(200), Some(BattleEventKind::DeathsOverKills));

    let mut wrapped = Step19State::default();
    wrapped.leaders[0] = active(0);
    wrapped.leaders[0].event_queue.frame_battle = i32::MAX;
    wrapped.leaders[0].event_queue.deaths_current_frame = 4;
    wrapped.leaders[0].event_queue.kills_current_frame = 2;
    // `i32::MIN + 48` is divisible by 50; wrapping elapsed from MAX is only 49.
    let trace = execute_step19(&mut wrapped, product(i32::MIN + 48));
    assert_eq!(
        trace.visits[0].outcome,
        LeaderOutcome::Due {
            requested_mood: None,
            battle_event: None,
            residuals: 0,
        }
    );
}

#[test]
fn unavailable_product_facts_emit_residuals_without_fabricating_tails_or_state() {
    let mut state = Step19State::default();
    state.leaders[0] = active(0);
    state.leaders[0].event_queue.hits_current_frame = 12;
    state.music.next_mood = 77;
    let facts = ProductFacts {
        frame: 0,
        console_who: 0,
        team_scores: [None; RETAIL_LEADER_SLOTS],
        encrypted_ages: [None; RETAIL_LEADER_SLOTS],
    };
    let trace = execute_step19(&mut state, facts);

    assert_eq!(
        trace.residuals,
        vec![
            ResidualReceipt {
                sequence: 1,
                leader_index: 0,
                residual: OpenResidual::TeamScoreUnavailable {
                    queried_leader_index: 0,
                },
            },
            ResidualReceipt {
                sequence: 2,
                leader_index: 0,
                residual: OpenResidual::EncryptedAgeUnavailable { who_index: 0 },
            },
        ]
    );
    assert!(trace.host_tails.is_empty());
    assert_eq!(state.music.next_mood, 77);
    assert_eq!(state.leaders[0].event_queue.current_events(), [0; 4]);
    assert_eq!(
        trace.visits[0].outcome,
        LeaderOutcome::Due {
            requested_mood: None,
            battle_event: None,
            residuals: 2,
        }
    );
}
