//! Source-bounded frame-zero projection of retail's synchronous territory pass.
//!
//! `Setup::build_game` finishes by calling `World::compute_all_territory` before the first
//! `Leader::plan_strategy`.  The pass is therefore the last writer of `WData::who/who2`
//! and `CityData::bordering +0x65` before the starting-City terrain census.  This module
//! binds the already canonical one-Village setup to the exact per-tile scorer in
//! `don-sim`; it does not use a recorded checksum as input.
//!
//! The current procedural-map owner stops inside `TerrainGroups::place_all`, so the
//! projection deliberately remains diagnostic.  Its receipt reports the input World's
//! unsourced byte count and the still-unpublished Leader/Region side effects.  A future
//! final-map receipt can consume the same transaction, but this module cannot turn an
//! exact function over a provisional input into a replay producer.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::systems::borders_fog::{
    city_border_is_contested, claim_tile, fine_to_tile, leader_border_params, BorderSource,
    BorderSourceKind, Diplomacy, LeaderBorderInput, LeaderBorderParams, TerritoryRules,
};
use don_sim::systems::map_terrain::World;
use don_sim::systems::regions::{Regions, LAND_REGION_COUNT, REGION_COUNT, SEA_REGION_FIRST};
use don_sim::systems::tech_cities::{self, CityPool};

use crate::initial::InitialWorld;
use crate::replay::Replay;
use crate::setup_cities_builds::{StartingSetupState, CITY_CENTER_TYPE, UNTEAMED};

pub const WORLD_COMPUTE_REG_TERRITORY_VA: u32 = 0x006b_0bb0;
pub const WORLD_COMPUTE_ALL_TERRITORY_VA: u32 = 0x006b_5700;
pub const CITY_BORDERING_CLEAR_VA: u32 = 0x006b_0c40;
pub const CITY_BORDERING_SET_VA: u32 = 0x006b_17c8;
pub const LEADER_TERRITORY_READY_FLAG: u32 = 0x0200_0000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartingCityTerritoryError {
    ReplayIdentityMismatch,
    WorldIdentityMismatch,
    UnsupportedStartingTechnology {
        primary: u8,
        secondary: u8,
    },
    ActivePlayerCountMismatch {
        replay: usize,
        setup: usize,
    },
    UnsupportedTeam {
        player_slot: u8,
        team: u8,
    },
    MissingSetupOwner {
        owner: u8,
    },
    DuplicateSetupOwner {
        owner: u8,
    },
    CitySlotNotZero {
        owner: u8,
        slot: i16,
    },
    CityPoolShape {
        owner: usize,
    },
    CityImageMismatch {
        owner: usize,
    },
    CenterBuildJoin {
        owner: usize,
        row: usize,
    },
    UnsupportedCenterBuild {
        row: usize,
        type_index: i32,
    },
    RegionSizeNegative {
        region: usize,
        size: i32,
    },
    RegionCoordinateCount {
        region: usize,
        size: usize,
        coords: usize,
    },
    RegionCoordinateOutsideWorld {
        region: usize,
        wx: i32,
        wy: i32,
    },
    RegionCoordinateOwner {
        region: usize,
        wx: i32,
        wy: i32,
        actual: i16,
    },
    ClaimCityIndex {
        owner: usize,
        index: i32,
    },
}

impl fmt::Display for StartingCityTerritoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "starting City territory projection refused: {self:?}")
    }
}

impl std::error::Error for StartingCityTerritoryError {}

/// Targeted after-images and exact side-effect accounting for the synchronous pass.
///
/// `first_checksum_city_image_ready` remains false until `input_world_complete` and the
/// returned Leader side effects have canonical consumers.  The World and City images are
/// still useful for checksum-independent first-divergence diagnostics.
#[derive(Clone, Debug)]
pub struct StartingCityTerritoryProjection {
    pub world: World,
    pub regions: Regions,
    pub cities: CityPool,
    /// Standalone final-byte authority when exhaustive geometry proves that unfinished
    /// Region/WData content cannot enter the native bordering set arm.
    pub bordering_authority: Option<StartingCityBorderingAuthority>,
    pub receipt: StartingCityTerritoryReceipt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartingCityBorderingAuthority {
    pub replay_payload_sha256: [u8; 32],
    pub world_shape: (i32, i32),
    pub player_limits: (i32, i32, i32),
    pub colonized_limits: (i32, i32, i32),
    pub active_owners: Vec<u8>,
    pub values: [u8; tech_cities::NUM_PLAYERS],
    pub cells_exhaustively_scanned: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartingCityTerritoryReceipt {
    pub replay_payload_sha256: [u8; 32],
    pub input_world_checksum: u32,
    pub output_world_checksum: u32,
    pub input_world_unsourced_walked_bytes: u64,
    pub input_world_complete: bool,
    pub active_owners: Vec<u8>,
    pub land_regions_walked: u32,
    pub sea_regions_walked: u32,
    pub land_cells_claimed: u32,
    pub sea_cells_cleared: u32,
    pub who_writes: u32,
    pub who2_writes: u32,
    pub city_bordering_clears: u32,
    pub city_bordering_sets: u32,
    pub city_capital_flag_sets: u32,
    /// Every WCoord in the rectangular map was tested under both retail Region limit
    /// triples. This is a geometry proof, not an observation of the provisional WData.
    pub bordering_cells_exhaustively_scanned: u32,
    pub potential_contested_city_cells: u32,
    /// True when no possible Region membership can reach the bordering set arm and all
    /// fresh Cities entered the pass at zero. This one City byte is then final-map
    /// independent even though `who/who2` are not.
    pub bordering_zero_independent_of_final_map: bool,
    pub bordering_before: [u8; tech_cities::NUM_PLAYERS],
    pub bordering_after: [u8; tech_cities::NUM_PLAYERS],
    pub territory_by_region: [[u16; LAND_REGION_COUNT]; tech_cities::NUM_PLAYERS],
    pub territory_total: [u32; tech_cities::NUM_PLAYERS],
    /// Retail ORs this into every active Leader after each completed land-region pass.
    pub unpublished_leader_flag_or: u32,
    /// Region `flags/borders` are returned in `regions`, but `StartingSetupState` does not
    /// own the canonical generation Regions store and therefore cannot publish them.
    pub unpublished_region_state: bool,
    pub city_world_projection_complete: bool,
    pub first_checksum_city_image_ready: bool,
}

/// Run the exact City/World portion of the final synchronous territory pass over the map
/// image currently owned by replay reconstruction.
///
/// This always returns a *projection*, never a channel authority.  In particular,
/// `InitialWorld::unsourced_walked_bytes() != 0` is retained in the receipt rather than
/// being waived.  The transaction stages all three target stores and publishes no partial
/// after-image on error.
pub fn project_starting_city_territory(
    replay: &Replay,
    setup: &StartingSetupState,
    map: &InitialWorld,
) -> Result<StartingCityTerritoryProjection, StartingCityTerritoryError> {
    if replay.initial.payload_sha256 != setup.receipt.replay_payload_sha256 {
        return Err(StartingCityTerritoryError::ReplayIdentityMismatch);
    }
    // Setup has already applied the exact starting-center TData masks to its World.  The
    // procedural owner remains the authority for WData/Regions, so join those planes plus
    // the immutable shape/seed rather than requiring the pre-Build and post-Build World
    // checksums to be equal.
    if setup.sim.map.world.wdata != map.world.wdata
        || setup.sim.map.world.xs != map.world.xs
        || setup.sim.map.world.ys != map.world.ys
        || setup.sim.map.world.seed != map.world.seed
    {
        return Err(StartingCityTerritoryError::WorldIdentityMismatch);
    }
    let settings = replay.initial.info.settings;
    if settings.starting_technology != 0
        || (settings.game_rules == 8 && settings.starting_technology2 != 0)
    {
        return Err(StartingCityTerritoryError::UnsupportedStartingTechnology {
            primary: settings.starting_technology,
            secondary: settings.starting_technology2,
        });
    }

    let active_players: Vec<_> = replay.initial.active_players().collect();
    if active_players.len() != setup.receipt.active_players {
        return Err(StartingCityTerritoryError::ActivePlayerCountMismatch {
            replay: active_players.len(),
            setup: setup.receipt.active_players,
        });
    }

    let mut inputs = [LeaderBorderInput::default(); tech_cities::NUM_PLAYERS];
    let mut diplomacy = Diplomacy::default();
    let mut active_owners = Vec::with_capacity(active_players.len());
    for who in 0..tech_cities::NUM_PLAYERS {
        diplomacy.team[who] = who as i32;
        // Leader::init writes the self declaration to Ally. Non-self ordinary FFA rows
        // remain zero; the bordering writer tests zero, not the later is_enemy predicate.
        diplomacy.diplo[who][who] = 2;
    }
    for player in active_players {
        if player.team != UNTEAMED {
            return Err(StartingCityTerritoryError::UnsupportedTeam {
                player_slot: player.slot,
                team: player.team,
            });
        }
        let owner = usize::from(player.who);
        if owner >= tech_cities::NUM_PLAYERS {
            return Err(StartingCityTerritoryError::MissingSetupOwner { owner: player.who });
        }
        if inputs[owner].active {
            return Err(StartingCityTerritoryError::DuplicateSetupOwner { owner: player.who });
        }
        inputs[owner] = LeaderBorderInput {
            active: true,
            // In ordinary (non-Tech-Race) games `LeaderData::starting_age` reads only the
            // primary byte. With starting technology zero, Leader::init has no territory
            // prerequisites, Wonders, rares, CTW bonuses, or nonzero government/age term
            // at this call; `starting_technology2` is inert unless `game_rules == 8`.
            gov_level: 0,
            age: 0,
            tribe_roman: player.tribe == 6,
            tribe_russian: player.tribe == 13,
            ..LeaderBorderInput::default()
        };
        active_owners.push(player.who);
    }
    active_owners.sort_unstable();

    let mut city_receipts = [None; tech_cities::NUM_PLAYERS];
    for (receipt_index, receipt) in setup.receipt.cities.iter().enumerate() {
        if receipt.city_slot != 0 {
            return Err(StartingCityTerritoryError::CitySlotNotZero {
                owner: receipt.owner,
                slot: receipt.city_slot,
            });
        }
        let owner = usize::from(receipt.owner);
        if !inputs.get(owner).is_some_and(|input| input.active) {
            return Err(StartingCityTerritoryError::MissingSetupOwner {
                owner: receipt.owner,
            });
        }
        if city_receipts[owner].replace(receipt_index).is_some() {
            return Err(StartingCityTerritoryError::DuplicateSetupOwner {
                owner: receipt.owner,
            });
        }
    }
    for &owner in &active_owners {
        if city_receipts[usize::from(owner)].is_none() {
            return Err(StartingCityTerritoryError::MissingSetupOwner { owner });
        }
    }

    let mut slots: [Vec<BorderSource>; tech_cities::NUM_PLAYERS] = Default::default();
    let mut bordering_before = [0u8; tech_cities::NUM_PLAYERS];
    for owner in 0..tech_cities::NUM_PLAYERS {
        let pool = setup
            .sim
            .cities
            .slots
            .get(owner)
            .ok_or(StartingCityTerritoryError::CityPoolShape { owner })?;
        let live: Vec<_> = pool
            .iter()
            .enumerate()
            .filter(|(_, city)| city.active())
            .collect();
        if inputs[owner].active {
            if live.len() != 1 || live[0].0 != 0 {
                return Err(StartingCityTerritoryError::CityPoolShape { owner });
            }
            let city = live[0].1;
            let receipt = &setup.receipt.cities[city_receipts[owner].unwrap()];
            if city.city != 0
                || city.who != owner as i8
                || city.race != owner as i8
                || city.founder != owner as i8
                || city.city_flags & 0x4001 != 0x4001
                || city.o != receipt.build.object_id
                || city.reg != receipt.region
                || (city.x, city.y) != receipt.snapped_position
                || setup.cities.slots[owner][0].pod_bytes() != city.pod_bytes()
            {
                return Err(StartingCityTerritoryError::CityImageMismatch { owner });
            }
            let Some(center) = setup.sim.builds.get(receipt.build.row) else {
                return Err(StartingCityTerritoryError::CenterBuildJoin {
                    owner,
                    row: receipt.build.row,
                });
            };
            if center.orig_type != CITY_CENTER_TYPE
                || center.who != owner as u8
                || center.object_id() != city.o
                || center.city != 0
                || center.position() != (city.x, city.y)
            {
                return Err(StartingCityTerritoryError::CenterBuildJoin {
                    owner,
                    row: receipt.build.row,
                });
            }
            bordering_before[owner] = city.bordering;
            slots[owner].push(BorderSource {
                kind: BorderSourceKind::City {
                    level: 1,
                    has_temple: city.city_flags & 0x80 != 0,
                    capital: city.city_flags & 0x4000 != 0,
                },
                slot: owner as i32,
                who: i32::from(city.who),
                tile_x: fine_to_tile(city.x),
                tile_y: fine_to_tile(city.y),
                alive: true,
            });
        } else if !live.is_empty() {
            return Err(StartingCityTerritoryError::CityPoolShape { owner });
        }
    }
    if setup.sim.builds.len() != active_owners.len() {
        return Err(StartingCityTerritoryError::UnsupportedCenterBuild {
            row: setup.sim.builds.len(),
            type_index: -1,
        });
    }
    for (row, build) in setup.sim.builds.iter().enumerate() {
        if build.orig_type != CITY_CENTER_TYPE {
            return Err(StartingCityTerritoryError::UnsupportedCenterBuild {
                row,
                type_index: build.orig_type,
            });
        }
    }

    validate_regions(&map.generation_regions, &map.world)?;

    let mut world = setup.sim.map.world.clone();
    let mut regions = map.generation_regions.clone();
    let mut cities = setup.sim.cities.clone();
    let input_world_checksum = world.checksum_sections().full;
    let rules = TerritoryRules::default();
    let active = std::array::from_fn(|who| inputs[who].active);
    let mut territory_by_region = [[0u16; LAND_REGION_COUNT]; tech_cities::NUM_PLAYERS];
    let mut land_regions_walked = 0u32;
    let mut sea_regions_walked = 0u32;
    let mut land_cells_claimed = 0u32;
    let mut sea_cells_cleared = 0u32;
    let mut who_writes = 0u32;
    let mut who2_writes = 0u32;
    let mut city_bordering_clears = 0u32;
    let mut city_bordering_sets = 0u32;
    let mut city_capital_flag_sets = 0u32;

    // Region flags select one of two World-resident limit triples. Exhaust both for every
    // coordinate in the map rectangle. Region membership can only remove cells from this
    // superset, so zero contested candidates proves City::bordering remains zero without
    // assuming any unfinished WData/Regions byte.
    let mut potential_contested_city_cells = 0u32;
    for wy in 0..world.ys {
        for wx in 0..world.xs {
            let mut contested = false;
            for limits in [
                (
                    world.player_territory_limit,
                    world.player_territory_limit_civic,
                    world.player_territory_limit_city,
                ),
                (
                    world.colonized_territory_limit,
                    world.colonized_territory_limit_civic,
                    world.colonized_territory_limit_city,
                ),
            ] {
                let mut params = [LeaderBorderParams::default(); tech_cities::NUM_PLAYERS];
                for owner in 0..tech_cities::NUM_PLAYERS {
                    params[owner] =
                        leader_border_params(&inputs[owner], &rules, limits.0, limits.1, limits.2);
                }
                let claim = claim_tile(wx, wy, &slots, &params, &active, &rules);
                contested |= claim.best_city_index >= 0
                    && claim.who2 >= 0
                    && city_border_is_contested(
                        &diplomacy,
                        i32::from(claim.who),
                        i32::from(claim.who2),
                    );
            }
            potential_contested_city_cells =
                potential_contested_city_cells.wrapping_add(u32::from(contested));
        }
    }
    let bordering_zero_independent_of_final_map = potential_contested_city_cells == 0
        && active_owners
            .iter()
            .all(|&owner| bordering_before[usize::from(owner)] == 0);

    for region_index in 0..LAND_REGION_COUNT {
        let region = &mut regions.list[region_index];
        if region.size == 0 {
            continue;
        }
        land_regions_walked = land_regions_walked.wrapping_add(1);
        region.borders = 0;
        // compute_reg_territory clears every live City whenever this Region cursor starts
        // at zero. compute_all_territory restarts every nonempty land Region.
        for owner in 0..tech_cities::NUM_PLAYERS {
            if !active[owner] {
                continue;
            }
            for city in &mut cities.slots[owner] {
                if city.active() {
                    city.bordering = 0;
                    city_bordering_clears = city_bordering_clears.wrapping_add(1);
                }
            }
        }
        region.flags &= !0x80;
        let limits = if region.flags & 4 != 0 {
            (
                world.player_territory_limit,
                world.player_territory_limit_civic,
                world.player_territory_limit_city,
            )
        } else {
            (
                world.colonized_territory_limit,
                world.colonized_territory_limit_civic,
                world.colonized_territory_limit_city,
            )
        };
        let mut params = [LeaderBorderParams::default(); tech_cities::NUM_PLAYERS];
        for owner in 0..tech_cities::NUM_PLAYERS {
            params[owner] =
                leader_border_params(&inputs[owner], &rules, limits.0, limits.1, limits.2);
        }

        for &(wx, wy) in region.coords.items.iter().take(region.size as usize) {
            let claim = claim_tile(wx, wy, &slots, &params, &active, &rules);
            let cell = world.wdata_mut(wx, wy);
            cell.who = claim.who;
            cell.who2 = claim.who2;
            who_writes = who_writes.wrapping_add(1);
            who2_writes = who2_writes.wrapping_add(1);
            land_cells_claimed = land_cells_claimed.wrapping_add(1);
            region.borders = region.borders.wrapping_add(1);

            if claim.who >= 0 {
                let owner = claim.who as usize;
                territory_by_region[owner][region_index] =
                    territory_by_region[owner][region_index].wrapping_add(1);
                if claim.best_city_index >= 0 {
                    if claim.best_city_index != 0 || slots[owner].len() != 1 {
                        return Err(StartingCityTerritoryError::ClaimCityIndex {
                            owner,
                            index: claim.best_city_index,
                        });
                    }
                    let city = &mut cities.slots[owner][0];
                    if claim.who2 >= 0
                        && city_border_is_contested(
                            &diplomacy,
                            i32::from(claim.who),
                            i32::from(claim.who2),
                        )
                    {
                        let before = city.bordering;
                        city.bordering |= 1u8 << (claim.who as u32 & 0x1f);
                        city.bordering |= 1u8 << (claim.who2 as u32 & 0x1f);
                        city_bordering_sets =
                            city_bordering_sets.wrapping_add(u32::from(city.bordering != before));
                        let city_wx = city.x.div_euclid(768);
                        let city_wy = city.y.div_euclid(768);
                        if (wx - city_wx).abs() + (wy - city_wy).abs() < 5 {
                            let before = city.city_flags;
                            city.city_flags |= 0x1000;
                            city_capital_flag_sets = city_capital_flag_sets
                                .wrapping_add(u32::from(city.city_flags != before));
                        }
                    }
                }
            }
        }
        region.flags |= 0x80 | 0x20;
    }

    for region_index in SEA_REGION_FIRST..REGION_COUNT {
        let region = &regions.list[region_index];
        if region.size == 0 {
            continue;
        }
        sea_regions_walked = sea_regions_walked.wrapping_add(1);
        for &(wx, wy) in region.coords.items.iter().take(region.size as usize) {
            let cell = world.wdata_mut(wx, wy);
            cell.who = -1;
            cell.who2 = -1;
            who_writes = who_writes.wrapping_add(1);
            who2_writes = who2_writes.wrapping_add(1);
            sea_cells_cleared = sea_cells_cleared.wrapping_add(1);
        }
    }

    let territory_total = std::array::from_fn(|owner| {
        territory_by_region[owner]
            .iter()
            .fold(0u32, |sum, &value| sum.wrapping_add(u32::from(value)))
    });
    let bordering_after = std::array::from_fn(|owner| {
        cities.slots[owner]
            .iter()
            .find(|city| city.active())
            .map_or(0, |city| city.bordering)
    });
    let input_world_unsourced_walked_bytes = map.unsourced_walked_bytes();
    let input_world_complete = input_world_unsourced_walked_bytes == 0;
    let output_world_checksum = world.checksum_sections().full;
    let bordering_cells_exhaustively_scanned = world.size as u32;
    let bordering_authority =
        bordering_zero_independent_of_final_map.then(|| StartingCityBorderingAuthority {
            replay_payload_sha256: replay.initial.payload_sha256,
            world_shape: (world.xs, world.ys),
            player_limits: (
                world.player_territory_limit,
                world.player_territory_limit_civic,
                world.player_territory_limit_city,
            ),
            colonized_limits: (
                world.colonized_territory_limit,
                world.colonized_territory_limit_civic,
                world.colonized_territory_limit_city,
            ),
            active_owners: active_owners.clone(),
            values: [0; tech_cities::NUM_PLAYERS],
            cells_exhaustively_scanned: bordering_cells_exhaustively_scanned,
        });
    Ok(StartingCityTerritoryProjection {
        world,
        regions,
        cities,
        bordering_authority,
        receipt: StartingCityTerritoryReceipt {
            replay_payload_sha256: replay.initial.payload_sha256,
            input_world_checksum,
            output_world_checksum,
            input_world_unsourced_walked_bytes,
            input_world_complete,
            active_owners,
            land_regions_walked,
            sea_regions_walked,
            land_cells_claimed,
            sea_cells_cleared,
            who_writes,
            who2_writes,
            city_bordering_clears,
            city_bordering_sets,
            city_capital_flag_sets,
            bordering_cells_exhaustively_scanned,
            potential_contested_city_cells,
            bordering_zero_independent_of_final_map,
            bordering_before,
            bordering_after,
            territory_by_region,
            territory_total,
            unpublished_leader_flag_or: LEADER_TERRITORY_READY_FLAG,
            unpublished_region_state: true,
            city_world_projection_complete: true,
            first_checksum_city_image_ready: false,
        },
    })
}

fn validate_regions(regions: &Regions, world: &World) -> Result<(), StartingCityTerritoryError> {
    for (region_index, region) in regions.list.iter().enumerate() {
        let size = usize::try_from(region.size).map_err(|_| {
            StartingCityTerritoryError::RegionSizeNegative {
                region: region_index,
                size: region.size,
            }
        })?;
        if size != region.coords.items.len() {
            return Err(StartingCityTerritoryError::RegionCoordinateCount {
                region: region_index,
                size,
                coords: region.coords.items.len(),
            });
        }
        for &(wx, wy) in &region.coords.items {
            if !world.valid_w(wx, wy) {
                return Err(StartingCityTerritoryError::RegionCoordinateOutsideWorld {
                    region: region_index,
                    wx,
                    wy,
                });
            }
            let cell = world.wdata(wx, wy);
            let owns = cell.region == region_index as i16
                || (cell.region2 == region_index as i16 && region_index >= SEA_REGION_FIRST);
            if !owns {
                return Err(StartingCityTerritoryError::RegionCoordinateOwner {
                    region: region_index,
                    wx,
                    wy,
                    actual: cell.region,
                });
            }
        }
    }
    Ok(())
}
