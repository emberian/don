// SPDX-License-Identifier: GPL-3.0-or-later
//! Transaction and mutation pins for the replay-to-`place_all` adapter.

use don_replay::continent::{ContinentReceipt, ContinentStop};
use don_replay::initial::{
    InitialItemBoundary, InitialItemReconstruction, InitialWorld, InitialWorldgenInputs,
    ReplayByteSpan, WorldgenSourceSpans,
};
use don_replay::place_all_boundary::{
    execute_replay_place_all, ReplayPlaceAllError, ReplayPlaceAllFacts, ReplayPlaceAllHostFacts,
    ReplayPlaceAllPlayerFacts, ReplayPlaceAllRuntime, ReplayPlaceAllTDataFacts,
    TERRAIN_GROUPS_PLACE_ALL_RETURN_VA,
};
use don_replay::post_continent::REGIONS_CLEAR_ALL_VA;
use don_replay::TERRAIN_GROUPS_PLACE_ALL_VA;
use don_sim::rng::Random;
use don_sim::systems::map_terrain::World;
use don_sim::systems::mountains::{MountainRangeEntry, MountainRangeList, Mountains};
use don_sim::systems::regions::Regions;
use don_sim::systems::terrain_doobers::DooberTilesetRules;
use don_sim::systems::terrain_groups::{
    FillFertileReceipt, PlaceAllError, PlacementReportingError, PlacementReportingInputs,
    TerrainGroups,
};

fn span() -> ReplayByteSpan {
    ReplayByteSpan {
        offset: 0,
        bytes: 1,
    }
}

fn plan() -> InitialItemReconstruction {
    InitialItemReconstruction {
        inputs: InitialWorldgenInputs {
            seed: 17,
            map_style: 6,
            map_size: 0,
            map_edge_world_cells: Some(8),
            players: 0,
            game_rules: 0,
            starting_town: 0,
            scenario_type: 0,
            active_slots: Vec::new(),
            active_who: Vec::new(),
            active_teams: Vec::new(),
            sources: WorldgenSourceSpans {
                seed: ReplayByteSpan {
                    offset: 0,
                    bytes: 4,
                },
                map_style: span(),
                map_size: span(),
                players: span(),
                game_rules: span(),
                starting_town: span(),
                scenario_type: span(),
                player_flags: [span(); 8],
                player_bodies: [None; 8],
            },
        },
        rules: None,
        style: None,
        post_continent: None,
        tile_selection: None,
        fertility: None,
        fertility_error: None,
        fill_fertile: Some(FillFertileReceipt::default()),
        place_all_advance: None,
        place_all_advance_error: None,
        mountain_range_error: None,
        boundary: InitialItemBoundary::MapTerrainGroupsPlaceAllUnavailable {
            next_va: TERRAIN_GROUPS_PLACE_ALL_VA,
        },
    }
}

fn continent(random_state: i32) -> ContinentReceipt {
    ContinentReceipt {
        map_style: 6,
        make_continents_va: 0x0068_d760,
        orientation: 0,
        rng_initial: 17,
        rng_final: random_state,
        direct_rng_sites: Vec::new(),
        retry_attempt: 1,
        world_wiped: true,
        world_inverted: false,
        regions_cleared: 2,
        region_seeds: Vec::new(),
        region_growths: Vec::new(),
        grow_valid_calls: Vec::new(),
        lake_candidates: Vec::new(),
        pool_eliminations: Vec::new(),
        player_land: None,
        team_partition: None,
        east_indies_tail: None,
        starts_added: 0,
        start_min: None,
        stop: ContinentStop::HookComplete {
            next_va: REGIONS_CLEAR_ALL_VA,
        },
    }
}

fn map() -> InitialWorld {
    let mut world = World::init_default_rules(8, 8);
    assert_eq!(world.seed_map_generation(17), Some(17));
    let checksum = world.checksum_sections();
    InitialWorld {
        world,
        generation_regions: Regions::default(),
        checksum,
        ownership: None,
        sourced_walked_bytes: 52,
    }
}

fn runtime() -> ReplayPlaceAllRuntime {
    let ranges = || {
        MountainRangeList::new(vec![
            MountainRangeEntry::new(11, 1),
            MountainRangeEntry::new(22, 2),
        ])
    };
    ReplayPlaceAllRuntime {
        terrain_groups: TerrainGroups {
            console_info: 71,
            ..TerrainGroups::default()
        },
        mountains: Mountains {
            small_ranges: ranges(),
            medium_ranges: ranges(),
            large_ranges: ranges(),
        },
    }
}

fn facts(map: &InitialWorld) -> ReplayPlaceAllFacts {
    let mut cells = map.world.tdata.clone();
    // Cliff blocker is not a mountain TCoord, but it makes installation and
    // the resulting world-channel checksum observably mutation-sensitive.
    cells[0] = 1;
    ReplayPlaceAllFacts {
        tdata: ReplayPlaceAllTDataFacts {
            tile_xs: map.world.tile_xs,
            tile_ys: map.world.tile_ys,
            tile_size: map.world.tile_size,
            cells,
        },
        doober_rules: DooberTilesetRules::default(),
        players: ReplayPlaceAllPlayerFacts {
            progress: 43,
            place_players: 1,
            helping: None,
            reporting: PlacementReportingInputs::default(),
        },
        host: ReplayPlaceAllHostFacts::default(),
    }
}

#[test]
fn lawful_replay_state_crosses_place_all_and_commits_checksum_and_host_tail() {
    let plan = plan();
    let continent = continent(0x1234_5678);
    let mut map = map();
    let old_checksum = map.checksum.clone();
    let facts = facts(&map);
    let mut runtime = runtime();
    let mut forwarded = Vec::new();

    let receipt =
        execute_replay_place_all(&plan, &mut map, &continent, &mut runtime, &facts, |event| {
            forwarded.push(event)
        })
        .unwrap();

    let mut expected_random = Random::new(continent.rng_final);
    expected_random.advance();
    expected_random.advance();
    expected_random.advance();
    assert_eq!(receipt.entry_va, TERRAIN_GROUPS_PLACE_ALL_VA);
    assert_eq!(receipt.return_va, TERRAIN_GROUPS_PLACE_ALL_RETURN_VA);
    assert_eq!(receipt.return_value, 1);
    assert_eq!(receipt.random_state_before, continent.rng_final);
    assert_eq!(receipt.random_state_after, expected_random.state());
    assert_eq!(receipt.host_events, forwarded);
    assert_eq!(receipt.host_events.len(), 6); // header plus five type rows
    assert_eq!(runtime.terrain_groups.console_info, 0);
    assert_eq!(map.world.tdata[0], 1);
    assert_eq!(map.checksum, map.world.checksum_sections());
    assert_ne!(map.checksum.full, old_checksum.full);
    assert_eq!(receipt.sourced_walked_bytes, 52);
}

#[test]
fn continent_rng_mutation_changes_the_three_mountain_cursor_draws() {
    let plan = plan();
    let mut map_a = map();
    let facts_a = facts(&map_a);
    let mut runtime_a = runtime();
    let receipt_a = execute_replay_place_all(
        &plan,
        &mut map_a,
        &continent(0x1234_5678),
        &mut runtime_a,
        &facts_a,
        |_| {},
    )
    .unwrap();

    let mut map_b = map();
    let facts_b = facts(&map_b);
    let mut runtime_b = runtime();
    let receipt_b = execute_replay_place_all(
        &plan,
        &mut map_b,
        &continent(0x1234_5679),
        &mut runtime_b,
        &facts_b,
        |_| {},
    )
    .unwrap();

    assert_ne!(receipt_a.random_state_before, receipt_b.random_state_before);
    assert_ne!(receipt_a.random_state_after, receipt_b.random_state_after);
}

#[test]
fn invalid_reporting_is_transactional_and_never_releases_buffered_host_effects() {
    let plan = plan();
    let continent = continent(0x1234_5678);
    let mut map = map();
    let world_checksum_before = map.world.checksum_sections();
    let checksum_before = map.checksum.clone();
    let mut input_facts = facts(&map);
    input_facts.players.reporting.num_players = 9;
    let mut runtime = runtime();
    let runtime_before = runtime.clone();
    let mut forwarded = Vec::new();

    let error = execute_replay_place_all(
        &plan,
        &mut map,
        &continent,
        &mut runtime,
        &input_facts,
        |event| forwarded.push(event),
    )
    .unwrap_err();

    assert_eq!(
        error,
        ReplayPlaceAllError::PlaceAll(PlaceAllError::InvalidPlacementReporting(
            PlacementReportingError::UnsupportedPlayerCount {
                num_players: 9,
                supported: 8,
            }
        ))
    );
    assert!(forwarded.is_empty());
    assert_eq!(runtime, runtime_before);
    assert_eq!(map.checksum, checksum_before);
    assert_eq!(map.world.checksum_sections(), world_checksum_before);
    assert_eq!(map.world.tdata[0], 0);
}

#[test]
fn tdata_shape_and_generated_start_proofs_fail_before_simulation() {
    let plan = plan();
    let mut continent = continent(9);
    let mut map = map();
    let mut input_facts = facts(&map);
    let mut runtime = runtime();

    input_facts.tdata.cells.pop();
    let expected_tdata_cells = map.world.tdata.len();
    let supplied_tdata_cells = input_facts.tdata.cells.len();
    assert_eq!(
        execute_replay_place_all(
            &plan,
            &mut map,
            &continent,
            &mut runtime,
            &input_facts,
            |_| {},
        ),
        Err(ReplayPlaceAllError::TDataLengthMismatch {
            expected: expected_tdata_cells,
            supplied: supplied_tdata_cells,
        })
    );

    input_facts = facts(&map);
    continent.starts_added = 1;
    assert_eq!(
        execute_replay_place_all(
            &plan,
            &mut map,
            &continent,
            &mut runtime,
            &input_facts,
            |_| {},
        ),
        Err(ReplayPlaceAllError::GeneratedStartCountMismatch {
            receipt_starts: 1,
            start_x: 0,
            start_y: 0,
        })
    );
}
