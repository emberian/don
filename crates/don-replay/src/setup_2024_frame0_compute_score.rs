// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact local prefix of owner zero's first `Leader::compute_score(0)` call.
//!
//! Retail reaches this call only after the same Leader's complete frame-zero
//! `plan_strategy` return. The planner remains red, so this module accepts only an
//! independently captured native return/call-entry authority joined to the existing
//! planner-prefix receipt. It never relabels the detached local planner prefix as a
//! completed call.
//!
//! At frame zero the score throttle is bypassed. The local body tests the two score-dirty
//! bits, writes `score_explored = 0`, and then calls `compute_unit_score` because ordinary
//! setup Unit initialization has set `NEW_UNITS`. That child needs the complete Unit/queue
//! census, type-score rows, and Armageddon state. Those facts do not yet have one golden
//! call-boundary owner, so the result is a typed request rather than a score mutation.
//!
//! The authority also consumes the integrated golden Market Leader-accounting receipt.
//! It proves that `num_buildings[22] == 1` is retained for the later build-score child and
//! that wealth `gather_slots[2]` plus its high-water mirror remain exactly one. Neither the
//! planner nor this score prefix may replace those cells with a blanket six-resource zero.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::systems::leader_market_build_accounting::{
    prepare_market_leader_accounting, LinkedMarketBuildFacts, MarketLeaderAccountingImage,
    MarketLeaderAccountingReceipt, MarketLeaderRegionAuthority, BUILD_ACTIVATE_DIRTY_FLAG,
    BUILD_INIT_DIRTY_FLAG, GOLDEN_MARKET_OBJECT_ID, GOLDEN_MARKET_OWNER, MARKET_BUILDING_SLOT,
    MARKET_GATHER_SLOT, MARKET_TYPE,
};
use don_sim::systems::setup_diplomacy::{LeaderTeamState, PlayerSetup, SETUP_SLOTS};
use don_sim::systems::team_setup_mutation::{
    init_teams_atomic, InitTeamsError, InitTeamsRequest, TeamSetupState,
};
use don_sim::systems::unit_inctime::SUPPORTED_RETAIL_EXE_SHA256;
use don_sim::systems::victory_score::{self, leader_flag};

use crate::initial::SHIPPED_TYPES_SERIALIZED_BYTES;
use crate::replay::{load_payload, Replay};
use crate::setup_2024_frame0_plan_strategy::Frame0PlanStrategyPrefixPlan;
use crate::setup_2024_frame379::REPLAY_FILE_SHA256;
use crate::world_owner_frontier::sha256;

pub const COMPUTE_SCORE_VA: u32 = 0x006e_c560;
pub const STRATEGY_COMPUTE_SCORE_CALL_VA: u32 = 0x006e_d45b;
pub const PLAN_STRATEGY_RETURN_VA: u32 = 0x006b_c0f7;
pub const SCORE_EXPLORED_STORE_VA: u32 = 0x006e_c5a0;
pub const COMPUTE_UNIT_SCORE_CALL_VA: u32 = 0x006e_c5ab;
pub const COMPUTE_UNIT_SCORE_VA: u32 = 0x006b_c500;
pub const COMPUTE_BUILD_SCORE_CALL_VA: u32 = 0x006e_c5b8;
pub const COMPUTE_BUILD_SCORE_VA: u32 = 0x006b_c3f0;
pub const SCORE_UNITS_STORE_VA: u32 = 0x006b_c50c;
pub const SCORE_UNITS_2_STORE_VA: u32 = 0x006b_c513;
pub const GET_ARMAGEDDON_CALL_VA: u32 = 0x006b_c51a;
pub const GET_ARMAGEDDON_VA: u32 = 0x0059_4020;
pub const ARMAGEDDON_COMPARE_LOAD_VA: u32 = 0x006b_c51f;
pub const ARMAGEDDON_COMPARE_VA: u32 = 0x006b_c525;
pub const UNIT_SCORE_CENSUS_FIRST_VA: u32 = 0x006b_c540;
pub const UNIT_SCORE_NUM_QUEUED_READ_VA: u32 = 0x006b_c549;
pub const UNIT_SCORE_NUM_UNITS_READ_VA: u32 = 0x006b_c551;
pub const UNIT_SCORE_VALUE_CALL_VA: u32 = 0x006b_c56c;
pub const UNIT_SCORE_VALUE_VTABLE_OFFSET: u32 = 0x7c;
pub const FIRST_UNIT_SCORE_TYPE: i32 = 50;
pub const FIRST_UNIT_NUM_QUEUED_LEADER_OFFSET: u32 = 0x5a86;
pub const FIRST_UNIT_NUM_UNITS_LEADER_OFFSET: u32 = 0x5762;
pub const TRACK_UNIT_TYPE_VA: u32 = 0x006e_0dd0;
pub const TRACK_UNIT_TYPE_COUNT_STORE_VA: u32 = 0x006e_0de4;
pub const TRACK_QUEUED_VA: u32 = 0x006e_0f30;
pub const TRACK_QUEUED_COUNT_STORE_VA: u32 = 0x006e_0f58;
pub const RULES_CONSTANTS_ARMAGEDDON_OFFSET: usize = 0x0d14;
pub const RULES_CONSTANTS_ARMAGEDDON_PER_NATION_OFFSET: usize = 0x0d18;
pub const RULES_CONSTANTS_ARMAGEDDON_PER_TEAM_OFFSET: usize = 0x0d1c;

pub const GOLDEN_FRAME: i32 = 0;
pub const GOLDEN_STEP: u8 = 11;
pub const GOLDEN_OWNER: u8 = 0;
pub const GOLDEN_STRATEGY_ORDINAL: u8 = 0;
pub const GOLDEN_FORCE: i32 = 0;
pub const GOLDEN_MARKET_BUILD_ROW: usize = 1;

const ACTIVE_HUMAN_MASK: i32 = 0x7;
const SCORE_DIRTY_MASK: i32 = leader_flag::NEW_UNITS | leader_flag::NEW_TECH;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Frame0ComputeScoreEntrySource {
    /// Supported retail captured immediately after owner zero's frame-zero
    /// `plan_strategy` return and immediately before `compute_score(0)` entry.
    CompleteRetailOwnerZeroPostPlanStrategy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame0ComputeScoreLeaderImage {
    pub leader_flags: i32,
    pub who: i32,
    pub score_explored: i32,
    pub buildings_built: i32,
    pub market_num_buildings: u16,
    pub market_regional_buildings: u16,
    pub market_gather_slots: i32,
    pub market_gather_slots_high: i32,
    pub market_high_buildings: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0ComputeScoreEntryCapture {
    pub revision: u64,
    pub source: Frame0ComputeScoreEntrySource,
    pub replay_file_sha256: [u8; 32],
    pub executable_sha256: [u8; 32],
    /// Exact native planner entry authority consumed by the detached local prefix.
    pub plan_entry_authority_digest: [u8; 32],
    pub plan_local_prefix_digest: [u8; 32],
    pub plan_first_open_request_sha256: [u8; 32],
    /// Source identity for the complete native body from the first open child through return.
    pub plan_strategy_return_sha256: [u8; 32],
    /// Exact simulator image at `0x006ed45b`, after the planner returned.
    pub call_entry_sim_sha256: [u8; 32],
    pub native_trace_sha256: [u8; 32],
    /// The post-Market City/Build image consumed by the Leader-accounting receipt.
    pub market_pre_mount_sim_sha256: [u8; 32],
    pub market_capture_revision: u64,
    pub frame: i32,
    pub step: u8,
    pub owner: u8,
    pub strategy_ordinal: u8,
    pub force: i32,
    pub leader: Frame0ComputeScoreLeaderImage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0ComputeScoreEntryAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub market_accounting_receipt_sha256: [u8; 32],
    pub capture: Frame0ComputeScoreEntryCapture,
    pub market_accounting: MarketLeaderAccountingReceipt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Frame0ComputeUnitScoreInputSurface {
    /// `num_queued[50..=401]`, `num_units[352]`, admitted type cost/support/attack rows,
    /// and the Game/Rules Armageddon comparison used by `0x006bc500`.
    CompleteUnitQueueTypeScoreAndArmageddonProjection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0ComputeUnitScoreRequest {
    pub request_sha256: [u8; 32],
    pub parent_authority_digest: [u8; 32],
    pub local_prefix_digest: [u8; 32],
    pub call_entry_sim_sha256: [u8; 32],
    pub market_accounting_receipt_sha256: [u8; 32],
    pub receiver_owner: u8,
    pub callsite_va: u32,
    pub callee_va: u32,
    pub input_surface: Frame0ComputeUnitScoreInputSurface,
    /// Exact later child which will consume the Market count after this request closes.
    pub next_callsite_if_complete: u32,
    pub next_callee_if_complete: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Frame0ComputeUnitScoreField {
    ScoreUnits,
    ScoreUnitsAttack,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame0ComputeUnitScoreWrite {
    pub store_va: u32,
    pub field: Frame0ComputeUnitScoreField,
    pub after: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Frame0GetArmageddonInputSurface {
    /// `RulesData::{armageddon,armageddon_per_nation,armageddon_per_team}` plus the live
    /// `Game::{num_nations,num_sides}` and `GameInfo::starting_resources` projection read by
    /// `0x00594020`. The later comparison against `Game::armageddon` is deliberately outside
    /// this child request.
    CompleteRulesAndLiveGameThresholdProjection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0GetArmageddonRequest {
    pub request_sha256: [u8; 32],
    pub parent_request_sha256: [u8; 32],
    pub parent_authority_digest: [u8; 32],
    pub local_prefix_digest: [u8; 32],
    pub call_entry_sim_sha256: [u8; 32],
    pub receiver_owner: u8,
    pub callsite_va: u32,
    pub callee_va: u32,
    pub input_surface: Frame0GetArmageddonInputSurface,
    /// First instruction after the child. It reads the separate live nuke counter.
    pub comparison_load_va_if_complete: u32,
    /// First Unit/queue census instruction if the Armageddon comparison falls through.
    pub census_first_va_if_clock_open: u32,
}

/// Exact unconditional prefix of `Leader::compute_unit_score`.
///
/// The pre-store values are intentionally absent: both retail stores overwrite their cells and
/// the post-planner call-entry snapshot is retained by hash, not relabelled as a projected Leader
/// inventory. This receipt is detached and cannot be installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0ComputeUnitScorePrefixPlan {
    pub authority_revision: u64,
    pub parent_authority_digest: [u8; 32],
    pub parent_request_sha256: [u8; 32],
    pub call_entry_sim_sha256: [u8; 32],
    pub receiver_owner: u8,
    pub writes: [Frame0ComputeUnitScoreWrite; 2],
    pub local_prefix_digest: [u8; 32],
    pub market_accounting_receipt_sha256: [u8; 32],
    pub open: Frame0GetArmageddonRequest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Frame0GetArmageddonSource {
    /// The supported replay's serialized Rules span, initial Game/GameInfo projection, and an
    /// exact rerun of `Game::init_teams` over its complete eight-player setup table.
    ReplayRulesInitialGameAndSetupTeamTransaction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame0ArmageddonThresholdFacts {
    pub armageddon: i32,
    pub armageddon_per_nation: i32,
    pub armageddon_per_team: i32,
    pub num_nations: i32,
    pub num_sides: i32,
    pub starting_resources: u8,
    /// `Game::armageddon +0x6E0`. The replay initial image carries zero and no setup,
    /// Market, or plan-strategy instruction can launch a nuke before this call.
    pub current_armageddon: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0GetArmageddonAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub source: Frame0GetArmageddonSource,
    pub replay_file_sha256: [u8; 32],
    pub replay_payload_sha256: [u8; 32],
    pub rules_serialized_sha256: [u8; 32],
    pub rules_constants_payload_offset: usize,
    pub team_setup_source_sha256: [u8; 32],
    pub parent_authority_digest: [u8; 32],
    pub parent_local_prefix_digest: [u8; 32],
    pub parent_request_sha256: [u8; 32],
    pub facts: Frame0ArmageddonThresholdFacts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Frame0UnitScoreFirstCensusInputSurface {
    /// Live post-`plan_strategy` `num_queued[50]` and `num_units[0]`. Setup Unit rows do not
    /// satisfy this request without a separate chronology proof that all Leader mirrors agree.
    ExactPostPlanType50CountPair,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0UnitScoreFirstCensusRequest {
    pub request_sha256: [u8; 32],
    pub parent_authority_digest: [u8; 32],
    pub armageddon_receipt_digest: [u8; 32],
    pub call_entry_sim_sha256: [u8; 32],
    pub receiver_owner: u8,
    pub type_index: i32,
    pub num_queued_read_va: u32,
    pub num_queued_leader_offset: u32,
    pub num_units_read_va: u32,
    pub num_units_leader_offset: u32,
    pub input_surface: Frame0UnitScoreFirstCensusInputSurface,
    /// Reached only if the exact count pair sums nonzero. The virtual callee is deliberately
    /// unresolved until the admitted type row supplies its concrete receiver/vtable.
    pub next_child_callsite_if_nonzero: u32,
    pub next_child_vtable_offset_if_nonzero: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0ComputeUnitScoreArmageddonPlan {
    pub authority_revision: u64,
    pub parent_authority_digest: [u8; 32],
    pub parent_local_prefix_digest: [u8; 32],
    pub parent_request_sha256: [u8; 32],
    pub source_authority_digest: [u8; 32],
    pub get_armageddon_callsite_va: u32,
    pub get_armageddon_callee_va: u32,
    pub returned_threshold: i32,
    pub current_armageddon: i32,
    pub comparison_load_va: u32,
    pub comparison_va: u32,
    pub clock_open: bool,
    pub local_prefix_digest: [u8; 32],
    pub open: Frame0UnitScoreFirstCensusRequest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Frame0UnitScoreCountField {
    NumQueued50,
    NumUnits0,
}

/// One exact retail producer/read pair which a native post-planner projection must bind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame0UnitScoreCountFieldCaptureGap {
    pub field: Frame0UnitScoreCountField,
    pub leader_offset: u32,
    pub read_va: u32,
    pub producer_va: u32,
    pub producer_store_va: u32,
}

/// Smallest missing authority at the first live Unit-score census.
///
/// The canonical simulator has duplicate Victory/production mirrors for these cells, but it has
/// not executed the complete native frame-zero planner. Conversely, the supported native capture
/// retains the correct call-entry Sim digest but does not project these two cells. This result
/// records that exact join gap; it does not carry guessed values or select the conditional child.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0UnitScoreCountPairCaptureGap {
    pub parent_authority_digest: [u8; 32],
    pub armageddon_source_digest: [u8; 32],
    pub armageddon_prefix_digest: [u8; 32],
    pub call_entry_sim_sha256: [u8; 32],
    pub receiver_owner: u8,
    pub type_index: i32,
    pub fields: [Frame0UnitScoreCountFieldCaptureGap; 2],
    pub native_capture_projects_count_pair: bool,
    pub canonical_sim_joined_to_native_capture: bool,
    pub setup_receipts_admissible: bool,
    pub can_select_next_child: bool,
    pub conditional_next_child_callsite: u32,
    pub conditional_next_child_vtable_offset: u32,
    pub gap_digest: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0ComputeScorePrefixPlan {
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub call_entry_sim_sha256: [u8; 32],
    pub leader_before: Frame0ComputeScoreLeaderImage,
    pub leader_after_local: Frame0ComputeScoreLeaderImage,
    pub score_explored_store_before: i32,
    pub score_explored_store_after: i32,
    pub score_explored_store_va: u32,
    pub market_accounting_receipt_sha256: [u8; 32],
    pub local_prefix_digest: [u8; 32],
    pub open: Frame0ComputeUnitScoreRequest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame0ComputeScoreError {
    MissingCaptureRevision,
    WrongCaptureSource,
    ReplayMismatch,
    UnsupportedExecutable,
    MissingPlanEntryAuthority,
    MissingPlanLocalPrefix,
    MissingPlanOpenRequest,
    MissingPlanReturn,
    MissingCallEntrySnapshot,
    MissingNativeTrace,
    MissingMarketSnapshot,
    WrongFrame { expected: i32, actual: i32 },
    WrongStep { expected: u8, actual: u8 },
    WrongOwner { expected: u8, actual: u8 },
    WrongStrategyOrdinal { expected: u8, actual: u8 },
    WrongForce { expected: i32, actual: i32 },
    PlanPrefixDisagreement,
    InvalidMarketAccounting,
    MarketChronologyDisagreement,
    OwnerNotActiveHuman,
    LeaderWhoMismatch,
    MissingNewUnitsDirtyBit,
    StaleAuthority,
    ComputeUnitScoreParentDisagreement,
    GetArmageddonReplayRead(String),
    GetArmageddonPayloadRead(String),
    GetArmageddonReplayMismatch,
    GetArmageddonMissingRules,
    GetArmageddonRulesSpanMismatch,
    GetArmageddonRosterMismatch,
    GetArmageddonMissingSemaphore,
    GetArmageddonTeamSetup(InitTeamsError),
    GetArmageddonInitialStateMismatch,
    GetArmageddonParentDisagreement,
    GetArmageddonStaleAuthority,
    GetArmageddonClockUnexpectedlyClosed,
    UnitScoreCountPairParentDisagreement,
}

impl fmt::Display for Frame0ComputeScoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "2024 frame-zero compute_score prefix refused: {self:?}")
    }
}

impl std::error::Error for Frame0ComputeScoreError {}

fn append_market_source(image: &mut Vec<u8>, source: MarketLeaderRegionAuthority) {
    image.extend_from_slice(&source.capture_revision.to_le_bytes());
    image.extend_from_slice(&source.native_trace_sha256);
    image.extend_from_slice(&source.after_sim_sha256);
    image.extend_from_slice(&(source.owner as u64).to_le_bytes());
    image.extend_from_slice(&source.object_id.to_le_bytes());
    image.extend_from_slice(&source.type_index.to_le_bytes());
    image.push(source.region);
}

fn append_market_link(image: &mut Vec<u8>, linked: LinkedMarketBuildFacts) {
    image.extend_from_slice(&(linked.row as u64).to_le_bytes());
    image.extend_from_slice(&(linked.owner as u64).to_le_bytes());
    image.extend_from_slice(&linked.object_id.to_le_bytes());
    image.extend_from_slice(&linked.type_index.to_le_bytes());
    image.extend_from_slice(&linked.city.to_le_bytes());
    image.push(u8::from(linked.active));
}

fn append_market_image(image: &mut Vec<u8>, value: MarketLeaderAccountingImage) {
    image.extend_from_slice(&value.flags.to_le_bytes());
    image.extend_from_slice(&value.buildings_built.to_le_bytes());
    image.extend_from_slice(&value.num_buildings.to_le_bytes());
    image.extend_from_slice(&value.regional_buildings.to_le_bytes());
    image.extend_from_slice(&value.gather_slots.to_le_bytes());
    image.extend_from_slice(&value.gather_slots_high.to_le_bytes());
    image.extend_from_slice(&value.high_buildings.to_le_bytes());
}

pub fn market_leader_accounting_receipt_sha256(
    receipt: &MarketLeaderAccountingReceipt,
) -> [u8; 32] {
    let mut image = b"don-2024-golden-market-leader-accounting-receipt-v1".to_vec();
    append_market_source(&mut image, receipt.source);
    append_market_link(&mut image, receipt.linked);
    image.extend_from_slice(&(receipt.regional_index as u64).to_le_bytes());
    append_market_image(&mut image, receipt.before);
    append_market_image(&mut image, receipt.after_init);
    append_market_image(&mut image, receipt.after_increment_stats);
    append_market_image(&mut image, receipt.after_market_special);
    sha256(&image)
}

fn append_leader(image: &mut Vec<u8>, leader: Frame0ComputeScoreLeaderImage) {
    image.extend_from_slice(&leader.leader_flags.to_le_bytes());
    image.extend_from_slice(&leader.who.to_le_bytes());
    image.extend_from_slice(&leader.score_explored.to_le_bytes());
    image.extend_from_slice(&leader.buildings_built.to_le_bytes());
    image.extend_from_slice(&leader.market_num_buildings.to_le_bytes());
    image.extend_from_slice(&leader.market_regional_buildings.to_le_bytes());
    image.extend_from_slice(&leader.market_gather_slots.to_le_bytes());
    image.extend_from_slice(&leader.market_gather_slots_high.to_le_bytes());
    image.extend_from_slice(&leader.market_high_buildings.to_le_bytes());
}

fn entry_authority_digest(
    capture: &Frame0ComputeScoreEntryCapture,
    market_receipt_sha256: [u8; 32],
) -> [u8; 32] {
    let mut image = b"don-2024-frame0-compute-score-entry-v1".to_vec();
    image.extend_from_slice(&capture.revision.to_le_bytes());
    image.push(capture.source as u8);
    image.extend_from_slice(&capture.replay_file_sha256);
    image.extend_from_slice(&capture.executable_sha256);
    image.extend_from_slice(&capture.plan_entry_authority_digest);
    image.extend_from_slice(&capture.plan_local_prefix_digest);
    image.extend_from_slice(&capture.plan_first_open_request_sha256);
    image.extend_from_slice(&capture.plan_strategy_return_sha256);
    image.extend_from_slice(&capture.call_entry_sim_sha256);
    image.extend_from_slice(&capture.native_trace_sha256);
    image.extend_from_slice(&capture.market_pre_mount_sim_sha256);
    image.extend_from_slice(&capture.market_capture_revision.to_le_bytes());
    image.extend_from_slice(&capture.frame.to_le_bytes());
    image.extend_from_slice(&[capture.step, capture.owner, capture.strategy_ordinal]);
    image.extend_from_slice(&capture.force.to_le_bytes());
    append_leader(&mut image, capture.leader);
    image.extend_from_slice(&market_receipt_sha256);
    sha256(&image)
}

fn expected_market_receipt(
    receipt: &MarketLeaderAccountingReceipt,
) -> Result<MarketLeaderAccountingReceipt, Frame0ComputeScoreError> {
    let mut leader = victory_score::LeaderState {
        leader_flags: receipt.before.flags as i32,
        buildings_built: receipt.before.buildings_built,
        ..victory_score::LeaderState::default()
    };
    let Some(num) = leader.num_buildings.get_mut(MARKET_BUILDING_SLOT) else {
        return Err(Frame0ComputeScoreError::InvalidMarketAccounting);
    };
    *num = receipt.before.num_buildings;
    let Some(regional) = leader.reg_buildings.get_mut(receipt.regional_index) else {
        return Err(Frame0ComputeScoreError::InvalidMarketAccounting);
    };
    *regional = receipt.before.regional_buildings;
    leader.gather_slots[MARKET_GATHER_SLOT] = receipt.before.gather_slots;
    leader.gather_slots_high[MARKET_GATHER_SLOT] = receipt.before.gather_slots_high;
    let Some(high) = leader.high_buildings.get_mut(MARKET_BUILDING_SLOT) else {
        return Err(Frame0ComputeScoreError::InvalidMarketAccounting);
    };
    *high = receipt.before.high_buildings;

    let prepared = prepare_market_leader_accounting(
        receipt.source,
        receipt.linked,
        &leader,
        receipt.before.flags,
    )
    .map_err(|_| Frame0ComputeScoreError::InvalidMarketAccounting)?;
    Ok(MarketLeaderAccountingReceipt {
        source: prepared.source,
        linked: prepared.linked,
        regional_index: prepared.regional_index,
        before: prepared.before,
        after_init: prepared.after_init,
        after_increment_stats: prepared.after_increment_stats,
        after_market_special: prepared.after_market_special,
    })
}

fn validate_market_receipt(
    capture: &Frame0ComputeScoreEntryCapture,
    receipt: &MarketLeaderAccountingReceipt,
) -> Result<(), Frame0ComputeScoreError> {
    if receipt != &expected_market_receipt(receipt)?
        || receipt.source.owner != GOLDEN_MARKET_OWNER
        || receipt.source.object_id != GOLDEN_MARKET_OBJECT_ID
        || receipt.source.type_index != MARKET_TYPE
        || receipt.source.capture_revision != capture.market_capture_revision
        || receipt.source.after_sim_sha256 != capture.market_pre_mount_sim_sha256
        || receipt.linked.row != GOLDEN_MARKET_BUILD_ROW
        || receipt.linked.owner != GOLDEN_MARKET_OWNER
        || receipt.linked.object_id != GOLDEN_MARKET_OBJECT_ID
        || receipt.linked.type_index != MARKET_TYPE
        || receipt.linked.city != 0
        || !receipt.linked.active
        || receipt.after_market_special.buildings_built != 2
        || receipt.after_market_special.num_buildings != 1
        || receipt.after_market_special.regional_buildings != 1
        || receipt.after_market_special.gather_slots != 1
        || receipt.after_market_special.gather_slots_high != 1
        || receipt.after_market_special.high_buildings != 1
        || receipt.after_market_special.flags & (BUILD_INIT_DIRTY_FLAG | BUILD_ACTIVATE_DIRTY_FLAG)
            != BUILD_INIT_DIRTY_FLAG | BUILD_ACTIVATE_DIRTY_FLAG
    {
        return Err(Frame0ComputeScoreError::InvalidMarketAccounting);
    }

    let leader = capture.leader;
    let market = receipt.after_market_special;
    if leader.buildings_built != market.buildings_built
        || leader.market_num_buildings != market.num_buildings
        || leader.market_regional_buildings != market.regional_buildings
        || leader.market_gather_slots != market.gather_slots
        || leader.market_gather_slots_high != market.gather_slots_high
        || leader.market_high_buildings != market.high_buildings
        || (leader.leader_flags as u32) & market.flags != market.flags
    {
        return Err(Frame0ComputeScoreError::MarketChronologyDisagreement);
    }
    Ok(())
}

pub fn bind_golden_frame0_owner0_compute_score_entry(
    plan: &Frame0PlanStrategyPrefixPlan,
    capture: Frame0ComputeScoreEntryCapture,
    market_accounting: MarketLeaderAccountingReceipt,
) -> Result<Frame0ComputeScoreEntryAuthority, Frame0ComputeScoreError> {
    if capture.revision == 0 {
        return Err(Frame0ComputeScoreError::MissingCaptureRevision);
    }
    if capture.source != Frame0ComputeScoreEntrySource::CompleteRetailOwnerZeroPostPlanStrategy {
        return Err(Frame0ComputeScoreError::WrongCaptureSource);
    }
    if capture.replay_file_sha256 != REPLAY_FILE_SHA256 {
        return Err(Frame0ComputeScoreError::ReplayMismatch);
    }
    if capture.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256 {
        return Err(Frame0ComputeScoreError::UnsupportedExecutable);
    }
    for (digest, error) in [
        (
            capture.plan_entry_authority_digest,
            Frame0ComputeScoreError::MissingPlanEntryAuthority,
        ),
        (
            capture.plan_local_prefix_digest,
            Frame0ComputeScoreError::MissingPlanLocalPrefix,
        ),
        (
            capture.plan_first_open_request_sha256,
            Frame0ComputeScoreError::MissingPlanOpenRequest,
        ),
        (
            capture.plan_strategy_return_sha256,
            Frame0ComputeScoreError::MissingPlanReturn,
        ),
        (
            capture.call_entry_sim_sha256,
            Frame0ComputeScoreError::MissingCallEntrySnapshot,
        ),
        (
            capture.native_trace_sha256,
            Frame0ComputeScoreError::MissingNativeTrace,
        ),
        (
            capture.market_pre_mount_sim_sha256,
            Frame0ComputeScoreError::MissingMarketSnapshot,
        ),
    ] {
        if digest == [0; 32] {
            return Err(error);
        }
    }
    if capture.frame != GOLDEN_FRAME {
        return Err(Frame0ComputeScoreError::WrongFrame {
            expected: GOLDEN_FRAME,
            actual: capture.frame,
        });
    }
    if capture.step != GOLDEN_STEP {
        return Err(Frame0ComputeScoreError::WrongStep {
            expected: GOLDEN_STEP,
            actual: capture.step,
        });
    }
    if capture.owner != GOLDEN_OWNER {
        return Err(Frame0ComputeScoreError::WrongOwner {
            expected: GOLDEN_OWNER,
            actual: capture.owner,
        });
    }
    if capture.strategy_ordinal != GOLDEN_STRATEGY_ORDINAL {
        return Err(Frame0ComputeScoreError::WrongStrategyOrdinal {
            expected: GOLDEN_STRATEGY_ORDINAL,
            actual: capture.strategy_ordinal,
        });
    }
    if capture.force != GOLDEN_FORCE {
        return Err(Frame0ComputeScoreError::WrongForce {
            expected: GOLDEN_FORCE,
            actual: capture.force,
        });
    }
    if capture.plan_entry_authority_digest != plan.authority_digest
        || capture.plan_local_prefix_digest != plan.local_prefix_digest
        || capture.plan_first_open_request_sha256 != plan.open.request_sha256
        || capture.owner != plan.open.receiver_owner
    {
        return Err(Frame0ComputeScoreError::PlanPrefixDisagreement);
    }
    if capture.leader.leader_flags & ACTIVE_HUMAN_MASK != ACTIVE_HUMAN_MASK {
        return Err(Frame0ComputeScoreError::OwnerNotActiveHuman);
    }
    if capture.leader.who != i32::from(capture.owner) {
        return Err(Frame0ComputeScoreError::LeaderWhoMismatch);
    }
    if capture.leader.leader_flags & leader_flag::NEW_UNITS == 0 {
        return Err(Frame0ComputeScoreError::MissingNewUnitsDirtyBit);
    }
    validate_market_receipt(&capture, &market_accounting)?;

    let receipt_sha256 = market_leader_accounting_receipt_sha256(&market_accounting);
    let composition_digest = entry_authority_digest(&capture, receipt_sha256);
    Ok(Frame0ComputeScoreEntryAuthority {
        revision: capture.revision,
        composition_digest,
        market_accounting_receipt_sha256: receipt_sha256,
        capture,
        market_accounting,
    })
}

fn local_prefix_digest(
    authority: &Frame0ComputeScoreEntryAuthority,
    after: Frame0ComputeScoreLeaderImage,
) -> [u8; 32] {
    let mut image = b"don-2024-frame0-compute-score-local-prefix-v1".to_vec();
    image.extend_from_slice(&authority.composition_digest);
    image.extend_from_slice(&SCORE_EXPLORED_STORE_VA.to_le_bytes());
    append_leader(&mut image, after);
    sha256(&image)
}

fn compute_unit_score_request_digest(
    authority: &Frame0ComputeScoreEntryAuthority,
    prefix_digest: [u8; 32],
) -> [u8; 32] {
    let mut image = b"don-2024-frame0-compute-unit-score-request-v1".to_vec();
    image.extend_from_slice(&authority.composition_digest);
    image.extend_from_slice(&prefix_digest);
    image.extend_from_slice(&authority.capture.call_entry_sim_sha256);
    image.extend_from_slice(&authority.market_accounting_receipt_sha256);
    image.push(authority.capture.owner);
    image.extend_from_slice(&COMPUTE_UNIT_SCORE_CALL_VA.to_le_bytes());
    image.extend_from_slice(&COMPUTE_UNIT_SCORE_VA.to_le_bytes());
    image.push(
        Frame0ComputeUnitScoreInputSurface::CompleteUnitQueueTypeScoreAndArmageddonProjection as u8,
    );
    image.extend_from_slice(&COMPUTE_BUILD_SCORE_CALL_VA.to_le_bytes());
    image.extend_from_slice(&COMPUTE_BUILD_SCORE_VA.to_le_bytes());
    sha256(&image)
}

fn compute_unit_score_local_prefix_digest(
    authority: &Frame0ComputeScoreEntryAuthority,
    parent: &Frame0ComputeScorePrefixPlan,
    writes: &[Frame0ComputeUnitScoreWrite; 2],
) -> [u8; 32] {
    let mut image = b"don-2024-frame0-compute-unit-score-local-prefix-v1".to_vec();
    image.extend_from_slice(&authority.revision.to_le_bytes());
    image.extend_from_slice(&authority.composition_digest);
    image.extend_from_slice(&parent.open.request_sha256);
    image.extend_from_slice(&parent.call_entry_sim_sha256);
    image.push(parent.open.receiver_owner);
    for write in writes {
        image.extend_from_slice(&write.store_va.to_le_bytes());
        image.push(write.field as u8);
        image.extend_from_slice(&write.after.to_le_bytes());
    }
    sha256(&image)
}

fn get_armageddon_request_digest(
    authority: &Frame0ComputeScoreEntryAuthority,
    parent: &Frame0ComputeScorePrefixPlan,
    local_prefix_digest: [u8; 32],
) -> [u8; 32] {
    let mut image = b"don-2024-frame0-get-armageddon-request-v1".to_vec();
    image.extend_from_slice(&parent.open.request_sha256);
    image.extend_from_slice(&authority.composition_digest);
    image.extend_from_slice(&local_prefix_digest);
    image.extend_from_slice(&parent.call_entry_sim_sha256);
    image.push(parent.open.receiver_owner);
    image.extend_from_slice(&GET_ARMAGEDDON_CALL_VA.to_le_bytes());
    image.extend_from_slice(&GET_ARMAGEDDON_VA.to_le_bytes());
    image.push(Frame0GetArmageddonInputSurface::CompleteRulesAndLiveGameThresholdProjection as u8);
    image.extend_from_slice(&ARMAGEDDON_COMPARE_LOAD_VA.to_le_bytes());
    image.extend_from_slice(&UNIT_SCORE_CENSUS_FIRST_VA.to_le_bytes());
    sha256(&image)
}

fn append_armageddon_facts(image: &mut Vec<u8>, facts: Frame0ArmageddonThresholdFacts) {
    image.extend_from_slice(&facts.armageddon.to_le_bytes());
    image.extend_from_slice(&facts.armageddon_per_nation.to_le_bytes());
    image.extend_from_slice(&facts.armageddon_per_team.to_le_bytes());
    image.extend_from_slice(&facts.num_nations.to_le_bytes());
    image.extend_from_slice(&facts.num_sides.to_le_bytes());
    image.push(facts.starting_resources);
    image.extend_from_slice(&facts.current_armageddon.to_le_bytes());
}

fn frame0_get_armageddon_authority_digest(authority: &Frame0GetArmageddonAuthority) -> [u8; 32] {
    let mut image = b"don-2024-frame0-get-armageddon-authority-v1".to_vec();
    image.extend_from_slice(&authority.revision.to_le_bytes());
    image.push(authority.source as u8);
    image.extend_from_slice(&authority.replay_file_sha256);
    image.extend_from_slice(&authority.replay_payload_sha256);
    image.extend_from_slice(&authority.rules_serialized_sha256);
    image.extend_from_slice(&(authority.rules_constants_payload_offset as u64).to_le_bytes());
    image.extend_from_slice(&authority.team_setup_source_sha256);
    image.extend_from_slice(&authority.parent_authority_digest);
    image.extend_from_slice(&authority.parent_local_prefix_digest);
    image.extend_from_slice(&authority.parent_request_sha256);
    append_armageddon_facts(&mut image, authority.facts);
    sha256(&image)
}

fn append_team_setup_source(image: &mut Vec<u8>, before: &TeamSetupState, after: &TeamSetupState) {
    image.push(before.setup.team_style);
    image.extend_from_slice(&before.setup.frame.to_le_bytes());
    image.push(before.setup.semaphore_820);
    for player in before.setup.players {
        image.extend_from_slice(&player.flags.to_le_bytes());
        image.push(player.who);
        image.push(player.team as u8);
    }
    for leader in before.setup.leaders {
        image.extend_from_slice(&leader.leader_flags.to_le_bytes());
        image.extend_from_slice(&leader.who.to_le_bytes());
    }
    for value in after.on_team {
        image.extend_from_slice(&value.to_le_bytes());
    }
    image.extend_from_slice(&after.num_teams.to_le_bytes());
    image.extend_from_slice(&after.num_sides.to_le_bytes());
    image.extend_from_slice(&after.semaphore_flags.to_le_bytes());
    image.push(after.setup.semaphore_820);
    for player in after.setup.players {
        image.push(player.team as u8);
    }
}

fn rules_i32(payload: &[u8], constants: usize, offset: usize) -> Option<i32> {
    let bytes = payload.get(constants.checked_add(offset)?..constants.checked_add(offset + 4)?)?;
    Some(i32::from_le_bytes(bytes.try_into().ok()?))
}

fn get_armageddon_threshold(facts: Frame0ArmageddonThresholdFacts) -> i32 {
    let mut threshold = facts
        .armageddon_per_team
        .wrapping_mul(facts.num_sides)
        .wrapping_add(facts.armageddon_per_nation.wrapping_mul(facts.num_nations))
        .wrapping_add(facts.armageddon);
    match facts.starting_resources {
        7 => threshold = threshold.wrapping_mul(3),
        5 | 6 | 11 | 12 => threshold = threshold.wrapping_mul(2),
        _ => {}
    }
    if facts.starting_resources == 8 && threshold < 100 {
        threshold = 100;
    }
    threshold
}

fn armageddon_local_prefix_digest(
    source: &Frame0GetArmageddonAuthority,
    parent: &Frame0ComputeUnitScorePrefixPlan,
    threshold: i32,
    clock_open: bool,
) -> [u8; 32] {
    let mut image = b"don-2024-frame0-compute-unit-score-armageddon-prefix-v1".to_vec();
    image.extend_from_slice(&source.composition_digest);
    image.extend_from_slice(&parent.local_prefix_digest);
    image.extend_from_slice(&parent.open.request_sha256);
    image.extend_from_slice(&GET_ARMAGEDDON_CALL_VA.to_le_bytes());
    image.extend_from_slice(&GET_ARMAGEDDON_VA.to_le_bytes());
    image.extend_from_slice(&threshold.to_le_bytes());
    image.extend_from_slice(&source.facts.current_armageddon.to_le_bytes());
    image.extend_from_slice(&ARMAGEDDON_COMPARE_LOAD_VA.to_le_bytes());
    image.extend_from_slice(&ARMAGEDDON_COMPARE_VA.to_le_bytes());
    image.push(u8::from(clock_open));
    sha256(&image)
}

fn first_unit_census_request_digest(
    authority: &Frame0ComputeScoreEntryAuthority,
    source: &Frame0GetArmageddonAuthority,
    local_prefix_digest: [u8; 32],
) -> [u8; 32] {
    let mut image = b"don-2024-frame0-unit-score-first-census-request-v1".to_vec();
    image.extend_from_slice(&authority.composition_digest);
    image.extend_from_slice(&source.composition_digest);
    image.extend_from_slice(&local_prefix_digest);
    image.extend_from_slice(&authority.capture.call_entry_sim_sha256);
    image.push(authority.capture.owner);
    image.extend_from_slice(&FIRST_UNIT_SCORE_TYPE.to_le_bytes());
    image.extend_from_slice(&UNIT_SCORE_NUM_QUEUED_READ_VA.to_le_bytes());
    image.extend_from_slice(&FIRST_UNIT_NUM_QUEUED_LEADER_OFFSET.to_le_bytes());
    image.extend_from_slice(&UNIT_SCORE_NUM_UNITS_READ_VA.to_le_bytes());
    image.extend_from_slice(&FIRST_UNIT_NUM_UNITS_LEADER_OFFSET.to_le_bytes());
    image.push(Frame0UnitScoreFirstCensusInputSurface::ExactPostPlanType50CountPair as u8);
    image.extend_from_slice(&UNIT_SCORE_VALUE_CALL_VA.to_le_bytes());
    image.extend_from_slice(&UNIT_SCORE_VALUE_VTABLE_OFFSET.to_le_bytes());
    sha256(&image)
}

fn unit_score_count_pair_gap_digest(gap: &Frame0UnitScoreCountPairCaptureGap) -> [u8; 32] {
    let mut image = b"don-2024-frame0-unit-score-count-pair-capture-gap-v1".to_vec();
    image.extend_from_slice(&gap.parent_authority_digest);
    image.extend_from_slice(&gap.armageddon_source_digest);
    image.extend_from_slice(&gap.armageddon_prefix_digest);
    image.extend_from_slice(&gap.call_entry_sim_sha256);
    image.push(gap.receiver_owner);
    image.extend_from_slice(&gap.type_index.to_le_bytes());
    for field in gap.fields {
        image.push(field.field as u8);
        image.extend_from_slice(&field.leader_offset.to_le_bytes());
        image.extend_from_slice(&field.read_va.to_le_bytes());
        image.extend_from_slice(&field.producer_va.to_le_bytes());
        image.extend_from_slice(&field.producer_store_va.to_le_bytes());
    }
    image.push(u8::from(gap.native_capture_projects_count_pair));
    image.push(u8::from(gap.canonical_sim_joined_to_native_capture));
    image.push(u8::from(gap.setup_receipts_admissible));
    image.push(u8::from(gap.can_select_next_child));
    image.extend_from_slice(&gap.conditional_next_child_callsite.to_le_bytes());
    image.extend_from_slice(&gap.conditional_next_child_vtable_offset.to_le_bytes());
    sha256(&image)
}

/// Execute the exact local instructions through the first unowned child.
///
/// No canonical Sim state is changed. The returned plan cannot be installed until the complete
/// Unit-score request and every later score child close in retail order.
pub fn plan_golden_frame0_owner0_compute_score_prefix(
    authority: &Frame0ComputeScoreEntryAuthority,
) -> Result<Frame0ComputeScorePrefixPlan, Frame0ComputeScoreError> {
    if authority.revision != authority.capture.revision
        || authority.market_accounting_receipt_sha256
            != market_leader_accounting_receipt_sha256(&authority.market_accounting)
        || authority.composition_digest
            != entry_authority_digest(
                &authority.capture,
                authority.market_accounting_receipt_sha256,
            )
    {
        return Err(Frame0ComputeScoreError::StaleAuthority);
    }

    // `test [leader_flags], 0x01800000` occurs before this store. The binder has established
    // NEW_UNITS, so retail stores zero and immediately calls compute_unit_score.
    debug_assert_ne!(authority.capture.leader.leader_flags & SCORE_DIRTY_MASK, 0);
    let before = authority.capture.leader;
    let mut after = before;
    after.score_explored = 0;
    // Wealth and all Market registry/history cells are intentionally retained verbatim.
    debug_assert_eq!(after.market_gather_slots, before.market_gather_slots);
    debug_assert_eq!(
        after.market_gather_slots_high,
        before.market_gather_slots_high
    );
    debug_assert_eq!(after.market_num_buildings, before.market_num_buildings);

    let prefix_digest = local_prefix_digest(authority, after);
    let request = Frame0ComputeUnitScoreRequest {
        request_sha256: compute_unit_score_request_digest(authority, prefix_digest),
        parent_authority_digest: authority.composition_digest,
        local_prefix_digest: prefix_digest,
        call_entry_sim_sha256: authority.capture.call_entry_sim_sha256,
        market_accounting_receipt_sha256: authority.market_accounting_receipt_sha256,
        receiver_owner: authority.capture.owner,
        callsite_va: COMPUTE_UNIT_SCORE_CALL_VA,
        callee_va: COMPUTE_UNIT_SCORE_VA,
        input_surface:
            Frame0ComputeUnitScoreInputSurface::CompleteUnitQueueTypeScoreAndArmageddonProjection,
        next_callsite_if_complete: COMPUTE_BUILD_SCORE_CALL_VA,
        next_callee_if_complete: COMPUTE_BUILD_SCORE_VA,
    };

    Ok(Frame0ComputeScorePrefixPlan {
        authority_revision: authority.revision,
        authority_digest: authority.composition_digest,
        call_entry_sim_sha256: authority.capture.call_entry_sim_sha256,
        leader_before: before,
        leader_after_local: after,
        score_explored_store_before: before.score_explored,
        score_explored_store_after: 0,
        score_explored_store_va: SCORE_EXPLORED_STORE_VA,
        market_accounting_receipt_sha256: authority.market_accounting_receipt_sha256,
        local_prefix_digest: prefix_digest,
        open: request,
    })
}

/// Advance the open Unit-score child through its two unconditional score resets.
///
/// This function replays and compares the complete parent prefix before advancing it. The next
/// retail instruction is `Game::get_armageddon`; its six scalar inputs do not yet have one
/// post-planner golden authority, so the returned plan stops at that exact child. It does not
/// consume setup-time Unit rows as if they were the live frame-zero census.
pub fn plan_golden_frame0_owner0_compute_unit_score_prefix(
    authority: &Frame0ComputeScoreEntryAuthority,
    parent: &Frame0ComputeScorePrefixPlan,
) -> Result<Frame0ComputeUnitScorePrefixPlan, Frame0ComputeScoreError> {
    let expected = plan_golden_frame0_owner0_compute_score_prefix(authority)?;
    if parent != &expected
        || parent.open.callsite_va != COMPUTE_UNIT_SCORE_CALL_VA
        || parent.open.callee_va != COMPUTE_UNIT_SCORE_VA
        || parent.open.receiver_owner != GOLDEN_OWNER
    {
        return Err(Frame0ComputeScoreError::ComputeUnitScoreParentDisagreement);
    }

    let writes = [
        Frame0ComputeUnitScoreWrite {
            store_va: SCORE_UNITS_STORE_VA,
            field: Frame0ComputeUnitScoreField::ScoreUnits,
            after: 0,
        },
        Frame0ComputeUnitScoreWrite {
            store_va: SCORE_UNITS_2_STORE_VA,
            field: Frame0ComputeUnitScoreField::ScoreUnitsAttack,
            after: 0,
        },
    ];
    let local_prefix_digest = compute_unit_score_local_prefix_digest(authority, parent, &writes);
    let open = Frame0GetArmageddonRequest {
        request_sha256: get_armageddon_request_digest(authority, parent, local_prefix_digest),
        parent_request_sha256: parent.open.request_sha256,
        parent_authority_digest: authority.composition_digest,
        local_prefix_digest,
        call_entry_sim_sha256: parent.call_entry_sim_sha256,
        receiver_owner: parent.open.receiver_owner,
        callsite_va: GET_ARMAGEDDON_CALL_VA,
        callee_va: GET_ARMAGEDDON_VA,
        input_surface: Frame0GetArmageddonInputSurface::CompleteRulesAndLiveGameThresholdProjection,
        comparison_load_va_if_complete: ARMAGEDDON_COMPARE_LOAD_VA,
        census_first_va_if_clock_open: UNIT_SCORE_CENSUS_FIRST_VA,
    };

    Ok(Frame0ComputeUnitScorePrefixPlan {
        authority_revision: authority.revision,
        parent_authority_digest: authority.composition_digest,
        parent_request_sha256: parent.open.request_sha256,
        call_entry_sim_sha256: parent.call_entry_sim_sha256,
        receiver_owner: parent.open.receiver_owner,
        writes,
        local_prefix_digest,
        market_accounting_receipt_sha256: authority.market_accounting_receipt_sha256,
        open,
    })
}

/// Bind `Game::get_armageddon` to the target replay's own Rules and setup inputs.
///
/// The replay file and decompressed payload are rehashed. The three Constants dwords are read
/// directly from the replay-carried serialized Rules span. `num_nations` comes from the complete
/// active Player roster, while `num_sides` is regenerated by the exact deterministic
/// `Game::init_teams` transaction over all eight Player rows. No caller-supplied threshold or
/// setup-era Unit count is accepted.
pub fn bind_golden_frame0_owner0_get_armageddon(
    replay: &Replay,
    score_authority: &Frame0ComputeScoreEntryAuthority,
    parent: &Frame0ComputeUnitScorePrefixPlan,
) -> Result<Frame0GetArmageddonAuthority, Frame0ComputeScoreError> {
    let expected_parent = plan_golden_frame0_owner0_compute_unit_score_prefix(
        score_authority,
        &plan_golden_frame0_owner0_compute_score_prefix(score_authority)?,
    )?;
    if parent != &expected_parent
        || parent.open.callsite_va != GET_ARMAGEDDON_CALL_VA
        || parent.open.callee_va != GET_ARMAGEDDON_VA
        || parent.open.receiver_owner != GOLDEN_OWNER
    {
        return Err(Frame0ComputeScoreError::GetArmageddonParentDisagreement);
    }

    let raw = std::fs::read(&replay.path)
        .map_err(|error| Frame0ComputeScoreError::GetArmageddonReplayRead(error.to_string()))?;
    let replay_file_sha256 = sha256(&raw);
    if replay_file_sha256 != REPLAY_FILE_SHA256
        || replay_file_sha256 != score_authority.capture.replay_file_sha256
    {
        return Err(Frame0ComputeScoreError::GetArmageddonReplayMismatch);
    }
    let payload = load_payload(&replay.path)
        .map_err(|error| Frame0ComputeScoreError::GetArmageddonPayloadRead(error.to_string()))?;
    let replay_payload_sha256 = sha256(&payload);
    if replay_payload_sha256 != replay.initial.payload_sha256 {
        return Err(Frame0ComputeScoreError::GetArmageddonReplayMismatch);
    }
    let rules = replay
        .initial
        .rules
        .ok_or(Frame0ComputeScoreError::GetArmageddonMissingRules)?;
    let rules_end = rules
        .serialized_offset
        .checked_add(rules.serialized_bytes)
        .ok_or(Frame0ComputeScoreError::GetArmageddonRulesSpanMismatch)?;
    let rules_span = payload
        .get(rules.serialized_offset..rules_end)
        .ok_or(Frame0ComputeScoreError::GetArmageddonRulesSpanMismatch)?;
    if sha256(rules_span) != rules.serialized_sha256 {
        return Err(Frame0ComputeScoreError::GetArmageddonRulesSpanMismatch);
    }
    let constants = rules
        .serialized_offset
        .checked_add(1 + SHIPPED_TYPES_SERIALIZED_BYTES)
        .ok_or(Frame0ComputeScoreError::GetArmageddonRulesSpanMismatch)?;
    let constants_end = constants
        .checked_add(RULES_CONSTANTS_ARMAGEDDON_PER_TEAM_OFFSET + 4)
        .ok_or(Frame0ComputeScoreError::GetArmageddonRulesSpanMismatch)?;
    if constants_end > rules_end {
        return Err(Frame0ComputeScoreError::GetArmageddonRulesSpanMismatch);
    }

    if replay.initial.info.players.len() != SETUP_SLOTS || replay.initial.game.frame != GOLDEN_FRAME
    {
        return Err(Frame0ComputeScoreError::GetArmageddonInitialStateMismatch);
    }
    let active: Vec<_> = replay.initial.active_players().collect();
    if active.len() != 1
        || active[0].who != GOLDEN_OWNER
        || usize::from(active[0].slot) >= SETUP_SLOTS
    {
        return Err(Frame0ComputeScoreError::GetArmageddonRosterMismatch);
    }

    let mut team = TeamSetupState::default();
    team.setup.team_style = replay.initial.info.settings.team_style;
    team.setup.frame = replay.initial.game.frame;
    team.setup.semaphore_820 = *replay
        .initial
        .game
        .semaphore
        .first()
        .ok_or(Frame0ComputeScoreError::GetArmageddonMissingSemaphore)?;
    for (slot, player) in replay.initial.info.players.iter().enumerate() {
        if usize::from(player.slot) != slot {
            return Err(Frame0ComputeScoreError::GetArmageddonRosterMismatch);
        }
        team.setup.players[slot] = PlayerSetup {
            flags: player.flags,
            who: player.who,
            team: player.team as i8,
        };
    }
    for player in &active {
        let owner = usize::from(player.who);
        if owner >= SETUP_SLOTS || team.setup.leaders[owner].is_present() {
            return Err(Frame0ComputeScoreError::GetArmageddonRosterMismatch);
        }
        team.setup.leaders[owner] = LeaderTeamState {
            leader_flags: 1,
            who: i32::from(player.who),
            diplos: [0; SETUP_SLOTS],
        };
    }
    let team_before = team.clone();
    init_teams_atomic(
        &mut team,
        InitTeamsRequest {
            local_player_setup_slot: usize::from(active[0].slot),
            ranked: false,
        },
    )
    .map_err(Frame0ComputeScoreError::GetArmageddonTeamSetup)?;
    let mut team_source = b"don-2024-frame0-armageddon-team-source-v1".to_vec();
    team_source.extend_from_slice(&replay_payload_sha256);
    append_team_setup_source(&mut team_source, &team_before, &team);
    let team_setup_source_sha256 = sha256(&team_source);

    let facts = Frame0ArmageddonThresholdFacts {
        armageddon: rules_i32(&payload, constants, RULES_CONSTANTS_ARMAGEDDON_OFFSET)
            .ok_or(Frame0ComputeScoreError::GetArmageddonRulesSpanMismatch)?,
        armageddon_per_nation: rules_i32(
            &payload,
            constants,
            RULES_CONSTANTS_ARMAGEDDON_PER_NATION_OFFSET,
        )
        .ok_or(Frame0ComputeScoreError::GetArmageddonRulesSpanMismatch)?,
        armageddon_per_team: rules_i32(
            &payload,
            constants,
            RULES_CONSTANTS_ARMAGEDDON_PER_TEAM_OFFSET,
        )
        .ok_or(Frame0ComputeScoreError::GetArmageddonRulesSpanMismatch)?,
        num_nations: i32::try_from(active.len())
            .map_err(|_| Frame0ComputeScoreError::GetArmageddonRosterMismatch)?,
        num_sides: team.num_sides,
        starting_resources: replay.initial.info.settings.starting_resources,
        current_armageddon: replay.initial.game.armageddon,
    };
    // These values are independently read above. The explicit target gate prevents a valid but
    // different replay/rules image from being relabelled as this golden chronology receipt.
    if facts
        != (Frame0ArmageddonThresholdFacts {
            armageddon: 4,
            armageddon_per_nation: 1,
            armageddon_per_team: 2,
            num_nations: 1,
            num_sides: 1,
            starting_resources: 0,
            current_armageddon: 0,
        })
    {
        return Err(Frame0ComputeScoreError::GetArmageddonInitialStateMismatch);
    }

    let mut authority = Frame0GetArmageddonAuthority {
        revision: score_authority.revision,
        composition_digest: [0; 32],
        source: Frame0GetArmageddonSource::ReplayRulesInitialGameAndSetupTeamTransaction,
        replay_file_sha256,
        replay_payload_sha256,
        rules_serialized_sha256: rules.serialized_sha256,
        rules_constants_payload_offset: constants,
        team_setup_source_sha256,
        parent_authority_digest: score_authority.composition_digest,
        parent_local_prefix_digest: parent.local_prefix_digest,
        parent_request_sha256: parent.open.request_sha256,
        facts,
    };
    authority.composition_digest = frame0_get_armageddon_authority_digest(&authority);
    Ok(authority)
}

/// Execute the source-owned Armageddon threshold child and its immediate comparison.
///
/// The golden clock is open (`0 < 7`). Retail next reads the live type-50 queue and Unit count.
/// Those post-planner mirrors are not projected by the replay setup rows, so this plan stops at
/// the first count pair rather than selecting a virtual score-value child.
pub fn plan_golden_frame0_owner0_compute_unit_score_armageddon(
    score_authority: &Frame0ComputeScoreEntryAuthority,
    parent: &Frame0ComputeUnitScorePrefixPlan,
    source: &Frame0GetArmageddonAuthority,
) -> Result<Frame0ComputeUnitScoreArmageddonPlan, Frame0ComputeScoreError> {
    let expected_parent = plan_golden_frame0_owner0_compute_unit_score_prefix(
        score_authority,
        &plan_golden_frame0_owner0_compute_score_prefix(score_authority)?,
    )?;
    if parent != &expected_parent
        || source.revision != score_authority.revision
        || source.source != Frame0GetArmageddonSource::ReplayRulesInitialGameAndSetupTeamTransaction
        || source.replay_file_sha256 != REPLAY_FILE_SHA256
        || source.parent_authority_digest != score_authority.composition_digest
        || source.parent_local_prefix_digest != parent.local_prefix_digest
        || source.parent_request_sha256 != parent.open.request_sha256
    {
        return Err(Frame0ComputeScoreError::GetArmageddonParentDisagreement);
    }
    if source.composition_digest != frame0_get_armageddon_authority_digest(source) {
        return Err(Frame0ComputeScoreError::GetArmageddonStaleAuthority);
    }

    let threshold = get_armageddon_threshold(source.facts);
    let clock_open = source.facts.current_armageddon < threshold;
    if !clock_open {
        return Err(Frame0ComputeScoreError::GetArmageddonClockUnexpectedlyClosed);
    }
    let local_prefix_digest = armageddon_local_prefix_digest(source, parent, threshold, clock_open);
    let open = Frame0UnitScoreFirstCensusRequest {
        request_sha256: first_unit_census_request_digest(
            score_authority,
            source,
            local_prefix_digest,
        ),
        parent_authority_digest: score_authority.composition_digest,
        armageddon_receipt_digest: local_prefix_digest,
        call_entry_sim_sha256: score_authority.capture.call_entry_sim_sha256,
        receiver_owner: score_authority.capture.owner,
        type_index: FIRST_UNIT_SCORE_TYPE,
        num_queued_read_va: UNIT_SCORE_NUM_QUEUED_READ_VA,
        num_queued_leader_offset: FIRST_UNIT_NUM_QUEUED_LEADER_OFFSET,
        num_units_read_va: UNIT_SCORE_NUM_UNITS_READ_VA,
        num_units_leader_offset: FIRST_UNIT_NUM_UNITS_LEADER_OFFSET,
        input_surface: Frame0UnitScoreFirstCensusInputSurface::ExactPostPlanType50CountPair,
        next_child_callsite_if_nonzero: UNIT_SCORE_VALUE_CALL_VA,
        next_child_vtable_offset_if_nonzero: UNIT_SCORE_VALUE_VTABLE_OFFSET,
    };

    Ok(Frame0ComputeUnitScoreArmageddonPlan {
        authority_revision: score_authority.revision,
        parent_authority_digest: score_authority.composition_digest,
        parent_local_prefix_digest: parent.local_prefix_digest,
        parent_request_sha256: parent.open.request_sha256,
        source_authority_digest: source.composition_digest,
        get_armageddon_callsite_va: GET_ARMAGEDDON_CALL_VA,
        get_armageddon_callee_va: GET_ARMAGEDDON_VA,
        returned_threshold: threshold,
        current_armageddon: source.facts.current_armageddon,
        comparison_load_va: ARMAGEDDON_COMPARE_LOAD_VA,
        comparison_va: ARMAGEDDON_COMPARE_VA,
        clock_open,
        local_prefix_digest,
        open,
    })
}

/// Freeze the first exact producer/capture gap after the Armageddon comparison.
///
/// Retail has concrete writers for both count cells, and canonical `Sim` duplicates them in its
/// Victory and production Leader mirrors. Neither fact binds their values to the independently
/// captured native post-`plan_strategy` call entry. The smallest admissible continuation is a
/// two-`u16` projection from that same native snapshot, keyed by its existing Sim digest. Until
/// then setup Unit receipts are historical only and the conditional type-score child stays shut.
pub fn audit_golden_frame0_owner0_unit_score_count_pair_gap(
    score_authority: &Frame0ComputeScoreEntryAuthority,
    parent: &Frame0ComputeUnitScorePrefixPlan,
    armageddon_source: &Frame0GetArmageddonAuthority,
    armageddon: &Frame0ComputeUnitScoreArmageddonPlan,
) -> Result<Frame0UnitScoreCountPairCaptureGap, Frame0ComputeScoreError> {
    let expected = plan_golden_frame0_owner0_compute_unit_score_armageddon(
        score_authority,
        parent,
        armageddon_source,
    )?;
    if armageddon != &expected {
        return Err(Frame0ComputeScoreError::UnitScoreCountPairParentDisagreement);
    }

    let fields = [
        Frame0UnitScoreCountFieldCaptureGap {
            field: Frame0UnitScoreCountField::NumQueued50,
            leader_offset: FIRST_UNIT_NUM_QUEUED_LEADER_OFFSET,
            read_va: UNIT_SCORE_NUM_QUEUED_READ_VA,
            producer_va: TRACK_QUEUED_VA,
            producer_store_va: TRACK_QUEUED_COUNT_STORE_VA,
        },
        Frame0UnitScoreCountFieldCaptureGap {
            field: Frame0UnitScoreCountField::NumUnits0,
            leader_offset: FIRST_UNIT_NUM_UNITS_LEADER_OFFSET,
            read_va: UNIT_SCORE_NUM_UNITS_READ_VA,
            producer_va: TRACK_UNIT_TYPE_VA,
            producer_store_va: TRACK_UNIT_TYPE_COUNT_STORE_VA,
        },
    ];
    let mut gap = Frame0UnitScoreCountPairCaptureGap {
        parent_authority_digest: score_authority.composition_digest,
        armageddon_source_digest: armageddon.source_authority_digest,
        armageddon_prefix_digest: armageddon.local_prefix_digest,
        call_entry_sim_sha256: armageddon.open.call_entry_sim_sha256,
        receiver_owner: armageddon.open.receiver_owner,
        type_index: armageddon.open.type_index,
        fields,
        native_capture_projects_count_pair: false,
        canonical_sim_joined_to_native_capture: false,
        setup_receipts_admissible: false,
        can_select_next_child: false,
        conditional_next_child_callsite: armageddon.open.next_child_callsite_if_nonzero,
        conditional_next_child_vtable_offset: armageddon.open.next_child_vtable_offset_if_nonzero,
        gap_digest: [0; 32],
    };
    gap.gap_digest = unit_score_count_pair_gap_digest(&gap);
    Ok(gap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup_2024_frame0_plan_strategy::{
        bind_golden_frame0_owner0_plan_strategy_entry,
        plan_golden_frame0_owner0_plan_strategy_prefix, Frame0PlanStrategyCityImage,
        Frame0PlanStrategyEntryCapture, Frame0PlanStrategyEntrySource,
        Frame0PlanStrategyLeaderImage, GOLDEN_CENTER_BUILD_O, GOLDEN_CENTER_CITY_SLOT,
        PLAN_ENTRY_SCRATCH_DWORDS, RESOURCE_SLOTS,
    };

    fn hash(byte: u8) -> [u8; 32] {
        [byte; 32]
    }

    fn planner() -> Frame0PlanStrategyPrefixPlan {
        let capture = Frame0PlanStrategyEntryCapture {
            revision: 7,
            source: Frame0PlanStrategyEntrySource::CompleteRetailOwnerZeroPlanStrategyEntry,
            replay_file_sha256: REPLAY_FILE_SHA256,
            executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
            completed_setup_sim_sha256: hash(1),
            setup_composition_digest: hash(2),
            preceding_chronology_digest: hash(3),
            call_entry_sim_sha256: hash(4),
            native_trace_sha256: hash(5),
            frame: 0,
            step: 11,
            owner: 0,
            strategy_ordinal: 0,
            leader: Frame0PlanStrategyLeaderImage {
                leader_flags: ACTIVE_HUMAN_MASK,
                who: 0,
                city_num: 1,
                village_num: 1,
                city_mark: 1,
                escrow_rate: [0; RESOURCE_SLOTS],
                scratch: [0; PLAN_ENTRY_SCRATCH_DWORDS],
            },
            cities: vec![Frame0PlanStrategyCityImage {
                slot: GOLDEN_CENTER_CITY_SLOT,
                city_flags: 1,
                city: GOLDEN_CENTER_CITY_SLOT,
                center_o: GOLDEN_CENTER_BUILD_O,
                who: 0,
                peasant_dist: 0,
                free: 0,
                busy: 0,
                gatherers: 0,
            }],
        };
        let authority = bind_golden_frame0_owner0_plan_strategy_entry(capture).unwrap();
        plan_golden_frame0_owner0_plan_strategy_prefix(&authority).unwrap()
    }

    fn market_receipt() -> MarketLeaderAccountingReceipt {
        let source = MarketLeaderRegionAuthority {
            capture_revision: 11,
            native_trace_sha256: hash(0x51),
            after_sim_sha256: hash(0x52),
            owner: GOLDEN_MARKET_OWNER,
            object_id: GOLDEN_MARKET_OBJECT_ID,
            type_index: MARKET_TYPE,
            region: 7,
        };
        let linked = LinkedMarketBuildFacts {
            row: GOLDEN_MARKET_BUILD_ROW,
            owner: GOLDEN_MARKET_OWNER,
            object_id: GOLDEN_MARKET_OBJECT_ID,
            type_index: MARKET_TYPE,
            city: 0,
            active: true,
        };
        let leader = victory_score::LeaderState {
            leader_flags: ACTIVE_HUMAN_MASK,
            buildings_built: 1,
            ..victory_score::LeaderState::default()
        };
        let prepared =
            prepare_market_leader_accounting(source, linked, &leader, ACTIVE_HUMAN_MASK as u32)
                .unwrap();
        MarketLeaderAccountingReceipt {
            source: prepared.source,
            linked: prepared.linked,
            regional_index: prepared.regional_index,
            before: prepared.before,
            after_init: prepared.after_init,
            after_increment_stats: prepared.after_increment_stats,
            after_market_special: prepared.after_market_special,
        }
    }

    fn capture(
        plan: &Frame0PlanStrategyPrefixPlan,
        market: &MarketLeaderAccountingReceipt,
    ) -> Frame0ComputeScoreEntryCapture {
        let after = market.after_market_special;
        Frame0ComputeScoreEntryCapture {
            revision: 13,
            source: Frame0ComputeScoreEntrySource::CompleteRetailOwnerZeroPostPlanStrategy,
            replay_file_sha256: REPLAY_FILE_SHA256,
            executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
            plan_entry_authority_digest: plan.authority_digest,
            plan_local_prefix_digest: plan.local_prefix_digest,
            plan_first_open_request_sha256: plan.open.request_sha256,
            plan_strategy_return_sha256: hash(0x61),
            call_entry_sim_sha256: hash(0x62),
            native_trace_sha256: hash(0x63),
            market_pre_mount_sim_sha256: market.source.after_sim_sha256,
            market_capture_revision: market.source.capture_revision,
            frame: GOLDEN_FRAME,
            step: GOLDEN_STEP,
            owner: GOLDEN_OWNER,
            strategy_ordinal: GOLDEN_STRATEGY_ORDINAL,
            force: GOLDEN_FORCE,
            leader: Frame0ComputeScoreLeaderImage {
                leader_flags: (after.flags as i32) | leader_flag::NEW_UNITS,
                who: 0,
                score_explored: 91,
                buildings_built: after.buildings_built,
                market_num_buildings: after.num_buildings,
                market_regional_buildings: after.regional_buildings,
                market_gather_slots: after.gather_slots,
                market_gather_slots_high: after.gather_slots_high,
                market_high_buildings: after.high_buildings,
            },
        }
    }

    fn score_authority_and_unit_prefix() -> (
        Frame0ComputeScoreEntryAuthority,
        Frame0ComputeUnitScorePrefixPlan,
    ) {
        let planner = planner();
        let market = market_receipt();
        let authority = bind_golden_frame0_owner0_compute_score_entry(
            &planner,
            capture(&planner, &market),
            market,
        )
        .unwrap();
        let score = plan_golden_frame0_owner0_compute_score_prefix(&authority).unwrap();
        let unit = plan_golden_frame0_owner0_compute_unit_score_prefix(&authority, &score).unwrap();
        (authority, unit)
    }

    fn armageddon_source(
        authority: &Frame0ComputeScoreEntryAuthority,
        unit: &Frame0ComputeUnitScorePrefixPlan,
    ) -> Frame0GetArmageddonAuthority {
        let mut source = Frame0GetArmageddonAuthority {
            revision: authority.revision,
            composition_digest: [0; 32],
            source: Frame0GetArmageddonSource::ReplayRulesInitialGameAndSetupTeamTransaction,
            replay_file_sha256: REPLAY_FILE_SHA256,
            replay_payload_sha256: hash(0x71),
            rules_serialized_sha256: hash(0x72),
            rules_constants_payload_offset: 0x1234,
            team_setup_source_sha256: hash(0x73),
            parent_authority_digest: authority.composition_digest,
            parent_local_prefix_digest: unit.local_prefix_digest,
            parent_request_sha256: unit.open.request_sha256,
            facts: Frame0ArmageddonThresholdFacts {
                armageddon: 4,
                armageddon_per_nation: 1,
                armageddon_per_team: 2,
                num_nations: 1,
                num_sides: 1,
                starting_resources: 0,
                current_armageddon: 0,
            },
        };
        source.composition_digest = frame0_get_armageddon_authority_digest(&source);
        source
    }

    #[test]
    fn owner_zero_stops_at_unit_score_and_preserves_market_wealth() {
        let plan = planner();
        let market = market_receipt();
        let capture = capture(&plan, &market);
        let unchanged = capture.clone();
        let authority =
            bind_golden_frame0_owner0_compute_score_entry(&plan, capture, market).unwrap();
        let prefix = plan_golden_frame0_owner0_compute_score_prefix(&authority).unwrap();

        assert_eq!(authority.capture, unchanged);
        assert_eq!(prefix.score_explored_store_va, SCORE_EXPLORED_STORE_VA);
        assert_eq!(prefix.score_explored_store_before, 91);
        assert_eq!(prefix.score_explored_store_after, 0);
        assert_eq!(prefix.leader_after_local.market_num_buildings, 1);
        assert_eq!(prefix.leader_after_local.market_gather_slots, 1);
        assert_eq!(prefix.leader_after_local.market_gather_slots_high, 1);
        assert_eq!(prefix.open.callsite_va, COMPUTE_UNIT_SCORE_CALL_VA);
        assert_eq!(prefix.open.callee_va, COMPUTE_UNIT_SCORE_VA);
        assert_eq!(
            prefix.open.next_callsite_if_complete,
            COMPUTE_BUILD_SCORE_CALL_VA
        );
        assert_eq!(prefix.open.next_callee_if_complete, COMPUTE_BUILD_SCORE_VA);
        assert_ne!(prefix.open.request_sha256, [0; 32]);
    }

    #[test]
    fn unit_score_resets_both_scores_then_stops_at_armageddon_threshold() {
        let planner = planner();
        let market = market_receipt();
        let authority = bind_golden_frame0_owner0_compute_score_entry(
            &planner,
            capture(&planner, &market),
            market,
        )
        .unwrap();
        let score = plan_golden_frame0_owner0_compute_score_prefix(&authority).unwrap();
        let unit = plan_golden_frame0_owner0_compute_unit_score_prefix(&authority, &score).unwrap();

        assert_eq!(
            unit.writes,
            [
                Frame0ComputeUnitScoreWrite {
                    store_va: SCORE_UNITS_STORE_VA,
                    field: Frame0ComputeUnitScoreField::ScoreUnits,
                    after: 0,
                },
                Frame0ComputeUnitScoreWrite {
                    store_va: SCORE_UNITS_2_STORE_VA,
                    field: Frame0ComputeUnitScoreField::ScoreUnitsAttack,
                    after: 0,
                },
            ]
        );
        assert_eq!(unit.parent_request_sha256, score.open.request_sha256);
        assert_eq!(
            unit.market_accounting_receipt_sha256,
            score.market_accounting_receipt_sha256
        );
        assert_eq!(unit.open.callsite_va, GET_ARMAGEDDON_CALL_VA);
        assert_eq!(unit.open.callee_va, GET_ARMAGEDDON_VA);
        assert_eq!(
            unit.open.input_surface,
            Frame0GetArmageddonInputSurface::CompleteRulesAndLiveGameThresholdProjection
        );
        assert_eq!(
            unit.open.comparison_load_va_if_complete,
            ARMAGEDDON_COMPARE_LOAD_VA
        );
        assert_eq!(
            unit.open.census_first_va_if_clock_open,
            UNIT_SCORE_CENSUS_FIRST_VA
        );
        assert_ne!(unit.local_prefix_digest, [0; 32]);
        assert_ne!(unit.open.request_sha256, [0; 32]);
    }

    #[test]
    fn armageddon_child_returns_seven_then_stops_at_live_type50_counts() {
        let (authority, unit) = score_authority_and_unit_prefix();
        let source = armageddon_source(&authority, &unit);
        let plan =
            plan_golden_frame0_owner0_compute_unit_score_armageddon(&authority, &unit, &source)
                .unwrap();

        assert_eq!(plan.returned_threshold, 7);
        assert_eq!(plan.current_armageddon, 0);
        assert!(plan.clock_open);
        assert_eq!(plan.comparison_load_va, ARMAGEDDON_COMPARE_LOAD_VA);
        assert_eq!(plan.comparison_va, ARMAGEDDON_COMPARE_VA);
        assert_eq!(plan.open.type_index, FIRST_UNIT_SCORE_TYPE);
        assert_eq!(plan.open.num_queued_read_va, UNIT_SCORE_NUM_QUEUED_READ_VA);
        assert_eq!(
            plan.open.num_queued_leader_offset,
            FIRST_UNIT_NUM_QUEUED_LEADER_OFFSET
        );
        assert_eq!(plan.open.num_units_read_va, UNIT_SCORE_NUM_UNITS_READ_VA);
        assert_eq!(
            plan.open.num_units_leader_offset,
            FIRST_UNIT_NUM_UNITS_LEADER_OFFSET
        );
        assert_eq!(
            plan.open.input_surface,
            Frame0UnitScoreFirstCensusInputSurface::ExactPostPlanType50CountPair
        );
        assert_eq!(
            plan.open.next_child_callsite_if_nonzero,
            UNIT_SCORE_VALUE_CALL_VA
        );
        assert_eq!(
            plan.open.next_child_vtable_offset_if_nonzero,
            UNIT_SCORE_VALUE_VTABLE_OFFSET
        );
        assert_ne!(plan.local_prefix_digest, [0; 32]);
        assert_ne!(plan.open.request_sha256, [0; 32]);
    }

    #[test]
    fn type50_count_pair_freezes_exact_native_capture_gap() {
        let (authority, unit) = score_authority_and_unit_prefix();
        let source = armageddon_source(&authority, &unit);
        let armageddon =
            plan_golden_frame0_owner0_compute_unit_score_armageddon(&authority, &unit, &source)
                .unwrap();
        let gap = audit_golden_frame0_owner0_unit_score_count_pair_gap(
            &authority,
            &unit,
            &source,
            &armageddon,
        )
        .unwrap();

        assert_eq!(
            gap.call_entry_sim_sha256,
            authority.capture.call_entry_sim_sha256
        );
        assert_eq!(gap.receiver_owner, GOLDEN_OWNER);
        assert_eq!(gap.type_index, FIRST_UNIT_SCORE_TYPE);
        assert_eq!(
            gap.fields,
            [
                Frame0UnitScoreCountFieldCaptureGap {
                    field: Frame0UnitScoreCountField::NumQueued50,
                    leader_offset: FIRST_UNIT_NUM_QUEUED_LEADER_OFFSET,
                    read_va: UNIT_SCORE_NUM_QUEUED_READ_VA,
                    producer_va: TRACK_QUEUED_VA,
                    producer_store_va: TRACK_QUEUED_COUNT_STORE_VA,
                },
                Frame0UnitScoreCountFieldCaptureGap {
                    field: Frame0UnitScoreCountField::NumUnits0,
                    leader_offset: FIRST_UNIT_NUM_UNITS_LEADER_OFFSET,
                    read_va: UNIT_SCORE_NUM_UNITS_READ_VA,
                    producer_va: TRACK_UNIT_TYPE_VA,
                    producer_store_va: TRACK_UNIT_TYPE_COUNT_STORE_VA,
                },
            ]
        );
        assert!(!gap.native_capture_projects_count_pair);
        assert!(!gap.canonical_sim_joined_to_native_capture);
        assert!(!gap.setup_receipts_admissible);
        assert!(!gap.can_select_next_child);
        assert_eq!(
            gap.conditional_next_child_callsite,
            UNIT_SCORE_VALUE_CALL_VA
        );
        assert_eq!(
            gap.conditional_next_child_vtable_offset,
            UNIT_SCORE_VALUE_VTABLE_OFFSET
        );
        assert_ne!(gap.gap_digest, [0; 32]);
    }

    #[test]
    fn type50_count_pair_gap_refuses_mutated_parent() {
        let (authority, unit) = score_authority_and_unit_prefix();
        let source = armageddon_source(&authority, &unit);
        let mut armageddon =
            plan_golden_frame0_owner0_compute_unit_score_armageddon(&authority, &unit, &source)
                .unwrap();
        armageddon.open.num_queued_read_va ^= 1;

        assert_eq!(
            audit_golden_frame0_owner0_unit_score_count_pair_gap(
                &authority,
                &unit,
                &source,
                &armageddon,
            )
            .unwrap_err(),
            Frame0ComputeScoreError::UnitScoreCountPairParentDisagreement
        );
    }

    #[test]
    fn armageddon_source_drift_and_closed_clock_bite() {
        let (authority, unit) = score_authority_and_unit_prefix();
        let mut stale = armageddon_source(&authority, &unit);
        stale.facts.armageddon_per_team ^= 1;
        assert_eq!(
            plan_golden_frame0_owner0_compute_unit_score_armageddon(&authority, &unit, &stale)
                .unwrap_err(),
            Frame0ComputeScoreError::GetArmageddonStaleAuthority
        );

        let mut closed = armageddon_source(&authority, &unit);
        closed.facts.current_armageddon = 7;
        closed.composition_digest = frame0_get_armageddon_authority_digest(&closed);
        assert_eq!(
            plan_golden_frame0_owner0_compute_unit_score_armageddon(&authority, &unit, &closed)
                .unwrap_err(),
            Frame0ComputeScoreError::GetArmageddonClockUnexpectedlyClosed
        );
    }

    #[test]
    fn market_or_planner_cross_wiring_refuses() {
        let plan = planner();
        let market = market_receipt();
        let mut wrong_plan = capture(&plan, &market);
        wrong_plan.plan_local_prefix_digest[0] ^= 1;
        assert_eq!(
            bind_golden_frame0_owner0_compute_score_entry(&plan, wrong_plan, market).unwrap_err(),
            Frame0ComputeScoreError::PlanPrefixDisagreement
        );

        let market = market_receipt();
        let mut wrong_market = market;
        wrong_market.after_market_special.gather_slots = 0;
        assert_eq!(
            bind_golden_frame0_owner0_compute_score_entry(
                &plan,
                capture(&plan, &market),
                wrong_market,
            )
            .unwrap_err(),
            Frame0ComputeScoreError::InvalidMarketAccounting
        );
    }

    #[test]
    fn missing_unit_dirty_bit_and_stale_authority_bite() {
        let plan = planner();
        let market = market_receipt();
        let mut no_units = capture(&plan, &market);
        no_units.leader.leader_flags &= !leader_flag::NEW_UNITS;
        assert_eq!(
            bind_golden_frame0_owner0_compute_score_entry(&plan, no_units, market).unwrap_err(),
            Frame0ComputeScoreError::MissingNewUnitsDirtyBit
        );

        let market = market_receipt();
        let mut authority =
            bind_golden_frame0_owner0_compute_score_entry(&plan, capture(&plan, &market), market)
                .unwrap();
        authority.capture.leader.score_explored ^= 1;
        assert_eq!(
            plan_golden_frame0_owner0_compute_score_prefix(&authority).unwrap_err(),
            Frame0ComputeScoreError::StaleAuthority
        );
    }

    #[test]
    fn unit_score_refuses_a_cross_wired_or_mutated_parent() {
        let planner = planner();
        let market = market_receipt();
        let authority = bind_golden_frame0_owner0_compute_score_entry(
            &planner,
            capture(&planner, &market),
            market,
        )
        .unwrap();
        let score = plan_golden_frame0_owner0_compute_score_prefix(&authority).unwrap();

        let mut wrong_request = score.clone();
        wrong_request.open.request_sha256[0] ^= 1;
        assert_eq!(
            plan_golden_frame0_owner0_compute_unit_score_prefix(&authority, &wrong_request)
                .unwrap_err(),
            Frame0ComputeScoreError::ComputeUnitScoreParentDisagreement
        );

        let mut wrong_store = score;
        wrong_store.score_explored_store_after = 1;
        assert_eq!(
            plan_golden_frame0_owner0_compute_unit_score_prefix(&authority, &wrong_store)
                .unwrap_err(),
            Frame0ComputeScoreError::ComputeUnitScoreParentDisagreement
        );
    }
}
