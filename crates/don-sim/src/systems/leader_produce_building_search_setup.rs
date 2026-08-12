//! Exact frame-zero search setup in `Leader::produce_building` (`0x006E150D..0x006E1AE1`).
//!
//! This tranche begins after the City/target-Type admission gate. Retail decodes the origin
//! Build position, reads its `WData::region`, derives `LeaderData::get_radius`, selects the
//! target footprint search box, and compares the first spiral offset with `circle_radius`.
//! The nonzero-frame policy and Oil Well's separate existing-well scan are deliberately not
//! admitted here. No candidate is tested and no gameplay state is mutated by this prefix.

use crate::objects::{Band, BUILD_BAND_BASE, WALL_BAND_BASE};
use crate::tick::{Sim, NUM_LEADERS};

use super::bhs_type_table::{TypeBuiltinState, TypeDomain};
use super::combat;
use super::leader_produce_building_prefix::{
    LeaderProduceBuildingSearchBoundary, LEADER_PRODUCE_BUILDING_CITY_GATE_CONTINUATION_VA,
};
use super::production::{
    runtime::{LiveProductionRuntime, LiveTypeClass},
    Footprint,
};
use super::tech_cities::{city_radius, CityRules};

pub const LEADER_PRODUCE_BUILDING_CANDIDATE_LOOP_VA: u32 = 0x006e_1ae1;
pub const LEADER_PRODUCE_BUILDING_SEARCH_SETUP_BYTES: u32 =
    LEADER_PRODUCE_BUILDING_CANDIDATE_LOOP_VA - LEADER_PRODUCE_BUILDING_CITY_GATE_CONTINUATION_VA;
pub const LEADER_PRODUCE_BUILDING_CANDIDATE_BYTES_REMAINING: u32 = 0x160d;

pub const FARM_TYPE: i32 = 417;
pub const MINE_TYPE: i32 = 419;
pub const UNIVERSITY_TYPE: i32 = 420;
pub const OIL_WELL_TYPE: i32 = 421;
pub const DOCK_TYPE: usize = 432;
pub const BUILD_FLAG_RESOURCE_SENSITIVE_SEARCH: u32 = 0x40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaderProduceBuildingSearchSetupStatus {
    SearchRadiusExhaustedBeforeCandidateLoop,
    ReadyForCandidateLoop,
}

/// Exact live locals handed to the first candidate iteration at `0x006E1AE1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaderProduceBuildingCandidateLoopBoundary {
    pub va: u32,
    pub bytes_remaining: u32,
    pub owner: u8,
    pub type_index: i32,
    pub origin_build_object: i16,
    pub mode: i32,
    pub origin_world_cell: [i32; 2],
    pub origin_region: i16,
    pub circle_radius_index: usize,
    pub circle_offset: i32,
    pub footprint_search: [i32; 3],
    pub resource_sensitive_search: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaderProduceBuildingSearchSetupReceipt {
    pub input: LeaderProduceBuildingSearchBoundary,
    pub status: LeaderProduceBuildingSearchSetupStatus,
    pub origin_build_row: usize,
    pub origin_type: i32,
    pub origin_coord: [i32; 2],
    pub origin_world_cell: [i32; 2],
    pub origin_region: i16,
    pub leader_radius: i32,
    pub circle_radius_index: usize,
    pub initial_circle_offset: i32,
    pub circle_radius_end: i32,
    pub target_build_flags: u32,
    pub target_is_dock: bool,
    pub target_footprint: Footprint,
    /// Retail arguments `(x_search, y_search, combined_search)` to the candidate finder.
    pub footprint_search: [i32; 3],
    pub resource_sensitive_search: bool,
    pub native_returned: Option<i32>,
    pub scenario_returned: Option<i32>,
    pub continuation: Option<LeaderProduceBuildingCandidateLoopBoundary>,
}

impl LeaderProduceBuildingSearchSetupReceipt {
    pub fn validates(&self) -> bool {
        let exhausted = self.initial_circle_offset >= self.circle_radius_end;
        match self.status {
            LeaderProduceBuildingSearchSetupStatus::SearchRadiusExhaustedBeforeCandidateLoop => {
                exhausted
                    && self.native_returned == Some(1)
                    && self.scenario_returned == Some(0)
                    && self.continuation.is_none()
            }
            LeaderProduceBuildingSearchSetupStatus::ReadyForCandidateLoop => {
                !exhausted
                    && self.native_returned.is_none()
                    && self.scenario_returned.is_none()
                    && self.continuation.is_some_and(|next| {
                        next.va == LEADER_PRODUCE_BUILDING_CANDIDATE_LOOP_VA
                            && next.bytes_remaining
                                == LEADER_PRODUCE_BUILDING_CANDIDATE_BYTES_REMAINING
                            && next.circle_offset == self.initial_circle_offset
                    })
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeaderProduceBuildingSearchSetupError {
    WrongBoundary { va: u32, bytes_remaining: u32 },
    InvalidOwner { owner: usize },
    InvalidType { type_index: i32 },
    MissingBuildObject { owner: usize, object_index: i16 },
    BuildRowOutOfRange { row: usize, builds: usize },
    BuildIdentityMismatch { row: usize },
    MissingOriginType { row: usize },
    OriginTypeMismatch { type_index: i32 },
    TargetTypeMismatch { type_index: i32 },
    UnsupportedNonzeroFrame { frame: i32 },
    UnsupportedOilWellSearch,
    MissingTargetFootprint,
    InvalidTargetFootprint { footprint: Footprint },
    OriginWorldCellOutOfRange { x: i32, y: i32 },
    CircleRadiusOutOfRange { radius: i32, index: i32 },
}

#[inline]
fn world_cell_of(coord: i32) -> i32 {
    // `div_3_table[coord >> 8]`, the exact `Coord -> WCoord` conversion retail uses here.
    (coord >> 8) / 3
}

#[inline]
fn quarter_radius(radius: i32) -> i32 {
    let plus_two = radius.wrapping_add(2);
    plus_two.wrapping_add((plus_two >> 31) & 3) >> 2
}

/// Execute the exact mutation-free frame-zero setup and stop before the candidate loop.
pub fn apply_sim_leader_produce_building_search_setup(
    sim: &Sim,
    production: &LiveProductionRuntime,
    types: &TypeBuiltinState,
    input: LeaderProduceBuildingSearchBoundary,
) -> Result<LeaderProduceBuildingSearchSetupReceipt, LeaderProduceBuildingSearchSetupError> {
    let expected_remaining = LEADER_PRODUCE_BUILDING_CANDIDATE_BYTES_REMAINING
        + LEADER_PRODUCE_BUILDING_SEARCH_SETUP_BYTES;
    if input.va != LEADER_PRODUCE_BUILDING_CITY_GATE_CONTINUATION_VA
        || input.bytes_remaining != expected_remaining
    {
        return Err(LeaderProduceBuildingSearchSetupError::WrongBoundary {
            va: input.va,
            bytes_remaining: input.bytes_remaining,
        });
    }
    let owner = input.owner as usize;
    if owner >= NUM_LEADERS {
        return Err(LeaderProduceBuildingSearchSetupError::InvalidOwner { owner });
    }
    let type_index = usize::try_from(input.type_index).map_err(|_| {
        LeaderProduceBuildingSearchSetupError::InvalidType {
            type_index: input.type_index,
        }
    })?;
    let Some(target) = production.types.get(type_index).and_then(Option::as_ref) else {
        return Err(LeaderProduceBuildingSearchSetupError::InvalidType {
            type_index: input.type_index,
        });
    };
    let Some(target_row) = types.types.rows().get(type_index) else {
        return Err(LeaderProduceBuildingSearchSetupError::InvalidType {
            type_index: input.type_index,
        });
    };
    if target.type_index != input.type_index
        || target.class != LiveTypeClass::Building
        || target_row.index != input.type_index
        || target_row.domain() != TypeDomain::Build
    {
        return Err(LeaderProduceBuildingSearchSetupError::TargetTypeMismatch {
            type_index: input.type_index,
        });
    }

    let object_index = input.origin_build_object;
    if !(BUILD_BAND_BASE as i16..WALL_BAND_BASE as i16).contains(&object_index) {
        return Err(LeaderProduceBuildingSearchSetupError::MissingBuildObject {
            owner,
            object_index,
        });
    }
    let slot = (object_index - BUILD_BAND_BASE as i16) as usize;
    let Some(&row) = sim.world.objects.slot(owner).band(Band::Build).get(slot) else {
        return Err(LeaderProduceBuildingSearchSetupError::MissingBuildObject {
            owner,
            object_index,
        });
    };
    let row = row as usize;
    let Some(build) = sim.builds.get(row) else {
        return Err(LeaderProduceBuildingSearchSetupError::BuildRowOutOfRange {
            row,
            builds: sim.builds.len(),
        });
    };
    if build.who as usize != owner || build.object_id() != object_index || !build.is_valid() {
        return Err(LeaderProduceBuildingSearchSetupError::BuildIdentityMismatch { row });
    }
    let Some(origin_type) = production.build_types.get(row).copied().flatten() else {
        return Err(LeaderProduceBuildingSearchSetupError::MissingOriginType { row });
    };
    let Some(origin) = production
        .types
        .get(usize::try_from(origin_type).unwrap_or(usize::MAX))
        .and_then(Option::as_ref)
    else {
        return Err(LeaderProduceBuildingSearchSetupError::OriginTypeMismatch {
            type_index: origin_type,
        });
    };
    if origin.type_index != origin_type || origin.class != LiveTypeClass::Building {
        return Err(LeaderProduceBuildingSearchSetupError::OriginTypeMismatch {
            type_index: origin_type,
        });
    }

    // The next retail arm is materially different once Game::frame is nonzero: it changes
    // the radius and first offset through three more Type relations. Keep that policy red.
    if sim.world.frame != 0 {
        return Err(
            LeaderProduceBuildingSearchSetupError::UnsupportedNonzeroFrame {
                frame: sim.world.frame,
            },
        );
    }
    if input.type_index == OIL_WELL_TYPE {
        return Err(LeaderProduceBuildingSearchSetupError::UnsupportedOilWellSearch);
    }

    let origin_coord = build.position();
    let origin_world_cell = [world_cell_of(origin_coord.0), world_cell_of(origin_coord.1)];
    if !sim
        .map
        .world
        .valid_w(origin_world_cell[0], origin_world_cell[1])
    {
        return Err(
            LeaderProduceBuildingSearchSetupError::OriginWorldCellOutOfRange {
                x: origin_world_cell[0],
                y: origin_world_cell[1],
            },
        );
    }
    let origin_region = sim
        .map
        .world
        .wdata(origin_world_cell[0], origin_world_cell[1])
        .region;
    let leader_radius = city_radius(
        &CityRules::RETAIL,
        origin_type,
        sim.step8.leaders[owner].unit_stats.has_tribe_bonus(0x15),
    );
    let radius_index = quarter_radius(leader_radius);
    let circle = combat::circle_table();
    let Ok(circle_radius_index) = usize::try_from(radius_index) else {
        return Err(
            LeaderProduceBuildingSearchSetupError::CircleRadiusOutOfRange {
                radius: leader_radius,
                index: radius_index,
            },
        );
    };
    let Some(&circle_radius_end) = circle.ring_end.get(circle_radius_index) else {
        return Err(
            LeaderProduceBuildingSearchSetupError::CircleRadiusOutOfRange {
                radius: leader_radius,
                index: radius_index,
            },
        );
    };

    let target_footprint = target
        .build_visibility
        .ok_or(LeaderProduceBuildingSearchSetupError::MissingTargetFootprint)?
        .footprint
        .ok_or(LeaderProduceBuildingSearchSetupError::MissingTargetFootprint)?;
    if target_footprint.x_size <= 0 || target_footprint.y_size <= 0 {
        return Err(
            LeaderProduceBuildingSearchSetupError::InvalidTargetFootprint {
                footprint: target_footprint,
            },
        );
    }
    let target_is_dock = target_row
        .is_list
        .iter()
        .any(|&related| usize::from(related) == DOCK_TYPE);
    let footprint_search = if target_is_dock {
        [4, 4, 8]
    } else {
        let x = if target_footprint.x_size < 4 {
            5 - target_footprint.x_size
        } else {
            1
        };
        let y = if target_footprint.y_size < 4 {
            5 - target_footprint.y_size
        } else {
            1
        };
        let combined = if target_footprint.x_size < 4 || target_footprint.y_size < 4 {
            x + y
        } else {
            2
        };
        [x, y, combined]
    };
    let resource_sensitive_search = target.build_flags & BUILD_FLAG_RESOURCE_SENSITIVE_SEARCH != 0
        && !matches!(input.type_index, FARM_TYPE | MINE_TYPE | UNIVERSITY_TYPE);
    let initial_circle_offset = 1;

    let (status, native_returned, scenario_returned, continuation) =
        if initial_circle_offset >= circle_radius_end {
            (
                LeaderProduceBuildingSearchSetupStatus::SearchRadiusExhaustedBeforeCandidateLoop,
                Some(1),
                Some(0),
                None,
            )
        } else {
            (
                LeaderProduceBuildingSearchSetupStatus::ReadyForCandidateLoop,
                None,
                None,
                Some(LeaderProduceBuildingCandidateLoopBoundary {
                    va: LEADER_PRODUCE_BUILDING_CANDIDATE_LOOP_VA,
                    bytes_remaining: LEADER_PRODUCE_BUILDING_CANDIDATE_BYTES_REMAINING,
                    owner: input.owner,
                    type_index: input.type_index,
                    origin_build_object: input.origin_build_object,
                    mode: input.mode,
                    origin_world_cell,
                    origin_region,
                    circle_radius_index,
                    circle_offset: initial_circle_offset,
                    footprint_search,
                    resource_sensitive_search,
                }),
            )
        };
    let receipt = LeaderProduceBuildingSearchSetupReceipt {
        input,
        status,
        origin_build_row: row,
        origin_type,
        origin_coord: [origin_coord.0, origin_coord.1],
        origin_world_cell,
        origin_region,
        leader_radius,
        circle_radius_index,
        initial_circle_offset,
        circle_radius_end,
        target_build_flags: target.build_flags,
        target_is_dock,
        target_footprint,
        footprint_search,
        resource_sensitive_search,
        native_returned,
        scenario_returned,
        continuation,
    };
    debug_assert!(receipt.validates());
    Ok(receipt)
}
