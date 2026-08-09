// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact ordering, transaction, and real-corpus input pins for the first
//! post-`place_all` World mutator.

use don_replay::check_player_forest::{
    execute_check_player_forest, CheckPlayerForestError, CheckPlayerForestFacts,
    PlayerForestResult, MAP_CHECK_PLAYER_FOREST_CALLER_RESUME_VA, MAP_CHECK_PLAYER_FOREST_RET_VA,
    MAP_CHECK_PLAYER_FOREST_VA, SHIPPED_CITY_CENTER_RADIUS, TERRAIN_GROUPS_NUBIFY_FOREST_VA,
};
use don_replay::checksum::Channel;
use don_replay::initial::{
    InitialItemBoundary, InitialItemReconstruction, InitialRules, InitialWorld,
    InitialWorldgenInputs, ReplayByteSpan, WorldgenSourceSpans,
};
use don_replay::place_all_boundary::{ReplayPlaceAllReceipt, TERRAIN_GROUPS_PLACE_ALL_RETURN_VA};
use don_replay::replay::Replay;
use don_replay::rules_channel::{
    RETAIL_AFTER_BALANCE, RETAIL_AFTER_CONSTANTS, RETAIL_AFTER_TYPES, RETAIL_WALKED_BYTES,
    SHIPPED_RULES_CHANNEL,
};
use don_replay::TERRAIN_GROUPS_PLACE_ALL_VA;
use don_sim::systems::combat::circle_table;
use don_sim::systems::map_terrain::{land, wflag, WCoord, World, WorldSection};
use don_sim::systems::regions::Regions;
use don_sim::systems::terrain_groups::FillFertileReceipt;
use std::path::Path;

const EDGE: i32 = 20;
const START: (i32, i32) = (10, 10);
const RNG_AFTER_PLACE_ALL: i32 = 0x1234_5678;

fn span(bytes: usize) -> ReplayByteSpan {
    ReplayByteSpan { offset: 0, bytes }
}

fn rules() -> InitialRules {
    InitialRules {
        serialized_offset: 0,
        serialized_bytes: 0,
        walked_bytes: RETAIL_WALKED_BYTES,
        checksum: SHIPPED_RULES_CHANNEL,
        after_types: RETAIL_AFTER_TYPES,
        after_constants: RETAIL_AFTER_CONSTANTS,
        after_balance: RETAIL_AFTER_BALANCE,
    }
}

fn plan() -> InitialItemReconstruction {
    InitialItemReconstruction {
        inputs: InitialWorldgenInputs {
            seed: 17,
            map_style: 6,
            map_size: 0,
            map_edge_world_cells: Some(EDGE),
            players: 1,
            game_rules: 0,
            starting_town: 0,
            scenario_type: 0,
            active_slots: vec![0],
            active_who: vec![0],
            active_teams: vec![0],
            sources: WorldgenSourceSpans {
                seed: span(4),
                map_style: span(1),
                map_size: span(1),
                players: span(1),
                game_rules: span(1),
                starting_town: span(1),
                scenario_type: span(1),
                player_flags: [span(1); 8],
                player_bodies: [None; 8],
            },
        },
        rules: Some(rules()),
        style: None,
        post_continent: None,
        tile_selection: None,
        fertility: None,
        fertility_error: None,
        fill_fertile: Some(FillFertileReceipt::default()),
        boundary: InitialItemBoundary::MapTerrainGroupsPlaceAllUnavailable {
            next_va: TERRAIN_GROUPS_PLACE_ALL_VA,
        },
    }
}

fn map(all_fertile: bool) -> InitialWorld {
    let mut world = World::init_default_rules(EDGE, EDGE);
    world.seed_map_generation(17);
    if all_fertile {
        for cell in &mut world.wdata {
            cell.land = land::FERTILE;
        }
    }
    world.add_starting_location(WCoord(START.0), WCoord(START.1));
    let checksum = world.checksum_sections();
    InitialWorld {
        world,
        generation_regions: Regions::default(),
        checksum,
        sourced_walked_bytes: 52,
    }
}

fn place_all(map: &InitialWorld) -> ReplayPlaceAllReceipt {
    ReplayPlaceAllReceipt {
        entry_va: TERRAIN_GROUPS_PLACE_ALL_VA,
        return_va: TERRAIN_GROUPS_PLACE_ALL_RETURN_VA,
        return_value: 1,
        map_style: 6,
        generated_starts: 1,
        tdata_cells: map.world.tdata.len(),
        random_state_before: 0x1020_3040,
        random_state_after: RNG_AFTER_PLACE_ALL,
        host_events: Vec::new(),
        checksum_before: map.checksum.clone(),
        checksum_after: map.checksum.clone(),
        sourced_walked_bytes: map.sourced_walked_bytes,
    }
}

fn facts() -> CheckPlayerForestFacts {
    CheckPlayerForestFacts::from_admitted_replay_rules(&plan(), false).unwrap()
}

fn circle_coord(index: usize) -> (i32, i32) {
    let circle = circle_table();
    (
        START.0 + circle.x[index] as i32,
        START.1 + circle.y[index] as i32,
    )
}

fn make_fertile(map: &mut InitialWorld, x: i32, y: i32, flags: u16) {
    let cell = map.world.wdata_mut(x, y);
    cell.land = land::FERTILE;
    cell.flags = flags;
}

fn refresh_checksum(map: &mut InitialWorld) {
    map.checksum = map.world.checksum_sections();
}

#[test]
fn shipped_ring_and_first_cardinal_prefix_change_only_wdata_without_rng() {
    let plan = plan();
    let mut map = map(true);
    map.world.wdata_mut(7, 8).land_sub = 0x7b;
    refresh_checksum(&mut map);
    let place_all = place_all(&map);

    let receipt = execute_check_player_forest(&plan, &mut map, &place_all, facts()).unwrap();

    assert_eq!(receipt.entry_va, MAP_CHECK_PLAYER_FOREST_VA);
    assert_eq!(receipt.return_va, MAP_CHECK_PLAYER_FOREST_RET_VA);
    assert_eq!(
        receipt.caller_resume_va,
        MAP_CHECK_PLAYER_FOREST_CALLER_RESUME_VA
    );
    assert_eq!(receipt.next_va, TERRAIN_GROUPS_NUBIFY_FOREST_VA);
    assert_eq!(receipt.city_center_radius, SHIPPED_CITY_CENTER_RADIUS);
    assert_eq!(receipt.circle_ring, 4);
    assert_eq!(receipt.existing_forest_scan, (1, 69));
    assert_eq!(receipt.candidate_anchor_scan, (21, 69));
    assert_eq!(receipt.random_state_before, RNG_AFTER_PLACE_ALL);
    assert_eq!(receipt.random_state_after, RNG_AFTER_PLACE_ALL);
    assert_eq!(receipt.random_draws, 0);
    assert_eq!(receipt.temporary_array_capacity, 4);
    assert_eq!(receipt.cells_changed, 3);
    assert_eq!(
        receipt
            .checksum_before
            .differing_sections(&receipt.checksum_after),
        [WorldSection::WData]
    );
    assert_eq!(receipt.sourced_walked_bytes, 52);

    let PlayerForestResult::Patched(patch) = &receipt.starts[0] else {
        panic!("expected a forest patch");
    };
    // Canonical ring 3 begins at (-3,-2). The target-3 prefix accepts N then E.
    assert_eq!(patch.anchor, (7, 8));
    assert_eq!(patch.requested_cells, 3);
    assert_eq!(patch.cells, [(7, 8), (7, 7), (8, 8)]);
    assert_eq!(map.world.wdata(7, 8).land_sub, 0x7b);
    for &(x, y) in &patch.cells {
        assert_eq!(
            map.world.wdata(x, y).flags & wflag::LAND_CLASS_MASK,
            wflag::FOREST
        );
    }
}

#[test]
fn existing_three_forests_skip_without_a_checksum_or_rng_change() {
    let plan = plan();
    let mut map = map(true);
    for index in 1..4 {
        let (x, y) = circle_coord(index);
        map.world.wdata_mut(x, y).flags |= wflag::FOREST;
    }
    refresh_checksum(&mut map);
    let place_all = place_all(&map);

    let receipt = execute_check_player_forest(&plan, &mut map, &place_all, facts()).unwrap();

    assert_eq!(receipt.cells_changed, 0);
    assert_eq!(receipt.checksum_before, receipt.checksum_after);
    assert_eq!(receipt.random_draws, 0);
    assert_eq!(
        receipt.starts,
        [PlayerForestResult::AlreadySufficient {
            start_index: 0,
            existing_forests: 3,
        }]
    );
}

#[test]
fn exact_three_two_one_fallback_and_waterhalf_admission_are_ordered() {
    for (eligible_offsets, expected_cells) in [
        (vec![(0, 0), (0, -1), (1, 0)], 3u8),
        (vec![(0, 0), (0, -1)], 2u8),
        (vec![(0, 0)], 1u8),
    ] {
        let plan = plan();
        let mut map = map(false);
        let anchor = circle_coord(21);
        for (dx, dy) in eligible_offsets {
            // Deep-water land is admissible precisely because WATERHALF makes
            // WorldData::is_ocean return false. The bit must survive set_land.
            let x = anchor.0 + dx;
            let y = anchor.1 + dy;
            let cell = map.world.wdata_mut(x, y);
            cell.flags = wflag::WATERHALF;
        }
        refresh_checksum(&mut map);
        let place_all = place_all(&map);

        let receipt = execute_check_player_forest(&plan, &mut map, &place_all, facts()).unwrap();
        let PlayerForestResult::Patched(patch) = &receipt.starts[0] else {
            panic!("expected fallback patch");
        };
        assert_eq!(patch.requested_cells, expected_cells);
        assert_eq!(patch.cells.len(), expected_cells as usize);
        for &(x, y) in &patch.cells {
            assert_ne!(map.world.wdata(x, y).flags & wflag::WATERHALF, 0);
            assert_ne!(map.world.wdata(x, y).flags & wflag::FOREST, 0);
        }
    }
}

#[test]
fn start_city_and_coast_candidates_are_rejected() {
    let plan = plan();
    let mut map = map(false);
    let city_candidate = circle_coord(21);
    let coast_candidate = circle_coord(22);
    make_fertile(&mut map, city_candidate.0, city_candidate.1, 0);
    make_fertile(&mut map, coast_candidate.0, coast_candidate.1, wflag::COAST);
    let city_index = city_candidate.1 * map.world.xs + city_candidate.0;
    map.world.start_city_locs[(city_index >> 3) as usize] |= 1u8 << ((city_index & 7) as u32);
    refresh_checksum(&mut map);
    let place_all = place_all(&map);

    let receipt = execute_check_player_forest(&plan, &mut map, &place_all, facts()).unwrap();

    assert_eq!(receipt.cells_changed, 0);
    assert_eq!(receipt.checksum_before, receipt.checksum_after);
    assert!(matches!(
        &receipt.starts[0],
        PlayerForestResult::NoPatch { .. }
    ));
}

#[test]
fn invalid_place_all_receipt_rolls_back_before_any_world_write() {
    let plan = plan();
    let mut map = map(true);
    let checksum_before = map.checksum.clone();
    let flags_before = map.world.wdata(7, 8).flags;
    let mut place_all = place_all(&map);
    place_all.return_value = 0;

    let error = execute_check_player_forest(&plan, &mut map, &place_all, facts()).unwrap_err();

    assert_eq!(error, CheckPlayerForestError::PlaceAllReceiptMismatch);
    assert_eq!(map.checksum, checksum_before);
    assert_eq!(map.world.checksum_sections(), checksum_before);
    assert_eq!(map.world.wdata(7, 8).flags, flags_before);
}

#[test]
fn retail_replay_proves_the_static_radius_and_a_nonempty_world_deadline_only() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("ron-data/replays/multi/Playback___2025.02.10_21_26_50__Mon_.rcx");
    if !path.is_file() {
        eprintln!(
            "\n  SKIPPED — NOT A PASS. The exact supported retail replay is absent; \
             check_player_forest corpus inputs were not exercised.\n"
        );
        return;
    }
    let replay = Replay::open(&path).expect("supported retail replay must decode");
    let plan = replay.initial.reconstruct_items();
    let facts = CheckPlayerForestFacts::from_admitted_replay_rules(&plan, false).unwrap();
    assert_eq!(facts.city_center_radius, SHIPPED_CITY_CENTER_RADIUS);
    assert_eq!(facts.rules_after_constants, RETAIL_AFTER_CONSTANTS);

    let recorded_world = replay
        .turns
        .iter()
        .find_map(|turn| turn.any_checksums())
        .expect("supported replay must carry checksums")
        .1
        .get(Channel::World);
    assert_ne!(recorded_world, 1, "retail World channel must be nonempty");

    // This test proves the replay-carried static input and the real checksum
    // deadline. It deliberately does not claim an end-to-end checksum match:
    // the unmodified corpus plan still stops at installed map-style content.
    assert!(matches!(
        plan.boundary,
        InitialItemBoundary::MapStyleContentUnavailable { .. }
    ));
}
