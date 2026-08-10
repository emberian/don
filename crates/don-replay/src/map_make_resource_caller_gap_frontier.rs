// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact `Map::make` caller bridge from checksum token `0x1ebe` to
//! the conditional `Map::place_resources` call.
//!
//! Shipped code in `0x0068c12f..0x0068c707` performs checksum logging,
//! progress/UI work, read-only diagnostics, and a lazy resource-placement
//! gate. It consumes no RNG and writes neither `Map` nor `World`. This module
//! preserves the incoming deterministic state and emits a typed entry receipt
//! only when the native gate reaches the call at `0x0068c707`.

use don_sim::systems::map_terrain::WorldChecksum;

pub const SHIPPED_EXE_SHA256: &str =
    "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079";
pub const SHIPPED_PDB_SHA256: &str =
    "334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5";

pub const MAP_MAKE_VA: u32 = 0x0068_bc90;
pub const MAP_POST_TRANSITIONS_CHECKPOINT_CALL_VA: u32 = 0x0068_c12a;
pub const MAP_POST_TRANSITIONS_SOURCE_TOKEN: u32 = 0x1ebe;
pub const MAP_RESOURCE_CALLER_GAP_RESUME_VA: u32 = 0x0068_c12f;
pub const MAP_RESOURCE_PROGRESS_REFRESH_CALL_VA: u32 = 0x0068_c16a;
pub const MAP_RESOURCE_PROGRESS_CHECKPOINT_CALL_VA: u32 = 0x0068_c1a2;
pub const MAP_RESOURCE_PROGRESS_SOURCE_TOKEN: u32 = 0x1ec5;
pub const MAP_RESOURCE_DIAGNOSTIC_CHECKPOINT_CALL_VA: u32 = 0x0068_c1da;
pub const MAP_RESOURCE_DIAGNOSTIC_SOURCE_TOKEN: u32 = 0x1ec8;
pub const MAP_RESOURCE_DIAGNOSTICS_BEGIN_VA: u32 = 0x0068_c1ee;
pub const MAP_RESOURCE_PLAYER_DIAGNOSTICS_LOOP_VA: u32 = 0x0068_c590;
pub const MAP_RESOURCE_FINAL_REFRESH_CALL_VA: u32 = 0x0068_c6c7;
pub const MAP_RESOURCE_GATE_BEGIN_VA: u32 = 0x0068_c6dd;
pub const MAP_PLACE_RESOURCES_CALL_VA: u32 = 0x0068_c707;
pub const MAP_PLACE_RESOURCES_ENTRY_VA: u32 = 0x0068_f4f0;
pub const MAP_RESOURCE_CALLER_CONTINUATION_VA: u32 = 0x0068_c70c;
pub const MAP_POST_RESOURCES_CHECKPOINT_CALL_VA: u32 = 0x0068_c72d;
pub const MAP_POST_RESOURCES_SOURCE_TOKEN: u32 = 0x1ef7;

pub const GAME_SEMAPHORE_OFFSET: u32 = 0x814;
pub const GAME_SEMAPHORE_BIT9_BYTE_OFFSET: u32 = 0x821;
pub const GAME_SEMAPHORE_BIT17_BYTE_OFFSET: u32 = 0x822;
pub const SEMAPHORE_TEST_MASK: u8 = 0x02;
pub const CONQUEST_GAME_STYLE_OFFSET: u32 = 0x1480;
pub const CONQUEST_STYLE_NO_RARE_OFFSET: u32 = 0x68;

pub const GAME_LOG_SAY_CHECKSUM_VA: u32 = 0x0093_0b30;
pub const SPLASH_SCREEN_REFRESH_VA: u32 = 0x0083_8ce0;
pub const LOG_SAY_VA: u32 = 0x00a3_ab00;
pub const VECTOR_DIST_VA: u32 = 0x0046_cff0;
pub const RANDOM_GET_VA: u32 = 0x00a3_9d70;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallerGapCheckpoint {
    pub call_va: u32,
    pub source_token: u32,
}

pub const CALLER_GAP_CHECKPOINTS: [CallerGapCheckpoint; 2] = [
    CallerGapCheckpoint {
        call_va: MAP_RESOURCE_PROGRESS_CHECKPOINT_CALL_VA,
        source_token: MAP_RESOURCE_PROGRESS_SOURCE_TOKEN,
    },
    CallerGapCheckpoint {
        call_va: MAP_RESOURCE_DIAGNOSTIC_CHECKPOINT_CALL_VA,
        source_token: MAP_RESOURCE_DIAGNOSTIC_SOURCE_TOKEN,
    },
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapMakeResourceCallerPriorReceipt {
    pub checkpoint_call_va: u32,
    pub source_token: u32,
    pub resume_va: u32,
    pub map_object_identity: u64,
    pub world_object_identity: u64,
    pub game_object_identity: u64,
    pub conquest_game_identity: u64,
    /// Opaque digest of the authoritative `Map` projection at token `0x1ebe`.
    pub map_state_sha256: [u8; 32],
    pub world_checksum: WorldChecksum,
    pub sourced_walked_bytes: u64,
    pub random_state: i32,
    /// First argument to `Map::make`, reloaded into EDI before the endpoint.
    pub player_count_argument: i32,
    /// Third argument to `Map::make`, retained in ESI throughout the body.
    pub progress_requested: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapMakeResourceCallerEvidence {
    RetailCapture {
        executable_sha256: String,
        pdb_sha256: String,
        capture_sha256: [u8; 32],
        map_make_va: u32,
        gap_resume_va: u32,
        place_resources_call_va: u32,
        map_object_identity: u64,
        world_object_identity: u64,
        game_object_identity: u64,
        conquest_game_identity: u64,
        map_state_sha256: [u8; 32],
        world_checksum: WorldChecksum,
        sourced_walked_bytes: u64,
        random_state: i32,
        player_count_argument: i32,
        progress_requested: bool,
        game_semaphore_0x821: u8,
        game_semaphore_0x822: u8,
        conquest_style_no_rare: Option<i32>,
        world_map_index_nonnegative: bool,
    },
    SyntheticFixture {
        fixture: String,
        map_object_identity: u64,
        world_object_identity: u64,
        game_object_identity: u64,
        conquest_game_identity: u64,
        map_state_sha256: [u8; 32],
        world_checksum: WorldChecksum,
        sourced_walked_bytes: u64,
        random_state: i32,
        player_count_argument: i32,
        progress_requested: bool,
        game_semaphore_0x821: u8,
        game_semaphore_0x822: u8,
        conquest_style_no_rare: Option<i32>,
        world_map_index_nonnegative: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapMakeResourceCallerFacts {
    pub game_semaphore_0x821: u8,
    pub game_semaphore_0x822: u8,
    /// `None` reproduces a null `ConquestGame::game_style` pointer. Retail
    /// reads `ConquestStyle::no_rare` only when CTW is set and this is `Some`.
    pub conquest_style_no_rare: Option<i32>,
    /// The first optional map-name log is present iff `World::map >= 0`.
    pub world_map_index_nonnegative: bool,
    pub evidence: MapMakeResourceCallerEvidence,
}

impl MapMakeResourceCallerFacts {
    fn evidence_matches(&self, prior: &MapMakeResourceCallerPriorReceipt) -> bool {
        match &self.evidence {
            MapMakeResourceCallerEvidence::RetailCapture {
                executable_sha256,
                pdb_sha256,
                capture_sha256,
                map_make_va,
                gap_resume_va,
                place_resources_call_va,
                map_object_identity,
                world_object_identity,
                game_object_identity,
                conquest_game_identity,
                map_state_sha256,
                world_checksum,
                sourced_walked_bytes,
                random_state,
                player_count_argument,
                progress_requested,
                game_semaphore_0x821,
                game_semaphore_0x822,
                conquest_style_no_rare,
                world_map_index_nonnegative,
            } => {
                executable_sha256 == SHIPPED_EXE_SHA256
                    && pdb_sha256 == SHIPPED_PDB_SHA256
                    && capture_sha256.iter().any(|byte| *byte != 0)
                    && *map_make_va == MAP_MAKE_VA
                    && *gap_resume_va == MAP_RESOURCE_CALLER_GAP_RESUME_VA
                    && *place_resources_call_va == MAP_PLACE_RESOURCES_CALL_VA
                    && *map_object_identity == prior.map_object_identity
                    && *world_object_identity == prior.world_object_identity
                    && *game_object_identity == prior.game_object_identity
                    && *conquest_game_identity == prior.conquest_game_identity
                    && map_state_sha256 == &prior.map_state_sha256
                    && world_checksum == &prior.world_checksum
                    && *sourced_walked_bytes == prior.sourced_walked_bytes
                    && *random_state == prior.random_state
                    && *player_count_argument == prior.player_count_argument
                    && *progress_requested == prior.progress_requested
                    && *game_semaphore_0x821 == self.game_semaphore_0x821
                    && *game_semaphore_0x822 == self.game_semaphore_0x822
                    && conquest_style_no_rare == &self.conquest_style_no_rare
                    && *world_map_index_nonnegative == self.world_map_index_nonnegative
            }
            MapMakeResourceCallerEvidence::SyntheticFixture {
                fixture,
                map_object_identity,
                world_object_identity,
                game_object_identity,
                conquest_game_identity,
                map_state_sha256,
                world_checksum,
                sourced_walked_bytes,
                random_state,
                player_count_argument,
                progress_requested,
                game_semaphore_0x821,
                game_semaphore_0x822,
                conquest_style_no_rare,
                world_map_index_nonnegative,
            } => {
                cfg!(test)
                    && !fixture.is_empty()
                    && *map_object_identity == prior.map_object_identity
                    && *world_object_identity == prior.world_object_identity
                    && *game_object_identity == prior.game_object_identity
                    && *conquest_game_identity == prior.conquest_game_identity
                    && map_state_sha256 == &prior.map_state_sha256
                    && world_checksum == &prior.world_checksum
                    && *sourced_walked_bytes == prior.sourced_walked_bytes
                    && *random_state == prior.random_state
                    && *player_count_argument == prior.player_count_argument
                    && *progress_requested == prior.progress_requested
                    && *game_semaphore_0x821 == self.game_semaphore_0x821
                    && *game_semaphore_0x822 == self.game_semaphore_0x822
                    && conquest_style_no_rare == &self.conquest_style_no_rare
                    && *world_map_index_nonnegative == self.world_map_index_nonnegative
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceGateRead {
    GameSemaphoreBit9 {
        instruction_va: u32,
        byte_offset: u32,
        mask: u8,
    },
    GameSemaphoreBit17 {
        instruction_va: u32,
        byte_offset: u32,
        mask: u8,
    },
    ConquestGameStylePointer {
        instruction_va: u32,
        offset: u32,
    },
    ConquestStyleNoRare {
        instruction_va: u32,
        offset: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaceResourcesCallDisposition {
    Called,
    SkippedGameSemaphoreBit9,
    SkippedConquestNoRare,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceResourcesCallerEntryReceipt {
    /// Checkpoint which began this caller bridge.
    pub upstream_checkpoint_call_va: u32,
    pub upstream_source_token: u32,
    /// Last checksum deadline before the diagnostics and endpoint gate.
    pub latest_checkpoint_call_va: u32,
    pub latest_source_token: u32,
    pub call_va: u32,
    pub entry_va: u32,
    pub player_count_argument: i32,
    pub map_object_identity: u64,
    pub world_object_identity: u64,
    pub game_object_identity: u64,
    pub map_state_sha256: [u8; 32],
    pub world_checksum: WorldChecksum,
    pub sourced_walked_bytes: u64,
    pub random_state_at_entry: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapMakeResourceCallerGapReceipt {
    pub resume_va: u32,
    pub checkpoints: [CallerGapCheckpoint; 2],
    pub diagnostics_begin_va: u32,
    pub diagnostic_log_calls: usize,
    pub player_diagnostic_iterations: usize,
    pub progress_refresh_calls: usize,
    pub gate_begin_va: u32,
    pub gate_reads: Vec<ResourceGateRead>,
    pub disposition: PlaceResourcesCallDisposition,
    pub place_resources_entry: Option<PlaceResourcesCallerEntryReceipt>,
    pub continuation_va: u32,
    pub post_resources_checkpoint_call_va: u32,
    pub post_resources_source_token: u32,
    pub random_get_va: u32,
    pub random_draws: usize,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub map_state_sha256_before: [u8; 32],
    pub map_state_sha256_after: [u8; 32],
    pub world_checksum_before: WorldChecksum,
    pub world_checksum_after: WorldChecksum,
    pub sourced_walked_bytes_before: u64,
    pub sourced_walked_bytes_after: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapMakeResourceCallerGapError {
    PriorReceiptMismatch,
    EmptyMapStateDigest,
    LiveFactsUnavailableOrMismatched,
}

/// Reproduce the exact no-RNG/no-map-write bridge and freeze the conditional
/// `Map::place_resources` entry handoff.
pub fn execute_map_make_resource_caller_gap(
    prior: &MapMakeResourceCallerPriorReceipt,
    facts: &MapMakeResourceCallerFacts,
) -> Result<MapMakeResourceCallerGapReceipt, MapMakeResourceCallerGapError> {
    if prior.checkpoint_call_va != MAP_POST_TRANSITIONS_CHECKPOINT_CALL_VA
        || prior.source_token != MAP_POST_TRANSITIONS_SOURCE_TOKEN
        || prior.resume_va != MAP_RESOURCE_CALLER_GAP_RESUME_VA
    {
        return Err(MapMakeResourceCallerGapError::PriorReceiptMismatch);
    }
    if prior.map_state_sha256.iter().all(|byte| *byte == 0) {
        return Err(MapMakeResourceCallerGapError::EmptyMapStateDigest);
    }
    if !facts.evidence_matches(prior) {
        return Err(MapMakeResourceCallerGapError::LiveFactsUnavailableOrMismatched);
    }

    let bit9_set = facts.game_semaphore_0x821 & SEMAPHORE_TEST_MASK != 0;
    let bit17_set = facts.game_semaphore_0x822 & SEMAPHORE_TEST_MASK != 0;
    let mut gate_reads = vec![ResourceGateRead::GameSemaphoreBit9 {
        instruction_va: MAP_RESOURCE_GATE_BEGIN_VA,
        byte_offset: GAME_SEMAPHORE_BIT9_BYTE_OFFSET,
        mask: SEMAPHORE_TEST_MASK,
    }];

    let disposition = if bit9_set {
        PlaceResourcesCallDisposition::SkippedGameSemaphoreBit9
    } else {
        gate_reads.push(ResourceGateRead::GameSemaphoreBit17 {
            instruction_va: 0x0068_c6e6,
            byte_offset: GAME_SEMAPHORE_BIT17_BYTE_OFFSET,
            mask: SEMAPHORE_TEST_MASK,
        });
        if !bit17_set {
            PlaceResourcesCallDisposition::Called
        } else {
            gate_reads.push(ResourceGateRead::ConquestGameStylePointer {
                instruction_va: 0x0068_c6ef,
                offset: CONQUEST_GAME_STYLE_OFFSET,
            });
            match facts.conquest_style_no_rare {
                None => PlaceResourcesCallDisposition::Called,
                Some(no_rare) => {
                    gate_reads.push(ResourceGateRead::ConquestStyleNoRare {
                        instruction_va: 0x0068_c6fe,
                        offset: CONQUEST_STYLE_NO_RARE_OFFSET,
                    });
                    if no_rare == 0 {
                        PlaceResourcesCallDisposition::Called
                    } else {
                        PlaceResourcesCallDisposition::SkippedConquestNoRare
                    }
                }
            }
        }
    };

    let player_diagnostic_iterations = if bit17_set {
        0
    } else {
        prior.player_count_argument.max(0) as usize
    };
    let diagnostic_log_calls =
        4 + usize::from(facts.world_map_index_nonnegative) + player_diagnostic_iterations;
    let place_resources_entry = (disposition == PlaceResourcesCallDisposition::Called).then(|| {
        PlaceResourcesCallerEntryReceipt {
            upstream_checkpoint_call_va: prior.checkpoint_call_va,
            upstream_source_token: prior.source_token,
            latest_checkpoint_call_va: MAP_RESOURCE_DIAGNOSTIC_CHECKPOINT_CALL_VA,
            latest_source_token: MAP_RESOURCE_DIAGNOSTIC_SOURCE_TOKEN,
            call_va: MAP_PLACE_RESOURCES_CALL_VA,
            entry_va: MAP_PLACE_RESOURCES_ENTRY_VA,
            player_count_argument: prior.player_count_argument,
            map_object_identity: prior.map_object_identity,
            world_object_identity: prior.world_object_identity,
            game_object_identity: prior.game_object_identity,
            map_state_sha256: prior.map_state_sha256,
            world_checksum: prior.world_checksum.clone(),
            sourced_walked_bytes: prior.sourced_walked_bytes,
            random_state_at_entry: prior.random_state,
        }
    });

    Ok(MapMakeResourceCallerGapReceipt {
        resume_va: MAP_RESOURCE_CALLER_GAP_RESUME_VA,
        checkpoints: CALLER_GAP_CHECKPOINTS,
        diagnostics_begin_va: MAP_RESOURCE_DIAGNOSTICS_BEGIN_VA,
        diagnostic_log_calls,
        player_diagnostic_iterations,
        progress_refresh_calls: usize::from(prior.progress_requested) * 2,
        gate_begin_va: MAP_RESOURCE_GATE_BEGIN_VA,
        gate_reads,
        disposition,
        place_resources_entry,
        continuation_va: MAP_RESOURCE_CALLER_CONTINUATION_VA,
        post_resources_checkpoint_call_va: MAP_POST_RESOURCES_CHECKPOINT_CALL_VA,
        post_resources_source_token: MAP_POST_RESOURCES_SOURCE_TOKEN,
        random_get_va: RANDOM_GET_VA,
        random_draws: 0,
        random_state_before: prior.random_state,
        random_state_after: prior.random_state,
        map_state_sha256_before: prior.map_state_sha256,
        map_state_sha256_after: prior.map_state_sha256,
        world_checksum_before: prior.world_checksum.clone(),
        world_checksum_after: prior.world_checksum.clone(),
        sourced_walked_bytes_before: prior.sourced_walked_bytes,
        sourced_walked_bytes_after: prior.sourced_walked_bytes,
    })
}
