// SPDX-License-Identifier: GPL-3.0-or-later
//! Setup-surviving `Leader::init` government and stamp scalar cohort.
//!
//! Active `Leader::init` writes `gov = -1` and clears the contiguous gather/support/nuke/flock,
//! nuke/missile usage, and tech-frame block. The canonical starting-town-one receipt performs
//! no gather, support, weapon use, or technology completion before its constructor boundary.
//! This binder admits those two exact fixed-body ranges only for that setup receipt.

#![forbid(unsafe_code)]

use crate::leader_initial_prefix::CHECKSUM_LEADER_SLOTS;
use crate::leaders_deferred_history_frontier::{
    GOV_BEGIN, GOV_END, SETUP_STAMPS_BEGIN, SETUP_STAMPS_END, SETUP_STAMP_DWORDS,
};
use crate::leaders_runtime_frontier::{LeadersWalkFrontier, RuntimeCoveredRange};
use crate::leaders_setup_region_history_frontier::{
    derive_frame_zero_region_strategy_history, FrameZeroRegionHistoryError,
};
use crate::leaders_type_mask_owner_frontier::RuntimeLeadersTypeMaskOwnerFrontier;
use crate::setup_cities_builds::StartingSetupState;

pub const LEADER_INIT_GOV_STORE_VA: u32 = 0x006e_3b0f;
pub const LEADER_INIT_SETUP_STAMPS_BEGIN_VA: u32 = 0x006e_3ba4;
pub const LEADER_INIT_SETUP_STAMPS_STOS_BEGIN_VA: u32 = 0x006e_3bee;
pub const LEADER_INIT_SETUP_STAMPS_STOS_END_VA: u32 = 0x006e_3bf9;
pub const FRAME_ZERO_INIT_SCALAR_WALKED_BYTES: usize =
    (GOV_END - GOV_BEGIN) + (SETUP_STAMPS_END - SETUP_STAMPS_BEGIN);

const _: () = assert!(SETUP_STAMP_DWORDS == 11);
const _: () = assert!(FRAME_ZERO_INIT_SCALAR_WALKED_BYTES == 48);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameZeroInitScalarSource {
    LeaderInitBeforeSetupActivity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameZeroInitScalarClaim {
    pub slot: u8,
    pub source: FrameZeroInitScalarSource,
    pub newly_canonical_walked_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameZeroInitScalars {
    active: [bool; CHECKSUM_LEADER_SLOTS],
    claims: Vec<FrameZeroInitScalarClaim>,
}

impl FrameZeroInitScalars {
    pub fn claims(&self) -> &[FrameZeroInitScalarClaim] {
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
pub struct RuntimeLeadersFrameZeroInitScalarFrontier {
    inner: Box<RuntimeLeadersTypeMaskOwnerFrontier>,
    scalars: FrameZeroInitScalars,
}

impl RuntimeLeadersFrameZeroInitScalarFrontier {
    pub fn inner(&self) -> &RuntimeLeadersTypeMaskOwnerFrontier {
        self.inner.as_ref()
    }

    pub fn scalars(&self) -> &FrameZeroInitScalars {
        &self.scalars
    }

    pub fn newly_canonicalized_walked_bytes(&self) -> usize {
        self.scalars.newly_canonical_walked_bytes()
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

    /// Read one range from the complete conditional fixed-body transcript beneath this
    /// setup-only authority. Later setup binders use this only as an independent agreement
    /// gate; a conditional byte is never promoted merely because it is present here.
    pub fn conditional_fixed_slice(
        &self,
        slot: usize,
        range: RuntimeCoveredRange,
    ) -> Option<&[u8]> {
        let dynamic = self
            .inner
            .inner()
            .inner()
            .inner()
            .inner()
            .inner()
            .inner()
            .inner();
        dynamic
            .previous()
            .rows()
            .get(slot)?
            .conditionally_admitted_slice(range)
    }

    pub fn conditional_row_active(&self, slot: usize) -> Option<bool> {
        let dynamic = self
            .inner
            .inner()
            .inner()
            .inner()
            .inner()
            .inner()
            .inner()
            .inner();
        Some(dynamic.previous().rows().get(slot)?.active)
    }

    pub fn checksum(&self) -> Result<(u32, u64), LeadersWalkFrontier> {
        Err(self.walk_frontier())
    }

    pub const fn installed_in_scoreboard(&self) -> bool {
        false
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameZeroInitScalarError {
    Setup(FrameZeroRegionHistoryError),
    ConditionalRosterDisagreement {
        slot: usize,
        scalars_active: bool,
        transcript_active: bool,
    },
    ConditionalDisagreement {
        slot: usize,
        field: &'static str,
        byte: usize,
        initialized: u8,
        conditional: u8,
    },
}

impl std::fmt::Display for FrameZeroInitScalarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "frame-zero Leader init scalars refused: {self:?}")
    }
}

impl std::error::Error for FrameZeroInitScalarError {}

impl From<FrameZeroRegionHistoryError> for FrameZeroInitScalarError {
    fn from(value: FrameZeroRegionHistoryError) -> Self {
        Self::Setup(value)
    }
}

pub fn derive_frame_zero_init_scalars(
    setup: &StartingSetupState,
) -> Result<FrameZeroInitScalars, FrameZeroInitScalarError> {
    let history = derive_frame_zero_region_strategy_history(setup)?;
    let mut active = [false; CHECKSUM_LEADER_SLOTS];
    let mut claims = Vec::with_capacity(history.claims().len());
    for claim in history.claims() {
        let slot = usize::from(claim.slot);
        active[slot] = true;
        claims.push(FrameZeroInitScalarClaim {
            slot: claim.slot,
            source: FrameZeroInitScalarSource::LeaderInitBeforeSetupActivity,
            newly_canonical_walked_bytes: FRAME_ZERO_INIT_SCALAR_WALKED_BYTES,
        });
    }
    Ok(FrameZeroInitScalars { active, claims })
}

pub fn bind_frame_zero_init_scalars(
    inner: RuntimeLeadersTypeMaskOwnerFrontier,
    scalars: FrameZeroInitScalars,
) -> Result<RuntimeLeadersFrameZeroInitScalarFrontier, FrameZeroInitScalarError> {
    let dynamic = inner
        .inner()
        .inner()
        .inner()
        .inner()
        .inner()
        .inner()
        .inner();
    let deferred = dynamic.previous();
    let gov = (-1i32).to_le_bytes();
    let stamps = [0u8; SETUP_STAMPS_END - SETUP_STAMPS_BEGIN];
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        let transcript_active = deferred.rows()[slot].active;
        if scalars.active[slot] != transcript_active {
            return Err(FrameZeroInitScalarError::ConditionalRosterDisagreement {
                slot,
                scalars_active: scalars.active[slot],
                transcript_active,
            });
        }
        if !transcript_active {
            continue;
        }
        for (field, range, initialized) in [
            (
                "gov",
                RuntimeCoveredRange {
                    begin: GOV_BEGIN,
                    end: GOV_END,
                },
                gov.as_slice(),
            ),
            (
                "setup_stamps",
                RuntimeCoveredRange {
                    begin: SETUP_STAMPS_BEGIN,
                    end: SETUP_STAMPS_END,
                },
                stamps.as_slice(),
            ),
        ] {
            let conditional = deferred.rows()[slot]
                .conditionally_admitted_slice(range)
                .expect("the complete conditional frontier owns Leader init scalars");
            if let Some(byte) = conditional
                .iter()
                .zip(initialized)
                .position(|(conditional, initialized)| conditional != initialized)
            {
                return Err(FrameZeroInitScalarError::ConditionalDisagreement {
                    slot,
                    field,
                    byte,
                    initialized: initialized[byte],
                    conditional: conditional[byte],
                });
            }
        }
    }
    Ok(RuntimeLeadersFrameZeroInitScalarFrontier {
        inner: Box::new(inner),
        scalars,
    })
}
