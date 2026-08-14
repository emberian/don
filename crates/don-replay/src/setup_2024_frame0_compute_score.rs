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
use don_sim::systems::unit_inctime::SUPPORTED_RETAIL_EXE_SHA256;
use don_sim::systems::victory_score::{self, leader_flag};

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
pub const UNIT_SCORE_CENSUS_FIRST_VA: u32 = 0x006b_c540;

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
