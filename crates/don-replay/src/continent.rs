//! Executable replay reconstruction of the admitted map-style continent prefix.
//!
//! The retail driver seeds the main `Random`, resolves `BASE_EDGE`, then calls
//! one virtual `Map*::make_continents` implementation.  This module executes
//! every instruction whose inputs and effects are represented by the replay,
//! the admitted map-style XML, and `don_sim::systems::map_terrain::World`.
//! It stops at the first unported geometry/region primitive and returns that
//! call as data; it never skips the primitive and consumes later RNG draws.

use crate::initial::InitialWorldgenInputs;
use crate::map_style::{MapStyleStaticData, StaticXmlEntry, MAP_MAKE_ORIENTATION_RNG_VA};
use don_sim::rng::Random;
use don_sim::systems::map_terrain::{land, wflag, WCoord, World};
use don_sim::trig::{cosx, sinx};

pub const REGIONS_FIND_ALL_VA: u32 = 0x0068_0060;
pub const MAP_FILL_CONT_VA: u32 = 0x0068_a960;
pub const MAP_LAND_DIST_VA: u32 = 0x0069_d970;
pub const MAP_MAKE_REGION_VA: u32 = 0x0069_d3f0;

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
    MissingMapParameter {
        tag: &'static str,
        attribute: &'static str,
    },
    InvalidMapParameter {
        tag: &'static str,
        attribute: &'static str,
        value: String,
    },
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

    let partial = match inputs.map_style {
        6 | 9 => old_world_or_himalayas(inputs.map_style, players, world, &mut rng, &mut sites),
        12 => mediterranean(inputs, world, &mut rng, &mut sites, style),
        14 => great_lakes(players, inputs.map_size, world, &mut rng, &mut sites, style)?,
        18 => east_indies(players, inputs.map_size, world, &mut rng, &mut sites, style)?,
        19 => east_meets_west(inputs, world, &mut sites),
        _ => unreachable!("admitted above"),
    };

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
        starts_added: partial.starts_added,
        start_min: partial.start_min,
        stop: partial.stop,
    })
}

struct PartialReceipt {
    world_inverted: bool,
    starts_added: usize,
    start_min: Option<i32>,
    stop: ContinentStop,
}

fn old_world_or_himalayas(
    map_style: u8,
    players: u8,
    world: &mut World,
    rng: &mut Random,
    sites: &mut Vec<u32>,
) -> PartialReceipt {
    world.wipe();
    invert_land(world);

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
        starts_added: players as usize,
        start_min: Some(start_min),
        stop: ContinentStop::HookComplete {
            next_va: REGIONS_FIND_ALL_VA,
        },
    }
}

fn mediterranean(
    _inputs: &InitialWorldgenInputs,
    world: &mut World,
    rng: &mut Random,
    sites: &mut Vec<u32>,
    _style: &MapStyleStaticData,
) -> PartialReceipt {
    world.wipe();
    let style_sites = &crate::map_style::MEDITERRANEAN_DIRECT_RNG_SITES;
    let min_dim = (world.xs.min(world.ys) & !1) / 3;
    let a = draw(rng, sites, style_sites[0]);
    let b = draw(rng, sites, style_sites[1]);
    let _angle = a.wrapping_shl(16).wrapping_add(b);
    let x = world.xs / 2 + (draw(rng, sites, style_sites[2]) & 1);
    let y = world.ys / 2 + (draw(rng, sites, style_sites[3]) & 1);
    let area = min_dim.wrapping_mul(min_dim).wrapping_mul(3);
    PartialReceipt {
        world_inverted: false,
        starts_added: 0,
        start_min: None,
        stop: ContinentStop::MakeRegion {
            primitive_va: MAP_MAKE_REGION_VA,
            call: RegionSeedCall {
                region: 1,
                x,
                y,
                area,
            },
        },
    }
}

fn great_lakes(
    players: u8,
    map_size: u8,
    world: &mut World,
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
    world: &mut World,
    rng: &mut Random,
    sites: &mut Vec<u32>,
    style: &MapStyleStaticData,
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

    world.wipe();
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
    let increment = (u32::MAX / players as u32) as i32;
    angle = angle.wrapping_add(increment);
    let center_x = world.xs / 2;
    let center_y = world.ys / 2;
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
    Ok(PartialReceipt {
        world_inverted: false,
        starts_added: 1,
        start_min: None,
        stop: ContinentStop::MakeRegion {
            primitive_va: MAP_MAKE_REGION_VA,
            call: RegionSeedCall {
                region: 1,
                x,
                y,
                area: region_area,
            },
        },
    })
}

fn east_meets_west(
    inputs: &InitialWorldgenInputs,
    world: &mut World,
    _sites: &mut Vec<u32>,
) -> PartialReceipt {
    world.wipe();
    PartialReceipt {
        world_inverted: false,
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
