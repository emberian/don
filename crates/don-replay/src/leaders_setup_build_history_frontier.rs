// SPDX-License-Identifier: GPL-3.0-or-later
//! Frame-zero producer for `LeaderData::last_building_finished[129]`.
//!
//! `Leader::init` fills the entire Building history with `-1`. The ordinary setup path then
//! calls `Build::activate(0, 0, 0)` for each starting Village. Retail's only activation-time
//! store to this history is guarded by the third argument, so the setup call bypasses it. This
//! module binds that exact surviving initializer image after the complete starting-Build census.
//!
//! Later production can change the array and is not yet a canonical `Sim` owner. The receipt is
//! therefore restricted to the same frame-zero boundary as the regional-Building census and
//! cannot install the Leaders channel.

#![forbid(unsafe_code)]

use crate::leader_initial_prefix::CHECKSUM_LEADER_SLOTS;
use crate::leaders_deferred_history_frontier::{
    LAST_BUILDING_FINISHED_BEGIN, LAST_BUILDING_FINISHED_END, LAST_BUILDING_FINISHED_VALUES,
};
use crate::leaders_runtime_frontier::{LeadersWalkFrontier, RuntimeCoveredRange};
use crate::leaders_setup_reg_buildings_frontier::{
    derive_frame_zero_regional_building_census, FrameZeroRegBuildingsError,
    RuntimeLeadersFrameZeroRegBuildingsFrontier,
};
use crate::setup_cities_builds::StartingSetupState;

pub const LEADER_INIT_LAST_BUILDING_HISTORY_VA: u32 = 0x006e_4bef;
pub const LEADER_INIT_LAST_BUILDING_HISTORY_FILL_VA: u32 = 0x006e_4bfa;
pub const SETUP_BUILD_ACTIVATE_VIRTUAL_CALL_VA: u32 = 0x005a_babf;
pub const BUILD_ACTIVATE_VA: u32 = 0x0062_3e20;
pub const BUILD_ACTIVATE_LAST_BUILDING_GUARD_VA: u32 = 0x0062_3f2d;
pub const BUILD_ACTIVATE_LAST_BUILDING_STORE_VA: u32 = 0x0062_3f47;
pub const FRAME_ZERO_LAST_BUILDING_HISTORY_WALKED_BYTES: usize =
    LAST_BUILDING_FINISHED_VALUES * std::mem::size_of::<i32>();

const _: () = assert!(LAST_BUILDING_FINISHED_VALUES == 129);
const _: () = assert!(
    LAST_BUILDING_FINISHED_END - LAST_BUILDING_FINISHED_BEGIN
        == FRAME_ZERO_LAST_BUILDING_HISTORY_WALKED_BYTES
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameZeroLastBuildingHistorySource {
    LeaderInitMinusOneSurvivesSetupActivation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameZeroLastBuildingHistoryClaim {
    pub slot: u8,
    pub entries: usize,
    pub source: FrameZeroLastBuildingHistorySource,
    pub newly_canonical_walked_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameZeroLastBuildingHistory {
    rows: [[i32; LAST_BUILDING_FINISHED_VALUES]; CHECKSUM_LEADER_SLOTS],
    active: [bool; CHECKSUM_LEADER_SLOTS],
    claims: Vec<FrameZeroLastBuildingHistoryClaim>,
}

impl FrameZeroLastBuildingHistory {
    pub fn row(&self, slot: usize) -> Option<&[i32; LAST_BUILDING_FINISHED_VALUES]> {
        self.rows.get(slot)
    }

    pub fn claims(&self) -> &[FrameZeroLastBuildingHistoryClaim] {
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
pub struct RuntimeLeadersFrameZeroBuildHistoryFrontier {
    inner: Box<RuntimeLeadersFrameZeroRegBuildingsFrontier>,
    history: FrameZeroLastBuildingHistory,
}

impl RuntimeLeadersFrameZeroBuildHistoryFrontier {
    pub fn inner(&self) -> &RuntimeLeadersFrameZeroRegBuildingsFrontier {
        self.inner.as_ref()
    }

    pub fn history(&self) -> &FrameZeroLastBuildingHistory {
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
pub enum FrameZeroBuildHistoryError {
    RegionalCensus(FrameZeroRegBuildingsError),
    SetupActivationChronologyDisagreement {
        receipt: usize,
    },
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

impl std::fmt::Display for FrameZeroBuildHistoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "frame-zero last-Building history refused: {self:?}")
    }
}

impl std::error::Error for FrameZeroBuildHistoryError {}

impl From<FrameZeroRegBuildingsError> for FrameZeroBuildHistoryError {
    fn from(value: FrameZeroRegBuildingsError) -> Self {
        Self::RegionalCensus(value)
    }
}

/// Derive the `-1` history which survives the ordinary setup activation calls.
pub fn derive_frame_zero_last_building_history(
    setup: &StartingSetupState,
) -> Result<FrameZeroLastBuildingHistory, FrameZeroBuildHistoryError> {
    let buildings = derive_frame_zero_regional_building_census(setup)?;
    for (receipt, city) in setup.receipt.cities.iter().enumerate() {
        if city.constructor.build_init_complete
            || city.constructor.build_activation_complete
            || city.constructor.builds_channel_ready
            || !city.constructor.city_init_complete
        {
            return Err(
                FrameZeroBuildHistoryError::SetupActivationChronologyDisagreement { receipt },
            );
        }
    }

    let rows = [[-1; LAST_BUILDING_FINISHED_VALUES]; CHECKSUM_LEADER_SLOTS];
    let mut active = [false; CHECKSUM_LEADER_SLOTS];
    let mut claims = Vec::with_capacity(buildings.claims().len());
    for building in buildings.claims() {
        let slot = usize::from(building.slot);
        active[slot] = true;
        claims.push(FrameZeroLastBuildingHistoryClaim {
            slot: building.slot,
            entries: LAST_BUILDING_FINISHED_VALUES,
            source: FrameZeroLastBuildingHistorySource::LeaderInitMinusOneSurvivesSetupActivation,
            newly_canonical_walked_bytes: FRAME_ZERO_LAST_BUILDING_HISTORY_WALKED_BYTES,
        });
    }

    Ok(FrameZeroLastBuildingHistory {
        rows,
        active,
        claims,
    })
}

/// Bind the source-derived setup history to the independent complete transcript.
pub fn bind_frame_zero_last_building_history(
    inner: RuntimeLeadersFrameZeroRegBuildingsFrontier,
    history: FrameZeroLastBuildingHistory,
) -> Result<RuntimeLeadersFrameZeroBuildHistoryFrontier, FrameZeroBuildHistoryError> {
    let deferred = inner.inner().inner().inner().previous();
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        let transcript_active = deferred.rows()[slot].active;
        if history.active[slot] != transcript_active {
            return Err(FrameZeroBuildHistoryError::ConditionalRosterDisagreement {
                slot,
                history_active: history.active[slot],
                transcript_active,
            });
        }
        if !transcript_active {
            continue;
        }

        let mut history_bytes = Vec::with_capacity(FRAME_ZERO_LAST_BUILDING_HISTORY_WALKED_BYTES);
        for value in history.rows[slot] {
            history_bytes.extend_from_slice(&value.to_le_bytes());
        }
        let conditional = deferred.rows()[slot]
            .conditionally_admitted_slice(RuntimeCoveredRange {
                begin: LAST_BUILDING_FINISHED_BEGIN,
                end: LAST_BUILDING_FINISHED_END,
            })
            .expect("the complete conditional frontier owns last_building_finished");
        if let Some(byte) = conditional
            .iter()
            .zip(&history_bytes)
            .position(|(a, b)| a != b)
        {
            return Err(FrameZeroBuildHistoryError::ConditionalDisagreement {
                slot,
                byte,
                history: history_bytes[byte],
                conditional: conditional[byte],
            });
        }
    }

    Ok(RuntimeLeadersFrameZeroBuildHistoryFrontier {
        inner: Box::new(inner),
        history,
    })
}
