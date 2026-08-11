//! Exact first `Map::place_start_in_region` selector and immediate caller edge.

use don_sim::rng::Random;
use don_sim::systems::combat::{circle_table, vector_dist, CircleTable};
use don_sim::systems::map_terrain::{place_start_in_region, WCoord, World, WorldSection};
use don_sim::systems::regions::Regions;

pub const MAP_PLACE_START_IN_REGION_VA: u32 = 0x0068_ac00;
pub const MAP_PLACE_START_RNG_CALL_VA: u32 = 0x0068_ac8b;
pub const MAP_PLACE_START_SUCCESS_RETURN_VA: u32 = 0x0068_ae16;
pub const MAP_PLACE_START_FAILURE_RETURN_VA: u32 = 0x0068_ae49;
pub const MAP_PLACE_START_END_VA: u32 = 0x0068_ae4c;
pub const EAST_MEETS_WEST_FIRST_PLACE_START_CALL_VA: u32 = 0x0069_6fe3;
pub const EAST_MEETS_WEST_PLACE_START_TEST_VA: u32 = 0x0069_6fe8;
pub const EAST_MEETS_WEST_PLACE_START_FAILURE_VA: u32 = 0x0069_7003;
pub const EAST_MEETS_WEST_ADD_START_CALL_VA: u32 = 0x0069_7461;
pub const WORLD_ADD_STARTING_LOCATION_VA: u32 = 0x006b_2de0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceStartSelectorDraw {
    pub pass: u8,
    pub call_va: u32,
    pub state_before: i32,
    pub raw: i32,
    pub anchor: i32,
    pub state_after: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaceStartSelectorNext {
    /// EAX=1; caller copies both output words and reaches the exact next World
    /// mutator without another RNG call or walked-state write.
    AddStartingLocation { caller_va: u32, primitive_va: u32 },
    /// EAX=0; caller enters its unported style-specific fallback search.
    CallerFallback { next_va: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceStartSelectorReceipt {
    pub primitive_va: u32,
    pub caller_va: u32,
    pub caller_test_va: u32,
    pub return_va: u32,
    pub end_va: u32,
    pub region: i32,
    pub region_size: i32,
    pub min_distance: i32,
    pub unread_argument: i32,
    pub used_world_start_arrays: bool,
    pub existing_starts: usize,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub draws: Vec<PlaceStartSelectorDraw>,
    pub output: Option<(i32, i32)>,
    pub accepted_pass: Option<u8>,
    pub full_checksum_before: u32,
    pub full_checksum_after: u32,
    pub wdata_checksum_before: u32,
    pub wdata_checksum_after: u32,
    pub next: PlaceStartSelectorNext,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaceStartSelectorError {
    InvalidRegion {
        region: i32,
    },
    NonPositiveRegionSize {
        region: i32,
        size: i32,
    },
    CoordinateStorageShort {
        region: i32,
        size: i32,
        coordinates: usize,
    },
    ExistingStartArrayMismatch {
        x: usize,
        y: usize,
    },
    RngReceiptMismatch {
        state_before: i32,
        state_after: i32,
    },
}

/// Execute the exact selector for East Meets West's first call domain.
///
/// The caller passes null optional arrays, so the selector reads World's own
/// start arrays. All fail-closed shape checks precede the first RNG draw.
pub fn execute_first_place_start_selector(
    world: &World,
    regions: &Regions,
    rng: &mut Random,
    region: i32,
    min_distance: i32,
    unread_argument: i32,
) -> Result<PlaceStartSelectorReceipt, PlaceStartSelectorError> {
    let Some(row) = usize::try_from(region)
        .ok()
        .and_then(|index| regions.list.get(index))
    else {
        return Err(PlaceStartSelectorError::InvalidRegion { region });
    };
    if row.size < 1 {
        return Err(PlaceStartSelectorError::NonPositiveRegionSize {
            region,
            size: row.size,
        });
    }
    if row.coords.items.len() < row.size as usize {
        return Err(PlaceStartSelectorError::CoordinateStorageShort {
            region,
            size: row.size,
            coordinates: row.coords.items.len(),
        });
    }
    if world.start_x.items.len() != world.start_y.items.len() {
        return Err(PlaceStartSelectorError::ExistingStartArrayMismatch {
            x: world.start_x.items.len(),
            y: world.start_y.items.len(),
        });
    }

    let coords = row.coords.items[..row.size as usize]
        .iter()
        .map(|&(x, y)| (WCoord(x), WCoord(y)))
        .collect::<Vec<_>>();
    let before = world.checksum_sections();
    let random_state_before = rng.state();
    let circle = circle_table();
    let output = place_start_in_region(
        world,
        &circle,
        rng,
        &coords,
        min_distance,
        unread_argument,
        None,
    )
    .map(|(x, y)| (x.0, y.0));
    let random_state_after = rng.state();
    let draws = recover_draws(random_state_before, random_state_after, row.size)?;
    let accepted_pass = output.map(|coord| {
        if row.size > 1 {
            draws.len() as u8
        } else if valid_on_pass(world, &circle, coord, min_distance, 1) {
            1
        } else {
            2
        }
    });
    let after = world.checksum_sections();
    let (return_va, next) = if output.is_some() {
        (
            MAP_PLACE_START_SUCCESS_RETURN_VA,
            PlaceStartSelectorNext::AddStartingLocation {
                caller_va: EAST_MEETS_WEST_ADD_START_CALL_VA,
                primitive_va: WORLD_ADD_STARTING_LOCATION_VA,
            },
        )
    } else {
        (
            MAP_PLACE_START_FAILURE_RETURN_VA,
            PlaceStartSelectorNext::CallerFallback {
                next_va: EAST_MEETS_WEST_PLACE_START_FAILURE_VA,
            },
        )
    };

    Ok(PlaceStartSelectorReceipt {
        primitive_va: MAP_PLACE_START_IN_REGION_VA,
        caller_va: EAST_MEETS_WEST_FIRST_PLACE_START_CALL_VA,
        caller_test_va: EAST_MEETS_WEST_PLACE_START_TEST_VA,
        return_va,
        end_va: MAP_PLACE_START_END_VA,
        region,
        region_size: row.size,
        min_distance,
        unread_argument,
        used_world_start_arrays: true,
        existing_starts: world.start_x.items.len(),
        random_state_before,
        random_state_after,
        draws,
        output,
        accepted_pass,
        full_checksum_before: before.full,
        full_checksum_after: after.full,
        wdata_checksum_before: before.section(WorldSection::WData).adler,
        wdata_checksum_after: after.section(WorldSection::WData).adler,
        next,
    })
}

fn valid_on_pass(
    world: &World,
    circle: &CircleTable,
    (x, y): (i32, i32),
    min_distance: i32,
    pass: u8,
) -> bool {
    let (low_margin, high_margin, inner_dry, outer_ocean, separation) = if pass == 1 {
        (5, 6, 3, 9, min_distance)
    } else {
        (3, 4, 2, 0, min_distance.min(6))
    };
    x >= low_margin
        && y >= low_margin
        && x <= world.xs.wrapping_sub(high_margin)
        && y <= world.ys.wrapping_sub(high_margin)
        && world
            .start_x
            .items
            .iter()
            .zip(&world.start_y.items)
            .all(|(&sx, &sy)| vector_dist(x.wrapping_sub(sx), y.wrapping_sub(sy)) >= separation)
        && world.is_near_ocean(circle, WCoord(x), WCoord(y), inner_dry, outer_ocean)
}

fn recover_draws(
    state_before: i32,
    state_after: i32,
    region_size: i32,
) -> Result<Vec<PlaceStartSelectorDraw>, PlaceStartSelectorError> {
    if region_size <= 1 {
        if state_after == state_before {
            return Ok(Vec::new());
        }
        return Err(PlaceStartSelectorError::RngReceiptMismatch {
            state_before,
            state_after,
        });
    }
    let mut replay = Random::new(state_before);
    let mut draws = Vec::with_capacity(2);
    for pass in 1..=2 {
        let draw_before = replay.state();
        let raw = replay.get(0, 0xffff);
        draws.push(PlaceStartSelectorDraw {
            pass,
            call_va: MAP_PLACE_START_RNG_CALL_VA,
            state_before: draw_before,
            raw,
            anchor: raw % region_size,
            state_after: replay.state(),
        });
        if replay.state() == state_after {
            return Ok(draws);
        }
    }
    Err(PlaceStartSelectorError::RngReceiptMismatch {
        state_before,
        state_after,
    })
}
