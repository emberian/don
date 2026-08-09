// SPDX-License-Identifier: GPL-3.0-or-later

//! Mutation-sensitive coverage for the exact post-drop continuation of
//! `TerrainGroup::place_region_group` (`0x006a2f60`).

use don_sim::rng::Random;
use don_sim::systems::map_terrain::{land, tflag, wflag, World};
use don_sim::systems::regions::Regions;
use don_sim::systems::terrain_drop_tile::{DropTileExternalRequest, DropTileExternalResolution};
use don_sim::systems::terrain_groups::TerrainGroup;
use don_sim::systems::terrain_region_continuation::{
    PlaceRegionGroupOutcome, RegionGrowthRejection,
};
use don_sim::systems::terrain_region_placement::{PlaceRegionGroupCall, RegionHelpingState};

fn fertile_world(xs: i32, ys: i32) -> World {
    let mut world = World::init_default_rules(xs, ys);
    for cell in &mut world.wdata {
        cell.land = land::FERTILE;
    }
    world
}

fn region(regions: &mut Regions, id: usize, coords: &[(i32, i32)]) {
    regions.list[id].coords.items = coords.to_vec();
    regions.list[id].coords.capacity = coords.len() as i32;
}

fn call(region_id: usize, target_tiles: i32, oil_deposits: i32) -> PlaceRegionGroupCall {
    PlaceRegionGroupCall {
        target_tiles,
        region_id,
        land_subtype: 12,
        oil_deposits,
        place_players: 0,
        group_index: 3,
    }
}

fn group(group_type: i32) -> TerrainGroup {
    TerrainGroup {
        group_type,
        ..TerrainGroup::default()
    }
}

fn oil_request(x: i32, y: i32, enabled: bool) -> DropTileExternalRequest {
    DropTileExternalRequest::OilGoodMutation {
        world_x: x,
        world_y: y,
        enabled,
        good_type: 5,
        coord_x: x * 0x300 + 0x180,
        coord_y: y * 0x300 + 0x180,
    }
}

#[test]
fn failed_drop_resumes_the_same_lfsr_cycle_without_another_cursor_draw() {
    let mut world = fertile_world(8, 8);
    // Seed 1 selects cursor 2. Its cell survives the entry filters but the NW
    // full-water cell rejects `available_rough_tile`'s coast ring.
    world.wdata_mut(4, 4).land = land::OCEAN;
    let mut regions = Regions::default();
    region(&mut regions, 2, &[(2, 2), (5, 5)]);
    let mut terrain_group = group(6);
    terrain_group.coast_space = 2;
    let mut random = Random::new(1);

    let receipt = terrain_group
        .apply_place_region_group(&mut world, &regions, &mut random, call(2, 1, 0), None, &[])
        .unwrap();

    assert_eq!(receipt.prefix.initial_cursor, 2);
    assert_eq!(receipt.prefix.region_cursor_draws, 1);
    assert_eq!(receipt.prefix.attempts.len(), 2);
    assert_eq!(receipt.prefix.attempts[0].cursor, 2);
    assert_eq!(receipt.prefix.attempts[1].cursor, 1);
    assert_eq!(receipt.drops.len(), 2);
    assert!(!receipt.drops[0].placed);
    assert!(receipt.drops[1].placed);
    assert_eq!(terrain_group.tiles.items, [(2, 2)]);
    assert_eq!(receipt.outcome, PlaceRegionGroupOutcome::Returned(1));
}

#[test]
fn first_success_updates_all_helping_scores_and_strict_maximum_player() {
    let mut world = fertile_world(9, 9);
    world.start_x.items.extend([1, 8]);
    world.start_y.items.extend([2, 8]);
    let mut regions = Regions::default();
    region(&mut regions, 1, &[(2, 2)]);
    let helping = RegionHelpingState {
        num_players: 2,
        lowest_player: [0; 5],
        scores: [[0; 5]; 8],
    };
    let mut terrain_group = group(4);
    let mut random = Random::new(7);

    let receipt = terrain_group
        .apply_place_region_group(
            &mut world,
            &regions,
            &mut random,
            call(1, 1, 0),
            Some(helping),
            &[],
        )
        .unwrap();
    let helping = receipt.helping_after.unwrap();

    assert_eq!(helping.scores[0][0], 1);
    assert_eq!(helping.scores[1][0], 9);
    assert_eq!(helping.lowest_player[0], 1);
    assert_eq!(receipt.outcome, PlaceRegionGroupOutcome::Returned(1));
}

#[test]
fn growth_restarts_after_each_drop_and_consumes_both_orthog_draws() {
    let mut world = fertile_world(10, 10);
    let mut regions = Regions::default();
    region(&mut regions, 1, &[(4, 4)]);
    let mut terrain_group = group(6);
    let mut random = Random::new(0x1234_5678);

    let receipt = terrain_group
        .apply_place_region_group(&mut world, &regions, &mut random, call(1, 3, 0), None, &[])
        .unwrap();

    assert_eq!(receipt.drops.len(), 1);
    assert_eq!(receipt.growth_passes.len(), 2);
    assert_eq!(terrain_group.tiles.items.len(), 3);
    assert_eq!(receipt.growth_passes[0].selected_index, 0);
    assert_eq!(receipt.growth_passes[0].base_order, [0]);
    assert_eq!(receipt.growth_passes[0].orthogs.len(), 1);
    assert_eq!(receipt.growth_passes[1].orthogs.len(), 1);
    // len==1 skips the base draw; len==2 consumes it. Each pass then consumes
    // cardinal and ignored-diagonal rotations: 2 + 3 exact calls.
    assert_eq!(receipt.growth_passes[0].orthogs[0].cardinal_rotation, 2);
    assert_eq!(receipt.growth_passes[0].orthogs[0].diagonal_rotation, 1);
    assert_eq!(receipt.growth_passes[1].selected_index, 0);
    assert_eq!(receipt.growth_passes[1].orthogs[0].cardinal_rotation, 3);
    assert_eq!(receipt.growth_passes[1].orthogs[0].diagonal_rotation, 2);
    assert_eq!(receipt.rng_state_after, -2_065_822_053);
    assert_eq!(receipt.rng_state_after, random.state());
    assert_eq!(receipt.outcome, PlaceRegionGroupOutcome::Returned(1));
}

#[test]
fn growth_uses_the_retail_fixed_occupancy_probe_not_the_candidate_tile() {
    let mut world = fertile_world(9, 9);
    let mut regions = Regions::default();
    region(&mut regions, 1, &[(4, 4)]);
    // Growth's shipped index is tile_xs * region_id + first_success_x.
    let fixed = (world.tile_xs + 4) as usize;
    world.tdata[fixed] |= tflag::STARTED;
    let mut terrain_group = group(6);
    let mut random = Random::new(11);

    let boundary = terrain_group
        .apply_place_region_group(&mut world, &regions, &mut random, call(1, 2, 0), None, &[])
        .unwrap();
    assert!(boundary.growth_passes[0]
        .attempts
        .iter()
        .all(|attempt| attempt.rejection == Some(RegionGrowthRejection::FixedOccupiedProbe)));
    assert_eq!(
        boundary.outcome,
        PlaceRegionGroupOutcome::ExternalResolutionRequired {
            request: oil_request(4, 4, false)
        }
    );

    let external = DropTileExternalResolution::OilGoodsApplied {
        request: oil_request(4, 4, false),
    };
    let mut world = fertile_world(9, 9);
    world.tdata[fixed] |= tflag::STARTED;
    let mut terrain_group = group(6);
    let mut random = Random::new(11);
    let receipt = terrain_group
        .apply_place_region_group(
            &mut world,
            &regions,
            &mut random,
            call(1, 2, 0),
            None,
            &[external],
        )
        .unwrap();
    // Retail leaves the successful entry drop in `local_30` even though the
    // stalled group was cleared before the candidate cycle exhausted.
    assert_eq!(receipt.outcome, PlaceRegionGroupOutcome::Returned(1));
    assert_eq!(receipt.clear_passes.len(), 1);
    assert_eq!(receipt.clear_passes[0].tdata_unblock_calls, 16);
    assert!(terrain_group.tiles.items.is_empty());
    assert_eq!(world.wdata(4, 4).flags & (wflag::ROCKS | wflag::OIL), 0);
}

#[test]
fn type_six_oil_tail_is_ordered_after_target_and_needs_a_good_receipt() {
    let mut regions = Regions::default();
    region(&mut regions, 1, &[(4, 4)]);
    let request = oil_request(4, 4, true);

    let mut world = fertile_world(9, 9);
    let mut terrain_group = group(6);
    let mut random = Random::new(9);
    let boundary = terrain_group
        .apply_place_region_group(&mut world, &regions, &mut random, call(1, 1, 1), None, &[])
        .unwrap();
    assert_eq!(
        boundary.outcome,
        PlaceRegionGroupOutcome::ExternalResolutionRequired { request }
    );
    assert_eq!(world.wdata(4, 4).flags & wflag::OIL, 0);

    let mut world = fertile_world(9, 9);
    let mut terrain_group = group(6);
    let mut random = Random::new(9);
    let receipt = terrain_group
        .apply_place_region_group(
            &mut world,
            &regions,
            &mut random,
            call(1, 1, 1),
            None,
            &[DropTileExternalResolution::OilGoodsApplied { request }],
        )
        .unwrap();
    assert_eq!(receipt.outcome, PlaceRegionGroupOutcome::Returned(1));
    assert_eq!(receipt.oil_deposits.as_ref().unwrap().placed, 1);
    assert_ne!(world.wdata(4, 4).flags & wflag::OIL, 0);
}

#[test]
fn mountain_oil_and_cliff_types_take_their_immediate_success_return_arms() {
    for group_type in [5, 7, 8] {
        let mut world = fertile_world(9, 9);
        if group_type == 7 {
            for cell in &mut world.wdata {
                cell.land = land::OCEAN;
            }
        }
        let mut regions = Regions::default();
        region(&mut regions, 1, &[(4, 4)]);
        let mut terrain_group = group(group_type);
        let invocation = don_sim::systems::terrain_region_placement::RegionDropTileInvocation {
            world_x: 4,
            world_y: 4,
            group_type,
            group_radius: 1,
            land_subtype: 12,
            target_tiles: 20,
            oil_deposits: 7,
            group_index: 3,
        };
        let request = terrain_group
            .drop_tile_external_request(invocation)
            .unwrap();
        let external = match group_type {
            5 => DropTileExternalResolution::Mountains { request, liberr: 0 },
            7 => DropTileExternalResolution::OilGoodsApplied { request },
            8 => DropTileExternalResolution::Cliffs {
                request,
                return_value: 1,
                rng_draws: 0,
            },
            _ => unreachable!(),
        };
        let mut random = Random::new(13);
        let receipt = terrain_group
            .apply_place_region_group(
                &mut world,
                &regions,
                &mut random,
                call(1, 20, 7),
                None,
                &[external],
            )
            .unwrap();
        assert_eq!(receipt.outcome, PlaceRegionGroupOutcome::Returned(1));
        assert!(receipt.growth_passes.is_empty());
        assert!(receipt.clear_passes.is_empty());
        assert!(receipt.oil_deposits.is_none());
    }
}
