// SPDX-License-Identifier: GPL-3.0-or-later
//! Focused executable proof for the post-nubify -> caller-gap -> resource-pool chain.

use don_replay::map_make_resource_caller_gap_frontier::{
    MapMakeResourceCallerEvidence, MapMakeResourceCallerFacts, PlaceResourcesCallDisposition,
    GAME_SEMAPHORE_BIT17_BYTE_OFFSET, GAME_SEMAPHORE_BIT9_BYTE_OFFSET, MAP_MAKE_VA,
    MAP_PLACE_RESOURCES_CALL_VA, MAP_PLACE_RESOURCES_ENTRY_VA,
    MAP_POST_RESOURCES_CHECKPOINT_CALL_VA, MAP_POST_RESOURCES_SOURCE_TOKEN,
    MAP_RESOURCE_CALLER_GAP_RESUME_VA, MAP_RESOURCE_DIAGNOSTIC_CHECKPOINT_CALL_VA,
    MAP_RESOURCE_DIAGNOSTIC_SOURCE_TOKEN, MAP_RESOURCE_PROGRESS_CHECKPOINT_CALL_VA,
    MAP_RESOURCE_PROGRESS_SOURCE_TOKEN, SEMAPHORE_TEST_MASK,
    SHIPPED_EXE_SHA256 as CALLER_EXE_SHA256, SHIPPED_PDB_SHA256 as CALLER_PDB_SHA256,
};
use don_replay::map_make_resource_schedule_integration::{
    execute_map_make_resource_schedule, execute_map_make_resource_schedule_with_xml,
    MapMakeResourceOwnerProvenance, MapMakeResourcePlacementReceipt, MapMakeResourceScheduleError,
};
use don_replay::map_style::MAP_MAKE_SCHEDULE;
use don_replay::nubify_forest_frontier::MAP_NUBIFY_FOREST_CALLER_RESUME_VA;
use don_replay::place_resources_pool_frontier::{
    resource_divvy_pool_digest, PlaceResourcesFactEvidence, PlaceResourcesLiveFacts,
    PlaceResourcesPoolError, RareGoodLiveFact, ResourceDivvyPoolState, ResourceGoodDisposition,
    ResourcePoolBitMask, FIRST_SCANNED_GOOD_ID, GAME_CONST_REFERENCE_VA, GOOD_TYPES_REFERENCE_VA,
    LANDS_OCEAN_RARE_LIST_POINTER_SLOT_VA, MAP_PLACE_RESOURCES_PREFIX_RESIDUAL_VA,
    SCANNED_GOOD_COUNT,
};
use don_replay::place_resources_xml_frontier::{
    BonusXmlRowFact, BonusesSectionSource, PlaceResourcesXmlError, PlaceResourcesXmlEvidence,
    PlaceResourcesXmlFacts, XmlHostHandles, XmlSectionFact, BONUSES_STRING_OFFSET,
    BONUS_STRING_OFFSET, DEFAULT_STYLE_STRING_OFFSET, INTERNAL_STRING_TABLE_VA,
    PLACE_RESOURCES_XML_RESIDUAL_VA, SHIPPED_EXE_SHA256 as XML_EXE_SHA256,
    SHIPPED_PDB_SHA256 as XML_PDB_SHA256, XML_EXTENSION_STRING_OFFSET,
};
use don_replay::post_nubify_transition_frontier::{
    PostNubifyTransitionReceipt, MAP_POST_NUBIFY_CHECKPOINT_CALL_VA,
    MAP_POST_NUBIFY_CHECKPOINT_END_VA, MAP_POST_NUBIFY_CHECKPOINT_RESUME_VA,
    MAP_POST_NUBIFY_SOURCE_TOKEN, MAP_POST_TRANSITIONS_CHECKPOINT_CALL_VA,
    MAP_POST_TRANSITIONS_SOURCE_TOKEN,
};
use don_sim::systems::map_terrain::World;

const RANDOM_STATE: i32 = 0x1234_5678;

fn post_nubify() -> PostNubifyTransitionReceipt {
    let checksum = World::init_default_rules(8, 8).checksum_sections();
    PostNubifyTransitionReceipt {
        caller_resume_va: MAP_NUBIFY_FOREST_CALLER_RESUME_VA,
        checkpoint_call_va: MAP_POST_NUBIFY_CHECKPOINT_CALL_VA,
        checkpoint_resume_va: MAP_POST_NUBIFY_CHECKPOINT_RESUME_VA,
        checkpoint_end_va: MAP_POST_NUBIFY_CHECKPOINT_END_VA,
        source_token: MAP_POST_NUBIFY_SOURCE_TOKEN,
        calls: Vec::new(),
        random_state_before: RANDOM_STATE,
        random_state_after: RANDOM_STATE,
        random_draws: Vec::new(),
        writes: Vec::new(),
        byte_changing_writes: 0,
        checksum_before: checksum.clone(),
        checksum_after: checksum,
        next_checkpoint_call_va: MAP_POST_TRANSITIONS_CHECKPOINT_CALL_VA,
        next_source_token: MAP_POST_TRANSITIONS_SOURCE_TOKEN,
        sourced_walked_bytes: 52,
    }
}

fn owner() -> MapMakeResourceOwnerProvenance {
    MapMakeResourceOwnerProvenance {
        map_object_identity: 0x1000,
        world_object_identity: 0x2000,
        game_object_identity: 0x3000,
        conquest_game_identity: 0x4000,
        map_state_sha256_at_0x1ebe: [0x5a; 32],
        player_count_argument: 4,
        progress_requested: true,
    }
}

fn caller_facts(
    post: &PostNubifyTransitionReceipt,
    owner: &MapMakeResourceOwnerProvenance,
    game_semaphore_0x821: u8,
) -> MapMakeResourceCallerFacts {
    MapMakeResourceCallerFacts {
        game_semaphore_0x821,
        game_semaphore_0x822: 0,
        conquest_style_no_rare: None,
        world_map_index_nonnegative: true,
        evidence: MapMakeResourceCallerEvidence::RetailCapture {
            executable_sha256: CALLER_EXE_SHA256.to_owned(),
            pdb_sha256: CALLER_PDB_SHA256.to_owned(),
            capture_sha256: [0xa5; 32],
            map_make_va: MAP_MAKE_VA,
            gap_resume_va: MAP_RESOURCE_CALLER_GAP_RESUME_VA,
            place_resources_call_va: MAP_PLACE_RESOURCES_CALL_VA,
            map_object_identity: owner.map_object_identity,
            world_object_identity: owner.world_object_identity,
            game_object_identity: owner.game_object_identity,
            conquest_game_identity: owner.conquest_game_identity,
            map_state_sha256: owner.map_state_sha256_at_0x1ebe,
            world_checksum: post.checksum_after.clone(),
            sourced_walked_bytes: post.sourced_walked_bytes,
            random_state: post.random_state_after,
            player_count_argument: owner.player_count_argument,
            progress_requested: owner.progress_requested,
            game_semaphore_0x821,
            game_semaphore_0x822: 0,
            conquest_style_no_rare: None,
            world_map_index_nonnegative: true,
        },
    }
}

fn resource_facts(post: &PostNubifyTransitionReceipt) -> PlaceResourcesLiveFacts {
    let mut goods = (0..SCANNED_GOOD_COUNT)
        .map(|ordinal| RareGoodLiveFact {
            good_id: FIRST_SCANNED_GOOD_ID + ordinal as i32,
            resolved_age: 0,
            exclude_ctw: 0,
            random_rare_drop: 0,
        })
        .collect::<Vec<_>>();
    goods[0].random_rare_drop = 1;
    goods[1].random_rare_drop = 1;
    goods[1].resolved_age = 3;
    PlaceResourcesLiveFacts {
        conquer_the_world: false,
        goods,
        ocean_rare_good_ids: vec![FIRST_SCANNED_GOOD_ID],
        evidence: PlaceResourcesFactEvidence::RetailCapture {
            executable_sha256: CALLER_EXE_SHA256.to_owned(),
            pdb_sha256: CALLER_PDB_SHA256.to_owned(),
            capture_sha256: [0xb6; 32],
            game_const_reference_va: GAME_CONST_REFERENCE_VA,
            good_types_reference_va: GOOD_TYPES_REFERENCE_VA,
            lands_ocean_rare_list_pointer_slot_va: LANDS_OCEAN_RARE_LIST_POINTER_SLOT_VA,
            checkpoint_call_va: MAP_POST_TRANSITIONS_CHECKPOINT_CALL_VA,
            source_token: MAP_POST_TRANSITIONS_SOURCE_TOKEN,
            call_va: MAP_PLACE_RESOURCES_CALL_VA,
            entry_va: MAP_PLACE_RESOURCES_ENTRY_VA,
            world_checksum: post.checksum_after.clone(),
            sourced_walked_bytes: post.sourced_walked_bytes,
            random_state_at_entry: post.random_state_after,
        },
    }
}

fn expected_resource_pool() -> ResourceDivvyPoolState {
    ResourceDivvyPoolState {
        early_bits: ResourcePoolBitMask {
            bits: 0,
            bytes: Vec::new(),
        },
        early_goods: Vec::new(),
        late_bits: ResourcePoolBitMask {
            bits: 1,
            bytes: vec![0],
        },
        late_goods: vec![FIRST_SCANNED_GOOD_ID + 1],
        water_bits: ResourcePoolBitMask {
            bits: 10,
            bytes: vec![0, 0],
        },
        water_goods: vec![FIRST_SCANNED_GOOD_ID; 10],
    }
}

fn xml_facts(
    post: &PostNubifyTransitionReceipt,
    resource_pool_digest: u64,
) -> PlaceResourcesXmlFacts {
    let section = XmlSectionFact {
        element_name: "BONUSES".to_owned(),
        handles: XmlHostHandles {
            head: true,
            tail: true,
            inline_tail_word: false,
        },
        bonus_rows: vec![
            BonusXmlRowFact {
                capture_ordinal: 7,
                element_name: "BONUS".to_owned(),
                handles: XmlHostHandles {
                    head: true,
                    tail: false,
                    inline_tail_word: true,
                },
            },
            BonusXmlRowFact {
                capture_ordinal: 3,
                element_name: "BONUS".to_owned(),
                handles: XmlHostHandles {
                    head: false,
                    tail: true,
                    inline_tail_word: false,
                },
            },
        ],
    };
    PlaceResourcesXmlFacts {
        selected_style_name: "eastwest.xml".to_owned(),
        selected_bonuses: Some(section),
        default_bonuses: None,
        evidence: PlaceResourcesXmlEvidence::RetailCapture {
            executable_sha256: XML_EXE_SHA256.to_owned(),
            pdb_sha256: XML_PDB_SHA256.to_owned(),
            capture_sha256: [0xc7; 32],
            internal_string_table_va: INTERNAL_STRING_TABLE_VA,
            default_style_string_offset: DEFAULT_STYLE_STRING_OFFSET,
            xml_extension_string_offset: XML_EXTENSION_STRING_OFFSET,
            bonuses_string_offset: BONUSES_STRING_OFFSET,
            bonus_string_offset: BONUS_STRING_OFFSET,
            entry_va: MAP_PLACE_RESOURCES_PREFIX_RESIDUAL_VA,
            random_state: post.random_state_after,
            world_checksum: post.checksum_after.clone(),
            sourced_walked_bytes: post.sourced_walked_bytes,
            resource_pool_digest,
        },
    }
}

#[test]
fn schedule_and_receipts_preserve_checkpoint_rng_world_and_map_identity() {
    let schedule_names = MAP_MAKE_SCHEDULE
        .iter()
        .map(|stage| stage.name)
        .collect::<Vec<_>>();
    assert_eq!(
        &schedule_names[10..13],
        [
            "post_nubify_transitions",
            "resource_caller_gap",
            "place_resources"
        ]
    );
    let resource_gap = &MAP_MAKE_SCHEDULE[11];
    assert_eq!(
        resource_gap.evidence_va,
        Some(MAP_RESOURCE_CALLER_GAP_RESUME_VA)
    );
    assert_eq!(resource_gap.checkpoint, None);

    let post = post_nubify();
    let owner = owner();
    let caller = caller_facts(&post, &owner, 0);
    let resources = resource_facts(&post);
    let mut pool = ResourceDivvyPoolState::default();
    let receipt =
        execute_map_make_resource_schedule(&mut pool, &post, &owner, &caller, Some(&resources))
            .unwrap();

    assert_eq!(
        receipt.post_nubify_checkpoint.checkpoint_call_va,
        0x0068_c12a
    );
    assert_eq!(receipt.post_nubify_checkpoint.source_token, 0x1ebe);
    assert_eq!(receipt.post_nubify_checkpoint.random_state, RANDOM_STATE);
    assert_eq!(receipt.caller_gap.random_draws, 0);
    assert_eq!(
        receipt.caller_gap.checkpoints[0].call_va,
        MAP_RESOURCE_PROGRESS_CHECKPOINT_CALL_VA
    );
    assert_eq!(
        receipt.caller_gap.checkpoints[0].source_token,
        MAP_RESOURCE_PROGRESS_SOURCE_TOKEN
    );
    assert_eq!(
        receipt.caller_gap.checkpoints[1].call_va,
        MAP_RESOURCE_DIAGNOSTIC_CHECKPOINT_CALL_VA
    );
    assert_eq!(
        receipt.caller_gap.checkpoints[1].source_token,
        MAP_RESOURCE_DIAGNOSTIC_SOURCE_TOKEN
    );
    assert_eq!(receipt.caller_gap.world_checksum_after, post.checksum_after);

    let MapMakeResourcePlacementReceipt::BodyOpen(boundary) = receipt.placement else {
        panic!("admitted pool facts must advance to the body boundary");
    };
    assert_eq!(boundary.residual_va, MAP_PLACE_RESOURCES_PREFIX_RESIDUAL_VA);
    assert_eq!(
        boundary.pending_checkpoint_call_va,
        MAP_POST_RESOURCES_CHECKPOINT_CALL_VA
    );
    assert_eq!(
        boundary.pending_source_token,
        MAP_POST_RESOURCES_SOURCE_TOKEN
    );
    assert_eq!(boundary.map_state_sha256_at_entry, [0x5a; 32]);
    assert_eq!(boundary.random_state, RANDOM_STATE);
    assert_eq!(boundary.world_checksum, post.checksum_after);
    assert_eq!(boundary.pool_prefix.random_draws, 0);
    assert_eq!(
        boundary.pool_prefix.random_state_before,
        boundary.pool_prefix.random_state_after
    );
    assert_eq!(
        boundary.pool_prefix.world_checksum_before,
        boundary.pool_prefix.world_checksum_after
    );
    assert_eq!(
        boundary.pool_prefix.decisions[0].disposition,
        ResourceGoodDisposition::Water { copies: 10 }
    );
    assert_eq!(
        boundary.pool_prefix.decisions[1].disposition,
        ResourceGoodDisposition::Late
    );
    assert_eq!(pool, boundary.pool_prefix.pool_after);
}

#[test]
fn missing_pool_facts_stop_at_the_exact_entry_without_mutation() {
    let post = post_nubify();
    let owner = owner();
    let caller = caller_facts(&post, &owner, 0);
    let mut pool = ResourceDivvyPoolState::default();
    pool.early_goods.push(99);
    let before = pool.clone();

    let receipt =
        execute_map_make_resource_schedule(&mut pool, &post, &owner, &caller, None).unwrap();
    let MapMakeResourcePlacementReceipt::EntryOpen(boundary) = receipt.placement else {
        panic!("missing facts must remain a typed entry boundary");
    };
    assert_eq!(boundary.entry.call_va, MAP_PLACE_RESOURCES_CALL_VA);
    assert_eq!(boundary.entry.entry_va, MAP_PLACE_RESOURCES_ENTRY_VA);
    assert_eq!(boundary.entry.random_state_at_entry, RANDOM_STATE);
    assert_eq!(pool, before);
}

#[test]
fn skipped_gate_never_touches_the_pool_or_claims_the_callee_entry() {
    let post = post_nubify();
    let owner = owner();
    let caller = caller_facts(&post, &owner, SEMAPHORE_TEST_MASK);
    let resources = resource_facts(&post);
    let mut pool = ResourceDivvyPoolState::default();
    pool.water_goods.push(77);
    let before = pool.clone();

    let receipt =
        execute_map_make_resource_schedule(&mut pool, &post, &owner, &caller, Some(&resources))
            .unwrap();
    let MapMakeResourcePlacementReceipt::Skipped(boundary) = receipt.placement else {
        panic!("bit 9 must stop on the caller continuation");
    };
    assert_eq!(
        boundary.disposition,
        PlaceResourcesCallDisposition::SkippedGameSemaphoreBit9
    );
    assert_eq!(receipt.caller_gap.gate_reads.len(), 1);
    assert_eq!(GAME_SEMAPHORE_BIT9_BYTE_OFFSET, 0x821);
    assert_eq!(GAME_SEMAPHORE_BIT17_BYTE_OFFSET, 0x822);
    assert_eq!(boundary.random_state, RANDOM_STATE);
    assert_eq!(pool, before);
}

#[test]
fn stale_post_or_pool_provenance_fails_before_committing_pool_state() {
    let mut stale_post = post_nubify();
    stale_post.next_source_token ^= 1;
    let owner = owner();
    let caller = caller_facts(&stale_post, &owner, 0);
    let mut pool = ResourceDivvyPoolState::default();
    pool.late_goods.push(88);
    let before = pool.clone();
    assert_eq!(
        execute_map_make_resource_schedule(&mut pool, &stale_post, &owner, &caller, None),
        Err(MapMakeResourceScheduleError::PostNubifyReceiptMismatch)
    );
    assert_eq!(pool, before);

    let post = post_nubify();
    let caller = caller_facts(&post, &owner, 0);
    let mut resources = resource_facts(&post);
    if let PlaceResourcesFactEvidence::RetailCapture {
        random_state_at_entry,
        ..
    } = &mut resources.evidence
    {
        *random_state_at_entry ^= 1;
    }
    assert_eq!(
        execute_map_make_resource_schedule(&mut pool, &post, &owner, &caller, Some(&resources)),
        Err(MapMakeResourceScheduleError::PoolPrefix(
            PlaceResourcesPoolError::LiveFactsUnavailableOrMismatched
        ))
    );
    assert_eq!(pool, before);
}

#[test]
fn typed_xml_owner_advances_schedule_to_exact_row_boundary() {
    let place_resources_stage = &MAP_MAKE_SCHEDULE[12];
    assert_eq!(place_resources_stage.name, "place_resources");
    assert!(place_resources_stage.rng.contains("0x0068fb9d"));

    let post = post_nubify();
    let owner = owner();
    let caller = caller_facts(&post, &owner, 0);
    let resources = resource_facts(&post);
    let expected_pool = expected_resource_pool();
    let expected_digest = resource_divvy_pool_digest(&expected_pool);
    let xml = xml_facts(&post, expected_digest);
    let mut pool = ResourceDivvyPoolState::default();
    let mut host = XmlHostHandles::default();

    let receipt = execute_map_make_resource_schedule_with_xml(
        &mut pool,
        &mut host,
        &post,
        &owner,
        &caller,
        Some(&resources),
        Some(&xml),
    )
    .unwrap();
    let MapMakeResourcePlacementReceipt::XmlRowsOpen(boundary) = receipt.placement else {
        panic!("admitted XML facts must advance to the ordered-row boundary");
    };

    assert_eq!(boundary.residual_va, PLACE_RESOURCES_XML_RESIDUAL_VA);
    assert_eq!(
        boundary.pending_checkpoint_call_va,
        MAP_POST_RESOURCES_CHECKPOINT_CALL_VA
    );
    assert_eq!(
        boundary.pending_source_token,
        MAP_POST_RESOURCES_SOURCE_TOKEN
    );
    assert_eq!(boundary.map_object_identity, owner.map_object_identity);
    assert_eq!(boundary.world_object_identity, owner.world_object_identity);
    assert_eq!(boundary.map_state_sha256_at_entry, [0x5a; 32]);
    assert_eq!(
        boundary.pool_prefix.prefix_residual_va,
        MAP_PLACE_RESOURCES_PREFIX_RESIDUAL_VA
    );
    assert_eq!(
        boundary.xml_entry.entry_va,
        MAP_PLACE_RESOURCES_PREFIX_RESIDUAL_VA
    );
    assert_eq!(boundary.xml_entry.resource_pool_digest, expected_digest);
    assert_eq!(boundary.xml_frontier.receipt.evidence, xml.evidence);
    assert_eq!(boundary.xml_frontier.receipt.random_draws, 0);
    assert_eq!(
        boundary.xml_frontier.receipt.random_state_after,
        RANDOM_STATE
    );
    assert_eq!(boundary.xml_frontier.receipt.world_mutations, 0);
    assert_eq!(
        boundary.xml_frontier.receipt.world_checksum_after,
        post.checksum_after
    );
    assert_eq!(boundary.xml_frontier.receipt.sourced_walked_bytes_after, 52);
    assert_eq!(
        boundary.xml_frontier.receipt.section_source,
        BonusesSectionSource::SelectedStyle
    );
    assert_eq!(
        boundary
            .xml_frontier
            .handoff
            .rows
            .iter()
            .map(|row| row.capture_ordinal)
            .collect::<Vec<_>>(),
        vec![7, 3]
    );
    assert_eq!(boundary.xml_frontier.handoff.current_category_handles, host);
    assert_eq!(pool, expected_pool);
}

#[test]
fn stale_xml_evidence_rolls_back_pool_and_host_as_one_transaction() {
    let post = post_nubify();
    let owner = owner();
    let caller = caller_facts(&post, &owner, 0);
    let resources = resource_facts(&post);
    let mut xml = xml_facts(&post, resource_divvy_pool_digest(&expected_resource_pool()));
    if let PlaceResourcesXmlEvidence::RetailCapture { random_state, .. } = &mut xml.evidence {
        *random_state ^= 1;
    }
    let mut pool = ResourceDivvyPoolState::default();
    let pool_before = pool.clone();
    let mut host = XmlHostHandles {
        head: true,
        tail: false,
        inline_tail_word: true,
    };
    let host_before = host;

    assert_eq!(
        execute_map_make_resource_schedule_with_xml(
            &mut pool,
            &mut host,
            &post,
            &owner,
            &caller,
            Some(&resources),
            Some(&xml),
        ),
        Err(MapMakeResourceScheduleError::XmlFrontier(
            PlaceResourcesXmlError::StaleEvidence
        ))
    );
    assert_eq!(pool, pool_before);
    assert_eq!(host, host_before);
}

#[test]
fn xml_facts_cannot_bypass_the_pool_owner() {
    let post = post_nubify();
    let owner = owner();
    let caller = caller_facts(&post, &owner, 0);
    let xml = xml_facts(&post, 1);
    let mut pool = ResourceDivvyPoolState::default();
    let mut host = XmlHostHandles::default();

    assert_eq!(
        execute_map_make_resource_schedule_with_xml(
            &mut pool,
            &mut host,
            &post,
            &owner,
            &caller,
            None,
            Some(&xml),
        ),
        Err(MapMakeResourceScheduleError::XmlFactsWithoutPoolPrefix)
    );
}
