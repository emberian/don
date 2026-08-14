// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact frame-zero ownership for the plain Leader production-economy caches.
//!
//! `Leader::init` clears the plain `econ`, `escrow`, `escrow_rate`, `base_rate`,
//! `worst_good`, `best_good`, and `shortages` fields. Ordinary setup reaches no
//! production-AI, market, gather, buy, or sell pass before the published setup receipt.
//! The adjacent `tributes[6]` row already has a runtime owner and is deliberately excluded.
//! This receipt expires before the first `Leader::plan_strategy` call.

#![forbid(unsafe_code)]

use crate::leader_initial_prefix::CHECKSUM_LEADER_SLOTS;
use crate::leaders_runtime_frontier::{LeadersWalkFrontier, RuntimeCoveredRange};
use crate::leaders_setup_fixed_tail_frontier::{
    derive_frame_zero_fixed_tail, FrameZeroFixedTailError, RuntimeLeadersFrameZeroFixedTailFrontier,
};
use crate::setup_cities_builds::StartingSetupState;

pub const FRAME_ZERO_ECONOMY_BEGIN: usize = 0x450;
pub const FRAME_ZERO_ECONOMY_END: usize = 0x498;
pub const FRAME_ZERO_PLANNING_RATE_BEGIN: usize = 0x4b0;
pub const FRAME_ZERO_PLANNING_RATE_END: usize = 0x4d4;
pub const FRAME_ZERO_ECONOMY_WALKED_BYTES: usize = (FRAME_ZERO_ECONOMY_END
    - FRAME_ZERO_ECONOMY_BEGIN)
    + (FRAME_ZERO_PLANNING_RATE_END - FRAME_ZERO_PLANNING_RATE_BEGIN);

pub const LEADER_INIT_ECONOMY_LOOP_BEGIN_VA: u32 = 0x006e_3de0;
pub const LEADER_INIT_ESCROW_ZERO_VA: u32 = 0x006e_3e86;
pub const LEADER_INIT_ESCROW_RATE_ZERO_VA: u32 = 0x006e_3e89;
pub const LEADER_INIT_BASE_RATE_ZERO_VA: u32 = 0x006e_3f04;
pub const LEADER_INIT_RATE_SCALARS_ZERO_BEGIN_VA: u32 = 0x006e_3f2c;
pub const LEADER_INIT_RATE_SCALARS_ZERO_END_VA: u32 = 0x006e_3f39;
pub const LEADER_INIT_ESCROW_RATE_CITY_GATE_VA: u32 = 0x006e_4105;
pub const LEADER_INIT_ESCROW_RATE_FILL_VA: u32 = 0x006e_411e;
pub const PLAN_STRATEGY_PRODUCTION_AI_CALL_VA: u32 = 0x006b_9662;
pub const PRODUCTION_AI_SETUP_CALL_VA: u32 = 0x006c_1abf;
pub const PRODUCTION_AI_MARKET_CALL_VA: u32 = 0x006c_8ad4;

const _: () = assert!(FRAME_ZERO_ECONOMY_WALKED_BYTES == 108);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameZeroEconomySource {
    LeaderInitBeforeFirstPlanOrEconomyAction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameZeroEconomyClaim {
    pub slot: u8,
    pub newly_canonical_walked_bytes: usize,
    pub source: FrameZeroEconomySource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameZeroEconomy {
    active: [bool; CHECKSUM_LEADER_SLOTS],
    claims: Vec<FrameZeroEconomyClaim>,
}

impl FrameZeroEconomy {
    pub fn claims(&self) -> &[FrameZeroEconomyClaim] {
        &self.claims
    }

    pub fn newly_canonical_walked_bytes(&self) -> usize {
        self.claims.len() * FRAME_ZERO_ECONOMY_WALKED_BYTES
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeLeadersFrameZeroEconomyFrontier {
    inner: Box<RuntimeLeadersFrameZeroFixedTailFrontier>,
    economy: FrameZeroEconomy,
}

impl RuntimeLeadersFrameZeroEconomyFrontier {
    pub fn inner(&self) -> &RuntimeLeadersFrameZeroFixedTailFrontier {
        self.inner.as_ref()
    }

    pub fn economy(&self) -> &FrameZeroEconomy {
        &self.economy
    }

    pub fn newly_canonicalized_walked_bytes(&self) -> usize {
        self.economy.newly_canonical_walked_bytes()
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
pub enum FrameZeroEconomyError {
    Setup(FrameZeroFixedTailError),
    ConditionalRosterDisagreement {
        slot: usize,
        economy_active: bool,
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

impl std::fmt::Display for FrameZeroEconomyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "frame-zero Leader economy refused: {self:?}")
    }
}

impl std::error::Error for FrameZeroEconomyError {}

impl From<FrameZeroFixedTailError> for FrameZeroEconomyError {
    fn from(value: FrameZeroFixedTailError) -> Self {
        Self::Setup(value)
    }
}

pub fn derive_frame_zero_economy(
    setup: &StartingSetupState,
) -> Result<FrameZeroEconomy, FrameZeroEconomyError> {
    // Re-run the complete ordinary-setup chain so this narrow constructor owner cannot
    // substitute either the roster or replay identity.
    let tail = derive_frame_zero_fixed_tail(setup)?;
    let mut active = [false; CHECKSUM_LEADER_SLOTS];
    let mut claims = Vec::with_capacity(tail.claims().len());
    for claim in tail.claims() {
        let slot = usize::from(claim.slot);
        active[slot] = true;
        claims.push(FrameZeroEconomyClaim {
            slot: claim.slot,
            newly_canonical_walked_bytes: FRAME_ZERO_ECONOMY_WALKED_BYTES,
            source: FrameZeroEconomySource::LeaderInitBeforeFirstPlanOrEconomyAction,
        });
    }
    Ok(FrameZeroEconomy { active, claims })
}

pub fn bind_frame_zero_economy(
    inner: RuntimeLeadersFrameZeroFixedTailFrontier,
    economy: FrameZeroEconomy,
) -> Result<RuntimeLeadersFrameZeroEconomyFrontier, FrameZeroEconomyError> {
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        let transcript_active = inner
            .inner()
            .conditional_row_active(slot)
            .expect("the complete Leader transcript has ten rows");
        if economy.active[slot] != transcript_active {
            return Err(FrameZeroEconomyError::ConditionalRosterDisagreement {
                slot,
                economy_active: economy.active[slot],
                transcript_active,
            });
        }
        if !transcript_active {
            continue;
        }
        for range in [
            RuntimeCoveredRange {
                begin: FRAME_ZERO_ECONOMY_BEGIN,
                end: FRAME_ZERO_ECONOMY_END,
            },
            RuntimeCoveredRange {
                begin: FRAME_ZERO_PLANNING_RATE_BEGIN,
                end: FRAME_ZERO_PLANNING_RATE_END,
            },
        ] {
            let conditional = inner
                .inner()
                .conditional_fixed_slice(slot, range)
                .expect("the complete conditional frontier owns the economy fixed body");
            if let Some(byte) = conditional.iter().position(|value| *value != 0) {
                return Err(FrameZeroEconomyError::ConditionalDisagreement {
                    slot,
                    begin: range.begin,
                    byte,
                    expected: 0,
                    conditional: conditional[byte],
                });
            }
        }
    }
    Ok(RuntimeLeadersFrameZeroEconomyFrontier {
        inner: Box::new(inner),
        economy,
    })
}
