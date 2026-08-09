//! `Group` and `Guy` — the two dynamic checksum channels.
//!
//! Serves **`CheckSums::check_groups` `0x00937530`** (channel 6) and
//! **`CheckSums::check_guys` `0x00937430`** (channel 7) of the fifteen in
//! `CheckSums::check_all` `0x00936560`.
//!
//! # The two things this file is about
//!
//! A **`Guy`** is one soldier. A `Unit` in Rise of Nations is a *squad*, and the bodies in
//! it are a `PtrArray<Guy>` at `UnitData +0xE4` — `guys`, its own 481-object class with its
//! own `process`/`move` called at the tail of `Unit::process`. (`docs/derivation/`
//! `architecture.md` §7.2 calls `Unit`'s third optional walk section "garrison contents";
//! the PDB field is `UnitData::guys : PtrArray<Guy>` at `+0xE4` and the walker is reached
//! from `check_guys` for every unit, so it is the squad, not a garrison. Two sentences,
//! moving on.)
//!
//! A **`Group`** is a selection: the set of units a player has selected or bound to a
//! control group, plus the formation state that selection carries. Every player command
//! addresses a `Group`, and `CommandPackage::process_X` calls `Group::action_X`, which
//! fans the command out into per-unit `UnitOrder`s. It is the layer the RL action space
//! sits on.
//!
//! # Provenance
//!
//! Everything below is `[measured]` on this Mac against `ron-bin/riseofnations.exe`
//! (sha256 `30478a44…625079`) and `ron-bin/sbl/rise.pdb`, by disassembly with capstone
//! unless the line says otherwise. Structure taken from `re/decomp-all/*.c` is marked
//! `[structure]` — Ghidra output is a hypothesis about shape, never a source of values.
//!
//! | symbol | VA | size | role |
//! |---|---|---:|---|
//! | `CheckSums::check_guys` | `0x00937430` | 176 | channel 7 driver |
//! | `CheckSums::check_groups` | `0x00937530` | 109 | channel 6 driver |
//! | `PtrArray<Guy>::walk_data` | `0x0046DF30` | 810 | per-unit guy array walk |
//! | `GuyData::walk_data` | `0x005E0210` | 29 | one guy: a flat byte range |
//! | `Group::walk_data` | `0x00708400` | 181 | one group, length-prefixed |
//! | `Groups::walk_data` | `0x00713E30` | 72 | save-side walk (superset) |
//! | `Guy::process` | `0x005E0230` | 649 | per-guy tick |
//! | `Guy::move` | `0x005D9240` | 1233 | the guy movement integrator |
//! | `Guy::turn_towards` | `0x005D9720` | 116 | angle slew, returns remainder |
//! | `Guy::turn_angles` | `0x005D98C0` | 134 | same, non-mutating, optional half step |
//! | `GuyData::turn_speed` | `0x005DE340` | 202 | per-frame angular step |
//! | `GuyData::get_speed` | `0x005DE410` | 220 | per-frame linear step source |
//! | `Unit::set_type` | `0x00612FA0` | 2157 | **guy allocation** |
//! | `Unit::set_new_location` | `0x005F8D20` | 1757 | initial squad lattice + Guy teleport |
//! | `Objects::kill_guy` | `0x00659410` | 978 | guy teardown |
//! | `Groups::process` | `0x006FA210` | 582 | one group per player per frame |
//! | `Group::normalize` | `0x00711540` | 473 | prune dead members + compact |
//! | `Group::get_num` | `0x00714700` | 320 | prune-and-count |
//! | `Group::add` | `0x00714350` | 616 | append a member |
//! | `Group::update_positions` | `0x00713810` | 576 | rotate formation offsets |
//! | `Group::action_form` | `0x00707220` | 746 | FORM command |
//! | `Group::compute_form` | `0x00707C80` | 766 | pick + fill a `Form` |
//! | `Form::categorize` / `Form::compute` | `0x0072E250` / `0x0072E8E0` | 1678 / 137 | formation layout |
//! | `vector_dist` | `0x0046CFF0` | 105 | integer `hypot` |
//! | `sinx` / `cosx` | `0x0092D100` / `0x0092D0C0` | 46 / 50 | the folds `Guy::move` uses |
//!
//! # Fidelity
//!
//! **Tier C except for a Tier-B initial-materialization state check.** Every function here
//! is an instruction-level transcription with its own unit test, but there is no injected
//! oracle call for a `Guy` or `Group` entry point yet. Coherent main-thread retail samples
//! do independently confirm raw Guy coordinates, zero initial `off_x/off_y`, and Guy 0 at
//! the unit anchor when `guy_mark == 1`; see the mechanics report. Treat the remaining tests
//! as evidence that the Rust matches the x86 reading, not that the reading is right. The
//! honest gap list is in `docs/mechanics/groups-guys.md` §"What is not derived".

use crate::trig::{find_angle, sin_table};

// ---------------------------------------------------------------------------
// Shapes and constants, all [measured]
// ---------------------------------------------------------------------------

/// Leader slots iterated by `check_guys` and `Groups::process`: the loops run
/// `leaders` `0x00E3A390` with stride `0x6EEC` while `ptr < 0x00E789DC`, which is
/// `(0xE789DC - 0xE3A390) / 0x6EEC = 8` exactly.
pub const NUM_LEADERS: usize = 8;

/// `Groups::process` `0x006FA210` indexes group `proc_group + who*0x40` and wraps
/// `proc_group` at `0x3F`, so each player owns 64 contiguous group slots.
pub const GROUPS_PER_PLAYER: usize = 64;

/// `groups.list` length implied by the two constants above.
pub const NUM_GROUPS: usize = NUM_LEADERS * GROUPS_PER_PLAYER;

/// `Group::add` `0x00714350` refuses at `num >= 0x80`, and every parallel array in
/// `GroupData` is 128 wide.
pub const GROUP_MAX_MEMBERS: usize = 128;

/// `sizeof(Group)`, the stride `check_groups` and `Groups::process` walk with (`0x9D4`).
pub const SIZEOF_GROUP: usize = 2516;

/// `sizeof(Guy)` — the `malloc` size in `PtrArray<Guy>::walk_data`'s read path is `0xF4`,
/// of which 4 bytes are the array cookie, so the object is `0xF0` = 240.
pub const SIZEOF_GUY: usize = 240;

/// `GuyData::walk_data` `0x005E0210` walks the flat range `[this+8, this+0xA3)`.
pub const GUY_WALK_LO: usize = 0x08;
/// Exclusive end of the guy checksum window. Excludes `last_norm`, `curr_bbox`,
/// `last_good_river*` and all of `GuyOut`.
pub const GUY_WALK_HI: usize = 0xA3;
/// Byte count of one guy's checksum image.
pub const GUY_WALK_LEN: usize = GUY_WALK_HI - GUY_WALK_LO; // 155

/// World units per formation-offset cell. `Group::update_positions` scales every
/// `off_x`/`off_y` by `lea edx,[eax+eax*2]; shl edx,4` = 48 — the same quarter-tile the
/// unit pathfinder's `astar_path` steps with.
pub const FORM_CELL: i32 = 48;

/// Turret slew per frame in `Guy::process`: `0x0AAAAAAA`, which is `2^32 / 24` — one
/// twenty-fourth of a turn, 15 degrees.
pub const TURRET_STEP: u32 = 0x0AAA_AAAA;

/// `Guy::turn_towards` snaps to the desired angle below this: `0x02222220`, one part in
/// 120 of a turn, i.e. 3 degrees.
pub const ANGLE_SNAP: u32 = 0x0222_2220;

/// `GuyData::turn_speed`'s default when the guy is crew rather than squad: a quarter turn.
pub const DEFAULT_TURN_SPEED: u32 = 0x4000_0000;

/// `GuyData::turn_speed`'s "face instantly" result: a half turn.
pub const SNAP_TURN_SPEED: u32 = 0x8000_0000;

/// The floor multiplier in `GuyData::turn_speed`: `imul esi, [constants+8], 0xB60B`.
pub const TURN_FLOOR_MUL: u32 = 0xB60B;

/// `vector_dist` switches to a cheaper form once the *smaller* leg reaches this.
pub const VECTOR_DIST_BIG: i32 = 60_000;

/// `Guy::move` scales `get_speed()` by 11/8 before stepping.
pub const SPEED_NUM: i32 = 11;
/// Denominator of the same.
pub const SPEED_DEN: i32 = 8;

// `GuyData::guy_flags` bits used by the ported code. Only the bits the ported paths test
// are named; the rest of the word is carried opaquely.
/// Cleared unconditionally at the top of `Guy::process` (`and 0xFFFD`); while set,
/// `Guy::move` skips the idle `turn_towards`.
pub const GUY_FLAG_NO_IDLE_TURN: u16 = 0x0002;
/// Cleared at the end of `Guy::process`'s periodic branch (`and 0xFFDF`).
pub const GUY_FLAG_BLOCK_DIRTY: u16 = 0x0020;
/// While set, `Guy::move` does not refresh `last_z`.
pub const GUY_FLAG_HOLD_Z: u16 = 0x0040;
/// With `last_speed == 0`, forces `turn_speed` to a half turn.
pub const GUY_FLAG_FAST_FACE: u16 = 0x0010;
/// Gates the turret-settling loop in `Guy::process`.
pub const GUY_FLAG_TURRETS: u16 = 0x0100;

/// `UnitData::unit_masks` bit tested by `GuyData::turn_speed` for the second scale.
pub const UNIT_MASK_TURN_SCALE2: u32 = 0x0008_0000;

// ---------------------------------------------------------------------------
// The trig folds `Guy::move` and `Group::update_positions` actually call
// ---------------------------------------------------------------------------
//
// `crate::trig` already carries `sin_table` `0x00A46A00` and the two raw cosine entries.
// It does **not** carry `sinx` `0x0092D100` and `cosx` `0x0092D0C0`, and those are not the
// same function: they pre-fold the angle into `[0, 0x3FFFFFFF]` before calling, so the
// second-quarter branch inside `sin_table` — the one that is *not* a mirror — is never
// reached through them. Calling `sin_table` directly where retail calls `sinx` gives a
// different number in two of the four quadrants.

/// `sinx` `0x0092D100` — `mag * sin(angle)`, with retail's fold.
///
/// ```text
/// if (mag == 0) return 0;                      ; test edx,edx / jne
/// if (angle < 0) { mag = -mag; angle &= 0x7FFFFFFF; }
/// a = (angle & 0x40000000) ? 0x7FFFFFFF - angle : angle;
/// return sin_table(a, mag);
/// ```
///
/// The reflection through `0x7FFFFFFF` (not `0x80000000`) and the byte-wrapping table
/// index at 255 together produce an `i32` overflow at exactly the quarter boundaries whose
/// wrap lands on the right answer — `cosx(0, r) == r` only because `-65535 * 0x3FFFFF`
/// wraps. Do not "fix" it.
#[inline]
pub fn sinx(angle: i32, mag: i32) -> i32 {
    if mag == 0 {
        return 0;
    }
    let (a, m) = if angle < 0 {
        (angle & 0x7FFF_FFFF, mag.wrapping_neg())
    } else {
        (angle, mag)
    };
    let folded = if a & 0x4000_0000 != 0 {
        0x7FFF_FFFFi32 - a
    } else {
        a
    };
    sin_table(folded, m)
}

/// `cosx` `0x0092D0C0` — `sinx(angle + 0x40000000, mag)`, with the add done before the fold.
#[inline]
pub fn cosx(angle: i32, mag: i32) -> i32 {
    sinx(angle.wrapping_add(0x4000_0000), mag)
}

/// `angle_diff` `0x0092D0B0` — the unsigned magnitude of a binary angle difference.
///
/// Note `not ecx`, not `neg ecx`: the reflected branch is off by one from a true absolute
/// value, and every caller inherits that.
#[inline]
pub fn angle_diff(a: u32, b: u32) -> u32 {
    let d = a.wrapping_sub(b);
    if d > 0x8000_0000 {
        !d
    } else {
        d
    }
}

/// `vector_dist` `0x0046CFF0` — the engine's integer `hypot`, used for `Guy::last_speed`.
///
/// `max + min*min / (2*max)`, with the division unsigned, degrading to `max + min/2` once
/// the smaller leg reaches 60000 (to keep `min*min` inside `i32`).
#[inline]
pub fn vector_dist(a: i32, b: i32) -> i32 {
    let aa = a.wrapping_abs();
    let ab = b.wrapping_abs();
    let (big, small) = if aa > ab { (aa, ab) } else { (ab, aa) };
    if big == 0 {
        return 0;
    }
    if small >= VECTOR_DIST_BIG {
        // lea eax,[small + big*2]; shr eax,1
        return ((small.wrapping_add(big.wrapping_mul(2))) as u32 >> 1) as i32;
    }
    // imul small,small ; lea ecx,[big+big] ; div ecx ; add eax, big
    let num = (small.wrapping_mul(small)) as u32;
    let den = (big.wrapping_mul(2)) as u32;
    (num / den) as i32 + big
}

// ---------------------------------------------------------------------------
// Adler-32 and the CheckSum visitor
// ---------------------------------------------------------------------------

/// The lockstep checksum primitive, re-exported from [`crate::checksum`].
///
/// Correction to this module's previous comment: the binary contains **two** `adler32`
/// procedures — `_adler32` `0x005089d0` (301 B, `__cdecl`, BHG's copy in `main/basic`) and
/// `adler32` `0x00a46830` (295 B, `__fastcall`), whose bodies are structurally identical
/// (`NMAX = 0x15b0`, 16-way unroll, null → 1). The one on the checksum path is
/// `0x00a46830`: `CheckSum::walk_function` `0x00936ff0` is `call 0xa46830` at `0x0093700a`
/// [measured]. `crate::checksum::adler32` is that one.
pub use crate::checksum::adler32;

/// The `CheckSum` half of the `DataWalk` interface: `walk_function` (slot 0) folds a byte
/// range into a running adler at `CheckSum +0x10`.
///
/// `CheckSum` (singular) is the visitor; `CheckSums` (plural) is the driver holding the
/// 54 `check_*` methods. Only the visitor is modelled here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckSum {
    /// The running value at `CheckSum +0x10`.
    pub value: u32,
}

impl Default for CheckSum {
    fn default() -> Self {
        // adler32's identity seed. `check_all` threads one value through all fifteen
        // channels; the seed it starts from is set by the caller, not here.
        CheckSum { value: 1 }
    }
}

impl CheckSum {
    /// `DataWalk::walk_function(begin, end)` as `CheckSum` implements it.
    #[inline]
    pub fn walk(&mut self, bytes: &[u8]) {
        self.value = adler32(self.value, bytes);
    }
}

// ---------------------------------------------------------------------------
// GuyData — the checksummed state of one soldier
// ---------------------------------------------------------------------------

/// One `Guy`'s simulation state, laid out to match `GuyData` `+0x08 ..= +0xA2`.
///
/// Field names and offsets are the PDB's, not invented. The five `f32`s at `+0x40..+0x54`
/// are inside the checksum window and are therefore sim-critical bytes even though they
/// are floats: `turret_inc`, `bank`, `last_bank`, `pitch`, `last_pitch`. None of the paths
/// ported here write them, but a full port must, and must do it bit-exactly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuyData {
    /// `+0x08` `type : TypeIndex`
    pub ty: i32,
    /// `+0x0C` `x : Coord`
    pub x: i32,
    /// `+0x10` `y : Coord`
    pub y: i32,
    /// `+0x14` `z : Coord`
    pub z: i32,
    /// `+0x18` binary angle, 2^32 per turn, 0 = north (-y), 0x40000000 = east
    pub angle: i32,
    /// `+0x1C`
    pub last_angle: i32,
    /// `+0x20`
    pub turret_angles: [i32; 4],
    /// `+0x30`
    pub des_turret_angles: [i32; 4],
    /// `+0x40`
    pub turret_inc: f32,
    /// `+0x44`
    pub bank: f32,
    /// `+0x48`
    pub last_bank: f32,
    /// `+0x4C`
    pub pitch: f32,
    /// `+0x50`
    pub last_pitch: f32,
    /// `+0x54`
    pub track_dx: i32,
    /// `+0x58`
    pub track_dy: i32,
    /// `+0x5C` destination, in world units
    pub des_x: i32,
    /// `+0x60`
    pub des_y: i32,
    /// `+0x64`
    pub des_angle: i32,
    /// `+0x68`
    pub last_x: i32,
    /// `+0x6C`
    pub last_y: i32,
    /// `+0x70`
    pub last_z: i32,
    /// `+0x74`
    pub cur_time: u32,
    /// `+0x78`
    pub end_time: u32,
    /// `+0x7C`
    pub last_time: i32,
    /// `+0x80` distance actually covered this frame
    pub last_speed: i32,
    /// `+0x84` smoothed speed: `(avg*3 + last) / 4` every frame
    pub avg_speed: i32,
    /// `+0x88`
    pub gpiece: i32,
    /// `+0x8C` index of the owning unit inside `objects.lists[who]`
    pub o: i16,
    /// `+0x8E`
    pub ox: i16,
    /// `+0x90`
    pub variation: i16,
    /// `+0x92` this guy's offset inside its squad's formation
    pub off_x: i16,
    /// `+0x94`
    pub off_y: i16,
    /// `+0x96`
    pub node_flags: i16,
    /// `+0x98`
    pub des_node_flags: i16,
    /// `+0x9A`
    pub guy_flags: u16,
    /// `+0x9C` current `UnitAnim`
    pub cur_anim: i8,
    /// `+0x9D`
    pub stopped: i8,
    /// `+0x9E`
    pub hold_attack: i8,
    /// `+0x9F`
    pub whom: i8,
    /// `+0xA0`
    pub queued_attack: i8,
    /// `+0xA1` owner slot of the owning unit
    pub who: i8,
    /// `+0xA2` index within the unit's `guys` array
    pub guy_num: i8,
}

impl Default for GuyData {
    fn default() -> Self {
        GuyData {
            ty: 0,
            x: 0,
            y: 0,
            z: 0,
            angle: 0,
            last_angle: 0,
            turret_angles: [0; 4],
            des_turret_angles: [0; 4],
            turret_inc: 0.0,
            bank: 0.0,
            last_bank: 0.0,
            pitch: 0.0,
            last_pitch: 0.0,
            track_dx: 0,
            track_dy: 0,
            des_x: 0,
            des_y: 0,
            des_angle: 0,
            last_x: 0,
            last_y: 0,
            last_z: 0,
            cur_time: 0,
            end_time: 0,
            last_time: 0,
            last_speed: 0,
            avg_speed: 0,
            gpiece: 0,
            o: 0,
            ox: 0,
            variation: 0,
            off_x: 0,
            off_y: 0,
            node_flags: 0,
            des_node_flags: 0,
            guy_flags: 0,
            cur_anim: 0,
            stopped: 0,
            hold_attack: 0,
            whom: 0,
            queued_attack: 0,
            who: 0,
            guy_num: 0,
        }
    }
}

impl GuyData {
    /// The exact 155 bytes `GuyData::walk_data` hands the visitor, little-endian.
    ///
    /// `GuyData::walk_data` `0x005E0210` is four instructions of substance:
    /// `lea eax,[ecx+0xA3]; lea eax,[ecx+8]; call [dw]` — one flat range, no field-wise
    /// walk, no mask bits. So the image is the raw struct bytes and there are no holes:
    /// every offset from `+0x08` to `+0xA2` is a named field.
    pub fn walk_bytes(&self) -> [u8; GUY_WALK_LEN] {
        let mut out = [0u8; GUY_WALK_LEN];
        let mut p = 0usize;
        macro_rules! w32 {
            ($v:expr) => {{
                out[p..p + 4].copy_from_slice(&($v as i32).to_le_bytes());
                p += 4;
            }};
        }
        macro_rules! wu32 {
            ($v:expr) => {{
                out[p..p + 4].copy_from_slice(&($v as u32).to_le_bytes());
                p += 4;
            }};
        }
        macro_rules! wf32 {
            ($v:expr) => {{
                out[p..p + 4].copy_from_slice(&($v as f32).to_le_bytes());
                p += 4;
            }};
        }
        macro_rules! w16 {
            ($v:expr) => {{
                out[p..p + 2].copy_from_slice(&($v as i16).to_le_bytes());
                p += 2;
            }};
        }
        macro_rules! wu16 {
            ($v:expr) => {{
                out[p..p + 2].copy_from_slice(&($v as u16).to_le_bytes());
                p += 2;
            }};
        }
        macro_rules! w8 {
            ($v:expr) => {{
                out[p] = ($v as i8) as u8;
                p += 1;
            }};
        }

        w32!(self.ty);
        w32!(self.x);
        w32!(self.y);
        w32!(self.z);
        w32!(self.angle);
        w32!(self.last_angle);
        for v in self.turret_angles {
            w32!(v);
        }
        for v in self.des_turret_angles {
            w32!(v);
        }
        wf32!(self.turret_inc);
        wf32!(self.bank);
        wf32!(self.last_bank);
        wf32!(self.pitch);
        wf32!(self.last_pitch);
        w32!(self.track_dx);
        w32!(self.track_dy);
        w32!(self.des_x);
        w32!(self.des_y);
        w32!(self.des_angle);
        w32!(self.last_x);
        w32!(self.last_y);
        w32!(self.last_z);
        wu32!(self.cur_time);
        wu32!(self.end_time);
        w32!(self.last_time);
        w32!(self.last_speed);
        w32!(self.avg_speed);
        w32!(self.gpiece);
        w16!(self.o);
        w16!(self.ox);
        w16!(self.variation);
        w16!(self.off_x);
        w16!(self.off_y);
        w16!(self.node_flags);
        w16!(self.des_node_flags);
        wu16!(self.guy_flags);
        w8!(self.cur_anim);
        w8!(self.stopped);
        w8!(self.hold_attack);
        w8!(self.whom);
        w8!(self.queued_attack);
        w8!(self.who);
        w8!(self.guy_num);
        debug_assert_eq!(p, GUY_WALK_LEN);
        out
    }
}

// ---------------------------------------------------------------------------
// The static rule data a guy's motion reads
// ---------------------------------------------------------------------------

/// The `UnitTypeData` / `ObjectTypeData` fields the guy and group code reads, at the
/// offsets `crates/don-rules/src/offsets.rs` binds them to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UnitTypeStats {
    /// `ObjectTypeData::domain` `+0x218`. 2 suppresses collision-block registration.
    pub domain: i32,
    /// `ObjectTypeData::guy_spacing` `+0x224` (`<GUY_SPACING>` in `unitrules.xml`).
    pub guy_spacing: i32,
    /// `ObjectTypeData::x_spacing` `+0x228`.
    pub x_spacing: i32,
    /// `ObjectTypeData::y_spacing` `+0x22C`.
    pub y_spacing: i32,
    /// `ObjectTypeData::guy_radius` `+0x23C`.
    pub guy_radius: i32,
    /// `ObjectTypeData::new_block_radius` `+0x248`; 0 suppresses block registration.
    pub new_block_radius: i32,
    /// `UnitTypeData::turn_speed` `+0x2C4` (`<TURN_SPEED>`).
    pub turn_speed: i32,
    /// `UnitTypeData::role` `+0x2C8`, OR-accumulated into `GroupData::role` by `Group::add`.
    pub role: i32,
    /// `UnitTypeData::squad_size` `+0x304`. Soldiers that fight and block.
    pub squad_size: i32,
    /// `UnitTypeData::uber_size` `+0x308` (`<UBER_SIZE>`).
    pub uber_size: i32,
    /// `UnitTypeData::crew_size` `+0x30C` (`<CREW_SIZE>`). Extra bodies, no collision.
    pub crew_size: i32,
    /// `UnitTypeData::base_form` `+0x310`.
    pub base_form: i32,
}

/// Everything outside the guy that `Guy::move` / `Guy::process` read, gathered so the
/// ported functions take no world reference.
#[derive(Clone, Copy, Debug, Default)]
pub struct GuyEnv {
    /// The owning unit's type rules.
    pub ut: UnitTypeStats,
    /// `Unit::speed()` `0x0060AAE0` via the owning unit's vtable slot `+0x17C`.
    pub unit_speed: i32,
    /// Result of `get_order()->vt[+0x2C]()` — when true, `GuyData::get_speed` adds 9.
    pub order_speed_bonus: bool,
    /// `UnitData::unit_masks & 0x00080000`, the second turn-speed scale gate.
    pub unit_mask_turn_scale2: bool,
    /// `Constants +0x08`, the turn-speed scale.
    pub turn_scale: i32,
    /// `Constants +0x0C`, the extra scale behind `unit_mask_turn_scale2`.
    pub turn_scale2: i32,
    /// `GameAccess::ai_speed` `[0x00C061C0]`; values above 1 multiply the step.
    pub ai_speed: i32,
}

impl GuyEnv {
    /// A neutral environment: both rule scales 1, `ai_speed` 1.
    pub fn with_type(ut: UnitTypeStats) -> Self {
        GuyEnv {
            ut,
            turn_scale: 1,
            turn_scale2: 1,
            ai_speed: 1,
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Guy behaviour
// ---------------------------------------------------------------------------

impl GuyData {
    /// True when this guy is a fighting member of the squad rather than crew.
    ///
    /// The predicate `guy_num < squad_size` gates three separate things in retail:
    /// the type-derived turn speed, the `+9` speed bonus, and collision-block
    /// registration. Crew guys (`guy_num >= squad_size`) are bodies only.
    #[inline]
    pub fn is_squad(&self, ut: &UnitTypeStats) -> bool {
        (self.guy_num as i32) < ut.squad_size
    }

    /// `GuyData::get_speed` `0x005DE410`.
    ///
    /// ```text
    /// base = unit->vt[0x17C](x, y, 1)            ; UnitData::speed
    /// if (guy_num < squad_size && unit has an order && order->vt[0x2C]())
    ///     return base + 9;
    /// return base;
    /// ```
    #[inline]
    pub fn get_speed(&self, env: &GuyEnv) -> i32 {
        if self.is_squad(&env.ut) && env.order_speed_bonus {
            env.unit_speed.wrapping_add(9)
        } else {
            env.unit_speed
        }
    }

    /// `GuyData::turn_speed` `0x005DE340` — the angular step allowed this frame.
    ///
    /// `arg` is retail's single parameter: non-zero means "give me the raw step", zero
    /// means "give me the step damped by how fast I am already moving".
    ///
    /// ```text
    /// step = 0x40000000                                  ; crew default, a quarter turn
    /// if (guy_num < squad_size) {
    ///     step = (type->turn_speed >> 8) * constants[+8];
    ///     if (unit->unit_masks & 0x80000) step *= constants[+0xC];
    /// } else if (track_dx || track_dy) return 0x40000000;
    /// if (last_speed == 0 && (guy_flags & 0x10)) return 0x80000000;
    /// if (arg) return step;
    /// floor = constants[+8] * 0xB60B;
    /// q = step / (avg_speed/4 + 1);                       ; UNSIGNED div
    /// return q > floor ? q : floor;                       ; unsigned compare
    /// ```
    ///
    /// The shift `>> 8` is logical on an `int` field, so a `turn_speed` rule value scales
    /// into the binary-angle space by 1/256 before the rule multiplier is applied.
    pub fn turn_speed(&self, env: &GuyEnv, arg: i32) -> u32 {
        let mut step = DEFAULT_TURN_SPEED;
        if self.is_squad(&env.ut) {
            step = ((env.ut.turn_speed as u32) >> 8).wrapping_mul(env.turn_scale as u32);
            if env.unit_mask_turn_scale2 {
                step = step.wrapping_mul(env.turn_scale2 as u32);
            }
        } else if self.track_dx != 0 || self.track_dy != 0 {
            return DEFAULT_TURN_SPEED;
        }
        if self.last_speed == 0 && (self.guy_flags & GUY_FLAG_FAST_FACE) != 0 {
            return SNAP_TURN_SPEED;
        }
        if arg != 0 {
            return step;
        }
        let floor = (env.turn_scale as u32).wrapping_mul(TURN_FLOOR_MUL);
        // `mov eax,[avg_speed]; cdq; and edx,3; lea ecx,[edx+eax]; sar ecx,2; inc ecx`
        // is signed division by 4 rounding toward zero, then +1.
        let den = ((self.avg_speed / 4) as u32).wrapping_add(1);
        if den == 0 {
            return floor;
        }
        let q = step / den;
        if q > floor {
            q
        } else {
            floor
        }
    }

    /// `Guy::turn_towards` `0x005D9720` — slew `angle` toward `des`, return what is left.
    ///
    /// The remainder is what `Guy::move` compares against `2 * turn_speed(1)` to decide
    /// whether it may translate this frame at all.
    ///
    /// Retail applies the new angle through `Guy::do_turn` `0x005D97A0`. Its
    /// `0x005D97B2..0x005D97BE` head marks a changed facing with
    /// [`GUY_FLAG_NO_IDLE_TURN`] before storing the angle. The later animation/pivot path
    /// is not yet ported.
    pub fn turn_towards(&mut self, des: u32, env: &GuyEnv) -> u32 {
        let (new_angle, rem) = self.turn_solve(des, env, false);
        if new_angle != self.angle as u32 {
            self.guy_flags |= GUY_FLAG_NO_IDLE_TURN;
        }
        self.angle = new_angle as i32;
        rem
    }

    /// `Guy::turn_angles` `0x005D98C0` — the same slew, without mutating, with an optional
    /// half step. Returns `(new_angle, remaining)`.
    ///
    /// `half` is retail's fourth argument: `shr edx, 1` on the step.
    pub fn turn_angles(&self, des: u32, env: &GuyEnv, half: bool) -> (u32, u32) {
        self.turn_solve(des, env, half)
    }

    fn turn_solve(&self, des: u32, env: &GuyEnv, half: bool) -> (u32, u32) {
        let cur = self.angle as u32;
        let diff = des.wrapping_sub(cur);
        // `cmp esi,0x80000000; jbe; not esi` — `not`, deliberately, not `neg`.
        let mag = if diff > 0x8000_0000 { !diff } else { diff };
        if mag < ANGLE_SNAP {
            return (des, 0);
        }
        let mut step = self.turn_speed(env, 1);
        if half {
            step >>= 1;
        }
        if mag > step {
            let new = if diff <= 0x8000_0000 {
                cur.wrapping_add(step)
            } else {
                cur.wrapping_sub(step)
            };
            (new, mag - step)
        } else {
            (des, 0)
        }
    }

    /// The `avg_speed` filter that closes every `Guy::move`.
    ///
    /// `iVar = avg*3 + last; avg = (iVar + ((iVar>>31)&3)) >> 2` — signed division by 4
    /// rounding toward zero. It runs on both the stopped and the moving path, but **not**
    /// on the early return when the guy is too far off-angle to translate.
    #[inline]
    pub fn update_avg_speed(&mut self) {
        let t = self.avg_speed.wrapping_mul(3).wrapping_add(self.last_speed);
        self.avg_speed = t / 4;
    }

    /// `Guy::move` `0x005D9240` — the per-frame movement integrator.
    ///
    /// Returns the reason the frame ended, which is the only thing outside `self` that a
    /// caller needs. `[structure]` from `re/decomp-all/005d9240.c`, values from the
    /// instruction stream.
    ///
    /// ```text
    /// last_x = x; last_y = y; if (!(guy_flags & 0x40)) last_z = z; last_angle = angle;
    /// if (des_x == x && des_y == y) {
    ///     last_speed = 0;
    ///     ...animation selection...
    ///     if (!(guy_flags & 2)) turn_towards(des_angle);
    /// } else if (guy_num == 0 || (track_dx == 0 && track_dy == 0)) {
    ///     last_speed = vector_dist(des_x - x, des_y - y);
    ///     set_new_location(des_x, des_y);                 ; teleport-to-destination path
    /// } else {
    ///     a   = find_angle(des_x - x, des_y - y);
    ///     rem = turn_towards(a);
    ///     if (rem > turn_speed(1) * 2) return;            ; turn only, no avg_speed update
    ///     step = get_speed() * 11 / 8;  last_speed = step;
    ///     if (ai_speed > 1) step *= ai_speed;
    ///     if (|dy| + |dx| <= step) set_new_location(des_x, des_y);
    ///     else {
    ///         sx = sinx(angle, step); sy = cosx(angle, step);
    ///         if (|dx| < |sx|) sx = dx;
    ///         if (|dy| < |sy|) sy = -dy;
    ///         x -= off_x; y -= off_y;
    ///         if (world.valid(x + sx, y - sy)) set_new_location(x + sx, y - sy);
    ///         else stopped = 0;
    ///     }
    /// }
    /// avg_speed = (avg_speed*3 + last_speed) / 4;
    /// ```
    ///
    /// The `x -= off_x; y -= off_y` before the step is retail's, and it is why
    /// `GuyData::off_x/off_y` are sim-critical rather than cosmetic: the guy's stored
    /// position carries its formation offset, and the step is taken from the unit anchor.
    ///
    /// `world_valid` stands in for `WorldData::valid` `0x0043F360`.
    pub fn r#move(&mut self, env: &GuyEnv, world_valid: &dyn Fn(i32, i32) -> bool) -> MoveOutcome {
        self.last_x = self.x;
        self.last_y = self.y;
        if self.guy_flags & GUY_FLAG_HOLD_Z == 0 {
            self.last_z = self.z;
        }
        self.last_angle = self.angle;

        if self.des_x == self.x && self.des_y == self.y {
            self.last_speed = 0;
            if self.guy_flags & GUY_FLAG_NO_IDLE_TURN == 0 {
                self.turn_towards(self.des_angle as u32, env);
            }
            self.update_avg_speed();
            return MoveOutcome::Idle;
        }

        let dx = self.des_x.wrapping_sub(self.x);
        let dy = self.des_y.wrapping_sub(self.y);

        if self.guy_num == 0 || (self.track_dx == 0 && self.track_dy == 0) {
            self.last_speed = vector_dist(dx, dy);
            self.x = self.des_x;
            self.y = self.des_y;
            self.update_avg_speed();
            return MoveOutcome::Snapped;
        }

        let want = find_angle(dx, dy);
        let rem = self.turn_towards(want as u32, env);
        if rem > self.turn_speed(env, 1).wrapping_mul(2) {
            // Retail returns here: no translation, and **no `avg_speed` update**.
            return MoveOutcome::TurnedOnly;
        }

        let raw = self.get_speed(env);
        let mut step = raw.wrapping_mul(SPEED_NUM) / SPEED_DEN;
        self.last_speed = step;
        if env.ai_speed > 1 {
            step = step.wrapping_mul(env.ai_speed);
        }

        if dy.wrapping_abs().wrapping_add(dx.wrapping_abs()) <= step {
            self.x = self.des_x;
            self.y = self.des_y;
            self.update_avg_speed();
            return MoveOutcome::Snapped;
        }

        let mut sx = sinx(self.angle, step);
        let mut sy = cosx(self.angle, step);
        if dx.wrapping_abs() < sx.wrapping_abs() {
            sx = dx;
        }
        if dy.wrapping_abs() < sy.wrapping_abs() {
            sy = dy.wrapping_neg();
        }
        self.x = self.x.wrapping_sub(self.off_x as i32);
        self.y = self.y.wrapping_sub(self.off_y as i32);
        let nx = self.x.wrapping_add(sx);
        let ny = self.y.wrapping_sub(sy);
        let outcome = if world_valid(nx, ny) {
            self.x = nx;
            self.y = ny;
            MoveOutcome::Stepped
        } else {
            self.stopped = 0;
            MoveOutcome::Blocked
        };
        self.update_avg_speed();
        outcome
    }

    /// `Guy::process` `0x005E0230` — the per-guy tick, called at the tail of
    /// `Unit::process` `0x00610BC0`.
    ///
    /// ```text
    /// Guy::move();
    /// guy_flags &= ~0x0002;
    /// if (guy_flags & 0x0100) settle the four turrets by 0x0AAAAAAA per frame;
    /// if ((game.frame + o) % 64 == 0 && avg_speed == 0) {
    ///     if (type->domain != 2 && guy_num < squad_size && type->new_block_radius != 0)
    ///         register this guy's collision-block bits;
    ///     guy_flags &= ~0x0020;
    /// }
    /// ```
    ///
    /// The `(frame + o) % 64` phase is per-unit, so collision blocks refresh on a
    /// 64-frame rotation spread across the object array — one more ordering fact a
    /// scheduler has to reproduce.
    ///
    /// Block registration itself is **not ported**: it walks a radius-indexed offset table
    /// (`0x00ADD1E0` counts, `0x00ADC400`/`0x00ADCAF0` offsets) into `World::new_coll_block`
    /// `0x0046D250`, which is the world lane's state. `register_blocks` is the hook; the
    /// gating predicate is exact.
    pub fn process(
        &mut self,
        env: &GuyEnv,
        frame: i32,
        world_valid: &dyn Fn(i32, i32) -> bool,
        register_blocks: &mut dyn FnMut(&GuyData),
    ) -> MoveOutcome {
        let outcome = self.r#move(env, world_valid);
        self.guy_flags &= !GUY_FLAG_NO_IDLE_TURN;

        if self.guy_flags & GUY_FLAG_TURRETS != 0 {
            for i in 0..4 {
                let cur = self.turret_angles[i] as u32;
                let des = self.des_turret_angles[i] as u32;
                if cur == des {
                    self.node_flags |= 1i16 << i;
                    continue;
                }
                let diff = cur.wrapping_sub(des);
                let mag = if diff > 0x8000_0000 { !diff } else { diff };
                if mag < TURRET_STEP {
                    self.turret_angles[i] = des as i32;
                    self.node_flags |= 1i16 << i;
                } else if diff < 0x8000_0000 {
                    self.turret_angles[i] = cur.wrapping_sub(TURRET_STEP) as i32;
                } else {
                    self.turret_angles[i] = cur.wrapping_add(TURRET_STEP) as i32;
                }
            }
        }

        if (frame.wrapping_add(self.o as i32)) % 64 == 0 && self.avg_speed == 0 {
            if env.ut.domain != 2 && self.is_squad(&env.ut) && env.ut.new_block_radius != 0 {
                register_blocks(self);
            }
            self.guy_flags &= !GUY_FLAG_BLOCK_DIRTY;
        }
        outcome
    }
}

/// What one `Guy::move` did, so callers can see the early-return path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveOutcome {
    /// Already at the destination; only the idle turn ran.
    Idle,
    /// Jumped straight to the destination (guy 0, untracked guy, or within one step).
    Snapped,
    /// Too far off-angle to translate. **`avg_speed` was not updated.**
    TurnedOnly,
    /// Translated by one step.
    Stepped,
    /// The step landed off-map; position unchanged apart from the offset subtraction.
    Blocked,
}

// ---------------------------------------------------------------------------
// Guy allocation — how many soldiers a unit has
// ---------------------------------------------------------------------------

/// The `guys` array of one unit, plus `UnitData::guy_mark`.
///
/// `guy_mark` (`UnitData +0xB5`, a `char`) is the count of *initialised squad* guys, and
/// is clamped to `squad_size` by `Unit::set_type`. Crew guys always occupy
/// `[squad_size, squad_size + crew_size)` and are allocated whole.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UnitGuys {
    /// `PtrArray<Guy>::list`. `None` is a null slot, which the checksum records.
    pub guys: Vec<Option<GuyData>>,
    /// `PtrArray<Guy>` `+0x08`, capacity.
    pub size: i32,
    /// `PtrArray<Guy>` `+0x0C`, the `ArrayBaseMaster::increment` short.
    pub increment: i16,
    /// `PtrArray<Guy>` `+0x14`.
    pub flags: u8,
    /// `UnitData::guy_mark` `+0xB5`.
    pub guy_mark: i8,
}

impl UnitGuys {
    /// `PtrArray<Guy>::length` — the count the checksum leads with.
    pub fn len(&self) -> usize {
        self.guys.len()
    }

    /// True when the unit has no guys at all.
    pub fn is_empty(&self) -> bool {
        self.guys.is_empty()
    }

    /// Total guys a unit of this type carries: `squad_size + crew_size`.
    ///
    /// Straight from `Unit::set_type` `0x00612FA0`:
    /// `guys.length = unittypes[t]->crew_size + unittypes[t]->squad_size` at `0x006131E?`,
    /// then crew slots `[squad_size, length)` are filled from `Recycler<Guy>::pop`.
    pub fn count_for(ut: &UnitTypeStats) -> usize {
        (ut.squad_size + ut.crew_size).max(0) as usize
    }

    /// Rebuild a unit's guy pointer array the way the full (`param_2 == 0`)
    /// `Unit::set_type` path does.
    ///
    /// - `guys.length = squad_size + crew_size`
    /// - every old crew pointer is killed; the new crew range is freshly allocated
    /// - `guy_mark = min(previous guy_mark, squad_size)`; squad guys `[0, guy_mark)` are
    ///   re-typed and given `guy_num = i`
    /// - crew guys `[squad_size, length)` are given `guy_num = i`, `who`, `o` and re-typed
    /// - squad slots in `[guy_mark, squad_size)` are left null
    ///
    /// The last point is the one that matters: **a squad's live soldier count is
    /// `guy_mark`, and the slots past it are genuinely null pointers**, which the
    /// checksum's presence-bit pass observes.
    pub fn set_type(
        &mut self,
        ty: i32,
        who: i8,
        o: i16,
        old_ut: &UnitTypeStats,
        ut: &UnitTypeStats,
    ) {
        let total = Self::count_for(ut);
        let old_squad = old_ut.squad_size.max(0) as usize;
        let mut old = std::mem::take(&mut self.guys);
        let mark = (self.guy_mark as i32).min(ut.squad_size).max(0);
        self.guys.resize(total, None);
        let needed = total as i32 - self.size;
        if needed > 0 {
            // Unit::set_type's call to ArrayBaseSimpleCopy<Guy*>::increase_size: a
            // non-negative increment is the minimum growth quantum; a negative increment
            // does not take increase_size's doubling sentinel here because the caller
            // supplies the exact deficit instead.
            let growth = if self.increment >= 0 {
                needed.max(self.increment as i32)
            } else {
                needed
            };
            self.size = self.size.wrapping_add(growth);
        }
        self.guy_mark = mark as i8;

        // Retail retains only the still-live squad prefix.  Old crew starts at the old
        // type's squad boundary and is killed before any new crew is allocated; growing a
        // squad leaves the new squad slots null rather than recycling those old crew Guys.
        for i in 0..mark as usize {
            let g = self.guys[i].insert(old.get_mut(i).and_then(Option::take).unwrap_or_default());
            g.ty = ty;
            g.guy_num = i as i8;
            g.who = who;
            g.o = o;
        }
        for slot in old.iter_mut().skip(old_squad) {
            // Make the old-crew destruction explicit.  Dropping the old vector would have
            // the same Rust result, but this mirrors the pointer-array transition being
            // documented and prevents a future optimization from reusing a crew slot.
            *slot = None;
        }
        for i in ut.squad_size.max(0) as usize..total {
            let g = self.guys[i].insert(GuyData::default());
            g.ty = ty;
            g.guy_num = i as i8;
            g.who = who;
            g.o = o;
        }
    }

    /// A brand-new unit: every squad slot live, `guy_mark = squad_size`.
    pub fn spawn_full(ty: i32, who: i8, o: i16, ut: &UnitTypeStats) -> Self {
        let mut u = UnitGuys {
            guy_mark: ut.squad_size.clamp(0, i8::MAX as i32) as i8,
            ..Default::default()
        };
        u.guys.resize(Self::count_for(ut), None);
        u.size = u.guys.len() as i32;
        for i in 0..u.guys.len() {
            let mut g = GuyData::default();
            g.ty = ty;
            g.who = who;
            g.o = o;
            g.guy_num = i as i8;
            u.guys[i] = Some(g);
        }
        u
    }

    /// Compute the live squad destinations from `Unit::set_new_location` `0x005F8D20`.
    ///
    /// This is the initial materialization path called by `Unit::init` after all Guys have
    /// been allocated and initialized.  It is a rotated, centered lattice whose width is
    /// normally three and is two for formation byte 8 (`Column`).  Formation byte 5
    /// (`Sparse`) doubles `ObjectTypeData::guy_spacing`; `unit_masks & 2` reverses both
    /// basis vectors.  Every multiply and add is wrapping x86 arithmetic, and `/ 2` is
    /// signed truncation toward zero.
    ///
    /// `world_max_x/y` are the exclusive retail coordinate bounds (`WorldData::tile_xs/ys
    /// * 192`).  `Unit::set_new_location` clamps each destination before teleporting the
    /// Guy, so the edge behavior belongs here rather than in a host.
    pub fn initial_squad_locations(
        live_count: usize,
        anchor_x: i32,
        anchor_y: i32,
        angle: i32,
        formation: i8,
        unit_masks: u32,
        world_max_x: i32,
        world_max_y: i32,
        ut: &UnitTypeStats,
    ) -> Vec<(i32, i32)> {
        if live_count == 0 {
            return Vec::new();
        }
        let n = live_count.min(i32::MAX as usize) as i32;
        let mut spacing = ut.guy_spacing;
        if formation == Formation::Sparse as i8 {
            spacing = spacing.wrapping_mul(2);
        }
        let mut a = sinx(angle.wrapping_sub(0x4000_0000), spacing);
        let mut b = sinx(angle, spacing);
        if unit_masks & 2 != 0 {
            a = a.wrapping_neg();
            b = b.wrapping_neg();
        }

        let columns = if formation == Formation::Column as i8 {
            if n == 1 {
                1
            } else {
                2
            }
        } else {
            n.min(3)
        };
        let last_row = (n - 1) / columns;
        let last_column = columns - 1;
        let center_x = last_row
            .wrapping_mul(b)
            .wrapping_div(2)
            .wrapping_sub(last_column.wrapping_mul(a).wrapping_div(2));
        let center_y = last_row
            .wrapping_mul(a)
            .wrapping_div(2)
            .wrapping_add(last_column.wrapping_mul(b).wrapping_div(2));

        let clamp_coord = |v: i32, max: i32| {
            if v < 0 {
                0
            } else if v >= max {
                max.wrapping_sub(1)
            } else {
                v
            }
        };
        let mut out = Vec::with_capacity(live_count);
        for i in 0..n {
            let row = i / columns;
            let column = i % columns;
            let x = column
                .wrapping_mul(a)
                .wrapping_sub(row.wrapping_mul(b))
                .wrapping_add(anchor_x)
                .wrapping_add(center_x);
            let y = anchor_y
                .wrapping_sub(column.wrapping_mul(b))
                .wrapping_sub(row.wrapping_mul(a))
                .wrapping_add(center_y);
            out.push((clamp_coord(x, world_max_x), clamp_coord(y, world_max_y)));
        }
        out
    }

    /// Materialize every live, non-null squad Guy at retail's initial lattice position.
    ///
    /// `Guy::clear` `0x005DB590` initializes the packed `off_x/off_y` dword to zero, and
    /// neither `Unit::init` nor `Unit::set_new_location` writes it.  Main-thread retail
    /// samples confirm `(0,0)` for all observed squad and crew Guys: the formation is carried
    /// in `x/y` and `des_x/des_y`, **not** in those shorts.  This method therefore clears the
    /// offsets and reproduces the `set_angle(..., 1)` + `set_new_location(..., 1)` state for
    /// the live squad prefix.
    ///
    /// Crew are deliberately not synthesized here.  Retail propagates them recursively from
    /// Guy 0 using each crew Guy's graphics-derived `track_dx/track_dy` (`Guy +0x54/+0x58`),
    /// data which is not part of [`UnitTypeStats`].  Crew do not own collision footprints;
    /// callers with graphics state may materialize that separate recursive tail exactly.
    pub fn set_initial_locations(
        &mut self,
        anchor_x: i32,
        anchor_y: i32,
        angle: i32,
        formation: i8,
        unit_masks: u32,
        world_max_x: i32,
        world_max_y: i32,
        ut: &UnitTypeStats,
    ) {
        let live = (self.guy_mark as i32).clamp(0, ut.squad_size.max(0)) as usize;
        let locations = Self::initial_squad_locations(
            live,
            anchor_x,
            anchor_y,
            angle,
            formation,
            unit_masks,
            world_max_x,
            world_max_y,
            ut,
        );
        for (slot, (x, y)) in self.guys.iter_mut().take(live).zip(locations) {
            let Some(g) = slot else { continue };
            g.off_x = 0;
            g.off_y = 0;
            g.des_angle = angle;
            g.angle = angle;
            g.last_angle = angle;
            g.des_x = x;
            g.des_y = y;
            g.x = x;
            g.y = y;
            g.last_x = x;
            g.last_y = y;
            g.last_z = g.z;
        }
    }

    /// `PtrArray<Guy>::walk_data` `0x0046DF30`, checksum (non-loading) side.
    ///
    /// The visitor sees, in this order:
    /// 1. `length` (4 bytes)
    /// 2. *stop here if `length == 0`*
    /// 3. `size` (4)
    /// 4. `increment` (2, at `+0x0C`)
    /// 5. `flags & ~0x40` (1) — the `0x40` bit is cleared in the object first
    /// 6. `length` presence bytes, one per slot, `1` if the pointer is non-null
    /// 7. `size` and `increment` **again**, as one 6-byte range `[this+8, this+0xE)`
    /// 8. each non-null guy's `GuyData::walk_data`
    ///
    /// Step 7 is not a transcription slip: `call [edx]` with `(this+8, this+0xE)` at
    /// `0x0046E1AF` re-walks bytes already walked at steps 3 and 4. Retail hashes them
    /// twice, so we hash them twice.
    pub fn walk(&self, cs: &mut CheckSum) {
        let len = self.len() as i32;
        cs.walk(&len.to_le_bytes());
        if len == 0 {
            return;
        }
        cs.walk(&self.size.to_le_bytes());
        cs.walk(&self.increment.to_le_bytes());
        cs.walk(&[self.flags & !0x40]);
        for slot in &self.guys {
            cs.walk(&[u8::from(slot.is_some())]);
        }
        let mut six = [0u8; 6];
        six[0..4].copy_from_slice(&self.size.to_le_bytes());
        six[4..6].copy_from_slice(&self.increment.to_le_bytes());
        cs.walk(&six);
        for slot in &self.guys {
            if let Some(g) = slot {
                cs.walk(&g.walk_bytes());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// GroupData
// ---------------------------------------------------------------------------

/// The ten formations in `rules.xml` `<FORMATIONS>`, in file order — which is the index
/// order `GroupData::form` and `UnitData::form` use.
///
/// `Group::action_form` cycles with `% 5`, so the FORM hotkey only reaches the first five;
/// the rest are set by the AI or by scenario script.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Formation {
    /// 0
    Line = 0,
    /// 1
    Refused = 1,
    /// 2
    Envelop = 2,
    /// 3
    EchelonRight = 3,
    /// 4
    EchelonLeft = 4,
    /// 5
    Sparse = 5,
    /// 6
    Square = 6,
    /// 7
    Wedge = 7,
    /// 8
    Column = 8,
    /// 9
    Mob = 9,
}

/// The formations the FORM hotkey cycles: `(get_form_option() + 1) % 5`.
pub const FORM_CYCLE_LEN: i32 = 5;

/// `Group::action_form`'s sentinel arguments.
pub mod form_arg {
    /// Cycle forward: `form = (current + 1) % 5`.
    pub const CYCLE_NEXT: i32 = -1;
    /// Copy the group leader's `UnitData::form`.
    pub const FROM_LEADER: i32 = -2;
    /// Cycle backward: `form = current - 1`, wrapping to 4 below zero.
    pub const CYCLE_PREV: i32 = -3;
}

/// `Group::action_form` `0x00707220`'s form selection, before it is written to each unit.
///
/// ```text
/// if (form == -3) { form = get_form_option() - 1; if (form < 0) form = 4; }
/// if (form == -1) { form = (get_form_option() + 1) % 5; }
/// if (form == -2) { form = leader_unit->form; }
/// ```
///
/// Note the asymmetry, which is retail's: `-1` wraps modulo 5 while `-3` wraps by an
/// explicit `< 0` test to 4. They agree for the five cycled forms and diverge if the
/// current form is 5..9 — `-1` maps 7 to 3, `-3` maps 7 to 6.
pub fn resolve_form(arg: i32, current_form_option: i32, leader_form: i32) -> i32 {
    match arg {
        form_arg::CYCLE_PREV => {
            let f = current_form_option - 1;
            if f < 0 {
                4
            } else {
                f
            }
        }
        form_arg::CYCLE_NEXT => (current_form_option + 1) % FORM_CYCLE_LEN,
        form_arg::FROM_LEADER => leader_form,
        other => other,
    }
}

/// Static object/type facts consumed by `Form::categorize` and `Form::compute_dests`.
///
/// The retail functions discover these through the object and unit-type tables.  Keeping
/// them as an explicit value makes the formation math usable by the command bridge without
/// smuggling a world reference into this checksum/state module.  `category` is the already
/// resolved `FormData::type_cat(...)` result; a host which substitutes a transport's cargo
/// type or a water/modern-infantry upgrade must do so before constructing this value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormationMember {
    /// `FormCatIndex`, in `0..18`.
    pub category: i32,
    /// `ObjectTypeData::x_spacing` `+0x228` for the effective formation type.
    pub x_spacing: i32,
    /// `ObjectTypeData::y_spacing` `+0x22C` for the effective formation type.
    pub y_spacing: i32,
    /// `UnitTypeData::uber_size` `+0x308` for the effective formation type.
    pub formation_size: i32,
    /// `ObjectTypeData::guy_spacing` `+0x224` (used by subordinate members).
    pub guy_spacing: i32,
    /// Result of `UnitData::is_modern_infantry` `0x00607B40`.
    pub modern_infantry: bool,
    /// Signed `UnitData::form_mod` byte `+0xAB`; `-1` is the retail
    /// `get_form_mod_option` non-contributor sentinel. Hosts must also use `-1` for
    /// object classes that function filters before reading the byte.
    pub width: i32,
    /// `UnitData::angle` `+0x50` for `Group::compute_form`'s leader-facing test.
    pub angle: i32,
}

impl Default for FormationMember {
    fn default() -> Self {
        FormationMember {
            category: 0,
            x_spacing: FORM_CELL,
            y_spacing: FORM_CELL,
            formation_size: 1,
            guy_spacing: FORM_CELL,
            modern_infantry: false,
            width: 50,
            angle: 0,
        }
    }
}

/// The transient global `Form` result consumed by `Group::action_move_near`.
///
/// Retail owns ten process-global `Form` objects, clears the range `+0x30..+0xE90` for
/// each computation, and immediately consumes these four arrays while installing orders.
/// Returning a value gives Rust the same lifetime without introducing shared mutable state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormationLayout {
    pub leader_index: usize,
    pub to_x: [i32; GROUP_MAX_MEMBERS],
    pub to_y: [i32; GROUP_MAX_MEMBERS],
    pub off_x: [i32; GROUP_MAX_MEMBERS],
    pub off_y: [i32; GROUP_MAX_MEMBERS],
    pub reverse: bool,
}

impl Default for FormationLayout {
    fn default() -> Self {
        FormationLayout {
            leader_index: 0,
            to_x: [0; GROUP_MAX_MEMBERS],
            to_y: [0; GROUP_MAX_MEMBERS],
            off_x: [0; GROUP_MAX_MEMBERS],
            off_y: [0; GROUP_MAX_MEMBERS],
            reverse: false,
        }
    }
}

#[inline]
fn div_3_table(v: i32) -> i32 {
    // `div_3_table` is initialized at 0x00681DB0. Positive entries are `n / 3`;
    // negative entries are `(n - 2) / 3`, i.e. mathematical floor division.
    let q = v / 3;
    let r = v % 3;
    if r != 0 && v < 0 {
        q - 1
    } else {
        q
    }
}

/// Convert a high-resolution `Form::to_x/to_y` coordinate to the coordinate passed to
/// `Unit::add_move_facing_order`: `div_3_table[coord >> 4]`.
#[inline]
pub fn formation_order_coord(coord: i32) -> i32 {
    div_3_table(coord >> 4)
}

#[inline]
fn opposite_half_turn(delta: i32) -> bool {
    matches!(delta as u32, 0x4000_0000..=0xC000_0000)
}

#[derive(Clone)]
struct FormScratch {
    form: i32,
    idx: usize,
    num_category: [i32; 18],
    x_spacing: [i32; 18],
    y_spacing: [i32; 18],
    cat_id: [i32; GROUP_MAX_MEMBERS],
    category: [i32; GROUP_MAX_MEMBERS],
    layout: FormationLayout,
}

impl FormScratch {
    fn new(form: i32) -> Self {
        FormScratch {
            form,
            idx: 0,
            num_category: [0; 18],
            x_spacing: [0; 18],
            y_spacing: [0; 18],
            cat_id: [0; GROUP_MAX_MEMBERS],
            category: [0; GROUP_MAX_MEMBERS],
            layout: FormationLayout::default(),
        }
    }

    /// `Form::categorize` `0x0072E250`, for the captain-only path.
    fn categorize(&mut self, members: &[FormationMember]) -> Option<()> {
        let mut largest_category = 8usize;
        let mut largest_count = 0;
        let mut leader: Option<(usize, usize)> = None;

        for (i, member) in members.iter().enumerate() {
            let category = usize::try_from(member.category).ok()?;
            if category >= 18 || category == 8 {
                continue;
            }
            self.category[i] = category as i32;
            self.cat_id[i] = self.num_category[category];
            self.num_category[category] = self.num_category[category].wrapping_add(1);
            self.accumulate_spacing(category, *member, true)?;
            if largest_count < self.num_category[category] {
                largest_category = category;
                largest_count = self.num_category[category];
            }
            if leader.is_none_or(|(_, best)| category < best) {
                leader = Some((i, category));
            }
        }

        // FormCatOther (8) is deliberately assigned in a second pass. For non-Square
        // forms retail picks the first populated category at/after largest+1, or that
        // category itself when the suffix is empty.
        for (i, member) in members.iter().enumerate() {
            if member.category != 8 {
                continue;
            }
            let mut category = 8usize;
            if self.form != Formation::Square as i32 && largest_category + 1 < 18 {
                category = largest_category + 1;
                if let Some(found) = (category..18).find(|&cat| self.num_category[cat] != 0) {
                    category = found;
                }
            }
            self.category[i] = category as i32;
            self.cat_id[i] = self.num_category[category];
            self.num_category[category] = self.num_category[category].wrapping_add(1);
            self.accumulate_spacing(category, *member, false)?;
            if largest_count <= self.num_category[category] {
                largest_count = self.num_category[category];
            }
            if leader.is_none_or(|(_, best)| category < best) {
                leader = Some((i, category));
            }
        }

        self.idx = leader.map_or(0, |(i, _)| i);
        self.layout.leader_index = self.idx;
        Some(())
    }

    fn accumulate_spacing(
        &mut self,
        category: usize,
        member: FormationMember,
        add_modern_pad: bool,
    ) -> Option<()> {
        if member.formation_size <= 0 || member.x_spacing <= 0 || member.y_spacing <= 0 {
            return None;
        }
        if member.formation_size == 1 {
            self.x_spacing[category] = self.x_spacing[category].max(member.x_spacing);
            self.y_spacing[category] = self.y_spacing[category].max(member.y_spacing);
            return Some(());
        }
        let columns = if self.form == Formation::Column as i32 {
            2
        } else {
            member.formation_size.min(3)
        };
        self.x_spacing[category] =
            self.x_spacing[category].max(columns.wrapping_mul(member.x_spacing));
        let rows = (member.formation_size - 1) / columns + 1;
        let mut y = rows.wrapping_mul(member.y_spacing);
        if add_modern_pad && member.modern_infantry {
            y = y.wrapping_add(FORM_CELL);
        }
        self.y_spacing[category] = self.y_spacing[category].max(y);
        Some(())
    }

    /// Non-Wedge `Form::compute_rows_and_columns` `0x0072D910`.
    fn rows_and_columns(&self, width: i32) -> Option<([i32; 18], [i32; 18])> {
        let mut per = [0i32; 18];
        let mut rows = [0i32; 18];
        let mut widest = 0i32;
        for category in 0..18 {
            let product = self.num_category[category].wrapping_mul(self.x_spacing[category]);
            // The compare at 0x0072DCD8 is against the category *count*, not its index.
            let candidate = if self.num_category[category] < 6 {
                product
            } else {
                product / 2
            };
            widest = widest.max(candidate);
        }
        for category in 0..18 {
            let count = self.num_category[category];
            if self.form == Formation::Column as i32 {
                rows[category] = (count + 2) / 3;
                continue;
            }
            if count == 0 {
                continue;
            }
            let spacing = self.x_spacing[category];
            if spacing <= 0 {
                return None;
            }
            let requested = widest.wrapping_mul(width) / 50 / spacing;
            per[category] = count.min(requested).max(1);
            rows[category] = (count - 1 + per[category]) / per[category];
        }
        Some((per, rows))
    }

    /// Captain branch of `Form::compute_dests` `0x0072CBA0` for forms 0..5 and Column.
    fn compute_captain_dests(
        &mut self,
        group: &mut GroupData,
        x: i32,
        y: i32,
        angle: i32,
        facing: bool,
        rows: &[i32; 18],
    ) -> Option<()> {
        let n = group.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
        group.form_num = group.num;
        let mut anchor_lateral = 0i32;
        let mut anchor_forward = 0i32;

        for i in 0..n {
            let category = usize::try_from(self.category[i]).ok()?;
            if category >= 18 {
                return None;
            }
            let slot = self.cat_id[i];
            let row_count = rows[category];
            if row_count <= 0 {
                return None;
            }
            let x_spacing = self.x_spacing[category];
            let y_spacing = self.y_spacing[category];
            let (placed_lateral, mut forward, base_forward, write_angle) = if self.form
                == Formation::Column as i32
            {
                let lateral = ((slot + 1) % 3 - 1).wrapping_mul(x_spacing);
                let forward = (slot / 3).wrapping_mul(y_spacing).wrapping_neg();
                (
                    if facing {
                        lateral.wrapping_neg()
                    } else {
                        lateral
                    },
                    forward,
                    forward,
                    false,
                )
            } else {
                let one_based = slot % row_count + 1;
                let half = one_based >> 1;
                let mut lateral = half.wrapping_mul(x_spacing).wrapping_neg();
                if slot % row_count & 1 == 0 {
                    lateral = half.wrapping_mul(x_spacing);
                }
                if row_count & 1 == 0 {
                    lateral = lateral.wrapping_add(x_spacing / 2);
                }
                if (category < 3 || category == 6 || category >= 12) && (slot / row_count & 1) != 0
                {
                    lateral = lateral.wrapping_add(x_spacing / 2);
                }

                // Categorized captains force local density to 2 at
                // 0x0072CCC2..CCCF, so `(density != 0) - 2` is -1 here.
                let base_forward = (slot / row_count).wrapping_mul(y_spacing).wrapping_neg();
                let forward = match self.form {
                    x if x == Formation::Refused as i32 => {
                        base_forward.wrapping_sub(lateral.wrapping_abs())
                    }
                    x if x == Formation::Envelop as i32 => {
                        base_forward.wrapping_add(lateral.wrapping_abs())
                    }
                    x if x == Formation::EchelonLeft as i32 => {
                        if facing {
                            base_forward.wrapping_sub(lateral)
                        } else {
                            base_forward.wrapping_add(lateral)
                        }
                    }
                    x if x == Formation::EchelonRight as i32 => {
                        if facing {
                            base_forward.wrapping_add(lateral)
                        } else {
                            base_forward.wrapping_sub(lateral)
                        }
                    }
                    _ => base_forward,
                };
                (
                    if facing {
                        lateral.wrapping_neg()
                    } else {
                        lateral
                    },
                    forward,
                    base_forward,
                    true,
                )
            };

            // Stack populated categories front-to-back. The `float` conversion and
            // truncation are instruction-visible (`cvtdq2ps` / `cvttss2si`).
            let mut first_nonempty = None;
            for previous in 0..=category {
                if self.num_category[previous] == 0 {
                    continue;
                }
                if first_nonempty.is_some() {
                    forward = forward.wrapping_sub(self.y_spacing[previous] / 2);
                } else {
                    first_nonempty = Some(previous);
                }
                if previous < category {
                    let shift =
                        (((rows[previous] as f32) - 0.5) * self.y_spacing[previous] as f32) as i32;
                    forward = forward.wrapping_sub(shift);
                }
            }

            self.layout.off_x[i] = placed_lateral;
            self.layout.off_y[i] = forward;
            self.layout.to_x[i] = x
                .wrapping_add(cosx(angle, placed_lateral))
                .wrapping_add(sinx(angle, forward));
            self.layout.to_y[i] = y
                .wrapping_add(sinx(angle, placed_lateral))
                .wrapping_sub(cosx(angle, forward));
            if write_angle {
                group.angles[i] = match (placed_lateral.signum(), (forward - base_forward).signum())
                {
                    (-1, -1) | (1, 1) => -0x20,
                    (-1, 1) | (1, -1) => 0x20,
                    _ if placed_lateral == 0 && self.form == Formation::EchelonLeft as i32 => -0x20,
                    _ if placed_lateral == 0 && self.form == Formation::EchelonRight as i32 => 0x20,
                    _ => 0,
                };
            }
            if first_nonempty == Some(category) && slot == 0 {
                anchor_lateral = placed_lateral;
                anchor_forward = forward;
            }
        }

        let correction_x = cosx(angle, anchor_lateral).wrapping_add(sinx(angle, anchor_forward));
        let correction_y = sinx(angle, anchor_lateral).wrapping_sub(cosx(angle, anchor_forward));
        for i in 0..n {
            self.layout.to_x[i] = self.layout.to_x[i].wrapping_sub(correction_x);
            self.layout.to_y[i] = self.layout.to_y[i].wrapping_sub(correction_y);
            self.layout.off_x[i] = self.layout.off_x[i].wrapping_sub(anchor_lateral);
            self.layout.off_y[i] = self.layout.off_y[i].wrapping_sub(anchor_forward);
            group.off_x[i] = div_3_table(self.layout.off_x[i] >> 4);
            group.off_y[i] = div_3_table(self.layout.off_y[i] >> 4);
        }
        Some(())
    }
}

/// One selection / control group. Layout matches `GroupData`, `sizeof` 2508.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupData {
    /// `+0x04`. Negative means "not a persistent group" — several prunes key off `id >= 0`.
    pub id: i32,
    /// `+0x08`
    pub army: i32,
    /// `+0x0C` live member count; **the length prefix for every array below**.
    pub num: i32,
    /// `+0x10` formation index
    pub form: i32,
    /// `+0x14` `Game::frame` at the last `add`
    pub stamp: i32,
    /// `+0x18`
    pub ox: i32,
    /// `+0x1C`
    pub oy: i32,
    /// `+0x20`
    pub o_dist: i32,
    /// `+0x24`
    pub o_angle: i32,
    /// `+0x28`
    pub disband: i32,
    /// `+0x2C`
    pub order_num: i32,
    /// `+0x30`
    pub priority: i32,
    /// `+0x34` OR of every member type's `UnitTypeData::role`
    pub role: i32,
    /// `+0x38`
    pub think_frame: i32,
    /// `+0x3C`
    pub new_speed: i32,
    /// `+0x40`
    pub speed: i32,
    /// `+0x44` number of *formation slots* filled — the bound `update_positions` uses
    pub form_num: i32,
    /// `+0x48`
    pub facing: u8,
    /// `+0x49` set from the first member's `is_building`
    pub buildings: u8,
    /// `+0x4A` owner slot
    pub who: u8,
    /// `+0x4B`
    pub march: u8,
    /// `+0x04C` lateral formation offset, in `FORM_CELL` units
    pub off_x: [i32; GROUP_MAX_MEMBERS],
    /// `+0x24C` forward formation offset, in `FORM_CELL` units
    pub off_y: [i32; GROUP_MAX_MEMBERS],
    /// `+0x44C` rotated world-space offset
    pub curr_x: [i32; GROUP_MAX_MEMBERS],
    /// `+0x64C` rotated world-space offset
    pub curr_y: [i32; GROUP_MAX_MEMBERS],
    /// `+0x84C`
    pub angles: [i8; GROUP_MAX_MEMBERS],
    /// `+0x8CC` member object indices into `objects.lists[who]`; negative = tombstone
    pub list: [i16; GROUP_MAX_MEMBERS],
}

impl Default for GroupData {
    fn default() -> Self {
        GroupData {
            id: -1,
            army: 0,
            num: 0,
            form: 0,
            stamp: 0,
            ox: 0,
            oy: 0,
            o_dist: 0,
            o_angle: 0,
            disband: 0,
            order_num: 0,
            priority: 0,
            role: 0,
            think_frame: 0,
            new_speed: 0,
            speed: 0,
            form_num: 0,
            facing: 0,
            buildings: 0,
            who: 0,
            march: 0,
            off_x: [0; GROUP_MAX_MEMBERS],
            off_y: [0; GROUP_MAX_MEMBERS],
            curr_x: [0; GROUP_MAX_MEMBERS],
            curr_y: [0; GROUP_MAX_MEMBERS],
            angles: [0; GROUP_MAX_MEMBERS],
            list: [0; GROUP_MAX_MEMBERS],
        }
    }
}

impl GroupData {
    /// `Group::walk_data` `0x00708400`, exactly.
    ///
    /// ```text
    /// walk(this+0x04, this+0x4C)                     ; 72-byte scalar header
    /// if (num != 0) {
    ///     walk(this+0x8CC, this + (num+0x466)*2)     ; list[0..num]    (i16)
    ///     walk(this+0x04C, this + (num+0x013)*4)     ; off_x[0..num]
    ///     walk(this+0x24C, this + (num+0x093)*4)     ; off_y[0..num]
    ///     walk(this+0x44C, this + (num+0x113)*4)     ; curr_x[0..num]
    ///     walk(this+0x64C, this + (num+0x193)*4)     ; curr_y[0..num]
    ///     walk(this+0x84C, this + 0x84C + num)       ; angles[0..num] (i8)
    /// }
    /// ```
    ///
    /// Two properties fall out and both matter for a checksum port: the walk is
    /// **length-prefixed by `num`**, so stale entries past the live count are invisible and
    /// need not be cleared; and **`list` is hashed before the offsets**, which is not the
    /// declaration order.
    pub fn walk(&self, cs: &mut CheckSum) {
        cs.walk(&self.header_bytes());
        let n = self.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
        if self.num == 0 {
            return;
        }
        let mut buf = Vec::with_capacity(n * 4);
        for v in &self.list[..n] {
            buf.extend_from_slice(&v.to_le_bytes());
        }
        cs.walk(&buf);
        for arr in [&self.off_x, &self.off_y, &self.curr_x, &self.curr_y] {
            buf.clear();
            for v in &arr[..n] {
                buf.extend_from_slice(&v.to_le_bytes());
            }
            cs.walk(&buf);
        }
        buf.clear();
        for v in &self.angles[..n] {
            buf.push(*v as u8);
        }
        cs.walk(&buf);
    }

    /// The 72-byte scalar header, `+0x04 .. +0x4C`.
    pub fn header_bytes(&self) -> [u8; 72] {
        let mut out = [0u8; 72];
        let ints = [
            self.id,
            self.army,
            self.num,
            self.form,
            self.stamp,
            self.ox,
            self.oy,
            self.o_dist,
            self.o_angle,
            self.disband,
            self.order_num,
            self.priority,
            self.role,
            self.think_frame,
            self.new_speed,
            self.speed,
            self.form_num,
        ];
        for (i, v) in ints.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        out[68] = self.facing;
        out[69] = self.buildings;
        out[70] = self.who;
        out[71] = self.march;
        out
    }

    /// `Group::add` `0x00714350` — append one member.
    ///
    /// The parts that are state, rather than the validity vtable calls:
    ///
    /// ```text
    /// if (num != 0 && who != group.who) return;      ; one owner per group
    /// if (member(o, who, 1)) return;                 ; no duplicates
    /// if (num >= 0x80) return;                       ; hard cap
    /// if (num == 0) group.who = who;
    /// off_x[num] = off_y[num] = curr_x[num] = curr_y[num] = 0; angles[num] = 0;
    /// list[num] = (short)o;
    /// num++;
    /// buildings = unit->is_building();
    /// stamp = Game::frame;
    /// role |= unittype->role;
    /// ...then recursively add the unit at unit->down (+0x90) if it is alive...
    /// if (id >= 0) compute_speed();
    /// ```
    ///
    /// Returns whether the member was appended.
    pub fn add(&mut self, o: i16, who: u8, is_building: bool, ty_role: i32, frame: i32) -> bool {
        if self.num != 0 && who != self.who {
            return false;
        }
        if self.member(o) {
            return false;
        }
        if self.num >= GROUP_MAX_MEMBERS as i32 {
            return false;
        }
        let n = self.num as usize;
        if self.num == 0 {
            self.who = who;
        }
        self.off_x[n] = 0;
        self.off_y[n] = 0;
        self.curr_x[n] = 0;
        self.curr_y[n] = 0;
        self.angles[n] = 0;
        self.list[n] = o;
        self.num += 1;
        self.buildings = u8::from(is_building);
        self.stamp = frame;
        self.role |= ty_role;
        true
    }

    /// `GroupData::member` `0x0070F8F0` — is this object index already a member?
    pub fn member(&self, o: i16) -> bool {
        self.list[..self.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize].contains(&o)
    }

    /// `Group::normalize` `0x00711540` — drop members that no longer qualify, compacting
    /// every parallel array.
    ///
    /// Retail scans **backwards** from `num-1` and, on a drop, shifts each of the six
    /// arrays down by one from that index. The predicate is:
    ///
    /// ```text
    /// drop if list[i] < 0
    /// drop if !(object->flags & 1)                       ; dead
    /// drop if priority == 0 && id >= 0
    ///         && (!object->is_unit() || object->group != group.id)
    /// drop if object->vt[+0x20]()                        ; "leaves groups" predicate
    /// ```
    ///
    /// `keep` supplies the three object-side answers as one closure so this file needs no
    /// world. It is called with the member's object index.
    pub fn normalize(&mut self, keep: &dyn Fn(i16) -> MemberState) {
        let mut i = self.num - 1;
        while i >= 0 {
            let idx = i as usize;
            let drop = if self.list[idx] < 0 {
                true
            } else {
                match keep(self.list[idx]) {
                    MemberState::Dead => true,
                    MemberState::LeavesGroups => true,
                    MemberState::NotOurUnit => self.priority == 0 && self.id >= 0,
                    MemberState::Keep => false,
                }
            };
            if drop {
                self.remove_at(idx);
            }
            i -= 1;
        }
    }

    /// `Group::get_num` `0x00714700` — prune dead members and return the count.
    ///
    /// Retail takes a shortcut worth reproducing: a group with fewer than four members and
    /// `id >= 0` runs the **full** `normalize` instead of the cheap alive-only scan, so a
    /// small named group is pruned more aggressively than a large one.
    pub fn get_num(&mut self, keep: &dyn Fn(i16) -> MemberState) -> i32 {
        if self.num < 1 {
            self.num = 0;
            self.ox = 0;
            self.oy = 0;
            return 0;
        }
        if self.num < 4 && self.id >= 0 {
            self.normalize(keep);
            return self.num;
        }
        let mut i = self.num - 1;
        while i >= 0 {
            let idx = i as usize;
            if keep(self.list[idx]) == MemberState::Dead {
                self.remove_at(idx);
            }
            i -= 1;
        }
        self.num
    }

    fn remove_at(&mut self, idx: usize) {
        let last = self.num as usize;
        for j in idx..last.saturating_sub(1) {
            self.list[j] = self.list[j + 1];
            self.angles[j] = self.angles[j + 1];
            self.off_x[j] = self.off_x[j + 1];
            self.off_y[j] = self.off_y[j + 1];
            self.curr_x[j] = self.curr_x[j + 1];
            self.curr_y[j] = self.curr_y[j + 1];
        }
        self.num -= 1;
    }

    /// `Group::compute_form` `0x00707C80` plus the captain branch of
    /// `Form::categorize` / `Form::compute`.
    ///
    /// This entry point intentionally accepts only the formation shapes for which every
    /// intermediate is recovered without an implicit retail-global dependency: Line,
    /// Refused, Envelop, both Echelons, Sparse, and Column. Those include all five shapes
    /// reachable from the FORM hotkey. Wedge reads an uninitialized stack cell in the
    /// shipped function, Square uses `FormData::space[4][18]`, and Mob carries an evolving
    /// binary-angle ring; they return `None` until those paths have oracle evidence.
    ///
    /// `members` must be the live, on-map **captain** prefix in exact group-list order.
    /// Returning `None` is transactional: no byte of `self` is changed.
    #[allow(clippy::too_many_arguments)]
    pub fn compute_form(
        &mut self,
        members: &[FormationMember],
        x: i32,
        y: i32,
        form: i32,
        width: i32,
        set_angle: bool,
        requested_angle: i32,
        old_x: i32,
        old_y: i32,
        force_facing_zero: bool,
    ) -> Option<(FormationLayout, i32)> {
        let n = self.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
        if n == 0 || members.len() != n {
            return None;
        }
        if !matches!(form, 0..=5 | 8) {
            return None;
        }

        let mut scratch = FormScratch::new(form);
        scratch.categorize(members)?;
        let (_, rows) = scratch.rows_and_columns(width)?;
        let leader_index = scratch.idx;
        let leader_angle = members
            .get(leader_index)?
            .angle
            .wrapping_sub((self.angles[leader_index] as i32).wrapping_mul(0x0100_0000));
        let dx = x.wrapping_sub(old_x);
        let dy = y.wrapping_sub(old_y);
        let mut angle = requested_angle;
        let mut reverse_move = false;
        if !set_angle {
            angle = if dx == 0 && dy == 0 {
                if old_x != self.ox || old_y != self.oy {
                    leader_angle
                } else {
                    self.o_angle
                }
            } else {
                find_angle(dx, dy)
            };
        } else {
            let found = find_angle(dx, dy);
            reverse_move = opposite_half_turn(found.wrapping_sub(angle));
        }

        let mut work = self.clone();
        work.form = form;
        if opposite_half_turn(leader_angle.wrapping_sub(angle)) {
            work.facing ^= 1;
        }
        if force_facing_zero {
            work.facing = 0;
            reverse_move = false;
        }
        let facing = work.facing != 0;
        scratch.compute_captain_dests(&mut work, x, y, angle, facing, &rows)?;
        if opposite_half_turn(leader_angle.wrapping_sub(angle)) {
            work.facing ^= 1;
        }

        let leader_x = scratch.layout.to_x[leader_index];
        let leader_y = scratch.layout.to_y[leader_index];
        work.o_angle = find_angle(x.wrapping_sub(leader_x), y.wrapping_sub(leader_y));
        work.o_dist = vector_dist(x.wrapping_sub(leader_x), y.wrapping_sub(leader_y));

        if reverse_move {
            scratch.layout.reverse = true;
            for i in 0..n {
                scratch.layout.off_x[i] = scratch.layout.off_x[i].wrapping_neg();
                scratch.layout.off_y[i] = scratch.layout.off_y[i].wrapping_neg();
                work.off_x[i] = work.off_x[i].wrapping_neg();
                work.off_y[i] = work.off_y[i].wrapping_neg();
            }
        }

        *self = work;
        Some((scratch.layout, angle))
    }

    /// `Group::update_positions` `0x00713810` — rotate the formation offsets into world
    /// space by the group's facing.
    ///
    /// ```text
    /// angle = leader_unit->angle;
    /// if the leader has a targeted order:
    ///     angle = find_angle(order.target_x - unit.x, order.target_y - unit.y);
    /// for (i = 0; i < form_num; i++) {
    ///     curr_x[i] = cosx(angle, off_x[i]*48) + sinx(angle, off_y[i]*48);
    ///     curr_y[i] = sinx(angle, off_x[i]*48) - cosx(angle, off_y[i]*48);
    /// }
    /// ```
    ///
    /// That is the engine's own basis: with angle 0 pointing north, "right" is
    /// `(cos, sin)` and "forward" is `(sin, -cos)`, so `off_x` is a lateral offset and
    /// `off_y` a forward one. **The loop bound is `form_num`, not `num`** — the formation
    /// can be wider than the live membership.
    ///
    /// One incidental discovery, recorded because it will bite every lane: the object's
    /// `Coord` fields are **XOR-obfuscated with `0x00063637`** in memory. `update_positions`
    /// reads `unit->x` as `[ecx+0x10] ^ 0x63637` and `unit->y` as `[ecx+0x14] ^ 0x63637`
    /// (`0x007138F6`, `0x00713903`). `GuyData`'s coords are *not* obfuscated.
    pub fn update_positions(&mut self, angle: i32) {
        let n = self.form_num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
        for i in 0..n {
            let rx = self.off_x[i].wrapping_mul(FORM_CELL);
            let ry = self.off_y[i].wrapping_mul(FORM_CELL);
            self.curr_x[i] = cosx(angle, rx).wrapping_add(sinx(angle, ry));
            self.curr_y[i] = sinx(angle, rx).wrapping_sub(cosx(angle, ry));
        }
    }

    /// The XOR mask retail stores object `Coord`s under. See [`Self::update_positions`].
    pub const COORD_XOR: i32 = 0x0006_3637;

    /// `Group::compute_speed` `0x00707F80` — `speed = new_speed = leader's speed`, or zero.
    ///
    /// ```text
    /// if (buildings == 0 && num > 0 && find_leader() >= 0) speed = UnitData::speed(leader);
    /// else speed = 0;
    /// new_speed = speed;
    /// ```
    pub fn compute_speed(&mut self, leader_speed: Option<i32>) {
        self.speed = match leader_speed {
            Some(s) if self.buildings == 0 && self.num > 0 => s,
            _ => 0,
        };
        self.new_speed = self.speed;
    }
}

/// The object-side verdict `Group::normalize` needs for one member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberState {
    /// Alive, a unit, and still claiming this group.
    Keep,
    /// `!(object->flags & 1)`.
    Dead,
    /// Not a unit, or `object->group != group.id`. Only drops when
    /// `priority == 0 && id >= 0`.
    NotOurUnit,
    /// `object->vt[+0x20]()` is true.
    LeavesGroups,
}

// ---------------------------------------------------------------------------
// Groups — the global container and its per-frame pass
// ---------------------------------------------------------------------------

/// `Groups`, the singleton at `0x00E85F10` (`GroupsData` + `GroupsOut`, `sizeof` 76).
#[derive(Clone, Debug)]
pub struct Groups {
    /// `GroupsData::list : Array<Group>` `+0x00`. 64 slots per player, player-major.
    pub list: Vec<GroupData>,
    /// `GroupsData::last_group : int[8]` `+0x1C` — hashed as a raw 32-byte block by
    /// `check_groups`, reached through the `const_last_group` pointer at `+0x3C`.
    pub last_group: [i32; NUM_LEADERS],
    /// `GroupsData::proc_group : int` `+0x40` — the round-robin cursor `Groups::process`
    /// advances. Walked by `Groups::walk_data` but **not** by `check_groups`.
    pub proc_group: i32,
}

impl Default for Groups {
    fn default() -> Self {
        Groups {
            list: vec![GroupData::default(); NUM_GROUPS],
            last_group: [0; NUM_LEADERS],
            proc_group: 0,
        }
    }
}

impl Groups {
    /// Index of player `who`'s group `g`.
    ///
    /// `Groups::process` computes `(proc_group + who*0x40) * sizeof(Group) + list`, so the
    /// array is player-major with a 64-group stride.
    #[inline]
    pub fn index(who: usize, g: usize) -> usize {
        who * GROUPS_PER_PLAYER + g
    }

    /// Borrow one player's group.
    pub fn get(&self, who: usize, g: usize) -> &GroupData {
        &self.list[Self::index(who, g)]
    }

    /// Borrow one player's group mutably.
    pub fn get_mut(&mut self, who: usize, g: usize) -> &mut GroupData {
        &mut self.list[Self::index(who, g)]
    }

    /// `Groups::process` `0x006FA210` — called last inside `GameDaemon::process_all`.
    ///
    /// It does **one group per active player per frame**, the same slot index for all of
    /// them, then advances the cursor:
    ///
    /// ```text
    /// for who in 0..8:
    ///     if (leaders[who].active & 1)
    ///         g = groups.list[proc_group + who*64];
    ///         <Group::normalize inlined verbatim>
    ///         g.find_role();
    ///         g.compute_speed();
    /// proc_group++; if (proc_group > 0x3F) proc_group = 0;
    /// ```
    ///
    /// So a group is re-validated once every 64 frames — about 4.3 game seconds — unless
    /// something touches it sooner. That latency is real and observable: a group's `num`
    /// can name dead units for up to 64 frames.
    ///
    /// The body is byte-identical to `Group::normalize` `0x00711540` followed by
    /// `Group::find_role` `0x007081F0` and the `compute_speed` tail; MSVC inlined all
    /// three. Reproduced here as the calls.
    pub fn process(
        &mut self,
        leader_active: &[bool; NUM_LEADERS],
        keep: &dyn Fn(usize, i16) -> MemberState,
        leader_speed: &dyn Fn(usize, &GroupData) -> Option<i32>,
    ) {
        let slot = self.proc_group.clamp(0, GROUPS_PER_PLAYER as i32 - 1) as usize;
        for who in 0..NUM_LEADERS {
            if !leader_active[who] {
                continue;
            }
            let idx = Self::index(who, slot);
            let keep_here = |o: i16| keep(who, o);
            self.list[idx].normalize(&keep_here);
            let sp = leader_speed(who, &self.list[idx]);
            self.list[idx].compute_speed(sp);
        }
        self.proc_group += 1;
        if self.proc_group > (GROUPS_PER_PLAYER as i32 - 1) {
            self.proc_group = 0;
        }
    }

    /// `CheckSums::check_groups` `0x00937530` — channel 6.
    ///
    /// ```text
    /// for (i = 0; i < groups.list.length; i++)  Group::walk_data(cs)   ; stride 0x9D4
    /// v = cs->value;
    /// for (off = 0; off < 0x20; off += 4)                              ; last_group[8]
    ///     v = adler32(v, (char*)*const_last_group + off, 4);
    /// cs->value = v;
    /// ```
    ///
    /// Note the second loop is **eight separate four-byte `adler32` calls**, not one call
    /// over 32 bytes. Adler-32 is a rolling sum, so chunking is transparent — but only
    /// because `s1`/`s2` are threaded through the return value, which they are here.
    ///
    /// Note also what is *absent*: `proc_group` is walked by `Groups::walk_data`
    /// (the save path) and is **not** in the checksum. Two lanes could disagree about the
    /// cursor without desyncing.
    pub fn check_groups(&self, cs: &mut CheckSum) {
        for g in &self.list {
            g.walk(cs);
        }
        for v in &self.last_group {
            cs.walk(&v.to_le_bytes());
        }
    }
}

// ---------------------------------------------------------------------------
// The guys channel driver
// ---------------------------------------------------------------------------

/// One owner slot's unit band, as `check_guys` traverses it.
#[derive(Clone, Debug, Default)]
pub struct OwnerUnits {
    /// `objects.lists[who].list[i]` for `i` in `[obj_base, unit_mark[who])`, in index
    /// order. `None` marks a slot the array holds but which is not a live object.
    pub units: Vec<Option<UnitSlot>>,
}

/// One unit, reduced to what `check_guys` needs.
#[derive(Clone, Debug, Default)]
pub struct UnitSlot {
    /// `ObjectData` flags byte at `+0x08`; bit 0 is "active".
    pub flags: u8,
    /// `UnitData::guys` at `+0xE4`.
    pub guys: UnitGuys,
}

/// `CheckSums::check_guys` `0x00937430` — channel 7.
///
/// ```text
/// if (objects.valid == 0) return;                              ; +0x1F4
/// for (who = 0; who < 8; who++) {                              ; leaders, stride 0x6EEC
///     if (!(leaders[who].flags & 1)) continue;
///     for (i = *obj_base; i < objects.obj_mark[0][who]; i++) {  ; unit band only
///         obj = objects.lists[who].list[i];
///         if (obj->flags & 1)
///             PtrArray<Guy>::walk_data(cs, obj)  on  obj + 0xE4;
///     }
/// }
/// ```
///
/// Three things to carry into a port. The band is **units only** — `obj_mark[0]` is
/// `unit_mark`, so buildings (band 2000) and walls (band 3000) contribute no guys, which
/// is right, since only `Unit` has a `guys` array. The traversal is **fixed player order**,
/// unlike `Objects::process_all`'s `(frame + i) % 10` rotation — the checksum is taken
/// outside the tick and does not inherit the rotation. And the leader-active test is on
/// the leader, not the unit, so an inactive player's units are skipped even if they still
/// exist.
pub fn check_guys(
    cs: &mut CheckSum,
    objects_valid: bool,
    leader_active: &[bool; NUM_LEADERS],
    owners: &[OwnerUnits; NUM_LEADERS],
) {
    if !objects_valid {
        return;
    }
    for who in 0..NUM_LEADERS {
        if !leader_active[who] {
            continue;
        }
        for slot in &owners[who].units {
            if let Some(u) = slot {
                if u.flags & 1 != 0 {
                    u.guys.walk(cs);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn ut() -> UnitTypeStats {
        UnitTypeStats {
            domain: 0,
            guy_spacing: 12,
            x_spacing: 12,
            y_spacing: 12,
            guy_radius: 1,
            new_block_radius: 1,
            turn_speed: 45,
            role: 0,
            squad_size: 4,
            uber_size: 1,
            crew_size: 0,
            base_form: 0,
        }
    }

    // --- trig folds ---------------------------------------------------------

    #[test]
    fn sinx_cosx_agree_with_f64_within_the_tables_own_error() {
        // The table is built with a /255 divisor over a 256-entry quarter, so it carries a
        // ~0.4% angular stretch. Anything inside 0.4% of the magnitude is the engine's own
        // approximation, not a porting slip.
        for deg in 0..360 {
            let a = ((deg as f64 / 360.0) * 4_294_967_296.0) as u32 as i32;
            let want_s = 1000.0 * (deg as f64).to_radians().sin();
            let want_c = 1000.0 * (deg as f64).to_radians().cos();
            let got_s = sinx(a, 1000) as f64;
            let got_c = cosx(a, 1000) as f64;
            assert!(
                (got_s - want_s).abs() < 6.0,
                "sinx {deg} got {got_s} want {want_s}"
            );
            assert!(
                (got_c - want_c).abs() < 6.0,
                "cosx {deg} got {got_c} want {want_c}"
            );
        }
    }

    #[test]
    fn cardinal_directions_are_exact() {
        // These are the four values the overflow-at-index-255 wrap has to land on.
        assert_eq!(cosx(0, 1000), 1000);
        assert_eq!(sinx(0, 1000), 0);
        assert_eq!(sinx(0x4000_0000, 1000), 1000);
        assert_eq!(cosx(0x4000_0000, 1000), 0);
        assert_eq!(cosx(-0x8000_0000, 1000), -1000);
        assert_eq!(sinx(-0x4000_0000, 1000), -1000);
    }

    #[test]
    fn zero_magnitude_short_circuits() {
        assert_eq!(sinx(0x1234_5678, 0), 0);
        assert_eq!(cosx(0x1234_5678, 0), 0);
    }

    #[test]
    fn angle_diff_uses_not_not_neg() {
        // The reflected branch is `not`, so it is one short of a true magnitude.
        assert_eq!(angle_diff(0, 1), !0u32.wrapping_sub(1) + 0);
        assert_eq!(angle_diff(10, 4), 6);
        assert_eq!(angle_diff(4, 10), !(4u32.wrapping_sub(10)));
        // ...which is 5, not 6.
        assert_eq!(angle_diff(4, 10), 5);
    }

    #[test]
    fn vector_dist_matches_the_documented_form() {
        assert_eq!(vector_dist(0, 0), 0);
        assert_eq!(vector_dist(100, 0), 100);
        assert_eq!(vector_dist(0, -100), 100);
        // max + min^2/(2*max)
        assert_eq!(vector_dist(300, 400), 400 + 300 * 300 / 800);
        assert_eq!(vector_dist(-400, 300), 400 + 300 * 300 / 800);
        // exact on the diagonal: 100 + 100*100/200 = 150 (true 141)
        assert_eq!(vector_dist(100, 100), 150);
        // large-leg branch
        assert_eq!(vector_dist(70_000, 60_000), (60_000 + 2 * 70_000) / 2);
    }

    // --- guy checksum image -------------------------------------------------

    #[test]
    fn guy_walk_image_is_155_bytes_and_packed() {
        assert_eq!(GUY_WALK_LEN, 155);
        let mut g = GuyData::default();
        g.ty = 0x0102_0304;
        g.guy_num = 7;
        g.who = 3;
        g.guy_flags = 0xBEEF;
        let img = g.walk_bytes();
        assert_eq!(&img[0..4], &[0x04, 0x03, 0x02, 0x01]);
        // guy_flags at +0x9A -> image offset 0x9A - 8 = 0x92
        assert_eq!(&img[0x92..0x94], &0xBEEFu16.to_le_bytes());
        // who at +0xA1 -> 0x99, guy_num at +0xA2 -> 0x9A
        assert_eq!(img[0x99], 3);
        assert_eq!(img[0x9A], 7);
    }

    #[test]
    fn ptrarray_walk_hashes_size_and_increment_twice() {
        // Distinguish the doubled range from a single pass: hashing the 6-byte block twice
        // must not equal hashing it once.
        let mut u = UnitGuys {
            size: 4,
            increment: 8,
            flags: 0x40,
            ..Default::default()
        };
        u.guys = vec![Some(GuyData::default()), None];
        let mut a = CheckSum::default();
        u.walk(&mut a);

        let mut b = CheckSum::default();
        b.walk(&2i32.to_le_bytes());
        b.walk(&4i32.to_le_bytes());
        b.walk(&8i16.to_le_bytes());
        b.walk(&[0]); // flags & !0x40
        b.walk(&[1, 0]);
        let mut six = [0u8; 6];
        six[0..4].copy_from_slice(&4i32.to_le_bytes());
        six[4..6].copy_from_slice(&8i16.to_le_bytes());
        b.walk(&six);
        b.walk(&GuyData::default().walk_bytes());
        assert_eq!(a, b);

        // and the doubling is load-bearing
        let mut c = CheckSum::default();
        c.walk(&2i32.to_le_bytes());
        c.walk(&4i32.to_le_bytes());
        c.walk(&8i16.to_le_bytes());
        c.walk(&[0]);
        c.walk(&[1, 0]);
        c.walk(&GuyData::default().walk_bytes());
        assert_ne!(a, c);
    }

    #[test]
    fn empty_guy_array_stops_after_the_length() {
        let u = UnitGuys::default();
        let mut a = CheckSum::default();
        u.walk(&mut a);
        let mut b = CheckSum::default();
        b.walk(&0i32.to_le_bytes());
        assert_eq!(a, b);
    }

    #[test]
    fn null_squad_slots_are_visible_to_the_checksum() {
        let t = ut();
        let mut full = UnitGuys::spawn_full(50, 1, 9, &t);
        let mut hurt = full.clone();
        hurt.guys[3] = None;
        hurt.guy_mark = 3;
        let (mut a, mut b) = (CheckSum::default(), CheckSum::default());
        full.walk(&mut a);
        hurt.walk(&mut b);
        assert_ne!(a, b, "losing a soldier must move the guys channel");
        full.guy_mark = 4;
    }

    // --- guy population -----------------------------------------------------

    #[test]
    fn guy_count_is_squad_plus_crew() {
        let mut t = ut();
        t.squad_size = 8;
        t.crew_size = 2;
        assert_eq!(UnitGuys::count_for(&t), 10);
        let u = UnitGuys::spawn_full(50, 0, 0, &t);
        assert_eq!(u.len(), 10);
        assert_eq!(u.guy_mark, 8);
        for (i, g) in u.guys.iter().enumerate() {
            assert_eq!(g.as_ref().unwrap().guy_num as usize, i);
        }
    }

    #[test]
    fn set_type_clamps_guy_mark_to_the_new_squad_size() {
        let mut big = ut();
        big.squad_size = 8;
        let mut u = UnitGuys::spawn_full(50, 0, 0, &big);
        assert_eq!(u.guy_mark, 8);
        let mut small = ut();
        small.squad_size = 3;
        small.crew_size = 1;
        u.set_type(51, 0, 0, &big, &small);
        assert_eq!(u.guy_mark, 3, "guy_mark is min(old, new squad_size)");
        assert_eq!(u.len(), 4);
    }

    #[test]
    fn set_type_destroys_old_crew_and_leaves_grown_squad_slots_null() {
        let mut old = ut();
        old.squad_size = 2;
        old.crew_size = 1;
        let mut u = UnitGuys::spawn_full(50, 0, 7, &old);
        u.guys[2].as_mut().unwrap().x = 12_345;

        let mut new = ut();
        new.squad_size = 3;
        new.crew_size = 1;
        u.set_type(51, 1, 8, &old, &new);

        assert_eq!(u.guy_mark, 2);
        assert!(u.guys[0].is_some() && u.guys[1].is_some());
        assert!(u.guys[2].is_none(), "new squad capacity is a null pointer");
        let crew = u.guys[3].as_ref().unwrap();
        assert_eq!(
            crew.x, 0,
            "old crew object was destroyed, not shifted/reused"
        );
        assert_eq!((crew.ty, crew.who, crew.o, crew.guy_num), (51, 1, 8, 3));
    }

    #[test]
    fn set_type_capacity_growth_honours_ptrarray_increment() {
        let mut old = ut();
        old.squad_size = 2;
        old.crew_size = 1;
        let mut u = UnitGuys::spawn_full(50, 0, 0, &old);
        u.increment = 4;

        let mut new = ut();
        new.squad_size = 4;
        new.crew_size = 1;
        u.set_type(51, 0, 0, &old, &new);
        assert_eq!(u.len(), 5);
        assert_eq!(u.size, 7, "capacity 3 grows by max(deficit 2, increment 4)");
    }

    #[test]
    fn crew_guys_are_not_squad() {
        let mut t = ut();
        t.squad_size = 2;
        t.crew_size = 2;
        let u = UnitGuys::spawn_full(50, 0, 0, &t);
        assert!(u.guys[0].as_ref().unwrap().is_squad(&t));
        assert!(u.guys[1].as_ref().unwrap().is_squad(&t));
        assert!(!u.guys[2].as_ref().unwrap().is_squad(&t));
        assert!(!u.guys[3].as_ref().unwrap().is_squad(&t));
    }

    #[test]
    fn initial_locations_are_retails_centered_three_wide_lattice() {
        let mut t = ut();
        t.squad_size = 4;
        t.crew_size = 0;
        t.guy_spacing = 48;
        let mut u = UnitGuys::spawn_full(50, 0, 0, &t);
        u.set_initial_locations(1_000, 1_000, 0, 0, 0, 10_000, 10_000, &t);

        let got: Vec<_> = u.guys.iter().flatten().map(|g| (g.x, g.y)).collect();
        assert_eq!(
            got,
            [(1_048, 976), (1_000, 976), (952, 976), (1_048, 1_024)]
        );
        for g in u.guys.iter().flatten() {
            assert_eq!((g.x, g.y), (g.des_x, g.des_y));
            assert_eq!((g.x, g.y), (g.last_x, g.last_y));
            assert_eq!((g.angle, g.des_angle, g.last_angle), (0, 0, 0));
            assert_eq!((g.off_x, g.off_y), (0, 0));
        }
    }

    #[test]
    fn sparse_column_reverse_and_world_edge_gates_are_exact() {
        let mut t = ut();
        t.guy_spacing = 48;
        let sparse = UnitGuys::initial_squad_locations(
            2,
            1_000,
            1_000,
            0x4000_0000,
            Formation::Sparse as i8,
            0,
            10_000,
            10_000,
            &t,
        );
        assert_eq!(sparse, [(1_000, 1_048), (1_000, 952)]);

        let column = UnitGuys::initial_squad_locations(
            4,
            1_000,
            1_000,
            0,
            Formation::Column as i8,
            0,
            10_000,
            10_000,
            &t,
        );
        assert_eq!(
            column,
            [(1_024, 976), (976, 976), (1_024, 1_024), (976, 1_024)]
        );

        let reversed = UnitGuys::initial_squad_locations(2, 0, 0, 0, 0, 2, 10_000, 10_000, &t);
        assert_eq!(
            reversed,
            [(0, 0), (24, 0)],
            "negative first point is clamped at the edge"
        );
    }

    #[test]
    fn measured_retail_single_squad_plus_crew_materializes_only_squad_at_anchor() {
        // Live v5 samples: type 62 had guy_mark=1, PtrArray length=3 and both crew Guys
        // at graphics-derived offsets; type 69 had guy_mark=1, length=2.  The collision
        // body is therefore exactly Guy 0 at the anchor in both cases.
        let mut t = ut();
        t.squad_size = 1;
        t.crew_size = 2;
        t.guy_spacing = 144;
        let mut u = UnitGuys::spawn_full(62, 0, 1, &t);
        u.guys[1].as_mut().unwrap().x = -192;
        u.guys[1].as_mut().unwrap().y = -56;
        u.set_initial_locations(
            3_480,
            29_592,
            0x5555_5555,
            0,
            0x0008_0000,
            50_000,
            50_000,
            &t,
        );
        let squad = u.guys[0].as_ref().unwrap();
        assert_eq!((squad.x, squad.y), (3_480, 29_592));
        assert_eq!((squad.off_x, squad.off_y), (0, 0));
        assert_eq!(
            (u.guys[1].as_ref().unwrap().x, u.guys[1].as_ref().unwrap().y),
            (-192, -56),
            "crew placement remains owned by graphics track offsets"
        );
    }

    // --- guy motion ---------------------------------------------------------

    #[test]
    fn turn_speed_crew_default_and_snap() {
        let t = ut();
        let env = GuyEnv::with_type(t);
        let mut g = GuyData {
            guy_num: 9,
            ..Default::default()
        }; // >= squad_size => crew
        g.track_dx = 1;
        assert_eq!(g.turn_speed(&env, 1), DEFAULT_TURN_SPEED);
        g.track_dx = 0;
        g.guy_flags |= GUY_FLAG_FAST_FACE;
        g.last_speed = 0;
        assert_eq!(g.turn_speed(&env, 1), SNAP_TURN_SPEED);
    }

    #[test]
    fn turn_speed_squad_uses_the_type_rule() {
        let mut t = ut();
        t.turn_speed = 0x1_0000; // >> 8 == 0x100
        let mut env = GuyEnv::with_type(t);
        env.turn_scale = 3;
        let g = GuyData {
            guy_num: 0,
            ..Default::default()
        };
        assert_eq!(g.turn_speed(&env, 1), 0x100 * 3);
        let mut env2 = env;
        env2.unit_mask_turn_scale2 = true;
        env2.turn_scale2 = 2;
        assert_eq!(g.turn_speed(&env2, 1), 0x100 * 3 * 2);
    }

    #[test]
    fn turn_speed_damped_form_floors_at_the_rule_product() {
        let mut t = ut();
        t.turn_speed = 0x100; // >> 8 == 1
        let mut env = GuyEnv::with_type(t);
        env.turn_scale = 2;
        let g = GuyData {
            guy_num: 0,
            avg_speed: 400,
            ..Default::default()
        };
        // step/(avg/4+1) = 2/101 = 0, so the floor wins.
        assert_eq!(g.turn_speed(&env, 0), 2 * TURN_FLOOR_MUL);
    }

    #[test]
    fn turn_towards_snaps_inside_three_degrees() {
        let t = ut();
        let env = GuyEnv::with_type(t);
        let mut g = GuyData {
            guy_num: 0,
            angle: 0,
            ..Default::default()
        };
        let des = (ANGLE_SNAP - 1) as i32;
        assert_eq!(g.turn_towards(des as u32, &env), 0);
        assert_eq!(g.angle, des);
        assert_ne!(g.guy_flags & GUY_FLAG_NO_IDLE_TURN, 0);
    }

    #[test]
    fn turn_towards_does_not_mark_an_unchanged_facing() {
        let t = ut();
        let env = GuyEnv::with_type(t);
        let mut g = GuyData {
            guy_num: 0,
            angle: 0x1234_5678,
            guy_flags: GUY_FLAG_FAST_FACE,
            ..Default::default()
        };
        assert_eq!(g.turn_towards(0x1234_5678, &env), 0);
        assert_eq!(g.guy_flags, GUY_FLAG_FAST_FACE);
    }

    #[test]
    fn turn_towards_takes_the_short_way_round() {
        let mut t = ut();
        t.turn_speed = 0x0100_0000; // >> 8 == 0x10000
        let env = GuyEnv::with_type(t);
        // Desired is just clockwise of current: angle should increase.
        let mut g = GuyData {
            guy_num: 0,
            angle: 0,
            ..Default::default()
        };
        g.turn_towards(0x2000_0000, &env);
        assert_eq!(g.angle, 0x0001_0000);
        // Desired is just anticlockwise: angle should decrease (wrap negative).
        let mut h = GuyData {
            guy_num: 0,
            angle: 0,
            ..Default::default()
        };
        h.turn_towards(0xE000_0000u32, &env);
        assert_eq!(h.angle as u32, 0u32.wrapping_sub(0x0001_0000));
    }

    #[test]
    fn turn_angles_halves_the_step_and_does_not_mutate() {
        let mut t = ut();
        t.turn_speed = 0x0100_0000;
        let env = GuyEnv::with_type(t);
        let g = GuyData {
            guy_num: 0,
            angle: 0,
            ..Default::default()
        };
        let (full, _) = g.turn_angles(0x2000_0000, &env, false);
        let (half, _) = g.turn_angles(0x2000_0000, &env, true);
        assert_eq!(full, 0x0001_0000);
        assert_eq!(half, 0x0000_8000);
        assert_eq!(g.angle, 0, "turn_angles must not mutate");
    }

    #[test]
    fn avg_speed_is_a_quarter_weight_filter_rounding_toward_zero() {
        let mut g = GuyData {
            avg_speed: 100,
            last_speed: 0,
            ..Default::default()
        };
        g.update_avg_speed();
        assert_eq!(g.avg_speed, 75);
        let mut h = GuyData {
            avg_speed: -3,
            last_speed: 0,
            ..Default::default()
        };
        h.update_avg_speed();
        assert_eq!(h.avg_speed, -2, "(-9)/4 truncates toward zero");
    }

    #[test]
    fn guy_zero_teleports_to_its_destination() {
        let t = ut();
        let env = GuyEnv::with_type(t);
        let mut g = GuyData {
            guy_num: 0,
            x: 0,
            y: 0,
            des_x: 1000,
            des_y: 0,
            ..Default::default()
        };
        let out = g.r#move(&env, &|_, _| true);
        assert_eq!(out, MoveOutcome::Snapped);
        assert_eq!((g.x, g.y), (1000, 0));
        assert_eq!(g.last_speed, 1000);
        assert_eq!(g.last_x, 0);
    }

    #[test]
    fn idle_guy_turns_toward_des_angle_and_zeroes_last_speed() {
        let mut t = ut();
        t.turn_speed = 0x0100_0000;
        let env = GuyEnv::with_type(t);
        let mut g = GuyData {
            guy_num: 0,
            x: 5,
            y: 5,
            des_x: 5,
            des_y: 5,
            des_angle: 0x2000_0000,
            avg_speed: 40,
            ..Default::default()
        };
        let out = g.r#move(&env, &|_, _| true);
        assert_eq!(out, MoveOutcome::Idle);
        assert_eq!(g.last_speed, 0);
        assert_eq!(g.angle, 0x0001_0000);
        assert_eq!(g.avg_speed, 30);
    }

    #[test]
    fn tracked_guy_off_angle_turns_only_and_skips_the_filter() {
        let mut t = ut();
        t.turn_speed = 0x100; // tiny step, so the remainder stays huge
        let env = GuyEnv::with_type(t);
        let mut g = GuyData {
            guy_num: 1,
            track_dx: 1,
            x: 0,
            y: 0,
            des_x: 0,
            des_y: -1000, // due north; guy faces east
            angle: 0x4000_0000,
            avg_speed: 100,
            last_speed: 40,
            ..Default::default()
        };
        let out = g.r#move(&env, &|_, _| true);
        assert_eq!(out, MoveOutcome::TurnedOnly);
        assert_eq!((g.x, g.y), (0, 0));
        assert_eq!(g.avg_speed, 100, "the early return skips update_avg_speed");
    }

    #[test]
    fn tracked_guy_steps_along_its_facing() {
        let mut t = ut();
        t.turn_speed = 0x0800_0000;
        let mut env = GuyEnv::with_type(t);
        env.unit_speed = 80; // step = 80*11/8 = 110
        let mut g = GuyData {
            guy_num: 1,
            track_dx: 1,
            x: 0,
            y: 0,
            des_x: 100_000,
            des_y: 0, // due east
            angle: 0x4000_0000,
            ..Default::default()
        };
        let out = g.r#move(&env, &|_, _| true);
        assert_eq!(out, MoveOutcome::Stepped);
        assert_eq!(g.last_speed, 110);
        assert_eq!((g.x, g.y), (110, 0));
    }

    #[test]
    fn a_blocked_step_leaves_the_offset_subtraction_behind() {
        // This is retail's behaviour, not a nicety: `x -= off_x` happens before the
        // validity test and is not undone.
        let mut t = ut();
        t.turn_speed = 0x0800_0000;
        let mut env = GuyEnv::with_type(t);
        env.unit_speed = 80;
        let mut g = GuyData {
            guy_num: 1,
            track_dx: 1,
            x: 0,
            y: 0,
            off_x: 7,
            off_y: 3,
            des_x: 100_000,
            des_y: 0,
            angle: 0x4000_0000,
            stopped: 1,
            ..Default::default()
        };
        let out = g.r#move(&env, &|_, _| false);
        assert_eq!(out, MoveOutcome::Blocked);
        assert_eq!((g.x, g.y), (-7, -3));
        assert_eq!(g.stopped, 0);
    }

    #[test]
    fn ai_speed_multiplies_the_step_but_not_last_speed() {
        let mut t = ut();
        t.turn_speed = 0x0800_0000;
        let mut env = GuyEnv::with_type(t);
        env.unit_speed = 80;
        env.ai_speed = 4;
        let mut g = GuyData {
            guy_num: 1,
            track_dx: 1,
            x: 0,
            y: 0,
            des_x: 100_000,
            des_y: 0,
            angle: 0x4000_0000,
            ..Default::default()
        };
        g.r#move(&env, &|_, _| true);
        assert_eq!(g.last_speed, 110, "last_speed is the unscaled step");
        assert_eq!(g.x, 440, "the applied step is scaled by ai_speed");
    }

    #[test]
    fn get_speed_bonus_is_squad_only() {
        let t = ut();
        let mut env = GuyEnv::with_type(t);
        env.unit_speed = 50;
        env.order_speed_bonus = true;
        let squad = GuyData {
            guy_num: 0,
            ..Default::default()
        };
        let crew = GuyData {
            guy_num: 9,
            ..Default::default()
        };
        assert_eq!(squad.get_speed(&env), 59);
        assert_eq!(crew.get_speed(&env), 50);
    }

    // --- guy process --------------------------------------------------------

    #[test]
    fn turrets_settle_at_one_twenty_fourth_of_a_turn() {
        assert_eq!(TURRET_STEP as u64 * 24, 0x0AAA_AAAAu64 * 24);
        assert!(((1u64 << 32) - TURRET_STEP as u64 * 24) < 24);
        let t = ut();
        let env = GuyEnv::with_type(t);
        let mut g = GuyData {
            guy_num: 0,
            ..Default::default()
        };
        g.guy_flags |= GUY_FLAG_TURRETS;
        g.turret_angles[0] = 0;
        g.des_turret_angles[0] = 0x4000_0000;
        let mut noop = |_: &GuyData| {};
        g.process(&env, 1, &|_, _| true, &mut noop);
        // 0x40000000 is more than one step away, and des is clockwise of cur, so
        // `cur - des` is negative-as-u32 => the >= 0x80000000 arm adds a step.
        assert_eq!(g.turret_angles[0] as u32, TURRET_STEP);
        assert_eq!(g.node_flags & 1, 0);
        // walk it all the way in
        for _ in 0..8 {
            g.process(&env, 1, &|_, _| true, &mut noop);
        }
        assert_eq!(g.turret_angles[0], 0x4000_0000);
        assert_eq!(g.node_flags & 1, 1);
    }

    #[test]
    fn block_registration_is_gated_on_phase_squad_and_radius() {
        let mut t = ut();
        t.new_block_radius = 2;
        let env = GuyEnv::with_type(t);
        let hits = std::cell::Cell::new(0);
        let mut count = |_: &GuyData| hits.set(hits.get() + 1);
        // o = 0 so the phase is `frame % 64`.
        let mut g = GuyData {
            guy_num: 0,
            o: 0,
            avg_speed: 0,
            ..Default::default()
        };
        g.des_x = g.x;
        g.des_y = g.y;
        g.process(&env, 64, &|_, _| true, &mut count);
        assert_eq!(hits.get(), 1);
        g.process(&env, 65, &|_, _| true, &mut count);
        assert_eq!(hits.get(), 1, "off-phase frames do not register");

        // air (domain 2) never registers
        let mut air = env;
        air.ut.domain = 2;
        g.process(&air, 128, &|_, _| true, &mut count);
        assert_eq!(hits.get(), 1);

        // crew never registers
        let mut crew = GuyData {
            guy_num: 9,
            o: 0,
            ..Default::default()
        };
        crew.des_x = crew.x;
        crew.des_y = crew.y;
        crew.process(&env, 128, &|_, _| true, &mut count);
        assert_eq!(hits.get(), 1);
    }

    #[test]
    fn process_clears_the_two_flag_bits() {
        let t = ut();
        let env = GuyEnv::with_type(t);
        let mut g = GuyData {
            guy_num: 0,
            o: 0,
            guy_flags: GUY_FLAG_NO_IDLE_TURN | GUY_FLAG_BLOCK_DIRTY,
            ..Default::default()
        };
        let mut noop = |_: &GuyData| {};
        g.process(&env, 64, &|_, _| true, &mut noop);
        assert_eq!(g.guy_flags & GUY_FLAG_NO_IDLE_TURN, 0);
        assert_eq!(g.guy_flags & GUY_FLAG_BLOCK_DIRTY, 0);
    }

    // --- groups -------------------------------------------------------------

    #[test]
    fn group_layout_offsets_match_the_pdb() {
        // header is +0x04..+0x4C
        assert_eq!(GroupData::default().header_bytes().len(), 0x4C - 0x04);
        assert_eq!(GROUP_MAX_MEMBERS, 128);
        // off_x .. curr_y are four 128-int arrays: 0x4C + 0x800 == 0x84C (angles)
        assert_eq!(0x4C + 4 * 4 * GROUP_MAX_MEMBERS, 0x84C);
        // angles is 128 bytes: 0x84C + 0x80 == 0x8CC (list)
        assert_eq!(0x84C + GROUP_MAX_MEMBERS, 0x8CC);
        // list is 128 shorts: 0x8CC + 0x100 == 0x9CC == sizeof(GroupData)
        assert_eq!(0x8CC + 2 * GROUP_MAX_MEMBERS, 2508);
        assert_eq!(SIZEOF_GROUP, 2516); // + GroupOut's 4 and Group's 4... see the doc
    }

    #[test]
    fn empty_group_walks_only_its_header() {
        let g = GroupData::default();
        let mut a = CheckSum::default();
        g.walk(&mut a);
        let mut b = CheckSum::default();
        b.walk(&g.header_bytes());
        assert_eq!(a, b);
    }

    #[test]
    fn group_walk_is_length_prefixed_so_stale_entries_are_invisible() {
        let mut a = GroupData::default();
        a.add(5, 0, false, 0, 1);
        a.add(6, 0, false, 0, 1);
        let mut b = a.clone();
        // scribble past the live count
        b.list[7] = 999;
        b.off_x[9] = 12345;
        b.curr_y[100] = -1;
        let (mut ca, mut cb) = (CheckSum::default(), CheckSum::default());
        a.walk(&mut ca);
        b.walk(&mut cb);
        assert_eq!(ca, cb);
    }

    #[test]
    fn group_walk_hashes_list_before_the_offsets() {
        let mut g = GroupData::default();
        g.add(5, 0, false, 0, 1);
        g.off_x[0] = 5;
        let mut got = CheckSum::default();
        g.walk(&mut got);

        let mut want = CheckSum::default();
        want.walk(&g.header_bytes());
        want.walk(&5i16.to_le_bytes()); // list first
        want.walk(&5i32.to_le_bytes()); // off_x
        want.walk(&0i32.to_le_bytes()); // off_y
        want.walk(&0i32.to_le_bytes()); // curr_x
        want.walk(&0i32.to_le_bytes()); // curr_y
        want.walk(&[0u8]); // angles
        assert_eq!(got, want);
    }

    #[test]
    fn group_add_caps_at_128_and_rejects_duplicates_and_other_owners() {
        let mut g = GroupData::default();
        assert!(g.add(1, 2, false, 0x10, 7));
        assert_eq!(g.who, 2);
        assert_eq!(g.stamp, 7);
        assert_eq!(g.role, 0x10);
        assert!(!g.add(1, 2, false, 0, 8), "duplicate rejected");
        assert!(!g.add(9, 3, false, 0, 8), "other owner rejected");
        assert!(g.add(2, 2, false, 0x01, 9));
        assert_eq!(g.role, 0x11, "role is an OR accumulator");
        for i in 3..=GROUP_MAX_MEMBERS as i16 {
            g.add(i, 2, false, 0, 9);
        }
        assert_eq!(g.num, GROUP_MAX_MEMBERS as i32);
        assert!(!g.add(1000, 2, false, 0, 9));
    }

    #[test]
    fn normalize_compacts_all_six_arrays_together() {
        let mut g = GroupData::default();
        for i in 0..4i16 {
            g.add(i, 0, false, 0, 0);
            let n = (g.num - 1) as usize;
            g.off_x[n] = i as i32 * 10;
            g.angles[n] = i as i8;
        }
        // drop object index 1
        g.normalize(&|o| {
            if o == 1 {
                MemberState::Dead
            } else {
                MemberState::Keep
            }
        });
        assert_eq!(g.num, 3);
        assert_eq!(&g.list[..3], &[0, 2, 3]);
        assert_eq!(&g.off_x[..3], &[0, 20, 30]);
        assert_eq!(&g.angles[..3], &[0, 2, 3]);
    }

    #[test]
    fn normalize_drops_negative_entries() {
        let mut g = GroupData::default();
        g.add(4, 0, false, 0, 0);
        g.add(5, 0, false, 0, 0);
        g.list[0] = -1;
        g.normalize(&|_| MemberState::Keep);
        assert_eq!(g.num, 1);
        assert_eq!(g.list[0], 5);
    }

    #[test]
    fn not_our_unit_only_drops_for_a_named_zero_priority_group() {
        let mut g = GroupData::default();
        g.add(4, 0, false, 0, 0);
        g.id = -1; // transient selection
        g.normalize(&|_| MemberState::NotOurUnit);
        assert_eq!(g.num, 1, "id < 0 keeps it");
        g.id = 3;
        g.priority = 1;
        g.normalize(&|_| MemberState::NotOurUnit);
        assert_eq!(g.num, 1, "priority != 0 keeps it");
        g.priority = 0;
        g.normalize(&|_| MemberState::NotOurUnit);
        assert_eq!(g.num, 0);
    }

    #[test]
    fn get_num_takes_the_full_normalize_path_below_four_members() {
        let mut small = GroupData {
            id: 2,
            ..Default::default()
        };
        small.add(4, 0, false, 0, 0);
        assert_eq!(small.get_num(&|_| MemberState::NotOurUnit), 0);

        let mut big = GroupData {
            id: 2,
            ..Default::default()
        };
        for i in 0..6i16 {
            big.add(i, 0, false, 0, 0);
        }
        assert_eq!(
            big.get_num(&|_| MemberState::NotOurUnit),
            6,
            "alive-only scan keeps them"
        );
    }

    #[test]
    fn update_positions_rotates_offsets_into_world_space() {
        let mut g = GroupData::default();
        g.form_num = 2;
        g.off_x[0] = 1; // one cell to the right
        g.off_y[0] = 0;
        g.off_x[1] = 0;
        g.off_y[1] = 1; // one cell forward

        // facing north: right is +x, forward is -y
        g.update_positions(0);
        assert_eq!((g.curr_x[0], g.curr_y[0]), (FORM_CELL, 0));
        assert_eq!((g.curr_x[1], g.curr_y[1]), (0, -FORM_CELL));

        // facing east: right is +y, forward is +x
        g.update_positions(0x4000_0000);
        assert_eq!((g.curr_x[0], g.curr_y[0]), (0, FORM_CELL));
        assert_eq!((g.curr_x[1], g.curr_y[1]), (FORM_CELL, 0));
    }

    #[test]
    fn update_positions_bound_is_form_num_not_num() {
        let mut g = GroupData::default();
        g.add(1, 0, false, 0, 0);
        g.form_num = 0;
        g.off_x[0] = 5;
        g.update_positions(0);
        assert_eq!(
            g.curr_x[0], 0,
            "num=1 but form_num=0 means nothing is written"
        );
    }

    #[test]
    fn compute_speed_zeroes_for_buildings_and_empty_groups() {
        let mut g = GroupData::default();
        g.add(1, 0, false, 0, 0);
        g.compute_speed(Some(42));
        assert_eq!((g.speed, g.new_speed), (42, 42));
        g.buildings = 1;
        g.compute_speed(Some(42));
        assert_eq!((g.speed, g.new_speed), (0, 0));
        let mut e = GroupData::default();
        e.compute_speed(Some(42));
        assert_eq!(e.speed, 0);
    }

    #[test]
    fn form_cycling_matches_both_of_retails_wrap_rules() {
        assert_eq!(resolve_form(form_arg::CYCLE_NEXT, 4, 0), 0);
        assert_eq!(resolve_form(form_arg::CYCLE_NEXT, 0, 0), 1);
        assert_eq!(resolve_form(form_arg::CYCLE_PREV, 0, 0), 4);
        assert_eq!(resolve_form(form_arg::CYCLE_PREV, 3, 0), 2);
        assert_eq!(resolve_form(form_arg::FROM_LEADER, 0, 7), 7);
        assert_eq!(resolve_form(2, 0, 0), 2);
        // the asymmetry, kept deliberately
        assert_eq!(resolve_form(form_arg::CYCLE_NEXT, 7, 0), 3);
        assert_eq!(resolve_form(form_arg::CYCLE_PREV, 7, 0), 6);
    }

    #[test]
    fn formations_are_the_ten_in_rules_xml() {
        assert_eq!(Formation::Line as i32, 0);
        assert_eq!(Formation::Mob as i32, 9);
        assert_eq!(FORM_CYCLE_LEN, 5);
    }

    // --- the two channels ---------------------------------------------------

    #[test]
    fn groups_process_walks_one_slot_per_player_per_frame_and_wraps_at_64() {
        let mut gs = Groups::default();
        let active = [true; NUM_LEADERS];
        for who in 0..NUM_LEADERS {
            gs.get_mut(who, 0).add(1, who as u8, false, 0, 0);
            gs.get_mut(who, 1).add(2, who as u8, false, 0, 0);
        }
        // frame 0 processes slot 0 only
        gs.process(&active, &|_, _| MemberState::Dead, &|_, _| None);
        for who in 0..NUM_LEADERS {
            assert_eq!(gs.get(who, 0).num, 0);
            assert_eq!(gs.get(who, 1).num, 1, "slot 1 is untouched this frame");
        }
        assert_eq!(gs.proc_group, 1);
        for _ in 0..GROUPS_PER_PLAYER - 1 {
            gs.process(&active, &|_, _| MemberState::Keep, &|_, _| None);
        }
        assert_eq!(gs.proc_group, 0, "the cursor wraps at 0x3F");
    }

    #[test]
    fn groups_process_skips_inactive_leaders() {
        let mut gs = Groups::default();
        let mut active = [false; NUM_LEADERS];
        active[3] = true;
        for who in 0..NUM_LEADERS {
            gs.get_mut(who, 0).add(1, who as u8, false, 0, 0);
        }
        gs.process(&active, &|_, _| MemberState::Dead, &|_, _| None);
        assert_eq!(gs.get(3, 0).num, 0);
        assert_eq!(gs.get(2, 0).num, 1);
    }

    #[test]
    fn check_groups_covers_every_slot_and_the_last_group_block() {
        let mut gs = Groups::default();
        assert_eq!(gs.list.len(), NUM_GROUPS);
        let mut base = CheckSum::default();
        gs.check_groups(&mut base);

        // a change in the very last player's very last slot must move the channel
        let mut moved = gs.clone();
        moved
            .get_mut(NUM_LEADERS - 1, GROUPS_PER_PLAYER - 1)
            .add(1, 7, false, 0, 0);
        let mut c = CheckSum::default();
        moved.check_groups(&mut c);
        assert_ne!(base, c);

        // so must last_group
        gs.last_group[7] = 5;
        let mut d = CheckSum::default();
        gs.check_groups(&mut d);
        assert_ne!(base, d);

        // proc_group must not
        let mut e = gs.clone();
        e.proc_group = 33;
        let mut f = CheckSum::default();
        e.check_groups(&mut f);
        assert_eq!(d, f);
    }

    #[test]
    fn check_guys_honours_the_three_gates() {
        let t = ut();
        let mk = || OwnerUnits {
            units: vec![Some(UnitSlot {
                flags: 1,
                guys: UnitGuys::spawn_full(50, 0, 0, &t),
            })],
        };
        let owners: [OwnerUnits; NUM_LEADERS] = [mk(), mk(), mk(), mk(), mk(), mk(), mk(), mk()];
        let all = [true; NUM_LEADERS];

        let mut a = CheckSum::default();
        check_guys(&mut a, true, &all, &owners);

        // objects.valid == 0 -> nothing at all
        let mut b = CheckSum::default();
        check_guys(&mut b, false, &all, &owners);
        assert_eq!(b, CheckSum::default());

        // an inactive leader removes its whole band
        let mut some = [true; NUM_LEADERS];
        some[5] = false;
        let mut c = CheckSum::default();
        check_guys(&mut c, true, &some, &owners);
        assert_ne!(a, c);

        // a unit with flags bit 0 clear is skipped
        let mut owners2 = owners.clone();
        owners2[0].units[0].as_mut().unwrap().flags = 0;
        let mut d = CheckSum::default();
        check_guys(&mut d, true, &all, &owners2);
        assert_ne!(a, d);
    }

    #[test]
    fn a_single_guy_moving_moves_the_guys_channel() {
        let t = ut();
        let mut guys = UnitGuys::spawn_full(50, 0, 0, &t);
        let owners_of = |g: &UnitGuys| -> [OwnerUnits; NUM_LEADERS] {
            let empty = OwnerUnits::default();
            [
                OwnerUnits {
                    units: vec![Some(UnitSlot {
                        flags: 1,
                        guys: g.clone(),
                    })],
                },
                empty.clone(),
                empty.clone(),
                empty.clone(),
                empty.clone(),
                empty.clone(),
                empty.clone(),
                empty,
            ]
        };
        let active = [true; NUM_LEADERS];
        let mut before = CheckSum::default();
        check_guys(&mut before, true, &active, &owners_of(&guys));

        guys.guys[2].as_mut().unwrap().x += 1;
        let mut after = CheckSum::default();
        check_guys(&mut after, true, &active, &owners_of(&guys));
        assert_ne!(before, after);
    }
}
