//! The map: coordinate ladder, tile grid, terrain, fog, and the `world` checksum channel.
//!
//! Lane: `map-terrain`. Checksum channel served: **`world`** (channel 12 of
//! `CheckSums::check_all` `0x00936560`, packet offset `+0x2d`).
//!
//! Everything in this module is derived from `ron-bin/riseofnations.exe`
//! (sha256 `30478a44…625079`) and its shipped PDB `ron-bin/sbl/rise.pdb`. Provenance for
//! every constant is in the comment next to it and in `docs/mechanics/map-terrain.md`.
//! Nothing here comes from community documentation.
//!
//! # The one structural fact
//!
//! The engine's world is **two grids plus three derived grids**, all integer:
//!
//! ```text
//!   Coord   1 world unit          — the position unit; Guy/Unit coordinates live here
//!   UCoord  48 Coord   = 1/4 tile — the pathfinder's 8-connected movement grid
//!   TCoord  192 Coord  = 1 tile   — TData lives here (terrain bits)
//!   FCoord  384 Coord  = 2 tiles  — the fog planes live here
//!   WCoord  768 Coord  = 4 tiles  — WData lives here (land, region, owner, collision)
//!   RCoord  1536 Coord = 8 tiles  — the danger maps live here
//! ```
//!
//! `Coord`, `WCoord` and `TCoord` are each a `class` wrapping one `int` (PDB: size 4, one
//! field `value`). The conversions are *not* plain shifts — the engine indexes a runtime
//! table `div_3_table` (`0x00cae5fc`, built by `init_coord_lookup_array` `0x00681db0`,
//! `t[i] == floor(i/3)` for both signs) with a power-of-two right shift of the `Coord`:
//!
//! ```text
//!   UCoord  = div_3_table[c >> 4]   ==  floor(c / 48)     [measured, 122 call sites]
//!   TCoord  = div_3_table[c >> 6]   ==  floor(c / 192)    [measured, Coord::operator TCoord 0x0046cd40]
//!   FCoord  = div_3_table[c >> 7]   ==  floor(c / 384)    [measured, 83 call sites]
//!   WCoord  = div_3_table[c >> 8]   ==  floor(c / 768)    [measured, Coord::operator WCoord 0x00461460]
//!   RCoord  = div_3_table[c >> 9]   ==  floor(c / 1536)   [measured, 10 call sites]
//! ```
//!
//! This is where the pathfinder's otherwise-mysterious `48 / 192 / 768` parameterisation
//! (`docs/derivation/architecture.md` §8) comes from: they are the U, T and W cell sizes,
//! matching `find_upath` / `find_tpath` / `find_wpath` name-for-name.
//!
//! # What is *not* here
//!
//! There is **no sim-side heightmap**. `Terrain` (`MiscAccess::terrain`, 27,336 bytes) is
//! `TerrainOut` + `GameAccess` and holds `master_land_heights : SimpleArray<float>` —
//! render geometry. Height never enters the `world` checksum. The sim's only vertical
//! structure is the discrete `CLIFF` / `MOUNTAIN` bits in [`TData`].
//!
//! Four `Terrain` fields *are* inside the `world` channel, though — see
//! [`TerrainSync`]. They are sections 10–13 of `World::walk_data`.

#![allow(clippy::too_many_arguments)]

// ---------------------------------------------------------------------------------------
// 1. The coordinate ladder
// ---------------------------------------------------------------------------------------

/// Coord units per quarter-tile (`UCoord`). [measured: `div_3_table[c >> 4]`]
pub const COORD_PER_UCELL: i32 = 48;
/// Coord units per tile (`TCoord`). [measured: `Coord::operator TCoord` `0x0046cd40`,
/// cross-checked by `WorldData::is_valid(Coord,Coord)` `0x0043f360` which bounds a `Coord`
/// by `tile_xs * 0xc0` — and `0xc0 == 192`.]
pub const COORD_PER_TILE: i32 = 192;
/// Coord units per fog cell (`FCoord`). [measured: `div_3_table[c >> 7]`]
pub const COORD_PER_FCELL: i32 = 384;
/// Coord units per world cell (`WCoord`). [measured: `Coord::operator WCoord` `0x00461460`,
/// cross-checked by `World::set_oil_at` `0x006b2a10` which places the oil `Good` at
/// `(wx * 0x300 + 0x180, wy * 0x300 + 0x180)` — cell size `0x300 == 768`, centre `0x180`.]
pub const COORD_PER_WCELL: i32 = 768;
/// Coord units per region cell (`RCoord`). [measured: `div_3_table[c >> 9]`]
pub const COORD_PER_RCELL: i32 = 1536;

/// Tiles per world cell, per axis. [measured: `World::init` sets `tile_xs = xs * 4`;
/// `WCoord::operator TCoord` `0x004613b0` is `t = w*4 + 2` (cell centre) and
/// `WCoord::get_tcorner` `0x0046f080` is `t = w*4` (top-left corner).]
pub const TILES_PER_WCELL: i32 = 4;
/// Tiles inside one world cell. [measured: `WCoord::traverse` `0x0042a610` returns 16.]
pub const TILES_IN_WCELL: i32 = TILES_PER_WCELL * TILES_PER_WCELL;
/// Fog cells per world cell, per axis. [measured: `World::init` sets `fog_xs = xs * 2`.]
pub const FCELLS_PER_WCELL: i32 = 2;
/// World cells per region cell, per axis. [measured: `World::init` sets `reg_xs = xs / 2`.]
pub const WCELLS_PER_RCELL: i32 = 2;

/// Floor division, matching `div_3_table[i] == floor(i/3)` for negative `i` too.
#[inline]
pub const fn floor_div(a: i32, b: i32) -> i32 {
    let q = a / b;
    if (a % b != 0) && ((a < 0) != (b < 0)) {
        q - 1
    } else {
        q
    }
}

/// `div_3_table[i]`. [measured: `init_coord_lookup_array` `0x00681db0` fills
/// `t[i] = i/3` for `i >= 0` and `t[j] = (j-2)/3` (C truncation) for `j < 0`, which is
/// `floor(j/3)` in both halves.]
#[inline]
pub const fn div_3(i: i32) -> i32 {
    floor_div(i, 3)
}

macro_rules! coord_kind {
    ($name:ident, $shift:expr, $scale:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Default, Hash)]
        #[repr(transparent)]
        pub struct $name(pub i32);

        impl $name {
            /// Coord units spanned by one cell of this grid.
            pub const SCALE: i32 = $scale;

            /// Convert a raw `Coord` exactly the way the engine does: arithmetic shift,
            /// then `div_3_table`.
            #[inline]
            pub const fn from_coord(c: Coord) -> Self {
                Self(div_3(c.0 >> $shift))
            }

            /// Coord of this cell's low corner.
            #[inline]
            pub const fn corner(self) -> Coord {
                Coord(self.0 * $scale)
            }

            /// Coord of this cell's centre.
            #[inline]
            pub const fn centre(self) -> Coord {
                Coord(self.0 * $scale + $scale / 2)
            }
        }
    };
}

/// A raw world unit. `class Coord` in the PDB — size 4, one `int value`.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Default, Hash)]
#[repr(transparent)]
pub struct Coord(pub i32);

coord_kind!(
    UCoord,
    4,
    COORD_PER_UCELL,
    "Quarter-tile cell — the pathfinder's 8-connected movement grid (`find_upath`)."
);
coord_kind!(
    TCoord,
    6,
    COORD_PER_TILE,
    "Tile cell — the [`TData`] grid (`find_tpath`)."
);
coord_kind!(
    FCoord,
    7,
    COORD_PER_FCELL,
    "Fog cell — the `seen`/`seen2`/`seen3` byte planes."
);
coord_kind!(
    WCoord,
    8,
    COORD_PER_WCELL,
    "World cell — the [`WData`] grid (`find_wpath`)."
);
coord_kind!(
    RCoord,
    9,
    COORD_PER_RCELL,
    "Region cell — the per-player `danger` maps."
);

impl WCoord {
    /// `WCoord::operator TCoord` `0x004613b0`: `t = w*4 + 2` — the *centre* tile, not the
    /// corner. Getting this wrong shifts every world→tile conversion by half a world cell.
    #[inline]
    pub const fn to_tcoord_centre(self) -> TCoord {
        TCoord(self.0 * TILES_PER_WCELL + 2)
    }
    /// `WCoord::get_tcorner` `0x0046f080`: `t = w*4`.
    #[inline]
    pub const fn tcorner(self) -> TCoord {
        TCoord(self.0 * TILES_PER_WCELL)
    }
    /// `WCoord::traverse_x(i)` `0x0046eef0`: `t = w*4 + (i mod 4)`, sign-correct mod.
    #[inline]
    pub const fn traverse_x(self, i: i32) -> TCoord {
        TCoord(self.0 * TILES_PER_WCELL + i.rem_euclid(TILES_PER_WCELL))
    }
    /// `WCoord::traverse_y(i)` `0x0046eed0`: `t = w*4 + floor(i/4)`.
    #[inline]
    pub const fn traverse_y(self, i: i32) -> TCoord {
        TCoord(self.0 * TILES_PER_WCELL + floor_div(i, TILES_PER_WCELL))
    }
}

impl TCoord {
    /// `TCoord::operator WCoord` `0x0046fab0`: `w = t >> 2` (arithmetic).
    #[inline]
    pub const fn to_wcoord(self) -> WCoord {
        WCoord(self.0 >> 2)
    }
}

/// The 8-connected neighbour ring, in the engine's own order: NW, N, NE, E, SE, S, SW, W.
/// [measured: the two `int[8]` tables at `0x00adcaf4` (dx) and `0x00adc404` (dy), read by
/// `World::set_blocked_at` `0x006b4900` and `WorldData::has_blocked_neighbors` `0x006b2990`.]
pub const NEIGHBOUR_DX: [i32; 8] = [-1, 0, 1, 1, 1, 0, -1, -1];
/// See [`NEIGHBOUR_DX`].
pub const NEIGHBOUR_DY: [i32; 8] = [-1, -1, -1, 0, 1, 1, 1, 0];

/// The 4-connected (von Neumann) ring, in the engine's order: N, E, S, W.
/// [measured: `int[4]` tables at `0x00add254` (dx) and `0x00add214` (dy), read by
/// `WorldData::has_gather_access` `0x006b4e50`.]
pub const NEIGHBOUR4_DX: [i32; 4] = [0, 1, 0, -1];
/// See [`NEIGHBOUR4_DX`].
pub const NEIGHBOUR4_DY: [i32; 4] = [-1, 0, 1, 0];

/// The 16 tile offsets `WorldData::space_at_corner` probes, in the engine's own probe
/// order. [measured: `int[16]` tables at `0x00adecf0` (dx) and `0x00aded30` (dy).]
///
/// Laid out on the 4x4 block they cover, the probe *indices* are:
///
/// ```text
///          dx=0  dx=1  dx=2  dx=3
///   dy=0 :   4     5     6     7
///   dy=1 :  12     0     1    13
///   dy=2 :  14     2     3    15
///   dy=3 :   8     9    10    11
/// ```
///
/// so indices **0–3 are the inner 2x2 core** — and the engine rejects outright
/// (`return 0`) the moment any of those four is blocked, before even scoring the ring.
pub const SPACE_PROBE_DX: [i32; 16] = [1, 2, 1, 2, 0, 1, 2, 3, 0, 1, 2, 3, 0, 3, 0, 3];
/// See [`SPACE_PROBE_DX`].
pub const SPACE_PROBE_DY: [i32; 16] = [1, 1, 2, 2, 0, 0, 0, 0, 3, 3, 3, 3, 1, 1, 2, 2];

/// The four 5-cell "approach L" groups `space_at_corner` tests for a clear side.
/// [measured: the 20 ints at `0x00adeca0`, walked in groups of 5 up to `0x00adecf0`.]
/// In probe-index terms these are the bottom-right, bottom-left, top-left and top-right
/// L-shaped approach corridors around the 2x2 core.
pub const SPACE_APPROACH_GROUPS: [[usize; 5]; 4] = [
    [9, 10, 11, 13, 15],
    [12, 14, 8, 9, 10],
    [14, 12, 4, 5, 6],
    [5, 6, 7, 13, 15],
];

/// Grades returned by [`World::space_at_corner`] / [`World::check_building_wcoord`].
/// Higher is more buildable; production's `BuildTypeData::blocked_location`
/// (`0x006375b0`) consumes these.
pub mod space {
    /// The 2x2 core is blocked, out of bounds, or owned by someone else — unusable.
    pub const CORE_BLOCKED: i32 = 0;
    /// Usable, but no full approach corridor is clear.
    pub const PARTIAL: i32 = 2;
    /// At least one of the four 5-cell approach L's is entirely clear.
    pub const APPROACH_CLEAR: i32 = 3;
    /// All 16 probed tiles are clear.
    pub const FULLY_CLEAR: i32 = 4;
}

// ---------------------------------------------------------------------------------------
// 2. TData — 2 bytes per tile
// ---------------------------------------------------------------------------------------

/// Bits of `TData::mask` (`unsigned short`, the whole of `TData`, PDB size 2).
///
/// Bits 0–1 are a 2-bit **blocker kind** and bits 4–5 a 2-bit **surface kind**; the rest
/// are independent flags. Every value below was read off the setter that writes it.
pub mod tflag {
    /// Blocker-kind field, bits 0–1.
    pub const BLOCKER_MASK: u16 = 0x0003;
    /// `World::set_cliff_at` `0x006b1eb0` — `(m & ~2) | 1`.
    /// `WorldData::is_cliff_at` `0x0046f8c0` — `(m & 3) == 1`.
    pub const BLOCKER_CLIFF: u16 = 1;
    /// `World::set_mountain_at` `0x006b1f00`; `WorldData::is_mountain_at` `0x0046f900`.
    pub const BLOCKER_MOUNTAIN: u16 = 2;
    /// `World::set_building_at` `0x006b45c0` — `m |= 3`.
    pub const BLOCKER_BUILDING: u16 = 3;

    /// `World::set_behind(.., behind, 0)` `0x006b4230`.
    pub const BEHIND_A: u16 = 0x0004;
    /// `World::set_behind(.., behind, 1)`; also set by `World::set_tree_at`.
    pub const BEHIND_B: u16 = 0x0008;

    /// Surface-kind field, bits 4–5.
    pub const SURFACE_MASK: u16 = 0x0030;
    /// `World::set_road_at` `0x006b43b0` — `(m & ~0x20) | 0x10`.
    pub const SURFACE_ROAD: u16 = 0x0010;
    /// `World::set_tocean` `0x006b1c60` / `set_waterhalf` `0x006b1c90`;
    /// `WorldData::is_tocean` `0x0046fb10` — `(m & 0x30) == 0x20`.
    pub const SURFACE_WATER: u16 = 0x0020;
    /// `World::set_tree_at` `0x006b2060` — `m |= 0x38`;
    /// `WorldData::is_tree_at` `0x0046f930` — `(m & 0x30) == 0x30`.
    pub const SURFACE_TREES: u16 = 0x0030;

    /// `World::set_started2_at` `0x006b4530`.
    pub const STARTED2: u16 = 0x0040;
    /// `World::set_started_at` `0x006b4570`; part of `WorldData::is_built_at` `0x0046f880`.
    pub const STARTED: u16 = 0x0080;
    /// `World::set_city_at` `0x006b4180`.
    pub const CITY: u16 = 0x0100;
    /// `World::set_resource_at` `0x006b3a80`.
    pub const RESOURCE: u16 = 0x0200;
    /// `World::set_coastal` `0x006b1d10`.
    pub const COASTAL: u16 = 0x0400;
    /// `World::set_river_at` `0x006b1f80`; `WorldData::is_river` `0x0046d390`.
    pub const RIVER: u16 = 0x0800;
    /// `World::set_gathered_at` `0x006b46b0`; `WorldData::is_gathered_from` `0x00472ac0`.
    pub const GATHERED: u16 = 0x1000;
    /// `World::set_bad_path` `0x006b4610` — maintained as a *count* in `WData::bad`.
    pub const BAD_PATH: u16 = 0x2000;
    /// `World::set_blocked_at` `0x006b4900`; `WorldData::is_blocked_at` `0x00461340`.
    pub const BLOCKED: u16 = 0x4000;
    /// `World::set_gather_edge` `0x006b2110`.
    pub const GATHER_EDGE: u16 = 0x8000;
}

// ---------------------------------------------------------------------------------------
// 3. WData — 28 bytes per world cell, 21 of them checksummed
// ---------------------------------------------------------------------------------------

/// Bits of `WData::flags` (`unsigned short`).
pub mod wflag {
    /// `WorldData::is_dead_build` `0x006b2390`.
    pub const DEAD_BUILD: u16 = 0x0002;
    /// `WorldData::is_coast` `0x006b3020`. Part of the land-class field below.
    pub const COAST: u16 = 0x0004;
    /// `WorldData::is_rocks` `0x006b4380`.
    pub const ROCKS: u16 = 0x0008;
    /// `WorldData::is_mountains` `0x006b5590`.
    pub const MOUNTAINS: u16 = 0x0010;
    /// `WorldData::is_forest` `0x006b5560`.
    pub const FOREST: u16 = 0x0020;
    /// Impassable, groups with `MOUNTAINS` in `get_land`'s `flags & 0x50` test and with
    /// the passability tests' `flags & 0x70`. **Writer not identified** — treat as
    /// measured-in-readers, unattributed-in-writers.
    pub const IMPASSABLE_X: u16 = 0x0040;
    /// Set by `World::set_road_at` on the owning W cell; cleared when no tile in the cell
    /// still carries [`tflag::SURFACE_ROAD`].
    pub const HAS_ROAD: u16 = 0x0080;
    /// `World::set_waterhalf` `0x006b1c90` / `clear_waterhalf` `0x006b1bd0`. A W cell that
    /// is part land, part water: `WorldData::get_tregion` reads `region2` for its water
    /// tiles and `region` for the rest.
    pub const WATERHALF: u16 = 0x0100;
    /// Set by `World::set_land` when the land class is `COAST`; cleared by
    /// `World::set_orig_coast` `0x006b1b80`.
    pub const ORIG_COAST: u16 = 0x0400;
    /// `World::set_oil_at` `0x006b2a10`; `WorldData::is_oil_at` `0x00472af0`.
    pub const OIL: u16 = 0x0800;
    /// Set by `World::set_river_at` on the owning W cell; cleared when no tile in the cell
    /// still carries [`tflag::RIVER`].
    pub const HAS_RIVER: u16 = 0x1000;

    /// The land-class field `World::set_land` clears before OR-ing its argument in
    /// (`flags &= 0xffc3`). One of `0`, `COAST`, `ROCKS`, `MOUNTAINS`, `FOREST`.
    pub const LAND_CLASS_MASK: u16 = 0x003c;
    /// `WorldData::is_passable` `0x006b23c0` — `(flags & 0x70) == 0`.
    pub const IMPASSABLE_MASK: u16 = 0x0070;
    /// `WorldData::buildings_allowed` `0x006b2340` — `(flags & 0x78) == 0`.
    pub const NO_BUILD_MASK: u16 = 0x0078;
}

/// `WData::land` values. `WorldData::is_ocean` `0x006b4830` treats **1 and 2** as water
/// (when [`wflag::WATERHALF`] is clear). `WorldData::offmap_world` (static `WData` at
/// `0x00c899a0`) has `land == 2`, so off-map reads as deep water.
pub mod land {
    /// Dry land. Matches `TileSetLandTypes::eTILE_FERTILE` (0) in the PDB enum, which is a
    /// tileset enum and only *cross-checks* this — do not treat the mapping as proven.
    pub const FERTILE: i8 = 0;
    /// Shallow / coastal water — counted as ocean by `is_ocean`.
    pub const COASTAL: i8 = 1;
    /// Deep water; the value `World::wipe` and `offmap_world` use.
    pub const OCEAN: i8 = 2;
}

/// One world cell. Field names and offsets are the PDB's, verbatim.
///
/// The engine struct is 28 bytes; **21 of them are checksummed** —
/// `World::walk_data` section 5 walks `[wdata + 28*i, +0x15)`, i.e. `flags` through
/// `was_seen`, stopping before the 3 padding bytes and the `CollBlock*`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WData {
    /// `+0x00` — see [`wflag`].
    pub flags: u16,
    /// `+0x02` — see [`land`].
    pub land: i8,
    /// `+0x03`
    pub land_sub: u8,
    /// `+0x04` — land region id.
    pub region: i16,
    /// `+0x06` — water region id (used for the water tiles of a [`wflag::WATERHALF`] cell).
    pub region2: i16,
    /// `+0x08`
    pub down: i16,
    /// `+0x0a`
    pub down_who: i16,
    /// `+0x0c`
    pub val: u8,
    /// `+0x0d` — `WorldData::get_goods` `0x0046ef10`.
    pub goods: u8,
    /// `+0x0e`
    pub light: u8,
    /// `+0x0f` — territory owner. `WorldData::get_who` / `get_whose`. `-1` = unowned.
    pub who: i8,
    /// `+0x10`
    pub who2: i8,
    /// `+0x11` — count of tiles in this cell carrying [`tflag::BLOCKED`].
    pub blocked: u8,
    /// `+0x12` — count of tiles in this cell carrying [`tflag::BAD_PATH`].
    pub bad: u8,
    /// `+0x13` — maintained by `set_blocked_at` (+1) and `set_tree_at` (−1); the *sign
    /// convention is as measured*, and it is not a plain blocker count.
    pub solid: i8,
    /// `+0x14`
    pub was_seen: u8,
    // +0x15..+0x17 padding — NOT checksummed.
    /// `+0x18` — lazily allocated `CollBlock`, `BitMask<768>`.
    pub block: Option<Box<CollBlock>>,
}

impl Default for WData {
    /// The exact state `World::wipe` `0x006b2c00` writes into every cell.
    /// [measured at instruction level, `0x006b2ce2`–`0x006b2d03`:
    /// `mov dword [edx+2], 0x400002` (land=2, land_sub=0, region=64),
    /// `mov word [edx], 0`, `mov dword [edx+0xa], 0`, `mov word [edx+6], 0`,
    /// `mov dword [edx+0xf], 0xffff` (who=-1, who2=-1, blocked=0, bad=0),
    /// `mov word [edx+0x13], 0`, `mov byte [edx+0xe], 0`, `mov word [edx+8], -1`.]
    fn default() -> Self {
        WData {
            flags: 0,
            land: land::OCEAN,
            land_sub: 0,
            region: 64,
            region2: 0,
            down: -1,
            down_who: 0,
            val: 0,
            goods: 0,
            light: 0,
            who: -1,
            who2: -1,
            blocked: 0,
            bad: 0,
            solid: 0,
            was_seen: 0,
            block: None,
        }
    }
}

impl WData {
    /// `WorldData::offmap_world`, the static `WData` at `0x00c899a0` returned for
    /// out-of-bounds reads. [measured: the 28 bytes are
    /// `00 00 02 00 00 00 00 00 ff ff 00 00 00 00 00 ff ff 00 00 00 00 …`.]
    pub fn offmap() -> Self {
        WData {
            flags: 0,
            land: land::OCEAN,
            land_sub: 0,
            region: 0,
            region2: 0,
            down: -1,
            down_who: 0,
            val: 0,
            goods: 0,
            light: 0,
            who: -1,
            who2: -1,
            blocked: 0,
            bad: 0,
            solid: 0,
            was_seen: 0,
            block: None,
        }
    }

    /// The 21 checksummed bytes, in memory order.
    pub fn checksum_bytes(&self) -> [u8; 21] {
        let mut b = [0u8; 21];
        b[0..2].copy_from_slice(&self.flags.to_le_bytes());
        b[2] = self.land as u8;
        b[3] = self.land_sub;
        b[4..6].copy_from_slice(&self.region.to_le_bytes());
        b[6..8].copy_from_slice(&self.region2.to_le_bytes());
        b[8..10].copy_from_slice(&self.down.to_le_bytes());
        b[10..12].copy_from_slice(&self.down_who.to_le_bytes());
        b[12] = self.val;
        b[13] = self.goods;
        b[14] = self.light;
        b[15] = self.who as u8;
        b[16] = self.who2 as u8;
        b[17] = self.blocked;
        b[18] = self.bad;
        b[19] = self.solid as u8;
        b[20] = self.was_seen;
        b
    }
}

/// `CollBlock : BitMask<768>` — the per-world-cell collision bitmap.
/// [measured: `World::new_coll_block` `0x0046d250` writes `bits = 0x300` (768),
/// `size = 0x60` (96 bytes), zeroes the 96 payload bytes, then `flags = 1`.
/// On load, `World::walk_data` sets `flags = 2` instead.]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct CollBlock {
    /// `+0x00`, always `768` at construction.
    pub bits: i32,
    /// `+0x04`, always `96` at construction.
    pub size: i32,
    /// `+0x08` — **not checksummed**; the walk skips `[+8, +0xc)`.
    pub flags: i32,
    /// `+0x0c`, `size` bytes.
    pub ptr: [u8; 96],
}

impl Default for CollBlock {
    fn default() -> Self {
        CollBlock {
            bits: 0x300,
            size: 0x60,
            flags: 1,
            ptr: [0u8; 96],
        }
    }
}

// ---------------------------------------------------------------------------------------
// 4. Checksummed dynamic arrays
// ---------------------------------------------------------------------------------------

/// A `SimpleArray<T>` / `Array<T>` as the checksum sees it.
///
/// `SimpleArray<T>::walk_data` (`0x0047c660` for `WCoord`, `0x00473120` for `int`) emits,
/// on the writing (checksum) path [measured]:
///
/// ```text
///   walk(&length, 4)
///   if length != 0:
///       walk(&size, 4)              // capacity
///       walk(this+0xc, 2)           // increment : short
///       flags &= ~0x40; walk(&flags, 1)
///       walk(list, length * sizeof(T))
/// ```
///
/// `Array<WCoordData>::walk_data` (`0x00478990`) has the identical header and walks its
/// 8-byte elements one at a time — byte-identical input to adler-32, so the same result.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct WalkedArray<T> {
    pub items: Vec<T>,
    /// The engine's `size` (capacity) field. Checksummed, so it must match retail's
    /// growth policy for a bit-exact channel — see the honest-gaps section of the report.
    pub capacity: i32,
    /// The engine's `increment : short`.
    pub increment: i16,
    /// The engine's `flags : unsigned char`; bit `0x40` is cleared before hashing.
    pub flags: u8,
}

impl<T> WalkedArray<T> {
    pub fn new() -> Self {
        WalkedArray {
            items: Vec::new(),
            capacity: 0,
            increment: 0,
            flags: 0,
        }
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// The four `Terrain` members that sit inside the `world` checksum channel.
///
/// [measured: `World::walk_data` sections 10–13 call through `[0x00c06218]`, which the PDB
/// names `MiscAccess::terrain : Terrain&`, at offsets `+0x4b80` `halfland_locs :
/// WCoordList`, `+0x4b9c` `halfland_types : SimpleArray<int>`, `+0x4bb8`
/// `halfland_subtypes : SimpleArray<int>`, `+0x4bd4` `nuke_hits : SimpleArray<int>`.]
///
/// So `MiscAccess` really is not "presentation": four fields of the render-side `Terrain`
/// object are lockstep-critical, and these are exactly which.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct TerrainSync {
    /// `Terrain::halfland_locs` — `Array<WCoordData>`, elements are `(x, y)` `WCoord` pairs.
    pub halfland_locs: WalkedArray<(i32, i32)>,
    /// `Terrain::halfland_types`
    pub halfland_types: WalkedArray<i32>,
    /// `Terrain::halfland_subtypes`
    pub halfland_subtypes: WalkedArray<i32>,
    /// `Terrain::nuke_hits`
    pub nuke_hits: WalkedArray<i32>,
}

// ---------------------------------------------------------------------------------------
// 5. The World
// ---------------------------------------------------------------------------------------

/// The tile world. Field names and offsets are the PDB's `WorldData` layout verbatim
/// (`World : WorldOut : WorldData`, 372 / 368 / 364 bytes).
///
/// **This is the single owner of everything the `world` checksum channel walks**: the
/// `WData` records (territory `who`/`who2` included), the tile mask, the three fog planes,
/// `wcoord_seen`, the danger planes, the collision blocks and the four synced `Terrain`
/// arrays. `crate::systems::borders_fog` supplies the territory and fog *behaviour* and
/// writes through this struct; nothing keeps a second copy.
#[derive(Clone, Debug)]
pub struct World {
    // +0x00 .. +0x04 — checksum section 1
    /// `+0x00` — width in world cells.
    pub xs: i32,
    /// `+0x04` — height in world cells.
    pub ys: i32,

    // +0x08 .. +0x7c — checksum section 4, one contiguous 120-byte block
    /// `+0x08` `= xs * ys`
    pub size: i32,
    /// `+0x0c` `= tile_xs * 2 / 4 = xs * 2`
    pub fog_xs: i32,
    /// `+0x10`
    pub fog_ys: i32,
    /// `+0x14` `= fog_xs * fog_ys`
    pub fog_size: i32,
    /// `+0x18` `= xs * 4`
    pub tile_xs: i32,
    /// `+0x1c` `= ys * 4`
    pub tile_ys: i32,
    /// `+0x20` `= tile_xs * tile_ys`
    pub tile_size: i32,
    /// `+0x24` `= tile_xs / 8 = xs / 2`
    pub reg_xs: i32,
    /// `+0x28`
    pub reg_ys: i32,
    /// `+0x2c` `= reg_xs * reg_ys`
    pub reg_size: i32,
    /// `+0x30`
    pub map: i32,
    /// `+0x34` — initialised to `-1`.
    pub sea_map: i32,
    /// `+0x38` — `Constants+0x118` (`territory_limit_base`, `44 tiles`).
    pub player_territory_limit: i32,
    /// `+0x3c` — `Constants+0x11c` (`territory_limit_civic`, `4 tiles`).
    pub player_territory_limit_civic: i32,
    /// `+0x40` — `Constants+0x120` (`territory_limit_city`, `4 tiles`).
    pub player_territory_limit_city: i32,
    /// `+0x44` — same three `Constants` fields again at init.
    pub colonized_territory_limit: i32,
    /// `+0x48`
    pub colonized_territory_limit_civic: i32,
    /// `+0x4c`
    pub colonized_territory_limit_city: i32,
    /// `+0x50`
    pub player_reg: i32,
    /// `+0x54`
    pub resource_reg: i32,
    /// `+0x58`
    pub forest_size: i32,
    /// `+0x5c`
    pub mountain_size: i32,
    /// `+0x60`
    pub rock_size: i32,
    /// `+0x64`
    pub total_metal: i32,
    /// `+0x68`
    pub total_oil: i32,
    /// `+0x6c`
    pub goodies: i32,
    /// `+0x70`
    pub land_resources: i32,
    /// `+0x74`
    pub sea_resources: i32,
    /// `+0x78`
    pub land_size: i32,
    /// `+0x7c` — the map seed. `Map::make` stores its `seed` argument here **and into the
    /// main RNG's state word**; see [`World::seed_map_generation`].
    pub seed: i32,

    // +0x80 .. +0xf0 — checksum sections 2 and 3
    /// `+0x80` `SimpleArray<WCoord> start_x`
    pub start_x: WalkedArray<i32>,
    /// `+0x9c` `SimpleArray<WCoord> start_y`
    pub start_y: WalkedArray<i32>,
    /// `+0xb8` `SimpleArray<WCoord> start_city_x`
    pub start_city_x: WalkedArray<i32>,
    /// `+0xd4` `SimpleArray<WCoord> start_city_y`
    pub start_city_y: WalkedArray<i32>,
    /// `+0xfc` `SimpleArray<WCoord> oil_x`
    pub oil_x: WalkedArray<i32>,
    /// `+0x118` `SimpleArray<WCoord> oil_y`
    pub oil_y: WalkedArray<i32>,
    /// `+0xf0` `DynamicBitMask start_city_locs` — **not walked by `World::walk_data`**;
    /// only cleared by `wipe`. Kept for state fidelity, absent from the checksum.
    pub start_city_locs: Vec<u8>,

    /// `+0x134` — `size` entries, indexed `wy * xs + wx` (`World::get_wdata` `0x0046d220`).
    pub wdata: Vec<WData>,
    /// `+0x138` — `tile_size` entries, indexed `ty * tile_xs + tx`.
    pub tdata: Vec<u16>,
    /// `+0x13c` — `int* danger[8]`, each `reg_size` ints, indexed by `RCoord`.
    pub danger: [Vec<i32>; 8],
    /// `+0x15c` — `fog_size` bytes; bit per player. `WorldData::is_really_seen`.
    pub seen: Vec<u8>,
    /// `+0x160` — `fog_size` bytes. `WorldData::was_seen`.
    pub seen2: Vec<u8>,
    /// `+0x164` — `fog_size` bytes. `WorldData::is_detected`.
    pub seen3: Vec<u8>,
    /// `+0x168` — `size` bytes (W-resolution). `WorldData::is_seen_wcoord`.
    pub wcoord_seen: Vec<u8>,

    /// Sections 10–13 of the `world` channel; see [`TerrainSync`].
    pub terrain_sync: TerrainSync,
}

impl World {
    /// `World::init(unsigned short xs, unsigned short ys)` `0x006b76f0`.
    ///
    /// Every derived dimension below is straight off the instruction stream
    /// (`0x006b7718`–`0x006b779a`). Note that `fog_xs` is computed as
    /// `(tile_xs * 2) / 4` and `reg_xs` as `tile_xs / 8` — signed divisions, so they
    /// round toward zero, which matters only for odd `xs`.
    ///
    /// `territory_limit_*` come from `Constants+0x118/0x11c/0x120`
    /// (`docs/derivation/rules-constants.json`: `44 / 4 / 4`), and the `colonized_*`
    /// triple is initialised from the *same three fields*, not from separate rules.
    pub fn init(
        xs: i32,
        ys: i32,
        territory_base: i32,
        territory_civic: i32,
        territory_city: i32,
    ) -> World {
        let tile_xs = xs * 4;
        let tile_ys = ys * 4;
        let fog_xs = (tile_xs * 2) / 4;
        let fog_ys = (tile_ys * 2) / 4;
        let reg_xs = tile_xs / 8;
        let reg_ys = tile_ys / 8;
        let size = xs * ys;
        let tile_size = tile_xs * tile_ys;
        let fog_size = fog_xs * fog_ys;
        let reg_size = reg_xs * reg_ys;

        World {
            xs,
            ys,
            size,
            fog_xs,
            fog_ys,
            fog_size,
            tile_xs,
            tile_ys,
            tile_size,
            reg_xs,
            reg_ys,
            reg_size,
            map: 0,
            sea_map: -1,
            player_territory_limit: territory_base,
            player_territory_limit_civic: territory_civic,
            player_territory_limit_city: territory_city,
            colonized_territory_limit: territory_base,
            colonized_territory_limit_civic: territory_civic,
            colonized_territory_limit_city: territory_city,
            player_reg: 0,
            resource_reg: 0,
            forest_size: 0,
            mountain_size: 0,
            rock_size: 0,
            total_metal: 0,
            total_oil: 0,
            goodies: 0,
            land_resources: 0,
            sea_resources: 0,
            land_size: 0,
            seed: 0,
            start_x: WalkedArray::new(),
            start_y: WalkedArray::new(),
            start_city_x: WalkedArray::new(),
            start_city_y: WalkedArray::new(),
            oil_x: WalkedArray::new(),
            oil_y: WalkedArray::new(),
            start_city_locs: Vec::new(),
            wdata: vec![WData::default(); size.max(0) as usize],
            tdata: vec![0u16; tile_size.max(0) as usize],
            danger: std::array::from_fn(|_| vec![0i32; reg_size.max(0) as usize]),
            seen: vec![0u8; fog_size.max(0) as usize],
            seen2: vec![0u8; fog_size.max(0) as usize],
            seen3: vec![0u8; fog_size.max(0) as usize],
            wcoord_seen: vec![0u8; size.max(0) as usize],
            terrain_sync: TerrainSync::default(),
        }
    }

    /// Default `Constants` values from `ron-data/rules.xml` via `Constants::init`:
    /// `territory_limit_base = 44`, `_civic = 4`, `_city = 4`.
    pub fn init_default_rules(xs: i32, ys: i32) -> World {
        World::init(xs, ys, 44, 4, 4)
    }

    /// `World::wipe` `0x006b2c00` — reset to the post-generation blank state.
    pub fn wipe(&mut self) {
        self.start_x.items.clear();
        self.start_y.items.clear();
        self.start_city_x.items.clear();
        self.start_city_y.items.clear();
        for b in self.start_city_locs.iter_mut() {
            *b = 0;
        }
        self.sea_map = -1;
        self.land_size = 0;
        self.player_reg = 0;
        self.resource_reg = 0;
        self.forest_size = 0;
        self.mountain_size = 0;
        self.rock_size = 0;
        self.total_metal = 0;
        self.total_oil = 0;
        self.goodies = 0;
        self.land_resources = 0;
        self.sea_resources = 0;
        for w in self.wdata.iter_mut() {
            let block = w.block.take();
            *w = WData::default();
            w.block = block;
        }
        for t in self.tdata.iter_mut() {
            *t = 0;
        }
        for b in self.seen.iter_mut() {
            *b = 0;
        }
        for b in self.seen2.iter_mut() {
            *b = 0;
        }
        for b in self.seen3.iter_mut() {
            *b = 0;
        }
        for d in self.danger.iter_mut() {
            for v in d.iter_mut() {
                *v = 0;
            }
        }
    }

    /// `Map::make` `0x0068bc90`, entry sequence at `0x0068bcbf`–`0x0068bcd0` [measured]:
    ///
    /// ```text
    ///   test ecx, ecx                ; ecx = the seed argument
    ///   js   skip                    ; negative seed => keep whatever is there
    ///   mov  eax, [0x00c06188]       ; GameAccess::world
    ///   mov  [eax + 0x7c], ecx       ; World::seed = seed
    ///   mov  eax, [0x00c06184]       ; GameAccess::game_random
    ///   mov  [eax], ecx              ; the LCG state word = seed
    /// ```
    ///
    /// So **map generation is reproducible from one 32-bit seed**, and it consumes the
    /// *main simulation* RNG stream (`game_random`, LCG `s ← s*1664525 + 1013904223`) —
    /// not a private generator stream. Returns the new LCG state.
    pub fn seed_map_generation(&mut self, seed: i32) -> Option<i32> {
        if seed < 0 {
            return None;
        }
        self.seed = seed;
        Some(seed)
    }

    /// `WorldData::start_city_wcoord(WCoord const&, WCoord const&)` `0x006b30e0`.
    ///
    /// The complete retail leaf is a row-major flatten followed by an LSB-first
    /// bit test.  It performs no bounds check; callers must pass a valid world
    /// coordinate backed by `start_city_locs`, just as the engine does.
    #[inline]
    pub fn start_city_wcoord(&self, x: WCoord, y: WCoord) -> bool {
        let index = y.0 * self.xs + x.0;
        self.start_city_locs[(index >> 3) as usize] & (1u8 << ((index & 7) as u32)) != 0
    }

    // -- indexing ------------------------------------------------------------------------

    /// `World::get_wdata` `0x0046d220`: `wdata[wy * xs + wx]`, stride 28.
    #[inline]
    pub fn w_index(&self, wx: i32, wy: i32) -> usize {
        (wy * self.xs + wx) as usize
    }
    /// `tdata[ty * tile_xs + tx]`, stride 2.
    #[inline]
    pub fn t_index(&self, tx: i32, ty: i32) -> usize {
        (ty * self.tile_xs + tx) as usize
    }
    /// `seen[fy * fog_xs + fx]` (`World::get_seen` `0x006b4290`).
    #[inline]
    pub fn f_index(&self, fx: i32, fy: i32) -> usize {
        (fy * self.fog_xs + fx) as usize
    }
    /// `danger[who][ry * reg_xs + rx]`.
    #[inline]
    pub fn r_index(&self, rx: i32, ry: i32) -> usize {
        (ry * self.reg_xs + rx) as usize
    }

    /// `WorldData::is_valid(WCoord,WCoord)` `0x00461420`.
    #[inline]
    pub fn valid_w(&self, wx: i32, wy: i32) -> bool {
        wx >= 0 && wy >= 0 && wx < self.xs && wy < self.ys
    }
    /// `WorldData::is_valid(TCoord,TCoord)` `0x0046f760`.
    #[inline]
    pub fn valid_t(&self, tx: i32, ty: i32) -> bool {
        tx >= 0 && ty >= 0 && tx < self.tile_xs && ty < self.tile_ys
    }
    /// Fog-plane bounds — the check `Object::update_seen` `0x00651b80` hoists out of the
    /// disc loop when the whole disc is on the map.
    #[inline]
    pub fn valid_f(&self, fx: i32, fy: i32) -> bool {
        fx >= 0 && fy >= 0 && fx < self.fog_xs && fy < self.fog_ys
    }
    /// `WorldData::is_valid(Coord,Coord)` `0x0043f360` — bounds are `tile_xs * 192`.
    #[inline]
    pub fn valid_coord(&self, cx: i32, cy: i32) -> bool {
        cx >= 0
            && cy >= 0
            && cx < self.tile_xs * COORD_PER_TILE
            && cy < self.tile_ys * COORD_PER_TILE
    }
    /// `WorldData::is_edge` `0x0047a970`.
    #[inline]
    pub fn is_edge(&self, wx: i32, wy: i32) -> bool {
        wx == 0 || wy == 0 || wx == self.xs - 1 || wy == self.ys - 1
    }

    #[inline]
    pub fn wdata(&self, wx: i32, wy: i32) -> &WData {
        &self.wdata[self.w_index(wx, wy)]
    }
    #[inline]
    pub fn wdata_mut(&mut self, wx: i32, wy: i32) -> &mut WData {
        let i = self.w_index(wx, wy);
        &mut self.wdata[i]
    }
    #[inline]
    pub fn tmask(&self, tx: i32, ty: i32) -> u16 {
        self.tdata[self.t_index(tx, ty)]
    }
    #[inline]
    pub fn tmask_mut(&mut self, tx: i32, ty: i32) -> &mut u16 {
        let i = self.t_index(tx, ty);
        &mut self.tdata[i]
    }

    // -- terrain predicates ---------------------------------------------------------------

    /// `WorldData::is_cliff_at` `0x0046f8c0`.
    pub fn is_cliff_at(&self, tx: i32, ty: i32) -> bool {
        self.tmask(tx, ty) & tflag::BLOCKER_MASK == tflag::BLOCKER_CLIFF
    }
    /// `WorldData::is_mountain_at` `0x0046f900`.
    pub fn is_mountain_at(&self, tx: i32, ty: i32) -> bool {
        self.tmask(tx, ty) & tflag::BLOCKER_MASK == tflag::BLOCKER_MOUNTAIN
    }
    /// `WorldData::is_tree_at` `0x0046f930`.
    pub fn is_tree_at(&self, tx: i32, ty: i32) -> bool {
        self.tmask(tx, ty) & tflag::SURFACE_MASK == tflag::SURFACE_TREES
    }
    /// `WorldData::is_tocean` `0x0046fb10`.
    pub fn is_tocean(&self, tx: i32, ty: i32) -> bool {
        self.tmask(tx, ty) & tflag::SURFACE_MASK == tflag::SURFACE_WATER
    }
    /// `WorldData::is_river` `0x0046d390`.
    pub fn is_river(&self, tx: i32, ty: i32) -> bool {
        self.tmask(tx, ty) & tflag::RIVER != 0
    }
    /// `WorldData::is_gathered_from` `0x00472ac0`.
    pub fn is_gathered_from(&self, tx: i32, ty: i32) -> bool {
        self.tmask(tx, ty) & tflag::GATHERED != 0
    }
    /// `WorldData::is_built_at` `0x0046f880` — building blocker **or** the `STARTED` bit.
    pub fn is_built_at(&self, tx: i32, ty: i32) -> bool {
        let m = self.tmask(tx, ty);
        (m & tflag::BLOCKER_MASK) == tflag::BLOCKER_BUILDING || (m & tflag::STARTED) != 0
    }
    /// `WorldData::is_blocked_at` `0x00461340`. `ignore_trees` is the third argument:
    /// when non-zero, a tile blocked *only* because it holds trees reads as unblocked.
    pub fn is_blocked_at(&self, tx: i32, ty: i32, ignore_trees: bool) -> bool {
        let m = self.tmask(tx, ty);
        if !ignore_trees {
            m & tflag::BLOCKED != 0
        } else {
            (m & tflag::BLOCKED) != 0 && (m & tflag::SURFACE_MASK) != tflag::SURFACE_TREES
        }
    }
    /// `WorldData::has_blocked_neighbors` `0x006b2990` — any of the 8 in-bounds
    /// neighbours carries [`tflag::BLOCKED`].
    pub fn has_blocked_neighbors(&self, tx: i32, ty: i32) -> bool {
        for i in 0..8 {
            let nx = tx + NEIGHBOUR_DX[i];
            let ny = ty + NEIGHBOUR_DY[i];
            if self.valid_t(nx, ny) && self.tmask(nx, ny) & tflag::BLOCKED != 0 {
                return true;
            }
        }
        false
    }

    /// `WorldData::is_ocean` `0x006b4830`: a `WATERHALF` cell is never "ocean";
    /// otherwise `land ∈ {1, 2}`.
    pub fn is_ocean(&self, wx: i32, wy: i32) -> bool {
        let w = self.wdata(wx, wy);
        if w.flags & wflag::WATERHALF != 0 {
            return false;
        }
        w.land == land::COASTAL || w.land == land::OCEAN
    }
    /// `WorldData::is_forest` `0x006b5560`.
    pub fn is_forest(&self, wx: i32, wy: i32) -> bool {
        self.wdata(wx, wy).flags & wflag::FOREST != 0
    }
    /// `WorldData::is_mountains` `0x006b5590`.
    pub fn is_mountains(&self, wx: i32, wy: i32) -> bool {
        self.wdata(wx, wy).flags & wflag::MOUNTAINS != 0
    }
    /// `WorldData::is_rocks` `0x006b4380`.
    pub fn is_rocks(&self, wx: i32, wy: i32) -> bool {
        self.wdata(wx, wy).flags & wflag::ROCKS != 0
    }
    /// `WorldData::is_coast` `0x006b3020`.
    pub fn is_coast(&self, wx: i32, wy: i32) -> bool {
        self.wdata(wx, wy).flags & wflag::COAST != 0
    }
    /// `WorldData::is_dead_build` `0x006b2390`.
    pub fn is_dead_build(&self, wx: i32, wy: i32) -> bool {
        self.wdata(wx, wy).flags & wflag::DEAD_BUILD != 0
    }
    /// `WorldData::is_oil_at` `0x00472af0`.
    pub fn is_oil_at(&self, wx: i32, wy: i32) -> bool {
        self.wdata(wx, wy).flags & wflag::OIL != 0
    }
    /// `WorldData::is_passable` `0x006b23c0` — `(flags & 0x70) == 0`.
    pub fn is_passable(&self, wx: i32, wy: i32) -> bool {
        self.wdata(wx, wy).flags & wflag::IMPASSABLE_MASK == 0
    }
    /// `WorldData::is_impassable` `0x006b4880`.
    pub fn is_impassable(&self, wx: i32, wy: i32) -> bool {
        !self.is_passable(wx, wy)
    }
    /// `WorldData::is_pass_land` `0x006b56b0` — passable-ish *and* not ocean.
    pub fn is_pass_land(&self, wx: i32, wy: i32) -> bool {
        self.wdata(wx, wy).flags & (wflag::MOUNTAINS | wflag::FOREST) == 0 && !self.is_ocean(wx, wy)
    }
    /// `WorldData::is_flat` `0x006b3120`.
    pub fn is_flat(&self, wx: i32, wy: i32) -> bool {
        let f = self.wdata(wx, wy).flags;
        f & (wflag::MOUNTAINS | wflag::FOREST) == 0
            && !self.is_ocean(wx, wy)
            && f & (wflag::ROCKS | wflag::IMPASSABLE_X) == 0
    }
    /// `WorldData::buildings_allowed` `0x006b2340` — `(flags & 0x78) == 0`.
    pub fn buildings_allowed(&self, wx: i32, wy: i32) -> bool {
        self.wdata(wx, wy).flags & wflag::NO_BUILD_MASK == 0
    }
    /// `WorldData::num_waterhalf` `0x006b4db0` — for a `WATERHALF` cell, how many of its
    /// 16 tiles are water.
    pub fn num_waterhalf(&self, wx: i32, wy: i32) -> i32 {
        if self.wdata(wx, wy).flags & wflag::WATERHALF == 0 {
            return 0;
        }
        let mut n = 0;
        for i in 0..TILES_IN_WCELL {
            let tx = wx * TILES_PER_WCELL + (i & 3);
            let ty = wy * TILES_PER_WCELL + (i >> 2);
            if self.tmask(tx, ty) & tflag::SURFACE_MASK == tflag::SURFACE_WATER {
                n += 1;
            }
        }
        n
    }

    /// `WorldData::get_region` `0x006b5680`.
    pub fn get_region(&self, wx: i32, wy: i32) -> i32 {
        self.wdata(wx, wy).region as i32
    }
    /// `WorldData::get_region2` `0x006b3960`.
    pub fn get_region2(&self, wx: i32, wy: i32) -> i32 {
        self.wdata(wx, wy).region2 as i32
    }
    /// `WorldData::get_who` / `get_whose` `0x006b4700` / `0x006b4d80` — territory owner.
    ///
    /// `who` and `who2` are section-5 bytes (`WData +0x0f`, `+0x10`), so **this struct owns
    /// territory ownership**: the writer is
    /// [`crate::systems::borders_fog::check_borders`] and it writes through here, not into a
    /// private plane. See that module's header for why there used to be two.
    pub fn get_who(&self, wx: i32, wy: i32) -> i32 {
        self.wdata(wx, wy).who as i32
    }
    /// `WorldData::get_who2` `0x006b2510` — the runner-up claimant.
    pub fn get_who2(&self, wx: i32, wy: i32) -> i32 {
        self.wdata(wx, wy).who2 as i32
    }
    /// `WorldData::get_goods` `0x0046ef10`.
    pub fn get_goods(&self, wx: i32, wy: i32) -> u8 {
        self.wdata(wx, wy).goods
    }
    /// `WorldData::get_tregion` `0x006b52e0` — the tile's region: `region2` when the owning
    /// cell is `WATERHALF` **and** this tile is water, otherwise `region`.
    pub fn get_tregion(&self, tx: i32, ty: i32) -> i32 {
        let w = self.wdata(tx >> 2, ty >> 2);
        if w.flags & wflag::WATERHALF != 0
            && self.tmask(tx, ty) & tflag::SURFACE_MASK == tflag::SURFACE_WATER
        {
            w.region2 as i32
        } else {
            w.region as i32
        }
    }
    /// `WorldData::get_coll_block` `0x006b5350` — `region < 0` means "any region".
    pub fn get_coll_block(&self, wx: i32, wy: i32, region: i32) -> Option<&CollBlock> {
        let w = self.wdata(wx, wy);
        if region >= 0 && w.region as i32 != region {
            return None;
        }
        w.block.as_deref()
    }

    /// `WorldData::get_land(WCoord, WCoord, int mode)` `0x006b4730`. The `mode > 0` arm is
    /// the terrain-class query the UI and the AI use:
    /// `3` coast, `4` forest, `5` mountains, `6` rocks, `7` rocks-with-oil, else `land`.
    pub fn get_land(&self, wx: i32, wy: i32, mode: i32) -> i32 {
        let w = self.wdata(wx, wy);
        let f = w.flags;
        let base = w.land as i32;
        if mode == 0 {
            if f & wflag::COAST != 0 {
                return 2;
            }
            base
        } else if mode < 0 {
            if f & wflag::COAST != 0 {
                3
            } else {
                base
            }
        } else {
            if f & wflag::COAST != 0 {
                return 3;
            }
            if f & wflag::FOREST != 0 {
                return 4;
            }
            if f & (wflag::MOUNTAINS | wflag::IMPASSABLE_X) != 0 {
                return 5;
            }
            if f & wflag::ROCKS != 0 {
                return if f & wflag::OIL != 0 { 7 } else { 6 };
            }
            if self.is_ocean(wx, wy) && f & wflag::OIL != 0 {
                return 7;
            }
            base
        }
    }

    // -- placement predicates (consumed by the production lane) -------------------------------

    /// `WorldData::space_at_corner(TCoord, TCoord, int who, int, int need_city)`
    /// `0x006b27f0` — grade the 4x4 tile block anchored at `(tx, ty)` for building
    /// placement. Returns one of the [`space`] constants.
    ///
    /// A probe position counts as blocked when **any** of these holds [measured]:
    /// out of bounds; `need_city` is set and the tile lacks [`tflag::CITY`]; the tile is a
    /// building ([`tflag::BLOCKER_BUILDING`]) or has [`tflag::STARTED`]; the owning cell's
    /// `who` is `>= 0` and differs from `who`; or the tile has [`tflag::BLOCKED`].
    ///
    /// The early `return 0` for the inner 2x2 fires on the *first* blocked core probe, so
    /// the ring is not even scored in that case.
    pub fn space_at_corner(&self, tx: i32, ty: i32, who: i32, need_city: bool) -> i32 {
        let mut slot = [0u8; 16];
        let mut blocked_count = 0;
        for i in 0..16usize {
            let px = tx + SPACE_PROBE_DX[i];
            let py = ty + SPACE_PROBE_DY[i];
            let bad = if !self.valid_t(px, py) {
                true
            } else {
                let m = self.tmask(px, py);
                let owner = self.wdata(px >> 2, py >> 2).who as i32;
                (m & tflag::CITY == 0 && need_city)
                    || (m & tflag::BLOCKER_MASK) == tflag::BLOCKER_BUILDING
                    || (m & tflag::STARTED) != 0
                    || (owner >= 0 && owner != who)
                    || (m & tflag::BLOCKED) != 0
            };
            if bad {
                if i < 4 {
                    return space::CORE_BLOCKED;
                }
                slot[i] = 1;
                blocked_count += 1;
            }
        }
        if blocked_count == 0 {
            return space::FULLY_CLEAR;
        }
        if blocked_count < 8 {
            for group in SPACE_APPROACH_GROUPS.iter() {
                if group.iter().all(|&i| slot[i] == 0) {
                    return space::APPROACH_CLEAR;
                }
            }
        }
        space::PARTIAL
    }

    /// `WorldData::check_building_wcoord` `0x006b26e0` — the W-cell gate in front of
    /// [`World::space_at_corner`].
    ///
    /// Rejects immediately when the cell is owned by another player, is impassable
    /// (`flags & 0x70`), or has **all sixteen** of its tiles blocked (`WData::blocked == 16`).
    /// Otherwise it sweeps `dx in -rx..=rx`, `dy in -ry..=ry` over the cell's tiles,
    /// skipping offsets whose Manhattan distance exceeds `max_dist` (except on the axes),
    /// and returns the best grade found, short-circuiting on [`space::FULLY_CLEAR`].
    pub fn check_building_wcoord(
        &self,
        wx: i32,
        wy: i32,
        who: i32,
        rx: i32,
        ry: i32,
        max_dist: i32,
        need_city: bool,
    ) -> i32 {
        let w = self.wdata(wx, wy);
        let owner = w.who as i32;
        if (owner >= 0 && owner != who) || w.flags & wflag::IMPASSABLE_MASK != 0 || w.blocked == 16
        {
            return space::CORE_BLOCKED;
        }
        let mut best = space::CORE_BLOCKED;
        let mut dx = -rx;
        while dx <= rx {
            let mut dy = -ry;
            while dy <= ry {
                if dx == 0 || dy == 0 || dx.abs() + dy.abs() <= max_dist {
                    let r = self.space_at_corner(wx * 4 + dx, wy * 4 + dy, who, need_city);
                    if r > best {
                        best = r;
                    }
                    if best == space::FULLY_CLEAR {
                        return best;
                    }
                }
                dy += 1;
            }
            dx += 1;
        }
        best
    }

    /// `WorldData::has_gather_access` `0x006b4e50`, `mode != 0` arm — can a gatherer reach
    /// this tile? True when the tile is trees, a mountain or a cliff **and** at least one
    /// of its four N/E/S/W neighbours is neither water nor blocked.
    pub fn has_gather_access(&self, tx: i32, ty: i32) -> bool {
        let m = self.tmask(tx, ty);
        let harvestable = (m & tflag::SURFACE_MASK) == tflag::SURFACE_TREES
            || (m & tflag::BLOCKER_MASK) == tflag::BLOCKER_MOUNTAIN
            || (m & tflag::BLOCKER_MASK) == tflag::BLOCKER_CLIFF;
        if !harvestable {
            return false;
        }
        for i in 0..4 {
            let nx = tx + NEIGHBOUR4_DX[i];
            let ny = ty + NEIGHBOUR4_DY[i];
            if !self.valid_t(nx, ny) {
                continue;
            }
            let nm = self.tmask(nx, ny);
            if (nm & tflag::SURFACE_MASK) != tflag::SURFACE_WATER && (nm & tflag::BLOCKED) == 0 {
                return true;
            }
        }
        false
    }

    // -- fog -------------------------------------------------------------------------------

    /// `World::get_seen` `0x006b4290` — the raw per-player bitmask byte.
    pub fn get_seen(&self, fx: i32, fy: i32) -> u8 {
        self.seen[self.f_index(fx, fy)]
    }
    /// `World::get_was_seen` `0x006b1ba0`.
    pub fn get_was_seen(&self, fx: i32, fy: i32) -> u8 {
        self.seen2[self.f_index(fx, fy)]
    }
    /// `WorldData::is_really_seen` `0x006b42c0`, the plane-and-mask core (the leader-level
    /// reveal/ally short-circuits belong to the leader lane).
    pub fn is_really_seen(&self, fx: i32, fy: i32, player_mask: u8) -> bool {
        self.get_seen(fx, fy) & player_mask != 0
    }
    /// `WorldData::was_seen` `0x006b53f0`, plane-and-mask core.
    pub fn was_seen(&self, fx: i32, fy: i32, player_mask: u8) -> bool {
        self.get_was_seen(fx, fy) & player_mask != 0
    }
    /// `WorldData::is_detected` `0x006b48c0`.
    pub fn is_detected(&self, fx: i32, fy: i32, player_mask: u8) -> bool {
        self.seen3[self.f_index(fx, fy)] & player_mask != 0
    }
    /// `WorldData::is_seen_wcoord` `0x006b2540`, plane-and-mask core (W resolution).
    pub fn is_seen_wcoord(&self, wx: i32, wy: i32, player_mask: u8) -> bool {
        self.wcoord_seen[self.w_index(wx, wy)] & player_mask != 0
    }
    /// `World::clear_seen` `0x006b2250` — zero the live-visibility plane. Called once per
    /// tick from `GameDaemon::update_all_seen`.
    pub fn clear_seen(&mut self) {
        // **Corrected while reconciling with `borders_fog`, which had this right.** The
        // retail body ends with two memsets, not one [measured, `re/decomp-all/006b2250.c`]:
        //   memset(world+0x168, 0, world+0x08)   // wcoord_seen, `size` bytes
        //   memset(world+0x15c, 0, world+0x14)   // seen,        `fog_size` bytes
        // Clearing only `seen` left `wcoord_seen` — section 7 of the checksum — accumulating
        // forever, so this world's channel 12 would have diverged from retail's on the
        // second frame of any game. The loop the two memsets follow walks `seen` and calls
        // a presentation callback per lit cell; it writes no sim state and is not ported.
        self.wcoord_seen.fill(0);
        self.seen.fill(0);
    }
    /// `World::clear_danger` `0x006b22e0`.
    pub fn clear_danger(&mut self, who: usize) {
        for v in self.danger[who].iter_mut() {
            *v = 0;
        }
    }

    // -- terrain mutators --------------------------------------------------------------------

    /// `World::set_blocked_at` `0x006b4900`. Maintains `WData::blocked` and `WData::solid`
    /// counters, clears the tile's own `BAD_PATH`, removes any road, then propagates
    /// `BAD_PATH` to (or re-evaluates it on) the 8 neighbours.
    pub fn set_blocked_at(&mut self, tx: i32, ty: i32, on: bool) {
        let wx = tx >> 2;
        let wy = ty >> 2;
        let ti = self.t_index(tx, ty);
        let wi = self.w_index(wx, wy);
        let had = self.tdata[ti] & tflag::BLOCKED != 0;
        if !on {
            if had {
                self.wdata[wi].blocked = self.wdata[wi].blocked.wrapping_sub(1);
                self.wdata[wi].solid = self.wdata[wi].solid.wrapping_sub(1);
            }
            self.tdata[ti] &= !tflag::BLOCKED;
        } else {
            if !had {
                self.wdata[wi].blocked = self.wdata[wi].blocked.wrapping_add(1);
                self.wdata[wi].solid = self.wdata[wi].solid.wrapping_add(1);
            }
            self.tdata[ti] |= tflag::BLOCKED;
            if self.tdata[ti] & tflag::BAD_PATH != 0 {
                self.wdata[wi].bad = self.wdata[wi].bad.wrapping_sub(1);
            }
            self.tdata[ti] &= !tflag::BAD_PATH;
            self.set_road_at(tx, ty, false, 0, false);
        }
        for i in 0..8 {
            let nx = tx + NEIGHBOUR_DX[i];
            let ny = ty + NEIGHBOUR_DY[i];
            if !self.valid_t(nx, ny) {
                continue;
            }
            let ni = self.t_index(nx, ny);
            let nwi = self.w_index(nx >> 2, ny >> 2);
            if on {
                if self.tdata[ni] & tflag::BAD_PATH == 0 {
                    self.wdata[nwi].bad = self.wdata[nwi].bad.wrapping_add(1);
                }
                self.tdata[ni] |= tflag::BAD_PATH;
            } else if !self.has_blocked_neighbors(nx, ny) {
                if self.tdata[ni] & tflag::BAD_PATH != 0 {
                    self.wdata[nwi].bad = self.wdata[nwi].bad.wrapping_sub(1);
                }
                self.tdata[ni] &= !tflag::BAD_PATH;
            }
        }
        if !on && self.has_blocked_neighbors(tx, ty) {
            if self.tdata[ti] & tflag::BAD_PATH == 0 {
                self.wdata[wi].bad = self.wdata[wi].bad.wrapping_add(1);
            }
            self.tdata[ti] |= tflag::BAD_PATH;
        }
    }

    /// `World::set_bad_path` `0x006b4610`.
    pub fn set_bad_path(&mut self, tx: i32, ty: i32, on: bool) {
        let ti = self.t_index(tx, ty);
        let wi = self.w_index(tx >> 2, ty >> 2);
        let had = self.tdata[ti] & tflag::BAD_PATH != 0;
        if on {
            if !had {
                self.wdata[wi].bad = self.wdata[wi].bad.wrapping_add(1);
            }
            self.tdata[ti] |= tflag::BAD_PATH;
        } else {
            if had {
                self.wdata[wi].bad = self.wdata[wi].bad.wrapping_sub(1);
            }
            self.tdata[ti] &= !tflag::BAD_PATH;
        }
    }

    /// `World::set_cliff_at` `0x006b1eb0` — blocks the tile, then stamps the blocker kind.
    /// Note the engine calls `set_blocked_at` **between** reading the tile pointer and
    /// writing the mask, so the blocker bits win over anything `set_blocked_at` did.
    pub fn set_cliff_at(&mut self, tx: i32, ty: i32) {
        self.set_blocked_at(tx, ty, true);
        let ti = self.t_index(tx, ty);
        self.tdata[ti] = (self.tdata[ti] & !0x2) | tflag::BLOCKER_CLIFF;
    }

    /// `World::set_mountain_at` `0x006b1f00`.
    pub fn set_mountain_at(&mut self, tx: i32, ty: i32, on: bool) {
        self.set_blocked_at(tx, ty, on);
        let ti = self.t_index(tx, ty);
        if on {
            self.tdata[ti] = (self.tdata[ti] & !0x1) | tflag::BLOCKER_MOUNTAIN;
        } else if self.tdata[ti] & tflag::BLOCKER_MASK == tflag::BLOCKER_MOUNTAIN {
            self.tdata[ti] &= !tflag::BLOCKER_MASK;
        }
    }

    /// `World::set_building_at` `0x006b45c0`.
    pub fn set_building_at(&mut self, tx: i32, ty: i32, on: bool) {
        let ti = self.t_index(tx, ty);
        if on {
            self.tdata[ti] |= tflag::BLOCKER_BUILDING;
        } else if self.tdata[ti] & tflag::BLOCKER_MASK == tflag::BLOCKER_BUILDING {
            self.tdata[ti] &= !tflag::BLOCKER_MASK;
        }
    }

    /// `World::set_road_at` `0x006b43b0`. `quiet` suppresses the `Roads::road_added` /
    /// `road_cleared` presentation callbacks (the engine's 5th argument); the `road_type`
    /// argument is passed straight through to `Roads::road_added` and does not touch tile
    /// state. Maintains [`wflag::HAS_ROAD`] on the owning W cell.
    pub fn set_road_at(&mut self, tx: i32, ty: i32, on: bool, _road_type: i32, _quiet: bool) {
        let ti = self.t_index(tx, ty);
        let wx = tx >> 2;
        let wy = ty >> 2;
        let wi = self.w_index(wx, wy);
        if on {
            self.tdata[ti] = (self.tdata[ti] & !tflag::SURFACE_WATER) | tflag::SURFACE_ROAD;
            self.wdata[wi].flags |= wflag::HAS_ROAD;
        } else {
            if self.tdata[ti] & tflag::SURFACE_MASK == tflag::SURFACE_ROAD {
                self.tdata[ti] &= !tflag::SURFACE_MASK;
            }
            // Clear the cell-level flag only when no tile of the cell still has a road.
            let mut any = false;
            'scan: for cx in wx * 4..wx * 4 + 4 {
                for cy in wy * 4..wy * 4 + 4 {
                    if self.tmask(cx, cy) & tflag::SURFACE_MASK == tflag::SURFACE_ROAD {
                        any = true;
                        break 'scan;
                    }
                }
            }
            if !any {
                self.wdata[wi].flags &= !wflag::HAS_ROAD;
            }
        }
    }

    /// `World::set_tree_at` `0x006b2060`. The `solid` counter moves the *opposite* way to
    /// `set_blocked_at`'s — that is measured, not a transcription slip.
    pub fn set_tree_at(&mut self, tx: i32, ty: i32, on: bool) {
        let ti = self.t_index(tx, ty);
        let wi = self.w_index(tx >> 2, ty >> 2);
        let was_trees = self.tdata[ti] & tflag::SURFACE_MASK == tflag::SURFACE_TREES;
        if on {
            if !was_trees {
                self.wdata[wi].solid = self.wdata[wi].solid.wrapping_sub(1);
            }
            self.tdata[ti] |= tflag::SURFACE_TREES | tflag::BEHIND_B;
        } else {
            if was_trees {
                self.tdata[ti] &= !tflag::SURFACE_MASK;
                self.wdata[wi].solid = self.wdata[wi].solid.wrapping_add(1);
            }
            self.tdata[ti] &= !tflag::BEHIND_B;
        }
    }

    /// `World::set_tocean` `0x006b1c60`.
    pub fn set_tocean(&mut self, tx: i32, ty: i32) {
        let ti = self.t_index(tx, ty);
        self.tdata[ti] = (self.tdata[ti] & !0x10) | tflag::SURFACE_WATER;
    }

    /// `World::set_waterhalf` `0x006b1c90`.
    pub fn set_waterhalf(&mut self, tx: i32, ty: i32, on: bool) {
        let ti = self.t_index(tx, ty);
        if on {
            self.tdata[ti] = (self.tdata[ti] & !0x10) | tflag::SURFACE_WATER;
            let wi = self.w_index(tx >> 2, ty >> 2);
            self.wdata[wi].flags |= wflag::WATERHALF;
        } else if self.tdata[ti] & tflag::SURFACE_MASK == tflag::SURFACE_WATER {
            self.tdata[ti] &= !tflag::SURFACE_MASK;
        }
    }

    /// `World::clear_waterhalf` `0x006b1bd0` — W-indexed: clears water from all 16 tiles
    /// (when `keep == false`) and always clears the cell flag.
    pub fn clear_waterhalf(&mut self, wx: i32, wy: i32, keep: bool) {
        if !keep {
            for i in 0..TILES_IN_WCELL {
                let tx = wx * 4 + (i & 3);
                let ty = wy * 4 + (i >> 2);
                let ti = self.t_index(tx, ty);
                if self.tdata[ti] & tflag::SURFACE_MASK == tflag::SURFACE_WATER {
                    self.tdata[ti] &= !tflag::SURFACE_MASK;
                }
            }
        }
        let wi = self.w_index(wx, wy);
        self.wdata[wi].flags &= !wflag::WATERHALF;
    }

    /// `World::set_river_at` `0x006b1f80` — maintains [`wflag::HAS_RIVER`] by scanning all
    /// 16 tiles of the owning cell on clear.
    pub fn set_river_at(&mut self, tx: i32, ty: i32, on: bool) {
        let ti = self.t_index(tx, ty);
        let wx = tx >> 2;
        let wy = ty >> 2;
        let wi = self.w_index(wx, wy);
        if on {
            self.tdata[ti] |= tflag::RIVER;
            self.wdata[wi].flags |= wflag::HAS_RIVER;
        } else {
            self.tdata[ti] &= !tflag::RIVER;
            let mut any = false;
            for i in 0..TILES_IN_WCELL {
                let cx = wx * 4 + (i & 3);
                let cy = wy * 4 + (i >> 2);
                if self.tmask(cx, cy) & tflag::RIVER != 0 {
                    any = true;
                    break;
                }
            }
            if !any {
                self.wdata[wi].flags &= !wflag::HAS_RIVER;
            }
        }
    }

    /// `World::set_coastal` `0x006b1d10`.
    pub fn set_coastal(&mut self, tx: i32, ty: i32, on: bool) {
        self.set_tbit(tx, ty, tflag::COASTAL, on)
    }
    /// `World::set_resource_at` `0x006b3a80` — the tile side of resource-node placement.
    pub fn set_resource_at(&mut self, tx: i32, ty: i32, on: bool) {
        self.set_tbit(tx, ty, tflag::RESOURCE, on)
    }
    /// `World::set_city_at` `0x006b4180`.
    pub fn set_city_at(&mut self, tx: i32, ty: i32, on: bool) {
        self.set_tbit(tx, ty, tflag::CITY, on)
    }
    /// `World::set_started_at` `0x006b4570`.
    pub fn set_started_at(&mut self, tx: i32, ty: i32, on: bool) {
        self.set_tbit(tx, ty, tflag::STARTED, on)
    }
    /// `World::set_started2_at` `0x006b4530`.
    pub fn set_started2_at(&mut self, tx: i32, ty: i32, on: bool) {
        self.set_tbit(tx, ty, tflag::STARTED2, on)
    }
    /// `World::set_gathered_at` `0x006b46b0`.
    pub fn set_gathered_at(&mut self, tx: i32, ty: i32, on: bool) {
        self.set_tbit(tx, ty, tflag::GATHERED, on)
    }
    /// `World::set_gather_edge` `0x006b2110` — set-only, the engine has no clear path.
    pub fn set_gather_edge(&mut self, tx: i32, ty: i32) {
        let ti = self.t_index(tx, ty);
        self.tdata[ti] |= tflag::GATHER_EDGE;
    }
    /// `World::set_behind` `0x006b4230`.
    pub fn set_behind(&mut self, tx: i32, ty: i32, on: bool, variant_b: bool) {
        let bit = if variant_b {
            tflag::BEHIND_B
        } else {
            tflag::BEHIND_A
        };
        self.set_tbit(tx, ty, bit, on)
    }

    #[inline]
    fn set_tbit(&mut self, tx: i32, ty: i32, bit: u16, on: bool) {
        let ti = self.t_index(tx, ty);
        if on {
            self.tdata[ti] |= bit;
        } else {
            self.tdata[ti] &= !bit;
        }
    }

    /// `World::set_land` `0x006b2b50` — W-indexed. Negative arguments mean "leave alone".
    /// `land_class` must be one of `0`, [`wflag::COAST`], [`wflag::ROCKS`],
    /// [`wflag::MOUNTAINS`], [`wflag::FOREST`]; setting `COAST` also sets
    /// [`wflag::ORIG_COAST`]. `clear_water` additionally clears `WATERHALF | ORIG_COAST`.
    pub fn set_land(
        &mut self,
        wx: i32,
        wy: i32,
        land: i32,
        land_sub: i32,
        land_class: i32,
        clear_water: bool,
    ) {
        let wi = self.w_index(wx, wy);
        if land_sub >= 0 {
            self.wdata[wi].land_sub = land_sub as u8;
        }
        if land >= 0 {
            self.wdata[wi].land = land as i8;
        }
        if land_class >= 0 {
            self.wdata[wi].flags &= !wflag::LAND_CLASS_MASK;
            if clear_water {
                self.wdata[wi].flags &= !(wflag::WATERHALF | wflag::ORIG_COAST);
            }
            self.wdata[wi].flags |= land_class as u16;
            if land_class as u16 == wflag::COAST {
                self.wdata[wi].flags |= wflag::ORIG_COAST;
            }
        }
    }

    /// `World::set_oil_at` `0x006b2a10` — sets the cell flag. The engine additionally
    /// spawns a `Good` of type `OIL` (5) at the cell centre `(wx*768 + 384, wy*768 + 384)`;
    /// that belongs to the objects lane, so this returns the centre rather than spawning.
    pub fn set_oil_at(&mut self, wx: i32, wy: i32, on: bool) -> (i32, i32) {
        let wi = self.w_index(wx, wy);
        if on {
            self.wdata[wi].flags |= wflag::OIL;
        } else {
            self.wdata[wi].flags &= !wflag::OIL;
        }
        (
            wx * COORD_PER_WCELL + COORD_PER_WCELL / 2,
            wy * COORD_PER_WCELL + COORD_PER_WCELL / 2,
        )
    }

    /// `World::set_down` `0x0046ef50`.
    pub fn set_down(&mut self, wx: i32, wy: i32, down: i16, down_who: i16) {
        let wi = self.w_index(wx, wy);
        self.wdata[wi].down = down;
        self.wdata[wi].down_who = down_who;
    }

    /// `World::new_coll_block` `0x0046d250`.
    pub fn new_coll_block(&mut self, wx: i32, wy: i32) -> &mut CollBlock {
        let wi = self.w_index(wx, wy);
        self.wdata[wi].block = Some(Box::new(CollBlock::default()));
        self.wdata[wi].block.as_deref_mut().unwrap()
    }

    // -- the world checksum channel ----------------------------------------------------------

    /// Reproduce `World::walk_data(CheckSum*, -1)` `0x006b5cf0` — the `world` channel of
    /// `CheckSums::check_all`.
    ///
    /// The engine runs each channel with a fresh adler-32 seeded to `1`. Section order and
    /// byte ranges are exactly as disassembled:
    ///
    /// | § | bytes walked |
    /// |---|---|
    /// | 1 | `xs`, `ys` — `walk(world+0, +8)` |
    /// | 2 | `start_x`, `start_y`, `start_city_x`, `start_city_y` — `SimpleArray<WCoord>::walk_data` |
    /// | 3 | `oil_x`, `oil_y` |
    /// | 4 | `walk(world+8, +0x80)` — 120 bytes, `size` … `seed` |
    /// | 5 | per W cell: `walk(wdata + 28*i, +0x15)` — 21 bytes |
    /// | 6 | `tdata` (`tile_size*2` B), then `seen`, `seen2`, `seen3` (`fog_size` B each) |
    /// | 7 | `wcoord_seen` (`size` B) |
    /// | 8 | `danger[p]` for `p in 0..8`, `reg_size*4` B each |
    /// | 9 | per W cell: an `i32` "has block", then the block's `[0,8)` and `[0xc, 0xc+size)` |
    /// | 10–13 | `Terrain::{halfland_locs, halfland_types, halfland_subtypes, nuke_hits}` |
    ///
    /// Note §12 in `check_all` is **conditional** on `world.wdata != NULL`
    /// (`[[0x00c06188]+0x134] != 0`), so an uninitialised world contributes nothing.
    pub fn checksum(&self) -> u32 {
        let mut a = Adler32::new();
        self.walk(&mut a);
        a.finish()
    }

    /// The whole channel — `World::walk_data(w, -1)`.
    pub fn walk<W: DataWalk>(&self, w: &mut W) {
        self.walk_section(w, WorldSection::ALL);
    }

    /// `World::walk_data(DataWalk* w, int section)` `0x006b5cf0`, **with its section
    /// argument**.
    ///
    /// The retail function is a chain of `if (section < 0 || section == N)` guards, one per
    /// section, with `check_all` passing `-1`. Modelling the argument rather than
    /// hard-coding "all" is what makes [`World::checksum_sections`] the *same code path* as
    /// the full walk instead of a parallel transcription of it — a per-section digest built
    /// from a second traversal can drift from the one that matters.
    pub fn walk_section<W: DataWalk>(&self, w: &mut W, section: i32) {
        let want = |n: i32| section < 0 || section == n;

        // §1
        if want(WorldSection::Dims as i32) {
            w.walk(&self.xs.to_le_bytes());
            w.walk(&self.ys.to_le_bytes());
        }
        // §2
        if want(WorldSection::StartArrays as i32) {
            walk_simple_array_i32(w, &self.start_x);
            walk_simple_array_i32(w, &self.start_y);
            walk_simple_array_i32(w, &self.start_city_x);
            walk_simple_array_i32(w, &self.start_city_y);
        }
        // §3
        if want(WorldSection::OilArrays as i32) {
            walk_simple_array_i32(w, &self.oil_x);
            walk_simple_array_i32(w, &self.oil_y);
        }
        if want(WorldSection::Scalars as i32) {
            self.walk_scalars(w);
        }
        if want(WorldSection::WData as i32) {
            // §5
            for cell in self.wdata.iter() {
                w.walk(&cell.checksum_bytes());
            }
        }
        if want(WorldSection::TDataAndFog as i32) {
            // §6
            for t in self.tdata.iter() {
                w.walk(&t.to_le_bytes());
            }
            w.walk(&self.seen);
            w.walk(&self.seen2);
            w.walk(&self.seen3);
        }
        if want(WorldSection::WCoordSeen as i32) {
            // §7
            w.walk(&self.wcoord_seen);
        }
        if want(WorldSection::Danger as i32) {
            // §8
            for plane in self.danger.iter() {
                for v in plane.iter() {
                    w.walk(&v.to_le_bytes());
                }
            }
        }
        if want(WorldSection::CollBlocks as i32) {
            // §9
            for cell in self.wdata.iter() {
                let has: i32 = if cell.block.is_some() { 1 } else { 0 };
                w.walk(&has.to_le_bytes());
                if let Some(b) = cell.block.as_deref() {
                    // `[block+0, +8)` then `[block+0xc, +0xc+size)`. `+8` (`flags`) is
                    // deliberately skipped by the walk.
                    w.walk(&b.bits.to_le_bytes());
                    w.walk(&b.size.to_le_bytes());
                    let n = (b.size.max(0) as usize).min(b.ptr.len());
                    w.walk(&b.ptr[..n]);
                }
            }
        }
        // §10–13
        if want(WorldSection::TerrainHalflandLocs as i32) {
            walk_array_pairs(w, &self.terrain_sync.halfland_locs);
        }
        if want(WorldSection::TerrainHalflandTypes as i32) {
            walk_simple_array_i32(w, &self.terrain_sync.halfland_types);
        }
        if want(WorldSection::TerrainHalflandSubtypes as i32) {
            walk_simple_array_i32(w, &self.terrain_sync.halfland_subtypes);
        }
        if want(WorldSection::TerrainNukeHits as i32) {
            walk_simple_array_i32(w, &self.terrain_sync.nuke_hits);
        }
    }

    /// Per-section digests of the `world` channel.
    ///
    /// Each entry is a **fresh adler seeded to 1** over that section alone, plus the byte
    /// count. The engine never computes these — `check_all` runs one accumulator across the
    /// whole channel — but a single 32-bit mismatch is not a debuggable statement, and this
    /// says *which* section diverged and after how many bytes. `full` is the value that has
    /// to match retail.
    ///
    /// Merged in from the `borders_fog` lane's `world_checksum`, which computed the same
    /// idea over a rival copy of the state; the rival is gone.
    pub fn checksum_sections(&self) -> WorldChecksum {
        let mut per_section = [SectionDigest::default(); WorldSection::COUNT];
        for (i, slot) in per_section.iter_mut().enumerate() {
            let mut a = Adler32::new();
            self.walk_section(&mut a, i as i32 + 1);
            *slot = SectionDigest {
                adler: a.finish(),
                bytes: a.bytes,
            };
        }
        let mut all = Adler32::new();
        self.walk(&mut all);
        WorldChecksum {
            per_section,
            full: all.finish(),
            bytes: all.bytes,
        }
    }

    /// The exact byte stream the channel hands the visitor — for locating the first
    /// differing offset against a captured retail walk.
    pub fn checksum_image(&self) -> ByteSink {
        let mut sink = ByteSink::new();
        self.walk(&mut sink);
        sink
    }

    /// §4 — `walk(world+8, world+0x80)`, one contiguous 120-byte range. Field order is the
    /// struct's, so it is load-bearing.
    fn walk_scalars<W: DataWalk>(&self, w: &mut W) {
        for v in [
            self.size,
            self.fog_xs,
            self.fog_ys,
            self.fog_size,
            self.tile_xs,
            self.tile_ys,
            self.tile_size,
            self.reg_xs,
            self.reg_ys,
            self.reg_size,
            self.map,
            self.sea_map,
            self.player_territory_limit,
            self.player_territory_limit_civic,
            self.player_territory_limit_city,
            self.colonized_territory_limit,
            self.colonized_territory_limit_civic,
            self.colonized_territory_limit_city,
            self.player_reg,
            self.resource_reg,
            self.forest_size,
            self.mountain_size,
            self.rock_size,
            self.total_metal,
            self.total_oil,
            self.goodies,
            self.land_resources,
            self.sea_resources,
            self.land_size,
            self.seed,
        ] {
            w.walk(&v.to_le_bytes());
        }
    }
}

/// The thirteen sections of `World::walk_data(DataWalk*, int)` `0x006b5cf0`, numbered as
/// the engine numbers them — the `int` argument is compared against these values directly.
///
/// [measured: `re/decomp-all/006b5cf0.c` is thirteen `if (sec < 0 || sec == N)` guards for
/// `N` in `1..=13`, cross-checked against `World` in `schema/state-schema.json`, whose 24
/// recovered ops line up one-for-one: op 1 is `[+0,+8)` on the global at `0x00c06188`,
/// ops 2–7 are the six `SimpleArray<WCoord>::walk_data` calls, op 8 is `[+8,+0x80)`,
/// ops 10–14 are the five plane pointers at `+0x138/+0x15c/+0x160/+0x164/+0x168`, and
/// ops 20–23 are the four `Terrain` arrays reached through `0x00c06218`.]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum WorldSection {
    /// `walk(world+0, +8)` — `xs`, `ys`.
    Dims = 1,
    /// The four `SimpleArray<WCoord>` start-position arrays.
    StartArrays = 2,
    /// The two oil-position arrays.
    OilArrays = 3,
    /// `walk(world+8, +0x80)` — 120 bytes: every derived size, the six territory limits,
    /// the resource totals and `seed`.
    Scalars = 4,
    /// `WData[size]`, bytes `[0, 0x15)` of each 28-byte record. **Territory ownership
    /// (`who`, `who2`) and the coarse explored bitmask (`was_seen`) ride here.**
    WData = 5,
    /// `TData[tile_size]` (2 B each), then `seen`, `seen2`, `seen3` (`fog_size` B each).
    /// **All three fog planes are in the sync checksum.**
    TDataAndFog = 6,
    /// `wcoord_seen[size]`.
    WCoordSeen = 7,
    /// `danger[8][reg_size]`, 4 B per entry.
    Danger = 8,
    /// The per-`WData` `CollBlock`: presence flag, then `[+0,+8)` and `[+0xc, +0xc+size)`.
    CollBlocks = 9,
    /// `Terrain::halfland_locs` — `Array<WCoordData>`, 8-byte elements.
    TerrainHalflandLocs = 10,
    /// `Terrain::halfland_types` — `SimpleArray<int>`.
    TerrainHalflandTypes = 11,
    /// `Terrain::halfland_subtypes` — `SimpleArray<int>`.
    TerrainHalflandSubtypes = 12,
    /// `Terrain::nuke_hits` — `SimpleArray<int>`.
    TerrainNukeHits = 13,
}

impl WorldSection {
    /// The value `check_all` passes: every section.
    pub const ALL: i32 = -1;
    /// How many sections there are.
    pub const COUNT: usize = 13;

    /// Section numbers `1..=13` in walk order.
    pub const fn all() -> [WorldSection; Self::COUNT] {
        use WorldSection::*;
        [
            Dims,
            StartArrays,
            OilArrays,
            Scalars,
            WData,
            TDataAndFog,
            WCoordSeen,
            Danger,
            CollBlocks,
            TerrainHalflandLocs,
            TerrainHalflandTypes,
            TerrainHalflandSubtypes,
            TerrainNukeHits,
        ]
    }
}

/// One section's isolated digest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SectionDigest {
    /// A fresh adler seeded to 1 over this section alone.
    pub adler: u32,
    /// Bytes the section handed the visitor.
    pub bytes: u64,
}

/// The `world` channel, broken out per section. See [`World::checksum_sections`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldChecksum {
    /// Indexed by `section - 1`.
    pub per_section: [SectionDigest; WorldSection::COUNT],
    /// The value channel 12 of `check_all` produces — one accumulator, all sections.
    pub full: u32,
    /// Total bytes the channel walks.
    pub bytes: u64,
}

impl WorldChecksum {
    /// This section's digest.
    #[inline]
    pub fn section(&self, s: WorldSection) -> SectionDigest {
        self.per_section[s as usize - 1]
    }

    /// The sections that differ from another world's, in walk order — the first entry is
    /// where a desync investigation starts.
    pub fn differing_sections(&self, other: &WorldChecksum) -> Vec<WorldSection> {
        WorldSection::all()
            .into_iter()
            .filter(|&s| self.section(s) != other.section(s))
            .collect()
    }
}

/// `SimpleArray<T>::walk_data` on the checksum path — see [`WalkedArray`].
fn walk_simple_array_i32<W: DataWalk>(w: &mut W, a: &WalkedArray<i32>) {
    let len = a.items.len() as i32;
    w.walk(&len.to_le_bytes());
    if len == 0 {
        return;
    }
    w.walk(&a.capacity.to_le_bytes());
    w.walk(&a.increment.to_le_bytes());
    w.walk(&[a.flags & !0x40]);
    for v in a.items.iter() {
        w.walk(&v.to_le_bytes());
    }
}

/// `Array<WCoordData>::walk_data` — identical header, 8-byte elements.
fn walk_array_pairs<W: DataWalk>(w: &mut W, a: &WalkedArray<(i32, i32)>) {
    let len = a.items.len() as i32;
    w.walk(&len.to_le_bytes());
    if len == 0 {
        return;
    }
    w.walk(&a.capacity.to_le_bytes());
    w.walk(&a.increment.to_le_bytes());
    w.walk(&[a.flags & !0x40]);
    for (x, y) in a.items.iter() {
        w.walk(&x.to_le_bytes());
        w.walk(&y.to_le_bytes());
    }
}

// ---------------------------------------------------------------------------------------
// 6. DataWalk / adler-32 — re-exported, never re-implemented
// ---------------------------------------------------------------------------------------

/// The checksum primitive and the `DataWalk` visitor live in [`crate::checksum`] and are
/// re-exported here so `map_terrain::{Adler32, DataWalk, adler32}` keep resolving.
///
/// This module used to carry its own `Adler32`. It does not any more: `adler32`
/// `0x00a46830` is the one arithmetic primitive beneath all fifteen channels, and it is
/// singular by construction now. See [`crate::checksum`] for the derivation and the
/// oracle lineage.
pub use crate::checksum::{adler32, Adler32, ByteSink, DataWalk};

// ---------------------------------------------------------------------------------------
// 7. Tests
// ---------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// `div_3_table[i] == floor(i/3)` on both sides of zero, as
    /// `init_coord_lookup_array` builds it.
    #[test]
    fn div_3_matches_floor() {
        for i in -400..400 {
            let expect = (i as f64 / 3.0).floor() as i32;
            assert_eq!(div_3(i), expect, "div_3({i})");
        }
    }

    /// The engine's shift-then-div-3 composition equals plain floor division by the cell
    /// size, for negative coordinates too.
    #[test]
    fn coord_conversions_are_floor_division() {
        for c in (-20_000..20_000).step_by(7) {
            let c = Coord(c);
            assert_eq!(UCoord::from_coord(c).0, floor_div(c.0, COORD_PER_UCELL));
            assert_eq!(TCoord::from_coord(c).0, floor_div(c.0, COORD_PER_TILE));
            assert_eq!(FCoord::from_coord(c).0, floor_div(c.0, COORD_PER_FCELL));
            assert_eq!(WCoord::from_coord(c).0, floor_div(c.0, COORD_PER_WCELL));
            assert_eq!(RCoord::from_coord(c).0, floor_div(c.0, COORD_PER_RCELL));
        }
    }

    /// The ladder is internally consistent: T→W by `>>2` agrees with Coord→W directly.
    #[test]
    fn ladder_is_consistent() {
        assert_eq!(COORD_PER_TILE, COORD_PER_UCELL * 4);
        assert_eq!(COORD_PER_WCELL, COORD_PER_TILE * TILES_PER_WCELL);
        assert_eq!(COORD_PER_FCELL * FCELLS_PER_WCELL, COORD_PER_WCELL);
        assert_eq!(COORD_PER_RCELL, COORD_PER_WCELL * WCELLS_PER_RCELL);
        for c in (-30_000..30_000).step_by(13) {
            let c = Coord(c);
            assert_eq!(TCoord::from_coord(c).to_wcoord(), WCoord::from_coord(c));
        }
    }

    /// `WCoord::operator TCoord` returns the *centre* tile, `get_tcorner` the corner, and
    /// `traverse_x/y` enumerate the cell's 4x4 tiles exactly once each.
    #[test]
    fn wcell_traversal_covers_sixteen_tiles() {
        let w = WCoord(3);
        assert_eq!(w.tcorner().0, 12);
        assert_eq!(w.to_tcoord_centre().0, 14);
        let mut seen = std::collections::HashSet::new();
        for i in 0..TILES_IN_WCELL {
            seen.insert((w.traverse_x(i).0, w.traverse_y(i).0));
        }
        assert_eq!(seen.len(), 16);
        for (x, y) in seen {
            assert!((12..16).contains(&x) && (12..16).contains(&y));
        }
    }

    /// `World::init` derived dimensions, exactly as the instruction stream computes them.
    #[test]
    fn world_init_dimensions() {
        let w = World::init_default_rules(80, 60);
        assert_eq!((w.xs, w.ys, w.size), (80, 60, 4800));
        assert_eq!((w.tile_xs, w.tile_ys, w.tile_size), (320, 240, 76800));
        assert_eq!((w.fog_xs, w.fog_ys, w.fog_size), (160, 120, 19200));
        assert_eq!((w.reg_xs, w.reg_ys, w.reg_size), (40, 30, 1200));
        assert_eq!(w.sea_map, -1);
        assert_eq!(w.player_territory_limit, 44);
        assert_eq!(w.colonized_territory_limit, 44);
        assert_eq!(w.player_territory_limit_civic, 4);
        assert_eq!(w.player_territory_limit_city, 4);
        assert_eq!(w.wdata.len(), 4800);
        assert_eq!(w.tdata.len(), 76800);
        assert_eq!(w.seen.len(), 19200);
        assert_eq!(w.wcoord_seen.len(), 4800);
        assert_eq!(w.danger[7].len(), 1200);
        // The whole map, measured in raw Coord units.
        assert_eq!(w.tile_xs * COORD_PER_TILE, w.xs * COORD_PER_WCELL);
    }

    #[test]
    fn start_city_bits_are_row_major_and_lsb_first() {
        let mut w = World::init_default_rules(9, 2);
        w.start_city_locs = vec![0; 3];
        // width=9, (8,1) => bit 17 => byte 2, mask 0x02.
        w.start_city_locs[2] = 0x02;
        assert!(w.start_city_wcoord(WCoord(8), WCoord(1)));
        assert!(!w.start_city_wcoord(WCoord(7), WCoord(1)));
    }

    /// `World::wipe` leaves every cell in the state the disassembly writes.
    #[test]
    fn wipe_defaults_match_the_disassembly() {
        let d = WData::default();
        assert_eq!(d.flags, 0);
        assert_eq!(d.land, 2);
        assert_eq!(d.land_sub, 0);
        assert_eq!(d.region, 64);
        assert_eq!(d.region2, 0);
        assert_eq!(d.down, -1);
        assert_eq!(d.down_who, 0);
        assert_eq!((d.who, d.who2), (-1, -1));
        assert_eq!((d.blocked, d.bad, d.solid, d.was_seen), (0, 0, 0, 0));
        // 21 checksummed bytes: 02 at land, 40 00 at region, ff ff at down, ff ff at who/who2.
        let b = d.checksum_bytes();
        assert_eq!(&b[0..8], &[0, 0, 2, 0, 0x40, 0, 0, 0]);
        assert_eq!(&b[8..12], &[0xff, 0xff, 0, 0]);
        assert_eq!(&b[15..17], &[0xff, 0xff]);
    }

    /// `offmap_world`'s 28 bytes, byte-for-byte from `0x00c899a0`.
    #[test]
    fn offmap_world_bytes() {
        let raw: [u8; 21] = [
            0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
            0x00, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
        ];
        assert_eq!(WData::offmap().checksum_bytes(), raw);
    }

    /// The blocker field is a 2-bit enum, not three independent bits.
    #[test]
    fn blocker_field_is_exclusive() {
        let mut w = World::init_default_rules(8, 8);
        w.set_cliff_at(4, 4);
        assert!(w.is_cliff_at(4, 4));
        assert!(!w.is_mountain_at(4, 4));
        assert!(w.is_blocked_at(4, 4, false));
        w.set_mountain_at(4, 4, true);
        assert!(w.is_mountain_at(4, 4));
        assert!(!w.is_cliff_at(4, 4));
        w.set_building_at(4, 4, true);
        assert!(w.is_built_at(4, 4));
        assert!(!w.is_mountain_at(4, 4));
        // Clearing a building only clears the field when the field *is* BUILDING.
        w.set_building_at(4, 4, false);
        assert_eq!(w.tmask(4, 4) & tflag::BLOCKER_MASK, 0);
    }

    /// The surface field is likewise exclusive: road, water and trees share bits 4-5.
    #[test]
    fn surface_field_is_exclusive() {
        let mut w = World::init_default_rules(8, 8);
        w.set_road_at(5, 5, true, 0, true);
        assert_eq!(w.tmask(5, 5) & tflag::SURFACE_MASK, tflag::SURFACE_ROAD);
        assert!(w.wdata(1, 1).flags & wflag::HAS_ROAD != 0);
        w.set_tocean(5, 5);
        assert!(w.is_tocean(5, 5));
        w.set_tree_at(5, 5, true);
        assert!(w.is_tree_at(5, 5));
        assert!(!w.is_tocean(5, 5));
        // Removing the last road tile clears the cell-level flag.
        w.set_road_at(5, 5, false, 0, true);
        assert!(w.wdata(1, 1).flags & wflag::HAS_ROAD == 0);
    }

    /// Blocking a tile paints BAD_PATH on the 8-ring and keeps `WData::bad` in step.
    #[test]
    fn blocked_propagates_bad_path_to_the_eight_ring() {
        let mut w = World::init_default_rules(8, 8);
        w.set_blocked_at(10, 10, true);
        for i in 0..8 {
            let (nx, ny) = (10 + NEIGHBOUR_DX[i], 10 + NEIGHBOUR_DY[i]);
            assert!(w.tmask(nx, ny) & tflag::BAD_PATH != 0, "neighbour {i}");
        }
        assert_eq!(w.tmask(10, 10) & tflag::BAD_PATH, 0);
        assert_eq!(w.wdata(2, 2).blocked, 1);
        // 8 neighbours of (10,10) all land in W cell (2,2) except none — (9..11, 9..11)
        // spans W cells (2,2) only for x,y in 8..11 -> (2,2) covers tiles 8..11.
        assert_eq!(w.wdata(2, 2).bad, 8);
        w.set_blocked_at(10, 10, false);
        assert_eq!(w.wdata(2, 2).blocked, 0);
        assert_eq!(w.wdata(2, 2).bad, 0);
    }

    /// River bits maintain the owning cell's HAS_RIVER exactly like the engine's rescan.
    #[test]
    fn river_cell_flag_tracks_its_sixteen_tiles() {
        let mut w = World::init_default_rules(8, 8);
        w.set_river_at(8, 8, true);
        w.set_river_at(9, 8, true);
        assert!(w.wdata(2, 2).flags & wflag::HAS_RIVER != 0);
        w.set_river_at(8, 8, false);
        assert!(
            w.wdata(2, 2).flags & wflag::HAS_RIVER != 0,
            "one river tile left"
        );
        w.set_river_at(9, 8, false);
        assert!(w.wdata(2, 2).flags & wflag::HAS_RIVER == 0);
    }

    /// `get_tregion` reads `region2` only for water tiles of a WATERHALF cell.
    #[test]
    fn tregion_splits_waterhalf_cells() {
        let mut w = World::init_default_rules(8, 8);
        w.wdata_mut(1, 1).region = 7;
        w.wdata_mut(1, 1).region2 = 9;
        assert_eq!(w.get_tregion(4, 4), 7);
        w.set_waterhalf(4, 4, true);
        assert_eq!(w.get_tregion(4, 4), 9);
        assert_eq!(w.get_tregion(5, 4), 7);
        assert_eq!(w.num_waterhalf(1, 1), 1);
    }

    /// `set_oil_at` reports the Coord the engine spawns the oil `Good` at.
    #[test]
    fn oil_good_lands_at_the_world_cell_centre() {
        let mut w = World::init_default_rules(8, 8);
        assert_eq!(w.set_oil_at(3, 5, true), (3 * 768 + 384, 5 * 768 + 384));
        assert!(w.is_oil_at(3, 5));
    }

    /// adler-32 against the zlib definition on a known vector.
    #[test]
    fn adler32_known_vector() {
        let mut a = Adler32::new();
        a.walk(b"Wikipedia");
        assert_eq!(a.finish(), 0x11E60398);
    }

    /// The world channel is order- and content-sensitive, and stable.
    #[test]
    fn world_checksum_is_deterministic_and_sensitive() {
        let a = World::init_default_rules(24, 24);
        let b = World::init_default_rules(24, 24);
        assert_eq!(a.checksum(), b.checksum());

        let mut c = World::init_default_rules(24, 24);
        c.set_cliff_at(10, 10);
        assert_ne!(a.checksum(), c.checksum());

        // A change confined to the 3 padding bytes / the CollBlock pointer must not move
        // the checksum, but allocating the block itself must (section 9 walks a presence
        // flag per cell).
        let mut d = World::init_default_rules(24, 24);
        d.new_coll_block(0, 0);
        assert_ne!(a.checksum(), d.checksum());
    }

    /// Byte accounting for the walk, so a regression in section order is caught by size
    /// as well as by hash.
    #[test]
    fn world_channel_byte_count() {
        let w = World::init_default_rules(16, 16);
        let mut a = Adler32::new();
        w.walk(&mut a);
        let size = 16 * 16i64; // 256 W cells
        let tile_size = 64 * 64i64;
        let fog_size = 32 * 32i64;
        let reg_size = 8 * 8i64;
        let expect = 8                    // §1 xs, ys
            + 6 * 4                       // §2/§3 six empty SimpleArrays, length word only
            + 120                         // §4
            + size * 21                   // §5
            + tile_size * 2               // §6 tdata
            + fog_size * 3                // §6 seen/seen2/seen3
            + size                        // §7 wcoord_seen
            + reg_size * 4 * 8            // §8 danger
            + size * 4                    // §9 presence flags, no blocks allocated
            + 4 * 4; // §10-13 four empty arrays
        assert_eq!(a.bytes as i64, expect);
    }

    /// The section-parameterised walk must be the *same* traversal as the whole-channel
    /// walk: concatenating sections 1..=13 in order has to give byte-for-byte the stream
    /// `walk_data(w, -1)` produces. This is what makes the per-section digest evidence
    /// about the real channel rather than a second, drifting transcription.
    #[test]
    fn the_thirteen_sections_concatenate_to_the_whole_channel() {
        let mut w = World::init_default_rules(12, 12);
        // Give every section something to say, so no section is vacuously equal.
        w.start_x.items.push(3);
        w.oil_y.items.push(7);
        w.wdata[5].who = 2;
        w.tdata[9] = 0x1234;
        w.seen[3] = 0b101;
        w.wcoord_seen[4] = 1;
        w.danger[6][2] = -9;
        w.new_coll_block(1, 1);
        w.terrain_sync.halfland_locs.items.push((4, 5));
        w.terrain_sync.nuke_hits.items.push(11);

        let whole = w.checksum_image();
        let mut pieces = ByteSink::new();
        let mut counted = 0u64;
        for s in WorldSection::all() {
            let mut one = ByteSink::new();
            w.walk_section(&mut one, s as i32);
            counted += one.0.len() as u64;
            pieces.walk(&one.0);
        }
        assert_eq!(
            whole.first_difference(&pieces),
            None,
            "section walk diverges from the whole-channel walk"
        );
        assert_eq!(counted, whole.0.len() as u64);

        // …and the digest agrees with the walk it claims to summarise.
        let sec = w.checksum_sections();
        assert_eq!(sec.full, w.checksum());
        assert_eq!(sec.full, whole.checksum());
        assert_eq!(sec.bytes, whole.0.len() as u64);
        for s in WorldSection::all() {
            let mut one = ByteSink::new();
            w.walk_section(&mut one, s as i32);
            assert_eq!(sec.section(s).adler, one.checksum(), "{s:?}");
            assert_eq!(sec.section(s).bytes, one.0.len() as u64, "{s:?}");
        }
    }

    /// Each section must be non-empty for a populated world — a section that silently walks
    /// nothing would make its digest a constant and hide every divergence inside it.
    #[test]
    fn no_section_is_silently_empty() {
        let mut w = World::init_default_rules(8, 8);
        w.start_x.items.push(1);
        w.start_y.items.push(1);
        w.start_city_x.items.push(1);
        w.start_city_y.items.push(1);
        w.oil_x.items.push(1);
        w.oil_y.items.push(1);
        w.new_coll_block(0, 0);
        w.terrain_sync.halfland_locs.items.push((1, 1));
        w.terrain_sync.halfland_types.items.push(1);
        w.terrain_sync.halfland_subtypes.items.push(1);
        w.terrain_sync.nuke_hits.items.push(1);
        let sec = w.checksum_sections();
        for s in WorldSection::all() {
            assert!(sec.section(s).bytes > 0, "section {s:?} walks nothing");
        }
    }

    /// `World::clear_seen` `0x006b2250` ends in **two** memsets — `wcoord_seen` (section 7)
    /// and `seen` (section 6) — and leaves `seen2` alone. Getting that wrong desyncs on the
    /// second frame of any game, which is why it is pinned here.
    #[test]
    fn clear_seen_clears_both_planes_and_spares_the_explored_one() {
        let mut w = World::init_default_rules(8, 8);
        w.seen[3] = 0xFF;
        w.seen2[3] = 0xFF;
        w.seen3[3] = 0xFF;
        w.wcoord_seen[1] = 0xFF;
        w.clear_seen();
        assert_eq!(w.seen[3], 0);
        assert_eq!(w.wcoord_seen[1], 0);
        assert_eq!(w.seen2[3], 0xFF, "explored accumulates forever");
        assert_eq!(w.seen3[3], 0xFF, "detected is cleared by update_all_seen");
    }

    /// Seeding: a negative seed is ignored, a non-negative seed is installed.
    #[test]
    fn map_seed_install() {
        let mut w = World::init_default_rules(8, 8);
        assert_eq!(w.seed_map_generation(-1), None);
        assert_eq!(w.seed, 0);
        assert_eq!(w.seed_map_generation(0x1234_5678), Some(0x1234_5678));
        assert_eq!(w.seed, 0x1234_5678);
    }

    /// Passability predicates use the exact masks the engine uses.
    #[test]
    fn passability_masks() {
        let mut w = World::init_default_rules(8, 8);
        w.wdata_mut(2, 2).land = land::FERTILE;
        assert!(w.is_passable(2, 2));
        assert!(w.buildings_allowed(2, 2));
        assert!(w.is_flat(2, 2));
        w.wdata_mut(2, 2).flags |= wflag::ROCKS;
        assert!(
            w.is_passable(2, 2),
            "rocks are passable (0x08 is outside 0x70)"
        );
        assert!(
            !w.buildings_allowed(2, 2),
            "but not buildable (0x08 is inside 0x78)"
        );
        assert!(!w.is_flat(2, 2));
        w.wdata_mut(2, 2).flags |= wflag::MOUNTAINS;
        assert!(!w.is_passable(2, 2));
        assert_eq!(
            w.get_land(2, 2, 1),
            5,
            "mountains outrank rocks in get_land"
        );
    }

    /// The 16 `space_at_corner` probes tile a 4x4 block exactly, with 0-3 as the core.
    #[test]
    fn space_probes_tile_a_4x4_block() {
        let mut seen = std::collections::HashSet::new();
        for i in 0..16 {
            seen.insert((SPACE_PROBE_DX[i], SPACE_PROBE_DY[i]));
        }
        assert_eq!(seen.len(), 16);
        for i in 0..4 {
            assert!((1..=2).contains(&SPACE_PROBE_DX[i]), "core probe {i} dx");
            assert!((1..=2).contains(&SPACE_PROBE_DY[i]), "core probe {i} dy");
        }
        // Every approach group is 5 distinct ring probes (never a core probe).
        for g in SPACE_APPROACH_GROUPS.iter() {
            let s: std::collections::HashSet<_> = g.iter().collect();
            assert_eq!(s.len(), 5);
            assert!(g.iter().all(|&i| i >= 4));
        }
    }

    /// The placement grade the production lane consumes, over the whole range.
    #[test]
    fn space_at_corner_grades() {
        let mut w = World::init_default_rules(8, 8);
        // Empty world, well inside bounds: everything clear.
        assert_eq!(w.space_at_corner(10, 10, 0, false), space::FULLY_CLEAR);

        // Block one ring tile -> no longer fully clear, but an approach L survives.
        let mut w2 = World::init_default_rules(8, 8);
        w2.set_blocked_at(10 + SPACE_PROBE_DX[7], 10 + SPACE_PROBE_DY[7], true);
        let g = w2.space_at_corner(10, 10, 0, false);
        assert!(g == space::APPROACH_CLEAR || g == space::PARTIAL);
        assert_ne!(g, space::FULLY_CLEAR);

        // Block a core tile -> immediate reject regardless of the ring.
        w.set_blocked_at(10 + SPACE_PROBE_DX[0], 10 + SPACE_PROBE_DY[0], true);
        assert_eq!(w.space_at_corner(10, 10, 0, false), space::CORE_BLOCKED);

        // Foreign territory rejects the same way.
        let mut w3 = World::init_default_rules(8, 8);
        w3.wdata_mut(2, 2).who = 3;
        assert_eq!(w3.space_at_corner(10, 10, 0, false), space::CORE_BLOCKED);
        assert_eq!(w3.space_at_corner(10, 10, 3, false), space::FULLY_CLEAR);

        // Out of bounds counts as blocked: anchoring at the map edge rejects.
        assert_eq!(w3.space_at_corner(31, 31, 3, false), space::CORE_BLOCKED);
    }

    /// `check_building_wcoord`'s three early rejects.
    #[test]
    fn check_building_wcoord_gates() {
        let mut w = World::init_default_rules(8, 8);
        assert_eq!(
            w.check_building_wcoord(3, 3, 0, 1, 1, 2, false),
            space::FULLY_CLEAR
        );
        w.wdata_mut(3, 3).flags |= wflag::MOUNTAINS;
        assert_eq!(
            w.check_building_wcoord(3, 3, 0, 1, 1, 2, false),
            space::CORE_BLOCKED
        );
        w.wdata_mut(3, 3).flags &= !wflag::MOUNTAINS;
        w.wdata_mut(3, 3).blocked = 16;
        assert_eq!(
            w.check_building_wcoord(3, 3, 0, 1, 1, 2, false),
            space::CORE_BLOCKED
        );
        w.wdata_mut(3, 3).blocked = 0;
        w.wdata_mut(3, 3).who = 5;
        assert_eq!(
            w.check_building_wcoord(3, 3, 0, 1, 1, 2, false),
            space::CORE_BLOCKED
        );
    }

    /// Gather access needs a harvestable tile and a dry, unblocked orthogonal neighbour.
    #[test]
    fn gather_access_needs_a_dry_neighbour() {
        let mut w = World::init_default_rules(8, 8);
        assert!(
            !w.has_gather_access(10, 10),
            "plain ground is not harvestable"
        );
        w.set_tree_at(10, 10, true);
        assert!(w.has_gather_access(10, 10));
        for i in 0..4 {
            w.set_tocean(10 + NEIGHBOUR4_DX[i], 10 + NEIGHBOUR4_DY[i]);
        }
        assert!(!w.has_gather_access(10, 10), "ringed by water");
        w.set_blocked_at(10 + NEIGHBOUR4_DX[0], 10 + NEIGHBOUR4_DY[0], true);
        assert!(!w.has_gather_access(10, 10));
    }

    /// `is_ocean` is false on a WATERHALF cell even when `land` says water.
    #[test]
    fn waterhalf_is_not_ocean() {
        let mut w = World::init_default_rules(8, 8);
        w.wdata_mut(1, 1).land = land::OCEAN;
        assert!(w.is_ocean(1, 1));
        w.wdata_mut(1, 1).flags |= wflag::WATERHALF;
        assert!(!w.is_ocean(1, 1));
        assert!(w.is_pass_land(1, 1));
    }
}
