// SPDX-License-Identifier: GPL-3.0-or-later
//! Mutation-sensitive source-only proof for the post-`nubify_forest` transition tail.

mod check_player_forest {
    pub use don_replay::check_player_forest::*;
}

mod initial {
    pub use don_replay::initial::*;
}

mod rules_channel {
    pub use don_replay::rules_channel::*;
}

#[path = "../src/nubify_forest_frontier.rs"]
mod nubify_forest_frontier;

#[path = "../src/post_nubify_transition_frontier.rs"]
mod post_nubify_transition_frontier;

use don_replay::check_player_forest::TERRAIN_GROUPS_NUBIFY_FOREST_VA;
use don_replay::initial::InitialWorld;
use don_replay::replay::Replay;
use don_replay::rules_channel::RETAIL_AFTER_CONSTANTS;
use don_sim::systems::map_terrain::{
    land, tflag, wflag, WData, World, WorldChecksum, WorldSection,
};
use don_sim::systems::regions::Regions;
use nubify_forest_frontier::{
    NubifyForestReceipt, MAP_NUBIFY_FOREST_CALLER_RESUME_VA, MAP_NUBIFY_FOREST_CALL_VA,
    TERRAIN_GROUPS_NUBIFY_FOREST_RET_VA,
};
use post_nubify_transition_frontier::{
    execute_post_nubify_transitions, PostNubifyTransitionError, TerrainTransitionEvidence,
    TerrainTransitionLiveFacts, TerrainTransitionStage, TerrainTransitionTuning,
    COAST_CHANCE_RANDOM_CALL_VA, FIX_NUBIFY_ONE_CALL_VA, FIX_NUBIFY_THREE_CALL_VA,
    FIX_NUBIFY_TWO_CALL_VA, FOREST_CHANCE_RANDOM_CALL_VA, MAP_CHANGE_COAST_BASE_CALL_VA,
    MAP_CHANGE_FOREST_BASE_CALL_VA, MAP_CHANGE_MOUNTAIN_BASE_CALL_VA, MAP_FIX_TRANSITIONS_CALL_VA,
    MAP_POST_NUBIFY_CHECKPOINT_CALL_VA, MAP_POST_NUBIFY_CHECKPOINT_END_VA,
    MAP_POST_NUBIFY_CHECKPOINT_RESUME_VA, MAP_POST_NUBIFY_SOURCE_TOKEN,
    MAP_POST_TRANSITIONS_CHECKPOINT_CALL_VA, MAP_POST_TRANSITIONS_SOURCE_TOKEN,
    MOUNTAIN_CHANCE_RANDOM_CALL_VA, NUBIFY_COLUMN_RANDOM_CALL_VA, SHIPPED_EXE_SHA256,
    TERRAIN_GROUPS_CHANGE_COAST_BASE_VA, TERRAIN_GROUPS_CHANGE_FOREST_BASE_VA,
    TERRAIN_GROUPS_CHANGE_MOUNTAIN_BASE_VA, TERRAIN_GROUPS_FIX_TRANSITIONS_VA,
    TERRAIN_GROUPS_NUBIFY_TRANSITIONS_VA, TILESET_DATA_POINTER_SLOT_VA,
};
use std::path::Path;

const SEED: i32 = 1;

/// Exact rollback projection for every byte this transaction can mutate, its mountain-classifier
/// input, the unwalked gate mask, and every dimension which defines WData/TData indexing. The
/// complete section checksum pins the remaining walked World state without `World: PartialEq`.
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

fn map(edge: i32) -> InitialWorld {
    let mut world = World::init_default_rules(edge, edge);
    for cell in &mut world.wdata {
        cell.flags = 0;
        cell.land = land::OCEAN;
        cell.land_sub = 0;
    }
    let checksum = world.checksum_sections();
    InitialWorld {
        world,
        generation_regions: Regions::default(),
        checksum,
        sourced_walked_bytes: 52,
    }
}

fn refresh_checksum(map: &mut InitialWorld) {
    map.checksum = map.world.checksum_sections();
}

fn previous(map: &InitialWorld, seed: i32) -> NubifyForestReceipt {
    NubifyForestReceipt {
        call_va: MAP_NUBIFY_FOREST_CALL_VA,
        entry_va: TERRAIN_GROUPS_NUBIFY_FOREST_VA,
        return_va: TERRAIN_GROUPS_NUBIFY_FOREST_RET_VA,
        caller_resume_va: MAP_NUBIFY_FOREST_CALLER_RESUME_VA,
        stack_argument_read: false,
        random_state_before: seed,
        random_state_after: seed,
        random_draws: Vec::new(),
        edge_receipts: Vec::new(),
        writes: Vec::new(),
        cells_changed: 0,
        temporary_array_capacity: 0,
        checksum_before: map.checksum.clone(),
        checksum_after: map.checksum.clone(),
        sourced_walked_bytes: map.sourced_walked_bytes,
    }
}

fn synthetic_facts(
    map: &InitialWorld,
    seed: i32,
    tuning: TerrainTransitionTuning,
) -> TerrainTransitionLiveFacts {
    TerrainTransitionLiveFacts {
        tuning,
        evidence: TerrainTransitionEvidence::SyntheticFixture {
            fixture: "post-nubify-mutation-sensitive".to_owned(),
            world_checksum: map.checksum.clone(),
            random_state: seed,
        },
    }
}

fn disabled_tuning() -> TerrainTransitionTuning {
    TerrainTransitionTuning {
        forest_base: -1,
        forest_chance: 0,
        mountain_base: -1,
        mountain_chance: 0,
        coast_base: -1,
        coast_chance: 0,
    }
}

fn make_world_cell_mountain(world: &mut World, x: i32, y: i32) {
    for ordinal in 0..16 {
        let tx = x * 4 + ordinal % 4;
        let ty = y * 4 + ordinal / 4;
        let index = (ty * world.tile_xs + tx) as usize;
        world.tdata[index] = tflag::BLOCKER_MOUNTAIN;
    }
}

fn base_fixture() -> InitialWorld {
    let mut map = map(8);

    let forest = map.world.wdata_mut(0, 0);
    forest.flags = wflag::FOREST;
    forest.land = -1;
    forest.land_sub = 0xaa;
    let forest_neighbor = map.world.wdata_mut(1, 0);
    forest_neighbor.land = land::FERTILE;
    forest_neighbor.land_sub = 0x11;

    make_world_cell_mountain(&mut map.world, 4, 4);
    let mountain = map.world.wdata_mut(4, 4);
    mountain.land = -1;
    mountain.land_sub = 0xbb;
    let mountain_neighbor = map.world.wdata_mut(5, 4);
    mountain_neighbor.land = land::FERTILE;
    mountain_neighbor.land_sub = 0x22;

    let coast = map.world.wdata_mut(7, 7);
    coast.flags = wflag::COAST;
    coast.land = land::COASTAL;
    coast.land_sub = 0xcc;
    let coast_neighbor = map.world.wdata_mut(6, 7);
    coast_neighbor.land = land::FERTILE;
    coast_neighbor.land_sub = 0x33;

    refresh_checksum(&mut map);
    map
}

fn enabled_tuning() -> TerrainTransitionTuning {
    TerrainTransitionTuning {
        // Native stores narrow to one byte.  This deliberately proves truncation.
        forest_base: 0x107,
        forest_chance: 100,
        mountain_base: 8,
        mountain_chance: 100,
        coast_base: 9,
        coast_chance: 100,
    }
}

#[test]
fn caller_order_base_spreads_and_rng_chronology_are_exact() {
    let mut map = base_fixture();
    let prior = previous(&map, SEED);
    let facts = synthetic_facts(&map, SEED, enabled_tuning());

    let receipt = execute_post_nubify_transitions(&mut map, &prior, &facts).unwrap();

    assert_eq!(receipt.caller_resume_va, 0x0068_c090);
    assert_eq!(
        receipt.checkpoint_call_va,
        MAP_POST_NUBIFY_CHECKPOINT_CALL_VA
    );
    assert_eq!(
        receipt.checkpoint_resume_va,
        MAP_POST_NUBIFY_CHECKPOINT_RESUME_VA
    );
    assert_eq!(receipt.checkpoint_end_va, MAP_POST_NUBIFY_CHECKPOINT_END_VA);
    assert_eq!(receipt.source_token, MAP_POST_NUBIFY_SOURCE_TOKEN);
    assert_eq!(
        receipt.next_checkpoint_call_va,
        MAP_POST_TRANSITIONS_CHECKPOINT_CALL_VA
    );
    assert_eq!(receipt.next_source_token, MAP_POST_TRANSITIONS_SOURCE_TOKEN);
    assert_eq!(receipt.random_state_before, SEED);
    assert_eq!(receipt.random_state_after, 0x8116_017e_u32 as i32);
    assert_eq!(receipt.sourced_walked_bytes, 52);

    assert_eq!(
        receipt
            .calls
            .iter()
            .map(|call| (
                call.call_va,
                call.callee_va,
                call.enabled,
                call.implicit_subtype
            ))
            .collect::<Vec<_>>(),
        [
            (
                MAP_CHANGE_FOREST_BASE_CALL_VA,
                TERRAIN_GROUPS_CHANGE_FOREST_BASE_VA,
                true,
                None,
            ),
            (
                MAP_CHANGE_MOUNTAIN_BASE_CALL_VA,
                TERRAIN_GROUPS_CHANGE_MOUNTAIN_BASE_VA,
                true,
                None,
            ),
            (
                MAP_CHANGE_COAST_BASE_CALL_VA,
                TERRAIN_GROUPS_CHANGE_COAST_BASE_VA,
                true,
                None,
            ),
            (
                MAP_FIX_TRANSITIONS_CALL_VA,
                TERRAIN_GROUPS_FIX_TRANSITIONS_VA,
                true,
                None,
            ),
            (
                FIX_NUBIFY_THREE_CALL_VA,
                TERRAIN_GROUPS_NUBIFY_TRANSITIONS_VA,
                true,
                Some(3),
            ),
            (
                FIX_NUBIFY_TWO_CALL_VA,
                TERRAIN_GROUPS_NUBIFY_TRANSITIONS_VA,
                true,
                Some(2),
            ),
            (
                FIX_NUBIFY_ONE_CALL_VA,
                TERRAIN_GROUPS_NUBIFY_TRANSITIONS_VA,
                true,
                Some(1),
            ),
        ]
    );

    assert_eq!(
        receipt
            .random_draws
            .iter()
            .map(|draw| {
                (
                    draw.call_va,
                    draw.stage,
                    draw.scan_x,
                    draw.scan_y,
                    draw.primary_candidate,
                    draw.raw,
                    draw.remainder,
                    draw.state_after,
                )
            })
            .collect::<Vec<_>>(),
        [
            (
                FOREST_CHANCE_RANDOM_CALL_VA,
                TerrainTransitionStage::ChangeForestBase,
                0,
                0,
                (1, 0),
                22_891,
                91,
                0x3c88_596c,
            ),
            (
                MOUNTAIN_CHANCE_RANDOM_CALL_VA,
                TerrainTransitionStage::ChangeMountainBase,
                4,
                4,
                (5, 4),
                34_266,
                66,
                0x5e88_85db,
            ),
            (
                COAST_CHANCE_RANDOM_CALL_VA,
                TerrainTransitionStage::ChangeCoastBase,
                7,
                7,
                (6, 7),
                381,
                81,
                0x8116_017e_u32 as i32,
            ),
        ]
    );
    assert!(receipt
        .random_draws
        .iter()
        .all(|draw| draw.accepted_candidate == Some(draw.primary_candidate)));

    assert_eq!(
        (map.world.wdata(0, 0).land, map.world.wdata(0, 0).land_sub),
        (0, 7)
    );
    assert_eq!(
        (map.world.wdata(1, 0).land, map.world.wdata(1, 0).land_sub),
        (0, 7)
    );
    assert_eq!(
        (map.world.wdata(4, 4).land, map.world.wdata(4, 4).land_sub),
        (0, 8)
    );
    assert_eq!(
        (map.world.wdata(5, 4).land, map.world.wdata(5, 4).land_sub),
        (0, 8)
    );
    assert_eq!(
        (map.world.wdata(7, 7).land, map.world.wdata(7, 7).land_sub),
        (0, 9)
    );
    assert_eq!(
        (map.world.wdata(6, 7).land, map.world.wdata(6, 7).land_sub),
        (0, 9)
    );
    assert_eq!(receipt.writes.len(), 6);
    assert_eq!(receipt.byte_changing_writes, 6);
    assert_eq!(
        receipt
            .checksum_before
            .differing_sections(&receipt.checksum_after),
        [WorldSection::WData]
    );
}

#[test]
fn chance_mutation_keeps_draws_but_rejects_neighbors_and_negative_base_skips_a_stage() {
    let mut rejected = base_fixture();
    let prior = previous(&rejected, SEED);
    let mut tuning = enabled_tuning();
    tuning.forest_chance = 0;
    tuning.mountain_chance = 0;
    tuning.coast_chance = 0;
    let facts = synthetic_facts(&rejected, SEED, tuning);
    let receipt = execute_post_nubify_transitions(&mut rejected, &prior, &facts).unwrap();

    assert_eq!(receipt.random_draws.len(), 3);
    assert!(receipt
        .random_draws
        .iter()
        .all(|draw| draw.accepted_candidate.is_none()));
    assert_eq!(receipt.random_state_after, 0x8116_017e_u32 as i32);
    assert_eq!(rejected.world.wdata(1, 0).land_sub, 0x11);
    assert_eq!(rejected.world.wdata(5, 4).land_sub, 0x22);
    assert_eq!(rejected.world.wdata(6, 7).land_sub, 0x33);

    let mut skipped = base_fixture();
    let prior = previous(&skipped, SEED);
    let mut tuning = enabled_tuning();
    tuning.forest_base = -1;
    let facts = synthetic_facts(&skipped, SEED, tuning);
    let receipt = execute_post_nubify_transitions(&mut skipped, &prior, &facts).unwrap();

    assert!(!receipt.calls[0].enabled);
    assert_eq!(receipt.random_draws.len(), 2);
    assert_eq!(
        receipt.random_draws[0].call_va,
        MOUNTAIN_CHANCE_RANDOM_CALL_VA
    );
    assert_eq!(receipt.random_draws[0].raw, 22_891);
    assert_eq!(receipt.random_draws[1].call_va, COAST_CHANCE_RANDOM_CALL_VA);
    assert_eq!(receipt.random_draws[1].raw, 34_266);
    assert_eq!(receipt.random_state_after, 0x5e88_85db);
    assert_eq!(skipped.world.wdata(0, 0).land, -1);
    assert_eq!(skipped.world.wdata(1, 0).land_sub, 0x11);
}

#[test]
fn five_cell_column_threshold_draws_before_both_candidate_gates() {
    let mut subject = map(6);
    for y in 0..5 {
        let cell = subject.world.wdata_mut(2, y);
        cell.land = land::FERTILE;
        cell.land_sub = 3;
    }
    refresh_checksum(&mut subject);
    let world_before = rollback_snapshot(&subject.world);
    let prior = previous(&subject, SEED);
    let facts = synthetic_facts(&subject, SEED, disabled_tuning());

    let receipt = execute_post_nubify_transitions(&mut subject, &prior, &facts).unwrap();

    assert_eq!(receipt.random_draws.len(), 1);
    let draw = &receipt.random_draws[0];
    assert_eq!(draw.call_va, NUBIFY_COLUMN_RANDOM_CALL_VA);
    assert_eq!(draw.stage, TerrainTransitionStage::NubifyThreeColumn);
    assert_eq!((draw.scan_x, draw.scan_y), (2, 4));
    assert_eq!(draw.raw, 22_891);
    assert_eq!(draw.remainder, 1);
    assert_eq!(draw.primary_candidate, (1, 2));
    assert_eq!(draw.fallback_candidate, Some((3, 2)));
    assert_eq!(draw.accepted_candidate, None);
    assert_eq!(receipt.random_state_after, 0x3c88_596c);
    assert!(receipt.writes.is_empty());
    assert_eq!(receipt.checksum_before, receipt.checksum_after);
    assert_eq!(rollback_snapshot(&subject.world), world_before);

    let mut broken = map(6);
    for y in [0, 1, 3, 4] {
        let cell = broken.world.wdata_mut(2, y);
        cell.land = land::FERTILE;
        cell.land_sub = 3;
    }
    refresh_checksum(&mut broken);
    let prior = previous(&broken, SEED);
    let facts = synthetic_facts(&broken, SEED, disabled_tuning());
    let receipt = execute_post_nubify_transitions(&mut broken, &prior, &facts).unwrap();
    assert!(receipt.random_draws.is_empty());
    assert_eq!(receipt.random_state_after, SEED);
}

#[test]
fn eight_neighbor_spread_writes_the_native_land_sub_word_only() {
    let mut map = map(3);
    let source = map.world.wdata_mut(1, 1);
    source.land = land::FERTILE;
    source.land_sub = 3;
    let target = map.world.wdata_mut(0, 0);
    target.land = land::FERTILE;
    target.land_sub = 9;
    target.flags = wflag::HAS_ROAD | wflag::ORIG_COAST;
    target.region = 77;
    refresh_checksum(&mut map);
    let prior = previous(&map, SEED);
    let facts = synthetic_facts(&map, SEED, disabled_tuning());

    let receipt = execute_post_nubify_transitions(&mut map, &prior, &facts).unwrap();

    assert!(receipt.random_draws.is_empty());
    let writes: Vec<_> = receipt
        .writes
        .iter()
        .filter(|write| write.stage == TerrainTransitionStage::SpreadThreeToTwo)
        .collect();
    assert_eq!(writes.len(), 1);
    assert_eq!((writes[0].x, writes[0].y), (0, 0));
    assert_eq!((writes[0].old_land, writes[0].old_land_sub), (0, 9));
    assert_eq!((writes[0].new_land, writes[0].new_land_sub), (0, 2));
    let target = map.world.wdata(0, 0);
    assert_eq!(target.flags, wflag::HAS_ROAD | wflag::ORIG_COAST);
    assert_eq!(target.region, 77);
    assert_eq!((target.land, target.land_sub), (0, 2));
    assert_eq!(
        receipt
            .checksum_before
            .differing_sections(&receipt.checksum_after),
        [WorldSection::WData]
    );
}

#[test]
fn stale_live_capture_fails_before_world_or_rng_commit() {
    let mut map = base_fixture();
    let prior = previous(&map, SEED);
    let world_before = rollback_snapshot(&map.world);
    let checksum_before = map.checksum.clone();
    let mut facts = synthetic_facts(&map, SEED, enabled_tuning());
    let TerrainTransitionEvidence::SyntheticFixture { world_checksum, .. } = &mut facts.evidence
    else {
        unreachable!()
    };
    world_checksum.per_section[0].adler = world_checksum.per_section[0].adler.wrapping_add(1);

    let error = execute_post_nubify_transitions(&mut map, &prior, &facts).unwrap_err();

    assert_eq!(
        error,
        PostNubifyTransitionError::LiveFactsUnavailableOrMismatched
    );
    assert_eq!(rollback_snapshot(&map.world), world_before);
    assert_eq!(map.checksum, checksum_before);
    assert_eq!(prior.random_state_after, SEED);
}

#[test]
fn typed_retail_capture_admission_binds_a_real_replay_rules_checkpoint() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("ron-data/replays/multi/Playback___2025.02.10_21_26_50__Mon_.rcx");
    if !path.is_file() {
        eprintln!(
            "\n  SKIPPED — NOT A PASS. The supported retail replay is absent; post-nubify \
             live-fact admission was not bound to a corpus Rules checkpoint.\n"
        );
        return;
    }
    let replay = Replay::open(&path).expect("supported retail replay must decode");
    let reconstruction = replay.initial.reconstruct_items();
    let rules = reconstruction
        .rules
        .expect("supported replay must admit shipped Rules");
    assert_eq!(rules.after_constants, RETAIL_AFTER_CONSTANTS);

    // The corpus does not contain the six selected-tileset words.  This object models the
    // independently captured half of the join and is bound to the exact staged checksum/RNG.
    // The adapter validates caller-carried evidence shape; it does not authenticate the digest.
    let mut map = map(4);
    refresh_checksum(&mut map);
    let prior = previous(&map, SEED);
    let facts = TerrainTransitionLiveFacts {
        tuning: disabled_tuning(),
        evidence: TerrainTransitionEvidence::RetailCapture {
            executable_sha256: SHIPPED_EXE_SHA256.to_owned(),
            capture_sha256: [0xa5; 32],
            tileset_data_pointer_slot_va: TILESET_DATA_POINTER_SLOT_VA,
            tileset_data_object_va: 0x1234_5000,
            rules_after_constants: rules.after_constants,
            checkpoint_call_va: MAP_POST_NUBIFY_CHECKPOINT_CALL_VA,
            source_token: MAP_POST_NUBIFY_SOURCE_TOKEN,
            world_checksum: map.checksum.clone(),
            random_state: SEED,
        },
    };

    let receipt = execute_post_nubify_transitions(&mut map, &prior, &facts).unwrap();
    assert_eq!(receipt.sourced_walked_bytes, 52);
    assert_eq!(receipt.random_state_before, SEED);
    assert_eq!(receipt.source_token, 0x1eb9);
    assert_eq!(receipt.next_source_token, 0x1ebe);
}
