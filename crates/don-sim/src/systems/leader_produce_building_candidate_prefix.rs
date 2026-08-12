//! Exact first-candidate loop prefix in `Leader::produce_building`
//! (`0x006E1AE1..0x006E1D3D`).
//!
//! Retail walks the canonical `circle_x/y` order and rejects candidates by map bounds,
//! a W-cell exclusion bit, `WorldData::check_building_wcoord`, footprint/edge rules,
//! `WorldData::buildings_allowed`, and ocean/domain polarity. The first survivor is
//! converted to exact centre coordinates and reaches `BuildTypeData::blocked_site` at
//! `0x006E1D3D`. This tranche stops before that call: it never claims the site is valid,
//! consumes no RNG, pays no resources, allocates no Build, and issues no Group order.

use crate::objects::{Band, BUILD_BAND_BASE, WALL_BAND_BASE};
use crate::tick::{Sim, NUM_LEADERS};

use super::bhs_type_table::{TypeBuiltinState, TypeDomain};
use super::combat;
use super::leader_produce_building_search_setup::{
    LeaderProduceBuildingCandidateLoopBoundary, BUILD_FLAG_RESOURCE_SENSITIVE_SEARCH, DOCK_TYPE,
    FARM_TYPE, LEADER_PRODUCE_BUILDING_CANDIDATE_BYTES_REMAINING,
    LEADER_PRODUCE_BUILDING_CANDIDATE_LOOP_VA, MINE_TYPE, UNIVERSITY_TYPE,
};
use super::map_terrain::space;
use super::production::{
    runtime::{LiveProductionRuntime, LiveTypeClass},
    Footprint, COORD_HALF_TILE, COORD_PER_TILE,
};

pub const BUILD_TYPE_BLOCKED_SITE_VA: u32 = 0x0063_6a50;
pub const LEADER_PRODUCE_BUILDING_BLOCKED_SITE_CALL_VA: u32 = 0x006e_1d3d;
pub const LEADER_PRODUCE_BUILDING_CANDIDATE_PREFIX_BYTES: u32 =
    LEADER_PRODUCE_BUILDING_BLOCKED_SITE_CALL_VA - LEADER_PRODUCE_BUILDING_CANDIDATE_LOOP_VA;
pub const LEADER_PRODUCE_BUILDING_BLOCKED_SITE_BYTES_REMAINING: u32 = 0x13b1;

/// Unnamed WData flag tested directly at `0x006E1B65`. Its writer is not yet identified.
pub const WDATA_BUILD_CANDIDATE_EXCLUDED: u16 = 0x4000;
pub const OIL_PLATFORM_TYPE: i32 = 422;
pub const WATER_DOMAIN: i32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CandidatePrefixRejection {
    OutOfBounds,
    WDataExcluded { flags: u16 },
    BuildingSpaceBelowPartial { grade: i32 },
    FootprintRequiresHigherGrade { grade: i32, required: i32 },
    SmallFootprintOnMapEdge,
    BuildingsDisallowed,
    LandWaterDomainMismatch { ocean: bool, domain: i32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CandidatePrefixProbe {
    pub circle_offset: i32,
    pub world_cell: [i32; 2],
    pub space_grade: Option<i32>,
    pub rejection: CandidatePrefixRejection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaderProduceBuildingBlockedSiteBoundary {
    pub va: u32,
    pub callee_va: u32,
    pub bytes_remaining: u32,
    pub owner: u8,
    pub type_index: i32,
    pub origin_build_object: i16,
    pub mode: i32,
    pub circle_offset: i32,
    pub candidate_world_cell: [i32; 2],
    pub space_grade: i32,
    pub target_domain: i32,
    pub target_footprint: Footprint,
    /// First two `BuildTypeData::blocked_site` arguments, in Coord units.
    pub placement_coord: [i32; 2],
    /// Literal fourth argument pushed by retail.
    pub city_constraint: i32,
    /// The pointed-to local is zeroed immediately before the call.
    pub blocked_detail_initial: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaderProduceBuildingCandidatePrefixStatus {
    ExhaustedBeforeBlockedSite,
    ReadyForBlockedSite,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaderProduceBuildingCandidatePrefixReceipt {
    pub input: LeaderProduceBuildingCandidateLoopBoundary,
    pub status: LeaderProduceBuildingCandidatePrefixStatus,
    pub origin_build_row: usize,
    pub target_footprint: Footprint,
    pub target_domain: i32,
    pub circle_radius_end: i32,
    pub probes: Vec<CandidatePrefixProbe>,
    pub native_returned: Option<i32>,
    pub scenario_returned: Option<i32>,
    pub continuation: Option<LeaderProduceBuildingBlockedSiteBoundary>,
}

impl LeaderProduceBuildingCandidatePrefixReceipt {
    pub fn validates(&self) -> bool {
        match self.status {
            LeaderProduceBuildingCandidatePrefixStatus::ExhaustedBeforeBlockedSite => {
                self.continuation.is_none()
                    && self.native_returned == Some(1)
                    && self.scenario_returned == Some(0)
                    && self
                        .probes
                        .last()
                        .is_some_and(|last| last.circle_offset + 1 == self.circle_radius_end)
            }
            LeaderProduceBuildingCandidatePrefixStatus::ReadyForBlockedSite => {
                self.native_returned.is_none()
                    && self.scenario_returned.is_none()
                    && self.continuation.is_some_and(|next| {
                        next.va == LEADER_PRODUCE_BUILDING_BLOCKED_SITE_CALL_VA
                            && next.callee_va == BUILD_TYPE_BLOCKED_SITE_VA
                            && next.bytes_remaining
                                == LEADER_PRODUCE_BUILDING_BLOCKED_SITE_BYTES_REMAINING
                            && next.city_constraint == -1
                            && next.blocked_detail_initial == 0
                    })
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeaderProduceBuildingCandidatePrefixError {
    WrongBoundary { va: u32, bytes_remaining: u32 },
    InvalidOwner { owner: usize },
    InvalidType { type_index: i32 },
    TargetTypeMismatch { type_index: i32 },
    MissingTargetFootprint,
    MissingTargetDomain,
    InvalidTargetFootprint { footprint: Footprint },
    SearchSetupMismatch,
    UnsupportedNonzeroFrame { frame: i32 },
    MissingBuildObject { owner: usize, object_index: i16 },
    BuildRowOutOfRange { row: usize, builds: usize },
    BuildIdentityMismatch { row: usize },
    CircleBoundaryOutOfRange { radius_index: usize, offset: i32 },
}

#[inline]
fn world_cell_of(coord: i32) -> i32 {
    (coord >> 8) / 3
}

fn search_shape(footprint: Footprint, is_dock: bool) -> [i32; 3] {
    if is_dock {
        return [4, 4, 8];
    }
    let x = if footprint.x_size < 4 {
        5 - footprint.x_size
    } else {
        1
    };
    let y = if footprint.y_size < 4 {
        5 - footprint.y_size
    } else {
        1
    };
    [
        x,
        y,
        if footprint.x_size < 4 || footprint.y_size < 4 {
            x + y
        } else {
            2
        },
    ]
}

#[inline]
fn placement_axis(cell: i32, size: i32) -> i32 {
    let base = cell
        .wrapping_mul(4 * COORD_PER_TILE)
        .wrapping_add(size.wrapping_mul(COORD_HALF_TILE));
    if size < 4 {
        base.wrapping_add(4i32.wrapping_sub(size).wrapping_mul(COORD_PER_TILE) / 2)
    } else {
        base
    }
}

/// Walk the exact frame-zero candidate prefix and stop before `blocked_site`.
pub fn apply_sim_leader_produce_building_candidate_prefix(
    sim: &Sim,
    production: &LiveProductionRuntime,
    types: &TypeBuiltinState,
    input: LeaderProduceBuildingCandidateLoopBoundary,
) -> Result<LeaderProduceBuildingCandidatePrefixReceipt, LeaderProduceBuildingCandidatePrefixError>
{
    if input.va != LEADER_PRODUCE_BUILDING_CANDIDATE_LOOP_VA
        || input.bytes_remaining != LEADER_PRODUCE_BUILDING_CANDIDATE_BYTES_REMAINING
    {
        return Err(LeaderProduceBuildingCandidatePrefixError::WrongBoundary {
            va: input.va,
            bytes_remaining: input.bytes_remaining,
        });
    }
    let owner = input.owner as usize;
    if owner >= NUM_LEADERS {
        return Err(LeaderProduceBuildingCandidatePrefixError::InvalidOwner { owner });
    }
    let type_index = usize::try_from(input.type_index).map_err(|_| {
        LeaderProduceBuildingCandidatePrefixError::InvalidType {
            type_index: input.type_index,
        }
    })?;
    let Some(target) = production.types.get(type_index).and_then(Option::as_ref) else {
        return Err(LeaderProduceBuildingCandidatePrefixError::InvalidType {
            type_index: input.type_index,
        });
    };
    let Some(target_row) = types.types.rows().get(type_index) else {
        return Err(LeaderProduceBuildingCandidatePrefixError::InvalidType {
            type_index: input.type_index,
        });
    };
    if target.type_index != input.type_index
        || target.class != LiveTypeClass::Building
        || target_row.index != input.type_index
        || target_row.domain() != TypeDomain::Build
    {
        return Err(
            LeaderProduceBuildingCandidatePrefixError::TargetTypeMismatch {
                type_index: input.type_index,
            },
        );
    }
    let visibility = target
        .build_visibility
        .ok_or(LeaderProduceBuildingCandidatePrefixError::MissingTargetFootprint)?;
    let footprint = visibility
        .footprint
        .ok_or(LeaderProduceBuildingCandidatePrefixError::MissingTargetFootprint)?;
    if footprint.x_size <= 0 || footprint.y_size <= 0 {
        return Err(
            LeaderProduceBuildingCandidatePrefixError::InvalidTargetFootprint { footprint },
        );
    }
    let domain = visibility
        .domain
        .ok_or(LeaderProduceBuildingCandidatePrefixError::MissingTargetDomain)?;
    let is_dock = target_row
        .is_list
        .iter()
        .any(|&related| usize::from(related) == DOCK_TYPE);
    let expected_resource_sensitive = target.build_flags & BUILD_FLAG_RESOURCE_SENSITIVE_SEARCH
        != 0
        && !matches!(input.type_index, FARM_TYPE | MINE_TYPE | UNIVERSITY_TYPE);
    if input.footprint_search != search_shape(footprint, is_dock)
        || input.resource_sensitive_search != expected_resource_sensitive
    {
        return Err(LeaderProduceBuildingCandidatePrefixError::SearchSetupMismatch);
    }
    if sim.world.frame != 0 {
        return Err(
            LeaderProduceBuildingCandidatePrefixError::UnsupportedNonzeroFrame {
                frame: sim.world.frame,
            },
        );
    }

    let object_index = input.origin_build_object;
    if !(BUILD_BAND_BASE as i16..WALL_BAND_BASE as i16).contains(&object_index) {
        return Err(
            LeaderProduceBuildingCandidatePrefixError::MissingBuildObject {
                owner,
                object_index,
            },
        );
    }
    let slot = (object_index - BUILD_BAND_BASE as i16) as usize;
    let Some(&row) = sim.world.objects.slot(owner).band(Band::Build).get(slot) else {
        return Err(
            LeaderProduceBuildingCandidatePrefixError::MissingBuildObject {
                owner,
                object_index,
            },
        );
    };
    let row = row as usize;
    let Some(build) = sim.builds.get(row) else {
        return Err(
            LeaderProduceBuildingCandidatePrefixError::BuildRowOutOfRange {
                row,
                builds: sim.builds.len(),
            },
        );
    };
    if build.who as usize != owner || build.object_id() != object_index || !build.is_valid() {
        return Err(LeaderProduceBuildingCandidatePrefixError::BuildIdentityMismatch { row });
    }
    let origin_coord = build.position();
    let origin_world_cell = [world_cell_of(origin_coord.0), world_cell_of(origin_coord.1)];
    if origin_world_cell != input.origin_world_cell
        || !sim
            .map
            .world
            .valid_w(origin_world_cell[0], origin_world_cell[1])
        || sim
            .map
            .world
            .wdata(origin_world_cell[0], origin_world_cell[1])
            .region
            != input.origin_region
    {
        return Err(LeaderProduceBuildingCandidatePrefixError::SearchSetupMismatch);
    }

    let circle = combat::circle_table();
    let Some(&circle_radius_end) = circle.ring_end.get(input.circle_radius_index) else {
        return Err(
            LeaderProduceBuildingCandidatePrefixError::CircleBoundaryOutOfRange {
                radius_index: input.circle_radius_index,
                offset: input.circle_offset,
            },
        );
    };
    if input.circle_offset < 0
        || input.circle_offset >= circle_radius_end
        || usize::try_from(input.circle_offset)
            .ok()
            .is_none_or(|offset| offset >= circle.x.len() || offset >= circle.y.len())
    {
        return Err(
            LeaderProduceBuildingCandidatePrefixError::CircleBoundaryOutOfRange {
                radius_index: input.circle_radius_index,
                offset: input.circle_offset,
            },
        );
    }

    let mut probes = Vec::new();
    for circle_offset in input.circle_offset..circle_radius_end {
        let offset = circle_offset as usize;
        let world_cell = [
            origin_world_cell[0].wrapping_add(i32::from(circle.x[offset])),
            origin_world_cell[1].wrapping_add(i32::from(circle.y[offset])),
        ];
        let mut space_grade = None;
        let rejection = if !sim.map.world.valid_w(world_cell[0], world_cell[1]) {
            Some(CandidatePrefixRejection::OutOfBounds)
        } else {
            let cell = sim.map.world.wdata(world_cell[0], world_cell[1]);
            if cell.flags & WDATA_BUILD_CANDIDATE_EXCLUDED != 0 {
                Some(CandidatePrefixRejection::WDataExcluded { flags: cell.flags })
            } else {
                let grade = sim.map.world.check_building_wcoord(
                    world_cell[0],
                    world_cell[1],
                    owner as i32,
                    input.footprint_search[0],
                    input.footprint_search[1],
                    input.footprint_search[2],
                    true,
                );
                space_grade = Some(grade);
                if grade < space::PARTIAL {
                    probes.push(CandidatePrefixProbe {
                        circle_offset,
                        world_cell,
                        space_grade: Some(grade),
                        rejection: CandidatePrefixRejection::BuildingSpaceBelowPartial { grade },
                    });
                    continue;
                }
                let max_size = footprint.x_size.max(footprint.y_size);
                if max_size <= 4 && grade < max_size {
                    Some(CandidatePrefixRejection::FootprintRequiresHigherGrade {
                        grade,
                        required: max_size,
                    })
                } else if max_size <= 4
                    && (world_cell[0] == 0
                        || world_cell[1] == 0
                        || world_cell[0] == sim.map.world.xs - 1
                        || world_cell[1] == sim.map.world.ys - 1)
                {
                    Some(CandidatePrefixRejection::SmallFootprintOnMapEdge)
                } else if input.type_index != OIL_PLATFORM_TYPE
                    && !sim
                        .map
                        .world
                        .buildings_allowed(world_cell[0], world_cell[1])
                {
                    Some(CandidatePrefixRejection::BuildingsDisallowed)
                } else {
                    let ocean = sim.map.world.is_ocean(world_cell[0], world_cell[1]);
                    if ocean != (domain == WATER_DOMAIN) {
                        Some(CandidatePrefixRejection::LandWaterDomainMismatch { ocean, domain })
                    } else {
                        let boundary = LeaderProduceBuildingBlockedSiteBoundary {
                            va: LEADER_PRODUCE_BUILDING_BLOCKED_SITE_CALL_VA,
                            callee_va: BUILD_TYPE_BLOCKED_SITE_VA,
                            bytes_remaining: LEADER_PRODUCE_BUILDING_BLOCKED_SITE_BYTES_REMAINING,
                            owner: input.owner,
                            type_index: input.type_index,
                            origin_build_object: input.origin_build_object,
                            mode: input.mode,
                            circle_offset,
                            candidate_world_cell: world_cell,
                            space_grade: grade,
                            target_domain: domain,
                            target_footprint: footprint,
                            placement_coord: [
                                placement_axis(world_cell[0], footprint.x_size),
                                placement_axis(world_cell[1], footprint.y_size),
                            ],
                            city_constraint: -1,
                            blocked_detail_initial: 0,
                        };
                        let receipt = LeaderProduceBuildingCandidatePrefixReceipt {
                            input,
                            status: LeaderProduceBuildingCandidatePrefixStatus::ReadyForBlockedSite,
                            origin_build_row: row,
                            target_footprint: footprint,
                            target_domain: domain,
                            circle_radius_end,
                            probes,
                            native_returned: None,
                            scenario_returned: None,
                            continuation: Some(boundary),
                        };
                        debug_assert!(receipt.validates());
                        return Ok(receipt);
                    }
                }
            }
        };
        if let Some(rejection) = rejection {
            probes.push(CandidatePrefixProbe {
                circle_offset,
                world_cell,
                space_grade,
                rejection,
            });
        }
    }

    let receipt = LeaderProduceBuildingCandidatePrefixReceipt {
        input,
        status: LeaderProduceBuildingCandidatePrefixStatus::ExhaustedBeforeBlockedSite,
        origin_build_row: row,
        target_footprint: footprint,
        target_domain: domain,
        circle_radius_end,
        probes,
        native_returned: Some(1),
        scenario_returned: Some(0),
        continuation: None,
    };
    debug_assert!(receipt.validates());
    Ok(receipt)
}
