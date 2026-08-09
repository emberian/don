//! Unit collision — the occupancy bitmap, the blocker query, and the local detour.
//!
//! Lane: `mech:collision`. Serves the **`units`** and **`world`** checksum channels: the
//! blocker fields this module writes (`UnitData+0x88/0x8a/0x8c/0x48/0xb3`) are walked by
//! `Unit::walk_data` `0x0060CF40`, and the `CollBlock` payloads it stamps are walked by
//! `World::walk_data` section 9 (`map_terrain::WorldSection::CollBlocks`).
//!
//! Everything here is derived from `ron-bin/riseofnations.exe` + `ron-bin/sbl/rise.pdb`.
//! Claims are marked **[measured]** (read here, at instruction or decompiled-C level) or
//! **UNVERIFIED** (structure taken from Ghidra output and not checked against behaviour).
//! No differential test against retail has been run for this lane, so every fidelity claim
//! is **tier C**. Nothing here is "verified" in the proof-assistant sense.
//!
//! # The substrate, in one paragraph
//!
//! Collision is **not** geometry between unit bodies. It is a **bitmap**. Every `WData` cell
//! (one `WCoord`, 768 world units, 4 tiles) owns a lazily allocated `CollBlock`
//! (`BitMask<768>`, `map_terrain::CollBlock`) whose first 256 bits are a 16x16 grid of
//! `UCoord` cells — 48 world units each, the same quarter-tile grid the unit pathfinder walks.
//! `Guy::set_new_location` `0x005D86F0` stamps a solid `(2r+1)^2` square of those bits, where
//! `r = ObjectTypeData+0x248 new_block_radius` [measured]. `CollCheck::collide_here`
//! `0x00682540` asks whether a footprint placed at a candidate cell would overlap a set bit.
//! `Unit::detect_unit_collision` `0x00617060` then walks the world's `down` object list to
//! find *which* unit owns that bit and whether it is one this unit must stop for.
//! `Unit::resolve_unit_collision` `0x005F9D30` reacts: wait, sidestep, or repath.
//!
//! # Provenance index
//!
//! | symbol | VA | size | status here |
//! |---|---|---:|---|
//! | `CollBlock::get` | `0x00681E30` | 74 | ported exact |
//! | `CollBlock::set` | `0x00682070` | 103 | ported exact |
//! | `BitMask<768>::empty` | `0x00479150` | 70 | ported exact (it mutates `flags`) |
//! | `CollCheck::fill_slots` | `0x006820E0` | 1109 | ported, including scratch clones |
//! | `CollCheck::collide_here` | `0x00682540` | 1419 | ported, all three arms |
//! | `CollCheck::move_unit` | `0x00682AD0` | 1077 | ported |
//! | `GameDaemon::process_coll_blocks` | `0x00731F90` | 204 | ported exact |
//! | `Guy::set_new_location` (coll part) | `0x005D86F0` | 899 | the `move_unit` call ported |
//! | `Unit::detect_unit_collision` | `0x00617060` | 2410 | ported; see §gaps for the cut arms |
//! | `Unit::resolve_unit_collision` | `0x005F9D30` | 2943 | detour/wait/repath ported |
//! | `UnitData::will_be_corner` | `0x00609FA0` | 153 | ported exact |
//! | `UnitData::is_here` | `0x0060A0C0` | 113 | ported exact |
//! | `UnitData::is_corner` | `0x0060A040` | 116 | ported exact |
//! | `GuyData::is_corner` | `0x005DE270` | 202 | ported exact |
//! | `GuyData::turn_speed` | `0x005DE340` | 202 | ported exact |
//! | `WorldData::get_down` | `0x004613D0` | 72 | ported exact |
//! | `WorldData::get_coll_block` | `0x006B5350` | 65 | reused from `map_terrain` |
//! | `World::new_coll_block` | `0x0046D250` | 179 | reused from `map_terrain` |
//! | `Objects::find_collision` | `0x00682110` | — | **not ported** (the other `collide_here` caller) |
//! | `Unit::detect_boat_collision` | `0x005FA8B0` | 1655 | multi-circle solver ported; world mutations are required trait hooks |
//!
//! Data tables, read out of `.rdata` [measured]: `RING_X` `0x00ADCAF0`, `RING_Y` `0x00ADC400`
//! (441 `int` each), `RING_COUNT` `0x00ADD1E0` (11 `int`), the 2x2 block offsets
//! `0x00ADD2A0` / `0x00ADD2B0`. The `UCoord`/`TCoord`/`WCoord` ladder is
//! `movement::{ucell_of, tile_of, wcell_of}` over `div_3_table` — reused, not re-derived.

use crate::rng::Random;
use crate::systems::combat::vector_dist;
use crate::systems::map_terrain::{tflag, wflag, CollBlock, World};
use crate::systems::movement::{tile_of, ucell_centre, ucell_of, PathData, PathStack, COORD_XOR};
use crate::trig::{cosx, find_angle, sinx};

// ---------------------------------------------------------------------------
// 1. Grid constants and the shipped ring tables  [measured]
// ---------------------------------------------------------------------------

/// One `UCoord`: 48 world units, a quarter tile. The collision grid's cell size.
pub const UCELL: i32 = 48;
/// A `CollBlock` covers 16x16 `UCoord` cells = one `WCoord` cell = 768 world units.
pub const BLOCK_UCELLS: i32 = 16;
/// Bits actually addressable by `CollBlock::get`/`set`: `(ux & 15) * 16 + (uy & 15)`.
/// The allocation is `BitMask<768>` (96 bytes) but only the first 32 bytes are ever
/// touched by the collision code — bits 256..767 stay zero. UNVERIFIED why 768.
pub const BLOCK_BITS: usize = 256;

/// `int move_x[441]` at `0x00ADCAF0` — the concentric-ring enumeration of `UCoord` offsets.
/// Entries `0..9` are the identity plus the engine's 8-direction rotation, which is why the
/// same table drives `astar_path`'s neighbour walk and this module's footprint walk.
///
/// **Shipped as-is, anomalies included.** Ring 8 substitutes `(-8,-16)` for `(-8,-7)`;
/// ring 9 substitutes `(-9,-10)` for `(-9,9)`, duplicates `(9,9)`, and omits `(9,-9)`;
/// ring 10 duplicates `(10,10)` and `(-10,0)`, omitting `(-10,10)` and `(10,-10)`.
/// Rings 0..7 are square-complete and duplicate-free [measured]. The retail unit table's
/// maximum `new_block_radius` is 7 (`BOMBER`), so the anomalous suffix is unreachable in
/// shipped content; it is reproduced rather than repaired, per "capture, do not calculate".
pub const RING_X: [i32; 441] = [
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

/// `int move_y[441]` at `0x00ADC400`. Same provenance and anomaly set as [`RING_X`].
pub const RING_Y: [i32; 441] = [
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

/// `int ring_count[11]` at `0x00ADD1E0` — `(2r+1)^2`, the prefix of [`RING_X`] that covers a
/// footprint of radius `r`. [measured]
pub const RING_COUNT: [i32; 11] = [1, 9, 25, 49, 81, 121, 169, 225, 289, 361, 441];

/// `0x00ADD2A0` / `0x00ADD2B0` — the four candidate blocks a footprint can straddle, in the
/// order `fill_slots` fills them. Slot index is `(bx > bx0) * 2 + (by > by0)`. [measured]
pub const BLOCK_DX: [i32; 4] = [0, 0, 1, 1];
pub const BLOCK_DY: [i32; 4] = [0, 1, 0, 1];

/// Number of ring entries covering radius `r`, clamped the way the engine's table is bounded.
#[inline]
pub fn ring_count(size: i32) -> i32 {
    if size < 0 {
        0
    } else if (size as usize) < RING_COUNT.len() {
        RING_COUNT[size as usize]
    } else {
        *RING_COUNT.last().unwrap()
    }
}

// ---------------------------------------------------------------------------
// 2. `CollBlock` — the 16x16 `UCoord` occupancy bitmap  [measured]
// ---------------------------------------------------------------------------

/// `CollBlock::get`/`set`'s bit index: `(ux mod 16) * 16 + (uy mod 16)`.
///
/// Retail computes the modulus as `v & 0x8000000f` then, if the result is negative,
/// `(v - 1 | 0xfffffff0) + 1` — a sign-preserving fixup that turns `-1 & 15 = 0x8000000f`
/// back into `15`. That is Euclidean remainder, so `rem_euclid` is exact. [measured
/// `0x00681E30`, `0x00682070`, and inline at four sites in `collide_here`.]
#[inline]
pub fn coll_bit(ux: i32, uy: i32) -> usize {
    (ux.rem_euclid(BLOCK_UCELLS) * BLOCK_UCELLS + uy.rem_euclid(BLOCK_UCELLS)) as usize
}

/// `CollBlock::get` `0x00681E30`.
#[inline]
pub fn block_get(b: &CollBlock, ux: i32, uy: i32) -> bool {
    let i = coll_bit(ux, uy);
    b.ptr[i >> 3] & (1u8 << (i & 7)) != 0
}

/// `CollBlock::set` `0x00682070`.
///
/// The `flags` tri-state is the whole point: setting a bit stamps `flags = 0`
/// ("known non-empty"); clearing a bit stamps `flags = 2` ("might be empty") **only if it
/// was 0**, so a block already known empty stays known empty.
#[inline]
pub fn block_set(b: &mut CollBlock, ux: i32, uy: i32, on: bool) {
    let i = coll_bit(ux, uy);
    if on {
        b.ptr[i >> 3] |= 1u8 << (i & 7);
        b.flags = 0;
    } else {
        b.ptr[i >> 3] &= !(1u8 << (i & 7));
        if b.flags == 0 {
            b.flags = 2;
        }
    }
}

/// `BitMask<768>::empty` `0x00479150`. **Mutating**: it memoises its answer into `flags`.
///
/// `flags & 1` -> empty without looking. `flags == 0` -> non-empty without looking.
/// Otherwise scan `size` bytes, and write back 0 or 1. Retail's `collide_here` calls this
/// once per straddled block per query, so a query can dirty the map's flag bytes — which is
/// why this takes `&mut`.
pub fn block_empty(b: &mut CollBlock) -> bool {
    if b.flags & 1 != 0 {
        return true;
    }
    if b.flags == 0 {
        return false;
    }
    let n = (b.size.max(0) as usize).min(b.ptr.len());
    let mut acc = 0u8;
    for &x in &b.ptr[..n] {
        acc |= x;
    }
    if acc != 0 {
        b.flags = 0;
        return false;
    }
    b.flags = 1;
    true
}

// ---------------------------------------------------------------------------
// 3. `CollCheck` — slot resolution and the footprint probe  [measured]
// ---------------------------------------------------------------------------

/// `class CollCheck collcheck` at `0x00CAB310`: `int valid[4]` then `CollBlock* block[4]`.
///
/// Here the block pointers are stored as `WData` indices into [`World::wdata`], because
/// `map_terrain` owns the storage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CollCheck {
    /// `collcheck+0x00` — is this slot a real, in-bounds block candidate?
    pub valid: [bool; 4],
    /// `collcheck+0x10` — the `WData` row holding the block, when one exists.
    pub slot: [Option<usize>; 4],
    /// The four temporary `CollBlock` copies installed in PathFinder's scratch tree while
    /// `overlay` is active. Retail keys a persistent tree by WData index; no routine other
    /// than `fill_slots` observes it, so retaining the four value copies for this query is
    /// behaviour-equivalent (the only mutation is `BitMask::empty`'s memoised `flags`).
    pub scratch: [Option<CollBlock>; 4],
    /// Not a retail field. Counts `collide_here` calls, for the coverage report.
    pub queries: u64,
}

impl CollCheck {
    pub fn new() -> Self {
        Self::default()
    }

    /// `CollCheck::fill_slots` `0x006820E0` — resolve the (at most) four `CollBlock`s a
    /// footprint of radius `size` centred on `(ux, uy)` can straddle.
    ///
    /// Order of operations, verbatim [measured]:
    ///
    /// 1. `bx0 = (ux - size) >> 4`, `by0 = (uy - size) >> 4` — the low block corner.
    /// 2. slot 0 is always a candidate; slot 2 and 3 are candidates iff
    ///    `(ux + size) >> 4 != bx0`; slot 3 is dropped and slot 1 armed by
    ///    `(uy + size) >> 4 != by0`.
    /// 3. any candidate outside `[0, World.xs) x [0, World.ys)` is dropped.
    /// 4. the **region gate**: the centre cell's `get_tregion` value is computed once, and a
    ///    candidate block is only accepted when the candidate cell's `region` matches it (or
    ///    the centre's is negative). This is why units do not collide across a region seam.
    /// 5. `WData.block` of `0` or `-1` leaves the slot empty.
    ///
    /// `overlay` is retail's `param_4`, which consults the pathfinder's scratch
    /// `Tree<CollBlock*,int>` at `PathFinder+0x4C` (`0x00E85E8C`) and clones each selected
    /// real block (or a new empty block) into it. This port keeps the query's four cloned
    /// values in [`CollCheck::scratch`]; only `fill_slots` uses the retail tree and its key
    /// lookup changes no bit-level answer [measured, sole references `0x006820E0` and
    /// `0x00689EC0`].
    pub fn fill_slots(&mut self, w: &World, ux: i32, uy: i32, size: i32, overlay: bool) {
        let bx0 = (ux - size) >> 4;
        let by0 = (uy - size) >> 4;

        let spans_x = ((ux + size) >> 4) != bx0;
        let spans_y = ((uy + size) >> 4) != by0;
        self.valid = [true, spans_y, spans_x, spans_x && spans_y];
        self.slot = [None; 4];
        self.scratch = [None; 4];

        // Step 3 — bounds. Retail folds this into a `skip[]` array that also suppresses the
        // block lookup below.
        let mut skip = [false; 4];
        for i in 0..4 {
            let bx = bx0 + BLOCK_DX[i];
            let by = by0 + BLOCK_DY[i];
            if !self.valid[i] || bx < 0 || by < 0 || bx >= w.xs || by >= w.ys {
                skip[i] = true;
                self.valid[i] = false;
            }
        }

        // Step 4 — the region the whole query is anchored to. Retail reads the *centre*
        // cell `(ux >> 4, uy >> 4)`, not the low corner, and picks `region2` only for the
        // water half of a `WATERHALF` cell — i.e. exactly `WorldData::get_tregion` on the
        // tile `(ux >> 2, uy >> 2)`.
        let cbx = ux >> 4;
        let cby = uy >> 4;
        let region = if w.valid_w(cbx, cby) {
            let rec = w.wdata(cbx, cby);
            let tx = ux >> 2;
            let ty = uy >> 2;
            let water =
                w.valid_t(tx, ty) && w.tmask(tx, ty) & tflag::SURFACE_MASK == tflag::SURFACE_WATER;
            if rec.flags & wflag::WATERHALF != 0 && water {
                rec.region2 as i32
            } else {
                rec.region as i32
            }
        } else {
            // Retail indexes `wdata` unconditionally here and would read out of bounds.
            // Refusing the query is the only safe reading; flagged as a divergence.
            -1
        };

        for i in 0..4 {
            if skip[i] {
                continue;
            }
            let bx = bx0 + BLOCK_DX[i];
            let by = by0 + BLOCK_DY[i];
            let idx = w.w_index(bx, by);
            let rec = &w.wdata[idx];
            if (region < 0 || rec.region as i32 == region) && rec.block.is_some() {
                self.slot[i] = Some(idx);
            }
        }

        if overlay {
            // With PathFinder's scratch tree installed, retail inserts one clone for every
            // valid candidate not already in the tree. A candidate rejected by the normal
            // region/block lookup receives a newly constructed empty CollBlock.
            for i in 0..4 {
                if !self.valid[i] {
                    continue;
                }
                let bx = bx0 + BLOCK_DX[i];
                let by = by0 + BLOCK_DY[i];
                let idx = w.w_index(bx, by);
                self.scratch[i] = Some(
                    self.slot[i]
                        .and_then(|wi| w.wdata[wi].block.as_deref().copied())
                        .unwrap_or_default(),
                );
                self.slot[i] = Some(idx);
            }
        }
    }

    /// Which of the four slots a candidate cell falls in, given the low block corner.
    /// `(bx > bx0) * 2 + (by > by0)` — retail writes it as two conditional moves. [measured]
    #[inline]
    fn slot_of(ux: i32, uy: i32, bx0: i32, by0: i32) -> usize {
        let a = if (ux >> 4) <= bx0 { 0 } else { 2 };
        let b = if (uy >> 4) <= by0 { 0 } else { 1 };
        (a + b) as usize
    }

    /// `CollCheck::collide_here` `0x00682540`.
    ///
    /// "Would a footprint of radius `size` placed at `(ux, uy)` overlap an occupied cell that
    /// is not already mine?" Returns the offending `UCoord` cell, which the caller needs in
    /// order to identify the blocker.
    ///
    /// Three arms, in retail's order:
    ///
    /// * **early out** — `size == 0` never collides (and never stamps, see [`move_unit`]);
    ///   if every straddled block is empty, return immediately.
    /// * **adjacent-step fast path** (only when `overlay` is false) — when the candidate is
    ///   exactly one cell away in `x` *or* in `y`, only the *leading edge* of the footprint
    ///   can be newly occupied, so retail walks that one column/row instead of the square.
    /// * **general path** — walk `RING[0..ring_count(size)]`, skipping cells already inside
    ///   this unit's own footprint, and test **every other cell**: the filter is
    ///   `(dx + size) & 1 == 0 && (dy + size) & 1 == 0`, a stride-2 subsample anchored so the
    ///   boundary ring is always included. Stamps are dense squares (see [`move_unit`]), so
    ///   the subsample still meets any overlapping stamp.
    ///
    /// **A quirk that is reproduced deliberately.** In both fast-path loops the cursor
    /// advances on odd iterations *and* on even-iteration misses, so the stride-2 sampling
    /// silently shifts phase whenever a probed cell lands in an absent or empty block. It is
    /// in the shipped control flow at `0x00682730`/`0x00682836` and is kept verbatim.
    pub fn collide_here<U: CollUnits>(
        &mut self,
        w: &mut World,
        units: &U,
        o: i32,
        who: i32,
        ux: i32,
        uy: i32,
        size: i32,
        overlay: bool,
    ) -> Option<(i32, i32)> {
        self.queries += 1;
        if size == 0 {
            return None;
        }
        self.fill_slots(w, ux, uy, size, overlay);

        // `local_30[i]` — "slot i cannot contribute". Note `BitMask::empty` mutates flags.
        let mut dead = [true; 4];
        for i in 0..4 {
            dead[i] = match (self.valid[i], self.slot[i]) {
                (false, _) | (_, None) => true,
                (true, Some(idx)) => match (overlay, self.scratch[i].as_mut()) {
                    (true, Some(b)) => block_empty(b),
                    (true, None) => true,
                    (false, _) => {
                        let b = w.wdata[idx]
                            .block
                            .as_deref_mut()
                            .expect("slot implies a block");
                        block_empty(b)
                    }
                },
            };
        }
        // Only the low block can contribute -> take the single-block fast reads.
        let single = dead[1] && dead[2] && dead[3];
        if single && dead[0] {
            return None;
        }

        let bx0 = (ux - size) >> 4;
        let by0 = (uy - size) >> 4;

        let me = units.row(who, o);

        // -- arm 2: the adjacent-step fast path -------------------------------------------
        if !overlay {
            if let Some(me) = me {
                let dx = ux - ucell_of(me.x);
                let dy = uy - ucell_of(me.y);
                if dx == 0 || dy == 0 {
                    if dx.abs() == 1 {
                        let n = size * 2 + 1;
                        let cx = ux + dx * size;
                        let mut cy = uy - size;
                        let mut k = 0;
                        while k < n {
                            let mut advance = true;
                            if k & 1 == 0 {
                                let s = Self::slot_of(cx, cy, bx0, by0);
                                let probe = if single {
                                    if (cx >> 4) <= bx0 && (cy >> 4) <= by0 {
                                        Some(0usize)
                                    } else {
                                        None
                                    }
                                } else if !dead[s] {
                                    Some(s)
                                } else {
                                    None
                                };
                                match probe {
                                    Some(s) => {
                                        let idx = self.slot[s].expect("live slot");
                                        let b = if overlay {
                                            self.scratch[s].as_ref().expect("live scratch slot")
                                        } else {
                                            w.wdata[idx].block.as_deref().unwrap()
                                        };
                                        if block_get(b, cx, cy) {
                                            return Some((cx, cy));
                                        }
                                    }
                                    // Phase quirk: no cursor advance when the slot is dead.
                                    None => advance = false,
                                }
                            }
                            if advance {
                                cy += 1;
                            }
                            k += 1;
                        }
                        return None;
                    }
                    if dy.abs() == 1 {
                        let n = size * 2 + 1;
                        let cy = uy + dy * size;
                        let mut cx = ux - size;
                        let mut k = 0;
                        while k < n {
                            let mut advance = true;
                            if k & 1 == 0 {
                                let s = Self::slot_of(cx, cy, bx0, by0);
                                let probe = if single {
                                    if (cx >> 4) <= bx0 && (cy >> 4) <= by0 {
                                        Some(0usize)
                                    } else {
                                        None
                                    }
                                } else if !dead[s] {
                                    Some(s)
                                } else {
                                    None
                                };
                                match probe {
                                    Some(s) => {
                                        let idx = self.slot[s].expect("live slot");
                                        let b = if overlay {
                                            self.scratch[s].as_ref().expect("live scratch slot")
                                        } else {
                                            w.wdata[idx].block.as_deref().unwrap()
                                        };
                                        if block_get(b, cx, cy) {
                                            return Some((cx, cy));
                                        }
                                    }
                                    None => advance = false,
                                }
                            }
                            if advance {
                                cx += 1;
                            }
                            k += 1;
                        }
                        return None;
                    }
                }
            }
        }

        // -- arm 3: the general footprint walk ---------------------------------------------
        let (mux, muy, on_map) = match me {
            Some(m) => (ucell_of(m.x), ucell_of(m.y), m.on_map),
            None => (0, 0, false),
        };
        let n = ring_count(size);
        for i in 0..n as usize {
            let (rx, ry) = (RING_X[i], RING_Y[i]);
            if (rx + size) & 1 != 0 || (ry + size) & 1 != 0 {
                continue;
            }
            let cx = rx + ux;
            let cy = ry + uy;
            // Skip cells already covered by my own stamp — my own bits are set there.
            if on_map && (cx - mux).abs() <= size && (cy - muy).abs() <= size {
                continue;
            }
            let s = Self::slot_of(cx, cy, bx0, by0);
            let probe = if single {
                if (cx >> 4) <= bx0 && (cy >> 4) <= by0 {
                    Some(0usize)
                } else {
                    None
                }
            } else if !dead[s] {
                Some(s)
            } else {
                None
            };
            if let Some(s) = probe {
                let idx = self.slot[s].expect("live slot");
                let b = if overlay {
                    self.scratch[s].as_ref().expect("live scratch slot")
                } else {
                    w.wdata[idx].block.as_deref().unwrap()
                };
                if block_get(b, cx, cy) {
                    return Some((cx, cy));
                }
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// 4. Stamping: `CollCheck::move_unit` and the reaper  [measured]
// ---------------------------------------------------------------------------

/// `CollCheck::move_unit` `0x00682AD0` — move a footprint's stamp from one cell to another.
///
/// Called from **`Guy::set_new_location` `0x005D86F0` only** [measured, sole caller]. So the
/// bitmap is stamped **per guy**, not per unit: every guy of a non-air unit with
/// `guy_num < UnitTypeData+0x304 squad_size` stamps a `(2r+1)^2` square at its own position
/// with the *parent unit type's* `new_block_radius`, and only when its `UCoord` cell actually
/// changes. A three-guy squad therefore leaves three overlapping squares.
///
/// `size == 0` returns immediately, so a type with `new_block_radius == 0` is invisible to
/// collision in both directions.
///
/// The clear pass walks the ring around `from` and **skips every cell within `size` of `to`**;
/// the set pass walks the ring around `to` and skips every cell within `size` of `from`. So a
/// short move only repaints the leading and trailing edges. There is **no parity filter** on
/// either pass — the stamp is a dense square, which is what licenses `collide_here`'s
/// stride-2 probe.
pub fn move_unit(w: &mut World, from: (i32, i32), to: (i32, i32), size: i32) {
    if size == 0 {
        return;
    }
    let (fx, fy) = from;
    let (tx, ty) = to;

    // The single-block fast mode: both footprints inside one block, and that block exists.
    let span = size * 2;
    let mut single: Option<usize> = None;
    if (fx - tx).abs() + span < BLOCK_UCELLS && (fy - ty).abs() + span < BLOCK_UCELLS {
        let bx0 = (fx.min(tx) - size) >> 4;
        let by0 = (fy.min(ty) - size) >> 4;
        if ((fx.max(tx) + size) >> 4) == bx0
            && ((fy.max(ty) + size) >> 4) == by0
            && w.valid_w(bx0, by0)
        {
            let region = w.get_tregion(fx >> 2, fy >> 2);
            let idx = w.w_index(bx0, by0);
            let rec = &w.wdata[idx];
            if region >= 0 && rec.region as i32 != region {
                return; // retail's `-1` -> return
            }
            match rec.block {
                None => return, // retail's `0` -> return
                Some(_) => single = Some(idx),
            }
        }
    }

    // -- clear pass, around `from` -------------------------------------------------------
    if fx >= 0 && fy >= 0 && fx < w.xs * BLOCK_UCELLS && fy < w.ys * BLOCK_UCELLS {
        let region = match single {
            Some(_) => -2, // unused in single mode
            None => w.get_tregion(fx >> 2, fy >> 2),
        };
        let n = ring_count(size);
        for i in 0..n as usize {
            let cx = RING_X[i] + fx;
            let cy = RING_Y[i] + fy;
            if (cx - tx).abs() <= size && (cy - ty).abs() <= size {
                continue; // still covered after the move
            }
            let idx = match single {
                Some(idx) => Some(idx),
                None => {
                    if cx < 0 || cy < 0 || cx >= w.xs * BLOCK_UCELLS || cy >= w.ys * BLOCK_UCELLS {
                        None
                    } else {
                        let bi = w.w_index(cx >> 4, cy >> 4);
                        let rec = &w.wdata[bi];
                        if (region < 0 || rec.region as i32 == region) && rec.block.is_some() {
                            Some(bi)
                        } else {
                            None
                        }
                    }
                }
            };
            if let Some(bi) = idx {
                if let Some(b) = w.wdata[bi].block.as_deref_mut() {
                    block_set(b, cx, cy, false);
                }
            }
        }
    }

    // -- set pass, around `to` -----------------------------------------------------------
    let region = match single {
        Some(_) => -2,
        None => w.get_tregion(tx >> 2, ty >> 2),
    };
    let n = ring_count(size);
    for i in 0..n as usize {
        let cx = RING_X[i] + tx;
        let cy = RING_Y[i] + ty;
        if (cx - fx).abs() <= size && (cy - fy).abs() <= size {
            continue; // already stamped before the move
        }
        let bi = match single {
            Some(bi) => Some(bi),
            None => {
                if cx < 0 || cy < 0 || cx >= w.xs * BLOCK_UCELLS || cy >= w.ys * BLOCK_UCELLS {
                    None
                } else {
                    let bi = w.w_index(cx >> 4, cy >> 4);
                    let rec = &w.wdata[bi];
                    if region >= 0 && rec.region as i32 != region {
                        None
                    } else {
                        if rec.block.is_none() {
                            w.new_coll_block(cx >> 4, cy >> 4);
                        }
                        Some(bi)
                    }
                }
            }
        };
        if let Some(bi) = bi {
            if let Some(b) = w.wdata[bi].block.as_deref_mut() {
                block_set(b, cx, cy, true);
            }
        }
    }
}

/// Stamp a guy into the collision map, the way `Guy::set_new_location` `0x005D86F0` does.
///
/// `from`/`to` are world `Coord`s (not XOR-obfuscated — `GuyData`'s coordinates are stored
/// plain). Returns whether the stamp moved.
pub fn guy_set_new_location(
    w: &mut World,
    from: (i32, i32),
    to: (i32, i32),
    domain: i32,
    guy_num: i32,
    squad_size: i32,
    block_radius: i32,
) -> bool {
    if domain == DOMAIN_AIR || guy_num >= squad_size {
        return false;
    }
    let f = (ucell_of(from.0), ucell_of(from.1));
    let t = (ucell_of(to.0), ucell_of(to.1));
    if f == t {
        return false;
    }
    move_unit(w, f, t, block_radius);
    true
}

/// `GameDaemon::process_coll_blocks` `0x00731F90` — step 12's collision-block reaper.
///
/// It walks a **persistent cursor** (`GameDaemon+0x20`) across the whole `wdata` plane,
/// `max(World.xs / 4, 5)` cells per frame, and frees any `CollBlock` that has gone empty.
/// The `flags` tri-state does the work: `0` means known non-empty and the cell is skipped
/// without touching the payload; `2` means "a bit was cleared since we last looked", so the
/// 96 bytes are OR-scanned and the flag settles to `0` or `1`; `1` frees the block.
///
/// This is the whole of the `Gap::GameDaemonProcessCollBlocks` entry `tick.rs` counts.
pub fn process_coll_blocks(w: &mut World, cursor: &mut i32) -> u32 {
    let total = w.xs * w.ys;
    if total <= 0 {
        return 0;
    }
    // `local_8 = xs / 4` truncating toward zero, then `if (local_8 < 5) local_8 = 5`.
    let mut budget = (w.xs + (if w.xs < 0 { 3 } else { 0 })) / 4;
    if budget < 5 {
        budget = 5;
    }
    let mut freed = 0;
    for _ in 0..budget {
        let i = (*cursor).clamp(0, total - 1) as usize;
        let mut drop = false;
        if let Some(b) = w.wdata[i].block.as_deref_mut() {
            if b.flags & 1 == 0 {
                if b.flags != 0 {
                    let n = (b.size.max(0) as usize).min(b.ptr.len());
                    let mut acc = 0u8;
                    for &x in &b.ptr[..n] {
                        acc |= x;
                    }
                    if acc != 0 {
                        b.flags = 0;
                    } else {
                        b.flags = 1;
                        drop = true;
                    }
                }
            } else {
                drop = true;
            }
        }
        if drop {
            w.wdata[i].block = None;
            freed += 1;
        }
        *cursor += 1;
        if *cursor >= total {
            *cursor = 0;
        }
    }
    freed
}

// ---------------------------------------------------------------------------
// 5. The object-side view collision needs
// ---------------------------------------------------------------------------

/// `ObjectTypeData+0x218 domain`. `2` is air: air units neither stamp nor collide.
pub const DOMAIN_LAND: i32 = 0;
pub const DOMAIN_WATER: i32 = 1;
pub const DOMAIN_AIR: i32 = 2;

/// `UnitData+0x68 unit_masks` bits this lane reads or writes. Named for what the code does
/// with them, not for a rule name — no rule name has been located for any of the three.
pub mod umask {
    /// Raised by `resolve_unit_collision`'s wait arm, cleared by `detect_unit_collision`'s
    /// no-collision epilogue. While it is up the unit holds position for a frame.
    pub const WAIT: u32 = 0x0000_0040;
    /// Raised by `detect_unit_collision` when it found bodies in the way but decided none of
    /// them blocks — `Unit::move_step` reads it and halves the step. "Yield".
    pub const YIELD: u32 = 0x0010_0000;
    /// Cleared by `resolve_unit_collision`'s first arm. UNVERIFIED meaning.
    pub const RESOLVE_ENTRY: u32 = 0x0400_0000;
}

/// The `UnitData` / `ObjectData` fields collision reads and writes, PDB names and offsets.
///
/// A `Copy` snapshot on purpose: the neighbour scan reads other units while the caller holds
/// a mutable borrow of its own row, and retail's own access pattern is read-then-write.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UnitRow {
    /// `ObjectData+0x09 who` and `+0x0A o` — the engine's object address.
    pub who: i32,
    pub o: i32,
    /// `ObjectData+0x10/+0x14`, **de-obfuscated** (the stored words are XOR [`COORD_XOR`]).
    pub x: i32,
    pub y: i32,
    /// `ObjectData+0x2C down` / `+0x2E down_who` — the next object in this `WData` cell's
    /// intrusive list. `down < 0` terminates.
    pub down: i16,
    pub down_who: i16,
    /// `ObjectTypeData+0x218 domain`.
    pub domain: i32,
    /// `ObjectTypeData+0x248 new_block_radius` — the footprint radius, in `UCoord` cells.
    pub block_radius: i32,
    /// `ObjectTypeData+0x244 big_radius`, used by `move_step`'s stuck test.
    pub big_radius: i32,
    /// `UnitTypeData+0x2F8/+0x2FC`: total bow-to-stern collision profile length and number
    /// of tangent circles. `detect_boat_collision` divides the former by the latter.
    pub push_size: i32,
    pub push_circles: i32,
    /// `UnitData+0x50 angle` and the first live guy's `GuyData+0x18 angle`. The boat solver
    /// uses the guy angle to lay out both hull profiles and to constrain a pushed unit.
    pub angle: i32,
    pub first_guy_angle: i32,
    /// `UnitData+0x80 group`; `-1` is ungrouped.
    pub group: i16,
    /// `UnitData+0x88 collide` — consecutive collided frames.
    pub collide: i16,
    /// `UnitData+0x8A collide_o` / `+0xB3 collide_who` — the recorded blocker. `-1` = none.
    pub collide_o: i16,
    pub collide_who: i8,
    /// `UnitData+0x8C collide_guy`.
    pub collide_guy: i16,
    /// `UnitData+0x48 collide_frame`.
    pub collide_frame: i32,
    /// `UnitData+0xB2 safe` — the pathfinder's retry delay. Non-zero suppresses the whole
    /// collision test [measured, the `0x00617141` gate].
    pub safe: i8,
    /// `UnitData+0x68 unit_masks`.
    pub unit_masks: u32,
    /// `UnitData::is_on_map` `0x0046CE30` — `(ObjectData+0x82 >> 15)`. An off-map unit has no
    /// stamp, so `collide_here` must not skip "its own" cells for it.
    pub on_map: bool,
    /// vtable `+0x18`, the liveness test the neighbour scan applies to each candidate.
    pub active: bool,
    /// `UnitData::is_moving` `0x00610AF0`.
    pub moving: bool,
    /// `UnitData::action_type` `0x0060A850` — the *current action*'s `OrderIndex`.
    pub action: i32,
    /// `UnitData::order_type` `0x00616E80` — the *current order*'s `OrderIndex`.
    pub order: i32,
    /// `UnitData+0xDC` — does the unit have an order list at all?
    pub has_orders: bool,
    /// `UnitData+0x104 openlist` — a suspended pathfinder search is parked on this unit.
    pub searching: bool,
    /// Flags on the top `PathData` record, or zero with no path. Bit 3 suppresses collision
    /// exactly as the `UnitData+0xC0`/top-record gate at `0x00617119` does.
    pub path_top_flags: u8,
    /// `UnitTypeData+0x2B4 unit_flags`.
    pub unit_flags: u32,
    /// `UnitTypeData+0x2B8 unit_flags2`; bit 2 selects the packed/unpacking exclusion.
    pub unit_flags2: u32,
    /// `ObjectTypeData+0x1E8 attack`; non-zero prevents a pusher carrying unit-flag 0x10
    /// from displacing this candidate.
    pub attack_value: i32,
    /// `SpellType` id when [`UnitRow::order`] is `CastSpell`, else `-1`.
    pub spell_id: i32,
    /// Dynamic predicate read through `UnitData::is_unpacking` in the land-domain arm.
    pub unpacking: bool,
    /// `UnitData::is_captain` `0x0046CEB0`; retained for the object-side adapter even though
    /// the retail collision functions in this module do not branch on it.
    pub captain: bool,
}

/// The exact `GuyData` slice needed by `UnitData::is_corner` `0x0060A040`.
///
/// Retail tests every live squad guy's own plain coordinates and that guy type's resolved
/// `ObjectTypeData+0x248 new_block_radius`; it does not substitute the unit anchor for a
/// multi-guy squad. Crew and null slots must be absent from the iterator passed to
/// [`unit_corner`], just as retail skips null pointers in the unit's guy array.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CollGuy {
    pub x: i32,
    pub y: i32,
    pub block_radius: i32,
}

/// The exact `Objects::find_units` query issued by `Unit::detect_boat_collision`.
///
/// The omitted finder arguments are fixed by the callsite: search index 0, owner -1,
/// maximum distance `0x200`, filter index 3, and the acting object's `(o, who)` exclusion.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoatQuery {
    pub x: i32,
    pub y: i32,
    pub radius: i32,
    pub exclude_o: i32,
    pub exclude_who: i32,
    pub circles: i32,
    pub water: bool,
}

/// The queries `detect_unit_collision` and `resolve_unit_collision` make of the world's
/// object side. Every method names the retail function it stands in for.
///
/// Deliberately has no defaults: an implementor must decide each answer, because a silently
/// wrong "no" here reads as "nothing blocks" and is invisible.
pub trait CollUnits {
    /// `Objects::get(who, o)` — `nullptr` for a dead or out-of-range address.
    fn row(&self, who: i32, o: i32) -> Option<UnitRow>;
    /// `UnitData::is_corner` `0x0060A040`. Implementations must walk the unit's live,
    /// non-null squad guys in pointer-array order and use each guy type's
    /// `new_block_radius`; [`unit_corner`] is the shared exact implementation.
    fn unit_corner(&self, who: i32, o: i32, cx: i32, cy: i32) -> i32;
    /// `Objects::find_units` `0x0065A620`, preserving the returned scratch-array order.
    fn find_boat_units(&mut self, query: BoatQuery) -> Vec<(i32, i32)>;
    /// `leaders[who].who`, which can differ from the slot under shared control.
    fn effective_owner(&self, who: i32) -> i32;
    /// `LeaderData+0x74 diplos[other]`.
    fn diplomacy(&self, who: i32, other: i32) -> i32;
    /// Candidate `UnitData::invalid_loc(tile_x, tile_y, 0, 0, 0, 0, 0)`.
    fn boat_invalid_loc(&self, who: i32, o: i32, tx: i32, ty: i32) -> bool;
    /// Candidate `Unit::set_new_location(x, y, 0, 0)`, including world links, per-guy
    /// destinations and collision stamps.
    fn set_boat_location(&mut self, who: i32, o: i32, x: i32, y: i32);
    /// The no-order turn tail: `Unit::set_angle(angle, pusher, 0)`, followed by first-guy
    /// `turn_angles(angle, ..., 1, 1)` and `do_turn(angle, ..., 1, 1)`.
    fn face_pushed_idle_unit(
        &mut self,
        who: i32,
        o: i32,
        angle: i32,
        pusher_who: i32,
        pusher_o: i32,
    );
    /// Write back a row mutated by [`Detect::apply`] / [`Resolve::apply`].
    fn write(&mut self, who: i32, o: i32, r: &UnitRow);
    /// `LeaderData::is_enemy` `0x006EBAA0`.
    fn is_enemy(&self, me: i32, them: i32) -> bool;
    /// `Unit::update_order()->vf+0x40` `+0x3C`/`+0x40` — the order's stored step target, which
    /// `detect_unit_collision` writes and `resolve_unit_collision` reads back.
    fn order_dest(&self, who: i32, o: i32) -> Option<(i32, i32)>;
    fn set_order_dest(&mut self, who: i32, o: i32, x: i32, y: i32);
    /// `order->vf+0x40` `+0x2C`/`+0x30` — where the detour waypoint is recorded.
    fn set_order_detour(&mut self, who: i32, o: i32, x: i32, y: i32);
    /// `order->vf+0x40` `+0x18` — the post-repath wait, and `+0x10`, cleared on every repath.
    fn set_order_wait(&mut self, who: i32, o: i32, ticks: i32);
    fn clear_order_retry(&mut self, who: i32, o: i32);
    /// Does this unit's current order name `(who, o)` as its target?
    /// (`ord->vf+0x3C`'s `+8`/`+0xC`, and the same test inside the `Guard` arm.)
    fn order_targets(&self, who: i32, o: i32, target_who: i32, target_o: i32) -> bool;
    /// `ObjectData::attack_dist` `0x006488F0` from the acting unit's action target to
    /// `(nx, ny)`, minus `range * 0xC0`, clamped at 0 — the `Attack`-crowding slack.
    fn attack_slack(&self, who: i32, o: i32, nx: i32, ny: i32) -> i32;
    /// `GameDaemon + who*4` — the per-owner, per-frame repath budget.
    fn repath_budget(&self, who: i32) -> i32;
    fn bump_repath_budget(&mut self, who: i32);
    /// `GameAccess::game + 0x550` — the simulation frame counter.
    fn frame(&self) -> i32;
}

// ---------------------------------------------------------------------------
// 6. The corner rules  [measured]
// ---------------------------------------------------------------------------

/// `UnitData::will_be_corner` `0x00609FA0`: is `(ax, ay)` exactly a corner of a footprint of
/// radius `size` centred on `(bx, by)`? Returns the engine's 8-direction code — `1` NW,
/// `3` NE, `5` SE, `7` SW, matching [`RING_X`]/[`RING_Y`] indices 1/3/5/7 — or `0`.
#[inline]
pub fn will_be_corner(ax: i32, ay: i32, bx: i32, by: i32, size: i32) -> i32 {
    let dx = ax - bx;
    let dy = ay - by;
    if dx.abs() != size || dy.abs() != size {
        return 0;
    }
    match (dx < 0, dy < 0) {
        (true, true) => 1,
        (false, true) if dx > 0 => 3,
        (false, false) if dx > 0 && dy > 0 => 5,
        (true, false) if dy > 0 => 7,
        _ => 0,
    }
}

/// `UnitData::is_here` `0x0060A0C0` — is the `UCoord` cell `(cx, cy)` inside this unit's
/// stamped footprint? `size == 0` is never anywhere.
#[inline]
pub fn is_here(cx: i32, cy: i32, unit: &UnitRow) -> bool {
    if unit.block_radius == 0 {
        return false;
    }
    (cx - ucell_of(unit.x)).abs() <= unit.block_radius
        && (cy - ucell_of(unit.y)).abs() <= unit.block_radius
}

/// `GuyData::is_corner` `0x005DE270`, exact once the guy's resolved type radius is supplied.
#[inline]
pub fn guy_corner(cx: i32, cy: i32, guy: CollGuy) -> i32 {
    will_be_corner(cx, cy, ucell_of(guy.x), ucell_of(guy.y), guy.block_radius)
}

/// `UnitData::is_corner` `0x0060A040`: walk live guy pointers in array order and return the
/// first non-zero `GuyData::is_corner` result.
pub fn unit_corner(cx: i32, cy: i32, guys: impl IntoIterator<Item = CollGuy>) -> i32 {
    for guy in guys {
        let corner = guy_corner(cx, cy, guy);
        if corner != 0 {
            return corner;
        }
    }
    0
}

// ---------------------------------------------------------------------------
// 7. `Unit::detect_boat_collision`  [measured]
// ---------------------------------------------------------------------------

/// Project `(x, y)` by `dist` along a retail binary heading.
///
/// This is `project` `0x0092CF40`: `x + sinx(angle, dist)`,
/// `y - cosx(angle, dist)`. All operations wrap as 32-bit coordinates do.
#[inline]
pub fn boat_project(x: i32, y: i32, angle: i32, dist: i32) -> (i32, i32) {
    (
        x.wrapping_add(sinx(angle, dist)),
        y.wrapping_sub(cosx(angle, dist)),
    )
}

/// Lay out the centres of a `push_circles` hull profile.
///
/// `push_size / push_circles` is each circle's radius. Profiles with two or more circles
/// are centred on the object and spaced by twice that radius along its first guy's heading,
/// making adjacent circles tangent. Division is intentionally unchecked: retail `idiv`
/// traps when mod data supplies a non-zero `push_size` with zero circles.
pub fn boat_hull_centres(
    x: i32,
    y: i32,
    angle: i32,
    push_size: i32,
    push_circles: i32,
) -> (i32, Vec<(i32, i32)>) {
    let radius = push_size / push_circles;
    if push_circles < 2 {
        return (
            radius,
            if push_circles > 0 {
                vec![(x, y)]
            } else {
                Vec::new()
            },
        );
    }
    let (dx, dy) = boat_project(0, 0, angle, radius);
    let mut cx = x.wrapping_sub((push_circles - 1).wrapping_mul(dx));
    let mut cy = y.wrapping_sub((push_circles - 1).wrapping_mul(dy));
    let sx = dx.wrapping_mul(2);
    let sy = dy.wrapping_mul(2);
    let mut out = Vec::with_capacity(push_circles as usize);
    for _ in 0..push_circles {
        out.push((cx, cy));
        cx = cx.wrapping_add(sx);
        cy = cy.wrapping_add(sy);
    }
    (radius, out)
}

/// `Unit::detect_boat_collision` `0x005FA8B0`.
///
/// Despite its name this is the engine's **multi-circle body-pushing solver**. Water units
/// always enter it, while siege/hero/supply land units reach it through
/// [`detect_unit_collision`]'s measured pre-gate. Each type describes a hull as
/// `push_circles` tangent circles spanning `push_size`; the solver finds the minimum retail
/// [`vector_dist`] between the acting profile at `(nx, ny)` and every finder candidate.
/// A positive overlap pushes the candidate outward by at most one UCoord (`0x30`) unless an
/// attack, diplomacy, packed-unit, unpacking, special-mask, tank, or heading constraint says
/// the body is immovable. An immovable body returns `false` immediately so ordinary bitmap
/// collision may handle it; a completed/no-op scan returns `true`.
///
/// Side effects are in shipped order:
///
/// 1. `Objects::find_units` fills its global scratch array and fixes candidate order.
/// 2. A movable candidate passes `invalid_loc`, then `set_new_location` runs.
/// 3. With `move_other` and either a one-circle target or a land pusher, its blocker IDs are
///    set to the pusher; an idle target is turned, including its first guy.
/// 4. A successfully relocated candidate receives `collide_frame = frame` at `0x005FAEE0`.
///    Filtered, non-overlapping, and invalid-location candidates jump directly to the loop
///    increment at `0x005FAEEE` and receive no frame write.
///
/// There is **no RNG call** anywhere in the 1,655-byte function. The exact state effect on
/// `game_random` is therefore zero draws.
pub fn detect_boat_collision<U: CollUnits>(
    units: &mut U,
    me: &UnitRow,
    nx: i32,
    ny: i32,
    move_other: bool,
) -> bool {
    if me.who >= 8 || me.push_size == 0 {
        return true;
    }

    let query = BoatQuery {
        x: nx,
        y: ny,
        radius: me.push_size,
        exclude_o: me.o,
        exclude_who: me.who,
        circles: me.push_circles,
        water: me.domain == DOMAIN_WATER,
    };
    let candidates = units.find_boat_units(query);
    let (my_radius, my_centres) =
        boat_hull_centres(nx, ny, me.first_guy_angle, me.push_size, me.push_circles);

    for (other_who, other_o) in candidates {
        let mut other = units
            .row(other_who, other_o)
            .expect("find_boat_units returned a non-unit address");

        let eligible = (me.domain == DOMAIN_LAND || other.who < 8)
            && other.domain == me.domain
            && !(other.who == me.who
                && other.group == me.group
                && me.group != -1
                && move_other
                && me.action != ORDER_ATTACK)
            && !(me.collide_o as i32 == other.o
                && me.collide_who as i32 == other.who
                && me.collide_frame == units.frame());

        if eligible && other.push_size != 0 {
            let (their_radius, their_centres) = boat_hull_centres(
                other.x,
                other.y,
                other.first_guy_angle,
                other.push_size,
                other.push_circles,
            );
            let mut minimum = 0x0fff_ffff;
            let mut first = true;
            for &(mx, my) in &my_centres {
                for &(tx, ty) in &their_centres {
                    let dist = vector_dist(mx.wrapping_sub(tx), my.wrapping_sub(ty));
                    if first || dist < minimum {
                        minimum = dist;
                        first = false;
                    }
                }
            }
            let overlap = their_radius.wrapping_sub(minimum).wrapping_add(my_radius) / 2;

            if overlap > 0 {
                if me.unit_flags & 0x10 != 0 && other.attack_value != 0 {
                    return false;
                }

                let effective = units.effective_owner(me.who);
                if other.who != effective
                    && (units.diplomacy(me.who, other.who) != 2
                        || units.diplomacy(other.who, effective) != 2)
                    && other.who < 8
                {
                    return false;
                }

                if me.domain == DOMAIN_LAND {
                    if other.unit_flags2 & 4 != 0
                        && (other.unit_masks & 0x0008_0000 == 0 || other.unpacking)
                    {
                        return false;
                    }
                    if other.unit_masks & 0x0200_0000 != 0 || other.unit_flags & 0x0008_0000 != 0 {
                        return false;
                    }
                }

                let mut push_angle = find_angle(other.x.wrapping_sub(nx), other.y.wrapping_sub(ny));
                if !other.moving {
                    let delta = (push_angle as u32).wrapping_sub(me.first_guy_angle as u32);
                    if delta < 0x2000_0000 {
                        push_angle = me.first_guy_angle.wrapping_add(0x2000_0000);
                    } else if delta > 0xe000_0000 {
                        push_angle = me.first_guy_angle.wrapping_sub(0x2000_0000);
                    }
                } else {
                    let window = (push_angle as u32)
                        .wrapping_sub(me.first_guy_angle as u32)
                        .wrapping_add(0xe000_0000);
                    if window > 0xc000_0000 {
                        return false;
                    }
                }

                {
                    let step = overlap.min(0x30);
                    let (tx, ty) = boat_project(other.x, other.y, push_angle, step);
                    if !units.boat_invalid_loc(other.who, other.o, tile_of(tx), tile_of(ty)) {
                        units.set_boat_location(other.who, other.o, tx, ty);
                        other = units.row(other.who, other.o).unwrap_or(other);
                        if move_other && (other.push_circles == 1 || me.domain == DOMAIN_LAND) {
                            other.collide_o = me.o as i16;
                            other.collide_who = me.who as i8;
                            units.write(other.who, other.o, &other);
                            if !other.has_orders {
                                units.face_pushed_idle_unit(
                                    other.who, other.o, push_angle, me.who, me.o,
                                );
                            }
                        }
                        other = units.row(other_who, other_o).unwrap_or(other);
                        other.collide_frame = units.frame();
                        units.write(other_who, other_o, &other);
                    }
                }
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// 8. `Unit::detect_unit_collision`  [measured]
// ---------------------------------------------------------------------------

/// The five flag parameters of `Unit::detect_unit_collision(Coord, Coord, int, int, int, int, int)`.
///
/// Retail call sites [measured]:
///
/// | caller | `probe` | `boat` | `overlay` | `skip_pre` |
/// |---|---|---|---|---|
/// | `PathFinder::valid_ucoord` `0x00687C80` | 1 | 1 | 1 | 0 |
/// | `Unit::move_step` `0x005FAF30` (step test) | 0 | 1 | 0 | 0 |
/// | `Unit::move_step` (waypoint re-test) | 1 | 1 | 0 | 0 |
/// | `Unit::resolve_unit_collision` (x4, detour candidates) | 1 | 1 | 0 | 0 |
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DetectArgs {
    /// `param_3`. Probe only: return as soon as the bitmap answers, never record a blocker,
    /// never clear the previous one.
    pub probe: bool,
    /// `param_4`. Permits the `Unit::detect_boat_collision` `0x005FA8B0` pre-check.
    pub boat: bool,
    /// `param_6`. Routes `collide_here` through the pathfinder's scratch overlay and stops
    /// before writing the order's step destination.
    pub overlay: bool,
    /// `param_7`. Skip the whole pre-check block.
    pub skip_pre: bool,
}

impl DetectArgs {
    /// The `PathFinder::valid_ucoord` call: `(x, y, 1, 1, ?, 1, 0)`.
    pub const VALID_UCOORD: DetectArgs = DetectArgs {
        probe: true,
        boat: true,
        overlay: true,
        skip_pre: false,
    };
    /// The `Unit::move_step` step test: `(x, y, 0, 1, 0, 0, 0)`.
    pub const MOVE_STEP: DetectArgs = DetectArgs {
        probe: false,
        boat: true,
        overlay: false,
        skip_pre: false,
    };
    /// The `Unit::resolve_unit_collision` detour probes: `(x, y, 1, 1, 0, 0, 0)`.
    pub const DETOUR_PROBE: DetectArgs = DetectArgs {
        probe: true,
        boat: true,
        overlay: false,
        skip_pre: false,
    };
}

/// What `detect_unit_collision` decided, separated from the mutations so the read-only
/// (probe) path is provably read-only and the write path is auditable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detect {
    /// Returned 0 with **no state change** — every no-collision exit taken with `probe`.
    Clear,
    /// Returned 0 through the `0x006177FA` epilogue: forget the blocker, maybe reset the
    /// collide counter, drop [`umask::WAIT`], and possibly raise [`umask::YIELD`].
    ClearAndReset { yielded: bool },
    /// Returned 1 in probe mode: the bitmap says occupied and that is all the caller asked.
    HitProbe,
    /// Returned 1 having identified the blocker.
    Hit {
        blocker_who: i32,
        blocker_o: i32,
        /// The candidate position, written into the order for `resolve` to read back.
        /// `None` in `overlay` mode, where retail returns before that write.
        dest: Option<(i32, i32)>,
    },
}

impl Detect {
    /// Did the engine return 1?
    pub fn blocked(self) -> bool {
        matches!(self, Detect::HitProbe | Detect::Hit { .. })
    }

    /// Apply the mutations. `row` is the moving unit's row; the caller writes it back.
    pub fn apply<U: CollUnits>(self, units: &mut U, row: &mut UnitRow, frame: i32) {
        match self {
            Detect::Clear | Detect::HitProbe => {}
            Detect::ClearAndReset { yielded } => {
                if yielded {
                    row.unit_masks |= umask::YIELD;
                }
                row.collide_who = -1;
                row.collide_o = -1;
                if row.collide_frame < frame - 5 {
                    row.collide = 0;
                }
                row.unit_masks &= !umask::WAIT;
            }
            Detect::Hit {
                blocker_who,
                blocker_o,
                dest,
            } => {
                row.collide_o = blocker_o as i16;
                row.collide_who = blocker_who as i8;
                row.collide_guy = 0;
                if let Some((x, y)) = dest {
                    units.set_order_dest(row.who, row.o, x, y);
                }
            }
        }
    }
}

/// `Unit::detect_unit_collision` `0x00617060`.
///
/// "If I step to `(nx, ny)`, does a body stop me — and which one?"
///
/// Structure [measured]:
///
/// 1. **Air is exempt.** `domain == 2` skips everything.
/// 2. **Pre-checks** (`skip_pre` clear): heroes and supply units on a non-`1` domain fall
///    through to the main test; everything else, when `overlay` is clear, requires
///    `probe == 0`, `boat != 0` and a clean `detect_boat_collision`.
/// 3. **Two suppressions.** A path top record with flag `8`, or a non-zero
///    `UnitData+0xB2 safe` (the pathfinder's retry delay), skips the test entirely — a unit
///    that just failed to path does not also collide.
/// 4. **The bitmap.** If the candidate is in a different `UCoord` cell than the unit's own,
///    ask [`CollCheck::collide_here`]. Nothing set -> no collision.
/// 5. `probe` returns 1 here. This is the only thing `PathFinder::valid_ucoord` needs, and
///    it is why A\* validity costs one bitmap probe per distinct cell and nothing more.
/// 6. **The blocker hunt.** Walk the 3x3 `WCoord` neighbourhood of the candidate — the same
///    [`RING_X`]/[`RING_Y`] prefix — and inside each cell walk `WData.down` /
///    `ObjectData.down`, skipping self, dead objects and air. A candidate is the blocker if
///    it is on the map and [`is_here`] covers the cell `collide_here` reported.
/// 7. **The corner exemption.** If the reported cell is a corner of *my* footprint and the
///    diagonally *opposite* corner of theirs (`|a - b| == 4` in the 8-direction code), we
///    slip past and keep scanning. Everything else is a hard block.
/// 8. **The yield marks.** Four situations (`local_8`) record "bodies were in the way but
///    none of them blocks": both on trade routes and moving; the other guarding me; both
///    attacking, same owner, radius 1, and I am more than `0x300` out of attack range; or a
///    same-group unit that is idle or on a movement order. They raise [`umask::YIELD`] on
///    the *no-collision* exit, which halves the next step.
#[allow(clippy::too_many_arguments)]
pub fn detect_unit_collision<U: CollUnits>(
    w: &mut World,
    cc: &mut CollCheck,
    units: &mut U,
    me: &UnitRow,
    nx: i32,
    ny: i32,
    args: DetectArgs,
) -> Detect {
    // 1. air
    if me.domain == DOMAIN_AIR {
        return if args.probe {
            Detect::Clear
        } else {
            Detect::ClearAndReset { yielded: false }
        };
    }

    // 2. pre-checks. Regular land units bypass the specialised boat-body solver. Water,
    // siege, hero and supply units enter it only on the non-probe/non-overlay move-step arm.
    let siege = me.unit_flags & 0x0002_0000 != 0;
    let hero = me.unit_flags2 & 0x20 != 0;
    let supply = me.unit_flags2 & 0x40 != 0;
    if !args.skip_pre && (me.domain == DOMAIN_WATER || siege || hero || supply) && !args.overlay {
        if args.probe || !args.boat {
            return Detect::ClearAndReset { yielded: false };
        }
        if detect_boat_collision(units, me, nx, ny, true) {
            return Detect::ClearAndReset { yielded: false };
        }
    }

    // 3. suppressions
    let suppressed = me.path_top_flags & 8 != 0;
    if suppressed || me.safe != 0 {
        return if args.probe {
            Detect::Clear
        } else {
            Detect::ClearAndReset { yielded: false }
        };
    }

    // 4. the bitmap
    let ux = ucell_of(nx);
    let uy = ucell_of(ny);
    if ux == ucell_of(me.x) && uy == ucell_of(me.y) {
        return if args.probe {
            Detect::Clear
        } else {
            Detect::ClearAndReset { yielded: false }
        };
    }
    let hit = cc.collide_here(
        w,
        units,
        me.o,
        me.who,
        ux,
        uy,
        me.block_radius,
        args.overlay,
    );
    let Some((ox, oy)) = hit else {
        return if args.probe {
            Detect::Clear
        } else {
            Detect::ClearAndReset { yielded: false }
        };
    };

    let corner = will_be_corner(ox, oy, ux, uy, me.block_radius);

    // 5. probe mode stops here
    if args.probe {
        return Detect::HitProbe;
    }

    // 6/7/8. the blocker hunt
    let mut yielded = false;
    let my_attack_slack = if me.action == ORDER_ATTACK {
        units.attack_slack(me.who, me.o, nx, ny)
    } else {
        0
    };
    let wx = crate::systems::movement::wcell_of(nx);
    let wy = crate::systems::movement::wcell_of(ny);

    for i in 0..9usize {
        let cx = RING_X[i] + wx;
        let cy = RING_Y[i] + wy;
        if cx < 0 || cy < 0 || cx >= w.xs || cy >= w.ys {
            continue;
        }
        let rec = w.wdata(cx, cy);
        // `WorldData::get_down` `0x004613D0` returns "list non-empty" as `~down >> 15`.
        if rec.down < 0 {
            continue;
        }
        let mut cur_o = rec.down as i32;
        let mut cur_who = rec.down_who as i32;
        while cur_o >= 0 {
            let Some(other) = units.row(cur_who, cur_o) else {
                break;
            };
            let (next_o, next_who) = (other.down as i32, other.down_who as i32);
            let is_self = cur_o == me.o && cur_who == me.who;
            if !is_self && other.active && other.domain != DOMAIN_AIR {
                if other.on_map && is_here(ox, oy, &other) {
                    // -- the yield marks -------------------------------------------------
                    if me.action == ORDER_TRADE_ROUTE && other.action == ORDER_TRADE_ROUTE {
                        if me.moving && other.moving {
                            yielded = true;
                        }
                    } else if other.action == ORDER_GUARD {
                        if units.order_targets(other.who, other.o, me.who, me.o) {
                            yielded = true;
                        }
                    } else if other.action == ORDER_ATTACK
                        && me.action == ORDER_ATTACK
                        && other.who == me.who
                        && me.block_radius == 1
                        && other.block_radius == 1
                        && me.moving
                        && other.moving
                        && my_attack_slack > 0x300
                    {
                        yielded = true;
                    }

                    // -- the same-group arm ----------------------------------------------
                    if me.group == other.group
                        && me.group != -1
                        && me.action != ORDER_ATTACK
                        && !other.searching
                    {
                        if !other.has_orders {
                            yielded = true;
                        } else if other.order == ORDER_CAST_SPELL {
                            if matches!(other.spell_id, 0x28b | 0x28d | 0x28f | 0x291) {
                                yielded = true;
                            }
                        } else if other.order == ORDER_NONE
                            || other.order == ORDER_GUARD
                            || (matches!(other.order, 1 | 2 | 3 | 4 | 0x12 | 0x13 | 0x15)
                                && other.action != ORDER_ATTACK)
                        {
                            yielded = true;
                        }
                    }

                    // -- the corner exemption --------------------------------------------
                    let theirs = units.unit_corner(other.who, other.o, ox, oy);
                    if corner == 0 || (corner - theirs).abs() != 4 {
                        return Detect::Hit {
                            blocker_who: cur_who,
                            blocker_o: cur_o,
                            dest: if args.overlay { None } else { Some((nx, ny)) },
                        };
                    }
                }
            }
            cur_o = next_o;
            cur_who = next_who;
        }
    }

    Detect::ClearAndReset { yielded }
}

/// `OrderIndex` values this module branches on. Mirrors [`crate::order::OrderIndex`]; kept as
/// plain constants because the retail code compares raw integers and several arms name a
/// *set* of them.
pub const ORDER_NONE: i32 = 0;
pub const ORDER_ATTACK: i32 = 10;
pub const ORDER_GUARD: i32 = 12;
pub const ORDER_CAST_SPELL: i32 = 14;
pub const ORDER_TRADE_ROUTE: i32 = 15;

/// The order types that reach `resolve_unit_collision`'s detour arm — `MOVE_TO`, `ATTACK_TO`,
/// `EXPLORE_TO`, `FLEE_TO`, `CHANGE_FORM`, `GROUP_MOVE`, `GROUP_ATTACK_TO`. [measured]
#[inline]
pub fn is_movement_order(order: i32) -> bool {
    matches!(order, 1 | 2 | 3 | 4 | 0x12 | 0x13 | 0x15)
}

// ---------------------------------------------------------------------------
// 8. `Unit::resolve_unit_collision`  [measured]
// ---------------------------------------------------------------------------

/// What `resolve_unit_collision` chose to do. Retail always returns 1; the interesting
/// output is which arm ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolve {
    /// The detour arm: a single sidestep waypoint was pushed. **This is what "pushing" is** —
    /// a local `find_upath` detour, never an impulse.
    Detour { to: (i32, i32) },
    /// The wait arm: [`umask::WAIT`] raised, hold position this frame.
    Wait,
    /// The repath arm ran: waypoints were popped back to a usable one, the unit was snapped
    /// to its cell centre and `find_upath` re-run.
    Repath {
        /// `Random::get(0, 0xFFFF) % 9 + 1` was drawn and stored as the order's wait.
        /// **This is an RNG consumer on `game_random`.**
        wait_drawn: Option<i32>,
        /// Whether `PathFinder::find_upath` returned non-zero. This includes retail's raw
        /// `-1` suspended result as well as `1` found; only `0` failure is false.
        found: bool,
    },
    /// The per-owner repath budget or the modulo throttle refused this unit this frame.
    Throttled,
    /// Nothing applied — no path stack, or an arm this lane does not own.
    None,
}

/// `Unit::resolve_unit_collision` `0x005F9D30`, the arms this lane owns.
///
/// Called from `Unit::move_step` `0x005FAF30` only [measured, sole caller], after
/// `detect_unit_collision` returned 1 and stored the step target in the order.
///
/// Order of arms, verbatim:
///
/// 1. **Detour.** For a land-domain unit on a movement order whose path top is *not* a unit
///    waypoint (`flags & 2` clear), take the recorded step target, compute the `UCoord` delta
///    from the unit, and try two alternatives: for a diagonal step the two orthogonal legs
///    `(tx, uy)` and `(ux, ty)`; for anything else the perpendicular pair
///    `(tx - dy, ty - dx)` and `(tx + dy, ty + dx)`. The first one that is neither
///    `invalid_loc` nor occupied becomes a `flags = 2` waypoint and the function returns.
///    A step onto the unit's own cell is a shipped assertion, `"Collided in my space?"`.
/// 2. **Bookkeeping.** `collide += 1`, `collide_frame = frame`.
/// 3. **Wait.** For a movement order other than `FLEE_TO` (or a spell with id `0x28A`):
///    if neither unit has collided too many times (`0x20`, or `0x80` once the owner's repath
///    budget is above 3), the blocker is not an enemy, the blocker is not *itself* blocked by
///    me, and neither of us already has [`umask::WAIT`] up — just wait a frame.
/// 4. **Repath.** Otherwise, and only if the owner's budget allows: pop waypoints until one
///    is "far enough" (`tolerance >= 0x60`, not a unit waypoint) or unoccupied, push it back,
///    snap to the cell centre via `Unit::set_new_location`, and re-run `find_upath`. If the
///    search returned non-zero (**including its `-1` suspended result**) **and** the blocker
///    is mutually blocked on me, draw
///    `Random::get(0, 0xFFFF) % 9 + 1` and store it as the order's wait.
///
/// The throttle is worth stating on its own, because it shapes the whole crowd behaviour:
/// `GameDaemon + who*4` counts repaths for this owner this frame. Under 4, everyone may
/// repath. From 4, only units with `(o + collide) % 4 == 0` may, and their collide counter is
/// divided by 4 first. From 8, additionally `(o + collide/4) % 16 == 0`. At 16 nobody repaths
/// at all. A dense crowd therefore degrades to waiting, by construction.
#[allow(clippy::too_many_arguments)]
pub fn resolve_unit_collision<U, F>(
    w: &mut World,
    cc: &mut CollCheck,
    units: &mut U,
    me: &mut UnitRow,
    path: &mut PathStack,
    rng: &mut Random,
    invalid_loc: &dyn Fn(i32, i32) -> bool,
    mut find_upath: F,
) -> Resolve
where
    U: CollUnits,
    // Retail tests only `find_upath(...) == 0`; adapters must map both Found and Suspended
    // to `true`, and only Failed to `false`.
    F: FnMut(&mut PathStack, bool) -> bool,
{
    let frame = units.frame();

    // -- 1. the detour arm ---------------------------------------------------------------
    if is_movement_order(me.order) && me.domain == DOMAIN_LAND {
        let top_is_waypoint = path
            .peek()
            .map(|r| r.flags & PathData::FLAG_WAYPOINT != 0)
            .unwrap_or(false);
        if !top_is_waypoint {
            if let Some((dx_w, dy_w)) = units.order_dest(me.who, me.o) {
                let tx = ucell_of(dx_w);
                let ty = ucell_of(dy_w);
                let ux = ucell_of(me.x);
                let uy = ucell_of(me.y);
                let dx = tx - ux;
                let dy = ty - uy;
                // `if (dx == 0 && dy == 0)` is the shipped `"Collided in my space?"` assert;
                // it does not change control flow, so nothing is done with it here.
                if !(dx == 0 && dy == 0) {
                    let candidates = if dx.abs() == dy.abs() {
                        [(tx, ty - dy), (tx - dx, ty)]
                    } else {
                        [(tx - dy, ty - dx), (tx + dy, ty + dx)]
                    };
                    for (cux, cuy) in candidates {
                        if invalid_loc(cux >> 2, cuy >> 2) {
                            continue;
                        }
                        let cx = ucell_centre(cux);
                        let cy = ucell_centre(cuy);
                        let probe = detect_unit_collision(
                            w,
                            cc,
                            units,
                            me,
                            cx,
                            cy,
                            DetectArgs::DETOUR_PROBE,
                        );
                        if !probe.blocked() {
                            path.push(PathData {
                                to_x: cx,
                                to_y: cy,
                                tolerance: 0,
                                flags: PathData::FLAG_WAYPOINT,
                            });
                            units.set_order_detour(me.who, me.o, cx, cy);
                            return Resolve::Detour { to: (cx, cy) };
                        }
                    }
                }
            }
        }
    }

    // -- 2. bookkeeping -------------------------------------------------------------------
    me.collide = me.collide.wrapping_add(1);
    me.collide_frame = frame;

    // -- 3. the wait arm ------------------------------------------------------------------
    let wait_eligible = if me.order == 4 {
        false
    } else if is_movement_order(me.order) {
        true
    } else {
        me.order == ORDER_CAST_SPELL && me.spell_id == 0x28a
    };
    if wait_eligible {
        let budget = units.repath_budget(me.who);
        let limit: i16 = if budget > 3 { 0x80 } else { 0x20 };
        if let Some(other) = units.row(me.collide_who as i32, me.collide_o as i32) {
            let mutual = other.collide_o as i32 == me.o && other.collide_who as i32 == me.who;
            let their_blocker_waiting = if other.unit_masks & umask::WAIT == 0 {
                false
            } else if other.collide_o >= 0 {
                match units.row(other.collide_who as i32, other.collide_o as i32) {
                    Some(b) => b.unit_masks & umask::WAIT != 0,
                    None => true,
                }
            } else {
                true
            };
            if other.collide < limit
                && me.collide < limit
                && !units.is_enemy(me.who, me.collide_who as i32)
                && !mutual
                && !their_blocker_waiting
            {
                me.unit_masks |= umask::WAIT;
                return Resolve::Wait;
            }
        }
    }

    // -- 4. the repath arm ----------------------------------------------------------------
    if path.is_empty() {
        return Resolve::None;
    }
    let budget = units.repath_budget(me.who);
    let mut k = me.collide as i32;
    if budget >= 4 {
        if (me.o + k) & 3 != 0 {
            return Resolve::Throttled;
        }
        // `(int)((k >> 31 & 3) + k) >> 2` — division by 4 truncating toward zero.
        k = ((k >> 31 & 3) + k) >> 2;
    }
    if budget >= 0x10 || (budget >= 8 && (me.o + k) & 0xf != 0) {
        return Resolve::Throttled;
    }
    units.bump_repath_budget(me.who);

    // Pop waypoints until one is usable, then push it back. Retail's `Stack<PathData>::pop`
    // clamps the count to 1 before decrementing, so an empty stack yields `records[0]`.
    let mut rec;
    loop {
        rec = path.pop_clamped();
        let tx = crate::systems::movement::tile_of(rec.to_x);
        let ty = crate::systems::movement::tile_of(rec.to_y);
        let blocked_terrain = w.valid_t(tx, ty) && w.tmask(tx, ty) & 0x6000 != 0;
        let occupied = if (rec.tolerance > 0x5f || rec.flags & PathData::FLAG_WAYPOINT == 0)
            && !blocked_terrain
        {
            cc.collide_here(
                w,
                units,
                me.o,
                me.who,
                ucell_of(rec.to_x),
                ucell_of(rec.to_y),
                me.block_radius,
                true,
            )
            .is_some()
        } else {
            blocked_terrain
        };
        let more = rec.flags & PathData::FLAG_MORE == 0
            && (rec.tolerance < 0x60 || rec.flags & PathData::FLAG_WAYPOINT != 0 || occupied);
        if !more || path.is_empty() {
            break;
        }
    }
    path.push(rec);

    // `Unit::set_new_location(ucell_centre(ux), ucell_centre(uy), 1, 0)` — snap to the cell
    // centre before searching, so the search starts from a grid-aligned position.
    me.x = ucell_centre(ucell_of(me.x));
    me.y = ucell_centre(ucell_of(me.y));

    let quick = me.action == ORDER_ATTACK;
    units.clear_order_retry(me.who, me.o);
    let found = find_upath(path, quick);
    let mut wait_drawn = None;
    if found {
        if let Some(other) = units.row(me.collide_who as i32, me.collide_o as i32) {
            if other.collide_o as i32 == me.o
                && other.collide_who as i32 == me.who
                && other.unit_masks & umask::WAIT == 0
            {
                let r = rng.get(0, 0xFFFF);
                let t = r % 9 + 1;
                units.set_order_wait(me.who, me.o, t);
                wait_drawn = Some(t);
            }
        }
    }
    Resolve::Repath { wait_drawn, found }
}

// ---------------------------------------------------------------------------
// 9. `GuyData::turn_speed` `0x005DE340`  [measured]
// ---------------------------------------------------------------------------

/// `Constants+0x08 unit_turn_speed`, shipped `256` — "1/1 rate (master control for unit turn
/// speed)", parsed by `String::fraction` with scale 256.
pub const UNIT_TURN_SPEED: i32 = 256;
/// `Constants+0x0C unit_pack_turn_bonus`, shipped `2` — "2x (units turn faster when packed)".
pub const UNIT_PACK_TURN_BONUS: i32 = 2;
/// The floor `turn_speed` never goes below: `unit_turn_speed * 0xB60B`. With the shipped 256
/// that is `0x00B60B00`, which is 2^32/360 to within 416 parts — **one degree per tick**.
pub const TURN_SPEED_FLOOR_MUL: i32 = 0xb60b;
/// The value returned when the guy is beyond `squad_size`, i.e. a decorative guy: a quarter
/// turn, so it snaps instantly.
pub const TURN_SPEED_FREE: u32 = 0x4000_0000;
/// The value returned for a guy with `last_speed == 0` and `guy_flags & 0x10`.
pub const TURN_SPEED_HALF: u32 = 0x8000_0000;

/// The `GuyData` fields `turn_speed` reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TurnGuy {
    /// `+0xA2 guy_num`, compared against `UnitTypeData+0x304 squad_size`.
    pub guy_num: i32,
    /// `+0x54 track_dx` / `+0x58 track_dy`.
    pub track_dx: i32,
    pub track_dy: i32,
    /// `+0x80 last_speed` / `+0x84 avg_speed`.
    pub last_speed: i32,
    pub avg_speed: i32,
    /// `+0x9A guy_flags`.
    pub guy_flags: u16,
}

/// `GuyData::turn_speed(int raw)` `0x005DE340` — the turn rate, in units of 2^32 per full turn.
///
/// `raw != 0` returns the type rate untouched; `raw == 0` — which is what `Unit::move_step`
/// passes — divides it by `avg_speed / 4 + 1` and floors it at one degree.
///
/// This is the arm `tick.rs` currently substitutes `i32::MAX` for, which is why units in the
/// driver turn instantly. Ported here so a driver can pass the real number.
pub fn turn_speed(
    guy: &TurnGuy,
    type_turn_speed: i32,
    squad_size: i32,
    obj_masks: u32,
    raw: bool,
) -> u32 {
    let mut rate: u32 = TURN_SPEED_FREE;
    if guy.guy_num < squad_size {
        rate = ((type_turn_speed as u32) >> 8).wrapping_mul(UNIT_TURN_SPEED as u32);
        if obj_masks & 0x0008_0000 != 0 {
            rate = rate.wrapping_mul(UNIT_PACK_TURN_BONUS as u32);
        }
    } else {
        if guy.track_dx != 0 || guy.track_dy != 0 {
            return TURN_SPEED_FREE;
        }
    }
    if guy.last_speed == 0 && guy.guy_flags & 0x10 != 0 {
        return TURN_SPEED_HALF;
    }
    if raw {
        return rate;
    }
    let div = ((((guy.avg_speed >> 31) & 3) + guy.avg_speed) >> 2) as u32 + 1;
    let scaled = rate / div;
    let floor = (UNIT_TURN_SPEED as u32).wrapping_mul(TURN_SPEED_FLOOR_MUL as u32);
    scaled.max(floor)
}

/// `Unit::move_step`'s turn block, `0x005FAF9E`..`0x005FB0??` [measured].
///
/// Given the desired heading, the current heading and the rate, returns
/// `(new_heading, residual)`. `residual == 0` means the unit is facing the target closely
/// enough to translate this tick; a non-zero residual is what makes `move_step` return
/// `TurnedOnly`.
///
/// The `0x02222220` dead-band is 4.8 degrees: a heading error under it is simply erased.
pub fn turn_toward(desired: i32, current: i32, rate: u32) -> (i32, u32) {
    let diff = (desired as u32).wrapping_sub(current as u32);
    let mut mag = if diff > 0x8000_0000 { !diff } else { diff };
    if mag < 0x0222_2220 || mag <= rate {
        return (desired, 0);
    }
    mag -= rate;
    let new = if diff < 0x8000_0001 {
        (current as u32).wrapping_add(rate)
    } else {
        (current as u32).wrapping_sub(rate)
    };
    (new as i32, mag)
}

// ---------------------------------------------------------------------------
// 10. The A* validity hook
// ---------------------------------------------------------------------------

/// `PathFinder::valid_ucoord` `0x00687C80`'s collision half, isolated.
///
/// This is the *whole* answer A\* needs, and it is much cheaper than the full
/// `detect_unit_collision`: the probe call passes `probe = 1`, so it stops the moment the
/// bitmap answers and never walks the `down` lists. `movement::PathFinder::valid_ucoord`
/// memoises by cell metric, so it runs at most once per distinct 48-unit cell per search.
///
/// Returns `true` when the cell is **blocked**, i.e. what `UnitWorld::unit_collides` wants.
pub fn unit_collides<U: CollUnits>(
    w: &mut World,
    cc: &mut CollCheck,
    units: &mut U,
    me: &UnitRow,
    x: i32,
    y: i32,
) -> bool {
    detect_unit_collision(w, cc, units, me, x, y, DetectArgs::VALID_UCOORD).blocked()
}

/// A tiny in-module [`CollUnits`] backed by a flat table, so this module's own tests and any
/// caller that only needs the bitmap can drive `collide_here` without wiring a world.
///
/// Not a mirror of the tick's storage: it exists so the probe path is testable in isolation.
#[derive(Clone, Debug)]
pub struct UnitTable {
    pub rows: Vec<UnitRow>,
    /// Live, non-null squad guys, kept in each unit's pointer-array order.
    pub guys: Vec<TableGuy>,
    pub boat_queries: Vec<BoatQuery>,
    pub effective_owners: [i32; 10],
    pub diplos: [[i32; 10]; 10],
    pub invalid_boat_locations: Vec<(i32, i32, i32, i32)>,
    pub boat_moves: Vec<BoatMove>,
    pub boat_faces: Vec<BoatFace>,
    pub frame: i32,
    pub budget: [i32; 10],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TableGuy {
    pub who: i32,
    pub o: i32,
    pub body: CollGuy,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoatMove {
    pub who: i32,
    pub o: i32,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoatFace {
    pub who: i32,
    pub o: i32,
    pub angle: i32,
    pub pusher_who: i32,
    pub pusher_o: i32,
}

impl Default for UnitTable {
    fn default() -> Self {
        let effective_owners = std::array::from_fn(|i| i as i32);
        let diplos = std::array::from_fn(|i| std::array::from_fn(|j| if i == j { 2 } else { 0 }));
        Self {
            rows: Vec::new(),
            guys: Vec::new(),
            boat_queries: Vec::new(),
            effective_owners,
            diplos,
            invalid_boat_locations: Vec::new(),
            boat_moves: Vec::new(),
            boat_faces: Vec::new(),
            frame: 0,
            budget: [0; 10],
        }
    }
}

impl UnitTable {
    pub fn find(&self, who: i32, o: i32) -> Option<usize> {
        self.rows.iter().position(|r| r.who == who && r.o == o)
    }
}

impl CollUnits for UnitTable {
    fn row(&self, who: i32, o: i32) -> Option<UnitRow> {
        self.find(who, o).map(|i| self.rows[i])
    }
    fn unit_corner(&self, who: i32, o: i32, cx: i32, cy: i32) -> i32 {
        let Some(row) = self.row(who, o) else {
            return 0;
        };
        if !row.active || !row.on_map {
            return 0;
        }
        unit_corner(
            cx,
            cy,
            self.guys
                .iter()
                .filter(|g| g.who == who && g.o == o)
                .map(|g| g.body),
        )
    }
    fn find_boat_units(&mut self, query: BoatQuery) -> Vec<(i32, i32)> {
        self.boat_queries.push(query);
        self.rows
            .iter()
            .filter(|r| r.who != query.exclude_who || r.o != query.exclude_o)
            .map(|r| (r.who, r.o))
            .collect()
    }
    fn effective_owner(&self, who: i32) -> i32 {
        self.effective_owners[who.clamp(0, 9) as usize]
    }
    fn diplomacy(&self, who: i32, other: i32) -> i32 {
        self.diplos[who.clamp(0, 9) as usize][other.clamp(0, 9) as usize]
    }
    fn boat_invalid_loc(&self, who: i32, o: i32, tx: i32, ty: i32) -> bool {
        self.invalid_boat_locations.contains(&(who, o, tx, ty))
    }
    fn set_boat_location(&mut self, who: i32, o: i32, x: i32, y: i32) {
        self.boat_moves.push(BoatMove { who, o, x, y });
        if let Some(i) = self.find(who, o) {
            self.rows[i].x = x;
            self.rows[i].y = y;
        }
    }
    fn face_pushed_idle_unit(
        &mut self,
        who: i32,
        o: i32,
        angle: i32,
        pusher_who: i32,
        pusher_o: i32,
    ) {
        self.boat_faces.push(BoatFace {
            who,
            o,
            angle,
            pusher_who,
            pusher_o,
        });
        if let Some(i) = self.find(who, o) {
            self.rows[i].angle = angle;
            self.rows[i].first_guy_angle = angle;
        }
    }
    fn write(&mut self, who: i32, o: i32, r: &UnitRow) {
        if let Some(i) = self.find(who, o) {
            self.rows[i] = *r;
        }
    }
    fn is_enemy(&self, me: i32, them: i32) -> bool {
        me != them
    }
    fn order_dest(&self, _who: i32, _o: i32) -> Option<(i32, i32)> {
        None
    }
    fn set_order_dest(&mut self, _who: i32, _o: i32, _x: i32, _y: i32) {}
    fn set_order_detour(&mut self, _who: i32, _o: i32, _x: i32, _y: i32) {}
    fn set_order_wait(&mut self, _who: i32, _o: i32, _t: i32) {}
    fn clear_order_retry(&mut self, _who: i32, _o: i32) {}
    fn order_targets(&self, _who: i32, _o: i32, _tw: i32, _to: i32) -> bool {
        false
    }
    fn attack_slack(&self, _who: i32, _o: i32, _nx: i32, _ny: i32) -> i32 {
        0
    }
    fn repath_budget(&self, who: i32) -> i32 {
        self.budget[who.clamp(0, 9) as usize]
    }
    fn bump_repath_budget(&mut self, who: i32) {
        self.budget[who.clamp(0, 9) as usize] += 1;
    }
    fn frame(&self) -> i32 {
        self.frame
    }
}

/// Relocate an already-linked unit anchor as the spatial half of
/// `Unit::set_new_location` `0x005F8D20`.
///
/// Retail compares the old and new `WCoord` cells.  When they differ it calls
/// `Object::remove_from_world` `0x00647970` **before** writing `ObjectData::x/y`, then
/// `Object::add_to_world` `0x0064D8C0` **after** the write.  The object is consequently
/// removed from its old cell without disturbing the relative order of its neighbours and
/// inserted at the head of the new cell.  A move inside one `WData` cell only writes the
/// coordinates and leaves the intrusive-list order untouched.
///
/// This intentionally does not call [`place`]: ordinary movement must neither append a new
/// table row nor stamp another initial set of guy footprints.  Per-guy bitmap relocation is
/// the separate `Guy::set_new_location` phase.
///
/// Returns `false` without changing anything if `(who, o)` is absent or a valid old cell's
/// chain does not contain it.  The latter is a broken-world invariant in retail (its removal
/// path asserts on linked-list loops), but a recoverable answer is more useful to hosts.
pub fn relocate_unit_anchor(
    w: &mut World,
    table: &mut UnitTable,
    who: i32,
    o: i32,
    x: i32,
    y: i32,
) -> bool {
    let Some(row_i) = table.find(who, o) else {
        return false;
    };
    let old = (
        crate::systems::movement::wcell_of(table.rows[row_i].x),
        crate::systems::movement::wcell_of(table.rows[row_i].y),
    );
    let new = (
        crate::systems::movement::wcell_of(x),
        crate::systems::movement::wcell_of(y),
    );

    if old == new {
        table.rows[row_i].x = x;
        table.rows[row_i].y = y;
        return true;
    }

    // Resolve the old splice before mutating anything.  `None` means the actor is the
    // WData head; `Some(i)` is its predecessor in the intrusive `down` chain.
    let predecessor = if w.valid_w(old.0, old.1) {
        let head = w.wdata(old.0, old.1);
        if head.down as i32 == o && head.down_who as i32 == who {
            Some(None)
        } else {
            let mut cur = (head.down_who as i32, head.down as i32);
            let mut found = None;
            let mut hops = 0usize;
            while cur.1 >= 0 && hops <= table.rows.len() {
                let Some(i) = table.find(cur.0, cur.1) else {
                    break;
                };
                let next = (table.rows[i].down_who as i32, table.rows[i].down as i32);
                if next == (who, o) {
                    found = Some(i);
                    break;
                }
                cur = next;
                hops += 1;
            }
            found.map(Some)
        }
    } else {
        // Off-map sentinels are not linked into WData.
        Some(None)
    };
    let Some(predecessor) = predecessor else {
        return false;
    };

    let actor_next = (table.rows[row_i].down, table.rows[row_i].down_who);
    if w.valid_w(old.0, old.1) {
        if let Some(pred_i) = predecessor {
            table.rows[pred_i].down = actor_next.0;
            table.rows[pred_i].down_who = actor_next.1;
        } else {
            w.set_down(old.0, old.1, actor_next.0, actor_next.1);
        }
    }

    // Object::remove_from_world clears the packed down/down_who dword to -1 before the
    // encrypted coordinate words are replaced.
    table.rows[row_i].down = -1;
    table.rows[row_i].down_who = -1;
    table.rows[row_i].x = x;
    table.rows[row_i].y = y;

    if w.valid_w(new.0, new.1) {
        let head = w.wdata(new.0, new.1);
        table.rows[row_i].down = head.down;
        table.rows[row_i].down_who = head.down_who;
        w.set_down(new.0, new.1, o as i16, who as i16);
    }
    true
}

/// Place a unit in the world: stamp its footprint and link it into its `WData` cell's `down`
/// list, exactly as `Guy::set_new_location` + `Object::set_down` do.
///
/// The `down` list is a stack: the newest object becomes the head. That order is what decides
/// **which** of several overlapping units `detect_unit_collision` reports as the blocker, so
/// it is sim-critical, not incidental.
pub fn place(
    w: &mut World,
    table: &mut UnitTable,
    mut row: UnitRow,
    guys: impl IntoIterator<Item = CollGuy>,
) {
    // Each live squad guy owns a separate square. `move_unit` from an off-map sentinel
    // would clear bits we do not own, so initial placement stamps them directly.
    for guy in guys {
        let ux = ucell_of(guy.x);
        let uy = ucell_of(guy.y);
        let n = ring_count(guy.block_radius);
        if guy.block_radius > 0 && row.domain != DOMAIN_AIR {
            for i in 0..n as usize {
                let cx = RING_X[i] + ux;
                let cy = RING_Y[i] + uy;
                if cx < 0 || cy < 0 || cx >= w.xs * BLOCK_UCELLS || cy >= w.ys * BLOCK_UCELLS {
                    continue;
                }
                let (bx, by) = (cx >> 4, cy >> 4);
                if w.wdata(bx, by).block.is_none() {
                    w.new_coll_block(bx, by);
                }
                let bi = w.w_index(bx, by);
                let b = w.wdata[bi].block.as_deref_mut().unwrap();
                block_set(b, cx, cy, true);
            }
        }
        table.guys.push(TableGuy {
            who: row.who,
            o: row.o,
            body: guy,
        });
    }
    let (wx, wy) = (
        crate::systems::movement::wcell_of(row.x),
        crate::systems::movement::wcell_of(row.y),
    );
    if w.valid_w(wx, wy) {
        let rec = w.wdata(wx, wy);
        row.down = rec.down;
        row.down_who = rec.down_who;
        w.set_down(wx, wy, row.o as i16, row.who as i16);
    } else {
        row.down = -1;
        row.down_who = -1;
    }
    table.rows.push(row);
}

/// The obfuscation constant, re-exported so callers that read raw `ObjectData` words do not
/// have to reach into `movement`. `GuyData`'s coordinates are **not** obfuscated.
pub const OBJECT_COORD_XOR: i32 = COORD_XOR;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    fn world(xs: i32, ys: i32) -> World {
        World::init(xs, ys, 44, 4, 4)
    }

    fn row(who: i32, o: i32, ux: i32, uy: i32, radius: i32) -> UnitRow {
        UnitRow {
            who,
            o,
            x: ucell_centre(ux),
            y: ucell_centre(uy),
            down: -1,
            down_who: -1,
            domain: DOMAIN_LAND,
            block_radius: radius,
            group: -1,
            collide_o: -1,
            collide_who: -1,
            on_map: true,
            active: true,
            ..UnitRow::default()
        }
    }

    fn guy(ux: i32, uy: i32, radius: i32) -> CollGuy {
        CollGuy {
            x: ucell_centre(ux),
            y: ucell_centre(uy),
            block_radius: radius,
        }
    }

    fn boat_row(who: i32, o: i32, x: i32, y: i32) -> UnitRow {
        UnitRow {
            who,
            o,
            x,
            y,
            down: -1,
            down_who: -1,
            domain: DOMAIN_WATER,
            push_size: 96,
            push_circles: 1,
            group: -1,
            collide_o: -1,
            collide_who: -1,
            on_map: true,
            active: true,
            ..UnitRow::default()
        }
    }

    fn table_bytes(a: &[i32]) -> Vec<u8> {
        a.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn retail_ring_tables_match_binary_byte_digests() {
        // Raw bytes at .rdata VAs 0x00ADCAF0 and 0x00ADC400 in
        // ron-bin/riseofnations.exe. These fail if any literal, order, or sign changes.
        assert_eq!(
            crate::checksum::adler32(1, &table_bytes(&RING_X)),
            0x6c7b45b8
        );
        assert_eq!(
            crate::checksum::adler32(1, &table_bytes(&RING_Y)),
            0x98df41bb
        );
        let mut both = table_bytes(&RING_X);
        both.extend(table_bytes(&RING_Y));
        assert_eq!(crate::checksum::adler32(1, &both), 0x82858772);
        assert_eq!(RING_COUNT, [1, 9, 25, 49, 81, 121, 169, 225, 289, 361, 441]);
        assert_eq!(BLOCK_DX, [0, 0, 1, 1]);
        assert_eq!(BLOCK_DY, [0, 1, 0, 1]);
    }

    #[test]
    fn retail_rings_zero_through_seven_are_complete_in_ordered_prefixes() {
        let mut start = 0usize;
        for radius in 0..=7i32 {
            let end = RING_COUNT[radius as usize] as usize;
            let got: BTreeSet<_> = (start..end).map(|i| (RING_X[i], RING_Y[i])).collect();
            let want: BTreeSet<_> = (-radius..=radius)
                .flat_map(|x| (-radius..=radius).map(move |y| (x, y)))
                .filter(|&(x, y)| radius == 0 || x.abs().max(y.abs()) == radius)
                .collect();
            assert_eq!(got, want, "radius {radius}");
            assert_eq!(end - start, got.len(), "radius {radius} has a duplicate");
            start = end;
        }
    }

    #[test]
    fn retail_ring_suffix_anomalies_are_preserved_not_repaired() {
        let expected = [(288, (-8, -16)), (360, (-9, -10)), (440, (-10, 0))];
        for (i, pair) in expected {
            assert_eq!((RING_X[i], RING_Y[i]), pair);
        }
        let ring9: Vec<_> = (289..361).map(|i| (RING_X[i], RING_Y[i])).collect();
        let ring10: Vec<_> = (361..441).map(|i| (RING_X[i], RING_Y[i])).collect();
        assert_eq!(ring9.iter().filter(|&&p| p == (9, 9)).count(), 2);
        assert!(!ring9.contains(&(-9, 9)) && !ring9.contains(&(9, -9)));
        assert_eq!(ring10.iter().filter(|&&p| p == (10, 10)).count(), 2);
        assert_eq!(ring10.iter().filter(|&&p| p == (-10, 0)).count(), 2);
        assert!(!ring10.contains(&(-10, 10)) && !ring10.contains(&(10, -10)));
    }

    #[test]
    fn collblock_negative_coordinates_and_flag_tristate_match_retail() {
        let mut b = CollBlock::default();
        assert_eq!(coll_bit(-1, -1), 255);
        assert!(block_empty(&mut b));
        assert_eq!(b.flags, 1);
        block_set(&mut b, -1, -1, true);
        assert!(block_get(&b, 15, 15));
        assert_eq!(b.flags, 0);
        assert!(!block_empty(&mut b));
        block_set(&mut b, 15, 15, false);
        assert_eq!(b.flags, 2);
        assert!(block_empty(&mut b));
        assert_eq!(b.flags, 1);
    }

    #[test]
    fn place_stamps_every_live_guy_not_a_unit_anchor_approximation() {
        let mut w = world(3, 3);
        let mut units = UnitTable::default();
        place(
            &mut w,
            &mut units,
            row(0, 7, 10, 10, 1),
            [guy(10, 10, 1), guy(14, 10, 1)],
        );
        let b = w.wdata(0, 0).block.as_deref().unwrap();
        for x in 9..=11 {
            for y in 9..=11 {
                assert!(block_get(b, x, y), "first guy ({x},{y})");
            }
        }
        for x in 13..=15 {
            for y in 9..=11 {
                assert!(block_get(b, x, y), "second guy ({x},{y})");
            }
        }
        assert!(
            !block_get(b, 12, 10),
            "the gap is not filled by a unit-wide square"
        );
        assert_eq!(units.guys.len(), 2);
    }

    #[test]
    fn relocate_anchor_splices_old_chain_and_pushes_new_head() {
        let mut w = world(3, 2);
        let mut units = UnitTable::default();
        // All three start in WData (0,0), newest first: 3 -> 2 -> 1.
        for o in 1..=3 {
            place(&mut w, &mut units, row(0, o, 2 + o, 4, 0), []);
        }
        assert_eq!((w.wdata(0, 0).down, w.wdata(0, 0).down_who), (3, 0));
        assert_eq!(
            (
                units.row(0, 3).unwrap().down,
                units.row(0, 3).unwrap().down_who
            ),
            (2, 0)
        );
        assert_eq!(
            (
                units.row(0, 2).unwrap().down,
                units.row(0, 2).unwrap().down_who
            ),
            (1, 0)
        );

        // Move the middle node into WData (1,0).  The untouched old neighbours retain
        // their order, while the moved actor becomes the new-cell head.
        let nx = crate::systems::map_terrain::COORD_PER_WCELL + 24;
        assert!(relocate_unit_anchor(&mut w, &mut units, 0, 2, nx, 24));
        assert_eq!((w.wdata(0, 0).down, w.wdata(0, 0).down_who), (3, 0));
        assert_eq!(
            (
                units.row(0, 3).unwrap().down,
                units.row(0, 3).unwrap().down_who
            ),
            (1, 0)
        );
        assert_eq!(
            (
                units.row(0, 1).unwrap().down,
                units.row(0, 1).unwrap().down_who
            ),
            (-1, 0)
        );
        assert_eq!((w.wdata(1, 0).down, w.wdata(1, 0).down_who), (2, 0));
        assert_eq!(
            (
                units.row(0, 2).unwrap().down,
                units.row(0, 2).unwrap().down_who
            ),
            (-1, 0)
        );
        assert_eq!(
            (units.row(0, 2).unwrap().x, units.row(0, 2).unwrap().y),
            (nx, 24)
        );
    }

    #[test]
    fn relocate_anchor_inside_one_wcell_preserves_blocker_precedence() {
        let mut w = world(2, 2);
        let mut units = UnitTable::default();
        place(&mut w, &mut units, row(0, 1, 3, 3, 0), []);
        place(&mut w, &mut units, row(1, 7, 4, 4, 0), []);
        let head_before = (w.wdata(0, 0).down, w.wdata(0, 0).down_who);
        let next_before = (
            units.row(1, 7).unwrap().down,
            units.row(1, 7).unwrap().down_who,
        );

        assert!(relocate_unit_anchor(&mut w, &mut units, 0, 1, 700, 600));
        assert_eq!((w.wdata(0, 0).down, w.wdata(0, 0).down_who), head_before);
        assert_eq!(
            (
                units.row(1, 7).unwrap().down,
                units.row(1, 7).unwrap().down_who
            ),
            next_before
        );
        assert_eq!(
            (units.row(0, 1).unwrap().x, units.row(0, 1).unwrap().y),
            (700, 600)
        );
    }

    #[test]
    fn unit_corner_walks_live_guys_in_pointer_array_order() {
        assert_eq!(
            unit_corner(13, 9, [guy(10, 10, 1), guy(14, 10, 1)]),
            1,
            "NW corner belongs to the second guy, not the unit anchor"
        );
        assert_eq!(unit_corner(11, 11, [guy(10, 10, 1), guy(12, 12, 1)]), 5);
        assert_eq!(unit_corner(40, 40, std::iter::empty()), 0);
    }

    #[test]
    fn overlay_uses_private_collblock_clones() {
        let mut w = world(2, 2);
        let b = w.new_coll_block(0, 0);
        block_set(b, 3, 4, true);
        let mut cc = CollCheck::new();
        cc.fill_slots(&w, 3, 4, 1, true);
        assert!(block_get(cc.scratch[0].as_ref().unwrap(), 3, 4));
        block_set(cc.scratch[0].as_mut().unwrap(), 3, 4, false);
        assert!(block_get(w.wdata(0, 0).block.as_deref().unwrap(), 3, 4));
    }

    #[test]
    fn astar_probe_sees_a_per_guy_stamp_without_mutating_order_state() {
        let mut w = world(3, 3);
        let mut units = UnitTable::default();
        place(&mut w, &mut units, row(0, 1, 7, 10, 1), [guy(7, 10, 1)]);
        place(&mut w, &mut units, row(1, 2, 12, 10, 1), [guy(12, 10, 1)]);
        let me = units.row(0, 1).unwrap();
        let mut cc = CollCheck::new();
        let d = detect_unit_collision(
            &mut w,
            &mut cc,
            &mut units,
            &me,
            ucell_centre(10),
            ucell_centre(10),
            DetectArgs::VALID_UCOORD,
        );
        assert_eq!(d, Detect::HitProbe);
        assert_eq!(units.row(0, 1).unwrap().collide_o, -1);
    }

    #[test]
    fn path_top_flag_eight_and_safe_byte_suppress_collision() {
        let mut w = world(3, 3);
        let mut units = UnitTable::default();
        place(&mut w, &mut units, row(0, 1, 7, 10, 1), [guy(7, 10, 1)]);
        place(&mut w, &mut units, row(1, 2, 12, 10, 1), [guy(12, 10, 1)]);
        let mut cc = CollCheck::new();
        for me in [
            UnitRow {
                path_top_flags: 8,
                ..units.row(0, 1).unwrap()
            },
            UnitRow {
                safe: 1,
                ..units.row(0, 1).unwrap()
            },
        ] {
            assert_eq!(
                detect_unit_collision(
                    &mut w,
                    &mut cc,
                    &mut units,
                    &me,
                    ucell_centre(10),
                    ucell_centre(10),
                    DetectArgs::VALID_UCOORD,
                ),
                Detect::Clear
            );
        }
    }

    #[test]
    fn boat_solver_gate_matches_domain_siege_hero_supply_and_overlay_rules() {
        let mut w = world(2, 2);
        let mut cc = CollCheck::new();
        for me in [
            UnitRow {
                domain: DOMAIN_WATER,
                push_size: 96,
                push_circles: 1,
                ..row(0, 1, 4, 4, 1)
            },
            UnitRow {
                unit_flags: 0x0002_0000,
                push_size: 96,
                push_circles: 1,
                ..row(0, 1, 4, 4, 1)
            },
            UnitRow {
                unit_flags2: 0x20,
                push_size: 96,
                push_circles: 1,
                ..row(0, 1, 4, 4, 1)
            },
            UnitRow {
                unit_flags2: 0x40,
                push_size: 96,
                push_circles: 1,
                ..row(0, 1, 4, 4, 1)
            },
        ] {
            let mut units = UnitTable::default();
            assert_eq!(
                detect_unit_collision(
                    &mut w,
                    &mut cc,
                    &mut units,
                    &me,
                    ucell_centre(5),
                    ucell_centre(4),
                    DetectArgs::MOVE_STEP,
                ),
                Detect::ClearAndReset { yielded: false }
            );
            assert_eq!(units.boat_queries.len(), 1);
            assert_eq!(units.boat_queries[0].exclude_o, me.o);
            assert_eq!(units.boat_queries[0].exclude_who, me.who);
        }

        let me = row(0, 1, 4, 4, 1);
        let mut units = UnitTable::default();
        let _ = detect_unit_collision(
            &mut w,
            &mut cc,
            &mut units,
            &me,
            ucell_centre(5),
            ucell_centre(4),
            DetectArgs::MOVE_STEP,
        );
        assert!(
            units.boat_queries.is_empty(),
            "ordinary land units bypass the boat solver"
        );

        let water = UnitRow {
            domain: DOMAIN_WATER,
            ..me
        };
        let _ = detect_unit_collision(
            &mut w,
            &mut cc,
            &mut units,
            &water,
            ucell_centre(5),
            ucell_centre(4),
            DetectArgs::VALID_UCOORD,
        );
        assert!(
            units.boat_queries.is_empty(),
            "overlay probes bypass the boat solver"
        );
    }

    #[test]
    fn boat_hulls_are_tangent_circles_along_the_first_guy_heading() {
        assert_eq!(boat_project(1000, 1000, 0, 96), (1000, 904));
        assert_eq!(boat_project(1000, 1000, 0x4000_0000, 96), (1096, 1000));
        let (radius, centres) = boat_hull_centres(1000, 1000, 0, 192, 2);
        assert_eq!(radius, 96);
        assert_eq!(centres, [(1000, 1096), (1000, 904)]);
        let (radius, centres) = boat_hull_centres(1000, 1000, 0, 288, 3);
        assert_eq!(radius, 96);
        assert_eq!(centres, [(1000, 1192), (1000, 1000), (1000, 808)]);
    }

    #[test]
    fn boat_solver_pushes_one_circle_candidate_and_writes_partner_turn_and_frame() {
        let me = boat_row(0, 7, 1000, 1000);
        let other = boat_row(0, 8, 1050, 1000);
        let mut units = UnitTable {
            rows: vec![other],
            frame: 77,
            ..UnitTable::default()
        };
        assert!(detect_boat_collision(&mut units, &me, 1000, 1000, true));
        assert_eq!(
            units.boat_queries,
            [BoatQuery {
                x: 1000,
                y: 1000,
                radius: 96,
                exclude_o: 7,
                exclude_who: 0,
                circles: 1,
                water: true,
            }]
        );
        assert_eq!(
            units.boat_moves,
            [BoatMove {
                who: 0,
                o: 8,
                x: 1098,
                y: 1000
            }]
        );
        let pushed = units.row(0, 8).unwrap();
        assert_eq!((pushed.x, pushed.y), (1098, 1000));
        assert_eq!((pushed.collide_who, pushed.collide_o), (0, 7));
        assert_eq!(pushed.collide_frame, 77);
        assert_eq!(units.boat_faces.len(), 1);
        assert_eq!(units.boat_faces[0].angle, 0x4000_0000);
        assert_eq!(
            (units.boat_faces[0].pusher_who, units.boat_faces[0].pusher_o),
            (0, 7)
        );
    }

    #[test]
    fn stationary_boat_push_angle_is_clamped_forty_five_degrees_from_pusher_heading() {
        let me = boat_row(0, 7, 1000, 1000);
        let other = boat_row(0, 8, 1000, 950);
        let mut units = UnitTable {
            rows: vec![other],
            ..UnitTable::default()
        };
        assert!(detect_boat_collision(&mut units, &me, 1000, 1000, true));
        assert_eq!(units.boat_faces[0].angle, 0x2000_0000);
        let expected = boat_project(1000, 950, 0x2000_0000, 0x30);
        assert_eq!(
            (units.row(0, 8).unwrap().x, units.row(0, 8).unwrap().y),
            expected
        );
    }

    #[test]
    fn moving_heading_rejection_and_attack_gate_return_false_without_frame_write() {
        let me = boat_row(0, 7, 1000, 1000);
        let moving = UnitRow {
            moving: true,
            ..boat_row(0, 8, 1000, 950)
        };
        let mut units = UnitTable {
            rows: vec![moving],
            frame: 91,
            ..UnitTable::default()
        };
        assert!(!detect_boat_collision(&mut units, &me, 1000, 1000, true));
        assert_eq!(units.row(0, 8).unwrap().collide_frame, 0);
        assert!(units.boat_moves.is_empty());

        let armed_me = UnitRow {
            unit_flags: 0x10,
            ..me
        };
        let armed = UnitRow {
            attack_value: 1,
            ..boat_row(0, 9, 1050, 1000)
        };
        let mut units = UnitTable {
            rows: vec![armed],
            frame: 92,
            ..UnitTable::default()
        };
        assert!(!detect_boat_collision(
            &mut units, &armed_me, 1000, 1000, true
        ));
        assert_eq!(units.row(0, 9).unwrap().collide_frame, 0);
    }

    #[test]
    fn invalid_or_filtered_boat_candidate_skips_the_post_move_frame_write() {
        let me = boat_row(0, 7, 1000, 1000);
        let other = boat_row(0, 8, 1050, 1000);
        let expected = boat_project(1050, 1000, 0x4000_0000, 0x30);
        let mut units = UnitTable {
            rows: vec![other],
            invalid_boat_locations: vec![(0, 8, tile_of(expected.0), tile_of(expected.1))],
            frame: 101,
            ..UnitTable::default()
        };
        assert!(detect_boat_collision(&mut units, &me, 1000, 1000, true));
        assert!(units.boat_moves.is_empty());
        assert_eq!(
            (units.row(0, 8).unwrap().x, units.row(0, 8).unwrap().y),
            (1050, 1000)
        );
        assert_eq!(units.row(0, 8).unwrap().collide_frame, 0);

        let filtered = UnitRow {
            domain: DOMAIN_LAND,
            ..other
        };
        let mut units = UnitTable {
            rows: vec![filtered],
            frame: 102,
            ..UnitTable::default()
        };
        assert!(detect_boat_collision(&mut units, &me, 1000, 1000, true));
        assert_eq!(units.row(0, 8).unwrap().collide_frame, 0);
    }

    #[test]
    fn mutual_diplomacy_two_allows_cross_owner_push() {
        let me = boat_row(0, 7, 1000, 1000);
        let other = boat_row(1, 8, 1050, 1000);
        let mut units = UnitTable {
            rows: vec![other],
            ..UnitTable::default()
        };
        assert!(!detect_boat_collision(&mut units, &me, 1000, 1000, true));
        units.rows[0] = other;
        units.diplos[0][1] = 2;
        units.diplos[1][0] = 2;
        assert!(detect_boat_collision(&mut units, &me, 1000, 1000, true));
        assert_eq!(units.boat_moves.last().unwrap().who, 1);
    }

    #[test]
    fn land_pusher_respects_packed_unpacking_special_mask_and_tank_vfunc_gates() {
        let me = UnitRow {
            domain: DOMAIN_LAND,
            ..boat_row(0, 7, 1000, 1000)
        };
        let base = UnitRow {
            domain: DOMAIN_LAND,
            ..boat_row(0, 8, 1050, 1000)
        };
        let refused = [
            UnitRow {
                unit_flags2: 4,
                ..base
            },
            UnitRow {
                unit_flags2: 4,
                unit_masks: 0x0008_0000,
                unpacking: true,
                ..base
            },
            UnitRow {
                unit_masks: 0x0200_0000,
                ..base
            },
            UnitRow {
                unit_flags: 0x0008_0000,
                ..base
            },
        ];
        for candidate in refused {
            let mut units = UnitTable {
                rows: vec![candidate],
                frame: 33,
                ..UnitTable::default()
            };
            assert!(!detect_boat_collision(&mut units, &me, 1000, 1000, true));
            assert!(units.boat_moves.is_empty());
            assert_eq!(units.row(0, 8).unwrap().collide_frame, 0);
        }

        let packed = UnitRow {
            unit_flags2: 4,
            unit_masks: 0x0008_0000,
            ..base
        };
        let mut units = UnitTable {
            rows: vec![packed],
            ..UnitTable::default()
        };
        assert!(detect_boat_collision(&mut units, &me, 1000, 1000, true));
        assert_eq!(units.boat_moves.len(), 1);
    }

    #[test]
    fn same_group_skip_and_move_other_false_preserve_retail_side_effect_split() {
        let me = UnitRow {
            group: 4,
            ..boat_row(0, 7, 1000, 1000)
        };
        let mate = UnitRow {
            group: 4,
            ..boat_row(0, 8, 1050, 1000)
        };
        let mut units = UnitTable {
            rows: vec![mate],
            frame: 55,
            ..UnitTable::default()
        };
        assert!(detect_boat_collision(&mut units, &me, 1000, 1000, true));
        assert!(units.boat_moves.is_empty());
        assert_eq!(units.row(0, 8).unwrap().collide_frame, 0);

        let me = UnitRow { group: -1, ..me };
        units.rows[0] = UnitRow {
            group: -1,
            collide_frame: 0,
            ..mate
        };
        assert!(detect_boat_collision(&mut units, &me, 1000, 1000, false));
        assert_eq!(
            units.boat_moves.len(),
            1,
            "param_3 does not gate relocation"
        );
        let pushed = units.row(0, 8).unwrap();
        assert_eq!((pushed.collide_who, pushed.collide_o), (-1, -1));
        assert!(units.boat_faces.is_empty());
    }

    #[test]
    fn move_unit_repaints_only_the_changed_edges() {
        let mut w = world(2, 2);
        w.new_coll_block(0, 0);
        for x in 2..=4 {
            for y in 2..=4 {
                block_set(w.wdata[0].block.as_deref_mut().unwrap(), x, y, true);
            }
        }
        move_unit(&mut w, (3, 3), (5, 3), 1);
        let b = w.wdata(0, 0).block.as_deref().unwrap();
        for x in 4..=6 {
            for y in 2..=4 {
                assert!(block_get(b, x, y));
            }
        }
        for y in 2..=4 {
            assert!(!block_get(b, 2, y));
        }
    }

    #[test]
    fn collision_block_reaper_honours_persistent_cursor_and_budget() {
        let mut w = world(8, 2);
        for x in 0..5 {
            w.new_coll_block(x, 0);
        }
        block_set(w.wdata[0].block.as_deref_mut().unwrap(), 0, 0, true);
        let mut cursor = 0;
        assert_eq!(process_coll_blocks(&mut w, &mut cursor), 4);
        assert_eq!(cursor, 5);
        assert!(w.wdata[0].block.is_some());
        assert!(w.wdata[1..5].iter().all(|r| r.block.is_none()));
    }

    #[test]
    fn turn_rate_and_heading_edges_are_integer_exact() {
        let guy = TurnGuy {
            guy_num: 0,
            avg_speed: 0,
            ..TurnGuy::default()
        };
        assert_eq!(turn_speed(&guy, 0x0100_0000, 1, 0, true), 0x0100_0000);
        assert_eq!(turn_speed(&guy, 0x0100_0000, 1, 0, false), 0x0100_0000);
        assert_eq!(turn_toward(0x0222_221f, 0, 1), (0x0222_221f, 0));
        assert_eq!(
            turn_toward(0x1000_0000, 0, 0x0100_0000),
            (0x0100_0000, 0x0f00_0000)
        );
    }

    #[test]
    fn retail_constants_have_no_accidental_duplicate_keys() {
        let constants = BTreeMap::from([
            ("unit_turn_speed", UNIT_TURN_SPEED),
            ("unit_pack_turn_bonus", UNIT_PACK_TURN_BONUS),
            ("turn_floor_mul", TURN_SPEED_FLOOR_MUL),
            ("ucell", UCELL),
            ("block_ucells", BLOCK_UCELLS),
        ]);
        assert_eq!(constants.len(), 5);
        assert_eq!(constants["unit_turn_speed"], 256);
        assert_eq!(constants["unit_pack_turn_bonus"], 2);
    }
}
