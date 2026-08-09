//! Combat: the resolution loop **around** `ObjectData::get_damage`.
//!
//! Lane `mech:combat`. Checksum channels served: **`units`** (`CheckSums::check_units`
//! `0x009371D0`) and **`deaths`** (`CheckSums::check_deaths` `0x00936BB0`).
//!
//! # What this module is
//!
//! `ObjectData::get_damage` (`0x00644130`) — the 31-step arithmetic chain — is already
//! ported in [`crate::mechanics`]. This module is everything that *calls* it and everything
//! that *consumes* its result:
//!
//! ```text
//! Unit::fight              0x005FD4D0  unit.cpp:11131   the attack cycle + recharge
//!  └─ Object::do_damage    0x0064A480  object.cpp:5812  the applier: scale, splash, overkill
//!      ├─ ObjectData::get_damage 0x00644130             (crate::mechanics::damage)
//!      ├─ Unit::target_opportunity 0x005FFFC0           retaliation
//!      ├─ Object::do_damage (recursive)                 splash victims
//!      └─ Object::take_damage 0x00652020 object.cpp:6628 hp, 1/16ths, death, cascade
//!          └─ Object::die     0x00647080 object.cpp:2936 corpse hold, DeathObj
//! ```
//!
//! # Provenance and fidelity
//!
//! Everything here is `[measured]` against `ron-bin/riseofnations.exe`
//! (sha256 `30478a44…625079`) plus `ron-bin/sbl/rise.pdb`, and against the shipped
//! `ron-data/rules.xml`. Every function carries the VA of the instruction sequence it
//! reproduces.
//!
//! **Fidelity is Tier C throughout: structure and constants read out of the binary,
//! never executed against retail.** There is no oracle harness for `Object::do_damage`
//! (9,214 bytes, walks the object table, the leader array, the world and six virtuals) or
//! for `Object::take_damage` (5,216 bytes). Nothing in this file has been differentially
//! tested. The one Tier-B item it *uses* is [`crate::mechanics::flank_level`]
//! (`0x0092CFE0`, 500,017 inputs). Do not promote anything here by proximity to it.
//!
//! Where a value came from decompiled C rather than from capstone output it says so on
//! the line; per the charter, decompiled C is a hypothesis. `Object::do_damage` is over
//! the 8,192-byte cap of `re/decomp-all/`, so **all of its arithmetic below was read from
//! capstone output**, not from Ghidra.
//!
//! # What is deliberately *not* here
//!
//! * **Target ranking.** `Object::compare_target` (`0x0064E5C0`, 3,553 B) and
//!   `Object::find_nearby_target` (`0x00648DA0`, 4,042 B) were mapped but not reduced here.
//!   Only the cheap gates — [`poor_target`], [`in_attack_range`] — are in this file.
//!   **They now live in [`crate::systems::target`]** (lane `assembly:target-selection`),
//!   together with the `World::wdata` acquisition grid, `attack_dir`'s settled semantics,
//!   and an engagement driver that composes this module's range/recharge pieces with
//!   `crate::mechanics::damage`. That module imports this one; nothing here imports it, so
//!   this file still builds standalone.
//! * **Experience / veterancy.** There is none. `ObjectData` has no experience field,
//!   `UnitData` has no kill counter, and no `Object::*` function reads one. The only
//!   per-unit progression is the player-wide `military_level` upgrade term already in
//!   [`crate::mechanics::get_attack`]. Recording that as a *measured absence*, not as an
//!   omission.
//! * **The balance matrix loader.** `crate::balance::BalanceTable` already loads
//!   `schema/live/balance-real.bin`; [`balance_percent`] takes its slice rather than
//!   duplicating the loader or embedding the bytes.
//!
//! # Standalone build
//!
//! This module has **no `crate::` imports**, so it can be checked on its own while
//! sibling lanes are mid-flight:
//!
//! ```sh
//! rustc --edition 2021 --test -o /tmp/combat-test \
//!     crates/don-sim/src/systems/combat.rs && /tmp/combat-test
//! ```

#![allow(clippy::needless_range_loop)]

// ===========================================================================================
// 1. The rules.xml combat block
// ===========================================================================================

/// The combat constants, with `Constants` struct offsets, `rules.xml` text and stored value.
///
/// Offsets are the PDB's `Constants` field offsets (`schema/pdb-types.json`); stored values
/// are `docs/derivation/rules-constants.json`, which records the parser (`wtoi` = plain
/// `_wtoi`, `scaled` = `String::fraction(scale)` `0x00A1D110`) each binder in
/// `Constants::init` `0x00569A90` used.
///
/// The scale tells you the format: a `/256` field is 8.8 fixed point, a `/100` field is a
/// plain integer percent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CombatConstants {
    // ---- ranges (all `wtoi`, denominated in tiles) ----
    /// `+0x0018 UNIT_RESPOND_RANGE` — "distance units normally look for enemies".
    pub unit_respond_range: i32,
    /// `+0x001C UNIT_DEFENSIVE_RESPOND_RANGE`.
    pub unit_defensive_respond_range: i32,
    /// `+0x0020 UNIT_GUARD_RESPOND_RANGE`.
    pub unit_guard_respond_range: i32,
    /// `+0x0024 UNIT_BUILD_RESPOND_RANGE`.
    pub unit_build_respond_range: i32,
    /// `+0x0028 UNIT_GATHER_RESPOND_RANGE`.
    pub unit_gather_respond_range: i32,
    /// `+0x002C AIRCRAFT_RESPOND_RANGE`.
    pub aircraft_respond_range: i32,
    /// `+0x0030 BOMBER_RESPOND_RANGE`.
    pub bomber_respond_range: i32,
    /// `+0x0034 SHIP_DEFENSIVE_RESPOND_RANGE`.
    pub ship_defensive_respond_range: i32,

    /// `+0x0038 TARGET_RADIUS`, `String::fraction(192)` of "1/2 tile" = 96.
    pub target_radius: i32,
    /// `+0x0040 RANGE_INACCURACY`, `String::fraction(256)` of "1/100" = **2**.
    /// Read by `Ammo::init`, not by the damage chain — see [`RANGE_INACCURACY_NOTE`].
    pub range_inaccuracy: i32,

    // ---- the damage-chain modifiers (mirrored into crate::mechanics::CombatRules) ----
    /// `+0x0044 HEIGHT_INCREMENT` = 200. Denominator; `× 100` at `0x00644D70`.
    pub height_increment: i32,
    /// `+0x0048 HEIGHT_BONUS` = 10 (integer percent per increment).
    pub height_bonus: i32,
    /// `+0x004C FLANK_BONUS` = 50 (integer percent per flank level).
    pub flank_bonus: i32,
    /// `+0x0050 CAVALRY_FLANK_BONUS` = 40. **`wtoi`, not a fraction** — applied `/256` at
    /// `0x00644B4F`, so cavalry get `50*40/256 = 7%` per level, not 40% of 50%.
    pub cavalry_flank_bonus: i32,
    /// `+0x0054 VEHICLE_FLANK_BONUS` = 33, same `/256` treatment at `0x00644B42`.
    pub vehicle_flank_bonus: i32,
    /// `+0x0058 ROCKY_MODIFIER`, `fraction(256)` of "2/3" = 170.
    pub rocky_modifier: i32,
    /// `+0x005C OVERKILL_FRAMES` = **30** sim frames = 2 game seconds.
    pub overkill_frames: i32,
    /// `+0x0060 OVERKILL_DAMAGE`, `fraction(256)` of "1/3" = **85**. `85/256 = 0.3320…`.
    pub overkill_damage: i32,
    /// `+0x0064 ENTRENCHMENT_MODIFIER`, `fraction(256)` of "2/3" = 170.
    pub entrenchment_modifier: i32,
    /// `+0x0068 RIVER_MODIFIER`, `fraction(256)` of "2/1" = 512.
    pub river_modifier: i32,
    /// `+0x006C RECAPTURE_CITY_MODIFIER`, `fraction(256)` of "2/1" = 512.
    pub recapture_city_modifier: i32,

    // ---- late-chain constants whose names the PDB supplied ----
    /// `+0x04C4 RED_FORT_AIR_DEFENSE` = 33 ("33% less damage").
    pub red_fort_air_defense: i32,
    /// `+0x0558 SUPER_IMMUNE` = 0 (disabled in the shipped rules).
    pub super_immune: i32,
    /// `+0x076C RUSSIAN_COSSACK_DAMAGE` = 25.
    pub russian_cossack_damage: i32,
    /// `+0x0794 JAPANESE_DAMAGE` = -5 ("negative is per age").
    pub japanese_damage: i32,
    /// `+0x08B8 DUTCH_ATTACK_BONUS` = 1.
    pub dutch_attack_bonus: i32,
    /// `+0x0B98 ANTIPATER_ENTRENCH_BONUS`, `fraction(256)` of "80/100" = 204.
    pub antipater_entrench_bonus: i32,
    /// `+0x0BBC WELLINGTON_SIEGE_ATTACK` = 1.
    pub wellington_siege_attack: i32,
    /// `+0x0C20 ARTILLERY_UNDER_ATTACK_FIRES_SLOWLY` = 1. Read by `UnitData::recharge`.
    pub artillery_under_attack_fires_slowly: i32,

    /// `+0x03F4 COMMANDO_MIN_DAMAGE` = 200.
    pub commando_min_damage: i32,
    /// `+0x03F8 SPECIAL_MIN_DAMAGE` = 400.
    pub special_min_damage: i32,
    /// `+0x03FC ELITE_MIN_DAMAGE` = 800.
    pub elite_min_damage: i32,
}

/// `RANGE_INACCURACY` is **not** read by `ObjectData::get_damage`.
///
/// A whole-binary per-procedure disassembly scan for `[reg + 0x1F0]` (`ObjectType::attenuate`)
/// finds exactly **two** consumers outside constructors, loaders and loggers, and both are in
/// `Ammo::init`: `0x0067C4B5` (`imul ecx, [eax+0x1f0]`) and `0x0067C4F2`. `to_hit`
/// (`+0x1EC`) likewise has one consumer, `Ammo::init` `0x0067C47E`.
///
/// That closes `docs/derivation/combat.md` §9's open question ("`to_hit` and `attenuate`
/// are never read … they most likely belong to the projectile code — **Unresolved**").
/// They do belong to the projectile code, and the projectile lane owns them. Accuracy is a
/// *hit/miss* decision made in `Ammo`, never a damage-magnitude term.
pub const RANGE_INACCURACY_NOTE: &str =
    "ATTENUATE/TO_HIT are read only by Ammo::init (0x0067C47E/0x0067C4B5/0x0067C4F2); \
     they are hit-resolution inputs, not damage-magnitude terms";

impl CombatConstants {
    /// The values in the shipped `ron-data/rules.xml`, as `Constants::init` stores them.
    pub const fn shipped() -> CombatConstants {
        CombatConstants {
            unit_respond_range: 12,
            unit_defensive_respond_range: 4,
            unit_guard_respond_range: 8,
            unit_build_respond_range: 12,
            unit_gather_respond_range: 32,
            aircraft_respond_range: 10,
            bomber_respond_range: 12,
            ship_defensive_respond_range: 8,

            target_radius: 96,
            range_inaccuracy: 2,

            height_increment: 200,
            height_bonus: 10,
            flank_bonus: 50,
            cavalry_flank_bonus: 40,
            vehicle_flank_bonus: 33,
            rocky_modifier: 170,
            overkill_frames: 30,
            overkill_damage: 85,
            entrenchment_modifier: 170,
            river_modifier: 512,
            recapture_city_modifier: 512,

            red_fort_air_defense: 33,
            super_immune: 0,
            russian_cossack_damage: 25,
            japanese_damage: -5,
            dutch_attack_bonus: 1,
            antipater_entrench_bonus: 204,
            wellington_siege_attack: 1,
            artillery_under_attack_fires_slowly: 1,

            commando_min_damage: 200,
            special_min_damage: 400,
            elite_min_damage: 800,
        }
    }
}

/// World units per tile in every *range* comparison in the combat code.
///
/// The idiom is `lea eax,[eax+eax*2]; shl eax,6` — `× 3 × 64 = × 192` — and it appears at
/// `0x0064C3F9` and `0x0064C41C` (splash range), `0x006784F6` (`Ammo::do_damage` splash
/// area), `0x0064A...`/`0x0064C880` (`attack_dist` comparisons) and inside
/// `Object::poor_target` `0x0064A270`. It matches `rules.xml`'s `1/192 tile` granularity
/// (`UNIT_MOVE_SPEED`, `TARGET_RADIUS` scale 192).
pub const RANGE_UNITS_PER_TILE: i32 = 192;

/// `2 × [`RANGE_UNITS_PER_TILE`]`, the floor on the splash *range* radius at `0x0064C3FF`.
pub const SPLASH_MIN_RANGE_RADIUS: i32 = 0x180;

// ===========================================================================================
// 2. Geometry — the engine's integer hypot and the circle scan order
// ===========================================================================================

/// `vector_dist(int,int)` — `0x0046CFF0`, `__fastcall(ecx=dx, edx=dy)`.
///
/// The engine's integer distance: `max + min² / (2·max)`, a first-order Taylor expansion of
/// `hypot`. The inner division is **unsigned** (`div`, `0x0046D02D`), and once the smaller
/// leg reaches `0xEA60 = 60000` it degrades to `(min + 2·max) >> 1` so `min²` cannot
/// overflow `i32`.
///
/// Both legs are absolute-valued first (`cdq; xor; sub`), so sign never reaches the result.
/// `vector_dist(0,0) == 0`.
///
/// This is a **32-bit truncating octagon metric, not Euclid**: `vector_dist(3,4) == 5`, but
/// `vector_dist(1,1) == 1` — the eight diagonal neighbours sit at distance 1, which is why
/// [`circle_table`]'s ring 1 has nine cells rather than five.
///
/// (`crate::systems::groups_guys` carries an identical port for `Guy::last_speed`. Kept
/// separate here so this module builds standalone; they should be deduped once both lanes
/// land.)
#[inline]
pub fn vector_dist(dx: i32, dy: i32) -> i32 {
    let a = dx.wrapping_abs() as u32;
    let b = dy.wrapping_abs() as u32;
    let (big, small) = if a > b { (a, b) } else { (b, a) };
    if big == 0 {
        return 0;
    }
    if small >= 60_000 {
        return ((small.wrapping_add(big.wrapping_mul(2))) >> 1) as i32;
    }
    (small.wrapping_mul(small) / big.wrapping_mul(2)).wrapping_add(big) as i32
}

/// `vector_dist(Coord&,Coord&,Coord&,Coord&)` — `0x0046D060`, which absolute-differences
/// the two points and tail-jumps to [`vector_dist`].
#[inline]
pub fn vector_dist_between(ax: i32, ay: i32, bx: i32, by: i32) -> i32 {
    vector_dist(
        ax.wrapping_sub(bx).wrapping_abs(),
        ay.wrapping_sub(by).wrapping_abs(),
    )
}

/// Highest ring index `circle_init` fills. `0x0068183B`: `while (r < 0x41)`.
pub const CIRCLE_MAX_RING: usize = 0x40;
/// Entry cap. `0x0068189D`: `if (0x3248 < n) { … return; }`.
pub const CIRCLE_MAX_ENTRIES: usize = 0x3248;

/// The global tile-offset spiral: `circle_x[] @0x00CB7E90`, `circle_y[] @0x00CBB0E0`,
/// `ring_end[] @0x00CBE330`.
///
/// Built once by `circle_init` (`0x006817F0`, 295 bytes) and read by ~50 functions —
/// `Object::find_nearby_target` `0x00649359`, `Object::do_damage` `0x0064C163`,
/// `Ammo::do_damage` `0x00678560`, fog, map generation, `Build::check_capture`. It is the
/// engine's one canonical "scan tiles outward from here" order, so **reproducing it exactly
/// reproduces the iteration order of every one of those searches**, which is what a
/// lockstep-faithful port needs.
///
/// The construction, transcribed from `re/decomp-all/006817f0.c` and checked against the
/// disassembly:
///
/// ```text
/// n = 0
/// for r in 0 ..= 0x40:
///     for x in -r ..= r:
///         for y in -r ..= r:
///             if vector_dist(x, y) == r: emit (x, y); n += 1
///     ring_end[r] = n
/// ```
///
/// `ring_end` is **cumulative**, so ring `r` occupies `[ring_end[r-1], ring_end[r])`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CircleTable {
    /// Signed tile offsets, x. `0x00CB7E90`.
    pub x: Vec<i8>,
    /// Signed tile offsets, y. `0x00CBB0E0`.
    pub y: Vec<i8>,
    /// Cumulative entry count through ring `r`. `0x00CBE330`.
    pub ring_end: [i32; CIRCLE_MAX_RING + 1],
}

/// Build the spiral exactly as `circle_init` `0x006817F0` does.
pub fn circle_table() -> CircleTable {
    let mut x = Vec::new();
    let mut y = Vec::new();
    let mut ring_end = [0i32; CIRCLE_MAX_RING + 1];
    let mut n = 0usize;
    for r in 0..=CIRCLE_MAX_RING as i32 {
        for cx in -r..=r {
            for cy in -r..=r {
                if vector_dist(cx, cy) == r {
                    x.push(cx as i8);
                    y.push(cy as i8);
                    n += 1;
                    if n > CIRCLE_MAX_ENTRIES {
                        // 0x0068189D: the overflow path stamps every remaining ring with
                        // the current count and returns.
                        for k in r as usize..=CIRCLE_MAX_RING {
                            ring_end[k] = n as i32;
                        }
                        return CircleTable { x, y, ring_end };
                    }
                }
            }
        }
        ring_end[r as usize] = n as i32;
    }
    CircleTable { x, y, ring_end }
}

// ===========================================================================================
// 3. Type-level combat stats
// ===========================================================================================

/// The `ObjectTypeData`/`UnitTypeData` fields the combat loop reads, at their PDB offsets.
///
/// Names are the PDB's own (`schema/pdb-types.json`), which settled several fields this
/// project had only addresses for — notably `+0x0308` = **`uber_size`**, the divisor at
/// `ObjectData::get_damage` `0x006448B9` that `docs/derivation/combat.md` §9 lists as
/// "What `defenderType[+0x308]` is — **not named by any loader binding I could find**".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TypeCombatStats {
    /// `+0x01E4 obj_masks`.
    pub obj_masks: u32,
    /// `+0x01E8 attack`, stored **×10**.
    pub attack_x10: i32,
    /// `+0x01EC to_hit`. Read only by `Ammo::init` — see [`RANGE_INACCURACY_NOTE`].
    pub to_hit: i32,
    /// `+0x01F0 attenuate`. Read only by `Ammo::init`.
    pub attenuate: i32,
    /// `+0x01F4 recharge`, in sim frames.
    pub recharge: i32,
    /// `+0x01F8 min_range`, tiles.
    pub min_range: i32,
    /// `+0x01FC max_range`, tiles.
    pub max_range: i32,
    /// `+0x0200 splash_area`, tiles. Consumed by `Ammo::do_damage` `0x006784F0`, *not* by
    /// `Object::do_damage` — see [`SplashScan`].
    pub splash_area: i32,
    /// `+0x0204 splash_percent`. Its **only** consumer in the whole binary is
    /// `ObjectData::get_damage` `0x006448DF` (step 14).
    pub splash_percent: i32,
    /// `+0x0208 ammo_per_att` — rounds per attack. Divisor at `Object::do_damage`
    /// `0x0064A735` (unit branch) and `0x0064A7BD` (building branch).
    pub ammo_per_att: i32,
    /// `+0x0210 hits`, the max-hp source for `Object::update_hits` `0x00647010`.
    pub hits: i32,
    /// `+0x0214 armor`, display scale (not ×10).
    pub armor: i32,
    /// `+0x0218 domain`: 0 land, 1 sea, 2 air.
    pub domain: i32,
    /// `+0x0234 x_size`, tiles.
    pub x_size: i32,
    /// `+0x0238 y_size`, tiles.
    pub y_size: i32,
    /// `+0x0240 block_radius`.
    pub block_radius: i32,
    /// `+0x0308 uber_size` — how many sub-objects one "uber" unit is made of.
    pub uber_size: i32,
    /// `+0x030C crew_size`.
    pub crew_size: i32,
    /// `UnitTypeData::is_siege`, type vtable slot `+0x010C` (`0x00470460`).
    pub is_siege: bool,
    /// `UnitTypeData::blocks_while_dead`, type vtable slot `+0x0120` (`0x00470440`).
    pub blocks_while_dead: bool,
}

/// `Object::domain` values, inferred from the tests at `0x0064459B`, `0x00644F3E`,
/// `0x0064C39D` and `0x0064C3D2` — never from a shipped name.
pub const DOMAIN_LAND: i32 = 0;
/// See [`DOMAIN_LAND`].
pub const DOMAIN_SEA: i32 = 1;
/// See [`DOMAIN_LAND`].
pub const DOMAIN_AIR: i32 = 2;

/// `Balance::final_balance_table` lookup — `0x0064418E`, over the slice
/// `crate::balance::BalanceTable::raw()`.
///
/// `(i32)(i16) table[attacker_type_id * 493 + defender_type_id]`, a percentage. The
/// attacker is the **row**. Returns `None` outside the table so a bad type id surfaces
/// instead of silently reading a neighbour's row (retail does not check; we refuse to
/// invent a value).
#[inline]
pub fn balance_percent(table: &[i16], attacker_type_id: i32, defender_type_id: i32) -> Option<i32> {
    const DIM: i32 = 493;
    if !(0..DIM).contains(&attacker_type_id) || !(0..DIM).contains(&defender_type_id) {
        return None;
    }
    let idx = attacker_type_id * DIM + defender_type_id;
    table.get(idx as usize).map(|v| *v as i32)
}

// ===========================================================================================
// 4. The attack cycle and RECHARGE
// ===========================================================================================

/// Inputs to [`recharge_frames`], each named for the retail read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RechargeInput {
    /// `type[+0x1F4]` — the base `RECHARGE`, in sim frames.
    pub base_recharge: i32,
    /// `type->vtable[+0x10C]()` = `UnitTypeData::is_siege`. Non-siege returns early.
    pub is_siege: bool,
    /// `UnitData +0x6C unit_masks2 & 1`, tested at `0x0060FE1F`.
    pub unit_masks2_bit0: bool,
    /// `UnitData::in_supply(int*)` `0x00609EF0`, called at `0x0060FE2A`.
    pub in_supply: bool,
    /// `this->is(BOMBARD)` — `ObjectData::is(0x10B, 0)` at `0x0060FE4C`. `TypeIndex 0x10B`
    /// is `BOMBARD` (PDB enum).
    pub is_bombard: bool,
}

/// `UnitData::recharge(int*)` — `0x0060FDF0`, unit.cpp:1337, vtable slot `+0x134`.
///
/// The base is `ObjectData::recharge` (`0x00646C30`, 12 bytes: `return type[+0x1F4]`).
/// `UnitData` overrides it with the artillery penalty:
///
/// ```text
/// r = type.recharge
/// if (!type->is_siege())                                       return r;
/// if ((RULES.artillery_under_attack_fires_slowly == 0 || !(unit_masks2 & 1))
///     && in_supply(out))                                       return r;
/// return this->is(BOMBARD) ? r * 2 : (r * 3) / 2;
/// ```
///
/// So a siege unit that is **out of supply** — or, with the shipped rule value 1, one that
/// is under attack — reloads at 1.5× (2× for bombards). `rules.xml` spells the intent out:
/// *"1 (0 = fires normal speed, 1 = fires as if out of supply)"*.
///
/// The `× 3 / 2` is a signed truncating divide (`lea eax,[eax+eax*2]; cdq; sub eax,edx;
/// sar eax,1` at `0x0060FE68`).
#[inline]
pub fn recharge_frames(i: &RechargeInput, r: &CombatConstants) -> i32 {
    let base = i.base_recharge;
    if !i.is_siege {
        return base;
    }
    let rule_off_or_not_flagged = r.artillery_under_attack_fires_slowly == 0 || !i.unit_masks2_bit0;
    if rule_off_or_not_flagged && i.in_supply {
        return base;
    }
    if i.is_bombard {
        base.wrapping_mul(2)
    } else {
        base.wrapping_mul(3) / 2
    }
}

/// The per-unit attack clock.
///
/// `UnitData +0xAE recharging` is an **`unsigned char`**, and `Unit::fight` writes it with
/// a byte store at `0x005FF0A4` (`mov byte [esi+0xae], al`) from the full `int` that
/// `recharge()` returned — so **a recharge over 255 frames wraps**. That is retail
/// behaviour, not a port artefact, and this type reproduces it.
///
/// The cycle, from `Unit::fight` `0x005FD4D0`:
/// * `recharging != 0` gates almost every fire path (`0x005FD5xx`, `0x005FE6xx`,
///   `0x005FEFxx`, `0x005FF08x`).
/// * On a completed attack the unit calls `vt[0x134](&out)` and stores the low byte.
/// * `Unit::inc_time` decrements it; a unit with `recharging == 0` and a live target fires.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AttackCycle {
    /// `UnitData +0xAE recharging`.
    pub recharging: u8,
}

impl AttackCycle {
    /// True when the unit may fire this frame.
    #[inline]
    pub fn ready(&self) -> bool {
        self.recharging == 0
    }

    /// Start a new reload. Truncates to a byte exactly as `0x005FF0A4` does.
    #[inline]
    pub fn fire(&mut self, recharge: i32) {
        self.recharging = recharge as u8;
    }

    /// One frame of cooldown. Saturating at zero — the byte is only ever decremented while
    /// non-zero.
    #[inline]
    pub fn tick(&mut self) {
        self.recharging = self.recharging.saturating_sub(1);
    }
}

// ===========================================================================================
// 5. Range and target gating
// ===========================================================================================

/// `attack_dist <= max_range * 192` — the range test used throughout `unit.cpp` and
/// `object.cpp` (e.g. `0x005FF08x` in `Unit::fight`, `0x0064A2E5`/`0x0064A317` in
/// `poor_target`).
///
/// `attack_dist` is `ObjectData::attack_dist` (`0x006488F0`), which is [`vector_dist`]
/// between the two objects with the *target's* footprint (`block_radius + 0x18`, or
/// `x_size`/`y_size × 0x60` for buildings) already subtracted; it is therefore an
/// **edge-to-edge** distance, which is why a `max_range` of 0 still allows a melee hit.
#[inline]
pub fn in_attack_range(attack_dist: i32, max_range_tiles: i32) -> bool {
    attack_dist <= max_range_tiles.wrapping_mul(RANGE_UNITS_PER_TILE)
}

/// Below `min_range` a ranged attacker cannot engage. `ObjectData::min_range` `0x00646C20`
/// returns `type[+0x1F8]`; the comparison is in the same `× 192` space.
#[inline]
pub fn below_min_range(attack_dist: i32, min_range_tiles: i32) -> bool {
    min_range_tiles > 0 && attack_dist < min_range_tiles.wrapping_mul(RANGE_UNITS_PER_TILE)
}

/// Inputs to [`poor_target`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PoorTargetInput {
    /// `this->vt[0x18]()` and `target->vt[0x18]()` — both must be units.
    pub both_are_units: bool,
    /// `GuyData +0x9A guy_flags & 0x40` on the target's lead guy (`0x0064A2A4`).
    pub target_guy_flag_0x40: bool,
    /// `this->has_objmask(0x80000000)` at `0x0064A2F5`.
    pub attacker_has_objmask_high: bool,
    /// `target->vt[0x17C](x,y,0)` — the target's terrain-speed query at `0x0064A2C3`.
    pub target_speed_here: i32,
    /// The same query on the attacker, `0x0064A2D6`.
    pub attacker_speed_here: i32,
    /// `find_angle(target - self)` differenced against the target's facing, biased by
    /// `0x80000000` exactly as `0x0064A2E0` does. Feed [`crate::mechanics::flank_level`].
    pub flank_level_from_behind: u32,
    /// `ObjectData::attack_dist(o,who)` `0x0064C880`.
    pub attack_dist: i32,
    /// `attacker type[+0x2C8] role & 0x400` at `0x0064A2FE`.
    pub attacker_role_0x400: bool,
    /// `this->max_range()` in tiles.
    pub max_range_tiles: i32,
}

/// `Object::poor_target(o, who, ?)` — `0x0064A270`, object.cpp:5048, 523 bytes.
///
/// Answers *"is this a target I should not bother chasing?"*. Returns `true` for a poor
/// target. Structure from `re/decomp-all/0064a270.c`, constants from capstone.
///
/// Two arms:
/// * **`guy_flags & 0x40` set** (the target is already engaged/fleeing-marked): poor if the
///   attacker lacks `objmask 0x80000000`, or if it is simply out of range.
/// * **clear**: poor only if the target is *faster here* than we are **and** we are behind
///   it (`flank_level > 1`, i.e. the deepest rear tier) **and** it is beyond `192` units
///   (or beyond `max_range × 192` for `role & 0x400` units). That is the engine's
///   "don't chase something you cannot catch" rule, and note it reuses the *flank*
///   classifier — the same `0x0092CFE0` the damage chain uses — to decide "behind".
pub fn poor_target(i: &PoorTargetInput, flank_level: impl Fn(u32) -> u32) -> bool {
    if !i.both_are_units {
        return false;
    }
    if i.target_guy_flag_0x40 {
        if !i.attacker_has_objmask_high {
            return true;
        }
        return i.attack_dist > i.max_range_tiles.wrapping_mul(RANGE_UNITS_PER_TILE);
    }
    // 0x0064A2C3..0x0064A2E5: strictly faster, and we are in its rear arc.
    if i.attacker_speed_here >= i.target_speed_here {
        return false;
    }
    if i.flank_level_from_behind <= 0x2AAA_AAA9 {
        return false;
    }
    if flank_level(i.flank_level_from_behind) <= 1 {
        return false;
    }
    if i.attacker_role_0x400 {
        i.attack_dist > i.max_range_tiles.wrapping_mul(RANGE_UNITS_PER_TILE)
    } else {
        i.attack_dist > RANGE_UNITS_PER_TILE
    }
}

// ===========================================================================================
// 6. Overkill
// ===========================================================================================

/// The overkill timestamp, `UnitData +0x4C damage_frame`, plus the last-damager pair
/// `+0xA4 damage_o` / `+0xA9 damage_who`.
///
/// This is the whole mechanism `rules.xml` describes as *"unit hit twice or more w/in this
/// time period receives attenuated damage from second and subsequent hits. Does not apply
/// to damage from buildings."*
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OverkillState {
    /// `+0x4C damage_frame`. Zero means "never damaged".
    pub damage_frame: i32,
    /// `+0xA4 damage_o` — the *captain* index of the last attacker (`i16`).
    pub damage_o: i16,
    /// `+0xA9 damage_who` — the last attacker's player.
    pub damage_who: u8,
}

/// Whether `Object::do_damage` re-stamps the window — `0x0064A5EB..0x0064A641`.
///
/// ```text
/// last = def->damage_frame
/// if (last != 0 && (game.frame - last) < RULES.overkill_frames) goto skip;   ; 0x0064A605
/// def->damage_frame = game.frame;                                           ; 0x0064A617
/// def->damage_o     = attacker->get_captain();                              ; 0x0064A62E
/// def->damage_who   = attacker->who;                                        ; 0x0064A641
/// ```
///
/// The stamp is refreshed **only once the window has fully elapsed**, never on every hit.
/// That is what makes the third and later hits inside one window still attenuate: the
/// window is anchored to the *first* hit, not slid forward.
#[inline]
pub fn overkill_should_stamp(st: &OverkillState, frame: i32, r: &CombatConstants) -> bool {
    st.damage_frame == 0 || frame.wrapping_sub(st.damage_frame) >= r.overkill_frames
}

/// Apply the stamp. Call only when [`overkill_should_stamp`] is true.
#[inline]
pub fn overkill_stamp(st: &mut OverkillState, frame: i32, attacker_captain: i16, attacker_who: u8) {
    st.damage_frame = frame;
    st.damage_o = attacker_captain;
    st.damage_who = attacker_who;
}

/// The gate `Object::do_damage` puts in front of the whole retaliation + stamp block —
/// `0x0064A594..0x0064A5A3`.
///
/// Requires the defender to be a unit (`vt[0x18]`) and the caller's 8th argument to be
/// zero. The splash recursion at `0x0064C4B7` passes `1` there, so **splash victims never
/// stamp the overkill window and never retaliate**.
#[inline]
pub fn overkill_block_runs(defender_is_unit: bool, suppress_arg8: bool) -> bool {
    defender_is_unit && !suppress_arg8
}

/// `unit_masks & 0x10` doubles the damage before the stamp — `0x0064A5DD`.
#[inline]
pub fn double_on_unit_mask_0x10(damage: i32, defender_unit_masks: u32) -> i32 {
    if defender_unit_masks & 0x10 != 0 {
        damage.wrapping_add(damage)
    } else {
        damage
    }
}

/// The step-23 attenuation predicate inside `ObjectData::get_damage` — `0x00644B94`.
///
/// ```text
/// if (overkill_gate && attacker->max_range() && attacker->is_unit() && defender->is_unit()
///     && def.damage_frame != 0
///     && (frame - def.damage_frame) < RULES.overkill_frames
///     && attacker->get_captain() != def.damage_o)
///        D = RULES.overkill_damage * D / 256;          // 85/256 = x0.332
///        if (defender->is(CATAPULT) && !attacker_type->is_siege()) D /= 2;
/// ```
///
/// Three things this makes concrete that the folklore does not:
/// * the trigger is a **different attacker** (`get_captain() != damage_o`), so a single
///   unit sustaining fire on one target is never attenuated;
/// * `attacker->max_range() != 0` plus `is_unit()` is the *"does not apply to damage from
///   buildings"* clause — and it also excludes melee, whose `max_range` is 0;
/// * the constant is `85/256`, `0.33203125`, not exactly `1/3`.
#[inline]
pub fn overkill_attenuates(
    overkill_gate: bool,
    attacker_max_range: i32,
    attacker_is_unit: bool,
    defender_is_unit: bool,
    attacker_captain: i16,
    st: &OverkillState,
    frame: i32,
    r: &CombatConstants,
) -> bool {
    overkill_gate
        && attacker_max_range != 0
        && attacker_is_unit
        && defender_is_unit
        && st.damage_frame != 0
        && frame.wrapping_sub(st.damage_frame) < r.overkill_frames
        && attacker_captain as i32 != st.damage_o as i32
}

/// The attenuation itself — `0x00644C35`, then the conditional halve at `0x00644C83`.
#[inline]
pub fn overkill_apply(
    damage: i32,
    defender_is_catapult: bool,
    attacker_type_is_siege: bool,
    r: &CombatConstants,
) -> i32 {
    let mut d = r.overkill_damage.wrapping_mul(damage) / 256;
    if defender_is_catapult && !attacker_type_is_siege {
        d /= 2;
    }
    d
}

// ===========================================================================================
// 7. The applier's scaling: 8.8 multiplier, rounds, uber_size, sixteenths
// ===========================================================================================

/// A damage amount carried as whole points plus sixteenths, the way `Object::do_damage`
/// hands it to `Object::take_damage`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScaledDamage {
    /// The first argument to `take_damage`, pushed from `[ebp-0x24]` at `0x0064BA15`.
    pub whole: i32,
    /// The second argument, a `char`, pushed from `[ebp-0x40]` at `0x0064BA10`.
    pub sixteenths: i8,
}

/// `Object::do_damage`'s scaling of `get_damage`'s return, unit branch —
/// `0x0064A6F9..0x0064A78D`.
///
/// ```text
/// D = max(D * scale_8_8, 0x100)            ; imul; cmp 0x100; cmovle    0x0064A701
/// if (ammo_index >= 0) D /= type.ammo_per_att                          ; 0x0064A735
/// q  = D / type.uber_size                                              ; 0x0064A766
/// q  = trunc(q / 16)                                                   ; 0x0064A76E
/// frac  = q % 16                                                       ; 0x0064A773
/// whole = trunc(q / 16)                                                ; 0x0064A78A
/// ```
///
/// `scale_8_8` is `Object::do_damage`'s 6th argument, an 8.8 fixed-point multiplier:
/// `Unit::fight` passes `0x100` (= 1.0) for a normal hit (`0x005FE7B4`, `0x005FE9A0`), and
/// the splash recursion passes `scale/4` or `scale/8` (see [`splash_scale`]).
///
/// The clamp to `0x100` is a **floor on the scaled product, before any division** — it is
/// what stops a very small `D` from vanishing entirely once the two `/16` steps run.
///
/// The final `/16, /16` pair means the applier works in 1/256ths internally and reports
/// 1/16ths; combined with the `× 0x100` that is exactly `D / uber_size`, with the leftover
/// sixteenths preserved rather than dropped.
pub fn scale_damage_unit(
    damage: i32,
    scale_8_8: i32,
    ammo_index: i32,
    ammo_per_att: i32,
    uber_size: i32,
) -> Result<ScaledDamage, DivideError> {
    let mut d = damage.wrapping_mul(scale_8_8);
    if d <= 0x100 {
        d = 0x100; // 0x0064A707 cmovle
    }
    if ammo_index >= 0 {
        if ammo_per_att == 0 || (d == i32::MIN && ammo_per_att == -1) {
            return Err(DivideError { site: "0x0064A735" });
        }
        d /= ammo_per_att;
    }
    if uber_size == 0 || (d == i32::MIN && uber_size == -1) {
        return Err(DivideError { site: "0x0064A766" });
    }
    let q = d / uber_size;
    let q16 = (q + ((q >> 31) & 15)) >> 4; // trunc(q/16), 0x0064A76E
    let frac = q16 % 16; // 0x0064A773 sign-corrected AND
    let whole = (q16 + ((q16 >> 31) & 15)) >> 4; // 0x0064A78A
    Ok(ScaledDamage {
        whole,
        sixteenths: frac as i8,
    })
}

/// The same scaling, building branch — `0x0064A792..0x0064A7F1`.
///
/// Identical except there is **no `uber_size` divide and no `0x100` floor**: it is
/// `D * scale / ammo_per_att`, then the two `/16` steps. Reached when the attacker is not
/// a unit but `vt[0x20]()` holds.
pub fn scale_damage_build(
    damage: i32,
    scale_8_8: i32,
    ammo_per_att: i32,
) -> Result<ScaledDamage, DivideError> {
    let d = damage.wrapping_mul(scale_8_8);
    if ammo_per_att == 0 || (d == i32::MIN && ammo_per_att == -1) {
        return Err(DivideError { site: "0x0064A7CB" });
    }
    let q = d / ammo_per_att;
    let q16 = (q + ((q >> 31) & 15)) >> 4;
    let frac = q16 % 16;
    let whole = (q16 + ((q16 >> 31) & 15)) >> 4;
    Ok(ScaledDamage {
        whole,
        sixteenths: frac as i8,
    })
}

/// A site where retail's `idiv` would raise `#DE`.
///
/// The engine checks none of these; it relies on shipped data never producing them.
/// Returning an error rather than inventing a value keeps the divergence visible — the same
/// choice [`crate::mechanics`] makes with its panics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DivideError {
    /// The VA of the `idiv`.
    pub site: &'static str,
}

impl std::fmt::Display for DivideError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "retail raises #DE at {}", self.site)
    }
}
impl std::error::Error for DivideError {}

// ===========================================================================================
// 8. Splash
// ===========================================================================================

/// The splash pass at the tail of `Object::do_damage`, `0x0064C10C..0x0064C4DD`.
///
/// **The scan is a fixed 3×3, not `SPLASH_AREA`.** The loop bound is the absolute
/// `[0x00CBE334]` at `0x0064C14B` and `0x0064C4D7`, which is `ring_end[1]` of the global
/// circle table — nine entries, because [`vector_dist`] puts the four diagonals at
/// distance 1 as well as the four orthogonals. `SPLASH_AREA` (`type[+0x200]`) is read by
/// `Ammo::do_damage` at `0x006784F0`, where it sizes *that* function's ring
/// (`ring = clamp(splash_area + 1, 0, 64)`, `0x0067851B`); the melee/direct applier does
/// not consult it at all.
pub const SPLASH_RING: usize = 1;

/// The candidate filter inside the splash scan, in retail order.
///
/// Sources, in address order: `Search::valid_search(3, who)` `0x0064C228`; `vt[0x8]()`
/// `0x0064C24C`; `vt[0xBC]()` = `is_on_map` `0x0064C25C`; `Search::valid_filter(…, 9)`
/// `0x0064C275`; `Search::valid_filter(…, 0xA)` `0x0064C28F`; the tribe-bonus veto
/// `0x0064C2AA`; then the two geometric tests below.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SplashCandidate {
    /// All of the `Search`/`is_on_map` gates passed.
    pub passes_search_filters: bool,
    /// `LeaderData::has_tribe_bonus(0x10)` on the *candidate's* owner **and**
    /// `RULES[+0x7DC] != 0` — a tribe that is immune to friendly splash. `0x0064C2B8`.
    pub owner_has_splash_immunity: bool,
    /// `vector_dist` from the **primary target** to this candidate, `0x0064C321`.
    pub dist_to_primary_target: i32,
    /// `vector_dist` from the attacker to this candidate, the 4-`Coord` form at
    /// `0x0064C48A`.
    pub dist_to_attacker: i32,
}

/// The two radii the splash scan compares against.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SplashRadii {
    /// `max(primary_target.x_size, primary_target.y_size) * 192` — `0x0064C340..0x0064C38E`.
    /// The splash "blast footprint" is sized by the **primary target**, not by the weapon.
    pub target_footprint: i32,
    /// `max(attacker->max_range() * 192, 0x180)` — `0x0064C3F3..0x0064C41F`. Only consulted
    /// on the non-siege arm.
    pub attacker_reach: i32,
}

/// Compute [`SplashRadii`].
#[inline]
pub fn splash_radii(
    primary_target_x_size: i32,
    primary_target_y_size: i32,
    attacker_max_range_tiles: i32,
) -> SplashRadii {
    let bigger = primary_target_x_size.max(primary_target_y_size);
    let reach = attacker_max_range_tiles.wrapping_mul(RANGE_UNITS_PER_TILE);
    SplashRadii {
        target_footprint: bigger.wrapping_mul(RANGE_UNITS_PER_TILE),
        attacker_reach: if reach < SPLASH_MIN_RANGE_RADIUS {
            SPLASH_MIN_RANGE_RADIUS
        } else {
            reach
        },
    }
}

/// The 8.8 multiplier handed to the recursive `Object::do_damage` for one splash victim.
///
/// * Land siege: `scale / 4` — `0x0064C3DB..0x0064C3E7`, taken when the attacker type
///   `is_siege()` **and** its domain is [`DOMAIN_LAND`]. This arm skips the
///   attacker-reach test entirely (`jmp 0x0064C4A3`).
/// * Everything else: `scale / 8` — `0x0064C497..0x0064C4A0`, after the reach test.
///
/// Both are truncating signed divides written as `cdq; and edx,N; add; sar`.
#[inline]
pub fn splash_scale(scale_8_8: i32, attacker_is_siege: bool, attacker_domain: i32) -> i32 {
    if attacker_is_siege && attacker_domain == DOMAIN_LAND {
        (scale_8_8 + ((scale_8_8 >> 31) & 3)) >> 2
    } else {
        (scale_8_8 + ((scale_8_8 >> 31) & 7)) >> 3
    }
}

/// Whole-attacker gates on the splash pass — `0x0064C39A..0x0064C3BA`.
///
/// * `attacker.domain == AIR` → no splash at all.
/// * `attacker.domain == SEA` **and** `ObjectData::is_siege()` → no splash.
#[inline]
pub fn splash_enabled(attacker_domain: i32, attacker_is_siege: bool) -> bool {
    if attacker_domain == DOMAIN_AIR {
        return false;
    }
    !(attacker_domain == DOMAIN_SEA && attacker_is_siege)
}

/// Does this candidate take splash?
///
/// Reproduces the accept path at `0x0064C2C5..0x0064C4B7`. `attacker_is_siege_land` selects
/// the arm that skips the reach test, matching `0x0064C3CD`'s branch.
pub fn splash_hits(c: &SplashCandidate, radii: &SplashRadii, attacker_is_siege_land: bool) -> bool {
    if !c.passes_search_filters || c.owner_has_splash_immunity {
        return false;
    }
    if c.dist_to_primary_target > radii.target_footprint {
        return false; // 0x0064C391
    }
    if attacker_is_siege_land {
        return true; // 0x0064C3EA jumps straight to the recursive call
    }
    c.dist_to_attacker <= radii.attacker_reach // 0x0064C492
}

/// A full splash pass: walk the nine tiles of ring 1 in circle order and yield the
/// `(who, o)` pairs that take damage, in the order retail visits them.
///
/// Order is load-bearing for lockstep: each victim's `do_damage` can kill, eject cargo and
/// consume RNG, so visiting them in a different order diverges. Retail's order is
/// `circle[0..9]` (tiles), and within a tile the object list threaded through
/// `ObjectData +0x2C down` / `+0x2E down_who` (`0x0064C1B4`, `0x0064C4BC`).
pub struct SplashScan<'a> {
    circle: &'a CircleTable,
    /// Tile of the primary target.
    pub origin_tile: (i32, i32),
}

impl<'a> SplashScan<'a> {
    /// Bind a scan to the shared circle table.
    pub fn new(circle: &'a CircleTable, origin_tile: (i32, i32)) -> Self {
        SplashScan {
            circle,
            origin_tile,
        }
    }

    /// The tiles retail visits, in retail order. Nine entries with the shipped table.
    pub fn tiles(&self) -> Vec<(i32, i32)> {
        let n = self.circle.ring_end[SPLASH_RING] as usize;
        (0..n)
            .map(|i| {
                (
                    self.origin_tile.0 + self.circle.x[i] as i32,
                    self.origin_tile.1 + self.circle.y[i] as i32,
                )
            })
            .collect()
    }
}

// ===========================================================================================
// 9. Flanking, height, entrenchment — computing the chain's *inputs*
// ===========================================================================================

/// The mask gate in front of the flank term — `0x00644A9B..0x00644B0C`.
///
/// Every one of these must hold before the angle is even formed. `am`/`dm` are the
/// attacker's and defender's `ObjectTypeData +0x1E4 obj_masks`.
#[inline]
pub fn flank_gate(attacker_is_unit: bool, defender_is_unit: bool, am: u32, dm: u32) -> bool {
    attacker_is_unit
        && defender_is_unit
        && am & 4 == 0
        && dm & 4 == 0
        && am & 0x1000_0000 == 0
        && dm & 0x1000_0000 == 0
        && am & 0x2000 == dm & 0x2000
        && dm & 0x2000 == 0
}

/// The angle delta the flank classifier consumes — `0x00644B12`.
///
/// `(defender_facing - attack_dir) - 0x80000000`, all in `u32`. The `-0x80000000` is its
/// own inverse, so it is a half-turn rotation.
///
/// `attack_dir` is `Object::do_damage`'s third argument (declared `unsigned long`); its
/// producers are `Unit::fight` (via `find_angle` `0x0092D130`, ported at
/// `crate::trig::find_angle`) and `Ammo::do_damage`.
///
/// **The arcs, exactly** — combining `crate::mechanics::flank_level` (`0x0092CFE0`) with
/// the caller's own `delta >= 0x2AAAAAAA` pre-guard at `0x00644B1D`:
///
/// | biased `delta` | width | flank level |
/// |---|---|---|
/// | `[0xD5555556, 0x2AAAAAA9]` wrapping through 0 | 120° | **none** (skipped, or tier 0) |
/// | `[0x2AAAAAAA, 0x5FFFFFFF]` | 75° | **2** |
/// | `[0x60000000, 0xA0000000]` | 90° | **1** |
/// | `[0xA0000001, 0xD5555555]` | 75° | **2** |
///
/// Which of those arcs is the defender's *front* is **not established** here: it depends on
/// `attack_dir`'s sign convention inside `Unit::fight`, which this lane did not derive. The
/// arithmetic above is measured; any statement of the form "being shot in the back gives
/// tier 2" would be folklore until that convention is pinned down. Recorded as an open
/// question rather than guessed.
#[doc(alias = "flank")]
#[inline]
pub fn flank_delta(defender_facing: i32, attack_dir: i32) -> u32 {
    (defender_facing as u32)
        .wrapping_sub(attack_dir as u32)
        .wrapping_sub(0x8000_0000)
}

/// The flank multiplier percent, given a tier from [`crate::mechanics::flank_level`] —
/// `0x00644B36..0x00644B7B`.
///
/// ```text
/// pct = RULES.flank_bonus                                  ; 50
/// if      (dm & 0x00200000) pct = RULES.vehicle_flank_bonus * pct / 256    ; 33*50/256 = 6
/// else if (dm & 0x00001000) pct = RULES.cavalry_flank_bonus * pct / 256    ; 40*50/256 = 7
/// D = (pct * level + 100) * D / 100
/// ```
///
/// With the shipped values: infantry take `+50%` at tier 1 and `+100%` at tier 2; cavalry
/// `+7%`/`+14%`; vehicles `+6%`/`+12%`. The `rules.xml` comment *"max bonus is twice this
/// number"* is exactly the tier-2 case.
///
/// Note the two sub-multipliers are `wtoi` integers applied with a `/256`, **not**
/// `fraction(256)` values — so "40%" and "33%" behave as `40/256` and `33/256`, not as
/// `0.40` and `0.33`. Anyone reading the XML alone gets this wrong by 6×.
#[inline]
pub fn flank_percent(level: i32, defender_masks: u32, r: &CombatConstants) -> i32 {
    let mut pct = r.flank_bonus;
    if defender_masks & 0x0020_0000 != 0 {
        pct = r.vehicle_flank_bonus.wrapping_mul(pct) / 256;
    } else if defender_masks & 0x1000 != 0 {
        pct = r.cavalry_flank_bonus.wrapping_mul(pct) / 256;
    }
    pct.wrapping_mul(level).wrapping_add(100)
}

/// The height term — `0x00644CF5..0x00644D7D`.
///
/// Fires only when neither party is airborne, the attacker's type is **not** siege
/// (`type->vt[0x10C]`), and the attacker is strictly above the defender. `z` is the
/// XOR-unmasked `SubObjectData +0x0C z_internal` (`^ 0x00063637`).
///
/// `D += (dz * HEIGHT_BONUS * D) / (HEIGHT_INCREMENT * 100)`, i.e. +10% per 200 z.
/// Returns `Err` where retail's `idiv` at `0x00644D78` would fault.
#[inline]
pub fn height_bonus_add(
    damage: i32,
    attacker_z: i32,
    defender_z: i32,
    r: &CombatConstants,
) -> Result<i32, DivideError> {
    let dz = attacker_z.wrapping_sub(defender_z);
    let num = dz.wrapping_mul(r.height_bonus).wrapping_mul(damage);
    let den = r.height_increment.wrapping_mul(100);
    if den == 0 || (num == i32::MIN && den == -1) {
        return Err(DivideError { site: "0x00644D78" });
    }
    Ok(num / den)
}

/// The height term's guard — `0x00644CF5`, `0x00644D50`, `0x00644D5A`.
#[inline]
pub fn height_applies(
    attacker_domain: i32,
    defender_domain: i32,
    attacker_type_is_siege: bool,
    attacker_z: i32,
    defender_z: i32,
) -> bool {
    defender_domain != DOMAIN_AIR
        && attacker_domain != DOMAIN_AIR
        && !attacker_type_is_siege
        && attacker_z > defender_z
}

/// XOR mask on every `Coord` inside an `Object` — `0x0064A662`, `0x0064C121`, and ~200
/// other sites.
///
/// `SubObjectData`'s `z_internal`/`x_internal`/`y_internal` are stored XOR'd with this;
/// `GuyData`'s `x`/`y`/`z` are **not**. Unmask before any arithmetic.
pub const COORD_XOR: i32 = 0x0006_3637;

/// Unmask an obfuscated object coordinate.
#[inline]
pub fn coord_unmask(raw: i32) -> i32 {
    raw ^ COORD_XOR
}

/// The entrenchment term's guard — `0x00644D7F..0x00644E44`.
///
/// Requires the defender to be a unit with `unit_masks & 0x02000000` (entrenched) and the
/// attacker **not** to be a `FLAMETHROWER` (`TypeIndex 0x83`, tested at `0x00644DE8`) — the
/// engine's own "flame ignores trenches" rule, previously recorded only as
/// `attacker_tech_0x83`.
///
/// The direction test uses [`crate::mechanics::entrench_dir_level`], a *different*
/// classifier from the flank one, against `trench_angle` (`UnitData +0x5C`) rather than
/// `angle` (`+0x50`). The modifier applies when the hit is splash **or** the direction tier
/// is 0 — i.e. entrenchment protects only against fire from the arc it faces.
#[inline]
pub fn entrench_applies(
    defender_is_unit: bool,
    defender_unit_masks: u32,
    attacker_is_flamethrower: bool,
) -> bool {
    defender_is_unit && defender_unit_masks & 0x0200_0000 != 0 && !attacker_is_flamethrower
}

/// The entrenchment direction delta — `0x00644E0E`, same `-0x80000000` bias as
/// [`flank_delta`] but against `trench_angle`.
#[inline]
pub fn entrench_delta(defender_trench_angle: i32, attack_dir: i32) -> u32 {
    (defender_trench_angle as u32)
        .wrapping_sub(attack_dir as u32)
        .wrapping_sub(0x8000_0000)
}

// ===========================================================================================
// 10. Hit points, death, and the corpse
// ===========================================================================================

/// The hit-point state a combat lane owns, at its `ObjectData` offsets.
///
/// All three are inside `Object::walk_data`'s `[32, 66)` byte range, so all three are in the
/// **units** checksum channel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HitPoints {
    /// `+0x20 myhits` — the **maximum**, refreshed by `Object::update_hits` `0x00647010`
    /// from `objecttypes[t].hits` (`+0x210`).
    pub myhits: i32,
    /// `+0x24 damage` — cumulative damage taken. Health is `myhits - damage`.
    pub damage: i32,
    /// `+0x3B damage_frac` — the carried sixteenths, a signed `char`.
    pub damage_frac: i8,
}

impl HitPoints {
    /// `ObjectData::hits_left()` — `0x006535C0`, object.cpp:1092.
    ///
    /// `min(myhits, myhits - damage)`, and `0` if either is negative. Not a plain
    /// subtraction: a negative `myhits` reports 0 health rather than a negative one.
    #[inline]
    pub fn hits_left(&self) -> i32 {
        let max = self.myhits;
        let left = self.myhits.wrapping_sub(self.damage);
        if left >= 0 && max >= 0 {
            left.min(max)
        } else {
            0
        }
    }

    /// The accumulate step of `Object::take_damage` — entry guard `0x00652042`, then
    /// `0x006522F2..0x00652318`.
    ///
    /// ```text
    /// if (damage < 1 && frac < 1) frac = 1;        ; every hit lands for at least 1/16
    /// u        = (char)(this->damage_frac + frac); ; wraps in a signed byte
    /// damage  += trunc(u / 16);
    /// this->damage_frac = u % 16;
    /// this->damage     += damage;
    /// ```
    ///
    /// The minimum is **1/16 of a hit point, not 1** — a distinct rule from
    /// `get_damage`'s conditional floor of 1 at step 28, and it applies unconditionally.
    /// Returns the whole-point amount actually added.
    pub fn accumulate(&mut self, damage: i32, frac: i8) -> i32 {
        let mut frac = frac;
        let mut damage = damage;
        if damage < 1 && frac < 1 {
            frac = 1; // 0x00652042
        }
        let u = self.damage_frac.wrapping_add(frac) as i32; // (char)(a+b), sign-extended
        damage = damage.wrapping_add((u + ((u >> 31) & 15)) >> 4);
        self.damage_frac = (u % 16) as i8;
        self.damage = self.damage.wrapping_add(damage);
        damage
    }
}

/// Which of the two `uber_size` divides `Object::take_damage` takes — `0x006528C9`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UberBranch {
    /// `local_c /= uber_size` — `0x0065293E`. The common arm.
    Even,
    /// `local_c -= ((uber_size - 1) * local_c) / uber_size` — the arm taken when
    /// `this->vt[0xE8]()` is non-zero **and** `0x0060A760(1)` returns exactly 1. Differs
    /// from [`UberBranch::Even`] only in rounding, and only when `max_hits` is not a
    /// multiple of `uber_size`.
    Remainder,
}

/// Per-sub-object maximum hit points — `Object::take_damage` `0x006528B0..0x00652960`.
///
/// **`uber_size` is emphatically live**, contrary to a sibling lane's reading. A
/// per-procedure disassembly of the whole `.text` for `[reg + 0x308]` finds 28 sim-side
/// sites, including a dedicated accessor `UnitData::uber_size` (`0x0060A803`), four
/// `idiv`s — `ObjectData::get_damage` `0x006448B9`, `Unit::same_damage` `0x005F949D`,
/// `Unit::repair_damage` `0x0060E19E`, `Object::take_damage` `0x0065293E` — plus
/// `Object::do_damage` `0x0064A75C`, `UnitData::total_damage` `0x00609CD7`, and six
/// `cmp …, 1` guards in `Unit::come_out` / `Unit::go_inside` / `Unit::suffer_attrition` /
/// `Object::eject_contents`.
///
/// What it is **not** is `guy_mark`. An "uber" unit is several *`Object` slots* sharing a
/// captain, not several `Guy`s inside one slot; `Object::take_damage` divides max-hp by
/// `uber_size` and cascades the overflow to `get_captain()`, never to a `Guy`. Both
/// readings can be true at once, and the field-offset evidence says they are.
///
/// Fidelity: structure from `re/decomp-all/00652020.c`, divide sites confirmed in capstone.
/// Never executed.
#[inline]
pub fn effective_max_hits(max_hits: i32, uber_size: i32, branch: UberBranch) -> i32 {
    if uber_size <= 1 {
        return max_hits;
    }
    match branch {
        UberBranch::Even => max_hits / uber_size,
        UberBranch::Remainder => {
            max_hits.wrapping_sub(uber_size.wrapping_sub(1).wrapping_mul(max_hits) / uber_size)
        }
    }
}

/// What one `Object::take_damage` call resolved to. Mirrors the function's three return
/// values (`0`, `1`, `2`) at `0x00652xxx`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageOutcome {
    /// Return 0 — survived.
    Survived,
    /// Return 1 — the disband path (`Object::disband` `0x006455C0`), reached when a
    /// heavily damaged unit of a non-eliminated player carries the `0x2000` graphic flag.
    Disbanded,
    /// Return 2 — died. `overflow` is `damage - effective_max_hits` and is forwarded to the
    /// captain with a nested `take_damage(overflow, 0, …)` at `0x006533E6`, which is how a
    /// multi-slot uber unit loses one sub-object per lethal hit rather than all of them.
    Died { overflow: i32 },
}

/// The life/death decision — `0x006522F2` … `0x006533B2`.
///
/// `if (this->damage < effective_max_hits) survive else die`. Note the comparison is
/// against **accumulated damage**, not against remaining hit points, and it is `<` — so an
/// object whose damage exactly equals its effective maximum dies.
pub fn resolve_damage(hp: &HitPoints, effective_max: i32) -> DamageOutcome {
    if hp.damage < effective_max {
        DamageOutcome::Survived
    } else {
        DamageOutcome::Died {
            overflow: hp.damage.wrapping_sub(effective_max),
        }
    }
}

/// How long a dead object's slot is held before it is reused — `Object::die` `0x00647080`,
/// object.cpp:2936.
///
/// ```text
/// hold = 1
/// if (type.max_range != 0)
///     for each live Ammo a with a.o == this->o and a.who == this->who:
///         hold = max(hold, (a.total_time - a.cur_time) + 1 + GLOBAL_00C0A888)
/// this->hold_frames = max(hold, this->hold_frames)          ; +0x32, u16
/// ```
///
/// So the corpse's *slot* survives until every projectile already in the air toward it has
/// landed. Any port that frees the slot at death will resolve in-flight ammo against the
/// wrong object. `hold_frames` is `ObjectData +0x32`, inside `Object::walk_data`'s
/// `[32,66)` range, so it is checksummed.
#[inline]
pub fn hold_frames_after_death(
    current_hold: u16,
    type_has_range: bool,
    inbound_ammo_remaining: &[i32],
    global_pad: i32,
) -> u16 {
    let mut hold: i32 = 1;
    if type_has_range {
        for &remaining in inbound_ammo_remaining {
            let candidate = remaining.wrapping_add(1).wrapping_add(global_pad);
            if candidate > hold {
                hold = candidate;
            }
        }
    }
    if (current_hold as i32) > hold {
        current_hold
    } else {
        hold as u16
    }
}

// ===========================================================================================
// 11. The deaths channel
// ===========================================================================================

/// `sizeof(DeathObj)` — `DeathObjData(76) + DeathObjOut` = **164** bytes, the array stride
/// at `Objects::add_death` `0x00653B60`.
pub const DEATH_OBJ_STRIDE: usize = 164;
/// `sizeof(DeathObjData)`.
pub const DEATH_OBJ_DATA_SIZE: usize = 76;
/// Bytes `DeathObjData::walk_data` actually hashes: `[0,4)` then `[4,0x4B)`.
pub const DEATH_WALKED_BYTES: usize = 75;

/// One entry of `ObjectsData::death_objs` (`Objects +0x13C`, an `ObjectArray<DeathObj>`;
/// count at `+0x140`, base pointer at `+0x14C`).
///
/// Fields and offsets are the PDB's `DeathObjData`. Only `[0, 0x4B)` is walked, so
/// `DeathObjOut`'s `bleed` / `anim_done` / `center_*` and the trailing pad byte at `+0x4B`
/// are **not** in the checksum — the corpse's render state is presentation, its identity
/// and timing are simulation.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DeathRecord {
    /// `+0x00 valid`. Zero means the slot is free; `DeathObjData::walk_data` hashes only
    /// this field when it is zero.
    pub valid: i32,
    /// `+0x04 first_frame` — the frame the corpse appeared. The ordering key in
    /// [`DeathRing::add_death`].
    pub first_frame: i32,
    /// `+0x08 cur_anim`.
    pub cur_anim: i32,
    /// `+0x0C x` (Coord, already unmasked in the record).
    pub x: i32,
    /// `+0x10 y`.
    pub y: i32,
    /// `+0x14 z`.
    pub z: i32,
    /// `+0x18 who` — the dead object's owner.
    pub who: i32,
    /// `+0x1C o` — the dead object's slot index.
    pub o: i32,
    /// `+0x20 gpiece`.
    pub gpiece: i32,
    /// `+0x24 ammo_gpiece`.
    pub ammo_gpiece: i32,
    /// `+0x28 ammo_angle` — **the one `float` in the walked range**, and therefore the one
    /// float in the deaths checksum. Hashed as raw IEEE binary32 bytes.
    pub ammo_angle: f32,
    /// `+0x2C new_angle`.
    pub new_angle: i32,
    /// `+0x30 turret_angles[4]`.
    pub turret_angles: [i32; 4],
    /// `+0x40 cur_frame` — the animation clock advanced by `DeathObj::inc_time`
    /// `0x008D5240`.
    pub cur_frame: i32,
    /// `+0x44 skel_gpiece`.
    pub skel_gpiece: i32,
    /// `+0x48 node_flags`.
    pub node_flags: i16,
    /// `+0x4A unit_crew` — the last walked byte.
    pub unit_crew: i8,
}

impl DeathRecord {
    /// Serialise the walked range `[0, 0x4B)` little-endian, exactly as `CheckSum` sees it.
    pub fn walked_bytes(&self) -> [u8; DEATH_WALKED_BYTES] {
        let mut b = [0u8; DEATH_WALKED_BYTES];
        let mut put = |off: usize, v: &[u8]| b[off..off + v.len()].copy_from_slice(v);
        put(0x00, &self.valid.to_le_bytes());
        put(0x04, &self.first_frame.to_le_bytes());
        put(0x08, &self.cur_anim.to_le_bytes());
        put(0x0C, &self.x.to_le_bytes());
        put(0x10, &self.y.to_le_bytes());
        put(0x14, &self.z.to_le_bytes());
        put(0x18, &self.who.to_le_bytes());
        put(0x1C, &self.o.to_le_bytes());
        put(0x20, &self.gpiece.to_le_bytes());
        put(0x24, &self.ammo_gpiece.to_le_bytes());
        put(0x28, &self.ammo_angle.to_bits().to_le_bytes());
        put(0x2C, &self.new_angle.to_le_bytes());
        for k in 0..4 {
            put(0x30 + 4 * k, &self.turret_angles[k].to_le_bytes());
        }
        put(0x40, &self.cur_frame.to_le_bytes());
        put(0x44, &self.skel_gpiece.to_le_bytes());
        put(0x48, &self.node_flags.to_le_bytes());
        put(0x4A, &self.unit_crew.to_le_bytes());
        b
    }
}

/// `ObjectsData::death_objs` — the corpse ring.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeathRing {
    /// The slots. `len()` is `Objects +0x140`.
    pub slots: Vec<DeathRecord>,
}

impl DeathRing {
    /// A ring of `n` free slots.
    pub fn with_capacity(n: usize) -> DeathRing {
        DeathRing {
            slots: vec![DeathRecord::default(); n],
        }
    }

    /// Slot selection in `Objects::add_death` — `0x00653B60`, objects.cpp:5119.
    ///
    /// ```text
    /// best = 0; best_frame = game.frame + 1
    /// for i in 0 .. count:
    ///     if deaths[i].valid == 0: return i                  ; first free slot wins
    ///     if !type_of(deaths[i]).blocks_while_dead()
    ///        && deaths[i].first_frame < best_frame:
    ///            best_frame = deaths[i].first_frame; best = i
    /// return best                                            ; oldest non-blocking
    /// ```
    ///
    /// Two details that matter. `best_frame` starts at `frame + 1`, so on a full ring in
    /// which *every* corpse is blocking the function returns slot **0** — it overwrites
    /// rather than failing. And a corpse whose type `blocks_while_dead()` (type vtable
    /// `+0x120`, `UnitTypeData::blocks_while_dead` `0x00470440`) is never chosen for
    /// eviction while a non-blocking one exists, because a blocking corpse still occupies
    /// its tile: `DeathObj::clear_blocking` `0x008D4AC0` has to run before its slot is
    /// reused.
    ///
    /// `blocks_while_dead` is supplied per slot because it lives on the dead object's type,
    /// which this module does not own.
    pub fn choose_slot(
        &self,
        frame: i32,
        blocks_while_dead: &dyn Fn(&DeathRecord) -> bool,
    ) -> usize {
        let mut best = 0usize;
        let mut best_frame = frame.wrapping_add(1);
        for (i, rec) in self.slots.iter().enumerate() {
            if rec.valid == 0 {
                return i;
            }
            if !blocks_while_dead(rec) && rec.first_frame < best_frame {
                best_frame = rec.first_frame;
                best = i;
            }
        }
        best
    }

    /// Install a corpse. Returns the slot index and whether the evicted occupant needed
    /// `DeathObj::clear_blocking` (`0x00653C0F`).
    pub fn add_death(
        &mut self,
        frame: i32,
        rec: DeathRecord,
        blocks_while_dead: &dyn Fn(&DeathRecord) -> bool,
    ) -> (usize, bool) {
        let slot = self.choose_slot(frame, blocks_while_dead);
        let evicted = self.slots[slot];
        let needs_clear = evicted.valid != 0 && blocks_while_dead(&evicted);
        self.slots[slot] = rec;
        (slot, needs_clear)
    }
}

// ===========================================================================================
// 12. Checksum channels
// ===========================================================================================

/// zlib `adler32` — `0x00A46830`, the hash behind every `CheckSum` channel.
///
/// Re-exported from [`crate::checksum`]; this module used to carry its own copy of the
/// arithmetic. `Adler32::new()` is still the seed `check_all` stores into `CheckSum::accum`
/// before each channel (`local_1c[0] = 1` at `0x0093658A`), and `finish()` is still
/// `(s2 << 16) | s1`.
pub use crate::checksum::Adler32;

/// One `CheckSum` walker — the object `CheckSums::check_all` builds on its own stack at
/// `0x00936583`.
///
/// The PDB layout is `CheckSum : DataWalk { vftable, input, checksum, flags }` plus
/// `+0x10 accum : unsigned long` and `+0x14 size : unsigned long`, 24 bytes. `check_all`
/// resets `accum = 1`, `size = 0`, runs one channel, prints `accum`, and **sums the
/// per-channel `accum` values** into its return (`iVar3 = iVar3 + local_1c[0]`, repeated
/// once per channel).
///
/// So the sync value is `Σ over 15 channels of adler32(that channel's walked bytes)`, with
/// 32-bit wraparound on the sum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckSumChannel {
    /// `+0x10 accum`.
    pub accum: Adler32,
    /// `+0x14 size`, the byte count.
    pub size: u32,
}

impl Default for CheckSumChannel {
    fn default() -> Self {
        CheckSumChannel::new()
    }
}

impl CheckSumChannel {
    /// A fresh channel: `accum = 1`, `size = 0`.
    pub const fn new() -> CheckSumChannel {
        CheckSumChannel {
            accum: Adler32::new(),
            size: 0,
        }
    }
    /// `DataWalk::walk(begin, end)` — vtable slot 0, the only op that feeds the hash.
    pub fn walk(&mut self, bytes: &[u8]) {
        self.accum.update(bytes);
        self.size = self.size.wrapping_add(bytes.len() as u32);
    }
    /// The channel value `check_all` adds to its running total.
    pub fn value(&self) -> u32 {
        self.accum.finish()
    }
}

/// `CheckSums::check_deaths` — `0x00936BB0`, checksums.cpp:824. Channel **5**.
///
/// ```text
/// for i in 0 .. objects.death_objs.count:            ; [Objects+0x140]
///     rec = objects.death_objs.base + i * 164        ; [Objects+0x14C]
///     if (rec->valid == 0) continue                  ; free slots contribute nothing
///     label("DeathObj")                              ; DataWalk::label, vtable +4
///     walk(&rec->valid, &rec->first_frame)           ; [0x00, 0x04)
///     if (rec->valid != 0)
///         walk(&rec->first_frame, (char*)rec + 0x4B) ; [0x04, 0x4B)
/// ```
///
/// (`DeathObjData::walk_data` `0x008D5520` is the identical body; `check_deaths` inlines
/// it rather than dispatching.)
///
/// The label call is `DataWalk::label(const char*)`, and for `SaveGame` it writes a section
/// name. **Whether `CheckSum::label` folds the string into `accum` is not established** —
/// `state-schema.json`'s extractor annotates it "walk_test -> 1 tag byte". This function
/// therefore hashes payload bytes only, and [`DEATHS_LABEL_UNRESOLVED`] records the gap. A
/// replay harness comparing against retail must resolve it before trusting an exact match.
pub fn check_deaths(ring: &DeathRing) -> CheckSumChannel {
    let mut ch = CheckSumChannel::new();
    for rec in &ring.slots {
        if rec.valid == 0 {
            continue;
        }
        let b = rec.walked_bytes();
        ch.walk(&b[0x00..0x04]);
        ch.walk(&b[0x04..0x4B]);
    }
    ch
}

/// The one unresolved piece of the deaths/units channel reproduction.
pub const DEATHS_LABEL_UNRESOLVED: &str =
    "CheckSum's DataWalk::label (vtable +4) is called once per record with a StringTable \
     pointer ([0x00C06378]+0x10 + 0xCBE8); whether it contributes bytes to accum is not \
     established. check_deaths/check_units here hash payload only.";

/// The combat-owned bytes of one live unit's `units`-channel contribution.
///
/// `CheckSums::check_units` (`0x009371D0`, checksums.cpp:502) iterates active players in
/// leader-array order (`0x00E3A390`, stride `0x6EEC`), then each player's object band from
/// `obj_base` to `obj_mark[0][player]`, and calls `vtable +0x7C` — `Unit::walk_data`
/// `0x0060CF40` — on every object whose `flags & 1` is set.
///
/// `Unit::walk_data` walks `Object::walk_data`'s `[32,66)` and then `UnitData[72,183)`,
/// plus the path stack, order list and garrison. This lane owns a strict subset of those
/// bytes; the rest belong to movement, orders and groups. Rather than pretend to produce
/// the whole channel, [`patch_object_range`] and [`patch_unit_range`] write **only** the
/// combat fields into a caller-supplied buffer at their true offsets, so the lanes compose
/// byte-exactly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UnitCombatState {
    /// `ObjectData +0x20/+0x24/+0x3B`.
    pub hp: HitPoints,
    /// `ObjectData +0x32 hold_frames`.
    pub hold_frames: u16,
    /// `ObjectData +0x3D targeted`.
    pub targeted: i8,
    /// `UnitData +0x4C/+0xA4/+0xA9`.
    pub overkill: OverkillState,
    /// `UnitData +0x50 angle` — the facing the flank term differences against.
    pub angle: i32,
    /// `UnitData +0x5C trench_angle`.
    pub trench_angle: i32,
    /// `UnitData +0x68 unit_masks`.
    pub unit_masks: u32,
    /// `UnitData +0x6C unit_masks2`.
    pub unit_masks2: u32,
    /// `UnitData +0xAE recharging`.
    pub cycle: AttackCycle,
}

/// Start of `Object::walk_data`'s byte range (`0x00647883`).
pub const OBJECT_WALK_BEGIN: usize = 32;
/// Length of `Object::walk_data`'s byte range: `[32, 66)`.
pub const OBJECT_WALK_LEN: usize = 34;
/// Start of `Unit::walk_data`'s byte range (`0x0060CFCB`).
pub const UNIT_WALK_BEGIN: usize = 72;
/// Length of `Unit::walk_data`'s byte range: `[72, 183)`.
pub const UNIT_WALK_LEN: usize = 111;

impl UnitCombatState {
    /// Write the combat-owned fields of `Object::walk_data`'s `[32,66)` window.
    ///
    /// Covers `myhits`, `damage`, `hold_frames`, `damage_frac`, `targeted`. Leaves the
    /// inside/up/down/uid/near/healing/infiltrated/mylos/visible/launch_frames bytes for
    /// the lanes that own them.
    pub fn patch_object_range(&self, buf: &mut [u8; OBJECT_WALK_LEN]) {
        let put = |buf: &mut [u8; OBJECT_WALK_LEN], off: usize, v: &[u8]| {
            let o = off - OBJECT_WALK_BEGIN;
            buf[o..o + v.len()].copy_from_slice(v);
        };
        put(buf, 0x20, &self.hp.myhits.to_le_bytes());
        put(buf, 0x24, &self.hp.damage.to_le_bytes());
        put(buf, 0x32, &self.hold_frames.to_le_bytes());
        put(buf, 0x3B, &self.hp.damage_frac.to_le_bytes());
        put(buf, 0x3D, &self.targeted.to_le_bytes());
    }

    /// Write the combat-owned fields of `Unit::walk_data`'s `[72,183)` window.
    ///
    /// Covers `damage_frame`, `angle`, `trench_angle`, `unit_masks`, `unit_masks2`,
    /// `damage_o`, `damage_who`, `recharging`.
    pub fn patch_unit_range(&self, buf: &mut [u8; UNIT_WALK_LEN]) {
        let put = |buf: &mut [u8; UNIT_WALK_LEN], off: usize, v: &[u8]| {
            let o = off - UNIT_WALK_BEGIN;
            buf[o..o + v.len()].copy_from_slice(v);
        };
        put(buf, 0x4C, &self.overkill.damage_frame.to_le_bytes());
        put(buf, 0x50, &self.angle.to_le_bytes());
        put(buf, 0x5C, &self.trench_angle.to_le_bytes());
        put(buf, 0x68, &self.unit_masks.to_le_bytes());
        put(buf, 0x6C, &self.unit_masks2.to_le_bytes());
        put(buf, 0xA4, &self.overkill.damage_o.to_le_bytes());
        put(buf, 0xA9, &self.overkill.damage_who.to_le_bytes());
        put(buf, 0xAE, &self.cycle.recharging.to_le_bytes());
    }
}

// ===========================================================================================
// 13. Ordering facts the replay harness needs
// ===========================================================================================

/// The order in which one attack's effects hit the world, from `Object::do_damage`'s
/// address order. Divergence here desyncs even when every number is right.
///
/// 1. `0x0064A4B6` attacker `is_unit()` and `attacker.unit_masks & 1` → **abort, no damage**.
/// 2. `0x0064A4F7` `get_damage(...)`, always with `overkill_gate = 1` from this caller.
/// 3. `0x0064A521` ammo-flag lookup selects the hit effect id.
/// 4. `0x0064A5CF` `Unit::target_opportunity` — the victim's captain is told to consider
///    the attacker (retaliation), *before* the damage is applied.
/// 5. `0x0064A5DD` `unit_masks & 0x10` doubling.
/// 6. `0x0064A5EB` overkill stamp, if the window elapsed.
/// 7. `0x0064A67B` `Object::attempt_launch` — a garrisoned defender may scramble.
/// 8. `0x0064A6F9` scaling to whole + sixteenths.
/// 9. `0x0064BA18` `take_damage`, which may kill and cascade to the captain.
/// 10. `0x0064BBB0` `Build::plunder`, `0x0064BC12` `Armies::emergency`,
///     `0x0064BCED` `Object::eject_contents` on a killed transport.
/// 11. `0x0064C10C` the splash pass, recursing into `do_damage` per victim.
pub const DO_DAMAGE_ORDER: &str = "see the doc comment on DO_DAMAGE_ORDER";

/// Attacker gate at the very top of `Object::do_damage` — `0x0064A49E`, `0x0064A4D8`.
///
/// `if (arg6 <= 0) return;` and `if (attacker->is_unit() && attacker.unit_masks & 1) return;`.
/// A zero or negative 8.8 scale is a no-op, and `unit_masks` bit 0 disables the attacker
/// outright.
#[inline]
pub fn do_damage_runs(scale_8_8: i32, attacker_is_unit: bool, attacker_unit_masks: u32) -> bool {
    if scale_8_8 <= 0 {
        return false;
    }
    !(attacker_is_unit && attacker_unit_masks & 1 != 0)
}

// ===========================================================================================
// Tests
// ===========================================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- the shipped constants ----

    #[test]
    fn overkill_is_thirty_frames_and_eighty_five_two_fifty_sixths() {
        let r = CombatConstants::shipped();
        assert_eq!(
            r.overkill_frames, 30,
            "OVERKILL_FRAMES, rules.xml + Constants+0x5C"
        );
        assert_eq!(r.overkill_damage, 85, "fraction(256) of \"1/3\"");
        // The folklore "one third" is 85/256 = 0.33203125, not 0.3333...
        assert_eq!(100 * 85 / 256, 33);
        assert_ne!(85 * 3, 256);
    }

    #[test]
    fn overkill_window_is_two_game_seconds() {
        // rules.xml header: "Times are specified in 'frames' or fifteenths of seconds".
        assert_eq!(CombatConstants::shipped().overkill_frames, 2 * 15);
    }

    // ---- vector_dist ----

    #[test]
    fn vector_dist_is_the_octagon_metric_not_euclid() {
        assert_eq!(vector_dist(0, 0), 0);
        assert_eq!(vector_dist(5, 0), 5);
        assert_eq!(vector_dist(0, -7), 7);
        // 3-4-5 lands exactly: 4 + 3*3/(2*4) = 4 + 1 = 5.
        assert_eq!(vector_dist(3, 4), 5);
        // ...but the diagonal collapses: 1 + 1/(2*1) = 1 + 0 = 1.
        assert_eq!(vector_dist(1, 1), 1);
        assert_eq!(vector_dist(-1, 1), 1);
        // 100,100 -> 100 + 10000/200 = 150, where Euclid says 141.
        assert_eq!(vector_dist(100, 100), 150);
    }

    #[test]
    fn vector_dist_degrades_above_sixty_thousand_to_avoid_overflow() {
        // small >= 60000 takes the (small + 2*big) >> 1 arm at 0x0046D01B.
        let d = vector_dist(70_000, 60_000);
        assert_eq!(d, ((60_000u32 + 2 * 70_000) >> 1) as i32);
        // one below the threshold takes the quadratic arm
        let d2 = vector_dist(70_000, 59_999);
        assert_eq!(d2, (59_999u32 * 59_999 / 140_000 + 70_000) as i32);
    }

    #[test]
    fn vector_dist_between_matches_the_two_arg_form() {
        assert_eq!(vector_dist_between(10, 10, 13, 14), vector_dist(3, 4));
    }

    // ---- the circle table ----

    #[test]
    fn ring_one_has_nine_cells_which_is_why_splash_is_a_three_by_three() {
        let t = circle_table();
        assert_eq!(t.ring_end[0], 1, "ring 0 is the origin alone");
        assert_eq!(
            t.ring_end[1], 9,
            "Object::do_damage's splash bound [0x00CBE334] = ring_end[1]"
        );
        let tiles: Vec<(i8, i8)> = (0..9).map(|i| (t.x[i], t.y[i])).collect();
        for dx in -1i8..=1 {
            for dy in -1i8..=1 {
                assert!(tiles.contains(&(dx, dy)), "3x3 block missing ({dx},{dy})");
            }
        }
    }

    #[test]
    fn circle_rings_are_cumulative_and_monotone() {
        let t = circle_table();
        for r in 1..=CIRCLE_MAX_RING {
            assert!(
                t.ring_end[r] >= t.ring_end[r - 1],
                "ring_end must be cumulative at r={r}"
            );
        }
        assert_eq!(t.x.len(), t.y.len());
        assert_eq!(t.x.len(), *t.ring_end.last().unwrap() as usize);
        // circle_init emits in row-major order within each ring; entry 0 is the origin.
        assert_eq!((t.x[0], t.y[0]), (0, 0));
    }

    #[test]
    fn every_circle_entry_sits_on_its_ring() {
        let t = circle_table();
        let mut lo = 0usize;
        for r in 0..=CIRCLE_MAX_RING {
            let hi = t.ring_end[r] as usize;
            for i in lo..hi {
                assert_eq!(
                    vector_dist(t.x[i] as i32, t.y[i] as i32),
                    r as i32,
                    "entry {i} is not on ring {r}"
                );
            }
            lo = hi;
        }
    }

    #[test]
    fn circle_offsets_fit_in_a_signed_byte() {
        // The tables are `char[]`; ring 0x40 reaches +-64, which is why 0x40 is the cap.
        let t = circle_table();
        assert!(t.x.iter().all(|v| (-64..=64).contains(&(*v as i32))));
    }

    // ---- recharge ----

    #[test]
    fn non_siege_ignores_the_artillery_penalty_entirely() {
        let r = CombatConstants::shipped();
        let i = RechargeInput {
            base_recharge: 20,
            is_siege: false,
            unit_masks2_bit0: true,
            in_supply: false,
            is_bombard: true,
        };
        assert_eq!(recharge_frames(&i, &r), 20);
    }

    #[test]
    fn siege_in_supply_and_unflagged_fires_at_base_rate() {
        let r = CombatConstants::shipped();
        let i = RechargeInput {
            base_recharge: 20,
            is_siege: true,
            unit_masks2_bit0: false,
            in_supply: true,
            is_bombard: false,
        };
        assert_eq!(recharge_frames(&i, &r), 20);
    }

    #[test]
    fn siege_out_of_supply_reloads_half_again_as_slowly() {
        let r = CombatConstants::shipped();
        let i = RechargeInput {
            base_recharge: 21,
            is_siege: true,
            unit_masks2_bit0: false,
            in_supply: false,
            is_bombard: false,
        };
        assert_eq!(recharge_frames(&i, &r), 31, "21*3/2 truncates to 31");
    }

    #[test]
    fn bombards_take_the_double_penalty_not_the_half() {
        let r = CombatConstants::shipped();
        let i = RechargeInput {
            base_recharge: 21,
            is_siege: true,
            unit_masks2_bit0: false,
            in_supply: false,
            is_bombard: true,
        };
        assert_eq!(recharge_frames(&i, &r), 42);
    }

    #[test]
    fn the_under_attack_flag_only_bites_when_the_rule_is_on() {
        let mut r = CombatConstants::shipped();
        let i = RechargeInput {
            base_recharge: 20,
            is_siege: true,
            unit_masks2_bit0: true,
            in_supply: true,
            is_bombard: false,
        };
        // shipped: rule == 1, flag set -> the in_supply short-circuit is skipped
        assert_eq!(recharge_frames(&i, &r), 30);
        r.artillery_under_attack_fires_slowly = 0;
        assert_eq!(recharge_frames(&i, &r), 20);
    }

    // ---- attack cycle ----

    #[test]
    fn recharging_is_a_byte_and_a_long_reload_wraps_like_retail() {
        let mut c = AttackCycle::default();
        assert!(c.ready());
        c.fire(300); // 0x005FF0A4 stores only AL
        assert_eq!(c.recharging, 44, "300 & 0xFF");
        c.fire(20);
        for _ in 0..20 {
            assert!(!c.ready());
            c.tick();
        }
        assert!(c.ready());
        c.tick();
        assert_eq!(c.recharging, 0);
    }

    // ---- range ----

    #[test]
    fn range_is_measured_in_one_hundred_ninety_twoths_of_a_tile() {
        assert_eq!(RANGE_UNITS_PER_TILE, 3 << 6);
        assert!(in_attack_range(192, 1));
        assert!(!in_attack_range(193, 1));
        assert!(
            in_attack_range(0, 0),
            "melee: max_range 0 still hits at dist 0"
        );
        assert!(!in_attack_range(1, 0));
        assert!(below_min_range(100, 1));
        assert!(!below_min_range(100, 0), "min_range 0 never gates");
    }

    // ---- poor_target ----

    fn flank_level(delta: u32) -> u32 {
        // crate::mechanics::flank_level, 0x0092CFE0, Tier B at 500,017 inputs.
        if delta > 0xD555_5555 {
            0
        } else if 0x4000_0000u32 < delta.wrapping_sub(0x6000_0000) {
            2
        } else {
            1
        }
    }

    #[test]
    fn a_slower_pursuer_behind_a_distant_runner_gives_up() {
        let i = PoorTargetInput {
            both_are_units: true,
            target_guy_flag_0x40: false,
            attacker_has_objmask_high: false,
            target_speed_here: 100,
            attacker_speed_here: 50,
            flank_level_from_behind: 0x3000_0000, // tier 2
            attack_dist: 500,
            attacker_role_0x400: false,
            max_range_tiles: 0,
        };
        assert_eq!(flank_level(i.flank_level_from_behind), 2);
        assert!(poor_target(&i, flank_level));

        // faster than the runner: chase it
        let mut j = i;
        j.attacker_speed_here = 150;
        assert!(!poor_target(&j, flank_level));

        // a tier-1 arc instead of tier 2: not the chase case
        let mut k = i;
        k.flank_level_from_behind = 0x7000_0000;
        assert_eq!(flank_level(k.flank_level_from_behind), 1);
        assert!(!poor_target(&k, flank_level));

        // close enough to reach
        let mut m = i;
        m.attack_dist = 100;
        assert!(!poor_target(&m, flank_level));
    }

    #[test]
    fn the_engaged_arm_only_needs_the_objmask_and_range() {
        let base = PoorTargetInput {
            both_are_units: true,
            target_guy_flag_0x40: true,
            attacker_has_objmask_high: false,
            max_range_tiles: 2,
            attack_dist: 10,
            ..Default::default()
        };
        assert!(poor_target(&base, flank_level), "no objmask -> poor");
        let mut ok = base;
        ok.attacker_has_objmask_high = true;
        assert!(!poor_target(&ok, flank_level));
        ok.attack_dist = 2 * RANGE_UNITS_PER_TILE + 1;
        assert!(poor_target(&ok, flank_level));
    }

    #[test]
    fn a_non_unit_is_never_a_poor_target() {
        let i = PoorTargetInput {
            both_are_units: false,
            ..Default::default()
        };
        assert!(!poor_target(&i, flank_level));
    }

    // ---- overkill ----

    #[test]
    fn the_window_is_anchored_to_the_first_hit_not_slid_forward() {
        let r = CombatConstants::shipped();
        let mut st = OverkillState::default();
        assert!(
            overkill_should_stamp(&st, 1000, &r),
            "never damaged -> stamp"
        );
        overkill_stamp(&mut st, 1000, 7, 1);
        assert_eq!(st.damage_frame, 1000);
        // inside the 30-frame window: no re-stamp, so the anchor stays at 1000
        for f in 1001..1030 {
            assert!(!overkill_should_stamp(&st, f, &r), "frame {f}");
        }
        assert!(
            overkill_should_stamp(&st, 1030, &r),
            "exactly 30 frames later"
        );
    }

    #[test]
    fn attenuation_needs_a_different_attacker() {
        let r = CombatConstants::shipped();
        let st = OverkillState {
            damage_frame: 1000,
            damage_o: 7,
            damage_who: 1,
        };
        // the same captain that stamped it: full damage, always
        assert!(!overkill_attenuates(true, 5, true, true, 7, &st, 1010, &r));
        // a different one, inside the window: attenuated
        assert!(overkill_attenuates(true, 5, true, true, 9, &st, 1010, &r));
        // outside the window: full damage
        assert!(!overkill_attenuates(true, 5, true, true, 9, &st, 1030, &r));
    }

    #[test]
    fn buildings_and_melee_never_overkill() {
        let r = CombatConstants::shipped();
        let st = OverkillState {
            damage_frame: 1000,
            damage_o: 7,
            damage_who: 1,
        };
        // rules.xml: "Does not apply to damage from buildings." -> is_unit gate
        assert!(!overkill_attenuates(true, 5, false, true, 9, &st, 1010, &r));
        // max_range == 0 is melee, and the same gate excludes it
        assert!(!overkill_attenuates(true, 0, true, true, 9, &st, 1010, &r));
    }

    #[test]
    fn attenuation_is_eighty_five_two_fifty_sixths_then_a_conditional_halve() {
        let r = CombatConstants::shipped();
        assert_eq!(overkill_apply(1000, false, false, &r), 332);
        // catapult defender, non-siege attacker: halved again
        assert_eq!(overkill_apply(1000, true, false, &r), 166);
        // catapult defender, siege attacker: not halved
        assert_eq!(overkill_apply(1000, true, true, &r), 332);
    }

    #[test]
    fn splash_victims_neither_retaliate_nor_stamp() {
        assert!(overkill_block_runs(true, false));
        assert!(
            !overkill_block_runs(true, true),
            "the recursion passes arg8 = 1"
        );
        assert!(
            !overkill_block_runs(false, false),
            "buildings are skipped too"
        );
    }

    #[test]
    fn unit_mask_ten_doubles_before_the_stamp() {
        assert_eq!(double_on_unit_mask_0x10(50, 0x10), 100);
        assert_eq!(double_on_unit_mask_0x10(50, 0x20), 50);
    }

    // ---- scaling ----

    #[test]
    fn a_normal_melee_hit_scales_by_one_and_divides_by_uber_size() {
        // Unit::fight's melee call: scale 0x100, ammo_index -1, so no ammo divide.
        let s = scale_damage_unit(40, 0x100, -1, 1, 1).unwrap();
        assert_eq!(s.whole, 40);
        assert_eq!(s.sixteenths, 0);
        // an uber of 4 takes a quarter each
        let s4 = scale_damage_unit(40, 0x100, -1, 1, 4).unwrap();
        assert_eq!(s4.whole, 10);
    }

    #[test]
    fn the_sixteenths_are_the_remainder_not_a_rounding_error() {
        // D=10, uber=3 -> 10*256/3 = 853; /16 = 53 -> frac 5, whole 3
        let s = scale_damage_unit(10, 0x100, -1, 1, 3).unwrap();
        assert_eq!((s.whole, s.sixteenths), (3, 5));
        // 3 + 5/16 = 3.3125, versus the exact 3.3333
        assert!((s.whole as f64 + s.sixteenths as f64 / 16.0 - 10.0 / 3.0).abs() < 0.03);
    }

    #[test]
    fn the_scaled_product_has_a_floor_of_two_fifty_six_before_any_divide() {
        // 0x0064A707 cmovle: a zero or negative chain result still lands 1/16.
        let s = scale_damage_unit(0, 0x100, -1, 1, 1).unwrap();
        assert_eq!((s.whole, s.sixteenths), (1, 0));
        let neg = scale_damage_unit(-500, 0x100, -1, 1, 1).unwrap();
        assert_eq!((neg.whole, neg.sixteenths), (1, 0));
    }

    #[test]
    fn a_volley_divides_by_rounds_only_when_an_ammo_index_is_supplied() {
        let no_ammo = scale_damage_unit(40, 0x100, -1, 4, 1).unwrap();
        assert_eq!(no_ammo.whole, 40);
        let with_ammo = scale_damage_unit(40, 0x100, 3, 4, 1).unwrap();
        assert_eq!(with_ammo.whole, 10, "40 / ammo_per_att=4");
    }

    #[test]
    fn zero_uber_size_reports_the_faulting_site_instead_of_inventing_a_number() {
        let e = scale_damage_unit(40, 0x100, -1, 1, 0).unwrap_err();
        assert_eq!(e.site, "0x0064A766");
        let e2 = scale_damage_unit(40, 0x100, 0, 0, 1).unwrap_err();
        assert_eq!(e2.site, "0x0064A735");
    }

    #[test]
    fn the_building_branch_has_no_uber_divide_and_no_floor() {
        let s = scale_damage_build(40, 0x100, 1).unwrap();
        assert_eq!(s.whole, 40);
        let tiny = scale_damage_build(0, 0x100, 1).unwrap();
        assert_eq!((tiny.whole, tiny.sixteenths), (0, 0), "no 0x100 clamp here");
    }

    // ---- splash ----

    #[test]
    fn land_siege_splashes_at_a_quarter_and_everyone_else_at_an_eighth() {
        assert_eq!(splash_scale(0x100, true, DOMAIN_LAND), 0x40);
        assert_eq!(splash_scale(0x100, false, DOMAIN_LAND), 0x20);
        assert_eq!(
            splash_scale(0x100, true, DOMAIN_SEA),
            0x20,
            "sea siege uses /8"
        );
    }

    #[test]
    fn splash_divides_truncate_toward_zero_for_negatives() {
        assert_eq!(splash_scale(-9, true, DOMAIN_LAND), -2);
        assert_eq!(splash_scale(-9, false, DOMAIN_LAND), -1);
    }

    #[test]
    fn aircraft_and_sea_siege_do_not_splash_at_all() {
        assert!(!splash_enabled(DOMAIN_AIR, false));
        assert!(!splash_enabled(DOMAIN_SEA, true));
        assert!(splash_enabled(DOMAIN_SEA, false));
        assert!(splash_enabled(DOMAIN_LAND, true));
    }

    #[test]
    fn the_blast_footprint_is_sized_by_the_target_and_the_reach_has_a_floor() {
        let r = splash_radii(2, 3, 0);
        assert_eq!(r.target_footprint, 3 * 192, "max(x_size, y_size) * 192");
        assert_eq!(r.attacker_reach, SPLASH_MIN_RANGE_RADIUS, "floor of 0x180");
        let far = splash_radii(1, 1, 5);
        assert_eq!(far.attacker_reach, 5 * 192);
    }

    #[test]
    fn splash_geometry_gates_in_retail_order() {
        let radii = splash_radii(1, 1, 4);
        let mut c = SplashCandidate {
            passes_search_filters: true,
            owner_has_splash_immunity: false,
            dist_to_primary_target: 100,
            dist_to_attacker: 100,
        };
        assert!(splash_hits(&c, &radii, false));
        c.dist_to_primary_target = radii.target_footprint + 1;
        assert!(!splash_hits(&c, &radii, false));
        c.dist_to_primary_target = 100;
        c.dist_to_attacker = radii.attacker_reach + 1;
        assert!(
            !splash_hits(&c, &radii, false),
            "beyond the attacker's reach"
        );
        assert!(
            splash_hits(&c, &radii, true),
            "land siege skips the reach test at 0x0064C3EA"
        );
        c.owner_has_splash_immunity = true;
        assert!(!splash_hits(&c, &radii, true));
    }

    #[test]
    fn the_splash_scan_visits_the_nine_tiles_in_circle_order() {
        let t = circle_table();
        let scan = SplashScan::new(&t, (10, 20));
        let tiles = scan.tiles();
        assert_eq!(tiles.len(), 9);
        assert_eq!(tiles[0], (10, 20), "the target's own tile first");
        let mut sorted = tiles.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 9, "no repeats");
    }

    // ---- flanking / height / entrenchment ----

    #[test]
    fn the_flank_gate_rejects_on_any_one_mask() {
        assert!(flank_gate(true, true, 0, 0));
        assert!(!flank_gate(false, true, 0, 0));
        assert!(!flank_gate(true, true, 4, 0));
        assert!(!flank_gate(true, true, 0, 4));
        assert!(!flank_gate(true, true, 0x1000_0000, 0));
        assert!(!flank_gate(true, true, 0x2000, 0), "am 0x2000 != dm 0x2000");
        assert!(
            !flank_gate(true, true, 0x2000, 0x2000),
            "equal but non-zero still rejects"
        );
    }

    /// `delta >= 0x2AAAAAAA`, the caller's pre-guard at `0x00644B1D`.
    fn flank_considered(delta: u32) -> bool {
        delta >= 0x2AAA_AAAA
    }

    #[test]
    fn the_flank_arcs_are_a_hundred_twenty_dead_then_seventy_five_ninety_seventy_five() {
        // A 120-degree arc centred on delta == 0 gets nothing: half of it is skipped by
        // the caller's guard, the other half is tier 0.
        assert!(!flank_considered(0));
        assert!(!flank_considered(0x2AAA_AAA9));
        assert_eq!(flank_level(0xFFFF_FFFF), 0);
        assert_eq!(flank_level(0xD555_5556), 0);
        // then 75 degrees of tier 2 on each side of a 90-degree tier-1 arc
        assert!(flank_considered(0x2AAA_AAAA));
        assert_eq!(flank_level(0x2AAA_AAAA), 2);
        assert_eq!(flank_level(0x5FFF_FFFF), 2);
        assert_eq!(flank_level(0x6000_0000), 1);
        assert_eq!(flank_level(0x8000_0000), 1, "centre of the tier-1 arc");
        assert_eq!(flank_level(0xA000_0000), 1);
        assert_eq!(flank_level(0xA000_0001), 2);
        assert_eq!(flank_level(0xD555_5555), 2);
    }

    #[test]
    fn the_bias_is_a_half_turn_and_is_its_own_inverse() {
        assert_eq!(flank_delta(0x0000_0000, 0x0000_0000), 0x8000_0000);
        assert_eq!(flank_delta(0x0000_0000, 0x8000_0000u32 as i32), 0);
        // subtracting 0x80000000 twice is the identity
        let x: u32 = 0x1234_5678;
        assert_eq!(x.wrapping_sub(0x8000_0000).wrapping_sub(0x8000_0000), x);
    }

    #[test]
    fn flank_percent_gives_fifty_per_level_to_infantry_and_far_less_to_horse() {
        let r = CombatConstants::shipped();
        assert_eq!(flank_percent(1, 0, &r), 150, "+50%");
        assert_eq!(flank_percent(2, 0, &r), 200, "rules.xml: max is twice");
        // cavalry: 40 * 50 / 256 = 7, not 20
        assert_eq!(flank_percent(1, 0x1000, &r), 107);
        assert_eq!(flank_percent(2, 0x1000, &r), 114);
        // vehicles: 33 * 50 / 256 = 6
        assert_eq!(flank_percent(1, 0x0020_0000, &r), 106);
        // vehicle bit wins over cavalry bit (else-if order at 0x00644B42)
        assert_eq!(flank_percent(1, 0x0020_1000, &r), 106);
    }

    #[test]
    fn height_is_ten_percent_per_two_hundred_z_and_only_uphill() {
        let r = CombatConstants::shipped();
        assert!(height_applies(DOMAIN_LAND, DOMAIN_LAND, false, 400, 200));
        assert!(!height_applies(DOMAIN_LAND, DOMAIN_LAND, false, 200, 400));
        assert!(!height_applies(DOMAIN_AIR, DOMAIN_LAND, false, 400, 200));
        assert!(
            !height_applies(DOMAIN_LAND, DOMAIN_LAND, true, 400, 200),
            "siege attackers get no height bonus"
        );
        // dz = 200 = one increment -> +10%
        assert_eq!(height_bonus_add(1000, 400, 200, &r).unwrap(), 100);
        assert_eq!(height_bonus_add(1000, 600, 200, &r).unwrap(), 200);
        let mut zero = r;
        zero.height_increment = 0;
        assert_eq!(
            height_bonus_add(1000, 400, 200, &zero).unwrap_err().site,
            "0x00644D78"
        );
    }

    #[test]
    fn coordinates_are_xor_obfuscated_and_the_mask_is_its_own_inverse() {
        assert_eq!(COORD_XOR, 0x00063637);
        assert_eq!(coord_unmask(coord_unmask(12345)), 12345);
    }

    #[test]
    fn flamethrowers_ignore_entrenchment() {
        assert!(entrench_applies(true, 0x0200_0000, false));
        assert!(!entrench_applies(true, 0x0200_0000, true), "TypeIndex 0x83");
        assert!(!entrench_applies(true, 0, false), "not entrenched");
        assert!(!entrench_applies(false, 0x0200_0000, false));
    }

    #[test]
    fn entrench_delta_uses_the_same_bias_as_the_flank_delta() {
        assert_eq!(entrench_delta(0x1234, 0x1234), 0x8000_0000);
        assert_eq!(flank_delta(0x1234, 0x1234), 0x8000_0000);
    }

    // ---- hit points ----

    #[test]
    fn hits_left_is_clamped_not_a_bare_subtraction() {
        let hp = HitPoints {
            myhits: 100,
            damage: 30,
            damage_frac: 0,
        };
        assert_eq!(hp.hits_left(), 70);
        let dead = HitPoints {
            myhits: 100,
            damage: 150,
            damage_frac: 0,
        };
        assert_eq!(dead.hits_left(), 0, "never negative");
        let odd = HitPoints {
            myhits: -5,
            damage: 0,
            damage_frac: 0,
        };
        assert_eq!(odd.hits_left(), 0);
    }

    #[test]
    fn a_hit_always_lands_for_at_least_one_sixteenth() {
        let mut hp = HitPoints::default();
        // get_damage returned 0 and no fraction: retail forces frac = 1.
        let applied = hp.accumulate(0, 0);
        assert_eq!(applied, 0);
        assert_eq!(hp.damage_frac, 1);
        assert_eq!(hp.damage, 0);
        // sixteen such pinpricks make one whole point
        for _ in 0..15 {
            hp.accumulate(0, 0);
        }
        assert_eq!(hp.damage_frac, 0);
        assert_eq!(hp.damage, 1);
    }

    #[test]
    fn the_sixteenths_carry_into_whole_points() {
        let mut hp = HitPoints::default();
        hp.accumulate(3, 10);
        assert_eq!((hp.damage, hp.damage_frac), (3, 10));
        hp.accumulate(3, 10);
        assert_eq!(
            (hp.damage, hp.damage_frac),
            (7, 4),
            "20/16 = 1 carry, 4 left"
        );
    }

    #[test]
    fn the_fraction_accumulator_wraps_in_a_signed_byte_like_retail() {
        // (char)(120 + 100) = (char)220 = -36, which then carries NEGATIVELY.
        let mut hp = HitPoints {
            myhits: 0,
            damage: 0,
            damage_frac: 120,
        };
        hp.accumulate(0, 100);
        let u = 120i8.wrapping_add(100) as i32;
        assert_eq!(u, -36);
        assert_eq!(hp.damage, (u + ((u >> 31) & 15)) >> 4);
        assert_eq!(hp.damage as i32, -2);
        assert_eq!(hp.damage_frac, (-36 % 16) as i8);
        // Recording this because it is a real overflow the engine tolerates, not a bug
        // in the port: nothing clamps damage_frac before the add at 0x006522F2.
    }

    #[test]
    fn effective_max_hits_splits_an_uber_unit() {
        assert_eq!(effective_max_hits(100, 1, UberBranch::Even), 100);
        assert_eq!(effective_max_hits(100, 0, UberBranch::Even), 100);
        assert_eq!(effective_max_hits(100, 4, UberBranch::Even), 25);
        assert_eq!(effective_max_hits(100, 3, UberBranch::Even), 33);
        // the remainder arm rounds the other way on a non-multiple
        assert_eq!(effective_max_hits(100, 3, UberBranch::Remainder), 34);
        assert_eq!(
            effective_max_hits(100, 4, UberBranch::Even),
            effective_max_hits(100, 4, UberBranch::Remainder),
            "identical when it divides evenly"
        );
    }

    #[test]
    fn death_is_at_or_past_the_effective_maximum_and_the_overflow_cascades() {
        let hp = HitPoints {
            myhits: 100,
            damage: 24,
            damage_frac: 0,
        };
        assert_eq!(resolve_damage(&hp, 25), DamageOutcome::Survived);
        let hp2 = HitPoints {
            myhits: 100,
            damage: 25,
            damage_frac: 0,
        };
        assert_eq!(
            resolve_damage(&hp2, 25),
            DamageOutcome::Died { overflow: 0 },
            "the comparison is <, so exactly-max dies"
        );
        let hp3 = HitPoints {
            myhits: 100,
            damage: 40,
            damage_frac: 0,
        };
        assert_eq!(
            resolve_damage(&hp3, 25),
            DamageOutcome::Died { overflow: 15 }
        );
    }

    // ---- corpse hold ----

    #[test]
    fn a_corpse_holds_its_slot_until_inbound_projectiles_land() {
        // no range on the type -> the ammo scan is skipped entirely
        assert_eq!(hold_frames_after_death(0, false, &[99], 0), 1);
        // 20 frames of flight left, +1, +global pad 0
        assert_eq!(hold_frames_after_death(0, true, &[20], 0), 21);
        // the longest inbound wins
        assert_eq!(hold_frames_after_death(0, true, &[5, 40, 12], 0), 41);
        // an existing longer hold is never shortened
        assert_eq!(hold_frames_after_death(200, true, &[5], 0), 200);
    }

    // ---- deaths channel ----

    #[test]
    fn a_death_record_walks_seventy_five_of_its_hundred_sixty_four_bytes() {
        assert_eq!(DEATH_OBJ_STRIDE, 164);
        assert_eq!(DEATH_OBJ_DATA_SIZE, 76);
        assert_eq!(DEATH_WALKED_BYTES, 0x4B);
        let r = DeathRecord {
            valid: 1,
            unit_crew: 7,
            ..Default::default()
        };
        let b = r.walked_bytes();
        assert_eq!(b.len(), 75);
        assert_eq!(b[0x4A], 7, "unit_crew is the last walked byte");
    }

    #[test]
    fn free_death_slots_contribute_nothing_to_the_channel() {
        let mut ring = DeathRing::with_capacity(4);
        let empty = check_deaths(&ring);
        assert_eq!(empty.size, 0);
        assert_eq!(empty.value(), Adler32::new().finish());
        ring.slots[2] = DeathRecord {
            valid: 1,
            first_frame: 500,
            ..Default::default()
        };
        let one = check_deaths(&ring);
        assert_eq!(one.size, 75);
        assert_ne!(one.value(), empty.value());
    }

    #[test]
    fn the_deaths_channel_notices_a_one_frame_difference() {
        let mut a = DeathRing::with_capacity(2);
        a.slots[0] = DeathRecord {
            valid: 1,
            first_frame: 500,
            ..Default::default()
        };
        let mut b = a.clone();
        b.slots[0].first_frame = 501;
        assert_ne!(check_deaths(&a).value(), check_deaths(&b).value());
    }

    #[test]
    fn adler32_matches_the_zlib_reference_vector() {
        let mut h = Adler32::new();
        h.update(b"Wikipedia");
        assert_eq!(h.finish(), 0x11E60398);
    }

    #[test]
    fn a_channel_is_a_running_adler_over_concatenated_walks() {
        let mut split = CheckSumChannel::new();
        split.walk(b"Wiki");
        split.walk(b"pedia");
        let mut whole = CheckSumChannel::new();
        whole.walk(b"Wikipedia");
        assert_eq!(split.value(), whole.value());
        assert_eq!(split.size, 9);
    }

    // ---- add_death slot policy ----

    #[test]
    fn add_death_takes_the_first_free_slot() {
        let mut ring = DeathRing::with_capacity(4);
        ring.slots[0].valid = 1;
        ring.slots[1].valid = 1;
        let never = |_: &DeathRecord| false;
        assert_eq!(ring.choose_slot(1000, &never), 2);
    }

    #[test]
    fn a_full_ring_evicts_the_oldest_non_blocking_corpse() {
        let mut ring = DeathRing::with_capacity(3);
        for (i, f) in [900, 500, 700].into_iter().enumerate() {
            ring.slots[i] = DeathRecord {
                valid: 1,
                first_frame: f,
                who: i as i32,
                ..Default::default()
            };
        }
        let never = |_: &DeathRecord| false;
        assert_eq!(
            ring.choose_slot(1000, &never),
            1,
            "first_frame 500 is oldest"
        );
        // if the oldest blocks, it is passed over
        let blocks_slot_1 = |r: &DeathRecord| r.who == 1;
        assert_eq!(ring.choose_slot(1000, &blocks_slot_1), 2);
    }

    #[test]
    fn a_full_ring_of_blocking_corpses_falls_back_to_slot_zero() {
        // best starts at 0 and best_frame at frame+1; if nothing updates them the
        // function returns 0 and overwrites, rather than refusing.
        let mut ring = DeathRing::with_capacity(3);
        for i in 0..3 {
            ring.slots[i] = DeathRecord {
                valid: 1,
                first_frame: 100 + i as i32,
                ..Default::default()
            };
        }
        let always = |_: &DeathRecord| true;
        assert_eq!(ring.choose_slot(1000, &always), 0);
    }

    #[test]
    fn add_death_reports_when_the_evicted_corpse_needed_unblocking() {
        let mut ring = DeathRing::with_capacity(1);
        ring.slots[0] = DeathRecord {
            valid: 1,
            first_frame: 10,
            ..Default::default()
        };
        let always = |_: &DeathRecord| true;
        let (slot, needs_clear) = ring.add_death(
            1000,
            DeathRecord {
                valid: 1,
                first_frame: 1000,
                ..Default::default()
            },
            &always,
        );
        assert_eq!(slot, 0);
        assert!(needs_clear, "DeathObj::clear_blocking must run first");
        assert_eq!(ring.slots[0].first_frame, 1000);
    }

    // ---- units channel patching ----

    #[test]
    fn the_combat_fields_land_at_their_true_walk_offsets() {
        let st = UnitCombatState {
            hp: HitPoints {
                myhits: 0x11223344,
                damage: 0x55667788,
                damage_frac: -3,
            },
            hold_frames: 0xBEEF,
            targeted: 1,
            overkill: OverkillState {
                damage_frame: 0x0A0B0C0D,
                damage_o: 0x1234,
                damage_who: 5,
            },
            angle: 0x40000000,
            trench_angle: -1,
            unit_masks: 0xDEAD_BEEF,
            unit_masks2: 1,
            cycle: AttackCycle { recharging: 42 },
        };
        let mut obj = [0u8; OBJECT_WALK_LEN];
        st.patch_object_range(&mut obj);
        assert_eq!(&obj[0..4], &0x11223344i32.to_le_bytes());
        assert_eq!(&obj[4..8], &0x55667788i32.to_le_bytes());
        assert_eq!(&obj[0x32 - 32..0x34 - 32], &0xBEEFu16.to_le_bytes());
        assert_eq!(obj[0x3B - 32] as i8, -3);
        assert_eq!(obj[0x3D - 32] as i8, 1);

        let mut unit = [0u8; UNIT_WALK_LEN];
        st.patch_unit_range(&mut unit);
        assert_eq!(&unit[0x4C - 72..0x50 - 72], &0x0A0B0C0Di32.to_le_bytes());
        assert_eq!(&unit[0x50 - 72..0x54 - 72], &0x40000000i32.to_le_bytes());
        assert_eq!(&unit[0x68 - 72..0x6C - 72], &0xDEADBEEFu32.to_le_bytes());
        assert_eq!(&unit[0xA4 - 72..0xA6 - 72], &0x1234i16.to_le_bytes());
        assert_eq!(unit[0xA9 - 72], 5);
        assert_eq!(unit[0xAE - 72], 42);
    }

    #[test]
    fn the_walk_windows_are_the_sizes_the_extractor_measured() {
        assert_eq!(OBJECT_WALK_LEN, 34, "Object::walk_data [32,66)");
        assert_eq!(UNIT_WALK_LEN, 111, "Unit::walk_data [72,183)");
        // every combat field must fit inside its window
        for off in [0x20usize, 0x24, 0x32, 0x3B, 0x3D] {
            assert!((OBJECT_WALK_BEGIN..OBJECT_WALK_BEGIN + OBJECT_WALK_LEN).contains(&off));
        }
        for off in [0x4Cusize, 0x50, 0x5C, 0x68, 0x6C, 0xA4, 0xA9, 0xAE] {
            assert!((UNIT_WALK_BEGIN..UNIT_WALK_BEGIN + UNIT_WALK_LEN).contains(&off));
        }
    }

    // ---- do_damage entry gate ----

    #[test]
    fn a_non_positive_scale_or_a_disabled_attacker_produces_no_damage_at_all() {
        assert!(do_damage_runs(0x100, true, 0));
        assert!(
            !do_damage_runs(0, true, 0),
            "arg6 <= 0 returns at 0x0064A4AE"
        );
        assert!(!do_damage_runs(-1, true, 0));
        assert!(!do_damage_runs(0x100, true, 1), "unit_masks bit 0 disables");
        assert!(
            do_damage_runs(0x100, false, 1),
            "the mask is only read for units"
        );
    }

    // ---- the real balance matrix ----

    #[test]
    fn balance_index_uses_the_four_ninety_three_stride_with_the_attacker_as_row() {
        let mut fake = vec![0i16; 493 * 493];
        fake[7 * 493 + 9] = 250;
        assert_eq!(balance_percent(&fake, 7, 9), Some(250));
        assert_eq!(balance_percent(&fake, 9, 7), Some(0));
        assert_eq!(balance_percent(&fake, 493, 0), None);
        assert_eq!(balance_percent(&fake, -1, 0), None);
    }

    /// Reads the captured table if it is present. It is gitignored game content, so this
    /// test reports and returns rather than failing where it is absent.
    #[test]
    fn the_shipped_balance_matrix_loads_and_is_all_percentages() {
        // `option_env!` rather than `env!` so the file still builds under a bare
        // `rustc --test`, which sets no cargo variables.
        let root = option_env!("CARGO_MANIFEST_DIR").unwrap_or("crates/don-sim");
        let path = std::path::Path::new(root).join("../../schema/live/balance-real.bin");
        let Ok(raw) = std::fs::read(&path) else {
            eprintln!(
                "skipped: {} absent (gitignored game content)",
                path.display()
            );
            return;
        };
        assert_eq!(raw.len(), 493 * 493 * 2, "short[493][493]");
        let table: Vec<i16> = raw
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert!(
            table.iter().all(|v| *v > 0),
            "a negative entry means the capture used the bias-folded base 0x00C06AFC"
        );
        let (lo, hi) = table
            .iter()
            .fold((i16::MAX, i16::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
        assert_eq!((lo, hi), (5, 2574), "the captured range");
        // 100% is by far the commonest entry: most pairs have no special relationship.
        let hundreds = table.iter().filter(|v| **v == 100).count();
        assert!(hundreds > table.len() / 3, "got {hundreds} entries at 100%");
        assert_eq!(balance_percent(&table, 50, 50), Some(100));
    }
}
