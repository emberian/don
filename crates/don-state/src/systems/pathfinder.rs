//! `PathFinder::astar_path` 0x00683770 and the unit-domain wrapper
//! `PathFinder::find_upath` 0x00682f30 (`re/decomp-all/00683770.c`,
//! `00682f30.c`), transcribed against the live `Save`.
//!
//! Leaf module: exposes free functions for the owning traversal
//! (`Unit::do_move` 0x005f7b30, untranscribed) to call; never edits sibling
//! modules.
//!
//! # Retail call shape
//!
//! ```text
//! Unit::do_move 0x005f7b30
//!  └─ PathFinder::find_upath(Stack<PathData>*, who, o, quick) 0x00688eb0
//!      limit = 500 / repaths[who]^2 (halved when quick)
//!      └─ PathFinder::find_upath(stack, unit.x, unit.y, who, o, quick) 0x00682f30
//!          sx/sy = unit tile; straight-line probe; three-record frame
//!          └─ PathFinder::astar_path(stack, 0x30, quick) 0x00683770
//!              ├─ first_open_node 0x00687970 (Tree leftmost + BRTree tombstone)
//!              ├─ valid_ucoord 0x00687c80
//!              │   ├─ UnitData::invalid_loc 0x00607c30 (→ WorldData::was_seen 0x006b53f0)
//!              │   └─ Unit::detect_unit_collision 0x00617060
//!              │       └─ CollCheck::collide_here 0x00682540 (→ fill_slots 0x006820e0)
//!              ├─ calc_cost 0x00684e50 (→ UnitData::needs_transport 0x00609920)
//!              ├─ PathFinderData::find_node_open 0x006882b0
//!              └─ failure epilogue: Random::get(0, 0xffff) on game_random
//!          collinear compression, then kill_lists 0x00687ae0
//! ```
//!
//! # What is walked vs scratch
//!
//! * Walked (`Save.pathfinder`, 27 i32 = `pathfinder+0x58..+0xc4`, see
//!   `PathFinder::walk_data` 0x00689cb0 → `DAT_00e85e98..DAT_00e85f04`):
//!   `sx sy dbg_collisions anti_unit offx offy army iroquois worker
//!   no_danger limit saving avoid_land avoid_sea valid_hit scouting
//!   can_transport dbg_view_failures road_*`. `show_debug` (+0xc4) is
//!   outside the block. The searches below write `sx sy anti_unit offx offy
//!   iroquois limit saving avoid_land avoid_sea valid_hit scouting` exactly
//!   where retail does.
//! * Scratch (never walked): the five containers at `pathfinder+0x40..+0x50`
//!   (`openlist` `Tree<PathNode*,int>`, `openlistrefs`
//!   `BRTree<TreeNode*,ulong>`, `closedlist` `BRTree<PathNode*,ulong>`,
//!   `blocklist` `Tree<CollBlock*,int>`, `validlist` `BRTree<int,ulong>`),
//!   the `Recycler<PathNode>` pool, and the per-unit parking slots
//!   `UnitData+0x104..+0x148` a suspended search is moved into (outside the
//!   walked `+0x48..+0xb7` body). They live in [`PathFinder`].
//! * The unit's `Stack<PathData>` (`UnitData+0xb8`, walked as
//!   `Unit.path`) is the output.
//!
//! # RNG
//!
//! `calc_cost` draws nothing. The only draw is in the unit-domain failure
//! epilogue (0x00684e02.. and the budget-exhausted twin at 0x006848a0..):
//! when the unit's current order answers virtual `+0x14` (is-move) and
//! `MoveOrder::attempts` (+0x20) `< 13`, `Random::get(0, 0xffff)` is drawn
//! from `GameAccess::game_random` and `retry = r % 3 + 6`; then
//! `Unit::safe += 30` unconditionally. One draw per failed unit search.
//! `find_upath` 0x00682f30 additionally calls `Unit::cant_move(0)`
//! 0x005e2cb0 on failure (untranscribed here; reported in
//! [`UPathResult::cant_move`]).

#![allow(dead_code)]
#![allow(clippy::too_many_arguments)]

use std::collections::{BTreeMap, HashMap};

use crate::sections::{Obj, Save, TypeRec};
use crate::tick::StepStatus;

/// `astar_path` (unit domain), `calc_cost` (unit arm), `valid_ucoord`,
/// `invalid_loc` (land/air arms), `detect_unit_collision` (path arm),
/// `collide_here`/`fill_slots`, the Tree/BRTree semantics, the failure
/// epilogue and `find_upath` are transcribed. The `0xc0`/`0x300` `calc_cost`
/// arms, `invalid_loc`'s sea arm and the virtual order predicates are the
/// stopping points (see `TODO(va)` notes). The module is not yet called from
/// the frame schedule.
pub const STATUS: StepStatus = StepStatus::Partial;

// ---------------------------------------------------------------------------
// Coordinate ladder
// ---------------------------------------------------------------------------

/// `[0x00cae5fc]` is the table built by 0x00681db0: `T[i] = i / 3` for
/// `i >= 0` and `T[-k] = (-k - 2) / 3` for the negative half, i.e. floor
/// division by 3 on both sides.
#[inline]
fn t3(i: i32) -> i32 {
    i.div_euclid(3)
}
/// `T[v >> 4]` — world → unit cell (48).
#[inline]
fn ucell(v: i32) -> i32 {
    t3(v >> 4)
}
/// `T[v >> 6]` — world → tile (192).
#[inline]
fn tile(v: i32) -> i32 {
    t3(v >> 6)
}
/// `T[v >> 8]` — world → water cell (768).
#[inline]
fn wcell(v: i32) -> i32 {
    t3(v >> 8)
}

const STEP_UNIT: i32 = 0x30;
const STEP_TILE: i32 = 0xc0;
const STEP_WATER: i32 = 0x300;

/// `int move_x[441]` 0x00adcaf0 / `move_y[441]` 0x00adc400 / `ring_count[11]`
/// 0x00add1e0, dumped from `.rdata`. Indices 1..=8 are the eight A*
/// directions (odd = diagonal); the rest is the concentric-ring footprint
/// enumeration `collide_here` walks.
const MOVE_X: [i32; 441] = [
    0, -1, 0, 1, 1, 1, 0, -1, -1, -1, 0, 1, 2, 2, 2, 1, 0, -1, -2, -2, -2, -2, 2, 2, -2, -3, -2,
    -1, 0, 1, 2, 3, 3, 3, 3, 3, 3, 3, 2, 1, 0, -1, -2, -3, -3, -3, -3, -3, -3, -4, -3, -2, -1, 0,
    1, 2, 3, 4, 4, 4, 4, 4, 4, 4, 4, 4, 3, 2, 1, 0, -1, -2, -3, -4, -4, -4, -4, -4, -4, -4, -4, -5,
    -4, -3, -2, -1, 0, 1, 2, 3, 4, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 4, 3, 2, 1, 0, -1, -2, -3, -4,
    -5, -5, -5, -5, -5, -5, -5, -5, -5, -5, -6, -5, -4, -3, -2, -1, 0, 1, 2, 3, 4, 5, 6, 6, 6, 6,
    6, 6, 6, 6, 6, 6, 6, 6, 6, 5, 4, 3, 2, 1, 0, -1, -2, -3, -4, -5, -6, -6, -6, -6, -6, -6, -6,
    -6, -6, -6, -6, -6, -7, -6, -5, -4, -3, -2, -1, 0, 1, 2, 3, 4, 5, 6, 7, 7, 7, 7, 7, 7, 7, 7, 7,
    7, 7, 7, 7, 7, 7, 6, 5, 4, 3, 2, 1, 0, -1, -2, -3, -4, -5, -6, -7, -7, -7, -7, -7, -7, -7, -7,
    -7, -7, -7, -7, -7, -7, -8, -7, -6, -5, -4, -3, -2, -1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 8, 8, 8, 8,
    8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 7, 6, 5, 4, 3, 2, 1, 0, -1, -2, -3, -4, -5, -6, -7, -8, -8,
    -8, -8, -8, -8, -8, -8, -8, -8, -8, -8, -8, -8, -8, -8, -9, -8, -7, -6, -5, -4, -3, -2, -1, 0,
    1, 2, 3, 4, 5, 6, 7, 8, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 8, 7, 6, 5, 4,
    3, 2, 1, 0, -1, -2, -3, -4, -5, -6, -7, -8, -9, -9, -9, -9, -9, -9, -9, -9, -9, -9, -9, -9, -9,
    -9, -9, -9, -9, -9, -10, -9, -8, -7, -6, -5, -4, -3, -2, -1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
    10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 9, 8, 7, 6, 5,
    4, 3, 2, 1, 0, -1, -2, -3, -4, -5, -6, -7, -8, -9, -10, -10, -10, -10, -10, -10, -10, -10, -10,
    -10, -10, -10, -10, -10, -10, -10, -10, -10, -10, -10,
];
const MOVE_Y: [i32; 441] = [
    0, -1, -1, -1, 0, 1, 1, 1, 0, -2, -2, -2, -1, 0, 1, 2, 2, 2, 1, 0, -1, -2, -2, 2, 2, -3, -3,
    -3, -3, -3, -3, -3, -2, -1, 0, 1, 2, 3, 3, 3, 3, 3, 3, 3, 2, 1, 0, -1, -2, -4, -4, -4, -4, -4,
    -4, -4, -4, -4, -3, -2, -1, 0, 1, 2, 3, 4, 4, 4, 4, 4, 4, 4, 4, 4, 3, 2, 1, 0, -1, -2, -3, -5,
    -5, -5, -5, -5, -5, -5, -5, -5, -5, -5, -4, -3, -2, -1, 0, 1, 2, 3, 4, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 4, 3, 2, 1, 0, -1, -2, -3, -4, -6, -6, -6, -6, -6, -6, -6, -6, -6, -6, -6, -6, -6, -5,
    -4, -3, -2, -1, 0, 1, 2, 3, 4, 5, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 5, 4, 3, 2, 1, 0, -1,
    -2, -3, -4, -5, -7, -7, -7, -7, -7, -7, -7, -7, -7, -7, -7, -7, -7, -7, -7, -6, -5, -4, -3, -2,
    -1, 0, 1, 2, 3, 4, 5, 6, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 6, 5, 4, 3, 2, 1, 0, -1,
    -2, -3, -4, -5, -6, -8, -8, -8, -8, -8, -8, -8, -8, -8, -8, -8, -8, -8, -8, -8, -8, -8, -7, -6,
    -5, -4, -3, -2, -1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8,
    7, 6, 5, 4, 3, 2, 1, 0, -1, -2, -3, -4, -5, -6, -16, -9, -9, -9, -9, -9, -9, -9, -9, -9, -9,
    -9, -9, -9, -9, -9, -9, -9, -9, -8, -7, -6, -5, -4, -3, -2, -1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9,
    9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, -1, -2, -3,
    -4, -5, -6, -7, -8, -10, -10, -10, -10, -10, -10, -10, -10, -10, -10, -10, -10, -10, -10, -10,
    -10, -10, -10, -10, -10, -10, -9, -8, -7, -6, -5, -4, -3, -2, -1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9,
    10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 9, 8, 7, 6,
    5, 4, 3, 2, 1, 0, -1, -2, -3, -4, -5, -6, -7, -8, -9, 0,
];
const RING_COUNT: [i32; 11] = [1, 9, 25, 49, 81, 121, 169, 225, 289, 361, 441];
/// `0x00add2a0` / `0x00add2b0` — the four block candidates `fill_slots` resolves.
const BLOCK_DX: [i32; 4] = [0, 0, 1, 1];
const BLOCK_DY: [i32; 4] = [0, 1, 0, 1];

// ---------------------------------------------------------------------------
// Integer trig (`find_angle` 0x0092d130, `sin_table` 0x00a46a00) — used only
// by find_upath's straight-line probe.
// ---------------------------------------------------------------------------

/// `trig_init` 0x00a46980: `T[i] = (int)(sin(i * 1.570796327 / 255.0) * 65535.0)`.
fn sin_quarter(i: usize) -> i32 {
    ((i as f64 * 1.570796327 / 255.0).sin() * 65535.0) as i32
}

/// `find_angle(ecx = dx, edx = dy)` 0x0092d130 — binary angle, 0 = -y.
fn find_angle(dx: i32, dy: i32) -> i32 {
    let ndy = dy.wrapping_neg();
    if dx == 0 {
        return if ndy > 0 { 0 } else { i32::MIN };
    }
    if ndy == 0 {
        return if dx > 0 { 0x4000_0000 } else { -0x4000_0000 };
    }
    let a = dx.wrapping_abs();
    let b = ndy.wrapping_abs();
    let (small, big) = if b < a { (b, a) } else { (a, b) };
    let swapped = a <= b;
    let r = small.wrapping_mul(0x4000) / big;
    let u = (0x1333 - r).wrapping_abs();
    let u = (0x2800 - (u.wrapping_mul(0xb00) >> 14)).wrapping_mul(r) & 0xffff_c000u32 as i32;
    let i2 = u.wrapping_mul(4);
    let n4 = u.wrapping_mul(-4);
    if dx <= 0 {
        if ndy <= 0 {
            return if swapped { i2.wrapping_add(i32::MIN) } else { n4.wrapping_sub(0x4000_0000) };
        }
        return if swapped { n4 } else { i2.wrapping_sub(0x4000_0000) };
    }
    if ndy > 0 {
        return if swapped { i2 } else { n4.wrapping_add(0x4000_0000) };
    }
    if swapped {
        n4.wrapping_add(i32::MIN)
    } else {
        i2.wrapping_add(0x4000_0000)
    }
}

/// `sin_table(ecx = angle, edx = amplitude)` 0x00a46a00.
fn sin_table(angle: i32, amplitude: i32) -> i32 {
    let amp = if angle >= 0 { amplitude } else { amplitude.wrapping_neg() };
    let a = angle as u32;
    let bit30 = a & 0x4000_0000 != 0;
    let idx = ((a & 0x3fff_ffff) >> 22) as usize;
    let frac = (a & 0x3f_ffff) as i32;
    let t0 = sin_quarter(idx);
    let t1 = sin_quarter((idx + 1) & 0xff);
    let interp = t1.wrapping_sub(t0).wrapping_mul(frac) >> 22;
    let unit = if bit30 { interp.wrapping_sub(t0).wrapping_add(0xffff) } else { interp.wrapping_add(t0) };
    if amp < 0xffff {
        unit.wrapping_mul(amp) >> 16
    } else if amp > 0xff_fffe {
        unit.wrapping_mul(amp >> 16)
    } else {
        unit.wrapping_mul(amp >> 8) >> 8
    }
}

/// The quadrant fold inlined at 0x00683190..0x006831b4 (and again for the
/// cosine at 0x006831bb..): negative angle flips the amplitude and clears
/// the sign; bit 30 reflects.
fn sin_fold(angle: i32, amp: i32) -> i32 {
    let (mut a, mut amp) = (angle, amp);
    if a < 0 {
        amp = amp.wrapping_neg();
        a &= 0x7fff_ffff;
    }
    let b = if a & 0x4000_0000 != 0 { 0x7fff_ffff - a } else { a };
    sin_table(b, amp)
}

/// `vector_dist(ecx, edx)` 0x0046cff0 — integer octagonal distance.
fn vector_dist(a: i32, b: i32) -> i32 {
    let a = a.wrapping_abs() as u32;
    let b = b.wrapping_abs() as u32;
    if (b as i32) < (a as i32) {
        if a != 0 {
            if b > 59999 {
                return ((b.wrapping_add(a.wrapping_mul(2))) >> 1) as i32;
            }
            return (b.wrapping_mul(b) / a.wrapping_mul(2)).wrapping_add(a) as i32;
        }
    } else if b != 0 {
        if a > 59999 {
            return ((a.wrapping_add(b.wrapping_mul(2))) >> 1) as i32;
        }
        return (a.wrapping_mul(a) / b.wrapping_mul(2)).wrapping_add(b) as i32;
    }
    0
}

/// The heuristic: `vector_dist * 10` in the unit domain, `* 60 / step` otherwise.
fn estimate(dx: i32, dy: i32, step: i32) -> i32 {
    let d = vector_dist(dx, dy);
    if step == STEP_UNIT {
        d.wrapping_mul(10)
    } else {
        d.wrapping_mul(0x3c) / step
    }
}

// ---------------------------------------------------------------------------
// PathData / Stack<PathData>
// ---------------------------------------------------------------------------

/// `class PathData` (16 B): `Coord to_x, to_y; int tolerance; int flags`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PathData {
    pub to_x: i32,
    pub to_y: i32,
    pub tolerance: i32,
    pub flags: i32,
}

/// `Stack<PathData>`: `list, size (cap), length, increment`. Mirrors
/// `sections::PathStack` (cap, len, inc, 16-byte records).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PStack {
    pub cap: i32,
    pub inc: i8,
    pub recs: Vec<PathData>,
}

impl PStack {
    pub fn from_walked(p: &crate::sections::PathStack) -> Self {
        let recs = p
            .data
            .chunks_exact(16)
            .map(|r| PathData {
                to_x: i32::from_le_bytes(r[0..4].try_into().unwrap()),
                to_y: i32::from_le_bytes(r[4..8].try_into().unwrap()),
                tolerance: i32::from_le_bytes(r[8..12].try_into().unwrap()),
                flags: i32::from_le_bytes(r[12..16].try_into().unwrap()),
            })
            .collect();
        PStack { cap: p.cap, inc: p.inc as i8, recs }
    }
    pub fn write_walked(&self, p: &mut crate::sections::PathStack) {
        p.cap = self.cap;
        p.len = self.recs.len() as i32;
        p.inc = self.inc as u8;
        p.data.clear();
        for r in &self.recs {
            p.data.extend_from_slice(&r.to_x.to_le_bytes());
            p.data.extend_from_slice(&r.to_y.to_le_bytes());
            p.data.extend_from_slice(&r.tolerance.to_le_bytes());
            p.data.extend_from_slice(&r.flags.to_le_bytes());
        }
    }
    pub fn len(&self) -> i32 {
        self.recs.len() as i32
    }
    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }
    /// `Stack<PathData>::push` 0x0046d820: `if (size <= length)
    /// increase_size(increment)` (0x0046e8c0: `n < 0 → n = size ? size : 1`;
    /// `size += n`), then store and `length++`.
    pub fn push(&mut self, r: PathData) {
        if self.cap <= self.len() {
            let mut n = self.inc as i32;
            if n < 0 {
                n = if self.cap != 0 { self.cap } else { 1 };
            }
            self.cap += n;
        }
        self.recs.push(r);
    }
    /// `Stack<PathData>::pop` 0x0046d870: `if (length < 1) length = 1;
    /// length--; return list[length]`. On an empty stack retail returns
    /// `list[0]` (whatever is there) and leaves `length == 0`.
    pub fn pop(&mut self) -> PathData {
        self.recs.pop().unwrap_or_default()
    }
    /// `Stack<PathData>::peek` 0x0046d890: `list[length - 1]` (no clamp).
    pub fn peek(&self) -> PathData {
        self.recs.last().copied().unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// World / unit / leader views over the Save
// ---------------------------------------------------------------------------

/// `WorldData` fields the search reads: `xs ys` (wcells), `fog_xs`,
/// `tile_xs tile_ys`, `wdata` (21 walked bytes of the 28-byte `WData`),
/// `tdata` (u16 mask per tile), `seen2` and the `CollBlock` planes.
struct Wld<'a> {
    xs: i32,
    ys: i32,
    fog_xs: i32,
    tile_xs: i32,
    tile_ys: i32,
    wdata: &'a [u8],
    tdata: &'a [u8],
    seen2: &'a [u8],
    blocks: &'a [crate::sections::CollBlock],
}

const WDATA_ROW: usize = 21;

impl<'a> Wld<'a> {
    fn new(save: &'a Save) -> Self {
        let w = &save.world;
        Wld {
            xs: w.xs,
            ys: w.ys,
            fog_xs: w.direct.get(1).copied().unwrap_or(0),
            tile_xs: w.direct.get(4).copied().unwrap_or(0),
            tile_ys: w.direct.get(5).copied().unwrap_or(0),
            wdata: &w.wdata,
            tdata: &w.tdata,
            seen2: &w.seen2,
            blocks: &w.blocks,
        }
    }
    fn w_index(&self, wx: i32, wy: i32) -> Option<usize> {
        if wx < 0 || wy < 0 || wx >= self.xs || wy >= self.ys {
            return None;
        }
        Some((wy * self.xs + wx) as usize)
    }
    /// `WData.flags` (+0).
    fn wflags(&self, wx: i32, wy: i32) -> u16 {
        self.w_index(wx, wy)
            .and_then(|i| self.wdata.get(i * WDATA_ROW..i * WDATA_ROW + 2))
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .unwrap_or(0)
    }
    /// `WData.region` (+4) / `region2` (+6).
    fn wregion(&self, wx: i32, wy: i32, second: bool) -> i16 {
        let off = if second { 6 } else { 4 };
        self.w_index(wx, wy)
            .and_then(|i| self.wdata.get(i * WDATA_ROW + off..i * WDATA_ROW + off + 2))
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .unwrap_or(-1)
    }
    /// `WData.who` (+0xf).
    fn wwho(&self, wx: i32, wy: i32) -> i8 {
        self.w_index(wx, wy).and_then(|i| self.wdata.get(i * WDATA_ROW + 0xf)).map(|&b| b as i8).unwrap_or(-1)
    }
    /// `TData.mask` for a tile.
    fn tmask(&self, tx: i32, ty: i32) -> u16 {
        if tx < 0 || ty < 0 || tx >= self.tile_xs || ty >= self.tile_ys {
            return 0;
        }
        let i = (ty * self.tile_xs + tx) as usize * 2;
        self.tdata.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).unwrap_or(0)
    }
    /// `WorldData::get_tregion` 0x006b52e0: `region2` when the wcell is
    /// `flags & 0x100` and the tile is water (`mask & 0x30 == 0x20`).
    fn tregion(&self, tx: i32, ty: i32) -> i32 {
        let (wx, wy) = (tx >> 2, ty >> 2);
        let second = self.wflags(wx, wy) & 0x100 != 0 && self.tmask(tx, ty) & 0x30 == 0x20;
        self.wregion(wx, wy, second) as i32
    }
    /// `UnitData::needs_transport` 0x00609920 on tiles.
    fn needs_transport(&self, x1: i32, y1: i32, x2: i32, y2: i32) -> i32 {
        if x1 == x2 && y1 == y2 {
            return 0;
        }
        let a = self.tmask(x1, y1) & 0x30;
        let b = self.tmask(x2, y2) & 0x30;
        if (a == 0x20) != (b == 0x20) {
            return (a != 0x20) as i32 + 1;
        }
        0
    }
    /// `WData.block` for a wcell: the walked `present` flag and bytes.
    fn block(&self, wi: usize) -> Option<&'a [u8]> {
        let b = self.blocks.get(wi)?;
        if b.present != 1 {
            return None;
        }
        Some(&b.data)
    }
}

/// Byte-image view of a serialized `Unit` (`UnitData` image offsets).
#[derive(Clone, Copy)]
struct UView<'a> {
    sub: &'a [u8],
    body: &'a [u8],
}

impl<'a> UView<'a> {
    fn of(u: &'a crate::sections::Unit) -> Option<Self> {
        if u.base.sub.body.len() != 19 || u.body.len() != 111 {
            return None;
        }
        Some(UView { sub: &u.base.sub.body, body: &u.body })
    }
    fn who(&self) -> u8 {
        self.sub[0]
    }
    fn o(&self) -> i16 {
        i16::from_le_bytes([self.sub[1], self.sub[2]])
    }
    fn x(&self) -> i32 {
        i32::from_le_bytes(self.sub[7..11].try_into().unwrap()) ^ 0x63637
    }
    fn y(&self) -> i32 {
        i32::from_le_bytes(self.sub[11..15].try_into().unwrap()) ^ 0x63637
    }
    fn ptype(&self) -> i32 {
        i32::from_le_bytes(self.sub[15..19].try_into().unwrap())
    }
    fn body_i32(&self, img: usize) -> i32 {
        i32::from_le_bytes(self.body[img - 0x48..img - 0x44].try_into().unwrap())
    }
    /// `unit_masks` +0x68.
    fn masks(&self) -> u32 {
        self.body_i32(0x68) as u32
    }
    /// `unit_masks2` +0x6c.
    fn masks2(&self) -> u32 {
        self.body_i32(0x6c) as u32
    }
    /// `inside_up` +0x82.
    fn inside_up(&self) -> i16 {
        i16::from_le_bytes([self.body[0x82 - 0x48], self.body[0x83 - 0x48]])
    }
    /// `safe` +0xb2.
    fn safe(&self) -> i8 {
        self.body[0xb2 - 0x48] as i8
    }
}

/// `ObjectType` scalars read through `Rules.types[ptype]` (see
/// `objects_process::TypeImg` for the image split).
#[derive(Clone, Copy, Debug, Default)]
struct TypeInfo {
    /// `+0x218` movement plane: 0 land, 1 sea, 2 air.
    kind: i32,
    /// `+0x248` block radius: unit grid half-size.
    radius: i32,
    /// `+0x2b4` flags (bit 0x10: can use transports).
    flags: u32,
    /// `+0x1e8`.
    f1e8: i32,
}

fn type_i32(t: &TypeRec, off: usize) -> Option<i32> {
    let (v, i) = match off {
        0x04..=0x5d => (&t.head, off - 4),
        0x1e4..=0x27b => (&t.obj_mid, off - 0x1e4),
        0x2b4..=0x2cb => (&t.ext, off - 0x2b4),
        0x2d4.. => (&t.ext, off - 0x2b4 - 8),
        _ => return None,
    };
    v.get(i..i + 4).map(|b| i32::from_le_bytes(b.try_into().unwrap()))
}

fn type_info(save: &Save, ptype: i32) -> TypeInfo {
    let Some(t) = usize::try_from(ptype).ok().and_then(|i| save.rules_tail.rules.types.get(i)) else {
        return TypeInfo::default();
    };
    TypeInfo {
        kind: type_i32(t, 0x218).unwrap_or(0),
        radius: type_i32(t, 0x248).unwrap_or(1),
        flags: type_i32(t, 0x2b4).unwrap_or(0) as u32,
        f1e8: type_i32(t, 0x1e8).unwrap_or(0),
    }
}

/// `LeaderData` reads: `flags` (+0), body i32/i16/u8 at image offsets.
fn leader_flags(save: &Save, who: usize) -> i32 {
    save.leaders.slots.get(who).map(|l| l.flags).unwrap_or(0)
}
fn leader_i32(save: &Save, who: usize, img: usize) -> i32 {
    save.leaders
        .slots
        .get(who)
        .and_then(|l| l.body.get(img - 8..img - 4))
        .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
        .unwrap_or(0)
}
fn leader_i16(save: &Save, who: usize, img: usize) -> i16 {
    save.leaders
        .slots
        .get(who)
        .and_then(|l| l.body.get(img - 8..img - 6))
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .unwrap_or(0)
}
fn leader_u8(save: &Save, who: usize, img: usize) -> u8 {
    save.leaders.slots.get(who).and_then(|l| l.body.get(img - 8)).copied().unwrap_or(0)
}

/// `LeaderData::is_ally(this = leaders[who], other)` 0x006edb50.
fn is_ally(save: &Save, who: usize, other: usize) -> bool {
    if other != leader_i32(save, who, 8) as usize
        && (leader_i32(save, who, 0x74 + other * 4) != 2 || leader_i32(save, other, 0x74 + who * 4) != 2)
    {
        return false;
    }
    true
}

/// `Game::info.reveal_map` (Game+0x30 = GameInfo settings[12]).
fn reveal_map(save: &Save) -> u8 {
    save.info.settings.get(12).copied().unwrap_or(0)
}

/// `WorldData::was_seen(fog_x, fog_y, who)` 0x006b53f0.
fn was_seen(save: &Save, w: &Wld, fx: i32, fy: i32, who: usize) -> bool {
    if who > 7 || reveal_map(save) > 1 {
        return true;
    }
    let lf = leader_flags(save, who);
    if lf & 0x1000 != 0 || lf & 0x800 != 0 || leader_i16(save, who, 0x59e4) != 0 {
        return true;
    }
    let (wx, wy) = (fx >> 1, fy >> 1);
    let owner = w.wwho(wx, wy);
    if owner >= 0 && is_ally(save, who, owner as usize) {
        let r = w.wregion(wx, wy, false) as i32;
        if r >= 0 {
            let off = (r * 2) as usize;
            if leader_i16(save, owner as usize, 0x125e + off) != 0 || leader_i16(save, owner as usize, 0x12de + off) != 0 {
                return true;
            }
        }
    }
    let i = (w.fog_xs * fy + fx) as usize;
    let seen = w.seen2.get(i).copied().unwrap_or(0);
    seen & leader_u8(save, who, 0x6929) != 0
}

// ---------------------------------------------------------------------------
// PathNode and the containers
// ---------------------------------------------------------------------------

/// `class PathNode` (36 B): `x y length estimate value timeout metric z_val
/// transport building parent`.
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

/// `Tree<PathNode*,int>` — unbalanced BST. `ordered_insert` 0x004796f0
/// descends right while `node.key < key` and left otherwise (equal keys go
/// LEFT); `first_open_node` takes the leftmost; `remove_current`
/// 0x00479770 splices a two-child node by hanging its right subtree under
/// the rightmost node of its left subtree and promoting the left child.
#[derive(Clone, Debug, Default)]
struct Tree {
    nodes: Vec<TNode>,
    free: Vec<u32>,
    head: Option<u32>,
    count: i32,
}

#[derive(Clone, Debug, Default)]
struct TNode {
    left: Option<u32>,
    right: Option<u32>,
    parent: Option<u32>,
    data: u32,
    key: i32,
}

impl Tree {
    fn clear(&mut self) {
        self.nodes.clear();
        self.free.clear();
        self.head = None;
        self.count = 0;
    }
    fn alloc(&mut self, n: TNode) -> u32 {
        if let Some(id) = self.free.pop() {
            self.nodes[id as usize] = n;
            id
        } else {
            self.nodes.push(n);
            (self.nodes.len() - 1) as u32
        }
    }
    fn insert(&mut self, data: u32, key: i32) -> u32 {
        let id = self.alloc(TNode { left: None, right: None, parent: None, data, key });
        match self.head {
            None => self.head = Some(id),
            Some(root) => {
                let mut cur = root;
                loop {
                    let n = &self.nodes[cur as usize];
                    if n.key < key {
                        match n.right {
                            Some(r) => cur = r,
                            None => {
                                self.nodes[cur as usize].right = Some(id);
                                break;
                            }
                        }
                    } else {
                        match n.left {
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
    /// `Tree<PathNode*,int>::remove_current` 0x00479770 with `current = id`.
    fn remove(&mut self, id: u32) {
        let Some(head) = self.head else { return };
        let h = &self.nodes[head as usize];
        if h.left.is_none() && h.right.is_none() {
            self.free.push(head);
            self.head = None;
            self.count = 0;
            return;
        }
        let (l, r, p) = {
            let n = &self.nodes[id as usize];
            (n.left, n.right, n.parent)
        };
        match (l, r) {
            (None, None) => self.replace_child(p, id, None),
            (None, Some(c)) | (Some(c), None) => {
                self.replace_child(p, id, Some(c));
                self.nodes[c as usize].parent = p;
            }
            (Some(l), Some(r)) => {
                let mut rm = l;
                while let Some(rr) = self.nodes[rm as usize].right {
                    rm = rr;
                }
                self.nodes[rm as usize].right = Some(r);
                self.nodes[r as usize].parent = Some(rm);
                self.replace_child(p, id, Some(l));
                self.nodes[l as usize].parent = p;
            }
        }
        self.free.push(id);
        self.count -= 1;
    }
}

/// A `CollBlock` clone in `PathFinder::blocklist` (`fill_slots` with the
/// overlay installs one per candidate wcell the first time it is touched;
/// `None` bytes = the fresh empty block retail constructs when the real
/// lookup yields nothing).
#[derive(Clone, Debug, Default)]
struct BlockClone {
    bits: Vec<u8>,
}

impl BlockClone {
    fn empty(&self) -> bool {
        self.bits.iter().all(|&b| b == 0)
    }
    /// `CollBlock::get` 0x00681e30: bit `(ux mod 16) * 16 + (uy mod 16)`.
    fn get(&self, ux: i32, uy: i32) -> bool {
        let i = (ux.rem_euclid(16) * 16 + uy.rem_euclid(16)) as usize;
        self.bits.get(i >> 3).map(|b| b & (1u8 << (i & 7)) != 0).unwrap_or(false)
    }
}

/// The scalar half of a parked search (`UnitData+0x118..+0x148`).
#[derive(Clone, Debug, Default)]
struct Parked {
    nodes: Vec<PathNode>,
    open: Tree,
    open_refs: BTreeMap<u32, u32>,
    closed: BTreeMap<u32, u32>,
    valid: BTreeMap<u32, bool>,
    blocks: BTreeMap<i32, BlockClone>,
    /// +0x128 `tol`, +0x12c `offset` (direction seed), +0x130 `start_dist`,
    /// +0x134 `valid_hit`, +0x138/+0x13c `avoid_land/sea`, +0x140/+0x144
    /// `endx/endy`, +0x148 `traversed`.
    tol: i32,
    offset: i32,
    start_dist: i32,
    valid_hit: i32,
    avoid_land: i32,
    avoid_sea: i32,
    endx: i32,
    endy: i32,
    traversed: i32,
}

/// Scratch state of the `pathfinder` singleton that is not walked.
#[derive(Clone, Debug, Default)]
pub struct PathFinder {
    nodes: Vec<PathNode>,
    open: Tree,
    /// `openlistrefs`: metric → open-tree node (BRTree with tombstones ≡ map).
    open_refs: BTreeMap<u32, u32>,
    /// `closedlist`: metric → node.
    closed: BTreeMap<u32, u32>,
    /// `validlist`: metric → `valid_ucoord` answer.
    valid: BTreeMap<u32, bool>,
    /// `blocklist`: wcell index → `CollBlock` clone.
    blocks: BTreeMap<i32, BlockClone>,
    /// Searches parked by `astar_path`'s `return -1` arm, by `(who, o)`.
    parked: HashMap<(u8, i16), Parked>,
    /// Non-retail counters for the lane tests.
    pub last_expanded: i32,
    pub collide_queries: u64,
}

impl PathFinder {
    pub fn new() -> Self {
        Self::default()
    }
    /// `PathFinder::kill_lists` 0x00687ae0.
    pub fn kill_lists(&mut self) {
        self.nodes.clear();
        self.open.clear();
        self.open_refs.clear();
        self.closed.clear();
        self.valid.clear();
        self.blocks.clear();
    }
    fn alloc(&mut self, n: PathNode) -> u32 {
        self.nodes.push(n);
        (self.nodes.len() - 1) as u32
    }
    /// `PathFinder::first_open_node` 0x00687970: leftmost of `openlist`,
    /// removed from both `openlist` and (by tombstone) `openlistrefs`.
    fn first_open_node(&mut self) -> u32 {
        let tid = self.open.leftmost().expect("first_open_node on empty open list");
        let node = self.open.nodes[tid as usize].data;
        let metric = self.nodes[node as usize].metric;
        if let Some(&t) = self.open_refs.get(&metric) {
            if t == tid {
                self.open_refs.remove(&metric);
            } else {
                // Retail tombstones whatever node the metric seeks to.
                self.open_refs.remove(&metric);
            }
        }
        self.open.remove(tid);
        node
    }
    /// `PathFinder::add_to_openlist` 0x00687aa0.
    fn add_to_openlist(&mut self, node: u32) {
        let n = self.nodes[node as usize];
        let tid = self.open.insert(node, n.value);
        self.open_refs.insert(n.metric, tid);
    }
}

// ---------------------------------------------------------------------------
// Walked `Save.pathfinder` block (pathfinder+0x58..+0xc4)
// ---------------------------------------------------------------------------

mod pf {
    pub const SX: usize = 0; // +0x58
    pub const SY: usize = 1; // +0x5c
    pub const ANTI_UNIT: usize = 3; // +0x64
    pub const OFFX: usize = 4; // +0x68
    pub const OFFY: usize = 5; // +0x6c
    pub const ARMY: usize = 6; // +0x70
    pub const IROQUOIS: usize = 7; // +0x74
    pub const WORKER: usize = 8; // +0x78
    pub const NO_DANGER: usize = 9; // +0x7c
    pub const LIMIT: usize = 10; // +0x80
    pub const SAVING: usize = 11; // +0x84
    pub const AVOID_LAND: usize = 12; // +0x88
    pub const AVOID_SEA: usize = 13; // +0x8c
    pub const VALID_HIT: usize = 14; // +0x90
    pub const SCOUTING: usize = 15; // +0x94
}

fn pf_get(save: &Save, i: usize) -> i32 {
    save.pathfinder.get(i * 4..i * 4 + 4).map(|b| i32::from_le_bytes(b.try_into().unwrap())).unwrap_or(0)
}
fn pf_set(save: &mut Save, i: usize, v: i32) {
    if save.pathfinder.len() < 27 * 4 {
        save.pathfinder.resize(27 * 4, 0);
    }
    save.pathfinder[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
}

/// `GameDaemon::repaths[who]` (`Save.post_world` +0).
fn repaths(save: &Save, who: usize) -> i32 {
    save.post_world.get(who * 4..who * 4 + 4).map(|b| i32::from_le_bytes(b.try_into().unwrap())).unwrap_or(0)
}

/// `Random::get(min, max)` 0x00a39d70 on `GameAccess::game_random`
/// (`Save.post_world` +40). Same body as `game_daemon::game_random`.
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

// ---------------------------------------------------------------------------
// Unit lookup
// ---------------------------------------------------------------------------

fn unit_ref(save: &Save, who: usize, o: usize) -> Option<&crate::sections::Unit> {
    match save.objects.lists.get(who)?.elems.get(o)? {
        Some(Obj::Unit(u)) => Some(u),
        Some(Obj::Animal(a)) => Some(&a.unit),
        _ => None,
    }
}
fn unit_mut(save: &mut Save, who: usize, o: usize) -> Option<&mut crate::sections::Unit> {
    match save.objects.lists.get_mut(who)?.elems.get_mut(o)? {
        Some(Obj::Unit(u)) => Some(u),
        Some(Obj::Animal(a)) => Some(&mut a.unit),
        _ => None,
    }
}

/// Everything about the pathing unit the search reads (`pathfinder+0x54`).
#[derive(Clone, Copy, Debug)]
struct PUnit {
    who: u8,
    o: i16,
    x: i32,
    y: i32,
    masks: u32,
    masks2: u32,
    inside_up: i16,
    safe: i8,
    ty: TypeInfo,
    /// Current order: concrete `OrderIndex` and `MoveOrder::attempts`
    /// (+0x20) when it is a MoveOrder.
    order_ty: Option<i32>,
    order_attempts: i32,
}

impl PUnit {
    fn load(save: &Save, who: usize, o: usize) -> Option<Self> {
        let u = unit_ref(save, who, o)?;
        let v = UView::of(u)?;
        let (order_ty, order_attempts) = match u.orders.orders.first() {
            Some(ord) if (1..=4).contains(&ord.ty) && ord.payload.len() >= 33 => {
                (Some(ord.ty), i32::from_le_bytes(ord.payload[29..33].try_into().unwrap()))
            }
            Some(ord) => (Some(ord.ty), 0),
            None => (None, 0),
        };
        Some(PUnit {
            who: v.who(),
            o: v.o(),
            x: v.x(),
            y: v.y(),
            masks: v.masks(),
            masks2: v.masks2(),
            inside_up: v.inside_up(),
            safe: v.safe(),
            ty: type_info(save, v.ptype()),
            order_ty,
            order_attempts,
        })
    }
    /// `UnitData::can_transport` 0x0046f960 — the unit may board boats.
    fn can_transport(&self) -> bool {
        (self.masks & 0x80_0000 != 0 && self.masks2 & 0x2000 == 0) || self.ty.flags & 0x10 != 0
    }
    /// Current order answers the is-move virtual (+0x14).
    /// TODO(0x00682f30/0x00683770): the predicate is a vtable slot; mapped
    /// here to the MoveOrder family `OrderIndex 1..=4` (77-byte payloads).
    fn order_is_move(&self) -> bool {
        matches!(self.order_ty, Some(1..=4))
    }
}

// ---------------------------------------------------------------------------
// Collision: fill_slots / collide_here / detect_unit_collision
// ---------------------------------------------------------------------------

/// `CollCheck::fill_slots(ux, uy, size, overlay)` 0x006820e0 resolved
/// against the pathfinder's `blocklist` overlay (always installed on the
/// A* path: `detect_unit_collision` passes `param_6 = 1`).
///
/// Returns, per slot, the clone to test (`None` = slot invalid or absent).
fn fill_slots<'p>(pf: &'p mut PathFinder, w: &Wld, ux: i32, uy: i32, size: i32) -> [Option<&'p BlockClone>; 4] {
    let bx0 = (ux - size) >> 4;
    let by0 = (uy - size) >> 4;
    let spans_x = ((ux + size) >> 4) != bx0;
    let spans_y = ((uy + size) >> 4) != by0;
    let mut valid = [true, spans_y, spans_x, spans_x && spans_y];
    let mut skip = [false; 4];
    for i in 0..4 {
        let bx = bx0 + BLOCK_DX[i];
        let by = by0 + BLOCK_DY[i];
        if !valid[i] || bx < 0 || by < 0 || bx >= w.xs || by >= w.ys {
            skip[i] = true;
            valid[i] = false;
        }
    }
    // Region gate anchored on the centre cell (`get_tregion` on tile ux>>2, uy>>2).
    let region = {
        let (cbx, cby) = (ux >> 4, uy >> 4);
        let second = w.wflags(cbx, cby) & 0x100 != 0 && w.tmask(ux >> 2, uy >> 2) & 0x30 == 0x20;
        w.wregion(cbx, cby, second)
    };
    let mut keys = [0i32; 4];
    for i in 0..4 {
        keys[i] = (by0 + BLOCK_DY[i]) * w.xs + bx0 + BLOCK_DX[i];
    }
    // Overlay pass 1: a clone already in the tree wins and suppresses the real lookup.
    for i in 0..4 {
        if valid[i] && pf.blocks.contains_key(&keys[i]) {
            skip[i] = true;
        }
    }
    // Real lookup for the remaining candidates, then clone into the tree.
    for i in 0..4 {
        if skip[i] {
            continue;
        }
        let wi = keys[i] as usize;
        let real = if region < 0 || w.wregion(bx0 + BLOCK_DX[i], by0 + BLOCK_DY[i], false) == region {
            w.block(wi)
        } else {
            None
        };
        let clone = BlockClone { bits: real.map(|b| b.to_vec()).unwrap_or_default() };
        if valid[i] {
            pf.blocks.insert(keys[i], clone);
        }
    }
    let mut out: [Option<&BlockClone>; 4] = [None; 4];
    for i in 0..4 {
        if valid[i] {
            out[i] = pf.blocks.get(&keys[i]);
        }
    }
    out
}

/// `CollCheck::collide_here(o, who, ux, uy, size, &hit_x, &hit_y, overlay = 1)`
/// 0x00682540, general arm (the adjacent-step fast path is gated on
/// `overlay == 0` and never runs from the pathfinder).
fn collide_here(pf: &mut PathFinder, w: &Wld, unit: &PUnit, ux: i32, uy: i32, size: i32) -> bool {
    if size == 0 {
        return false;
    }
    pf.collide_queries += 1;
    let slots = fill_slots(pf, w, ux, uy, size);
    let absent: [bool; 4] = std::array::from_fn(|i| slots[i].map(|b| b.empty()).unwrap_or(true));
    if absent.iter().all(|&a| a) {
        return false;
    }
    let only0 = absent[1] && absent[2] && absent[3];
    let bx0 = (ux - size) >> 4;
    let by0 = (uy - size) >> 4;
    let own_ux = ucell(unit.x);
    let own_uy = ucell(unit.y);
    // Virtual +0xbc default (`UnitData` 0x0046ce30): `inside_up >> 15`.
    let exclude_own = (unit.inside_up as u16 >> 15) != 0;
    let n = RING_COUNT.get(size as usize).copied().unwrap_or(441);
    for k in 0..n as usize {
        let (rx, ry) = (MOVE_X[k], MOVE_Y[k]);
        if (rx + size) & 1 != 0 || (ry + size) & 1 != 0 {
            continue;
        }
        let cx = rx + ux;
        let cy = ry + uy;
        if exclude_own && (cx - own_ux).abs() <= size && (cy - own_uy).abs() <= size {
            continue;
        }
        if !only0 {
            let a = if (cx >> 4) <= bx0 { 0 } else { 2 };
            let b = if (cy >> 4) <= by0 { 0 } else { 1 };
            let s = a + b;
            if !absent[s] && slots[s].map(|bl| bl.get(cx, cy)).unwrap_or(false) {
                return true;
            }
        } else if (cx >> 4) <= bx0 && (cy >> 4) <= by0 && slots[0].map(|bl| bl.get(cx, cy)).unwrap_or(false) {
            return true;
        }
    }
    false
}

/// `Unit::detect_unit_collision(x, y, 1, 1, _, 1, 0)` 0x00617060 — the
/// pathfinder arm (`param_3 != 0` returns 1 right after `collide_here`).
fn detect_unit_collision_path(pf: &mut PathFinder, w: &Wld, unit: &PUnit, top_flags: Option<i32>, x: i32, y: i32) -> bool {
    if unit.ty.kind == 2 {
        return false;
    }
    // LAB_00617119: a top path record with flags & 8 skips the test.
    if let Some(f) = top_flags {
        if f & 8 != 0 {
            return false;
        }
    }
    if unit.safe != 0 {
        return false;
    }
    let (cx, cy) = (ucell(x), ucell(y));
    if cx == ucell(unit.x) && cy == ucell(unit.y) {
        return false;
    }
    collide_here(pf, w, unit, cx, cy, unit.ty.radius)
}

/// `UnitData::invalid_loc(tx, ty, p3, p4 = 1, p5, p6, p7)` 0x00607c30.
/// Non-zero = the unit may not stand on the tile.
fn invalid_loc(save: &Save, w: &Wld, unit: &PUnit, top_flags: Option<i32>, tx: i32, ty: i32, p3: i32, p5: i32, mut p6: i32, p7: i32) -> i32 {
    if let Some(f) = top_flags {
        if f & 4 != 0 {
            p6 = 1;
        }
    }
    if tx < 0 || ty < 0 || tx >= w.tile_xs || ty >= w.tile_ys {
        return 1;
    }
    let (wx, wy) = (tx >> 2, ty >> 2);
    // Human owner: four unseen fog cells make the tile valid regardless.
    if leader_flags(save, unit.who as usize) & 4 != 0 {
        let (fx, fy) = (wx * 2, wy * 2);
        let who = unit.who as usize;
        if !was_seen(save, w, fx, fy, who)
            && !was_seen(save, w, fx, fy + 1, who)
            && !was_seen(save, w, fx + 1, fy, who)
            && !was_seen(save, w, fx + 1, fy + 1, who)
        {
            return 0;
        }
    }
    let td = w.tmask(tx, ty);
    let m = td & 0x30;
    match unit.ty.kind {
        1 => {
            // Sea unit.
            if m == 0x20 {
                // TODO(0x00607c30 +0xd0..): `type+0x1e8 == 0` consults a
                // virtual (`+0xb8`, default `type->+0x60`); untranscribed.
                if td & 0x2400 != 0 {
                    return 3;
                }
            } else {
                if p6 == 0 && p7 == 0 {
                    return 2;
                }
                if !unit.can_transport() {
                    return 2;
                }
                // `wdata[xs*wy + (hi dword of can_transport)]` — retail reads
                // the EDX half of a bool return here; treated as this wcell.
                if w.wflags(wx, wy) & 0x70 != 0 {
                    return 2;
                }
            }
        }
        0 => {
            let wf = w.wflags(wx, wy);
            if wf & 0x70 != 0 && ((wf & 0x20 == 0 || unit.masks2 & 0x4000 == 0) && p3 != 0) && p6 != 0 {
                return 2;
            }
            let cliff = td & 3 == 1;
            if m == 0x30 || td & 3 == 2 || cliff {
                if m != 0x30 {
                    return 2;
                }
                if unit.masks2 & 0x4000 == 0 {
                    return 2;
                }
            }
            if ((p6 == 0 && p7 == 0) || unit.masks & 0x80_0000 == 0) && m == 0x20 {
                return 2;
            }
        }
        2 => return 0,
        _ => {}
    }
    // LAB_00607f50: a built tile the unit is not already standing on.
    if p3 == 0 && td & 0x4000 != 0 && (m != 0x30 || unit.masks2 & 0x4000 == 0) {
        let own = w.tmask(tile(unit.x), tile(unit.y));
        if own & 0x4000 == 0 {
            if unit.ty.f1e8 != 0 && p5 != 0 {
                // TODO(0x00607c30 +0x312): `is_built_at` + `Builds::...`
                // 0x00659180 owner test; `p5 == 0` on every pathfinder call.
            }
            return 4;
        }
    }
    0
}

/// `PathFinder::valid_ucoord(x, y, metric)` 0x00687c80.
fn valid_ucoord(save: &mut Save, pf: &mut PathFinder, unit: &PUnit, top_flags: Option<i32>, x: i32, y: i32, metric: u32) -> bool {
    let w = Wld::new(save);
    if x < 0 || y < 0 || x >= w.tile_xs * 0xc0 || y >= w.tile_ys * 0xc0 {
        return false;
    }
    if let Some(&v) = pf.valid.get(&metric) {
        let hits = pf_get(save, pf::VALID_HIT) + 1;
        pf_set(save, pf::VALID_HIT, hits);
        return v;
    }
    let ok = invalid_loc(save, &w, unit, top_flags, tile(x), tile(y), 0, 0, 1, 0) == 0
        && !detect_unit_collision_path(pf, &w, unit, top_flags, x, y);
    pf.valid.insert(metric, ok);
    ok
}

// ---------------------------------------------------------------------------
// calc_cost
// ---------------------------------------------------------------------------

/// `PathFinder::calc_cost(from, to, dir, step, depth, &transport)` 0x00684e50.
/// `i32::MAX` rejects the edge. Only the `step == 0x30` arm is transcribed:
/// `base = 0x100`, `extra = 0`, then the transport test and
/// `(base * 32) >> 8 + extra + (dir & 1) * 8`.
/// TODO(0x00684e50 +0x95..+0x923): the `0xc0`/`0x300` arms (terrain,
/// danger, borders, buildings, `was_really_seen`) return the unit arm's
/// geometry here.
fn calc_cost(save: &Save, w: &Wld, unit: &PUnit, from_x: i32, from_y: i32, to_x: i32, to_y: i32, dir: u32, step: i32, depth: i32, transport: &mut u8) -> i32 {
    *transport = 0;
    let base = 0x100i32;
    let mut extra = 0i32;
    let (fty, ttx, tty, ftx) = (tile(from_y), tile(to_x), tile(to_y), tile(from_x));
    let avoid_land = pf_get(save, pf::AVOID_LAND);
    let avoid_sea = pf_get(save, pf::AVOID_SEA);
    let can = unit.can_transport();
    let nt = w.needs_transport(ftx, fty, ttx, tty);
    if nt >= 1 && can && depth >= 2 {
        if avoid_sea == 2 {
            return i32::MAX;
        }
        if nt != 1 {
            extra += if avoid_sea == 0 && avoid_land == 0 { 500 } else { 2000 };
        }
        if step == STEP_UNIT {
            extra <<= 2;
        }
        *transport = 1;
    } else if depth == 1 {
        let nt2 = w.needs_transport(pf_get(save, pf::SX), pf_get(save, pf::SY), ttx, tty);
        if nt2 > 0 && can {
            if avoid_sea == 2 {
                return i32::MAX;
            }
            if nt2 != 1 {
                extra += if avoid_sea == 0 && avoid_land == 0 { 0xfa } else { 1000 };
            }
            *transport = 1;
        }
    }
    (base.wrapping_mul(0x20) >> 8) + extra + (dir & 1) as i32 * 8
}

// ---------------------------------------------------------------------------
// astar_path
// ---------------------------------------------------------------------------

/// Outcome of one `astar_path` call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathResult {
    /// Retail return: `1` found (path pushed), `0` failed, `-1` suspended.
    pub status: i32,
    /// `Random::get` draws on `game_random` (0 or 1).
    pub draws: u32,
    /// Node expansions this call (`local_58`).
    pub expanded: i32,
}

/// `PathFinder::astar_path(Stack<PathData>* stack, int step, int quick)`
/// 0x00683770 for the unit at `(who, o)` (`pathfinder.pathing_unit`).
///
/// Stack protocol (unit domain, as `find_upath` leaves it): top = start
/// record, below it the goal record, below that the anchor whose
/// `tolerance` halves into the arrival tolerance. Start and goal are popped;
/// on success the chain is pushed goal-end first (so the record nearest the
/// unit is on top), each `{x, y, 0, 2 | 4·transport | 0x10·building}`,
/// skipping a record equal to the current top.
pub fn astar_path(save: &mut Save, pf: &mut PathFinder, who: usize, o: usize, stack: &mut PStack, step: i32, mut quick: i32) -> PathResult {
    let Some(unit) = PUnit::load(save, who, o) else {
        return PathResult { status: 0, draws: 0, expanded: 0 };
    };
    let mut draws = 0u32;
    let mut expanded = 0i32; // local_58
    let mut budget = 0x32i32; // local_84
    let mut stride = 1i32; // local_80
    let mut unit_size = 1i32; // local_68
    // pathfinder.iroquois = unit_masks2 & 0x4000
    pf_set(save, pf::IROQUOIS, (unit.masks2 & 0x4000) as i32);
    let row_stride; // local_64
    if step == STEP_UNIT {
        budget = 500;
        unit_size = ((unit.ty.radius + 1) / 2).max(1);
        row_stride = save.world.xs << 4;
        stride = (quick != 0) as i32 + 1;
    } else if step == STEP_TILE {
        row_stride = save.world.direct.get(4).copied().unwrap_or(0);
    } else {
        row_stride = save.world.xs;
    }
    let step_scale = unit_size * step; // local_7c
    // pathfinder.no_danger: TODO(0x00683770 +0x6e..+0x8f): `UnitData::get_action`
    // virtual +0x10 == 10 / `order_type` 2 or 0x15; only `who >= 8` is
    // evaluable from the save.
    pf_set(save, pf::NO_DANGER, (unit.who >= 8) as i32);
    // MoveOrder off_x/off_y (local_74/local_78): tile/water waypoint jitter.
    let (mut off_x, mut off_y) = (0i32, 0i32);
    if let Some(u) = unit_ref(save, who, o) {
        if let Some(ord) = u.orders.orders.first() {
            if (1..=4).contains(&ord.ty) && ord.payload.len() >= 77 {
                off_x = i16::from_le_bytes([ord.payload[73], ord.payload[74]]) as i32;
                off_y = i16::from_le_bytes([ord.payload[75], ord.payload[76]]) as i32;
            }
        }
    }

    let (gx, gy, mut tol, seed, mut start_dist, mut traversed);
    if pf_get(save, pf::SAVING) != 0 {
        // Resume: adopt the unit's parked containers.
        let Some(p) = pf.parked.remove(&(unit.who, unit.o)) else {
            return PathResult { status: 0, draws: 0, expanded: 0 };
        };
        pf.nodes = p.nodes;
        pf.open = p.open;
        pf.open_refs = p.open_refs;
        pf.closed = p.closed;
        pf.valid = p.valid;
        pf.blocks = p.blocks;
        pf_set(save, pf::VALID_HIT, p.valid_hit);
        pf_set(save, pf::AVOID_LAND, p.avoid_land);
        pf_set(save, pf::AVOID_SEA, p.avoid_sea);
        gx = p.endx;
        gy = p.endy;
        tol = p.tol;
        seed = p.offset;
        start_dist = p.start_dist;
        traversed = p.traversed;
    } else {
        pf_set(save, pf::VALID_HIT, 0);
        let start = stack.pop();
        let goal = stack.pop();
        gx = goal.to_x;
        gy = goal.to_y;
        // local_60: tolerance of the record below the goal (0 when none).
        tol = if stack.is_empty() { 0 } else { stack.peek().tolerance };
        let (sx, sy) = (start.to_x, start.to_y);
        let metric0 = match step {
            STEP_WATER => (wcell(sx) + wcell(sy) * save.world.xs) as u32,
            STEP_TILE => (tile(sx) + tile(sy) * row_stride) as u32,
            _ => (ucell(sx) + ucell(sy) * save.world.xs * 0x10) as u32,
        };
        let h0 = estimate(sx - gx, sy - gy, step);
        let root = pf.alloc(PathNode { x: sx, y: sy, estimate: h0, value: h0, metric: metric0, ..Default::default() });
        pf.add_to_openlist(root);

        // Start/end classification (avoid_land / avoid_sea).
        let w = Wld::new(save);
        let same_region = if step == STEP_WATER {
            w.wregion(wcell(sx), wcell(sy), false) == w.wregion(wcell(gx), wcell(gy), false)
        } else {
            w.tregion(tile(gx), tile(gy)) == w.tregion(tile(sx), tile(sy))
        };
        if same_region {
            let start_water = if step == STEP_WATER {
                // WorldData::is_ocean 0x006b4830 on the start wcell.
                let f = w.wflags(wcell(sx), wcell(sy));
                let land = w
                    .w_index(wcell(sx), wcell(sy))
                    .and_then(|i| w.wdata.get(i * WDATA_ROW + 2))
                    .map(|&b| b as i8)
                    .unwrap_or(0);
                f & 0x100 == 0 && (land == 1 || land == 2)
            } else {
                w.tmask(tile(sx), tile(sy)) & 0x30 == 0x20
            };
            if unit.ty.flags & 0x10 == 0 || unit.masks & 0x40000 == 0 {
                if start_water {
                    pf_set(save, pf::AVOID_SEA, 0);
                    pf_set(save, pf::AVOID_LAND, 1);
                } else {
                    pf_set(save, pf::AVOID_SEA, 1);
                    pf_set(save, pf::AVOID_LAND, 0);
                    // TODO(0x00683770 +0x3c0): `get_action` virtual +0x10 == 10 → avoid_sea = 2.
                }
            } else {
                pf_set(save, pf::AVOID_SEA, 0);
                pf_set(save, pf::AVOID_LAND, 0);
            }
        } else {
            pf_set(save, pf::AVOID_SEA, 0);
            pf_set(save, pf::AVOID_LAND, 0);
            // TODO(0x00683770 +0x40a): `UnitData::get_order` flags & 0x20 → avoid_land = 1
            // (the store goes through EDX of a 64-bit return; unresolved).
        }
        let dx = sx - gx;
        let dy = sy - gy;
        start_dist = dx.abs() + dy.abs();
        seed = if dy.abs() < dx.abs() { (gx < sx) as i32 * 4 + 3 } else { (sy <= gy) as i32 * 4 + 1 };
        traversed = 0;
    }

    let arrive = tol / 2 + step_scale; // local_24
    let top_flags = stack.recs.last().map(|r| r.flags);

    while pf.open.count != 0 {
        let cur = pf.first_open_node();
        let cn = pf.nodes[cur as usize];
        let manh = (cn.x - gx).abs() + (cn.y - gy).abs();
        let anti_unit = pf_get(save, pf::ANTI_UNIT);
        let limit = pf_get(save, pf::LIMIT);
        let soft = anti_unit != 0 && limit < expanded;
        if manh <= arrive || budget * 0x40 <= traversed + expanded || soft {
            if soft && manh > arrive && quick == 0 {
                // Park the search in the unit and return -1.
                pf.add_to_openlist(cur);
                pf_set(save, pf::SAVING, 1);
                let p = Parked {
                    nodes: std::mem::take(&mut pf.nodes),
                    open: std::mem::take(&mut pf.open),
                    open_refs: std::mem::take(&mut pf.open_refs),
                    closed: std::mem::take(&mut pf.closed),
                    valid: std::mem::take(&mut pf.valid),
                    blocks: std::mem::take(&mut pf.blocks),
                    tol,
                    offset: seed,
                    start_dist,
                    valid_hit: pf_get(save, pf::VALID_HIT),
                    avoid_land: pf_get(save, pf::AVOID_LAND),
                    avoid_sea: pf_get(save, pf::AVOID_SEA),
                    endx: gx,
                    endy: gy,
                    traversed: traversed + expanded,
                };
                pf.parked.insert((unit.who, unit.o), p);
                pf.last_expanded = expanded;
                return PathResult { status: -1, draws, expanded };
            }
            pf_set(save, pf::SAVING, 0);
            let mut transport_tail = false; // bVar15
            if !(traversed + expanded < budget * 0x40 || quick != 0) {
                // Budget exhausted.
                if step == STEP_TILE && anti_unit != 0 {
                    pf.last_expanded = expanded;
                    return PathResult { status: 0, draws, expanded };
                }
                if step == STEP_UNIT {
                    draws += failure_epilogue(save, &unit, who, o);
                    pf.last_expanded = expanded;
                    return PathResult { status: 0, draws, expanded };
                }
                // Tile (no anti_unit) / water: keep the open node nearest the goal.
                let mut best = cur;
                while pf.open.count != 0 {
                    let n = pf.first_open_node();
                    let (bn, nn) = (pf.nodes[best as usize], pf.nodes[n as usize]);
                    let db = vector_dist(bn.x - gx, bn.y - gy);
                    let dn = vector_dist(nn.x - gx, nn.y - gy);
                    if dn < db {
                        best = n;
                    }
                }
                let w = Wld::new(save);
                let bn = pf.nodes[best as usize];
                if w.needs_transport(tile(bn.x), tile(bn.y), tile(gx), tile(gy)) != 0 && unit.can_transport() {
                    let mut r = stack.pop();
                    r.flags |= 4;
                    stack.push(r);
                }
                transport_tail = true;
                return emit_path(save, pf, stack, best, step, quick, transport_tail, off_x, off_y, &unit);
            }
            return emit_path(save, pf, stack, cur, step, quick, transport_tail, off_x, off_y, &unit);
        }

        // Expand the eight directions starting one past the seed.
        let mut d = seed + 1; // local_54
        let mut n = 1i32; // local_5c
        while n < 9 {
            let dir = if d < 9 { d } else { d - 8 };
            let (mx, my) = (MOVE_X[dir as usize], MOVE_Y[dir as usize]);
            let cx = mx * step_scale + cn.x;
            let cy = my * step_scale + cn.y;
            let cmetric = (cn.metric as i32).wrapping_add(mx).wrapping_add(my.wrapping_mul(row_stride)) as u32;
            let ok = if step == STEP_WATER {
                // TODO(0x00687da0): valid_wcoord untranscribed.
                false
            } else if step == STEP_TILE {
                let w = Wld::new(save);
                invalid_loc(save, &w, &unit, top_flags, tile(cx), tile(cy), 0, 1, 1, 0) == 0
            } else if unit_size < 2 || dir & 1 == 0 {
                valid_ucoord(save, pf, &unit, top_flags, cx, cy, cmetric)
            } else {
                // Wide unit, diagonal: sample each intermediate cell. Retail
                // passes a metric in WORLD units here (`mx*48*k + metric +
                // my*48*k*row_stride`); reproduced as-is.
                let (mut ax, mut ay) = (mx * step, my * step);
                let mut good = true;
                let mut k = 1;
                while k <= unit_size {
                    let m = ax.wrapping_add(cn.metric as i32).wrapping_add(ay.wrapping_mul(row_stride)) as u32;
                    if !valid_ucoord(save, pf, &unit, top_flags, cn.x + ax, cn.y + ay, m) {
                        good = false;
                        break;
                    }
                    ax += mx * step;
                    ay += my * step;
                    k += 1;
                }
                good
            };
            if ok {
                expanded += if anti_unit != 0 && step == STEP_TILE { 5 } else { 1 };
                let mut transport = 0u8;
                let cost = {
                    let w = Wld::new(save);
                    calc_cost(save, &w, &unit, cn.x, cn.y, cx, cy, dir as u32, step, cn.timeout + 1, &mut transport)
                };
                if cost != i32::MAX {
                    let cost = if cx == gx && cy == gy { cost / 2 } else { cost };
                    let g = cn.length + cost;
                    let closed = pf.closed.contains_key(&cmetric);
                    let mut reject = closed;
                    if !reject {
                        if let Some(&otid) = pf.open_refs.get(&cmetric) {
                            let onode = pf.open.nodes[otid as usize].data;
                            if pf.nodes[onode as usize].length <= g {
                                reject = true;
                            } else {
                                pf.open.remove(otid);
                                pf.open_refs.remove(&cmetric);
                            }
                        }
                    }
                    if !reject {
                        let h = estimate(cx - gx, cy - gy, step);
                        let f = g + h;
                        let depth = cn.timeout + 1;
                        let depth_ok = !(step == STEP_TILE && ((h + (h >> 31 & 0x1f)) >> 5) + depth > 0x78);
                        if depth_ok {
                            let child = pf.alloc(PathNode {
                                x: cx,
                                y: cy,
                                length: g,
                                estimate: h,
                                value: f,
                                timeout: depth,
                                metric: cmetric,
                                z_val: 0,
                                transport,
                                building: 0,
                                parent: Some(cur),
                            });
                            pf.add_to_openlist(child);
                        }
                    }
                }
            }
            n += stride;
            d += stride;
        }
        pf.closed.insert(cn.metric, cur);
    }

    // Open list exhausted.
    pf_set(save, pf::SAVING, 0);
    if step == STEP_UNIT {
        draws += failure_epilogue(save, &unit, who, o);
    }
    quick = 0;
    let _ = quick;
    pf.last_expanded = expanded;
    PathResult { status: 0, draws, expanded }
}

/// The unit-domain failure epilogue (0x00684e02..0x00684e94, and its twin
/// at 0x006848a0..). Returns the number of `game_random` draws (0 or 1).
fn failure_epilogue(save: &mut Save, unit: &PUnit, who: usize, o: usize) -> u32 {
    let mut draws = 0;
    if unit.order_is_move() && unit.order_attempts < 0xd {
        let r = game_random(save, 0, 0xffff);
        draws = 1;
        if let Some(u) = unit_mut(save, who, o) {
            if let Some(ord) = u.orders.orders.first_mut() {
                if ord.payload.len() >= 29 {
                    ord.payload[25..29].copy_from_slice(&(r % 3 + 6).to_le_bytes());
                }
            }
        }
    }
    if let Some(u) = unit_mut(save, who, o) {
        if u.body.len() == 111 {
            u.body[0xb2 - 0x48] = u.body[0xb2 - 0x48].wrapping_add(0x1e);
        }
    }
    draws
}

/// The success tail 0x00684a5b..0x00684ce8: optional building-flag walk
/// (tile domain), then push the chain from `end` to the root.
fn emit_path(save: &mut Save, pf: &mut PathFinder, stack: &mut PStack, end: u32, step: i32, _quick: i32, transport_tail: bool, off_x: i32, off_y: i32, unit: &PUnit) -> PathResult {
    let en = pf.nodes[end as usize];
    let Some(_) = en.parent else {
        pf.last_expanded = 0;
        return PathResult { status: 1, draws: 0, expanded: pf.last_expanded };
    };
    let mut from = en.parent.unwrap();
    if step != STEP_WATER || transport_tail || en.transport != 0 {
        from = end;
        if step == STEP_TILE {
            let w = Wld::new(save);
            let mut n = end;
            loop {
                let node = pf.nodes[n as usize];
                let td = w.tmask(tile(node.x), tile(node.y));
                if td & 0x4000 != 0 && td & 3 == 3 {
                    pf.nodes[n as usize].building = 1;
                    from = n;
                }
                let Some(p) = node.parent else { break };
                if pf.nodes[p as usize].parent.is_none() {
                    break;
                }
                n = p;
            }
        }
    }
    // local_24: the unit's current order is a move order (virtual +0x14).
    let jitter = unit.order_is_move();
    let anti_unit = pf_get(save, pf::ANTI_UNIT);
    let mut n = Some(from);
    let mut emitted = 0;
    while let Some(id) = n {
        let node = pf.nodes[id as usize];
        if node.parent.is_none() && step != STEP_UNIT {
            break;
        }
        let (mut x, mut y) = (node.x, node.y);
        let tolerance;
        if !jitter {
            tolerance = match step {
                STEP_WATER => 0x180,
                STEP_UNIT => 0,
                _ => tile_tolerance(anti_unit, unit),
            };
        } else if step == STEP_WATER {
            tolerance = 0x180;
            x = x + off_x - 0x180;
            y = y + off_y - 0x180;
        } else if step == STEP_TILE {
            x = x + off_x % 0xc0 - 0x60;
            y = y + off_y % 0xc0 - 0x60;
            tolerance = tile_tolerance(anti_unit, unit);
        } else {
            tolerance = 0;
        }
        let mut flags = if anti_unit == 0 { if step == STEP_UNIT { 2 } else { 0 } } else { 2 };
        if pf.nodes[from as usize].building != 0 {
            flags |= 0x10;
        }
        let top = stack.peek();
        if stack.is_empty() || x != top.to_x || y != top.to_y {
            let mut tol = tolerance;
            if pf.nodes[from as usize].transport != 0 {
                flags |= 4;
                tol = 0;
            }
            stack.push(PathData { to_x: x, to_y: y, tolerance: tol, flags });
            emitted += 1;
        }
        from = id;
        n = node.parent;
    }
    let _ = emitted;
    PathResult { status: 1, draws: 0, expanded: pf.last_expanded }
}

/// LAB_00684bfc: tile-domain tolerance `0` when `anti_unit == 0` and the
/// unit can board transports, else `0x60`.
fn tile_tolerance(anti_unit: i32, unit: &PUnit) -> i32 {
    if anti_unit == 0 && unit.can_transport() {
        0
    } else {
        0x60
    }
}

// ---------------------------------------------------------------------------
// find_upath
// ---------------------------------------------------------------------------

/// Outcome of `find_upath`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UPathResult {
    /// Retail return: `-1` off-map (stack cleared), otherwise the stack
    /// length (`> 0` success) or `0`.
    pub ret: i32,
    pub astar: Option<PathResult>,
    /// Retail calls `Unit::cant_move(0)` 0x005e2cb0 here (untranscribed).
    pub cant_move: bool,
    pub draws: u32,
}

/// `PathFinder::find_upath(Stack<PathData>*, who, o, quick)` 0x00688eb0 →
/// `find_upath(stack, unit.x, unit.y, who, o, quick)` 0x00682f30, operating
/// on the unit's own walked `Stack<PathData>`.
pub fn find_upath(save: &mut Save, pf: &mut PathFinder, who: usize, o: usize, quick: i32) -> UPathResult {
    let fail = UPathResult { ret: 0, astar: None, cant_move: false, draws: 0 };
    let Some(unit) = PUnit::load(save, who, o) else { return fail };
    let Some(u) = unit_ref(save, who, o) else { return fail };
    let mut stack = PStack::from_walked(&u.path);
    // 0x00688eb0: limit = 500 / max(1, repaths[who]^2), halved when quick.
    let rp = repaths(save, who);
    let sq = (rp * rp).max(1);
    let mut limit = (500i64 / sq as i64) as i32;
    if quick != 0 {
        limit /= 2;
    }
    pf_set(save, pf::LIMIT, limit);
    let r = find_upath_inner(save, pf, &unit, &mut stack, unit.x, unit.y, quick);
    pf_set(save, pf::IROQUOIS, 0); // DAT_00e85ec4 = 0 after the inner call (0x00688eb0 tail)
    if let Some(u) = unit_mut(save, who, o) {
        stack.write_walked(&mut u.path);
    }
    r
}

fn find_upath_inner(save: &mut Save, pf: &mut PathFinder, unit: &PUnit, stack: &mut PStack, px: i32, py: i32, quick: i32) -> UPathResult {
    let xs = save.world.xs;
    let ys = save.world.ys;
    pf_set(save, pf::SCOUTING, 0);
    pf_set(save, pf::SX, tile(px));
    pf_set(save, pf::SY, tile(py));
    pf_set(save, pf::OFFX, 0);
    pf_set(save, pf::OFFY, 0);
    let mut res = UPathResult { ret: 0, astar: None, cant_move: false, draws: 0 };
    if pf_get(save, pf::SAVING) == 0 {
        let (ux, uy) = (ucell(px), ucell(py));
        let dest = stack.pop();
        let (mut bx, mut by) = (dest.to_x, dest.to_y);
        let (mut cx, mut cy) = (ucell(bx), ucell(by));
        if cx < 0 || cy < 0 || xs * 0x10 <= cx || ys * 0x10 <= cy {
            stack.recs.clear();
            res.ret = -1;
            return res;
        }
        let mut rec = dest;
        if ux == cx && uy == cy {
            rec.tolerance = 0;
            stack.push(rec);
            res.ret = stack.len();
            return res;
        }
        if unit.ty.kind < 2 && !unit.can_transport() {
            // Straight-line probe: pull the destination toward the unit in
            // 24-unit steps until it is a valid cell in the unit's region.
            let mut found = false;
            loop {
                let w = Wld::new(save);
                let same = w.tregion(cx >> 2, cy >> 2) == w.tregion(ux >> 2, uy >> 2);
                drop(w);
                if same {
                    let metric = (xs * cy * 0x10 + cx) as u32;
                    if valid_ucoord(save, pf, unit, stack.recs.last().map(|r| r.flags), bx, by, metric) {
                        found = true;
                        break;
                    }
                }
                if (px - bx).abs() < 0x18 && (py - by).abs() < 0x18 {
                    if dest.flags & 1 == 0 {
                        res.ret = 0;
                        return res;
                    }
                    // break → push the probe record.
                    rec.to_x = bx;
                    rec.to_y = by;
                    stack.push(rec);
                    res.ret = stack.len();
                    return res;
                }
                let angle = find_angle(px - bx, py - by);
                bx = bx.wrapping_add(sin_fold(angle, 0x18));
                by = by.wrapping_sub(sin_fold(angle.wrapping_add(0x4000_0000), 0x18));
                cx = ucell(bx);
                cy = ucell(by);
                rec.to_x = bx;
                rec.to_y = by;
                if ux == cx && uy == cy {
                    stack.push(rec);
                    res.ret = stack.len();
                    return res;
                }
            }
            let _ = found;
        }
        // LAB_00683250
        if (ux - cx).abs() + (uy - cy).abs() < 2 {
            rec.tolerance = 0;
            stack.push(rec);
            res.ret = stack.len();
            return res;
        }
        stack.push(rec);
        stack.push(PathData { to_x: cx * 0x30 + 0x18, to_y: cy * 0x30 + 0x18, tolerance: rec.tolerance, flags: 0 });
        stack.push(PathData { to_x: ux * 0x30 + 0x18, to_y: uy * 0x30 + 0x18, tolerance: 0, flags: 0 });
    }
    pf_set(save, pf::OFFX, 0);
    pf_set(save, pf::OFFY, 0);
    pf_set(save, pf::ANTI_UNIT, 1);
    let ar = astar_path(save, pf, unit.who as usize, unit.o as usize, stack, STEP_UNIT, quick);
    pf_set(save, pf::IROQUOIS, 0);
    pf_set(save, pf::ANTI_UNIT, 0);
    res.astar = Some(ar);
    res.draws = ar.draws;
    let mut ret = ar.status;
    if ret < 1 {
        if pf_get(save, pf::SAVING) == 0 {
            ret = 0;
            // Drop the top record unless its flags & 1.
            let top = stack.peek();
            if stack.is_empty() || top.flags & 1 == 0 {
                stack.pop();
            }
            // MoveOrder::retry != 0 skips cant_move.
            let retry = unit_ref(save, unit.who as usize, unit.o as usize)
                .and_then(|u| u.orders.orders.first())
                .filter(|ord| (1..=4).contains(&ord.ty) && ord.payload.len() >= 29)
                .map(|ord| i32::from_le_bytes(ord.payload[25..29].try_into().unwrap()))
                .unwrap_or(0);
            if !(unit.order_is_move() && retry != 0) {
                res.cant_move = true;
            }
        }
    } else {
        // Collinear compression of the freshly pushed chain.
        if stack.len() > 3 {
            let mut top = stack.pop();
            if top.to_x == unit.x && top.to_y == unit.y && top.flags & 1 == 0 {
                top = stack.pop();
            }
            let (mut prev_x, mut prev_y) = (top.to_x, top.to_y);
            let top_flags = top.flags;
            stack.push(top);
            if top_flags & 2 != 0 && stack.len() > 2 {
                // 0x006834f0..0x006836a0: walk the chain top-down keeping a
                // record when it is a transport leg, or when the step into it
                // differs from the step out of it AND it is not a one-cell
                // corner cut (|prev - next| == 48 on both axes). `prev` is
                // the record above in the ORIGINAL chain whenever the drop was
                // for collinearity; a corner-cut drop leaves `prev` alone
                // (the short-circuit assignments in the retail condition).
                let mut keep: Vec<PathData> = Vec::new();
                let mut cur = stack.pop();
                let mut next = stack.peek();
                while cur.flags & 2 != 0 && next.flags & 2 != 0 && stack.len() > 1 {
                    let (mut np_x, mut np_y) = (prev_x, prev_y);
                    let keep_it = if cur.flags & 4 != 0 {
                        true
                    } else {
                        let dx_equal = prev_x - cur.to_x == cur.to_x - next.to_x;
                        let mut first = !dx_equal;
                        if dx_equal {
                            np_x = cur.to_x;
                            np_y = cur.to_y;
                            first = prev_y - cur.to_y != cur.to_y - next.to_y;
                        }
                        if !first {
                            false
                        } else if (prev_x - next.to_x).abs() != 0x30 {
                            true
                        } else {
                            np_x = prev_x;
                            np_y = prev_y;
                            (prev_y - next.to_y).abs() != 0x30
                        }
                    };
                    if keep_it {
                        keep.push(cur);
                        prev_x = cur.to_x;
                        prev_y = cur.to_y;
                    } else {
                        prev_x = np_x;
                        prev_y = np_y;
                    }
                    cur = stack.pop();
                    next = stack.peek();
                }
                stack.push(cur);
                while let Some(r) = keep.pop() {
                    stack.push(r);
                }
            }
        }
        ret = stack.len();
    }
    res.ret = ret;
    pf.kill_lists();
    res
}

#[cfg(test)]
mod tests {
    use super::*;
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
            let Some(frame) = seg.trim_start().split(|c: char| !c.is_ascii_digit()).next().and_then(|t| t.parse::<i64>().ok()) else { continue };
            let save = seg.split("\"save_name\":").nth(1).and_then(|s| s.split('"').nth(1)).unwrap_or_default().to_string();
            out.push((frame, save));
        }
        out
    }

    fn fmt_recs(recs: &[PathData]) -> String {
        recs.iter().map(|r| format!("({},{} t{} f{:x})", r.to_x, r.to_y, r.tolerance, r.flags)).collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn trig_matches_binary_table() {
        // trig_init anchors: T[0]=0, T[1]=403, T[128]=46197, T[255]=65535.
        assert_eq!(sin_quarter(0), 0);
        assert_eq!(sin_quarter(1), 403);
        assert_eq!(sin_quarter(128), 46197);
        assert_eq!(sin_quarter(255), 65535);
        // find_angle axes.
        assert_eq!(find_angle(0, -5), 0);
        assert_eq!(find_angle(5, 0), 0x4000_0000);
        assert_eq!(find_angle(-5, 0), -0x4000_0000);
        assert_eq!(find_angle(0, 5), i32::MIN);
        assert_eq!(vector_dist(3, 4), 4 + 9 / 8);
        assert_eq!(t3(-1), -1);
        assert_eq!(t3(-4), -2);
    }

    #[test]
    fn tree_insert_equal_goes_left_and_remove_splices_predecessor_side() {
        let mut t = Tree::default();
        let a = t.insert(0, 5);
        let b = t.insert(1, 5);
        let c = t.insert(2, 3);
        let _d = t.insert(3, 9);
        // leftmost is c (3); among equal 5s the later insert (b) is left of a.
        assert_eq!(t.leftmost(), Some(c));
        t.remove(c);
        assert_eq!(t.leftmost(), Some(b));
        t.remove(b);
        assert_eq!(t.leftmost(), Some(a));
        // Two-child removal of the root: left child promoted, right subtree
        // hung under the rightmost of the left subtree.
        let mut t = Tree::default();
        let r = t.insert(0, 10);
        let l = t.insert(1, 5);
        let rr = t.insert(2, 15);
        let lr = t.insert(3, 7);
        t.remove(r);
        assert_eq!(t.head, Some(l));
        assert_eq!(t.nodes[lr as usize].right, Some(rr));
        assert_eq!(t.nodes[rr as usize].parent, Some(lr));
        assert_eq!(t.count, 3);
    }

    /// Oracle: every stride-1 pair in which a unit's walked path gained
    /// unit-domain records (`flags & 2`) on top of an unchanged prefix.
    /// `find_upath` from frame N must reproduce frame N+1's stack exactly,
    /// or exactly minus the top record (retail's `move_step` may pop the
    /// first waypoint in the same frame — accepted only when the unit's
    /// N+1 position IS that popped waypoint).
    #[test]
    fn unit_path_matches_retail_next_frame() {
        let dirs = capture_dirs();
        if dirs.is_empty() {
            eprintln!("no captures; skipping");
            return;
        }
        let mut checked = 0;
        let mut exact = 0;
        let mut failures = Vec::new();
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
                for who in 0..a.objects.lists.len() {
                    for o in 0..a.objects.lists[who].elems.len().min(b.objects.lists[who].elems.len()) {
                        let (Some(ua), Some(ub)) = (unit_ref(&a, who, o), unit_ref(&b, who, o)) else { continue };
                        let pa = PStack::from_walked(&ua.path);
                        let pb = PStack::from_walked(&ub.path);
                        // Fresh unit search: B = A's records + new flags&2 records.
                        if pb.len() <= pa.len() || pb.recs[..pa.recs.len()] != pa.recs[..] {
                            continue;
                        }
                        if pa.recs.iter().any(|r| r.flags & 2 != 0) || !pb.recs[pa.recs.len()..].iter().all(|r| r.flags & 2 != 0) {
                            continue;
                        }
                        let Some(vb) = UView::of(ub) else { continue };
                        let mut ours = a.clone();
                        let mut pf = PathFinder::new();
                        let r = find_upath(&mut ours, &mut pf, who, o, 0);
                        let got = PStack::from_walked(&unit_ref(&ours, who, o).unwrap().path);
                        checked += 1;
                        let popped_ok = got.recs.len() == pb.recs.len() + 1
                            && got.recs[..pb.recs.len()] == pb.recs[..]
                            && got.recs.last().map(|t| t.to_x == vb.x() && t.to_y == vb.y()).unwrap_or(false);
                        if got.recs == pb.recs && got.cap == pb.cap {
                            exact += 1;
                        } else if popped_ok && got.cap == pb.cap {
                            exact += 1;
                        } else {
                            failures.push(format!(
                                "{} f{fa}->f{fb} who{who} o{o} ret {:?} expanded {}\n   A    {}\n   want cap{} {}\n   got  cap{} {}",
                                dir.file_name().unwrap().to_string_lossy(),
                                r.ret,
                                r.astar.map(|x| x.expanded).unwrap_or(-1),
                                fmt_recs(&pa.recs),
                                pb.cap,
                                fmt_recs(&pb.recs),
                                got.cap,
                                fmt_recs(&got.recs)
                            ));
                        }
                    }
                }
            }
        }
        for f in &failures {
            eprintln!("MISMATCH {f}");
        }
        eprintln!("unit searches checked {checked}, exact {exact}, mismatched {}", failures.len());
        assert!(checked > 0, "no fresh unit searches in the captures");
        assert!(failures.is_empty(), "{} of {checked} unit searches diverge from retail", failures.len());
    }
}
