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

use don_sim::systems::air_patrol_building_search_frontier::WORLD_CELL_SEARCH_OFFSETS;
use don_sim::systems::build_type_find_friends::{
    BuildTypeFindFriendsReceipt, BuildTypeFindFriendsStop, BUILD_TYPE_FIND_FRIENDS_VA,
};
use don_sim::systems::leader_produce_building_blocked_site_prefix::{
    LeaderProduceBuildingMarketBlockedLocationReceipt,
    LeaderProduceBuildingMarketBlockedLocationRequest,
};
use don_sim::systems::objects_find_building_placed_at::{
    ObjectsFindBuildingPlacedAtReceipt, ObjectsFindBuildingPlacedAtStop,
    ObjectsSelectedOwnerAuthority, ObjectsSpatialBand, ObjectsSpatialKey,
};
use don_sim::systems::unit_inctime::SUPPORTED_RETAIL_EXE_SHA256;

use crate::setup_2024_frame0_get_team_terr::{
    advance_frame0_plan_strategy_first_diplomacy_read,
    frame0_get_team_terr_call_entry_authority_digest, frame0_get_team_terr_receipt_digest,
    frame0_plan_strategy_diplomacy_read_authority_digest,
    frame0_plan_strategy_diplomacy_step_digest,
    frame0_plan_strategy_opponent_get_team_terr_receipt_digest,
    frame0_plan_strategy_post_team_terr_digest,
    frame0_plan_strategy_reverse_diplomacy_read_authority_digest,
    plan_frame0_owner0_after_get_team_terr, resolve_captured_frame0_get_team_terr,
    resolve_frame0_plan_strategy_opponent_get_team_terr, Frame0GetTeamTerrCallEntryAuthority,
    Frame0GetTeamTerrError, Frame0GetTeamTerrReceipt, Frame0PlanStrategyDiplomacyReadAuthority,
    Frame0PlanStrategyDiplomacyStepError, Frame0PlanStrategyDiplomacyStepOpen,
    Frame0PlanStrategyDiplomacyStepPlan, Frame0PlanStrategyOpponentGetTeamTerrError,
    Frame0PlanStrategyOpponentGetTeamTerrReceipt, Frame0PlanStrategyPostTeamTerrError,
    Frame0PlanStrategyPostTeamTerrPlan, Frame0PlanStrategyReverseDiplomacyReadAuthority,
};
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
    golden_market_footprint_receipt_sha256, GoldenStartingMarketAcceptedPlacementReceipt,
    GoldenStartingMarketAfterFoundBuildContinuation, GoldenStartingMarketCityReceipt,
    GoldenStartingMarketFindFriendsAfterFirstLookupBoundary,
    GoldenStartingMarketFindFriendsCompleteReceipt, GoldenStartingMarketFindFriendsFoundBuildRead,
    GoldenStartingMarketFindFriendsLookupDisposition,
    GoldenStartingMarketFindFriendsOwnedRingEvent, GoldenStartingMarketFindFriendsRingAdvance,
    GoldenStartingMarketFindFriendsRingStep, GoldenStartingMarketFirstObjectLookupReceipt,
    GoldenStartingMarketFoundBuildPredicateOutcome, GoldenStartingMarketFoundBuildPredicateReceipt,
    GoldenStartingMarketFoundBuildRejectReason, GoldenStartingMarketFoundBuildTypeReads,
    GoldenStartingMarketSetupEntryAuthority, BUILD_GATHER_TYPE_MASK, GATHER_ENHANCER_TYPES,
    MARKET_FIND_FRIENDS_AFTER_FIRST_LOOKUP_VA, MARKET_FIND_FRIENDS_FIRST_FOUND_TYPE_VIRTUAL_SLOT,
};
use crate::world_owner_frontier::sha256;

pub const GOLDEN_CHRONOLOGY_SPINE_SCHEMA_VERSION: u64 = 5;
pub const GOLDEN_FRAME0_STRATEGY_SPINE_SCHEMA_VERSION: u64 = 1;
pub const GOLDEN_FRAME0_TEAM_TERR_SPINE_SCHEMA_VERSION: u64 = 4;

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
    /// Complete execution-backed first Object lookup, including every World/spatial read and
    /// the committed `ObjectsData+0x200` scratch journal.
    pub first_object_lookup: ObjectsFindBuildingPlacedAtReceipt,
    pub first_object_lookup_receipt_sha256: [u8; 32],
    pub objects_selected_owner_before: ObjectsSelectedOwnerAuthority,
    pub objects_selected_owner_after: ObjectsSelectedOwnerAuthority,
    /// Typed resume boundary after the first lookup returns. This is not a `find_friends`
    /// result or a coarse Market score.
    pub find_friends_after_first_lookup: GoldenStartingMarketFindFriendsAfterFirstLookupBoundary,
    pub find_friends_after_first_lookup_sha256: [u8; 32],
    /// Complete exact multi-hit `find_friends` return, including every lookup, predicate outcome,
    /// accumulator transition, and scratch journal in program order.
    pub complete_find_friends: GoldenStartingMarketFindFriendsCompleteReceipt,
    pub complete_find_friends_sha256: [u8; 32],
    pub objects_selected_owner_find_friends_after: ObjectsSelectedOwnerAuthority,
    pub find_friends_returned: i32,
    /// The caller's coarse score is the next unowned operation.
    pub market_residual: GoldenMarketResidual,
    pub market_before_sim_sha256: [u8; 32],
    pub post_market_oracle_sim_sha256: [u8; 32],
    /// Evidence identity only. A nonzero opaque digest is not an execution receipt.
    pub opaque_native_trace_sha256: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoldenMarketResidual {
    /// `find_friends` has returned exactly. The caller's coarse score and its conditional later
    /// World/Object search are both still unowned.
    CallerCoarseScore {
        find_friends_returned: i32,
        coarse_score_authority_issued: bool,
        conditional_world_find_authority_issued: bool,
    },
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
    /// The Market caller coarse-score residual in the parent remains globally earlier.
    pub global_market_frontier_retained: bool,
    pub reachability: GoldenFrame0StrategyReachability,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoldenDetachedChildEvidence {
    /// The child is evaluated exactly over a native call-entry projection, but no parent stores
    /// or chronological completion are published.
    SourceExactDetached,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenFrame0TeamTerrSpineReceipt {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub schema_version: u64,
    pub parent_strategy_spine_digest: [u8; 32],
    /// The unified whole-Sim plan-entry capture remains an image authority only.
    pub call_entry_evidence: GoldenOracleEvidence,
    pub call_entry_authority: Frame0GetTeamTerrCallEntryAuthority,
    pub child_evidence: GoldenDetachedChildEvidence,
    pub child: Frame0GetTeamTerrReceipt,
    /// Exact detached stores and receiver-self skip after the child return. The open diplomacy
    /// read remains unexecuted.
    pub continuation_evidence: GoldenDetachedChildEvidence,
    pub continuation: Frame0PlanStrategyPostTeamTerrPlan,
    /// One relation value is projected from the same captured call-entry Sim. The image remains
    /// capture-only even though the field identity is exact.
    pub diplomacy_read_evidence: GoldenOracleEvidence,
    pub diplomacy_read: Frame0PlanStrategyDiplomacyReadAuthority,
    /// Exact branch semantics after the read; the reverse read/repeated child remains open.
    pub diplomacy_step_evidence: GoldenDetachedChildEvidence,
    pub diplomacy_step: Frame0PlanStrategyDiplomacyStepPlan,
    /// Exact branch-specific final owned child. A reverse relation remains a capture projection;
    /// the repeated team-territory result is detached source execution.
    pub diplomacy_drain: GoldenFrame0DiplomacyDrain,
    pub diplomacy_residual: GoldenFrame0DiplomacyResidual,
    /// The earlier Market caller coarse-score residual in the root spine remains open.
    pub global_market_frontier_retained: bool,
    /// `get_team_terr` is only one child inside owner zero's larger native strategy call.
    pub reachability: GoldenFrame0StrategyReachability,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoldenFrame0DiplomacyDrain {
    ReverseReadCapture(Frame0PlanStrategyReverseDiplomacyReadAuthority),
    OpponentTeamTerrSourceExact(Frame0PlanStrategyOpponentGetTeamTerrReceipt),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoldenFrame0DiplomacyResidual {
    /// The reverse relation is exact. If it is not ally `2`, the named repeated child still has
    /// no receipt; if it is ally, the later loop continuation remains unowned.
    AfterReverseRead {
        value: i32,
        reverse_relation_is_ally: bool,
        repeated_child_call_va_if_not_ally: u32,
        repeated_child_authority_issued: bool,
        owner0_strategy_complete: bool,
    },
    /// The repeated child has returned exactly; max/min opponent-territory stores are next.
    AfterOpponentTeamTerrReturn {
        result: i32,
        return_va: u32,
        post_return_store_authority_issued: bool,
        owner0_strategy_complete: bool,
    },
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
    TeamTerr(Frame0GetTeamTerrError),
    PostTeamTerr(Frame0PlanStrategyPostTeamTerrError),
    DiplomacyStep(Frame0PlanStrategyDiplomacyStepError),
    OpponentTeamTerr(Frame0PlanStrategyOpponentGetTeamTerrError),
    InvalidDiplomacyDrain,
    InvalidTeamTerrCallEntryAuthority,
    InvalidDetachedTeamTerrChild,
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

impl From<Frame0GetTeamTerrError> for GoldenChronologySpineError {
    fn from(value: Frame0GetTeamTerrError) -> Self {
        Self::TeamTerr(value)
    }
}

impl From<Frame0PlanStrategyPostTeamTerrError> for GoldenChronologySpineError {
    fn from(value: Frame0PlanStrategyPostTeamTerrError) -> Self {
        Self::PostTeamTerr(value)
    }
}

impl From<Frame0PlanStrategyDiplomacyStepError> for GoldenChronologySpineError {
    fn from(value: Frame0PlanStrategyDiplomacyStepError) -> Self {
        Self::DiplomacyStep(value)
    }
}

impl From<Frame0PlanStrategyOpponentGetTeamTerrError> for GoldenChronologySpineError {
    fn from(value: Frame0PlanStrategyOpponentGetTeamTerrError) -> Self {
        Self::OpponentTeamTerr(value)
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
    first_object_lookup_receipt_sha256: [u8; 32],
    find_friends_after_first_lookup_sha256: [u8; 32],
    complete_find_friends_sha256: [u8; 32],
) -> [u8; 32] {
    let mut image = b"don-2024-golden-market-evidence-v5".to_vec();
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
    image.extend_from_slice(&first_object_lookup_receipt_sha256);
    append_selected_owner_authority(&mut image, market.objects_selected_owner_before);
    append_selected_owner_authority(&mut image, market.objects_selected_owner_after);
    image.extend_from_slice(&find_friends_after_first_lookup_sha256);
    image.extend_from_slice(&complete_find_friends_sha256);
    append_selected_owner_authority(&mut image, market.objects_selected_owner_find_friends_after);
    image.extend_from_slice(&market.find_friends_returned.to_le_bytes());
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
    image.extend_from_slice(&(market.fine_random_draws.len() as u64).to_le_bytes());
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

fn append_objects_spatial_key(image: &mut Vec<u8>, key: ObjectsSpatialKey) {
    image.extend_from_slice(&key.who.to_le_bytes());
    image.extend_from_slice(&key.o.to_le_bytes());
}

fn append_selected_owner_authority(image: &mut Vec<u8>, authority: ObjectsSelectedOwnerAuthority) {
    image.extend_from_slice(&authority.revision.to_le_bytes());
    image.extend_from_slice(&authority.source_sha256);
    image.extend_from_slice(&authority.selected_owner.to_le_bytes());
}

fn append_object_lookup_request(
    image: &mut Vec<u8>,
    request: don_sim::systems::build_type_find_friends::ObjectsFindBuildingPlacedAtBoundary,
) {
    image.extend_from_slice(&request.call_va.to_le_bytes());
    image.extend_from_slice(&request.callee_va.to_le_bytes());
    for value in request.native_push_order {
        image.extend_from_slice(&value.to_le_bytes());
    }
    image.extend_from_slice(&request.circle_offset.to_le_bytes());
    for value in request.world_cell.into_iter().chain(request.tile) {
        image.extend_from_slice(&value.to_le_bytes());
    }
    image.extend_from_slice(&request.owner_filter.to_le_bytes());
    image.extend_from_slice(&request.excluded_object.to_le_bytes());
    image.extend_from_slice(&request.excluded_owner.to_le_bytes());
    image.extend_from_slice(&request.selected_owner_offset.to_le_bytes());
    image.extend_from_slice(&request.selected_owner_first_write.to_le_bytes());
}

/// Stable identity for the complete execution-backed first Object lookup. Every terrain,
/// spatial-chain, object/type/footprint read and every scratch-journal field participates.
pub fn golden_market_first_object_lookup_receipt_sha256(
    receipt: &ObjectsFindBuildingPlacedAtReceipt,
) -> [u8; 32] {
    let mut image = b"don-2024-golden-market-first-object-lookup-receipt-v1".to_vec();
    append_object_lookup_request(&mut image, receipt.request);

    append_world_checksum(&mut image, &receipt.world_checksum);
    for value in receipt.terrain.tile {
        image.extend_from_slice(&value.to_le_bytes());
    }
    image.extend_from_slice(&receipt.terrain.mask.to_le_bytes());
    image.extend_from_slice(&[
        u8::from(receipt.terrain.building_blocker),
        u8::from(receipt.terrain.started),
        u8::from(receipt.terrain.spatial_walk_reached),
    ]);

    image.extend_from_slice(&(receipt.cells.len() as u64).to_le_bytes());
    for read in &receipt.cells {
        image.push(read.offset_index);
        for value in read.offset.into_iter().chain(read.world_cell) {
            image.extend_from_slice(&value.to_le_bytes());
        }
        image.push(u8::from(read.in_bounds));
        match read.head {
            Some(key) => {
                image.push(1);
                append_objects_spatial_key(&mut image, key);
            }
            None => image.push(0),
        }
    }

    image.extend_from_slice(&(receipt.objects.len() as u64).to_le_bytes());
    for read in &receipt.objects {
        image.push(read.cell_offset_index);
        append_objects_spatial_key(&mut image, read.key);
        append_objects_spatial_key(&mut image, read.down);
        image.push(match read.band {
            ObjectsSpatialBand::Unit => 1,
            ObjectsSpatialBand::Build => 2,
            ObjectsSpatialBand::Wall => 3,
        });
        image.extend_from_slice(&(read.row as u64).to_le_bytes());
        image.extend_from_slice(&[
            u8::from(read.playable_owner),
            u8::from(read.excluded),
            u8::from(read.owner_filter_matches),
        ]);
        append_option_bool(&mut image, read.valid_build);
        append_option_i32(&mut image, read.type_index);
        match read.footprint {
            Some(footprint) => {
                image.push(1);
                image.extend_from_slice(&footprint.x_size.to_le_bytes());
                image.extend_from_slice(&footprint.y_size.to_le_bytes());
            }
            None => image.push(0),
        }
        for value in [read.position, read.corner] {
            match value {
                Some(value) => {
                    image.push(1);
                    for component in value {
                        image.extend_from_slice(&component.to_le_bytes());
                    }
                }
                None => image.push(0),
            }
        }
        append_option_bool(&mut image, read.contains_query_tile);
    }

    image.extend_from_slice(&receipt.scratch.offset.to_le_bytes());
    append_selected_owner_authority(&mut image, receipt.scratch.before);
    image.extend_from_slice(&receipt.scratch.first_write.to_le_bytes());
    image.extend_from_slice(&receipt.scratch.staged_selected_owner.to_le_bytes());
    match receipt.scratch.after {
        Some(authority) => {
            image.push(1);
            append_selected_owner_authority(&mut image, authority);
        }
        None => image.push(0),
    }
    image.push(match receipt.stop {
        ObjectsFindBuildingPlacedAtStop::TerrainGateReturn => 1,
        ObjectsFindBuildingPlacedAtStop::SpatialExhaustionReturn => 2,
        ObjectsFindBuildingPlacedAtStop::ObjectHitReturn => 3,
        ObjectsFindBuildingPlacedAtStop::WallBandIdentityBoundary => 4,
    });
    append_option_i32(&mut image, receipt.returned);
    append_option_i32(&mut image, receipt.returned_owner);
    match receipt.wall_boundary {
        Some(boundary) => {
            image.push(1);
            image.extend_from_slice(&boundary.instruction_va.to_le_bytes());
            image.push(boundary.cell_offset_index);
            append_objects_spatial_key(&mut image, boundary.key);
            image.extend_from_slice(&(boundary.row as u64).to_le_bytes());
            image.extend_from_slice(&boundary.wall_stored_o.to_le_bytes());
            image.extend_from_slice(&boundary.required_banded_o.to_le_bytes());
        }
        None => image.push(0),
    }
    sha256(&image)
}

/// Stable identity for the exact `find_friends` resume boundary after its first Object lookup.
pub fn golden_market_find_friends_after_first_lookup_sha256(
    boundary: &GoldenStartingMarketFindFriendsAfterFirstLookupBoundary,
) -> [u8; 32] {
    let mut image = b"don-2024-golden-market-find-friends-after-first-lookup-v1".to_vec();
    image.extend_from_slice(&boundary.instruction_va.to_le_bytes());
    image.extend_from_slice(&boundary.function_va.to_le_bytes());
    image.extend_from_slice(&boundary.request.call_va.to_le_bytes());
    image.extend_from_slice(&boundary.request.callee_va.to_le_bytes());
    image.extend_from_slice(&boundary.request.type_index.to_le_bytes());
    for value in boundary.request.candidate_world_cell {
        image.extend_from_slice(&value.to_le_bytes());
    }
    image.extend_from_slice(&boundary.request.city_filter.to_le_bytes());
    image.extend_from_slice(&boundary.request.owner.to_le_bytes());
    image.extend_from_slice(&boundary.circle_offset.to_le_bytes());
    image.extend_from_slice(&boundary.accumulator_before.to_le_bytes());
    image.extend_from_slice(&boundary.effective_city_filter.to_le_bytes());
    image.extend_from_slice(&boundary.returned.to_le_bytes());
    append_option_i32(&mut image, boundary.returned_owner);
    sha256(&image)
}

fn first_object_lookup_extends_find_friends(
    lookup: &ObjectsFindBuildingPlacedAtReceipt,
    scratch_before: ObjectsSelectedOwnerAuthority,
    scratch_after: ObjectsSelectedOwnerAuthority,
    next: &GoldenStartingMarketFindFriendsAfterFirstLookupBoundary,
    find_friends: &BuildTypeFindFriendsReceipt,
) -> bool {
    lookup.validates()
        && find_friends.first_child == Some(lookup.request)
        && lookup.returned.is_some()
        && lookup.stop != ObjectsFindBuildingPlacedAtStop::WallBandIdentityBoundary
        && lookup.scratch.before == scratch_before
        && lookup.scratch.after == Some(scratch_after)
        && next.instruction_va == MARKET_FIND_FRIENDS_AFTER_FIRST_LOOKUP_VA
        && next.function_va == BUILD_TYPE_FIND_FRIENDS_VA
        && next.request == find_friends.request
        && next.circle_offset == lookup.request.circle_offset
        && next.accumulator_before == 0
        && find_friends.effective_city_filter == Some(next.effective_city_filter)
        && lookup.returned == Some(next.returned)
        && lookup.returned_owner == next.returned_owner
        && ((next.returned == -1 && next.returned_owner.is_none())
            || (next.returned >= 0 && next.returned_owner == Some(find_friends.request.owner)))
}

fn first_lookup_from_market(
    market: &GoldenStartingMarketCityReceipt,
) -> GoldenStartingMarketFirstObjectLookupReceipt {
    GoldenStartingMarketFirstObjectLookupReceipt {
        accepted: GoldenStartingMarketAcceptedPlacementReceipt {
            placement: market.placement.clone(),
            blocked_location: market.blocked_location.clone(),
            find_friends: market.find_friends.clone(),
            before_sim_sha256: market.before_sim_sha256,
            source_produced_city_bytes: 0,
            installed_in_scoreboard: false,
        },
        object_lookup: market.first_object_lookup.clone(),
        scratch_before: market.objects_selected_owner_before,
        scratch_after: market.objects_selected_owner_after,
        next: market.find_friends_after_first_lookup,
        source_produced_city_bytes: 0,
        installed_in_scoreboard: false,
    }
}

fn ring_first_lookup(
    advance: &GoldenStartingMarketFindFriendsRingAdvance,
) -> &GoldenStartingMarketFirstObjectLookupReceipt {
    match advance {
        GoldenStartingMarketFindFriendsRingAdvance::ReturnedZero(receipt) => &receipt.first_lookup,
        GoldenStartingMarketFindFriendsRingAdvance::FoundBuildTypeBoundary(receipt) => {
            &receipt.first_lookup
        }
        GoldenStartingMarketFindFriendsRingAdvance::WallBandIdentityBoundary(receipt) => {
            &receipt.first_lookup
        }
    }
}

fn found_read_matches_lookup(
    found: GoldenStartingMarketFindFriendsFoundBuildRead,
    lookup: &ObjectsFindBuildingPlacedAtReceipt,
    effective_city_filter: i32,
) -> bool {
    lookup.returned == Some(found.object)
        && lookup.returned_owner == Some(found.owner)
        && found.effective_city_filter == effective_city_filter
        && found.object >= 0
        && found.owner >= 0
}

fn append_found_build_read(
    image: &mut Vec<u8>,
    found: GoldenStartingMarketFindFriendsFoundBuildRead,
) {
    image.extend_from_slice(&found.owner.to_le_bytes());
    image.extend_from_slice(&found.object.to_le_bytes());
    image.extend_from_slice(&(found.build_row as u64).to_le_bytes());
    image.extend_from_slice(&found.type_index.to_le_bytes());
    image.extend_from_slice(&found.city.to_le_bytes());
    image.extend_from_slice(&found.effective_city_filter.to_le_bytes());
}

fn append_ring_step(image: &mut Vec<u8>, step: &GoldenStartingMarketFindFriendsRingStep) {
    image.extend_from_slice(&step.circle_offset.to_le_bytes());
    for value in step.world_cell {
        image.extend_from_slice(&value.to_le_bytes());
    }
    match step.child {
        Some(child) => {
            image.push(1);
            append_object_lookup_request(image, child);
        }
        None => image.push(0),
    }
    match &step.object_lookup {
        Some(lookup) => {
            image.push(1);
            image.extend_from_slice(&golden_market_first_object_lookup_receipt_sha256(lookup));
        }
        None => image.push(0),
    }
    match step.city_mismatch {
        Some(found) => {
            image.push(1);
            append_found_build_read(image, found);
        }
        None => image.push(0),
    }
    image.extend_from_slice(&step.accumulator_before.to_le_bytes());
    image.extend_from_slice(&step.accumulator_after.to_le_bytes());
}

/// Stable identity for the entire bounded ring result, including every lookup receipt and
/// `ObjectsData+0x200` authority transition.
pub fn golden_market_find_friends_ring_advance_sha256(
    advance: &GoldenStartingMarketFindFriendsRingAdvance,
) -> [u8; 32] {
    let mut image = b"don-2024-golden-market-find-friends-ring-advance-v1".to_vec();
    let first = ring_first_lookup(advance);
    image.extend_from_slice(&golden_market_blocked_location_receipt_sha256(
        &first.accepted.blocked_location,
    ));
    image.extend_from_slice(&golden_market_find_friends_receipt_sha256(
        &first.accepted.find_friends,
    ));
    image.extend_from_slice(&golden_market_first_object_lookup_receipt_sha256(
        &first.object_lookup,
    ));
    append_selected_owner_authority(&mut image, first.scratch_before);
    append_selected_owner_authority(&mut image, first.scratch_after);
    image.extend_from_slice(&golden_market_find_friends_after_first_lookup_sha256(
        &first.next,
    ));
    image.extend_from_slice(&first.accepted.before_sim_sha256);
    image.extend_from_slice(&first.accepted.source_produced_city_bytes.to_le_bytes());
    image.push(u8::from(first.accepted.installed_in_scoreboard));
    image.extend_from_slice(&first.source_produced_city_bytes.to_le_bytes());
    image.push(u8::from(first.installed_in_scoreboard));

    match advance {
        GoldenStartingMarketFindFriendsRingAdvance::ReturnedZero(receipt) => {
            image.push(1);
            append_selected_owner_authority(&mut image, receipt.scratch_entry);
            image.extend_from_slice(&(receipt.remaining_steps.len() as u64).to_le_bytes());
            for step in &receipt.remaining_steps {
                append_ring_step(&mut image, step);
            }
            append_selected_owner_authority(&mut image, receipt.scratch_after);
            image.extend_from_slice(&receipt.returned.to_le_bytes());
            image.extend_from_slice(&receipt.source_produced_city_bytes.to_le_bytes());
            image.push(u8::from(receipt.installed_in_scoreboard));
        }
        GoldenStartingMarketFindFriendsRingAdvance::FoundBuildTypeBoundary(receipt) => {
            image.push(2);
            append_selected_owner_authority(&mut image, receipt.scratch_entry);
            image.extend_from_slice(&(receipt.completed_steps.len() as u64).to_le_bytes());
            for step in &receipt.completed_steps {
                append_ring_step(&mut image, step);
            }
            append_selected_owner_authority(&mut image, receipt.scratch_at_boundary);
            image.extend_from_slice(&golden_market_first_object_lookup_receipt_sha256(
                &receipt.object_lookup,
            ));
            append_found_build_read(&mut image, receipt.found);
            image.extend_from_slice(&receipt.accumulator_before.to_le_bytes());
            image.extend_from_slice(&receipt.first_unowned_virtual_slot.to_le_bytes());
            image.extend_from_slice(&receipt.source_produced_city_bytes.to_le_bytes());
            image.push(u8::from(receipt.installed_in_scoreboard));
        }
        GoldenStartingMarketFindFriendsRingAdvance::WallBandIdentityBoundary(receipt) => {
            image.push(3);
            append_selected_owner_authority(&mut image, receipt.scratch_entry);
            image.extend_from_slice(&(receipt.completed_steps.len() as u64).to_le_bytes());
            for step in &receipt.completed_steps {
                append_ring_step(&mut image, step);
            }
            append_selected_owner_authority(&mut image, receipt.scratch_at_boundary);
            image.extend_from_slice(&golden_market_first_object_lookup_receipt_sha256(
                &receipt.object_lookup,
            ));
            image.extend_from_slice(&receipt.wall_boundary.instruction_va.to_le_bytes());
            image.push(receipt.wall_boundary.cell_offset_index);
            append_objects_spatial_key(&mut image, receipt.wall_boundary.key);
            image.extend_from_slice(&(receipt.wall_boundary.row as u64).to_le_bytes());
            image.extend_from_slice(&receipt.wall_boundary.wall_stored_o.to_le_bytes());
            image.extend_from_slice(&receipt.wall_boundary.required_banded_o.to_le_bytes());
            image.extend_from_slice(&receipt.accumulator_before.to_le_bytes());
            image.extend_from_slice(&receipt.source_produced_city_bytes.to_le_bytes());
            image.push(u8::from(receipt.installed_in_scoreboard));
        }
    }
    sha256(&image)
}

fn append_found_build_reject_reason(
    image: &mut Vec<u8>,
    reason: GoldenStartingMarketFoundBuildRejectReason,
) {
    match reason {
        GoldenStartingMarketFoundBuildRejectReason::GatherTypeOutsideUniversity => image.push(1),
        GoldenStartingMarketFoundBuildRejectReason::GatherEnhancer { relation_type } => {
            image.push(2);
            image.extend_from_slice(&relation_type.to_le_bytes());
        }
        GoldenStartingMarketFoundBuildRejectReason::UncapturedMilitaryTrainer => image.push(3),
        GoldenStartingMarketFoundBuildRejectReason::Wonder => image.push(4),
    }
}

/// Stable identity for the complete lazy found-Build predicate receipt and its typed
/// next-ring/return continuation.
pub fn golden_market_found_build_predicate_receipt_sha256(
    receipt: &GoldenStartingMarketFoundBuildPredicateReceipt,
) -> [u8; 32] {
    let mut image = b"don-2024-golden-market-found-build-predicates-v1".to_vec();
    image.extend_from_slice(&golden_market_find_friends_ring_advance_sha256(
        &GoldenStartingMarketFindFriendsRingAdvance::FoundBuildTypeBoundary(receipt.input.clone()),
    ));
    let reads = &receipt.reads;
    image.extend_from_slice(&(reads.type_rows as u64).to_le_bytes());
    image.extend_from_slice(&reads.mutation_revision.to_le_bytes());
    image.extend_from_slice(&reads.found_type_index.to_le_bytes());
    image.extend_from_slice(&reads.found_build_flags.to_le_bytes());
    image.push(u8::from(reads.is_gather_type));
    append_option_bool(&mut image, reads.university_relation);
    for relation in reads.gather_enhancer_relations {
        append_option_bool(&mut image, relation);
    }
    match reads.basic_type_call_va {
        Some(call_va) => {
            image.push(1);
            image.extend_from_slice(&call_va.to_le_bytes());
        }
        None => image.push(0),
    }
    image.extend_from_slice(&(reads.basic_type_chain.len() as u64).to_le_bytes());
    for type_index in &reads.basic_type_chain {
        image.extend_from_slice(&type_index.to_le_bytes());
    }
    append_option_i32(&mut image, reads.basic_type);
    match reads.basic_type_build_flags {
        Some(flags) => {
            image.push(1);
            image.extend_from_slice(&flags.to_le_bytes());
        }
        None => image.push(0),
    }
    append_option_bool(&mut image, reads.military_trainer);
    append_option_bool(&mut image, reads.captured);
    match reads.wonder_virtual_slot {
        Some(slot) => {
            image.push(1);
            image.extend_from_slice(&slot.to_le_bytes());
        }
        None => image.push(0),
    }
    append_option_bool(&mut image, reads.is_wonder_type);
    match receipt.outcome {
        GoldenStartingMarketFoundBuildPredicateOutcome::Rejected(reason) => {
            image.push(1);
            append_found_build_reject_reason(&mut image, reason);
        }
        GoldenStartingMarketFoundBuildPredicateOutcome::Counted { weight } => {
            image.push(2);
            image.extend_from_slice(&weight.to_le_bytes());
        }
    }
    image.extend_from_slice(&receipt.accumulator_before.to_le_bytes());
    image.extend_from_slice(&receipt.accumulator_after.to_le_bytes());
    match receipt.continuation {
        GoldenStartingMarketAfterFoundBuildContinuation::NextRingOffset {
            circle_offset,
            accumulator,
        } => {
            image.push(1);
            image.extend_from_slice(&circle_offset.to_le_bytes());
            image.extend_from_slice(&accumulator.to_le_bytes());
        }
        GoldenStartingMarketAfterFoundBuildContinuation::Returned { value } => {
            image.push(2);
            image.extend_from_slice(&value.to_le_bytes());
        }
    }
    image.extend_from_slice(&receipt.source_produced_city_bytes.to_le_bytes());
    image.push(u8::from(receipt.installed_in_scoreboard));
    sha256(&image)
}

fn found_build_predicate_shape_validates(
    receipt: &GoldenStartingMarketFoundBuildPredicateReceipt,
) -> bool {
    let input = &receipt.input;
    if input.first_unowned_virtual_slot != MARKET_FIND_FRIENDS_FIRST_FOUND_TYPE_VIRTUAL_SLOT
        || input.source_produced_city_bytes != 0
        || input.installed_in_scoreboard
        || receipt.reads.type_rows == 0
        || usize::try_from(receipt.reads.found_type_index)
            .ok()
            .is_none_or(|index| index >= receipt.reads.type_rows)
        || receipt.reads.found_type_index != input.found.type_index
        || receipt.reads.is_gather_type
            != (receipt.reads.found_build_flags & BUILD_GATHER_TYPE_MASK != 0)
        || receipt.reads.university_relation.is_some() != receipt.reads.is_gather_type
        || receipt.accumulator_before != input.accumulator_before
        || receipt.source_produced_city_bytes != 0
        || receipt.installed_in_scoreboard
    {
        return false;
    }
    let expected_after = match receipt.outcome {
        GoldenStartingMarketFoundBuildPredicateOutcome::Rejected(reason) => {
            let reason_matches = match reason {
                GoldenStartingMarketFoundBuildRejectReason::GatherTypeOutsideUniversity => {
                    receipt.reads.is_gather_type && receipt.reads.university_relation == Some(false)
                }
                GoldenStartingMarketFoundBuildRejectReason::GatherEnhancer { relation_type } => {
                    GATHER_ENHANCER_TYPES
                        .iter()
                        .position(|&candidate| candidate as i32 == relation_type)
                        .is_some_and(|index| {
                            receipt.reads.gather_enhancer_relations[index] == Some(true)
                                && receipt.reads.gather_enhancer_relations[..index]
                                    .iter()
                                    .all(|value| *value == Some(false))
                        })
                }
                GoldenStartingMarketFoundBuildRejectReason::UncapturedMilitaryTrainer => {
                    receipt.reads.military_trainer == Some(true)
                        && receipt.reads.captured == Some(false)
                }
                GoldenStartingMarketFoundBuildRejectReason::Wonder => {
                    receipt.reads.is_wonder_type == Some(true)
                }
            };
            if !reason_matches {
                return false;
            }
            receipt.accumulator_before
        }
        GoldenStartingMarketFoundBuildPredicateOutcome::Counted { weight } => {
            let expected_weight = if input.object_lookup.request.circle_offset & 1 == 0 {
                2
            } else {
                1
            };
            if weight != expected_weight || receipt.reads.is_wonder_type != Some(false) {
                return false;
            }
            receipt.accumulator_before.wrapping_add(weight)
        }
    };
    if receipt.accumulator_after != expected_after {
        return false;
    }
    match receipt.continuation {
        GoldenStartingMarketAfterFoundBuildContinuation::NextRingOffset {
            circle_offset,
            accumulator,
        } => {
            input.object_lookup.request.circle_offset < 8
                && circle_offset == input.object_lookup.request.circle_offset + 1
                && accumulator == expected_after
        }
        GoldenStartingMarketAfterFoundBuildContinuation::Returned { value } => {
            input.object_lookup.request.circle_offset == 8 && value == expected_after
        }
    }
}

fn append_found_build_type_reads(
    image: &mut Vec<u8>,
    reads: &GoldenStartingMarketFoundBuildTypeReads,
) {
    image.extend_from_slice(&(reads.type_rows as u64).to_le_bytes());
    image.extend_from_slice(&reads.mutation_revision.to_le_bytes());
    image.extend_from_slice(&reads.found_type_index.to_le_bytes());
    image.extend_from_slice(&reads.found_build_flags.to_le_bytes());
    image.push(u8::from(reads.is_gather_type));
    append_option_bool(image, reads.university_relation);
    for relation in reads.gather_enhancer_relations {
        append_option_bool(image, relation);
    }
    match reads.basic_type_call_va {
        Some(call_va) => {
            image.push(1);
            image.extend_from_slice(&call_va.to_le_bytes());
        }
        None => image.push(0),
    }
    image.extend_from_slice(&(reads.basic_type_chain.len() as u64).to_le_bytes());
    for type_index in &reads.basic_type_chain {
        image.extend_from_slice(&type_index.to_le_bytes());
    }
    append_option_i32(image, reads.basic_type);
    match reads.basic_type_build_flags {
        Some(flags) => {
            image.push(1);
            image.extend_from_slice(&flags.to_le_bytes());
        }
        None => image.push(0),
    }
    append_option_bool(image, reads.military_trainer);
    append_option_bool(image, reads.captured);
    match reads.wonder_virtual_slot {
        Some(slot) => {
            image.push(1);
            image.extend_from_slice(&slot.to_le_bytes());
        }
        None => image.push(0),
    }
    append_option_bool(image, reads.is_wonder_type);
}

fn append_found_build_predicate_outcome(
    image: &mut Vec<u8>,
    outcome: GoldenStartingMarketFoundBuildPredicateOutcome,
) {
    match outcome {
        GoldenStartingMarketFoundBuildPredicateOutcome::Rejected(reason) => {
            image.push(1);
            append_found_build_reject_reason(image, reason);
        }
        GoldenStartingMarketFoundBuildPredicateOutcome::Counted { weight } => {
            image.push(2);
            image.extend_from_slice(&weight.to_le_bytes());
        }
    }
}

fn complete_found_predicate_shape_validates(
    found: GoldenStartingMarketFindFriendsFoundBuildRead,
    circle_offset: i32,
    reads: &GoldenStartingMarketFoundBuildTypeReads,
    outcome: GoldenStartingMarketFoundBuildPredicateOutcome,
    accumulator_before: i32,
    accumulator_after: i32,
) -> bool {
    if reads.type_rows == 0
        || usize::try_from(reads.found_type_index)
            .ok()
            .is_none_or(|index| index >= reads.type_rows)
        || reads.found_type_index != found.type_index
        || reads.is_gather_type != (reads.found_build_flags & BUILD_GATHER_TYPE_MASK != 0)
        || reads.university_relation.is_some() != reads.is_gather_type
    {
        return false;
    }
    let expected_after = match outcome {
        GoldenStartingMarketFoundBuildPredicateOutcome::Rejected(reason) => {
            let reason_matches = match reason {
                GoldenStartingMarketFoundBuildRejectReason::GatherTypeOutsideUniversity => {
                    reads.is_gather_type && reads.university_relation == Some(false)
                }
                GoldenStartingMarketFoundBuildRejectReason::GatherEnhancer { relation_type } => {
                    GATHER_ENHANCER_TYPES
                        .iter()
                        .position(|&candidate| candidate as i32 == relation_type)
                        .is_some_and(|index| {
                            reads.gather_enhancer_relations[index] == Some(true)
                                && reads.gather_enhancer_relations[..index]
                                    .iter()
                                    .all(|value| *value == Some(false))
                        })
                }
                GoldenStartingMarketFoundBuildRejectReason::UncapturedMilitaryTrainer => {
                    reads.military_trainer == Some(true) && reads.captured == Some(false)
                }
                GoldenStartingMarketFoundBuildRejectReason::Wonder => {
                    reads.is_wonder_type == Some(true)
                }
            };
            if !reason_matches {
                return false;
            }
            accumulator_before
        }
        GoldenStartingMarketFoundBuildPredicateOutcome::Counted { weight } => {
            let expected_weight = if circle_offset & 1 == 0 { 2 } else { 1 };
            if weight != expected_weight || reads.is_wonder_type != Some(false) {
                return false;
            }
            accumulator_before.wrapping_add(weight)
        }
    };
    accumulator_after == expected_after
}

/// Stable identity for the complete exact multi-hit `find_friends` return.
pub fn golden_market_complete_find_friends_receipt_sha256(
    receipt: &GoldenStartingMarketFindFriendsCompleteReceipt,
) -> [u8; 32] {
    let mut image = b"don-2024-golden-market-complete-find-friends-v1".to_vec();
    let first = &receipt.first_lookup;
    image.extend_from_slice(&golden_market_blocked_location_receipt_sha256(
        &first.accepted.blocked_location,
    ));
    image.extend_from_slice(&golden_market_find_friends_receipt_sha256(
        &first.accepted.find_friends,
    ));
    image.extend_from_slice(&golden_market_first_object_lookup_receipt_sha256(
        &first.object_lookup,
    ));
    append_selected_owner_authority(&mut image, first.scratch_before);
    append_selected_owner_authority(&mut image, first.scratch_after);
    image.extend_from_slice(&golden_market_find_friends_after_first_lookup_sha256(
        &first.next,
    ));
    image.extend_from_slice(&first.accepted.before_sim_sha256);
    image.extend_from_slice(&first.accepted.source_produced_city_bytes.to_le_bytes());
    image.push(u8::from(first.accepted.installed_in_scoreboard));
    image.extend_from_slice(&first.source_produced_city_bytes.to_le_bytes());
    image.push(u8::from(first.installed_in_scoreboard));
    image.extend_from_slice(&(receipt.events.len() as u64).to_le_bytes());
    for event in &receipt.events {
        match event {
            GoldenStartingMarketFindFriendsOwnedRingEvent::OutOfBounds {
                circle_offset,
                world_cell,
                accumulator,
            } => {
                image.push(1);
                image.extend_from_slice(&circle_offset.to_le_bytes());
                for value in world_cell {
                    image.extend_from_slice(&value.to_le_bytes());
                }
                image.extend_from_slice(&accumulator.to_le_bytes());
            }
            GoldenStartingMarketFindFriendsOwnedRingEvent::Lookup(step) => {
                image.push(2);
                image.extend_from_slice(&step.circle_offset.to_le_bytes());
                for value in step.world_cell {
                    image.extend_from_slice(&value.to_le_bytes());
                }
                image.extend_from_slice(&golden_market_first_object_lookup_receipt_sha256(
                    &step.object_lookup,
                ));
                image.push(u8::from(step.committed_by_this_receipt));
                match &step.disposition {
                    GoldenStartingMarketFindFriendsLookupDisposition::Miss => image.push(1),
                    GoldenStartingMarketFindFriendsLookupDisposition::CityMismatch(found) => {
                        image.push(2);
                        append_found_build_read(&mut image, *found);
                    }
                    GoldenStartingMarketFindFriendsLookupDisposition::FoundBuildPredicate {
                        found,
                        reads,
                        outcome,
                    } => {
                        image.push(3);
                        append_found_build_read(&mut image, *found);
                        append_found_build_type_reads(&mut image, reads);
                        append_found_build_predicate_outcome(&mut image, *outcome);
                    }
                }
                image.extend_from_slice(&step.accumulator_before.to_le_bytes());
                image.extend_from_slice(&step.accumulator_after.to_le_bytes());
            }
        }
    }
    append_selected_owner_authority(&mut image, receipt.scratch_entry);
    append_selected_owner_authority(&mut image, receipt.scratch_after);
    image.extend_from_slice(&receipt.returned.to_le_bytes());
    image.extend_from_slice(&receipt.source_produced_city_bytes.to_le_bytes());
    image.push(u8::from(receipt.installed_in_scoreboard));
    sha256(&image)
}

fn complete_find_friends_receipt_shape_validates(
    receipt: &GoldenStartingMarketFindFriendsCompleteReceipt,
) -> bool {
    if !receipt.first_lookup.validates()
        || receipt.scratch_entry != receipt.first_lookup.scratch_after
        || receipt.source_produced_city_bytes != 0
        || receipt.installed_in_scoreboard
        || receipt.events.is_empty()
        || !(1..=8).contains(&receipt.first_lookup.next.circle_offset)
        || !matches!(
            receipt.events.first(),
            Some(GoldenStartingMarketFindFriendsOwnedRingEvent::Lookup(step))
                if step.object_lookup == receipt.first_lookup.object_lookup
        )
    {
        return false;
    }
    let city_filter = receipt.first_lookup.next.effective_city_filter;
    let first_offset = receipt.first_lookup.next.circle_offset;
    let mut expected_offset = first_offset;
    let mut accumulator = 0;
    let mut scratch = receipt.scratch_entry;
    for (ordinal, event) in receipt.events.iter().enumerate() {
        let (dx, dy) = WORLD_CELL_SEARCH_OFFSETS[expected_offset as usize];
        let expected_world_cell = [
            receipt.first_lookup.next.request.candidate_world_cell[0].wrapping_add(dx),
            receipt.first_lookup.next.request.candidate_world_cell[1].wrapping_add(dy),
        ];
        match event {
            GoldenStartingMarketFindFriendsOwnedRingEvent::OutOfBounds {
                circle_offset,
                world_cell,
                accumulator: event_accumulator,
            } => {
                if *circle_offset != expected_offset
                    || *world_cell != expected_world_cell
                    || *event_accumulator != accumulator
                {
                    return false;
                }
            }
            GoldenStartingMarketFindFriendsOwnedRingEvent::Lookup(step) => {
                if step.circle_offset != expected_offset
                    || step.world_cell != expected_world_cell
                    || step.world_cell != step.object_lookup.request.world_cell
                    || !step.object_lookup.validates()
                    || step.accumulator_before != accumulator
                    || step.committed_by_this_receipt != (ordinal != 0)
                {
                    return false;
                }
                if ordinal == 0 {
                    if step.object_lookup != receipt.first_lookup.object_lookup
                        || step.object_lookup.scratch.after != Some(scratch)
                    {
                        return false;
                    }
                } else {
                    if step.object_lookup.scratch.before != scratch {
                        return false;
                    }
                    let Some(after) = step.object_lookup.scratch.after else {
                        return false;
                    };
                    scratch = after;
                }
                let disposition_valid = match &step.disposition {
                    GoldenStartingMarketFindFriendsLookupDisposition::Miss => {
                        step.object_lookup.returned == Some(-1)
                            && step.object_lookup.returned_owner.is_none()
                            && step.accumulator_after == accumulator
                    }
                    GoldenStartingMarketFindFriendsLookupDisposition::CityMismatch(found) => {
                        found_read_matches_lookup(*found, &step.object_lookup, city_filter)
                            && i32::from(found.city) != city_filter
                            && step.accumulator_after == accumulator
                    }
                    GoldenStartingMarketFindFriendsLookupDisposition::FoundBuildPredicate {
                        found,
                        reads,
                        outcome,
                    } => {
                        found_read_matches_lookup(*found, &step.object_lookup, city_filter)
                            && i32::from(found.city) == city_filter
                            && complete_found_predicate_shape_validates(
                                *found,
                                step.circle_offset,
                                reads,
                                *outcome,
                                accumulator,
                                step.accumulator_after,
                            )
                    }
                };
                if !disposition_valid {
                    return false;
                }
                accumulator = step.accumulator_after;
            }
        }
        expected_offset = expected_offset.wrapping_add(1);
    }
    expected_offset == 9 && receipt.scratch_after == scratch && receipt.returned == accumulator
}

fn complete_find_friends_shape_validates(
    market: &GoldenStartingMarketCityReceipt,
    receipt: &GoldenStartingMarketFindFriendsCompleteReceipt,
) -> bool {
    receipt.first_lookup == first_lookup_from_market(market)
        && complete_find_friends_receipt_shape_validates(receipt)
        && market.objects_selected_owner_find_friends_after == receipt.scratch_after
        && market.find_friends_returned == receipt.returned
}

fn found_build_predicates_extend_ring(
    advance: &GoldenStartingMarketFindFriendsRingAdvance,
    predicates: Option<&GoldenStartingMarketFoundBuildPredicateReceipt>,
) -> bool {
    match (advance, predicates) {
        (
            GoldenStartingMarketFindFriendsRingAdvance::FoundBuildTypeBoundary(boundary),
            Some(receipt),
        ) => receipt.input == *boundary && found_build_predicate_shape_validates(receipt),
        (GoldenStartingMarketFindFriendsRingAdvance::FoundBuildTypeBoundary(_), None) => false,
        (_, None) => true,
        (_, Some(_)) => false,
    }
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

fn append_market_residual(image: &mut Vec<u8>, residual: &GoldenMarketResidual) {
    match residual {
        GoldenMarketResidual::CallerCoarseScore {
            find_friends_returned,
            coarse_score_authority_issued,
            conditional_world_find_authority_issued,
        } => {
            image.push(1);
            image.extend_from_slice(&find_friends_returned.to_le_bytes());
            image.extend_from_slice(&[
                u8::from(*coarse_score_authority_issued),
                u8::from(*conditional_world_find_authority_issued),
            ]);
        }
    }
}

pub fn golden_chronology_spine_digest(receipt: &GoldenChronologySpineReceipt) -> [u8; 32] {
    let mut image = b"don-2024-golden-chronology-spine-v5".to_vec();
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
            image.extend_from_slice(&residual.first_object_lookup_receipt_sha256);
            append_selected_owner_authority(&mut image, residual.objects_selected_owner_before);
            append_selected_owner_authority(&mut image, residual.objects_selected_owner_after);
            image.extend_from_slice(&residual.find_friends_after_first_lookup_sha256);
            image.extend_from_slice(&residual.complete_find_friends_sha256);
            append_selected_owner_authority(
                &mut image,
                residual.objects_selected_owner_find_friends_after,
            );
            image.extend_from_slice(&residual.find_friends_returned.to_le_bytes());
            append_market_residual(&mut image, &residual.market_residual);
            image.extend_from_slice(&residual.market_before_sim_sha256);
            image.extend_from_slice(&residual.post_market_oracle_sim_sha256);
            image.extend_from_slice(&residual.opaque_native_trace_sha256);
        }
    }
    image.push(u8::from(receipt.downstream_execution_authority_issued));
    sha256(&image)
}

/// Compose the strict Market join and the downstream oracle topology without issuing a false
/// execution authority. The complete `find_friends` return is retained exactly; the missing child
/// is the caller's coarse score, not a captured digest or guessed later checkpoint.
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
    let first_object_lookup = market.first_object_lookup.clone();
    let objects_selected_owner_before = market.objects_selected_owner_before;
    let objects_selected_owner_after = market.objects_selected_owner_after;
    let find_friends_after_first_lookup = market.find_friends_after_first_lookup;
    if !blocked_location.validates()
        || blocked_location.input != market.placement.blocked_location
        || !find_friends_extends_blocked_location(&find_friends, &blocked_location)
        || !first_object_lookup_extends_find_friends(
            &first_object_lookup,
            objects_selected_owner_before,
            objects_selected_owner_after,
            &find_friends_after_first_lookup,
            &find_friends,
        )
        || !complete_find_friends_shape_validates(market, &market.complete_find_friends)
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
    let first_object_lookup_receipt_sha256 =
        golden_market_first_object_lookup_receipt_sha256(&first_object_lookup);
    let find_friends_after_first_lookup_sha256 =
        golden_market_find_friends_after_first_lookup_sha256(&find_friends_after_first_lookup);
    let complete_find_friends_sha256 =
        golden_market_complete_find_friends_receipt_sha256(&market.complete_find_friends);
    let market_evidence_digest = golden_market_evidence_digest(
        market,
        blocked_location_receipt_sha256,
        find_friends_receipt_sha256,
        first_object_lookup_receipt_sha256,
        find_friends_after_first_lookup_sha256,
        complete_find_friends_sha256,
    );
    let market_residual = GoldenMarketResidual::CallerCoarseScore {
        find_friends_returned: market.find_friends_returned,
        coarse_score_authority_issued: false,
        conditional_world_find_authority_issued: false,
    };
    let first_missing_authority =
        GoldenChronologyFirstMissingAuthority::MarketScoringChild(GoldenMarketExecutionResidual {
            blocked_location_receipt_sha256,
            find_friends_receipt_sha256,
            first_object_lookup_receipt_sha256,
            objects_selected_owner_before,
            objects_selected_owner_after,
            find_friends_after_first_lookup_sha256,
            complete_find_friends: market.complete_find_friends.clone(),
            complete_find_friends_sha256,
            objects_selected_owner_find_friends_after: market
                .objects_selected_owner_find_friends_after,
            find_friends_returned: market.find_friends_returned,
            market_residual,
            market_before_sim_sha256: market.before_sim_sha256,
            post_market_oracle_sim_sha256: market.after_sim_sha256,
            opaque_native_trace_sha256: market.native_trace_sha256,
            blocked_location,
            find_friends,
            first_object_lookup,
            find_friends_after_first_lookup,
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
                && first_object_lookup_extends_find_friends(
                    &residual.first_object_lookup,
                    residual.objects_selected_owner_before,
                    residual.objects_selected_owner_after,
                    &residual.find_friends_after_first_lookup,
                    &residual.find_friends,
                )
                && residual.first_object_lookup_receipt_sha256
                    == golden_market_first_object_lookup_receipt_sha256(
                        &residual.first_object_lookup,
                    )
                && residual.find_friends_after_first_lookup_sha256
                    == golden_market_find_friends_after_first_lookup_sha256(
                        &residual.find_friends_after_first_lookup,
                    )
                && residual
                    .complete_find_friends
                    .first_lookup
                    .accepted
                    .blocked_location
                    == residual.blocked_location
                && residual
                    .complete_find_friends
                    .first_lookup
                    .accepted
                    .find_friends
                    == residual.find_friends
                && residual.complete_find_friends.first_lookup.object_lookup
                    == residual.first_object_lookup
                && residual.complete_find_friends.first_lookup.scratch_before
                    == residual.objects_selected_owner_before
                && residual.complete_find_friends.first_lookup.scratch_after
                    == residual.objects_selected_owner_after
                && residual.complete_find_friends.first_lookup.next
                    == residual.find_friends_after_first_lookup
                && residual.complete_find_friends.scratch_after
                    == residual.objects_selected_owner_find_friends_after
                && residual.complete_find_friends.returned == residual.find_friends_returned
                && complete_find_friends_receipt_shape_validates(
                    &residual.complete_find_friends,
                )
                && residual.complete_find_friends_sha256
                    == golden_market_complete_find_friends_receipt_sha256(
                        &residual.complete_find_friends,
                    )
                && residual.market_residual
                    == (GoldenMarketResidual::CallerCoarseScore {
                        find_friends_returned: residual.find_friends_returned,
                        coarse_score_authority_issued: false,
                        conditional_world_find_authority_issued: false,
                    })
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
/// globally earliest replay gap: the parent's Market caller coarse-score residual remains open.
/// The captured step-11 entry and all original eleven oracle nodes remain capture-only, and no
/// Unit actor is made reachable.
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

pub fn golden_frame0_team_terr_spine_digest(
    receipt: &GoldenFrame0TeamTerrSpineReceipt,
) -> [u8; 32] {
    let mut image = b"don-2024-golden-frame0-team-terr-spine-v4".to_vec();
    image.extend_from_slice(&receipt.revision.to_le_bytes());
    image.extend_from_slice(&receipt.schema_version.to_le_bytes());
    image.extend_from_slice(&receipt.parent_strategy_spine_digest);
    image.push(match receipt.call_entry_evidence {
        GoldenOracleEvidence::CaptureOnly => 1,
    });
    image.extend_from_slice(&receipt.call_entry_authority.composition_digest);
    image.extend_from_slice(
        &receipt
            .call_entry_authority
            .plan_strategy_entry_authority_digest,
    );
    image.extend_from_slice(&receipt.call_entry_authority.call_entry_sim_sha256);
    image.extend_from_slice(&receipt.call_entry_authority.expected_request_sha256);
    image.extend_from_slice(&receipt.call_entry_authority.local_prefix_digest);
    image.push(u8::from(
        receipt
            .call_entry_authority
            .local_prefix_preserved_input_projection,
    ));
    image.extend_from_slice(&receipt.call_entry_authority.game.composition_digest);
    image.extend_from_slice(&receipt.call_entry_authority.leader.composition_digest);
    image.push(match receipt.child_evidence {
        GoldenDetachedChildEvidence::SourceExactDetached => 1,
    });
    image.extend_from_slice(&receipt.child.composition_digest);
    image.extend_from_slice(&receipt.child.request.request_sha256);
    image.extend_from_slice(&receipt.child.result.to_le_bytes());
    image.push(match receipt.continuation_evidence {
        GoldenDetachedChildEvidence::SourceExactDetached => 1,
    });
    image.extend_from_slice(&receipt.continuation.composition_digest);
    image.extend_from_slice(&receipt.continuation.open.request_sha256);
    image.extend_from_slice(&receipt.continuation.open.read_va.to_le_bytes());
    image.extend_from_slice(&receipt.continuation.open.candidate_who.to_le_bytes());
    image.push(u8::from(receipt.continuation.owner0_strategy_complete));
    image.push(match receipt.diplomacy_read_evidence {
        GoldenOracleEvidence::CaptureOnly => 1,
    });
    image.extend_from_slice(&receipt.diplomacy_read.composition_digest);
    image.extend_from_slice(&receipt.diplomacy_read.request_sha256);
    image.extend_from_slice(&receipt.diplomacy_read.value.to_le_bytes());
    image.push(match receipt.diplomacy_step_evidence {
        GoldenDetachedChildEvidence::SourceExactDetached => 1,
    });
    image.extend_from_slice(&receipt.diplomacy_step.composition_digest);
    match &receipt.diplomacy_step.open {
        crate::setup_2024_frame0_get_team_terr::Frame0PlanStrategyDiplomacyStepOpen::ReverseRead(
            request,
        ) => {
            image.push(1);
            image.extend_from_slice(&request.request_sha256);
        }
        crate::setup_2024_frame0_get_team_terr::Frame0PlanStrategyDiplomacyStepOpen::OpponentGetTeamTerr(
            request,
        ) => {
            image.push(2);
            image.extend_from_slice(&request.request_sha256);
        }
    }
    image.push(u8::from(receipt.diplomacy_step.owner0_strategy_complete));
    match &receipt.diplomacy_drain {
        GoldenFrame0DiplomacyDrain::ReverseReadCapture(authority) => {
            image.push(1);
            image.extend_from_slice(&authority.composition_digest);
            image.extend_from_slice(&authority.request_sha256);
            image.extend_from_slice(&authority.value.to_le_bytes());
        }
        GoldenFrame0DiplomacyDrain::OpponentTeamTerrSourceExact(opponent) => {
            image.push(2);
            image.extend_from_slice(&opponent.composition_digest);
            image.extend_from_slice(&opponent.request.request_sha256);
            image.extend_from_slice(&opponent.result.to_le_bytes());
            image.extend_from_slice(&opponent.return_va.to_le_bytes());
        }
    }
    match &receipt.diplomacy_residual {
        GoldenFrame0DiplomacyResidual::AfterReverseRead {
            value,
            reverse_relation_is_ally,
            repeated_child_call_va_if_not_ally,
            repeated_child_authority_issued,
            owner0_strategy_complete,
        } => {
            image.push(1);
            image.extend_from_slice(&value.to_le_bytes());
            image.push(u8::from(*reverse_relation_is_ally));
            image.extend_from_slice(&repeated_child_call_va_if_not_ally.to_le_bytes());
            image.extend_from_slice(&[
                u8::from(*repeated_child_authority_issued),
                u8::from(*owner0_strategy_complete),
            ]);
        }
        GoldenFrame0DiplomacyResidual::AfterOpponentTeamTerrReturn {
            result,
            return_va,
            post_return_store_authority_issued,
            owner0_strategy_complete,
        } => {
            image.push(2);
            image.extend_from_slice(&result.to_le_bytes());
            image.extend_from_slice(&return_va.to_le_bytes());
            image.extend_from_slice(&[
                u8::from(*post_return_store_authority_issued),
                u8::from(*owner0_strategy_complete),
            ]);
        }
    }
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

fn resolve_strategy_team_territory_child(
    strategy: &GoldenFrame0StrategySpineReceipt,
    authority: &Frame0GetTeamTerrCallEntryAuthority,
) -> Result<Frame0GetTeamTerrReceipt, GoldenChronologySpineError> {
    if authority.revision != strategy.entry_authority.revision
        || authority.composition_digest == [0; 32]
        || authority.composition_digest
            != frame0_get_team_terr_call_entry_authority_digest(authority)
        || authority.plan_strategy_entry_authority_digest
            != strategy.entry_authority.composition_digest
        || authority.call_entry_sim_sha256 != strategy.entry_authority.capture.call_entry_sim_sha256
        || authority.expected_request_sha256 != strategy.local_prefix.open.request_sha256
        || authority.local_prefix_digest != strategy.local_prefix.local_prefix_digest
        || !authority.local_prefix_preserved_input_projection
        || authority.replay_file_sha256 != strategy.entry_authority.capture.replay_file_sha256
        || authority.executable_sha256 != strategy.entry_authority.capture.executable_sha256
    {
        return Err(GoldenChronologySpineError::InvalidTeamTerrCallEntryAuthority);
    }
    resolve_captured_frame0_get_team_terr(&strategy.local_prefix.open, authority)
        .map_err(GoldenChronologySpineError::TeamTerr)
}

fn derive_frame0_diplomacy_drain(
    strategy: &GoldenFrame0StrategySpineReceipt,
    call_entry: &Frame0GetTeamTerrCallEntryAuthority,
    first_child: &Frame0GetTeamTerrReceipt,
    step: &Frame0PlanStrategyDiplomacyStepPlan,
    reverse_read: Option<&Frame0PlanStrategyReverseDiplomacyReadAuthority>,
) -> Result<(GoldenFrame0DiplomacyDrain, GoldenFrame0DiplomacyResidual), GoldenChronologySpineError>
{
    match &step.open {
        Frame0PlanStrategyDiplomacyStepOpen::ReverseRead(request) => {
            let authority =
                reverse_read.ok_or(GoldenChronologySpineError::InvalidDiplomacyDrain)?;
            if authority.revision != step.revision
                || authority.composition_digest == [0; 32]
                || authority.source != step.first_read.source
                || authority.replay_file_sha256 != call_entry.replay_file_sha256
                || authority.executable_sha256 != call_entry.executable_sha256
                || authority.call_entry_authority_digest != call_entry.composition_digest
                || authority.diplomacy_step_digest != step.composition_digest
                || authority.request_sha256 != request.request_sha256
                || authority.call_entry_sim_sha256 != request.call_entry_sim_sha256
                || authority.receiver_leader_slot != request.receiver_leader_slot
                || authority.receiver_who != request.receiver_who
                || authority.candidate_leader_slot != request.candidate_leader_slot
                || authority.candidate_who != request.candidate_who
                || authority.target_who_index != request.target_who_index
                || authority.read_va != request.read_va
                || authority.composition_digest
                    != frame0_plan_strategy_reverse_diplomacy_read_authority_digest(authority)
            {
                return Err(GoldenChronologySpineError::InvalidDiplomacyDrain);
            }
            Ok((
                GoldenFrame0DiplomacyDrain::ReverseReadCapture(authority.clone()),
                GoldenFrame0DiplomacyResidual::AfterReverseRead {
                    value: authority.value,
                    reverse_relation_is_ally: authority.value == request.ally_value,
                    repeated_child_call_va_if_not_ally: request.get_team_terr_call_va_if_not_ally,
                    repeated_child_authority_issued: false,
                    owner0_strategy_complete: false,
                },
            ))
        }
        Frame0PlanStrategyDiplomacyStepOpen::OpponentGetTeamTerr(_) => {
            if reverse_read.is_some() {
                return Err(GoldenChronologySpineError::InvalidDiplomacyDrain);
            }
            let opponent = resolve_frame0_plan_strategy_opponent_get_team_terr(
                &strategy.entry_authority,
                call_entry,
                first_child,
                step,
            )?;
            Ok((
                GoldenFrame0DiplomacyDrain::OpponentTeamTerrSourceExact(opponent.clone()),
                GoldenFrame0DiplomacyResidual::AfterOpponentTeamTerrReturn {
                    result: opponent.result,
                    return_va: opponent.return_va,
                    post_return_store_authority_issued: false,
                    owner0_strategy_complete: false,
                },
            ))
        }
    }
}

/// Attach the strict eight-Player/eight-Leader native plan-entry projection and evaluate only
/// owner zero's detached `get_team_terr` child.
///
/// The call-entry image remains capture-only. The local strategy prefix proves its own stores do
/// not touch this projection. After the child returns, the three team-territory stores and the
/// receiver-self skip are source-exact. One capture-bound directional diplomacy value selects the
/// exact reverse-read/repeated-child branch; the corresponding exact authority/receipt is retained.
/// It still cannot complete owner zero's native `plan_strategy`, begin owner one, or make any Unit
/// actor reachable.
pub fn compose_golden_frame0_team_terr_spine(
    parent: &GoldenChronologySpineReceipt,
    setup: &Frame379SetupReceipt,
    strategy: &GoldenFrame0StrategySpineReceipt,
    authority: &Frame0GetTeamTerrCallEntryAuthority,
    diplomacy_read: &Frame0PlanStrategyDiplomacyReadAuthority,
    reverse_read: Option<&Frame0PlanStrategyReverseDiplomacyReadAuthority>,
) -> Result<GoldenFrame0TeamTerrSpineReceipt, GoldenChronologySpineError> {
    validate_golden_frame0_strategy_spine(parent, setup, strategy)?;
    let child = resolve_strategy_team_territory_child(strategy, authority)?;
    let continuation =
        plan_frame0_owner0_after_get_team_terr(&strategy.entry_authority, authority, &child)?;
    let diplomacy_step =
        advance_frame0_plan_strategy_first_diplomacy_read(&continuation, diplomacy_read)?;
    let (diplomacy_drain, diplomacy_residual) =
        derive_frame0_diplomacy_drain(strategy, authority, &child, &diplomacy_step, reverse_read)?;
    let mut receipt = GoldenFrame0TeamTerrSpineReceipt {
        revision: authority.revision,
        composition_digest: [0; 32],
        schema_version: GOLDEN_FRAME0_TEAM_TERR_SPINE_SCHEMA_VERSION,
        parent_strategy_spine_digest: strategy.composition_digest,
        call_entry_evidence: GoldenOracleEvidence::CaptureOnly,
        call_entry_authority: authority.clone(),
        child_evidence: GoldenDetachedChildEvidence::SourceExactDetached,
        child,
        continuation_evidence: GoldenDetachedChildEvidence::SourceExactDetached,
        continuation,
        diplomacy_read_evidence: GoldenOracleEvidence::CaptureOnly,
        diplomacy_read: diplomacy_read.clone(),
        diplomacy_step_evidence: GoldenDetachedChildEvidence::SourceExactDetached,
        diplomacy_step,
        diplomacy_drain,
        diplomacy_residual,
        global_market_frontier_retained: true,
        reachability: GoldenFrame0StrategyReachability::default(),
    };
    receipt.composition_digest = golden_frame0_team_terr_spine_digest(&receipt);
    Ok(receipt)
}

pub fn validate_golden_frame0_team_terr_spine(
    parent: &GoldenChronologySpineReceipt,
    setup: &Frame379SetupReceipt,
    strategy: &GoldenFrame0StrategySpineReceipt,
    receipt: &GoldenFrame0TeamTerrSpineReceipt,
) -> Result<(), GoldenChronologySpineError> {
    validate_golden_frame0_strategy_spine(parent, setup, strategy)?;
    if receipt.revision == 0
        || receipt.composition_digest == [0; 32]
        || receipt.schema_version != GOLDEN_FRAME0_TEAM_TERR_SPINE_SCHEMA_VERSION
        || receipt.parent_strategy_spine_digest != strategy.composition_digest
        || receipt.call_entry_evidence != GoldenOracleEvidence::CaptureOnly
        || receipt.child_evidence != GoldenDetachedChildEvidence::SourceExactDetached
        || receipt.continuation_evidence != GoldenDetachedChildEvidence::SourceExactDetached
        || receipt.diplomacy_read_evidence != GoldenOracleEvidence::CaptureOnly
        || receipt.diplomacy_step_evidence != GoldenDetachedChildEvidence::SourceExactDetached
        || !receipt.global_market_frontier_retained
    {
        return Err(GoldenChronologySpineError::InvalidDetachedTeamTerrChild);
    }
    if receipt.reachability != GoldenFrame0StrategyReachability::default() {
        return Err(GoldenChronologySpineError::StrategyReachabilityOverclaim);
    }
    let expected = resolve_strategy_team_territory_child(strategy, &receipt.call_entry_authority)?;
    let expected_continuation = plan_frame0_owner0_after_get_team_terr(
        &strategy.entry_authority,
        &receipt.call_entry_authority,
        &expected,
    )?;
    let expected_diplomacy_step = advance_frame0_plan_strategy_first_diplomacy_read(
        &expected_continuation,
        &receipt.diplomacy_read,
    )?;
    let reverse_read = match &receipt.diplomacy_drain {
        GoldenFrame0DiplomacyDrain::ReverseReadCapture(authority) => Some(authority),
        GoldenFrame0DiplomacyDrain::OpponentTeamTerrSourceExact(_) => None,
    };
    let (expected_drain, expected_residual) = derive_frame0_diplomacy_drain(
        strategy,
        &receipt.call_entry_authority,
        &expected,
        &expected_diplomacy_step,
        reverse_read,
    )?;
    if receipt.revision != receipt.call_entry_authority.revision
        || expected != receipt.child
        || receipt.child.composition_digest != frame0_get_team_terr_receipt_digest(&receipt.child)
        || expected_continuation != receipt.continuation
        || receipt.continuation.composition_digest
            != frame0_plan_strategy_post_team_terr_digest(&receipt.continuation)
        || receipt.continuation.owner0_strategy_complete
        || receipt.diplomacy_read.composition_digest
            != frame0_plan_strategy_diplomacy_read_authority_digest(&receipt.diplomacy_read)
        || expected_diplomacy_step != receipt.diplomacy_step
        || receipt.diplomacy_step.composition_digest
            != frame0_plan_strategy_diplomacy_step_digest(&receipt.diplomacy_step)
        || receipt.diplomacy_step.owner0_strategy_complete
        || expected_drain != receipt.diplomacy_drain
        || expected_residual != receipt.diplomacy_residual
        || matches!(
            &receipt.diplomacy_drain,
            GoldenFrame0DiplomacyDrain::OpponentTeamTerrSourceExact(opponent)
                if opponent.composition_digest
                    != frame0_plan_strategy_opponent_get_team_terr_receipt_digest(opponent)
                    || opponent.owner0_strategy_complete
        )
        || receipt.composition_digest != golden_frame0_team_terr_spine_digest(receipt)
    {
        return Err(GoldenChronologySpineError::InvalidDetachedTeamTerrChild);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use don_sim::systems::map_terrain::{SectionDigest, WorldChecksum, WorldSection};

    use crate::setup_2024_frame0_get_team_terr::{
        frame0_get_team_terr_game_projection_digest, frame0_get_team_terr_leader_projection_digest,
        Frame0GetTeamTerrCallEntrySource, Frame0GetTeamTerrGameProjection,
        Frame0GetTeamTerrGameSource, Frame0GetTeamTerrLeaderProjection, Frame0GetTeamTerrLeaderRow,
        Frame0GetTeamTerrLeaderSource, Frame0GetTeamTerrPlayerRow,
        Frame0PlanStrategyDiplomacyReadSource, Frame0PlanStrategyTeamTerritoryPreimage,
    };
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

        let strategy = receipt.clone();
        let mut game = Frame0GetTeamTerrGameProjection {
            revision: strategy.revision,
            composition_digest: [0; 32],
            source: Frame0GetTeamTerrGameSource::SourceBackedGameInfoPlayerTeamProjection,
            frame: GOLDEN_FRAME,
            team_style: 0,
            players: std::array::from_fn(|slot| Frame0GetTeamTerrPlayerRow {
                flags: 1,
                who: slot as u8,
                team: if slot < 2 { 0 } else { 1 },
            }),
        };
        game.composition_digest = frame0_get_team_terr_game_projection_digest(&game);
        let mut leader = Frame0GetTeamTerrLeaderProjection {
            revision: strategy.revision,
            composition_digest: [0; 32],
            source: Frame0GetTeamTerrLeaderSource::CompleteRetailGetTeamTerrCallEntry,
            call_entry_sim_sha256: strategy.entry_authority.capture.call_entry_sim_sha256,
            leaders: std::array::from_fn(|slot| Frame0GetTeamTerrLeaderRow {
                leader_flags: if slot == 0 { 0x7 } else { 0x3 },
                who: slot as i32,
                territory: slot as i32 + 1,
            }),
        };
        leader.composition_digest = frame0_get_team_terr_leader_projection_digest(&leader);
        let mut call_entry = Frame0GetTeamTerrCallEntryAuthority {
            revision: strategy.revision,
            composition_digest: [0; 32],
            source: Frame0GetTeamTerrCallEntrySource::CompleteRetailOwnerZeroPlanStrategyEntry,
            native_trace_sha256: strategy.entry_authority.capture.native_trace_sha256,
            replay_file_sha256: REPLAY_FILE_SHA256,
            executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
            plan_strategy_entry_authority_digest: strategy.entry_authority.composition_digest,
            call_entry_sim_sha256: strategy.entry_authority.capture.call_entry_sim_sha256,
            expected_request_sha256: strategy.local_prefix.open.request_sha256,
            local_prefix_digest: strategy.local_prefix.local_prefix_digest,
            local_prefix_preserved_input_projection: true,
            frame: GOLDEN_FRAME,
            step: GOLDEN_STEP,
            owner: GOLDEN_FIRST_STRATEGY_OWNER,
            strategy_ordinal: GOLDEN_FIRST_STRATEGY_ORDINAL,
            team_territory_preimage: Frame0PlanStrategyTeamTerritoryPreimage {
                other_team_terr: 91,
                min_other_team_terr: 92,
                my_team_terr: 93,
            },
            game,
            leader,
        };
        call_entry.composition_digest =
            frame0_get_team_terr_call_entry_authority_digest(&call_entry);
        let child = resolve_strategy_team_territory_child(&strategy, &call_entry).unwrap();
        assert_eq!(child.result, 3);
        let continuation =
            plan_frame0_owner0_after_get_team_terr(&strategy.entry_authority, &call_entry, &child)
                .unwrap();
        assert_eq!(continuation.after_local.my_team_terr, 3);
        assert_eq!(continuation.open.read_va, 0x006b_9910);
        assert!(!continuation.owner0_strategy_complete);
        let mut diplomacy_read = Frame0PlanStrategyDiplomacyReadAuthority {
            revision: call_entry.revision,
            composition_digest: [0; 32],
            source: Frame0PlanStrategyDiplomacyReadSource::CompleteRetailPlanStrategyCallEntry,
            replay_file_sha256: REPLAY_FILE_SHA256,
            executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
            call_entry_authority_digest: call_entry.composition_digest,
            staged_plan_digest: continuation.composition_digest,
            request_sha256: continuation.open.request_sha256,
            call_entry_sim_sha256: call_entry.call_entry_sim_sha256,
            candidate_leader_slot: continuation.open.candidate_leader_slot,
            candidate_who: continuation.open.candidate_who,
            target_who_index: continuation.open.target_who_index,
            read_va: continuation.open.read_va,
            value: 0,
        };
        diplomacy_read.composition_digest =
            frame0_plan_strategy_diplomacy_read_authority_digest(&diplomacy_read);
        let diplomacy_step =
            advance_frame0_plan_strategy_first_diplomacy_read(&continuation, &diplomacy_read)
                .unwrap();
        assert!(matches!(
            &diplomacy_step.open,
            Frame0PlanStrategyDiplomacyStepOpen::OpponentGetTeamTerr(_)
        ));
        assert!(!diplomacy_step.owner0_strategy_complete);
        let (diplomacy_drain, diplomacy_residual) =
            derive_frame0_diplomacy_drain(&strategy, &call_entry, &child, &diplomacy_step, None)
                .unwrap();
        assert!(matches!(
            &diplomacy_drain,
            GoldenFrame0DiplomacyDrain::OpponentTeamTerrSourceExact(_)
        ));

        let mut team_terr = GoldenFrame0TeamTerrSpineReceipt {
            revision: call_entry.revision,
            composition_digest: [0; 32],
            schema_version: GOLDEN_FRAME0_TEAM_TERR_SPINE_SCHEMA_VERSION,
            parent_strategy_spine_digest: strategy.composition_digest,
            call_entry_evidence: GoldenOracleEvidence::CaptureOnly,
            call_entry_authority: call_entry,
            child_evidence: GoldenDetachedChildEvidence::SourceExactDetached,
            child,
            continuation_evidence: GoldenDetachedChildEvidence::SourceExactDetached,
            continuation,
            diplomacy_read_evidence: GoldenOracleEvidence::CaptureOnly,
            diplomacy_read,
            diplomacy_step_evidence: GoldenDetachedChildEvidence::SourceExactDetached,
            diplomacy_step,
            diplomacy_drain,
            diplomacy_residual,
            global_market_frontier_retained: true,
            reachability: GoldenFrame0StrategyReachability::default(),
        };
        team_terr.composition_digest = golden_frame0_team_terr_spine_digest(&team_terr);
        let team_digest = team_terr.composition_digest;
        team_terr.reachability.owner0_strategy_complete = true;
        assert_ne!(
            team_digest,
            golden_frame0_team_terr_spine_digest(&team_terr)
        );

        let digest = receipt.composition_digest;
        receipt.reachability.scout_reachable = true;
        assert_ne!(digest, golden_frame0_strategy_spine_digest(&receipt));
    }
}
