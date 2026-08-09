// SPDX-License-Identifier: GPL-3.0-or-later

//! Exact pattern-0 / `place_player_group` entry transaction.

use don_sim::rng::Random;
use don_sim::systems::map_terrain::{land, World};
use don_sim::systems::mountains::{MountainRangeEntry, MountainRangeList, Mountains};
use don_sim::systems::terrain_groups::{
    PlaceAllError, PlaceAllHostEvent, TerrainGroup, TerrainGroups, TerrainPlacementBoundary,
};
use don_sim::systems::terrain_player_group::{
    PlacePlayerGroupCall, PlacePlayerGroupOutcome, PlayerCandidateRejection,
    PlayerGroupExternalRequest, PlayerGroupExternalResolution,
};

fn world_with_starts(starts: &[(i32, i32)]) -> World {
    let mut world = World::init_default_rules(15, 15);
    for cell in &mut world.wdata {
        cell.land = land::FERTILE;
        cell.region = 1;
    }
    for &(x, y) in starts {
        world.start_x.items.push(x);
        world.start_y.items.push(y);
    }
    world
}

fn group(group_type: i32) -> TerrainGroup {
    TerrainGroup {
        group_type,
        start_min: 0,
        start_max: 1,
        ..TerrainGroup::default()
    }
}

fn call(group_index: usize, subtype: i32) -> PlacePlayerGroupCall {
    PlacePlayerGroupCall {
        target_tiles: 1,
        player_index: 0,
        land_subtype: subtype,
        oil_deposits: 0,
        group_index,
        strict_type_four: false,
    }
}

#[test]
fn ring_cursor_starts_after_the_selected_index_and_surfaces_exact_mountain_abi() {
    let mut world = world_with_starts(&[(7, 7)]);
    let mut terrain_group = group(5);
    let mut random = Random::new(1);
    let mut xs = Vec::new();
    let mut ys = Vec::new();

    let receipt = terrain_group
        .apply_place_player_group_prefix(
            &mut world,
            &mut random,
            call(3, 42),
            &mut xs,
            &mut ys,
            &[],
        )
        .unwrap();

    assert_eq!(receipt.selected_circle_index, Some(4));
    assert_eq!(receipt.first_circle_index, Some(5));
    assert_eq!(receipt.circle_selection_draws, 1);
    assert_eq!(
        (receipt.attempts[0].world_x, receipt.attempts[0].world_y),
        (7, 8)
    );
    assert_eq!(
        receipt.outcome,
        PlacePlayerGroupOutcome::ExternalResolutionRequired {
            request: PlayerGroupExternalRequest::MountainsAddMountain {
                template: 42,
                world_x: 7,
                world_y: 8,
                pattern: 5,
                mountain_space: 0,
                forest_space: 0,
                rock_space: 0,
                coast_space: 0,
                start_min: 0,
            }
        }
    );
    assert_eq!(random.state(), 0x3c88_596cu32 as i32);
}

#[test]
fn nearest_start_voronoi_filter_rejects_before_object_effect() {
    let mut world = world_with_starts(&[(7, 7), (7, 8)]);
    let mut terrain_group = group(5);
    let mut random = Random::new(1);
    let mut xs = Vec::new();
    let mut ys = Vec::new();

    let receipt = terrain_group
        .apply_place_player_group_prefix(&mut world, &mut random, call(0, 9), &mut xs, &mut ys, &[])
        .unwrap();

    assert_eq!(
        receipt.attempts[0].rejection,
        Some(PlayerCandidateRejection::CloserToOtherPlayer {
            player: 1,
            distance: 0,
        })
    );
    assert!(receipt.attempts.len() > 1);
    assert!(matches!(
        receipt.outcome,
        PlacePlayerGroupOutcome::ExternalResolutionRequired { .. }
    ));
}

#[test]
fn mountain_resolution_zero_returns_one_and_nonzero_resumes_the_ring() {
    let base_world = world_with_starts(&[(7, 7)]);
    let base_group = group(5);
    let mut probe_world = base_world.clone();
    let mut probe_group = base_group.clone();
    let mut probe_random = Random::new(1);
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    let probe = probe_group
        .apply_place_player_group_prefix(
            &mut probe_world,
            &mut probe_random,
            call(0, 42),
            &mut xs,
            &mut ys,
            &[],
        )
        .unwrap();
    let PlacePlayerGroupOutcome::ExternalResolutionRequired { request } = probe.outcome else {
        panic!("expected mountain request")
    };

    let mut world = base_world;
    let mut terrain_group = base_group;
    let mut random = Random::new(1);
    let receipt = terrain_group
        .apply_place_player_group_prefix(
            &mut world,
            &mut random,
            call(0, 42),
            &mut Vec::new(),
            &mut Vec::new(),
            &[PlayerGroupExternalResolution::Mountains { request, liberr: 0 }],
        )
        .unwrap();
    assert_eq!(receipt.external_resolutions_consumed, 1);
    assert_eq!(receipt.outcome, PlacePlayerGroupOutcome::Returned(1));
}

#[test]
fn type_six_executes_first_drop_before_the_exact_growth_boundary() {
    let mut world = world_with_starts(&[(7, 7)]);
    let mut terrain_group = group(6);
    let mut random = Random::new(1);
    let mut xs = Vec::new();
    let mut ys = Vec::new();

    let receipt = terrain_group
        .apply_place_player_group_prefix(&mut world, &mut random, call(4, 1), &mut xs, &mut ys, &[])
        .unwrap();

    let PlacePlayerGroupOutcome::GrowthKernel {
        invocation,
        first_drop,
    } = receipt.outcome
    else {
        panic!("expected growth boundary")
    };
    assert_eq!(invocation.group_index, 4);
    assert!(first_drop.placed);
    assert_eq!(terrain_group.tiles.items, [(7, 8)]);
    assert_ne!(world.wdata(7, 8).flags, 0);
}

#[test]
fn cliff_subtype_five_skips_verify_and_requests_position_with_retail_arguments() {
    let mut world = world_with_starts(&[(7, 7)]);
    let mut terrain_group = group(8);
    terrain_group.cliff_face = 13;
    let mut random = Random::new(1);

    let receipt = terrain_group
        .apply_place_player_group_prefix(
            &mut world,
            &mut random,
            call(0, 5),
            &mut Vec::new(),
            &mut Vec::new(),
            &[],
        )
        .unwrap();

    assert_eq!(
        receipt.outcome,
        PlacePlayerGroupOutcome::ExternalResolutionRequired {
            request: PlayerGroupExternalRequest::CliffsPositionCliff {
                world_x: 7,
                world_y: 8,
                cliff_type: 4,
                start_x: 7,
                start_y: 7,
                defensive: true,
                cliff_face: 13,
                anchor_y: 7,
            }
        }
    );
}

#[test]
fn pattern_zero_type_four_retries_without_strict_forest_probe_or_second_pump() {
    let mut world = world_with_starts(&[(7, 7)]);
    world.wdata_mut(7, 7).flags = don_sim::systems::map_terrain::wflag::FOREST;
    let terrain_group = TerrainGroup {
        group_type: 4,
        chance: 100,
        min_clumps: 1,
        max_clumps: 1,
        pattern: 0,
        min_size: 1,
        max_size: 1,
        start_min: 0,
        start_max: 1,
        ..TerrainGroup::default()
    };
    let mut groups = TerrainGroups {
        groups: vec![terrain_group],
        ..TerrainGroups::default()
    };
    let mut mountains = Mountains::default();
    let mut random = Random::new(17);
    let mut host = Vec::new();

    let error = groups
        .place_all_with_player_group_inputs(
            &mut world,
            &mut random,
            &mut mountains,
            0,
            1,
            &[],
            |event| host.push(event),
        )
        .unwrap_err();
    let PlaceAllError::GameplayPlacementUnavailable { preview, boundary } = error else {
        panic!("unexpected error: {error:?}")
    };
    assert!(matches!(
        boundary,
        TerrainPlacementBoundary::PlayerGroupPatternComplete { group_index: 0 }
    ));
    let calls = preview.player_group_prefix.unwrap();
    assert_eq!(calls.len(), 2);
    assert!(calls[0].attempts.iter().all(|attempt| matches!(
        attempt.rejection,
        Some(PlayerCandidateRejection::TypeFourForestRing { .. })
    )));
    assert!(matches!(
        calls[1].outcome,
        PlacePlayerGroupOutcome::Returned(_)
    ));
    assert!(calls[1].growth.is_some());
    assert_eq!(preview.player_group_host_events.len(), 1);
    assert_eq!(preview.player_group_formation_x, [0, 0]);
    assert_eq!(preview.player_group_formation_y, [0, -1]);
}

#[test]
fn place_all_composes_player_entry_pump_and_keeps_growth_preview_transactional() {
    let mut world = world_with_starts(&[(7, 7)]);
    let terrain_group = TerrainGroup {
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
    };
    let mut groups = TerrainGroups {
        groups: vec![terrain_group],
        ..TerrainGroups::default()
    };
    let mut mountains = Mountains {
        small_ranges: MountainRangeList::new(vec![MountainRangeEntry::new(9, 0)]),
        ..Mountains::default()
    };
    let mut random = Random::new(0x1234_5678);
    let before_world = world.clone();
    let before_groups = groups.clone();
    let before_mountains = mountains.clone();
    let before_random = random;
    let mut host = Vec::new();

    let error = groups
        .place_all_with_player_group_inputs(
            &mut world,
            &mut random,
            &mut mountains,
            0,
            1,
            &[],
            |event| host.push(event),
        )
        .unwrap_err();
    let PlaceAllError::GameplayPlacementUnavailable { preview, boundary } = error else {
        panic!("unexpected error: {error:?}")
    };

    assert_eq!(
        boundary,
        TerrainPlacementBoundary::PlayerGroupPatternComplete { group_index: 0 }
    );
    assert_eq!(
        host.last(),
        Some(&PlaceAllHostEvent::NetDaemonProcessAllPlayer {
            group_index: 0,
            clump_index: 0,
            player_index: 0,
        })
    );
    assert_eq!(preview.player_group_prefix.as_ref().unwrap().len(), 1);
    assert!(preview.player_group_prefix.as_ref().unwrap()[0]
        .growth
        .is_some());
    assert_eq!(world.wdata, before_world.wdata);
    assert_eq!(world.tdata, before_world.tdata);
    assert_eq!(groups, before_groups);
    assert_eq!(mountains, before_mountains);
    assert_eq!(random, before_random);
}
