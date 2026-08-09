// SPDX-License-Identifier: GPL-3.0-or-later
//! Mutation-sensitive source-only proof for the deterministic `Map::place_resources` prefix.

#[path = "../src/place_resources_pool_frontier.rs"]
mod place_resources_pool_frontier;

use don_sim::systems::map_terrain::World;
use place_resources_pool_frontier::{
    execute_place_resources_pool_prefix, PlaceResourcesEntryHandoff, PlaceResourcesFactEvidence,
    PlaceResourcesLiveFacts, PlaceResourcesPoolError, RareGoodLiveFact, ResourceDivvyPoolState,
    ResourceGoodDisposition, ResourcePoolBitMask, DYNAMIC_BIT_MASK_INIT_VA, FIRST_SCANNED_GOOD_ID,
    GAME_CONST_REFERENCE_VA, GOOD_TYPES_REFERENCE_VA, LANDS_OCEAN_RARE_LIST_POINTER_SLOT_VA,
    MAP_PLACE_RESOURCES_ADD_GOOD_CALL_VA, MAP_PLACE_RESOURCES_CALL_VA,
    MAP_PLACE_RESOURCES_DONE_ADDING_CALL_VA, MAP_PLACE_RESOURCES_ENTRY_VA,
    MAP_PLACE_RESOURCES_PREFIX_RESIDUAL_VA, MAP_POST_TRANSITIONS_CHECKPOINT_CALL_VA,
    MAP_POST_TRANSITIONS_SOURCE_TOKEN, RESOURCE_DIVVY_ADD_GOOD_VA,
    RESOURCE_DIVVY_DONE_ADDING_GOODS_VA, SCANNED_GOOD_COUNT, SHIPPED_EXE_SHA256,
    SHIPPED_PDB_SHA256, WEIGHTED_WATER_GOOD_COPIES,
};

fn entry() -> PlaceResourcesEntryHandoff {
    let checksum = World::init_default_rules(4, 4).checksum_sections();
    PlaceResourcesEntryHandoff {
        checkpoint_call_va: MAP_POST_TRANSITIONS_CHECKPOINT_CALL_VA,
        source_token: MAP_POST_TRANSITIONS_SOURCE_TOKEN,
        world_checksum: checksum,
        sourced_walked_bytes: 52,
        call_va: MAP_PLACE_RESOURCES_CALL_VA,
        entry_va: MAP_PLACE_RESOURCES_ENTRY_VA,
        player_count_argument: 4,
        random_state_at_entry: 0x1234_5678,
    }
}

fn catalog() -> Vec<RareGoodLiveFact> {
    (0..SCANNED_GOOD_COUNT)
        .map(|ordinal| RareGoodLiveFact {
            good_id: FIRST_SCANNED_GOOD_ID + ordinal as i32,
            resolved_age: 0,
            exclude_ctw: 0,
            random_rare_drop: 0,
        })
        .collect()
}

fn facts(entry: &PlaceResourcesEntryHandoff) -> PlaceResourcesLiveFacts {
    PlaceResourcesLiveFacts {
        conquer_the_world: false,
        goods: catalog(),
        ocean_rare_good_ids: Vec::new(),
        evidence: PlaceResourcesFactEvidence::SyntheticFixture {
            fixture: "place-resources-pool-frontier".to_owned(),
            checkpoint_call_va: entry.checkpoint_call_va,
            source_token: entry.source_token,
            call_va: entry.call_va,
            entry_va: entry.entry_va,
            world_checksum: entry.world_checksum.clone(),
            sourced_walked_bytes: entry.sourced_walked_bytes,
            random_state_at_entry: entry.random_state_at_entry,
        },
    }
}

fn poisoned_pool() -> ResourceDivvyPoolState {
    ResourceDivvyPoolState {
        early_bits: ResourcePoolBitMask {
            bits: 99,
            bytes: vec![0xff; 13],
        },
        early_goods: vec![99, 98],
        late_bits: ResourcePoolBitMask {
            bits: 1,
            bytes: vec![0xff],
        },
        late_goods: vec![97],
        water_bits: ResourcePoolBitMask {
            bits: 17,
            bytes: vec![0xff; 3],
        },
        water_goods: vec![96],
    }
}

#[test]
fn classifies_exact_scan_in_early_late_and_weighted_water_order() {
    let entry = entry();
    let mut facts = facts(&entry);
    facts.goods[0].random_rare_drop = 1; // good 6, water, ten copies
    facts.goods[1].random_rare_drop = 1; // good 7, early
    facts.goods[2].random_rare_drop = 1; // good 8, late
    facts.goods[2].resolved_age = 3;
    facts.goods[3].random_rare_drop = 1; // good 9, water, one copy
    facts.goods[3].resolved_age = 99; // water membership wins before age
    facts.ocean_rare_good_ids = vec![6, 9];

    let mut pool = ResourceDivvyPoolState::default();
    let receipt = execute_place_resources_pool_prefix(&mut pool, &entry, &facts).unwrap();

    assert_eq!(pool.early_goods, vec![7]);
    assert_eq!(pool.late_goods, vec![8]);
    assert_eq!(
        pool.water_goods,
        [vec![6; WEIGHTED_WATER_GOOD_COPIES], vec![9]].concat()
    );
    assert_eq!(receipt.add_good_calls, 4);
    assert_eq!(receipt.decisions.len(), SCANNED_GOOD_COUNT);
    assert_eq!(
        receipt.decisions[0].disposition,
        ResourceGoodDisposition::Water { copies: 10 }
    );
    assert_eq!(
        receipt.decisions[1].disposition,
        ResourceGoodDisposition::Early
    );
    assert_eq!(
        receipt.decisions[2].disposition,
        ResourceGoodDisposition::Late
    );
    assert_eq!(
        receipt.decisions[3].disposition,
        ResourceGoodDisposition::Water { copies: 1 }
    );
    assert_eq!(
        receipt.decisions[0].add_good_call_va,
        Some(MAP_PLACE_RESOURCES_ADD_GOOD_CALL_VA)
    );
    assert_eq!(
        receipt.decisions[0].add_good_callee_va,
        Some(RESOURCE_DIVVY_ADD_GOOD_VA)
    );
}

#[test]
fn conquest_exclusion_precedes_random_rare_filter() {
    let entry = entry();
    let mut facts = facts(&entry);
    facts.conquer_the_world = true;
    facts.goods[0].exclude_ctw = 1;
    facts.goods[0].random_rare_drop = 1;
    facts.goods[1].exclude_ctw = 1;
    facts.goods[1].random_rare_drop = 0;
    facts.goods[2].exclude_ctw = 0;
    facts.goods[2].random_rare_drop = 0;
    facts.goods[3].exclude_ctw = 0;
    facts.goods[3].random_rare_drop = -7;

    let mut pool = ResourceDivvyPoolState::default();
    let receipt = execute_place_resources_pool_prefix(&mut pool, &entry, &facts).unwrap();
    assert_eq!(
        receipt.decisions[0].disposition,
        ResourceGoodDisposition::SkippedConquestExclusion
    );
    assert_eq!(
        receipt.decisions[1].disposition,
        ResourceGoodDisposition::SkippedConquestExclusion
    );
    assert_eq!(
        receipt.decisions[2].disposition,
        ResourceGoodDisposition::SkippedNotRandomRare
    );
    assert_eq!(
        receipt.decisions[3].disposition,
        ResourceGoodDisposition::Early
    );
    assert_eq!(receipt.add_good_calls, 1);

    facts.conquer_the_world = false;
    let receipt = execute_place_resources_pool_prefix(&mut pool, &entry, &facts).unwrap();
    assert_eq!(
        receipt.decisions[0].disposition,
        ResourceGoodDisposition::Early
    );
    assert_eq!(
        receipt.decisions[1].disposition,
        ResourceGoodDisposition::SkippedNotRandomRare
    );
}

#[test]
fn done_adding_recreates_ceil_div_eight_masks_and_clears_twice() {
    let entry = entry();
    let mut facts = facts(&entry);
    for ordinal in 0..17 {
        facts.goods[ordinal].random_rare_drop = 1;
        facts.goods[ordinal].resolved_age = if ordinal < 9 { 2 } else { 3 };
    }
    facts.ocean_rare_good_ids = vec![15];

    let mut pool = ResourceDivvyPoolState {
        early_bits: ResourcePoolBitMask {
            bits: 99,
            bytes: vec![0xff; 13],
        },
        late_bits: ResourcePoolBitMask {
            bits: 1,
            bytes: vec![0xff],
        },
        water_bits: ResourcePoolBitMask {
            bits: 17,
            bytes: vec![0xff; 3],
        },
        ..ResourceDivvyPoolState::default()
    };
    let receipt = execute_place_resources_pool_prefix(&mut pool, &entry, &facts).unwrap();
    assert_eq!(
        pool.early_bits,
        ResourcePoolBitMask {
            bits: 9,
            bytes: vec![0, 0]
        }
    );
    assert_eq!(
        pool.late_bits,
        ResourcePoolBitMask {
            bits: 7,
            bytes: vec![0]
        }
    );
    assert_eq!(
        pool.water_bits,
        ResourcePoolBitMask {
            bits: 1,
            bytes: vec![0]
        }
    );
    assert_eq!(
        receipt.done_adding_call_va,
        MAP_PLACE_RESOURCES_DONE_ADDING_CALL_VA
    );
    assert_eq!(
        receipt.done_adding_callee_va,
        RESOURCE_DIVVY_DONE_ADDING_GOODS_VA
    );
    assert_eq!(receipt.bit_mask_init_va, DYNAMIC_BIT_MASK_INIT_VA);
    assert_eq!(receipt.bit_mask_clear_passes, 2);
}

#[test]
fn prefix_has_exact_boundary_and_owns_neither_rng_nor_world_checksum() {
    let entry = entry();
    let mut facts = facts(&entry);
    facts.goods[0].random_rare_drop = 1;
    let mut pool = ResourceDivvyPoolState::default();
    let receipt = execute_place_resources_pool_prefix(&mut pool, &entry, &facts).unwrap();

    assert_eq!(receipt.upstream_source_token, 0x1ebe);
    assert_eq!(receipt.call_va, 0x0068_c707);
    assert_eq!(receipt.entry_va, 0x0068_f4f0);
    assert_eq!(
        receipt.prefix_residual_va,
        MAP_PLACE_RESOURCES_PREFIX_RESIDUAL_VA
    );
    assert_eq!(receipt.random_state_before, entry.random_state_at_entry);
    assert_eq!(receipt.random_state_after, entry.random_state_at_entry);
    assert_eq!(receipt.random_draws, 0);
    assert_eq!(receipt.world_checksum_before, entry.world_checksum);
    assert_eq!(receipt.world_checksum_after, entry.world_checksum);
    assert_eq!(receipt.sourced_walked_bytes_before, 52);
    assert_eq!(receipt.sourced_walked_bytes_after, 52);
}

#[test]
fn repeated_native_invocation_appends_goods_before_rebuilding_masks() {
    let entry = entry();
    let mut facts = facts(&entry);
    facts.goods[0].random_rare_drop = 1;
    facts.goods[1].random_rare_drop = 1;
    facts.goods[1].resolved_age = 3;
    let mut pool = ResourceDivvyPoolState {
        early_goods: vec![40],
        late_goods: vec![41],
        water_goods: vec![42],
        ..ResourceDivvyPoolState::default()
    };

    execute_place_resources_pool_prefix(&mut pool, &entry, &facts).unwrap();
    assert_eq!(pool.early_goods, vec![40, 6]);
    assert_eq!(pool.late_goods, vec![41, 7]);
    assert_eq!(pool.water_goods, vec![42]);
    assert_eq!(pool.early_bits.bits, 2);
    assert_eq!(pool.late_bits.bits, 2);
    assert_eq!(pool.water_bits.bits, 1);
}

#[test]
fn malformed_catalogs_and_ocean_list_roll_back_authoritative_pool() {
    let checkpoint = entry();
    let original = poisoned_pool();

    let mut missing_catalog = facts(&checkpoint);
    missing_catalog.goods.pop();
    let mut pool = original.clone();
    assert_eq!(
        execute_place_resources_pool_prefix(&mut pool, &checkpoint, &missing_catalog),
        Err(PlaceResourcesPoolError::GoodCatalogLength { observed: 43 })
    );
    assert_eq!(pool, original);

    let mut shifted_catalog = facts(&checkpoint);
    shifted_catalog.goods[7].good_id += 1;
    assert!(matches!(
        execute_place_resources_pool_prefix(&mut pool, &checkpoint, &shifted_catalog),
        Err(PlaceResourcesPoolError::GoodCatalogId { ordinal: 7, .. })
    ));
    assert_eq!(pool, original);

    let mut oversized_ocean_list = facts(&checkpoint);
    oversized_ocean_list.ocean_rare_good_ids = vec![6; 45];
    assert_eq!(
        execute_place_resources_pool_prefix(&mut pool, &checkpoint, &oversized_ocean_list),
        Err(PlaceResourcesPoolError::OceanRareListTooLong { observed: 45 })
    );
    assert_eq!(pool, original);
}

#[test]
fn entry_and_evidence_mismatches_roll_back_authoritative_pool() {
    let mut bad_entry = entry();
    let entry_bound_facts = facts(&bad_entry);
    let original = poisoned_pool();
    let mut pool = original.clone();
    bad_entry.call_va += 1;
    assert_eq!(
        execute_place_resources_pool_prefix(&mut pool, &bad_entry, &entry_bound_facts),
        Err(PlaceResourcesPoolError::EntryHandoffMismatch)
    );
    assert_eq!(pool, original);

    let checkpoint = entry();
    let mut bad_evidence = facts(&checkpoint);
    if let PlaceResourcesFactEvidence::SyntheticFixture {
        random_state_at_entry: ref mut captured_random_state,
        ..
    } = bad_evidence.evidence
    {
        *captured_random_state ^= 1;
    }
    assert_eq!(
        execute_place_resources_pool_prefix(&mut pool, &checkpoint, &bad_evidence),
        Err(PlaceResourcesPoolError::LiveFactsUnavailableOrMismatched)
    );
    assert_eq!(pool, original);
}

#[test]
fn retail_evidence_requires_exact_binary_pdb_globals_and_nonzero_capture() {
    let entry = entry();
    let mut facts = facts(&entry);
    facts.goods[0].random_rare_drop = 1;
    facts.evidence = PlaceResourcesFactEvidence::RetailCapture {
        executable_sha256: SHIPPED_EXE_SHA256.to_owned(),
        pdb_sha256: SHIPPED_PDB_SHA256.to_owned(),
        capture_sha256: [0x5a; 32],
        game_const_reference_va: GAME_CONST_REFERENCE_VA,
        good_types_reference_va: GOOD_TYPES_REFERENCE_VA,
        lands_ocean_rare_list_pointer_slot_va: LANDS_OCEAN_RARE_LIST_POINTER_SLOT_VA,
        checkpoint_call_va: entry.checkpoint_call_va,
        source_token: entry.source_token,
        call_va: entry.call_va,
        entry_va: entry.entry_va,
        world_checksum: entry.world_checksum.clone(),
        sourced_walked_bytes: entry.sourced_walked_bytes,
        random_state_at_entry: entry.random_state_at_entry,
    };

    let mut pool = ResourceDivvyPoolState::default();
    execute_place_resources_pool_prefix(&mut pool, &entry, &facts).unwrap();

    if let PlaceResourcesFactEvidence::RetailCapture { capture_sha256, .. } = &mut facts.evidence {
        *capture_sha256 = [0; 32];
    }
    assert_eq!(
        execute_place_resources_pool_prefix(&mut pool, &entry, &facts),
        Err(PlaceResourcesPoolError::LiveFactsUnavailableOrMismatched)
    );
}

#[test]
fn every_live_fact_axis_is_mutation_sensitive() {
    let entry = entry();
    let mut facts = facts(&entry);
    facts.goods[4].random_rare_drop = 1;
    facts.goods[4].resolved_age = 2;
    let mut pool = ResourceDivvyPoolState::default();
    let baseline = execute_place_resources_pool_prefix(&mut pool, &entry, &facts)
        .unwrap()
        .pool_after;

    facts.goods[4].resolved_age = 3;
    let age_mutant = execute_place_resources_pool_prefix(&mut pool, &entry, &facts)
        .unwrap()
        .pool_after;
    assert_ne!(baseline, age_mutant);

    facts.goods[4].resolved_age = 2;
    facts.ocean_rare_good_ids.push(facts.goods[4].good_id);
    let land_mutant = execute_place_resources_pool_prefix(&mut pool, &entry, &facts)
        .unwrap()
        .pool_after;
    assert_ne!(baseline, land_mutant);

    facts.ocean_rare_good_ids.clear();
    facts.conquer_the_world = true;
    facts.goods[4].exclude_ctw = 1;
    let conquest_mutant = execute_place_resources_pool_prefix(&mut pool, &entry, &facts)
        .unwrap()
        .pool_after;
    assert_ne!(baseline, conquest_mutant);
}
