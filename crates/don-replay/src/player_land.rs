//! Executable reconstruction of retail `Map::check_player_land`.
//!
//! The helper is the Mediterranean bridge between starting-location placement
//! and the final continent.  It visits `World::start_city_x/y`, which contain
//! four cells per player, rather than visiting only the player centres.  For
//! each footprint cell it first attaches a 3x3 patch to the first nearby land
//! region, then grows the selected region through the canonical combat-circle
//! order.  The shipped body consumes no RNG.

use don_sim::systems::combat::{circle_table, CIRCLE_MAX_RING};
use don_sim::systems::map_terrain::{land, wflag, World};
use don_sim::systems::regions::{Regions, REGION_COUNT};

pub const MAP_CHECK_PLAYER_LAND_VA: u32 = 0x0068_ef00;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CheckPlayerLandCall {
    /// Register ECX: enable the nearby-continent exclusion scan.
    pub enabled: i32,
    /// Register EDX: square-spiral exclusion radius, capped at ten by retail.
    pub avoid_continent: i32,
    /// First stack argument. Negative values select the retail default of five.
    pub radius: i32,
    /// The caller supplies a fourth argument, but the shipped body never reads it.
    pub unused: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckPlayerLandReceipt {
    pub primitive_va: u32,
    pub call: CheckPlayerLandCall,
    pub effective_radius: i32,
    pub effective_avoid_continent: Option<i32>,
    pub footprints_visited: usize,
    pub target_searches: usize,
    pub target_regions_found: usize,
    pub seed_cells_written: usize,
    pub seed_coords_removed: usize,
    pub outer_candidates: usize,
    pub outer_cells_written: usize,
    pub conflict_rejections: usize,
    /// Region zero is a lawful target here. Immediately before the
    /// Mediterranean call every region id is zero, so retail grows and may
    /// append duplicate coordinates to this list.
    pub region_zero_appends: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckPlayerLandError {
    InvalidWorldShape {
        xs: i32,
        ys: i32,
        size: i32,
        wdata_len: usize,
    },
    StartCityArrayMismatch {
        x_len: usize,
        y_len: usize,
    },
    InvalidStartCityStorage {
        axis: char,
        length: usize,
        capacity: i32,
    },
    InvalidStartCityCoordinate {
        index: usize,
        x: i32,
        y: i32,
    },
    InvalidWorldRegion {
        x: i32,
        y: i32,
        region: i16,
    },
    InvalidRadius {
        radius: i32,
    },
    InvalidAvoidContinent {
        avoid_continent: i32,
    },
    InvalidCoordinateStorage {
        region: i32,
        length: usize,
        capacity: i32,
        increment: i16,
    },
}

/// Execute `Map::check_player_land` `0x0068ef00` transactionally.
///
/// All structural failures leave both authoritative stores untouched. The
/// successful path intentionally retains retail's duplicate coordinate-list
/// appends; deduplicating them would change later region iteration and checksums.
pub fn execute_check_player_land(
    world: &mut World,
    regions: &mut Regions,
    call: CheckPlayerLandCall,
) -> Result<CheckPlayerLandReceipt, CheckPlayerLandError> {
    let effective_radius = if call.radius < 0 { 5 } else { call.radius };
    validate(world, regions, call, effective_radius)?;

    let mut next_world = world.clone();
    let mut next_regions = regions.clone();
    let receipt =
        check_player_land_body(&mut next_world, &mut next_regions, call, effective_radius)?;
    *world = next_world;
    *regions = next_regions;
    Ok(receipt)
}

fn validate(
    world: &World,
    regions: &Regions,
    call: CheckPlayerLandCall,
    effective_radius: i32,
) -> Result<(), CheckPlayerLandError> {
    let expected = i64::from(world.xs).checked_mul(i64::from(world.ys));
    if world.xs <= 0
        || world.ys <= 0
        || world.size != world.xs.saturating_mul(world.ys)
        || expected != Some(world.wdata.len() as i64)
    {
        return Err(CheckPlayerLandError::InvalidWorldShape {
            xs: world.xs,
            ys: world.ys,
            size: world.size,
            wdata_len: world.wdata.len(),
        });
    }
    if !(0..=CIRCLE_MAX_RING as i32).contains(&effective_radius) {
        return Err(CheckPlayerLandError::InvalidRadius {
            radius: effective_radius,
        });
    }
    if call.enabled != 0 && call.avoid_continent < 0 {
        // Retail would index before the shipped radius table. Treat malformed
        // direct API input as an error rather than manufacturing another map.
        return Err(CheckPlayerLandError::InvalidAvoidContinent {
            avoid_continent: call.avoid_continent,
        });
    }
    if world.start_city_x.items.len() != world.start_city_y.items.len() {
        return Err(CheckPlayerLandError::StartCityArrayMismatch {
            x_len: world.start_city_x.items.len(),
            y_len: world.start_city_y.items.len(),
        });
    }
    for (axis, array) in [('x', &world.start_city_x), ('y', &world.start_city_y)] {
        if array.capacity < 0 || array.items.len() > array.capacity as usize {
            return Err(CheckPlayerLandError::InvalidStartCityStorage {
                axis,
                length: array.items.len(),
                capacity: array.capacity,
            });
        }
    }
    for (index, (&x, &y)) in world
        .start_city_x
        .items
        .iter()
        .zip(&world.start_city_y.items)
        .enumerate()
    {
        if !world.valid_w(x, y) {
            return Err(CheckPlayerLandError::InvalidStartCityCoordinate { index, x, y });
        }
    }
    for y in 0..world.ys {
        for x in 0..world.xs {
            let region = world.wdata(x, y).region;
            if region < 0 || region as usize >= REGION_COUNT {
                return Err(CheckPlayerLandError::InvalidWorldRegion { x, y, region });
            }
        }
    }
    for region in &regions.list {
        validate_coord_storage(
            region.region,
            region.coords.items.len(),
            region.coords.capacity,
            region.coords.increment,
        )?;
    }
    Ok(())
}

fn check_player_land_body(
    world: &mut World,
    regions: &mut Regions,
    call: CheckPlayerLandCall,
    effective_radius: i32,
) -> Result<CheckPlayerLandReceipt, CheckPlayerLandError> {
    let spiral = square_spiral(10);
    debug_assert_eq!(spiral.len(), 441);
    let circle = circle_table();
    let outer_end = circle.ring_end[effective_radius as usize] as usize;
    let effective_avoid_continent = (call.enabled != 0).then(|| call.avoid_continent.min(10));
    let exclusion_end = effective_avoid_continent
        .map(|radius| ((radius * 2 + 1) * (radius * 2 + 1)) as usize)
        .unwrap_or(1);

    let starts: Vec<(i32, i32)> = world
        .start_city_x
        .items
        .iter()
        .copied()
        .zip(world.start_city_y.items.iter().copied())
        .collect();
    let mut receipt = CheckPlayerLandReceipt {
        primitive_va: MAP_CHECK_PLAYER_LAND_VA,
        call,
        effective_radius,
        effective_avoid_continent,
        footprints_visited: 0,
        target_searches: 0,
        target_regions_found: 0,
        seed_cells_written: 0,
        seed_coords_removed: 0,
        outer_candidates: 0,
        outer_cells_written: 0,
        conflict_rejections: 0,
        region_zero_appends: 0,
    };

    for (start_x, start_y) in starts {
        receipt.footprints_visited += 1;
        let mut local_region = i32::from(world.wdata(start_x, start_y).region);
        if local_region == 0 {
            receipt.target_searches += 1;
            // The assembly addresses move_x/y at base+4 and performs 48
            // iterations: the complete radii-one-through-three perimeter,
            // excluding the centre at index zero.
            let target = spiral[1..49].iter().find_map(|&(dx, dy)| {
                let x = start_x.wrapping_add(dx);
                let y = start_y.wrapping_add(dy);
                world
                    .valid_w(x, y)
                    .then(|| i32::from(world.wdata(x, y).region))
                    .filter(|region| (1..=63).contains(region))
            });
            if let Some(target) = target {
                receipt.target_regions_found += 1;
                for &(dx, dy) in &spiral[..9] {
                    let x = start_x.wrapping_add(dx);
                    let y = start_y.wrapping_add(dy);
                    if !world.valid_w(x, y) {
                        continue;
                    }
                    {
                        let cell = world.wdata_mut(x, y);
                        cell.land = land::FERTILE;
                        cell.land_sub = 0;
                    }
                    // 0x0068f055 computes this list from the evolving
                    // `local_region`, not from the cell's old WData.region.
                    // After the first write local_region is the target, so a
                    // missing removal can still be followed by a size
                    // decrement. The resulting size/list mismatch is retail.
                    let remove_region = local_region as usize;
                    if let Some(index) = regions.list[remove_region]
                        .coords
                        .items
                        .iter()
                        .position(|&coord| coord == (x, y))
                    {
                        regions.list[remove_region].coords.items.remove(index);
                        receipt.seed_coords_removed += 1;
                    }
                    if regions.list[remove_region].size != 0 {
                        regions.list[remove_region].size =
                            regions.list[remove_region].size.wrapping_sub(1);
                    }
                    push_region_coord(regions, target as usize, (x, y))?;
                    regions.list[target as usize].size =
                        regions.list[target as usize].size.wrapping_add(1);
                    world.wdata_mut(x, y).region = target as i16;
                    local_region = target;
                    receipt.seed_cells_written += 1;
                }
            }
        }

        for index in 1..outer_end {
            let x = start_x.wrapping_add(i32::from(circle.x[index]));
            let y = start_y.wrapping_add(i32::from(circle.y[index]));
            if !world.valid_w(x, y) {
                continue;
            }
            receipt.outer_candidates += 1;
            let rejected = if call.enabled != 0 {
                spiral[1..exclusion_end].iter().any(|&(dx, dy)| {
                    let nx = x.wrapping_add(dx);
                    let ny = y.wrapping_add(dy);
                    if !world.valid_w(nx, ny) {
                        return false;
                    }
                    let neighbor = world.wdata(nx, ny);
                    let traversable = neighbor.flags & wflag::WATERHALF != 0
                        || (neighbor.land != land::COASTAL && neighbor.land != land::OCEAN);
                    traversable
                        && i32::from(neighbor.region) != local_region
                        && regions.list[neighbor.region as usize].coords.items.len() > 4
                })
            } else {
                false
            };
            if rejected {
                receipt.conflict_rejections += 1;
                continue;
            }

            {
                let cell = world.wdata_mut(x, y);
                cell.land = land::FERTILE;
                cell.land_sub = 0;
            }
            push_region_coord(regions, local_region as usize, (x, y))?;
            regions.list[local_region as usize].size =
                regions.list[local_region as usize].size.wrapping_add(1);
            world.wdata_mut(x, y).region = local_region as i16;
            receipt.outer_cells_written += 1;
            if local_region == 0 {
                receipt.region_zero_appends += 1;
            }
        }
    }

    Ok(receipt)
}

/// `move_x/move_y`: centre followed by each square perimeter clockwise from
/// its north-west corner. Radius ten is the largest one read by this helper.
fn square_spiral(max_radius: i32) -> Vec<(i32, i32)> {
    let mut offsets = Vec::with_capacity(((max_radius * 2 + 1).pow(2)) as usize);
    offsets.push((0, 0));
    for radius in 1..=max_radius {
        for x in -radius..=radius {
            offsets.push((x, -radius));
        }
        for y in (-radius + 1)..=radius {
            offsets.push((radius, y));
        }
        for x in (-radius..=(radius - 1)).rev() {
            offsets.push((x, radius));
        }
        for y in ((-radius + 1)..=(radius - 1)).rev() {
            offsets.push((-radius, y));
        }
    }
    offsets
}

fn validate_coord_storage(
    region: i32,
    length: usize,
    capacity: i32,
    increment: i16,
) -> Result<(), CheckPlayerLandError> {
    let cannot_grow = length >= capacity.max(0) as usize && increment == 0;
    if capacity < 0 || length > capacity.max(0) as usize || cannot_grow {
        return Err(CheckPlayerLandError::InvalidCoordinateStorage {
            region,
            length,
            capacity,
            increment,
        });
    }
    Ok(())
}

fn push_region_coord(
    regions: &mut Regions,
    region: usize,
    coord: (i32, i32),
) -> Result<(), CheckPlayerLandError> {
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
            return Err(CheckPlayerLandError::InvalidCoordinateStorage {
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
