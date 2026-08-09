// SPDX-License-Identifier: GPL-3.0-or-later

//! Mutation-sensitive coverage for `TerrainGroup::available_rough_tile`
//! (`0x006a24d0`) and `TerrainGroup::drop_tile` (`0x006a2ab0`).

use don_sim::rng::Random;
use don_sim::systems::map_terrain::{land, tflag, wflag, World};
use don_sim::systems::mountains::Mountains;
use don_sim::systems::regions::Regions;
use don_sim::systems::terrain_drop_tile::{
    AvailableRoughTileRejection, DropTileBranch, DropTileError, DropTileExternalRequest,
    DropTileExternalResolution,
};
use don_sim::systems::terrain_groups::{
    PlaceAllError, ResolvedRegionGroupPlacement, TerrainGroup, TerrainGroups,
    TerrainPlacementBoundary,
};
use don_sim::systems::terrain_region_placement::RegionDropTileInvocation;

fn fertile_world(xs: i32, ys: i32) -> World {
    let mut world = World::init_default_rules(xs, ys);
    for cell in &mut world.wdata {
        cell.land = land::FERTILE;
    }
    world
}

fn invocation(group_type: i32, x: i32, y: i32) -> RegionDropTileInvocation {
    RegionDropTileInvocation {
        world_x: x,
        world_y: y,
        group_type,
        group_radius: 3,
        land_subtype: 13,
        target_tiles: 9,
        oil_deposits: 2,
        group_index: 0,
    }
}

fn group(group_type: i32) -> TerrainGroup {
    TerrainGroup {
        group_type,
        ..TerrainGroup::default()
    }
}

#[test]
fn available_rough_tile_checks_mountain_forest_rock_then_coast_rings() {
    let mut world = fertile_world(9, 9);
    let mut terrain_group = group(6);
    terrain_group.mount_space = 2;
    terrain_group.forest_space = 2;
    terrain_group.rock_space = 2;
    terrain_group.coast_space = 2;
    // circle_init index 1 is (-1,-1). Multiple mutations prove the first
    // class-ring scan wins.
    world.wdata_mut(3, 3).flags |= wflag::MOUNTAINS | wflag::FOREST | wflag::ROCKS;

    let rejected = terrain_group
        .available_rough_tile(&world, 4, 4, wflag::ROCKS)
        .unwrap();
    assert_eq!(
        rejected.rejection,
        Some(AvailableRoughTileRejection::MountainSpacing { offset_index: 1 })
    );

    // A coordinate already placed by this same TerrainGroup is explicitly
    // exempt from all three rough-class spacing checks.
    terrain_group.tiles.items.push((3, 3));
    terrain_group.tiles.capacity = 4;
    let accepted = terrain_group
        .available_rough_tile(&world, 4, 4, wflag::FOREST)
        .unwrap();
    assert!(accepted.accepted);

    world.wdata_mut(3, 3).land = land::OCEAN;
    let coast = terrain_group
        .available_rough_tile(&world, 4, 4, wflag::FOREST)
        .unwrap();
    assert_eq!(
        coast.rejection,
        Some(AvailableRoughTileRejection::CoastSpacing { offset_index: 1 })
    );
}

#[test]
fn malformed_start_city_mask_fails_before_the_native_bit_probe() {
    let mut world = fertile_world(5, 5);
    world.start_city_locs.clear();
    let before_wdata = world.wdata.clone();
    let before_tdata = world.tdata.clone();
    let mut terrain_group = group(6);
    let before_group = terrain_group.clone();
    let mut random = Random::new(0x2345_6789);
    let before_rng = random;

    assert_eq!(
        terrain_group.apply_drop_tile(&mut world, &mut random, invocation(6, 2, 2), None,),
        Err(DropTileError::InvalidWorldShape {
            xs: 5,
            ys: 5,
            size: 25,
            wdata_len: 25,
            start_city_locs_len: 0,
            tile_xs: 20,
            tile_ys: 20,
            tile_size: 400,
            tdata_len: 400,
        })
    );
    assert_eq!(world.wdata, before_wdata);
    assert_eq!(world.tdata, before_tdata);
    assert_eq!(terrain_group, before_group);
    assert_eq!(random, before_rng);
}

#[test]
fn forest_branch_stamps_class_and_runs_all_sixteen_blocker_transactions() {
    let mut world = fertile_world(5, 5);
    let mut terrain_group = group(4);
    let mut random = Random::new(0x1234_5678);
    let before_rng = random;

    let receipt = terrain_group
        .apply_drop_tile(&mut world, &mut random, invocation(4, 2, 2), None)
        .unwrap();

    assert!(receipt.placed);
    assert_eq!(receipt.branch, DropTileBranch::Forest);
    assert_eq!(
        world.wdata(2, 2).flags & wflag::LAND_CLASS_MASK,
        wflag::FOREST
    );
    assert_eq!(world.wdata(2, 2).blocked, 16);
    assert_eq!(world.wdata(2, 2).solid, 16);
    for local_y in 0..4 {
        for local_x in 0..4 {
            assert_ne!(world.tmask(8 + local_x, 8 + local_y) & tflag::BLOCKED, 0);
        }
    }
    assert_eq!(terrain_group.tiles.items, [(2, 2)]);
    assert_eq!(terrain_group.tiles.capacity, 4);
    // The sixteen blocker transactions also propagate BAD_PATH to the
    // surrounding one-tile fringe. That changes the center WData record plus
    // its eight neighbours' `bad` counters in row-major order.
    assert_eq!(
        receipt.changed_wdata_indices,
        [6, 7, 8, 11, 12, 13, 16, 17, 18]
    );
    assert_eq!(
        receipt.changed_tdata_indices,
        [
            147, 148, 149, 150, 151, 152, 167, 168, 169, 170, 171, 172, 187, 188, 189, 190, 191,
            192, 207, 208, 209, 210, 211, 212, 227, 228, 229, 230, 231, 232, 247, 248, 249, 250,
            251, 252,
        ]
    );
    assert_eq!(random, before_rng);
}

#[test]
fn forest_radius_is_measured_from_the_first_tile_and_fails_before_world_writes() {
    let mut world = fertile_world(8, 8);
    let before = world.clone();
    let mut terrain_group = group(4);
    terrain_group.tiles.items.push((1, 1));
    terrain_group.tiles.capacity = 4;
    let mut call = invocation(4, 5, 5);
    call.group_radius = 2;
    let mut random = Random::new(3);

    let receipt = terrain_group
        .apply_drop_tile(&mut world, &mut random, call, None)
        .unwrap();

    assert!(!receipt.placed);
    assert_eq!(world.wdata, before.wdata);
    assert_eq!(world.tdata, before.tdata);
    assert_eq!(terrain_group.tiles.items, [(1, 1)]);
}

#[test]
fn rock_branch_checks_neighbor_mountain_tiles_then_writes_only_rough_class() {
    let mut blocked = fertile_world(6, 6);
    // NW is neighbor index 0 in the shipped eight-neighbor tables.
    blocked.set_mountain_at(4, 4, true);
    let mut terrain_group = group(6);
    let mut random = Random::new(4);
    let rejected = terrain_group
        .apply_drop_tile(&mut blocked, &mut random, invocation(6, 2, 2), None)
        .unwrap();
    assert!(!rejected.placed);
    assert_eq!(
        rejected.available_rough_tile.unwrap().rejection,
        Some(AvailableRoughTileRejection::RockTouchesMountainTiles { neighbor_index: 0 })
    );

    let mut open = fertile_world(6, 6);
    let before_tdata = open.tdata.clone();
    let placed = terrain_group
        .apply_drop_tile(&mut open, &mut random, invocation(6, 2, 2), None)
        .unwrap();
    assert!(placed.placed);
    assert_eq!(
        open.wdata(2, 2).flags & wflag::LAND_CLASS_MASK,
        wflag::ROCKS
    );
    assert_eq!(open.tdata, before_tdata);
    assert!(placed.changed_tdata_indices.is_empty());
}

#[test]
fn mountain_request_pins_all_nine_arguments_and_liberr_zero_success() {
    let world = fertile_world(6, 6);
    let mut terrain_group = TerrainGroup {
        group_type: 5,
        mount_space: 4,
        forest_space: 5,
        rock_space: 6,
        coast_space: 7,
        start_min: 8,
        ..TerrainGroup::default()
    };
    let call = invocation(5, 2, 3);
    let request = terrain_group.drop_tile_external_request(call).unwrap();
    assert_eq!(
        request,
        DropTileExternalRequest::MountainsAddMountain {
            template: 13,
            world_x: 2,
            world_y: 3,
            pattern: 4,
            mountain_space: 4,
            forest_space: 5,
            rock_space: 6,
            coast_space: 7,
            start_min: 8,
        }
    );
    let mut failed_world = world.clone();
    let mut random = Random::new(5);
    let failed = terrain_group
        .apply_drop_tile(
            &mut failed_world,
            &mut random,
            call,
            Some(DropTileExternalResolution::Mountains { request, liberr: 1 }),
        )
        .unwrap();
    assert!(!failed.placed);
    assert!(terrain_group.tiles.items.is_empty());

    let mut placed_world = world;
    let placed = terrain_group
        .apply_drop_tile(
            &mut placed_world,
            &mut random,
            call,
            Some(DropTileExternalResolution::Mountains { request, liberr: 0 }),
        )
        .unwrap();
    assert!(placed.placed);
    assert_eq!(terrain_group.tiles.items, [(2, 3)]);
    assert_eq!(placed.rng_draws, 0);
}

#[test]
fn oil_branch_requires_object_ack_then_sets_flag_and_appends_coordinate() {
    let mut world = fertile_world(6, 6);
    let before = world.clone();
    let mut terrain_group = group(7);
    let call = invocation(7, 2, 3);
    let request = terrain_group.drop_tile_external_request(call).unwrap();
    assert_eq!(
        request,
        DropTileExternalRequest::OilGoodMutation {
            world_x: 2,
            world_y: 3,
            enabled: true,
            good_type: 5,
            coord_x: 0x780,
            coord_y: 0xa80,
        }
    );
    let mut random = Random::new(6);
    assert_eq!(
        terrain_group.apply_drop_tile(&mut world, &mut random, call, None),
        Err(DropTileError::ExternalResolutionRequired { request })
    );
    assert_eq!(world.wdata, before.wdata);

    let receipt = terrain_group
        .apply_drop_tile(
            &mut world,
            &mut random,
            call,
            Some(DropTileExternalResolution::OilGoodsApplied { request }),
        )
        .unwrap();
    assert!(receipt.placed);
    assert_ne!(world.wdata(2, 3).flags & wflag::OIL, 0);
    assert_eq!(terrain_group.tiles.items, [(2, 3)]);
    assert_eq!(receipt.changed_wdata_indices, [20]);
}

#[test]
fn cliff_request_pins_abi_and_applies_its_zero_or_one_rng_draw() {
    let mut world = fertile_world(6, 6);
    let mut terrain_group = group(8);
    let call = invocation(8, 2, 3);
    let request = terrain_group.drop_tile_external_request(call).unwrap();
    assert_eq!(
        request,
        DropTileExternalRequest::CliffsPositionCliff {
            world_x: 2,
            world_y: 3,
            cliff_type: 12,
            target_x: -1,
            target_y: -1,
            mode: 0,
            facing: 0,
            anchor_y: 3,
        }
    );
    let mut random = Random::new(7);
    let mut expected = random;
    expected.advance();
    let receipt = terrain_group
        .apply_drop_tile(
            &mut world,
            &mut random,
            call,
            Some(DropTileExternalResolution::Cliffs {
                request,
                return_value: 1,
                rng_draws: 1,
            }),
        )
        .unwrap();
    assert!(receipt.placed);
    assert_eq!(receipt.rng_draws, 1);
    assert_eq!(random, expected);

    let before = random;
    assert_eq!(
        terrain_group.apply_drop_tile(
            &mut world,
            &mut random,
            call,
            Some(DropTileExternalResolution::Cliffs {
                request,
                return_value: 1,
                rng_draws: 2,
            }),
        ),
        Err(DropTileError::InvalidExternalRngDraws {
            group_type: 8,
            draws: 2,
        })
    );
    assert_eq!(random, before);
}

#[test]
fn place_all_surfaces_then_consumes_the_oil_good_boundary_transactionally() {
    let world = World::init_default_rules(5, 5);
    let mut regions = Regions::default();
    regions.list[65].coords.items = vec![(2, 2)];
    regions.list[65].coords.capacity = 4;
    let terrain_group = TerrainGroup {
        group_type: 7,
        chance: 100,
        min_clumps: 1,
        max_clumps: 1,
        pattern: 1,
        min_size: 1,
        max_size: 1,
        ..TerrainGroup::default()
    };
    let original_groups = TerrainGroups {
        groups: vec![terrain_group],
        ..TerrainGroups::default()
    };
    let mut groups = original_groups.clone();
    let mut unresolved_world = world.clone();
    let mut mountains = Mountains::default();
    let mut random = Random::new(8);
    let before_rng = random;
    let base = ResolvedRegionGroupPlacement {
        group_index: 0,
        clump_index: 0,
        region_id: 65,
        land_subtype: 13,
        rng_state_at_call: 0x1234,
        helping: None,
        drop_tile_externals: Vec::new(),
    };
    let error = groups
        .place_all_with_resolved_region_group(
            &mut unresolved_world,
            &regions,
            &mut random,
            &mut mountains,
            0,
            0,
            base.clone(),
            |_| {},
        )
        .unwrap_err();
    let PlaceAllError::GameplayPlacementUnavailable { preview, boundary } = error else {
        panic!("unexpected error: {error:?}");
    };
    let TerrainPlacementBoundary::RegionGroupDropTileExternalSubsystem { request } = boundary
    else {
        panic!("unexpected boundary: {boundary:?}");
    };
    assert!(preview.region_group_drop.is_none());

    let mut resolved = base;
    resolved
        .drop_tile_externals
        .push(DropTileExternalResolution::OilGoodsApplied { request });
    let mut resolved_world = world.clone();
    let error = groups
        .place_all_with_resolved_region_group(
            &mut resolved_world,
            &regions,
            &mut random,
            &mut mountains,
            0,
            0,
            resolved,
            |_| {},
        )
        .unwrap_err();
    let PlaceAllError::GameplayPlacementUnavailable { preview, boundary } = error else {
        panic!("unexpected error: {error:?}");
    };
    assert_eq!(
        boundary,
        TerrainPlacementBoundary::RegionGroupReturnControl {
            group_index: 0,
            return_value: 1,
        }
    );
    assert!(preview.region_group_drop.unwrap().placed);
    assert_eq!(groups, original_groups);
    assert_eq!(resolved_world.wdata, world.wdata);
    assert_eq!(random, before_rng);
}
