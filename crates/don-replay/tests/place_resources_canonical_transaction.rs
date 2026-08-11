// SPDX-License-Identifier: GPL-3.0-or-later
//! Focused transaction proof for the supported canonical resource categories.

#[path = "../src/place_resources_pool_frontier.rs"]
pub mod place_resources_pool_frontier;

#[path = "../src/resource_divvy_pool_selection_frontier.rs"]
pub mod resource_divvy_pool_selection_frontier;

#[path = "../src/place_resources_xml_frontier.rs"]
pub mod place_resources_xml_frontier;

#[path = "../src/place_resources_category_frontier.rs"]
pub mod place_resources_category_frontier;

#[path = "../src/place_resources_bonus_mutation_frontier.rs"]
pub mod place_resources_bonus_mutation_frontier;

#[path = "../src/place_resources_bonus_rows_mutation_frontier.rs"]
pub mod place_resources_bonus_rows_mutation_frontier;

#[path = "../src/place_region_resource_world_body_frontier.rs"]
pub mod place_region_resource_world_body_frontier;

#[path = "../src/place_player_resource_body_frontier.rs"]
pub mod place_player_resource_body_frontier;

#[path = "../src/place_resources_canonical_transaction.rs"]
mod place_resources_canonical_transaction;

use don_sim::systems::map_terrain::World;
use place_player_resource_body_frontier::{
    player_placement_prefix_receipt_digest, player_resource_body_facts_digest, GoodPlacementFact,
    PlayerAllocationHost, PlayerAllocationReceipt, PlayerAllocationRequest,
    PlayerBodyPlacementEvidence, PlayerResourceBodyEvidence, PlayerResourceBodyFacts,
    PlayerResourceBodyParameters, PlayerResourceBodyState,
};
use place_region_resource_world_body_frontier::{
    region_placement_prefix_receipt_digest, region_world_body_facts_digest, DownChainFact,
    RegionBodyPlacementEvidence, RegionCandidateSetFact, RegionInitGoodHost, RegionInitGoodReceipt,
    RegionInitGoodRequest, RegionWorldBodyEvidence, RegionWorldBodyFacts,
    RegionWorldBodyParameters, RegionWorldBodyState, RegionWorldCellFact, TerrainRareFact,
};
use place_resources_bonus_mutation_frontier::{
    BonusMutationEvidence, FirstBonusMutationFacts, PlaceResourcesBonusMutationState,
    PlacementPattern, ResourceTypeResolution, ScaledAttribute, ScaledAttributeFact,
    CENTER_KEEP_AWAY_SCALE_CALL_VA, CENTER_STAY_NEAR_SCALE_CALL_VA, CORNER_KEEP_AWAY_SCALE_CALL_VA,
    CORNER_STAY_NEAR_SCALE_CALL_VA, EDGE_KEEP_AWAY_SCALE_CALL_VA, EDGE_STAY_NEAR_SCALE_CALL_VA,
    FIRST_BONUS_ROW_BODY_VA, GROUP_SPACING_SCALE_CALL_VA, NUM_RARE_SCALE_CALL_VA,
    PLAYER_KEEP_AWAY_SCALE_CALL_VA, PLAYER_STAY_NEAR_SCALE_CALL_VA,
};
use place_resources_bonus_rows_mutation_frontier::{
    later_bonus_facts_digest, LaterBonusMutationEvidence, LaterBonusMutationFacts,
    CARRIED_CATEGORY_FIRST_ENTRY_VA, LATER_ROW_MUTATION_ENTRY_VA,
};
use place_resources_canonical_transaction::*;
use place_resources_category_frontier::{
    CategoryAdvanceFacts, CategoryRowFact, CategorySectionFact, HostHandles,
    PlayerPlacementPrefixFacts, PlayerPlacementPrefixRequest, PlayerStartFact,
    RegionPlacementPrefixFacts, RegionPlacementPrefixRequest, RegionPrefixFact, ResourceCategory,
    ResourceFrontierEvidence, CATEGORY_TAIL_VA, MAP_PLACE_PLAYER_RESOURCE_VA,
    MAP_PLACE_REGION_RESOURCE_VA, REGION_CANDIDATE_SCAN_VA, ROW_COUNT_TEST_VA,
    ROW_ENUMERATE_CALL_VA,
};
use place_resources_pool_frontier::{resource_divvy_pool_digest, ResourceDivvyPoolState};
use place_resources_xml_frontier::{
    BonusXmlRowFact, BonusesSectionSource, PlaceResourcesBonusRowsHandoff, XmlHostHandles,
    PLACE_RESOURCES_XML_RESIDUAL_VA,
};

const RANDOM_STATE: i32 = 0x1234_5678;
const GOOD_DIGEST: u64 = 0x1122_3344_5566_7788;
const ITEM_DIGEST: u64 = 0x8877_6655_4433_2211;

fn xml_handles() -> XmlHostHandles {
    XmlHostHandles {
        head: true,
        tail: true,
        inline_tail_word: false,
    }
}

fn host_handles() -> HostHandles {
    HostHandles {
        head: true,
        tail: true,
        inline_tail_word: false,
    }
}

fn zero_scaled() -> Vec<ScaledAttributeFact> {
    vec![ScaledAttributeFact {
        attribute: ScaledAttribute::NumRare,
        call_va: NUM_RARE_SCALE_CALL_VA,
        expression: "0".to_owned(),
        scaled: 0,
    }]
}

fn placement_scaled() -> Vec<ScaledAttributeFact> {
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

fn empty_chain() -> DownChainFact {
    DownChainFact {
        initial_index: -1,
        initial_who: 0,
        links: Vec::new(),
    }
}

fn exact_player_placement(world: &World, pool: &ResourceDivvyPoolState) -> CanonicalPlacementFacts {
    let entry_state = place_resources_category_frontier::DeterministicResourceState {
        random_state: RANDOM_STATE,
        world_checksum: world.checksum_sections(),
        sourced_walked_bytes: 52,
        resource_pool_digest: resource_divvy_pool_digest(pool),
        allocated_resources: 0,
        requested_resources: 0,
        last_chance_group: -1,
        signed_chance_budget: -1,
        winner_seen: true,
    };
    let prefix = PlayerPlacementPrefixFacts {
        world_width: 8,
        world_height: 8,
        starts: vec![
            PlayerStartFact { x: 2, y: 2 },
            PlayerStartFact { x: 5, y: 5 },
        ],
        spiral_ring_ends: vec![0; 65],
        evidence: ResourceFrontierEvidence::SyntheticFixture {
            fixture: "canonical-player-prefix".to_owned(),
            entry_va: MAP_PLACE_PLAYER_RESOURCE_VA,
            random_state: entry_state.random_state,
            world_checksum: entry_state.world_checksum.clone(),
            sourced_walked_bytes: entry_state.sourced_walked_bytes,
            resource_pool_digest: entry_state.resource_pool_digest,
        },
    };
    let parameters = PlayerResourceBodyParameters {
        player_count: 2,
        good_id: 6,
        num_rare: 1,
        player_keep_away: 1,
        player_stay_near: 1,
        existing_resource_spacing: 0,
        placed_resource_spacing: 0,
        center_keep_away: 0,
        center_stay_near: 0,
        edge_keep_away: 0,
        edge_stay_near: 0,
        corner_keep_away: 0,
        corner_stay_near: 0,
        selector: 0,
    };
    let prefix_receipt = place_resources_category_frontier::execute_player_placement_prefix(
        &entry_state,
        PlayerPlacementPrefixRequest {
            player_count: parameters.player_count,
            good_id: parameters.good_id,
            num_rare: parameters.num_rare,
            spacing: parameters.player_keep_away,
            group_spacing: parameters.player_stay_near,
            selector: parameters.selector,
        },
        &prefix,
    )
    .unwrap();
    let body_state = PlayerResourceBodyState {
        deterministic: entry_state,
        resource_pool: pool.clone(),
        good_state_digest: GOOD_DIGEST,
        item_state_digest: ITEM_DIGEST,
    };
    let cells = (0..8)
        .flat_map(|y| {
            (0..8).map(move |x| RegionWorldCellFact {
                x,
                y,
                primary_flags: 0,
                primary_land: 0,
                shadow_flags: 0,
                shadow_occupant: 0,
                down_chain: empty_chain(),
                has_mountain_tcoords: false,
                has_south_forest: false,
            })
        })
        .collect();
    let mut body = PlayerResourceBodyFacts {
        spiral_x_offsets: Vec::new(),
        spiral_y_offsets: Vec::new(),
        cells,
        start_occupied: vec![false; 64],
        existing_resources: Vec::new(),
        terrain_rare: (0..=7)
            .map(|terrain_class| TerrainRareFact {
                terrain_class,
                good_ids: if terrain_class == 0 {
                    vec![6]
                } else {
                    Vec::new()
                },
            })
            .collect(),
        good_catalog: vec![GoodPlacementFact {
            good_id: 6,
            uses_offset_0x120: false,
        }],
        placement_evidence: PlayerBodyPlacementEvidence::SyntheticFixture {
            fixture: "canonical-player-placement".to_owned(),
        },
        evidence: PlayerResourceBodyEvidence::SyntheticFixture {
            fixture: "canonical-player-body".to_owned(),
            entry_va: MAP_PLACE_PLAYER_RESOURCE_VA,
            random_state: body_state.deterministic.random_state,
            world_checksum: body_state.deterministic.world_checksum.clone(),
            sourced_walked_bytes: body_state.deterministic.sourced_walked_bytes,
            resource_pool_digest: body_state.deterministic.resource_pool_digest,
            good_state_digest: body_state.good_state_digest,
            item_state_digest: body_state.item_state_digest,
            facts_digest: 0,
            prefix_digest: player_placement_prefix_receipt_digest(&prefix_receipt),
        },
    };
    let digest = player_resource_body_facts_digest(parameters, &prefix, &body);
    if let PlayerResourceBodyEvidence::SyntheticFixture { facts_digest, .. } = &mut body.evidence {
        *facts_digest = digest;
    }
    CanonicalPlacementFacts::Player { prefix, body }
}

fn exact_region_placement(world: &World, pool: &ResourceDivvyPoolState) -> CanonicalPlacementFacts {
    let entry_state = place_resources_category_frontier::DeterministicResourceState {
        random_state: RANDOM_STATE,
        world_checksum: world.checksum_sections(),
        sourced_walked_bytes: 52,
        resource_pool_digest: resource_divvy_pool_digest(pool),
        allocated_resources: 0,
        requested_resources: 0,
        last_chance_group: -1,
        signed_chance_budget: -1,
        winner_seen: true,
    };
    let mut regions = (1..=126)
        .map(|region_id| RegionPrefixFact {
            region_id,
            land_cell_count: 0,
            contains_player_start: false,
            candidate_point_count: 0,
        })
        .collect::<Vec<_>>();
    regions[0].land_cell_count = 6;
    regions[0].candidate_point_count = 1;
    let prefix = RegionPlacementPrefixFacts {
        ocean_rare_good_ids: vec![6, 31],
        regions,
        evidence: ResourceFrontierEvidence::SyntheticFixture {
            fixture: "canonical-region-prefix".to_owned(),
            entry_va: MAP_PLACE_REGION_RESOURCE_VA,
            random_state: entry_state.random_state,
            world_checksum: entry_state.world_checksum.clone(),
            sourced_walked_bytes: entry_state.sourced_walked_bytes,
            resource_pool_digest: entry_state.resource_pool_digest,
        },
    };
    let parameters = RegionWorldBodyParameters {
        good_id: 10,
        pattern: 1,
        saturate: 0,
        num_rare: 1,
        selector: 0,
        player_keep_away: 1,
        player_stay_near: 1,
        existing_resource_spacing: 0,
        group_spacing: 0,
        center_keep_away: 0,
        center_stay_near: 0,
        edge_keep_away: 0,
        edge_stay_near: 0,
        corner_keep_away: 0,
        corner_stay_near: 0,
    };
    let prefix_receipt = place_resources_category_frontier::execute_region_placement_prefix(
        &entry_state,
        RegionPlacementPrefixRequest {
            good_id: parameters.good_id,
            pattern: parameters.pattern,
            saturate: parameters.saturate,
            num_rare: parameters.num_rare,
            selector: parameters.selector,
        },
        &prefix,
    )
    .unwrap();
    let body_state = RegionWorldBodyState {
        deterministic: entry_state,
        good_state_digest: GOOD_DIGEST,
    };
    let cells = (0..8)
        .flat_map(|y| {
            (0..8).map(move |x| RegionWorldCellFact {
                x,
                y,
                primary_flags: 0,
                primary_land: 0,
                shadow_flags: if (x, y) == (2, 2) { 0x10 } else { 0 },
                shadow_occupant: 0,
                down_chain: empty_chain(),
                has_mountain_tcoords: false,
                has_south_forest: false,
            })
        })
        .collect();
    let mut body = RegionWorldBodyFacts {
        world_width: 8,
        world_height: 8,
        cells,
        starts: Vec::new(),
        existing_resources: Vec::new(),
        regions: vec![RegionCandidateSetFact {
            region_id: 1,
            points: vec![(2, 2)],
        }],
        terrain_rare: (0..=7)
            .map(|terrain_class| TerrainRareFact {
                terrain_class,
                good_ids: if terrain_class == 0 {
                    vec![10]
                } else {
                    Vec::new()
                },
            })
            .collect(),
        coord_metric_table: (0..32).map(|value| value / 3).collect(),
        good_uses_offset_0x120: false,
        placement_evidence: RegionBodyPlacementEvidence::SyntheticFixture {
            fixture: "canonical-region-placement".to_owned(),
        },
        evidence: RegionWorldBodyEvidence::SyntheticFixture {
            fixture: "canonical-region-body".to_owned(),
            entry_va: REGION_CANDIDATE_SCAN_VA,
            random_state: body_state.deterministic.random_state,
            world_checksum: body_state.deterministic.world_checksum.clone(),
            resource_pool_digest: body_state.deterministic.resource_pool_digest,
            good_state_digest: body_state.good_state_digest,
            facts_digest: 0,
            prefix_digest: region_placement_prefix_receipt_digest(&prefix_receipt),
        },
    };
    let digest = region_world_body_facts_digest(parameters, &body);
    if let RegionWorldBodyEvidence::SyntheticFixture { facts_digest, .. } = &mut body.evidence {
        *facts_digest = digest;
    }
    CanonicalPlacementFacts::RegionWorld { prefix, body }
}

fn initial_row(entry: &PlaceResourcesBonusRowsHandoff) -> CanonicalRowFacts {
    CanonicalRowFacts {
        mutation: CanonicalRowMutationFacts::InitialBonus(FirstBonusMutationFacts {
            capture_ordinal: entry.rows[0].capture_ordinal,
            type_name: "WHALES".to_owned(),
            type_resolution: ResourceTypeResolution::CatalogGood { good_id: 6 },
            chance: 1,
            chance_group: -1,
            pattern_name: "PLAYER".to_owned(),
            pattern: PlacementPattern::Player,
            saturate: 0,
            spacing: 0,
            scaled: zero_scaled(),
            evidence: BonusMutationEvidence::SyntheticFixture {
                fixture: "canonical-bonuses-row-zero".to_owned(),
                entry_va: entry.resume_va,
                capture_ordinal: entry.rows[0].capture_ordinal,
                random_state: entry.random_state,
                world_checksum: entry.world_checksum.clone(),
                sourced_walked_bytes: entry.sourced_walked_bytes,
                resource_pool_digest: entry.resource_pool_digest,
            },
        }),
        placement: None,
    }
}

fn initial_player_row(
    entry: &PlaceResourcesBonusRowsHandoff,
    world: &World,
    pool: &ResourceDivvyPoolState,
) -> CanonicalRowFacts {
    CanonicalRowFacts {
        mutation: CanonicalRowMutationFacts::InitialBonus(FirstBonusMutationFacts {
            capture_ordinal: entry.rows[0].capture_ordinal,
            type_name: "WHALES".to_owned(),
            type_resolution: ResourceTypeResolution::CatalogGood { good_id: 6 },
            chance: 1,
            chance_group: -1,
            pattern_name: "PLAYER".to_owned(),
            pattern: PlacementPattern::Player,
            saturate: 0,
            spacing: 0,
            scaled: placement_scaled(),
            evidence: BonusMutationEvidence::SyntheticFixture {
                fixture: "canonical-player-row".to_owned(),
                entry_va: entry.resume_va,
                capture_ordinal: entry.rows[0].capture_ordinal,
                random_state: entry.random_state,
                world_checksum: entry.world_checksum.clone(),
                sourced_walked_bytes: entry.sourced_walked_bytes,
                resource_pool_digest: entry.resource_pool_digest,
            },
        }),
        placement: Some(exact_player_placement(world, pool)),
    }
}

fn initial_region_row(
    entry: &PlaceResourcesBonusRowsHandoff,
    world: &World,
    pool: &ResourceDivvyPoolState,
) -> CanonicalRowFacts {
    let mut row = initial_player_row(entry, world, pool);
    let CanonicalRowMutationFacts::InitialBonus(facts) = &mut row.mutation else {
        unreachable!()
    };
    facts.type_name = "BISON".to_owned();
    facts.type_resolution = ResourceTypeResolution::CatalogGood { good_id: 10 };
    facts.pattern_name = "WORLD".to_owned();
    facts.pattern = PlacementPattern::World;
    row.placement = Some(exact_region_placement(world, pool));
    row
}

fn carried_row(
    category: ResourceCategory,
    capture_ordinal: u32,
    world: &World,
    pool_digest: u64,
) -> CanonicalRowFacts {
    let mut facts = LaterBonusMutationFacts {
        capture_ordinal,
        type_name: "WHALES".to_owned(),
        type_resolution: ResourceTypeResolution::CatalogGood { good_id: 6 },
        chance: 0,
        chance_group: -1,
        pattern_name: "PLAYER".to_owned(),
        pattern: PlacementPattern::Player,
        saturate: 0,
        spacing: 0,
        scaled: zero_scaled(),
        evidence: LaterBonusMutationEvidence::CarriedSyntheticFixture {
            fixture: format!("canonical-{category:?}-row-zero"),
            category,
            entry_va: CARRIED_CATEGORY_FIRST_ENTRY_VA,
            row_enumerate_call_va: ROW_ENUMERATE_CALL_VA,
            row_count_test_va: ROW_COUNT_TEST_VA,
            mutation_entry_va: LATER_ROW_MUTATION_ENTRY_VA,
            row_index: 0,
            capture_ordinal,
            random_state: RANDOM_STATE,
            world_checksum: world.checksum_sections(),
            sourced_walked_bytes: 52,
            resource_pool_digest: pool_digest,
            last_chance_group: -1,
            signed_chance_budget: -1,
            winner_seen: true,
            facts_digest: 0,
        },
    };
    let digest = later_bonus_facts_digest(&facts);
    if let LaterBonusMutationEvidence::CarriedSyntheticFixture { facts_digest, .. } =
        &mut facts.evidence
    {
        *facts_digest = digest;
    }
    CanonicalRowFacts {
        mutation: CanonicalRowMutationFacts::CarriedOrLater(facts),
        placement: None,
    }
}

fn advance_evidence(world: &World, pool_digest: u64) -> ResourceFrontierEvidence {
    ResourceFrontierEvidence::SyntheticFixture {
        fixture: "canonical-category-tail".to_owned(),
        entry_va: CATEGORY_TAIL_VA,
        random_state: RANDOM_STATE,
        world_checksum: world.checksum_sections(),
        sourced_walked_bytes: 52,
        resource_pool_digest: pool_digest,
    }
}

fn section(category: ResourceCategory, capture_ordinal: u32) -> CategorySectionFact {
    CategorySectionFact {
        lookup_token: match category {
            ResourceCategory::Fish => "FISH ",
            ResourceCategory::Goodies => "GOODIES",
            ResourceCategory::Bonuses => unreachable!(),
        }
        .to_owned(),
        returned_element_name: match category {
            ResourceCategory::Fish => "FISH",
            ResourceCategory::Goodies => "GOODIES",
            ResourceCategory::Bonuses => unreachable!(),
        }
        .to_owned(),
        handles: host_handles(),
        rows: vec![CategoryRowFact {
            capture_ordinal,
            element_name: "BONUS".to_owned(),
            handles: host_handles(),
        }],
    }
}

fn fixture() -> (
    CanonicalPlaceResourcesState,
    PlaceResourcesBonusRowsHandoff,
    CanonicalPlaceResourcesFacts,
    World,
) {
    let world = World::init_default_rules(8, 8);
    let pool = ResourceDivvyPoolState::default();
    let pool_digest = resource_divvy_pool_digest(&pool);
    let entry = PlaceResourcesBonusRowsHandoff {
        resume_va: PLACE_RESOURCES_XML_RESIDUAL_VA,
        first_row_body_va: FIRST_BONUS_ROW_BODY_VA,
        player_count_argument: 2,
        section_source: BonusesSectionSource::SelectedStyle,
        current_category_handles: xml_handles(),
        rows: vec![BonusXmlRowFact {
            capture_ordinal: 10,
            element_name: "BONUS".to_owned(),
            handles: xml_handles(),
        }],
        selected_document_live: true,
        default_document_live: true,
        random_state: RANDOM_STATE,
        world_checksum: world.checksum_sections(),
        sourced_walked_bytes: 52,
        resource_pool_digest: pool_digest,
    };
    let state = CanonicalPlaceResourcesState {
        mutation: PlaceResourcesBonusMutationState {
            random_state: RANDOM_STATE,
            world_checksum: world.checksum_sections(),
            sourced_walked_bytes: 52,
            resource_pool_digest: pool_digest,
            resource_pool: Some(pool),
            allocated_resources: 0,
            requested_resources: 0,
        },
        good_state_digest: GOOD_DIGEST,
        item_state_digest: ITEM_DIGEST,
        selected_document_handles: host_handles(),
        default_document_handles: host_handles(),
    };
    let facts = CanonicalPlaceResourcesFacts {
        bonuses: vec![initial_row(&entry)],
        bonuses_tail: CategoryAdvanceFacts {
            selected_style_name_nonempty: true,
            selected_section: Some(section(ResourceCategory::Fish, 20)),
            default_section: None,
            evidence: advance_evidence(&world, pool_digest),
        },
        fish: vec![carried_row(ResourceCategory::Fish, 20, &world, pool_digest)],
        fish_tail: CategoryAdvanceFacts {
            selected_style_name_nonempty: true,
            selected_section: Some(section(ResourceCategory::Goodies, 30)),
            default_section: None,
            evidence: advance_evidence(&world, pool_digest),
        },
        goodies: vec![carried_row(
            ResourceCategory::Goodies,
            30,
            &world,
            pool_digest,
        )],
        goodies_tail: CategoryAdvanceFacts {
            selected_style_name_nonempty: true,
            selected_section: None,
            default_section: None,
            evidence: advance_evidence(&world, pool_digest),
        },
    };
    (state, entry, facts, world)
}

#[derive(Default)]
struct NoAllocationHost;

impl PlayerAllocationHost for NoAllocationHost {
    fn propose_allocation(
        &mut self,
        _request: &PlayerAllocationRequest,
    ) -> Option<PlayerAllocationReceipt> {
        panic!("zero-requested chronology must not reach Player allocation")
    }
}

impl RegionInitGoodHost for NoAllocationHost {
    fn propose_init_good(
        &mut self,
        _request: &RegionInitGoodRequest,
    ) -> Option<RegionInitGoodReceipt> {
        panic!("zero-requested chronology must not reach Region allocation")
    }
}

#[test]
fn canonical_owner_is_source_reachable_without_channel_installation() {
    assert_eq!(
        place_resources_category_frontier::FUNCTION_RETURN_VA,
        0x0069_0476
    );
}

#[test]
fn canonical_transaction_runs_bonus_fish_goodies_and_releases_documents() {
    let (mut state, entry, facts, _) = fixture();
    let mut host = NoAllocationHost;

    let receipt = execute_canonical_place_resources(&mut state, &entry, &facts, &mut host).unwrap();

    assert_eq!(
        receipt
            .categories
            .iter()
            .map(|category| category.category)
            .collect::<Vec<_>>(),
        vec![
            ResourceCategory::Bonuses,
            ResourceCategory::Fish,
            ResourceCategory::Goodies
        ]
    );
    assert_eq!(receipt.return_count, 0);
    assert_eq!(receipt.deterministic_after.last_chance_group, -1);
    assert_eq!(receipt.deterministic_after.signed_chance_budget, -1);
    assert!(receipt.deterministic_after.winner_seen);
    assert_eq!(receipt.resource_pool_before, receipt.resource_pool_after);
    assert_eq!(
        receipt.good_state_digest_before,
        receipt.good_state_digest_after
    );
    assert_eq!(
        receipt.item_state_digest_before,
        receipt.item_state_digest_after
    );
    assert_eq!(state.selected_document_handles, HostHandles::default());
    assert_eq!(state.default_document_handles, HostHandles::default());
}

#[test]
fn late_category_failure_rolls_back_rng_world_pool_good_item_and_documents() {
    let (mut state, entry, mut facts, _) = fixture();
    let before = state.clone();
    let mut host = NoAllocationHost;
    if let ResourceFrontierEvidence::SyntheticFixture { random_state, .. } =
        &mut facts.fish_tail.evidence
    {
        *random_state = random_state.wrapping_add(1);
    }

    assert!(matches!(
        execute_canonical_place_resources(&mut state, &entry, &facts, &mut host),
        Err(CanonicalPlaceResourcesError::Category(_))
    ));
    assert_eq!(state, before);
}

#[test]
fn canonical_player_adapter_executes_the_exact_body_and_returns_its_receipt() {
    let (mut state, entry, mut facts, world) = fixture();
    let pool = state.mutation.resource_pool.clone().unwrap();
    facts.bonuses = vec![initial_player_row(&entry, &world, &pool)];
    facts.bonuses_tail.selected_style_name_nonempty = false;
    facts.bonuses_tail.selected_section = None;
    facts.fish.clear();
    facts.fish_tail.selected_style_name_nonempty = false;
    facts.fish_tail.selected_section = None;
    facts.goodies.clear();
    let mut host = NoAllocationHost;

    let receipt = execute_canonical_place_resources(&mut state, &entry, &facts, &mut host).unwrap();

    assert!(matches!(
        receipt.categories[0].rows[0].exact_placement,
        Some(CanonicalExactPlacementReceipt::Player { ref body, .. })
            if body.return_count == 0 && body.attempts.len() == 2
    ));
    assert_eq!(state.mutation.requested_resources, 2);
    assert_eq!(state.mutation.allocated_resources, 0);
    assert_eq!(receipt.return_count, 0);
}

#[test]
fn canonical_region_adapter_executes_prefix_draw_chronology_and_exact_world_body() {
    let (mut state, entry, mut facts, world) = fixture();
    let pool = state.mutation.resource_pool.clone().unwrap();
    facts.bonuses = vec![initial_region_row(&entry, &world, &pool)];
    facts.bonuses_tail.selected_style_name_nonempty = false;
    facts.bonuses_tail.selected_section = None;
    facts.fish.clear();
    facts.fish_tail.selected_style_name_nonempty = false;
    facts.fish_tail.selected_section = None;
    facts.goodies.clear();
    let mut host = NoAllocationHost;

    let receipt = execute_canonical_place_resources(&mut state, &entry, &facts, &mut host).unwrap();

    assert!(matches!(
        receipt.categories[0].rows[0].exact_placement,
        Some(CanonicalExactPlacementReceipt::RegionWorld { ref prefix, ref body })
            if prefix.random_state_before == prefix.random_state_after
                && body.return_count == 0
                && body.attempts.len() == 1
    ));
    assert_eq!(state.mutation.requested_resources, 1);
    assert_eq!(state.mutation.allocated_resources, 0);
    assert_eq!(receipt.return_count, 0);
}
