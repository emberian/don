//! Retail gathering-site state and the worker/site adapter contract.
//!
//! This module owns the seam between map-derived gathering capacity and consumers such as
//! the playable arena. It deliberately does **not** map a terrain label to a slot count.
//! Retail obtains `BuildData::gather_max` by calling
//! `BuildTypeData::max_gatherers` (`0x0063C430`), which in turn runs the 7,668-byte
//! `BuildTypeData::calc_gather` (`0x00639E40`) over the real world cells, mountain/cliff
//! objects, ownership/diplomacy gates, player properties, and the building's `MiningList`.
//! The admitted retail roster is executable here: Farm's flat result, Woodcutter's
//! ring/LandData/river/access reduction, and Mine's selected Mountain/Cliff object
//! reduction. Callers supply the real world/object/LandData records through
//! `AuthoritativeGatherTerrain`; missing evidence fails instead of becoming a slot table.
//!
//! The same module retains the persistent state on either side of that evaluator:
//!
//! * `BuildData::gather_max` is a signed byte at `+0x80`.
//! * `BuildData::gather_down` at `+0x70` heads an owner-local chain of unit object indices.
//! * every linked `UnitData::gather_down` is a signed short at `+0x92`.
//! * `Build::add_gatherer` (`0x0062F640`), `check_gatherers` (`0x0062F710`) and
//!   `remove_gatherer` (`0x0062F8D0`) mutate that chain.
//! * active membership is decided by `UnitData::is_gathering_at` (`0x00608880`), including
//!   the `GatherOrder::been_there` byte.
//!
//! Structure and transitions are read from the named retail functions and the matching PDB.
//! They are Tier C: they have not been run differentially against retail.

use crate::container::{EngineArray, INCREMENT_DOUBLE};
use crate::mechanics::{credit_resource, resource_period};
use crate::rng::Random;

use super::economy::{self, EconRules, NUM_RESOURCES};
use super::map_terrain::{
    tflag, wflag, Coord, TCoord, UCoord, WCoord, World, NEIGHBOUR4_DX, NEIGHBOUR4_DY,
    TILES_PER_WCELL,
};
use super::movement::{cosx, sinx, vector_dist};

/// Retail's null owner-local object link.
pub const NO_OBJECT: i16 = -1;
/// `BuildTypeData::max_flat_gatherers` (`0x006365F0`).
pub const MAX_FLAT_GATHERERS: i32 = 1;
/// `BuildTypeData::max_knowledge_gatherers` (`0x006365E0`).
pub const MAX_KNOWLEDGE_GATHERERS: i32 = 7;

/// One fine-cell coordinate returned by the authoritative gathering-terrain search.
/// `BuildData::gather_from` stores these pairs; this type does not decide which cells qualify.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GatherTile {
    pub tx: i32,
    pub ty: i32,
}

/// The two terrain-object collections retail searches for a Mine.
///
/// These are object identities, not labels on independent tiles.  A Mine retains the
/// selected signed object index in `MiningList::mtn` or `MiningList::cliff` and later walks
/// that object's ordered coordinate array.  Reconstructing an object with a connected-
/// component search changes both list order and capacity and is therefore not accepted by
/// the authoritative adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MiningObjectKind {
    Mountain,
    Cliff,
}

/// Result of retail's nearest Mountain/Cliff object search.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MiningObjectCandidate {
    /// Index in the corresponding retail terrain-object collection.  `MiningList` stores
    /// it in a signed byte, so only `0..=127` is representable persistently.
    pub index: i32,
    /// Distance returned through the search function's out-parameter, in `Coord` units.
    pub distance: i32,
}

/// One absolute coarse cell in a MountainRangeData `solid_mount` walk.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GatherWorldCell {
    pub wx: i32,
    pub wy: i32,
}

/// One of the four `(make, num_make)` entries in retail `LandData`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LandGatherSlot {
    /// Six-slot resource index, or a negative unused marker.
    pub make: i32,
    /// Per-cell quantity added sixteen times by `calc_gather`.
    pub num_make: i32,
}

/// The four gathering entries selected by `WorldData::get_land(wcoord, 1)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LandGatherData {
    pub slots: [LandGatherSlot; 4],
}

/// Read-only world boundary consumed by `BuildTypeData::find_gather_tcoords`
/// (`0x0063BDC0`).
///
/// The ordinary tile and territory reads can be backed directly by the synchronized
/// `map_terrain::World`.  The Mine methods deliberately require the map generator/runtime
/// to retain retail terrain-object identity and coordinate order; there is no default that
/// turns an unstructured mountain bitmap into a synthetic object.
pub trait AuthoritativeGatherTerrain {
    /// World dimensions in 4x4-tile `WCoord` cells.
    fn world_cell_dimensions(&self) -> (i32, i32);
    /// Fine-tile dimensions stored independently at WorldData `+0x18/+0x1C`.
    fn tile_dimensions(&self) -> (i32, i32);
    /// Raw `TData` mask. `None` means off-map or unavailable.
    fn tile_mask(&self, tile: GatherTile) -> Option<u16>;
    /// `WData::who`; `None` means off-map or unavailable, while `Some(-1)` is unowned.
    fn territory_owner(&self, wx: i32, wy: i32) -> Option<i32>;
    /// Raw `WData::flags`; Mine capacity distinguishes coarse forest cells with bit
    /// `0x20` independently of fine-tile tree bits.
    fn world_cell_flags(&self, wx: i32, wy: i32) -> Option<u16>;
    /// Retail `LeaderData::is_ally` result. Self and unowned territory are handled before
    /// this callback and do not call it. `None` means diplomacy state is unavailable;
    /// callers fail instead of treating missing state as an enemy relation.
    fn is_allied(&self, site_owner: i32, territory_owner: i32) -> Option<bool>;
    /// Region-aware nearest-object query used only when `MiningList` has no retained
    /// Mountain or Cliff index.
    fn nearest_mining_object(
        &self,
        kind: MiningObjectKind,
        site_x: Coord,
        site_y: Coord,
        site_region: i16,
    ) -> Option<MiningObjectCandidate>;
    /// The selected object's stable, ordered fine-tile coordinate array.
    fn mining_object_tiles(&self, kind: MiningObjectKind, index: i32) -> Option<&[GatherTile]>;
    /// `MountainRangeData::mountain_size`, used for the five shipped capacity bands.
    fn mining_object_size(&self, _kind: MiningObjectKind, _index: i32) -> Option<i32> {
        None
    }
    /// Absolute WCoord cells in MountainRangeData's stable `solid_mount` order.
    fn mountain_solid_world_cells(&self, _index: i32) -> Option<&[GatherWorldCell]> {
        None
    }
    /// Exact four-slot `LandData` payload for Woodcutter capacity.
    fn land_gather_data(&self, _wx: i32, _wy: i32) -> Option<LandGatherData> {
        None
    }
}

/// Exact non-flat source admitted by the recovered terrain search.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatherTerrainKind {
    /// Build property `0x1A2`: the Woodcutter/forest ring-table arm.
    Forest,
    /// Build property `0x1A3`: the retained Mountain/Cliff object arm.
    Mine,
}

/// Inputs to `BuildTypeData::find_gather_tcoords` for a completed non-flat building.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherTerrainRequest {
    pub kind: GatherTerrainKind,
    /// Building centre in raw engine `Coord` units.
    pub site_x: Coord,
    pub site_y: Coord,
    pub site_owner: i32,
    /// WData land-region value passed to the Mountain/Cliff nearest-object searches.
    pub site_region: i16,
    /// `BuildTypeData::gather_radius`, in fine tiles (8 Woodcutter, 6 Mine in shipped
    /// rules). Retail compares it to fine-tile distance and multiplies it by 192 for the
    /// Mountain/Cliff object-distance cutoff.
    pub gather_radius: i32,
}

/// Observable result of one incremental terrain discovery pass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GatherTerrainDiscovery {
    pub appended: usize,
    /// Cumulative ring-table entries inspected. Mine discovery reports zero.
    pub ring_entries: usize,
    /// Newly selected persistent terrain object, if the Mine arm had to search.
    pub selected_object: Option<(MiningObjectKind, i8)>,
}

/// Fail-closed errors at the recovered discovery boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatherTerrainError {
    NegativeRadius(i32),
    /// Retail's global ring arrays contain shells 0..=64 and stop after entry 12,873.
    RadiusBeyondRetailTable(i32),
    MissingWorldCell(i32, i32),
    MissingTile(GatherTile),
    MissingDiplomacy(i32, i32),
    MissingMiningObject(MiningObjectKind, i32),
    MissingMiningCapacityData(MiningObjectKind, i32),
    MissingLandGatherData(i32, i32),
    MiningObjectIndexNotPersistent(i32),
    ZeroMiningDenominator(MiningObjectKind, i32),
    InvalidFootprint(i32, i32),
    LengthOverflow,
}

/// Retail's global gather/search ring arrays hold at most this many signed-byte pairs.
/// `init_coord_lookup_array` (`0x006817F0`) stops immediately after writing entry 12,873.
pub const RETAIL_RING_TABLE_ENTRIES: usize = 0x3248 + 1;
/// Largest shell for which `init_coord_lookup_array` publishes a cumulative count.
pub const RETAIL_RING_MAX_SHELL: i32 = 64;

/// Rebuild the prefix of retail's `DAT_00CB7E90`/`DAT_00CBB0E0` ring tables required for
/// a fine-tile radius.
///
/// Shells are enumerated in `(dx outer, dy inner)` order and retain points whose retail
/// `vector_dist(dx, dy)` equals the shell. This is not a Euclidean circle and not a spiral
/// invented by the adapter. The returned prefix is exactly the cumulative count at
/// `DAT_00CBE330[ceil(radius / 4)]`, including retail's final-table truncation.
pub fn retail_gather_ring_prefix(gather_radius: i32) -> Result<Vec<(i8, i8)>, GatherTerrainError> {
    if gather_radius < 0 {
        return Err(GatherTerrainError::NegativeRadius(gather_radius));
    }
    let shell = gather_radius
        .checked_add(TILES_PER_WCELL - 1)
        .ok_or(GatherTerrainError::RadiusBeyondRetailTable(gather_radius))?
        / TILES_PER_WCELL;
    if shell > RETAIL_RING_MAX_SHELL {
        return Err(GatherTerrainError::RadiusBeyondRetailTable(gather_radius));
    }

    let mut result = Vec::new();
    'shells: for radius in 0..=shell {
        for dx in -radius..=radius {
            for dy in -radius..=radius {
                if vector_dist(dx, dy) == radius {
                    result.push((dx as i8, dy as i8));
                    if result.len() == RETAIL_RING_TABLE_ENTRIES {
                        break 'shells;
                    }
                }
            }
        }
    }
    Ok(result)
}

#[inline]
fn territory_is_usable<H: AuthoritativeGatherTerrain + ?Sized>(
    host: &H,
    site_owner: i32,
    wx: i32,
    wy: i32,
) -> Result<bool, GatherTerrainError> {
    let owner = host
        .territory_owner(wx, wy)
        .ok_or(GatherTerrainError::MissingWorldCell(wx, wy))?;
    if owner < 0 || owner == site_owner {
        return Ok(true);
    }
    host.is_allied(site_owner, owner)
        .ok_or(GatherTerrainError::MissingDiplomacy(site_owner, owner))
}

/// The exact `WorldData::has_gather_access(tile, who, 1, 0)` predicate used by
/// `Unit::do_non_flat_gather`.
///
/// The final zero selects retail's `mode == 0` arm. That arm is intentionally different
/// from `map_terrain::World::has_gather_access`, which ports the other arm: the centre must
/// carry `GATHER_EDGE` (`TData` negative as a signed short), and one N/E/S/W neighbour must
/// be in usable territory, non-water and not `BLOCKED`. The `flag == 1` argument bypasses
/// the centre-territory test, but does not bypass each neighbour's territory gate.
pub fn has_non_flat_gather_access<H: AuthoritativeGatherTerrain + ?Sized>(
    host: &H,
    tile: GatherTile,
    worker_owner: i32,
) -> Result<bool, GatherTerrainError> {
    let centre = host
        .tile_mask(tile)
        .ok_or(GatherTerrainError::MissingTile(tile))?;
    if centre & tflag::GATHER_EDGE == 0 {
        return Ok(false);
    }
    let (tile_xs, tile_ys) = host.tile_dimensions();
    for i in 0..4 {
        let neighbour = GatherTile {
            tx: tile.tx.wrapping_add(NEIGHBOUR4_DX[i]),
            ty: tile.ty.wrapping_add(NEIGHBOUR4_DY[i]),
        };
        if neighbour.tx < 0
            || neighbour.ty < 0
            || neighbour.tx >= tile_xs
            || neighbour.ty >= tile_ys
        {
            continue;
        }
        if worker_owner >= 0
            && !territory_is_usable(host, worker_owner, neighbour.tx >> 2, neighbour.ty >> 2)?
        {
            continue;
        }
        let mask = host
            .tile_mask(neighbour)
            .ok_or(GatherTerrainError::MissingTile(neighbour))?;
        if mask & tflag::SURFACE_MASK != tflag::SURFACE_WATER && mask & tflag::BLOCKED == 0 {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Exact incremental `BuildTypeData::find_gather_tcoords` terrain append.
///
/// This function does not reserve cells and does not draw RNG. `Build::find_gather_tiles`
/// performs those mutations afterward through [`finish_gather_tile_refresh`]. Existing
/// list entries are deliberately retained; retail discovery is incremental.
pub fn discover_gather_terrain<H: AuthoritativeGatherTerrain + ?Sized>(
    host: &H,
    list: &mut GatherMiningList,
    request: GatherTerrainRequest,
) -> Result<GatherTerrainDiscovery, GatherTerrainError> {
    if request.gather_radius < 0 {
        return Err(GatherTerrainError::NegativeRadius(request.gather_radius));
    }
    match request.kind {
        GatherTerrainKind::Forest => discover_forest_terrain(host, list, request),
        GatherTerrainKind::Mine => discover_mine_terrain(host, list, request),
    }
}

fn discover_forest_terrain<H: AuthoritativeGatherTerrain + ?Sized>(
    host: &H,
    list: &mut GatherMiningList,
    request: GatherTerrainRequest,
) -> Result<GatherTerrainDiscovery, GatherTerrainError> {
    let ring = retail_gather_ring_prefix(request.gather_radius)?;
    let origin_wx = WCoord::from_coord(request.site_x).0;
    let origin_wy = WCoord::from_coord(request.site_y).0;
    let origin_tx = TCoord::from_coord(request.site_x).0;
    let origin_ty = TCoord::from_coord(request.site_y).0;
    let (world_xs, world_ys) = host.world_cell_dimensions();
    let mut additions = Vec::new();

    for &(dx, dy) in &ring {
        let wx = origin_wx.wrapping_add(dx as i32);
        let wy = origin_wy.wrapping_add(dy as i32);
        if wx < 0 || wy < 0 || wx >= world_xs || wy >= world_ys {
            continue;
        }
        let centre_tx = wx.wrapping_mul(TILES_PER_WCELL).wrapping_add(2);
        let centre_ty = wy.wrapping_mul(TILES_PER_WCELL).wrapping_add(2);
        if vector_dist(
            centre_tx.wrapping_sub(origin_tx),
            centre_ty.wrapping_sub(origin_ty),
        ) > request.gather_radius
        {
            continue;
        }
        if !territory_is_usable(host, request.site_owner, wx, wy)? {
            continue;
        }
        let centre = GatherTile {
            tx: centre_tx,
            ty: centre_ty,
        };
        let centre_mask = host
            .tile_mask(centre)
            .ok_or(GatherTerrainError::MissingTile(centre))?;
        if centre_mask & tflag::GATHERED != 0 {
            continue;
        }

        let corner_tx = wx.wrapping_mul(TILES_PER_WCELL);
        let corner_ty = wy.wrapping_mul(TILES_PER_WCELL);
        for i in 0..16 {
            let tile = GatherTile {
                tx: corner_tx.wrapping_add(i & 3),
                ty: corner_ty.wrapping_add(i >> 2),
            };
            let mask = host
                .tile_mask(tile)
                .ok_or(GatherTerrainError::MissingTile(tile))?;
            if mask & tflag::SURFACE_MASK == tflag::SURFACE_TREES && mask & tflag::GATHERED == 0 {
                additions.push(tile);
            }
        }
    }
    list.len()
        .checked_add(additions.len())
        .filter(|length| *length <= i32::MAX as usize)
        .ok_or(GatherTerrainError::LengthOverflow)?;
    for tile in &additions {
        list.add(*tile);
    }
    Ok(GatherTerrainDiscovery {
        appended: additions.len(),
        ring_entries: ring.len(),
        selected_object: None,
    })
}

fn discover_mine_terrain<H: AuthoritativeGatherTerrain + ?Sized>(
    host: &H,
    list: &mut GatherMiningList,
    request: GatherTerrainRequest,
) -> Result<GatherTerrainDiscovery, GatherTerrainError> {
    let mut newly_selected = false;
    let selected = if list.mtn >= 0 {
        Some((MiningObjectKind::Mountain, list.mtn as i32, 0))
    } else if list.cliff >= 0 {
        Some((MiningObjectKind::Cliff, list.cliff as i32, 0))
    } else {
        let mountain = host.nearest_mining_object(
            MiningObjectKind::Mountain,
            request.site_x,
            request.site_y,
            request.site_region,
        );
        let cliff = host.nearest_mining_object(
            MiningObjectKind::Cliff,
            request.site_x,
            request.site_y,
            request.site_region,
        );
        newly_selected = true;
        match (mountain, cliff) {
            (Some(m), Some(c)) if m.distance <= c.distance => {
                Some((MiningObjectKind::Mountain, m.index, m.distance))
            }
            (Some(_), Some(c)) => Some((MiningObjectKind::Cliff, c.index, c.distance)),
            (Some(m), None) => Some((MiningObjectKind::Mountain, m.index, m.distance)),
            (None, Some(c)) => Some((MiningObjectKind::Cliff, c.index, c.distance)),
            (None, None) => None,
        }
    };
    let Some((kind, index, distance)) = selected else {
        return Ok(GatherTerrainDiscovery::default());
    };
    let cutoff = request
        .gather_radius
        .checked_mul(super::map_terrain::COORD_PER_TILE)
        .ok_or(GatherTerrainError::RadiusBeyondRetailTable(
            request.gather_radius,
        ))?;
    if newly_selected && distance > cutoff {
        return Ok(GatherTerrainDiscovery::default());
    }
    let persistent_index = i8::try_from(index)
        .ok()
        .filter(|value| *value >= 0)
        .ok_or(GatherTerrainError::MiningObjectIndexNotPersistent(index))?;
    let object_tiles = host
        .mining_object_tiles(kind, index)
        .ok_or(GatherTerrainError::MissingMiningObject(kind, index))?;
    let mut additions = Vec::new();
    for &tile in object_tiles {
        let mask = host
            .tile_mask(tile)
            .ok_or(GatherTerrainError::MissingTile(tile))?;
        let wx = tile.tx >> 2;
        let wy = tile.ty >> 2;
        if mask & tflag::SURFACE_MASK != tflag::SURFACE_TREES
            && territory_is_usable(host, request.site_owner, wx, wy)?
            && mask & tflag::GATHERED == 0
        {
            additions.push(tile);
        }
    }
    list.len()
        .checked_add(additions.len())
        .filter(|length| *length <= i32::MAX as usize)
        .ok_or(GatherTerrainError::LengthOverflow)?;

    if newly_selected {
        match kind {
            MiningObjectKind::Mountain => list.mtn = persistent_index,
            MiningObjectKind::Cliff => list.cliff = persistent_index,
        }
    }
    for tile in &additions {
        list.add(*tile);
    }
    Ok(GatherTerrainDiscovery {
        appended: additions.len(),
        ring_entries: 0,
        selected_object: newly_selected.then_some((kind, persistent_index)),
    })
}

/// Exact rules consumed by the admitted Woodcutter/Mine capacity arms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherCapacityRules {
    pub mountain_size_thresholds: [i32; 4],
    pub mountain_capacity_bases: [i32; 5],
    pub french_woodies: i32,
    pub german_miners: i32,
    pub taj_farms: i32,
    pub kremlin_farms: i32,
}

impl GatherCapacityRules {
    /// Shipped `rules.xml` values. Wonder/Mine bonuses are present even though all three
    /// ship as zero; mods may make them checksum-relevant.
    pub const fn shipped() -> Self {
        Self {
            mountain_size_thresholds: [100, 210, 275, 400],
            mountain_capacity_bases: [3, 5, 6, 8, 10],
            french_woodies: 1,
            german_miners: 0,
            taj_farms: 0,
            kremlin_farms: 0,
        }
    }

    /// Read the engine Rules dword block at the exact retail offsets.
    pub fn from_rules_block(block: &[i32]) -> Option<Self> {
        let at = |byte: usize| block.get(byte / 4).copied();
        Some(Self {
            mountain_size_thresholds: [at(0x9cc)?, at(0x9d0)?, at(0x9d4)?, at(0x9d8)?],
            mountain_capacity_bases: [at(0x9e0)?, at(0x9e4)?, at(0x9e8)?, at(0x9ec)?, at(0x9f0)?],
            french_woodies: at(0x6c8)?,
            german_miners: at(0x728)?,
            taj_farms: at(0x508)?,
            kremlin_farms: at(0x518)?,
        })
    }

    #[inline]
    fn mining_base(self, size: i32) -> i32 {
        let band = self
            .mountain_size_thresholds
            .iter()
            .position(|threshold| size < *threshold)
            .unwrap_or(4);
        self.mountain_capacity_bases[band]
    }
}

/// Player/wonder predicates queried after terrain capacity is nonzero.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GatherCapacityBonuses {
    pub french_tribe: bool,
    pub german_tribe: bool,
    pub taj_mahal: bool,
    pub kremlin: bool,
}

/// One Farm move destination and its direct main-RNG consumption.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FarmGatherMove {
    pub destination: GatherNearbyPoint,
    pub rng_draws: usize,
}

#[inline]
fn gather_rnd_exclusive(n: i32, rng: &mut Random, draws: &mut usize) -> i32 {
    if n <= 1 {
        0
    } else {
        *draws += 1;
        rng.get(0, 0xffff) % n
    }
}

/// Farm's gated initial footprint move at `0x005EFD77`.
///
/// A nonzero low-byte gate performs no movement and no RNG draw. On the one-in-256 gate,
/// x then y each call `GameAccess::rnd(size/2)`, which itself draws only when the argument
/// exceeds one. The result is a queued move destination; the worker remains on-map.
pub fn initial_farm_gather_move(
    game_gate_value: i32,
    worker_o: i32,
    worker_owner: i32,
    corner_tx: i32,
    corner_ty: i32,
    x_size: i32,
    y_size: i32,
    rng: &mut Random,
) -> Result<Option<FarmGatherMove>, GatherTerrainError> {
    if x_size <= 0 || y_size <= 0 {
        return Err(GatherTerrainError::InvalidFootprint(x_size, y_size));
    }
    let gate = game_gate_value
        .wrapping_add(worker_o.wrapping_mul(7))
        .wrapping_add(worker_owner)
        & 0xff;
    if gate != 0 {
        return Ok(None);
    }
    let mut draws = 0;
    let tx =
        corner_tx
            .wrapping_add(1)
            .wrapping_add(gather_rnd_exclusive(x_size / 2, rng, &mut draws));
    let ty =
        corner_ty
            .wrapping_add(1)
            .wrapping_add(gather_rnd_exclusive(y_size / 2, rng, &mut draws));
    Ok(Some(FarmGatherMove {
        destination: GatherNearbyPoint {
            x: TCoord(tx).centre(),
            y: TCoord(ty).centre(),
        },
        rng_draws: draws,
    }))
}

/// General Farm footprint relocation at `0x005EFFF9`: x then y call
/// `GameAccess::rnd(full_size)`. This is also a queued move, never `go_inside`.
pub fn farm_gather_relocation(
    corner_tx: i32,
    corner_ty: i32,
    x_size: i32,
    y_size: i32,
    rng: &mut Random,
) -> Result<FarmGatherMove, GatherTerrainError> {
    if x_size <= 0 || y_size <= 0 {
        return Err(GatherTerrainError::InvalidFootprint(x_size, y_size));
    }
    let mut draws = 0;
    let tx = corner_tx.wrapping_add(gather_rnd_exclusive(x_size, rng, &mut draws));
    let ty = corner_ty.wrapping_add(gather_rnd_exclusive(y_size, rng, &mut draws));
    Ok(FarmGatherMove {
        destination: GatherNearbyPoint {
            x: TCoord(tx).centre(),
            y: TCoord(ty).centre(),
        },
        rng_draws: draws,
    })
}

/// Exact Woodcutter capacity inputs for the admitted retail TypeIndex 418 shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WoodGatherCapacityRequest {
    /// Top-left footprint TCoord passed into `BuildTypeData::calc_gather`.
    pub site_tx: i32,
    pub site_ty: i32,
    pub x_size: i32,
    pub y_size: i32,
    pub owner: i32,
    pub bonuses: GatherCapacityBonuses,
}

/// Exact Mine capacity inputs after terrain-object discovery selected an id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MineGatherCapacityRequest {
    pub owner: i32,
    pub bonuses: GatherCapacityBonuses,
}

#[inline]
fn list_contains(list: &GatherMiningList, tile: GatherTile) -> bool {
    list.tiles().contains(&tile)
}

fn has_capacity_gather_access<H: AuthoritativeGatherTerrain + ?Sized>(
    host: &H,
    tile: GatherTile,
    owner: i32,
) -> Result<bool, GatherTerrainError> {
    let centre = host
        .tile_mask(tile)
        .ok_or(GatherTerrainError::MissingTile(tile))?;
    if centre & tflag::GATHER_EDGE == 0 {
        return Ok(false);
    }
    if !territory_is_usable(host, owner, tile.tx >> 2, tile.ty >> 2)? {
        return Ok(false);
    }
    let (tile_xs, tile_ys) = host.tile_dimensions();
    for i in 0..4 {
        let neighbour = GatherTile {
            tx: tile.tx.wrapping_add(NEIGHBOUR4_DX[i]),
            ty: tile.ty.wrapping_add(NEIGHBOUR4_DY[i]),
        };
        if neighbour.tx < 0
            || neighbour.ty < 0
            || neighbour.tx >= tile_xs
            || neighbour.ty >= tile_ys
        {
            continue;
        }
        let mask = host
            .tile_mask(neighbour)
            .ok_or(GatherTerrainError::MissingTile(neighbour))?;
        if mask & tflag::SURFACE_MASK != tflag::SURFACE_WATER && mask & tflag::BLOCKED == 0 {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `BuildTypeData::calc_gather`'s exact Woodcutter capacity reduction.
///
/// Capacity is derived from `LandData.make/num_make`, not a tree count. The synchronized
/// MiningList must contain each contributing W-cell centre, and the final value is capped
/// at twice `total_gather_access`. No RNG is consumed.
pub fn woodcutter_gather_capacity<H: AuthoritativeGatherTerrain + ?Sized>(
    host: &H,
    list: &GatherMiningList,
    request: WoodGatherCapacityRequest,
    rules: GatherCapacityRules,
) -> Result<i32, GatherTerrainError> {
    const WOOD_RADIUS: i32 = 8;
    const HALF_TILE: i32 = super::map_terrain::COORD_PER_TILE / 2;
    let centre_x = Coord(
        request
            .site_tx
            .wrapping_mul(2)
            .wrapping_add(request.x_size)
            .wrapping_mul(HALF_TILE),
    );
    let centre_y = Coord(
        request
            .site_ty
            .wrapping_mul(2)
            .wrapping_add(request.y_size)
            .wrapping_mul(HALF_TILE),
    );
    let centre_tx = TCoord::from_coord(centre_x).0;
    let centre_ty = TCoord::from_coord(centre_y).0;
    let origin_wx = WCoord::from_coord(centre_x).0;
    let origin_wy = WCoord::from_coord(centre_y).0;
    let (world_xs, world_ys) = host.world_cell_dimensions();
    let (tile_xs, tile_ys) = host.tile_dimensions();
    let ring = retail_gather_ring_prefix(WOOD_RADIUS)?;
    let mut raw16 = 0i32;

    for (dx, dy) in ring {
        let wx = origin_wx.wrapping_add(dx as i32);
        let wy = origin_wy.wrapping_add(dy as i32);
        if wx < 0 || wy < 0 || wx >= world_xs || wy >= world_ys {
            continue;
        }
        let cell_centre = GatherTile {
            tx: wx.wrapping_mul(4).wrapping_add(2),
            ty: wy.wrapping_mul(4).wrapping_add(2),
        };
        if vector_dist(
            cell_centre.tx.wrapping_sub(centre_tx),
            cell_centre.ty.wrapping_sub(centre_ty),
        ) > WOOD_RADIUS
            || !territory_is_usable(host, request.owner, wx, wy)?
            || !list_contains(list, cell_centre)
        {
            continue;
        }
        let land = host
            .land_gather_data(wx, wy)
            .ok_or(GatherTerrainError::MissingLandGatherData(wx, wy))?;
        for slot in land.slots {
            if slot.make == economy::RES_TIMBER as i32 && slot.num_make != 0 {
                raw16 = raw16.wrapping_add(slot.num_make.wrapping_mul(16));
            }
        }
    }

    let mut rivers = 0i32;
    for tx in request.site_tx..request.site_tx.wrapping_add(request.x_size) {
        for ty in request.site_ty..request.site_ty.wrapping_add(request.y_size) {
            if tx < 0 || ty < 0 || tx >= tile_xs || ty >= tile_ys {
                continue;
            }
            let tile = GatherTile { tx, ty };
            let mask = host
                .tile_mask(tile)
                .ok_or(GatherTerrainError::MissingTile(tile))?;
            rivers = rivers.wrapping_add(i32::from(mask & tflag::RIVER != 0));
        }
    }
    raw16 = raw16.wrapping_add(raw16.wrapping_mul(rivers) / 16);
    let mut slots = raw16.wrapping_add(8) / 16;
    if slots != 0 && request.owner >= 0 {
        if request.bonuses.french_tribe {
            slots = slots.wrapping_add(rules.french_woodies);
        }
        if request.bonuses.taj_mahal {
            slots = slots.wrapping_add(rules.taj_farms);
        }
        if request.bonuses.kremlin {
            slots = slots.wrapping_add(rules.kremlin_farms);
        }
    }

    let mut access = 0i32;
    for &tile in list.tiles() {
        if has_capacity_gather_access(host, tile, request.owner)? {
            let px = tile
                .tx
                .wrapping_mul(super::map_terrain::COORD_PER_TILE)
                .wrapping_add(HALF_TILE);
            let py = tile
                .ty
                .wrapping_mul(super::map_terrain::COORD_PER_TILE)
                .wrapping_add(HALF_TILE);
            let dx = centre_x.0.wrapping_sub(px).wrapping_abs();
            let dy = centre_y.0.wrapping_sub(py).wrapping_abs();
            if dx.wrapping_add(dy) > 0x240 || dx >= 0x240 || dy >= 0x240 {
                access = access.wrapping_add(1);
            }
        }
    }
    Ok(slots.min(access.wrapping_mul(2)))
}

/// Exact selected Mountain/Cliff capacity reduction for a Mine. No RNG is consumed.
pub fn mine_gather_capacity<H: AuthoritativeGatherTerrain + ?Sized>(
    host: &H,
    list: &GatherMiningList,
    request: MineGatherCapacityRequest,
    rules: GatherCapacityRules,
) -> Result<i32, GatherTerrainError> {
    let (kind, index) = if list.mtn >= 0 {
        (MiningObjectKind::Mountain, i32::from(list.mtn))
    } else if list.cliff >= 0 {
        (MiningObjectKind::Cliff, i32::from(list.cliff))
    } else {
        return Err(GatherTerrainError::MissingMiningCapacityData(
            MiningObjectKind::Mountain,
            -1,
        ));
    };
    let mut capacity = match kind {
        MiningObjectKind::Mountain => {
            let size = host
                .mining_object_size(kind, index)
                .ok_or(GatherTerrainError::MissingMiningCapacityData(kind, index))?;
            let cells = host
                .mountain_solid_world_cells(index)
                .ok_or(GatherTerrainError::MissingMiningCapacityData(kind, index))?;
            let mut denominator = 0i32;
            let mut eligible = 0i32;
            for cell in cells {
                let flags = host
                    .world_cell_flags(cell.wx, cell.wy)
                    .ok_or(GatherTerrainError::MissingWorldCell(cell.wx, cell.wy))?;
                let centre = GatherTile {
                    tx: cell.wx.wrapping_mul(4).wrapping_add(2),
                    ty: cell.wy.wrapping_mul(4).wrapping_add(2),
                };
                let mask = host
                    .tile_mask(centre)
                    .ok_or(GatherTerrainError::MissingTile(centre))?;
                if flags & wflag::FOREST == 0
                    && mask & tflag::BLOCKER_MASK == tflag::BLOCKER_MOUNTAIN
                {
                    denominator = denominator.wrapping_add(1);
                    if territory_is_usable(host, request.owner, cell.wx, cell.wy)?
                        && list_contains(list, centre)
                    {
                        eligible = eligible.wrapping_add(1);
                    }
                }
            }
            if denominator == 0 {
                return Err(GatherTerrainError::ZeroMiningDenominator(kind, index));
            }
            let mut value = rules.mining_base(size).wrapping_mul(eligible) / denominator;
            if eligible != 0 && value < 1 {
                value = 1;
            }
            value
        }
        MiningObjectKind::Cliff => {
            let tiles = host
                .mining_object_tiles(kind, index)
                .ok_or(GatherTerrainError::MissingMiningObject(kind, index))?;
            if tiles.is_empty() {
                return Err(GatherTerrainError::ZeroMiningDenominator(kind, index));
            }
            let mut eligible = 0i32;
            for &tile in tiles {
                if territory_is_usable(host, request.owner, tile.tx >> 2, tile.ty >> 2)?
                    && list_contains(list, tile)
                {
                    eligible = eligible.wrapping_add(1);
                }
            }
            let mut value =
                rules.mining_base(tiles.len() as i32).wrapping_mul(eligible) / tiles.len() as i32;
            if eligible != 0 && value < 1 {
                value = 1;
            }
            value
        }
    };
    if capacity != 0 && request.owner >= 0 {
        if request.bonuses.german_tribe {
            capacity = capacity.wrapping_add(rules.german_miners);
        }
        if request.bonuses.taj_mahal {
            capacity = capacity.wrapping_add(rules.taj_farms);
        }
        if request.bonuses.kremlin {
            capacity = capacity.wrapping_add(rules.kremlin_farms);
        }
    }
    Ok(capacity)
}

/// `BuildData::gather_from` (`+0x98`), retaining the engine array header that participates
/// in `BuildData::walk_data`.
///
/// `MiningList::MiningList` (`0x00472260`) constructs the array with `size = 0` and
/// `increment = -1`; it does not use `SimpleArray`'s five-element initial allocation.
/// The two signed tail bytes are walked before the array body even though they follow it
/// in memory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GatherMiningList {
    tiles: EngineArray<GatherTile>,
    pub mtn: i8,
    pub cliff: i8,
}

impl Default for GatherMiningList {
    fn default() -> Self {
        Self {
            tiles: EngineArray::with_size(0, INCREMENT_DOUBLE),
            mtn: -1,
            cliff: -1,
        }
    }
}

impl GatherMiningList {
    #[inline]
    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    #[inline]
    pub fn tiles(&self) -> &[GatherTile] {
        self.tiles.as_slice()
    }

    /// `(length, capacity, increment, flags)` as the retail array stores it.
    #[inline]
    pub fn array_header(&self) -> (i32, i32, i16, u8) {
        self.tiles.checksum_header()
    }

    /// Append one coordinate produced by the authoritative terrain evaluator.
    #[inline]
    pub fn add(&mut self, tile: GatherTile) -> usize {
        self.tiles.add(tile)
    }

    /// Remove the first matching coordinate, as `Array<TCoordData>::remove`
    /// (`0x0046D4F0`) does. Capacity never shrinks.
    pub fn remove_first(&mut self, tile: GatherTile) -> bool {
        let Some(i) = self
            .tiles
            .as_slice()
            .iter()
            .position(|candidate| *candidate == tile)
        else {
            return false;
        };
        self.tiles.remove(i);
        true
    }

    /// Retail's selection and shuffle primitive: remove the first equal coordinate and
    /// append the value to the tail.
    pub fn move_to_back(&mut self, tile: GatherTile) -> bool {
        if !self.remove_first(tile) {
            return false;
        }
        self.tiles.add(tile);
        true
    }

    /// The exact bytes emitted for the `MiningList` portion of `BuildData::walk_data`:
    /// `mtn`, `cliff`, then `Array<TCoordData>::walk_data`.
    pub fn walked_image(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(15 + self.len() * 8);
        out.push(self.mtn as u8);
        out.push(self.cliff as u8);
        let (length, size, increment, flags) = self.tiles.checksum_header();
        out.extend_from_slice(&length.to_le_bytes());
        if length != 0 {
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&increment.to_le_bytes());
            out.push(flags & !0x40);
            for tile in self.tiles() {
                out.extend_from_slice(&tile.tx.to_le_bytes());
                out.extend_from_slice(&tile.ty.to_le_bytes());
            }
        }
        out
    }
}

/// Result of the post-discovery half of `Build::find_gather_tiles` (`0x00623350`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GatherRefresh {
    pub reserved_tiles: usize,
    pub move_to_back_steps: usize,
    pub rng_draws: usize,
}

/// Fail-closed errors at the authoritative terrain seam. Retail assumes these invariants
/// and would index raw memory; the Rust adapter refuses before mutating world or RNG state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatherRefreshError {
    PreviousLengthBeyondCurrent,
    LengthOverflow,
    InvalidTile(GatherTile),
}

/// Finish `Build::find_gather_tiles` after `BuildTypeData::find_gather_tcoords` has appended
/// authoritative cells to `list`.
///
/// A long-standing transcription trap is frozen here: retail does **not** add four weighted
/// duplicates per cell. When discovery grew the list, it performs `4 * new_len` iterations
/// of random-index, remove-first-by-value, append-to-tail. Length remains constant. A
/// one-element list takes the four move-to-back steps without drawing RNG at all.
pub fn finish_gather_tile_refresh(
    world: &mut World,
    list: &mut GatherMiningList,
    previous_len: usize,
    rng: &mut Random,
) -> Result<GatherRefresh, GatherRefreshError> {
    if previous_len > list.len() {
        return Err(GatherRefreshError::PreviousLengthBeyondCurrent);
    }
    if list.len() > i32::MAX as usize {
        return Err(GatherRefreshError::LengthOverflow);
    }
    if let Some(tile) = list
        .tiles()
        .iter()
        .copied()
        .find(|tile| !world.valid_t(tile.tx, tile.ty))
    {
        return Err(GatherRefreshError::InvalidTile(tile));
    }

    for tile in list.tiles() {
        world.set_gathered_at(tile.tx, tile.ty, true);
    }

    let mut result = GatherRefresh {
        reserved_tiles: list.len(),
        ..GatherRefresh::default()
    };
    if previous_len < list.len() {
        let len = list.len();
        result.move_to_back_steps = len
            .checked_mul(4)
            .ok_or(GatherRefreshError::LengthOverflow)?;
        for _ in 0..result.move_to_back_steps {
            let index = if len <= 1 {
                0
            } else {
                result.rng_draws += 1;
                (rng.get(0, 0xffff) % len as i32) as usize
            };
            let tile = list.tiles()[index];
            let moved = list.move_to_back(tile);
            debug_assert!(moved, "a value read from the list must still be removable");
        }
    }
    Ok(result)
}

/// The complete post-discovery mutation order, including the final `gather_max` store.
/// `authoritative_capacity` must be the result of the real `BuildTypeData::max_gatherers`
/// evaluation; this function intentionally provides no building/terrain fallback table.
pub fn finish_gather_tile_refresh_with_capacity(
    world: &mut World,
    list: &mut GatherMiningList,
    previous_len: usize,
    rng: &mut Random,
    site: &mut GatherSite,
    authoritative_capacity: i32,
) -> Result<GatherRefresh, GatherRefreshError> {
    let result = finish_gather_tile_refresh(world, list, previous_len, rng)?;
    site.set_authoritative_capacity(authoritative_capacity);
    Ok(result)
}

/// `Build::verify_gather_tiles` (`0x00623570`). The caller supplies the exact
/// territory/diplomacy predicate; failed coordinates lose TData reservation bit `0x1000`
/// and are removed first-match, without shrinking the array capacity.
pub fn verify_gather_tiles<F>(
    world: &mut World,
    list: &mut GatherMiningList,
    mut territory_valid: F,
) -> Result<usize, GatherRefreshError>
where
    F: FnMut(GatherTile) -> bool,
{
    if let Some(tile) = list
        .tiles()
        .iter()
        .copied()
        .find(|tile| !world.valid_t(tile.tx, tile.ty))
    {
        return Err(GatherRefreshError::InvalidTile(tile));
    }
    let mut removed = 0;
    let mut i = 0;
    while i < list.len() {
        let tile = list.tiles()[i];
        if territory_valid(tile) {
            i += 1;
        } else {
            world.set_gathered_at(tile.tx, tile.ty, false);
            let did_remove = list.remove_first(tile);
            debug_assert!(did_remove);
            removed += 1;
        }
    }
    Ok(removed)
}

/// The gathering portion of `Build::close` (`0x00628980`): release every reserved cell,
/// reset the two `MiningList` tail bytes, and set length to zero while retaining capacity.
/// The caller must separately mark the building-center terrain update record with `0x20`.
pub fn close_gather_tiles(
    world: &mut World,
    list: &mut GatherMiningList,
) -> Result<usize, GatherRefreshError> {
    if let Some(tile) = list
        .tiles()
        .iter()
        .copied()
        .find(|tile| !world.valid_t(tile.tx, tile.ty))
    {
        return Err(GatherRefreshError::InvalidTile(tile));
    }
    let released = list.len();
    for tile in list.tiles() {
        world.set_gathered_at(tile.tx, tile.ty, false);
    }
    list.mtn = -1;
    list.cliff = -1;
    list.tiles.clear();
    Ok(released)
}

/// Read the retail `WorldData::is_gathered_from` reservation bit (`0x1000`).
#[inline]
pub fn gather_tile_reserved(world: &World, tile: GatherTile) -> Option<bool> {
    world
        .valid_t(tile.tx, tile.ty)
        .then(|| world.is_gathered_from(tile.tx, tile.ty))
}

/// Apply `World::set_gathered_at` to coordinates already selected by the authoritative
/// type/terrain evaluator. Bounds are checked for the whole list before any mutation.
/// The bit is a site reservation/claim, not a resource-depletion amount.
pub fn set_gather_tiles_reserved(
    world: &mut World,
    tiles: &[GatherTile],
    reserved: bool,
) -> Result<(), GatherTile> {
    if let Some(invalid) = tiles
        .iter()
        .copied()
        .find(|tile| !world.valid_t(tile.tx, tile.ty))
    {
        return Err(invalid);
    }
    for tile in tiles {
        world.set_gathered_at(tile.tx, tile.ty, reserved);
    }
    Ok(())
}

/// The four worker TypeIndices accepted by `Build::add_gatherer` and
/// `UnitData::is_gathering_at` (`0x32..=0x35`).
#[inline]
pub fn is_building_gatherer_type(type_index: i32) -> bool {
    matches!(type_index, 0x32..=0x35)
}

/// The subset of a live GatherOrder needed to decide site membership.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherAssignment {
    /// `TargetOrder::whom` — owner of the target object.
    pub target_owner: i32,
    /// `TargetOrder::ox` — owner-local target object index.
    pub target_build: i32,
    /// `TargetOrder::uid`; `target_exists` uses it to reject a recycled object slot.
    pub target_uid: u16,
    /// `GatherOrder::been_there` (`+0x27`). `num_gatherers(1, 1)` requires it.
    pub been_there: bool,
    /// Types `0x34` and `0x35` can count while physically off-map and ultimately nested
    /// inside the target before consulting the current order. `None` means the worker is
    /// on-map or its `inside_up` chain does not end at a building.
    pub inside_target: Option<(u8, i16)>,
}

impl GatherAssignment {
    /// `TargetOrder::target_exists` (`0x0072FF10`) identity check after the live-bit gate.
    #[inline]
    pub fn targets_live_site(self, site: &GatherSite) -> bool {
        self.target_owner == i32::from(site.owner)
            && self.target_build == i32::from(site.build_o)
            && self.target_uid == site.uid
    }
}

/// Persistent gathering fields from one `UnitData` row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherWorker {
    pub owner: u8,
    pub unit_o: i16,
    pub type_index: i32,
    /// The object still passes the retail validity/unit virtual gates.
    pub valid_unit: bool,
    pub assignment: Option<GatherAssignment>,
    /// `UnitData::gather_down` (`+0x92`).
    pub gather_down: i16,
    /// `UnitData::good_obj` (`+0x94`), the nearby good selected by
    /// `UnitData::calc_gather`; `-1` when no good is selected.
    pub good_obj: i16,
    /// `UnitData::group` (`+0x80`). `do_non_flat_gather` detaches the unit from its group
    /// at the start of every tick.
    pub group: i16,
    /// `UnitData::unit_masks` (`+0x68`). Four high gather-action bits are cleared on
    /// non-flat reset and retirement.
    pub unit_masks: u32,
    /// `UnitData::doober` (`+0x86`), a held visual object for worker types `0x32..=0x35`.
    pub hold_doober: i16,
}

impl GatherWorker {
    pub fn new(owner: u8, unit_o: i16, type_index: i32) -> Self {
        Self {
            owner,
            unit_o,
            type_index,
            valid_unit: true,
            assignment: None,
            gather_down: NO_OBJECT,
            good_obj: NO_OBJECT,
            group: NO_OBJECT,
            unit_masks: 0,
            hold_doober: NO_OBJECT,
        }
    }

    /// `UnitData::is_gathering_at` (`0x00608880`) reduced to its persistent inputs.
    pub fn is_gathering_at(&self, site: &GatherSite, require_been_there: bool) -> bool {
        if !self.valid_unit
            || self.owner != site.owner
            || !is_building_gatherer_type(self.type_index)
        {
            return false;
        }
        let Some(a) = self.assignment else {
            return false;
        };
        if matches!(self.type_index, 0x34 | 0x35)
            && a.inside_target == Some((site.owner, site.build_o))
        {
            return true;
        }
        a.target_owner == i32::from(site.owner)
            && a.target_build == i32::from(site.build_o)
            && (!require_been_there || a.been_there)
    }
}

/// Persistent gathering fields from one `BuildData` row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherSite {
    pub owner: u8,
    pub build_o: i16,
    /// Object generation captured into new `GatherOrder::uid` values.
    pub uid: u16,
    /// `BuildData::gather_max` (`+0x80`), sign-extended by `max_gatherers`.
    pub gather_max: i8,
    /// `BuildData::gather_down` (`+0x70`).
    pub gather_down: i16,
    /// `WallData::mylos` / `BuildData::build_masks` (`+0x60`). Bit `0x800` latches the
    /// first arrived non-flat gatherer.
    pub build_masks: u16,
    /// `BuildData::recharging` (`+0x7A`), incremented once when that latch is first set.
    pub recharging: i16,
}

impl GatherSite {
    pub fn new(owner: u8, build_o: i16) -> Self {
        Self {
            owner,
            build_o,
            uid: 0,
            gather_max: 0,
            gather_down: NO_OBJECT,
            build_masks: 0,
            recharging: 0,
        }
    }

    /// Store the result returned by retail `BuildTypeData::max_gatherers`.
    ///
    /// That function clamps a negative computed capacity to zero; `Build::update_max_gatherers`
    /// (`0x00623310`) then stores only `AL`. The signed-byte round trip is intentional.
    pub fn set_authoritative_capacity(&mut self, computed: i32) {
        self.gather_max = computed.max(0) as u8 as i8;
    }

    #[inline]
    pub fn max_gatherers(&self) -> i32 {
        i32::from(self.gather_max)
    }
}

/// Which retail count the caller needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatherCount {
    /// `num_gatherers(0, 0)`: order targets the site, whether or not the worker arrived.
    Assigned,
    /// `num_gatherers(1, 1)`: requires `GatherOrder::been_there`.
    Active,
}

/// Result required by arena and command adapters. Retail's `add_gatherer` returns void;
/// these distinctions expose why it made no mutation without inventing another transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachResult {
    Attached,
    Full,
    AlreadyAttached,
    Invalid,
}

fn worker_pos(workers: &[GatherWorker], owner: u8, unit_o: i16) -> Option<usize> {
    workers
        .iter()
        .position(|w| w.owner == owner && w.unit_o == unit_o)
}

fn set_link(
    site: &mut GatherSite,
    workers: &mut [GatherWorker],
    previous: Option<usize>,
    next: i16,
) {
    if let Some(previous) = previous {
        workers[previous].gather_down = next;
    } else {
        site.gather_down = next;
    }
}

/// `Build::check_gatherers` (`0x0062F710`): unlink workers that no longer have a valid
/// gather order for this site. A corrupt/missing owner-local link fails rather than being
/// silently repaired; retail assumes its object table is internally valid too.
pub fn check_gatherers(
    site: &mut GatherSite,
    workers: &mut [GatherWorker],
) -> Result<usize, &'static str> {
    let mut previous = None;
    let mut current = site.gather_down;
    let mut visited = 0usize;
    let mut removed = 0usize;
    while current >= 0 {
        if visited >= workers.len() {
            return Err("gather chain contains a cycle");
        }
        visited += 1;
        let Some(pos) = worker_pos(workers, site.owner, current) else {
            return Err("gather chain references a missing owner-local unit");
        };
        let next = workers[pos].gather_down;
        if workers[pos].is_gathering_at(site, false) {
            previous = Some(pos);
        } else {
            set_link(site, workers, previous, next);
            workers[pos].gather_down = NO_OBJECT;
            removed += 1;
        }
        current = next;
    }
    Ok(removed)
}

/// Count the linked workers accepted by the corresponding retail `num_gatherers` mode.
/// `count_inside` is the separately evaluated `ObjectData::count_inside` contribution:
/// property `0x1A6` counts type `0x32`, and property `0x1A4` counts type `0x34` (with
/// inside mode `0x11` or `0x13`). Ordinary sites pass zero.
pub fn num_gatherers(
    site: &GatherSite,
    workers: &[GatherWorker],
    count: GatherCount,
    count_inside: i32,
) -> Result<i32, &'static str> {
    let require_been_there = count == GatherCount::Active;
    let mut total = count_inside;
    let mut current = site.gather_down;
    let mut visited = 0usize;
    while current >= 0 {
        if visited >= workers.len() {
            return Err("gather chain contains a cycle");
        }
        visited += 1;
        let Some(pos) = worker_pos(workers, site.owner, current) else {
            return Err("gather chain references a missing owner-local unit");
        };
        if workers[pos].is_gathering_at(site, require_been_there) {
            total = total.wrapping_add(1);
        }
        current = workers[pos].gather_down;
    }
    Ok(total)
}

/// Attach one worker against the site's already-refreshed authoritative capacity.
///
/// Capacity refresh is deliberately separate: retail `Build::add_gatherer` reads persistent
/// `BuildData::gather_max`; `Build::find_gather_tiles`/`update_max_gatherers` refresh it when
/// terrain state changes. No building-type fallback exists here.
pub fn attach_worker(
    site: &mut GatherSite,
    workers: &mut [GatherWorker],
    unit_o: i16,
) -> AttachResult {
    let Ok(occupied) = num_gatherers(site, workers, GatherCount::Assigned, 0) else {
        return AttachResult::Invalid;
    };
    if occupied >= site.max_gatherers() {
        return AttachResult::Full;
    }
    let Some(pos) = worker_pos(workers, site.owner, unit_o) else {
        return AttachResult::Invalid;
    };
    if !workers[pos].is_gathering_at(site, false)
        || !workers[pos]
            .assignment
            .is_some_and(|assignment| assignment.targets_live_site(site))
    {
        return AttachResult::Invalid;
    }

    let mut current = site.gather_down;
    while current >= 0 {
        if current == unit_o {
            return AttachResult::AlreadyAttached;
        }
        let Some(i) = worker_pos(workers, site.owner, current) else {
            return AttachResult::Invalid;
        };
        current = workers[i].gather_down;
    }
    if check_gatherers(site, workers).is_err() {
        return AttachResult::Invalid;
    }

    workers[pos].gather_down = site.gather_down;
    site.gather_down = unit_o;
    AttachResult::Attached
}

/// `Build::remove_gatherer` (`0x0062F8D0`). Returns whether the worker was linked.
pub fn detach_worker(
    site: &mut GatherSite,
    workers: &mut [GatherWorker],
    unit_o: i16,
) -> Result<bool, &'static str> {
    let mut previous = None;
    let mut current = site.gather_down;
    let mut visited = 0usize;
    while current >= 0 {
        if visited >= workers.len() {
            return Err("gather chain contains a cycle");
        }
        visited += 1;
        let Some(pos) = worker_pos(workers, site.owner, current) else {
            return Err("gather chain references a missing owner-local unit");
        };
        let next = workers[pos].gather_down;
        if current == unit_o {
            set_link(site, workers, previous, next);
            workers[pos].gather_down = NO_OBJECT;
            return Ok(true);
        }
        previous = Some(pos);
        current = next;
    }
    Ok(false)
}

/// High action bits owned by gathering animation/path state. `do_non_flat_gather` clears
/// these when `goto_build == 0`; `kill_current_order` clears the same mask on retirement.
pub const GATHER_ACTION_MASKS: u32 = 0x7800_0000;

/// The checksum-visible twenty-byte suffix of a live `GatherOrder`, with retail field
/// widths and offsets preserved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NonFlatGatherState {
    pub tx: i32,
    pub ty: i32,
    pub build_type: i32,
    pub wait: i32,
    pub goto_build: u8,
    pub non_flat_gather: u8,
    pub dist_mod: u8,
    pub been_there: u8,
}

impl NonFlatGatherState {
    #[inline]
    pub fn tile(&self) -> GatherTile {
        GatherTile {
            tx: self.tx,
            ty: self.ty,
        }
    }
}

/// Persistent effects produced at the entry of `Unit::do_non_flat_gather`
/// (`0x005F0170..0x005F0272`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NonFlatBegin {
    pub leader_economy_dirty: bool,
    pub latched_site_recharge: bool,
}

fn store_been_there(worker: &mut GatherWorker, state: &mut NonFlatGatherState, value: bool) {
    state.been_there = u8::from(value);
    if let Some(assignment) = worker.assignment.as_mut() {
        assignment.been_there = value;
    }
}

/// Execute the exact persistent mutations at the top of every non-flat gathering tick.
/// World lookup, animation, and pathfinding remain host operations and are not guessed.
pub fn begin_non_flat_gather_tick(
    site: &mut GatherSite,
    worker: &mut GatherWorker,
    state: &mut NonFlatGatherState,
) -> NonFlatBegin {
    let mut result = NonFlatBegin::default();
    if state.been_there != 0 && site.build_masks & 0x800 == 0 {
        site.recharging = site.recharging.wrapping_add(1);
        site.build_masks |= 0x800;
        result.latched_site_recharge = true;
    }
    worker.group = NO_OBJECT;
    if state.goto_build == 0 {
        worker.unit_masks &= !GATHER_ACTION_MASKS;
        if state.been_there == 0 {
            store_been_there(worker, state, true);
            result.leader_economy_dirty = true;
        }
    }
    result
}

/// Why the non-flat tile preparation returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NonFlatTileResult {
    /// This primitive was called outside retail's `goto_build != 0 && wait < 0` arm.
    WrongPhase,
    /// The order's existing coordinate still passed `WorldData::has_gather_access`.
    Existing(GatherTile),
    /// A candidate was selected and moved to the list tail.
    Selected(GatherTile),
    /// No list entry passed the resource/access predicate; retail consumes no RNG here.
    NoCandidate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NonFlatTilePreparation {
    pub result: NonFlatTileResult,
    pub leader_economy_dirty: bool,
    pub removed_hold_doober: Option<i16>,
}

/// Prepare the terrain coordinate used by the `wait < 0` arm of
/// `Unit::do_non_flat_gather` (`0x005F027F..0x005F0725`).
///
/// `candidate_eligible` stands for the exact conjunction of in-bounds, TData bit `0x4000`,
/// and `WorldData::has_gather_access(tile, owner, 1, 0)`. `site_origin` is the target
/// building coordinate after retail's coordinate-table conversion. `dist_mod_limit` is
/// the rules value at `Constants+0x9CC`; passing it explicitly avoids inventing a terrain
/// table.
pub fn prepare_non_flat_tile<SetDefaultAnim, RemoveDoober, Existing, Candidate>(
    list: &mut GatherMiningList,
    worker: &mut GatherWorker,
    state: &mut NonFlatGatherState,
    site_origin: GatherTile,
    dist_mod_limit: i32,
    target_has_property_1a2: bool,
    rng: &mut Random,
    set_default_animation: SetDefaultAnim,
    mut remove_hold_doober: RemoveDoober,
    mut existing_access: Existing,
    mut candidate_eligible: Candidate,
) -> NonFlatTilePreparation
where
    SetDefaultAnim: FnOnce(),
    RemoveDoober: FnMut(i16),
    Existing: FnMut(GatherTile) -> bool,
    Candidate: FnMut(GatherTile) -> bool,
{
    if state.goto_build == 0 || state.wait >= 0 {
        return NonFlatTilePreparation {
            result: NonFlatTileResult::WrongPhase,
            leader_economy_dirty: false,
            removed_hold_doober: None,
        };
    }
    let leader_economy_dirty = state.been_there == 0;
    if leader_economy_dirty {
        store_been_there(worker, state, true);
    }
    set_default_animation();

    let existing = state.tile();
    let mut removed_hold_doober = None;
    let result = if existing.tx >= 0 && existing.ty >= 0 && existing_access(existing) {
        NonFlatTileResult::Existing(existing)
    } else {
        if is_building_gatherer_type(worker.type_index) && worker.hold_doober >= 0 {
            let doober = worker.hold_doober;
            remove_hold_doober(doober);
            worker.hold_doober = NO_OBJECT;
            removed_hold_doober = Some(doober);
        }
        let effective_dist_mod =
            if list.mtn >= 0 && (list.len() as i32) < dist_mod_limit && state.dist_mod > 3 {
                3
            } else {
                state.dist_mod
            };
        let mut best_score = 0x0098_967f;
        let mut best_tile = None;
        for (index, tile) in list.tiles().iter().copied().enumerate() {
            if !candidate_eligible(tile) {
                continue;
            }
            let distance = vector_dist(
                tile.tx.wrapping_sub(site_origin.tx),
                tile.ty.wrapping_sub(site_origin.ty),
            )
            .max(3);
            let score = distance
                .wrapping_mul(i32::from(effective_dist_mod))
                .wrapping_add((index >> 2) as i32);
            if score < best_score {
                best_score = score;
                best_tile = Some(tile);
            }
        }
        let Some(tile) = best_tile else {
            return NonFlatTilePreparation {
                result: NonFlatTileResult::NoCandidate,
                leader_economy_dirty,
                removed_hold_doober,
            };
        };
        let moved = list.move_to_back(tile);
        debug_assert!(moved);
        state.tx = tile.tx;
        state.ty = tile.ty;
        NonFlatTileResult::Selected(tile)
    };

    let draw = rng.get(0, 0xffff);
    state.wait = draw % 200 + 400;
    state.goto_build = 0;
    if target_has_property_1a2 {
        worker.unit_masks |= 0x1000_0000;
    } else {
        worker.unit_masks |= 0x4000_0000;
        state.wait = 1_000_000;
    }
    NonFlatTilePreparation {
        result,
        leader_economy_dirty,
        removed_hold_doober,
    }
}

/// A successful world-coordinate result from `UnitType::find_nearby_spot`
/// (`0x0061DE70`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GatherNearbyPoint {
    pub x: Coord,
    pub y: Coord,
}

/// PDB `FilterIndex`; gathering always passes value 3.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct GatherFilterIndex(pub i32);

impl GatherFilterIndex {
    pub const GATHER: Self = Self(3);
}

/// Exact finder tuple used by the three non-flat gather movement sites.
///
/// Field order is the PDB signature's exact 16-argument order after the two output
/// pointers are removed from this value object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherNearbySpotRequest {
    pub centre: GatherNearbyPoint,
    pub min_radius: i32,
    pub max_radius: i32,
    pub radial_step: i32,
    pub base_angle: u32,
    pub filter: GatherFilterIndex,
    /// Stable TypeIndex whose full spatial/domain fields the host must resolve. Retail
    /// invokes the method on this `UnitType`; a generic free-space query is insufficient.
    pub worker_type: i32,
    pub worker_o: i32,
    pub worker_owner: i32,
    pub accept_without_collision: i32,
    pub expanded: i32,
    pub overlap_o: i32,
    pub overlap_owner: i32,
    pub required_region: i32,
}

impl GatherNearbySpotRequest {
    /// Normal target-building approach, including the far retry when `radius == 0x600`.
    pub const fn building(
        centre: GatherNearbyPoint,
        min_radius: i32,
        base_angle: u32,
        worker_type: i32,
        worker_o: i32,
        worker_owner: i32,
    ) -> Self {
        Self {
            centre,
            min_radius,
            max_radius: -1,
            radial_step: 0,
            base_angle,
            filter: GatherFilterIndex::GATHER,
            worker_type,
            worker_o,
            worker_owner,
            accept_without_collision: 0,
            expanded: 0,
            overlap_o: -1,
            overlap_owner: 0,
            required_region: -1,
        }
    }

    /// Selected terrain-tile approach (`min=0xC0`, `max=0x100`, `radial_step=2`).
    pub const fn terrain(
        centre: GatherNearbyPoint,
        base_angle: u32,
        worker_type: i32,
        worker_o: i32,
        worker_owner: i32,
    ) -> Self {
        Self {
            centre,
            min_radius: 0xc0,
            max_radius: 0x100,
            radial_step: 2,
            base_angle,
            filter: GatherFilterIndex::GATHER,
            worker_type,
            worker_o,
            worker_owner,
            accept_without_collision: 0,
            expanded: 0,
            overlap_o: -1,
            overlap_owner: 0,
            required_region: -1,
        }
    }
}

/// One deterministic geometric probe before the host applies region, terrain, overlap,
/// object-collision and ordered-collision gates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherNearbyProbe {
    pub radius: i32,
    pub phase: i32,
    pub angle: u32,
    pub point: GatherNearbyPoint,
}

/// Retail's 31 nonzero-radius phases: `0,+1,-1,...,+15,-15`.
pub const GATHER_NEARBY_PHASES: [i32; 31] = [
    0, 1, -1, 2, -2, 3, -3, 4, -4, 5, -5, 6, -6, 7, -7, 8, -8, 9, -9, 10, -10, 11, -11, 12, -12,
    13, -13, 14, -14, 15, -15,
];

/// Exact asymmetric phase-to-angle expression at `0x0061E017..0x0061E03F`.
#[inline]
pub fn gather_nearby_phase_angle(base_angle: u32, phase: i32) -> u32 {
    let correction = if (phase.wrapping_add(7) as u32) > 14 {
        0x0800_0000
    } else {
        0
    };
    base_angle
        .wrapping_add((phase as u32).wrapping_mul(0x1000_0000))
        .wrapping_add(correction)
}

/// Build the exact snapped geometry stream after the host resolves auto `max_radius` and
/// auto `radial_step` from the full worker UnitType.
///
/// A zero radius probes the snapped centre once. Nonzero radii use all 31 phases. Radius
/// progression is inclusive and advances by `max(1, radial_step)` without overshooting.
pub fn gather_nearby_probes(
    request: GatherNearbySpotRequest,
    resolved_max_radius: i32,
    resolved_radial_step: i32,
) -> Vec<GatherNearbyProbe> {
    let max_radius = resolved_max_radius.max(request.min_radius);
    let step = resolved_radial_step.max(1);
    let mut radius = request.min_radius;
    let mut result = Vec::new();
    loop {
        if radius == 0 {
            result.push(GatherNearbyProbe {
                radius,
                phase: 0,
                angle: request.base_angle,
                point: GatherNearbyPoint {
                    x: UCoord::from_coord(request.centre.x).centre(),
                    y: UCoord::from_coord(request.centre.y).centre(),
                },
            });
        } else {
            for phase in GATHER_NEARBY_PHASES {
                let angle = gather_nearby_phase_angle(request.base_angle, phase);
                let x = request.centre.x.0.wrapping_add(sinx(angle as i32, radius));
                let y = request.centre.y.0.wrapping_sub(cosx(angle as i32, radius));
                result.push(GatherNearbyProbe {
                    radius,
                    phase,
                    angle,
                    point: GatherNearbyPoint {
                        x: UCoord::from_coord(Coord(x)).centre(),
                        y: UCoord::from_coord(Coord(y)).centre(),
                    },
                });
            }
        }
        if radius >= max_radius {
            break;
        }
        radius = max_radius.min(radius.wrapping_add(step));
    }
    result
}

/// Radius passed for the ordinary building approach. Retail uses the smaller footprint
/// dimension, multiplied by half a tile (`0x60`), plus a quarter tile (`0x30`).
#[inline]
pub fn gather_building_approach_radius(x_size: i32, y_size: i32) -> i32 {
    x_size.min(y_size).wrapping_mul(0x60).wrapping_add(0x30)
}

/// Authoritative movement seam for the still-unported body of
/// `UnitType::find_nearby_spot`. Implementations must resolve `worker_type` to the complete
/// UnitType spatial/domain data and preserve the deterministic radius/angle probes, UCoord
/// snapping, region/terrain tests, object collision and ordered-collision gates. The
/// function itself has no RNG draw; callers must not substitute a random free-tile search.
pub trait AuthoritativeGatherPlacement {
    type Error;

    fn find_nearby_spot(
        &mut self,
        request: GatherNearbySpotRequest,
    ) -> Result<Option<GatherNearbyPoint>, Self::Error>;
}

/// Owner-local identity used by retail's bidirectional inside chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherInsideObject {
    pub owner: i8,
    pub object: i16,
}

/// Auditable result of `Object::insert_inside`/`remove_from_inside`, rather than a boolean
/// "seated" flag that cannot preserve nested identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherInsideReceipt {
    pub child: GatherInsideObject,
    pub container: GatherInsideObject,
    pub deepest_parent: GatherInsideObject,
}

/// `Unit::come_out` either finds a collision-valid point and commits the entire unlink /
/// world-placement transaction, or leaves the chain untouched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatherComeOutResult {
    Exited {
        point: GatherNearbyPoint,
        receipt: GatherInsideReceipt,
    },
    Blocked,
}

/// Fail-closed seam for the Scholar/special-flat containment path. Ordinary Farm,
/// Woodcutter and Mine workers never call this trait: gather-chain attachment happens
/// first and they remain on-map/outside. Implementations must own world removal/insertion,
/// `inside_down`/`inside_up` splicing, stats/dirty state and collision placement together.
pub trait AuthoritativeGatherContainment {
    type Error;

    fn enter_inside(
        &mut self,
        child: GatherInsideObject,
        container: GatherInsideObject,
    ) -> Result<GatherInsideReceipt, Self::Error>;

    fn come_out(
        &mut self,
        child: GatherInsideObject,
        mode: i32,
    ) -> Result<GatherComeOutResult, Self::Error>;
}

/// Timer arm selected by the worker's current gathering animation/location.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NonFlatWaitBand {
    /// Animation `0x19`: reschedule to `random % 100 + 300`.
    Animation19,
    /// Within `0x140` of the selected tile: reschedule to `random % 50 + 100`.
    NearTile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NonFlatWaitResult {
    Waiting,
    AllGathering,
    Rescheduled,
}

/// Decrement and service one of the two exact non-flat wait loops. RNG is consumed only
/// when the decrement reaches exactly zero and `Build::all_gathering` returns false.
pub fn tick_non_flat_wait<AllGathering>(
    state: &mut NonFlatGatherState,
    band: NonFlatWaitBand,
    rng: &mut Random,
    all_gathering: AllGathering,
) -> NonFlatWaitResult
where
    AllGathering: FnOnce() -> bool,
{
    state.wait = state.wait.wrapping_sub(1);
    if state.wait != 0 {
        return NonFlatWaitResult::Waiting;
    }
    if all_gathering() {
        state.wait = -1;
        return NonFlatWaitResult::AllGathering;
    }
    let draw = rng.get(0, 0xffff);
    state.wait = match band {
        NonFlatWaitBand::Animation19 => draw % 100 + 300,
        NonFlatWaitBand::NearTile => draw % 50 + 100,
    };
    NonFlatWaitResult::Rescheduled
}

/// Persistent reset shared by the failed `find_nearby_spot` and degenerate-point arms.
/// Returns the held doober id the host must remove, if retail would remove one.
pub fn reset_non_flat_destination(
    worker: &mut GatherWorker,
    state: &mut NonFlatGatherState,
    worker_type_gate: bool,
) -> Option<i16> {
    state.tx = -1;
    state.ty = -1;
    state.wait = -1;
    state.goto_build = 1;
    if state.dist_mod != 0 {
        state.dist_mod -= 1;
    }
    if !worker_type_gate || worker.hold_doober < 0 {
        return None;
    }
    let removed = worker.hold_doober;
    worker.hold_doober = NO_OBJECT;
    Some(removed)
}

/// Gather-specific side effects performed before `kill_current_order` removes the order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GatherRetirement {
    pub leader_economy_dirty: bool,
    pub detached: bool,
    pub removed_hold_doober: Option<i16>,
    pub cleaned_worker: Option<(u8, i16)>,
}

/// `kill_current_order`'s `GATHER` epilogue (`0x005E2D69..0x005E2ED4`). A live,
/// still-live target with matching raw `(whom, ox)` is detached immediately; this epilogue
/// does not compare `GatherOrder::uid`. A vanished target leaves the stale intrusive link
/// for `check_gatherers` to prune. `substituted_inside_worker` represents retail's guarded
/// `inside_down` identity substitution; callers pass `None` outside that live-game/type
/// gate. The order assignment is then cleared, action bits reset, and worker doobers retired.
pub fn retire_gather_order(
    site: Option<&mut GatherSite>,
    workers: &mut [GatherWorker],
    worker_owner: u8,
    unit_o: i16,
    substituted_inside_worker: Option<(u8, i16)>,
) -> Result<GatherRetirement, &'static str> {
    let Some(order_pos) = worker_pos(workers, worker_owner, unit_o) else {
        return Err("retired gatherer is missing");
    };
    let assignment = workers[order_pos].assignment;
    let (clean_owner, clean_o) = substituted_inside_worker.unwrap_or((worker_owner, unit_o));
    let Some(clean_pos) = worker_pos(workers, clean_owner, clean_o) else {
        return Err("resolved gather worker is missing");
    };
    let mut result = GatherRetirement {
        leader_economy_dirty: true,
        cleaned_worker: Some((clean_owner, clean_o)),
        ..GatherRetirement::default()
    };
    if let (Some(site), Some(assignment)) = (site, assignment) {
        if assignment.target_owner == i32::from(site.owner)
            && assignment.target_build == i32::from(site.build_o)
        {
            result.detached = detach_worker(site, workers, clean_o)?;
        }
    }
    workers[clean_pos].unit_masks &= !GATHER_ACTION_MASKS;
    if is_building_gatherer_type(workers[clean_pos].type_index)
        && workers[clean_pos].hold_doober >= 0
    {
        result.removed_hold_doober = Some(workers[clean_pos].hold_doober);
        workers[clean_pos].hold_doober = NO_OBJECT;
    }
    workers[order_pos].assignment = None;
    Ok(result)
}

/// Gross-income units contributed by one ordinary building worker before terrain/player
/// modifiers. `BuildTypeData::calc_gather` unscales `PEASANT_RATE`/`OIL_RATE` from 8.8,
/// then shifts the result left four before adding it to the six-slot leader income.
#[inline]
pub fn base_worker_gross(rules: &EconRules, oil: bool) -> i32 {
    economy::worker_rate(rules, oil).wrapping_shl(4)
}

/// Apply a fully evaluated per-worker six-slot terrain/type result to the live active
/// worker count. The caller is responsible for producing `per_worker` through the
/// authoritative `BuildTypeData::calc_gather` path, including bonuses and resource choice.
pub fn site_gross(
    per_worker: [i32; NUM_RESOURCES],
    active_workers: i32,
    gather_max: i8,
) -> [i32; NUM_RESOURCES] {
    let used = active_workers.max(0).min(i32::from(gather_max).max(0));
    per_worker.map(|v| v.wrapping_mul(used))
}

/// The crowding tail of `UnitData::calc_gather` (`0x00609180`). Resource nodes gathered
/// directly by units divide every slot independently by `competitors + 1`.
#[inline]
pub fn direct_unit_gross(
    evaluated: [i32; NUM_RESOURCES],
    competitors: i32,
) -> [i32; NUM_RESOURCES] {
    economy::share_among_gatherers(evaluated, competitors)
}

/// Move this frame's six gross-income slots through retail's `GATHER_RATE << 4`
/// accumulators. Commerce caps/expenses/handicaps are deliberately not repeated here;
/// `Leader::do_gather` applies those through `resource_tick` before this accumulator.
pub fn credit_gather_frame(
    income: [i32; NUM_RESOURCES],
    gather_rate: i32,
    accumulators: &mut [i32; NUM_RESOURCES],
) -> [i32; NUM_RESOURCES] {
    let period = resource_period(gather_rate);
    std::array::from_fn(|i| credit_resource(income[i], period, &mut accumulators[i]))
}

/// GatherOrder's checksum-visible scalar payload.
///
/// `GatherOrder::walk_data` (`0x00486E60`) walks the order base byte, TargetOrder's
/// `[ox, whom, uid]` ten-byte span, then GatherOrder's twenty bytes from `tx` through
/// `been_there`. Container metadata belongs to the surrounding `OrderList` walker.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GatherOrderWalk {
    pub order_base_byte: u8,
    pub ox: i32,
    pub whom: i32,
    pub uid: u16,
    pub tx: i32,
    pub ty: i32,
    pub build_type: i32,
    pub wait: i32,
    pub goto_build: u8,
    pub non_flat_gather: u8,
    pub dist_mod: u8,
    pub been_there: u8,
}

/// The complete network command payload accepted by `Unit::add_gather_order`
/// (`0x0061A5C0`): command `0x13`, target owner-local object index, and `QueuePos`.
/// The command does not transmit a UID; order construction captures the target's current
/// UID so later `TargetOrder::target_exists` can reject slot reuse.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GatherCommandWalk {
    pub opcode: u8,
    pub target_o: i32,
    pub queue_pos: i32,
}

impl GatherCommandWalk {
    pub const WALKED_BYTES: usize = 9;

    pub fn image(self) -> [u8; Self::WALKED_BYTES] {
        let mut out = [0; Self::WALKED_BYTES];
        out[0] = self.opcode;
        out[1..5].copy_from_slice(&self.target_o.to_le_bytes());
        out[5..9].copy_from_slice(&self.queue_pos.to_le_bytes());
        out
    }
}

impl GatherOrderWalk {
    pub const WALKED_BYTES: usize = 31;

    pub fn image(&self) -> [u8; Self::WALKED_BYTES] {
        let mut out = [0u8; Self::WALKED_BYTES];
        out[0] = self.order_base_byte;
        out[1..5].copy_from_slice(&self.ox.to_le_bytes());
        out[5..9].copy_from_slice(&self.whom.to_le_bytes());
        out[9..11].copy_from_slice(&self.uid.to_le_bytes());
        out[11..15].copy_from_slice(&self.tx.to_le_bytes());
        out[15..19].copy_from_slice(&self.ty.to_le_bytes());
        out[19..23].copy_from_slice(&self.build_type.to_le_bytes());
        out[23..27].copy_from_slice(&self.wait.to_le_bytes());
        out[27] = self.goto_build;
        out[28] = self.non_flat_gather;
        out[29] = self.dist_mod;
        out[30] = self.been_there;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestGatherTerrain {
        world: World,
        mountain: Vec<GatherTile>,
        cliff: Vec<GatherTile>,
        mountain_solid: Vec<GatherWorldCell>,
        mountain_size: Option<i32>,
        land: Option<LandGatherData>,
        mountain_candidate: Option<MiningObjectCandidate>,
        cliff_candidate: Option<MiningObjectCandidate>,
    }

    impl TestGatherTerrain {
        fn new(world: World) -> Self {
            Self {
                world,
                mountain: Vec::new(),
                cliff: Vec::new(),
                mountain_solid: Vec::new(),
                mountain_size: None,
                land: None,
                mountain_candidate: None,
                cliff_candidate: None,
            }
        }
    }

    impl AuthoritativeGatherTerrain for TestGatherTerrain {
        fn world_cell_dimensions(&self) -> (i32, i32) {
            (self.world.xs, self.world.ys)
        }

        fn tile_dimensions(&self) -> (i32, i32) {
            (self.world.tile_xs, self.world.tile_ys)
        }

        fn tile_mask(&self, tile: GatherTile) -> Option<u16> {
            self.world
                .valid_t(tile.tx, tile.ty)
                .then(|| self.world.tmask(tile.tx, tile.ty))
        }

        fn territory_owner(&self, wx: i32, wy: i32) -> Option<i32> {
            self.world
                .valid_w(wx, wy)
                .then(|| i32::from(self.world.wdata(wx, wy).who))
        }

        fn world_cell_flags(&self, wx: i32, wy: i32) -> Option<u16> {
            self.world
                .valid_w(wx, wy)
                .then(|| self.world.wdata(wx, wy).flags)
        }

        fn is_allied(&self, site_owner: i32, territory_owner: i32) -> Option<bool> {
            Some((site_owner, territory_owner) == (0, 1))
        }

        fn nearest_mining_object(
            &self,
            kind: MiningObjectKind,
            _site_x: Coord,
            _site_y: Coord,
            _site_region: i16,
        ) -> Option<MiningObjectCandidate> {
            match kind {
                MiningObjectKind::Mountain => self.mountain_candidate,
                MiningObjectKind::Cliff => self.cliff_candidate,
            }
        }

        fn mining_object_tiles(&self, kind: MiningObjectKind, index: i32) -> Option<&[GatherTile]> {
            let expected = match kind {
                MiningObjectKind::Mountain => self.mountain_candidate?.index,
                MiningObjectKind::Cliff => self.cliff_candidate?.index,
            };
            (index == expected).then_some(match kind {
                MiningObjectKind::Mountain => self.mountain.as_slice(),
                MiningObjectKind::Cliff => self.cliff.as_slice(),
            })
        }

        fn mining_object_size(&self, kind: MiningObjectKind, index: i32) -> Option<i32> {
            (kind == MiningObjectKind::Mountain && self.mountain_candidate?.index == index)
                .then_some(self.mountain_size?)
        }

        fn mountain_solid_world_cells(&self, index: i32) -> Option<&[GatherWorldCell]> {
            (self.mountain_candidate?.index == index).then_some(self.mountain_solid.as_slice())
        }

        fn land_gather_data(&self, _wx: i32, _wy: i32) -> Option<LandGatherData> {
            self.land
        }
    }

    fn assigned(owner: u8, unit_o: i16, target: i16, been_there: bool) -> GatherWorker {
        let mut w = GatherWorker::new(owner, unit_o, 0x32);
        w.assignment = Some(GatherAssignment {
            target_owner: i32::from(owner),
            target_build: i32::from(target),
            target_uid: 0,
            been_there,
            inside_target: None,
        });
        w
    }

    #[test]
    fn attach_and_detach_use_the_owner_local_short_chain() {
        let mut site = GatherSite::new(2, 41);
        site.set_authoritative_capacity(2);
        let mut workers = [assigned(2, 7, 41, true), assigned(2, 9, 41, true)];
        assert_eq!(
            attach_worker(&mut site, &mut workers, 7),
            AttachResult::Attached
        );
        assert_eq!(
            attach_worker(&mut site, &mut workers, 9),
            AttachResult::Attached
        );
        assert_eq!(site.gather_down, 9);
        assert_eq!(workers[1].gather_down, 7);
        assert_eq!(workers[0].gather_down, NO_OBJECT);
        assert_eq!(
            num_gatherers(&site, &workers, GatherCount::Active, 0),
            Ok(2)
        );

        assert_eq!(detach_worker(&mut site, &mut workers, 9), Ok(true));
        assert_eq!(site.gather_down, 7);
        assert_eq!(workers[1].gather_down, NO_OBJECT);
        assert_eq!(detach_worker(&mut site, &mut workers, 7), Ok(true));
        assert_eq!(site.gather_down, NO_OBJECT);
    }

    #[test]
    fn capacity_and_arrival_are_separate_retail_counts() {
        let mut site = GatherSite::new(0, 4);
        site.set_authoritative_capacity(2);
        let mut workers = [assigned(0, 1, 4, false), assigned(0, 2, 4, true)];
        assert_eq!(
            attach_worker(&mut site, &mut workers, 1),
            AttachResult::Attached
        );
        assert_eq!(
            attach_worker(&mut site, &mut workers, 2),
            AttachResult::Attached
        );
        assert_eq!(
            num_gatherers(&site, &workers, GatherCount::Assigned, 0),
            Ok(2)
        );
        assert_eq!(
            num_gatherers(&site, &workers, GatherCount::Active, 0),
            Ok(1)
        );
    }

    #[test]
    fn off_map_scholar_inside_target_counts_in_both_modes() {
        let mut site = GatherSite::new(0, 4);
        site.set_authoritative_capacity(1);
        let mut worker = assigned(0, 1, 99, false);
        worker.type_index = 0x34;
        worker.assignment.as_mut().unwrap().inside_target = Some((0, 4));
        let mut workers = [worker];
        // The inside arm is a membership/count special case, not an alternate
        // `Build::add_gatherer` order-identity gate.
        site.gather_down = 1;
        workers[0].gather_down = NO_OBJECT;
        assert_eq!(
            num_gatherers(&site, &workers, GatherCount::Assigned, 0),
            Ok(1)
        );
        assert_eq!(
            num_gatherers(&site, &workers, GatherCount::Active, 0),
            Ok(1)
        );
    }

    #[test]
    fn stale_orders_are_pruned_and_capacity_is_not_a_type_table() {
        let mut site = GatherSite::new(0, 12);
        let mut workers = [assigned(0, 3, 12, true), assigned(0, 5, 99, true)];
        workers[0].gather_down = 5;
        site.gather_down = 3;
        assert_eq!(check_gatherers(&mut site, &mut workers), Ok(1));
        assert_eq!(site.gather_down, 3);
        assert_eq!(workers[0].gather_down, NO_OBJECT);
        site.set_authoritative_capacity(0);
        assert_eq!(
            attach_worker(&mut site, &mut workers, 3),
            AttachResult::Full
        );
        assert_eq!(site.max_gatherers(), 0);
    }

    #[test]
    fn gather_tiles_are_reservations_not_depletion_counters() {
        let mut world = World::init_default_rules(4, 4);
        let tiles = [GatherTile { tx: 3, ty: 5 }, GatherTile { tx: 4, ty: 5 }];
        assert_eq!(gather_tile_reserved(&world, tiles[0]), Some(false));
        assert_eq!(set_gather_tiles_reserved(&mut world, &tiles, true), Ok(()));
        assert_eq!(gather_tile_reserved(&world, tiles[0]), Some(true));
        assert_eq!(gather_tile_reserved(&world, tiles[1]), Some(true));
        assert_eq!(set_gather_tiles_reserved(&mut world, &tiles, false), Ok(()));
        assert_eq!(gather_tile_reserved(&world, tiles[0]), Some(false));

        let invalid = GatherTile { tx: -1, ty: 0 };
        assert_eq!(
            set_gather_tiles_reserved(&mut world, &[tiles[0], invalid], true),
            Err(invalid)
        );
        assert_eq!(gather_tile_reserved(&world, tiles[0]), Some(false));
        assert_eq!(gather_tile_reserved(&world, invalid), None);
    }

    #[test]
    fn base_rate_and_accumulator_keep_retails_sixteenth_scale() {
        let rules = EconRules::shipped();
        assert_eq!(base_worker_gross(&rules, false), 160);
        assert_eq!(base_worker_gross(&rules, true), 560);
        let mut acc = [0; NUM_RESOURCES];
        let mut credited = [0; NUM_RESOURCES];
        for _ in 0..rules.gather_rate() {
            let frame = credit_gather_frame([160, 0, 0, 0, 0, 0], rules.gather_rate(), &mut acc);
            for i in 0..NUM_RESOURCES {
                credited[i] += frame[i];
            }
        }
        assert_eq!(credited, [10, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn retail_fixed_capacity_helpers_are_one_and_seven() {
        assert_eq!(MAX_FLAT_GATHERERS, 1);
        assert_eq!(MAX_KNOWLEDGE_GATHERERS, 7);
        assert_eq!(
            site_gross([0, 0, 0, 80, 0, 0], 8, MAX_KNOWLEDGE_GATHERERS as i8),
            [0, 0, 0, 560, 0, 0]
        );
    }

    #[test]
    fn generated_rules_bridge_preserves_all_six_scholar_rates_and_cap_seven() {
        let generated = don_rules::Rules::shipped();
        let rules = EconRules::from_block(&generated.raw);
        assert_eq!(
            std::array::from_fn::<_, 6, _>(|i| rules.scholar_rate(i)),
            [1280, 1792, 2560, 3840, 5120, 6400]
        );
        assert_eq!(
            std::array::from_fn::<_, 6, _>(|i| {
                economy::scholar_rate_for_level(&rules, i as i32 + 1)
            }),
            [80, 112, 160, 240, 320, 400]
        );
        assert_eq!(MAX_KNOWLEDGE_GATHERERS, 7);
    }

    #[test]
    fn site_capacity_and_direct_crowding_have_distinct_arithmetic() {
        assert_eq!(site_gross([160, 0, 0, 0, 0, 0], 5, 3), [480, 0, 0, 0, 0, 0]);
        assert_eq!(
            direct_unit_gross([11, -11, 0, 0, 0, 0], 2),
            [3, -3, 0, 0, 0, 0]
        );
    }

    #[test]
    fn gather_order_walk_has_the_three_retail_windows_in_order() {
        let order = GatherOrderWalk {
            order_base_byte: 0xaa,
            ox: 0x0102_0304,
            whom: 0x1112_1314,
            uid: 0x2122,
            tx: 0x3132_3334,
            ty: 0x4142_4344,
            build_type: 0x5152_5354,
            wait: 0x6162_6364,
            goto_build: 0x71,
            non_flat_gather: 0x72,
            dist_mod: 0x73,
            been_there: 0x74,
        };
        assert_eq!(
            order.image(),
            [
                0xaa, 4, 3, 2, 1, 0x14, 0x13, 0x12, 0x11, 0x22, 0x21, 0x34, 0x33, 0x32, 0x31, 0x44,
                0x43, 0x42, 0x41, 0x54, 0x53, 0x52, 0x51, 0x64, 0x63, 0x62, 0x61, 0x71, 0x72, 0x73,
                0x74,
            ]
        );
    }

    #[test]
    fn gather_command_is_nine_bytes_and_order_uid_rejects_slot_reuse() {
        let command = GatherCommandWalk {
            opcode: 0x13,
            target_o: 0x0102_0304,
            queue_pos: 2,
        };
        assert_eq!(command.image(), [0x13, 4, 3, 2, 1, 2, 0, 0, 0]);

        let mut site = GatherSite::new(3, 9);
        site.uid = 44;
        let current = GatherAssignment {
            target_owner: 3,
            target_build: 9,
            target_uid: 44,
            been_there: false,
            inside_target: None,
        };
        assert!(current.targets_live_site(&site));
        assert!(!GatherAssignment {
            target_uid: 45,
            ..current
        }
        .targets_live_site(&site));
    }

    #[test]
    fn mining_list_keeps_retails_zero_capacity_header_and_walk_order() {
        let mut list = GatherMiningList::default();
        assert_eq!(list.array_header(), (0, 0, -1, 0));
        assert_eq!(list.walked_image(), [0xff, 0xff, 0, 0, 0, 0]);

        list.mtn = 2;
        list.cliff = 3;
        list.add(GatherTile { tx: 1, ty: 2 });
        assert_eq!(list.array_header(), (1, 4, -1, 0));
        assert_eq!(
            list.walked_image(),
            [
                2, 3, // MiningList tail is walked first
                1, 0, 0, 0, // length
                4, 0, 0, 0, // capacity after first grow from zero
                0xff, 0xff, // increment
                0,    // flags with bit 0x40 cleared
                1, 0, 0, 0, 2, 0, 0, 0,
            ]
        );
    }

    #[test]
    fn retail_ring_prefix_uses_runtime_table_order_and_cumulative_counts() {
        assert_eq!(retail_gather_ring_prefix(0), Ok(vec![(0, 0)]));
        assert_eq!(
            retail_gather_ring_prefix(4),
            Ok(vec![
                (0, 0),
                (-1, -1),
                (-1, 0),
                (-1, 1),
                (0, -1),
                (0, 1),
                (1, -1),
                (1, 0),
                (1, 1),
            ])
        );
        assert_eq!(retail_gather_ring_prefix(8).unwrap().len(), 21);
        assert_eq!(
            retail_gather_ring_prefix(257),
            Err(GatherTerrainError::RadiusBeyondRetailTable(257))
        );
    }

    #[test]
    fn forest_discovery_uses_ring_distance_territory_and_reservation_before_row_major_tiles() {
        let mut world = World::init_default_rules(4, 4);
        // Site is the centre of W cell (1,1), T cell (6,6).
        world.set_tree_at(4, 4, true);
        world.set_tree_at(5, 4, true);
        world.set_tree_at(6, 2, true); // north W-cell, exactly radius 4
        world.set_tree_at(2, 2, true); // north-west centre is vector_dist(4,4)=6: excluded
        world.set_tree_at(10, 6, true); // east W-cell, but its centre reservation rejects it
        world.set_gathered_at(10, 6, true);
        let host = TestGatherTerrain::new(world);
        let mut list = GatherMiningList::default();
        let result = discover_gather_terrain(
            &host,
            &mut list,
            GatherTerrainRequest {
                kind: GatherTerrainKind::Forest,
                site_x: TCoord(6).centre(),
                site_y: TCoord(6).centre(),
                site_owner: 0,
                site_region: 64,
                gather_radius: 4,
            },
        )
        .unwrap();
        assert_eq!(result.ring_entries, 9);
        assert_eq!(result.appended, 3);
        // Ring entry (0,0) precedes shell 1; each W cell traverses fine tiles 0..15.
        assert_eq!(
            list.tiles(),
            &[
                GatherTile { tx: 4, ty: 4 },
                GatherTile { tx: 5, ty: 4 },
                GatherTile { tx: 6, ty: 2 },
            ]
        );
    }

    #[test]
    fn non_flat_order_access_uses_gather_edge_and_neighbour_territory_mode_zero_arm() {
        let mut world = World::init_default_rules(4, 4);
        let centre = GatherTile { tx: 6, ty: 6 };
        let mut host = TestGatherTerrain::new(world.clone());
        assert_eq!(has_non_flat_gather_access(&host, centre, 0), Ok(false));

        world.set_gather_edge(centre.tx, centre.ty);
        // N and S are blocked, E is water; W remains the one usable neighbour.
        world.set_blocked_at(6, 5, true);
        world.set_blocked_at(6, 7, true);
        world.set_tocean(7, 6);
        host.world = world;
        assert_eq!(has_non_flat_gather_access(&host, centre, 0), Ok(true));

        host.world.set_blocked_at(5, 6, true);
        assert_eq!(has_non_flat_gather_access(&host, centre, 0), Ok(false));
    }

    #[test]
    fn nearby_probe_geometry_keeps_phase_quirk_floor_snap_and_inclusive_radii() {
        assert_eq!(gather_nearby_phase_angle(0, 7), 0x7000_0000);
        assert_eq!(gather_nearby_phase_angle(0, 8), 0x8800_0000);
        assert_eq!(gather_nearby_phase_angle(0, -8), 0x8800_0000);

        let zero = GatherNearbySpotRequest {
            centre: GatherNearbyPoint {
                x: Coord(49),
                y: Coord(-1),
            },
            min_radius: 0,
            max_radius: 0,
            radial_step: 0,
            base_angle: 0,
            filter: GatherFilterIndex::GATHER,
            worker_type: 0x32,
            worker_o: 4,
            worker_owner: 0,
            accept_without_collision: 0,
            expanded: 0,
            overlap_o: -1,
            overlap_owner: 0,
            required_region: -1,
        };
        let probes = gather_nearby_probes(zero, 0, 0);
        assert_eq!(probes.len(), 1);
        assert_eq!(probes[0].point.x, Coord(72));
        assert_eq!(probes[0].point.y, Coord(-24));

        let terrain = GatherNearbySpotRequest::terrain(
            GatherNearbyPoint {
                x: Coord(1000),
                y: Coord(1000),
            },
            0,
            0x32,
            4,
            0,
        );
        let probes = gather_nearby_probes(terrain, 0x100, 2);
        assert_eq!(probes.len(), 33 * 31);
        assert_eq!(probes[0].radius, 0xc0);
        assert_eq!(probes[0].phase, 0);
        assert_eq!(probes[30].phase, -15);
        assert_eq!(probes.last().unwrap().radius, 0x100);
    }

    #[test]
    fn farm_moves_keep_gate_and_x_then_y_rng_boundaries() {
        let mut rng = Random::new(1);
        let before = rng.state();
        assert_eq!(
            initial_farm_gather_move(1, 0, 0, 10, 20, 6, 6, &mut rng),
            Ok(None)
        );
        assert_eq!(rng.state(), before);

        let move_ = initial_farm_gather_move(0, 0, 0, 10, 20, 2, 2, &mut rng)
            .unwrap()
            .unwrap();
        assert_eq!(move_.rng_draws, 0);
        assert_eq!(move_.destination.x, TCoord(11).centre());
        assert_eq!(move_.destination.y, TCoord(21).centre());
        assert_eq!(rng.state(), before);

        let move_ = farm_gather_relocation(10, 20, 2, 2, &mut rng).unwrap();
        assert_eq!(move_.rng_draws, 2);
        assert_ne!(rng.state(), before);
    }

    #[test]
    fn mine_discovery_selects_mountain_on_tie_and_keeps_object_coordinate_order() {
        let mut world = World::init_default_rules(4, 4);
        let tree = GatherTile { tx: 2, ty: 2 };
        let a = GatherTile { tx: 3, ty: 2 };
        let reserved = GatherTile { tx: 4, ty: 2 };
        let b = GatherTile { tx: 5, ty: 2 };
        world.set_tree_at(tree.tx, tree.ty, true);
        world.set_mountain_at(a.tx, a.ty, true);
        world.set_mountain_at(reserved.tx, reserved.ty, true);
        world.set_mountain_at(b.tx, b.ty, true);
        world.set_gathered_at(reserved.tx, reserved.ty, true);
        let mut host = TestGatherTerrain::new(world);
        host.mountain = vec![tree, a, reserved, b];
        host.cliff = vec![GatherTile { tx: 8, ty: 8 }];
        host.mountain_candidate = Some(MiningObjectCandidate {
            index: 5,
            distance: 6 * super::super::map_terrain::COORD_PER_TILE,
        });
        host.cliff_candidate = Some(MiningObjectCandidate {
            index: 9,
            distance: 6 * super::super::map_terrain::COORD_PER_TILE,
        });
        let mut list = GatherMiningList::default();
        let result = discover_gather_terrain(
            &host,
            &mut list,
            GatherTerrainRequest {
                kind: GatherTerrainKind::Mine,
                site_x: TCoord(6).centre(),
                site_y: TCoord(6).centre(),
                site_owner: 0,
                site_region: 64,
                gather_radius: 6,
            },
        )
        .unwrap();
        assert_eq!(
            result.selected_object,
            Some((MiningObjectKind::Mountain, 5))
        );
        assert_eq!(list.mtn, 5);
        assert_eq!(list.cliff, -1);
        assert_eq!(list.tiles(), &[a, b]);
    }

    #[test]
    fn wood_capacity_uses_land_make_then_the_two_per_access_cap() {
        let mut world = World::init_default_rules(4, 4);
        let centre = GatherTile { tx: 2, ty: 6 };
        let second_access = GatherTile { tx: 2, ty: 5 };
        world.set_gather_edge(centre.tx, centre.ty);
        world.set_gather_edge(second_access.tx, second_access.ty);
        let mut host = TestGatherTerrain::new(world);
        host.land = Some(LandGatherData {
            slots: [
                LandGatherSlot {
                    make: economy::RES_TIMBER as i32,
                    num_make: 3,
                },
                LandGatherSlot {
                    make: -1,
                    num_make: 0,
                },
                LandGatherSlot {
                    make: -1,
                    num_make: 0,
                },
                LandGatherSlot {
                    make: -1,
                    num_make: 0,
                },
            ],
        });
        let mut list = GatherMiningList::default();
        list.add(centre);
        list.add(second_access);
        assert_eq!(
            woodcutter_gather_capacity(
                &host,
                &list,
                WoodGatherCapacityRequest {
                    site_tx: 5,
                    site_ty: 5,
                    x_size: 2,
                    y_size: 2,
                    owner: 0,
                    bonuses: GatherCapacityBonuses {
                        french_tribe: true,
                        ..GatherCapacityBonuses::default()
                    },
                },
                GatherCapacityRules::shipped(),
            ),
            // Three LandData slots + one French slot, capped by 2 * two access points.
            Ok(4)
        );
    }

    #[test]
    fn mountain_capacity_scales_the_band_by_whitelisted_solid_cells() {
        let mut world = World::init_default_rules(4, 4);
        let a = GatherTile { tx: 2, ty: 2 };
        let b = GatherTile { tx: 6, ty: 2 };
        world.set_mountain_at(a.tx, a.ty, true);
        world.set_mountain_at(b.tx, b.ty, true);
        let mut host = TestGatherTerrain::new(world);
        host.mountain_candidate = Some(MiningObjectCandidate {
            index: 5,
            distance: 0,
        });
        host.mountain_size = Some(220); // shipped medium band => base 6
        host.mountain_solid = vec![
            GatherWorldCell { wx: 0, wy: 0 },
            GatherWorldCell { wx: 1, wy: 0 },
        ];
        let mut list = GatherMiningList::default();
        list.mtn = 5;
        list.add(a); // one of two denominator cells is assigned to this Mine
        assert_eq!(
            mine_gather_capacity(
                &host,
                &list,
                MineGatherCapacityRequest {
                    owner: 0,
                    bonuses: GatherCapacityBonuses::default(),
                },
                GatherCapacityRules::shipped(),
            ),
            Ok(3)
        );
    }

    #[test]
    fn find_gather_tiles_mixes_without_adding_duplicates() {
        let mut world = World::init_default_rules(4, 4);
        let a = GatherTile { tx: 1, ty: 1 };
        let b = GatherTile { tx: 2, ty: 1 };
        let c = GatherTile { tx: 3, ty: 1 };
        let mut list = GatherMiningList::default();
        list.add(a);
        list.add(b);
        list.add(c);
        let mut rng = Random::new(1);
        let mut site = GatherSite::new(0, 5);

        assert_eq!(
            finish_gather_tile_refresh_with_capacity(
                &mut world, &mut list, 0, &mut rng, &mut site, 7,
            ),
            Ok(GatherRefresh {
                reserved_tiles: 3,
                move_to_back_steps: 12,
                rng_draws: 12,
            })
        );
        assert_eq!(list.tiles(), &[a, c, b]);
        assert_eq!(list.len(), 3, "the four-N loop never changes length");
        assert_eq!(site.max_gatherers(), 7);
        assert_eq!(rng.state() as u32, 0x6c20_f30d);
        assert_eq!(gather_tile_reserved(&world, a), Some(true));
        assert_eq!(gather_tile_reserved(&world, b), Some(true));
        assert_eq!(gather_tile_reserved(&world, c), Some(true));
    }

    #[test]
    fn one_tile_refresh_performs_four_rotations_and_zero_draws() {
        let mut world = World::init_default_rules(2, 2);
        let tile = GatherTile { tx: 1, ty: 1 };
        let mut list = GatherMiningList::default();
        list.add(tile);
        let mut rng = Random::new(0x1234_5678);
        let before = rng.state();
        let result = finish_gather_tile_refresh(&mut world, &mut list, 0, &mut rng).unwrap();
        assert_eq!(result.move_to_back_steps, 4);
        assert_eq!(result.rng_draws, 0);
        assert_eq!(rng.state(), before);
        assert_eq!(list.tiles(), &[tile]);
    }

    #[test]
    fn verify_and_close_release_reservations_without_shrinking_capacity() {
        let mut world = World::init_default_rules(3, 3);
        let keep = GatherTile { tx: 1, ty: 1 };
        let drop = GatherTile { tx: 2, ty: 1 };
        let mut list = GatherMiningList::default();
        list.add(keep);
        list.add(drop);
        list.add(drop);
        list.mtn = 4;
        list.cliff = 5;
        let mut rng = Random::new(1);
        let previous_len = list.len();
        finish_gather_tile_refresh(&mut world, &mut list, previous_len, &mut rng).unwrap();

        assert_eq!(
            verify_gather_tiles(&mut world, &mut list, |t| t != drop),
            Ok(2)
        );
        assert_eq!(list.tiles(), &[keep]);
        assert_eq!(gather_tile_reserved(&world, drop), Some(false));
        assert_eq!(gather_tile_reserved(&world, keep), Some(true));
        assert_eq!(close_gather_tiles(&mut world, &mut list), Ok(1));
        assert_eq!(list.array_header(), (0, 4, -1, 0));
        assert_eq!((list.mtn, list.cliff), (-1, -1));
        assert_eq!(gather_tile_reserved(&world, keep), Some(false));
    }

    fn non_flat_state() -> NonFlatGatherState {
        NonFlatGatherState {
            tx: -1,
            ty: -1,
            build_type: -1,
            wait: -1,
            goto_build: 1,
            non_flat_gather: 1,
            dist_mod: 8,
            been_there: 1,
        }
    }

    #[test]
    fn non_flat_selection_uses_score_rotates_and_consumes_one_timer_draw() {
        let near = GatherTile { tx: 3, ty: 0 };
        let far = GatherTile { tx: 10, ty: 0 };
        let mut list = GatherMiningList::default();
        list.mtn = 0;
        list.add(near);
        list.add(far);
        let mut worker = GatherWorker::new(0, 2, 0x32);
        worker.assignment = Some(GatherAssignment {
            target_owner: 0,
            target_build: 4,
            target_uid: 0,
            been_there: true,
            inside_target: None,
        });
        let mut state = non_flat_state();
        state.been_there = 0;
        worker.assignment.as_mut().unwrap().been_there = false;
        let mut rng = Random::new(1);

        assert_eq!(
            prepare_non_flat_tile(
                &mut list,
                &mut worker,
                &mut state,
                GatherTile { tx: 0, ty: 0 },
                100,
                true,
                &mut rng,
                || {},
                |_| {},
                |_| false,
                |_| true,
            ),
            NonFlatTilePreparation {
                result: NonFlatTileResult::Selected(near),
                leader_economy_dirty: true,
                removed_hold_doober: None,
            }
        );
        assert_eq!(list.tiles(), &[far, near]);
        assert_eq!(state.tile(), near);
        assert_eq!(state.wait, 491);
        assert_eq!(state.goto_build, 0);
        assert_eq!(state.been_there, 1);
        assert_eq!(worker.assignment.unwrap().been_there, true);
        assert_eq!(worker.unit_masks, 0x1000_0000);
        assert_eq!(rng.state() as u32, 0x3c88_596c);
    }

    #[test]
    fn non_flat_no_candidate_consumes_no_rng_and_non_1a2_uses_long_wait() {
        let mut list = GatherMiningList::default();
        list.add(GatherTile { tx: 2, ty: 2 });
        let mut worker = GatherWorker::new(0, 2, 0x32);
        worker.hold_doober = 7;
        let mut state = non_flat_state();
        let mut rng = Random::new(9);
        let before = rng.state();
        assert_eq!(
            prepare_non_flat_tile(
                &mut list,
                &mut worker,
                &mut state,
                GatherTile::default(),
                5,
                false,
                &mut rng,
                || {},
                |_| {},
                |_| false,
                |_| false,
            ),
            NonFlatTilePreparation {
                result: NonFlatTileResult::NoCandidate,
                leader_economy_dirty: false,
                removed_hold_doober: Some(7),
            }
        );
        assert_eq!(rng.state(), before);
        assert_eq!(state.wait, -1);
        assert_eq!(worker.hold_doober, NO_OBJECT);

        state.tx = 2;
        state.ty = 2;
        assert_eq!(
            prepare_non_flat_tile(
                &mut list,
                &mut worker,
                &mut state,
                GatherTile::default(),
                5,
                false,
                &mut rng,
                || {},
                |_| {},
                |_| true,
                |_| false,
            ),
            NonFlatTilePreparation {
                result: NonFlatTileResult::Existing(GatherTile { tx: 2, ty: 2 }),
                leader_economy_dirty: false,
                removed_hold_doober: None,
            }
        );
        assert_eq!(state.wait, 1_000_000);
        assert_eq!(state.goto_build, 0);
        assert_eq!(worker.unit_masks, 0x4000_0000);
    }

    #[test]
    fn non_flat_entry_reset_wait_and_retirement_preserve_exact_mutation_order() {
        let mut site = GatherSite::new(1, 7);
        site.uid = 99;
        site.set_authoritative_capacity(1);
        site.recharging = 4;
        let mut worker = assigned(1, 3, 7, true);
        worker.assignment.as_mut().unwrap().target_uid = 99;
        worker.unit_masks = u32::MAX;
        worker.hold_doober = 12;
        worker.good_obj = 6;
        worker.group = 4;
        let mut state = non_flat_state();
        state.goto_build = 0;

        assert_eq!(
            begin_non_flat_gather_tick(&mut site, &mut worker, &mut state),
            NonFlatBegin {
                leader_economy_dirty: false,
                latched_site_recharge: true,
            }
        );
        assert_eq!(site.recharging, 5);
        assert_eq!(site.build_masks & 0x800, 0x800);
        assert_eq!(worker.group, NO_OBJECT);
        assert_eq!(worker.good_obj, 6, "the +0x80 write is group, not good_obj");
        assert_eq!(worker.unit_masks, u32::MAX & !GATHER_ACTION_MASKS);

        state.wait = 1;
        let mut rng = Random::new(1);
        assert_eq!(
            tick_non_flat_wait(&mut state, NonFlatWaitBand::Animation19, &mut rng, || false),
            NonFlatWaitResult::Rescheduled
        );
        assert_eq!(state.wait, 391);
        assert_eq!(
            reset_non_flat_destination(&mut worker, &mut state, true),
            Some(12)
        );
        assert_eq!(
            (state.tx, state.ty, state.wait, state.dist_mod),
            (-1, -1, -1, 7)
        );
        assert_eq!(state.goto_build, 1);

        let mut workers = [worker];
        assert_eq!(
            attach_worker(&mut site, &mut workers, 3),
            AttachResult::Attached
        );
        workers[0].unit_masks = u32::MAX;
        workers[0].hold_doober = 21;
        assert_eq!(
            retire_gather_order(Some(&mut site), &mut workers, 1, 3, None),
            Ok(GatherRetirement {
                leader_economy_dirty: true,
                detached: true,
                removed_hold_doober: Some(21),
                cleaned_worker: Some((1, 3)),
            })
        );
        assert_eq!(site.gather_down, NO_OBJECT);
        assert_eq!(workers[0].gather_down, NO_OBJECT);
        assert_eq!(workers[0].assignment, None);
        assert_eq!(workers[0].unit_masks, u32::MAX & !GATHER_ACTION_MASKS);
        assert_eq!(workers[0].hold_doober, NO_OBJECT);
    }
}
