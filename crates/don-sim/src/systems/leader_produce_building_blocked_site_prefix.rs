//! Exact entry prefix of `BuildTypeData::blocked_site`
//! (`0x00636A50..0x00636BCC`).
//!
//! The reached `Leader::produce_building` call always supplies the literal City constraint
//! `-1`. Retail converts the candidate Coord pair to the footprint's top-left TCoord, asks the
//! canonical non-strict Type relation whether the target is a City, skips the City-only stale
//! capital scan because the constraint is negative, and begins the footprint walk in x-outer /
//! y-inner order. This tranche also follows the reached ordinary-land
//! `BuildTypeData::blocked_tcoord` path through the exact `get_good` switch and TCoord land
//! classification. The source-backed continuation executes the complete read-only
//! `LandData::get_amount` and repeats the admitted ordinary-land path over the full footprint.
//! The installed Farm continuations then own both the reached dry/unowned-territory exit and
//! the dry/self-owned Town/Farm-capacity success from `BuildTypeData::blocked_location`, plus
//! the final non-immediate `blocked_site` return filter.
//! Nothing in this module mutates Sim.

use crate::objects::{Band, BUILD_BAND_BASE};
use crate::tick::{Sim, NUM_LEADERS};

use super::bhs_type_table::{TypeBuiltinState, TypeDomain};
use super::gather_terrain::{GatherTerrainMaterialization, GatherTerrainMaterializationError};
use super::gathering::LandGatherData;
use super::leader_produce_building_candidate_prefix::{
    LeaderProduceBuildingBlockedSiteBoundary, BUILD_TYPE_BLOCKED_SITE_VA,
    LEADER_PRODUCE_BUILDING_BLOCKED_SITE_BYTES_REMAINING,
    LEADER_PRODUCE_BUILDING_BLOCKED_SITE_CALL_VA,
};
use super::map_terrain::{tflag, wflag, Coord, TCoord};
use super::production::{
    flag,
    runtime::{LiveProductionRuntime, LiveTypeClass},
    Footprint,
};
use super::tech_cities::{city_radius, vector_dist, CityRules};

pub const BUILD_TYPE_BLOCKED_SITE_END_VA: u32 = 0x0063_6d77;
pub const BUILD_TYPE_BLOCKED_TCOORD_VA: u32 = 0x0063_6db0;
pub const BUILD_TYPE_BLOCKED_SITE_BLOCKED_TCOORD_CALL_VA: u32 = 0x0063_6bcc;
pub const BUILD_TYPE_BLOCKED_SITE_PREFIX_BYTES: u32 =
    BUILD_TYPE_BLOCKED_SITE_BLOCKED_TCOORD_CALL_VA - BUILD_TYPE_BLOCKED_SITE_VA;
pub const BUILD_TYPE_BLOCKED_SITE_BYTES_REMAINING: u32 =
    BUILD_TYPE_BLOCKED_SITE_END_VA - BUILD_TYPE_BLOCKED_SITE_BLOCKED_TCOORD_CALL_VA;
pub const BUILD_TYPE_BLOCKED_TCOORD_END_VA: u32 = 0x0063_75ac;
pub const BUILD_TYPE_GET_GOOD_VA: u32 = 0x0063_bd50;
pub const BUILD_TYPE_BLOCKED_TCOORD_GET_GOOD_CALL_VA: u32 = 0x0063_751c;
pub const WORLD_DATA_GET_LAND_TCOORD_VA: u32 = 0x006b_4c70;
pub const BUILD_TYPE_BLOCKED_TCOORD_GET_LAND_CALL_VA: u32 = 0x0063_7532;
pub const LAND_DATA_GET_AMOUNT_VA: u32 = 0x0067_e6d0;
pub const BUILD_TYPE_BLOCKED_TCOORD_GET_AMOUNT_CALL_VA: u32 = 0x0063_7545;
pub const BUILD_TYPE_BLOCKED_TCOORD_LAND_PREFIX_BYTES: u32 =
    BUILD_TYPE_BLOCKED_TCOORD_GET_AMOUNT_CALL_VA - BUILD_TYPE_BLOCKED_TCOORD_VA;
pub const BUILD_TYPE_BLOCKED_TCOORD_BYTES_REMAINING: u32 =
    BUILD_TYPE_BLOCKED_TCOORD_END_VA - BUILD_TYPE_BLOCKED_TCOORD_GET_AMOUNT_CALL_VA;
pub const BUILD_TYPE_BLOCKED_LOCATION_VA: u32 = 0x0063_75b0;
pub const BUILD_TYPE_BLOCKED_LOCATION_END_VA: u32 = 0x0063_89b0;
pub const BUILD_TYPE_NON_FRIENDLY_TERRITORY_VA: u32 = 0x0063_89c0;
pub const BUILD_TYPE_BLOCKED_LOCATION_NON_FRIENDLY_CALL_VA: u32 = 0x0063_768b;
pub const BUILD_TYPE_BLOCKED_SITE_BLOCKED_LOCATION_CALL_VA: u32 = 0x0063_6d18;
pub const BUILD_TYPE_BLOCKED_SITE_BLOCKED_LOCATION_BYTES_REMAINING: u32 =
    BUILD_TYPE_BLOCKED_SITE_END_VA - BUILD_TYPE_BLOCKED_SITE_BLOCKED_LOCATION_CALL_VA;
pub const CITY_TYPE: usize = 414;
pub const OIL_WELL_TYPE: usize = 421;
pub const OIL_PLATFORM_TYPE: usize = 422;
pub const DOCK_TYPE: usize = 432;
pub const FORT_TYPE: usize = 443;
pub const FARM_TYPE: usize = 417;
pub const GAME_SEMAPHORE_IMMEDIATE_BIT: u32 = 11;
pub const LAKOTA_TRIBE: i32 = 19;
pub const LEADER_PRODUCE_BUILDING_SUCCESSFUL_SITE_VA: u32 = 0x006e_1e82;
pub const LEADER_PRODUCE_BUILDING_SUCCESSFUL_SITE_BYTES_REMAINING: u32 = 0x126c;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildTypeBlockedTcoordBoundary {
    pub va: u32,
    pub callee_va: u32,
    pub blocked_site_bytes_remaining: u32,
    pub owner: u8,
    pub type_index: i32,
    pub candidate_world_cell: [i32; 2],
    pub placement_coord: [i32; 2],
    pub footprint_corner: [i32; 2],
    /// First tile in retail's x-outer / y-inner footprint walk.
    pub tile: [i32; 2],
    pub city_constraint: i32,
    pub blocked_detail_initial: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaderProduceBuildingBlockedSitePrefixReceipt {
    pub input: LeaderProduceBuildingBlockedSiteBoundary,
    pub target_footprint: Footprint,
    pub target_is_city: bool,
    pub placement_tcoord: [i32; 2],
    pub footprint_corner: [i32; 2],
    pub native_returned: Option<i32>,
    pub continuation: BuildTypeBlockedTcoordBoundary,
}

impl LeaderProduceBuildingBlockedSitePrefixReceipt {
    pub fn validates(&self) -> bool {
        self.native_returned.is_none()
            && self.input.city_constraint == -1
            && self.continuation.va == BUILD_TYPE_BLOCKED_SITE_BLOCKED_TCOORD_CALL_VA
            && self.continuation.callee_va == BUILD_TYPE_BLOCKED_TCOORD_VA
            && self.continuation.blocked_site_bytes_remaining
                == BUILD_TYPE_BLOCKED_SITE_BYTES_REMAINING
            && self.continuation.owner == self.input.owner
            && self.continuation.type_index == self.input.type_index
            && self.continuation.placement_coord == self.input.placement_coord
            && self.continuation.footprint_corner == self.footprint_corner
            && self.continuation.tile == self.footprint_corner
            && self.continuation.city_constraint == -1
            && self.continuation.blocked_detail_initial == 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeaderProduceBuildingBlockedSitePrefixError {
    WrongBoundary {
        va: u32,
        callee_va: u32,
        bytes_remaining: u32,
    },
    InvalidOwner {
        owner: usize,
    },
    InvalidType {
        type_index: i32,
    },
    TargetTypeMismatch {
        type_index: i32,
    },
    MissingTargetFootprint,
    MissingTargetDomain,
    InvalidTargetFootprint {
        footprint: Footprint,
    },
    FootprintMismatch,
    DomainMismatch,
    CandidateCoordinateMismatch,
    UnsupportedCityConstraint {
        city_constraint: i32,
    },
    BlockedDetailMismatch {
        blocked_detail_initial: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LandDataGetAmountBoundary {
    pub va: u32,
    pub callee_va: u32,
    pub blocked_tcoord_bytes_remaining: u32,
    pub owner: u8,
    pub type_index: i32,
    pub tile: [i32; 2],
    pub city_constraint: i32,
    pub was_seen: bool,
    /// Exact `EBX` truth value retained by `blocked_tcoord`: the result of
    /// `WorldData::was_seen`, OR the immediate-game semaphore override.
    pub seen_or_immediate: bool,
    pub world_region: i16,
    pub terrain_mask: u16,
    pub build_flags: u32,
    /// Exact `BuildTypeData::get_good` switch result retained as the first argument.
    pub good: i32,
    /// Exact canonical `WorldData::get_land(TCoord, TCoord, 1)` result selecting LandData.
    pub land_index: i32,
    /// The second `LandData::get_amount` argument. Retail passes the TData linear index even
    /// though the recovered retail callee does not read it.
    pub tile_linear_index: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LandDataGetAmountReceipt {
    pub input: LandDataGetAmountBoundary,
    pub land_name: String,
    pub land_gather: LandGatherData,
    pub returned: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildTypeBlockedTcoordLandReceipt {
    pub prefix: BuildTypeBlockedTcoordPrefixReceipt,
    pub amount: LandDataGetAmountReceipt,
    /// Exact raw child return consumed by `blocked_site` after the complete resource arm.
    pub raw_returned_to_blocked_site: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildTypeBlockedLocationBoundary {
    pub va: u32,
    pub callee_va: u32,
    pub blocked_site_bytes_remaining: u32,
    pub owner: u8,
    pub type_index: i32,
    pub placement_coord: [i32; 2],
    pub footprint_corner: [i32; 2],
    pub city_constraint: i32,
    pub blocked_detail: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaderProduceBuildingBlockedSiteFootprintReceipt {
    pub entry: LeaderProduceBuildingBlockedSitePrefixReceipt,
    /// Retail order is x outer / y inner.
    pub tiles: Vec<BuildTypeBlockedTcoordLandReceipt>,
    pub blocked_detail: i32,
    pub locally_seen_tiles: i32,
    pub continuation: BuildTypeBlockedLocationBoundary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildTypeNonFriendlyTerritoryRead {
    pub tile: [i32; 2],
    pub terrain_mask: u16,
    pub territory_owner: i8,
}

/// Complete reached Farm verdict from `blocked_location` and the final `blocked_site` filter.
///
/// Retail scans the footprint x-outer/y-inner. Water tiles do not consult WData ownership;
/// every reached installed tile is dry and therefore records the canonical `WData::who` byte.
/// An unowned byte (`-1`) raises `non_friendly_territory` to one. A non-Lakota owner then exits
/// `blocked_location` with `0x1a`, before any Town, City-capacity, water-count, or gather reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaderProduceBuildingBlockedSiteFarmUnownedReceipt {
    pub input: LeaderProduceBuildingBlockedSiteFootprintReceipt,
    pub territory_reads: Vec<BuildTypeNonFriendlyTerritoryRead>,
    pub non_friendly_territory: i32,
    pub lakota_bonus: bool,
    pub blocked_location_returned: i32,
    pub immediate: bool,
    /// Exact scalar returned by `BuildTypeData::blocked_site` to `Leader::produce_building`.
    pub native_returned: i32,
}

impl LeaderProduceBuildingBlockedSiteFarmUnownedReceipt {
    pub fn validates(&self) -> bool {
        self.input.continuation.va == BUILD_TYPE_BLOCKED_SITE_BLOCKED_LOCATION_CALL_VA
            && self.input.continuation.callee_va == BUILD_TYPE_BLOCKED_LOCATION_VA
            && self.input.continuation.blocked_site_bytes_remaining
                == BUILD_TYPE_BLOCKED_SITE_BLOCKED_LOCATION_BYTES_REMAINING
            && self.input.continuation.type_index == FARM_TYPE as i32
            && self.territory_reads.len() == self.input.tiles.len()
            && self.territory_reads.iter().all(|read| {
                read.terrain_mask & tflag::SURFACE_MASK != tflag::SURFACE_WATER
                    && read.territory_owner == -1
            })
            && self.non_friendly_territory == 1
            && !self.lakota_bonus
            && !self.immediate
            && self.blocked_location_returned == 0x1a
            && self.native_returned == self.blocked_location_returned
    }
}

/// Exact Town/City-capacity reads on the reached friendly Farm path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildTypeFarmTownRead {
    pub city_slot: usize,
    pub city_object: i16,
    pub center_type: i32,
    pub distance: i32,
    pub radius: i32,
    pub counted_farms: i32,
    /// The least possible installed-retail result of `CityData::get_farm_limit`.
    /// Egyptian and Olive Oil bonuses can only increase this value.
    pub farm_limit_lower_bound: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaderProduceBuildingSuccessfulSiteBoundary {
    pub va: u32,
    pub bytes_remaining: u32,
    pub owner: u8,
    pub type_index: i32,
    pub origin_build_object: i16,
    pub circle_offset: i32,
    pub candidate_world_cell: [i32; 2],
    pub placement_coord: [i32; 2],
}

/// Complete reached Farm verdict in self-owned, dry City territory.
///
/// This cone intentionally does not guess diplomacy. Every WData cell is owned by the calling
/// Leader, `get_town` selects one canonical active City, and that City's Build chain contains no
/// Farm. Since retail's Farm limit is at least five, the exact capacity comparison succeeds
/// without needing the still-unprojected Olive Oil bonus. Domain zero contributes no water, and
/// build flag `0x10000000` returns zero before `calc_gather`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaderProduceBuildingBlockedSiteFarmOwnedReceipt {
    pub input: LeaderProduceBuildingBlockedSiteFootprintReceipt,
    pub territory_reads: Vec<BuildTypeNonFriendlyTerritoryRead>,
    pub non_friendly_territory: i32,
    pub lakota_bonus: bool,
    pub town: BuildTypeFarmTownRead,
    pub water_tiles: i32,
    pub blocked_location_returned: i32,
    pub immediate: bool,
    pub native_returned: i32,
    /// First instruction after the successful `blocked_site` call. Candidate scoring, RNG,
    /// best-site selection, builder selection, and construction mutation begin here.
    pub continuation: LeaderProduceBuildingSuccessfulSiteBoundary,
}

impl LeaderProduceBuildingBlockedSiteFarmOwnedReceipt {
    pub fn validates(&self) -> bool {
        self.input.continuation.va == BUILD_TYPE_BLOCKED_SITE_BLOCKED_LOCATION_CALL_VA
            && self.input.continuation.callee_va == BUILD_TYPE_BLOCKED_LOCATION_VA
            && self.input.continuation.type_index == FARM_TYPE as i32
            && self.territory_reads.len() == self.input.tiles.len()
            && self.territory_reads.iter().all(|read| {
                read.terrain_mask & tflag::SURFACE_MASK != tflag::SURFACE_WATER
                    && read.territory_owner == self.input.continuation.owner as i8
            })
            && self.non_friendly_territory == 0
            && self.town.counted_farms < self.town.farm_limit_lower_bound
            && self.town.farm_limit_lower_bound == CityRules::RETAIL.farms_per_city_base
            && self.water_tiles == 0
            && !self.immediate
            && self.blocked_location_returned == 0
            && self.native_returned == 0
            && self.continuation.va == LEADER_PRODUCE_BUILDING_SUCCESSFUL_SITE_VA
            && self.continuation.bytes_remaining
                == LEADER_PRODUCE_BUILDING_SUCCESSFUL_SITE_BYTES_REMAINING
            && self.continuation.owner == self.input.continuation.owner
            && self.continuation.type_index == self.input.continuation.type_index
            && self.continuation.origin_build_object == self.input.entry.input.origin_build_object
            && self.continuation.circle_offset == self.input.entry.input.circle_offset
            && self.continuation.candidate_world_cell == self.input.entry.input.candidate_world_cell
            && self.continuation.placement_coord == self.input.continuation.placement_coord
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildTypeBlockedTcoordPrefixStatus {
    ReturnedToBlockedSite,
    ReadyForLandDataGetAmount,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildTypeBlockedTcoordPrefixReceipt {
    pub input: BuildTypeBlockedTcoordBoundary,
    pub status: BuildTypeBlockedTcoordPrefixStatus,
    pub was_seen: Option<bool>,
    pub world_region: Option<i16>,
    pub terrain_mask: Option<u16>,
    /// Raw `blocked_tcoord` result. `blocked_site` still owns its aggregation unless this is
    /// the immediate off-map code `0x22`.
    pub raw_returned_to_blocked_site: Option<i32>,
    pub continuation: Option<LandDataGetAmountBoundary>,
}

impl BuildTypeBlockedTcoordPrefixReceipt {
    pub fn validates(&self) -> bool {
        match self.status {
            BuildTypeBlockedTcoordPrefixStatus::ReturnedToBlockedSite => {
                self.raw_returned_to_blocked_site.is_some() && self.continuation.is_none()
            }
            BuildTypeBlockedTcoordPrefixStatus::ReadyForLandDataGetAmount => {
                self.raw_returned_to_blocked_site.is_none()
                    && self.continuation.is_some_and(|next| {
                        next.va == BUILD_TYPE_BLOCKED_TCOORD_GET_AMOUNT_CALL_VA
                            && next.callee_va == LAND_DATA_GET_AMOUNT_VA
                            && next.blocked_tcoord_bytes_remaining
                                == BUILD_TYPE_BLOCKED_TCOORD_BYTES_REMAINING
                            && next.tile == self.input.tile
                            && next.owner == self.input.owner
                            && next.city_constraint == self.input.city_constraint
                            && (!next.was_seen || next.seen_or_immediate)
                            && next.good == build_type_good(next.type_index)
                    })
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuildTypeBlockedTcoordPrefixError {
    WrongBoundary {
        va: u32,
        callee_va: u32,
        bytes_remaining: u32,
    },
    InvalidOwner {
        owner: usize,
    },
    InvalidType {
        type_index: i32,
    },
    TargetTypeMismatch {
        type_index: i32,
    },
    MissingTargetFacts,
    UnsupportedCityConstraint {
        city_constraint: i32,
    },
    UnsupportedDockProfile,
    UnsupportedNonLandDomain {
        domain: i32,
    },
    UnsupportedOilProfile,
    UnsupportedPlacedBuildingLookup {
        terrain_mask: u16,
    },
    UnsupportedCityFortTerritory,
    UnsupportedTerrainState {
        terrain_mask: u16,
        build_flags: u32,
    },
    UnsupportedOwnedTerritorySeenShortcut {
        territory_owner: i8,
        region: i16,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeaderProduceBuildingBlockedSiteFootprintError {
    InvalidEntryReceipt,
    Prefix(BuildTypeBlockedTcoordPrefixError),
    Land(GatherTerrainMaterializationError),
    UnsupportedChildWithoutLandContinuation { tile: [i32; 2], raw: i32 },
    UnsupportedNonZeroChild { tile: [i32; 2], raw: i32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeaderProduceBuildingBlockedSiteFarmUnownedError {
    InvalidFootprintReceipt,
    InvalidOwner {
        owner: usize,
    },
    InvalidFarmProfile,
    ImmediateSemaphore,
    InvalidTile {
        tile: [i32; 2],
    },
    TerrainReceiptMismatch {
        tile: [i32; 2],
        receipt_mask: u16,
        actual_mask: u16,
    },
    UnsupportedWaterTile {
        tile: [i32; 2],
        terrain_mask: u16,
    },
    UnsupportedTerritoryOwner {
        tile: [i32; 2],
        territory_owner: i8,
    },
    UnsupportedLakotaTerritoryBypass,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeaderProduceBuildingBlockedSiteFarmOwnedError {
    InvalidFootprintReceipt,
    InvalidOwner {
        owner: usize,
    },
    InvalidFarmProfile,
    ImmediateSemaphore,
    InvalidTile {
        tile: [i32; 2],
    },
    TerrainReceiptMismatch {
        tile: [i32; 2],
        receipt_mask: u16,
        actual_mask: u16,
    },
    UnsupportedWaterTile {
        tile: [i32; 2],
        terrain_mask: u16,
    },
    UnsupportedTerritoryOwner {
        tile: [i32; 2],
        territory_owner: i8,
    },
    MissingCityMask {
        tile: [i32; 2],
        terrain_mask: u16,
    },
    InvalidCityMark {
        owner: usize,
        mark: i32,
        slots: usize,
    },
    MissingTown,
    InvalidCityCenter {
        city_slot: usize,
        object_index: i16,
    },
    InvalidCityChain {
        object_index: i16,
    },
    UnsupportedFarmCapacity {
        counted: i32,
        lower_bound: i32,
    },
}

#[inline]
fn placement_axis(cell: i32, size: i32) -> i32 {
    let base = cell
        .wrapping_mul(4 * super::map_terrain::COORD_PER_TILE)
        .wrapping_add(size.wrapping_mul(super::production::COORD_HALF_TILE));
    if size < 4 {
        base.wrapping_add(
            4i32.wrapping_sub(size)
                .wrapping_mul(super::production::COORD_PER_TILE)
                / 2,
        )
    } else {
        base
    }
}

/// Reproduce the exact negative-City-constraint entry path and stop before the first
/// `blocked_tcoord` call.
pub fn apply_sim_leader_produce_building_blocked_site_prefix(
    production: &LiveProductionRuntime,
    types: &TypeBuiltinState,
    input: LeaderProduceBuildingBlockedSiteBoundary,
) -> Result<
    LeaderProduceBuildingBlockedSitePrefixReceipt,
    LeaderProduceBuildingBlockedSitePrefixError,
> {
    if input.va != LEADER_PRODUCE_BUILDING_BLOCKED_SITE_CALL_VA
        || input.callee_va != BUILD_TYPE_BLOCKED_SITE_VA
        || input.bytes_remaining != LEADER_PRODUCE_BUILDING_BLOCKED_SITE_BYTES_REMAINING
    {
        return Err(LeaderProduceBuildingBlockedSitePrefixError::WrongBoundary {
            va: input.va,
            callee_va: input.callee_va,
            bytes_remaining: input.bytes_remaining,
        });
    }
    let owner = input.owner as usize;
    if owner >= NUM_LEADERS {
        return Err(LeaderProduceBuildingBlockedSitePrefixError::InvalidOwner { owner });
    }
    let type_index = usize::try_from(input.type_index).map_err(|_| {
        LeaderProduceBuildingBlockedSitePrefixError::InvalidType {
            type_index: input.type_index,
        }
    })?;
    let Some(target) = production.types.get(type_index).and_then(Option::as_ref) else {
        return Err(LeaderProduceBuildingBlockedSitePrefixError::InvalidType {
            type_index: input.type_index,
        });
    };
    let Some(target_row) = types.types.rows().get(type_index) else {
        return Err(LeaderProduceBuildingBlockedSitePrefixError::InvalidType {
            type_index: input.type_index,
        });
    };
    if target.type_index != input.type_index
        || target.class != LiveTypeClass::Building
        || target_row.index != input.type_index
        || target_row.domain() != TypeDomain::Build
    {
        return Err(
            LeaderProduceBuildingBlockedSitePrefixError::TargetTypeMismatch {
                type_index: input.type_index,
            },
        );
    }
    let visibility = target
        .build_visibility
        .ok_or(LeaderProduceBuildingBlockedSitePrefixError::MissingTargetFootprint)?;
    let footprint = visibility
        .footprint
        .ok_or(LeaderProduceBuildingBlockedSitePrefixError::MissingTargetFootprint)?;
    if footprint.x_size <= 0 || footprint.y_size <= 0 {
        return Err(
            LeaderProduceBuildingBlockedSitePrefixError::InvalidTargetFootprint { footprint },
        );
    }
    let domain = visibility
        .domain
        .ok_or(LeaderProduceBuildingBlockedSitePrefixError::MissingTargetDomain)?;
    if input.target_footprint != footprint {
        return Err(LeaderProduceBuildingBlockedSitePrefixError::FootprintMismatch);
    }
    if input.target_domain != domain {
        return Err(LeaderProduceBuildingBlockedSitePrefixError::DomainMismatch);
    }
    let expected_placement = [
        placement_axis(input.candidate_world_cell[0], footprint.x_size),
        placement_axis(input.candidate_world_cell[1], footprint.y_size),
    ];
    if input.placement_coord != expected_placement {
        return Err(LeaderProduceBuildingBlockedSitePrefixError::CandidateCoordinateMismatch);
    }
    if input.city_constraint != -1 {
        return Err(
            LeaderProduceBuildingBlockedSitePrefixError::UnsupportedCityConstraint {
                city_constraint: input.city_constraint,
            },
        );
    }
    if input.blocked_detail_initial != 0 {
        return Err(
            LeaderProduceBuildingBlockedSitePrefixError::BlockedDetailMismatch {
                blocked_detail_initial: input.blocked_detail_initial,
            },
        );
    }

    let placement_tcoord = [
        TCoord::from_coord(Coord(input.placement_coord[0])).0,
        TCoord::from_coord(Coord(input.placement_coord[1])).0,
    ];
    let footprint_corner = [
        placement_tcoord[0].wrapping_sub(footprint.x_size >> 1),
        placement_tcoord[1].wrapping_sub(footprint.y_size >> 1),
    ];
    let target_is_city = target_row
        .is_list
        .iter()
        .any(|&related| usize::from(related) == CITY_TYPE);
    let continuation = BuildTypeBlockedTcoordBoundary {
        va: BUILD_TYPE_BLOCKED_SITE_BLOCKED_TCOORD_CALL_VA,
        callee_va: BUILD_TYPE_BLOCKED_TCOORD_VA,
        blocked_site_bytes_remaining: BUILD_TYPE_BLOCKED_SITE_BYTES_REMAINING,
        owner: input.owner,
        type_index: input.type_index,
        candidate_world_cell: input.candidate_world_cell,
        placement_coord: input.placement_coord,
        footprint_corner,
        tile: footprint_corner,
        city_constraint: input.city_constraint,
        blocked_detail_initial: input.blocked_detail_initial,
    };
    let receipt = LeaderProduceBuildingBlockedSitePrefixReceipt {
        input,
        target_footprint: footprint,
        target_is_city,
        placement_tcoord,
        footprint_corner,
        native_returned: None,
        continuation,
    };
    debug_assert!(receipt.validates());
    Ok(receipt)
}

#[inline]
fn raw_for_seen_or_immediate(seen_or_immediate: bool, seen_code: i32) -> i32 {
    if seen_or_immediate {
        seen_code
    } else {
        0x24
    }
}

/// Exact `BuildTypeData::get_good` (`0x0063BD50`) switch on the concrete Type index.
#[inline]
const fn build_type_good(type_index: i32) -> i32 {
    match type_index {
        417 => 0,
        418 => 1,
        420 => 3,
        419 => 4,
        421 | 422 => 5,
        _ => -1,
    }
}

/// Exact positive-mode classifier in the TCoord overload of `WorldData::get_land`
/// (`0x006B4C70`). Unlike the WCoord overload, the mountain answer comes from the addressed
/// TData blocker kind and mixed coast cells distinguish their water and ground tiles.
#[inline]
const fn land_at_tcoord_mode_one(world_flags: u16, land: i8, terrain_mask: u16) -> i32 {
    if world_flags & wflag::COAST != 0 {
        if terrain_mask & tflag::SURFACE_MASK == tflag::SURFACE_WATER {
            2
        } else {
            0
        }
    } else if world_flags & wflag::FOREST != 0 {
        4
    } else if terrain_mask & tflag::BLOCKER_MASK == tflag::BLOCKER_MOUNTAIN {
        5
    } else if world_flags & wflag::OIL != 0 {
        7
    } else if world_flags & wflag::ROCKS != 0 {
        6
    } else {
        land as i32
    }
}

/// Follow the exact ordinary land path inside the first `blocked_tcoord` call. Deterministic
/// terrain refusals return a raw child verdict to `blocked_site`; the installed Farm advances
/// through the exact `get_good` switch and canonical TCoord `WorldData::get_land`, then stops
/// before the first still-unowned resource query, `LandData::get_amount`.
pub fn apply_sim_build_type_blocked_tcoord_land_prefix(
    sim: &Sim,
    production: &LiveProductionRuntime,
    types: &TypeBuiltinState,
    input: BuildTypeBlockedTcoordBoundary,
) -> Result<BuildTypeBlockedTcoordPrefixReceipt, BuildTypeBlockedTcoordPrefixError> {
    if input.va != BUILD_TYPE_BLOCKED_SITE_BLOCKED_TCOORD_CALL_VA
        || input.callee_va != BUILD_TYPE_BLOCKED_TCOORD_VA
        || input.blocked_site_bytes_remaining != BUILD_TYPE_BLOCKED_SITE_BYTES_REMAINING
    {
        return Err(BuildTypeBlockedTcoordPrefixError::WrongBoundary {
            va: input.va,
            callee_va: input.callee_va,
            bytes_remaining: input.blocked_site_bytes_remaining,
        });
    }
    let owner = input.owner as usize;
    if owner >= NUM_LEADERS {
        return Err(BuildTypeBlockedTcoordPrefixError::InvalidOwner { owner });
    }
    if input.city_constraint != -1 {
        return Err(
            BuildTypeBlockedTcoordPrefixError::UnsupportedCityConstraint {
                city_constraint: input.city_constraint,
            },
        );
    }
    let type_index = usize::try_from(input.type_index).map_err(|_| {
        BuildTypeBlockedTcoordPrefixError::InvalidType {
            type_index: input.type_index,
        }
    })?;
    let Some(target) = production.types.get(type_index).and_then(Option::as_ref) else {
        return Err(BuildTypeBlockedTcoordPrefixError::InvalidType {
            type_index: input.type_index,
        });
    };
    let Some(target_row) = types.types.rows().get(type_index) else {
        return Err(BuildTypeBlockedTcoordPrefixError::InvalidType {
            type_index: input.type_index,
        });
    };
    if target.type_index != input.type_index
        || target.class != LiveTypeClass::Building
        || target_row.index != input.type_index
        || target_row.domain() != TypeDomain::Build
    {
        return Err(BuildTypeBlockedTcoordPrefixError::TargetTypeMismatch {
            type_index: input.type_index,
        });
    }
    let Some(visibility) = target.build_visibility else {
        return Err(BuildTypeBlockedTcoordPrefixError::MissingTargetFacts);
    };
    let Some(domain) = visibility.domain else {
        return Err(BuildTypeBlockedTcoordPrefixError::MissingTargetFacts);
    };
    let type_is = |query: usize| {
        target_row
            .is_list
            .iter()
            .any(|&related| usize::from(related) == query)
    };

    let returned = |raw_returned_to_blocked_site, was_seen, world_region, terrain_mask| {
        let receipt = BuildTypeBlockedTcoordPrefixReceipt {
            input,
            status: BuildTypeBlockedTcoordPrefixStatus::ReturnedToBlockedSite,
            was_seen,
            world_region,
            terrain_mask,
            raw_returned_to_blocked_site: Some(raw_returned_to_blocked_site),
            continuation: None,
        };
        debug_assert!(receipt.validates());
        receipt
    };

    let [tx, ty] = input.tile;
    if !sim.map.world.valid_t(tx, ty) {
        return Ok(returned(0x22, None, None, None));
    }
    let world = &sim.map.world;
    let world_cell = world.wdata(tx >> 2, ty >> 2);
    let fog = &sim.map.fog;
    let fog_leader = &fog.leaders[owner];
    let was_seen = if fog.option.0 > 1
        || fog_leader.explored_all
        || fog_leader.see_all
        || fog_leader.reveal_counter != 0
    {
        true
    } else if world_cell.who >= 0 {
        // `WorldData::was_seen` first asks whether the WData owner is allied, then treats either
        // a City or Fort in this region as explored. Sim does not yet own general diplomacy or
        // the Fort region counts. The reached self-owned arm is nevertheless exact: a Leader is
        // allied with itself, and the canonical CityPool supplies the corresponding region-City
        // witness. Foreign owners and self-owned regions without that witness remain fail-closed.
        let mark = sim.cities.city_mark[owner];
        let self_owned_city_region = types.leaders[owner].leader_flags & 1 != 0
            && world_cell.who == owner as i8
            && usize::try_from(mark)
                .ok()
                .filter(|&mark| mark <= sim.cities.slots[owner].len())
                .is_some_and(|mark| {
                    sim.cities.slots[owner][..mark].iter().any(|city| {
                        city.active() && city.who == owner as i8 && city.reg == world_cell.region
                    })
                });
        if self_owned_city_region {
            true
        } else {
            return Err(
                BuildTypeBlockedTcoordPrefixError::UnsupportedOwnedTerritorySeenShortcut {
                    territory_owner: world_cell.who,
                    region: world_cell.region,
                },
            );
        }
    } else {
        world.was_seen(tx >> 1, ty >> 1, fog_leader.player_mask)
    };
    let immediate = sim.vic_match.semaphore & (1 << GAME_SEMAPHORE_IMMEDIATE_BIT) != 0;
    let seen_for_verdict = was_seen || immediate;
    if world_cell.region < 0 {
        return Ok(returned(
            raw_for_seen_or_immediate(seen_for_verdict, 4),
            Some(was_seen),
            Some(world_cell.region),
            None,
        ));
    }
    let terrain_mask = world.tmask(tx, ty);
    if terrain_mask & tflag::BLOCKER_MASK == tflag::BLOCKER_BUILDING {
        return Ok(returned(
            raw_for_seen_or_immediate(seen_for_verdict, 1),
            Some(was_seen),
            Some(world_cell.region),
            Some(terrain_mask),
        ));
    }
    if type_is(DOCK_TYPE) {
        return Err(BuildTypeBlockedTcoordPrefixError::UnsupportedDockProfile);
    }
    if domain != 0 {
        return Err(BuildTypeBlockedTcoordPrefixError::UnsupportedNonLandDomain { domain });
    }
    if terrain_mask & tflag::SURFACE_MASK == tflag::SURFACE_WATER {
        return Ok(returned(
            raw_for_seen_or_immediate(seen_for_verdict, 0x0e),
            Some(was_seen),
            Some(world_cell.region),
            Some(terrain_mask),
        ));
    }
    if terrain_mask & 0x4000 != 0 || terrain_mask & 0x0200 != 0 {
        return Err(BuildTypeBlockedTcoordPrefixError::UnsupportedTerrainState {
            terrain_mask,
            build_flags: target.build_flags,
        });
    }
    if type_is(OIL_WELL_TYPE) || type_is(OIL_PLATFORM_TYPE) {
        return Err(BuildTypeBlockedTcoordPrefixError::UnsupportedOilProfile);
    }
    if terrain_mask & 0x0080 != 0 {
        return Err(
            BuildTypeBlockedTcoordPrefixError::UnsupportedPlacedBuildingLookup { terrain_mask },
        );
    }
    if target.build_flags & 0x10 == 0 && terrain_mask & tflag::CITY == 0 {
        return Ok(returned(
            raw_for_seen_or_immediate(seen_for_verdict, 0x1f),
            Some(was_seen),
            Some(world_cell.region),
            Some(terrain_mask),
        ));
    }
    if type_is(CITY_TYPE) || type_is(FORT_TYPE) {
        return Err(BuildTypeBlockedTcoordPrefixError::UnsupportedCityFortTerritory);
    }
    if target.build_flags & 0x400 != 0 {
        return Ok(returned(
            raw_for_seen_or_immediate(seen_for_verdict, 0x2b),
            Some(was_seen),
            Some(world_cell.region),
            Some(terrain_mask),
        ));
    }
    if terrain_mask & 0x0800 != 0 {
        return Ok(returned(
            0x24 + 8 * i32::from(seen_for_verdict),
            Some(was_seen),
            Some(world_cell.region),
            Some(terrain_mask),
        ));
    }
    if target.build_flags & 0x40 == 0 || target.build_flags & 0x1000_0000 == 0 {
        return Ok(returned(
            0,
            Some(was_seen),
            Some(world_cell.region),
            Some(terrain_mask),
        ));
    }

    let good = build_type_good(input.type_index);
    let land_index = land_at_tcoord_mode_one(world_cell.flags, world_cell.land, terrain_mask);
    let tile_linear_index = ty.wrapping_mul(world.tile_xs).wrapping_add(tx);
    let continuation = LandDataGetAmountBoundary {
        va: BUILD_TYPE_BLOCKED_TCOORD_GET_AMOUNT_CALL_VA,
        callee_va: LAND_DATA_GET_AMOUNT_VA,
        blocked_tcoord_bytes_remaining: BUILD_TYPE_BLOCKED_TCOORD_BYTES_REMAINING,
        owner: input.owner,
        type_index: input.type_index,
        tile: input.tile,
        city_constraint: input.city_constraint,
        was_seen,
        seen_or_immediate: seen_for_verdict,
        world_region: world_cell.region,
        terrain_mask,
        build_flags: target.build_flags,
        good,
        land_index,
        tile_linear_index,
    };
    let receipt = BuildTypeBlockedTcoordPrefixReceipt {
        input,
        status: BuildTypeBlockedTcoordPrefixStatus::ReadyForLandDataGetAmount,
        was_seen: Some(was_seen),
        world_region: Some(world_cell.region),
        terrain_mask: Some(terrain_mask),
        raw_returned_to_blocked_site: None,
        continuation: Some(continuation),
    };
    debug_assert!(receipt.validates());
    Ok(receipt)
}

/// Complete the 45-byte `LandData::get_amount` call reached by the ordinary-land path.
pub fn apply_sim_land_data_get_amount(
    sim: &Sim,
    terrain: &GatherTerrainMaterialization,
    input: LandDataGetAmountBoundary,
) -> Result<LandDataGetAmountReceipt, GatherTerrainMaterializationError> {
    let index = usize::try_from(input.land_index).map_err(|_| {
        GatherTerrainMaterializationError::InvalidLandIndex {
            index: input.land_index,
        }
    })?;
    let land =
        terrain
            .lands()
            .get(index)
            .ok_or(GatherTerrainMaterializationError::InvalidLandIndex {
                index: input.land_index,
            })?;
    let returned = terrain.land_amount(
        &sim.map.world,
        input.land_index,
        input.good,
        input.tile_linear_index,
    )?;
    Ok(LandDataGetAmountReceipt {
        input,
        land_name: land.name.clone(),
        land_gather: land.gather,
        returned,
    })
}

/// Execute the installed ordinary-land cohort across every footprint tile and stop at the
/// first still-unowned parent call, `BuildTypeData::blocked_location`. Nonzero child results
/// deliberately remain fail-closed because retail's precedence filter is a separate cone.
pub fn apply_sim_leader_produce_building_blocked_site_land_footprint(
    sim: &Sim,
    production: &LiveProductionRuntime,
    types: &TypeBuiltinState,
    terrain: &GatherTerrainMaterialization,
    entry: LeaderProduceBuildingBlockedSitePrefixReceipt,
) -> Result<
    LeaderProduceBuildingBlockedSiteFootprintReceipt,
    LeaderProduceBuildingBlockedSiteFootprintError,
> {
    if !entry.validates() || entry.native_returned.is_some() {
        return Err(LeaderProduceBuildingBlockedSiteFootprintError::InvalidEntryReceipt);
    }
    let [corner_x, corner_y] = entry.footprint_corner;
    let end_x = corner_x
        .checked_add(entry.target_footprint.x_size)
        .ok_or(LeaderProduceBuildingBlockedSiteFootprintError::InvalidEntryReceipt)?;
    let end_y = corner_y
        .checked_add(entry.target_footprint.y_size)
        .ok_or(LeaderProduceBuildingBlockedSiteFootprintError::InvalidEntryReceipt)?;
    let mut tiles = Vec::new();
    for tx in corner_x..end_x {
        for ty in corner_y..end_y {
            let mut boundary = entry.continuation;
            boundary.tile = [tx, ty];
            let prefix =
                apply_sim_build_type_blocked_tcoord_land_prefix(sim, production, types, boundary)
                    .map_err(LeaderProduceBuildingBlockedSiteFootprintError::Prefix)?;
            let Some(amount_boundary) = prefix.continuation else {
                return Err(
                    LeaderProduceBuildingBlockedSiteFootprintError::UnsupportedChildWithoutLandContinuation {
                        tile: [tx, ty],
                        raw: prefix.raw_returned_to_blocked_site.unwrap_or_default(),
                    },
                );
            };
            let amount = apply_sim_land_data_get_amount(sim, terrain, amount_boundary)
                .map_err(LeaderProduceBuildingBlockedSiteFootprintError::Land)?;
            let raw = if amount.returned != 0 {
                0
            } else {
                raw_for_seen_or_immediate(amount.input.seen_or_immediate, 9)
            };
            if raw != 0 {
                return Err(
                    LeaderProduceBuildingBlockedSiteFootprintError::UnsupportedNonZeroChild {
                        tile: [tx, ty],
                        raw,
                    },
                );
            }
            tiles.push(BuildTypeBlockedTcoordLandReceipt {
                prefix,
                amount,
                raw_returned_to_blocked_site: raw,
            });
        }
    }
    let continuation = BuildTypeBlockedLocationBoundary {
        va: BUILD_TYPE_BLOCKED_SITE_BLOCKED_LOCATION_CALL_VA,
        callee_va: BUILD_TYPE_BLOCKED_LOCATION_VA,
        blocked_site_bytes_remaining: BUILD_TYPE_BLOCKED_SITE_BLOCKED_LOCATION_BYTES_REMAINING,
        owner: entry.input.owner,
        type_index: entry.input.type_index,
        placement_coord: entry.input.placement_coord,
        footprint_corner: entry.footprint_corner,
        city_constraint: entry.input.city_constraint,
        blocked_detail: 0,
    };
    Ok(LeaderProduceBuildingBlockedSiteFootprintReceipt {
        entry,
        tiles,
        blocked_detail: 0,
        // The reached caller's city constraint is -1, so retail skips its locally-seen count.
        locally_seen_tiles: 0,
        continuation,
    })
}

/// Execute the complete reached installed-Farm `blocked_location` verdict and its parent
/// `blocked_site` return filter.
///
/// This is intentionally a narrow terminal cone. The synchronized fixture reaches only dry,
/// unowned WData and a non-Lakota owner. Retail returns `0x1a` immediately from that territory
/// arm, so Town lookup, per-City Farm capacity, water counting, and `calc_gather` are not read.
/// Owned/allied/enemy territory and the immediate-game override remain separate fail-closed
/// branches rather than being inferred from City tile masks.
pub fn apply_sim_leader_produce_building_blocked_site_farm_unowned_tail(
    sim: &Sim,
    production: &LiveProductionRuntime,
    types: &TypeBuiltinState,
    input: LeaderProduceBuildingBlockedSiteFootprintReceipt,
) -> Result<
    LeaderProduceBuildingBlockedSiteFarmUnownedReceipt,
    LeaderProduceBuildingBlockedSiteFarmUnownedError,
> {
    if !input.entry.validates()
        || input.entry.native_returned.is_some()
        || input.blocked_detail != 0
        || input.locally_seen_tiles != 0
        || input.continuation.va != BUILD_TYPE_BLOCKED_SITE_BLOCKED_LOCATION_CALL_VA
        || input.continuation.callee_va != BUILD_TYPE_BLOCKED_LOCATION_VA
        || input.continuation.blocked_site_bytes_remaining
            != BUILD_TYPE_BLOCKED_SITE_BLOCKED_LOCATION_BYTES_REMAINING
        || input.continuation.owner != input.entry.input.owner
        || input.continuation.type_index != input.entry.input.type_index
        || input.continuation.placement_coord != input.entry.input.placement_coord
        || input.continuation.footprint_corner != input.entry.footprint_corner
        || input.continuation.city_constraint != -1
        || input.continuation.blocked_detail != 0
    {
        return Err(LeaderProduceBuildingBlockedSiteFarmUnownedError::InvalidFootprintReceipt);
    }
    let owner = input.continuation.owner as usize;
    if owner >= NUM_LEADERS {
        return Err(LeaderProduceBuildingBlockedSiteFarmUnownedError::InvalidOwner { owner });
    }
    let farm_index = FARM_TYPE;
    let Some(target) = production.types.get(farm_index).and_then(Option::as_ref) else {
        return Err(LeaderProduceBuildingBlockedSiteFarmUnownedError::InvalidFarmProfile);
    };
    let Some(target_row) = types.types.rows().get(farm_index) else {
        return Err(LeaderProduceBuildingBlockedSiteFarmUnownedError::InvalidFarmProfile);
    };
    let visibility = target.build_visibility;
    let footprint = visibility.and_then(|facts| facts.footprint);
    let domain = visibility.and_then(|facts| facts.domain);
    let is_related = |query: usize| {
        target_row
            .is_list
            .iter()
            .any(|&related| usize::from(related) == query)
    };
    if input.continuation.type_index != FARM_TYPE as i32
        || target.type_index != FARM_TYPE as i32
        || target.class != LiveTypeClass::Building
        || target_row.index != FARM_TYPE as i32
        || target_row.domain() != TypeDomain::Build
        || target.build_flags != 0x1000_0049
        || domain != Some(0)
        || footprint != Some(input.entry.target_footprint)
        || input.entry.target_footprint
            != (Footprint {
                x_size: 4,
                y_size: 4,
            })
        || input.tiles.len() != 16
        || is_related(CITY_TYPE)
        || is_related(FORT_TYPE)
        || is_related(DOCK_TYPE)
        || input.tiles.iter().any(|tile| {
            !tile.prefix.validates()
                || tile.prefix.continuation != Some(tile.amount.input)
                || tile.raw_returned_to_blocked_site != 0
                || tile.amount.returned == 0
                || tile.prefix.input.tile != tile.amount.input.tile
        })
    {
        return Err(LeaderProduceBuildingBlockedSiteFarmUnownedError::InvalidFarmProfile);
    }

    let immediate = sim.vic_match.semaphore & (1 << GAME_SEMAPHORE_IMMEDIATE_BIT) != 0;
    if immediate {
        return Err(LeaderProduceBuildingBlockedSiteFarmUnownedError::ImmediateSemaphore);
    }

    let [corner_x, corner_y] = input.entry.footprint_corner;
    let expected_tiles =
        (corner_x..corner_x + 4).flat_map(|tx| (corner_y..corner_y + 4).map(move |ty| [tx, ty]));
    let mut territory_reads = Vec::with_capacity(input.tiles.len());
    for (tile, expected) in input.tiles.iter().zip(expected_tiles) {
        let reached = tile.prefix.input.tile;
        if reached != expected || !sim.map.world.valid_t(reached[0], reached[1]) {
            return Err(
                LeaderProduceBuildingBlockedSiteFarmUnownedError::InvalidTile { tile: reached },
            );
        }
        let terrain_mask = sim.map.world.tmask(reached[0], reached[1]);
        if tile.amount.input.terrain_mask != terrain_mask {
            return Err(
                LeaderProduceBuildingBlockedSiteFarmUnownedError::TerrainReceiptMismatch {
                    tile: reached,
                    receipt_mask: tile.amount.input.terrain_mask,
                    actual_mask: terrain_mask,
                },
            );
        }
        if terrain_mask & tflag::SURFACE_MASK == tflag::SURFACE_WATER {
            return Err(
                LeaderProduceBuildingBlockedSiteFarmUnownedError::UnsupportedWaterTile {
                    tile: reached,
                    terrain_mask,
                },
            );
        }
        let territory_owner = sim.map.world.wdata(reached[0] >> 2, reached[1] >> 2).who;
        if territory_owner != -1 {
            return Err(
                LeaderProduceBuildingBlockedSiteFarmUnownedError::UnsupportedTerritoryOwner {
                    tile: reached,
                    territory_owner,
                },
            );
        }
        territory_reads.push(BuildTypeNonFriendlyTerritoryRead {
            tile: reached,
            terrain_mask,
            territory_owner,
        });
    }

    // `LeaderData::has_tribe_bonus(0x13)`: Lakota may build in unowned territory and therefore
    // continues to the still-separate Town/capacity tail instead of returning `0x1a` here.
    let lakota_bonus = types.leaders[owner].tribe == LAKOTA_TRIBE;
    if lakota_bonus {
        return Err(
            LeaderProduceBuildingBlockedSiteFarmUnownedError::UnsupportedLakotaTerritoryBypass,
        );
    }

    let receipt = LeaderProduceBuildingBlockedSiteFarmUnownedReceipt {
        input,
        territory_reads,
        non_friendly_territory: 1,
        lakota_bonus,
        blocked_location_returned: 0x1a,
        immediate,
        // With the immediate semaphore clear, 0x00636D5F returns the child code unchanged.
        native_returned: 0x1a,
    };
    debug_assert!(receipt.validates());
    Ok(receipt)
}

fn farm_owned_build_row(
    sim: &Sim,
    owner: usize,
    object_index: i16,
) -> Result<usize, LeaderProduceBuildingBlockedSiteFarmOwnedError> {
    let slot = i32::from(object_index)
        .checked_sub(BUILD_BAND_BASE as i32)
        .and_then(|slot| usize::try_from(slot).ok())
        .ok_or(LeaderProduceBuildingBlockedSiteFarmOwnedError::InvalidCityChain { object_index })?;
    let row = sim
        .world
        .objects
        .slot(owner)
        .band(Band::Build)
        .get(slot)
        .copied()
        .ok_or(LeaderProduceBuildingBlockedSiteFarmOwnedError::InvalidCityChain { object_index })?
        as usize;
    let build = sim
        .builds
        .get(row)
        .ok_or(LeaderProduceBuildingBlockedSiteFarmOwnedError::InvalidCityChain { object_index })?;
    if build.who as usize != owner || build.object_id() != object_index {
        return Err(
            LeaderProduceBuildingBlockedSiteFarmOwnedError::InvalidCityChain { object_index },
        );
    }
    Ok(row)
}

/// Execute the exact installed-Farm friendly-territory continuation through the terminal zero
/// `blocked_location` / `blocked_site` verdict.
pub fn apply_sim_leader_produce_building_blocked_site_farm_owned_tail(
    sim: &Sim,
    production: &LiveProductionRuntime,
    types: &TypeBuiltinState,
    input: LeaderProduceBuildingBlockedSiteFootprintReceipt,
) -> Result<
    LeaderProduceBuildingBlockedSiteFarmOwnedReceipt,
    LeaderProduceBuildingBlockedSiteFarmOwnedError,
> {
    if !input.entry.validates()
        || input.entry.native_returned.is_some()
        || input.blocked_detail != 0
        || input.locally_seen_tiles != 0
        || input.continuation.va != BUILD_TYPE_BLOCKED_SITE_BLOCKED_LOCATION_CALL_VA
        || input.continuation.callee_va != BUILD_TYPE_BLOCKED_LOCATION_VA
        || input.continuation.blocked_site_bytes_remaining
            != BUILD_TYPE_BLOCKED_SITE_BLOCKED_LOCATION_BYTES_REMAINING
        || input.continuation.owner != input.entry.input.owner
        || input.continuation.type_index != input.entry.input.type_index
        || input.continuation.placement_coord != input.entry.input.placement_coord
        || input.continuation.footprint_corner != input.entry.footprint_corner
        || input.continuation.city_constraint != -1
        || input.continuation.blocked_detail != input.blocked_detail
    {
        return Err(LeaderProduceBuildingBlockedSiteFarmOwnedError::InvalidFootprintReceipt);
    }
    let owner = input.continuation.owner as usize;
    if owner >= NUM_LEADERS {
        return Err(LeaderProduceBuildingBlockedSiteFarmOwnedError::InvalidOwner { owner });
    }
    let Some(target) = production.types.get(FARM_TYPE).and_then(Option::as_ref) else {
        return Err(LeaderProduceBuildingBlockedSiteFarmOwnedError::InvalidFarmProfile);
    };
    let Some(target_row) = types.types.rows().get(FARM_TYPE) else {
        return Err(LeaderProduceBuildingBlockedSiteFarmOwnedError::InvalidFarmProfile);
    };
    let visibility = target.build_visibility;
    let is_related = |query: usize| {
        target_row
            .is_list
            .iter()
            .any(|&related| usize::from(related) == query)
    };
    if input.continuation.type_index != FARM_TYPE as i32
        || target.type_index != FARM_TYPE as i32
        || target.class != LiveTypeClass::Building
        || target_row.index != FARM_TYPE as i32
        || target_row.domain() != TypeDomain::Build
        || types.leaders[owner].leader_flags & 1 == 0
        || target.build_flags != 0x1000_0049
        || visibility.and_then(|facts| facts.domain) != Some(0)
        || visibility.and_then(|facts| facts.footprint) != Some(input.entry.target_footprint)
        || input.entry.target_footprint
            != (Footprint {
                x_size: 4,
                y_size: 4,
            })
        || input.tiles.len() != 16
        || is_related(CITY_TYPE)
        || is_related(FORT_TYPE)
        || is_related(DOCK_TYPE)
        || input.tiles.iter().any(|tile| {
            !tile.prefix.validates()
                || tile.prefix.continuation != Some(tile.amount.input)
                || tile.prefix.was_seen != Some(true)
                || !tile.amount.input.was_seen
                || tile.raw_returned_to_blocked_site != 0
                || tile.amount.returned == 0
                || tile.prefix.input.tile != tile.amount.input.tile
        })
    {
        return Err(LeaderProduceBuildingBlockedSiteFarmOwnedError::InvalidFarmProfile);
    }
    let immediate = sim.vic_match.semaphore & (1 << GAME_SEMAPHORE_IMMEDIATE_BIT) != 0;
    if immediate {
        return Err(LeaderProduceBuildingBlockedSiteFarmOwnedError::ImmediateSemaphore);
    }

    let [corner_x, corner_y] = input.entry.footprint_corner;
    let expected_tiles =
        (corner_x..corner_x + 4).flat_map(|tx| (corner_y..corner_y + 4).map(move |ty| [tx, ty]));
    let mut territory_reads = Vec::with_capacity(input.tiles.len());
    for (tile, expected) in input.tiles.iter().zip(expected_tiles) {
        let reached = tile.prefix.input.tile;
        if reached != expected || !sim.map.world.valid_t(reached[0], reached[1]) {
            return Err(
                LeaderProduceBuildingBlockedSiteFarmOwnedError::InvalidTile { tile: reached },
            );
        }
        let terrain_mask = sim.map.world.tmask(reached[0], reached[1]);
        if tile.amount.input.terrain_mask != terrain_mask {
            return Err(
                LeaderProduceBuildingBlockedSiteFarmOwnedError::TerrainReceiptMismatch {
                    tile: reached,
                    receipt_mask: tile.amount.input.terrain_mask,
                    actual_mask: terrain_mask,
                },
            );
        }
        if terrain_mask & tflag::SURFACE_MASK == tflag::SURFACE_WATER {
            return Err(
                LeaderProduceBuildingBlockedSiteFarmOwnedError::UnsupportedWaterTile {
                    tile: reached,
                    terrain_mask,
                },
            );
        }
        if terrain_mask & tflag::CITY == 0 {
            return Err(
                LeaderProduceBuildingBlockedSiteFarmOwnedError::MissingCityMask {
                    tile: reached,
                    terrain_mask,
                },
            );
        }
        let territory_owner = sim.map.world.wdata(reached[0] >> 2, reached[1] >> 2).who;
        if territory_owner != owner as i8 {
            return Err(
                LeaderProduceBuildingBlockedSiteFarmOwnedError::UnsupportedTerritoryOwner {
                    tile: reached,
                    territory_owner,
                },
            );
        }
        territory_reads.push(BuildTypeNonFriendlyTerritoryRead {
            tile: reached,
            terrain_mask,
            territory_owner,
        });
    }

    let mark = sim.cities.city_mark[owner];
    let mark = usize::try_from(mark).map_err(|_| {
        LeaderProduceBuildingBlockedSiteFarmOwnedError::InvalidCityMark {
            owner,
            mark,
            slots: sim.cities.slots[owner].len(),
        }
    })?;
    if mark > sim.cities.slots[owner].len() {
        return Err(
            LeaderProduceBuildingBlockedSiteFarmOwnedError::InvalidCityMark {
                owner,
                mark: mark as i32,
                slots: sim.cities.slots[owner].len(),
            },
        );
    }
    let site_tx = TCoord::from_coord(Coord(input.continuation.placement_coord[0])).0;
    let site_ty = TCoord::from_coord(Coord(input.continuation.placement_coord[1])).0;
    let indian_radius_bonus = types.leaders[owner].tribe == 0x15;
    let mut selected: Option<BuildTypeFarmTownRead> = None;
    for (city_slot, city) in sim.cities.slots[owner][..mark].iter().enumerate() {
        if !city.active() || city.who != owner as i8 {
            continue;
        }
        let center_row = farm_owned_build_row(sim, owner, city.o).map_err(|_| {
            LeaderProduceBuildingBlockedSiteFarmOwnedError::InvalidCityCenter {
                city_slot,
                object_index: city.o,
            }
        })?;
        let center = &sim.builds[center_row];
        if center.flags & flag::VALID == 0 || center.city != city_slot as i16 {
            return Err(
                LeaderProduceBuildingBlockedSiteFarmOwnedError::InvalidCityCenter {
                    city_slot,
                    object_index: city.o,
                },
            );
        }
        let center_type = production
            .build_types
            .get(center_row)
            .copied()
            .flatten()
            .ok_or(
                LeaderProduceBuildingBlockedSiteFarmOwnedError::InvalidCityCenter {
                    city_slot,
                    object_index: city.o,
                },
            )?;
        let distance = vector_dist(
            TCoord::from_coord(Coord(city.x)).0.wrapping_sub(site_tx),
            TCoord::from_coord(Coord(city.y)).0.wrapping_sub(site_ty),
        ) as i32;
        let radius = city_radius(&CityRules::RETAIL, center_type, indian_radius_bonus);
        if distance > radius
            || selected
                .as_ref()
                .is_some_and(|prior| distance > prior.distance)
        {
            continue;
        }
        selected = Some(BuildTypeFarmTownRead {
            city_slot,
            city_object: city.o,
            center_type,
            distance,
            radius,
            counted_farms: 0,
            farm_limit_lower_bound: CityRules::RETAIL.farms_per_city_base,
        });
    }
    let mut town = selected.ok_or(LeaderProduceBuildingBlockedSiteFarmOwnedError::MissingTown)?;

    let mut object_index = town.city_object;
    let mut visited = Vec::new();
    let mut counted_farms = 0;
    while object_index >= 0 {
        if visited.contains(&object_index) {
            return Err(
                LeaderProduceBuildingBlockedSiteFarmOwnedError::InvalidCityChain { object_index },
            );
        }
        visited.push(object_index);
        let row = farm_owned_build_row(sim, owner, object_index)?;
        let build = &sim.builds[row];
        if build.flags & flag::VALID != 0 {
            let build_type = production
                .build_types
                .get(row)
                .copied()
                .flatten()
                .and_then(|index| usize::try_from(index).ok())
                .filter(|&index| index < types.types.rows().len())
                .ok_or(
                    LeaderProduceBuildingBlockedSiteFarmOwnedError::InvalidCityChain {
                        object_index,
                    },
                )?;
            let farm_relation = types
                .types
                .row(build_type)
                .is_list
                .iter()
                .any(|&related| usize::from(related) == FARM_TYPE);
            if build.flags & flag::ACTIVE != 0 && farm_relation {
                counted_farms += 1;
            }
        }
        object_index = build.city_down;
    }
    town.counted_farms = counted_farms;
    if counted_farms >= town.farm_limit_lower_bound {
        return Err(
            LeaderProduceBuildingBlockedSiteFarmOwnedError::UnsupportedFarmCapacity {
                counted: counted_farms,
                lower_bound: town.farm_limit_lower_bound,
            },
        );
    }

    let continuation = LeaderProduceBuildingSuccessfulSiteBoundary {
        va: LEADER_PRODUCE_BUILDING_SUCCESSFUL_SITE_VA,
        bytes_remaining: LEADER_PRODUCE_BUILDING_SUCCESSFUL_SITE_BYTES_REMAINING,
        owner: input.continuation.owner,
        type_index: input.continuation.type_index,
        origin_build_object: input.entry.input.origin_build_object,
        circle_offset: input.entry.input.circle_offset,
        candidate_world_cell: input.entry.input.candidate_world_cell,
        placement_coord: input.continuation.placement_coord,
    };
    let receipt = LeaderProduceBuildingBlockedSiteFarmOwnedReceipt {
        input,
        territory_reads,
        non_friendly_territory: 0,
        lakota_bonus: types.leaders[owner].tribe == LAKOTA_TRIBE,
        town,
        water_tiles: 0,
        blocked_location_returned: 0,
        immediate,
        native_returned: 0,
        continuation,
    };
    debug_assert!(receipt.validates());
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_good_matches_the_retail_six_entry_jump_table() {
        assert_eq!(build_type_good(417), 0);
        assert_eq!(build_type_good(418), 1);
        assert_eq!(build_type_good(419), 4);
        assert_eq!(build_type_good(420), 3);
        assert_eq!(build_type_good(421), 5);
        assert_eq!(build_type_good(422), 5);
        assert_eq!(build_type_good(416), -1);
        assert_eq!(build_type_good(423), -1);
    }

    #[test]
    fn tcoord_land_classifier_preserves_retail_precedence() {
        assert_eq!(
            land_at_tcoord_mode_one(wflag::COAST, 8, tflag::SURFACE_WATER),
            2
        );
        assert_eq!(
            land_at_tcoord_mode_one(wflag::COAST, 8, tflag::BLOCKER_MOUNTAIN),
            0
        );
        assert_eq!(
            land_at_tcoord_mode_one(wflag::FOREST, 8, tflag::BLOCKER_MOUNTAIN),
            4
        );
        assert_eq!(
            land_at_tcoord_mode_one(wflag::OIL, 8, tflag::BLOCKER_MOUNTAIN),
            5
        );
        assert_eq!(land_at_tcoord_mode_one(wflag::OIL, 8, 0), 7);
        assert_eq!(land_at_tcoord_mode_one(wflag::OIL | wflag::ROCKS, 8, 0), 7);
        assert_eq!(land_at_tcoord_mode_one(wflag::ROCKS, 8, 0), 6);
        assert_eq!(land_at_tcoord_mode_one(0, -1, 0), -1);
    }
}
