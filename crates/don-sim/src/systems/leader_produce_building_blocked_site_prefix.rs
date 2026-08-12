//! Exact entry prefix of `BuildTypeData::blocked_site`
//! (`0x00636A50..0x00636BCC`).
//!
//! The reached `Leader::produce_building` call always supplies the literal City constraint
//! `-1`. Retail converts the candidate Coord pair to the footprint's top-left TCoord, asks the
//! canonical non-strict Type relation whether the target is a City, skips the City-only stale
//! capital scan because the constraint is negative, and begins the footprint walk in x-outer /
//! y-inner order. This tranche also follows the reached ordinary-land
//! `BuildTypeData::blocked_tcoord` path through the exact `get_good` switch and TCoord land
//! classification. It stops before `LandData::get_amount` and mutates nothing.

use crate::tick::{Sim, NUM_LEADERS};

use super::bhs_type_table::{TypeBuiltinState, TypeDomain};
use super::leader_produce_building_candidate_prefix::{
    LeaderProduceBuildingBlockedSiteBoundary, BUILD_TYPE_BLOCKED_SITE_VA,
    LEADER_PRODUCE_BUILDING_BLOCKED_SITE_BYTES_REMAINING,
    LEADER_PRODUCE_BUILDING_BLOCKED_SITE_CALL_VA,
};
use super::map_terrain::{tflag, wflag, Coord, TCoord};
use super::production::{
    runtime::{LiveProductionRuntime, LiveTypeClass},
    Footprint,
};

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
pub const CITY_TYPE: usize = 414;
pub const OIL_WELL_TYPE: usize = 421;
pub const OIL_PLATFORM_TYPE: usize = 422;
pub const DOCK_TYPE: usize = 432;
pub const FORT_TYPE: usize = 443;
pub const GAME_SEMAPHORE_IMMEDIATE_BIT: u32 = 11;

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
    UnownedSeenTerritory {
        territory_owner: i8,
        region: i16,
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
fn raw_for_seen(was_seen: bool, seen_code: i32) -> i32 {
    if was_seen {
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
        return Err(BuildTypeBlockedTcoordPrefixError::UnownedSeenTerritory {
            territory_owner: world_cell.who,
            region: world_cell.region,
        });
    } else {
        world.was_seen(tx >> 1, ty >> 1, fog_leader.player_mask)
    };
    let immediate = sim.vic_match.semaphore & (1 << GAME_SEMAPHORE_IMMEDIATE_BIT) != 0;
    let seen_for_verdict = was_seen || immediate;
    if world_cell.region < 0 {
        return Ok(returned(
            raw_for_seen(seen_for_verdict, 4),
            Some(was_seen),
            Some(world_cell.region),
            None,
        ));
    }
    let terrain_mask = world.tmask(tx, ty);
    if terrain_mask & tflag::BLOCKER_MASK == tflag::BLOCKER_BUILDING {
        return Ok(returned(
            raw_for_seen(seen_for_verdict, 1),
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
            raw_for_seen(seen_for_verdict, 0x0e),
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
            raw_for_seen(seen_for_verdict, 0x1f),
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
            raw_for_seen(seen_for_verdict, 0x2b),
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
