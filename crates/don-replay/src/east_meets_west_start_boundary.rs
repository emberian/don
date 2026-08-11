//! Exact East Meets West caller setup through its first start-selection call.

use crate::continent::TeamContinentPartitionReceipt;
use crate::initial::InitialWorldgenInputs;
use crate::region_centroid::EastMeetsWestCentroidReceipt;
use don_sim::rng::Random;
use don_sim::systems::map_terrain::World;
use don_sim::trig::find_angle;

pub const EAST_MEETS_WEST_START_ANGLE_RNG_VA: u32 = 0x0069_6f82;
pub const EAST_MEETS_WEST_FIRST_PLACE_START_CALL_VA: u32 = 0x0069_6fe3;
pub const MAP_PLACE_START_IN_REGION_VA: u32 = 0x0068_ac00;
const START_DISTANCE_INPUT: i32 = 12;
const STANDARD_MAP_EDGE: i32 = 70;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EastMeetsWestStartAngleDraw {
    pub call_va: u32,
    pub state_before: i32,
    pub raw: i32,
    pub state_after: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EastMeetsWestPlaceStartBoundary {
    pub caller_va: u32,
    pub primitive_va: u32,
    pub player_slot: u8,
    pub team: u8,
    pub assigned_continent: i32,
    pub selected_continent: i32,
    pub region: i32,
    pub centroid_x: i32,
    pub centroid_y: i32,
    pub base_angle: i32,
    pub continent_players: i32,
    pub angle_spacing: i32,
    pub angle_adjustment: i32,
    pub angle: i32,
    pub radius: i32,
    pub search_distance: i32,
    pub min_start_distance: i32,
    /// The shipped selector never reads this fifth explicit argument.
    pub unread_argument: i32,
    pub optional_start_arrays_are_null: bool,
    pub random_draw: Option<EastMeetsWestStartAngleDraw>,
    pub direct_rng_sites: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EastMeetsWestStartBoundaryError {
    NoActiveLeader,
    PlayerLayoutMismatch,
    InvalidTeam {
        slot: u8,
        team: u8,
    },
    InvalidAssignedContinent {
        slot: u8,
        continent: i32,
    },
    CurrentRegionNotFound {
        assigned_continent: i32,
        region: i32,
    },
    NonPositiveContinentPopulation {
        continent: i32,
        population: i32,
    },
}

/// Execute `0x00696d14..0x00696fe3` only far enough to materialize the first
/// `Map::place_start_in_region` call. This caller prefix mutates no World byte.
/// Its sole conditional side effect is the direct angle draw at `0x00696f82`
/// when the selected continent contains fewer than two players.
pub fn execute_first_place_start_boundary(
    inputs: &InitialWorldgenInputs,
    world: &World,
    partition: &TeamContinentPartitionReceipt,
    centroids: &EastMeetsWestCentroidReceipt,
    rng: &mut Random,
) -> Result<EastMeetsWestPlaceStartBoundary, EastMeetsWestStartBoundaryError> {
    if inputs.active_slots.len() != inputs.active_teams.len() {
        return Err(EastMeetsWestStartBoundaryError::PlayerLayoutMismatch);
    }
    let Some(&slot) = partition.active_slots.first() else {
        return Err(EastMeetsWestStartBoundaryError::NoActiveLeader);
    };
    let Some(position) = inputs
        .active_slots
        .iter()
        .position(|candidate| *candidate == slot)
    else {
        return Err(EastMeetsWestStartBoundaryError::PlayerLayoutMismatch);
    };
    let team = inputs.active_teams[position];
    if team > 8 {
        return Err(EastMeetsWestStartBoundaryError::InvalidTeam { slot, team });
    }

    let mapping_index = if team == 8 { slot } else { team } as usize;
    let mut assigned = partition.team_to_continent[mapping_index];
    if assigned == -1 {
        assigned = i32::from(slot);
    }
    let centroid_count = centroids.centroid_x.len();
    if assigned < 0
        || assigned as usize >= centroid_count
        || centroids.centroid_y.len() != centroid_count
    {
        return Err(EastMeetsWestStartBoundaryError::InvalidAssignedContinent {
            slot,
            continent: assigned,
        });
    }

    let assigned_index = assigned as usize;
    let assigned_region = i32::from(
        world
            .wdata(
                centroids.centroid_x[assigned_index],
                centroids.centroid_y[assigned_index],
            )
            .region,
    );
    // Ordinary teams re-find the centroid-array index by its post-rebuild
    // region word. The unteamed branch uses its mapped index directly.
    let selected = if team == 8 {
        assigned_index
    } else {
        (0..centroid_count)
            .find(|&candidate| {
                i32::from(
                    world
                        .wdata(
                            centroids.centroid_x[candidate],
                            centroids.centroid_y[candidate],
                        )
                        .region,
                ) == assigned_region
            })
            .ok_or(EastMeetsWestStartBoundaryError::CurrentRegionNotFound {
                assigned_continent: assigned,
                region: assigned_region,
            })?
    };
    let population = partition.continent_counts[selected];
    if population < 1 {
        return Err(
            EastMeetsWestStartBoundaryError::NonPositiveContinentPopulation {
                continent: selected as i32,
                population,
            },
        );
    }

    let centroid_x = centroids.centroid_x[selected];
    let centroid_y = centroids.centroid_y[selected];
    let region = i32::from(world.wdata(centroid_x, centroid_y).region);
    let base_angle = find_angle(
        centroid_x.wrapping_sub(world.xs / 2),
        centroid_y.wrapping_sub(world.ys / 2),
    );
    let angle_spacing = if population == 1 {
        0
    } else if population < 4 {
        0x2aaa_aaaa
    } else {
        0x6aaa_aaaa / population
    };

    let (angle_adjustment, random_draw, direct_rng_sites) = if population < 2 {
        let state_before = rng.state();
        let raw = rng.get(0, 0xffff);
        (
            0x0aaa_aaaa_i32.wrapping_sub(raw % 0x1555_5555),
            Some(EastMeetsWestStartAngleDraw {
                call_va: EAST_MEETS_WEST_START_ANGLE_RNG_VA,
                state_before,
                raw,
                state_after: rng.state(),
            }),
            vec![EAST_MEETS_WEST_START_ANGLE_RNG_VA],
        )
    } else {
        (
            -((population >> 1).wrapping_mul(angle_spacing)),
            None,
            Vec::new(),
        )
    };
    let radius = (world.xs.min(world.ys) / 2).wrapping_mul(3) / 4;
    let search_distance = (radius / 2).wrapping_mul(3) / 2;
    let scaled = scale_by_area(world, START_DISTANCE_INPUT);
    let min_start_distance =
        scale_land_area(scaled, inputs.active_slots.len() as u8, inputs.map_size);

    Ok(EastMeetsWestPlaceStartBoundary {
        caller_va: EAST_MEETS_WEST_FIRST_PLACE_START_CALL_VA,
        primitive_va: MAP_PLACE_START_IN_REGION_VA,
        player_slot: slot,
        team,
        assigned_continent: assigned,
        selected_continent: selected as i32,
        region,
        centroid_x,
        centroid_y,
        base_angle,
        continent_players: population,
        angle_spacing,
        angle_adjustment,
        angle: base_angle.wrapping_add(angle_adjustment),
        radius,
        search_distance,
        min_start_distance,
        unread_argument: START_DISTANCE_INPUT,
        optional_start_arrays_are_null: true,
        random_draw,
        direct_rng_sites,
    })
}

fn scale_by_area(world: &World, value: i32) -> i32 {
    if value < 1 {
        0
    } else {
        let standard_area = STANDARD_MAP_EDGE * STANDARD_MAP_EDGE;
        ((standard_area / 2).wrapping_add(world.size.wrapping_mul(value)) / standard_area).max(1)
    }
}

fn player_baseline(map_size: u8) -> i32 {
    match map_size {
        0 | 1 => 2,
        2 => 3,
        3 => 4,
        4 => 6,
        _ => 8,
    }
}

fn scale_land_area(value: i32, players: u8, map_size: u8) -> i32 {
    let baseline = player_baseline(map_size);
    let overage = if players == 0 {
        baseline
    } else {
        (i32::from(players) - baseline).max(0)
    };
    if overage == 0 {
        return value;
    }
    let ratio = overage as f32 / baseline as f32;
    (value as f32 / ratio) as i32
}
