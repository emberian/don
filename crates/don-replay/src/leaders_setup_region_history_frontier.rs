// SPDX-License-Identifier: GPL-3.0-or-later
//! Pre-strategy setup producer for the four 64-byte regional history arrays.
//!
//! `Leader::init` clears `reg_attacked`, `reg_wars`, `reg_neutrals`, and `reg_allies` in
//! its region loop. Their first executable writers are in `Leader::plan_strategy`; the
//! canonical setup receipt explicitly precedes that first-checkpoint census. This owner is
//! consequently valid only at the same constructor boundary as the starting-Build receipts.

#![forbid(unsafe_code)]

use crate::leader_initial_prefix::CHECKSUM_LEADER_SLOTS;
use crate::leaders_deferred_history_frontier::{
    REG_ATTACKED_BEGIN, REG_STRATEGY_HISTORY_END, REG_STRATEGY_HISTORY_VALUES,
};
use crate::leaders_runtime_frontier::{LeadersWalkFrontier, RuntimeCoveredRange};
use crate::leaders_setup_build_history_frontier::{
    derive_frame_zero_last_building_history, FrameZeroBuildHistoryError,
    RuntimeLeadersFrameZeroBuildHistoryFrontier,
};
use crate::setup_cities_builds::StartingSetupState;

pub const LEADER_INIT_REGION_HISTORY_BEGIN_VA: u32 = 0x006e_4b18;
pub const LEADER_INIT_REGION_HISTORY_LOOP_END_VA: u32 = 0x006e_4ba2;
pub const PLAN_STRATEGY_REG_ATTACKED_FIRST_STORE_VA: u32 = 0x006b_9bf8;
pub const PLAN_STRATEGY_RELATION_HISTORY_CLEAR_VA: u32 = 0x006b_bb82;
pub const PLAN_STRATEGY_RELATION_HISTORY_LAST_STORE_VA: u32 = 0x006b_bcc2;
pub const FRAME_ZERO_REG_STRATEGY_HISTORY_WALKED_BYTES: usize = REG_STRATEGY_HISTORY_VALUES;

const _: () = assert!(REG_STRATEGY_HISTORY_VALUES == 256);
const _: () = assert!(
    REG_STRATEGY_HISTORY_END - REG_ATTACKED_BEGIN == FRAME_ZERO_REG_STRATEGY_HISTORY_WALKED_BYTES
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameZeroRegionHistorySource {
    LeaderInitZeroBeforeFirstPlanStrategy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameZeroRegionHistoryClaim {
    pub slot: u8,
    pub regions_per_history: usize,
    pub histories: usize,
    pub source: FrameZeroRegionHistorySource,
    pub newly_canonical_walked_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameZeroRegionStrategyHistory {
    rows: [[u8; REG_STRATEGY_HISTORY_VALUES]; CHECKSUM_LEADER_SLOTS],
    active: [bool; CHECKSUM_LEADER_SLOTS],
    claims: Vec<FrameZeroRegionHistoryClaim>,
}

impl FrameZeroRegionStrategyHistory {
    pub fn row(&self, slot: usize) -> Option<&[u8; REG_STRATEGY_HISTORY_VALUES]> {
        self.rows.get(slot)
    }

    pub fn claims(&self) -> &[FrameZeroRegionHistoryClaim] {
        &self.claims
    }

    pub fn newly_canonical_walked_bytes(&self) -> usize {
        self.claims
            .iter()
            .map(|claim| claim.newly_canonical_walked_bytes)
            .sum()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeLeadersFrameZeroRegionHistoryFrontier {
    inner: Box<RuntimeLeadersFrameZeroBuildHistoryFrontier>,
    history: FrameZeroRegionStrategyHistory,
}

impl RuntimeLeadersFrameZeroRegionHistoryFrontier {
    pub fn inner(&self) -> &RuntimeLeadersFrameZeroBuildHistoryFrontier {
        self.inner.as_ref()
    }

    pub fn history(&self) -> &FrameZeroRegionStrategyHistory {
        &self.history
    }

    pub fn newly_canonicalized_walked_bytes(&self) -> usize {
        self.history.newly_canonical_walked_bytes()
    }

    pub fn unique_canonical_walked_bytes(&self) -> usize {
        self.inner.unique_canonical_walked_bytes() + self.newly_canonicalized_walked_bytes()
    }

    pub fn remaining_unsourced_walked_bytes(&self) -> u64 {
        self.walk_frontier()
            .bytes_walked
            .saturating_sub(self.unique_canonical_walked_bytes() as u64)
    }

    pub fn walk_frontier(&self) -> LeadersWalkFrontier {
        self.inner.walk_frontier()
    }

    pub fn checksum(&self) -> Result<(u32, u64), LeadersWalkFrontier> {
        Err(self.walk_frontier())
    }

    pub const fn installed_in_scoreboard(&self) -> bool {
        false
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameZeroRegionHistoryError {
    BuildHistory(FrameZeroBuildHistoryError),
    ConditionalRosterDisagreement {
        slot: usize,
        history_active: bool,
        transcript_active: bool,
    },
    ConditionalDisagreement {
        slot: usize,
        byte: usize,
        history: u8,
        conditional: u8,
    },
}

impl std::fmt::Display for FrameZeroRegionHistoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "frame-zero regional strategy history refused: {self:?}")
    }
}

impl std::error::Error for FrameZeroRegionHistoryError {}

impl From<FrameZeroBuildHistoryError> for FrameZeroRegionHistoryError {
    fn from(value: FrameZeroBuildHistoryError) -> Self {
        Self::BuildHistory(value)
    }
}

pub fn derive_frame_zero_region_strategy_history(
    setup: &StartingSetupState,
) -> Result<FrameZeroRegionStrategyHistory, FrameZeroRegionHistoryError> {
    let build_history = derive_frame_zero_last_building_history(setup)?;
    let rows = [[0; REG_STRATEGY_HISTORY_VALUES]; CHECKSUM_LEADER_SLOTS];
    let mut active = [false; CHECKSUM_LEADER_SLOTS];
    let mut claims = Vec::with_capacity(build_history.claims().len());
    for build in build_history.claims() {
        let slot = usize::from(build.slot);
        active[slot] = true;
        claims.push(FrameZeroRegionHistoryClaim {
            slot: build.slot,
            regions_per_history: 64,
            histories: 4,
            source: FrameZeroRegionHistorySource::LeaderInitZeroBeforeFirstPlanStrategy,
            newly_canonical_walked_bytes: FRAME_ZERO_REG_STRATEGY_HISTORY_WALKED_BYTES,
        });
    }
    Ok(FrameZeroRegionStrategyHistory {
        rows,
        active,
        claims,
    })
}

pub fn bind_frame_zero_region_strategy_history(
    inner: RuntimeLeadersFrameZeroBuildHistoryFrontier,
    history: FrameZeroRegionStrategyHistory,
) -> Result<RuntimeLeadersFrameZeroRegionHistoryFrontier, FrameZeroRegionHistoryError> {
    let deferred = inner.inner().inner().inner().inner().previous();
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        let transcript_active = deferred.rows()[slot].active;
        if history.active[slot] != transcript_active {
            return Err(FrameZeroRegionHistoryError::ConditionalRosterDisagreement {
                slot,
                history_active: history.active[slot],
                transcript_active,
            });
        }
        if !transcript_active {
            continue;
        }
        let conditional = deferred.rows()[slot]
            .conditionally_admitted_slice(RuntimeCoveredRange {
                begin: REG_ATTACKED_BEGIN,
                end: REG_STRATEGY_HISTORY_END,
            })
            .expect("the complete conditional frontier owns regional strategy history");
        if let Some(byte) = conditional
            .iter()
            .zip(history.rows[slot])
            .position(|(a, b)| *a != b)
        {
            return Err(FrameZeroRegionHistoryError::ConditionalDisagreement {
                slot,
                byte,
                history: history.rows[slot][byte],
                conditional: conditional[byte],
            });
        }
    }
    Ok(RuntimeLeadersFrameZeroRegionHistoryFrontier {
        inner: Box::new(inner),
        history,
    })
}
