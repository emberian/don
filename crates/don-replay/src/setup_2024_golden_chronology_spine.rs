//! Honest source/oracle composition for the supported 2024 golden prefix.
//!
//! [`crate::setup_2024_golden_capture::GoldenCaptureManifest`] is an image topology. Its
//! SHA-256 values prove adjacency and identity of captured saves; they do not prove that Don
//! executed the transitions between those saves. This module keeps that distinction explicit.
//! It joins the separate starting-Market lifecycle receipt to the post-Market oracle, exposes
//! all eleven schema-v2 images as [`GoldenOracleEvidence::CaptureOnly`], and returns the first
//! still-unowned execution child carried by the execution-backed Market placement receipt.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::systems::build_type_find_friends::{
    BuildTypeFindFriendsReceipt, BuildTypeFindFriendsStop,
};
use don_sim::systems::leader_produce_building_blocked_site_prefix::{
    LeaderProduceBuildingMarketBlockedLocationReceipt,
    LeaderProduceBuildingMarketBlockedLocationRequest,
};
use don_sim::systems::unit_inctime::SUPPORTED_RETAIL_EXE_SHA256;

use crate::setup_2024_frame0_plan_strategy::{
    bind_golden_frame0_owner0_plan_strategy_entry, frame0_plan_strategy_entry_authority_digest,
    plan_golden_frame0_owner0_plan_strategy_prefix, Frame0PlanStrategyEntryAuthority,
    Frame0PlanStrategyError, Frame0PlanStrategyPrefixPlan,
};
use crate::setup_2024_frame379::{
    Frame379SetupEntryReceipt, Frame379SetupReceipt, REPLAY_FILE_SHA256, SETUP_CALLS,
};
use crate::setup_2024_golden_capture::{
    bind_golden_starting_market_setup_entry, validate_golden_capture_manifest,
    GoldenCaptureManifest, GoldenCaptureManifestError, GoldenStartingMarketTransactionError,
    GoldenStartingMarketTransactionManifest, GOLDEN_CAPTURE_SCHEMA_VERSION,
    MINIMAL_UNIQUE_SIM_SNAPSHOTS,
};
use crate::setup_2024_starting_market::{
    golden_market_footprint_receipt_sha256, GoldenStartingMarketCityReceipt,
    GoldenStartingMarketSetupEntryAuthority,
};
use crate::world_owner_frontier::sha256;

pub const GOLDEN_CHRONOLOGY_SPINE_SCHEMA_VERSION: u64 = 1;
pub const GOLDEN_FRAME0_STRATEGY_SPINE_SCHEMA_VERSION: u64 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoldenOracleEvidence {
    /// A supported-retail whole-Sim image is bound by hash. No parent execution claim follows.
    CaptureOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoldenOracleBoundary {
    PostMarketBuildUnitsEntry,
    PostSetupReceiver { setup_ordinal: usize },
    PostFrameZeroPreSerialOne,
    PostSerialOneLeaderOptions,
    PostFrameOneTick,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenOracleNode {
    pub ordinal: usize,
    pub frame: i32,
    pub boundary: GoldenOracleBoundary,
    pub evidence: GoldenOracleEvidence,
    pub sim_sha256: [u8; 32],
}

/// The three RNG states are different chronological boundaries. Equality is not required or
/// forbidden here: the number of valid Market fine probes is source-owned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoldenMarketRngBoundaries {
    pub post_place_all: i32,
    pub post_shuffle_pre_market: i32,
    pub post_market_build_units_entry: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenMarketExecutionResidual {
    /// Complete execution-backed read-only accepted-site receipt on the pre-Market Sim.
    pub blocked_location: LeaderProduceBuildingMarketBlockedLocationReceipt,
    pub blocked_location_receipt_sha256: [u8; 32],
    /// Source-exact type/World prefix of the first scoring child.
    pub find_friends: BuildTypeFindFriendsReceipt,
    pub find_friends_receipt_sha256: [u8; 32],
    /// Derived from the receipt's current first open child, not a hardcoded horizon.
    pub child_call_va: u32,
    pub child_callee_va: u32,
    pub market_before_sim_sha256: [u8; 32],
    pub post_market_oracle_sim_sha256: [u8; 32],
    /// Evidence identity only. A nonzero opaque digest is not an execution receipt.
    pub opaque_native_trace_sha256: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoldenChronologyFirstMissingAuthority {
    MarketScoringChild(GoldenMarketExecutionResidual),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenChronologySpineReceipt {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub schema_version: u64,
    pub replay_file_sha256: [u8; 32],
    pub executable_sha256: [u8; 32],
    pub oracle_schema_version: u64,
    pub oracle_manifest_digest: [u8; 32],
    pub starting_market_setup_entry_digest: [u8; 32],
    pub market_evidence_digest: [u8; 32],
    pub rng: GoldenMarketRngBoundaries,
    pub oracle_nodes: Vec<GoldenOracleNode>,
    pub first_missing_authority: GoldenChronologyFirstMissingAuthority,
    /// This receipt deliberately never promotes capture adjacency to executed chronology.
    pub downstream_execution_authority_issued: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoldenDetachedPlanEvidence {
    /// Exact local stores are derived from decoded retail instructions, but no parent Sim is
    /// mutated and no chronological reachability follows.
    SourceExactDetached,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GoldenFrame0StrategyReachability {
    pub owner0_strategy_complete: bool,
    pub owner1_strategy_started: bool,
    pub owner1_strategy_complete: bool,
    pub scout_reachable: bool,
    pub merchants_reachable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenFrame0StrategySpineReceipt {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub schema_version: u64,
    pub parent_spine_digest: [u8; 32],
    pub completed_setup_composition_digest: [u8; 32],
    pub completed_setup_sim_sha256: [u8; 32],
    /// The step-11 entry is still a native image oracle, not proof that the parent ran there.
    pub entry_evidence: GoldenOracleEvidence,
    pub entry_authority: Frame0PlanStrategyEntryAuthority,
    /// Local scalar stores are executable source semantics on detached images only.
    pub local_prefix_evidence: GoldenDetachedPlanEvidence,
    pub local_prefix: Frame0PlanStrategyPrefixPlan,
    /// The Market Object-lookup frontier in the parent remains globally earlier.
    pub global_market_frontier_retained: bool,
    pub reachability: GoldenFrame0StrategyReachability,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoldenChronologySpineError {
    Oracle(GoldenCaptureManifestError),
    Market(GoldenStartingMarketTransactionError),
    StartingMarketAuthorityMismatch,
    InvalidMarketPlacementPrefix,
    MissingMarketEvidence,
    WrongRngBoundary,
    WrongOracleNodeCount { expected: usize, actual: usize },
    NonCaptureOracleNode { ordinal: usize },
    InvalidOracleNode { ordinal: usize },
    MissingCompositionDigest,
    CompositionDigestMismatch,
    ExecutionAuthorityOverclaim,
    Strategy(Frame0PlanStrategyError),
    InvalidCompletedSetupReceipt,
    CompletedSetupJoinMismatch,
    InvalidDetachedStrategyPrefix,
    StrategyReachabilityOverclaim,
}

impl fmt::Display for GoldenChronologySpineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "2024 golden chronology spine refused: {self:?}")
    }
}

impl std::error::Error for GoldenChronologySpineError {}

impl From<GoldenCaptureManifestError> for GoldenChronologySpineError {
    fn from(value: GoldenCaptureManifestError) -> Self {
        Self::Oracle(value)
    }
}

impl From<GoldenStartingMarketTransactionError> for GoldenChronologySpineError {
    fn from(value: GoldenStartingMarketTransactionError) -> Self {
        Self::Market(value)
    }
}

impl From<Frame0PlanStrategyError> for GoldenChronologySpineError {
    fn from(value: Frame0PlanStrategyError) -> Self {
        Self::Strategy(value)
    }
}

fn append_world_checksum(
    image: &mut Vec<u8>,
    checksum: &don_sim::systems::map_terrain::WorldChecksum,
) {
    image.extend_from_slice(&checksum.full.to_le_bytes());
    image.extend_from_slice(&checksum.bytes.to_le_bytes());
    for section in &checksum.per_section {
        image.extend_from_slice(&section.adler.to_le_bytes());
        image.extend_from_slice(&section.bytes.to_le_bytes());
    }
}

/// Stable identity for the schema-v2 oracle claims. It still describes captures, not execution.
pub fn golden_oracle_manifest_digest(manifest: &GoldenCaptureManifest) -> [u8; 32] {
    let mut image = b"don-2024-golden-oracle-manifest-v2".to_vec();
    image.extend_from_slice(&manifest.schema_version.to_le_bytes());
    image.extend_from_slice(&manifest.starting_market_o.to_le_bytes());
    image.extend_from_slice(&manifest.starting_market_type.to_le_bytes());

    let entry = &manifest.setup_entry;
    image.push(match entry.source {
        crate::setup_2024_frame379::Frame379SetupEntrySource::CompleteRetailBuildUnitsEntry => 1,
    });
    image.extend_from_slice(&entry.revision.to_le_bytes());
    image.extend_from_slice(&entry.replay_file_sha256);
    image.extend_from_slice(&entry.executable_sha256);
    image.extend_from_slice(&entry.entry_sim_sha256);
    append_world_checksum(&mut image, &entry.post_place_all_world_checksum);
    image.extend_from_slice(&entry.post_place_all_random_state.to_le_bytes());
    image.extend_from_slice(&entry.entry_random_state.to_le_bytes());
    image.extend_from_slice(&entry.terrain_source_digest);
    image.extend_from_slice(&entry.mountain_height_receipt_sha256);

    for capture in &manifest.completed_inits {
        image.push(match capture.source {
            crate::setup_2024_frame379::Frame379CompletedInitSource::CompleteRetailObjectsInitUnitReceiver => 1,
        });
        image.extend_from_slice(&capture.revision.to_le_bytes());
        image.extend_from_slice(&(capture.setup_ordinal as u64).to_le_bytes());
        image.extend_from_slice(&capture.replay_file_sha256);
        image.extend_from_slice(&capture.executable_sha256);
        image.extend_from_slice(&capture.before_sim_sha256);
        image.extend_from_slice(&capture.after_sim_sha256);
        image.extend_from_slice(&capture.detailed_receipt_sha256);
    }
    image.push(match manifest.frame1_entry.source {
        crate::setup_2024_frame1_leader_options::Frame1CommandEntrySource::AuthoritativePlaybackChronologyFrameZeroThroughOne => 1,
    });
    image.extend_from_slice(&manifest.frame1_entry.revision.to_le_bytes());
    image.extend_from_slice(&manifest.frame1_entry.replay_file_sha256);
    image.extend_from_slice(&manifest.frame1_entry.executable_sha256);
    image.extend_from_slice(&manifest.frame1_entry.setup_sim_sha256);
    image.extend_from_slice(&manifest.frame1_entry.command_entry_sim_sha256);
    image.push(match manifest.frame1_post_command.source {
        crate::setup_2024_golden_capture::Frame1PostCommandSource::CompleteRetailSerialOneLeaderOptionsReturn => 1,
    });
    image.extend_from_slice(&manifest.frame1_post_command.revision.to_le_bytes());
    image.extend_from_slice(&manifest.frame1_post_command.replay_file_sha256);
    image.extend_from_slice(&manifest.frame1_post_command.executable_sha256);
    image.extend_from_slice(&manifest.frame1_post_command.command_entry_sim_sha256);
    image.extend_from_slice(&manifest.frame1_post_command.post_command_sim_sha256);
    image.push(match manifest.frame2_post_tick.source {
        crate::setup_2024_golden_capture::Frame2PostTickSource::CompleteRetailPostCommandFrameOneDoFrame => 1,
    });
    image.extend_from_slice(&manifest.frame2_post_tick.revision.to_le_bytes());
    image.extend_from_slice(&manifest.frame2_post_tick.replay_file_sha256);
    image.extend_from_slice(&manifest.frame2_post_tick.executable_sha256);
    image.extend_from_slice(&manifest.frame2_post_tick.post_command_sim_sha256);
    image.extend_from_slice(&manifest.frame2_post_tick.checkpoint_sim_sha256);
    sha256(&image)
}

/// Project the manifest into its eleven unique whole-Sim nodes. Every node remains capture-only.
pub fn golden_capture_only_oracle_nodes(
    manifest: &GoldenCaptureManifest,
) -> Result<Vec<GoldenOracleNode>, GoldenChronologySpineError> {
    validate_golden_capture_manifest(manifest)?;
    let mut nodes = Vec::with_capacity(MINIMAL_UNIQUE_SIM_SNAPSHOTS);
    nodes.push(GoldenOracleNode {
        ordinal: 0,
        frame: 0,
        boundary: GoldenOracleBoundary::PostMarketBuildUnitsEntry,
        evidence: GoldenOracleEvidence::CaptureOnly,
        sim_sha256: manifest.setup_entry.entry_sim_sha256,
    });
    for (setup_ordinal, capture) in manifest.completed_inits.iter().enumerate() {
        nodes.push(GoldenOracleNode {
            ordinal: nodes.len(),
            frame: 0,
            boundary: GoldenOracleBoundary::PostSetupReceiver { setup_ordinal },
            evidence: GoldenOracleEvidence::CaptureOnly,
            sim_sha256: capture.after_sim_sha256,
        });
    }
    nodes.push(GoldenOracleNode {
        ordinal: nodes.len(),
        frame: 1,
        boundary: GoldenOracleBoundary::PostFrameZeroPreSerialOne,
        evidence: GoldenOracleEvidence::CaptureOnly,
        sim_sha256: manifest.frame1_entry.command_entry_sim_sha256,
    });
    nodes.push(GoldenOracleNode {
        ordinal: nodes.len(),
        frame: 1,
        boundary: GoldenOracleBoundary::PostSerialOneLeaderOptions,
        evidence: GoldenOracleEvidence::CaptureOnly,
        sim_sha256: manifest.frame1_post_command.post_command_sim_sha256,
    });
    nodes.push(GoldenOracleNode {
        ordinal: nodes.len(),
        frame: 2,
        boundary: GoldenOracleBoundary::PostFrameOneTick,
        evidence: GoldenOracleEvidence::CaptureOnly,
        sim_sha256: manifest.frame2_post_tick.checkpoint_sim_sha256,
    });
    if nodes.len() != MINIMAL_UNIQUE_SIM_SNAPSHOTS {
        return Err(GoldenChronologySpineError::WrongOracleNodeCount {
            expected: MINIMAL_UNIQUE_SIM_SNAPSHOTS,
            actual: nodes.len(),
        });
    }
    Ok(nodes)
}

/// Digest the exact source-derived Market prefix and the captured effects it can validate.
/// `native_trace_sha256` is included as an identity but never interpreted as an execution bit.
pub fn golden_market_evidence_digest(
    market: &GoldenStartingMarketCityReceipt,
    blocked_location_receipt_sha256: [u8; 32],
    find_friends_receipt_sha256: [u8; 32],
) -> [u8; 32] {
    let mut image = b"don-2024-golden-market-evidence-v1".to_vec();
    image.extend_from_slice(&market.capture_revision.to_le_bytes());
    image.push(match market.source {
        crate::setup_2024_starting_market::GoldenStartingMarketCaptureSource::CompleteRetailLeaderProduceBuildingReturn => 1,
    });
    image.extend_from_slice(&market.placement.plan.replay_file_sha256);
    image.extend_from_slice(&market.executable_sha256);
    image.extend_from_slice(&market.native_trace_sha256);
    image.extend_from_slice(&market.footprint_receipt_sha256);
    image.extend_from_slice(&blocked_location_receipt_sha256);
    image.extend_from_slice(&find_friends_receipt_sha256);
    image.extend_from_slice(&market.before_sim_sha256);
    image.extend_from_slice(&market.after_sim_sha256);
    image.extend_from_slice(&market.before_cities.checksum.to_le_bytes());
    image.extend_from_slice(&market.before_cities.bytes_walked.to_le_bytes());
    image.extend_from_slice(&market.before_cities.cities_walked.to_le_bytes());
    image.extend_from_slice(&market.before_cities.slots_scanned.to_le_bytes());
    image.extend_from_slice(&market.after_cities.checksum.to_le_bytes());
    image.extend_from_slice(&market.after_cities.bytes_walked.to_le_bytes());
    image.extend_from_slice(&market.after_cities.cities_walked.to_le_bytes());
    image.extend_from_slice(&market.after_cities.slots_scanned.to_le_bytes());
    image.extend_from_slice(&(market.center_build_row as u64).to_le_bytes());
    image.extend_from_slice(&(market.market_build_row as u64).to_le_bytes());
    image.extend_from_slice(&market.market_build_o.to_le_bytes());
    image.extend_from_slice(&market.market_city_slot.to_le_bytes());
    image.extend_from_slice(&market.space_grade.to_le_bytes());
    image.extend_from_slice(&market.random_state_before.to_le_bytes());
    image.extend_from_slice(&market.random_state_after.to_le_bytes());
    image.extend_from_slice(&market.selected_placement_coord[0].to_le_bytes());
    image.extend_from_slice(&market.selected_placement_coord[1].to_le_bytes());
    for draw in &market.fine_random_draws {
        image.extend_from_slice(&draw.call_va.to_le_bytes());
        image.extend_from_slice(&draw.callee_va.to_le_bytes());
        image.extend_from_slice(&draw.placement_coord[0].to_le_bytes());
        image.extend_from_slice(&draw.placement_coord[1].to_le_bytes());
        image.extend_from_slice(&draw.state_before.to_le_bytes());
        image.extend_from_slice(&draw.raw.to_le_bytes());
        image.extend_from_slice(&draw.remainder.to_le_bytes());
        image.extend_from_slice(&draw.state_after.to_le_bytes());
    }
    for write in &market.city_checksum_writes {
        image.extend_from_slice(&write.offset.to_le_bytes());
        image.push(write.before);
        image.push(write.after);
        image.extend_from_slice(&write.writer_va.to_le_bytes());
        image.push(u8::from(write.value_changed));
    }
    image.extend_from_slice(&market.source_produced_city_bytes.to_le_bytes());
    image.push(u8::from(market.installed_in_scoreboard));
    sha256(&image)
}

fn blocked_location_request_digest(
    request: &LeaderProduceBuildingMarketBlockedLocationRequest,
) -> [u8; 32] {
    let mut image = b"don-2024-golden-market-blocked-location-request-v1".to_vec();
    image.extend_from_slice(&golden_market_footprint_receipt_sha256(&request.input));
    image.extend_from_slice(&request.call_va.to_le_bytes());
    image.extend_from_slice(&request.callee_va.to_le_bytes());
    image.push(request.owner);
    image.extend_from_slice(&request.type_index.to_le_bytes());
    for value in request
        .placement_coord
        .into_iter()
        .chain(request.placement_tcoord)
        .chain(request.footprint_corner)
    {
        image.extend_from_slice(&value.to_le_bytes());
    }
    image.extend_from_slice(&request.city_constraint.to_le_bytes());
    image.extend_from_slice(&request.blocked_detail.to_le_bytes());
    image.extend_from_slice(&request.first_world_child_call_va.to_le_bytes());
    image.extend_from_slice(&request.first_world_child_callee_va.to_le_bytes());
    sha256(&image)
}

/// Stable identity for the complete execution-backed, read-only Market accepted-site receipt.
/// Every projected World/City/Build read and the typed next child is included.
pub fn golden_market_blocked_location_receipt_sha256(
    receipt: &LeaderProduceBuildingMarketBlockedLocationReceipt,
) -> [u8; 32] {
    let mut image = b"don-2024-golden-market-blocked-location-receipt-v1".to_vec();
    image.extend_from_slice(&blocked_location_request_digest(&receipt.input));

    let tregion = &receipt.tregion;
    image.extend_from_slice(&tregion.query.call_va.to_le_bytes());
    image.extend_from_slice(&tregion.query.callee_va.to_le_bytes());
    for value in tregion.query.tcoord {
        image.extend_from_slice(&value.to_le_bytes());
    }
    for value in tregion.world_dims {
        image.extend_from_slice(&value.to_le_bytes());
    }
    for value in tregion.tile_dims {
        image.extend_from_slice(&value.to_le_bytes());
    }
    append_world_checksum(&mut image, &tregion.world_checksum);
    for value in tregion.owning_wcoord {
        image.extend_from_slice(&value.to_le_bytes());
    }
    image.extend_from_slice(&(tregion.wdata_index as u64).to_le_bytes());
    image.extend_from_slice(&tregion.wdata_flags.to_le_bytes());
    image.extend_from_slice(&tregion.region.to_le_bytes());
    image.extend_from_slice(&tregion.region2.to_le_bytes());
    match tregion.tdata_index {
        Some(index) => {
            image.push(1);
            image.extend_from_slice(&(index as u64).to_le_bytes());
        }
        None => image.push(0),
    }
    match tregion.tdata_low_byte {
        Some(value) => image.extend_from_slice(&[1, value]),
        None => image.push(0),
    }
    image.extend_from_slice(&tregion.returned_region.to_le_bytes());

    image.push(u8::from(receipt.immediate));
    image.extend_from_slice(&(receipt.territory_reads.len() as u64).to_le_bytes());
    for read in &receipt.territory_reads {
        for value in read.tile {
            image.extend_from_slice(&value.to_le_bytes());
        }
        image.extend_from_slice(&read.terrain_mask.to_le_bytes());
        image.extend_from_slice(&read.territory_owner.to_le_bytes());
    }
    image.extend_from_slice(&receipt.non_friendly_territory.to_le_bytes());

    let town = receipt.town;
    image.extend_from_slice(&(town.city_slot as u64).to_le_bytes());
    image.extend_from_slice(&town.city_object.to_le_bytes());
    image.extend_from_slice(&(town.center_build_row as u64).to_le_bytes());
    image.extend_from_slice(&town.center_type.to_le_bytes());
    image.push(u8::from(town.center_is_town));
    image.extend_from_slice(&town.distance.to_le_bytes());
    image.extend_from_slice(&town.radius.to_le_bytes());
    image.extend_from_slice(&(town.center_queue_len as u64).to_le_bytes());

    image.extend_from_slice(&(receipt.city_buildings.len() as u64).to_le_bytes());
    for read in &receipt.city_buildings {
        image.extend_from_slice(&read.object_index.to_le_bytes());
        image.extend_from_slice(&(read.row as u64).to_le_bytes());
        image.extend_from_slice(&read.current_type.to_le_bytes());
        image.push(u8::from(read.valid));
        image.push(u8::from(read.active));
        image.push(u8::from(read.market_relation));
        image.push(u8::from(read.counted));
        image.extend_from_slice(&read.next_object.to_le_bytes());
    }
    image.extend_from_slice(&receipt.counted_markets.to_le_bytes());

    image.extend_from_slice(&(receipt.water_reads.len() as u64).to_le_bytes());
    for read in &receipt.water_reads {
        for value in read.tile {
            image.extend_from_slice(&value.to_le_bytes());
        }
        image.extend_from_slice(&read.terrain_mask.to_le_bytes());
        image.push(u8::from(read.water));
    }
    image.extend_from_slice(&receipt.water_tiles.to_le_bytes());
    image.extend_from_slice(&receipt.blocked_location_returned.to_le_bytes());
    image.extend_from_slice(&receipt.native_returned.to_le_bytes());

    let continuation = receipt.continuation;
    image.extend_from_slice(&continuation.va.to_le_bytes());
    image.extend_from_slice(&continuation.bytes_remaining.to_le_bytes());
    image.push(continuation.owner);
    image.extend_from_slice(&continuation.type_index.to_le_bytes());
    image.extend_from_slice(&continuation.origin_build_object.to_le_bytes());
    image.extend_from_slice(&continuation.circle_offset.to_le_bytes());
    for value in continuation.candidate_world_cell {
        image.extend_from_slice(&value.to_le_bytes());
    }
    for value in continuation.placement_coord {
        image.extend_from_slice(&value.to_le_bytes());
    }

    let next = receipt.next_child;
    image.extend_from_slice(&next.call_va.to_le_bytes());
    image.extend_from_slice(&next.callee_va.to_le_bytes());
    image.push(next.owner);
    image.extend_from_slice(&next.type_index.to_le_bytes());
    for value in next.candidate_world_cell {
        image.extend_from_slice(&value.to_le_bytes());
    }
    image.extend_from_slice(&next.origin_city_filter.to_le_bytes());
    sha256(&image)
}

fn append_option_bool(image: &mut Vec<u8>, value: Option<bool>) {
    match value {
        Some(value) => image.extend_from_slice(&[1, u8::from(value)]),
        None => image.push(0),
    }
}

fn append_option_i32(image: &mut Vec<u8>, value: Option<i32>) {
    match value {
        Some(value) => {
            image.push(1);
            image.extend_from_slice(&value.to_le_bytes());
        }
        None => image.push(0),
    }
}

/// Stable identity for the exact `find_friends` type/World prefix and its typed first Object
/// lookup. No child return or coarse score is represented.
pub fn golden_market_find_friends_receipt_sha256(
    receipt: &BuildTypeFindFriendsReceipt,
) -> [u8; 32] {
    let mut image = b"don-2024-golden-market-find-friends-receipt-v1".to_vec();
    let request = receipt.request;
    image.extend_from_slice(&request.call_va.to_le_bytes());
    image.extend_from_slice(&request.callee_va.to_le_bytes());
    image.extend_from_slice(&request.type_index.to_le_bytes());
    for value in request.candidate_world_cell {
        image.extend_from_slice(&value.to_le_bytes());
    }
    image.extend_from_slice(&request.city_filter.to_le_bytes());
    image.extend_from_slice(&request.owner.to_le_bytes());
    for value in receipt.native_push_order {
        image.extend_from_slice(&value.to_le_bytes());
    }

    let type_reads = receipt.type_reads;
    image.extend_from_slice(&(type_reads.type_rows as u64).to_le_bytes());
    image.extend_from_slice(&type_reads.mutation_revision.to_le_bytes());
    image.extend_from_slice(&type_reads.attack.to_le_bytes());
    append_option_bool(&mut image, type_reads.tower_relation);
    append_option_bool(&mut image, type_reads.lookout_relation);
    append_option_bool(&mut image, type_reads.woodcutter_relation);
    match type_reads.build_flags {
        Some(value) => {
            image.push(1);
            image.extend_from_slice(&value.to_le_bytes());
        }
        None => image.push(0),
    }
    append_option_i32(&mut image, receipt.effective_city_filter);
    match &receipt.world_checksum {
        Some(checksum) => {
            image.push(1);
            append_world_checksum(&mut image, checksum);
        }
        None => image.push(0),
    }
    image.extend_from_slice(&(receipt.out_of_bounds.len() as u64).to_le_bytes());
    for probe in &receipt.out_of_bounds {
        image.extend_from_slice(&probe.circle_offset.to_le_bytes());
        for value in probe.world_cell {
            image.extend_from_slice(&value.to_le_bytes());
        }
    }
    image.push(match receipt.stop {
        BuildTypeFindFriendsStop::ReturnedAtTypeGate => 1,
        BuildTypeFindFriendsStop::ReturnedAfterBoundsExhaustion => 2,
        BuildTypeFindFriendsStop::FirstObjectLookup => 3,
    });
    append_option_i32(&mut image, receipt.returned);
    match receipt.first_child {
        Some(child) => {
            image.push(1);
            image.extend_from_slice(&child.call_va.to_le_bytes());
            image.extend_from_slice(&child.callee_va.to_le_bytes());
            for value in child.native_push_order {
                image.extend_from_slice(&value.to_le_bytes());
            }
            image.extend_from_slice(&child.circle_offset.to_le_bytes());
            for value in child.world_cell.into_iter().chain(child.tile) {
                image.extend_from_slice(&value.to_le_bytes());
            }
            image.extend_from_slice(&child.owner_filter.to_le_bytes());
            image.extend_from_slice(&child.excluded_object.to_le_bytes());
            image.extend_from_slice(&child.excluded_owner.to_le_bytes());
            image.extend_from_slice(&child.selected_owner_offset.to_le_bytes());
            image.extend_from_slice(&child.selected_owner_first_write.to_le_bytes());
        }
        None => image.push(0),
    }
    sha256(&image)
}

fn find_friends_extends_blocked_location(
    find_friends: &BuildTypeFindFriendsReceipt,
    blocked_location: &LeaderProduceBuildingMarketBlockedLocationReceipt,
) -> bool {
    let parent = blocked_location.next_child;
    find_friends.validates()
        && find_friends.request.call_va == parent.call_va
        && find_friends.request.callee_va == parent.callee_va
        && find_friends.request.owner == i32::from(parent.owner)
        && find_friends.request.type_index == parent.type_index
        && find_friends.request.candidate_world_cell == parent.candidate_world_cell
        && find_friends.request.city_filter == parent.origin_city_filter
        && find_friends.stop == BuildTypeFindFriendsStop::FirstObjectLookup
        && find_friends.returned.is_none()
        && find_friends.first_child.is_some()
}

fn append_oracle_node(image: &mut Vec<u8>, node: &GoldenOracleNode) {
    image.extend_from_slice(&(node.ordinal as u64).to_le_bytes());
    image.extend_from_slice(&node.frame.to_le_bytes());
    match node.boundary {
        GoldenOracleBoundary::PostMarketBuildUnitsEntry => image.extend_from_slice(&[1, 0]),
        GoldenOracleBoundary::PostSetupReceiver { setup_ordinal } => {
            image.extend_from_slice(&[2, setup_ordinal as u8]);
        }
        GoldenOracleBoundary::PostFrameZeroPreSerialOne => image.extend_from_slice(&[3, 0]),
        GoldenOracleBoundary::PostSerialOneLeaderOptions => image.extend_from_slice(&[4, 0]),
        GoldenOracleBoundary::PostFrameOneTick => image.extend_from_slice(&[5, 0]),
    }
    image.push(match node.evidence {
        GoldenOracleEvidence::CaptureOnly => 1,
    });
    image.extend_from_slice(&node.sim_sha256);
}

pub fn golden_chronology_spine_digest(receipt: &GoldenChronologySpineReceipt) -> [u8; 32] {
    let mut image = b"don-2024-golden-chronology-spine-v1".to_vec();
    image.extend_from_slice(&receipt.revision.to_le_bytes());
    image.extend_from_slice(&receipt.schema_version.to_le_bytes());
    image.extend_from_slice(&receipt.replay_file_sha256);
    image.extend_from_slice(&receipt.executable_sha256);
    image.extend_from_slice(&receipt.oracle_schema_version.to_le_bytes());
    image.extend_from_slice(&receipt.oracle_manifest_digest);
    image.extend_from_slice(&receipt.starting_market_setup_entry_digest);
    image.extend_from_slice(&receipt.market_evidence_digest);
    image.extend_from_slice(&receipt.rng.post_place_all.to_le_bytes());
    image.extend_from_slice(&receipt.rng.post_shuffle_pre_market.to_le_bytes());
    image.extend_from_slice(&receipt.rng.post_market_build_units_entry.to_le_bytes());
    for node in &receipt.oracle_nodes {
        append_oracle_node(&mut image, node);
    }
    match &receipt.first_missing_authority {
        GoldenChronologyFirstMissingAuthority::MarketScoringChild(residual) => {
            image.push(1);
            image.extend_from_slice(&residual.blocked_location_receipt_sha256);
            image.extend_from_slice(&residual.find_friends_receipt_sha256);
            image.extend_from_slice(&residual.child_call_va.to_le_bytes());
            image.extend_from_slice(&residual.child_callee_va.to_le_bytes());
            image.extend_from_slice(&residual.market_before_sim_sha256);
            image.extend_from_slice(&residual.post_market_oracle_sim_sha256);
            image.extend_from_slice(&residual.opaque_native_trace_sha256);
        }
    }
    image.push(u8::from(receipt.downstream_execution_authority_issued));
    sha256(&image)
}

/// Compose the strict Market join and the downstream oracle topology without issuing a false
/// execution authority. The missing child is derived from the accepted-site receipt's current
/// open boundary, allowing that frontier to move when the placement owner gains a decoded receipt.
pub fn compose_golden_chronology_spine(
    transaction: &GoldenStartingMarketTransactionManifest,
    oracle: &GoldenCaptureManifest,
    market: &GoldenStartingMarketCityReceipt,
    setup_entry: &Frame379SetupEntryReceipt,
    starting_market: &GoldenStartingMarketSetupEntryAuthority,
) -> Result<GoldenChronologySpineReceipt, GoldenChronologySpineError> {
    let derived =
        bind_golden_starting_market_setup_entry(transaction, oracle, market, setup_entry)?;
    if &derived != starting_market {
        return Err(GoldenChronologySpineError::StartingMarketAuthorityMismatch);
    }
    let blocked_location = market.blocked_location.clone();
    let find_friends = market.find_friends.clone();
    if !blocked_location.validates()
        || blocked_location.input != market.placement.blocked_location
        || !find_friends_extends_blocked_location(&find_friends, &blocked_location)
    {
        return Err(GoldenChronologySpineError::InvalidMarketPlacementPrefix);
    }
    if market.native_trace_sha256 == [0; 32]
        || market.before_sim_sha256 == [0; 32]
        || market.after_sim_sha256 == [0; 32]
    {
        return Err(GoldenChronologySpineError::MissingMarketEvidence);
    }
    let rng = GoldenMarketRngBoundaries {
        post_place_all: setup_entry.post_place_all_random_state,
        post_shuffle_pre_market: market.random_state_before,
        post_market_build_units_entry: market.random_state_after,
    };
    if rng.post_market_build_units_entry != setup_entry.entry_random_state
        || rng.post_market_build_units_entry != starting_market.random_state_after
    {
        return Err(GoldenChronologySpineError::WrongRngBoundary);
    }
    let oracle_nodes = golden_capture_only_oracle_nodes(oracle)?;
    let blocked_location_receipt_sha256 =
        golden_market_blocked_location_receipt_sha256(&blocked_location);
    let find_friends_receipt_sha256 = golden_market_find_friends_receipt_sha256(&find_friends);
    let market_evidence_digest = golden_market_evidence_digest(
        market,
        blocked_location_receipt_sha256,
        find_friends_receipt_sha256,
    );
    let child = find_friends
        .first_child
        .ok_or(GoldenChronologySpineError::InvalidMarketPlacementPrefix)?;
    let first_missing_authority =
        GoldenChronologyFirstMissingAuthority::MarketScoringChild(GoldenMarketExecutionResidual {
            child_call_va: child.call_va,
            child_callee_va: child.callee_va,
            blocked_location_receipt_sha256,
            find_friends_receipt_sha256,
            market_before_sim_sha256: market.before_sim_sha256,
            post_market_oracle_sim_sha256: market.after_sim_sha256,
            opaque_native_trace_sha256: market.native_trace_sha256,
            blocked_location,
            find_friends,
        });
    let mut receipt = GoldenChronologySpineReceipt {
        revision: starting_market.revision,
        composition_digest: [0; 32],
        schema_version: GOLDEN_CHRONOLOGY_SPINE_SCHEMA_VERSION,
        replay_file_sha256: REPLAY_FILE_SHA256,
        executable_sha256: starting_market.executable_sha256,
        oracle_schema_version: oracle.schema_version,
        oracle_manifest_digest: golden_oracle_manifest_digest(oracle),
        starting_market_setup_entry_digest: starting_market.composition_digest,
        market_evidence_digest,
        rng,
        oracle_nodes,
        first_missing_authority,
        downstream_execution_authority_issued: false,
    };
    receipt.composition_digest = golden_chronology_spine_digest(&receipt);
    Ok(receipt)
}

pub fn validate_golden_chronology_spine_receipt(
    receipt: &GoldenChronologySpineReceipt,
) -> Result<(), GoldenChronologySpineError> {
    if receipt.composition_digest == [0; 32] {
        return Err(GoldenChronologySpineError::MissingCompositionDigest);
    }
    if receipt.schema_version != GOLDEN_CHRONOLOGY_SPINE_SCHEMA_VERSION
        || receipt.revision == 0
        || receipt.oracle_schema_version != GOLDEN_CAPTURE_SCHEMA_VERSION
        || receipt.replay_file_sha256 != REPLAY_FILE_SHA256
        || receipt.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256
        || receipt.oracle_manifest_digest == [0; 32]
        || receipt.starting_market_setup_entry_digest == [0; 32]
        || receipt.market_evidence_digest == [0; 32]
        || receipt.oracle_nodes.len() != MINIMAL_UNIQUE_SIM_SNAPSHOTS
    {
        return Err(GoldenChronologySpineError::CompositionDigestMismatch);
    }
    for (ordinal, node) in receipt.oracle_nodes.iter().enumerate() {
        if node.ordinal != ordinal || node.evidence != GoldenOracleEvidence::CaptureOnly {
            return Err(GoldenChronologySpineError::NonCaptureOracleNode { ordinal });
        }
        let (expected_frame, expected_boundary) = match ordinal {
            0 => (0, GoldenOracleBoundary::PostMarketBuildUnitsEntry),
            1..=SETUP_CALLS => (
                0,
                GoldenOracleBoundary::PostSetupReceiver {
                    setup_ordinal: ordinal - 1,
                },
            ),
            value if value == SETUP_CALLS + 1 => {
                (1, GoldenOracleBoundary::PostFrameZeroPreSerialOne)
            }
            value if value == SETUP_CALLS + 2 => {
                (1, GoldenOracleBoundary::PostSerialOneLeaderOptions)
            }
            value if value == SETUP_CALLS + 3 => (2, GoldenOracleBoundary::PostFrameOneTick),
            _ => return Err(GoldenChronologySpineError::InvalidOracleNode { ordinal }),
        };
        if node.frame != expected_frame
            || node.boundary != expected_boundary
            || node.sim_sha256 == [0; 32]
        {
            return Err(GoldenChronologySpineError::InvalidOracleNode { ordinal });
        }
    }
    if receipt.downstream_execution_authority_issued {
        return Err(GoldenChronologySpineError::ExecutionAuthorityOverclaim);
    }
    match &receipt.first_missing_authority {
        GoldenChronologyFirstMissingAuthority::MarketScoringChild(residual)
            if residual.blocked_location.validates()
                && residual.blocked_location_receipt_sha256
                    == golden_market_blocked_location_receipt_sha256(
                        &residual.blocked_location,
                    )
                && find_friends_extends_blocked_location(
                    &residual.find_friends,
                    &residual.blocked_location,
                )
                && residual.find_friends_receipt_sha256
                    == golden_market_find_friends_receipt_sha256(&residual.find_friends)
                && residual.find_friends.first_child.is_some_and(|child| {
                    residual.child_call_va == child.call_va
                        && residual.child_callee_va == child.callee_va
                })
                && residual.child_call_va != 0
                && residual.child_callee_va != 0
                && residual.market_before_sim_sha256 != [0; 32]
                && residual.post_market_oracle_sim_sha256 != [0; 32]
                && residual.market_before_sim_sha256 != residual.post_market_oracle_sim_sha256
                && residual.post_market_oracle_sim_sha256 == receipt.oracle_nodes[0].sim_sha256
                && residual.opaque_native_trace_sha256 != [0; 32] => {}
        _ => return Err(GoldenChronologySpineError::InvalidMarketPlacementPrefix),
    }
    if receipt.composition_digest != golden_chronology_spine_digest(receipt) {
        return Err(GoldenChronologySpineError::CompositionDigestMismatch);
    }
    Ok(())
}

pub fn golden_frame0_strategy_spine_digest(receipt: &GoldenFrame0StrategySpineReceipt) -> [u8; 32] {
    let mut image = b"don-2024-golden-frame0-strategy-spine-v1".to_vec();
    image.extend_from_slice(&receipt.revision.to_le_bytes());
    image.extend_from_slice(&receipt.schema_version.to_le_bytes());
    image.extend_from_slice(&receipt.parent_spine_digest);
    image.extend_from_slice(&receipt.completed_setup_composition_digest);
    image.extend_from_slice(&receipt.completed_setup_sim_sha256);
    image.push(match receipt.entry_evidence {
        GoldenOracleEvidence::CaptureOnly => 1,
    });
    image.extend_from_slice(&receipt.entry_authority.composition_digest);
    image.extend_from_slice(&receipt.entry_authority.capture.preceding_chronology_digest);
    image.extend_from_slice(&receipt.entry_authority.capture.call_entry_sim_sha256);
    image.extend_from_slice(&receipt.entry_authority.capture.native_trace_sha256);
    image.push(match receipt.local_prefix_evidence {
        GoldenDetachedPlanEvidence::SourceExactDetached => 1,
    });
    image.extend_from_slice(&receipt.local_prefix.local_prefix_digest);
    image.extend_from_slice(&receipt.local_prefix.open.request_sha256);
    image.extend_from_slice(&receipt.local_prefix.open.callsite_va.to_le_bytes());
    image.extend_from_slice(&receipt.local_prefix.open.callee_va.to_le_bytes());
    image.push(u8::from(receipt.global_market_frontier_retained));
    image.extend_from_slice(&[
        u8::from(receipt.reachability.owner0_strategy_complete),
        u8::from(receipt.reachability.owner1_strategy_started),
        u8::from(receipt.reachability.owner1_strategy_complete),
        u8::from(receipt.reachability.scout_reachable),
        u8::from(receipt.reachability.merchants_reachable),
    ]);
    sha256(&image)
}

fn validate_completed_setup_receipt(
    setup: &Frame379SetupReceipt,
) -> Result<(), GoldenChronologySpineError> {
    if setup.authority_revision == 0
        || setup.authority_digest == [0; 32]
        || setup.leader_authority_revision == 0
        || setup.leader_authority_digest == [0; 32]
        || setup.replay_file_sha256 != REPLAY_FILE_SHA256
        || setup.replay_payload_sha256 == [0; 32]
        || setup.calls.len() != SETUP_CALLS
        || setup.canonical_frame != 0
        || setup.canonical_composition_digest == [0; 32]
    {
        return Err(GoldenChronologySpineError::InvalidCompletedSetupReceipt);
    }
    Ok(())
}

/// Extend the honest oracle spine with the detached owner-zero step-11 prefix.
///
/// The returned `get_team_terr` request is the deepest source-exact local boundary, not the
/// globally earliest replay gap: the parent's Market scoring child remains open. The captured
/// step-11 entry and all original eleven oracle nodes remain capture-only, and no Unit actor is
/// made reachable.
pub fn compose_golden_frame0_strategy_spine(
    parent: &GoldenChronologySpineReceipt,
    setup: &Frame379SetupReceipt,
    entry_authority: &Frame0PlanStrategyEntryAuthority,
) -> Result<GoldenFrame0StrategySpineReceipt, GoldenChronologySpineError> {
    validate_golden_chronology_spine_receipt(parent)?;
    validate_completed_setup_receipt(setup)?;

    let rebound = bind_golden_frame0_owner0_plan_strategy_entry(entry_authority.capture.clone())?;
    if &rebound != entry_authority {
        return Err(GoldenChronologySpineError::InvalidDetachedStrategyPrefix);
    }
    let local_prefix = plan_golden_frame0_owner0_plan_strategy_prefix(entry_authority)?;
    let completed_setup_node = parent
        .oracle_nodes
        .get(SETUP_CALLS)
        .ok_or(GoldenChronologySpineError::CompletedSetupJoinMismatch)?;
    if completed_setup_node.boundary
        != (GoldenOracleBoundary::PostSetupReceiver {
            setup_ordinal: SETUP_CALLS - 1,
        })
        || completed_setup_node.evidence != GoldenOracleEvidence::CaptureOnly
        || setup.canonical_composition_digest != entry_authority.capture.setup_composition_digest
        || completed_setup_node.sim_sha256 != entry_authority.capture.completed_setup_sim_sha256
        || entry_authority.capture.replay_file_sha256 != parent.replay_file_sha256
        || entry_authority.capture.executable_sha256 != parent.executable_sha256
    {
        return Err(GoldenChronologySpineError::CompletedSetupJoinMismatch);
    }

    let mut receipt = GoldenFrame0StrategySpineReceipt {
        revision: entry_authority.revision,
        composition_digest: [0; 32],
        schema_version: GOLDEN_FRAME0_STRATEGY_SPINE_SCHEMA_VERSION,
        parent_spine_digest: parent.composition_digest,
        completed_setup_composition_digest: setup.canonical_composition_digest,
        completed_setup_sim_sha256: completed_setup_node.sim_sha256,
        entry_evidence: GoldenOracleEvidence::CaptureOnly,
        entry_authority: entry_authority.clone(),
        local_prefix_evidence: GoldenDetachedPlanEvidence::SourceExactDetached,
        local_prefix,
        global_market_frontier_retained: true,
        reachability: GoldenFrame0StrategyReachability::default(),
    };
    receipt.composition_digest = golden_frame0_strategy_spine_digest(&receipt);
    Ok(receipt)
}

pub fn validate_golden_frame0_strategy_spine(
    parent: &GoldenChronologySpineReceipt,
    setup: &Frame379SetupReceipt,
    receipt: &GoldenFrame0StrategySpineReceipt,
) -> Result<(), GoldenChronologySpineError> {
    validate_golden_chronology_spine_receipt(parent)?;
    validate_completed_setup_receipt(setup)?;
    if receipt.revision == 0
        || receipt.composition_digest == [0; 32]
        || receipt.schema_version != GOLDEN_FRAME0_STRATEGY_SPINE_SCHEMA_VERSION
        || receipt.parent_spine_digest != parent.composition_digest
        || receipt.completed_setup_composition_digest != setup.canonical_composition_digest
        || receipt.entry_evidence != GoldenOracleEvidence::CaptureOnly
        || receipt.local_prefix_evidence != GoldenDetachedPlanEvidence::SourceExactDetached
        || !receipt.global_market_frontier_retained
    {
        return Err(GoldenChronologySpineError::InvalidDetachedStrategyPrefix);
    }
    if receipt.reachability != GoldenFrame0StrategyReachability::default() {
        return Err(GoldenChronologySpineError::StrategyReachabilityOverclaim);
    }

    let rebound =
        bind_golden_frame0_owner0_plan_strategy_entry(receipt.entry_authority.capture.clone())?;
    let expected_prefix = plan_golden_frame0_owner0_plan_strategy_prefix(&rebound)?;
    let completed_setup_node = parent
        .oracle_nodes
        .get(SETUP_CALLS)
        .ok_or(GoldenChronologySpineError::CompletedSetupJoinMismatch)?;
    if rebound != receipt.entry_authority
        || expected_prefix != receipt.local_prefix
        || receipt.revision != receipt.entry_authority.revision
        || receipt.entry_authority.composition_digest
            != frame0_plan_strategy_entry_authority_digest(&receipt.entry_authority.capture)
        || receipt.completed_setup_sim_sha256 != completed_setup_node.sim_sha256
        || receipt.completed_setup_sim_sha256
            != receipt.entry_authority.capture.completed_setup_sim_sha256
        || receipt.completed_setup_composition_digest
            != receipt.entry_authority.capture.setup_composition_digest
        || receipt.entry_authority.capture.replay_file_sha256 != parent.replay_file_sha256
        || receipt.entry_authority.capture.executable_sha256 != parent.executable_sha256
    {
        return Err(GoldenChronologySpineError::CompletedSetupJoinMismatch);
    }
    if receipt.composition_digest != golden_frame0_strategy_spine_digest(receipt) {
        return Err(GoldenChronologySpineError::CompositionDigestMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use don_sim::systems::map_terrain::{SectionDigest, WorldChecksum, WorldSection};

    use crate::setup_2024_frame0_plan_strategy::{
        Frame0PlanStrategyCityImage, Frame0PlanStrategyEntryCapture, Frame0PlanStrategyEntrySource,
        Frame0PlanStrategyLeaderImage, GOLDEN_CENTER_BUILD_O, GOLDEN_CENTER_CITY_SLOT,
        GOLDEN_FIRST_STRATEGY_ORDINAL, GOLDEN_FIRST_STRATEGY_OWNER, GOLDEN_FRAME, GOLDEN_STEP,
        PLAN_ENTRY_SCRATCH_DWORDS,
    };
    use crate::setup_2024_frame1_leader_options::{
        Frame1CommandEntryCapture, Frame1CommandEntrySource,
    };
    use crate::setup_2024_frame379::{
        Frame379CompletedInitCapture, Frame379CompletedInitSource, Frame379SetupEntryCapture,
        Frame379SetupEntrySource, DUTCH_STARTING_MARKET_O, DUTCH_STARTING_MARKET_TYPE,
    };
    use crate::setup_2024_golden_capture::{
        Frame1PostCommandCapture, Frame1PostCommandSource, Frame2PostTickCapture,
        Frame2PostTickSource,
    };

    fn hash(byte: u8) -> [u8; 32] {
        [byte; 32]
    }

    fn manifest() -> GoldenCaptureManifest {
        let images: [[u8; 32]; MINIMAL_UNIQUE_SIM_SNAPSHOTS] =
            std::array::from_fn(|index| hash(index as u8 + 1));
        GoldenCaptureManifest {
            schema_version: GOLDEN_CAPTURE_SCHEMA_VERSION,
            starting_market_o: DUTCH_STARTING_MARKET_O,
            starting_market_type: DUTCH_STARTING_MARKET_TYPE,
            setup_entry: Frame379SetupEntryCapture {
                revision: 1,
                source: Frame379SetupEntrySource::CompleteRetailBuildUnitsEntry,
                replay_file_sha256: REPLAY_FILE_SHA256,
                executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
                entry_sim_sha256: images[0],
                post_place_all_world_checksum: WorldChecksum {
                    per_section: [SectionDigest::default(); WorldSection::COUNT],
                    full: 0,
                    bytes: 0,
                },
                post_place_all_random_state: 11,
                entry_random_state: 13,
                terrain_source_digest: hash(40),
                mountain_height_receipt_sha256: hash(41),
            },
            completed_inits: std::array::from_fn(|setup_ordinal| Frame379CompletedInitCapture {
                revision: 1,
                source: Frame379CompletedInitSource::CompleteRetailObjectsInitUnitReceiver,
                replay_file_sha256: REPLAY_FILE_SHA256,
                executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
                setup_ordinal,
                before_sim_sha256: images[setup_ordinal],
                after_sim_sha256: images[setup_ordinal + 1],
                detailed_receipt_sha256: hash(50 + setup_ordinal as u8),
            }),
            frame1_entry: Frame1CommandEntryCapture {
                revision: 1,
                source:
                    Frame1CommandEntrySource::AuthoritativePlaybackChronologyFrameZeroThroughOne,
                replay_file_sha256: REPLAY_FILE_SHA256,
                executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
                setup_sim_sha256: images[SETUP_CALLS],
                command_entry_sim_sha256: images[8],
            },
            frame1_post_command: Frame1PostCommandCapture {
                revision: 1,
                source: Frame1PostCommandSource::CompleteRetailSerialOneLeaderOptionsReturn,
                replay_file_sha256: REPLAY_FILE_SHA256,
                executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
                command_entry_sim_sha256: images[8],
                post_command_sim_sha256: images[9],
            },
            frame2_post_tick: Frame2PostTickCapture {
                revision: 1,
                source: Frame2PostTickSource::CompleteRetailPostCommandFrameOneDoFrame,
                replay_file_sha256: REPLAY_FILE_SHA256,
                executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
                post_command_sim_sha256: images[9],
                checkpoint_sim_sha256: images[10],
            },
        }
    }

    #[test]
    fn schema_v2_projects_exactly_eleven_capture_only_nodes() {
        let nodes = golden_capture_only_oracle_nodes(&manifest()).unwrap();
        assert_eq!(nodes.len(), 11);
        assert!(nodes
            .iter()
            .all(|node| node.evidence == GoldenOracleEvidence::CaptureOnly));
        assert_eq!(
            nodes[0].boundary,
            GoldenOracleBoundary::PostMarketBuildUnitsEntry
        );
        assert_eq!(
            nodes[7].boundary,
            GoldenOracleBoundary::PostSetupReceiver { setup_ordinal: 6 }
        );
        assert_eq!(
            nodes[8].boundary,
            GoldenOracleBoundary::PostFrameZeroPreSerialOne
        );
        assert_eq!(
            nodes[9].boundary,
            GoldenOracleBoundary::PostSerialOneLeaderOptions
        );
        assert_eq!(nodes[10].boundary, GoldenOracleBoundary::PostFrameOneTick);
    }

    #[test]
    fn oracle_digest_binds_capture_claims_without_upgrading_evidence() {
        let original = manifest();
        let digest = golden_oracle_manifest_digest(&original);
        let mut changed = original;
        changed.completed_inits[4].detailed_receipt_sha256[0] ^= 1;
        assert_ne!(digest, golden_oracle_manifest_digest(&changed));
        assert!(golden_capture_only_oracle_nodes(&changed).is_ok());
    }

    #[test]
    fn detached_strategy_stops_at_team_territory_without_actor_reachability() {
        let entry = bind_golden_frame0_owner0_plan_strategy_entry(Frame0PlanStrategyEntryCapture {
            revision: 3,
            source: Frame0PlanStrategyEntrySource::CompleteRetailOwnerZeroPlanStrategyEntry,
            replay_file_sha256: REPLAY_FILE_SHA256,
            executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
            completed_setup_sim_sha256: hash(7),
            setup_composition_digest: hash(70),
            preceding_chronology_digest: hash(71),
            call_entry_sim_sha256: hash(72),
            native_trace_sha256: hash(73),
            frame: GOLDEN_FRAME,
            step: GOLDEN_STEP,
            owner: GOLDEN_FIRST_STRATEGY_OWNER,
            strategy_ordinal: GOLDEN_FIRST_STRATEGY_ORDINAL,
            leader: Frame0PlanStrategyLeaderImage {
                leader_flags: 0x7,
                who: 0,
                city_num: 1,
                village_num: 1,
                city_mark: 1,
                escrow_rate: [0; 6],
                scratch: [1; PLAN_ENTRY_SCRATCH_DWORDS],
            },
            cities: vec![Frame0PlanStrategyCityImage {
                slot: GOLDEN_CENTER_CITY_SLOT,
                city_flags: 1,
                city: GOLDEN_CENTER_CITY_SLOT,
                center_o: GOLDEN_CENTER_BUILD_O,
                who: GOLDEN_FIRST_STRATEGY_OWNER as i8,
                peasant_dist: -1,
                free: 1,
                busy: 2,
                gatherers: 3,
            }],
        })
        .unwrap();
        let local_prefix = plan_golden_frame0_owner0_plan_strategy_prefix(&entry).unwrap();
        let mut receipt = GoldenFrame0StrategySpineReceipt {
            revision: entry.revision,
            composition_digest: [0; 32],
            schema_version: GOLDEN_FRAME0_STRATEGY_SPINE_SCHEMA_VERSION,
            parent_spine_digest: hash(80),
            completed_setup_composition_digest: entry.capture.setup_composition_digest,
            completed_setup_sim_sha256: entry.capture.completed_setup_sim_sha256,
            entry_evidence: GoldenOracleEvidence::CaptureOnly,
            entry_authority: entry,
            local_prefix_evidence: GoldenDetachedPlanEvidence::SourceExactDetached,
            local_prefix,
            global_market_frontier_retained: true,
            reachability: GoldenFrame0StrategyReachability::default(),
        };
        receipt.composition_digest = golden_frame0_strategy_spine_digest(&receipt);

        assert_eq!(receipt.local_prefix.open.receiver_owner, 0);
        assert_eq!(receipt.local_prefix.open.callsite_va, 0x006b_98d0);
        assert_eq!(receipt.local_prefix.open.callee_va, 0x006d_62e0);
        assert_eq!(
            receipt.reachability,
            GoldenFrame0StrategyReachability::default()
        );
        assert!(receipt.global_market_frontier_retained);

        let digest = receipt.composition_digest;
        receipt.reachability.scout_reachable = true;
        assert_ne!(digest, golden_frame0_strategy_spine_digest(&receipt));
    }
}
