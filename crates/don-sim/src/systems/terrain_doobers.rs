//! Exact deterministic body of `TerrainGroups::add_doobers`.
//!
//! Provenance is the PDB `TileSetGroupData` layout, the retail body at
//! `0x006a1540`, the literal `"bush"` at `0x00ade2b4`, and the retail `move_x`
//! / `move_y` tables at `0x00adcaf0` / `0x00adc400`.
//!
//! The first pass is complete for the supported direction-table domain. It
//! scans interior world cells X-major/Y-inner, considers a forest's south edge
//! before its east edge, and emits the exact `Doober::add_doober` coordinates.
//! The second pass checks mountain fringes west before north and emits the same
//! retail `"bush"` doober type. `Doober::has_doobers` at `0x008466d0` contains a
//! shipped comparison bug which makes it return zero for every registry state;
//! that proof closes the former occupancy boundary without inventing host state.

use super::map_terrain::{div_3, land, wflag, World};
use crate::rng::Random;

const MOVE_X: [i32; 8] = [0, -1, 0, 1, 1, 1, 0, -1];
const MOVE_Y: [i32; 8] = [0, -1, -1, -1, 0, 1, 1, 1];

/// The eight `TileSetGroupData` fields at offsets `+32` through `+60`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct DooberTilesetRules {
    pub bush_clump_prob: i32,
    pub bush_spacing: i32,
    pub bush_min: i32,
    pub bush_max: i32,
    pub mountain_rock_clump_prob: i32,
    pub mountain_rock_spacing: i32,
    pub mountain_rock_min: i32,
    pub mountain_rock_max: i32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ForestFringeEdge {
    South,
    East,
}

/// One exact call to `Doober::add_doober("bush", Coord, Coord)`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct BushDooberPlacement {
    pub forest_x: i32,
    pub forest_y: i32,
    pub edge: ForestFringeEdge,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BushFringeReceipt {
    pub placements: Vec<BushDooberPlacement>,
    pub chance_draws: u32,
    pub offset_draws: u32,
    pub rng_state_after: i32,
}

/// One `TexCoord` row from `Doober::doober_locs` (`Doober + 0x58`).
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct DooberLocation {
    pub x: f32,
    pub y: f32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MountainRockNeighbor {
    West,
    North,
}

/// One second-pass `Doober::add_doober("bush", Coord, Coord)` call.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MountainRockDooberPlacement {
    pub world_x: i32,
    pub world_y: i32,
    pub mountain_neighbor: MountainRockNeighbor,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountainRockFringeReceipt {
    pub placements: Vec<MountainRockDooberPlacement>,
    /// Calls to the shipped `Doober::has_doobers` body. They are pure and always
    /// return zero, but their position before the river test is instruction-pinned.
    pub occupancy_queries: u32,
    pub chance_draws: u32,
    pub tile_offset_draws: u32,
    pub rng_state_after: i32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BushFringeError {
    InvalidWorldShape {
        xs: i32,
        ys: i32,
        size: i32,
        wdata_len: usize,
    },
    InvalidBushCountSpan {
        bush_min: i32,
        bush_max: i32,
    },
    /// The recovered compass prefix supports all retail accesses for positive
    /// clump counts through five; larger data needs the remainder of the global
    /// movement table captured before it can be executed safely.
    UnsupportedBushCount {
        maximum_positive_count: i64,
        supported: i32,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MountainRockFringeError {
    InvalidWorldShape {
        xs: i32,
        ys: i32,
        size: i32,
        wdata_len: usize,
    },
    InvalidMountainRockCountSpan {
        mountain_rock_min: i32,
        mountain_rock_max: i32,
    },
    UnsupportedMountainRockCount {
        maximum_positive_count: i64,
        supported: i32,
    },
}

/// Complete first pass of `TerrainGroups::add_doobers` (`0x006a15ae` through
/// `0x006a19c7`) with external add calls returned as a receipt.
pub fn plan_bush_fringe(
    world: &World,
    rules: DooberTilesetRules,
    random: &mut Random,
) -> Result<BushFringeReceipt, BushFringeError> {
    plan_bush_fringe_with_host(world, rules, random, |_| {})
}

/// Same exact pass with each external `Doober::add_doober` surfaced in native
/// call order.  Validation happens before the first RNG draw or host callback.
pub fn plan_bush_fringe_with_host(
    world: &World,
    rules: DooberTilesetRules,
    random: &mut Random,
    mut add_doober: impl FnMut(BushDooberPlacement),
) -> Result<BushFringeReceipt, BushFringeError> {
    validate_bush_fringe_inputs(world, rules)?;
    let count_span = validate_count_domain(rules)?;

    let mut receipt = BushFringeReceipt {
        placements: Vec::new(),
        chance_draws: 0,
        offset_draws: 0,
        rng_state_after: random.state(),
    };
    if world.xs <= 2 || world.ys <= 2 {
        return Ok(receipt);
    }

    for forest_x in 1..world.xs - 1 {
        for forest_y in 1..world.ys - 1 {
            let flags = world.wdata(forest_x, forest_y).flags;
            if flags & wflag::FOREST == 0 || flags & wflag::HAS_RIVER != 0 {
                continue;
            }

            let south_edge = world.wdata(forest_x, forest_y + 1).flags & wflag::FOREST == 0;
            let east_edge = world.wdata(forest_x + 1, forest_y).flags & wflag::FOREST == 0;
            if south_edge {
                consider_edge(
                    &mut receipt,
                    &mut add_doober,
                    random,
                    rules,
                    count_span,
                    forest_x,
                    forest_y,
                    ForestFringeEdge::South,
                );
            }
            if east_edge {
                consider_edge(
                    &mut receipt,
                    &mut add_doober,
                    random,
                    rules,
                    count_span,
                    forest_x,
                    forest_y,
                    ForestFringeEdge::East,
                );
            }
        }
    }
    receipt.rng_state_after = random.state();
    Ok(receipt)
}

/// Complete shipped body of `Doober::has_doobers` (`0x008466d0`).
///
/// Retail correctly maps `location.x` through `div_3_table[(int)x >> 8]`, but
/// the Y expression compares the float to `world_y` first, converts that boolean
/// to float and back to integer, shifts it by eight, and indexes `div_3_table`.
/// The index is therefore always zero and `div_3_table[0] == 0`, so the native
/// function cannot take its `return 1` arm. The registry arguments remain in the
/// API to make the recovered, mutation-invariant behavior explicit.
pub fn has_doobers(locations: &[DooberLocation], world_x: i32, world_y: i32) -> bool {
    for location in locations {
        let location_world_x = div_3((location.x as i32) >> 8);
        if location_world_x != world_x {
            continue;
        }
        let y_equal = location.y == world_y as f32;
        let erroneous_y_index = (y_equal as i32) >> 8;
        if div_3(erroneous_y_index) != 0 {
            return true;
        }
    }
    false
}

/// Complete second (mountain-rock fringe) pass of `TerrainGroups::add_doobers`
/// (`0x006a19db` through `0x006a1c9a`).
pub fn plan_mountain_rock_fringe(
    world: &World,
    rules: DooberTilesetRules,
    random: &mut Random,
) -> Result<MountainRockFringeReceipt, MountainRockFringeError> {
    plan_mountain_rock_fringe_with_host(world, rules, random, |_| {})
}

/// Same exact pass with each external add call surfaced in native order.
/// Validation completes before its first RNG draw or host callback.
pub fn plan_mountain_rock_fringe_with_host(
    world: &World,
    rules: DooberTilesetRules,
    random: &mut Random,
    mut add_doober: impl FnMut(MountainRockDooberPlacement),
) -> Result<MountainRockFringeReceipt, MountainRockFringeError> {
    validate_mountain_rock_fringe_inputs(world, rules)?;
    let count_span = validate_mountain_count_domain(rules)?;
    let mut receipt = MountainRockFringeReceipt {
        placements: Vec::new(),
        occupancy_queries: 0,
        chance_draws: 0,
        tile_offset_draws: 0,
        rng_state_after: random.state(),
    };
    if world.xs <= 2 || world.ys <= 2 {
        return Ok(receipt);
    }

    for world_x in 1..world.xs - 1 {
        for world_y in 1..world.ys - 1 {
            let cell = world.wdata(world_x, world_y);
            if cell.flags & wflag::WATERHALF == 0
                && (cell.land == land::COASTAL || cell.land == land::OCEAN)
            {
                continue;
            }
            if cell.flags & (wflag::ROCKS | wflag::MOUNTAINS | wflag::FOREST) != 0 {
                continue;
            }

            receipt.occupancy_queries += 1;
            if has_doobers(&[], world_x, world_y) || cell.flags & wflag::HAS_RIVER != 0 {
                continue;
            }

            let mountain_neighbor =
                if world.wdata(world_x - 1, world_y).flags & wflag::MOUNTAINS != 0 {
                    Some(MountainRockNeighbor::West)
                } else if world.wdata(world_x, world_y - 1).flags & wflag::MOUNTAINS != 0 {
                    Some(MountainRockNeighbor::North)
                } else {
                    None
                };
            let Some(mountain_neighbor) = mountain_neighbor else {
                continue;
            };

            receipt.chance_draws += 1;
            if random.get(0, 0xffff) % 100 >= rules.mountain_rock_clump_prob {
                continue;
            }
            receipt.tile_offset_draws += 1;
            let offset = random.get(0, 0xffff) % 16;
            let tile_x = world_x.wrapping_mul(4).wrapping_add(offset % 4);
            let tile_y = world_y.wrapping_mul(4).wrapping_add(offset / 4);
            append_mountain_rock_clump(
                &mut receipt,
                &mut add_doober,
                rules,
                count_span,
                world_x,
                world_y,
                mountain_neighbor,
                tile_x,
                tile_y,
            );
        }
    }
    receipt.rng_state_after = random.state();
    Ok(receipt)
}

#[allow(clippy::too_many_arguments)]
fn append_mountain_rock_clump(
    receipt: &mut MountainRockFringeReceipt,
    add_doober: &mut impl FnMut(MountainRockDooberPlacement),
    rules: DooberTilesetRules,
    count_span: i32,
    world_x: i32,
    world_y: i32,
    mountain_neighbor: MountainRockNeighbor,
    tile_x: i32,
    tile_y: i32,
) {
    let hash = tile_y.wrapping_mul(tile_x);
    let count_hash = hash.wrapping_mul(tile_x);
    let count = (count_hash % count_span).wrapping_add(rules.mountain_rock_min);
    if count <= 0 {
        return;
    }

    let base_x = ((hash as u32 & 0x7f) as i32).wrapping_sub(0x3f);
    let direction_x = (base_x as u32 & 3) as usize;
    let tile_sum = tile_y.wrapping_add(tile_x);
    let base_y = ((tile_sum as u32 & 0x7f) as i32).wrapping_sub(0x3f);
    let direction_y = (base_y as u32 & 3) as usize;
    let anchor_x = tile_x.wrapping_mul(0xc0);
    let anchor_y = tile_y.wrapping_mul(0xc0);

    for index in 0..count as usize {
        let x_jitter = rules
            .mountain_rock_spacing
            .wrapping_add((hash.wrapping_add(index as i32) & 3).wrapping_mul(0x28));
        let y_jitter = rules
            .mountain_rock_spacing
            .wrapping_add((tile_sum.wrapping_add(index as i32) & 3).wrapping_mul(0x28));
        let placement = MountainRockDooberPlacement {
            world_x,
            world_y,
            mountain_neighbor,
            x: base_x
                .wrapping_add(anchor_x)
                .wrapping_add(0x60)
                .wrapping_add(x_jitter.wrapping_mul(MOVE_X[direction_x + index])),
            y: y_jitter
                .wrapping_mul(MOVE_Y[direction_y + index])
                .wrapping_add(base_y)
                .wrapping_add(anchor_y)
                .wrapping_add(0x60),
        };
        receipt.placements.push(placement);
        add_doober(placement);
    }
}

#[allow(clippy::too_many_arguments)]
fn consider_edge(
    receipt: &mut BushFringeReceipt,
    add_doober: &mut impl FnMut(BushDooberPlacement),
    random: &mut Random,
    rules: DooberTilesetRules,
    count_span: i32,
    forest_x: i32,
    forest_y: i32,
    edge: ForestFringeEdge,
) {
    receipt.chance_draws += 1;
    if random.get(0, 0xffff) % 100 >= rules.bush_clump_prob {
        return;
    }

    receipt.offset_draws += 1;
    let offset = random.get(0, 0xffff) % 4;
    let (tile_x, tile_y) = match edge {
        ForestFringeEdge::South => (
            forest_x.wrapping_mul(4).wrapping_add(offset),
            forest_y.wrapping_mul(4).wrapping_add(3),
        ),
        ForestFringeEdge::East => (
            forest_x.wrapping_mul(4).wrapping_add(3),
            forest_y.wrapping_mul(4).wrapping_add(offset),
        ),
    };
    let hash = tile_y.wrapping_mul(tile_x);
    let count_hash = hash.wrapping_mul(tile_x);
    let count = (count_hash % count_span).wrapping_add(rules.bush_min);
    if count <= 0 {
        return;
    }

    let base_x = ((hash as u32 & 0x7f) as i32).wrapping_sub(0x3f);
    let direction_x = (base_x as u32 & 3) as usize;
    let tile_sum = tile_y.wrapping_add(tile_x);
    let base_y = ((tile_sum as u32 & 0x7f) as i32).wrapping_sub(0x3f);
    let direction_y = (base_y as u32 & 3) as usize;
    let anchor_x = tile_x.wrapping_mul(0xc0);
    let anchor_y = tile_y.wrapping_mul(0xc0);

    for index in 0..count as usize {
        let x_jitter = rules
            .bush_spacing
            .wrapping_add((hash.wrapping_add(index as i32) & 3).wrapping_mul(0x28));
        let y_jitter = rules
            .bush_spacing
            .wrapping_add((tile_sum.wrapping_add(index as i32) & 3).wrapping_mul(0x28));
        let placement = BushDooberPlacement {
            forest_x,
            forest_y,
            edge,
            x: base_x
                .wrapping_add(anchor_x)
                .wrapping_add(0x60)
                .wrapping_add(x_jitter.wrapping_mul(MOVE_X[direction_x + index])),
            y: y_jitter
                .wrapping_mul(MOVE_Y[direction_y + index])
                .wrapping_add(base_y)
                .wrapping_add(anchor_y)
                .wrapping_add(0x60),
        };
        receipt.placements.push(placement);
        add_doober(placement);
    }
}

fn validate_world(world: &World) -> Result<(), BushFringeError> {
    let expected = world.xs.checked_mul(world.ys);
    if world.xs < 0
        || world.ys < 0
        || expected != Some(world.size)
        || usize::try_from(world.size).ok() != Some(world.wdata.len())
    {
        return Err(BushFringeError::InvalidWorldShape {
            xs: world.xs,
            ys: world.ys,
            size: world.size,
            wdata_len: world.wdata.len(),
        });
    }
    Ok(())
}

/// Shared composition preflight. This is visible only to sibling worldgen
/// systems so all artificial fail-closed checks can precede native host calls.
pub(super) fn validate_bush_fringe_inputs(
    world: &World,
    rules: DooberTilesetRules,
) -> Result<(), BushFringeError> {
    validate_world(world)?;
    validate_count_domain(rules)?;
    Ok(())
}

fn validate_mountain_world(world: &World) -> Result<(), MountainRockFringeError> {
    let expected = world.xs.checked_mul(world.ys);
    if world.xs < 0
        || world.ys < 0
        || expected != Some(world.size)
        || usize::try_from(world.size).ok() != Some(world.wdata.len())
    {
        return Err(MountainRockFringeError::InvalidWorldShape {
            xs: world.xs,
            ys: world.ys,
            size: world.size,
            wdata_len: world.wdata.len(),
        });
    }
    Ok(())
}

pub(super) fn validate_mountain_rock_fringe_inputs(
    world: &World,
    rules: DooberTilesetRules,
) -> Result<(), MountainRockFringeError> {
    validate_mountain_world(world)?;
    validate_mountain_count_domain(rules)?;
    Ok(())
}

fn validate_count_domain(rules: DooberTilesetRules) -> Result<i32, BushFringeError> {
    let distance = (i64::from(rules.bush_max) - i64::from(rules.bush_min)).abs();
    let span = distance + 1;
    if span > i64::from(i32::MAX) {
        return Err(BushFringeError::InvalidBushCountSpan {
            bush_min: rules.bush_min,
            bush_max: rules.bush_max,
        });
    }
    let maximum_positive_count = i64::from(rules.bush_min) + distance;
    if maximum_positive_count > 5 {
        return Err(BushFringeError::UnsupportedBushCount {
            maximum_positive_count,
            supported: 5,
        });
    }
    Ok(span as i32)
}

fn validate_mountain_count_domain(
    rules: DooberTilesetRules,
) -> Result<i32, MountainRockFringeError> {
    let distance = (i64::from(rules.mountain_rock_max) - i64::from(rules.mountain_rock_min)).abs();
    let span = distance + 1;
    if span > i64::from(i32::MAX) {
        return Err(MountainRockFringeError::InvalidMountainRockCountSpan {
            mountain_rock_min: rules.mountain_rock_min,
            mountain_rock_max: rules.mountain_rock_max,
        });
    }
    let maximum_positive_count = i64::from(rules.mountain_rock_min) + distance;
    if maximum_positive_count > 5 {
        return Err(MountainRockFringeError::UnsupportedMountainRockCount {
            maximum_positive_count,
            supported: 5,
        });
    }
    Ok(span as i32)
}
