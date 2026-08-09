//! Exact deterministic bush-fringe prefix of `TerrainGroups::add_doobers`.
//!
//! Provenance is the PDB `TileSetGroupData` layout, the retail body at
//! `0x006a1540`, the literal `"bush"` at `0x00ade2b4`, and the retail `move_x`
//! / `move_y` tables at `0x00adcaf0` / `0x00adc400`.
//!
//! The first pass is complete for the supported direction-table domain.  It
//! scans interior world cells X-major/Y-inner, considers a forest's south edge
//! before its east edge, and emits the exact `Doober::add_doober` coordinates.
//! The next pass begins with the gameplay-external `Doober::has_doobers` query at
//! `0x008466d0`; callers must supply that registry before rock-fringe placement.

use super::map_terrain::{wflag, World};
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
    validate_world(world)?;
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
