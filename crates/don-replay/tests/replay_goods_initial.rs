//! Bounded initial-Goods runtime and local-corpus audit.
//!
//! The source module is path-mounted until the replay orchestrator adds its one
//! shared `lib.rs` registration hook.

#[path = "../src/replay_goods_initial.rs"]
mod replay_goods_initial;

use don_replay::checksum::Channel;
use don_replay::replay::{corpus, Replay};
use don_sim::systems::map_terrain::{wflag, World};
use don_sim::systems::terrain_drop_tile::DropTileExternalRequest;
use don_sim::systems::world_oil_goods::{GoodAllocationKind, OilGoodMutation};
use replay_goods_initial::*;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn world() -> World {
    World::init_default_rules(8, 8)
}

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

fn skip(reason: &str) {
    eprintln!("\n  SKIPPED — NOT A PASS. {reason}\n  Nothing corpus-backed was established.\n");
}

#[test]
fn cold_objects_init_has_no_rows_mark_or_holes() {
    let runtime = ReplayInitialGoodsRuntime::cold_process();
    let init = runtime.initialization();
    let prefix = runtime.prefix_snapshot();

    assert_eq!(init.objects_init_va, OBJECTS_INIT_VA);
    assert_eq!(init.objects_clear_va, OBJECTS_CLEAR_VA);
    assert_eq!(init.goods_singleton_va, GOODS_SINGLETON_VA);
    assert_eq!(init.storage_source, InitialGoodsStorageSource::ColdPeImage);
    assert_eq!(init.state.array.length, 0);
    assert_eq!(init.state.array.capacity, 0);
    assert_eq!(init.state.array.increment, -1);
    assert_eq!(init.state.array.flags, 0);
    assert_eq!(init.state.good_mark, 0);
    assert_eq!(init.state.active_count, 0);
    assert_eq!(init.holes_below_mark, 0);
    assert_eq!(init.state.goods_walked_bytes, 0);
    assert_eq!(init.state.goods_checksum, 1);
    assert_eq!(prefix.sourced_walked_bytes, 0);
    assert!(!prefix.complete_for_first_checkpoint);
    assert_eq!(prefix.later_good_owner_va, MAP_PLACE_RESOURCES_VA);
}

#[test]
fn retained_capacity_is_explicit_checksum_neutral_and_starts_without_holes() {
    let cold = ReplayInitialGoodsRuntime::cold_process();
    let mut retained =
        ReplayInitialGoodsRuntime::from_measured_retained_storage(RetainedGoodsStorageFact {
            capacity: 16,
            cur_index: 7,
        })
        .unwrap();

    assert_eq!(
        retained.initialization().storage_source,
        InitialGoodsStorageSource::MeasuredRetainedStorage
    );
    assert_eq!(retained.initialization().state.array.capacity, 16);
    assert_eq!(retained.initialization().state.cur_index, 7);
    assert_eq!(retained.initialization().state.array.length, 0);
    assert_eq!(retained.initialization().state.good_mark, 0);
    assert_eq!(retained.initialization().holes_below_mark, 0);
    assert_eq!(
        retained.prefix_snapshot().state.goods_checksum,
        cold.prefix_snapshot().state.goods_checksum,
        "PtrArray capacity is not visited by CheckSums::check_goods"
    );
    assert_eq!(
        ReplayInitialGoodsRuntime::from_measured_retained_storage(RetainedGoodsStorageFact {
            capacity: -1,
            cur_index: 3,
        }),
        Err(ReplayInitialGoodsError::NegativeRetainedCapacity { capacity: -1 })
    );

    let mut retained_world = world();
    let allocation = retained
        .resolve_oil_request(&mut retained_world, oil_request(4, 4, true))
        .unwrap()
        .execution
        .allocation
        .unwrap();
    assert_eq!(
        (allocation.capacity_before, allocation.capacity_after),
        (16, 16)
    );
    assert_eq!(retained.goods().cur_index, 7);
}

#[test]
fn oil_adapter_retains_canonical_allocation_and_world_receipts() {
    let mut world = world();
    let mut runtime = ReplayInitialGoodsRuntime::cold_process();
    let request = oil_request(3, 5, true);

    let receipt = runtime.resolve_oil_request(&mut world, request).unwrap();

    assert_eq!(receipt.ordinal, 0);
    assert_eq!(receipt.request, request);
    assert_eq!(
        receipt.resolution,
        don_sim::systems::terrain_drop_tile::DropTileExternalResolution::OilGoodsApplied {
            request
        }
    );
    assert_eq!(receipt.execution.rng_draws, 0);
    assert_eq!(receipt.execution.closed_slots, Vec::<usize>::new());
    let allocation = receipt.execution.allocation.unwrap();
    assert_eq!(allocation.kind, GoodAllocationKind::Appended);
    assert_eq!(allocation.slot, 0);
    assert_eq!(
        (allocation.capacity_before, allocation.capacity_after),
        (0, 4)
    );
    assert_eq!((receipt.holes_before, receipt.holes_after), (0, 0));
    assert_ne!(world.wdata(3, 5).flags & wflag::OIL, 0);

    let prefix = runtime.prefix_snapshot();
    assert_eq!(prefix.state.array.length, 1);
    assert_eq!(prefix.state.array.capacity, 4);
    assert_eq!(prefix.state.good_mark, 1);
    assert_eq!(prefix.state.active_count, 1);
    assert_eq!(prefix.state.goods_walked_bytes, 21);
    assert_eq!(prefix.sourced_walked_bytes, 21);
    assert_ne!(prefix.state.goods_checksum, 1);
    assert_eq!(prefix.oil_receipts, 1);
    assert_eq!(runtime.oil_receipts().len(), 1);
    assert_eq!(runtime.oil_receipts()[0], receipt);
    assert!(!prefix.complete_for_first_checkpoint);
}

#[test]
fn close_preserves_mark_creates_a_hole_and_next_enable_reuses_it() {
    let mut world = world();
    let mut runtime = ReplayInitialGoodsRuntime::cold_process();
    runtime
        .resolve_oil_request(&mut world, oil_request(1, 2, true))
        .unwrap();
    runtime
        .resolve_oil_request(&mut world, oil_request(2, 2, true))
        .unwrap();

    let closed = runtime
        .resolve_oil_request(&mut world, oil_request(1, 2, false))
        .unwrap();
    assert_eq!(closed.execution.closed_slots, vec![0]);
    assert_eq!(closed.execution.allocation, None);
    assert_eq!((closed.holes_before, closed.holes_after), (0, 1));
    assert_eq!(runtime.goods().good_mark, 2);
    assert_eq!(runtime.prefix_snapshot().state.active_count, 1);

    let reused = runtime
        .resolve_oil_request(&mut world, oil_request(6, 2, true))
        .unwrap();
    let allocation = reused.execution.allocation.unwrap();
    assert_eq!(allocation.kind, GoodAllocationKind::ReusedInactive);
    assert_eq!(allocation.slot, 0);
    assert_eq!((reused.holes_before, reused.holes_after), (1, 0));
    assert_eq!(runtime.goods().slots.len(), 2);
    assert_eq!(runtime.goods().good_mark, 2);
    assert_eq!(
        runtime
            .goods()
            .scenario_rows()
            .iter()
            .map(|row| (row.slot, row.coord_x))
            .collect::<Vec<_>>(),
        vec![
            (0, OilGoodMutation::at(6, 2, true).coord_x),
            (1, OilGoodMutation::at(2, 2, true).coord_x),
        ]
    );
}

#[test]
fn unsupported_external_is_atomic_and_receipt_free() {
    let mut world = world();
    let mut runtime = ReplayInitialGoodsRuntime::cold_process();
    let request = DropTileExternalRequest::CliffsPositionCliff {
        world_x: 2,
        world_y: 3,
        cliff_type: 1,
        target_x: 4,
        target_y: 5,
        mode: 0,
        facing: 2,
        anchor_y: 3,
    };
    let before_world = world.clone();
    let before_runtime = runtime.clone();

    assert_eq!(
        runtime.resolve_oil_request(&mut world, request),
        Err(ReplayInitialGoodsError::UnsupportedExternalRequest { request })
    );
    assert_eq!(world.checksum(), before_world.checksum());
    assert_eq!(world.wdata, before_world.wdata);
    assert_eq!(world.tdata, before_world.tdata);
    assert_eq!(runtime, before_runtime);
}

/// Corpus boundary only: the recorded Goods value is never an input to the
/// runtime.  This proves why the exact empty/reset state and oil prefix cannot
/// yet be installed as the first-checkpoint producer.
#[test]
fn corpus_first_goods_is_nonempty_while_the_replay_bridge_has_no_producer() {
    let files = corpus(&repo_root());
    if files.is_empty() {
        skip("ron-data/replays contains no .rcx files.");
        return;
    }
    assert_eq!(
        don_replay::check_all::CHANNEL_SOURCE[Channel::Goods as usize],
        don_replay::check_all::ChannelSource::Absent,
        "a bounded oil prefix must not be advertised as the complete Goods producer"
    );

    let mut checksummed = 0usize;
    let mut first_values = BTreeSet::new();
    let mut first_turns = BTreeSet::new();
    for path in &files {
        let Ok(rep) = Replay::open(path) else {
            continue;
        };
        let Some((turn, recorded)) = rep
            .turns
            .iter()
            .find_map(|turn| turn.any_checksums().map(|(_, sums)| (turn.turn, sums)))
        else {
            continue;
        };
        checksummed += 1;
        first_turns.insert(turn);
        let retail_goods = recorded.get(Channel::Goods);
        first_values.insert(retail_goods);
        assert_ne!(
            retail_goods,
            1,
            "{}: retail's first Goods checkpoint is unexpectedly empty",
            path.display()
        );
    }

    assert!(checksummed > 0, "no replay carried CheckSumsCommand 0x39");
    if files.len() == 61 {
        assert_eq!(checksummed, 21, "the known 61-file corpus changed");
        assert_eq!(first_turns, BTreeSet::from([2]));
        assert_eq!(first_values.len(), 21, "first Goods values stopped varying");
    }
    eprintln!(
        "  {checksummed} checksum-bearing recordings, {} distinct first Goods values; \
         all are nonempty and the replay bridge installs none",
        first_values.len()
    );
}
