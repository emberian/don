// SPDX-License-Identifier: GPL-3.0-or-later
//! Mutation-sensitive proof for later rows in the current `BONUSES` array.

#[path = "../src/place_resources_xml_frontier.rs"]
mod place_resources_xml_frontier;

#[path = "../src/place_resources_pool_frontier.rs"]
mod place_resources_pool_frontier;

#[path = "../src/resource_divvy_pool_selection_frontier.rs"]
mod resource_divvy_pool_selection_frontier;

#[path = "../src/place_resources_bonus_mutation_frontier.rs"]
mod place_resources_bonus_mutation_frontier;

#[path = "../src/place_resources_bonus_rows_mutation_frontier.rs"]
mod place_resources_bonus_rows_mutation_frontier;

use don_sim::systems::map_terrain::{World, COORD_PER_WCELL};
use place_resources_bonus_mutation_frontier::{
    execute_first_bonus_mutation, AllocationKind, BonusMutationEvidence, FirstBonusMutationFacts,
    PlaceResourcesBonusMutationState, PlacementEvidence, PlacementHost, PlacementPattern,
    PlacementReceipt, PlacementRequest, ResourceAllocation, ResourceTypeResolution,
    ScaledAttribute, ScaledAttributeFact, WorldOccupancyWrite, GOOD_WDATA_DOWN_MARKER,
    GOOD_WDATA_DOWN_WHO_WRITE_VA, GOOD_WDATA_DOWN_WRITE_VA, OBJECTS_INIT_GOOD_VA,
    PLAYER_INIT_GOOD_CALL_VA,
};
use place_resources_bonus_rows_mutation_frontier::{
    execute_next_bonus_mutation, later_bonus_facts_digest, LaterBonusDisposition,
    LaterBonusMutationEvidence, LaterBonusMutationFacts, RemainingBonusRowsError,
    RemainingBonusRowsState, BONUS_CATEGORY_TAIL_VA, LATER_BONUS_ENTRY_VA,
    LATER_ROW_MUTATION_ENTRY_VA,
};
use place_resources_xml_frontier::{
    BonusXmlRowFact, BonusesSectionSource, PlaceResourcesBonusRowsHandoff, XmlHostHandles,
    PLACE_RESOURCES_XML_RESIDUAL_VA,
};

const SEED: i32 = 0x1234_5678;
const POOL_DIGEST: u64 = 0x0123_4567_89ab_cdef;

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
        first_row_body_va: place_resources_bonus_mutation_frontier::FIRST_BONUS_ROW_BODY_VA,
        player_count_argument: 4,
        section_source: BonusesSectionSource::SelectedStyle,
        current_category_handles: handles(),
        rows: (0..rows)
            .map(|index| BonusXmlRowFact {
                capture_ordinal: 20 + index as u32,
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

fn scaled(num_rare: i32) -> Vec<ScaledAttributeFact> {
    use place_resources_bonus_mutation_frontier::{
        CENTER_KEEP_AWAY_SCALE_CALL_VA, CENTER_STAY_NEAR_SCALE_CALL_VA,
        CORNER_KEEP_AWAY_SCALE_CALL_VA, CORNER_STAY_NEAR_SCALE_CALL_VA,
        EDGE_KEEP_AWAY_SCALE_CALL_VA, EDGE_STAY_NEAR_SCALE_CALL_VA, GROUP_SPACING_SCALE_CALL_VA,
        NUM_RARE_SCALE_CALL_VA, PLAYER_KEEP_AWAY_SCALE_CALL_VA, PLAYER_STAY_NEAR_SCALE_CALL_VA,
    };
    [
        (ScaledAttribute::NumRare, NUM_RARE_SCALE_CALL_VA, num_rare),
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

fn first_facts(
    entry: &PlaceResourcesBonusRowsHandoff,
    chance: i32,
    chance_group: i32,
    num_rare: i32,
) -> FirstBonusMutationFacts {
    FirstBonusMutationFacts {
        capture_ordinal: entry.rows[0].capture_ordinal,
        type_name: "WHALES".to_owned(),
        type_resolution: ResourceTypeResolution::CatalogGood { good_id: 6 },
        chance,
        chance_group,
        pattern_name: "PLAYER".to_owned(),
        pattern: PlacementPattern::Player,
        saturate: 0,
        spacing: 0,
        scaled: scaled(num_rare),
        evidence: BonusMutationEvidence::SyntheticFixture {
            fixture: "later-row-first-seam".to_owned(),
            entry_va: entry.resume_va,
            capture_ordinal: entry.rows[0].capture_ordinal,
            random_state: entry.random_state,
            world_checksum: entry.world_checksum.clone(),
            sourced_walked_bytes: entry.sourced_walked_bytes,
            resource_pool_digest: entry.resource_pool_digest,
        },
    }
}

fn later_facts(
    state: &RemainingBonusRowsState,
    entry: &PlaceResourcesBonusRowsHandoff,
    chance: i32,
    chance_group: i32,
    num_rare: i32,
) -> LaterBonusMutationFacts {
    let row = &entry.rows[state.next_row_index];
    let mut facts = LaterBonusMutationFacts {
        capture_ordinal: row.capture_ordinal,
        type_name: "WHALES".to_owned(),
        type_resolution: ResourceTypeResolution::CatalogGood { good_id: 6 },
        chance,
        chance_group,
        pattern_name: "PLAYER".to_owned(),
        pattern: PlacementPattern::Player,
        saturate: 0,
        spacing: 0,
        scaled: scaled(num_rare),
        evidence: LaterBonusMutationEvidence::SyntheticFixture {
            fixture: format!("later-row-{}", state.next_row_index),
            entry_va: LATER_BONUS_ENTRY_VA,
            row_body_va: place_resources_bonus_mutation_frontier::FIRST_BONUS_ROW_BODY_VA,
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
    refresh_facts_digest(&mut facts);
    facts
}

fn refresh_facts_digest(facts: &mut LaterBonusMutationFacts) {
    let digest = later_bonus_facts_digest(facts);
    let LaterBonusMutationEvidence::SyntheticFixture { facts_digest, .. } = &mut facts.evidence
    else {
        unreachable!("test helper always constructs synthetic evidence")
    };
    *facts_digest = digest;
}

struct ScriptedHost {
    world_checksum_after: don_sim::systems::map_terrain::WorldChecksum,
    allocate: bool,
    corrupt_random: bool,
    requests: Vec<PlacementRequest>,
}

impl ScriptedHost {
    fn no_allocation(world: &World) -> Self {
        Self {
            world_checksum_after: world.checksum_sections(),
            allocate: false,
            corrupt_random: false,
            requests: Vec::new(),
        }
    }
}

impl PlacementHost for ScriptedHost {
    fn place(&mut self, request: &PlacementRequest) -> Option<PlacementReceipt> {
        self.requests.push(request.clone());
        let allocations = self
            .allocate
            .then(|| ResourceAllocation {
                call_va: PLAYER_INIT_GOOD_CALL_VA,
                callee_va: OBJECTS_INIT_GOOD_VA,
                kind: AllocationKind::Good,
                type_id: request.params.good_id,
                coord_x: COORD_PER_WCELL + COORD_PER_WCELL / 2,
                coord_y: 2 * COORD_PER_WCELL + COORD_PER_WCELL / 2,
                slot: 7,
                occupancy_write: Some(WorldOccupancyWrite {
                    world_x: 1,
                    world_y: 2,
                    down_write_va: GOOD_WDATA_DOWN_WRITE_VA,
                    down_who_write_va: GOOD_WDATA_DOWN_WHO_WRITE_VA,
                    old_down: 0,
                    old_down_who: 0,
                    new_down: GOOD_WDATA_DOWN_MARKER,
                    new_down_who: 7,
                }),
            })
            .into_iter()
            .collect::<Vec<_>>();
        Some(PlacementReceipt {
            request: request.clone(),
            random_draws: Vec::new(),
            random_state_after: if self.corrupt_random {
                request.random_state_before.wrapping_add(1)
            } else {
                request.random_state_before
            },
            allocated_count: allocations.len() as i32,
            allocations,
            world_checksum_after: self.world_checksum_after.clone(),
            sourced_walked_bytes_after: request.sourced_walked_bytes,
            resource_pool_digest_after: request.resource_pool_digest_before,
            resource_pool_selections: Vec::new(),
            resource_pool_after: request.resource_pool_before.clone(),
            evidence: PlacementEvidence::SyntheticFixture {
                fixture: "later-row-placement".to_owned(),
            },
        })
    }
}

fn carry_after_first(
    world: &World,
    entry: &PlaceResourcesBonusRowsHandoff,
    chance: i32,
    chance_group: i32,
    num_rare: i32,
) -> (RemainingBonusRowsState, ScriptedHost) {
    let facts = first_facts(entry, chance, chance_group, num_rare);
    let mut mutation = PlaceResourcesBonusMutationState::from_handoff(entry);
    let mut host = ScriptedHost::no_allocation(world);
    let receipt = execute_first_bonus_mutation(&mut mutation, entry, &facts, &mut host).unwrap();
    let carry = RemainingBonusRowsState::from_first_row(entry, &facts, &receipt, &mutation)
        .expect("valid first-row seam");
    (carry, host)
}

#[test]
fn same_nonzero_group_reuses_budget_without_a_second_draw() {
    let world = World::init_default_rules(4, 4);
    let entry = handoff(&world, 3);
    let (mut state, mut host) = carry_after_first(&world, &entry, 0, 7, 2);
    let budget = state.signed_chance_budget;
    let facts = later_facts(&state, &entry, budget.wrapping_add(1), 7, 2);

    let receipt = execute_next_bonus_mutation(&mut state, &entry, &facts, &mut host).unwrap();

    assert!(receipt.chance_draw.is_none());
    assert!(receipt.budget_subtracted);
    assert_eq!(
        receipt.disposition,
        LaterBonusDisposition::Placed(
            place_resources_bonus_mutation_frontier::PlacementPath::Player
        )
    );
    assert_eq!(
        receipt.next_va,
        place_resources_bonus_mutation_frontier::FIRST_BONUS_ROW_BODY_VA
    );
    assert_eq!(receipt.host_ref_boundary.entry_va, 0x0068_fbb3);
    assert_eq!(receipt.host_ref_boundary.residual_va, 0x0068_fc0d);
    assert!(receipt.host_ref_boundary.pointer_identity_external);
    assert_eq!(state.next_row_index, 2);
}

#[test]
fn group_zero_forces_a_fresh_draw_on_every_later_row() {
    let world = World::init_default_rules(4, 4);
    let entry = handoff(&world, 3);
    let (mut state, mut host) = carry_after_first(&world, &entry, 0, 0, 2);
    let second = later_facts(&state, &entry, 0, 0, 2);
    let receipt2 = execute_next_bonus_mutation(&mut state, &entry, &second, &mut host).unwrap();
    let third = later_facts(&state, &entry, 0, 0, 2);
    let receipt3 = execute_next_bonus_mutation(&mut state, &entry, &third, &mut host).unwrap();

    assert!(receipt2.chance_draw.is_some());
    assert!(receipt3.chance_draw.is_some());
    assert_eq!(receipt3.next_va, BONUS_CATEGORY_TAIL_VA);
    assert_eq!(state.next_row_index, entry.rows.len());
}

#[test]
fn transition_to_minus_one_draws_even_though_the_initial_minus_one_did_not() {
    let world = World::init_default_rules(4, 4);
    let entry = handoff(&world, 2);
    let (mut state, mut host) = carry_after_first(&world, &entry, 0, 5, 2);
    let facts = later_facts(&state, &entry, 0, -1, 2);

    let receipt = execute_next_bonus_mutation(&mut state, &entry, &facts, &mut host).unwrap();

    assert!(receipt.chance_draw.is_some());
    assert_eq!(receipt.last_chance_group_before, 5);
    assert_eq!(receipt.last_chance_group_after, -1);
}

#[test]
fn unknown_type_advances_only_the_row_cursor_and_preserves_bucket_state() {
    let world = World::init_default_rules(4, 4);
    let entry = handoff(&world, 2);
    let (mut state, mut host) = carry_after_first(&world, &entry, 100, 6, 2);
    let mut facts = later_facts(&state, &entry, 0, 0, 2);
    facts.type_name = "NOT_A_GOOD".to_owned();
    facts.type_resolution = ResourceTypeResolution::Unknown;
    facts.scaled.clear();
    refresh_facts_digest(&mut facts);
    let before = state.clone();

    let receipt = execute_next_bonus_mutation(&mut state, &entry, &facts, &mut host).unwrap();

    assert_eq!(receipt.disposition, LaterBonusDisposition::UnknownType);
    assert!(receipt.chance_draw.is_none());
    assert_eq!(state.next_row_index, before.next_row_index + 1);
    assert_eq!(state.last_chance_group, before.last_chance_group);
    assert_eq!(state.signed_chance_budget, before.signed_chance_budget);
    assert_eq!(state.winner_seen, before.winner_seen);
    assert_eq!(state.mutation, before.mutation);
}

#[test]
fn negative_carry_blocks_nonzero_chance_and_clears_winner() {
    let world = World::init_default_rules(4, 4);
    let entry = handoff(&world, 2);
    let (mut state, mut host) = carry_after_first(&world, &entry, 100, 9, 2);
    assert!(state.signed_chance_budget < 0);
    assert!(state.winner_seen);
    let facts = later_facts(&state, &entry, 1, 9, 2);

    let receipt = execute_next_bonus_mutation(&mut state, &entry, &facts, &mut host).unwrap();

    assert_eq!(
        receipt.disposition,
        LaterBonusDisposition::NegativeCarryBlocked
    );
    assert!(!receipt.budget_subtracted);
    assert!(!state.winner_seen);
    assert!(host.requests.len() == 1, "only the first row placed");
}

#[test]
fn zero_chance_after_a_winner_reuses_the_negative_bucket_and_places() {
    let world = World::init_default_rules(4, 4);
    let entry = handoff(&world, 2);
    let (mut state, mut host) = carry_after_first(&world, &entry, 100, 11, 2);
    let facts = later_facts(&state, &entry, 0, 11, 2);

    let receipt = execute_next_bonus_mutation(&mut state, &entry, &facts, &mut host).unwrap();

    assert_eq!(
        receipt.disposition,
        LaterBonusDisposition::Placed(
            place_resources_bonus_mutation_frontier::PlacementPath::Player
        )
    );
    assert!(receipt.chance_draw.is_none());
    assert_eq!(host.requests.len(), 2);
}

#[test]
fn zero_numrare_retains_the_native_winner_without_calling_placement() {
    let world = World::init_default_rules(4, 4);
    let entry = handoff(&world, 2);
    let (mut state, mut host) = carry_after_first(&world, &entry, 0, 13, 2);
    let budget = state.signed_chance_budget;
    let mut facts = later_facts(&state, &entry, budget.wrapping_add(1), 13, 0);
    facts.scaled.truncate(1);
    refresh_facts_digest(&mut facts);
    let calls_before = host.requests.len();

    let receipt = execute_next_bonus_mutation(&mut state, &entry, &facts, &mut host).unwrap();

    assert_eq!(receipt.disposition, LaterBonusDisposition::ZeroRequested);
    assert!(state.winner_seen);
    assert_eq!(host.requests.len(), calls_before);
}

#[test]
fn admitted_world_write_advances_only_after_the_complete_receipt_validates() {
    let world = World::init_default_rules(4, 4);
    let entry = handoff(&world, 2);
    let (mut state, _) = carry_after_first(&world, &entry, 0, 17, 2);
    let budget = state.signed_chance_budget;
    let facts = later_facts(&state, &entry, budget.wrapping_add(1), 17, 2);
    let mut after = world.clone();
    after.wdata_mut(1, 2).down = GOOD_WDATA_DOWN_MARKER;
    after.wdata_mut(1, 2).down_who = 7;
    let mut host = ScriptedHost {
        world_checksum_after: after.checksum_sections(),
        allocate: true,
        corrupt_random: false,
        requests: Vec::new(),
    };

    let receipt = execute_next_bonus_mutation(&mut state, &entry, &facts, &mut host).unwrap();

    assert_eq!(receipt.placement.as_ref().unwrap().allocated_count, 1);
    assert_eq!(state.mutation.allocated_resources, 1);
    assert_ne!(receipt.world_checksum_before, receipt.world_checksum_after);
}

#[test]
fn rejected_placement_rolls_back_cursor_chance_rng_world_pool_and_counters() {
    let world = World::init_default_rules(4, 4);
    let entry = handoff(&world, 2);
    let (mut state, _) = carry_after_first(&world, &entry, 0, 19, 2);
    let budget = state.signed_chance_budget;
    let facts = later_facts(&state, &entry, budget.wrapping_add(1), 19, 2);
    let before = state.clone();
    let mut host = ScriptedHost::no_allocation(&world);
    host.corrupt_random = true;

    assert_eq!(
        execute_next_bonus_mutation(&mut state, &entry, &facts, &mut host),
        Err(RemainingBonusRowsError::InvalidCalleeRandomDraw { index: 0 })
    );
    assert_eq!(state, before);
}

#[test]
fn a_spliced_row_array_is_rejected_before_cursor_or_carry_mutation() {
    let world = World::init_default_rules(4, 4);
    let entry = handoff(&world, 2);
    let (mut state, mut host) = carry_after_first(&world, &entry, 0, 23, 2);
    let facts = later_facts(&state, &entry, 1, 23, 2);
    let before = state.clone();
    let mut spliced = entry.clone();
    spliced.rows[1].capture_ordinal = spliced.rows[1].capture_ordinal.wrapping_add(1);

    assert_eq!(
        execute_next_bonus_mutation(&mut state, &spliced, &facts, &mut host),
        Err(RemainingBonusRowsError::WrongHandoff)
    );
    assert_eq!(state, before);
}

#[test]
fn substituted_behavior_facts_fail_the_capture_content_digest() {
    let world = World::init_default_rules(4, 4);
    let entry = handoff(&world, 2);
    let (mut state, mut host) = carry_after_first(&world, &entry, 0, 29, 2);
    let mut facts = later_facts(&state, &entry, 1, 29, 2);
    facts.chance = facts.chance.wrapping_add(1);
    let before = state.clone();

    assert_eq!(
        execute_next_bonus_mutation(&mut state, &entry, &facts, &mut host),
        Err(RemainingBonusRowsError::StaleEvidence)
    );
    assert_eq!(state, before);
}

#[test]
fn first_row_facts_cannot_be_mixed_with_a_different_valid_receipt() {
    let world = World::init_default_rules(4, 4);
    let entry = handoff(&world, 2);
    let facts = first_facts(&entry, 0, 31, 2);
    let mut mutation = PlaceResourcesBonusMutationState::from_handoff(&entry);
    let mut host = ScriptedHost::no_allocation(&world);
    let receipt = execute_first_bonus_mutation(&mut mutation, &entry, &facts, &mut host).unwrap();
    let mut substituted = facts.clone();
    substituted.chance_group = 32;

    assert_eq!(
        RemainingBonusRowsState::from_first_row(&entry, &substituted, &receipt, &mutation,),
        Err(RemainingBonusRowsError::WrongFirstRowReceipt)
    );
}
