//! The combat cone, transcribed as free functions over `Save`:
//!
//! ```text
//!   Unit::do_attack        0x005f1b80  (order dispatcher)            [do_attack]
//!     └ Unit::fight        0x005fd4d0  (attack cycle, 8,157 B)       TODO — RNG sites recorded
//!        └ Object::do_damage   0x0064a480  (applier)                 [do_damage]
//!            ├ ObjectData::get_damage 0x00644130 (31-stage chain)   [get_damage]
//!            └ Object::take_damage    0x00652020 (hp accounting,
//!                                      death, uber cascade)          [take_damage]
//!                └ Unit::die 0x0060eda0 -> Object::die 0x00647080    [object_die]
//!                    └ Unit::close 0x0060ee50 -> Objects::add_death
//!                                              0x00653b60            [add_death]
//!   Objects::remove        0x00658980  (mark rollback)               [objects_remove]
//! ```
//!
//! Every object read/write is addressed by the retail image offset (PDB
//! `ObjectData` / `UnitData` / `WallData` / `BuildData`) through [`OImg`],
//! which maps onto the serialized sub-ranges of [`crate::sections::Unit`] and
//! [`crate::sections::Build`]:
//!
//! ```text
//!   +0x08         SubObject.flags                     base.sub.flags
//!   +0x09..+0x1c  SubObject  who,o,z,x,y,ptype         base.sub.body
//!   +0x20..+0x42  ObjectData myhits..launch_frames    base.mid
//!   +0x48..+0xb7  UnitData   collide_frame..play      Unit.body
//!   +0x48..+0x66  WallData   job_counter..demolition  Build.wall_body
//!   +0x70..+0x86  BuildData  gather_down..infiltrate2 Build.body
//! ```
//!
//! Virtual dispatch is resolved statically from the concrete plane
//! (`Unit::vftable` 0x00b417d0, `Animal::vftable` 0x00b4145c,
//! `Build::vftable` 0x00b42174, `Wall::vftable` 0x00b42cf8, dumped with
//! Capstone/pefile): `+0x18 is_unit` (Unit/Animal), `+0x1c is_build`
//! (Build/Wall), `+0x20 is_building` (Build only), `+0x4c` = `flags & 1`
//! (Unit) / `WallData::is_active` = `flags & 4` (Build/Wall), `+0xe4
//! get_captain`, `+0xe8 is_captain` (`o_up < 0`, Unit) / 1 (Build), `+0x11c
//! hits`, `+0x120 attack`, `+0x124 armor`, `+0x130 max_range`, `+0x148
//! has_objmask`, `+0x158 die`, `+0x16c take_damage`. Type vtable (0x00b41fd4 /
//! 0x00b42b94): `+0x60 ObjectTypeData::is`, `+0xfc is_fort` (Build) / 0,
//! `+0x10c is_siege` = `unit_flags & 0x20000`, `+0x120 blocks_while_dead` =
//! `unit_flags & 0x800000`, `+0x130 is_caravan` = `unit_flags2 & 8`.
//!
//! # Verification
//!
//! No capture in `schema/live/frame-pairs/` contains combat, so nothing here
//! is validated against a retail frame pair. The pure damage chain
//! [`get_damage`] is checked against 130 input/output samples recorded from
//! `don_sim::mechanics::damage_traced`, which the i686 oracle
//! (`schema/oracle-regression.json` case `damage_pipeline`, VA 0x00644130)
//! ran differentially against retail for 7,986,675 trials with 0 mismatches.
//! The samples cover every retail-verified step; steps 10 (Wellington
//! `+0xbbc`) and 11 (Japanese `+0x794`) were unreachable in the oracle and
//! stay UNVERIFIED here too (their terms default inert). The oracle compared
//! the return value only, and excluded building defenders (the
//! `[0x00c0ab84]`/`[0x00c0aec0]` table alias) — so the chain is verified for
//! unit defenders only.
//!
//! Everything else — the applier's sixteenth split, `take_damage`'s
//! accumulator and death threshold, the `DeathObj` slot policy — is a literal
//! transcription with structural tests only, **unverified against retail**.
//!
//! # RNG (`Random::get` 0x00a39d70 on `GameAccess::game_random`)
//!
//! Draws on the transcribed path, in order:
//! * `take_damage` 0x00652020: when `quiet == 0 && this->damage == 0 &&
//!   is_build()`: one draw `Random::get(0,0xffff) % 100 < 5` (0x00652150);
//!   if it hits and the building is a fort / `is(0x1b5)` / `is(0x19f)` and
//!   the attacker's type `is_siege()`: a second draw `Random::get(0,0xffff)
//!   & 1` (0x006521a9) feeding `Objects::add_flock(tile_x, tile_y, -1, r+3)`.
//! * `Unit::fight` 0x005fd4d0 (NOT transcribed): `Random::get(0,0xffff) & 1`
//!   at 0x005fdcfd (captain with `unit_masks2 & 0x40000`, deciding whether
//!   to re-check the target) and `Random::get(0,0xffff) % 5 == 0` at
//!   0x005fe0a0 (melee re-target gate). Both only when the unit is not
//!   recharging.
//! * `Object::do_damage` 0x0064a480 and `ObjectData::get_damage` 0x00644130
//!   draw nothing.

#![allow(dead_code)]

use crate::sections::{Build, Obj, Save, TypeRec, Unit};
use crate::tick::StepStatus;

pub const STATUS: StepStatus = StepStatus::Partial;

/// `(who, o)` — an `Objects.lists[who][o]` slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjRef {
    pub who: i32,
    pub o: i32,
}

impl ObjRef {
    pub fn new(who: i32, o: i32) -> Self {
        ObjRef { who, o }
    }
    fn valid(&self) -> bool {
        self.who >= 0 && self.o >= 0
    }
}

// ===========================================================================
// 1. ObjectData::get_damage 0x00644130 — the 31-stage chain
// ===========================================================================

/// The `Constants` (`[0x00c061e4]` / `[0x00c061f0]`, walked as
/// `Rules.constants`) the chain reads. `/100` fields are integer percents,
/// `/256` fields are 8.8 fixed point (`String::fraction(256)`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CombatRules {
    /// `+0x44` HEIGHT_INCREMENT — denominator (×100) at 0x00644d70.
    pub height_increment: i32,
    /// `+0x48` HEIGHT_BONUS — percent per increment.
    pub height_bonus: i32,
    /// `+0x4c` FLANK_BONUS — percent per flank level.
    pub flank_bonus: i32,
    /// `+0x50` CAVALRY_FLANK_BONUS — 8.8 scaling of `flank_bonus` (`wtoi`, 40).
    pub cavalry_flank_bonus: i32,
    /// `+0x54` VEHICLE_FLANK_BONUS — 8.8 scaling of `flank_bonus` (`wtoi`, 33).
    pub vehicle_flank_bonus: i32,
    /// `+0x58` ROCKY_MODIFIER — 8.8.
    pub rocky_modifier: i32,
    /// `+0x5c` OVERKILL_FRAMES — frame window (30).
    pub overkill_frames: i32,
    /// `+0x60` OVERKILL_DAMAGE — 8.8 (`"1/3"` → 85).
    pub overkill_damage: i32,
    /// `+0x64` ENTRENCHMENT_MODIFIER — 8.8.
    pub entrenchment_modifier: i32,
    /// `+0x68` RIVER_MODIFIER — 8.8.
    pub river_modifier: i32,
    /// `+0x6c` RECAPTURE_CITY_MODIFIER — 8.8 (`"2/1"` → 512).
    pub recapture_city_modifier: i32,
    /// `+0x4c4` RED_FORT_AIR_DEFENSE — percent, applied `(100 - v)/100`.
    pub red_fort_air_defense: i32,
    /// `+0x558` SUPER_IMMUNE — gate for the step-29 zeroing.
    pub super_immune: i32,
    /// `+0x76c` RUSSIAN_COSSACK_DAMAGE — percent, `(100 + v)/100` (step 27).
    pub russian_cossack_damage: i32,
    /// `+0xb98` ANTIPATER_ENTRENCH_BONUS — 8.8, inside the entrenchment branch.
    pub antipater_entrench_bonus: i32,
}

impl CombatRules {
    pub fn from_save(save: &Save) -> Self {
        CombatRules {
            height_increment: constant(save, 0x44),
            height_bonus: constant(save, 0x48),
            flank_bonus: constant(save, 0x4c),
            cavalry_flank_bonus: constant(save, 0x50),
            vehicle_flank_bonus: constant(save, 0x54),
            rocky_modifier: constant(save, 0x58),
            overkill_frames: constant(save, 0x5c),
            overkill_damage: constant(save, 0x60),
            entrenchment_modifier: constant(save, 0x64),
            river_modifier: constant(save, 0x68),
            recapture_city_modifier: constant(save, 0x6c),
            red_fort_air_defense: constant(save, 0x4c4),
            super_immune: constant(save, 0x558),
            russian_cossack_damage: constant(save, 0x76c),
            antipater_entrench_bonus: constant(save, 0xb98),
        }
    }
}

/// Everything the chain reads that is not a rules constant and not a
/// virtual predicate. Field names keep the oracle harness's vocabulary
/// (`don_sim::mechanics::DamageInput`) so the recorded samples map 1:1; the
/// PDB name of each source field is in its comment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DamageInput {
    /// 0x0064418e: `(i32)(i16) Balance[(atk_type-50)*493 + (def_type-50)]`.
    pub balance_pct: i32,
    /// 0x006441be: `attacker->attack()` (vtable +0x120), carried ×10.
    pub attack: i32,
    /// 0x006441b1: `defender->armor()` (vtable +0x124).
    pub armor: i32,
    /// attacker `ObjectTypeData +0x1e4 obj_masks`.
    pub attacker_masks: u32,
    /// defender `ObjectTypeData +0x1e4 obj_masks`.
    pub defender_masks: u32,
    /// arg 3 — the attack direction differenced against `angle`/`trench_angle`.
    pub attack_dir: i32,
    /// arg 4 — non-zero selects the splash steps 13..16 and suppresses the floor.
    pub splash_flag: i32,
    /// arg 5 — gates the overkill block (step 23). `do_damage` passes 1.
    pub overkill_gate: i32,
    /// attacker `SubObject +0x09 who`.
    pub attacker_player: u32,
    /// attacker `TypeData +0x04 type` (TypeIndex).
    pub attacker_type_id: i32,
    /// attacker `UnitTypeData +0x218 domain` (0 land, 1 sea, 2 air).
    pub attacker_domain: i32,
    /// attacker `UnitTypeData +0x204 splash_percent`.
    pub attacker_splash_percent: i32,
    /// attacker `TypeData +0x40 where`.
    pub attacker_type_0x40: i32,
    /// attacker `z_internal ^ 0x63637`.
    pub attacker_z: i32,
    /// attacker `SubObject.flags & 0x20`.
    pub attacker_flag8_bit5: bool,
    /// defender `TypeData +0x04 type`.
    pub defender_type_id: i32,
    /// defender `UnitTypeData +0x218 domain`.
    pub defender_domain: i32,
    /// defender `UnitTypeData +0x2b8 unit_flags2 & 4`.
    pub defender_type_0x2b8_bit2: bool,
    /// defender `UnitTypeData +0x308 uber_size` — real `idiv` at 0x006448b9.
    pub defender_splash_divisor: i32,
    /// defender `UnitData +0x68 unit_masks`.
    pub defender_flags_0x68: u32,
    /// defender `UnitData +0x6c unit_masks2 & 0x1000`.
    pub defender_flags_0x6c_bit12: bool,
    /// defender `z_internal ^ 0x63637`.
    pub defender_z: i32,
    /// defender `UnitData +0x50 angle`.
    pub defender_facing: i32,
    /// defender `UnitData +0x5c trench_angle`.
    pub defender_facing_entrench: i32,
    /// defender `UnitData +0x4c damage_frame`.
    pub defender_overkill_stamp: i32,
    /// defender `UnitData +0xa4 damage_o` (sign-extended).
    pub defender_word_0xa4: i32,
    /// `attacker->get_captain()` (vtable +0xe4) at 0x00644c0d.
    pub attacker_vf_0xe4: i32,
    /// `Game+0x550 frame`.
    pub current_frame: i32,
    /// `Game+0x821 & 2` (`sem_ptr[1]`).
    pub game_flag_0x821_bit1: bool,
    /// `WData[tile(def)].flags & 8` at 0x00644cd7.
    pub tile_rocky: bool,
    /// `WData[tile(def)] +0x0f owner` at 0x00644509.
    pub tile_owner: i32,
}

/// The guards the chain evaluates through the object graph or a virtual.
/// Each is named for its retail slot; the resolved meaning is in the comment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DamagePredicates {
    /// 0x00644243..0x006442a1 — the aux-object chain (`Types[0x222]` tech
    /// preq through `LeaderData::has_preq`) authorising the attacker-mask fixup.
    pub mask_fixup_authorised: bool,
    /// attacker `+0x18 is_unit`.
    pub attacker_vf_0x18: bool,
    /// attacker `+0x1c is_build`.
    pub attacker_vf_0x1c: bool,
    /// attacker `+0x20 is_building` (Build yes, Wall no).
    pub attacker_vf_0x20: bool,
    /// attacker `+0x130 max_range() != 0`.
    pub attacker_vf_0x130: bool,
    /// attacker type `+0x10c is_siege`.
    pub attacker_type_vf_0x10c: bool,
    /// the attacker re-fetched from the table, `+0x20` (same as
    /// `attacker_vf_0x20` in a consistent world).
    pub attacker_table_vf_0x20: bool,
    /// defender `+0x18 is_unit`.
    pub defender_vf_0x18: bool,
    /// defender `+0x1c is_build`.
    pub defender_vf_0x1c: bool,
    /// defender `+0x20 is_building`.
    pub defender_vf_0x20: bool,
    /// defender `+0x120 attack() != 0`.
    pub defender_vf_0x120: bool,
    /// defender `+0xcc UnitData::is_supply` = `unit_flags2 & 0x40`.
    pub defender_vf_0xcc: bool,
    /// defender `+0xd0 UnitData::is_caravan` = type `unit_flags2 & 8`.
    pub defender_vf_0xd0: bool,
    /// defender `+0xd8 UnitData::is_moving` 0x00610af0.
    pub defender_vf_0xd8: bool,
    /// defender type `+0x10c is_siege`.
    pub defender_type_vf_0x10c: bool,
    /// `defender->get_build()` (+0x3c) non-null and `WallData::is_active`
    /// (`flags & 4`) non-zero (0x00644453 / 0x0064456d).
    pub defender_build_flag: bool,
    /// `defender->get_build()->flags & 0x20` (city centre) — 0x00644fe5.
    pub defender_build_0x20: bool,
    /// step-30 city-owner comparison — 0x00645018.
    pub recapture_owner_matches: bool,
    /// `(defender +0x40 get_wall_data())->+0x184` — 0x00644a44.
    pub defender_carrier_vf_0x184: bool,
    /// `(defender +0x40)->+0x20` — 0x00644a74.
    pub defender_carrier_vf_0x20: bool,
    /// `(defender +0x3c)->type +0x1e8 attack == 0` — 0x00644a8f.
    pub defender_mount_attack_is_zero: bool,
    /// attacker type `is(0x42 MILITIA)`.
    pub attacker_tech_0x42: bool,
    /// attacker type `is(0x139 V2ROCKET)`.
    pub attacker_tech_0x139: bool,
    /// attacker type `is(0x83 FLAMETHROWER)` — ignores entrenchment.
    pub attacker_tech_0x83: bool,
    /// defender type `is(0x216 REDFORT)`.
    pub defender_tech_0x216: bool,
    /// defender type `is(0x143 BARK)`.
    pub defender_tech_0x143: bool,
    /// defender type `is(0x109 CATAPULT)`.
    pub defender_tech_0x109: bool,
    /// UNVERIFIED step 10 — `ObjectData::has_general(0, 0x164)` (Wellington).
    pub step10_bonus_applies: bool,
    /// UNVERIFIED step 11 — `LeaderData::has_tribe_bonus(0xf)` (Japanese).
    pub step11_player_prop_0xf: bool,
    /// UNVERIFIED step 27 — `LeaderData::has_tribe_bonus(0xd)` (Cossack).
    pub step27_player_prop_0xd: bool,
    /// step 12 — `LeaderData::get_target(def_player) != attacker player`.
    pub step12_team_differs: bool,
    /// `Game+0x24 settings[0] == 2` — 0x00644866.
    pub game_mode_is_2: bool,
}

/// Terms for the two steps the oracle never reached. Default inert.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UnreachedTerms {
    /// `Constants +0xbbc` WELLINGTON_SIEGE_ATTACK, added at 0x0064473d.
    pub wellington_siege_attack: i32,
    /// `Constants +0x794` JAPANESE_DAMAGE, step-11 percent (negative = per level).
    pub japanese_damage: i32,
    /// `min(leader+0xdc ^ 0x62766, leader+0xe8 ^ 0x63187)` for step 11.
    pub step11_player_level: i32,
}

/// `x / 100` truncating — the `0x51eb851f` multiply idiom; the product
/// must already be wrapped to 32 bits.
#[inline]
fn div100(x: i32) -> i32 {
    x / 100
}

/// `x / 256` truncating — `cdq; and edx,0xff; lea; sar 8`.
#[inline]
fn div256(x: i32) -> i32 {
    x / 256
}

/// A real `idiv` with no retail zero check: panics where retail raises `#DE`.
#[inline]
#[track_caller]
fn idiv(num: i32, den: i32, site: &str) -> i32 {
    if den == 0 || (num == i32::MIN && den == -1) {
        panic!("retail raises #DE: idiv {num} / {den} at {site}");
    }
    num / den
}

/// `flank_level` 0x0092cfe0 — 0/1/2 from the biased angle delta.
#[inline]
pub fn flank_level(delta: u32) -> u32 {
    if delta > 0xd555_5555 {
        0
    } else if 0x4000_0000u32 < delta.wrapping_sub(0x6000_0000) {
        2
    } else {
        1
    }
}

/// The entrenchment direction classifier inlined at 0x00644e0e — a
/// different reject test from [`flank_level`].
#[inline]
pub fn entrench_dir_level(delta: u32) -> u32 {
    if delta.wrapping_sub(0x2aaa_aaaa) > 0xaaaa_aaab {
        0
    } else if 0x4000_0000u32 < delta.wrapping_sub(0x6000_0000) {
        2
    } else {
        1
    }
}

/// `ObjectData::get_damage` 0x00644130 — return value only (`out_kind`,
/// the 6th argument, is written `2` at entry and never changed here).
pub fn get_damage(i: &DamageInput, p: &DamagePredicates, r: &CombatRules, u: &UnreachedTerms) -> i32 {
    get_damage_traced(i, p, r, u).0
}

/// [`get_damage`] plus a bitmask of which guarded steps ran (bit n ↔ step
/// table in `docs/mechanics/combat.md`; used to prove sample coverage).
#[allow(clippy::collapsible_if)]
pub fn get_damage_traced(
    i: &DamageInput,
    p: &DamagePredicates,
    r: &CombatRules,
    u: &UnreachedTerms,
) -> (i32, u64) {
    let mut t: u64 = 0;
    let b = i.balance_pct;
    let mut arm = i.armor;
    let mut am = i.attacker_masks;
    let dm = i.defender_masks;

    // 0x00644204..0x006442ae: attacker-mask fixup for FORTX/CASTLE or a
    // flag-0x20 attacker; the machine code also ORs bit 6 at 0x006442ab.
    if i.attacker_type_id == 0x1bc || i.attacker_type_id == 0x1bb || i.attacker_flag8_bit5 {
        if p.mask_fixup_authorised {
            am = (am & 0xfffd_ffff) | 0x40;
            t |= 1 << 0;
        }
    }

    // 1 — 0x006442b1: attack × balance / 100 (wrapping product).
    let mut d = div100(i.attack.wrapping_mul(b));

    // 2 — 0x006442c9: armor × 133 / 100 for obj_masks & 8.
    if am & 0x8 != 0 {
        arm = div100(arm.wrapping_mul(133));
        t |= 1 << 1;
    }

    // 3 — 0x00644307
    if p.attacker_table_vf_0x20
        && dm & 0x0004_0000 != 0
        && p.defender_vf_0x18
        && i.defender_type_0x2b8_bit2
        && i.defender_flags_0x68 & 0x0008_0000 == 0
    {
        d /= 3; // 0x00644352
        t |= 1 << 2;
    }

    // 4 — 0x0064435b
    if p.attacker_vf_0x20 && p.defender_vf_0x18 && p.defender_vf_0x120 && p.defender_vf_0xd8 {
        if dm & 0x20 != 0 {
            d = d.wrapping_mul(3) / 4; // 0x006443bd
            t |= 1 << 3;
        } else if p.defender_type_vf_0x10c || p.defender_vf_0xcc || p.defender_vf_0xd0 {
            d /= 2; // 0x0064441e
            t |= 1 << 4;
        }
    }

    // 5 — 0x00644420
    if p.attacker_vf_0x20 && p.defender_vf_0x20 && !p.defender_build_flag {
        d = d.wrapping_mul(4); // 0x0064446a
        t |= 1 << 5;
    }

    // 6 — 0x0064446d: peasant/militia attacker on foreign territory.
    if p.defender_vf_0x20 {
        let id_hit = matches!(i.attacker_type_id, 0x32 | 0x33 | 0x34 | 0x35);
        if (id_hit || p.attacker_tech_0x42) && i.tile_owner != i.attacker_player as i32 {
            d /= 2; // 0x0064451d
            t |= 1 << 6;
        }
    }

    // 7 — 0x0064451f
    if p.attacker_tech_0x139 && p.defender_vf_0x20 && !p.defender_build_flag {
        d /= 2; // 0x0064458b
        t |= 1 << 7;
    }

    // 8 — 0x0064458d: Red Fort vs air; the mask re-read is the unmutated type field.
    if p.attacker_vf_0x18
        && i.attacker_domain == 2
        && i.attacker_masks & 0x0800_0000 == 0
        && p.defender_tech_0x216
    {
        d = div100(d.wrapping_mul(100i32.wrapping_sub(r.red_fort_air_defense))); // 0x006445f7
        t |= 1 << 8;
    }

    // 9 — 0x00644606
    if p.defender_vf_0x18 {
        let c = i.defender_flags_0x68 & 0x0008_0000 != 0;
        if c {
            d = d.wrapping_add(d); // 0x00644632
            t |= 1 << 9;
        }
        if i.defender_type_0x2b8_bit2 && !c {
            arm = arm.wrapping_add(1); // 0x00644644
            t |= 1 << 10;
        }
    }

    // 10 — 0x006446be. UNVERIFIED.
    if p.step10_bonus_applies {
        d = d.wrapping_add(u.wellington_siege_attack); // 0x0064473d
        t |= 1 << 11;
    }

    // 11 — 0x00644743. UNVERIFIED.
    if p.step11_player_prop_0xf && i.attacker_type_0x40 == 0x1ab {
        let mut m = u.japanese_damage;
        if m < 0 {
            m = m.wrapping_mul(u.step11_player_level).wrapping_neg(); // 0x006447e3
        }
        d = div100(m.wrapping_add(100).wrapping_mul(d)); // 0x006447f0
        t |= 1 << 12;
    }

    // 12 — 0x006447ff
    if p.attacker_vf_0x20 || (i.attacker_domain == 0 && i.defender_domain != 0) {
        if p.defender_vf_0x18 {
            let by_flag = i.defender_flags_0x68 & 0x0040_0000 != 0;
            let by_team = i.defender_domain == 1 && p.game_mode_is_2 && p.step12_team_differs;
            if by_flag || by_team {
                d = d.wrapping_add(d); // 0x00644889
                t |= 1 << 13;
            }
        }
    }

    // 13..16 — splash block, 0x0064488b
    if i.splash_flag != 0 {
        if p.defender_vf_0x18 {
            d = idiv(d, i.defender_splash_divisor, "0x006448b9"); // 13
            t |= 1 << 14;
        }
        d = div100(i.attacker_splash_percent.wrapping_mul(d)); // 14 — 0x006448ea
        t |= 1 << 15;
        if p.defender_vf_0x18 {
            if p.defender_type_vf_0x10c
                && i.defender_type_0x2b8_bit2
                && i.defender_flags_0x68 & 0x0008_0000 == 0
            {
                d = d.wrapping_mul(3); // 15 — 0x0064494e
                t |= 1 << 16;
            }
            if p.defender_tech_0x143 {
                d = div100(d.wrapping_mul(25)); // 16 — 0x00644980
                t |= 1 << 17;
            }
        }
    }

    // 17 — 0x00644994: river (unmasked z negative).
    if p.defender_vf_0x18 && i.defender_z < 0 {
        d = div256(r.river_modifier.wrapping_mul(d)); // 0x006449c7
        t |= 1 << 18;
    }

    // 18 — 0x006449d7: unit_masks & 1 — armor zeroed, ×1000 scale.
    if p.defender_vf_0x18 && i.defender_flags_0x68 & 1 != 0 {
        arm = 0; // 0x006449fd
        d = d.max(i.attack).wrapping_mul(1000); // 0x00644a11
        t |= 1 << 19;
    }

    // 19 — 0x00644a17
    if p.defender_vf_0x1c && p.defender_carrier_vf_0x184 && !i.game_flag_0x821_bit1 {
        arm = 0; // 0x00644a61
        if !p.defender_carrier_vf_0x20 || p.defender_mount_attack_is_zero {
            d = d.wrapping_mul(4); // 0x00644a98
            t |= 1 << 20;
        }
    }

    // 20 — flank, 0x00644a9b..0x00644b7b
    if p.attacker_vf_0x18
        && p.defender_vf_0x18
        && am & 4 == 0
        && dm & 4 == 0
        && am & 0x1000_0000 == 0
        && dm & 0x1000_0000 == 0
        && am & 0x2000 == dm & 0x2000
        && dm & 0x2000 == 0
    {
        let delta = (i.defender_facing as u32)
            .wrapping_sub(i.attack_dir as u32)
            .wrapping_sub(0x8000_0000); // 0x00644b12
        if delta >= 0x2aaa_aaaa {
            let lvl = flank_level(delta) as i32;
            if lvl != 0 {
                let mut pct = r.flank_bonus;
                if dm & 0x0020_0000 != 0 {
                    pct = div256(r.vehicle_flank_bonus.wrapping_mul(pct)); // 0x00644b42
                } else if dm & 0x1000 != 0 {
                    pct = div256(r.cavalry_flank_bonus.wrapping_mul(pct)); // 0x00644b4f
                }
                d = div100(pct.wrapping_mul(lvl).wrapping_add(100).wrapping_mul(d)); // 0x00644b62
                t |= 1 << 21;
            }
        }
    }

    // 21 — 0x00644b7d: off the ×10 scale, round half up.
    d = d.wrapping_add(5) / 10;
    // 22 — 0x00644b91: armor subtracts here.
    d = d.wrapping_sub(arm);

    // 23 — overkill, 0x00644b94
    if i.overkill_gate != 0 && p.attacker_vf_0x130 && p.attacker_vf_0x18 && p.defender_vf_0x18 {
        let stamp = i.defender_overkill_stamp;
        if stamp != 0
            && i.current_frame.wrapping_sub(stamp) < r.overkill_frames
            && i.attacker_vf_0xe4 != i.defender_word_0xa4
        {
            d = div256(r.overkill_damage.wrapping_mul(d)); // 0x00644c35
            t |= 1 << 22;
            if p.defender_tech_0x109 && !p.attacker_type_vf_0x10c {
                d /= 2; // 0x00644c83
                t |= 1 << 23;
            }
        }
    }

    // 24 — rocky, 0x00644c85
    if dm & 0x0001_0108 != 0 && i.tile_rocky {
        d = div256(r.rocky_modifier.wrapping_mul(d)); // 0x00644ce5
        t |= 1 << 24;
    }

    // 25 — height, 0x00644cf5
    if i.defender_domain != 2
        && i.attacker_domain != 2
        && !p.attacker_type_vf_0x10c
        && i.attacker_z > i.defender_z
    {
        let dz = i.attacker_z.wrapping_sub(i.defender_z);
        let num = dz.wrapping_mul(r.height_bonus).wrapping_mul(d);
        let den = r.height_increment.wrapping_mul(100);
        d = d.wrapping_add(idiv(num, den, "0x00644d78"));
        t |= 1 << 25;
    }

    // 26 — entrenchment, 0x00644d7f
    if p.defender_vf_0x18 && i.defender_flags_0x68 & 0x0200_0000 != 0 && !p.attacker_tech_0x83 {
        let raw = (i.defender_facing_entrench as u32)
            .wrapping_sub(i.attack_dir as u32)
            .wrapping_sub(0x8000_0000);
        let dir = entrench_dir_level(raw);
        if i.splash_flag != 0 || dir == 0 {
            d = div256(r.entrenchment_modifier.wrapping_mul(d)); // 0x00644e44
            t |= 1 << 26;
            if i.defender_flags_0x6c_bit12 {
                d = div256(r.antipater_entrench_bonus.wrapping_mul(d)); // 0x00644e66
            }
        }
    }

    // 27 — 0x00644e7e. UNVERIFIED (Cossack).
    if r.russian_cossack_damage != 0
        && p.step27_player_prop_0xd
        && p.attacker_vf_0x18
        && i.attacker_type_0x40 == 0x1ac
        && (p.defender_vf_0xcc || p.defender_type_vf_0x10c)
    {
        d = div100(r.russian_cossack_damage.wrapping_add(100).wrapping_mul(d)); // 0x00644f04
    }

    // 28 — conditional floor of 1, 0x00644f13
    if d < 1 {
        let masks_agree = (dm & 0x1000_0000) == ((am >> 3) & 0x1000_0000);
        let land_vs_sea = i.attacker_domain == 0 && i.defender_domain == 1;
        if !land_vs_sea && masks_agree && i.splash_flag == 0 {
            d = 1; // 0x00644f61
            t |= 1 << 27;
        }
    }

    // 29 — 0x00644f69: SUPER_IMMUNE target vs air.
    if i.defender_type_id == 0x21d && i.attacker_domain == 2 && r.super_immune != 0 {
        d = 0; // 0x00644f9a
        t |= 1 << 28;
    }

    // 30 — 0x00644f9d: recapturing your own city centre.
    if p.defender_vf_0x20 && p.defender_build_flag && p.defender_build_0x20 && p.recapture_owner_matches {
        t |= 1 << 29;
        return (div256(r.recapture_city_modifier.wrapping_mul(d)), t); // 0x00645024
    }

    (d, t) // 31 — 0x0064503c
}

// ---------------------------------------------------------------------------
// Resolving the chain's inputs from `Save`
// ---------------------------------------------------------------------------

/// The chain's inputs as far as the walked state resolves them, plus the
/// list of predicates that could not be resolved (left at their `Default`).
pub struct ResolvedDamage {
    pub input: DamageInput,
    pub preds: DamagePredicates,
    pub unresolved: Vec<&'static str>,
}

/// Fill [`DamageInput`]/[`DamagePredicates`] for `atk` hitting `def` from
/// the save image. `attack`/`armor` are the virtual getters' returns — see
/// [`unit_attack`] / [`unit_armor`]; what their world-query terms need is
/// reported in `unresolved` rather than guessed.
pub fn resolve_damage(
    save: &Save,
    atk: ObjRef,
    def: ObjRef,
    attack_dir: i32,
    splash_flag: i32,
    overkill_gate: i32,
) -> Option<ResolvedDamage> {
    let a = OImg::of(save, atk)?;
    let d = OImg::of(save, def)?;
    let at = type_rec(save, a.ptype())?;
    let dt = type_rec(save, d.ptype())?;
    let mut unresolved = Vec::new();

    let (attack, un_a) = unit_attack(save, atk);
    let (armor, un_d) = unit_armor(save, def);
    unresolved.extend(un_a);
    unresolved.extend(un_d);

    let atk_type_id = at.i32(0x04).unwrap_or(-1);
    let def_type_id = dt.i32(0x04).unwrap_or(-1);
    let balance_pct = balance(save, atk_type_id, def_type_id).unwrap_or(100) as i32;

    let (tx, ty) = (d.x() >> 8, d.y() >> 8);
    let tile = tile_rec(save, tx, ty);
    let tile_rocky = tile.map(|t| t[0] & 8 != 0).unwrap_or(false);
    let tile_owner = tile.map(|t| t[0x0f] as i8 as i32).unwrap_or(-1);

    let def_is_unit = d.kind.is_unit();
    let input = DamageInput {
        balance_pct,
        attack,
        armor,
        attacker_masks: at.i32(0x1e4).unwrap_or(0) as u32,
        defender_masks: dt.i32(0x1e4).unwrap_or(0) as u32,
        attack_dir,
        splash_flag,
        overkill_gate,
        attacker_player: a.who() as u32,
        attacker_type_id: atk_type_id,
        attacker_domain: at.i32(0x218).unwrap_or(0),
        attacker_splash_percent: at.i32(0x204).unwrap_or(0),
        attacker_type_0x40: at.i32(0x40).unwrap_or(0),
        attacker_z: a.z(),
        attacker_flag8_bit5: a.flags() & 0x20 != 0,
        defender_type_id: def_type_id,
        defender_domain: dt.i32(0x218).unwrap_or(0),
        defender_type_0x2b8_bit2: dt.i32(0x2b8).unwrap_or(0) & 4 != 0,
        defender_splash_divisor: dt.i32(0x308).unwrap_or(1),
        defender_flags_0x68: if def_is_unit { d.i32(0x68) as u32 } else { 0 },
        defender_flags_0x6c_bit12: def_is_unit && d.i32(0x6c) & 0x1000 != 0,
        defender_z: d.z(),
        defender_facing: if def_is_unit { d.i32(0x50) } else { 0 },
        defender_facing_entrench: if def_is_unit { d.i32(0x5c) } else { 0 },
        defender_overkill_stamp: if def_is_unit { d.i32(0x4c) } else { 0 },
        defender_word_0xa4: if def_is_unit { d.i16(0xa4) as i32 } else { 0 },
        attacker_vf_0xe4: get_captain(save, atk),
        current_frame: frame(save),
        game_flag_0x821_bit1: save.game.sem_ptr.get(1).map(|b| b & 2 != 0).unwrap_or(false),
        tile_rocky,
        tile_owner,
    };

    // Build-side predicates: `get_build()` (+0x3c) is the object itself for a
    // Build, null otherwise; its `is_active` is `flags & 4`.
    let def_build_flag = d.kind == ObjKind::Build && d.flags() & 4 != 0;
    let def_build_0x20 = d.kind == ObjKind::Build && d.flags() & 0x20 != 0;
    let preds = DamagePredicates {
        mask_fixup_authorised: {
            unresolved.push("mask_fixup_authorised: Types[0x222] preq via LeaderData::has_preq 0x006db810");
            false
        },
        attacker_vf_0x18: a.kind.is_unit(),
        attacker_vf_0x1c: a.kind.is_build(),
        attacker_vf_0x20: a.kind == ObjKind::Build,
        attacker_vf_0x130: max_range(save, atk) != 0,
        attacker_type_vf_0x10c: at.i32(0x2b4).unwrap_or(0) & 0x20000 != 0,
        attacker_table_vf_0x20: a.kind == ObjKind::Build,
        defender_vf_0x18: def_is_unit,
        defender_vf_0x1c: d.kind.is_build(),
        defender_vf_0x20: d.kind == ObjKind::Build,
        defender_vf_0x120: {
            let (v, un) = unit_attack(save, def);
            unresolved.extend(un);
            v != 0
        },
        defender_vf_0xcc: def_is_unit && dt.i32(0x2b8).unwrap_or(0) & 0x40 != 0,
        defender_vf_0xd0: def_is_unit && dt.i32(0x2b8).unwrap_or(0) & 8 != 0,
        defender_vf_0xd8: {
            if def_is_unit {
                unresolved.push("defender_vf_0xd8: UnitData::is_moving 0x00610af0");
            }
            false
        },
        defender_type_vf_0x10c: dt.i32(0x2b4).unwrap_or(0) & 0x20000 != 0,
        defender_build_flag: def_build_flag,
        defender_build_0x20: def_build_0x20,
        recapture_owner_matches: {
            if def_build_0x20 {
                unresolved.push("recapture_owner_matches: City race vs attacker player 0x00645018");
            }
            false
        },
        defender_carrier_vf_0x184: {
            if d.kind.is_build() {
                unresolved.push("defender_carrier_vf_0x184: (get_wall_data())->+0x184 0x00644a44");
            }
            false
        },
        defender_carrier_vf_0x20: d.kind == ObjKind::Build,
        defender_mount_attack_is_zero: d.kind == ObjKind::Build && dt.i32(0x1e8).unwrap_or(0) == 0,
        attacker_tech_0x42: type_is(&at, 0x42, 0),
        attacker_tech_0x139: type_is(&at, 0x139, 0),
        attacker_tech_0x83: type_is(&at, 0x83, 0),
        defender_tech_0x216: type_is(&dt, 0x216, 0),
        defender_tech_0x143: type_is(&dt, 0x143, 0),
        defender_tech_0x109: type_is(&dt, 0x109, 0),
        step10_bonus_applies: {
            unresolved.push("step10: ObjectData::has_general(0,0x164) 0x00646b00 (HeroesData::find_hero)");
            false
        },
        step11_player_prop_0xf: {
            unresolved.push("step11: LeaderData::has_tribe_bonus(0xf) 0x006e1370");
            false
        },
        step27_player_prop_0xd: {
            unresolved.push("step27: LeaderData::has_tribe_bonus(0xd) 0x006e1370");
            false
        },
        step12_team_differs: {
            unresolved.push("step12: LeaderData::get_target 0x006da000");
            false
        },
        game_mode_is_2: save.game.info.settings.first().map(|&s| s == 2).unwrap_or(false),
    };
    Some(ResolvedDamage { input, preds, unresolved })
}

/// `UnitData::attack` 0x006103c0 / `BuildData::attack` 0x0062e610 /
/// `ObjectData::attack` 0x006469f0 (Wall) — base `type +0x1e8 attack`, with
/// every bonus term a leader/world query. Returns the base and the list of
/// terms left unresolved (each would ADD to the base when its predicate holds):
/// `has_wonder(0x211) → +Constants[0x458]*10`, `has_general(0,0x164) →
/// +[0xc94]*10`, two `has_objmask` × leader-flag terms `+[0x944]*10`, the
/// air `has_wonder(0x21e)` percent `[0x560]`, and the per-tribe general
/// terms `[0xb44] [0xb48] [0xba4] [0xb50] [0xb68] [0xb74] [0xbcc] [0xbc8]
/// [0xb88] [0xc80]`. `ObjectData::attack`'s own Dutch term needs
/// `has_tribe_bonus(0x16)` and `Constants[0x8b8]`.
pub fn unit_attack(save: &Save, r: ObjRef) -> (i32, Vec<&'static str>) {
    let Some(o) = OImg::of(save, r) else { return (0, vec![]) };
    let Some(t) = type_rec(save, o.ptype()) else { return (0, vec![]) };
    let base = t.i32(0x1e8).unwrap_or(0);
    let mut un = vec!["attack: ObjectData::attack 0x006469f0 Dutch term (has_tribe_bonus 0x16)"];
    if base == 0 && o.kind.is_unit() {
        return (0, un); // 0x006103d7: a zero base short-circuits every bonus
    }
    un.push(match o.kind {
        ObjKind::Unit | ObjKind::Animal => "attack: UnitData::attack 0x006103c0 wonder/general/tribe terms",
        ObjKind::Build => "attack: BuildData::attack 0x0062e610 Tower-of-Babel/general terms",
        ObjKind::Wall => "attack: ObjectData::attack 0x006469f0 (Wall)",
    });
    (base, un)
}

/// `UnitData::armor` 0x00610160 — base `UnitData +0x9c myarmor` (i16);
/// `WallData::armor` 0x0063fa60 / `ObjectData::armor` 0x00647db0 — base
/// `type +0x214 armor`. Every bonus is a hero/city proximity term
/// (`HeroesData::find_hero` 0x0073a1b0 → `LeaderData::get_general_upgrade`
/// × `Constants[0xc64]` plus the tribe terms) and stays unresolved.
pub fn unit_armor(save: &Save, r: ObjRef) -> (i32, Vec<&'static str>) {
    let Some(o) = OImg::of(save, r) else { return (0, vec![]) };
    match o.kind {
        ObjKind::Unit | ObjKind::Animal => (
            o.i16(0x9c) as i32,
            vec!["armor: UnitData::armor 0x00610160 general/city proximity terms (find_hero 0x0073a1b0)"],
        ),
        _ => {
            let base = type_rec(save, o.ptype()).and_then(|t| t.i32(0x214)).unwrap_or(0);
            (base, vec!["armor: WallData::armor 0x0063fa60 general/Dutch terms"])
        }
    }
}

/// `UnitData::max_range` 0x0060fe90 is not transcribed; its base is `type
/// +0x1fc max_range` (what `ObjectData::max_range` 0x00646c00 returns).
fn max_range(save: &Save, r: ObjRef) -> i32 {
    OImg::of(save, r)
        .and_then(|o| type_rec(save, o.ptype()))
        .and_then(|t| t.i32(0x1fc))
        .unwrap_or(0)
}

// ===========================================================================
// 2. Object::do_damage 0x0064a480 — the applier
// ===========================================================================

/// Damage handed to `take_damage`: whole hit points and a sixteenth count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sixteenths {
    pub whole: i32,
    pub frac: i8,
}

/// 0x0064a6ea..0x0064a7f1 — scale the chain's result into sixteenths.
///
/// Unit attacker (0x0064a73b..): `D = max(D*scale, 0x100)`; `if ammo_index
/// >= 0 { D /= type.ammo_per_att (+0x208) }`; `D /= type.uber_size (+0x308)`;
/// then `q = D/16; frac = q % 16; whole = q/16` (truncating; the second shift
/// re-uses D's sign bias, which is 0 after the floor).
///
/// Building attacker (0x0064a6fd..): `D = D*scale / ammo_per_att`, no floor,
/// no uber divide, same split (here the sign bias is D's own).
/// Neither a unit nor a building: `D` is passed through unsplit (`frac` 0).
pub fn scale_to_sixteenths(
    d: i32,
    scale: i32,
    ammo_index: i32,
    ammo_per_att: i32,
    uber_size: i32,
    attacker_is_unit: bool,
    attacker_is_building: bool,
) -> Sixteenths {
    fn split(d: i32) -> Sixteenths {
        let bias = (d >> 31) & 0xf;
        let biased = d.wrapping_add(bias);
        let q = biased >> 4;
        let frac = q % 16;
        let whole = (q.wrapping_add(bias)) >> 4;
        Sixteenths { whole, frac: frac as i8 }
    }
    if attacker_is_unit {
        let mut d = d.wrapping_mul(scale);
        if d < 0x101 {
            d = 0x100;
        }
        if ammo_index >= 0 {
            d = idiv(d, ammo_per_att, "0x0064a735");
        }
        d = idiv(d, uber_size, "0x0064a766");
        split(d)
    } else if attacker_is_building {
        split(idiv(d.wrapping_mul(scale), ammo_per_att, "0x0064a70e"))
    } else {
        Sixteenths { whole: d, frac: 0 }
    }
}

/// `Object::do_damage(target_o, target_who, attack_dir, guy, ammo_index,
/// scale_8_8, splash_flag, no_retaliate)` 0x0064a480 for attacker `atk`.
///
/// Transcribed: the early-outs, `get_damage`, the per-leader hit counters
/// (`Game+0x80c/+0x804` per player, under `Game+0x821 & 2`), the overkill
/// stamp (`damage_frame`/`damage_o`/`damage_who`), the `unit_masks & 0x10`
/// doubling, the sixteenth split and the `take_damage` call (vtable +0x16c).
/// Returns `take_damage`'s result (0 survived / capped, 1 died, 2 died with
/// uber overflow forwarded) or `None` when the call did not happen.
///
/// NOT transcribed (effects note each): `Unit::target_opportunity`
/// 0x005fffc0 (retaliation), `Object::attempt_launch` 0x00643a10,
/// `SubObjectData::play_sound`, the presentation band
/// 0x0064a828..0x0064b0d0, the post-hit world transaction
/// 0x0064ba18..0x0064c10c (razing counters, plunder, flamethrower eject),
/// the splash scan 0x0064c10c..0x0064c4dd and the capture attempt
/// 0x0064c4e3..0x0064c558.
#[allow(clippy::too_many_arguments)]
pub fn do_damage(
    save: &mut Save,
    atk: ObjRef,
    def: ObjRef,
    attack_dir: i32,
    // `guy` is only consumed by the presentation band (0x0064a828..).
    _guy: i32,
    ammo_index: i32,
    scale: i32,
    splash_flag: i32,
    no_retaliate: bool,
    effects: &mut Vec<String>,
) -> Option<i32> {
    if scale < 1 {
        return None; // 0x0064a4b7
    }
    let a = OImg::of(save, atk)?;
    // 0x0064a4c6: a unit attacker whose unit_masks & 1 never damages.
    if a.kind.is_unit() && a.i32(0x68) & 1 != 0 {
        return None;
    }
    let res = resolve_damage(save, atk, def, attack_dir, splash_flag, 1)?;
    let rules = CombatRules::from_save(save);
    let mut d = get_damage(&res.input, &res.preds, &rules, &UnreachedTerms::default());
    let tag = format!("do_damage {}:{} -> {}:{}", atk.who, atk.o, def.who, def.o);
    if !res.unresolved.is_empty() {
        effects.push(format!("{tag}: get_damage={d} with {} unresolved predicates: {}", res.unresolved.len(), res.unresolved.join("; ")));
    }
    // out_kind (0x0064a4f4): 2 from get_damage; 4 when ammo_index >= 0 and
    // GraphicPieces::verify_ammo_flags 0x009072e0 holds (graphics table).
    let mut kind: i32 = 2;
    if ammo_index >= 0 {
        effects.push(format!("{tag}: out_kind depends on GraphicPieces::verify_ammo_flags 0x009072e0 (graphics tables) — kept 2"));
    }

    // 0x0064a514: scenario hit statistics, Game+0x80c[who]++ / +0x804[who] += D.
    if save.game.sem_ptr.get(1).map(|b| b & 2 != 0).unwrap_or(false) && a.who() as i32 != def.who {
        let who = a.who();
        let c = game_i32(save, 0x80c + who * 4);
        set_game_i32(save, 0x80c + who * 4, c.wrapping_add(1));
        let s = game_i32(save, 0x804 + who * 4);
        set_game_i32(save, 0x804 + who * 4, s.wrapping_add(d));
        effects.push(format!("Game.scalars hits[{who}] {c} -> {} dmg {s} -> {}", c + 1, s.wrapping_add(d)));
    }

    let dk = OImg::of(save, def)?.kind;
    let a_who = a.who();
    let a_captain = get_captain(save, atk);
    if dk.is_unit() && !no_retaliate {
        // 0x0064a5a3: defender->get_captain(); Unit::target_opportunity(atk_o, atk_who, 0)
        effects.push(format!("{tag}: Unit::target_opportunity 0x005fffc0 (retaliation) not transcribed"));
        let fr = frame(save);
        let overkill_frames = constant(save, 0x5c);
        with_obj(save, def, |_, mut img| {
            if img.i32(0x68) & 0x10 != 0 {
                d = d.wrapping_mul(2); // 0x0064a5dd
            }
            let stamp = img.i32(0x4c);
            if stamp == 0 || overkill_frames <= fr.wrapping_sub(stamp) {
                img.set_i32(0x4c, fr); // damage_frame
                img.set_i16(0xa4, a_captain as i16); // damage_o = attacker's captain
                img.set_u8(0xa9, a_who as u8); // damage_who
                effects.push(format!("{tag}: overkill window re-anchored at frame {fr} (damage_o={a_captain}, damage_who={a_who})"));
            }
        });
    }
    // 0x0064a6b1: !(atk_type.obj_masks & 0x8000000) && flags&1 -> Object::attempt_launch
    effects.push(format!("{tag}: Object::attempt_launch 0x00643a10 / play_sound not transcribed"));

    let at = type_rec(save, a.ptype())?;
    let s = scale_to_sixteenths(
        d,
        scale,
        ammo_index,
        at.i32(0x208).unwrap_or(1),
        at.i32(0x308).unwrap_or(1),
        a.kind.is_unit(),
        a.kind == ObjKind::Build,
    );
    // 0x0064a817..: out_kind forced to 1 when the defender is a building.
    if dk == ObjKind::Build {
        kind = 1;
    }
    effects.push(format!("{tag}: presentation band 0x0064a828..0x0064b0d0 skipped"));
    let r = take_damage(save, def, s.whole, s.frac, kind as u8, ammo_index, 0, attack_dir, atk, splash_flag, effects);
    if r != 0 {
        effects.push(format!("{tag}: post-kill transaction 0x0064ba18.. (razing counters, plunder, score) not transcribed"));
    }
    effects.push(format!("{tag}: splash scan 0x0064c10c and capture attempt 0x0064c4e3 not transcribed"));
    Some(r)
}

// ===========================================================================
// 3. Object::take_damage 0x00652020
// ===========================================================================

/// 0x006522f2..0x00652311 — fold `frac` sixteenths into `damage_frac`
/// (a plain `char`: wraps unclamped) and return the whole points carried.
pub fn accumulate_frac(damage_frac: i8, whole: i32, frac: i8) -> (i32, i8) {
    let u = damage_frac.wrapping_add(frac) as i32; // (char)(damage_frac + frac)
    let carry = (u + ((u >> 31) & 0xf)) >> 4; // trunc(u / 16)
    let rem = u % 16; // signed remainder
    (whole.wrapping_add(carry), rem as i8)
}

/// 0x006528a0..0x00652946 — the hit-point threshold a sub-object of an
/// uber unit dies at: `hits / uber_size` for a non-captain or a captain with
/// live sub-objects, else `hits - ((uber_size-1)*hits)/uber_size` (the
/// captain carries the remainder once it is alone).
pub fn uber_threshold(hits: i32, uber_size: i32, is_captain: bool, curr_uber_size: i32) -> i32 {
    if uber_size <= 1 {
        return hits;
    }
    if !is_captain || curr_uber_size != 1 {
        idiv(hits, uber_size, "0x0065293e")
    } else {
        hits - idiv((uber_size - 1).wrapping_mul(hits), uber_size, "0x0065292e")
    }
}

/// `Object::take_damage(damage, frac, kind, ammo_index, quiet, angle,
/// attacker_o, attacker_who, splash)` 0x00652020 on `def`.
///
/// Returns 0 (survived / city-centre cap), 1 (died), 2 (died and forwarded
/// the overflow to the captain). Writes: `damage`, `damage_frac`, the
/// captain's `SubObject.flags |= 0x10`, `LeaderData.frame_attacked`
/// (+0xa40), the building-under-attack bits (`BuildData.build_masks |=
/// 0x30`, `City.flags |= 0xe`), the city-centre cap (`flags |= 0x10`,
/// `damage = hits`), the death counters (`units_lost` +0x810,
/// `deaths_current_frame` +0xa58, `kills_current_frame` +0xa5a,
/// `units_killed` +0x80c, `score_combat` +0x44, `buildings_lost` +0x81c,
/// `ScenarioData.last_razed[who]`), `die` → `hold_frames`, and the
/// recursive captain `take_damage`.
#[allow(clippy::too_many_arguments)]
pub fn take_damage(
    save: &mut Save,
    def: ObjRef,
    mut damage: i32,
    mut frac: i8,
    kind: u8,
    ammo_index: i32,
    quiet: i32,
    angle: i32,
    atk: ObjRef,
    // `splash` (arg 9) only gates the untranscribed plunder arm at 0x00653018.
    _splash: i32,
    effects: &mut Vec<String>,
) -> i32 {
    let Some(d0) = OImg::of(save, def) else { return 0 };
    if d0.flags() & 1 == 0 {
        return 0; // 0x00652035
    }
    let tag = format!("take_damage {}:{}", def.who, def.o);
    let who = d0.who();
    let dk = d0.kind;
    let dptype = d0.ptype();
    let fr = frame(save);
    // GameLog::say ×2 (0x0065205b, 0x0065207a): desync log only.

    if damage < 1 && frac < 1 {
        frac = 1; // 0x006520a9: every hit lands at least 1/16
    }

    let atk_img = if atk.valid() { OImg::of(save, atk) } else { None };
    // Owned snapshot of the attacker's type scalars (keeps `save` free to mutate).
    struct TypeSnap {
        id: i32,
        obj_masks: u32,
        unit_flags: u32,
        where_: i32,
        is_militia: bool,
    }
    let atk_type: Option<TypeSnap> = atk_img.as_ref().and_then(|a| type_rec(save, a.ptype())).map(|t| TypeSnap {
        id: t.i32(4).unwrap_or(-1),
        obj_masks: t.i32(0x1e4).unwrap_or(0) as u32,
        unit_flags: t.i32(0x2b4).unwrap_or(0) as u32,
        where_: t.i32(0x40).unwrap_or(-1),
        is_militia: type_is(&t, 0x42, 0),
    });

    if quiet == 0 {
        // 0x006520b8: Leader[who].frame_attacked = frame unless (Game+0x820
        // & 4) or Game+0x2b (settings[7]) >= 2.
        let g820 = save.game.sem_ptr.first().copied().unwrap_or(0);
        let g2b = save.game.info.settings.get(0x2b - 0x24).copied().unwrap_or(0);
        if g820 & 4 == 0 && g2b < 2 {
            if let Some(prev) = leader_i32(save, who, 0xa40) {
                set_leader_i32(save, who, 0xa40, fr);
                if prev != fr {
                    effects.push(format!("Leader[{who}].frame_attacked {prev} -> {fr}"));
                }
            }
        }
        // 0x00652113: first damage to a building — 5% smoke/fire flock.
        if d0.i32(0x24) == 0 && dk.is_build() {
            let roll = game_random(save, 0, 0xffff); // DRAW 1
            if roll % 100 < 5 {
                let t = type_rec(save, dptype);
                let is_fort = dk.is_build() && t.as_ref().map(|t| type_is(t, 0x1bb, 0)).unwrap_or(false);
                let special = t.as_ref().map(|t| type_is(t, 0x1b5, 0) || type_is(t, 0x19f, 0)).unwrap_or(false);
                if (is_fort || special) && atk.valid() {
                    let siege = atk_type.as_ref().map(|t| t.unit_flags & 0x20000 != 0).unwrap_or(false);
                    if siege {
                        let r = game_random(save, 0, 0xffff) & 1; // DRAW 2 (0x006521a9), signed % 2
                        effects.push(format!("{tag}: Objects::add_flock 0x0065c0e0 (tile {},{}, -1, {}) not transcribed", d0.x() >> 8, d0.y() >> 8, r + 3));
                    }
                }
            }
        }
        // 0x006521eb: an attacker type with obj_masks & 0x8000000 hitting an
        // object on its owner's own tile auto-declares war
        // (Leader::action_declare 0x006dab50) — a diplomacy transaction.
        if let Some(t) = atk_type.as_ref() {
            if t.obj_masks & 0x0800_0000 != 0 && who as i32 != atk.who {
                effects.push(format!("{tag}: auto war declaration chain (Game::war_allowed 0x00594670, Leader::action_declare 0x006dab50) not transcribed"));
            }
        }
    }

    // 0x006522f2: fold the sixteenths, then damage += whole.
    let (added, new_frac) = accumulate_frac(d0.i8(0x3b), damage, frac);
    damage = added;
    let old_damage = d0.i32(0x24);
    let new_damage = old_damage.wrapping_add(damage);
    with_obj(save, def, |_, mut img| {
        img.set_u8(0x3b, new_frac as u8);
        img.set_i32(0x24, new_damage);
    });
    effects.push(format!("{tag}.damage {old_damage} -> {new_damage} (frac -> {new_frac})"));

    // 0x00652340..0x006523d4: `is_unit() && quiet==0 && !vf0x4c && damage>0
    // && !attacker_is_air` → army emergency bookkeeping. For a Unit, vtable
    // +0x4c is `flags & 1` (0x0046cda0), already true at entry, so the arm
    // is dead on this plane; nothing to write.

    let hits = hits0(save, def);
    if dk != ObjKind::Build {
        // 0x006523e5: unit / wall — AI "home city attacked" group when the
        // owner is a non-human AI (`flags & 4 == 0`, `flags2 & 8 == 0`,
        // get_diff() > 1), the victim is a peasant, and it stands in a city.
        if leader_i32(save, who, 0).unwrap_or(0) & 4 == 0 && leader_i32(save, who, 4).unwrap_or(0) & 8 == 0 {
            effects.push(format!("{tag}: AI city-defence group (LeaderData::get_diff, ObjectsData::find_city_at 0x0065b870, Groups 0x0070f9e0) not transcribed"));
        }
    } else {
        // 0x00652460: a building hit by a foreign attacker is flagged under attack.
        if quiet == 0 && who as i32 != atk.who {
            let mut flag = !atk.valid();
            if !flag {
                // 0x006524d4: (!attacker.is_peasant && !attacker.is(0x42)) || tile owner == attacker
                let peasant = atk_type.as_ref().map(|t| matches!(t.id, 0x32 | 0x33)).unwrap_or(false);
                let militia = atk_type.as_ref().map(|t| t.is_militia).unwrap_or(false);
                let tile_owner = tile_rec(save, d0.x() >> 8, d0.y() >> 8).map(|t| t[0x0f] as i8 as i32).unwrap_or(-1);
                flag = !(peasant || militia) || tile_owner == atk.who;
            }
            if flag {
                let old = d0.u16(0x60);
                with_obj(save, def, |_, mut img| img.set_u16(0x60, old | 0x30));
                if old & 0x30 != 0x30 {
                    effects.push(format!("{tag}.build_masks {old:#x} -> {:#x}", old | 0x30));
                }
            }
            // 0x00652565: City[who][city].flags |= 0xe and the AI defence group.
            let city = d0.i16(0x72);
            if city >= 0 {
                // `CityData +4 flags` is the walked `City.flags` u16.
                if let Some(c) = save.cities.lists.get_mut(who).and_then(|l| l.elems.get_mut(city as usize)).and_then(|c| c.as_mut()) {
                    let old = c.flags;
                    c.flags = old | 0xe;
                    if old & 0xe != 0xe {
                        effects.push(format!("Cities[{who}][{city}].flags {old:#x} -> {:#x}", old | 0xe));
                    }
                }
                effects.push(format!("{tag}: AI city-defence group 0x0070f9e0 not transcribed"));
            }
        }
        // 0x00652690: a full building ejects its contents.
        if hits <= new_damage {
            effects.push(format!("{tag}: ObjectData::can_carry(2) 0x00646c40 / Object::eject_contents 0x0064cd20 not transcribed"));
        }
        // 0x006526c6: a city centre (flags & 0x20) that is active never dies.
        if d0.flags() & 0x20 != 0 && d0.flags() & 4 != 0 {
            if new_damage < hits {
                return 0;
            }
            with_obj(save, def, |_, mut img| {
                img.set_u8(0x08, img.flags() | 0x10);
                img.set_i32(0x24, hits);
            });
            effects.push(format!("{tag}: city centre capped — flags |= 0x10, damage {new_damage} -> {hits}"));
            return 0;
        }
    }

    // 0x00652714: the death threshold.
    let mut threshold = hits;
    if dk.is_unit() {
        if quiet == 0 {
            effects.push(format!("{tag}: Unit::set_in_danger 0x005fcfb0 not transcribed"));
        }
        let uber = type_rec(save, dptype).and_then(|t| t.i32(0x308)).unwrap_or(1);
        if uber > 1 {
            threshold = uber_threshold(hits, uber, is_captain(save, def), curr_uber_size(save, def, 1));
        }
        // 0x00652953: Cossack bonus (has_tribe_bonus(0) on the victim's leader,
        // attacker a unit whose type.where in {0x1ab,0x1ac,0x1b0}) sets the
        // captain's unit_masks |= 0x4000.
        if atk.valid() {
            if let Some(t) = atk_type.as_ref() {
                if matches!(t.where_, 0x1ab | 0x1ac | 0x1b0) && atk_img.as_ref().map(|a| a.kind.is_unit()).unwrap_or(false) {
                    effects.push(format!("{tag}: captain.unit_masks |= 0x4000 gated on LeaderData::has_tribe_bonus(0) 0x006e1370 — not resolved"));
                }
            }
        }
    }

    if new_damage < threshold {
        // 0x00652a04: survived — captain flags |= 0x10. The disband arm
        // (0x00652a2d..) needs `vf+8 && !vf+0x4c`, dead on both planes.
        let cap = get_captain(save, def);
        let cref = ObjRef::new(who as i32, cap);
        with_obj(save, cref, |_, mut img| {
            let f = img.flags();
            if f & 0x10 == 0 {
                img.set_u8(0x08, f | 0x10);
                effects.push(format!("Objects.lists[{}][{}].SubObject.flags {f:#x} -> {:#x}", who, cap, f | 0x10));
            }
        });
        return 0;
    }

    // ---- death -----------------------------------------------------------
    // 0x00652aa8: kind 3 projectile kill of a unit with obj_masks 0x20/0x1000
    // turns the guys toward the shot (Guy::set_angle 0x005d9010 — GuyData
    // walked fields). Not transcribed.
    if dk.is_unit() && kind == 3 {
        effects.push(format!("{tag}: Guy::set_angle 0x005d9010 death-facing loop not transcribed"));
    }
    // 0x00652c05: ammo gpiece and the shot angle (float degrees) for the corpse.
    let ammo_gpiece: i32 = if ammo_index >= 0 { -1 } else { -1 };
    if ammo_index >= 0 {
        effects.push(format!("{tag}: DeathObj ammo_gpiece from Objects.ammo[{ammo_index}]+8 and angle_to_degrees(find_angle) not resolved"));
    }
    let unit_mask_1 = if dk.is_unit() { d0.i32(0x68) & 1 } else { 0 };
    if dk.is_unit() && is_captain(save, def) {
        // 0x00652c9d: a caravan captain on land dying off-ally territory → Caravans::new_danger.
        if type_rec(save, dptype).map(|t| t.i32(0x2b8).unwrap_or(0) & 8 != 0 && t.i32(0x218) == Some(0)).unwrap_or(false) {
            effects.push(format!("{tag}: Caravans::new_danger 0x0073e0c0 not transcribed"));
        }
    }

    if dk.is_build() {
        // 0x00652dd0: buildings_lost++, last_razed[who] = o.
        if let Some(v) = leader_i32(save, who, 0x81c) {
            set_leader_i32(save, who, 0x81c, v.wrapping_add(1));
            effects.push(format!("Leader[{who}].buildings_lost {v} -> {}", v + 1));
        }
        if save.scenario.g4.len() >= (who + 1) * 4 {
            let old = i32::from_le_bytes(save.scenario.g4[who * 4..who * 4 + 4].try_into().unwrap());
            save.scenario.g4[who * 4..who * 4 + 4].copy_from_slice(&def.o.to_le_bytes());
            effects.push(format!("ScenarioData.last_razed[{who}] {old} -> {}", def.o));
        }
    } else if is_captain(save, def) && curr_uber_size(save, def, 0) == 1 && unit_mask_1 == 0 {
        // 0x00652e4e: units_lost++ …
        if let Some(v) = leader_i32(save, who, 0x810) {
            set_leader_i32(save, who, 0x810, v.wrapping_add(1));
            effects.push(format!("Leader[{who}].units_lost {v} -> {}", v + 1));
        }
        if atk.valid() {
            // 0x00652e7f: deaths/kills this frame, units_killed, combat score.
            if let Some(v) = leader_i16(save, who, 0xa58) {
                set_leader_i16(save, who, 0xa58, v.wrapping_add(1));
            }
            let aw = atk.who as usize;
            if let Some(v) = leader_i16(save, aw, 0xa5a) {
                set_leader_i16(save, aw, 0xa5a, v.wrapping_add(1));
            }
            if let Some(v) = leader_i32(save, aw, 0x80c) {
                set_leader_i32(save, aw, 0x80c, v.wrapping_add(1));
                effects.push(format!("Leader[{aw}].units_killed {v} -> {}", v + 1));
            }
            // 0x00652ec0: a carried unit (type unit_flags & 0x10) credits its
            // carrier (inside_down_who / inside_down).
            let mut victim = def;
            if dk.is_unit() && type_rec(save, dptype).map(|t| t.i32(0x2b4).unwrap_or(0) & 0x10 != 0).unwrap_or(false) {
                victim = ObjRef::new(d0.i8(0x3e) as i32, d0.i16(0x28) as i32);
            }
            if victim.o >= 0 && OImg::of(save, victim).map(|v| v.kind.is_unit()).unwrap_or(false) {
                effects.push(format!("{tag}: Leader[{aw}].score_combat ± UnitTypeData::get_kill_value 0x0061d3d0 / 5 (needs LeaderData::is_ally 0x006edb50, TypeData::get_cost) not transcribed"));
            }
            // 0x00652f6c..0x00653354: Unit::plunder 0x00605b00 (two arms).
            effects.push(format!("{tag}: Unit::plunder 0x00605b00 arms not transcribed"));
        }
    }

    // 0x006533b2: overflow, die, cascade.
    let overflow = new_damage.wrapping_sub(threshold);
    object_die(save, def, kind, ammo_gpiece, effects);
    if unit_mask_1 == 0 {
        return 1;
    }
    if overflow > 0 {
        let cap = get_captain(save, def);
        if cap >= 0 && cap != def.o {
            let cref = ObjRef::new(who as i32, cap);
            // Literal retail arguments: the "attacker" passed down is (captain, who).
            take_damage(save, cref, overflow, 0, kind, ammo_index, quiet, angle, cref, 0, effects);
        }
    }
    2
}

// ===========================================================================
// 4. Death: Object::die, Objects::add_death, Objects::remove
// ===========================================================================

/// `Unit::die` 0x0060eda0 / `Wall::die` 0x0063f6c0 → `Object::die` 0x00647080:
/// `close(kind, ammo_gpiece, angle)` (vtable +0x150 — `Unit::close`
/// 0x0060ee50 / `Build::close` 0x00628980 / `Wall::close` 0x0063f6e0, NOT
/// transcribed: population, group and `Objects::add_death` side), then
/// `hold_frames = max(hold_frames, 1, max over live Ammo aimed here of
/// (total_time - cur_time) + 1 + [0x00c0a888])` — only when `type.max_range
/// != 0`. `[0x00c0a888]` is an ammo-system global not in the walked image;
/// the ammo term is reported, not applied.
pub fn object_die(save: &mut Save, r: ObjRef, kind: u8, ammo_gpiece: i32, effects: &mut Vec<String>) {
    let Some(o) = OImg::of(save, r) else { return };
    let tag = format!("die {}:{}", r.who, r.o);
    effects.push(format!("{tag}: close(kind={kind}, gpiece={ammo_gpiece}) (Unit::close 0x0060ee50 / Build::close 0x00628980 — Objects::add_death side) not transcribed"));
    let mut hold: i32 = 1;
    let ranged = type_rec(save, o.ptype()).and_then(|t| t.i32(0x1fc)).unwrap_or(0) != 0;
    if ranged {
        let aimed = save
            .objects
            .ammo
            .elems
            .iter()
            .flatten()
            .filter(|a| a.body.len() >= 0x44 && a.body[0] & 3 != 0)
            .filter(|a| {
                let tgt_o = i32::from_le_bytes(a.body[0x3c..0x40].try_into().unwrap());
                let tgt_who = i32::from_le_bytes(a.body[0x38..0x3c].try_into().unwrap());
                tgt_o == r.o && tgt_who == r.who
            })
            .count();
        if aimed > 0 {
            effects.push(format!("{tag}: {aimed} live Ammo aimed here — hold_frames ammo term ([0x00c0a888] global) not applied"));
        }
    }
    let cur = o.u16(0x32) as i32;
    if cur > hold {
        hold = cur;
    }
    if cur != hold {
        with_obj(save, r, |_, mut img| img.set_u16(0x32, hold as u16));
        effects.push(format!("{tag}.hold_frames {cur} -> {hold}"));
    }
}

/// `Objects::add_death` 0x00653b60 slot policy over `Objects.deaths`
/// (`ObjectArray<DeathObj>`, 0xa4-byte records; the walked row is
/// `DeathObjData +4..+75`): first `valid == 0` slot wins; otherwise the
/// oldest `first_frame` among corpses whose type does not
/// `blocks_while_dead()` (`unit_flags & 0x800000`); if every slot is
/// occupied and blocking, `best` stays 0 (`best_frame` was `frame + 1`).
/// Returns the chosen slot and whether it held a blocking corpse that
/// `DeathObj::clear_blocking` 0x008d4ac0 must evict first.
pub fn add_death_slot(save: &Save) -> (usize, bool) {
    let fr = frame(save);
    let n = save.objects.deaths.len.max(0) as usize;
    let mut best = 0usize;
    let mut best_frame = fr.wrapping_add(1);
    let mut i = 0usize;
    while i < n {
        let Some(row) = save.objects.deaths.elems.get(i) else { break };
        if row.valid == 0 {
            break;
        }
        let who = row.body.get(0x14..0x18).map(|v| i32::from_le_bytes(v.try_into().unwrap())).unwrap_or(-1);
        let o = row.body.get(0x18..0x1c).map(|v| i32::from_le_bytes(v.try_into().unwrap())).unwrap_or(-1);
        let blocks = death_type_blocks(save, ObjRef::new(who, o));
        if !blocks {
            let first = row.body.first().map(|_| i32::from_le_bytes(row.body[0..4].try_into().unwrap())).unwrap_or(0);
            if first < best_frame {
                best_frame = first;
                best = i;
            }
        }
        i += 1;
    }
    let slot = if i == n { best } else { i };
    let evict = save
        .objects
        .deaths
        .elems
        .get(slot)
        .map(|row| row.valid != 0 && {
            let who = row.body.get(0x14..0x18).map(|v| i32::from_le_bytes(v.try_into().unwrap())).unwrap_or(-1);
            let o = row.body.get(0x18..0x1c).map(|v| i32::from_le_bytes(v.try_into().unwrap())).unwrap_or(-1);
            death_type_blocks(save, ObjRef::new(who, o))
        })
        .unwrap_or(false);
    (slot, evict)
}

fn death_type_blocks(save: &Save, r: ObjRef) -> bool {
    OImg::of(save, r)
        .and_then(|o| type_rec(save, o.ptype()))
        .map(|t| t.i32(0x2b4).unwrap_or(0) & 0x0080_0000 != 0)
        .unwrap_or(false)
}

/// `Objects::add_death(who, o, guy, ammo_gpiece, ammo_index, ammo_angle)`
/// 0x00653b60 → `DeathObj::init` 0x008d4c60: picks the slot
/// ([`add_death_slot`]) and writes the `DeathObjData` row from the dying
/// object's guy `guy`:
///
/// ```text
///   valid=1  first_frame=frame  cur_anim=kind (+4 when kind<15)
///   x,y = guy +0x0c/+0x10 (guy world position), z = guy +0x18 (land) / 0 (sea)
///   who,o  gpiece (graphics: vtable +0x178 get_gpiece)  ammo_gpiece  ammo_angle
///   new_angle = ammo_index when Objects.ammo[ammo_index] flags & 1, else -1
///   turret_angles[4] = guy +0x20..+0x30   cur_frame=0   skel_gpiece (graphics)
///   node_flags=0xffff   unit_crew = guy >= type.squad_size ? guy - squad_size : -1
/// ```
///
/// `gpiece`/`skel_gpiece`/the anim-packet checks come from the graphics
/// tables (`[0x00c06214]+0x728`) that are not in the save; they are written
/// as `-1`/`0` and reported. The guy position fields need the GuyData row
/// (`Unit.guys[guy]`, 155 B at +0x08): `x,y,z` are `+0x0c,+0x10,+0x18`
/// (XOR 0x63637 like every Coord), turret angles `+0x20..+0x30`.
pub fn add_death(save: &mut Save, r: ObjRef, guy: i32, kind: u8, ammo_gpiece: i32, ammo_index: i32, ammo_angle: f32, effects: &mut Vec<String>) -> Option<usize> {
    let o = OImg::of(save, r)?;
    let t = type_rec(save, o.ptype())?;
    let squad = t.i32(0x304).unwrap_or(0);
    let sea = t.i32(0x218) == Some(1);
    let row = guy_row(save, r, guy as usize).map(|g| g.to_vec());
    let (slot, evict) = add_death_slot(save);
    if evict {
        effects.push(format!("Objects.deaths[{slot}]: evicting blocking corpse — DeathObj::clear_blocking 0x008d4ac0 not transcribed"));
    }
    let fr = frame(save);
    let mut body = vec![0u8; 71];
    let put = |b: &mut Vec<u8>, off: usize, v: i32| b[off - 4..off].copy_from_slice(&v.to_le_bytes());
    put(&mut body, 0x08, fr); // first_frame
    put(&mut body, 0x0c, if kind < 15 { kind as i32 + 4 } else { kind as i32 }); // cur_anim
    let (gx, gy, gz) = match &row {
        Some(g) if g.len() == 155 => (
            i32::from_le_bytes(g[0x0c - 8..0x10 - 8].try_into().unwrap()),
            i32::from_le_bytes(g[0x10 - 8..0x14 - 8].try_into().unwrap()),
            i32::from_le_bytes(g[0x18 - 8..0x1c - 8].try_into().unwrap()),
        ),
        _ => (o.i32(0x10), o.i32(0x14), o.i32(0x0c)),
    };
    put(&mut body, 0x10, gx);
    put(&mut body, 0x14, gy);
    put(&mut body, 0x18, if sea { 0 } else { gz });
    put(&mut body, 0x1c, r.who);
    put(&mut body, 0x20, r.o);
    put(&mut body, 0x24, -1); // gpiece — graphics
    put(&mut body, 0x28, ammo_gpiece);
    body[0x28..0x2c].copy_from_slice(&ammo_angle.to_le_bytes()); // ammo_angle (+0x28)
    let new_angle = if ammo_index >= 0
        && save.objects.ammo.elems.get(ammo_index as usize).and_then(|a| a.as_ref()).map(|a| a.body.first().copied().unwrap_or(0) & 1 != 0).unwrap_or(false)
    {
        ammo_index
    } else {
        -1
    };
    put(&mut body, 0x30, new_angle);
    if let Some(g) = &row {
        if g.len() == 155 {
            for k in 0..4 {
                let v = i32::from_le_bytes(g[0x20 - 8 + k * 4..0x24 - 8 + k * 4].try_into().unwrap());
                put(&mut body, 0x34 + k * 4, v);
            }
        }
    }
    put(&mut body, 0x44, 0); // cur_frame
    put(&mut body, 0x48, -1); // skel_gpiece — graphics
    body[0x48 - 4..0x4a - 4].copy_from_slice(&0xffffu16.to_le_bytes()); // node_flags
    body[0x4a - 4] = if guy < squad { 0xff } else { (guy - squad) as u8 }; // unit_crew
    effects.push(format!("Objects.deaths[{slot}]: DeathObj for {}:{} guy {guy} (gpiece/skel_gpiece from graphics tables left -1)", r.who, r.o));
    if let Some(dst) = save.objects.deaths.elems.get_mut(slot) {
        dst.valid = 1;
        dst.body = body;
    }
    Some(slot)
}

/// `Objects::remove(who, o, _)` 0x00658980 — after an object's slot is
/// released, roll the owner's high-water mark back over trailing inactive,
/// unheld slots: `unit_mark` (also requires `o_up < 0`, i.e. captain),
/// `build_mark` (floor 2000), `wall_mark` (floor 3000). The mark arrays are
/// the `Objects.scalars` block (`Objects::walk_data` 0x006541e0). Which
/// mark is tested is the removed object's plane (`vf+8` unit, `+0x10`
/// build, `+0xc` wall).
pub fn objects_remove(save: &mut Save, r: ObjRef, effects: &mut Vec<String>) {
    let Some(o) = OImg::of(save, r) else { return };
    let who = r.who as usize;
    if who >= 9 {
        return;
    }
    match o.kind {
        ObjKind::Unit | ObjKind::Animal => {
            let mut m = unit_mark(save, who);
            let start = m;
            while m > 0 {
                let Some(top) = OImg::of(save, ObjRef::new(r.who, m - 1)) else { break };
                if top.flags() & 1 != 0 || top.u16(0x32) != 0 || top.i16(0x8e) >= 0 {
                    break;
                }
                m -= 1;
            }
            if m != start {
                set_mark(save, 16 + who * 4, m);
                effects.push(format!("Objects.unit_mark[{who}] {start} -> {m}"));
            }
        }
        ObjKind::Build => {
            let mut m = build_mark(save, who);
            let start = m;
            while m > 2000 {
                let Some(top) = OImg::of(save, ObjRef::new(r.who, m - 1)) else { break };
                if top.flags() & 1 != 0 || top.u16(0x32) != 0 {
                    break;
                }
                m -= 1;
            }
            if m != start {
                set_mark(save, 16 + 36 + who * 4, m);
                effects.push(format!("Objects.build_mark[{who}] {start} -> {m}"));
            }
        }
        ObjKind::Wall => {
            let mut m = wall_mark(save, who);
            let start = m;
            while m > 3000 {
                let Some(top) = OImg::of(save, ObjRef::new(r.who, m - 1)) else { break };
                if top.flags() & 1 != 0 {
                    if top.u16(0x32) != 0 {
                        break;
                    }
                    // retail: an active top slot keeps looping only while
                    // the loop condition `flags & 1 == 0` fails — i.e. stops.
                    break;
                }
                if top.u16(0x32) != 0 {
                    break;
                }
                m -= 1;
            }
            if m != start {
                set_mark(save, 16 + 72 + who * 4, m);
                effects.push(format!("Objects.wall_mark[{who}] {start} -> {m}"));
            }
        }
    }
}

// ===========================================================================
// 5. Unit::do_attack 0x005f1b80
// ===========================================================================

/// `Unit::do_attack(UnitOrder*)` 0x005f1b80 for the unit at `(owner, slot)`;
/// `order` is the serialized order payload (`UnitOrder`: `+0x08 o`, `+0x0c
/// who`, `+0x1c flag` byte — read through the order's `+0x50 get_data`).
///
/// Transcribed structure (nothing on this path draws from the LCG):
/// 1. `is(0x15f, 1)` (a transport type): if the target is active and the
///    game is not paused (`Game+0x550 & 0xf`... see below) walk the target's
///    `inside_down` chain calling `Unit::add_order_front` 0x005e48c0 — a
///    garrison order chain; else `Unit::finish_order` 0x005e2cb0.
/// 2. `type.max_range == 0 && is(0x140, 0)`: melee rush —
///    `Object::valid_target` 0x00648ba0, `Unit::find_attack_pos`
///    0x00601280 ×2, `Unit::set_dest` 0x00616ed0.
/// 3. `type.attack == 0 && cavarch_o >= 0`: a non-combat unit's escort
///    repositioning — `Unit::is_cav_archer`-style band selection,
///    `find_angle`, `Unit::find_spot` 0x0061de70, `Unit::set_dest`.
/// 4. Otherwise `Unit::fight(o, who, flag, 0, 0)` 0x005fd4d0, with the
///    cav-archer (`unit_flags & 0x200000`) `cavarch_o/who/uid` + `unit_masks2
///    |= 0x100` bookkeeping before it, and the `unit_flags & 0x400`
///    stance-2 double-shot (`type.max_range = second_max_range` around a
///    second `fight`) / `Unit::fight_again`-style follow-up
///    (`FUN_005ff4b0`) after it.
///
/// Only step 4's cav-archer field writes are performed here; `Unit::fight`
/// is the 8,157-byte attack cycle and is NOT transcribed — its `do_damage`
/// call sites are 0x005fee5e / 0x005fef31 (one per guy `0..guy_mark`,
/// `do_damage(target, attack_dir, guy, -1, 0x100, 0, 0)`), the hit path
/// 0x005ff6e9 for splash secondaries, and `recharging = recharge()` is the
/// byte store at 0x005ff0a4.
pub fn do_attack(save: &mut Save, owner: usize, slot: usize, order: &[u8], effects: &mut Vec<String>) {
    let me = ObjRef::new(owner as i32, slot as i32);
    let Some(u) = OImg::of(save, me) else { return };
    if !u.kind.is_unit() {
        return;
    }
    let (is_transport, melee_rush, no_attack, unit_flags) = {
        let Some(t) = type_rec(save, u.ptype()) else { return };
        (
            type_is(&t, 0x15f, 1),
            t.i32(0x1fc) == Some(0) && type_is(&t, 0x140, 0),
            t.i32(0x1e8) == Some(0),
            t.i32(0x2b4).unwrap_or(0) as u32,
        )
    };
    let tag = format!("Objects.lists[{owner}][{slot}]");
    let rd32 = |off: usize| order.get(off..off + 4).map(|v| i32::from_le_bytes(v.try_into().unwrap())).unwrap_or(-1);
    let target = ObjRef::new(rd32(0x0c), rd32(0x08));
    let flag = order.get(0x1c).copied().unwrap_or(0);

    if is_transport {
        effects.push(format!("{tag}: do_attack transport arm (Unit::add_order_front 0x005e48c0 chain / finish_order 0x005e2cb0) not transcribed"));
        return;
    }
    if melee_rush {
        effects.push(format!("{tag}: do_attack melee-rush arm (valid_target 0x00648ba0, find_attack_pos 0x00601280, set_dest 0x00616ed0) not transcribed"));
        return;
    }
    if no_attack && u.i16(0xa2) >= 0 {
        effects.push(format!("{tag}: do_attack escort arm (find_spot 0x0061de70, set_dest) not transcribed"));
        return;
    }
    if flag != 0 && unit_flags & 0x0020_0000 != 0 {
        // 0x005f2179: cav-archer bookkeeping.
        let tgt_uid = OImg::of(save, target).filter(|o| o.flags() & 1 != 0).map(|o| o.u16(0x30));
        with_obj(save, me, |_, mut img| {
            img.set_i16(0xa2, target.o as i16);
            img.set_u8(0xa8, target.who as u8);
            let m2 = img.i32(0x6c);
            match tgt_uid {
                Some(uid) => {
                    img.set_i32(0x6c, m2 | 0x100);
                    img.set_u16(0xa6, uid);
                }
                None => img.set_i32(0x6c, m2 & !0x100),
            }
        });
        effects.push(format!("{tag}.cavarch_o/who/uid <- {}:{} (unit_masks2 bit 0x100 {})", target.who, target.o, tgt_uid.is_some()));
    }
    if flag == 0 && unit_flags & 0x400 != 0 {
        effects.push(format!("{tag}: stance-2 double shot (second_max_range) around Unit::fight — get_combat_stance 0x00610a30 not resolved"));
    }
    effects.push(format!("{tag}: Unit::fight({}:{}, flag={flag}) 0x005fd4d0 not transcribed", target.who, target.o));
}

// ===========================================================================
// Image accessors and global-state readers
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjKind {
    Unit,
    Animal,
    Build,
    Wall,
}

impl ObjKind {
    fn is_unit(self) -> bool {
        matches!(self, ObjKind::Unit | ObjKind::Animal)
    }
    fn is_build(self) -> bool {
        matches!(self, ObjKind::Build | ObjKind::Wall)
    }
}

/// Read-only image view (copies the serialized ranges) of any object plane.
#[derive(Clone)]
struct OImg {
    kind: ObjKind,
    flags: u8,
    sub: Vec<u8>,
    mid: Vec<u8>,
    body: Vec<u8>,
    wall: Vec<u8>,
}

impl OImg {
    fn of(save: &Save, r: ObjRef) -> Option<OImg> {
        if !r.valid() {
            return None;
        }
        let l = save.objects.lists.get(r.who as usize)?;
        let e = l.elems.get(r.o as usize)?.as_ref()?;
        let (kind, u, b): (ObjKind, Option<&Unit>, Option<&Build>) = match e {
            Obj::Unit(u) => (ObjKind::Unit, Some(u), None),
            Obj::Animal(a) => (ObjKind::Animal, Some(&a.unit), None),
            Obj::Build(b) => (if r.o >= 3000 { ObjKind::Wall } else { ObjKind::Build }, None, Some(b)),
        };
        let img = match (u, b) {
            (Some(u), _) => OImg { kind, flags: u.base.sub.flags, sub: u.base.sub.body.clone(), mid: u.base.mid.clone(), body: u.body.clone(), wall: vec![] },
            (_, Some(b)) => OImg { kind, flags: b.base.sub.flags, sub: b.base.sub.body.clone(), mid: b.base.mid.clone(), body: b.body.clone(), wall: b.wall_body.clone() },
            _ => return None,
        };
        if img.sub.len() != 19 || img.mid.len() != 34 {
            return None;
        }
        Some(img)
    }
    fn bytes(&self, off: usize, n: usize) -> Option<&[u8]> {
        let (v, i): (&Vec<u8>, usize) = match (self.kind.is_unit(), off) {
            (_, 0x09..=0x1b) => (&self.sub, off - 0x09),
            (_, 0x20..=0x41) => (&self.mid, off - 0x20),
            (true, 0x48..=0xb6) => (&self.body, off - 0x48),
            (false, 0x48..=0x65) => (&self.wall, off - 0x48),
            (false, 0x70..=0x85) => (&self.body, off - 0x70),
            _ => return None,
        };
        v.get(i..i + n)
    }
    fn flags(&self) -> u8 {
        self.flags
    }
    fn u8(&self, off: usize) -> u8 {
        if off == 0x08 {
            return self.flags;
        }
        self.bytes(off, 1).map(|b| b[0]).unwrap_or(0)
    }
    fn i8(&self, off: usize) -> i8 {
        self.u8(off) as i8
    }
    fn i16(&self, off: usize) -> i16 {
        self.bytes(off, 2).map(|b| i16::from_le_bytes([b[0], b[1]])).unwrap_or(0)
    }
    fn u16(&self, off: usize) -> u16 {
        self.i16(off) as u16
    }
    fn i32(&self, off: usize) -> i32 {
        self.bytes(off, 4).map(|b| i32::from_le_bytes(b.try_into().unwrap())).unwrap_or(0)
    }
    fn who(&self) -> usize {
        self.u8(0x09) as usize
    }
    fn ptype(&self) -> i32 {
        self.i32(0x18)
    }
    fn z(&self) -> i32 {
        self.i32(0x0c) ^ 0x63637
    }
    fn x(&self) -> i32 {
        self.i32(0x10) ^ 0x63637
    }
    fn y(&self) -> i32 {
        self.i32(0x14) ^ 0x63637
    }
}

/// Mutable image view over the object taken out of its slot.
struct OMut<'a> {
    kind: ObjKind,
    flags: &'a mut u8,
    sub: &'a mut Vec<u8>,
    mid: &'a mut Vec<u8>,
    body: &'a mut Vec<u8>,
    wall: Option<&'a mut Vec<u8>>,
}

impl OMut<'_> {
    fn slot(&mut self, off: usize) -> Option<(&mut Vec<u8>, usize)> {
        Some(match (self.kind.is_unit(), off) {
            (_, 0x09..=0x1b) => (&mut *self.sub, off - 0x09),
            (_, 0x20..=0x41) => (&mut *self.mid, off - 0x20),
            (true, 0x48..=0xb6) => (&mut *self.body, off - 0x48),
            (false, 0x48..=0x65) => (self.wall.as_deref_mut()?, off - 0x48),
            (false, 0x70..=0x85) => (&mut *self.body, off - 0x70),
            _ => return None,
        })
    }
    fn flags(&self) -> u8 {
        *self.flags
    }
    fn i32(&mut self, off: usize) -> i32 {
        self.slot(off).and_then(|(v, i)| v.get(i..i + 4).map(|b| i32::from_le_bytes(b.try_into().unwrap()))).unwrap_or(0)
    }
    fn set_u8(&mut self, off: usize, x: u8) {
        if off == 0x08 {
            *self.flags = x;
            return;
        }
        if let Some((v, i)) = self.slot(off) {
            if i < v.len() {
                v[i] = x;
            }
        }
    }
    fn set_i16(&mut self, off: usize, x: i16) {
        if let Some((v, i)) = self.slot(off) {
            if i + 2 <= v.len() {
                v[i..i + 2].copy_from_slice(&x.to_le_bytes());
            }
        }
    }
    fn set_u16(&mut self, off: usize, x: u16) {
        self.set_i16(off, x as i16)
    }
    fn set_i32(&mut self, off: usize, x: i32) {
        if let Some((v, i)) = self.slot(off) {
            if i + 4 <= v.len() {
                v[i..i + 4].copy_from_slice(&x.to_le_bytes());
            }
        }
    }
}

/// Run `f` over a mutable image of `Objects.lists[r.who][r.o]`; the object
/// is taken out of its slot for the duration so `f` may read `&Save`.
fn with_obj(save: &mut Save, r: ObjRef, f: impl FnOnce(&Save, OMut<'_>)) {
    if !r.valid() {
        return;
    }
    let (who, o) = (r.who as usize, r.o as usize);
    let Some(list) = save.objects.lists.get_mut(who) else { return };
    let Some(cell) = list.elems.get_mut(o) else { return };
    let taken = cell.take();
    match taken {
        Some(Obj::Unit(mut u)) => {
            f(save, OMut { kind: ObjKind::Unit, flags: &mut u.base.sub.flags, sub: &mut u.base.sub.body, mid: &mut u.base.mid, body: &mut u.body, wall: None });
            save.objects.lists[who].elems[o] = Some(Obj::Unit(u));
        }
        Some(Obj::Animal(mut a)) => {
            f(save, OMut { kind: ObjKind::Animal, flags: &mut a.unit.base.sub.flags, sub: &mut a.unit.base.sub.body, mid: &mut a.unit.base.mid, body: &mut a.unit.body, wall: None });
            save.objects.lists[who].elems[o] = Some(Obj::Animal(a));
        }
        Some(Obj::Build(mut b)) => {
            let kind = if o >= 3000 { ObjKind::Wall } else { ObjKind::Build };
            f(save, OMut { kind, flags: &mut b.base.sub.flags, sub: &mut b.base.sub.body, mid: &mut b.base.mid, body: &mut b.body, wall: Some(&mut b.wall_body) });
            save.objects.lists[who].elems[o] = Some(Obj::Build(b));
        }
        None => {}
    }
}

fn guy_row(save: &Save, r: ObjRef, guy: usize) -> Option<&[u8]> {
    let l = save.objects.lists.get(r.who as usize)?;
    let u = match l.elems.get(r.o as usize)?.as_ref()? {
        Obj::Unit(u) => u,
        Obj::Animal(a) => &a.unit,
        Obj::Build(_) => return None,
    };
    u.guys.elems.get(guy)?.as_ref().map(|g| g.data.as_slice())
}

/// `UnitData::get_captain` 0x00610ab0: follow `o_up` (+0x8e) until it is
/// negative; `ObjectData::get_captain` 0x00472400 (Build/Wall) is `o`.
fn get_captain(save: &Save, r: ObjRef) -> i32 {
    let mut cur = r;
    for _ in 0..64 {
        let Some(o) = OImg::of(save, cur) else { return cur.o };
        if !o.kind.is_unit() {
            return cur.o;
        }
        let up = o.i16(0x8e);
        if up < 0 {
            return cur.o;
        }
        cur = ObjRef::new(cur.who, up as i32);
    }
    cur.o
}

/// `UnitData::is_captain` 0x0046ceb0: `o_up < 0`; Build/Wall: 1.
fn is_captain(save: &Save, r: ObjRef) -> bool {
    OImg::of(save, r).map(|o| !o.kind.is_unit() || o.i16(0x8e) < 0).unwrap_or(false)
}

/// `UnitData::curr_uber_size(from_self)` 0x0060a760: walk to the captain
/// (unless `from_self`), then count the live `o_down` (+0x90) chain + 1.
fn curr_uber_size(save: &Save, r: ObjRef, from_self: i32) -> i32 {
    let mut cur = r;
    if from_self == 0 {
        cur = ObjRef::new(r.who, get_captain(save, r));
    }
    let mut n = 1;
    for _ in 0..64 {
        let Some(o) = OImg::of(save, cur) else { break };
        if !o.kind.is_unit() {
            break;
        }
        let down = o.i16(0x90);
        if down < 0 {
            break;
        }
        let next = ObjRef::new(cur.who, down as i32);
        match OImg::of(save, next) {
            Some(d) if d.flags() & 1 != 0 => {
                n += 1;
                cur = next;
            }
            _ => break,
        }
    }
    n
}

/// `hits(0)` (vtable +0x11c): `UnitData::hits` 0x00610890 = `myhits`
/// (+0x20); `WallData::hits` 0x00642bb0 = `construct_hits` (+0x54) for
/// arg 0; `BuildData::hits` 0x0062e740 = `construct_hits`, scaled while a
/// Wonder (`0x29a`/`0x286` in the queue) is under construction — that
/// scaling (`ObjectData::construct_time` 0x006508c0) is not transcribed.
fn hits0(save: &Save, r: ObjRef) -> i32 {
    let Some(o) = OImg::of(save, r) else { return 0 };
    if o.kind.is_unit() {
        o.i32(0x20)
    } else {
        o.i32(0x54)
    }
}

/// `Game+0x550` (`Game::frame`).
fn frame(save: &Save) -> i32 {
    i32::from_le_bytes(save.game.scalars[0..4].try_into().unwrap())
}

fn game_i32(save: &Save, image_off: usize) -> i32 {
    let o = image_off - 0x550;
    save.game.scalars.get(o..o + 4).map(|v| i32::from_le_bytes(v.try_into().unwrap())).unwrap_or(0)
}

fn set_game_i32(save: &mut Save, image_off: usize, x: i32) {
    let o = image_off - 0x550;
    if let Some(v) = save.game.scalars.get_mut(o..o + 4) {
        v.copy_from_slice(&x.to_le_bytes());
    }
}

/// `Constants` i32 at image offset `off` (`Rules.constants`, falling back
/// to the direct `Save.constants` block).
fn constant(save: &Save, off: usize) -> i32 {
    let rules = &save.rules_tail.rules.constants;
    if rules.len() >= off + 4 {
        return i32::from_le_bytes(rules[off..off + 4].try_into().unwrap());
    }
    save.constants.get(off..off + 4).map(|v| i32::from_le_bytes(v.try_into().unwrap())).unwrap_or(0)
}

/// `LeaderData` i32 at image offset `off` (`flags` +0, `flags2` +4, body
/// +0x08..). `None` when the slot has no walked body.
fn leader_i32(save: &Save, who: usize, off: usize) -> Option<i32> {
    let l = save.leaders.slots.get(who)?;
    Some(match off {
        0 => l.flags,
        4 => l.flags2,
        _ => i32::from_le_bytes(l.body.get(off - 8..off - 4)?.try_into().unwrap()),
    })
}

fn set_leader_i32(save: &mut Save, who: usize, off: usize, x: i32) {
    if let Some(l) = save.leaders.slots.get_mut(who) {
        if let Some(v) = l.body.get_mut(off - 8..off - 4) {
            v.copy_from_slice(&x.to_le_bytes());
        }
    }
}

fn leader_i16(save: &Save, who: usize, off: usize) -> Option<i16> {
    let l = save.leaders.slots.get(who)?;
    l.body.get(off - 8..off - 6).map(|v| i16::from_le_bytes([v[0], v[1]]))
}

fn set_leader_i16(save: &mut Save, who: usize, off: usize, x: i16) {
    if let Some(l) = save.leaders.slots.get_mut(who) {
        if let Some(v) = l.body.get_mut(off - 8..off - 6) {
            v.copy_from_slice(&x.to_le_bytes());
        }
    }
}

/// `WData[y*xs+x]` — the 21-byte tile record (byte 0 flags, +0x0f owner).
fn tile_rec(save: &Save, tx: i32, ty: i32) -> Option<&[u8]> {
    if tx < 0 || ty < 0 || tx >= save.world.xs || ty >= save.world.ys {
        return None;
    }
    let idx = (ty * save.world.xs + tx) as usize;
    save.world.wdata.get(idx * 21..idx * 21 + 21)
}

/// `Objects` scalar block marks (`[0x1f4..0x1fc) [0x154..0x15c) unit_mark[9]
/// build_mark[9] wall_mark[9]`).
fn unit_mark(save: &Save, owner: usize) -> i32 {
    let o = 16 + owner * 4;
    save.objects.scalars.get(o..o + 4).map(|v| i32::from_le_bytes(v.try_into().unwrap())).unwrap_or(0)
}
fn build_mark(save: &Save, owner: usize) -> i32 {
    let o = 16 + 36 + owner * 4;
    save.objects.scalars.get(o..o + 4).map(|v| i32::from_le_bytes(v.try_into().unwrap())).unwrap_or(0)
}
fn wall_mark(save: &Save, owner: usize) -> i32 {
    let o = 16 + 72 + owner * 4;
    save.objects.scalars.get(o..o + 4).map(|v| i32::from_le_bytes(v.try_into().unwrap())).unwrap_or(0)
}
fn set_mark(save: &mut Save, o: usize, x: i32) {
    if let Some(v) = save.objects.scalars.get_mut(o..o + 4) {
        v.copy_from_slice(&x.to_le_bytes());
    }
}

/// `Random::get(min, max)` 0x00a39d70 on `GameAccess::game_random`
/// (`Save.post_world` +40) — same body as `game_daemon::game_random`.
fn game_random(save: &mut Save, min: i32, max: i32) -> i32 {
    const GAME_RANDOM: usize = 0x28;
    let (lo, hi) = if max < min { (max, min) } else { (min, max) };
    if lo == hi {
        return min;
    }
    let seed = u32::from_le_bytes(save.post_world[GAME_RANDOM..GAME_RANDOM + 4].try_into().unwrap());
    let seed = crate::tick::rng_step(seed);
    save.post_world[GAME_RANDOM..GAME_RANDOM + 4].copy_from_slice(&seed.to_le_bytes());
    (((seed & 0xffff) as i32).wrapping_mul(hi - lo) as u32 >> 16) as i32 + lo
}

/// Type record image reader (`Rules.types[idx]`): `head` = image[4..94),
/// `obj_mid` = image[0x1e4..0x27c), `ext` = the unit tail
/// `[0x2b4..0x2cc) ++ [0x2d4..0x2dc) ++ [0x2dc..0x2e0) ++ [0x2e0..0x5d4)`,
/// so image offsets >= 0x2d4 are shifted by the 8-byte gap at 0x2cc.
struct TypeImg<'a>(&'a TypeRec);

impl TypeImg<'_> {
    fn i32(&self, off: usize) -> Option<i32> {
        let (v, i) = match off {
            0x04..=0x5d => (&self.0.head, off - 4),
            0x1e4..=0x27b => (&self.0.obj_mid, off - 0x1e4),
            0x2b4..=0x2cb => (&self.0.ext, off - 0x2b4),
            0x2d4.. => (&self.0.ext, off - 0x2b4 - 8),
            _ => return None,
        };
        v.get(i..i + 4).map(|b| i32::from_le_bytes(b.try_into().unwrap()))
    }
}

fn type_rec(save: &Save, idx: i32) -> Option<TypeImg<'_>> {
    usize::try_from(idx).ok().and_then(|i| save.rules_tail.rules.types.get(i)).map(TypeImg)
}

/// `ObjectTypeData::is(idx, flag)` 0x0065f7d0: `type == idx`, else membership
/// in the type's first (`flag == 0`, image +0x27c) or second (`flag != 0`,
/// +0x298) `SimpleArray<u16>` — serialized as `TypeRec.arr0` / `arr1`. When
/// the chosen array is empty retail falls through to the type vtable `+0xf4`
/// (not transcribed; treated as false).
fn type_is(t: &TypeImg<'_>, idx: i32, flag: i32) -> bool {
    if t.i32(4) == Some(idx) {
        return true;
    }
    let arr = if flag == 0 { &t.0.arr0 } else { &t.0.arr1 };
    arr.data.chunks_exact(2).any(|c| u16::from_le_bytes([c[0], c[1]]) as i32 == idx)
}

/// `Balance[(atk-50)*493 + (def-50)]` — the serialized 493×493 i16 matrix
/// (`Rules.balance`, captured at 0x00c12bf4; retail's folded base
/// 0x00c06afc with the raw `atk*493+def` index is the same cell).
fn balance(save: &Save, atk_type: i32, def_type: i32) -> Option<i16> {
    if !(50..=542).contains(&atk_type) || !(50..=542).contains(&def_type) {
        return None;
    }
    let idx = ((atk_type - 50) * 493 + (def_type - 50)) as usize * 2;
    save.rules_tail.rules.balance.get(idx..idx + 2).map(|v| i16::from_le_bytes([v[0], v[1]]))
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    struct Sample {
        i: DamageInput,
        p: DamagePredicates,
        r: CombatRules,
        expect: i32,
        trace: u64,
    }

    /// Recorded (not computed here) from `don_sim::mechanics::damage_traced`,
    /// the model the i686 oracle ran against retail `ObjectData::get_damage`
    /// 0x00644130 (schema/oracle-regression.json `damage_pipeline`:
    /// 7,986,675 trials, 0 mismatches). Generator: xorshift64 seed
    /// 0x2545f4914f6cdd1d, 110 random draws over the oracle's input mixture
    /// plus 20 steered at steps 3, 4a and 23b; steps 10/11 are forced off
    /// because the oracle never verified them.
    fn samples() -> Vec<Sample> {
        vec![
        Sample { i: DamageInput { balance_pct: 657, attack: 1358, armor: 12, attacker_masks: 0x10108, defender_masks: 0x0, attack_dir: -1594280740, splash_flag: 1, overkill_gate: 0, attacker_player: 6, attacker_type_id: 0x1bf, attacker_domain: 1, attacker_splash_percent: 0, attacker_type_0x40: 0x1, attacker_z: 256, attacker_flag8_bit5: true, defender_type_id: 0x1c8, defender_domain: 1, defender_type_0x2b8_bit2: false, defender_splash_divisor: 3, defender_flags_0x68: 0x8050108, defender_flags_0x6c_bit12: false, defender_z: -79, defender_facing: 715827881, defender_facing_entrench: 133774559, defender_overkill_stamp: 95636, defender_word_0xa4: 2188, attacker_vf_0xe4: 1394, current_frame: 73903, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 4 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: false, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: true, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 9, height_bonus: 35, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -15, trace: 0x8003 },
        Sample { i: DamageInput { balance_pct: 99, attack: 82, armor: 256, attacker_masks: 0x10050108, defender_masks: 0x34cff96d, attack_dir: -2147483648, splash_flag: 0, overkill_gate: 1, attacker_player: 6, attacker_type_id: 0x19f, attacker_domain: 2, attacker_splash_percent: 7, attacker_type_0x40: 0xffffffce, attacker_z: -2720, attacker_flag8_bit5: false, defender_type_id: 0x124, defender_domain: 1, defender_type_0x2b8_bit2: false, defender_splash_divisor: 3, defender_flags_0x68: 0x1000, defender_flags_0x6c_bit12: true, defender_z: -32768, defender_facing: -715827882, defender_facing_entrench: -199149526, defender_overkill_stamp: 97275, defender_word_0xa4: 1886, attacker_vf_0xe4: 2969, current_frame: 25208, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 0 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 9, height_bonus: 4, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -336, trace: 0x1040002 },
        Sample { i: DamageInput { balance_pct: 100, attack: 935, armor: 14, attacker_masks: 0x0, defender_masks: 0x0, attack_dir: 1210164967, splash_flag: 1, overkill_gate: 0, attacker_player: 5, attacker_type_id: 0x34, attacker_domain: 0, attacker_splash_percent: 83, attacker_type_0x40: 0x104c, attacker_z: -3478, attacker_flag8_bit5: false, defender_type_id: 0x1aa, defender_domain: 0, defender_type_0x2b8_bit2: false, defender_splash_divisor: 2, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: false, defender_z: -29, defender_facing: 0, defender_facing_entrench: -715827883, defender_overkill_stamp: 0, defender_word_0xa4: 1731, attacker_vf_0xe4: 1681, current_frame: 98815, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 1 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: true, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: true },
                r: CombatRules { height_increment: 5, height_bonus: 41, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 64, trace: 0x8000 },
        Sample { i: DamageInput { balance_pct: 1470, attack: 1873, armor: 28, attacker_masks: 0x5af1a6ec, defender_masks: 0xdc5ef9ec, attack_dir: -1610612735, splash_flag: 0, overkill_gate: 1, attacker_player: 2, attacker_type_id: 0x33, attacker_domain: 2, attacker_splash_percent: 90, attacker_type_0x40: 0x1ac, attacker_z: -53, attacker_flag8_bit5: true, defender_type_id: 0xb6, defender_domain: 0, defender_type_0x2b8_bit2: false, defender_splash_divisor: 3, defender_flags_0x68: 0x1008, defender_flags_0x6c_bit12: false, defender_z: 63, defender_facing: -715827882, defender_facing_entrench: 0, defender_overkill_stamp: 0, defender_word_0xa4: 1032, attacker_vf_0xe4: 2027, current_frame: 83368, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 2 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 2, height_bonus: 14, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 2716, trace: 0x1000003 },
        Sample { i: DamageInput { balance_pct: 345, attack: 1586, armor: -62, attacker_masks: 0x12002000, defender_masks: 0x20, attack_dir: 715827881, splash_flag: 1, overkill_gate: 0, attacker_player: 3, attacker_type_id: 0x1bc, attacker_domain: 0, attacker_splash_percent: 91, attacker_type_0x40: 0x9c, attacker_z: 7149, attacker_flag8_bit5: false, defender_type_id: 0x12a, defender_domain: 2, defender_type_0x2b8_bit2: true, defender_splash_divisor: 2, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: true, defender_z: -72, defender_facing: -1, defender_facing_entrench: 715827881, defender_overkill_stamp: 46192, defender_word_0xa4: 2163, attacker_vf_0xe4: 2589, current_frame: 42737, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 0 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 185, trace: 0x4c400 },
        Sample { i: DamageInput { balance_pct: 1432, attack: -482, armor: 30, attacker_masks: 0xa000000, defender_masks: 0x0, attack_dir: 0, splash_flag: 1, overkill_gate: 0, attacker_player: 6, attacker_type_id: 0x21d, attacker_domain: 0, attacker_splash_percent: 12, attacker_type_0x40: 0x1ac, attacker_z: -8972, attacker_flag8_bit5: true, defender_type_id: 0x21d, defender_domain: 0, defender_type_0x2b8_bit2: true, defender_splash_divisor: 2, defender_flags_0x68: 0x8000c, defender_flags_0x6c_bit12: false, defender_z: -8555, defender_facing: -1631472841, defender_facing_entrench: -1610612736, defender_overkill_stamp: 0, defender_word_0xa4: 883, attacker_vf_0xe4: 1661, current_frame: 48365, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 6 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 5, height_bonus: 31, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -39, trace: 0x6c201 },
        Sample { i: DamageInput { balance_pct: 100, attack: 1, armor: 1, attacker_masks: 0x372c27ae, defender_masks: 0x6efd518b, attack_dir: 1890449851, splash_flag: 1, overkill_gate: 1, attacker_player: 2, attacker_type_id: 0x1bc, attacker_domain: 1, attacker_splash_percent: 62, attacker_type_0x40: 0x1ab, attacker_z: -54, attacker_flag8_bit5: false, defender_type_id: 0x1a7, defender_domain: 0, defender_type_0x2b8_bit2: false, defender_splash_divisor: 1, defender_flags_0x68: 0x8, defender_flags_0x6c_bit12: false, defender_z: -4525, defender_facing: 2134297814, defender_facing_entrench: -1, defender_overkill_stamp: 0, defender_word_0xa4: 375, attacker_vf_0xe4: 240, current_frame: 96647, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 5 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: false, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 0, trace: 0x11080c2 },
        Sample { i: DamageInput { balance_pct: 100, attack: 7506, armor: 15, attacker_masks: 0x10000001, defender_masks: 0x7dd90964, attack_dir: -1232896206, splash_flag: 1, overkill_gate: 0, attacker_player: 5, attacker_type_id: 0x198, attacker_domain: 0, attacker_splash_percent: 32, attacker_type_0x40: 0x35, attacker_z: 211327110, attacker_flag8_bit5: true, defender_type_id: 0x18f, defender_domain: 1, defender_type_0x2b8_bit2: true, defender_splash_divisor: 3, defender_flags_0x68: 0x3001, defender_flags_0x6c_bit12: true, defender_z: 18, defender_facing: 185782143, defender_facing_entrench: 1610612736, defender_overkill_stamp: 59032, defender_word_0xa4: 1154, attacker_vf_0xe4: 2218, current_frame: 8889, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 7 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: false, defender_vf_0x1c: false, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: true, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 8, height_bonus: 22, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -2378418, trace: 0x3008001 },
        Sample { i: DamageInput { balance_pct: 100, attack: 1726, armor: -75, attacker_masks: 0x10000000, defender_masks: 0x8, attack_dir: -2147483648, splash_flag: 1, overkill_gate: 1, attacker_player: 3, attacker_type_id: 0x76, attacker_domain: 2, attacker_splash_percent: 96, attacker_type_0x40: 0x1ab, attacker_z: -1, attacker_flag8_bit5: true, defender_type_id: 0x21d, defender_domain: 0, defender_type_0x2b8_bit2: false, defender_splash_divisor: 2, defender_flags_0x68: 0x5, defender_flags_0x6c_bit12: true, defender_z: -1442893612, defender_facing: -715827883, defender_facing_entrench: 2113276995, defender_overkill_stamp: 0, defender_word_0xa4: 2769, attacker_vf_0xe4: 251, current_frame: 11673, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 6 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: true, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 0, trace: 0x110ec001 },
        Sample { i: DamageInput { balance_pct: 100, attack: -5299, armor: 16, attacker_masks: 0x68d9edcb, defender_masks: 0x0, attack_dir: -715827883, splash_flag: 0, overkill_gate: 1, attacker_player: 3, attacker_type_id: 0x1bb, attacker_domain: 0, attacker_splash_percent: 90, attacker_type_0x40: 0x1ab, attacker_z: 64, attacker_flag8_bit5: true, defender_type_id: 0x21d, defender_domain: 2, defender_type_0x2b8_bit2: true, defender_splash_divisor: 1, defender_flags_0x68: 0x89715add, defender_flags_0x6c_bit12: false, defender_z: 109811942, defender_facing: -715827882, defender_facing_entrench: -2147483648, defender_overkill_stamp: 79840, defender_word_0xa4: 1165, attacker_vf_0xe4: 2824, current_frame: 19798, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: -1 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: true, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 8, height_bonus: 46, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 1, trace: 0x8082412 },
        Sample { i: DamageInput { balance_pct: 554, attack: 50, armor: 8, attacker_masks: 0x402000, defender_masks: 0x2002000, attack_dir: -1839376031, splash_flag: 0, overkill_gate: 0, attacker_player: 4, attacker_type_id: 0x1bc, attacker_domain: 2, attacker_splash_percent: 34, attacker_type_0x40: 0x1ac, attacker_z: 43, attacker_flag8_bit5: true, defender_type_id: 0x157, defender_domain: 0, defender_type_0x2b8_bit2: true, defender_splash_divisor: 4, defender_flags_0x68: 0x40001, defender_flags_0x6c_bit12: true, defender_z: -115, defender_facing: -715827883, defender_facing_entrench: -1009426621, defender_overkill_stamp: 0, defender_word_0xa4: 1430, attacker_vf_0xe4: 941, current_frame: 75510, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 1 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: false, defender_vf_0x18: false, defender_vf_0x1c: false, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 5, height_bonus: 48, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 6, trace: 0x101 },
        Sample { i: DamageInput { balance_pct: 237, attack: 74, armor: 23, attacker_masks: 0x401000, defender_masks: 0x0, attack_dir: 1610612735, splash_flag: 1, overkill_gate: 0, attacker_player: 6, attacker_type_id: 0x21d, attacker_domain: 2, attacker_splash_percent: 16, attacker_type_0x40: 0x1ac, attacker_z: -91, attacker_flag8_bit5: true, defender_type_id: 0x153, defender_domain: 0, defender_type_0x2b8_bit2: false, defender_splash_divisor: 4, defender_flags_0x68: 0x8000000, defender_flags_0x6c_bit12: false, defender_z: 100, defender_facing: -715827882, defender_facing_entrench: 715827882, defender_overkill_stamp: 0, defender_word_0xa4: 808, attacker_vf_0xe4: 2508, current_frame: 34433, game_flag_0x821_bit1: false, tile_rocky: false, tile_owner: 6 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -23, trace: 0x2c000 },
        Sample { i: DamageInput { balance_pct: 1926, attack: 1, armor: 16, attacker_masks: 0x2a2b58e6, defender_masks: 0x8001020, attack_dir: 2125185505, splash_flag: 1, overkill_gate: 0, attacker_player: 0, attacker_type_id: 0xea, attacker_domain: 0, attacker_splash_percent: 9, attacker_type_0x40: 0x7fffffff, attacker_z: 6652, attacker_flag8_bit5: false, defender_type_id: 0x107, defender_domain: 1, defender_type_0x2b8_bit2: true, defender_splash_divisor: 3, defender_flags_0x68: 0x10000000, defender_flags_0x6c_bit12: true, defender_z: 7290, defender_facing: 1405539241, defender_facing_entrench: -512999706, defender_overkill_stamp: 0, defender_word_0xa4: 1548, attacker_vf_0xe4: 1327, current_frame: 33921, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 3 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: false, defender_vf_0x1c: false, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -16, trace: 0x8000 },
        Sample { i: DamageInput { balance_pct: 100, attack: 1222100454, armor: 10, attacker_masks: 0x0, defender_masks: 0x8200000, attack_dir: -1610612735, splash_flag: 1, overkill_gate: 1, attacker_player: 5, attacker_type_id: 0x1bc, attacker_domain: 0, attacker_splash_percent: 6, attacker_type_0x40: 0x1ac, attacker_z: -40, attacker_flag8_bit5: false, defender_type_id: 0x11b, defender_domain: 1, defender_type_0x2b8_bit2: false, defender_splash_divisor: 3, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: false, defender_z: 912634718, defender_facing: 1733328231, defender_facing_entrench: 715827882, defender_overkill_stamp: 3172, defender_word_0xa4: 2912, attacker_vf_0xe4: 1075, current_frame: 88027, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 2 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 78028, trace: 0xe071 },
        Sample { i: DamageInput { balance_pct: 1791, attack: 1105, armor: 15, attacker_masks: 0x10002000, defender_masks: 0x0, attack_dir: -216511731, splash_flag: 0, overkill_gate: 0, attacker_player: 2, attacker_type_id: 0x4f, attacker_domain: 1, attacker_splash_percent: 49, attacker_type_0x40: 0x129677ca, attacker_z: -2769, attacker_flag8_bit5: false, defender_type_id: 0x87, defender_domain: 0, defender_type_0x2b8_bit2: false, defender_splash_divisor: 4, defender_flags_0x68: 0x7785e045, defender_flags_0x6c_bit12: true, defender_z: -88, defender_facing: 1610612736, defender_facing_entrench: 715827881, defender_overkill_stamp: 0, defender_word_0xa4: 468, attacker_vf_0xe4: 2725, current_frame: 21664, game_flag_0x821_bit1: false, tile_rocky: false, tile_owner: 0 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 1979, trace: 0x0 },
        Sample { i: DamageInput { balance_pct: 628, attack: -1, armor: 45, attacker_masks: 0x3f1e9d4c, defender_masks: 0x80004, attack_dir: 715827881, splash_flag: 0, overkill_gate: 0, attacker_player: 7, attacker_type_id: 0xc0, attacker_domain: 0, attacker_splash_percent: 84, attacker_type_0x40: 0x1ac, attacker_z: 82, attacker_flag8_bit5: true, defender_type_id: 0x21d, defender_domain: 1, defender_type_0x2b8_bit2: true, defender_splash_divisor: 2, defender_flags_0x68: 0x10000000, defender_flags_0x6c_bit12: false, defender_z: -2068375831, defender_facing: 1990434369, defender_facing_entrench: -2147483648, defender_overkill_stamp: 52530, defender_word_0xa4: 2343, attacker_vf_0xe4: 118, current_frame: 34158, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 4 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: true, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -1, trace: 0x23 },
        Sample { i: DamageInput { balance_pct: 2477, attack: 543, armor: -7000, attacker_masks: 0x13fff009, defender_masks: 0x8000000, attack_dir: 715827882, splash_flag: 0, overkill_gate: 1, attacker_player: 1, attacker_type_id: 0x1bb, attacker_domain: 2, attacker_splash_percent: 20, attacker_type_0x40: 0x1ac, attacker_z: 4023, attacker_flag8_bit5: false, defender_type_id: 0x17a, defender_domain: 1, defender_type_0x2b8_bit2: true, defender_splash_divisor: 4, defender_flags_0x68: 0xa002000, defender_flags_0x6c_bit12: true, defender_z: 5, defender_facing: -1865950653, defender_facing_entrench: 1610612735, defender_overkill_stamp: 51094, defender_word_0xa4: 215, attacker_vf_0xe4: 2923, current_frame: 19158, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: 4 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: true, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: false, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 10655, trace: 0x2 },
        Sample { i: DamageInput { balance_pct: 553, attack: 6638, armor: -1, attacker_masks: 0x0, defender_masks: 0x2028, attack_dir: -1610612735, splash_flag: 0, overkill_gate: 1, attacker_player: 4, attacker_type_id: 0x21d, attacker_domain: 0, attacker_splash_percent: 28, attacker_type_0x40: 0xffffffc2, attacker_z: -654, attacker_flag8_bit5: false, defender_type_id: 0x21d, defender_domain: 0, defender_type_0x2b8_bit2: false, defender_splash_divisor: 1, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: false, defender_z: -883, defender_facing: -248305028, defender_facing_entrench: -1701867122, defender_overkill_stamp: 0, defender_word_0xa4: 1662, attacker_vf_0xe4: 1326, current_frame: 47210, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 3 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: true, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 14683, trace: 0x1100000 },
        Sample { i: DamageInput { balance_pct: 100, attack: 509, armor: 42, attacker_masks: 0x52db0a3f, defender_masks: 0x2010108, attack_dir: 98455637, splash_flag: 0, overkill_gate: 0, attacker_player: 2, attacker_type_id: 0x195, attacker_domain: 0, attacker_splash_percent: 87, attacker_type_0x40: 0x1ab, attacker_z: -60, attacker_flag8_bit5: true, defender_type_id: 0xb5, defender_domain: 0, defender_type_0x2b8_bit2: true, defender_splash_divisor: 2, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: false, defender_z: 0, defender_facing: -903678388, defender_facing_entrench: 158698977, defender_overkill_stamp: 84103, defender_word_0xa4: 2142, attacker_vf_0xe4: 152, current_frame: 62113, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 3 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 148, trace: 0x1000423 },
        Sample { i: DamageInput { balance_pct: 2272, attack: 1447, armor: -5772, attacker_masks: 0x8200000, defender_masks: 0x80008, attack_dir: -1016103901, splash_flag: 0, overkill_gate: 1, attacker_player: 5, attacker_type_id: 0x34, attacker_domain: 0, attacker_splash_percent: 69, attacker_type_0x40: 0x1ac, attacker_z: 58, attacker_flag8_bit5: true, defender_type_id: 0x1ce, defender_domain: 2, defender_type_0x2b8_bit2: false, defender_splash_divisor: 3, defender_flags_0x68: 0x400000, defender_flags_0x6c_bit12: true, defender_z: -42, defender_facing: -1, defender_facing_entrench: -715827882, defender_overkill_stamp: 0, defender_word_0xa4: 5, attacker_vf_0xe4: 2317, current_frame: 62876, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 5 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 9, height_bonus: 21, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 12347, trace: 0x242000 },
        Sample { i: DamageInput { balance_pct: 100, attack: 645, armor: 28, attacker_masks: 0x0, defender_masks: 0x0, attack_dir: -583125827, splash_flag: 0, overkill_gate: 1, attacker_player: 5, attacker_type_id: 0x1bc, attacker_domain: 0, attacker_splash_percent: 96, attacker_type_0x40: 0xfffffbec, attacker_z: -57, attacker_flag8_bit5: true, defender_type_id: 0x143, defender_domain: 0, defender_type_0x2b8_bit2: false, defender_splash_divisor: 2, defender_flags_0x68: 0x1008, defender_flags_0x6c_bit12: true, defender_z: 3863, defender_facing: -1610612735, defender_facing_entrench: -1695270272, defender_overkill_stamp: 0, defender_word_0xa4: 2335, attacker_vf_0xe4: 2737, current_frame: 84198, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: 5 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: false, defender_vf_0x1c: false, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 37, trace: 0x0 },
        Sample { i: DamageInput { balance_pct: 100, attack: 1873, armor: 27, attacker_masks: 0x0, defender_masks: 0x0, attack_dir: -1610612735, splash_flag: 0, overkill_gate: 0, attacker_player: 6, attacker_type_id: 0x190, attacker_domain: 1, attacker_splash_percent: 86, attacker_type_0x40: 0xffffffb4, attacker_z: 10, attacker_flag8_bit5: true, defender_type_id: 0x3c, defender_domain: 1, defender_type_0x2b8_bit2: false, defender_splash_divisor: 1, defender_flags_0x68: 0x2200020, defender_flags_0x6c_bit12: true, defender_z: -99, defender_facing: 1486375909, defender_facing_entrench: 1199810806, defender_overkill_stamp: 0, defender_word_0xa4: 2906, attacker_vf_0xe4: 172, current_frame: 59415, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 4 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 26, trace: 0x60400e0 },
        Sample { i: DamageInput { balance_pct: 2249, attack: 49, armor: 8, attacker_masks: 0x0, defender_masks: 0x90108, attack_dir: -1, splash_flag: 1, overkill_gate: 0, attacker_player: 6, attacker_type_id: 0xaf, attacker_domain: 2, attacker_splash_percent: 85, attacker_type_0x40: 0x10d41464, attacker_z: -3651, attacker_flag8_bit5: false, defender_type_id: 0x196, defender_domain: 1, defender_type_0x2b8_bit2: true, defender_splash_divisor: 2, defender_flags_0x68: 0x1001010c, defender_flags_0x6c_bit12: false, defender_z: -4, defender_facing: -1336684674, defender_facing_entrench: -458264178, defender_overkill_stamp: 0, defender_word_0xa4: 203, attacker_vf_0xe4: 2788, current_frame: 46056, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 0 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: false, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: true, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 38, trace: 0x24e440 },
        Sample { i: DamageInput { balance_pct: 1258, attack: -56, armor: 921962799, attacker_masks: 0x4, defender_masks: 0x210108, attack_dir: 2035566263, splash_flag: 1, overkill_gate: 1, attacker_player: 1, attacker_type_id: 0x9a, attacker_domain: 1, attacker_splash_percent: 86, attacker_type_0x40: 0x1ac, attacker_z: -2491, attacker_flag8_bit5: true, defender_type_id: 0x139, defender_domain: 1, defender_type_0x2b8_bit2: true, defender_splash_divisor: 3, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: false, defender_z: 2147483647, defender_facing: 1005327478, defender_facing_entrench: -892928387, defender_overkill_stamp: 0, defender_word_0xa4: 983, attacker_vf_0xe4: 2443, current_frame: 86504, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: 2 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: false, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 784052, trace: 0x1008081 },
        Sample { i: DamageInput { balance_pct: 100, attack: -6020, armor: -6244, attacker_masks: 0xad132284, defender_masks: 0x8000000, attack_dir: -585756187, splash_flag: 0, overkill_gate: 1, attacker_player: 6, attacker_type_id: 0x15d, attacker_domain: 1, attacker_splash_percent: 82, attacker_type_0x40: 0x80000000, attacker_z: -37, attacker_flag8_bit5: false, defender_type_id: 0x107, defender_domain: 2, defender_type_0x2b8_bit2: true, defender_splash_divisor: 1, defender_flags_0x68: 0x2c0000, defender_flags_0x6c_bit12: true, defender_z: -66, defender_facing: 948559839, defender_facing_entrench: 1930189335, defender_overkill_stamp: 0, defender_word_0xa4: 652, attacker_vf_0xe4: 1113, current_frame: 57278, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 0 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -2407, trace: 0x140200 },
        Sample { i: DamageInput { balance_pct: 100, attack: 1910, armor: -77, attacker_masks: 0x0, defender_masks: 0x200000, attack_dir: -160309573, splash_flag: 0, overkill_gate: 1, attacker_player: 2, attacker_type_id: 0x1bc, attacker_domain: 0, attacker_splash_percent: 79, attacker_type_0x40: 0xffffffe7, attacker_z: -85, attacker_flag8_bit5: false, defender_type_id: 0x60, defender_domain: 0, defender_type_0x2b8_bit2: true, defender_splash_divisor: 1, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: false, defender_z: 17, defender_facing: 1610612736, defender_facing_entrench: 1382217265, defender_overkill_stamp: 0, defender_word_0xa4: 2841, attacker_vf_0xe4: 2796, current_frame: 29120, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 7 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 267, trace: 0x401 },
        Sample { i: DamageInput { balance_pct: 100, attack: 0, armor: 81, attacker_masks: 0x400000, defender_masks: 0x8000020, attack_dir: -1348698371, splash_flag: 0, overkill_gate: 0, attacker_player: 5, attacker_type_id: 0x1bc, attacker_domain: 0, attacker_splash_percent: 1, attacker_type_0x40: 0xffffffec, attacker_z: -432, attacker_flag8_bit5: true, defender_type_id: 0xee, defender_domain: 2, defender_type_0x2b8_bit2: true, defender_splash_divisor: 2, defender_flags_0x68: 0x2001, defender_flags_0x6c_bit12: true, defender_z: -45, defender_facing: -715827883, defender_facing_entrench: 1610612735, defender_overkill_stamp: 82520, defender_word_0xa4: 2155, attacker_vf_0xe4: 2525, current_frame: 43972, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 1 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: false, defender_vf_0x18: false, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 1, trace: 0x8000040 },
        Sample { i: DamageInput { balance_pct: 100, attack: 547, armor: 1814322650, attacker_masks: 0x4, defender_masks: 0x1004, attack_dir: -715827883, splash_flag: 0, overkill_gate: 0, attacker_player: 2, attacker_type_id: 0x1cd, attacker_domain: 2, attacker_splash_percent: 5, attacker_type_0x40: 0x1ac, attacker_z: 2832, attacker_flag8_bit5: false, defender_type_id: 0x124, defender_domain: 2, defender_type_0x2b8_bit2: true, defender_splash_divisor: 1, defender_flags_0x68: 0x49c71d84, defender_flags_0x6c_bit12: true, defender_z: 18, defender_facing: -1610612736, defender_facing_entrench: -715827882, defender_overkill_stamp: 20456, defender_word_0xa4: 2858, attacker_vf_0xe4: 1356, current_frame: 66951, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 5 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: false, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 1, trace: 0x80000e0 },
        Sample { i: DamageInput { balance_pct: 1077, attack: 1375, armor: 29, attacker_masks: 0x9cfe4a53, defender_masks: 0xedbfd3de, attack_dir: -715827882, splash_flag: 0, overkill_gate: 0, attacker_player: 6, attacker_type_id: 0x164, attacker_domain: 2, attacker_splash_percent: 30, attacker_type_0x40: 0x1ac, attacker_z: -2147483648, attacker_flag8_bit5: false, defender_type_id: 0x21d, defender_domain: 2, defender_type_0x2b8_bit2: false, defender_splash_divisor: 3, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: true, defender_z: -88, defender_facing: -1962964858, defender_facing_entrench: 1610612735, defender_overkill_stamp: 30960, defender_word_0xa4: 1823, attacker_vf_0xe4: 116, current_frame: 70593, game_flag_0x821_bit1: false, tile_rocky: false, tile_owner: 5 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: true, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 0, trace: 0x10100000 },
        Sample { i: DamageInput { balance_pct: 100, attack: 81, armor: 28, attacker_masks: 0x0, defender_masks: 0x0, attack_dir: -282672197, splash_flag: 0, overkill_gate: 1, attacker_player: 6, attacker_type_id: 0x1e5, attacker_domain: 2, attacker_splash_percent: 98, attacker_type_0x40: 0x246e, attacker_z: 1934, attacker_flag8_bit5: true, defender_type_id: 0x41, defender_domain: 2, defender_type_0x2b8_bit2: false, defender_splash_divisor: 4, defender_flags_0x68: 0xe9c54b67, defender_flags_0x6c_bit12: true, defender_z: 89, defender_facing: -1361405449, defender_facing_entrench: 2058051430, defender_overkill_stamp: 16056, defender_word_0xa4: 2511, attacker_vf_0xe4: 713, current_frame: 58653, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: 6 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 1, trace: 0x8000001 },
        Sample { i: DamageInput { balance_pct: 1329, attack: -6340, armor: -736996369, attacker_masks: 0x0, defender_masks: 0x20, attack_dir: 715827882, splash_flag: 0, overkill_gate: 0, attacker_player: 5, attacker_type_id: 0x1bb, attacker_domain: 1, attacker_splash_percent: 50, attacker_type_0x40: 0x1ac, attacker_z: -292284257, attacker_flag8_bit5: false, defender_type_id: 0x21d, defender_domain: 2, defender_type_0x2b8_bit2: true, defender_splash_divisor: 3, defender_flags_0x68: 0x280000, defender_flags_0x6c_bit12: true, defender_z: 2160, defender_facing: -118863134, defender_facing_entrench: -715827883, defender_overkill_stamp: 0, defender_word_0xa4: 1061, attacker_vf_0xe4: 167, current_frame: 70465, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 6 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 736979518, trace: 0x201 },
        Sample { i: DamageInput { balance_pct: 100, attack: -68, armor: 21, attacker_masks: 0x80000, defender_masks: 0x5b768d1f, attack_dir: 1610612735, splash_flag: 1, overkill_gate: 1, attacker_player: 5, attacker_type_id: 0x21d, attacker_domain: 2, attacker_splash_percent: 87, attacker_type_0x40: 0x1ac, attacker_z: 9369, attacker_flag8_bit5: false, defender_type_id: 0xe7, defender_domain: 1, defender_type_0x2b8_bit2: true, defender_splash_divisor: 2, defender_flags_0x68: 0xa000000, defender_flags_0x6c_bit12: true, defender_z: -97, defender_facing: -715827882, defender_facing_entrench: 1197290099, defender_overkill_stamp: 0, defender_word_0xa4: 757, attacker_vf_0xe4: 2960, current_frame: 74248, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 1 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: true },
                r: CombatRules { height_increment: 9, height_bonus: 21, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -22, trace: 0x106e410 },
        Sample { i: DamageInput { balance_pct: 562, attack: 1748, armor: 15, attacker_masks: 0x0, defender_masks: 0xb2a1b07c, attack_dir: -1904583180, splash_flag: 0, overkill_gate: 1, attacker_player: 5, attacker_type_id: 0x1bb, attacker_domain: 1, attacker_splash_percent: 67, attacker_type_0x40: 0xffffffcd, attacker_z: 0, attacker_flag8_bit5: false, defender_type_id: 0x21d, defender_domain: 0, defender_type_0x2b8_bit2: true, defender_splash_divisor: 1, defender_flags_0x68: 0x10000028, defender_flags_0x6c_bit12: true, defender_z: 39, defender_facing: -1692097910, defender_facing_entrench: -715827883, defender_overkill_stamp: 32302, defender_word_0xa4: 973, attacker_vf_0xe4: 2072, current_frame: 30752, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 2 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 4, height_bonus: 43, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 230, trace: 0x4c0 },
        Sample { i: DamageInput { balance_pct: 1408, attack: 1043, armor: 1368954037, attacker_masks: 0x0, defender_masks: 0x32787973, attack_dir: 0, splash_flag: 0, overkill_gate: 0, attacker_player: 5, attacker_type_id: 0x4a, attacker_domain: 2, attacker_splash_percent: 68, attacker_type_0x40: 0x1ab, attacker_z: -64, attacker_flag8_bit5: true, defender_type_id: 0xc6, defender_domain: 1, defender_type_0x2b8_bit2: false, defender_splash_divisor: 4, defender_flags_0x68: 0x441000, defender_flags_0x6c_bit12: false, defender_z: 3082, defender_facing: 0, defender_facing_entrench: 653290611, defender_overkill_stamp: 0, defender_word_0xa4: 162, attacker_vf_0xe4: 2628, current_frame: 96759, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: -1 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 6778409, trace: 0x1000041 },
        Sample { i: DamageInput { balance_pct: 100, attack: 425, armor: 43, attacker_masks: 0x42001, defender_masks: 0x21, attack_dir: 1610612736, splash_flag: 0, overkill_gate: 1, attacker_player: 2, attacker_type_id: 0x1bc, attacker_domain: 1, attacker_splash_percent: 33, attacker_type_0x40: 0x1ab, attacker_z: -326, attacker_flag8_bit5: false, defender_type_id: 0x21d, defender_domain: 0, defender_type_0x2b8_bit2: false, defender_splash_divisor: 2, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: false, defender_z: 6756, defender_facing: -715827882, defender_facing_entrench: -1610612736, defender_overkill_stamp: 33566, defender_word_0xa4: 279, attacker_vf_0xe4: 1819, current_frame: 81868, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: 0 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: true, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 1, trace: 0x8000000 },
        Sample { i: DamageInput { balance_pct: 2362, attack: 1277, armor: 3192, attacker_masks: 0xbb47be9, defender_masks: 0x40be9887, attack_dir: -545115330, splash_flag: 0, overkill_gate: 1, attacker_player: 4, attacker_type_id: 0x176, attacker_domain: 2, attacker_splash_percent: 95, attacker_type_0x40: 0x1, attacker_z: -923015088, attacker_flag8_bit5: false, defender_type_id: 0x11c, defender_domain: 2, defender_type_0x2b8_bit2: true, defender_splash_divisor: 2, defender_flags_0x68: 0x82000, defender_flags_0x6c_bit12: true, defender_z: -33, defender_facing: -1425517047, defender_facing_entrench: 1610612735, defender_overkill_stamp: 54917, defender_word_0xa4: 665, attacker_vf_0xe4: 2045, current_frame: 27111, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 5 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: true, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: true, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 6032, trace: 0x100082 },
        Sample { i: DamageInput { balance_pct: 253, attack: 1246, armor: 33, attacker_masks: 0x40000, defender_masks: 0x0, attack_dir: -2147483648, splash_flag: 0, overkill_gate: 0, attacker_player: 5, attacker_type_id: 0x1bb, attacker_domain: 1, attacker_splash_percent: 29, attacker_type_0x40: 0xffff8000, attacker_z: -8298, attacker_flag8_bit5: true, defender_type_id: 0x1a1, defender_domain: 2, defender_type_0x2b8_bit2: true, defender_splash_divisor: 3, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: false, defender_z: -7717, defender_facing: -715827882, defender_facing_entrench: 948655545, defender_overkill_stamp: 13480, defender_word_0xa4: 1514, attacker_vf_0xe4: 1192, current_frame: 59858, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 7 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 282, trace: 0x1 },
        Sample { i: DamageInput { balance_pct: 100, attack: 3907, armor: 18, attacker_masks: 0x282000, defender_masks: 0x10108, attack_dir: 459506566, splash_flag: 0, overkill_gate: 1, attacker_player: 4, attacker_type_id: 0x191, attacker_domain: 1, attacker_splash_percent: 91, attacker_type_0x40: 0x1ac, attacker_z: 9007, attacker_flag8_bit5: true, defender_type_id: 0x18f, defender_domain: 2, defender_type_0x2b8_bit2: false, defender_splash_divisor: 1, defender_flags_0x68: 0x8000000, defender_flags_0x6c_bit12: false, defender_z: -34, defender_facing: -994218718, defender_facing_entrench: -432949434, defender_overkill_stamp: 36899, defender_word_0xa4: 1142, attacker_vf_0xe4: 152, current_frame: 89193, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 5 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: false, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: true, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 373, trace: 0x1000001 },
        Sample { i: DamageInput { balance_pct: 1673, attack: 712, armor: -2700, attacker_masks: 0x15191245, defender_masks: 0x40000, attack_dir: -826891275, splash_flag: 0, overkill_gate: 1, attacker_player: 5, attacker_type_id: 0x1bc, attacker_domain: 0, attacker_splash_percent: 58, attacker_type_0x40: 0x1ab, attacker_z: -28, attacker_flag8_bit5: false, defender_type_id: 0x13b, defender_domain: 2, defender_type_0x2b8_bit2: true, defender_splash_divisor: 3, defender_flags_0x68: 0x10108, defender_flags_0x6c_bit12: true, defender_z: -3279, defender_facing: -1610612735, defender_facing_entrench: 661784272, defender_overkill_stamp: 0, defender_word_0xa4: 2455, attacker_vf_0xe4: 2293, current_frame: 89075, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 3 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 4, height_bonus: 10, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 3295, trace: 0x40401 },
        Sample { i: DamageInput { balance_pct: 1879, attack: 813, armor: 36, attacker_masks: 0x11f92754, defender_masks: 0x10200000, attack_dir: 43581972, splash_flag: 1, overkill_gate: 0, attacker_player: 0, attacker_type_id: 0x1bc, attacker_domain: 2, attacker_splash_percent: 99, attacker_type_0x40: 0x1ab, attacker_z: 100, attacker_flag8_bit5: false, defender_type_id: 0x181, defender_domain: 1, defender_type_0x2b8_bit2: true, defender_splash_divisor: 3, defender_flags_0x68: 0x16d91bad, defender_flags_0x6c_bit12: false, defender_z: 2147483647, defender_facing: -715827882, defender_facing_entrench: -1610612735, defender_overkill_stamp: 0, defender_word_0xa4: 1989, attacker_vf_0xe4: 1670, current_frame: 37349, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: 7 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 504000, trace: 0x200ae300 },
        Sample { i: DamageInput { balance_pct: 100, attack: 1102, armor: 5, attacker_masks: 0x8, defender_masks: 0x8040004, attack_dir: 715827882, splash_flag: 0, overkill_gate: 0, attacker_player: 1, attacker_type_id: 0x1b8, attacker_domain: 1, attacker_splash_percent: 92, attacker_type_0x40: 0x1ab, attacker_z: 1000, attacker_flag8_bit5: false, defender_type_id: 0x50, defender_domain: 2, defender_type_0x2b8_bit2: true, defender_splash_divisor: 3, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: false, defender_z: -32, defender_facing: -715827883, defender_facing_entrench: -715827883, defender_overkill_stamp: 0, defender_word_0xa4: 418, attacker_vf_0xe4: 2064, current_frame: 78775, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: 7 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 48, trace: 0x40402 },
        Sample { i: DamageInput { balance_pct: 100, attack: 29, armor: 24, attacker_masks: 0x0, defender_masks: 0x8000000, attack_dir: 715827881, splash_flag: 1, overkill_gate: 0, attacker_player: 5, attacker_type_id: 0x34, attacker_domain: 1, attacker_splash_percent: 33, attacker_type_0x40: 0x1ac, attacker_z: 9027, attacker_flag8_bit5: false, defender_type_id: 0x132, defender_domain: 0, defender_type_0x2b8_bit2: true, defender_splash_divisor: 3, defender_flags_0x68: 0x80008, defender_flags_0x6c_bit12: false, defender_z: 57, defender_facing: 516887674, defender_facing_entrench: 2130811387, defender_overkill_stamp: 31040, defender_word_0xa4: 1341, attacker_vf_0xe4: 1374, current_frame: 26205, game_flag_0x821_bit1: false, tile_rocky: false, tile_owner: 2 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 0, trace: 0x212c200 },
        Sample { i: DamageInput { balance_pct: 25, attack: 7720, armor: 28, attacker_masks: 0x400000, defender_masks: 0x210108, attack_dir: 1728468779, splash_flag: 1, overkill_gate: 1, attacker_player: 4, attacker_type_id: 0x21d, attacker_domain: 0, attacker_splash_percent: 7, attacker_type_0x40: 0x1ab, attacker_z: -1480, attacker_flag8_bit5: true, defender_type_id: 0x123, defender_domain: 1, defender_type_0x2b8_bit2: false, defender_splash_divisor: 4, defender_flags_0x68: 0x10000000, defender_flags_0x6c_bit12: false, defender_z: 7975, defender_facing: 1402377556, defender_facing_entrench: -856459740, defender_overkill_stamp: 2107, defender_word_0xa4: 1417, attacker_vf_0xe4: 2393, current_frame: 80689, game_flag_0x821_bit1: false, tile_rocky: false, tile_owner: -1 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: true, defender_vf_0x18: false, defender_vf_0x1c: false, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -14, trace: 0x8000 },
        Sample { i: DamageInput { balance_pct: 1187, attack: 1370, armor: -1, attacker_masks: 0x0, defender_masks: 0x0, attack_dir: 193250503, splash_flag: 0, overkill_gate: 1, attacker_player: 1, attacker_type_id: 0x1bb, attacker_domain: 0, attacker_splash_percent: 59, attacker_type_0x40: 0xffffffda, attacker_z: -100, attacker_flag8_bit5: false, defender_type_id: 0x138, defender_domain: 2, defender_type_0x2b8_bit2: false, defender_splash_divisor: 1, defender_flags_0x68: 0x400000, defender_flags_0x6c_bit12: true, defender_z: -1209376436, defender_facing: -2028448317, defender_facing_entrench: -1569273533, defender_overkill_stamp: 0, defender_word_0xa4: 1208, attacker_vf_0xe4: 1824, current_frame: 33189, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: 2 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 814, trace: 0x420f1 },
        Sample { i: DamageInput { balance_pct: 1700, attack: 1324, armor: 31, attacker_masks: 0x82000, defender_masks: 0x80020, attack_dir: -2055285680, splash_flag: 0, overkill_gate: 1, attacker_player: 0, attacker_type_id: 0x1bb, attacker_domain: 2, attacker_splash_percent: 19, attacker_type_0x40: 0x100, attacker_z: -17, attacker_flag8_bit5: true, defender_type_id: 0x1b5, defender_domain: 1, defender_type_0x2b8_bit2: false, defender_splash_divisor: 2, defender_flags_0x68: 0x4, defender_flags_0x6c_bit12: false, defender_z: -464, defender_facing: -2103168610, defender_facing_entrench: 715827882, defender_overkill_stamp: 59497, defender_word_0xa4: 537, attacker_vf_0xe4: 1900, current_frame: 78805, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 3 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 1094, trace: 0x40001 },
        Sample { i: DamageInput { balance_pct: 100, attack: -1, armor: 0, attacker_masks: 0x39081da1, defender_masks: 0x2000001, attack_dir: -816893546, splash_flag: 1, overkill_gate: 1, attacker_player: 3, attacker_type_id: 0x1bc, attacker_domain: 2, attacker_splash_percent: 47, attacker_type_0x40: 0x1ab, attacker_z: -88, attacker_flag8_bit5: false, defender_type_id: 0x1c1, defender_domain: 2, defender_type_0x2b8_bit2: false, defender_splash_divisor: 2, defender_flags_0x68: 0xb1653fc8, defender_flags_0x6c_bit12: true, defender_z: 2638, defender_facing: -1175538702, defender_facing_entrench: -715827883, defender_overkill_stamp: 21845, defender_word_0xa4: 2290, attacker_vf_0xe4: 1675, current_frame: 20592, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 4 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: false, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: false, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 0, trace: 0x8001 },
        Sample { i: DamageInput { balance_pct: 100, attack: -67, armor: 15, attacker_masks: 0x254fdd2f, defender_masks: 0xe84dabed, attack_dir: -1101190723, splash_flag: 0, overkill_gate: 1, attacker_player: 0, attacker_type_id: 0x1bb, attacker_domain: 2, attacker_splash_percent: 46, attacker_type_0x40: 0x1ac, attacker_z: -82, attacker_flag8_bit5: false, defender_type_id: 0x1e7, defender_domain: 1, defender_type_0x2b8_bit2: false, defender_splash_divisor: 4, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: false, defender_z: -82, defender_facing: 325914977, defender_facing_entrench: -2147483648, defender_overkill_stamp: 14394, defender_word_0xa4: 1269, attacker_vf_0xe4: 716, current_frame: 77106, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 0 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: true, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 10, height_bonus: 38, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 1, trace: 0x9140103 },
        Sample { i: DamageInput { balance_pct: 100, attack: -5060, armor: 73, attacker_masks: 0x281000, defender_masks: 0x1eff614b, attack_dir: -715827882, splash_flag: 0, overkill_gate: 0, attacker_player: 4, attacker_type_id: 0x21d, attacker_domain: 2, attacker_splash_percent: 54, attacker_type_0x40: 0x1ab, attacker_z: 2696, attacker_flag8_bit5: true, defender_type_id: 0x21d, defender_domain: 2, defender_type_0x2b8_bit2: true, defender_splash_divisor: 4, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: true, defender_z: -57, defender_facing: -1610612736, defender_facing_entrench: 1320580218, defender_overkill_stamp: 49150, defender_word_0xa4: 2061, attacker_vf_0xe4: 1218, current_frame: 52516, game_flag_0x821_bit1: false, tile_rocky: false, tile_owner: 4 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: false, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -252, trace: 0x80 },
        Sample { i: DamageInput { balance_pct: 100, attack: -1, armor: -2238, attacker_masks: 0xc521bd23, defender_masks: 0x0, attack_dir: -2147483648, splash_flag: 0, overkill_gate: 1, attacker_player: 1, attacker_type_id: 0x199, attacker_domain: 2, attacker_splash_percent: 93, attacker_type_0x40: 0xfffffd95, attacker_z: -18, attacker_flag8_bit5: true, defender_type_id: 0x8f, defender_domain: 2, defender_type_0x2b8_bit2: false, defender_splash_divisor: 4, defender_flags_0x68: 0x200000, defender_flags_0x6c_bit12: false, defender_z: -32768, defender_facing: 715827882, defender_facing_entrench: -1610612736, defender_overkill_stamp: 56465, defender_word_0xa4: 2377, attacker_vf_0xe4: 1839, current_frame: 40115, game_flag_0x821_bit1: false, tile_rocky: false, tile_owner: 5 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: false, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 2238, trace: 0x40121 },
        Sample { i: DamageInput { balance_pct: 443, attack: 393, armor: 14, attacker_masks: 0x0, defender_masks: 0x200000, attack_dir: -445335917, splash_flag: 1, overkill_gate: 0, attacker_player: 7, attacker_type_id: 0x21d, attacker_domain: 0, attacker_splash_percent: 36, attacker_type_0x40: 0x26, attacker_z: -4004, attacker_flag8_bit5: false, defender_type_id: 0x21d, defender_domain: 0, defender_type_0x2b8_bit2: true, defender_splash_divisor: 2, defender_flags_0x68: 0x200000, defender_flags_0x6c_bit12: false, defender_z: -90, defender_facing: 2020302041, defender_facing_entrench: -1560560872, defender_overkill_stamp: 65402, defender_word_0xa4: 1630, attacker_vf_0xe4: 946, current_frame: 25907, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: 5 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -3, trace: 0x5c4c0 },
        Sample { i: DamageInput { balance_pct: 100, attack: 122, armor: 34, attacker_masks: 0x8, defender_masks: 0x212108, attack_dir: 1745422668, splash_flag: 0, overkill_gate: 0, attacker_player: 2, attacker_type_id: 0x1bc, attacker_domain: 1, attacker_splash_percent: 21, attacker_type_0x40: 0xfffff3f6, attacker_z: -1185805509, attacker_flag8_bit5: true, defender_type_id: 0x135, defender_domain: 2, defender_type_0x2b8_bit2: true, defender_splash_divisor: 1, defender_flags_0x68: 0x10210108, defender_flags_0x6c_bit12: false, defender_z: 81, defender_facing: -443415719, defender_facing_entrench: -2147483648, defender_overkill_stamp: 0, defender_word_0xa4: 2145, attacker_vf_0xe4: 2637, current_frame: 23059, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: -1 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 1, trace: 0x8000402 },
        Sample { i: DamageInput { balance_pct: 100, attack: 195, armor: 19, attacker_masks: 0x7636beda, defender_masks: 0x10000004, attack_dir: 50913369, splash_flag: 1, overkill_gate: 0, attacker_player: 2, attacker_type_id: 0x21d, attacker_domain: 2, attacker_splash_percent: 11, attacker_type_0x40: 0x1ab, attacker_z: -37, attacker_flag8_bit5: false, defender_type_id: 0x127, defender_domain: 2, defender_type_0x2b8_bit2: true, defender_splash_divisor: 2, defender_flags_0x68: 0x12080000, defender_flags_0x6c_bit12: false, defender_z: 5812, defender_facing: -1610612736, defender_facing_entrench: 715827882, defender_overkill_stamp: 65590, defender_word_0xa4: 145, attacker_vf_0xe4: 2759, current_frame: 76301, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 2 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: true, defender_vf_0x18: false, defender_vf_0x1c: false, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 9, height_bonus: 5, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -23, trace: 0x8002 },
        Sample { i: DamageInput { balance_pct: 100, attack: -99, armor: 35, attacker_masks: 0x1001, defender_masks: 0x31efd14a, attack_dir: 792921172, splash_flag: 0, overkill_gate: 0, attacker_player: 2, attacker_type_id: 0x79, attacker_domain: 1, attacker_splash_percent: 69, attacker_type_0x40: 0x1ab, attacker_z: 6096, attacker_flag8_bit5: false, defender_type_id: 0x21d, defender_domain: 0, defender_type_0x2b8_bit2: true, defender_splash_divisor: 4, defender_flags_0x68: 0x10010108, defender_flags_0x6c_bit12: false, defender_z: -1073364328, defender_facing: -2147483648, defender_facing_entrench: 475854702, defender_overkill_stamp: 34058, defender_word_0xa4: 840, attacker_vf_0xe4: 1477, current_frame: 3866, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 7 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: false, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: true },
                r: CombatRules { height_increment: 8, height_bonus: 46, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -44, trace: 0x0 },
        Sample { i: DamageInput { balance_pct: 1608, attack: 1383, armor: 1, attacker_masks: 0x366e645c, defender_masks: 0x0, attack_dir: -912298142, splash_flag: 1, overkill_gate: 0, attacker_player: 5, attacker_type_id: 0x167, attacker_domain: 1, attacker_splash_percent: 38, attacker_type_0x40: 0x1ab, attacker_z: 835600892, attacker_flag8_bit5: true, defender_type_id: 0xfe, defender_domain: 1, defender_type_0x2b8_bit2: true, defender_splash_divisor: 1, defender_flags_0x68: 0x2000, defender_flags_0x6c_bit12: false, defender_z: -577970987, defender_facing: 242114444, defender_facing_entrench: -2147483648, defender_overkill_stamp: 0, defender_word_0xa4: 346, attacker_vf_0xe4: 2380, current_frame: 60537, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: 5 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 844, trace: 0x2008003 },
        Sample { i: DamageInput { balance_pct: 100, attack: 257, armor: 11, attacker_masks: 0x49ceea51, defender_masks: 0xdfe73954, attack_dir: -1607103646, splash_flag: 0, overkill_gate: 1, attacker_player: 3, attacker_type_id: 0x1bb, attacker_domain: 0, attacker_splash_percent: 65, attacker_type_0x40: 0x1ac, attacker_z: 68, attacker_flag8_bit5: false, defender_type_id: 0xe8, defender_domain: 1, defender_type_0x2b8_bit2: false, defender_splash_divisor: 2, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: true, defender_z: 10, defender_facing: -60400954, defender_facing_entrench: 1610612736, defender_overkill_stamp: 0, defender_word_0xa4: 1737, attacker_vf_0xe4: 60, current_frame: 99779, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 5 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -5, trace: 0x20000c0 },
        Sample { i: DamageInput { balance_pct: 100, attack: 66, armor: -94, attacker_masks: 0x8040004, defender_masks: 0x8002000, attack_dir: 1632620749, splash_flag: 1, overkill_gate: 0, attacker_player: 1, attacker_type_id: 0x1bb, attacker_domain: 0, attacker_splash_percent: 18, attacker_type_0x40: 0x1ab, attacker_z: 77, attacker_flag8_bit5: false, defender_type_id: 0xbf, defender_domain: 2, defender_type_0x2b8_bit2: false, defender_splash_divisor: 4, defender_flags_0x68: 0x6a2103fa, defender_flags_0x6c_bit12: true, defender_z: -3, defender_facing: 715827881, defender_facing_entrench: 1083175240, defender_overkill_stamp: 60883, defender_word_0xa4: 128, attacker_vf_0xe4: 336, current_frame: 30755, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 6 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 94, trace: 0x4c000 },
        Sample { i: DamageInput { balance_pct: 100, attack: 1071, armor: 6, attacker_masks: 0x202000, defender_masks: 0x1000, attack_dir: 642002177, splash_flag: 1, overkill_gate: 1, attacker_player: 1, attacker_type_id: 0x34, attacker_domain: 2, attacker_splash_percent: 41, attacker_type_0x40: 0x1ac, attacker_z: 57, attacker_flag8_bit5: true, defender_type_id: 0x21d, defender_domain: 2, defender_type_0x2b8_bit2: true, defender_splash_divisor: 3, defender_flags_0x68: 0x8280000, defender_flags_0x6c_bit12: false, defender_z: -32768, defender_facing: 1313347255, defender_facing_entrench: 1723284044, defender_overkill_stamp: 27478, defender_word_0xa4: 1536, attacker_vf_0xe4: 1534, current_frame: 78851, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 0 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: true, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 5, trace: 0x8140 },
        Sample { i: DamageInput { balance_pct: 100, attack: 1500, armor: 56, attacker_masks: 0xfae6f497, defender_masks: 0x8010108, attack_dir: -1, splash_flag: 1, overkill_gate: 1, attacker_player: 1, attacker_type_id: 0x162, attacker_domain: 1, attacker_splash_percent: 92, attacker_type_0x40: 0x5bf28082, attacker_z: -25, attacker_flag8_bit5: false, defender_type_id: 0x33, defender_domain: 0, defender_type_0x2b8_bit2: true, defender_splash_divisor: 1, defender_flags_0x68: 0x10109, defender_flags_0x6c_bit12: false, defender_z: -27, defender_facing: 1919039643, defender_facing_entrench: 1889202142, defender_overkill_stamp: 0, defender_word_0xa4: 1049, attacker_vf_0xe4: 2053, current_frame: 84092, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: 7 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 26, trace: 0x23008040 },
        Sample { i: DamageInput { balance_pct: 986, attack: 300, armor: 15, attacker_masks: 0x10400000, defender_masks: 0x0, attack_dir: 0, splash_flag: 1, overkill_gate: 1, attacker_player: 6, attacker_type_id: 0x4d, attacker_domain: 1, attacker_splash_percent: 70, attacker_type_0x40: 0x1ab, attacker_z: 1528730301, attacker_flag8_bit5: false, defender_type_id: 0x15d, defender_domain: 0, defender_type_0x2b8_bit2: true, defender_splash_divisor: 2, defender_flags_0x68: 0x280000, defender_flags_0x6c_bit12: false, defender_z: 1, defender_facing: -1610612736, defender_facing_entrench: -1086948096, defender_overkill_stamp: 48153, defender_word_0xa4: 625, attacker_vf_0xe4: 599, current_frame: 509, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: 4 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 6, height_bonus: 10, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 3, trace: 0x42c210 },
        Sample { i: DamageInput { balance_pct: 658, attack: 1147, armor: 29, attacker_masks: 0x2000001, defender_masks: 0x92ab87d5, attack_dir: 161275921, splash_flag: 1, overkill_gate: 0, attacker_player: 6, attacker_type_id: 0x1bc, attacker_domain: 1, attacker_splash_percent: 53, attacker_type_0x40: 0x1ab, attacker_z: 82, attacker_flag8_bit5: false, defender_type_id: 0x1c5, defender_domain: 2, defender_type_0x2b8_bit2: true, defender_splash_divisor: 1, defender_flags_0x68: 0xfc217d0e, defender_flags_0x6c_bit12: false, defender_z: 6717, defender_facing: 314618364, defender_facing_entrench: -2147483648, defender_overkill_stamp: 0, defender_word_0xa4: 252, attacker_vf_0xe4: 1906, current_frame: 41577, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 2 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: false, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 9, height_bonus: 27, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 70, trace: 0x2c400 },
        Sample { i: DamageInput { balance_pct: 100, attack: 961461635, armor: -26, attacker_masks: 0xbb6023b5, defender_masks: 0x10000000, attack_dir: -530095208, splash_flag: 0, overkill_gate: 1, attacker_player: 1, attacker_type_id: 0x21d, attacker_domain: 1, attacker_splash_percent: 4, attacker_type_0x40: 0x1ac, attacker_z: 8245, attacker_flag8_bit5: true, defender_type_id: 0x164, defender_domain: 2, defender_type_0x2b8_bit2: false, defender_splash_divisor: 4, defender_flags_0x68: 0x2000000, defender_flags_0x6c_bit12: false, defender_z: 68, defender_facing: -715827882, defender_facing_entrench: 0, defender_overkill_stamp: 21231, defender_word_0xa4: 382, attacker_vf_0xe4: 2667, current_frame: 4504, game_flag_0x821_bit1: false, tile_rocky: false, tile_owner: 4 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: false, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: false, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 414247, trace: 0xc1 },
        Sample { i: DamageInput { balance_pct: 783, attack: 256, armor: 6, attacker_masks: 0x32931e9, defender_masks: 0x8002020, attack_dir: 1610612735, splash_flag: 0, overkill_gate: 1, attacker_player: 7, attacker_type_id: 0x19d, attacker_domain: 2, attacker_splash_percent: 65, attacker_type_0x40: 0xffffdbc7, attacker_z: -45, attacker_flag8_bit5: false, defender_type_id: 0x21d, defender_domain: 0, defender_type_0x2b8_bit2: true, defender_splash_divisor: 1, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: false, defender_z: 97, defender_facing: 613843208, defender_facing_entrench: -1624589994, defender_overkill_stamp: 13847, defender_word_0xa4: 1966, attacker_vf_0xe4: 391, current_frame: 86305, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: 6 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: true, defender_vf_0x18: false, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 386, trace: 0x20000002 },
        Sample { i: DamageInput { balance_pct: 652, attack: 23, armor: 28, attacker_masks: 0xb2be65bd, defender_masks: 0x200004, attack_dir: -715827883, splash_flag: 0, overkill_gate: 0, attacker_player: 5, attacker_type_id: 0x34, attacker_domain: 0, attacker_splash_percent: 93, attacker_type_0x40: 0x1ab, attacker_z: 6873, attacker_flag8_bit5: true, defender_type_id: 0x19a, defender_domain: 0, defender_type_0x2b8_bit2: true, defender_splash_divisor: 2, defender_flags_0x68: 0x400000, defender_flags_0x6c_bit12: false, defender_z: 9718, defender_facing: 1610612736, defender_facing_entrench: -618852270, defender_overkill_stamp: 30447, defender_word_0xa4: 373, attacker_vf_0xe4: 1481, current_frame: 42882, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 4 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: false, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 3, height_bonus: 35, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -30, trace: 0x42 },
        Sample { i: DamageInput { balance_pct: 100, attack: 76, armor: -28, attacker_masks: 0x9894e8fa, defender_masks: 0x8200020, attack_dir: -584099004, splash_flag: 0, overkill_gate: 1, attacker_player: 6, attacker_type_id: 0xc2, attacker_domain: 2, attacker_splash_percent: 11, attacker_type_0x40: 0x1ab, attacker_z: 1000, attacker_flag8_bit5: false, defender_type_id: 0x9d, defender_domain: 0, defender_type_0x2b8_bit2: true, defender_splash_divisor: 4, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: false, defender_z: 2147483647, defender_facing: 2032940554, defender_facing_entrench: 2036394047, defender_overkill_stamp: 0, defender_word_0xa4: 2018, attacker_vf_0xe4: 878, current_frame: 69371, game_flag_0x821_bit1: false, tile_rocky: false, tile_owner: -1 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: false, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 45, trace: 0x2 },
        Sample { i: DamageInput { balance_pct: 100, attack: 327, armor: 35, attacker_masks: 0x10000000, defender_masks: 0xf169bf93, attack_dir: 715827882, splash_flag: 1, overkill_gate: 0, attacker_player: 2, attacker_type_id: 0x92, attacker_domain: 1, attacker_splash_percent: 70, attacker_type_0x40: 0xffffe792, attacker_z: 205983866, attacker_flag8_bit5: false, defender_type_id: 0x1ec, defender_domain: 0, defender_type_0x2b8_bit2: false, defender_splash_divisor: 3, defender_flags_0x68: 0x2000, defender_flags_0x6c_bit12: true, defender_z: 60, defender_facing: -2147483648, defender_facing_entrench: -1610612736, defender_overkill_stamp: 0, defender_word_0xa4: 2804, attacker_vf_0xe4: 1274, current_frame: 87031, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 4 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -27, trace: 0x200c0e0 },
        Sample { i: DamageInput { balance_pct: 100, attack: 1613, armor: 6745, attacker_masks: 0x5, defender_masks: 0x41001, attack_dir: 715827881, splash_flag: 0, overkill_gate: 1, attacker_player: 1, attacker_type_id: 0x1c6, attacker_domain: 2, attacker_splash_percent: 6, attacker_type_0x40: 0x1ab, attacker_z: 7413, attacker_flag8_bit5: false, defender_type_id: 0x1b2, defender_domain: 2, defender_type_0x2b8_bit2: true, defender_splash_divisor: 2, defender_flags_0x68: 0x200004, defender_flags_0x6c_bit12: false, defender_z: 2870, defender_facing: -1610612735, defender_facing_entrench: -1, defender_overkill_stamp: 0, defender_word_0xa4: 998, attacker_vf_0xe4: 1526, current_frame: 78027, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 5 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: false, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 1, trace: 0x8000400 },
        Sample { i: DamageInput { balance_pct: 2458, attack: 1664, armor: -823, attacker_masks: 0xd9fd0325, defender_masks: 0x10000021, attack_dir: 0, splash_flag: 0, overkill_gate: 1, attacker_player: 3, attacker_type_id: 0x1bb, attacker_domain: 1, attacker_splash_percent: 92, attacker_type_0x40: 0x0, attacker_z: 55, attacker_flag8_bit5: false, defender_type_id: 0xa5, defender_domain: 2, defender_type_0x2b8_bit2: true, defender_splash_divisor: 1, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: true, defender_z: 1004218822, defender_facing: -1610612736, defender_facing_entrench: 715827882, defender_overkill_stamp: 0, defender_word_0xa4: 1974, attacker_vf_0xe4: 2733, current_frame: 48603, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: 2 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: true, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 9, height_bonus: 46, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 4912, trace: 0x400 },
        Sample { i: DamageInput { balance_pct: 100, attack: 309, armor: 29, attacker_masks: 0x72f7b9cd, defender_masks: 0x10000000, attack_dir: 762645472, splash_flag: 0, overkill_gate: 0, attacker_player: 0, attacker_type_id: 0x1d1, attacker_domain: 0, attacker_splash_percent: 5, attacker_type_0x40: 0x1ab, attacker_z: 41, attacker_flag8_bit5: false, defender_type_id: 0x13f, defender_domain: 0, defender_type_0x2b8_bit2: true, defender_splash_divisor: 4, defender_flags_0x68: 0x7c01766f, defender_flags_0x6c_bit12: true, defender_z: 249, defender_facing: -839453211, defender_facing_entrench: -1810975924, defender_overkill_stamp: 86480, defender_word_0xa4: 1279, attacker_vf_0xe4: 2097, current_frame: 94958, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: 2 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -7, trace: 0x2 },
        Sample { i: DamageInput { balance_pct: 416, attack: 1200, armor: -6788, attacker_masks: 0x402000, defender_masks: 0x8402000, attack_dir: 135987441, splash_flag: 0, overkill_gate: 1, attacker_player: 4, attacker_type_id: 0x34, attacker_domain: 2, attacker_splash_percent: 19, attacker_type_0x40: 0x1ab, attacker_z: 0, attacker_flag8_bit5: false, defender_type_id: 0x1d6, defender_domain: 1, defender_type_0x2b8_bit2: true, defender_splash_divisor: 3, defender_flags_0x68: 0xfbd50499, defender_flags_0x6c_bit12: false, defender_z: -56, defender_facing: -2147483648, defender_facing_entrench: 1609308249, defender_overkill_stamp: 0, defender_word_0xa4: 2972, attacker_vf_0xe4: 2589, current_frame: 24582, game_flag_0x821_bit1: false, tile_rocky: false, tile_owner: 4 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 44, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 499200, trace: 0x40c2520 },
        Sample { i: DamageInput { balance_pct: 642, attack: 658, armor: 20, attacker_masks: 0xc0020, defender_masks: 0x10128, attack_dir: -1775353691, splash_flag: 0, overkill_gate: 0, attacker_player: 4, attacker_type_id: 0x8b, attacker_domain: 0, attacker_splash_percent: 14, attacker_type_0x40: 0xfffffff4, attacker_z: -259, attacker_flag8_bit5: true, defender_type_id: 0x21d, defender_domain: 1, defender_type_0x2b8_bit2: false, defender_splash_divisor: 2, defender_flags_0x68: 0x200008, defender_flags_0x6c_bit12: false, defender_z: 256, defender_facing: -497356375, defender_facing_entrench: 1955318422, defender_overkill_stamp: 0, defender_word_0xa4: 825, attacker_vf_0xe4: 2399, current_frame: 14774, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: -1 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 6, height_bonus: 28, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 191, trace: 0x1000041 },
        Sample { i: DamageInput { balance_pct: 1117, attack: 1115, armor: 34, attacker_masks: 0x0, defender_masks: 0x10010108, attack_dir: 715827881, splash_flag: 1, overkill_gate: 1, attacker_player: 2, attacker_type_id: 0x21d, attacker_domain: 0, attacker_splash_percent: 53, attacker_type_0x40: 0x1ac, attacker_z: -32768, attacker_flag8_bit5: false, defender_type_id: 0x21d, defender_domain: 1, defender_type_0x2b8_bit2: true, defender_splash_divisor: 3, defender_flags_0x68: 0x2000, defender_flags_0x6c_bit12: false, defender_z: 3149, defender_facing: -1610612735, defender_facing_entrench: -1366036675, defender_overkill_stamp: 0, defender_word_0xa4: 979, attacker_vf_0xe4: 927, current_frame: 97250, game_flag_0x821_bit1: false, tile_rocky: false, tile_owner: 2 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: true, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 185, trace: 0xc400 },
        Sample { i: DamageInput { balance_pct: 100, attack: 13, armor: -1, attacker_masks: 0x41000, defender_masks: 0x12001000, attack_dir: 715827881, splash_flag: 1, overkill_gate: 1, attacker_player: 3, attacker_type_id: 0x1bf, attacker_domain: 0, attacker_splash_percent: 78, attacker_type_0x40: 0x1ac, attacker_z: 36, attacker_flag8_bit5: false, defender_type_id: 0x8d, defender_domain: 2, defender_type_0x2b8_bit2: false, defender_splash_divisor: 3, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: false, defender_z: 1173795459, defender_facing: -1014035048, defender_facing_entrench: -2147483648, defender_overkill_stamp: 0, defender_word_0xa4: 2537, attacker_vf_0xe4: 273, current_frame: 1160, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: 1 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 10, height_bonus: 16, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 2, trace: 0x20008040 },
        Sample { i: DamageInput { balance_pct: 100, attack: 932, armor: 13, attacker_masks: 0x1ca3a625, defender_masks: 0x400000, attack_dir: 715827881, splash_flag: 0, overkill_gate: 1, attacker_player: 6, attacker_type_id: 0xce, attacker_domain: 0, attacker_splash_percent: 5, attacker_type_0x40: 0x4c, attacker_z: 88, attacker_flag8_bit5: false, defender_type_id: 0xdf, defender_domain: 2, defender_type_0x2b8_bit2: true, defender_splash_divisor: 4, defender_flags_0x68: 0x18040000, defender_flags_0x6c_bit12: false, defender_z: 53, defender_facing: -715827882, defender_facing_entrench: -1610612736, defender_overkill_stamp: 0, defender_word_0xa4: 1333, attacker_vf_0xe4: 1670, current_frame: 51170, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 7 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: false, defender_vf_0x18: false, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 80, trace: 0x0 },
        Sample { i: DamageInput { balance_pct: 100, attack: -9441, armor: -83, attacker_masks: 0x10109, defender_masks: 0x2002008, attack_dir: -1935381464, splash_flag: 0, overkill_gate: 1, attacker_player: 4, attacker_type_id: 0x21d, attacker_domain: 1, attacker_splash_percent: 87, attacker_type_0x40: 0x1ab, attacker_z: -100, attacker_flag8_bit5: false, defender_type_id: 0x11b, defender_domain: 1, defender_type_0x2b8_bit2: true, defender_splash_divisor: 2, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: true, defender_z: -20, defender_facing: -2147483648, defender_facing_entrench: 243826985, defender_overkill_stamp: 0, defender_word_0xa4: 2302, attacker_vf_0xe4: 124, current_frame: 50190, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 1 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 1, trace: 0x8040442 },
        Sample { i: DamageInput { balance_pct: 962, attack: -43, armor: 11, attacker_masks: 0x0, defender_masks: 0x7fe39042, attack_dir: 145479533, splash_flag: 1, overkill_gate: 0, attacker_player: 1, attacker_type_id: 0x1bb, attacker_domain: 2, attacker_splash_percent: 39, attacker_type_0x40: 0x94c, attacker_z: 1926, attacker_flag8_bit5: false, defender_type_id: 0x13f, defender_domain: 1, defender_type_0x2b8_bit2: true, defender_splash_divisor: 2, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: false, defender_z: 74, defender_facing: -1610612736, defender_facing_entrench: 232101596, defender_overkill_stamp: 0, defender_word_0xa4: 1331, attacker_vf_0xe4: 1656, current_frame: 21072, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 4 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -19, trace: 0xc400 },
        Sample { i: DamageInput { balance_pct: 100, attack: -4, armor: -1044699657, attacker_masks: 0xe3af039d, defender_masks: 0xa2d50f7e, attack_dir: -2114585979, splash_flag: 1, overkill_gate: 0, attacker_player: 0, attacker_type_id: 0x1bc, attacker_domain: 1, attacker_splash_percent: 73, attacker_type_0x40: 0x670, attacker_z: 58, attacker_flag8_bit5: true, defender_type_id: 0x115, defender_domain: 1, defender_type_0x2b8_bit2: false, defender_splash_divisor: 1, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: true, defender_z: 75, defender_facing: -1610612736, defender_facing_entrench: 0, defender_overkill_stamp: 0, defender_word_0xa4: 2903, attacker_vf_0xe4: 652, current_frame: 77557, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: 5 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: false, defender_vf_0x18: false, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -1716207, trace: 0x10080a3 },
        Sample { i: DamageInput { balance_pct: 100, attack: 819, armor: 14, attacker_masks: 0xfbaff326, defender_masks: 0x0, attack_dir: -563397130, splash_flag: 1, overkill_gate: 1, attacker_player: 2, attacker_type_id: 0x21d, attacker_domain: 1, attacker_splash_percent: 30, attacker_type_0x40: 0x1ab, attacker_z: 1, attacker_flag8_bit5: true, defender_type_id: 0x186, defender_domain: 0, defender_type_0x2b8_bit2: true, defender_splash_divisor: 3, defender_flags_0x68: 0x1024, defender_flags_0x6c_bit12: false, defender_z: 3246, defender_facing: 2077283455, defender_facing_entrench: 1610612736, defender_overkill_stamp: 64408, defender_word_0xa4: 2334, attacker_vf_0xe4: 2972, current_frame: 6808, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: 5 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: false, defender_vf_0x18: false, defender_vf_0x1c: false, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 11, trace: 0x8000 },
        Sample { i: DamageInput { balance_pct: 1980, attack: -7190, armor: 28, attacker_masks: 0x0, defender_masks: 0x80020, attack_dir: -1285156025, splash_flag: 1, overkill_gate: 0, attacker_player: 2, attacker_type_id: 0x18c, attacker_domain: 0, attacker_splash_percent: 80, attacker_type_0x40: 0x4, attacker_z: -2872, attacker_flag8_bit5: false, defender_type_id: 0x196, defender_domain: 1, defender_type_0x2b8_bit2: false, defender_splash_divisor: 2, defender_flags_0x68: 0x8, defender_flags_0x6c_bit12: true, defender_z: 38, defender_facing: 1610612736, defender_facing_entrench: -1687737116, defender_overkill_stamp: 0, defender_word_0xa4: 777, attacker_vf_0xe4: 1201, current_frame: 51520, game_flag_0x821_bit1: false, tile_rocky: false, tile_owner: 0 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: true, defender_vf_0x18: false, defender_vf_0x1c: false, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 7, height_bonus: 49, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -11416, trace: 0x8000 },
        Sample { i: DamageInput { balance_pct: 100, attack: 48, armor: 34, attacker_masks: 0x1004, defender_masks: 0x10000020, attack_dir: 1610612735, splash_flag: 0, overkill_gate: 0, attacker_player: 6, attacker_type_id: 0x33, attacker_domain: 1, attacker_splash_percent: 80, attacker_type_0x40: 0x1ab, attacker_z: 683725648, attacker_flag8_bit5: false, defender_type_id: 0xe4, defender_domain: 0, defender_type_0x2b8_bit2: false, defender_splash_divisor: 1, defender_flags_0x68: 0x5fc80f9d, defender_flags_0x6c_bit12: false, defender_z: -22, defender_facing: -1244434968, defender_facing_entrench: 715827881, defender_overkill_stamp: 96806, defender_word_0xa4: 1620, attacker_vf_0xe4: 689, current_frame: 37314, game_flag_0x821_bit1: false, tile_rocky: false, tile_owner: 4 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: false, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: true, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -32, trace: 0x2000040 },
        Sample { i: DamageInput { balance_pct: 100, attack: -4979, armor: 65, attacker_masks: 0xa5ab1eb7, defender_masks: 0x12000000, attack_dir: -171020973, splash_flag: 0, overkill_gate: 1, attacker_player: 6, attacker_type_id: 0xc9, attacker_domain: 1, attacker_splash_percent: 43, attacker_type_0x40: 0x1ac, attacker_z: -38, attacker_flag8_bit5: false, defender_type_id: 0x5b, defender_domain: 2, defender_type_0x2b8_bit2: true, defender_splash_divisor: 2, defender_flags_0x68: 0x2400000, defender_flags_0x6c_bit12: true, defender_z: -85, defender_facing: 715827881, defender_facing_entrench: -1610612736, defender_overkill_stamp: 0, defender_word_0xa4: 2512, attacker_vf_0xe4: 2893, current_frame: 55487, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 4 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 1, trace: 0xc042400 },
        Sample { i: DamageInput { balance_pct: 100, attack: -3022, armor: 1347386665, attacker_masks: 0x21010c, defender_masks: 0x9f05f8df, attack_dir: -689452511, splash_flag: 0, overkill_gate: 0, attacker_player: 5, attacker_type_id: 0x35, attacker_domain: 2, attacker_splash_percent: 66, attacker_type_0x40: 0x1ac, attacker_z: -34, attacker_flag8_bit5: false, defender_type_id: 0x66, defender_domain: 2, defender_type_0x2b8_bit2: false, defender_splash_divisor: 4, defender_flags_0x68: 0x33167ac4, defender_flags_0x6c_bit12: true, defender_z: 32767, defender_facing: 1986373083, defender_facing_entrench: 807967392, defender_overkill_stamp: 0, defender_word_0xa4: 1417, attacker_vf_0xe4: 2493, current_frame: 49267, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 0 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 4726205, trace: 0x4000042 },
        Sample { i: DamageInput { balance_pct: 1194, attack: -461, armor: -17, attacker_masks: 0x99952c94, defender_masks: 0x2000, attack_dir: 1610612736, splash_flag: 0, overkill_gate: 1, attacker_player: 7, attacker_type_id: 0x87, attacker_domain: 0, attacker_splash_percent: 95, attacker_type_0x40: 0x1ac, attacker_z: 46, attacker_flag8_bit5: true, defender_type_id: 0x107, defender_domain: 2, defender_type_0x2b8_bit2: false, defender_splash_divisor: 1, defender_flags_0x68: 0xd99fcde8, defender_flags_0x6c_bit12: true, defender_z: 2147483647, defender_facing: -1, defender_facing_entrench: -1610612735, defender_overkill_stamp: 0, defender_word_0xa4: 122, attacker_vf_0xe4: 855, current_frame: 12431, game_flag_0x821_bit1: false, tile_rocky: false, tile_owner: 6 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -1083, trace: 0x200 },
        Sample { i: DamageInput { balance_pct: 100, attack: 1680, armor: 0, attacker_masks: 0xa400000, defender_masks: 0x0, attack_dir: -715827883, splash_flag: 1, overkill_gate: 0, attacker_player: 7, attacker_type_id: 0x1bc, attacker_domain: 1, attacker_splash_percent: 60, attacker_type_0x40: 0x1ab, attacker_z: -1262226796, attacker_flag8_bit5: true, defender_type_id: 0x1e4, defender_domain: 0, defender_type_0x2b8_bit2: false, defender_splash_divisor: 1, defender_flags_0x68: 0x40008, defender_flags_0x6c_bit12: false, defender_z: -23, defender_facing: -664204651, defender_facing_entrench: -1367196037, defender_overkill_stamp: 0, defender_word_0xa4: 1821, attacker_vf_0xe4: 2, current_frame: 33, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: 0 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 101, trace: 0x8001 },
        Sample { i: DamageInput { balance_pct: 1985, attack: 715, armor: -44, attacker_masks: 0x2210108, defender_masks: 0x0, attack_dir: 1296095731, splash_flag: 1, overkill_gate: 1, attacker_player: 1, attacker_type_id: 0x35, attacker_domain: 2, attacker_splash_percent: 82, attacker_type_0x40: 0x1ab, attacker_z: 87, attacker_flag8_bit5: false, defender_type_id: 0xd4, defender_domain: 2, defender_type_0x2b8_bit2: true, defender_splash_divisor: 3, defender_flags_0x68: 0x8000000, defender_flags_0x6c_bit12: false, defender_z: -9145, defender_facing: -1793676661, defender_facing_entrench: 1261712404, defender_overkill_stamp: 5518, defender_word_0xa4: 2436, attacker_vf_0xe4: 574, current_frame: 94297, game_flag_0x821_bit1: false, tile_rocky: false, tile_owner: 7 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: false, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 10, height_bonus: 12, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 640, trace: 0x8042 },
        Sample { i: DamageInput { balance_pct: 2212, attack: 45, armor: 24, attacker_masks: 0x2000, defender_masks: 0x10108, attack_dir: 715827882, splash_flag: 1, overkill_gate: 1, attacker_player: 3, attacker_type_id: 0x21d, attacker_domain: 0, attacker_splash_percent: 79, attacker_type_0x40: 0x1ab, attacker_z: -683256014, attacker_flag8_bit5: false, defender_type_id: 0x88, defender_domain: 0, defender_type_0x2b8_bit2: false, defender_splash_divisor: 1, defender_flags_0x68: 0x11109, defender_flags_0x6c_bit12: false, defender_z: 533014255, defender_facing: 872823308, defender_facing_entrench: 1373620498, defender_overkill_stamp: 53220, defender_word_0xa4: 2112, attacker_vf_0xe4: 1095, current_frame: 950, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 4 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 157, trace: 0x1108040 },
        Sample { i: DamageInput { balance_pct: 823, attack: 829, armor: 1631695001, attacker_masks: 0x0, defender_masks: 0x0, attack_dir: 1610612736, splash_flag: 0, overkill_gate: 1, attacker_player: 2, attacker_type_id: 0x52, attacker_domain: 0, attacker_splash_percent: 46, attacker_type_0x40: 0x1ac, attacker_z: -2027800316, attacker_flag8_bit5: false, defender_type_id: 0x93, defender_domain: 0, defender_type_0x2b8_bit2: true, defender_splash_divisor: 4, defender_flags_0x68: 0x4f3437b4, defender_flags_0x6c_bit12: true, defender_z: -2057913557, defender_facing: 715827881, defender_facing_entrench: 0, defender_overkill_stamp: 0, defender_word_0xa4: 2332, attacker_vf_0xe4: 1058, current_frame: 10851, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 1 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: true, defender_vf_0x18: false, defender_vf_0x1c: false, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 1, trace: 0x8000000 },
        Sample { i: DamageInput { balance_pct: 2104, attack: 1094, armor: 24, attacker_masks: 0x10128, defender_masks: 0x0, attack_dir: 715827882, splash_flag: 0, overkill_gate: 1, attacker_player: 6, attacker_type_id: 0x8f, attacker_domain: 0, attacker_splash_percent: 31, attacker_type_0x40: 0x4f, attacker_z: -7699, attacker_flag8_bit5: true, defender_type_id: 0x10a, defender_domain: 0, defender_type_0x2b8_bit2: true, defender_splash_divisor: 2, defender_flags_0x68: 0xa080000, defender_flags_0x6c_bit12: true, defender_z: -31, defender_facing: 0, defender_facing_entrench: 715827881, defender_overkill_stamp: 14455, defender_word_0xa4: 2886, attacker_vf_0xe4: 540, current_frame: 72678, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 1 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 4572, trace: 0x40263 },
        Sample { i: DamageInput { balance_pct: 100, attack: 667, armor: 71, attacker_masks: 0x3020, defender_masks: 0x40000, attack_dir: -97460807, splash_flag: 1, overkill_gate: 0, attacker_player: 0, attacker_type_id: 0x1bc, attacker_domain: 0, attacker_splash_percent: 86, attacker_type_0x40: 0x44, attacker_z: 2147483647, attacker_flag8_bit5: true, defender_type_id: 0x21d, defender_domain: 1, defender_type_0x2b8_bit2: true, defender_splash_divisor: 3, defender_flags_0x68: 0x40000, defender_flags_0x6c_bit12: true, defender_z: -88, defender_facing: -715827883, defender_facing_entrench: -1165277098, defender_overkill_stamp: 49552, defender_word_0xa4: 1470, attacker_vf_0xe4: 1118, current_frame: 44067, game_flag_0x821_bit1: false, tile_rocky: false, tile_owner: 5 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: false, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: true, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -62, trace: 0x204c401 },
        Sample { i: DamageInput { balance_pct: 250, attack: 317, armor: 5, attacker_masks: 0xa463066c, defender_masks: 0x0, attack_dir: -715827882, splash_flag: 0, overkill_gate: 0, attacker_player: 3, attacker_type_id: 0x32, attacker_domain: 2, attacker_splash_percent: 18, attacker_type_0x40: 0x1ab, attacker_z: -8142, attacker_flag8_bit5: false, defender_type_id: 0x21d, defender_domain: 2, defender_type_0x2b8_bit2: true, defender_splash_divisor: 2, defender_flags_0x68: 0x998ee587, defender_flags_0x6c_bit12: false, defender_z: -2147483648, defender_facing: -2147483648, defender_facing_entrench: 993301169, defender_overkill_stamp: 77915, defender_word_0xa4: 958, attacker_vf_0xe4: 829, current_frame: 47946, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 4 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 0, trace: 0x100c0202 },
        Sample { i: DamageInput { balance_pct: 1879, attack: 4779, armor: 1, attacker_masks: 0x2020, defender_masks: 0xa8c1f6a1, attack_dir: -373690890, splash_flag: 0, overkill_gate: 0, attacker_player: 7, attacker_type_id: 0x21d, attacker_domain: 0, attacker_splash_percent: 43, attacker_type_0x40: 0x1ab, attacker_z: -16, attacker_flag8_bit5: true, defender_type_id: 0x21d, defender_domain: 2, defender_type_0x2b8_bit2: false, defender_splash_divisor: 2, defender_flags_0x68: 0xc0000, defender_flags_0x6c_bit12: true, defender_z: -51, defender_facing: 723906829, defender_facing_entrench: 715827882, defender_overkill_stamp: 0, defender_word_0xa4: 1005, attacker_vf_0xe4: 226, current_frame: 46029, game_flag_0x821_bit1: false, tile_rocky: false, tile_owner: 7 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 9, height_bonus: 39, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 17958, trace: 0x20000001 },
        Sample { i: DamageInput { balance_pct: 2585, attack: 34, armor: -7239, attacker_masks: 0x0, defender_masks: 0x68c1fdf9, attack_dir: -1016344186, splash_flag: 0, overkill_gate: 0, attacker_player: 1, attacker_type_id: 0xa9, attacker_domain: 2, attacker_splash_percent: 17, attacker_type_0x40: 0x1ab, attacker_z: -50, attacker_flag8_bit5: false, defender_type_id: 0x21d, defender_domain: 0, defender_type_0x2b8_bit2: true, defender_splash_divisor: 1, defender_flags_0x68: 0x200000, defender_flags_0x6c_bit12: false, defender_z: 76, defender_facing: -2147483648, defender_facing_entrench: -715827883, defender_overkill_stamp: 20251, defender_word_0xa4: 2620, attacker_vf_0xe4: 2329, current_frame: 5476, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: 4 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: false, defender_vf_0x1c: false, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: false, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 9, height_bonus: 47, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 7327, trace: 0x1000000 },
        Sample { i: DamageInput { balance_pct: 234, attack: 646, armor: 26, attacker_masks: 0x41db5585, defender_masks: 0x1020, attack_dir: -1610612735, splash_flag: 0, overkill_gate: 1, attacker_player: 7, attacker_type_id: 0x21d, attacker_domain: 0, attacker_splash_percent: 85, attacker_type_0x40: 0x1ac, attacker_z: 53, attacker_flag8_bit5: false, defender_type_id: 0x58, defender_domain: 1, defender_type_0x2b8_bit2: true, defender_splash_divisor: 2, defender_flags_0x68: 0x20, defender_flags_0x6c_bit12: true, defender_z: 23, defender_facing: 1123737175, defender_facing_entrench: -1610612736, defender_overkill_stamp: 0, defender_word_0xa4: 1867, attacker_vf_0xe4: 2709, current_frame: 71923, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: 0 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: true, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 2, height_bonus: 7, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 508, trace: 0x22000400 },
        Sample { i: DamageInput { balance_pct: 100, attack: -2147483648, armor: 8, attacker_masks: 0x50108, defender_masks: 0x10000000, attack_dir: -2121574435, splash_flag: 1, overkill_gate: 0, attacker_player: 3, attacker_type_id: 0x1bb, attacker_domain: 1, attacker_splash_percent: 13, attacker_type_0x40: 0x1ab, attacker_z: 1049, attacker_flag8_bit5: false, defender_type_id: 0x3e, defender_domain: 0, defender_type_0x2b8_bit2: false, defender_splash_divisor: 4, defender_flags_0x68: 0x400000, defender_flags_0x6c_bit12: false, defender_z: 8042, defender_facing: -230352043, defender_facing_entrench: -715827883, defender_overkill_stamp: 0, defender_word_0xa4: 1249, attacker_vf_0xe4: 571, current_frame: 5981, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 1 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: false, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -10, trace: 0x8063 },
        Sample { i: DamageInput { balance_pct: 963, attack: 1521667427, armor: 33, attacker_masks: 0x2400020, defender_masks: 0x2000, attack_dir: -1711492513, splash_flag: 1, overkill_gate: 0, attacker_player: 6, attacker_type_id: 0x1bb, attacker_domain: 2, attacker_splash_percent: 97, attacker_type_0x40: 0x1ab, attacker_z: 4818, attacker_flag8_bit5: true, defender_type_id: 0x116, defender_domain: 0, defender_type_0x2b8_bit2: false, defender_splash_divisor: 3, defender_flags_0x68: 0x8010128, defender_flags_0x6c_bit12: false, defender_z: -376221702, defender_facing: -1610612736, defender_facing_entrench: 1768400916, defender_overkill_stamp: 21184, defender_word_0xa4: 1056, attacker_vf_0xe4: 2864, current_frame: 77490, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: 1 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: false, defender_vf_0x1c: false, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 758395, trace: 0x8001 },
        Sample { i: DamageInput { balance_pct: 2258, attack: 71, armor: 4, attacker_masks: 0xd469823f, defender_masks: 0x400001, attack_dir: -1364665144, splash_flag: 0, overkill_gate: 1, attacker_player: 7, attacker_type_id: 0x142, attacker_domain: 1, attacker_splash_percent: 16, attacker_type_0x40: 0x1ab, attacker_z: 31, attacker_flag8_bit5: false, defender_type_id: 0x6a, defender_domain: 1, defender_type_0x2b8_bit2: true, defender_splash_divisor: 2, defender_flags_0x68: 0x42000, defender_flags_0x6c_bit12: true, defender_z: -5221, defender_facing: 1610612736, defender_facing_entrench: -715827882, defender_overkill_stamp: 0, defender_word_0xa4: 1270, attacker_vf_0xe4: 785, current_frame: 85597, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 7 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 315, trace: 0x2040422 },
        Sample { i: DamageInput { balance_pct: 100, attack: 1072, armor: 15, attacker_masks: 0x2000001, defender_masks: 0x10108, attack_dir: -1610612735, splash_flag: 1, overkill_gate: 1, attacker_player: 1, attacker_type_id: 0x70, attacker_domain: 0, attacker_splash_percent: 80, attacker_type_0x40: 0x1ac, attacker_z: 97, attacker_flag8_bit5: true, defender_type_id: 0x64, defender_domain: 0, defender_type_0x2b8_bit2: false, defender_splash_divisor: 1, defender_flags_0x68: 0x8200001, defender_flags_0x6c_bit12: true, defender_z: 57073809, defender_facing: -899958462, defender_facing_entrench: -1655938931, defender_overkill_stamp: 57594, defender_word_0xa4: 678, attacker_vf_0xe4: 2835, current_frame: 97933, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 1 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 214400, trace: 0x12ac001 },
        Sample { i: DamageInput { balance_pct: 100, attack: -45, armor: 400062899, attacker_masks: 0x0, defender_masks: 0x21, attack_dir: -715827883, splash_flag: 0, overkill_gate: 0, attacker_player: 7, attacker_type_id: 0xcf, attacker_domain: 2, attacker_splash_percent: 75, attacker_type_0x40: 0x20, attacker_z: 0, attacker_flag8_bit5: false, defender_type_id: 0x105, defender_domain: 2, defender_type_0x2b8_bit2: false, defender_splash_divisor: 3, defender_flags_0x68: 0x10400000, defender_flags_0x6c_bit12: false, defender_z: 256, defender_facing: -715827882, defender_facing_entrench: 1467058470, defender_overkill_stamp: 99251, defender_word_0xa4: 2688, attacker_vf_0xe4: 2546, current_frame: 17032, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 1 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 1, trace: 0x8000040 },
        Sample { i: DamageInput { balance_pct: 100, attack: 71, armor: 1675, attacker_masks: 0x20, defender_masks: 0x18000000, attack_dir: 715827882, splash_flag: 1, overkill_gate: 1, attacker_player: 3, attacker_type_id: 0x1c0, attacker_domain: 2, attacker_splash_percent: 83, attacker_type_0x40: 0x220c, attacker_z: -89, attacker_flag8_bit5: false, defender_type_id: 0x115, defender_domain: 0, defender_type_0x2b8_bit2: true, defender_splash_divisor: 3, defender_flags_0x68: 0x28, defender_flags_0x6c_bit12: true, defender_z: 17, defender_facing: -1610612736, defender_facing_entrench: -715827882, defender_overkill_stamp: 68299, defender_word_0xa4: 1574, attacker_vf_0xe4: 2394, current_frame: 5024, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: -1 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -1673, trace: 0x1c410 },
        Sample { i: DamageInput { balance_pct: 100, attack: -6803, armor: 34, attacker_masks: 0x2000009, defender_masks: 0xc0000, attack_dir: -1941523037, splash_flag: 0, overkill_gate: 0, attacker_player: 0, attacker_type_id: 0x32, attacker_domain: 1, attacker_splash_percent: 49, attacker_type_0x40: 0x1ac, attacker_z: 83, attacker_flag8_bit5: false, defender_type_id: 0x68, defender_domain: 1, defender_type_0x2b8_bit2: false, defender_splash_divisor: 2, defender_flags_0x68: 0x8200000, defender_flags_0x6c_bit12: false, defender_z: 31, defender_facing: -2147483648, defender_facing_entrench: 1288585045, defender_overkill_stamp: 0, defender_word_0xa4: 2918, attacker_vf_0xe4: 2780, current_frame: 39259, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 2 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: false, defender_vf_0x1c: false, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 6, height_bonus: 25, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 1, trace: 0xa000002 },
        Sample { i: DamageInput { balance_pct: 100, attack: 89, armor: 8, attacker_masks: 0x0, defender_masks: 0x80004, attack_dir: -715827883, splash_flag: 0, overkill_gate: 0, attacker_player: 5, attacker_type_id: 0x78, attacker_domain: 0, attacker_splash_percent: 59, attacker_type_0x40: 0x6f2, attacker_z: 707104054, attacker_flag8_bit5: false, defender_type_id: 0x1a0, defender_domain: 1, defender_type_0x2b8_bit2: false, defender_splash_divisor: 1, defender_flags_0x68: 0xd0108, defender_flags_0x6c_bit12: false, defender_z: 7338, defender_facing: 2030144278, defender_facing_entrench: -945223013, defender_overkill_stamp: 11730, defender_word_0xa4: 1470, attacker_vf_0xe4: 2104, current_frame: 90106, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: 7 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 2, height_bonus: 33, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 63, trace: 0x220 },
        Sample { i: DamageInput { balance_pct: 100, attack: 365, armor: -1441, attacker_masks: 0x24e16246, defender_masks: 0x24, attack_dir: -1168787622, splash_flag: 0, overkill_gate: 1, attacker_player: 0, attacker_type_id: 0x198, attacker_domain: 0, attacker_splash_percent: 16, attacker_type_0x40: 0x1ac, attacker_z: -2147483648, attacker_flag8_bit5: true, defender_type_id: 0x21d, defender_domain: 1, defender_type_0x2b8_bit2: false, defender_splash_divisor: 2, defender_flags_0x68: 0x1001, defender_flags_0x6c_bit12: false, defender_z: -897214647, defender_facing: 1610612735, defender_facing_entrench: 1610612735, defender_overkill_stamp: 21304, defender_word_0xa4: 1813, attacker_vf_0xe4: 2746, current_frame: 70409, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 7 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 3, height_bonus: 43, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 36500, trace: 0xc0000 },
        Sample { i: DamageInput { balance_pct: 465, attack: 1766, armor: 8, attacker_masks: 0x400000, defender_masks: 0xdef105ec, attack_dir: -1610612735, splash_flag: 0, overkill_gate: 1, attacker_player: 1, attacker_type_id: 0x1bc, attacker_domain: 0, attacker_splash_percent: 21, attacker_type_0x40: 0x1ac, attacker_z: -3991, attacker_flag8_bit5: false, defender_type_id: 0x52, defender_domain: 1, defender_type_0x2b8_bit2: false, defender_splash_divisor: 1, defender_flags_0x68: 0x2000, defender_flags_0x6c_bit12: false, defender_z: 1, defender_facing: 1610612735, defender_facing_entrench: -2147483648, defender_overkill_stamp: 94014, defender_word_0xa4: 2960, attacker_vf_0xe4: 564, current_frame: 31077, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 2 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 1634, trace: 0xa0 },
        Sample { i: DamageInput { balance_pct: 1323, attack: 48, armor: 51, attacker_masks: 0x13527e1f, defender_masks: 0x201020, attack_dir: -2107609591, splash_flag: 0, overkill_gate: 0, attacker_player: 7, attacker_type_id: 0x32, attacker_domain: 2, attacker_splash_percent: 71, attacker_type_0x40: 0x1ac, attacker_z: -24, attacker_flag8_bit5: false, defender_type_id: 0x3c, defender_domain: 0, defender_type_0x2b8_bit2: false, defender_splash_divisor: 1, defender_flags_0x68: 0x2004, defender_flags_0x6c_bit12: true, defender_z: 2147483647, defender_facing: 203222991, defender_facing_entrench: -1416790949, defender_overkill_stamp: 0, defender_word_0xa4: 1418, attacker_vf_0xe4: 2545, current_frame: 71805, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 7 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 254, trace: 0x100002 },
        Sample { i: DamageInput { balance_pct: 754, attack: 1454, armor: 20, attacker_masks: 0x0, defender_masks: 0x61a6f60e, attack_dir: 584194834, splash_flag: 0, overkill_gate: 1, attacker_player: 2, attacker_type_id: 0x1b7, attacker_domain: 1, attacker_splash_percent: 67, attacker_type_0x40: 0x1ac, attacker_z: -63, attacker_flag8_bit5: false, defender_type_id: 0xaa, defender_domain: 0, defender_type_0x2b8_bit2: true, defender_splash_divisor: 2, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: false, defender_z: 10, defender_facing: -1828522397, defender_facing_entrench: 715827881, defender_overkill_stamp: 62731, defender_word_0xa4: 1519, attacker_vf_0xe4: 2514, current_frame: 97021, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 6 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 1076, trace: 0x0 },
        Sample { i: DamageInput { balance_pct: 100, attack: -8860, armor: 0, attacker_masks: 0x600001, defender_masks: 0x5, attack_dir: -715827883, splash_flag: 1, overkill_gate: 0, attacker_player: 3, attacker_type_id: 0xaf, attacker_domain: 2, attacker_splash_percent: 81, attacker_type_0x40: 0x1ac, attacker_z: 3, attacker_flag8_bit5: true, defender_type_id: 0x21d, defender_domain: 1, defender_type_0x2b8_bit2: true, defender_splash_divisor: 4, defender_flags_0x68: 0x320cf7e7, defender_flags_0x6c_bit12: false, defender_z: 1000, defender_facing: -715827883, defender_facing_entrench: -1, defender_overkill_stamp: 0, defender_word_0xa4: 2586, attacker_vf_0xe4: 1283, current_frame: 90121, game_flag_0x821_bit1: false, tile_rocky: false, tile_owner: 2 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: false, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 6, height_bonus: 20, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -1434, trace: 0x8061 },
        Sample { i: DamageInput { balance_pct: 100, attack: 376, armor: -2061, attacker_masks: 0x2002000, defender_masks: 0x0, attack_dir: -1812990402, splash_flag: 0, overkill_gate: 0, attacker_player: 2, attacker_type_id: 0x9e, attacker_domain: 1, attacker_splash_percent: 84, attacker_type_0x40: 0x1ab, attacker_z: 28, attacker_flag8_bit5: false, defender_type_id: 0x1c7, defender_domain: 0, defender_type_0x2b8_bit2: false, defender_splash_divisor: 3, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: false, defender_z: -2147483648, defender_facing: -1579467042, defender_facing_entrench: -1, defender_overkill_stamp: 0, defender_word_0xa4: 2271, attacker_vf_0xe4: 1641, current_frame: 3033, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 4 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: true, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: true, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 150, trace: 0x100000 },
        Sample { i: DamageInput { balance_pct: 62, attack: 1847, armor: 6, attacker_masks: 0x3fdd8af8, defender_masks: 0x20, attack_dir: -539231135, splash_flag: 0, overkill_gate: 0, attacker_player: 0, attacker_type_id: 0x35, attacker_domain: 1, attacker_splash_percent: 5, attacker_type_0x40: 0x1ab, attacker_z: -55, attacker_flag8_bit5: true, defender_type_id: 0x120, defender_domain: 0, defender_type_0x2b8_bit2: false, defender_splash_divisor: 4, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: true, defender_z: -14, defender_facing: -1, defender_facing_entrench: 1610612736, defender_overkill_stamp: 0, defender_word_0xa4: 2458, attacker_vf_0xe4: 1947, current_frame: 21641, game_flag_0x821_bit1: false, tile_rocky: false, tile_owner: 4 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 50, trace: 0x40002 },
        Sample { i: DamageInput { balance_pct: 100, attack: 1507, armor: 5, attacker_masks: 0x983b54da, defender_masks: 0x8040004, attack_dir: -1, splash_flag: 0, overkill_gate: 0, attacker_player: 2, attacker_type_id: 0x21d, attacker_domain: 1, attacker_splash_percent: 48, attacker_type_0x40: 0x1ab, attacker_z: 65, attacker_flag8_bit5: false, defender_type_id: 0xfe, defender_domain: 1, defender_type_0x2b8_bit2: true, defender_splash_divisor: 4, defender_flags_0x68: 0x9, defender_flags_0x6c_bit12: true, defender_z: -998279948, defender_facing: 1610612735, defender_facing_entrench: -715827882, defender_overkill_stamp: 0, defender_word_0xa4: 2815, attacker_vf_0xe4: 39, current_frame: 83700, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 1 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: false, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 35, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -2252144, trace: 0x22000002 },
        Sample { i: DamageInput { balance_pct: 501, attack: 32767, armor: 642567245, attacker_masks: 0x189aeba7, defender_masks: 0x0, attack_dir: 0, splash_flag: 1, overkill_gate: 1, attacker_player: 2, attacker_type_id: 0x32, attacker_domain: 0, attacker_splash_percent: 32, attacker_type_0x40: 0x1ab, attacker_z: -9567, attacker_flag8_bit5: true, defender_type_id: 0x10b, defender_domain: 2, defender_type_0x2b8_bit2: false, defender_splash_divisor: 3, defender_flags_0x68: 0x10240000, defender_flags_0x6c_bit12: true, defender_z: 55, defender_facing: 1610612735, defender_facing_entrench: 1610612735, defender_overkill_stamp: 0, defender_word_0xa4: 1805, attacker_vf_0xe4: 2351, current_frame: 46685, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 7 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: false, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -642566369, trace: 0xc040 },
        Sample { i: DamageInput { balance_pct: 2077, attack: 8959, armor: 8, attacker_masks: 0x0, defender_masks: 0x200005, attack_dir: 715827882, splash_flag: 0, overkill_gate: 0, attacker_player: 5, attacker_type_id: 0x21d, attacker_domain: 2, attacker_splash_percent: 27, attacker_type_0x40: 0x1ab, attacker_z: 47, attacker_flag8_bit5: true, defender_type_id: 0x97, defender_domain: 2, defender_type_0x2b8_bit2: true, defender_splash_divisor: 1, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: true, defender_z: 98, defender_facing: 715827882, defender_facing_entrench: 1331252517, defender_overkill_stamp: 0, defender_word_0xa4: 2345, attacker_vf_0xe4: 2647, current_frame: 76244, game_flag_0x821_bit1: false, tile_rocky: false, tile_owner: 5 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 9295, trace: 0x481 },
        Sample { i: DamageInput { balance_pct: 2300, attack: 298, armor: 56, attacker_masks: 0x0, defender_masks: 0x440008, attack_dir: -1610612735, splash_flag: 0, overkill_gate: 0, attacker_player: 0, attacker_type_id: 0x21d, attacker_domain: 2, attacker_splash_percent: 0, attacker_type_0x40: 0x1ab, attacker_z: 2147483647, attacker_flag8_bit5: false, defender_type_id: 0x21d, defender_domain: 1, defender_type_0x2b8_bit2: true, defender_splash_divisor: 3, defender_flags_0x68: 0x5ca5f69f, defender_flags_0x6c_bit12: false, defender_z: -1, defender_facing: 2115057203, defender_facing_entrench: 1610612735, defender_overkill_stamp: 0, defender_word_0xa4: 1238, attacker_vf_0xe4: 1533, current_frame: 91104, game_flag_0x821_bit1: false, tile_rocky: false, tile_owner: 6 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 8, height_bonus: 11, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 0, trace: 0x100c0444 },
        Sample { i: DamageInput { balance_pct: 100, attack: 2122735353, armor: -6278, attacker_masks: 0x80001, defender_masks: 0x984b593a, attack_dir: 254252686, splash_flag: 0, overkill_gate: 1, attacker_player: 4, attacker_type_id: 0x21d, attacker_domain: 0, attacker_splash_percent: 32, attacker_type_0x40: 0x1ac, attacker_z: 28, attacker_flag8_bit5: false, defender_type_id: 0x1ad, defender_domain: 2, defender_type_0x2b8_bit2: false, defender_splash_divisor: 2, defender_flags_0x68: 0x18040000, defender_flags_0x6c_bit12: false, defender_z: -7065, defender_facing: -77281550, defender_facing_entrench: 1610612735, defender_overkill_stamp: 0, defender_word_0xa4: 786, attacker_vf_0xe4: 683, current_frame: 69528, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 3 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 688830, trace: 0x1040008 },
        Sample { i: DamageInput { balance_pct: 53, attack: 5031, armor: 14, attacker_masks: 0x2041000, defender_masks: 0x0, attack_dir: -1610612735, splash_flag: 0, overkill_gate: 1, attacker_player: 2, attacker_type_id: 0x34, attacker_domain: 1, attacker_splash_percent: 38, attacker_type_0x40: 0x1ab, attacker_z: -6508, attacker_flag8_bit5: false, defender_type_id: 0x94, defender_domain: 2, defender_type_0x2b8_bit2: true, defender_splash_divisor: 2, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: true, defender_z: 0, defender_facing: -1743836373, defender_facing_entrench: -1610612735, defender_overkill_stamp: 1000, defender_word_0xa4: 5, attacker_vf_0xe4: 7, current_frame: 1010, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 0 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 63, trace: 0xe004e0 },
        Sample { i: DamageInput { balance_pct: 100, attack: 606, armor: 16, attacker_masks: 0x8e63832, defender_masks: 0x74c7bd2d, attack_dir: -715827883, splash_flag: 1, overkill_gate: 1, attacker_player: 1, attacker_type_id: 0x5b, attacker_domain: 1, attacker_splash_percent: 16, attacker_type_0x40: 0xffff8000, attacker_z: 68, attacker_flag8_bit5: false, defender_type_id: 0xa8, defender_domain: 2, defender_type_0x2b8_bit2: true, defender_splash_divisor: 3, defender_flags_0x68: 0x30d36109, defender_flags_0x6c_bit12: true, defender_z: -9518, defender_facing: -872325473, defender_facing_entrench: -475131563, defender_overkill_stamp: 0, defender_word_0xa4: 2990, attacker_vf_0xe4: 961, current_frame: 72916, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 0 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: false, defender_vf_0xcc: true, defender_vf_0xd0: true, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 60600, trace: 0xfc404 },
        Sample { i: DamageInput { balance_pct: 100, attack: -18, armor: 4921, attacker_masks: 0xae87a4f5, defender_masks: 0x20, attack_dir: -1288629513, splash_flag: 0, overkill_gate: 0, attacker_player: 7, attacker_type_id: 0x21d, attacker_domain: 1, attacker_splash_percent: 78, attacker_type_0x40: 0xffffffac, attacker_z: -38, attacker_flag8_bit5: true, defender_type_id: 0x1e3, defender_domain: 1, defender_type_0x2b8_bit2: false, defender_splash_divisor: 3, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: true, defender_z: 80, defender_facing: -769003430, defender_facing_entrench: -227285424, defender_overkill_stamp: 70391, defender_word_0xa4: 415, attacker_vf_0xe4: 96, current_frame: 11624, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 4 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -4921, trace: 0x8 },
        Sample { i: DamageInput { balance_pct: 1443, attack: 1356, armor: 17, attacker_masks: 0xd7f923fd, defender_masks: 0x10010108, attack_dir: 1724775693, splash_flag: 1, overkill_gate: 1, attacker_player: 6, attacker_type_id: 0xa7, attacker_domain: 2, attacker_splash_percent: 62, attacker_type_0x40: 0x1ab, attacker_z: 5871, attacker_flag8_bit5: false, defender_type_id: 0xef, defender_domain: 2, defender_type_0x2b8_bit2: false, defender_splash_divisor: 1, defender_flags_0x68: 0x1aaaa72e, defender_flags_0x6c_bit12: false, defender_z: 20, defender_facing: 1394250379, defender_facing_entrench: -313600988, defender_overkill_stamp: 1000, defender_word_0xa4: 5, attacker_vf_0xe4: 7, current_frame: 1010, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 5 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 9, height_bonus: 36, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 98, trace: 0x4c0c302 },
        Sample { i: DamageInput { balance_pct: 100, attack: 1443, armor: 6, attacker_masks: 0xcfed7c5a, defender_masks: 0x8042000, attack_dir: 836680365, splash_flag: 1, overkill_gate: 1, attacker_player: 4, attacker_type_id: 0x1bb, attacker_domain: 2, attacker_splash_percent: 93, attacker_type_0x40: 0x1ab, attacker_z: 1000, attacker_flag8_bit5: false, defender_type_id: 0x11b, defender_domain: 0, defender_type_0x2b8_bit2: true, defender_splash_divisor: 2, defender_flags_0x68: 0x9a065561, defender_flags_0x6c_bit12: true, defender_z: -22, defender_facing: 1243549871, defender_facing_entrench: -708445436, defender_overkill_stamp: 14001, defender_word_0xa4: 1938, attacker_vf_0xe4: 1540, current_frame: 64013, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 3 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: true, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 48, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 144300, trace: 0xfc407 },
        Sample { i: DamageInput { balance_pct: 207, attack: 1885, armor: 15, attacker_masks: 0x0, defender_masks: 0x240020, attack_dir: -505003835, splash_flag: 1, overkill_gate: 0, attacker_player: 2, attacker_type_id: 0x21d, attacker_domain: 0, attacker_splash_percent: 13, attacker_type_0x40: 0x1ac, attacker_z: -2147483648, attacker_flag8_bit5: false, defender_type_id: 0x91, defender_domain: 1, defender_type_0x2b8_bit2: true, defender_splash_divisor: 2, defender_flags_0x68: 0x400000, defender_flags_0x6c_bit12: false, defender_z: -17, defender_facing: 1229243335, defender_facing_entrench: -2116188361, defender_overkill_stamp: 0, defender_word_0xa4: 2449, attacker_vf_0xe4: 120, current_frame: 37251, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 7 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: true, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 41, trace: 0x7e428 },
        Sample { i: DamageInput { balance_pct: 1543, attack: 375, armor: -73, attacker_masks: 0x0, defender_masks: 0x2000000, attack_dir: 1607804520, splash_flag: 0, overkill_gate: 1, attacker_player: 6, attacker_type_id: 0xb8, attacker_domain: 1, attacker_splash_percent: 13, attacker_type_0x40: 0x1ac, attacker_z: -39, attacker_flag8_bit5: false, defender_type_id: 0x164, defender_domain: 0, defender_type_0x2b8_bit2: false, defender_splash_divisor: 2, defender_flags_0x68: 0x23f83e3b, defender_flags_0x6c_bit12: true, defender_z: 1607615616, defender_facing: 715827882, defender_facing_entrench: 459197825, defender_overkill_stamp: 1000, defender_word_0xa4: 5, attacker_vf_0xe4: 7, current_frame: 1010, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: 3 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 1, trace: 0x8e80200 },
        Sample { i: DamageInput { balance_pct: 2510, attack: 119, armor: 28, attacker_masks: 0x80542142, defender_masks: 0x40000, attack_dir: 1584878706, splash_flag: 0, overkill_gate: 0, attacker_player: 4, attacker_type_id: 0x1e5, attacker_domain: 2, attacker_splash_percent: 16, attacker_type_0x40: 0x1ac, attacker_z: -89, attacker_flag8_bit5: true, defender_type_id: 0xce, defender_domain: 2, defender_type_0x2b8_bit2: true, defender_splash_divisor: 3, defender_flags_0x68: 0x10108, defender_flags_0x6c_bit12: true, defender_z: 14, defender_facing: 965641702, defender_facing_entrench: 1610612735, defender_overkill_stamp: 0, defender_word_0xa4: 2861, attacker_vf_0xe4: 1930, current_frame: 20461, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: 3 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: false, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: false, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 71, trace: 0x404 },
        Sample { i: DamageInput { balance_pct: 2220, attack: 1573, armor: 32, attacker_masks: 0x8000000, defender_masks: 0x1020, attack_dir: 1527445482, splash_flag: 0, overkill_gate: 0, attacker_player: 6, attacker_type_id: 0x1b8, attacker_domain: 2, attacker_splash_percent: 8, attacker_type_0x40: 0x1ac, attacker_z: 55, attacker_flag8_bit5: true, defender_type_id: 0x15b, defender_domain: 1, defender_type_0x2b8_bit2: false, defender_splash_divisor: 3, defender_flags_0x68: 0x80000, defender_flags_0x6c_bit12: false, defender_z: -24, defender_facing: 712215240, defender_facing_entrench: 1610612735, defender_overkill_stamp: 94840, defender_word_0xa4: 1861, attacker_vf_0xe4: 1869, current_frame: 43085, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: 3 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 11911, trace: 0x2422a9 },
        Sample { i: DamageInput { balance_pct: 100, attack: 6318, armor: 31, attacker_masks: 0x42020, defender_masks: 0x0, attack_dir: -1610612735, splash_flag: 0, overkill_gate: 1, attacker_player: 0, attacker_type_id: 0x21d, attacker_domain: 1, attacker_splash_percent: 63, attacker_type_0x40: 0x58, attacker_z: -4570, attacker_flag8_bit5: true, defender_type_id: 0x1a9, defender_domain: 2, defender_type_0x2b8_bit2: false, defender_splash_divisor: 3, defender_flags_0x68: 0xafdc13e9, defender_flags_0x6c_bit12: false, defender_z: 3832, defender_facing: -1610612735, defender_facing_entrench: 715827882, defender_overkill_stamp: 1000, defender_word_0xa4: 5, attacker_vf_0xe4: 7, current_frame: 1010, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: -1 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: false },
                r: CombatRules { height_increment: 10, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 419554, trace: 0x4d82210 },
        Sample { i: DamageInput { balance_pct: 100, attack: 32, armor: 22, attacker_masks: 0x40bf6260, defender_masks: 0x40000, attack_dir: 1639124392, splash_flag: 1, overkill_gate: 0, attacker_player: 2, attacker_type_id: 0x1bc, attacker_domain: 1, attacker_splash_percent: 17, attacker_type_0x40: 0x3d, attacker_z: -357426906, attacker_flag8_bit5: false, defender_type_id: 0x1a1, defender_domain: 1, defender_type_0x2b8_bit2: true, defender_splash_divisor: 2, defender_flags_0x68: 0x10010108, defender_flags_0x6c_bit12: false, defender_z: -32768, defender_facing: -1610612735, defender_facing_entrench: 1595608193, defender_overkill_stamp: 78349, defender_word_0xa4: 1563, attacker_vf_0xe4: 2206, current_frame: 76984, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 7 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: false, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -23, trace: 0x5c415 },
        Sample { i: DamageInput { balance_pct: 100, attack: 3, armor: 13, attacker_masks: 0x0, defender_masks: 0x20, attack_dir: 1610612735, splash_flag: 0, overkill_gate: 0, attacker_player: 2, attacker_type_id: 0x1bc, attacker_domain: 1, attacker_splash_percent: 100, attacker_type_0x40: 0x1ab, attacker_z: -1737990542, attacker_flag8_bit5: false, defender_type_id: 0x56, defender_domain: 0, defender_type_0x2b8_bit2: false, defender_splash_divisor: 3, defender_flags_0x68: 0x80000, defender_flags_0x6c_bit12: true, defender_z: -1230367626, defender_facing: 1003932517, defender_facing_entrench: 1610612735, defender_overkill_stamp: 55960, defender_word_0xa4: 161, attacker_vf_0xe4: 870, current_frame: 49325, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: 3 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: true, recapture_owner_matches: true, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 2, trace: 0x28040248 },
        Sample { i: DamageInput { balance_pct: 100, attack: 594, armor: 5506, attacker_masks: 0x11109, defender_masks: 0x1, attack_dir: 1118355848, splash_flag: 0, overkill_gate: 1, attacker_player: 5, attacker_type_id: 0x1bc, attacker_domain: 1, attacker_splash_percent: 73, attacker_type_0x40: 0xfffffff6, attacker_z: 47, attacker_flag8_bit5: false, defender_type_id: 0x155, defender_domain: 1, defender_type_0x2b8_bit2: true, defender_splash_divisor: 4, defender_flags_0x68: 0xcf5fc01d, defender_flags_0x6c_bit12: true, defender_z: -5235, defender_facing: -1, defender_facing_entrench: -1205680259, defender_overkill_stamp: 1000, defender_word_0xa4: 5, attacker_vf_0xe4: 7, current_frame: 1010, game_flag_0x821_bit1: true, tile_rocky: false, tile_owner: -1 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: false, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: true, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: true, defender_tech_0x143: true, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: true },
                r: CombatRules { height_increment: 8, height_bonus: 8, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 2122929, trace: 0x2ec22b3 },
        Sample { i: DamageInput { balance_pct: 100, attack: -1, armor: 95, attacker_masks: 0x10000028, defender_masks: 0x40008, attack_dir: -1610612736, splash_flag: 1, overkill_gate: 1, attacker_player: 7, attacker_type_id: 0x21d, attacker_domain: 1, attacker_splash_percent: 43, attacker_type_0x40: 0x1ac, attacker_z: 4277, attacker_flag8_bit5: true, defender_type_id: 0x6e, defender_domain: 0, defender_type_0x2b8_bit2: true, defender_splash_divisor: 1, defender_flags_0x68: 0x8000024, defender_flags_0x6c_bit12: false, defender_z: 174135787, defender_facing: 454556314, defender_facing_entrench: 715827881, defender_overkill_stamp: 39794, defender_word_0xa4: 1882, attacker_vf_0xe4: 1290, current_frame: 61078, game_flag_0x821_bit1: false, tile_rocky: true, tile_owner: 1 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: false, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: true, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: -127, trace: 0x100c406 },
        Sample { i: DamageInput { balance_pct: 100, attack: 1027, armor: 100, attacker_masks: 0x8, defender_masks: 0x2040020, attack_dir: 1670151182, splash_flag: 0, overkill_gate: 0, attacker_player: 3, attacker_type_id: 0x1bc, attacker_domain: 1, attacker_splash_percent: 5, attacker_type_0x40: 0x7, attacker_z: 61, attacker_flag8_bit5: false, defender_type_id: 0x21d, defender_domain: 0, defender_type_0x2b8_bit2: true, defender_splash_divisor: 3, defender_flags_0x68: 0x7ef0a099, defender_flags_0x6c_bit12: false, defender_z: 100, defender_facing: -671371738, defender_facing_entrench: 627011675, defender_overkill_stamp: 0, defender_word_0xa4: 1882, attacker_vf_0xe4: 1979, current_frame: 76592, game_flag_0x821_bit1: false, tile_rocky: false, tile_owner: 6 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: true, defender_vf_0xd8: true, defender_type_vf_0x10c: false, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: true, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: false, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 410800, trace: 0x18240f },
        Sample { i: DamageInput { balance_pct: 315, attack: 1929, armor: 39, attacker_masks: 0x2000, defender_masks: 0x10000004, attack_dir: -1947179307, splash_flag: 1, overkill_gate: 1, attacker_player: 6, attacker_type_id: 0x32, attacker_domain: 0, attacker_splash_percent: 39, attacker_type_0x40: 0x1ac, attacker_z: 5758, attacker_flag8_bit5: true, defender_type_id: 0x5e, defender_domain: 1, defender_type_0x2b8_bit2: false, defender_splash_divisor: 3, defender_flags_0x68: 0xdf527f0a, defender_flags_0x6c_bit12: false, defender_z: 50147493, defender_facing: -1406963197, defender_facing_entrench: -1, defender_overkill_stamp: 1000, defender_word_0xa4: 5, attacker_vf_0xe4: 7, current_frame: 1010, game_flag_0x821_bit1: false, tile_rocky: false, tile_owner: 7 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: false, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: false, defender_build_0x20: true, recapture_owner_matches: false, defender_carrier_vf_0x184: false, defender_carrier_vf_0x20: true, defender_mount_attack_is_zero: false, attacker_tech_0x42: true, attacker_tech_0x139: false, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 19, trace: 0xc0e001 },
        Sample { i: DamageInput { balance_pct: 1479, attack: -56, armor: -46, attacker_masks: 0xa400000, defender_masks: 0x44000c, attack_dir: 1748272683, splash_flag: 0, overkill_gate: 0, attacker_player: 4, attacker_type_id: 0xff, attacker_domain: 0, attacker_splash_percent: 77, attacker_type_0x40: 0xffffffa4, attacker_z: -70, attacker_flag8_bit5: false, defender_type_id: 0x1ab, defender_domain: 1, defender_type_0x2b8_bit2: true, defender_splash_divisor: 2, defender_flags_0x68: 0x40000, defender_flags_0x6c_bit12: false, defender_z: 81, defender_facing: -1558135631, defender_facing_entrench: -413594517, defender_overkill_stamp: 0, defender_word_0xa4: 2433, attacker_vf_0xe4: 2460, current_frame: 27156, game_flag_0x821_bit1: true, tile_rocky: true, tile_owner: 5 },
                p: DamagePredicates { mask_fixup_authorised: true, attacker_vf_0x18: true, attacker_vf_0x1c: false, attacker_vf_0x20: true, attacker_vf_0x130: false, attacker_type_vf_0x10c: true, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: true, defender_vf_0x20: false, defender_vf_0x120: true, defender_vf_0xcc: false, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: true, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: true, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: false, defender_tech_0x143: true, defender_tech_0x109: false, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: true },
                r: CombatRules { height_increment: 1, height_bonus: 38, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 1, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 32, trace: 0x1000414 },
        Sample { i: DamageInput { balance_pct: 100, attack: 261, armor: -46, attacker_masks: 0x2008, defender_masks: 0x28, attack_dir: 715827882, splash_flag: 1, overkill_gate: 0, attacker_player: 3, attacker_type_id: 0x21d, attacker_domain: 1, attacker_splash_percent: 75, attacker_type_0x40: 0xffffee95, attacker_z: -83, attacker_flag8_bit5: true, defender_type_id: 0x135, defender_domain: 0, defender_type_0x2b8_bit2: false, defender_splash_divisor: 2, defender_flags_0x68: 0x0, defender_flags_0x6c_bit12: false, defender_z: 51, defender_facing: 715827882, defender_facing_entrench: -803173840, defender_overkill_stamp: 77418, defender_word_0xa4: 2926, attacker_vf_0xe4: 541, current_frame: 95569, game_flag_0x821_bit1: false, tile_rocky: false, tile_owner: 6 },
                p: DamagePredicates { mask_fixup_authorised: false, attacker_vf_0x18: false, attacker_vf_0x1c: true, attacker_vf_0x20: true, attacker_vf_0x130: true, attacker_type_vf_0x10c: false, attacker_table_vf_0x20: true, defender_vf_0x18: true, defender_vf_0x1c: false, defender_vf_0x20: true, defender_vf_0x120: true, defender_vf_0xcc: true, defender_vf_0xd0: false, defender_vf_0xd8: true, defender_type_vf_0x10c: true, defender_build_flag: true, defender_build_0x20: false, recapture_owner_matches: false, defender_carrier_vf_0x184: true, defender_carrier_vf_0x20: false, defender_mount_attack_is_zero: false, attacker_tech_0x42: false, attacker_tech_0x139: true, attacker_tech_0x83: true, defender_tech_0x216: true, defender_tech_0x143: false, defender_tech_0x109: true, step10_bonus_applies: false, step11_player_prop_0xf: false, step27_player_prop_0xd: false, step12_team_differs: false, game_mode_is_2: false },
                r: CombatRules { height_increment: 1, height_bonus: 0, flank_bonus: 50, cavalry_flank_bonus: 40, vehicle_flank_bonus: 33, rocky_modifier: 256, overkill_frames: 30, overkill_damage: 85, entrenchment_modifier: 128, river_modifier: 128, recapture_city_modifier: 512, red_fort_air_defense: 50, super_immune: 0, russian_cossack_damage: 25, antipater_entrench_bonus: 204 },
                expect: 68, trace: 0xc00a },
        ]
    }

    #[test]
    fn get_damage_matches_oracle_model_samples() {
        let u = UnreachedTerms::default();
        let mut cov = 0u64;
        for (n, s) in samples().iter().enumerate() {
            let (d, t) = get_damage_traced(&s.i, &s.p, &s.r, &u);
            assert_eq!(d, s.expect, "sample {n}: damage");
            assert_eq!(t, s.trace, "sample {n}: step trace");
            cov |= t;
        }
        // every retail-verified step is exercised by at least one sample
        let unverified = (1 << 11) | (1 << 12);
        assert_eq!(cov | unverified, (1u64 << 30) - 1, "step coverage {cov:#x}");
    }

    #[test]
    fn sixteenth_split_unit_branch() {
        // D*scale below the floor -> 0x100 -> q = 16 -> whole 1, frac 0
        assert_eq!(scale_to_sixteenths(0, 0x100, -1, 1, 1, true, false), Sixteenths { whole: 1, frac: 0 });
        // 7 hp * 0x100 = 1792 -> q = 112 -> whole 7, frac 0
        assert_eq!(scale_to_sixteenths(7, 0x100, -1, 1, 1, true, false), Sixteenths { whole: 7, frac: 0 });
        // uber_size 2: 1792/2 = 896 -> q 56 -> whole 3 frac 8
        assert_eq!(scale_to_sixteenths(7, 0x100, -1, 1, 2, true, false), Sixteenths { whole: 3, frac: 8 });
        // ammo_per_att 3 with ammo_index >= 0: 1792/3 = 597 -> q 37 -> whole 2 frac 5
        assert_eq!(scale_to_sixteenths(7, 0x100, 0, 3, 1, true, false), Sixteenths { whole: 2, frac: 5 });
        // ammo_index < 0 ignores ammo_per_att
        assert_eq!(scale_to_sixteenths(7, 0x100, -1, 3, 1, true, false), Sixteenths { whole: 7, frac: 0 });
        // building branch: no floor, no uber divide
        assert_eq!(scale_to_sixteenths(0, 0x100, -1, 1, 4, false, true), Sixteenths { whole: 0, frac: 0 });
        assert_eq!(scale_to_sixteenths(-5, 0x100, -1, 1, 4, false, true), Sixteenths { whole: -5, frac: 0 });
        assert_eq!(scale_to_sixteenths(-5, 0x90, -1, 1, 4, false, true), Sixteenths { whole: -2, frac: -13 });
        // neither plane: pass-through
        assert_eq!(scale_to_sixteenths(9, 0x100, -1, 1, 1, false, false), Sixteenths { whole: 9, frac: 0 });
    }

    #[test]
    fn frac_accumulator_wraps_as_a_char() {
        assert_eq!(accumulate_frac(0, 0, 1), (0, 1));
        assert_eq!(accumulate_frac(15, 0, 1), (1, 0));
        assert_eq!(accumulate_frac(15, 2, 3), (3, 2));
        // (char)(100 + 100) = -56 -> carry trunc(-56/16) = -3, rem -8: heals.
        assert_eq!(accumulate_frac(100, 0, 100), (-3, -8));
    }

    #[test]
    fn uber_threshold_splits_hits() {
        assert_eq!(uber_threshold(100, 1, true, 1), 100);
        assert_eq!(uber_threshold(100, 3, false, 3), 33);
        assert_eq!(uber_threshold(100, 3, true, 2), 33);
        // lone captain carries the remainder: 100 - (2*100)/3 = 34
        assert_eq!(uber_threshold(100, 3, true, 1), 34);
    }

    #[test]
    fn classifiers() {
        assert_eq!(flank_level(0), 0);
        assert_eq!(flank_level(0x2aaa_aaaa), 2);
        assert_eq!(flank_level(0x6000_0000), 1);
        assert_eq!(flank_level(0xa000_0000), 1);
        assert_eq!(flank_level(0xa000_0001), 2);
        assert_eq!(flank_level(0xd555_5556), 0);
        assert_eq!(entrench_dir_level(0), 0);
        assert_eq!(entrench_dir_level(0x2aaa_aaaa), 2);
        assert_eq!(entrench_dir_level(0xd555_5555), 2);
        assert_eq!(entrench_dir_level(0xd555_5556), 0);
    }
}
