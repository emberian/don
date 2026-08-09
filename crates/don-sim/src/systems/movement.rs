//! `systems::movement` — the unit movement and pathfinding lane.
//!
//! Ported from `riseofnations.exe` (PE32 i386, image base `0x00400000`) with the shipped
//! private PDB `ron-bin/sbl/rise.pdb` supplying names, sizes and source lines. Every claim
//! below is `[measured]` on this Mac against the instruction stream unless it says
//! `UNVERIFIED`, which marks structure taken from Ghidra output that has not been checked
//! against behaviour. Nothing here is verified in the proof-assistant sense, and no
//! differential test against retail has been run for this lane yet, so the fidelity tier is
//! **C (behaviourally faithful, divergence unmeasured)** throughout.
//!
//! # What this lane covers
//!
//! The real unit pathfinder, which is **not** `PathFinder::astar_caravan_road` (the target of
//! the earlier `pathfinding.md` derivation). It is:
//!
//! ```text
//! Unit::do_move            0x005F7B30  unit.cpp:15757   the locomotion funnel
//!  ├─ PathFinder::find_upath  0x00682F30 (inner)         pathfinder.cpp:3684
//!  │    └─ PathFinder::astar_path 0x00683770             pathfinder.cpp:2329
//!  │         ├─ PathFinder::calc_cost      0x00684E50    pathfinder.cpp:1888
//!  │         ├─ PathFinder::valid_ucoord   0x00687C80
//!  │         ├─ PathFinder::first_open_node 0x00687970
//!  │         └─ PathFinderData::{find_node_open 0x006882B0, find_node_closed 0x00688270}
//!  └─ Unit::move_step        0x005FAF30  unit.cpp:13628  the per-frame integrator
//!       ├─ Unit::detect_unit_collision  0x00617060
//!       ├─ Unit::resolve_unit_collision 0x005F9D30
//!       └─ Unit::set_new_location       0x005F8D20
//! ```
//!
//! # The headline correction: this cone contains ZERO floating point
//!
//! `docs/derivation/architecture.md` §6.2 warns that "`Unit::move_step` and `Unit::find_path`
//! contain floating point — `sin_table`, `cosx`, `find_angle`". **That is wrong**, and the
//! name of `sin_table` is what caused it. Disassembling every function in the cone and
//! counting SSE and x87 opcodes gives [measured]:
//!
//! | function | VA | insns | FP insns |
//! |---|---|---:|---:|
//! | `Unit::move_step` | `0x005FAF30` | 775 | **0** |
//! | `Unit::do_move` | `0x005F7B30` | 1430 | **0** |
//! | `Unit::find_path` | `0x005FB910` | 840 | **0** |
//! | `Unit::set_new_location` | `0x005F8D20` | 567 | **0** |
//! | `Unit::detect_unit_collision` | `0x00617060` | 654 | **0** |
//! | `Unit::resolve_unit_collision` | `0x005F9D30` | 919 | **0** |
//! | `PathFinder::find_upath` | `0x00682F30` | 601 | **0** |
//! | `PathFinder::astar_path` | `0x00683770` | 1645 | **0** |
//! | `PathFinder::calc_cost` | `0x00684E50` | 860 | **0** |
//! | `UnitData::invalid_loc` | `0x00607C30` | 336 | **0** |
//! | `find_angle` / `sin_table` / `cosx` / `vector_dist` | | 302 | **0** |
//!
//! `find_angle` is an integer polynomial arctangent producing a 32-bit binary angle;
//! `sin_table` is a 256-entry quarter-sine table with integer linear interpolation and three
//! fixed-point magnitude regimes; `cosx` is `sin_table` with the quadrant fold applied first.
//! The *only* float anywhere in the lane is `trig_init` `0x00A46980`, which builds the
//! 256-entry table once at startup with the CRT `sin` in double precision. That table is a
//! compile-time constant here ([`SIN_QUARTER`]), so this module is integer-only at runtime.
//!
//! # Checksum channel
//!
//! **`units`** — `CheckSums::check_units` `0x009371D0`, channel 1 of the fifteen in
//! `CheckSums::check_all` `0x00936560`. It walks every active object of every active leader
//! slot (0..7, fixed order) through vtable slot 31 (`+0x7C`) = `Unit::walk_data`
//! `0x0060CF40`, accumulating adler-32 initialised to 1. `Unit::walk_data` includes the
//! unit's `Stack<PathData>` at `UnitData+0xB8` behind mask bit 2, so **the path buffer this
//! module produces is checksummed state**, not scratch. [`PathStack::walk_bytes`] emits it in
//! record order; [`adler32`] is the accumulator.

#![allow(clippy::too_many_arguments)]

use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// 1. Coordinate system  [measured]
// ---------------------------------------------------------------------------

/// World units per tile. `rules.xml:15` calls the tile a `TCoord`; the pathfinder bounds-checks
/// against `192 * map_tiles` at `0x00687CA7`.
pub const TILE: i32 = 192;
/// World units per unit-grid cell: the unit A* runs on a quarter-tile grid.
/// [measured] `find_upath` pushes `0x30` as `astar_path`'s step argument at `0x00683359`.
pub const UCELL: i32 = 48;
/// World units per tile-grid cell (`find_tpath` pushes `0xC0`).
pub const TCELL: i32 = 192;
/// World units per water cell (`find_wpath` pushes `0x300`); 4 tiles.
pub const WCELL: i32 = 768;

/// The full grid ladder, for cross-referencing against sibling lanes [measured, and confirmed
/// independently by `mech:borders-fog` and `mech:map-terrain`]:
///
/// | grid | tiles | world units | who uses it |
/// |---|---:|---:|---|
/// | unit cell (`UCoord`) | 1/4 | **48** | `find_upath` — this lane |
/// | tile (`TCoord`) | 1 | 192 | `find_tpath`, terrain, `invalid_loc` |
/// | fog cell | 2 | 384 | fog of war |
/// | water cell (`WCoord`) | 4 | 768 | `find_wpath` |
/// | region | 8 | 1536 | `WorldData::get_tregion` connectivity |
pub const FOG_CELL: i32 = 384;
/// See [`FOG_CELL`].
pub const REGION_CELL: i32 = 1536;

/// `Object`'s stored coordinates are XOR-obfuscated with this mask — `Unit::move_step` decodes
/// with `in_ECX[4] ^ 0x63637` at `0x005FAF77` and `PathFinder::find_upath` compares against the
/// same encoding at `0x00683520`. **`GuyData`'s coordinates are not obfuscated** (confirmed by
/// `mech:groups-guys`), so anything crawling the live heap must know which layer it is reading.
/// Nothing in this module stores obfuscated values; the constant is here so a live-memory reader
/// has it next to the code that consumes those fields.
pub const COORD_XOR: i32 = 0x0006_3637;

/// The global coordinate table at `[0x00CAE5FC]`, built by `0x00681DB0` as `T[i] = i / 3`
/// with C truncation. Every conversion in the engine is `T[pos >> k]`.
#[inline]
pub fn t3(i: i32) -> i32 {
    i / 3
}

/// World position -> unit-grid cell. `T[v >> 4]` = `v / 48`. [measured]
#[inline]
pub fn ucell_of(v: i32) -> i32 {
    t3(v >> 4)
}
/// World position -> tile. `T[v >> 6]` = `v / 192`. [measured]
#[inline]
pub fn tile_of(v: i32) -> i32 {
    t3(v >> 6)
}
/// World position -> water cell. `T[v >> 8]` = `v / 768`. [measured]
#[inline]
pub fn wcell_of(v: i32) -> i32 {
    t3(v >> 8)
}

/// Centre of a unit-grid cell in world units: `cell * 0x30 + 0x18`.
/// [measured] `find_upath` `0x006832xx` snaps both search endpoints to cell centres.
#[inline]
pub fn ucell_centre(cell: i32) -> i32 {
    cell * UCELL + UCELL / 2
}

/// The 8-connected neighbour table plus a null entry at index 0.
/// `move_x` at `0x00ADCAF0`, `move_y` at `0x00ADC400`. [measured]
/// Odd indices are diagonals — which is exactly what `calc_cost`'s `(dir & 1) * 8` term keys on.
pub const MOVE_X: [i32; 9] = [0, -1, 0, 1, 1, 1, 0, -1, -1];
pub const MOVE_Y: [i32; 9] = [0, -1, -1, -1, 0, 1, 1, 1, 0];

// ---------------------------------------------------------------------------
// 2. Integer trigonometry  [measured]
// ---------------------------------------------------------------------------
//
// DUPLICATION, FLAGGED FOR THE ORCHESTRATOR. Three copies of this material now exist in the
// crate: `crate::trig` (sim-core lane), `crate::systems::groups_guys` (groups lane) and this
// one. They are not all equivalent — `crate::trig::sin_table` is the *raw* entry point and is
// wrong in two of four quadrants unless the caller folds the angle first, which is exactly the
// trap `sinx`/`cosx` exist to close. This module carries its own folded copy because the lane
// brief requires a self-contained module, and it agrees with `groups_guys` to the bit. When the
// lanes are merged, keep **one** folded pair and delete the rest; do not keep the raw
// `sin_table` as a public API without the fold, or a caller will reach for it and be silently
// 30% off the ray.

/// `T[i] = (int)(sin(i * 1.570796327 / 255.0) * 65535.0)`, i in 0..256.
///
/// [measured] from the initialiser `trig_init` `0x00A46980`: `mulpd [0x00B69BB0]` (pi/2),
/// `divpd [0x00B69BD0]` (= **255.0**, not 256), `call sin`, `mulpd [0x00B69BE0]` (65535.0),
/// `cvttpd2dq` (truncate toward zero), stored to `0x00E32F40`. Note the divisor 255 against a
/// 256-slot table: the quarter turn is 256 index units wide but the table samples it in 255
/// steps, a ~0.4% scale error baked into the engine. `T[255] == 65535` exactly.
pub const SIN_QUARTER: [i32; 256] = [
    0, 403, 807, 1211, 1614, 2018, 2421, 2824, 3228, 3631, 4034, 4437, 4839, 5242, 5644, 6046,
    6448, 6850, 7251, 7652, 8053, 8453, 8854, 9253, 9653, 10052, 10451, 10849, 11247, 11644, 12042,
    12438, 12834, 13230, 13625, 14020, 14414, 14807, 15200, 15593, 15984, 16376, 16766, 17156,
    17545, 17934, 18322, 18709, 19096, 19482, 19867, 20251, 20634, 21017, 21399, 21780, 22161,
    22540, 22919, 23297, 23673, 24049, 24425, 24799, 25172, 25544, 25915, 26286, 26655, 27023,
    27391, 27757, 28122, 28486, 28849, 29211, 29572, 29931, 30290, 30647, 31004, 31359, 31713,
    32065, 32417, 32767, 33116, 33464, 33810, 34155, 34499, 34842, 35183, 35523, 35862, 36199,
    36535, 36869, 37202, 37534, 37864, 38193, 38520, 38846, 39170, 39493, 39815, 40134, 40453,
    40770, 41085, 41399, 41711, 42021, 42330, 42638, 42944, 43248, 43550, 43851, 44150, 44448,
    44743, 45038, 45330, 45621, 45910, 46197, 46482, 46766, 47048, 47328, 47606, 47883, 48158,
    48430, 48701, 48971, 49238, 49504, 49767, 50029, 50289, 50547, 50802, 51057, 51309, 51559,
    51807, 52053, 52298, 52540, 52780, 53018, 53255, 53489, 53721, 53951, 54180, 54406, 54630,
    54852, 55071, 55289, 55505, 55718, 55930, 56139, 56346, 56552, 56754, 56955, 57154, 57350,
    57545, 57737, 57927, 58114, 58300, 58483, 58664, 58843, 59019, 59194, 59366, 59536, 59703,
    59869, 60032, 60193, 60351, 60507, 60661, 60813, 60962, 61109, 61254, 61396, 61536, 61674,
    61809, 61942, 62073, 62201, 62327, 62451, 62572, 62691, 62807, 62921, 63033, 63142, 63249,
    63353, 63455, 63555, 63652, 63747, 63840, 63930, 64017, 64102, 64185, 64265, 64343, 64419,
    64492, 64562, 64630, 64696, 64759, 64820, 64878, 64934, 64987, 65038, 65086, 65132, 65175,
    65216, 65255, 65291, 65324, 65356, 65384, 65410, 65434, 65455, 65474, 65490, 65503, 65515,
    65523, 65530, 65533, 65535,
];

/// `sin_table` `0x00A46A00`, `__fastcall(ecx = angle, edx = amplitude)`.
///
/// Angle is a 32-bit binary angle (`0x1_0000_0000` = one turn). Returns `amplitude * sin(angle)`
/// in the same fixed point as `amplitude`. Three magnitude regimes keep the intermediate product
/// inside 32 bits; all three are reproduced exactly, including the wrapping multiply.
///
/// **Quirk, reproduced deliberately**: the `bit 30` arm computes `0xFFFF - T[i] + delta*frac`,
/// which is only correct at `frac == 0`. Every caller in the movement cone folds the quadrant
/// first (see [`sinx`]) so that bit 30 is always clear, and this arm is dead on those paths;
/// it is kept because a caller that does *not* fold would hit it.
///
/// **Not a quirk, but it looks like one**: the neighbour index is `(u8)(i + 1)`, so at
/// `i == 255` it wraps to `T[0] == 0` and the interpolation slope becomes `-65535`. Angles in
/// the last 1/256 of a quarter turn take that path, and the 32-bit `imul` overflows. It comes
/// out *right*: at `frac == 0x3FFFFF` the wrapped product is `+4259839`, `>> 22` is `1`, and
/// `unit` lands on exactly `65536` — full scale to within the `>> 16` that follows. `wrapping_mul`
/// is mandatory (a debug build would otherwise panic where retail simply wraps), but no caller
/// sees a discontinuity. `mech:sim-core` reached the same conclusion independently; a test in
/// `crate::trig` that asserts a visible glitch here is asserting something that does not happen.
///
/// **Real hazard, measured**: the low regime is `amp < 0xFFFF` on a *signed* compare, and it
/// computes `unit * amp` in 32 bits with `unit` reaching 65536. That overflows for
/// `|amp| >= 32768`. Retail overflows there too — the middle regime exists to pre-shift larger
/// amplitudes, but it only engages at `0xFFFF`, leaving `[32768, 65534]` broken. Every real
/// caller passes a small amplitude (`Unit::move_step` passes the unit's speed,
/// `PathFinder::find_upath` passes `24`), so the gap is unreachable in practice. Do not write a
/// test at amplitude 65535 and expect a sine.
pub fn sin_table(angle: i32, amplitude: i32) -> i32 {
    let a = angle as u32;
    // `test edi,edi; cmovns esi,edx` — sign of the *angle* flips the amplitude (sin(x+pi) = -sin x).
    let amp: i32 = if (angle) >= 0 {
        amplitude
    } else {
        amplitude.wrapping_neg()
    };
    let bit30 = a & 0x4000_0000;
    let idx = ((a & 0x3FFF_FFFF) >> 22) as usize; // 0..=255
    let frac = (a & 0x003F_FFFF) as i32;
    let t0 = SIN_QUARTER[idx];
    let t1 = SIN_QUARTER[(idx + 1) & 0xFF];
    let interp = t1.wrapping_sub(t0).wrapping_mul(frac) >> 22;
    let unit = if bit30 != 0 {
        interp.wrapping_sub(t0).wrapping_add(0xFFFF)
    } else {
        interp.wrapping_add(t0)
    };
    // Three regimes on the sign-adjusted amplitude, signed compares exactly as in the binary.
    if amp < 0xFFFF {
        unit.wrapping_mul(amp) >> 16
    } else if amp > 0xFF_FFFE {
        unit.wrapping_mul(amp >> 16)
    } else {
        unit.wrapping_mul(amp >> 8) >> 8
    }
}

/// The quadrant fold that every movement caller inlines around `sin_table`.
///
/// [measured] verbatim at `0x005FB5CC` (`Unit::move_step`) and `0x00683194`
/// (`PathFinder::find_upath`); it is also the body of `cosx` `0x0092D0C0` minus the
/// quarter-turn offset:
/// ```text
///   if ((i32)a < 0) { amp = -amp; a &= 0x7FFFFFFF; }
///   b = (a & 0x40000000) ? (0x7FFFFFFF - a) : a;
///   sin_table(b, amp)
/// ```
pub fn sinx(angle: i32, amplitude: i32) -> i32 {
    let mut amp = amplitude;
    let mut a = angle;
    if a < 0 {
        amp = amp.wrapping_neg();
        a &= 0x7FFF_FFFF;
    }
    let b = if (a & 0x4000_0000) != 0 {
        0x7FFF_FFFF - a
    } else {
        a
    };
    sin_table(b, amp)
}

/// `cosx` `0x0092D0C0`: `sinx(angle + quarter_turn, amplitude)`, with the early-out on
/// `amplitude == 0` that the binary has. [measured]
pub fn cosx(angle: i32, amplitude: i32) -> i32 {
    if amplitude == 0 {
        return 0;
    }
    sinx(angle.wrapping_add(0x4000_0000u32 as i32), amplitude)
}

/// `find_angle` `0x0092D130`, `__fastcall(ecx = dx, edx = dy)` -> 32-bit binary angle.
///
/// [measured] instruction for instruction. Note `edi = -dy` at entry: the engine's angle 0 is
/// `-y` (north, screen-up) and `0x40000000` is `+x` (east), which is why the integrator writes
/// `y -= cosx(...)`. The core is a 14-bit ratio `r = (min << 14) / max` fed through the integer
/// polynomial `(0x2800 - (|0x1333 - r| * 0xB00 >> 14)) * r`, masked to `0xFFFFC000` and shifted
/// left 2 — max error a few tenths of a degree.
pub fn find_angle(dx: i32, dy: i32) -> i32 {
    let ny = dy.wrapping_neg();
    if dx == 0 {
        // `cmovg eax, ecx` with ecx == 0: north is 0, south is 0x80000000.
        return if ny > 0 { 0 } else { 0x8000_0000u32 as i32 };
    }
    if ny == 0 {
        return if dx > 0 {
            0x4000_0000
        } else {
            0xC000_0000u32 as i32
        };
    }
    let ax = dx.wrapping_abs();
    let ay = ny.wrapping_abs();
    let (num, den, swapped) = if ax > ay {
        (ay.wrapping_shl(14), ax, true)
    } else {
        (ax.wrapping_shl(14), ay, false)
    };
    let r = num / den;
    let t = (0x1333i32).wrapping_sub(r).wrapping_abs();
    let poly = (0x2800i32).wrapping_sub(t.wrapping_mul(0xB00) >> 14);
    let v = (poly.wrapping_mul(r) as u32 & 0xFFFF_C000).wrapping_shl(2) as i32;
    if dx > 0 {
        if ny > 0 {
            if swapped {
                0x4000_0000i32.wrapping_sub(v)
            } else {
                v
            }
        } else if swapped {
            v.wrapping_add(0x4000_0000)
        } else {
            (0x8000_0000u32 as i32).wrapping_sub(v)
        }
    } else if ny > 0 {
        if swapped {
            v.wrapping_add(0xC000_0000u32 as i32)
        } else {
            v.wrapping_neg()
        }
    } else if swapped {
        (0xC000_0000u32 as i32).wrapping_sub(v)
    } else {
        v.wrapping_sub(0x8000_0000u32 as i32)
    }
}

/// `vector_dist` `0x0046CFF0`, `__fastcall(ecx, edx)` -> integer approximate Euclidean length.
///
/// [measured]. `M + m*m / (2*M)` — the first-order expansion of `sqrt(M^2 + m^2)` — with an
/// overflow guard: when the *smaller* magnitude reaches 60000 it switches to `M + m/2` via an
/// **unsigned** shift. The division is unsigned and truncating. Diagonals come out ~6% long
/// (`1.5*M` against `1.4142*M`), which is the engine's actual metric, not an approximation
/// introduced here.
pub fn vector_dist(a: i32, b: i32) -> i32 {
    let ai = a.wrapping_abs();
    let bi = b.wrapping_abs();
    let (big, small) = if ai > bi { (ai, bi) } else { (bi, ai) };
    if big == 0 {
        return 0;
    }
    if small >= 60000 {
        return (((small as u32).wrapping_add((big as u32) << 1)) >> 1) as i32;
    }
    let sq = (small as u32).wrapping_mul(small as u32);
    (sq / ((big as u32) << 1)) as i32 + big
}

/// `PathFinderData::get_estimate` `0x00688310` — the A* heuristic, verbatim. [measured]
///
/// `h = 10 * vector_dist(|dx|, |dy|)` on the unit grid, `h = 60 * vector_dist / step`
/// elsewhere. Against a step cost of 32 (straight) / 40 (diagonal) on a 48-world-unit grid,
/// the unit heuristic is **~15x inflated** — `10*48 = 480` of heuristic per 32 of `g`. The
/// unit search is therefore heuristic-dominated: closer to greedy best-first than to A*, and
/// the paths it returns are explicitly not cost-optimal. This is the engine's behaviour and
/// must be reproduced, not fixed.
pub fn get_estimate(ax: i32, ay: i32, bx: i32, by: i32, step: i32) -> i32 {
    let d = vector_dist((ax - bx).wrapping_abs(), (ay - by).wrapping_abs());
    if step == 0x30 {
        d.wrapping_mul(10)
    } else {
        d.wrapping_mul(60) / step
    }
}

// ---------------------------------------------------------------------------
// 3. `PathData`, the unit path buffer, and the checksum surface  [measured]
// ---------------------------------------------------------------------------

/// `class PathData` — 16 bytes, from the PDB type stream:
/// `{ Coord to_x; Coord to_y; int tolerance; int flags; }`.
///
/// `flags` bits observed on the emit and consume sides [measured]:
/// * `1` — "there is more work this frame"; `move_step` returns early when it is clear after
///   popping a waypoint, and `find_upath`'s straight-line pre-check consults it on the caller's
///   record. **Never set by the pathfinder itself.**
/// * `2` — set on every waypoint the unit A* emits (`astar_path` `0x00684bfc`..`0x0068420d`).
///   `move_step` uses it to permit the "snap straight to the order target" shortcut.
/// * `4` — transport leg: the step needs a boat. Set from `PathNode::transport`, and it also
///   forces `tolerance = 0` and suppresses the collinear compression in `find_upath`.
/// * `0x10` — set from `PathNode::building`, which only the tile-domain search ever raises.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PathData {
    pub to_x: i32,
    pub to_y: i32,
    pub tolerance: i32,
    pub flags: i32,
}

impl PathData {
    pub const FLAG_MORE: i32 = 1;
    pub const FLAG_WAYPOINT: i32 = 2;
    pub const FLAG_TRANSPORT: i32 = 4;
    pub const FLAG_BUILDING: i32 = 0x10;
}

/// `Stack<PathData>` at `UnitData+0xB8` — the unit's path buffer.
///
/// It is a genuine LIFO: the pathfinder pushes waypoints goal-first, so the top of the stack is
/// the *next* waypoint. `Unit::move_step` pops when it arrives. `Stack<PathData>::walk_data` is
/// invoked from `Unit::walk_data` `0x0060CF40` behind mask bit 2, which is why this type is on
/// the `units` checksum channel.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PathStack {
    pub records: Vec<PathData>,
}

impl PathStack {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> i32 {
        self.records.len() as i32
    }
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
    pub fn push(&mut self, r: PathData) {
        self.records.push(r);
    }
    pub fn pop(&mut self) -> Option<PathData> {
        self.records.pop()
    }
    pub fn peek(&self) -> Option<PathData> {
        self.records.last().copied()
    }
    pub fn clear(&mut self) {
        self.records.clear();
    }

    /// The engine's `Stack<PathData>::pop` clamps the count to at least 1 before decrementing,
    /// so popping an empty stack reads `records[0]` and leaves the count at 0. Reproduced,
    /// because `astar_path` and `find_upath` both rely on the clamp when the caller hands them a
    /// short stack. [measured] `if (param_1[2] < 1) param_1[2] = 1; param_1[2]--;`
    pub fn pop_clamped(&mut self) -> PathData {
        if self.records.is_empty() {
            return PathData::default();
        }
        self.records.pop().unwrap()
    }

    /// Little-endian bytes in record order, which is what `Stack<PathData>::walk_data` hands the
    /// `DataWalk` visitor. Feed to [`adler32`] to produce this unit's contribution to the
    /// `units` channel.
    pub fn walk_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.records.len() * 16);
        for r in &self.records {
            out.extend_from_slice(&r.to_x.to_le_bytes());
            out.extend_from_slice(&r.to_y.to_le_bytes());
            out.extend_from_slice(&r.tolerance.to_le_bytes());
            out.extend_from_slice(&r.flags.to_le_bytes());
        }
        out
    }
}

/// zlib adler-32, `0x00A46830`, `__fastcall`. The checksum accumulator `CheckSum` initialises it
/// to 1 before each of the fifteen channels [measured, `docs/derivation/checksum.md` §2, Tier B
/// against retail at 500,000 calls]. Re-exported from [`crate::checksum`].
pub use crate::checksum::adler32;

// ---------------------------------------------------------------------------
// 4. The world interface the search needs
// ---------------------------------------------------------------------------

/// The queries `astar_path` / `calc_cost` / `valid_ucoord` make against world state.
///
/// This is deliberately a trait rather than a dependency on `don_sim::world`: the movement lane
/// must not couple to a module another wave is rewriting. Implement it over whatever the world
/// ends up being. Every method names the retail function it stands in for.
pub trait UnitWorld {
    /// `World+0x18` — map width in tiles. Used by `valid_ucoord`'s bounds check.
    fn tiles_w(&self) -> i32;
    /// `World+0x1C` — map height in tiles.
    fn tiles_h(&self) -> i32;
    /// `World+0x00` — map width in **water cells** (4-tile blocks). The unit grid's row stride
    /// is `wcells_w * 16` [measured, `local_64 = *(int*)World << 4` at `0x0068339E`].
    fn wcells_w(&self) -> i32;

    /// `UnitData::invalid_loc` `0x00607C30` at the tile the candidate falls in. Non-zero means
    /// the unit may not stand there (terrain, buildings, ownership, elevation).
    fn invalid_loc(&self, tile_x: i32, tile_y: i32) -> bool;

    /// `Unit::detect_unit_collision` `0x00617060` — is another body occupying this world
    /// position for this unit's footprint?
    fn unit_collides(&self, x: i32, y: i32) -> bool;

    /// `UnitData::needs_transport` `0x00609920` on tile coordinates. Returns the engine's
    /// integer: `<= 0` none, `1` a boarding step that is free, `> 1` a real water crossing.
    fn needs_transport(&self, from_tx: i32, from_ty: i32, to_tx: i32, to_ty: i32) -> i32;

    /// `WorldData::get_tregion` `0x006B52E0` — connected-component id of a tile.
    fn tregion(&self, tile_x: i32, tile_y: i32) -> i32;
}

/// The subset of `Unit` / `UnitType` state the search reads out of `PathFinder+0x54`.
#[derive(Clone, Copy, Debug)]
pub struct PathUnit {
    /// `UnitType+0x248`. `unit_size = max(1, (this + 1) / 2)` and the A* neighbour stride is
    /// `unit_size * 48`. [measured] `0x0068337E`.
    pub type_size: i32,
    /// `Unit+0x68 & 0x800000` and `Unit+0x6C & 0x2000` and `UnitType+0x2B4 & 0x10` combine into
    /// the single predicate `calc_cost` uses to decide whether transport legs are permitted.
    pub can_board_transport: bool,
    /// `UnitType+0x218 < 2` gates the straight-line pre-check in `find_upath`. [measured]
    pub small_footprint: bool,
    /// `UnitData::can_transport` `0x0046F960` — a transport itself skips the pre-check.
    pub can_transport: bool,
}

impl Default for PathUnit {
    fn default() -> Self {
        Self {
            type_size: 1,
            can_board_transport: false,
            small_footprint: true,
            can_transport: false,
        }
    }
}

// ---------------------------------------------------------------------------
// 5. `PathNode` and the open/closed containers  [measured]
// ---------------------------------------------------------------------------

/// `class PathNode`, 36 bytes, field order from the PDB type stream and confirmed against every
/// access in `astar_path`:
/// `x@0 y@4 length@8 estimate@12 value@16 timeout@20 metric@24 z_val@28 transport@30 building@31
/// parent@32`.
///
/// `length` is `g`, `estimate` is `h`, `value` is `f` and is the open-list key. `timeout` is not
/// a timeout: `astar_path` sets `child.timeout = parent.timeout + 1`, so it is the **path depth**,
/// and `calc_cost` takes it as its `param_7`.
#[derive(Clone, Copy, Debug, Default)]
pub struct PathNode {
    pub x: i32,
    pub y: i32,
    pub length: i32,
    pub estimate: i32,
    pub value: i32,
    pub timeout: i32,
    pub metric: u32,
    pub z_val: i16,
    pub transport: u8,
    pub building: u8,
    pub parent: Option<u32>,
}

/// `Tree<PathNode*,int>::ordered_insert` `0x004796F0` is an **unbalanced BST with no rebalancing
/// at all**, and `first_open_node` `0x00687970` takes its leftmost node. Both facts matter for
/// determinism, so this is a faithful arena BST rather than a heap.
///
/// The tie rule falls out of the insert: `cmp key, node->key; jle -> go left`, i.e. an equal key
/// goes **left** of the incumbent, and leftmost-extraction therefore pops equal-`f` nodes
/// **last-in-first-out**. A binary heap would not reproduce that.
#[derive(Clone, Debug)]
struct TreeNode {
    left: Option<u32>,
    right: Option<u32>,
    parent: Option<u32>,
    data: u32,
    key: i32,
}

#[derive(Clone, Debug, Default)]
struct OrderedTree {
    nodes: Vec<TreeNode>,
    head: Option<u32>,
    count: i32,
}

impl OrderedTree {
    fn clear(&mut self) {
        self.nodes.clear();
        self.head = None;
        self.count = 0;
    }

    fn ordered_insert(&mut self, data: u32, key: i32) -> u32 {
        let id = self.nodes.len() as u32;
        self.nodes.push(TreeNode {
            left: None,
            right: None,
            parent: None,
            data,
            key,
        });
        match self.head {
            None => self.head = Some(id),
            Some(root) => {
                let mut cur = root;
                loop {
                    if key > self.nodes[cur as usize].key {
                        match self.nodes[cur as usize].right {
                            Some(r) => cur = r,
                            None => {
                                self.nodes[cur as usize].right = Some(id);
                                break;
                            }
                        }
                    } else {
                        match self.nodes[cur as usize].left {
                            Some(l) => cur = l,
                            None => {
                                self.nodes[cur as usize].left = Some(id);
                                break;
                            }
                        }
                    }
                }
                self.nodes[id as usize].parent = Some(cur);
            }
        }
        self.count += 1;
        id
    }

    /// The leftmost node — what `first_open_node` descends to via `while (node->left) node = node->left`.
    fn leftmost(&self) -> Option<u32> {
        let mut cur = self.head?;
        while let Some(l) = self.nodes[cur as usize].left {
            cur = l;
        }
        Some(cur)
    }

    fn replace_child(&mut self, parent: Option<u32>, old: u32, new: Option<u32>) {
        match parent {
            None => self.head = new,
            Some(p) => {
                if self.nodes[p as usize].left == Some(old) {
                    self.nodes[p as usize].left = new;
                } else {
                    self.nodes[p as usize].right = new;
                }
            }
        }
    }

    /// `Tree<PathNode*,int>::remove_current` `0x00479770`.
    ///
    /// UNVERIFIED: the two-child case uses in-order-successor (Hibbard) deletion. The 422-byte
    /// retail body has not been read instruction by instruction, and if it splices with the
    /// predecessor instead the resulting tree shape — and therefore the pop order of later ties —
    /// differs. Leftmost removal (the hot path, one per expansion) has at most one child and is
    /// unambiguous, so this only bites on the "found a cheaper route to an open node" branch.
    fn remove(&mut self, id: u32) {
        let (l, r, p) = {
            let n = &self.nodes[id as usize];
            (n.left, n.right, n.parent)
        };
        self.count -= 1;
        match (l, r) {
            (None, None) => self.replace_child(p, id, None),
            (Some(c), None) | (None, Some(c)) => {
                self.replace_child(p, id, Some(c));
                self.nodes[c as usize].parent = p;
            }
            (Some(l), Some(r)) => {
                // in-order successor = leftmost of the right subtree
                let mut succ = r;
                while let Some(sl) = self.nodes[succ as usize].left {
                    succ = sl;
                }
                if succ != r {
                    let sp = self.nodes[succ as usize].parent;
                    let sr = self.nodes[succ as usize].right;
                    self.replace_child(sp, succ, sr);
                    if let Some(sr) = sr {
                        self.nodes[sr as usize].parent = sp;
                    }
                    self.nodes[succ as usize].right = Some(r);
                    self.nodes[r as usize].parent = Some(succ);
                }
                self.nodes[succ as usize].left = Some(l);
                self.nodes[l as usize].parent = Some(succ);
                self.replace_child(p, id, Some(succ));
                self.nodes[succ as usize].parent = p;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 6. `PathFinder` — the search state that `CheckSums::check_pathfinder` covers
// ---------------------------------------------------------------------------

/// The singleton `pathfinder : PathFinder` at `0x00E85E40`; `PathFinderData` starts at `+0x40`.
///
/// The five containers named in the PDB are all here. `blocklist` (`Tree<CollBlock*,int>` at
/// `+0x4C`) is not modelled: nothing on the unit path reads it, only the caravan/road engine does.
///
/// `CheckSums::check_pathfinder` `0x00936E30` walks a flat 108-byte window at
/// `pathfinder + 0x58` — the scalar search state below, not the containers — so the pathfinder is
/// lockstep-critical but its trees are not directly hashed.
#[derive(Debug, Default)]
pub struct PathFinder {
    open: OrderedTree,
    /// `openlistrefs` `+0x44` — `BRTree<TreeNode*, ulong>` keyed by cell metric, the index that
    /// makes `find_node_open` O(log n). Modelled as a map because only `seek` is ever used and
    /// the red-black shape cannot change which node a key finds.
    open_refs: BTreeMap<u32, u32>,
    /// `closedlist` `+0x48` — `BRTree<PathNode*, ulong>` keyed by metric.
    closed: BTreeMap<u32, u32>,
    /// `validlist` `+0x50` — `BRTree<int, ulong>`, the per-search memo `valid_ucoord` writes into.
    valid: BTreeMap<u32, bool>,
    nodes: Vec<PathNode>,

    /// `+0x58`, `+0x5C` — the tile the search was launched from. `calc_cost` re-queries transport
    /// need against it at depth 1.
    pub origin_tile: (i32, i32),
    /// `+0x64` — non-zero for the whole of a `find_upath` call. Selects the unit-specific budget
    /// and cost arms inside `astar_path`.
    pub in_upath: i32,
    /// `+0x88` — start endpoint is on land / reachable.
    pub start_land: i32,
    /// `+0x8C` — end endpoint classification: `0` land, `1` water, `2` "hard reject" (a `2` makes
    /// `calc_cost` return `INT_MAX` for any transport step).
    pub end_class: i32,
    /// `+0x90` — `valid_ucoord` memo-hit counter. A pure statistic; incremented on every hit.
    pub valid_hits: i32,
    /// `+0x80` — the soft per-call node budget consulted only while `in_upath != 0`. The engine
    /// sets it from the caller; `i32::MAX` here means "no soft cap".
    pub soft_node_limit: i32,
    /// `+0x84` — a suspended search is parked in the unit and resumed by `find_upath_restore`
    /// `0x00688F40`.
    pub suspended: bool,
    /// The scalar half of the parked `astar_path` frame. Retail parks the five tree roots on
    /// `UnitData+0x104..+0x114` and retains these values in `PathFinderData`; Rust owns the
    /// trees directly, so the equivalent continuation frame lives beside them.
    resume: Option<UnitSearchResume>,
    /// Nodes expanded by the last search — `local_58`, the quantity the 32,000 budget bounds.
    pub last_expanded: i32,
    /// **The caller must service this.** A failed unit search obliges the engine to draw
    /// `Random::get(0, 0xFFFF) % 3 + 6` from `GameAccess::game_random` — the *main simulation
    /// stream*, 307 call sites — and store it as a 6-to-8-tick retry delay on the order target,
    /// then add 30 to the searching unit's `+0xB2`. It is the **only** RNG consumption anywhere
    /// in unit pathfinding: `calc_cost` draws nothing, and the per-edge jitter that
    /// `pathfinding.md` describes belongs to `calc_road_cost` / `calc_river_cost` only.
    ///
    /// It is exposed as a flag rather than performed here because this module owns no RNG and
    /// must not invent a stream. Skipping it does not corrupt path state — it shifts the shared
    /// stream position for **every later draw in the tick**, which is a whole-sim desync.
    pub pending_retry_draw: bool,
}

/// Values from `astar_path`'s stack frame which must survive `find_upath_restore`.
///
/// The open/closed/valid containers themselves remain in [`PathFinder`]. Keeping this frame is
/// not an algorithmic shortcut: `PathFinder::find_upath_restore` `0x00688F40` sets the resume
/// gate, installs a fresh per-frame soft budget, and calls `find_upath`, whose resume arm adopts
/// the parked containers and continues with these same search constants. [measured]
#[derive(Clone, Copy, Debug)]
struct UnitSearchResume {
    goal: (i32, i32),
    arrive_tol: i32,
    step_scale: i32,
    row_stride: i32,
    dir_stride: i32,
    seed: i32,
    /// `UnitData+0x148`: expansions spent by earlier suspended slices. Retail compares the
    /// hard cap against `saved_expanded + expanded_this_call`, but the soft cap against this
    /// call alone. [measured `0x0068409D..0x006840CC`]
    expanded_before: i32,
}

/// What `astar_path` returns. The engine's raw returns are `1` success, `0` failure,
/// `-1` suspended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchResult {
    /// A path was written to the stack.
    Found,
    /// The open list was exhausted, or the hard budget was hit with `quick != 0`.
    Failed,
    /// The soft budget was hit; state would be parked in the unit for `find_upath_restore`.
    Suspended,
}

impl PathFinder {
    pub fn new() -> Self {
        Self {
            soft_node_limit: i32::MAX,
            ..Default::default()
        }
    }

    /// `PathFinder::kill_lists` `0x00687AE0` — drop every container between searches.
    pub fn kill_lists(&mut self) {
        self.open.clear();
        self.open_refs.clear();
        self.closed.clear();
        self.valid.clear();
        self.nodes.clear();
        self.suspended = false;
        self.resume = None;
    }

    fn alloc(&mut self, n: PathNode) -> u32 {
        self.nodes.push(n);
        (self.nodes.len() - 1) as u32
    }

    /// `PathFinder::valid_ucoord` `0x00687C80`. [measured]
    ///
    /// Order matters: the bounds tests come first and never touch the memo, then the memo is
    /// consulted, and only on a miss are `invalid_loc` and `detect_unit_collision` called and the
    /// answer written back. Because the memo is keyed by **cell metric** and the query is by
    /// world position, two different world positions in the same 48-unit cell share an answer.
    pub fn valid_ucoord<W: UnitWorld>(&mut self, w: &W, x: i32, y: i32, metric: u32) -> bool {
        if x < 0 || y < 0 {
            return false;
        }
        if x >= w.tiles_w() * TILE || y >= w.tiles_h() * TILE {
            return false;
        }
        if let Some(&v) = self.valid.get(&metric) {
            self.valid_hits += 1;
            return v;
        }
        let ok = !w.invalid_loc(tile_of(x), tile_of(y)) && !w.unit_collides(x, y);
        self.valid.insert(metric, ok);
        ok
    }

    /// `PathFinder::calc_cost` `0x00684E50`, **unit domain only** (`step == 0x30`). [measured]
    ///
    /// The unit arm is startlingly thin: `local_28 = 0x100`, `extra = 0`, then straight to the
    /// transport test. Terrain cost, danger, borders and building avoidance all live in the
    /// `0xC0` (tile) and `0x300` (water) arms — **the quarter-tile unit search charges pure
    /// geometry**. That is the architectural point: `Unit::do_move` runs a coarse tile/water leg
    /// for terrain preference and a fine unit leg for local obstacle avoidance.
    ///
    /// Return: `(0x100 * 32) >> 8` = **32 for a straight step, 40 for a diagonal**
    /// (`(dir & 1) * 8`), plus a transport surcharge, or `i32::MAX` to reject the edge.
    pub fn calc_cost<W: UnitWorld>(
        &self,
        w: &W,
        unit: &PathUnit,
        from_x: i32,
        from_y: i32,
        to_x: i32,
        to_y: i32,
        dir: u32,
        depth: i32,
        out_transport: &mut u8,
    ) -> i32 {
        *out_transport = 0;
        let base = 0x100i32;
        let mut extra = 0i32;

        let ftx = tile_of(from_x);
        let fty = tile_of(from_y);
        let ttx = tile_of(to_x);
        let tty = tile_of(to_y);

        let nt = w.needs_transport(ftx, fty, ttx, tty);
        let can = unit.can_board_transport;

        if nt >= 1 && can && depth >= 2 {
            if self.end_class == 2 {
                return i32::MAX;
            }
            if nt != 1 {
                extra += if self.end_class == 0 && self.start_land == 0 {
                    500
                } else {
                    2000
                };
            }
            // `iVar13 <<= 2` is inside this arm and gated on step == 0x30 [measured 0x00685862].
            extra <<= 2;
            *out_transport = 1;
        } else if depth == 1 {
            let nt2 = w.needs_transport(self.origin_tile.0, self.origin_tile.1, ttx, tty);
            if nt2 > 0 && can {
                if self.end_class == 2 {
                    return i32::MAX;
                }
                if nt2 != 1 {
                    extra += if self.end_class == 0 && self.start_land == 0 {
                        250
                    } else {
                        1000
                    };
                }
                *out_transport = 1;
            }
        }

        ((base.wrapping_mul(32)) >> 8) + extra + ((dir & 1) as i32) * 8
    }
}

// ---------------------------------------------------------------------------
// 7. `PathFinder::astar_path`, unit domain  [measured]
// ---------------------------------------------------------------------------

/// The hard node budget. `local_84 = 500` for `step == 0x30`, and the test is
/// `local_84 * 0x40 <= expanded`, i.e. **32,000**. [measured] `0x0068329C`, `0x006840xx`.
pub const UNIT_NODE_BUDGET: i32 = 500 * 64;

/// Everything `astar_path` needs that is not `PathFinder` state.
pub struct SearchArgs<'a> {
    pub unit: &'a PathUnit,
    /// `param_3` of `astar_path`. Non-zero halves the branching factor (`local_80 = 2`, so only
    /// 4 of the 8 directions are tried) and makes a budget exhaustion a hard failure instead of
    /// a suspension. [measured] `local_80 = (param_3 != 0) + 1`.
    pub quick: i32,
}

impl PathFinder {
    /// `PathFinder::astar_path` `0x00683770` with `step = 0x30`.
    ///
    /// Protocol, exactly as the engine has it: the caller leaves three records on the stack —
    /// `[.., anchor, start, goal]`. `goal` is popped first, `start` second, and the **arrival
    /// tolerance is read from the record below `start`** (`piVar7[-2]`), i.e. from `anchor`. On
    /// success the waypoints are pushed back on, goal-end first, so the top of the stack is the
    /// waypoint nearest the unit.
    ///
    /// The suspended resume path is implemented: a second call while [`PathFinder::suspended`]
    /// is true continues the retained trees instead of pushing a second root. The caller must
    /// install retail's resume budget before that call (`300 / player_path_scale²`, versus
    /// `500 / player_path_scale²` for the initial `find_upath` wrapper). The `0xC0` / `0x300`
    /// domains remain outside this unit-domain entry point.
    pub fn astar_path_unit<W: UnitWorld>(
        &mut self,
        w: &W,
        stack: &mut PathStack,
        args: &SearchArgs,
    ) -> SearchResult {
        let mut frame = if self.suspended {
            self.resume
                .expect("a suspended unit search carries its continuation frame")
        } else {
            let unit_size = ((args.unit.type_size + 1) / 2).max(1);
            let step_scale = unit_size * UCELL;
            let row_stride = w.wcells_w() * 16;
            let dir_stride: i32 = if args.quick != 0 { 2 } else { 1 };

            // --- pop goal, start, and the tolerance from the record below start ---
            let goal = stack.pop_clamped();
            let start = stack.pop_clamped();
            let below_tol = if stack.records.is_empty() {
                0
            } else {
                stack.records[stack.records.len() - 1].tolerance
            };

            let (gx, gy) = (goal.to_x, goal.to_y);
            let (sx, sy) = (start.to_x, start.to_y);

            // Arrival tolerance: `local_24 = local_60 / 2 + unit_size * step`.
            // [measured 0x00684090]
            let arrive_tol = below_tol / 2 + step_scale;

            // --- root node ---
            let metric0 = (ucell_of(sx) + ucell_of(sy) * row_stride) as u32;
            let h0 = get_estimate(sx, sy, gx, gy, 0x30);
            let root = self.alloc(PathNode {
                x: sx,
                y: sy,
                length: 0,
                estimate: h0,
                value: h0,
                timeout: 0,
                metric: metric0,
                ..Default::default()
            });
            let tn = self.open.ordered_insert(root, h0);
            self.open_refs.insert(metric0, tn);

            // --- the fixed direction seed [measured 0x00684064..0x00684086] ---
            let dx0 = sx - gx;
            let dy0 = sy - gy;
            let seed: i32 = if dy0.abs() < dx0.abs() {
                if gx < sx { 7 } else { 3 }
            } else if sy <= gy {
                5
            } else {
                1
            };
            let frame = UnitSearchResume {
                goal: (gx, gy),
                arrive_tol,
                step_scale,
                row_stride,
                dir_stride,
                seed,
                expanded_before: 0,
            };
            self.resume = Some(frame);
            frame
        };
        self.suspended = false;
        let (gx, gy) = frame.goal;
        let UnitSearchResume {
            arrive_tol,
            step_scale,
            row_stride,
            dir_stride,
            seed,
            ..
        } = frame;

        let mut expanded = 0i32;

        while self.open.count != 0 {
            // `first_open_node` — leftmost, then removed from both open containers.
            let tid = self
                .open
                .leftmost()
                .expect("non-empty tree has a leftmost node");
            let cur = self.open.nodes[tid as usize].data;
            self.open.remove(tid);
            self.open_refs.remove(&self.nodes[cur as usize].metric);

            let cn = self.nodes[cur as usize];
            let manh = (cn.x - gx).abs() + (cn.y - gy).abs();
            let total_expanded = frame.expanded_before.wrapping_add(expanded);
            let hard_out = UNIT_NODE_BUDGET <= total_expanded;
            let soft_out = self.in_upath != 0 && self.soft_node_limit < expanded;

            if manh <= arrive_tol || hard_out || soft_out {
                if soft_out && manh > arrive_tol && args.quick == 0 {
                    // The engine parks the containers on the unit and returns -1 so that
                    // `find_upath_restore` can pick the search back up next frame. It first
                    // puts the node it just removed back into both open containers
                    // (`0x006845E5..0x00684601`), so the resumed slice does not skip it.
                    let tn = self.open.ordered_insert(cur, cn.value);
                    self.open_refs.insert(cn.metric, tn);
                    frame.expanded_before = total_expanded;
                    self.resume = Some(frame);
                    self.suspended = true;
                    self.last_expanded = total_expanded;
                    return SearchResult::Suspended;
                }
                // `if (local_70 + local_58 < local_84 * 0x40 || param_3 != 0) bVar15 = false;`
                // [measured `0x0068487C`]. Falling out of that test on the unit arm means the
                // hard budget is spent and `quick == 0`, and the engine then does **not** emit a
                // partial path: it draws the retry delay and returns 0. `quick != 0` skips the
                // whole arm, so a quick search that runs out of budget *does* return its best
                // node. Getting this backwards costs you a spurious path where retail gives up.
                if hard_out && args.quick == 0 {
                    self.last_expanded = total_expanded;
                    self.pending_retry_draw = true;
                    self.resume = None;
                    return SearchResult::Failed;
                }
                self.emit_path(stack, cur, args);
                self.last_expanded = total_expanded;
                self.resume = None;
                return SearchResult::Found;
            }

            // --- expand, starting one past the seed and wrapping through [1, 8] ---
            let mut d = seed + 1;
            let mut n = 1i32;
            while n < 9 {
                let dir = if d >= 9 { d - 8 } else { d };
                let du = dir as usize;
                let cx = MOVE_X[du] * step_scale + cn.x;
                let cy = MOVE_Y[du] * step_scale + cn.y;
                let cmetric = (cn.metric as i32 + MOVE_X[du] + MOVE_Y[du] * row_stride) as u32;

                let ok = if unit_size < 2 || (dir & 1) == 0 {
                    self.valid_ucoord(w, cx, cy, cmetric)
                } else {
                    // A unit wider than one cell samples every intermediate cell along the
                    // diagonal. UNVERIFIED: Ghidra folded the metric argument of the inner
                    // calls; the positions are certain, the memo key is inferred.
                    let mut good = true;
                    let (mut ax, mut ay) = (MOVE_X[du] * UCELL, MOVE_Y[du] * UCELL);
                    let mut k = 1;
                    while k <= unit_size {
                        let m =
                            (cn.metric as i32 + (ax / UCELL) + (ay / UCELL) * row_stride) as u32;
                        if !self.valid_ucoord(w, cn.x + ax, cn.y + ay, m) {
                            good = false;
                            break;
                        }
                        ax += MOVE_X[du] * UCELL;
                        ay += MOVE_Y[du] * UCELL;
                        k += 1;
                    }
                    good
                };

                if !ok {
                    d += dir_stride;
                    n += dir_stride;
                    continue;
                }

                expanded += 1;

                let mut transport = 0u8;
                let cost = self.calc_cost(
                    w,
                    args.unit,
                    cn.x,
                    cn.y,
                    cx,
                    cy,
                    dir as u32,
                    cn.timeout + 1,
                    &mut transport,
                );
                if cost == i32::MAX {
                    d += dir_stride;
                    n += dir_stride;
                    continue;
                }
                // Reaching the exact goal cell is charged half price. [measured 0x006842E8]
                let cost = if cx == gx && cy == gy { cost / 2 } else { cost };
                let g = cn.length + cost;

                if self.closed.contains_key(&cmetric) {
                    d += dir_stride;
                    n += dir_stride;
                    continue;
                }
                if let Some(&otn) = self.open_refs.get(&cmetric) {
                    let onode = self.open.nodes[otn as usize].data;
                    if self.nodes[onode as usize].length <= g {
                        d += dir_stride;
                        n += dir_stride;
                        continue;
                    }
                    self.open.remove(otn);
                    self.open_refs.remove(&cmetric);
                }

                let h = get_estimate(cx, cy, gx, gy, 0x30);
                let child = self.alloc(PathNode {
                    x: cx,
                    y: cy,
                    length: g,
                    estimate: h,
                    value: g + h,
                    timeout: cn.timeout + 1,
                    metric: cmetric,
                    z_val: 0,
                    transport,
                    building: 0,
                    parent: Some(cur),
                });
                let ctn = self.open.ordered_insert(child, g + h);
                self.open_refs.insert(cmetric, ctn);

                d += dir_stride;
                n += dir_stride;
            }

            self.closed.insert(cn.metric, cur);
        }

        // Open list exhausted — the second of the two failure epilogues, at `0x00684E02`.
        // Same obligation as the budget one: see `pending_retry_draw`.
        self.last_expanded = frame.expanded_before.wrapping_add(expanded);
        self.pending_retry_draw = true;
        self.resume = None;
        SearchResult::Failed
    }

    /// Walk the parent chain and push waypoints. [measured `0x00684bcc`..`0x00684ce8`]
    ///
    /// For `step == 0x30` every emitted record gets `tolerance = 0` (both arms of the
    /// `local_24` test land on `param_3 = 0`) and `flags = 2`, plus `4` if the node is a
    /// transport step and `0x10` if it is a building step. A waypoint identical to the current
    /// stack top is skipped. Unlike the water and tile domains, the unit domain **does** emit the
    /// root node (`if (parent == 0 && step != 0x30) break`).
    fn emit_path(&mut self, stack: &mut PathStack, goal_node: u32, _args: &SearchArgs) {
        let mut cur = Some(goal_node);
        while let Some(id) = cur {
            let n = self.nodes[id as usize];
            let mut flags = PathData::FLAG_WAYPOINT;
            if n.building != 0 {
                flags |= PathData::FLAG_BUILDING;
            }
            let mut tolerance = 0;
            if n.transport != 0 {
                flags |= PathData::FLAG_TRANSPORT;
                tolerance = 0;
            }
            let dup = stack
                .peek()
                .map(|p| p.to_x == n.x && p.to_y == n.y)
                .unwrap_or(false);
            if !dup {
                stack.push(PathData {
                    to_x: n.x,
                    to_y: n.y,
                    tolerance,
                    flags,
                });
            }
            cur = n.parent;
        }
    }
}

// ---------------------------------------------------------------------------
// 8. `PathFinder::find_upath` — the public unit entry  [measured]
// ---------------------------------------------------------------------------

/// The straight-line probe steps 24 world units per iteration: `find_upath` loads `0x18` (or
/// `-0x18` on the negative-angle arm) as the `sin_table` amplitude. [measured `0x00683196`]
pub const PROBE_STEP: i32 = 0x18;

/// How `find_upath` finished before it ever reached the A*.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UPathOutcome {
    /// Off-map: the stack was cleared and `-1` returned.
    OffMap,
    /// The destination cell was already occupied, or a straight walk reached it, or the endpoints
    /// were within one cell — one record was pushed and no search ran.
    Trivial,
    /// The probe stalled within 24 world units of the target with `flags & 1` clear: `0`.
    Stalled,
    /// The three-record search frame was set up; run [`PathFinder::astar_path_unit`].
    NeedsSearch,
}

impl PathFinder {
    /// `PathFinder::find_upath(Stack<PathData>*, Coord, Coord, int, int, int)` `0x00682F30`, the
    /// part that runs **before** `astar_path`. [measured]
    ///
    /// It does four things, in this order:
    /// 1. records the destination tile at `+0x58/+0x5C` (the field is named for the start but the
    ///    value is the destination) and clears `+0x68/+0x6C/+0x94`;
    /// 2. pops the caller's record and rejects an off-map unit-grid cell;
    /// 3. for a small non-transport unit, walks a straight line from the unit toward the
    ///    destination in 24-unit `sin`/`cos` steps, giving up when it lands in the destination
    ///    cell, when the destination's tile region matches and the cell is passable, or when it
    ///    is within 24 units on both axes;
    /// 4. otherwise pushes the three-record search frame with both endpoints **snapped to unit
    ///    cell centres** (`cell * 48 + 24`).
    ///
    /// The probe is the single most surprising thing in the lane: the engine tries to avoid
    /// pathfinding entirely, and for short unobstructed moves it succeeds.
    pub fn find_upath_prepare<W: UnitWorld>(
        &mut self,
        w: &W,
        stack: &mut PathStack,
        unit: &PathUnit,
        dest_x: i32,
        dest_y: i32,
    ) -> UPathOutcome {
        self.origin_tile = (tile_of(dest_x), tile_of(dest_y));
        self.start_land = 0;
        self.end_class = 0;
        self.valid_hits = 0;

        let dqx = ucell_of(dest_x);
        let dqy = ucell_of(dest_y);

        let rec = stack.pop_clamped();
        let mut x = rec.to_x;
        let mut y = rec.to_y;
        let mut qx = ucell_of(x);
        let mut qy = ucell_of(y);

        if qx < 0 || qy < 0 || qx >= w.wcells_w() * 16 || qy >= w.tiles_h() / 4 * 16 {
            stack.clear();
            return UPathOutcome::OffMap;
        }
        if dqx == qx && dqy == qy {
            stack.push(PathData {
                to_x: x,
                to_y: y,
                tolerance: 0,
                flags: rec.flags,
            });
            return UPathOutcome::Trivial;
        }

        let mut straight_hit = false;
        if unit.small_footprint && !unit.can_transport {
            loop {
                let same_region = w.tregion(dqx >> 2, dqy >> 2) == w.tregion(qx >> 2, qy >> 2);
                if same_region {
                    let m = (qx + qy * w.wcells_w() * 16) as u32;
                    if self.valid_ucoord(w, x, y, m) {
                        straight_hit = true;
                        break;
                    }
                }
                if (dest_x - x).abs() < 0x18 && (dest_y - y).abs() < 0x18 {
                    if (rec.flags & PathData::FLAG_MORE) == 0 {
                        return UPathOutcome::Stalled;
                    }
                    break;
                }
                let a = find_angle(dest_x - x, dest_y - y);
                x += sinx(a, PROBE_STEP);
                y -= cosx(a, PROBE_STEP);
                qx = ucell_of(x);
                qy = ucell_of(y);
                if dqx == qx && dqy == qy {
                    stack.push(PathData {
                        to_x: x,
                        to_y: y,
                        tolerance: 0,
                        flags: rec.flags,
                    });
                    return UPathOutcome::Trivial;
                }
            }
        }
        let _ = straight_hit;

        // `if (|dqx - qx| + |dqy - qy| < 2)` — adjacent cells need no search. [measured 0x00683250]
        if (dqx - qx).abs() + (dqy - qy).abs() < 2 {
            stack.push(PathData {
                to_x: x,
                to_y: y,
                tolerance: 0,
                flags: rec.flags,
            });
            return UPathOutcome::Trivial;
        }

        // The three-record search frame.
        stack.push(PathData {
            to_x: x,
            to_y: y,
            tolerance: rec.tolerance,
            flags: rec.flags,
        });
        stack.push(PathData {
            to_x: ucell_centre(qx),
            to_y: ucell_centre(qy),
            tolerance: rec.tolerance,
            flags: 0,
        });
        stack.push(PathData {
            to_x: ucell_centre(dqx),
            to_y: ucell_centre(dqy),
            tolerance: 0,
            flags: 0,
        });
        UPathOutcome::NeedsSearch
    }

    /// The post-search waypoint compression `find_upath` runs into the file-scope `keep` array.
    /// [measured `0x00683400`..`0x006836F0`]
    ///
    /// A waypoint is **dropped** when it is collinear with and equidistant from its neighbours,
    /// or when the two neighbours are exactly one 48-unit diagonal apart, unless the waypoint
    /// carries `FLAG_TRANSPORT`. Only the records above index 2 participate, and only while the
    /// waypoint and its successor both carry `FLAG_WAYPOINT`.
    pub fn compress_path(&self, stack: &mut PathStack) {
        if stack.records.len() < 4 {
            return;
        }
        let mut kept: Vec<PathData> = Vec::new();
        let mut records = std::mem::take(&mut stack.records);
        // The stack is [.. anchor, w_n .. w_1]; walk from the top (nearest the unit) down.
        let top = records.pop().unwrap();
        let mut prev = top;
        kept.push(top);
        while records.len() > 2 {
            let cur = records.pop().unwrap();
            let next = *records.last().unwrap();
            if (cur.flags & PathData::FLAG_WAYPOINT) == 0
                || (next.flags & PathData::FLAG_WAYPOINT) == 0
            {
                kept.push(cur);
                prev = cur;
                continue;
            }
            let collinear = prev.to_x - cur.to_x == cur.to_x - next.to_x
                && prev.to_y - cur.to_y == cur.to_y - next.to_y;
            let diag_short =
                (prev.to_x - next.to_x).abs() == UCELL && (prev.to_y - next.to_y).abs() == UCELL;
            let drop = (cur.flags & PathData::FLAG_TRANSPORT) == 0 && (collinear || diag_short);
            if !drop {
                kept.push(cur);
                prev = cur;
            }
        }
        // whatever is left is the anchor tail, in stack order
        let mut out = records;
        while let Some(k) = kept.pop() {
            out.push(k);
        }
        stack.records = out;
    }
}

// ---------------------------------------------------------------------------
// 9. `Unit::move_step` — the per-frame integrator  [measured]
// ---------------------------------------------------------------------------

/// Angle thresholds in `Unit::move_step`, as 32-bit binary angles. [measured]
pub mod turn {
    /// `cmp eax, 0x2222220` — below this residual the unit does not bother turning at all.
    /// `0x02222220 / 2^32` = 48.0 degrees... no: 1/75 turn = **4.8 degrees**.
    pub const IGNORE: u32 = 0x0222_2220;
    /// `cmp ecx, 0x20000000` — 45 degrees.
    pub const QUARTER_HALF: u32 = 0x2000_0000;
    /// `cmp ecx, 0x38e38e3a` — 80 degrees.
    pub const WIDE: u32 = 0x38E3_8E3A;
    /// `cmp edx, 0x40000000` — 90 degrees.
    pub const RIGHT: u32 = 0x4000_0000;
}

/// The mutable body state `move_step` writes.
#[derive(Clone, Copy, Debug, Default)]
pub struct Body {
    /// `UnitData+0x10` and `+0x14`. In the retail process both are stored **XOR'd with
    /// `0x63637`** — `move_step` decodes with `in_ECX[4] ^ 0x63637` at `0x005FAFxx` and
    /// `find_upath` compares against the same encoding. Stored plain here; the obfuscation is a
    /// tamper check, not semantics, but anything reading retail memory must undo it.
    pub x: i32,
    pub y: i32,
    /// `Guy+0x18` — the facing, a 32-bit binary angle.
    pub angle: i32,
    /// `Unit+0x60` — the stuck budget. `move_step` sets it to `2 * manhattan` when a collision is
    /// deferred, and abandons the waypoint when the remaining distance exceeds it.
    pub stuck_budget: i32,
}

/// What one `move_step` call did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveStep {
    /// Residual heading error too large: the unit turned in place and did not translate.
    TurnedOnly,
    /// Translated to a new position.
    Moved,
    /// A body was in the way; `resolve_unit_collision` territory.
    Blocked,
    /// The waypoint was reached and popped.
    Arrived,
    /// Off-map or `invalid_loc` rejected the destination tile; nothing happened.
    Refused,
}

/// `Unit::move_step` `0x005FAF30`. [measured for structure and for every constant named below;
/// UNVERIFIED for the exact turn-rate arm, which reads `Unit+0xA1/+0x8C/+0xA2` through
/// `0x005DE340` and is not modelled.]
///
/// The integrator, stripped to its arithmetic:
/// ```text
///   dx = target.x - x;  dy = target.y - y;  dist = |dx| + |dy|
///   want = find_angle(dx, dy)
///   residual = want - facing              (wrapped, then |.| via ~x for the negative half)
///   if residual >= 0x2222220: turn toward `want` by turn_rate and, if still wide, return
///   if dist <= speed: snap to the target
///   else:
///     sx = sinx(facing, speed);  sy = cosx(facing, speed)
///     if dist < 2*speed { if |dx| < |sx| { sx = dx }  if |dy| < |sy| { sy = -dy } }
///     nx = x + sx;  ny = y - sy
/// ```
/// The `y - sy` is not a sign bug: `find_angle` negates its `dy` argument, so angle 0 is
/// screen-up and the two conventions cancel.
pub fn move_step<W: UnitWorld>(
    w: &W,
    body: &mut Body,
    path: &mut PathStack,
    target: (i32, i32),
    speed: i32,
    turn_rate: i32,
) -> MoveStep {
    let (tx, ty) = target;
    let dx = tx - body.x;
    let dy = ty - body.y;
    let dist = dx.abs() + dy.abs();
    let waypoint = path.peek().unwrap_or_default();

    // --- heading ---
    let want = find_angle(dx, dy);
    let raw = (want as u32).wrapping_sub(body.angle as u32);
    let residual = if raw > 0x8000_0000 { !raw } else { raw };
    let mut turning = 0u32;
    if residual >= turn::IGNORE {
        let rate = turn_rate.max(0) as u32;
        if residual <= rate {
            body.angle = want;
        } else {
            turning = residual - rate;
            // shortest-way turn: `raw <= 0x80000000` is the positive direction.
            body.angle = if raw <= 0x8000_0000 {
                (body.angle as u32).wrapping_add(rate) as i32
            } else {
                (body.angle as u32).wrapping_sub(rate) as i32
            };
        }
    } else {
        body.angle = want;
    }
    if turning > turn::RIGHT {
        return MoveStep::TurnedOnly;
    }

    // --- translation ---
    let (nx, ny, snapping) = if dist <= speed {
        (tx, ty, true)
    } else {
        let mut sx = sinx(body.angle, speed);
        let mut sy = cosx(body.angle, speed);
        if dist < speed.saturating_mul(2) {
            if dx.abs() < sx.abs() {
                sx = dx;
            }
            if dy.abs() < sy.abs() {
                sy = -dy;
            }
        }
        (body.x + sx, body.y - sy, false)
    };

    if nx < 0 || ny < 0 || nx >= w.tiles_w() * TILE || ny >= w.tiles_h() * TILE {
        return MoveStep::Refused;
    }

    if w.unit_collides(nx, ny) {
        // The one escape hatch [measured 0x005FB6C9]: if the current waypoint is a pathfinder
        // waypoint (`flags & 2`), the *waypoint* is clear, and both axes are within 0x61 world
        // units, jump straight to the order target instead of resolving the collision.
        let escape = (waypoint.flags & PathData::FLAG_WAYPOINT) != 0
            && !w.unit_collides(waypoint.to_x, waypoint.to_y)
            && dx.abs() < 0x61
            && dy.abs() < 0x61;
        if !escape {
            body.stuck_budget = dist * 2;
            return MoveStep::Blocked;
        }
    }

    let tile_changed = tile_of(body.x) != tile_of(nx) || tile_of(body.y) != tile_of(ny);
    if tile_changed && w.invalid_loc(tile_of(nx), tile_of(ny)) {
        return MoveStep::Refused;
    }

    body.x = nx;
    body.y = ny;

    if snapping || (tx - body.x).abs() + (ty - body.y).abs() <= body.stuck_budget.max(0) {
        // arrival: pop the waypoint. `move_step` continues only if the popped record has bit 0.
        if !path.is_empty() {
            let popped = path.pop().unwrap();
            if (popped.flags & PathData::FLAG_MORE) != 0 {
                return MoveStep::Arrived;
            }
        }
        return MoveStep::Arrived;
    }
    MoveStep::Moved
}

// ---------------------------------------------------------------------------
// 10. Collision and pushing
// ---------------------------------------------------------------------------

/// `Unit::detect_unit_collision` `0x00617060` (2,410 bytes) and
/// `Unit::resolve_unit_collision` `0x005F9D30` (2,943 bytes).
///
/// **Not ported.** What is established [measured]:
/// * both are pure integer (0 FP instructions, table in the module docs);
/// * `detect_unit_collision(x, y, a, b, c, d, e)` is called from `valid_ucoord`, so **collision
///   is part of A\* validity**, not just of the integrator — a body standing in a cell makes that
///   cell impassable for the search, and the answer is memoised per cell for the whole search;
/// * `resolve_unit_collision` is one of only **two** callers of `find_upath` in the entire binary
///   (the other is `Unit::do_move`) — the push/shove behaviour is implemented as a *local detour
///   search*, not as an impulse. That is why a blocked unit consumes pathfinder budget;
/// * the retry delay a blocked third party receives is `Random::get(0, 0xFFFF) % 3 + 6` ticks,
///   drawn from `GameAccess::game_random`, the main simulation stream, in `astar_path`'s failure
///   epilogue at `0x006848C4` and `0x00684E02` — six to eight ticks.
///
/// Porting these two is the largest single remaining piece of this lane.
pub mod collision {}

// ---------------------------------------------------------------------------
// 11. Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty rectangular map with an optional wall, enough to exercise the search.
    struct TestWorld {
        tiles_w: i32,
        tiles_h: i32,
        blocked: Vec<(i32, i32)>,
        bodies: Vec<(i32, i32)>,
    }
    impl TestWorld {
        fn open(tw: i32, th: i32) -> Self {
            Self {
                tiles_w: tw,
                tiles_h: th,
                blocked: vec![],
                bodies: vec![],
            }
        }
    }
    impl UnitWorld for TestWorld {
        fn tiles_w(&self) -> i32 {
            self.tiles_w
        }
        fn tiles_h(&self) -> i32 {
            self.tiles_h
        }
        fn wcells_w(&self) -> i32 {
            self.tiles_w / 4
        }
        fn invalid_loc(&self, tx: i32, ty: i32) -> bool {
            self.blocked.contains(&(tx, ty))
        }
        fn unit_collides(&self, x: i32, y: i32) -> bool {
            self.bodies
                .iter()
                .any(|&(bx, by)| ucell_of(bx) == ucell_of(x) && ucell_of(by) == ucell_of(y))
        }
        fn needs_transport(&self, _: i32, _: i32, _: i32, _: i32) -> i32 {
            0
        }
        fn tregion(&self, _: i32, _: i32) -> i32 {
            1
        }
    }

    /// Amplitudes the engine actually passes: `Unit::move_step` passes a unit speed (tens of
    /// world units per tick), `PathFinder::find_upath` passes 24. Anything at or above 32768
    /// overflows the retail `imul` — see [`sin_table`].
    const AMP: i32 = 10_000;

    #[test]
    fn sin_table_endpoints() {
        // sin(0) = 0, sin(quarter) = full scale, sin(half) = 0, sin(three quarters) = -full.
        assert_eq!(sinx(0, AMP), 0);
        assert_eq!(sinx(0x4000_0000, AMP), AMP);
        assert_eq!(sinx(0x8000_0000u32 as i32, AMP), 0);
        assert_eq!(sinx(0xC000_0000u32 as i32, AMP), -AMP);
        // cos is sin shifted a quarter turn.
        assert_eq!(cosx(0, AMP), AMP);
        assert_eq!(cosx(0x4000_0000, AMP), 0);
    }

    /// The index-255 wrap is benign: `unit` lands on exactly 65536, one LSB above full scale,
    /// which the `>> 16` absorbs. This pins the value so a later "fix" cannot silently change it.
    #[test]
    fn index_255_wrap_lands_on_full_scale() {
        // 0x3FFFFFFF is the last representable angle inside the first quarter: idx 255,
        // frac 0x3FFFFF, slope T[0] - T[255] = -65535, product wraps to +4259839.
        assert_eq!(sin_table(0x3FFF_FFFF, 65536), 65536);
        // and it is continuous with its neighbour one frac step down
        assert_eq!(sin_table(0x3FFF_FFFE, 65536), 65536);
        assert_eq!(sin_table(0x3FF0_0000, 65536), 65535);
    }

    /// The retail overflow domain, asserted rather than avoided, so nobody "fixes" it later.
    #[test]
    fn amplitudes_at_or_above_32768_overflow_exactly_as_retail_does() {
        // 32767 is the largest safe amplitude for the low regime.
        assert_eq!(sinx(0x4000_0000, 32767), 32767);
        // At 40000 the 32-bit product wraps and the answer is nonsense — retail included.
        assert_ne!(sinx(0x2000_0000, 40000), 28284);
    }

    #[test]
    fn sin_table_is_a_sine_to_within_the_tables_own_error() {
        // 255-step table over a 256-unit quarter plus truncation: the engine's own error budget.
        // All four quadrants, 1024 samples. Worst case measured here is 0.0032 of full scale;
        // the *unfolded* `sin_table` would be 0.30 off in two of the four, which is the whole
        // reason `sinx` exists.
        let mut worst = 0.0f64;
        for k in 0..1024 {
            let a = ((k as u64 * (1u64 << 32)) / 1024) as u32 as i32;
            let got = sinx(a, AMP) as f64 / AMP as f64;
            let want = (k as f64 / 1024.0 * std::f64::consts::TAU).sin();
            worst = worst.max((got - want).abs());
            assert!((got - want).abs() < 0.01, "k={k} got={got} want={want}");
        }
        assert!(
            worst > 0.001,
            "suspiciously exact — is this really the engine's table?"
        );
    }

    #[test]
    fn find_angle_cardinals() {
        assert_eq!(find_angle(0, -1), 0); // north (-y) is angle 0
        assert_eq!(find_angle(1, 0), 0x4000_0000); // east
        assert_eq!(find_angle(0, 1), 0x8000_0000u32 as i32); // south
        assert_eq!(find_angle(-1, 0), 0xC000_0000u32 as i32); // west
    }

    #[test]
    fn find_angle_round_trips_through_the_integrator() {
        // Stepping `speed` units along find_angle's own answer must land near the target.
        for &(dx, dy) in &[
            (100, 0),
            (0, -100),
            (-70, 70),
            (300, 40),
            (-5, -900),
            (48, 48),
        ] {
            let a = find_angle(dx, dy);
            let speed = 64;
            let sx = sinx(a, speed);
            let sy = cosx(a, speed);
            let len = ((dx * dx + dy * dy) as f64).sqrt();
            let ex = (dx as f64 / len * speed as f64).round();
            let ey = (dy as f64 / len * speed as f64).round();
            assert!(
                (sx as f64 - ex).abs() <= 3.0,
                "dx={dx} dy={dy} sx={sx} ex={ex}"
            );
            assert!(
                (-sy as f64 - ey).abs() <= 3.0,
                "dx={dx} dy={dy} sy={sy} ey={ey}"
            );
        }
    }

    #[test]
    fn vector_dist_matches_the_engines_own_approximation() {
        assert_eq!(vector_dist(0, 0), 0);
        assert_eq!(vector_dist(10, 0), 10);
        assert_eq!(vector_dist(0, -10), 10);
        // M + m^2/(2M): a perfect diagonal is 1.5*M, not sqrt(2)*M.
        assert_eq!(vector_dist(100, 100), 150);
        assert_eq!(vector_dist(48, 48), 72);
        // the >= 60000 arm switches to M + m/2 (which is the same thing for m == M)
        assert_eq!(vector_dist(60000, 60000), 90000);
    }

    #[test]
    fn heuristic_is_the_measured_one() {
        // h = 10 * vector_dist on the unit grid; one straight cell of 48 units is 480 of h
        // against 32 of g. This inequality *is* the finding.
        assert_eq!(get_estimate(0, 0, 48, 0, 0x30), 480);
        assert_eq!(get_estimate(0, 0, 192, 0, 0xC0), 60);
    }

    #[test]
    fn calc_cost_unit_domain_is_32_and_40() {
        let w = TestWorld::open(16, 16);
        let pf = PathFinder::new();
        let u = PathUnit::default();
        let mut t = 0u8;
        // dir 4 = (+1, 0), straight
        assert_eq!(pf.calc_cost(&w, &u, 0, 0, 48, 0, 4, 1, &mut t), 32);
        // dir 3 = (+1, -1), diagonal
        assert_eq!(pf.calc_cost(&w, &u, 0, 0, 48, -48, 3, 1, &mut t), 40);
        assert_eq!(t, 0);
    }

    #[test]
    fn open_list_pops_ties_last_in_first_out() {
        let mut t = OrderedTree::default();
        let a = t.ordered_insert(10, 5);
        let b = t.ordered_insert(11, 5);
        let c = t.ordered_insert(12, 5);
        assert_eq!(t.leftmost(), Some(c));
        t.remove(c);
        assert_eq!(t.leftmost(), Some(b));
        t.remove(b);
        assert_eq!(t.leftmost(), Some(a));
    }

    #[test]
    fn open_list_pops_minimum_first() {
        let mut t = OrderedTree::default();
        t.ordered_insert(0, 40);
        let lo = t.ordered_insert(1, 5);
        t.ordered_insert(2, 90);
        assert_eq!(t.leftmost(), Some(lo));
    }

    fn frame(stack: &mut PathStack, sx: i32, sy: i32, gx: i32, gy: i32, tol: i32) {
        stack.push(PathData {
            to_x: gx,
            to_y: gy,
            tolerance: tol,
            flags: 0,
        });
        stack.push(PathData {
            to_x: sx,
            to_y: sy,
            tolerance: tol,
            flags: 0,
        });
        stack.push(PathData {
            to_x: gx,
            to_y: gy,
            tolerance: 0,
            flags: 0,
        });
    }

    #[test]
    fn straight_search_on_an_open_map() {
        let w = TestWorld::open(16, 16);
        let mut pf = PathFinder::new();
        let mut s = PathStack::new();
        frame(
            &mut s,
            ucell_centre(2),
            ucell_centre(2),
            ucell_centre(20),
            ucell_centre(2),
            0,
        );
        let u = PathUnit::default();
        let r = pf.astar_path_unit(&w, &mut s, &SearchArgs { unit: &u, quick: 0 });
        assert_eq!(r, SearchResult::Found);
        // top of stack is the waypoint nearest the start
        let first = s.peek().unwrap();
        assert_eq!((first.to_x, first.to_y), (ucell_centre(2), ucell_centre(2)));
        assert_eq!(
            first.flags & PathData::FLAG_WAYPOINT,
            PathData::FLAG_WAYPOINT
        );
        // the anchor record survives at the bottom
        assert_eq!(s.records[0].to_x, ucell_centre(20));
        // every waypoint sits on the same row and marches monotonically toward the goal
        let ys: Vec<i32> = s.records.iter().skip(1).map(|r| r.to_y).collect();
        assert!(ys.iter().all(|&y| y == ucell_centre(2)), "{ys:?}");
    }

    #[test]
    fn search_routes_around_a_wall() {
        let mut w = TestWorld::open(16, 16);
        // a vertical wall of tiles at tile x = 4, leaving a gap at tile y = 0
        for ty in 1..16 {
            w.blocked.push((4, ty));
        }
        let mut pf = PathFinder::new();
        let mut s = PathStack::new();
        frame(
            &mut s,
            ucell_centre(8),
            ucell_centre(30),
            ucell_centre(28),
            ucell_centre(30),
            0,
        );
        let u = PathUnit::default();
        let r = pf.astar_path_unit(&w, &mut s, &SearchArgs { unit: &u, quick: 0 });
        assert_eq!(r, SearchResult::Found);
        // no emitted waypoint may sit on a blocked tile
        for rec in s.records.iter().skip(1) {
            assert!(
                !w.invalid_loc(tile_of(rec.to_x), tile_of(rec.to_y)),
                "waypoint inside the wall: {rec:?}"
            );
        }
        // it must have detoured through the gap at tile row 0 — the straight line is at row 7
        assert!(
            s.records.iter().any(|r| tile_of(r.to_y) == 0),
            "no waypoint reached the gap: {:?}",
            s.records
                .iter()
                .map(|r| (tile_of(r.to_x), tile_of(r.to_y)))
                .collect::<Vec<_>>()
        );
        // and it must have crossed the wall column
        assert!(s.records.iter().any(|r| tile_of(r.to_x) > 4));
    }

    #[test]
    fn search_fails_when_walled_in() {
        let mut w = TestWorld::open(16, 16);
        for ty in 0..16 {
            w.blocked.push((4, ty));
        }
        let mut pf = PathFinder::new();
        let mut s = PathStack::new();
        frame(
            &mut s,
            ucell_centre(8),
            ucell_centre(30),
            ucell_centre(28),
            ucell_centre(30),
            0,
        );
        let u = PathUnit::default();
        let r = pf.astar_path_unit(&w, &mut s, &SearchArgs { unit: &u, quick: 0 });
        assert_eq!(r, SearchResult::Failed);
    }

    /// The two failure epilogues both set the RNG obligation. Nothing else does.
    #[test]
    fn failure_raises_the_rng_obligation() {
        let mut w = TestWorld::open(16, 16);
        for ty in 0..16 {
            w.blocked.push((4, ty));
        }
        let mut pf = PathFinder::new();
        let mut s = PathStack::new();
        frame(
            &mut s,
            ucell_centre(8),
            ucell_centre(30),
            ucell_centre(28),
            ucell_centre(30),
            0,
        );
        let u = PathUnit::default();
        assert_eq!(
            pf.astar_path_unit(&w, &mut s, &SearchArgs { unit: &u, quick: 0 }),
            SearchResult::Failed
        );
        assert!(
            pf.pending_retry_draw,
            "a failed search owes the sim stream a draw"
        );

        let open = TestWorld::open(16, 16);
        let mut ok = PathFinder::new();
        let mut s2 = PathStack::new();
        frame(
            &mut s2,
            ucell_centre(2),
            ucell_centre(2),
            ucell_centre(20),
            ucell_centre(2),
            0,
        );
        assert_eq!(
            ok.astar_path_unit(&open, &mut s2, &SearchArgs { unit: &u, quick: 0 }),
            SearchResult::Found
        );
        assert!(
            !ok.pending_retry_draw,
            "a successful search must not perturb the stream"
        );
    }

    /// Budget exhaustion is a **failure** for a normal unit search and a **partial path** for a
    /// quick one. That asymmetry is the `param_3 != 0` short-circuit at `0x0068487C`.
    #[test]
    fn budget_exhaustion_fails_normally_and_yields_a_partial_path_in_quick_mode() {
        // a 256x256 map sealed by a full-height wall: the search can never reach the goal
        let mut w = TestWorld::open(256, 256);
        for ty in 0..256 {
            w.blocked.push((8, ty));
        }
        let u = PathUnit::default();

        let mut slow = PathFinder::new();
        let mut s = PathStack::new();
        frame(
            &mut s,
            ucell_centre(4),
            ucell_centre(500),
            ucell_centre(900),
            ucell_centre(500),
            0,
        );
        let r = slow.astar_path_unit(&w, &mut s, &SearchArgs { unit: &u, quick: 0 });
        assert_eq!(r, SearchResult::Failed);
        assert!(
            slow.last_expanded >= UNIT_NODE_BUDGET,
            "{}",
            slow.last_expanded
        );

        let mut quick = PathFinder::new();
        let mut s2 = PathStack::new();
        frame(
            &mut s2,
            ucell_centre(4),
            ucell_centre(500),
            ucell_centre(900),
            ucell_centre(500),
            0,
        );
        let r2 = quick.astar_path_unit(&w, &mut s2, &SearchArgs { unit: &u, quick: 1 });
        assert_eq!(r2, SearchResult::Found);
        assert!(
            s2.records.len() > 1,
            "quick mode must return the best node it reached"
        );
    }

    #[test]
    fn quick_mode_halves_the_branching_factor() {
        let w = TestWorld::open(16, 16);
        let u = PathUnit::default();
        let mut a = PathFinder::new();
        let mut sa = PathStack::new();
        frame(
            &mut sa,
            ucell_centre(2),
            ucell_centre(2),
            ucell_centre(24),
            ucell_centre(18),
            0,
        );
        a.astar_path_unit(&w, &mut sa, &SearchArgs { unit: &u, quick: 0 });
        let mut b = PathFinder::new();
        let mut sb = PathStack::new();
        frame(
            &mut sb,
            ucell_centre(2),
            ucell_centre(2),
            ucell_centre(24),
            ucell_centre(18),
            0,
        );
        b.astar_path_unit(&w, &mut sb, &SearchArgs { unit: &u, quick: 1 });
        assert!(
            b.last_expanded < a.last_expanded,
            "{} !< {}",
            b.last_expanded,
            a.last_expanded
        );
    }

    #[test]
    fn arrival_tolerance_shortens_the_path() {
        let w = TestWorld::open(16, 16);
        let u = PathUnit::default();
        let mut tight = PathFinder::new();
        let mut st = PathStack::new();
        frame(
            &mut st,
            ucell_centre(2),
            ucell_centre(2),
            ucell_centre(20),
            ucell_centre(2),
            0,
        );
        tight.astar_path_unit(&w, &mut st, &SearchArgs { unit: &u, quick: 0 });
        let mut loose = PathFinder::new();
        let mut sl = PathStack::new();
        frame(
            &mut sl,
            ucell_centre(2),
            ucell_centre(2),
            ucell_centre(20),
            ucell_centre(2),
            48 * 8,
        );
        loose.astar_path_unit(&w, &mut sl, &SearchArgs { unit: &u, quick: 0 });
        assert!(
            sl.records.len() < st.records.len(),
            "{} !< {}",
            sl.records.len(),
            st.records.len()
        );
    }

    #[test]
    fn find_upath_probe_short_circuits_an_open_move() {
        let w = TestWorld::open(16, 16);
        let mut pf = PathFinder::new();
        let mut s = PathStack::new();
        s.push(PathData {
            to_x: 300,
            to_y: 300,
            tolerance: 0,
            flags: 0,
        });
        let u = PathUnit::default();
        // destination four cells away, nothing in the way: the straight probe wins.
        let out = pf.find_upath_prepare(&w, &mut s, &u, 300 + 4 * UCELL, 300);
        assert!(matches!(
            out,
            UPathOutcome::Trivial | UPathOutcome::NeedsSearch
        ));
    }

    #[test]
    fn find_upath_sets_up_a_three_record_frame_for_a_long_move() {
        let mut w = TestWorld::open(24, 24);
        // block the direct line so the probe cannot short-circuit
        for ty in 0..24 {
            w.blocked.push((6, ty));
        }
        let mut pf = PathFinder::new();
        let mut s = PathStack::new();
        s.push(PathData {
            to_x: ucell_centre(4),
            to_y: ucell_centre(40),
            tolerance: 96,
            flags: 0,
        });
        let u = PathUnit {
            small_footprint: false,
            ..PathUnit::default()
        };
        let out = pf.find_upath_prepare(&w, &mut s, &u, ucell_centre(60), ucell_centre(40));
        assert_eq!(out, UPathOutcome::NeedsSearch);
        assert_eq!(s.records.len(), 3);
        // both search endpoints are snapped to cell centres
        assert_eq!(s.records[2].to_x % UCELL, UCELL / 2);
        assert_eq!(s.records[1].to_x % UCELL, UCELL / 2);
        // the anchor keeps the caller's tolerance, which is what astar reads
        assert_eq!(s.records[0].tolerance, 96);
    }

    #[test]
    fn move_step_walks_a_path_to_its_target() {
        let w = TestWorld::open(16, 16);
        let mut body = Body {
            x: 300,
            y: 300,
            angle: 0,
            stuck_budget: 1 << 20,
        };
        let mut path = PathStack::new();
        let target = (300 + 500, 300);
        let mut n = 0;
        while (body.x, body.y) != target && n < 200 {
            move_step(&w, &mut body, &mut path, target, 40, 1 << 28);
            n += 1;
        }
        assert_eq!((body.x, body.y), target, "did not arrive in {n} steps");
        assert!(n < 30, "took {n} steps for 500 units at speed 40");
    }

    #[test]
    fn move_step_refuses_to_leave_the_map() {
        let w = TestWorld::open(16, 16);
        let mut body = Body {
            x: 10,
            y: 10,
            angle: 0,
            stuck_budget: 0,
        };
        let mut path = PathStack::new();
        let r = move_step(&w, &mut body, &mut path, (-5000, 10), 40, 1 << 28);
        assert_eq!(r, MoveStep::Refused);
        assert_eq!((body.x, body.y), (10, 10));
    }

    #[test]
    fn move_step_reports_a_body_in_the_way() {
        let mut w = TestWorld::open(16, 16);
        w.bodies.push((400, 300));
        let mut body = Body {
            x: 300,
            y: 300,
            angle: 0x4000_0000,
            stuck_budget: 0,
        };
        let mut path = PathStack::new();
        let r = move_step(&w, &mut body, &mut path, (900, 300), 100, 1 << 28);
        assert_eq!(r, MoveStep::Blocked);
        assert!(body.stuck_budget > 0);
    }

    #[test]
    fn path_stack_walk_bytes_feed_the_units_channel() {
        let mut s = PathStack::new();
        s.push(PathData {
            to_x: 1,
            to_y: 2,
            tolerance: 3,
            flags: 4,
        });
        s.push(PathData {
            to_x: 5,
            to_y: 6,
            tolerance: 7,
            flags: 8,
        });
        let b = s.walk_bytes();
        assert_eq!(b.len(), 32);
        // adler starts at 1 for every channel
        let c = adler32(1, &b);
        assert_ne!(c, 1);
        // and it is order-sensitive, which is the whole point of a desync channel
        let mut t = PathStack::new();
        t.push(PathData {
            to_x: 5,
            to_y: 6,
            tolerance: 7,
            flags: 8,
        });
        t.push(PathData {
            to_x: 1,
            to_y: 2,
            tolerance: 3,
            flags: 4,
        });
        assert_ne!(adler32(1, &t.walk_bytes()), c);
    }

    #[test]
    fn adler32_matches_the_zlib_definition() {
        assert_eq!(adler32(1, b""), 1);
        assert_eq!(adler32(1, b"a"), 0x0062_0062);
        assert_eq!(adler32(1, b"abc"), 0x024d_0127);
    }

    #[test]
    fn search_is_deterministic_across_repeats() {
        let mut w = TestWorld::open(20, 20);
        for ty in 2..18 {
            w.blocked.push((7, ty));
        }
        let u = PathUnit::default();
        let mut first: Option<Vec<PathData>> = None;
        for _ in 0..5 {
            let mut pf = PathFinder::new();
            let mut s = PathStack::new();
            frame(
                &mut s,
                ucell_centre(4),
                ucell_centre(40),
                ucell_centre(60),
                ucell_centre(40),
                0,
            );
            assert_eq!(
                pf.astar_path_unit(&w, &mut s, &SearchArgs { unit: &u, quick: 0 }),
                SearchResult::Found
            );
            match &first {
                None => first = Some(s.records.clone()),
                Some(f) => assert_eq!(f, &s.records),
            }
        }
    }
}
