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
    PlaceResourcesCallerEntryReceipt, MAP_PLACE_RESOURCES_CALL_VA, MAP_PLACE_RESOURCES_ENTRY_VA,
    MAP_POST_RESOURCES_CHECKPOINT_CALL_VA, MAP_POST_RESOURCES_SOURCE_TOKEN,
    MAP_POST_TRANSITIONS_CHECKPOINT_CALL_VA, MAP_POST_TRANSITIONS_SOURCE_TOKEN,
    MAP_RESOURCE_CALLER_CONTINUATION_VA, MAP_RESOURCE_CALLER_GAP_RESUME_VA,
    MAP_RESOURCE_DIAGNOSTIC_CHECKPOINT_CALL_VA, MAP_RESOURCE_DIAGNOSTIC_SOURCE_TOKEN,
};
use crate::nubify_forest_frontier::MAP_NUBIFY_FOREST_CALLER_RESUME_VA;
use crate::place_resources_pool_frontier::{
    execute_place_resources_pool_prefix, resource_divvy_pool_digest, PlaceResourcesEntryHandoff,
    PlaceResourcesLiveFacts, PlaceResourcesPoolError, PlaceResourcesPoolReceipt,
    ResourceDivvyPoolState, MAP_PLACE_RESOURCES_PREFIX_RESIDUAL_VA,
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
pub enum MapMakeResourcePlacementReceipt {
    Skipped(PlaceResourcesSkippedBoundary),
    EntryOpen(PlaceResourcesEntryBoundary),
    BodyOpen(PlaceResourcesBodyBoundary),
    XmlRowsOpen(PlaceResourcesXmlBoundary),
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
