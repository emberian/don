// SPDX-License-Identifier: GPL-3.0-or-later
//! Mutation-sensitive proof for the complete concrete-good World/region body.

#[path = "../src/place_region_resource_world_body_frontier.rs"]
mod place_region_resource_world_body_frontier;
#[path = "../src/place_resources_category_frontier.rs"]
mod place_resources_category_frontier;

use don_sim::systems::map_terrain::{World, WorldSection};
use place_region_resource_world_body_frontier::*;
use place_resources_category_frontier::*;

const GOOD_ID: i32 = 10;
const GOOD_DIGEST: u64 = 0x1122_3344_5566_7788;

fn deterministic(world: &World) -> DeterministicResourceState {
    DeterministicResourceState {
        random_state: 0x1234_5678,
        world_checksum: world.checksum_sections(),
        sourced_walked_bytes: 0x7788,
        resource_pool_digest: 0x8877_6655_4433_2211,
        allocated_resources: 13,
        requested_resources: 19,
        last_chance_group: 7,
        signed_chance_budget: -11,
        winner_seen: true,
    }
}

fn prefix_evidence(
    state: &DeterministicResourceState,
) -> place_resources_category_frontier::ResourceFrontierEvidence {
    ResourceFrontierEvidence::SyntheticFixture {
        fixture: "place-region-world-prefix".to_owned(),
        entry_va: MAP_PLACE_REGION_RESOURCE_VA,
        random_state: state.random_state,
        world_checksum: state.world_checksum.clone(),
        sourced_walked_bytes: state.sourced_walked_bytes,
        resource_pool_digest: state.resource_pool_digest,
    }
}

fn prefix_facts(state: &DeterministicResourceState) -> RegionPlacementPrefixFacts {
    let mut regions: Vec<_> = (1..=126)
        .map(|region_id| RegionPrefixFact {
            region_id,
            land_cell_count: 0,
            contains_player_start: false,
            candidate_point_count: 0,
        })
        .collect();
    regions[0].land_cell_count = 6;
    regions[0].candidate_point_count = 3;
    regions[1].land_cell_count = 6;
    regions[1].candidate_point_count = 3;
    RegionPlacementPrefixFacts {
        ocean_rare_good_ids: vec![6, 31],
        regions,
        evidence: prefix_evidence(state),
    }
}

fn parameters() -> RegionWorldBodyParameters {
    RegionWorldBodyParameters {
        good_id: GOOD_ID,
        pattern: WORLD_PATTERN,
        saturate: 0,
        num_rare: 2,
        selector: CONCRETE_SELECTOR,
        player_keep_away: 1,
        player_stay_near: 20,
        existing_resource_spacing: 1,
        group_spacing: 1,
        center_keep_away: 1,
        center_stay_near: 20,
        edge_keep_away: 1,
        edge_stay_near: 20,
        corner_keep_away: 1,
        corner_stay_near: 20,
    }
}

fn prefix(
    state: &DeterministicResourceState,
    params: RegionWorldBodyParameters,
) -> RegionPlacementPrefixReceipt {
    execute_region_placement_prefix(
        state,
        RegionPlacementPrefixRequest {
            good_id: params.good_id,
            pattern: params.pattern,
            saturate: params.saturate,
            num_rare: params.num_rare,
            selector: params.selector,
        },
        &prefix_facts(state),
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
    state: &RegionWorldBodyState,
    params: RegionWorldBodyParameters,
    prefix: &RegionPlacementPrefixReceipt,
) -> RegionWorldBodyFacts {
    let width = 12;
    let height = 12;
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
    let points = |region_id| match region_id {
        1 => vec![(3, 7), (5, 7), (7, 7)],
        2 => vec![(3, 3), (5, 3), (7, 3)],
        _ => unreachable!(),
    };
    let mut facts = RegionWorldBodyFacts {
        world_width: width,
        world_height: height,
        cells,
        starts: vec![
            RegionPlayerStartFact {
                cell_x: 2,
                cell_y: 2,
                metric_x: 2,
                metric_y: 2,
            },
            RegionPlayerStartFact {
                cell_x: 9,
                cell_y: 9,
                metric_x: 9,
                metric_y: 9,
            },
        ],
        existing_resources: vec![ExistingResourceFact {
            active: true,
            kind: ExistingResourceKind::Good { good_id: 20 },
            cell_x: 10,
            cell_y: 6,
        }],
        regions: prefix
            .available_regions
            .iter()
            .map(|region_id| RegionCandidateSetFact {
                region_id: *region_id,
                points: points(*region_id),
            })
            .collect(),
        terrain_rare: (0..=7)
            .map(|terrain_class| TerrainRareFact {
                terrain_class,
                good_ids: if terrain_class == 0 {
                    vec![GOOD_ID]
                } else {
                    Vec::new()
                },
            })
            .collect(),
        // The exact algorithm only indexes 3*x+1 / 3*y+1 for these fixtures.
        coord_metric_table: (0..64).map(|value| value / 3).collect(),
        good_uses_offset_0x120: false,
        placement_evidence: RegionBodyPlacementEvidence::SyntheticFixture {
            fixture: "region-world-body-placement".to_owned(),
        },
        evidence: RegionWorldBodyEvidence::SyntheticFixture {
            fixture: "region-world-body".to_owned(),
            entry_va: REGION_CANDIDATE_SCAN_VA,
            random_state: state.deterministic.random_state,
            world_checksum: state.deterministic.world_checksum.clone(),
            resource_pool_digest: state.deterministic.resource_pool_digest,
            good_state_digest: state.good_state_digest,
            facts_digest: 0,
            prefix_digest: region_placement_prefix_receipt_digest(prefix),
        },
    };
    let digest = region_world_body_facts_digest(params, &facts);
    if let RegionWorldBodyEvidence::SyntheticFixture { facts_digest, .. } = &mut facts.evidence {
        *facts_digest = digest;
    }
    facts
}

#[derive(Clone)]
struct TranscriptGoodHost {
    world: World,
    next_slot: i32,
    corrupt_index: Option<usize>,
    calls: usize,
}

impl TranscriptGoodHost {
    fn new(world: World) -> Self {
        Self {
            world,
            next_slot: 4,
            corrupt_index: None,
            calls: 0,
        }
    }
}

impl RegionInitGoodHost for TranscriptGoodHost {
    fn propose_init_good(
        &mut self,
        request: &RegionInitGoodRequest,
    ) -> Option<RegionInitGoodReceipt> {
        assert_eq!(
            request.world_checksum_before,
            self.world.checksum_sections()
        );
        let slot = self.next_slot;
        self.next_slot += 1;
        let occupancy_write = if request.good_id == 5 {
            None
        } else {
            self.world.set_down(
                request.world_x,
                request.world_y,
                GOOD_WDATA_DOWN_MARKER,
                slot as i16,
            );
            Some(RegionWorldOccupancyWrite {
                world_x: request.world_x,
                world_y: request.world_y,
                down_write_va: GOOD_WDATA_DOWN_WRITE_VA,
                down_who_write_va: GOOD_WDATA_DOWN_WHO_WRITE_VA,
                old_down: request.old_down,
                old_down_who: request.old_down_who,
                new_down: GOOD_WDATA_DOWN_MARKER,
                new_down_who: slot as i16,
            })
        };
        let mut receipt = RegionInitGoodReceipt {
            request: request.clone(),
            allocation: RegionGoodAllocation {
                call_va: REGION_INIT_GOOD_CALL_VA,
                callee_va: OBJECTS_INIT_GOOD_VA,
                good_id: request.good_id,
                coord_x: request.coord_x,
                coord_y: request.coord_y,
                slot,
                occupancy_write,
            },
            world_checksum_after: self.world.checksum_sections(),
            good_state_digest_after: request
                .good_state_digest_before
                .rotate_left(7)
                .wrapping_add(slot as u64)
                ^ ((request.world_x as u64) << 32)
                ^ request.world_y as u64,
            sourced_walked_bytes_after: request.sourced_walked_bytes,
        };
        if self.corrupt_index == Some(self.calls) {
            receipt
                .allocation
                .occupancy_write
                .as_mut()
                .unwrap()
                .new_down = -3;
        }
        self.calls += 1;
        Some(receipt)
    }
}

#[test]
fn world_pattern_walks_every_region_and_carries_rng_world_pool_good_and_chance_state() {
    let world = World::init_default_rules(12, 12);
    let deterministic = deterministic(&world);
    let params = parameters();
    let prefix = prefix(&deterministic, params);
    let mut state = RegionWorldBodyState {
        deterministic,
        good_state_digest: GOOD_DIGEST,
    };
    let before = state.clone();
    let facts = body_facts(&state, params, &prefix);
    let mut host = TranscriptGoodHost::new(world);
    let receipt =
        execute_region_world_body(&mut state, params, &prefix, &facts, &mut host).unwrap();

    assert_eq!(receipt.circular_region_order.len(), 2);
    assert_eq!(
        receipt.circular_region_order[0],
        match prefix.disposition {
            RegionPrefixDisposition::CandidateScan {
                selected_region, ..
            } => selected_region,
            _ => unreachable!(),
        }
    );
    assert_eq!(receipt.attempts.len(), 4);
    assert_eq!(receipt.allocations.len(), 4);
    assert_eq!(receipt.return_count, 4);
    assert_eq!(receipt.function_return_value_va, 0x0069_1f52);
    assert_eq!(receipt.caller_return_accumulate_va, 0x0069_01f1);
    assert_eq!(receipt.random_draws[0].call_va, 0x0068_f498);
    assert!(receipt.random_draws[1..]
        .iter()
        .all(|draw| draw.call_va == 0x0069_071a));
    assert_eq!(receipt.random_draws.len(), 5);
    assert_eq!(state.deterministic.allocated_resources, 17);
    assert_eq!(
        state.deterministic.requested_resources,
        before.deterministic.requested_resources
    );
    assert_eq!(
        state.deterministic.resource_pool_digest,
        before.deterministic.resource_pool_digest
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
    assert_ne!(state.good_state_digest, GOOD_DIGEST);
    assert_eq!(receipt.good_state_digest_after, state.good_state_digest);
    assert!(receipt
        .allocations
        .iter()
        .all(|allocation| allocation.call_va == REGION_INIT_GOOD_CALL_VA
            && allocation.callee_va == OBJECTS_INIT_GOOD_VA
            && allocation.occupancy_write.is_some()));

    let accepted_after_first = receipt
        .attempts
        .iter()
        .skip(1)
        .flat_map(|attempt| &attempt.candidates)
        .find(|candidate| candidate.rejection.is_none())
        .unwrap();
    let kinds: Vec<_> = accepted_after_first
        .observations
        .iter()
        .map(|observation| observation.kind)
        .collect();
    let ordered = [
        RegionFilterKind::PrimaryStartOccupied,
        RegionFilterKind::PlayerKeepAway,
        RegionFilterKind::ShadowFlags10,
        RegionFilterKind::ShadowFlags68,
        RegionFilterKind::ShadowFlags1800,
        RegionFilterKind::ShadowOccupant,
        RegionFilterKind::PlayerStayNear,
        RegionFilterKind::CenterKeepAway,
        RegionFilterKind::CenterStayNear,
        RegionFilterKind::EdgeKeepAway,
        RegionFilterKind::EdgeStayNear,
        RegionFilterKind::CornerKeepAway,
        RegionFilterKind::CornerStayNear,
        RegionFilterKind::GroupSpacing,
        RegionFilterKind::ExistingResourceSpacing,
        RegionFilterKind::LandNeighbor,
        RegionFilterKind::HasMountainTcoords,
        RegionFilterKind::Border,
        RegionFilterKind::HasSouthForest,
        RegionFilterKind::TerrainRare,
        RegionFilterKind::DownChain,
    ];
    let positions: Vec<_> = ordered
        .iter()
        .map(|kind| kinds.iter().position(|actual| actual == kind).unwrap())
        .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn failed_first_point_uses_native_filter_order_and_lfsr_without_an_extra_draw() {
    let world = World::init_default_rules(12, 12);
    let deterministic = deterministic(&world);
    let mut params = parameters();
    params.num_rare = 1;
    let prefix = prefix(&deterministic, params);
    let mut state = RegionWorldBodyState {
        deterministic,
        good_state_digest: GOOD_DIGEST,
    };
    let mut facts = body_facts(&state, params, &prefix);
    let RegionPrefixDisposition::CandidateScan {
        selected_region,
        point_index,
        ..
    } = prefix.disposition
    else {
        unreachable!()
    };
    let region = facts
        .regions
        .iter()
        .find(|region| region.region_id == selected_region)
        .unwrap();
    let (x, y) = region.points[point_index as usize];
    facts.cells[(y * facts.world_width + x) as usize].shadow_flags = 0x10;
    let digest = region_world_body_facts_digest(params, &facts);
    if let RegionWorldBodyEvidence::SyntheticFixture { facts_digest, .. } = &mut facts.evidence {
        *facts_digest = digest;
    }
    let mut host = TranscriptGoodHost::new(world);
    let receipt =
        execute_region_world_body(&mut state, params, &prefix, &facts, &mut host).unwrap();
    let first_attempt = &receipt.attempts[0];
    assert!(first_attempt.candidates.len() >= 2);
    let first = &first_attempt.candidates[0];
    assert_eq!(first.rejection, Some(RegionFilterKind::ShadowFlags10));
    assert_eq!(
        first.observations[0].kind,
        RegionFilterKind::PrimaryStartOccupied
    );
    let shadow_position = first
        .observations
        .iter()
        .position(|observation| observation.kind == RegionFilterKind::ShadowFlags10)
        .unwrap();
    assert!(first.observations[1..shadow_position]
        .iter()
        .all(|observation| observation.kind == RegionFilterKind::PlayerKeepAway));
    assert!(first_attempt.allocation_index.is_some());
    // find-avail + one initial point draw per region; candidate permutation itself is draw-free.
    assert_eq!(receipt.random_draws.len(), 3);
}

#[test]
fn wrong_good_receipt_refuses_atomically_after_the_full_candidate_scan() {
    let world = World::init_default_rules(12, 12);
    let deterministic = deterministic(&world);
    let params = parameters();
    let prefix = prefix(&deterministic, params);
    let mut state = RegionWorldBodyState {
        deterministic,
        good_state_digest: GOOD_DIGEST,
    };
    let before = state.clone();
    let facts = body_facts(&state, params, &prefix);
    let mut host = TranscriptGoodHost::new(world);
    host.corrupt_index = Some(0);
    assert_eq!(
        execute_region_world_body(&mut state, params, &prefix, &facts, &mut host),
        Err(RegionWorldBodyError::InvalidAllocationReceipt { index: 0 })
    );
    assert_eq!(state, before);
}

#[test]
fn one_region_limit_uses_attempt_index_and_no_start_keeps_native_sentinel() {
    let world = World::init_default_rules(12, 12);
    let deterministic = deterministic(&world);
    let mut params = parameters();
    params.num_rare = 4;
    let mut prefix_catalog = prefix_facts(&deterministic);
    prefix_catalog.regions[1].land_cell_count = 0;
    prefix_catalog.regions[1].candidate_point_count = 0;
    let prefix = execute_region_placement_prefix(
        &deterministic,
        RegionPlacementPrefixRequest {
            good_id: params.good_id,
            pattern: params.pattern,
            saturate: params.saturate,
            num_rare: params.num_rare,
            selector: params.selector,
        },
        &prefix_catalog,
    )
    .unwrap();
    assert_eq!(prefix.per_region_limit, 2);

    let mut state = RegionWorldBodyState {
        deterministic,
        good_state_digest: GOOD_DIGEST,
    };
    let mut facts = body_facts(&state, params, &prefix);
    facts.starts.clear();
    let digest = region_world_body_facts_digest(params, &facts);
    if let RegionWorldBodyEvidence::SyntheticFixture { facts_digest, .. } = &mut facts.evidence {
        *facts_digest = digest;
    }
    let mut host = TranscriptGoodHost::new(world);
    let receipt =
        execute_region_world_body(&mut state, params, &prefix, &facts, &mut host).unwrap();
    assert_eq!(receipt.attempts.len(), 4);
    assert_eq!(receipt.return_count, 2);
    assert!(receipt.attempts[2..]
        .iter()
        .flat_map(|attempt| &attempt.candidates)
        .flat_map(|candidate| &candidate.observations)
        .any(
            |observation| observation.kind == RegionFilterKind::SaturationOwner
                && !observation.passed
                && observation.subject_index.is_none()
        ));
}

#[test]
fn stale_evidence_wrong_prefix_and_external_paths_refuse_atomically() {
    let world = World::init_default_rules(12, 12);
    let deterministic = deterministic(&world);
    let params = parameters();
    let prefix = prefix(&deterministic, params);
    let state = RegionWorldBodyState {
        deterministic,
        good_state_digest: GOOD_DIGEST,
    };

    let mut stale_state = state.clone();
    let mut stale_facts = body_facts(&stale_state, params, &prefix);
    if let RegionWorldBodyEvidence::SyntheticFixture { random_state, .. } =
        &mut stale_facts.evidence
    {
        *random_state ^= 1;
    }
    let before = stale_state.clone();
    let mut host = TranscriptGoodHost::new(world.clone());
    assert_eq!(
        execute_region_world_body(&mut stale_state, params, &prefix, &stale_facts, &mut host),
        Err(RegionWorldBodyError::StaleEvidence)
    );
    assert_eq!(stale_state, before);

    let mut wrong_prefix_state = state.clone();
    let facts = body_facts(&wrong_prefix_state, params, &prefix);
    let mut wrong_prefix = prefix.clone();
    wrong_prefix.random_state_after ^= 1;
    let before = wrong_prefix_state.clone();
    let mut host = TranscriptGoodHost::new(world.clone());
    assert_eq!(
        execute_region_world_body(
            &mut wrong_prefix_state,
            params,
            &wrong_prefix,
            &facts,
            &mut host
        ),
        Err(RegionWorldBodyError::StaleEvidence)
    );
    assert_eq!(wrong_prefix_state, before);

    for (mut external, error) in [
        (
            {
                let mut p = params;
                p.pattern = 4;
                p
            },
            RegionWorldBodyError::UnsupportedPattern,
        ),
        (
            {
                let mut p = params;
                p.selector = 2;
                p
            },
            RegionWorldBodyError::UnsupportedSelector,
        ),
        (
            {
                let mut p = params;
                p.good_id = ITEM_GOOD_ID;
                p
            },
            RegionWorldBodyError::UnsupportedGood,
        ),
    ] {
        let mut external_state = state.clone();
        let before = external_state.clone();
        let facts = body_facts(&external_state, external, &prefix);
        let mut host = TranscriptGoodHost::new(world.clone());
        assert_eq!(
            execute_region_world_body(&mut external_state, external, &prefix, &facts, &mut host),
            Err(error)
        );
        assert_eq!(external_state, before);
    }
}

#[test]
fn native_distance_and_static_tables_are_frozen() {
    assert_eq!(retail_approx_distance(0, 0, 3, 4), 5);
    assert_eq!(
        NEIGHBOR_OFFSETS,
        [
            (-1, -1),
            (0, -1),
            (1, -1),
            (1, 0),
            (1, 1),
            (0, 1),
            (-1, 1),
            (-1, 0),
            (-1, -2),
            (0, -2),
            (1, -2),
            (2, -1),
            (2, 0),
            (2, 1),
            (1, 2),
            (0, 2),
            (-1, 2),
            (-2, 1),
            (-2, 0),
            (-2, -1),
            (-2, -2),
            (2, -2),
            (2, 2),
            (-2, 2),
        ]
    );
    assert_eq!(CORNER_MULTIPLIERS, [(0, 0), (0, 1), (1, 0), (1, 1)]);
    assert_eq!(WORLD_DATA_HAS_MOUNTAIN_TCOORDS_VA, 0x006b_3050);
    assert_eq!(TERRAIN_HAS_SOUTH_FOREST_VA, 0x0085_0060);
}
