// SPDX-License-Identifier: GPL-3.0-or-later
//! Mutation-sensitive source-only proof for retail `TerrainGroups::nubify_forest`.

mod check_player_forest {
    pub use don_replay::check_player_forest::*;
}

mod initial {
    pub use don_replay::initial::*;
}

#[path = "../src/nubify_forest_frontier.rs"]
mod nubify_forest_frontier;

use don_replay::check_player_forest::{
    CheckPlayerForestReceipt, MAP_CHECK_PLAYER_FOREST_CALLER_RESUME_VA,
    MAP_CHECK_PLAYER_FOREST_CALL_VA, MAP_CHECK_PLAYER_FOREST_RET_VA, MAP_CHECK_PLAYER_FOREST_VA,
    TERRAIN_GROUPS_NUBIFY_FOREST_VA,
};
use don_replay::initial::InitialWorld;
use don_sim::systems::map_terrain::{land, wflag, WData, World, WorldChecksum, WorldSection};
use don_sim::systems::regions::Regions;
use nubify_forest_frontier::{
    execute_nubify_forest_frontier, EdgeOfRegionHost, EdgeOfRegionProvenance, EdgeOfRegionReceipt,
    EdgeOfRegionRequest, NubifyForestError, NubifyPass, NubifyRandomUse, COLUMN_EDGE_CALL_VA,
    COLUMN_OFFSET_RANDOM_VA, MAP_NUBIFY_FOREST_CALLER_RESUME_VA, MAP_NUBIFY_FOREST_CALL_VA,
    MAP_POST_NUBIFY_CHECKSUM_CALL_VA, MAP_POST_NUBIFY_CHECKSUM_SOURCE_TOKEN,
    ROW_DIRECTION_RANDOM_VA, ROW_EDGE_CALL_VA, ROW_OFFSET_RANDOM_VA,
    TERRAIN_GROUPS_CHANGE_COAST_BASE_VA, TERRAIN_GROUPS_CHANGE_FOREST_BASE_VA,
    TERRAIN_GROUPS_CHANGE_MOUNTAIN_BASE_VA, TERRAIN_GROUPS_FIX_TRANSITIONS_VA,
    TERRAIN_GROUPS_NUBIFY_FOREST_END_VA, TERRAIN_GROUPS_NUBIFY_FOREST_RET_VA,
    WORLD_IS_EDGE_OF_REGION_VA,
};
use std::collections::VecDeque;

const EDGE: i32 = 8;
const SEED: i32 = 1;

/// Exact rollback projection for every byte nubify can mutate, the unwalked gate mask, and all
/// dimensions which define WData/TData indexing. The full section checksum additionally pins the
/// rest of walked World state without requiring `World: PartialEq`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct WorldRollbackSnapshot {
    xs: i32,
    ys: i32,
    size: i32,
    fog_xs: i32,
    fog_ys: i32,
    fog_size: i32,
    tile_xs: i32,
    tile_ys: i32,
    tile_size: i32,
    reg_xs: i32,
    reg_ys: i32,
    reg_size: i32,
    start_city_locs: Vec<u8>,
    wdata: Vec<WData>,
    tdata: Vec<u16>,
    checksum: WorldChecksum,
}

fn rollback_snapshot(world: &World) -> WorldRollbackSnapshot {
    WorldRollbackSnapshot {
        xs: world.xs,
        ys: world.ys,
        size: world.size,
        fog_xs: world.fog_xs,
        fog_ys: world.fog_ys,
        fog_size: world.fog_size,
        tile_xs: world.tile_xs,
        tile_ys: world.tile_ys,
        tile_size: world.tile_size,
        reg_xs: world.reg_xs,
        reg_ys: world.reg_ys,
        reg_size: world.reg_size,
        start_city_locs: world.start_city_locs.clone(),
        wdata: world.wdata.clone(),
        tdata: world.tdata.clone(),
        checksum: world.checksum_sections(),
    }
}

#[derive(Default)]
struct ScriptedEdgeHost {
    results: VecDeque<i32>,
    requests: Vec<EdgeOfRegionRequest>,
    corrupt_ordinal: Option<usize>,
    invalid_provenance_ordinal: Option<usize>,
}

impl ScriptedEdgeHost {
    fn with_results(results: impl IntoIterator<Item = i32>) -> Self {
        Self {
            results: results.into_iter().collect(),
            requests: Vec::new(),
            corrupt_ordinal: None,
            invalid_provenance_ordinal: None,
        }
    }
}

impl EdgeOfRegionHost for ScriptedEdgeHost {
    fn is_edge_of_region(
        &mut self,
        world: &World,
        request: &EdgeOfRegionRequest,
    ) -> Option<EdgeOfRegionReceipt> {
        assert_eq!(request.world_checksum, world.checksum_sections());
        let result = self
            .results
            .pop_front()
            .expect("unexpected is_edge_of_region request");
        self.requests.push(request.clone());

        let mut echoed = request.clone();
        if self.corrupt_ordinal == Some(request.ordinal) {
            echoed.candidate_x = echoed.candidate_x.wrapping_add(1);
        }
        let fixture = if self.invalid_provenance_ordinal == Some(request.ordinal) {
            String::new()
        } else {
            "mutation-sensitive-staged-world".to_owned()
        };
        Some(EdgeOfRegionReceipt {
            request: echoed,
            result,
            provenance: EdgeOfRegionProvenance::SyntheticFixture { fixture },
        })
    }
}

fn map() -> InitialWorld {
    map_with_shape(EDGE, EDGE)
}

fn map_with_shape(xs: i32, ys: i32) -> InitialWorld {
    let mut world = World::init_default_rules(xs, ys);
    for cell in &mut world.wdata {
        cell.land = land::FERTILE;
    }
    let checksum = world.checksum_sections();
    InitialWorld {
        world,
        generation_regions: Regions::default(),
        checksum,
        ownership: None,
        sourced_walked_bytes: 52,
    }
}

fn refresh_checksum(map: &mut InitialWorld) {
    map.checksum = map.world.checksum_sections();
}

fn forest_rectangle(map: &mut InitialWorld) {
    for x in 3..=4 {
        for y in 1..=4 {
            map.world.wdata_mut(x, y).flags = wflag::FOREST;
        }
    }
}

fn set_start_city_bit(world: &mut World, x: i32, y: i32) {
    let index = y * world.xs + x;
    world.start_city_locs[(index >> 3) as usize] |= 1u8 << ((index & 7) as u32);
}

fn previous(map: &InitialWorld) -> CheckPlayerForestReceipt {
    CheckPlayerForestReceipt {
        entry_va: MAP_CHECK_PLAYER_FOREST_VA,
        return_va: MAP_CHECK_PLAYER_FOREST_RET_VA,
        caller_resume_va: MAP_CHECK_PLAYER_FOREST_CALLER_RESUME_VA,
        next_va: TERRAIN_GROUPS_NUBIFY_FOREST_VA,
        city_center_radius: 20,
        circle_ring: 4,
        existing_forest_scan: (1, 69),
        candidate_anchor_scan: (21, 69),
        starts: Vec::new(),
        cells_changed: 0,
        random_state_before: SEED,
        random_state_after: SEED,
        random_draws: 0,
        temporary_array_capacity: 0,
        checksum_before: map.checksum.clone(),
        checksum_after: map.checksum.clone(),
        sourced_walked_bytes: map.sourced_walked_bytes,
    }
}

#[test]
fn column_offsets_precede_row_ties_and_receipts_bind_the_staged_world() {
    let mut map = map();
    forest_rectangle(&mut map);

    // A selected side cell is already FOREST. Deliberately combine conflicting class bits
    // and a coastal WATERHALF value so the native normalisation is byte-visible.
    let sentinel = map.world.wdata_mut(4, 1);
    sentinel.flags = wflag::FOREST | wflag::ROCKS | wflag::COAST | wflag::WATERHALF;
    sentinel.land = land::COASTAL;
    sentinel.land_sub = 0x7b;
    refresh_checksum(&mut map);
    let prior = previous(&map);
    let mut host = ScriptedEdgeHost::with_results([0, 1]);

    let receipt = execute_nubify_forest_frontier(&mut map, &prior, &mut host).unwrap();

    assert_eq!(receipt.call_va, MAP_NUBIFY_FOREST_CALL_VA);
    assert_eq!(receipt.entry_va, TERRAIN_GROUPS_NUBIFY_FOREST_VA);
    assert_eq!(receipt.return_va, TERRAIN_GROUPS_NUBIFY_FOREST_RET_VA);
    assert_eq!(receipt.caller_resume_va, MAP_NUBIFY_FOREST_CALLER_RESUME_VA);
    assert_eq!(TERRAIN_GROUPS_NUBIFY_FOREST_END_VA, 0x006a_9a51);
    assert!(!receipt.stack_argument_read);
    assert_eq!(receipt.random_state_before, SEED);
    assert_eq!(receipt.random_state_after, 0xb473_3ac5_u32 as i32);
    assert_eq!(receipt.sourced_walked_bytes, 52);

    let schedule: Vec<_> = receipt
        .random_draws
        .iter()
        .map(|draw| {
            (
                draw.call_va,
                draw.pass,
                draw.use_,
                draw.scan_x,
                draw.scan_y,
                draw.state_before,
                draw.raw,
                draw.derived,
                draw.state_after,
            )
        })
        .collect();
    assert_eq!(
        schedule,
        [
            (
                COLUMN_OFFSET_RANDOM_VA,
                NubifyPass::Column,
                NubifyRandomUse::OffsetWithinFourCellRun,
                3,
                4,
                SEED,
                22_891,
                3,
                0x3c88_596c,
            ),
            (
                COLUMN_OFFSET_RANDOM_VA,
                NubifyPass::Column,
                NubifyRandomUse::OffsetWithinFourCellRun,
                4,
                4,
                0x3c88_596c,
                34_266,
                2,
                0x5e88_85db,
            ),
            (
                ROW_DIRECTION_RANDOM_VA,
                NubifyPass::Row,
                NubifyRandomUse::RowDirectionTie,
                3,
                2,
                0x5e88_85db,
                381,
                1,
                0x8116_017e_u32 as i32,
            ),
            (
                ROW_DIRECTION_RANDOM_VA,
                NubifyPass::Row,
                NubifyRandomUse::RowDirectionTie,
                3,
                3,
                0x8116_017e_u32 as i32,
                15_044,
                -1,
                0xb473_3ac5_u32 as i32,
            ),
        ]
    );

    assert_eq!(host.requests.len(), 2);
    assert_eq!(
        host.requests
            .iter()
            .map(|request| {
                (
                    request.call_va,
                    request.callee_va,
                    request.ordinal,
                    request.pass,
                    request.scan_x,
                    request.scan_y,
                    request.candidate_x,
                    request.candidate_y,
                    request.arg_zero,
                    request.arg_region,
                )
            })
            .collect::<Vec<_>>(),
        [
            (
                COLUMN_EDGE_CALL_VA,
                WORLD_IS_EDGE_OF_REGION_VA,
                0,
                NubifyPass::Column,
                3,
                4,
                4,
                1,
                0,
                0xffff,
            ),
            (
                COLUMN_EDGE_CALL_VA,
                WORLD_IS_EDGE_OF_REGION_VA,
                1,
                NubifyPass::Column,
                4,
                4,
                3,
                2,
                0,
                0xffff,
            ),
        ]
    );
    assert_ne!(
        host.requests[0].world_checksum, host.requests[1].world_checksum,
        "the second opaque-callee receipt must name the post-write staged World"
    );

    assert_eq!(receipt.edge_receipts.len(), 2);
    assert_eq!(receipt.writes.len(), 1);
    let write = &receipt.writes[0];
    assert_eq!((write.candidate_x, write.candidate_y), (4, 1));
    assert_eq!(
        write.old_flags,
        wflag::FOREST | wflag::ROCKS | wflag::COAST | wflag::WATERHALF
    );
    assert_eq!(write.old_land, land::COASTAL);
    assert_eq!(write.old_land_sub, 0x7b);
    assert_eq!(write.new_flags, wflag::FOREST | wflag::WATERHALF);
    assert_eq!(write.new_land, land::OCEAN);
    assert_eq!(write.new_land_sub, 0x7b);
    assert_eq!(receipt.cells_changed, 1);
    assert_eq!(receipt.temporary_array_capacity, 4);
    assert_eq!(
        receipt
            .checksum_before
            .differing_sections(&receipt.checksum_after),
        [WorldSection::WData]
    );
    assert_eq!(
        map.world.wdata(4, 1).flags,
        wflag::FOREST | wflag::WATERHALF
    );
}

#[test]
fn every_run_offset_is_consumed_before_start_city_and_terrain_filters() {
    let mut map = map();
    forest_rectangle(&mut map);
    set_start_city_bit(&mut map.world, 4, 1);
    map.world.wdata_mut(3, 2).land = land::OCEAN;
    refresh_checksum(&mut map);
    let world_before = rollback_snapshot(&map.world);
    let checksum_before = map.checksum.clone();
    let prior = previous(&map);
    let mut host = ScriptedEdgeHost::default();

    let receipt = execute_nubify_forest_frontier(&mut map, &prior, &mut host).unwrap();

    assert_eq!(receipt.random_draws.len(), 4);
    assert_eq!(receipt.random_draws[0].call_va, COLUMN_OFFSET_RANDOM_VA);
    assert_eq!(receipt.random_draws[1].call_va, COLUMN_OFFSET_RANDOM_VA);
    assert_eq!(receipt.random_state_after, 0xb473_3ac5_u32 as i32);
    assert!(host.requests.is_empty());
    assert!(receipt.edge_receipts.is_empty());
    assert!(receipt.writes.is_empty());
    assert_eq!(receipt.cells_changed, 0);
    assert_eq!(receipt.temporary_array_capacity, 0);
    assert_eq!(receipt.checksum_before, receipt.checksum_after);
    assert_eq!(rollback_snapshot(&map.world), world_before);
    assert_eq!(map.checksum, checksum_before);
}

#[test]
fn row_tie_precedes_row_offset_and_negative_land_is_not_stored() {
    let mut map = map_with_shape(8, 3);
    for y in 0..=2 {
        for x in 1..=4 {
            map.world.wdata_mut(x, y).flags = wflag::FOREST;
        }
    }
    let sentinel = map.world.wdata_mut(2, 2);
    sentinel.flags = wflag::FOREST | wflag::ROCKS | wflag::ORIG_COAST;
    sentinel.land = -1;
    sentinel.land_sub = 0x5a;
    refresh_checksum(&mut map);
    let prior = previous(&map);
    let mut host = ScriptedEdgeHost::with_results([0]);

    let receipt = execute_nubify_forest_frontier(&mut map, &prior, &mut host).unwrap();

    assert_eq!(receipt.random_draws.len(), 2);
    assert_eq!(
        receipt
            .random_draws
            .iter()
            .map(|draw| {
                (
                    draw.call_va,
                    draw.use_,
                    draw.scan_x,
                    draw.scan_y,
                    draw.raw,
                    draw.derived,
                )
            })
            .collect::<Vec<_>>(),
        [
            (
                ROW_DIRECTION_RANDOM_VA,
                NubifyRandomUse::RowDirectionTie,
                1,
                1,
                22_891,
                1,
            ),
            (
                ROW_OFFSET_RANDOM_VA,
                NubifyRandomUse::OffsetWithinFourCellRun,
                4,
                1,
                34_266,
                2,
            ),
        ]
    );
    assert_eq!(receipt.random_state_after, 0x5e88_85db);
    assert_eq!(host.requests.len(), 1);
    assert_eq!(host.requests[0].call_va, ROW_EDGE_CALL_VA);
    assert_eq!(host.requests[0].pass, NubifyPass::Row);
    assert_eq!((host.requests[0].scan_x, host.requests[0].scan_y), (4, 1));
    assert_eq!(
        (host.requests[0].candidate_x, host.requests[0].candidate_y),
        (2, 2)
    );
    assert_eq!(receipt.writes.len(), 1);
    assert_eq!(receipt.writes[0].old_land, -1);
    assert_eq!(receipt.writes[0].new_land, -1);
    assert_eq!(receipt.writes[0].old_land_sub, 0x5a);
    assert_eq!(receipt.writes[0].new_land_sub, 0x5a);
    assert_eq!(
        receipt.writes[0].new_flags,
        wflag::FOREST | wflag::ORIG_COAST
    );
    assert_eq!(map.world.wdata(2, 2).land, -1);
    assert_eq!(map.world.wdata(2, 2).land_sub, 0x5a);
}

#[test]
fn mismatched_second_edge_receipt_rolls_back_an_earlier_staged_write() {
    let mut map = map();
    forest_rectangle(&mut map);
    map.world.wdata_mut(4, 1).flags = wflag::FOREST | wflag::ROCKS;
    refresh_checksum(&mut map);
    let world_before = rollback_snapshot(&map.world);
    let regions_before = map.generation_regions.clone();
    let checksum_before = map.checksum.clone();
    let sourced_before = map.sourced_walked_bytes;
    let prior = previous(&map);
    let mut host = ScriptedEdgeHost::with_results([0, 0]);
    host.corrupt_ordinal = Some(1);

    let error = execute_nubify_forest_frontier(&mut map, &prior, &mut host).unwrap_err();

    assert!(matches!(
        error,
        NubifyForestError::EdgeReceiptMismatch { .. }
    ));
    assert_eq!(host.requests.len(), 2);
    assert_eq!(rollback_snapshot(&map.world), world_before);
    assert_eq!(map.generation_regions, regions_before);
    assert_eq!(map.checksum, checksum_before);
    assert_eq!(map.sourced_walked_bytes, sourced_before);
}

#[test]
fn invalid_second_provenance_rolls_back_authoritative_map_and_rng_handoff() {
    let mut map = map();
    forest_rectangle(&mut map);
    map.world.wdata_mut(4, 1).flags = wflag::FOREST | wflag::ROCKS;
    refresh_checksum(&mut map);
    let world_before = rollback_snapshot(&map.world);
    let checksum_before = map.checksum.clone();
    let prior = previous(&map);
    let mut host = ScriptedEdgeHost::with_results([0, 0]);
    host.invalid_provenance_ordinal = Some(1);

    let error = execute_nubify_forest_frontier(&mut map, &prior, &mut host).unwrap_err();

    assert!(matches!(
        error,
        NubifyForestError::EdgeReceiptInvalidProvenance { .. }
    ));
    assert_eq!(host.requests.len(), 2);
    assert_eq!(rollback_snapshot(&map.world), world_before);
    assert_eq!(map.checksum, checksum_before);
    assert_eq!(prior.random_state_after, SEED);
}

#[test]
fn stale_check_player_forest_handoff_fails_before_rng_world_or_host_work() {
    let mut map = map();
    forest_rectangle(&mut map);
    refresh_checksum(&mut map);
    let world_before = rollback_snapshot(&map.world);
    let checksum_before = map.checksum.clone();
    let mut prior = previous(&map);
    prior.next_va = 0;
    let mut host = ScriptedEdgeHost::with_results([0, 1]);

    let error = execute_nubify_forest_frontier(&mut map, &prior, &mut host).unwrap_err();

    assert_eq!(error, NubifyForestError::CheckPlayerForestReceiptMismatch);
    assert!(host.requests.is_empty());
    assert_eq!(rollback_snapshot(&map.world), world_before);
    assert_eq!(map.checksum, checksum_before);
}

#[test]
fn shared_schedule_owner_has_an_exact_replacement_for_terrain_repairs() {
    assert_eq!(MAP_CHECK_PLAYER_FOREST_CALL_VA, 0x0068_c04d);
    assert_eq!(MAP_NUBIFY_FOREST_CALL_VA, 0x0068_c08b);
    assert_eq!(MAP_NUBIFY_FOREST_CALLER_RESUME_VA, 0x0068_c090);
    assert_eq!(MAP_POST_NUBIFY_CHECKSUM_CALL_VA, 0x0068_c0b4);
    assert_eq!(MAP_POST_NUBIFY_CHECKSUM_SOURCE_TOKEN, 0x1eb9);
    assert_eq!(TERRAIN_GROUPS_CHANGE_FOREST_BASE_VA, 0x006a_9a60);
    assert_eq!(TERRAIN_GROUPS_CHANGE_MOUNTAIN_BASE_VA, 0x006a_9c10);
    assert_eq!(TERRAIN_GROUPS_CHANGE_COAST_BASE_VA, 0x006a_9dc0);
    assert_eq!(TERRAIN_GROUPS_FIX_TRANSITIONS_VA, 0x006a_9f70);
}

#[test]
fn a_canonical_forest_write_is_receipted_even_when_it_is_byte_idempotent() {
    let mut map = map();
    forest_rectangle(&mut map);
    refresh_checksum(&mut map);
    let checksum_before = map.checksum.clone();
    let prior = previous(&map);
    let mut host = ScriptedEdgeHost::with_results([0, 1]);

    let receipt = execute_nubify_forest_frontier(&mut map, &prior, &mut host).unwrap();

    assert_eq!(receipt.edge_receipts.len(), 2);
    assert_eq!(receipt.writes.len(), 1);
    assert_eq!(receipt.cells_changed, 0);
    assert_eq!(receipt.temporary_array_capacity, 4);
    assert_eq!(receipt.checksum_before, checksum_before);
    assert_eq!(receipt.checksum_after, checksum_before);
    assert_eq!(map.checksum, checksum_before);
}
