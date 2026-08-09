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
//!   `0x0060EDC0`**, that the building band does not. Its exact 131-byte dispatcher is
//!   [`unit_execute_events`]. The exact Guy-side body is [`guy_execute_events`]; shipped
//!   graphics-event tables and the RELEASE / RELEASE_PLANE sink integrations remain blockers.
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
//! `0x005DB339`, each `Random::get(0, 0xFFFF)` followed by `% 100` [measured]. The sites
//! sit in mutually exclusive requested-class arms (`0`, `12`, and `8` respectively), so
//! one `set_anim` **activation** draws **zero or one time**, never three. The captain/uber
//! path can recursively activate `set_anim` at `0x005DAB80`, so one root call has no finite
//! draw bound established here. Whether any activation reaches its one site is data-dependent.
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
//! The partial `Guy`/`Unit` drivers draw nothing. They **count** every skipped root call in
//! [`IncTimeGaps`], the way the tick driver counts the anti-air dud roll: drawing the wrong
//! number of values is strictly worse than drawing none and reporting it. The isolated
//! [`set_anim_rng_arm`] helper does advance a supplied RNG, but is not wired into those
//! partial drivers because their missing head state cannot select an arm honestly.
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
//! | `Unit::inc_time` | `0x00610B40` | 122 | dispatcher transcribed; called guy path is partial |
//! | `Guy::inc_time` | `0x005D9E10` | 1251 | outer clock control flow; `set_anim` calls are partial |
//! | `Guy::set_anim` | `0x005DA300` | 4723 | head measured; local RNG sites isolated; full body unported |
//! | `AnimationPacket::get_anim_time` | `0x00918C40` | 88 | anim duration, default `200` |
//! | `AnimationPacket::get_game_frames` | `0x00918CC0` | 88 | used by `Wall`/`DeathObj` |
//! | `Guy::set_new_location` | `0x005D86F0` | 899 | crew-guy call site ported exactly |
//! | `GuyOut::graph_inc_frame` | `0x005DD200` | 3890 | presentation, `internal_random` |
//! | `Unit::execute_events` | `0x0060EDC0` | 131 | exact dispatcher in [`unit_execute_events`] |
//! | `Guy::execute_events` | `0x005D99C0` | 1093 | exact body in [`guy_execute_events`] over explicit lookup inputs |
//! | `GraphicEvents::get_event_group` | `0x008E23C0` | 397 | exact linked selector in [`select_event_group`] |
//! | `GraphicEvents::verify_load` | `0x008E4780` | 256 | exact resource dispatcher in [`graphic_events_verify_load`] |
//! | `GraphicEvents::init_unit_events` | `0x008E2520` | 5053 | installed-data result admitted by [`init_unit_events_from_extractor`] |
//! | `GraphicEvents::execute_game_events` | `0x008E48E0` | 1115 | RELEASE/PLANE bridge exact in [`execute_simulation_graphic_event`]; runtime sinks absent |
//! | anim-class table | `0x00AF4370` | 38×4 | transcribed in [`ANIM_CLASS`] |
//!
//! # Fidelity
//!
//! **Research-only Tier C.** Every function here is an instruction-level transcription
//! with its own test; nothing here has been executed against retail, and the oracle has no
//! case for it. The composite `Guy` / `Unit` / `Objects` drivers are test-only and carry
//! a `_research_partial` suffix, so this recovered module cannot be mistaken for the shipped
//! step-15 runtime merely because it is declared. The machine-readable boundary is
//! [`RUNTIME_FIDELITY_READY`] / [`RUNTIME_FIDELITY_BLOCKERS`]. The complete admission
//! ledger is `docs/mechanics/unit-inctime.md`. Two boundaries inside the recovered unit/guy
//! path are load-bearing and are counted rather than papered over:
//!
//! 1. **`Guy::set_anim`'s measured head is not integrated.** Bytes
//!    `0x005DA300..0x005DB3F0` resolve *which* animation actually plays (same-family early
//!    return, captain/uber recursion, hero/spell special-cases, packet fallbacks, and one of
//!    three mutually exclusive local RNG sites). The test-only partial driver applies only
//!    the common changed-animation writes: `cur_anim = <already resolved>`, `cur_time = 0`,
//!    `end_time = anim_frames(anim)`. The class-8/walk path at `0x005DB3FA` instead preserves
//!    phase; the binary queries `get_anim_time(old_cur_anim)` twice with the identical old
//!    animation, so its observed rescale ratio is one. Neither path is exposed as runtime.
//! 2. **The animation packets are art assets we do not load.** `end_time` is
//!    `anim_frames[anim_index]` read out of the loaded `.anm` data. Retail's own fallback
//!    when the animation is absent is `end_time = 3` (`mov eax, 3` at `0x005DB558`) and
//!    `get_anim_time` returns `200` (`0x00918C8D`); [`MissingAnimData`] is exactly those
//!    two constants, so the clock runs and the state machine is exercised, but the
//!    *durations* are retail's missing-asset fallback rather than retail's real ones.

use crate::objects::{Band, ObjectRegistry, BUILD_BAND_BASE, OWNER_SLOTS};
use crate::rng::Random;
use crate::systems::groups_guys::{GuyData, UnitGuys, UnitTypeStats};

/// Whether this module may serve step 15 on a fidelity or product surface.
pub const RUNTIME_FIDELITY_READY: bool = false;

/// Retail step-15 work still absent from the recovered driver.
///
/// The first three entries block even the unit/guy sub-path; the remainder block the full
/// `Objects::inc_time` step. The list stays explicit so green research tests cannot become
/// a completeness claim.
pub const RUNTIME_FIDELITY_BLOCKERS: &[&str] = &[
    "Guy::set_anim 0x005DA300 full state/RNG integration (including recursive activations)",
    "extracted shipped GraphicEvents/EventGroup and graphic-node resource pack wired at runtime",
    "complete Objects::add_ammo/Ammo::init RELEASE adapter (target abort, anti-air RNG, scatter, counter/checksum)",
    "complete Unit::come_out/launching/Guy storage RELEASE_PLANE adapter with transitive RNG accounting",
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
// Guy::set_anim — the three mutually exclusive game_random sites
// ---------------------------------------------------------------------------

/// A `game_random` site in `Guy::set_anim`, in control-flow order.
///
/// These are alternatives, not a sequence. `0x005DA709` separates requested class `0`
/// from the non-zero classes; `0x005DB20E` and `0x005DB2B8` then select class `12` or `8`.
/// Consequently one call can reach at most one variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetAnimRngSite {
    /// Default/idle fallback, call at `0x005DAC75` (site begins `0x005DAC68`).
    DefaultVariation,
    /// Attack variation, call at `0x005DB22A` (site begins `0x005DB21D`).
    AttackVariation,
    /// Nature locomotion variation, call at `0x005DB346` (site begins `0x005DB339`).
    NatureWalkVariation,
}

impl SetAnimRngSite {
    pub const fn call_va(self) -> u32 {
        match self {
            SetAnimRngSite::DefaultVariation => 0x005D_AC75,
            SetAnimRngSite::AttackVariation => 0x005D_B22A,
            SetAnimRngSite::NatureWalkVariation => 0x005D_B346,
        }
    }
}

/// The one optional random result produced by an entered `set_anim` class arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetAnimRngOutcome {
    pub site: Option<SetAnimRngSite>,
    /// `Random::get(0, 0xFFFF) % 100`, when a draw occurred.
    pub percent: Option<i32>,
    /// Immediate candidate selected by the attack or nature-walk roll, before packet
    /// fallbacks and unit-mask overrides. The default arm's remaining selection depends on
    /// more head state and therefore returns `None` here.
    pub candidate_anim: Option<i8>,
}

impl SetAnimRngOutcome {
    const fn no_draw(candidate_anim: Option<i8>) -> Self {
        SetAnimRngOutcome {
            site: None,
            percent: None,
            candidate_anim,
        }
    }
}

/// One already-entered, mutually exclusive animation-class arm.
///
/// This intentionally does not pretend to be all of `Guy::set_anim`: the 4.7 KiB
/// function's prelude can return, redirect, or recursively invoke the function before these
/// arms. It isolates only each site's final draw gate and its immediate roll mapping; packet
/// fallbacks, unit-mask overrides, and the surrounding state writes remain outside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetAnimRngArm {
    /// Control has reached `0x005DAC3E`. Retail draws only when the owning unit's
    /// `UnitData::openlist` at `+0x104` is null; otherwise it installs percentage zero
    /// without advancing the RNG.
    DefaultVariation { unit_openlist_is_null: bool },
    /// Control has reached the class-12 resolution block after its queue/hold early returns.
    /// The third `Guy::set_anim` argument is the variation / autoselect gate: zero preserves
    /// the actual requested attack anim and consumes no RNG; non-zero rolls among 11/12/13.
    AttackVariation {
        requested: i8,
        variant_autoselect: bool,
    },
    /// Control has reached the class-8 resolution block. Only owner 9 and types
    /// 0x192/0x193/0x194 draw here; a roll at least 50 selects jog (9), otherwise the
    /// class-canonical walk (8) remains.
    NatureWalkVariation { owner: i8, unit_type: i32 },
    /// Any class/path with no `game_random` site.
    None,
}

/// Execute the exact local RNG behavior of one entered [`SetAnimRngArm`].
///
/// Every draw is `Random::get(0, 0xFFFF)` followed by signed remainder `% 100`; the draw is
/// non-negative, so Rust's `%` agrees with x86 `idiv`. The enum makes the retail CFG's
/// mutual exclusion structural: this function cannot consume more than one draw.
pub fn set_anim_rng_arm(arm: SetAnimRngArm, rng: &mut Random) -> SetAnimRngOutcome {
    let mut draw = |site| {
        let percent = rng.get(0, 0xFFFF) % 100;
        SetAnimRngOutcome {
            site: Some(site),
            percent: Some(percent),
            candidate_anim: None,
        }
    };

    match arm {
        SetAnimRngArm::DefaultVariation {
            unit_openlist_is_null,
        } => {
            if unit_openlist_is_null {
                draw(SetAnimRngSite::DefaultVariation)
            } else {
                SetAnimRngOutcome {
                    site: None,
                    percent: Some(0),
                    candidate_anim: None,
                }
            }
        }
        SetAnimRngArm::AttackVariation {
            requested,
            variant_autoselect,
        } => {
            if !variant_autoselect {
                return SetAnimRngOutcome::no_draw(Some(requested));
            }
            let mut out = draw(SetAnimRngSite::AttackVariation);
            let percent = out.percent.expect("draw arm always sets percent");
            out.candidate_anim = Some(if percent < 30 {
                anim::CHAR_ATTACK1
            } else if percent > 70 {
                anim::CHAR_ATTACK3
            } else {
                anim::CHAR_ATTACK2
            });
            out
        }
        SetAnimRngArm::NatureWalkVariation { owner, unit_type } => {
            if owner != 9 {
                // Owners 0..8 take the speed-ratio branch before this site. That branch is
                // deterministic but outside this RNG helper, so do not invent its result.
                return SetAnimRngOutcome::no_draw(None);
            }
            if !matches!(unit_type, 0x192 | 0x193 | 0x194) {
                return SetAnimRngOutcome::no_draw(Some(anim::CHAR_WALK));
            }
            let mut out = draw(SetAnimRngSite::NatureWalkVariation);
            out.candidate_anim = Some(
                if out.percent.expect("draw arm always sets percent") >= 50 {
                    anim::CHAR_JOG
                } else {
                    anim::CHAR_WALK
                },
            );
            out
        }
        SetAnimRngArm::None => SetAnimRngOutcome::no_draw(None),
    }
}

// ---------------------------------------------------------------------------
// Guy::execute_events — exact package, order identity, and unit-side body
// ---------------------------------------------------------------------------

/// `GameDataPackage` from the retail PDB, exactly 56 bytes.
///
/// `Guy::execute_events` builds this on its stack and passes it to
/// `GraphicEvents::execute_game_events`. The final target UID is deliberately not part of
/// the package; retail keeps it in a separate local for stale-target validation.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GameDataPackage {
    pub gpiece: i32,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub cur_anim: i32,
    pub cur_time: u32,
    pub last_time: i32,
    pub angle: i32,
    pub pivot_angles: [f32; 4],
    pub node_flags: u16,
    pub o: i16,
    pub ox: i16,
    pub who: i8,
    pub whom: i8,
}

const _: [(); 56] = [(); std::mem::size_of::<GameDataPackage>()];

/// `fast_angle_to_degrees` `0x00A28F70` after its 256-entry lazy table has been built.
///
/// The function indexes only the high byte of the binary angle. Exhaustive evaluation of
/// the table-building instruction stream reduces to this integer expression for all 256
/// entries; every result is an exactly representable integer `f32` in `0..=359`.
#[inline]
pub fn fast_angle_to_degrees(angle: i32) -> f32 {
    let high = (angle as u32) >> 24;
    ((high.wrapping_mul(360).wrapping_add(128)) >> 8) as f32
}

/// `angle_to_degrees` `0x00A28E00`, including its integer approximation constants.
///
/// This deliberately follows the retail quotient/remainder decomposition rather than
/// replacing it with a mathematically cleaner 64-bit multiply. The latter disagrees at
/// boundary values because the shipped function rounds through several reciprocal-multiply
/// chunks.
#[inline]
pub fn angle_to_degrees(angle: i32) -> i32 {
    let angle = angle as u32;
    let quadrants = angle >> 30;
    let within_quadrant = angle.wrapping_add(quadrants.wrapping_mul(0xC000_0000));
    let fifteens = within_quadrant >> 29;
    let within_fifteen = within_quadrant.wrapping_add(fifteens.wrapping_mul(0xE000_0000));
    let within_five = within_fifteen % 0x1555_5555;
    let within_degree = within_five % 0x0AAA_AAAA;
    let result = (within_degree % 0x038E_38E3 + 0x005B_05B0) / 0x00B6_0B60
        + (within_degree / 0x038E_38E3) * 5
        + ((fifteens + quadrants * 2) * 3
            + within_five / 0x0AAA_AAAA
            + (within_fifteen / 0x1555_5555) * 2)
            * 15;
    result as i32
}

/// `degrees_to_angle` `0x00A28EB0`, preserving signed x86 quotient/remainder and wrapping
/// arithmetic. Retail event color at `GraphicEvent +0x10` is passed through this helper by
/// RELEASE_PLANE before being added to the source unit's binary angle.
#[inline]
pub fn degrees_to_angle(degrees: i32) -> i32 {
    let mod_90 = degrees % 90;
    let mod_45 = mod_90 % 45;
    let mod_30 = mod_45 % 30;
    let mod_15 = mod_30 % 15;
    ((mod_45 / 30)
        .wrapping_mul(0x071C_71C7)
        .wrapping_add(mod_15 / 5))
    .wrapping_mul(3)
    .wrapping_add(
        ((mod_90 / 45).wrapping_add((degrees / 90).wrapping_mul(2))).wrapping_mul(0x2000_0000),
    )
    .wrapping_add((mod_30 / 15).wrapping_mul(0x0AAA_AAAA))
    .wrapping_add(mod_15.wrapping_mul(0x00B6_0B60))
}

/// Target identity carried beside a [`GameDataPackage`] while Guy events are validated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuyEventTarget {
    pub o: i16,
    pub who: i8,
    pub uid: u16,
    /// Result of `UnitOrder::update_attack_ground_order()` on the non-group attack path.
    /// A non-null result bypasses stale-target validation at `0x005D9DB7`.
    pub bypass_attack_validation: bool,
}

impl GuyEventTarget {
    pub const fn new(o: i16, who: i8, uid: u16) -> Self {
        GuyEventTarget {
            o,
            who,
            uid,
            bypass_attack_validation: false,
        }
    }
}

/// The current-order shapes read by `Guy::execute_events`.
///
/// This is not a synthetic action model. Each variant is the exact result of the virtual
/// queries in `0x005D9AEE..0x005D9C14`, with the PDB fields retained. Values are narrowed
/// exactly as the x86 loads do (`movsx word` for object index, `movsx byte` for owner).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuyEventOrder {
    /// Null order or any order that is neither attack-ground nor special-animation.
    FallbackToCavarch,
    /// `is_attack() && !is_group()`: `TargetOrder::{ox,whom,uid}`.
    Attack {
        ox: i32,
        whom: i32,
        uid: u16,
        update_attack_ground_present: bool,
    },
    /// `is_attack() && is_group()`: `GroupAttackOrder::{oxxx,whosoever}` plus inherited UID.
    GroupAttack { oxxx: i32, whosoever: i32, uid: u16 },
    /// Order type 23 or 24. Retail installs `(-1,-1,-1)` and bypasses target validation.
    AttackGround,
    /// Order type 25. Retail narrows `SpecialAnimOrder::{data1,data2}` into target identity.
    SpecialAnim { data1: i32, data2: i32 },
}

/// Resolve the package target and separate UID local exactly as `Guy::execute_events` does.
pub fn resolve_guy_event_target(order: GuyEventOrder, cavarch: GuyEventTarget) -> GuyEventTarget {
    match order {
        GuyEventOrder::FallbackToCavarch => GuyEventTarget {
            bypass_attack_validation: false,
            ..cavarch
        },
        GuyEventOrder::Attack {
            ox,
            whom,
            uid,
            update_attack_ground_present,
        } => GuyEventTarget {
            o: ox as i16,
            who: whom as i8,
            uid,
            bypass_attack_validation: update_attack_ground_present,
        },
        GuyEventOrder::GroupAttack {
            oxxx,
            whosoever,
            uid,
        } => GuyEventTarget::new(oxxx as i16, whosoever as i8, uid),
        GuyEventOrder::AttackGround => GuyEventTarget {
            o: -1,
            who: -1,
            uid: u16::MAX,
            bypass_attack_validation: true,
        },
        GuyEventOrder::SpecialAnim { data1, data2 } => {
            GuyEventTarget::new(data1 as i16, data2 as i8, u16::MAX)
        }
    }
}

/// The owning-unit fields and virtual-query results used by one Guy event call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuyEventUnitView {
    /// `ObjectTypeData::domain +0x218`; only `SEA == 1` enters the carrier counter path.
    pub domain: i32,
    /// Result of the exact `UnitData::is_type(0x15F, 0)` query.
    pub is_type_0x15f: bool,
    /// `UnitData::launching.length`, or zero when the array pointer is null.
    pub launching_len: i32,
    /// Checksummed `UnitData::trench_angle +0x5C`, reused by this carrier animation path.
    pub trench_angle: i32,
    /// Fallback identity at `cavarch_o/+0xA2`, `cavarch_uid/+0xA6`, `cavarch_who/+0xA8`.
    pub cavarch: GuyEventTarget,
}

/// Build the PDB-layout package before the carrier override.
pub fn guy_event_package(guy: &GuyData, target: GuyEventTarget) -> GameDataPackage {
    let pivot_angles = if guy.turret_angles == [0; 4] {
        [0.0; 4]
    } else {
        guy.turret_angles.map(fast_angle_to_degrees)
    };
    GameDataPackage {
        gpiece: guy.gpiece,
        x: guy.x,
        y: guy.y,
        z: guy.z,
        cur_anim: guy.cur_anim as i32,
        cur_time: guy.cur_time,
        last_time: if guy.cur_time == 0 { -1 } else { guy.last_time },
        angle: guy.angle,
        pivot_angles,
        node_flags: guy.node_flags as u16,
        o: guy.o,
        ox: target.o,
        who: guy.who,
        whom: target.who,
    }
}

/// Apply `0x005D9C51..0x005D9D88`, the exact SEA/type-0x15F event-animation counter.
///
/// Returns whether `trench_angle` was written. The high two bits encode animation 11/12;
/// the low 30 bits are its event clock. Expiry is strict (`clock > game_frames`).
pub fn advance_sea_guy_event_animation<A: AnimData>(
    guy: &GuyData,
    unit: &mut GuyEventUnitView,
    package: &mut GameDataPackage,
    anims: &A,
) -> bool {
    if unit.domain != 1 || !unit.is_type_0x15f {
        return false;
    }

    let mut wrote = false;
    if unit.trench_angle as u32 & 0xC000_0000 == 0 && unit.launching_len != 0 {
        unit.trench_angle = if unit.launching_len > 1 {
            0xC000_0000u32 as i32
        } else {
            0x4000_0000
        };
        wrote = true;
    }
    if unit.trench_angle as u32 & 0xC000_0000 == 0 {
        return wrote;
    }

    unit.trench_angle = unit.trench_angle.wrapping_add(1);
    wrote = true;
    package.cur_anim = if unit.trench_angle < 0 { 12 } else { 11 };
    let event_clock = unit.trench_angle as u32 & 0x3FFF_FFFF;
    package.cur_time = event_clock;
    let frames = anims.game_frames(guy.gpiece, package.cur_anim as i8);
    if (event_clock as i32) > frames {
        package.cur_anim = guy.cur_anim as i32;
        package.cur_time = guy.cur_time;
        unit.trench_angle = 0;
    } else {
        package.last_time = guy
            .last_time
            .wrapping_sub(guy.cur_time as i32)
            .wrapping_add(event_clock as i32);
    }
    wrote
}

/// Exact normalized fields read from PDB `GraphicEvent` by the game-event executor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GraphicEventView {
    pub event_type: i32,
    pub animation: i32,
    pub start_time: u16,
    pub end_time: u16,
    /// Raw `GraphicEvent +0x10`; `color` for the ordinary event view and the first
    /// underlay union word for underlay events.
    pub color_or_underlay: u32,
    pub subject: i32,
    pub sound: i32,
    pub emit_subject: u16,
    pub time: u16,
    pub type_code: i16,
    pub axis: i8,
    pub node_num: i8,
}

/// Simulation-relevant dispatch arms in `GraphicEvents::execute_game_events`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphicEventAction {
    Release,
    ReleasePlane,
    Sound,
}

/// Number of animation-indexed `PtrArray<GraphicEvent>` members in retail `EventGroup`.
pub const GRAPHIC_EVENT_ANIMATION_SLOTS: usize = 38;

/// SHA-256 of the only executable for which the PDB layouts and instruction addresses in
/// this module are admitted.
pub const SUPPORTED_RETAIL_EXE_SHA256: [u8; 32] = [
    0x30, 0x47, 0x8a, 0x44, 0xb5, 0x77, 0xcb, 0x11, 0xeb, 0xcb, 0xbb, 0xf5, 0x3d, 0x3e, 0x93, 0xba,
    0x02, 0xfd, 0x2a, 0xac, 0xf3, 0xbd, 0xef, 0xa6, 0x55, 0x2c, 0x9b, 0x64, 0x49, 0x62, 0x50, 0x79,
];

/// SHA-256 of the supported retail `Data/unit_graphics.xml` (3,440,809 bytes).
pub const SUPPORTED_UNIT_GRAPHICS_SHA256: [u8; 32] = [
    0xf0, 0x1b, 0x09, 0x1f, 0x1d, 0xf8, 0xc7, 0x92, 0x07, 0x68, 0x3f, 0x54, 0xda, 0xa4, 0x17, 0xc2,
    0xfb, 0x98, 0x61, 0xfb, 0xbb, 0x65, 0x95, 0xe3, 0x3e, 0x4a, 0x0d, 0x74, 0xd1, 0x08, 0xe5, 0x4d,
];

/// Provenance supplied by the installed-data/live-memory event extractor.
///
/// `GraphicEvents::init_unit_events` resolves XML names through the loaded `RData`, graphic
/// piece, animation, sound, and object-type registries. Those resolved tables are retail
/// data and are not redistributed by this crate. A runtime pack must therefore be captured
/// from the user's supported installation and must retain both identities. The capture is
/// accepted only when its main-thread observer reported one coherent game frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphicEventResourceProvenance {
    pub executable_sha256: [u8; 32],
    pub installed_event_data_sha256: [u8; 32],
    pub coherent_capture: bool,
}

/// One `EventGroup` after retail has resolved all XML/RData names to integer IDs.
///
/// Vector order is the exact `PtrArray` order. `next` is represented by the enclosing
/// gpiece vector order, beginning with the unconditional root selected by
/// [`select_event_group`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractedEventGroup {
    pub selector: EventGroupSelector,
    pub events: [Vec<GraphicEventView>; GRAPHIC_EVENT_ANIMATION_SLOTS],
}

/// The resolved result of one retail `GraphicEvents::init_unit_events(gpiece)` call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractedGpieceEvents {
    pub gpiece: i32,
    pub groups_in_link_order: Vec<ExtractedEventGroup>,
}

/// Fail-loud resource errors shared by the extractor and installer boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GraphicEventResourceError {
    UnsupportedExecutable,
    UnsupportedInstalledEventData,
    IncoherentCapture,
    MissingInstalledDataIdentity,
    MissingGpiece(i32),
    WrongGpiece {
        requested: i32,
        extracted: i32,
    },
    MissingRootGroup(i32),
    InvalidAnimationBucket {
        gpiece: i32,
        group: usize,
        bucket: usize,
        event_animation: i32,
    },
    ExtractorFailure(String),
    InstallerFailure(String),
}

/// Extract resolved events from the supported user's retail installation/live process.
///
/// There is intentionally no built-in empty implementation. The live extractor walks
/// `graphic_events` at preferred VA `0x00C0B010` (RVA `0x0080B010`), whose PDB layout is
/// `GraphicEvents.events +0x04 -> EventGroup[38] -> GraphicEvent`. An offline extractor may
/// instead run the retail resolver over the installed graphics XML and registries, but it
/// must produce the same normalized IDs and provenance.
pub trait GraphicEventExtractor {
    fn provenance(&self) -> GraphicEventResourceProvenance;
    fn extract_gpiece(
        &mut self,
        gpiece: i32,
    ) -> Result<ExtractedGpieceEvents, GraphicEventResourceError>;
}

/// Destination for a validated, resolved gpiece event chain.
pub trait GraphicEventInstaller {
    fn install_gpiece_events(
        &mut self,
        events: ExtractedGpieceEvents,
    ) -> Result<(), GraphicEventResourceError>;
}

/// Counts produced by a successful extracted `init_unit_events` installation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InitUnitEventsStats {
    pub groups: usize,
    pub events: usize,
}

/// Extractor-backed runtime equivalent of `GraphicEvents::init_unit_events` `0x008E2520`.
///
/// The retail body is a 5 KiB parser/resolver over copyrighted installed data. This
/// function admits its *resolved result*, validates every simulation-relevant structural
/// invariant, and installs it without inventing IDs or presentation defaults. It fails on
/// absent data, an unsupported executable, incoherent live memory, an absent root, or an
/// event stored under a different animation bucket.
pub fn init_unit_events_from_extractor<E: GraphicEventExtractor, I: GraphicEventInstaller>(
    gpiece: i32,
    extractor: &mut E,
    installer: &mut I,
) -> Result<InitUnitEventsStats, GraphicEventResourceError> {
    let provenance = extractor.provenance();
    if provenance.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256 {
        return Err(GraphicEventResourceError::UnsupportedExecutable);
    }
    if !provenance.coherent_capture {
        return Err(GraphicEventResourceError::IncoherentCapture);
    }
    if provenance.installed_event_data_sha256 == [0; 32] {
        return Err(GraphicEventResourceError::MissingInstalledDataIdentity);
    }
    if provenance.installed_event_data_sha256 != SUPPORTED_UNIT_GRAPHICS_SHA256 {
        return Err(GraphicEventResourceError::UnsupportedInstalledEventData);
    }

    let extracted = extractor.extract_gpiece(gpiece)?;
    if extracted.gpiece != gpiece {
        return Err(GraphicEventResourceError::WrongGpiece {
            requested: gpiece,
            extracted: extracted.gpiece,
        });
    }
    if extracted.groups_in_link_order.is_empty() {
        return Err(GraphicEventResourceError::MissingRootGroup(gpiece));
    }

    let mut stats = InitUnitEventsStats {
        groups: extracted.groups_in_link_order.len(),
        events: 0,
    };
    for (group_index, group) in extracted.groups_in_link_order.iter().enumerate() {
        for (bucket, events) in group.events.iter().enumerate() {
            stats.events += events.len();
            for event in events {
                if event.animation != bucket as i32 {
                    return Err(GraphicEventResourceError::InvalidAnimationBucket {
                        gpiece,
                        group: group_index,
                        bucket,
                        event_animation: event.animation,
                    });
                }
            }
        }
    }
    installer.install_gpiece_events(extracted)?;
    Ok(stats)
}

/// The common interval predicate used by RELEASE(1), RELEASE_PLANE(5), and EV_SOUND(7).
///
/// The first comparison is signed and the second unsigned in the x86 stream:
/// `last_time < start_time && start_time <= cur_time`.
pub fn graphic_event_action(
    package: &GameDataPackage,
    event: GraphicEventView,
) -> Option<GraphicEventAction> {
    if package.cur_anim != event.animation
        || package.last_time >= event.start_time as i32
        || event.start_time as u32 > package.cur_time
    {
        return None;
    }
    match event.event_type {
        1 => Some(GraphicEventAction::Release),
        5 => Some(GraphicEventAction::ReleasePlane),
        7 => Some(GraphicEventAction::Sound),
        _ => None,
    }
}

/// One object-table identity. The package narrows `o` to i16, while a launching entry is
/// read as a full i32, so the normalized bridge retains the wider object index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphicEventObject {
    pub who: i8,
    pub o: i32,
}

/// Result of `GraphicPieces::get_position` `0x0090B750` before x86 float-to-int
/// truncation.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GraphicNodeOffset {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// All measured arguments that choose the RELEASE/RELEASE_PLANE node transform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphicNodeQuery {
    pub source: GraphicEventObject,
    pub node_num: i8,
    pub animation: i32,
    pub start_time: u16,
    pub angle_degrees: f32,
    pub pivot_angles: [f32; 4],
    pub restriction_count: i32,
}

/// Exact receipt required from `Objects::add_ammo -> Ammo::init`.
///
/// `ammo_index` advances even when `Ammo::init` aborts and leaves the claimed slot free.
/// `rng_draws` includes the anti-air gate first and aim scatter second (zero through four
/// total for the measured ordinary path), not merely the two currently common scatter
/// draws. The projectile's shooter/target ownership is the package passed to the sink.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmmoInitReceipt {
    pub slot: usize,
    pub graph_index: i32,
    pub occupied_after_init: bool,
    pub rng_draws: u32,
}

/// Exact receipt required from the complete `Unit::come_out(0)` body.
///
/// The wrapper has two mutually exclusive direct random sites (zero or one draw), but
/// recursively reached `set_anim` and helper bodies can draw too. The adapter reports the
/// complete transitive count rather than presenting the direct count as a bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComeOutReceipt {
    pub rng_draws: u32,
}

/// World/object operations behind the exact simulation arms of
/// `GraphicEvents::execute_game_events` `0x008E48E0`.
///
/// No method has a default implementation. A headless runtime must supply the extracted
/// node tables, the first-free ammo allocator plus complete `Ammo::init`, the launching
/// array, and complete `Unit::come_out`; it cannot silently consume an event.
pub trait GraphicEventSimulationSink {
    type Error;

    /// For RELEASE's targetless exception, return the source unit's current order type;
    /// `None` means the source object is absent or `vt+0x08` says it is not a valid unit.
    fn source_unit_order_type(
        &mut self,
        source: GraphicEventObject,
    ) -> Result<Option<i32>, Self::Error>;

    /// Normal RELEASE uses `GraphicPieces::has_restrictions(gpiece)`.
    fn release_restriction_count(&mut self, gpiece: i32) -> Result<i32, Self::Error>;

    /// RELEASE_PLANE uses the equivalent `gpiece_type-50` unit-data table field at +0x14.
    fn release_plane_restriction_count(&mut self, gpiece: i32) -> Result<i32, Self::Error>;

    /// Source virtual `get_gpiece` followed by extracted `GraphicPieces::get_position`.
    fn graphic_node_offset(
        &mut self,
        query: GraphicNodeQuery,
    ) -> Result<GraphicNodeOffset, Self::Error>;

    /// Must perform the exact first-free `Objects::add_ammo`, complete `Ammo::init`, and
    /// unconditional `Objects::ammo_index++` sequence.
    fn objects_add_ammo_and_init(
        &mut self,
        package: &GameDataPackage,
    ) -> Result<AmmoInitReceipt, Self::Error>;

    fn first_launching_o(&mut self, source: GraphicEventObject)
        -> Result<Option<i32>, Self::Error>;
    fn unit_is_active(&mut self, unit: GraphicEventObject) -> Result<bool, Self::Error>;
    fn unit_come_out(
        &mut self,
        unit: GraphicEventObject,
        arg: i32,
    ) -> Result<ComeOutReceipt, Self::Error>;
    /// Retail removes by value, not blindly by index, and also removes an invalid first
    /// entry.
    fn remove_launching_value(
        &mut self,
        source: GraphicEventObject,
        launched_o: i32,
    ) -> Result<(), Self::Error>;
    fn unit_set_new_location(
        &mut self,
        unit: GraphicEventObject,
        x: i32,
        y: i32,
        arg3: i32,
        arg4: i32,
    ) -> Result<(), Self::Error>;
    /// Must perform both checksummed writes in order: `last_z = z; z = new_z`.
    fn unit_guy0_set_z(&mut self, unit: GraphicEventObject, z: i32) -> Result<(), Self::Error>;
    fn source_angle(&mut self, source: GraphicEventObject) -> Result<i32, Self::Error>;
    /// Retail also pushes the owner object-table pointer, but `Unit::set_angle` does not
    /// read that argument in this binary. `arg` is the live trailing zero passed onward to
    /// `Guy::set_angle`, which updates checksummed `des_angle` and can propagate desired
    /// angle/location and tracked offsets through the allocated crew suffix. This method
    /// means the complete Unit/Guy body, not a single unit-angle write.
    fn unit_set_angle(
        &mut self,
        unit: GraphicEventObject,
        angle: i32,
        arg: i32,
    ) -> Result<(), Self::Error>;
    /// Must perform both checksummed writes in order: `last_pitch = pitch; pitch = 0`.
    fn unit_guy0_zero_pitch(&mut self, unit: GraphicEventObject) -> Result<(), Self::Error>;
}

/// Simulation result of one GraphicEvent. SOUND is named but remains presentation-only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphicEventSimulationEffect {
    NotDueOrNotSimulation,
    Sound,
    ReleaseTargetRejected,
    ReleaseNodeRejected,
    Projectile(AmmoInitReceipt),
    ReleasePlaneNodeRejected,
    ReleasePlaneQueueEmpty,
    InvalidReleasedPlaneRemoved(GraphicEventObject),
    ReleasedPlane {
        unit: GraphicEventObject,
        come_out: ComeOutReceipt,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum GraphicEventSimulationError<E> {
    Sink(E),
}

#[inline]
fn graphic_release_node_allowed(
    restriction_count: i32,
    node_num: i8,
    node_flag_base: u32,
    node_flags: u16,
) -> bool {
    let node = (node_num as i32) & 3;
    restriction_count == 0
        || node >= restriction_count
        || (node_flags as u32 & (1u32 << (node as u32 + node_flag_base))) != 0
}

/// x86 `cvttss2si`: invalid/non-finite/out-of-range values produce `INT_MIN`, while Rust's
/// float cast saturates. Shipped nodes are finite, but preserving the exceptional result
/// keeps malformed extracted data from silently taking a different coordinate.
#[inline]
fn cvttss2si(value: f32) -> i32 {
    if !value.is_finite() || !(-2_147_483_648.0..2_147_483_648.0).contains(&value) {
        i32::MIN
    } else {
        value.trunc() as i32
    }
}

/// Exact RELEASE and RELEASE_PLANE bridge from `GraphicEvents::execute_game_events`.
///
/// RELEASE temporarily replaces the package projectile gpiece/angle and position, invokes
/// the ammo allocator/initializer, then restores every byte even when the sink reports an
/// error. RELEASE_PLANE intentionally leaves its node-offset position accumulated in the
/// package, because retail's event loop exposes that mutation to later events.
pub fn execute_simulation_graphic_event<S: GraphicEventSimulationSink>(
    package: &mut GameDataPackage,
    event: GraphicEventView,
    sink: &mut S,
) -> Result<GraphicEventSimulationEffect, GraphicEventSimulationError<S::Error>> {
    let Some(action) = graphic_event_action(package, event) else {
        return Ok(GraphicEventSimulationEffect::NotDueOrNotSimulation);
    };
    if action == GraphicEventAction::Sound {
        return Ok(GraphicEventSimulationEffect::Sound);
    }

    let source = GraphicEventObject {
        who: package.who,
        o: package.o as i32,
    };

    if action == GraphicEventAction::Release {
        if (package.whom == -1 || package.ox == -1)
            && !matches!(
                sink.source_unit_order_type(source)
                    .map_err(GraphicEventSimulationError::Sink)?,
                Some(23 | 24)
            )
        {
            return Ok(GraphicEventSimulationEffect::ReleaseTargetRejected);
        }

        let restrictions = sink
            .release_restriction_count(package.gpiece)
            .map_err(GraphicEventSimulationError::Sink)?;
        if !graphic_release_node_allowed(restrictions, event.node_num, 0, package.node_flags) {
            return Ok(GraphicEventSimulationEffect::ReleaseNodeRejected);
        }

        let old_gpiece = package.gpiece;
        let old_angle = package.angle;
        package.gpiece = event.subject;
        let offset = match sink.graphic_node_offset(GraphicNodeQuery {
            source,
            node_num: event.node_num,
            animation: event.animation,
            start_time: event.start_time,
            angle_degrees: fast_angle_to_degrees(old_angle.wrapping_sub(i32::MIN)),
            pivot_angles: package.pivot_angles,
            restriction_count: restrictions,
        }) {
            Ok(offset) => offset,
            Err(err) => {
                package.gpiece = old_gpiece;
                return Err(GraphicEventSimulationError::Sink(err));
            }
        };
        let dx = cvttss2si(offset.x);
        let dy = cvttss2si(offset.y);
        let dz = cvttss2si(offset.z);
        package.x = package.x.wrapping_add(dx);
        package.y = package.y.wrapping_add(dy);
        package.z = package.z.wrapping_add(dz);
        package.angle = event.node_num as i32;
        let result = sink.objects_add_ammo_and_init(package);
        package.gpiece = old_gpiece;
        package.angle = old_angle;
        package.x = package.x.wrapping_sub(dx);
        package.y = package.y.wrapping_sub(dy);
        package.z = package.z.wrapping_sub(dz);
        return result
            .map(GraphicEventSimulationEffect::Projectile)
            .map_err(GraphicEventSimulationError::Sink);
    }

    let restrictions = sink
        .release_plane_restriction_count(package.gpiece)
        .map_err(GraphicEventSimulationError::Sink)?;
    if !graphic_release_node_allowed(restrictions, event.node_num, 4, package.node_flags) {
        return Ok(GraphicEventSimulationEffect::ReleasePlaneNodeRejected);
    }
    let Some(plane_o) = sink
        .first_launching_o(source)
        .map_err(GraphicEventSimulationError::Sink)?
    else {
        return Ok(GraphicEventSimulationEffect::ReleasePlaneQueueEmpty);
    };
    let plane = GraphicEventObject {
        who: package.who,
        o: plane_o,
    };
    if plane_o < 0
        || !sink
            .unit_is_active(plane)
            .map_err(GraphicEventSimulationError::Sink)?
    {
        sink.remove_launching_value(source, plane_o)
            .map_err(GraphicEventSimulationError::Sink)?;
        return Ok(GraphicEventSimulationEffect::InvalidReleasedPlaneRemoved(
            plane,
        ));
    }

    let come_out = sink
        .unit_come_out(plane, 0)
        .map_err(GraphicEventSimulationError::Sink)?;
    sink.remove_launching_value(source, plane_o)
        .map_err(GraphicEventSimulationError::Sink)?;
    let offset = sink
        .graphic_node_offset(GraphicNodeQuery {
            source,
            node_num: event.node_num,
            animation: event.animation,
            start_time: event.start_time,
            angle_degrees: angle_to_degrees(package.angle.wrapping_sub(i32::MIN)) as f32,
            pivot_angles: package.pivot_angles,
            restriction_count: restrictions,
        })
        .map_err(GraphicEventSimulationError::Sink)?;
    package.x = package.x.wrapping_add(cvttss2si(offset.x));
    package.y = package.y.wrapping_add(cvttss2si(offset.y));
    package.z = package.z.wrapping_add(cvttss2si(offset.z));
    sink.unit_set_new_location(plane, package.x, package.y, 1, 1)
        .map_err(GraphicEventSimulationError::Sink)?;
    sink.unit_guy0_set_z(plane, package.z)
        .map_err(GraphicEventSimulationError::Sink)?;
    let source_angle = sink
        .source_angle(source)
        .map_err(GraphicEventSimulationError::Sink)?;
    sink.unit_set_angle(
        plane,
        source_angle.wrapping_add(degrees_to_angle(event.color_or_underlay as i32)),
        0,
    )
    .map_err(GraphicEventSimulationError::Sink)?;
    sink.unit_guy0_zero_pitch(plane)
        .map_err(GraphicEventSimulationError::Sink)?;
    Ok(GraphicEventSimulationEffect::ReleasedPlane {
        unit: plane,
        come_out,
    })
}

/// Metadata on one linked `EventGroup` (PDB offsets `civ +0x428`, `age +0x429`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventGroupSelector {
    pub civ: i8,
    pub age: i8,
}

/// `GraphicEvents::get_event_group`'s linked-list selection, after the gpiece root exists.
///
/// Group zero is the unconditional fallback. Retail walks every `next`, retaining the last
/// group whose civilization is wildcard/exact and whose `age` is strictly below the
/// current age. A missing root is represented as `None`, not an invented empty table.
pub fn select_event_group(
    groups_in_link_order: &[EventGroupSelector],
    civ: i32,
    current_age: i32,
) -> Option<usize> {
    if groups_in_link_order.is_empty() {
        return None;
    }
    let mut selected = 0;
    for (i, group) in groups_in_link_order.iter().enumerate().skip(1) {
        if (group.civ == -1 || group.civ as i32 == civ) && (group.age as i32) < current_age {
            selected = i;
        }
    }
    Some(selected)
}

/// Inputs read by `GraphicEvents::verify_load` `0x008E4780`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphicLoadView {
    /// `GraphicEvents::events.length != 0`; false queues the gpiece for later loading.
    pub graphics_initialized: bool,
    /// `GraphicEventsData::loaded[gpiece]`.
    pub already_loaded: bool,
    /// `GraphicPieces::get_gpiece_type(gpiece)`.
    pub gpiece_type: i32,
    /// `GraphicPieces::get_gpiece_unit_type(gpiece)`, read only for gpiece type 1.
    pub unit_type: i32,
    /// Optional `ObjectTypeData*` at the graphic-piece type table. `None` is retail's null
    /// pointer case; otherwise bit 1 of byte `+0x92` requests animations 7..11.
    pub object_type_flags_0x92: Option<u8>,
}

/// Resource mutations performed by the exact `verify_load` dispatcher.
pub trait GraphicLoadSink {
    fn queue_preload_gpiece(&mut self, gpiece: i32);
    fn init_unit_events(&mut self, gpiece: i32);
    fn add_animation(&mut self, gpiece: i32, animation: i32, arg2: i32, arg3: i32, arg4: i32);
    fn mark_loaded(&mut self, gpiece: i32);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphicLoadResult {
    QueuedUntilInitialized,
    AlreadyLoaded,
    Loaded,
}

/// `GraphicEvents::verify_load` `0x008E4780`, all resource-control branches.
///
/// The called `init_unit_events` body still requires shipped graphics-event tables, and is
/// therefore a sink operation rather than a fabricated empty event group. The five
/// `add_animation` calls are exact: animations 7 through 11, arguments `(0, 0, 3)`.
pub fn graphic_events_verify_load<S: GraphicLoadSink>(
    gpiece: i32,
    view: GraphicLoadView,
    sink: &mut S,
) -> GraphicLoadResult {
    if !view.graphics_initialized {
        sink.queue_preload_gpiece(gpiece);
        return GraphicLoadResult::QueuedUntilInitialized;
    }
    if view.already_loaded {
        sink.mark_loaded(gpiece);
        return GraphicLoadResult::AlreadyLoaded;
    }

    if view.gpiece_type == 0
        || (view.gpiece_type == 1 && matches!(view.unit_type, 0x20D | 0x20B | 0x20C))
    {
        sink.init_unit_events(gpiece);
    }
    let add_default_animations = view.gpiece_type == 1
        || view
            .object_type_flags_0x92
            .is_some_and(|flags| flags & 2 != 0);
    if add_default_animations {
        for animation in 7..=11 {
            sink.add_animation(gpiece, animation, 0, 0, 3);
        }
    }
    sink.mark_loaded(gpiece);
    GraphicLoadResult::Loaded
}

/// External object/graphics operations at the boundary of the exact Guy-side body.
pub trait GuyEventSink {
    /// Return the UID only for an existing object whose `flags & 1` is set.
    fn valid_target_uid(&self, who: i8, o: i16) -> Option<u16>;
    fn verify_graphic_load(&mut self, gpiece: i32);
    fn execute_graphic_events(&mut self, package: &GameDataPackage);
}

/// Observable endpoint of one exact `Guy::execute_events` pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuyEventResult {
    /// `GraphicPieces::get_gpiece_type(gpiece) != 0`.
    NonUnitGraphic,
    /// Attack-class stale/missing target rejected after `verify_load`.
    AttackTargetRejected,
    /// Package was passed to `GraphicEvents::execute_game_events`.
    Executed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuyEventError {
    InvalidAnimation(i32),
}

/// `Guy::execute_events` `0x005D99C0`, with virtual/resource lookups supplied explicitly.
///
/// `gpiece_type` is the result of `GraphicPieces::get_gpiece_type`. The function contains
/// all package construction, current-order target selection, carrier state mutation,
/// verify-load ordering, and stale-target control flow. The sink's graphics-event executor
/// remains a separately measured boundary because its RELEASE arms require shipped node
/// tables plus integrated ammo/unit allocation; it is never replaced with a no-op here.
pub fn guy_execute_events<A: AnimData, S: GuyEventSink>(
    guy: &GuyData,
    order: GuyEventOrder,
    unit: &mut GuyEventUnitView,
    gpiece_type: i32,
    anims: &A,
    sink: &mut S,
) -> Result<GuyEventResult, GuyEventError> {
    let target = resolve_guy_event_target(order, unit.cavarch);
    let mut package = guy_event_package(guy, target);
    advance_sea_guy_event_animation(guy, unit, &mut package, anims);

    if gpiece_type != 0 {
        return Ok(GuyEventResult::NonUnitGraphic);
    }
    sink.verify_graphic_load(package.gpiece);

    let animation = usize::try_from(package.cur_anim)
        .ok()
        .filter(|&a| a < ANIM_CLASS.len())
        .ok_or(GuyEventError::InvalidAnimation(package.cur_anim))?;
    if ANIM_CLASS[animation] == 12 && !target.bypass_attack_validation {
        if target.o < 0
            || target.who < 0
            || sink.valid_target_uid(target.who, target.o) != Some(target.uid)
        {
            return Ok(GuyEventResult::AttackTargetRejected);
        }
    }
    sink.execute_graphic_events(&package);
    Ok(GuyEventResult::Executed)
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

    /// `AnimationPacket::get_game_frames` `0x00918CC0`. This reads the same frame-count
    /// table as [`AnimData::anim_frames`] and returns `3` when the animation is absent.
    /// It is signed in retail even though valid shipped frame counts are non-negative.
    fn game_frames(&self, gpiece: i32, a: i8) -> i32 {
        self.anim_frames(gpiece, a) as i32
    }

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
// Unit::execute_events — exact 131-byte dispatcher
// ---------------------------------------------------------------------------

/// Inputs read by `Unit::execute_events` `0x0060EDC0` before it walks the live guy prefix.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UnitEventView {
    /// Result of virtual slot `+0x08`, `UnitData::is_valid_unit` `0x0046CDA0`
    /// (`flags & 1`).
    pub unit_is_valid: bool,
    /// Result of virtual slot `+0xBC`, `UnitData::is_on_map` `0x0046CE30`
    /// (`(u16)inside_up >> 15`).
    pub unit_is_on_map: bool,
    /// `UnitData::unit_masks2` `+0x6C`; bit `0x10` selects verify-load instead of execution.
    pub unit_masks2: u32,
}

/// Which side of `Unit::execute_events`'s only branch retail takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitEventPath {
    /// Call `Guy::execute_events` `0x005D99C0` for every guy in `[0, guy_mark)`.
    ExecuteGuyEvents,
    /// Call `GraphicEvents::verify_load(guy.gpiece)` `0x008E4780` over the same prefix.
    VerifyGraphicLoads,
}

impl UnitEventView {
    /// The condition at `0x0060EDC6..0x0060EDDF`, transcribed from the instruction stream.
    pub const fn path(self) -> UnitEventPath {
        if !self.unit_is_valid
            || !self.unit_is_on_map
            || self.unit_masks2 & UNIT_MASKS2_FREEZE_ANIM != 0
        {
            UnitEventPath::VerifyGraphicLoads
        } else {
            UnitEventPath::ExecuteGuyEvents
        }
    }
}

/// The two external calls made by the exact `Unit::execute_events` dispatcher.
///
/// The callback receives the whole array and index for the execute path so it may mutate
/// `guy_mark`; retail re-reads that byte at the loop backedge (`0x0060EDFF`).
pub trait UnitEventSink {
    fn execute_guy_events(&mut self, guys: &mut UnitGuys, index: usize);
    fn verify_graphic_load(&mut self, gpiece: i32);
}

/// Structural state inconsistency that would be a null dereference in retail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitEventError {
    NullGuySlot(usize),
}

/// Counts produced by one exact `Unit::execute_events` dispatcher pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitEventStats {
    pub path: UnitEventPath,
    pub guys: u32,
}

/// `Unit::execute_events` `0x0060EDC0`, all 131 bytes of control flow.
///
/// This closes the unit-level dispatcher only. [`guy_execute_events`] supplies the exact
/// Guy-side execute callback over explicit external inputs, and
/// [`graphic_events_verify_load`] supplies the verify-load resource dispatcher. The called
/// `init_unit_events` resource body and the RELEASE sinks remain blockers and are not
/// approximated here.
pub fn unit_execute_events<S: UnitEventSink>(
    view: UnitEventView,
    guys: &mut UnitGuys,
    sink: &mut S,
) -> Result<UnitEventStats, UnitEventError> {
    let path = view.path();
    let mut stats = UnitEventStats { path, guys: 0 };
    let mut i = 0i32;
    while i < guys.guy_mark as i32 {
        let idx = i as usize;
        let gpiece = match guys.guys.get(idx) {
            Some(Some(guy)) => guy.gpiece,
            _ => return Err(UnitEventError::NullGuySlot(idx)),
        };
        match path {
            UnitEventPath::ExecuteGuyEvents => sink.execute_guy_events(guys, idx),
            UnitEventPath::VerifyGraphicLoads => sink.verify_graphic_load(gpiece),
        }
        stats.guys += 1;
        i += 1;
    }
    Ok(stats)
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
    /// Root `Guy::set_anim` `0x005DA300` activations skipped by the partial driver. Each
    /// activation has **0–1 local `game_random` draws** at one of the mutually exclusive
    /// sites `0x005DAC68` / `0x005DB21D` / `0x005DB339`, but the captain/uber path may
    /// recursively create more activations. Full animation resolution is also skipped.
    pub set_anim_head: u64,
    /// `GuyOut::graph_inc_frame` `0x005DD200` calls skipped (owners 0..7). Presentation;
    /// draws six times from `internal_random`, so skipping it does **not** move the sim
    /// stream.
    pub graph_inc_frame: u64,
    /// Calls to the now-ported [`unit_execute_events`] dispatcher that the test-only
    /// `Objects::inc_time` research driver still does not supply sink inputs for.
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
    #[cfg(test)]
    #[allow(dead_code)]
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
    /// Total `game_random` draws this pass failed to make.
    ///
    /// `None` means no finite upper bound has been established: one skipped root activation
    /// can recursively invoke `set_anim` through captain/uber coordination. Returning the
    /// direct-site count as a total bound would silently under-report that path.
    pub fn missing_sim_rng_draws(&self) -> (u64, Option<u64>) {
        (
            0,
            if self.gaps.set_anim_head == 0 {
                Some(0)
            } else {
                None
            },
        )
    }

    /// Range contributed by the already-counted activations themselves, excluding any
    /// recursive activations that the unported body would have created.
    pub fn missing_direct_set_anim_rng_draws(&self) -> (u64, u64) {
        (0, self.gaps.set_anim_head)
    }

    #[cfg(test)]
    #[allow(dead_code)]
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

/// The common changed-animation write arm of `Guy::set_anim`, ending at `0x005DB570`
/// [measured]. This is a test-only component of the partial `Guy::inc_time` driver.
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
/// **The path not taken.** At `0x005DB3FA` retail tests the measured old animation class
/// against `8` (walk). That arm either subtracts the old end time or preserves phase across
/// an animation change. The latter calls `AnimationPacket::get_anim_time(old_cur_anim)`
/// twice with the identical old animation (`0x005DB43C`, `0x005DB44D`), so the observed
/// integer rescale ratio is one. The partial driver lacks the head state needed to select
/// this path honestly, so every root call remains counted in [`IncTimeGaps::set_anim_head`].
///
/// `variant_autoselect` is `set_anim`'s third argument. It gates the attack-variation RNG
/// draw at `0x005DB217` and nothing in the ported tail. The second argument is the separate
/// same-class restart/override gate; all `Guy::inc_time` call sites pass it as zero.
#[cfg(test)]
pub(crate) fn set_anim_tail<A: AnimData>(
    g: &mut GuyData,
    a: i8,
    variant_autoselect: i32,
    data: &A,
    st: &mut IncTimeStats,
) {
    let _ = variant_autoselect;
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

/// Research transcription of `Guy::inc_time` `0x005D9E10` [measured, 1,251 bytes].
///
/// The outer control flow is instruction-level, but calls to `Guy::set_anim` execute only
/// [`set_anim_tail`]. That missing head changes animation choice and simulation RNG position,
/// so this function is intentionally crate-private and named `_research_partial`.
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
#[cfg(test)]
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
#[cfg(test)]
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
#[cfg(test)]
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
#[cfg(test)]
#[allow(dead_code)]
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
        assert!(RUNTIME_FIDELITY_BLOCKERS
            .iter()
            .any(|s| s.contains("set_anim")));
        assert!(RUNTIME_FIDELITY_BLOCKERS
            .iter()
            .any(|s| s.contains("Farms::inc_time")));
    }

    fn seed_for_set_anim_percent(want: i32) -> i32 {
        for seed in 0..1_000_000 {
            let mut rng = Random::new(seed);
            if rng.get(0, 0xFFFF) % 100 == want {
                return seed;
            }
        }
        panic!("no seed found for set_anim percentage {want}");
    }

    #[test]
    fn set_anim_rng_sites_are_mutually_exclusive_single_draw_arms() {
        let arms = [
            (
                SetAnimRngArm::DefaultVariation {
                    unit_openlist_is_null: true,
                },
                SetAnimRngSite::DefaultVariation,
            ),
            (
                SetAnimRngArm::AttackVariation {
                    requested: anim::CHAR_ATTACK2,
                    variant_autoselect: true,
                },
                SetAnimRngSite::AttackVariation,
            ),
            (
                SetAnimRngArm::NatureWalkVariation {
                    owner: 9,
                    unit_type: 0x192,
                },
                SetAnimRngSite::NatureWalkVariation,
            ),
        ];
        for (arm, site) in arms {
            let mut got = Random::new(0x1234_5678);
            let mut expected = got;
            let percent = expected.get(0, 0xFFFF) % 100;
            let out = set_anim_rng_arm(arm, &mut got);
            assert_eq!(out.site, Some(site));
            assert_eq!(out.percent, Some(percent));
            assert_eq!(got.state(), expected.state(), "exactly one LCG step");
        }
        assert_eq!(SetAnimRngSite::DefaultVariation.call_va(), 0x005D_AC75);
        assert_eq!(SetAnimRngSite::AttackVariation.call_va(), 0x005D_B22A);
        assert_eq!(SetAnimRngSite::NatureWalkVariation.call_va(), 0x005D_B346);
    }

    #[test]
    fn set_anim_no_draw_gates_do_not_advance_game_random() {
        let cases = [
            SetAnimRngArm::DefaultVariation {
                unit_openlist_is_null: false,
            },
            SetAnimRngArm::AttackVariation {
                requested: anim::CHAR_ATTACKSPECIAL,
                variant_autoselect: false,
            },
            SetAnimRngArm::NatureWalkVariation {
                owner: 8,
                unit_type: 0x192,
            },
            SetAnimRngArm::NatureWalkVariation {
                owner: 9,
                unit_type: 0x191,
            },
            SetAnimRngArm::None,
        ];
        for arm in cases {
            let mut rng = Random::new(77);
            let before = rng.state();
            let out = set_anim_rng_arm(arm, &mut rng);
            assert_eq!(out.site, None);
            assert_eq!(rng.state(), before);
        }
        let mut rng = Random::new(77);
        assert_eq!(
            set_anim_rng_arm(
                SetAnimRngArm::DefaultVariation {
                    unit_openlist_is_null: false,
                },
                &mut rng,
            )
            .percent,
            Some(0),
            "retail writes a synthetic zero at 0x005DAC89"
        );
    }

    #[test]
    fn attack_variation_thresholds_are_30_and_70_inclusive_in_the_middle() {
        for (percent, want) in [
            (29, anim::CHAR_ATTACK1),
            (30, anim::CHAR_ATTACK2),
            (70, anim::CHAR_ATTACK2),
            (71, anim::CHAR_ATTACK3),
        ] {
            let mut rng = Random::new(seed_for_set_anim_percent(percent));
            let out = set_anim_rng_arm(
                SetAnimRngArm::AttackVariation {
                    requested: anim::CHAR_ATTACKSPECIAL,
                    variant_autoselect: true,
                },
                &mut rng,
            );
            assert_eq!(out.percent, Some(percent));
            assert_eq!(out.candidate_anim, Some(want));
        }

        let mut rng = Random::new(5);
        let before = rng.state();
        let out = set_anim_rng_arm(
            SetAnimRngArm::AttackVariation {
                requested: anim::CHAR_ATTACKSPECIAL,
                variant_autoselect: false,
            },
            &mut rng,
        );
        assert_eq!(out.candidate_anim, Some(anim::CHAR_ATTACKSPECIAL));
        assert_eq!(rng.state(), before);
    }

    #[test]
    fn nature_walk_variation_jogs_at_fifty() {
        for (percent, want) in [(49, anim::CHAR_WALK), (50, anim::CHAR_JOG)] {
            let mut rng = Random::new(seed_for_set_anim_percent(percent));
            let out = set_anim_rng_arm(
                SetAnimRngArm::NatureWalkVariation {
                    owner: 9,
                    unit_type: 0x194,
                },
                &mut rng,
            );
            assert_eq!(out.percent, Some(percent));
            assert_eq!(out.candidate_anim, Some(want));
        }
    }

    #[test]
    fn game_data_package_is_the_exact_pdb_layout_and_snapshot() {
        assert_eq!(std::mem::size_of::<GameDataPackage>(), 56);
        let mut guy = GuyData {
            gpiece: 17,
            x: 101,
            y: -202,
            z: 303,
            cur_anim: anim::CHAR_ATTACK2,
            cur_time: 44,
            last_time: 39,
            angle: 0x4000_0000,
            turret_angles: [0, 0x4000_0000, 0x8000_0000u32 as i32, -1],
            node_flags: 0x4321,
            o: 7,
            who: 3,
            ..GuyData::default()
        };
        let target = GuyEventTarget::new(9, 4, 0x1234);
        let package = guy_event_package(&guy, target);
        assert_eq!(package.gpiece, 17);
        assert_eq!((package.x, package.y, package.z), (101, -202, 303));
        assert_eq!(package.cur_anim, anim::CHAR_ATTACK2 as i32);
        assert_eq!((package.cur_time, package.last_time), (44, 39));
        assert_eq!(package.pivot_angles, [0.0, 90.0, 180.0, 359.0]);
        assert_eq!((package.o, package.ox), (7, 9));
        assert_eq!((package.who, package.whom), (3, 4));

        guy.cur_time = 0;
        guy.turret_angles = [0; 4];
        let package = guy_event_package(&guy, target);
        assert_eq!(package.last_time, -1);
        assert_eq!(package.pivot_angles, [0.0; 4]);
        assert_eq!(fast_angle_to_degrees(0x00ff_ffff), 0.0);
        assert_eq!(fast_angle_to_degrees(0x0100_0000), 1.0);
    }

    #[test]
    fn guy_event_order_resolution_preserves_retails_narrowing_and_bypass() {
        let fallback = GuyEventTarget::new(6, 2, 99);
        assert_eq!(
            resolve_guy_event_target(GuyEventOrder::FallbackToCavarch, fallback),
            fallback
        );
        assert_eq!(
            resolve_guy_event_target(
                GuyEventOrder::Attack {
                    ox: 0x1_0002,
                    whom: 0x103,
                    uid: 7,
                    update_attack_ground_present: true,
                },
                fallback,
            ),
            GuyEventTarget {
                o: 2,
                who: 3,
                uid: 7,
                bypass_attack_validation: true,
            }
        );
        assert_eq!(
            resolve_guy_event_target(GuyEventOrder::AttackGround, fallback),
            GuyEventTarget {
                o: -1,
                who: -1,
                uid: u16::MAX,
                bypass_attack_validation: true,
            }
        );
        assert_eq!(
            resolve_guy_event_target(
                GuyEventOrder::SpecialAnim {
                    data1: 0x1_0004,
                    data2: 0x105,
                },
                fallback,
            ),
            GuyEventTarget::new(4, 5, u16::MAX)
        );
    }

    #[test]
    fn sea_event_counter_selects_11_or_12_and_expires_strictly_after_frames() {
        let guy = GuyData {
            gpiece: 8,
            cur_anim: anim::CHAR_IDLE1,
            cur_time: 5,
            last_time: 10,
            ..GuyData::default()
        };
        let mut unit = GuyEventUnitView {
            domain: 1,
            is_type_0x15f: true,
            launching_len: 1,
            trench_angle: 0,
            cavarch: GuyEventTarget::new(-1, -1, 0),
        };
        for clock in 1..=3 {
            let mut package = guy_event_package(&guy, unit.cavarch);
            assert!(advance_sea_guy_event_animation(
                &guy,
                &mut unit,
                &mut package,
                &MissingAnimData,
            ));
            assert_eq!(package.cur_anim, 11);
            assert_eq!(package.cur_time, clock);
            assert_eq!(package.last_time, 10 - 5 + clock as i32);
            assert_ne!(unit.trench_angle, 0, "clock equal to frames remains live");
        }
        let mut package = guy_event_package(&guy, unit.cavarch);
        advance_sea_guy_event_animation(&guy, &mut unit, &mut package, &MissingAnimData);
        assert_eq!(unit.trench_angle, 0);
        assert_eq!(package.cur_anim, anim::CHAR_IDLE1 as i32);
        assert_eq!(package.cur_time, 5);

        unit.launching_len = 2;
        let mut package = guy_event_package(&guy, unit.cavarch);
        advance_sea_guy_event_animation(&guy, &mut unit, &mut package, &MissingAnimData);
        assert_eq!(
            package.cur_anim, 12,
            "negative high-bit state selects anim 12"
        );
    }

    #[test]
    fn graphic_event_interval_and_group_selection_match_retail_boundaries() {
        let package = GameDataPackage {
            cur_anim: 12,
            last_time: 29,
            cur_time: 30,
            ..GameDataPackage::default()
        };
        let event = GraphicEventView {
            event_type: 1,
            animation: 12,
            start_time: 30,
            ..GraphicEventView::default()
        };
        assert_eq!(
            graphic_event_action(&package, event),
            Some(GraphicEventAction::Release)
        );
        assert_eq!(
            graphic_event_action(
                &GameDataPackage {
                    last_time: 30,
                    ..package
                },
                event,
            ),
            None,
            "last_time is strictly below the event"
        );
        assert_eq!(
            graphic_event_action(
                &GameDataPackage {
                    cur_time: 29,
                    ..package
                },
                event,
            ),
            None,
            "current time includes the event boundary"
        );

        let groups = [
            EventGroupSelector { civ: 4, age: 99 },
            EventGroupSelector { civ: -1, age: 1 },
            EventGroupSelector { civ: 3, age: 2 },
            EventGroupSelector { civ: 3, age: 3 },
        ];
        assert_eq!(select_event_group(&[], 3, 4), None);
        assert_eq!(select_event_group(&groups, 3, 3), Some(2));
        assert_eq!(select_event_group(&groups, 2, 3), Some(1));
    }

    #[derive(Clone)]
    struct ExtractorProbe {
        provenance: GraphicEventResourceProvenance,
        extracted: Option<ExtractedGpieceEvents>,
    }

    impl GraphicEventExtractor for ExtractorProbe {
        fn provenance(&self) -> GraphicEventResourceProvenance {
            self.provenance
        }

        fn extract_gpiece(
            &mut self,
            gpiece: i32,
        ) -> Result<ExtractedGpieceEvents, GraphicEventResourceError> {
            self.extracted
                .take()
                .ok_or(GraphicEventResourceError::MissingGpiece(gpiece))
        }
    }

    #[derive(Default)]
    struct InstallerProbe {
        installed: Vec<ExtractedGpieceEvents>,
    }

    impl GraphicEventInstaller for InstallerProbe {
        fn install_gpiece_events(
            &mut self,
            events: ExtractedGpieceEvents,
        ) -> Result<(), GraphicEventResourceError> {
            self.installed.push(events);
            Ok(())
        }
    }

    fn supported_event_provenance() -> GraphicEventResourceProvenance {
        GraphicEventResourceProvenance {
            executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
            installed_event_data_sha256: SUPPORTED_UNIT_GRAPHICS_SHA256,
            coherent_capture: true,
        }
    }

    fn extracted_release(gpiece: i32, bucket: usize) -> ExtractedGpieceEvents {
        let mut events: [Vec<GraphicEventView>; GRAPHIC_EVENT_ANIMATION_SLOTS] =
            std::array::from_fn(|_| Vec::new());
        events[bucket].push(GraphicEventView {
            event_type: 1,
            animation: bucket as i32,
            start_time: 10,
            subject: 77,
            ..GraphicEventView::default()
        });
        ExtractedGpieceEvents {
            gpiece,
            groups_in_link_order: vec![ExtractedEventGroup {
                selector: EventGroupSelector { civ: -1, age: -1 },
                events,
            }],
        }
    }

    #[test]
    fn init_unit_events_requires_pinned_coherent_resolved_retail_data() {
        let mut extractor = ExtractorProbe {
            provenance: supported_event_provenance(),
            extracted: Some(extracted_release(44, 12)),
        };
        let mut installer = InstallerProbe::default();
        assert_eq!(
            init_unit_events_from_extractor(44, &mut extractor, &mut installer),
            Ok(InitUnitEventsStats {
                groups: 1,
                events: 1,
            })
        );
        assert_eq!(installer.installed.len(), 1);

        for (provenance, error) in [
            (
                GraphicEventResourceProvenance {
                    executable_sha256: [1; 32],
                    ..supported_event_provenance()
                },
                GraphicEventResourceError::UnsupportedExecutable,
            ),
            (
                GraphicEventResourceProvenance {
                    coherent_capture: false,
                    ..supported_event_provenance()
                },
                GraphicEventResourceError::IncoherentCapture,
            ),
            (
                GraphicEventResourceProvenance {
                    installed_event_data_sha256: [2; 32],
                    ..supported_event_provenance()
                },
                GraphicEventResourceError::UnsupportedInstalledEventData,
            ),
        ] {
            let mut extractor = ExtractorProbe {
                provenance,
                extracted: Some(extracted_release(44, 12)),
            };
            let mut installer = InstallerProbe::default();
            assert_eq!(
                init_unit_events_from_extractor(44, &mut extractor, &mut installer),
                Err(error)
            );
            assert!(installer.installed.is_empty());
        }
    }

    #[test]
    fn init_unit_events_rejects_empty_roots_and_cross_bucket_events() {
        let mut no_root = ExtractorProbe {
            provenance: supported_event_provenance(),
            extracted: Some(ExtractedGpieceEvents {
                gpiece: 9,
                groups_in_link_order: Vec::new(),
            }),
        };
        assert_eq!(
            init_unit_events_from_extractor(9, &mut no_root, &mut InstallerProbe::default()),
            Err(GraphicEventResourceError::MissingRootGroup(9))
        );

        let mut bad = extracted_release(9, 12);
        bad.groups_in_link_order[0].events[12][0].animation = 11;
        let mut extractor = ExtractorProbe {
            provenance: supported_event_provenance(),
            extracted: Some(bad),
        };
        assert_eq!(
            init_unit_events_from_extractor(9, &mut extractor, &mut InstallerProbe::default()),
            Err(GraphicEventResourceError::InvalidAnimationBucket {
                gpiece: 9,
                group: 0,
                bucket: 12,
                event_animation: 11,
            })
        );
    }

    #[derive(Default)]
    struct SimulationEventProbe {
        log: Vec<&'static str>,
        order_type: Option<i32>,
        release_restrictions: i32,
        plane_restrictions: i32,
        node_offset: GraphicNodeOffset,
        node_queries: Vec<GraphicNodeQuery>,
        ammo_packages: Vec<GameDataPackage>,
        launching_o: Option<i32>,
        active_plane: bool,
        source_angle: i32,
        locations: Vec<(GraphicEventObject, i32, i32)>,
        z_writes: Vec<(GraphicEventObject, i32)>,
        angle_writes: Vec<(GraphicEventObject, i32)>,
    }

    impl GraphicEventSimulationSink for SimulationEventProbe {
        type Error = &'static str;

        fn source_unit_order_type(
            &mut self,
            _source: GraphicEventObject,
        ) -> Result<Option<i32>, Self::Error> {
            self.log.push("order");
            Ok(self.order_type)
        }

        fn release_restriction_count(&mut self, _gpiece: i32) -> Result<i32, Self::Error> {
            self.log.push("release-restrict");
            Ok(self.release_restrictions)
        }

        fn release_plane_restriction_count(&mut self, _gpiece: i32) -> Result<i32, Self::Error> {
            self.log.push("plane-restrict");
            Ok(self.plane_restrictions)
        }

        fn graphic_node_offset(
            &mut self,
            query: GraphicNodeQuery,
        ) -> Result<GraphicNodeOffset, Self::Error> {
            self.log.push("node");
            self.node_queries.push(query);
            Ok(self.node_offset)
        }

        fn objects_add_ammo_and_init(
            &mut self,
            package: &GameDataPackage,
        ) -> Result<AmmoInitReceipt, Self::Error> {
            self.log.push("ammo");
            self.ammo_packages.push(*package);
            Ok(AmmoInitReceipt {
                slot: 4,
                graph_index: 99,
                occupied_after_init: true,
                rng_draws: 3,
            })
        }

        fn first_launching_o(
            &mut self,
            _source: GraphicEventObject,
        ) -> Result<Option<i32>, Self::Error> {
            self.log.push("front");
            Ok(self.launching_o)
        }

        fn unit_is_active(&mut self, _unit: GraphicEventObject) -> Result<bool, Self::Error> {
            self.log.push("active");
            Ok(self.active_plane)
        }

        fn unit_come_out(
            &mut self,
            _unit: GraphicEventObject,
            arg: i32,
        ) -> Result<ComeOutReceipt, Self::Error> {
            assert_eq!(arg, 0);
            self.log.push("come-out");
            Ok(ComeOutReceipt { rng_draws: 2 })
        }

        fn remove_launching_value(
            &mut self,
            _source: GraphicEventObject,
            _launched_o: i32,
        ) -> Result<(), Self::Error> {
            self.log.push("remove");
            Ok(())
        }

        fn unit_set_new_location(
            &mut self,
            unit: GraphicEventObject,
            x: i32,
            y: i32,
            arg3: i32,
            arg4: i32,
        ) -> Result<(), Self::Error> {
            assert_eq!((arg3, arg4), (1, 1));
            self.log.push("location");
            self.locations.push((unit, x, y));
            Ok(())
        }

        fn unit_guy0_set_z(&mut self, unit: GraphicEventObject, z: i32) -> Result<(), Self::Error> {
            self.log.push("z");
            self.z_writes.push((unit, z));
            Ok(())
        }

        fn source_angle(&mut self, _source: GraphicEventObject) -> Result<i32, Self::Error> {
            self.log.push("source-angle");
            Ok(self.source_angle)
        }

        fn unit_set_angle(
            &mut self,
            unit: GraphicEventObject,
            angle: i32,
            arg: i32,
        ) -> Result<(), Self::Error> {
            assert_eq!(arg, 0);
            self.log.push("angle");
            self.angle_writes.push((unit, angle));
            Ok(())
        }

        fn unit_guy0_zero_pitch(&mut self, _unit: GraphicEventObject) -> Result<(), Self::Error> {
            self.log.push("pitch");
            Ok(())
        }
    }

    #[test]
    fn release_temporarily_builds_the_ammo_package_then_restores_it() {
        let mut package = GameDataPackage {
            gpiece: 10,
            x: 100,
            y: 200,
            z: 300,
            cur_anim: 12,
            cur_time: 10,
            last_time: 9,
            angle: 0x1000_0000,
            node_flags: 1 << 2,
            o: 7,
            ox: 8,
            who: 3,
            whom: 4,
            ..GameDataPackage::default()
        };
        let original = package;
        let event = GraphicEventView {
            event_type: 1,
            animation: 12,
            start_time: 10,
            subject: 99,
            node_num: 2,
            ..GraphicEventView::default()
        };
        let mut sink = SimulationEventProbe {
            release_restrictions: 3,
            node_offset: GraphicNodeOffset {
                x: 1.9,
                y: -2.9,
                z: 3.9,
            },
            ..SimulationEventProbe::default()
        };
        assert_eq!(
            execute_simulation_graphic_event(&mut package, event, &mut sink),
            Ok(GraphicEventSimulationEffect::Projectile(AmmoInitReceipt {
                slot: 4,
                graph_index: 99,
                occupied_after_init: true,
                rng_draws: 3,
            }))
        );
        assert_eq!(package, original, "RELEASE restores its stack package");
        assert_eq!(sink.log, ["release-restrict", "node", "ammo"]);
        assert_eq!(sink.ammo_packages.len(), 1);
        let ammo = sink.ammo_packages[0];
        assert_eq!((ammo.gpiece, ammo.angle), (99, 2));
        assert_eq!((ammo.x, ammo.y, ammo.z), (101, 198, 303));
        assert_eq!((ammo.who, ammo.o, ammo.whom, ammo.ox), (3, 7, 4, 8));
        assert_eq!(
            sink.node_queries[0].angle_degrees,
            fast_angle_to_degrees(0x9000_0000u32 as i32)
        );
    }

    #[test]
    fn release_uses_exact_minus_one_target_and_masked_raw_node_gates() {
        let event = GraphicEventView {
            event_type: 1,
            animation: 12,
            start_time: 10,
            subject: 99,
            node_num: -1,
            ..GraphicEventView::default()
        };
        let mut targetless = GameDataPackage {
            cur_anim: 12,
            cur_time: 10,
            last_time: 9,
            node_flags: 1 << 3,
            whom: -1,
            ox: -1,
            ..GameDataPackage::default()
        };
        let mut allowed = SimulationEventProbe {
            order_type: Some(23),
            release_restrictions: 4,
            ..SimulationEventProbe::default()
        };
        assert!(matches!(
            execute_simulation_graphic_event(&mut targetless, event, &mut allowed),
            Ok(GraphicEventSimulationEffect::Projectile(_))
        ));
        assert_eq!(allowed.log, ["order", "release-restrict", "node", "ammo"]);
        assert_eq!(allowed.node_queries[0].node_num, -1, "query keeps raw i8");

        let mut minus_two_target = GameDataPackage {
            whom: -2,
            ox: -2,
            ..targetless
        };
        let mut bypass = SimulationEventProbe::default();
        assert!(matches!(
            execute_simulation_graphic_event(&mut minus_two_target, event, &mut bypass),
            Ok(GraphicEventSimulationEffect::Projectile(_))
        ));
        assert!(!bypass.log.contains(&"order"), "only exact -1 is absent");

        assert!(!graphic_release_node_allowed(4, -1, 0, 0));
        assert!(graphic_release_node_allowed(4, -1, 0, 1 << 3));
        assert!(graphic_release_node_allowed(4, -1, 4, 1 << 7));
    }

    #[test]
    fn release_plane_orders_come_out_and_checksummed_writes_and_keeps_offset() {
        let mut package = GameDataPackage {
            gpiece: 10,
            x: 100,
            y: 200,
            z: 300,
            cur_anim: 11,
            cur_time: 20,
            last_time: 19,
            angle: 0,
            node_flags: 1 << 5,
            o: 7,
            who: 3,
            ..GameDataPackage::default()
        };
        let event = GraphicEventView {
            event_type: 5,
            animation: 11,
            start_time: 20,
            color_or_underlay: 90,
            node_num: 1,
            ..GraphicEventView::default()
        };
        let plane = GraphicEventObject { who: 3, o: 33 };
        let mut sink = SimulationEventProbe {
            plane_restrictions: 2,
            node_offset: GraphicNodeOffset {
                x: 5.9,
                y: -6.9,
                z: 7.9,
            },
            launching_o: Some(33),
            active_plane: true,
            source_angle: 0x1000_0000,
            ..SimulationEventProbe::default()
        };
        assert_eq!(
            execute_simulation_graphic_event(&mut package, event, &mut sink),
            Ok(GraphicEventSimulationEffect::ReleasedPlane {
                unit: plane,
                come_out: ComeOutReceipt { rng_draws: 2 },
            })
        );
        assert_eq!((package.x, package.y, package.z), (105, 194, 307));
        assert_eq!(
            sink.log,
            [
                "plane-restrict",
                "front",
                "active",
                "come-out",
                "remove",
                "node",
                "location",
                "z",
                "source-angle",
                "angle",
                "pitch",
            ]
        );
        assert_eq!(sink.locations, vec![(plane, 105, 194)]);
        assert_eq!(sink.z_writes, vec![(plane, 307)]);
        assert_eq!(
            sink.angle_writes,
            vec![(plane, 0x5000_0000)],
            "source angle + degrees_to_angle(90)"
        );
        assert_eq!(sink.node_queries[0].angle_degrees, 180.0);
    }

    #[test]
    fn release_plane_removes_an_invalid_front_by_value_without_coming_out() {
        let mut package = GameDataPackage {
            cur_anim: 11,
            cur_time: 20,
            last_time: 19,
            who: 2,
            ..GameDataPackage::default()
        };
        let event = GraphicEventView {
            event_type: 5,
            animation: 11,
            start_time: 20,
            ..GraphicEventView::default()
        };
        let mut sink = SimulationEventProbe {
            launching_o: Some(-4),
            ..SimulationEventProbe::default()
        };
        assert_eq!(
            execute_simulation_graphic_event(&mut package, event, &mut sink),
            Ok(GraphicEventSimulationEffect::InvalidReleasedPlaneRemoved(
                GraphicEventObject { who: 2, o: -4 }
            ))
        );
        assert_eq!(sink.log, ["plane-restrict", "front", "remove"]);
    }

    #[test]
    fn retail_angle_helpers_keep_the_shipped_integer_approximations() {
        assert_eq!(angle_to_degrees(0), 0);
        assert_eq!(angle_to_degrees(0x4000_0000), 90);
        assert_eq!(angle_to_degrees(i32::MIN), 180);
        assert_eq!(angle_to_degrees(-1), 360);
        assert_eq!(degrees_to_angle(0), 0);
        assert_eq!(degrees_to_angle(90), 0x4000_0000);
        assert_eq!(degrees_to_angle(180), i32::MIN);
        assert_eq!(degrees_to_angle(360), 0);
        assert_eq!(degrees_to_angle(359) as u32, 0xff49_f49b);
    }

    #[derive(Default)]
    struct GraphicLoadProbe {
        queued: Vec<i32>,
        initialized: Vec<i32>,
        animations: Vec<(i32, i32, i32, i32, i32)>,
        marked: Vec<i32>,
    }

    impl GraphicLoadSink for GraphicLoadProbe {
        fn queue_preload_gpiece(&mut self, gpiece: i32) {
            self.queued.push(gpiece);
        }

        fn init_unit_events(&mut self, gpiece: i32) {
            self.initialized.push(gpiece);
        }

        fn add_animation(&mut self, gpiece: i32, animation: i32, arg2: i32, arg3: i32, arg4: i32) {
            self.animations.push((gpiece, animation, arg2, arg3, arg4));
        }

        fn mark_loaded(&mut self, gpiece: i32) {
            self.marked.push(gpiece);
        }
    }

    #[test]
    fn graphic_verify_load_queues_or_initializes_without_an_empty_table_fallback() {
        let mut probe = GraphicLoadProbe::default();
        assert_eq!(
            graphic_events_verify_load(
                22,
                GraphicLoadView {
                    graphics_initialized: false,
                    already_loaded: false,
                    gpiece_type: 0,
                    unit_type: 0,
                    object_type_flags_0x92: None,
                },
                &mut probe,
            ),
            GraphicLoadResult::QueuedUntilInitialized
        );
        assert_eq!(probe.queued, vec![22]);
        assert!(probe.initialized.is_empty());
        assert!(probe.marked.is_empty());

        let mut probe = GraphicLoadProbe::default();
        assert_eq!(
            graphic_events_verify_load(
                23,
                GraphicLoadView {
                    graphics_initialized: true,
                    already_loaded: false,
                    gpiece_type: 1,
                    unit_type: 0x20D,
                    object_type_flags_0x92: None,
                },
                &mut probe,
            ),
            GraphicLoadResult::Loaded
        );
        assert_eq!(probe.initialized, vec![23]);
        assert_eq!(
            probe.animations,
            (7..=11)
                .map(|animation| (23, animation, 0, 0, 3))
                .collect::<Vec<_>>()
        );
        assert_eq!(probe.marked, vec![23]);

        let mut probe = GraphicLoadProbe::default();
        graphic_events_verify_load(
            24,
            GraphicLoadView {
                graphics_initialized: true,
                already_loaded: false,
                gpiece_type: 2,
                unit_type: 0,
                object_type_flags_0x92: Some(2),
            },
            &mut probe,
        );
        assert!(probe.initialized.is_empty());
        assert_eq!(probe.animations.len(), 5);
        assert_eq!(probe.marked, vec![24]);
    }

    #[derive(Default)]
    struct GuyEventProbe {
        valid: Option<(i8, i16, u16)>,
        verified: Vec<i32>,
        executed: Vec<GameDataPackage>,
    }

    impl GuyEventSink for GuyEventProbe {
        fn valid_target_uid(&self, who: i8, o: i16) -> Option<u16> {
            self.valid
                .filter(|&(w, object, _)| w == who && object == o)
                .map(|(_, _, uid)| uid)
        }

        fn verify_graphic_load(&mut self, gpiece: i32) {
            self.verified.push(gpiece);
        }

        fn execute_graphic_events(&mut self, package: &GameDataPackage) {
            self.executed.push(*package);
        }
    }

    #[test]
    fn guy_execute_events_verifies_before_stale_target_gate_and_never_fakes_a_sink() {
        let guy = GuyData {
            gpiece: 44,
            cur_anim: anim::CHAR_ATTACK2,
            cur_time: 12,
            last_time: 8,
            ..GuyData::default()
        };
        let mut unit = GuyEventUnitView {
            domain: 0,
            is_type_0x15f: false,
            launching_len: 0,
            trench_angle: 0,
            cavarch: GuyEventTarget::new(-1, -1, 0),
        };
        let order = GuyEventOrder::Attack {
            ox: 7,
            whom: 2,
            uid: 0x1234,
            update_attack_ground_present: false,
        };
        let mut sink = GuyEventProbe::default();
        assert_eq!(
            guy_execute_events(&guy, order, &mut unit, 0, &MissingAnimData, &mut sink),
            Ok(GuyEventResult::AttackTargetRejected)
        );
        assert_eq!(sink.verified, vec![44]);
        assert!(sink.executed.is_empty());

        sink.valid = Some((2, 7, 0x1234));
        assert_eq!(
            guy_execute_events(&guy, order, &mut unit, 0, &MissingAnimData, &mut sink),
            Ok(GuyEventResult::Executed)
        );
        assert_eq!(sink.executed.len(), 1);
        assert_eq!((sink.executed[0].ox, sink.executed[0].whom), (7, 2));

        let mut non_unit = GuyEventProbe::default();
        assert_eq!(
            guy_execute_events(
                &guy,
                GuyEventOrder::AttackGround,
                &mut unit,
                1,
                &MissingAnimData,
                &mut non_unit,
            ),
            Ok(GuyEventResult::NonUnitGraphic)
        );
        assert!(non_unit.verified.is_empty());
        assert!(non_unit.executed.is_empty());
    }

    #[derive(Default)]
    struct EventProbe {
        executed: Vec<usize>,
        verified: Vec<i32>,
        shrink_mark_after_first: bool,
    }

    impl UnitEventSink for EventProbe {
        fn execute_guy_events(&mut self, guys: &mut UnitGuys, index: usize) {
            self.executed.push(index);
            if self.shrink_mark_after_first {
                guys.guy_mark = 1;
            }
        }

        fn verify_graphic_load(&mut self, gpiece: i32) {
            self.verified.push(gpiece);
        }
    }

    #[test]
    fn unit_execute_events_takes_the_exact_gate_and_live_prefix() {
        let mut guys = squad(2, 1);
        guys.guys[0].as_mut().unwrap().gpiece = 40;
        guys.guys[1].as_mut().unwrap().gpiece = 41;
        guys.guys[2].as_mut().unwrap().gpiece = 99;

        let mut execute = EventProbe::default();
        let st = unit_execute_events(
            UnitEventView {
                unit_is_valid: true,
                unit_is_on_map: true,
                unit_masks2: 0,
            },
            &mut guys,
            &mut execute,
        )
        .unwrap();
        assert_eq!(st.path, UnitEventPath::ExecuteGuyEvents);
        assert_eq!(st.guys, 2);
        assert_eq!(execute.executed, vec![0, 1]);
        assert!(execute.verified.is_empty());

        for view in [
            UnitEventView {
                unit_is_valid: false,
                unit_is_on_map: true,
                unit_masks2: 0,
            },
            UnitEventView {
                unit_is_valid: true,
                unit_is_on_map: false,
                unit_masks2: 0,
            },
            UnitEventView {
                unit_is_valid: true,
                unit_is_on_map: true,
                unit_masks2: UNIT_MASKS2_FREEZE_ANIM,
            },
        ] {
            let mut verify = EventProbe::default();
            let st = unit_execute_events(view, &mut guys, &mut verify).unwrap();
            assert_eq!(st.path, UnitEventPath::VerifyGraphicLoads);
            assert_eq!(verify.verified, vec![40, 41]);
            assert!(verify.executed.is_empty());
        }
    }

    #[test]
    fn unit_execute_events_reloads_guy_mark_and_fails_loudly_on_null() {
        let mut guys = squad(3, 0);
        let mut probe = EventProbe {
            shrink_mark_after_first: true,
            ..Default::default()
        };
        let st = unit_execute_events(
            UnitEventView {
                unit_is_valid: true,
                unit_is_on_map: true,
                unit_masks2: 0,
            },
            &mut guys,
            &mut probe,
        )
        .unwrap();
        assert_eq!(st.guys, 1);
        assert_eq!(probe.executed, vec![0]);

        let mut guys = squad(2, 0);
        guys.guys[1] = None;
        let mut probe = EventProbe::default();
        assert_eq!(
            unit_execute_events(
                UnitEventView {
                    unit_is_valid: true,
                    unit_is_on_map: true,
                    unit_masks2: 0,
                },
                &mut guys,
                &mut probe,
            ),
            Err(UnitEventError::NullGuySlot(1))
        );
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
            (0, Some(0)),
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
        unit_inc_time_research_partial(
            &view(1, 0),
            &mut g,
            &MissingAnimData,
            &FlatTerrain,
            &mut st,
        );
        assert_eq!(st.gaps.set_anim_head, 1);
        assert_eq!(st.missing_direct_set_anim_rng_draws(), (0, 1));
        assert_eq!(st.missing_sim_rng_draws(), (0, None));
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
