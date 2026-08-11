//! Exact residual of `MapEastIndies::make_continents` after player-continent growth.
//!
//! The admitted continent prefix stops at `0x00697b72`, immediately before retail
//! chooses the number of non-player islands.  This module owns the rest of the style
//! virtual through its return at `0x00697da4`.  It deliberately does not own the common
//! `Map::make` continuation (`Regions::clear_all` at `0x00680060`).
//!
//! Provenance is the shipped PE/PDB, not a replay checksum.  In particular, the two
//! rejection budgets, the adaptive island-area floor, and every direct RNG site are
//! transcribed from `riseofnations.exe` `0x00697b4f..0x00697da9`.

use crate::growth::{
    execute_grow_region, execute_grow_valid, GrowRegionCall, GrowRegionError, GrowRegionReceipt,
    GrowValidCall, GrowValidReceipt, MapGrowthConfig,
};
use don_sim::rng::Random;
use don_sim::systems::combat::circle_table;
use don_sim::systems::map_terrain::{land, WCoord, World, WorldSection};
use don_sim::systems::regions::{Regions, LAND_REGION_COUNT};

pub const EAST_INDIES_NONPLAYER_ISLANDS_VA: u32 = 0x0069_7b72;
pub const EAST_INDIES_ISLAND_X_RNG_VA: u32 = 0x0069_7c3f;
pub const EAST_INDIES_ISLAND_Y_RNG_VA: u32 = 0x0069_7c69;
pub const EAST_INDIES_ISLAND_AREA_RNG_VA: u32 = 0x0069_7d6a;
pub const EAST_INDIES_RETURN_VA: u32 = 0x0069_7da4;
pub const REGIONS_CLEAR_ALL_VA: u32 = 0x0068_0060;

const MAX_CONSECUTIVE_SEED_FAILURES: i32 = 99;
const MAX_LAND_DISTANCE_REJECTIONS: i32 = 1000;
const MIN_REDUCED_ISLAND_AREA: i32 = 20;

/// Caller-owned values installed in `Map` and copied by `Map::make_region`.
///
/// East Indies overwrites `common_factor`, `goody_factor`, and `climate` after
/// `grow_region`, but the seed-time values and `flags` still come from these fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EastIndiesRegionDefaults {
    pub common_factor: i32,
    pub goody_factor: i32,
    pub flags: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EastIndiesTailCall {
    /// Number of player regions already present.  The next island is `players + 1`.
    pub player_regions: i32,
    pub region_defaults: EastIndiesRegionDefaults,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EastIndiesDirectDrawKind {
    IslandCountParity,
    CandidateX,
    CandidateY,
    NextIslandArea,
}

/// One direct `Random::get(0, 0xffff)` issued by the style virtual.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EastIndiesDirectDraw {
    pub kind: EastIndiesDirectDrawKind,
    pub call_va: u32,
    pub state_before: i32,
    pub raw: i32,
    pub state_after: i32,
}

/// Complete chronological RNG span, including helper-owned draws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EastIndiesRngSpanKind {
    Direct(EastIndiesDirectDrawKind),
    GrowValid { region: i32, x: i32, y: i32 },
    GrowRegion { region: i32, target_area: i32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EastIndiesRngSpan {
    pub kind: EastIndiesRngSpanKind,
    pub state_before: i32,
    pub state_after: i32,
    /// Exact instruction sites reached inside this span, in execution order.
    pub call_sites: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EastIndiesIslandReceipt {
    pub region: i32,
    pub x: i32,
    pub y: i32,
    pub target_area: i32,
    pub spacing_area: i32,
    pub required_land_distance: i32,
    pub actual_land_distance: i32,
    pub land_distance_rejections: i32,
    pub seed_failures_before: i32,
    pub coord_capacity_before: i32,
    pub coord_capacity_after: i32,
    pub growth: GrowRegionReceipt,
    pub common_factor: i32,
    pub goody_factor: i32,
    pub flags: i32,
    pub climate: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EastIndiesAreaAdjustmentKind {
    CollapseToMinimum,
    ReduceMinimum,
}

/// Adaptive branch reached only after 1,000 land-distance rejections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EastIndiesAreaAdjustment {
    pub kind: EastIndiesAreaAdjustmentKind,
    pub target_before: i32,
    pub minimum_before: i32,
    pub maximum_before: i32,
    pub target_after: i32,
    pub minimum_after: i32,
    pub maximum_after: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EastIndiesTailReturn {
    AllIslandsPlaced,
    ConsecutiveSeedFailureBudget,
    ReducedAreaBelowTwenty,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EastIndiesTailReceipt {
    pub entry_va: u32,
    pub return_va: u32,
    pub common_continuation_va: u32,
    pub rng_initial: i32,
    pub rng_final: i32,
    pub requested_islands: i32,
    pub placed_islands: i32,
    /// Native returns even when its two bounded escape paths leave work undone.
    pub remaining_islands: i32,
    pub return_reason: EastIndiesTailReturn,
    pub direct_draws: Vec<EastIndiesDirectDraw>,
    pub rng_chronology: Vec<EastIndiesRngSpan>,
    pub grow_valid_calls: Vec<GrowValidReceipt>,
    pub islands: Vec<EastIndiesIslandReceipt>,
    pub area_adjustments: Vec<EastIndiesAreaAdjustment>,
    /// Checksum sections changed by this exact tranche; this is diagnostic only.
    pub world_sections_changed: Vec<WorldSection>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EastIndiesTailError {
    InvalidWorldShape {
        xs: i32,
        ys: i32,
        size: i32,
        wdata_len: usize,
    },
    InvalidPlayerRegions {
        player_regions: i32,
        regions_land: i32,
    },
    MissingPlayerRegion {
        region: i32,
        size: i32,
        coords: usize,
    },
    LandRegionCapacity {
        player_regions: i32,
        requested_islands: i32,
    },
    InvalidIslandCount {
        player_regions: i32,
        requested_islands: i32,
    },
    InvalidRegionCoordStorage {
        region: i32,
        length: usize,
        capacity: i32,
        increment: i16,
    },
    Growth(GrowRegionError),
}

/// Execute `0x00697b72..0x00697da9` transactionally.
///
/// Rust-side validation failures commit neither World, Regions, RNG, nor the persistent
/// `Map+0x64` edge-jitter word carried in `growth_config`.  Native bounded exits are
/// successful receipts, because retail returns from the virtual on both paths.
pub fn execute_east_indies_tail(
    world: &mut World,
    regions: &mut Regions,
    rng: &mut Random,
    growth_config: &mut MapGrowthConfig,
    call: EastIndiesTailCall,
) -> Result<EastIndiesTailReceipt, EastIndiesTailError> {
    validate_entry(world, regions, growth_config, call)?;
    let before = world.checksum_sections();
    let mut next_world = world.clone();
    let mut next_regions = regions.clone();
    let mut next_rng = *rng;
    let mut next_config = growth_config.clone();
    let mut receipt = execute_body(
        &mut next_world,
        &mut next_regions,
        &mut next_rng,
        &mut next_config,
        call,
    )?;
    let after = next_world.checksum_sections();
    receipt.world_sections_changed = before.differing_sections(&after);
    *world = next_world;
    *regions = next_regions;
    *rng = next_rng;
    *growth_config = next_config;
    Ok(receipt)
}

fn validate_entry(
    world: &World,
    regions: &Regions,
    growth_config: &MapGrowthConfig,
    call: EastIndiesTailCall,
) -> Result<(), EastIndiesTailError> {
    let expected = i64::from(world.xs).checked_mul(i64::from(world.ys));
    if world.xs < 2
        || world.ys < 2
        || world.size != world.xs.saturating_mul(world.ys)
        || expected != Some(world.wdata.len() as i64)
    {
        return Err(EastIndiesTailError::InvalidWorldShape {
            xs: world.xs,
            ys: world.ys,
            size: world.size,
            wdata_len: world.wdata.len(),
        });
    }
    if call.player_regions <= 0
        || call.player_regions as usize >= LAND_REGION_COUNT
        || regions.land < call.player_regions
    {
        return Err(EastIndiesTailError::InvalidPlayerRegions {
            player_regions: call.player_regions,
            regions_land: regions.land,
        });
    }
    for region in 1..=call.player_regions {
        let row = &regions.list[region as usize];
        if row.size <= 0 || row.coords.items.is_empty() {
            return Err(EastIndiesTailError::MissingPlayerRegion {
                region,
                size: row.size,
                coords: row.coords.items.len(),
            });
        }
    }
    for (field, value, valid) in [
        (
            "avoid_continent",
            growth_config.avoid_continent,
            (0..=64).contains(&growth_config.avoid_continent),
        ),
        (
            "base_edge",
            growth_config.base_edge,
            (0..=3).contains(&growth_config.base_edge),
        ),
        (
            "edge_jitter",
            growth_config.edge_jitter,
            (0..=4).contains(&growth_config.edge_jitter),
        ),
    ] {
        if !valid {
            return Err(EastIndiesTailError::Growth(
                GrowRegionError::InvalidMapField { field, value },
            ));
        }
    }
    Ok(())
}

fn execute_body(
    world: &mut World,
    regions: &mut Regions,
    rng: &mut Random,
    growth_config: &mut MapGrowthConfig,
    call: EastIndiesTailCall,
) -> Result<EastIndiesTailReceipt, EastIndiesTailError> {
    let rng_initial = rng.state();
    let mut direct_draws = Vec::new();
    let mut rng_chronology = Vec::new();
    let parity = direct_draw(
        rng,
        &mut direct_draws,
        &mut rng_chronology,
        EastIndiesDirectDrawKind::IslandCountParity,
        EAST_INDIES_NONPLAYER_ISLANDS_VA,
    ) & 1;
    let requested_islands = world
        .xs
        .wrapping_div(12)
        .wrapping_sub(call.player_regions)
        .wrapping_add(8)
        .wrapping_add(parity);
    if requested_islands < 0 {
        return Err(EastIndiesTailError::InvalidIslandCount {
            player_regions: call.player_regions,
            requested_islands,
        });
    }
    if requested_islands > 0
        && call.player_regions.saturating_add(requested_islands) as usize >= LAND_REGION_COUNT
    {
        return Err(EastIndiesTailError::LandRegionCapacity {
            player_regions: call.player_regions,
            requested_islands,
        });
    }

    let mut maximum_area = (world.size / 20).max(90);
    let mut minimum_area = (world.size / 40).max(24);
    let mut target_area = maximum_area;
    let mut remaining = requested_islands;
    let mut current_region = call.player_regions;
    let mut seed_failures = 0;
    let mut grow_valid_calls = Vec::new();
    let mut islands = Vec::new();
    let mut area_adjustments = Vec::new();
    let circle = circle_table();
    let return_reason;

    if remaining == 0 {
        return_reason = EastIndiesTailReturn::AllIslandsPlaced;
    } else {
        'islands: loop {
            seed_failures += 1;
            if seed_failures > MAX_CONSECUTIVE_SEED_FAILURES {
                return_reason = EastIndiesTailReturn::ConsecutiveSeedFailureBudget;
                break;
            }

            let spacing_area = target_area.wrapping_mul(7) / 22;
            let required_land_distance = ((growth_config.avoid_continent + 1) / 2)
                .wrapping_add(integer_sqrt_floor(spacing_area))
                .max(3);
            let mut land_rejections = 0;
            let accepted = loop {
                let raw_x = direct_draw(
                    rng,
                    &mut direct_draws,
                    &mut rng_chronology,
                    EastIndiesDirectDrawKind::CandidateX,
                    EAST_INDIES_ISLAND_X_RNG_VA,
                );
                let raw_y = direct_draw(
                    rng,
                    &mut direct_draws,
                    &mut rng_chronology,
                    EastIndiesDirectDrawKind::CandidateY,
                    EAST_INDIES_ISLAND_Y_RNG_VA,
                );
                let x = raw_x % world.xs;
                let y = raw_y % world.ys;
                let distance = world.land_dist(&circle, WCoord(x), WCoord(y), true);
                if distance >= required_land_distance {
                    break Some((x, y, distance));
                }
                land_rejections += 1;
                if land_rejections >= MAX_LAND_DISTANCE_REJECTIONS {
                    break None;
                }
            };

            let Some((x, y, actual_land_distance)) = accepted else {
                let target_before = target_area;
                let minimum_before = minimum_area;
                let maximum_before = maximum_area;
                let reduce_minimum = target_area <= minimum_area;
                maximum_area = minimum_area;
                target_area = minimum_area;
                let kind = if reduce_minimum {
                    minimum_area = minimum_area.wrapping_mul(13) / 16;
                    maximum_area = minimum_area;
                    target_area = minimum_area;
                    EastIndiesAreaAdjustmentKind::ReduceMinimum
                } else {
                    EastIndiesAreaAdjustmentKind::CollapseToMinimum
                };
                area_adjustments.push(EastIndiesAreaAdjustment {
                    kind,
                    target_before,
                    minimum_before,
                    maximum_before,
                    target_after: target_area,
                    minimum_after: minimum_area,
                    maximum_after: maximum_area,
                });
                if reduce_minimum && minimum_area < MIN_REDUCED_ISLAND_AREA {
                    return_reason = EastIndiesTailReturn::ReducedAreaBelowTwenty;
                    break 'islands;
                }
                continue;
            };

            let region = current_region + 1;
            let valid_call = GrowValidCall { region, x, y };
            let helper_before = rng.state();
            let valid = execute_grow_valid(world, regions, rng, growth_config, &valid_call)
                .map_err(EastIndiesTailError::Growth)?;
            rng_chronology.push(EastIndiesRngSpan {
                kind: EastIndiesRngSpanKind::GrowValid { region, x, y },
                state_before: helper_before,
                state_after: rng.state(),
                call_sites: valid.rng_sites.clone(),
            });
            let seed_accepted = valid.retail_return != 0;
            grow_valid_calls.push(valid);

            if seed_accepted {
                let capacity_before =
                    seed_region(world, regions, region, x, y, call.region_defaults)?;
                let growth_call = GrowRegionCall {
                    region,
                    target_area,
                    max_distance: world.xs,
                    anchor_x: -1,
                    anchor_y: -1,
                    return_partial_size: 0,
                };
                let helper_before = rng.state();
                let growth = execute_grow_region(world, regions, rng, growth_config, &growth_call)
                    .map_err(EastIndiesTailError::Growth)?;
                rng_chronology.push(EastIndiesRngSpan {
                    kind: EastIndiesRngSpanKind::GrowRegion {
                        region,
                        target_area,
                    },
                    state_before: helper_before,
                    state_after: rng.state(),
                    call_sites: growth.rng_sites.clone(),
                });

                // Caller stores at `0x00697d36..0x00697d54`, after grow_region.
                let row = &mut regions.list[region as usize];
                row.common_factor = 5;
                row.goody_factor = 5;
                row.climate = 1;
                let island = EastIndiesIslandReceipt {
                    region,
                    x,
                    y,
                    target_area,
                    spacing_area,
                    required_land_distance,
                    actual_land_distance,
                    land_distance_rejections: land_rejections,
                    seed_failures_before: seed_failures,
                    coord_capacity_before: capacity_before,
                    coord_capacity_after: row.coords.capacity,
                    growth,
                    common_factor: row.common_factor,
                    goody_factor: row.goody_factor,
                    flags: row.flags,
                    climate: row.climate,
                };
                islands.push(island);
                current_region = region;
                remaining -= 1;
                seed_failures = 0;
            }

            let range = maximum_area - minimum_area;
            target_area = if range <= 1 {
                minimum_area
            } else {
                minimum_area
                    + direct_draw(
                        rng,
                        &mut direct_draws,
                        &mut rng_chronology,
                        EastIndiesDirectDrawKind::NextIslandArea,
                        EAST_INDIES_ISLAND_AREA_RNG_VA,
                    ) % range
            };
            if remaining == 0 {
                return_reason = EastIndiesTailReturn::AllIslandsPlaced;
                break;
            }
        }
    }

    Ok(EastIndiesTailReceipt {
        entry_va: EAST_INDIES_NONPLAYER_ISLANDS_VA,
        return_va: EAST_INDIES_RETURN_VA,
        common_continuation_va: REGIONS_CLEAR_ALL_VA,
        rng_initial,
        rng_final: rng.state(),
        requested_islands,
        placed_islands: islands.len() as i32,
        remaining_islands: remaining,
        return_reason,
        direct_draws,
        rng_chronology,
        grow_valid_calls,
        islands,
        area_adjustments,
        world_sections_changed: Vec::new(),
    })
}

fn direct_draw(
    rng: &mut Random,
    direct_draws: &mut Vec<EastIndiesDirectDraw>,
    chronology: &mut Vec<EastIndiesRngSpan>,
    kind: EastIndiesDirectDrawKind,
    call_va: u32,
) -> i32 {
    let state_before = rng.state();
    let raw = rng.get(0, 0xffff);
    let draw = EastIndiesDirectDraw {
        kind,
        call_va,
        state_before,
        raw,
        state_after: rng.state(),
    };
    direct_draws.push(draw);
    chronology.push(EastIndiesRngSpan {
        kind: EastIndiesRngSpanKind::Direct(kind),
        state_before,
        state_after: rng.state(),
        call_sites: vec![call_va],
    });
    raw
}

fn seed_region(
    world: &mut World,
    regions: &mut Regions,
    region: i32,
    x: i32,
    y: i32,
    defaults: EastIndiesRegionDefaults,
) -> Result<i32, EastIndiesTailError> {
    let row = &mut regions.list[region as usize];
    let capacity_before = row.coords.capacity;
    let length = row.coords.items.len();
    if capacity_before < 0 || length > i32::MAX as usize {
        return Err(EastIndiesTailError::InvalidRegionCoordStorage {
            region,
            length,
            capacity: capacity_before,
            increment: row.coords.increment,
        });
    }
    if length as i32 >= capacity_before {
        let increase = if row.coords.increment < 0 {
            capacity_before.max(4)
        } else {
            i32::from(row.coords.increment)
        };
        let Some(next_capacity) = capacity_before.checked_add(increase) else {
            return Err(EastIndiesTailError::InvalidRegionCoordStorage {
                region,
                length,
                capacity: capacity_before,
                increment: row.coords.increment,
            });
        };
        if increase <= 0 || next_capacity <= length as i32 {
            return Err(EastIndiesTailError::InvalidRegionCoordStorage {
                region,
                length,
                capacity: capacity_before,
                increment: row.coords.increment,
            });
        }
        row.coords.capacity = next_capacity;
    }

    let cell = world.wdata_mut(x, y);
    cell.land = land::FERTILE;
    cell.land_sub = 0;
    cell.region = region as i16;
    row.coords.items.push((x, y));
    row.size = 1;
    row.common_factor = defaults.common_factor;
    row.goody_factor = defaults.goody_factor;
    row.flags = defaults.flags;
    regions.land = regions.land.max(region);
    Ok(capacity_before)
}

/// Newton loop at `0x00697bdb..0x00697bf5`; positive inputs use truncating `idiv`.
fn integer_sqrt_floor(value: i32) -> i32 {
    if value <= 1 {
        return value.max(0);
    }
    let mut previous = value >> 1;
    loop {
        let next = (previous + value / previous) >> 1;
        if next >= previous {
            return previous;
        }
        previous = next;
    }
}
