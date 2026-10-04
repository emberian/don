//! Step 8: `Leaders::process_all` 0x006ED2A0 (`re/decomp-all/006ed2a0.c`)
//! and the per-leader economy pass it drives.
//!
//! # Loop bound
//!
//! Both loops in `process_all` run `0x00e3a390 .. 0x00e71af0` at stride
//! `0x6eec` — exactly **eight** Leader records (slots 0..7). Slot 8
//! (Nature, `who == 8`) is never visited here even though its
//! `leader_flags` carries bit 1 — the oracle confirms it (Nature has
//! `leader_flags & 0x2000000` and `(8 + 16) & 7 == 0`, so a visit at frame
//! 16 would have stamped `gather_stamp`; it did not).
//!
//! # Per leader with `leader_flags & 2` (0x006ED2B0..0x006ED40D)
//!
//! 1. `leader_flags &= ~0x40000`; `pop_issues` (+0x7e8) = 0; `retargets`
//!    (+0x9f4) = 0.
//! 2. Threat flag (0x006ED2E0..0x006ED321): for each other record `o`
//!    (slots 0..7) with `leader_flags & 1` and `o.who != who`: if
//!    `(o.diplos[who] != 2 || Leaders[who].diplos[o.who] != 2) &&
//!    (o.leader_flags & 0x20000)` then `leader_flags |= 0x40000`.
//! 3. `Leader::gather` 0x006CE280 (below).
//! 4. `leader_flags & 0x8000000` → clear it, `Leader::calc_wall_stats`
//!    0x006CF7C0 — UNTRANSCRIBED (rebuilds wall statistics from the
//!    Objects lists; the flag clear is transcribed).
//! 5. `leader_flags & 0x4000000` → clear it, `Leader::calc_unit_stats`
//!    0x006CF970 — UNTRANSCRIBED (same; flag clear transcribed).
//! 6. `Leader::process_elimination` 0x006B8A20: gate transcribed
//!    (`Game.info.elimination == 1 && lost_capital_timer != 0`), the body
//!    (`Game::retake_capital` 0x00594530 → `Leader::defeat` 0x006ECB00) is
//!    UNTRANSCRIBED.
//! 7. Timer ticks (0x006ED35F..0x006ED3CE): when
//!    `Constants[0xd00] != 0 && frame % Constants[0xd00] == 0`: for the
//!    pairs (`lost_capital_stamp` +0x414, `lost_capital_timer` +0x418),
//!    (`popwin_stamp` +0x440, `popwin_timer` +0x444), (`wonderwin_stamp`
//!    +0x448, `wonderwin_timer` +0x44c): `if stamp < 0 && timer == 0 {
//!    stamp += 1 }`.
//! 8. Taunts (0x006ED3CE..0x006ED407): if `frame != 0`, for `i` in 0..8
//!    with `incoming_taunt_frame[i] (+0x3d4) == frame`:
//!    `Leader::process_taunt(incoming_taunt[i] (+0x394),
//!    incoming_taunt_who[i] (+0x3b4))` 0x006B8CC0 — UNTRANSCRIBED (AI chat).
//! 9. `leader_flags &= ~0x80000`.
//!
//! # `Leader::gather` 0x006CE280 (`006ce280.c`)
//!
//! 1. Decrypt `resources[0..6]` into locals and call `Leader::calc_gather`
//!    0x006CEEE0 on them. Its gate (0x006CEEF4..0x006CEF27): with
//!    `leader_flags & 0x2000000` clear, run only when
//!    `frame >= gather_stamp + 300 && (frame + who*8) % 256 == 0`; with it
//!    set, run when `frame == 0 || (who + frame) % 8 == 0`. The body
//!    (income recomputation from cities/buildings/caravans/rares, writes
//!    `resources`, `known_rares`, `rares_collected`, `rare_owned`,
//!    `gather_stamp = frame`, clears 0x2000000) is UNTRANSCRIBED: when the
//!    gate passes, nothing is written and the effect log says so. In the
//!    stride-1 captures the gate never passes (frames 11..40,
//!    `gather_stamp` 2..9).
//! 2. `rare = rare_owned | rare_conquest` (`BitMask<44>::operator|`
//!    0x0047CE20 / `operator!=` 0x0047CD90 compare payload bytes only); on
//!    change `leader_flags |= 0xc000000`.
//! 3. `support[0..6] = 0` (stores of the bare key `0x26076`).
//! 4. `Leader::calc_resource_caps` 0x006CE900 — transcribed (below).
//! 5. `Leader::do_gather` 0x006CE450 — transcribed (below).
//!
//! # `LeaderDataEncrypt` and `data_encrypted`
//!
//! In memory every dword of `LeaderDataEncrypt` (`Leader+0x6eb8` →, 248 B)
//! is stored XOR a per-field key: `bucket ^0x8221`, `leftover ^0x3421`,
//! `resource_cap ^0x1281`, `over_cap ^0x8932`, `resources ^0x872`,
//! `support ^0x26076`, `income ^0x90236`, `rate ^0x73862`, `bonus ^0x6722`,
//! `epoch[4] ^0x63187`, `ages ^0x62766`, `epochs ^0x69587`,
//! `discovered ^0x13985`. `LeaderDataEncrypt::walk_data` 0x006D9900 walks
//! the **plaintext** (XORs on both load and save), in this order: for each
//! good `g` in 0..6 the nine dwords `bucket[g], leftover[g],
//! resource_cap[g], over_cap[g], resources[g], support[g], income[g],
//! rate[g], bonus[g]` (serialized index `9*g + k`), then `resource_cap[6]`
//! (54), `epoch[0..4]` (55..58), `ages` (59), `epochs` (60), `discovered`
//! (61). `Save.leaders.slots[p].data_encrypted` therefore holds plain
//! values and this module reads/writes it with no key.
//!
//! Semantics: `bucket[g]` is the stockpile shown in the HUD; `resources[g]`
//! is the gross per-minute gather rate ×16 computed by `calc_gather`;
//! `support[g]` upkeep (zeroed here, filled by later steps); `income[g]` the
//! net rate after caps; `leftover[g]` the sub-unit accumulator; `over_cap`
//! the "at commerce cap" indicator (0 / 1 / 2).
//!
//! # `Leader::calc_resource_caps` 0x006CE900 (`006ce900.c`)
//!
//! For `g` in 0..7 (seven slots; `resource_cap[6]` exists):
//! `g == 3` → `cap = 999`. Otherwise `cap = Constants[0x400 + epoch[2]*4]`;
//! `has_tribe_bonus(0xb)` → `cap = (Constants[0x6d0]+100)*cap/100`;
//! `g==2 && Constants[0x598] != 0 && has_tribe_bonus(2)`,
//! `g==1 && Constants[0x6cc] != 0 && has_tribe_bonus(10)`,
//! `g==0 && Constants[0x654] != 0 && has_tribe_bonus(7)` → same
//! percentage form; `Leaders[who].rare.data[2] & 0x40 ||
//! Leaders[who].rare_conquest.data[2] & 0x40` → `(Constants[0x92c]+100)%`;
//! wonder adds (`has_wonder` 0x006EBC10): `g∈{0,2}` wonder 0x20e →
//! `+Constants[0x428]`; `g∈{1,2}` wonder 0x20f → `+[0x444]`, and `g==2`
//! wonder 0x21b → `+[0x500]`; `g==5` wonder 0x21c → `+[0x544]`, wonder
//! 0x21a → `+[0x50c]`; `g∈{0,4}` wonder 0x21a → `+[0x50c]`, `g==4` wonder
//! 0x217 → `+[0x4d4]` (via `resource_cap_add` 0x0047DA40); then
//! `has_preq(0x31f)` → `+[0x9a8]` else `has_preq(0x31e)` → `+[0x9a4]` else
//! `has_preq(0x31d)` → `+[0x9a0]`; `cap += bonus_cap[g]` (+0x918); clamp to
//! 0..999. For every `g` (including 3): `has_preq(0x2b7)` → `cap = 999`;
//! finally `resource_cap[g] = cap << 4`.
//!
//! # `Leader::do_gather` 0x006CE450 (`006ce450.c`)
//!
//! For `g` in 0..6 with `LeaderData::type_avail(g, 1) != 0`:
//! * `Game.info.starting_resources == 8` → `Leaders[who].bucket[g] =
//!   0x1269f`, `Leaders[who].escrow[g] = 0`.
//! * else `inc = resources[g] - support[g] + base_rate[g]` (+0x4b0).
//!   `inc < 0` → `income[g] = inc; over_cap[g] = 0` and nothing else.
//!   Otherwise: `resource_cap[g] < inc` → (UI feedback when `over_cap[g]
//!   == 0 && who == Console::who`: `MessageWin::add_feedback` +
//!   `SoundGlobal::play`, which draws on `SoundGlobal::random`, not the
//!   main LCG) `over_cap[g] = (resource_cap[g] > 0x3e6f) + 1; inc =
//!   resource_cap[g]`; else `over_cap[g] = 0`. `g != 3 &&
//!   has_tribe_bonus(0x16)`: `base = Game.starting[g]` (Game+0x600, i.e.
//!   `scalars[0xb0 + 4g]`), or `(starting_resources2+1) * starting[g]`
//!   when `Game.info.game_rules == 8` and `get_team() == 0`, or `starting[g] *
//!   Constants[0xa60]` when `Game.semaphore.ptr[2] & 2 && leader_flags2 &
//!   0x80`; `over = Leaders[who].bucket[g] - base`; `over > 0` → `inc2 =
//!   inc + (Constants[0x8a8]*over/100)*16` capped at `Constants[0x8ac]*16 +
//!   resource_cap[g]`; `inc = min(inc2, 0x3e70)`. **`income[g] = inc`.**
//!   Then local-only scaling: `h = get_gather_handicap()` → `inc =
//!   (h+100)*inc/100` when `h != 0`; `g == 3 && tech_cost > 4` → `inc*3/4`
//!   (`tech_cost < 7`) or `inc/2`; `Game.info.flags & 2 || game_rules == 9`
//!   → `inc*3/2`; `GameAccess::ai_speed > 1` → `inc *= ai_speed`
//!   (`ai_speed` is a cheat global, not serialized; treated as 1).
//!   `unit = Constants[0x27c]*16` (450 → 7200): `whole = inc / unit;
//!   leftover[g] += inc % unit; while leftover[g] >= unit { whole += 1;
//!   leftover[g] -= unit }`; `Leaders[who].bucket[g] += whole;
//!   collected[g] += whole`. If `Leaders[who].leader_flags & 0xc != 4`:
//!   `x = escrow_rate[g]*inc; u = Constants[0x27c]*0x640; q = x/u; r =
//!   x%u; if r != 0 { d = max(2, (u + r/2)/r); if frame % d == 0 { q += 1
//!   } }; Leaders[who].escrow[g] += q`.
//!
//! `LeaderData::type_avail` 0x006E33A0 / `has_preq` 0x006DB810 /
//! `type_eligible` 0x006DBD10 / `has_tech` 0x006E0C80 / `has_tribe_bonus`
//! 0x006E1370 / `has_wonder` 0x006EBC10 / `get_gather_handicap` 0x006D66A0
//! are transcribed over the serialized Rules (`TypeRec.head` = Type+0x4..
//! +0x5e: `tribe_mask` head[0xc], `preq[3]` head[0x2c..0x38], `obs`
//! head[0x48]; `TechType.age` = tech `ext[0..4]`; `Tribe.tribe` =
//! `TribeRec.a[0..4]`) with the `TypeData` virtuals resolved statically by
//! index range (`is_unit_type` 0x32..0x19d, `is_building_type`
//! 0x19e..0x21e, `is_wonder_type` 0x20e..0x21e, `is_age_type` 0x220..0x226,
//! `is_epoch_type` 0x227..0x242, `is_tech_type` 0x220..0x274,
//! `is_gov_type` 0x26f..0x274, `is_bonus_type` 0x2ac..0x325,
//! `GoodTypeData::num_preq == BonusTypeData::num_preq == 2`). Branches that
//! need unserialized state (the `Objects` wonder list when `wonder_mark >
//! 0`, `get_handicap`'s Categories table, `get_team`'s `is_team` scan,
//! `has_preq` on unit/building/age/gov types, `get_preq(1)` substitutions)
//! return `None`; a `None` anywhere in a good's evaluation leaves that
//! good's fields untouched and is reported in the effect log.
//!
//! # RNG
//!
//! No path above calls `Random::get` on `GameAccess::game_random`. The only
//! RNG reachable is `SoundGlobal::play` inside the over-cap UI branch,
//! which draws on `SoundGlobal::random` (0x00e85f0c). Main-LCG draws per
//! frame from this step: 0.

use crate::prim::BitMask;
use crate::sections::{Game, Leader, Rules};
use crate::tick::{StepStatus, FRAME};
use crate::Save;

/// `process_all` control flow, `gather`, `calc_resource_caps` and
/// `do_gather` are transcribed; `calc_gather`'s body, `calc_wall_stats`,
/// `calc_unit_stats`, `process_elimination`'s defeat path and
/// `process_taunt` are not.
pub const STATUS: StepStatus = StepStatus::Partial;

// --- LeaderData image offsets (body index = image offset − 8) ---------------
const LD_BASE: usize = 0x8;
const LD_WHO: usize = 0x8;
const LD_TRIBE: usize = 0xc;
const LD_MULTI_DIFF: usize = 0x50;
const LD_DIPLOS: usize = 0x74;
const LD_INCOMING_TAUNT: usize = 0x394;
const LD_INCOMING_TAUNT_WHO: usize = 0x3b4;
const LD_INCOMING_TAUNT_FRAME: usize = 0x3d4;
const LD_CITY_NUM: usize = 0x3f8;
const LD_LOST_CAPITAL_STAMP: usize = 0x414;
const LD_LOST_CAPITAL_TIMER: usize = 0x418;
const LD_WONDER_MARK: usize = 0x424;
const LD_POPWIN_STAMP: usize = 0x440;
const LD_POPWIN_TIMER: usize = 0x444;
const LD_WONDERWIN_STAMP: usize = 0x448;
const LD_WONDERWIN_TIMER: usize = 0x44c;
const LD_ESCROW: usize = 0x468;
const LD_ESCROW_RATE: usize = 0x480;
const LD_BASE_RATE: usize = 0x4b0;
const LD_GATHER_STAMP: usize = 0x7ac;
const LD_POP_ISSUES: usize = 0x7e8;
const LD_COLLECTED: usize = 0x874;
const LD_BONUS_CAP: usize = 0x918;
const LD_RETARGETS: usize = 0x9f4;
/// Body length needed for every field above.
const LD_MIN_BODY: usize = LD_RETARGETS + 4 - LD_BASE;

// --- LeaderDataEncrypt serialized dword indices -----------------------------
const ENC_BUCKET: usize = 0;
const ENC_LEFTOVER: usize = 1;
const ENC_RESOURCE_CAP: usize = 2;
const ENC_OVER_CAP: usize = 3;
const ENC_RESOURCES: usize = 4;
const ENC_SUPPORT: usize = 5;
const ENC_INCOME: usize = 6;
const ENC_PER_GOOD: usize = 9;
const ENC_RESOURCE_CAP6: usize = 54;
const ENC_EPOCH: usize = 55; // epoch[0..4] = 55..58
const ENC_LEN: usize = 62 * 4;
const NUM_GOODS: usize = 6;

// --- GameInfo offsets (GameInfo-relative; Game+X == GameInfo+(X-0xc)) -------
const GI_FLAGS: usize = 0x14; // head[20..24]
const GI_SETTINGS_BASE: usize = 0x18;
const GI_GAME_RULES: usize = 0x1e;
const GI_DIFFICULTY: usize = 0x1f;
const GI_STARTING_TOWN: usize = 0x20;
const GI_STARTING_RESOURCES: usize = 0x21;
const GI_STARTING_RESOURCES2: usize = 0x22;
const GI_TECH_COST: usize = 0x23;
const GI_STARTING_TECHNOLOGY: usize = 0x28;
const GI_STARTING_TECHNOLOGY2: usize = 0x29;
const GI_ENDING_TECHNOLOGY: usize = 0x2a;
const GI_ELIMINATION: usize = 0x2b;

/// `Game::starting[6]` (Game+0x600) inside `Game::scalars`.
const GAME_STARTING: usize = 0x600 - 0x550;

// --- Constants offsets -------------------------------------------------------
const C_CAP_BY_EPOCH: usize = 0x400;
const C_GATHER_RATE: usize = 0x27c;
const C_TIMER_DIVISOR: usize = 0xd00;
const C_COMMERCE_OVER_PCT: usize = 0x8a8;
const C_COMMERCE_OVER_CAP: usize = 0x8ac;
const C_CONQUEST_START_MULT: usize = 0xa60;
const C_WONDER_AGE_MIN: usize = 0xad4;

// --- Type index ranges (TypeData virtuals, re/decomp-all/004705b0..00470870)
fn is_unit_type(t: i32) -> bool {
    (0x32..0x19e).contains(&t)
}
fn is_building_type(t: i32) -> bool {
    (0x19e..0x21f).contains(&t)
}
fn is_wonder_type(t: i32) -> bool {
    (0x20e..0x21f).contains(&t)
}
fn is_age_type(t: i32) -> bool {
    (0x220..0x227).contains(&t)
}
fn is_epoch_type(t: i32) -> bool {
    (0x227..0x243).contains(&t)
}
fn is_tech_type(t: i32) -> bool {
    (0x220..0x275).contains(&t)
}
fn is_gov_type(t: i32) -> bool {
    (0x26f..0x275).contains(&t)
}
fn is_bonus_type(t: i32) -> bool {
    (0x2ac..0x326).contains(&t)
}
fn is_good_type(t: i32) -> bool {
    t < 0x32
}
/// `TypeData::is_plain_tech_type` 0x004705D0.
fn is_plain_tech_type(t: i32) -> bool {
    is_tech_type(t) && !is_age_type(t) && !is_epoch_type(t)
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
fn bit(mask: &BitMask, b: i32) -> bool {
    if b < 0 {
        return false;
    }
    let b = b as usize;
    mask.data.get(b >> 3).is_some_and(|x| x & (1 << (b & 7)) != 0)
}

/// Read-only game context shared by every leader in the pass.
struct Ctx<'a> {
    frame: i32,
    game: &'a Game,
    rules: &'a Rules,
    constants_fallback: &'a [u8],
}

impl Ctx<'_> {
    /// `Constants` image accessor (typed Rules image, falling back to the
    /// earlier direct walk of the same object).
    fn constant(&self, off: usize) -> i32 {
        let c = &self.rules.constants;
        if c.len() >= off + 4 {
            return get_i32(c, off);
        }
        if self.constants_fallback.len() >= off + 4 {
            return get_i32(self.constants_fallback, off);
        }
        0
    }
    fn info_byte(&self, gi_off: usize) -> u8 {
        self.game.info.settings.get(gi_off - GI_SETTINGS_BASE).copied().unwrap_or(0)
    }
    fn info_flags(&self) -> u32 {
        let h = &self.game.info.head;
        if h.len() >= GI_FLAGS + 4 {
            get_i32(h, GI_FLAGS) as u32
        } else {
            0
        }
    }
    fn sem(&self, i: usize) -> u8 {
        self.game.sem_ptr.get(i).copied().unwrap_or(0)
    }
    fn starting(&self, g: usize) -> i32 {
        get_i32(&self.game.scalars, GAME_STARTING + g * 4)
    }
    fn type_head(&self, t: i32) -> Option<&[u8]> {
        let t = usize::try_from(t).ok()?;
        let h = &self.rules.types.get(t)?.head;
        (h.len() >= 90).then_some(h.as_slice())
    }
    fn type_tribe_mask(&self, t: i32) -> Option<u32> {
        self.type_head(t).map(|h| get_i32(h, 0xc) as u32)
    }
    fn type_preq(&self, t: i32, i: usize) -> Option<i32> {
        self.type_head(t).map(|h| get_i32(h, 0x2c + i * 4))
    }
    fn type_obs(&self, t: i32) -> Option<i32> {
        self.type_head(t).map(|h| get_i32(h, 0x48))
    }
    /// `TechType::age` (+0x1c8) — serialized as tech `ext[0..4]`.
    fn tech_age(&self, t: i32) -> Option<i32> {
        let rec = self.rules.types.get(usize::try_from(t).ok()?)?;
        if !is_tech_type(t) || rec.ext.len() < 4 {
            return None;
        }
        Some(get_i32(&rec.ext, 0))
    }
    /// `Tribes[tribe].tribe` (+0x54) — `TribeRec.a[0..4]`.
    fn tribe_bonus_id(&self, tribe: i32) -> Option<i32> {
        let rec = self.rules.tribes.get(usize::try_from(tribe).ok()?)?;
        (rec.a.len() >= 4).then(|| get_i32(&rec.a, 0))
    }
}

/// Evaluator for the `LeaderData` const queries over one leader snapshot.
struct Eval<'a> {
    ctx: &'a Ctx<'a>,
    l: &'a Leader,
}

impl Eval<'_> {
    fn who(&self) -> i32 {
        ld_i32(self.l, LD_WHO)
    }
    fn tribe(&self) -> i32 {
        ld_i32(self.l, LD_TRIBE)
    }
    fn tech_bit(&self, t: i32) -> bool {
        bit(&self.l.tech, t)
    }

    /// `LeaderData::has_tribe_bonus(int)` 0x006E1370.
    fn has_tribe_bonus(&self, b: i32) -> Option<bool> {
        if self.ctx.info_flags() & 4 != 0 {
            return Some(false);
        }
        if self.ctx.info_byte(GI_STARTING_TOWN) == 0 && ld_i32(self.l, LD_CITY_NUM) == 0 {
            return Some(false);
        }
        let tribe = self.tribe();
        if tribe < 0 {
            return Some(false);
        }
        if bit(&self.l.conquest_racial_powers, b) {
            return Some(true);
        }
        if self.l.flags2 & 0x40 != 0 {
            return Some(false);
        }
        Some(self.ctx.tribe_bonus_id(tribe)? == b)
    }

    /// `LeaderData::has_tech(TypeIndex)` 0x006E0C80.
    fn has_tech(&self, t: i32, depth: u32) -> Option<bool> {
        if t == -1 {
            return Some(true);
        }
        if t == -2 {
            return Some(false);
        }
        if t < 0x32 {
            return Some(true);
        }
        if is_unit_type(t) && !self.has_preq(t, depth + 1)? {
            return Some(false);
        }
        if is_building_type(t) {
            return self.has_preq(t, depth + 1);
        }
        Some(self.tech_bit(t))
    }

    /// `LeaderData::get_govs_taken` 0x006D69F0.
    fn govs_taken(&self, depth: u32) -> Option<i32> {
        let mut n = 0;
        for g in 0x26f..0x275 {
            if self.has_tech(g, depth + 1)? {
                n += 1;
            }
        }
        Some(n)
    }

    /// `TypeData::get_preq(int, int)` 0x00668700 for the preq slots the
    /// goods/bonus evaluation reaches. Slot 0 is `preq[0]` verbatim; slot 1
    /// is `preq[1]` when negative, or when no starting/ending-technology
    /// substitution applies (`starting_technology == 0 &&
    /// ending_technology >= 7`, `this` not a gov type); other cases are
    /// the age-ladder substitutions and return `None`.
    fn get_preq(&self, t: i32, i: usize) -> Option<i32> {
        match i {
            0 => self.ctx.type_preq(t, 0),
            1 => {
                let p = self.ctx.type_preq(t, 1)?;
                if p < 0 {
                    return Some(p);
                }
                if is_gov_type(t) {
                    return None;
                }
                let st = if self.ctx.info_byte(GI_GAME_RULES) == 8 {
                    self.ctx.info_byte(GI_STARTING_TECHNOLOGY).min(self.ctx.info_byte(GI_STARTING_TECHNOLOGY2))
                } else {
                    self.ctx.info_byte(GI_STARTING_TECHNOLOGY)
                };
                if st == 0 && self.ctx.info_byte(GI_ENDING_TECHNOLOGY) >= 7 {
                    Some(p)
                } else {
                    None
                }
            }
            _ => Some(-1),
        }
    }

    /// `LeaderData::has_preq(TypeIndex)` 0x006DB810, for good (0..0x32)
    /// and bonus (0x2ac..0x325) types — the two kinds whose vtables the
    /// callers here reach (`GoodTypeData::num_preq == BonusTypeData::num_preq
    /// == 2`, `special_preq` is the identity for both).
    fn has_preq(&self, t: i32, depth: u32) -> Option<bool> {
        if depth > 16 || !(is_good_type(t) && t >= 0 || is_bonus_type(t)) {
            return None;
        }
        // Special cases on the way to the generic loop (006db810.c:17-126).
        if t == 3 && self.has_tribe_bonus(5)? && self.ctx.constant(0x5dc) != 0 {
            return Some(true);
        }
        if t == 4 && self.has_tribe_bonus(0xc)? && self.ctx.constant(0x714) != 0 {
            return Some(true);
        }
        // LAB_006dba3b generic preq loop.
        for i in 0..2 {
            let p = self.get_preq(t, i)?;
            if p < 0 {
                if !self.has_tech(p, depth + 1)? {
                    return Some(false);
                }
                continue;
            }
            // special_preq(t, &p) == 0 and p unchanged for good/bonus `t`.
            if !is_bonus_type(p) {
                if !self.has_tech(p, depth + 1)? {
                    return Some(false);
                }
                continue;
            }
            // Bonus-card preq: gov count gates (006db810.c:155-181).
            let p2 = self.get_preq(t, i)?;
            let mut via_b4e = p2 < 0;
            if !via_b4e && !is_bonus_type(p2) {
                via_b4e = true;
            }
            if via_b4e {
                if self.govs_taken(depth + 1)? < 2 {
                    return Some(false);
                }
                let x = self.ctx.type_preq(p, 0)?;
                if (x == 0x271 || x == 0x272) && !self.has_tech(0x273, depth + 1)? && !self.has_tech(0x274, depth + 1)? {
                    return Some(false);
                }
            } else if self.govs_taken(depth + 1)? < 3 {
                return Some(false);
            }
            if p == 0x2ad && self.has_tribe_bonus(4)? {
                continue;
            }
            if !self.has_preq(p, depth + 1)? {
                return Some(false);
            }
        }
        if is_age_type(t) || is_gov_type(t) {
            return None; // techs_per_age / gov ladder: not reached for goods/bonus
        }
        if t < 0x271 {
            return Some(true);
        }
        if self.has_tech(t - 2, depth + 1)? {
            return Some(true);
        }
        self.has_tech(((t - 0x26f) ^ 1) + 0x26d, depth + 1)
    }

    /// `LeaderData::tribe_can_type` 0x006D9410 for a non-unit type.
    fn tribe_can_type(&self, t: i32) -> Option<i32> {
        if is_unit_type(t) {
            return None;
        }
        let mask = self.ctx.type_tribe_mask(t)?;
        let tribe = self.tribe();
        Some(if mask & (1u32 << (tribe as u32 & 0x1f)) != 0 { 4 } else { 0 })
    }

    /// `LeaderData::type_eligible(TypeIndex, 1)` 0x006DBD10 for a good.
    fn type_eligible_good(&self, g: i32) -> Option<i32> {
        if self.tribe_can_type(g)? != 4 {
            return Some(0);
        }
        for i in 0..2 {
            let p = self.get_preq(g, i)?;
            if p < -1 {
                return Some(0);
            }
            if p >= 0 && !is_plain_tech_type(p) && !is_epoch_type(p) {
                // ending_technology < techtypes[p].age → 0
                let age = self.ctx.tech_age(p)?;
                if (self.ctx.info_byte(GI_ENDING_TECHNOLOGY) as i32) < age {
                    return Some(0);
                }
            }
        }
        // is_good_type → param_2 != 0 → has_tech(obs) == 0 ? 4 : 0
        let obs = self.ctx.type_obs(g)?;
        Some(if self.has_tech(obs, 0)? { 0 } else { 4 })
    }

    /// `LeaderData::type_avail(TypeIndex, 1)` 0x006E33A0 for a good: after
    /// `type_eligible == 4` the good is neither unit, building nor gov, so
    /// the function returns 4 (0x006E342D..0x006E3449).
    fn type_avail_good(&self, g: i32) -> Option<i32> {
        if !self.has_preq(g, 0)? {
            return Some(0);
        }
        let e = self.type_eligible_good(g)?;
        if e != 4 {
            return Some(e);
        }
        Some(4)
    }

    /// `LeaderData::has_wonder(int)` 0x006EBC10. The owned-wonder scan over
    /// the Objects lists is only entered when `wonder_mark > 0`.
    fn has_wonder(&self, w: i32) -> Option<i32> {
        if !is_wonder_type(w) {
            return Some(0);
        }
        let mut r = 0;
        if bit(&self.l.conquest_wonders, w - 0x20e)
            && self.ctx.constant(C_WONDER_AGE_MIN) <= enc_i32(self.l, ENC_EPOCH + 1)
        {
            r = 2;
        }
        if ld_i32(self.l, LD_WONDER_MARK) > 0 {
            return None;
        }
        Some(r)
    }

    /// `LeaderData::get_gather_handicap` 0x006D66A0.
    fn get_gather_handicap(&self) -> Option<i32> {
        let sem0 = self.ctx.sem(0);
        if self.l.flags & 4 != 0 {
            if sem0 & 4 != 0 {
                return None; // LeaderData::get_handicap 0x006DA740: Categories table
            }
            return Some(0);
        }
        let multi_diff = ld_i32(self.l, LD_MULTI_DIFF);
        let diff = if sem0 & 4 == 0 {
            let s1 = self.ctx.sem(1);
            let s2 = self.ctx.sem(2);
            if (s1 & 0x10 == 0 && s2 & 2 == 0) || s2 & 2 != 0 || multi_diff < 0 {
                self.ctx.info_byte(GI_DIFFICULTY) as i32
            } else {
                multi_diff
            }
        } else {
            multi_diff
        };
        Some(match diff {
            0 => -35,
            1 => -15,
            2 => -7,
            4 => 25,
            5 => 50,
            _ => 0,
        })
    }

    /// `LeaderData::get_team` 0x006EC040: the player-table scan is
    /// transcribed; the `team == 8` observer fallback (`is_team` scan) is
    /// not.
    fn get_team(&self) -> Option<i32> {
        let who = self.who();
        let players = &self.ctx.game.info.players;
        let mut idx = 0usize;
        for (i, p) in players.iter().enumerate().take(8) {
            if p.flags & 1 != 0 && p.body.len() > 0x34 && p.body[0x33] as i32 == who && p.flags & 0x50 == 0 {
                idx = i;
                break;
            }
        }
        let p = players.get(idx)?;
        if p.flags & 1 == 0 || p.body.len() <= 0x34 {
            return Some(8);
        }
        let team = p.body[0x34] as i8 as i32;
        if team == 8 && (self.ctx.sem(0) as i8) >= 0 {
            return None;
        }
        Some(team)
    }
}

/// Dispatcher used by `tick.rs` for step 8.
pub fn run(save: &mut Save, effects: &mut Vec<String>) {
    let Save { leaders, game, rules_tail, constants, .. } = save;
    if game.scalars.len() < GAME_STARTING + NUM_GOODS * 4 {
        return;
    }
    let ctx = Ctx {
        frame: get_i32(&game.scalars, FRAME),
        game: &*game,
        rules: &rules_tail.rules,
        constants_fallback: &*constants,
    };
    process_all(&ctx, &mut leaders.slots, effects);
}

fn readable(l: &Leader) -> bool {
    l.flags & 1 != 0 && l.body.len() >= LD_MIN_BODY && l.data_encrypted.len() >= ENC_LEN
}

/// `Leaders::process_all` 0x006ED2A0 over slots 0..7.
fn process_all(ctx: &Ctx, slots: &mut [Leader], effects: &mut Vec<String>) {
    let frame = ctx.frame;
    let n = slots.len().min(8);
    for slot in 0..n {
        if slots[slot].flags & 2 == 0 {
            continue;
        }
        slots[slot].flags &= !0x40000;
        if !readable(&slots[slot]) {
            effects.push(format!(
                "Leader[{slot}]: leader_flags & 2 but no serialized body; only the flag clears ran"
            ));
            slots[slot].flags &= !0x80000;
            continue;
        }
        let who = ld_i32(&slots[slot], LD_WHO);
        ld_put_i32(&mut slots[slot], LD_POP_ISSUES, 0);
        ld_put_i32(&mut slots[slot], LD_RETARGETS, 0);

        // 2. threat flag.
        let who_u = usize::try_from(who).ok().filter(|&w| w < n && readable(&slots[w]));
        for o in 0..n {
            if o == slot || slots[o].flags & 1 == 0 || slots[o].body.len() < LD_MIN_BODY {
                continue;
            }
            let o_who = ld_i32(&slots[o], LD_WHO);
            if o_who == who {
                continue;
            }
            let (Ok(wi), Ok(oi)) = (usize::try_from(who), usize::try_from(o_who)) else { continue };
            if wi >= 8 || oi >= 8 {
                continue;
            }
            let o_diplo = ld_i32(&slots[o], LD_DIPLOS + wi * 4);
            let my_diplo = match who_u {
                Some(w) => ld_i32(&slots[w], LD_DIPLOS + oi * 4),
                None => continue, // Leaders[who] body unavailable
            };
            if (o_diplo != 2 || my_diplo != 2) && slots[o].flags & 0x20000 != 0 {
                slots[slot].flags |= 0x40000;
            }
        }

        // 3.
        gather(ctx, slots, slot, effects);

        // 4./5. stats flags.
        if slots[slot].flags & 0x8000000 != 0 {
            slots[slot].flags &= !0x8000000;
            effects.push(format!("Leader[{slot}] who {who}: calc_wall_stats 0x006CF7C0 due: UNTRANSCRIBED (flag cleared)"));
        }
        if slots[slot].flags & 0x4000000 != 0 {
            slots[slot].flags &= !0x4000000;
            effects.push(format!("Leader[{slot}] who {who}: calc_unit_stats 0x006CF970 due: UNTRANSCRIBED (flag cleared)"));
        }

        // 6. process_elimination gate.
        if ctx.info_byte(GI_ELIMINATION) == 1 && ld_i32(&slots[slot], LD_LOST_CAPITAL_TIMER) != 0 {
            effects.push(format!(
                "Leader[{slot}] who {who}: process_elimination 0x006B8A20 body due (lost_capital_timer != 0): UNTRANSCRIBED"
            ));
        }

        // 7. timer ticks.
        let d = ctx.constant(C_TIMER_DIVISOR);
        if d != 0 && frame % d == 0 {
            for (stamp, timer, name) in [
                (LD_LOST_CAPITAL_STAMP, LD_LOST_CAPITAL_TIMER, "lost_capital_stamp"),
                (LD_POPWIN_STAMP, LD_POPWIN_TIMER, "popwin_stamp"),
                (LD_WONDERWIN_STAMP, LD_WONDERWIN_TIMER, "wonderwin_stamp"),
            ] {
                let s = ld_i32(&slots[slot], stamp);
                if s < 0 && ld_i32(&slots[slot], timer) == 0 {
                    ld_put_i32(&mut slots[slot], stamp, s + 1);
                    effects.push(format!("Leader[{slot}] who {who}: {name} {s} -> {} (frame % {d} == 0)", s + 1));
                }
            }
        }

        // 8. taunt gate.
        if frame != 0 {
            for i in 0..8 {
                if ld_i32(&slots[slot], LD_INCOMING_TAUNT_FRAME + i * 4) == frame {
                    effects.push(format!(
                        "Leader[{slot}] who {who}: process_taunt({}, {}) 0x006B8CC0 due: UNTRANSCRIBED",
                        ld_i32(&slots[slot], LD_INCOMING_TAUNT + i * 4),
                        ld_i32(&slots[slot], LD_INCOMING_TAUNT_WHO + i * 4)
                    ));
                }
            }
        }

        // 9.
        slots[slot].flags &= !0x80000;
    }
}

/// `Leader::gather` 0x006CE280.
fn gather(ctx: &Ctx, slots: &mut [Leader], slot: usize, effects: &mut Vec<String>) {
    let frame = ctx.frame;
    let who = ld_i32(&slots[slot], LD_WHO);

    // 1. calc_gather gate (0x006CEEF4..0x006CEF27, 0x006CF788..0x006CF7A4).
    let due = if slots[slot].flags & 0x2000000 == 0 {
        frame >= ld_i32(&slots[slot], LD_GATHER_STAMP).wrapping_add(300)
            && frame.wrapping_add(who.wrapping_mul(8)) % 256 == 0
    } else {
        frame == 0 || who.wrapping_add(frame) % 8 == 0
    };
    if due {
        effects.push(format!(
            "Leader[{slot}] who {who}: calc_gather 0x006CEEE0 body due (frame {frame}): UNTRANSCRIBED (resources/gather_stamp untouched)"
        ));
    }

    // 2. rare = rare_owned | rare_conquest.
    {
        let l = &slots[slot];
        let n = l.rare_owned.data.len();
        let mut or: Vec<u8> = l.rare_owned.data.clone();
        for (i, b) in or.iter_mut().enumerate() {
            *b |= l.rare_conquest.data.get(i).copied().unwrap_or(0);
        }
        // operator!= compares `rare.size` payload bytes of rare vs tmp.
        let differs = l.rare.data.iter().zip(or.iter().chain(std::iter::repeat(&0))).any(|(a, b)| a != b);
        if differs {
            let l = &mut slots[slot];
            l.rare.bits = l.rare_owned.bits;
            l.rare.size = n as i32;
            l.rare.data = or;
            l.flags |= 0xc000000;
            effects.push(format!("Leader[{slot}] who {who}: rare = rare_owned | rare_conquest; leader_flags |= 0xc000000"));
        }
    }

    // 3. support[0..6] = 0.
    for g in 0..NUM_GOODS {
        enc_put_i32(&mut slots[slot], g * ENC_PER_GOOD + ENC_SUPPORT, 0);
    }

    // 4.
    calc_resource_caps(ctx, slots, slot, effects);
    // 5.
    do_gather(ctx, slots, slot, effects);
}

/// `Leader::calc_resource_caps` 0x006CE900.
fn calc_resource_caps(ctx: &Ctx, slots: &mut [Leader], slot: usize, effects: &mut Vec<String>) {
    let who = ld_i32(&slots[slot], LD_WHO);
    let who_slot = usize::try_from(who).ok().filter(|&w| w < slots.len() && readable(&slots[w]));
    let mut caps: Vec<Option<i32>> = vec![None; 7];
    {
        let l = &slots[slot];
        let ev = Eval { ctx, l };
        let epoch2 = enc_i32(l, ENC_EPOCH + 2);
        let rare_bit = who_slot.map(|w| {
            let lw = &slots[w];
            lw.rare.data.get(2).is_some_and(|b| b & 0x40 != 0) || lw.rare_conquest.data.get(2).is_some_and(|b| b & 0x40 != 0)
        });
        let pct = |cap: i32, c: i32| -> i32 { (c + 100).wrapping_mul(cap) / 100 };
        for g in 0..7usize {
            let r: Option<i32> = (|| {
                let mut cap: i32;
                if g == 3 {
                    cap = 999;
                } else {
                    let e = usize::try_from(epoch2).ok()?;
                    cap = ctx.constant(C_CAP_BY_EPOCH + e * 4);
                    if ev.has_tribe_bonus(0xb)? {
                        cap = pct(cap, ctx.constant(0x6d0));
                    }
                    match g {
                        2 => {
                            let c = ctx.constant(0x598);
                            if c != 0 && ev.has_tribe_bonus(2)? {
                                cap = pct(cap, c);
                            }
                        }
                        1 => {
                            let c = ctx.constant(0x6cc);
                            if c != 0 && ev.has_tribe_bonus(10)? {
                                cap = pct(cap, c);
                            }
                        }
                        0 => {
                            let c = ctx.constant(0x654);
                            if c != 0 && ev.has_tribe_bonus(7)? {
                                cap = pct(cap, c);
                            }
                        }
                        _ => {}
                    }
                    if rare_bit? {
                        cap = pct(cap, ctx.constant(0x92c));
                    }
                    if (g == 0 || g == 2) && ev.has_wonder(0x20e)? != 0 {
                        cap = cap.wrapping_add(ctx.constant(0x428));
                    }
                    if g == 1 || g == 2 {
                        if ev.has_wonder(0x20f)? != 0 {
                            cap = cap.wrapping_add(ctx.constant(0x444));
                        }
                        if g == 2 && ev.has_wonder(0x21b)? != 0 {
                            cap = cap.wrapping_add(ctx.constant(0x500));
                        }
                    } else if g == 5 {
                        if ev.has_wonder(0x21c)? != 0 {
                            cap = cap.wrapping_add(ctx.constant(0x544));
                        }
                        if ev.has_wonder(0x21a)? != 0 {
                            cap = cap.wrapping_add(ctx.constant(0x50c));
                        }
                    } else if g < 6 {
                        if ev.has_wonder(0x21a)? != 0 {
                            cap = cap.wrapping_add(ctx.constant(0x50c));
                        }
                        if g == 4 && ev.has_wonder(0x217)? != 0 {
                            cap = cap.wrapping_add(ctx.constant(0x4d4));
                        }
                    }
                    if ev.has_preq(0x31f, 0)? {
                        cap = cap.wrapping_add(ctx.constant(0x9a8));
                    } else if ev.has_preq(0x31e, 0)? {
                        cap = cap.wrapping_add(ctx.constant(0x9a4));
                    } else if ev.has_preq(0x31d, 0)? {
                        cap = cap.wrapping_add(ctx.constant(0x9a0));
                    }
                    cap = cap.wrapping_add(ld_i32(l, LD_BONUS_CAP + g * 4));
                    cap = cap.clamp(0, 999);
                }
                if ev.has_preq(0x2b7, 0)? {
                    cap = 999;
                }
                Some(cap << 4)
            })();
            caps[g] = r;
        }
    }
    let mut changed = Vec::new();
    for (g, c) in caps.iter().enumerate() {
        let idx = if g < NUM_GOODS { g * ENC_PER_GOOD + ENC_RESOURCE_CAP } else { ENC_RESOURCE_CAP6 };
        match c {
            Some(v) => {
                let old = enc_i32(&slots[slot], idx);
                if old != *v {
                    changed.push(format!("resource_cap[{g}] {old} -> {v}"));
                }
                enc_put_i32(&mut slots[slot], idx, *v);
            }
            None => effects.push(format!(
                "Leader[{slot}] who {who}: calc_resource_caps good {g} needs unserialized state; resource_cap untouched"
            )),
        }
    }
    if !changed.is_empty() {
        effects.push(format!("Leader[{slot}] who {who}: {}", changed.join(", ")));
    }
}

/// `Leader::do_gather` 0x006CE450.
fn do_gather(ctx: &Ctx, slots: &mut [Leader], slot: usize, effects: &mut Vec<String>) {
    let frame = ctx.frame;
    let who = ld_i32(&slots[slot], LD_WHO);
    let Some(who_slot) = usize::try_from(who).ok().filter(|&w| w < slots.len() && readable(&slots[w])) else {
        effects.push(format!("Leader[{slot}] who {who}: Leaders[who] not serialized; do_gather skipped"));
        return;
    };
    let infinite = ctx.info_byte(GI_STARTING_RESOURCES) == 8;
    let unit = ctx.constant(C_GATHER_RATE).wrapping_mul(16);
    let mut log = Vec::new();

    for g in 0..NUM_GOODS {
        let base = g * ENC_PER_GOOD;
        // Availability and all read-only queries on an immutable snapshot.
        let avail = {
            let ev = Eval { ctx, l: &slots[slot] };
            ev.type_avail_good(g as i32)
        };
        match avail {
            None => {
                effects.push(format!("Leader[{slot}] who {who}: type_avail({g}) needs unserialized state; good untouched"));
                continue;
            }
            Some(0) => continue,
            Some(_) => {}
        }
        if infinite {
            enc_put_i32(&mut slots[who_slot], base + ENC_BUCKET, 0x1269f);
            ld_put_i32(&mut slots[who_slot], LD_ESCROW + g * 4, 0);
            log.push(format!("good {g}: infinite resources (bucket = 0x1269f, escrow = 0)"));
            continue;
        }

        let l = &slots[slot];
        let mut inc = enc_i32(l, base + ENC_RESOURCES)
            .wrapping_sub(enc_i32(l, base + ENC_SUPPORT))
            .wrapping_add(ld_i32(l, LD_BASE_RATE + g * 4));
        if inc < 0 {
            enc_put_i32(&mut slots[slot], base + ENC_INCOME, inc);
            enc_put_i32(&mut slots[slot], base + ENC_OVER_CAP, 0);
            log.push(format!("good {g}: income {inc} < 0, over_cap = 0"));
            continue;
        }
        let cap = enc_i32(l, base + ENC_RESOURCE_CAP);
        let over_cap = if cap < inc {
            // UI feedback (MessageWin/SoundGlobal) when over_cap == 0 && who == Console::who: no walked state.
            inc = cap;
            (cap > 0x3e6f) as i32 + 1
        } else {
            0
        };

        // Commerce bonus (tribe bonus 0x16).
        let bonus_path: Option<Option<i32>> = (|| {
            let ev = Eval { ctx, l };
            if g == 3 || !ev.has_tribe_bonus(0x16)? {
                return Some(None);
            }
            let starting = ctx.starting(g);
            let base_amount = if ctx.info_byte(GI_GAME_RULES) == 8 {
                let team = ev.get_team()?;
                if team != 0 {
                    // LAB_006ce675 with a non-zero team: plain starting[g]
                    // (the conquest multiplier branch below still applies).
                    if ctx.sem(2) & 2 != 0 && l.flags2 & 0x80 != 0 {
                        starting.wrapping_mul(ctx.constant(C_CONQUEST_START_MULT))
                    } else {
                        starting
                    }
                } else {
                    (ctx.info_byte(GI_STARTING_RESOURCES2) as i32 + 1).wrapping_mul(starting)
                }
            } else if ctx.sem(2) & 2 != 0 && l.flags2 & 0x80 != 0 {
                starting.wrapping_mul(ctx.constant(C_CONQUEST_START_MULT))
            } else {
                starting
            };
            let over = enc_i32(&slots[who_slot], base + ENC_BUCKET).wrapping_sub(base_amount);
            let mut v = inc;
            if over > 0 {
                v = inc.wrapping_add((ctx.constant(C_COMMERCE_OVER_PCT).wrapping_mul(over) / 100).wrapping_mul(16));
                let lim = ctx.constant(C_COMMERCE_OVER_CAP).wrapping_mul(16).wrapping_add(cap);
                if lim < v {
                    v = lim;
                }
            }
            Some(Some(v.min(0x3e70)))
        })();
        match bonus_path {
            None => {
                effects.push(format!("Leader[{slot}] who {who}: commerce-bonus path for good {g} needs unserialized state; good untouched"));
                continue;
            }
            Some(Some(v)) => inc = v,
            Some(None) => {}
        }
        // income[g] stored here (0x006CE71C), before the local-only scaling.
        let stored_income = inc;

        let handicap = match (Eval { ctx, l }).get_gather_handicap() {
            Some(h) => h,
            None => {
                effects.push(format!("Leader[{slot}] who {who}: get_gather_handicap needs unserialized state; good {g} untouched"));
                continue;
            }
        };
        if handicap != 0 {
            inc = (handicap + 100).wrapping_mul(inc) / 100;
        }
        if g == 3 {
            let tc = ctx.info_byte(GI_TECH_COST);
            if tc > 4 {
                inc = if tc < 7 { inc.wrapping_mul(3) / 4 } else { inc / 2 };
            }
        }
        if ctx.info_flags() & 2 != 0 || ctx.info_byte(GI_GAME_RULES) == 9 {
            inc = inc.wrapping_mul(3) / 2;
        }
        // GameAccess::ai_speed > 1 → inc *= ai_speed: cheat global, not serialized (treated as 1).

        // Accumulate (0x006CE7E9..0x006CE88B).
        let mut whole = if unit != 0 { inc / unit } else { 0 };
        let mut leftover = enc_i32(l, base + ENC_LEFTOVER).wrapping_add(if unit != 0 { inc % unit } else { 0 });
        if unit > 0 {
            while leftover >= unit {
                whole += 1;
                leftover -= unit;
            }
        }
        let l = &mut slots[slot];
        enc_put_i32(l, base + ENC_OVER_CAP, over_cap);
        enc_put_i32(l, base + ENC_INCOME, stored_income);
        enc_put_i32(l, base + ENC_LEFTOVER, leftover);
        let collected = ld_i32(l, LD_COLLECTED + g * 4).wrapping_add(whole);
        ld_put_i32(l, LD_COLLECTED + g * 4, collected);
        let escrow_rate = ld_i32(l, LD_ESCROW_RATE + g * 4);
        let lw = &mut slots[who_slot];
        let bucket = enc_i32(lw, base + ENC_BUCKET).wrapping_add(whole);
        enc_put_i32(lw, base + ENC_BUCKET, bucket);
        let mut esc_note = String::new();
        if lw.flags & 0xc != 4 {
            let x = escrow_rate.wrapping_mul(inc);
            let u = ctx.constant(C_GATHER_RATE).wrapping_mul(0x640);
            if u != 0 {
                let mut q = x / u;
                let r = x % u;
                if r != 0 {
                    let d = ((u.wrapping_add(r / 2)) / r).max(2);
                    if frame % d == 0 {
                        q += 1;
                    }
                }
                let e = ld_i32(lw, LD_ESCROW + g * 4).wrapping_add(q);
                ld_put_i32(lw, LD_ESCROW + g * 4, e);
                if q != 0 {
                    esc_note = format!(", escrow += {q}");
                }
            }
        }
        if whole != 0 || inc != 0 {
            log.push(format!(
                "good {g}: income {stored_income} (rate {inc}) leftover -> {leftover}, bucket += {whole} -> {bucket}, collected -> {collected}{esc_note}"
            ));
        }
    }
    if !log.is_empty() {
        effects.push(format!("Leader[{slot}] who {who}: {}", log.join("; ")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sections::{GameInfo, TribeRec, TypeRec};
    use std::path::{Path, PathBuf};

    fn capture_dirs() -> Vec<PathBuf> {
        let Ok(root) = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(root.join("schema/live/frame-pairs")) {
            for e in rd.flatten() {
                let d = e.path();
                if d.is_dir() && d.join("manifest.json").is_file() {
                    out.push(d);
                }
            }
        }
        out.sort();
        out
    }

    fn manifest_steps(dir: &Path) -> Vec<(i64, String)> {
        let text = std::fs::read_to_string(dir.join("manifest.json")).unwrap();
        let mut out = Vec::new();
        for seg in text.split("\"frame\":").skip(1) {
            let Some(frame) = seg
                .trim_start()
                .split(|c: char| !c.is_ascii_digit())
                .next()
                .and_then(|t| t.parse::<i64>().ok())
            else {
                continue;
            };
            let save = seg
                .split("\"save_name\":")
                .nth(1)
                .and_then(|s| s.split('"').nth(1))
                .unwrap_or_default()
                .to_string();
            out.push((frame, save));
        }
        out
    }

    /// Synthetic save: one leader (who 0, tribe 1) with Food/Timber rules
    /// (preq −1, obs −2, all tribes), Knowledge with a Classical-Age preq.
    fn synth(frame: i32, income: [i32; 6], leftover: [i32; 6]) -> Save {
        let mut s = Save::default();
        s.game.scalars = vec![0u8; 404];
        put_i32(&mut s.game.scalars, FRAME, frame);
        for g in 0..6 {
            put_i32(&mut s.game.scalars, GAME_STARTING + g * 4, 200);
        }
        s.game.sem_ptr = vec![8, 1, 0, 0];
        let mut info = GameInfo::default();
        info.head = vec![0u8; 24];
        info.settings = vec![0u8; 30];
        info.settings[GI_STARTING_TOWN - GI_SETTINGS_BASE] = 2;
        info.settings[GI_STARTING_RESOURCES - GI_SETTINGS_BASE] = 1;
        info.settings[GI_DIFFICULTY - GI_SETTINGS_BASE] = 3;
        info.settings[GI_ENDING_TECHNOLOGY - GI_SETTINGS_BASE] = 7;
        s.game.info = info;
        // Rules: 806 types, constants, 24 tribes.
        let mut rules = Rules::default();
        rules.types = (0..806)
            .map(|t| {
                let mut r = TypeRec::default();
                r.head = vec![0u8; 90];
                put_i32(&mut r.head, 0xc, -1); // tribe_mask all
                for i in 0..3 {
                    put_i32(&mut r.head, 0x2c + i * 4, -1);
                }
                put_i32(&mut r.head, 0x48, -2); // obs
                if t == 3 || t == 4 {
                    put_i32(&mut r.head, 0x2c, 0x220);
                }
                if is_tech_type(t) {
                    r.ext = vec![0u8; 27];
                    put_i32(&mut r.ext, 0, (t - 0x21f).max(0));
                }
                r
            })
            .collect();
        rules.constants = vec![0u8; 0xd40];
        put_i32(&mut rules.constants, C_GATHER_RATE, 450);
        put_i32(&mut rules.constants, C_CAP_BY_EPOCH, 70);
        put_i32(&mut rules.constants, C_CAP_BY_EPOCH + 4, 100);
        put_i32(&mut rules.constants, 0x6cc, 10);
        put_i32(&mut rules.constants, C_TIMER_DIVISOR, 5);
        rules.tribes = (0..24)
            .map(|i| {
                let mut t = TribeRec::default();
                t.a = vec![0u8; 24];
                put_i32(&mut t.a, 0, i);
                t.b = vec![0u8; 1408];
                t
            })
            .collect();
        s.rules_tail.rules = rules;
        let mut l = Leader::default();
        l.flags = 0x13;
        l.body = vec![0u8; 0x6922];
        ld_put_i32(&mut l, LD_WHO, 0);
        ld_put_i32(&mut l, LD_TRIBE, 1);
        ld_put_i32(&mut l, LD_CITY_NUM, 1);
        ld_put_i32(&mut l, LD_MULTI_DIFF, 3);
        l.data_encrypted = vec![0u8; ENC_LEN];
        for g in 0..6 {
            enc_put_i32(&mut l, g * ENC_PER_GOOD + ENC_RESOURCES, income[g]);
            enc_put_i32(&mut l, g * ENC_PER_GOOD + ENC_LEFTOVER, leftover[g]);
            enc_put_i32(&mut l, g * ENC_PER_GOOD + ENC_RESOURCE_CAP, 1120);
            enc_put_i32(&mut l, g * ENC_PER_GOOD + ENC_SUPPORT, 7);
        }
        enc_put_i32(&mut l, ENC_RESOURCE_CAP6, 1120);
        for bm in [&mut l.tech, &mut l.rare, &mut l.rare_owned, &mut l.rare_conquest, &mut l.conquest_wonders, &mut l.conquest_racial_powers] {
            bm.bits = 44;
            bm.size = 6;
            bm.data = vec![0u8; 101];
        }
        s.leaders.slots = vec![l];
        s
    }

    #[test]
    fn leftover_rolls_into_bucket_and_collected() {
        let mut s = synth(11, [640, 320, 0, 0, 0, 0], [6560, 100, 0, 0, 0, 0]);
        let mut fx = Vec::new();
        run(&mut s, &mut fx);
        let l = &s.leaders.slots[0];
        // Food: 6560 + 640 = 7200 → one unit.
        assert_eq!(enc_i32(l, ENC_LEFTOVER), 0);
        assert_eq!(enc_i32(l, ENC_BUCKET), 1);
        assert_eq!(ld_i32(l, LD_COLLECTED), 1);
        assert_eq!(enc_i32(l, ENC_INCOME), 640);
        assert_eq!(enc_i32(l, ENC_OVER_CAP), 0);
        // Timber: no rollover.
        assert_eq!(enc_i32(l, ENC_PER_GOOD + ENC_LEFTOVER), 420);
        assert_eq!(enc_i32(l, ENC_PER_GOOD + ENC_BUCKET), 0);
        // support zeroed everywhere.
        for g in 0..6 {
            assert_eq!(enc_i32(l, g * ENC_PER_GOOD + ENC_SUPPORT), 0);
        }
        // Knowledge/Metal need the Classical Age tech bit: untouched (leftover stays 0, income 0).
        assert_eq!(enc_i32(l, 3 * ENC_PER_GOOD + ENC_INCOME), 0);
        // Caps: tribe 1 has bonus 1 → no cap bonus; 70 << 4 = 1120, knowledge 999 << 4.
        assert_eq!(enc_i32(l, 3 * ENC_PER_GOOD + ENC_RESOURCE_CAP), 15984);
        assert_eq!(enc_i32(l, ENC_RESOURCE_CAP), 1120);
        assert_eq!(enc_i32(l, ENC_RESOURCE_CAP6), 1120);
        assert!(fx.iter().any(|e| e.contains("bucket += 1")), "{fx:?}");
        assert!(!fx.iter().any(|e| e.contains("UNTRANSCRIBED") || e.contains("unserialized")), "{fx:?}");
    }

    #[test]
    fn tribe_bonus_scales_timber_cap_and_knowledge_follows_age_bit() {
        let mut s = synth(11, [0; 6], [0; 6]);
        ld_put_i32(&mut s.leaders.slots[0], LD_TRIBE, 10);
        let mut fx = Vec::new();
        run(&mut s, &mut fx);
        let l = &s.leaders.slots[0];
        assert_eq!(enc_i32(l, ENC_PER_GOOD + ENC_RESOURCE_CAP), 77 << 4);
        assert_eq!(enc_i32(l, ENC_RESOURCE_CAP), 70 << 4);
        // Grant Classical Age: Knowledge becomes available; its income is stored.
        let mut s = synth(11, [0, 0, 0, 160, 0, 0], [0; 6]);
        s.leaders.slots[0].tech.data[0x220 >> 3] |= 1 << (0x220 & 7);
        let mut fx = Vec::new();
        run(&mut s, &mut fx);
        let l = &s.leaders.slots[0];
        assert_eq!(enc_i32(l, 3 * ENC_PER_GOOD + ENC_INCOME), 160);
        assert_eq!(enc_i32(l, 3 * ENC_PER_GOOD + ENC_LEFTOVER), 160);
    }

    #[test]
    fn over_cap_clamps_income() {
        let mut s = synth(11, [5000, 0, 0, 0, 0, 0], [0; 6]);
        let mut fx = Vec::new();
        run(&mut s, &mut fx);
        let l = &s.leaders.slots[0];
        assert_eq!(enc_i32(l, ENC_INCOME), 1120);
        assert_eq!(enc_i32(l, ENC_OVER_CAP), 1);
        assert_eq!(enc_i32(l, ENC_LEFTOVER), 1120);
    }

    #[test]
    fn timers_tick_on_divisor_frames() {
        let mut s = synth(15, [0; 6], [0; 6]);
        ld_put_i32(&mut s.leaders.slots[0], LD_POPWIN_STAMP, -3);
        ld_put_i32(&mut s.leaders.slots[0], LD_LOST_CAPITAL_STAMP, -3);
        ld_put_i32(&mut s.leaders.slots[0], LD_LOST_CAPITAL_TIMER, 1);
        ld_put_i32(&mut s.leaders.slots[0], LD_POP_ISSUES, 9);
        s.leaders.slots[0].flags |= 0x40000 | 0x80000;
        let mut fx = Vec::new();
        run(&mut s, &mut fx);
        let l = &s.leaders.slots[0];
        assert_eq!(ld_i32(l, LD_POPWIN_STAMP), -2);
        assert_eq!(ld_i32(l, LD_LOST_CAPITAL_STAMP), -3, "timer != 0 blocks");
        assert_eq!(ld_i32(l, LD_POP_ISSUES), 0);
        assert_eq!(l.flags & (0x40000 | 0x80000), 0);
        let mut s = synth(16, [0; 6], [0; 6]);
        ld_put_i32(&mut s.leaders.slots[0], LD_POPWIN_STAMP, -3);
        run(&mut s, &mut Vec::new());
        assert_eq!(ld_i32(&s.leaders.slots[0], LD_POPWIN_STAMP), -3, "16 % 5 != 0");
    }

    /// Oracle: for every stride-1 pair, after `run` on retail N the
    /// `data_encrypted` block and `collected[6]` of every Leader equal
    /// retail N+1, and no byte that retail left unchanged in the Leader
    /// records (flags, body, rare masks, data_encrypted) was changed by us.
    #[test]
    fn leader_economy_matches_retail_next_frame() {
        let dirs = capture_dirs();
        if dirs.is_empty() {
            eprintln!("no captures; skipping");
            return;
        }
        let mut pairs = 0;
        let mut rollovers = 0;
        let mut gather_skipped = 0;
        for dir in dirs {
            let steps = manifest_steps(&dir);
            for k in 0..steps.len().saturating_sub(1) {
                let (fa, sa) = &steps[k];
                let (fb, sb) = &steps[k + 1];
                if fb - fa != 1 {
                    continue;
                }
                let raw_a = crate::container::load_svx(&dir.join(format!("{sa}.svx"))).unwrap();
                let raw_b = crate::container::load_svx(&dir.join(format!("{sb}.svx"))).unwrap();
                let a = crate::load(&raw_a).unwrap().state;
                let b = crate::load(&raw_b).unwrap().state;
                let mut ours = a.clone();
                let mut fx = Vec::new();
                run(&mut ours, &mut fx);
                for (li, ((la, lo), lb)) in a.leaders.slots.iter().zip(ours.leaders.slots.iter()).zip(b.leaders.slots.iter()).enumerate() {
                    if la.flags & 1 == 0 {
                        continue;
                    }
                    // A leader whose calc_gather gate passed had `resources`
                    // recomputed by the untranscribed body; its economy
                    // values are out of scope for this pair (introduced
                    // check below still applies).
                    let gather_due = fx.iter().any(|e| e.starts_with(&format!("Leader[{li}] who")) && e.contains("calc_gather 0x006CEEE0 body due"));
                    if gather_due {
                        gather_skipped += 1;
                    } else {
                        for idx in 0..62 {
                            let (o, r) = (enc_i32(lo, idx), enc_i32(lb, idx));
                            if idx < 54 && idx % ENC_PER_GOOD == ENC_BUCKET {
                                // Other steps (production, market) spend from
                                // the stockpile within the same frame; our
                                // income credit must be ≥ retail's result.
                                assert!(
                                    o == r || r < o,
                                    "{} f{fa}->f{fb} Leader[{li}] bucket[{}] ours {o} retail {r}\n{fx:#?}",
                                    dir.display(),
                                    idx / ENC_PER_GOOD
                                );
                            } else {
                                assert_eq!(
                                    o, r,
                                    "{} f{fa}->f{fb} Leader[{li}] data_encrypted[{idx}] (good {} field {})\n{fx:#?}",
                                    dir.display(),
                                    idx / ENC_PER_GOOD,
                                    idx % ENC_PER_GOOD
                                );
                            }
                        }
                        let coll = |l: &Leader| (0..6).map(|g| ld_i32(l, LD_COLLECTED + g * 4)).collect::<Vec<_>>();
                        assert_eq!(coll(lo), coll(lb), "{} f{fa}->f{fb} Leader[{li}] collected", dir.display());
                        if coll(la) != coll(lb) {
                            rollovers += 1;
                        }
                    }
                    for (i, (x, y)) in la.data_encrypted.iter().zip(lb.data_encrypted.iter()).enumerate() {
                        if x == y {
                            assert_eq!(lo.data_encrypted[i], *x, "{} f{fa}->f{fb} Leader[{li}] introduced data_encrypted[{}]", dir.display(), i / 4);
                        }
                    }
                    // Introduced check over the Leader record images.
                    for (i, (x, y)) in la.body.iter().zip(lb.body.iter()).enumerate() {
                        if x == y {
                            assert_eq!(lo.body[i], *x, "{} f{fa}->f{fb} Leader[{li}] introduced body img+{:#x}", dir.display(), i + 8);
                        }
                    }
                    if la.flags == lb.flags {
                        assert_eq!(lo.flags, la.flags, "{} f{fa}->f{fb} Leader[{li}] introduced flags", dir.display());
                    }
                    if la.rare.data == lb.rare.data {
                        assert_eq!(lo.rare.data, la.rare.data, "{} f{fa}->f{fb} Leader[{li}] introduced rare", dir.display());
                    }
                }
                pairs += 1;
            }
        }
        eprintln!("{pairs} stride-1 pairs checked, {rollovers} bucket rollovers reproduced, {gather_skipped} leader-pairs skipped (calc_gather body due)");
        assert!(pairs > 0);
    }
}
