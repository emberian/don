//! Production: construction, training queues, cost ramping, repair and destruction.
//!
//! # What this is
//!
//! An executable port of the engine's building subsystem, from `game/build.cpp`,
//! `game/wall.cpp`, `game/buildtype.cpp`, `game/object.cpp` and `game/type.cpp`.
//! It serves the **`check_builds`** channel of `CheckSums::check_all` (`0x00936560`,
//! checksums.cpp:16) — channel 2 of 15, `CheckSums::check_builds` `0x00937290`
//! (checksums.cpp:646).
//!
//! The call chain, all names from the shipped PDB (`ron-bin/sbl/rise.pdb`):
//!
//! ```text
//! Objects::process_all        0x0065DCE0
//!  +- obj->vt[+0x9C] = Build::process        0x0061EDF0  build.cpp:7217
//!      +- Wall::process                      0x00640450  wall.cpp        <- resets `helpers`
//!      +- Build::do_queue(0)                 0x0061E410  build.cpp:7839  -> [`queue_step`]
//!      |   +- ObjectData::train_time         0x006508C0  object.cpp:1897 -> [`train_time_ramp`]
//!      |   +- Build::finished(type)          0x00628490  build.cpp:3277
//!      |   +- Build::unqueue(slot, 0)        0x006207C0  build.cpp:6743
//!      +- Build::find_gather_tiles           0x00623350
//!
//! Unit::do_job -> Unit::do_build             0x005EEBF0  unit.cpp:21588 (do_repair sibling)
//!  +- Wall::do_construct(rate)               0x006434D0  wall.cpp        -> [`do_construct`]
//!      +- Build::start(1)                    0x006273A0  build.cpp:3800
//!      +- Build::activate(0,1,1)             0x00623E20  build.cpp:3903  <- completion
//!
//! Wall::update_hits                          0x0063F0D0  wall.cpp:1951   -> [`construct_hits`]
//! Wall::update_construct_time                0x0063D560  wall.cpp:1886   -> [`update_construct_time`]
//! BuildData::construct_time                  0x0062D5C0  build.cpp:762   -> [`construct_time`]
//! Object::take_damage                        0x00652020  object.cpp:6628 -> [`under_construction_collapses`]
//! Build::repair_damage                       0x00628130  build.cpp:3471  -> [`repair_damage`]
//! Build::refund_cost                         0x00620490  build.cpp:6849  -> [`refund_amount`]
//! Build::unpay_cost                          0x006206E0  build.cpp:6827  -> [`unpay_amounts`]
//! TypeData::get_cost                         0x00664090  type.cpp:639    -> [`ramp_cost`]
//! BuildTypeData::{area,corner_tile,tile_corner}                          -> [`Footprint`]
//! ```
//!
//! # Provenance and tier
//!
//! Every function carries the VA it was ported from. Structure came from
//! `re/decomp-all/<VA>.c`; every load-bearing constant, comparison and division was
//! re-read at the instruction level with capstone against `ron-bin/riseofnations.exe`
//! (sha256 `30478a44…625079`). Field offsets are `[measured]` from the PDB TPI stream
//! (`schema/pdb-types.json`), not inferred. Rule offsets are `[measured]` from
//! `docs/derivation/rules-constants.json`.
//!
//! That makes this **[measured] structure, Tier C behaviour**: nothing here has been
//! executed against retail. It is *not* verified and *not* differentially tested. No claim
//! in this file may be promoted without an oracle run on hbox.
//!
//! # Four things that will silently break a reimplementation
//!
//! 1. **Construction progress is divided by `helpers + 1`, and `helpers` counts *within
//!    the current frame*.** `Wall::do_construct` (`0x006434D0`) does
//!    `rate /= helpers + 1; helpers += 1;` per worker, and `Wall::process` zeroes
//!    `helpers` at the top of each tick. So the *n*-th citizen to touch the site this
//!    frame contributes `rate/n`, and total progress is `rate * H_n` (harmonic), not
//!    `rate * n`. Worker iteration order therefore changes the result — see
//!    [`do_construct`].
//! 2. **The under-attack penalty is a `/4` on the construction *rate*, not on progress**,
//!    and it is skipped entirely for a player holding tribe bonus 16 while
//!    `korean_build_under_fire` (`Rules+0x7DC`) is non-zero. See [`construct_rate`].
//! 3. **A building under construction has *scaled* max HP**, computed with a `>>5`
//!    precision reduction on **both** numerator and denominator, each floored at 1
//!    (`Wall::update_hits` `0x0063F0D0`). Doing the ratio in full precision gives
//!    different integers. See [`construct_hits`].
//! 4. **The queue record is 20 bytes but only its first 18 are checksummed.**
//!    `BuildQueue::walk_data` (`0x006305F0`) emits `w.walk(entry, entry + 0x12)`. The
//!    trailing 2 bytes are never hashed. See [`BuildQueueEntry::WALKED_BYTES`].

#![allow(clippy::too_many_arguments)]

use crate::deviations::{behaviour as deviation_behaviour, ModeConfig};

// =======================================================================================
// Pool geometry
// =======================================================================================

/// First index of the **build band** inside a player's object list.
///
/// `CheckSums::check_builds` (`0x00937290`) starts its inner loop at
/// `GameAccess::obj_base[1]` (`[0x00C06198] + 4`). The literal `2000` appears directly in
/// `BuildData::construct_time` (`0x0062D5C0`) where the same band is scanned
/// (`iVar5 = 2000; while (iVar5 < counts[p])`), which pins `obj_base[1] == 2000`
/// [measured, two independent sites].
pub const BUILD_BAND_BASE: usize = 2000;

/// Slots in the build band. 601, per the lane brief; the band runs `2000..=2600` and the
/// next band (`obj_base[2]`) begins at 3000 per `Objects::process_all` (`0x0065DCE0`).
pub const BUILD_POOL_SLOTS: usize = 601;

/// Leader records: 8, `?leaders@@3VLeaders@@A` at `0x00E3A390`, stride `0x6EEC`, ending at
/// `0x00E71AF0` [measured]. `check_builds` walks exactly this range.
pub const NUM_LEADERS: usize = 8;

/// `sizeof(BuildData)` [measured, PDB TPI: `.?AVBuildData@@` size 220].
pub const BUILDDATA_SIZE: usize = 220;

/// One tile in `Coord` units. `BuildTypeData::corner_tile` (`0x006364C0`) multiplies the
/// doubled tile index by `0x60`; `BuildTypeData::tile_corner` (`0x00636440`) multiplies the
/// tile index by `0xC0` [measured]. So a tile is `0xC0 = 192` Coord units and `0x60 = 96`
/// is the half-tile.
pub const COORD_PER_TILE: i32 = 0xC0;
/// Half a tile in `Coord` units.
pub const COORD_HALF_TILE: i32 = 0x60;

// =======================================================================================
// Field offsets -- [measured] from schema/pdb-types.json (TPI stream)
// =======================================================================================

/// Byte offsets into `BuildData`. The class is `Object`(0..80) + `WallData`(72..112) +
/// `BuildData`(108..220); the overlap is real — `WallData` reuses the tail of the
/// `Object` allocation.
pub mod off {
    // -- Object / ObjectData ------------------------------------------------------------
    /// `ObjectData::myhits` — full (completed) max hit points.
    pub const MYHITS: usize = 32; // 0x20
    /// `ObjectData::damage` — accumulated damage; `hits_left = hits() - damage`.
    pub const DAMAGE: usize = 36; // 0x24
    /// `ObjectData::uid` — index of this object inside its owner's list.
    pub const UID: usize = 48; // 0x30
    /// `ObjectData::damage_frac` — sub-integer damage carry; cleared by `repair_damage`.
    pub const DAMAGE_FRAC: usize = 59; // 0x3B

    // -- WallData -----------------------------------------------------------------------
    /// `WallData::job_counter` — construction progress, in the same units as `constr_time`.
    pub const JOB_COUNTER: usize = 72; // 0x48
    /// `WallData::job_counter_2` — the parallel construction accumulator. It receives the
    /// same per-builder increment as `job_counter`; measured `Wall::activate` resets both
    /// counters to zero on completion.
    pub const JOB_COUNTER_2: usize = 76; // 0x4C
    /// `WallData::constr_time` — cached total construction time, written by
    /// `Wall::update_construct_time` (`0x0063D560`).
    pub const CONSTR_TIME: usize = 80; // 0x50
    /// `WallData::construct_hits` — *effective* max HP right now (scaled while building).
    pub const CONSTRUCT_HITS: usize = 84; // 0x54
    /// `WallData::gpiece` — art binding. Presentation, but inside the walked window.
    pub const GPIECE: usize = 88; // 0x58
    /// `WallData::frame_started`.
    pub const FRAME_STARTED: usize = 92; // 0x5C
    /// `WallData::build_masks` — bit field; see [`mask`].
    pub const BUILD_MASKS: usize = 96; // 0x60
    /// `WallData::ever_seen`.
    pub const EVER_SEEN: usize = 98; // 0x62
    /// `WallData::ever_seen_completed`.
    pub const EVER_SEEN_COMPLETED: usize = 99; // 0x63
    /// `WallData::helpers` — workers that have contributed **this frame**.
    pub const HELPERS: usize = 100; // 0x64
    /// `WallData::demolition`.
    pub const DEMOLITION: usize = 101; // 0x65

    // -- BuildData ----------------------------------------------------------------------
    /// `BuildData::orig_type`.
    pub const ORIG_TYPE: usize = 108; // 0x6C
    /// `BuildData::gather_down`.
    pub const GATHER_DOWN: usize = 112; // 0x70
    /// `BuildData::city`.
    pub const CITY: usize = 114; // 0x72
    /// `BuildData::city_down`.
    pub const CITY_DOWN: usize = 116; // 0x74
    /// `BuildData::wonder`.
    pub const WONDER: usize = 118; // 0x76
    /// `BuildData::dock` / `farm` / `fort` / `oil_well` — a union, one short.
    pub const DOCK: usize = 120; // 0x78
    /// `BuildData::recharging`.
    pub const RECHARGING: usize = 122; // 0x7A
    /// `BuildData::attack_ox`.
    pub const ATTACK_OX: usize = 124; // 0x7C
    /// `BuildData::stance`.
    pub const STANCE: usize = 126; // 0x7E
    /// `BuildData::founder`.
    pub const FOUNDER: usize = 127; // 0x7F
    /// `BuildData::gather_max`.
    pub const GATHER_MAX: usize = 128; // 0x80
    /// `BuildData::attack_whom`.
    pub const ATTACK_WHOM: usize = 129; // 0x81
    /// `BuildData::queued` — **number of occupied queue slots** (u8).
    pub const QUEUED: usize = 130; // 0x82
    /// `BuildData::max_age`.
    pub const MAX_AGE: usize = 131; // 0x83
    /// `BuildData::infiltrate`.
    pub const INFILTRATE: usize = 132; // 0x84
    /// `BuildData::infiltrate2`.
    pub const INFILTRATE2: usize = 133; // 0x85
    /// `BuildData::build_queue` — `BuildQueue`, 16 bytes: `{i32 num; T* data; …}`.
    pub const BUILD_QUEUE: usize = 136; // 0x88
    /// `BuildQueue::num`, the allocated/valid entry count.
    pub const BUILD_QUEUE_NUM: usize = 136; // 0x88
    /// `BuildQueue::data`, pointer to the entry array.
    pub const BUILD_QUEUE_DATA: usize = 140; // 0x8C
    /// `BuildData::gather_from` — `MiningList` (32 bytes), **embedded** at +0x98.
    pub const GATHER_FROM: usize = 152; // 0x98
    /// `MiningList::mtn` (`gather_from + 28`).
    pub const MINING_MTN: usize = 180; // 0xB4
    /// `MiningList::cliff` (`gather_from + 29`).
    pub const MINING_CLIFF: usize = 181; // 0xB5
    /// `BuildData::gather` — `GatherPointList` (28 bytes), **embedded** at +0xB8.
    pub const GATHER: usize = 184; // 0xB8
}

/// `WallData::build_masks` bits, all `[measured]` from their consumers.
pub mod mask {
    /// Set while the building is taking fire. `WallData::is_under_attack` (`0x00472420`)
    /// is literally `return build_masks & 0x20;`.
    pub const UNDER_ATTACK: u16 = 0x0020;
    /// Latch used by `Object::take_damage` (`0x00652020`) to gate the
    /// under-construction collapse rule.
    pub const COLLAPSE_ELIGIBLE: u16 = 0x2000;
    /// "Something built on me last frame" — set by `Wall::process` (`0x00640450`) when
    /// `helpers != 0` at the top of the tick, cleared otherwise.
    pub const WORKED_LAST_FRAME: u16 = 0x0400;
    /// "This site has already bumped its owner's `recharging` counter this frame" —
    /// set once per frame by `Wall::do_construct` (`0x006434D0`), cleared by
    /// `Wall::process`.
    pub const HELPER_COUNTED: u16 = 0x0800;
    /// Ejection pending (`Build::process` → `Build::process_ejection` `0x006201E0`).
    pub const EJECTING: u16 = 0x4000;
    /// Ownership/visibility latch consumed by `Build::process`.
    pub const OWNERSHIP_LATCH: u16 = 0x0200;
    /// Repeat-production latch. `Build::do_queue` snapshots this before removing a
    /// completed unit, clears it, then calls `Build::action_queue`; a failed repeat sets
    /// it again. `Build::unqueue` also clears it whenever the logical queue becomes empty.
    pub const REPEAT_QUEUE: u16 = 0x0040;
}

/// `Object` flag byte at `+0x08`, `[measured]` from the folded accessors.
pub mod flag {
    /// `SubObjectData::is_valid` — the slot holds a live object.
    /// (`Build::close` `0x00628980` reads `flags & 1` first.)
    pub const VALID: u8 = 0x01;
    /// `WallData::is_started` (`0x00472360`) — `flags & 2`. The site exists on the map.
    pub const STARTED: u8 = 0x02;
    /// `WallData::is_active` (`0x00472350`) — `flags & 4`. Construction is **finished**.
    pub const ACTIVE: u8 = 0x04;
    /// "Took damage since the last update" — set by `Object::take_damage`.
    pub const DAMAGED: u8 = 0x10;
    /// Captured / converted marker, read by `Wall::update_hits` and `Build::close`.
    pub const CAPTURED: u8 = 0x20;
}

// =======================================================================================
// Rules
// =======================================================================================

/// Bytes of the `Constants` value block this module addresses.
const RULES_BYTES: usize = 0xD00;

/// The engine's `Constants` singleton, addressed by **byte offset**, exactly as
/// `crates/don-rules` stores it.
///
/// `[0x00C061F0]` is `GameAccess::constants : Constants&` and `[0x00C061E4]` is
/// `GameAccessConst::constantsc` — the same object; the decompiler shows both and they
/// index identically.
#[derive(Clone, Debug)]
pub struct ProdRules {
    raw: [i32; RULES_BYTES / 4],
}

/// Byte offsets of every rule this module reads, `[measured]` from
/// `docs/derivation/rules-constants.json`, which records the loader call site and the
/// `rules.xml` text for each.
pub mod rule {
    /// `unit_rate_base` — `6/5` scaled by 100 → **120**. The base scale on train/build time.
    pub const UNIT_RATE_BASE: usize = 0x220;
    /// `unit_rate_progression` — `3/4` scaled by 100 → **75**. Per-unit time ramp slope.
    pub const UNIT_RATE_PROGRESSION: usize = 0x224;
    /// `accel_train` — **100**. Queue progress per frame for a unit.
    pub const ACCEL_TRAIN: usize = 0x228;
    /// `accel_construct` — **100**. Queue progress per frame for a building, *and* the
    /// per-worker construction rate in `Unit::do_build`.
    pub const ACCEL_CONSTRUCT: usize = 0x22C;
    /// `accel_research` — **100**. Queue progress per frame for a tech.
    pub const ACCEL_RESEARCH: usize = 0x230;
    /// `build_cost_factor` — **10**.
    pub const BUILD_COST_FACTOR: usize = 0x358;
    /// `tech_science_discount` — **10** percent per age.
    pub const TECH_SCIENCE_DISCOUNT: usize = 0x364;
    /// `tech_science_speedup` — **10** percent per age.
    pub const TECH_SCIENCE_SPEEDUP: usize = 0x368;
    /// `build_support_factor` — **1**.
    pub const BUILD_SUPPORT_FACTOR: usize = 0x37C;
    /// `unit_scholar_ramp_max` — **2000** %.
    pub const UNIT_SCHOLAR_RAMP_MAX: usize = 0x394;
    /// `unit_worker_ramp_max` — **500** %.
    pub const UNIT_WORKER_RAMP_MAX: usize = 0x398;
    /// `unit_other_civilian_ramp_max` — **200** %.
    pub const UNIT_OTHER_CIVILIAN_RAMP_MAX: usize = 0x39C;
    /// `unit_military_ramp_max` — **125** %.
    pub const UNIT_MILITARY_RAMP_MAX: usize = 0x3A0;
    /// `ramp_final` — **50**. Research-cost ramp slope, per tech in progress.
    pub const RAMP_FINAL: usize = 0x3AC;
    /// `capital_build_time` — **300** % (nomad games).
    pub const CAPITAL_BUILD_TIME: usize = 0x3BC;
    /// `hanging_gardens_build_time` — **0** % faster.
    pub const HANGING_GARDENS_BUILD_TIME: usize = 0x44C;
    /// `versailles_building_speed` — **0** % faster.
    pub const VERSAILLES_BUILDING_SPEED: usize = 0x4E4;
    /// `maya_building_speed` — **20** % bonus.
    pub const MAYA_BUILDING_SPEED: usize = 0x590;
    /// `roman_fort_speed` — **50** % faster.
    pub const ROMAN_FORT_SPEED: usize = 0x610;
    /// `british_aa_speed` — **33** % faster creation.
    pub const BRITISH_AA_SPEED: usize = 0x6E4;
    /// `korean_build_under_fire` — **1**. Non-zero cancels the under-attack `/4`.
    pub const KOREAN_BUILD_UNDER_FIRE: usize = 0x7DC;
    /// `korean_repair` — **50**.
    pub const KOREAN_REPAIR: usize = 0x7E0;
    /// `dutch_fort_speed` — **0** % faster.
    pub const DUTCH_FORT_SPEED: usize = 0x8B4;
    /// `tobacco_building_speed` — **10** %.
    pub const TOBACCO_BUILDING_SPEED: usize = 0x90C;
    /// `maize_ramping_bonus` — **50** %. Cuts the ramped *cost*.
    pub const MAIZE_RAMPING_BONUS: usize = 0x98C;
    /// `thepresident_unit_build_speed` — **100** %.
    pub const THEPRESIDENT_UNIT_BUILD_SPEED: usize = 0xC84;
    /// `thepresident_building_speed` — **33** %.
    pub const THEPRESIDENT_BUILDING_SPEED: usize = 0xC90;
}

/// The shipped values for every rule this module reads, `[measured]` from
/// `docs/derivation/rules-constants.json` (`stored` field, i.e. post-tokenizer).
pub const SHIPPED_PRODUCTION_RULES: &[(usize, &str, i32)] = &[
    (rule::UNIT_RATE_BASE, "unit_rate_base", 120),
    (rule::UNIT_RATE_PROGRESSION, "unit_rate_progression", 75),
    (rule::ACCEL_TRAIN, "accel_train", 100),
    (rule::ACCEL_CONSTRUCT, "accel_construct", 100),
    (rule::ACCEL_RESEARCH, "accel_research", 100),
    (rule::BUILD_COST_FACTOR, "build_cost_factor", 10),
    (rule::TECH_SCIENCE_DISCOUNT, "tech_science_discount", 10),
    (rule::TECH_SCIENCE_SPEEDUP, "tech_science_speedup", 10),
    (rule::BUILD_SUPPORT_FACTOR, "build_support_factor", 1),
    (rule::UNIT_SCHOLAR_RAMP_MAX, "unit_scholar_ramp_max", 2000),
    (rule::UNIT_WORKER_RAMP_MAX, "unit_worker_ramp_max", 500),
    (
        rule::UNIT_OTHER_CIVILIAN_RAMP_MAX,
        "unit_other_civilian_ramp_max",
        200,
    ),
    (rule::UNIT_MILITARY_RAMP_MAX, "unit_military_ramp_max", 125),
    (rule::RAMP_FINAL, "ramp_final", 50),
    (rule::CAPITAL_BUILD_TIME, "capital_build_time", 300),
    (
        rule::HANGING_GARDENS_BUILD_TIME,
        "hanging_gardens_build_time",
        0,
    ),
    (
        rule::VERSAILLES_BUILDING_SPEED,
        "versailles_building_speed",
        0,
    ),
    (rule::MAYA_BUILDING_SPEED, "maya_building_speed", 20),
    (rule::ROMAN_FORT_SPEED, "roman_fort_speed", 50),
    (rule::BRITISH_AA_SPEED, "british_aa_speed", 33),
    (rule::KOREAN_BUILD_UNDER_FIRE, "korean_build_under_fire", 1),
    (rule::KOREAN_REPAIR, "korean_repair", 50),
    (rule::DUTCH_FORT_SPEED, "dutch_fort_speed", 0),
    (rule::TOBACCO_BUILDING_SPEED, "tobacco_building_speed", 10),
    (rule::MAIZE_RAMPING_BONUS, "maize_ramping_bonus", 50),
    (
        rule::THEPRESIDENT_UNIT_BUILD_SPEED,
        "thepresident_unit_build_speed",
        100,
    ),
    (
        rule::THEPRESIDENT_BUILDING_SPEED,
        "thepresident_building_speed",
        33,
    ),
];

impl Default for ProdRules {
    fn default() -> Self {
        ProdRules::shipped()
    }
}

impl ProdRules {
    /// All-zero rules. Useful for isolating one constant in a test.
    pub fn zeroed() -> Self {
        ProdRules {
            raw: [0; RULES_BYTES / 4],
        }
    }

    /// The values shipped in `ron-data/rules.xml`, as the engine stores them.
    pub fn shipped() -> Self {
        let mut r = ProdRules::zeroed();
        for &(off, _, v) in SHIPPED_PRODUCTION_RULES {
            r.set(off, v);
        }
        r
    }

    /// Read a rule by byte offset. Panics on a misaligned or out-of-range offset —
    /// a wrong offset is a derivation bug, never something to paper over.
    #[inline]
    pub fn get(&self, byte_off: usize) -> i32 {
        assert!(byte_off % 4 == 0, "rule offset {byte_off:#x} is misaligned");
        self.raw[byte_off / 4]
    }

    /// Write a rule by byte offset.
    #[inline]
    pub fn set(&mut self, byte_off: usize, v: i32) {
        assert!(byte_off % 4 == 0, "rule offset {byte_off:#x} is misaligned");
        self.raw[byte_off / 4] = v;
    }
}

// =======================================================================================
// Integer primitives -- the engine's exact division idioms
// =======================================================================================

/// `x / 100` as MSVC emits it: `imul 0x51EB851F; sar edx,5; add (edx>>31)`.
///
/// This is C signed division, i.e. truncation toward zero, which is what Rust's `/` does.
/// It is named because the *magic* is what appears in the disassembly and mis-reading
/// `sar edx,5` as `sar edx,6` turns a `/100` into a `/200`.
#[inline]
pub fn div100(x: i32) -> i32 {
    x.wrapping_div(100)
}

/// `x / 4` as MSVC emits it for a signed value: `cdq; and edx,3; add eax,edx; sar eax,2`.
/// Truncation toward zero, identical to Rust `/ 4`.
#[inline]
pub fn div4_trunc(x: i32) -> i32 {
    x.wrapping_div(4)
}

/// `pct_up(v, p) = v * 100 / (p + 100)` — the engine's "N % faster" idiom.
///
/// Appears 14 times in `ObjectData::train_time` alone. Note it is *not* symmetric with
/// [`pct_down`]: "33% faster" divides by 1.33, it does not multiply by 0.67.
#[inline]
pub fn pct_up(v: i32, p: i32) -> i32 {
    let d = p.wrapping_add(100);
    if d == 0 {
        return 0;
    }
    v.wrapping_mul(100).wrapping_div(d)
}

/// `pct_down(v, p) = (100 - p) * v / 100` — the engine's "N % cheaper/shorter" idiom.
#[inline]
pub fn pct_down(v: i32, p: i32) -> i32 {
    div100(100i32.wrapping_sub(p).wrapping_mul(v))
}

/// `pct_scale(v, p) = (p + 100) * v / 100` — the engine's "N % more" idiom
/// (`Wall::update_hits`).
#[inline]
pub fn pct_scale(v: i32, p: i32) -> i32 {
    div100(p.wrapping_add(100).wrapping_mul(v))
}

// =======================================================================================
// Placement: footprint and coordinate geometry
// =======================================================================================

/// A building type's footprint, from `ObjectTypeData::x_size` (`+0x234`) and `y_size`
/// (`+0x238`) [measured, PDB].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Footprint {
    /// `ObjectTypeData::x_size`, in tiles.
    pub x_size: i32,
    /// `ObjectTypeData::y_size`, in tiles.
    pub y_size: i32,
}

impl Footprint {
    /// `BuildTypeData::area` (`0x006389B0`) — **14 bytes, exactly**
    /// `mov eax,[ecx+0x238]; imul eax,[ecx+0x234]; ret` [measured, disassembled].
    ///
    /// Note the operand order: `y_size * x_size`. It matters only for overflow, and it
    /// cannot overflow at real footprint sizes, but the port matches the instruction.
    #[inline]
    pub fn area(&self) -> i32 {
        self.y_size.wrapping_mul(self.x_size)
    }

    /// `BuildTypeData::corner_tile` (`0x006364C0`) — tile corner → building **centre**
    /// `Coord` [measured]:
    ///
    /// ```text
    /// *cx = (x_size + tx * 2) * 0x60
    /// *cy = (y_size + ty * 2) * 0x60
    /// ```
    ///
    /// `tx * 2 * 0x60` is `tx * 192`, the tile origin; the extra `x_size * 0x60` is half
    /// the footprint, so the result is the centre of a footprint whose lowest tile is
    /// `(tx, ty)`.
    #[inline]
    pub fn corner_tile(&self, tx: i32, ty: i32) -> (i32, i32) {
        (
            self.x_size
                .wrapping_add(tx.wrapping_mul(2))
                .wrapping_mul(COORD_HALF_TILE),
            self.y_size
                .wrapping_add(ty.wrapping_mul(2))
                .wrapping_mul(COORD_HALF_TILE),
        )
    }

    /// `BuildTypeData::tile_corner` (`0x00636440`) — centre `Coord` → lowest tile
    /// [measured]. The inverse of [`Footprint::corner_tile`], but it is *not* algebraically
    /// exact: it snaps through the tile grid, adding a half tile first when the footprint
    /// dimension is odd.
    ///
    /// ```text
    /// ix  = tile_of(cx) * 0xC0                 ; snap down to the tile origin
    /// ix += 0x60 if (x_size & 1)               ; odd footprints sit on a tile centre
    /// *tx = tile_of(ix) - (x_size >> 1)
    /// ```
    #[inline]
    pub fn tile_corner(&self, cx: i32, cy: i32) -> (i32, i32) {
        let mut ix = tile_of(cx).wrapping_mul(COORD_PER_TILE);
        if self.x_size & 1 != 0 {
            ix = ix.wrapping_add(COORD_HALF_TILE);
        }
        let mut iy = tile_of(cy).wrapping_mul(COORD_PER_TILE);
        if self.y_size & 1 != 0 {
            iy = iy.wrapping_add(COORD_HALF_TILE);
        }
        (
            tile_of(ix).wrapping_sub(self.x_size >> 1),
            tile_of(iy).wrapping_sub(self.y_size >> 1),
        )
    }

    /// Every tile the footprint occupies, given its lowest tile. Row-major, `y` outer —
    /// the order `BuildType::mask_me` (`0x006312A0`) stamps the build mask in.
    pub fn tiles(&self, tx: i32, ty: i32) -> Vec<(i32, i32)> {
        let mut out = Vec::with_capacity((self.area().max(0)) as usize);
        for dy in 0..self.y_size {
            for dx in 0..self.x_size {
                out.push((tx + dx, ty + dy));
            }
        }
        out
    }
}

/// `Coord` → `TCoord`. The engine does it through a table at `[0x00CAE5FC]` indexed by
/// `coord >> 6`, whose entries are `i / 3` — i.e. `coord / 192` without a divide
/// [measured, `BuildTypeData::tile_corner` `0x00636440`, `Build::process` `0x0061EDF0`].
///
/// Reproduced as the same two steps rather than as `c / 192`, because the arithmetic
/// shift and the truncating divide disagree for negative `c` and the table's behaviour
/// below zero is **not** established. Coordinates below zero are off-map; if one ever
/// reaches here, that is the bug, not this function.
#[inline]
pub fn tile_of(c: i32) -> i32 {
    (c >> 6) / 3
}

/// The verdicts `BuildTypeData::blocked_site` (`0x00636A50`) returns, as consumed by
/// `Wall::do_construct` (`0x006434D0`) [measured, from the switch in `do_construct`].
///
/// `do_construct` treats `0, 0x27, 0x28, 0x29, 0x2B` as "keep going, start the site" and
/// `0x2A` as "keep going **only** if the linked city has wonder capacity"; every other
/// value disbands the site. The exact condition is
/// `city >= 0 && CityData::num_wonders(1) <= 1 + LeaderData::has_tribe_bonus(7)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SiteVerdict {
    /// `0, 0x27, 0x28, 0x29, 0x2B` — placement stands.
    Ok,
    /// `0x2A` — stands only if the linked city can take another wonder.
    OkIfLinkedCityHasWonderCapacity,
    /// Anything else — the site is disbanded on the next construction tick.
    Blocked,
}

impl SiteVerdict {
    /// Classify a raw `blocked_site` return code.
    #[inline]
    pub fn from_code(code: i32) -> SiteVerdict {
        match code {
            0 | 0x27 | 0x28 | 0x29 | 0x2B => SiteVerdict::Ok,
            0x2A => SiteVerdict::OkIfLinkedCityHasWonderCapacity,
            _ => SiteVerdict::Blocked,
        }
    }
}

// =======================================================================================
// The build queue
// =======================================================================================

/// One production-queue record.
///
/// The 20-byte layout is `[measured]` by triangulating four functions that index it with
/// a stride of `0x14`: `Build::do_queue` (`0x0061E410`, `+0x00` and `+0x04`),
/// `BuildData::count_queue` (`0x0062DEE0`, `+0x04`), `Build::unpay_cost` (`0x006206E0`,
/// `+0x06/08/0A` and `+0x0C/0E/10`) and `Build::refund_cost` (`0x00620490`, the same six).
///
/// ```text
/// +0x00  i32  elapsed        progress, compared against ObjectData::train_time
/// +0x04  i16  type           TypeIndex being produced
/// +0x06  i16  res[0]         resource index paid from, or -1
/// +0x08  i16  res[1]
/// +0x0A  i16  res[2]
/// +0x0C  i16  amt[0]         amount taken from res[0]
/// +0x0E  i16  amt[1]
/// +0x10  i16  amt[2]
/// +0x12  i16  (never checksummed -- BuildQueue::walk_data stops at +0x12)
/// ```
///
/// Only **three** resources can be charged per queued item, even though `TypeData::costs`
/// is `int[6]`. That is a hard cap in the record layout, not a rule.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BuildQueueEntry {
    /// `+0x00` — frames of progress accumulated.
    pub elapsed: i32,
    /// `+0x04` — the `TypeIndex` being produced. `-1` when the slot is empty.
    pub type_index: i16,
    /// `+0x06/08/0A` — resource indices charged, `-1` for unused.
    pub res: [i16; 3],
    /// `+0x0C/0E/10` — amounts charged, parallel to [`BuildQueueEntry::res`].
    pub amt: [i16; 3],
    /// `+0x12` — the trailing short. Not walked; carried so the image round-trips.
    pub tail: i16,
}

impl BuildQueueEntry {
    /// `sizeof` the record: `0x14`.
    pub const SIZE: usize = 0x14;
    /// Bytes of each record that reach the checksum: `0x12`.
    ///
    /// `BuildQueue::walk_data` (`0x006305F0`) emits `w.walk(base + i*0x14, base + i*0x14 + 0x12)`
    /// [measured]. The final short is invisible to the lockstep hash.
    pub const WALKED_BYTES: usize = 0x12;

    /// The record's 20 little-endian bytes, at the measured offsets.
    pub fn image(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        b[0x00..0x04].copy_from_slice(&self.elapsed.to_le_bytes());
        b[0x04..0x06].copy_from_slice(&self.type_index.to_le_bytes());
        for i in 0..3 {
            b[0x06 + i * 2..0x08 + i * 2].copy_from_slice(&self.res[i].to_le_bytes());
            b[0x0C + i * 2..0x0E + i * 2].copy_from_slice(&self.amt[i].to_le_bytes());
        }
        b[0x12..0x14].copy_from_slice(&self.tail.to_le_bytes());
        b
    }
}

/// `BuildQueue`, `BuildData + 0x88` [measured, PDB: 16 bytes, base `BuildQueueOut` 12].
///
/// The engine keeps **two** lengths and they are not the same thing:
/// `BuildQueue::num` (`+0x88`) is the allocated entry count, while `BuildData::queued`
/// (`+0x82`, a `u8`) is how many of them the *game logic* considers occupied. Every
/// accessor guards with `slot < queued` **and** `slot < num`, returning `-1` when the
/// slot is past `num` — see `BuildData::count_queue` (`0x0062DEE0`), which iterates
/// `0..queued` but reads `-1` for any index `>= num`.
#[derive(Clone, Debug, Default)]
pub struct BuildQueue {
    /// `BuildData::queued` (`+0x82`) — logical length, `u8`.
    pub queued: u8,
    /// The entry array. `entries.len()` is `BuildQueue::num` (`+0x88`).
    pub entries: Vec<BuildQueueEntry>,
}

impl BuildQueue {
    /// `BuildQueue::num`.
    #[inline]
    pub fn num(&self) -> usize {
        self.entries.len()
    }

    /// The `TypeIndex` at `slot`, with the engine's exact bounds behaviour:
    /// `slot < num ? entries[slot].type : -1` (`Build::do_queue` `0x0061E410`).
    #[inline]
    pub fn type_at(&self, slot: usize) -> i32 {
        if slot < self.num() {
            self.entries[slot].type_index as i32
        } else {
            -1
        }
    }

    /// The progress at `slot`, same bounds rule.
    #[inline]
    pub fn elapsed_at(&self, slot: usize) -> i32 {
        if slot < self.num() {
            self.entries[slot].elapsed
        } else {
            -1
        }
    }

    /// `BuildData::find_in_queue` (`0x0062DE70`) — first slot in `0..queued` whose type
    /// satisfies `pred`, or `-1`. The engine passes `ObjectTypeData::is(what, 1)` as the
    /// predicate; the type-tree query is the caller's, exactly as `mechanics.rs` treats
    /// object-graph inputs.
    pub fn find_in_queue<F: Fn(i32) -> bool>(&self, pred: F) -> i32 {
        for slot in 0..self.queued as usize {
            if pred(self.type_at(slot)) {
                return slot as i32;
            }
        }
        -1
    }

    /// `BuildData::count_queue(QueueCountIndex, int)` (`0x0062DEE0`) — how many of the
    /// `queued` slots match a mode.
    ///
    /// Mode 1 (`is(what, 1)`) is the only one expressible without the type tree; modes 0
    /// and 2 additionally need "is this a unit type" and "can this player make it",
    /// which are supplied by the caller. All three iterate `0..queued`, reading `-1` for
    /// slots past `num`.
    pub fn count_queue<F: Fn(i32) -> bool>(&self, pred: F) -> i32 {
        let mut n = 0;
        for slot in 0..self.queued as usize {
            if pred(self.type_at(slot)) {
                n += 1;
            }
        }
        n
    }
}

// =======================================================================================
// Construction time
// =======================================================================================

/// The per-player, per-building modifiers `Wall::update_construct_time` (`0x0063D560`)
/// consults. Each is an **input** — resolving it needs the tech tree, the tribe table or
/// the wonder list, none of which live in this module.
#[derive(Clone, Copy, Debug, Default)]
pub struct ConstructTimeGates {
    /// `LeaderData::has_tribe_bonus(1)` — Maya.
    pub maya: bool,
    /// `BuildData::is_wonder()` (vtable `+0x2C`).
    pub is_wonder: bool,
    /// `LeaderData::has_bonus(0x313)`.
    pub bonus_313: bool,
    /// `LeaderData::has_wonder(0x218)` — Versailles.
    pub versailles: bool,
    /// Tobacco rare-resource flag (`leaders[who] + 0x6DA7 & 0x20`, or the `+0x6DCF` alias).
    pub tobacco: bool,
    /// `LeaderData::has_tribe_bonus(0xB)` — British, and the type is `0x20B` (AA gun).
    pub british_aa: bool,
    /// `LeaderData::has_tribe_bonus(0x16)` — Dutch, and the type is a fort.
    pub dutch_fort: bool,
    /// `LeaderData::has_tribe_bonus(6)` — Roman, and the type is a fort.
    pub roman_fort: bool,
    /// `leaders[who] + 0x788 == 0`, the nomad-capital gate on `capital_build_time`.
    pub nomad_capital: bool,
    /// `LeaderData::get_building_speed_upgrade()` (`0x006DAE90`), a level in `0..=10`.
    pub building_speed_upgrade: i32,
}

/// `Wall::update_construct_time` (`0x0063D560`) — recompute and cache
/// `WallData::constr_time` (`+0x50`).
///
/// [measured] chain, in the emitted order (order matters: every step truncates):
///
/// ```text
/// t = build_type->job_time(who)                            ; vt +0x6C
/// if maya && !is_wonder:      t = t*100/(maya_building_speed+100)
/// if bonus(0x313):            t = t*3/4                    ; unsigned shr, not a rule
/// if versailles:              t = t*100/(versailles_building_speed+100)
/// if tobacco:                 t = t*100/(tobacco_building_speed+100)
/// if british && type==0x20B:  t = t*100/(british_aa_speed+100)
/// if dutch && fort && !wond:  t = t*100/(dutch_fort_speed+100)
/// if roman && fort && !wond:  t = t*100/(roman_fort_speed+100)
/// if nomad_capital:           t = capital_build_time * t / 100
/// t = (10 - building_speed_upgrade) * t / 10
/// this->constr_time = t
/// ```
///
/// Note the last line is **not** clamped: a `building_speed_upgrade` of 10 yields zero,
/// and the floor of 1 arrives later, in [`construct_time`].
pub fn update_construct_time(base_job_time: u32, g: &ConstructTimeGates, r: &ProdRules) -> u32 {
    let mut t = base_job_time as i32;
    if g.maya && !g.is_wonder {
        t = pct_up(t, r.get(rule::MAYA_BUILDING_SPEED));
    }
    if g.bonus_313 {
        // `uVar1 * 3 >> 2` on an unsigned -- a logical shift, so no round-to-zero fixup.
        t = ((t as u32).wrapping_mul(3) >> 2) as i32;
    }
    if g.versailles {
        t = pct_up(t, r.get(rule::VERSAILLES_BUILDING_SPEED));
    }
    if g.tobacco {
        t = pct_up(t, r.get(rule::TOBACCO_BUILDING_SPEED));
    }
    if g.british_aa {
        t = pct_up(t, r.get(rule::BRITISH_AA_SPEED));
    }
    if g.dutch_fort && !g.is_wonder {
        t = pct_up(t, r.get(rule::DUTCH_FORT_SPEED));
    }
    if g.roman_fort && !g.is_wonder {
        t = pct_up(t, r.get(rule::ROMAN_FORT_SPEED));
    }
    if g.nomad_capital {
        t = div100(r.get(rule::CAPITAL_BUILD_TIME).wrapping_mul(t));
    }
    t = 10i32
        .wrapping_sub(g.building_speed_upgrade)
        .wrapping_mul(t)
        .wrapping_div(10);
    t as u32
}

/// Gates for `BuildData::construct_time` (`0x0062D5C0`), the *query* that sits on top of
/// the cached [`update_construct_time`] value.
#[derive(Clone, Copy, Debug, Default)]
pub struct ConstructQueryGates {
    /// `BuildData::is_wonder()`.
    pub is_wonder: bool,
    /// The wonder-uniqueness scan found **no** other player holding this type. The engine
    /// scans every other active player's build band from index 2000 for an object of the
    /// same type; if the scan completes without a hit the time collapses to 1
    /// [measured, `0x0062D6xx`]. Gated behind `has_tribe_bonus(0x14)` and two type
    /// exclusions (`0x21D`, `0x21E`).
    pub wonder_unique_instant: bool,
    /// The owning city holds the Hanging Gardens (type `0x210`).
    pub hanging_gardens: bool,
    /// `LeaderData::has_bonus(0x163)` — The President.
    pub president: bool,
    /// `has_tribe_bonus(0x12)` and the type is `0x1B6` and the player owns none yet.
    pub free_first: bool,
}

/// `BuildData::construct_time(int skip_modifiers)` (`0x0062D5C0`, build.cpp:762).
///
/// ```text
/// t = this->constr_time                                     ; +0x50
/// if wonder && unique-scan found nothing:  t = 1
/// if skip_modifiers: goto end
/// if hanging_gardens:  t = (100 - hanging_gardens_build_time) * t / 100
/// if president:        t = t*100/(thepresident_building_speed+100)
/// if free_first:       t = 0
/// end:
/// return max(1, t)                                           ; <- the floor lives HERE
/// ```
///
/// `skip_modifiers` is the argument every hot caller passes as `0`; `Build::process` and
/// `Wall::update_hits` both call `construct_time(0)`.
pub fn construct_time(
    cached_constr_time: u32,
    skip_modifiers: bool,
    g: &ConstructQueryGates,
    r: &ProdRules,
) -> u32 {
    let mut t = cached_constr_time as i32;
    if g.is_wonder && g.wonder_unique_instant {
        t = 1;
    }
    if !skip_modifiers {
        if g.hanging_gardens {
            t = pct_down(t, r.get(rule::HANGING_GARDENS_BUILD_TIME));
        }
        if g.president {
            t = pct_up(t, r.get(rule::THEPRESIDENT_BUILDING_SPEED));
        }
        if g.free_first {
            t = 0;
        }
    }
    if t > 1 {
        t as u32
    } else {
        1
    }
}

// =======================================================================================
// Construction progress
// =======================================================================================

/// `Unit::do_build` (`0x005EEBF0`) — the construction rate one worker contributes this
/// frame, before `Wall::do_construct` divides it among helpers.
///
/// [measured, instruction stream `0x005EEEE3`–`0x005EEF75`]:
///
/// ```text
/// 005eeee3  mov eax,[0x00C061F0]          ; Constants
/// 005eeeea  mov eax,[eax+0x22c]           ; accel_construct
/// 005eeef0  mov [ebp-8],eax
/// 005eef03  call LeaderData::has_tribe_bonus(0x10)
/// 005eef11  cmp dword [Constants+0x7dc],0 ; korean_build_under_fire
/// 005eef18  jne skip
/// 005eef42  movsx eax, word [wall+0x60]   ; build_masks
/// 005eef46  and  eax, 0x20                ; UNDER_ATTACK
/// 005eef4b  je   skip
/// 005eef4d  cdq / and edx,3 / add eax,edx / sar eax,2   ; rate /= 4  (toward zero)
/// 005eef75  call Wall::do_construct(rate)
/// ```
///
/// So the penalty is a **quartering of the rate**, and the Korean tribe bonus
/// (`has_tribe_bonus(0x10)`, paired with the non-zero `korean_build_under_fire` rule)
/// removes it entirely. The bonus is checked *first* and short-circuits the mask read.
#[inline]
pub fn construct_rate(under_attack: bool, korean_bonus: bool, r: &ProdRules) -> i32 {
    let rate = r.get(rule::ACCEL_CONSTRUCT);
    if korean_bonus && r.get(rule::KOREAN_BUILD_UNDER_FIRE) != 0 {
        return rate;
    }
    if under_attack {
        return div4_trunc(rate);
    }
    rate
}

/// What one `Wall::do_construct` call did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConstructStep {
    /// Work actually applied, after the helper division and the floor of 1.
    pub applied: i32,
    /// `job_counter` after the step.
    pub job_counter: u32,
    /// `job_counter_2` after the step.
    pub job_counter_2: u32,
    /// `helpers` after the step.
    pub helpers: u8,
    /// True when `construct_time <= job_counter`, i.e. `Build::activate(0,1,1)` fires.
    pub completed: bool,
}

/// `Wall::do_construct(int rate)` (`0x006434D0`, wall.cpp) — apply one worker's frame of
/// construction. **This is the core production mechanic.**
///
/// [measured] arithmetic, in emitted order:
///
/// ```text
/// if (ai_speed > 1) rate *= ai_speed;         ; GameAccess::ai_speed [0x00C061C0]
/// if (!is_started()) { ...blocked_site check, Build::start(1) then CONTINUE,
///                      or Object::disband(1) and stop... }
/// if (is_active()) return 0;                  ; already finished, nothing to do
/// rate /= helpers + 1;                        ; <-- DIMINISHING RETURNS, before the bump
/// if (!(build_masks & 0x800)) { BuildData::recharging += 1; build_masks |= 0x800; }
/// helpers += 1;                               ; u8, wraps
/// if (rate < 1) rate = 1;                     ; <-- floor AFTER the division
/// job_counter_2 += rate;
/// job_counter   += rate;
/// if (construct_time(0) <= job_counter) { Build::activate(0,1,1); return 1; }
/// return 0;
/// ```
///
/// Three details that a plausible reimplementation gets wrong:
///
/// - The division is by `helpers + 1` **before** the increment, so the first worker of the
///   frame divides by 1, the second by 2, the third by 3. Total work in a frame with *n*
///   builders is `rate · H_n`, not `rate · n`.
/// - The floor of 1 is applied **after** the division, so a fourth builder on a rate-100
///   site still adds 25, and a 100th builder adds 1 rather than 0. Construction never
///   stalls from over-crowding, it only stops speeding up.
/// - `job_counter_2` is bumped by the same amount but is never consulted by this
///   completion test. Only `job_counter` gates completion and feeds [`construct_hits`].
///   `Wall::activate` resets **both** counters after this leaf returns completed.
pub fn do_construct(
    rate: i32,
    ai_speed: i32,
    is_active: bool,
    job_counter: u32,
    job_counter_2: u32,
    helpers: u8,
    construct_time_now: u32,
) -> ConstructStep {
    let mut rate = rate;
    if ai_speed > 1 {
        rate = rate.wrapping_mul(ai_speed);
    }
    if is_active {
        return ConstructStep {
            applied: 0,
            job_counter,
            job_counter_2,
            helpers,
            completed: false,
        };
    }
    rate = rate.wrapping_div(helpers as i32 + 1);
    let helpers = helpers.wrapping_add(1);
    if rate < 1 {
        rate = 1;
    }
    let job_counter_2 = job_counter_2.wrapping_add(rate as u32);
    let job_counter = job_counter.wrapping_add(rate as u32);
    ConstructStep {
        applied: rate,
        job_counter,
        job_counter_2,
        helpers,
        completed: construct_time_now <= job_counter,
    }
}

/// Run a whole frame's worth of builders over one site, in the order the object loop
/// visits them.
///
/// Provided because the per-worker call is easy to compose incorrectly: `helpers` must
/// thread through the sequence, and it is **not** reset between workers, only between
/// frames (`Wall::process` `0x00640450` zeroes it at the top of the tick). The order of
/// `workers` is therefore load-bearing — it is `Objects::process_all`'s rotated owner-slot
/// order, which this module does not own.
pub fn construct_frame(
    rates: &[i32],
    ai_speed: i32,
    job_counter: u32,
    job_counter_2: u32,
    construct_time_now: u32,
) -> ConstructStep {
    let mut st = ConstructStep {
        applied: 0,
        job_counter,
        job_counter_2,
        helpers: 0,
        completed: false,
    };
    let mut total = 0;
    for &rate in rates {
        let next = do_construct(
            rate,
            ai_speed,
            st.completed,
            st.job_counter,
            st.job_counter_2,
            st.helpers,
            construct_time_now,
        );
        total += next.applied;
        st = next;
    }
    st.applied = total;
    st
}

// =======================================================================================
// Under-construction hit points and collapse
// =======================================================================================

/// `Wall::update_hits` (`0x0063F0D0`, wall.cpp:1951) — the tail that turns full hit points
/// into the *effective* hit points of a site still under construction.
///
/// [measured]:
///
/// ```text
/// this->myhits = h;                             ; +0x20, the FULL value
/// if (!is_active()) {
///     jc = job_counter;  ct = construct_time(0);
///     if (jc < ct) {
///         a = jc >> 5;  if (a == 0) a = 1;      ; UNSIGNED shift
///         b = ct >> 5;  if (b == 0) b = 1;
///         if (!is_wonder) { h = a*h / b;            if (h < 1) h = 1; }
///         else            { t = (h/2)*a / b; if (t < 1) t = 1; h = (h+1)/2 + t; }
///     }
/// }
/// this->construct_hits = h;                     ; +0x54
/// ```
///
/// The `>> 5` on **both** sides is the part that will silently diverge: it is a
/// precision reduction to keep `a*h` inside 32 bits, and it quantises progress into
/// 32-unit steps. Computing `h * jc / ct` in full precision gives different integers for
/// almost every input. Each side is floored at 1 *after* the shift, so a site with
/// `job_counter < 32` still gets `1/b` of its hit points rather than zero.
///
/// Wonders keep **half** their hit points from the instant the foundation is laid:
/// `(h+1)/2` unconditionally, plus a scaled share of the other half.
pub fn construct_hits(full_hits: i32, is_active: bool, is_wonder: bool, jc: u32, ct: u32) -> i32 {
    let mut h = full_hits;
    if !is_active && jc < ct {
        let a = {
            let v = jc >> 5;
            if v == 0 {
                1
            } else {
                v
            }
        } as i32;
        let b = {
            let v = ct >> 5;
            if v == 0 {
                1
            } else {
                v
            }
        } as i32;
        if !is_wonder {
            h = a.wrapping_mul(h).wrapping_div(b);
            if h < 1 {
                h = 1;
            }
        } else {
            let mut t = (h / 2).wrapping_mul(a).wrapping_div(b);
            if t < 1 {
                t = 1;
            }
            h = (h.wrapping_add(1) / 2).wrapping_add(t);
        }
    }
    h
}

/// `Object::take_damage` (`0x00652020`, object.cpp:6628) — the **under-construction
/// secondary damage rule**.
///
/// After the primary death test (`hits <= damage`) has *not* fired, the engine runs a
/// second, construction-only test [measured, `0x006520xx`, the `damage < local_c` arm]:
///
/// ```text
/// if (is_valid_build()
///     && !is_active()                      ; still under construction
///     && !(leader_flags & 4)
///     && hits(0) <= damage * 2             ; damage has reached HALF of effective HP
///     && (wall->build_masks & 0x2000))
/// {
///     Object::disband(0);                  ; the site collapses outright
///     return 1;
/// }
/// ```
///
/// So a construction site does **not** have to be reduced to zero: once cumulative damage
/// reaches half of its *current, construction-scaled* hit points, it is removed. Combined
/// with [`construct_hits`], a barely-started building is destroyed by a single hit —
/// `construct_hits` is small, and half of small is smaller.
///
/// `hits` here is `BuildData::hits(0)` (vtable `+0x11C`), i.e. `construct_hits`, **not**
/// `myhits` — verified against `BuildData::hits` (`0x0062E740`), whose `param == 0` arm
/// returns `this[0x15]` = `+0x54`.
#[inline]
pub fn under_construction_collapses(
    is_valid_build: bool,
    is_active: bool,
    leader_flag_4: bool,
    construct_hits_now: i32,
    damage: i32,
    build_masks: u16,
) -> bool {
    is_valid_build
        && !is_active
        && !leader_flag_4
        && construct_hits_now <= damage.wrapping_mul(2)
        && (build_masks & mask::COLLAPSE_ELIGIBLE) != 0
}

/// `BuildData::hits(int full)` (`0x0062E740`, build.cpp:103).
///
/// ```text
/// h = full ? myhits : construct_hits
/// if (is_active() && queued && queue.num > 0 && queue[0].type in {0x29A, 0x286}) {
///     prog  = queue[0].elapsed
///     total = ObjectData::train_time(queue[0].type)
///     h = (int)( (float)(total - prog) * (float)h / (float)total )
///     if (h < 1) h = 1
/// }
/// return h
/// ```
///
/// **This is one of the few floats inside walked state.** Types `0x29A` and `0x286` are
/// the razing/disband pseudo-items: while a building is being demolished its hit points
/// fall linearly to 1, and the interpolation is done in `f32` with a truncating cast, not
/// in integers. `README-LLM`'s "the sim is INTEGERS" has an exception here; it belongs on
/// the same list as `LeaderData::anti_att` and `Unit::move_step`.
///
/// The float is reproduced literally (`f32` multiply/divide, `as i32` truncation) rather
/// than "cleaned up", because cleaning it up changes results.
pub fn build_hits(
    full: bool,
    myhits: i32,
    construct_hits_cached: i32,
    razing: Option<(i32, i32)>,
) -> i32 {
    let h = if full { myhits } else { construct_hits_cached };
    match razing {
        Some((elapsed, total)) if total != 0 => {
            let v = ((total - elapsed) as f32 * h as f32 / total as f32) as i32;
            if v < 1 {
                1
            } else {
                v
            }
        }
        _ => h,
    }
}

// =======================================================================================
// Repair
// =======================================================================================

/// `Build::repair_damage(int, int, int)` (`0x00628130`, build.cpp:3471).
///
/// Byte-for-byte the same 72-byte body as `Object::repair_damage` (`0x00646F90`) — MSVC
/// emitted both, they were not folded.
///
/// ```text
/// if (hits(0) < this->damage) this->damage = hits(0);   ; clamp a stale over-damage
/// this->damage_frac = 0;                                ; +0x3B, always cleared
/// if (this->damage <= amount) this->damage = 0;
/// else                        this->damage -= amount;
/// ```
///
/// The clamp comes **first**, so repairing a building whose max HP just shrank (a lost
/// upgrade, a captured city) heals it from the new ceiling, not the old one. And
/// `damage_frac` is cleared unconditionally — even a zero-amount repair discards the
/// accumulated sub-integer damage, which is why spamming repair is worth something.
#[inline]
pub fn repair_damage(damage: i32, amount: i32, hits_now: i32) -> (i32, i8) {
    let mut d = damage;
    if hits_now < d {
        d = hits_now;
    }
    if d <= amount {
        (0, 0)
    } else {
        (d - amount, 0)
    }
}

// =======================================================================================
// Training / queue tick
// =======================================================================================

/// Which accelerator `Build::do_queue` picks for the item in a slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueKind {
    /// A building type (`TypeIndex` in `0x19E..=0x21E`), or the special `0x29A`.
    /// Uses `accel_construct`.
    Building,
    /// A unit type the player can currently make. Uses `accel_train`.
    Unit,
    /// Anything else, in practice a technology. Uses `accel_research`.
    Research,
}

impl QueueKind {
    /// The classification `Build::do_queue` (`0x0061E410`) performs [measured]:
    ///
    /// ```text
    /// if (type->is_build() || 0x19D < type_index < 0x21F) -> accel_construct
    /// if (type_index == 0x29A)                            -> accel_construct
    /// if (!type->is_unit() || !player_can_make[type])     -> accel_research
    /// else                                                -> accel_train
    /// ```
    ///
    /// `is_build` / `is_unit` / `can_make` are type-tree queries and stay inputs.
    pub fn classify(type_index: i32, is_build: bool, is_unit: bool, can_make: bool) -> QueueKind {
        if is_build || (0x19D < type_index && type_index < 0x21F) || type_index == 0x29A {
            QueueKind::Building
        } else if is_unit && can_make {
            QueueKind::Unit
        } else {
            QueueKind::Research
        }
    }

    /// The rule offset this kind reads.
    pub fn rule_offset(self) -> usize {
        match self {
            QueueKind::Building => rule::ACCEL_CONSTRUCT,
            QueueKind::Unit => rule::ACCEL_TRAIN,
            QueueKind::Research => rule::ACCEL_RESEARCH,
        }
    }
}

/// Result of one `Build::do_queue` slot advance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueStep {
    /// `elapsed` to write back into the slot.
    pub elapsed: i32,
    /// True when `total <= elapsed`, i.e. the item is finished this frame and
    /// `Build::finished(type)` (`0x00628490`) then `Build::unqueue(slot, 0)`
    /// (`0x006207C0`) run.
    pub done: bool,
    /// The accelerator that was applied, after the `ai_speed` multiply.
    pub rate: i32,
}

/// `Build::do_queue(int slot)` (`0x0061E410`, build.cpp:7839) — advance one queue slot by
/// one frame.
///
/// The retail function is 2,515 bytes, and the great majority of that is the local
/// player's HUD warnings ("not enough resources", "population capped") which are gated on
/// `who == local_player`. Those branches are pure presentation and are deliberately not
/// ported. What remains is the whole of the simulation-visible behaviour [measured]:
///
/// ```text
/// if (this->queued == 0) return;
/// type  = slot < queue.num ? queue[slot].type : -1
/// total = ObjectData::train_time(type)                    ; 0x006508C0
/// rate  = accel_construct | accel_train | accel_research   ; see QueueKind
/// if (ai_speed > 1) rate *= ai_speed
/// prog  = slot < queue.num ? queue[slot].elapsed : -1
/// if (total == 1) prog = 1                                ; instant items finish now
/// done  = total <= prog
/// next  = min(prog + rate, total)
/// if (done) { Build::finished(type); Build::unqueue(slot, 0); }
/// else      { queue[slot].elapsed = next; }
/// ```
///
/// Two behaviours worth naming:
///
/// - `prog` starts at `-1` for a slot past `num`, so an item in an unallocated slot needs
///   one extra frame to reach zero. Real queues keep `num >= queued`, so this only bites
///   a reimplementation that lets the two lengths drift.
/// - `total == 1` forces `prog = 1` and therefore `done`, which is how a free or
///   `construct_time`-collapsed item completes on the frame it is queued rather than one
///   frame later.
///
/// Multi-slot buildings (`type->is(0x1B3)`, e.g. the ones that train several items at
/// once) additionally recurse into `do_queue(slot + 1)` before this arithmetic; the
/// recursion is the caller's, since the "how many parallel slots" query is a type-tree
/// lookup.
pub fn queue_step(
    queued: u8,
    slot: usize,
    queue_num: usize,
    elapsed_in_slot: i32,
    total_train_time: i32,
    kind: QueueKind,
    ai_speed: i32,
    r: &ProdRules,
) -> Option<QueueStep> {
    if queued == 0 {
        return None;
    }
    let mut rate = r.get(kind.rule_offset());
    if ai_speed > 1 {
        rate = rate.wrapping_mul(ai_speed);
    }
    let mut prog = if slot < queue_num {
        elapsed_in_slot
    } else {
        -1
    };
    if total_train_time == 1 {
        prog = 1;
    }
    let done = total_train_time <= prog;
    let next = {
        let sum = prog.wrapping_add(rate);
        if sum < total_train_time {
            sum
        } else {
            total_train_time
        }
    };
    Some(QueueStep {
        elapsed: next,
        done,
        rate,
    })
}

// =======================================================================================
// Executable single-slot queue transaction
// =======================================================================================

/// Mandatory world boundary for one local, non-parallel `Build::do_queue` transaction.
///
/// The queue owns progress and compaction. The host owns type-table queries, the enormous
/// `Build::finished` world mutation, per-player queued counters, and the paid repeat-queue
/// operation. None has a default: silently omitting any one leaves checksum-visible state
/// stale.
pub trait QueueCompletionHost {
    /// `ObjectData::train_time(type)` `0x006508C0`.
    fn train_time(&mut self, type_index: i32) -> i32;
    /// The type-tree / player-availability classification used to select the accelerator.
    fn classify(&mut self, type_index: i32) -> QueueKind;
    /// `Build::finished(type)` `0x00628490`. The queue entry has already been saturated to
    /// its total, exactly as at `0x0061EBC1..0x0061EBCD`. Return false when population,
    /// support, placement, or another world gate prevents completion this frame.
    fn finished(&mut self, type_index: i32, queue: &BuildQueue, slot: usize) -> bool;
    /// The store-dirty write at `0x006208A8..0x006208B1`, before `unqueue` touches the
    /// entry. This is a mandatory callback because its owner is outside `BuildData`.
    fn mark_queue_dirty(&mut self);
    /// Per-player counter mutations inside `Build::unqueue(slot, 0)`, after the removed
    /// entry's elapsed field is zeroed but before logical compaction.
    fn completed_unqueue(&mut self, type_index: i32, queue: &BuildQueue, slot: usize);
    /// `Build::action_queue(type, 0)` `0x00620F40`, reached only for a completed unit when
    /// the pre-unqueue repeat latch was set. The host must perform payment and queue
    /// mutation. Return true on retail's non-zero success result.
    fn repeat_unit(&mut self, build: &mut BuildData, type_index: i32) -> bool;
}

/// Fail-closed violations of the normal retail queue invariant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueTransactionError {
    /// `queued` is a logical prefix of `BuildQueue::num` in reachable retail state.
    LogicalLengthExceedsAllocation { queued: usize, allocated: usize },
    /// `Build::process` starts at slot zero and parallel recursion checks `slot < queued`.
    SlotOutsideLogicalQueue { slot: usize, queued: usize },
    /// A reachable queue record always names a real type; `-1` is only an accessor sentinel.
    InvalidTypeIndex(i32),
}

/// Result of one executable queue-slot transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueTransaction {
    /// Progress was advanced and saturated, but completion is tested next frame.
    Advanced { type_index: i32, step: QueueStep },
    /// Progress was already complete, but `Build::finished` rejected the world mutation.
    FinishBlocked { type_index: i32, step: QueueStep },
    /// `finished` succeeded and local `unqueue(slot, 0)` completed.
    Completed {
        type_index: i32,
        step: QueueStep,
        repeat_attempted: bool,
        repeat_succeeded: bool,
    },
}

/// Execute the ordinary local, single-slot body of `Build::do_queue` (`0x0061E410`) through
/// successful `Build::unqueue(slot, 0)` (`0x006207C0`). [measured]
///
/// This function deliberately starts *after* the two routing decisions that need the
/// owning object graph: a multi-slot producer recurses into `slot + 1` first, and a Library
/// can forward an overflow slot to the player's primary Library. The caller must resolve
/// those two decisions before entering this local transaction.
///
/// The allocated `entries` vector is not shortened on completion. Retail's
/// `BuildQueue::un_queue` memmoves the remaining logical suffix but leaves `num` and the
/// last physical record intact; that stale record is checksum-visible because the queue
/// walker uses allocated length. `Vec::remove` would therefore desynchronise immediately.
pub fn execute_local_queue_slot<H: QueueCompletionHost>(
    build: &mut BuildData,
    slot: usize,
    ai_speed: i32,
    rules: &ProdRules,
    host: &mut H,
) -> Result<Option<QueueTransaction>, QueueTransactionError> {
    let queued = build.queue.queued as usize;
    if queued == 0 {
        return Ok(None);
    }
    let allocated = build.queue.num();
    if queued > allocated {
        return Err(QueueTransactionError::LogicalLengthExceedsAllocation { queued, allocated });
    }
    if slot >= queued {
        return Err(QueueTransactionError::SlotOutsideLogicalQueue { slot, queued });
    }

    let type_index = build.queue.type_at(slot);
    if type_index < 0 {
        return Err(QueueTransactionError::InvalidTypeIndex(type_index));
    }
    let total = host.train_time(type_index);
    let kind = host.classify(type_index);
    let mut step = queue_step(
        build.queue.queued,
        slot,
        allocated,
        build.queue.entries[slot].elapsed,
        total,
        kind,
        ai_speed,
        rules,
    )
    .expect("non-empty queue was checked above");

    if !step.done {
        build.queue.entries[slot].elapsed = step.elapsed;
        return Ok(Some(QueueTransaction::Advanced { type_index, step }));
    }

    // The ordinary branch writes the total (not the wrapped `prog + rate` candidate)
    // before invoking Build::finished; this distinction is observable if stale progress
    // or a modded accelerator overflows.
    step.elapsed = total;
    build.queue.entries[slot].elapsed = step.elapsed;
    if !host.finished(type_index, &build.queue, slot) {
        return Ok(Some(QueueTransaction::FinishBlocked { type_index, step }));
    }

    let repeat_attempted = kind == QueueKind::Unit && (build.build_masks & mask::REPEAT_QUEUE) != 0;

    // Build::unqueue(slot, 0): dirty marker, elapsed reset, external queued counters,
    // physical memmove, logical decrement, and empty-queue latch clear in this order.
    host.mark_queue_dirty();
    build.queue.entries[slot].elapsed = 0;
    host.completed_unqueue(type_index, &build.queue, slot);
    if slot + 1 < queued {
        build.queue.entries.copy_within(slot + 1..queued, slot);
    }
    build.queue.queued -= 1;
    if build.queue.queued == 0 {
        build.build_masks &= !mask::REPEAT_QUEUE;
    }

    let repeat_succeeded = if repeat_attempted {
        build.build_masks &= !mask::REPEAT_QUEUE;
        let succeeded = host.repeat_unit(build, type_index);
        if !succeeded {
            build.build_masks |= mask::REPEAT_QUEUE;
        }
        succeeded
    } else {
        false
    };

    Ok(Some(QueueTransaction::Completed {
        type_index,
        step,
        repeat_attempted,
        repeat_succeeded,
    }))
}

// =======================================================================================
// Train time: the JOB_EXTRA_TIME ramp
// =======================================================================================

/// Inputs to the `JOB_EXTRA_TIME` ramp inside `ObjectData::train_time` (`0x006508C0`).
#[derive(Clone, Copy, Debug, Default)]
pub struct TrainRampInput {
    /// The type's base time, from `Type` vtable `+0x6C` (normal) or `+0x70` (the
    /// "already discovered" variant), called with the owning player.
    pub base: i32,
    /// The player's live count of this type: a `u16` at
    /// `leaders + 0x56FE + (who*0x3776 + type)*2` [measured, `0x00650B17`]. Note the
    /// **per-player stride** `0x3776`, which `docs/derivation/economy.md` §4.2 omits.
    pub count: u16,
    /// `UnitTypeData::job_extra_time`, `UnitType + 0x2E8` [measured, PDB].
    pub job_extra_time: i32,
}

/// The `JOB_EXTRA_TIME` build-time ramp and its cap — `0x00650AB5`..`0x00650B4D`, inside
/// `ObjectData::train_time` (`0x006508C0`, object.cpp:1897).
///
/// ```text
/// base    = base * unit_rate_base / 100          ; 0x00650AF6
/// ceiling = base * 3                             ; 0x00650B2E, lea eax,[ecx+ecx*2]
/// v       = count * job_extra_time * unit_rate_progression + base
///                                                ; 0x00650B27, 0x00650B31, 0x00650B38
/// if (v < 0 || ceiling < 0) v = 0                ; 0x00650B4B
/// else if (v > ceiling)     v = ceiling          ; 0x00650B47
/// ```
///
/// **The cap is 3× the scaled base**, not a rule. With the shipped
/// `unit_rate_base = 120` that is `3.6×` the type's raw time, and it is reached at
/// `count = base * 120 * 2 / (100 * job_extra_time * 75)`.
///
/// This supersedes `mechanics::ramped_rate`, whose doc comment records "**What `x` is has
/// not been established**". It is now established: the enclosing function is
/// `ObjectData::train_time(TypeIndex)`, `x` is the type's base job time from vtable
/// `+0x6C`/`+0x70`, and the whole thing is the **build/train time**, not a gather rate.
/// `mechanics::ramped_rate` computes the same integers and should be retired in favour of
/// this one once the sim-core lane is done with `mechanics.rs`.
///
/// The clamp is exactly as written: a wrapped product collapses to 0 rather than
/// propagating a negative.
#[inline]
pub fn train_time_ramp(i: &TrainRampInput, r: &ProdRules) -> i32 {
    let base = div100(i.base.wrapping_mul(r.get(rule::UNIT_RATE_BASE)));
    let ceiling = base.wrapping_mul(3);
    let ramp = (i.count as i32)
        .wrapping_mul(i.job_extra_time)
        .wrapping_mul(r.get(rule::UNIT_RATE_PROGRESSION));
    let v = ramp.wrapping_add(base);
    if v < 0 || ceiling < 0 {
        0
    } else if v > ceiling {
        ceiling
    } else {
        v
    }
}

/// The final two steps of `ObjectData::train_time` (`0x006508C0`), after every civ,
/// tech, wonder and government modifier.
///
/// ```text
/// ; age-behind penalty  0x0065183x
/// if (type_age < player_age)
///     t += (player_age - type_age) * tech_science_speedup * t / 100
///
/// ; game-speed scale on the DIFFICULTY byte at game+0x2F  0x006517xx
/// if (diff == 0 || diff == 2)              t = t*2/3
/// else if (diff == 4 || 6 || 8)            t = t*3/2
///
/// return max(1, t)
/// ```
///
/// The age term goes the direction that surprises people: a type from an **earlier** age
/// takes *longer*, by 10 % per age elapsed, not shorter.
#[inline]
pub fn train_time_age_penalty(t: i32, player_age: i32, type_age: i32, r: &ProdRules) -> i32 {
    if type_age < player_age {
        let d = player_age.wrapping_sub(type_age);
        t.wrapping_add(div100(
            d.wrapping_mul(r.get(rule::TECH_SCIENCE_SPEEDUP))
                .wrapping_mul(t),
        ))
    } else {
        t
    }
}

/// The difficulty scale and the floor of 1 that close `ObjectData::train_time`.
#[inline]
pub fn train_time_finalize(t: i32, difficulty: u8) -> i32 {
    let t = match difficulty {
        0 | 2 => t.wrapping_mul(2).wrapping_div(3),
        4 | 6 | 8 => t.wrapping_mul(3).wrapping_div(2),
        _ => t,
    };
    if t > 1 {
        t
    } else {
        1
    }
}

// =======================================================================================
// Cost ramping: SUPPORT and PROGRESSION
// =======================================================================================

/// The four ramp-cap classes `TypeData::get_cost` (`0x00664090`) selects between
/// [measured, `0x0066536B`..`0x006656B5`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RampClass {
    /// `TypeData::type` in `{0x34, 0x35}` — scholars. Cap `unit_scholar_ramp_max` (2000 %),
    /// **and** the extra super-linear term in [`scholar_extra`].
    Scholar,
    /// `type` in `{0x32, 0x33, 0x34, 0x35}`, or `TypeData::is_merchant()`, or
    /// `UnitTypeData::unit_flags2 & 8`. Cap `unit_worker_ramp_max` (500 %).
    Worker,
    /// `ObjectTypeData::obj_masks & 4`. Cap `unit_other_civilian_ramp_max` (200 %).
    OtherCivilian,
    /// Everything else. Cap `unit_military_ramp_max` (125 %).
    Military,
}

impl RampClass {
    /// The classification, in the engine's evaluation order — the tests are a
    /// fall-through chain and the first match wins.
    pub fn classify(
        type_id: i32,
        is_merchant: bool,
        unit_flags2: i32,
        obj_masks: i32,
    ) -> RampClass {
        if type_id == 0x34 || type_id == 0x35 {
            RampClass::Scholar
        } else if (0x32..=0x35).contains(&type_id) || is_merchant || (unit_flags2 & 8) != 0 {
            RampClass::Worker
        } else if (obj_masks & 4) != 0 {
            RampClass::OtherCivilian
        } else {
            RampClass::Military
        }
    }

    /// The rule offset holding this class's cap percentage.
    pub fn rule_offset(self) -> usize {
        match self {
            RampClass::Scholar => rule::UNIT_SCHOLAR_RAMP_MAX,
            RampClass::Worker => rule::UNIT_WORKER_RAMP_MAX,
            RampClass::OtherCivilian => rule::UNIT_OTHER_CIVILIAN_RAMP_MAX,
            RampClass::Military => rule::UNIT_MILITARY_RAMP_MAX,
        }
    }
}

/// `UnitTypeData::progression` (`UnitType + 0x2F4`) as a **mode selector**, values 0..3.
///
/// Only one consumer is located: `TypeData::get_cost` (`0x00664090`) at `0x00665355`,
/// `test byte ptr [ecx+0x2f4], 2` — **bit 1** switches the ramp count from linear to
/// triangular:
///
/// ```text
/// 00665355  test byte ptr [ecx+0x2f4], 2
/// 0066535c  je   linear
/// 0066535e  lea  eax,[edi+1]        ; n+1
/// 00665361  imul eax,edi            ; n*(n+1)
/// 00665364  cdq / sub eax,edx       ; round toward zero
/// 00665369  sar  edi,1              ; /2
/// ```
///
/// So `n -> n(n+1)/2`, the triangular number: with the shipped rules a `progression = 2`
/// unit's *k*-th copy costs `support_cost * k(k+1)/2` extra, i.e. quadratic in the count.
///
/// **Bit 0's consumer is not located.** It is not read anywhere in `get_cost` and not in
/// `ObjectData::train_time`. Treated as unknown rather than assumed inert.
#[inline]
pub fn progression_ramp_count(n: i32, progression: i32) -> i32 {
    if progression & 2 != 0 {
        let p = n.wrapping_add(1).wrapping_mul(n);
        // `cdq; sub eax,edx; sar eax,1` -- C's `/2`, i.e. truncation toward zero.
        p.wrapping_div(2)
    } else {
        n
    }
}

/// The scholar-only super-linear term — `0x006656C0`..`0x006656F0`, reached only for
/// `RampClass::Scholar` and only when the ramp count exceeds 7 [measured]:
///
/// ```text
/// ecx = 7; edx = 0;
/// if (n <= 7) extra = 0;
/// else do { edx += n - ecx; ecx += 7; } while (ecx < n);
/// ```
///
/// i.e. `extra = Σ_{k≥1, 7(k-1) < n} (n - 7k)` — an additional cost step every seventh
/// scholar, on top of the 2000 % cap. This is why the eighth university scholar is the
/// one that hurts.
pub fn scholar_extra(n: i32) -> i32 {
    if n <= 7 {
        return 0;
    }
    let mut extra = 0i32;
    let mut k = 7i32;
    loop {
        extra = extra.wrapping_add(n.wrapping_sub(k));
        k = k.wrapping_add(7);
        if k >= n {
            break;
        }
    }
    extra
}

/// A type's SUPPORT pair — `ObjectTypeData::support : TypeIndex[2]` (`+0x268`) and
/// `ObjectTypeData::support_cost : int[2]` (`+0x270`) [measured, PDB].
#[derive(Clone, Copy, Debug, Default)]
pub struct SupportPair {
    /// Resource / support `TypeIndex` charged, `-1` for unused.
    pub support: [i32; 2],
    /// Per-unit-of-count cost against the matching `support`.
    pub support_cost: [i32; 2],
}

/// `TypeData::get_cost` (`0x00664090`, type.cpp:639) — the SUPPORT × PROGRESSION cost
/// ramp for one resource.
///
/// [measured] block, `0x0066528F`..`0x006654B6`:
///
/// ```text
/// n = support_type < 0 ? 0 : LeaderData::get_support_count(support_type)   ; 0x006DA110
/// ; citizen/scholar special cases fold three support counts together (see `count`)
/// if (type->is(0x13B)) n += leaders[who].korean_citizens_bonus            ; +0x7BC
/// if (n <= 0) return 0
/// n = progression_ramp_count(n, unit_type->progression)
/// ceiling = ramp_max_pct * base_cost / 100          ; class-selected, see RampClass
/// extra   = scholars only, see scholar_extra
/// total = 0
/// for i in 0..2:
///     if (support[i] == resource) {
///         v = support_cost[i] * n + extra
///         if (ceiling != 0 && ceiling < v) v = ceiling     ; cmovl, 0x00665452
///         if (maize) v = (100 - maize_ramping_bonus) * v / 100
///         total += v
///     }
/// ```
///
/// **A zero ceiling means no ceiling** (`test esi,esi; je`), which is the opposite of the
/// natural reading and is why the clamp is a named step. The pre-existing
/// `mechanics::clamp_cost_to_ramp_ceiling` already had this right; what it lacked, and
/// what is added here, is where `n` comes from and which class picks the cap.
///
/// `get_support_count` and the citizen/scholar folding stay as the caller's `count`,
/// because they read `LeaderData` this module does not own.
pub fn ramp_cost(
    resource: i32,
    count: i32,
    progression: i32,
    class: RampClass,
    base_cost: i32,
    sup: &SupportPair,
    maize: bool,
    r: &ProdRules,
) -> i32 {
    if count <= 0 {
        return 0;
    }
    let n = progression_ramp_count(count, progression);
    let ceiling = div100(r.get(class.rule_offset()).wrapping_mul(base_cost));
    let extra = if class == RampClass::Scholar {
        scholar_extra(n)
    } else {
        0
    };
    let mut total = 0i32;
    for i in 0..2 {
        if sup.support[i] != resource {
            continue;
        }
        let mut v = sup.support_cost[i].wrapping_mul(n).wrapping_add(extra);
        if ceiling != 0 && ceiling < v {
            v = ceiling;
        }
        if maize {
            v = pct_down(v, r.get(rule::MAIZE_RAMPING_BONUS));
        }
        total = total.wrapping_add(v);
    }
    total
}

/// The research-cost ramp at the tail of `TypeData::get_cost` — `0x006666D0`..`0x00666739`
/// [measured].
///
/// ```text
/// ebx = 0
/// for t in 0..0x247: if (LeaderData::researching(t)) ebx += 8
/// pct  = ramp_final * ebx / 8 + 100          ; == 100 + ramp_final * n
/// cost = pct * base_cost / 100
/// ```
///
/// With the shipped `ramp_final = 50`, every technology already in progress makes the
/// next one **50 % more expensive**, compounding linearly with the count.
#[inline]
pub fn research_ramp_cost(base_cost: i32, techs_in_progress: i32, r: &ProdRules) -> i32 {
    // The engine accumulates `8` per tech and immediately divides by 8; reproduced as the
    // same two steps so a future `researching()` weighting other than 8 stays visible.
    let acc = techs_in_progress.wrapping_mul(8);
    let pct = (r.get(rule::RAMP_FINAL).wrapping_mul(acc) >> 3).wrapping_add(100);
    div100(pct.wrapping_mul(base_cost))
}

// =======================================================================================
// Unqueue: refund and repayment
// =======================================================================================

/// `Build::unpay_cost(int slot)` (`0x006206E0`, build.cpp:6827) — the *plain* refund.
///
/// ```text
/// if (slot >= queued) return;
/// for i in 0..3:
///     res = queue[slot].res[i];
///     if (res >= 0) leaders[who].stockpile[res] += queue[slot].amt[i];
/// ```
///
/// The stockpile is **XOR-obfuscated with `0x8221`** in memory — the engine reads
/// `*p ^ 0x8221`, adds, and writes `result ^ 0x8221` [measured]. That obfuscated form is
/// what the lockstep checksum hashes, so any port that stores plain integers and hashes
/// them desyncs on frame 1. This function returns the deltas and leaves the obfuscation
/// to the economy lane, which owns the stockpile.
pub fn unpay_amounts(entry: &BuildQueueEntry) -> [(i32, i32); 3] {
    let mut out = [(-1, 0); 3];
    for i in 0..3 {
        if entry.res[i] >= 0 {
            out[i] = (entry.res[i] as i32, entry.amt[i] as i32);
        }
    }
    out
}

/// `Build::refund_cost(int slot)` (`0x00620490`, build.cpp:6849) — the **age-adjusted**
/// refund, used when an item is cancelled after the player has advanced an age.
///
/// [measured]:
///
/// ```text
/// d = player_age - type_age                          ; ages elapsed
/// for i in 0..3, res[i] >= 0:
///     adj  = amt[i] * 100 / (100 - tech_science_discount * d)
///     adj += (d + 1) * tech_science_discount * adj / 100
///     stockpile[res[i]] += amt[i] - adj
///     amt[i] = adj                                   ; the record is REWRITTEN
/// ```
///
/// Read that credit line carefully: the player is given `amt - adj`, which is **negative**
/// whenever `adj > amt`. Cancelling a stale queue item can therefore *charge* the player.
/// The engine also writes `adj` back into the queue record, so a second cancel of the same
/// slot compounds — which is only reachable through `Build::action_unqueue`
/// (`0x00620280`) re-entering, and is a real behaviour, not a guard to add.
///
/// `player_age` comes from `leaders[who] + 0x6EB8 + 0xF4`, XOR-obfuscated with `0x63187`;
/// `type_age` from `Type + 0x1C8`.
pub fn refund_amount(amt: i32, ages_elapsed: i32, r: &ProdRules) -> i32 {
    let disc = r.get(rule::TECH_SCIENCE_DISCOUNT);
    let denom = 100i32.wrapping_sub(disc.wrapping_mul(ages_elapsed));
    if denom == 0 {
        return 0;
    }
    let base = amt.wrapping_mul(100).wrapping_div(denom);
    base.wrapping_add(div100(
        ages_elapsed
            .wrapping_add(1)
            .wrapping_mul(disc)
            .wrapping_mul(base),
    ))
}

/// The full `refund_cost` pass over one queue record: returns the stockpile deltas and
/// the rewritten amounts.
pub fn refund_cost(
    entry: &BuildQueueEntry,
    ages_elapsed: i32,
    r: &ProdRules,
    mode: &ModeConfig,
) -> ([(i32, i32); 3], [i16; 3]) {
    let mut deltas = [(-1i32, 0i32); 3];
    let mut new_amts = entry.amt;
    for i in 0..3 {
        if entry.res[i] < 0 {
            continue;
        }
        let adj = refund_amount(entry.amt[i] as i32, ages_elapsed, r);
        let outcome = deviation_behaviour::refund_slot(mode, entry.amt[i] as i32, adj);
        deltas[i] = (entry.res[i] as i32, outcome.credit);
        new_amts[i] = outcome.new_amt as i16;
    }
    (deltas, new_amts)
}

// =======================================================================================
// The building record, and its checksum image
// =======================================================================================

/// One `GatherPoint` (`GatherPointList` node) — 16 bytes, PDB `.?AVGatherPoint@@`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GatherPoint {
    /// `GatherPoint::x` (`+0x04`), a `Coord`.
    pub x: i32,
    /// `GatherPoint::y` (`+0x08`), a `Coord`.
    pub y: i32,
    /// `GatherPoint::action` (`+0x0C`).
    pub action: u8,
    /// The list node's own byte at `node + 0x0C`, walked immediately before the point.
    pub node_tag: u8,
}

/// `MiningList` (`BuildData::gather_from`, `+0x98`, 32 bytes) — `TCoordList` plus two
/// bytes. The tiles this building is currently drawing a resource from.
#[derive(Clone, Debug, Default)]
pub struct MiningList {
    /// The `Array<TCoordData>` payload: packed tile coordinates.
    pub tiles: Vec<u32>,
    /// `MiningList::mtn` (`+0x1C`).
    pub mtn: i8,
    /// `MiningList::cliff` (`+0x1D`).
    pub cliff: i8,
}

/// A building, laid out at the `[measured]` `BuildData` offsets.
///
/// Only the fields the production channel owns are typed; everything else in the 220-byte
/// record is carried as opaque bytes in [`BuildData::other`] so the image round-trips and
/// the checksum stays exact while the object/unit lanes fill in the rest.
#[derive(Clone, Debug)]
pub struct BuildData {
    // -- Object base (owned elsewhere; carried so the walk is complete) -----------------
    /// `Object` flag byte at `+0x08`. See [`flag`].
    pub flags: u8,
    /// Owning player, `+0x09`.
    pub who: u8,
    /// `ObjectData::myhits` (`+0x20`).
    pub myhits: i32,
    /// `ObjectData::damage` (`+0x24`).
    pub damage: i32,
    /// `ObjectData::uid` (`+0x30`).
    pub uid: u16,
    /// `ObjectData::damage_frac` (`+0x3B`).
    pub damage_frac: i8,

    // -- WallData ------------------------------------------------------------------------
    /// `WallData::job_counter` (`+0x48`).
    pub job_counter: u32,
    /// `WallData::job_counter_2` (`+0x4C`).
    pub job_counter_2: u32,
    /// `WallData::constr_time` (`+0x50`).
    pub constr_time: u32,
    /// `WallData::construct_hits` (`+0x54`).
    pub construct_hits: i32,
    /// `WallData::gpiece` (`+0x58`).
    pub gpiece: i32,
    /// `WallData::frame_started` (`+0x5C`).
    pub frame_started: i32,
    /// `WallData::build_masks` (`+0x60`). See [`mask`].
    pub build_masks: u16,
    /// `WallData::ever_seen` (`+0x62`).
    pub ever_seen: u8,
    /// `WallData::ever_seen_completed` (`+0x63`).
    pub ever_seen_completed: u8,
    /// `WallData::helpers` (`+0x64`) — reset each frame by `Wall::process`.
    pub helpers: u8,
    /// `WallData::demolition` (`+0x65`).
    pub demolition: u8,

    // -- BuildData -----------------------------------------------------------------------
    /// `BuildData::orig_type` (`+0x6C`).
    pub orig_type: i32,
    /// `BuildData::gather_down` (`+0x70`).
    pub gather_down: i16,
    /// `BuildData::city` (`+0x72`).
    pub city: i16,
    /// `BuildData::city_down` (`+0x74`).
    pub city_down: i16,
    /// `BuildData::wonder` (`+0x76`).
    pub wonder: i16,
    /// `BuildData::dock`/`farm`/`fort`/`oil_well` union (`+0x78`).
    pub dock: i16,
    /// `BuildData::recharging` (`+0x7A`).
    pub recharging: i16,
    /// `BuildData::attack_ox` (`+0x7C`).
    pub attack_ox: i16,
    /// `BuildData::stance` (`+0x7E`).
    pub stance: i8,
    /// `BuildData::founder` (`+0x7F`).
    pub founder: i8,
    /// `BuildData::gather_max` (`+0x80`).
    pub gather_max: i8,
    /// `BuildData::attack_whom` (`+0x81`).
    pub attack_whom: i8,
    /// `BuildData::max_age` (`+0x83`).
    pub max_age: u8,
    /// `BuildData::infiltrate` (`+0x84`).
    pub infiltrate: u8,
    /// `BuildData::infiltrate2` (`+0x85`).
    pub infiltrate2: u8,
    /// `BuildData::build_queue` (`+0x88`) — carries `queued` (`+0x82`) with it.
    pub queue: BuildQueue,
    /// `BuildData::gather_from` (`+0x98`), the `MiningList`.
    pub gather_from: MiningList,
    /// `BuildData::gather` (`+0xB8`), the `GatherPointList`.
    pub gather: Vec<GatherPoint>,

    /// Bytes of the 220-byte record this module does not model, preserved verbatim so
    /// [`BuildData::image`] is byte-exact. Indices are absolute `BuildData` offsets.
    pub other: [u8; BUILDDATA_SIZE],
}

impl Default for BuildData {
    fn default() -> Self {
        BuildData {
            flags: 0,
            who: 0,
            myhits: 0,
            damage: 0,
            uid: 0,
            damage_frac: 0,
            job_counter: 0,
            job_counter_2: 0,
            constr_time: 0,
            construct_hits: 0,
            gpiece: 0,
            frame_started: 0,
            build_masks: 0,
            ever_seen: 0,
            ever_seen_completed: 0,
            helpers: 0,
            demolition: 0,
            orig_type: 0,
            gather_down: 0,
            city: 0,
            city_down: 0,
            wonder: 0,
            dock: 0,
            recharging: 0,
            attack_ox: 0,
            stance: 0,
            founder: 0,
            gather_max: 0,
            attack_whom: 0,
            max_age: 0,
            infiltrate: 0,
            infiltrate2: 0,
            queue: BuildQueue::default(),
            gather_from: MiningList::default(),
            gather: Vec::new(),
            other: [0; BUILDDATA_SIZE],
        }
    }
}

impl BuildData {
    /// `SubObjectData::is_valid` — `flags & 1`.
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.flags & flag::VALID != 0
    }
    /// `WallData::is_started` (`0x00472360`) — `flags & 2`.
    #[inline]
    pub fn is_started(&self) -> bool {
        self.flags & flag::STARTED != 0
    }
    /// `WallData::is_active` (`0x00472350`) — `flags & 4`. Construction complete.
    #[inline]
    pub fn is_active(&self) -> bool {
        self.flags & flag::ACTIVE != 0
    }
    /// `WallData::is_under_attack` (`0x00472420`) — literally `build_masks & 0x20`.
    #[inline]
    pub fn is_under_attack(&self) -> bool {
        self.build_masks & mask::UNDER_ATTACK != 0
    }
    /// `ObjectData::hits_left` — `hits - damage`.
    #[inline]
    pub fn hits_left(&self, hits: i32) -> i32 {
        hits - self.damage
    }

    /// The head of `Wall::process` (`0x00640450`) that resets the per-frame helper state
    /// [measured, the three writes at the end of the `frame % 16` block]:
    ///
    /// ```text
    /// if (helpers == 0) build_masks &= ~0x400;
    /// else            { helpers = 0; build_masks |= 0x400; }
    /// build_masks &= ~0x800;
    /// ```
    ///
    /// So `WORKED_LAST_FRAME` is a one-frame-delayed "someone is building me" latch, and
    /// `helpers` restarts at zero every tick — which is what makes [`do_construct`]'s
    /// division a *per-frame* harmonic and not a permanent penalty.
    pub fn begin_frame_construction(&mut self) {
        if self.helpers == 0 {
            self.build_masks &= !mask::WORKED_LAST_FRAME;
        } else {
            self.helpers = 0;
            self.build_masks |= mask::WORKED_LAST_FRAME;
        }
        self.build_masks &= !mask::HELPER_COUNTED;
    }

    /// The 220-byte little-endian image, at the `[measured]` field offsets.
    ///
    /// Fields not modelled here come from [`BuildData::other`], so a record loaded from a
    /// live-memory dump round-trips exactly and the checksum is computable before every
    /// field has an owner.
    pub fn image(&self) -> [u8; BUILDDATA_SIZE] {
        let mut b = self.other;
        b[0x08] = self.flags;
        b[0x09] = self.who;
        b[off::MYHITS..off::MYHITS + 4].copy_from_slice(&self.myhits.to_le_bytes());
        b[off::DAMAGE..off::DAMAGE + 4].copy_from_slice(&self.damage.to_le_bytes());
        b[off::UID..off::UID + 2].copy_from_slice(&self.uid.to_le_bytes());
        b[off::DAMAGE_FRAC] = self.damage_frac as u8;

        b[off::JOB_COUNTER..off::JOB_COUNTER + 4].copy_from_slice(&self.job_counter.to_le_bytes());
        b[off::JOB_COUNTER_2..off::JOB_COUNTER_2 + 4]
            .copy_from_slice(&self.job_counter_2.to_le_bytes());
        b[off::CONSTR_TIME..off::CONSTR_TIME + 4].copy_from_slice(&self.constr_time.to_le_bytes());
        b[off::CONSTRUCT_HITS..off::CONSTRUCT_HITS + 4]
            .copy_from_slice(&self.construct_hits.to_le_bytes());
        b[off::GPIECE..off::GPIECE + 4].copy_from_slice(&self.gpiece.to_le_bytes());
        b[off::FRAME_STARTED..off::FRAME_STARTED + 4]
            .copy_from_slice(&self.frame_started.to_le_bytes());
        b[off::BUILD_MASKS..off::BUILD_MASKS + 2].copy_from_slice(&self.build_masks.to_le_bytes());
        b[off::EVER_SEEN] = self.ever_seen;
        b[off::EVER_SEEN_COMPLETED] = self.ever_seen_completed;
        b[off::HELPERS] = self.helpers;
        b[off::DEMOLITION] = self.demolition;

        b[off::ORIG_TYPE..off::ORIG_TYPE + 4].copy_from_slice(&self.orig_type.to_le_bytes());
        b[off::GATHER_DOWN..off::GATHER_DOWN + 2].copy_from_slice(&self.gather_down.to_le_bytes());
        b[off::CITY..off::CITY + 2].copy_from_slice(&self.city.to_le_bytes());
        b[off::CITY_DOWN..off::CITY_DOWN + 2].copy_from_slice(&self.city_down.to_le_bytes());
        b[off::WONDER..off::WONDER + 2].copy_from_slice(&self.wonder.to_le_bytes());
        b[off::DOCK..off::DOCK + 2].copy_from_slice(&self.dock.to_le_bytes());
        b[off::RECHARGING..off::RECHARGING + 2].copy_from_slice(&self.recharging.to_le_bytes());
        b[off::ATTACK_OX..off::ATTACK_OX + 2].copy_from_slice(&self.attack_ox.to_le_bytes());
        b[off::STANCE] = self.stance as u8;
        b[off::FOUNDER] = self.founder as u8;
        b[off::GATHER_MAX] = self.gather_max as u8;
        b[off::ATTACK_WHOM] = self.attack_whom as u8;
        b[off::QUEUED] = self.queue.queued;
        b[off::MAX_AGE] = self.max_age;
        b[off::INFILTRATE] = self.infiltrate;
        b[off::INFILTRATE2] = self.infiltrate2;

        b[off::BUILD_QUEUE_NUM..off::BUILD_QUEUE_NUM + 4]
            .copy_from_slice(&(self.queue.num() as u32).to_le_bytes());
        b[off::MINING_MTN] = self.gather_from.mtn as u8;
        b[off::MINING_CLIFF] = self.gather_from.cliff as u8;
        b
    }

    /// `BuildData::walk_data(DataWalk*)` (`0x0062F270`, build.cpp:35) — feed this record
    /// to a checksum in the engine's exact window order.
    ///
    /// [measured] by decompiling the three walkers in the chain and mapping every
    /// `w->vt[0](begin, end)` pair onto the PDB field offsets:
    ///
    /// ```text
    /// BuildData::walk_data                 0x0062F270
    ///   1. walk [0x7F, 0x80)                   founder                          1 B
    ///   2. walk [0x83, 0x84)                   max_age                          1 B
    ///   3. WallData::walk_data               0x00642510
    ///        a. Object::walk_data            0x00647830
    ///             - SubObject::walk_data     0x006621D0     (object lane)
    ///             - walk [0x20, 0x42)          myhits..launch_frames           34 B
    ///             - optional `launching` SimpleArray<int>
    ///        b. walk [0x48, 0x66)               job_counter..demolition         30 B
    ///   4. Object::must_walk gate            0x00647930
    ///   5. walk [0x70, 0x86)                   gather_down..infiltrate2        22 B
    ///   6. BuildQueue::walk_data             0x006305F0
    ///        - walk [num, num+4)                                                4 B
    ///        - per entry: walk [e, e+0x12)                                     18 B each
    ///   7. walk [0xB4, 0xB6)                   MiningList::{mtn,cliff}          2 B
    ///   8. Array<TCoordData>::walk_data      0x00471C30   (the MiningList body)
    ///   9. PtrLinkListAbstract<GatherPoint>::walk_data 0x004708A0
    ///        - walk count                                                       4 B
    ///        - per point: walk [node+0xC, node+0xD)  1 B, then walk [gp+4, gp+0xD)  9 B
    ///  10. walk [0x6C, 0x70)                   orig_type                        4 B
    /// ```
    ///
    /// The order of 7/8 looks wrong — the two `MiningList` tail bytes are hashed *before*
    /// the array they follow in memory — but that is what the emitted call order says, and
    /// a checksum is order-sensitive, so it is reproduced as written.
    ///
    /// `must_walk` (`Object::must_walk` `0x00647930`) is `true` for any object that is
    /// either owned (`type != 0`), flagged, or has a live `city` link; it is passed in
    /// because the predicate reads state this module does not own.
    pub fn walk(&self, w: &mut CheckSum, must_walk: bool) {
        let img = self.image();
        w.walk(&img[off::FOUNDER..off::FOUNDER + 1]);
        w.walk(&img[off::MAX_AGE..off::MAX_AGE + 1]);

        // WallData::walk_data -> Object::walk_data
        w.walk(&img[0x20..0x42]);
        if must_walk {
            w.walk(&img[off::JOB_COUNTER..0x66]);
        }
        if !must_walk {
            return;
        }
        w.walk(&img[off::GATHER_DOWN..0x86]);

        // BuildQueue::walk_data 0x006305F0
        w.walk(&(self.queue.num() as u32).to_le_bytes());
        for e in &self.queue.entries {
            w.walk(&e.image()[..BuildQueueEntry::WALKED_BYTES]);
        }

        w.walk(&img[off::MINING_MTN..off::MINING_MTN + 2]);
        w.walk(&(self.gather_from.tiles.len() as u32).to_le_bytes());
        for t in &self.gather_from.tiles {
            w.walk(&t.to_le_bytes());
        }

        // PtrLinkListAbstract<GatherPoint>::walk_data 0x004708A0
        w.walk(&(self.gather.len() as u32).to_le_bytes());
        for gp in &self.gather {
            w.walk(&[gp.node_tag]);
            let mut b = [0u8; 9];
            b[0..4].copy_from_slice(&gp.x.to_le_bytes());
            b[4..8].copy_from_slice(&gp.y.to_le_bytes());
            b[8] = gp.action;
            w.walk(&b);
        }

        w.walk(&img[off::ORIG_TYPE..off::ORIG_TYPE + 4]);
    }
}

// =======================================================================================
// Checksum
// =======================================================================================

/// The `CheckSum` `DataWalk` implementation — a running adler32 plus a byte count.
///
/// `CheckSum::walk_function` (`0x00936FF0`, checksums.cpp:109) is the whole of it
/// [measured]:
///
/// ```text
/// this->bytes (+0x14) += (end - begin);
/// this->sum   (+0x10)  = adler32(end - begin, this->sum, begin);
/// ```
///
/// Note the seed: the *running* sum is carried into each call, so the channel value
/// depends on window order and on every previous window, exactly like a stream hash.
#[derive(Clone, Copy, Debug)]
pub struct CheckSum {
    /// `CheckSum + 0x10` — the running adler32.
    pub sum: u32,
    /// `CheckSum + 0x14` — total bytes hashed.
    pub bytes: u64,
}

impl Default for CheckSum {
    fn default() -> Self {
        CheckSum::new()
    }
}

impl CheckSum {
    /// A fresh checksum, seeded the way `adler32` seeds (`1`).
    pub fn new() -> CheckSum {
        CheckSum { sum: 1, bytes: 0 }
    }

    /// One `w->vt[0](begin, end)` window.
    #[inline]
    pub fn walk(&mut self, window: &[u8]) {
        self.bytes += window.len() as u64;
        self.sum = adler32(self.sum, window);
    }
}

/// zlib `adler32`, the lockstep checksum primitive (`0x00A46830`, `__fastcall`, and the
/// duplicate at `0x005089D0`).
///
/// `BASE = 65521`, `NMAX = 5552 = 0x15B0` — both literals appear in the disassembly.
/// Re-exported from [`crate::checksum`], the crate's only implementation.
pub use crate::checksum::adler32;

// =======================================================================================
// The pool, and the channel
// =======================================================================================

/// Every player's build band, in the shape `CheckSums::check_builds` walks it.
///
/// One `Vec` per player, indexed by *band-relative* slot (`0..BUILD_POOL_SLOTS`), so
/// absolute object index is `BUILD_BAND_BASE + slot`.
#[derive(Clone, Debug)]
pub struct BuildPool {
    /// Per-player slots. `None` is an empty slot.
    pub slots: Vec<Vec<Option<BuildData>>>,
    /// `objects.counts[player]` — one past the highest *used* absolute index. The engine
    /// iterates `obj_base[1] .. counts[player]`, so slots beyond this are never walked
    /// even if occupied.
    pub counts: [usize; NUM_LEADERS],
    /// `leaders[p].flags & 1` — only active players are walked.
    pub active: [bool; NUM_LEADERS],
}

impl Default for BuildPool {
    fn default() -> Self {
        BuildPool::new()
    }
}

impl BuildPool {
    /// An empty pool with all players inactive.
    pub fn new() -> BuildPool {
        BuildPool {
            slots: (0..NUM_LEADERS)
                .map(|_| (0..BUILD_POOL_SLOTS).map(|_| None).collect())
                .collect(),
            counts: [BUILD_BAND_BASE; NUM_LEADERS],
            active: [false; NUM_LEADERS],
        }
    }

    /// Place a building at a band-relative slot and extend `counts` the way
    /// `Objects::init_build` does.
    pub fn insert(&mut self, player: usize, slot: usize, b: BuildData) {
        self.slots[player][slot] = Some(b);
        let abs = BUILD_BAND_BASE + slot + 1;
        if abs > self.counts[player] {
            self.counts[player] = abs;
        }
    }

    /// `CheckSums::check_builds` (`0x00937290`, checksums.cpp:646) — the **builds
    /// channel**.
    ///
    /// [measured] iteration, from the decompiled body:
    ///
    /// ```text
    /// for p in 0..8:                                  ; leaders 0x00E3A390, stride 0x6EEC
    ///     if (!(leaders[p].flags & 1)) continue;       ; inactive players contribute nothing
    ///     for i in obj_base[1] .. objects.counts[p]:   ; obj_base[1] == 2000
    ///         o = objects.list[p][i]->get_build();     ; vt +0xAC
    ///         if (o->flags & 1) o->walk_data(w);       ; vt +0x7C
    /// ```
    ///
    /// Two things this pins down, both of which a plausible design gets wrong:
    /// the outer loop is over **players**, not over a global object array, so a flat pool
    /// hashes in a different order; and the `flags & 1` test is on the *object*, so a
    /// destroyed-but-not-yet-compacted slot is skipped rather than hashed as zeros.
    ///
    /// `must_walk` is supplied per building by the caller, because `Object::must_walk`
    /// (`0x00647930`) reads object state this module does not own.
    pub fn check_builds<F: Fn(usize, usize, &BuildData) -> bool>(
        &self,
        w: &mut CheckSum,
        must_walk: F,
    ) {
        for p in 0..NUM_LEADERS {
            if !self.active[p] {
                continue;
            }
            let end = self.counts[p].saturating_sub(BUILD_BAND_BASE);
            for slot in 0..end.min(BUILD_POOL_SLOTS) {
                if let Some(b) = &self.slots[p][slot] {
                    if b.is_valid() {
                        b.walk(w, must_walk(p, slot, b));
                    }
                }
            }
        }
    }

    /// The channel value on its own, seeded fresh. `check_all` does **not** do this — it
    /// carries one running `CheckSum` through all 15 channels — but an isolated channel
    /// value is what a per-subsystem replay diff wants.
    pub fn channel_checksum<F: Fn(usize, usize, &BuildData) -> bool>(&self, must_walk: F) -> u32 {
        let mut w = CheckSum::new();
        self.check_builds(&mut w, must_walk);
        w.sum
    }
}

// =======================================================================================
// Tests
// =======================================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- layout ------------------------------------------------------------------------

    #[test]
    fn embedded_lists_sit_where_the_brief_says() {
        // The lane brief states MiningList at Build+0x98 and GatherPointList at Build+0xb8
        // "from address geometry". The PDB TPI agrees exactly.
        assert_eq!(off::GATHER_FROM, 0x98);
        assert_eq!(off::GATHER, 0xB8);
        // ...and the MiningList tail bytes fall inside the MiningList, not after it.
        assert_eq!(off::MINING_MTN, off::GATHER_FROM + 28);
        assert_eq!(off::MINING_CLIFF, off::GATHER_FROM + 29);
        assert!(off::MINING_CLIFF < off::GATHER);
    }

    #[test]
    fn queue_record_is_twenty_bytes_of_which_eighteen_are_hashed() {
        assert_eq!(BuildQueueEntry::SIZE, 0x14);
        assert_eq!(BuildQueueEntry::WALKED_BYTES, 0x12);
        let e = BuildQueueEntry {
            elapsed: 0x11223344,
            type_index: 0x0102,
            res: [1, 2, 3],
            amt: [10, 20, 30],
            tail: 0x7F7F,
        };
        let img = e.image();
        assert_eq!(&img[0..4], &0x11223344u32.to_le_bytes());
        assert_eq!(&img[4..6], &0x0102i16.to_le_bytes());
        assert_eq!(&img[6..8], &1i16.to_le_bytes());
        assert_eq!(&img[0x0C..0x0E], &10i16.to_le_bytes());
        // The tail is present in the image but must never reach the hash.
        assert_eq!(&img[0x12..0x14], &0x7F7Fi16.to_le_bytes());
    }

    #[test]
    fn image_round_trips_through_other() {
        let mut b = BuildData::default();
        b.other[0xA0] = 0xAB; // an unmodelled byte inside the record
        b.job_counter = 1234;
        b.build_masks = mask::UNDER_ATTACK | mask::WORKED_LAST_FRAME;
        let img = b.image();
        assert_eq!(img[0xA0], 0xAB);
        assert_eq!(
            u32::from_le_bytes(
                img[off::JOB_COUNTER..off::JOB_COUNTER + 4]
                    .try_into()
                    .unwrap()
            ),
            1234
        );
        assert_eq!(
            u16::from_le_bytes(
                img[off::BUILD_MASKS..off::BUILD_MASKS + 2]
                    .try_into()
                    .unwrap()
            ),
            mask::UNDER_ATTACK | mask::WORKED_LAST_FRAME
        );
        assert_eq!(img.len(), BUILDDATA_SIZE);
    }

    // -- placement ---------------------------------------------------------------------

    #[test]
    fn area_is_y_times_x() {
        assert_eq!(
            Footprint {
                x_size: 3,
                y_size: 4
            }
            .area(),
            12
        );
        assert_eq!(
            Footprint {
                x_size: 1,
                y_size: 1
            }
            .area(),
            1
        );
    }

    #[test]
    fn corner_tile_puts_the_centre_where_the_multiply_says() {
        let f = Footprint {
            x_size: 2,
            y_size: 2,
        };
        // (2 + 0*2) * 0x60 = 0xC0 -- one tile in, i.e. the centre of a 2x2 at tile (0,0).
        assert_eq!(f.corner_tile(0, 0), (0xC0, 0xC0));
        // Each tile step adds 2*0x60 = 0xC0 = one full tile.
        assert_eq!(f.corner_tile(1, 0), (0x180, 0xC0));
        let odd = Footprint {
            x_size: 1,
            y_size: 3,
        };
        assert_eq!(odd.corner_tile(4, 4), ((1 + 8) * 0x60, (3 + 8) * 0x60));
    }

    #[test]
    fn tile_corner_inverts_corner_tile_on_the_grid() {
        for &(x_size, y_size) in &[(1, 1), (2, 2), (3, 3), (2, 3), (4, 4)] {
            let f = Footprint { x_size, y_size };
            for tx in 0..6 {
                for ty in 0..6 {
                    let (cx, cy) = f.corner_tile(tx, ty);
                    assert_eq!(
                        f.tile_corner(cx, cy),
                        (tx, ty),
                        "footprint {x_size}x{y_size} at tile ({tx},{ty})"
                    );
                }
            }
        }
    }

    #[test]
    fn tile_of_is_the_engines_two_step_divide() {
        assert_eq!(tile_of(0), 0);
        assert_eq!(tile_of(191), 0);
        assert_eq!(tile_of(192), 1);
        assert_eq!(tile_of(383), 1);
        assert_eq!(tile_of(384), 2);
        // The table version agrees with a plain /192 for every on-map coordinate.
        for c in 0..(192 * 200) {
            assert_eq!(tile_of(c), c / 192, "coord {c}");
        }
    }

    #[test]
    fn site_verdicts_partition_the_way_do_construct_branches() {
        for c in [0, 0x27, 0x28, 0x29, 0x2B] {
            assert_eq!(SiteVerdict::from_code(c), SiteVerdict::Ok);
        }
        assert_eq!(
            SiteVerdict::from_code(0x2A),
            SiteVerdict::OkIfLinkedCityHasWonderCapacity
        );
        for c in [1, 0x26, 0x2C, 0x40] {
            assert_eq!(SiteVerdict::from_code(c), SiteVerdict::Blocked);
        }
    }

    // -- construction ------------------------------------------------------------------

    #[test]
    fn under_attack_quarters_the_construction_rate() {
        let r = ProdRules::shipped();
        assert_eq!(construct_rate(false, false, &r), 100);
        assert_eq!(construct_rate(true, false, &r), 25);
    }

    #[test]
    fn korean_bonus_cancels_the_under_attack_penalty() {
        let r = ProdRules::shipped();
        assert_eq!(r.get(rule::KOREAN_BUILD_UNDER_FIRE), 1);
        assert_eq!(construct_rate(true, true, &r), 100);
        // ...but only while the rule is non-zero. A modded rules.xml turning it off
        // restores the penalty even for Korea.
        let mut off = r.clone();
        off.set(rule::KOREAN_BUILD_UNDER_FIRE, 0);
        assert_eq!(construct_rate(true, true, &off), 25);
    }

    #[test]
    fn helpers_make_construction_harmonic_not_linear() {
        // Four builders at rate 100 on a fresh site: 100 + 50 + 33 + 25 = 208, not 400.
        let st = construct_frame(&[100, 100, 100, 100], 1, 0, 0, 100_000);
        assert_eq!(st.applied, 100 + 50 + 33 + 25);
        assert_eq!(st.job_counter, 208);
        assert_eq!(st.job_counter_2, 208);
        assert_eq!(st.helpers, 4);
        assert!(!st.completed);
    }

    #[test]
    fn the_floor_of_one_lands_after_the_helper_divide() {
        // The 200th builder in a frame divides 100 by 200 -> 0, floored to 1.
        let st = do_construct(100, 1, false, 0, 0, 199, 100_000);
        assert_eq!(st.applied, 1);
        assert_eq!(st.job_counter, 1);
        // A crowd never stalls construction; it only stops accelerating it.
        let rates = vec![100i32; 64];
        let all = construct_frame(&rates, 1, 0, 0, 100_000);
        assert!(all.applied > 0);
    }

    #[test]
    fn ai_speed_multiplies_before_the_helper_divide() {
        let one = do_construct(100, 1, false, 0, 0, 0, 100_000);
        let four = do_construct(100, 4, false, 0, 0, 0, 100_000);
        assert_eq!(one.applied, 100);
        assert_eq!(four.applied, 400);
        // ai_speed <= 1 is a no-op, matching `if (1 < ai_speed)`.
        assert_eq!(do_construct(100, 0, false, 0, 0, 0, 100_000).applied, 100);
    }

    #[test]
    fn completion_fires_on_reaching_construct_time() {
        let st = do_construct(100, 1, false, 900, 0, 0, 1000);
        assert!(st.completed);
        assert_eq!(st.job_counter, 1000);
        let st = do_construct(100, 1, false, 899, 0, 0, 1000);
        assert!(!st.completed);
    }

    #[test]
    fn an_active_building_absorbs_no_work() {
        let st = do_construct(100, 1, true, 5, 5, 3, 1000);
        assert_eq!(st.applied, 0);
        assert_eq!(st.job_counter, 5);
        assert_eq!(st.helpers, 3);
    }

    #[test]
    fn wall_process_resets_helpers_and_latches_the_worked_bit() {
        let mut b = BuildData::default();
        b.helpers = 3;
        b.build_masks = mask::HELPER_COUNTED;
        b.begin_frame_construction();
        assert_eq!(b.helpers, 0);
        assert!(b.build_masks & mask::WORKED_LAST_FRAME != 0);
        assert!(b.build_masks & mask::HELPER_COUNTED == 0);
        // A second idle frame clears the latch.
        b.begin_frame_construction();
        assert!(b.build_masks & mask::WORKED_LAST_FRAME == 0);
    }

    // -- construct time ----------------------------------------------------------------

    #[test]
    fn construct_time_floor_is_one_even_when_a_modifier_zeroes_it() {
        let r = ProdRules::shipped();
        let g = ConstructQueryGates {
            free_first: true,
            ..Default::default()
        };
        assert_eq!(construct_time(5000, false, &g, &r), 1);
        // ...and skip_modifiers bypasses every modifier but not the floor.
        assert_eq!(construct_time(0, true, &g, &r), 1);
    }

    #[test]
    fn building_speed_upgrade_scales_by_tenths_and_can_reach_zero() {
        let r = ProdRules::shipped();
        let mut g = ConstructTimeGates::default();
        assert_eq!(update_construct_time(1000, &g, &r), 1000);
        g.building_speed_upgrade = 1;
        assert_eq!(update_construct_time(1000, &g, &r), 900);
        g.building_speed_upgrade = 10;
        assert_eq!(update_construct_time(1000, &g, &r), 0);
        // The floor only exists in `construct_time`, which is the query every caller uses.
        assert_eq!(
            construct_time(0, false, &ConstructQueryGates::default(), &r),
            1
        );
    }

    #[test]
    fn maya_speed_is_a_divide_not_a_multiply() {
        let r = ProdRules::shipped();
        let g = ConstructTimeGates {
            maya: true,
            ..Default::default()
        };
        // 20% faster == /1.2, i.e. 1000 -> 833, NOT 1000*0.8 == 800.
        assert_eq!(update_construct_time(1000, &g, &r), 833);
    }

    // -- hit points --------------------------------------------------------------------

    #[test]
    fn construct_hits_quantises_progress_into_32_unit_steps() {
        // Full precision would give 1000*500/1000 == 500. The engine's >>5 gives
        // (500>>5)=15, (1000>>5)=31, 15*1000/31 == 483.
        assert_eq!(construct_hits(1000, false, false, 500, 1000), 483);
        assert_ne!(construct_hits(1000, false, false, 500, 1000), 500);
    }

    #[test]
    fn construct_hits_floors_both_sides_at_one() {
        // job_counter 1 -> a = 1 (not 0), so a barely-started site has 1/31 of its HP.
        assert_eq!(construct_hits(1000, false, false, 1, 1000), 1000 / 31);
        // A tiny construct_time floors the denominator too.
        assert_eq!(construct_hits(1000, false, false, 1, 4), 1000);
    }

    #[test]
    fn a_finished_building_has_full_hits() {
        assert_eq!(construct_hits(1000, true, false, 0, 1000), 1000);
        // ...and so does one whose progress already met its time.
        assert_eq!(construct_hits(1000, false, false, 1000, 1000), 1000);
    }

    #[test]
    fn wonders_keep_half_their_hits_from_the_foundation() {
        let h = construct_hits(1000, false, true, 1, 1000);
        assert!(h >= 500, "wonder floor is (h+1)/2 == 500, got {h}");
        assert_eq!(h, 500 + (500 * 1 / 31).max(1));
    }

    #[test]
    fn under_construction_collapse_needs_half_damage_and_the_latch() {
        // 100 effective HP, 50 damage: exactly at the threshold -> collapses.
        assert!(under_construction_collapses(
            true,
            false,
            false,
            100,
            50,
            mask::COLLAPSE_ELIGIBLE
        ));
        // One point short -> survives.
        assert!(!under_construction_collapses(
            true,
            false,
            false,
            100,
            49,
            mask::COLLAPSE_ELIGIBLE
        ));
        // Without the latch bit the rule never fires.
        assert!(!under_construction_collapses(
            true, false, false, 100, 99, 0
        ));
        // A finished building is exempt: it dies only on the primary hits<=damage test.
        assert!(!under_construction_collapses(
            true,
            true,
            false,
            100,
            99,
            mask::COLLAPSE_ELIGIBLE
        ));
    }

    #[test]
    fn a_barely_started_site_dies_to_one_hit() {
        // 1000 full HP, 1 frame of progress out of 1000 -> 32 effective HP.
        let eff = construct_hits(1000, false, false, 1, 1000);
        assert_eq!(eff, 32);
        // 16 damage is half of that, so a single 16-point hit collapses the site.
        assert!(under_construction_collapses(
            true,
            false,
            false,
            eff,
            16,
            mask::COLLAPSE_ELIGIBLE
        ));
    }

    #[test]
    fn razing_interpolates_hits_in_float() {
        // half razed -> half hits.
        assert_eq!(build_hits(false, 0, 1000, Some((500, 1000))), 500);
        // never below 1.
        assert_eq!(build_hits(false, 0, 1000, Some((1000, 1000))), 1);
        // no raze in progress -> the cached value passes through.
        assert_eq!(build_hits(false, 777, 1000, None), 1000);
        assert_eq!(build_hits(true, 777, 1000, None), 777);
    }

    // -- repair ------------------------------------------------------------------------

    #[test]
    fn repair_clamps_stale_damage_before_healing() {
        // damage 500 but max hits fell to 300: the clamp runs first, then the heal.
        assert_eq!(repair_damage(500, 100, 300), (200, 0));
        // Over-repair floors at zero.
        assert_eq!(repair_damage(50, 100, 1000), (0, 0));
        // damage_frac is cleared even by a zero repair.
        assert_eq!(repair_damage(50, 0, 1000).1, 0);
    }

    // -- queue -------------------------------------------------------------------------

    #[test]
    fn an_empty_queue_does_nothing() {
        let r = ProdRules::shipped();
        assert!(queue_step(0, 0, 1, 0, 100, QueueKind::Unit, 1, &r).is_none());
    }

    #[test]
    fn queue_progress_saturates_at_the_total() {
        let r = ProdRules::shipped();
        let s = queue_step(1, 0, 1, 950, 1000, QueueKind::Unit, 1, &r).unwrap();
        assert_eq!(s.rate, 100);
        assert_eq!(s.elapsed, 1000);
        assert!(!s.done);
        // Completion is detected on the frame *after* elapsed reaches total.
        let s = queue_step(1, 0, 1, 1000, 1000, QueueKind::Unit, 1, &r).unwrap();
        assert!(s.done);
    }

    #[test]
    fn a_one_frame_item_completes_immediately() {
        let r = ProdRules::shipped();
        let s = queue_step(1, 0, 1, 0, 1, QueueKind::Unit, 1, &r).unwrap();
        assert!(s.done, "total==1 forces prog=1 and therefore done");
    }

    #[test]
    fn a_slot_past_num_starts_at_minus_one() {
        let r = ProdRules::shipped();
        let s = queue_step(2, 1, 1, 12345, 1000, QueueKind::Unit, 1, &r).unwrap();
        assert_eq!(s.elapsed, 99, "prog is -1, not the stale value");
    }

    #[test]
    fn kind_selects_the_accelerator() {
        let mut r = ProdRules::shipped();
        r.set(rule::ACCEL_CONSTRUCT, 7);
        r.set(rule::ACCEL_TRAIN, 11);
        r.set(rule::ACCEL_RESEARCH, 13);
        let f = |k| queue_step(1, 0, 1, 0, 10_000, k, 1, &r).unwrap().rate;
        assert_eq!(f(QueueKind::Building), 7);
        assert_eq!(f(QueueKind::Unit), 11);
        assert_eq!(f(QueueKind::Research), 13);
        // The type-index window classifies buildings without any type-tree help.
        assert_eq!(
            QueueKind::classify(0x1A0, false, true, true),
            QueueKind::Building
        );
        assert_eq!(
            QueueKind::classify(0x29A, false, true, true),
            QueueKind::Building
        );
        assert_eq!(
            QueueKind::classify(0x60, false, true, true),
            QueueKind::Unit
        );
        assert_eq!(
            QueueKind::classify(0x60, false, true, false),
            QueueKind::Research,
            "a unit the player cannot make falls to accel_research"
        );
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum QueueEvent {
        TrainTime(i32),
        Classify(i32),
        Finished(i32, i32),
        Dirty,
        CompletedUnqueue(i32, i32),
        Repeat(i32, u8, bool),
    }

    struct QueueProbe {
        total: i32,
        kind: QueueKind,
        finish_ok: bool,
        repeat_ok: bool,
        events: Vec<QueueEvent>,
    }

    impl QueueProbe {
        fn unit(total: i32) -> Self {
            Self {
                total,
                kind: QueueKind::Unit,
                finish_ok: true,
                repeat_ok: false,
                events: Vec::new(),
            }
        }
    }

    impl QueueCompletionHost for QueueProbe {
        fn train_time(&mut self, type_index: i32) -> i32 {
            self.events.push(QueueEvent::TrainTime(type_index));
            self.total
        }

        fn classify(&mut self, type_index: i32) -> QueueKind {
            self.events.push(QueueEvent::Classify(type_index));
            self.kind
        }

        fn finished(&mut self, type_index: i32, queue: &BuildQueue, slot: usize) -> bool {
            self.events.push(QueueEvent::Finished(
                type_index,
                queue.entries[slot].elapsed,
            ));
            self.finish_ok
        }

        fn mark_queue_dirty(&mut self) {
            self.events.push(QueueEvent::Dirty);
        }

        fn completed_unqueue(&mut self, type_index: i32, queue: &BuildQueue, slot: usize) {
            self.events.push(QueueEvent::CompletedUnqueue(
                type_index,
                queue.entries[slot].elapsed,
            ));
        }

        fn repeat_unit(&mut self, build: &mut BuildData, type_index: i32) -> bool {
            self.events.push(QueueEvent::Repeat(
                type_index,
                build.queue.queued,
                (build.build_masks & mask::REPEAT_QUEUE) != 0,
            ));
            if self.repeat_ok {
                build.queue.entries[0] = BuildQueueEntry {
                    type_index: type_index as i16,
                    res: [-1; 3],
                    ..BuildQueueEntry::default()
                };
                build.queue.queued = 1;
            }
            self.repeat_ok
        }
    }

    fn one_item_build(type_index: i16, elapsed: i32) -> BuildData {
        let mut build = BuildData::default();
        build.flags = flag::VALID | flag::ACTIVE;
        build.queue.queued = 1;
        build.queue.entries.push(BuildQueueEntry {
            elapsed,
            type_index,
            res: [-1; 3],
            ..BuildQueueEntry::default()
        });
        build
    }

    #[test]
    fn executable_queue_saturates_then_completes_on_the_following_frame() {
        let rules = ProdRules::shipped();
        let mut build = one_item_build(60, 950);
        let mut host = QueueProbe::unit(1000);

        let first = execute_local_queue_slot(&mut build, 0, 1, &rules, &mut host).unwrap();
        assert_eq!(
            first,
            Some(QueueTransaction::Advanced {
                type_index: 60,
                step: QueueStep {
                    elapsed: 1000,
                    done: false,
                    rate: 100,
                },
            })
        );
        assert_eq!(build.queue.queued, 1);
        assert_eq!(build.queue.entries[0].elapsed, 1000);
        assert_eq!(
            host.events,
            vec![QueueEvent::TrainTime(60), QueueEvent::Classify(60)]
        );

        host.events.clear();
        let second = execute_local_queue_slot(&mut build, 0, 1, &rules, &mut host).unwrap();
        assert_eq!(
            second,
            Some(QueueTransaction::Completed {
                type_index: 60,
                step: QueueStep {
                    elapsed: 1000,
                    done: true,
                    rate: 100,
                },
                repeat_attempted: false,
                repeat_succeeded: false,
            })
        );
        assert_eq!(build.queue.queued, 0);
        assert_eq!(build.queue.num(), 1, "unqueue does not shrink allocation");
        assert_eq!(build.queue.entries[0].type_index, 60);
        assert_eq!(build.queue.entries[0].elapsed, 0);
        assert_eq!(
            host.events,
            vec![
                QueueEvent::TrainTime(60),
                QueueEvent::Classify(60),
                QueueEvent::Finished(60, 1000),
                QueueEvent::Dirty,
                QueueEvent::CompletedUnqueue(60, 0),
            ]
        );
    }

    #[test]
    fn completion_compacts_only_the_logical_prefix_and_preserves_stale_allocation() {
        let rules = ProdRules::shipped();
        let mut build = BuildData::default();
        build.queue.queued = 2;
        build.queue.entries = vec![
            BuildQueueEntry {
                elapsed: 1000,
                type_index: 551,
                ..BuildQueueEntry::default()
            },
            BuildQueueEntry {
                elapsed: 17,
                type_index: 552,
                tail: 7,
                ..BuildQueueEntry::default()
            },
            BuildQueueEntry {
                elapsed: 88,
                type_index: 553,
                tail: 9,
                ..BuildQueueEntry::default()
            },
        ];
        let mut host = QueueProbe {
            total: 1000,
            kind: QueueKind::Research,
            finish_ok: true,
            repeat_ok: false,
            events: Vec::new(),
        };

        execute_local_queue_slot(&mut build, 0, 1, &rules, &mut host).unwrap();

        assert_eq!(build.queue.queued, 1);
        assert_eq!(build.queue.num(), 3);
        assert_eq!(build.queue.entries[0].type_index, 552);
        assert_eq!(build.queue.entries[0].elapsed, 17);
        assert_eq!(
            build.queue.entries[1], build.queue.entries[0],
            "memmove leaves the old source as the checksum-visible stale tail"
        );
        assert_eq!(build.queue.entries[2].type_index, 553);
        assert_eq!(build.queue.entries[2].elapsed, 88);
    }

    #[test]
    fn failed_finish_keeps_the_saturated_item_and_skips_unqueue_effects() {
        let rules = ProdRules::shipped();
        let mut build = one_item_build(60, 1000);
        let mut host = QueueProbe::unit(1000);
        host.finish_ok = false;

        let result = execute_local_queue_slot(&mut build, 0, 1, &rules, &mut host).unwrap();

        assert!(matches!(
            result,
            Some(QueueTransaction::FinishBlocked { .. })
        ));
        assert_eq!(build.queue.queued, 1);
        assert_eq!(build.queue.entries[0].elapsed, 1000);
        assert_eq!(
            host.events,
            vec![
                QueueEvent::TrainTime(60),
                QueueEvent::Classify(60),
                QueueEvent::Finished(60, 1000),
            ]
        );
    }

    #[test]
    fn completion_commits_total_not_an_overflowed_progress_candidate() {
        let rules = ProdRules::shipped();
        let mut build = one_item_build(60, i32::MAX);
        let mut host = QueueProbe::unit(100);
        host.finish_ok = false;

        let result = execute_local_queue_slot(&mut build, 0, 1, &rules, &mut host).unwrap();

        assert!(matches!(
            result,
            Some(QueueTransaction::FinishBlocked { .. })
        ));
        assert_eq!(build.queue.entries[0].elapsed, 100);
        assert!(host.events.contains(&QueueEvent::Finished(60, 100)));
    }

    #[test]
    fn unit_repeat_runs_after_unqueue_with_the_latch_temporarily_clear() {
        let rules = ProdRules::shipped();
        let mut build = one_item_build(60, 0);
        build.build_masks |= mask::REPEAT_QUEUE;
        let mut host = QueueProbe::unit(1);
        host.repeat_ok = true;

        let result = execute_local_queue_slot(&mut build, 0, 1, &rules, &mut host).unwrap();

        assert!(matches!(
            result,
            Some(QueueTransaction::Completed {
                repeat_attempted: true,
                repeat_succeeded: true,
                ..
            })
        ));
        assert_eq!(build.queue.queued, 1);
        assert_eq!(build.queue.entries[0].type_index, 60);
        assert_eq!(build.queue.entries[0].elapsed, 0);
        assert_eq!(build.build_masks & mask::REPEAT_QUEUE, 0);
        assert!(host.events.contains(&QueueEvent::Repeat(60, 0, false)));

        let mut failed_build = one_item_build(60, 0);
        failed_build.build_masks |= mask::REPEAT_QUEUE;
        let mut failed_host = QueueProbe::unit(1);
        execute_local_queue_slot(&mut failed_build, 0, 1, &rules, &mut failed_host).unwrap();
        assert_eq!(failed_build.queue.queued, 0);
        assert_ne!(failed_build.build_masks & mask::REPEAT_QUEUE, 0);
        assert!(failed_host
            .events
            .contains(&QueueEvent::Repeat(60, 0, false)));
    }

    #[test]
    fn executable_queue_refuses_corrupt_lengths_before_world_callbacks() {
        let rules = ProdRules::shipped();
        let mut build = one_item_build(60, 0);
        build.queue.queued = 2;
        let mut host = QueueProbe::unit(1000);
        assert_eq!(
            execute_local_queue_slot(&mut build, 0, 1, &rules, &mut host),
            Err(QueueTransactionError::LogicalLengthExceedsAllocation {
                queued: 2,
                allocated: 1,
            })
        );
        assert!(host.events.is_empty());

        build.queue.queued = 1;
        assert_eq!(
            execute_local_queue_slot(&mut build, 1, 1, &rules, &mut host),
            Err(QueueTransactionError::SlotOutsideLogicalQueue { slot: 1, queued: 1 })
        );
        assert!(host.events.is_empty());
    }

    #[test]
    fn queue_accessors_return_minus_one_past_num() {
        let q = BuildQueue {
            queued: 3,
            entries: vec![BuildQueueEntry {
                type_index: 42,
                elapsed: 7,
                ..Default::default()
            }],
        };
        assert_eq!(q.type_at(0), 42);
        assert_eq!(q.elapsed_at(0), 7);
        assert_eq!(q.type_at(1), -1);
        assert_eq!(q.elapsed_at(2), -1);
        // count_queue walks `queued` slots, reading -1 for the missing ones.
        assert_eq!(q.count_queue(|t| t == 42), 1);
        assert_eq!(q.count_queue(|t| t == -1), 2);
        assert_eq!(q.find_in_queue(|t| t == -1), 1);
    }

    // -- train time --------------------------------------------------------------------

    #[test]
    fn the_train_time_ramp_caps_at_three_times_the_scaled_base() {
        let r = ProdRules::shipped();
        let i = TrainRampInput {
            base: 100,
            count: 0,
            job_extra_time: 10,
        };
        // base scaled by unit_rate_base 120% -> 120.
        assert_eq!(train_time_ramp(&i, &r), 120);
        // count 1: +1*10*75 = 750, clamped to 3*120 = 360.
        let i1 = TrainRampInput { count: 1, ..i };
        assert_eq!(train_time_ramp(&i1, &r), 360);
        // and it stays there.
        let i9 = TrainRampInput { count: 9, ..i };
        assert_eq!(train_time_ramp(&i9, &r), 360);
    }

    #[test]
    fn a_zero_job_extra_time_disables_the_ramp_entirely() {
        let r = ProdRules::shipped();
        let i = TrainRampInput {
            base: 450,
            count: 40,
            job_extra_time: 0,
        };
        assert_eq!(train_time_ramp(&i, &r), div100(450 * 120));
    }

    #[test]
    fn a_negative_intermediate_collapses_to_zero_rather_than_propagating() {
        // The clamp is `if (v < 0 || ceiling < 0) v = 0`, i.e. BOTH arms, not a `max(0)`.
        // A negative ramp (a modded job_extra_time) trips the `v < 0` arm...
        let r = ProdRules::shipped();
        let i = TrainRampInput {
            base: 100,
            count: 100,
            job_extra_time: -100,
        };
        assert_eq!(train_time_ramp(&i, &r), 0);

        // ...and a negative scaled base trips the `ceiling < 0` arm, which a naive
        // `min(v, 3*base)` would miss because `v` itself is still positive there.
        let mut neg = r.clone();
        neg.set(rule::UNIT_RATE_BASE, -100);
        let i = TrainRampInput {
            base: 100,
            count: 0,
            job_extra_time: 0,
        };
        assert_eq!(train_time_ramp(&i, &neg), 0);
    }

    #[test]
    fn the_age_penalty_makes_older_types_slower() {
        let r = ProdRules::shipped();
        // 2 ages elapsed, 10% each -> +20%.
        assert_eq!(train_time_age_penalty(1000, 5, 3, &r), 1200);
        // A type from the current or a later age is untouched.
        assert_eq!(train_time_age_penalty(1000, 3, 3, &r), 1000);
        assert_eq!(train_time_age_penalty(1000, 3, 5, &r), 1000);
    }

    #[test]
    fn difficulty_scales_train_time_by_two_thirds_or_three_halves() {
        assert_eq!(train_time_finalize(300, 0), 200);
        assert_eq!(train_time_finalize(300, 2), 200);
        assert_eq!(train_time_finalize(300, 4), 450);
        assert_eq!(train_time_finalize(300, 3), 300);
        assert_eq!(train_time_finalize(0, 3), 1, "floor of 1");
    }

    // -- cost ramp ---------------------------------------------------------------------

    #[test]
    fn progression_bit_one_makes_the_ramp_triangular() {
        assert_eq!(progression_ramp_count(4, 0), 4);
        assert_eq!(progression_ramp_count(4, 1), 4, "bit 0 is not the selector");
        assert_eq!(progression_ramp_count(4, 2), 10);
        assert_eq!(progression_ramp_count(4, 3), 10);
        for n in 0..20 {
            assert_eq!(progression_ramp_count(n, 2), n * (n + 1) / 2);
        }
    }

    #[test]
    fn ramp_classes_pick_the_right_cap() {
        let r = ProdRules::shipped();
        assert_eq!(RampClass::classify(0x34, false, 0, 0), RampClass::Scholar);
        assert_eq!(RampClass::classify(0x32, false, 0, 0), RampClass::Worker);
        assert_eq!(RampClass::classify(0x99, true, 0, 0), RampClass::Worker);
        assert_eq!(RampClass::classify(0x99, false, 8, 0), RampClass::Worker);
        assert_eq!(
            RampClass::classify(0x99, false, 0, 4),
            RampClass::OtherCivilian
        );
        assert_eq!(RampClass::classify(0x99, false, 0, 0), RampClass::Military);
        assert_eq!(r.get(RampClass::Military.rule_offset()), 125);
        assert_eq!(r.get(RampClass::Scholar.rule_offset()), 2000);
    }

    #[test]
    fn the_ramp_ceiling_is_a_percentage_of_the_base_cost_and_zero_means_none() {
        let r = ProdRules::shipped();
        let sup = SupportPair {
            support: [3, -1],
            support_cost: [1000, 0],
        };
        // Military cap: 125% of a 100 base cost == 125, so a count of 5 clamps.
        let v = ramp_cost(3, 5, 0, RampClass::Military, 100, &sup, false, &r);
        assert_eq!(v, 125);
        // A zero cap means NO cap, not "cost nothing".
        let mut nocap = r.clone();
        nocap.set(rule::UNIT_MILITARY_RAMP_MAX, 0);
        let v = ramp_cost(3, 5, 0, RampClass::Military, 100, &sup, false, &nocap);
        assert_eq!(v, 5000);
    }

    #[test]
    fn only_matching_support_slots_contribute() {
        let r = ProdRules::shipped();
        let sup = SupportPair {
            support: [3, 4],
            support_cost: [10, 20],
        };
        let mut nocap = r.clone();
        nocap.set(rule::UNIT_MILITARY_RAMP_MAX, 0);
        assert_eq!(
            ramp_cost(3, 2, 0, RampClass::Military, 100, &sup, false, &nocap),
            20
        );
        assert_eq!(
            ramp_cost(4, 2, 0, RampClass::Military, 100, &sup, false, &nocap),
            40
        );
        assert_eq!(
            ramp_cost(9, 2, 0, RampClass::Military, 100, &sup, false, &nocap),
            0
        );
    }

    #[test]
    fn a_zero_or_negative_count_short_circuits_the_whole_ramp() {
        let r = ProdRules::shipped();
        let sup = SupportPair {
            support: [3, -1],
            support_cost: [1000, 0],
        };
        assert_eq!(
            ramp_cost(3, 0, 0, RampClass::Military, 100, &sup, false, &r),
            0
        );
        assert_eq!(
            ramp_cost(3, -5, 0, RampClass::Military, 100, &sup, false, &r),
            0
        );
    }

    #[test]
    fn maize_cuts_the_ramped_cost_in_half() {
        let mut r = ProdRules::shipped();
        r.set(rule::UNIT_MILITARY_RAMP_MAX, 0);
        let sup = SupportPair {
            support: [3, -1],
            support_cost: [100, 0],
        };
        let plain = ramp_cost(3, 4, 0, RampClass::Military, 100, &sup, false, &r);
        let maize = ramp_cost(3, 4, 0, RampClass::Military, 100, &sup, true, &r);
        assert_eq!(plain, 400);
        assert_eq!(maize, 200, "maize_ramping_bonus is 50%");
    }

    #[test]
    fn the_scholar_extra_starts_at_eight() {
        assert_eq!(scholar_extra(7), 0);
        assert_eq!(scholar_extra(8), 1); // n-7
        assert_eq!(scholar_extra(14), 7); // 14-7
        assert_eq!(scholar_extra(15), 8 + 1); // (15-7) + (15-14)
        assert_eq!(scholar_extra(21), 14 + 7); // (21-7) + (21-14)
    }

    #[test]
    fn research_ramp_is_fifty_percent_per_tech_in_progress() {
        let r = ProdRules::shipped();
        assert_eq!(research_ramp_cost(200, 0, &r), 200);
        assert_eq!(research_ramp_cost(200, 1, &r), 300);
        assert_eq!(research_ramp_cost(200, 2, &r), 400);
    }

    // -- refund ------------------------------------------------------------------------

    #[test]
    fn a_same_age_refund_already_overcharges() {
        let r = ProdRules::shipped();
        // base = 100*100/(100 - 10*0) = 100; then += (0+1)*10*100/100 = 10  ->  110.
        // Even at zero ages elapsed the adjusted figure exceeds what was paid, so the
        // `amt - adj` credit is already negative. That is the function as emitted; it is
        // not a sign error in the port.
        assert_eq!(refund_amount(100, 0, &r), 110);
        // One age later: base = 100*100/90 = 111; += 2*10*111/100 = 22  ->  133.
        assert_eq!(refund_amount(100, 1, &r), 133);
    }

    #[test]
    fn a_stale_refund_can_charge_the_player() {
        let r = ProdRules::shipped();
        let e = BuildQueueEntry {
            res: [3, -1, -1],
            amt: [100, 0, 0],
            ..Default::default()
        };
        let (deltas, new_amts) = refund_cost(&e, 0, &r, &ModeConfig::fidelity());
        assert_eq!(deltas[0].0, 3);
        assert_eq!(deltas[0].1, 100 - 110, "the player is charged 10");
        assert_eq!(new_amts[0], 110, "the record is rewritten in place");
        // Unused slots are left alone.
        assert_eq!(deltas[1], (-1, 0));
    }

    #[test]
    fn improved_refund_neither_bills_nor_rewrites_on_the_real_queue_path() {
        let r = ProdRules::shipped();
        let e = BuildQueueEntry {
            res: [3, -1, -1],
            amt: [100, 0, 0],
            ..Default::default()
        };
        let (deltas, new_amts) = refund_cost(&e, 0, &r, &ModeConfig::improved());
        assert_eq!(deltas[0], (3, 0), "cancelling cannot charge the player");
        assert_eq!(new_amts[0], 100, "the paid amount is not compounded");

        let mut sign_only = ModeConfig::improved_bare();
        sign_only
            .enable(crate::deviations::Deviation::RefundChargesPlayer)
            .unwrap();
        let (deltas, new_amts) = refund_cost(&e, 0, &r, &sign_only);
        assert_eq!(deltas[0], (3, 0));
        assert_eq!(
            new_amts[0], 110,
            "the write-back bug remains independently selectable"
        );

        let mut state_only = ModeConfig::improved_bare();
        state_only
            .enable(crate::deviations::Deviation::RefundRepeatCompounding)
            .unwrap();
        let (deltas, new_amts) = refund_cost(&e, 0, &r, &state_only);
        assert_eq!(
            deltas[0],
            (3, -10),
            "the negative credit bug remains independently selectable"
        );
        assert_eq!(new_amts[0], 100);
    }

    #[test]
    fn unpay_is_the_plain_path() {
        let e = BuildQueueEntry {
            res: [3, 5, -1],
            amt: [40, 60, 999],
            ..Default::default()
        };
        let d = unpay_amounts(&e);
        assert_eq!(d[0], (3, 40));
        assert_eq!(d[1], (5, 60));
        assert_eq!(d[2], (-1, 0), "a -1 resource slot pays nothing back");
    }

    // -- checksum ----------------------------------------------------------------------

    #[test]
    fn adler32_matches_known_vectors() {
        assert_eq!(adler32(1, b""), 1);
        assert_eq!(adler32(1, b"a"), 0x00620062);
        assert_eq!(adler32(1, b"abc"), 0x024d0127);
        assert_eq!(adler32(1, b"Wikipedia"), 0x11E60398);
    }

    #[test]
    fn adler32_is_a_running_seed_not_a_one_shot() {
        // The engine carries `sum` between windows, so hashing in two pieces must equal
        // hashing the concatenation. If it did not, window order would not matter, and it
        // very much does.
        let a = adler32(adler32(1, b"abc"), b"def");
        let b = adler32(1, b"abcdef");
        assert_eq!(a, b);
    }

    #[test]
    fn the_walk_is_order_sensitive() {
        let mut b = BuildData::default();
        b.founder = 1;
        b.max_age = 2;
        let mut w1 = CheckSum::new();
        b.walk(&mut w1, true);

        let mut c = b.clone();
        c.founder = 2;
        c.max_age = 1;
        let mut w2 = CheckSum::new();
        c.walk(&mut w2, true);
        assert_ne!(
            w1.sum, w2.sum,
            "swapping two walked bytes must change the sum"
        );
        assert_eq!(w1.bytes, w2.bytes);
    }

    #[test]
    fn must_walk_false_truncates_the_window_set() {
        let b = BuildData::default();
        let mut full = CheckSum::new();
        b.walk(&mut full, true);
        let mut short = CheckSum::new();
        b.walk(&mut short, false);
        assert!(short.bytes < full.bytes);
        // The unconditional prefix is founder + max_age + the 34-byte object window.
        assert_eq!(short.bytes, 1 + 1 + 0x22);
    }

    #[test]
    fn the_queue_tail_short_never_reaches_the_hash() {
        let mut a = BuildData::default();
        a.queue.queued = 1;
        a.queue.entries.push(BuildQueueEntry {
            elapsed: 10,
            type_index: 5,
            tail: 0,
            ..Default::default()
        });
        let mut b = a.clone();
        b.queue.entries[0].tail = -1;

        let mut wa = CheckSum::new();
        a.walk(&mut wa, true);
        let mut wb = CheckSum::new();
        b.walk(&mut wb, true);
        assert_eq!(wa.sum, wb.sum, "BuildQueue::walk_data stops at entry+0x12");

        // ...while a byte inside the walked prefix does change it.
        let mut c = a.clone();
        c.queue.entries[0].elapsed = 11;
        let mut wc = CheckSum::new();
        c.walk(&mut wc, true);
        assert_ne!(wa.sum, wc.sum);
    }

    #[test]
    fn check_builds_walks_players_in_order_and_skips_inactive_ones() {
        let mut pool = BuildPool::new();
        let mk = |founder: i8| {
            let mut b = BuildData::default();
            b.flags = flag::VALID | flag::ACTIVE;
            b.founder = founder;
            b
        };
        pool.active[0] = true;
        pool.active[1] = true;
        pool.insert(0, 0, mk(1));
        pool.insert(1, 0, mk(2));
        let both = pool.channel_checksum(|_, _, _| true);

        // Same buildings, opposite players: a flat pool would hash the same, the engine
        // ordering must not.
        let mut swapped = BuildPool::new();
        swapped.active[0] = true;
        swapped.active[1] = true;
        swapped.insert(0, 0, mk(2));
        swapped.insert(1, 0, mk(1));
        assert_ne!(both, swapped.channel_checksum(|_, _, _| true));

        // An inactive player contributes nothing at all.
        let mut off = pool.clone();
        off.active[1] = false;
        let only0 = off.channel_checksum(|_, _, _| true);
        let mut solo = BuildPool::new();
        solo.active[0] = true;
        solo.insert(0, 0, mk(1));
        assert_eq!(only0, solo.channel_checksum(|_, _, _| true));
    }

    #[test]
    fn an_invalid_slot_is_skipped_not_hashed_as_zeros() {
        let mut pool = BuildPool::new();
        pool.active[0] = true;
        let mut dead = BuildData::default();
        dead.flags = 0; // not VALID
        pool.insert(0, 0, dead);
        let mut live = BuildData::default();
        live.flags = flag::VALID;
        pool.insert(0, 1, live.clone());

        let with_dead = pool.channel_checksum(|_, _, _| true);

        let mut only_live = BuildPool::new();
        only_live.active[0] = true;
        only_live.slots[0][1] = Some(live);
        only_live.counts[0] = BUILD_BAND_BASE + 2;
        assert_eq!(with_dead, only_live.channel_checksum(|_, _, _| true));
    }

    #[test]
    fn slots_past_the_count_are_never_walked() {
        let mut pool = BuildPool::new();
        pool.active[0] = true;
        let mut b = BuildData::default();
        b.flags = flag::VALID;
        pool.slots[0][5] = Some(b);
        // counts still says the band is empty, so nothing is hashed.
        assert_eq!(pool.counts[0], BUILD_BAND_BASE);
        assert_eq!(pool.channel_checksum(|_, _, _| true), CheckSum::new().sum);
    }

    #[test]
    fn the_band_base_is_two_thousand() {
        assert_eq!(BUILD_BAND_BASE, 2000);
        assert_eq!(BUILD_POOL_SLOTS, 601);
        // The band must not reach the next one, which starts at 3000.
        assert!(BUILD_BAND_BASE + BUILD_POOL_SLOTS <= 3000);
    }

    // -- rules -------------------------------------------------------------------------

    #[test]
    fn shipped_rules_carry_the_values_the_loader_stores() {
        let r = ProdRules::shipped();
        assert_eq!(r.get(rule::UNIT_RATE_BASE), 120);
        assert_eq!(r.get(rule::UNIT_RATE_PROGRESSION), 75);
        assert_eq!(r.get(rule::ACCEL_CONSTRUCT), 100);
        assert_eq!(r.get(rule::UNIT_SCHOLAR_RAMP_MAX), 2000);
        assert_eq!(r.get(rule::MAIZE_RAMPING_BONUS), 50);
        for &(off, name, v) in SHIPPED_PRODUCTION_RULES {
            assert_eq!(r.get(off), v, "{name} at {off:#x}");
        }
    }

    #[test]
    #[should_panic(expected = "misaligned")]
    fn a_misaligned_rule_offset_is_a_bug_not_a_rounding() {
        ProdRules::shipped().get(0x221);
    }

    // -- integer idioms ----------------------------------------------------------------

    #[test]
    fn pct_up_and_pct_down_are_not_inverses() {
        assert_eq!(pct_up(100, 33), 75);
        assert_eq!(pct_down(100, 33), 67);
        assert_ne!(pct_up(100, 33), pct_down(100, 33));
    }

    #[test]
    fn the_divisions_truncate_toward_zero_like_c() {
        assert_eq!(div4_trunc(-7), -1);
        assert_eq!(div100(-150), -1);
        assert_eq!(div4_trunc(7), 1);
    }
}
