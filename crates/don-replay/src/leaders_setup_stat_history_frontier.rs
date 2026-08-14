// SPDX-License-Identifier: GPL-3.0-or-later
//! Setup-surviving Leader gather, score, war, garrison, and action history.
//!
//! These three disjoint fixed-body ranges retain their exact constructor values through the
//! ordinary one-Village setup. They expire before the first plan/gather/combat or relevant
//! player action and are not a live-state producer.

#![forbid(unsafe_code)]

use crate::leader_initial_prefix::CHECKSUM_LEADER_SLOTS;
use crate::leaders_lifetime_mask_header_frontier::RuntimeLeadersLifetimeMaskHeaderFrontier;
use crate::leaders_runtime_frontier::{LeadersWalkFrontier, RuntimeCoveredRange};
use crate::leaders_setup_rare_history_frontier::{
    derive_frame_zero_rare_history, FrameZeroRareHistoryError,
};
use crate::setup_cities_builds::StartingSetupState;

pub const GATHER_TRADE_HISTORY_BEGIN: usize = 0x8d4;
pub const GATHER_TRADE_HISTORY_END: usize = 0x8f8;
pub const BEST_WAR_HISTORY_BEGIN: usize = 0x99c;
pub const BEST_WAR_HISTORY_END: usize = 0x9bc;
pub const INACTIVE_ACTION_HISTORY_BEGIN: usize = 0xa28;
pub const INACTIVE_ACTION_HISTORY_END: usize = 0xa4c;
pub const ATTACKED_BY_OFFSET: usize = 0xa44;
pub const GOV_HERO_FRAME_OFFSET: usize = 0xa48;
pub const FRAME_ZERO_STAT_HISTORY_WALKED_BYTES: usize = (GATHER_TRADE_HISTORY_END
    - GATHER_TRADE_HISTORY_BEGIN)
    + (BEST_WAR_HISTORY_END - BEST_WAR_HISTORY_BEGIN)
    + (INACTIVE_ACTION_HISTORY_END - INACTIVE_ACTION_HISTORY_BEGIN);

pub const LEADER_RESET_SCORE_VA: u32 = 0x006e_37f0;
pub const LEADER_RESET_SCORE_TRADE_PROFIT_ZERO_VA: u32 = 0x006e_38b3;
pub const LEADER_RESET_SCORE_FORTS_BUILT_ZERO_VA: u32 = 0x006e_38b9;
pub const LEADER_RESET_SCORE_UNITS_BRIBED_ZERO_VA: u32 = 0x006e_38bf;
pub const PLAN_STRATEGY_GATHER_HIGH_BEGIN_VA: u32 = 0x006b_b25c;
pub const PLAN_STRATEGY_GATHER_HIGH_FIRST_STORE_VA: u32 = 0x006b_b276;
pub const PLAN_STRATEGY_GATHER_HIGH_END_VA: u32 = 0x006b_b2d8;
pub const LEADER_INIT_BEST_STATS_BEGIN_VA: u32 = 0x006e_4a60;
pub const LEADER_INIT_BEST_STATS_END_VA: u32 = 0x006e_4a73;
pub const PLAN_STRATEGY_WAR_RESET_BEGIN_VA: u32 = 0x006b_b95c;
pub const PLAN_STRATEGY_WAR_RESET_END_VA: u32 = 0x006b_b982;
pub const LEADER_INIT_GARRISON_STATS_BEGIN_VA: u32 = 0x006e_4c62;
pub const LEADER_INIT_GARRISON_STATS_END_VA: u32 = 0x006e_4c75;
pub const LEADER_INIT_ATTACK_STATS_BEGIN_VA: u32 = 0x006e_4a9c;
pub const LEADER_INIT_ATTACK_STATS_END_VA: u32 = 0x006e_4aaf;
pub const LEADER_INIT_GOV_HERO_FRAME_VA: u32 = 0x006e_3b6e;

const _: () = assert!(FRAME_ZERO_STAT_HISTORY_WALKED_BYTES == 104);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameZeroStatHistorySource {
    LeaderInitBeforeFirstPlanGatherOrAction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameZeroStatHistoryClaim {
    pub slot: u8,
    pub newly_canonical_walked_bytes: usize,
    pub source: FrameZeroStatHistorySource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameZeroStatHistory {
    active: [bool; CHECKSUM_LEADER_SLOTS],
    claims: Vec<FrameZeroStatHistoryClaim>,
}

impl FrameZeroStatHistory {
    pub fn claims(&self) -> &[FrameZeroStatHistoryClaim] {
        &self.claims
    }

    pub fn newly_canonical_walked_bytes(&self) -> usize {
        self.claims.len() * FRAME_ZERO_STAT_HISTORY_WALKED_BYTES
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeLeadersFrameZeroStatHistoryFrontier {
    inner: Box<RuntimeLeadersLifetimeMaskHeaderFrontier>,
    history: FrameZeroStatHistory,
}

impl RuntimeLeadersFrameZeroStatHistoryFrontier {
    pub fn inner(&self) -> &RuntimeLeadersLifetimeMaskHeaderFrontier {
        self.inner.as_ref()
    }

    pub fn history(&self) -> &FrameZeroStatHistory {
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
pub enum FrameZeroStatHistoryError {
    Setup(FrameZeroRareHistoryError),
    ConditionalRosterDisagreement {
        slot: usize,
        history_active: bool,
        transcript_active: bool,
    },
    ConditionalDisagreement {
        slot: usize,
        begin: usize,
        byte: usize,
        expected: u8,
        conditional: u8,
    },
}

impl std::fmt::Display for FrameZeroStatHistoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "frame-zero Leader stat history refused: {self:?}")
    }
}

impl std::error::Error for FrameZeroStatHistoryError {}

impl From<FrameZeroRareHistoryError> for FrameZeroStatHistoryError {
    fn from(value: FrameZeroRareHistoryError) -> Self {
        Self::Setup(value)
    }
}

pub fn derive_frame_zero_stat_history(
    setup: &StartingSetupState,
) -> Result<FrameZeroStatHistory, FrameZeroStatHistoryError> {
    let rare = derive_frame_zero_rare_history(setup)?;
    let mut active = [false; CHECKSUM_LEADER_SLOTS];
    let mut claims = Vec::with_capacity(rare.claims().len());
    for claim in rare.claims() {
        let slot = usize::from(claim.slot);
        active[slot] = true;
        claims.push(FrameZeroStatHistoryClaim {
            slot: claim.slot,
            newly_canonical_walked_bytes: FRAME_ZERO_STAT_HISTORY_WALKED_BYTES,
            source: FrameZeroStatHistorySource::LeaderInitBeforeFirstPlanGatherOrAction,
        });
    }
    Ok(FrameZeroStatHistory { active, claims })
}

pub fn bind_frame_zero_stat_history(
    inner: RuntimeLeadersLifetimeMaskHeaderFrontier,
    history: FrameZeroStatHistory,
) -> Result<RuntimeLeadersFrameZeroStatHistoryFrontier, FrameZeroStatHistoryError> {
    let init = inner.inner().inner().inner();
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        let transcript_active = init
            .conditional_row_active(slot)
            .expect("the complete Leader transcript has ten rows");
        if history.active[slot] != transcript_active {
            return Err(FrameZeroStatHistoryError::ConditionalRosterDisagreement {
                slot,
                history_active: history.active[slot],
                transcript_active,
            });
        }
        if !transcript_active {
            continue;
        }
        for range in [
            RuntimeCoveredRange {
                begin: GATHER_TRADE_HISTORY_BEGIN,
                end: GATHER_TRADE_HISTORY_END,
            },
            RuntimeCoveredRange {
                begin: BEST_WAR_HISTORY_BEGIN,
                end: BEST_WAR_HISTORY_END,
            },
            RuntimeCoveredRange {
                begin: INACTIVE_ACTION_HISTORY_BEGIN,
                end: INACTIVE_ACTION_HISTORY_END,
            },
        ] {
            let conditional = init
                .conditional_fixed_slice(slot, range)
                .expect("the complete conditional frontier owns stat history");
            let mut expected = vec![0; range.end - range.begin];
            for offset in [ATTACKED_BY_OFFSET, GOV_HERO_FRAME_OFFSET] {
                if range.begin <= offset && offset + 4 <= range.end {
                    expected[offset - range.begin..offset - range.begin + 4].fill(0xff);
                }
            }
            if let Some(byte) = conditional
                .iter()
                .zip(&expected)
                .position(|(conditional, expected)| conditional != expected)
            {
                return Err(FrameZeroStatHistoryError::ConditionalDisagreement {
                    slot,
                    begin: range.begin,
                    byte,
                    expected: expected[byte],
                    conditional: conditional[byte],
                });
            }
        }
    }
    Ok(RuntimeLeadersFrameZeroStatHistoryFrontier {
        inner: Box::new(inner),
        history,
    })
}
