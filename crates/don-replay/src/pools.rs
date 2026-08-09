//! Executable reconstruction of retail `Map::eliminate_pools`.
//!
//! The PDB fixes `0x0068b890` as `Map::eliminate_pools`, not
//! `Map::make_coastlines` (which is `0x006947a0`).  The helper rebuilds
//! connected regions, collects the sea regions selected by its edge mode, and
//! repeatedly changes the smallest selected pool into the first adjacent land
//! region.  It consumes no RNG.

use don_sim::systems::map_terrain::{land, World, NEIGHBOUR_DX, NEIGHBOUR_DY};
use don_sim::systems::regions::{RegionBuildReceipt, Regions, RegionsError, LAND_REGION_COUNT};

pub const MAP_ELIMINATE_POOLS_VA: u32 = 0x0068_b890;

/// PDB enum `Map::ElimPoolParam`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum ElimPoolParam {
    CornersOnly = 0,
    EdgesOnly = 1,
    EntireWorld = 2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PoolMergeReceipt {
    pub source_region: i32,
    pub target_region: i32,
    pub cells_changed: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EliminatePoolsReceipt {
    pub primitive_va: u32,
    pub param: ElimPoolParam,
    pub rebuild: RegionBuildReceipt,
    pub initial_pool_ids: Vec<i32>,
    pub merges: Vec<PoolMergeReceipt>,
    pub surviving_pool_ids: Vec<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EliminatePoolsError {
    RegionRebuild(RegionsError),
    InvalidCoordinateStorage {
        region: i32,
        length: usize,
        capacity: i32,
        increment: i16,
    },
    MissingSmallestPool {
        candidates: Vec<i32>,
        threshold: i32,
    },
    MissingAdjacentLand {
        source_region: i32,
    },
    RegionCoordinateMismatch {
        source_region: i32,
        x: i32,
        y: i32,
        world_region: i16,
    },
}

/// Execute `Map::eliminate_pools` `0x0068b890` transactionally.
pub fn execute_eliminate_pools(
    world: &mut World,
    regions: &mut Regions,
    param: ElimPoolParam,
) -> Result<EliminatePoolsReceipt, EliminatePoolsError> {
    validate_coordinate_storage(regions)?;
    let mut next_world = world.clone();
    let mut next_regions = regions.clone();
    let rebuild = next_regions
        .rebuild_after_coastlines(&mut next_world)
        .map_err(EliminatePoolsError::RegionRebuild)?;
    let receipt = eliminate_pools_body(&mut next_world, &mut next_regions, param, rebuild)?;
    *world = next_world;
    *regions = next_regions;
    Ok(receipt)
}

fn validate_coordinate_storage(regions: &Regions) -> Result<(), EliminatePoolsError> {
    for region in &regions.list {
        let list = &region.coords;
        let length = list.items.len();
        let valid_growth = list.increment < 0 || list.increment > 0;
        let capacity = usize::try_from(list.capacity).ok();
        if list.capacity < 0
            || capacity.map_or(true, |capacity| length > capacity)
            || (length >= list.capacity.max(0) as usize && !valid_growth)
        {
            return Err(EliminatePoolsError::InvalidCoordinateStorage {
                region: region.region,
                length,
                capacity: list.capacity,
                increment: list.increment,
            });
        }
    }
    Ok(())
}

fn eliminate_pools_body(
    world: &mut World,
    regions: &mut Regions,
    param: ElimPoolParam,
    rebuild: RegionBuildReceipt,
) -> Result<EliminatePoolsReceipt, EliminatePoolsError> {
    let mut candidates = Vec::new();
    for y in 0..world.ys {
        for x in 0..world.xs {
            let selected = match param {
                // The shipped enum name is counter-intuitive: mode zero scans
                // both vertical edge columns, not only four literal corners.
                ElimPoolParam::CornersOnly => x == 0 || x == world.xs - 1,
                ElimPoolParam::EdgesOnly => {
                    x == 0 || y == 0 || x == world.xs - 1 || y == world.ys - 1
                }
                ElimPoolParam::EntireWorld => true,
            };
            if !selected {
                continue;
            }
            let region = i32::from(world.wdata(x, y).region);
            if region > LAND_REGION_COUNT as i32 && !candidates.contains(&region) {
                candidates.push(region);
            }
        }
    }
    let initial_pool_ids = candidates.clone();
    let mut merges = Vec::new();
    while candidates.len() > 1 {
        // Retail seeds this threshold from xs*xs and updates only on strict
        // less-than, so equal pools retain scan/region order.
        let threshold = world.xs.wrapping_mul(world.xs);
        let source = candidates
            .iter()
            .copied()
            .filter(|&region| regions.list[region as usize].size < threshold)
            .min_by_key(|&region| {
                (
                    regions.list[region as usize].size,
                    candidates
                        .iter()
                        .position(|&candidate| candidate == region)
                        .unwrap(),
                )
            })
            .ok_or_else(|| EliminatePoolsError::MissingSmallestPool {
                candidates: candidates.clone(),
                threshold,
            })?;
        let source_index = source as usize;
        let source_coords = regions.list[source_index].coords.items.clone();
        let mut target = None;
        'coords: for &(x, y) in &source_coords {
            if world.wdata(x, y).region != source as i16 {
                return Err(EliminatePoolsError::RegionCoordinateMismatch {
                    source_region: source,
                    x,
                    y,
                    world_region: world.wdata(x, y).region,
                });
            }
            for (&dx, &dy) in NEIGHBOUR_DX.iter().zip(NEIGHBOUR_DY.iter()) {
                let nx = x + dx;
                let ny = y + dy;
                if !world.valid_w(nx, ny) {
                    continue;
                }
                let adjacent = i32::from(world.wdata(nx, ny).region);
                if adjacent != source && adjacent < (LAND_REGION_COUNT + 1) as i32 {
                    target = Some(adjacent);
                    break 'coords;
                }
            }
        }
        let target = target.ok_or(EliminatePoolsError::MissingAdjacentLand {
            source_region: source,
        })?;
        let target_index = target as usize;

        for &(x, y) in &source_coords {
            push_region_coord(regions, target_index, (x, y))?;
            regions.list[target_index].size = regions.list[target_index].size.wrapping_add(1);
            let cell = world.wdata_mut(x, y);
            cell.land = land::FERTILE;
            cell.land_sub = 0;
            cell.region = target as i16;
        }
        regions.list[source_index].coords.items.clear();
        regions.list[source_index].size = 0;
        candidates.retain(|&candidate| candidate != source);
        merges.push(PoolMergeReceipt {
            source_region: source,
            target_region: target,
            cells_changed: source_coords.len() as i32,
        });
    }

    Ok(EliminatePoolsReceipt {
        primitive_va: MAP_ELIMINATE_POOLS_VA,
        param,
        rebuild,
        initial_pool_ids,
        merges,
        surviving_pool_ids: candidates,
    })
}

fn push_region_coord(
    regions: &mut Regions,
    region: usize,
    coord: (i32, i32),
) -> Result<(), EliminatePoolsError> {
    let list = &mut regions.list[region].coords;
    let length = list.items.len() as i32;
    if length >= list.capacity {
        let increase = if list.increment < 0 {
            list.capacity.max(4)
        } else {
            i32::from(list.increment)
        };
        let Some(capacity) = (increase > 0)
            .then(|| list.capacity.checked_add(increase))
            .flatten()
        else {
            return Err(EliminatePoolsError::InvalidCoordinateStorage {
                region: region as i32,
                length: list.items.len(),
                capacity: list.capacity,
                increment: list.increment,
            });
        };
        list.capacity = capacity;
    }
    list.items.push(coord);
    Ok(())
}
