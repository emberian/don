//! Executable replay reconstruction of the admitted map-style continent prefix.
//!
//! The retail driver seeds the main `Random`, resolves `BASE_EDGE`, then calls
//! one virtual `Map*::make_continents` implementation.  This module executes
//! every instruction whose inputs and effects are represented by the replay,
//! the admitted map-style XML, and `don_sim::systems::map_terrain::World`.
//! It stops at the first unported geometry/region primitive and returns that
//! call as data; it never skips the primitive and consumes later RNG draws.

use crate::growth::{
    execute_grow_region, GrowRegionCall, GrowRegionError, GrowRegionReceipt, MapGrowthConfig,
};
use crate::initial::InitialWorldgenInputs;
use crate::map_style::{MapStyleStaticData, StaticXmlEntry, MAP_MAKE_ORIENTATION_RNG_VA};
use don_sim::rng::Random;
use don_sim::systems::map_terrain::{land, wflag, WCoord, World};
use don_sim::systems::regions::Regions;
use don_sim::trig::{cosx, sinx};

pub const REGIONS_FIND_ALL_VA: u32 = 0x0068_0060;
pub const MAP_FILL_CONT_VA: u32 = 0x0068_a960;
pub const MAP_LAND_DIST_VA: u32 = 0x0069_d970;
pub const MAP_MAKE_REGION_VA: u32 = 0x0069_d3f0;
pub const MAP_GROW_REGION_VA: u32 = 0x0069_c600;
pub const MAP_MAKE_COASTLINES_VA: u32 = 0x0068_b890;
pub const EAST_INDIES_NONPLAYER_ISLANDS_VA: u32 = 0x0069_7b72;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionSeedCall {
    pub region: i32,
    pub x: i32,
    pub y: i32,
    pub area: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LandDistanceCall {
    pub x: i32,
    pub y: i32,
    pub region: i32,
    pub required_distance: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionSeedReceipt {
    pub call: RegionSeedCall,
    pub coord_capacity_before: i32,
    pub coord_capacity_after: i32,
    pub common_factor: i32,
    pub goody_factor: i32,
    pub flags: i32,
}

/// First call not executed by a style prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContinentStop {
    /// The style virtual returned; the common driver next rebuilds regions.
    HookComplete { next_va: u32 },
    /// `Map::make_region` is the first unavailable map mutator.
    MakeRegion {
        primitive_va: u32,
        call: RegionSeedCall,
    },
    /// The exact seed-cell helper completed; region flood/growth is next.
    GrowRegion {
        primitive_va: u32,
        call: GrowRegionCall,
    },
    /// Mediterranean has rebuilt its first connected-region pass; coastline
    /// carving is the next unported map mutation.
    MakeCoastlines { primitive_va: u32, passes: i32 },
    /// East Indies completed both player-region growth passes. The next stage
    /// chooses and grows non-player islands.
    EastIndiesNonplayerIslands { next_rng_va: u32 },
    /// Retail abandons this generation pass and restarts the style virtual.
    RetryGeneration {
        failed_region: i32,
        retail_return: i32,
    },
    /// Great Lakes needs the current generated-land distance before its retry
    /// branch can be selected.
    LandDistance {
        primitive_va: u32,
        call: LandDistanceCall,
    },
    /// East Meets West first partitions players by team into continent shares.
    FillCont {
        primitive_va: u32,
        active_teams: Vec<u8>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContinentReceipt {
    pub map_style: u8,
    pub make_continents_va: u32,
    pub orientation: i32,
    pub rng_initial: i32,
    pub rng_final: i32,
    /// Call sites actually executed, in order. Called helpers are deliberately
    /// absent until the helper itself is executed.
    pub direct_rng_sites: Vec<u32>,
    /// One-based pass through the style's generation path. Every represented
    /// stop occurs during the first pass, before a retry decision.
    pub retry_attempt: u32,
    pub world_wiped: bool,
    pub world_inverted: bool,
    pub regions_cleared: u32,
    pub region_seeds: Vec<RegionSeedReceipt>,
    pub region_growths: Vec<GrowRegionReceipt>,
    pub starts_added: usize,
    pub start_min: Option<i32>,
    pub stop: ContinentStop,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContinentError {
    SelectorMismatch {
        replay: u8,
        style: u8,
    },
    UnsupportedStyle {
        map_style: u8,
    },
    ZeroPlayers,
    PlayerLayoutMismatch {
        declared: u8,
        active: usize,
    },
    MapPrefixMismatch {
        expected_edge: i32,
        actual_xs: i32,
        actual_ys: i32,
        expected_seed: i32,
        actual_seed: i32,
    },
    InvalidMapDimensions {
        xs: i32,
        ys: i32,
    },
    InvalidRegionSeed {
        region: i32,
        x: i32,
        y: i32,
    },
    InvalidRegionCoordStorage {
        region: i32,
        length: usize,
        capacity: i32,
        increment: i16,
    },
    MissingMapParameter {
        tag: &'static str,
        attribute: &'static str,
    },
    InvalidMapParameter {
        tag: &'static str,
        attribute: &'static str,
        value: String,
    },
    RegionGrowth(GrowRegionError),
    RegionRebuild(don_sim::systems::regions::RegionsError),
}

/// Execute the largest deterministic prefix of one admitted style virtual.
///
/// Validation is transactional: every fail-closed input check happens before
/// either `World::wipe` or an RNG draw. Once execution begins the receipt names
/// the exact next primitive, so no caller can mistake a partial map for a
/// completed one.
pub fn execute_continent_prefix(
    inputs: &InitialWorldgenInputs,
    style: &MapStyleStaticData,
    world: &mut World,
) -> Result<ContinentReceipt, ContinentError> {
    let mut regions = Regions::default();
    execute_continent_prefix_with_regions(inputs, style, world, &mut regions)
}

/// Stateful form used by replay reconstruction. `Regions` is retained beside
/// the generated world so `make_region` and later growth stages have one
/// authoritative caller-owned store.
pub fn execute_continent_prefix_with_regions(
    inputs: &InitialWorldgenInputs,
    style: &MapStyleStaticData,
    world: &mut World,
    regions: &mut Regions,
) -> Result<ContinentReceipt, ContinentError> {
    if style.identity.ordinal != inputs.map_style {
        return Err(ContinentError::SelectorMismatch {
            replay: inputs.map_style,
            style: style.identity.ordinal,
        });
    }
    let make_continents_va =
        style
            .identity
            .make_continents_va
            .ok_or(ContinentError::UnsupportedStyle {
                map_style: inputs.map_style,
            })?;
    if !matches!(inputs.map_style, 6 | 9 | 12 | 14 | 18 | 19) {
        return Err(ContinentError::UnsupportedStyle {
            map_style: inputs.map_style,
        });
    }
    let players = inputs.active_slots.len();
    if players == 0 {
        return Err(ContinentError::ZeroPlayers);
    }
    if players > u8::MAX as usize
        || inputs.active_who.len() != players
        || inputs.active_teams.len() != players
    {
        return Err(ContinentError::PlayerLayoutMismatch {
            declared: players.min(u8::MAX as usize) as u8,
            active: inputs.active_who.len(),
        });
    }
    let players = players as u8;
    let edge = inputs.map_edge_world_cells.unwrap_or(0);
    let seed = inputs.seed as i32;
    if (world.xs, world.ys, world.seed) != (edge, edge, seed) {
        return Err(ContinentError::MapPrefixMismatch {
            expected_edge: edge,
            actual_xs: world.xs,
            actual_ys: world.ys,
            expected_seed: seed,
            actual_seed: world.seed,
        });
    }
    // Every supported retail map is much larger, but two cells is the exact
    // safety floor for the first style operations (`% xs`, radii, and edge
    // tests). Reject malformed direct API inputs before wipe or RNG mutation.
    if world.xs < 2 || world.ys < 2 {
        return Err(ContinentError::InvalidMapDimensions {
            xs: world.xs,
            ys: world.ys,
        });
    }

    // `Map::make_region` reads these installed Map fields. Resolve them before
    // any wipe or RNG draw so missing static data remains transactional.
    let region_defaults = if matches!(inputs.map_style, 12 | 18) {
        RegionSeedDefaults {
            common_factor: map_int(style, "COMMON_RESOURCES", "value")?,
            goody_factor: map_int(style, "GOODY_BOXES", "value")?,
            flags: 0,
        }
    } else {
        RegionSeedDefaults {
            common_factor: 0,
            goody_factor: 0,
            flags: 0,
        }
    };

    // Map::Map initializes orientation to -1. load_map_data resolves BASE_EDGE
    // from the selected/default MAP data before this branch.
    let base_edge = map_int(style, "BASE_EDGE", "value")?;
    if !(-1..=3).contains(&base_edge) {
        return Err(ContinentError::InvalidMapParameter {
            tag: "BASE_EDGE",
            attribute: "value",
            value: base_edge.to_string(),
        });
    }
    let mut rng = Random::new(seed);
    let mut sites = Vec::new();
    let orientation = if base_edge < 0 {
        sites.push(MAP_MAKE_ORIENTATION_RNG_VA);
        rng.get(0, 0xffff) & 3
    } else {
        base_edge
    };
    let rng_initial = seed;

    // The geometry algorithms are written against clones so every Rust-side
    // validation failure leaves both authoritative stores unchanged.
    let mut next_world = world.clone();
    let mut next_regions = regions.clone();
    let partial = match inputs.map_style {
        6 | 9 => old_world_or_himalayas(
            inputs.map_style,
            players,
            &mut next_world,
            &mut next_regions,
            &mut rng,
            &mut sites,
        ),
        12 => mediterranean(
            inputs,
            orientation,
            &mut next_world,
            &mut next_regions,
            &mut rng,
            &mut sites,
            style,
            region_defaults,
        )?,
        14 => great_lakes(
            players,
            inputs.map_size,
            &mut next_world,
            &mut next_regions,
            &mut rng,
            &mut sites,
            style,
        )?,
        18 => east_indies(
            players,
            inputs.map_size,
            orientation,
            &mut next_world,
            &mut next_regions,
            &mut rng,
            &mut sites,
            style,
            region_defaults,
        )?,
        19 => east_meets_west(inputs, &mut next_world, &mut next_regions, &mut sites),
        _ => unreachable!("admitted above"),
    };

    *world = next_world;
    *regions = next_regions;

    Ok(ContinentReceipt {
        map_style: inputs.map_style,
        make_continents_va,
        orientation,
        rng_initial,
        rng_final: rng.state(),
        direct_rng_sites: sites,
        retry_attempt: 1,
        world_wiped: true,
        world_inverted: partial.world_inverted,
        regions_cleared: partial.regions_cleared,
        region_seeds: partial.region_seeds,
        region_growths: partial.region_growths,
        starts_added: partial.starts_added,
        start_min: partial.start_min,
        stop: partial.stop,
    })
}

struct PartialReceipt {
    world_inverted: bool,
    regions_cleared: u32,
    region_seeds: Vec<RegionSeedReceipt>,
    region_growths: Vec<GrowRegionReceipt>,
    starts_added: usize,
    start_min: Option<i32>,
    stop: ContinentStop,
}

#[derive(Copy, Clone)]
struct RegionSeedDefaults {
    common_factor: i32,
    goody_factor: i32,
    flags: i32,
}

fn old_world_or_himalayas(
    map_style: u8,
    players: u8,
    world: &mut World,
    regions: &mut Regions,
    rng: &mut Random,
    sites: &mut Vec<u32>,
) -> PartialReceipt {
    world.wipe();
    regions.clear_all(world);
    invert_land(world);
    // `Map::invert_land` ends with its own `Regions::clear_all` call.
    regions.clear_all(world);

    let radius_half = world.xs.min(world.ys) / 2;
    let radius = ((radius_half * 80) / 100).min(radius_half - 4);
    let increment = (u32::MAX / players as u32) as i32;
    let style_sites: &[u32] = if map_style == 6 {
        &crate::map_style::OLD_WORLD_DIRECT_RNG_SITES
    } else {
        &crate::map_style::HIMALAYAS_DIRECT_RNG_SITES
    };
    let a = draw(rng, sites, style_sites[0]);
    let b = draw(rng, sites, style_sites[1]);
    let mut angle = a.wrapping_shl(16).wrapping_add(b);
    let center_x = world.xs / 2 + (draw(rng, sites, style_sites[2]) & 1);
    let center_y = world.ys / 2 + (draw(rng, sites, style_sites[3]) & 1);
    let start_min = (world.xs / 16).max(4);

    for _ in 0..players {
        angle = angle.wrapping_add(increment);
        let (x, y) = project(center_x, center_y, angle, radius);
        world.add_starting_location(WCoord(x), WCoord(y));
    }
    PartialReceipt {
        world_inverted: true,
        regions_cleared: 2,
        region_seeds: Vec::new(),
        region_growths: Vec::new(),
        starts_added: players as usize,
        start_min: Some(start_min),
        stop: ContinentStop::HookComplete {
            next_va: REGIONS_FIND_ALL_VA,
        },
    }
}

fn mediterranean(
    inputs: &InitialWorldgenInputs,
    orientation: i32,
    world: &mut World,
    regions: &mut Regions,
    rng: &mut Random,
    sites: &mut Vec<u32>,
    style: &MapStyleStaticData,
    defaults: RegionSeedDefaults,
) -> Result<PartialReceipt, ContinentError> {
    world.wipe();
    regions.clear_all(world);
    let style_sites = &crate::map_style::MEDITERRANEAN_DIRECT_RNG_SITES;
    let min_dim = (world.xs.min(world.ys) & !1) / 3;
    let a = draw(rng, sites, style_sites[0]);
    let b = draw(rng, sites, style_sites[1]);
    let _angle = a.wrapping_shl(16).wrapping_add(b);
    let x = world.xs / 2 + (draw(rng, sites, style_sites[2]) & 1);
    let y = world.ys / 2 + (draw(rng, sites, style_sites[3]) & 1);
    let area = min_dim.wrapping_mul(min_dim).wrapping_mul(3);
    let seed = RegionSeedCall {
        region: 1,
        x,
        y,
        area,
    };
    let seed_receipt = apply_make_region(world, regions, &seed, defaults)?;
    let mut growth_config = map_growth_config(
        style,
        world,
        inputs.active_slots.len() as u8,
        inputs.map_size,
        orientation,
    )?;
    let growth_call = GrowRegionCall {
        region: 1,
        target_area: area,
        max_distance: min_dim,
        anchor_x: -1,
        anchor_y: -1,
        return_partial_size: 0,
    };
    let growth = execute_grow_region(world, regions, rng, &mut growth_config, &growth_call)
        .map_err(ContinentError::RegionGrowth)?;
    sites.extend_from_slice(&growth.rng_sites);
    if growth.retail_return != 0 {
        return Ok(PartialReceipt {
            world_inverted: false,
            regions_cleared: 1,
            region_seeds: vec![seed_receipt],
            region_growths: vec![growth.clone()],
            starts_added: 0,
            start_min: None,
            stop: ContinentStop::RetryGeneration {
                failed_region: 1,
                retail_return: growth.retail_return,
            },
        });
    }
    regions
        .rebuild_after_coastlines(world)
        .map_err(ContinentError::RegionRebuild)?;
    Ok(PartialReceipt {
        world_inverted: false,
        regions_cleared: 2,
        region_seeds: vec![seed_receipt],
        region_growths: vec![growth],
        starts_added: 0,
        start_min: None,
        stop: ContinentStop::MakeCoastlines {
            primitive_va: MAP_MAKE_COASTLINES_VA,
            passes: 2,
        },
    })
}

fn great_lakes(
    players: u8,
    map_size: u8,
    world: &mut World,
    regions: &mut Regions,
    rng: &mut Random,
    sites: &mut Vec<u32>,
    style: &MapStyleStaticData,
) -> Result<PartialReceipt, ContinentError> {
    let land_area = scale_land_area(
        map_int(style, "AVOID_CONTINENT", "scalevalue")?,
        players,
        map_size,
    )
    .max(6);
    world.wipe();
    regions.clear_all(world);
    let style_sites = &crate::map_style::GREAT_LAKES_DIRECT_RNG_SITES;
    let a = draw(rng, sites, style_sites[0]);
    let b = draw(rng, sites, style_sites[1]);
    let _angle = a.wrapping_shl(16).wrapping_add(b);
    let mut region_area = world.size / 20;
    if region_area < 90 {
        region_area = 90;
    }
    let mut min_region_area = world.size / 40;
    if min_region_area < 24 {
        min_region_area = 24;
    }
    let remaining = world.xs / 12 + (draw(rng, sites, style_sites[2]) & 1);
    debug_assert!(remaining > 0);

    // First pass through the retry loop, stopping immediately before land_dist.
    let shrunk = region_area.wrapping_mul(7) / 22;
    let required_distance = ((land_area + 1) / 2 + integer_sqrt_floor(shrunk)).max(3);
    let x = draw(rng, sites, style_sites[3]) % world.xs;
    let y = draw(rng, sites, style_sites[4]) % world.ys;
    let _ = min_region_area; // feeds the later grow-size retry, after this boundary.
    Ok(PartialReceipt {
        world_inverted: false,
        regions_cleared: 1,
        region_seeds: Vec::new(),
        region_growths: Vec::new(),
        starts_added: 0,
        start_min: None,
        stop: ContinentStop::LandDistance {
            primitive_va: MAP_LAND_DIST_VA,
            call: LandDistanceCall {
                x,
                y,
                region: 1,
                required_distance,
            },
        },
    })
}

fn east_indies(
    players: u8,
    map_size: u8,
    orientation: i32,
    world: &mut World,
    regions: &mut Regions,
    rng: &mut Random,
    sites: &mut Vec<u32>,
    style: &MapStyleStaticData,
    defaults: RegionSeedDefaults,
) -> Result<PartialReceipt, ContinentError> {
    // The selected East Indies XML supplies plain integer edge values; this
    // exact maximum is read before the first make_region call.
    let margin = [
        "AVOID_EDGE_0",
        "AVOID_EDGE_1",
        "AVOID_EDGE_2",
        "AVOID_EDGE_3",
    ]
    .into_iter()
    .map(|tag| map_int(style, tag, "scalevalue"))
    .collect::<Result<Vec<_>, _>>()?
    .into_iter()
    .max()
    .unwrap_or(0)
    .wrapping_add(4);
    let _land_area = scale_land_area(
        map_int(style, "AVOID_CONTINENT", "scalevalue")?,
        players,
        map_size,
    )
    .max(4);
    let region_area = scale_land_area(250, players, map_size);
    let mut growth_config = map_growth_config(style, world, players, map_size, orientation)?;

    world.wipe();
    regions.clear_all(world);
    let style_sites = &crate::map_style::EAST_INDIES_DIRECT_RNG_SITES;
    let first = draw(rng, sites, style_sites[0]);
    let min_dim = world.xs.min(world.ys);
    let (mut angle, mut radius) = if players < 4 {
        let rem = first & 3;
        let angle = rem.wrapping_mul(0x3fff_ffff).wrapping_add(0x1fff_ffff);
        let radius = if players == 3 {
            (min_dim / 2).wrapping_mul(7) / 8
        } else {
            min_dim / 2
        };
        (angle, radius)
    } else {
        let second = draw(rng, sites, style_sites[1]);
        (
            first.wrapping_shl(16).wrapping_add(second),
            (min_dim / 2).wrapping_mul(7) / 8,
        )
    };
    let initial_radius = radius;
    let increment = (u32::MAX / players as u32) as i32;
    let center_x = world.xs / 2;
    let center_y = world.ys / 2;
    let mut seeds = Vec::with_capacity(players as usize);
    for region in 1..=players {
        angle = angle.wrapping_add(increment);
        let (x, y) = loop {
            let candidate = project(center_x, center_y, angle, radius);
            if candidate.0 > margin
                && world.xs - candidate.0 > margin
                && candidate.1 > margin
                && world.ys - candidate.1 > margin
            {
                break candidate;
            }
            radius -= 1;
        };
        world.add_starting_location(WCoord(x), WCoord(y));
        let call = RegionSeedCall {
            region: i32::from(region),
            x,
            y,
            area: region_area,
        };
        let mut receipt = apply_make_region(world, regions, &call, defaults)?;
        // Exact caller writes immediately following each make_region call.
        let seeded = &mut regions.list[region as usize];
        seeded.common_factor = 8;
        seeded.goody_factor = 8;
        seeded.flags |= 2;
        seeded.climate = 0;
        receipt.common_factor = seeded.common_factor;
        receipt.goody_factor = seeded.goody_factor;
        receipt.flags = seeded.flags;
        seeds.push(receipt);
    }
    let mut growths = Vec::with_capacity(players as usize * 2);
    let max_distance = initial_radius / 2;
    for target_area in [region_area / 3, region_area] {
        for region in 1..=players {
            let call = GrowRegionCall {
                region: i32::from(region),
                target_area,
                max_distance,
                anchor_x: -1,
                anchor_y: -1,
                return_partial_size: 0,
            };
            let growth = execute_grow_region(world, regions, rng, &mut growth_config, &call)
                .map_err(ContinentError::RegionGrowth)?;
            sites.extend_from_slice(&growth.rng_sites);
            let failed = growth.retail_return != 0;
            let retail_return = growth.retail_return;
            growths.push(growth);
            if failed {
                return Ok(PartialReceipt {
                    world_inverted: false,
                    regions_cleared: 1,
                    region_seeds: seeds,
                    region_growths: growths,
                    starts_added: players as usize,
                    start_min: None,
                    stop: ContinentStop::RetryGeneration {
                        failed_region: i32::from(region),
                        retail_return,
                    },
                });
            }
        }
    }
    Ok(PartialReceipt {
        world_inverted: false,
        regions_cleared: 1,
        region_seeds: seeds,
        region_growths: growths,
        starts_added: players as usize,
        start_min: None,
        stop: ContinentStop::EastIndiesNonplayerIslands {
            next_rng_va: EAST_INDIES_NONPLAYER_ISLANDS_VA,
        },
    })
}

fn east_meets_west(
    inputs: &InitialWorldgenInputs,
    world: &mut World,
    regions: &mut Regions,
    _sites: &mut Vec<u32>,
) -> PartialReceipt {
    world.wipe();
    regions.clear_all(world);
    PartialReceipt {
        world_inverted: false,
        regions_cleared: 1,
        region_seeds: Vec::new(),
        region_growths: Vec::new(),
        starts_added: 0,
        start_min: None,
        stop: ContinentStop::FillCont {
            primitive_va: MAP_FILL_CONT_VA,
            active_teams: inputs.active_teams.clone(),
        },
    }
}

fn draw(rng: &mut Random, sites: &mut Vec<u32>, site: u32) -> i32 {
    sites.push(site);
    rng.get(0, 0xffff)
}

fn project(cx: i32, cy: i32, angle: i32, radius: i32) -> (i32, i32) {
    (
        cx.wrapping_add(sinx(angle, radius)),
        cy.wrapping_sub(cosx(angle, radius)),
    )
}

/// `Map::invert_land` `0x0069d880`, including the overlapping 16-bit writes.
fn invert_land(world: &mut World) {
    for cell in &mut world.wdata {
        if cell.flags & wflag::WATERHALF == 0
            && (cell.land == land::COASTAL || cell.land == land::OCEAN)
        {
            cell.land = land::FERTILE;
        } else {
            cell.land = land::OCEAN;
        }
        cell.land_sub = 0;
        cell.region = 0;
        cell.region2 = 0;
    }
}

/// Complete `Map::make_region` (`0x0069d3f0`). The final `area` argument is
/// diagnostic/caller state only; the shipped body seeds exactly one cell and
/// one coordinate, while the following `grow_region` consumes the target.
fn apply_make_region(
    world: &mut World,
    regions: &mut Regions,
    call: &RegionSeedCall,
    defaults: RegionSeedDefaults,
) -> Result<RegionSeedReceipt, ContinentError> {
    let Ok(region_index) = usize::try_from(call.region) else {
        return Err(ContinentError::InvalidRegionSeed {
            region: call.region,
            x: call.x,
            y: call.y,
        });
    };
    if region_index >= regions.list.len() || !world.valid_w(call.x, call.y) {
        return Err(ContinentError::InvalidRegionSeed {
            region: call.region,
            x: call.x,
            y: call.y,
        });
    }

    let region = &mut regions.list[region_index];
    let capacity_before = region.coords.capacity;
    let length = region.coords.items.len();
    if capacity_before < 0 || length > i32::MAX as usize {
        return Err(ContinentError::InvalidRegionCoordStorage {
            region: call.region,
            length,
            capacity: capacity_before,
            increment: region.coords.increment,
        });
    }
    if length as i32 >= region.coords.capacity {
        let increase = if region.coords.increment < 0 {
            region.coords.capacity.max(4)
        } else {
            i32::from(region.coords.increment)
        };
        let Some(next_capacity) = region.coords.capacity.checked_add(increase) else {
            return Err(ContinentError::InvalidRegionCoordStorage {
                region: call.region,
                length,
                capacity: capacity_before,
                increment: region.coords.increment,
            });
        };
        if increase <= 0 || next_capacity <= length as i32 {
            return Err(ContinentError::InvalidRegionCoordStorage {
                region: call.region,
                length,
                capacity: capacity_before,
                increment: region.coords.increment,
            });
        }
        region.coords.capacity = next_capacity;
    }

    let cell = world.wdata_mut(call.x, call.y);
    cell.land = land::FERTILE;
    cell.land_sub = 0;
    cell.region = call.region as i16;
    region.coords.items.push((call.x, call.y));
    region.size = 1;
    region.flags = defaults.flags;
    region.common_factor = defaults.common_factor;
    region.goody_factor = defaults.goody_factor;
    regions.land = regions.land.max(call.region);

    Ok(RegionSeedReceipt {
        call: call.clone(),
        coord_capacity_before: capacity_before,
        coord_capacity_after: region.coords.capacity,
        common_factor: region.common_factor,
        goody_factor: region.goody_factor,
        flags: region.flags,
    })
}

fn map_int(
    style: &MapStyleStaticData,
    tag: &'static str,
    attribute: &'static str,
) -> Result<i32, ContinentError> {
    let entry = find_entry(&style.selected_map_entries, tag)
        .or_else(|| find_entry(&style.default_map_entries, tag))
        .ok_or(ContinentError::MissingMapParameter { tag, attribute })?;
    let raw = entry
        .attribute(attribute)
        .ok_or(ContinentError::MissingMapParameter { tag, attribute })?;
    // Plain integer parameters are sufficient for every value consumed before
    // the currently represented stops. SCALE expressions are rejected rather
    // than guessed; styles whose early calls do not consume them need not parse
    // them here.
    raw.trim()
        .parse::<i32>()
        .map_err(|_| ContinentError::InvalidMapParameter {
            tag,
            attribute,
            value: raw.to_owned(),
        })
}

fn map_growth_config(
    style: &MapStyleStaticData,
    world: &World,
    players: u8,
    map_size: u8,
    orientation: i32,
) -> Result<MapGrowthConfig, ContinentError> {
    let avoid_center = map_scaled_int(style, "AVOID_CENTER", "scalevalue", world)?;
    let avoid_continent = scale_land_area(
        map_scaled_int(style, "AVOID_CONTINENT", "scalevalue", world)?,
        players,
        map_size,
    )
    .max(4);
    let mut edge_avoid = [0; 4];
    for (index, value) in edge_avoid.iter_mut().enumerate() {
        let tag = match index {
            0 => "AVOID_EDGE_0",
            1 => "AVOID_EDGE_1",
            2 => "AVOID_EDGE_2",
            3 => "AVOID_EDGE_3",
            _ => unreachable!(),
        };
        *value = map_scaled_int(style, tag, "scalevalue", world)?;
    }
    Ok(MapGrowthConfig {
        avoid_center,
        avoid_continent,
        edge_avoid,
        base_edge: orientation,
        edge_jitter: 0,
    })
}

fn map_scaled_int(
    style: &MapStyleStaticData,
    tag: &'static str,
    attribute: &'static str,
    world: &World,
) -> Result<i32, ContinentError> {
    let entry = find_entry(&style.selected_map_entries, tag)
        .or_else(|| find_entry(&style.default_map_entries, tag))
        .ok_or(ContinentError::MissingMapParameter { tag, attribute })?;
    let raw = entry
        .attribute(attribute)
        .ok_or(ContinentError::MissingMapParameter { tag, attribute })?;
    let fields = raw.split_whitespace().collect::<Vec<_>>();
    let value = fields
        .first()
        .and_then(|field| field.parse::<i32>().ok())
        .ok_or_else(|| ContinentError::InvalidMapParameter {
            tag,
            attribute,
            value: raw.to_owned(),
        })?;
    match fields.as_slice() {
        [_] => Ok(value),
        [_, "SCALE"] => Ok(scale_by_axis(world, value)),
        [_, "AREA"] => Ok(scale_by_area(world, value)),
        _ => Err(ContinentError::InvalidMapParameter {
            tag,
            attribute,
            value: raw.to_owned(),
        }),
    }
}

fn scale_by_axis(world: &World, value: i32) -> i32 {
    if value < 1 {
        0
    } else {
        ((STANDARD_MAP_EDGE / 2).wrapping_add(world.xs.wrapping_mul(value)) / STANDARD_MAP_EDGE)
            .max(1)
    }
}

fn scale_by_area(world: &World, value: i32) -> i32 {
    if value < 1 {
        0
    } else {
        let standard_area = STANDARD_MAP_EDGE * STANDARD_MAP_EDGE;
        ((standard_area / 2).wrapping_add(world.size.wrapping_mul(value)) / standard_area).max(1)
    }
}

const STANDARD_MAP_EDGE: i32 = 70;

fn find_entry<'a>(entries: &'a [StaticXmlEntry], tag: &str) -> Option<&'a StaticXmlEntry> {
    entries.iter().find(|entry| entry.tag == tag)
}

fn player_baseline(map_size: u8) -> i32 {
    match map_size {
        0 | 1 => 2,
        2 => 3,
        3 => 4,
        4 => 6,
        _ => 8,
    }
}

/// `Map::scale_land_area` `0x0068b1e0` / `get_player_overage` `0x0068b280`.
fn scale_land_area(value: i32, players: u8, map_size: u8) -> i32 {
    let baseline = player_baseline(map_size);
    let overage = if players == 0 {
        baseline
    } else {
        (players as i32 - baseline).max(0)
    };
    if overage == 0 {
        return value;
    }
    let ratio = overage as f32 / baseline as f32;
    (value as f32 / ratio) as i32
}

fn integer_sqrt_floor(value: i32) -> i32 {
    if value <= 1 {
        return value.max(0);
    }
    let mut current = value >> 1;
    loop {
        let next = (current + value / current) >> 1;
        if next >= current {
            return current;
        }
        current = next;
    }
}
