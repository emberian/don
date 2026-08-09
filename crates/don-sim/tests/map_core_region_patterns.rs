// SPDX-License-Identifier: GPL-3.0-or-later

//! Exact pattern-1/2/3 region and clump control in `TerrainGroups::place_all`.

use don_sim::rng::Random;
use don_sim::systems::map_terrain::{land, wflag, World};
use don_sim::systems::mountains::{MountainRangeEntry, MountainRangeList, Mountains};
use don_sim::systems::regions::Regions;
use don_sim::systems::terrain_drop_tile::{DropTileExternalRequest, DropTileExternalResolution};
use don_sim::systems::terrain_groups::{
    PlaceAllError, TerrainGroup, TerrainGroups, TerrainPlacementBoundary,
};
use don_sim::systems::terrain_region_patterns::{RegionPatternOutcome, RegionPatternReceipt};
use don_sim::systems::terrain_region_placement::RegionHelpingState;

fn world(xs: i32, ys: i32) -> World {
    let mut world = World::init_default_rules(xs, ys);
    for cell in &mut world.wdata {
        cell.land = land::FERTILE;
    }
    world
}

fn region(regions: &mut Regions, id: usize, size: i32, coords: &[(i32, i32)]) {
    regions.list[id].size = size;
    regions.list[id].coords.items = coords.to_vec();
    regions.list[id].coords.capacity = coords.len() as i32;
}

fn group(group_type: i32) -> TerrainGroup {
    TerrainGroup {
        group_type,
        ..TerrainGroup::default()
    }
}

#[test]
fn pattern_one_is_region_major_then_retries_each_failed_clump_as_helping() {
    let mut world = world(9, 9);
    let mut regions = Regions::default();
    region(&mut regions, 1, 6, &[(2, 2)]);
    region(&mut regions, 2, 6, &[(6, 6)]);
    let mut terrain_group = group(6);
    let mut mountains = Mountains::default();
    let mut random = Random::new(4);

    let receipt = terrain_group
        .apply_region_pattern(
            &mut world,
            &regions,
            &mut random,
            &mut mountains,
            1,
            &[1, 1],
            &[0, 0],
            1,
            0,
            0,
            None,
            &[],
        )
        .unwrap();

    assert_eq!(receipt.eligible_regions, [1, 2]);
    assert_eq!(receipt.failed_clumps, [1, 1]);
    assert_eq!(
        receipt
            .calls
            .iter()
            .map(|call| call.region_id)
            .collect::<Vec<_>>(),
        [1, 1, 2, 2, 1, 2, 1, 2]
    );
    assert_eq!(
        receipt
            .calls
            .iter()
            .map(|call| call.is_helping)
            .collect::<Vec<_>>(),
        [false, true, false, true, true, true, true, true]
    );
    assert_eq!(terrain_group.placed, [1, 0, 1, 0]);
    assert_eq!(receipt.outcome, RegionPatternOutcome::Complete);
    assert_eq!(random, Random::new(4));
}

#[test]
fn pattern_two_failed_clump_walks_the_closed_region_cycle_until_marker_wrap() {
    let mut world = world(9, 9);
    // Region 2 survives the size scan but its only candidate fails entry.
    world.wdata_mut(6, 6).flags = wflag::MOUNTAINS;
    let mut regions = Regions::default();
    region(&mut regions, 1, 6, &[(2, 2)]);
    region(&mut regions, 2, 6, &[(6, 6)]);
    let mut terrain_group = group(6);
    let mut mountains = Mountains::default();
    let mut random = Random::new(1);

    let receipt = terrain_group
        .apply_region_pattern(
            &mut world,
            &regions,
            &mut random,
            &mut mountains,
            2,
            &[1, 1],
            &[0, 0],
            9,
            0,
            0,
            None,
            &[],
        )
        .unwrap();

    assert_eq!(receipt.region_cycle, [1, 2]);
    assert_eq!(receipt.initial_cycle_index, Some(1));
    assert_eq!(receipt.region_selection_draws, 1);
    assert_eq!(
        receipt
            .calls
            .iter()
            .map(|call| call.region_id)
            .collect::<Vec<_>>(),
        [2, 1, 2]
    );
    assert_eq!(terrain_group.placed, [0, 1, 0]);
    assert_eq!(receipt.outcome, RegionPatternOutcome::Complete);
}

#[test]
fn pattern_three_excludes_every_player_start_region_before_selection() {
    let mut world = world(9, 9);
    world.start_x.items.push(2);
    world.start_y.items.push(2);
    world.wdata_mut(2, 2).region = 1;
    let mut regions = Regions::default();
    region(&mut regions, 1, 6, &[(2, 2)]);
    region(&mut regions, 2, 6, &[(6, 6)]);
    let helping = RegionHelpingState {
        is_helping: false,
        num_players: 1,
        lowest_player: [0; 5],
        scores: [[0; 5]; 8],
    };
    let mut terrain_group = group(6);
    let mut mountains = Mountains::default();
    let mut random = Random::new(7);

    let receipt = terrain_group
        .apply_region_pattern(
            &mut world,
            &regions,
            &mut random,
            &mut mountains,
            3,
            &[1],
            &[0],
            0,
            0,
            0,
            Some(helping),
            &[],
        )
        .unwrap();

    assert_eq!(receipt.eligible_regions, [2]);
    assert_eq!(receipt.region_cycle, [2]);
    assert_eq!(receipt.region_selection_draws, 0);
    assert_eq!(receipt.calls[0].region_id, 2);
    assert_eq!(receipt.calls[0].is_helping, true);
    assert_eq!(terrain_group.placed, [1]);
}

#[test]
fn sparse_pattern_two_cycle_uses_retail_f32_proportional_duplication() {
    let mut world = world(9, 9);
    let mut regions = Regions::default();
    region(&mut regions, 1, 6, &[(2, 2)]);
    region(&mut regions, 2, 18, &[(6, 6)]);
    let mut terrain_group = group(5);
    let mut mountains = Mountains {
        small_ranges: MountainRangeList::new(vec![MountainRangeEntry::new(42, 0)]),
        ..Mountains::default()
    };
    let mut random = Random::new(1);

    let receipt = terrain_group
        .apply_region_pattern(
            &mut world,
            &regions,
            &mut random,
            &mut mountains,
            2,
            &[1; 8],
            &[0; 8],
            9,
            0,
            0,
            None,
            &[],
        )
        .unwrap();

    assert_eq!(receipt.region_cycle, [1, 2, 1, 2, 2, 2, 2, 2, 2]);
    assert_eq!(receipt.region_selection_draws, 1);
    assert_eq!(receipt.calls.len(), 1);
    assert_eq!(receipt.calls[0].land_subtype, 42);
    assert!(matches!(
        receipt.outcome,
        RegionPatternOutcome::ExternalResolutionRequired { .. }
    ));
}

#[test]
fn pattern_two_type_five_failure_rotates_templates_before_advancing_clump() {
    let mut world = world(9, 9);
    let mut regions = Regions::default();
    region(&mut regions, 1, 6, &[(2, 2)]);
    let mut terrain_group = group(5);
    let mut mountains = Mountains {
        small_ranges: MountainRangeList::new(vec![
            MountainRangeEntry::new(10, 0),
            MountainRangeEntry::new(20, 0),
        ]),
        ..Mountains::default()
    };
    let request = |template| DropTileExternalRequest::MountainsAddMountain {
        template,
        world_x: 2,
        world_y: 2,
        pattern: 4,
        mountain_space: 0,
        forest_space: 0,
        rock_space: 0,
        coast_space: 0,
        start_min: 0,
    };
    let externals = [
        DropTileExternalResolution::Mountains {
            request: request(10),
            liberr: 1,
        },
        DropTileExternalResolution::Mountains {
            request: request(10),
            liberr: 1,
        },
        DropTileExternalResolution::Mountains {
            request: request(20),
            liberr: 0,
        },
    ];
    let mut random = Random::new(5);

    let receipt = terrain_group
        .apply_region_pattern(
            &mut world,
            &regions,
            &mut random,
            &mut mountains,
            2,
            &[1],
            &[0],
            9,
            0,
            0,
            None,
            &externals,
        )
        .unwrap();

    assert_eq!(
        receipt
            .calls
            .iter()
            .map(|call| call.land_subtype)
            .collect::<Vec<_>>(),
        [10, 10, 20]
    );
    assert_eq!(receipt.external_resolutions_consumed, 3);
    assert_eq!(terrain_group.placed, [1]);
    assert_eq!(terrain_group.tiles.items, [(2, 2)]);
    assert_eq!(receipt.outcome, RegionPatternOutcome::Complete);
}

#[test]
fn place_all_composes_pattern_loop_without_resolved_call_and_keeps_state_transactional() {
    let mut world = world(8, 8);
    let mut regions = Regions::default();
    region(&mut regions, 1, 6, &[(3, 3)]);
    let terrain_group = TerrainGroup {
        group_type: 6,
        chance: 100,
        min_clumps: 1,
        max_clumps: 1,
        pattern: 2,
        min_size: 1,
        max_size: 1,
        ..TerrainGroup::default()
    };
    let mut groups = TerrainGroups {
        groups: vec![terrain_group],
        ..TerrainGroups::default()
    };
    let mut mountains = Mountains::default();
    let mut random = Random::new(0x1234_5678);
    let before_world = world.clone();
    let before_groups = groups.clone();
    let before_mountains = mountains.clone();
    let before_random = random;
    let mut host = Vec::new();

    let error = groups
        .place_all_with_regions(
            &mut world,
            &regions,
            &mut random,
            &mut mountains,
            0,
            0,
            None,
            &[],
            |event| host.push(event),
        )
        .unwrap_err();
    let PlaceAllError::GameplayPlacementUnavailable { preview, boundary } = error else {
        panic!("unexpected error: {error:?}");
    };

    assert_eq!(boundary, TerrainPlacementBoundary::AddDoobers);
    assert_eq!(preview.completed_placement_groups, [0]);
    let pattern: RegionPatternReceipt = preview.region_pattern.unwrap();
    assert_eq!(pattern.calls.len(), 1);
    assert_eq!(pattern.calls[0].region_id, 1);
    assert_eq!(
        pattern.calls[0].placement.outcome,
        don_sim::systems::terrain_region_continuation::PlaceRegionGroupOutcome::Returned(1)
    );
    assert_eq!(host.len(), 1);
    assert_eq!(world.wdata, before_world.wdata);
    assert_eq!(world.tdata, before_world.tdata);
    assert_eq!(world.start_city_locs, before_world.start_city_locs);
    assert_eq!(groups, before_groups);
    assert_eq!(mountains, before_mountains);
    assert_eq!(random, before_random);
}

#[test]
fn same_kind_region_stream_carries_preview_state_across_selected_groups() {
    let mut world = world(8, 8);
    let mut regions = Regions::default();
    region(&mut regions, 1, 6, &[(3, 3)]);
    let terrain_group = TerrainGroup {
        group_type: 6,
        chance: 100,
        min_clumps: 1,
        max_clumps: 1,
        pattern: 2,
        min_size: 1,
        max_size: 1,
        ..TerrainGroup::default()
    };
    let mut groups = TerrainGroups {
        groups: vec![terrain_group.clone(), terrain_group],
        ..TerrainGroups::default()
    };
    let mut mountains = Mountains::default();
    let mut random = Random::new(0x1357_2468);
    let before_world = world.clone();
    let before_groups = groups.clone();
    let before_mountains = mountains.clone();
    let before_random = random;
    let mut host = Vec::new();

    let error = groups
        .place_all_with_regions(
            &mut world,
            &regions,
            &mut random,
            &mut mountains,
            0,
            0,
            None,
            &[],
            |event| host.push(event),
        )
        .unwrap_err();
    let PlaceAllError::GameplayPlacementUnavailable { preview, boundary } = error else {
        panic!("unexpected error: {error:?}");
    };

    assert_eq!(boundary, TerrainPlacementBoundary::AddDoobers);
    assert_eq!(preview.completed_placement_groups, [0, 1]);
    assert_eq!(
        preview
            .region_pattern_dispatches
            .iter()
            .map(|dispatch| dispatch.group_index)
            .collect::<Vec<_>>(),
        [0, 1]
    );
    assert!(preview
        .region_pattern_dispatches
        .iter()
        .all(|dispatch| dispatch.pattern.outcome == RegionPatternOutcome::Complete));
    assert_eq!(host.len(), 2);
    assert_eq!(world.wdata, before_world.wdata);
    assert_eq!(world.tdata, before_world.tdata);
    assert_eq!(world.start_city_locs, before_world.start_city_locs);
    assert_eq!(groups, before_groups);
    assert_eq!(mountains, before_mountains);
    assert_eq!(random, before_random);
}
