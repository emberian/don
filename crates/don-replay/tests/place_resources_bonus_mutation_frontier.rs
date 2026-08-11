// SPDX-License-Identifier: GPL-3.0-or-later
//! Mutation-sensitive source-only proof for the first `BONUS` placement cone.

#[path = "../src/place_resources_xml_frontier.rs"]
mod place_resources_xml_frontier;

#[path = "../src/place_resources_pool_frontier.rs"]
mod place_resources_pool_frontier;

#[path = "../src/resource_divvy_pool_selection_frontier.rs"]
mod resource_divvy_pool_selection_frontier;

#[path = "../src/place_resources_bonus_mutation_frontier.rs"]
mod place_resources_bonus_mutation_frontier;

use don_sim::rng::Random;
use don_sim::systems::map_terrain::{World, WorldSection, COORD_PER_WCELL};
use place_resources_bonus_mutation_frontier::{
    execute_first_bonus_mutation, AllocationKind, BonusMutationEvidence, CallbackKind,
    CalleeRandomDraw, FirstBonusDisposition, FirstBonusMutationError, FirstBonusMutationFacts,
    PlacementEvidence, PlacementHost, PlacementPath, PlacementReceipt, PlacementRequest,
    ResourceAllocation, ResourceTypeResolution, ScaledAttribute, ScaledAttributeFact,
    WorldOccupancyWrite, BONUS_CATEGORY_TAIL_VA, CHANCE_RANDOM_GET_CALL_VA,
    FIRST_BONUS_ROW_BODY_VA, FIRST_BONUS_ROW_LOOP_INIT_VA, FIRST_BONUS_ROW_RESIDUAL_VA,
    GOOD_WDATA_DOWN_MARKER, GOOD_WDATA_DOWN_WHO_WRITE_VA, GOOD_WDATA_DOWN_WRITE_VA,
    MAP_PLACE_PLAYER_RESOURCE_VA, NEXT_BONUS_ROW_VA, OBJECTS_INIT_GOOD_VA,
    PLAYER_INIT_GOOD_CALL_VA, PLAYER_PLACEMENT_CALL_VA, PLAYER_PLACEMENT_RANDOM_CALL_VA,
    RANDOM_GET_VA, REGION_FIND_AVAIL_RANDOM_CALL_VA, REGION_PLACEMENT_RANDOM_CALL_VA,
    RESOURCE_POOL_GET_EARLY_RANDOM_CALL_VA, SELECTOR_TWO_IGNORE_CALL_VA,
};
use place_resources_bonus_mutation_frontier::{PlaceResourcesBonusMutationState, PlacementPattern};
use place_resources_pool_frontier::{
    resource_divvy_pool_digest, ResourceDivvyPoolState, ResourcePoolBitMask,
};
use place_resources_xml_frontier::{
    BonusXmlRowFact, BonusesSectionSource, PlaceResourcesBonusRowsHandoff, XmlHostHandles,
    PLACE_RESOURCES_XML_RESIDUAL_VA,
};
use resource_divvy_pool_selection_frontier::{execute_resource_pool_selection, ResourcePoolLane};

const SEED: i32 = 0x1234_5678;
const POOL_DIGEST: u64 = 0x0123_4567_89ab_cdef;

fn concrete_pool() -> ResourceDivvyPoolState {
    ResourceDivvyPoolState {
        early_bits: ResourcePoolBitMask {
            bits: 3,
            bytes: vec![0],
        },
        early_goods: vec![6, 7, 8],
        late_bits: ResourcePoolBitMask {
            bits: 2,
            bytes: vec![0],
        },
        late_goods: vec![20, 21],
        water_bits: ResourcePoolBitMask {
            bits: 1,
            bytes: vec![0],
        },
        water_goods: vec![30],
    }
}

fn handles() -> XmlHostHandles {
    XmlHostHandles {
        head: true,
        tail: true,
        inline_tail_word: false,
    }
}

fn handoff(world: &World, rows: usize) -> PlaceResourcesBonusRowsHandoff {
    PlaceResourcesBonusRowsHandoff {
        resume_va: PLACE_RESOURCES_XML_RESIDUAL_VA,
        first_row_body_va: FIRST_BONUS_ROW_BODY_VA,
        player_count_argument: 4,
        section_source: BonusesSectionSource::SelectedStyle,
        current_category_handles: handles(),
        rows: (0..rows)
            .map(|ordinal| BonusXmlRowFact {
                capture_ordinal: 10 + ordinal as u32,
                element_name: "BONUS".to_owned(),
                handles: handles(),
            })
            .collect(),
        selected_document_live: true,
        default_document_live: true,
        random_state: SEED,
        world_checksum: world.checksum_sections(),
        sourced_walked_bytes: 52,
        resource_pool_digest: POOL_DIGEST,
    }
}

fn scaled_facts() -> Vec<ScaledAttributeFact> {
    use place_resources_bonus_mutation_frontier::{
        CENTER_KEEP_AWAY_SCALE_CALL_VA, CENTER_STAY_NEAR_SCALE_CALL_VA,
        CORNER_KEEP_AWAY_SCALE_CALL_VA, CORNER_STAY_NEAR_SCALE_CALL_VA,
        EDGE_KEEP_AWAY_SCALE_CALL_VA, EDGE_STAY_NEAR_SCALE_CALL_VA, GROUP_SPACING_SCALE_CALL_VA,
        NUM_RARE_SCALE_CALL_VA, PLAYER_KEEP_AWAY_SCALE_CALL_VA, PLAYER_STAY_NEAR_SCALE_CALL_VA,
    };
    [
        (ScaledAttribute::NumRare, NUM_RARE_SCALE_CALL_VA, "2", 2),
        (
            ScaledAttribute::GroupSpacing,
            GROUP_SPACING_SCALE_CALL_VA,
            "-3",
            -3,
        ),
        (
            ScaledAttribute::PlayerKeepAway,
            PLAYER_KEEP_AWAY_SCALE_CALL_VA,
            "0",
            0,
        ),
        (
            ScaledAttribute::PlayerStayNear,
            PLAYER_STAY_NEAR_SCALE_CALL_VA,
            "-2",
            -2,
        ),
        (
            ScaledAttribute::CenterKeepAway,
            CENTER_KEEP_AWAY_SCALE_CALL_VA,
            "3",
            3,
        ),
        (
            ScaledAttribute::CenterStayNear,
            CENTER_STAY_NEAR_SCALE_CALL_VA,
            "4",
            4,
        ),
        (
            ScaledAttribute::CornerKeepAway,
            CORNER_KEEP_AWAY_SCALE_CALL_VA,
            "5",
            5,
        ),
        (
            ScaledAttribute::CornerStayNear,
            CORNER_STAY_NEAR_SCALE_CALL_VA,
            "6",
            6,
        ),
        (
            ScaledAttribute::EdgeKeepAway,
            EDGE_KEEP_AWAY_SCALE_CALL_VA,
            "7",
            7,
        ),
        (
            ScaledAttribute::EdgeStayNear,
            EDGE_STAY_NEAR_SCALE_CALL_VA,
            "8",
            8,
        ),
    ]
    .into_iter()
    .map(
        |(attribute, call_va, expression, scaled)| ScaledAttributeFact {
            attribute,
            call_va,
            expression: expression.to_owned(),
            scaled,
        },
    )
    .collect()
}

fn facts(
    entry: &PlaceResourcesBonusRowsHandoff,
    type_resolution: ResourceTypeResolution,
    chance: i32,
    chance_group: i32,
) -> FirstBonusMutationFacts {
    FirstBonusMutationFacts {
        capture_ordinal: entry.rows[0].capture_ordinal,
        type_name: "Early".to_owned(),
        type_resolution,
        chance,
        chance_group,
        pattern_name: "player".to_owned(),
        pattern: PlacementPattern::Player,
        saturate: -4,
        spacing: 8,
        scaled: scaled_facts(),
        evidence: BonusMutationEvidence::SyntheticFixture {
            fixture: "first-bonus-mutation".to_owned(),
            entry_va: entry.resume_va,
            capture_ordinal: entry.rows[0].capture_ordinal,
            random_state: entry.random_state,
            world_checksum: entry.world_checksum.clone(),
            sourced_walked_bytes: entry.sourced_walked_bytes,
            resource_pool_digest: entry.resource_pool_digest,
        },
    }
}

#[derive(Clone)]
struct ScriptedPlacementHost {
    world_checksum_after: don_sim::systems::map_terrain::WorldChecksum,
    draw: bool,
    pool_draw: bool,
    allocation: bool,
    corrupt_random: bool,
    corrupt_occupancy_marker: bool,
    requests: Vec<PlacementRequest>,
}

impl ScriptedPlacementHost {
    fn no_allocation(world: &World) -> Self {
        Self {
            world_checksum_after: world.checksum_sections(),
            draw: false,
            pool_draw: false,
            allocation: false,
            corrupt_random: false,
            corrupt_occupancy_marker: false,
            requests: Vec::new(),
        }
    }
}

impl PlacementHost for ScriptedPlacementHost {
    fn place(&mut self, request: &PlacementRequest) -> Option<PlacementReceipt> {
        self.requests.push(request.clone());
        let mut random = Random::new(request.random_state_before);
        let mut resource_pool_selections = Vec::new();
        let resource_pool_after = if request.params.selector != 0 && self.pool_draw {
            let mut pool = request.resource_pool_before.clone()?;
            let lane = if request.params.selector == 2 {
                ResourcePoolLane::Early
            } else {
                ResourcePoolLane::Late
            };
            let selection = execute_resource_pool_selection(&mut pool, lane, &mut random).ok()?;
            resource_pool_selections.push(selection);
            Some(pool)
        } else {
            request.resource_pool_before.clone()
        };
        let mut random_draws = resource_pool_selections
            .iter()
            .flat_map(|selection| selection.random_draws.iter())
            .map(|draw| CalleeRandomDraw {
                call_va: draw.call_va,
                random_get_va: draw.random_get_va,
                low: draw.low,
                high: draw.high,
                state_before: draw.state_before,
                raw: draw.raw,
                state_after: draw.state_after,
            })
            .collect::<Vec<_>>();
        if self.draw && !self.pool_draw {
            let state_before = random.state();
            let expected = random.get(0, 0xffff);
            random_draws.push(CalleeRandomDraw {
                call_va: PLAYER_PLACEMENT_RANDOM_CALL_VA,
                random_get_va: RANDOM_GET_VA,
                low: 0,
                high: 0xffff,
                state_before,
                raw: if self.corrupt_random {
                    expected ^ 1
                } else {
                    expected
                },
                state_after: random.state(),
            });
        }
        let allocations = if self.allocation {
            let coord_x = COORD_PER_WCELL + COORD_PER_WCELL / 2;
            let coord_y = COORD_PER_WCELL * 2 + COORD_PER_WCELL / 2;
            vec![ResourceAllocation {
                call_va: PLAYER_INIT_GOOD_CALL_VA,
                callee_va: OBJECTS_INIT_GOOD_VA,
                kind: AllocationKind::Good,
                type_id: request.params.good_id,
                coord_x,
                coord_y,
                slot: 7,
                occupancy_write: Some(WorldOccupancyWrite {
                    world_x: 1,
                    world_y: 2,
                    down_write_va: GOOD_WDATA_DOWN_WRITE_VA,
                    down_who_write_va: GOOD_WDATA_DOWN_WHO_WRITE_VA,
                    old_down: -1,
                    old_down_who: 0,
                    new_down: if self.corrupt_occupancy_marker {
                        -9
                    } else {
                        GOOD_WDATA_DOWN_MARKER
                    },
                    new_down_who: 7,
                }),
            }]
        } else {
            Vec::new()
        };
        Some(PlacementReceipt {
            request: request.clone(),
            random_draws,
            random_state_after: random.state(),
            allocated_count: allocations.len() as i32,
            allocations,
            world_checksum_after: self.world_checksum_after.clone(),
            sourced_walked_bytes_after: request.sourced_walked_bytes,
            resource_pool_digest_after: resource_pool_after
                .as_ref()
                .map(resource_divvy_pool_digest)
                .unwrap_or(request.resource_pool_digest_before),
            resource_pool_selections,
            resource_pool_after,
            evidence: PlacementEvidence::SyntheticFixture {
                fixture: "typed-placement-receipt".to_owned(),
            },
        })
    }
}

struct TranscriptPlacementHost {
    call_vas: Vec<u32>,
    corrupt_state_at: Option<usize>,
}

impl PlacementHost for TranscriptPlacementHost {
    fn place(&mut self, request: &PlacementRequest) -> Option<PlacementReceipt> {
        let mut random = Random::new(request.random_state_before);
        let random_draws = self
            .call_vas
            .iter()
            .enumerate()
            .map(|(index, call_va)| {
                let actual_state_before = random.state();
                let raw = random.get(0, 0xffff);
                CalleeRandomDraw {
                    call_va: *call_va,
                    random_get_va: RANDOM_GET_VA,
                    low: 0,
                    high: 0xffff,
                    state_before: if self.corrupt_state_at == Some(index) {
                        actual_state_before ^ 1
                    } else {
                        actual_state_before
                    },
                    raw,
                    state_after: random.state(),
                }
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
            resource_pool_digest_after: request.resource_pool_digest_before,
            resource_pool_selections: Vec::new(),
            resource_pool_after: request.resource_pool_before.clone(),
            evidence: PlacementEvidence::SyntheticFixture {
                fixture: "placement-rng-transcript".to_owned(),
            },
        })
    }
}

fn region_facts(entry: &PlaceResourcesBonusRowsHandoff) -> FirstBonusMutationFacts {
    let mut facts = facts(
        entry,
        ResourceTypeResolution::CatalogGood { good_id: 6 },
        100,
        0,
    );
    facts.pattern_name = "world".to_owned();
    facts.pattern = PlacementPattern::World;
    facts
}

#[test]
fn first_player_row_draws_then_commits_exact_allocation_and_wdata_write() {
    let world = World::init_default_rules(4, 4);
    let entry = handoff(&world, 2);
    let mut after = world.clone();
    after.wdata_mut(1, 2).down = GOOD_WDATA_DOWN_MARKER;
    after.wdata_mut(1, 2).down_who = 7;
    let mut host = ScriptedPlacementHost {
        world_checksum_after: after.checksum_sections(),
        draw: true,
        pool_draw: false,
        allocation: true,
        corrupt_random: false,
        corrupt_occupancy_marker: false,
        requests: Vec::new(),
    };
    let facts = facts(
        &entry,
        ResourceTypeResolution::CatalogGood { good_id: 6 },
        100,
        0,
    );
    let mut state = PlaceResourcesBonusMutationState::from_handoff(&entry);

    let receipt = execute_first_bonus_mutation(&mut state, &entry, &facts, &mut host).unwrap();

    assert_eq!(receipt.entry_va, FIRST_BONUS_ROW_LOOP_INIT_VA);
    assert_eq!(receipt.row_body_va, FIRST_BONUS_ROW_BODY_VA);
    assert_eq!(receipt.residual_va, FIRST_BONUS_ROW_RESIDUAL_VA);
    assert_eq!(receipt.next_va, NEXT_BONUS_ROW_VA);
    assert_eq!(
        receipt.disposition,
        FirstBonusDisposition::Placed(PlacementPath::Player)
    );
    let chance = receipt.chance_draw.as_ref().unwrap();
    assert_eq!(chance.call_va, CHANCE_RANDOM_GET_CALL_VA);
    assert_eq!(chance.random_get_va, RANDOM_GET_VA);
    assert_eq!(chance.state_before, SEED);
    assert!(chance.modulo_100 < 100);
    assert!(receipt.chance_budget_after_subtract < 0);
    assert!(receipt.chance_winner_seen);

    let request = &host.requests[0];
    assert_eq!(request.call_va, PLAYER_PLACEMENT_CALL_VA);
    assert_eq!(request.callee_va, MAP_PLACE_PLAYER_RESOURCE_VA);
    assert_eq!(request.params.good_id, 6);
    assert_eq!(request.params.selector, 0);
    assert_eq!(request.params.saturate, 0);
    assert_eq!(request.params.group_spacing, 0);
    assert_eq!(request.params.player_keep_away, 1);
    assert_eq!(request.params.player_stay_near, 1);
    assert_eq!(state.allocated_resources, 1);
    assert_eq!(state.requested_resources, 8);
    assert_eq!(
        receipt
            .world_checksum_before
            .differing_sections(&receipt.world_checksum_after),
        [WorldSection::WData]
    );
    assert_eq!(
        receipt.callbacks.last().unwrap().kind,
        CallbackKind::PlacePlayerResource
    );
}

#[test]
fn chance_miss_consumes_only_the_direct_draw_and_never_calls_placement_host() {
    let world = World::init_default_rules(4, 4);
    let entry = handoff(&world, 1);
    let mut host = ScriptedPlacementHost::no_allocation(&world);
    let facts = facts(
        &entry,
        ResourceTypeResolution::CatalogGood { good_id: 6 },
        0,
        0,
    );
    let mut state = PlaceResourcesBonusMutationState::from_handoff(&entry);
    let before = state.clone();

    let receipt = execute_first_bonus_mutation(&mut state, &entry, &facts, &mut host).unwrap();

    assert_eq!(receipt.disposition, FirstBonusDisposition::ChanceMiss);
    assert_eq!(receipt.next_va, BONUS_CATEGORY_TAIL_VA);
    assert!(receipt.chance_draw.is_some());
    assert!(host.requests.is_empty());
    assert_ne!(state.random_state, before.random_state);
    assert_eq!(state.world_checksum, before.world_checksum);
    assert_eq!(state.resource_pool_digest, before.resource_pool_digest);
}

#[test]
fn native_minus_one_group_selects_from_zero_budget_without_a_direct_draw() {
    let world = World::init_default_rules(4, 4);
    let entry = handoff(&world, 1);
    let mut host = ScriptedPlacementHost::no_allocation(&world);
    let facts = facts(
        &entry,
        ResourceTypeResolution::CatalogGood { good_id: 6 },
        1,
        -1,
    );
    let mut state = PlaceResourcesBonusMutationState::from_handoff(&entry);

    let receipt = execute_first_bonus_mutation(&mut state, &entry, &facts, &mut host).unwrap();

    assert_eq!(
        receipt.disposition,
        FirstBonusDisposition::Placed(PlacementPath::Player)
    );
    assert!(receipt.chance_draw.is_none());
    assert_eq!(receipt.random_state_before, SEED);
    assert_eq!(host.requests[0].random_state_before, SEED);
    assert_eq!(state.random_state, SEED);
}

#[test]
fn zero_numrare_keeps_the_native_winner_but_skips_the_placement_body() {
    let world = World::init_default_rules(4, 4);
    let entry = handoff(&world, 1);
    let mut host = ScriptedPlacementHost::no_allocation(&world);
    let mut facts = facts(
        &entry,
        ResourceTypeResolution::CatalogGood { good_id: 6 },
        1,
        -1,
    );
    facts.scaled[0].scaled = 0;
    facts.scaled.truncate(1);
    let mut state = PlaceResourcesBonusMutationState::from_handoff(&entry);

    let receipt = execute_first_bonus_mutation(&mut state, &entry, &facts, &mut host).unwrap();

    assert_eq!(receipt.disposition, FirstBonusDisposition::ZeroRequested);
    assert!(receipt.chance_winner_seen);
    assert!(receipt.placement.is_none());
    assert!(host.requests.is_empty());
    assert_eq!(state.requested_resources, 0);
    assert_eq!(state.allocated_resources, 0);
}

#[test]
fn unknown_type_exits_before_chance_and_scaling_callbacks() {
    let world = World::init_default_rules(4, 4);
    let entry = handoff(&world, 1);
    let mut host = ScriptedPlacementHost::no_allocation(&world);
    let mut facts = facts(&entry, ResourceTypeResolution::Unknown, 100, 0);
    facts.scaled.clear();
    let mut state = PlaceResourcesBonusMutationState::from_handoff(&entry);
    let before = state.clone();

    let receipt = execute_first_bonus_mutation(&mut state, &entry, &facts, &mut host).unwrap();

    assert_eq!(receipt.disposition, FirstBonusDisposition::UnknownType);
    assert!(receipt.chance_draw.is_none());
    assert_eq!(receipt.callbacks.len(), 6);
    assert_eq!(
        receipt.callbacks.last().unwrap().kind,
        CallbackKind::MatchTypeFallback
    );
    assert!(host.requests.is_empty());
    assert_eq!(state, before);
}

#[test]
fn pool_selector_receipt_may_advance_the_same_rng_and_pool_digest() {
    let world = World::init_default_rules(4, 4);
    let pool = concrete_pool();
    let mut entry = handoff(&world, 1);
    entry.resource_pool_digest = resource_divvy_pool_digest(&pool);
    let mut host = ScriptedPlacementHost {
        world_checksum_after: world.checksum_sections(),
        draw: true,
        pool_draw: true,
        allocation: false,
        corrupt_random: false,
        corrupt_occupancy_marker: false,
        requests: Vec::new(),
    };
    let facts = facts(
        &entry,
        ResourceTypeResolution::PoolSelector {
            selector: 2,
            matched_call_va: SELECTOR_TWO_IGNORE_CALL_VA,
        },
        100,
        0,
    );
    let mut state =
        PlaceResourcesBonusMutationState::from_handoff_with_pool(&entry, &pool).unwrap();

    let receipt = execute_first_bonus_mutation(&mut state, &entry, &facts, &mut host).unwrap();

    assert_eq!(
        receipt.disposition,
        FirstBonusDisposition::Placed(PlacementPath::Player)
    );
    assert_eq!(host.requests[0].params.good_id, -1);
    assert_eq!(host.requests[0].params.selector, 2);
    assert_eq!(
        receipt.placement.as_ref().unwrap().random_draws[0].call_va,
        RESOURCE_POOL_GET_EARLY_RANDOM_CALL_VA
    );
    assert_ne!(state.resource_pool_digest, entry.resource_pool_digest);
    assert_eq!(
        state.resource_pool.as_ref().map(resource_divvy_pool_digest),
        Some(state.resource_pool_digest)
    );
    assert_eq!(state.world_checksum, entry.world_checksum);
}

#[test]
fn region_receipt_accepts_find_avail_rng_only_before_region_point_rng() {
    let world = World::init_default_rules(4, 4);
    let entry = handoff(&world, 1);
    let facts = region_facts(&entry);
    let mut state = PlaceResourcesBonusMutationState::from_handoff(&entry);
    let mut host = TranscriptPlacementHost {
        call_vas: vec![
            REGION_FIND_AVAIL_RANDOM_CALL_VA,
            REGION_PLACEMENT_RANDOM_CALL_VA,
        ],
        corrupt_state_at: None,
    };

    let receipt = execute_first_bonus_mutation(&mut state, &entry, &facts, &mut host).unwrap();

    assert_eq!(
        receipt.disposition,
        FirstBonusDisposition::Placed(PlacementPath::Region)
    );
    assert_eq!(
        receipt
            .placement
            .as_ref()
            .unwrap()
            .random_draws
            .iter()
            .map(|draw| draw.call_va)
            .collect::<Vec<_>>(),
        vec![
            REGION_FIND_AVAIL_RANDOM_CALL_VA,
            REGION_PLACEMENT_RANDOM_CALL_VA,
        ]
    );
    assert_ne!(state.random_state, entry.random_state);
}

#[test]
fn find_avail_rng_rejects_player_wrong_order_state_and_call_atomically() {
    let world = World::init_default_rules(4, 4);
    let entry = handoff(&world, 1);

    let cases = [
        (
            facts(
                &entry,
                ResourceTypeResolution::CatalogGood { good_id: 6 },
                100,
                0,
            ),
            vec![
                REGION_FIND_AVAIL_RANDOM_CALL_VA,
                PLAYER_PLACEMENT_RANDOM_CALL_VA,
            ],
            None,
            0,
        ),
        (
            region_facts(&entry),
            vec![
                REGION_PLACEMENT_RANDOM_CALL_VA,
                REGION_FIND_AVAIL_RANDOM_CALL_VA,
            ],
            None,
            1,
        ),
        (
            region_facts(&entry),
            vec![
                REGION_FIND_AVAIL_RANDOM_CALL_VA,
                REGION_PLACEMENT_RANDOM_CALL_VA,
            ],
            Some(1),
            1,
        ),
        (
            region_facts(&entry),
            vec![REGION_FIND_AVAIL_RANDOM_CALL_VA + 1],
            None,
            0,
        ),
    ];

    for (facts, call_vas, corrupt_state_at, expected_index) in cases {
        let mut state = PlaceResourcesBonusMutationState::from_handoff(&entry);
        let before = state.clone();
        let mut host = TranscriptPlacementHost {
            call_vas,
            corrupt_state_at,
        };
        assert_eq!(
            execute_first_bonus_mutation(&mut state, &entry, &facts, &mut host),
            Err(FirstBonusMutationError::InvalidCalleeRandomDraw {
                index: expected_index,
            })
        );
        assert_eq!(state, before);
    }
}

#[test]
fn malformed_callee_rng_receipt_is_rejected_atomically() {
    let world = World::init_default_rules(4, 4);
    let entry = handoff(&world, 1);
    let mut host = ScriptedPlacementHost {
        world_checksum_after: world.checksum_sections(),
        draw: true,
        pool_draw: false,
        allocation: false,
        corrupt_random: true,
        corrupt_occupancy_marker: false,
        requests: Vec::new(),
    };
    let facts = facts(
        &entry,
        ResourceTypeResolution::CatalogGood { good_id: 6 },
        100,
        0,
    );
    let mut state = PlaceResourcesBonusMutationState::from_handoff(&entry);
    let before = state.clone();

    assert_eq!(
        execute_first_bonus_mutation(&mut state, &entry, &facts, &mut host),
        Err(FirstBonusMutationError::InvalidCalleeRandomDraw { index: 0 })
    );
    assert_eq!(state, before);
}

#[test]
fn wrong_occupancy_marker_is_rejected_before_world_state_commit() {
    let world = World::init_default_rules(4, 4);
    let entry = handoff(&world, 1);
    let mut after = world.clone();
    after.wdata_mut(1, 2).down = GOOD_WDATA_DOWN_MARKER;
    after.wdata_mut(1, 2).down_who = 7;
    let mut host = ScriptedPlacementHost {
        world_checksum_after: after.checksum_sections(),
        draw: false,
        pool_draw: false,
        allocation: true,
        corrupt_random: false,
        corrupt_occupancy_marker: true,
        requests: Vec::new(),
    };
    let facts = facts(
        &entry,
        ResourceTypeResolution::CatalogGood { good_id: 6 },
        100,
        0,
    );
    let mut state = PlaceResourcesBonusMutationState::from_handoff(&entry);
    let before = state.clone();

    assert_eq!(
        execute_first_bonus_mutation(&mut state, &entry, &facts, &mut host),
        Err(FirstBonusMutationError::InvalidAllocation { index: 0 })
    );
    assert_eq!(state, before);
}
