// SPDX-License-Identifier: GPL-3.0-or-later
//! Mutation-sensitive proof for the complete player resource body.

pub use don_replay::place_region_resource_world_body_frontier;
pub use don_replay::place_resources_pool_frontier;
pub use don_replay::resource_divvy_pool_selection_frontier;

#[path = "../src/place_player_resource_body_frontier.rs"]
mod place_player_resource_body_frontier;
#[path = "../src/place_resources_category_frontier.rs"]
pub mod place_resources_category_frontier;

use don_replay::place_region_resource_world_body_frontier::{
    DownChainFact, ExistingResourceFact, RegionWorldCellFact, TerrainRareFact,
};
use don_replay::place_resources_pool_frontier::{
    resource_divvy_pool_digest, ResourceDivvyPoolState, ResourcePoolBitMask,
};
use don_sim::systems::map_terrain::{World, WorldSection};
use place_player_resource_body_frontier::*;
use place_resources_category_frontier::*;

const GOOD_DIGEST: u64 = 0x1122_3344_5566_7788;
const ITEM_DIGEST: u64 = 0x8877_6655_4433_2211;

fn pool() -> ResourceDivvyPoolState {
    ResourceDivvyPoolState {
        early_bits: ResourcePoolBitMask {
            bits: 2,
            bytes: vec![0],
        },
        early_goods: vec![10, 11],
        late_bits: ResourcePoolBitMask {
            bits: 2,
            bytes: vec![0],
        },
        late_goods: vec![12, 13],
        water_bits: ResourcePoolBitMask {
            bits: 2,
            bytes: vec![0],
        },
        water_goods: vec![6, 31],
    }
}

fn deterministic(world: &World, pool: &ResourceDivvyPoolState) -> DeterministicResourceState {
    DeterministicResourceState {
        random_state: 0x1234_5678,
        world_checksum: world.checksum_sections(),
        sourced_walked_bytes: 0x7788,
        resource_pool_digest: resource_divvy_pool_digest(pool),
        allocated_resources: 13,
        requested_resources: 19,
        last_chance_group: 7,
        signed_chance_budget: -11,
        winner_seen: true,
    }
}

fn parameters(good_id: i32, selector: i32) -> PlayerResourceBodyParameters {
    PlayerResourceBodyParameters {
        player_count: 2,
        good_id,
        num_rare: 1,
        player_keep_away: 1,
        player_stay_near: 4,
        existing_resource_spacing: 1,
        placed_resource_spacing: 1,
        center_keep_away: 0,
        center_stay_near: 20,
        edge_keep_away: 1,
        edge_stay_near: 20,
        corner_keep_away: 1,
        corner_stay_near: 30,
        selector,
    }
}

fn prefix_facts(state: &DeterministicResourceState) -> PlayerPlacementPrefixFacts {
    let mut ring_ends = vec![0; 65];
    ring_ends[2..].fill(4);
    PlayerPlacementPrefixFacts {
        world_width: 16,
        world_height: 16,
        starts: vec![
            PlayerStartFact { x: 4, y: 4 },
            PlayerStartFact { x: 11, y: 11 },
        ],
        spiral_ring_ends: ring_ends,
        evidence: ResourceFrontierEvidence::SyntheticFixture {
            fixture: "player-resource-prefix".to_owned(),
            entry_va: MAP_PLACE_PLAYER_RESOURCE_VA,
            random_state: state.random_state,
            world_checksum: state.world_checksum.clone(),
            sourced_walked_bytes: state.sourced_walked_bytes,
            resource_pool_digest: state.resource_pool_digest,
        },
    }
}

fn prefix(
    state: &DeterministicResourceState,
    params: PlayerResourceBodyParameters,
    facts: &PlayerPlacementPrefixFacts,
) -> PlayerPlacementPrefixReceipt {
    execute_player_placement_prefix(
        state,
        PlayerPlacementPrefixRequest {
            player_count: params.player_count,
            good_id: params.good_id,
            num_rare: params.num_rare,
            spacing: params.player_keep_away,
            group_spacing: params.player_stay_near,
            selector: params.selector,
        },
        facts,
    )
    .unwrap()
}

fn empty_chain() -> DownChainFact {
    DownChainFact {
        initial_index: -1,
        initial_who: 0,
        links: Vec::new(),
    }
}

fn body_facts(
    state: &PlayerResourceBodyState,
    params: PlayerResourceBodyParameters,
    prefix_facts: &PlayerPlacementPrefixFacts,
    prefix: &PlayerPlacementPrefixReceipt,
) -> PlayerResourceBodyFacts {
    let width = prefix_facts.world_width;
    let height = prefix_facts.world_height;
    let cells = (0..height)
        .flat_map(|y| {
            (0..width).map(move |x| RegionWorldCellFact {
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
    let mut facts = PlayerResourceBodyFacts {
        spiral_x_offsets: vec![2, 0, -2, 0],
        spiral_y_offsets: vec![0, 2, 0, -2],
        cells,
        start_occupied: vec![false; (width * height) as usize],
        existing_resources: Vec::<ExistingResourceFact>::new(),
        terrain_rare: (0..=7)
            .map(|terrain_class| TerrainRareFact {
                terrain_class,
                good_ids: if terrain_class == 0 {
                    vec![10, 11, 12, 13]
                } else {
                    Vec::new()
                },
            })
            .collect(),
        good_catalog: vec![
            GoodPlacementFact {
                good_id: 10,
                uses_offset_0x120: false,
            },
            GoodPlacementFact {
                good_id: 11,
                uses_offset_0x120: true,
            },
            GoodPlacementFact {
                good_id: 12,
                uses_offset_0x120: false,
            },
            GoodPlacementFact {
                good_id: 13,
                uses_offset_0x120: true,
            },
        ],
        placement_evidence: PlayerBodyPlacementEvidence::SyntheticFixture {
            fixture: "player-resource-allocation".to_owned(),
        },
        evidence: PlayerResourceBodyEvidence::SyntheticFixture {
            fixture: "player-resource-body".to_owned(),
            entry_va: MAP_PLACE_PLAYER_RESOURCE_VA,
            random_state: state.deterministic.random_state,
            world_checksum: state.deterministic.world_checksum.clone(),
            sourced_walked_bytes: state.deterministic.sourced_walked_bytes,
            resource_pool_digest: state.deterministic.resource_pool_digest,
            good_state_digest: state.good_state_digest,
            item_state_digest: state.item_state_digest,
            facts_digest: 0,
            prefix_digest: player_placement_prefix_receipt_digest(prefix),
        },
    };
    let digest = player_resource_body_facts_digest(params, prefix_facts, &facts);
    if let PlayerResourceBodyEvidence::SyntheticFixture { facts_digest, .. } = &mut facts.evidence {
        *facts_digest = digest;
    }
    facts
}

#[derive(Clone)]
struct TranscriptHost {
    world: World,
    next_slot: i32,
    calls: usize,
    corrupt_index: Option<usize>,
}

impl TranscriptHost {
    fn new(world: World) -> Self {
        Self {
            world,
            next_slot: 4,
            calls: 0,
            corrupt_index: None,
        }
    }
}

impl PlayerAllocationHost for TranscriptHost {
    fn propose_allocation(
        &mut self,
        request: &PlayerAllocationRequest,
    ) -> Option<PlayerAllocationReceipt> {
        assert_eq!(
            request.world_checksum_before,
            self.world.checksum_sections()
        );
        let slot = self.next_slot;
        self.next_slot += 1;
        let (marker, down_write_va, down_who_write_va) = match request.kind {
            PlayerAllocationKind::Good { .. } => (
                GOOD_WDATA_DOWN_MARKER,
                GOOD_WDATA_DOWN_WRITE_VA,
                GOOD_WDATA_DOWN_WHO_WRITE_VA,
            ),
            PlayerAllocationKind::Item => (
                ITEM_WDATA_DOWN_MARKER,
                ITEM_WDATA_DOWN_WRITE_VA,
                ITEM_WDATA_DOWN_WHO_WRITE_VA,
            ),
        };
        self.world
            .set_down(request.world_x, request.world_y, marker, slot as i16);
        let mut allocation = PlayerResourceAllocation {
            call_va: request.call_va,
            callee_va: request.callee_va,
            kind: request.kind,
            coord_x: request.coord_x,
            coord_y: request.coord_y,
            slot,
            occupancy_write: Some(PlayerWorldOccupancyWrite {
                world_x: request.world_x,
                world_y: request.world_y,
                down_write_va,
                down_who_write_va,
                old_down: request.old_down,
                old_down_who: request.old_down_who,
                new_down: marker,
                new_down_who: slot as i16,
            }),
        };
        if self.corrupt_index == Some(self.calls) {
            allocation.occupancy_write.as_mut().unwrap().new_down = -9;
        }
        self.calls += 1;
        Some(PlayerAllocationReceipt {
            request: request.clone(),
            allocation,
            world_checksum_after: self.world.checksum_sections(),
            good_state_digest_after: match request.kind {
                PlayerAllocationKind::Good { .. } => request
                    .good_state_digest_before
                    .rotate_left(7)
                    .wrapping_add(slot as u64),
                PlayerAllocationKind::Item => request.good_state_digest_before,
            },
            item_state_digest_after: match request.kind {
                PlayerAllocationKind::Good { .. } => request.item_state_digest_before,
                PlayerAllocationKind::Item => request
                    .item_state_digest_before
                    .rotate_left(9)
                    .wrapping_add(slot as u64),
            },
            sourced_walked_bytes_after: request.sourced_walked_bytes,
        })
    }
}

fn state_and_world() -> (PlayerResourceBodyState, World) {
    let world = World::init_default_rules(16, 16);
    let resource_pool = pool();
    let deterministic = deterministic(&world, &resource_pool);
    (
        PlayerResourceBodyState {
            deterministic,
            resource_pool,
            good_state_digest: GOOD_DIGEST,
            item_state_digest: ITEM_DIGEST,
        },
        world,
    )
}

#[test]
fn item_path_walks_all_players_and_carries_rng_world_pool_and_category_state() {
    let (mut state, world) = state_and_world();
    let params = parameters(ITEM_GOOD_ID, 0);
    let prefix_facts = prefix_facts(&state.deterministic);
    let prefix = prefix(&state.deterministic, params, &prefix_facts);
    let facts = body_facts(&state, params, &prefix_facts, &prefix);
    let before = state.clone();
    let mut host = TranscriptHost::new(world);
    let receipt = execute_player_resource_body(
        &mut state,
        params,
        &prefix_facts,
        &prefix,
        &facts,
        &mut host,
    )
    .unwrap();

    assert_eq!(receipt.attempts.len(), 2);
    assert_eq!(receipt.allocations.len(), 2);
    assert_eq!(receipt.return_count, 2);
    assert_eq!(receipt.outer_player_tail_va, 0x0069_2e0d);
    assert_eq!(receipt.function_return_value_va, 0x0069_2e40);
    assert_eq!(receipt.caller_return_accumulate_va, 0x0069_00e1);
    assert_eq!(receipt.random_draws.len(), 2);
    assert_eq!(receipt.random_draws[0], prefix.point_draw.unwrap().into());
    assert!(receipt.allocations.iter().all(|allocation| allocation.kind
        == PlayerAllocationKind::Item
        && allocation.call_va == PLAYER_INIT_ITEM_CALL_VA
        && allocation.callee_va == OBJECTS_INIT_ITEM_VA
        && allocation.occupancy_write.as_ref().unwrap().new_down == ITEM_WDATA_DOWN_MARKER));
    assert_eq!(state.deterministic.allocated_resources, 15);
    assert_eq!(
        state.deterministic.resource_pool_digest,
        before.deterministic.resource_pool_digest
    );
    assert_eq!(state.resource_pool, before.resource_pool);
    assert_eq!(state.good_state_digest, before.good_state_digest);
    assert_ne!(state.item_state_digest, before.item_state_digest);
    assert_eq!(
        before
            .deterministic
            .world_checksum
            .differing_sections(&state.deterministic.world_checksum),
        [WorldSection::WData]
    );
    assert_eq!(
        state.deterministic.world_checksum,
        host.world.checksum_sections()
    );
    assert_eq!(
        state.deterministic.requested_resources,
        before.deterministic.requested_resources
    );
    assert_eq!(
        state.deterministic.sourced_walked_bytes,
        before.deterministic.sourced_walked_bytes
    );
    assert_eq!(
        state.deterministic.last_chance_group,
        before.deterministic.last_chance_group
    );
    assert_eq!(
        state.deterministic.signed_chance_budget,
        before.deterministic.signed_chance_budget
    );
    assert_eq!(
        state.deterministic.winner_seen,
        before.deterministic.winner_seen
    );

    let accepted = receipt.attempts[0]
        .candidates
        .iter()
        .find(|candidate| candidate.rejection.is_none())
        .unwrap();
    let kinds: Vec<_> = accepted
        .observations
        .iter()
        .map(|observation| observation.kind)
        .collect();
    for pair in [
        (PlayerFilterKind::Bounds, PlayerFilterKind::StartBit),
        (
            PlayerFilterKind::StartBit,
            PlayerFilterKind::PrimaryStartOccupied,
        ),
        (
            PlayerFilterKind::PrimaryStartOccupied,
            PlayerFilterKind::ShadowFlags10,
        ),
        (
            PlayerFilterKind::ShadowOccupant,
            PlayerFilterKind::PlayerKeepAway,
        ),
        (
            PlayerFilterKind::PlayerKeepAway,
            PlayerFilterKind::PlayerStayNear,
        ),
        (PlayerFilterKind::HasSouthForest, PlayerFilterKind::Border),
        (
            PlayerFilterKind::Border,
            PlayerFilterKind::HasMountainTcoords,
        ),
        (
            PlayerFilterKind::HasMountainTcoords,
            PlayerFilterKind::ItemTerrain,
        ),
        (PlayerFilterKind::ItemTerrain, PlayerFilterKind::DownChain),
    ] {
        let left = kinds.iter().position(|kind| *kind == pair.0).unwrap();
        let right = kinds.iter().position(|kind| *kind == pair.1).unwrap();
        assert!(left < right, "wrong filter order for {pair:?}");
    }
}

#[test]
fn early_selector_draws_pool_before_spiral_on_every_attempt_and_allocates_goods() {
    let (mut state, world) = state_and_world();
    let params = parameters(-1, 2);
    let prefix_facts = prefix_facts(&state.deterministic);
    let prefix = prefix(&state.deterministic, params, &prefix_facts);
    assert!(matches!(
        prefix.disposition,
        PlayerPrefixDisposition::PoolSelectionBoundary {
            call_va: PLAYER_POOL_SELECTION_VA
        }
    ));
    let facts = body_facts(&state, params, &prefix_facts, &prefix);
    let before = state.clone();
    let mut host = TranscriptHost::new(world);
    let receipt = execute_player_resource_body(
        &mut state,
        params,
        &prefix_facts,
        &prefix,
        &facts,
        &mut host,
    )
    .unwrap();

    assert_eq!(receipt.attempts.len(), 2);
    assert_eq!(receipt.pool_selections.len(), 2);
    assert_eq!(receipt.allocations.len(), 2);
    assert_eq!(receipt.random_draws.len(), 2);
    let mut selected = receipt
        .attempts
        .iter()
        .map(|attempt| attempt.resolved_good_id)
        .collect::<Vec<_>>();
    selected.sort_unstable();
    assert_eq!(selected, vec![10, 11]);
    for attempt in &receipt.attempts {
        let selection = attempt.pool_selection.as_ref().unwrap();
        assert_eq!(attempt.selector_call_va, Some(PLAYER_GET_EARLY_CALL_VA));
        assert_eq!(
            selection.lane,
            resource_divvy_pool_selection_frontier::ResourcePoolLane::Early
        );
        assert_eq!(
            selection.random_state_after,
            attempt.point_draw.unwrap().state_before
        );
        assert_eq!(
            selection.pool_digest_after,
            resource_divvy_pool_digest(&selection.pool_after)
        );
    }
    assert!(receipt.pool_selections[1].cleared_after_exhaustion);
    assert_eq!(state.resource_pool.early_bits.bytes, vec![0]);
    assert_eq!(
        state.deterministic.resource_pool_digest,
        resource_divvy_pool_digest(&state.resource_pool)
    );
    assert_eq!(
        receipt.attempts[0].point_draw.unwrap().state_after,
        receipt.attempts[1]
            .pool_selection
            .as_ref()
            .unwrap()
            .random_state_before
    );
    assert_ne!(state.good_state_digest, before.good_state_digest);
    assert_eq!(state.item_state_digest, before.item_state_digest);
    assert!(receipt.allocations.iter().all(|allocation| matches!(
        allocation.kind,
        PlayerAllocationKind::Good { good_id: 10 | 11 }
    )));
}

#[test]
fn invalid_first_start_skips_to_the_next_player_before_any_prefix_draw() {
    let (mut state, world) = state_and_world();
    let params = parameters(ITEM_GOOD_ID, 0);
    let mut prefix_facts = prefix_facts(&state.deterministic);
    prefix_facts.starts[0] = PlayerStartFact { x: -1, y: -1 };
    let prefix = prefix(&state.deterministic, params, &prefix_facts);
    assert!(matches!(
        prefix.disposition,
        PlayerPrefixDisposition::InvalidFirstStart {
            path_va: OUTER_PLAYER_TAIL_VA
        }
    ));
    assert!(prefix.point_draw.is_none());
    let facts = body_facts(&state, params, &prefix_facts, &prefix);
    let mut host = TranscriptHost::new(world);
    let receipt = execute_player_resource_body(
        &mut state,
        params,
        &prefix_facts,
        &prefix,
        &facts,
        &mut host,
    )
    .unwrap();
    assert_eq!(receipt.attempts.len(), 1);
    assert_eq!(receipt.attempts[0].player_index, 1);
    assert_eq!(receipt.random_draws.len(), 1);
    assert_eq!(receipt.return_count, 1);
}

#[test]
fn rejected_first_spiral_cell_advances_linearly_without_an_extra_draw() {
    let (mut state, world) = state_and_world();
    let params = parameters(ITEM_GOOD_ID, 0);
    let prefix_facts = prefix_facts(&state.deterministic);
    let prefix = prefix(&state.deterministic, params, &prefix_facts);
    let mut facts = body_facts(&state, params, &prefix_facts, &prefix);
    let PlayerPrefixDisposition::CandidateScan {
        first_candidate_table_index,
        ..
    } = prefix.disposition
    else {
        unreachable!()
    };
    let start = prefix_facts.starts[0];
    let x = start.x + i32::from(facts.spiral_x_offsets[first_candidate_table_index as usize]);
    let y = start.y + i32::from(facts.spiral_y_offsets[first_candidate_table_index as usize]);
    facts.cells[(y * prefix_facts.world_width + x) as usize].shadow_flags = 0x800;
    let digest = player_resource_body_facts_digest(params, &prefix_facts, &facts);
    if let PlayerResourceBodyEvidence::SyntheticFixture { facts_digest, .. } = &mut facts.evidence {
        *facts_digest = digest;
    }
    let mut host = TranscriptHost::new(world);
    let receipt = execute_player_resource_body(
        &mut state,
        params,
        &prefix_facts,
        &prefix,
        &facts,
        &mut host,
    )
    .unwrap();
    let attempt = &receipt.attempts[0];
    assert!(attempt.candidates.len() >= 2);
    assert_eq!(
        attempt.candidates[0].rejection,
        Some(PlayerFilterKind::ShadowFlags800)
    );
    assert!(attempt.allocation_index.is_some());
    assert_eq!(receipt.random_draws.len(), 2);
}

#[test]
fn wrong_allocation_and_mutated_prefix_refuse_rng_pool_world_and_object_state_atomically() {
    let (state, world) = state_and_world();
    let params = parameters(-1, 1);
    let prefix_facts = prefix_facts(&state.deterministic);
    let prefix = prefix(&state.deterministic, params, &prefix_facts);
    let facts = body_facts(&state, params, &prefix_facts, &prefix);

    let mut corrupt_state = state.clone();
    let before = corrupt_state.clone();
    let mut host = TranscriptHost::new(world.clone());
    host.corrupt_index = Some(0);
    assert_eq!(
        execute_player_resource_body(
            &mut corrupt_state,
            params,
            &prefix_facts,
            &prefix,
            &facts,
            &mut host,
        ),
        Err(PlayerResourceBodyError::InvalidAllocationReceipt { index: 0 })
    );
    assert_eq!(corrupt_state, before);

    let mut wrong_prefix = prefix.clone();
    wrong_prefix.random_state_after ^= 1;
    let mut wrong_state = state.clone();
    let before = wrong_state.clone();
    let mut host = TranscriptHost::new(world);
    assert_eq!(
        execute_player_resource_body(
            &mut wrong_state,
            params,
            &prefix_facts,
            &wrong_prefix,
            &facts,
            &mut host,
        ),
        Err(PlayerResourceBodyError::WrongPrefix)
    );
    assert_eq!(wrong_state, before);
}

#[test]
fn static_executable_boundaries_are_frozen() {
    assert_eq!(PLAYER_GET_EARLY_CALL_VA, 0x0069_20b5);
    assert_eq!(PLAYER_GET_LATE_CALL_VA, 0x0069_20bc);
    assert_eq!(PLAYER_RANDOM_GET_CALL_VA, 0x0069_2114);
    assert_eq!(HAS_SOUTH_FOREST_CALL_VA, 0x0069_285c);
    assert_eq!(HAS_MOUNTAIN_CALL_VA, 0x0069_2c06);
    assert_eq!(PLAYER_INIT_GOOD_CALL_VA, 0x0069_2d90);
    assert_eq!(PLAYER_INIT_ITEM_CALL_VA, 0x0069_2db5);
    assert_eq!(FUNCTION_RETURN_VALUE_VA, 0x0069_2e40);
}
