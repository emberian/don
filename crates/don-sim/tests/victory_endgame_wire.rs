// SPDX-License-Identifier: GPL-3.0-or-later
//! The command wire → `Leader::defeat` → `Game::check_victory` → end-game path, driven
//! through the real tick.
//!
//! Every test here decodes a real `CommandPackage` packet with
//! `tail_command_transactions::decode_tail_command`, commits it through
//! `Sim::apply_tail_command_transaction` — the command-pump entry, which is where retail
//! runs `CommandPackage::process_*`, outside the 29 entries of `Game::do_frame` — and then
//! runs `Sim::do_frame` to observe the end-game transition. Nothing calls the ported bodies
//! directly.
//!
//! What each test would catch:
//!
//! * a `Leader::defeat` that never runs (the state op-life could not reach at all),
//! * a defeat that runs but does not reach `Game::check_victory` `0x005926B0`,
//! * a match that reaches game-over but never consumes the
//!   `game_sem::VICTORY_RESOLVED` latch at step 27 (`Game::process_end_game`),
//! * a terminal Build-queue sweep left queued across the frame boundary,
//! * a refusal that mutated something on the way out.

use don_sim::command::tail_command_transactions::{
    decode_tail_command, lifecycle, TailCommandFacts, TailCommandRequest,
};
use don_sim::systems::production;
use don_sim::systems::victory_score::{game_sem, leader_flag, DefeatType};
use don_sim::tick::lifecycle_host::{PlayerTable, SimTailError, SimTailOutcome};
use don_sim::tick::{Sim, StepRun};

/// `Player::flags` for a seated, playing player: `PLAYER_PRESENT | 0x04`, the two bits the
/// `Player::leave_game` co-tenant scan requires of a candidate.
const SEATED: u16 = lifecycle::PLAYER_PRESENT | lifecycle::PLAYER_LEAVE_SCAN_REQUIRED;

/// Two leaders at war, both seated, with a local console on play 0.
///
/// The console is load-bearing, not scenery: `Player::resign`'s remote arm reaches
/// `departure_sound`, which indexes `leaders[Console::who]`, so the fail-closed default
/// `console_who == -1` refuses every remote departure with `ConsoleWhoOutOfRange`.
fn seated_sim(seed: u64) -> Sim {
    let mut sim = Sim::new(seed, 16);
    sim.activate(0);
    sim.activate(1);
    let mut table = PlayerTable::new();
    table.seat(0, SEATED, 0, 0);
    table.seat(1, SEATED, 1, 1);
    table.console_play = 0;
    table.console_who = 0;
    sim.players = Some(table);
    // `Leaders::strategy_all` `0x006ED483` calls `Game::check_victory` only under semaphore
    // bit 9, so a match that expects the tick to resolve victory has to be in that mode.
    sim.vic_match.set_sem(game_sem::CHECK_VICTORY_MODE);
    sim
}

fn resign(play: i32) -> TailCommandRequest {
    let mut wire = [0u8; 5];
    wire[0] = 70;
    wire[1..].copy_from_slice(&play.to_le_bytes());
    decode_tail_command(&wire).expect("row 70 decodes")
}

fn quit(play: i32, replay: u8, system_quit: u8) -> TailCommandRequest {
    let mut wire = [0u8; 7];
    wire[0] = 71;
    wire[1..5].copy_from_slice(&play.to_le_bytes());
    wire[5] = replay;
    wire[6] = system_quit;
    decode_tail_command(&wire).expect("row 71 decodes")
}

fn ungraceful_drop(play: u8, state: u8) -> TailCommandRequest {
    decode_tail_command(&[80, play, state]).expect("row 80 decodes")
}

#[test]
fn a_resign_on_the_wire_ends_the_match_and_the_tick_consumes_the_latch() {
    let mut sim = seated_sim(0x5100);

    // A normal frame first: nothing is resolved, so step 27 is vacuous.
    let before = sim.do_frame();
    assert_eq!(before.steps[27], StepRun::Vacuous);
    assert_eq!(before.work[27], 0);
    assert!(!sim.vic_match.sem(game_sem::GAME_OVER));
    assert!(sim.vic_leaders.slots[1].is_alive());

    let request = resign(1);
    assert!(matches!(
        sim.tail_command_facts(&request),
        TailCommandFacts::PlayerLifecycle { .. }
    ));
    let receipt = sim.apply_tail_command_transaction(&request);
    assert!(receipt.committed(), "{:?}", receipt.outcome);
    assert!(receipt.validates());

    // `Player::resign` set both player bits and left with `DefeatTypeIndex` 6.
    let table = sim.players.as_ref().expect("table installed");
    assert_ne!(table.players[1].flags & lifecycle::PLAYER_RESIGNED, 0);
    assert_ne!(table.players[1].flags & lifecycle::PLAYER_LEFT, 0);
    assert!(sim.vic_leaders.slots[1].flag(leader_flag::DEFEATED));
    assert_eq!(
        sim.vic_leaders.slots[1].defeat_type,
        DefeatType::Resign as i32
    );

    // `Leader::defeat` `0x006ECDD8` called `Game::check_victory`, which ended the match and
    // handed the last standing leader a generic victory.
    assert!(sim.vic_match.sem(game_sem::GAME_OVER));
    assert!(sim.vic_match.sem(game_sem::VICTORY_RESOLVED));
    assert!(sim.vic_leaders.slots[0].flag(leader_flag::WON));

    // Step 27 (`Game::process_end_game`) consumes the one-shot latch on the next frame, and
    // only once.
    let ending = sim.do_frame();
    assert_eq!(ending.steps[27], StepRun::Executed);
    assert_eq!(ending.work[27], 1);
    assert!(!sim.vic_match.sem(game_sem::VICTORY_RESOLVED));

    let after = sim.do_frame();
    assert_eq!(after.steps[27], StepRun::Vacuous);
    assert_eq!(after.work[27], 0);
}

#[test]
fn a_resign_drains_the_terminal_build_queue_before_the_next_frame() {
    let mut sim = seated_sim(0x5101);
    let queued_type = 600usize;
    let row = sim.spawn_build(
        1,
        production::BuildData {
            flags: production::flag::VALID | production::flag::ACTIVE,
            build_masks: production::mask::REPEAT_QUEUE,
            queue: production::BuildQueue {
                queued: 1,
                entries: vec![production::BuildQueueEntry {
                    elapsed: 23,
                    type_index: queued_type as i16,
                    ..Default::default()
                }],
            },
            ..Default::default()
        },
    );
    sim.production_runtime.leaders[1].queued_counts[queued_type] = 1;

    let receipt = sim.apply_tail_command_transaction(&resign(1));
    assert!(receipt.committed(), "{:?}", receipt.outcome);

    // `Leader::defeat` `0x006ECB00` requests `Build::clean_queue(0)` over every live owned
    // Build; the command pump drains it immediately, not one tick later.
    assert_eq!(sim.builds[row].queue.queued, 0);
    assert_eq!(sim.builds[row].queue.entries[0].elapsed, 0);
    assert_eq!(
        sim.builds[row].build_masks & production::mask::REPEAT_QUEUE,
        0
    );
    assert_eq!(
        sim.production_runtime.leaders[1].queued_counts[queued_type],
        0
    );
    assert_eq!(sim.vic_leaders.take_terminal_queue_cleanup(), 0);

    // And the tick still runs cleanly afterwards.
    let t = sim.do_frame();
    assert_eq!(t.steps[27], StepRun::Executed);
}

#[test]
fn a_quit_commits_the_row71_semaphore_prefix_and_stops_the_match() {
    let mut sim = seated_sim(0x5102);

    // play 0 is the local console player, replay != 0, system_quit != 0.
    let receipt = sim.apply_tail_command_transaction(&quit(0, 1, 1));
    assert!(receipt.committed(), "{:?}", receipt.outcome);
    assert!(receipt.validates());

    // `CommandPackage::process_quit` `0x00943A95` set semaphore bit 18. `Player::quit`
    // `0x006EDC0F` sampled bit 15 *before* `Player::resign(1)` could set it and therefore
    // cleared it again, leaving the flags dword at 2.
    assert!(sim.vic_match.sem(lifecycle::SEM_QUIT_PREFIX));
    assert!(!sim.vic_match.sem(lifecycle::SEM_LOCAL_LEFT));
    let table = sim.players.as_ref().expect("table installed");
    assert_eq!(table.semaphore_flags, 2);
    assert_eq!(table.playing, 0);

    assert!(sim.vic_leaders.slots[0].flag(leader_flag::DEFEATED));
    assert_eq!(
        sim.vic_leaders.slots[0].defeat_type,
        DefeatType::Resign as i32
    );
    assert!(sim.vic_leaders.slots[1].flag(leader_flag::WON));
    assert!(sim.vic_match.sem(game_sem::GAME_OVER));

    let ending = sim.do_frame();
    assert_eq!(ending.steps[27], StepRun::Executed);
    assert_eq!(ending.work[27], 1);
}

#[test]
fn a_drop_vote_resolution_hands_the_leader_to_the_ai_and_the_match_continues() {
    let mut sim = seated_sim(0x5103);
    // `CommandPackage::process_ungraceful_player_drop` `0x00943EFA` enters `DropControl`
    // only under semaphore bit 4.
    sim.vic_match.set_sem(lifecycle::SEM_DROP_CONTROL);
    sim.vic_leaders.slots[1].leader_flags |= leader_flag::HUMAN;

    let receipt = sim.apply_tail_command_transaction(&ungraceful_drop(1, 3));
    assert!(receipt.committed(), "{:?}", receipt.outcome);
    assert!(receipt.validates());

    // State 3 is the one arm that does **not** call `Player::drop`: the leader keeps
    // playing, under AI control at `LeaderData::multi_diff == 3`.
    assert!(!sim.vic_leaders.slots[1].flag(leader_flag::DEFEATED));
    assert!(sim.vic_leaders.slots[1].is_alive());
    assert_eq!(
        sim.vic_leaders.slots[1].leader_flags & leader_flag::HUMAN,
        0
    );
    assert_eq!(sim.vic_leaders.slots[1].multi_diff, 3);

    let table = sim.players.as_ref().expect("table installed");
    assert!(table.drop_window_open);
    // `and word ptr [players[play].flags], 0xFFEB` clears exactly 0x04 and 0x10.
    assert_eq!(
        table.players[1].flags,
        SEATED & lifecycle::PLAYER_DROP_RETAIN_MASK
    );

    let t = sim.do_frame();
    assert!(!sim.vic_match.sem(game_sem::GAME_OVER));
    assert_eq!(t.steps[27], StepRun::Vacuous);
    assert!(sim.vic_leaders.slots[1].is_alive());
}

#[test]
fn a_co_tenant_keeps_the_leader_in_the_game_and_the_match_running() {
    let mut sim = seated_sim(0x5104);
    // The co-tenant scan runs only under semaphore bit 2.
    sim.vic_match.set_sem(game_sem::NET_OR_RECORDING);
    // A third seated player holding the *same* leader as play 1.
    sim.players.as_mut().unwrap().seat(2, SEATED, 1, 1);

    let receipt = sim.apply_tail_command_transaction(&resign(1));
    assert!(receipt.committed(), "{:?}", receipt.outcome);
    assert!(receipt.validates());

    // `Player::leave_game` `0x006EE050` found a co-tenant and returned with no defeat.
    let table = sim.players.as_ref().expect("table installed");
    assert_ne!(table.players[1].flags & lifecycle::PLAYER_RESIGNED, 0);
    assert_ne!(table.players[1].flags & lifecycle::PLAYER_LEFT, 0);
    assert!(!sim.vic_leaders.slots[1].flag(leader_flag::DEFEATED));
    assert!(sim.vic_leaders.slots[1].is_alive());
    assert!(!sim.vic_match.sem(game_sem::GAME_OVER));

    let t = sim.do_frame();
    assert_eq!(t.steps[27], StepRun::Vacuous);
    assert!(!sim.vic_match.sem(game_sem::GAME_OVER));
}

#[test]
fn without_a_player_table_the_wire_cannot_end_the_match() {
    let mut sim = Sim::new(0x5105, 16);
    sim.activate(0);
    sim.activate(1);
    sim.vic_match.set_sem(game_sem::CHECK_VICTORY_MODE);

    let receipt = sim.apply_tail_command_transaction(&resign(1));
    assert_eq!(
        receipt.outcome,
        SimTailOutcome::Refused(SimTailError::NoPlayerTable)
    );
    assert!(receipt.validates());

    for _ in 0..5 {
        let t = sim.do_frame();
        assert_eq!(t.steps[27], StepRun::Vacuous);
    }
    assert!(sim.vic_leaders.slots[1].is_alive());
    assert!(!sim.vic_match.sem(game_sem::GAME_OVER));
}

#[test]
fn the_drop_states_that_declare_war_refuse_without_touching_the_match() {
    let mut sim = seated_sim(0x5106);
    sim.vic_match.set_sem(lifecycle::SEM_DROP_CONTROL);
    let team_style = sim.vic_match.options.team_style;
    let flags_before = sim.players.as_ref().unwrap().players[1].flags;

    // States 1 and 2 fan `Leader::action_declare` `0x006DAB50` out over every leader pair.
    // That is command row 38's open tail; this host owns no implementation of it, so the
    // whole transaction refuses.
    for state in [1u8, 2u8] {
        let receipt = sim.apply_tail_command_transaction(&ungraceful_drop(1, state));
        assert!(!receipt.committed(), "state {state} must not commit");
        assert!(receipt.validates());
    }

    let table = sim.players.as_ref().expect("table installed");
    assert!(!table.drop_window_open);
    assert_eq!(table.players[1].flags, flags_before);
    assert_eq!(sim.vic_match.options.team_style, team_style);
    assert!(sim.vic_leaders.slots[1].is_alive());

    let t = sim.do_frame();
    assert_eq!(t.steps[27], StepRun::Vacuous);
    assert!(!sim.vic_match.sem(game_sem::GAME_OVER));
}

#[test]
fn rows_the_host_owns_no_state_for_keep_their_whole_row_boundary() {
    let sim = seated_sim(0x5107);
    // Row 78 `ConsoleCmd`: `Sim` owns no console parser, so it never gets lifecycle facts.
    let mut wire = [0u8; 521];
    wire[0] = 78;
    let console = decode_tail_command(&wire).expect("row 78 decodes");
    assert_eq!(
        sim.tail_command_facts(&console),
        TailCommandFacts::NoExternalFacts
    );
}
