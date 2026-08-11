//! Exact transactional reconstruction of `Map::eliminate_edge_canals`.

use don_sim::systems::map_terrain::{land, World, WorldSection};
use don_sim::systems::regions::{RegionBuildReceipt, Regions, RegionsError};

pub const MAP_ELIMINATE_EDGE_CANALS_VA: u32 = 0x0068_b360;
pub const MAP_ELIMINATE_EDGE_CANALS_RETURN_VA: u32 = 0x0068_b886;
pub const EAST_MEETS_WEST_EDGE_CANALS_CALL_VA: u32 = 0x0069_6d0f;
pub const EAST_MEETS_WEST_AFTER_EDGE_CANALS_VA: u32 = 0x0069_6d14;
pub const EAST_MEETS_WEST_FIRST_PLACE_START_CALL_VA: u32 = 0x0069_6fe3;
pub const MAP_PLACE_START_IN_REGION_VA: u32 = 0x0068_ac00;
pub const MAX_RETAIL_EDGE: i32 = 100;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EdgeCanalSide {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgeCanalWrite {
    pub side: EdgeCanalSide,
    /// One-based coordinate along the selected edge; endpoints are never
    /// candidate middles.
    pub middle: i32,
    /// One for the first cell inside the edge, two for the next cell.
    pub depth: i32,
    pub x: i32,
    pub y: i32,
    pub prior_land: i8,
    pub prior_land_sub: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgeCanalPassReceipt {
    pub side: EdgeCanalSide,
    /// Snapshot made at the start of this side's pass. Retail compares only
    /// the raw `land` byte to `OCEAN`, not `WorldData::is_ocean`.
    pub outer_ocean: Vec<bool>,
    /// The first inward row/column after earlier-middle writes have been
    /// reflected in the local byte array.
    pub first_inward_ocean_final: Vec<bool>,
    pub writes: Vec<EdgeCanalWrite>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EliminateEdgeCanalsReceipt {
    pub primitive_va: u32,
    pub return_va: u32,
    pub caller_va: u32,
    pub edge: i32,
    pub passes: Vec<EdgeCanalPassReceipt>,
    /// This body has no call or inline site that advances the main `Random`.
    pub direct_rng_sites: Vec<u32>,
    pub region_rebuild: RegionBuildReceipt,
    pub full_checksum_before: u32,
    pub full_checksum_after: u32,
    pub wdata_checksum_before: u32,
    pub wdata_checksum_after: u32,
    pub next_caller_va: u32,
    pub next_external_va: u32,
}

impl EliminateEdgeCanalsReceipt {
    pub fn canal_writes(&self) -> usize {
        self.passes.iter().map(|pass| pass.writes.len()).sum()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EliminateEdgeCanalsError {
    InvalidWorldShape {
        xs: i32,
        ys: i32,
        size: i32,
        wdata_len: usize,
    },
    UnsupportedRetailEdge {
        edge: i32,
        maximum: i32,
    },
    RegionRebuild(RegionsError),
}

/// Execute all four edge passes, followed by retail's exact
/// `Regions::clear_all` / `Regions::find_all` pair.
///
/// The native body owns two fixed `char[100]` stack arrays and indexes both
/// axes with `World::xs`; supported procedural maps are square. The safe port
/// therefore refuses non-square, undersized, or over-100 direct inputs before
/// mutation rather than inventing behaviour outside the caller domain.
pub fn execute_eliminate_edge_canals(
    world: &mut World,
    regions: &mut Regions,
) -> Result<EliminateEdgeCanalsReceipt, EliminateEdgeCanalsError> {
    let expected = world
        .xs
        .checked_mul(world.ys)
        .and_then(|size| usize::try_from(size).ok());
    if world.xs != world.ys
        || world.xs < 3
        || world.size != world.xs.wrapping_mul(world.ys)
        || expected != Some(world.wdata.len())
    {
        return Err(EliminateEdgeCanalsError::InvalidWorldShape {
            xs: world.xs,
            ys: world.ys,
            size: world.size,
            wdata_len: world.wdata.len(),
        });
    }
    if world.xs > MAX_RETAIL_EDGE {
        return Err(EliminateEdgeCanalsError::UnsupportedRetailEdge {
            edge: world.xs,
            maximum: MAX_RETAIL_EDGE,
        });
    }

    let mut next_world = world.clone();
    let mut next_regions = regions.clone();
    let before = next_world.checksum_sections();
    let mut passes = Vec::with_capacity(4);
    for side in [
        EdgeCanalSide::Left,
        EdgeCanalSide::Right,
        EdgeCanalSide::Top,
        EdgeCanalSide::Bottom,
    ] {
        passes.push(execute_side(&mut next_world, side));
    }
    let region_rebuild = next_regions
        .rebuild_after_coastlines(&mut next_world)
        .map_err(EliminateEdgeCanalsError::RegionRebuild)?;
    let after = next_world.checksum_sections();
    let receipt = EliminateEdgeCanalsReceipt {
        primitive_va: MAP_ELIMINATE_EDGE_CANALS_VA,
        return_va: MAP_ELIMINATE_EDGE_CANALS_RETURN_VA,
        caller_va: EAST_MEETS_WEST_EDGE_CANALS_CALL_VA,
        edge: next_world.xs,
        passes,
        direct_rng_sites: Vec::new(),
        region_rebuild,
        full_checksum_before: before.full,
        full_checksum_after: after.full,
        wdata_checksum_before: before.section(WorldSection::WData).adler,
        wdata_checksum_after: after.section(WorldSection::WData).adler,
        next_caller_va: EAST_MEETS_WEST_AFTER_EDGE_CANALS_VA,
        next_external_va: MAP_PLACE_START_IN_REGION_VA,
    };
    *world = next_world;
    *regions = next_regions;
    Ok(receipt)
}

fn execute_side(world: &mut World, side: EdgeCanalSide) -> EdgeCanalPassReceipt {
    let edge = world.xs;
    let mut outer_ocean = Vec::with_capacity(edge as usize);
    let mut first_inward_ocean = Vec::with_capacity(edge as usize);
    for along in 0..edge {
        let (outer_x, outer_y) = side_coord(edge, side, along, 0);
        let (inner_x, inner_y) = side_coord(edge, side, along, 1);
        outer_ocean.push(world.wdata(outer_x, outer_y).land == land::OCEAN);
        first_inward_ocean.push(world.wdata(inner_x, inner_y).land == land::OCEAN);
    }

    let mut writes = Vec::new();
    for middle in 1..edge - 1 {
        let index = middle as usize;
        if !(outer_ocean[index - 1] && outer_ocean[index] && outer_ocean[index + 1]) {
            continue;
        }
        let (first_x, first_y) = side_coord(edge, side, middle, 1);
        if world.wdata(first_x, first_y).land == land::FERTILE {
            write_ocean(world, side, middle, 1, first_x, first_y, &mut writes);
            first_inward_ocean[index] = true;
        }
        if first_inward_ocean[index - 1]
            && first_inward_ocean[index]
            && first_inward_ocean[index + 1]
        {
            let (second_x, second_y) = side_coord(edge, side, middle, 2);
            if world.wdata(second_x, second_y).land == land::FERTILE {
                write_ocean(world, side, middle, 2, second_x, second_y, &mut writes);
            }
        }
    }

    EdgeCanalPassReceipt {
        side,
        outer_ocean,
        first_inward_ocean_final: first_inward_ocean,
        writes,
    }
}

fn side_coord(edge: i32, side: EdgeCanalSide, along: i32, depth: i32) -> (i32, i32) {
    match side {
        EdgeCanalSide::Left => (depth, along),
        EdgeCanalSide::Right => (edge - 1 - depth, along),
        EdgeCanalSide::Top => (along, depth),
        EdgeCanalSide::Bottom => (along, edge - 1 - depth),
    }
}

fn write_ocean(
    world: &mut World,
    side: EdgeCanalSide,
    middle: i32,
    depth: i32,
    x: i32,
    y: i32,
    writes: &mut Vec<EdgeCanalWrite>,
) {
    let cell = world.wdata_mut(x, y);
    writes.push(EdgeCanalWrite {
        side,
        middle,
        depth,
        x,
        y,
        prior_land: cell.land,
        prior_land_sub: cell.land_sub,
    });
    // Native's 16-bit store at WData+2 writes `{land=2, land_sub=0}` and
    // leaves flags, region and region2 for the final region rebuild.
    cell.land = land::OCEAN;
    cell.land_sub = 0;
}
