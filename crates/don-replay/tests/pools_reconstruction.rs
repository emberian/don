// SPDX-License-Identifier: GPL-3.0-or-later
//! Mutation-sensitive executable pins for `Map::eliminate_pools`.

use don_replay::pools::{
    execute_eliminate_pools, ElimPoolParam, EliminatePoolsError, MAP_ELIMINATE_POOLS_VA,
};
use don_sim::systems::map_terrain::{land, World};
use don_sim::systems::regions::Regions;

fn three_pool_world() -> World {
    let mut world = World::init_default_rules(9, 9);
    for cell in &mut world.wdata {
        cell.land = land::FERTILE;
        cell.land_sub = 7;
        cell.region = 91;
    }
    // A nine-cell edge ocean plus isolated one- and two-cell inland pools.
    for y in 0..9 {
        let cell = world.wdata_mut(0, y);
        cell.land = land::OCEAN;
        cell.land_sub = 3;
    }
    for (x, y) in [(3, 3), (6, 5), (6, 6)] {
        let cell = world.wdata_mut(x, y);
        cell.land = land::OCEAN;
        cell.land_sub = 3;
    }
    world
}

#[test]
fn entire_world_keeps_the_largest_selected_sea_and_fills_smaller_pools() {
    let mut world = three_pool_world();
    let mut regions = Regions::default();
    let receipt =
        execute_eliminate_pools(&mut world, &mut regions, ElimPoolParam::EntireWorld).unwrap();

    assert_eq!(receipt.primitive_va, MAP_ELIMINATE_POOLS_VA);
    assert_eq!(receipt.initial_pool_ids, vec![65, 66, 67]);
    assert_eq!(receipt.surviving_pool_ids, vec![65]);
    assert_eq!(
        receipt
            .merges
            .iter()
            .map(|merge| (
                merge.source_region,
                merge.target_region,
                merge.cells_changed
            ))
            .collect::<Vec<_>>(),
        vec![(66, 1, 1), (67, 1, 2)]
    );
    assert_eq!(regions.list[1].size, 72);
    assert_eq!(regions.list[1].coords.items.len(), 72);
    assert_eq!(regions.list[1].coords.capacity, 138);
    assert_eq!(regions.list[65].size, 9);
    assert_eq!(regions.get_num_sea(), 1);
    for (x, y) in [(3, 3), (6, 5), (6, 6)] {
        let cell = world.wdata(x, y);
        assert_eq!(
            (cell.land, cell.land_sub, cell.region),
            (land::FERTILE, 0, 1)
        );
    }
    assert_eq!(world.wdata(0, 4).land, land::OCEAN);
}

#[test]
fn edge_selector_mutation_leaves_inland_pools_untouched() {
    for param in [ElimPoolParam::CornersOnly, ElimPoolParam::EdgesOnly] {
        let mut world = three_pool_world();
        let mut regions = Regions::default();
        let receipt = execute_eliminate_pools(&mut world, &mut regions, param).unwrap();

        assert_eq!(receipt.initial_pool_ids, vec![65]);
        assert!(receipt.merges.is_empty());
        assert_eq!(regions.get_num_sea(), 3);
        assert_eq!(world.wdata(3, 3).land, land::OCEAN);
        assert_eq!(world.wdata(6, 5).land, land::OCEAN);
    }
}

#[test]
fn corrupt_array_metadata_fails_closed_before_region_rebuild() {
    let mut world = three_pool_world();
    let mut regions = Regions::default();
    regions.list[0].coords.capacity = -1;
    let before_world = world.wdata.clone();
    let before_regions = regions.clone();

    assert_eq!(
        execute_eliminate_pools(&mut world, &mut regions, ElimPoolParam::EntireWorld),
        Err(EliminatePoolsError::InvalidCoordinateStorage {
            region: 0,
            length: 0,
            capacity: -1,
            increment: -1,
        })
    );
    assert_eq!(world.wdata, before_world);
    assert_eq!(regions, before_regions);
}
