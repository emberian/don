// SPDX-License-Identifier: GPL-3.0-or-later
//! Mutation-sensitive proof for the post-BONUSES category and placement prefixes.

#[path = "../src/place_resources_category_frontier.rs"]
mod place_resources_category_frontier;

use don_sim::rng::Random;
use don_sim::systems::map_terrain::World;
use place_resources_category_frontier::*;

fn deterministic() -> DeterministicResourceState {
    DeterministicResourceState {
        random_state: 0x1234_5678,
        world_checksum: World::init_default_rules(4, 4).checksum_sections(),
        sourced_walked_bytes: 0x1234,
        resource_pool_digest: 0x1122_3344_5566_7788,
        allocated_resources: 17,
        requested_resources: 23,
        last_chance_group: 7,
        signed_chance_budget: -14,
        winner_seen: true,
    }
}

fn evidence(entry_va: u32, state: &DeterministicResourceState) -> ResourceFrontierEvidence {
    ResourceFrontierEvidence::SyntheticFixture {
        fixture: "place-resources-category-frontier".to_owned(),
        entry_va,
        random_state: state.random_state,
        world_checksum: state.world_checksum.clone(),
        sourced_walked_bytes: state.sourced_walked_bytes,
        resource_pool_digest: state.resource_pool_digest,
    }
}

fn handles(bits: u8) -> HostHandles {
    HostHandles {
        head: bits & 1 != 0,
        tail: bits & 2 != 0,
        inline_tail_word: bits & 4 != 0,
    }
}

fn section(name: &str, ordinals: &[u32], section_handles: HostHandles) -> CategorySectionFact {
    CategorySectionFact {
        lookup_token: name.to_owned(),
        returned_element_name: name.trim_end().to_owned(),
        handles: section_handles,
        rows: ordinals
            .iter()
            .map(|ordinal| CategoryRowFact {
                capture_ordinal: *ordinal,
                element_name: "BONUS".to_owned(),
                handles: handles((*ordinal & 7) as u8),
            })
            .collect(),
    }
}

fn category_state(category: ResourceCategory) -> CategoryLoopState {
    CategoryLoopState {
        category,
        rows_remaining: 0,
        category_handles: handles(3),
        row_handles: handles(3),
        selected_document_handles: handles(3),
        default_document_handles: handles(3),
        deterministic: deterministic(),
    }
}

#[test]
fn bonuses_tail_preserves_cross_category_chance_carry_and_uses_exact_fish_tokens() {
    assert_eq!(BONUSES_STRING_OFFSET, 0x17a98);
    assert_eq!(BONUS_ROW_STRING_OFFSET, 0x17ae8);
    let mut state = category_state(ResourceCategory::Bonuses);
    let before = state.deterministic.clone();
    let facts = CategoryAdvanceFacts {
        selected_style_name_nonempty: true,
        // This fixture exercises fallback; the separate selected-token test proves
        // that the query token and returned element name are distinct facts.
        selected_section: None,
        default_section: Some(section("FISH", &[90, 12], handles(3))),
        evidence: evidence(CATEGORY_TAIL_VA, &state.deterministic),
    };
    let receipt = advance_completed_category(&mut state, &facts).unwrap();

    assert_eq!(state.category, ResourceCategory::Fish);
    assert_eq!(state.rows_remaining, 2);
    assert_eq!(state.deterministic, before);
    assert_eq!(receipt.selected_lookup_call_va, Some(0x0068_f8f6));
    assert_eq!(receipt.selected_lookup_string_offset, Some(0x17aac));
    assert_eq!(receipt.default_lookup_call_va, Some(0x0068_f9b0));
    assert_eq!(receipt.default_lookup_string_offset, Some(0x17ac0));
    assert_eq!(receipt.row_enumerate_call_va, Some(0x0068_fb98));
    assert_eq!(receipt.row_count_test_va, Some(0x0068_fba8));
    assert!(matches!(
        receipt.disposition,
        CategoryAdvanceDisposition::NextCategory {
            category: ResourceCategory::Fish,
            source: SectionSource::Default,
            residual_va: ROW_BODY_VA,
            ..
        }
    ));
    assert_eq!(
        receipt
            .cleanup_operations
            .iter()
            .take(4)
            .map(|operation| operation.call_va)
            .collect::<Vec<_>>(),
        CATEGORY_RELEASE_CALL_VAS
    );
}

#[test]
fn selected_fish_lookup_keeps_the_query_token_distinct_from_the_returned_name() {
    let mut selected_state = category_state(ResourceCategory::Bonuses);
    let selected_facts = CategoryAdvanceFacts {
        selected_style_name_nonempty: true,
        selected_section: Some(section("FISH ", &[1], handles(3))),
        default_section: Some(section("FISH", &[2], handles(3))),
        evidence: evidence(CATEGORY_TAIL_VA, &selected_state.deterministic),
    };
    let receipt = advance_completed_category(&mut selected_state, &selected_facts).unwrap();
    assert!(matches!(
        receipt.disposition,
        CategoryAdvanceDisposition::NextCategory {
            source: SectionSource::Selected,
            ..
        }
    ));

    let mut state = category_state(ResourceCategory::Bonuses);
    let facts = CategoryAdvanceFacts {
        selected_style_name_nonempty: true,
        selected_section: Some(section("FISH", &[1], handles(3))),
        default_section: Some(section("FISH", &[2], handles(3))),
        evidence: evidence(CATEGORY_TAIL_VA, &state.deterministic),
    };
    let before = state.clone();
    assert_eq!(
        advance_completed_category(&mut state, &facts),
        Err(CategoryAdvanceError::WrongLookupToken {
            expected: "FISH ",
            actual: "FISH".to_owned(),
        })
    );
    assert_eq!(state, before);
}

#[test]
fn empty_fish_falls_directly_through_to_the_same_category_tail() {
    let mut state = category_state(ResourceCategory::Bonuses);
    let facts = CategoryAdvanceFacts {
        selected_style_name_nonempty: false,
        selected_section: None,
        default_section: Some(section("FISH", &[], handles(4))),
        evidence: evidence(CATEGORY_TAIL_VA, &state.deterministic),
    };
    let receipt = advance_completed_category(&mut state, &facts).unwrap();
    assert_eq!(state.category, ResourceCategory::Fish);
    assert_eq!(state.rows_remaining, 0);
    assert_eq!(receipt.selected_lookup_call_va, None);
    assert!(matches!(
        receipt.disposition,
        CategoryAdvanceDisposition::NextCategory {
            residual_va: CATEGORY_TAIL_VA,
            rows,
            ..
        } if rows.is_empty()
    ));
}

#[test]
fn fish_tail_selects_goodies_without_resetting_the_budget_or_rng() {
    let mut state = category_state(ResourceCategory::Fish);
    let before = state.deterministic.clone();
    let facts = CategoryAdvanceFacts {
        selected_style_name_nonempty: true,
        selected_section: Some(section("GOODIES", &[3, 4, 5], handles(5))),
        default_section: Some(section("GOODIES", &[99], handles(3))),
        evidence: evidence(CATEGORY_TAIL_VA, &state.deterministic),
    };
    let receipt = advance_completed_category(&mut state, &facts).unwrap();
    assert_eq!(state.category, ResourceCategory::Goodies);
    assert_eq!(state.rows_remaining, 3);
    assert_eq!(state.deterministic, before);
    assert_eq!(receipt.selected_lookup_call_va, Some(0x0068_f79b));
    assert_eq!(receipt.selected_lookup_string_offset, Some(0x17ad4));
    assert_eq!(receipt.default_lookup_call_va, None);
    assert!(matches!(
        receipt.disposition,
        CategoryAdvanceDisposition::NextCategory {
            category: ResourceCategory::Goodies,
            source: SectionSource::Selected,
            ..
        }
    ));
}

#[test]
fn goodies_tail_releases_documents_and_returns_the_actual_allocated_count() {
    let mut state = category_state(ResourceCategory::Goodies);
    let before = state.deterministic.clone();
    let facts = CategoryAdvanceFacts {
        selected_style_name_nonempty: true,
        selected_section: None,
        default_section: None,
        evidence: evidence(CATEGORY_TAIL_VA, &state.deterministic),
    };
    let receipt = advance_completed_category(&mut state, &facts).unwrap();
    assert_eq!(state.deterministic, before);
    assert_eq!(state.selected_document_handles, HostHandles::default());
    assert_eq!(state.default_document_handles, HostHandles::default());
    assert_eq!(receipt.category_dispatch_va, None);
    assert!(matches!(
        receipt.disposition,
        CategoryAdvanceDisposition::Returned {
            return_count: 17,
            return_va: FUNCTION_RETURN_VA,
        }
    ));
    assert_eq!(
        receipt
            .cleanup_operations
            .iter()
            .rev()
            .take(4)
            .map(|operation| operation.call_va)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>(),
        DOCUMENT_RELEASE_CALL_VAS
    );
}

#[test]
fn category_rejection_is_atomic_for_rows_or_stale_evidence() {
    let mut state = category_state(ResourceCategory::Bonuses);
    state.rows_remaining = 1;
    let facts = CategoryAdvanceFacts {
        selected_style_name_nonempty: false,
        selected_section: None,
        default_section: None,
        evidence: evidence(CATEGORY_TAIL_VA, &state.deterministic),
    };
    let before = state.clone();
    assert_eq!(
        advance_completed_category(&mut state, &facts),
        Err(CategoryAdvanceError::RowsRemain)
    );
    assert_eq!(state, before);

    state.rows_remaining = 0;
    let mut stale = facts;
    if let ResourceFrontierEvidence::SyntheticFixture { random_state, .. } = &mut stale.evidence {
        *random_state ^= 1;
    }
    let before = state.clone();
    assert_eq!(
        advance_completed_category(&mut state, &stale),
        Err(CategoryAdvanceError::StaleEvidence)
    );
    assert_eq!(state, before);
}

#[test]
fn retail_evidence_requires_the_supported_exe_pdb_and_a_nonzero_capture() {
    let mut state = category_state(ResourceCategory::Bonuses);
    let mut facts = CategoryAdvanceFacts {
        selected_style_name_nonempty: false,
        selected_section: None,
        default_section: Some(section("FISH", &[], handles(4))),
        evidence: ResourceFrontierEvidence::RetailCapture {
            executable_sha256: SHIPPED_EXE_SHA256.to_owned(),
            pdb_sha256: SHIPPED_PDB_SHA256.to_owned(),
            capture_sha256: [0x5a; 32],
            entry_va: CATEGORY_TAIL_VA,
            random_state: state.deterministic.random_state,
            world_checksum: state.deterministic.world_checksum.clone(),
            sourced_walked_bytes: state.deterministic.sourced_walked_bytes,
            resource_pool_digest: state.deterministic.resource_pool_digest,
        },
    };
    advance_completed_category(&mut state, &facts).unwrap();

    let mut state = category_state(ResourceCategory::Bonuses);
    if let ResourceFrontierEvidence::RetailCapture {
        executable_sha256, ..
    } = &mut facts.evidence
    {
        executable_sha256.replace_range(..1, "f");
    }
    let before = state.clone();
    assert_eq!(
        advance_completed_category(&mut state, &facts),
        Err(CategoryAdvanceError::StaleEvidence)
    );
    assert_eq!(state, before);
}

fn region_facts(state: &DeterministicResourceState) -> RegionPlacementPrefixFacts {
    RegionPlacementPrefixFacts {
        ocean_rare_good_ids: vec![6, 31],
        regions: (1..=126)
            .map(|region_id| RegionPrefixFact {
                region_id,
                land_cell_count: 0,
                contains_player_start: false,
                candidate_point_count: 0,
            })
            .collect(),
        evidence: evidence(MAP_PLACE_REGION_RESOURCE_VA, state),
    }
}

fn random_draw(state: i32, modulus: i32) -> (i32, i32) {
    let mut random = Random::new(state);
    let raw = random.get(0, 0xffff);
    (raw % modulus, random.state())
}

#[test]
fn region_prefix_includes_the_transitive_find_avail_rng_before_point_rng() {
    let state = deterministic();
    let mut facts = region_facts(&state);
    for region_id in [1, 2, 3] {
        facts.regions[region_id - 1].land_cell_count = 6;
        facts.regions[region_id - 1].candidate_point_count = 5;
    }
    let request = RegionPlacementPrefixRequest {
        good_id: 10,
        pattern: 0,
        saturate: 0,
        num_rare: 4,
        selector: 0,
    };
    let receipt = execute_region_placement_prefix(&state, request, &facts).unwrap();

    // Ascending scans are prepended by LinkList::add.
    assert_eq!(receipt.available_regions, vec![3, 2, 1]);
    let (region_index, after_find) = random_draw(state.random_state, 3);
    let (point_index, after_point) = random_draw(after_find, 5);
    let find_draw = receipt.find_avail_draw.as_ref().unwrap();
    let point_draw = receipt.point_draw.as_ref().unwrap();
    assert_eq!(find_draw.call_va, 0x0068_f498);
    assert_eq!(find_draw.remainder, region_index);
    assert_eq!(point_draw.call_va, 0x0069_071a);
    assert_eq!(point_draw.state_before, after_find);
    assert_eq!(point_draw.remainder, point_index);
    assert_eq!(receipt.random_state_after, after_point);
    assert_eq!(receipt.per_region_limit, 2);
    assert!(matches!(
        receipt.disposition,
        RegionPrefixDisposition::CandidateScan {
            residual_va: REGION_CANDIDATE_SCAN_VA,
            point_index: p,
            ..
        } if p == point_index
    ));
    assert_eq!(receipt.world_checksum, state.world_checksum);
}

#[test]
fn region_water_classification_changes_the_exact_region_id_domain() {
    let state = deterministic();
    let mut facts = region_facts(&state);
    facts.regions[0].land_cell_count = 99;
    facts.regions[63].land_cell_count = 6;
    facts.regions[63].candidate_point_count = 1;
    let receipt = execute_region_placement_prefix(
        &state,
        RegionPlacementPrefixRequest {
            good_id: 6,
            pattern: 0,
            saturate: 1,
            num_rare: 1,
            selector: 0,
        },
        &facts,
    )
    .unwrap();
    assert!(receipt.water_good);
    assert_eq!(receipt.available_regions, vec![64]);
    assert!(receipt.find_avail_draw.is_none());
    assert!(receipt.point_draw.is_none());
    assert_eq!(receipt.random_state_after, state.random_state);
}

#[test]
fn nonplayer_pattern_excludes_regions_containing_player_starts() {
    let state = deterministic();
    let mut facts = region_facts(&state);
    for region_id in [7, 8] {
        facts.regions[region_id - 1].land_cell_count = 6;
        facts.regions[region_id - 1].candidate_point_count = 1;
    }
    facts.regions[7].contains_player_start = true;
    let receipt = execute_region_placement_prefix(
        &state,
        RegionPlacementPrefixRequest {
            good_id: 10,
            pattern: 3,
            saturate: 0,
            num_rare: 1,
            selector: 0,
        },
        &facts,
    )
    .unwrap();
    assert_eq!(receipt.available_regions, vec![7]);
}

#[test]
fn region_selector_stops_after_find_avail_rng_before_unknown_pool_draws() {
    let state = deterministic();
    let mut facts = region_facts(&state);
    for region_id in [10, 11] {
        facts.regions[region_id - 1].land_cell_count = 6;
        facts.regions[region_id - 1].candidate_point_count = 9;
    }
    let receipt = execute_region_placement_prefix(
        &state,
        RegionPlacementPrefixRequest {
            good_id: -1,
            pattern: 0,
            saturate: 0,
            num_rare: 2,
            selector: 2,
        },
        &facts,
    )
    .unwrap();
    assert!(receipt.find_avail_draw.is_some());
    assert!(receipt.point_draw.is_none());
    assert!(matches!(
        receipt.disposition,
        RegionPrefixDisposition::PoolSelectionBoundary {
            call_va: REGION_POOL_SELECTION_VA,
            ..
        }
    ));
}

fn player_facts(state: &DeterministicResourceState) -> PlayerPlacementPrefixFacts {
    PlayerPlacementPrefixFacts {
        world_width: 64,
        world_height: 64,
        starts: vec![PlayerStartFact { x: 10, y: 12 }],
        // A compact monotone fixture; retail capture supplies the exact table bytes.
        spiral_ring_ends: (0..=64).map(|radius| radius * radius).collect(),
        evidence: evidence(MAP_PLACE_PLAYER_RESOURCE_VA, state),
    }
}

#[test]
fn player_prefix_clamps_ring_bounds_and_uses_span_plus_one_rng_modulus() {
    let state = deterministic();
    let facts = player_facts(&state);
    let receipt = execute_player_placement_prefix(
        &state,
        PlayerPlacementPrefixRequest {
            player_count: 1,
            good_id: 6,
            num_rare: 1,
            spacing: 9,
            group_spacing: 5,
            selector: 0,
        },
        &facts,
    )
    .unwrap();
    assert!(receipt.water_good);
    assert_eq!(receipt.spacing_after_clamp, 5);
    assert_eq!(receipt.group_spacing_after_clamp, 5);
    assert_eq!(receipt.candidate_span, 0);
    assert!(receipt.point_draw.is_none());

    let receipt = execute_player_placement_prefix(
        &state,
        PlayerPlacementPrefixRequest {
            player_count: 1,
            good_id: 31,
            num_rare: 1,
            spacing: 2,
            group_spacing: 4,
            selector: 0,
        },
        &facts,
    )
    .unwrap();
    let span = 16 - 4;
    assert_eq!(receipt.candidate_span, span);
    assert_eq!(receipt.point_draw.as_ref().unwrap().modulus, span + 1);
    let start = receipt.point_draw.as_ref().unwrap().remainder;
    assert!(matches!(
        receipt.disposition,
        PlayerPrefixDisposition::CandidateScan {
            residual_va: PLAYER_CANDIDATE_SCAN_VA,
            first_candidate_table_index,
        } if first_candidate_table_index == start % span + 4
    ));
}

#[test]
fn player_group_spacing_zero_and_above_64_both_clamp_to_64() {
    let state = deterministic();
    let facts = player_facts(&state);
    for group_spacing in [0, 65] {
        let receipt = execute_player_placement_prefix(
            &state,
            PlayerPlacementPrefixRequest {
                player_count: 1,
                good_id: 10,
                num_rare: 1,
                spacing: 64,
                group_spacing,
                selector: 0,
            },
            &facts,
        )
        .unwrap();
        assert_eq!(receipt.group_spacing_after_clamp, 64);
        assert_eq!(receipt.spacing_after_clamp, 64);
        assert!(receipt.point_draw.is_none());
    }
}

#[test]
fn player_prefix_rejects_bad_start_and_stops_before_pool_selector() {
    let state = deterministic();
    let mut facts = player_facts(&state);
    facts.starts[0].x = -1;
    let receipt = execute_player_placement_prefix(
        &state,
        PlayerPlacementPrefixRequest {
            player_count: 1,
            good_id: 10,
            num_rare: 1,
            spacing: 1,
            group_spacing: 2,
            selector: 0,
        },
        &facts,
    )
    .unwrap();
    assert!(matches!(
        receipt.disposition,
        PlayerPrefixDisposition::InvalidFirstStart {
            path_va: PLAYER_INVALID_START_PATH_VA,
        }
    ));
    assert_eq!(receipt.random_state_after, state.random_state);

    facts.starts[0].x = 1;
    let receipt = execute_player_placement_prefix(
        &state,
        PlayerPlacementPrefixRequest {
            player_count: 1,
            good_id: -1,
            num_rare: 1,
            spacing: 1,
            group_spacing: 2,
            selector: 3,
        },
        &facts,
    )
    .unwrap();
    assert!(matches!(
        receipt.disposition,
        PlayerPrefixDisposition::PoolSelectionBoundary {
            call_va: PLAYER_POOL_SELECTION_VA,
        }
    ));
    assert!(receipt.point_draw.is_none());
}
