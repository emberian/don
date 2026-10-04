//! The small `Game::do_frame` 0x00591ef0 steps: 0, 1, 2, 3, 9, 10, 18, 24,
//! 26, 27, 28. For each one this module records, from the disassembly,
//! whether the body writes any WALKED state (anything `WalkDataGame`
//! 0x005a2360 serialises / the 15 `check_*` channels hash) and transcribes
//! what can be executed from a `Save`.
//!
//! `tick.rs` currently calls `run(save, effects)` for every one of these
//! steps without the step index. `run` is therefore a no-op; the real
//! bodies live behind [`run_step`] / [`step_status`] so the schedule can
//! dispatch per step (see the lead note in the summary). Nothing here is
//! derived from the frame diff — the retail code is the only source; the
//! burn-down is the oracle.
//!
//! Save-tree offsets used below (all from `crates/don-state/src/sections.rs`):
//! `Game.scalars` = Game+0x550..+0x6e4 (`frame` at +0x550 = scalars[0]);
//! `Game.sem_ptr` = Game+0x820.. (`semaphore.ptr[32]`, length `sem_size`);
//! `Game.info.settings` = GameInfo+0x18..+0x36 (`rush_rules` Game+0x32 →
//! settings[0x1a], `cannon_times` +0x33 → settings[0x1b]);
//! `console1` = Console+0x298..+0x2b8 (`my_player` Console+0x2a0 →
//! console1[8..12]); `Leader[i].body` = LeaderData+0x08.. ;
//! `World.direct[k]` = World+(8+4k) (`land_size` +0x78 → direct[28]);
//! `Achieve.head` = `{max_record_times: u16 @0x00e885a0, rate: u16 @0x00e885a2}`,
//! `Achieve.list` = `Achieve::times` (SimpleArray<int> @0x00e87d44),
//! `Achieve.data[r].l[p]` = record r (6 × 0x140 @0x00e87d60) SimpleArray<int>
//! for leader p (stride 0x1c at record+4).
//!
//! # Per-step findings
//!
//! ## 0 `AutoSave::restore` 0x005A20C0 — `Stub` (gate transcribed)
//! Called only `if (Game+0x821 & 0x20)` (00591ef0.c:66) — `semaphore.ptr[1]`
//! bit 5, **walked** (`Game.sem_ptr[1]`). Body (005a20c0.c): builds the
//! auto-save path (`AutoSave auto_save` @0x00E80474 + string +0x6b8 +
//! `.svx`), calls `LoadGame::load_game` 0x005A72F0 — which re-walks the
//! ENTIRE save tree from disk — then clears the bit (`Game+0x821 &= 0xdf`,
//! walked) and sets `Game.semaphore.flags` (+0x81c, unwalked) to 2 if 0.
//! Writes walked state: **yes, when armed** (whole tree + the flag). Not
//! executable from a `Save` (file IO); when the bit is set `run_step`
//! reports it and touches nothing. Closed in every capture (sem_ptr[1]==0x01).
//!
//! ## 1 `GameLog::begin_frame` 0x00932A70 — `Ported` (no-op)
//! `game_log` @0x00EB1360 is the desync-debug log. Body: `fflush` two
//! FILE*s, and if `Game.frame` is inside the `[+0x74, +0x78)` dump window
//! (or the window start is negative) set `+0x44 = 3`, call
//! `GameLog::full_dump` 0x00930380, reset `+0x44 = 0`. `full_dump` drives
//! the `GameLog::dump_*` walkers, which *read* state through a logging
//! `DataWalk`. No `GameLog` field is walked anywhere (no section in
//! `WalkDataGame`, not a checksum channel). Writes walked state: **no**.
//!
//! ## 2 `Random::get` artificial lag — `Ported` (no-op, 0 game_random draws)
//! 00591ef0.c:84-92. Gate `Game.test_delay_max` (+0x9e0, unwalked) != 0.
//! The draw is `call 0xa39d70` at 0x0059202D with `mov ecx, 0xeb697c`
//! (0x00592028) — `this` is `class Random internal_random` @0x00EB697C,
//! NOT `GameAccess::game_random` (`[0x00c06184]`, the seed walked at
//! `post_world`+40). Every code reference to `internal_random` is
//! presentation (`GuyOut::graph_inc_frame`, `Scene::render`,
//! `Particle::pass_time`, `Surf::*`, …; 0x00A39D44 `random()` wrapper);
//! none is a walker. The result only feeds a `timeGetTime`/`Sleep(0)`
//! spin. Writes walked state: **no**; contributes **0** of the 23–25
//! `game_random` draws measured per idle frame.
//!
//! ## 3 `CommandManager::issue_player_speed` 0x00943100 — `Stub` (gate transcribed)
//! Gate (00591ef0.c:93): `(Game.frame & 7) == Console.my_player` (+0x2a0,
//! walked in `console1[8..12]`). Body: if `CommandManager::check_accept_issue`
//! 0x00940A70 (returns 1 unless `use_mp_playback` +0x15264, or the
//! replay/record semaphore bits say otherwise — true in SP), snapshot the
//! 8 `Player` accumulators (`Player+0x81..+0x8d`: `accum_frames_zoomed_in`,
//! `_out`, clicks, hotkeys, …; `Player::walk_data` 0x006EE2D0 walks only
//! Player+0x30..+0x32 and +0x00..+0x39 — these are **unwalked**), zero
//! them, and `CommandPackage::add_command` 0x0094BAE0 a 9-byte opcode
//! `0x4f` into `CommandManager.local_package` (+0x10 size, +0x12 data —
//! **walked**: `CommandManager.local_package`). Writes walked state:
//! **yes, transiently**. The local package is flushed by
//! `CommandManager::process_turn` 0x0093EF10 from `TurnControl` in
//! `Game::loop`, *outside* `do_frame`; at every captured save boundary
//! `local_package.size == 0` and all eight FIFOs are empty, including the
//! frames right after an issue (f16→f17, f24→f25, …). Porting the append
//! without the flush would introduce bytes, so the body is left untouched
//! and the armed gate is reported. The command payload is also
//! UI-derived (zoom counters), so it is not fully determined by a `Save`.
//!
//! ## 9 `NetDaemon::process_all` 0x00951300 — `Stub`
//! Re-entrancy guarded (`DAT_00ee12e0 < 2`), calls `NetDaemon::process`
//! 0x00950F30 until it returns 0. Everything inside is gated on
//! `NetSys *netsys` @0x00E335C8 != 0 and on a received packet; packet
//! kinds 0/5/7/0xc… deliver command packages into `CommandManager`
//! (walked FIFOs) and turn-control state. Writes walked state: **only
//! when a packet arrives**; the captures show the FIFOs empty at every
//! save boundary. The network layer is not part of the `Save`, so there is
//! nothing to evaluate here; retail calls it five times per frame.
//!
//! ## 10 rush-rules timer expiry (inline 0x0059225E..0x0059241C) — `Stub` (gate transcribed)
//! 00591ef0.c:156-208. Gate: `rush = Game.info.rush_rules` (+0x32,
//! **walked** settings[0x1a]) `!= 0 && rush > 8 &&
//! Game.frame == rush_rules.list[rush].minutes(+0x3c) * 900` where the
//! table is `class Categories rush_rules` @0x00E80078 (list ptr at +0x10 =
//! 0x00E80088, stride 0x58) — rules-derived, not in the `Save`. Body when
//! it fires: `MessageWin` colour fields (+0x580/+0x588, unwalked) +
//! `MessageWin::add_message` 0x007E9FB0 (walked `message_win` lists),
//! `SoundGlobal::play(0x74)`; then unless `game_rules` (+0x24) is 0/8/11:
//! for every active leader `LeaderData+0x1f4..+0x1f8 = 0`
//! (`attrition_stamp`, walked `Leader.body[0x1ec..0x1f0]`), and for every
//! other active leader not allied (`FUN_006edb50`) and not at war
//! (`FUN_006ebaa0`): `Leader::set_diplo(j, WAR=0)` 0x006EC6A0 plus a
//! message for the local player. Writes walked state: **yes, when it
//! fires**. Executable gate part: `rush > 8`; the frame compare needs the
//! Categories table (stopping point). Captures: rush==2 → never armed.
//!
//! ## 18 `Achieve::capture_data` 0x007AF980 — `Partial`
//! Called `if ((Game+0x821 & 8) == 0)` (00591ef0.c:224; sem_ptr[1] bit 3,
//! clear in captures). Body (007af980.c):
//!
//! ```text
//! if rate(0x00e885a2) == 0: return                      // Achieve.head >> 16
//! if Game.frame % rate != 0: Achieve::record_data(0)    // overwrite last sample
//! else:
//!   Achieve::record_data(1)                             // append sample + min/max
//!   if times.length(0x00e87d48) >= max_record_times(0x00e885a0):
//!     AchieveData::condense() on all 6 records; halve times; rate <<= 1
//! ```
//!
//! `Achieve::record_data` 0x007AEC50, per active leader p (Leader+0 bit 0),
//! p in 0..8 (loop bound 0x00E786FF), writes one sample into six records
//! (`this` for each `FUN_007af270` call read from the disassembly):
//! * record 1 (0x00E87EA0): `LeaderData.score_units_2` (+0x28 → body[0x20])
//! * record 2 (0x00E87FE0): `LeaderData.territory` (+0x9d8 → body[0x9d0]) * 1000 / `World.land_size` (+0x78 → direct[28]), `idiv`
//! * record 0 (0x00E87D60): `LeaderData.score` (+0x18 → body[0x10])
//! * record 4 (0x00E88260): tech hash — popcount of the `tech` BitMask words
//!   (`bit_count` 0x00A47FB0, cached in `tech.flags` bit 0) + `has_tech` 0x006E0C80 count over
//!   TypeIndex 0x243..0x246 + `((epochs^0x69587) + n + (ages^0x62766)*2)*5 + (discovered^0x13985)*2`
//!   from `LeaderDataEncrypt` (+0xdc/+0xe0/+0xe4)
//! * record 3 (0x00E88120): Σ over resource i in 0..6 with `LeaderData::type_avail(i,1)` 0x006E33A0 of `income[i] >> 4` (signed, `LeaderDataEncrypt+0x94`)
//! * record 5 (0x00E883A0): `8 - p`
//!
//! then `times[len-1] = Game.frame` (mode 0) or push (mode 1). All of
//! `Achieve.head`, `Achieve.list` (times) and `Achieve.data[r].l[p]` are
//! **walked** (`Achieve::walk_data` 0x007AF790; the `AchieveData` min/max
//! blocks too). Writes walked state: **yes, when `rate != 0`**.
//!
//! Transcribed: the gate, the mode decision, and mode 0 for records 0, 1,
//! 2, 5 and `times`. Stopping points (fields left untouched, reported in
//! `effects`): records 3 and 4 (need `LeaderData::has_tech` /
//! `type_avail`, which read the rules tables), and the whole mode-1 path
//! (append + min/max + condense + `rate <<= 1`). In every capture
//! `Achieve.head == 0` (rate 0) so the body is skipped and nothing
//! changes; the transcription is exercised by the synthetic unit test.
//!
//! ## 24 `TurnControl::check_cannon_time` 0x009579E0 — `Stub`
//! Child of the `frame % 15 == 0` branch (00591ef0.c:239). Gate:
//! `TurnControl.cannon_time_who` (+0x24) `>= 0 && Game.frame -
//! TurnControl.cannon_time_stamp` (+0x28) `> 0x4a`. `TurnControl`
//! (`GameAccess::turn_control` @0x00C06180) is **not** in the save tree:
//! `TurnControl::walk_data` 0x00956C50 is only ever called from
//! `LoadGame::verify_load` 0x005A39F0 (a debug cross-check), never from
//! `WalkDataGame` or a checksum channel. When it fires:
//! `TurnControl::end_cannon_time` 0x00956470 (TurnControl +0x24/+0x2c/+0x30,
//! unwalked), `MessageWin::add_message` 0x007E9FB0 (**walked**
//! `message_win`), `SoundGlobal::play(0x13a)`, `IFaceMainBase::do_notice`.
//! Writes walked state: **yes, when armed**, but the arming state is not
//! recoverable from a `Save`. `cannon_time_who` is set only by
//! `TurnControl::start_cannon_time` 0x00956500 (a player command that also
//! decrements `LeaderData+0x48`); never issued in the idle captures.
//!
//! ## 26 `GameLog::end_frame` 0x009329D0 — `Ported` (no-op)
//! Same dump window as step 1 with marker 4, then two one-shot flags
//! (`game_log+0x5c`, `+0x64`) that call `Log::end` 0x00A3B530 on the log
//! file. Debug IO only; no walked field. Writes walked state: **no**.
//!
//! ## 27 `Game::process_end_game` 0x00591CE0 — `Stub` (gate transcribed)
//! Called `if (Game+0x822 & 0x40)` (00591ef0.c:286) — `semaphore.ptr[2]`
//! bit 6, **walked** (`Game.sem_ptr[2]`). Body: `fast_forward_frame`
//! (+0x930, unwalked) = 0; conquest popup; Steam ELO upload;
//! `Game::determine_end_of_game_stats_and_achievements` 0x00583B30
//! (2.6 KB, touches LeaderData/Player stats); `EndGameWin::exec`; then
//! clears the bit (`Game+0x822 &= 0xbf`, walked) and sets
//! `semaphore.flags` (+0x81c, unwalked). Writes walked state: **yes, when
//! armed**. Not transcribed (end-game only; the stats body is a separate
//! lane). Closed in every capture (sem_ptr[2]==0).
//!
//! ## 28 `Scene::process_capture_sequence` 0x008C13C0 — `Stub` (gate transcribed)
//! Gate `Scene+0x1f3 & 2` = `Scene.flags.ptr[3]` bit 1 — **walked**
//! (`Scene.flags` BitMask data, `Scene::walk_data` 0x008C0F70). Body:
//! formats `capture_file` (+0x2c4) + zero-padded `capture_sequence`
//! (+0x2d8, unwalked counter) and asks the renderer (`[0x00e335a0]`
//! vtbl+0x88) for a screenshot; `capture_sequence += 1`; after 1000
//! frames clears the flag bit (walked) and sets `Scene.flags.flags`
//! (+0x1ec, unwalked). Writes walked state: **yes, when armed** (the bit
//! clear after 1000 shots) — not recoverable from a `Save` because the
//! counter is unwalked. Closed in every capture (`Scene.flags` size 0).
//!
//! # Also noticed (not one of this module's steps)
//! 00591ef0.c:246-253, between steps 25 and 26: every frame increments
//! `Player[my_player].accum_frames_zoomed_in` (+0x81) or
//! `accum_frames_zoomed_out` (+0x82) depending on `Scene+0x294` (camera
//! mode 5/6). Unwalked (see step 3), so no `STEPS` entry is needed for
//! the burn-down, but it is a real per-frame write the schedule omits.

use crate::tick::{StepStatus, FRAME};
use crate::Save;

/// Aggregate for the shared `0..=3 | 9 | 10 | 18 | 24 | 26..=28 =>
/// misc_steps::run` arm in `tick.rs`. `run` itself executes nothing (it
/// does not know which step it is), so the aggregate stays `Stub`; the
/// per-step truth is [`step_status`].
pub const STATUS: StepStatus = StepStatus::Stub;

/// Steps this module owns, in `do_frame` order.
pub const STEP_IDS: [usize; 11] = [0, 1, 2, 3, 9, 10, 18, 24, 26, 27, 28];

/// Per-step status (see module docs for the evidence behind each).
pub fn step_status(idx: usize) -> StepStatus {
    match idx {
        1 | 2 | 26 => StepStatus::Ported,
        18 => StepStatus::Partial,
        0 | 3 | 9 | 10 | 24 | 27 | 28 => StepStatus::Stub,
        _ => StepStatus::Stub,
    }
}

/// Shared entry used by `tick.rs` for all eleven steps. Deliberately a
/// no-op: the step index is not passed, and the one transcribed write
/// (step 18) must run exactly once, before `Game::frame++`.
pub fn run(_save: &mut Save, _effects: &mut Vec<String>) {}

/// Per-step entry. `effects` receives the writes performed and, for armed
/// gates whose bodies are untranscribed, a `stop:` note. Fields of
/// untranscribed branches are never touched.
pub fn run_step(idx: usize, save: &mut Save, effects: &mut Vec<String>) {
    match idx {
        0 => auto_save_restore(save, effects),
        1 => {}  // GameLog::begin_frame — debug log only (module docs)
        2 => {}  // Random::get on internal_random — unwalked, 0 game_random draws
        3 => issue_player_speed(save, effects),
        9 => {}  // NetDaemon::process_all — nothing evaluable from a Save
        10 => rush_rules_expiry(save, effects),
        18 => achieve_capture_data(save, effects),
        24 => {} // TurnControl::check_cannon_time — TurnControl is unwalked
        26 => {} // GameLog::end_frame — debug log only
        27 => process_end_game(save, effects),
        28 => process_capture_sequence(save, effects),
        _ => {}
    }
}

fn get_i32(buf: &[u8], off: usize) -> i32 {
    i32::from_le_bytes(buf[off..off + 4].try_into().unwrap())
}

fn put_i32(buf: &mut [u8], off: usize, v: i32) {
    buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

fn frame(save: &Save) -> i32 {
    get_i32(&save.game.scalars, FRAME)
}

/// Step 0 gate — 00591ef0.c:66 `(Game+0x821 & 0x20) != 0`.
fn auto_save_restore(save: &mut Save, effects: &mut Vec<String>) {
    if save.game.sem_ptr.get(1).is_some_and(|b| b & 0x20 != 0) {
        effects.push(
            "stop: AutoSave::restore 0x005A20C0 armed (Game.semaphore.ptr[1] & 0x20): \
             LoadGame::load_game would replace the whole Save and clear the bit — untranscribed, untouched"
                .into(),
        );
    }
}

/// Step 3 gate — 00591ef0.c:93 `(Game.frame & 7) == Console.my_player`.
fn issue_player_speed(save: &mut Save, effects: &mut Vec<String>) {
    if save.console1.len() < 12 {
        return;
    }
    let my_player = get_i32(&save.console1, 8);
    if (frame(save) & 7) == my_player {
        effects.push(
            "stop: CommandManager::issue_player_speed 0x00943100 armed: 9-byte opcode 0x4f append to \
             CommandManager.local_package is transient (flushed by process_turn outside do_frame) — untouched"
                .into(),
        );
    }
}

/// Step 10 gate — 00591ef0.c:157 `rush_rules != 0 && rush_rules > 8 && frame == table[rush].minutes*900`.
fn rush_rules_expiry(save: &mut Save, effects: &mut Vec<String>) {
    let Some(&rush) = save.game.info.settings.get(0x1a) else {
        return;
    };
    if rush != 0 && rush > 8 {
        effects.push(format!(
            "stop: rush-rules expiry armed (Game.info.rush_rules = {rush} > 8): frame compare needs \
             Categories rush_rules[{rush}].minutes (0x00E80088 table, not in Save) — untranscribed, untouched"
        ));
    }
}

/// Step 27 gate — 00591ef0.c:286 `(Game+0x822 & 0x40) != 0`.
fn process_end_game(save: &mut Save, effects: &mut Vec<String>) {
    if save.game.sem_ptr.get(2).is_some_and(|b| b & 0x40 != 0) {
        effects.push(
            "stop: Game::process_end_game 0x00591CE0 armed (Game.semaphore.ptr[2] & 0x40): end-game \
             stats + bit clear untranscribed, untouched"
                .into(),
        );
    }
}

/// Step 28 gate — 008c13c0.c `(Scene+0x1f3 & 2) != 0` (Scene.flags.ptr[3] bit 1).
fn process_capture_sequence(save: &mut Save, effects: &mut Vec<String>) {
    if save.scene.flags.data.get(3).is_some_and(|b| b & 2 != 0) {
        effects.push(
            "stop: Scene::process_capture_sequence 0x008C13C0 armed (Scene.flags[3] & 2): capture_sequence \
             counter is unwalked; bit clear after 1000 frames untranscribed, untouched"
                .into(),
        );
    }
}

// ---------------------------------------------------------------------------
// Step 18 — Achieve::capture_data 0x007AF980 (Partial)
// ---------------------------------------------------------------------------

/// `Achieve.head` is the 4 bytes at 0x00E885A0: `max_record_times: u16`
/// (low) then `rate: u16` (high).
fn achieve_rate(save: &Save) -> i32 {
    ((save.achieve.head as u32) >> 16) as i32
}

/// Overwrite the last element of a `SimpleArray<int>` if it has one —
/// `FUN_007af270(.., value, 0)`: `if (length-1 >= 0) list[length-1] = value`.
/// Returns true when a write happened.
fn overwrite_last(v: &mut crate::prim::SimpleVec, value: i32) -> bool {
    let n = v.data.len();
    if n >= 4 {
        put_i32(&mut v.data, n - 4, value);
        true
    } else {
        false
    }
}

fn achieve_capture_data(save: &mut Save, effects: &mut Vec<String>) {
    // 007af980.c:7 — `if (DAT_00e885a2 != 0)`.
    let rate = achieve_rate(save);
    if rate == 0 {
        return;
    }
    let f = frame(save);
    // 007af980.c:8 — `if (frame % rate != 0) record_data(0)` else record_data(1) + condense.
    if f % rate != 0 {
        achieve_record_data_overwrite(save, f, effects);
    } else {
        effects.push(format!(
            "stop: Achieve::capture_data interval frame (frame {f} % rate {rate} == 0): record_data(1) \
             append + min/max + condense untranscribed, untouched"
        ));
    }
}

/// `Achieve::record_data(0)` 0x007AEC50 — overwrite-last mode for records
/// 0, 1, 2, 5 and `times`. Records 3 and 4 are stopping points.
fn achieve_record_data_overwrite(save: &mut Save, f: i32, effects: &mut Vec<String>) {
    if save.achieve.data.len() < 6 {
        return;
    }
    let land_size = save.world.direct.get(28).copied().unwrap_or(0);
    let mut wrote = 0usize;
    let mut skipped = Vec::new();
    for p in 0..8usize {
        let Some(leader) = save.leaders.slots.get(p) else { break };
        // 007aec50.c:21 — `(*(byte*)(Leader+0) & 1) != 0` (active slot).
        if leader.flags & 1 == 0 || leader.body.len() < 0x9d4 {
            continue;
        }
        let score_units_2 = get_i32(&leader.body, 0x20); // LeaderData+0x28
        let territory = get_i32(&leader.body, 0x9d0); // LeaderData+0x9d8
        let score = get_i32(&leader.body, 0x10); // LeaderData+0x18

        // record 1 ← score_units_2 (0x007aec93, this=0x00E87EA0)
        if let Some(v) = save.achieve.data[1].l.get_mut(p) {
            wrote += overwrite_last(v, score_units_2) as usize;
        }
        // record 2 ← territory*1000 / land_size (0x007aecb6, this=0x00E87FE0; idiv)
        if land_size != 0 {
            let v2 = territory.wrapping_mul(1000) / land_size;
            if let Some(v) = save.achieve.data[2].l.get_mut(p) {
                wrote += overwrite_last(v, v2) as usize;
            }
        } else {
            skipped.push(format!("record 2 leader {p}: World.land_size == 0 (retail would fault)"));
        }
        // record 0 ← score (0x007aecca, this=0x00E87D60)
        if let Some(v) = save.achieve.data[0].l.get_mut(p) {
            wrote += overwrite_last(v, score) as usize;
        }
        // record 4 (tech hash, 0x007aed7e) and record 3 (income sum,
        // 0x007aede4): need LeaderData::has_tech / type_avail — stop.
        // record 5 ← 8 - p (0x007aedf5, this=0x00E883A0)
        if let Some(v) = save.achieve.data[5].l.get_mut(p) {
            wrote += overwrite_last(v, 8 - p as i32) as usize;
        }
    }
    // 007aec50.c:104 — `times[length-1] = Game.frame` (mode 0).
    if overwrite_last(&mut save.achieve.list, f) {
        wrote += 1;
    }
    if wrote > 0 {
        effects.push(format!(
            "Achieve::record_data(0): overwrote {wrote} last samples (records 0,1,2,5 + times) at frame {f}"
        ));
    }
    effects.push(
        "stop: Achieve::record_data(0) records 3 (income Σ via type_avail) and 4 (tech hash via has_tech) \
         untranscribed, untouched"
            .into(),
    );
    for s in skipped {
        effects.push(format!("stop: {s}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn capture_saves() -> Vec<PathBuf> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema/live/frame-pairs");
        let Ok(rd) = std::fs::read_dir(&root) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for dir in rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()) {
            if let Ok(files) = std::fs::read_dir(&dir) {
                out.extend(
                    files
                        .filter_map(|e| e.ok())
                        .map(|e| e.path())
                        .filter(|p| p.extension().is_some_and(|x| x == "svx")),
                );
            }
        }
        out.sort();
        out
    }

    #[test]
    fn statuses() {
        for idx in STEP_IDS {
            let st = step_status(idx);
            match idx {
                1 | 2 | 26 => assert_eq!(st, StepStatus::Ported, "step {idx}"),
                18 => assert_eq!(st, StepStatus::Partial, "step {idx}"),
                _ => assert_eq!(st, StepStatus::Stub, "step {idx}"),
            }
        }
    }

    /// With every gate closed (as in all captured frames) none of the
    /// eleven steps may change a single walked byte — `introduced` must
    /// stay 0. Also records which gates the captures actually arm.
    #[test]
    fn closed_gates_introduce_nothing() {
        let saves = capture_saves();
        if saves.is_empty() {
            eprintln!("no captures under schema/live/frame-pairs — skipping");
            return;
        }
        let mut checked = 0;
        for path in saves {
            let raw = crate::container::load_svx(&path).expect("svx");
            let img = crate::load(&raw).expect("load");
            let mut ours = img.state.clone();
            let mut effects = Vec::new();
            for idx in STEP_IDS {
                run_step(idx, &mut ours, &mut effects);
            }
            let before = crate::save(&mut img.state.clone()).expect("save a");
            let after = crate::save(&mut ours).expect("save b");
            assert_eq!(before.len(), after.len(), "{}", path.display());
            assert!(before == after, "{}: misc steps introduced bytes", path.display());
            // Gates we expect closed in the idle captures; a capture that
            // arms one is interesting, so say so loudly rather than fail.
            for e in &effects {
                eprintln!("{}: {e}", path.file_name().unwrap().to_string_lossy());
            }
            assert_eq!(achieve_rate(&img.state), 0, "{}: Achieve.rate", path.display());
            checked += 1;
        }
        eprintln!("checked {checked} captures");
    }

    /// Synthetic exercise of the step-18 mode-0 transcription: arm
    /// `Achieve.rate`, give each walked array one sample, and check the
    /// overwrite-last writes land where `FUN_007af270(.., 0)` puts them.
    #[test]
    fn achieve_record_data_overwrite_last() {
        let Some(path) = capture_saves().into_iter().next() else {
            eprintln!("no captures — skipping");
            return;
        };
        let raw = crate::container::load_svx(&path).expect("svx");
        let img = crate::load(&raw).expect("load");
        let mut s = img.state.clone();
        // rate = 7 (high u16), max_record_times = 100 (low u16).
        s.achieve.head = ((7u32 << 16) | 100) as i32;
        put_i32(&mut s.game.scalars, FRAME, 13); // 13 % 7 != 0 -> record_data(0)
        let f = frame(&s);
        let active: Vec<usize> =
            (0..8).filter(|&p| s.leaders.slots[p].flags & 1 != 0 && s.leaders.slots[p].body.len() >= 0x9d4).collect();
        assert!(!active.is_empty());
        for r in 0..6 {
            for p in 0..8 {
                s.achieve.data[r].l[p].data = vec![0xEE; 8]; // two sentinel samples
            }
        }
        s.achieve.list.data = vec![0xEE; 8];
        let mut effects = Vec::new();
        run_step(18, &mut s, &mut effects);
        let land = s.world.direct[28];
        assert_ne!(land, 0);
        for &p in &active {
            let body = &s.leaders.slots[p].body;
            let expect = [
                (0usize, get_i32(body, 0x10)),
                (1, get_i32(body, 0x20)),
                (2, get_i32(body, 0x9d0).wrapping_mul(1000) / land),
                (5, 8 - p as i32),
            ];
            for (r, v) in expect {
                let d = &s.achieve.data[r].l[p].data;
                assert_eq!(get_i32(d, 0), 0xEEEEEEEEu32 as i32, "record {r} leader {p}: first sample untouched");
                assert_eq!(get_i32(d, 4), v, "record {r} leader {p}: last sample");
            }
            for r in [3usize, 4] {
                assert_eq!(s.achieve.data[r].l[p].data, vec![0xEE; 8], "record {r} is a stopping point");
            }
        }
        for p in 0..8 {
            if !active.contains(&p) {
                for r in 0..6 {
                    assert_eq!(s.achieve.data[r].l[p].data, vec![0xEE; 8], "inactive leader {p} untouched");
                }
            }
        }
        assert_eq!(get_i32(&s.achieve.list.data, 4), f, "times[last] = frame");
        assert_eq!(get_i32(&s.achieve.list.data, 0), 0xEEEEEEEEu32 as i32);
        assert!(effects.iter().any(|e| e.starts_with("Achieve::record_data(0)")), "{effects:?}");
        assert!(effects.iter().any(|e| e.contains("records 3") && e.contains("4")), "{effects:?}");

        // Interval frame: nothing written, stopping point reported.
        let mut s2 = img.state.clone();
        s2.achieve.head = ((7u32 << 16) | 100) as i32;
        put_i32(&mut s2.game.scalars, FRAME, 70);
        for r in 0..6 {
            for p in 0..8 {
                s2.achieve.data[r].l[p].data = vec![0xEE; 8];
            }
        }
        s2.achieve.list.data = vec![0xEE; 8];
        let mut effects = Vec::new();
        run_step(18, &mut s2, &mut effects);
        assert_eq!(s2.achieve.list.data, vec![0xEE; 8]);
        assert!(effects.iter().any(|e| e.contains("interval frame")), "{effects:?}");
    }
}
