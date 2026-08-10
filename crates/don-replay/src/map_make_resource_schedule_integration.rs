// SPDX-License-Identifier: GPL-3.0-or-later
//! Typed receipt integration from the post-nubify checksum through the deterministic
//! `Map::place_resources` pool and XML bootstrap.
//!
//! This adapter does not authenticate an in-memory `Map` projection. The caller supplies its
//! immutable object identities and capture digest, and the caller-gap evidence binds them to the
//! exact post-nubify World/RNG receipt. Pool and XML host mutations are represented by their
//! complete receipts; no stale whole-Map digest is claimed after mutation.

use crate::map_make_resource_caller_gap_frontier::{
    execute_map_make_resource_caller_gap, MapMakeResourceCallerFacts,
    MapMakeResourceCallerGapError, MapMakeResourceCallerGapReceipt,
    MapMakeResourceCallerPriorReceipt, PlaceResourcesCallDisposition,
    PlaceResourcesCallerEntryReceipt, CALLER_GAP_CHECKPOINTS, MAP_PLACE_RESOURCES_CALL_VA,
    MAP_PLACE_RESOURCES_ENTRY_VA, MAP_POST_RESOURCES_CHECKPOINT_CALL_VA,
    MAP_POST_RESOURCES_SOURCE_TOKEN, MAP_POST_TRANSITIONS_CHECKPOINT_CALL_VA,
    MAP_POST_TRANSITIONS_SOURCE_TOKEN, MAP_RESOURCE_CALLER_CONTINUATION_VA,
    MAP_RESOURCE_CALLER_GAP_RESUME_VA, MAP_RESOURCE_DIAGNOSTICS_BEGIN_VA,
    MAP_RESOURCE_DIAGNOSTIC_CHECKPOINT_CALL_VA, MAP_RESOURCE_DIAGNOSTIC_SOURCE_TOKEN,
    MAP_RESOURCE_GATE_BEGIN_VA, RANDOM_GET_VA as CALLER_RANDOM_GET_VA,
};
use crate::nubify_forest_frontier::MAP_NUBIFY_FOREST_CALLER_RESUME_VA;
use crate::place_resources_bonus_mutation_frontier::{
    execute_first_bonus_mutation, FirstBonusMutationError, FirstBonusMutationFacts,
    FirstBonusMutationReceipt, PlaceResourcesBonusMutationState, PlacementHost, PlacementReceipt,
};
use crate::place_resources_bonus_rows_mutation_frontier::{
    execute_next_bonus_mutation, LaterBonusMutationFacts, LaterBonusMutationReceipt,
    RemainingBonusRowsError, RemainingBonusRowsState, BONUS_CATEGORY_TAIL_VA, LATER_BONUS_ENTRY_VA,
    ROW_COUNT_DECREMENT_VA, ROW_POINTER_STRIDE, ROW_RECURRENCE_BRANCH_VA,
};
use crate::place_resources_pool_frontier::{
    execute_place_resources_pool_prefix, resource_divvy_pool_digest, PlaceResourcesEntryHandoff,
    PlaceResourcesLiveFacts, PlaceResourcesPoolError, PlaceResourcesPoolReceipt,
    ResourceDivvyPoolState, DYNAMIC_BIT_MASK_INIT_VA, MAP_PLACE_RESOURCES_ADD_GOOD_CALL_VA,
    MAP_PLACE_RESOURCES_DONE_ADDING_CALL_VA, MAP_PLACE_RESOURCES_PREFIX_RESIDUAL_VA,
    MAP_PLACE_RESOURCES_SCAN_VA, RANDOM_GET_VA as POOL_RANDOM_GET_VA, RESOURCE_DIVVY_ADD_GOOD_VA,
    RESOURCE_DIVVY_DONE_ADDING_GOODS_VA, SCANNED_GOOD_COUNT,
};
use crate::place_resources_xml_frontier::{
    execute_place_resources_xml_frontier, PlaceResourcesXmlEntryHandoff, PlaceResourcesXmlError,
    PlaceResourcesXmlFacts, PlaceResourcesXmlOutcome, XmlHostHandles,
    PLACE_RESOURCES_XML_RESIDUAL_VA,
};
use crate::post_nubify_transition_frontier::{
    PostNubifyTransitionReceipt, MAP_POST_NUBIFY_CHECKPOINT_CALL_VA,
    MAP_POST_NUBIFY_CHECKPOINT_END_VA, MAP_POST_NUBIFY_CHECKPOINT_RESUME_VA,
    MAP_POST_NUBIFY_SOURCE_TOKEN,
};
use don_sim::systems::map_terrain::WorldChecksum;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapMakeResourceOwnerProvenance {
    pub map_object_identity: u64,
    pub world_object_identity: u64,
    pub game_object_identity: u64,
    pub conquest_game_identity: u64,
    /// Immutable digest of the authoritative `Map` projection at token `0x1ebe`.
    pub map_state_sha256_at_0x1ebe: [u8; 32],
    pub player_count_argument: i32,
    pub progress_requested: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceResourcesSkippedBoundary {
    pub disposition: PlaceResourcesCallDisposition,
    pub continuation_va: u32,
    pub next_checkpoint_call_va: u32,
    pub next_source_token: u32,
    pub map_object_identity: u64,
    pub world_object_identity: u64,
    pub map_state_sha256: [u8; 32],
    pub world_checksum: WorldChecksum,
    pub sourced_walked_bytes: u64,
    pub random_state: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceResourcesEntryBoundary {
    pub entry: PlaceResourcesCallerEntryReceipt,
    pub reason: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceResourcesBodyBoundary {
    /// First unowned instruction after `ResourceDivvyPool::done_adding_goods`.
    pub residual_va: u32,
    pub caller_continuation_va: u32,
    /// The caller checkpoint is chronological evidence only; the open body has not reached it.
    pub pending_checkpoint_call_va: u32,
    pub pending_source_token: u32,
    pub map_object_identity: u64,
    pub world_object_identity: u64,
    /// Digest at entry. The exact `pool_prefix.pool_after` delta supersedes this projection for the
    /// mutated `Map::resource_pool`; no post-mutation whole-Map digest is invented.
    pub map_state_sha256_at_entry: [u8; 32],
    pub world_checksum: WorldChecksum,
    pub sourced_walked_bytes: u64,
    pub random_state: i32,
    pub pool_prefix: PlaceResourcesPoolReceipt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceResourcesXmlBoundary {
    /// Exact first unowned instruction after the typed XML/bootstrap owner.
    pub residual_va: u32,
    pub caller_continuation_va: u32,
    /// The caller checkpoint is chronological evidence only; the open body has not reached it.
    pub pending_checkpoint_call_va: u32,
    pub pending_source_token: u32,
    pub map_object_identity: u64,
    pub world_object_identity: u64,
    /// Immutable digest at `Map::place_resources` entry. Pool mutation is owned separately by
    /// `pool_prefix` and its canonical logical digest in `xml_entry`.
    pub map_state_sha256_at_entry: [u8; 32],
    pub pool_prefix: PlaceResourcesPoolReceipt,
    pub xml_entry: PlaceResourcesXmlEntryHandoff,
    pub xml_frontier: PlaceResourcesXmlOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceResourcesFirstBonusBoundary {
    /// The XML boundary whose typed row handoff this transaction consumed.
    pub xml: PlaceResourcesXmlBoundary,
    /// Stops before the `add esi, 0x28` recurrence at `0x00690215`.
    pub first_bonus: FirstBonusMutationReceipt,
    /// Authoritative concrete public-pool projection after the placement transaction.
    pub resource_pool_after: ResourceDivvyPoolState,
    pub pending_checkpoint_call_va: u32,
    pub pending_source_token: u32,
}

pub const BONUS_CATEGORY_TAIL_RESTORE_VA: u32 = 0x0069_0222;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaterBonusScheduleStep {
    /// Complete behavior-driving fact projection retained for deterministic history replay.
    pub facts: LaterBonusMutationFacts,
    pub receipt: LaterBonusMutationReceipt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BonusCategoryTailAdvanceReceipt {
    pub entry_va: u32,
    pub row_pointer_stride: u32,
    pub row_count_decrement_va: u32,
    pub recurrence_branch_va: u32,
    pub row_count_before: usize,
    pub row_count_after: usize,
    pub branch_taken: bool,
    pub restore_category_pointer_va: u32,
    pub residual_va: u32,
    pub random_state: i32,
    pub world_checksum: WorldChecksum,
    pub sourced_walked_bytes: u64,
    pub resource_pool_digest: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceResourcesBonusRowsBoundary {
    pub first: PlaceResourcesFirstBonusBoundary,
    /// Exact later-row history, replayed before every new public continuation.
    pub steps: Vec<LaterBonusScheduleStep>,
    pub remaining_state: RemainingBonusRowsState,
    pub resource_pool_after: ResourceDivvyPoolState,
    /// `Some` only after the final `0x00690215` recurrence falls through to `0x00690225`.
    pub category_tail: Option<BonusCategoryTailAdvanceReceipt>,
    pub residual_va: u32,
    pub pending_checkpoint_call_va: u32,
    pub pending_source_token: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapMakeResourcePlacementReceipt {
    Skipped(PlaceResourcesSkippedBoundary),
    EntryOpen(PlaceResourcesEntryBoundary),
    BodyOpen(PlaceResourcesBodyBoundary),
    XmlRowsOpen(PlaceResourcesXmlBoundary),
    FirstBonusRowOpen(PlaceResourcesFirstBonusBoundary),
    BonusRowsOpen(PlaceResourcesBonusRowsBoundary),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapMakeResourceScheduleReceipt {
    pub post_nubify_checkpoint: MapMakeResourceCallerPriorReceipt,
    pub caller_gap: MapMakeResourceCallerGapReceipt,
    pub placement: MapMakeResourcePlacementReceipt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapMakeResourceScheduleError {
    PostNubifyReceiptMismatch,
    EmptyMapStateDigest,
    CallerGap(MapMakeResourceCallerGapError),
    CallerEntryContinuityMismatch,
    PoolPrefix(PlaceResourcesPoolError),
    XmlFactsWithoutPoolPrefix,
    XmlFrontier(PlaceResourcesXmlError),
    XmlContinuityMismatch,
    BonusContinuationRequiresXmlRows,
    BonusFrontier(FirstBonusMutationError),
    BonusContinuityMismatch,
    BonusRowsContinuationRequiresFirstOrLaterBoundary,
    BonusRowsFrontier(RemainingBonusRowsError),
    BonusRowsContinuityMismatch,
    BonusCategoryTailRequiresCompletedRows,
    BonusRowsComplete,
}

fn post_nubify_receipt_matches(receipt: &PostNubifyTransitionReceipt) -> bool {
    let expected_after = receipt
        .random_draws
        .last()
        .map(|draw| draw.state_after)
        .unwrap_or(receipt.random_state_before);
    let draw_chain_matches = receipt
        .random_draws
        .first()
        .map(|draw| draw.state_before == receipt.random_state_before)
        .unwrap_or(receipt.random_state_before == receipt.random_state_after)
        && receipt
            .random_draws
            .windows(2)
            .all(|pair| pair[0].state_after == pair[1].state_before)
        && expected_after == receipt.random_state_after;

    receipt.caller_resume_va == MAP_NUBIFY_FOREST_CALLER_RESUME_VA
        && receipt.checkpoint_call_va == MAP_POST_NUBIFY_CHECKPOINT_CALL_VA
        && receipt.checkpoint_resume_va == MAP_POST_NUBIFY_CHECKPOINT_RESUME_VA
        && receipt.checkpoint_end_va == MAP_POST_NUBIFY_CHECKPOINT_END_VA
        && receipt.source_token == MAP_POST_NUBIFY_SOURCE_TOKEN
        && receipt.next_checkpoint_call_va == MAP_POST_TRANSITIONS_CHECKPOINT_CALL_VA
        && receipt.next_source_token == MAP_POST_TRANSITIONS_SOURCE_TOKEN
        && draw_chain_matches
}

fn pool_entry(entry: &PlaceResourcesCallerEntryReceipt) -> PlaceResourcesEntryHandoff {
    PlaceResourcesEntryHandoff {
        checkpoint_call_va: entry.upstream_checkpoint_call_va,
        source_token: entry.upstream_source_token,
        world_checksum: entry.world_checksum.clone(),
        sourced_walked_bytes: entry.sourced_walked_bytes,
        call_va: entry.call_va,
        entry_va: entry.entry_va,
        player_count_argument: entry.player_count_argument,
        random_state_at_entry: entry.random_state_at_entry,
    }
}

fn caller_entry_matches(
    entry: &PlaceResourcesCallerEntryReceipt,
    prior: &MapMakeResourceCallerPriorReceipt,
) -> bool {
    entry.upstream_checkpoint_call_va == prior.checkpoint_call_va
        && entry.upstream_source_token == prior.source_token
        && entry.latest_checkpoint_call_va == MAP_RESOURCE_DIAGNOSTIC_CHECKPOINT_CALL_VA
        && entry.latest_source_token == MAP_RESOURCE_DIAGNOSTIC_SOURCE_TOKEN
        && entry.call_va == MAP_PLACE_RESOURCES_CALL_VA
        && entry.entry_va == MAP_PLACE_RESOURCES_ENTRY_VA
        && entry.player_count_argument == prior.player_count_argument
        && entry.map_object_identity == prior.map_object_identity
        && entry.world_object_identity == prior.world_object_identity
        && entry.game_object_identity == prior.game_object_identity
        && entry.map_state_sha256 == prior.map_state_sha256
        && entry.world_checksum == prior.world_checksum
        && entry.sourced_walked_bytes == prior.sourced_walked_bytes
        && entry.random_state_at_entry == prior.random_state
}

fn xml_outcome_matches(
    entry: &PlaceResourcesXmlEntryHandoff,
    outcome: &PlaceResourcesXmlOutcome,
    final_host: XmlHostHandles,
) -> bool {
    let receipt = &outcome.receipt;
    let handoff = &outcome.handoff;
    receipt.entry_va == entry.entry_va
        && receipt.residual_va == PLACE_RESOURCES_XML_RESIDUAL_VA
        && receipt.random_state_before == entry.random_state
        && receipt.random_state_after == entry.random_state
        && receipt.random_draws == 0
        && receipt.world_checksum_before == entry.world_checksum
        && receipt.world_checksum_after == entry.world_checksum
        && receipt.sourced_walked_bytes_before == entry.sourced_walked_bytes
        && receipt.sourced_walked_bytes_after == entry.sourced_walked_bytes
        && receipt.resource_pool_digest_before == entry.resource_pool_digest
        && receipt.resource_pool_digest_after == entry.resource_pool_digest
        && receipt.world_mutations == 0
        && handoff.resume_va == PLACE_RESOURCES_XML_RESIDUAL_VA
        && handoff.player_count_argument == entry.player_count_argument
        && handoff.current_category_handles == final_host
        && handoff.random_state == entry.random_state
        && handoff.world_checksum == entry.world_checksum
        && handoff.sourced_walked_bytes == entry.sourced_walked_bytes
        && handoff.resource_pool_digest == entry.resource_pool_digest
}

/// Revalidate the state-carrying prefix needed by a BONUS continuation.
///
/// Capture-only descriptive fields remain provenance retained from the original admitted
/// schedule; this check binds the caller, pool, and XML RNG/World/pool chronology that drives the
/// row transaction.
fn bonus_prefix_matches(
    schedule: &MapMakeResourceScheduleReceipt,
    xml: &PlaceResourcesXmlBoundary,
) -> bool {
    let prior = &schedule.post_nubify_checkpoint;
    let gap = &schedule.caller_gap;
    let Some(caller_entry) = gap.place_resources_entry.as_ref() else {
        return false;
    };
    let gap_matches = prior.checkpoint_call_va == MAP_POST_TRANSITIONS_CHECKPOINT_CALL_VA
        && prior.source_token == MAP_POST_TRANSITIONS_SOURCE_TOKEN
        && prior.resume_va == MAP_RESOURCE_CALLER_GAP_RESUME_VA
        && prior.map_state_sha256.iter().any(|byte| *byte != 0)
        && gap.resume_va == MAP_RESOURCE_CALLER_GAP_RESUME_VA
        && gap.checkpoints == CALLER_GAP_CHECKPOINTS
        && gap.diagnostics_begin_va == MAP_RESOURCE_DIAGNOSTICS_BEGIN_VA
        && gap.diagnostic_log_calls >= 4_usize.saturating_add(gap.player_diagnostic_iterations)
        && gap.diagnostic_log_calls <= 5_usize.saturating_add(gap.player_diagnostic_iterations)
        && gap.progress_refresh_calls == usize::from(prior.progress_requested) * 2
        && gap.gate_begin_va == MAP_RESOURCE_GATE_BEGIN_VA
        && gap.disposition == PlaceResourcesCallDisposition::Called
        && caller_entry_matches(caller_entry, prior)
        && gap.continuation_va == MAP_RESOURCE_CALLER_CONTINUATION_VA
        && gap.post_resources_checkpoint_call_va == MAP_POST_RESOURCES_CHECKPOINT_CALL_VA
        && gap.post_resources_source_token == MAP_POST_RESOURCES_SOURCE_TOKEN
        && gap.random_get_va == CALLER_RANDOM_GET_VA
        && gap.random_draws == 0
        && gap.random_state_before == prior.random_state
        && gap.random_state_after == prior.random_state
        && gap.map_state_sha256_before == prior.map_state_sha256
        && gap.map_state_sha256_after == prior.map_state_sha256
        && gap.world_checksum_before == prior.world_checksum
        && gap.world_checksum_after == prior.world_checksum
        && gap.sourced_walked_bytes_before == prior.sourced_walked_bytes
        && gap.sourced_walked_bytes_after == prior.sourced_walked_bytes;

    let prefix = &xml.pool_prefix;
    let prefix_matches = prefix.upstream_checkpoint_call_va == prior.checkpoint_call_va
        && prefix.upstream_source_token == prior.source_token
        && prefix.unresolved_caller_gap_start_va == MAP_RESOURCE_CALLER_GAP_RESUME_VA
        && prefix.call_va == MAP_PLACE_RESOURCES_CALL_VA
        && prefix.entry_va == MAP_PLACE_RESOURCES_ENTRY_VA
        && prefix.prefix_residual_va == MAP_PLACE_RESOURCES_PREFIX_RESIDUAL_VA
        && prefix.player_count_argument == prior.player_count_argument
        && prefix.decisions.len() == SCANNED_GOOD_COUNT
        && prefix.decisions.iter().all(|decision| {
            decision.scan_va == MAP_PLACE_RESOURCES_SCAN_VA
                && match (decision.add_good_call_va, decision.add_good_callee_va) {
                    (Some(call), Some(callee)) => {
                        call == MAP_PLACE_RESOURCES_ADD_GOOD_CALL_VA
                            && callee == RESOURCE_DIVVY_ADD_GOOD_VA
                    }
                    (None, None) => true,
                    _ => false,
                }
        })
        && prefix.add_good_calls
            == prefix
                .decisions
                .iter()
                .filter(|decision| decision.add_good_call_va.is_some())
                .count()
        && prefix.done_adding_call_va == MAP_PLACE_RESOURCES_DONE_ADDING_CALL_VA
        && prefix.done_adding_callee_va == RESOURCE_DIVVY_DONE_ADDING_GOODS_VA
        && prefix.bit_mask_init_va == DYNAMIC_BIT_MASK_INIT_VA
        && prefix.bit_mask_clear_passes == 2
        && prefix.random_state_before == prior.random_state
        && prefix.random_state_after == prior.random_state
        && prefix.random_draws == 0
        && prefix.random_get_va == POOL_RANDOM_GET_VA
        && prefix.world_checksum_before == prior.world_checksum
        && prefix.world_checksum_after == prior.world_checksum
        && prefix.sourced_walked_bytes_before == prior.sourced_walked_bytes
        && prefix.sourced_walked_bytes_after == prior.sourced_walked_bytes;

    let xml_entry = &xml.xml_entry;
    let xml_matches = xml.residual_va == PLACE_RESOURCES_XML_RESIDUAL_VA
        && xml.caller_continuation_va == MAP_RESOURCE_CALLER_CONTINUATION_VA
        && xml.pending_checkpoint_call_va == MAP_POST_RESOURCES_CHECKPOINT_CALL_VA
        && xml.pending_source_token == MAP_POST_RESOURCES_SOURCE_TOKEN
        && xml.map_object_identity == prior.map_object_identity
        && xml.world_object_identity == prior.world_object_identity
        && xml.map_state_sha256_at_entry == prior.map_state_sha256
        && xml_entry.entry_va == prefix.prefix_residual_va
        && xml_entry.player_count_argument == prefix.player_count_argument
        && xml_entry.random_state == prefix.random_state_after
        && xml_entry.world_checksum == prefix.world_checksum_after
        && xml_entry.sourced_walked_bytes == prefix.sourced_walked_bytes_after
        && xml_entry.resource_pool_digest == resource_divvy_pool_digest(&prefix.pool_after)
        && xml.xml_frontier.receipt.section_source == xml.xml_frontier.handoff.section_source
        && xml.xml_frontier.receipt.row_count == xml.xml_frontier.handoff.rows.len()
        && xml_outcome_matches(
            xml_entry,
            &xml.xml_frontier,
            xml.xml_frontier.handoff.current_category_handles,
        );

    gap_matches && prefix_matches && xml_matches
}

/// Advance the exact `Map::make` receipt chain through the no-RNG caller gap and, when live
/// resource facts are available, the deterministic resource-pool prefix.
///
/// `None` for `pool_facts` is a successful typed stop at the admitted function entry. A pool
/// validation failure returns an error without committing any pool mutation.
pub fn execute_map_make_resource_schedule(
    pool: &mut ResourceDivvyPoolState,
    post_nubify: &PostNubifyTransitionReceipt,
    owner: &MapMakeResourceOwnerProvenance,
    caller_facts: &MapMakeResourceCallerFacts,
    pool_facts: Option<&PlaceResourcesLiveFacts>,
) -> Result<MapMakeResourceScheduleReceipt, MapMakeResourceScheduleError> {
    let mut detached_xml_host = XmlHostHandles::default();
    execute_map_make_resource_schedule_with_xml(
        pool,
        &mut detached_xml_host,
        post_nubify,
        owner,
        caller_facts,
        pool_facts,
        None,
    )
}

/// Advance the same receipt chain through the typed XML bootstrap at `0x0068fb9d`.
///
/// The pool projection and XML host references are staged together. A stale XML capture or any
/// seam-continuity failure commits neither mutation. `xml_facts` may only be supplied with live
/// pool facts because its evidence is bound to the canonical digest of the resulting six-field
/// pool projection.
pub fn execute_map_make_resource_schedule_with_xml(
    pool: &mut ResourceDivvyPoolState,
    xml_host: &mut XmlHostHandles,
    post_nubify: &PostNubifyTransitionReceipt,
    owner: &MapMakeResourceOwnerProvenance,
    caller_facts: &MapMakeResourceCallerFacts,
    pool_facts: Option<&PlaceResourcesLiveFacts>,
    xml_facts: Option<&PlaceResourcesXmlFacts>,
) -> Result<MapMakeResourceScheduleReceipt, MapMakeResourceScheduleError> {
    if !post_nubify_receipt_matches(post_nubify) {
        return Err(MapMakeResourceScheduleError::PostNubifyReceiptMismatch);
    }
    if owner
        .map_state_sha256_at_0x1ebe
        .iter()
        .all(|byte| *byte == 0)
    {
        return Err(MapMakeResourceScheduleError::EmptyMapStateDigest);
    }
    if xml_facts.is_some() && pool_facts.is_none() {
        return Err(MapMakeResourceScheduleError::XmlFactsWithoutPoolPrefix);
    }

    let prior = MapMakeResourceCallerPriorReceipt {
        checkpoint_call_va: post_nubify.next_checkpoint_call_va,
        source_token: post_nubify.next_source_token,
        resume_va: MAP_RESOURCE_CALLER_GAP_RESUME_VA,
        map_object_identity: owner.map_object_identity,
        world_object_identity: owner.world_object_identity,
        game_object_identity: owner.game_object_identity,
        conquest_game_identity: owner.conquest_game_identity,
        map_state_sha256: owner.map_state_sha256_at_0x1ebe,
        world_checksum: post_nubify.checksum_after.clone(),
        sourced_walked_bytes: post_nubify.sourced_walked_bytes,
        random_state: post_nubify.random_state_after,
        player_count_argument: owner.player_count_argument,
        progress_requested: owner.progress_requested,
    };
    let caller_gap = execute_map_make_resource_caller_gap(&prior, caller_facts)
        .map_err(MapMakeResourceScheduleError::CallerGap)?;

    let placement = match caller_gap.disposition {
        PlaceResourcesCallDisposition::Called => {
            let entry = caller_gap
                .place_resources_entry
                .as_ref()
                .ok_or(MapMakeResourceScheduleError::CallerEntryContinuityMismatch)?;
            if !caller_entry_matches(entry, &prior) {
                return Err(MapMakeResourceScheduleError::CallerEntryContinuityMismatch);
            }
            match pool_facts {
                None => MapMakeResourcePlacementReceipt::EntryOpen(PlaceResourcesEntryBoundary {
                    entry: entry.clone(),
                    reason: "resource-pool live facts unavailable",
                }),
                Some(facts) => {
                    let handoff = pool_entry(entry);
                    let mut staged_pool = pool.clone();
                    let pool_prefix =
                        execute_place_resources_pool_prefix(&mut staged_pool, &handoff, facts)
                            .map_err(MapMakeResourceScheduleError::PoolPrefix)?;
                    if pool_prefix.random_state_after != prior.random_state
                        || pool_prefix.world_checksum_after != prior.world_checksum
                        || pool_prefix.sourced_walked_bytes_after != prior.sourced_walked_bytes
                    {
                        return Err(MapMakeResourceScheduleError::CallerEntryContinuityMismatch);
                    }
                    match xml_facts {
                        None => {
                            *pool = staged_pool;
                            MapMakeResourcePlacementReceipt::BodyOpen(PlaceResourcesBodyBoundary {
                                residual_va: MAP_PLACE_RESOURCES_PREFIX_RESIDUAL_VA,
                                caller_continuation_va: MAP_RESOURCE_CALLER_CONTINUATION_VA,
                                pending_checkpoint_call_va: MAP_POST_RESOURCES_CHECKPOINT_CALL_VA,
                                pending_source_token: MAP_POST_RESOURCES_SOURCE_TOKEN,
                                map_object_identity: prior.map_object_identity,
                                world_object_identity: prior.world_object_identity,
                                map_state_sha256_at_entry: prior.map_state_sha256,
                                world_checksum: prior.world_checksum.clone(),
                                sourced_walked_bytes: prior.sourced_walked_bytes,
                                random_state: prior.random_state,
                                pool_prefix,
                            })
                        }
                        Some(xml_facts) => {
                            let xml_entry = PlaceResourcesXmlEntryHandoff {
                                entry_va: pool_prefix.prefix_residual_va,
                                player_count_argument: pool_prefix.player_count_argument,
                                random_state: pool_prefix.random_state_after,
                                world_checksum: pool_prefix.world_checksum_after.clone(),
                                sourced_walked_bytes: pool_prefix.sourced_walked_bytes_after,
                                resource_pool_digest: resource_divvy_pool_digest(
                                    &pool_prefix.pool_after,
                                ),
                            };
                            let mut staged_xml_host = *xml_host;
                            let xml_frontier = execute_place_resources_xml_frontier(
                                &mut staged_xml_host,
                                &xml_entry,
                                xml_facts,
                            )
                            .map_err(MapMakeResourceScheduleError::XmlFrontier)?;
                            if staged_pool != pool_prefix.pool_after
                                || !xml_outcome_matches(&xml_entry, &xml_frontier, staged_xml_host)
                            {
                                return Err(MapMakeResourceScheduleError::XmlContinuityMismatch);
                            }
                            *pool = staged_pool;
                            *xml_host = staged_xml_host;
                            MapMakeResourcePlacementReceipt::XmlRowsOpen(
                                PlaceResourcesXmlBoundary {
                                    residual_va: PLACE_RESOURCES_XML_RESIDUAL_VA,
                                    caller_continuation_va: MAP_RESOURCE_CALLER_CONTINUATION_VA,
                                    pending_checkpoint_call_va:
                                        MAP_POST_RESOURCES_CHECKPOINT_CALL_VA,
                                    pending_source_token: MAP_POST_RESOURCES_SOURCE_TOKEN,
                                    map_object_identity: prior.map_object_identity,
                                    world_object_identity: prior.world_object_identity,
                                    map_state_sha256_at_entry: prior.map_state_sha256,
                                    pool_prefix,
                                    xml_entry,
                                    xml_frontier,
                                },
                            )
                        }
                    }
                }
            }
        }
        disposition => {
            if caller_gap.place_resources_entry.is_some() {
                return Err(MapMakeResourceScheduleError::CallerEntryContinuityMismatch);
            }
            if xml_facts.is_some() {
                return Err(MapMakeResourceScheduleError::XmlFactsWithoutPoolPrefix);
            }
            MapMakeResourcePlacementReceipt::Skipped(PlaceResourcesSkippedBoundary {
                disposition,
                continuation_va: MAP_RESOURCE_CALLER_CONTINUATION_VA,
                next_checkpoint_call_va: MAP_POST_RESOURCES_CHECKPOINT_CALL_VA,
                next_source_token: MAP_POST_RESOURCES_SOURCE_TOKEN,
                map_object_identity: prior.map_object_identity,
                world_object_identity: prior.world_object_identity,
                map_state_sha256: prior.map_state_sha256,
                world_checksum: prior.world_checksum.clone(),
                sourced_walked_bytes: prior.sourced_walked_bytes,
                random_state: prior.random_state,
            })
        }
    };

    Ok(MapMakeResourceScheduleReceipt {
        post_nubify_checkpoint: prior,
        caller_gap,
        placement,
    })
}

/// Continue an admitted XML-row schedule receipt through exactly the first `BONUS` row.
///
/// The placement host remains two-phase, but its receipt must now carry exact selector
/// subreceipts and the concrete post-call pool projection. The caller's public pool is committed
/// only after both the first-row receipt and concrete digest continuity validate. The caller
/// checkpoint/token remain pending because later rows and category cleanup are still open.
pub fn continue_map_make_resource_schedule_first_bonus<H: PlacementHost>(
    pool: &mut ResourceDivvyPoolState,
    schedule: &MapMakeResourceScheduleReceipt,
    facts: &FirstBonusMutationFacts,
    host: &mut H,
) -> Result<MapMakeResourceScheduleReceipt, MapMakeResourceScheduleError> {
    let MapMakeResourcePlacementReceipt::XmlRowsOpen(xml) = &schedule.placement else {
        return Err(MapMakeResourceScheduleError::BonusContinuationRequiresXmlRows);
    };
    if !bonus_prefix_matches(schedule, xml)
        || pool != &xml.pool_prefix.pool_after
        || resource_divvy_pool_digest(pool) != xml.xml_frontier.handoff.resource_pool_digest
    {
        return Err(MapMakeResourceScheduleError::BonusContinuityMismatch);
    }

    let mut mutation =
        PlaceResourcesBonusMutationState::from_handoff_with_pool(&xml.xml_frontier.handoff, pool)
            .ok_or(MapMakeResourceScheduleError::BonusContinuityMismatch)?;
    let first_bonus =
        execute_first_bonus_mutation(&mut mutation, &xml.xml_frontier.handoff, facts, host)
            .map_err(MapMakeResourceScheduleError::BonusFrontier)?;
    let resource_pool_after = mutation
        .resource_pool
        .clone()
        .ok_or(MapMakeResourceScheduleError::BonusContinuityMismatch)?;
    if resource_divvy_pool_digest(&resource_pool_after) != mutation.resource_pool_digest
        || first_bonus.resource_pool_digest_after != mutation.resource_pool_digest
        || first_bonus.random_state_after != mutation.random_state
        || first_bonus.world_checksum_after != mutation.world_checksum
        || first_bonus.allocated_resources_after != mutation.allocated_resources
        || first_bonus.requested_resources_after != mutation.requested_resources
    {
        return Err(MapMakeResourceScheduleError::BonusContinuityMismatch);
    }

    *pool = resource_pool_after.clone();
    Ok(MapMakeResourceScheduleReceipt {
        post_nubify_checkpoint: schedule.post_nubify_checkpoint.clone(),
        caller_gap: schedule.caller_gap.clone(),
        placement: MapMakeResourcePlacementReceipt::FirstBonusRowOpen(
            PlaceResourcesFirstBonusBoundary {
                xml: xml.clone(),
                first_bonus,
                resource_pool_after,
                pending_checkpoint_call_va: MAP_POST_RESOURCES_CHECKPOINT_CALL_VA,
                pending_source_token: MAP_POST_RESOURCES_SOURCE_TOKEN,
            },
        ),
    })
}

#[derive(Clone)]
struct PlacementReceiptPlayback {
    receipt: Option<PlacementReceipt>,
}

impl PlacementHost for PlacementReceiptPlayback {
    fn place(
        &mut self,
        _request: &crate::place_resources_bonus_mutation_frontier::PlacementRequest,
    ) -> Option<PlacementReceipt> {
        self.receipt.take()
    }
}

fn rebuild_remaining_bonus_state(
    first: &PlaceResourcesFirstBonusBoundary,
    steps: &[LaterBonusScheduleStep],
) -> Result<RemainingBonusRowsState, MapMakeResourceScheduleError> {
    if first.pending_checkpoint_call_va != MAP_POST_RESOURCES_CHECKPOINT_CALL_VA
        || first.pending_source_token != MAP_POST_RESOURCES_SOURCE_TOKEN
    {
        return Err(MapMakeResourceScheduleError::BonusRowsContinuityMismatch);
    }
    let entry = &first.xml.xml_frontier.handoff;
    let mut mutation = PlaceResourcesBonusMutationState::from_handoff_with_pool(
        entry,
        &first.xml.pool_prefix.pool_after,
    )
    .ok_or(MapMakeResourceScheduleError::BonusRowsContinuityMismatch)?;
    let mut first_playback = PlacementReceiptPlayback {
        receipt: first.first_bonus.placement.clone(),
    };
    let rebuilt_first = execute_first_bonus_mutation(
        &mut mutation,
        entry,
        &first.first_bonus.facts,
        &mut first_playback,
    )
    .map_err(|_| MapMakeResourceScheduleError::BonusRowsContinuityMismatch)?;
    if rebuilt_first != first.first_bonus
        || mutation.resource_pool.as_ref() != Some(&first.resource_pool_after)
        || resource_divvy_pool_digest(&first.resource_pool_after) != mutation.resource_pool_digest
    {
        return Err(MapMakeResourceScheduleError::BonusRowsContinuityMismatch);
    }

    let mut state = RemainingBonusRowsState::from_first_row(
        entry,
        &first.first_bonus.facts,
        &first.first_bonus,
        &mutation,
    )
    .map_err(|_| MapMakeResourceScheduleError::BonusRowsContinuityMismatch)?;
    for step in steps {
        let mut playback = PlacementReceiptPlayback {
            receipt: step.receipt.placement.clone(),
        };
        let rebuilt = execute_next_bonus_mutation(&mut state, entry, &step.facts, &mut playback)
            .map_err(|_| MapMakeResourceScheduleError::BonusRowsContinuityMismatch)?;
        if rebuilt != step.receipt {
            return Err(MapMakeResourceScheduleError::BonusRowsContinuityMismatch);
        }
    }
    Ok(state)
}

fn category_tail_receipt(state: &RemainingBonusRowsState) -> BonusCategoryTailAdvanceReceipt {
    BonusCategoryTailAdvanceReceipt {
        entry_va: LATER_BONUS_ENTRY_VA,
        row_pointer_stride: ROW_POINTER_STRIDE,
        row_count_decrement_va: ROW_COUNT_DECREMENT_VA,
        recurrence_branch_va: ROW_RECURRENCE_BRANCH_VA,
        row_count_before: 1,
        row_count_after: 0,
        branch_taken: false,
        restore_category_pointer_va: BONUS_CATEGORY_TAIL_RESTORE_VA,
        residual_va: BONUS_CATEGORY_TAIL_VA,
        random_state: state.mutation.random_state,
        world_checksum: state.mutation.world_checksum.clone(),
        sourced_walked_bytes: state.mutation.sourced_walked_bytes,
        resource_pool_digest: state.mutation.resource_pool_digest,
    }
}

/// Advance a first-row boundary for a singleton `BONUSES` array through the final no-RNG,
/// no-World recurrence fallthrough at `0x00690215..0x00690225`.
pub fn continue_map_make_resource_schedule_bonus_category_tail(
    pool: &mut ResourceDivvyPoolState,
    schedule: &MapMakeResourceScheduleReceipt,
) -> Result<MapMakeResourceScheduleReceipt, MapMakeResourceScheduleError> {
    let MapMakeResourcePlacementReceipt::FirstBonusRowOpen(first) = &schedule.placement else {
        return Err(
            MapMakeResourceScheduleError::BonusRowsContinuationRequiresFirstOrLaterBoundary,
        );
    };
    if !bonus_prefix_matches(schedule, &first.xml) {
        return Err(MapMakeResourceScheduleError::BonusRowsContinuityMismatch);
    }
    let state = rebuild_remaining_bonus_state(first, &[])?;
    if state.next_row_index != first.xml.xml_frontier.handoff.rows.len() {
        return Err(MapMakeResourceScheduleError::BonusCategoryTailRequiresCompletedRows);
    }
    let resource_pool_after = state
        .mutation
        .resource_pool
        .clone()
        .ok_or(MapMakeResourceScheduleError::BonusRowsContinuityMismatch)?;
    if pool != &resource_pool_after
        || resource_divvy_pool_digest(pool) != state.mutation.resource_pool_digest
    {
        return Err(MapMakeResourceScheduleError::BonusRowsContinuityMismatch);
    }
    let category_tail = category_tail_receipt(&state);
    *pool = resource_pool_after.clone();
    Ok(MapMakeResourceScheduleReceipt {
        post_nubify_checkpoint: schedule.post_nubify_checkpoint.clone(),
        caller_gap: schedule.caller_gap.clone(),
        placement: MapMakeResourcePlacementReceipt::BonusRowsOpen(
            PlaceResourcesBonusRowsBoundary {
                first: first.clone(),
                steps: Vec::new(),
                remaining_state: state,
                resource_pool_after,
                residual_va: category_tail.residual_va,
                category_tail: Some(category_tail),
                pending_checkpoint_call_va: MAP_POST_RESOURCES_CHECKPOINT_CALL_VA,
                pending_source_token: MAP_POST_RESOURCES_SOURCE_TOKEN,
            },
        ),
    })
}

/// Execute one exact later `BONUS` row and retain enough history to revalidate the complete
/// chance/RNG/World/pool carry before every subsequent public continuation.
///
/// When the final row completes, this also owns the no-RNG/no-World recurrence tail
/// `0x00690215..0x00690225`. Category cleanup beginning at `0x00690225` remains open, so the
/// caller checkpoint and token `0x1ef7` stay pending.
pub fn continue_map_make_resource_schedule_next_bonus<H: PlacementHost>(
    pool: &mut ResourceDivvyPoolState,
    schedule: &MapMakeResourceScheduleReceipt,
    facts: &LaterBonusMutationFacts,
    host: &mut H,
) -> Result<MapMakeResourceScheduleReceipt, MapMakeResourceScheduleError> {
    let (first, mut steps, rows_claim) = match &schedule.placement {
        MapMakeResourcePlacementReceipt::FirstBonusRowOpen(first) => {
            (first.clone(), Vec::new(), None)
        }
        MapMakeResourcePlacementReceipt::BonusRowsOpen(rows) => {
            (rows.first.clone(), rows.steps.clone(), Some(rows))
        }
        _ => {
            return Err(
                MapMakeResourceScheduleError::BonusRowsContinuationRequiresFirstOrLaterBoundary,
            )
        }
    };

    if !bonus_prefix_matches(schedule, &first.xml) {
        return Err(MapMakeResourceScheduleError::BonusRowsContinuityMismatch);
    }
    let mut state = rebuild_remaining_bonus_state(&first, &steps)?;
    let claimed_pool =
        rows_claim.map_or(&first.resource_pool_after, |rows| &rows.resource_pool_after);
    if rows_claim.is_some_and(|rows| rows.remaining_state != state)
        || state.mutation.resource_pool.as_ref() != Some(claimed_pool)
        || pool != claimed_pool
        || resource_divvy_pool_digest(pool) != state.mutation.resource_pool_digest
    {
        return Err(MapMakeResourceScheduleError::BonusRowsContinuityMismatch);
    }
    if let Some(rows) = rows_claim {
        if rows.pending_checkpoint_call_va != MAP_POST_RESOURCES_CHECKPOINT_CALL_VA
            || rows.pending_source_token != MAP_POST_RESOURCES_SOURCE_TOKEN
        {
            return Err(MapMakeResourceScheduleError::BonusRowsContinuityMismatch);
        }
        match &rows.category_tail {
            Some(tail) => {
                if tail != &category_tail_receipt(&state)
                    || rows.residual_va != BONUS_CATEGORY_TAIL_VA
                    || state.next_row_index != first.xml.xml_frontier.handoff.rows.len()
                {
                    return Err(MapMakeResourceScheduleError::BonusRowsContinuityMismatch);
                }
                return Err(MapMakeResourceScheduleError::BonusRowsComplete);
            }
            None if rows.residual_va != LATER_BONUS_ENTRY_VA
                || state.next_row_index >= first.xml.xml_frontier.handoff.rows.len() =>
            {
                return Err(MapMakeResourceScheduleError::BonusRowsContinuityMismatch);
            }
            None => {}
        }
    }
    if state.next_row_index >= first.xml.xml_frontier.handoff.rows.len() {
        return Err(MapMakeResourceScheduleError::BonusRowsComplete);
    }

    let receipt =
        execute_next_bonus_mutation(&mut state, &first.xml.xml_frontier.handoff, facts, host)
            .map_err(MapMakeResourceScheduleError::BonusRowsFrontier)?;
    let resource_pool_after = state
        .mutation
        .resource_pool
        .clone()
        .ok_or(MapMakeResourceScheduleError::BonusRowsContinuityMismatch)?;
    if resource_divvy_pool_digest(&resource_pool_after) != state.mutation.resource_pool_digest
        || receipt.resource_pool_digest_after != state.mutation.resource_pool_digest
        || receipt.random_state_after != state.mutation.random_state
        || receipt.world_checksum_after != state.mutation.world_checksum
        || receipt.allocated_resources_after != state.mutation.allocated_resources
        || receipt.requested_resources_after != state.mutation.requested_resources
    {
        return Err(MapMakeResourceScheduleError::BonusRowsContinuityMismatch);
    }
    steps.push(LaterBonusScheduleStep {
        facts: facts.clone(),
        receipt,
    });

    let complete = state.next_row_index == first.xml.xml_frontier.handoff.rows.len();
    let category_tail = complete.then(|| category_tail_receipt(&state));
    let residual_va = category_tail
        .as_ref()
        .map_or(LATER_BONUS_ENTRY_VA, |tail| tail.residual_va);
    *pool = resource_pool_after.clone();
    Ok(MapMakeResourceScheduleReceipt {
        post_nubify_checkpoint: schedule.post_nubify_checkpoint.clone(),
        caller_gap: schedule.caller_gap.clone(),
        placement: MapMakeResourcePlacementReceipt::BonusRowsOpen(
            PlaceResourcesBonusRowsBoundary {
                first,
                steps,
                remaining_state: state,
                resource_pool_after,
                category_tail,
                residual_va,
                pending_checkpoint_call_va: MAP_POST_RESOURCES_CHECKPOINT_CALL_VA,
                pending_source_token: MAP_POST_RESOURCES_SOURCE_TOKEN,
            },
        ),
    })
}
