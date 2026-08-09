// SPDX-License-Identifier: GPL-3.0-or-later

//! Mutation-sensitive pins for `place_player_group` `0x006a4cf0` growth.

use don_sim::rng::Random;
use don_sim::systems::map_terrain::{land, wflag, World};
use don_sim::systems::mountains::Mountains;
use don_sim::systems::terrain_groups::{
    PlaceAllError, PlaceAllHostEvent, TerrainGroup, TerrainGroups, TerrainPlacementBoundary,
};
use don_sim::systems::terrain_player_group::{
    PlacePlayerGroupCall, PlayerGroupExternalRequest, PlayerGroupExternalResolution,
};
use don_sim::systems::terrain_player_growth::{
    PlayerGroupGrowthOutcome, PlayerGrowthDirectionKind,
};

fn world() -> World {
    let mut world = World::init_default_rules(17, 17);
    for cell in &mut world.wdata {
        cell.land = land::FERTILE;
        cell.region = 1;
    }
    world.start_x.items.push(8);
    world.start_y.items.push(8);
    world
}

fn call(target_tiles: i32, oil_deposits: i32) -> PlacePlayerGroupCall {
    PlacePlayerGroupCall {
        target_tiles,
        player_index: 0,
        land_subtype: -1,
        oil_deposits,
        group_index: 3,
        strict_type_four: false,
    }
}

#[test]
fn shipped_cardinal_order_and_two_draw_rotation_place_the_first_growth_tile() {
    let mut world = world();
    let mut group = TerrainGroup {
        group_type: 6,
        start_max: 64,
        ..TerrainGroup::default()
    };
    group.tiles.items.push((8, 8));
    group.tiles.capacity = 4;
    world.wdata_mut(8, 8).flags |= wflag::ROCKS;
    let mut random = Random::new(1);

    let receipt = group
        .apply_player_group_growth(
            &mut world,
            &mut random,
            call(2, 0),
            &mut Vec::new(),
            &mut Vec::new(),
            &[],
        )
        .unwrap();

    assert_eq!(receipt.outcome, PlayerGroupGrowthOutcome::Returned(1));
    assert_eq!(receipt.passes.len(), 1);
    let pass = &receipt.passes[0];
    assert_eq!(pass.selection_draws, 0);
    assert_eq!(pass.base_order, [0]);
    assert_eq!(pass.rotations[0].cardinal_rotation, 3);
    assert_eq!(pass.rotations[0].diagonal_rotation, 2);
    assert_eq!(pass.attempts.len(), 1);
    assert_eq!(
        pass.attempts[0].direction_kind,
        PlayerGrowthDirectionKind::Cardinal
    );
    assert_eq!(pass.attempts[0].direction_index, 3);
    assert_eq!((pass.attempts[0].world_x, pass.attempts[0].world_y), (7, 8));
    assert!(pass.attempts[0].drop.as_ref().unwrap().placed);
    assert_eq!(group.tiles.items, [(8, 8), (7, 8)]);
    assert_eq!(random.state(), 0x5e88_85db);
}

#[test]
fn type_four_keeps_the_growth_base_at_tile_zero_while_termination_cursor_cycles() {
    let mut world = world();
    world.start_city_locs.fill(0xff);
    let mut group = TerrainGroup {
        group_type: 4,
        start_max: 64,
        ..TerrainGroup::default()
    };
    group.tiles.items = vec![(5, 5), (8, 8), (11, 11)];
    group.tiles.capacity = 4;
    for &(x, y) in &group.tiles.items {
        world.wdata_mut(x, y).flags |= wflag::FOREST;
    }
    let mut random = Random::new(1);

    let receipt = group
        .apply_player_group_growth(
            &mut world,
            &mut random,
            call(4, 0),
            &mut vec![0, 3, 6],
            &mut vec![0, 3, 6],
            &[],
        )
        .unwrap();

    assert_eq!(receipt.passes.len(), 1);
    assert_eq!(receipt.passes[0].base_order, [0, 0, 0]);
    assert_eq!(receipt.passes[0].attempts.len(), 12);
    assert!(receipt.passes[0]
        .attempts
        .iter()
        .all(|attempt| attempt.base_index == 0));
    assert_eq!(
        receipt.outcome,
        PlayerGroupGrowthOutcome::ExternalResolutionRequired {
            request: PlayerGroupExternalRequest::OilGoodMutation {
                world_x: 5,
                world_y: 5,
                enabled: false,
                good_type: 5,
                coord_x: 0x1080,
                coord_y: 0x1080,
            }
        }
    );
}

#[test]
fn type_six_oil_tail_is_an_ordered_typed_effect_after_growth() {
    let base_world = world();
    let mut base_group = TerrainGroup {
        group_type: 6,
        start_max: 64,
        ..TerrainGroup::default()
    };
    base_group.tiles.items.push((8, 8));
    base_group.tiles.capacity = 4;
    let mut base_world = base_world;
    base_world.wdata_mut(8, 8).flags |= wflag::ROCKS;

    let mut probe_world = base_world.clone();
    let mut probe_group = base_group.clone();
    let mut probe_random = Random::new(1);
    let probe = probe_group
        .apply_player_group_growth(
            &mut probe_world,
            &mut probe_random,
            call(2, 1),
            &mut Vec::new(),
            &mut Vec::new(),
            &[],
        )
        .unwrap();
    let PlayerGroupGrowthOutcome::ExternalResolutionRequired { request } = probe.outcome else {
        panic!("expected oil object boundary")
    };
    assert_eq!(
        request,
        PlayerGroupExternalRequest::OilGoodMutation {
            world_x: 8,
            world_y: 8,
            enabled: true,
            good_type: 5,
            coord_x: 0x1980,
            coord_y: 0x1980,
        }
    );

    let mut world = base_world;
    let mut group = base_group;
    let mut random = Random::new(1);
    let receipt = group
        .apply_player_group_growth(
            &mut world,
            &mut random,
            call(2, 1),
            &mut Vec::new(),
            &mut Vec::new(),
            &[PlayerGroupExternalResolution::OilGoodsApplied { request }],
        )
        .unwrap();
    assert_eq!(receipt.external_resolutions_consumed, 1);
    assert_eq!(receipt.outcome, PlayerGroupGrowthOutcome::Returned(1));
    assert!(receipt.oil_deposits.as_ref().unwrap().completed);
    assert_ne!(world.wdata(8, 8).flags & wflag::OIL, 0);
}

#[test]
fn place_all_completes_nonmountain_clumps_player_inside_clump_with_one_pump_each() {
    let mut world = world();
    world.start_x.items.push(12);
    world.start_y.items.push(12);
    let mut groups = TerrainGroups {
        groups: vec![TerrainGroup {
            group_type: 6,
            chance: 100,
            min_clumps: 2,
            max_clumps: 2,
            pattern: 0,
            min_size: 1,
            max_size: 1,
            start_min: 0,
            start_max: 1,
            ..TerrainGroup::default()
        }],
        ..TerrainGroups::default()
    };
    let mut mountains = Mountains::default();
    let mut random = Random::new(0x1234_5678);
    let mut host_events = Vec::new();

    let error = groups
        .place_all_with_player_group_inputs(
            &mut world,
            &mut random,
            &mut mountains,
            0,
            1,
            &[],
            |event| host_events.push(event),
        )
        .unwrap_err();
    let PlaceAllError::GameplayPlacementUnavailable { preview, boundary } = error else {
        panic!("unexpected place_all result: {error:?}")
    };

    assert_eq!(
        boundary,
        TerrainPlacementBoundary::PlayerGroupPatternComplete { group_index: 0 }
    );
    assert_eq!(
        preview.player_group_host_events,
        [
            PlaceAllHostEvent::NetDaemonProcessAllPlayer {
                group_index: 0,
                clump_index: 0,
                player_index: 0,
            },
            PlaceAllHostEvent::NetDaemonProcessAllPlayer {
                group_index: 0,
                clump_index: 0,
                player_index: 1,
            },
            PlaceAllHostEvent::NetDaemonProcessAllPlayer {
                group_index: 0,
                clump_index: 1,
                player_index: 0,
            },
            PlaceAllHostEvent::NetDaemonProcessAllPlayer {
                group_index: 0,
                clump_index: 1,
                player_index: 1,
            },
        ]
    );
    assert_eq!(
        host_events.first(),
        Some(&PlaceAllHostEvent::NetDaemonProcessAll { group_index: 0 })
    );
    assert_eq!(&host_events[1..], preview.player_group_host_events);
    assert_eq!(preview.player_group_prefix.as_ref().unwrap().len(), 4);
    assert_eq!(preview.player_group_placed_after, [1, 1, 1, 1]);
}
