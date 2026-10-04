//! `ObjectData::train_time` 0x006508c0 (PDB name; the lead's working name
//! is `construct_time` — `BuildData::construct_time` is a different,
//! wall-construction routine) — the total job time a `Build::do_queue`
//! 0x0061e410 row clamps `QueueItem.job_counter` against, with every
//! tech / government / tribe / rare / wonder / handicap modifier. Leaf
//! module: exposes free functions for the owning traversal
//! (`build_process.rs`) to call; never edits sibling modules.
//!
//! Also transcribed here: `ObjectData::has_general` 0x00646b00,
//! `LeaderData::check_population` 0x006e1330, `LeaderData::get_first_library`
//! 0x006db6c0, `LeaderData::get_building_cities` 0x006e06f0 (+
//! `CityData::has_building` 0x00739390), `BuildData::is_unassimilated`
//! 0x0062d470, `LeaderData::is_team` 0x006ebd30 / `is_ally` 0x006edb50 /
//! `get_player` 0x006ec0f0, the three `get_*_speed_upgrade` 0x006da800 /
//! 0x006da850 / 0x006da8a0, `TypeData::time` 0x00663f20, `TypeData::
//! research_time` 0x006639a0, `UnitTypeData::research_time` 0x0061d760,
//! `TypeData::count_discovered` 0x00663f80, `ObjectTypeData::is` 0x0065f7d0
//! / `is_slow` 0x00661ae0, and the `do_queue` arithmetic tail
//! ([`complete_queue_step`]).
//!
//! Every function returns `Option`: `None` means a branch needed state the
//! save does not serialize (documented at each site) — never a guess.
//!
//! # Globals (retail address → serialized field)
//!
//! ```text
//!   [0x00c061e0] Leaders     LeaderData[9] stride 0x6eec  Save.leaders.slots[who]
//!                              body = image +0x08..+0x692a; tech BitMask = +0x6c18
//!                              rare/rare_conquest BitMask = +0x6da4/+0x6dcc
//!                              data_encrypted (plaintext) = *(+0x6eb8)
//!   [0x00c061e4] Constants   0xd40 B                       Save.rules_tail.rules.constants
//!   [0x00c061e8] Game        frame +0x550, num_players +0x69c, num_nations
//!                              +0x6a0, semaphore.ptr +0x820, GameInfo settings
//!                              +0x24.. (team_style +0x24, difficulty +0x2b,
//!                              tech_cost +0x2f), players +0x74 stride 0x8c
//!   [0x00c061d4] Cities      per-owner PtrArray (+who*0x1c+0x10) Save.cities
//!   [0x00e85ddc] types.list  TypeData*[806]               Save.rules_tail.rules.types
//!   [0x00c0a274] unittypes   UnitType*  (same index)       ... types[i].ext
//!   [0x00c0aabc] techtypes   TechType*  (same index)       ... types[i].ext
//!   [0x00c0ab84] objects     per-owner lists               Save.objects.lists
//! ```

#![allow(dead_code)]

use crate::prim::BitMask;
use crate::sections::{Build, Obj, Save, TypeRec};
use crate::tick::StepStatus;

/// Leaf helper module; it has no `do_frame` step of its own.
pub const STATUS: StepStatus = StepStatus::Stub;

// ---------------------------------------------------------------------------
// Type index ranges (TypeData virtuals, re/decomp-all/004705b0..00470870)
// ---------------------------------------------------------------------------

fn is_unit_type(t: i32) -> bool {
    (0x32..=0x19d).contains(&t)
}
fn is_building_type(t: i32) -> bool {
    (0x19e..=0x21e).contains(&t)
}
fn is_wonder_type(t: i32) -> bool {
    (0x20e..=0x21e).contains(&t)
}
fn is_age_type(t: i32) -> bool {
    (0x220..=0x226).contains(&t)
}
fn is_epoch_type(t: i32) -> bool {
    (0x227..=0x242).contains(&t)
}
fn is_tech_type(t: i32) -> bool {
    (0x220..=0x274).contains(&t)
}
fn is_gov_type(t: i32) -> bool {
    (0x26f..=0x274).contains(&t)
}
fn is_spell_type(t: i32) -> bool {
    (0x275..=0x2ab).contains(&t)
}
fn is_bonus_type(t: i32) -> bool {
    (0x2ac..=0x325).contains(&t)
}
/// `TypeData::is_merchant` 0x0042dbd0.
fn is_merchant(t: i32) -> bool {
    t == 0x3d || t == 0x3e || t == 400
}
/// Slots whose `TypeData::is` (vtable +0x60) resolves to `ObjectTypeData::is`
/// 0x0065f7d0 (Good/Unit/Build/Object records carry `is_list`); every other
/// kind uses `TypeData::is` 0x004771c0 (exact match).
fn has_object_vtable(t: i32) -> bool {
    (0..=543).contains(&t)
}

fn rd_i32(buf: &[u8], off: usize) -> Option<i32> {
    buf.get(off..off + 4).map(|b| i32::from_le_bytes(b.try_into().unwrap()))
}
fn rd_u16(buf: &[u8], off: usize) -> Option<u16> {
    buf.get(off..off + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}
fn bit(mask: &BitMask, b: i32) -> bool {
    if b < 0 {
        return false;
    }
    let b = b as usize;
    mask.data.get(b >> 3).is_some_and(|x| x & (1 << (b & 7)) != 0)
}

/// Signed `x * 3 / 4` as retail emits it: `(x*3 + ((x*3 >> 31) & 3)) >> 2`.
fn mul3_div4(x: i32) -> i32 {
    let y = x.wrapping_mul(3);
    (y.wrapping_add((y >> 31) & 3)) >> 2
}
/// Signed `x * 5 / 4`, same idiom.
fn mul5_div4(x: i32) -> i32 {
    let y = x.wrapping_mul(5);
    (y.wrapping_add((y >> 31) & 3)) >> 2
}
/// `x * 100 / (pct + 100)` — the dominant "percent faster" form. A zero
/// divisor would be a retail `idiv` fault; surfaced as `None`.
fn faster(x: i32, pct: i32) -> Option<i32> {
    x.wrapping_mul(100).checked_div(pct.wrapping_add(100))
}

// ---------------------------------------------------------------------------
// Type record image reader
// ---------------------------------------------------------------------------

/// `Rules.types[idx]` addressed by retail image offset. `head` is
/// image[4..94); `obj_mid` image[0x1e4..0x27c); `ext` is the per-kind tail:
/// unit `[0x2b4..0x2cc) ++ [0x2d4..0x2dc) ++ [0x2dc..0x2e0) ++ [0x2e0..0x5d4)`
/// (note the 8-byte hole at 0x2cc), build `[0x2b4..0x2e5)`, good
/// `[0x2b4..0x2f8)`, tech/spell `[0x1c8..)`.
struct TypeImg<'a> {
    idx: i32,
    rec: &'a TypeRec,
}

impl TypeImg<'_> {
    fn i32(&self, off: usize) -> Option<i32> {
        match off {
            0x04..=0x5d => rd_i32(&self.rec.head, off - 4),
            0x1c8..=0x1e3 if is_tech_type(self.idx) || is_spell_type(self.idx) => rd_i32(&self.rec.ext, off - 0x1c8),
            0x1e4..=0x27b => rd_i32(&self.rec.obj_mid, off - 0x1e4),
            0x2b4.. if is_unit_type(self.idx) => {
                let o = match off {
                    0x2b4..=0x2cb => off - 0x2b4,
                    0x2cc..=0x2d3 => return None, // not serialized (fire_proj, second_max_range)
                    _ => off - 0x2b4 - 8,
                };
                rd_i32(&self.rec.ext, o)
            }
            0x2b4.. => rd_i32(&self.rec.ext, off - 0x2b4),
            _ => None,
        }
    }
    fn job_time(&self) -> Option<i32> {
        self.i32(0x08)
    }
    fn res_time(&self) -> Option<i32> {
        self.i32(0x0c)
    }
    fn cat(&self) -> Option<i32> {
        self.i32(0x14)
    }
    fn preq(&self, i: usize) -> Option<i32> {
        self.i32(0x30 + i * 4)
    }
    fn from(&self) -> Option<i32> {
        self.i32(0x3c)
    }
    fn where_(&self) -> Option<i32> {
        self.i32(0x40)
    }
    /// `ObjectType.obj_masks` +0x1e4 (`has_objmask` 0x00470860).
    fn obj_masks(&self) -> Option<u32> {
        self.i32(0x1e4).map(|v| v as u32)
    }
    fn attack(&self) -> Option<i32> {
        self.i32(0x1e8)
    }
    fn domain(&self) -> Option<i32> {
        self.i32(0x218)
    }
    fn x_size(&self) -> Option<i32> {
        self.i32(0x234)
    }
    fn y_size(&self) -> Option<i32> {
        self.i32(0x238)
    }
    fn graft(&self) -> Option<i32> {
        self.i32(0x25c)
    }
    /// `UnitType.unit_flags` +0x2b4.
    fn unit_flags(&self) -> Option<u32> {
        self.i32(0x2b4).map(|v| v as u32)
    }
    /// `UnitType.unit_flags2` +0x2b8 (bit 8 = `is_specialty_type`).
    fn unit_flags2(&self) -> Option<u32> {
        self.i32(0x2b8).map(|v| v as u32)
    }
    /// `UnitType.role` +0x2c8.
    fn role(&self) -> Option<u32> {
        self.i32(0x2c8).map(|v| v as u32)
    }
    /// `UnitType.research_premium_time` +0x2e4.
    fn research_premium_time(&self) -> Option<i32> {
        self.i32(0x2e4)
    }
    /// `UnitType.job_extra_time` +0x2e8.
    fn job_extra_time(&self) -> Option<i32> {
        self.i32(0x2e8)
    }
    /// `UnitType.control_cost` +0x2f0.
    fn control_cost(&self) -> Option<i32> {
        self.i32(0x2f0)
    }
    /// `BuildType.build_flags` +0x2c0.
    fn build_flags(&self) -> Option<u32> {
        self.i32(0x2c0).map(|v| v as u32)
    }
    /// `TechType.age` +0x1c8.
    fn tech_age(&self) -> Option<i32> {
        self.i32(0x1c8)
    }
    /// `is_list` (+0x280/+0x28c) / `is_strict_list` (+0x29c/+0x2a8).
    fn list(&self, strict: bool) -> impl Iterator<Item = u16> + '_ {
        let a = if strict { &self.rec.arr1 } else { &self.rec.arr0 };
        a.data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]))
    }
}

// ---------------------------------------------------------------------------
// Context: the read-only globals every function here consults
// ---------------------------------------------------------------------------

/// Read-only view of the state `train_time` and friends read.
pub struct Ctx<'a> {
    save: &'a Save,
}

impl<'a> Ctx<'a> {
    pub fn new(save: &'a Save) -> Self {
        Ctx { save }
    }

    fn ty(&self, idx: i32) -> Option<TypeImg<'_>> {
        let rec = self.save.rules_tail.rules.types.get(usize::try_from(idx).ok()?)?;
        if rec.head.len() < 90 {
            return None;
        }
        Some(TypeImg { idx, rec })
    }

    /// `Constants` i32 at image offset `off` (`[0x00c061e4]`).
    fn constant(&self, off: usize) -> Option<i32> {
        let c = &self.save.rules_tail.rules.constants;
        rd_i32(c, off).or_else(|| rd_i32(&self.save.constants, off))
    }

    /// `Game+0x550` frame.
    fn frame(&self) -> i32 {
        rd_i32(&self.save.game.scalars, 0).unwrap_or(0)
    }
    /// `Game+0x69c` num_players.
    fn num_players(&self) -> i32 {
        rd_i32(&self.save.game.scalars, 0x69c - 0x550).unwrap_or(0)
    }
    /// `Game+0x6a0` num_nations.
    fn num_nations(&self) -> i32 {
        rd_i32(&self.save.game.scalars, 0x6a0 - 0x550).unwrap_or(0)
    }
    /// `GameInfo` setting byte at `Game+0x24+i`.
    fn setting(&self, i: usize) -> u8 {
        self.save.game.info.settings.get(i).copied().unwrap_or(0)
    }
    fn team_style(&self) -> u8 {
        self.setting(0)
    }
    fn game_rules(&self) -> u8 {
        self.setting(0x2a - 0x24)
    }
    fn tech_cost(&self) -> u8 {
        self.setting(0x2f - 0x24)
    }
    fn starting_technology(&self) -> u8 {
        self.setting(0x34 - 0x24)
    }
    fn starting_technology2(&self) -> u8 {
        self.setting(0x35 - 0x24)
    }
    fn ending_technology(&self) -> u8 {
        self.setting(0x36 - 0x24)
    }
    fn info_flags(&self) -> u32 {
        rd_i32(&self.save.game.info.head, 0x14).unwrap_or(0) as u32
    }
    /// `Game.semaphore.ptr[i]` (`Game+0x820+i`).
    fn sem(&self, i: usize) -> u8 {
        self.save.game.sem_ptr.get(i).copied().unwrap_or(0)
    }
    /// `Game+0x74 + p*0x8c`: PlayerInfo `flags` (u16), `who` (+3), `team` (+4).
    /// The serialized `GameInfo.player[p].body` holds the Player image from
    /// Game+0x44 (flags at body[0x30], who body[0x33], team body[0x34]).
    fn player(&self, p: usize) -> Option<(u16, i32, i32)> {
        let pl = self.save.game.info.players.get(p)?;
        if pl.flags & 1 == 0 || pl.body.len() < 0x35 {
            return Some((pl.flags, -1, 8));
        }
        Some((pl.flags, pl.body[0x33] as i32, pl.body[0x34] as i8 as i32))
    }

    fn leader(&self, who: usize) -> Option<&crate::sections::Leader> {
        let l = self.save.leaders.slots.get(who)?;
        (l.flags & 1 != 0 && l.body.len() >= 0x6922 && l.data_encrypted.len() >= 62 * 4).then_some(l)
    }
    fn leader_flags(&self, who: usize) -> i32 {
        self.save.leaders.slots.get(who).map(|l| l.flags).unwrap_or(0)
    }
    /// LeaderData i32 at image offset `off` (body index = off − 8).
    fn ld_i32(&self, who: usize, off: usize) -> Option<i32> {
        rd_i32(&self.leader(who)?.body, off - 8)
    }
    fn ld_u16(&self, who: usize, off: usize) -> Option<u16> {
        rd_u16(&self.leader(who)?.body, off - 8)
    }
    fn ld_u8(&self, who: usize, off: usize) -> Option<u8> {
        self.leader(who)?.body.get(off - 8).copied()
    }
    /// `LeaderDataEncrypt` dword by serialized index (plaintext).
    fn enc(&self, who: usize, idx: usize) -> Option<i32> {
        rd_i32(&self.leader(who)?.data_encrypted, idx * 4)
    }
    /// `+0xe8+4i ^ 0x63187` → serialized 55+i.
    fn epoch(&self, who: usize, i: usize) -> Option<i32> {
        self.enc(who, 55 + i)
    }
    /// `+0xdc ^ 0x62766` → serialized 59.
    fn ages(&self, who: usize) -> Option<i32> {
        self.enc(who, 59)
    }
    /// `+0xe0 ^ 0x69587` → serialized 60.
    fn epochs(&self, who: usize) -> Option<i32> {
        self.enc(who, 60)
    }
    fn tribe(&self, who: usize) -> Option<i32> {
        self.ld_i32(who, 0xc)
    }
    /// `tech` BitMask (+0x6c18) bit `t`.
    fn tech_bit(&self, who: usize, t: i32) -> Option<bool> {
        Some(bit(&self.leader(who)?.tech, t))
    }
    /// `rare` (+0x6da4) or `rare_conquest` (+0x6dcc) bit `b`.
    fn rare_bit(&self, who: usize, b: i32) -> Option<bool> {
        let l = self.leader(who)?;
        Some(bit(&l.rare, b) || bit(&l.rare_conquest, b))
    }
    /// `num_units[t - 50]` (+0x5762) — read at `+0x56fe + t*2`.
    fn num_units(&self, who: usize, t: i32) -> Option<u16> {
        if !is_unit_type(t) {
            return None;
        }
        self.ld_u16(who, 0x56fe + t as usize * 2)
    }
    /// `Tribes[tribe].tribe` (+0x54) — `TribeRec.a[0..4]`.
    fn tribe_bonus_id(&self, tribe: i32) -> Option<i32> {
        let rec = self.save.rules_tail.rules.tribes.get(usize::try_from(tribe).ok()?)?;
        rd_i32(&rec.a, 0)
    }

    // --- TypeData virtuals ----------------------------------------------------

    /// `ObjectTypeData::is_slow` 0x00661ae0 (`strict == 0`: self, graft, or
    /// recursion through `from`; `strict`: unit types only, graft match unless
    /// the target's `unit_flags` carries 0x1000000).
    fn type_is_slow(&self, this: i32, what: i32, strict: bool, depth: u8) -> bool {
        if this == what {
            return true;
        }
        let Some(t) = self.ty(this) else { return false };
        if !strict {
            if what < 0 {
                return false;
            }
            if t.graft() == Some(what) {
                return true;
            }
            match t.from() {
                Some(from) if from >= 0 && depth < 16 => self.type_is_slow(from, what, false, depth + 1),
                _ => false,
            }
        } else {
            if !is_unit_type(this) {
                return false;
            }
            t.graft() == Some(what) && self.ty(what).and_then(|w| w.unit_flags()).is_some_and(|f| f & 0x1000000 == 0)
        }
    }

    /// `TypeData::is(what, strict)` through vtable +0x60: `ObjectTypeData::is`
    /// 0x0065f7d0 for object-kind records, exact match otherwise.
    fn type_is(&self, this: i32, what: i32, strict: bool) -> bool {
        if this == what {
            return true;
        }
        if !has_object_vtable(this) {
            return false;
        }
        let Some(t) = self.ty(this) else { return false };
        let mut it = t.list(strict).peekable();
        if it.peek().is_none() {
            return self.type_is_slow(this, what, strict, 0);
        }
        if !strict && what < 0 {
            return false;
        }
        it.any(|v| v as i32 == what)
    }

    /// `TypeData::time(who)` 0x00663f20: `job_time * 100`, or `res_time * 100`
    /// for a spell the leader cannot cast (`LeaderData::can_cast` 0x006e0bc0
    /// — not transcribed; spells return `None`).
    fn type_time(&self, t: i32, _who: usize) -> Option<i32> {
        if is_spell_type(t) {
            return None;
        }
        self.ty(t)?.job_time().map(|j| j.wrapping_mul(100))
    }

    /// `TypeData::research_time(who)` 0x006639a0 (`(time * Constants.
    /// research_tick_premium) >>u 8`) and the `UnitTypeData::research_time`
    /// 0x0061d760 override (`* research_premium_time >> 8`, signed).
    fn type_research_time(&self, t: i32, who: usize) -> Option<i32> {
        if is_spell_type(t) {
            return None;
        }
        let base = self.type_time(t, who)?;
        let r = ((base.wrapping_mul(self.constant(0x3a8)?)) as u32 >> 8) as i32;
        if is_unit_type(t) {
            let p = self.ty(t)?.research_premium_time()?;
            let x = r.wrapping_mul(p);
            Some((x.wrapping_add((x >> 31) & 0xff)) >> 8)
        } else {
            Some(r)
        }
    }

    /// `TypeData::count_discovered` 0x00663f80 for a tech type: the number of
    /// live leaders (slots 0..8) whose `tech` bit is set. Unit/building/good
    /// kinds take the `has_preq` route and are not needed here.
    fn count_discovered(&self, t: i32) -> Option<i32> {
        if !is_tech_type(t) {
            return None;
        }
        let mut n = 0;
        for who in 0..8 {
            if self.leader_flags(who) & 1 == 0 {
                continue;
            }
            if self.tech_bit(who, t)? {
                n += 1;
            }
        }
        Some(n)
    }

    // --- LeaderData queries -----------------------------------------------------

    /// `LeaderData::has_tribe_bonus(int)` 0x006e1370.
    fn has_tribe_bonus(&self, who: usize, b: i32) -> Option<bool> {
        if self.info_flags() & 4 != 0 {
            return Some(false);
        }
        if self.setting(0x2c - 0x24) == 0 && self.ld_i32(who, 0x3f8)? == 0 {
            return Some(false);
        }
        let tribe = self.tribe(who)?;
        if tribe < 0 {
            return Some(false);
        }
        let l = self.leader(who)?;
        if bit(&l.conquest_racial_powers, b) {
            return Some(true);
        }
        if l.flags2 & 0x40 != 0 {
            return Some(false);
        }
        Some(self.tribe_bonus_id(tribe)? == b)
    }

    /// `LeaderData::has_wonder(int)` 0x006ebc10: conquest-wonder bit (age
    /// gated by `Constants+0xad4 <= epoch[1]`), then the owned-wonder scan
    /// over the Objects lists when `wonder_mark > 0` — that scan is not
    /// transcribed (`None`).
    fn has_wonder(&self, who: usize, w: i32) -> Option<i32> {
        if !is_wonder_type(w) {
            return Some(0);
        }
        let l = self.leader(who)?;
        let mut r = 0;
        if bit(&l.conquest_wonders, w - 0x20e) && self.constant(0xad4)? <= self.epoch(who, 1)? {
            r = 2;
        }
        if self.ld_i32(who, 0x424)? > 0 {
            return None;
        }
        Some(r)
    }

    /// `LeaderData::has_tech(TypeIndex)` 0x006e0c80 for the arguments the
    /// bonus-card preq chains reach: `-1` → true, `-2` → false, goods → true,
    /// tech/bonus kinds → `tech` bit. Unit and building kinds route through
    /// `has_preq` on an object type (not transcribed here → `None`).
    fn has_tech(&self, who: usize, t: i32) -> Option<bool> {
        if t == -1 {
            return Some(true);
        }
        if t == -2 {
            return Some(false);
        }
        if t < 0x32 {
            return Some(true);
        }
        if is_unit_type(t) || is_building_type(t) {
            return None;
        }
        self.tech_bit(who, t)
    }

    /// `TypeData::get_preq(slot, who)` 0x00668700 for a bonus type (never a
    /// unit or gov type, so the age-ladder substitutions reduce to: slot 1 is
    /// rewritten only when `starting_technology != 0` and `preq[1]` is an
    /// epoch type — `None` there).
    fn bonus_preq(&self, t: i32, slot: usize) -> Option<i32> {
        let ty = self.ty(t)?;
        match slot {
            0 => ty.preq(0),
            1 => {
                let p = ty.preq(1)?;
                if p < 0 || !is_epoch_type(p) {
                    return Some(p);
                }
                let st = if self.game_rules() == 8 {
                    self.starting_technology().min(self.starting_technology2())
                } else {
                    self.starting_technology()
                };
                if st == 0 && self.ending_technology() >= 7 {
                    Some(p)
                } else {
                    None
                }
            }
            _ => Some(-1),
        }
    }

    /// `LeaderData::has_preq(TypeIndex)` 0x006db810 restricted to **bonus
    /// types** (0x2ac..=0x325): the two preq slots (`BonusTypeData::num_preq
    /// == 2`, `special_preq` is the identity) must each pass `has_tech`; a
    /// preq that is itself a bonus card takes the government-count ladder
    /// (`get_govs_taken` 0x006d69f0 …), which `leaders_process.rs` holds
    /// privately — `None` here rather than a second copy. After the loop a
    /// non-age, non-gov type returns 1 (0x006dbbf0..0x006dbc1c).
    fn has_preq_bonus(&self, who: usize, t: i32) -> Option<bool> {
        if !is_bonus_type(t) {
            return None;
        }
        for i in 0..2 {
            let p = self.bonus_preq(t, i)?;
            if p < 0 {
                if !self.has_tech(who, p)? {
                    return Some(false);
                }
                continue;
            }
            if is_bonus_type(p) {
                return None;
            }
            if !self.has_tech(who, p)? {
                return Some(false);
            }
        }
        Some(true)
    }

    /// `LeaderData::get_ships_speed_upgrade` 0x006da800: `has_preq` count
    /// over bonus cards 0x2ec..=0x2ee (the `0x2ad` arm inside the loop is
    /// unreachable). Troops 0x006da850: 0x2e6..=0x2e8; vehicle 0x006da8a0:
    /// 0x2f8..=0x2fa.
    fn speed_upgrade(&self, who: usize, range: std::ops::RangeInclusive<i32>) -> Option<i32> {
        let mut n = 0;
        for t in range {
            if self.has_preq_bonus(who, t)? {
                n += 1;
            }
        }
        Some(n)
    }

    /// `LeaderData::get_player` 0x006ec0f0: first player row with `flags & 1`,
    /// `who == this.who`, `flags & 0x50 == 0`; else the last such-`who` row
    /// seen, else 0.
    fn get_player(&self, who: usize) -> usize {
        let mut fallback = 0usize;
        for p in 0..8 {
            let Some((flags, pwho, _)) = self.player(p) else { continue };
            if flags & 1 != 0 && pwho == who as i32 {
                fallback = p;
                if flags & 0x50 == 0 {
                    return p;
                }
            }
        }
        fallback
    }

    /// `LeaderData::is_ally(int)` 0x006edb50.
    fn is_ally(&self, who: usize, other: usize) -> Option<bool> {
        if who == other {
            return Some(true);
        }
        let mine = self.ld_i32(who, 0x74 + other * 4)?;
        let theirs = self.ld_i32(other, 0x74 + who * 4)?;
        Some(mine == 2 && theirs == 2)
    }

    /// `LeaderData::is_team(other, strict)` 0x006ebd30.
    fn is_team(&self, who: usize, other: usize, strict: bool) -> Option<bool> {
        if who == other {
            return Some(true);
        }
        let ts = self.team_style();
        if ts == 7 {
            for w in [who, other] {
                let (flags, _, team) = self.player(self.get_player(w))?;
                if flags & 1 != 0 && team == 8 {
                    return Some(false);
                }
            }
        }
        if self.frame() != 0 && !strict {
            return self.is_ally(who, other);
        }
        let (_, _, my_team) = self.player(self.get_player(who))?;
        if !(0..4).contains(&my_team) {
            return Some(false);
        }
        let (_, _, their_team) = self.player(self.get_player(other))?;
        if my_team != their_team {
            return Some(false);
        }
        if self.frame() != 0 && strict && (ts == 0 || ts == 0xb || ts == 8) {
            return self.is_ally(who, other);
        }
        Some(true)
    }

    /// `LeaderData::check_population(TypeIndex)` 0x006e1330:
    /// `pop_cap (+0x7e4) < unittypes[t].control_cost + control (+0x940)` —
    /// true when the leader is **short** of population for the unit.
    pub fn check_population(&self, who: usize, t: i32) -> Option<bool> {
        let cost = self.ty(t)?.control_cost()?;
        Some(self.ld_i32(who, 0x7e4)? < cost.wrapping_add(self.ld_i32(who, 0x940)?))
    }

    /// `LeaderData::get_handicap` 0x006da740 — indexes the `Categories`
    /// difficulty table `[0x00e80138] + i*0x58 + 0x3c`, which is not
    /// serialized. `None`.
    fn get_handicap(&self, _who: usize) -> Option<i32> {
        None
    }

    // --- Object-plane queries -----------------------------------------------------

    fn build_at(&self, who: usize, slot: usize) -> Option<&Build> {
        match self.save.objects.lists.get(who)?.elems.get(slot)? {
            Some(Obj::Build(b)) => Some(b),
            _ => None,
        }
    }

    /// `BuildData::is_unassimilated` 0x0062d470: `city >= 0 &&
    /// Cities[who][city].race (+0x5f) != who && (flags & 0x20 || !(ptype.
    /// build_flags & 0x10))`.
    fn is_unassimilated(&self, b: &Build) -> Option<bool> {
        if b.body.len() < 22 || b.base.sub.body.len() < 19 {
            return None;
        }
        let city = rd_u16(&b.body, 0x72 - 0x70)? as i16;
        if city < 0 {
            return Some(false);
        }
        let who = b.base.sub.body[0] as usize;
        let c = self.save.cities.lists.get(who)?.elems.get(city as usize)?.as_ref()?;
        let race = *c.pod.get(0x5f - 6)? as i8 as i32;
        if race == who as i32 {
            return Some(false);
        }
        if b.base.sub.flags & 0x20 != 0 {
            return Some(true);
        }
        let ptype = rd_i32(&b.base.sub.body, 15)?;
        Some(self.ty(ptype)?.build_flags()? & 0x10 == 0)
    }

    /// `LeaderData::get_first_library` 0x006db6c0: the first Build slot in
    /// `2000..build_mark[who]` that is live (`flags & 1`), active
    /// (`WallData::is_active` = `flags & 4`), attached to a city, assimilated,
    /// and whose type `is(0x1b3, 0)`; `-1` when none.
    pub fn get_first_library(&self, who: usize) -> Option<i32> {
        let o = 16 + 36 + who * 4;
        let mark = rd_i32(&self.save.objects.scalars, o)?;
        for slot in 2000..mark.max(2000) {
            let Some(b) = self.build_at(who, slot as usize) else { continue };
            let f = b.base.sub.flags;
            if f & 1 == 0 || f & 4 == 0 || b.body.len() < 22 || b.base.sub.body.len() < 19 {
                continue;
            }
            if (rd_u16(&b.body, 2)? as i16) < 0 {
                continue;
            }
            if self.is_unassimilated(b)? {
                continue;
            }
            let ptype = rd_i32(&b.base.sub.body, 15)?;
            if self.type_is(ptype, 0x1b3, false) {
                return Some(slot);
            }
        }
        Some(-1)
    }

    /// `CityData::has_building(what, strict=0, active=1)` 0x00739390: walk
    /// the city's building chain (`CityData+8` head, `Build+0x74` next) and
    /// count live, active (`flags & 4`) builds whose type `is(what, 0)`.
    fn city_has_building(&self, who: usize, city: &crate::sections::City, what: i32) -> Option<i32> {
        let mut n = 0;
        let mut cur = rd_u16(&city.pod, 8 - 6)? as i16;
        let cwho = *city.pod.get(0x5e - 6)? as i8;
        if cwho < 0 {
            return Some(0);
        }
        let mut guard = 0;
        while cur >= 0 {
            guard += 1;
            if guard > 4096 {
                return None;
            }
            let b = self.build_at(cwho as usize, cur as usize)?;
            if b.body.len() < 22 || b.base.sub.body.len() < 19 {
                return None;
            }
            let next = rd_u16(&b.body, 0x74 - 0x70)? as i16;
            let f = b.base.sub.flags;
            if f & 1 != 0 && f & 4 != 0 {
                let ptype = rd_i32(&b.base.sub.body, 15)?;
                if self.type_is(ptype, what, false) {
                    n += 1;
                }
            }
            cur = next;
        }
        let _ = who;
        Some(n)
    }

    /// `LeaderData::get_building_cities(0x1b3, 0, 1)` 0x006e06f0: cities in
    /// `0..city_mark` with `flags & 1`, `race == who`, and a Library. This is
    /// the number of queue rows `Build::do_queue` advances per frame in a
    /// Library.
    pub fn get_building_cities(&self, who: usize, what: i32) -> Option<i32> {
        let mark = self.ld_i32(who, 0x408)?;
        let lists = self.save.cities.lists.get(who)?;
        let mut n = 0;
        for i in 0..mark.max(0) as usize {
            let Some(Some(c)) = lists.elems.get(i) else { continue };
            if c.flags & 1 == 0 || c.pod.len() < 108 {
                continue;
            }
            if *c.pod.get(0x5f - 6)? as i8 as i32 != who as i32 {
                continue;
            }
            if self.city_has_building(who, c, what)? != 0 {
                n += 1;
            }
        }
        Some(n)
    }

    /// `ObjectData::has_general(ability, hero_type)` 0x00646b00 for a
    /// **Build** (`is_unit` is false, `is_build` true): radius
    /// `(x_size + y_size) * 0x60`; `-1` straight away when `num_units[hero_type]
    /// == 0` (the leader owns none); otherwise `HeroesData::find_hero`
    /// 0x0073a1b0 scans the hero list against the Unit plane — not
    /// transcribed, `None`.
    pub fn has_general(&self, b: &Build, ability: u32, hero_type: i32) -> Option<i32> {
        if b.base.sub.body.len() < 19 {
            return None;
        }
        let who = b.base.sub.body[0] as usize;
        if hero_type >= 0 && self.num_units(who, hero_type)? == 0 {
            return Some(-1);
        }
        if self.ld_i32(who, 0x420)? <= 0 {
            // hero_mark == 0: find_hero's loop body never runs.
            return Some(-1);
        }
        let _ = ability;
        None
    }
}

// ---------------------------------------------------------------------------
// ObjectData::train_time 0x006508c0
// ---------------------------------------------------------------------------

/// `ObjectData::train_time(type)` evaluated for the Build `b` (the `this`
/// object: `who` = sub.body[0], `o` = sub.body[1..3], `ptype` = sub.body
/// [15..19]). Returns the clamp total `Build::do_queue` uses, `>= 1`.
pub fn train_time(save: &Save, b: &Build, type_index: i32) -> Option<i32> {
    Ctx::new(save).train_time(b, type_index, 0)
}

/// Lead's working name for [`train_time`].
pub fn construct_time(save: &Save, b: &Build, type_index: i32) -> Option<i32> {
    train_time(save, b, type_index)
}

impl Ctx<'_> {
    fn train_time(&self, b: &Build, t: i32, depth: u8) -> Option<i32> {
        if b.base.sub.body.len() < 19 || depth > 2 {
            return None;
        }
        let who = b.base.sub.body[0] as usize;
        let ptype = rd_i32(&b.base.sub.body, 15)?;
        self.leader(who)?;
        let ty = self.ty(t)?;
        let known = is_unit_type(t) && self.tech_bit(who, t)?;

        // Base: units without their tech bit use research_time(who), else time(who).
        let mut v = if is_unit_type(t) && !known { self.type_research_time(t, who)? } else { self.type_time(t, who)? };

        // 0x29a / 0x286 (city "raze"/"disband" spells): time of this building's
        // own type, scaled by disband_city_rate / disband_senate_rate / Lakota
        // raze speed, halved.
        if t == 0x29a || t == 0x286 {
            v = self.train_time(b, ptype, depth + 1)?;
            if self.type_is(ptype, 0x19e, false) {
                v = self.constant(0x380)?.wrapping_mul(v) / 100;
            }
            if self.type_is(ptype, 0x1b6, false) {
                v = self.constant(0x384)?.wrapping_mul(v) / 100;
            }
            if self.has_tribe_bonus(who, 0x13)? {
                v = self.constant(0x850)?.wrapping_mul(v) / 100;
            }
            v /= 2;
        }

        // is(0x13b) nuke && rare bit 29 (uranium): uranium_nuke_speed.
        if self.type_is(t, 0x13b, false) && self.rare_bit(who, 29)? {
            v = faster(v, self.constant(0x958)?)?;
        }

        // ---- unit training block (unit type with its tech bit) 0x00650ab5 ----
        if known {
            v = self.constant(0x220)?.wrapping_mul(v) / 100; // unit_rate_base
            let cap = v.wrapping_mul(3);
            let n = self.num_units(who, t)? as i32;
            v = n.wrapping_mul(ty.job_extra_time()?).wrapping_mul(self.constant(0x224)?).wrapping_add(v); // unit_rate_progression
            if v < 0 || cap < 0 {
                v = 0;
            } else if cap < v {
                v = cap;
            }
            if self.sem(0) & 4 != 0 {
                let h = self.get_handicap(who)?;
                v = (200 - h).wrapping_mul(v) / 200;
            }
            // has_general(0, 0x163) >= 0: thepresident_unit_build_speed.
            if self.has_general(b, 0, 0x163)? >= 0 {
                v = faster(v, self.constant(0xc84)?)?;
            }
            let where_ = ty.where_()?;
            // Mongol: armed unit from a stable-like (where is(0x1ac)).
            if self.has_tribe_bonus(who, 0x11)? && ty.attack()? != 0 && where_ >= 0 && self.type_is(where_, 0x1ac, false) {
                v = faster(v, self.constant(0x7ec)?)?;
            }
            // Japanese: barracks (where == 0x1ab) scaled by min(ages, epoch[0]) when negative; carriers is(0x15f).
            if self.has_tribe_bonus(who, 0xf)? {
                if where_ == 0x1ab {
                    let mut p = self.constant(0x79c)?;
                    if p < 0 {
                        let m = self.ages(who)?.min(self.epoch(who, 0)?);
                        p = p.wrapping_mul(m).wrapping_neg();
                    }
                    v = faster(v, p)?;
                }
                if self.type_is(t, 0x15f, false) {
                    v = faster(v, self.constant(0x7a4)?)?;
                }
            }
            // Chinese: citizens (0x32/0x33), merchants, specialty units.
            if self.has_tribe_bonus(who, 0xe)? {
                let applies = t == 0x32 || t == 0x33 || is_merchant(t) || ty.unit_flags2()? & 8 != 0;
                if applies {
                    v = faster(v, self.constant(0x780)?)?;
                    if self.constant(0x77c)? != 0 {
                        return Some(1);
                    }
                }
            }
            // British: ships (domain 1), archers is(0xaa), AA is(0x119).
            if self.has_tribe_bonus(who, 0xb)? {
                if ty.domain()? == 1 {
                    v = faster(v, self.constant(0x6d4)?)?;
                }
                if self.type_is(t, 0xaa, false) {
                    v = faster(v, self.constant(0x6d8)?)?;
                }
                if self.type_is(t, 0x119, false) {
                    v = faster(v, self.constant(0x6e4)?)?;
                }
            }
            // French: siege (where 0x1ae/0x1af) or is(0x36).
            if self.has_tribe_bonus(who, 10)? {
                if where_ == 0x1ae || where_ == 0x1af {
                    v = faster(v, self.constant(0x6ac)?)?;
                } else if self.type_is(t, 0x36, false) {
                    v = faster(v, self.constant(0x6a4)?)?;
                }
            }
            // German: air (domain 2), submarines is(0x14e).
            if self.has_tribe_bonus(who, 0xc)? {
                if ty.domain()? == 2 {
                    v = faster(v, self.constant(0x710)?)?;
                }
                if self.type_is(t, 0x14e, false) {
                    v = faster(v, self.constant(0x70c)?)?;
                }
            }
            // Roman: legions is(0x84).
            if self.has_tribe_bonus(who, 6)? && self.type_is(t, 0x84, false) {
                v = faster(v, self.constant(0x634)?)?;
            }
            // role & 0x10000 && bonus card 0x315: *3/4.
            if ty.role()? & 0x10000 != 0 && self.has_preq_bonus(who, 0x315)? {
                v = mul3_div4(v);
            }
            // Speed-upgrade cards: ships (domain 1) / troops (objmask 0x20 or
            // 0x1000) / vehicles (objmask 0x200000): (10 - n) * v / 10.
            let masks = ty.obj_masks()?;
            let up = if ty.domain()? == 1 {
                Some(self.speed_upgrade(who, 0x2ec..=0x2ee)?)
            } else if masks & 0x20 != 0 || masks & 0x1000 != 0 {
                Some(self.speed_upgrade(who, 0x2e6..=0x2e8)?)
            } else if masks & 0x200000 != 0 {
                Some(self.speed_upgrade(who, 0x2f8..=0x2fa)?)
            } else {
                None
            };
            if let Some(u) = up {
                v = (10 - u).wrapping_mul(v) / 10;
            }
            // Cotton (rare bit 16): barracks/stable/dock-trained.
            if (where_ == 0x1ab || where_ == 0x1ac || where_ == 0x1b0) && self.rare_bit(who, 16)? {
                v = faster(v, self.constant(0x918)?)?;
            }
            // (is(0x36,1) || is(0x3a,1)) && card 0x302: /2.
            if (self.type_is(t, 0x36, true) || self.type_is(t, 0x3a, true)) && self.has_preq_bonus(who, 0x302)? {
                v /= 2;
            }
            // Wool (rare bit 27): citizens.
            if (t == 0x32 || t == 0x33) && self.rare_bit(who, 27)? {
                v = faster(v, self.constant(0x94c)?)?;
            }
            // Monarchy cards on stable-trained units.
            if where_ == 0x1ac {
                if self.has_preq_bonus(who, 0x321)? {
                    v = faster(v, self.constant(0x9b0)?)?;
                } else if self.has_preq_bonus(who, 0x320)? {
                    v = faster(v, self.constant(0x9ac)?)?;
                }
            }
            // Socialism card on siege/factory/dock units.
            if (where_ == 0x1ae || where_ == 0x1bf || where_ == 0x1b0) && self.has_preq_bonus(who, 0x324)? {
                v = faster(v, self.constant(0x9bc)?)?;
            }
            // Wonders by domain: land 0x219 (Statue of Liberty), sea 0x215
            // (Porcelain Tower), air 0x21e (Space Program).
            match ty.domain()? {
                0 => {
                    if self.has_wonder(who, 0x219)? != 0 {
                        v = faster(v, self.constant(0x520)?)?;
                    }
                }
                1 => {
                    if self.has_wonder(who, 0x215)? != 0 {
                        v = faster(v, self.constant(0x4ac)?)?;
                    }
                }
                2 => {
                    if self.has_wonder(who, 0x21e)? != 0 {
                        v = faster(v, self.constant(0x564)?)?;
                    }
                }
                _ => {}
            }
            // (is(0x3a,1) && wonder 0x21a) || card 0x2b8: instant (accel_train*5 - 1).
            if (self.type_is(t, 0x3a, true) && self.has_wonder(who, 0x21a)? != 0) || self.has_preq_bonus(who, 0x2b8)? {
                return Some(self.constant(0x228)?.wrapping_mul(5) - 1);
            }
        }

        // ---- tech block 0x00651366 ----
        if is_tech_type(t) {
            if is_age_type(t) {
                // Catch-up: leaders ahead by (ages + ages_queued) get a discount.
                let mine = self.ages(who)?.wrapping_add(self.ld_u8(who, 0x67f4)? as i32).wrapping_add(1);
                let mut n = 0;
                for other in 0..8 {
                    if self.leader_flags(other) & 1 == 0 {
                        continue;
                    }
                    let theirs = self.ages(other)?.wrapping_add(self.ld_u8(other, 0x67f4)? as i32);
                    if mine < theirs {
                        n += 1;
                        if !self.is_team(other, who, false)? {
                            n += 2;
                        }
                    }
                }
                if n != 0 {
                    let k = self.num_nations().wrapping_mul(2);
                    v = (k - n + 2).wrapping_mul(v) / (k + 2);
                }
                if self.has_tribe_bonus(who, 5)? {
                    v = faster(v, self.constant(0x5d4)?)?;
                }
            } else if is_epoch_type(t) {
                let mine = self.ages(who)?.wrapping_add(self.epochs(who)?).wrapping_add(3);
                let mut n = 0;
                for other in 0..8 {
                    if self.leader_flags(other) & 1 == 0 {
                        continue;
                    }
                    let theirs = self.ages(other)?.wrapping_add(self.epochs(other)?);
                    if mine < theirs {
                        n += 1;
                        if !self.is_team(other, who, false)? {
                            n += 2;
                        }
                    }
                }
                if n != 0 {
                    let k = self.num_nations().wrapping_mul(2);
                    v = (k - n + 2).wrapping_mul(v) / (k + 2);
                }
                if self.has_tribe_bonus(who, 5)? {
                    v = faster(v, self.constant(0x5d4)?)?;
                }
            } else {
                // Plain tech: discount by how many players already have it.
                let n = self.count_discovered(t)?;
                if n != 0 {
                    let np = self.num_players();
                    v = (np - n + 1).wrapping_mul(v) / (np + 1);
                }
            }
            if self.has_wonder(who, 0x21d)? != 0 && t != 0x29a && t != 0x29b {
                v = 0;
            }
        }

        // ---- research block 0x00651602: techs, and units/buildings without
        // their tech bit (i.e. being researched rather than produced) ----
        let research = is_tech_type(t) || ((is_unit_type(t) || is_building_type(t)) && !self.tech_bit(who, t)?);
        if research {
            if self.has_preq_bonus(who, 0x314)? {
                v = mul3_div4(v);
            }
            if self.has_wonder(who, 0x217)? != 0 {
                v = faster(v, self.constant(0x4cc)?)?;
            }
            // Relics (rare bit 1).
            if self.rare_bit(who, 1)? {
                v = (100 - self.constant(0x8d4)?).wrapping_mul(v) / 100;
            }
            // CtW great thinkers: num_bonus_cards[7] repeats.
            if self.sem(2) & 2 != 0 {
                let n = self.ld_u8(who, 0x68fd)?;
                for _ in 0..n {
                    v = faster(v, self.constant(0xae0)?)?;
                }
            }
            if self.sem(0) & 4 != 0 {
                let h = self.get_handicap(who)?;
                v = (200 - h).wrapping_mul(v) / 200;
            }
            match self.tech_cost() {
                0 | 2 => v = v.wrapping_mul(2) / 3,
                4 | 6 | 8 => v = v.wrapping_mul(3) / 2,
                _ => {}
            }
            // Science age (epoch[3]) ahead of the item's age: tech_science_speedup
            // percent per age, applied as v += -(x/100) through the 0xAE147AE1
            // reciprocal — kept bit-exact.
            let cur = self.epoch(who, 3)?;
            let age = if is_tech_type(t) {
                ty.tech_age()?
            } else {
                let p0 = ty.preq(0)?;
                if p0 < 0 {
                    0
                } else {
                    // techtypes[preq0].age — only meaningful for a tech preq.
                    if !is_tech_type(p0) {
                        return None;
                    }
                    self.ty(p0)?.tech_age()?
                }
            };
            if age < cur {
                let x = (cur - age).wrapping_mul(self.constant(0x368)?).wrapping_mul(v);
                let hi = ((x as i64).wrapping_mul(-0x51eb851fi64) >> 32) as i32;
                v = v.wrapping_add((hi >> 5) - (hi >> 31));
            }
            // Wonder 0x21d (Supercollider): instant research for techs only.
            if self.has_wonder(who, 0x21d)? != 0 && t != 0x29a && t != 0x29b && !is_building_type(t) && !is_unit_type(t) {
                v = 0;
            }
        }

        // ---- 0x006518f9: unassimilated-city penalty for builds ----
        if self.is_unassimilated(b)? {
            v = mul5_div4(v);
        }
        Some(v.max(1))
    }
}

// ---------------------------------------------------------------------------
// Build::do_queue 0x0061e410 arithmetic tail
// ---------------------------------------------------------------------------

/// What `Build::do_queue(slot)` would do to `QueueItem[slot].job_counter`
/// once the gates have passed (the caller — `build_process::do_queue` —
/// owns the gates, the row write, `Build::finished` 0x00628490 and
/// `Build::unqueue` 0x006207c0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueStep {
    /// `ObjectData::train_time(type)`.
    pub total: i32,
    /// Per-frame increment: `Constants.accel_construct` (+0x22c) for
    /// building types and 0x29a, `accel_train` (+0x228) for units with
    /// their tech bit, `accel_research` (+0x230) otherwise.
    pub rate: i32,
    /// `total <= cur` where `cur = total == 1 ? 1 : job_counter`.
    pub done: bool,
    /// `min(cur + rate, total)` — the value written when not `done`
    /// (retail writes `total` itself on the Library path's non-done row and
    /// on the non-Library done row).
    pub next: i32,
    /// `LeaderData::check_population(type)` fired (unit with tech bit and the
    /// leader short of pop): retail does `leader.pop_issues (+0x7e8) += 1`.
    pub pop_issue: bool,
}

/// Compute the `do_queue` step for `b`'s queue row `slot`. `None` when
/// `train_time` could not be evaluated (see module doc). The game-speed
/// multiplier `GameAccess::ai_speed` (`[0x00c061c0]`) is a cheat global, not
/// walked; treated as 1.
pub fn complete_queue_step(save: &Save, b: &Build, slot: usize) -> Option<QueueStep> {
    let ctx = Ctx::new(save);
    let row = b.queue.chunks_exact(18).nth(slot)?;
    let job_counter = rd_i32(row, 0)?;
    let t = rd_u16(row, 4)? as i16 as i32;
    let who = b.base.sub.body.first().copied()? as usize;
    let total = ctx.train_time(b, t, 0)?;
    let mut pop_issue = false;
    let rate = if is_building_type(t) || t == 0x29a {
        ctx.constant(0x22c)?
    } else if is_unit_type(t) && ctx.tech_bit(who, t)? {
        pop_issue = ctx.check_population(who, t)?;
        ctx.constant(0x228)?
    } else {
        ctx.constant(0x230)?
    };
    let cur = if total == 1 { 1 } else { job_counter };
    let done = total <= cur;
    let next = if cur.wrapping_add(rate) < total { cur.wrapping_add(rate) } else { total };
    Some(QueueStep { total, rate, done, next, pop_issue })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{container, load};
    use std::path::{Path, PathBuf};

    fn capture_dirs() -> Vec<PathBuf> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema/live/frame-pairs");
        let Ok(rd) = std::fs::read_dir(&root) else { return vec![] };
        let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.join("manifest.json").exists()).collect();
        v.sort();
        v
    }

    fn steps(dir: &Path) -> Vec<(i64, String)> {
        let text = std::fs::read_to_string(dir.join("manifest.json")).unwrap();
        let mut out = Vec::new();
        for seg in text.split("\"frame\":").skip(1) {
            let frame = seg.trim_start().split(|c: char| !c.is_ascii_digit()).next().unwrap().parse::<i64>().unwrap();
            let save = seg.split("\"save_name\":").nth(1).and_then(|s| s.split('"').nth(1)).unwrap().to_string();
            out.push((frame, save));
        }
        out
    }

    fn load_pair(dir: &Path, a: &str, b: &str) -> (Save, Save) {
        let ra = container::load_svx(&dir.join(format!("{a}.svx"))).unwrap();
        let rb = container::load_svx(&dir.join(format!("{b}.svx"))).unwrap();
        (load(&ra).unwrap().state, load(&rb).unwrap().state)
    }

    /// Diagnostic: every queue row in every capture frame, with our
    /// train_time and the retail N+1 counter.
    #[test]
    #[ignore]
    fn dump_queues() {
        for dir in capture_dirs() {
            let st = steps(&dir);
            println!("== {}", dir.display());
            for k in 0..st.len().saturating_sub(1) {
                let (a, b) = load_pair(&dir, &st[k].1, &st[k + 1].1);
                let ctx = Ctx::new(&a);
                println!(
                    "-- f{}->f{} stride {} sem0={:#x} sem2={:#x} tech_cost={} np={} nn={}",
                    st[k].0,
                    st[k + 1].0,
                    st[k + 1].0 - st[k].0,
                    ctx.sem(0),
                    ctx.sem(2),
                    ctx.tech_cost(),
                    ctx.num_players(),
                    ctx.num_nations()
                );
                for owner in 0..a.objects.lists.len() {
                    for slot in 0..a.objects.lists[owner].elems.len() {
                        let Some(Obj::Build(xa)) = &a.objects.lists[owner].elems[slot] else { continue };
                        if xa.queue.is_empty() {
                            continue;
                        }
                        let xb = match b.objects.lists.get(owner).and_then(|l| l.elems.get(slot)) {
                            Some(Some(Obj::Build(xb))) => Some(xb),
                            _ => None,
                        };
                        let ptype = rd_i32(&xa.base.sub.body, 15).unwrap_or(-1);
                        let who = xa.base.sub.body.first().copied().unwrap_or(0);
                        let queued = xa.body.get(0x82 - 0x70).copied().unwrap_or(0);
                        let first_lib = ctx.get_first_library(who as usize);
                        let libs = ctx.get_building_cities(who as usize, 0x1b3);
                        for (i, row) in xa.queue.chunks_exact(18).enumerate() {
                            let jc = rd_i32(row, 0).unwrap();
                            let t = rd_u16(row, 4).unwrap() as i16 as i32;
                            let nb = xb.and_then(|xb| xb.queue.chunks_exact(18).nth(i)).map(|r| (rd_i32(r, 0).unwrap(), rd_u16(r, 4).unwrap() as i16));
                            let step = complete_queue_step(&a, xa, i);
                            println!(
                                "  [{owner}][{slot}] who={who} ptype={ptype} flags={:#x} queued={queued} first_lib={first_lib:?} libs={libs:?} row{i}: type={t} jc={jc} -> retail {nb:?} | ours {step:?}",
                                xa.base.sub.flags
                            );
                        }
                    }
                }
            }
        }
    }

    /// Lane gate: for every stride-1 pair in every capture dir, for every
    /// Build with a non-empty queue whose row 0 passes the `do_queue` gates
    /// we can evaluate, retail N+1's `job_counter` must equal our `next`
    /// (`min(cur + rate, train_time)`), or — when the row completed — the
    /// row must have been consumed (`Build::unqueue`) or rewritten to
    /// `total`. `None` evaluations are counted and reported, never asserted
    /// against; at least one row must be verified.
    #[test]
    fn queue_counter_matches_retail() {
        let dirs = capture_dirs();
        if dirs.is_empty() {
            eprintln!("no capture dirs; skipping");
            return;
        }
        let (mut verified, mut unevaluated, mut skipped_gate) = (0usize, 0usize, 0usize);
        let mut mismatches = Vec::new();
        for dir in dirs {
            let st = steps(&dir);
            for k in 0..st.len().saturating_sub(1) {
                if st[k + 1].0 - st[k].0 != 1 {
                    continue;
                }
                let (a, b) = load_pair(&dir, &st[k].1, &st[k + 1].1);
                let ctx = Ctx::new(&a);
                for owner in 0..a.objects.lists.len() {
                    for slot in 0..a.objects.lists[owner].elems.len() {
                        let Some(Obj::Build(xa)) = &a.objects.lists[owner].elems[slot] else { continue };
                        if xa.queue.is_empty() || xa.body.len() < 22 || xa.base.sub.body.len() < 19 {
                            continue;
                        }
                        // Traversal gate: only live, active Builds reach Build::process → do_queue.
                        if xa.base.sub.flags & 1 == 0 || xa.base.sub.flags & 4 == 0 {
                            continue;
                        }
                        if xa.body[0x82 - 0x70] == 0 {
                            continue;
                        }
                        let who = xa.base.sub.body[0] as usize;
                        let o = rd_u16(&xa.base.sub.body, 1).unwrap() as i16 as i32;
                        let ptype = rd_i32(&xa.base.sub.body, 15).unwrap();
                        let row0 = xa.queue.chunks_exact(18).next().unwrap();
                        let t0 = rd_u16(row0, 4).unwrap() as i16 as i32;
                        // Library gate: is(0x1b3) && o != get_first_library() && type != 0x29a → return.
                        if ctx.type_is(ptype, 0x1b3, false) && t0 != 0x29a {
                            match ctx.get_first_library(who) {
                                Some(fl) if fl == o => {}
                                Some(_) => {
                                    skipped_gate += 1;
                                    continue;
                                }
                                None => {
                                    unevaluated += 1;
                                    continue;
                                }
                            }
                        }
                        // is(0x208,1) && inside_down >= 0 && tech bit → return.
                        if ctx.type_is(ptype, 0x208, true)
                            && (rd_u16(&xa.base.mid, 0x28 - 0x20).unwrap() as i16) >= 0
                            && t0 != 0x29a
                            && ctx.tech_bit(who, t0) == Some(true)
                        {
                            skipped_gate += 1;
                            continue;
                        }
                        // Library: rows 0..min(get_building_cities, queued) all advance this frame.
                        let rows = if ctx.type_is(ptype, 0x1b3, false) {
                            match ctx.get_building_cities(who, 0x1b3) {
                                Some(n) => (n.max(1) as usize).min(xa.body[0x82 - 0x70] as usize),
                                None => {
                                    unevaluated += 1;
                                    continue;
                                }
                            }
                        } else {
                            1
                        };
                        let xb = match b.objects.lists.get(owner).and_then(|l| l.elems.get(slot)) {
                            Some(Some(Obj::Build(xb))) => xb,
                            _ => continue,
                        };
                        for i in 0..rows.min(xa.queue.len() / 18) {
                            let row = xa.queue.chunks_exact(18).nth(i).unwrap();
                            let jc = rd_i32(row, 0).unwrap();
                            let t = rd_u16(row, 4).unwrap() as i16 as i32;
                            let Some(step) = complete_queue_step(&a, xa, i) else {
                                unevaluated += 1;
                                continue;
                            };
                            let retail = xb.queue.chunks_exact(18).nth(i).map(|r| (rd_i32(r, 0).unwrap(), rd_u16(r, 4).unwrap() as i16 as i32));
                            let ok = match retail {
                                Some((rjc, rt)) if rt == t => {
                                    if step.done {
                                        // Non-Library done row is rewritten to `total` (then usually
                                        // consumed); Library done row is consumed. Either way the
                                        // surviving same-type row must not have advanced past total.
                                        rjc == step.total || rjc <= jc
                                    } else {
                                        rjc == step.next
                                    }
                                }
                                // Row consumed (unqueue shifted the queue, or the type changed):
                                // only legitimate when the row completed.
                                _ => step.done,
                            };
                            if ok {
                                verified += 1;
                            } else {
                                mismatches.push(format!(
                                    "{} f{}->f{} [{owner}][{slot}] ptype={ptype} row{i} type={t} jc={jc} retail={retail:?} ours={step:?}",
                                    dir.file_name().unwrap().to_string_lossy(),
                                    st[k].0,
                                    st[k + 1].0
                                ));
                            }
                        }
                    }
                }
            }
        }
        eprintln!("queue rows: verified={verified} unevaluated={unevaluated} skipped_gate={skipped_gate} mismatches={}", mismatches.len());
        for m in mismatches.iter().take(40) {
            eprintln!("  MISMATCH {m}");
        }
        assert!(mismatches.is_empty(), "queue counter mismatches");
        assert!(verified > 0, "no queue row verified");
    }
}
