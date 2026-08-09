// SPDX-License-Identifier: GPL-3.0-or-later

//! Mixed terrain-group composition through `TerrainGroups::add_doobers`.

use don_sim::rng::Random;
use don_sim::systems::map_terrain::{land, wflag, World};
use don_sim::systems::mountains::Mountains;
use don_sim::systems::regions::Regions;
use don_sim::systems::terrain_doobers::{DooberTilesetRules, MountainRockFringeError};
use don_sim::systems::terrain_groups::{
    PlaceAllError, PlaceAllGroupInput, PlaceAllHostEvent, TerrainGroup, TerrainGroups,
    TerrainPlacementBoundary,
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

fn group(group_type: i32, pattern: i32) -> TerrainGroup {
    TerrainGroup {
        group_type,
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

fn rules() -> DooberTilesetRules {
    DooberTilesetRules {
        bush_clump_prob: 100,
        bush_spacing: 0,
        bush_min: 1,
        bush_max: 1,
        mountain_rock_clump_prob: 100,
        mountain_rock_spacing: 0,
        mountain_rock_min: 1,
        mountain_rock_max: 1,
        ..DooberTilesetRules::default()
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
fn completed_mixed_transaction_feeds_its_preview_world_to_both_doober_passes() {
    let mut world = world();
    // The mountain fringe guarantees a second-pass object receipt. There is no
    // forest in caller state: every first-pass receipt must therefore observe
    // the type-4 player arm's preview mutation.
    world.wdata_mut(3, 3).flags |= wflag::MOUNTAINS;
    assert!(world
        .wdata
        .iter()
        .all(|cell| cell.flags & wflag::FOREST == 0));
    let before_world = world.clone();
    let mut groups = TerrainGroups {
        groups: vec![group(4, 0)],
        ..TerrainGroups::default()
    };
    let before_groups = groups.clone();
    let mut mountains = Mountains::default();
    let before_mountains = mountains.clone();
    let mut random = Random::new(0x1234_5678);
    let before_random = random;
    let regions = Regions::default();
    let inputs = [PlaceAllGroupInput::Player {
        group_index: 0,
        externals: Vec::new(),
    }];
    let mut host = Vec::new();

    let error = groups
        .place_all_with_group_and_doober_inputs(
            &mut world,
            &regions,
            &mut random,
            &mut mountains,
            0,
            1,
            Some(helping()),
            &inputs,
            rules(),
            |event| host.push(event),
        )
        .unwrap_err();
    let PlaceAllError::GameplayPlacementUnavailable { preview, boundary } = error else {
        panic!("unexpected place_all result: {error:?}");
    };

    assert_eq!(boundary, TerrainPlacementBoundary::TreeifyMountainsMapStyle);
    assert_eq!(preview.completed_placement_groups, [0]);
    let bush = preview.bush_fringe.expect("bush pass must complete");
    let mountain = preview
        .mountain_rock_fringe
        .expect("mountain-rock pass must complete");
    assert!(!bush.placements.is_empty());
    assert!(!mountain.placements.is_empty());

    let first_doober = host
        .iter()
        .position(|event| {
            matches!(
                event,
                PlaceAllHostEvent::AddBushDoober { .. }
                    | PlaceAllHostEvent::AddMountainRockDoober { .. }
            )
        })
        .expect("doober effects must be surfaced");
    assert_eq!(
        &host[..first_doober],
        [
            PlaceAllHostEvent::NetDaemonProcessAll { group_index: 0 },
            PlaceAllHostEvent::NetDaemonProcessAllPlayer {
                group_index: 0,
                clump_index: 0,
                player_index: 0,
            },
        ]
    );
    let emitted_bush = host
        .iter()
        .filter_map(|event| match event {
            PlaceAllHostEvent::AddBushDoober { placement } => Some(*placement),
            _ => None,
        })
        .collect::<Vec<_>>();
    let emitted_mountain = host
        .iter()
        .filter_map(|event| match event {
            PlaceAllHostEvent::AddMountainRockDoober { placement } => Some(*placement),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(emitted_bush, bush.placements);
    assert_eq!(emitted_mountain, mountain.placements);
    assert!(host[first_doober..first_doober + emitted_bush.len()]
        .iter()
        .all(|event| matches!(event, PlaceAllHostEvent::AddBushDoober { .. })));
    assert!(host[first_doober + emitted_bush.len()..]
        .iter()
        .all(|event| matches!(event, PlaceAllHostEvent::AddMountainRockDoober { .. })));

    assert_eq!(world.wdata, before_world.wdata);
    assert_eq!(world.tdata, before_world.tdata);
    assert_eq!(groups, before_groups);
    assert_eq!(mountains, before_mountains);
    assert_eq!(random, before_random);
}

#[test]
fn invalid_second_pass_rules_fail_before_group_pumps_or_rng_draws() {
    let mut world = world();
    let before_world = world.clone();
    let mut groups = TerrainGroups {
        groups: vec![group(4, 0)],
        ..TerrainGroups::default()
    };
    let before_groups = groups.clone();
    let mut mountains = Mountains::default();
    let before_mountains = mountains.clone();
    let mut random = Random::new(0x2468_1357);
    let before_random = random;
    let regions = Regions::default();
    let inputs = [PlaceAllGroupInput::Player {
        group_index: 0,
        externals: Vec::new(),
    }];
    let mut invalid = rules();
    invalid.mountain_rock_max = 6;
    let mut host = Vec::new();

    let error = groups.place_all_with_group_and_doober_inputs(
        &mut world,
        &regions,
        &mut random,
        &mut mountains,
        0,
        1,
        Some(helping()),
        &inputs,
        invalid,
        |event| host.push(event),
    );

    assert_eq!(
        error,
        Err(PlaceAllError::InvalidMountainRockFringe(
            MountainRockFringeError::UnsupportedMountainRockCount {
                maximum_positive_count: 6,
                supported: 5,
            }
        ))
    );
    assert!(host.is_empty());
    assert_eq!(world.wdata, before_world.wdata);
    assert_eq!(world.tdata, before_world.tdata);
    assert_eq!(groups, before_groups);
    assert_eq!(mountains, before_mountains);
    assert_eq!(random, before_random);
}

#[test]
fn unfinished_selected_arm_suppresses_both_doober_effect_streams() {
    let mut world = world();
    let mut groups = TerrainGroups {
        groups: vec![group(4, 0), group(6, 2)],
        ..TerrainGroups::default()
    };
    let mut mountains = Mountains::default();
    let mut random = Random::new(0x1020_3040);
    let regions = Regions::default();
    let inputs = [PlaceAllGroupInput::Player {
        group_index: 0,
        externals: Vec::new(),
    }];
    let mut host = Vec::new();

    let error = groups
        .place_all_with_group_and_doober_inputs(
            &mut world,
            &regions,
            &mut random,
            &mut mountains,
            0,
            1,
            Some(helping()),
            &inputs,
            rules(),
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
    assert!(preview.bush_fringe.is_none());
    assert!(preview.mountain_rock_fringe.is_none());
    assert!(host.iter().all(|event| !matches!(
        event,
        PlaceAllHostEvent::AddBushDoober { .. } | PlaceAllHostEvent::AddMountainRockDoober { .. }
    )));
}
