//! Exact, read-only reconstruction of retail `Map::find_region_centroid`.
//!
//! The body at `0x0068ae50` ignores its `Map *this`, indexes the global
//! `Regions::list`, adds the first `Region::size` coordinate pairs with wrapping
//! 32-bit arithmetic, and divides both signed sums by that positive size.  It
//! consumes no RNG and mutates neither `World` nor `Regions`.

use don_sim::systems::regions::Regions;

pub const MAP_FIND_REGION_CENTROID_VA: u32 = 0x0068_ae50;
pub const MAP_FIND_REGION_CENTROID_RETURN_VA: u32 = 0x0068_b09b;
pub const EAST_MEETS_WEST_FIRST_CENTROID_CALL_VA: u32 = 0x0069_6c82;
pub const EAST_MEETS_WEST_AFTER_CENTROID_LOOP_VA: u32 = 0x0069_6d07;
pub const MAP_ELIMINATE_POOLS_VA: u32 = 0x0068_b890;
pub const MAP_ELIMINATE_EDGE_CANALS_VA: u32 = 0x0068_b360;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionCentroidReceipt {
    pub primitive_va: u32,
    pub return_va: u32,
    pub region: i32,
    /// Native `Region::size`, which is both the array bound and divisor.
    pub size: i32,
    /// Number of eight-coordinate SSE2 blocks at `0x0068aec0`.
    pub vector_blocks: u32,
    /// Number of two-coordinate scalar iterations at `0x0068b000`.
    pub scalar_pairs: u32,
    /// Whether `0x0068b030` consumes the final coordinate.
    pub scalar_tail: bool,
    pub x_sum: i32,
    pub y_sum: i32,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EastMeetsWestCentroidReceipt {
    pub caller_first_call_va: u32,
    pub centroids: Vec<RegionCentroidReceipt>,
    /// Exact caller-owned first `SimpleArray<int>`: centroid X by region.
    pub centroid_x: Vec<i32>,
    /// Exact caller-owned second `SimpleArray<int>`: centroid Y by region.
    pub centroid_y: Vec<i32>,
    pub next_va: u32,
    pub next_pool_param: i32,
    /// PDB exposes a second `int`; the shipped callee never reads it.
    pub next_pool_second_argument_is_dead: bool,
    pub after_pools_va: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegionCentroidError {
    InvalidRegion {
        region: i32,
    },
    NonPositiveSize {
        region: i32,
        size: i32,
    },
    CoordinateStorageShort {
        region: i32,
        size: i32,
        coordinates: usize,
    },
    InvalidContinentCount {
        count: i32,
    },
}

/// Execute `Map::find_region_centroid(int, WCoord*, WCoord*)` for its lawful
/// map-generation domain.
///
/// Retail trusts the region metadata and would fault on a zero divisor or a
/// short native coordinate array.  The safe port preserves the exact result
/// for valid storage and refuses malformed state before reading or committing
/// anything.  Extra coordinates are deliberately ignored because native uses
/// `Region::size`, not `WCoordList::length`, as its bound.
pub fn execute_find_region_centroid(
    regions: &Regions,
    region: i32,
) -> Result<RegionCentroidReceipt, RegionCentroidError> {
    let Ok(region_index) = usize::try_from(region) else {
        return Err(RegionCentroidError::InvalidRegion { region });
    };
    let Some(row) = regions.list.get(region_index) else {
        return Err(RegionCentroidError::InvalidRegion { region });
    };
    if row.size <= 0 {
        return Err(RegionCentroidError::NonPositiveSize {
            region,
            size: row.size,
        });
    }
    let size = row.size as usize;
    if row.coords.items.len() < size {
        return Err(RegionCentroidError::CoordinateStorageShort {
            region,
            size: row.size,
            coordinates: row.coords.items.len(),
        });
    }

    // `paddd` and the scalar `add` instructions are both modulo 2^32. Their
    // grouping differs, but wrapping addition is associative, so this ordered
    // fold is bit-identical to the SSE2 reduction and scalar tail.
    let (x_sum, y_sum) = row.coords.items[..size]
        .iter()
        .fold((0_i32, 0_i32), |(x_sum, y_sum), &(x, y)| {
            (x_sum.wrapping_add(x), y_sum.wrapping_add(y))
        });
    let divisor = row.size;

    Ok(RegionCentroidReceipt {
        primitive_va: MAP_FIND_REGION_CENTROID_VA,
        return_va: MAP_FIND_REGION_CENTROID_RETURN_VA,
        region,
        size: row.size,
        vector_blocks: (size / 8) as u32,
        scalar_pairs: ((size % 8) / 2) as u32,
        scalar_tail: size % 2 != 0,
        x_sum,
        y_sum,
        x: x_sum / divisor,
        y: y_sum / divisor,
    })
}

/// Execute the caller-owned centroid loop at `0x00696c65..0x00696d07`.
///
/// The loop is one-based and appends X and Y to separate `SimpleArray<int>`
/// instances. Its next mutator is `eliminate_pools(EntireWorld, dead)`,
/// followed by the separately ported `eliminate_edge_canals` body.
pub fn execute_east_meets_west_centroids(
    regions: &Regions,
    continent_count: i32,
) -> Result<EastMeetsWestCentroidReceipt, RegionCentroidError> {
    if !(1..=8).contains(&continent_count) {
        return Err(RegionCentroidError::InvalidContinentCount {
            count: continent_count,
        });
    }
    let mut centroids = Vec::with_capacity(continent_count as usize);
    let mut centroid_x = Vec::with_capacity(continent_count as usize);
    let mut centroid_y = Vec::with_capacity(continent_count as usize);
    for region in 1..=continent_count {
        let receipt = execute_find_region_centroid(regions, region)?;
        centroid_x.push(receipt.x);
        centroid_y.push(receipt.y);
        centroids.push(receipt);
    }
    Ok(EastMeetsWestCentroidReceipt {
        caller_first_call_va: EAST_MEETS_WEST_FIRST_CENTROID_CALL_VA,
        centroids,
        centroid_x,
        centroid_y,
        next_va: MAP_ELIMINATE_POOLS_VA,
        next_pool_param: 2,
        next_pool_second_argument_is_dead: true,
        after_pools_va: MAP_ELIMINATE_EDGE_CANALS_VA,
    })
}
