// SPDX-License-Identifier: GPL-3.0-or-later
//! Mutation pins for the complete `Map::grow_region` helper graph.

use don_replay::growth::{
    execute_grow_region, GrowRegionCall, GrowRegionError, MapGrowthConfig, MAP_GROW_VALID_VA,
    MAP_LARGE_STAMP_VA, MAP_POINT_VA, MAP_STAMP_VA,
};
use don_sim::rng::Random;
use don_sim::systems::map_terrain::{land, World};
use don_sim::systems::regions::Regions;

fn seeded_region(edge: i32, seed: i32) -> (World, Regions) {
    let mut world = World::init_default_rules(edge, edge);
    world.seed_map_generation(seed);
    world.wipe();
    let mut regions = Regions::default();
    regions.clear_all(&mut world);
    let center = edge / 2;
    let cell = world.wdata_mut(center, center);
    cell.land = land::FERTILE;
    cell.land_sub = 0;
    cell.region = 1;
    regions.list[1].coords.capacity = 4;
    regions.list[1].coords.items.push((center, center));
    regions.list[1].size = 1;
    regions.land = 1;
    (world, regions)
}

fn config() -> MapGrowthConfig {
    MapGrowthConfig {
        avoid_center: 0,
        avoid_continent: 0,
        edge_avoid: [0; 4],
        base_edge: 0,
        edge_jitter: 0,
    }
}

#[test]
fn exact_growth_executes_large_stamp_and_finishing_paths_non_vacuously() {
    let seed = 0x1234_5678;
    let (mut world, mut regions) = seeded_region(40, seed);
    let mut rng = Random::new(seed);
    let mut growth_config = config();
    let call = GrowRegionCall {
        region: 1,
        target_area: 256,
        max_distance: 30,
        anchor_x: -1,
        anchor_y: -1,
        return_partial_size: 0,
    };
    let receipt = execute_grow_region(
        &mut world,
        &mut regions,
        &mut rng,
        &mut growth_config,
        &call,
    )
    .unwrap();
    assert_eq!(
        (
            receipt.radius,
            receipt.final_size,
            receipt.rng_final,
            receipt.rng_sites.len(),
            receipt.point_attempts,
            receipt.points_added,
            receipt.stamp_calls,
            receipt.large_stamp_calls,
            receipt.stalled_iterations,
            receipt.edge_jitter_final,
        ),
        (5, 256, 1_490_971_998, 166, 729, 255, 64, 20, 29, 0)
    );

    assert!(receipt.completed);
    assert_eq!(receipt.retail_return, 0);
    assert!(receipt.final_size >= 256);
    assert!(receipt.points_added >= 255);
    assert!(receipt.large_stamp_calls > 0);
    assert!(receipt.stamp_calls >= receipt.large_stamp_calls);
    assert_eq!(
        regions.list[1].size as usize,
        regions.list[1].coords.items.len()
    );
    assert_eq!(
        world.wdata.iter().filter(|cell| cell.region == 1).count(),
        regions.list[1].size as usize
    );
    assert_eq!(receipt.rng_final, rng.state());
    assert!(receipt.rng_sites.iter().all(|site| *site != MAP_POINT_VA));
    assert_ne!(MAP_LARGE_STAMP_VA, MAP_STAMP_VA);
    assert_ne!(MAP_GROW_VALID_VA, MAP_POINT_VA);
}

#[test]
fn edge_jitter_is_rng_ordered_and_parameter_mutation_changes_the_projection() {
    let seed = 77;
    let call = GrowRegionCall {
        region: 1,
        target_area: 72,
        max_distance: 30,
        anchor_x: -1,
        anchor_y: -1,
        return_partial_size: 0,
    };
    let (mut baseline_world, mut baseline_regions) = seeded_region(40, seed);
    let mut baseline_rng = Random::new(seed);
    let mut baseline_config = config();
    let baseline = execute_grow_region(
        &mut baseline_world,
        &mut baseline_regions,
        &mut baseline_rng,
        &mut baseline_config,
        &call,
    )
    .unwrap();
    assert_eq!(
        (
            baseline.final_size,
            baseline.rng_final,
            baseline.rng_sites.len(),
            baseline.point_attempts,
            baseline.points_added,
            baseline.stamp_calls,
            baseline.large_stamp_calls,
            baseline.stalled_iterations,
            baseline.edge_jitter_final,
        ),
        (72, 1_223_589_023, 26, 174, 71, 0, 0, 9, 0)
    );

    let (mut edged_world, mut edged_regions) = seeded_region(40, seed);
    let mut edged_rng = Random::new(seed);
    let mut edged_config = config();
    edged_config.edge_avoid = [7, 7, 7, 7];
    let edged = execute_grow_region(
        &mut edged_world,
        &mut edged_regions,
        &mut edged_rng,
        &mut edged_config,
        &call,
    )
    .unwrap();
    assert_eq!(
        (
            edged.final_size,
            edged.rng_final,
            edged.rng_sites.len(),
            edged.point_attempts,
            edged.points_added,
            edged.stamp_calls,
            edged.large_stamp_calls,
            edged.stalled_iterations,
            edged.edge_jitter_final,
        ),
        (73, -667_842_549, 486, 154, 72, 0, 0, 6, 4)
    );

    assert!(edged.rng_sites.len() > baseline.rng_sites.len());
    assert_ne!(edged.rng_final, baseline.rng_final);
    assert_ne!(
        edged_regions.list[1].coords.items,
        baseline_regions.list[1].coords.items
    );
    assert!((0..=4).contains(&edged.edge_jitter_final));
}

#[test]
fn corrupt_coordinate_growth_metadata_fails_closed_after_a_real_attempt() {
    let seed = 91;
    let (mut world, mut regions) = seeded_region(40, seed);
    regions.list[1].coords.capacity = 1;
    regions.list[1].coords.increment = 0;
    let before_world = world.clone();
    let before_regions = regions.clone();
    let mut rng = Random::new(seed);
    let before_rng = rng;
    let mut growth_config = config();
    let before_config = growth_config.clone();
    let result = execute_grow_region(
        &mut world,
        &mut regions,
        &mut rng,
        &mut growth_config,
        &GrowRegionCall {
            region: 1,
            target_area: 80,
            max_distance: 30,
            anchor_x: -1,
            anchor_y: -1,
            return_partial_size: 0,
        },
    );
    assert!(matches!(
        result,
        Err(GrowRegionError::InvalidCoordinateStorage { region: 1, .. })
    ));
    assert_eq!(world.wdata, before_world.wdata);
    assert_eq!(world.start_x, before_world.start_x);
    assert_eq!(world.start_y, before_world.start_y);
    assert_eq!(regions, before_regions);
    assert_eq!(rng, before_rng);
    assert_eq!(growth_config, before_config);
}
