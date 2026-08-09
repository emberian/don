// SPDX-License-Identifier: GPL-3.0-or-later

//! Mixed terrain-group composition through the map-style treeification gate.

use don_sim::rng::Random;
use don_sim::systems::map_terrain::{land, tflag, wflag, World};
use don_sim::systems::mountains::Mountains;
use don_sim::systems::regions::Regions;
use don_sim::systems::terrain_doobers::DooberTilesetRules;
use don_sim::systems::terrain_groups::{
    PlaceAllError, PlaceAllGroupInput, PlaceAllHostEvent, TerrainGroup, TerrainGroups,
    TerrainPlacementBoundary, TreeifyMountainsError, TreeifyMutationKind,
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

fn group() -> TerrainGroup {
    TerrainGroup {
        group_type: 6,
        chance: 100,
        min_clumps: 1,
        max_clumps: 1,
        pattern: 0,
        min_size: 1,
        max_size: 1,
        start_min: 0,
        start_max: 1,
        ..TerrainGroup::default()
    }
}

fn rules() -> DooberTilesetRules {
    DooberTilesetRules {
        mountain_fringe_tree_prob: 100,
        bush_clump_prob: 100,
        bush_min: 1,
        bush_max: 1,
        mountain_rock_clump_prob: 100,
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

fn inputs() -> [PlaceAllGroupInput; 1] {
    [PlaceAllGroupInput::Player {
        group_index: 0,
        externals: Vec::new(),
    }]
}

#[test]
fn non_nine_style_treeifies_after_group_and_both_doober_rng_passes() {
    let mut world = world();
    world.wdata_mut(5, 5).flags |= wflag::FOREST;
    world.wdata_mut(7, 7).flags |= wflag::MOUNTAINS;
    *world.tmask_mut(12, 12) = tflag::BLOCKER_MOUNTAIN;
    let before_world = world.clone();
    let mut groups = TerrainGroups {
        groups: vec![group()],
        ..TerrainGroups::default()
    };
    let before_groups = groups.clone();
    let mut mountains = Mountains::default();
    let before_mountains = mountains.clone();
    let mut random = Random::new(0x1234_5678);
    let before_random = random;
    let regions = Regions::default();
    let mut host = Vec::new();

    let error = groups
        .place_all_with_group_treeify_inputs(
            &mut world,
            &regions,
            &mut random,
            &mut mountains,
            0,
            1,
            Some(helping()),
            &inputs(),
            rules(),
            0,
            |event| host.push(event),
        )
        .unwrap_err();
    let PlaceAllError::GameplayPlacementUnavailable { preview, boundary } = error else {
        panic!("unexpected place_all result: {error:?}");
    };

    assert_eq!(boundary, TerrainPlacementBoundary::PostPlacementReporting);
    assert_eq!(preview.completed_placement_groups, [0]);
    let bush = preview.bush_fringe.expect("bush pass must complete");
    let mountain = preview
        .mountain_rock_fringe
        .expect("mountain-rock pass must complete");
    assert!(!bush.placements.is_empty());
    assert!(!mountain.placements.is_empty());
    let gate = preview
        .treeify_mountains
        .expect("treeification gate must complete");
    assert_eq!(gate.map_style, 0);
    assert!(gate.called);
    let treeify = gate.treeify.expect("treeification body must run");
    let mountain_tcoord_mutation = treeify
        .mutations
        .iter()
        .find(|mutation| mutation.world_x == 3 && mutation.world_y == 3)
        .expect("the supplied mountain TCoord must be scanned");
    assert_eq!(mountain_tcoord_mutation.kind, TreeifyMutationKind::Forest);
    assert_eq!(mountain_tcoord_mutation.flags_before, 0);
    assert_ne!(treeify.rng_state_after, mountain.rng_state_after);
    assert!(matches!(
        host.first(),
        Some(PlaceAllHostEvent::NetDaemonProcessAll { group_index: 0 })
    ));
    assert!(host
        .iter()
        .any(|event| matches!(event, PlaceAllHostEvent::AddBushDoober { .. })));
    assert!(host
        .iter()
        .any(|event| matches!(event, PlaceAllHostEvent::AddMountainRockDoober { .. })));

    assert_eq!(world.wdata, before_world.wdata);
    assert_eq!(world.tdata, before_world.tdata);
    assert_eq!(groups, before_groups);
    assert_eq!(mountains, before_mountains);
    assert_eq!(random, before_random);
}

#[test]
fn style_nine_records_the_exact_skip_without_consuming_treeify_rng() {
    let mut world = world();
    world.wdata_mut(5, 5).flags |= wflag::FOREST;
    world.wdata_mut(7, 7).flags |= wflag::MOUNTAINS;
    *world.tmask_mut(12, 12) = tflag::BLOCKER_MOUNTAIN;
    let mut groups = TerrainGroups {
        groups: vec![group()],
        ..TerrainGroups::default()
    };
    let mut mountains = Mountains::default();
    let mut random = Random::new(0x2468_1357);
    let regions = Regions::default();

    let error = groups
        .place_all_with_group_treeify_inputs(
            &mut world,
            &regions,
            &mut random,
            &mut mountains,
            0,
            1,
            Some(helping()),
            &inputs(),
            rules(),
            9,
            |_| {},
        )
        .unwrap_err();
    let PlaceAllError::GameplayPlacementUnavailable { preview, boundary } = error else {
        panic!("unexpected place_all result: {error:?}");
    };

    assert_eq!(boundary, TerrainPlacementBoundary::PostPlacementReporting);
    let mountain_rng = preview
        .mountain_rock_fringe
        .expect("mountain-rock pass must complete")
        .rng_state_after;
    let gate = preview
        .treeify_mountains
        .expect("treeification gate must complete");
    assert_eq!(gate.map_style, 9);
    assert!(!gate.called);
    assert!(gate.treeify.is_none());
    assert_eq!(gate.rng_state_after, mountain_rng);
}

#[test]
fn malformed_treeify_storage_fails_before_group_or_doober_host_effects() {
    let mut world = world();
    world.tile_size -= 1;
    let before_world = world.clone();
    let mut groups = TerrainGroups {
        groups: vec![group()],
        ..TerrainGroups::default()
    };
    let before_groups = groups.clone();
    let mut mountains = Mountains::default();
    let before_mountains = mountains.clone();
    let mut random = Random::new(0x1020_3040);
    let before_random = random;
    let regions = Regions::default();
    let mut host = Vec::new();

    let error = groups.place_all_with_group_treeify_inputs(
        &mut world,
        &regions,
        &mut random,
        &mut mountains,
        0,
        1,
        Some(helping()),
        &inputs(),
        rules(),
        0,
        |event| host.push(event),
    );

    assert!(matches!(
        error,
        Err(PlaceAllError::InvalidTreeifyMountains(
            TreeifyMountainsError::InvalidWorldShape { .. }
        ))
    ));
    assert!(host.is_empty());
    assert_eq!(world.wdata, before_world.wdata);
    assert_eq!(world.tdata, before_world.tdata);
    assert_eq!(groups, before_groups);
    assert_eq!(mountains, before_mountains);
    assert_eq!(random, before_random);
}
