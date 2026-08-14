// SPDX-License-Identifier: GPL-3.0-or-later
//! Constructor-bound rare-resource gather history.
//!
//! `Leader::init` explicitly clears `known_rares` and `rares_collected[44]`. Ordinary setup
//! can append to the separate dynamic `new_rares` list, but does not reach the fixed history;
//! the first `Leader::calc_gather` clears and recomputes this cohort. This authority therefore
//! expires before that first gather pass and is never a live checksum producer.

#![forbid(unsafe_code)]

use crate::leader_initial_prefix::CHECKSUM_LEADER_SLOTS;
use crate::leaders_runtime_frontier::{LeadersWalkFrontier, RuntimeCoveredRange};
use crate::leaders_setup_plan_scratch_frontier::{
    derive_frame_zero_plan_scratch, FrameZeroPlanScratchError,
    RuntimeLeadersFrameZeroPlanScratchFrontier,
};
use crate::setup_cities_builds::StartingSetupState;

pub const KNOWN_RARES_BEGIN: usize = 0x6d4;
pub const KNOWN_RARES_END: usize = 0x6d8;
pub const RARES_COLLECTED_BEGIN: usize = 0x6d8;
pub const RARES_COLLECTED_END: usize = 0x788;
pub const RARES_COLLECTED_VALUES: usize = 44;
pub const FRAME_ZERO_RARE_HISTORY_WALKED_BYTES: usize = RARES_COLLECTED_END - KNOWN_RARES_BEGIN;

pub const LEADER_INIT_KNOWN_RARES_ZERO_VA: u32 = 0x006e_4aba;
pub const LEADER_INIT_RARES_COLLECTED_FILL_BEGIN_VA: u32 = 0x006e_4ac0;
pub const LEADER_INIT_RARES_COLLECTED_FILL_END_VA: u32 = 0x006e_4acc;
pub const LEADER_CALC_GATHER_VA: u32 = 0x006c_eee0;
pub const LEADER_CALC_GATHER_KNOWN_RARES_CLEAR_VA: u32 = 0x006c_ef42;
pub const LEADER_CALC_GATHER_REG_KNOWN_RARES_SUM_BEGIN_VA: u32 = 0x006c_ef4b;
pub const LEADER_CALC_GATHER_REG_KNOWN_RARES_SUM_END_VA: u32 = 0x006c_ef56;
pub const LEADER_CALC_GATHER_RARES_COLLECTED_CLEAR_BEGIN_VA: u32 = 0x006c_ef5b;
pub const LEADER_CALC_GATHER_RARES_COLLECTED_CLEAR_END_VA: u32 = 0x006c_ef6d;
pub const LEADER_GATHER_VA: u32 = 0x006c_e280;

const _: () = assert!(RARES_COLLECTED_END - RARES_COLLECTED_BEGIN == RARES_COLLECTED_VALUES * 4);
const _: () = assert!(FRAME_ZERO_RARE_HISTORY_WALKED_BYTES == 180);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameZeroRareHistorySource {
    LeaderInitBeforeFirstCalcGather,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameZeroRareHistoryClaim {
    pub slot: u8,
    pub source: FrameZeroRareHistorySource,
    pub newly_canonical_walked_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameZeroRareHistory {
    active: [bool; CHECKSUM_LEADER_SLOTS],
    claims: Vec<FrameZeroRareHistoryClaim>,
}

impl FrameZeroRareHistory {
    pub fn claims(&self) -> &[FrameZeroRareHistoryClaim] {
        &self.claims
    }

    pub fn newly_canonical_walked_bytes(&self) -> usize {
        self.claims.len() * FRAME_ZERO_RARE_HISTORY_WALKED_BYTES
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeLeadersFrameZeroRareHistoryFrontier {
    inner: Box<RuntimeLeadersFrameZeroPlanScratchFrontier>,
    history: FrameZeroRareHistory,
}

impl RuntimeLeadersFrameZeroRareHistoryFrontier {
    pub fn inner(&self) -> &RuntimeLeadersFrameZeroPlanScratchFrontier {
        self.inner.as_ref()
    }

    pub fn history(&self) -> &FrameZeroRareHistory {
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
pub enum FrameZeroRareHistoryError {
    Setup(FrameZeroPlanScratchError),
    ConditionalRosterDisagreement {
        slot: usize,
        history_active: bool,
        transcript_active: bool,
    },
    ConditionalDisagreement {
        slot: usize,
        byte: usize,
        conditional: u8,
    },
}

impl std::fmt::Display for FrameZeroRareHistoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "frame-zero Leader rare history refused: {self:?}")
    }
}

impl std::error::Error for FrameZeroRareHistoryError {}

impl From<FrameZeroPlanScratchError> for FrameZeroRareHistoryError {
    fn from(value: FrameZeroPlanScratchError) -> Self {
        Self::Setup(value)
    }
}

pub fn derive_frame_zero_rare_history(
    setup: &StartingSetupState,
) -> Result<FrameZeroRareHistory, FrameZeroRareHistoryError> {
    let scratch = derive_frame_zero_plan_scratch(setup)?;
    let mut active = [false; CHECKSUM_LEADER_SLOTS];
    let mut claims = Vec::with_capacity(scratch.claims().len());
    for claim in scratch.claims() {
        let slot = usize::from(claim.slot);
        active[slot] = true;
        claims.push(FrameZeroRareHistoryClaim {
            slot: claim.slot,
            source: FrameZeroRareHistorySource::LeaderInitBeforeFirstCalcGather,
            newly_canonical_walked_bytes: FRAME_ZERO_RARE_HISTORY_WALKED_BYTES,
        });
    }
    Ok(FrameZeroRareHistory { active, claims })
}

pub fn bind_frame_zero_rare_history(
    inner: RuntimeLeadersFrameZeroPlanScratchFrontier,
    history: FrameZeroRareHistory,
) -> Result<RuntimeLeadersFrameZeroRareHistoryFrontier, FrameZeroRareHistoryError> {
    let range = RuntimeCoveredRange {
        begin: KNOWN_RARES_BEGIN,
        end: RARES_COLLECTED_END,
    };
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        let transcript_active = inner
            .inner()
            .conditional_row_active(slot)
            .expect("the complete Leader transcript has ten rows");
        if history.active[slot] != transcript_active {
            return Err(FrameZeroRareHistoryError::ConditionalRosterDisagreement {
                slot,
                history_active: history.active[slot],
                transcript_active,
            });
        }
        if !transcript_active {
            continue;
        }
        let conditional = inner
            .inner()
            .conditional_fixed_slice(slot, range)
            .expect("the complete conditional frontier owns rare gather history");
        if let Some(byte) = conditional.iter().position(|value| *value != 0) {
            return Err(FrameZeroRareHistoryError::ConditionalDisagreement {
                slot,
                byte,
                conditional: conditional[byte],
            });
        }
    }
    Ok(RuntimeLeadersFrameZeroRareHistoryFrontier {
        inner: Box::new(inner),
        history,
    })
}
