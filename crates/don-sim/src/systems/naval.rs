//! `systems::naval` — water, ships, docks, transports, naval combat and fish.
//!
//! Ported from `ron-bin/riseofnations.exe` (PE32 i386, image base `0x00400000`) with the
//! shipped private PDB `ron-bin/sbl/rise.pdb` supplying names, offsets and signatures, and
//! from the shipped rule data under `ron-data/`. Every claim in this header is `[measured]`
//! on this Mac against the instruction stream, the PDB type stream, or a shipped data file,
//! unless it says `STRUCTURE-ONLY` — which marks control flow taken from Ghidra output in
//! `re/decomp-all/` that has **not** been checked against behaviour.
//!
//! Nothing here is verified in the proof-assistant sense and **no differential test against
//! retail has been run for this lane**, so the strongest fidelity tier is **C (divergence
//! unmeasured)**. The module also contains explicitly named `*_proxy` helpers that are not
//! complete retail ports and must not be used for checksum claims. Where a value came from a
//! shipped XML file rather than the binary, the doc comment says so. The integration boundary
//! and missing systems are tracked in `docs/mechanics/naval.md`.
//!
//! # What this lane covers, and the call graph it was recovered from
//!
//! ```text
//! water terrain      WorldData::is_ocean       0x006B4830   (WCoord)
//!                    WorldData::is_tocean      0x0046FB10   (TCoord)
//!                    WorldData::is_tocean_slow 0x006B2400
//!                    WorldData::is_coast       0x006B3020
//!                    WorldData::num_waterhalf  0x006B4DB0
//!                    World::set_tocean         0x006B1C60 / set_waterhalf 0x006B1C90
//!                    World::clear_waterhalf    0x006B1BD0 / set_coastal   0x006B1D10
//! ship pathing       PathFinder::find_wpath    0x00688FC0 -> astar_path 0x00683770 step 768
//!                    PathFinder::valid_wcoord  0x00687DA0 -> UnitData::invalid_loc 0x00607C30
//! docks              BuildTypeData::is_dock_tile 0x00636700   <- THE SHORE CONSTRAINT
//!                    Dock::init  0x00740A80 · Docks::init_dock 0x00740FC0
//!                    Docks::close_dock 0x00740F50 · Dock::close 0x007409F0
//! transports         UnitData::can_transport      0x0046F960
//!                    UnitData::can_ever_transport 0x0046F290
//!                    UnitData::transport_type     0x0046F790
//!                    UnitData::needs_transport    0x00609920
//!                    LeaderData::can_transport    0x006E0C60
//!                    ObjectData::in_a_ship        0x006440C0
//!                    Unit::check_meet_ship        0x00604550  <- the rendezvous
//!                    Unit::do_board 0x005ED1F0 · Unit::do_await_board 0x005ED040
//!                    Group::action_set_transport  0x007024B0
//! naval combat       ObjectData::get_damage       0x00644130  (domain terms only; the
//!                    full chain belongs to the combat lane)
//! fish               Unit::think_fish             0x005F4C60
//!                    GoodTypeData::is_herd_type   0x004761E0
//! ```
//!
//! # Checksum channels this module's state belongs to
//!
//! * **`check_units`** (channel 1) — ships are `Unit`s; `UnitData::unit_masks`,
//!   `unit_masks2` and the `Stack<PathData>` produced by retail `find_wpath` are all inside
//!   `Unit::walk_data` `0x0060CF40`.
//! * **`World::walk_data`** (channel 12) — `WData` and `TData` carry every water bit here.
//! * **Docks are NOT one of the fifteen `check_all` channels.** `CheckSums::check_docks`
//!   `0x00936DA0` exists and is 15 bytes (a tail jump, i.e. non-empty), but it does not
//!   appear in `CheckSums::check_all` `0x00936560`'s ordered call list. `Docks::walk_data`
//!   `0x00741100` is still reached through save/load, so dock state is save-game state
//!   without being a live desync channel. Do not "fix" this by adding a 16th channel.
//!
//! # The five headline findings
//!
//! 1. **`div_3_table` is real and named.** `?div_3_table@@3PAHA` at `0x00CAE5FC` is an
//!    `int*` the engine uses instead of dividing: `tile = div_3_table[world >> 6]`
//!    (= world/192), `wcell = div_3_table[world >> 8]` (= world/768), `ugrid =
//!    div_3_table[world >> 4]` (= world/48). Every coordinate conversion in the naval cone
//!    goes through it. See [`tile_of`], [`wcell_of`], [`ugrid_of`].
//! 2. **`ObjectTypeData::domain` is at `+0x218`**, an `int` holding `DomainIndex`, and the
//!    enum has an alias the name list hides: `GROUND=0, SEA=1, BOTH=2, AIR=2, NUM_DOMAIN=3,
//!    REAL_AIR=4`. `BOTH` and `AIR` are **the same value**. See [`Domain`].
//! 3. **The dock shore constraint is a WCoord-scale predicate, not a tile one.** A dock is
//!    4x4 tiles = exactly one WCoord cell (`ron-data/buildingrules.xml`: Dock `X_SIZE 4
//!    Y_SIZE 4`). `is_dock_tile` requires the cell itself to be ocean and at least one of
//!    the four orthogonal neighbours to be shore with a clear 4x2 tile apron. See
//!    [`is_dock_tile`].
//! 4. **`move_x`/`move_y` are a 289-entry spiral, not an 8-entry neighbour table.**
//!    `docs/derivation/architecture.md` §6.1 records them as `{0,-1,0,1,1,1,0,-1,-1}`
//!    "8-connected plus a null entry". That is only ring 0 and ring 1. The real tables at
//!    The 289-entry prefix used by fish is frozen from `0x00ADCAF0`/`0x00ADC400` — mostly
//!    rings 0..8, plus retail's outlying final `(-8,-16)` entry. `Unit::check_meet_ship`
//!    scans indices `0..0x50` (rings 0..4) while `Unit::think_fish` scans `0..=0x120`.
//!    The A* uses only `[1..=8]`. See
//!    [`SPIRAL_X`].
//! 5. **A GROUND attacker gets no floor-of-1 against a SEA target.** `get_damage`'s
//!    conditional floor is `if (damage < 1) { if ((attacker.domain != GROUND ||
//!    target.domain != SEA) && ...) damage = 1; }` — so a land unit whose damage is fully
//!    absorbed by ship armour deals **zero**, not one. `ron-data/unitrules.xml`'s own
//!    `ARMOR` comment names this case ("rifleman vs. battleship"). See
//!    [`ground_vs_sea_skips_damage_floor`].
//!
//! # Territory and supply over water — the honest answer
//!
//! Territory is stored **per WCoord cell**: `World::compute_reg_territory` `0x006B0BB0`
//! writes `WData::who` (`+0x0F`) and `WData::who2` (`+0x10`), and `WorldData::is_enemy_territory`
//! `0x006B2490` reads `WData::who`. Ocean cells are ordinary `WData` records, so **an ocean
//! cell can carry an owner exactly like a land cell** — there is no water exemption in the
//! storage. What this module does *not* claim, because it was not read, is whether
//! `compute_reg_territory` applies a different radius or falloff over water; that function
//! is 4,039 bytes and only its writeback was read. It is listed in the report's open items.
//!
//! What *is* settled here is the naval reachability index: `LeaderData::reg_docks[64]`
//! (`+0x135E`, `unsigned short[64]`) counts each player's docks **per region**, maintained
//! by `Dock::init`/`Dock::close`, alongside `reg_naval[63]` (`+0x0EE2`) and
//! `reg_transports[63]` (`+0x0F60`). See [`LeaderNaval`].

#![allow(clippy::too_many_arguments)]

use crate::container::EngineArray;
use crate::rng::Random;

// ---------------------------------------------------------------------------
// 1. The coordinate ladder and `div_3_table`
// ---------------------------------------------------------------------------

/// World units per tile (`TCoord`). Everything in the sim is denominated in these.
pub const TILE: i32 = 192;
/// World units per unit-movement-grid cell. `find_upath` runs A\* on this grid.
pub const UGRID: i32 = 48;
/// Tiles per `WCoord` cell.
pub const WCELL_TILES: i32 = 4;
/// World units per `WCoord` cell — the grid the **water** A\* runs on.
///
/// [measured] `PathFinder::find_wpath` `0x00688FC0` pushes `0x300` as `astar_path`'s step
/// argument at `0x0068973D`, and `astar_path` selects `valid_wcoord` on `cmp ecx, 0x300`.
pub const WCELL: i32 = TILE * WCELL_TILES; // 768
/// Tiles per fog cell.
pub const FOG_TILES: i32 = 2;
/// Tiles per region cell.
pub const REGION_TILES: i32 = 8;

/// Object `Coord`s are XOR-obfuscated in memory with this mask; `GuyData` `Coord`s are not.
pub const COORD_XOR: i32 = 0x0006_3637;

/// `div_3_table[n] == n / 3`.
///
/// [measured] the symbol is `?div_3_table@@3PAHA` at `0x00CAE5FC` (an `int*` in `.data`,
/// filled at init). The *contents* are an inference, but a tightly constrained one: the
/// engine reads it as `div_3_table[x >> 6]` where the result is used as a `TCoord`
/// (x/192 = (x/64)/3) and as `div_3_table[x >> 8]` where the result is used as a `WCoord`
/// (x/768 = (x/256)/3). Both identities hold only for `t[n] = n/3`.
///
/// Only non-negative indices are defined; the engine never divides a negative world
/// coordinate through the table (callers bounds-check first). This function therefore
/// truncates toward zero for negatives, matching C's `/`, and callers must range-check.
#[inline]
pub fn div3(n: i32) -> i32 {
    n / 3
}

/// World units -> `TCoord`. `div_3_table[world >> 6]`.
#[inline]
pub fn tile_of(world: i32) -> i32 {
    div3(world >> 6)
}

/// World units -> `WCoord`. `div_3_table[world >> 8]`.
#[inline]
pub fn wcell_of(world: i32) -> i32 {
    div3(world >> 8)
}

/// World units -> unit-movement-grid cell. `div_3_table[world >> 4]`.
///
/// [measured] `Unit::check_meet_ship` `0x00604550` snaps the rendezvous with
/// `div_3_table[v >> 4] * 0x30 + 0x18`.
#[inline]
pub fn ugrid_of(world: i32) -> i32 {
    div3(world >> 4)
}

/// Centre of a unit-grid cell in world units: `c * 48 + 24` [measured, `0x00604550`].
#[inline]
pub fn ugrid_center(cell: i32) -> i32 {
    cell * UGRID + UGRID / 2
}

/// Centre of a `WCoord` cell in world units: `c * 768 + 384` [measured, `0x00688FC0`
/// builds `PathData` waypoints as `w * 0x300 + 0x180`].
#[inline]
pub fn wcell_center(cell: i32) -> i32 {
    cell * WCELL + WCELL / 2
}

/// De-obfuscate an object `Coord` read out of memory.
#[inline]
pub fn deobfuscate(raw: i32) -> i32 {
    raw ^ COORD_XOR
}

// ---------------------------------------------------------------------------
// 2. The spiral offset tables
// ---------------------------------------------------------------------------

/// `?move_x@@3QBHB` at `0x00ADCAF0`, all 289 entries read directly from the PE [measured].
///
/// This is a **square-spiral** offset table, not the 8-neighbour table
/// `docs/derivation/architecture.md` §6.1 describes. Index 0 is the centre; `1..=8` is
/// ring 1 (what `astar_path` uses); `9..=24` ring 2; `25..=48` ring 3; `49..=80` ring 4;
/// `81..=120` ring 5; rings continue through index 288. `Unit::check_meet_ship` scans
/// `0..0x51` and `Unit::think_fish` scans `0..=0x120`.
pub const SPIRAL_X: [i32; 289] = [
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
    -8, -8, -8, -8, -8, -8, -8, -8, -8, -8, -8, -8, -8, -8,
];

/// `?move_y@@3QBHB` at `0x00ADC400`, all 289 entries read directly from the PE [measured].
///
/// Entry 288 is `(x=-8, y=-16)`, not the `(-8, -7)` a generated ring would produce.
/// That outlier is present in the shipped executable and is intentionally preserved here.
pub const SPIRAL_Y: [i32; 289] = [
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
    7, 6, 5, 4, 3, 2, 1, 0, -1, -2, -3, -4, -5, -6, -16,
];

/// The 8-connected neighbourhood the A\* engines actually relax — `SPIRAL_[XY][1..=8]`.
pub const A_STAR_NEIGHBOURS: [(i32, i32); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
];

/// `?orthog_x@@3QBHB` at `0x00ADD250` [measured]. Index 0 is a null entry; the engine
/// reads `orthog_x[1..=4]` (`&DAT_00add254`), the four cardinal directions.
pub const ORTHOG_X: [i32; 5] = [0, 0, 1, 0, -1];
/// `?orthog_y@@3QBHB` at `0x00ADD210` [measured].
pub const ORTHOG_Y: [i32; 5] = [0, -1, 0, 1, 0];

/// `?corner_box_x@@3QBHB` / `?corner_box_y@@3QBHB` at `0x00ADD2A0` / `0x00ADD2B0`
/// [measured] — the four corners of a unit box.
pub const CORNER_BOX: [(i32, i32); 4] = [(0, 0), (0, 1), (1, 0), (1, 1)];

// ---------------------------------------------------------------------------
// 3. Tile and cell bit layouts
// ---------------------------------------------------------------------------

/// `TData` is exactly `{ unsigned short mask; }` — 2 bytes per tile [measured, PDB type
/// stream; `WorldData::tdata` is `TData*` at `WorldData+0x138`, indexed
/// `y * WorldData::tile_xs + x`].
///
/// The bit layout below was recovered by reading every small `WorldData::is_*` accessor
/// and every `World::set_*` mutator, not from a flags enum — the engine has no `TMask`
/// enum in the PDB.
pub mod tmask {
    /// Bits 0..1, the "feature" field. `World::set_building_at` `0x006B45C0` writes 3.
    pub const FEATURE: u16 = 0x0003;
    /// `WorldData::is_cliff_at` `0x0046F8C0`: `(mask & 3) == 1`.
    pub const FEATURE_CLIFF: u16 = 1;
    /// `WorldData::is_mountain_at` `0x0046F900`: `(mask & 3) == 2`.
    pub const FEATURE_MOUNTAIN: u16 = 2;
    /// `World::set_building_at` `0x006B45C0` sets, and `WorldData::is_built_at`
    /// `0x0046F880` tests, `(mask & 3) == 3`.
    pub const FEATURE_BUILDING: u16 = 3;

    /// Bits 4..5, the "cover" field.
    pub const COVER: u16 = 0x0030;
    /// `WorldData::is_tocean` `0x0046FB10`: `(mask & 0x30) == 0x20`.
    pub const COVER_OCEAN: u16 = 0x0020;
    /// `WorldData::is_tree_at` `0x0046F930`: `(mask & 0x30) == 0x30`.
    pub const COVER_TREE: u16 = 0x0030;

    /// The other half of `WorldData::is_built_at` `0x0046F880`:
    /// `((mask & 3) == 3) || (mask & 0x80)`.
    pub const BUILT: u16 = 0x0080;
    /// `World::set_coastal` `0x006B1D10`.
    pub const COASTAL: u16 = 0x0400;
    /// `WorldData::is_river` `0x0046D390`.
    pub const RIVER: u16 = 0x0800;
    /// `WorldData::is_gathered_from` `0x00472AC0`.
    pub const GATHERED: u16 = 0x1000;
    /// `WorldData::is_blocked_at` `0x00461340`.
    pub const BLOCKED: u16 = 0x4000;
}

/// `WData::flags` bits — these DO come from a PDB enum, the unnamed `FLAG_GOODY` block
/// [measured, `schema/types.json` enums].
pub mod wflag {
    pub const RESOURCE: u16 = 0x0001;
    /// The PDB gives two names at value 2: `FLAG_RIVER_VALLEY` and `FLAG_DEAD_BUILD`.
    pub const RIVER_VALLEY: u16 = 0x0002;
    pub const COAST: u16 = 0x0004;
    pub const ROCK: u16 = 0x0008;
    pub const MOUNTAIN: u16 = 0x0010;
    pub const FOREST: u16 = 0x0020;
    pub const CLIFF: u16 = 0x0040;
    pub const ROAD: u16 = 0x0080;
    /// Set on a `WCoord` cell that contains **both** land and water tiles.
    /// `World::set_waterhalf` `0x006B1C90` sets it; `World::clear_waterhalf` `0x006B1BD0`
    /// clears it and reverts the 16 tiles.
    pub const HALFLAND: u16 = 0x0100;
    pub const NEAR_BLOCKING: u16 = 0x0200;
    pub const ORIGINAL_COAST: u16 = 0x0400;
    pub const OIL: u16 = 0x0800;
    pub const RIVER: u16 = 0x1000;
    /// Two names at value 0x2000: `FLAG_RIVERBED` and `FLAG_TEST_FOG`.
    pub const RIVERBED: u16 = 0x2000;
    pub const BUILDING: u16 = 0x4000;
    pub const GOODY: u16 = 0x8000;
}

/// `WData::land` values that mean water.
///
/// [measured] `WorldData::is_ocean` `0x006B4830` is
/// `!(flags & HALFLAND) && (land == 1 || land == 2)`. The engine has no name for these
/// two codes in the PDB; only the predicate is recovered.
pub const LAND_WATER_CODES: [i8; 2] = [1, 2];

// ---------------------------------------------------------------------------
// 4. The water world
// ---------------------------------------------------------------------------

/// One `WData` record — the 28-byte per-`WCoord` cell [measured, PDB type stream:
/// `flags@0 land@2 land_sub@3 region@4 region2@6 down@8 down_who@0xA val@0xC goods@0xD
/// light@0xE who@0xF who2@0x10 blocked@0x11 bad@0x12 solid@0x13 was_seen@0x14 block@0x18`].
///
/// Only the fields the naval lane reads are carried here; the full record belongs to the
/// map/terrain lane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WCell {
    /// `WData::flags` `+0x00`.
    pub flags: u16,
    /// `WData::land` `+0x02`.
    pub land: i8,
    /// `WData::land_sub` `+0x03`.
    pub land_sub: u8,
    /// `WData::region` `+0x04`.
    pub region: i16,
    /// `WData::down` `+0x08` — the object standing on this cell, or -1.
    pub down: i16,
    /// `WData::down_who` `+0x0A`.
    pub down_who: i16,
    /// `WData::who` `+0x0F` — territory owner, -1 for unowned.
    pub who: i8,
    /// `WData::who2` `+0x10`.
    pub who2: i8,
}

/// The water-relevant slice of `World`: the `WData` grid and the `TData` grid.
///
/// `WorldData` itself is 364 bytes with `xs@0 ys@4` (WCoord dims), `tile_xs@0x18
/// tile_ys@0x1C`, `wdata@0x134`, `tdata@0x138` [measured, PDB]. `tile_xs == xs * 4`
/// always, because a `WCoord` cell is 4 tiles.
#[derive(Clone, Debug)]
pub struct WaterWorld {
    /// `WorldData::xs` — width in `WCoord` cells.
    pub xs: i32,
    /// `WorldData::ys` — height in `WCoord` cells.
    pub ys: i32,
    /// `WorldData::wdata`, row-major `y * xs + x`.
    pub wdata: Vec<WCell>,
    /// `WorldData::tdata`, row-major `y * tile_xs + x`.
    pub tdata: Vec<u16>,
}

impl WaterWorld {
    /// An all-land world of `xs * ys` `WCoord` cells.
    pub fn new(xs: i32, ys: i32) -> WaterWorld {
        assert!(xs > 0 && ys > 0);
        WaterWorld {
            xs,
            ys,
            wdata: vec![
                WCell {
                    who: -1,
                    who2: -1,
                    down: -1,
                    down_who: -1,
                    ..WCell::default()
                };
                (xs * ys) as usize
            ],
            tdata: vec![0u16; (xs * ys * WCELL_TILES * WCELL_TILES) as usize],
        }
    }

    /// `WorldData::tile_xs` `+0x18`.
    #[inline]
    pub fn tile_xs(&self) -> i32 {
        self.xs * WCELL_TILES
    }
    /// `WorldData::tile_ys` `+0x1C`.
    #[inline]
    pub fn tile_ys(&self) -> i32 {
        self.ys * WCELL_TILES
    }

    /// `WorldData::is_valid(WCoord, WCoord)` `0x00461420`.
    #[inline]
    pub fn valid_w(&self, wx: i32, wy: i32) -> bool {
        wx >= 0 && wy >= 0 && wx < self.xs && wy < self.ys
    }
    /// `WorldData::is_valid(TCoord, TCoord)` `0x0046F760`.
    #[inline]
    pub fn valid_t(&self, tx: i32, ty: i32) -> bool {
        tx >= 0 && ty >= 0 && tx < self.tile_xs() && ty < self.tile_ys()
    }

    #[inline]
    fn widx(&self, wx: i32, wy: i32) -> usize {
        (wy * self.xs + wx) as usize
    }
    #[inline]
    fn tidx(&self, tx: i32, ty: i32) -> usize {
        (ty * self.tile_xs() + tx) as usize
    }

    /// `World::get_wdata` `0x0046D220`.
    #[inline]
    pub fn wcell(&self, wx: i32, wy: i32) -> WCell {
        self.wdata[self.widx(wx, wy)]
    }
    #[inline]
    pub fn wcell_mut(&mut self, wx: i32, wy: i32) -> &mut WCell {
        let i = self.widx(wx, wy);
        &mut self.wdata[i]
    }
    /// The raw `TData::mask` at a tile.
    #[inline]
    pub fn tmask(&self, tx: i32, ty: i32) -> u16 {
        self.tdata[self.tidx(tx, ty)]
    }
    #[inline]
    pub fn tmask_mut(&mut self, tx: i32, ty: i32) -> &mut u16 {
        let i = self.tidx(tx, ty);
        &mut self.tdata[i]
    }

    // -- predicates -------------------------------------------------------

    /// `WorldData::is_ocean(WCoord, WCoord)` `0x006B4830` [measured, verbatim]:
    ///
    /// ```text
    /// if (flags & 0x100) return 0;              ; a HALFLAND cell is never "ocean"
    /// return land == 2 || land == 1;
    /// ```
    ///
    /// The `HALFLAND` early-out is the load-bearing half: a cell that is half land and half
    /// water reports **not ocean** to every whole-cell query, which is why ships path on a
    /// grid that excludes shorelines.
    pub fn is_ocean(&self, wx: i32, wy: i32) -> bool {
        let c = self.wcell(wx, wy);
        if c.flags & wflag::HALFLAND != 0 {
            return false;
        }
        LAND_WATER_CODES.contains(&c.land)
    }

    /// `WorldData::is_tocean(TCoord, TCoord)` `0x0046FB10`: `(mask & 0x30) == 0x20`.
    pub fn is_tocean(&self, tx: i32, ty: i32) -> bool {
        self.tmask(tx, ty) & tmask::COVER == tmask::COVER_OCEAN
    }

    /// `WorldData::is_tocean_slow(TCoord, TCoord)` `0x006B2400` — the whole-cell fast path
    /// first, then the per-tile test [measured]:
    ///
    /// ```text
    /// if (is_ocean(t >> 2)) return 1;
    /// return (tdata[t].mask & 0x30) == 0x20;
    /// ```
    pub fn is_tocean_slow(&self, tx: i32, ty: i32) -> bool {
        if self.is_ocean(tx >> 2, ty >> 2) {
            return true;
        }
        self.is_tocean(tx, ty)
    }

    /// `WorldData::is_coast(WCoord, WCoord)` `0x006B3020`: `flags & FLAG_COAST`.
    pub fn is_coast(&self, wx: i32, wy: i32) -> bool {
        self.wcell(wx, wy).flags & wflag::COAST != 0
    }

    /// `WorldData::num_waterhalf(WCoord, WCoord)` `0x006B4DB0` [measured] — how many of the
    /// cell's 16 tiles are ocean, **zero unless the cell is flagged `HALFLAND`**.
    ///
    /// The tile scan order is `i in 0..16` with `row = i >> 2`, `col = i & 3`, i.e.
    /// row-major inside the 4x4 block.
    pub fn num_waterhalf(&self, wx: i32, wy: i32) -> i32 {
        if self.wcell(wx, wy).flags & wflag::HALFLAND == 0 {
            return 0;
        }
        let mut n = 0;
        for i in 0..16 {
            let (row, col) = (i >> 2, i & 3);
            if self.is_tocean(wx * 4 + col, wy * 4 + row) {
                n += 1;
            }
        }
        n
    }

    /// `WorldData::is_pass_land(WCoord, WCoord)` `0x006B56B0`:
    /// `(flags & (MOUNTAIN|FOREST)) == 0 && !is_ocean`.
    pub fn is_pass_land(&self, wx: i32, wy: i32) -> bool {
        let f = self.wcell(wx, wy).flags;
        (f & (wflag::MOUNTAIN | wflag::FOREST)) == 0 && !self.is_ocean(wx, wy)
    }

    /// `WorldData::is_passable(WCoord, WCoord)` `0x006B23C0`:
    /// `(flags & (MOUNTAIN|FOREST|CLIFF)) == 0`. Note this says nothing about water — an
    /// ocean cell is "passable", which is exactly right for a ship.
    pub fn is_passable(&self, wx: i32, wy: i32) -> bool {
        self.wcell(wx, wy).flags & (wflag::MOUNTAIN | wflag::FOREST | wflag::CLIFF) == 0
    }

    /// `WorldData::buildings_allowed(WCoord, WCoord)` `0x006B2340`:
    /// `(flags & (ROCK|MOUNTAIN|FOREST|CLIFF)) == 0`.
    pub fn buildings_allowed(&self, wx: i32, wy: i32) -> bool {
        self.wcell(wx, wy).flags & (wflag::ROCK | wflag::MOUNTAIN | wflag::FOREST | wflag::CLIFF)
            == 0
    }

    /// `WorldData::is_blocked_at(TCoord, TCoord, int)` `0x00461340` [measured]:
    ///
    /// ```text
    /// if (mode == 0) return mask & 0x4000;
    /// return (mask & 0x4000) && (mask & 0x30) != 0x30;   ; trees do not block in mode 1
    /// ```
    pub fn is_blocked_at(&self, tx: i32, ty: i32, mode: bool) -> bool {
        let m = self.tmask(tx, ty);
        if !mode {
            return m & tmask::BLOCKED != 0;
        }
        m & tmask::BLOCKED != 0 && m & tmask::COVER != tmask::COVER_TREE
    }

    /// `WorldData::is_built_at(TCoord, TCoord)` `0x0046F880`:
    /// `(mask & 3) == 3 || (mask & 0x80) != 0`.
    pub fn is_built_at(&self, tx: i32, ty: i32) -> bool {
        let m = self.tmask(tx, ty);
        m & tmask::FEATURE == tmask::FEATURE_BUILDING || m & tmask::BUILT != 0
    }

    // -- mutators ---------------------------------------------------------

    /// `World::set_tocean(TCoord, TCoord)` `0x006B1C60`:
    /// `mask = (mask & ~0x10) | 0x20`.
    pub fn set_tocean(&mut self, tx: i32, ty: i32) {
        let m = self.tmask_mut(tx, ty);
        *m = (*m & !0x0010) | tmask::COVER_OCEAN;
    }

    /// `World::set_waterhalf(TCoord, TCoord, int)` `0x006B1C90` [measured]:
    ///
    /// ```text
    /// if (on) { tdata[t] = (tdata[t] & ~0x10) | 0x20;  wdata[t>>2].flags |= 0x100; }
    /// else if ((tdata[t] & 0x30) == 0x20) tdata[t] &= ~0x30;
    /// ```
    ///
    /// Turning a tile to water marks its whole `WCoord` cell `HALFLAND`; turning it back
    /// only clears the tile, never the cell flag. `clear_waterhalf` is the cell-scale
    /// inverse.
    pub fn set_waterhalf(&mut self, tx: i32, ty: i32, on: bool) {
        if on {
            let m = self.tmask_mut(tx, ty);
            *m = (*m & !0x0010) | tmask::COVER_OCEAN;
            self.wcell_mut(tx >> 2, ty >> 2).flags |= wflag::HALFLAND;
        } else {
            let m = self.tmask_mut(tx, ty);
            if *m & tmask::COVER == tmask::COVER_OCEAN {
                *m &= !tmask::COVER;
            }
        }
    }

    /// `World::clear_waterhalf(WCoord, WCoord, int)` `0x006B1BD0` [measured]:
    ///
    /// ```text
    /// if (!keep_tiles) for (i = 0; i < 16; i++)
    ///     if ((tile.mask & 0x30) == 0x20) tile.mask &= ~0x30;
    /// wdata[w].flags &= ~0x100;
    /// ```
    ///
    /// The third argument selects "clear the flag only" vs. "revert the tiles too"; the
    /// flag clear is unconditional either way.
    pub fn clear_waterhalf(&mut self, wx: i32, wy: i32, keep_tiles: bool) {
        if !keep_tiles {
            for i in 0..16 {
                let (row, col) = (i >> 2, i & 3);
                let (tx, ty) = (wx * 4 + col, wy * 4 + row);
                let m = self.tmask_mut(tx, ty);
                if *m & tmask::COVER == tmask::COVER_OCEAN {
                    *m &= !tmask::COVER;
                }
            }
        }
        self.wcell_mut(wx, wy).flags &= !wflag::HALFLAND;
    }

    /// `World::set_coastal(TCoord, TCoord, int)` `0x006B1D10`: tile bit `0x400`.
    pub fn set_coastal(&mut self, tx: i32, ty: i32, on: bool) {
        let m = self.tmask_mut(tx, ty);
        if on {
            *m |= tmask::COASTAL;
        } else {
            *m &= !tmask::COASTAL;
        }
    }

    /// `World::set_building_at(TCoord, TCoord, int)` `0x006B45C0` [measured]:
    /// sets `mask |= 3`; clearing only clears when the field is exactly 3.
    pub fn set_building_at(&mut self, tx: i32, ty: i32, on: bool) {
        let m = self.tmask_mut(tx, ty);
        if on {
            *m |= tmask::FEATURE_BUILDING;
        } else if *m & tmask::FEATURE == tmask::FEATURE_BUILDING {
            *m &= !tmask::FEATURE;
        }
    }

    /// `World::set_land(WCoord, WCoord, land, land_sub, feature, clear_extra)` `0x006B2B50`
    /// [measured]. Negative `land`/`land_sub`/`feature` mean "leave alone".
    ///
    /// The tail is the one that matters here: **setting `FLAG_COAST` also sets
    /// `FLAG_ORIGINAL_COAST`**, which is what `World::revert_coast` `0x006B1D60` later
    /// restores from.
    pub fn set_land(
        &mut self,
        wx: i32,
        wy: i32,
        land: i32,
        land_sub: i32,
        feature: i32,
        clear_extra: bool,
    ) {
        let c = self.wcell_mut(wx, wy);
        if land_sub >= 0 {
            c.land_sub = land_sub as u8;
        }
        if land >= 0 {
            c.land = land as i8;
        }
        if feature >= 0 {
            // 0xFFC3: clears COAST | ROCK | MOUNTAIN | FOREST.
            c.flags &= 0xFFC3;
            if clear_extra {
                // 0xFAFF: clears HALFLAND | ORIGINAL_COAST.
                c.flags &= 0xFAFF;
            }
            c.flags |= feature as u16;
            if feature as u16 == wflag::COAST {
                c.flags |= wflag::ORIGINAL_COAST;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 5. Domain
// ---------------------------------------------------------------------------

/// `DomainIndex` [measured, PDB enum stream].
///
/// The raw enum is `GROUND=0, SEA=1, BOTH=2, AIR=2, NUM_DOMAIN=3, REAL_AIR=4`. **`BOTH`
/// and `AIR` are the same value** — the engine models "flies over everything" and "can go
/// anywhere" as one code, which is why `get_damage`'s elevation term keys off `domain != 2`
/// on both sides.
///
/// `ObjectTypeData::domain` lives at **`+0x218`** [measured, PDB type stream]. In shipped
/// data it is `<DOMAIN>Land|Sea|Air</DOMAIN>` in `ron-data/unitrules.xml`: 287 Land,
/// 43 Sea, 34 Air.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i32)]
pub enum Domain {
    Ground = 0,
    Sea = 1,
    /// `BOTH` and `AIR` alias to 2.
    BothOrAir = 2,
    RealAir = 4,
}

impl Domain {
    /// Byte offset of `ObjectTypeData::domain` inside `ObjectTypeData` [measured].
    pub const FIELD_OFFSET: usize = 0x218;

    pub fn from_i32(v: i32) -> Option<Domain> {
        match v {
            0 => Some(Domain::Ground),
            1 => Some(Domain::Sea),
            2 => Some(Domain::BothOrAir),
            4 => Some(Domain::RealAir),
            _ => None,
        }
    }
    #[inline]
    pub fn as_i32(self) -> i32 {
        self as i32
    }
    /// True for the domain that must stay on water.
    #[inline]
    pub fn is_sea(self) -> bool {
        self == Domain::Sea
    }
    /// True for the domain that must stay on land.
    #[inline]
    pub fn is_ground(self) -> bool {
        self == Domain::Ground
    }
}

// ---------------------------------------------------------------------------
// 6. Type flags, instance masks, leader flags
// ---------------------------------------------------------------------------

/// `UnitTypeData::unit_flags`, `+0x2B4` [measured, PDB type stream + `UnitFlags` enum].
///
/// **The XML letter maps to the bit index directly**: `ron-data/unitrules.xml`'s
/// `<FLAGS>` letters satisfy `bit = 1 << (letter - 'a')` for every letter in the legend,
/// cross-checked name-for-name against the PDB `UnitFlags` enum (`n` = "destroyed when it
/// attacks" = `UNITTYPE_FIRESHIP`; `u` = "is a submarine" = `UNITTYPE_SUBMARINE`;
/// `g` = "attacks sideways" = `UNITTYPE_ATTACK_SIDEWAYS`). See [`unit_flags_from_letters`].
pub mod unit_flag {
    pub const IGNORETERRAIN: u32 = 1 << 0; // 'a'
    pub const HORSEDRAWN: u32 = 1 << 1; // 'b'
    pub const SPECIAL_SUPPORT: u32 = 1 << 2; // 'c'
    pub const SUPPORT: u32 = 1 << 3; // 'd'
    /// `'e'`, and the legend says "(This flag is set in the program)" — no shipped unit
    /// carries it in XML. `UnitData::can_transport` reads it.
    pub const TRANSPORT: u32 = 1 << 4;
    pub const HELICOPTER: u32 = 1 << 5; // 'f'
    pub const ATTACK_SIDEWAYS: u32 = 1 << 6; // 'g'
    pub const NO_RESEARCH: u32 = 1 << 7; // 'h'
    pub const INFANTRY: u32 = 1 << 8; // 'i'
    pub const SKIP_PREDECESSORS: u32 = 1 << 9; // 'j'
    pub const MELEE_AND_RANGED: u32 = 1 << 10; // 'k'
    pub const GARRISON_SMALL: u32 = 1 << 11; // 'l'
    pub const GARRISON_LARGE: u32 = 1 << 12; // 'm'
    /// `'n'` — "Unit destroyed when it attacks". Every Fire Raft / Fireship carries it.
    pub const FIRESHIP: u32 = 1 << 13;
    pub const CLOAK: u32 = 1 << 14; // 'o'
    pub const SPECIALTY: u32 = 1 << 15; // 'p'
    pub const BROADSIDE: u32 = 1 << 16; // 'q'
    pub const SIEGE: u32 = 1 << 17; // 'r'
    pub const STEALTH: u32 = 1 << 18; // 's'
    pub const TANK: u32 = 1 << 19; // 't'
    pub const SUBMARINE: u32 = 1 << 20; // 'u'
    pub const FIRE_WHILE_MOVING: u32 = 1 << 21; // 'v'
    pub const STRAFING: u32 = 1 << 22; // 'w'
    pub const DEAD_BLOCKER: u32 = 1 << 23; // 'x'
    pub const UNIQUE: u32 = 1 << 24; // 'y'
    pub const ROCKING_ATTACK: u32 = 1 << 25; // 'z'
    pub const GOVERNMENT_HERO: u32 = 1 << 26; // '1'
}

/// `UnitTypeData::unit_flags2`, `+0x2B8` [measured].
pub mod unit_flag2 {
    pub const RETARGETS: u32 = 1;
    pub const SPELLCASTER: u32 = 2;
    pub const PACKER: u32 = 4;
    pub const CARAVAN: u32 = 8;
    pub const SPECIAL: u32 = 16;
    pub const HERO: u32 = 32;
    pub const SUPPLY: u32 = 64;
}

/// `UnitData::unit_masks`, `+0x68` — the per-**instance** mask [measured, PDB `UnitMask`].
/// Only the members this lane touches are listed; the enum has 33 entries.
pub mod unit_mask {
    /// Set/cleared by `Unit::think_fish` `0x005F4C60` when the current fishing spot stops
    /// paying out.
    pub const BAD_FISH: u32 = 0x0000_0020;
    pub const PACKED: u32 = 0x0008_0000;
    pub const SEVERE_ATTRITION: u32 = 0x0040_0000;
    /// The per-unit "this unit may be picked up by a transport" bit that
    /// `CommandManager::issue_set_transport` (opcode 14) toggles.
    pub const CAN_TRANSPORT: u32 = 0x0080_0000;
    pub const ENTRENCHED: u32 = 0x0200_0000;
    /// `Unit::check_meet_ship` clears this when it installs the rendezvous move orders.
    pub const MULTIMOVE: u32 = 0x0400_0000;
}

/// `UnitData::unit_masks2`, `+0x6C` [measured, PDB `UnitMask2`].
pub mod unit_mask2 {
    /// Marines: the "can capture from a transport" mechanic.
    pub const MARINE_TRANS: u32 = 0x0000_0200;
    pub const MARINES: u32 = 0x0000_0800;
    /// Vetoes `UNIT_CAN_TRANSPORT`.
    pub const DISABLE_TRANSPORT: u32 = 0x0000_2000;
    pub const MARINE_ENTRENCH: u32 = 0x0002_0000;
}

/// `LeaderData::flags` bits this lane reads [measured, PDB `LeaderFlagIndex`].
pub mod leader_flag {
    pub const HUMAN: u32 = 4;
    pub const CAN_TRANSPORT_CIV: u32 = 256;
    pub const CAN_TRANSPORT_MIL: u32 = 512;
    pub const CAN_TRANSPORT_SCT: u32 = 1024;
}

/// `BuildTypeData::build_flags`, `+0x2C0` [measured, PDB unnamed `BUILDTYPE_GROUND` enum].
///
/// Same letter law as the unit flags: `bit = 1 << (letter - 'a')` for the
/// `ron-data/buildingrules.xml` `<BUILD_FLAGS>` legend. Dock is `ebn` and Shipyard is
/// `ecbn`, so **`b` = `BUILDTYPE_SEA` is the flag that lets a building sit on water**.
pub mod build_flag {
    pub const GROUND: u32 = 1; // 'a' — "can be built on land squares"
    /// `'b'` — "can be built on sea squares". The dock/shipyard flag.
    pub const SEA: u32 = 2;
    pub const GLOBAL: u32 = 4; // 'c'
    pub const OUTSIDE: u32 = 16; // 'e' — outside city radius
    pub const ENEMY: u32 = 32; // 'f'
    pub const GATHER: u32 = 64; // 'g'
    pub const ONE: u32 = 512; // 'j'
    pub const ONWALL: u32 = 1024; // 'k'
    pub const TOWN: u32 = 2048; // 'l'
    pub const WITHIN_CITY: u32 = 4096; // 'm'
    pub const NOT_CAPTURE: u32 = 8192; // 'n'
    pub const UPGRADABLE: u32 = 1 << 26;
    pub const RESEARCH: u32 = 1 << 27;
    pub const FLAT_GATHER: u32 = 1 << 28;
    pub const SPELLCASTER: u32 = 1 << 29;
    pub const MIL_TRAINING: u32 = 1 << 30;
    pub const TRAINING: u32 = 1 << 31;
}

/// `bit = 1 << (letter - 'a')`, the mapping shared by `<FLAGS>` and `<BUILD_FLAGS>`.
///
/// Letters outside `a..=z` are ignored except `'1'`, which the unit legend uses for
/// `UNITTYPE_GOVERNMENT_HERO` (bit 26 — the slot after `'z'`).
pub fn unit_flags_from_letters(letters: &str) -> u32 {
    let mut f = 0u32;
    for c in letters.chars() {
        match c {
            'a'..='z' => f |= 1u32 << (c as u32 - 'a' as u32),
            '1' => f |= unit_flag::GOVERNMENT_HERO,
            _ => {}
        }
    }
    f
}

// ---------------------------------------------------------------------------
// 7. Transports
// ---------------------------------------------------------------------------

/// `TransportType` [measured, PDB enum]. The values are a **ladder**, not a bitmask:
/// `LeaderData::can_transport` returns the highest tier the player has unlocked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i32)]
pub enum TransportType {
    None = 0,
    Scout = 1,
    Military = 2,
    Civilian = 3,
}

/// The `DEFAULT_TRANSPORT` tri-state the scenario editor toggles
/// (`ScenarioEditor::toggle_always_transport` `0x009A13B0`,
/// `toggle_never_transport` `0x009A1420`) [measured, PDB unnamed enum].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum TransportPolicy {
    Default = 0,
    Always = 1,
    Never = 2,
}

/// The naval-relevant slice of a unit type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NavalUnitType {
    /// `Type::index` — the game-wide type id. Unit ids start at 50, so
    /// `index = ordinal_in_unitrules_xml + 50` [measured: `UnitData::transport_type`
    /// special-cases 50..=53 / 61 / 62 / 400, which are exactly Citizen, Citizen, Scholar,
    /// Scholar, Merchant, Armed Merchant, Fur Trapper].
    pub index: i32,
    /// `ObjectTypeData::domain` `+0x218`.
    pub domain: Domain,
    /// `UnitTypeData::unit_flags` `+0x2B4`.
    pub unit_flags: u32,
    /// `UnitTypeData::unit_flags2` `+0x2B8`.
    pub unit_flags2: u32,
    /// `UnitTypeData::carry` `+0x2D4` — transport capacity. 0 for a non-carrier.
    pub carry: i32,
    /// `UnitTypeData::carry_size` `+0x2D8`.
    pub carry_size: i32,
    /// `ObjectTypeData::min_range` `+0x1F8`, in **TCoords** per
    /// `ron-data/unitrules.xml`'s own `RANGE` comment ("in TCoords; 1 WCoord = 4 TCoords").
    pub min_range_tiles: i32,
    /// `ObjectTypeData::max_range` `+0x1FC`, in TCoords.
    pub max_range_tiles: i32,
    /// `UnitTypeData::moves` `+0x2C0`.
    pub moves: i32,
}

impl NavalUnitType {
    /// Minimum weapon range in world units.
    #[inline]
    pub fn min_range_world(&self) -> i32 {
        self.min_range_tiles * TILE
    }
    /// Maximum weapon range in world units.
    #[inline]
    pub fn max_range_world(&self) -> i32 {
        self.max_range_tiles * TILE
    }
    #[inline]
    pub fn is_fireship(&self) -> bool {
        self.unit_flags & unit_flag::FIRESHIP != 0
    }
    #[inline]
    pub fn is_submarine(&self) -> bool {
        self.unit_flags & unit_flag::SUBMARINE != 0
    }
    /// The type-side half of `UnitData::can_transport`.
    #[inline]
    pub fn is_transport_type(&self) -> bool {
        self.unit_flags & unit_flag::TRANSPORT != 0
    }
}

/// The per-instance state `UnitData` carries that the transport predicates read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NavalUnit {
    /// `SubObjectData::o` `+0x0A` — the index in the owner's object list.
    pub o: i16,
    /// `SubObjectData::who` `+0x09`.
    pub who: u8,
    /// `UnitData::unit_masks` `+0x68`.
    pub unit_masks: u32,
    /// `UnitData::unit_masks2` `+0x6C`.
    pub unit_masks2: u32,
    /// De-obfuscated world position.
    pub x: i32,
    pub y: i32,
}

/// `UnitData::can_transport()` `0x0046F960` [measured, verbatim structure]:
///
/// ```text
/// if ( ((unit_masks & 0x800000) == 0 || (unit_masks2 & 0x2000) != 0)
///      && ((type->unit_flags & 0x10) == 0) ) return 0;
/// return 1;
/// ```
///
/// De Morgan'd: **a unit can transport iff it is a transport *type*, or the per-instance
/// `UNIT_CAN_TRANSPORT` bit is on and `UNIT2_DISABLE_TRANSPORT` is off.** Only the second
/// arm is reachable for the shipped Transport Barge/Galleon/Freighter, whose XML `<FLAGS>`
/// do not contain `'e'` — the legend says outright that `e` "is set in the program".
pub fn can_transport(unit: &NavalUnit, ty: &NavalUnitType) -> bool {
    let inst = unit.unit_masks & unit_mask::CAN_TRANSPORT != 0
        && unit.unit_masks2 & unit_mask2::DISABLE_TRANSPORT == 0;
    inst || ty.is_transport_type()
}

/// `UnitData::can_ever_transport()` `0x0046F290` [measured, STRUCTURE-ONLY for the ability
/// query]:
///
/// ```text
/// if (type->domain == GROUND) return 1;
/// if (is_unit() && type->carry != 0 && !has_abil(0x15F)) return 1;
/// return 0;
/// ```
///
/// So *anything* on the ground is eligible to be carried, and a sea/air unit is eligible
/// only if it is itself a carrier (`carry != 0`, e.g. the Aircraft Carrier's 12) and does
/// not carry ability `0x15F`. Ability index `0x15F` is not resolved to a rule name here.
pub fn can_ever_transport(ty: &NavalUnitType, is_unit: bool, has_abil_0x15f: bool) -> bool {
    if ty.domain == Domain::Ground {
        return true;
    }
    is_unit && ty.carry != 0 && !has_abil_0x15f
}

/// Type indices `UnitData::transport_type` `0x0046F790` treats as civilian cargo
/// [measured, the literals compared at `0x0046F7C6`..`0x0046F7FE`].
///
/// Resolved against `ron-data/unitrules.xml` with `index = ordinal + 50`:
/// 50/51 Citizen, 52/53 Scholar, 61 Merchant, 62 Armed Merchant, 400 Fur Trapper.
pub const CIVILIAN_CARGO_TYPE_INDICES: [i32; 7] = [50, 51, 52, 53, 61, 62, 400];

/// `UnitData::transport_type()` `0x0046F790` [measured; the two ability indices are
/// STRUCTURE-ONLY, they are not resolved to rule names]:
///
/// ```text
/// if (has_abil(0x45)) return TRANSPORT_SCOUT;
/// if (index not in {50,51,52,53}) {
///     if (!has_abil(0x3B) && index not in {61,62,400}) return TRANSPORT_MILITARY;
/// }
/// return TRANSPORT_CIVILIAN;
/// ```
pub fn transport_type(type_index: i32, has_abil_0x45: bool, has_abil_0x3b: bool) -> TransportType {
    if has_abil_0x45 {
        return TransportType::Scout;
    }
    let first_four = matches!(type_index, 50 | 51 | 52 | 53);
    if !first_four && !has_abil_0x3b && !matches!(type_index, 61 | 62 | 400) {
        return TransportType::Military;
    }
    TransportType::Civilian
}

/// The verdict `UnitData::needs_transport` returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum TransportNeed {
    /// Same medium, or same tile — walk/sail straight there.
    None = 0,
    /// Source is water and destination is land: the passenger must **get out**.
    Disembark = 1,
    /// Source is land and destination is water: the passenger must **get in**.
    Embark = 2,
}

/// `UnitData::needs_transport(TCoord, TCoord, TCoord, TCoord)` `0x00609920`
/// [measured, verbatim — this function is 118 bytes and reads nothing but two tile masks]:
///
/// ```text
/// if (x1 != x2 || y1 != y2) {
///     a = (tdata[y1][x1].mask & 0x30) == 0x20;
///     b = (tdata[y2][x2].mask & 0x30) == 0x20;
///     if (a != b) return (a ? 1 : 2);
/// }
/// return 0;
/// ```
///
/// Note it uses the raw per-tile ocean bit, **not** `is_tocean_slow` — a half-water
/// `WCoord` cell is judged tile by tile here.
pub fn needs_transport(world: &WaterWorld, from: (i32, i32), to: (i32, i32)) -> TransportNeed {
    if from == to {
        return TransportNeed::None;
    }
    let a = world.is_tocean(from.0, from.1);
    let b = world.is_tocean(to.0, to.1);
    if a == b {
        return TransportNeed::None;
    }
    if a {
        TransportNeed::Disembark
    } else {
        TransportNeed::Embark
    }
}

/// `LeaderData::can_transport()` `0x006E0C60` [measured, verbatim]:
///
/// ```text
/// if (flags & 0x100) return 3;        ; LEADER_CAN_TRANSPORT_CIV -> CIVILIAN
/// if (flags & 0x200) return 2;        ; LEADER_CAN_TRANSPORT_MIL -> MILITARY
/// return (flags >> 10) & 1;           ; LEADER_CAN_TRANSPORT_SCT -> SCOUT else NONE
/// ```
///
/// A strict ladder — a player who has the civilian tech reports `Civilian` even if the
/// military bit is also set.
pub fn leader_transport_level(leader_flags: u32) -> TransportType {
    if leader_flags & leader_flag::CAN_TRANSPORT_CIV != 0 {
        return TransportType::Civilian;
    }
    if leader_flags & leader_flag::CAN_TRANSPORT_MIL != 0 {
        return TransportType::Military;
    }
    if (leader_flags >> 10) & 1 != 0 {
        TransportType::Scout
    } else {
        TransportType::None
    }
}

/// `Group::action_set_transport(int on)` `0x007024B0`, the per-unit half [measured,
/// STRUCTURE-ONLY for the selection walk]:
///
/// ```text
/// on_eff = (leader_transport_level(leader.flags) == 0) ? 0 : on;
/// for each selected unit:
///     if (is_unit && UnitData::can_ever_transport())
///         if (on_eff == 0) unit->unit_masks &= ~UNIT_CAN_TRANSPORT;
///         else             unit->unit_masks |=  UNIT_CAN_TRANSPORT;
/// ```
///
/// This is what `CommandManager::issue_set_transport` (opcode **14**, 5 bytes, group-
/// prefixed) drives. A player with no transport tech cannot turn the bit on at all.
pub fn action_set_transport(
    unit: &mut NavalUnit,
    ty: &NavalUnitType,
    leader_flags: u32,
    on: bool,
    is_unit: bool,
    has_abil_0x15f: bool,
) {
    if !is_unit || !can_ever_transport(ty, is_unit, has_abil_0x15f) {
        return;
    }
    let effective = leader_transport_level(leader_flags) != TransportType::None && on;
    if effective {
        unit.unit_masks |= unit_mask::CAN_TRANSPORT;
    } else {
        unit.unit_masks &= !unit_mask::CAN_TRANSPORT;
    }
}

/// `ObjectData::in_a_ship()` `0x006440C0` [measured, verbatim]:
///
/// ```text
/// carrier = get_inside(&who);            ; ObjectData::get_inside 0x00651A80
/// if (carrier < 0) return false;
/// if (!carrier->is_unit()) return false;
/// return carrier->type->domain == SEA;
/// ```
///
/// "In a ship" is decided purely by the **carrier's domain**, not by any transport flag —
/// a unit garrisoned inside an Aircraft Carrier is "in a ship" for every downstream query.
pub fn in_a_ship(carrier: Option<(bool, Domain)>) -> bool {
    match carrier {
        Some((is_unit, domain)) => is_unit && domain == Domain::Sea,
        None => false,
    }
}

/// `ObjectData::can_carry` `0x006483C0` — the capacity gate, reduced to the part this
/// lane needs. **STRUCTURE-ONLY**: only the capacity comparison was read; the full 730-byte
/// function also consults ownership, diplomacy and the passenger's own state.
///
/// The capacity itself is `UnitTypeData::carry` `+0x2D4`, and the shipped values are
/// Transport Barge 20, Transport Galleon 30, Transport Freighter 40, Aircraft Carrier 12
/// [measured, `ron-data/unitrules.xml`].
pub fn has_carry_room(ty: &NavalUnitType, current_load: i32) -> bool {
    ty.carry > 0 && current_load < ty.carry
}

// ---------------------------------------------------------------------------
// 8. The boarding protocol
// ---------------------------------------------------------------------------

/// `TargetOrder`'s payload, shared by `BoardOrder` and `AwaitBoardOrder` [measured, PDB:
/// `TargetOrder { int ox@8; int whom@0xC; unsigned short uid@0x10; }`, `sizeof = 32`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct TargetRef {
    /// `TargetOrder::ox` — the target's index in its owner's object list.
    pub ox: i32,
    /// `TargetOrder::whom` — the target's owner slot.
    pub whom: i32,
    /// `TargetOrder::uid` — `ObjectData::uid` `+0x30`, the generational id.
    pub uid: u16,
}

/// The two `OrderIndex` arms this lane owns [measured, `OrderNames` at `0x00ECEBA0` and
/// the PDB `OrderIndex` enum].
pub const ORDER_BOARD_SHIP: i32 = 8;
/// See [`ORDER_BOARD_SHIP`].
pub const ORDER_AWAIT_BOARD: i32 = 9;

/// What one tick of the boarding handshake decides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoardStep {
    /// `Unit::check_meet_ship` returned non-zero: a rendezvous is in progress, both sides
    /// have fresh move orders, nothing else happens this tick.
    Rendezvous,
    /// The passenger is adjacent and the ship has room: `Unit::go_inside` `0x0061A2E0`.
    GoInside,
    /// The ship cannot carry the passenger: the order dies.
    Abandon,
}

/// Ordered external mutations issued by one `Unit::do_board` tick.
///
/// This is an exact transaction boundary for the 114-byte shipped executor: the world
/// layer must first apply `set_anim(0, 0, 1)`, then (when present)
/// `kill_current_order(0)`, then `go_inside(target, 0)`. The target lookup and
/// `ObjectData::can_carry(passenger_o, passenger_who)` result are mandatory inputs to
/// [`board_order_transaction`]; this module does not guess capacity from a type row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoardOrderTransaction {
    pub step: BoardStep,
    /// The three literal arguments passed to `Unit::set_anim`.
    pub set_anim: (i32, i32, i32),
    /// Argument to `Unit::kill_current_order`, if the call occurs.
    pub kill_current_order: Option<i32>,
    /// Target and mode argument for `Unit::go_inside`, if the call occurs.
    pub go_inside: Option<(TargetRef, i32)>,
}

/// Build the complete externally-applied call transaction for `Unit::do_board`
/// `0x005ED1F0` [measured].
///
/// `meet_ship_pending` is the actual return value of `Unit::check_meet_ship`; when it is
/// true, retail performs no order removal or containment call. `target_can_carry` is the
/// actual post-rendezvous `ObjectData::can_carry` result and is consulted only after the
/// current order has been killed.
pub fn board_order_transaction(
    target: TargetRef,
    meet_ship_pending: bool,
    target_can_carry: bool,
) -> BoardOrderTransaction {
    let step = if meet_ship_pending {
        BoardStep::Rendezvous
    } else if target_can_carry {
        BoardStep::GoInside
    } else {
        BoardStep::Abandon
    };
    BoardOrderTransaction {
        step,
        set_anim: (0, 0, 1),
        kill_current_order: (!meet_ship_pending).then_some(0),
        go_inside: matches!(step, BoardStep::GoInside).then_some((target, 0)),
    }
}

/// `Unit::do_board(UnitOrder*)` `0x005ED1F0` [measured, verbatim structure — the function
/// is 114 bytes and this is all of it]:
///
/// ```text
/// b = ord->update_board_order();          ; vtable +0x64
/// Unit::set_anim(0, 0, 1);
/// if (check_meet_ship(b->ox, b->whom) == 0) {
///     Unit::kill_current_order(0);
///     if (target->can_carry(this->o, this->who))
///         Unit::go_inside(b->ox, b->whom, 0);
/// }
/// ```
///
/// The order is **not** killed while a rendezvous is running; it is killed the moment the
/// rendezvous resolves, and only then is the load attempted.
///
/// This proxy classifies that branch only. It does not mutate either unit, run
/// `Unit::go_inside`, or implement unloading/disembarkation.
pub fn classify_board_step_proxy(meet_ship_pending: bool, target_can_carry: bool) -> BoardStep {
    board_order_transaction(TargetRef::default(), meet_ship_pending, target_can_carry).step
}

/// `Unit::do_await_board(UnitOrder*)` `0x005ED040` — the **ship's** side [measured,
/// STRUCTURE-ONLY for the two vtable probes].
///
/// The ship holds an `AwaitBoardOrder` naming the passenger. Each tick it re-validates:
/// the passenger must still exist, still be carriable, and its *current action* must still
/// be a `BOARD_SHIP` order naming **this** ship. If any link breaks, the ship's await order
/// is killed (`Unit::kill_current_order(0)`), releasing it to do something else.
///
/// The self-referential case (`target == self`) short-circuits straight to the kill.
pub fn await_board_still_valid(self_ref: TargetRef, passenger: Option<PassengerView>) -> bool {
    let Some(p) = passenger else { return false };
    if p.me.ox == self_ref.ox && p.me.whom == self_ref.whom {
        // The order names the ship itself — degenerate, drop it.
        return false;
    }
    p.is_unit
        && p.carriable
        && p.current_action_order == ORDER_BOARD_SHIP
        && p.board_target.ox == self_ref.ox
        && p.board_target.whom == self_ref.whom
}

/// What the ship needs to know about the unit it is waiting for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PassengerView {
    /// The passenger's own identity.
    pub me: TargetRef,
    /// `Object::is_unit()` — vtable slot 2.
    pub is_unit: bool,
    /// `ObjectData::can_carry` said yes.
    pub carriable: bool,
    /// `UnitData::get_action()->get_type()` — vtable `+0x10` on the action order.
    pub current_action_order: i32,
    /// The `BoardOrder` payload if `current_action_order == ORDER_BOARD_SHIP`.
    pub board_target: TargetRef,
}

/// `Unit::check_meet_ship(int o, int who)` `0x00604550`, the distance banding
/// [measured, the three compares at `0x0060459F`..`0x006045C7`].
///
/// `vector_dist` `0x0046CFF0` between passenger and ship selects one of four regimes:
///
/// | distance (world units) | regime |
/// |---|---|
/// | `<= Constants+0x98` | already met — return 0, the caller loads immediately |
/// | `< 0xC0` (1 tile) | rendezvous is the plain **midpoint** of the two positions |
/// | `< 0x900` (12 tiles) | search the coast: 81-entry spiral (rings 0..4) for a `WCoord` in the ship's tile-region where `Region::coast_here` fires |
/// | `>= 0x900` | coarse: scan `orthog`-style 16-entry offsets from the ship's `WCoord`, take the first `!invalid_loc`, rendezvous at that cell's centre |
///
/// Whichever regime fires, the rendezvous point is snapped to the **unit movement grid**
/// (`div_3_table[v >> 4] * 0x30 + 0x18`, i.e. the centre of a 48-unit cell), a fresh
/// `MoveOrder` is installed on **both** units, the ship's `UNIT_MULTIMOVE` bit is cleared,
/// and `Unit::add_await_board_order` `0x005E48C0` puts the `AwaitBoardOrder` on the ship.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeetRegime {
    AlreadyMet,
    Midpoint,
    CoastSearch,
    CoarseOffsets,
}

/// Threshold constants for [`MeetRegime`] [measured].
pub const MEET_NEAR_WORLD: i32 = 0xC0; // 192 = 1 tile
/// See [`MEET_NEAR_WORLD`].
pub const MEET_FAR_WORLD: i32 = 0x900; // 2304 = 12 tiles

/// Classify a rendezvous by distance. `meet_slack` is `Constants+0x98`, which this lane
/// did not resolve to a rule name — pass the live value.
pub fn meet_regime(dist_world: i32, meet_slack: i32) -> MeetRegime {
    if dist_world <= meet_slack {
        MeetRegime::AlreadyMet
    } else if dist_world < MEET_NEAR_WORLD {
        MeetRegime::Midpoint
    } else if dist_world < MEET_FAR_WORLD {
        MeetRegime::CoastSearch
    } else {
        MeetRegime::CoarseOffsets
    }
}

/// The `Midpoint` regime's arithmetic, verbatim: `(a + b) / 2` on de-obfuscated
/// coordinates, then snapped to the unit grid [measured, `0x0060463A`].
pub fn midpoint_rendezvous(a: (i32, i32), b: (i32, i32)) -> (i32, i32) {
    let mx = (a.0 + b.0) / 2;
    let my = (a.1 + b.1) / 2;
    (ugrid_center(ugrid_of(mx)), ugrid_center(ugrid_of(my)))
}

// ---------------------------------------------------------------------------
// 9. Docks
// ---------------------------------------------------------------------------

/// `DockData`, 10 bytes [measured, PDB type stream — `dock@0 o@2 reg@4 gull_o@6
/// dock_flags@8 who@9`]. `Dock` adds a `DockOut` base and is 20 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dock {
    /// `DockData::dock` — this dock's own slot in its owner's `PtrArray<Dock>`.
    pub dock: i16,
    /// `DockData::o` — the dock *building*'s index in the owner's object list, -1 when free.
    pub o: i16,
    /// `DockData::reg` — the `WData::region` of the building's `WCoord` cell. This is the
    /// water region the dock opens onto, and it is what makes naval reachability a
    /// constant-time query.
    pub reg: i16,
    /// `DockData::gull_o` — the seagull decoration object `Dock::init` spawns, or -1.
    pub gull_o: i16,
    /// `DockData::dock_flags` — bit 0 is "in use".
    pub dock_flags: u8,
    /// `DockData::who` — owner slot, `0xFF` when free.
    pub who: u8,
}

/// The free-slot sentinel used before `Dock::init` claims an array element.
///
/// `EngineArray<T>` requires `Default` because retail's grow path materializes elements.
/// These are the same sentinel fields used by `Docks::init_dock` before the measured init
/// writes run; in particular, a free owner is `0xFF`, not player zero.
impl Default for Dock {
    fn default() -> Self {
        Self {
            dock: -1,
            o: -1,
            reg: -1,
            gull_o: -1,
            dock_flags: 0,
            who: 0xFF,
        }
    }
}

/// `DockData::dock_flags` bit 0, set by `Dock::init` and cleared by `Dock::close`
/// [measured].
pub const DOCK_FLAG_IN_USE: u8 = 1;

/// The type index `Dock::init` spawns as the dock's seagull [measured,
/// `0x00740B4C` pushes `0x194` = 404 to `Objects::init_unit`]. Ordinal 354 in
/// `ron-data/unitrules.xml` under the `index = ordinal + 50` law.
pub const DOCK_GULL_TYPE_INDEX: i32 = 404;

/// The owner slot the gull is created under — the nature/unowned slot. [measured,
/// `Dock::close` reaches the gull through `units.lists[9].data`, i.e. `units + 0x10C`
/// where `?units@@3VUnits@@A` is at `0x00C0AEB0` and each `PtrArray` is 0x1C bytes:
/// `0x10C = 9 * 0x1C + 0x10`.]
pub const NATURE_OWNER_SLOT: usize = 9;

/// The exact `Objects::init_unit` request issued by `Dock::init` `0x00740A80`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DockGullSpawnRequest {
    pub who: usize,
    pub type_index: i32,
    pub x: i32,
    pub y: i32,
    pub arg5: i32,
    pub arg6: i32,
    pub arg7: i32,
}

/// The two calls issued after a successful dock-gull allocation.
///
/// The world callback passed to [`Docks::init_dock_transaction`] must apply
/// `Unit::set_angle(angle, 7, 0)` to `gull_o`, followed by
/// `Unit::add_strafe_order(building_o, building_who, -1, -1, 1, 2, 0)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DockGullInitEffect {
    pub gull_o: i16,
    pub angle: i32,
    pub angle_steps: i32,
    pub angle_mode: i32,
    pub building_o: i32,
    pub building_who: i32,
    pub target_o: i32,
    pub target_who: i32,
    pub strafe_flag: i32,
    pub queue_pos: i32,
    pub trailing: i32,
}

/// The destruction call issued by `Dock::close` after clearing the dock fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DockGullDestroyEffect {
    pub who: usize,
    pub gull_o: i16,
    /// Arguments to the gull's vtable `+0x150` call.
    pub args: (i32, i32, i32),
}

/// The per-leader naval bookkeeping this lane maintains.
///
/// All four arrays are inside `LeaderData` and therefore inside checksum channel 8
/// (`LeaderData::walk_data` `0x006D6750`, walked inline by `CheckSums::check_all`)
/// [measured, PDB field offsets].
#[derive(Clone, Debug)]
pub struct LeaderNaval {
    /// `LeaderData::dock_mark` `+0x42C` — the live length of this player's dock list.
    /// `Docks::init_dock` uses it as the allocation high-water mark, *separate* from the
    /// `PtrArray`'s own `length`.
    pub dock_mark: i32,
    /// `LeaderData::reg_docks` `+0x135E`, `unsigned short[64]` — docks per region.
    pub reg_docks: [u16; 64],
    /// `LeaderData::reg_naval` `+0x0EE2`, `unsigned short[63]` — naval units per region.
    pub reg_naval: [u16; 63],
    /// `LeaderData::reg_transports` `+0x0F60`, `unsigned short[63]`.
    pub reg_transports: [u16; 63],
    /// `LeaderData::naval` `+0x960`, `sea_combat` `+0x94C`, `transports` `+0x96C`,
    /// `fishermen` `+0x970`, `idle_fishermen` `+0x974`, `dock_units` `+0xA08`,
    /// `dock_queued` `+0xA20`, `sea_mod` `+0x7A0`.
    pub naval: i32,
    pub sea_combat: i32,
    pub transports: i32,
    pub fishermen: i32,
    pub idle_fishermen: i32,
    pub dock_units: i32,
    pub dock_queued: i32,
    pub sea_mod: i32,
}

impl Default for LeaderNaval {
    fn default() -> LeaderNaval {
        LeaderNaval {
            dock_mark: 0,
            reg_docks: [0; 64],
            reg_naval: [0; 63],
            reg_transports: [0; 63],
            naval: 0,
            sea_combat: 0,
            transports: 0,
            fishermen: 0,
            idle_fishermen: 0,
            dock_units: 0,
            dock_queued: 0,
            sea_mod: 0,
        }
    }
}

/// The cap `Dock::init` and `Dock::close` guard `reg_docks` with [measured,
/// `cmp reg, 0x40` at `0x00740B15`].
pub const MAX_DOCK_REGION: i16 = 64;

/// `Docks`, the eight per-player `PtrArray<Dock>` lists.
///
/// `DocksData::lists : PtrArray<Dock>[8]` at offset 0, `sizeof(DocksData) = 224`,
/// `sizeof(Docks) = 232` [measured, PDB]. The array uses the engine's own growth policy —
/// see [`EngineArray`] — because `Docks::walk_data` `0x00741100` emits the capacity and
/// growth hint, so a `Vec` here would be a latent save/desync difference.
pub struct Docks {
    pub lists: [EngineArray<Dock>; 8],
}

impl Default for Docks {
    fn default() -> Docks {
        Docks::new()
    }
}

impl Docks {
    pub fn new() -> Docks {
        Docks {
            lists: [
                EngineArray::new(),
                EngineArray::new(),
                EngineArray::new(),
                EngineArray::new(),
                EngineArray::new(),
                EngineArray::new(),
                EngineArray::new(),
                EngineArray::new(),
            ],
        }
    }

    /// Allocate/reuse the exact registry slot, initialize its measured fields, and update
    /// `LeaderData::reg_docks`. The 16-bit counter uses retail's wrapping increment.
    fn claim_dock_registry(
        &mut self,
        leader: &mut LeaderNaval,
        who: usize,
        o: i16,
        region: i16,
    ) -> i16 {
        let list = &mut self.lists[who];
        let mut slot: i32 = -1;
        let mark = leader.dock_mark.min(list.len() as i32);
        for i in 0..mark {
            if list
                .get(i as usize)
                .map(|d| d.dock_flags & DOCK_FLAG_IN_USE == 0)
                == Some(true)
            {
                slot = i;
                break;
            }
        }
        if slot < 0 {
            if leader.dock_mark < list.len() as i32 {
                slot = leader.dock_mark;
            } else {
                list.add(Dock::default());
                slot = list.len() as i32 - 1;
            }
        }
        leader.dock_mark = leader.dock_mark.max(slot + 1);
        let d = self.lists[who]
            .get_mut(slot as usize)
            .expect("slot in range");
        d.init_fields(slot as i16, who as u8, o, region);
        if region >= 0 && region < MAX_DOCK_REGION {
            let count = &mut leader.reg_docks[region as usize];
            *count = count.wrapping_add(1);
        }
        slot as i16
    }

    /// `Docks::init_dock(int who, int o)` `0x00740FC0` [measured, verbatim structure]:
    ///
    /// ```text
    /// slot = first i < leader.dock_mark with (lists[who][i]->dock_flags & 1) == 0
    /// if none:
    ///     if (dock_mark < lists[who].length) slot = dock_mark++          ; reuse tail
    ///     else { push a fresh Dock; slot = lists[who].length - 1; }
    /// leader.dock_mark = max(leader.dock_mark, slot + 1);
    /// Dock::init(slot, who, o);
    /// ```
    ///
    /// The free-slot scan is a linear walk from index 0, so **dock slot assignment is
    /// order-dependent and must be reproduced exactly** — the slot index is what
    /// `BuildData::dock` (`+0x78`, `short`) stores on the building.
    ///
    /// `spawn_gull` is `Dock::init`'s tail; see [`Dock::init_fields`] for why it consumes
    /// RNG.
    /// Registry-only proxy. This does **not** spawn the retail gull, consume its RNG draw,
    /// install its strafe order, or write the dock building's backlink. Callers must not
    /// treat it as a complete `Docks::init_dock` port.
    pub fn init_dock_registry_only(
        &mut self,
        leader: &mut LeaderNaval,
        who: usize,
        o: i16,
        region: i16,
    ) -> i16 {
        self.claim_dock_registry(leader, who, o, region)
    }

    /// Execute `Docks::init_dock` + `Dock::init` through the recovered external-call
    /// boundary [measured, `0x00740A80` and `0x00740FC0`].
    ///
    /// `spawn_gull` is invoked only after the dock fields and region counter are live, with
    /// the exact nature-owner/type/coordinate arguments. If it returns a non-negative
    /// object index, this function consumes exactly one draw from the supplied **main game
    /// RNG**, invokes `finish_gull` with the exact angle and strafe calls, and only then
    /// stores `DockData::gull_o`. A failed allocation consumes no draw and invokes no
    /// finish callback.
    pub fn init_dock_transaction<S, F>(
        &mut self,
        leader: &mut LeaderNaval,
        who: usize,
        building_o: i16,
        building_xy: (i32, i32),
        region: i16,
        rng: &mut Random,
        spawn_gull: S,
        finish_gull: F,
    ) -> i16
    where
        S: FnOnce(DockGullSpawnRequest) -> i16,
        F: FnOnce(DockGullInitEffect),
    {
        let slot = self.claim_dock_registry(leader, who, building_o, region);
        let gull_o = spawn_gull(DockGullSpawnRequest {
            who: NATURE_OWNER_SLOT,
            type_index: DOCK_GULL_TYPE_INDEX,
            x: building_xy.0.wrapping_sub(TILE),
            y: building_xy.1.wrapping_sub(TILE),
            arg5: -1,
            arg6: -1,
            arg7: -1,
        });
        if let Some(angle) = Dock::gull_angle_after_spawn(rng, gull_o) {
            finish_gull(DockGullInitEffect {
                gull_o,
                angle,
                angle_steps: 7,
                angle_mode: 0,
                building_o: building_o as i32,
                building_who: who as i32,
                target_o: -1,
                target_who: -1,
                strafe_flag: 1,
                queue_pos: 2,
                trailing: 0,
            });
        }
        self.lists[who]
            .get_mut(slot as usize)
            .expect("claimed dock slot")
            .gull_o = gull_o;
        slot
    }

    /// `Docks::close_dock(int who, int o)` `0x00740F50` [measured] — pops trailing free
    /// slots off `dock_mark` after the dock is closed. Note the retail function returns -1
    /// unconditionally; the return value carries no information.
    /// Registry-only proxy. This does **not** dispatch the retail gull destruction call.
    pub fn close_dock_registry_only(&mut self, leader: &mut LeaderNaval, who: usize, slot: i16) {
        if let Some(d) = self.lists[who].get_mut(slot as usize) {
            let reg = d.reg;
            let in_use = d.dock_flags & DOCK_FLAG_IN_USE != 0;
            d.close_fields();
            if in_use && reg >= 0 && reg < MAX_DOCK_REGION {
                let r = &mut leader.reg_docks[reg as usize];
                *r = r.wrapping_sub(1);
            }
        }
        while leader.dock_mark > 0 {
            let i = (leader.dock_mark - 1) as usize;
            match self.lists[who].get(i) {
                Some(d) if d.dock_flags & DOCK_FLAG_IN_USE != 0 => break,
                _ => leader.dock_mark -= 1,
            }
        }
    }

    /// Execute `Docks::close_dock` + `Dock::close` through the recovered external-call
    /// boundary [measured, `0x007409F0` and `0x00740F50`].
    ///
    /// `building_active` is the mandatory result of the exact object lookup and
    /// `SubObject::flags & 1` test performed before retail decrements `reg_docks`. The gull
    /// destroy callback runs after the dock fields are cleared, matching retail call
    /// order. Trailing free registry slots are then removed from `dock_mark`.
    pub fn close_dock_transaction<K>(
        &mut self,
        leader: &mut LeaderNaval,
        who: usize,
        slot: i16,
        building_active: bool,
        destroy_gull: K,
    ) where
        K: FnOnce(DockGullDestroyEffect),
    {
        let mut gull_o = -1;
        if let Some(d) = self.lists[who].get_mut(slot as usize) {
            let o = d.o;
            let reg = d.reg;
            gull_o = d.gull_o;
            if o >= 0 && building_active && reg >= 0 && reg < MAX_DOCK_REGION {
                let count = &mut leader.reg_docks[reg as usize];
                *count = count.wrapping_sub(1);
            }
            d.close_fields();
        }
        if gull_o >= 0 {
            destroy_gull(DockGullDestroyEffect {
                who: NATURE_OWNER_SLOT,
                gull_o,
                args: (0, -1, 0),
            });
        }
        while leader.dock_mark > 0 {
            let i = (leader.dock_mark - 1) as usize;
            match self.lists[who].get(i) {
                Some(d) if d.dock_flags & DOCK_FLAG_IN_USE != 0 => break,
                _ => leader.dock_mark -= 1,
            }
        }
    }

    /// Every region in which `who` owns at least one dock — the naval-reachability index.
    pub fn dock_regions(leader: &LeaderNaval) -> Vec<i16> {
        (0..MAX_DOCK_REGION)
            .filter(|r| leader.reg_docks[*r as usize] > 0)
            .collect()
    }
}

impl Dock {
    /// `Dock::init(int dock, int who, short o)` `0x00740A80`, the field writes [measured,
    /// verbatim]:
    ///
    /// ```text
    /// this->dock = dock; this->who = who; this->o = o;
    /// this->dock_flags = 1;
    /// this->reg = wdata[wcell(building.y)][wcell(building.x)].region;
    /// if (reg < 64) leaders[who].reg_docks[reg]++;
    /// gull = Objects::init_unit(9, 404, x - 0xC0, y - 0xC0, -1, -1, -1);
    /// if (gull >= 0) {
    ///     r = game_random.get(0, 0xFFFF);
    ///     Unit::set_angle((r % 7) * 0xAAAAAAA - 0x40000000, 7, 0);
    ///     Unit::add_strafe_order(o, who, -1, -1, 1, 2, 0);
    /// }
    /// this->gull_o = gull;
    /// ```
    ///
    /// **`Dock::init` draws from `game_random`.** Building a dock therefore advances the
    /// shared lockstep stream — a reimplementation that skips the "cosmetic" seagull
    /// desyncs everything downstream. `dock_flags` bit 0 is set before the spawn, and note
    /// the `reg_docks` increment happens even if the gull spawn fails.
    pub fn init_fields(&mut self, dock: i16, who: u8, o: i16, region: i16) {
        self.dock = dock;
        self.who = who;
        self.o = o;
        // 0x00740A9B is `mov byte ptr [esi+8], 1`, not an OR. Reusing a slot therefore
        // discards every stale auxiliary bit left by save data or a prior lifecycle.
        self.dock_flags = DOCK_FLAG_IN_USE;
        self.reg = region;
        self.gull_o = -1;
    }

    /// The conditional `game_random` draw and angle arm of `Dock::init` [measured].
    ///
    /// A failed `Objects::init_unit` returns a negative gull index and consumes **zero**
    /// draws. A successful spawn consumes exactly one. This helper deliberately takes the
    /// spawn result so a future full port cannot accidentally draw unconditionally.
    pub fn gull_angle_after_spawn(rng: &mut Random, gull_o: i16) -> Option<i32> {
        if gull_o < 0 {
            return None;
        }
        let r = rng.get(0, 0xFFFF);
        Some((r % 7).wrapping_mul(0x0AAA_AAAA).wrapping_sub(0x4000_0000))
    }

    /// `Dock::close()` `0x007409F0` [measured, verbatim]:
    ///
    /// ```text
    /// if (o >= 0 && object(who, o)->flags & 1 && reg < 64) leaders[who].reg_docks[reg]--;
    /// dock_flags &= ~1;
    /// o = -1; reg = 0; who = 0xFF;
    /// if (gull_o >= 0) units[9][gull_o]->vt[0x150](0, -1, 0);
    /// ```
    ///
    /// Note `reg` is reset to **0**, not -1 — the engine leaves a live-looking region id on
    /// a freed dock. Reproduce that or a save-game round trip differs.
    pub fn close_fields(&mut self) {
        self.dock_flags &= !DOCK_FLAG_IN_USE;
        self.o = -1;
        self.reg = 0;
        self.who = 0xFF;
    }
}

/// `BuildTypeData::is_dock_tile(WCoord wx, WCoord wy, int)` `0x00636700` — **the shore
/// constraint** [measured, full transcription of the 834-byte function].
///
/// A dock is 4x4 tiles, i.e. exactly one `WCoord` cell (`ron-data/buildingrules.xml`:
/// Dock and Shipyard both `X_SIZE 4 / Y_SIZE 4`), which is why this predicate is
/// WCoord-scale. The engine's own words, restructured:
///
/// ```text
/// if (!valid_w(wx, wy)) return 0;
/// if (!is_ocean(wx, wy)) return 0;                  ; the dock's own cell must be open water
/// tx = wx * 4; ty = wy * 4;
/// for k in 1..=4:                                   ; orthog_x[1..4] / orthog_y[1..4]
///     dx = orthog_x[k]; dy = orthog_y[k];
///     if (dx > 0) dx <<= 2;                         ; +1 cell -> +4 tiles; -1 stays -1
///     if (dy > 0) dy <<= 2;
///     nx = tx + dx; ny = ty + dy;
///     if (!valid_t(nx, ny)) continue;
///     if (is_ocean(nx >> 2, ny >> 2)) continue;     ; that way is more water, not shore
///     m = tdata[ny][nx].mask;
///     if ((m & 3) == 2) continue;                   ; mountain shore: no
///     if ((m & 0x30) == 0x30) continue;             ; forest shore: no
///     ok = 1;
///     if (dx == 0):                                 ; vertical approach -> scan 2 rows x 4 cols
///         for row in 0..2:
///             if (!ok) break;
///             if (row) ny += sign(dy);
///             for c in 0..4:
///                 mm = tdata[ny][tx + c].mask;
///                 if ((mm & 0x80) || (mm & 3) == 3) { ok = 0; break; }
///     else:                                         ; horizontal approach -> 4 rows x 2 cols
///         for col in 0..2:
///             if (!ok) break;
///             if (col) nx += sign(dx);
///             for c in 0..4:
///                 mm = tdata[ty + c][nx].mask;
///                 if ((mm & 0x80) || (mm & 3) == 3) { ok = 0; break; }
///     if (ok) return 1;
/// return 0;
/// ```
///
/// In one sentence: **the dock cell must be open water, and at least one of its four
/// orthogonal neighbours must be a non-mountain non-forest shore tile whose 4-long strip,
/// two tiles deep, is free of buildings.** That 4x2 apron is the gangway the citizens walk
/// on, and it is why docks refuse to place on cliff-backed or built-up coastline.
pub fn is_dock_tile(world: &WaterWorld, wx: i32, wy: i32) -> bool {
    if !world.valid_w(wx, wy) {
        return false;
    }
    if !world.is_ocean(wx, wy) {
        return false;
    }
    let (tx, ty) = (wx * WCELL_TILES, wy * WCELL_TILES);
    for k in 1..=4usize {
        let mut dx = ORTHOG_X[k];
        let mut dy = ORTHOG_Y[k];
        if dx > 0 {
            dx <<= 2;
        }
        if dy > 0 {
            dy <<= 2;
        }
        let (mut nx, mut ny) = (tx + dx, ty + dy);
        if !world.valid_t(nx, ny) {
            continue;
        }
        if world.is_ocean(nx >> 2, ny >> 2) {
            continue;
        }
        let m = world.tmask(nx, ny);
        if m & tmask::FEATURE == tmask::FEATURE_MOUNTAIN {
            continue;
        }
        if m & tmask::COVER == tmask::COVER_TREE {
            continue;
        }
        let mut ok = true;
        if dx == 0 {
            for row in 0..2 {
                if !ok {
                    break;
                }
                if row != 0 {
                    ny += dy.signum();
                }
                for c in 0..4 {
                    if !world.valid_t(tx + c, ny) {
                        ok = false;
                        break;
                    }
                    let mm = world.tmask(tx + c, ny);
                    if mm & tmask::BUILT != 0 || mm & tmask::FEATURE == tmask::FEATURE_BUILDING {
                        ok = false;
                        break;
                    }
                }
            }
        } else {
            for col in 0..2 {
                if !ok {
                    break;
                }
                if col != 0 {
                    nx += dx.signum();
                }
                for c in 0..4 {
                    if !world.valid_t(nx, ty + c) {
                        ok = false;
                        break;
                    }
                    let mm = world.tmask(nx, ty + c);
                    if mm & tmask::BUILT != 0 || mm & tmask::FEATURE == tmask::FEATURE_BUILDING {
                        ok = false;
                        break;
                    }
                }
            }
        }
        if ok {
            return true;
        }
    }
    false
}

/// A building may sit on water iff its `build_flags` carry `BUILDTYPE_SEA`
/// (`ron-data/buildingrules.xml` letter `b`, "Building can be built on sea squares").
/// Only Dock and Shipyard carry it in the shipped data.
#[inline]
pub fn build_allowed_on_sea(build_flags: u32) -> bool {
    build_flags & build_flag::SEA != 0
}

// ---------------------------------------------------------------------------
// 10. Water pathing
// ---------------------------------------------------------------------------

/// `PathData` — the pathfinder's output waypoint, 16 bytes [measured, PDB:
/// `to_x@0 to_y@4 tolerance@8 flags@0xC`]. The `Stack<PathData>` a unit carries at
/// `UnitData::path` `+0xB8` is checksummed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct PathData {
    pub to_x: i32,
    pub to_y: i32,
    pub tolerance: i32,
    pub flags: i32,
}

/// `PathFinderData` — the naval-relevant search flags [measured, PDB type stream, 136
/// bytes at `pathfinder + 0x40` = `0x00E85E80`].
///
/// `find_wpath` writes `sx`/`sy` (`+0x18`/`+0x1C` = `0x00E85E98`/`0x00E85E9C`), which sit
/// inside `CheckSums::check_pathfinder`'s flat 108-byte window
/// `[0x00E85E98, 0x00E85F04)` — **the search's scratch state is lockstep-critical**.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WaterSearchFlags {
    /// `+0x18` / `+0x1C` — the start tile, `div_3_table[world >> 6]`.
    pub sx: i32,
    pub sy: i32,
    /// `+0x28` / `+0x2C`, both zeroed by `find_wpath`.
    pub offx: i32,
    pub offy: i32,
    /// `+0x30`.
    pub army: bool,
    /// `+0x38`.
    pub worker: bool,
    /// `+0x48` / `+0x4C` — the naval pathing vetoes.
    pub avoid_land: bool,
    pub avoid_sea: bool,
    /// `+0x54` — set when the pathing unit is exploring past a blocked coast.
    pub scouting: bool,
    /// `+0x58`.
    pub can_transport: bool,
}

/// `PathFinder::valid_wcoord(x, y, step, dx, dy)` `0x00687DA0` [measured, verbatim]:
///
/// ```text
/// if (x == pfd.avoid_x && y == pfd.avoid_y) return false;
/// r = UnitData::invalid_loc(div3[x>>6], div3[y>>6], 1, step > 1, 0, 1, 0, x >> 6);
/// if (r == 3 && div3[x>>8] == div3[dx>>8] && div3[y>>8] == div3[dy>>8]) r = 0;
/// return r == 0;
/// ```
///
/// Two things matter. First, the validity test runs at **tile** granularity even though
/// the water A\* steps 768 world units — the engine converts down before asking. Second,
/// `invalid_loc` result **3** is forgiven when the candidate is in the same `WCoord` cell
/// as the destination: a ship is allowed to finish inside an otherwise-invalid cell, which
/// is how it reaches a dock.
///
/// `UnitData::invalid_loc` `0x00607C30` (1,034 bytes) is **not ported here** — it belongs
/// to the movement lane, which already owns the unit A\*. Pass its verdict in.
pub fn valid_wcoord(
    invalid_loc: i32,
    x: i32,
    y: i32,
    dest_x: i32,
    dest_y: i32,
    avoid: (i32, i32),
) -> bool {
    if x == avoid.0 && y == avoid.1 {
        return false;
    }
    let mut r = invalid_loc;
    if r == 3 && wcell_of(x) == wcell_of(dest_x) && wcell_of(y) == wcell_of(dest_y) {
        r = 0;
    }
    r == 0
}

/// What the entry-only `find_wpath` proxy can establish without running retail A\*.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WPathProxyOutcome {
    /// Start and destination are in the same `WCoord` cell, or the pathing unit is a
    /// helicopter (`UNITTYPE_HELICOPTER`): the waypoint is pushed back unchanged.
    NoRouteNeeded,
    /// The destination `WCoord` is off the map: `stack.len = 0`, return -1.
    OffMap,
    /// None of the measured early exits applied. The missing coarse walk and retail A\*
    /// must run; this proxy deliberately does not guess their result.
    FullSearchRequired,
}

/// `PathFinder::find_wpath(Stack<PathData>*, Coord x, Coord y, int play, int id)`
/// `0x00688FC0`, the entry conditions [measured; the interior coarse walk is
/// STRUCTURE-ONLY].
///
/// The thin overload `0x00688E10` is what `Unit::do_move` calls; it looks the unit up in
/// `objects[play].list[id]`, de-obfuscates `SubObjectData::x_internal`/`y_internal`
/// (`^ 0x63637`) and forwards. The body then:
///
/// 1. writes `pfd.pathing_unit`, clears `pfd.scouting`, sets `pfd.sx/sy` from the unit's
///    **tile**, zeroes `pfd.offx/offy`;
/// 2. **pops** the top `PathData` off the caller's stack — that is the destination;
/// 3. bounds-checks the destination `WCoord` against `world.xs`/`world.ys`, and on failure
///    empties the stack and returns -1;
/// 4. if start and destination share a `WCoord`, **or** the unit type carries
///    `UNITTYPE_HELICOPTER` (`unit_flags & 0x20`), pushes the waypoint back and returns;
/// 5. branches on `LeaderData::flags & LEADER_HUMAN` (`4`) — the human and AI paths差 in
///    which coarse walk they run first;
/// 6. runs a straight-line coarse walk in world space, stepping `0x30` when the remaining
///    Manhattan distance is under `0x300` and `0x180` otherwise, using `find_angle`
///    `0x0092D130` + `sin_table` `0x00A46A00`, stopping when `WorldData::get_tregion`
///    agrees between the cursor and the destination;
/// 7. sets `pfd.worker` from `ObjectData::is_worker`, `pfd.army` when the unit is a supply
///    unit that is neither a worker nor attacking and is not standing on a `HALFLAND` cell;
/// 8. pushes the destination and the cursor as `WCoord`-centre waypoints
///    (`w * 0x300 + 0x180`) and calls `astar_path(stack, 0x300, 0)`;
/// 9. always finishes with `PathFinder::kill_lists` `0x00687AE0` and clears
///    `pfd.army`/`pfd.worker`/`pfd.iroquois`.
///
/// This function reproduces steps 2-4 exactly, which is where every early-out lives, and
/// reports which regime the remainder would have taken.
pub fn find_wpath_entry_proxy(
    world: &WaterWorld,
    stack: &mut Vec<PathData>,
    unit_x: i32,
    unit_y: i32,
    unit_flags: u32,
    pfd: &mut WaterSearchFlags,
) -> (WPathProxyOutcome, i32) {
    pfd.sx = tile_of(unit_x);
    pfd.sy = tile_of(unit_y);
    pfd.offx = 0;
    pfd.offy = 0;
    pfd.scouting = false;

    let (swx, swy) = (wcell_of(unit_x), wcell_of(unit_y));

    if stack.is_empty() {
        // The engine clamps `len` to at least 1 before decrementing, so an empty stack
        // reads slot 0 as garbage. We refuse instead and say so.
        return (WPathProxyOutcome::OffMap, -1);
    }
    let dest = stack.pop().expect("non-empty");
    let (dwx, dwy) = (wcell_of(dest.to_x), wcell_of(dest.to_y));

    if !world.valid_w(dwx, dwy) {
        stack.clear();
        return (WPathProxyOutcome::OffMap, -1);
    }

    if (swx == dwx && swy == dwy) || unit_flags & unit_flag::HELICOPTER != 0 {
        stack.push(dest);
        return (WPathProxyOutcome::NoRouteNeeded, stack.len() as i32);
    }

    // Steps 6-9 need `invalid_loc`, `get_tregion`, `find_angle` and `sin_table`, all owned
    // by other lanes. Push the destination back so the caller sees the engine's stack
    // shape and report which regime is next.
    stack.push(dest);
    (WPathProxyOutcome::FullSearchRequired, stack.len() as i32)
}

/// The step `find_wpath` hands to `astar_path`, which is also how `astar_path` selects
/// `valid_wcoord` (`cmp ecx, 0x300`) [measured].
pub const WATER_ASTAR_STEP: i32 = 0x300;
/// The unit-domain step, for contrast — `find_upath` pushes `0x30` [measured].
pub const UNIT_ASTAR_STEP: i32 = 0x30;
/// The tile-domain step — `find_tpath` pushes `0xC0` [measured].
pub const TILE_ASTAR_STEP: i32 = 0xC0;

/// The coarse walk's step selection [measured, `0x00689236`]: `0x30` while the remaining
/// Manhattan distance is under `0x300`, `0x180` once it is at or above.
pub fn coarse_step(remaining_manhattan: i32) -> i32 {
    if remaining_manhattan >= 0x300 {
        0x180
    } else {
        0x30
    }
}

/// A minimal, engine-shaped water A\* over `WCoord` cells.
///
/// This is the **shape** of `astar_path` at step 768: 8-connected using
/// `SPIRAL_[XY][1..=8]` in that exact index order, integer costs, and a monotone
/// tie-break by neighbour index so the relaxation order matches the engine's. It is **not**
/// a port of `PathFinder::astar_path` `0x00683770` — that function is 5,845 bytes with an
/// ordered-tree open list and `PathFinder::calc_cost` `0x00684E50` supplying edge weights,
/// and it belongs to the movement lane, which already owns the unit domain. What is
/// naval-specific and *is* reproduced here is the passability rule: a `WCoord` is
/// traversable by a sea-domain unit iff [`WaterWorld::is_ocean`].
///
/// Returns the cell path from `start` to `goal` inclusive, or `None`.
pub fn water_route_proxy(
    world: &WaterWorld,
    start: (i32, i32),
    goal: (i32, i32),
    node_budget: usize,
) -> Option<Vec<(i32, i32)>> {
    use std::collections::BinaryHeap;
    if !world.valid_w(start.0, start.1) || !world.valid_w(goal.0, goal.1) {
        return None;
    }
    if !world.is_ocean(goal.0, goal.1) {
        return None;
    }
    let w = world.xs as usize;
    let h = world.ys as usize;
    let idx = |x: i32, y: i32| (y as usize) * w + (x as usize);

    let mut g = vec![i32::MAX; w * h];
    let mut parent = vec![usize::MAX; w * h];
    let mut closed = vec![false; w * h];

    // Chebyshev heuristic scaled to the 10/14 diagonal cost below.
    let hcost = |x: i32, y: i32| {
        let dx = (goal.0 - x).abs();
        let dy = (goal.1 - y).abs();
        let (lo, hi) = if dx < dy { (dx, dy) } else { (dy, dx) };
        14 * lo + 10 * (hi - lo)
    };

    // Max-heap of (-f, -tiebreak, index) so lower f pops first and the tie-break keeps the
    // neighbour insertion order stable.
    let mut open: BinaryHeap<(i32, i32, usize)> = BinaryHeap::new();
    g[idx(start.0, start.1)] = 0;
    open.push((-hcost(start.0, start.1), 0, idx(start.0, start.1)));

    let mut expanded = 0usize;
    let mut seq: i32 = 0;
    while let Some((_, _, cur)) = open.pop() {
        if closed[cur] {
            continue;
        }
        closed[cur] = true;
        expanded += 1;
        if expanded > node_budget {
            return None;
        }
        let cx = (cur % w) as i32;
        let cy = (cur / w) as i32;
        if (cx, cy) == goal {
            let mut path = vec![(cx, cy)];
            let mut p = cur;
            while parent[p] != usize::MAX {
                p = parent[p];
                path.push(((p % w) as i32, (p / w) as i32));
            }
            path.reverse();
            return Some(path);
        }
        for k in 1..=8usize {
            let nx = cx + SPIRAL_X[k];
            let ny = cy + SPIRAL_Y[k];
            if !world.valid_w(nx, ny) {
                continue;
            }
            if !world.is_ocean(nx, ny) {
                continue;
            }
            let step = if SPIRAL_X[k] != 0 && SPIRAL_Y[k] != 0 {
                14
            } else {
                10
            };
            let ni = idx(nx, ny);
            let ng = g[cur].saturating_add(step);
            if ng < g[ni] {
                g[ni] = ng;
                parent[ni] = cur;
                seq += 1;
                open.push((-(ng + hcost(nx, ny)), -seq, ni));
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 11. Naval combat
// ---------------------------------------------------------------------------

/// The domain terms inside `ObjectData::get_damage` `0x00644130` [measured, structure from
/// `re/decomp-all/00644130.c` cross-checked against the disassembly at the cited lines].
///
/// The full 31-step chain belongs to the combat lane
/// (`crates/don-sim/src/systems/combat.rs`); these four predicates are the naval slice, and
/// they are stated as predicates rather than as a damage number so the combat lane can call
/// them without either module owning the other's arithmetic.
pub mod damage_domain {
    use super::Domain;

    /// The `x2` bonus arm at `0x006446xx`: fires when the attacker is anti-air **or** is a
    /// GROUND unit shooting a non-GROUND target, and the target is a unit that is either
    /// severely attrited or a SEA unit under a rules condition.
    ///
    /// This is the naval half of "anti-air": a land unit firing on a *ship* takes the same
    /// branch a land unit firing on a *plane* does.
    pub fn doubles_against(attacker: Domain, target: Domain, attacker_is_anti_air: bool) -> bool {
        attacker_is_anti_air || (attacker == Domain::Ground && target != Domain::Ground)
    }

    /// The elevation term is skipped when **either** side is domain 2 (`BOTH`/`AIR`)
    /// [measured, the `!= 2` pair guarding the height bonus]. Sea-vs-sea and
    /// sea-vs-ground still take it.
    pub fn elevation_applies(attacker: Domain, target: Domain) -> bool {
        attacker != Domain::BothOrAir && target != Domain::BothOrAir
    }

    /// **The floor of 1 does not apply to a GROUND attacker hitting a SEA target**
    /// [measured, `if (damage < 1) { if ((attacker.domain != 0 || target.domain != 1) && …)
    /// damage = 1; }`]. `ron-data/unitrules.xml`'s `ARMOR` comment names exactly this case:
    /// "exceptions include non-anti-air vs. air, and 'rifleman vs. battleship'".
    ///
    /// Returns true when the attack is allowed to deal literally zero.
    pub fn ground_vs_sea_skips_damage_floor(attacker: Domain, target: Domain) -> bool {
        attacker == Domain::Ground && target == Domain::Sea
    }
}

pub use damage_domain::ground_vs_sea_skips_damage_floor;

/// Whether `attacker` may fire on `target` at `dist_world`, from the type's own
/// `min_range`/`max_range` (`ObjectTypeData +0x1F8` / `+0x1FC`, stored in **TCoords**).
///
/// The min-range band is real for the siege ships: Siegeship, Bomb Vessel, Bomb Ketch and
/// Catapult Ship all ship `4-19rng` or `4-20rng`, i.e. they cannot hit anything inside
/// four tiles [measured, `ron-data/unitrules.xml`].
pub fn in_weapon_band(ty: &NavalUnitType, dist_world: i32) -> bool {
    dist_world >= ty.min_range_world() && dist_world <= ty.max_range_world()
}

// ---------------------------------------------------------------------------
// 12. Fish
// ---------------------------------------------------------------------------

/// The `RESOURCE` ordinals `GoodTypeData::is_herd_type()` `0x004761E0` accepts [measured,
/// the seven `Type::is(index)` probes at `0x004761E5`..`0x00476255`], resolved against
/// `ron-data/resourcerules.xml`.
///
/// Two of them are the water goods: **6 = Fish**, **31 = Whales**. A herd good is gathered
/// by chasing a wandering `Herd` unit rather than by standing on a static node, which is
/// why Fishermen have a wander-following AI and Farmers do not.
pub const HERD_GOOD_INDICES: [i32; 7] = [6, 14, 21, 23, 25, 31, 33];

/// `RESOURCE` ordinal of Fish in `ron-data/resourcerules.xml` — Food +10, Wealth +10,
/// `RANDOM_RARE_DROP 1`.
pub const GOOD_FISH: i32 = 6;
/// `RESOURCE` ordinal of Whales — Food +10, Metal +10, and its own description says it
/// "increases sailing ship speed", the only shipped good that touches naval movement.
pub const GOOD_WHALES: i32 = 31;

/// The two herd units that live on water [measured, `ron-data/unitrules.xml` ordinals 361
/// and 362 under `index = ordinal + 50`]. Both are `DOMAIN Sea` with `HITS 1` and
/// `OBJ_MASK FCW`.
pub const UNIT_HERD_FISH: i32 = 411;
/// See [`UNIT_HERD_FISH`].
pub const UNIT_HERD_WHALES: i32 = 412;

/// The number of spiral entries `Unit::think_fish` scans: `0..=0x120` = 289 = rings 0..8
/// [measured, `cmp local_c, 0x120` at `0x005F4EFF`].
pub const FISH_SPIRAL_LEN: usize = 0x121;

/// The re-think period: `((this->o + Game::frame) & 0x800003FF) == 0`, i.e. **every 1024
/// frames**, phase-offset by the unit's own object index [measured, `0x005F4C8A`].
///
/// The mask includes the sign bit so the test is written as a signed "is zero or is
/// `0x800003FF`-negative-zero" pair in the decompile; on non-negative sums it is exactly
/// `(o + frame) % 1024 == 0`. At 15 frames per game second that is once every ~68 s.
pub fn fish_rethink_due(o: i32, frame: i32) -> bool {
    let v = o.wrapping_add(frame);
    v >= 0 && (v & 0x3FF) == 0
}

/// One candidate cell as `Unit::think_fish` scores it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FishCandidate {
    pub wx: i32,
    pub wy: i32,
    pub score: i32,
    pub spiral_index: usize,
}

/// `Unit::think_fish()` `0x005F4C60`, the target-cell selection [measured for the scoring
/// arithmetic and the RNG placement; STRUCTURE-ONLY for the `calc_gather` pre-pass].
///
/// ```text
/// my_reg = get_tregion(tile(x), tile(y));
/// best = 0; best_i = 0;
/// for i in 0..=0x120:
///     wx = wcell(x) + move_x[i];  wy = wcell(y) + move_y[i];
///     if (!valid_w) continue;
///     if (!is_ocean(wx, wy)) continue;                 ; HALFLAND cells are excluded
///     if (wdata[wy][wx].region != my_reg) continue;
///     down = wdata.down; down_who = wdata.down_who;
///     if (find_good_at(wx, wy, who, 0, 0) < 0) val = 0;
///     else if (down >= 0 && down_who < 8 && down != this->o) continue;   ; occupied
///     else val = 1000000;
///     d = |wcell(x) - wx| + |wcell(y) - wy|;  if (d < 1) d = 1;
///     score = val / d + game_random.get(0, 0xFFFF) % 0x3C + i;
///     if (score > best) { best = score; best_i = i; }
/// if (best_i > 0x120) best_i = game_random.get(0, 0xFFFF) % 0x79 + 0x19;
/// ```
///
/// Three lockstep-critical details. The RNG draw happens **inside the candidate loop, once
/// per surviving candidate**, so the number of ocean cells in range determines how far the
/// shared stream advances — reproduce the filter order exactly or everything downstream
/// shifts. The `+ i` term makes later spiral indices (further cells) win ties, biasing the
/// wander outward. The emitted `best_i > 0x120` fallback is dead under this control flow:
/// `best_i` starts at zero and can only receive `0..=0x120`. It therefore consumes no draw.
pub fn think_fish_pick(
    world: &WaterWorld,
    rng: &mut Random,
    unit: &NavalUnit,
    my_region: i16,
    good_present: &dyn Fn(i32, i32) -> bool,
) -> FishCandidate {
    let (sx, sy) = (wcell_of(unit.x), wcell_of(unit.y));
    let mut best = 0i32;
    let mut best_i = 0usize;
    for i in 0..FISH_SPIRAL_LEN {
        let (dx, dy) = (SPIRAL_X[i], SPIRAL_Y[i]);
        let (wx, wy) = (sx + dx, sy + dy);
        if !world.valid_w(wx, wy) {
            continue;
        }
        if !world.is_ocean(wx, wy) {
            continue;
        }
        let c = world.wcell(wx, wy);
        if c.region != my_region {
            continue;
        }
        let val = if !good_present(wx, wy) {
            0
        } else if c.down >= 0 && c.down_who < 8 && c.down != unit.o {
            continue;
        } else {
            1_000_000
        };
        let mut d = (sx - wx).abs() + (sy - wy).abs();
        if d < 1 {
            d = 1;
        }
        let score = val / d + rng.get(0, 0xFFFF) % 0x3C + i as i32;
        if score > best {
            best = score;
            best_i = i;
        }
    }
    if best_i > 0x120 {
        best_i = (rng.get(0, 0xFFFF) % 0x79 + 0x19) as usize;
    }
    let (dx, dy) = (SPIRAL_X[best_i], SPIRAL_Y[best_i]);
    FishCandidate {
        wx: sx + dx,
        wy: sy + dy,
        score: best,
        spiral_index: best_i,
    }
}

// ---------------------------------------------------------------------------
// 13. The naval unit line
// ---------------------------------------------------------------------------

/// One row of the shipped naval roster.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NavalRosterEntry {
    /// `index = ordinal_in_unitrules_xml + 50`.
    pub index: i32,
    pub name: &'static str,
    /// `<WHERE>` — the building that trains it. `None` means it is only reachable by
    /// upgrade or by the transport/merchant auto-forms.
    pub trained_at: Option<&'static str>,
    /// `<FLAGS>` letters, verbatim. Feed to [`unit_flags_from_letters`].
    pub flag_letters: &'static str,
    /// `<CARRY>`.
    pub carry: i32,
    /// `<RANGE>` low, in TCoords.
    pub min_range_tiles: i32,
    /// `<RANGE>` high, in TCoords.
    pub max_range_tiles: i32,
    /// `<ATTACK>`. Note the engine stores attack **x10** internally; this is the XML value.
    pub attack: i32,
    /// `<HITS>`.
    pub hits: i32,
    /// `<MOVES>`.
    pub moves: i32,
    /// `<POP>`.
    pub pop: i32,
}

/// Every `DOMAIN Sea` unit in `ron-data/unitrules.xml`, in file order [measured, parsed
/// from the shipped XML]. 43 rows.
///
/// The line reads as five families plus two herds:
/// gatherers (Fishermen), merchant fleets, transports (carry 20/30/40, `POP 0`), the light
/// warship ladder (Bark -> Missile Cruiser, plus the Brig/Fluyt/Clipper unique branch),
/// fireships (`UNITTYPE_FIRESHIP`, destroyed on attack), submarines (`UNITTYPE_SUBMARINE`
/// + `CLOAK`), heavy warships (Trireme -> Advanced Battleship), bombard ships with a
/// **4-tile minimum range**, and the Aircraft Carrier (`carry 12`).
pub const NAVAL_ROSTER: [NavalRosterEntry; 43] = [
    NavalRosterEntry {
        index: 317,
        name: "Fishermen",
        trained_at: Some("Dock"),
        flag_letters: "h",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 0,
        attack: 0,
        hits: 170,
        moves: 38,
        pop: 1,
    },
    NavalRosterEntry {
        index: 318,
        name: "Merchant Fleet",
        trained_at: None,
        flag_letters: "h",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 0,
        attack: 0,
        hits: 172,
        moves: 22,
        pop: 1,
    },
    NavalRosterEntry {
        index: 319,
        name: "Modern Merchant Fleet",
        trained_at: None,
        flag_letters: "hj",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 0,
        attack: 0,
        hits: 230,
        moves: 22,
        pop: 1,
    },
    NavalRosterEntry {
        index: 320,
        name: "Transport Barge",
        trained_at: None,
        flag_letters: "h",
        carry: 20,
        min_range_tiles: 0,
        max_range_tiles: 0,
        attack: 0,
        hits: 50,
        moves: 25,
        pop: 0,
    },
    NavalRosterEntry {
        index: 321,
        name: "Transport Galleon",
        trained_at: None,
        flag_letters: "hj",
        carry: 30,
        min_range_tiles: 0,
        max_range_tiles: 0,
        attack: 0,
        hits: 90,
        moves: 25,
        pop: 0,
    },
    NavalRosterEntry {
        index: 322,
        name: "Transport Freighter",
        trained_at: None,
        flag_letters: "hj",
        carry: 40,
        min_range_tiles: 0,
        max_range_tiles: 0,
        attack: 0,
        hits: 130,
        moves: 25,
        pop: 0,
    },
    NavalRosterEntry {
        index: 323,
        name: "Bark",
        trained_at: Some("Dock"),
        flag_letters: "hvg",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 5,
        attack: 11,
        hits: 140,
        moves: 47,
        pop: 1,
    },
    NavalRosterEntry {
        index: 324,
        name: "Dromon",
        trained_at: Some("Dock"),
        flag_letters: "jvg",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 6,
        attack: 12,
        hits: 145,
        moves: 50,
        pop: 1,
    },
    NavalRosterEntry {
        index: 325,
        name: "Caravel",
        trained_at: Some("Dock"),
        flag_letters: "jvzg",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 7,
        attack: 13,
        hits: 160,
        moves: 53,
        pop: 1,
    },
    NavalRosterEntry {
        index: 326,
        name: "Corvette",
        trained_at: Some("Dock"),
        flag_letters: "jvzg",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 8,
        attack: 14,
        hits: 175,
        moves: 54,
        pop: 1,
    },
    NavalRosterEntry {
        index: 327,
        name: "Sloop",
        trained_at: Some("Dock"),
        flag_letters: "jvzg",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 9,
        attack: 17,
        hits: 200,
        moves: 57,
        pop: 1,
    },
    NavalRosterEntry {
        index: 328,
        name: "Destroyer",
        trained_at: Some("Dock"),
        flag_letters: "vjzg",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 11,
        attack: 18,
        hits: 205,
        moves: 63,
        pop: 1,
    },
    NavalRosterEntry {
        index: 329,
        name: "Cruiser",
        trained_at: Some("Dock"),
        flag_letters: "vjg",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 13,
        attack: 19,
        hits: 210,
        moves: 67,
        pop: 1,
    },
    NavalRosterEntry {
        index: 330,
        name: "Missile Cruiser",
        trained_at: Some("Dock"),
        flag_letters: "vjg",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 14,
        attack: 20,
        hits: 215,
        moves: 69,
        pop: 1,
    },
    NavalRosterEntry {
        index: 331,
        name: "Brig",
        trained_at: Some("Dock"),
        flag_letters: "jvzgy",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 7,
        attack: 14,
        hits: 165,
        moves: 54,
        pop: 1,
    },
    NavalRosterEntry {
        index: 332,
        name: "Fluyt",
        trained_at: Some("Dock"),
        flag_letters: "jvzgy",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 8,
        attack: 15,
        hits: 180,
        moves: 55,
        pop: 1,
    },
    NavalRosterEntry {
        index: 333,
        name: "Clipper",
        trained_at: Some("Dock"),
        flag_letters: "jvzgy",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 9,
        attack: 18,
        hits: 205,
        moves: 58,
        pop: 1,
    },
    NavalRosterEntry {
        index: 334,
        name: "Fire Raft",
        trained_at: Some("Dock"),
        flag_letters: "nhz",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 1,
        attack: 53,
        hits: 120,
        moves: 38,
        pop: 2,
    },
    NavalRosterEntry {
        index: 335,
        name: "Heavy Fire Raft",
        trained_at: Some("Dock"),
        flag_letters: "njz",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 1,
        attack: 62,
        hits: 140,
        moves: 40,
        pop: 2,
    },
    NavalRosterEntry {
        index: 336,
        name: "Fireship",
        trained_at: Some("Dock"),
        flag_letters: "njz",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 1,
        attack: 71,
        hits: 170,
        moves: 42,
        pop: 2,
    },
    NavalRosterEntry {
        index: 337,
        name: "Heavy Fireship",
        trained_at: Some("Dock"),
        flag_letters: "njz",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 1,
        attack: 80,
        hits: 210,
        moves: 44,
        pop: 2,
    },
    NavalRosterEntry {
        index: 338,
        name: "Submarine",
        trained_at: Some("Dock"),
        flag_letters: "ujo",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 9,
        attack: 96,
        hits: 250,
        moves: 46,
        pop: 2,
    },
    NavalRosterEntry {
        index: 339,
        name: "Attack Submarine",
        trained_at: Some("Dock"),
        flag_letters: "ujo",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 12,
        attack: 103,
        hits: 290,
        moves: 50,
        pop: 2,
    },
    NavalRosterEntry {
        index: 340,
        name: "Trireme",
        trained_at: Some("Dock"),
        flag_letters: "hg",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 9,
        attack: 20,
        hits: 180,
        moves: 35,
        pop: 2,
    },
    NavalRosterEntry {
        index: 341,
        name: "Galley",
        trained_at: Some("Dock"),
        flag_letters: "jg",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 9,
        attack: 22,
        hits: 210,
        moves: 37,
        pop: 2,
    },
    NavalRosterEntry {
        index: 342,
        name: "Carrack",
        trained_at: Some("Dock"),
        flag_letters: "jg",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 9,
        attack: 24,
        hits: 250,
        moves: 39,
        pop: 2,
    },
    NavalRosterEntry {
        index: 343,
        name: "Frigate",
        trained_at: Some("Dock"),
        flag_letters: "jzg",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 10,
        attack: 26,
        hits: 270,
        moves: 41,
        pop: 2,
    },
    NavalRosterEntry {
        index: 344,
        name: "Man o' War",
        trained_at: Some("Dock"),
        flag_letters: "jzg",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 11,
        attack: 29,
        hits: 300,
        moves: 43,
        pop: 2,
    },
    NavalRosterEntry {
        index: 345,
        name: "Siegeship",
        trained_at: Some("Dock"),
        flag_letters: "h",
        carry: 0,
        min_range_tiles: 4,
        max_range_tiles: 19,
        attack: 29,
        hits: 200,
        moves: 29,
        pop: 2,
    },
    NavalRosterEntry {
        index: 346,
        name: "Bomb Vessel",
        trained_at: Some("Dock"),
        flag_letters: "h",
        carry: 0,
        min_range_tiles: 4,
        max_range_tiles: 19,
        attack: 29,
        hits: 200,
        moves: 29,
        pop: 2,
    },
    NavalRosterEntry {
        index: 347,
        name: "Bomb Ketch",
        trained_at: Some("Dock"),
        flag_letters: "j",
        carry: 0,
        min_range_tiles: 4,
        max_range_tiles: 20,
        attack: 34,
        hits: 220,
        moves: 32,
        pop: 2,
    },
    NavalRosterEntry {
        index: 348,
        name: "Dreadnought",
        trained_at: Some("Dock"),
        flag_letters: "jzg",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 22,
        attack: 35,
        hits: 325,
        moves: 44,
        pop: 2,
    },
    NavalRosterEntry {
        index: 349,
        name: "Battleship",
        trained_at: Some("Dock"),
        flag_letters: "jzg",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 24,
        attack: 36,
        hits: 335,
        moves: 47,
        pop: 2,
    },
    NavalRosterEntry {
        index: 350,
        name: "Advanced Battleship",
        trained_at: Some("Dock"),
        flag_letters: "j",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 26,
        attack: 41,
        hits: 375,
        moves: 49,
        pop: 2,
    },
    NavalRosterEntry {
        index: 351,
        name: "Aircraft Carrier",
        trained_at: Some("Dock"),
        flag_letters: "h",
        carry: 12,
        min_range_tiles: 0,
        max_range_tiles: 9,
        attack: 60,
        hits: 600,
        moves: 42,
        pop: 2,
    },
    NavalRosterEntry {
        index: 377,
        name: "Ship of the Line",
        trained_at: None,
        flag_letters: "jzg",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 13,
        attack: 32,
        hits: 315,
        moves: 43,
        pop: 2,
    },
    NavalRosterEntry {
        index: 389,
        name: "Patrol Boat",
        trained_at: None,
        flag_letters: "vjg",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 10,
        attack: 15,
        hits: 170,
        moves: 67,
        pop: 1,
    },
    NavalRosterEntry {
        index: 396,
        name: "Galleon",
        trained_at: None,
        flag_letters: "jzg",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 10,
        attack: 26,
        hits: 320,
        moves: 41,
        pop: 2,
    },
    NavalRosterEntry {
        index: 397,
        name: "Ironclad",
        trained_at: None,
        flag_letters: "cgjvz",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 9,
        attack: 27,
        hits: 250,
        moves: 46,
        pop: 1,
    },
    NavalRosterEntry {
        index: 398,
        name: "Nuclear Missile Sub",
        trained_at: None,
        flag_letters: "ujo",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 12,
        attack: 103,
        hits: 290,
        moves: 50,
        pop: 2,
    },
    NavalRosterEntry {
        index: 399,
        name: "Catapult Ship",
        trained_at: None,
        flag_letters: "j",
        carry: 0,
        min_range_tiles: 4,
        max_range_tiles: 20,
        attack: 34,
        hits: 220,
        moves: 32,
        pop: 2,
    },
    NavalRosterEntry {
        index: 411,
        name: "Herd Fish",
        trained_at: Some("Large City"),
        flag_letters: "a",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 0,
        attack: 0,
        hits: 1,
        moves: 25,
        pop: 1,
    },
    NavalRosterEntry {
        index: 412,
        name: "Herd Whales",
        trained_at: Some("Large City"),
        flag_letters: "a",
        carry: 0,
        min_range_tiles: 0,
        max_range_tiles: 0,
        attack: 0,
        hits: 1,
        moves: 25,
        pop: 1,
    },
];

impl NavalRosterEntry {
    /// Build the runtime type record this module's predicates take.
    pub fn to_type(&self) -> NavalUnitType {
        NavalUnitType {
            index: self.index,
            domain: Domain::Sea,
            unit_flags: unit_flags_from_letters(self.flag_letters),
            unit_flags2: 0,
            carry: self.carry,
            carry_size: 0,
            min_range_tiles: self.min_range_tiles,
            max_range_tiles: self.max_range_tiles,
            moves: self.moves,
        }
    }
}

/// Look a roster row up by type index.
pub fn naval_roster(index: i32) -> Option<&'static NavalRosterEntry> {
    NAVAL_ROSTER.iter().find(|e| e.index == index)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn ocean_world(xs: i32, ys: i32) -> WaterWorld {
        let mut w = WaterWorld::new(xs, ys);
        for y in 0..ys {
            for x in 0..xs {
                let cell = w.wcell_mut(x, y);
                cell.land = 1;
                cell.region = 1;
                cell.down = -1;
                cell.down_who = -1;
            }
        }
        for ty in 0..w.tile_ys() {
            for tx in 0..w.tile_xs() {
                *w.tmask_mut(tx, ty) = tmask::COVER_OCEAN;
            }
        }
        w
    }

    fn make_land(w: &mut WaterWorld, wx: i32, wy: i32) {
        w.wcell_mut(wx, wy).land = 0;
        for r in 0..4 {
            for c in 0..4 {
                *w.tmask_mut(wx * 4 + c, wy * 4 + r) = 0;
            }
        }
    }

    // -- coordinate ladder ------------------------------------------------

    #[test]
    fn div3_table_identities_hold() {
        // The two identities that pin `div_3_table[n] == n / 3`.
        for world in (0..200_000).step_by(37) {
            assert_eq!(tile_of(world), world / TILE, "tile at {world}");
            assert_eq!(wcell_of(world), world / WCELL, "wcell at {world}");
            assert_eq!(ugrid_of(world), world / UGRID, "ugrid at {world}");
        }
    }

    #[test]
    fn cell_centres_match_the_engine_arithmetic() {
        assert_eq!(wcell_center(0), 0x180);
        assert_eq!(wcell_center(3), 3 * 0x300 + 0x180);
        assert_eq!(ugrid_center(0), 0x18);
        assert_eq!(ugrid_center(5), 5 * 0x30 + 0x18);
    }

    #[test]
    fn coord_obfuscation_round_trips() {
        assert_eq!(deobfuscate(deobfuscate(12345)), 12345);
        assert_eq!(COORD_XOR, 0x63637);
    }

    // -- spiral tables ----------------------------------------------------

    #[test]
    fn spiral_ring_one_is_the_astar_neighbourhood() {
        for (k, (x, y)) in A_STAR_NEIGHBOURS.iter().enumerate() {
            assert_eq!((SPIRAL_X[k + 1], SPIRAL_Y[k + 1]), (*x, *y));
        }
        assert_eq!((SPIRAL_X[0], SPIRAL_Y[0]), (0, 0));
    }

    #[test]
    fn spiral_rings_have_the_right_cardinalities() {
        // Chebyshev radius by index band: ring r occupies 8r entries after the centre.
        let cheb = |i: usize| SPIRAL_X[i].abs().max(SPIRAL_Y[i].abs());
        assert_eq!(cheb(0), 0);
        for radius in 1..=7usize {
            let start = (2 * radius - 1).pow(2);
            let end = (2 * radius + 1).pow(2) - 1;
            for i in start..=end {
                assert_eq!(cheb(i), radius as i32, "index {i} should be ring {radius}");
            }
        }
        for i in 225..=287 {
            assert_eq!(cheb(i), 8, "index {i} should be ring 8");
        }
        // Shipped table anomaly: the final Y is -16 rather than generated-ring -7.
        assert_eq!((SPIRAL_X[288], SPIRAL_Y[288]), (-8, -16));
    }

    #[test]
    fn spiral_ring_entries_are_distinct() {
        let mut seen = std::collections::HashSet::new();
        for i in 0..FISH_SPIRAL_LEN {
            assert!(seen.insert((SPIRAL_X[i], SPIRAL_Y[i])), "duplicate at {i}");
        }
        assert_eq!(seen.len(), FISH_SPIRAL_LEN);
    }

    #[test]
    fn spiral_table_bytes_match_the_shipped_pe_digest() {
        // FNV-1a over SPIRAL_X then SPIRAL_Y as little-endian i32 bytes. The expected
        // value was computed directly from riseofnations.exe at 0xADCAF0/0xADC400.
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for value in SPIRAL_X.into_iter().chain(SPIRAL_Y) {
            for byte in value.to_le_bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
        assert_eq!(hash, 0xE72D_E577_7268_87FC);
    }

    #[test]
    fn orthog_is_the_four_cardinals() {
        let dirs: Vec<(i32, i32)> = (1..=4).map(|k| (ORTHOG_X[k], ORTHOG_Y[k])).collect();
        assert_eq!(dirs, vec![(0, -1), (1, 0), (0, 1), (-1, 0)]);
    }

    // -- water predicates -------------------------------------------------

    #[test]
    fn is_ocean_rejects_halfland_cells() {
        let mut w = ocean_world(4, 4);
        assert!(w.is_ocean(1, 1));
        w.wcell_mut(1, 1).flags |= wflag::HALFLAND;
        assert!(
            !w.is_ocean(1, 1),
            "HALFLAND early-out must win over land code"
        );
    }

    #[test]
    fn both_water_land_codes_read_as_ocean() {
        let mut w = ocean_world(2, 2);
        for code in LAND_WATER_CODES {
            w.wcell_mut(0, 0).land = code;
            assert!(w.is_ocean(0, 0), "land code {code}");
        }
        w.wcell_mut(0, 0).land = 3;
        assert!(!w.is_ocean(0, 0));
    }

    #[test]
    fn is_tocean_slow_takes_the_cell_fast_path() {
        let mut w = ocean_world(2, 2);
        // Clear every tile bit but leave the cell flagged as water.
        for ty in 0..w.tile_ys() {
            for tx in 0..w.tile_xs() {
                *w.tmask_mut(tx, ty) = 0;
            }
        }
        assert!(!w.is_tocean(0, 0), "the tile bit is clear");
        assert!(
            w.is_tocean_slow(0, 0),
            "the cell fast path still says ocean"
        );
    }

    #[test]
    fn num_waterhalf_counts_only_on_halfland() {
        let mut w = ocean_world(2, 2);
        assert_eq!(w.num_waterhalf(0, 0), 0, "no HALFLAND flag -> zero");
        w.wcell_mut(0, 0).flags |= wflag::HALFLAND;
        assert_eq!(w.num_waterhalf(0, 0), 16);
        *w.tmask_mut(0, 0) = 0;
        *w.tmask_mut(3, 3) = 0;
        assert_eq!(w.num_waterhalf(0, 0), 14);
    }

    #[test]
    fn set_waterhalf_marks_the_cell_and_clear_reverts_the_tiles() {
        let mut w = WaterWorld::new(2, 2);
        w.set_waterhalf(1, 2, true);
        assert!(w.is_tocean(1, 2));
        assert!(w.wcell(0, 0).flags & wflag::HALFLAND != 0);
        w.clear_waterhalf(0, 0, false);
        assert!(!w.is_tocean(1, 2));
        assert_eq!(w.wcell(0, 0).flags & wflag::HALFLAND, 0);
    }

    #[test]
    fn clear_waterhalf_keep_tiles_only_drops_the_flag() {
        let mut w = WaterWorld::new(2, 2);
        w.set_waterhalf(1, 2, true);
        w.clear_waterhalf(0, 0, true);
        assert!(w.is_tocean(1, 2), "tiles survive when keep_tiles is set");
        assert_eq!(w.wcell(0, 0).flags & wflag::HALFLAND, 0);
    }

    #[test]
    fn set_land_coast_also_sets_original_coast() {
        let mut w = WaterWorld::new(2, 2);
        w.set_land(0, 0, 0, 0, wflag::COAST as i32, false);
        let f = w.wcell(0, 0).flags;
        assert!(f & wflag::COAST != 0);
        assert!(
            f & wflag::ORIGINAL_COAST != 0,
            "revert_coast depends on this"
        );
    }

    #[test]
    fn is_blocked_at_mode_one_forgives_trees() {
        let mut w = WaterWorld::new(1, 1);
        *w.tmask_mut(0, 0) = tmask::BLOCKED | tmask::COVER_TREE;
        assert!(w.is_blocked_at(0, 0, false));
        assert!(!w.is_blocked_at(0, 0, true));
    }

    // -- domain ------------------------------------------------------------

    #[test]
    fn domain_both_and_air_alias() {
        assert_eq!(Domain::BothOrAir.as_i32(), 2);
        assert_eq!(Domain::from_i32(2), Some(Domain::BothOrAir));
        assert_eq!(
            Domain::from_i32(3),
            None,
            "NUM_DOMAIN is a count, not a domain"
        );
        assert_eq!(Domain::from_i32(4), Some(Domain::RealAir));
        assert_eq!(Domain::FIELD_OFFSET, 0x218);
    }

    // -- flag letter law ---------------------------------------------------

    #[test]
    fn unit_flag_letters_match_the_pdb_enum() {
        assert_eq!(unit_flags_from_letters("a"), unit_flag::IGNORETERRAIN);
        assert_eq!(unit_flags_from_letters("e"), unit_flag::TRANSPORT);
        assert_eq!(unit_flags_from_letters("n"), unit_flag::FIRESHIP);
        assert_eq!(unit_flags_from_letters("u"), unit_flag::SUBMARINE);
        assert_eq!(unit_flags_from_letters("z"), unit_flag::ROCKING_ATTACK);
        assert_eq!(unit_flags_from_letters("1"), unit_flag::GOVERNMENT_HERO);
    }

    #[test]
    fn shipped_fireships_and_subs_carry_the_right_bits() {
        // Every Fire Raft / Fireship has 'n'; every Submarine has 'u' and 'o'.
        for e in NAVAL_ROSTER {
            let f = unit_flags_from_letters(e.flag_letters);
            if e.name.contains("Fire") {
                assert!(f & unit_flag::FIRESHIP != 0, "{} lacks FIRESHIP", e.name);
            }
            if e.name.contains("Sub") {
                assert!(f & unit_flag::SUBMARINE != 0, "{} lacks SUBMARINE", e.name);
                assert!(f & unit_flag::CLOAK != 0, "{} lacks CLOAK", e.name);
            }
        }
    }

    #[test]
    fn dock_and_shipyard_are_the_sea_buildings() {
        // 'ebn' and 'ecbn' from ron-data/buildingrules.xml.
        let dock = unit_flags_from_letters("ebn");
        let shipyard = unit_flags_from_letters("ecbn");
        assert!(build_allowed_on_sea(dock));
        assert!(build_allowed_on_sea(shipyard));
        assert_eq!(dock & build_flag::OUTSIDE, build_flag::OUTSIDE);
        assert_eq!(dock & build_flag::NOT_CAPTURE, build_flag::NOT_CAPTURE);
        assert_eq!(shipyard & build_flag::GLOBAL, build_flag::GLOBAL);
        // A Farm ('ag' say) must not be sea-placeable.
        assert!(!build_allowed_on_sea(unit_flags_from_letters("ag")));
    }

    // -- transports --------------------------------------------------------

    fn barge() -> NavalUnitType {
        naval_roster(320).unwrap().to_type()
    }

    #[test]
    fn can_transport_takes_either_arm() {
        let ty = barge();
        assert!(!ty.is_transport_type(), "the XML barge lacks 'e'");
        let mut u = NavalUnit {
            o: 1,
            who: 0,
            unit_masks: 0,
            unit_masks2: 0,
            x: 0,
            y: 0,
        };
        assert!(!can_transport(&u, &ty));
        u.unit_masks |= unit_mask::CAN_TRANSPORT;
        assert!(can_transport(&u, &ty));
        u.unit_masks2 |= unit_mask2::DISABLE_TRANSPORT;
        assert!(!can_transport(&u, &ty), "DISABLE vetoes the instance bit");
        // But a real transport type wins regardless.
        let mut engine_set = ty;
        engine_set.unit_flags |= unit_flag::TRANSPORT;
        assert!(can_transport(&u, &engine_set));
    }

    #[test]
    fn transport_type_civilian_list_matches_the_shipped_units() {
        for idx in CIVILIAN_CARGO_TYPE_INDICES {
            assert_eq!(
                transport_type(idx, false, false),
                TransportType::Civilian,
                "index {idx}"
            );
        }
        // A generic soldier is military cargo.
        assert_eq!(transport_type(100, false, false), TransportType::Military);
        // The scout ability wins over everything.
        assert_eq!(transport_type(100, true, false), TransportType::Scout);
        assert_eq!(transport_type(50, true, false), TransportType::Scout);
        // Ability 0x3B demotes to civilian.
        assert_eq!(transport_type(100, false, true), TransportType::Civilian);
    }

    #[test]
    fn leader_transport_is_a_ladder() {
        use leader_flag::*;
        assert_eq!(leader_transport_level(0), TransportType::None);
        assert_eq!(
            leader_transport_level(CAN_TRANSPORT_SCT),
            TransportType::Scout
        );
        assert_eq!(
            leader_transport_level(CAN_TRANSPORT_MIL),
            TransportType::Military
        );
        assert_eq!(
            leader_transport_level(CAN_TRANSPORT_CIV),
            TransportType::Civilian
        );
        // Civilian dominates even with every bit set.
        assert_eq!(
            leader_transport_level(CAN_TRANSPORT_CIV | CAN_TRANSPORT_MIL | CAN_TRANSPORT_SCT),
            TransportType::Civilian
        );
    }

    #[test]
    fn needs_transport_returns_the_three_verdicts() {
        let mut w = ocean_world(4, 4);
        make_land(&mut w, 0, 0);
        // land -> land
        assert_eq!(needs_transport(&w, (0, 0), (1, 1)), TransportNeed::None);
        // water -> water
        assert_eq!(needs_transport(&w, (8, 8), (9, 9)), TransportNeed::None);
        // land -> water = embark (2)
        assert_eq!(needs_transport(&w, (0, 0), (8, 8)), TransportNeed::Embark);
        // water -> land = disembark (1)
        assert_eq!(
            needs_transport(&w, (8, 8), (0, 0)),
            TransportNeed::Disembark
        );
        // identical tiles short-circuit
        assert_eq!(needs_transport(&w, (0, 0), (0, 0)), TransportNeed::None);
    }

    #[test]
    fn set_transport_needs_leader_tech() {
        let ty = NavalUnitType {
            index: 100,
            domain: Domain::Ground,
            unit_flags: 0,
            unit_flags2: 0,
            carry: 0,
            carry_size: 0,
            min_range_tiles: 0,
            max_range_tiles: 0,
            moves: 30,
        };
        let mut u = NavalUnit {
            o: 1,
            who: 0,
            unit_masks: 0,
            unit_masks2: 0,
            x: 0,
            y: 0,
        };
        action_set_transport(&mut u, &ty, 0, true, true, false);
        assert_eq!(
            u.unit_masks & unit_mask::CAN_TRANSPORT,
            0,
            "no tech, no bit"
        );
        action_set_transport(
            &mut u,
            &ty,
            leader_flag::CAN_TRANSPORT_MIL,
            true,
            true,
            false,
        );
        assert_ne!(u.unit_masks & unit_mask::CAN_TRANSPORT, 0);
        action_set_transport(
            &mut u,
            &ty,
            leader_flag::CAN_TRANSPORT_MIL,
            false,
            true,
            false,
        );
        assert_eq!(u.unit_masks & unit_mask::CAN_TRANSPORT, 0);
    }

    #[test]
    fn can_ever_transport_is_domain_first() {
        let ground = NavalUnitType {
            index: 100,
            domain: Domain::Ground,
            unit_flags: 0,
            unit_flags2: 0,
            carry: 0,
            carry_size: 0,
            min_range_tiles: 0,
            max_range_tiles: 0,
            moves: 30,
        };
        assert!(
            can_ever_transport(&ground, true, true),
            "ground is always eligible"
        );
        let carrier = naval_roster(351).unwrap().to_type();
        assert_eq!(carrier.carry, 12);
        assert!(can_ever_transport(&carrier, true, false));
        assert!(
            !can_ever_transport(&carrier, true, true),
            "ability 0x15F vetoes"
        );
        let warship = naval_roster(349).unwrap().to_type();
        assert!(!can_ever_transport(&warship, true, false), "carry == 0");
    }

    #[test]
    fn in_a_ship_keys_off_the_carriers_domain() {
        assert!(!in_a_ship(None));
        assert!(!in_a_ship(Some((true, Domain::Ground))));
        assert!(!in_a_ship(Some((false, Domain::Sea))), "must be a unit");
        assert!(in_a_ship(Some((true, Domain::Sea))));
    }

    // -- boarding ----------------------------------------------------------

    #[test]
    fn board_step_proxy_only_classifies_the_retail_branch() {
        assert_eq!(classify_board_step_proxy(true, true), BoardStep::Rendezvous);
        assert_eq!(classify_board_step_proxy(false, true), BoardStep::GoInside);
        assert_eq!(classify_board_step_proxy(false, false), BoardStep::Abandon);
    }

    #[test]
    fn await_board_requires_a_matching_board_order() {
        let ship = TargetRef {
            ox: 7,
            whom: 1,
            uid: 42,
        };
        let good = PassengerView {
            me: TargetRef {
                ox: 3,
                whom: 1,
                uid: 9,
            },
            is_unit: true,
            carriable: true,
            current_action_order: ORDER_BOARD_SHIP,
            board_target: ship,
        };
        assert!(await_board_still_valid(ship, Some(good)));
        assert!(!await_board_still_valid(ship, None));
        let wrong_order = PassengerView {
            current_action_order: 1,
            ..good
        };
        assert!(!await_board_still_valid(ship, Some(wrong_order)));
        let wrong_target = PassengerView {
            board_target: TargetRef {
                ox: 8,
                whom: 1,
                uid: 42,
            },
            ..good
        };
        assert!(!await_board_still_valid(ship, Some(wrong_target)));
        let not_carriable = PassengerView {
            carriable: false,
            ..good
        };
        assert!(!await_board_still_valid(ship, Some(not_carriable)));
        let itself = PassengerView { me: ship, ..good };
        assert!(!await_board_still_valid(ship, Some(itself)));
    }

    #[test]
    fn meet_regimes_band_by_distance() {
        assert_eq!(meet_regime(10, 32), MeetRegime::AlreadyMet);
        assert_eq!(meet_regime(100, 32), MeetRegime::Midpoint);
        assert_eq!(meet_regime(0xC0, 32), MeetRegime::CoastSearch);
        assert_eq!(meet_regime(0x8FF, 32), MeetRegime::CoastSearch);
        assert_eq!(meet_regime(0x900, 32), MeetRegime::CoarseOffsets);
    }

    #[test]
    fn midpoint_rendezvous_snaps_to_the_unit_grid() {
        let (x, y) = midpoint_rendezvous((0, 0), (192, 192));
        assert_eq!(x % UGRID, UGRID / 2);
        assert_eq!(y % UGRID, UGRID / 2);
        assert_eq!((x, y), (ugrid_center(2), ugrid_center(2)));
    }

    // -- docks -------------------------------------------------------------

    #[test]
    fn dock_default_is_the_free_slot_sentinel() {
        let d = Dock::default();
        assert_eq!((d.dock, d.o, d.reg, d.gull_o), (-1, -1, -1, -1));
        assert_eq!(d.dock_flags, 0);
        assert_eq!(d.who, 0xFF);
    }

    #[test]
    fn dock_placement_needs_water_with_a_clear_shore() {
        let mut w = ocean_world(5, 5);
        // Open ocean everywhere: no shore anywhere, so no dock site.
        assert!(!is_dock_tile(&w, 2, 2), "open water has no shore");
        // Put land at (3,2): now (2,2) has a shore to its east.
        make_land(&mut w, 3, 2);
        assert!(is_dock_tile(&w, 2, 2));
        // A dock cell must itself be water.
        assert!(!is_dock_tile(&w, 3, 2));
    }

    #[test]
    fn dock_placement_rejects_mountain_and_forest_shores() {
        let mut w = ocean_world(5, 5);
        make_land(&mut w, 3, 2);
        assert!(is_dock_tile(&w, 2, 2));
        // The facing tile of the land cell is (12, 8) — its west edge column.
        *w.tmask_mut(12, 8) = tmask::FEATURE_MOUNTAIN;
        assert!(!is_dock_tile(&w, 2, 2), "mountain shore is refused");
        *w.tmask_mut(12, 8) = tmask::COVER_TREE;
        assert!(!is_dock_tile(&w, 2, 2), "forest shore is refused");
        *w.tmask_mut(12, 8) = 0;
        assert!(is_dock_tile(&w, 2, 2));
    }

    #[test]
    fn dock_placement_rejects_a_built_up_apron() {
        let mut w = ocean_world(5, 5);
        make_land(&mut w, 3, 2);
        assert!(is_dock_tile(&w, 2, 2));
        // Any building anywhere in the 4x2 apron kills the site.
        *w.tmask_mut(13, 10) = tmask::FEATURE_BUILDING;
        assert!(!is_dock_tile(&w, 2, 2));
        *w.tmask_mut(13, 10) = 0;
        assert!(is_dock_tile(&w, 2, 2));
        *w.tmask_mut(12, 11) = tmask::BUILT;
        assert!(!is_dock_tile(&w, 2, 2));
    }

    #[test]
    fn dock_placement_works_on_all_four_sides() {
        for (lx, ly) in [(3, 2), (1, 2), (2, 3), (2, 1)] {
            let mut w = ocean_world(5, 5);
            make_land(&mut w, lx, ly);
            assert!(is_dock_tile(&w, 2, 2), "land at ({lx},{ly})");
        }
    }

    #[test]
    fn dock_registry_tracks_regions_and_reuses_slots() {
        let mut docks = Docks::new();
        let mut leader = LeaderNaval::default();
        let a = docks.init_dock_registry_only(&mut leader, 0, 11, 4);
        let b = docks.init_dock_registry_only(&mut leader, 0, 12, 4);
        assert_eq!((a, b), (0, 1));
        assert_eq!(leader.reg_docks[4], 2);
        assert_eq!(leader.dock_mark, 2);
        assert_eq!(Docks::dock_regions(&leader), vec![4]);

        docks.close_dock_registry_only(&mut leader, 0, a);
        assert_eq!(leader.reg_docks[4], 1);
        // Slot 0 is free again and must be reused before the tail grows.
        let c = docks.init_dock_registry_only(&mut leader, 0, 13, 7);
        assert_eq!(c, 0, "the free-slot scan is a linear walk from index 0");
        assert_eq!(leader.reg_docks[7], 1);
        assert_eq!(docks.lists[0].len(), 2, "no new slot was allocated");
    }

    #[test]
    fn dock_close_leaves_reg_at_zero_not_minus_one() {
        let mut docks = Docks::new();
        let mut leader = LeaderNaval::default();
        let s = docks.init_dock_registry_only(&mut leader, 0, 5, 9);
        docks.close_dock_registry_only(&mut leader, 0, s);
        let d = docks.lists[0].get(s as usize).unwrap();
        assert_eq!(d.reg, 0, "Dock::close writes 0, matching retail");
        assert_eq!(d.o, -1);
        assert_eq!(d.who, 0xFF);
        assert_eq!(d.dock_flags & DOCK_FLAG_IN_USE, 0);
        assert_eq!(leader.dock_mark, 0, "the tail is popped");
    }

    #[test]
    fn dock_array_uses_the_engine_growth_policy() {
        let mut docks = Docks::new();
        let mut leader = LeaderNaval::default();
        for i in 0..6 {
            docks.init_dock_registry_only(&mut leader, 3, i, 1);
        }
        let (len, size, incr, _flags) = docks.lists[3].checksum_header();
        assert_eq!(len, 6);
        // 5 -> 10, never Rust's 8. This is the whole reason EngineArray exists.
        assert_eq!(size, 10, "capacity is checksummed; a Vec would say 8");
        assert_eq!(incr, crate::container::INCREMENT_DOUBLE);
    }

    #[test]
    fn dock_region_cap_is_respected() {
        let mut docks = Docks::new();
        let mut leader = LeaderNaval::default();
        // reg >= 64 is not counted (`cmp reg, 0x40` in Dock::init).
        docks.init_dock_registry_only(&mut leader, 0, 1, 64);
        assert_eq!(leader.reg_docks.iter().sum::<u16>(), 0);
        docks.init_dock_registry_only(&mut leader, 0, 2, 63);
        assert_eq!(leader.reg_docks[63], 1);
    }

    #[test]
    fn dock_gull_draw_is_conditional_and_exactly_one_rng_step() {
        let mut rng = Random::new(1);
        assert_eq!(Dock::gull_angle_after_spawn(&mut rng, -1), None);
        assert_eq!(rng.state(), 1, "failed gull spawn consumes no RNG");

        let angle = Dock::gull_angle_after_spawn(&mut rng, 42).expect("spawn succeeded");
        let mut counter = Random::new(1);
        let draw = counter.get(0, 0xFFFF);
        assert_eq!(
            rng.state(),
            counter.state(),
            "successful spawn consumes one draw"
        );
        assert_eq!(
            angle,
            (draw % 7)
                .wrapping_mul(0x0AAA_AAAA)
                .wrapping_sub(0x4000_0000)
        );
    }

    // -- pathing -----------------------------------------------------------

    #[test]
    fn astar_steps_are_the_domain_selector() {
        assert_eq!(WATER_ASTAR_STEP, WCELL);
        assert_eq!(TILE_ASTAR_STEP, TILE);
        assert_eq!(UNIT_ASTAR_STEP, UGRID);
    }

    #[test]
    fn coarse_step_switches_at_one_wcell() {
        assert_eq!(coarse_step(0x2FF), 0x30);
        assert_eq!(coarse_step(0x300), 0x180);
    }

    #[test]
    fn find_wpath_short_circuits_within_one_cell() {
        let w = ocean_world(4, 4);
        let mut stack = vec![PathData {
            to_x: 100,
            to_y: 100,
            tolerance: 0,
            flags: 0,
        }];
        let mut pfd = WaterSearchFlags::default();
        let (out, n) = find_wpath_entry_proxy(&w, &mut stack, 50, 50, 0, &mut pfd);
        assert_eq!(out, WPathProxyOutcome::NoRouteNeeded);
        assert_eq!(n, 1);
        assert_eq!(stack.len(), 1, "the waypoint is pushed back untouched");
        assert_eq!(pfd.sx, tile_of(50));
    }

    #[test]
    fn find_wpath_short_circuits_for_helicopters() {
        let w = ocean_world(4, 4);
        let mut stack = vec![PathData {
            to_x: 2000,
            to_y: 2000,
            tolerance: 0,
            flags: 0,
        }];
        let mut pfd = WaterSearchFlags::default();
        let (out, _) =
            find_wpath_entry_proxy(&w, &mut stack, 50, 50, unit_flag::HELICOPTER, &mut pfd);
        assert_eq!(out, WPathProxyOutcome::NoRouteNeeded);
    }

    #[test]
    fn find_wpath_empties_the_stack_off_map() {
        let w = ocean_world(2, 2);
        let mut stack = vec![PathData {
            to_x: 99_999,
            to_y: 0,
            tolerance: 0,
            flags: 0,
        }];
        let mut pfd = WaterSearchFlags::default();
        let (out, n) = find_wpath_entry_proxy(&w, &mut stack, 50, 50, 0, &mut pfd);
        assert_eq!(out, WPathProxyOutcome::OffMap);
        assert_eq!(n, -1);
        assert!(
            stack.is_empty(),
            "retail sets stack.len = 0 before returning -1"
        );
    }

    #[test]
    fn find_wpath_proxy_refuses_to_claim_the_missing_full_search_ran() {
        let w = ocean_world(4, 4);
        let mut stack = vec![PathData {
            to_x: wcell_center(3),
            to_y: wcell_center(3),
            tolerance: 0,
            flags: 0,
        }];
        let mut pfd = WaterSearchFlags::default();
        let (out, n) = find_wpath_entry_proxy(
            &w,
            &mut stack,
            wcell_center(0),
            wcell_center(0),
            0,
            &mut pfd,
        );
        assert_eq!(out, WPathProxyOutcome::FullSearchRequired);
        assert_eq!(n, 1);
        assert_eq!(stack.len(), 1);
    }

    #[test]
    fn valid_wcoord_forgives_result_three_in_the_goal_cell() {
        // Same WCoord as the destination: invalid_loc == 3 is downgraded to valid.
        assert!(valid_wcoord(3, 100, 100, 200, 200, (-1, -1)));
        // A different WCoord: 3 stays fatal.
        assert!(!valid_wcoord(3, 100, 100, 5000, 5000, (-1, -1)));
        // Any other non-zero verdict is fatal regardless.
        assert!(!valid_wcoord(1, 100, 100, 200, 200, (-1, -1)));
        assert!(valid_wcoord(0, 100, 100, 5000, 5000, (-1, -1)));
        // The avoid cell always loses.
        assert!(!valid_wcoord(0, 100, 100, 200, 200, (100, 100)));
    }

    #[test]
    fn water_route_stays_on_water() {
        let mut w = ocean_world(9, 9);
        // A land wall down column 4, with a gap at row 8.
        for y in 0..8 {
            make_land(&mut w, 4, y);
        }
        let path = water_route_proxy(&w, (0, 0), (8, 0), 10_000).expect("a route around the wall");
        assert_eq!(path.first(), Some(&(0, 0)));
        assert_eq!(path.last(), Some(&(8, 0)));
        for (x, y) in &path {
            assert!(w.is_ocean(*x, *y), "route left the water at ({x},{y})");
        }
        // It has to detour through the gap.
        assert!(path.iter().any(|(_, y)| *y >= 8));
    }

    #[test]
    fn water_route_refuses_a_land_destination() {
        let mut w = ocean_world(4, 4);
        make_land(&mut w, 3, 3);
        assert!(water_route_proxy(&w, (0, 0), (3, 3), 10_000).is_none());
    }

    #[test]
    fn water_route_is_deterministic() {
        let mut w = ocean_world(7, 7);
        make_land(&mut w, 3, 3);
        let a = water_route_proxy(&w, (0, 0), (6, 6), 10_000).unwrap();
        let b = water_route_proxy(&w, (0, 0), (6, 6), 10_000).unwrap();
        assert_eq!(a, b);
    }

    // -- combat ------------------------------------------------------------

    #[test]
    fn ground_versus_sea_can_deal_zero() {
        assert!(ground_vs_sea_skips_damage_floor(
            Domain::Ground,
            Domain::Sea
        ));
        assert!(!ground_vs_sea_skips_damage_floor(Domain::Sea, Domain::Sea));
        assert!(!ground_vs_sea_skips_damage_floor(
            Domain::Ground,
            Domain::Ground
        ));
        assert!(!ground_vs_sea_skips_damage_floor(
            Domain::BothOrAir,
            Domain::Sea
        ));
    }

    #[test]
    fn elevation_is_skipped_when_either_side_is_air() {
        use damage_domain::elevation_applies;
        assert!(elevation_applies(Domain::Sea, Domain::Ground));
        assert!(!elevation_applies(Domain::BothOrAir, Domain::Sea));
        assert!(!elevation_applies(Domain::Sea, Domain::BothOrAir));
    }

    #[test]
    fn ground_attacking_non_ground_takes_the_double_arm() {
        use damage_domain::doubles_against;
        assert!(doubles_against(Domain::Ground, Domain::Sea, false));
        assert!(doubles_against(Domain::Ground, Domain::BothOrAir, false));
        assert!(!doubles_against(Domain::Ground, Domain::Ground, false));
        assert!(
            doubles_against(Domain::Sea, Domain::Sea, true),
            "anti-air arm"
        );
    }

    #[test]
    fn bombard_ships_have_a_minimum_range_band() {
        for name in ["Siegeship", "Bomb Vessel", "Bomb Ketch", "Catapult Ship"] {
            let e = NAVAL_ROSTER.iter().find(|e| e.name == name).unwrap();
            let ty = e.to_type();
            assert_eq!(ty.min_range_tiles, 4, "{name}");
            assert!(
                !in_weapon_band(&ty, 3 * TILE),
                "{name} should not hit at 3 tiles"
            );
            assert!(
                in_weapon_band(&ty, 4 * TILE),
                "{name} should hit at 4 tiles"
            );
            assert!(!in_weapon_band(&ty, 30 * TILE));
        }
    }

    #[test]
    fn range_conversion_is_tiles_to_world_units() {
        let bs = naval_roster(350).unwrap().to_type();
        assert_eq!(bs.max_range_tiles, 26);
        assert_eq!(bs.max_range_world(), 26 * 192);
    }

    // -- fish --------------------------------------------------------------

    #[test]
    fn fish_and_whales_are_herd_goods() {
        assert!(HERD_GOOD_INDICES.contains(&GOOD_FISH));
        assert!(HERD_GOOD_INDICES.contains(&GOOD_WHALES));
        assert_eq!(HERD_GOOD_INDICES.len(), 7);
    }

    #[test]
    fn fish_rethink_period_is_1024_frames_phased_by_object_index() {
        assert!(fish_rethink_due(0, 0));
        assert!(!fish_rethink_due(0, 1));
        assert!(fish_rethink_due(0, 1024));
        assert!(fish_rethink_due(5, 1019));
        assert!(!fish_rethink_due(5, 1024));
    }

    #[test]
    fn think_fish_consumes_rng_once_per_surviving_candidate() {
        let mut w = ocean_world(9, 9);
        // Give every cell region 1 and make two cells land so they are filtered out.
        make_land(&mut w, 4, 4);
        make_land(&mut w, 5, 4);
        let unit = NavalUnit {
            o: 3,
            who: 9,
            unit_masks: 0,
            unit_masks2: 0,
            x: wcell_center(4),
            y: wcell_center(4),
        };

        // Count the ocean cells the scan will reach so the RNG draw count is predictable.
        let (sx, sy) = (4, 4);
        let mut expected_draws = 0;
        for i in 0..SPIRAL_X.len().min(FISH_SPIRAL_LEN) {
            let (wx, wy) = (sx + SPIRAL_X[i], sy + SPIRAL_Y[i]);
            if w.valid_w(wx, wy) && w.is_ocean(wx, wy) && w.wcell(wx, wy).region == 1 {
                expected_draws += 1;
            }
        }
        assert!(expected_draws > 0);

        let mut rng = Random::new(12345);
        let mut counter = Random::new(12345);
        let _ = think_fish_pick(&w, &mut rng, &unit, 1, &|_, _| true);
        for _ in 0..expected_draws {
            counter.get(0, 0xFFFF);
        }
        assert_eq!(
            rng.state(),
            counter.state(),
            "one draw per surviving candidate, no more, no less"
        );
    }

    #[test]
    fn think_fish_scans_all_289_retail_offsets_and_consumes_289_draws() {
        let w = ocean_world(40, 40);
        let unit = NavalUnit {
            o: 3,
            who: 9,
            unit_masks: 0,
            unit_masks2: 0,
            x: wcell_center(20),
            y: wcell_center(20),
        };
        let mut actual = Random::new(0x1234_5678);
        let mut expected = actual;

        let _ = think_fish_pick(&w, &mut actual, &unit, 1, &|_, _| true);
        for _ in 0..FISH_SPIRAL_LEN {
            expected.get(0, 0xFFFF);
        }

        assert_eq!(FISH_SPIRAL_LEN, 289);
        assert_eq!(actual.state(), expected.state());
    }

    #[test]
    fn think_fish_is_deterministic_for_a_given_stream() {
        let w = ocean_world(9, 9);
        let unit = NavalUnit {
            o: 1,
            who: 9,
            unit_masks: 0,
            unit_masks2: 0,
            x: wcell_center(4),
            y: wcell_center(4),
        };
        let mut a = Random::new(7);
        let mut b = Random::new(7);
        let ra = think_fish_pick(&w, &mut a, &unit, 1, &|_, _| true);
        let rb = think_fish_pick(&w, &mut b, &unit, 1, &|_, _| true);
        assert_eq!(ra, rb);
    }

    #[test]
    fn think_fish_skips_occupied_cells() {
        let mut w = ocean_world(5, 5);
        // Mark every neighbour occupied by someone else; the shoal must not pick them.
        for i in 1..=8usize {
            let (wx, wy) = (2 + SPIRAL_X[i], 2 + SPIRAL_Y[i]);
            let c = w.wcell_mut(wx, wy);
            c.down = 99;
            c.down_who = 0;
        }
        let unit = NavalUnit {
            o: 3,
            who: 9,
            unit_masks: 0,
            unit_masks2: 0,
            x: wcell_center(2),
            y: wcell_center(2),
        };
        let mut rng = Random::new(3);
        let pick = think_fish_pick(&w, &mut rng, &unit, 1, &|_, _| true);
        let occupied: Vec<(i32, i32)> = (1..=8usize)
            .map(|i| (2 + SPIRAL_X[i], 2 + SPIRAL_Y[i]))
            .collect();
        assert!(
            !occupied.contains(&(pick.wx, pick.wy)),
            "picked an occupied cell"
        );
    }

    // -- roster ------------------------------------------------------------

    #[test]
    fn roster_is_complete_and_consistent() {
        assert_eq!(
            NAVAL_ROSTER.len(),
            43,
            "unitrules.xml has 43 DOMAIN Sea units"
        );
        let mut seen = std::collections::HashSet::new();
        for e in NAVAL_ROSTER {
            assert!(seen.insert(e.index), "duplicate index {}", e.index);
            assert_eq!(e.to_type().domain, Domain::Sea);
            assert!(e.min_range_tiles <= e.max_range_tiles, "{}", e.name);
        }
        // The three transports and the carrier are the only carriers.
        let carriers: Vec<&str> = NAVAL_ROSTER
            .iter()
            .filter(|e| e.carry > 0)
            .map(|e| e.name)
            .collect();
        assert_eq!(
            carriers,
            vec![
                "Transport Barge",
                "Transport Galleon",
                "Transport Freighter",
                "Aircraft Carrier"
            ]
        );
    }

    #[test]
    fn transports_cost_no_pop() {
        for name in [
            "Transport Barge",
            "Transport Galleon",
            "Transport Freighter",
        ] {
            let e = NAVAL_ROSTER.iter().find(|e| e.name == name).unwrap();
            assert_eq!(e.pop, 0, "{name} is a meta unit and must not eat pop cap");
        }
    }

    #[test]
    fn dock_is_the_naval_training_building() {
        let dock_trained = NAVAL_ROSTER
            .iter()
            .filter(|e| e.trained_at == Some("Dock"))
            .count();
        assert_eq!(dock_trained, 30);
        // The herds come from Large City, not a Dock.
        assert_eq!(
            naval_roster(UNIT_HERD_FISH).unwrap().trained_at,
            Some("Large City")
        );
        assert_eq!(
            naval_roster(UNIT_HERD_WHALES).unwrap().trained_at,
            Some("Large City")
        );
    }
}
