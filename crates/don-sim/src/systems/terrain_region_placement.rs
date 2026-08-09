//! Exact candidate-selection prefix of `TerrainGroup::place_region_group`.
//!
//! The shipped PDB identifies `TerrainGroup::place_region_group` at
//! `0x006a2f60` (4,647 bytes, `terraingroups.cpp:809..1318`).  This module
//! implements its complete entry search through the first call to
//! `TerrainGroup::drop_tile` (`0x006a2ab0`).  That call is the first mutation
//! kernel: the prefix itself only clears the native group's temporary tile-list
//! length, reads the world/region planes, and advances the map RNG once when a
//! region contains more than one coordinate.

use super::ammo::vector_dist;
use super::borders_fog::CircleTable;
use super::map_terrain::{land, tflag, wflag, World};
use super::regions::{Regions, REGION_COUNT};
use super::terrain_groups::TerrainGroup;
use crate::rng::Random;

const REGION_CURSOR_XOR: u32 = 0x0001_6800;
const REGION_CURSOR_MAX: usize = 0xffff;
const TYPE_FOUR_DX: [i32; 9] = [0, -1, 0, 1, 1, 1, 0, -1, -1];
const TYPE_FOUR_DY: [i32; 9] = [0, -1, -1, -1, 0, 1, 1, 1, 0];
const CORNER_X: [i32; 4] = [0, 0, 1, 1];
const CORNER_Y: [i32; 4] = [0, 1, 0, 1];

/// The arguments not already stored on `TerrainGroup` at the `place_region_group`
/// call in `TerrainGroups::place_all` (`0x006a8249..0x006a827c`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PlaceRegionGroupCall {
    pub target_tiles: i32,
    pub region_id: usize,
    pub land_subtype: i32,
    pub oil_deposits: i32,
    pub place_players: i32,
    pub group_index: usize,
}

/// State read only when retail's global `is_helping` is nonzero.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RegionHelpingState {
    /// Retail global `is_helping`. Scores are updated after success regardless;
    /// this flag gates only the nearest-player candidate filter.
    pub is_helping: bool,
    pub num_players: usize,
    /// The five shipped `lowest_player[type - 4]` entries.
    pub lowest_player: [i32; 5],
    /// Retail's player-major five-column helping-distance table.
    /// `place_region_group` adds the first successful tile's distance to every
    /// participating player, then recomputes `lowest_player` from this table.
    pub scores: [[i32; 5]; 8],
}

/// The consumed part of the first `drop_tile` invocation.
///
/// Its PDB signature has seven integer arguments. Disassembly proves arguments
/// four and seven are the same dead `ECX` residue from a preceding logging
/// destructor, and `drop_tile` never reads either. The five values below are
/// every argument that can affect the mutation kernel.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RegionDropTileInvocation {
    pub world_x: i32,
    pub world_y: i32,
    pub group_type: i32,
    pub group_radius: i32,
    pub land_subtype: i32,
    /// Continuation context used after the first successful mutation.
    pub target_tiles: i32,
    pub oil_deposits: i32,
    pub group_index: usize,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RegionCandidateRejection {
    LandClass,
    OccupiedTile,
    StartCityLocation,
    PlayerStartMinimum {
        player: usize,
        distance: i32,
    },
    PlayerStartMaximum {
        player: usize,
        distance: i32,
    },
    ForbiddenWorldFlags {
        flags: u16,
    },
    BlockedWorldCell,
    TypeSevenWaterRing {
        offset_index: usize,
    },
    CenterMinimum {
        distance: i32,
    },
    CenterMaximum {
        distance: i32,
    },
    EdgeMinimum {
        distance: i32,
    },
    EdgeMaximum {
        distance: i32,
    },
    /// Retail compares the final Y coordinate with `xs - 1`, not `ys - 1`.
    HardBoundary,
    CornerMinimum {
        distance: i32,
    },
    CornerMaximum {
        distance: i32,
    },
    TypeFourImpassableNeighbor {
        offset_index: usize,
    },
    HelpingPlayer {
        nearest: i32,
        required: i32,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RegionCandidateAttempt {
    /// One-based shipped LFSR state. `coord_index == cursor - 1`.
    pub cursor: u32,
    pub coord_index: usize,
    pub world_x: i32,
    pub world_y: i32,
    pub rejection: Option<RegionCandidateRejection>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaceRegionGroupPrefixOutcome {
    Exhausted,
    DropTile(RegionDropTileInvocation),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceRegionGroupPrefixReceipt {
    /// Native writes `tiles.length = 0` before its first region/RNG read.
    pub cleared_tiles: usize,
    pub initial_cursor: u32,
    pub region_cursor_draws: u32,
    pub attempts: Vec<RegionCandidateAttempt>,
    pub outcome: PlaceRegionGroupPrefixOutcome,
    pub rng_state_after: i32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PlaceRegionGroupPrefixError {
    InvalidWorldShape {
        xs: i32,
        ys: i32,
        size: i32,
        wdata_len: usize,
        tile_xs: i32,
        tile_ys: i32,
        tile_size: i32,
        tdata_len: usize,
    },
    InvalidRegion {
        region_id: usize,
    },
    EmptyRegion {
        region_id: usize,
    },
    RegionTooLarge {
        region_id: usize,
        coordinates: usize,
    },
    RegionCoordinateOutOfBounds {
        region_id: usize,
        coord_index: usize,
        world_x: i32,
        world_y: i32,
    },
    StartArrayLengthMismatch {
        x: usize,
        y: usize,
    },
    StartCityMaskTooShort {
        required_bytes: usize,
        actual_bytes: usize,
    },
    InvalidHelpingPlayerCount {
        requested: usize,
        available: usize,
    },
    UnsupportedHelpingGroupType {
        group_type: i32,
    },
    InvalidCursorCycle {
        region_id: usize,
    },
}

impl TerrainGroup {
    /// Complete deterministic prefix of `TerrainGroup::place_region_group`
    /// (`0x006a2f60`) through the first `drop_tile` call.
    ///
    /// Validation is deliberately performed before the possible region-index
    /// draw. Unsupported fabricated state therefore fails without advancing the
    /// RNG. The returned receipt records the native `tiles.length = 0` write, but
    /// this planning adapter does not mutate the group or world; composition can
    /// remain transactional until `drop_tile` itself is recovered.
    pub fn plan_place_region_group_prefix(
        &self,
        world: &World,
        regions: &Regions,
        random: &mut Random,
        call: PlaceRegionGroupCall,
        helping: Option<RegionHelpingState>,
    ) -> Result<PlaceRegionGroupPrefixReceipt, PlaceRegionGroupPrefixError> {
        validate_inputs(self, world, regions, call, helping)?;

        let coords = &regions.list[call.region_id].coords.items;
        let region_cursor_draws = u32::from(coords.len() > 1);
        let initial_cursor = if coords.len() > 1 {
            (random.get(0, 0xffff) % coords.len() as i32) as u32 + 1
        } else {
            1
        };
        let mut cursor = initial_cursor;
        let mut attempts = Vec::with_capacity(coords.len());

        loop {
            let coord_index = cursor as usize - 1;
            let (world_x, world_y) = coords[coord_index];
            let rejection = reject_candidate(self, world, world_x, world_y, call, helping);
            attempts.push(RegionCandidateAttempt {
                cursor,
                coord_index,
                world_x,
                world_y,
                rejection,
            });

            if rejection.is_none() {
                // 0x006a3963..0x006a3971: signed truncation toward zero.
                let group_radius = call.target_tiles / 4 + 1;
                return Ok(PlaceRegionGroupPrefixReceipt {
                    cleared_tiles: self.tiles.items.len(),
                    initial_cursor,
                    region_cursor_draws,
                    attempts,
                    outcome: PlaceRegionGroupPrefixOutcome::DropTile(RegionDropTileInvocation {
                        world_x,
                        world_y,
                        group_type: self.group_type,
                        group_radius,
                        land_subtype: call.land_subtype,
                        target_tiles: call.target_tiles,
                        oil_deposits: call.oil_deposits,
                        group_index: call.group_index,
                    }),
                    rng_state_after: random.state(),
                });
            }

            cursor = next_region_cursor(cursor, coords.len() as u32, initial_cursor).ok_or(
                PlaceRegionGroupPrefixError::InvalidCursorCycle {
                    region_id: call.region_id,
                },
            )?;
            if cursor == initial_cursor {
                return Ok(PlaceRegionGroupPrefixReceipt {
                    cleared_tiles: self.tiles.items.len(),
                    initial_cursor,
                    region_cursor_draws,
                    attempts,
                    outcome: PlaceRegionGroupPrefixOutcome::Exhausted,
                    rng_state_after: random.state(),
                });
            }
        }
    }
}

pub(crate) fn validate_inputs(
    group: &TerrainGroup,
    world: &World,
    regions: &Regions,
    call: PlaceRegionGroupCall,
    helping: Option<RegionHelpingState>,
) -> Result<(), PlaceRegionGroupPrefixError> {
    let world_size = world.xs.checked_mul(world.ys);
    let tile_size = world.tile_xs.checked_mul(world.tile_ys);
    if world.xs <= 0
        || world.ys <= 0
        || world.tile_xs <= 0
        || world.tile_ys <= 0
        || world_size != Some(world.size)
        || usize::try_from(world.size).ok() != Some(world.wdata.len())
        || tile_size != Some(world.tile_size)
        || usize::try_from(world.tile_size).ok() != Some(world.tdata.len())
    {
        return Err(PlaceRegionGroupPrefixError::InvalidWorldShape {
            xs: world.xs,
            ys: world.ys,
            size: world.size,
            wdata_len: world.wdata.len(),
            tile_xs: world.tile_xs,
            tile_ys: world.tile_ys,
            tile_size: world.tile_size,
            tdata_len: world.tdata.len(),
        });
    }
    if call.region_id >= REGION_COUNT {
        return Err(PlaceRegionGroupPrefixError::InvalidRegion {
            region_id: call.region_id,
        });
    }
    let coords = &regions.list[call.region_id].coords.items;
    if coords.is_empty() {
        return Err(PlaceRegionGroupPrefixError::EmptyRegion {
            region_id: call.region_id,
        });
    }
    if coords.len() > REGION_CURSOR_MAX {
        return Err(PlaceRegionGroupPrefixError::RegionTooLarge {
            region_id: call.region_id,
            coordinates: coords.len(),
        });
    }
    for (coord_index, &(world_x, world_y)) in coords.iter().enumerate() {
        if !world.valid_w(world_x, world_y)
            || call.place_players == 0
                && (world_y
                    .checked_mul(world.tile_xs)
                    .and_then(|row| row.checked_add(world_x))
                    .and_then(|index| usize::try_from(index).ok()))
                .is_none_or(|index| index >= world.tdata.len())
        {
            return Err(PlaceRegionGroupPrefixError::RegionCoordinateOutOfBounds {
                region_id: call.region_id,
                coord_index,
                world_x,
                world_y,
            });
        }
    }
    if world.start_x.items.len() != world.start_y.items.len() {
        return Err(PlaceRegionGroupPrefixError::StartArrayLengthMismatch {
            x: world.start_x.items.len(),
            y: world.start_y.items.len(),
        });
    }
    if call.place_players != 0 {
        let required_bytes = world.size as usize / 8 + usize::from(world.size & 7 != 0);
        if world.start_city_locs.len() < required_bytes {
            return Err(PlaceRegionGroupPrefixError::StartCityMaskTooShort {
                required_bytes,
                actual_bytes: world.start_city_locs.len(),
            });
        }
    }
    if let Some(helping) = helping {
        // `lowest_player` is indexed by `type - 4` only in helping mode.
        if !(4..=8).contains(&group.group_type) {
            return Err(PlaceRegionGroupPrefixError::UnsupportedHelpingGroupType {
                group_type: group.group_type,
            });
        }
        if helping.num_players > helping.scores.len()
            || helping.num_players > world.start_x.items.len()
        {
            return Err(PlaceRegionGroupPrefixError::InvalidHelpingPlayerCount {
                requested: helping.num_players,
                available: world.start_x.items.len(),
            });
        }
    }
    Ok(())
}

pub(crate) fn reject_candidate(
    group: &TerrainGroup,
    world: &World,
    x: i32,
    y: i32,
    call: PlaceRegionGroupCall,
    helping: Option<RegionHelpingState>,
) -> Option<RegionCandidateRejection> {
    let cell = world.wdata(x, y);
    let classified_land = if cell.flags & wflag::COAST != 0 {
        land::OCEAN
    } else {
        cell.land
    };
    if group.group_type == 7 {
        if classified_land != land::OCEAN {
            return Some(RegionCandidateRejection::LandClass);
        }
    } else if cell.flags & wflag::WATERHALF == 0 && matches!(cell.land, land::COASTAL | land::OCEAN)
    {
        return Some(RegionCandidateRejection::LandClass);
    }

    if call.place_players == 0 {
        let tile = world.tdata[(world.tile_xs * y + x) as usize];
        if tile & tflag::STARTED != 0 || tile & tflag::BLOCKER_MASK == tflag::BLOCKER_BUILDING {
            return Some(RegionCandidateRejection::OccupiedTile);
        }
    } else if world.start_city_wcoord(super::map_terrain::WCoord(x), super::map_terrain::WCoord(y))
    {
        return Some(RegionCandidateRejection::StartCityLocation);
    }

    for (player, (&start_x, &start_y)) in world
        .start_x
        .items
        .iter()
        .zip(&world.start_y.items)
        .enumerate()
    {
        let distance = vector_dist(x.wrapping_sub(start_x), y.wrapping_sub(start_y));
        if group.start_min > 0 && distance < group.start_min {
            return Some(RegionCandidateRejection::PlayerStartMinimum { player, distance });
        }
        if group.start_max > 0 && group.start_max < distance {
            return Some(RegionCandidateRejection::PlayerStartMaximum { player, distance });
        }
    }

    let forbidden = cell.flags & (wflag::MOUNTAINS | 0x0068 | 0x1800);
    if forbidden != 0 {
        return Some(RegionCandidateRejection::ForbiddenWorldFlags { flags: forbidden });
    }
    if cell.blocked != 0 {
        return Some(RegionCandidateRejection::BlockedWorldCell);
    }

    if group.group_type == 7 {
        let circle = CircleTable::build();
        let radius = if group.rock_space > 3 {
            group.rock_space.min(63) as usize
        } else {
            3
        };
        let end = circle.radius[radius] as usize;
        let inner_end = circle.radius[2] as usize;
        for offset_index in 0..end {
            let nx = x + i32::from(circle.x[offset_index]);
            let ny = y + i32::from(circle.y[offset_index]);
            if !world.valid_w(nx, ny) {
                continue;
            }
            let neighbor = world.wdata(nx, ny);
            let full_water = neighbor.flags & wflag::WATERHALF == 0
                && matches!(neighbor.land, land::COASTAL | land::OCEAN);
            if full_water && neighbor.flags & wflag::OIL == 0 {
                continue;
            }
            if full_water || offset_index < inner_end {
                return Some(RegionCandidateRejection::TypeSevenWaterRing { offset_index });
            }
        }
    }

    let center_x = world.xs >> 1;
    let center_y = world.ys >> 1;
    let target_x = center_x + i32::from(x > center_x);
    let target_y = center_y + i32::from(y > center_y);
    let center_distance = vector_dist(x - target_x, y - target_y);
    if group.cent_min > 0 && center_distance < group.cent_min {
        return Some(RegionCandidateRejection::CenterMinimum {
            distance: center_distance,
        });
    }
    if group.cent_max > 0 && group.cent_max < center_distance {
        return Some(RegionCandidateRejection::CenterMaximum {
            distance: center_distance,
        });
    }

    let edge_distance = y.min(x).min(world.xs - x).min(world.ys - y);
    if group.edge_min > 0 && edge_distance < group.edge_min {
        return Some(RegionCandidateRejection::EdgeMinimum {
            distance: edge_distance,
        });
    }
    if group.edge_max > 0 && group.edge_max < edge_distance {
        return Some(RegionCandidateRejection::EdgeMaximum {
            distance: edge_distance,
        });
    }

    if x == 0 || y == 0 || x == world.xs - 1 || y == world.xs - 1 {
        return Some(RegionCandidateRejection::HardBoundary);
    }

    let corner_distance = (0..4)
        .map(|corner| {
            vector_dist(
                x - CORNER_X[corner] * world.xs,
                y - CORNER_Y[corner] * world.ys,
            )
        })
        .min()
        .unwrap_or(99_999);
    if group.corner_min > 0 && corner_distance < group.corner_min {
        return Some(RegionCandidateRejection::CornerMinimum {
            distance: corner_distance,
        });
    }
    if group.corner_max > 0 && group.corner_max < corner_distance {
        return Some(RegionCandidateRejection::CornerMaximum {
            distance: corner_distance,
        });
    }

    if group.group_type == 4 {
        for offset_index in 0..TYPE_FOUR_DX.len() {
            let nx = x + TYPE_FOUR_DX[offset_index];
            let ny = y + TYPE_FOUR_DY[offset_index];
            if world.valid_w(nx, ny) && world.wdata(nx, ny).flags & wflag::IMPASSABLE_X != 0 {
                return Some(RegionCandidateRejection::TypeFourImpassableNeighbor { offset_index });
            }
        }
    }

    if helping.is_some_and(|helping| helping.is_helping) {
        let helping = helping.unwrap();
        let mut nearest_distance = i32::MAX;
        let mut nearest = i32::MAX;
        for player in 0..helping.num_players {
            let distance = vector_dist(
                world.start_x.items[player] - x,
                world.start_y.items[player] - y,
            );
            if distance < nearest_distance {
                nearest_distance = distance;
                nearest = player as i32;
            }
        }
        let required = helping.lowest_player[(group.group_type - 4) as usize];
        if nearest != required {
            return Some(RegionCandidateRejection::HelpingPlayer { nearest, required });
        }
    }

    None
}

pub(crate) fn next_region_cursor(mut cursor: u32, count: u32, start: u32) -> Option<u32> {
    for _ in 0..=REGION_CURSOR_MAX {
        if cursor & 1 != 0 {
            cursor ^= REGION_CURSOR_XOR;
        }
        cursor = ((cursor as i32) >> 1) as u32;
        if cursor <= count || cursor == start {
            return Some(cursor);
        }
    }
    None
}
