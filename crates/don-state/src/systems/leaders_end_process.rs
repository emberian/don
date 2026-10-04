//! Steps 17 and 19: `Leaders::end_process_all` 0x006ED070 and the
//! `Leader::process_event_frame` 0x006EC180 loop that `Game::do_frame`
//! runs inline (`re/decomp-all/00591ef0.c:229-233`: nine Leader records at
//! stride 0x6eec from 0x00e3a390, gated on `leader_flags & 1`).
//!
//! Neither function draws from `GameAccess::game_random`. The only RNG use
//! reachable from either is `SoundGlobal::play` 0x0097f770, which calls
//! `Random::get` on `SoundGlobal::random` (0x00e85f0c, `mov ecx, 0xe85f0c`
//! at 0x0097f78d), and `JukeBox::set_next_mood` → `FUN_0097cc90`, which
//! uses the JukeBox-owned `Random` at `JukeBox+0x44` (seeded from
//! `timeGetTime`). Main-LCG draws per frame from these two steps: 0.
//!
//! # Step 17 — `Leaders::end_process_all` 0x006ED070 (`006ed070.c`)
//!
//! For each of the nine Leader records with `leader_flags & 2`:
//!
//! * `pop_issues` (LeaderData+0x7e8) `== 0`: for each of the eight
//!   `Game::info.player[i]` (Game+0x44 + i*0x8c; `Player` sizeof 0x8c) with
//!   `flags & 1` (Player+0x30) and `who` (Player+0x33) `== leader.who`,
//!   clear bit 0x800 of `Player::flags`. Both walked images of the Player
//!   record are updated (`GameInfo.player[i].flags` and `body[0x30..0x32]`,
//!   the same retail memory; `GameInfo` is walked twice — top level and
//!   inside `Game`).
//! * otherwise, if `leader.who == Console::who` (`[0x00c06210]+0x298`,
//!   walked as `Save.console1[0..4]`): when
//!   `Game::frame - Player[Console::play].pop_cap_frame > 0x1c1`, set
//!   `pop_cap_frame = Game::frame` (Player+0x2c, `body[0x2c..0x30]`;
//!   `Console::play` = `Save.console1[8..12]`). The remaining branch is
//!   UI only — `pop_cap < Categories pop_limits[Game::info.pop_limit].cap`
//!   gates `MessageWin::add_feedback` 0x007E9AB0 + `SoundGlobal::play`
//!   0x0097F770; neither writes walked state.
//!
//! Every walked write of the function is transcribed, so the step is
//! `Ported`. The lone caveat: a record with `leader_flags & 2` but not
//! `& 1` has no serialized body, so `pop_issues`/`who` cannot be read and
//! the record is skipped (retail would read live memory).
//!
//! # Step 19 — `Leader::process_event_frame` 0x006EC180 (`006ec180.c`)
//!
//! Early return unless `Game::frame % 50 == 0` (0x006EC1A2..0x006EC1B5).
//! Otherwise, on the LeaderData battle-statistics block (all `u16`, movzx
//! loads, 0x006EC1BB..0x006EC2D3):
//!
//! 1. `deaths_fifteen_seconds += deaths_current_frame` (+0xa60 += +0xa58),
//!    `damage_fifteen_seconds += damage_current_frame` (+0xa66 += +0xa5e),
//!    `kills_fifteen_seconds += kills_current_frame` (+0xa62 += +0xa5a),
//!    `hits_fifteen_seconds += hits_current_frame` (+0xa64 += +0xa5c).
//! 2. each `*_current_frame *= 100` (u16 wrap); then each average is
//!    `cur == 0 ? avg*7 >> 3 : (cur + avg) >> 1`:
//!    `average_death_rate` +0xa50 ← deaths, `average_kill_rate` +0xa52 ←
//!    kills, `average_hit_rate` +0xa56 ← hits, `average_damage_rate`
//!    +0xa54 ← damage.
//! 3. `who == Console::who` branch (0x006EC2DA..0x006EC44D): computes a
//!    battle mood from hit/damage rates and `LeaderData::get_team_score`
//!    0x006D6520 and writes only `JukeBox` globals (0x00ecba2c) /
//!    `JukeBox::set_next_mood` 0x0097D5D0. No walked state; not executed.
//! 4. Battle-event detection (0x006EC452..0x006EC4FF): with
//!    `age = Leader[who].data_encrypted->ages ^ 0x62766` (the serialized,
//!    decrypted value — `data_encrypted` bytes 236..240, serialization
//!    index 59 of `FUN_006d9900`): if
//!    `(int)(avg_death + avg_kill) >= (age+1)*125` and
//!    (`frame_battle == 0` or `frame - frame_battle >= 0x708`), then if
//!    `avg_death >= age*10 + 20 + avg_kill` → event type 1, else if
//!    `avg_kill >= age*10 + 20 + avg_death` → event type 0, else nothing.
//!    On an event: `Achieve::add_event(type, who, EMPTY_STRING)` 0x007AF660
//!    — UNTRANSCRIBED (appends `{frame, type, ""}` to the walked
//!    `Achieve.events[who]` ObjectArray; growth goes through the array
//!    vtable) — then `average_death_rate = average_kill_rate = 0xfc18`
//!    (`mov dword [+0xa50], 0xfc18fc18`) and `frame_battle = Game::frame`
//!    (+0xa4c).
//! 5. `deaths_current_frame = kills_current_frame = 0` (`mov dword
//!    [+0xa58], 0`) and `hits_current_frame = damage_current_frame = 0`
//!    (`mov dword [+0xa5c], 0`).
//!
//! All LeaderData writes are transcribed; the `Achieve.events` append in
//! the battle branch is not, so the step is `Partial`.
//!
//! Byte-image mapping: `Leader.body[k]` is LeaderData image offset `8 + k`
//! (see `spandiff::image_base`), so e.g. `average_death_rate` +0xa50 is
//! `body[0xa48..0xa4a]`.
//!
//! # Wiring note
//!
//! `tick.rs` routes both steps through `run`. The two bodies are exposed as
//! [`end_process_all`] (step 17) and [`process_event_frame_all`] (step 19);
//! `run` alternates between them per call (17 first) because nothing in the
//! save distinguishes the two call sites. The lead should wire
//! `17 => end_process_all`, `19 => process_event_frame_all` directly and
//! drop the toggle.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::tick::{StepStatus, FRAME};
use crate::Save;

/// Step 17: every walked write transcribed (Player flag clear and the
/// local-player `pop_cap_frame` stamp).
pub const STATUS_END_PROCESS_ALL: StepStatus = StepStatus::Ported;
/// Step 19: all LeaderData writes transcribed; the `Achieve::add_event`
/// append in the battle-event branch is not.
pub const STATUS_PROCESS_EVENT_FRAME: StepStatus = StepStatus::Partial;
/// Shared status while `tick.rs` routes both steps through `run`
/// (conservative: the weaker of the two).
pub const STATUS: StepStatus = StepStatus::Partial;

// --- LeaderData image offsets (body index = image offset − 8) ---------------
const LD_BASE: usize = 0x8;
const LD_WHO: usize = 0x8;
const LD_POP_ISSUES: usize = 0x7e8;
const LD_FRAME_BATTLE: usize = 0xa4c;
const LD_AVG_DEATH: usize = 0xa50;
const LD_AVG_KILL: usize = 0xa52;
const LD_AVG_DAMAGE: usize = 0xa54;
const LD_AVG_HIT: usize = 0xa56;
const LD_DEATHS_CUR: usize = 0xa58;
const LD_KILLS_CUR: usize = 0xa5a;
const LD_HITS_CUR: usize = 0xa5c;
const LD_DAMAGE_CUR: usize = 0xa5e;
const LD_DEATHS_15: usize = 0xa60;
const LD_KILLS_15: usize = 0xa62;
const LD_HITS_15: usize = 0xa64;
const LD_DAMAGE_15: usize = 0xa66;
/// `LeaderDataEncrypt::ages` is serialized (decrypted) at index 59 of the
/// 62-dword `data_encrypted` block (`FUN_006d9900`: 6×9 column dwords,
/// +0x48, +0xe8..+0xf8, then +0xdc, +0xe0, +0xe4).
const ENC_AGES_SER: usize = 59 * 4;

// --- Player image offsets inside `GameInfo.player[i].body` (Player+0x00..) --
const PL_POP_CAP_FRAME: usize = 0x2c;
const PL_FLAGS: usize = 0x30;
const PL_WHO: usize = 0x33;

// --- Console block (`Save.console1` = Console+0x298..+0x2b8) -----------------
const CON_WHO: usize = 0x298 - 0x298;
const CON_PLAY: usize = 0x2a0 - 0x298;

fn get_i32(buf: &[u8], off: usize) -> i32 {
    i32::from_le_bytes(buf[off..off + 4].try_into().unwrap())
}

fn put_i32(buf: &mut [u8], off: usize, v: i32) {
    buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

fn get_u16(buf: &[u8], off: usize) -> u16 {
    u16::from_le_bytes(buf[off..off + 2].try_into().unwrap())
}

fn put_u16(buf: &mut [u8], off: usize, v: u16) {
    buf[off..off + 2].copy_from_slice(&v.to_le_bytes());
}

/// LeaderData field accessors over `Leader.body` (image offset − 8).
fn ld_i32(body: &[u8], img: usize) -> i32 {
    get_i32(body, img - LD_BASE)
}
fn ld_u16(body: &[u8], img: usize) -> u16 {
    get_u16(body, img - LD_BASE)
}
fn ld_put_i32(body: &mut [u8], img: usize, v: i32) {
    put_i32(body, img - LD_BASE, v)
}
fn ld_put_u16(body: &mut [u8], img: usize, v: u16) {
    put_u16(body, img - LD_BASE, v)
}

static PHASE_EVENT_FRAME: AtomicBool = AtomicBool::new(false);

/// Dispatcher used by `tick.rs` for both step 17 and step 19: alternates
/// `end_process_all` (first call) and `process_event_frame_all` (second).
pub fn run(save: &mut Save, effects: &mut Vec<String>) {
    if PHASE_EVENT_FRAME.swap(true, Ordering::Relaxed) {
        PHASE_EVENT_FRAME.store(false, Ordering::Relaxed);
        process_event_frame_all(save, effects);
    } else {
        end_process_all(save, effects);
    }
}

/// Step 17 — `Leaders::end_process_all` 0x006ED070.
pub fn end_process_all(save: &mut Save, effects: &mut Vec<String>) {
    let frame = get_i32(&save.game.scalars, FRAME);
    let console_who = get_i32(&save.console1, CON_WHO);
    let console_play = get_i32(&save.console1, CON_PLAY);

    for (li, leader) in save.leaders.slots.iter().enumerate() {
        // `puVar3[-2] & 2` — leader_flags bit 1.
        if leader.flags & 2 == 0 {
            continue;
        }
        if leader.flags & 1 == 0 || leader.body.len() < LD_POP_ISSUES + 4 - LD_BASE {
            // No serialized body: `pop_issues`/`who` are not available.
            continue;
        }
        let who = ld_i32(&leader.body, LD_WHO);
        let pop_issues = ld_i32(&leader.body, LD_POP_ISSUES);
        if pop_issues == 0 {
            // 006ed070.c:21-44 — eight unrolled Player slots: clear 0x800.
            for pi in 0..8 {
                let mut changed = false;
                for info in [&mut save.info, &mut save.game.info] {
                    let Some(pl) = info.players.get_mut(pi) else { continue };
                    if pl.flags & 1 == 0 || pl.body.len() < 0x39 {
                        continue;
                    }
                    if pl.body[PL_WHO] as i32 != who {
                        continue;
                    }
                    let f = get_u16(&pl.body, PL_FLAGS);
                    if f & 0x800 != 0 {
                        changed = true;
                    }
                    let nf = f & 0xf7ff;
                    put_u16(&mut pl.body, PL_FLAGS, nf);
                    pl.flags = nf;
                }
                if changed {
                    effects.push(format!(
                        "GameInfo.player[{pi}].flags &= ~0x800 (Leader[{li}] who {who} pop_issues == 0)"
                    ));
                }
            }
        } else if who == console_who {
            // 006ed070.c:46-49 — local player's pop-cap warning stamp.
            let Ok(play) = usize::try_from(console_play) else { continue };
            let mut stamped = false;
            for info in [&mut save.info, &mut save.game.info] {
                let Some(pl) = info.players.get_mut(play) else { continue };
                if pl.flags & 1 == 0 || pl.body.len() < 0x39 {
                    continue;
                }
                let stamp = get_i32(&pl.body, PL_POP_CAP_FRAME);
                if frame.wrapping_sub(stamp) > 0x1c1 {
                    put_i32(&mut pl.body, PL_POP_CAP_FRAME, frame);
                    stamped = true;
                }
            }
            if stamped {
                effects.push(format!(
                    "GameInfo.player[{play}].pop_cap_frame = {frame} (Leader[{li}] who {who} pop_issues != 0)"
                ));
            }
            // `pop_cap < pop_limits[info.pop_limit].cap` → MessageWin /
            // SoundGlobal: UI only, no walked state.
        }
    }
}

/// Step 19 — the `Game::do_frame` loop over nine Leader records calling
/// `Leader::process_event_frame` 0x006EC180 for each with `leader_flags & 1`.
pub fn process_event_frame_all(save: &mut Save, effects: &mut Vec<String>) {
    let frame = get_i32(&save.game.scalars, FRAME);
    // 0x006EC1A2..0x006EC1B5: `frame % 50 != 0` → return.
    if frame % 0x32 != 0 {
        return;
    }
    // `ages` per who, read before mutating (`Leader[who]` is indexed by the
    // record's `who`, not the slot).
    let ages: Vec<Option<i32>> = save
        .leaders
        .slots
        .iter()
        .map(|l| {
            (l.flags & 1 != 0 && l.data_encrypted.len() >= ENC_AGES_SER + 4)
                .then(|| get_i32(&l.data_encrypted, ENC_AGES_SER))
        })
        .collect();

    for li in 0..save.leaders.slots.len() {
        let leader = &mut save.leaders.slots[li];
        if leader.flags & 1 == 0 || leader.body.len() < LD_DAMAGE_15 + 2 - LD_BASE {
            continue;
        }
        let body = &mut leader.body;

        // 1. fifteen-second rollups.
        let deaths_cur = ld_u16(body, LD_DEATHS_CUR);
        let damage_cur = ld_u16(body, LD_DAMAGE_CUR);
        let kills_cur = ld_u16(body, LD_KILLS_CUR);
        let hits_cur = ld_u16(body, LD_HITS_CUR);
        for (acc, cur) in [
            (LD_DEATHS_15, deaths_cur),
            (LD_DAMAGE_15, damage_cur),
            (LD_KILLS_15, kills_cur),
            (LD_HITS_15, hits_cur),
        ] {
            let v = ld_u16(body, acc).wrapping_add(cur);
            ld_put_u16(body, acc, v);
        }

        // 2. `*_current_frame *= 100` then rate averaging.
        let deaths100 = deaths_cur.wrapping_mul(100);
        let damage100 = damage_cur.wrapping_mul(100);
        let kills100 = kills_cur.wrapping_mul(100);
        let hits100 = hits_cur.wrapping_mul(100);
        ld_put_u16(body, LD_DEATHS_CUR, deaths100);
        ld_put_u16(body, LD_DAMAGE_CUR, damage100);
        ld_put_u16(body, LD_KILLS_CUR, kills100);
        ld_put_u16(body, LD_HITS_CUR, hits100);
        let avg = |cur: u16, prev: u16| -> u16 {
            if cur == 0 {
                ((prev as u32 * 7) >> 3) as u16
            } else {
                ((cur as u32 + prev as u32) >> 1) as u16
            }
        };
        let avg_death = avg(deaths100, ld_u16(body, LD_AVG_DEATH));
        ld_put_u16(body, LD_AVG_DEATH, avg_death);
        let avg_kill = avg(kills100, ld_u16(body, LD_AVG_KILL));
        ld_put_u16(body, LD_AVG_KILL, avg_kill);
        let avg_hit = avg(hits100, ld_u16(body, LD_AVG_HIT));
        ld_put_u16(body, LD_AVG_HIT, avg_hit);
        let avg_damage = avg(damage100, ld_u16(body, LD_AVG_DAMAGE));
        ld_put_u16(body, LD_AVG_DAMAGE, avg_damage);
        let _ = avg_hit;

        let who = ld_i32(body, LD_WHO);
        effects.push(format!(
            "Leader[{li}] who {who}: rates death/kill/damage/hit -> {avg_death}/{avg_kill}/{avg_damage}/{avg_hit}, \
             fifteen += {deaths_cur}/{kills_cur}/{hits_cur}/{damage_cur}, current zeroed (frame {frame} % 50 == 0)"
        ));

        // 3. `who == Console::who` JukeBox mood branch: globals only, skipped.

        // 4. battle-event detection.
        let age = usize::try_from(who).ok().and_then(|w| ages.get(w).copied().flatten());
        if let Some(age) = age {
            let sum = avg_death as i32 + avg_kill as i32;
            let frame_battle = ld_i32(body, LD_FRAME_BATTLE);
            if sum >= (age.wrapping_add(1)).wrapping_mul(0x7d)
                && (frame_battle == 0 || frame.wrapping_sub(frame_battle) >= 0x708)
            {
                let thresh = age.wrapping_mul(10).wrapping_add(0x14);
                let event = if avg_death as i32 >= thresh.wrapping_add(avg_kill as i32) {
                    Some(1)
                } else if avg_kill as i32 >= thresh.wrapping_add(avg_death as i32) {
                    Some(0)
                } else {
                    None
                };
                if let Some(ev) = event {
                    // Achieve::add_event(ev, who, EMPTY_STRING) 0x007AF660:
                    // UNTRANSCRIBED (Achieve.events[who] append).
                    ld_put_u16(body, LD_AVG_DEATH, 0xfc18);
                    ld_put_u16(body, LD_AVG_KILL, 0xfc18);
                    ld_put_i32(body, LD_FRAME_BATTLE, frame);
                    effects.push(format!(
                        "Leader[{li}] who {who}: battle event type {ev} (age {age}); \
                         average_death_rate = average_kill_rate = 0xfc18, frame_battle = {frame}; \
                         Achieve::add_event NOT transcribed"
                    ));
                }
            }
        }

        // 5. zero the current-frame counters (two dword stores).
        ld_put_u16(body, LD_DEATHS_CUR, 0);
        ld_put_u16(body, LD_KILLS_CUR, 0);
        ld_put_u16(body, LD_HITS_CUR, 0);
        ld_put_u16(body, LD_DAMAGE_CUR, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sections::{GameInfo, Leader, PlayerInfo};

    fn leader(flags: i32, who: i32) -> Leader {
        let mut l = Leader::default();
        l.flags = flags;
        l.body = vec![0u8; 0x6922];
        ld_put_i32(&mut l.body, LD_WHO, who);
        l.data_encrypted = vec![0u8; 62 * 4];
        l
    }

    fn player(who: u8, flags: u16) -> PlayerInfo {
        let mut p = PlayerInfo::default();
        p.flags = flags;
        p.body = vec![0u8; 0x39];
        put_u16(&mut p.body, PL_FLAGS, flags);
        p.body[PL_WHO] = who;
        p
    }

    fn save_with(frame: i32, leaders: Vec<Leader>, players: Vec<PlayerInfo>) -> Save {
        let mut s = Save::default();
        s.game.scalars = vec![0u8; 404];
        put_i32(&mut s.game.scalars, FRAME, frame);
        s.console1 = vec![0u8; 0x20];
        let mut info = GameInfo::default();
        info.players = players;
        s.info = info.clone();
        s.game.info = info;
        s.leaders.slots = leaders;
        s
    }

    #[test]
    fn end_process_all_clears_pop_cap_flag_when_no_issues() {
        let mut l = leader(3, 2);
        ld_put_i32(&mut l.body, LD_POP_ISSUES, 0);
        let players = vec![player(0, 0x801), player(2, 0x801), player(2, 0x800)];
        let mut s = save_with(100, vec![l], players);
        let mut fx = Vec::new();
        end_process_all(&mut s, &mut fx);
        for info in [&s.info, &s.game.info] {
            assert_eq!(info.players[0].flags, 0x801, "other who untouched");
            assert_eq!(info.players[1].flags, 0x001);
            assert_eq!(get_u16(&info.players[1].body, PL_FLAGS), 0x001);
            assert_eq!(info.players[2].flags, 0x800, "flags&1 == 0 skipped");
        }
        assert_eq!(fx.len(), 1, "{fx:?}");
    }

    #[test]
    fn end_process_all_stamps_local_pop_cap_frame() {
        let mut l = leader(3, 1);
        ld_put_i32(&mut l.body, LD_POP_ISSUES, 5);
        let mut s = save_with(1000, vec![l], vec![player(0, 1), player(1, 1)]);
        put_i32(&mut s.console1, CON_WHO, 1);
        put_i32(&mut s.console1, CON_PLAY, 1);
        put_i32(&mut s.info.players[1].body, PL_POP_CAP_FRAME, 1000 - 0x1c1);
        put_i32(&mut s.game.info.players[1].body, PL_POP_CAP_FRAME, 1000 - 0x1c1);
        let mut fx = Vec::new();
        end_process_all(&mut s, &mut fx);
        assert_eq!(get_i32(&s.info.players[1].body, PL_POP_CAP_FRAME), 1000 - 0x1c1, "exactly 0x1c1: not >");
        assert!(fx.is_empty());
        put_i32(&mut s.info.players[1].body, PL_POP_CAP_FRAME, 1000 - 0x1c2);
        put_i32(&mut s.game.info.players[1].body, PL_POP_CAP_FRAME, 1000 - 0x1c2);
        end_process_all(&mut s, &mut fx);
        assert_eq!(get_i32(&s.info.players[1].body, PL_POP_CAP_FRAME), 1000);
        assert_eq!(get_i32(&s.game.info.players[1].body, PL_POP_CAP_FRAME), 1000);
        assert_eq!(get_i32(&s.info.players[0].body, PL_POP_CAP_FRAME), 0);
        assert_eq!(fx.len(), 1);
    }

    #[test]
    fn process_event_frame_skips_off_frames() {
        let mut l = leader(1, 0);
        ld_put_u16(&mut l.body, LD_DEATHS_CUR, 3);
        let before = l.body.clone();
        let mut s = save_with(49, vec![l], vec![]);
        let mut fx = Vec::new();
        process_event_frame_all(&mut s, &mut fx);
        assert_eq!(s.leaders.slots[0].body, before);
        assert!(fx.is_empty());
    }

    #[test]
    fn process_event_frame_rolls_up_and_averages() {
        let mut l = leader(1, 0);
        ld_put_u16(&mut l.body, LD_DEATHS_CUR, 3);
        ld_put_u16(&mut l.body, LD_KILLS_CUR, 0);
        ld_put_u16(&mut l.body, LD_HITS_CUR, 7);
        ld_put_u16(&mut l.body, LD_DAMAGE_CUR, 2);
        ld_put_u16(&mut l.body, LD_DEATHS_15, 10);
        ld_put_u16(&mut l.body, LD_AVG_DEATH, 100);
        ld_put_u16(&mut l.body, LD_AVG_KILL, 80);
        ld_put_u16(&mut l.body, LD_AVG_HIT, 1);
        ld_put_u16(&mut l.body, LD_AVG_DAMAGE, 0);
        // age 0 → threshold 125; avg_death 200 + avg_kill 70 = 270 ≥ 125,
        // and avg_death 200 ≥ 20 + 70 → event type 1.
        let mut s = save_with(150, vec![l], vec![]);
        let mut fx = Vec::new();
        process_event_frame_all(&mut s, &mut fx);
        let b = &s.leaders.slots[0].body;
        assert_eq!(ld_u16(b, LD_DEATHS_15), 13);
        assert_eq!(ld_u16(b, LD_HITS_15), 7);
        assert_eq!(ld_u16(b, LD_DAMAGE_15), 2);
        assert_eq!(ld_u16(b, LD_KILLS_15), 0);
        assert_eq!(ld_u16(b, LD_AVG_HIT), (700 + 1) >> 1);
        assert_eq!(ld_u16(b, LD_AVG_DAMAGE), (200 + 0) >> 1);
        // event fired: death/kill averages reset, frame_battle stamped.
        assert_eq!(ld_u16(b, LD_AVG_DEATH), 0xfc18);
        assert_eq!(ld_u16(b, LD_AVG_KILL), 0xfc18);
        assert_eq!(ld_i32(b, LD_FRAME_BATTLE), 150);
        for f in [LD_DEATHS_CUR, LD_KILLS_CUR, LD_HITS_CUR, LD_DAMAGE_CUR] {
            assert_eq!(ld_u16(b, f), 0);
        }
        assert_eq!(fx.len(), 2, "{fx:?}");
    }

    #[test]
    fn process_event_frame_no_event_below_threshold() {
        let mut l = leader(1, 0);
        ld_put_u16(&mut l.body, LD_AVG_DEATH, 100);
        ld_put_u16(&mut l.body, LD_AVG_KILL, 80);
        // decay only: 100*7>>3 = 87, 80*7>>3 = 70; sum 157 ≥ 125 but
        // neither dominates by 20 → no event.
        let mut s = save_with(50, vec![l], vec![]);
        let mut fx = Vec::new();
        process_event_frame_all(&mut s, &mut fx);
        let b = &s.leaders.slots[0].body;
        assert_eq!(ld_u16(b, LD_AVG_DEATH), 87);
        assert_eq!(ld_u16(b, LD_AVG_KILL), 70);
        assert_eq!(ld_i32(b, LD_FRAME_BATTLE), 0);
        assert_eq!(fx.len(), 1);
    }

    #[test]
    fn process_event_frame_idle_leader_is_a_no_op() {
        let l = leader(1, 4);
        let before = l.body.clone();
        let mut s = save_with(0, vec![l], vec![]);
        let mut fx = Vec::new();
        process_event_frame_all(&mut s, &mut fx);
        assert_eq!(s.leaders.slots[0].body, before);
    }
}
