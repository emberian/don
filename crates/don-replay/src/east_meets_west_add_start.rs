//! Exact first East Meets West `World::add_starting_location` mutation.

use don_sim::systems::map_terrain::{WCoord, WalkedArray, World, WorldSection};

pub const MAP_PLACE_START_SUCCESS_RETURN_VA: u32 = 0x0068_ae16;
pub const WORLD_ADD_STARTING_LOCATION_VA: u32 = 0x006b_2de0;
pub const WORLD_ADD_STARTING_LOCATION_RETURN_VA: u32 = 0x006b_3017;
pub const WORLD_ADD_STARTING_LOCATION_END_VA: u32 = 0x006b_301a;
pub const EAST_MEETS_WEST_ADD_START_CALL_VA: u32 = 0x0069_7461;
pub const EAST_MEETS_WEST_ADD_START_RETURN_VA: u32 = 0x0069_7466;
pub const EAST_MEETS_WEST_RECORD_START_X_VA: u32 = 0x0069_746c;
pub const EAST_MEETS_WEST_RECORD_START_Y_VA: u32 = 0x0069_7473;
pub const EAST_MEETS_WEST_START_LOOP_JUMP_VA: u32 = 0x0069_747f;
pub const EAST_MEETS_WEST_START_LOOP_HEAD_VA: u32 = 0x0069_6d28;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EastMeetsWestAddStartInput {
    pub selector_return_va: u32,
    pub caller_va: u32,
    pub primitive_va: u32,
    pub output: Option<(i32, i32)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartArrayState {
    pub length: usize,
    pub capacity: i32,
    pub increment: i16,
    pub flags: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartArraysState {
    pub start_x: StartArrayState,
    pub start_y: StartArrayState,
    pub start_city_x: StartArrayState,
    pub start_city_y: StartArrayState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartCityOccupancyWrite {
    pub x: i32,
    pub y: i32,
    pub flat_index: usize,
    pub byte_index: usize,
    pub mask: u8,
    pub byte_before: u8,
    pub byte_after: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EastMeetsWestAddStartReceipt {
    pub primitive_va: u32,
    pub return_va: u32,
    pub end_va: u32,
    pub caller_va: u32,
    pub caller_return_va: u32,
    pub caller_record_x_va: u32,
    pub caller_record_y_va: u32,
    pub caller_loop_jump_va: u32,
    pub caller_loop_head_va: u32,
    pub input: (i32, i32),
    pub returned_start_index: i32,
    pub arrays_before: StartArraysState,
    pub arrays_after: StartArraysState,
    pub appended_start: (i32, i32),
    pub appended_city_x: [i32; 4],
    pub appended_city_y: [i32; 4],
    pub occupancy_writes: [StartCityOccupancyWrite; 4],
    pub full_checksum_before: u32,
    pub full_checksum_after: u32,
    pub start_arrays_checksum_before: u32,
    pub start_arrays_checksum_after: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EastMeetsWestAddStartError {
    SelectorDidNotReachCall {
        selector_return_va: u32,
        caller_va: u32,
        primitive_va: u32,
        output: Option<(i32, i32)>,
    },
    StartArrayLengthMismatch {
        start_x: usize,
        start_y: usize,
        start_city_x: usize,
        start_city_y: usize,
    },
    InvalidArrayMetadata {
        array: &'static str,
        length: usize,
        capacity: i32,
        increment: i16,
        additions: usize,
    },
    InvalidCoordinate {
        x: i32,
        y: i32,
        world_xs: i32,
        world_ys: i32,
    },
    StartCityBitPlaneShort {
        required_bytes: usize,
        available_bytes: usize,
    },
    RetailPortMismatch {
        field: &'static str,
    },
    UnexpectedWalkedMutation {
        section: WorldSection,
    },
}

/// Execute the exact first start append reached by the selector's success edge.
///
/// Validation is completed against a cloned World before the caller-visible
/// state is committed. Retail itself performs no coordinate or storage checks;
/// these guards refuse malformed replay state instead of exposing a partial
/// Rust mutation that the shipped caller domain cannot produce.
pub fn execute_first_add_start(
    world: &mut World,
    input: EastMeetsWestAddStartInput,
) -> Result<EastMeetsWestAddStartReceipt, EastMeetsWestAddStartError> {
    let Some((x, y)) = input.output else {
        return Err(selector_edge_error(input));
    };
    if input.selector_return_va != MAP_PLACE_START_SUCCESS_RETURN_VA
        || input.caller_va != EAST_MEETS_WEST_ADD_START_CALL_VA
        || input.primitive_va != WORLD_ADD_STARTING_LOCATION_VA
    {
        return Err(selector_edge_error(input));
    }

    let arrays_before = arrays_state(world);
    let start_count = world.start_x.items.len();
    let city_count =
        start_count
            .checked_mul(4)
            .ok_or(EastMeetsWestAddStartError::StartArrayLengthMismatch {
                start_x: world.start_x.items.len(),
                start_y: world.start_y.items.len(),
                start_city_x: world.start_city_x.items.len(),
                start_city_y: world.start_city_y.items.len(),
            })?;
    if world.start_y.items.len() != start_count
        || world.start_city_x.items.len() != city_count
        || world.start_city_y.items.len() != city_count
    {
        return Err(EastMeetsWestAddStartError::StartArrayLengthMismatch {
            start_x: world.start_x.items.len(),
            start_y: world.start_y.items.len(),
            start_city_x: world.start_city_x.items.len(),
            start_city_y: world.start_city_y.items.len(),
        });
    }
    validate_growth("start_x", &world.start_x, 1)?;
    validate_growth("start_y", &world.start_y, 1)?;
    validate_growth("start_city_x", &world.start_city_x, 4)?;
    validate_growth("start_city_y", &world.start_city_y, 4)?;
    if x < 1 || y < 1 || x >= world.xs || y >= world.ys {
        return Err(EastMeetsWestAddStartError::InvalidCoordinate {
            x,
            y,
            world_xs: world.xs,
            world_ys: world.ys,
        });
    }

    let city_coords = [(x, y), (x - 1, y), (x, y - 1), (x - 1, y - 1)];
    let mut expected_bits = world.start_city_locs.clone();
    let mut occupancy = Vec::with_capacity(4);
    for (wx, wy) in city_coords {
        let flat = usize::try_from(wy)
            .ok()
            .and_then(|row| row.checked_mul(world.xs as usize))
            .and_then(|row| row.checked_add(wx as usize))
            .ok_or(EastMeetsWestAddStartError::InvalidCoordinate {
                x,
                y,
                world_xs: world.xs,
                world_ys: world.ys,
            })?;
        let byte_index = flat >> 3;
        let available_bytes = expected_bits.len();
        let Some(byte) = expected_bits.get_mut(byte_index) else {
            return Err(EastMeetsWestAddStartError::StartCityBitPlaneShort {
                required_bytes: byte_index + 1,
                available_bytes,
            });
        };
        let mask = 1u8 << (flat & 7);
        let byte_before = *byte;
        *byte |= mask;
        occupancy.push(StartCityOccupancyWrite {
            x: wx,
            y: wy,
            flat_index: flat,
            byte_index,
            mask,
            byte_before,
            byte_after: *byte,
        });
    }

    let checksum_before = world.checksum_sections();
    let mut candidate = world.clone();
    let returned_start_index = candidate.add_starting_location(WCoord(x), WCoord(y));
    if returned_start_index != start_count as i32 {
        return Err(EastMeetsWestAddStartError::RetailPortMismatch {
            field: "returned_start_index",
        });
    }
    if candidate.start_city_locs != expected_bits {
        return Err(EastMeetsWestAddStartError::RetailPortMismatch {
            field: "start_city_locs",
        });
    }
    let start_range = start_count..start_count + 1;
    let city_range = city_count..city_count + 4;
    if candidate.start_x.items[start_range.clone()] != [x]
        || candidate.start_y.items[start_range] != [y]
        || candidate.start_city_x.items[city_range.clone()] != [x, x - 1, x, x - 1]
        || candidate.start_city_y.items[city_range] != [y, y, y - 1, y - 1]
    {
        return Err(EastMeetsWestAddStartError::RetailPortMismatch {
            field: "appended_coordinates",
        });
    }
    let checksum_after = candidate.checksum_sections();
    for section in WorldSection::all() {
        if section != WorldSection::StartArrays
            && checksum_before.section(section) != checksum_after.section(section)
        {
            return Err(EastMeetsWestAddStartError::UnexpectedWalkedMutation { section });
        }
    }
    let arrays_after = arrays_state(&candidate);
    *world = candidate;

    Ok(EastMeetsWestAddStartReceipt {
        primitive_va: WORLD_ADD_STARTING_LOCATION_VA,
        return_va: WORLD_ADD_STARTING_LOCATION_RETURN_VA,
        end_va: WORLD_ADD_STARTING_LOCATION_END_VA,
        caller_va: EAST_MEETS_WEST_ADD_START_CALL_VA,
        caller_return_va: EAST_MEETS_WEST_ADD_START_RETURN_VA,
        caller_record_x_va: EAST_MEETS_WEST_RECORD_START_X_VA,
        caller_record_y_va: EAST_MEETS_WEST_RECORD_START_Y_VA,
        caller_loop_jump_va: EAST_MEETS_WEST_START_LOOP_JUMP_VA,
        caller_loop_head_va: EAST_MEETS_WEST_START_LOOP_HEAD_VA,
        input: (x, y),
        returned_start_index,
        arrays_before,
        arrays_after,
        appended_start: (x, y),
        appended_city_x: [x, x - 1, x, x - 1],
        appended_city_y: [y, y, y - 1, y - 1],
        occupancy_writes: occupancy.try_into().expect("four exact footprint writes"),
        full_checksum_before: checksum_before.full,
        full_checksum_after: checksum_after.full,
        start_arrays_checksum_before: checksum_before.section(WorldSection::StartArrays).adler,
        start_arrays_checksum_after: checksum_after.section(WorldSection::StartArrays).adler,
    })
}

fn selector_edge_error(input: EastMeetsWestAddStartInput) -> EastMeetsWestAddStartError {
    EastMeetsWestAddStartError::SelectorDidNotReachCall {
        selector_return_va: input.selector_return_va,
        caller_va: input.caller_va,
        primitive_va: input.primitive_va,
        output: input.output,
    }
}

fn array_state(array: &WalkedArray<i32>) -> StartArrayState {
    StartArrayState {
        length: array.items.len(),
        capacity: array.capacity,
        increment: array.increment,
        flags: array.flags,
    }
}

fn arrays_state(world: &World) -> StartArraysState {
    StartArraysState {
        start_x: array_state(&world.start_x),
        start_y: array_state(&world.start_y),
        start_city_x: array_state(&world.start_city_x),
        start_city_y: array_state(&world.start_city_y),
    }
}

fn validate_growth(
    name: &'static str,
    array: &WalkedArray<i32>,
    additions: usize,
) -> Result<(), EastMeetsWestAddStartError> {
    let Ok(mut capacity) = usize::try_from(array.capacity) else {
        return Err(invalid_metadata(name, array, additions));
    };
    if array.items.len() > capacity {
        return Err(invalid_metadata(name, array, additions));
    }
    let Some(target) = array.items.len().checked_add(additions) else {
        return Err(invalid_metadata(name, array, additions));
    };
    while capacity < target {
        let increase = if array.increment < 0 {
            if capacity == 0 {
                4
            } else {
                capacity
            }
        } else {
            array.increment as usize
        };
        if increase == 0 {
            return Err(invalid_metadata(name, array, additions));
        }
        capacity = capacity
            .checked_add(increase)
            .ok_or_else(|| invalid_metadata(name, array, additions))?;
    }
    Ok(())
}

fn invalid_metadata(
    array_name: &'static str,
    array: &WalkedArray<i32>,
    additions: usize,
) -> EastMeetsWestAddStartError {
    EastMeetsWestAddStartError::InvalidArrayMetadata {
        array: array_name,
        length: array.items.len(),
        capacity: array.capacity,
        increment: array.increment,
        additions,
    }
}
