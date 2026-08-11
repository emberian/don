// SPDX-License-Identifier: GPL-3.0-or-later
//! Mutation pins for the recovered `Player::resign` / `Player::quit` / `Player::drop` /
//! `DropControl::process_drop` bodies.
//!
//! Every assertion below is a transcription pin against the capstone disassembly cited in
//! the module. None of it has been executed against retail; a green run here is Tier C
//! evidence that the port matches the read instruction stream, nothing more.

#[path = "../src/systems/player_lifecycle_tails.rs"]
mod lifecycle;

use lifecycle::*;

fn image() -> LifecycleImage {
    let mut img = LifecycleImage::default();
    for slot in 0..PLAYER_SLOTS {
        img.players[slot].play = slot as u8;
        img.players[slot].who = slot as u8;
        img.players[slot].team = 0;
        img.players[slot].flags = 0;
        img.leaders[slot].who = slot as i32;
    }
    img
}

/// A two-human match: players 0 and 1 present, on leaders 0 and 1, both leaders valid+active.
fn duel() -> LifecycleImage {
    let mut img = image();
    for slot in 0..2 {
        img.players[slot].flags = PLAYER_PRESENT | PLAYER_LEAVE_SCAN_REQUIRED;
        img.leaders[slot].leader_flags = LEADER_VALID_ACTIVE | LEADER_HUMAN;
    }
    img.console_play = 1;
    img.console_who = 1;
    img
}

fn calls(plan: &LifecyclePlan) -> Vec<LifecycleCall> {
    LifecycleReceipt::required_calls(plan)
}

fn sounds(plan: &LifecyclePlan) -> Vec<i32> {
    plan.effects
        .iter()
        .filter_map(|e| match e {
            LifecycleEffect::Presentation(LifecyclePresentation::Sound { category }) => {
                Some(*category)
            }
            _ => None,
        })
        .collect()
}

fn presentations(plan: &LifecyclePlan) -> Vec<LifecyclePresentation> {
    plan.effects
        .iter()
        .filter_map(|e| match e {
            LifecycleEffect::Presentation(p) => Some(*p),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The two co-tenant scans
// ---------------------------------------------------------------------------

#[test]
fn leave_game_cotenant_requires_every_gate_the_scan_tests() {
    let mut img = image();
    // Subject: player 0 on leader 3. Candidate: player 5, also on leader 3.
    img.players[0].who = 3;
    img.players[0].flags = PLAYER_PRESENT | PLAYER_LEAVE_SCAN_REQUIRED;
    img.players[5].who = 3;
    img.players[5].flags = PLAYER_PRESENT | PLAYER_LEAVE_SCAN_REQUIRED;
    assert_eq!(leave_game_cotenant(&img, 0), Some(5));

    // Each gate individually removes the candidate.
    for clear in [PLAYER_PRESENT, PLAYER_LEAVE_SCAN_REQUIRED] {
        let mut m = img.clone();
        m.players[5].flags &= !clear;
        assert_eq!(leave_game_cotenant(&m, 0), None, "clearing {clear:#06x}");
    }
    for set in [
        PLAYER_SCAN_EXCLUDE_HIGH,
        PLAYER_LEFT,
        PLAYER_RESIGNED,
        PLAYER_DROP_GATE,
    ] {
        let mut m = img.clone();
        m.players[5].flags |= set;
        assert_eq!(leave_game_cotenant(&m, 0), None, "setting {set:#06x}");
    }
    // A different leader is not a co-tenant.
    let mut m = img.clone();
    m.players[5].who = 4;
    assert_eq!(leave_game_cotenant(&m, 0), None);
    // The subject never matches itself even though it satisfies every gate.
    let mut solo = image();
    solo.players[0].who = 3;
    solo.players[0].flags = PLAYER_PRESENT | PLAYER_LEAVE_SCAN_REQUIRED;
    assert_eq!(leave_game_cotenant(&solo, 0), None);
}

#[test]
fn process_drop_scan_differs_from_leave_game_exactly_in_the_0x04_test() {
    let mut img = image();
    img.players[0].who = 2;
    img.players[0].flags = PLAYER_PRESENT;
    img.players[6].who = 2;
    img.players[6].flags = PLAYER_PRESENT; // no PLAYER_LEAVE_SCAN_REQUIRED
    assert_eq!(process_drop_cotenant(&img, 0), Some(6));
    assert_eq!(leave_game_cotenant(&img, 0), None);

    img.players[6].flags |= PLAYER_LEAVE_SCAN_REQUIRED;
    assert_eq!(process_drop_cotenant(&img, 0), Some(6));
    assert_eq!(leave_game_cotenant(&img, 0), Some(6));
}

// ---------------------------------------------------------------------------
// `Player::resign`
// ---------------------------------------------------------------------------

#[test]
fn local_resign_selects_its_category_from_the_quit_argument_only() {
    let mut img = duel();
    img.console_play = 0;
    img.console_who = 0;

    let plain = plan_resign(&img, 0, 0).unwrap();
    assert_eq!(sounds(&plain), vec![SOUND_LOCAL_RESIGN]);
    let from_quit = plan_resign(&img, 0, 1).unwrap();
    assert_eq!(sounds(&from_quit), vec![SOUND_LOCAL_QUIT]);

    // The local arm skips the whole remote envelope.
    assert_eq!(
        presentations(&plain),
        vec![LifecyclePresentation::Sound {
            category: SOUND_LOCAL_RESIGN
        }]
    );
    // Flags, semaphore bit 15 and the semaphore flags dword all move.
    assert_eq!(
        plain.image.players[0].flags,
        PLAYER_PRESENT | PLAYER_LEAVE_SCAN_REQUIRED | PLAYER_RESIGNED | PLAYER_LEFT
    );
    assert!(plain.image.sem(SEM_LOCAL_LEFT));
    assert_eq!(plain.image.semaphore_flags, 0);
}

#[test]
fn a_remote_resign_posts_the_ordered_envelope_and_leaves_the_semaphore_alone() {
    let img = duel(); // console_play = 1, resigner = 0
    let plan = plan_resign(&img, 0, 0).unwrap();
    assert_eq!(
        presentations(&plan),
        vec![
            LifecyclePresentation::PlayerName { play: 0 },
            LifecyclePresentation::Message {
                text_record: TEXT_RESIGNED
            },
            LifecyclePresentation::Notice {
                internal_string_record: INTERNAL_STRING_NOTICE
            },
            LifecyclePresentation::Sound {
                category: SOUND_OTHER_LEFT
            },
        ]
    );
    assert!(!plan.image.sem(SEM_LOCAL_LEFT));
    assert_eq!(plan.image.semaphore_flags, img.semaphore_flags);
}

#[test]
fn the_game_over_semaphore_bit_suppresses_only_the_interface_notice() {
    let mut img = duel();
    img.set_semaphore_bit(SEM_GAME_OVER, true);
    let plan = plan_resign(&img, 0, 0).unwrap();
    assert!(!presentations(&plan)
        .iter()
        .any(|p| matches!(p, LifecyclePresentation::Notice { .. })));
    assert!(presentations(&plan)
        .iter()
        .any(|p| matches!(p, LifecyclePresentation::Message { .. })));
}

#[test]
fn the_departure_category_needs_a_mutual_alliance() {
    let mut img = duel();
    // Own leader.
    img.console_who = 0;
    assert_eq!(departure_sound(&img, 0), Ok(SOUND_FRIENDLY_LEFT));

    img.console_who = 1;
    assert_eq!(departure_sound(&img, 0), Ok(SOUND_OTHER_LEFT));
    // One-sided: leaders[0].diplos[1] only.
    img.leaders[0].diplos[1] = DIPLO_ALLY;
    assert_eq!(departure_sound(&img, 0), Ok(SOUND_OTHER_LEFT));
    // Mutual.
    img.leaders[1].diplos[0] = DIPLO_ALLY;
    assert_eq!(departure_sound(&img, 0), Ok(SOUND_FRIENDLY_LEFT));
    // The reverse test is indexed by `LeaderData::who`, not by the slot.
    img.leaders[0].diplos[1] = 0;
    assert_eq!(departure_sound(&img, 0), Ok(SOUND_OTHER_LEFT));
}

#[test]
fn no_local_display_leader_is_refused_rather_than_read_out_of_range() {
    let mut img = duel();
    img.console_who = -1;
    assert_eq!(
        plan_resign(&img, 0, 0),
        Err(LifecycleError::ConsoleWhoOutOfRange { console_who: -1 })
    );
    // ... but the local arm never reaches the diplomacy test at all.
    img.console_play = 0;
    let plan = plan_resign(&img, 0, 0).unwrap();
    assert_eq!(sounds(&plan), vec![SOUND_LOCAL_RESIGN]);
}

// ---------------------------------------------------------------------------
// `Player::leave_game` inside resign/drop
// ---------------------------------------------------------------------------

#[test]
fn resign_defeats_with_reason_six_and_drop_with_reason_seven() {
    let img = duel();
    assert_eq!(
        calls(&plan_resign(&img, 0, 0).unwrap()),
        vec![LifecycleCall::LeaderDefeat {
            who: 0,
            defeat_type: LEAVE_REASON_RESIGN,
            arg: -1,
            instant: 0,
        }]
    );
    assert_eq!(
        calls(&plan_drop(&img, 0).unwrap()),
        vec![LifecycleCall::LeaderDefeat {
            who: 0,
            defeat_type: LEAVE_REASON_DROP,
            arg: -1,
            instant: 0,
        }]
    );
    // `Player::drop` sets no player flag and writes no semaphore.
    let drop = plan_drop(&img, 0).unwrap();
    assert_eq!(drop.image.players[0].flags & PLAYER_RESIGNED, 0);
    assert_eq!(drop.image.semaphore, img.semaphore);
}

#[test]
fn the_cotenant_scan_only_runs_under_semaphore_bit_two() {
    let mut img = duel();
    img.players[1].who = 0; // player 1 shares leader 0
    img.leaders[1].leader_flags = 0;

    // Bit 2 clear: retail never scans, so the leader is defeated anyway.
    assert_eq!(calls(&plan_resign(&img, 0, 0).unwrap()).len(), 1);

    img.set_semaphore_bit(SEM_NET_OR_RECORDING, true);
    let scanned = plan_resign(&img, 0, 0).unwrap();
    assert!(calls(&scanned).is_empty());
    // The flag stores before the scan still commit.
    assert_ne!(scanned.image.players[0].flags & PLAYER_LEFT, 0);
    assert_ne!(scanned.image.players[0].flags & PLAYER_RESIGNED, 0);
}

#[test]
fn a_running_capital_timer_overrides_the_leave_reason() {
    let mut img = duel();
    img.leaders[0].lost_capital_timer = 41;
    assert_eq!(
        calls(&plan_resign(&img, 0, 0).unwrap()),
        vec![LifecycleCall::LeaderDefeat {
            who: 0,
            defeat_type: DEFEAT_TYPE_CAPITAL,
            arg: 0,
            instant: 0,
        }]
    );

    // Both low leader flags are required; ACTIVE alone is not enough.
    let mut inactive = img.clone();
    inactive.leaders[0].leader_flags = LEADER_VALID;
    assert_eq!(
        calls(&plan_resign(&inactive, 0, 0).unwrap()),
        vec![LifecycleCall::LeaderDefeat {
            who: 0,
            defeat_type: LEAVE_REASON_RESIGN,
            arg: -1,
            instant: 0,
        }]
    );

    // Capital elimination reaches `LeaderData::find_capital`, which this lane did not recover.
    let mut capital = img.clone();
    capital.elimination = 1;
    let plan = plan_resign(&capital, 0, 0).unwrap();
    assert_eq!(
        plan.boundary,
        Some(LifecycleBoundary::FindCapitalForDefeat { who: 0 })
    );
    assert!(calls(&plan).is_empty());
    // No semaphore write is authorized past the boundary, even for the local player.
    let mut local = capital;
    local.console_play = 0;
    local.console_who = 0;
    let plan = plan_resign(&local, 0, 0).unwrap();
    assert!(plan.boundary.is_some());
    assert!(!plan.image.sem(SEM_LOCAL_LEFT));
}

// ---------------------------------------------------------------------------
// `Player::quit`
// ---------------------------------------------------------------------------

#[test]
fn quit_restores_the_semaphore_bit_it_sampled_before_resigning() {
    let mut img = duel();
    img.console_play = 0;
    img.console_who = 0;

    // Sampled clear: resign sets it (local), quit clears it again and seeds the flags dword.
    let cleared = plan_quit(&img, 0, 0).unwrap();
    assert!(!cleared.image.sem(SEM_LOCAL_LEFT));
    assert_eq!(cleared.image.semaphore_flags, 2);

    // Sampled set: quit puts it back and zeroes the flags dword.
    let mut pre = img.clone();
    pre.set_semaphore_bit(SEM_LOCAL_LEFT, true);
    pre.semaphore_flags = 9;
    let restored = plan_quit(&pre, 0, 0).unwrap();
    assert!(restored.image.sem(SEM_LOCAL_LEFT));
    assert_eq!(restored.image.semaphore_flags, 0);

    // The clear arm only seeds 2 into a flags dword that is already zero. For a local
    // player `Player::resign` always zeroes it first, so the seed is unconditional there;
    // a remote quit is the branch where a nonzero dword survives.
    let mut remote = duel(); // console_play = 1
    remote.players[1].who = 5; // no co-tenant for leader 0
    remote.semaphore_flags = 5;
    let kept = plan_quit(&remote, 0, 0).unwrap();
    assert_eq!(kept.image.semaphore_flags, 5);
    assert!(!kept.image.sem(SEM_LOCAL_LEFT));
}

#[test]
fn quit_always_resigns_with_the_quit_category() {
    let mut img = duel();
    img.console_play = 0;
    img.console_who = 0;
    let plan = plan_quit(&img, 0, 0).unwrap();
    assert_eq!(sounds(&plan), vec![SOUND_LOCAL_QUIT]);
    assert_ne!(plan.image.players[0].flags & PLAYER_RESIGNED, 0);
}

#[test]
fn a_remote_quit_under_semaphore_bit_two_returns_before_playing_and_the_report() {
    let mut img = duel(); // console_play = 1
    img.set_semaphore_bit(SEM_NET_OR_RECORDING, true);
    img.players[1].who = 5; // no co-tenant for leader 0
    let plan = plan_quit(&img, 0, 0).unwrap();
    assert_eq!(plan.image.playing, img.playing);
    assert!(!presentations(&plan)
        .iter()
        .any(|p| matches!(p, LifecyclePresentation::Report { .. })));

    // The local player takes the other arm and still stops the game.
    let mut local = img.clone();
    local.console_play = 0;
    local.console_who = 0;
    let plan = plan_quit(&local, 0, 0).unwrap();
    assert_eq!(plan.image.playing, 0);
    assert!(presentations(&plan)
        .iter()
        .any(|p| matches!(p, LifecyclePresentation::Report { .. })));
}

#[test]
fn the_drop_control_bit_keeps_the_game_playing_unless_quit_is_forced() {
    let mut img = duel();
    img.console_play = 0;
    img.console_who = 0;
    img.set_semaphore_bit(SEM_DROP_CONTROL, true);

    assert_eq!(plan_quit(&img, 0, 0).unwrap().image.playing, 1);
    assert_eq!(plan_quit(&img, 0, 1).unwrap().image.playing, 0);
    // Without the bit, `force_stop` is irrelevant.
    img.set_semaphore_bit(SEM_DROP_CONTROL, false);
    assert_eq!(plan_quit(&img, 0, 0).unwrap().image.playing, 0);
}

// ---------------------------------------------------------------------------
// `DropControl::process_drop`
// ---------------------------------------------------------------------------

#[test]
fn the_drop_vote_window_opens_exactly_once() {
    let img = duel();
    let plan = plan_process_drop(&img, 0, 3).unwrap();
    assert_eq!(
        presentations(&plan)[0],
        LifecyclePresentation::DropVoteWindow {
            internal_string_record: INTERNAL_STRING_DROP_LOG
        }
    );
    assert!(plan.image.drop_window_open);

    let mut open = img.clone();
    open.drop_window_open = true;
    let again = plan_process_drop(&open, 0, 3).unwrap();
    assert!(!presentations(&again)
        .iter()
        .any(|p| matches!(p, LifecyclePresentation::DropVoteWindow { .. })));
}

#[test]
fn state_three_hands_the_leader_to_the_ai_and_never_drops_the_player() {
    let mut img = duel();
    img.players[0].flags |= PLAYER_LEFT;
    let plan = plan_process_drop(&img, 0, 3).unwrap();

    assert_eq!(
        plan.image.players[0].flags,
        (PLAYER_PRESENT | PLAYER_LEAVE_SCAN_REQUIRED | PLAYER_LEFT) & PLAYER_DROP_RETAIN_MASK
    );
    assert_eq!(plan.image.players[0].flags, PLAYER_PRESENT);
    assert_eq!(plan.image.leaders[0].leader_flags & LEADER_HUMAN, 0);
    assert_eq!(
        plan.image.leaders[0].leader_flags & LEADER_VALID_ACTIVE,
        LEADER_VALID_ACTIVE
    );
    assert_eq!(plan.image.leaders[0].multi_diff, DROPPED_LEADER_MULTI_DIFF);
    assert!(calls(&plan).is_empty());
    assert!(!presentations(&plan)
        .iter()
        .any(|p| matches!(p, LifecyclePresentation::Sound { .. })));
}

#[test]
fn state_three_refuses_a_player_already_marked_dropped_or_still_shared() {
    let mut gated = duel();
    gated.players[0].flags |= PLAYER_DROP_GATE;
    let plan = plan_process_drop(&gated, 0, 3).unwrap();
    assert_eq!(plan.image.players[0].flags, gated.players[0].flags);
    assert_eq!(plan.image.leaders[0], gated.leaders[0]);

    let mut shared = duel();
    shared.players[1].who = 0;
    let plan = plan_process_drop(&shared, 0, 3).unwrap();
    assert_eq!(plan.image.players[0].flags, shared.players[0].flags);
    assert_eq!(plan.image.leaders[0].multi_diff, 0);
}

#[test]
fn states_one_and_two_dissolve_teams_and_declare_every_remaining_pair() {
    let mut img = duel();
    img.leaders[3].leader_flags = LEADER_VALID_ACTIVE;
    img.players[3].flags = PLAYER_PRESENT;
    img.players[3].team = 2;
    img.team_style = 4;

    for (state, expected_style) in [(1, 0u8), (2, 1u8)] {
        let plan = plan_process_drop(&img, 0, state).unwrap();
        assert_eq!(plan.image.team_style, expected_style);
        for slot in [0usize, 1, 3] {
            assert_eq!(plan.image.players[slot].team, PLAYER_TEAM_AUTO, "slot {slot}");
        }
        // An absent player row is untouched.
        assert_eq!(plan.image.players[4].team, 0);

        let declares: Vec<_> = calls(&plan)
            .into_iter()
            .filter(|c| matches!(c, LifecycleCall::LeaderActionDeclare { .. }))
            .collect();
        assert_eq!(
            declares,
            vec![
                LifecycleCall::LeaderActionDeclare {
                    who: 0,
                    whom: 1,
                    treaty: 0,
                    no_payment: 1,
                    over: 1
                },
                LifecycleCall::LeaderActionDeclare {
                    who: 0,
                    whom: 3,
                    treaty: 0,
                    no_payment: 1,
                    over: 1
                },
                LifecycleCall::LeaderActionDeclare {
                    who: 1,
                    whom: 3,
                    treaty: 0,
                    no_payment: 1,
                    over: 1
                },
            ],
            "state {state}"
        );
        // The war fan-out is followed by the drop tail, in that order.
        assert_eq!(
            calls(&plan).last(),
            Some(&LifecycleCall::LeaderDefeat {
                who: 0,
                defeat_type: LEAVE_REASON_DROP,
                arg: -1,
                instant: 0,
            })
        );
    }
}

#[test]
fn an_unrecognised_state_drops_the_player_with_no_team_or_diplomacy_work() {
    let img = duel();
    let plan = plan_process_drop(&img, 0, 9).unwrap();
    assert_eq!(plan.image.team_style, img.team_style);
    assert_eq!(
        calls(&plan),
        vec![LifecycleCall::LeaderDefeat {
            who: 0,
            defeat_type: LEAVE_REASON_DROP,
            arg: -1,
            instant: 0,
        }]
    );
}

// ---------------------------------------------------------------------------
// Fail-closed image validation
// ---------------------------------------------------------------------------

#[test]
fn a_malformed_image_is_refused_before_any_effect_is_recorded() {
    let img = duel();
    assert_eq!(
        plan_resign(&img, -1, 0),
        Err(LifecycleError::PlayOutOfRange { play: -1 })
    );
    assert_eq!(
        plan_resign(&img, 8, 0),
        Err(LifecycleError::PlayOutOfRange { play: 8 })
    );

    let mut slots = img.clone();
    slots.players[2].play = 5;
    assert_eq!(
        plan_resign(&slots, 0, 0),
        Err(LifecycleError::PlaySlotMismatch { slot: 2, play: 5 })
    );

    let mut leaders = img.clone();
    leaders.leaders[4].who = 6;
    assert_eq!(
        plan_resign(&leaders, 0, 0),
        Err(LifecycleError::LeaderSlotMismatch { slot: 4, who: 6 })
    );

    let mut who = img;
    who.players[0].who = 9;
    assert_eq!(
        plan_resign(&who, 0, 0),
        Err(LifecycleError::WhoOutOfRange { play: 0, who: 9 })
    );
}

// ---------------------------------------------------------------------------
// Receipt
// ---------------------------------------------------------------------------

#[test]
fn an_applied_receipt_needs_a_clean_replan_and_every_call_acknowledged() {
    let img = duel();
    let request = LifecycleRequest::Resign {
        play: 0,
        from_quit: 0,
    };
    let plan = plan_lifecycle(&img, request).unwrap();
    let required = LifecycleReceipt::required_calls(&plan);
    assert_eq!(required.len(), 1);

    let good = LifecycleReceipt {
        request,
        status: LifecycleStatus::Applied,
        before: Some(img.clone()),
        plan: Some(plan.clone()),
        executed_calls: required.clone(),
    };
    assert!(good.validates(request));

    // Dropping the acknowledgement invalidates it.
    let unacked = LifecycleReceipt {
        executed_calls: Vec::new(),
        ..good.clone()
    };
    assert!(!unacked.validates(request));

    // So does a before-image that does not replan to the observed plan.
    let mut other = img.clone();
    other.console_play = 0;
    let wrong_before = LifecycleReceipt {
        before: Some(other),
        ..good.clone()
    };
    assert!(!wrong_before.validates(request));

    // A different request never validates.
    assert!(!good.validates(LifecycleRequest::Drop { play: 0 }));

    // Unavailable is a valid identity-preserving refusal.
    assert!(LifecycleReceipt::unavailable(request).validates(request));
}

#[test]
fn a_boundary_plan_can_never_be_reported_as_applied() {
    let mut img = duel();
    img.leaders[0].lost_capital_timer = 3;
    img.elimination = 1;
    let request = LifecycleRequest::Resign {
        play: 0,
        from_quit: 0,
    };
    let plan = plan_lifecycle(&img, request).unwrap();
    assert!(plan.boundary.is_some());

    let forged = LifecycleReceipt {
        request,
        status: LifecycleStatus::Applied,
        before: Some(img),
        plan: Some(plan),
        executed_calls: Vec::new(),
    };
    assert!(!forged.validates(request));
}

// ---------------------------------------------------------------------------
// Literal pins.  Every value below is written out rather than compared against the
// constant it names, so a one-bit edit to the module fails here instead of silently
// agreeing with itself.
// ---------------------------------------------------------------------------

#[test]
fn every_recovered_constant_is_pinned_to_its_literal() {
    // `Player::flags`, from the two co-tenant scans and the three store sites.
    assert_eq!(PLAYER_PRESENT, 0x0001);
    assert_eq!(PLAYER_LEAVE_SCAN_REQUIRED, 0x0004);
    assert_eq!(PLAYER_LEFT, 0x0010);
    assert_eq!(PLAYER_RESIGNED, 0x0040);
    assert_eq!(PLAYER_DROP_GATE, 0x0080);
    assert_eq!(PLAYER_SCAN_EXCLUDE_HIGH, 0x0100);
    assert_eq!(PLAYER_SCAN_EXCLUDE_MASK, 0x00d0);
    assert_eq!(PLAYER_DROP_RETAIN_MASK, 0xffeb);
    assert_eq!(PLAYER_TEAM_AUTO, 8);
    // `and word ptr [..], 0xFFEB` clears exactly these two bits and nothing else.
    assert_eq!(!PLAYER_DROP_RETAIN_MASK, PLAYER_LEAVE_SCAN_REQUIRED | PLAYER_LEFT);
    // `test al, 0xD0` covers exactly the three "gone" bits.
    assert_eq!(
        PLAYER_SCAN_EXCLUDE_MASK,
        PLAYER_LEFT | PLAYER_RESIGNED | PLAYER_DROP_GATE
    );

    // `LeaderData`.
    assert_eq!(LEADER_VALID, 0x01);
    assert_eq!(LEADER_VALID_ACTIVE, 0x03);
    assert_eq!(LEADER_HUMAN, 0x04);
    assert_eq!(DROPPED_LEADER_MULTI_DIFF, 3);

    // `Game::semaphore` bit indices: `Game+0x820` is byte 0, `+0x821` byte 1, `+0x822` byte 2.
    assert_eq!((SEM_NET_OR_RECORDING / 8, 1u8 << (SEM_NET_OR_RECORDING % 8)), (0, 0x04));
    assert_eq!((SEM_DROP_CONTROL / 8, 1u8 << (SEM_DROP_CONTROL % 8)), (0, 0x10));
    assert_eq!((SEM_GAME_OVER / 8, 1u8 << (SEM_GAME_OVER % 8)), (0, 0x40));
    assert_eq!((SEM_LOCAL_LEFT / 8, 1u8 << (SEM_LOCAL_LEFT % 8)), (1, 0x80));
    assert_eq!((SEM_QUIT_PREFIX / 8, 1u8 << (SEM_QUIT_PREFIX % 8)), (2, 0x04));

    // `Leader::defeat` arguments.
    assert_eq!(DEFEAT_TYPE_CAPITAL, 1);
    assert_eq!(LEAVE_REASON_RESIGN, 6);
    assert_eq!(LEAVE_REASON_DROP, 7);

    // `SoundGlobalCat` values and the ally comparand.
    assert_eq!(SOUND_LOCAL_RESIGN, 0x80);
    assert_eq!(SOUND_LOCAL_QUIT, 0x153);
    assert_eq!(SOUND_FRIENDLY_LEFT, 7);
    assert_eq!(SOUND_OTHER_LEFT, 0x22);
    assert_eq!(DIPLO_ALLY, 2);

    // String records: a `.text` byte offset into a 20-byte `String` array.
    assert_eq!(INTERNAL_STRING_NOTICE * 20, 0x1a3ec);
    assert_eq!(INTERNAL_STRING_QUIT_REPORT * 20, 0x1a400);
    assert_eq!(INTERNAL_STRING_DROP_LOG * 20, 0xcd28);
    assert_eq!(TEXT_RESIGNED * 20, 0xf104);
    assert_eq!(TEXT_DROPPED * 20, 0xf0f0);
    assert_eq!(TEXT_DROP_VOTE * 20, 0x56f4);

    // Record strides.
    assert_eq!(PLAYER_STRIDE, 0x8c);
    assert_eq!(LEADER_STRIDE, 0x6eec);
    assert_eq!(SEMAPHORE_BYTES, 32);
    assert_eq!(PLAYER_SLOTS, 8);
    assert_eq!(LEADER_SLOTS, 8);
}
