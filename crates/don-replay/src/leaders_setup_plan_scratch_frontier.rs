// SPDX-License-Identifier: GPL-3.0-or-later
//! Constructor-bound Leader strategy scratch and regional-census zero owner.
//!
//! `Leader::init` clears these exact disjoint fixed-body ranges. The first
//! `Leader::plan_strategy` pass clears or recomputes every admitted byte, so the claim expires
//! before that pass and cannot be used as a later-frame producer.

#![forbid(unsafe_code)]

use crate::leader_initial_prefix::CHECKSUM_LEADER_SLOTS;
use crate::leaders_runtime_frontier::{LeadersWalkFrontier, RuntimeCoveredRange};
use crate::leaders_setup_init_scalar_frontier::{
    derive_frame_zero_init_scalars, FrameZeroInitScalarError,
    RuntimeLeadersFrameZeroInitScalarFrontier,
};
use crate::setup_cities_builds::StartingSetupState;

pub const LEADER_INIT_PLAN_REGIONS_BEGIN_VA: u32 = 0x006e_4ae7;
pub const LEADER_INIT_PLAN_REGIONS_END_VA: u32 = 0x006e_4ba2;
pub const LEADER_INIT_PLAN_SCALARS_BEGIN_VA: u32 = 0x006e_49c8;
pub const LEADER_INIT_PLAN_SCALARS_END_VA: u32 = 0x006e_4aaf;
pub const LEADERS_INIT_NEGATIVE_BULK_ZERO_BEGIN_VA: u32 = 0x006e_398c;
pub const LEADERS_INIT_NEGATIVE_BULK_ZERO_END_VA: u32 = 0x006e_3996;
pub const LEADERS_INIT_NEGATIVE_CALL_VA: u32 = 0x006e_d8cb;
pub const PLAN_STRATEGY_FILLED_GATHER_CLEAR_BEGIN_VA: u32 = 0x006b_997d;
pub const PLAN_STRATEGY_FILLED_GATHER_CLEAR_END_VA: u32 = 0x006b_99b0;
pub const PLAN_STRATEGY_ENTRY_SCALARS_BEGIN_VA: u32 = 0x006b_97cc;
pub const PLAN_STRATEGY_ENTRY_SCALARS_END_VA: u32 = 0x006b_99b0;
pub const PLAN_STRATEGY_REGION_RECOMPUTE_BEGIN_VA: u32 = 0x006b_9bb0;
pub const PLAN_STRATEGY_REGION_RECOMPUTE_END_VA: u32 = 0x006b_9c63;
pub const PLAN_STRATEGY_STRATEGY_FIRST_STORE_VA: u32 = 0x006b_bba1;
pub const PLAN_STRATEGY_STRATEGY_LAST_STORE_VA: u32 = 0x006b_be70;

pub const FRAME_ZERO_PLAN_SCRATCH_WALKED_BYTES: usize = 2_558;

const PLAN_ZERO_RANGES: &[RuntimeCoveredRange] = &[
    RuntimeCoveredRange {
        begin: 0x4d4,
        end: 0x6d4,
    },
    RuntimeCoveredRange {
        begin: 0x8bc,
        end: 0x8d4,
    },
    RuntimeCoveredRange {
        begin: 0xa68,
        end: 0xe62,
    },
    RuntimeCoveredRange {
        begin: 0xee2,
        end: 0x125e,
    },
];

// Entry scratch dwords which are constructor-zero and cleared again at plan entry. Fields with
// a different owner or a non-zero/not-cleared entry value are deliberately absent.
const PLAN_ZERO_DWORDS: &[usize] = &[
    0x93c, 0x944, 0x948, 0x94c, 0x950, 0x954, 0x958, 0x960, 0x964, 0x968, 0x96c, 0x970, 0x974,
    0x978, 0x97c, 0x980, 0x984, 0x98c, 0x990, 0x994, 0x998, 0x9bc, 0x9c0, 0x9c4, 0x9cc, 0x9d0,
    0x9e4, 0x9e8,
];

const _: () = assert!(PLAN_ZERO_DWORDS.len() == 28);
const _: () = assert!(
    (0x6d4 - 0x4d4)
        + (0x8d4 - 0x8bc)
        + PLAN_ZERO_DWORDS.len() * 4
        + (0xe62 - 0xa68)
        + (0x125e - 0xee2)
        == FRAME_ZERO_PLAN_SCRATCH_WALKED_BYTES
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameZeroPlanScratchSource {
    LeaderInitBeforeFirstPlanStrategy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameZeroPlanScratchClaim {
    pub slot: u8,
    pub source: FrameZeroPlanScratchSource,
    pub newly_canonical_walked_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameZeroPlanScratch {
    active: [bool; CHECKSUM_LEADER_SLOTS],
    claims: Vec<FrameZeroPlanScratchClaim>,
}

impl FrameZeroPlanScratch {
    pub fn claims(&self) -> &[FrameZeroPlanScratchClaim] {
        &self.claims
    }

    pub fn newly_canonical_walked_bytes(&self) -> usize {
        self.claims.len() * FRAME_ZERO_PLAN_SCRATCH_WALKED_BYTES
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeLeadersFrameZeroPlanScratchFrontier {
    inner: Box<RuntimeLeadersFrameZeroInitScalarFrontier>,
    scratch: FrameZeroPlanScratch,
}

impl RuntimeLeadersFrameZeroPlanScratchFrontier {
    pub fn inner(&self) -> &RuntimeLeadersFrameZeroInitScalarFrontier {
        self.inner.as_ref()
    }

    pub fn scratch(&self) -> &FrameZeroPlanScratch {
        &self.scratch
    }

    pub fn newly_canonicalized_walked_bytes(&self) -> usize {
        self.scratch.newly_canonical_walked_bytes()
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
pub enum FrameZeroPlanScratchError {
    Setup(FrameZeroInitScalarError),
    ConditionalRosterDisagreement {
        slot: usize,
        scratch_active: bool,
        transcript_active: bool,
    },
    ConditionalDisagreement {
        slot: usize,
        begin: usize,
        byte: usize,
        conditional: u8,
    },
}

impl std::fmt::Display for FrameZeroPlanScratchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "frame-zero Leader plan scratch refused: {self:?}")
    }
}

impl std::error::Error for FrameZeroPlanScratchError {}

impl From<FrameZeroInitScalarError> for FrameZeroPlanScratchError {
    fn from(value: FrameZeroInitScalarError) -> Self {
        Self::Setup(value)
    }
}

pub fn derive_frame_zero_plan_scratch(
    setup: &StartingSetupState,
) -> Result<FrameZeroPlanScratch, FrameZeroPlanScratchError> {
    let scalars = derive_frame_zero_init_scalars(setup)?;
    let mut active = [false; CHECKSUM_LEADER_SLOTS];
    let mut claims = Vec::with_capacity(scalars.claims().len());
    for claim in scalars.claims() {
        let slot = usize::from(claim.slot);
        active[slot] = true;
        claims.push(FrameZeroPlanScratchClaim {
            slot: claim.slot,
            source: FrameZeroPlanScratchSource::LeaderInitBeforeFirstPlanStrategy,
            newly_canonical_walked_bytes: FRAME_ZERO_PLAN_SCRATCH_WALKED_BYTES,
        });
    }
    Ok(FrameZeroPlanScratch { active, claims })
}

pub fn bind_frame_zero_plan_scratch(
    inner: RuntimeLeadersFrameZeroInitScalarFrontier,
    scratch: FrameZeroPlanScratch,
) -> Result<RuntimeLeadersFrameZeroPlanScratchFrontier, FrameZeroPlanScratchError> {
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        let transcript_active = inner
            .conditional_row_active(slot)
            .expect("the complete Leader transcript has ten rows");
        if scratch.active[slot] != transcript_active {
            return Err(FrameZeroPlanScratchError::ConditionalRosterDisagreement {
                slot,
                scratch_active: scratch.active[slot],
                transcript_active,
            });
        }
        if !transcript_active {
            continue;
        }
        let ranges = PLAN_ZERO_RANGES
            .iter()
            .copied()
            .chain(PLAN_ZERO_DWORDS.iter().map(|begin| RuntimeCoveredRange {
                begin: *begin,
                end: *begin + 4,
            }));
        for range in ranges {
            let conditional = inner
                .conditional_fixed_slice(slot, range)
                .expect("the complete conditional frontier owns plan scratch");
            if let Some(byte) = conditional.iter().position(|value| *value != 0) {
                return Err(FrameZeroPlanScratchError::ConditionalDisagreement {
                    slot,
                    begin: range.begin,
                    byte,
                    conditional: conditional[byte],
                });
            }
        }
    }
    Ok(RuntimeLeadersFrameZeroPlanScratchFrontier {
        inner: Box::new(inner),
        scratch,
    })
}
