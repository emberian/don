// SPDX-License-Identifier: GPL-3.0-or-later
//! Receipt-chain integration for the reconstructed post-placement `Map::make` tail.

use don_replay::check_player_forest::{
    execute_check_player_forest, CheckPlayerForestFacts, MAP_CHECK_PLAYER_FOREST_CALL_VA,
};
use don_replay::initial::{
    InitialItemBoundary, InitialItemReconstruction, InitialRules, InitialWorld,
    InitialWorldgenInputs, MapTerrainRepairError, ReplayByteSpan, WorldgenSourceSpans,
};
use don_replay::map_style::{
    MapGenerationCheckpoint, MAP_MAKE_SCHEDULE, MAP_POST_CHECK_PLAYER_FOREST_CHECKPOINT_CALL_VA,
    MAP_POST_CHECK_PLAYER_FOREST_SOURCE_TOKEN, MAP_POST_NUBIFY_FOREST_CHECKPOINT_CALL_VA,
    MAP_POST_NUBIFY_FOREST_SOURCE_TOKEN, MAP_POST_PLACE_ALL_CHECKPOINT_CALL_VA,
    MAP_POST_PLACE_ALL_SOURCE_TOKEN, MAP_POST_TERRAIN_TRANSITIONS_CHECKPOINT_CALL_VA,
    MAP_POST_TERRAIN_TRANSITIONS_SOURCE_TOKEN,
};
use don_replay::nubify_forest_frontier::{
    execute_nubify_forest_frontier, EdgeOfRegionHost, EdgeOfRegionProvenance, EdgeOfRegionReceipt,
    EdgeOfRegionRequest, MAP_NUBIFY_FOREST_CALL_VA, MAP_POST_NUBIFY_CHECKSUM_CALL_VA,
    MAP_POST_NUBIFY_CHECKSUM_SOURCE_TOKEN,
};
use don_replay::place_all_boundary::{ReplayPlaceAllReceipt, TERRAIN_GROUPS_PLACE_ALL_RETURN_VA};
use don_replay::post_nubify_transition_frontier::{
    PostNubifyTransitionError, TerrainTransitionEvidence, TerrainTransitionLiveFacts,
    TerrainTransitionTuning, MAP_FIX_TRANSITIONS_CALL_VA, MAP_POST_NUBIFY_CHECKPOINT_CALL_VA,
    MAP_POST_NUBIFY_SOURCE_TOKEN, MAP_POST_TRANSITIONS_CHECKPOINT_CALL_VA,
    MAP_POST_TRANSITIONS_SOURCE_TOKEN, SHIPPED_EXE_SHA256, TILESET_DATA_POINTER_SLOT_VA,
};
use don_replay::rules_channel::{
    RETAIL_AFTER_BALANCE, RETAIL_AFTER_CONSTANTS, RETAIL_AFTER_TYPES, RETAIL_WALKED_BYTES,
    SHIPPED_RULES_CHANNEL,
};
use don_replay::world_owner_frontier::{
    InitialWorldPrefixEvidence, ReplaySpan, RulesWorldEvidence, WorldOwnerLedger,
};
use don_replay::TERRAIN_GROUPS_PLACE_ALL_VA;
use don_sim::rng::Random;
use don_sim::systems::map_terrain::{land, wflag, WCoord, World, WorldSection};
use don_sim::systems::regions::Regions;
use don_sim::systems::terrain_groups::FillFertileReceipt;

const EDGE: i32 = 8;
const RNG_AFTER_PLACE_ALL: i32 = 1;

fn span(bytes: usize) -> ReplayByteSpan {
    ReplayByteSpan { offset: 0, bytes }
}

fn plan() -> InitialItemReconstruction {
    InitialItemReconstruction {
        inputs: InitialWorldgenInputs {
            seed: 1,
            map_style: 6,
            map_size: 0,
            map_edge_world_cells: Some(EDGE),
            players: 0,
            game_rules: 0,
            starting_town: 0,
            scenario_type: 0,
            active_slots: Vec::new(),
            active_who: Vec::new(),
            active_teams: Vec::new(),
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
        rules: Some(InitialRules {
            serialized_offset: 0,
            serialized_bytes: 0,
            serialized_sha256: [1; 32],
            walked_bytes: RETAIL_WALKED_BYTES,
            checksum: SHIPPED_RULES_CHANNEL,
            after_types: RETAIL_AFTER_TYPES,
            after_constants: RETAIL_AFTER_CONSTANTS,
            after_balance: RETAIL_AFTER_BALANCE,
        }),
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

fn map() -> InitialWorld {
    let mut world = World::init_default_rules(EDGE, EDGE);
    for cell in &mut world.wdata {
        cell.land = land::FERTILE;
    }
    for x in 3..=4 {
        for y in 1..=4 {
            world.wdata_mut(x, y).flags = wflag::FOREST;
        }
    }
    let checksum = world.checksum_sections();
    InitialWorld {
        world,
        generation_regions: Regions::default(),
        checksum,
        ownership: None,
        sourced_walked_bytes: 52,
    }
}

fn owned_map() -> InitialWorld {
    let mut world = World::init_default_rules(40, 40);
    assert_eq!(world.seed_map_generation(1), Some(1));
    for cell in &mut world.wdata {
        cell.land = land::FERTILE;
    }
    for x in 3..=4 {
        for y in 1..=4 {
            world.wdata_mut(x, y).flags = wflag::FOREST;
        }
    }
    // A real player start makes `Map::check_player_forest` non-vacuous. It sits far
    // enough from the forest patch that the shipped `city_center_radius = 20` clear
    // does not consume it, so the nubify passes downstream still have candidates.
    assert_eq!(world.add_starting_location(WCoord(20), WCoord(20)), 0);
    let ownership = WorldOwnerLedger::from_initial_prefix(
        &world,
        InitialWorldPrefixEvidence {
            replay_sha256: [0x11; 32],
            map_size: 0,
            map_size_span: ReplaySpan::new(10, 1),
            seed: 1,
            seed_span: ReplaySpan::new(20, 4),
            rules: Some(RulesWorldEvidence {
                serialized_span: ReplaySpan::new(
                    100,
                    don_replay::initial::SHIPPED_RULES_SERIALIZED_BYTES,
                ),
                serialized_sha256: [0x22; 32],
                checksum: SHIPPED_RULES_CHANNEL,
                after_constants: RETAIL_AFTER_CONSTANTS,
                player_base: 44,
                player_civic: 4,
                player_city: 4,
            }),
        },
    )
    .unwrap();
    let sourced_walked_bytes = ownership.coverage().owned_bytes as u64;
    let checksum = world.checksum_sections();
    InitialWorld {
        world,
        generation_regions: Regions::default(),
        checksum,
        ownership: Some(ownership),
        sourced_walked_bytes,
    }
}

fn place_all(map: &InitialWorld) -> ReplayPlaceAllReceipt {
    ReplayPlaceAllReceipt {
        entry_va: TERRAIN_GROUPS_PLACE_ALL_VA,
        return_va: TERRAIN_GROUPS_PLACE_ALL_RETURN_VA,
        return_value: 1,
        map_style: 6,
        generated_starts: map.world.start_x.items.len(),
        tdata_cells: map.world.tdata.len(),
        random_state_before: RNG_AFTER_PLACE_ALL,
        random_state_after: RNG_AFTER_PLACE_ALL,
        host_events: Vec::new(),
        checksum_before: map.checksum.clone(),
        checksum_after: map.checksum.clone(),
        sourced_walked_bytes: map.sourced_walked_bytes,
    }
}

#[derive(Default)]
struct RejectingExactEdgeHost {
    requests: Vec<EdgeOfRegionRequest>,
}

impl EdgeOfRegionHost for RejectingExactEdgeHost {
    fn is_edge_of_region(
        &mut self,
        world: &World,
        request: &EdgeOfRegionRequest,
    ) -> Option<EdgeOfRegionReceipt> {
        assert_eq!(request.world_checksum, world.checksum_sections());
        self.requests.push(request.clone());
        Some(EdgeOfRegionReceipt {
            request: request.clone(),
            result: 1,
            provenance: EdgeOfRegionProvenance::ExactPort {
                implementation_sha256: [0x51; 32],
                proof_document: "integration-fixture-edge-port".to_owned(),
            },
        })
    }
}

#[derive(Default)]
struct AcceptingExactEdgeHost {
    requests: Vec<EdgeOfRegionRequest>,
}

impl EdgeOfRegionHost for AcceptingExactEdgeHost {
    fn is_edge_of_region(
        &mut self,
        world: &World,
        request: &EdgeOfRegionRequest,
    ) -> Option<EdgeOfRegionReceipt> {
        assert_eq!(request.world_checksum, world.checksum_sections());
        self.requests.push(request.clone());
        Some(EdgeOfRegionReceipt {
            request: request.clone(),
            result: 0,
            provenance: EdgeOfRegionProvenance::ExactPort {
                implementation_sha256: [0x51; 32],
                proof_document: "integration-fixture-edge-port".to_owned(),
            },
        })
    }
}

fn expected_nubify_random_state() -> i32 {
    let mut random = Random::new(RNG_AFTER_PLACE_ALL);
    for _ in 0..4 {
        random.get(0, 0xffff);
    }
    random.state()
}

fn transition_facts(map: &InitialWorld, random_state: i32) -> TerrainTransitionLiveFacts {
    TerrainTransitionLiveFacts {
        tuning: TerrainTransitionTuning {
            forest_base: -1,
            forest_chance: 0,
            mountain_base: -1,
            mountain_chance: 0,
            coast_base: -1,
            coast_chance: 0,
        },
        // This fixture exercises the production evidence shape. The carried
        // digest is not presented as independent authentication.
        evidence: TerrainTransitionEvidence::RetailCapture {
            executable_sha256: SHIPPED_EXE_SHA256.to_owned(),
            capture_sha256: [0xa5; 32],
            tileset_data_pointer_slot_va: TILESET_DATA_POINTER_SLOT_VA,
            tileset_data_object_va: 0x1234_5000,
            rules_after_constants: RETAIL_AFTER_CONSTANTS,
            checkpoint_call_va: MAP_POST_NUBIFY_CHECKPOINT_CALL_VA,
            source_token: MAP_POST_NUBIFY_SOURCE_TOKEN,
            world_checksum: map.checksum.clone(),
            random_state,
        },
    }
}

fn checkpoint(name: &str) -> MapGenerationCheckpoint {
    MAP_MAKE_SCHEDULE
        .iter()
        .find(|stage| stage.name == name)
        .and_then(|stage| stage.checkpoint)
        .expect("post-placement stage must retain its exact checksum deadline")
}

fn evidence_va(name: &str) -> u32 {
    MAP_MAKE_SCHEDULE
        .iter()
        .find(|stage| stage.name == name)
        .and_then(|stage| stage.evidence_va)
        .expect("reconstructed post-placement stage must retain its exact call evidence")
}

#[test]
fn schedule_replaces_the_opaque_repair_row_with_exact_calls_and_tokens() {
    let names = MAP_MAKE_SCHEDULE
        .iter()
        .map(|stage| stage.name)
        .collect::<Vec<_>>();
    assert_eq!(
        &names[7..11],
        [
            "terrain_groups_place_all",
            "check_player_forest",
            "nubify_forest",
            "post_nubify_transitions",
        ]
    );
    assert!(!names.contains(&"terrain_repairs"));
    assert_eq!(evidence_va("terrain_groups_place_all"), 0x0068_c010);
    assert_eq!(
        evidence_va("check_player_forest"),
        MAP_CHECK_PLAYER_FOREST_CALL_VA
    );
    assert_eq!(evidence_va("nubify_forest"), MAP_NUBIFY_FOREST_CALL_VA);
    assert_eq!(
        evidence_va("post_nubify_transitions"),
        MAP_FIX_TRANSITIONS_CALL_VA
    );
    assert_eq!(
        checkpoint("terrain_groups_place_all"),
        MapGenerationCheckpoint {
            call_va: MAP_POST_PLACE_ALL_CHECKPOINT_CALL_VA,
            source_token: MAP_POST_PLACE_ALL_SOURCE_TOKEN,
        }
    );
    assert_eq!(
        checkpoint("check_player_forest"),
        MapGenerationCheckpoint {
            call_va: MAP_POST_CHECK_PLAYER_FOREST_CHECKPOINT_CALL_VA,
            source_token: MAP_POST_CHECK_PLAYER_FOREST_SOURCE_TOKEN,
        }
    );
    assert_eq!(
        checkpoint("nubify_forest"),
        MapGenerationCheckpoint {
            call_va: MAP_POST_NUBIFY_CHECKSUM_CALL_VA,
            source_token: MAP_POST_NUBIFY_CHECKSUM_SOURCE_TOKEN,
        }
    );
    assert_eq!(
        MAP_POST_NUBIFY_FOREST_CHECKPOINT_CALL_VA,
        MAP_POST_NUBIFY_CHECKPOINT_CALL_VA
    );
    assert_eq!(
        MAP_POST_NUBIFY_FOREST_SOURCE_TOKEN,
        MAP_POST_NUBIFY_SOURCE_TOKEN
    );
    assert_eq!(
        checkpoint("post_nubify_transitions"),
        MapGenerationCheckpoint {
            call_va: MAP_POST_TRANSITIONS_CHECKPOINT_CALL_VA,
            source_token: MAP_POST_TRANSITIONS_SOURCE_TOKEN,
        }
    );
    assert_eq!(
        MAP_POST_TERRAIN_TRANSITIONS_CHECKPOINT_CALL_VA,
        MAP_POST_TRANSITIONS_CHECKPOINT_CALL_VA
    );
    assert_eq!(
        MAP_POST_TERRAIN_TRANSITIONS_SOURCE_TOKEN,
        MAP_POST_TRANSITIONS_SOURCE_TOKEN
    );
}

#[test]
fn real_bridge_preserves_checksum_rng_and_live_fact_identity() {
    let plan = plan();
    let mut map = map();
    let place_all = place_all(&map);
    let facts = transition_facts(&map, expected_nubify_random_state());
    let forest_facts = CheckPlayerForestFacts::from_admitted_replay_rules(&plan, false).unwrap();
    let mut edge_host = RejectingExactEdgeHost::default();

    let receipt = plan
        .advance_map_make_terrain_repairs(
            &mut map,
            &place_all,
            forest_facts,
            &mut edge_host,
            &facts,
        )
        .unwrap();

    assert!(!edge_host.requests.is_empty());
    assert_eq!(receipt.nubify_forest.random_draws.len(), 4);
    assert_eq!(
        receipt.check_player_forest.random_state_after,
        receipt.nubify_forest.random_state_before
    );
    assert_eq!(
        receipt.nubify_forest.random_state_after,
        receipt.post_nubify_transitions.random_state_before
    );
    assert_eq!(
        receipt.check_player_forest.checksum_after,
        receipt.nubify_forest.checksum_before
    );
    assert_eq!(
        receipt.nubify_forest.checksum_after,
        receipt.post_nubify_transitions.checksum_before
    );
    assert_eq!(map.checksum, receipt.post_nubify_transitions.checksum_after);
    assert_eq!(receipt.post_nubify_transitions.source_token, 0x1eb9);
    assert_eq!(receipt.post_nubify_transitions.next_source_token, 0x1ebe);
    assert_eq!(receipt.post_nubify_transitions.sourced_walked_bytes, 52);
}

#[test]
fn canonical_owner_advances_only_changed_world_bytes_across_the_chain() {
    let mut plan = plan();
    plan.inputs.map_edge_world_cells = Some(40);
    let mut map = owned_map();
    assert_eq!(map.sourced_walked_bytes, 76);
    assert!(map.ownership_is_coherent());
    let place_all = place_all(&map);
    let forest_facts = CheckPlayerForestFacts::from_admitted_replay_rules(&plan, false).unwrap();

    // Preview only the two stages needed to bind the post-nubify live-fact
    // checksum. The real transaction below starts again from the pristine map.
    let mut preview = map.clone();
    let preview_forest =
        execute_check_player_forest(&plan, &mut preview, &place_all, forest_facts).unwrap();
    let mut preview_host = AcceptingExactEdgeHost::default();
    let preview_nubify =
        execute_nubify_forest_frontier(&mut preview, &preview_forest, &mut preview_host).unwrap();
    let facts = transition_facts(&preview, preview_nubify.random_state_after);

    let mut edge_host = AcceptingExactEdgeHost::default();
    let receipt = plan
        .advance_map_make_terrain_repairs(
            &mut map,
            &place_all,
            forest_facts,
            &mut edge_host,
            &facts,
        )
        .unwrap();

    assert_eq!(receipt.ownership_transitions.len(), 3);
    assert!(receipt
        .ownership_transitions
        .iter()
        .flat_map(|transition| &transition.changed_ranges)
        .all(|range| range.section == WorldSection::WData));
    let changed: usize = receipt
        .ownership_transitions
        .iter()
        .map(|transition| transition.changed_bytes)
        .sum();
    assert!(changed > 0, "accepted nubify candidates must change WData");
    assert_eq!(map.sourced_walked_bytes, 76 + changed as u64);
    assert_eq!(
        receipt.post_nubify_transitions.sourced_walked_bytes,
        map.sourced_walked_bytes
    );
    assert!(map.ownership_is_coherent());
}

#[test]
fn late_failure_rolls_back_the_world_owner_ledger_with_the_world() {
    let mut plan = plan();
    plan.inputs.map_edge_world_cells = Some(40);
    let mut map = owned_map();
    let before = map.clone();
    let place_all = place_all(&map);
    let forest_facts = CheckPlayerForestFacts::from_admitted_replay_rules(&plan, false).unwrap();
    let facts = transition_facts(&map, expected_nubify_random_state().wrapping_add(1));
    let mut edge_host = AcceptingExactEdgeHost::default();

    assert!(matches!(
        plan.advance_map_make_terrain_repairs(
            &mut map,
            &place_all,
            forest_facts,
            &mut edge_host,
            &facts,
        ),
        Err(MapTerrainRepairError::PostNubifyTransitions(
            PostNubifyTransitionError::LiveFactsUnavailableOrMismatched
        ))
    ));
    assert_eq!(
        map.world.checksum_sections(),
        before.world.checksum_sections()
    );
    assert_eq!(map.checksum, before.checksum);
    assert_eq!(map.ownership, before.ownership);
    assert_eq!(map.sourced_walked_bytes, before.sourced_walked_bytes);
}

#[test]
fn stale_tileset_facts_roll_back_the_complete_replay_side_chain() {
    let plan = plan();
    let mut map = map();
    let place_all = place_all(&map);
    let world_before = map.world.clone();
    let checksum_before = map.checksum.clone();
    let regions_before = map.generation_regions.clone();
    let sourced_before = map.sourced_walked_bytes;
    let facts = transition_facts(&map, expected_nubify_random_state().wrapping_add(1));
    let forest_facts = CheckPlayerForestFacts::from_admitted_replay_rules(&plan, false).unwrap();
    let mut edge_host = RejectingExactEdgeHost::default();

    let error = plan
        .advance_map_make_terrain_repairs(
            &mut map,
            &place_all,
            forest_facts,
            &mut edge_host,
            &facts,
        )
        .unwrap_err();

    assert_eq!(
        error,
        MapTerrainRepairError::PostNubifyTransitions(
            PostNubifyTransitionError::LiveFactsUnavailableOrMismatched
        )
    );
    assert!(!edge_host.requests.is_empty());
    assert_eq!(map.world.wdata, world_before.wdata);
    assert_eq!(map.world.tdata, world_before.tdata);
    assert_eq!(map.world.start_city_locs, world_before.start_city_locs);
    assert_eq!(
        map.world.checksum_sections(),
        world_before.checksum_sections()
    );
    assert_eq!(map.checksum, checksum_before);
    assert_eq!(map.generation_regions, regions_before);
    assert_eq!(map.sourced_walked_bytes, sourced_before);
}
