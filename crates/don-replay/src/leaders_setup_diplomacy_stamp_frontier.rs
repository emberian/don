// SPDX-License-Identifier: GPL-3.0-or-later
//! Frame-zero Leader diplomacy, conquest-Hero, and repair action stamps.
//!
//! The fresh negative-init path clears this contiguous six-dword range. The ordinary setup
//! transaction reaches no diplomacy action, Conquer-the-World Hero action, or repair order,
//! so those exact values survive the published setup receipt. This is historical setup state,
//! not a live maintainer.

#![forbid(unsafe_code)]

use crate::leader_initial_prefix::CHECKSUM_LEADER_SLOTS;
use crate::leaders_runtime_frontier::{LeadersWalkFrontier, RuntimeCoveredRange};
use crate::leaders_setup_stat_history_frontier::{
    derive_frame_zero_stat_history, FrameZeroStatHistoryError,
    RuntimeLeadersFrameZeroStatHistoryFrontier,
};
use crate::setup_cities_builds::StartingSetupState;

pub const ACTION_STAMPS_BEGIN: usize = 0x1f4;
pub const ACTION_STAMPS_END: usize = 0x20c;
pub const FRAME_ZERO_ACTION_STAMPS_WALKED_BYTES: usize = ACTION_STAMPS_END - ACTION_STAMPS_BEGIN;

const _: () = assert!(FRAME_ZERO_ACTION_STAMPS_WALKED_BYTES == 24);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameZeroActionStampsSource {
    LeaderNegativeInitBeforeDiplomacyConquestOrRepairAction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameZeroActionStampsClaim {
    pub slot: u8,
    pub newly_canonical_walked_bytes: usize,
    pub source: FrameZeroActionStampsSource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameZeroActionStamps {
    active: [bool; CHECKSUM_LEADER_SLOTS],
    claims: Vec<FrameZeroActionStampsClaim>,
}

impl FrameZeroActionStamps {
    pub fn claims(&self) -> &[FrameZeroActionStampsClaim] {
        &self.claims
    }

    pub fn newly_canonical_walked_bytes(&self) -> usize {
        self.claims.len() * FRAME_ZERO_ACTION_STAMPS_WALKED_BYTES
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeLeadersFrameZeroActionStampsFrontier {
    inner: Box<RuntimeLeadersFrameZeroStatHistoryFrontier>,
    stamps: FrameZeroActionStamps,
}

impl RuntimeLeadersFrameZeroActionStampsFrontier {
    pub fn inner(&self) -> &RuntimeLeadersFrameZeroStatHistoryFrontier {
        self.inner.as_ref()
    }

    pub fn stamps(&self) -> &FrameZeroActionStamps {
        &self.stamps
    }

    pub fn newly_canonicalized_walked_bytes(&self) -> usize {
        self.stamps.newly_canonical_walked_bytes()
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
pub enum FrameZeroActionStampsError {
    Setup(FrameZeroStatHistoryError),
    ConditionalRosterDisagreement {
        slot: usize,
        stamps_active: bool,
        transcript_active: bool,
    },
    ConditionalDisagreement {
        slot: usize,
        byte: usize,
        conditional: u8,
    },
}

impl std::fmt::Display for FrameZeroActionStampsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "frame-zero Leader action stamps refused: {self:?}")
    }
}

impl std::error::Error for FrameZeroActionStampsError {}

impl From<FrameZeroStatHistoryError> for FrameZeroActionStampsError {
    fn from(value: FrameZeroStatHistoryError) -> Self {
        Self::Setup(value)
    }
}

pub fn derive_frame_zero_action_stamps(
    setup: &StartingSetupState,
) -> Result<FrameZeroActionStamps, FrameZeroActionStampsError> {
    let stat_history = derive_frame_zero_stat_history(setup)?;
    let mut active = [false; CHECKSUM_LEADER_SLOTS];
    let mut claims = Vec::with_capacity(stat_history.claims().len());
    for claim in stat_history.claims() {
        let slot = usize::from(claim.slot);
        active[slot] = true;
        claims.push(FrameZeroActionStampsClaim {
            slot: claim.slot,
            newly_canonical_walked_bytes: FRAME_ZERO_ACTION_STAMPS_WALKED_BYTES,
            source:
                FrameZeroActionStampsSource::LeaderNegativeInitBeforeDiplomacyConquestOrRepairAction,
        });
    }
    Ok(FrameZeroActionStamps { active, claims })
}

pub fn bind_frame_zero_action_stamps(
    inner: RuntimeLeadersFrameZeroStatHistoryFrontier,
    stamps: FrameZeroActionStamps,
) -> Result<RuntimeLeadersFrameZeroActionStampsFrontier, FrameZeroActionStampsError> {
    let init = inner.inner().inner().inner().inner();
    let range = RuntimeCoveredRange {
        begin: ACTION_STAMPS_BEGIN,
        end: ACTION_STAMPS_END,
    };
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        let transcript_active = init
            .conditional_row_active(slot)
            .expect("the complete Leader transcript has ten rows");
        if stamps.active[slot] != transcript_active {
            return Err(FrameZeroActionStampsError::ConditionalRosterDisagreement {
                slot,
                stamps_active: stamps.active[slot],
                transcript_active,
            });
        }
        if !transcript_active {
            continue;
        }
        let conditional = init
            .conditional_fixed_slice(slot, range)
            .expect("the complete conditional frontier owns action stamps");
        if let Some(byte) = conditional.iter().position(|value| *value != 0) {
            return Err(FrameZeroActionStampsError::ConditionalDisagreement {
                slot,
                byte,
                conditional: conditional[byte],
            });
        }
    }
    Ok(RuntimeLeadersFrameZeroActionStampsFrontier {
        inner: Box::new(inner),
        stamps,
    })
}
