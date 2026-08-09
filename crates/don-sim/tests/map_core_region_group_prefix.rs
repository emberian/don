// SPDX-License-Identifier: GPL-3.0-or-later

//! Mutation-sensitive coverage for `TerrainGroup::place_region_group`'s exact
//! deterministic entry search (`0x006a2f60`).

use don_sim::rng::Random;
use don_sim::systems::map_terrain::{land, wflag, World};
use don_sim::systems::mountains::Mountains;
use don_sim::systems::regions::Regions;
use don_sim::systems::terrain_groups::{
    PlaceAllError, ResolvedRegionGroupPlacement, TerrainGroup, TerrainGroups,
    TerrainPlacementBoundary,
};
use don_sim::systems::terrain_region_placement::{
    PlaceRegionGroupCall, PlaceRegionGroupPrefixError, PlaceRegionGroupPrefixOutcome,
    RegionCandidateRejection, RegionDropTileInvocation, RegionHelpingState,
};

fn call(region_id: usize) -> PlaceRegionGroupCall {
    PlaceRegionGroupCall {
        target_tiles: 9,
        region_id,
        land_subtype: 13,
        oil_deposits: 2,
        place_players: 0,
        group_index: 4,
    }
}

fn group(group_type: i32) -> TerrainGroup {
    TerrainGroup {
        group_type,
        ..TerrainGroup::default()
    }
}

fn set_region(regions: &mut Regions, region_id: usize, coords: &[(i32, i32)]) {
    regions.list[region_id].coords.items = coords.to_vec();
    regions.list[region_id].coords.capacity = coords.len() as i32;
}

#[test]
fn singleton_region_reaches_the_exact_consumed_drop_tile_call_without_a_draw() {
    let mut world = World::init_default_rules(5, 5);
    world.wdata_mut(2, 2).land = land::FERTILE;
    let mut regions = Regions::default();
    set_region(&mut regions, 3, &[(2, 2)]);
    let mut terrain_group = group(6);
    terrain_group.tiles.items = vec![(8, 8), (9, 9)];
    let mut random = Random::new(0x1234_5678);
    let before = random;

    let receipt = terrain_group
        .plan_place_region_group_prefix(&world, &regions, &mut random, call(3), None)
        .unwrap();

    assert_eq!(receipt.cleared_tiles, 2);
    assert_eq!(receipt.initial_cursor, 1);
    assert_eq!(receipt.region_cursor_draws, 0);
    assert_eq!(receipt.attempts.len(), 1);
    assert_eq!(receipt.attempts[0].coord_index, 0);
    assert_eq!(receipt.attempts[0].rejection, None);
    assert_eq!(random, before);
    assert_eq!(terrain_group.tiles.items, vec![(8, 8), (9, 9)]);
    assert_eq!(
        receipt.outcome,
        PlaceRegionGroupPrefixOutcome::DropTile(RegionDropTileInvocation {
            world_x: 2,
            world_y: 2,
            group_type: 6,
            group_radius: 3,
            land_subtype: 13,
            target_tiles: 9,
            oil_deposits: 2,
            group_index: 4,
        })
    );
}

#[test]
fn region_cursor_draw_and_lfsr_candidate_order_are_byte_pinned() {
    let mut world = World::init_default_rules(6, 6);
    let coords = [(1, 1), (2, 1), (3, 1), (1, 2), (2, 2), (3, 2)];
    for &(x, y) in &coords {
        let cell = world.wdata_mut(x, y);
        cell.land = land::FERTILE;
        cell.flags = wflag::FOREST;
    }
    let mut regions = Regions::default();
    set_region(&mut regions, 7, &coords);
    let mut random = Random::new(1);

    let receipt = group(6)
        .plan_place_region_group_prefix(&world, &regions, &mut random, call(7), None)
        .unwrap();

    assert_eq!(receipt.initial_cursor, 2);
    assert_eq!(receipt.region_cursor_draws, 1);
    assert_eq!(
        receipt
            .attempts
            .iter()
            .map(|attempt| attempt.cursor)
            .collect::<Vec<_>>(),
        [2, 1, 5, 6, 3, 4]
    );
    assert_eq!(
        receipt
            .attempts
            .iter()
            .map(|attempt| attempt.coord_index)
            .collect::<Vec<_>>(),
        [1, 0, 4, 5, 2, 3]
    );
    assert!(receipt.attempts.iter().all(|attempt| {
        attempt.rejection
            == Some(RegionCandidateRejection::ForbiddenWorldFlags {
                flags: wflag::FOREST,
            })
    }));
    assert_eq!(receipt.outcome, PlaceRegionGroupPrefixOutcome::Exhausted);
    assert_eq!(random.state(), 0x3c88_596c);
}

#[test]
fn mutating_the_first_candidate_reassigns_the_same_rng_stream_to_the_next_cursor() {
    let mut open = World::init_default_rules(6, 6);
    open.wdata_mut(2, 2).land = land::FERTILE;
    open.wdata_mut(3, 3).land = land::FERTILE;
    let mut blocked = open.clone();
    blocked.wdata_mut(3, 3).flags |= wflag::FOREST;
    let mut regions = Regions::default();
    set_region(&mut regions, 11, &[(2, 2), (3, 3)]);
    let mut open_rng = Random::new(1);
    let mut blocked_rng = open_rng;

    let open_receipt = group(6)
        .plan_place_region_group_prefix(&open, &regions, &mut open_rng, call(11), None)
        .unwrap();
    let blocked_receipt = group(6)
        .plan_place_region_group_prefix(&blocked, &regions, &mut blocked_rng, call(11), None)
        .unwrap();

    assert_eq!(open_receipt.attempts[0].cursor, 2);
    assert_eq!(open_receipt.attempts[0].rejection, None);
    assert_eq!(blocked_receipt.attempts[0].cursor, 2);
    assert_eq!(blocked_receipt.attempts[1].cursor, 1);
    assert_eq!(
        blocked_receipt.attempts[0].rejection,
        Some(RegionCandidateRejection::ForbiddenWorldFlags {
            flags: wflag::FOREST,
        })
    );
    assert_eq!(blocked_receipt.attempts[1].rejection, None);
    assert_eq!(open_rng, blocked_rng);
}

#[test]
fn player_start_distance_rejects_before_forbidden_world_flags() {
    let mut world = World::init_default_rules(5, 5);
    let cell = world.wdata_mut(2, 2);
    cell.land = land::FERTILE;
    cell.flags = wflag::MOUNTAINS;
    world.start_x.items.push(2);
    world.start_y.items.push(2);
    let mut regions = Regions::default();
    set_region(&mut regions, 1, &[(2, 2)]);
    let mut terrain_group = group(6);
    terrain_group.start_min = 1;
    let mut random = Random::new(3);

    let receipt = terrain_group
        .plan_place_region_group_prefix(&world, &regions, &mut random, call(1), None)
        .unwrap();

    assert_eq!(
        receipt.attempts[0].rejection,
        Some(RegionCandidateRejection::PlayerStartMinimum {
            player: 0,
            distance: 0,
        })
    );
}

#[test]
fn type_seven_ring_distinguishes_plain_water_oil_and_inner_land() {
    let mut ocean = World::init_default_rules(7, 7);
    let mut regions = Regions::default();
    set_region(&mut regions, 65, &[(3, 3)]);
    let terrain_group = group(7);

    let mut ocean_rng = Random::new(4);
    let ocean_receipt = terrain_group
        .plan_place_region_group_prefix(&ocean, &regions, &mut ocean_rng, call(65), None)
        .unwrap();
    assert!(matches!(
        ocean_receipt.outcome,
        PlaceRegionGroupPrefixOutcome::DropTile(_)
    ));

    ocean.wdata_mut(2, 2).flags |= wflag::OIL;
    let mut oil_rng = Random::new(4);
    let oil_receipt = terrain_group
        .plan_place_region_group_prefix(&ocean, &regions, &mut oil_rng, call(65), None)
        .unwrap();
    assert_eq!(
        oil_receipt.attempts[0].rejection,
        Some(RegionCandidateRejection::TypeSevenWaterRing { offset_index: 1 })
    );

    let cell = ocean.wdata_mut(2, 2);
    cell.flags &= !wflag::OIL;
    cell.land = land::FERTILE;
    let mut land_rng = Random::new(4);
    let land_receipt = terrain_group
        .plan_place_region_group_prefix(&ocean, &regions, &mut land_rng, call(65), None)
        .unwrap();
    assert_eq!(
        land_receipt.attempts[0].rejection,
        Some(RegionCandidateRejection::TypeSevenWaterRing { offset_index: 1 })
    );
}

#[test]
fn rectangular_maps_preserve_the_shipped_xs_for_last_y_boundary_bug() {
    let mut world = World::init_default_rules(5, 8);
    world.wdata_mut(2, 4).land = land::FERTILE;
    let mut regions = Regions::default();
    set_region(&mut regions, 2, &[(2, 4)]);
    let mut random = Random::new(5);

    let receipt = group(6)
        .plan_place_region_group_prefix(&world, &regions, &mut random, call(2), None)
        .unwrap();

    assert_eq!(
        receipt.attempts[0].rejection,
        Some(RegionCandidateRejection::HardBoundary)
    );
}

#[test]
fn type_four_checks_center_then_the_eight_shipped_neighbor_offsets() {
    let mut world = World::init_default_rules(5, 5);
    world.wdata_mut(2, 2).land = land::FERTILE;
    world.wdata_mut(3, 2).flags |= wflag::IMPASSABLE_X;
    let mut regions = Regions::default();
    set_region(&mut regions, 4, &[(2, 2)]);
    let mut random = Random::new(6);

    let receipt = group(4)
        .plan_place_region_group_prefix(&world, &regions, &mut random, call(4), None)
        .unwrap();

    assert_eq!(
        receipt.attempts[0].rejection,
        Some(RegionCandidateRejection::TypeFourImpassableNeighbor { offset_index: 4 })
    );
}

#[test]
fn helping_mode_keeps_the_first_player_on_equal_distance() {
    let mut world = World::init_default_rules(7, 7);
    world.wdata_mut(3, 3).land = land::FERTILE;
    world.start_x.items.extend([2, 4]);
    world.start_y.items.extend([3, 3]);
    let mut regions = Regions::default();
    set_region(&mut regions, 5, &[(3, 3)]);
    let mut random = Random::new(7);

    let receipt = group(6)
        .plan_place_region_group_prefix(
            &world,
            &regions,
            &mut random,
            call(5),
            Some(RegionHelpingState {
                is_helping: true,
                num_players: 2,
                lowest_player: [0, 0, 1, 0, 0],
                scores: [[0; 5]; 8],
            }),
        )
        .unwrap();

    assert_eq!(
        receipt.attempts[0].rejection,
        Some(RegionCandidateRejection::HelpingPlayer {
            nearest: 0,
            required: 1,
        })
    );

    let mut random = Random::new(7);
    let receipt = group(6)
        .plan_place_region_group_prefix(
            &world,
            &regions,
            &mut random,
            call(5),
            Some(RegionHelpingState {
                is_helping: false,
                num_players: 2,
                lowest_player: [0, 0, 1, 0, 0],
                scores: [[0; 5]; 8],
            }),
        )
        .unwrap();
    assert_eq!(receipt.attempts[0].rejection, None);
}

#[test]
fn malformed_region_fails_before_the_only_possible_rng_draw() {
    let world = World::init_default_rules(5, 5);
    let mut regions = Regions::default();
    set_region(&mut regions, 9, &[(2, 2), (5, 2)]);
    let mut random = Random::new(0x7654_3210);
    let before = random;

    let error = group(6)
        .plan_place_region_group_prefix(&world, &regions, &mut random, call(9), None)
        .unwrap_err();

    assert_eq!(
        error,
        PlaceRegionGroupPrefixError::RegionCoordinateOutOfBounds {
            region_id: 9,
            coord_index: 1,
            world_x: 5,
            world_y: 2,
        }
    );
    assert_eq!(random, before);
}

#[test]
fn place_all_composes_the_resolved_call_and_stays_transactional_at_drop_tile() {
    let mut world = World::init_default_rules(5, 5);
    world.wdata_mut(2, 2).land = land::FERTILE;
    let mut regions = Regions::default();
    set_region(&mut regions, 3, &[(2, 2)]);
    let terrain_group = TerrainGroup {
        group_type: 6,
        chance: 100,
        min_clumps: 1,
        max_clumps: 1,
        pattern: 1,
        min_size: 1,
        max_size: 1,
        min_oil: 0,
        max_oil: 0,
        ..TerrainGroup::default()
    };
    let mut groups = TerrainGroups {
        groups: vec![terrain_group],
        ..TerrainGroups::default()
    };
    let mut mountains = Mountains::default();
    let mut random = Random::new(0x1111_2222);
    let before_world = world.clone();
    let before_groups = groups.clone();
    let before_mountains = mountains.clone();
    let before_random = random;
    let mut host = Vec::new();

    let error = groups
        .place_all_with_resolved_region_group(
            &mut world,
            &regions,
            &mut random,
            &mut mountains,
            0,
            0,
            ResolvedRegionGroupPlacement {
                group_index: 0,
                clump_index: 0,
                region_id: 3,
                land_subtype: 13,
                rng_state_at_call: 0x2222_3333,
                helping: None,
                drop_tile_externals: Vec::new(),
            },
            |event| host.push(event),
        )
        .unwrap_err();

    let PlaceAllError::GameplayPlacementUnavailable { preview, boundary } = error else {
        panic!("unexpected place_all result: {error:?}");
    };
    assert_eq!(
        boundary,
        TerrainPlacementBoundary::RegionGroupReturnControl {
            group_index: 0,
            return_value: 1,
        }
    );
    let prefix = preview.region_group_prefix.unwrap();
    assert_eq!(prefix.region_cursor_draws, 0);
    assert_eq!(prefix.rng_state_after, 0x2222_3333);
    let drop = preview.region_group_drop.unwrap();
    assert!(drop.placed);
    assert_eq!(drop.tiles_len_after, 1);
    assert_eq!(host.len(), 1);
    assert_eq!(world.wdata, before_world.wdata);
    assert_eq!(world.tdata, before_world.tdata);
    assert_eq!(world.start_city_locs, before_world.start_city_locs);
    assert_eq!(groups, before_groups);
    assert_eq!(mountains, before_mountains);
    assert_eq!(random, before_random);
}
