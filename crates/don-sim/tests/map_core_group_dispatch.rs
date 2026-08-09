// SPDX-License-Identifier: GPL-3.0-or-later

//! Exact `TerrainGroups::place_all` group-index continuation.

use don_sim::rng::Random;
use don_sim::systems::map_terrain::{land, World};
use don_sim::systems::mountains::Mountains;
use don_sim::systems::terrain_groups::{
    PlaceAllError, PlaceAllHostEvent, TerrainGroup, TerrainGroups, TerrainPlacementBoundary,
};

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

fn group(group_type: i32, chance: i32, pattern: i32) -> TerrainGroup {
    TerrainGroup {
        group_type,
        chance,
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

#[test]
fn completed_player_group_increments_skips_and_prepares_the_next_selected_group() {
    let mut world = world();
    let mut groups = TerrainGroups {
        groups: vec![group(6, 100, 0), group(4, 0, 0), group(4, 100, 1)],
        ..TerrainGroups::default()
    };
    let mut mountains = Mountains::default();
    let mut random = Random::new(0x1234_5678);
    let before_world = world.clone();
    let before_groups = groups.clone();
    let before_mountains = mountains.clone();
    let before_random = random;
    let mut host_events = Vec::new();

    let error = groups
        .place_all_with_player_group_inputs(
            &mut world,
            &mut random,
            &mut mountains,
            1,
            1,
            &[],
            |event| host_events.push(event),
        )
        .unwrap_err();
    let PlaceAllError::GameplayPlacementUnavailable { preview, boundary } = error else {
        panic!("unexpected place_all result: {error:?}");
    };

    assert_eq!(
        boundary,
        TerrainPlacementBoundary::UnitTypeCatalogAndRegionPlacementKernel {
            group_index: 2,
            pattern: 1,
        }
    );
    assert_eq!(preview.completed_placement_groups, [0]);
    assert_eq!(
        preview
            .placement_preparation
            .prepared_groups
            .iter()
            .map(|group| group.group_index)
            .collect::<Vec<_>>(),
        [0, 2]
    );
    assert_eq!(
        preview.placement_preparation.host_events,
        [
            PlaceAllHostEvent::NetDaemonProcessAll { group_index: 0 },
            PlaceAllHostEvent::ProgressDisplay {
                group_index: 0,
                pattern: 0,
            },
            PlaceAllHostEvent::NetDaemonProcessAll { group_index: 1 },
            PlaceAllHostEvent::NetDaemonProcessAll { group_index: 2 },
            PlaceAllHostEvent::ProgressDisplay {
                group_index: 2,
                pattern: 1,
            },
        ]
    );
    assert_eq!(
        host_events,
        [
            PlaceAllHostEvent::NetDaemonProcessAll { group_index: 0 },
            PlaceAllHostEvent::ProgressDisplay {
                group_index: 0,
                pattern: 0,
            },
            PlaceAllHostEvent::NetDaemonProcessAllPlayer {
                group_index: 0,
                clump_index: 0,
                player_index: 0,
            },
            PlaceAllHostEvent::NetDaemonProcessAll { group_index: 1 },
            PlaceAllHostEvent::NetDaemonProcessAll { group_index: 2 },
            PlaceAllHostEvent::ProgressDisplay {
                group_index: 2,
                pattern: 1,
            },
        ]
    );
    assert_eq!(preview.player_group_prefix.as_ref().unwrap().len(), 1);
    assert_eq!(world.wdata, before_world.wdata);
    assert_eq!(world.tdata, before_world.tdata);
    assert_eq!(groups, before_groups);
    assert_eq!(mountains, before_mountains);
    assert_eq!(random, before_random);
}
