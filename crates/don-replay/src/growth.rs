//! Executable reconstruction of retail region growth.
//!
//! This is the complete deterministic call graph rooted at `Map::grow_region`
//! `0x0069c600`: the generated radius table, `large_stamp`, `stamp`, `point`,
//! and `grow_valid`.  The implementation deliberately owns no second map or
//! coordinate model; it mutates the replay's persistent [`World`] and
//! [`Regions`] stores transactionally.

use don_sim::rng::Random;
use don_sim::systems::map_terrain::{land, World};
use don_sim::systems::regions::{Regions, LAND_REGION_COUNT};
use std::sync::OnceLock;

pub const MAP_GROW_REGION_VA: u32 = 0x0069_c600;
pub const MAP_LARGE_STAMP_VA: u32 = 0x0069_cb20;
pub const MAP_STAMP_VA: u32 = 0x0069_cf80;
pub const MAP_GROW_VALID_VA: u32 = 0x0069_d000;
pub const MAP_POINT_VA: u32 = 0x0069_d5a0;

const GROW_PICK_GATE_RNG_VA: u32 = 0x0069_c85b;
const GROW_TAIL_OFFSET_RNG_VA: u32 = 0x0069_c8a9;
const GROW_RANDOM_ALL_RNG_VA: u32 = 0x0069_c9cb;
const GROW_RANDOM_BOUNDARY_RNG_VA: u32 = 0x0069_c9ff;
const LARGE_STAMP_PATTERN_RNG_VA: u32 = 0x0069_cb4e;
const LARGE_STAMP_TOP_EDGE_RNG_VA: u32 = 0x0069_ce26;
const LARGE_STAMP_BOTTOM_EDGE_RNG_VA: u32 = 0x0069_ce65;
const GROW_VALID_EDGE_RNG_VA: u32 = 0x0069_d2ab;
const STANDARD_MAP_EDGE: i32 = 70;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrowRegionCall {
    pub region: i32,
    pub target_area: i32,
    pub max_distance: i32,
    pub anchor_x: i32,
    pub anchor_y: i32,
    pub return_partial_size: i32,
}

/// Installed `Map` fields read by `grow_valid`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapGrowthConfig {
    pub avoid_center: i32,
    pub avoid_continent: i32,
    pub edge_avoid: [i32; 4],
    pub base_edge: i32,
    /// Retail keeps this jitter at `Map+0x64` across every validation call.
    pub edge_jitter: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrowRegionReceipt {
    pub call: GrowRegionCall,
    pub radius: i32,
    pub initial_size: i32,
    pub final_size: i32,
    pub retail_return: i32,
    pub completed: bool,
    pub rng_initial: i32,
    pub rng_final: i32,
    pub rng_sites: Vec<u32>,
    pub point_attempts: u32,
    pub points_added: u32,
    pub stamp_calls: u32,
    pub large_stamp_calls: u32,
    pub stalled_iterations: u32,
    pub edge_jitter_final: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrowRegionError {
    InvalidWorldShape {
        xs: i32,
        ys: i32,
        size: i32,
        wdata_len: usize,
    },
    InvalidWorldRegion {
        x: i32,
        y: i32,
        region: i16,
    },
    InvalidRegion {
        region: i32,
    },
    EmptyRegion {
        region: i32,
    },
    InvalidCoordinateStorage {
        region: i32,
        length: usize,
        capacity: i32,
        increment: i16,
    },
    InvalidMapField {
        field: &'static str,
        value: i32,
    },
}

#[derive(Default)]
struct Counters {
    sites: Vec<u32>,
    point_attempts: u32,
    points_added: u32,
    stamp_calls: u32,
    large_stamp_calls: u32,
    stalled_iterations: u32,
}

/// Execute retail `Map::grow_region` and its mandatory helpers.
///
/// Structural validation and all mutations are transactional. A retail stall
/// return is not a Rust error: it is exposed as `retail_return != 0`, allowing
/// the style driver to select its exact retry branch.
pub fn execute_grow_region(
    world: &mut World,
    regions: &mut Regions,
    rng: &mut Random,
    config: &mut MapGrowthConfig,
    call: &GrowRegionCall,
) -> Result<GrowRegionReceipt, GrowRegionError> {
    validate(world, regions, config, call)?;
    let mut next_world = world.clone();
    let mut next_regions = regions.clone();
    let mut next_rng = *rng;
    let mut next_config = config.clone();
    let receipt = grow_region_body(
        &mut next_world,
        &mut next_regions,
        &mut next_rng,
        &mut next_config,
        call,
    )?;
    *world = next_world;
    *regions = next_regions;
    *rng = next_rng;
    *config = next_config;
    Ok(receipt)
}

fn validate(
    world: &World,
    regions: &Regions,
    config: &MapGrowthConfig,
    call: &GrowRegionCall,
) -> Result<(), GrowRegionError> {
    let expected = i64::from(world.xs).checked_mul(i64::from(world.ys));
    if world.xs <= 0
        || world.ys <= 0
        || world.size != world.xs.saturating_mul(world.ys)
        || expected != Some(world.wdata.len() as i64)
    {
        return Err(GrowRegionError::InvalidWorldShape {
            xs: world.xs,
            ys: world.ys,
            size: world.size,
            wdata_len: world.wdata.len(),
        });
    }
    let Ok(region_index) = usize::try_from(call.region) else {
        return Err(GrowRegionError::InvalidRegion {
            region: call.region,
        });
    };
    if region_index >= LAND_REGION_COUNT || region_index >= regions.list.len() {
        return Err(GrowRegionError::InvalidRegion {
            region: call.region,
        });
    }
    let region = &regions.list[region_index];
    if region.size <= 0 || region.coords.items.is_empty() {
        return Err(GrowRegionError::EmptyRegion {
            region: call.region,
        });
    }
    validate_coord_storage(
        call.region,
        region.coords.items.len(),
        region.coords.capacity,
        region.coords.increment,
    )?;
    if region.size as usize != region.coords.items.len() {
        return Err(GrowRegionError::InvalidCoordinateStorage {
            region: call.region,
            length: region.coords.items.len(),
            capacity: region.coords.capacity,
            increment: region.coords.increment,
        });
    }
    if !(0..=64).contains(&config.avoid_continent) {
        return Err(GrowRegionError::InvalidMapField {
            field: "avoid_continent",
            value: config.avoid_continent,
        });
    }
    for y in 0..world.ys {
        for x in 0..world.xs {
            let cell_region = world.wdata(x, y).region;
            if cell_region < 0 || cell_region as usize >= regions.list.len() {
                return Err(GrowRegionError::InvalidWorldRegion {
                    x,
                    y,
                    region: cell_region,
                });
            }
        }
    }
    if !(0..=3).contains(&config.base_edge) {
        return Err(GrowRegionError::InvalidMapField {
            field: "base_edge",
            value: config.base_edge,
        });
    }
    if !(0..=4).contains(&config.edge_jitter) {
        return Err(GrowRegionError::InvalidMapField {
            field: "edge_jitter",
            value: config.edge_jitter,
        });
    }
    Ok(())
}

fn validate_coord_storage(
    region: i32,
    length: usize,
    capacity: i32,
    increment: i16,
) -> Result<(), GrowRegionError> {
    if capacity < 0 || length > i32::MAX as usize || length as i32 > capacity {
        return Err(GrowRegionError::InvalidCoordinateStorage {
            region,
            length,
            capacity,
            increment,
        });
    }
    Ok(())
}

fn grow_region_body(
    world: &mut World,
    regions: &mut Regions,
    rng: &mut Random,
    config: &mut MapGrowthConfig,
    call: &GrowRegionCall,
) -> Result<GrowRegionReceipt, GrowRegionError> {
    let rng_initial = rng.state();
    let mut counters = Counters::default();
    let mut radius: i32 = 2;
    let quarter = call.target_area / 4;
    if quarter > 12 {
        while radius < 64 {
            radius += 1;
            if radius.wrapping_mul(radius).wrapping_mul(3) >= quarter {
                break;
            }
        }
    }
    let table = radius_table();
    let region_index = call.region as usize;
    let initial_size = regions.list[region_index].size;
    let first = regions.list[region_index].coords.items[0];
    let (anchor_x, anchor_y) = if call.anchor_x < 0 || call.anchor_y < 0 {
        first
    } else {
        (call.anchor_x, call.anchor_y)
    };

    let boundary_start = if initial_size < 2 {
        for &(dx, dy) in &table.coords[1..table.end[radius as usize] as usize] {
            let x = first.0 + i32::from(dx);
            let y = first.1 + i32::from(dy);
            if world.valid_w(x, y)
                && grow_valid(
                    world,
                    regions,
                    rng,
                    config,
                    call.region,
                    x,
                    y,
                    &mut counters,
                )
            {
                let _ = point(
                    world,
                    regions,
                    rng,
                    config,
                    call.region,
                    x,
                    y,
                    &mut counters,
                )?;
            }
        }
        table.start(radius)
    } else {
        0
    };

    let mut stalls_total = 0i32;
    let mut stalls_consecutive = 0i32;
    let retail_return;
    loop {
        let old_size = regions.list[region_index].size;
        if old_size >= call.target_area {
            retail_return = if call.return_partial_size == 0 {
                0
            } else {
                old_size
            };
            break;
        }

        let scaled_gate = scale_by_area(world, 512);
        let bucket = old_size / 8;
        let choose_random = old_size < scaled_gate
            || bucket <= 1
            || draw(rng, &mut counters, GROW_PICK_GATE_RNG_VA) % bucket == 0;
        let mut selected = if choose_random {
            random_growth_index(
                regions,
                rng,
                &mut counters,
                region_index,
                boundary_start,
                table.ring_len(radius),
            )
        } else {
            let width = (old_size + 7) / 8;
            let offset = if width <= 1 {
                0
            } else {
                draw(rng, &mut counters, GROW_TAIL_OFFSET_RNG_VA) % width
            };
            let mut index = old_size.wrapping_mul(7) / 8 + offset;
            index = index.clamp(0, old_size - 1);
            let coord = regions.list[region_index].coords.items[index as usize];
            if approx_distance(coord.0 - anchor_x, coord.1 - anchor_y) > call.max_distance {
                random_growth_index(
                    regions,
                    rng,
                    &mut counters,
                    region_index,
                    boundary_start,
                    table.ring_len(radius),
                )
            } else {
                index
            }
        };
        selected = selected.clamp(0, old_size - 1);
        let (x, y) = regions.list[region_index].coords.items[selected as usize];
        if call.target_area - old_size < 64 {
            if point(
                world,
                regions,
                rng,
                config,
                call.region,
                x,
                y,
                &mut counters,
            )? && point(
                world,
                regions,
                rng,
                config,
                call.region,
                x + 1,
                y,
                &mut counters,
            )? && point(
                world,
                regions,
                rng,
                config,
                call.region,
                x - 1,
                y,
                &mut counters,
            )? && point(
                world,
                regions,
                rng,
                config,
                call.region,
                x,
                y - 1,
                &mut counters,
            )? {
                let _ = point(
                    world,
                    regions,
                    rng,
                    config,
                    call.region,
                    x,
                    y + 1,
                    &mut counters,
                )?;
            }
        } else {
            let _ = large_stamp(
                world,
                regions,
                rng,
                config,
                call.region,
                x,
                y,
                &mut counters,
            )?;
        }

        let new_size = regions.list[region_index].size;
        if new_size == old_size {
            counters.stalled_iterations += 1;
            stalls_total += 1;
            if stalls_total >= call.target_area.wrapping_mul(8) {
                retail_return = new_size;
                break;
            }
            stalls_consecutive += 1;
            if stalls_consecutive >= call.target_area {
                retail_return = new_size;
                break;
            }
        } else {
            stalls_consecutive = 0;
        }
    }

    let final_size = regions.list[region_index].size;
    Ok(GrowRegionReceipt {
        call: call.clone(),
        radius,
        initial_size,
        final_size,
        retail_return,
        completed: final_size >= call.target_area,
        rng_initial,
        rng_final: rng.state(),
        rng_sites: counters.sites,
        point_attempts: counters.point_attempts,
        points_added: counters.points_added,
        stamp_calls: counters.stamp_calls,
        large_stamp_calls: counters.large_stamp_calls,
        stalled_iterations: counters.stalled_iterations,
        edge_jitter_final: config.edge_jitter,
    })
}

fn random_growth_index(
    regions: &Regions,
    rng: &mut Random,
    counters: &mut Counters,
    region: usize,
    boundary_start: i32,
    ring_len: i32,
) -> i32 {
    let size = regions.list[region].size;
    if boundary_start == 0 || size - boundary_start < ring_len {
        if size <= 1 {
            0
        } else {
            draw(rng, counters, GROW_RANDOM_ALL_RNG_VA) % size
        }
    } else {
        let length = size - boundary_start;
        if length <= 1 {
            boundary_start
        } else {
            boundary_start + draw(rng, counters, GROW_RANDOM_BOUNDARY_RNG_VA) % length
        }
    }
}

fn large_stamp(
    world: &mut World,
    regions: &mut Regions,
    rng: &mut Random,
    config: &mut MapGrowthConfig,
    region: i32,
    x: i32,
    y: i32,
    counters: &mut Counters,
) -> Result<bool, GrowRegionError> {
    counters.large_stamp_calls += 1;
    if !stamp(world, regions, rng, config, region, x, y, counters)? {
        return Ok(false);
    }
    let pattern = draw(rng, counters, LARGE_STAMP_PATTERN_RNG_VA) & 15;
    let offsets: &[(i32, i32)] = match pattern {
        0 => &[(-1, -1), (-2, -2), (-4, -1), (-4, 0), (-5, -1), (-5, -2)],
        1 => &[(1, 1), (2, 2), (4, 1), (4, 0), (5, 1), (5, 2)],
        2 => &[(1, 0), (1, -2), (1, 2), (2, -4), (3, -4), (4, 1)],
        3 => &[(-1, 0), (-1, -2), (-1, 2), (-2, 4), (-3, 4), (-4, -1)],
        5 => &[(1, -2), (2, -4)],
        6 => &[(-1, -2), (-2, -4)],
        7 => &[(-1, 2), (-2, 4)],
        8 => &[(1, 2), (2, 4)],
        9..=15 => return Ok(true),
        4 => {
            if !stamp(world, regions, rng, config, region, x, y - 1, counters)?
                || !stamp(world, regions, rng, config, region, x, y + 1, counters)?
            {
                return Ok(false);
            }
            for dx in -2..=2 {
                if !stamp(world, regions, rng, config, region, x + dx, y, counters)? {
                    return Ok(false);
                }
            }
            let top_dx = if draw(rng, counters, LARGE_STAMP_TOP_EDGE_RNG_VA) & 1 != 0 {
                1
            } else {
                -1
            };
            if !stamp(
                world,
                regions,
                rng,
                config,
                region,
                x + top_dx,
                y - 1,
                counters,
            )? {
                return Ok(false);
            }
            let bottom_dx = if draw(rng, counters, LARGE_STAMP_BOTTOM_EDGE_RNG_VA) & 1 != 0 {
                1
            } else {
                -1
            };
            return stamp(
                world,
                regions,
                rng,
                config,
                region,
                x + bottom_dx,
                y + 1,
                counters,
            );
        }
        _ => unreachable!(),
    };
    for &(dx, dy) in offsets {
        if !stamp(
            world,
            regions,
            rng,
            config,
            region,
            x + dx,
            y + dy,
            counters,
        )? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn stamp(
    world: &mut World,
    regions: &mut Regions,
    rng: &mut Random,
    config: &mut MapGrowthConfig,
    region: i32,
    x: i32,
    y: i32,
    counters: &mut Counters,
) -> Result<bool, GrowRegionError> {
    counters.stamp_calls += 1;
    if !point(world, regions, rng, config, region, x, y, counters)?
        || !point(world, regions, rng, config, region, x + 1, y, counters)?
        || !point(world, regions, rng, config, region, x - 1, y, counters)?
        || !point(world, regions, rng, config, region, x, y - 1, counters)?
    {
        return Ok(false);
    }
    point(world, regions, rng, config, region, x, y + 1, counters)
}

/// Return value matches retail: out-of-bounds and occupied points are benign
/// `true`; only `grow_valid` rejection returns `false`.
fn point(
    world: &mut World,
    regions: &mut Regions,
    rng: &mut Random,
    config: &mut MapGrowthConfig,
    region: i32,
    x: i32,
    y: i32,
    counters: &mut Counters,
) -> Result<bool, GrowRegionError> {
    counters.point_attempts += 1;
    if !world.valid_w(x, y) || world.wdata(x, y).region != 0 {
        return Ok(true);
    }
    if !grow_valid(world, regions, rng, config, region, x, y, counters) {
        return Ok(false);
    }
    let region_index = region as usize;
    let coords = &regions.list[region_index].coords;
    validate_coord_storage(
        region,
        coords.items.len(),
        coords.capacity,
        coords.increment,
    )?;
    ensure_coord_capacity(regions, region_index)?;

    let cell = world.wdata_mut(x, y);
    // Overlapping word write at WData+2 zeroes both land subtype bytes.
    cell.land = land::FERTILE;
    cell.land_sub = 0;
    cell.region = region as i16;
    regions.list[region_index].coords.items.push((x, y));
    regions.list[region_index].size += 1;
    counters.points_added += 1;

    if regions.list[region_index].flags & 1 != 0 {
        update_region_connections(world, regions, region_index, x, y);
    }
    Ok(true)
}

fn ensure_coord_capacity(regions: &mut Regions, region: usize) -> Result<(), GrowRegionError> {
    let list = &mut regions.list[region].coords;
    let length = list.items.len() as i32;
    if length >= list.capacity {
        let increase = if list.increment < 0 {
            list.capacity.max(4)
        } else {
            i32::from(list.increment)
        };
        let Some(next_capacity) = list.capacity.checked_add(increase) else {
            return Err(GrowRegionError::InvalidCoordinateStorage {
                region: region as i32,
                length: list.items.len(),
                capacity: list.capacity,
                increment: list.increment,
            });
        };
        if increase <= 0 || next_capacity <= length {
            return Err(GrowRegionError::InvalidCoordinateStorage {
                region: region as i32,
                length: list.items.len(),
                capacity: list.capacity,
                increment: list.increment,
            });
        }
        list.capacity = next_capacity;
    }
    Ok(())
}

fn update_region_connections(world: &World, regions: &mut Regions, region: usize, x: i32, y: i32) {
    const DX: [i32; 9] = [0, -1, 0, 1, 1, 1, 0, -1, -1];
    const DY: [i32; 9] = [0, -1, -1, -1, 0, 1, 1, 1, 0];
    for (&dx, &dy) in DX.iter().zip(DY.iter()) {
        let (nx, ny) = (x + dx, y + dy);
        if !world.valid_w(nx, ny) {
            continue;
        }
        let other = world.wdata(nx, ny).region as u16 as usize;
        if other == 0 || other == region || other >= regions.list.len() {
            continue;
        }
        let combined = regions.list[region].borders
            | regions.list[other].borders
            | 1i32.wrapping_shl((region & 31) as u32)
            | 1i32.wrapping_shl((other & 31) as u32);
        regions.list[region].borders |= combined;
        regions.list[other].borders |= combined;
        for candidate in 0..32 {
            if regions.list[candidate].borders & combined != 0 {
                regions.list[candidate].borders |= combined;
            }
        }
    }
}

fn grow_valid(
    world: &World,
    regions: &Regions,
    rng: &mut Random,
    config: &mut MapGrowthConfig,
    target: i32,
    x: i32,
    y: i32,
    counters: &mut Counters,
) -> bool {
    if config.avoid_continent > 64 {
        return false;
    }
    let target_flags = if target >= 0 {
        regions.list[target as usize].flags
    } else {
        0
    };
    for other in 1..LAND_REGION_COUNT {
        let candidate = &regions.list[other];
        if candidate.size != 0
            && other as i32 != target
            && (target < 0 || target_flags & candidate.flags & 1 == 0)
            && approx_distance(x - world.xs / 2, y - world.ys / 2) < config.avoid_center
        {
            return false;
        }
    }

    let table = radius_table();
    let neighbour_end = table.end[config.avoid_continent as usize] as usize;
    for &(dx, dy) in &table.coords[..neighbour_end] {
        let (nx, ny) = (x + i32::from(dx), y + i32::from(dy));
        if !world.valid_w(nx, ny) {
            continue;
        }
        let other = world.wdata(nx, ny).region as i32;
        if other != 0
            && other != target
            && (target < 0 || target_flags & regions.list[other as usize].flags & 1 == 0)
        {
            return false;
        }
    }

    for edge_index in 0..4 {
        let direction = (config.base_edge + edge_index as i32) % 4;
        let edge = config.edge_avoid[edge_index];
        if edge == 0 {
            continue;
        }
        config.edge_jitter += draw(rng, counters, GROW_VALID_EDGE_RNG_VA) % 3 - 1;
        config.edge_jitter = config.edge_jitter.clamp(0, 4);
        let effective = edge + config.edge_jitter;
        let rejected = match direction {
            0 => x < effective,
            1 => y < effective,
            2 => world.xs - x <= effective,
            3 => world.ys - y <= effective,
            _ => false,
        };
        if rejected {
            return false;
        }
    }
    true
}

fn draw(rng: &mut Random, counters: &mut Counters, site: u32) -> i32 {
    counters.sites.push(site);
    rng.get(0, 0xffff)
}

fn scale_by_area(world: &World, value: i32) -> i32 {
    if value < 1 {
        return 0;
    }
    let standard_area = STANDARD_MAP_EDGE * STANDARD_MAP_EDGE;
    ((standard_area / 2).wrapping_add(world.size.wrapping_mul(value)) / standard_area).max(1)
}

fn approx_distance(dx: i32, dy: i32) -> i32 {
    let ax = dx.wrapping_abs();
    let ay = dy.wrapping_abs();
    let (large, small) = if ax > ay { (ax, ay) } else { (ay, ax) };
    if large == 0 {
        0
    } else if small < 60_000 {
        small.wrapping_mul(small) / large.wrapping_mul(2) + large
    } else {
        (small + large.wrapping_mul(2)) >> 1
    }
}

struct RadiusTable {
    coords: Vec<(i8, i8)>,
    end: [i32; 65],
}

fn radius_table() -> &'static RadiusTable {
    static TABLE: OnceLock<RadiusTable> = OnceLock::new();
    TABLE.get_or_init(RadiusTable::new)
}

impl RadiusTable {
    /// Runtime initializer `0x006817f0`, including its unusual cumulative
    /// square scan order rather than a sorted geometric perimeter.
    fn new() -> Self {
        let mut coords = Vec::with_capacity(12_873);
        let mut end = [0; 65];
        let mut minimum = 0;
        for radius in 0..=64 {
            for x in minimum..=radius {
                for y in minimum..=radius {
                    if approx_distance(x, y) == radius {
                        coords.push((x as i8, y as i8));
                        if coords.len() > 0x3248 {
                            end[radius as usize..].fill(coords.len() as i32);
                            return Self { coords, end };
                        }
                    }
                }
            }
            end[radius as usize] = coords.len() as i32;
            minimum -= 1;
        }
        Self { coords, end }
    }

    fn start(&self, radius: i32) -> i32 {
        if radius == 0 {
            0
        } else {
            self.end[radius as usize - 1]
        }
    }

    fn ring_len(&self, radius: i32) -> i32 {
        self.end[radius as usize] - self.start(radius)
    }
}
