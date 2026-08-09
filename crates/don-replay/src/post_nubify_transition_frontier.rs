// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact post-`nubify_forest` terrain-base and transition transaction.
//!
//! `Map::make` resumes at `0x0068c090`, emits checkpoint token `0x1eb9`, conditionally
//! calls the three base spreaders, and always calls `TerrainGroups::fix_transitions`.
//! The six selected-tileset tuning words are not replay bytes.  They therefore cross a
//! typed, checkpoint-bound live-fact boundary instead of being guessed from a map name.

use crate::check_player_forest::TERRAIN_GROUPS_NUBIFY_FOREST_VA;
use crate::initial::InitialWorld;
use crate::nubify_forest_frontier::{
    NubifyForestReceipt, MAP_NUBIFY_FOREST_CALLER_RESUME_VA, MAP_NUBIFY_FOREST_CALL_VA,
    TERRAIN_GROUPS_NUBIFY_FOREST_RET_VA,
};
use crate::rules_channel::RETAIL_AFTER_CONSTANTS;
use don_sim::rng::Random;
use don_sim::systems::map_terrain::{
    land, tflag, wflag, World, WorldChecksum, WorldSection, NEIGHBOUR4_DX, NEIGHBOUR4_DY,
    NEIGHBOUR_DX, NEIGHBOUR_DY,
};

pub const SHIPPED_EXE_SHA256: &str =
    "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079";

pub const MAP_POST_NUBIFY_CHECKPOINT_CALL_VA: u32 = 0x0068_c0b4;
pub const MAP_POST_NUBIFY_CHECKPOINT_RESUME_VA: u32 = 0x0068_c0b9;
pub const MAP_POST_NUBIFY_CHECKPOINT_END_VA: u32 = 0x0068_c0c8;
pub const MAP_POST_NUBIFY_SOURCE_TOKEN: u32 = 0x1eb9;
pub const MAP_POST_TRANSITIONS_CHECKPOINT_CALL_VA: u32 = 0x0068_c12a;
pub const MAP_POST_TRANSITIONS_SOURCE_TOKEN: u32 = 0x1ebe;

pub const TILESET_DATA_POINTER_SLOT_VA: u32 = 0x00e8_85f0;
pub const FOREST_BASE_OFFSET: u32 = 0x618;
pub const FOREST_CHANCE_OFFSET: u32 = 0x61c;
pub const MOUNTAIN_BASE_OFFSET: u32 = 0x620;
pub const MOUNTAIN_CHANCE_OFFSET: u32 = 0x624;
pub const COAST_BASE_OFFSET: u32 = 0x62c;
pub const COAST_CHANCE_OFFSET: u32 = 0x630;

pub const TERRAIN_GROUPS_CHANGE_FOREST_BASE_VA: u32 = 0x006a_9a60;
pub const TERRAIN_GROUPS_CHANGE_FOREST_BASE_RET_VA: u32 = 0x006a_9c02;
pub const TERRAIN_GROUPS_CHANGE_MOUNTAIN_BASE_VA: u32 = 0x006a_9c10;
pub const TERRAIN_GROUPS_CHANGE_MOUNTAIN_BASE_RET_VA: u32 = 0x006a_9dbf;
pub const TERRAIN_GROUPS_CHANGE_COAST_BASE_VA: u32 = 0x006a_9dc0;
pub const TERRAIN_GROUPS_CHANGE_COAST_BASE_RET_VA: u32 = 0x006a_9f62;
pub const TERRAIN_GROUPS_FIX_TRANSITIONS_VA: u32 = 0x006a_9f70;
pub const TERRAIN_GROUPS_FIX_TRANSITIONS_RET_VA: u32 = 0x006a_a205;
pub const TERRAIN_GROUPS_NUBIFY_TRANSITIONS_VA: u32 = 0x006a_1f30;
pub const TERRAIN_GROUPS_NUBIFY_TRANSITIONS_RET_VA: u32 = 0x006a_2314;

pub const MAP_CHANGE_FOREST_BASE_CALL_VA: u32 = 0x0068_c0d6;
pub const MAP_CHANGE_MOUNTAIN_BASE_CALL_VA: u32 = 0x0068_c0e9;
pub const MAP_CHANGE_COAST_BASE_CALL_VA: u32 = 0x0068_c0fc;
pub const MAP_FIX_TRANSITIONS_CALL_VA: u32 = 0x0068_c101;
pub const FIX_NUBIFY_THREE_CALL_VA: u32 = 0x006a_9f7e;
pub const FIX_NUBIFY_TWO_CALL_VA: u32 = 0x006a_a0b4;
pub const FIX_NUBIFY_ONE_CALL_VA: u32 = 0x006a_a1fa;

pub const FOREST_CHANCE_RANDOM_CALL_VA: u32 = 0x006a_9b7c;
pub const MOUNTAIN_CHANCE_RANDOM_CALL_VA: u32 = 0x006a_9d2d;
pub const COAST_CHANCE_RANDOM_CALL_VA: u32 = 0x006a_9edc;
pub const NUBIFY_COLUMN_RANDOM_CALL_VA: u32 = 0x006a_1fcb;
pub const NUBIFY_ROW_RANDOM_CALL_VA: u32 = 0x006a_21d0;
pub const RANDOM_GET_VA: u32 = 0x00a3_9d70;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainTransitionTuning {
    pub forest_base: i32,
    pub forest_chance: i32,
    pub mountain_base: i32,
    pub mountain_chance: i32,
    pub coast_base: i32,
    pub coast_chance: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerrainTransitionEvidence {
    RetailCapture {
        executable_sha256: String,
        capture_sha256: [u8; 32],
        tileset_data_pointer_slot_va: u32,
        tileset_data_object_va: u32,
        rules_after_constants: u32,
        checkpoint_call_va: u32,
        source_token: u32,
        world_checksum: WorldChecksum,
        random_state: i32,
    },
    SyntheticFixture {
        fixture: String,
        world_checksum: WorldChecksum,
        random_state: i32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerrainTransitionLiveFacts {
    pub tuning: TerrainTransitionTuning,
    pub evidence: TerrainTransitionEvidence,
}

impl TerrainTransitionLiveFacts {
    fn admissible_for(&self, checksum: &WorldChecksum, random_state: i32) -> bool {
        match &self.evidence {
            TerrainTransitionEvidence::RetailCapture {
                executable_sha256,
                capture_sha256,
                tileset_data_pointer_slot_va,
                tileset_data_object_va,
                rules_after_constants,
                checkpoint_call_va,
                source_token,
                world_checksum,
                random_state: captured_random_state,
            } => {
                executable_sha256 == SHIPPED_EXE_SHA256
                    && capture_sha256.iter().any(|byte| *byte != 0)
                    && *tileset_data_pointer_slot_va == TILESET_DATA_POINTER_SLOT_VA
                    && *tileset_data_object_va != 0
                    && *rules_after_constants == RETAIL_AFTER_CONSTANTS
                    && *checkpoint_call_va == MAP_POST_NUBIFY_CHECKPOINT_CALL_VA
                    && *source_token == MAP_POST_NUBIFY_SOURCE_TOKEN
                    && world_checksum == checksum
                    && *captured_random_state == random_state
            }
            TerrainTransitionEvidence::SyntheticFixture {
                fixture,
                world_checksum,
                random_state: captured_random_state,
            } => {
                cfg!(test)
                    && !fixture.is_empty()
                    && world_checksum == checksum
                    && *captured_random_state == random_state
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerrainTransitionStage {
    ChangeForestBase,
    ChangeMountainBase,
    ChangeCoastBase,
    NubifyThreeColumn,
    NubifyThreeRow,
    SpreadThreeToTwo,
    NubifyTwoColumn,
    NubifyTwoRow,
    SpreadTwoToOne,
    NubifyOneColumn,
    NubifyOneRow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerrainTransitionCall {
    pub call_va: u32,
    pub callee_va: u32,
    pub return_va: u32,
    pub enabled: bool,
    pub implicit_subtype: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerrainTransitionRandomDraw {
    pub call_va: u32,
    pub random_va: u32,
    pub stage: TerrainTransitionStage,
    pub scan_x: i32,
    pub scan_y: i32,
    pub primary_candidate: (i32, i32),
    pub fallback_candidate: Option<(i32, i32)>,
    pub accepted_candidate: Option<(i32, i32)>,
    pub state_before: i32,
    pub raw: i32,
    pub remainder: i32,
    pub state_after: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerrainTransitionWrite {
    pub stage: TerrainTransitionStage,
    pub x: i32,
    pub y: i32,
    pub old_land: i8,
    pub old_land_sub: u8,
    pub new_land: i8,
    pub new_land_sub: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostNubifyTransitionReceipt {
    pub caller_resume_va: u32,
    pub checkpoint_call_va: u32,
    pub checkpoint_resume_va: u32,
    pub checkpoint_end_va: u32,
    pub source_token: u32,
    pub calls: Vec<TerrainTransitionCall>,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub random_draws: Vec<TerrainTransitionRandomDraw>,
    pub writes: Vec<TerrainTransitionWrite>,
    pub byte_changing_writes: usize,
    pub checksum_before: WorldChecksum,
    pub checksum_after: WorldChecksum,
    pub next_checkpoint_call_va: u32,
    pub next_source_token: u32,
    pub sourced_walked_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PostNubifyTransitionError {
    NubifyForestReceiptMismatch,
    StoredChecksumMismatch,
    LiveFactsUnavailableOrMismatched,
    WorldShapeMismatch {
        xs: i32,
        ys: i32,
        size: i32,
        wdata: usize,
        tile_xs: i32,
        tile_ys: i32,
        tile_size: i32,
        tdata: usize,
    },
    UnexpectedChecksumSections {
        sections: Vec<WorldSection>,
    },
}

fn validate_world_shape(world: &World) -> Result<(), PostNubifyTransitionError> {
    let cells = world.xs.checked_mul(world.ys).filter(|cells| *cells >= 0);
    let tile_xs = world.xs.checked_mul(4);
    let tile_ys = world.ys.checked_mul(4);
    let tile_cells = tile_xs.and_then(|xs| tile_ys.and_then(|ys| xs.checked_mul(ys)));
    if world.xs < 0
        || world.xs != world.ys
        || cells != Some(world.size)
        || cells.map(|cells| cells as usize) != Some(world.wdata.len())
        || tile_xs != Some(world.tile_xs)
        || tile_ys != Some(world.tile_ys)
        || tile_cells != Some(world.tile_size)
        || tile_cells.map(|cells| cells as usize) != Some(world.tdata.len())
    {
        return Err(PostNubifyTransitionError::WorldShapeMismatch {
            xs: world.xs,
            ys: world.ys,
            size: world.size,
            wdata: world.wdata.len(),
            tile_xs: world.tile_xs,
            tile_ys: world.tile_ys,
            tile_size: world.tile_size,
            tdata: world.tdata.len(),
        });
    }
    Ok(())
}

fn store_land_sub(
    world: &mut World,
    stage: TerrainTransitionStage,
    x: i32,
    y: i32,
    subtype: u8,
    writes: &mut Vec<TerrainTransitionWrite>,
    byte_changing_writes: &mut usize,
) {
    let old = world.wdata(x, y).clone();
    let cell = world.wdata_mut(x, y);
    cell.land = land::FERTILE;
    cell.land_sub = subtype;
    if old.land != cell.land || old.land_sub != cell.land_sub {
        *byte_changing_writes += 1;
    }
    writes.push(TerrainTransitionWrite {
        stage,
        x,
        y,
        old_land: old.land,
        old_land_sub: old.land_sub,
        new_land: cell.land,
        new_land_sub: cell.land_sub,
    });
}

fn all_sixteen_tiles_are_mountain(world: &World, x: i32, y: i32) -> bool {
    for ordinal in 0..16 {
        let tx = x * 4 + ordinal % 4;
        let ty = y * 4 + ordinal / 4;
        let index = (ty * world.tile_xs + tx) as usize;
        if world.tdata[index] & tflag::BLOCKER_MASK != tflag::BLOCKER_MOUNTAIN {
            return false;
        }
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn change_base<F>(
    world: &mut World,
    random: &mut Random,
    stage: TerrainTransitionStage,
    base: i32,
    chance: i32,
    random_call_va: u32,
    is_source: F,
    draws: &mut Vec<TerrainTransitionRandomDraw>,
    writes: &mut Vec<TerrainTransitionWrite>,
    byte_changing_writes: &mut usize,
) where
    F: Fn(&World, i32, i32) -> bool,
{
    for x in 0..world.xs {
        for y in 0..world.ys {
            if !is_source(world, x, y) {
                continue;
            }
            store_land_sub(world, stage, x, y, base as u8, writes, byte_changing_writes);
            for ordinal in 0..4 {
                let nx = x + NEIGHBOUR4_DX[ordinal];
                let ny = y + NEIGHBOUR4_DY[ordinal];
                if !world.valid_w(nx, ny)
                    || is_source(world, nx, ny)
                    || world.wdata(nx, ny).land == land::OCEAN
                {
                    continue;
                }
                let state_before = random.state();
                let raw = random.get(0, 0xffff);
                let remainder = raw % 100;
                let accepted = remainder < chance;
                draws.push(TerrainTransitionRandomDraw {
                    call_va: random_call_va,
                    random_va: RANDOM_GET_VA,
                    stage,
                    scan_x: x,
                    scan_y: y,
                    primary_candidate: (nx, ny),
                    fallback_candidate: None,
                    accepted_candidate: accepted.then_some((nx, ny)),
                    state_before,
                    raw,
                    remainder,
                    state_after: random.state(),
                });
                if accepted {
                    store_land_sub(
                        world,
                        stage,
                        nx,
                        ny,
                        base as u8,
                        writes,
                        byte_changing_writes,
                    );
                }
            }
        }
    }
}

fn transition_candidate_eligible(world: &World, x: i32, y: i32, subtype: u8) -> bool {
    if !world.valid_w(x, y) {
        return false;
    }
    let cell = world.wdata(x, y);
    cell.flags & wflag::COAST == 0 && cell.land == land::FERTILE && cell.land_sub < subtype
}

#[allow(clippy::too_many_arguments)]
fn attempt_nubify_draw(
    world: &mut World,
    random: &mut Random,
    stage: TerrainTransitionStage,
    call_va: u32,
    scan_x: i32,
    scan_y: i32,
    divisor: i32,
    primary: (i32, i32),
    fallback: (i32, i32),
    subtype: u8,
    draws: &mut Vec<TerrainTransitionRandomDraw>,
    writes: &mut Vec<TerrainTransitionWrite>,
    byte_changing_writes: &mut usize,
) -> bool {
    let state_before = random.state();
    let raw = random.get(0, 0xffff);
    let remainder = raw % divisor;
    let primary = if divisor == 3 {
        (primary.0, primary.1 - remainder)
    } else {
        (primary.0 - remainder, primary.1)
    };
    let fallback = if divisor == 3 {
        (fallback.0, fallback.1 - remainder)
    } else {
        (fallback.0 - remainder, fallback.1)
    };
    let accepted = if transition_candidate_eligible(world, primary.0, primary.1, subtype) {
        Some(primary)
    } else if transition_candidate_eligible(world, fallback.0, fallback.1, subtype) {
        Some(fallback)
    } else {
        None
    };
    draws.push(TerrainTransitionRandomDraw {
        call_va,
        random_va: RANDOM_GET_VA,
        stage,
        scan_x,
        scan_y,
        primary_candidate: primary,
        fallback_candidate: Some(fallback),
        accepted_candidate: accepted,
        state_before,
        raw,
        remainder,
        state_after: random.state(),
    });
    if let Some((x, y)) = accepted {
        store_land_sub(world, stage, x, y, subtype, writes, byte_changing_writes);
        true
    } else {
        false
    }
}

#[allow(clippy::too_many_arguments)]
fn nubify_transitions(
    world: &mut World,
    random: &mut Random,
    subtype: u8,
    column_stage: TerrainTransitionStage,
    row_stage: TerrainTransitionStage,
    draws: &mut Vec<TerrainTransitionRandomDraw>,
    writes: &mut Vec<TerrainTransitionWrite>,
    byte_changing_writes: &mut usize,
) {
    // 0x006a1f50..0x006a2147: X outer, Y inner; threshold five.
    for x in 0..world.xs {
        let mut run = 0i32;
        for y in 0..world.ys {
            let cell = world.wdata(x, y);
            if cell.flags & wflag::COAST == 0
                && cell.land == land::FERTILE
                && cell.land_sub == subtype
            {
                run += 1;
                if run >= 5 {
                    let wrote = attempt_nubify_draw(
                        world,
                        random,
                        column_stage,
                        NUBIFY_COLUMN_RANDOM_CALL_VA,
                        x,
                        y,
                        3,
                        (x - 1, y - 1),
                        (x + 1, y - 1),
                        subtype,
                        draws,
                        writes,
                        byte_changing_writes,
                    );
                    if wrote {
                        run = 0;
                    }
                }
            } else {
                run = 0;
            }
        }
    }

    // 0x006a2147..0x006a230e: Y outer, X inner; threshold four.  Retail compares
    // both loop bounds with `xs`, so the transaction admits square generated worlds only.
    for y in 0..world.xs {
        let mut run = 0i32;
        for x in 0..world.xs {
            let cell = world.wdata(x, y);
            if cell.flags & wflag::COAST == 0
                && cell.land == land::FERTILE
                && cell.land_sub == subtype
            {
                run += 1;
                if run >= 4 {
                    let wrote = attempt_nubify_draw(
                        world,
                        random,
                        row_stage,
                        NUBIFY_ROW_RANDOM_CALL_VA,
                        x,
                        y,
                        4,
                        (x, y - 1),
                        (x, y + 1),
                        subtype,
                        draws,
                        writes,
                        byte_changing_writes,
                    );
                    if wrote {
                        run = 0;
                    }
                }
            } else {
                run = 0;
            }
        }
    }
}

fn spread_transition(
    world: &mut World,
    source_subtype: u8,
    destination_subtype: u8,
    stage: TerrainTransitionStage,
    writes: &mut Vec<TerrainTransitionWrite>,
    byte_changing_writes: &mut usize,
) {
    for x in 0..world.xs {
        for y in 0..world.ys {
            let source = world.wdata(x, y);
            if source.land != land::FERTILE || source.land_sub != source_subtype {
                continue;
            }
            for ordinal in 0..8 {
                let nx = x + NEIGHBOUR_DX[ordinal];
                let ny = y + NEIGHBOUR_DY[ordinal];
                if !world.valid_w(nx, ny) {
                    continue;
                }
                let candidate = world.wdata(nx, ny);
                if candidate.flags & wflag::WATERHALF == 0
                    && matches!(candidate.land, land::COASTAL | land::OCEAN)
                    && candidate.flags & wflag::COAST == 0
                {
                    continue;
                }
                let already_allowed = candidate.land == land::FERTILE
                    && match destination_subtype {
                        2 => matches!(candidate.land_sub, 2 | 3),
                        1 => matches!(candidate.land_sub, 1 | 2 | 3),
                        _ => false,
                    };
                if !already_allowed {
                    store_land_sub(
                        world,
                        stage,
                        nx,
                        ny,
                        destination_subtype,
                        writes,
                        byte_changing_writes,
                    );
                }
            }
        }
    }
}

/// Execute the exact caller sequence through the checksum checkpoint at token `0x1ebe`.
///
/// World and RNG work is staged.  An invalid handoff, unavailable live capture, malformed
/// generated-world shape, or checksum-scope violation leaves the authoritative map and the
/// upstream RNG handoff untouched.
pub fn execute_post_nubify_transitions(
    map: &mut InitialWorld,
    previous: &NubifyForestReceipt,
    facts: &TerrainTransitionLiveFacts,
) -> Result<PostNubifyTransitionReceipt, PostNubifyTransitionError> {
    if previous.call_va != MAP_NUBIFY_FOREST_CALL_VA
        || previous.entry_va != TERRAIN_GROUPS_NUBIFY_FOREST_VA
        || previous.return_va != TERRAIN_GROUPS_NUBIFY_FOREST_RET_VA
        || previous.caller_resume_va != MAP_NUBIFY_FOREST_CALLER_RESUME_VA
        || previous.stack_argument_read
        || previous.checksum_after != map.checksum
        || previous.sourced_walked_bytes != map.sourced_walked_bytes
        || previous
            .checksum_before
            .differing_sections(&previous.checksum_after)
            .iter()
            .any(|section| *section != WorldSection::WData)
    {
        return Err(PostNubifyTransitionError::NubifyForestReceiptMismatch);
    }
    validate_world_shape(&map.world)?;
    if map.world.checksum_sections() != map.checksum {
        return Err(PostNubifyTransitionError::StoredChecksumMismatch);
    }
    if !facts.admissible_for(&map.checksum, previous.random_state_after) {
        return Err(PostNubifyTransitionError::LiveFactsUnavailableOrMismatched);
    }

    let checksum_before = map.checksum.clone();
    let random_state_before = previous.random_state_after;
    let mut random = Random::new(random_state_before);
    let mut staged = map.world.clone();
    let mut calls = Vec::new();
    let mut draws = Vec::new();
    let mut writes = Vec::new();
    let mut byte_changing_writes = 0usize;
    let tuning = facts.tuning;

    let base_calls = [
        (
            MAP_CHANGE_FOREST_BASE_CALL_VA,
            TERRAIN_GROUPS_CHANGE_FOREST_BASE_VA,
            TERRAIN_GROUPS_CHANGE_FOREST_BASE_RET_VA,
            tuning.forest_base >= 0,
        ),
        (
            MAP_CHANGE_MOUNTAIN_BASE_CALL_VA,
            TERRAIN_GROUPS_CHANGE_MOUNTAIN_BASE_VA,
            TERRAIN_GROUPS_CHANGE_MOUNTAIN_BASE_RET_VA,
            tuning.mountain_base >= 0,
        ),
        (
            MAP_CHANGE_COAST_BASE_CALL_VA,
            TERRAIN_GROUPS_CHANGE_COAST_BASE_VA,
            TERRAIN_GROUPS_CHANGE_COAST_BASE_RET_VA,
            tuning.coast_base >= 0,
        ),
    ];
    calls.extend(base_calls.map(|(call_va, callee_va, return_va, enabled)| {
        TerrainTransitionCall {
            call_va,
            callee_va,
            return_va,
            enabled,
            implicit_subtype: None,
        }
    }));

    if tuning.forest_base >= 0 {
        change_base(
            &mut staged,
            &mut random,
            TerrainTransitionStage::ChangeForestBase,
            tuning.forest_base,
            tuning.forest_chance,
            FOREST_CHANCE_RANDOM_CALL_VA,
            |world, x, y| world.wdata(x, y).flags & wflag::FOREST != 0,
            &mut draws,
            &mut writes,
            &mut byte_changing_writes,
        );
    }
    if tuning.mountain_base >= 0 {
        change_base(
            &mut staged,
            &mut random,
            TerrainTransitionStage::ChangeMountainBase,
            tuning.mountain_base,
            tuning.mountain_chance,
            MOUNTAIN_CHANCE_RANDOM_CALL_VA,
            all_sixteen_tiles_are_mountain,
            &mut draws,
            &mut writes,
            &mut byte_changing_writes,
        );
    }
    if tuning.coast_base >= 0 {
        change_base(
            &mut staged,
            &mut random,
            TerrainTransitionStage::ChangeCoastBase,
            tuning.coast_base,
            tuning.coast_chance,
            COAST_CHANCE_RANDOM_CALL_VA,
            |world, x, y| world.wdata(x, y).flags & wflag::COAST != 0,
            &mut draws,
            &mut writes,
            &mut byte_changing_writes,
        );
    }

    calls.push(TerrainTransitionCall {
        call_va: MAP_FIX_TRANSITIONS_CALL_VA,
        callee_va: TERRAIN_GROUPS_FIX_TRANSITIONS_VA,
        return_va: TERRAIN_GROUPS_FIX_TRANSITIONS_RET_VA,
        enabled: true,
        implicit_subtype: None,
    });
    for (call_va, subtype) in [
        (FIX_NUBIFY_THREE_CALL_VA, 3),
        (FIX_NUBIFY_TWO_CALL_VA, 2),
        (FIX_NUBIFY_ONE_CALL_VA, 1),
    ] {
        calls.push(TerrainTransitionCall {
            call_va,
            callee_va: TERRAIN_GROUPS_NUBIFY_TRANSITIONS_VA,
            return_va: TERRAIN_GROUPS_NUBIFY_TRANSITIONS_RET_VA,
            enabled: true,
            implicit_subtype: Some(subtype),
        });
    }

    nubify_transitions(
        &mut staged,
        &mut random,
        3,
        TerrainTransitionStage::NubifyThreeColumn,
        TerrainTransitionStage::NubifyThreeRow,
        &mut draws,
        &mut writes,
        &mut byte_changing_writes,
    );
    spread_transition(
        &mut staged,
        3,
        2,
        TerrainTransitionStage::SpreadThreeToTwo,
        &mut writes,
        &mut byte_changing_writes,
    );
    nubify_transitions(
        &mut staged,
        &mut random,
        2,
        TerrainTransitionStage::NubifyTwoColumn,
        TerrainTransitionStage::NubifyTwoRow,
        &mut draws,
        &mut writes,
        &mut byte_changing_writes,
    );
    spread_transition(
        &mut staged,
        2,
        1,
        TerrainTransitionStage::SpreadTwoToOne,
        &mut writes,
        &mut byte_changing_writes,
    );
    nubify_transitions(
        &mut staged,
        &mut random,
        1,
        TerrainTransitionStage::NubifyOneColumn,
        TerrainTransitionStage::NubifyOneRow,
        &mut draws,
        &mut writes,
        &mut byte_changing_writes,
    );

    let checksum_after = staged.checksum_sections();
    let sections = checksum_before.differing_sections(&checksum_after);
    if sections
        .iter()
        .any(|section| *section != WorldSection::WData)
    {
        return Err(PostNubifyTransitionError::UnexpectedChecksumSections { sections });
    }

    map.world = staged;
    map.checksum = checksum_after.clone();
    Ok(PostNubifyTransitionReceipt {
        caller_resume_va: MAP_NUBIFY_FOREST_CALLER_RESUME_VA,
        checkpoint_call_va: MAP_POST_NUBIFY_CHECKPOINT_CALL_VA,
        checkpoint_resume_va: MAP_POST_NUBIFY_CHECKPOINT_RESUME_VA,
        checkpoint_end_va: MAP_POST_NUBIFY_CHECKPOINT_END_VA,
        source_token: MAP_POST_NUBIFY_SOURCE_TOKEN,
        calls,
        random_state_before,
        random_state_after: random.state(),
        random_draws: draws,
        writes,
        byte_changing_writes,
        checksum_before,
        checksum_after,
        next_checkpoint_call_va: MAP_POST_TRANSITIONS_CHECKPOINT_CALL_VA,
        next_source_token: MAP_POST_TRANSITIONS_SOURCE_TOKEN,
        sourced_walked_bytes: map.sourced_walked_bytes,
    })
}
