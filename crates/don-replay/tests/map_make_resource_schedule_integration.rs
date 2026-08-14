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
    continue_map_make_resource_schedule_bonus_category_tail,
    continue_map_make_resource_schedule_empty_fish,
    continue_map_make_resource_schedule_first_bonus,
    continue_map_make_resource_schedule_first_fish,
    continue_map_make_resource_schedule_fish_category,
    continue_map_make_resource_schedule_fish_recurrence,
    continue_map_make_resource_schedule_next_bonus, execute_map_make_resource_schedule,
    execute_map_make_resource_schedule_with_xml, MapMakeResourceOwnerProvenance,
    MapMakeResourcePlacementReceipt, MapMakeResourceScheduleError, MapMakeResourceScheduleReceipt,
    PlaceResourcesDocumentHostAuthority, BONUS_CATEGORY_TAIL_RESTORE_VA,
};
use don_replay::map_style::MAP_MAKE_SCHEDULE;
use don_replay::nubify_forest_frontier::MAP_NUBIFY_FOREST_CALLER_RESUME_VA;
use don_replay::place_player_resource_body_frontier::{
    PlayerAllocationHost, PlayerAllocationReceipt, PlayerAllocationRequest,
};
use don_replay::place_region_resource_world_body_frontier::{
    RegionInitGoodHost, RegionInitGoodReceipt, RegionInitGoodRequest,
};
use don_replay::place_resources_bonus_mutation_frontier::{
    BonusMutationEvidence, CalleeRandomDraw, FirstBonusDisposition, FirstBonusMutationError,
    FirstBonusMutationFacts, PlaceResourcesBonusMutationState, PlacementEvidence, PlacementHost,
    PlacementPattern, PlacementReceipt, PlacementRequest, ResourceTypeResolution, ScaledAttribute,
    ScaledAttributeFact, CENTER_KEEP_AWAY_SCALE_CALL_VA, CENTER_STAY_NEAR_SCALE_CALL_VA,
    CORNER_KEEP_AWAY_SCALE_CALL_VA, CORNER_STAY_NEAR_SCALE_CALL_VA, EDGE_KEEP_AWAY_SCALE_CALL_VA,
    EDGE_STAY_NEAR_SCALE_CALL_VA, FIRST_BONUS_ROW_BODY_VA, GROUP_SPACING_SCALE_CALL_VA,
    NUM_RARE_SCALE_CALL_VA, PLAYER_KEEP_AWAY_SCALE_CALL_VA, PLAYER_STAY_NEAR_SCALE_CALL_VA,
    SELECTOR_THREE_IGNORE_CALL_VA, SHIPPED_EXE_SHA256 as BONUS_EXE_SHA256,
    SHIPPED_PDB_SHA256 as BONUS_PDB_SHA256,
};
use don_replay::place_resources_bonus_rows_mutation_frontier::{
    later_bonus_facts_digest, LaterBonusMutationEvidence, LaterBonusMutationFacts,
    RemainingBonusRowsError, RemainingBonusRowsState, BONUS_CATEGORY_TAIL_VA,
    CARRIED_CATEGORY_FIRST_ENTRY_VA, LATER_BONUS_ENTRY_VA, LATER_ROW_MUTATION_ENTRY_VA,
};
use don_replay::place_resources_canonical_transaction::{
    CanonicalPlaceResourcesState, CanonicalRowFacts, CanonicalRowMutationFacts,
    CanonicalRowMutationReceipt,
};
use don_replay::place_resources_category_frontier::{
    CategoryAdvanceDisposition, CategoryAdvanceFacts, CategoryRowFact, CategorySectionFact,
    HostHandles, HostRefKind, ResourceCategory, ResourceFrontierEvidence, SectionSource,
    CATEGORY_RELEASE_CALL_VAS, CATEGORY_TAIL_VA, ROW_BODY_VA, ROW_COUNT_TEST_VA,
    ROW_ENUMERATE_CALL_VA, SHIPPED_EXE_SHA256 as CATEGORY_EXE_SHA256,
    SHIPPED_PDB_SHA256 as CATEGORY_PDB_SHA256,
};
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
use don_replay::resource_divvy_pool_selection_frontier::{
    execute_resource_pool_selection, ResourcePoolLane,
};
use don_sim::{rng::Random, systems::map_terrain::World};

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

struct NoPlacementExpected;

impl PlacementHost for NoPlacementExpected {
    fn place(&mut self, _request: &PlacementRequest) -> Option<PlacementReceipt> {
        panic!("chance-miss integration must not enter an opaque placement body")
    }
}

struct ExactSelectorHost;

impl PlacementHost for ExactSelectorHost {
    fn place(&mut self, request: &PlacementRequest) -> Option<PlacementReceipt> {
        let mut pool = request.resource_pool_before.clone()?;
        let mut random = Random::new(request.random_state_before);
        let selection =
            execute_resource_pool_selection(&mut pool, ResourcePoolLane::Late, &mut random).ok()?;
        let random_draws = selection
            .random_draws
            .iter()
            .map(|draw| CalleeRandomDraw {
                call_va: draw.call_va,
                random_get_va: draw.random_get_va,
                low: draw.low,
                high: draw.high,
                state_before: draw.state_before,
                raw: draw.raw,
                state_after: draw.state_after,
            })
            .collect();
        Some(PlacementReceipt {
            request: request.clone(),
            random_draws,
            random_state_after: random.state(),
            allocations: Vec::new(),
            allocated_count: 0,
            world_checksum_after: request.world_checksum_before.clone(),
            sourced_walked_bytes_after: request.sourced_walked_bytes,
            resource_pool_digest_after: resource_divvy_pool_digest(&pool),
            resource_pool_selections: vec![selection],
            resource_pool_after: Some(pool),
            evidence: PlacementEvidence::RetailCapture {
                executable_sha256: BONUS_EXE_SHA256.to_owned(),
                capture_sha256: [0xe9; 32],
            },
        })
    }
}

struct CorruptSelectorHost;

impl PlacementHost for CorruptSelectorHost {
    fn place(&mut self, request: &PlacementRequest) -> Option<PlacementReceipt> {
        let mut exact = ExactSelectorHost;
        let mut receipt = exact.place(request)?;
        receipt.resource_pool_selections[0]
            .pool_after
            .late_bits
            .bytes[0] ^= 1;
        Some(receipt)
    }
}

fn selector_scaled_facts() -> Vec<ScaledAttributeFact> {
    [
        (ScaledAttribute::NumRare, NUM_RARE_SCALE_CALL_VA, 1),
        (
            ScaledAttribute::GroupSpacing,
            GROUP_SPACING_SCALE_CALL_VA,
            0,
        ),
        (
            ScaledAttribute::PlayerKeepAway,
            PLAYER_KEEP_AWAY_SCALE_CALL_VA,
            1,
        ),
        (
            ScaledAttribute::PlayerStayNear,
            PLAYER_STAY_NEAR_SCALE_CALL_VA,
            1,
        ),
        (
            ScaledAttribute::CenterKeepAway,
            CENTER_KEEP_AWAY_SCALE_CALL_VA,
            0,
        ),
        (
            ScaledAttribute::CenterStayNear,
            CENTER_STAY_NEAR_SCALE_CALL_VA,
            0,
        ),
        (
            ScaledAttribute::CornerKeepAway,
            CORNER_KEEP_AWAY_SCALE_CALL_VA,
            0,
        ),
        (
            ScaledAttribute::CornerStayNear,
            CORNER_STAY_NEAR_SCALE_CALL_VA,
            0,
        ),
        (
            ScaledAttribute::EdgeKeepAway,
            EDGE_KEEP_AWAY_SCALE_CALL_VA,
            0,
        ),
        (
            ScaledAttribute::EdgeStayNear,
            EDGE_STAY_NEAR_SCALE_CALL_VA,
            0,
        ),
    ]
    .into_iter()
    .map(|(attribute, call_va, scaled)| ScaledAttributeFact {
        attribute,
        call_va,
        expression: scaled.to_string(),
        scaled,
    })
    .collect()
}

fn selector_bonus_facts(
    handoff: &don_replay::place_resources_xml_frontier::PlaceResourcesBonusRowsHandoff,
    chance_group: i32,
) -> FirstBonusMutationFacts {
    let row = &handoff.rows[0];
    FirstBonusMutationFacts {
        capture_ordinal: row.capture_ordinal,
        type_name: "Late".to_owned(),
        type_resolution: ResourceTypeResolution::PoolSelector {
            selector: 3,
            matched_call_va: SELECTOR_THREE_IGNORE_CALL_VA,
        },
        chance: 100,
        chance_group,
        pattern_name: "player".to_owned(),
        pattern: PlacementPattern::Player,
        saturate: 0,
        spacing: 0,
        scaled: selector_scaled_facts(),
        evidence: BonusMutationEvidence::RetailCapture {
            executable_sha256: BONUS_EXE_SHA256.to_owned(),
            pdb_sha256: BONUS_PDB_SHA256.to_owned(),
            capture_sha256: [0xfa; 32],
            entry_va: handoff.resume_va,
            capture_ordinal: row.capture_ordinal,
            random_state: handoff.random_state,
            world_checksum: handoff.world_checksum.clone(),
            sourced_walked_bytes: handoff.sourced_walked_bytes,
            resource_pool_digest: handoff.resource_pool_digest,
        },
    }
}

fn first_chance_miss_facts(
    handoff: &don_replay::place_resources_xml_frontier::PlaceResourcesBonusRowsHandoff,
) -> FirstBonusMutationFacts {
    let row = &handoff.rows[0];
    FirstBonusMutationFacts {
        capture_ordinal: row.capture_ordinal,
        type_name: "Salt".to_owned(),
        type_resolution: ResourceTypeResolution::CatalogGood { good_id: 6 },
        chance: 0,
        chance_group: 0,
        pattern_name: "player".to_owned(),
        pattern: PlacementPattern::Player,
        saturate: 0,
        spacing: 0,
        scaled: Vec::new(),
        evidence: BonusMutationEvidence::RetailCapture {
            executable_sha256: BONUS_EXE_SHA256.to_owned(),
            pdb_sha256: BONUS_PDB_SHA256.to_owned(),
            capture_sha256: [0xd8; 32],
            entry_va: handoff.resume_va,
            capture_ordinal: row.capture_ordinal,
            random_state: handoff.random_state,
            world_checksum: handoff.world_checksum.clone(),
            sourced_walked_bytes: handoff.sourced_walked_bytes,
            resource_pool_digest: handoff.resource_pool_digest,
        },
    }
}

fn remaining_state_from_first(
    first: &don_replay::map_make_resource_schedule_integration::PlaceResourcesFirstBonusBoundary,
) -> RemainingBonusRowsState {
    let receipt = &first.first_bonus;
    let mutation = PlaceResourcesBonusMutationState {
        random_state: receipt.random_state_after,
        world_checksum: receipt.world_checksum_after.clone(),
        sourced_walked_bytes: receipt.sourced_walked_bytes,
        resource_pool_digest: receipt.resource_pool_digest_after,
        resource_pool: Some(first.resource_pool_after.clone()),
        allocated_resources: receipt.allocated_resources_after,
        requested_resources: receipt.requested_resources_after,
    };
    RemainingBonusRowsState::from_first_row(
        &first.xml.xml_frontier.handoff,
        &receipt.facts,
        receipt,
        &mutation,
    )
    .unwrap()
}

fn later_chance_miss_facts(
    state: &RemainingBonusRowsState,
    handoff: &don_replay::place_resources_xml_frontier::PlaceResourcesBonusRowsHandoff,
) -> LaterBonusMutationFacts {
    let row = &handoff.rows[state.next_row_index];
    let mut facts = LaterBonusMutationFacts {
        capture_ordinal: row.capture_ordinal,
        type_name: "Salt".to_owned(),
        type_resolution: ResourceTypeResolution::CatalogGood { good_id: 6 },
        chance: 0,
        chance_group: 0,
        pattern_name: "player".to_owned(),
        pattern: PlacementPattern::Player,
        saturate: 0,
        spacing: 0,
        scaled: Vec::new(),
        evidence: LaterBonusMutationEvidence::RetailCapture {
            executable_sha256: BONUS_EXE_SHA256.to_owned(),
            pdb_sha256: BONUS_PDB_SHA256.to_owned(),
            capture_sha256: [0xab; 32],
            entry_va: LATER_BONUS_ENTRY_VA,
            row_body_va: FIRST_BONUS_ROW_BODY_VA,
            mutation_entry_va: LATER_ROW_MUTATION_ENTRY_VA,
            row_index: state.next_row_index,
            capture_ordinal: row.capture_ordinal,
            random_state: state.mutation.random_state,
            world_checksum: state.mutation.world_checksum.clone(),
            sourced_walked_bytes: state.mutation.sourced_walked_bytes,
            resource_pool_digest: state.mutation.resource_pool_digest,
            last_chance_group: state.last_chance_group,
            signed_chance_budget: state.signed_chance_budget,
            winner_seen: state.winner_seen,
            facts_digest: 0,
        },
    };
    let digest = later_bonus_facts_digest(&facts);
    if let LaterBonusMutationEvidence::RetailCapture { facts_digest, .. } = &mut facts.evidence {
        *facts_digest = digest;
    }
    facts
}

fn later_selector_facts(
    state: &RemainingBonusRowsState,
    handoff: &don_replay::place_resources_xml_frontier::PlaceResourcesBonusRowsHandoff,
) -> LaterBonusMutationFacts {
    let row = &handoff.rows[state.next_row_index];
    let mut facts = LaterBonusMutationFacts {
        capture_ordinal: row.capture_ordinal,
        type_name: "Late".to_owned(),
        type_resolution: ResourceTypeResolution::PoolSelector {
            selector: 3,
            matched_call_va: SELECTOR_THREE_IGNORE_CALL_VA,
        },
        chance: 100,
        chance_group: 0,
        pattern_name: "player".to_owned(),
        pattern: PlacementPattern::Player,
        saturate: 0,
        spacing: 0,
        scaled: selector_scaled_facts(),
        evidence: LaterBonusMutationEvidence::RetailCapture {
            executable_sha256: BONUS_EXE_SHA256.to_owned(),
            pdb_sha256: BONUS_PDB_SHA256.to_owned(),
            capture_sha256: [0xbc; 32],
            entry_va: LATER_BONUS_ENTRY_VA,
            row_body_va: FIRST_BONUS_ROW_BODY_VA,
            mutation_entry_va: LATER_ROW_MUTATION_ENTRY_VA,
            row_index: state.next_row_index,
            capture_ordinal: row.capture_ordinal,
            random_state: state.mutation.random_state,
            world_checksum: state.mutation.world_checksum.clone(),
            sourced_walked_bytes: state.mutation.sourced_walked_bytes,
            resource_pool_digest: state.mutation.resource_pool_digest,
            last_chance_group: state.last_chance_group,
            signed_chance_budget: state.signed_chance_budget,
            winner_seen: state.winner_seen,
            facts_digest: 0,
        },
    };
    let digest = later_bonus_facts_digest(&facts);
    if let LaterBonusMutationEvidence::RetailCapture { facts_digest, .. } = &mut facts.evidence {
        *facts_digest = digest;
    }
    facts
}

fn chance_miss_first_bonus_schedule(
    row_count: usize,
) -> (ResourceDivvyPoolState, MapMakeResourceScheduleReceipt) {
    assert!(matches!(row_count, 1 | 3));
    let post = post_nubify();
    let owner = owner();
    let caller = caller_facts(&post, &owner, 0);
    let mut resources = resource_facts(&post);
    resources.goods[2].random_rare_drop = 1;
    resources.goods[2].resolved_age = 3;
    let mut expected_pool = expected_resource_pool();
    expected_pool.late_bits.bits = 2;
    expected_pool.late_goods.push(FIRST_SCANNED_GOOD_ID + 2);
    let mut xml = xml_facts(&post, resource_divvy_pool_digest(&expected_pool));
    let rows = &mut xml.selected_bonuses.as_mut().unwrap().bonus_rows;
    if row_count == 1 {
        rows.truncate(1);
    } else {
        rows.push(BonusXmlRowFact {
            capture_ordinal: 11,
            element_name: "BONUS".to_owned(),
            handles: XmlHostHandles {
                head: true,
                tail: true,
                inline_tail_word: false,
            },
        });
    }
    let mut pool = ResourceDivvyPoolState::default();
    let mut xml_host = XmlHostHandles::default();
    let xml_schedule = execute_map_make_resource_schedule_with_xml(
        &mut pool,
        &mut xml_host,
        &post,
        &owner,
        &caller,
        Some(&resources),
        Some(&xml),
    )
    .unwrap();
    let handoff = match &xml_schedule.placement {
        MapMakeResourcePlacementReceipt::XmlRowsOpen(boundary) => {
            boundary.xml_frontier.handoff.clone()
        }
        _ => panic!("XML facts must produce the first-row handoff"),
    };
    let facts = first_chance_miss_facts(&handoff);
    let mut host = NoPlacementExpected;
    let first_schedule = continue_map_make_resource_schedule_first_bonus(
        &mut pool,
        &xml_schedule,
        &facts,
        &mut host,
    )
    .unwrap();
    (pool, first_schedule)
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
    assert!(place_resources_stage.rng.contains("0x00690225"));

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
fn first_bonus_schedule_continuation_preserves_the_authoritative_public_pool() {
    let post = post_nubify();
    let owner = owner();
    let caller = caller_facts(&post, &owner, 0);
    let resources = resource_facts(&post);
    let expected_pool = expected_resource_pool();
    let xml = xml_facts(&post, resource_divvy_pool_digest(&expected_pool));
    let mut pool = ResourceDivvyPoolState::default();
    let mut xml_host = XmlHostHandles::default();
    let schedule = execute_map_make_resource_schedule_with_xml(
        &mut pool,
        &mut xml_host,
        &post,
        &owner,
        &caller,
        Some(&resources),
        Some(&xml),
    )
    .unwrap();
    let handoff = match &schedule.placement {
        MapMakeResourcePlacementReceipt::XmlRowsOpen(boundary) => {
            boundary.xml_frontier.handoff.clone()
        }
        _ => panic!("XML facts must produce the first-row handoff"),
    };
    let row = &handoff.rows[0];
    let facts = FirstBonusMutationFacts {
        capture_ordinal: row.capture_ordinal,
        type_name: "Salt".to_owned(),
        type_resolution: ResourceTypeResolution::CatalogGood { good_id: 6 },
        chance: 0,
        chance_group: 0,
        pattern_name: "player".to_owned(),
        pattern: PlacementPattern::Player,
        saturate: 0,
        spacing: 0,
        scaled: Vec::new(),
        evidence: BonusMutationEvidence::RetailCapture {
            executable_sha256: BONUS_EXE_SHA256.to_owned(),
            pdb_sha256: BONUS_PDB_SHA256.to_owned(),
            capture_sha256: [0xd8; 32],
            entry_va: handoff.resume_va,
            capture_ordinal: row.capture_ordinal,
            random_state: handoff.random_state,
            world_checksum: handoff.world_checksum.clone(),
            sourced_walked_bytes: handoff.sourced_walked_bytes,
            resource_pool_digest: handoff.resource_pool_digest,
        },
    };
    let pool_before = pool.clone();
    let mut placement_host = NoPlacementExpected;

    let continued = continue_map_make_resource_schedule_first_bonus(
        &mut pool,
        &schedule,
        &facts,
        &mut placement_host,
    )
    .unwrap();
    let MapMakeResourcePlacementReceipt::FirstBonusRowOpen(boundary) = continued.placement else {
        panic!("first row must advance to its exact recurrence boundary");
    };

    assert_eq!(
        boundary.first_bonus.disposition,
        FirstBonusDisposition::ChanceMiss
    );
    assert_eq!(boundary.first_bonus.residual_va, 0x0069_0215);
    assert_eq!(
        boundary.pending_checkpoint_call_va,
        MAP_POST_RESOURCES_CHECKPOINT_CALL_VA
    );
    assert_eq!(
        boundary.pending_source_token,
        MAP_POST_RESOURCES_SOURCE_TOKEN
    );
    assert_eq!(boundary.resource_pool_after, pool_before);
    assert_eq!(pool, pool_before);
    assert_eq!(MAP_MAKE_SCHEDULE[12].checkpoint, None);
}

#[test]
fn selector_continuation_commits_the_exact_concrete_post_placement_pool() {
    let post = post_nubify();
    let owner = owner();
    let caller = caller_facts(&post, &owner, 0);
    let mut resources = resource_facts(&post);
    resources.goods[2].random_rare_drop = 1;
    resources.goods[2].resolved_age = 3;
    let mut expected_pool = expected_resource_pool();
    expected_pool.late_bits.bits = 2;
    expected_pool.late_goods.push(FIRST_SCANNED_GOOD_ID + 2);
    let xml = xml_facts(&post, resource_divvy_pool_digest(&expected_pool));
    let mut pool = ResourceDivvyPoolState::default();
    let mut xml_host = XmlHostHandles::default();
    let schedule = execute_map_make_resource_schedule_with_xml(
        &mut pool,
        &mut xml_host,
        &post,
        &owner,
        &caller,
        Some(&resources),
        Some(&xml),
    )
    .unwrap();
    assert_eq!(pool, expected_pool);
    let handoff = match &schedule.placement {
        MapMakeResourcePlacementReceipt::XmlRowsOpen(boundary) => {
            boundary.xml_frontier.handoff.clone()
        }
        _ => panic!("XML facts must produce the first-row handoff"),
    };
    let facts = selector_bonus_facts(&handoff, -1);
    let digest_before = resource_divvy_pool_digest(&pool);
    let mut placement_host = ExactSelectorHost;

    let continued = continue_map_make_resource_schedule_first_bonus(
        &mut pool,
        &schedule,
        &facts,
        &mut placement_host,
    )
    .unwrap();
    let MapMakeResourcePlacementReceipt::FirstBonusRowOpen(boundary) = continued.placement else {
        panic!("selector row must advance to its exact recurrence boundary");
    };
    let placement = boundary.first_bonus.placement.as_ref().unwrap();

    assert_eq!(
        boundary.first_bonus.disposition,
        FirstBonusDisposition::Placed(
            don_replay::place_resources_bonus_mutation_frontier::PlacementPath::Player
        )
    );
    assert_eq!(placement.resource_pool_selections.len(), 1);
    assert_eq!(
        placement.resource_pool_selections[0].lane,
        ResourcePoolLane::Late
    );
    assert_eq!(pool, boundary.resource_pool_after);
    assert_eq!(
        resource_divvy_pool_digest(&pool),
        placement.resource_pool_digest_after
    );
    assert_ne!(resource_divvy_pool_digest(&pool), digest_before);
}

#[test]
fn malformed_selector_subreceipt_rolls_back_the_public_pool_after_direct_chance_rng() {
    let post = post_nubify();
    let owner = owner();
    let caller = caller_facts(&post, &owner, 0);
    let mut resources = resource_facts(&post);
    resources.goods[2].random_rare_drop = 1;
    resources.goods[2].resolved_age = 3;
    let mut expected_pool = expected_resource_pool();
    expected_pool.late_bits.bits = 2;
    expected_pool.late_goods.push(FIRST_SCANNED_GOOD_ID + 2);
    let xml = xml_facts(&post, resource_divvy_pool_digest(&expected_pool));
    let mut pool = ResourceDivvyPoolState::default();
    let mut xml_host = XmlHostHandles::default();
    let schedule = execute_map_make_resource_schedule_with_xml(
        &mut pool,
        &mut xml_host,
        &post,
        &owner,
        &caller,
        Some(&resources),
        Some(&xml),
    )
    .unwrap();
    let handoff = match &schedule.placement {
        MapMakeResourcePlacementReceipt::XmlRowsOpen(boundary) => {
            boundary.xml_frontier.handoff.clone()
        }
        _ => panic!("XML facts must produce the first-row handoff"),
    };
    // Group zero consumes the direct row draw before the corrupt placement receipt arrives.
    let facts = selector_bonus_facts(&handoff, 0);
    let pool_before = pool.clone();
    let mut corrupt_host = CorruptSelectorHost;

    assert_eq!(
        continue_map_make_resource_schedule_first_bonus(
            &mut pool,
            &schedule,
            &facts,
            &mut corrupt_host,
        ),
        Err(MapMakeResourceScheduleError::BonusFrontier(
            FirstBonusMutationError::InvalidResourcePoolSelection { index: 0 }
        ))
    );
    assert_eq!(pool, pool_before);
}

#[test]
fn singleton_bonus_array_advances_through_only_the_no_rng_category_tail() {
    let (mut pool, first_schedule) = chance_miss_first_bonus_schedule(1);
    let pool_before = pool.clone();
    let first = match &first_schedule.placement {
        MapMakeResourcePlacementReceipt::FirstBonusRowOpen(first) => first,
        _ => panic!("singleton row must first stop at its recurrence seam"),
    };
    let random_state = first.first_bonus.random_state_after;
    let world_checksum = first.first_bonus.world_checksum_after.clone();

    let continued =
        continue_map_make_resource_schedule_bonus_category_tail(&mut pool, &first_schedule)
            .unwrap();
    let rows = match &continued.placement {
        MapMakeResourcePlacementReceipt::BonusRowsOpen(rows) => rows,
        _ => panic!("singleton row must publish the authenticated category tail"),
    };
    let tail = rows.category_tail.as_ref().unwrap();

    assert!(rows.steps.is_empty());
    assert_eq!(rows.remaining_state.next_row_index, 1);
    assert_eq!(rows.residual_va, BONUS_CATEGORY_TAIL_VA);
    assert_eq!(tail.row_count_before, 1);
    assert_eq!(tail.row_count_after, 0);
    assert!(!tail.branch_taken);
    assert_eq!(tail.random_state, random_state);
    assert_eq!(tail.world_checksum, world_checksum);
    assert_eq!(pool, pool_before);

    let (mut incomplete_pool, incomplete) = chance_miss_first_bonus_schedule(3);
    let incomplete_before = incomplete_pool.clone();
    assert_eq!(
        continue_map_make_resource_schedule_bonus_category_tail(&mut incomplete_pool, &incomplete,),
        Err(MapMakeResourceScheduleError::BonusCategoryTailRequiresCompletedRows)
    );
    assert_eq!(incomplete_pool, incomplete_before);
}

#[test]
fn later_selector_commits_concrete_pool_and_corruption_rolls_back_the_schedule_boundary() {
    let (mut pool, first_schedule) = chance_miss_first_bonus_schedule(3);
    let first = match &first_schedule.placement {
        MapMakeResourcePlacementReceipt::FirstBonusRowOpen(first) => first,
        _ => panic!("first continuation must expose its recurrence boundary"),
    };
    let state = remaining_state_from_first(first);
    let facts = later_selector_facts(&state, &first.xml.xml_frontier.handoff);
    let pool_before = pool.clone();
    let digest_before = resource_divvy_pool_digest(&pool);
    let mut exact_host = ExactSelectorHost;

    let continued = continue_map_make_resource_schedule_next_bonus(
        &mut pool,
        &first_schedule,
        &facts,
        &mut exact_host,
    )
    .unwrap();
    let rows = match &continued.placement {
        MapMakeResourcePlacementReceipt::BonusRowsOpen(rows) => rows,
        _ => panic!("later selector must retain a typed schedule boundary"),
    };
    let receipt = &rows.steps[0].receipt;
    let placement = receipt.placement.as_ref().unwrap();

    assert_eq!(pool, rows.resource_pool_after);
    assert_eq!(state.mutation.resource_pool.as_ref(), Some(&pool_before));
    assert_eq!(
        rows.remaining_state.mutation.resource_pool.as_ref(),
        Some(&pool)
    );
    assert_ne!(resource_divvy_pool_digest(&pool), digest_before);
    assert_eq!(
        resource_divvy_pool_digest(&pool),
        receipt.resource_pool_digest_after
    );
    assert_eq!(placement.resource_pool_selections.len(), 1);
    assert_eq!(
        placement.resource_pool_selections[0].lane,
        ResourcePoolLane::Late
    );
    assert_eq!(
        placement.random_draws[0].state_before,
        receipt.chance_draw.as_ref().unwrap().state_after
    );
    assert_eq!(placement.random_state_after, receipt.random_state_after);

    let mut rollback_pool = pool_before.clone();
    let rollback_before = rollback_pool.clone();
    let schedule_before = first_schedule.clone();
    let mut corrupt_host = CorruptSelectorHost;
    assert_eq!(
        continue_map_make_resource_schedule_next_bonus(
            &mut rollback_pool,
            &first_schedule,
            &facts,
            &mut corrupt_host,
        ),
        Err(MapMakeResourceScheduleError::BonusRowsFrontier(
            RemainingBonusRowsError::InvalidResourcePoolSelection { index: 0 }
        ))
    );
    assert_eq!(rollback_pool, rollback_before);
    assert_eq!(first_schedule, schedule_before);
}

#[test]
fn repeated_later_bonus_rows_replay_carry_and_reach_only_the_category_tail() {
    let (mut pool, first_schedule) = chance_miss_first_bonus_schedule(3);
    let pool_before = pool.clone();
    let first = match &first_schedule.placement {
        MapMakeResourcePlacementReceipt::FirstBonusRowOpen(first) => first,
        _ => panic!("first continuation must expose its recurrence boundary"),
    };
    let first_state = remaining_state_from_first(first);
    let first_later_facts = later_chance_miss_facts(&first_state, &first.xml.xml_frontier.handoff);
    let mut host = NoPlacementExpected;

    let after_second = continue_map_make_resource_schedule_next_bonus(
        &mut pool,
        &first_schedule,
        &first_later_facts,
        &mut host,
    )
    .unwrap();
    let second = match &after_second.placement {
        MapMakeResourcePlacementReceipt::BonusRowsOpen(rows) => rows,
        _ => panic!("later continuation must retain the typed carry"),
    };
    assert_eq!(second.steps.len(), 1);
    assert!(second.category_tail.is_none());
    assert_eq!(second.residual_va, LATER_BONUS_ENTRY_VA);
    assert_eq!(
        second.steps[0].receipt.random_state_before,
        first.first_bonus.random_state_after
    );
    let second_later_facts = later_chance_miss_facts(
        &second.remaining_state,
        &second.first.xml.xml_frontier.handoff,
    );

    let after_third = continue_map_make_resource_schedule_next_bonus(
        &mut pool,
        &after_second,
        &second_later_facts,
        &mut host,
    )
    .unwrap();
    let third = match &after_third.placement {
        MapMakeResourcePlacementReceipt::BonusRowsOpen(rows) => rows,
        _ => panic!("final later row must retain the category-tail boundary"),
    };
    let tail = third.category_tail.as_ref().unwrap();

    assert_eq!(third.steps.len(), 2);
    assert_eq!(third.remaining_state.next_row_index, 3);
    assert_eq!(third.residual_va, BONUS_CATEGORY_TAIL_VA);
    assert_eq!(tail.entry_va, LATER_BONUS_ENTRY_VA);
    assert_eq!(tail.row_count_before, 1);
    assert_eq!(tail.row_count_after, 0);
    assert!(!tail.branch_taken);
    assert_eq!(
        tail.restore_category_pointer_va,
        BONUS_CATEGORY_TAIL_RESTORE_VA
    );
    assert_eq!(tail.residual_va, BONUS_CATEGORY_TAIL_VA);
    assert_eq!(
        third.steps[1].receipt.random_state_before,
        third.steps[0].receipt.random_state_after
    );
    assert_eq!(tail.random_state, third.steps[1].receipt.random_state_after);
    assert_eq!(pool, pool_before);
    assert_eq!(third.resource_pool_after, pool_before);
    assert_eq!(
        third.pending_checkpoint_call_va,
        MAP_POST_RESOURCES_CHECKPOINT_CALL_VA
    );
    assert_eq!(third.pending_source_token, MAP_POST_RESOURCES_SOURCE_TOKEN);
    assert_eq!(MAP_MAKE_SCHEDULE[12].checkpoint, None);

    let pool_at_tail = pool.clone();
    assert_eq!(
        continue_map_make_resource_schedule_next_bonus(
            &mut pool,
            &after_third,
            &second_later_facts,
            &mut host,
        ),
        Err(MapMakeResourceScheduleError::BonusRowsComplete)
    );
    assert_eq!(pool, pool_at_tail);

    let mut missing_tail = after_third.clone();
    match &mut missing_tail.placement {
        MapMakeResourcePlacementReceipt::BonusRowsOpen(rows) => {
            rows.category_tail = None;
            rows.residual_va = LATER_BONUS_ENTRY_VA;
        }
        _ => unreachable!(),
    }
    assert_eq!(
        continue_map_make_resource_schedule_next_bonus(
            &mut pool,
            &missing_tail,
            &second_later_facts,
            &mut host,
        ),
        Err(MapMakeResourceScheduleError::BonusRowsContinuityMismatch)
    );
    assert_eq!(pool, pool_at_tail);

    let mut forged_tail = after_third.clone();
    match &mut forged_tail.placement {
        MapMakeResourcePlacementReceipt::BonusRowsOpen(rows) => {
            rows.category_tail.as_mut().unwrap().random_state ^= 1;
        }
        _ => unreachable!(),
    }
    assert_eq!(
        continue_map_make_resource_schedule_next_bonus(
            &mut pool,
            &forged_tail,
            &second_later_facts,
            &mut host,
        ),
        Err(MapMakeResourceScheduleError::BonusRowsContinuityMismatch)
    );
    assert_eq!(pool, pool_at_tail);
}

#[test]
fn stale_later_row_evidence_rolls_back_public_pool_and_schedule_carry() {
    let (mut pool, first_schedule) = chance_miss_first_bonus_schedule(3);
    let first = match &first_schedule.placement {
        MapMakeResourcePlacementReceipt::FirstBonusRowOpen(first) => first,
        _ => panic!("first continuation must expose its recurrence boundary"),
    };
    let state = remaining_state_from_first(first);
    let mut facts = later_chance_miss_facts(&state, &first.xml.xml_frontier.handoff);
    if let LaterBonusMutationEvidence::RetailCapture { random_state, .. } = &mut facts.evidence {
        *random_state ^= 1;
    }
    let pool_before = pool.clone();
    let schedule_before = first_schedule.clone();
    let mut host = NoPlacementExpected;

    assert_eq!(
        continue_map_make_resource_schedule_next_bonus(
            &mut pool,
            &first_schedule,
            &facts,
            &mut host,
        ),
        Err(MapMakeResourceScheduleError::BonusRowsFrontier(
            RemainingBonusRowsError::StaleEvidence
        ))
    );
    assert_eq!(pool, pool_before);
    assert_eq!(first_schedule, schedule_before);
}

#[test]
fn state_carrying_upstream_prefix_tampering_is_rejected_before_later_row_execution() {
    let (mut pool, first_schedule) = chance_miss_first_bonus_schedule(3);
    let first = match &first_schedule.placement {
        MapMakeResourcePlacementReceipt::FirstBonusRowOpen(first) => first,
        _ => panic!("first continuation must expose its recurrence boundary"),
    };
    let state = remaining_state_from_first(first);
    let facts = later_chance_miss_facts(&state, &first.xml.xml_frontier.handoff);
    let pool_before = pool.clone();
    let mut host = NoPlacementExpected;

    let mut caller_tamper = first_schedule.clone();
    caller_tamper.post_nubify_checkpoint.random_state ^= 1;
    assert_eq!(
        continue_map_make_resource_schedule_next_bonus(
            &mut pool,
            &caller_tamper,
            &facts,
            &mut host,
        ),
        Err(MapMakeResourceScheduleError::BonusRowsContinuityMismatch)
    );
    assert_eq!(pool, pool_before);

    let mut xml_tamper = first_schedule.clone();
    match &mut xml_tamper.placement {
        MapMakeResourcePlacementReceipt::FirstBonusRowOpen(first) => {
            first.xml.xml_frontier.receipt.random_state_before ^= 1;
        }
        _ => unreachable!(),
    }
    assert_eq!(
        continue_map_make_resource_schedule_next_bonus(&mut pool, &xml_tamper, &facts, &mut host,),
        Err(MapMakeResourceScheduleError::BonusRowsContinuityMismatch)
    );
    assert_eq!(pool, pool_before);
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

fn fish_category_inputs(
    state: &RemainingBonusRowsState,
) -> (CategoryAdvanceFacts, PlaceResourcesDocumentHostAuthority) {
    fish_category_inputs_with_rows(state, 1)
}

fn fish_category_inputs_with_rows(
    state: &RemainingBonusRowsState,
    row_count: usize,
) -> (CategoryAdvanceFacts, PlaceResourcesDocumentHostAuthority) {
    let capture_sha256 = [0x6d; 32];
    let selected_document_handles = HostHandles {
        head: true,
        tail: true,
        inline_tail_word: false,
    };
    let default_document_handles = HostHandles {
        head: true,
        tail: false,
        inline_tail_word: true,
    };
    let section = CategorySectionFact {
        lookup_token: "FISH ".to_owned(),
        returned_element_name: "FISH".to_owned(),
        handles: HostHandles {
            head: true,
            tail: true,
            inline_tail_word: false,
        },
        rows: (0..row_count)
            .map(|index| CategoryRowFact {
                capture_ordinal: 41 + index as u32,
                element_name: "BONUS".to_owned(),
                handles: HostHandles {
                    head: index % 2 == 1,
                    tail: true,
                    inline_tail_word: false,
                },
            })
            .collect(),
    };
    (
        CategoryAdvanceFacts {
            selected_style_name_nonempty: true,
            selected_section: Some(section),
            default_section: None,
            evidence: ResourceFrontierEvidence::RetailCapture {
                executable_sha256: CATEGORY_EXE_SHA256.to_owned(),
                pdb_sha256: CATEGORY_PDB_SHA256.to_owned(),
                capture_sha256,
                entry_va: CATEGORY_TAIL_VA,
                random_state: state.mutation.random_state,
                world_checksum: state.mutation.world_checksum.clone(),
                sourced_walked_bytes: state.mutation.sourced_walked_bytes,
                resource_pool_digest: state.mutation.resource_pool_digest,
            },
        },
        PlaceResourcesDocumentHostAuthority {
            selected_document_handles,
            default_document_handles,
            capture_sha256,
        },
    )
}

fn empty_fish_category_inputs(
    state: &RemainingBonusRowsState,
) -> (CategoryAdvanceFacts, PlaceResourcesDocumentHostAuthority) {
    fish_category_inputs_with_rows(state, 0)
}

fn first_fish_chance_miss_facts(
    fish: &don_replay::map_make_resource_schedule_integration::PlaceResourcesFishCategoryBoundary,
) -> CanonicalRowFacts {
    let state = &fish.category_state_after.deterministic;
    let capture_ordinal = fish.fish_handoff.rows[0].capture_ordinal;
    let mut facts = LaterBonusMutationFacts {
        capture_ordinal,
        type_name: "WHALES".to_owned(),
        type_resolution: ResourceTypeResolution::CatalogGood { good_id: 6 },
        chance: 0,
        chance_group: state.last_chance_group,
        pattern_name: "PLAYER".to_owned(),
        pattern: PlacementPattern::Player,
        saturate: 0,
        spacing: 0,
        scaled: Vec::new(),
        evidence: LaterBonusMutationEvidence::CarriedRetailCapture {
            executable_sha256: BONUS_EXE_SHA256.to_owned(),
            pdb_sha256: BONUS_PDB_SHA256.to_owned(),
            capture_sha256: [0x7e; 32],
            category: ResourceCategory::Fish,
            entry_va: CARRIED_CATEGORY_FIRST_ENTRY_VA,
            row_enumerate_call_va: ROW_ENUMERATE_CALL_VA,
            row_count_test_va: ROW_COUNT_TEST_VA,
            mutation_entry_va: LATER_ROW_MUTATION_ENTRY_VA,
            row_index: 0,
            capture_ordinal,
            random_state: state.random_state,
            world_checksum: state.world_checksum.clone(),
            sourced_walked_bytes: state.sourced_walked_bytes,
            resource_pool_digest: state.resource_pool_digest,
            last_chance_group: state.last_chance_group,
            signed_chance_budget: state.signed_chance_budget,
            winner_seen: state.winner_seen,
            facts_digest: 0,
        },
    };
    let digest = later_bonus_facts_digest(&facts);
    if let LaterBonusMutationEvidence::CarriedRetailCapture { facts_digest, .. } =
        &mut facts.evidence
    {
        *facts_digest = digest;
    }
    CanonicalRowFacts {
        mutation: CanonicalRowMutationFacts::CarriedOrLater(facts),
        placement: None,
    }
}

fn canonical_fish_state(
    fish: &don_replay::map_make_resource_schedule_integration::PlaceResourcesFishCategoryBoundary,
    pool: &ResourceDivvyPoolState,
) -> CanonicalPlaceResourcesState {
    let state = &fish.category_state_after;
    CanonicalPlaceResourcesState {
        mutation: PlaceResourcesBonusMutationState {
            random_state: state.deterministic.random_state,
            world_checksum: state.deterministic.world_checksum.clone(),
            sourced_walked_bytes: state.deterministic.sourced_walked_bytes,
            resource_pool_digest: state.deterministic.resource_pool_digest,
            resource_pool: Some(pool.clone()),
            allocated_resources: state.deterministic.allocated_resources,
            requested_resources: state.deterministic.requested_resources,
        },
        good_state_digest: 0x1122_3344_5566_7788,
        item_state_digest: 0x8877_6655_4433_2211,
        selected_document_handles: state.selected_document_handles,
        default_document_handles: state.default_document_handles,
    }
}

fn goodies_category_facts(
    fish: &don_replay::map_make_resource_schedule_integration::PlaceResourcesFishCategoryBoundary,
) -> CategoryAdvanceFacts {
    let state = &fish.category_state_after.deterministic;
    CategoryAdvanceFacts {
        selected_style_name_nonempty: true,
        selected_section: Some(CategorySectionFact {
            lookup_token: "GOODIES".to_owned(),
            returned_element_name: "GOODIES".to_owned(),
            handles: HostHandles {
                head: true,
                tail: true,
                inline_tail_word: false,
            },
            rows: vec![CategoryRowFact {
                capture_ordinal: 51,
                element_name: "BONUS".to_owned(),
                handles: HostHandles {
                    head: true,
                    tail: false,
                    inline_tail_word: true,
                },
            }],
        }),
        default_section: None,
        evidence: ResourceFrontierEvidence::RetailCapture {
            executable_sha256: CATEGORY_EXE_SHA256.to_owned(),
            pdb_sha256: CATEGORY_PDB_SHA256.to_owned(),
            capture_sha256: fish.document_host_authority.capture_sha256,
            entry_va: CATEGORY_TAIL_VA,
            random_state: state.random_state,
            world_checksum: state.world_checksum.clone(),
            sourced_walked_bytes: state.sourced_walked_bytes,
            resource_pool_digest: state.resource_pool_digest,
        },
    }
}

#[derive(Default)]
struct NoCanonicalAllocationHost;

impl PlayerAllocationHost for NoCanonicalAllocationHost {
    fn propose_allocation(
        &mut self,
        _request: &PlayerAllocationRequest,
    ) -> Option<PlayerAllocationReceipt> {
        panic!("chance-miss FISH row must not allocate a Player resource")
    }
}

impl RegionInitGoodHost for NoCanonicalAllocationHost {
    fn propose_init_good(
        &mut self,
        _request: &RegionInitGoodRequest,
    ) -> Option<RegionInitGoodReceipt> {
        panic!("chance-miss FISH row must not allocate a Region resource")
    }
}

fn chance_miss_first_fish_schedule(
    fish_row_count: usize,
) -> (
    ResourceDivvyPoolState,
    MapMakeResourceScheduleReceipt,
    CanonicalPlaceResourcesState,
) {
    assert!(fish_row_count > 0);
    let (mut pool, first_schedule) = chance_miss_first_bonus_schedule(1);
    let completed =
        continue_map_make_resource_schedule_bonus_category_tail(&mut pool, &first_schedule)
            .unwrap();
    let rows = match &completed.placement {
        MapMakeResourcePlacementReceipt::BonusRowsOpen(rows) => rows,
        _ => panic!("singleton BONUSES must reach category cleanup"),
    };
    let (category_facts, authority) =
        fish_category_inputs_with_rows(&rows.remaining_state, fish_row_count);
    let fish_schedule = continue_map_make_resource_schedule_fish_category(
        &mut pool,
        &completed,
        &authority,
        &category_facts,
    )
    .unwrap();
    let fish = match &fish_schedule.placement {
        MapMakeResourcePlacementReceipt::FishCategoryOpen(fish) => fish,
        _ => panic!("FISH category must stop before row zero"),
    };
    let facts = first_fish_chance_miss_facts(fish);
    let mut canonical = canonical_fish_state(fish, &pool);
    let mut host = NoCanonicalAllocationHost;
    let first_fish = continue_map_make_resource_schedule_first_fish(
        &mut pool,
        &mut canonical,
        &fish_schedule,
        &facts,
        &mut host,
    )
    .unwrap();
    (pool, first_fish, canonical)
}

#[test]
fn completed_bonus_schedule_owns_cleanup_and_stops_before_first_fish_row() {
    let (mut pool, first_schedule) = chance_miss_first_bonus_schedule(1);
    let completed =
        continue_map_make_resource_schedule_bonus_category_tail(&mut pool, &first_schedule)
            .unwrap();
    let rows = match &completed.placement {
        MapMakeResourcePlacementReceipt::BonusRowsOpen(rows) => rows,
        _ => panic!("singleton BONUSES must reach category cleanup"),
    };
    let deterministic_before = rows.remaining_state.clone();
    let pool_before = pool.clone();
    let (facts, authority) = fish_category_inputs(&rows.remaining_state);

    let continued = continue_map_make_resource_schedule_fish_category(
        &mut pool, &completed, &authority, &facts,
    )
    .unwrap();
    let fish = match &continued.placement {
        MapMakeResourcePlacementReceipt::FishCategoryOpen(fish) => fish,
        _ => panic!("category continuation must stop at the FISH boundary"),
    };

    assert_eq!(fish.category_receipt.entry_va, CATEGORY_TAIL_VA);
    assert_eq!(
        fish.category_receipt.completed_category,
        ResourceCategory::Bonuses
    );
    assert_eq!(fish.category_state_after.category, ResourceCategory::Fish);
    assert_eq!(fish.category_state_after.rows_remaining, 1);
    assert_eq!(fish.fish_handoff.rows.len(), 1);
    assert_eq!(fish.fish_handoff.rows[0].capture_ordinal, 41);
    assert_eq!(fish.residual_va, ROW_BODY_VA);
    assert_eq!(
        fish.category_state_after.deterministic.random_state,
        deterministic_before.mutation.random_state
    );
    assert_eq!(
        fish.category_state_after.deterministic.world_checksum,
        deterministic_before.mutation.world_checksum
    );
    assert_eq!(
        fish.category_state_after.deterministic.last_chance_group,
        deterministic_before.last_chance_group
    );
    assert_eq!(pool, pool_before);
    assert!(matches!(
        fish.category_receipt.disposition,
        CategoryAdvanceDisposition::NextCategory {
            category: ResourceCategory::Fish,
            source: SectionSource::Selected,
            residual_va: ROW_BODY_VA,
            ..
        }
    ));
    assert_eq!(
        fish.category_receipt
            .cleanup_operations
            .iter()
            .take(3)
            .map(|operation| operation.call_va)
            .collect::<Vec<_>>(),
        vec![
            CATEGORY_RELEASE_CALL_VAS[0],
            CATEGORY_RELEASE_CALL_VAS[1],
            CATEGORY_RELEASE_CALL_VAS[3],
        ]
    );
    assert_eq!(
        fish.category_receipt
            .cleanup_operations
            .iter()
            .map(|operation| operation.kind)
            .collect::<Vec<_>>(),
        vec![
            HostRefKind::ReleaseCategoryTail,
            HostRefKind::ReleaseCategoryHead,
            HostRefKind::ReleaseRowHead,
            HostRefKind::AcquireCategoryHead,
            HostRefKind::AcquireCategoryTail,
        ]
    );
}

#[test]
fn first_fish_row_composes_carried_mutation_and_stops_before_recurrence() {
    let (mut pool, first_schedule) = chance_miss_first_bonus_schedule(1);
    let completed =
        continue_map_make_resource_schedule_bonus_category_tail(&mut pool, &first_schedule)
            .unwrap();
    let rows = match &completed.placement {
        MapMakeResourcePlacementReceipt::BonusRowsOpen(rows) => rows,
        _ => panic!("singleton BONUSES must reach category cleanup"),
    };
    let (category_facts, authority) = fish_category_inputs(&rows.remaining_state);
    let fish_schedule = continue_map_make_resource_schedule_fish_category(
        &mut pool,
        &completed,
        &authority,
        &category_facts,
    )
    .unwrap();
    let fish = match &fish_schedule.placement {
        MapMakeResourcePlacementReceipt::FishCategoryOpen(fish) => fish,
        _ => panic!("FISH category must stop before row zero"),
    };
    let random_before = fish.category_state_after.deterministic.random_state;
    let pool_before = pool.clone();
    let facts = first_fish_chance_miss_facts(fish);
    let mut canonical = canonical_fish_state(fish, &pool);
    let mut host = NoCanonicalAllocationHost;

    let continued = continue_map_make_resource_schedule_first_fish(
        &mut pool,
        &mut canonical,
        &fish_schedule,
        &facts,
        &mut host,
    )
    .unwrap();
    let first_fish = match &continued.placement {
        MapMakeResourcePlacementReceipt::FirstFishRowOpen(first_fish) => first_fish,
        _ => panic!("first FISH row must stop before recurrence"),
    };
    let CanonicalRowMutationReceipt::CarriedFirst(receipt) = &first_fish.row_receipt.mutation
    else {
        panic!("row zero must use the carried-category owner")
    };

    assert_eq!(
        first_fish.residual_va,
        don_replay::place_resources_bonus_mutation_frontier::FIRST_BONUS_ROW_RESIDUAL_VA
    );
    assert_eq!(first_fish.remaining_state.next_row_index, 1);
    assert_eq!(receipt.entry_va, CARRIED_CATEGORY_FIRST_ENTRY_VA);
    assert_eq!(receipt.category, ResourceCategory::Fish);
    assert_eq!(receipt.random_state_before, random_before);
    let draw = receipt
        .chance_draw
        .as_ref()
        .expect("chance group zero redraws at every row entry");
    let mut expected_random = Random::new(random_before);
    let expected_raw = expected_random.get(0, 0xffff);
    assert_eq!(draw.state_before, random_before);
    assert_eq!(draw.raw, expected_raw);
    assert_eq!(draw.modulo_100, expected_raw % 100);
    assert_eq!(draw.state_after, expected_random.state());
    assert_eq!(receipt.random_state_after, expected_random.state());
    assert!(receipt.placement.is_none());
    assert_eq!(pool, pool_before);
    assert_eq!(canonical, first_fish.canonical_state_after);
}

#[test]
fn singleton_fish_recurrence_falls_through_without_mutating_any_authority() {
    let (pool, first_fish_schedule, canonical) = chance_miss_first_fish_schedule(1);
    let first = match &first_fish_schedule.placement {
        MapMakeResourcePlacementReceipt::FirstFishRowOpen(first) => first,
        _ => panic!("fixture must stop after FISH row zero"),
    };
    assert!(first.allocation_transcript.player.is_empty());
    assert!(first.allocation_transcript.region.is_empty());
    let pool_before = pool.clone();
    let canonical_before = canonical.clone();

    let continued = continue_map_make_resource_schedule_fish_recurrence(
        &pool,
        &canonical,
        &first_fish_schedule,
    )
    .unwrap();
    let recurrence = match &continued.placement {
        MapMakeResourcePlacementReceipt::FishRecurrenceOpen(recurrence) => recurrence,
        _ => panic!("singleton FISH must stop at category cleanup"),
    };

    assert_eq!(recurrence.recurrence.row_count_before, 1);
    assert_eq!(recurrence.recurrence.row_count_after, 0);
    assert!(!recurrence.recurrence.branch_taken);
    assert_eq!(recurrence.residual_va, CATEGORY_TAIL_VA);
    assert_eq!(
        recurrence.recurrence.random_state,
        first.remaining_state.mutation.random_state
    );
    assert_eq!(
        recurrence.recurrence.world_checksum,
        first.remaining_state.mutation.world_checksum
    );
    assert_eq!(pool, pool_before);
    assert_eq!(canonical, canonical_before);
}

#[test]
fn multirow_fish_recurrence_replays_row_zero_and_stops_before_next_row() {
    let (pool, first_fish_schedule, canonical) = chance_miss_first_fish_schedule(2);
    let continued = continue_map_make_resource_schedule_fish_recurrence(
        &pool,
        &canonical,
        &first_fish_schedule,
    )
    .unwrap();
    let recurrence = match &continued.placement {
        MapMakeResourcePlacementReceipt::FishRecurrenceOpen(recurrence) => recurrence,
        _ => panic!("multirow FISH must stop before the next row"),
    };

    assert_eq!(recurrence.recurrence.row_count_before, 2);
    assert_eq!(recurrence.recurrence.row_count_after, 1);
    assert!(recurrence.recurrence.branch_taken);
    assert_eq!(recurrence.residual_va, ROW_BODY_VA);

    let mut tampered = first_fish_schedule.clone();
    let MapMakeResourcePlacementReceipt::FirstFishRowOpen(first) = &mut tampered.placement else {
        unreachable!()
    };
    first.canonical_state_after.good_state_digest ^= 1;
    assert_eq!(
        continue_map_make_resource_schedule_fish_recurrence(&pool, &canonical, &tampered),
        Err(MapMakeResourceScheduleError::FishRecurrenceContinuityMismatch)
    );
}

#[test]
fn empty_fish_cleanup_reaches_first_goodies_row_without_rng_or_world_change() {
    let (mut pool, first_schedule) = chance_miss_first_bonus_schedule(1);
    let completed =
        continue_map_make_resource_schedule_bonus_category_tail(&mut pool, &first_schedule)
            .unwrap();
    let rows = match &completed.placement {
        MapMakeResourcePlacementReceipt::BonusRowsOpen(rows) => rows,
        _ => panic!("singleton BONUSES must reach category cleanup"),
    };
    let (fish_facts, authority) = empty_fish_category_inputs(&rows.remaining_state);
    let fish_schedule = continue_map_make_resource_schedule_fish_category(
        &mut pool,
        &completed,
        &authority,
        &fish_facts,
    )
    .unwrap();
    let fish = match &fish_schedule.placement {
        MapMakeResourcePlacementReceipt::FishCategoryOpen(fish) => fish,
        _ => panic!("empty FISH must stop at category cleanup"),
    };
    assert!(fish.fish_handoff.rows.is_empty());
    assert_eq!(fish.residual_va, CATEGORY_TAIL_VA);
    let random_before = fish.category_state_after.deterministic.random_state;
    let world_before = fish
        .category_state_after
        .deterministic
        .world_checksum
        .clone();
    let pool_before = pool.clone();
    let goodies_facts = goodies_category_facts(fish);

    let continued = continue_map_make_resource_schedule_empty_fish(
        &mut pool,
        &fish_schedule,
        &authority,
        &goodies_facts,
    )
    .unwrap();
    let goodies = match &continued.placement {
        MapMakeResourcePlacementReceipt::GoodiesCategoryOpen(goodies) => goodies,
        _ => panic!("empty FISH cleanup must produce the GOODIES handoff"),
    };

    assert_eq!(goodies.category_receipt.entry_va, CATEGORY_TAIL_VA);
    assert_eq!(
        goodies.category_receipt.completed_category,
        ResourceCategory::Fish
    );
    assert_eq!(
        goodies.category_state_after.category,
        ResourceCategory::Goodies
    );
    assert_eq!(goodies.goodies_handoff.rows.len(), 1);
    assert_eq!(goodies.goodies_handoff.rows[0].capture_ordinal, 51);
    assert_eq!(goodies.residual_va, ROW_BODY_VA);
    assert_eq!(
        goodies.category_state_after.deterministic.random_state,
        random_before
    );
    assert_eq!(
        goodies.category_state_after.deterministic.world_checksum,
        world_before
    );
    assert_eq!(pool, pool_before);
}

#[test]
fn fish_cleanup_rejects_detached_document_capture_and_preserves_pool() {
    let (mut pool, first_schedule) = chance_miss_first_bonus_schedule(1);
    let completed =
        continue_map_make_resource_schedule_bonus_category_tail(&mut pool, &first_schedule)
            .unwrap();
    let rows = match &completed.placement {
        MapMakeResourcePlacementReceipt::BonusRowsOpen(rows) => rows,
        _ => panic!("singleton BONUSES must reach category cleanup"),
    };
    let (facts, mut authority) = fish_category_inputs(&rows.remaining_state);
    authority.capture_sha256[0] ^= 1;
    let pool_before = pool.clone();

    assert_eq!(
        continue_map_make_resource_schedule_fish_category(
            &mut pool, &completed, &authority, &facts,
        ),
        Err(MapMakeResourceScheduleError::FishCategoryDocumentAuthorityMismatch)
    );
    assert_eq!(pool, pool_before);
}
