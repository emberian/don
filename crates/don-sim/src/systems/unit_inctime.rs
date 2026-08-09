//! `Unit::inc_time` `0x00610B40` — the unported half of `Objects::inc_time`, step 15.
//!
//! Serves the **`guys` channel** (channel 7, `CheckSums::check_guys` `0x00937430`) of the
//! fifteen in `CheckSums::check_all` `0x00936560`. Every field this module writes —
//! `cur_anim`, `cur_time`, `end_time`, `last_time`, `queued_attack`, `x`, `y`, `z`,
//! `last_x`, `last_y`, `last_z` — is inside the 155 bytes `GuyData::walk_data`
//! `0x005E0210` hands the visitor, so **animation timing is simulation state, not
//! presentation**. It rides [`crate::systems::groups_guys::GuyData`] directly; this module
//! declares no state of its own.
//!
//! # What step 15 actually is
//!
//! The `ammo` lane established that `Ammo::process` is a one-byte `ret` and that all
//! projectile motion happens in `Ammo::inc_time` from this step. The same reading of
//! `Objects::inc_time` `0x0065DB70` gives the rest of the step, and it is **six loops, not
//! one** [measured, capstone + `re/decomp-all/0065db70.c`]:
//!
//! ```text
//! for s in 0..10:                              # NO ROTATION. Fixed 0..9.
//!     if leaders[s].flags & 1:
//!         for k in 0 .. unit_mark[s]:          # the unit band
//!             if obj.flags & 1: obj->vt[+0xA0]();  obj->vt[+0x154]()
//!         for k in 2000 .. build_mark[s]:      # the building band
//!             if obj.flags & 1: obj->vt[+0xA0]()
//! for g in goods:                              # skips objects whose vtable IS Good::vftable
//!     if g.flags & 1: g->vt[+0xA0]()
//! for a in ammo:   if a.flags & 3: Ammo::inc_time(a)      # direct, non-virtual
//! for d in deaths: if d.live:     DeathObj::inc_time(d)
//! Farms::inc_time(x);  Doober::inc_time();  Surf::inc_time()
//! ```
//!
//! Three things fall out that were not previously recorded anywhere in this repository:
//!
//! * **Step 15 does not rotate owners.** `Objects::process_all` (step 14) walks
//!   `(frame + i) % 10`; `Objects::inc_time` walks the ten leader records straight through
//!   as a pointer walk `0x00E3A390 .. 0x00E7F8C8` step `0x6EEC`, which is exactly ten
//!   iterations. A scheduler that reuses step 14's rotation for step 15 is wrong.
//! * **Step 15 never visits the wall band.** The second inner loop is bounded by
//!   `build_mark` (`Objects +0x184`), and there is no third loop at base 3000. Walls get
//!   `process` and never `inc_time`.
//! * **The unit band calls a second virtual, `+0x154` `Unit::execute_events`
//!   `0x0060EDC0`**, that the building band does not. It is unported and named in
//!   [`IncTimeGaps::execute_events`].
//!
//! # The `inc_time` family, complete
//!
//! The brief asked which `Objects::inc_time` children are stubs, the way `Ammo::process`
//! turned out to be. Every `Object` subclass resolved through its RTTI vtable at slot
//! `+0xA0` [measured, `schema/vtables.json` + a direct `.rdata` read]:
//!
//! | class | vtable `+0xA0` | size | verdict |
//! |---|---|---:|---|
//! | `Unit` | `Unit::inc_time` `0x00610B40` | 122 | **ported here** |
//! | `Animal` | `Unit::inc_time` `0x00610B40` | 122 | same body — `AnimalData : Unit` |
//! | `Build` | `Wall::inc_time` `0x0063FB60` | 2273 | same body — `BuildData : Wall` |
//! | `Wall` | `Wall::inc_time` `0x0063FB60` | 2273 | reachable only via the build band |
//! | `Good` | `Good::inc_time` `0x0066D850` | **1** | **bare `ret`** |
//! | `Object` | `0x0041C150` | **1** | **bare `ret`**, COMDAT-folded |
//! | `SubObject` | `0x0041C150` | **1** | **bare `ret`**, COMDAT-folded |
//! | `Item` | `0x0041C150` | **1** | **bare `ret`**, COMDAT-folded |
//! | `City` | — | — | **not an `Object`**; `City : CityOut`, never in the arrays |
//!
//! `Good::inc_time` is the sharper finding of the two kinds. It is its own one-byte
//! function at its own address, *and* the goods loop in `Objects::inc_time` compares each
//! object's vtable pointer against `Good::vftable` and skips the call entirely when it
//! matches (`0x0065DBF7`). Retail hand-devirtualises a known-empty override rather than
//! paying the indirect call. A one-byte `ret` is a finding: goods and items have no
//! per-tick clock at all.
//!
//! `Wall::inc_time` — shared by every building and every wall — is 2,273 bytes of which
//! almost all is presentation: 9× `GraphicPieces::emit_build_particles`,
//! `GraphicPieces::emit_queue_particles`, `GraphicChad::GraphicChad`,
//! `fast_angle_to_degrees`, `WallOut::get_gpiece`. Its two non-presentation calls are
//! **`Wall::update_hits` `0x0063F0D0`** (1,509 B, gated on `vt[+0x4C] == WallData::is_active`
//! and `flags & 4`, i.e. real walls only) and `GraphicEvents::execute_game_events`. It is
//! **not** ported here; it is a different lane's shape and this module names it rather than
//! guessing at it. `DeathObj::inc_time` `0x008D5240` (540 B) is the corpse clock —
//! `DeathObj::clear_blocking` is its sim payload. `Doober::inc_time` `0x00846770` is an
//! alpha fade over terrain clutter with no RNG. `Surf::inc_time` `0x008A1A00` draws from
//! **`internal_random`** `0x00EB697C`, not the sim stream, and is cosmetic water.
//!
//! # RNG — the reason this matters beyond animation
//!
//! `Guy::inc_time` calls `Guy::set_anim` `0x005DA300`, and **`Guy::set_anim` draws from
//! `game_random`** — `mov ecx, [0x00C06184]` at `0x005DAC68`, `0x005DB21D` and
//! `0x005DB339`, each `Random::get(0, 0xFFFF)` followed by `% 100` [measured]. Three
//! conditional sites, so the draws per call are between 0 and 3 and are data-dependent.
//! Animation transitions therefore *move the simulation RNG stream*, which means a port
//! that skips them silently desyncs everything drawn afterwards in the tick.
//!
//! Two more `game_random` consumers live in step 15 and are recorded here because nothing
//! else in the repository records them: **`Farms::inc_time` `0x008D8600` draws twice**
//! (`Random::get(0,0xFFFF) % 1000` at `0x008D87A9`, then `% (n-1)` at `0x008D87D9`) and it
//! runs unconditionally at the tail of every step 15. `GuyOut::graph_inc_frame`
//! `0x005DD200`, called from the tail of `Guy::inc_time` for owners 0..7, draws six times
//! but from `internal_random`, so it does not perturb the sim stream.
//!
//! This module draws nothing. It **counts** every skipped site in [`IncTimeGaps`], the way
//! the tick driver counts the anti-air dud roll: drawing the wrong number of values is
//! strictly worse than drawing none and reporting the count.
//!
//! # Provenance
//!
//! `[measured]` on this Mac against `ron-bin/riseofnations.exe` (sha256 `30478a44…625079`)
//! and `ron-bin/sbl/rise.pdb`, by capstone disassembly. Structure cross-read from
//! `re/decomp-all/*.c` is a hypothesis about shape only.
//!
//! | symbol | VA | size | role |
//! |---|---|---:|---|
//! | `Objects::inc_time` | `0x0065DB70` | 360 | step 15 driver |
//! | `Unit::inc_time` | `0x00610B40` | 122 | dispatcher — ported exactly |
//! | `Guy::inc_time` | `0x005D9E10` | 1251 | the animation clock — ported exactly |
//! | `Guy::set_anim` | `0x005DA300` | 4723 | **tail ported, head unread** |
//! | `AnimationPacket::get_anim_time` | `0x00918C40` | 88 | anim duration, default `200` |
//! | `AnimationPacket::get_game_frames` | `0x00918CC0` | 88 | used by `Wall`/`DeathObj` |
//! | `Guy::set_new_location` | `0x005D86F0` | 899 | crew-guy call site ported exactly |
//! | `GuyOut::graph_inc_frame` | `0x005DD200` | 3890 | presentation, `internal_random` |
//! | `Unit::execute_events` | `0x0060EDC0` | — | the `+0x154` sibling call, unported |
//! | anim-class table | `0x00AF4370` | 38×4 | transcribed in [`ANIM_CLASS`] |
//!
//! # Fidelity
//!
//! **Research-only Tier C.** Every function here is an instruction-level transcription
//! with its own test; nothing here has been executed against retail, and the oracle has no
//! case for it. The composite `Guy` / `Unit` / `Objects` drivers are crate-private and carry
//! a `_research_partial` suffix, so this recovered module cannot be mistaken for the shipped
//! step-15 runtime merely because it is declared. The machine-readable boundary is
//! [`RUNTIME_FIDELITY_READY`] / [`RUNTIME_FIDELITY_BLOCKERS`]. Two boundaries inside the
//! recovered unit/guy path are load-bearing and are counted rather than papered over:
//! The complete admission ledger is `docs/mechanics/unit-inctime.md`.
//!
//! 1. **`Guy::set_anim`'s head is unread.** Bytes `0x005DA300..0x005DB3F0` resolve *which*
//!    animation actually plays (variation selection, hero and spell special-cases, the
//!    three RNG draws). What is ported is the tail, `0x005DB3FA..0x005DB570`, which is the
//!    part that writes the checksummed fields: `cur_anim = <resolved>`, `cur_time = 0`,
//!    `end_time = anim_frames(anim)`. There is a second tail path, gated on a head-local
//!    (`[ebp-0x1C] == 8`) that preserves animation phase across the change instead of
//!    zeroing `cur_time`; whether that local is the walk class is a *hypothesis* and is
//!    deliberately not implemented. See [`set_anim_tail`].
//! 2. **The animation packets are art assets we do not load.** `end_time` is
//!    `anim_frames[anim_index]` read out of the loaded `.anm` data. Retail's own fallback
//!    when the animation is absent is `end_time = 3` (`mov eax, 3` at `0x005DB558`) and
//!    `get_anim_time` returns `200` (`0x00918C8D`); [`MissingAnimData`] is exactly those
//!    two constants, so the clock runs and the state machine is exercised, but the
//!    *durations* are retail's missing-asset fallback rather than retail's real ones.

use crate::objects::{Band, ObjectRegistry, BUILD_BAND_BASE, OWNER_SLOTS};
use crate::systems::groups_guys::{GuyData, UnitGuys, UnitTypeStats};

/// Whether this module may serve step 15 on a fidelity or product surface.
pub const RUNTIME_FIDELITY_READY: bool = false;

/// Retail step-15 work still absent from the recovered driver.
///
/// The first three entries block even the unit/guy sub-path; the remainder block the full
/// `Objects::inc_time` step. The list stays explicit so green research tests cannot become
/// a completeness claim.
pub const RUNTIME_FIDELITY_BLOCKERS: &[&str] = &[
    "Guy::set_anim head 0x005DA300 (including game_random draws)",
    "Unit::execute_events 0x0060EDC0",
    "retail AnimationPacket/.anm data",
    "Wall::inc_time 0x0063FB60",
    "DeathObj::inc_time 0x008D5240",
    "Farms::inc_time 0x008D8600 (including game_random draws)",
    "Guy::set_new_location recursive squad path 0x005D86F0",
    "Doober::inc_time 0x00846770",
    "Surf::inc_time 0x008A1A00",
];

// ---------------------------------------------------------------------------
// UnitAnim — the 38-value enum and the class table at 0x00AF4370
// ---------------------------------------------------------------------------

/// `UnitAnim::NUM_PEASANT_ANIMS` — the full enum has 38 members [measured, PDB
/// `LF_ENUM UnitAnim` field list `0x46B0`].
pub const NUM_PEASANT_ANIMS: usize = 38;

/// `UnitAnim::NUM_UNIT_ANIMS` — non-peasant units only reach the first 25.
pub const NUM_UNIT_ANIMS: i32 = 25;

/// The `UnitAnim` values this file names, exactly as the PDB spells them.
pub mod anim {
    pub const CHAR_DEFAULT: i8 = 0;
    pub const CHAR_IDLE1: i8 = 1;
    pub const CHAR_IDLE2: i8 = 2;
    pub const CHAR_IDLE3: i8 = 3;
    pub const CHAR_GROUP_IDLE1: i8 = 4;
    pub const CHAR_GROUP_IDLE2: i8 = 5;
    pub const CHAR_GROUP_IDLE3: i8 = 6;
    pub const CHAR_SLOG: i8 = 7;
    pub const CHAR_WALK: i8 = 8;
    pub const CHAR_JOG: i8 = 9;
    pub const CHAR_ATTACKWALK: i8 = 10;
    pub const CHAR_ATTACK1: i8 = 11;
    pub const CHAR_ATTACK2: i8 = 12;
    pub const CHAR_ATTACK3: i8 = 13;
    pub const CHAR_ATTACKSPECIAL: i8 = 14;
    pub const CHAR_DEATH_STAB1: i8 = 15;
    pub const CHAR_DEATH_SPLODED2: i8 = 20;
    pub const CHAR_TURN_LEFT: i8 = 21;
    pub const CHAR_CHOP_WOOD: i8 = 25;
    pub const CHAR_WALK_WITH_WOOD: i8 = 26;
    pub const CHAR_WALK_TO_ORE: i8 = 32;
    pub const CHAR_BUILD: i8 = 33;
    pub const CHAR_FARM: i8 = 37;
}

/// The animation-class table at `0x00AF4370`, transcribed [measured, `.rdata` read].
///
/// Retail indexes it with a `movsx` of `Guy::cur_anim` and **no bounds check**; entry 38
/// onward is string data, so an out-of-range `cur_anim` reads garbage in retail. The table
/// maps every animation to the canonical member of its family, which is why the three
/// comparisons in `Guy::inc_time` (`== 8`, `== 12`) cover seven and four animations
/// respectively.
///
/// | class | animations |
/// |---|---|
/// | `0` `CHAR_DEFAULT` | 0–6 — default and the six idles |
/// | `8` `CHAR_WALK` | 7, 8, 9, 26, 28, 30, 32 — every locomotion, including carrying |
/// | `10` `CHAR_ATTACKWALK` | 10 alone |
/// | `12` `CHAR_ATTACK2` | 11–14 — every attack |
/// | `15` `CHAR_DEATH_STAB1` | 15–20 — every death |
/// | *self* | 21–25, 27, 29, 31, 33–37 — turns, pack, and the work animations |
pub const ANIM_CLASS: [i8; NUM_PEASANT_ANIMS] = [
    0, 0, 0, 0, 0, 0, 0, // 0..6   default + idles
    8, 8, 8,  // 7..9    slog / walk / jog
    10, // 10      attackwalk
    12, 12, 12, 12, // 11..14  attacks
    15, 15, 15, 15, 15, 15, // 15..20  deaths
    21, 22, 23, 24, 25, // 21..25  turns, pack, unpack, chop
    8,  // 26      walk with wood
    27, // 27      dump wood
    8,  // 28      walk to wood
    29, // 29      mine ore
    8,  // 30      walk with ore
    31, // 31      dump ore
    8,  // 32      walk to ore
    33, 34, 35, 36, 37, // 33..37  build, repair, sow, reap, farm
];

/// `ANIM_CLASS[CHAR_WALK]` — the locomotion family.
pub const CLASS_WALK: i8 = 8;
/// `ANIM_CLASS[CHAR_ATTACK2]` — the attack family.
pub const CLASS_ATTACK: i8 = 12;
/// `ANIM_CLASS[CHAR_DEATH_STAB1]` — the death family.
pub const CLASS_DEATH: i8 = 15;

/// What [`anim_class`] answers for an animation outside the table.
///
/// Retail reads past the end of `0x00AF4370` and gets whatever `.rdata` follows; this
/// returns a value that matches no class instead, so the caller takes the "no special
/// family" branch and no index panics. A `cur_anim` that reaches here is already a desync.
pub const CLASS_OUT_OF_TABLE: i8 = -1;

/// The canonical animation of `a`'s family, per the table at `0x00AF4370`.
#[inline]
pub fn anim_class(a: i8) -> i8 {
    if a >= 0 && (a as usize) < NUM_PEASANT_ANIMS {
        ANIM_CLASS[a as usize]
    } else {
        CLASS_OUT_OF_TABLE
    }
}

// ---------------------------------------------------------------------------
// The bits and sentinels the two functions read
// ---------------------------------------------------------------------------

/// `TypeIndex::SCHOLARS` = 52 [measured, PDB `LF_ENUM TypeIndex`].
pub const TYPE_SCHOLARS: i32 = 52;
/// `TypeIndex::SCHOLARSKOREAN` = 53.
pub const TYPE_SCHOLARS_KOREAN: i32 = 53;

/// `UnitData::unit_masks2` `+0x6C` bit `0x10`: while set, the guy clock does not advance.
///
/// The increment is zeroed, but `last_time` is still copied and the `cur_time >= end_time`
/// loop is still entered — so a unit frozen exactly on a boundary still transitions.
pub const UNIT_MASKS2_FREEZE_ANIM: u32 = 0x0000_0010;

/// `GuyData::guy_flags` `+0x9A` bit `0x4`: an attack-family animation advances **two**
/// clock units per tick instead of one.
///
/// Named from its only observed use, which is this one comparison at `0x005D9E35`. No
/// writer of the bit has been traced, so the name describes the effect, not a rule field.
pub const GUY_FLAG_DOUBLE_ANIM_STEP: u16 = 0x0004;

/// `UnitTypeData::unit_flags2` `+0x2B8` bit `0x20` — the bit `UnitData::is_hero`
/// `0x0046CE60` returns (`mov eax,[ptype+0x2B8]; and eax,0x20`).
pub const UNIT_FLAGS2_HERO: i32 = 0x0000_0020;

/// `end_time` when the animation is absent from the packet: `mov eax, 3` at `0x005DB558`.
pub const NO_ANIM_END_TIME: u32 = 3;

/// `AnimationPacket::get_anim_time`'s answer when the animation is absent:
/// `mov eax, 0xC8` at `0x00918C8D`.
pub const NO_ANIM_TIME: i32 = 200;

/// `Guy::set_new_location`'s air branch: the height a `domain == 2` guy climbs toward.
pub const AIR_Z_TARGET_OFFSET: i32 = 1000;

/// …clamped to `±30` per call (`0x1E`).
pub const AIR_Z_STEP_CLAMP: i32 = 30;

/// Owner slots that reach `GuyOut::graph_inc_frame` (`cmp byte [esi+0xA1], 8; jge`).
pub const GRAPH_INC_FRAME_OWNERS: i8 = 8;

/// Hard cap on the `while cur_time >= end_time` loop.
///
/// **Retail has no cap** — it relies on `set_anim` always leaving `end_time` above
/// `cur_time`, which the missing-animation fallback of `3` guarantees. A caller supplying
/// [`AnimData`] that returns `0` frames would spin retail forever; here it trips this cap
/// and increments [`IncTimeGaps::anim_loop_capped`] instead of hanging the simulation.
pub const ANIM_LOOP_CAP: u32 = 64;

// ---------------------------------------------------------------------------
// The two data boundaries
// ---------------------------------------------------------------------------

/// The `AnimationPacket` questions `Guy::inc_time` and `Guy::set_anim` ask of the loaded
/// art.
///
/// Retail reaches this through `GraphicPieces[gpiece]->[+0x54]`, then an
/// `int[]` at `+0x20` indexed by the animation, then the `AnimMgr` load table at
/// `[0x00EBF704]` / `[0x00EBF6E8]`. We do not load `.anm` data, so the questions are a
/// trait and the shipped-data-absent answers are [`MissingAnimData`].
pub trait AnimData {
    /// `GraphicPieces[gpiece] != 0`. False takes retail's
    /// *"DO NOT IGNORE. FATAL. No Animation Packet"* path, which returns from
    /// `Guy::inc_time` immediately.
    fn packet_present(&self, _gpiece: i32) -> bool {
        true
    }

    /// The three-part test at `0x005D9FB8`: the animation is inside the packet's table,
    /// its index is non-negative and below `[0x00EBF6F8]`, and `[0x00EBF704][idx]` is
    /// non-zero. True means "this animation exists, loop it".
    fn has_anim(&self, gpiece: i32, a: i8) -> bool;

    /// `[0x00EBF6E8][idx]` — the animation's length in clock units, which is what
    /// `Guy::set_anim` stores into `end_time`. [`NO_ANIM_END_TIME`] when absent.
    fn anim_frames(&self, gpiece: i32, a: i8) -> u32;

    /// `AnimationPacket::get_anim_time` `0x00918C40` — [`NO_ANIM_TIME`] when absent.
    /// Only the unported phase-preserving tail path uses it; it is declared so that a
    /// future port of that path has somewhere to ask.
    fn anim_time(&self, _gpiece: i32, _a: i8) -> i32 {
        NO_ANIM_TIME
    }
}

/// The art is not loaded: every animation is absent, so every duration is retail's own
/// missing-asset fallback.
///
/// This is a faithful reproduction of retail's behaviour *with no animation data*, not a
/// reproduction of retail. The visible consequence is that the "loop the current
/// animation" branch at `0x005D9FDC` is never taken, so every animation runs its
/// [`NO_ANIM_END_TIME`] units and then advances along the state machine.
#[derive(Clone, Copy, Debug, Default)]
pub struct MissingAnimData;

impl AnimData for MissingAnimData {
    fn has_anim(&self, _gpiece: i32, _a: i8) -> bool {
        false
    }
    fn anim_frames(&self, _gpiece: i32, _a: i8) -> u32 {
        NO_ANIM_END_TIME
    }
}

/// A caller-supplied table, for when real per-`gpiece` animation lengths exist.
///
/// `frames[a]` of `0` means "absent" and reproduces the fallback, matching the retail test
/// (`[0x00EBF704][idx] != 0`) rather than inventing one.
#[derive(Clone, Debug, Default)]
pub struct TableAnimData {
    /// One row per `gpiece`, `NUM_PEASANT_ANIMS` entries each.
    pub frames: Vec<[u32; NUM_PEASANT_ANIMS]>,
}

impl TableAnimData {
    fn lookup(&self, gpiece: i32, a: i8) -> Option<u32> {
        let row = self.frames.get(usize::try_from(gpiece).ok()?)?;
        let v = *row.get(usize::try_from(a).ok()?)?;
        if v == 0 {
            None
        } else {
            Some(v)
        }
    }
}

impl AnimData for TableAnimData {
    fn packet_present(&self, gpiece: i32) -> bool {
        usize::try_from(gpiece)
            .map(|g| g < self.frames.len())
            .unwrap_or(false)
    }
    fn has_anim(&self, gpiece: i32, a: i8) -> bool {
        self.lookup(gpiece, a).is_some()
    }
    fn anim_frames(&self, gpiece: i32, a: i8) -> u32 {
        self.lookup(gpiece, a).unwrap_or(NO_ANIM_END_TIME)
    }
}

/// `World::get_z`-class terrain height, the one external read `Guy::set_new_location`
/// makes on the crew path.
pub trait TerrainZ {
    fn z_at(&self, x: i32, y: i32) -> i32;
}

/// The same flat-zero stand-in `tick.rs` uses for `terrain_z`, so the two agree.
#[derive(Clone, Copy, Debug, Default)]
pub struct FlatTerrain;

impl TerrainZ for FlatTerrain {
    fn z_at(&self, _x: i32, _y: i32) -> i32 {
        0
    }
}

// ---------------------------------------------------------------------------
// The unit-side inputs
// ---------------------------------------------------------------------------

/// Everything outside the guy array that `Unit::inc_time` and `Guy::inc_time` read off the
/// owning unit and its type, gathered so both ported functions take no world reference.
#[derive(Clone, Copy, Debug, Default)]
pub struct UnitAnimView {
    /// `UnitData::inside_up` `+0x82`. Negative means "not inside anything".
    pub inside_up: i16,
    /// `ObjectType::type` `+0x04`, the `TypeIndex`.
    pub type_index: i32,
    /// `UnitData::unit_masks2` `+0x6C`.
    pub unit_masks2: u32,
    /// `UnitTypeData::unit_flags2` `+0x2B8`.
    pub unit_flags2: i32,
    /// The type rules `squad_size` / `crew_size` / `domain` come from.
    pub ut: UnitTypeStats,
}

impl UnitAnimView {
    /// The whole of `Unit::inc_time`'s gate, at `0x00610B43`:
    /// `inside_up < 0 || type == SCHOLARS || type == SCHOLARSKOREAN`.
    ///
    /// A unit that is *inside* something — garrisoned in a building, loaded on a
    /// transport — has its guys' clocks stopped entirely. **Scholars are the sole
    /// exception**, and the exception is spelled as two literal type ids in the machine
    /// code, not as a rule field, so it cannot be reached from `unitrules.xml`.
    #[inline]
    pub fn animates(&self) -> bool {
        self.inside_up < 0
            || self.type_index == TYPE_SCHOLARS
            || self.type_index == TYPE_SCHOLARS_KOREAN
    }

    /// `UnitData::is_hero` `0x0046CE60`, devirtualised the way `Guy::inc_time` does it at
    /// `0x005DA026` (it compares the vtable slot against the base implementation and reads
    /// the flag inline when they match).
    #[inline]
    pub fn is_hero(&self) -> bool {
        self.unit_flags2 & UNIT_FLAGS2_HERO != 0
    }

    /// `unit_masks2 & 0x10` — the clock is frozen this tick.
    #[inline]
    pub fn anim_frozen(&self) -> bool {
        self.unit_masks2 & UNIT_MASKS2_FREEZE_ANIM != 0
    }
}

// ---------------------------------------------------------------------------
// Counters
// ---------------------------------------------------------------------------

/// Every retail sub-call this module reaches but does not perform, counted per call site.
///
/// The pattern is the tick driver's: a gap that is counted is a measured divergence; a gap
/// that is guessed at is an unmeasurable one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IncTimeGaps {
    /// `Guy::set_anim` `0x005DA300` head calls. Each is **0–3 skipped `game_random`
    /// draws** at `0x005DAC68` / `0x005DB21D` / `0x005DB339`, plus the unread animation
    /// resolution. Only the tail is ported.
    pub set_anim_head: u64,
    /// `GuyOut::graph_inc_frame` `0x005DD200` calls skipped (owners 0..7). Presentation;
    /// draws six times from `internal_random`, so skipping it does **not** move the sim
    /// stream.
    pub graph_inc_frame: u64,
    /// `Unit::execute_events` `0x0060EDC0`, the `+0x154` call the unit band makes right
    /// after `inc_time`. Unported.
    pub execute_events: u64,
    /// `GraphicPieces[gpiece] == 0` — retail's fatal *"No Animation Packet"* path.
    pub no_anim_packet: u64,
    /// `Guy::set_new_location`'s crew-propagation block, reached only when `squad_size` is
    /// 0 so that a crew guy has `guy_num == 0`. Not ported.
    pub set_new_location_recursion: u64,
    /// A guy slot inside a walked range held `None`. Retail would dereference a null
    /// pointer here, so this is our state being inconsistent, not a retail behaviour.
    pub null_guy_slot: u64,
    /// The `cur_time >= end_time` loop hit [`ANIM_LOOP_CAP`]. Degenerate animation data.
    pub anim_loop_capped: u64,
}

impl IncTimeGaps {
    fn add(&mut self, o: &IncTimeGaps) {
        self.set_anim_head += o.set_anim_head;
        self.graph_inc_frame += o.graph_inc_frame;
        self.execute_events += o.execute_events;
        self.no_anim_packet += o.no_anim_packet;
        self.set_new_location_recursion += o.set_new_location_recursion;
        self.null_guy_slot += o.null_guy_slot;
        self.anim_loop_capped += o.anim_loop_capped;
    }
}

/// What one pass over `Unit::inc_time` / `Objects::inc_time` did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IncTimeStats {
    /// `Unit::inc_time` entries, including the ones the gate rejects.
    pub units: u64,
    /// Units the gate rejected (`inside_up >= 0` and not a Scholar).
    pub units_gated_out: u64,
    /// `Guy::inc_time` calls.
    pub guys: u64,
    /// Guys that took the crew-mirror branch instead of running their own clock.
    pub guys_slaved: u64,
    /// Animation transitions applied by [`set_anim_tail`].
    pub anim_changes: u64,
    /// `queued_attack` values consumed — the number of attack animations started.
    pub attacks_started: u64,
    /// Crew guys repositioned by the guy-0 attack mirror.
    pub crew_mirrored: u64,
    pub gaps: IncTimeGaps,
}

impl IncTimeStats {
    /// Total `game_random` draws this pass failed to make, expressed as the range the
    /// three conditional sites in `Guy::set_anim` permit. Reported as a range because the
    /// exact count is decided in bytes we have not read.
    pub fn missing_sim_rng_draws(&self) -> (u64, u64) {
        (0, self.gaps.set_anim_head * 3)
    }

    fn add(&mut self, o: &IncTimeStats) {
        self.units += o.units;
        self.units_gated_out += o.units_gated_out;
        self.guys += o.guys;
        self.guys_slaved += o.guys_slaved;
        self.anim_changes += o.anim_changes;
        self.attacks_started += o.attacks_started;
        self.crew_mirrored += o.crew_mirrored;
        self.gaps.add(&o.gaps);
    }
}

// ---------------------------------------------------------------------------
// Guy::set_anim — the tail
// ---------------------------------------------------------------------------

/// The tail of `Guy::set_anim` `0x005DA300`, `0x005DB3FA..0x005DB570` [measured].
///
/// This is the part that writes checksummed state, and it is exactly:
///
/// ```text
/// cur_anim = <resolved animation>          ; 0x005DB4A5
/// anim_index_hints.len = 0                 ; 0x005DB4AF  (presentation, GuyOut +0xD0)
/// cur_time = 0                             ; 0x005DB4F1
/// end_time = anim_frames[cur_anim]         ; 0x005DB55D, or 3 when the anim is absent
/// ```
///
/// **The path not taken.** At `0x005DB3FA` retail tests a head-local against `8`, and the
/// other arm preserves phase across the change — either subtracting a head-local from
/// `cur_time` or rescaling it by the ratio of two `AnimationPacket::get_anim_time` results
/// (`imul`/`div` at `0x005DB495`). Which calls land on that arm is decided in the 4,300
/// unread bytes ahead of it. Implementing it on the guess that `8` means the walk class
/// would be inventing sim state, so it is left out and every call is counted in
/// [`IncTimeGaps::set_anim_head`].
///
/// `force` is `set_anim`'s third argument. It gates one of the head's RNG draws
/// (`0x005DB217`) and nothing in the ported tail; it is taken and recorded so call sites
/// stay transcribed rather than simplified.
pub fn set_anim_tail<A: AnimData>(
    g: &mut GuyData,
    a: i8,
    force: i32,
    data: &A,
    st: &mut IncTimeStats,
) {
    let _ = force;
    st.gaps.set_anim_head += 1;
    st.anim_changes += 1;
    g.cur_anim = a;
    g.cur_time = 0;
    g.end_time = data.anim_frames(g.gpiece, a);
}

// ---------------------------------------------------------------------------
// Guy::set_new_location — the crew call site
// ---------------------------------------------------------------------------

/// `Guy::set_new_location(x, y, snap)` `0x005D86F0`, **for the crew-guy call site only**.
///
/// The crew mirror inside `Guy::inc_time` calls it at `0x005DA204` with
/// `(des_x, des_y, 1)` on guys whose `guy_num >= squad_size`. On that path the function
/// reduces to what is written here, because both of its two large blocks are gated out:
///
/// * the collision-block update needs `domain != 2 && guy_num < squad_size` — false for
///   crew;
/// * the propagate-to-crew recursion needs `guy_num == 0` — false for crew, unless
///   `squad_size == 0`, which is counted as [`IncTimeGaps::set_new_location_recursion`]
///   and skipped.
///
/// What remains is the position write and the domain-dependent height, transcribed:
/// `domain == 2` climbs toward `terrain_z + 1000` in steps clamped to `±30`; `domain == 1`
/// pins `z` to `0`; anything else takes `terrain_z` outright.
pub fn set_new_location_crew<T: TerrainZ>(
    g: &mut GuyData,
    x: i32,
    y: i32,
    snap: bool,
    ut: &UnitTypeStats,
    terrain: &T,
    st: &mut IncTimeStats,
) {
    if g.guy_num == 0 {
        st.gaps.set_new_location_recursion += 1;
    }
    g.x = x;
    g.y = y;
    match ut.domain {
        2 => {
            let mut d = terrain
                .z_at(x, y)
                .wrapping_sub(g.z)
                .wrapping_add(AIR_Z_TARGET_OFFSET);
            if d < 0 {
                if d < -AIR_Z_STEP_CLAMP {
                    d = -AIR_Z_STEP_CLAMP;
                }
            } else if d > AIR_Z_STEP_CLAMP {
                d = AIR_Z_STEP_CLAMP;
            }
            g.z = g.z.wrapping_add(d);
        }
        1 => g.z = 0,
        _ => g.z = terrain.z_at(x, y),
    }
    if snap {
        g.last_x = x;
        g.last_y = y;
        g.last_z = g.z;
    }
}

// ---------------------------------------------------------------------------
// Guy::inc_time
// ---------------------------------------------------------------------------

/// `Guy::inc_time` `0x005D9E10` [measured, 1,251 bytes, transcribed instruction by
/// instruction].
///
/// The animation clock, and — because `queued_attack` is consumed here and nowhere else on
/// this path — **the thing that actually starts an attack animation**.
///
/// ```text
/// step = ((guy_flags & 4) && class(cur_anim) == ATTACK) ? 2 : 1
/// inc  = (unit.unit_masks2 & 0x10) ? 0 : step
///
/// if guy_num >= squad_size && class(cur_anim) != WALK:      # a crew guy, not walking
///     cur_anim = guys[0].cur_anim                            # mirror the squad leader
///     cur_time = guys[0].cur_time
/// else:
///     last_time = cur_time
///     cur_time += inc
///     while cur_time >= end_time:                            # unsigned
///         if packet missing: FATAL, return
///         if the animation exists:      set_anim(cur_anim, 1)         # loop it
///         elif class(cur_anim) == ATTACK:
///             set_anim(CHAR_DEFAULT, 0)
///             if !unit.is_hero() and queued_attack:
///                 q = queued_attack; queued_attack = 0
///                 set_anim(q > 1 ? q : CHAR_ATTACK2, q > 1 ? 0 : 1)
///         else:  set_anim(cur_anim == CHAR_ATTACKWALK ? CHAR_WALK : CHAR_DEFAULT, 1)
///
///     if class(cur_anim) != ATTACK and queued_attack and cur_anim != CHAR_WALK:
///         q = queued_attack; queued_attack = 0
///         set_anim(q > 1 ? q : CHAR_ATTACK1, q > 1 ? 0 : 1)
///
///     if guy_num == 0 and class(cur_anim) == ATTACK:         # drag the crew along
///         for k in squad_size .. guys.length:
///             guys[k].set_new_location(des_x, des_y, 1)
///             guys[k].cur_anim = cur_anim
///             guys[k].cur_time = cur_time
///
/// if who < 8: GuyOut::graph_inc_frame()                      # presentation
/// ```
///
/// Two details that are easy to get backwards and are asserted in the tests:
///
/// * `CHAR_ATTACK1` starts a *fresh* attack (the post-loop trigger) while `CHAR_ATTACK2`
///   continues one whose animation just ended (the in-loop trigger). A `queued_attack`
///   above 1 is used verbatim as the animation instead, which is how `CHAR_ATTACK3` and
///   `CHAR_ATTACKSPECIAL` are reached.
/// * the crew mirror repositions **before** copying the animation, and it copies
///   `cur_time` too, so crew bodies stay frame-locked to the squad leader mid-swing.
pub(crate) fn guy_inc_time_research_partial<A: AnimData, T: TerrainZ>(
    unit: &UnitAnimView,
    guys: &mut UnitGuys,
    i: usize,
    data: &A,
    terrain: &T,
    st: &mut IncTimeStats,
) {
    let Some(Some(g0)) = guys.guys.get(i) else {
        st.gaps.null_guy_slot += 1;
        return;
    };
    st.guys += 1;

    let cur = g0.cur_anim;
    let flags = g0.guy_flags;
    let guy_num = g0.guy_num as i32;
    let who = g0.who;

    // 0x005D9E35: the double-step gate.
    let step: u32 = if flags & GUY_FLAG_DOUBLE_ANIM_STEP != 0 && anim_class(cur) == CLASS_ATTACK {
        2
    } else {
        1
    };
    // 0x005D9E7E: `test [unit+0x6C], 0x10` — `cmove` leaves the increment at zero.
    let inc: u32 = if unit.anim_frozen() { 0 } else { step };

    // 0x005D9E92 / 0x005D9EA7: crew guys that are not walking mirror the squad leader.
    if guy_num >= unit.ut.squad_size && anim_class(cur) != CLASS_WALK {
        st.guys_slaved += 1;
        let Some(Some(lead)) = guys.guys.first() else {
            st.gaps.null_guy_slot += 1;
            return;
        };
        let (lead_anim, lead_time) = (lead.cur_anim, lead.cur_time);
        let g = guys.guys[i].as_mut().expect("slot checked above");
        g.cur_anim = lead_anim;
        g.cur_time = lead_time;
        tail_graph_inc_frame(who, st);
        return;
    }

    // 0x005D9F71: the clock.
    {
        let g = guys.guys[i].as_mut().expect("slot checked above");
        g.last_time = g.cur_time as i32;
        g.cur_time = g.cur_time.wrapping_add(inc);
    }

    let mut spins = 0u32;
    loop {
        let (cur_time, end_time, gpiece) = {
            let g = guys.guys[i].as_ref().expect("slot checked above");
            (g.cur_time, g.end_time, g.gpiece)
        };
        if cur_time < end_time {
            break;
        }
        if spins >= ANIM_LOOP_CAP {
            st.gaps.anim_loop_capped += 1;
            break;
        }
        spins += 1;

        // 0x005D9FA4: the fatal "No Animation Packet" path returns from the function.
        if !data.packet_present(gpiece) {
            st.gaps.no_anim_packet += 1;
            return;
        }

        let cur = guys.guys[i].as_ref().expect("slot checked above").cur_anim;
        if data.has_anim(gpiece, cur) {
            // 0x005D9FDC: the animation exists, restart it.
            let g = guys.guys[i].as_mut().expect("slot checked above");
            set_anim_tail(g, cur, 1, data, st);
        } else if anim_class(cur) == CLASS_ATTACK {
            // 0x005D9FF0: an attack animation ran out.
            let queued = {
                let g = guys.guys[i].as_mut().expect("slot checked above");
                set_anim_tail(g, anim::CHAR_DEFAULT, 0, data, st);
                g.queued_attack
            };
            if !unit.is_hero() && queued != 0 {
                let g = guys.guys[i].as_mut().expect("slot checked above");
                g.queued_attack = 0;
                st.attacks_started += 1;
                if queued > 1 {
                    set_anim_tail(g, queued, 0, data, st);
                } else {
                    set_anim_tail(g, anim::CHAR_ATTACK2, 1, data, st);
                }
            }
        } else {
            // 0x005DA06B: an attackwalk decays to a walk, everything else to the default.
            let next = if cur == anim::CHAR_ATTACKWALK {
                anim::CHAR_WALK
            } else {
                anim::CHAR_DEFAULT
            };
            let g = guys.guys[i].as_mut().expect("slot checked above");
            set_anim_tail(g, next, 1, data, st);
        }
    }

    // 0x005DA08D: the fresh-attack trigger.
    let (cur, queued) = {
        let g = guys.guys[i].as_ref().expect("slot checked above");
        (g.cur_anim, g.queued_attack)
    };
    if anim_class(cur) != CLASS_ATTACK && queued != 0 && cur != anim::CHAR_WALK {
        let g = guys.guys[i].as_mut().expect("slot checked above");
        g.queued_attack = 0;
        st.attacks_started += 1;
        if queued > 1 {
            set_anim_tail(g, queued, 0, data, st);
        } else {
            set_anim_tail(g, anim::CHAR_ATTACK1, 1, data, st);
        }
    }

    // 0x005DA167: guy 0 attacking drags every crew guy to its desired position and
    // frame-locks it to the leader.
    let (guy_num, cur, cur_time) = {
        let g = guys.guys[i].as_ref().expect("slot checked above");
        (g.guy_num, g.cur_anim, g.cur_time)
    };
    if guy_num == 0 && anim_class(cur) == CLASS_ATTACK {
        let start = unit.ut.squad_size.max(0) as usize;
        let mut k = start;
        while k < guys.guys.len() {
            if let Some(c) = guys.guys[k].as_mut() {
                let (dx, dy) = (c.des_x, c.des_y);
                set_new_location_crew(c, dx, dy, true, &unit.ut, terrain, st);
                c.cur_anim = cur;
                c.cur_time = cur_time;
                st.crew_mirrored += 1;
            } else {
                st.gaps.null_guy_slot += 1;
            }
            k += 1;
        }
    }

    tail_graph_inc_frame(who, st);
}

/// `cmp byte [esi+0xA1], 8; jge` at `0x005DA2D2` — owners 0..7 only.
#[inline]
fn tail_graph_inc_frame(who: i8, st: &mut IncTimeStats) {
    if who < GRAPH_INC_FRAME_OWNERS {
        st.gaps.graph_inc_frame += 1;
    }
}

// ---------------------------------------------------------------------------
// Unit::inc_time
// ---------------------------------------------------------------------------

/// `Unit::inc_time` `0x00610B40` [measured, 122 bytes, transcribed in full].
///
/// A pure dispatcher. It is 122 bytes and contains exactly one gate and two loops:
///
/// ```text
/// if inside_up >= 0 and type != SCHOLARS and type != SCHOLARSKOREAN: return
/// for i in 0 .. guy_mark:                 # guy_mark re-read every iteration
///     guys[i]->inc_time()
/// for i in squad_size .. guys.length:     # length re-read every iteration
///     guys[i]->inc_time()
/// ```
///
/// The index space is the one `Unit::set_type` `0x00612FA0` builds: `[0, guy_mark)` are the
/// live squad soldiers, `[guy_mark, squad_size)` are dead slots that hold null pointers,
/// and `[squad_size, length)` are crew. **The dead middle band is skipped**, which is why
/// the loop is written as two ranges instead of one — a port that walks `0..length` would
/// tick corpses.
///
/// Both bounds are re-read from the object on every iteration in retail (`movsx eax,
/// [esi+0xB5]` at `0x00610B7E`, `cmp edi, [esi+0xE8]` at `0x00610BAF`), so a guy that dies
/// inside the pass shortens the pass. That is reproduced here rather than hoisted.
pub(crate) fn unit_inc_time_research_partial<A: AnimData, T: TerrainZ>(
    unit: &UnitAnimView,
    guys: &mut UnitGuys,
    data: &A,
    terrain: &T,
    st: &mut IncTimeStats,
) {
    st.units += 1;
    if !unit.animates() {
        st.units_gated_out += 1;
        return;
    }
    let mut i: i32 = 0;
    while i < guys.guy_mark as i32 {
        guy_inc_time_research_partial(unit, guys, i as usize, data, terrain, st);
        i += 1;
    }
    let mut i: i32 = unit.ut.squad_size;
    while i >= 0 && (i as usize) < guys.guys.len() {
        guy_inc_time_research_partial(unit, guys, i as usize, data, terrain, st);
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// Objects::inc_time — the traversal
// ---------------------------------------------------------------------------

/// The object traversal order of `Objects::inc_time` `0x0065DB70`, the step-15 counterpart
/// to [`ObjectRegistry::traversal_into`].
///
/// It differs from step 14's in the two ways this module's header records, and both
/// differences are asserted in the tests:
///
/// * **no owner rotation** — the ten leader records are walked in address order, so the
///   traversal is identical every frame;
/// * **no wall band** — only `[0, unit_mark)` and `[2000, build_mark)` are visited.
///
/// Emitting `(slot, band, o, row)` matches the existing traversal's shape so a caller can
/// drive both steps off one loop body.
pub fn inc_time_traversal(reg: &ObjectRegistry, out: &mut Vec<(usize, Band, u32, u32)>) {
    out.clear();
    for s in 0..OWNER_SLOTS {
        if !reg.is_active(s) {
            continue;
        }
        for (k, &row) in reg.slot(s).band(Band::Unit).iter().enumerate() {
            out.push((s, Band::Unit, k as u32, row));
        }
        for (k, &row) in reg.slot(s).band(Band::Build).iter().enumerate() {
            out.push((s, Band::Build, BUILD_BAND_BASE + k as u32, row));
        }
    }
}

/// One `Objects::inc_time` unit pass over a caller-held population.
///
/// `units` is indexed by the `row` [`inc_time_traversal`] yields; a row missing from the
/// map is skipped rather than assumed. The `+0x154` `Unit::execute_events` call the unit
/// band makes after every `inc_time` is counted, not performed.
pub(crate) fn objects_inc_time_units_research_partial<A: AnimData, T: TerrainZ>(
    reg: &ObjectRegistry,
    order: &mut Vec<(usize, Band, u32, u32)>,
    mut unit_at: impl FnMut(u32) -> Option<(UnitAnimView, *mut UnitGuys)>,
    data: &A,
    terrain: &T,
) -> IncTimeStats {
    inc_time_traversal(reg, order);
    let mut st = IncTimeStats::default();
    for &(_, band, _, row) in order.iter() {
        if band != Band::Unit {
            continue;
        }
        if let Some((view, guys)) = unit_at(row) {
            let mut one = IncTimeStats::default();
            // SAFETY: the closure hands back a pointer into the caller's own storage for
            // exactly one row, and no two rows in a traversal share a row index.
            unit_inc_time_research_partial(&view, unsafe { &mut *guys }, data, terrain, &mut one);
            one.gaps.execute_events += 1;
            st.add(&one);
        }
    }
    st
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::objects::{Band, ObjectRegistry};

    #[test]
    fn recovered_inc_time_driver_is_fail_closed_for_runtime_fidelity() {
        assert!(!RUNTIME_FIDELITY_READY);
        assert!(!RUNTIME_FIDELITY_BLOCKERS.is_empty());
        assert!(RUNTIME_FIDELITY_BLOCKERS.iter().any(|s| s.contains("set_anim head")));
        assert!(RUNTIME_FIDELITY_BLOCKERS.iter().any(|s| s.contains("Farms::inc_time")));
    }

    fn ut(squad: i32, crew: i32) -> UnitTypeStats {
        UnitTypeStats {
            squad_size: squad,
            crew_size: crew,
            ..Default::default()
        }
    }

    fn view(squad: i32, crew: i32) -> UnitAnimView {
        UnitAnimView {
            inside_up: -1,
            type_index: 100,
            unit_masks2: 0,
            unit_flags2: 0,
            ut: ut(squad, crew),
        }
    }

    fn squad(squad: i32, crew: i32) -> UnitGuys {
        let mut g = UnitGuys::spawn_full(100, 0, 0, &ut(squad, crew));
        g.guy_mark = squad as i8;
        g
    }

    /// The whole gate of `Unit::inc_time`, including the two literal type ids.
    #[test]
    fn a_garrisoned_unit_freezes_unless_it_is_a_scholar() {
        let mut v = view(2, 0);
        assert!(v.animates(), "inside_up < 0 means not inside anything");
        v.inside_up = 7;
        assert!(!v.animates());
        v.type_index = TYPE_SCHOLARS;
        assert!(v.animates(), "SCHOLARS animate while garrisoned");
        v.type_index = TYPE_SCHOLARS_KOREAN;
        assert!(v.animates());
        v.type_index = TYPE_SCHOLARS - 1;
        assert!(!v.animates());

        // and end to end: a garrisoned non-scholar runs no guy at all.
        let mut g = squad(2, 0);
        let mut st = IncTimeStats::default();
        v.type_index = 100;
        unit_inc_time_research_partial(&v, &mut g, &MissingAnimData, &FlatTerrain, &mut st);
        assert_eq!(st.guys, 0);
        assert_eq!(st.units_gated_out, 1);
    }

    /// The table is the load-bearing transcription in this file; assert it literally, and
    /// assert the three families the machine code actually compares against.
    #[test]
    fn the_anim_class_table_is_the_rdata_table() {
        assert_eq!(ANIM_CLASS.len(), NUM_PEASANT_ANIMS);
        assert_eq!(
            ANIM_CLASS,
            [
                0, 0, 0, 0, 0, 0, 0, 8, 8, 8, 10, 12, 12, 12, 12, 15, 15, 15, 15, 15, 15, 21, 22,
                23, 24, 25, 8, 27, 8, 29, 8, 31, 8, 33, 34, 35, 36, 37
            ]
        );
        // The walk family is seven animations, not three: carrying wood and ore count.
        let walk: Vec<i8> = (0..NUM_PEASANT_ANIMS as i8)
            .filter(|&a| anim_class(a) == CLASS_WALK)
            .collect();
        assert_eq!(walk, vec![7, 8, 9, 26, 28, 30, 32]);
        let attack: Vec<i8> = (0..NUM_PEASANT_ANIMS as i8)
            .filter(|&a| anim_class(a) == CLASS_ATTACK)
            .collect();
        assert_eq!(attack, vec![11, 12, 13, 14]);
        let death: Vec<i8> = (0..NUM_PEASANT_ANIMS as i8)
            .filter(|&a| anim_class(a) == CLASS_DEATH)
            .collect();
        assert_eq!(death, vec![15, 16, 17, 18, 19, 20]);
        // Retail reads out of bounds here; we must not panic and must not match a family.
        for a in [-1i8, 38, 100, i8::MIN, i8::MAX] {
            assert_eq!(anim_class(a), CLASS_OUT_OF_TABLE);
        }
    }

    /// `guy_flags & 4` doubles the step, and only for the attack family.
    #[test]
    fn the_double_step_flag_only_bites_on_attack_animations() {
        let v = view(1, 0);
        let mut g = squad(1, 0);
        {
            let s = g.guys[0].as_mut().unwrap();
            s.guy_flags = GUY_FLAG_DOUBLE_ANIM_STEP;
            s.cur_anim = anim::CHAR_ATTACK1;
            s.end_time = 100;
        }
        let mut st = IncTimeStats::default();
        unit_inc_time_research_partial(&v, &mut g, &MissingAnimData, &FlatTerrain, &mut st);
        assert_eq!(g.guys[0].as_ref().unwrap().cur_time, 2);

        let mut g = squad(1, 0);
        {
            let s = g.guys[0].as_mut().unwrap();
            s.guy_flags = GUY_FLAG_DOUBLE_ANIM_STEP;
            s.cur_anim = anim::CHAR_WALK;
            s.end_time = 100;
        }
        let mut st = IncTimeStats::default();
        unit_inc_time_research_partial(&v, &mut g, &MissingAnimData, &FlatTerrain, &mut st);
        assert_eq!(g.guys[0].as_ref().unwrap().cur_time, 1);
    }

    /// `unit_masks2 & 0x10` zeroes the increment but does not skip the transition test —
    /// a guy sitting exactly on its boundary still advances.
    #[test]
    fn the_freeze_mask_stops_the_clock_but_not_the_boundary() {
        let mut v = view(1, 0);
        v.unit_masks2 = UNIT_MASKS2_FREEZE_ANIM;
        let mut g = squad(1, 0);
        {
            let s = g.guys[0].as_mut().unwrap();
            s.cur_anim = anim::CHAR_IDLE1;
            s.cur_time = 5;
            s.end_time = 100;
        }
        let mut st = IncTimeStats::default();
        unit_inc_time_research_partial(&v, &mut g, &MissingAnimData, &FlatTerrain, &mut st);
        assert_eq!(g.guys[0].as_ref().unwrap().cur_time, 5, "clock frozen");
        assert_eq!(st.anim_changes, 0);

        // …but on the boundary the transition still runs.
        let mut g = squad(1, 0);
        {
            let s = g.guys[0].as_mut().unwrap();
            s.cur_anim = anim::CHAR_IDLE1;
            s.cur_time = 7;
            s.end_time = 7;
        }
        let mut st = IncTimeStats::default();
        unit_inc_time_research_partial(&v, &mut g, &MissingAnimData, &FlatTerrain, &mut st);
        assert_eq!(st.anim_changes, 1);
        assert_eq!(g.guys[0].as_ref().unwrap().cur_anim, anim::CHAR_DEFAULT);
        assert_eq!(g.guys[0].as_ref().unwrap().end_time, NO_ANIM_END_TIME);
    }

    /// The post-loop trigger: a queued attack of 1 starts `CHAR_ATTACK1`.
    #[test]
    fn a_queued_attack_starts_char_attack1() {
        let v = view(1, 0);
        let mut g = squad(1, 0);
        {
            let s = g.guys[0].as_mut().unwrap();
            s.cur_anim = anim::CHAR_IDLE1;
            s.end_time = 100;
            s.queued_attack = 1;
        }
        let mut st = IncTimeStats::default();
        unit_inc_time_research_partial(&v, &mut g, &MissingAnimData, &FlatTerrain, &mut st);
        let s = g.guys[0].as_ref().unwrap();
        assert_eq!(s.cur_anim, anim::CHAR_ATTACK1);
        assert_eq!(s.queued_attack, 0, "consumed");
        assert_eq!(s.cur_time, 0, "set_anim tail zeroes the clock");
        assert_eq!(st.attacks_started, 1);
    }

    /// A queued value above 1 is used verbatim — that is how ATTACK3/ATTACKSPECIAL play.
    #[test]
    fn a_queued_attack_above_one_is_the_animation_index() {
        let v = view(1, 0);
        let mut g = squad(1, 0);
        {
            let s = g.guys[0].as_mut().unwrap();
            s.cur_anim = anim::CHAR_IDLE1;
            s.end_time = 100;
            s.queued_attack = anim::CHAR_ATTACKSPECIAL;
        }
        let mut st = IncTimeStats::default();
        unit_inc_time_research_partial(&v, &mut g, &MissingAnimData, &FlatTerrain, &mut st);
        assert_eq!(
            g.guys[0].as_ref().unwrap().cur_anim,
            anim::CHAR_ATTACKSPECIAL
        );
    }

    /// The trigger is suppressed while already attacking, and while walking.
    #[test]
    fn the_attack_trigger_is_suppressed_mid_attack_and_mid_walk() {
        for (start, keep) in [(anim::CHAR_ATTACK3, true), (anim::CHAR_WALK, true)] {
            let v = view(1, 0);
            let mut g = squad(1, 0);
            {
                let s = g.guys[0].as_mut().unwrap();
                s.cur_anim = start;
                s.end_time = 100;
                s.queued_attack = 1;
            }
            let mut st = IncTimeStats::default();
            unit_inc_time_research_partial(&v, &mut g, &MissingAnimData, &FlatTerrain, &mut st);
            let s = g.guys[0].as_ref().unwrap();
            assert_eq!(s.cur_anim, start);
            assert_eq!(s.queued_attack, if keep { 1 } else { 0 });
            assert_eq!(st.attacks_started, 0);
        }
    }

    /// The in-loop chain: an attack whose animation ran out re-arms as `CHAR_ATTACK2`,
    /// and a hero's does not re-arm at all.
    #[test]
    fn an_expiring_attack_chains_to_char_attack2_unless_hero() {
        let v = view(1, 0);
        let mut g = squad(1, 0);
        {
            let s = g.guys[0].as_mut().unwrap();
            s.cur_anim = anim::CHAR_ATTACK1;
            s.cur_time = 9;
            s.end_time = 3;
            s.queued_attack = 1;
        }
        let mut st = IncTimeStats::default();
        unit_inc_time_research_partial(&v, &mut g, &MissingAnimData, &FlatTerrain, &mut st);
        assert_eq!(g.guys[0].as_ref().unwrap().cur_anim, anim::CHAR_ATTACK2);

        let mut hero = view(1, 0);
        hero.unit_flags2 = UNIT_FLAGS2_HERO;
        let mut g = squad(1, 0);
        {
            let s = g.guys[0].as_mut().unwrap();
            s.cur_anim = anim::CHAR_ATTACK1;
            s.cur_time = 9;
            s.end_time = 3;
            s.queued_attack = 1;
        }
        let mut st = IncTimeStats::default();
        unit_inc_time_research_partial(&hero, &mut g, &MissingAnimData, &FlatTerrain, &mut st);
        let s = g.guys[0].as_ref().unwrap();
        // The attack decays to the default; the post-loop trigger then fires because the
        // hero gate only guards the in-loop re-arm.
        assert_eq!(s.cur_anim, anim::CHAR_ATTACK1);
        assert_eq!(s.queued_attack, 0);
    }

    /// `CHAR_ATTACKWALK` decays to `CHAR_WALK`; everything else decays to `CHAR_DEFAULT`.
    #[test]
    fn attackwalk_decays_to_walk() {
        for (start, want) in [
            (anim::CHAR_ATTACKWALK, anim::CHAR_WALK),
            (anim::CHAR_CHOP_WOOD, anim::CHAR_DEFAULT),
            (anim::CHAR_WALK_WITH_WOOD, anim::CHAR_DEFAULT),
        ] {
            let v = view(1, 0);
            let mut g = squad(1, 0);
            {
                let s = g.guys[0].as_mut().unwrap();
                s.cur_anim = start;
                s.cur_time = 5;
                s.end_time = 3;
            }
            let mut st = IncTimeStats::default();
            unit_inc_time_research_partial(&v, &mut g, &MissingAnimData, &FlatTerrain, &mut st);
            assert_eq!(g.guys[0].as_ref().unwrap().cur_anim, want, "from {start}");
        }
    }

    /// Crew guys mirror guy 0 unless they are walking — and the walk family is the whole
    /// seven-animation family, not just `CHAR_WALK`.
    #[test]
    fn crew_guys_mirror_guy_zero_unless_they_are_walking() {
        let v = view(2, 1);
        let mut g = squad(2, 1);
        assert_eq!(g.guys.len(), 3);
        {
            let s = g.guys[0].as_mut().unwrap();
            s.cur_anim = anim::CHAR_CHOP_WOOD;
            s.cur_time = 4;
            s.end_time = 100;
        }
        {
            let c = g.guys[2].as_mut().unwrap();
            c.cur_anim = anim::CHAR_IDLE2;
            c.cur_time = 99;
            c.end_time = 100;
        }
        let mut st = IncTimeStats::default();
        unit_inc_time_research_partial(&v, &mut g, &MissingAnimData, &FlatTerrain, &mut st);
        let c = g.guys[2].as_ref().unwrap();
        assert_eq!(c.cur_anim, anim::CHAR_CHOP_WOOD, "mirrored");
        assert_eq!(
            c.cur_time, 5,
            "and frame-locked to the post-increment leader"
        );
        assert_eq!(st.guys_slaved, 1);

        // A crew guy in the walk family runs its own clock instead.
        let mut g = squad(2, 1);
        {
            let s = g.guys[0].as_mut().unwrap();
            s.cur_anim = anim::CHAR_CHOP_WOOD;
            s.end_time = 100;
        }
        {
            let c = g.guys[2].as_mut().unwrap();
            c.cur_anim = anim::CHAR_WALK_TO_ORE;
            c.cur_time = 3;
            c.end_time = 100;
        }
        let mut st = IncTimeStats::default();
        unit_inc_time_research_partial(&v, &mut g, &MissingAnimData, &FlatTerrain, &mut st);
        let c = g.guys[2].as_ref().unwrap();
        assert_eq!(c.cur_anim, anim::CHAR_WALK_TO_ORE);
        assert_eq!(c.cur_time, 4);
        assert_eq!(st.guys_slaved, 0);
    }

    /// Guy 0 attacking drags the crew to `des_x`/`des_y` and frame-locks them, in that
    /// order.
    #[test]
    fn guy_zero_attacking_drags_the_crew() {
        let v = view(1, 2);
        let mut g = squad(1, 2);
        {
            let s = g.guys[0].as_mut().unwrap();
            s.cur_anim = anim::CHAR_IDLE1;
            s.end_time = 100;
            s.queued_attack = 1;
        }
        for k in 1..3 {
            let c = g.guys[k].as_mut().unwrap();
            c.des_x = 4800 + k as i32;
            c.des_y = 9600;
            c.x = 0;
            c.y = 0;
            c.cur_anim = anim::CHAR_IDLE3;
            c.end_time = 100;
        }
        let mut st = IncTimeStats::default();
        unit_inc_time_research_partial(&v, &mut g, &MissingAnimData, &FlatTerrain, &mut st);
        assert_eq!(g.guys[0].as_ref().unwrap().cur_anim, anim::CHAR_ATTACK1);
        for k in 1..3 {
            let c = g.guys[k].as_ref().unwrap();
            assert_eq!(c.x, 4800 + k as i32, "moved to des_x");
            assert_eq!(c.last_x, c.x, "snap = 1");
            assert_eq!(c.cur_anim, anim::CHAR_ATTACK1, "frame-locked");
            assert_eq!(c.cur_time, 0);
        }
        assert!(st.crew_mirrored >= 2);
    }

    /// The dead middle band `[guy_mark, squad_size)` is never ticked.
    #[test]
    fn the_dead_squad_band_is_skipped() {
        let v = view(4, 1);
        let mut g = squad(4, 1);
        g.guy_mark = 2;
        // slots 2 and 3 are dead in retail; make them explicitly absent.
        g.guys[2] = None;
        g.guys[3] = None;
        let mut st = IncTimeStats::default();
        unit_inc_time_research_partial(&v, &mut g, &MissingAnimData, &FlatTerrain, &mut st);
        assert_eq!(
            st.guys, 3,
            "guys 0,1 from the live band and 4 from the crew"
        );
        assert_eq!(st.gaps.null_guy_slot, 0, "the dead band was never entered");
    }

    /// A guy dying mid-pass shortens the pass, because retail re-reads `guy_mark`.
    #[test]
    fn guy_mark_is_re_read_every_iteration() {
        // Hand-drive the loop shape: shrinking guy_mark between iterations must be
        // observed. `unit_inc_time` reads `guys.guy_mark` inside the condition, so a
        // shrink applied by any callee takes effect immediately.
        let v = view(3, 0);
        let mut g = squad(3, 0);
        for k in 0..3 {
            let s = g.guys[k].as_mut().unwrap();
            s.end_time = 100;
        }
        let mut st = IncTimeStats::default();
        g.guy_mark = 1;
        unit_inc_time_research_partial(&v, &mut g, &MissingAnimData, &FlatTerrain, &mut st);
        assert_eq!(st.guys, 1);
        assert_eq!(g.guys[1].as_ref().unwrap().cur_time, 0, "never ticked");
    }

    /// `MissingAnimData` reproduces retail's own missing-asset fallbacks.
    #[test]
    fn missing_animation_data_uses_retails_fallbacks() {
        let d = MissingAnimData;
        assert!(!d.has_anim(0, anim::CHAR_WALK));
        assert_eq!(d.anim_frames(0, anim::CHAR_WALK), 3);
        assert_eq!(d.anim_time(0, anim::CHAR_WALK), 200);
        let mut g = GuyData::default();
        let mut st = IncTimeStats::default();
        set_anim_tail(&mut g, anim::CHAR_JOG, 1, &d, &mut st);
        assert_eq!(g.cur_anim, anim::CHAR_JOG);
        assert_eq!(g.cur_time, 0);
        assert_eq!(g.end_time, NO_ANIM_END_TIME);
        assert_eq!(st.gaps.set_anim_head, 1);
    }

    /// A present animation loops in place instead of decaying — the branch
    /// `MissingAnimData` can never reach.
    #[test]
    fn a_present_animation_loops_instead_of_decaying() {
        let mut frames = [0u32; NUM_PEASANT_ANIMS];
        frames[anim::CHAR_CHOP_WOOD as usize] = 12;
        let data = TableAnimData {
            frames: vec![frames],
        };
        let v = view(1, 0);
        let mut g = squad(1, 0);
        {
            let s = g.guys[0].as_mut().unwrap();
            s.gpiece = 0;
            s.cur_anim = anim::CHAR_CHOP_WOOD;
            s.cur_time = 12;
            s.end_time = 12;
        }
        let mut st = IncTimeStats::default();
        unit_inc_time_research_partial(&v, &mut g, &data, &FlatTerrain, &mut st);
        let s = g.guys[0].as_ref().unwrap();
        assert_eq!(s.cur_anim, anim::CHAR_CHOP_WOOD, "looped, not decayed");
        assert_eq!(s.cur_time, 0);
        assert_eq!(s.end_time, 12);
    }

    /// A missing graphic packet is retail's fatal path and returns immediately, skipping
    /// the attack trigger that would otherwise have fired.
    #[test]
    fn a_missing_packet_returns_immediately() {
        let data = TableAnimData { frames: vec![] };
        let v = view(1, 0);
        let mut g = squad(1, 0);
        {
            let s = g.guys[0].as_mut().unwrap();
            s.cur_anim = anim::CHAR_IDLE1;
            s.cur_time = 5;
            s.end_time = 1;
            s.queued_attack = 1;
        }
        let mut st = IncTimeStats::default();
        unit_inc_time_research_partial(&v, &mut g, &data, &FlatTerrain, &mut st);
        assert_eq!(st.gaps.no_anim_packet, 1);
        assert_eq!(g.guys[0].as_ref().unwrap().queued_attack, 1, "not consumed");
        assert_eq!(st.gaps.graph_inc_frame, 0, "the tail is skipped too");
    }

    /// Degenerate data cannot hang the simulation.
    #[test]
    fn a_zero_length_animation_trips_the_cap_instead_of_spinning() {
        // Every animation absent AND end_time forced to 0 by a table of zeroes: the
        // fallback is 3, so the loop terminates; force the pathological case directly.
        struct ZeroFrames;
        impl AnimData for ZeroFrames {
            fn has_anim(&self, _g: i32, _a: i8) -> bool {
                false
            }
            fn anim_frames(&self, _g: i32, _a: i8) -> u32 {
                0
            }
        }
        let v = view(1, 0);
        let mut g = squad(1, 0);
        {
            let s = g.guys[0].as_mut().unwrap();
            s.end_time = 0;
        }
        let mut st = IncTimeStats::default();
        unit_inc_time_research_partial(&v, &mut g, &ZeroFrames, &FlatTerrain, &mut st);
        assert_eq!(st.gaps.anim_loop_capped, 1);
    }

    /// The `+0x154` sibling call and the presentation tail are counted, never performed.
    #[test]
    fn every_skipped_retail_call_is_counted() {
        let v = view(2, 0);
        let mut g = squad(2, 0);
        for k in 0..2 {
            g.guys[k].as_mut().unwrap().end_time = 100;
        }
        let mut st = IncTimeStats::default();
        unit_inc_time_research_partial(&v, &mut g, &MissingAnimData, &FlatTerrain, &mut st);
        assert_eq!(st.gaps.graph_inc_frame, 2, "owner 0 is below 8");
        assert_eq!(
            st.missing_sim_rng_draws(),
            (0, 0),
            "no transitions, no draws"
        );

        let mut g = squad(1, 0);
        {
            let s = g.guys[0].as_mut().unwrap();
            s.cur_anim = anim::CHAR_IDLE1;
            s.end_time = 100;
            s.queued_attack = 1;
        }
        let mut st = IncTimeStats::default();
        unit_inc_time_research_partial(&view(1, 0), &mut g, &MissingAnimData, &FlatTerrain, &mut st);
        assert_eq!(st.gaps.set_anim_head, 1);
        assert_eq!(st.missing_sim_rng_draws(), (0, 3));
    }

    /// Owners 8 and 9 (nature) never reach `graph_inc_frame`.
    #[test]
    fn nature_owners_skip_the_presentation_tail() {
        let v = view(1, 0);
        let mut g = squad(1, 0);
        {
            let s = g.guys[0].as_mut().unwrap();
            s.who = 9;
            s.end_time = 100;
        }
        let mut st = IncTimeStats::default();
        unit_inc_time_research_partial(&v, &mut g, &MissingAnimData, &FlatTerrain, &mut st);
        assert_eq!(st.gaps.graph_inc_frame, 0);
    }

    /// Step 15's traversal is not step 14's: no rotation, and no wall band.
    #[test]
    fn step_15_does_not_rotate_and_never_visits_walls() {
        let mut r = ObjectRegistry::new();
        for s in 0..OWNER_SLOTS {
            r.insert(s, Band::Unit, s as u32);
            r.insert(s, Band::Build, 100 + s as u32);
            r.insert(s, Band::Wall, 200 + s as u32);
        }
        let mut a = Vec::new();
        inc_time_traversal(&r, &mut a);
        let mut b = Vec::new();
        inc_time_traversal(&r, &mut b);
        assert_eq!(a, b, "the traversal does not depend on the frame at all");

        let slots: Vec<usize> = a
            .iter()
            .filter(|e| e.1 == Band::Unit)
            .map(|e| e.0)
            .collect();
        assert_eq!(
            slots,
            (0..OWNER_SLOTS).collect::<Vec<_>>(),
            "fixed 0..9, unlike Objects::process_all"
        );
        assert_eq!(a.iter().filter(|e| e.1 == Band::Wall).count(), 0);
        assert_eq!(
            a.iter().filter(|e| e.1 == Band::Build).count(),
            OWNER_SLOTS,
            "all ten slots get their build band, unlike step 14's eight"
        );
        // step 14 rotates; step 15 does not. Assert the two really differ.
        let mut t14 = Vec::new();
        r.traversal_into(3, &mut t14);
        let s14: Vec<usize> = t14
            .iter()
            .filter(|e| e.1 == Band::Unit)
            .map(|e| e.0)
            .collect();
        assert_ne!(s14, slots);
    }

    /// Two identical populations tick identically, and ticking is a pure function of the
    /// state handed in.
    #[test]
    fn stepping_is_deterministic() {
        let v = view(3, 2);
        let mut a = squad(3, 2);
        for k in 0..a.guys.len() {
            let s = a.guys[k].as_mut().unwrap();
            s.cur_anim = (k as i8) % 12;
            s.end_time = (k as u32 % 5) + 1;
            s.queued_attack = (k as i8) % 3;
            s.des_x = 100 * k as i32;
        }
        let mut b = a.clone();
        let mut sa = IncTimeStats::default();
        let mut sb = IncTimeStats::default();
        for _ in 0..40 {
            unit_inc_time_research_partial(&v, &mut a, &MissingAnimData, &FlatTerrain, &mut sa);
            unit_inc_time_research_partial(&v, &mut b, &MissingAnimData, &FlatTerrain, &mut sb);
        }
        assert_eq!(a, b);
        assert_eq!(sa, sb);
    }

    /// The crew placement branch is the domain switch, transcribed.
    #[test]
    fn set_new_location_takes_the_domain_branch() {
        struct Hill(i32);
        impl TerrainZ for Hill {
            fn z_at(&self, _x: i32, _y: i32) -> i32 {
                self.0
            }
        }
        let mut st = IncTimeStats::default();

        let mut g = GuyData {
            guy_num: 1,
            ..Default::default()
        };
        set_new_location_crew(&mut g, 10, 20, true, &ut(1, 1), &Hill(77), &mut st);
        assert_eq!((g.x, g.y, g.z), (10, 20, 77), "land takes terrain z");
        assert_eq!((g.last_x, g.last_y, g.last_z), (10, 20, 77));

        let mut water = ut(1, 1);
        water.domain = 1;
        let mut g = GuyData {
            guy_num: 1,
            z: 500,
            ..Default::default()
        };
        set_new_location_crew(&mut g, 1, 2, false, &water, &Hill(77), &mut st);
        assert_eq!(g.z, 0, "water pins z");
        assert_eq!(g.last_x, 0, "snap = 0 leaves last_* alone");

        let mut air = ut(1, 1);
        air.domain = 2;
        let mut g = GuyData {
            guy_num: 1,
            z: 0,
            ..Default::default()
        };
        set_new_location_crew(&mut g, 1, 2, false, &air, &Hill(0), &mut st);
        assert_eq!(g.z, AIR_Z_STEP_CLAMP, "air climbs, clamped to 30");
        assert_eq!(st.gaps.set_new_location_recursion, 0);
    }
}
