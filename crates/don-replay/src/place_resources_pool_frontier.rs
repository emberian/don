// SPDX-License-Identifier: GPL-3.0-or-later
//! Source-only replay frontier for the deterministic prefix of `Map::place_resources`.
//!
//! The caller reaches `Map::place_resources` at `0x0068c707`, after an unresolved caller gap
//! from the checksum token `0x1ebe`. The exact prefix reconstructed here spans
//! `0x0068f4f0..0x0068f597`: it filters good IDs `6..=49`, extends the early/late/water
//! divvy lists, and recreates all three `DynamicBitMask` objects. It consumes no RNG and does
//! not write World, so the entry RNG state and upstream World checksum remain unchanged.

use don_sim::systems::map_terrain::WorldChecksum;

pub const SHIPPED_EXE_SHA256: &str =
    "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079";
pub const SHIPPED_PDB_SHA256: &str =
    "334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5";

pub const MAP_POST_TRANSITIONS_CHECKPOINT_CALL_VA: u32 = 0x0068_c12a;
pub const MAP_POST_TRANSITIONS_SOURCE_TOKEN: u32 = 0x1ebe;
pub const MAP_POST_TRANSITIONS_CALLER_RESUME_VA: u32 = 0x0068_c12f;
pub const MAP_PLACE_RESOURCES_CALL_VA: u32 = 0x0068_c707;
pub const MAP_PLACE_RESOURCES_ENTRY_VA: u32 = 0x0068_f4f0;
pub const MAP_PLACE_RESOURCES_SCAN_VA: u32 = 0x0068_f540;
pub const MAP_PLACE_RESOURCES_ADD_GOOD_CALL_VA: u32 = 0x0068_f579;
pub const MAP_PLACE_RESOURCES_DONE_ADDING_CALL_VA: u32 = 0x0068_f592;
pub const MAP_PLACE_RESOURCES_PREFIX_RESIDUAL_VA: u32 = 0x0068_f597;

pub const RESOURCE_DIVVY_DONE_ADDING_GOODS_VA: u32 = 0x0068_a700;
pub const RESOURCE_DIVVY_ADD_GOOD_VA: u32 = 0x0068_a830;
pub const DYNAMIC_BIT_MASK_INIT_VA: u32 = 0x00a3_a3c0;
pub const RANDOM_GET_VA: u32 = 0x00a3_9d70;

pub const GAME_CONST_REFERENCE_VA: u32 = 0x00c0_61e8;
pub const GOOD_TYPES_REFERENCE_VA: u32 = 0x00c0_61ac;
pub const LANDS_OCEAN_RARE_LIST_POINTER_SLOT_VA: u32 = 0x00e3_a380;
pub const GAME_SEMAPHORE_OFFSET: u32 = 0x814;
pub const GAME_SEMAPHORE_CONQUEST_BIT: u32 = 17;
pub const GOOD_TYPE_AGE_OFFSET: u32 = 0x278;
pub const GOOD_TYPE_EXCLUDE_CTW_OFFSET: u32 = 0x2f4;
pub const GOOD_TYPE_RANDOM_RARE_DROP_OFFSET: u32 = 0x2f8;
pub const OCEAN_LAND_INDEX: i32 = 2;
pub const LAND_NUM_RARE_OFFSET: u32 = 0x4c;
pub const LAND_RARE_OFFSET: u32 = 0x50;

pub const FIRST_SCANNED_GOOD_ID: i32 = 6;
pub const SCANNED_GOOD_END_EXCLUSIVE: i32 = 50;
pub const SCANNED_GOOD_COUNT: usize = (SCANNED_GOOD_END_EXCLUSIVE - FIRST_SCANNED_GOOD_ID) as usize;
pub const LAND_RARE_CAPACITY: usize = 44;
pub const WEIGHTED_WATER_GOOD_ID: i32 = 6;
pub const WEIGHTED_WATER_GOOD_COPIES: usize = 10;
pub const EARLY_AGE_END_EXCLUSIVE: i32 = 3;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResourcePoolBitMask {
    /// Logical bit count passed to `DynamicBitMask::init`.
    pub bits: i32,
    /// Native allocation size, `(bits + 7) / 8`; every byte is zero after both clear passes.
    pub bytes: Vec<u8>,
}

impl ResourcePoolBitMask {
    fn zeroed(bits: usize) -> Self {
        Self {
            bits: bits as i32,
            bytes: vec![0; bits.saturating_add(7) / 8],
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResourceDivvyPoolState {
    pub early_bits: ResourcePoolBitMask,
    pub early_goods: Vec<i32>,
    pub late_bits: ResourcePoolBitMask,
    pub late_goods: Vec<i32>,
    pub water_bits: ResourcePoolBitMask,
    pub water_goods: Vec<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceResourcesEntryHandoff {
    /// Last completed caller checksum before the unresolved `Map::make` gap.
    pub checkpoint_call_va: u32,
    pub source_token: u32,
    pub world_checksum: WorldChecksum,
    pub sourced_walked_bytes: u64,
    /// Exact direct call which entered this frontier.
    pub call_va: u32,
    pub entry_va: u32,
    pub player_count_argument: i32,
    /// Captured at function entry; deliberately not inferred from token `0x1ebe`.
    pub random_state_at_entry: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RareGoodLiveFact {
    pub good_id: i32,
    /// Resolved `ObjectTypeData::age`, including `get_age_slow` when stored age was `-1`.
    pub resolved_age: i32,
    pub exclude_ctw: i32,
    pub random_rare_drop: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaceResourcesFactEvidence {
    RetailCapture {
        executable_sha256: String,
        pdb_sha256: String,
        capture_sha256: [u8; 32],
        game_const_reference_va: u32,
        good_types_reference_va: u32,
        lands_ocean_rare_list_pointer_slot_va: u32,
        checkpoint_call_va: u32,
        source_token: u32,
        call_va: u32,
        entry_va: u32,
        world_checksum: WorldChecksum,
        sourced_walked_bytes: u64,
        random_state_at_entry: i32,
    },
    SyntheticFixture {
        fixture: String,
        checkpoint_call_va: u32,
        source_token: u32,
        call_va: u32,
        entry_va: u32,
        world_checksum: WorldChecksum,
        sourced_walked_bytes: u64,
        random_state_at_entry: i32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceResourcesLiveFacts {
    /// `Game::semaphore` bit 17, tested at `Game + 0x822 & 0x02`.
    pub conquer_the_world: bool,
    /// Exact `GoodType` projection for the native inclusive range `6..=49`.
    pub goods: Vec<RareGoodLiveFact>,
    /// `lands[2].rare[0..num_rare]`, where land index 2 is Ocean.
    pub ocean_rare_good_ids: Vec<i32>,
    pub evidence: PlaceResourcesFactEvidence,
}

impl PlaceResourcesLiveFacts {
    fn evidence_matches(&self, entry: &PlaceResourcesEntryHandoff) -> bool {
        match &self.evidence {
            PlaceResourcesFactEvidence::RetailCapture {
                executable_sha256,
                pdb_sha256,
                capture_sha256,
                game_const_reference_va,
                good_types_reference_va,
                lands_ocean_rare_list_pointer_slot_va,
                checkpoint_call_va,
                source_token,
                call_va,
                entry_va,
                world_checksum,
                sourced_walked_bytes,
                random_state_at_entry,
            } => {
                executable_sha256 == SHIPPED_EXE_SHA256
                    && pdb_sha256 == SHIPPED_PDB_SHA256
                    && capture_sha256.iter().any(|byte| *byte != 0)
                    && *game_const_reference_va == GAME_CONST_REFERENCE_VA
                    && *good_types_reference_va == GOOD_TYPES_REFERENCE_VA
                    && *lands_ocean_rare_list_pointer_slot_va
                        == LANDS_OCEAN_RARE_LIST_POINTER_SLOT_VA
                    && *checkpoint_call_va == entry.checkpoint_call_va
                    && *source_token == entry.source_token
                    && *call_va == entry.call_va
                    && *entry_va == entry.entry_va
                    && world_checksum == &entry.world_checksum
                    && *sourced_walked_bytes == entry.sourced_walked_bytes
                    && *random_state_at_entry == entry.random_state_at_entry
            }
            PlaceResourcesFactEvidence::SyntheticFixture {
                fixture,
                checkpoint_call_va,
                source_token,
                call_va,
                entry_va,
                world_checksum,
                sourced_walked_bytes,
                random_state_at_entry,
            } => {
                cfg!(test)
                    && !fixture.is_empty()
                    && *checkpoint_call_va == entry.checkpoint_call_va
                    && *source_token == entry.source_token
                    && *call_va == entry.call_va
                    && *entry_va == entry.entry_va
                    && world_checksum == &entry.world_checksum
                    && *sourced_walked_bytes == entry.sourced_walked_bytes
                    && *random_state_at_entry == entry.random_state_at_entry
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceGoodDisposition {
    SkippedConquestExclusion,
    SkippedNotRandomRare,
    Early,
    Late,
    Water { copies: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceGoodDecision {
    pub scan_va: u32,
    pub good_id: i32,
    pub add_good_call_va: Option<u32>,
    pub add_good_callee_va: Option<u32>,
    pub disposition: ResourceGoodDisposition,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceResourcesPoolReceipt {
    pub upstream_checkpoint_call_va: u32,
    pub upstream_source_token: u32,
    pub unresolved_caller_gap_start_va: u32,
    pub call_va: u32,
    pub entry_va: u32,
    pub prefix_residual_va: u32,
    pub player_count_argument: i32,
    pub decisions: Vec<ResourceGoodDecision>,
    pub add_good_calls: usize,
    pub done_adding_call_va: u32,
    pub done_adding_callee_va: u32,
    pub bit_mask_init_va: u32,
    pub bit_mask_clear_passes: u8,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub random_draws: usize,
    pub random_get_va: u32,
    pub world_checksum_before: WorldChecksum,
    pub world_checksum_after: WorldChecksum,
    pub sourced_walked_bytes_before: u64,
    pub sourced_walked_bytes_after: u64,
    pub pool_after: ResourceDivvyPoolState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaceResourcesPoolError {
    EntryHandoffMismatch,
    LiveFactsUnavailableOrMismatched,
    GoodCatalogLength {
        observed: usize,
    },
    GoodCatalogId {
        ordinal: usize,
        expected: i32,
        observed: i32,
    },
    OceanRareListTooLong {
        observed: usize,
    },
}

/// Execute the exact no-RNG divvy-pool prefix and commit its Map-local state atomically.
pub fn execute_place_resources_pool_prefix(
    pool: &mut ResourceDivvyPoolState,
    entry: &PlaceResourcesEntryHandoff,
    facts: &PlaceResourcesLiveFacts,
) -> Result<PlaceResourcesPoolReceipt, PlaceResourcesPoolError> {
    if entry.checkpoint_call_va != MAP_POST_TRANSITIONS_CHECKPOINT_CALL_VA
        || entry.source_token != MAP_POST_TRANSITIONS_SOURCE_TOKEN
        || entry.call_va != MAP_PLACE_RESOURCES_CALL_VA
        || entry.entry_va != MAP_PLACE_RESOURCES_ENTRY_VA
    {
        return Err(PlaceResourcesPoolError::EntryHandoffMismatch);
    }
    if !facts.evidence_matches(entry) {
        return Err(PlaceResourcesPoolError::LiveFactsUnavailableOrMismatched);
    }
    if facts.goods.len() != SCANNED_GOOD_COUNT {
        return Err(PlaceResourcesPoolError::GoodCatalogLength {
            observed: facts.goods.len(),
        });
    }
    for (ordinal, good) in facts.goods.iter().enumerate() {
        let expected = FIRST_SCANNED_GOOD_ID + ordinal as i32;
        if good.good_id != expected {
            return Err(PlaceResourcesPoolError::GoodCatalogId {
                ordinal,
                expected,
                observed: good.good_id,
            });
        }
    }
    if facts.ocean_rare_good_ids.len() > LAND_RARE_CAPACITY {
        return Err(PlaceResourcesPoolError::OceanRareListTooLong {
            observed: facts.ocean_rare_good_ids.len(),
        });
    }

    // `place_resources` does not clear the three goods arrays. A fresh Map starts empty, but
    // repeated native invocation appends again; retain that behavior instead of normalizing it.
    let mut staged = pool.clone();
    let mut decisions = Vec::with_capacity(SCANNED_GOOD_COUNT);
    let mut add_good_calls = 0usize;

    for good in &facts.goods {
        let disposition = if facts.conquer_the_world && good.exclude_ctw != 0 {
            ResourceGoodDisposition::SkippedConquestExclusion
        } else if good.random_rare_drop == 0 {
            ResourceGoodDisposition::SkippedNotRandomRare
        } else {
            add_good_calls += 1;
            if facts.ocean_rare_good_ids.contains(&good.good_id) {
                let copies = if good.good_id == WEIGHTED_WATER_GOOD_ID {
                    WEIGHTED_WATER_GOOD_COPIES
                } else {
                    1
                };
                staged
                    .water_goods
                    .extend(std::iter::repeat(good.good_id).take(copies));
                ResourceGoodDisposition::Water { copies }
            } else if good.resolved_age < EARLY_AGE_END_EXCLUSIVE {
                staged.early_goods.push(good.good_id);
                ResourceGoodDisposition::Early
            } else {
                staged.late_goods.push(good.good_id);
                ResourceGoodDisposition::Late
            }
        };
        let accepted = !matches!(
            disposition,
            ResourceGoodDisposition::SkippedConquestExclusion
                | ResourceGoodDisposition::SkippedNotRandomRare
        );
        decisions.push(ResourceGoodDecision {
            scan_va: MAP_PLACE_RESOURCES_SCAN_VA,
            good_id: good.good_id,
            add_good_call_va: accepted.then_some(MAP_PLACE_RESOURCES_ADD_GOOD_CALL_VA),
            add_good_callee_va: accepted.then_some(RESOURCE_DIVVY_ADD_GOOD_VA),
            disposition,
        });
    }

    staged.early_bits = ResourcePoolBitMask::zeroed(staged.early_goods.len());
    staged.late_bits = ResourcePoolBitMask::zeroed(staged.late_goods.len());
    staged.water_bits = ResourcePoolBitMask::zeroed(staged.water_goods.len());

    let receipt = PlaceResourcesPoolReceipt {
        upstream_checkpoint_call_va: entry.checkpoint_call_va,
        upstream_source_token: entry.source_token,
        unresolved_caller_gap_start_va: MAP_POST_TRANSITIONS_CALLER_RESUME_VA,
        call_va: entry.call_va,
        entry_va: entry.entry_va,
        prefix_residual_va: MAP_PLACE_RESOURCES_PREFIX_RESIDUAL_VA,
        player_count_argument: entry.player_count_argument,
        decisions,
        add_good_calls,
        done_adding_call_va: MAP_PLACE_RESOURCES_DONE_ADDING_CALL_VA,
        done_adding_callee_va: RESOURCE_DIVVY_DONE_ADDING_GOODS_VA,
        bit_mask_init_va: DYNAMIC_BIT_MASK_INIT_VA,
        bit_mask_clear_passes: 2,
        random_state_before: entry.random_state_at_entry,
        random_state_after: entry.random_state_at_entry,
        random_draws: 0,
        random_get_va: RANDOM_GET_VA,
        world_checksum_before: entry.world_checksum.clone(),
        world_checksum_after: entry.world_checksum.clone(),
        sourced_walked_bytes_before: entry.sourced_walked_bytes,
        sourced_walked_bytes_after: entry.sourced_walked_bytes,
        pool_after: staged.clone(),
    };
    *pool = staged;
    Ok(receipt)
}
