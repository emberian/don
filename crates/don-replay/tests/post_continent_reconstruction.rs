// SPDX-License-Identifier: GPL-3.0-or-later
//! Mutation-sensitive pins for the common post-style `Map::make` chain.

use don_replay::post_continent::{
    execute_post_continent, PostContinentError, TerritoryLimits, MAP_FIX_DIAG_LAND_VA,
    MAP_MAKE_COASTLINES_VA, REGIONS_CLEAR_ALL_VA, REGIONS_FIND_ALL_VA,
    TERRAIN_GROUPS_FILL_FERTILE_VA,
};
use don_replay::MAP_MAKE_SCHEDULE;
use don_sim::systems::map_terrain::{land, wflag, World};
use don_sim::systems::regions::{Regions, RegionsError};

fn terrain_fixture() -> (World, Regions) {
    let mut world = World::init_default_rules(11, 11);
    // A durable 3x3 continent: its perimeter becomes coast while its centre
    // remains dry, preventing the second coastline pass from erasing it.
    for y in 4..=6 {
        for x in 4..=6 {
            world.wdata_mut(x, y).land = land::FERTILE;
        }
    }
    // A disconnected diagonal pair with ocean on both orthogonal bridges.
    // fix_diag_land must repair at least one cell before coastline processing.
    world.wdata_mut(1, 1).land = land::FERTILE;
    world.wdata_mut(2, 2).land = land::FERTILE;
    (world, Regions::default())
}

fn distinctive_limits() -> TerritoryLimits {
    TerritoryLimits {
        player_base: 101,
        player_civic: 102,
        player_city: 103,
        colonized_base: 201,
        colonized_civic: 202,
        colonized_city: 203,
    }
}

#[test]
fn exact_common_chain_rebuilds_both_region_planes_and_reaches_fill_fertile() {
    let (mut world, mut regions) = terrain_fixture();
    let receipt = execute_post_continent(&mut world, &mut regions, distinctive_limits()).unwrap();

    assert_eq!(receipt.first_clear_va, REGIONS_CLEAR_ALL_VA);
    assert_eq!(receipt.first_find_va, REGIONS_FIND_ALL_VA);
    assert_eq!(receipt.fix_diag_va, MAP_FIX_DIAG_LAND_VA);
    assert_eq!(receipt.make_coastlines_va, MAP_MAKE_COASTLINES_VA);
    assert_eq!(receipt.second_clear_va, REGIONS_CLEAR_ALL_VA);
    assert_eq!(receipt.second_find_va, REGIONS_FIND_ALL_VA);
    assert_eq!(receipt.next_va, TERRAIN_GROUPS_FILL_FERTILE_VA);
    assert_eq!(receipt.limits, distinctive_limits());
    assert_eq!(receipt.fix_diag_cells_changed, 1);
    assert_eq!(receipt.coastline_cells_changed, 9);
    assert_eq!(receipt.final_land_cells, 9);
    assert_eq!(receipt.final_coast_cells, 8);
    assert_eq!(receipt.final_ocean_cells, 112);
    assert_eq!(receipt.final_half_water_cells, 0);
    assert_eq!(receipt.first_regions.land_components_found, 2);
    assert_eq!(receipt.second_regions.land_components_found, 1);

    assert_eq!(world.player_territory_limit, 101);
    assert_eq!(world.player_territory_limit_civic, 102);
    assert_eq!(world.player_territory_limit_city, 103);
    assert_eq!(world.colonized_territory_limit, 201);
    assert_eq!(world.colonized_territory_limit_civic, 202);
    assert_eq!(world.colonized_territory_limit_city, 203);
    let coords = regions
        .list
        .iter()
        .map(|region| region.coords.items.len())
        .sum::<usize>();
    assert_eq!(coords, world.wdata.len());
    assert!(world
        .wdata
        .iter()
        .any(|cell| cell.flags & wflag::COAST != 0));
}

#[test]
fn territory_limit_mutation_changes_only_the_six_copied_world_fields() {
    let (mut first_world, mut first_regions) = terrain_fixture();
    let (mut second_world, mut second_regions) = terrain_fixture();
    let first = distinctive_limits();
    let mut second = first;
    second.colonized_city += 1;

    execute_post_continent(&mut first_world, &mut first_regions, first).unwrap();
    execute_post_continent(&mut second_world, &mut second_regions, second).unwrap();

    assert_eq!(first_world.wdata, second_world.wdata);
    assert_eq!(first_regions, second_regions);
    assert_eq!(first_world.colonized_territory_limit_city, 203);
    assert_eq!(second_world.colonized_territory_limit_city, 204);
}

#[test]
fn malformed_world_shape_fails_closed_before_first_region_clear() {
    let (mut world, mut regions) = terrain_fixture();
    world.size += 1;
    let before_world = world.clone();
    let before_regions = regions.clone();

    assert_eq!(
        execute_post_continent(&mut world, &mut regions, distinctive_limits()),
        Err(PostContinentError::FirstRegionBuild(
            RegionsError::InvalidWorldShape {
                xs: 11,
                ys: 11,
                size: 122,
                wdata_len: 121,
            }
        ))
    );
    assert_eq!(world.wdata, before_world.wdata);
    assert_eq!(world.size, before_world.size);
    assert_eq!(regions, before_regions);
}

#[test]
fn stage_inventory_names_the_exact_no_rng_chain_and_external_next_input() {
    let stage = |name| {
        MAP_MAKE_SCHEDULE
            .iter()
            .find(|stage| stage.name == name)
            .expect("stage must remain in Map::make order")
    };

    assert_eq!(
        stage("make_regions").evidence_va,
        Some(REGIONS_CLEAR_ALL_VA)
    );
    assert_eq!(
        stage("fix_diag_land").evidence_va,
        Some(MAP_FIX_DIAG_LAND_VA)
    );
    assert_eq!(
        stage("make_coastlines").evidence_va,
        Some(MAP_MAKE_COASTLINES_VA)
    );
    assert_eq!(
        stage("fill_fertile").evidence_va,
        Some(TERRAIN_GROUPS_FILL_FERTILE_VA)
    );
    assert_eq!(stage("fix_diag_land").rng, "none");
    assert_eq!(stage("make_coastlines").rng, "none");
    assert!(stage("fill_fertile")
        .rng
        .contains("unreconstructed Fractal::frac"));
}
