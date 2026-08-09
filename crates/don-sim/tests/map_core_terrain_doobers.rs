// SPDX-License-Identifier: GPL-3.0-or-later

//! Mutation-sensitive tests for the first `TerrainGroups::add_doobers` pass.

use don_sim::rng::Random;
use don_sim::systems::map_terrain::{wflag, World};
use don_sim::systems::mountains::Mountains;
use don_sim::systems::terrain_doobers::{
    plan_bush_fringe, plan_bush_fringe_with_host, BushDooberPlacement, BushFringeError,
    DooberTilesetRules, ForestFringeEdge,
};
use don_sim::systems::terrain_groups::{
    PlaceAllError, PlaceAllHostEvent, TerrainGroup, TerrainGroups, TerrainPlacementBoundary,
};

fn rules() -> DooberTilesetRules {
    DooberTilesetRules {
        bush_clump_prob: 100,
        bush_spacing: 0,
        bush_min: 1,
        bush_max: 1,
        ..DooberTilesetRules::default()
    }
}

#[test]
fn isolated_forest_emits_south_then_east_with_exact_coordinates() {
    let mut world = World::init_default_rules(4, 4);
    world.wdata_mut(1, 1).flags |= wflag::FOREST;
    let mut random = Random::new(0x1234_5678);
    let mut host = Vec::new();

    let receipt = plan_bush_fringe_with_host(&world, rules(), &mut random, |placement| {
        host.push(placement)
    })
    .unwrap();

    assert_eq!(receipt.chance_draws, 2);
    assert_eq!(receipt.offset_draws, 2);
    assert_eq!(
        receipt.placements,
        vec![
            BushDooberPlacement {
                forest_x: 1,
                forest_y: 1,
                edge: ForestFringeEdge::South,
                x: 1028,
                y: 1389,
            },
            BushDooberPlacement {
                forest_x: 1,
                forest_y: 1,
                edge: ForestFringeEdge::East,
                x: 1426,
                y: 1311,
            },
        ]
    );
    assert_eq!(host, receipt.placements);
    assert_eq!(receipt.rng_state_after, random.state());
}

#[test]
fn forest_neighbor_mutation_reassigns_the_rng_stream_to_the_east_edge() {
    let mut isolated = World::init_default_rules(3, 3);
    isolated.wdata_mut(1, 1).flags |= wflag::FOREST;
    let mut connected = isolated.clone();
    // The south neighbor is a boundary cell and is not itself scanned.
    connected.wdata_mut(1, 2).flags |= wflag::FOREST;
    let mut isolated_rng = Random::new(0x1357_2468);
    let mut connected_rng = isolated_rng;

    let isolated_receipt = plan_bush_fringe(&isolated, rules(), &mut isolated_rng).unwrap();
    let connected_receipt = plan_bush_fringe(&connected, rules(), &mut connected_rng).unwrap();

    assert_eq!(isolated_receipt.placements[0].edge, ForestFringeEdge::South);
    assert_eq!(connected_receipt.placements[0].edge, ForestFringeEdge::East);
    assert_ne!(
        isolated_receipt.placements[1],
        connected_receipt.placements[0]
    );
}

#[test]
fn river_forest_and_rejected_chance_preserve_exact_draw_counts() {
    let mut world = World::init_default_rules(5, 4);
    world.wdata_mut(1, 1).flags |= wflag::FOREST | wflag::HAS_RIVER;
    world.wdata_mut(2, 1).flags |= wflag::FOREST;
    let mut rejected = rules();
    rejected.bush_clump_prob = 0;
    let mut random = Random::new(91);
    let before = random.state();

    let receipt = plan_bush_fringe(&world, rejected, &mut random).unwrap();

    assert!(receipt.placements.is_empty());
    assert_eq!(receipt.chance_draws, 2);
    assert_eq!(receipt.offset_draws, 0);
    assert_ne!(random.state(), before);
}

#[test]
fn malformed_shape_and_unsupported_count_fail_before_rng_or_host() {
    let mut world = World::init_default_rules(3, 3);
    world.wdata_mut(1, 1).flags |= wflag::FOREST;
    let mut random = Random::new(7);
    let before = random.state();
    let mut host_calls = 0;
    let mut unsupported = rules();
    unsupported.bush_max = 6;

    assert_eq!(
        plan_bush_fringe_with_host(&world, unsupported, &mut random, |_| host_calls += 1),
        Err(BushFringeError::UnsupportedBushCount {
            maximum_positive_count: 6,
            supported: 5,
        })
    );
    assert_eq!(random.state(), before);
    assert_eq!(host_calls, 0);

    world.size = 8;
    assert_eq!(
        plan_bush_fringe(&world, rules(), &mut random),
        Err(BushFringeError::InvalidWorldShape {
            xs: 3,
            ys: 3,
            size: 8,
            wdata_len: 9,
        })
    );
    assert_eq!(random.state(), before);
}

#[test]
fn place_all_advances_from_group_pump_through_bushes_to_occupancy_boundary() {
    let mut world = World::init_default_rules(4, 4);
    world.wdata_mut(1, 1).flags |= wflag::FOREST;
    let before_world = world.clone();
    let mut groups = TerrainGroups {
        groups: vec![TerrainGroup {
            group_type: 4,
            chance: 100,
            min_clumps: 0,
            max_clumps: 0,
            pattern: 99,
            ..TerrainGroup::default()
        }],
        ..TerrainGroups::default()
    };
    let before_groups = groups.clone();
    let mut mountains = Mountains::default();
    let before_mountains = mountains.clone();
    let mut random = Random::new(0x1234_5678);
    let before_random = random.state();
    let mut host = Vec::new();

    let error = groups
        .place_all_with_doober_rules(
            &mut world,
            &mut random,
            &mut mountains,
            0,
            0,
            rules(),
            |event| host.push(event),
        )
        .unwrap_err();
    let PlaceAllError::GameplayPlacementUnavailable { preview, boundary } = error else {
        panic!("unexpected place_all result: {error:?}");
    };

    assert_eq!(
        boundary,
        TerrainPlacementBoundary::DooberOccupancyRegistryAndRockFringe
    );
    assert_eq!(
        host.first(),
        Some(&PlaceAllHostEvent::NetDaemonProcessAll { group_index: 0 })
    );
    let emitted: Vec<_> = host
        .iter()
        .filter_map(|event| match event {
            PlaceAllHostEvent::AddBushDoober { placement } => Some(*placement),
            _ => None,
        })
        .collect();
    let bush = preview.bush_fringe.expect("bush pass must be present");
    assert_eq!(emitted, bush.placements);
    assert_eq!(emitted.len(), 2);
    assert_eq!(host.len(), 3);
    assert_ne!(
        bush.rng_state_after,
        preview.placement_preparation.rng_state_after
    );

    // The missing rock occupancy registry keeps deterministic owned state
    // transactional even though ordered host calls were surfaced.
    assert_eq!(random.state(), before_random);
    assert_eq!(mountains, before_mountains);
    assert_eq!(world.wdata, before_world.wdata);
    assert_eq!(groups, before_groups);
}
