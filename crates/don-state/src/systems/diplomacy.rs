//! Diplomacy state transitions: `Leader::set_diplo` 0x006EC6A0 and the
//! command/timer bodies that reach it. Leaf module: exposes free functions
//! for the owning traversals (`commands.rs` for opcodes 38/41, `misc_steps`
//! step 10, `research.rs` for the rush-age unlock) to call; never edits
//! sibling modules.
//!
//! Every function here is a line-by-line transcription of the Capstone
//! listing (`ron-bin/riseofnations.exe`, `re/decomp-all/<EA>.c` as the
//! control-flow map). Presentation calls (`MessageWin::*`, `SoundGlobal::play`,
//! `IFaceMainBase::*`, `Leader::chat_to_local`, `Camera::outdate`,
//! `IFaceData+0x22a = 1`) are noted inline and not modelled; none of them
//! touches walked state and none draws on `GameAccess::game_random`.
//!
//! # Leader image
//!
//! `Leaders[i]` live at `0x00e3a390 + i*0x6eec` (`LeaderData` sizeof 0x6eec;
//! eight slots are walked by every loop here, `0x00e3a390..0x00e71af0`).
//! `Save.leaders.slots[i].body` is image `+0x08..+0x692a`, so
//! `body[off - 8]`. Fields used (`re/scripts/pdb_layout.py LeaderData`):
//! `who` +0x8, `diplos[8]` +0x74 (0 war / 1 peace / 2 ally), `treaties[8]`
//! +0x94 (bit mask; bit 1 = "met"), `attrition_stamp` +0x1f4, `blacken`
//! +0x20c, `dow[8]` +0x270, `invaders[8]` +0x290, `broke_alliance[8]`
//! +0x2b0, `counteroffer[8]` +0x314, `tribute_demanded[8]` +0x334,
//! `escrow[6]` +0x468, `tribute_sent` +0x860... (+0x498 `tributes[6]`),
//! `ally_mask` +0x6929 (one bit per leader; read by `GraphicEvents` for
//! shared line-of-sight), `dip[8]` +0x692c (`Diplomacy` sizeof 0x5c:
//! `agree` +0, `any_offer` +4, `treaty` +8, `offers[6]` +0xc, `dows[6]`
//! +0x24, `attacks[8]` +0x3c) — walked as `Save.leaders.slots[i].diplomacy`.
//!
//! # `Leader::set_diplo(int whom, int state)` 0x006EC6A0 (783 B)
//!
//! ```text
//! 006ec6bb  if diplos[whom] == state: return
//! 006ec6c5  if diplos[whom] == 2:                                 // leaving alliance
//! 006ec6da     Leaders[who].ally_mask  &= ~(1 << whom)             // btr
//! 006ec6ea     Leaders[whom].ally_mask &= ~(1 << who)
//! 006ec70a     Leaders[who].eject_my_shit_from_his_ass(whom)       // Objects: units garrisoned in
//! 006ec718     Leaders[whom].eject_my_shit_from_his_ass(who)       // the ex-ally's buildings come out
//! 006ec720  if who != Console.who && whom != Console.who
//!             && console_leader.has_treaty(who,1) && console_leader.has_treaty(whom,1):
//!                MessageWin::add_message + SoundGlobal::play(0x135)  // presentation only
//! 006ec869  diplos[whom] = state
//! 006ec870  Leaders[whom].diplos[who] = state                      // matrix kept symmetric
//! 006ec877  if state == 2:
//! 006ec892     if Leaders[who].has_preq(0x2b0)  || Game.info.reveal_map >= 1: Leaders[who].ally_mask  |= 1 << whom
//! 006ec8ce     if Leaders[whom].has_preq(0x2b0) || Game.info.reveal_map >= 1: Leaders[whom].ally_mask |= 1 << who
//! 006ec8f5     n = #{ o in 0..8 : o != who, o != whom, Leaders[o].flags & 3 == 3,
//!                     !Leaders[o].is_ally(who), !Leaders[o].is_ally(whom) }
//! 006ec959     if n == 0: Leaders[who].victory(0, 0); Game.semaphore.set(0x16, 1)   // allied victory
//! 006ec98b  Armies::diplo_change(who)                              // AI armies; unwalked
//! 006ec995  IFaceData.diplo_dirty (+0x22a) = 1
//! ```
//!
//! `LeaderData::is_ally(o)` 0x006EDB50: `o == who || (diplos[o] == 2 &&
//! Leaders[o].diplos[who] == 2)`. `is_enemy(o)` 0x006EBAA0: `o != who &&
//! (diplos[o] == 0 || Leaders[o].diplos[who] == 0)`. `has_treaty(o, m)`
//! 0x006E11E0: `o >= 0 && treaties[o] & m`. `is_neutral()` 0x006EBAE0:
//! `team_style == 7 && players[get_player()].flags & 1 && players[..].side
//! == 8` with `get_player()` 0x006EC0F0 = first player slot whose `flags & 1
//! && who == this.who`, preferring one with `flags & 0x50 == 0`.
//!
//! `Leader::victory` 0x006EC9B0 (330 B): gate `leader_flags & 0x60 == 0`;
//! `leader_flags |= 0x20`; `flags2 = flags2 | 0x100` or `& ~0x100` (arg 2);
//! `victory_type` (+0x7d8) = arg 1; `Achieve::capture_event(0xb, who)`;
//! Objects loop (units index >= 2000, `is_hero`-like virtual → `vt+0xac` +
//! `Build::refund_cost`); for each player with `who == this.who`:
//! `Leader::score_update`-like 0x006EE290; then for every other
//! `flags & 3 == 3` leader: `flags |= 0x80`, allies recurse `victory`,
//! non-allies `Leader::defeat(5, -1, 0)` 0x006ECB00. Only the first three
//! writes are transcribed; the rest is reported as a stop.
//!
//! # `CommandPackage::process_declare` 0x009472B0 / `process_accept` 0x00946FB0
//!
//! Wire layout (`DeclareCommand`: opcode u8, `who` i32 @1, `whom` i32 @5,
//! `treaty` i32 @9 → returns 13; `AcceptCommand`: opcode, `who` @1, `whom`
//! @5 → returns 9). After `SyncLogger::logToMemory` / `Log::say` (no state):
//! `Leaders[who].action_declare(whom, treaty, 0, 0)` /
//! `Leaders[who].action_respond(whom, 1)`.
//!
//! # `Leader::action_declare(int whom, int treaty, int free, int forced)` 0x006DAB50 (829 B)
//!
//! Returns 1 when refused, 0 otherwise. Note the first line: against a
//! leader we are already at war with the command does nothing — peace is
//! never *declared*; it is offered (`dip[].offers`) and reached through
//! `action_respond`. `action_declare` only moves ally → peace → war.
//!
//! ```text
//! 006dab63  if is_enemy(whom): return 0
//! LOOP (006dab73):
//! 006dab73  if treaty == 0 && forced == 0:
//! 006dab7f     if is_ally(whom): treaty = 1; if is_enemy(whom) return 0; goto LOOP   // break alliance -> peace first
//! 006dab8d     stamp = max(broke_alliance[whom], Leaders[whom].broke_alliance[who])
//! 006daba9     if stamp != 0 && frame - stamp < Constants[0xcf8]:
//!                 (who == Console.who: feedback + SoundGlobal::play(0x40)); return 1   // truce cooldown
//! 006dabc7  if free == 0 && !afford_dow(whom, treaty, &short_good):
//!              LeaderOut::warn_resources(...); return 1
//! 006dabe2  if treaty == 0 && !Game::war_allowed():
//!              if forced == 0: (who == Console.who: Game::say_no_war + feedback); return 1
//!              treaty = 1; if is_enemy(whom) return 0; goto LOOP
//! 006dacec  if free == 0:
//! 006dacf6     pay_dow(whom, treaty)
//! 006dad05     if is_ally(whom) && treaty == 0: pay_dow(whom, treaty)        // second payment
//! 006dad14     if treaty == 0 && (is_ally(whom) || invaders[whom] < 5):
//!                 if dow[whom] != 0: blacken += 1
//!                 dow[whom] += 1
//! 006dad48     if is_ally(whom):
//!                 if dow[whom] != 0: blacken += 1
//!                 dow[whom] += 1
//! 006dad6b  if is_ally(whom): broke_alliance[whom] = frame
//! 006dad99  set_diplo(whom, treaty)
//! 006dadb5  for o in 0..8, o != who, o != whom, Leaders[o].flags & 1:
//!              if Leaders[o].is_team(who, 0):
//!                 if !Leaders[o].is_team(whom, 0) && treaty < Leaders[o].diplos[whom]:
//!                    Leaders[o].ally_diplo(whom, treaty)                 // = set_diplo + local message
//!              else if Leaders[o].is_team(whom, 0) && treaty < Leaders[o].diplos[who]:
//!                    Leaders[o].ally_diplo(who, treaty)
//! 006dae3a  (whom == Console.who: message + SoundGlobal::play(0x72) + chat_to_local)
//!           return 0
//! ```
//!
//! `LeaderData::afford_dow` 0x006D5CE0 / `Leader::pay_dow` 0x006D2B10 price
//! the declaration with `SpellType[0x29c + whom]` (war) or `[0x2a4 + whom]`
//! (peace) through `TypeData::get_cost(good, who, -1, -1, 0, 1, -1)`
//! (vtable +0x78, 0x00664090, 13 KB — the shared cost engine owned by the
//! production lane). `pay_dow`: for each good `g` with `type_avail(g, 1)`:
//! `bucket[g] = max(0, bucket[g] - cost)`, `escrow[g] = max(0, escrow[g] -
//! cost)`. The caller supplies the per-good cost vector; without one the
//! paid paths stop before writing.
//!
//! `Game::war_allowed` 0x00594670: `rush_rules == 0 → 1`; else `0` when
//! `Game::current_age() < rush_rules && (rush_rules < 9 || frame <
//! rush_rules_table[rush].minutes * 900)`; `Game::current_age` 0x005946D0 =
//! max over `flags & 1` leaders of `ages` (clamped at 0 for slot 0). For
//! `rush_rules >= 9` the table (`Categories rush_rules` 0x00E80088, stride
//! 0x58, `minutes` +0x3c) is rules-derived and not in the `Save`: `None`.
//!
//! `LeaderData::is_team(o, strict)` 0x006EBD30: `o == who → 1`; team_style
//! 7 neutral-side players → 0; `frame != 0 && strict == 0 → is_ally(o)`;
//! else both players' `side` bytes equal and in 0..4 → (`frame != 0 &&
//! strict && team_style in {0, 0xb, 8} → is_ally(o)`, else 1); else 0.
//!
//! # Rush-rules expiry body (inline in `Game::do_frame`, 0x0059230E..0x0059241D)
//!
//! After the gate `misc_steps` owns (`rush_rules > 8 && frame ==
//! table[rush].minutes*900`) and the message/sound, when `team_style ∉ {0,
//! 8, 0xb}`: for every `flags & 1` leader `i`: `attrition_stamp = 0`; if
//! `!Leaders[i].is_neutral()`: for every `flags & 1` leader `o` (skipping
//! team_style-7 neutral-side players) with `o != i`, `!is_ally(i, o)`,
//! `!is_enemy(i, o)`: `Leaders[i].set_diplo(o, WAR)` (+ local chat).
//!
//! # `Leader::action_respond(int whom, int accept)` 0x006D03C0 (3,988 B)
//!
//! Entry (both branches): `tribute_demanded[whom] = 0`, `counteroffer[whom]
//! = 0`. `accept == 0`: `Leader::clear_offer(whom)` 0x006D1AF0 + local chat.
//! `accept == 1`: when `dip[whom].agree != 1` the pending `dip[whom].offers[]`
//! tribute is priced (`SpellType[0x29c..]::get_cost`), checked against
//! `bucket[]`, paid (`bucket`, `escrow`, `tributes[g]` +0x498 += amount,
//! `dip[whom].dows[g]`), `dip[whom].agree = 1`, `Leaders[whom].agendas[who]
//! |= 4`; then the treaty/alliance resolution over `dip[whom].attacks[]`
//! with `set_diplo` at +0xc7d/+0xcad/+0xd76/+0xdb8 and `action_declare` at
//! +0x12d/+0xeb3/+0xed7. Only the entry clears are transcribed; the body
//! stops at the cost engine. STATUS below is `Partial` for this reason.
//!
//! # RNG
//!
//! No function in this module (nor `set_diplo`, `action_declare`,
//! `action_respond`, `victory`, `war_allowed`) calls `Random::get` on the
//! main LCG. `SoundGlobal::play` draws on `SoundGlobal::random`. Main-LCG
//! draws: 0.
//!
//! # Oracle
//!
//! None of the 55 captured frames (stride-1 11..40, stride-15 40..325,
//! idle) changes any `diplos[]`, `ally_mask`, `broke_alliance[]`, `dow[]`
//! or `attrition_stamp` byte (`tests::oracle_no_diplo_change`), and none has
//! `rush_rules > 8`, so `set_diplo` is exercised only by the synthetic tests
//! here — it is a transcription, not a fit.

#![allow(dead_code)]

use crate::sections::{Leader, Save};
use crate::tick::{StepStatus, FRAME};

/// `set_diplo`, `action_declare` (free/forced paths), the rush-expiry body
/// and `war_allowed` for `rush_rules < 9` are transcribed. Stops:
/// `eject_my_shit_from_his_ass` (Objects), `Armies::diplo_change`
/// (unwalked AI armies), `Leader::victory`'s Objects/defeat cascade,
/// `afford_dow`/`pay_dow` without a caller-supplied cost vector, and
/// `action_respond`'s offer body.
pub const STATUS: StepStatus = StepStatus::Partial;

pub const WAR: i32 = 0;
pub const PEACE: i32 = 1;
pub const ALLY: i32 = 2;

/// Walked Leader slots visited by every retail loop (`0x00e3a390..0x00e71af0`).
pub const NUM_LEADERS: usize = 8;

// --- LeaderData image offsets (body index = image offset − 8) ---------------
const LD_BASE: usize = 0x8;
const LD_WHO: usize = 0x8;
const LD_DIPLOS: usize = 0x74;
const LD_TREATIES: usize = 0x94;
const LD_AGENDAS: usize = 0xb4;
const LD_ATTRITION_STAMP: usize = 0x1f4;
const LD_BLACKEN: usize = 0x20c;
const LD_DOW: usize = 0x270;
const LD_INVADERS: usize = 0x290;
const LD_BROKE_ALLIANCE: usize = 0x2b0;
const LD_COUNTEROFFER: usize = 0x314;
const LD_TRIBUTE_DEMANDED: usize = 0x334;
const LD_ESCROW: usize = 0x468;
const LD_VICTORY_TYPE: usize = 0x7d8;
const LD_ALLY_MASK: usize = 0x6929;
const LD_MIN_BODY: usize = LD_ALLY_MASK + 1 - LD_BASE;

/// `Diplomacy` row (walked `Leader.diplomacy`, 8 × 0x5c).
const DIP_STRIDE: usize = 0x5c;
const DIP_AGREE: usize = 0x0;

/// `LeaderDataEncrypt` serialized dword indices (see `leaders_process.rs`).
const ENC_BUCKET: usize = 0;
const ENC_PER_GOOD: usize = 9;
const ENC_AGES: usize = 59;

// --- GameInfo settings bytes (`Game.info.settings[i]` = GameInfo+0x18+i) ---
const GI_TEAM_STYLE: usize = 0x0;
const GI_REVEAL_MAP: usize = 0xc;
const GI_RUSH_RULES: usize = 0xe;

// --- Player image (`GameInfo.player[i].body`, Player+0x00..) --------------
const PL_FLAGS: usize = 0x30;
const PL_WHO: usize = 0x33;
const PL_SIDE: usize = 0x34;

// --- Constants ---------------------------------------------------------------
const C_TRUCE_FRAMES: usize = 0xcf8;

/// Type indices: `SpellType` rows priced by `afford_dow`/`pay_dow`.
pub const SPELL_DECLARE_WAR_BASE: i32 = 0x29c;
pub const SPELL_DECLARE_PEACE_BASE: i32 = 0x2a4;
/// Bonus type whose `has_preq` grants shared vision with allies.
pub const BONUS_SHARED_VISION: i32 = 0x2b0;

fn get_i32(buf: &[u8], off: usize) -> i32 {
    i32::from_le_bytes(buf[off..off + 4].try_into().unwrap())
}
fn put_i32(buf: &mut [u8], off: usize, v: i32) {
    buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
}
fn get_u16(buf: &[u8], off: usize) -> u16 {
    u16::from_le_bytes(buf[off..off + 2].try_into().unwrap())
}

fn ld_i32(l: &Leader, img: usize) -> i32 {
    get_i32(&l.body, img - LD_BASE)
}
fn ld_put_i32(l: &mut Leader, img: usize, v: i32) {
    put_i32(&mut l.body, img - LD_BASE, v)
}
fn ld_u8(l: &Leader, img: usize) -> u8 {
    l.body[img - LD_BASE]
}
fn ld_put_u8(l: &mut Leader, img: usize, v: u8) {
    l.body[img - LD_BASE] = v
}

fn frame(save: &Save) -> i32 {
    if save.game.scalars.len() >= FRAME + 4 {
        get_i32(&save.game.scalars, FRAME)
    } else {
        0
    }
}

fn setting(save: &Save, i: usize) -> u8 {
    save.game.info.settings.get(i).copied().unwrap_or(0)
}

fn constant(save: &Save, off: usize) -> i32 {
    let c = &save.rules_tail.rules.constants;
    if c.len() >= off + 4 {
        return get_i32(c, off);
    }
    if save.constants.len() >= off + 4 {
        return get_i32(&save.constants, off);
    }
    0
}

/// A Leader slot with a `flags & 1` record long enough for every field here.
fn active(save: &Save, i: usize) -> bool {
    save.leaders
        .slots
        .get(i)
        .is_some_and(|l| l.flags & 1 != 0 && l.body.len() >= LD_MIN_BODY)
}

fn leader(save: &Save, i: usize) -> &Leader {
    &save.leaders.slots[i]
}
fn leader_mut(save: &mut Save, i: usize) -> &mut Leader {
    &mut save.leaders.slots[i]
}

/// `Console.who` (Console+0x298 = `Save.console1[0..4]`): the local player.
pub fn console_who(save: &Save) -> i32 {
    if save.console1.len() >= 4 {
        get_i32(&save.console1, 0)
    } else {
        -1
    }
}

// --- LeaderData predicates ---------------------------------------------------

/// `diplos[o]` of leader `who`.
pub fn diplo(save: &Save, who: usize, o: usize) -> i32 {
    ld_i32(leader(save, who), LD_DIPLOS + o * 4)
}

/// `LeaderData::is_ally(int)` 0x006EDB50.
pub fn is_ally(save: &Save, who: usize, o: usize) -> bool {
    o == who || (diplo(save, who, o) == ALLY && diplo(save, o, who) == ALLY)
}

/// `LeaderData::is_enemy(int)` 0x006EBAA0.
pub fn is_enemy(save: &Save, who: usize, o: usize) -> bool {
    o != who && (diplo(save, who, o) == WAR || diplo(save, o, who) == WAR)
}

/// `LeaderData::has_treaty(int, uint)` 0x006E11E0.
pub fn has_treaty(save: &Save, who: usize, o: i32, mask: u32) -> bool {
    if o < 0 {
        return false;
    }
    (ld_i32(leader(save, who), LD_TREATIES + o as usize * 4) as u32) & mask != 0
}

/// `LeaderData::get_player()` 0x006EC0F0: first `GameInfo.player[]` slot
/// with `flags & 1 && who == this.who`, preferring one whose
/// `flags & 0x50 == 0`; falls back to the last match (or 0).
pub fn get_player(save: &Save, who: usize) -> usize {
    let mut fallback = 0usize;
    for (i, p) in save.game.info.players.iter().enumerate().take(8) {
        if p.body.len() < PL_SIDE + 1 {
            continue;
        }
        let f = get_u16(&p.body, PL_FLAGS);
        if f & 1 == 0 || p.body[PL_WHO] as usize != who {
            continue;
        }
        fallback = i;
        if f & 0x50 == 0 {
            return i;
        }
    }
    fallback
}

fn player_side(save: &Save, p: usize) -> Option<(u16, u8)> {
    let pl = save.game.info.players.get(p)?;
    (pl.body.len() >= PL_SIDE + 1).then(|| (get_u16(&pl.body, PL_FLAGS), pl.body[PL_SIDE]))
}

/// `LeaderData::is_neutral()` 0x006EBAE0: team_style 7 and this leader's
/// player sits on side 8.
pub fn is_neutral(save: &Save, who: usize) -> bool {
    if setting(save, GI_TEAM_STYLE) != 7 {
        return false;
    }
    match player_side(save, get_player(save, who)) {
        Some((f, side)) => f & 1 != 0 && side == 8,
        None => false,
    }
}

/// `LeaderData::is_team(int, int)` 0x006EBD30.
pub fn is_team(save: &Save, who: usize, o: usize, strict: bool) -> bool {
    if o == who {
        return true;
    }
    let ts = setting(save, GI_TEAM_STYLE);
    if ts == 7 {
        for w in [who, o] {
            if let Some((f, side)) = player_side(save, get_player(save, w)) {
                if f & 1 != 0 && side == 8 {
                    return false;
                }
            }
        }
    }
    let fr = frame(save);
    if fr != 0 && !strict {
        return is_ally(save, who, o);
    }
    let Some((_, my_side)) = player_side(save, get_player(save, who)) else {
        return false;
    };
    let Some((_, o_side)) = player_side(save, get_player(save, o)) else {
        return false;
    };
    if my_side < 4 && my_side == o_side {
        if fr != 0 && strict && matches!(ts, 0 | 0xb | 8) {
            return is_ally(save, who, o);
        }
        return true;
    }
    false
}

/// `Game::current_age()` 0x005946D0: max `ages` over `flags & 1` leaders.
pub fn current_age(save: &Save) -> i32 {
    let mut best = 0i32;
    for (i, l) in save.leaders.slots.iter().enumerate().take(NUM_LEADERS) {
        if l.flags & 1 == 0 || l.data_encrypted.len() < (ENC_AGES + 1) * 4 {
            continue;
        }
        let a = get_i32(&l.data_encrypted, ENC_AGES * 4);
        // Slot 0 is clamped at 0 before the chain of maxes; later slots only
        // replace the running value when strictly larger.
        let a = if i == 0 { a.max(0) } else { a };
        if i == 0 || a > best {
            best = a;
        }
    }
    best
}

/// `Game::war_allowed()` 0x00594670. `None` when the answer needs the
/// `rush_rules` Categories table (`rush_rules >= 9`).
pub fn war_allowed(save: &Save) -> Option<bool> {
    let rush = setting(save, GI_RUSH_RULES);
    if rush == 0 {
        return Some(true);
    }
    if current_age(save) >= rush as i32 {
        return Some(true);
    }
    if rush < 9 {
        return Some(false);
    }
    None
}

/// `LeaderData::has_preq(0x2b0)` for the shared-vision bonus — the slice of
/// `has_preq` 0x006DB810 reachable for that row: `preq[0..2]` from the
/// serialized `Types[0x2b0].head[0x2c..0x34]`, each resolved by `has_tech`
/// (`-1` → true, `-2` → false, tech/gov index → `tech` bit). A bonus-type
/// preq (gov-count ladder) or a missing Rules image yields `None`.
fn has_shared_vision_preq(save: &Save, who: usize) -> Option<bool> {
    let rec = save.rules_tail.rules.types.get(BONUS_SHARED_VISION as usize)?;
    if rec.head.len() < 0x34 {
        return None;
    }
    let l = leader(save, who);
    for i in 0..2 {
        let p = get_i32(&rec.head, 0x2c + i * 4);
        let ok = match p {
            -1 => true,
            -2 => false,
            p if p < 0 => return None,
            p if (0x220..0x275).contains(&p) => tech_bit(l, p),
            _ => return None,
        };
        if !ok {
            return Some(false);
        }
    }
    Some(true)
}

fn tech_bit(l: &Leader, t: i32) -> bool {
    let b = t as usize;
    l.tech.data.get(b >> 3).is_some_and(|x| x & (1 << (b & 7)) != 0)
}

/// Shared-vision gate at 0x006EC892/0x006EC8CE: `has_preq(0x2b0) ||
/// Game.info.reveal_map >= 1`. `reveal_map` is checked first here because
/// it short-circuits the only unresolvable term; retail evaluates
/// `has_preq` first, but it is pure.
fn shared_vision_grant(save: &Save, who: usize) -> Option<bool> {
    if setting(save, GI_REVEAL_MAP) >= 1 {
        return Some(true);
    }
    has_shared_vision_preq(save, who)
}

// --- Leader::set_diplo -------------------------------------------------------

/// `Leader::set_diplo(int whom, int state)` 0x006EC6A0 on `Leaders[who]`.
/// Writes `diplos` (both directions), `ally_mask` (both leaders), and on an
/// allied victory `leader_flags |= 0x20`, `flags2 &= ~0x100`,
/// `victory_type = 0`, `Game.semaphore.ptr` bit 0x16. Everything else is
/// reported in `effects`.
pub fn set_diplo(save: &mut Save, who: usize, whom: usize, state: i32, effects: &mut Vec<String>) {
    if who >= NUM_LEADERS || whom >= NUM_LEADERS {
        effects.push(format!("set_diplo({who},{whom},{state}): slot out of the 0..8 Leader range, untouched"));
        return;
    }
    if !active(save, who) || !active(save, whom) {
        effects.push(format!("set_diplo({who},{whom},{state}): inactive/short Leader record, untouched"));
        return;
    }
    let old = diplo(save, who, whom);
    if old == state {
        return;
    }
    if old == ALLY {
        // 006ec6da / 006ec6ea: both ally_mask bits drop.
        let m = ld_u8(leader(save, who), LD_ALLY_MASK) & !(1u8 << (whom & 7));
        ld_put_u8(leader_mut(save, who), LD_ALLY_MASK, m);
        let m = ld_u8(leader(save, whom), LD_ALLY_MASK) & !(1u8 << (who & 7));
        ld_put_u8(leader_mut(save, whom), LD_ALLY_MASK, m);
        effects.push(format!(
            "set_diplo({who},{whom}): alliance dropped — ally_mask bits cleared; stop: \
             Leader::eject_my_shit_from_his_ass 0x006D0220 ×2 (units garrisoned in the \
             ex-ally's buildings: Objects list, unwalked) untranscribed"
        ));
    }
    // 006ec720..006ec85d: MessageWin + SoundGlobal::play(0x135) for third parties — presentation.
    ld_put_i32(leader_mut(save, who), LD_DIPLOS + whom * 4, state);
    ld_put_i32(leader_mut(save, whom), LD_DIPLOS + who * 4, state);
    effects.push(format!("Leaders[{who}].diplos[{whom}] = Leaders[{whom}].diplos[{who}] = {state} (was {old})"));
    if state == ALLY {
        for (a, b) in [(who, whom), (whom, who)] {
            match shared_vision_grant(save, a) {
                Some(true) => {
                    let m = ld_u8(leader(save, a), LD_ALLY_MASK) | (1u8 << (b & 7));
                    ld_put_u8(leader_mut(save, a), LD_ALLY_MASK, m);
                    effects.push(format!("Leaders[{a}].ally_mask |= 1<<{b} (= {m:#04x})"));
                }
                Some(false) => {}
                None => effects.push(format!(
                    "stop: Leaders[{a}].has_preq(0x2b0) needs the bonus-ladder branch of has_preq \
                     0x006DB810 (unresolved) and reveal_map == 0 — ally_mask bit {b} untouched"
                )),
            }
        }
        // 006ec8f5..006ec954: anyone left who is allied to neither party?
        let mut n = 0;
        for o in 0..NUM_LEADERS {
            if o == who || o == whom || !active(save, o) {
                continue;
            }
            if leader(save, o).flags & 3 != 3 {
                continue;
            }
            if !is_ally(save, o, who) && !is_ally(save, o, whom) {
                n += 1;
            }
        }
        if n == 0 {
            victory(save, who, 0, false, effects);
            if let Some(b) = save.game.sem_ptr.get_mut(0x16 >> 3) {
                *b |= 1 << (0x16 & 7);
                effects.push("Game.semaphore.ptr[2] |= 0x40 (BitMask::set(0x16,1): allied victory)".into());
            }
        }
    }
    effects.push(format!(
        "note: Armies::diplo_change({who}) 0x006F30F0 (AI armies re-target; Armies unwalked) and \
         IFaceData.diplo_dirty = 1 not modelled"
    ));
}

/// `Leader::victory(int type, int flag)` 0x006EC9B0 — the three Leader
/// writes behind the `leader_flags & 0x60 == 0` gate; the Objects refund
/// loop, player score update and the defeat/victory cascade over the other
/// leaders are reported as a stop.
pub fn victory(save: &mut Save, who: usize, vtype: i32, flag: bool, effects: &mut Vec<String>) {
    let l = leader_mut(save, who);
    if l.flags & 0x60 != 0 {
        return;
    }
    l.flags |= 0x20;
    if flag {
        l.flags2 |= 0x100;
    } else {
        l.flags2 &= !0x100;
    }
    ld_put_i32(l, LD_VICTORY_TYPE, vtype);
    effects.push(format!(
        "Leaders[{who}].victory({vtype},{}): leader_flags |= 0x20, flags2 bit 0x100 {}, victory_type = {vtype}; \
         stop: Achieve::capture_event(0xb), Objects refund loop, player score update, and the \
         `flags |= 0x80` / Leader::defeat(5,-1,0) 0x006ECB00 / recursive victory cascade over \
         the other flags&3==3 leaders untranscribed",
        flag as i32,
        if flag { "set" } else { "cleared" }
    ));
}

// --- Leader::action_declare --------------------------------------------------

/// Per-good price of one declaration, as `TypeData::get_cost(g, who, -1,
/// -1, 0, 1, -1)` on `SpellType[0x29c + whom]` (war) / `[0x2a4 + whom]`
/// (peace) would return. Supplied by the caller that owns the cost engine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DowCost {
    pub goods: [i32; 6],
}

/// `LeaderData::afford_dow(int whom, int treaty, int* short_good)`
/// 0x006D5CE0 over a supplied cost vector. Returns `Ok(())` when every
/// available good is covered, `Err(first_short_good)` otherwise
/// (`-1` when more than one good is short, as retail reports).
fn afford_dow(save: &Save, who: usize, whom: usize, treaty: i32, cost: &DowCost) -> Result<(), i32> {
    // 006d5ce0: a declaration that changes nothing is always affordable.
    if whom != who && (diplo(save, who, whom) == WAR || diplo(save, whom, who) == WAR) {
        return Ok(());
    }
    if treaty != 0 && whom != who && (diplo(save, who, whom) != ALLY || diplo(save, whom, who) != ALLY) {
        return Ok(());
    }
    let l = leader(save, who);
    let mut short = -1i32;
    for g in 0..6 {
        if !type_avail_good(save, who, g) {
            continue;
        }
        let have = get_i32(&l.data_encrypted, (g * ENC_PER_GOOD + ENC_BUCKET) * 4);
        if have < cost.goods[g] {
            short = if short == -1 { g as i32 } else { -2 };
        }
    }
    match short {
        -1 => Ok(()),
        s => Err(s.max(-1)),
    }
}

/// `LeaderData::type_avail(good, 1)` 0x006E33A0 for the six goods: the
/// good's row is enabled and its `preq` chain is met. The full predicate
/// lives in `leaders_process.rs`; here goods 0..4 are always available and
/// knowledge (4) / oil (5) follow the Leader's `tech` bits for the age
/// techs their rows name — resolved from `Types[g].head[0x2c]` (`preq[0]`).
fn type_avail_good(save: &Save, who: usize, g: usize) -> bool {
    let Some(rec) = save.rules_tail.rules.types.get(g) else {
        return true;
    };
    if rec.head.len() < 0x30 {
        return true;
    }
    let p = get_i32(&rec.head, 0x2c);
    match p {
        -1 => true,
        -2 => false,
        p if (0x220..0x275).contains(&p) => tech_bit(leader(save, who), p),
        _ => true,
    }
}

/// `Leader::pay_dow(int whom, int treaty)` 0x006D2B10 over a supplied cost
/// vector: `bucket[g] = max(0, bucket[g] - c)`, `escrow[g] = max(0,
/// escrow[g] - c)` for each available good.
fn pay_dow(save: &mut Save, who: usize, cost: &DowCost, effects: &mut Vec<String>) {
    for g in 0..6 {
        if !type_avail_good(save, who, g) {
            continue;
        }
        let c = cost.goods[g];
        let l = leader_mut(save, who);
        let off = (g * ENC_PER_GOOD + ENC_BUCKET) * 4;
        let b = get_i32(&l.data_encrypted, off);
        put_i32(&mut l.data_encrypted, off, (b - c).max(0));
        let e = ld_i32(l, LD_ESCROW + g * 4);
        ld_put_i32(l, LD_ESCROW + g * 4, (e - c).max(0));
        if c != 0 {
            effects.push(format!("Leaders[{who}].bucket[{g}] {b} -> {}, escrow[{g}] {e} -> {}", (b - c).max(0), (e - c).max(0)));
        }
    }
}

/// `Leader::action_declare(int whom, int treaty, int free, int forced)`
/// 0x006DAB50 on `Leaders[who]`. `cost` is consulted only when `free` is
/// false; when it is needed and absent the function stops before any write
/// and returns `None`. Otherwise returns retail's result: `1` refused, `0`
/// applied.
pub fn action_declare(
    save: &mut Save,
    who: usize,
    whom: usize,
    treaty: i32,
    free: bool,
    forced: bool,
    cost: Option<&DowCost>,
    effects: &mut Vec<String>,
) -> Option<i32> {
    if who >= NUM_LEADERS || whom >= NUM_LEADERS || !active(save, who) || !active(save, whom) {
        effects.push(format!("action_declare({who},{whom},{treaty}): inactive/out-of-range Leader, untouched"));
        return Some(0);
    }
    let mut treaty = treaty;
    // 006dab63
    if is_enemy(save, who, whom) {
        return Some(0);
    }
    loop {
        // 006dab73
        if treaty == 0 && !forced {
            if is_ally(save, who, whom) {
                // 006dab88: breaking an alliance is a declaration of peace first.
                treaty = 1;
                effects.push(format!("action_declare({who},{whom}): war on an ally becomes peace (treaty = 1)"));
                if is_enemy(save, who, whom) {
                    return Some(0);
                }
                continue;
            }
            // 006dab8d: truce cooldown after a broken alliance.
            let mine = ld_i32(leader(save, who), LD_BROKE_ALLIANCE + whom * 4);
            let theirs = ld_i32(leader(save, whom), LD_BROKE_ALLIANCE + who * 4);
            let stamp = mine.max(theirs);
            if stamp != 0 && frame(save) - stamp < constant(save, C_TRUCE_FRAMES) {
                effects.push(format!(
                    "action_declare({who},{whom}): refused — truce since frame {stamp} (< Constants[0xcf8] = {})",
                    constant(save, C_TRUCE_FRAMES)
                ));
                return Some(1);
            }
        }
        // 006dabc7
        if !free {
            let Some(c) = cost else {
                effects.push(format!(
                    "stop: action_declare({who},{whom},{treaty}) needs SpellType[{:#x}]::get_cost \
                     (TypeData::get_cost 0x00664090) for afford_dow/pay_dow — no cost supplied, untouched",
                    if treaty == 0 { SPELL_DECLARE_WAR_BASE } else { SPELL_DECLARE_PEACE_BASE } + whom as i32
                ));
                return None;
            };
            if let Err(g) = afford_dow(save, who, whom, treaty, c) {
                effects.push(format!("action_declare({who},{whom}): refused — cannot afford (short good {g}); LeaderOut::warn_resources"));
                return Some(1);
            }
        }
        // 006dabe2
        if treaty == 0 {
            match war_allowed(save) {
                Some(true) => {}
                Some(false) => {
                    if !forced {
                        effects.push(format!("action_declare({who},{whom}): refused — Game::war_allowed() == 0 (rush rules)"));
                        return Some(1);
                    }
                    treaty = 1;
                    if is_enemy(save, who, whom) {
                        return Some(0);
                    }
                    continue;
                }
                None => {
                    effects.push(format!(
                        "stop: Game::war_allowed 0x00594670 needs rush_rules[{}].minutes (Categories table \
                         0x00E80088, not in Save), untouched",
                        setting(save, GI_RUSH_RULES)
                    ));
                    return None;
                }
            }
        }
        break;
    }
    // 006dacec
    if !free {
        let c = *cost.expect("checked above");
        pay_dow(save, who, &c, effects);
        let ally = is_ally(save, who, whom);
        if ally && treaty == 0 {
            pay_dow(save, who, &c, effects);
        }
        if treaty == 0 && (ally || ld_i32(leader(save, who), LD_INVADERS + whom * 4) < 5) {
            bump_dow(save, who, whom, effects);
        }
        if ally {
            bump_dow(save, who, whom, effects);
        }
    }
    // 006dad6b
    if is_ally(save, who, whom) {
        let f = frame(save);
        ld_put_i32(leader_mut(save, who), LD_BROKE_ALLIANCE + whom * 4, f);
        effects.push(format!("Leaders[{who}].broke_alliance[{whom}] = {f}"));
    }
    set_diplo(save, who, whom, treaty, effects);
    // 006dadb5: team-mates follow.
    for o in 0..NUM_LEADERS {
        if o == who || o == whom || !active(save, o) {
            continue;
        }
        if is_team(save, o, who, false) {
            if !is_team(save, o, whom, false) && treaty < diplo(save, o, whom) {
                set_diplo(save, o, whom, treaty, effects); // Leader::ally_diplo 0x006D0120
            }
        } else if is_team(save, o, whom, false) && treaty < diplo(save, o, who) {
            set_diplo(save, o, who, treaty, effects);
        }
    }
    Some(0)
}

fn bump_dow(save: &mut Save, who: usize, whom: usize, effects: &mut Vec<String>) {
    let l = leader_mut(save, who);
    let d = ld_i32(l, LD_DOW + whom * 4);
    if d != 0 {
        let b = ld_i32(l, LD_BLACKEN) + 1;
        ld_put_i32(l, LD_BLACKEN, b);
        effects.push(format!("Leaders[{who}].blacken = {b}"));
    }
    ld_put_i32(l, LD_DOW + whom * 4, d + 1);
    effects.push(format!("Leaders[{who}].dow[{whom}] = {}", d + 1));
}

// --- Command bodies ----------------------------------------------------------

/// `CommandPackage::process_declare(DeclareCommand*)` 0x009472B0. `cmd` is
/// the wire record starting at the opcode byte (38). Returns the consumed
/// length (13) when the record is well-formed.
pub fn process_declare(save: &mut Save, cmd: &[u8], cost: Option<&DowCost>, effects: &mut Vec<String>) -> Option<usize> {
    if cmd.len() < 13 {
        return None;
    }
    let who = get_i32(cmd, 1);
    let whom = get_i32(cmd, 5);
    let treaty = get_i32(cmd, 9);
    effects.push(format!("process_declare who: {who} whom: {whom} treaty: {treaty} game.frame: {}", frame(save)));
    if who < 0 || whom < 0 {
        return Some(13);
    }
    action_declare(save, who as usize, whom as usize, treaty, false, false, cost, effects);
    Some(13)
}

/// `CommandPackage::process_accept(AcceptCommand*)` 0x00946FB0 → returns 9.
pub fn process_accept(save: &mut Save, cmd: &[u8], effects: &mut Vec<String>) -> Option<usize> {
    if cmd.len() < 9 {
        return None;
    }
    let who = get_i32(cmd, 1);
    let whom = get_i32(cmd, 5);
    effects.push(format!("process_accept who: {who} whom: {whom} game.frame: {}", frame(save)));
    if who >= 0 && whom >= 0 {
        action_respond(save, who as usize, whom as usize, 1, effects);
    }
    Some(9)
}

/// `Leader::action_respond(int whom, int accept)` 0x006D03C0 — entry writes
/// only (see module docs); the offer body stops at the cost engine.
pub fn action_respond(save: &mut Save, who: usize, whom: usize, accept: i32, effects: &mut Vec<String>) {
    if who >= NUM_LEADERS || whom >= NUM_LEADERS || !active(save, who) {
        effects.push(format!("action_respond({who},{whom},{accept}): inactive/out-of-range Leader, untouched"));
        return;
    }
    {
        let l = leader_mut(save, who);
        ld_put_i32(l, LD_TRIBUTE_DEMANDED + whom * 4, 0);
        ld_put_i32(l, LD_COUNTEROFFER + whom * 4, 0);
    }
    effects.push(format!("Leaders[{who}].tribute_demanded[{whom}] = 0, counteroffer[{whom}] = 0"));
    let agree = {
        let l = leader(save, who);
        let off = whom * DIP_STRIDE + DIP_AGREE;
        if l.diplomacy.len() >= off + 4 {
            get_i32(&l.diplomacy, off)
        } else {
            0
        }
    };
    match accept {
        0 => effects.push(format!(
            "stop: action_respond({who},{whom},0) → Leader::clear_offer 0x006D1AF0 (dip[{whom}] reset) untranscribed"
        )),
        1 => effects.push(format!(
            "stop: action_respond({who},{whom},1) offer body (dip[{whom}].agree = {agree}; tribute pricing via \
             SpellType::get_cost 0x00664090, bucket/escrow/tributes writes, agendas |= 4, treaty resolution \
             with set_diplo/action_declare) untranscribed"
        )),
        _ => effects.push(format!("stop: action_respond({who},{whom},{accept}) counter-offer branch untranscribed")),
    }
}

// --- Rush-rules expiry body --------------------------------------------------

/// Body of the `Game::do_frame` rush-rules block once its gate has fired
/// (0x005922EE..0x0059241D), after the message/sound: zero every active
/// leader's `attrition_stamp` and put every non-neutral, non-allied,
/// non-warring pair at war through `set_diplo`. The owning step
/// (`misc_steps` 10) decides *when*; this is *what*.
pub fn rush_expiry_body(save: &mut Save, effects: &mut Vec<String>) {
    let ts = setting(save, GI_TEAM_STYLE);
    if matches!(ts, 0 | 8 | 0xb) {
        effects.push(format!("rush expiry: team_style {ts} ∈ {{0,8,0xb}} — no diplomacy change"));
        return;
    }
    for i in 0..NUM_LEADERS {
        if !active(save, i) {
            continue;
        }
        ld_put_i32(leader_mut(save, i), LD_ATTRITION_STAMP, 0);
        effects.push(format!("Leaders[{i}].attrition_stamp = 0"));
        if is_neutral(save, i) {
            continue;
        }
        for o in 0..NUM_LEADERS {
            if !active(save, o) {
                continue;
            }
            if ts == 7 {
                if let Some((f, side)) = player_side(save, get_player(save, o)) {
                    if f & 1 != 0 && side == 8 {
                        continue;
                    }
                }
            }
            if i == o || is_ally(save, i, o) || is_enemy(save, i, o) {
                continue;
            }
            set_diplo(save, i, o, WAR, effects);
            // 005923ad..005923ed: local-player chat only.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sections::{Leader, PlayerInfo, TypeRec};

    fn leader(who: i32) -> Leader {
        let mut l = Leader::default();
        l.flags = 3;
        l.body = vec![0u8; 0x6922];
        ld_put_i32(&mut l, LD_WHO, who);
        for o in 0..8 {
            ld_put_i32(&mut l, LD_DIPLOS + o * 4, PEACE);
        }
        l.data_encrypted = vec![0u8; 62 * 4];
        l.tech.data = vec![0u8; 101];
        l.diplomacy = vec![0u8; 8 * 0x5c];
        l
    }

    fn save(n: usize) -> Save {
        let mut s = Save::default();
        s.game.scalars = vec![0u8; 404];
        put_i32(&mut s.game.scalars, FRAME, 1000);
        s.game.info.settings = vec![0u8; 0x1e];
        s.game.sem_ptr = vec![0u8; 32];
        s.console1 = vec![0xff; 0x20];
        s.leaders.slots = (0..9).map(|i| if i < n { leader(i as i32) } else { Leader::default() }).collect();
        // Rules image: 0x2b0 (shared vision bonus) with preq[0] = -1, preq[1] = -1.
        s.rules_tail.rules.types = (0..806)
            .map(|_| {
                let mut t = TypeRec::default();
                t.head = vec![0u8; 90];
                put_i32(&mut t.head, 0x2c, -1);
                put_i32(&mut t.head, 0x30, -1);
                t
            })
            .collect();
        s.rules_tail.rules.constants = vec![0u8; 0xd40];
        put_i32(&mut s.rules_tail.rules.constants, C_TRUCE_FRAMES, 900);
        for i in 0..n {
            let mut p = PlayerInfo::default();
            p.body = vec![0u8; 0x39];
            p.body[PL_FLAGS] = 1;
            p.body[PL_WHO] = i as u8;
            p.body[PL_SIDE] = (i % 2) as u8;
            s.game.info.players.push(p);
        }
        s
    }

    #[test]
    fn set_diplo_is_symmetric_and_idempotent() {
        let mut s = save(3);
        let mut e = Vec::new();
        set_diplo(&mut s, 0, 1, WAR, &mut e);
        assert_eq!(diplo(&s, 0, 1), WAR);
        assert_eq!(diplo(&s, 1, 0), WAR);
        assert!(is_enemy(&s, 0, 1) && is_enemy(&s, 1, 0));
        assert!(!is_enemy(&s, 0, 2));
        let before = e.len();
        set_diplo(&mut s, 1, 0, WAR, &mut e);
        assert_eq!(e.len(), before, "no-op when already in that state");
    }

    #[test]
    fn alliance_sets_ally_mask_and_breaking_it_clears() {
        let mut s = save(4);
        let mut e = Vec::new();
        set_diplo(&mut s, 0, 1, ALLY, &mut e);
        assert!(is_ally(&s, 0, 1));
        assert_eq!(ld_u8(&s.leaders.slots[0], LD_ALLY_MASK), 0b10);
        assert_eq!(ld_u8(&s.leaders.slots[1], LD_ALLY_MASK), 0b01);
        // Two outsiders (2, 3) remain -> no victory.
        assert_eq!(s.leaders.slots[0].flags & 0x20, 0);
        assert_eq!(s.game.sem_ptr[2] & 0x40, 0);
        set_diplo(&mut s, 1, 0, PEACE, &mut e);
        assert_eq!(ld_u8(&s.leaders.slots[0], LD_ALLY_MASK), 0);
        assert_eq!(ld_u8(&s.leaders.slots[1], LD_ALLY_MASK), 0);
        assert!(e.iter().any(|x| x.contains("eject_my_shit_from_his_ass")));
    }

    #[test]
    fn ally_mask_needs_shared_vision_preq_or_reveal_map() {
        let mut s = save(3);
        put_i32(&mut s.rules_tail.rules.types[0x2b0].head, 0x2c, 0x230); // a tech preq
        let mut e = Vec::new();
        set_diplo(&mut s, 0, 1, ALLY, &mut e);
        assert_eq!(ld_u8(&s.leaders.slots[0], LD_ALLY_MASK), 0, "preq tech missing");
        set_diplo(&mut s, 0, 1, PEACE, &mut e);
        s.game.info.settings[GI_REVEAL_MAP] = 1;
        set_diplo(&mut s, 0, 1, ALLY, &mut e);
        assert_eq!(ld_u8(&s.leaders.slots[0], LD_ALLY_MASK), 0b10, "reveal_map grants it");
    }

    #[test]
    fn last_alliance_triggers_victory() {
        let mut s = save(3);
        let mut e = Vec::new();
        set_diplo(&mut s, 0, 1, ALLY, &mut e);
        assert_eq!(s.leaders.slots[0].flags & 0x20, 0);
        set_diplo(&mut s, 2, 0, ALLY, &mut e);
        // Leader 1 is allied to 0 -> nobody is allied to neither -> victory for 2.
        assert_ne!(s.leaders.slots[2].flags & 0x20, 0);
        assert_eq!(ld_i32(&s.leaders.slots[2], LD_VICTORY_TYPE), 0);
        assert_ne!(s.game.sem_ptr[2] & 0x40, 0);
    }

    #[test]
    fn war_allowed_follows_rush_rules_and_age() {
        let mut s = save(2);
        assert_eq!(war_allowed(&s), Some(true));
        s.game.info.settings[GI_RUSH_RULES] = 3;
        assert_eq!(war_allowed(&s), Some(false));
        put_i32(&mut s.leaders.slots[1].data_encrypted, ENC_AGES * 4, 3);
        assert_eq!(current_age(&s), 3);
        assert_eq!(war_allowed(&s), Some(true));
        s.game.info.settings[GI_RUSH_RULES] = 9;
        assert_eq!(war_allowed(&s), None);
    }

    #[test]
    fn declare_free_war_and_truce_cooldown() {
        let mut s = save(3);
        let mut e = Vec::new();
        // Alliance first, then "war" on the ally becomes peace and stamps broke_alliance.
        set_diplo(&mut s, 0, 1, ALLY, &mut e);
        let r = action_declare(&mut s, 0, 1, WAR, true, false, None, &mut e);
        assert_eq!(r, Some(0));
        assert_eq!(diplo(&s, 0, 1), PEACE);
        assert_eq!(ld_i32(&s.leaders.slots[0], LD_BROKE_ALLIANCE + 4), 1000);
        // Within Constants[0xcf8] frames war is refused.
        let r = action_declare(&mut s, 0, 1, WAR, true, false, None, &mut e);
        assert_eq!(r, Some(1));
        assert_eq!(diplo(&s, 0, 1), PEACE);
        put_i32(&mut s.game.scalars, FRAME, 1000 + 900);
        let r = action_declare(&mut s, 1, 0, WAR, true, false, None, &mut e);
        assert_eq!(r, Some(0));
        assert_eq!(diplo(&s, 0, 1), WAR);
        // dow/blacken only on the paid path.
        assert_eq!(ld_i32(&s.leaders.slots[1], LD_DOW), 0);
    }

    #[test]
    fn declare_paid_path_bills_and_counts() {
        let mut s = save(3);
        let mut e = Vec::new();
        for g in 0..6 {
            put_i32(&mut s.leaders.slots[0].data_encrypted, g * ENC_PER_GOOD * 4, 500);
            ld_put_i32(&mut s.leaders.slots[0], LD_ESCROW + g * 4, 50);
        }
        let cost = DowCost { goods: [100, 0, 0, 0, 0, 0] };
        let r = action_declare(&mut s, 0, 1, WAR, false, false, Some(&cost), &mut e);
        assert_eq!(r, Some(0));
        assert_eq!(get_i32(&s.leaders.slots[0].data_encrypted, 0), 400);
        assert_eq!(ld_i32(&s.leaders.slots[0], LD_ESCROW), 0);
        assert_eq!(ld_i32(&s.leaders.slots[0], LD_DOW + 4), 1);
        assert_eq!(ld_i32(&s.leaders.slots[0], LD_BLACKEN), 0);
        // At war, action_declare is a no-op (006dab63) — peace comes through
        // the offer path (action_respond), so step the matrix directly.
        assert_eq!(action_declare(&mut s, 0, 1, PEACE, false, false, Some(&cost), &mut e), Some(0));
        assert_eq!(diplo(&s, 0, 1), WAR);
        set_diplo(&mut s, 0, 1, PEACE, &mut e);
        // War again: second dow increments blacken.
        let r = action_declare(&mut s, 0, 1, WAR, false, false, Some(&cost), &mut e);
        assert_eq!(r, Some(0));
        assert_eq!(ld_i32(&s.leaders.slots[0], LD_DOW + 4), 2);
        assert_eq!(ld_i32(&s.leaders.slots[0], LD_BLACKEN), 1);
        // Can't afford -> refused, nothing written.
        let big = DowCost { goods: [10_000, 0, 0, 0, 0, 0] };
        set_diplo(&mut s, 0, 1, PEACE, &mut e);
        let r = action_declare(&mut s, 0, 1, WAR, false, false, Some(&big), &mut e);
        assert_eq!(r, Some(1));
        assert_eq!(diplo(&s, 0, 1), PEACE);
        // No cost vector on a paid path stops before writing.
        let r = action_declare(&mut s, 0, 2, WAR, false, false, None, &mut e);
        assert_eq!(r, None);
        assert_eq!(diplo(&s, 0, 2), PEACE);
    }

    #[test]
    fn declare_drags_team_mates_along() {
        let mut s = save(4);
        put_i32(&mut s.game.scalars, FRAME, 0); // frame 0: is_team by side bytes
        // sides: 0,1,0,1 -> {0,2} vs {1,3}
        let mut e = Vec::new();
        let r = action_declare(&mut s, 0, 1, WAR, true, false, None, &mut e);
        assert_eq!(r, Some(0));
        assert_eq!(diplo(&s, 0, 1), WAR);
        assert_eq!(diplo(&s, 2, 1), WAR, "0's team-mate 2 joins against 1");
        assert_eq!(diplo(&s, 3, 0), WAR, "1's team-mate 3 joins against 0");
        assert_eq!(diplo(&s, 2, 3), PEACE, "unrelated pair untouched");
    }

    #[test]
    fn process_declare_and_accept_wire() {
        let mut s = save(2);
        let mut e = Vec::new();
        let mut cmd = vec![38u8];
        cmd.extend_from_slice(&0i32.to_le_bytes());
        cmd.extend_from_slice(&1i32.to_le_bytes());
        cmd.extend_from_slice(&WAR.to_le_bytes());
        // Paid path without a cost vector: parsed, stopped, untouched.
        assert_eq!(process_declare(&mut s, &cmd, None, &mut e), Some(13));
        assert_eq!(diplo(&s, 0, 1), PEACE);
        assert_eq!(process_declare(&mut s, &cmd, Some(&DowCost::default()), &mut e), Some(13));
        assert_eq!(diplo(&s, 0, 1), WAR);
        let mut acc = vec![41u8];
        acc.extend_from_slice(&0i32.to_le_bytes());
        acc.extend_from_slice(&1i32.to_le_bytes());
        ld_put_i32(&mut s.leaders.slots[0], LD_TRIBUTE_DEMANDED + 4, 77);
        assert_eq!(process_accept(&mut s, &acc, &mut e), Some(9));
        assert_eq!(ld_i32(&s.leaders.slots[0], LD_TRIBUTE_DEMANDED + 4), 0);
    }

    #[test]
    fn rush_expiry_puts_everyone_at_war() {
        let mut s = save(4);
        s.game.info.settings[GI_TEAM_STYLE] = 1;
        for i in 0..4 {
            ld_put_i32(&mut s.leaders.slots[i], LD_ATTRITION_STAMP, 123);
        }
        let mut e = Vec::new();
        set_diplo(&mut s, 0, 1, ALLY, &mut e);
        rush_expiry_body(&mut s, &mut e);
        for i in 0..4 {
            assert_eq!(ld_i32(&s.leaders.slots[i], LD_ATTRITION_STAMP), 0);
        }
        assert!(is_ally(&s, 0, 1), "allies stay allied");
        assert!(is_enemy(&s, 0, 2) && is_enemy(&s, 1, 3) && is_enemy(&s, 2, 3));
        // team_style 0: nothing happens.
        let mut s2 = save(3);
        rush_expiry_body(&mut s2, &mut e);
        assert!(!is_enemy(&s2, 0, 1));
    }

    #[test]
    fn oracle_no_diplo_change() {
        // Every captured stride pair: diplos/ally_mask/broke_alliance/dow/attrition_stamp
        // bytes are identical frame to frame, so no capture exercises set_diplo.
        let Ok(root) = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize() else {
            return;
        };
        let pairs = root.join("schema/live/frame-pairs");
        let Ok(rd) = std::fs::read_dir(&pairs) else {
            return;
        };
        let mut dirs: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.join("manifest.json").is_file()).collect();
        dirs.sort();
        for d in dirs {
            let mut svx: Vec<_> = std::fs::read_dir(&d)
                .unwrap()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "svx"))
                .collect();
            svx.sort_by_key(|p| {
                p.file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.trim_start_matches("donf").parse::<i64>().ok())
                    .unwrap_or(0)
            });
            let mut prev: Option<Vec<Vec<u8>>> = None;
            for p in svx {
                let raw = crate::container::load_svx(&p).unwrap();
                let (s, _) = crate::sections::load_save(&raw).unwrap();
                let cur: Vec<Vec<u8>> = s
                    .leaders
                    .slots
                    .iter()
                    .take(NUM_LEADERS)
                    .map(|l| {
                        if l.body.len() < LD_MIN_BODY {
                            return Vec::new();
                        }
                        let mut v = Vec::new();
                        v.extend_from_slice(&l.body[LD_DIPLOS - LD_BASE..LD_DIPLOS - LD_BASE + 32]);
                        v.extend_from_slice(&l.body[LD_ATTRITION_STAMP - LD_BASE..LD_ATTRITION_STAMP - LD_BASE + 4]);
                        v.extend_from_slice(&l.body[LD_DOW - LD_BASE..LD_DOW - LD_BASE + 32]);
                        v.extend_from_slice(&l.body[LD_BROKE_ALLIANCE - LD_BASE..LD_BROKE_ALLIANCE - LD_BASE + 32]);
                        v.push(l.body[LD_ALLY_MASK - LD_BASE]);
                        v
                    })
                    .collect();
                assert!(setting(&s, GI_RUSH_RULES) <= 8, "{}: rush_rules > 8", p.display());
                if let Some(pv) = &prev {
                    assert_eq!(pv, &cur, "{}: diplomacy bytes changed", p.display());
                }
                prev = Some(cur);
            }
        }
    }
}
