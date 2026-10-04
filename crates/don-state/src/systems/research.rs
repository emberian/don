//! Tech completion effects: `Leader::gain_tech` 0x006DCB60 (15,001 B — the
//! Library's "research finished" body), its bookkeeping children, and the
//! rules arithmetic the Library's research cost scales with. Leaf module:
//! exposes free functions for the owning traversals to call; never edits
//! sibling modules. The queue pop that *reaches* `gain_tech`
//! (`Build::finished` 0x00628490 +0xb8: `Leaders[owner].gain_tech(type,
//! build.x ^ 0x63637, build.y ^ 0x63637, 1, 1)`) belongs to the
//! `production_train` lane; this module owns what happens to the Leader.
//!
//! Other retail callers of `gain_tech`: `Build::queue_up` 0x00620F40
//! (+0x37f, zero-time techs), `Leader::init` 0x006E3930 (starting techs),
//! `Leader::set_age` 0x006D25A0 / `set_epoch` 0x006D26F0 (cheat / scenario
//! ladders, via the thunk 0x00471110 with `(t, 0, 0, 0, 1)`),
//! `ScenarioFuncSet::gain_tech`/`gain_upgrade`,
//! `ConquestGame::setup_scenario_bonuses`, and `gain_tech` itself (36
//! recursive sites: wonder-granted techs, upgrade chains).
//!
//! # Leader image (`re/scripts/pdb_layout.py LeaderData LeaderDataEncrypt TechType`)
//!
//! `Save.leaders.slots[i].body` = image `+0x08..+0x692a` (`body[off - 8]`).
//! Fields: `who` +0x8, `age_stamp[7]` +0x8f8 (frame each age type was
//! gained), `gov_hero_frame` +0xa48, `tech_frame` +0x7c4 / `tech_cat_frame[4]`
//! +0x7c8 (AI pacing; not written here), `ally_mask` +0x6929, `tech`
//! BitMask<806> +0x6c0c (walked as `Leader.tech`), `data_encrypted` →
//! `LeaderDataEncrypt` (`bucket[6]` +0, `ages` +0xdc, `epochs` +0xe0,
//! `discovered` +0xe4, `epoch[4]` +0xe8; serialized plain as dwords 0..5
//! (bucket, stride 9), 59, 60, 61, 55..58 — see `leaders_process.rs`).
//!
//! `TechType` (`Types[t]`, `t` in 0x220..0x275): `cat` +0x14 (`head[0x10]`;
//! 0 military / 1 civic / 2 commerce / 3 science / 4+ for ages & specials),
//! `preq[3]` +0x30 (`head[0x2c..]`), `obs` +0x4c (`head[0x48]`), `age`
//! +0x1c8 (`ext[0..4]`). Index classes (`TypeData` virtuals): age types
//! 0x220..0x227 (`vt+0x34`), epoch types 0x227..0x243 (`vt+0x38`), gov types
//! 0x26f..0x275 (`vt+0x2c`), unit types 0x32..0x19e (`vt+0xc`), building
//! types 0x19e..0x21f, goods 0..0x32.
//!
//! # `Leader::gain_tech(TypeIndex t, Coord x, Coord y, int from_build, int announce)` — head
//!
//! Instruction-ordered (Capstone), walked-state writes in **bold**:
//!
//! ```text
//! 006dcb99  **leader_flags |= 0x1000000**; IFaceData+0x90 = 1 (UI: tech tree dirty)
//! 006dcbba  if is_gov_type(t) && frame == 0: **gov_hero_frame = 1**
//! 006dcbe1  if is_epoch_type(t) && TechType[t].cat == 3 && frame != 0:
//!             lib = get_first_library(); for each queued Build in Objects[lib].queue:
//!               if get_queue(i) != t && is_tech_type(get_queue(i)): Build::refund_cost   // STOP: Objects
//! 006dccc1  if is_age_type(t) && rush_rules != 0 && t - 0x21f == rush_rules:
//!             no_war = !Game::war_allowed()                              // local; message text only
//! 006dcd07  upgrade = TechType[t].vt+0x60(0x134, 0) ? current_upgrade(0x134)
//!           : (vt+0x60(get_graft(0x127), 1) ? { gain_tech(0x135, x, y, 0, 1); t } : -1)   // STOP: graft tables
//! 006dcd83  if !has_tech(t):
//!             is_age_type:   **ages += 1**; (local: IFaceMainBase::age_update); **age_stamp[t-0x220] = frame**
//!             is_epoch_type: **epochs += 1**; **epoch[TechType[t].cat] += 1**
//!             else:          **discovered += 1**
//! 006dcdf5  if Game.semaphore.ptr[2] & 2 && is_epoch_type(t) && cat == 1 && frame != 0
//!             && epoch[1] == Constants[0xad4]:                           // conquest: wonder age reached
//!             (local + conquest_wonders empty: do_notice)
//!             if has_wonder(0x21e): **leader_flags |= 0x800**
//!             if has_wonder(0x219) && Constants[0x534] != 0:
//!               for u in 0x32..0x192: has_preq(u) && (UnitType[u].domain == 0 || where == 0x1bf)
//!                 && !has_tech(u) && type_eligible(u, 0): gain_tech(u, 0, 0, 0, 1)   // STOP: unit has_preq
//! 006dcf85  avail_before[g] = has_preq(g) for g in 0..6                 // good types
//! 006dcfab  **tech.set(t, 1)**                                            // BitMask<806>::set 0x00450360
//! 006dcfb7  if has_tech(0x243..0x246): (local: achievement 0x19)
//! 006dd027  **leader_flags |= 0x2000000**                                 // calc_gather re-runs
//! 006dd02d  if !(Game.semaphore.ptr[1] & 8):                             // not observing-only
//! 006dd04c    for g in 0..6 with !avail_before[g] && GoodType[g].preq[0] == t:
//!               amount = Game.starting[g]                               // Game+0x600+4g
//!                 | (score_goal+1) * starting[g]   if game_rules == 8 && get_team() == 0
//!                 | starting[g] * Constants[0xa60] if semaphore.ptr[2] & 2 && flags2 & 0x80
//!               **bucket_add(g, amount)**                                // LeaderData::bucket_add 0x0043ED10
//!               if starting_resources == 8: **bucket[g] = 0x1269f**      // (raw 0x104be ^ 0x8221)
//! 006dd19e    for g in 0..6, g ∉ {2,3}, GoodType[g].obs == t:
//!               while bucket[g] > 100: Leader::action_sell(g, 0)       // STOP: market (Game.market*)
//! 006dd1e4    if t == 0x220 && has_tribe_bonus(5) && Constants[0x5f8] != 0:
//!               **bucket_add(3, starting[3] scaled as above)**
//! 006dd268  (local && !is_gov_type && announce: message + sound; t == 5: Camera::outdate)
//! 006dd38d  if is_epoch_type(t) && cat == 1 && epoch[1] == Constants[0xad4]:
//!             calc_pop_cap; calc_anti_attrition; calc_attrition; Region::fix_borders   // STOP
//! 006dd3e3  special_techs[0xac0].type == t: for allies o: **ally_mask |= 1<<o**     // STOP: Categories table
//!           [0xac8] == t: **leader_flags |= 0x1000**; get_preq([0xad0]) == t: |= 0x800;
//!           get_preq([0xacc]) == t: |= 0x2000; get_preq([0xae4]) == t: ...          // STOP: Categories table
//! 006dd5b0 … 006e05c5  per-type effects: City::assimilate / Leader::defeat (capital
//!           captures on age), calc_pop_cap, check_transport, Build::train, track_queued,
//!           BuildQueue::set_queue, Leader::victory (tech-race / wonder victory via
//!           BitMask<806>::get on `tech`), Regions::fix_all_borders, Wall::mask_city,
//!           City::check_upgrade, 30+ wonder/age-granted `gain_tech` recursions,
//!           Game::teams_locked + set_diplo (diplomacy unlock), Unit::update_gpiece,
//!           Wall::update_gpiece, TerrainOil::gain_tech                          // STOP: Objects/Cities/Walls
//! ```
//!
//! `LeaderData::bucket_add(good, amount)` 0x0043ED10: `bucket[good] +=
//! amount` (XOR 0x8221 in memory; plain here).
//!
//! `LeaderData::has_tech` 0x006E0C80, `has_preq` 0x006DB810, `type_eligible`
//! 0x006DBD10 and `has_tribe_bonus` 0x006E1370 are transcribed in
//! `leaders_process.rs`; here only the `has_preq(good)` slice needed for
//! the good-unlock loop is evaluated (good `preq[0..2]` through `has_tech`
//! on tech indices; anything else → that good is left untouched and
//! reported).
//!
//! # `LeaderData::techs_per_age(int t)` 0x006D7280 — research cost ladder
//!
//! The Library prices tech `t` by how many techs of its age are "due":
//! `st = starting_technology` (or, in `game_rules == 8` with `get_team() ==
//! 0`, `min(ending, clamp7(starting) + clamp7(starting2))` —
//! `LeaderData::starting_age` 0x006D7320), `end = ending_technology`.
//! `st == 0 && end > 6` → `4*age + 2`. `n = end - st + 1`; `n == 0` → 0;
//! `r = (age - st + 1) * (28 / n) - 2`; `st == 0 && age == 0 && r == 1` → 2;
//! else `r`.
//!
//! # `Leader::set_age(int)` 0x006D25A0 / `set_epoch(int, int)` 0x006D26F0
//!
//! Cheat/scenario ladders: clamp to 0..7 (ages) / 0..3 × 0..7 (epochs);
//! `lose_tech` every age/epoch type above the target, `gain_tech(t, 0, 0,
//! 0, 1)` every one below; then drop every tech/unit/building whose
//! `has_preq` no longer holds; `Leader::fix_unit_flags`-like 0x006E32A0,
//! `calc_unit_stats`, `calc_wall_stats`, `Camera::outdate`. Not transcribed
//! (no capture, cheat path).
//!
//! `Leader::lose_tech` 0x006D2850: `tech.set(t, 0)`, recursive loss of
//! techs for which `t` is an `is_ultimate_preq` 0x0066C410, `fix_tech_flags`
//! 0x006D2480, counters (`discovered -= 1` / `epochs -= 1, epoch[cat] =
//! count` / age recount). Not transcribed.
//!
//! `Leader::fix_tech_flags` 0x006D2480: `Regions::fix_all_borders`;
//! `ally_mask = 1 << who`, plus every ally's bit when the special-tech
//! `[0xac0]` is held; `leader_flags` bits 0x1000/0x800/0x2000 from the
//! `[0xac8]/[0xad0]/[0xacc]` specials (0x800 also from wonder 0x21e);
//! `Leader::check_transport`. Needs the `Categories` special-tech table
//! (not in `Save`): the `ally_mask = 1 << who` seed is exposed, the rest
//! stops.
//!
//! # RNG
//!
//! No instruction in the transcribed head reaches `Random::get` on the main
//! LCG; `SoundGlobal::play` draws on `SoundGlobal::random`. The untranscribed
//! tail calls `Build::train` / `City::assimilate`, which do draw — any
//! future transcription of those sections must account for them. Main-LCG
//! draws from this module: 0.
//!
//! # Oracle
//!
//! No captured frame pair changes any Leader `tech` bit, `ages`, `epochs`,
//! `discovered` or `epoch[]` dword (`tests::oracle_no_tech_change`), so the
//! head is exercised only by the synthetic tests here. STATUS is `Partial`:
//! the head (through 0x006DD383) is transcribed, the per-type tail is not.

#![allow(dead_code)]

use crate::sections::{Leader, Save};
use crate::tick::{StepStatus, FRAME};

/// Head of `gain_tech` (flags, counters, `age_stamp`, the tech bit, good
/// unlocks) and `techs_per_age` transcribed; the per-type tail, `set_age`,
/// `set_epoch`, `lose_tech` and `fix_tech_flags` are documented stops.
pub const STATUS: StepStatus = StepStatus::Partial;

// --- LeaderData image offsets (body index = image offset − 8) ---------------
const LD_BASE: usize = 0x8;
const LD_WHO: usize = 0x8;
const LD_AGE_STAMP: usize = 0x8f8;
const LD_GOV_HERO_FRAME: usize = 0xa48;
const LD_ALLY_MASK: usize = 0x6929;
const LD_MIN_BODY: usize = LD_ALLY_MASK + 1 - LD_BASE;

// --- LeaderDataEncrypt serialized dword indices -----------------------------
const ENC_BUCKET: usize = 0;
const ENC_PER_GOOD: usize = 9;
const ENC_EPOCH: usize = 55; // epoch[0..4] = 55..58
const ENC_AGES: usize = 59;
const ENC_EPOCHS: usize = 60;
const ENC_DISCOVERED: usize = 61;
const ENC_LEN: usize = 62 * 4;
const NUM_GOODS: usize = 6;

// --- GameInfo settings bytes (`Game.info.settings[i]` = GameInfo+0x18+i) ---
const GI_GAME_RULES: usize = 0x6;
const GI_STARTING_RESOURCES: usize = 0x9;
const GI_RUSH_RULES: usize = 0xe;
const GI_STARTING_TECHNOLOGY: usize = 0x10;
const GI_STARTING_TECHNOLOGY2: usize = 0x11;
const GI_ENDING_TECHNOLOGY: usize = 0x12;
const GI_SCORE_GOAL: usize = 0x16;

/// `Game::starting[6]` (Game+0x600) inside `Game::scalars` (Game+0x550..).
const GAME_STARTING: usize = 0x600 - 0x550;

// --- Constants ---------------------------------------------------------------
const C_WONDER_BONUS_AGE: usize = 0xad4;
const C_CONQUEST_START_MULT: usize = 0xa60;
const C_TRIBE5_SCIENCE_START: usize = 0x5f8;

// --- Type index ranges (TypeData virtuals, re/decomp-all/004705b0..00470870)
pub const FIRST_AGE: i32 = 0x220;
pub const NUM_AGES: i32 = 7;
pub fn is_age_type(t: i32) -> bool {
    (0x220..0x227).contains(&t)
}
pub fn is_epoch_type(t: i32) -> bool {
    (0x227..0x243).contains(&t)
}
pub fn is_tech_type(t: i32) -> bool {
    (0x220..0x275).contains(&t)
}
pub fn is_gov_type(t: i32) -> bool {
    (0x26f..0x275).contains(&t)
}
pub fn is_good_type(t: i32) -> bool {
    (0..0x32).contains(&t)
}

fn get_i32(buf: &[u8], off: usize) -> i32 {
    i32::from_le_bytes(buf[off..off + 4].try_into().unwrap())
}
fn put_i32(buf: &mut [u8], off: usize, v: i32) {
    buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
}
fn ld_i32(l: &Leader, img: usize) -> i32 {
    get_i32(&l.body, img - LD_BASE)
}
fn ld_put_i32(l: &mut Leader, img: usize, v: i32) {
    put_i32(&mut l.body, img - LD_BASE, v)
}
fn enc_i32(l: &Leader, idx: usize) -> i32 {
    get_i32(&l.data_encrypted, idx * 4)
}
fn enc_put_i32(l: &mut Leader, idx: usize, v: i32) {
    put_i32(&mut l.data_encrypted, idx * 4, v)
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
fn starting(save: &Save, g: usize) -> i32 {
    let s = &save.game.scalars;
    if s.len() >= GAME_STARTING + g * 4 + 4 {
        get_i32(s, GAME_STARTING + g * 4)
    } else {
        0
    }
}

/// `Types[t].head` (Type+0x4..+0x5e) when the Rules image carries it.
fn type_head(save: &Save, t: i32) -> Option<&[u8]> {
    let rec = save.rules_tail.rules.types.get(usize::try_from(t).ok()?)?;
    (rec.head.len() >= 90).then_some(rec.head.as_slice())
}
/// `TechType::cat` (+0x14).
pub fn tech_cat(save: &Save, t: i32) -> Option<i32> {
    type_head(save, t).map(|h| get_i32(h, 0x10))
}
/// `TechType::age` (+0x1c8, serialized `ext[0..4]`).
pub fn tech_age(save: &Save, t: i32) -> Option<i32> {
    let rec = save.rules_tail.rules.types.get(usize::try_from(t).ok()?)?;
    (is_tech_type(t) && rec.ext.len() >= 4).then(|| get_i32(&rec.ext, 0))
}
fn type_preq(save: &Save, t: i32, i: usize) -> Option<i32> {
    type_head(save, t).map(|h| get_i32(h, 0x2c + i * 4))
}
fn type_obs(save: &Save, t: i32) -> Option<i32> {
    type_head(save, t).map(|h| get_i32(h, 0x48))
}

/// `tech` BitMask<806> bit (`LeaderData+0x6c0c`).
pub fn has_tech_bit(l: &Leader, t: i32) -> bool {
    if t < 0 {
        return false;
    }
    let b = t as usize;
    l.tech.data.get(b >> 3).is_some_and(|x| x & (1 << (b & 7)) != 0)
}
fn set_tech_bit(l: &mut Leader, t: i32, on: bool) {
    let b = t as usize;
    if let Some(x) = l.tech.data.get_mut(b >> 3) {
        if on {
            *x |= 1 << (b & 7);
        } else {
            *x &= !(1 << (b & 7));
        }
    }
}

/// `LeaderData::has_tech(TypeIndex)` 0x006E0C80 restricted to the indices
/// this module meets: `-1` → true, `-2` → false, goods → true, tech/gov
/// indices → the `tech` bit. Unit/building indices (which recurse into
/// `has_preq`) → `None`.
fn has_tech(l: &Leader, t: i32) -> Option<bool> {
    match t {
        -1 => Some(true),
        -2 => Some(false),
        t if t < 0 => None,
        t if is_good_type(t) => Some(true),
        t if is_tech_type(t) => Some(has_tech_bit(l, t)),
        _ => None,
    }
}

/// `LeaderData::has_preq(good)` for a good type: `preq[0..2]` through
/// `has_tech`. `None` when a preq is not a tech index (the bonus ladder in
/// `leaders_process.rs`).
fn good_has_preq(save: &Save, l: &Leader, g: usize) -> Option<bool> {
    for i in 0..2 {
        let p = type_preq(save, g as i32, i)?;
        if !has_tech(l, p)? {
            return Some(false);
        }
    }
    Some(true)
}

/// `LeaderData::get_team()` 0x006EC040 — side byte of this leader's
/// player (8 = no team / neutral; the side-8 "any ally" rescan needs
/// `is_team` over every leader and is approximated by the raw side).
fn get_team(save: &Save, who: usize) -> i32 {
    let mut idx = 0usize;
    for (i, p) in save.game.info.players.iter().enumerate().take(8) {
        if p.body.len() < 0x35 {
            continue;
        }
        let f = u16::from_le_bytes([p.body[0x30], p.body[0x31]]);
        if f & 1 != 0 && p.body[0x33] as usize == who {
            idx = i;
            if f & 0x50 == 0 {
                break;
            }
        }
    }
    match save.game.info.players.get(idx) {
        Some(p) if p.body.len() >= 0x35 && p.body[0x30] & 1 != 0 => p.body[0x34] as i8 as i32,
        _ => 8,
    }
}

/// `LeaderData::starting_age()` 0x006D7320.
pub fn starting_age(save: &Save, who: usize) -> i32 {
    let st = (setting(save, GI_STARTING_TECHNOLOGY) as i32).min(7);
    if setting(save, GI_GAME_RULES) == 8 && get_team(save, who) == 0 {
        let st2 = (setting(save, GI_STARTING_TECHNOLOGY2) as i32).min(7);
        let end = setting(save, GI_ENDING_TECHNOLOGY) as i32;
        return (st + st2).min(end);
    }
    st
}

/// `LeaderData::techs_per_age(int t)` 0x006D7280 — the "techs due in this
/// age" ladder the Library's research price scales by.
pub fn techs_per_age(save: &Save, who: usize, t: i32) -> Option<i32> {
    let st = if setting(save, GI_GAME_RULES) == 8 {
        setting(save, GI_STARTING_TECHNOLOGY) as i32
    } else {
        starting_age(save, who)
    };
    let end = setting(save, GI_ENDING_TECHNOLOGY) as i32;
    let age = tech_age(save, t)?;
    if st == 0 && end > 6 {
        return Some(age * 4 + 2);
    }
    let n = end - st + 1;
    if n == 0 {
        return Some(0);
    }
    let r = (age - st + 1) * (28 / n) - 2;
    if st == 0 && age == 0 && r == 1 {
        return Some(2);
    }
    Some(r)
}

/// `Game.starting[g]`-derived grant used at 0x006DD08D/0x006DD137/0x006DD149
/// and 0x006DD22B/0x006DD24E.
fn starting_grant(save: &Save, who: usize, g: usize) -> i32 {
    let base = starting(save, g);
    if setting(save, GI_GAME_RULES) == 8 && get_team(save, who) == 0 {
        return (setting(save, GI_SCORE_GOAL) as i32 + 1) * base;
    }
    let l = &save.leaders.slots[who];
    if save.game.sem_ptr.get(2).is_some_and(|b| b & 2 != 0) && l.flags2 & 0x80 != 0 {
        return base * constant(save, C_CONQUEST_START_MULT);
    }
    base
}

/// `LeaderData::bucket_add(good, amount)` 0x0043ED10.
fn bucket_add(l: &mut Leader, g: usize, amount: i32) -> i32 {
    let idx = g * ENC_PER_GOOD + ENC_BUCKET;
    let v = enc_i32(l, idx).wrapping_add(amount);
    enc_put_i32(l, idx, v);
    v
}

fn ready(save: &Save, who: usize) -> bool {
    save.leaders.slots.get(who).is_some_and(|l| {
        l.flags & 1 != 0 && l.body.len() >= LD_MIN_BODY && l.data_encrypted.len() >= ENC_LEN && l.tech.data.len() >= 101
    })
}

/// `Leader::gain_tech(t, x, y, from_build, announce)` 0x006DCB60 on
/// `Leaders[who]` — the head through 0x006DD383 (see module docs). Writes
/// `leader_flags`, `gov_hero_frame`, `ages`/`epochs`/`discovered`/`epoch[]`,
/// `age_stamp[]`, the `tech` bit and newly unlocked goods' `bucket[]`.
/// Every untranscribed section whose gate is open is reported as a stop.
pub fn gain_tech(save: &mut Save, who: usize, t: i32, from_build: bool, announce: bool, effects: &mut Vec<String>) {
    let _ = (from_build, announce);
    if !ready(save, who) || !(0..806).contains(&t) {
        effects.push(format!("gain_tech({who},{t:#x}): inactive/short Leader record or bad type, untouched"));
        return;
    }
    if !is_tech_type(t) {
        // Unit/building upgrades reach gain_tech too (Build::finished on a
        // Barracks etc.); their `has_tech` is `bit && has_preq(t)` over the
        // unit/building preq chain, which lives in leaders_process.rs.
        effects.push(format!(
            "stop: gain_tech({who},{t:#x}) on a non-tech type (unit/building upgrade): has_tech → has_preq \
             chain untranscribed here, untouched"
        ));
        return;
    }
    let fr = frame(save);
    let cat = tech_cat(save, t);
    // 006dcb99
    save.leaders.slots[who].flags |= 0x1000000;
    effects.push(format!("Leaders[{who}].leader_flags |= 0x1000000 (gain_tech {t:#x})"));
    // 006dcbba
    if is_gov_type(t) && fr == 0 {
        ld_put_i32(&mut save.leaders.slots[who], LD_GOV_HERO_FRAME, 1);
        effects.push(format!("Leaders[{who}].gov_hero_frame = 1"));
    }
    // 006dcbe1
    if is_epoch_type(t) && cat == Some(3) && fr != 0 {
        effects.push(format!(
            "stop: gain_tech({t:#x}) science-epoch refund loop over Objects[get_first_library()].queue \
             (Build::refund_cost 0x00620490) untranscribed"
        ));
    }
    // 006dccc1: `no_war` is only message text; computed for the log.
    if is_age_type(t) {
        let rush = setting(save, GI_RUSH_RULES);
        if rush != 0 && t - 0x21f == rush as i32 {
            effects.push(format!("note: age {t:#x} is the rush_rules unlock age ({rush}); war becomes allowed (message only)"));
        }
    }
    // 006dcd07
    effects.push(format!(
        "stop: gain_tech({t:#x}) upgrade probe TechType.vt+0x60(0x134/graft 0x127) → current_upgrade / \
         recursive gain_tech(0x135) needs the graft tables — skipped"
    ));
    // 006dcd83
    let had = has_tech_bit(&save.leaders.slots[who], t);
    if !had {
        let l = &mut save.leaders.slots[who];
        if is_age_type(t) {
            let a = enc_i32(l, ENC_AGES) + 1;
            enc_put_i32(l, ENC_AGES, a);
            ld_put_i32(l, LD_AGE_STAMP + (t - FIRST_AGE) as usize * 4, fr);
            effects.push(format!("Leaders[{who}].ages = {a}, age_stamp[{}] = {fr}", t - FIRST_AGE));
        } else if is_epoch_type(t) {
            let e = enc_i32(l, ENC_EPOCHS) + 1;
            enc_put_i32(l, ENC_EPOCHS, e);
            match cat {
                Some(c) if (0..4).contains(&c) => {
                    let v = enc_i32(l, ENC_EPOCH + c as usize) + 1;
                    enc_put_i32(l, ENC_EPOCH + c as usize, v);
                    effects.push(format!("Leaders[{who}].epochs = {e}, epoch[{c}] = {v}"));
                }
                other => effects.push(format!(
                    "stop: Leaders[{who}].epochs = {e}; epoch[cat] += 1 needs TechType[{t:#x}].cat (got {other:?}) — untouched"
                )),
            }
        } else {
            let d = enc_i32(l, ENC_DISCOVERED) + 1;
            enc_put_i32(l, ENC_DISCOVERED, d);
            effects.push(format!("Leaders[{who}].discovered = {d}"));
        }
    }
    // 006dcdf5: conquest wonder-age branch.
    if save.game.sem_ptr.get(2).is_some_and(|b| b & 2 != 0) && is_epoch_type(t) && cat == Some(1) && fr != 0 {
        let e1 = enc_i32(&save.leaders.slots[who], ENC_EPOCH + 1);
        if e1 == constant(save, C_WONDER_BONUS_AGE) {
            effects.push(format!(
                "stop: gain_tech({t:#x}) conquest wonder-age branch (has_wonder 0x21e → flags 0x800; \
                 has_wonder 0x219 unit grants) needs the Objects wonder list — untouched"
            ));
        }
    }
    // 006dcf85
    let mut avail_before = [None; NUM_GOODS];
    for (g, slot) in avail_before.iter_mut().enumerate() {
        *slot = good_has_preq(save, &save.leaders.slots[who], g);
    }
    // 006dcfab
    set_tech_bit(&mut save.leaders.slots[who], t, true);
    effects.push(format!("Leaders[{who}].tech.set({t:#x}) (was {})", had as i32));
    // 006dd027
    save.leaders.slots[who].flags |= 0x2000000;
    effects.push(format!("Leaders[{who}].leader_flags |= 0x2000000"));
    // 006dd02d
    if save.game.sem_ptr.get(1).is_some_and(|b| b & 8 != 0) {
        effects.push("note: Game.semaphore.ptr[1] & 8 — good-unlock grants skipped (retail skips too)".into());
    } else {
        for g in 0..NUM_GOODS {
            match avail_before[g] {
                Some(true) => continue,
                None => {
                    effects.push(format!(
                        "stop: has_preq(good {g}) before gain needs the bonus ladder — good-unlock grant for {g} unevaluated"
                    ));
                    continue;
                }
                Some(false) => {}
            }
            if type_preq(save, g as i32, 0) != Some(t) {
                continue;
            }
            let amount = starting_grant(save, who, g);
            let v = bucket_add(&mut save.leaders.slots[who], g, amount);
            effects.push(format!("Leaders[{who}].bucket[{g}] += {amount} (good unlocked by {t:#x}) = {v}"));
            if setting(save, GI_STARTING_RESOURCES) == 8 {
                enc_put_i32(&mut save.leaders.slots[who], g * ENC_PER_GOOD + ENC_BUCKET, 0x1269f);
                effects.push(format!("Leaders[{who}].bucket[{g}] = 0x1269f (starting_resources == 8)"));
            }
        }
        // 006dd19e
        for g in 0..NUM_GOODS {
            if g == 2 || g == 3 || type_obs(save, g as i32) != Some(t) {
                continue;
            }
            let b = enc_i32(&save.leaders.slots[who], g * ENC_PER_GOOD + ENC_BUCKET);
            if b > 100 {
                effects.push(format!(
                    "stop: good {g} obsoleted by {t:#x} with bucket {b} > 100 → Leader::action_sell({g}, 0) loop \
                     0x006CFA90 (market prices) untranscribed"
                ));
            }
        }
        // 006dd1e4
        if t == FIRST_AGE && constant(save, C_TRIBE5_SCIENCE_START) != 0 {
            effects.push(format!(
                "stop: gain_tech(0x220) has_tribe_bonus(5) science grant (Constants[0x5f8] != 0) — \
                 has_tribe_bonus lives in leaders_process; untouched"
            ));
        }
    }
    // 006dd268..006dd383: local-player message/sound/camera — presentation.
    // 006dd38d
    if is_epoch_type(t) && cat == Some(1) {
        let e1 = enc_i32(&save.leaders.slots[who], ENC_EPOCH + 1);
        if e1 == constant(save, C_WONDER_BONUS_AGE) {
            effects.push(format!(
                "stop: gain_tech({t:#x}) epoch[1] == Constants[0xad4]: calc_pop_cap / calc_anti_attrition / \
                 calc_attrition / Region::fix_borders untranscribed"
            ));
        }
    }
    effects.push(format!(
        "stop: gain_tech({t:#x}) tail 0x006DD3E3..0x006E05C5 (special-tech Categories table, per-type \
         Objects/Cities/Walls effects, wonder-granted recursions, tech-race victory) untranscribed"
    ));
}

/// `Leader::fix_tech_flags` 0x006D2480 — the `ally_mask = 1 << who` seed
/// (0x006D2495). The ally bits and `leader_flags` 0x1000/0x800/0x2000 need
/// the special-tech `Categories` table and are reported.
pub fn fix_tech_flags_seed(save: &mut Save, who: usize, effects: &mut Vec<String>) {
    if !ready(save, who) {
        return;
    }
    let l = &mut save.leaders.slots[who];
    l.body[LD_ALLY_MASK - LD_BASE] = 1u8 << (ld_i32(l, LD_WHO) & 7);
    effects.push(format!(
        "Leaders[{who}].ally_mask = 1<<who; stop: ally bits (special tech [0xac0]) and leader_flags \
         0x1000/0x800/0x2000 (specials [0xac8]/[0xad0]/[0xacc], wonder 0x21e) need the Categories table"
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sections::TypeRec;

    fn save() -> Save {
        let mut s = Save::default();
        s.game.scalars = vec![0u8; 404];
        put_i32(&mut s.game.scalars, FRAME, 500);
        for g in 0..6 {
            put_i32(&mut s.game.scalars, GAME_STARTING + g * 4, 200 + g as i32);
        }
        s.game.info.settings = vec![0u8; 0x1e];
        s.game.info.settings[GI_ENDING_TECHNOLOGY] = 7;
        s.game.sem_ptr = vec![0u8; 32];
        s.rules_tail.rules.constants = vec![0u8; 0xd40];
        s.rules_tail.rules.types = (0..806)
            .map(|i| {
                let mut t = TypeRec::default();
                t.head = vec![0u8; 90];
                put_i32(&mut t.head, 0x2c, -1);
                put_i32(&mut t.head, 0x30, -1);
                put_i32(&mut t.head, 0x48, -1);
                if is_tech_type(i as i32) {
                    t.ext = vec![0u8; 27];
                    // Epoch types: cat = (i - 0x227) % 4; age types cat 4.
                    let cat = if is_age_type(i as i32) { 4 } else if is_epoch_type(i as i32) { (i as i32 - 0x227) % 4 } else { 5 };
                    put_i32(&mut t.head, 0x10, cat);
                    let age = if is_age_type(i as i32) { i as i32 - 0x220 } else { 2 };
                    put_i32(&mut t.ext, 0, age);
                }
                t
            })
            .collect();
        let mut l = Leader::default();
        l.flags = 3;
        l.body = vec![0u8; 0x6922];
        ld_put_i32(&mut l, LD_WHO, 0);
        l.data_encrypted = vec![0u8; ENC_LEN];
        l.tech.data = vec![0u8; 101];
        s.leaders.slots = vec![l];
        s
    }

    #[test]
    fn plain_tech_sets_bit_and_counts_discovered() {
        let mut s = save();
        let mut e = Vec::new();
        gain_tech(&mut s, 0, 0x250, true, true, &mut e);
        let l = &s.leaders.slots[0];
        assert!(has_tech_bit(l, 0x250));
        assert_eq!(enc_i32(l, ENC_DISCOVERED), 1);
        assert_eq!(enc_i32(l, ENC_AGES), 0);
        assert_eq!(l.flags & 0x3000000, 0x3000000);
        // Re-gaining does not recount.
        gain_tech(&mut s, 0, 0x250, true, true, &mut e);
        assert_eq!(enc_i32(&s.leaders.slots[0], ENC_DISCOVERED), 1);
    }

    #[test]
    fn age_and_epoch_counters() {
        let mut s = save();
        let mut e = Vec::new();
        gain_tech(&mut s, 0, 0x221, true, true, &mut e);
        let l = &s.leaders.slots[0];
        assert_eq!(enc_i32(l, ENC_AGES), 1);
        assert_eq!(ld_i32(l, LD_AGE_STAMP + 4), 500);
        assert_eq!(enc_i32(l, ENC_DISCOVERED), 0);
        gain_tech(&mut s, 0, 0x228, true, true, &mut e); // epoch cat 1
        gain_tech(&mut s, 0, 0x22c, true, true, &mut e); // epoch cat 1
        gain_tech(&mut s, 0, 0x229, true, true, &mut e); // epoch cat 2
        let l = &s.leaders.slots[0];
        assert_eq!(enc_i32(l, ENC_EPOCHS), 3);
        assert_eq!(enc_i32(l, ENC_EPOCH + 1), 2);
        assert_eq!(enc_i32(l, ENC_EPOCH + 2), 1);
        assert_eq!(enc_i32(l, ENC_EPOCH), 0);
    }

    #[test]
    fn gov_at_frame_zero_marks_hero_frame() {
        let mut s = save();
        put_i32(&mut s.game.scalars, FRAME, 0);
        let mut e = Vec::new();
        gain_tech(&mut s, 0, 0x270, false, true, &mut e);
        assert_eq!(ld_i32(&s.leaders.slots[0], LD_GOV_HERO_FRAME), 1);
    }

    #[test]
    fn unlocking_a_good_grants_starting_stock() {
        let mut s = save();
        // Good 4 (knowledge) requires tech 0x230.
        put_i32(&mut s.rules_tail.rules.types[4].head, 0x2c, 0x230);
        put_i32(&mut s.leaders.slots[0].data_encrypted, (4 * ENC_PER_GOOD) * 4, 10);
        let mut e = Vec::new();
        gain_tech(&mut s, 0, 0x230, true, true, &mut e);
        assert_eq!(enc_i32(&s.leaders.slots[0], 4 * ENC_PER_GOOD), 10 + 204);
        // Other goods untouched.
        assert_eq!(enc_i32(&s.leaders.slots[0], 0), 0);
        // starting_resources == 8 pins the bucket afterwards.
        let mut s2 = save();
        put_i32(&mut s2.rules_tail.rules.types[5].head, 0x2c, 0x231);
        s2.game.info.settings[GI_STARTING_RESOURCES] = 8;
        gain_tech(&mut s2, 0, 0x231, true, true, &mut e);
        assert_eq!(enc_i32(&s2.leaders.slots[0], 5 * ENC_PER_GOOD), 0x1269f);
        // Observer semaphore skips grants.
        let mut s3 = save();
        put_i32(&mut s3.rules_tail.rules.types[4].head, 0x2c, 0x230);
        s3.game.sem_ptr[1] |= 8;
        gain_tech(&mut s3, 0, 0x230, true, true, &mut e);
        assert_eq!(enc_i32(&s3.leaders.slots[0], 4 * ENC_PER_GOOD), 0);
    }

    #[test]
    fn techs_per_age_ladder() {
        let mut s = save();
        // st = 0, end = 7 -> 4*age + 2.
        assert_eq!(techs_per_age(&s, 0, 0x250), Some(10));
        assert_eq!(techs_per_age(&s, 0, 0x223), Some(14));
        // st = 1, end = 5: n = 5, 28/5 = 5; age 2 -> (2-1+1)*5-2 = 8.
        s.game.info.settings[GI_STARTING_TECHNOLOGY] = 1;
        s.game.info.settings[GI_ENDING_TECHNOLOGY] = 5;
        assert_eq!(techs_per_age(&s, 0, 0x250), Some(8));
        // st = 0, end = 6: n = 7, 28/7 = 4; age 0 -> 4-2 = 2; age 2 -> 12-2 = 10.
        s.game.info.settings[GI_STARTING_TECHNOLOGY] = 0;
        s.game.info.settings[GI_ENDING_TECHNOLOGY] = 6;
        assert_eq!(techs_per_age(&s, 0, 0x220), Some(2));
        assert_eq!(techs_per_age(&s, 0, 0x250), Some(10));
    }

    #[test]
    fn oracle_no_tech_change() {
        let Ok(root) = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize() else {
            return;
        };
        let Ok(rd) = std::fs::read_dir(root.join("schema/live/frame-pairs")) else {
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
            let mut prev: Option<Vec<(Vec<u8>, Vec<u8>)>> = None;
            for p in svx {
                let raw = crate::container::load_svx(&p).unwrap();
                let (s, _) = crate::sections::load_save(&raw).unwrap();
                let cur: Vec<(Vec<u8>, Vec<u8>)> = s
                    .leaders
                    .slots
                    .iter()
                    .map(|l| {
                        let enc = if l.data_encrypted.len() >= ENC_LEN {
                            l.data_encrypted[ENC_EPOCH * 4..ENC_LEN].to_vec()
                        } else {
                            Vec::new()
                        };
                        (l.tech.data.clone(), enc)
                    })
                    .collect();
                if let Some(pv) = &prev {
                    assert_eq!(pv, &cur, "{}: tech bits / age counters changed", p.display());
                }
                prev = Some(cur);
            }
        }
    }
}
