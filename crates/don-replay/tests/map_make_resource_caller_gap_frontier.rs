// SPDX-License-Identifier: GPL-3.0-or-later
//! Mutation-sensitive source proof for the caller bridge into resource placement.

#[path = "../src/map_make_resource_caller_gap_frontier.rs"]
mod map_make_resource_caller_gap_frontier;

use don_sim::systems::map_terrain::World;
use map_make_resource_caller_gap_frontier::{
    execute_map_make_resource_caller_gap, MapMakeResourceCallerEvidence,
    MapMakeResourceCallerFacts, MapMakeResourceCallerGapError, MapMakeResourceCallerPriorReceipt,
    PlaceResourcesCallDisposition, ResourceGateRead, CALLER_GAP_CHECKPOINTS,
    CONQUEST_GAME_STYLE_OFFSET, CONQUEST_STYLE_NO_RARE_OFFSET, GAME_SEMAPHORE_BIT17_BYTE_OFFSET,
    GAME_SEMAPHORE_BIT9_BYTE_OFFSET, MAP_PLACE_RESOURCES_CALL_VA, MAP_PLACE_RESOURCES_ENTRY_VA,
    MAP_POST_RESOURCES_CHECKPOINT_CALL_VA, MAP_POST_RESOURCES_SOURCE_TOKEN,
    MAP_POST_TRANSITIONS_CHECKPOINT_CALL_VA, MAP_POST_TRANSITIONS_SOURCE_TOKEN,
    MAP_RESOURCE_CALLER_CONTINUATION_VA, MAP_RESOURCE_CALLER_GAP_RESUME_VA,
    MAP_RESOURCE_DIAGNOSTIC_CHECKPOINT_CALL_VA, MAP_RESOURCE_DIAGNOSTIC_SOURCE_TOKEN,
    MAP_RESOURCE_GATE_BEGIN_VA, RANDOM_GET_VA, SEMAPHORE_TEST_MASK, SHIPPED_EXE_SHA256,
    SHIPPED_PDB_SHA256,
};

fn prior() -> MapMakeResourceCallerPriorReceipt {
    MapMakeResourceCallerPriorReceipt {
        checkpoint_call_va: MAP_POST_TRANSITIONS_CHECKPOINT_CALL_VA,
        source_token: MAP_POST_TRANSITIONS_SOURCE_TOKEN,
        resume_va: MAP_RESOURCE_CALLER_GAP_RESUME_VA,
        map_object_identity: 0x1000,
        world_object_identity: 0x2000,
        game_object_identity: 0x3000,
        conquest_game_identity: 0x4000,
        map_state_sha256: [0x5a; 32],
        world_checksum: World::init_default_rules(8, 8).checksum_sections(),
        sourced_walked_bytes: 52,
        random_state: 0x1234_5678,
        player_count_argument: 4,
        progress_requested: true,
    }
}

fn facts(prior: &MapMakeResourceCallerPriorReceipt) -> MapMakeResourceCallerFacts {
    MapMakeResourceCallerFacts {
        game_semaphore_0x821: 0,
        game_semaphore_0x822: 0,
        conquest_style_no_rare: None,
        world_map_index_nonnegative: true,
        evidence: MapMakeResourceCallerEvidence::SyntheticFixture {
            fixture: "map-make-resource-caller-gap".to_owned(),
            map_object_identity: prior.map_object_identity,
            world_object_identity: prior.world_object_identity,
            game_object_identity: prior.game_object_identity,
            conquest_game_identity: prior.conquest_game_identity,
            map_state_sha256: prior.map_state_sha256,
            world_checksum: prior.world_checksum.clone(),
            sourced_walked_bytes: prior.sourced_walked_bytes,
            random_state: prior.random_state,
            player_count_argument: prior.player_count_argument,
            progress_requested: prior.progress_requested,
            game_semaphore_0x821: 0,
            game_semaphore_0x822: 0,
            conquest_style_no_rare: None,
            world_map_index_nonnegative: true,
        },
    }
}

fn rebind_evidence(
    facts: &mut MapMakeResourceCallerFacts,
    prior: &MapMakeResourceCallerPriorReceipt,
) {
    facts.evidence = MapMakeResourceCallerEvidence::SyntheticFixture {
        fixture: "map-make-resource-caller-gap".to_owned(),
        map_object_identity: prior.map_object_identity,
        world_object_identity: prior.world_object_identity,
        game_object_identity: prior.game_object_identity,
        conquest_game_identity: prior.conquest_game_identity,
        map_state_sha256: prior.map_state_sha256,
        world_checksum: prior.world_checksum.clone(),
        sourced_walked_bytes: prior.sourced_walked_bytes,
        random_state: prior.random_state,
        player_count_argument: prior.player_count_argument,
        progress_requested: prior.progress_requested,
        game_semaphore_0x821: facts.game_semaphore_0x821,
        game_semaphore_0x822: facts.game_semaphore_0x822,
        conquest_style_no_rare: facts.conquest_style_no_rare,
        world_map_index_nonnegative: facts.world_map_index_nonnegative,
    };
}

#[test]
fn ordinary_path_freezes_exact_checkpoints_and_place_resources_entry() {
    let prior = prior();
    let facts = facts(&prior);
    let receipt = execute_map_make_resource_caller_gap(&prior, &facts).unwrap();

    assert_eq!(receipt.resume_va, MAP_RESOURCE_CALLER_GAP_RESUME_VA);
    assert_eq!(receipt.checkpoints, CALLER_GAP_CHECKPOINTS);
    assert_eq!(receipt.gate_begin_va, MAP_RESOURCE_GATE_BEGIN_VA);
    assert_eq!(receipt.disposition, PlaceResourcesCallDisposition::Called);
    let entry = receipt.place_resources_entry.unwrap();
    assert_eq!(entry.upstream_checkpoint_call_va, 0x0068_c12a);
    assert_eq!(entry.upstream_source_token, 0x1ebe);
    assert_eq!(entry.latest_checkpoint_call_va, 0x0068_c1da);
    assert_eq!(entry.latest_source_token, 0x1ec8);
    assert_eq!(entry.call_va, MAP_PLACE_RESOURCES_CALL_VA);
    assert_eq!(entry.entry_va, MAP_PLACE_RESOURCES_ENTRY_VA);
    assert_eq!(entry.player_count_argument, 4);
    assert_eq!(entry.random_state_at_entry, prior.random_state);
    assert_eq!(entry.world_checksum, prior.world_checksum);
    assert_eq!(receipt.continuation_va, MAP_RESOURCE_CALLER_CONTINUATION_VA);
    assert_eq!(
        receipt.post_resources_checkpoint_call_va,
        MAP_POST_RESOURCES_CHECKPOINT_CALL_VA
    );
    assert_eq!(
        receipt.post_resources_source_token,
        MAP_POST_RESOURCES_SOURCE_TOKEN
    );
}

#[test]
fn game_semaphore_bit9_skips_lazily_before_ctw_or_style_reads() {
    let prior = prior();
    let mut facts = facts(&prior);
    facts.game_semaphore_0x821 = SEMAPHORE_TEST_MASK;
    facts.game_semaphore_0x822 = SEMAPHORE_TEST_MASK;
    facts.conquest_style_no_rare = Some(1);
    rebind_evidence(&mut facts, &prior);

    let receipt = execute_map_make_resource_caller_gap(&prior, &facts).unwrap();
    assert_eq!(
        receipt.disposition,
        PlaceResourcesCallDisposition::SkippedGameSemaphoreBit9
    );
    assert!(receipt.place_resources_entry.is_none());
    assert_eq!(
        receipt.gate_reads,
        vec![ResourceGateRead::GameSemaphoreBit9 {
            instruction_va: MAP_RESOURCE_GATE_BEGIN_VA,
            byte_offset: GAME_SEMAPHORE_BIT9_BYTE_OFFSET,
            mask: SEMAPHORE_TEST_MASK,
        }]
    );
}

#[test]
fn ctw_no_rare_skips_only_after_pointer_and_field_reads() {
    let prior = prior();
    let mut facts = facts(&prior);
    facts.game_semaphore_0x822 = SEMAPHORE_TEST_MASK;
    facts.conquest_style_no_rare = Some(-1);
    rebind_evidence(&mut facts, &prior);

    let receipt = execute_map_make_resource_caller_gap(&prior, &facts).unwrap();
    assert_eq!(
        receipt.disposition,
        PlaceResourcesCallDisposition::SkippedConquestNoRare
    );
    assert!(receipt.place_resources_entry.is_none());
    assert_eq!(receipt.gate_reads.len(), 4);
    assert_eq!(
        receipt.gate_reads[1],
        ResourceGateRead::GameSemaphoreBit17 {
            instruction_va: 0x0068_c6e6,
            byte_offset: GAME_SEMAPHORE_BIT17_BYTE_OFFSET,
            mask: SEMAPHORE_TEST_MASK,
        }
    );
    assert_eq!(
        receipt.gate_reads[2],
        ResourceGateRead::ConquestGameStylePointer {
            instruction_va: 0x0068_c6ef,
            offset: CONQUEST_GAME_STYLE_OFFSET,
        }
    );
    assert_eq!(
        receipt.gate_reads[3],
        ResourceGateRead::ConquestStyleNoRare {
            instruction_va: 0x0068_c6fe,
            offset: CONQUEST_STYLE_NO_RARE_OFFSET,
        }
    );
}

#[test]
fn ctw_null_style_or_zero_no_rare_still_calls_without_player_logs() {
    let prior = prior();
    let mut null_style = facts(&prior);
    null_style.game_semaphore_0x822 = SEMAPHORE_TEST_MASK;
    rebind_evidence(&mut null_style, &prior);
    let null_receipt = execute_map_make_resource_caller_gap(&prior, &null_style).unwrap();
    assert_eq!(
        null_receipt.disposition,
        PlaceResourcesCallDisposition::Called
    );
    assert_eq!(null_receipt.gate_reads.len(), 3);
    assert_eq!(null_receipt.player_diagnostic_iterations, 0);

    let mut zero_style = null_style;
    zero_style.conquest_style_no_rare = Some(0);
    rebind_evidence(&mut zero_style, &prior);
    let zero_receipt = execute_map_make_resource_caller_gap(&prior, &zero_style).unwrap();
    assert_eq!(
        zero_receipt.disposition,
        PlaceResourcesCallDisposition::Called
    );
    assert_eq!(zero_receipt.gate_reads.len(), 4);
    assert_eq!(zero_receipt.player_diagnostic_iterations, 0);
}

#[test]
fn diagnostics_and_progress_are_observations_not_rng_or_map_mutations() {
    let prior = prior();
    let mut facts = facts(&prior);
    let receipt = execute_map_make_resource_caller_gap(&prior, &facts).unwrap();
    assert_eq!(receipt.progress_refresh_calls, 2);
    assert_eq!(receipt.player_diagnostic_iterations, 4);
    assert_eq!(receipt.diagnostic_log_calls, 9);
    assert_eq!(receipt.random_get_va, RANDOM_GET_VA);
    assert_eq!(receipt.random_draws, 0);
    assert_eq!(receipt.random_state_before, receipt.random_state_after);
    assert_eq!(
        receipt.map_state_sha256_before,
        receipt.map_state_sha256_after
    );
    assert_eq!(receipt.world_checksum_before, receipt.world_checksum_after);
    assert_eq!(receipt.sourced_walked_bytes_before, 52);
    assert_eq!(receipt.sourced_walked_bytes_after, 52);

    let mut no_progress = prior.clone();
    no_progress.progress_requested = false;
    facts.world_map_index_nonnegative = false;
    rebind_evidence(&mut facts, &no_progress);
    let no_progress_receipt = execute_map_make_resource_caller_gap(&no_progress, &facts).unwrap();
    assert_eq!(no_progress_receipt.progress_refresh_calls, 0);
    assert_eq!(no_progress_receipt.diagnostic_log_calls, 8);
    assert_eq!(
        no_progress_receipt.random_state_after,
        receipt.random_state_after
    );
    assert_eq!(
        no_progress_receipt.world_checksum_after,
        receipt.world_checksum_after
    );
}

#[test]
fn nonpositive_player_count_does_not_enter_diagnostic_loop_but_is_forwarded() {
    let mut prior = prior();
    prior.player_count_argument = -3;
    let facts = facts(&prior);
    let receipt = execute_map_make_resource_caller_gap(&prior, &facts).unwrap();
    assert_eq!(receipt.player_diagnostic_iterations, 0);
    assert_eq!(receipt.diagnostic_log_calls, 5);
    assert_eq!(
        receipt.place_resources_entry.unwrap().player_count_argument,
        -3
    );
}

#[test]
fn stale_prior_or_fact_evidence_is_refused() {
    let prior = prior();
    let facts = facts(&prior);

    let mut bad_token = prior.clone();
    bad_token.source_token ^= 1;
    assert_eq!(
        execute_map_make_resource_caller_gap(&bad_token, &facts),
        Err(MapMakeResourceCallerGapError::PriorReceiptMismatch)
    );

    let mut empty_digest = prior.clone();
    empty_digest.map_state_sha256 = [0; 32];
    assert_eq!(
        execute_map_make_resource_caller_gap(&empty_digest, &facts),
        Err(MapMakeResourceCallerGapError::EmptyMapStateDigest)
    );

    let mut stale_facts = facts;
    if let MapMakeResourceCallerEvidence::SyntheticFixture {
        random_state: captured,
        ..
    } = &mut stale_facts.evidence
    {
        *captured ^= 1;
    }
    assert_eq!(
        execute_map_make_resource_caller_gap(&prior, &stale_facts),
        Err(MapMakeResourceCallerGapError::LiveFactsUnavailableOrMismatched)
    );
}

#[test]
fn retail_evidence_requires_shipped_binary_pdb_and_nonzero_capture() {
    let prior = prior();
    let mut facts = facts(&prior);
    facts.evidence = MapMakeResourceCallerEvidence::RetailCapture {
        executable_sha256: SHIPPED_EXE_SHA256.to_owned(),
        pdb_sha256: SHIPPED_PDB_SHA256.to_owned(),
        capture_sha256: [0xa5; 32],
        map_make_va: 0x0068_bc90,
        gap_resume_va: MAP_RESOURCE_CALLER_GAP_RESUME_VA,
        place_resources_call_va: MAP_PLACE_RESOURCES_CALL_VA,
        map_object_identity: prior.map_object_identity,
        world_object_identity: prior.world_object_identity,
        game_object_identity: prior.game_object_identity,
        conquest_game_identity: prior.conquest_game_identity,
        map_state_sha256: prior.map_state_sha256,
        world_checksum: prior.world_checksum.clone(),
        sourced_walked_bytes: prior.sourced_walked_bytes,
        random_state: prior.random_state,
        player_count_argument: prior.player_count_argument,
        progress_requested: prior.progress_requested,
        game_semaphore_0x821: 0,
        game_semaphore_0x822: 0,
        conquest_style_no_rare: None,
        world_map_index_nonnegative: true,
    };
    execute_map_make_resource_caller_gap(&prior, &facts).unwrap();

    if let MapMakeResourceCallerEvidence::RetailCapture { capture_sha256, .. } = &mut facts.evidence
    {
        *capture_sha256 = [0; 32];
    }
    assert_eq!(
        execute_map_make_resource_caller_gap(&prior, &facts),
        Err(MapMakeResourceCallerGapError::LiveFactsUnavailableOrMismatched)
    );
}

#[test]
fn every_resource_gate_axis_changes_only_disposition_and_read_chronology() {
    let prior = prior();
    let mut facts = facts(&prior);
    let baseline = execute_map_make_resource_caller_gap(&prior, &facts).unwrap();

    facts.game_semaphore_0x821 = SEMAPHORE_TEST_MASK;
    rebind_evidence(&mut facts, &prior);
    let bit9 = execute_map_make_resource_caller_gap(&prior, &facts).unwrap();
    assert_ne!(baseline.disposition, bit9.disposition);
    assert_ne!(baseline.gate_reads, bit9.gate_reads);

    facts.game_semaphore_0x821 = 0;
    facts.game_semaphore_0x822 = SEMAPHORE_TEST_MASK;
    facts.conquest_style_no_rare = Some(1);
    rebind_evidence(&mut facts, &prior);
    let no_rare = execute_map_make_resource_caller_gap(&prior, &facts).unwrap();
    assert_ne!(baseline.disposition, no_rare.disposition);
    assert_ne!(baseline.gate_reads, no_rare.gate_reads);

    for receipt in [&baseline, &bit9, &no_rare] {
        assert_eq!(receipt.random_state_after, prior.random_state);
        assert_eq!(receipt.map_state_sha256_after, prior.map_state_sha256);
        assert_eq!(receipt.world_checksum_after, prior.world_checksum);
        assert_eq!(
            receipt.checkpoints[1].call_va,
            MAP_RESOURCE_DIAGNOSTIC_CHECKPOINT_CALL_VA
        );
        assert_eq!(
            receipt.checkpoints[1].source_token,
            MAP_RESOURCE_DIAGNOSTIC_SOURCE_TOKEN
        );
    }
}
