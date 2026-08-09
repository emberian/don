// SPDX-License-Identifier: GPL-3.0-or-later

//! Mutation-sensitive tests for the first `TerrainGroups::add_doobers` pass.

use don_sim::rng::Random;
use don_sim::systems::map_terrain::{wflag, World};
use don_sim::systems::mountains::Mountains;
use don_sim::systems::terrain_doobers::{
    has_doobers, plan_bush_fringe, plan_bush_fringe_with_host, plan_mountain_rock_fringe,
    plan_mountain_rock_fringe_with_host, BushDooberPlacement, BushFringeError, DooberLocation,
    DooberTilesetRules, ForestFringeEdge, MountainRockDooberPlacement, MountainRockFringeError,
    MountainRockNeighbor,
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
fn has_doobers_shipped_y_bug_makes_registry_mutations_invariant() {
    let exact_raw_y_match = DooberLocation {
        x: 2.0 * 768.0,
        y: 7.0,
    };
    let normal_world_position = DooberLocation {
        x: 2.0 * 768.0,
        y: 7.0 * 768.0,
    };

    assert!(!has_doobers(&[], 2, 7));
    assert!(!has_doobers(&[exact_raw_y_match], 2, 7));
    assert!(!has_doobers(
        &[exact_raw_y_match, normal_world_position],
        2,
        7
    ));
}

fn mountain_rules() -> DooberTilesetRules {
    DooberTilesetRules {
        mountain_rock_clump_prob: 100,
        mountain_rock_spacing: 0,
        mountain_rock_min: 1,
        mountain_rock_max: 1,
        ..DooberTilesetRules::default()
    }
}

#[test]
fn mountain_fringe_uses_west_first_and_exact_rng_coordinates() {
    let mut world = World::init_default_rules(4, 4);
    world.wdata_mut(2, 2).land = 0;
    world.wdata_mut(1, 2).flags |= wflag::MOUNTAINS;
    world.wdata_mut(2, 1).flags |= wflag::MOUNTAINS;
    let mut random = Random::new(0x1234_5678);
    let mut host = Vec::new();

    let receipt =
        plan_mountain_rock_fringe_with_host(&world, mountain_rules(), &mut random, |placement| {
            host.push(placement)
        })
        .unwrap();

    assert_eq!(receipt.occupancy_queries, 1);
    assert_eq!(receipt.chance_draws, 1);
    assert_eq!(receipt.tile_offset_draws, 1);
    assert_eq!(
        receipt.placements,
        vec![MountainRockDooberPlacement {
            world_x: 2,
            world_y: 2,
            mountain_neighbor: MountainRockNeighbor::West,
            x: 1931,
            y: 1972,
        }]
    );
    assert_eq!(host, receipt.placements);
    assert_eq!(receipt.rng_state_after, random.state());
}

#[test]
fn west_neighbor_mutation_reassigns_the_same_transaction_to_north() {
    let mut west_first = World::init_default_rules(4, 4);
    west_first.wdata_mut(2, 2).land = 0;
    west_first.wdata_mut(1, 2).flags |= wflag::MOUNTAINS;
    west_first.wdata_mut(2, 1).flags |= wflag::MOUNTAINS;
    let mut north_only = west_first.clone();
    north_only.wdata_mut(1, 2).flags &= !wflag::MOUNTAINS;
    let mut west_rng = Random::new(0x1357_2468);
    let mut north_rng = west_rng;

    let west = plan_mountain_rock_fringe(&west_first, mountain_rules(), &mut west_rng).unwrap();
    let north = plan_mountain_rock_fringe(&north_only, mountain_rules(), &mut north_rng).unwrap();

    assert_eq!(
        west.placements[0].mountain_neighbor,
        MountainRockNeighbor::West
    );
    assert_eq!(
        north.placements[0].mountain_neighbor,
        MountainRockNeighbor::North
    );
    assert_eq!(west.placements[0].x, north.placements[0].x);
    assert_eq!(west.placements[0].y, north.placements[0].y);
    assert_eq!(west.rng_state_after, north.rng_state_after);
}

#[test]
fn waterhalf_and_river_tests_preserve_native_query_and_draw_order() {
    let mut waterhalf = World::init_default_rules(4, 4);
    waterhalf.wdata_mut(2, 2).flags |= wflag::WATERHALF | wflag::HAS_RIVER;
    waterhalf.wdata_mut(1, 2).flags |= wflag::MOUNTAINS;
    let mut plain_water = waterhalf.clone();
    plain_water.wdata_mut(2, 2).flags &= !wflag::WATERHALF;
    let mut waterhalf_rng = Random::new(91);
    let mut plain_rng = waterhalf_rng;
    let before = waterhalf_rng.state();

    let river =
        plan_mountain_rock_fringe(&waterhalf, mountain_rules(), &mut waterhalf_rng).unwrap();
    let skipped =
        plan_mountain_rock_fringe(&plain_water, mountain_rules(), &mut plain_rng).unwrap();

    assert_eq!(river.occupancy_queries, 1);
    assert_eq!(river.chance_draws, 0);
    assert!(river.placements.is_empty());
    assert_eq!(skipped.occupancy_queries, 0);
    assert_eq!(waterhalf_rng.state(), before);
    assert_eq!(plain_rng.state(), before);
}

#[test]
fn unsupported_mountain_count_fails_before_rng_or_host() {
    let mut world = World::init_default_rules(4, 4);
    world.wdata_mut(2, 2).land = 0;
    world.wdata_mut(1, 2).flags |= wflag::MOUNTAINS;
    let mut unsupported = mountain_rules();
    unsupported.mountain_rock_max = 6;
    let mut random = Random::new(7);
    let before = random.state();
    let mut host_calls = 0;

    assert_eq!(
        plan_mountain_rock_fringe_with_host(&world, unsupported, &mut random, |_| {
            host_calls += 1
        }),
        Err(MountainRockFringeError::UnsupportedMountainRockCount {
            maximum_positive_count: 6,
            supported: 5,
        })
    );
    assert_eq!(random.state(), before);
    assert_eq!(host_calls, 0);
}

#[test]
fn place_all_preflights_both_passes_before_any_host_event() {
    let mut world = World::init_default_rules(4, 4);
    world.wdata_mut(1, 1).flags |= wflag::FOREST;
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
    let mut invalid = rules();
    invalid.mountain_rock_max = 6;
    let mut mountains = Mountains::default();
    let mut random = Random::new(13);
    let before = random.state();
    let mut host_calls = 0;

    assert_eq!(
        groups.place_all_with_doober_rules(
            &mut world,
            &mut random,
            &mut mountains,
            0,
            0,
            invalid,
            |_| host_calls += 1,
        ),
        Err(PlaceAllError::InvalidMountainRockFringe(
            MountainRockFringeError::UnsupportedMountainRockCount {
                maximum_positive_count: 6,
                supported: 5,
            }
        ))
    );
    assert_eq!(random.state(), before);
    assert_eq!(host_calls, 0);
}

#[test]
fn place_all_advances_from_group_pump_through_both_doober_passes() {
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
        TerrainPlacementBoundary::TreeifyMountainsGameModeAndProbability
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
    let mountain = preview
        .mountain_rock_fringe
        .expect("mountain-rock pass must be present");
    assert_eq!(emitted, bush.placements);
    assert_eq!(emitted.len(), 2);
    assert_eq!(host.len(), 3);
    assert!(mountain.placements.is_empty());
    assert_ne!(
        bush.rng_state_after,
        preview.placement_preparation.rng_state_after
    );

    // The missing post-doober stage keeps deterministic owned state transactional
    // even though ordered host calls were surfaced.
    assert_eq!(random.state(), before_random);
    assert_eq!(mountains, before_mountains);
    assert_eq!(world.wdata, before_world.wdata);
    assert_eq!(groups, before_groups);
}
