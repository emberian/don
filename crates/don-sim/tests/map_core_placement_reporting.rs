// SPDX-License-Identifier: GPL-3.0-or-later

//! Exact `TerrainGroups::place_all` reporting tail and successful commit.

use don_sim::rng::Random;
use don_sim::systems::map_terrain::{land, wflag, World};
use don_sim::systems::mountains::Mountains;
use don_sim::systems::regions::Regions;
use don_sim::systems::terrain_doobers::DooberTilesetRules;
use don_sim::systems::terrain_groups::{
    PlaceAllError, PlaceAllGroupInput, PlaceAllHostEvent, PlacementReportString,
    PlacementReportingError, PlacementReportingInputs, TerrainGroup, TerrainGroups,
};
use don_sim::systems::terrain_region_placement::RegionHelpingState;

fn reporting(num_players: i32) -> PlacementReportingInputs {
    let mut player_scores = [[0; 5]; 8];
    for (player, scores) in player_scores.iter_mut().enumerate() {
        for (type_slot, score) in scores.iter_mut().enumerate() {
            *score = player as i32 * 100 + type_slot as i32;
        }
    }
    PlacementReportingInputs {
        num_players,
        player_scores,
    }
}

#[test]
fn reporting_emits_header_then_type_major_player_score_lines() {
    let inputs = reporting(2);
    let mut host = Vec::new();

    let receipt =
        TerrainGroups::plan_placement_reporting(37, inputs, |event| host.push(event)).unwrap();

    assert_eq!(receipt.console_info_before, 37);
    assert_eq!(receipt.console_info_after, 0);
    assert_eq!(receipt.return_value, 1);
    assert_eq!(receipt.host_events, host);
    assert_eq!(host.len(), 16);
    assert_eq!(
        host[0],
        PlaceAllHostEvent::PlacementReportHeader {
            text: PlacementReportString::Header,
        }
    );

    let labels = [
        PlacementReportString::GroupType4,
        PlacementReportString::GroupType5,
        PlacementReportString::GroupType6,
        PlacementReportString::GroupType7,
        PlacementReportString::GroupType8,
    ];
    for (type_slot, label) in labels.into_iter().enumerate() {
        let base = 1 + type_slot * 3;
        assert_eq!(
            host[base],
            PlaceAllHostEvent::PlacementReportGroup {
                prefix: PlacementReportString::GroupPrefix,
                label,
                type_slot: type_slot as i32,
            }
        );
        for player_index in 0..2 {
            assert_eq!(
                host[base + 1 + player_index],
                PlaceAllHostEvent::PlacementReportPlayerScore {
                    prefix: PlacementReportString::PlayerPrefix,
                    player_index: player_index as i32,
                    score_prefix: PlacementReportString::ScorePrefix,
                    type_slot: type_slot as i32,
                    score: inputs.player_scores[player_index][type_slot],
                }
            );
        }
    }

    assert_eq!(PlacementReportString::Header as u32, 0x1fcc0);
    assert_eq!(PlacementReportString::GroupType8 as u32, 0x2878);
    assert_eq!(PlacementReportString::ScorePrefix as u32, 0x1fd4c);
}

#[test]
fn nonpositive_player_count_keeps_the_five_native_type_lines() {
    let mut host = Vec::new();

    let receipt =
        TerrainGroups::plan_placement_reporting(-3, reporting(-1), |event| host.push(event))
            .unwrap();

    assert_eq!(receipt.host_events.len(), 6);
    assert!(host
        .iter()
        .skip(1)
        .all(|event| matches!(event, PlaceAllHostEvent::PlacementReportGroup { .. })));
}

#[test]
fn player_count_beyond_the_pdb_array_fails_before_logging() {
    let mut host_calls = 0;

    assert_eq!(
        TerrainGroups::plan_placement_reporting(9, reporting(9), |_| host_calls += 1),
        Err(PlacementReportingError::UnsupportedPlayerCount {
            num_players: 9,
            supported: 8,
        })
    );
    assert_eq!(host_calls, 0);
}

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

fn helping() -> RegionHelpingState {
    RegionHelpingState {
        is_helping: false,
        num_players: 1,
        lowest_player: [0; 5],
        scores: [[0; 5]; 8],
    }
}

#[test]
fn complete_mixed_transaction_commits_preview_state_after_the_final_log() {
    let mut world = world();
    let before_world = world.clone();
    let mut groups = TerrainGroups {
        groups: vec![group()],
        console_info: 37,
        ..TerrainGroups::default()
    };
    let before_groups = groups.clone();
    let mut mountains = Mountains::default();
    let mut random = Random::new(0x1234_5678);
    let before_random = random;
    let regions = Regions::default();
    let inputs = [PlaceAllGroupInput::Player {
        group_index: 0,
        externals: Vec::new(),
    }];
    let mut host = Vec::new();

    let result = groups.place_all_with_group_reporting_inputs(
        &mut world,
        &regions,
        &mut random,
        &mut mountains,
        0,
        1,
        Some(helping()),
        &inputs,
        DooberTilesetRules::default(),
        9,
        reporting(1),
        |event| host.push(event),
    );

    assert_eq!(result, Ok(1));
    assert_eq!(groups.console_info, 0);
    assert_ne!(groups, before_groups);
    assert_eq!(groups.groups[0].placed, [1]);
    assert!(world
        .wdata
        .iter()
        .any(|cell| cell.flags & wflag::ROCKS != 0));
    assert_ne!(world.wdata, before_world.wdata);
    assert_ne!(random, before_random);
    assert_eq!(
        &host[..2],
        [
            PlaceAllHostEvent::NetDaemonProcessAll { group_index: 0 },
            PlaceAllHostEvent::NetDaemonProcessAllPlayer {
                group_index: 0,
                clump_index: 0,
                player_index: 0,
            },
        ]
    );
    assert!(matches!(
        host[2],
        PlaceAllHostEvent::PlacementReportHeader { .. }
    ));
    assert_eq!(host.len(), 13);
}

#[test]
fn invalid_reporting_shape_is_preflighted_before_the_first_group_pump() {
    let mut world = world();
    let before_world = world.clone();
    let mut groups = TerrainGroups {
        groups: vec![group()],
        console_info: 37,
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
    let mut host = Vec::new();

    let result = groups.place_all_with_group_reporting_inputs(
        &mut world,
        &regions,
        &mut random,
        &mut mountains,
        0,
        1,
        Some(helping()),
        &inputs,
        DooberTilesetRules::default(),
        9,
        reporting(9),
        |event| host.push(event),
    );

    assert_eq!(
        result,
        Err(PlaceAllError::InvalidPlacementReporting(
            PlacementReportingError::UnsupportedPlayerCount {
                num_players: 9,
                supported: 8,
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
