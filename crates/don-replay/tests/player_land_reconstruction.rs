// SPDX-License-Identifier: GPL-3.0-or-later
//! Mutation-sensitive executable pins for `Map::check_player_land`.

use don_replay::player_land::{
    execute_check_player_land, CheckPlayerLandCall, CheckPlayerLandError, MAP_CHECK_PLAYER_LAND_VA,
};
use don_sim::systems::map_terrain::{land, WCoord, World};
use don_sim::systems::regions::Regions;
use std::collections::BTreeSet;

fn call(enabled: i32, avoid_continent: i32, radius: i32) -> CheckPlayerLandCall {
    CheckPlayerLandCall {
        enabled,
        avoid_continent,
        radius,
        unused: 0x1357_2468,
    }
}

#[test]
fn cleared_mediterranean_state_grows_region_zero_from_all_four_city_cells() {
    let mut world = World::init_default_rules(13, 13);
    for cell in &mut world.wdata {
        cell.region = 0;
    }
    world.add_starting_location(WCoord(6), WCoord(6));
    let mut regions = Regions::default();

    let receipt = execute_check_player_land(&mut world, &mut regions, call(1, 4, 1)).unwrap();

    assert_eq!(receipt.primitive_va, MAP_CHECK_PLAYER_LAND_VA);
    assert_eq!(receipt.effective_radius, 1);
    assert_eq!(receipt.effective_avoid_continent, Some(4));
    assert_eq!(receipt.footprints_visited, 4);
    assert_eq!(receipt.target_searches, 4);
    assert_eq!(receipt.target_regions_found, 0);
    assert_eq!(receipt.seed_cells_written, 0);
    assert_eq!(receipt.outer_candidates, 32);
    assert_eq!(receipt.outer_cells_written, 32);
    assert_eq!(receipt.conflict_rejections, 0);
    assert_eq!(receipt.region_zero_appends, 32);
    assert_eq!(regions.list[0].size, 32);
    assert_eq!(regions.list[0].coords.items.len(), 32);
    assert_eq!(regions.list[0].coords.capacity, 32);

    let unique: BTreeSet<_> = regions.list[0].coords.items.iter().copied().collect();
    assert_eq!(unique.len(), 16, "retail retains repeated footprint growth");
    for (x, y) in unique {
        assert_eq!(
            (world.wdata(x, y).land, world.wdata(x, y).region),
            (land::FERTILE, 0)
        );
    }
}

#[test]
fn first_nearby_land_region_owns_the_exact_three_by_three_seed_patch() {
    let mut world = World::init_default_rules(9, 9);
    for cell in &mut world.wdata {
        cell.region = 0;
    }
    world.start_city_x.items.push(4);
    world.start_city_x.capacity = 4;
    world.start_city_y.items.push(4);
    world.start_city_y.capacity = 4;
    let mut regions = Regions::default();

    // move_x/y index one is north-west. A later candidate must not replace it.
    world.wdata_mut(3, 3).region = 2;
    world.wdata_mut(4, 3).region = 3;
    regions.list[2].coords.items.push((3, 3));
    regions.list[2].coords.capacity = 4;
    regions.list[2].size = 1;
    regions.list[3].coords.items.push((4, 3));
    regions.list[3].coords.capacity = 4;
    regions.list[3].size = 1;

    let receipt = execute_check_player_land(&mut world, &mut regions, call(0, -99, 0)).unwrap();

    assert_eq!(receipt.effective_avoid_continent, None);
    assert_eq!(receipt.target_regions_found, 1);
    assert_eq!(receipt.seed_cells_written, 9);
    assert_eq!(receipt.seed_coords_removed, 1);
    assert_eq!(receipt.outer_candidates, 0);
    // Retail changes local_region to the target after the first write, then
    // removes/decrements that same target on each remaining cell even when the
    // coordinate was not yet present. The size/list mismatch is observable.
    assert_eq!(regions.list[2].size, 2);
    assert_eq!(regions.list[2].coords.items.len(), 9);
    assert_eq!(regions.list[3].size, 1);
    assert_eq!(regions.list[3].coords.items, [(4, 3)]);
    for y in 3..=5 {
        for x in 3..=5 {
            let cell = world.wdata(x, y);
            assert_eq!(
                (cell.land, cell.land_sub, cell.region),
                (land::FERTILE, 0, 2)
            );
        }
    }
}

#[test]
fn populated_foreign_region_rejects_growth_only_when_exclusion_is_enabled() {
    fn fixture() -> (World, Regions) {
        let mut world = World::init_default_rules(11, 11);
        world.start_city_x.items.push(5);
        world.start_city_x.capacity = 4;
        world.start_city_y.items.push(5);
        world.start_city_y.capacity = 4;
        world.wdata_mut(5, 5).region = 1;
        let mut regions = Regions::default();
        regions.list[1].coords.items.push((5, 5));
        regions.list[1].coords.capacity = 4;
        regions.list[1].size = 1;
        for coord in [(7, 4), (7, 5), (7, 6), (8, 5), (8, 6)] {
            world.wdata_mut(coord.0, coord.1).region = 2;
            world.wdata_mut(coord.0, coord.1).land = land::FERTILE;
            regions.list[2].coords.items.push(coord);
        }
        regions.list[2].coords.capacity = 8;
        regions.list[2].size = 5;
        (world, regions)
    }

    let (mut guarded_world, mut guarded_regions) = fixture();
    let guarded =
        execute_check_player_land(&mut guarded_world, &mut guarded_regions, call(1, 1, 1)).unwrap();
    let (mut open_world, mut open_regions) = fixture();
    let open =
        execute_check_player_land(&mut open_world, &mut open_regions, call(0, 1, 1)).unwrap();

    assert!(guarded.conflict_rejections > 0);
    assert!(guarded.outer_cells_written < open.outer_cells_written);
    assert_eq!(open.conflict_rejections, 0);
    assert_eq!(open.outer_cells_written, 8);
}

#[test]
fn malformed_parallel_start_arrays_fail_closed() {
    let mut world = World::init_default_rules(9, 9);
    world.start_city_x.items.push(4);
    world.start_city_x.capacity = 4;
    let mut regions = Regions::default();
    let before_world = world.clone();
    let before_regions = regions.clone();

    assert_eq!(
        execute_check_player_land(&mut world, &mut regions, call(1, 4, 6)),
        Err(CheckPlayerLandError::StartCityArrayMismatch { x_len: 1, y_len: 0 })
    );
    assert_eq!(world.wdata, before_world.wdata);
    assert_eq!(world.start_city_x, before_world.start_city_x);
    assert_eq!(regions, before_regions);
}
