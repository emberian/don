//! Goods-channel continuation through admitted `Map::place_resources` allocations.

use don_replay::place_resources_bonus_mutation_frontier::{
    AllocationKind, PlacementEvidence, PlacementParameters, PlacementPath, PlacementPattern,
    PlacementReceipt, PlacementRequest, ResourceAllocation, WorldOccupancyWrite,
    BONUS_CATEGORY_TAIL_VA, GOOD_WDATA_DOWN_MARKER, GOOD_WDATA_DOWN_WHO_WRITE_VA,
    GOOD_WDATA_DOWN_WRITE_VA, OBJECTS_INIT_GOOD_VA, REGION_INIT_GOOD_CALL_VA,
};
use don_replay::replay_goods_initial::ReplayInitialGoodsRuntime;
use don_sim::systems::map_terrain::World;
use don_sim::systems::terrain_drop_tile::DropTileExternalRequest;
use don_sim::systems::world_oil_goods::{OilGoodMutation, SUBOBJECT_COORD_XOR};
use don_replay::replay_goods_resource_schedule::*;

fn oil_request(wx: i32, wy: i32, enabled: bool) -> DropTileExternalRequest {
    let request = OilGoodMutation::at(wx, wy, enabled);
    DropTileExternalRequest::OilGoodMutation {
        world_x: request.world_x,
        world_y: request.world_y,
        enabled: request.enabled,
        good_type: request.good_type,
        coord_x: request.coord_x,
        coord_y: request.coord_y,
    }
}

fn catalog(type_index: i32, is_flat: bool) -> ResourceGoodTypeFact {
    ResourceGoodTypeFact {
        type_index,
        is_flat,
        evidence: GoodTypeFlatEvidence::ExactPort {
            implementation_sha256: [type_index as u8 + 1; 32],
            proof_document: format!("test-type-{type_index}"),
        },
    }
}

fn good_placement(type_id: i32, slot: i32, coord_x: i32, coord_y: i32) -> PlacementReceipt {
    let mut world = World::init_default_rules(8, 8);
    let checksum_before = world.checksum_sections();
    let wx = don_sim::systems::map_terrain::div_3(coord_x >> 8);
    let wy = don_sim::systems::map_terrain::div_3(coord_y >> 8);
    let request = PlacementRequest {
        call_va: don_replay::place_resources_bonus_mutation_frontier::REGION_PLACEMENT_CALL_VA,
        callee_va:
            don_replay::place_resources_bonus_mutation_frontier::MAP_PLACE_REGION_RESOURCE_VA,
        path: PlacementPath::Region,
        params: PlacementParameters {
            player_count: 2,
            good_id: type_id,
            selector: 0,
            pattern: PlacementPattern::World,
            saturate: 0,
            num_rare: 1,
            spacing: 0,
            group_spacing: 0,
            player_keep_away: 1,
            player_stay_near: 1,
            center_keep_away: 0,
            center_stay_near: 0,
            corner_keep_away: 0,
            corner_stay_near: 0,
            edge_keep_away: 0,
            edge_stay_near: 0,
        },
        random_state_before: 0x1234,
        world_checksum_before: checksum_before,
        sourced_walked_bytes: 99,
        resource_pool_digest_before: 7,
        resource_pool_before: None,
    };
    world.wdata_mut(wx, wy).down = GOOD_WDATA_DOWN_MARKER;
    world.wdata_mut(wx, wy).down_who = slot as i16;
    let checksum_after = world.checksum_sections();
    PlacementReceipt {
        request,
        random_draws: Vec::new(),
        random_state_after: 0x1234,
        allocations: vec![ResourceAllocation {
            call_va: REGION_INIT_GOOD_CALL_VA,
            callee_va: OBJECTS_INIT_GOOD_VA,
            kind: AllocationKind::Good,
            type_id,
            coord_x,
            coord_y,
            slot,
            occupancy_write: Some(WorldOccupancyWrite {
                world_x: wx,
                world_y: wy,
                down_write_va: GOOD_WDATA_DOWN_WRITE_VA,
                down_who_write_va: GOOD_WDATA_DOWN_WHO_WRITE_VA,
                old_down: -1,
                old_down_who: -1,
                new_down: GOOD_WDATA_DOWN_MARKER,
                new_down_who: slot as i16,
            }),
        }],
        allocated_count: 1,
        world_checksum_after: checksum_after,
        sourced_walked_bytes_after: 99,
        resource_pool_digest_after: 7,
        resource_pool_selections: Vec::new(),
        resource_pool_after: None,
        evidence: PlacementEvidence::ExactPort {
            implementation_sha256: [type_id as u8 + 1; 32],
            proof_document: format!("test-good-{type_id}-slot-{slot}"),
        },
    }
}

fn runtime_with_one_oil_and_one_hole() -> ReplayInitialGoodsRuntime {
    let mut runtime = ReplayInitialGoodsRuntime::cold_process();
    let mut world = World::init_default_rules(8, 8);
    runtime
        .resolve_oil_request(&mut world, oil_request(1, 1, true))
        .unwrap();
    runtime
        .resolve_oil_request(&mut world, oil_request(2, 1, true))
        .unwrap();
    runtime
        .resolve_oil_request(&mut world, oil_request(1, 1, false))
        .unwrap();
    runtime
}

#[test]
fn resource_allocations_reuse_then_append_and_add_exact_walked_rows() {
    let initial = runtime_with_one_oil_and_one_hole();
    assert_eq!(initial.goods().summary().goods_walked_bytes, 22);
    assert_eq!(initial.goods().slots.len(), 2);
    assert!(!initial.goods().slots[0].active());

    let reused = good_placement(6, 0, 0x780, 0x900);
    let appended = good_placement(7, 2, 0xa80, 0xc00);
    let receipt = apply_admitted_resource_placements(
        initial.goods(),
        ResourceScheduleGoodsBoundary::BeforeBonusCategoryCleanup,
        &[&reused, &appended],
        &[catalog(6, true), catalog(7, false)],
    )
    .unwrap();

    assert_eq!(receipt.placements_seen, 2);
    assert_eq!(receipt.allocations_seen, 2);
    assert_eq!(receipt.item_allocations_ignored, 0);
    assert_eq!(receipt.goods_allocations.len(), 2);
    assert_eq!(
        receipt.goods_allocations[0].storage_kind,
        ResourceGoodAllocationKind::ReusedInactive
    );
    assert_eq!(
        receipt.goods_allocations[1].storage_kind,
        ResourceGoodAllocationKind::Appended
    );
    assert_eq!(receipt.newly_owned_walked_bytes, 44);
    assert_eq!(receipt.before.goods_walked_bytes, 22);
    assert_eq!(receipt.after.goods_walked_bytes, 66);
    assert_eq!(receipt.after.active_count, 3);
    assert_eq!(receipt.after.array.length, 3);
    assert_eq!(receipt.after.array.capacity, 4);
    assert_eq!(receipt.after.good_mark, 3);
    assert_eq!(receipt.goods_after.slots[0].node.flags, 0x21);
    assert_eq!(receipt.goods_after.slots[2].node.flags, 0x01);
    assert_eq!(receipt.goods_after.slots[0].node.z, SUBOBJECT_COORD_XOR);
    assert_eq!(
        receipt.goods_after.slots[0].node.x,
        0x780 ^ SUBOBJECT_COORD_XOR
    );
    assert_eq!(
        receipt.goods_after.slots[0].node.y,
        0x900 ^ SUBOBJECT_COORD_XOR
    );
    assert!(!receipt.complete_for_first_checkpoint);
    assert_eq!(
        receipt.first_remaining_schedule_va,
        Some(BONUS_CATEGORY_TAIL_VA)
    );
}

#[test]
fn missing_flat_fact_and_wrong_sparse_slot_refuse_atomically() {
    let initial = runtime_with_one_oil_and_one_hole();
    let before = initial.goods().clone();
    let allocation = good_placement(6, 0, 0x780, 0x900);

    assert_eq!(
        apply_admitted_resource_placements(
            initial.goods(),
            ResourceScheduleGoodsBoundary::AfterFirstBonusRow,
            &[&allocation],
            &[],
        ),
        Err(ResourceScheduleGoodsError::MissingGoodTypeFact { type_index: 6 })
    );
    assert_eq!(initial.goods(), &before);

    let wrong_slot = good_placement(6, 2, 0x780, 0x900);
    assert_eq!(
        apply_admitted_resource_placements(
            initial.goods(),
            ResourceScheduleGoodsBoundary::AfterFirstBonusRow,
            &[&wrong_slot],
            &[catalog(6, true)],
        ),
        Err(ResourceScheduleGoodsError::SlotMismatch {
            expected: 0,
            actual: 2,
        })
    );
    assert_eq!(initial.goods(), &before);
}

#[test]
fn exact_constants_and_pending_checkpoint_are_frozen() {
    assert_eq!(SUBOBJECT_INIT_VA, 0x0066_2300);
    assert_eq!(GOOD_TYPE_IS_FLAT_VA, 0x0047_80c0);
    assert_eq!(GOOD_INIT_VA, 0x0066_da20);
    assert_eq!(SUBOBJECT_FLAT_FLAG, 0x20);
    assert_eq!(
        ResourceScheduleGoodsBoundary::BeforeBonusRows.first_remaining_schedule_va(),
        Some(0x0068_fbb3)
    );
    assert_eq!(
        ResourceScheduleGoodsBoundary::BeforeBonusCategoryCleanup.first_remaining_schedule_va(),
        Some(0x0069_0225)
    );
    assert_eq!(
        ResourceScheduleGoodsBoundary::PlaceResourcesSkipped.first_remaining_schedule_va(),
        None
    );
}
