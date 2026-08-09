// SPDX-License-Identifier: GPL-3.0-or-later

//! Heterogeneous typed-input composition for `TerrainGroups::place_all`.

use don_sim::rng::Random;
use don_sim::systems::map_terrain::{land, World};
use don_sim::systems::mountains::Mountains;
use don_sim::systems::regions::Regions;
use don_sim::systems::terrain_groups::{
    PlaceAllError, PlaceAllGroupInput, PlaceAllHostEvent, TerrainGroup, TerrainGroupInputKind,
    TerrainGroups, TerrainPlacementBoundary,
};
use don_sim::systems::terrain_region_placement::RegionHelpingState;

fn world() -> World {
    let mut world = World::init_default_rules(25, 25);
    for cell in &mut world.wdata {
        cell.land = land::FERTILE;
        cell.region = 1;
    }
    world.start_x.items.push(12);
    world.start_y.items.push(12);
    world
}

fn regions() -> Regions {
    let mut regions = Regions::default();
    regions.list[1].size = 6;
    regions.list[1].coords.items.push((3, 3));
    regions.list[1].coords.capacity = 1;
    regions
}

fn group(pattern: i32) -> TerrainGroup {
    TerrainGroup {
        group_type: 6,
        chance: 100,
        min_clumps: 1,
        max_clumps: 1,
        pattern,
        min_size: 1,
        max_size: 1,
        start_min: 0,
        start_max: 1,
        ..TerrainGroup::default()
    }
}

fn helping() -> RegionHelpingState {
    RegionHelpingState {
        is_helping: false,
        num_players: 1,
        lowest_player: [0; 5],
        scores: [[0; 5]; 8],
    }
}

#[test]
fn player_region_player_rows_execute_in_one_preview_transaction() {
    let mut world = world();
    let regions = regions();
    let mut groups = TerrainGroups {
        groups: vec![group(0), group(2), group(0)],
        ..TerrainGroups::default()
    };
    let mut mountains = Mountains::default();
    let mut random = Random::new(0x1234_5678);
    let before_world = world.clone();
    let before_groups = groups.clone();
    let before_mountains = mountains.clone();
    let before_random = random;
    let mut host = Vec::new();
    let inputs = vec![
        PlaceAllGroupInput::Player {
            group_index: 0,
            externals: Vec::new(),
        },
        PlaceAllGroupInput::Region {
            group_index: 1,
            externals: Vec::new(),
        },
        PlaceAllGroupInput::Player {
            group_index: 2,
            externals: Vec::new(),
        },
    ];

    let error = groups
        .place_all_with_group_inputs(
            &mut world,
            &regions,
            &mut random,
            &mut mountains,
            0,
            1,
            Some(helping()),
            &inputs,
            |event| host.push(event),
        )
        .unwrap_err();
    let PlaceAllError::GameplayPlacementUnavailable { preview, boundary } = error else {
        panic!("unexpected place_all result: {error:?}");
    };

    assert_eq!(boundary, TerrainPlacementBoundary::AddDoobers);
    assert_eq!(preview.completed_placement_groups, [0, 1, 2]);
    assert_eq!(
        preview
            .player_group_dispatches
            .iter()
            .map(|dispatch| dispatch.group_index)
            .collect::<Vec<_>>(),
        [0, 2]
    );
    assert_eq!(preview.region_pattern_dispatches.len(), 1);
    assert_eq!(preview.region_pattern_dispatches[0].group_index, 1);
    assert_eq!(
        host,
        [
            PlaceAllHostEvent::NetDaemonProcessAll { group_index: 0 },
            PlaceAllHostEvent::NetDaemonProcessAllPlayer {
                group_index: 0,
                clump_index: 0,
                player_index: 0,
            },
            PlaceAllHostEvent::NetDaemonProcessAll { group_index: 1 },
            PlaceAllHostEvent::NetDaemonProcessAll { group_index: 2 },
            PlaceAllHostEvent::NetDaemonProcessAllPlayer {
                group_index: 2,
                clump_index: 0,
                player_index: 0,
            },
        ]
    );
    assert_eq!(world.wdata, before_world.wdata);
    assert_eq!(world.tdata, before_world.tdata);
    assert_eq!(groups, before_groups);
    assert_eq!(mountains, before_mountains);
    assert_eq!(random, before_random);
}

#[test]
fn omitted_next_row_stops_at_its_prepared_kernel_without_gameplay_host_effects() {
    let mut world = world();
    let regions = regions();
    let mut groups = TerrainGroups {
        groups: vec![group(0), group(2)],
        ..TerrainGroups::default()
    };
    let mut mountains = Mountains::default();
    let mut random = Random::new(0x2468_1357);
    let mut host = Vec::new();
    let inputs = [PlaceAllGroupInput::Player {
        group_index: 0,
        externals: Vec::new(),
    }];

    let error = groups
        .place_all_with_group_inputs(
            &mut world,
            &regions,
            &mut random,
            &mut mountains,
            0,
            1,
            Some(helping()),
            &inputs,
            |event| host.push(event),
        )
        .unwrap_err();
    let PlaceAllError::GameplayPlacementUnavailable { preview, boundary } = error else {
        panic!("unexpected place_all result: {error:?}");
    };

    assert_eq!(
        boundary,
        TerrainPlacementBoundary::UnitTypeCatalogAndRegionPlacementKernel {
            group_index: 1,
            pattern: 2,
        }
    );
    assert_eq!(preview.completed_placement_groups, [0]);
    assert!(preview.region_pattern_dispatches.is_empty());
    assert_eq!(
        host,
        [
            PlaceAllHostEvent::NetDaemonProcessAll { group_index: 0 },
            PlaceAllHostEvent::NetDaemonProcessAllPlayer {
                group_index: 0,
                clump_index: 0,
                player_index: 0,
            },
            PlaceAllHostEvent::NetDaemonProcessAll { group_index: 1 },
        ]
    );
}

#[test]
fn conflicting_row_kind_fails_closed_before_the_player_kernel() {
    let mut world = world();
    let regions = regions();
    let mut groups = TerrainGroups {
        groups: vec![group(0)],
        ..TerrainGroups::default()
    };
    let mut mountains = Mountains::default();
    let mut random = Random::new(0x1020_3040);
    let before_world = world.clone();
    let before_groups = groups.clone();
    let before_mountains = mountains.clone();
    let before_random = random;
    let mut host = Vec::new();
    let inputs = [PlaceAllGroupInput::Region {
        group_index: 0,
        externals: Vec::new(),
    }];

    let error = groups
        .place_all_with_group_inputs(
            &mut world,
            &regions,
            &mut random,
            &mut mountains,
            0,
            1,
            Some(helping()),
            &inputs,
            |event| host.push(event),
        )
        .unwrap_err();

    assert_eq!(
        error,
        PlaceAllError::InvalidMixedGroupInput {
            expected_group_index: 0,
            expected_kind: TerrainGroupInputKind::Player,
            actual_group_index: 0,
            actual_kind: TerrainGroupInputKind::Region,
        }
    );
    assert_eq!(
        host,
        [PlaceAllHostEvent::NetDaemonProcessAll { group_index: 0 }]
    );
    assert_eq!(world.wdata, before_world.wdata);
    assert_eq!(world.tdata, before_world.tdata);
    assert_eq!(groups, before_groups);
    assert_eq!(mountains, before_mountains);
    assert_eq!(random, before_random);
}
