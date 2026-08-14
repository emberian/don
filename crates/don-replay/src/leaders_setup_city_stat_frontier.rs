// SPDX-License-Identifier: GPL-3.0-or-later
//! Starting-Village Leader counters retained by the complete City constructor receipt.
//!
//! The fresh City activation increments `city_mine` and the setup caller increments
//! `cities_built`. The adjacent Village counters have no store in this transaction and retain
//! their constructor zeroes. These four exact dwords are setup history, not live maintainers.

#![forbid(unsafe_code)]

use crate::leader_initial_prefix::CHECKSUM_LEADER_SLOTS;
use crate::leaders_runtime_frontier::{LeadersWalkFrontier, RuntimeCoveredRange};
use crate::leaders_setup_diplomacy_stamp_frontier::{
    derive_frame_zero_action_stamps, FrameZeroActionStampsError,
    RuntimeLeadersFrameZeroActionStampsFrontier,
};
use crate::setup_cities_builds::StartingSetupState;

pub const VILLAGE_CITY_MINE_HISTORY_BEGIN: usize = 0x3fc;
pub const VILLAGE_CITY_MINE_HISTORY_END: usize = 0x408;
pub const CITIES_BUILT_HISTORY_BEGIN: usize = 0x820;
pub const CITIES_BUILT_HISTORY_END: usize = 0x824;
pub const FRAME_ZERO_CITY_STATS_WALKED_BYTES: usize = (VILLAGE_CITY_MINE_HISTORY_END
    - VILLAGE_CITY_MINE_HISTORY_BEGIN)
    + (CITIES_BUILT_HISTORY_END - CITIES_BUILT_HISTORY_BEGIN);

pub const SETUP_BUILD_CITIES_VA: u32 = 0x005a_b910;
pub const BUILD_ACTIVATE_VA: u32 = 0x0062_3e20;
pub const CITY_INIT_VA: u32 = 0x0073_7050;

const _: () = assert!(FRAME_ZERO_CITY_STATS_WALKED_BYTES == 16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameZeroCityStatsSource {
    CompleteFreshStartingVillageChronology,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameZeroCityStatsClaim {
    pub slot: u8,
    pub villages: usize,
    pub newly_canonical_walked_bytes: usize,
    pub source: FrameZeroCityStatsSource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameZeroCityStats {
    active: [bool; CHECKSUM_LEADER_SLOTS],
    values: [[i32; 4]; CHECKSUM_LEADER_SLOTS],
    claims: Vec<FrameZeroCityStatsClaim>,
}

impl FrameZeroCityStats {
    pub fn claims(&self) -> &[FrameZeroCityStatsClaim] {
        &self.claims
    }

    pub fn values(&self, slot: usize) -> Option<[i32; 4]> {
        self.active.get(slot).copied()?.then_some(self.values[slot])
    }

    pub fn newly_canonical_walked_bytes(&self) -> usize {
        self.claims.len() * FRAME_ZERO_CITY_STATS_WALKED_BYTES
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeLeadersFrameZeroCityStatsFrontier {
    inner: Box<RuntimeLeadersFrameZeroActionStampsFrontier>,
    stats: FrameZeroCityStats,
}

impl RuntimeLeadersFrameZeroCityStatsFrontier {
    pub fn inner(&self) -> &RuntimeLeadersFrameZeroActionStampsFrontier {
        self.inner.as_ref()
    }

    pub fn stats(&self) -> &FrameZeroCityStats {
        &self.stats
    }

    pub fn newly_canonicalized_walked_bytes(&self) -> usize {
        self.stats.newly_canonical_walked_bytes()
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
pub enum FrameZeroCityStatsError {
    Setup(FrameZeroActionStampsError),
    IncompleteVillageChronology {
        slot: usize,
        receipts: usize,
    },
    UnexpectedVillageEffects {
        slot: usize,
        city_mine_delta: i32,
        cities_built_delta: i32,
    },
    ConditionalRosterDisagreement {
        slot: usize,
        stats_active: bool,
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

impl std::fmt::Display for FrameZeroCityStatsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "frame-zero Leader City stats refused: {self:?}")
    }
}

impl std::error::Error for FrameZeroCityStatsError {}

impl From<FrameZeroActionStampsError> for FrameZeroCityStatsError {
    fn from(value: FrameZeroActionStampsError) -> Self {
        Self::Setup(value)
    }
}

pub fn derive_frame_zero_city_stats(
    setup: &StartingSetupState,
) -> Result<FrameZeroCityStats, FrameZeroCityStatsError> {
    let action_stamps = derive_frame_zero_action_stamps(setup)?;
    let mut active = [false; CHECKSUM_LEADER_SLOTS];
    let mut values = [[0; 4]; CHECKSUM_LEADER_SLOTS];
    let mut claims = Vec::with_capacity(action_stamps.claims().len());

    for claim in action_stamps.claims() {
        active[usize::from(claim.slot)] = true;
    }
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        let receipts: Vec<_> = setup
            .receipt
            .cities
            .iter()
            .filter(|city| usize::from(city.owner) == slot)
            .collect();
        let expected = usize::from(active[slot]);
        if receipts.len() != expected {
            return Err(FrameZeroCityStatsError::IncompleteVillageChronology {
                slot,
                receipts: receipts.len(),
            });
        }
        if !active[slot] {
            continue;
        }
        let receipt = receipts[0];
        let effects = receipt.constructor.effects;
        if !receipt.constructor.city_init_complete
            || effects.leader_city_mine_delta != 1
            || effects.setup_cities_built_delta != 1
        {
            return Err(FrameZeroCityStatsError::UnexpectedVillageEffects {
                slot,
                city_mine_delta: effects.leader_city_mine_delta,
                cities_built_delta: effects.setup_cities_built_delta,
            });
        }
        // village_num, city_mine, village_mine, cities_built.
        values[slot] = [
            0,
            effects.leader_city_mine_delta,
            0,
            effects.setup_cities_built_delta,
        ];
        claims.push(FrameZeroCityStatsClaim {
            slot: slot as u8,
            villages: 1,
            newly_canonical_walked_bytes: FRAME_ZERO_CITY_STATS_WALKED_BYTES,
            source: FrameZeroCityStatsSource::CompleteFreshStartingVillageChronology,
        });
    }
    Ok(FrameZeroCityStats {
        active,
        values,
        claims,
    })
}

pub fn bind_frame_zero_city_stats(
    inner: RuntimeLeadersFrameZeroActionStampsFrontier,
    stats: FrameZeroCityStats,
) -> Result<RuntimeLeadersFrameZeroCityStatsFrontier, FrameZeroCityStatsError> {
    let init = inner.inner().inner().inner().inner().inner();
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        let transcript_active = init
            .conditional_row_active(slot)
            .expect("the complete Leader transcript has ten rows");
        if stats.active[slot] != transcript_active {
            return Err(FrameZeroCityStatsError::ConditionalRosterDisagreement {
                slot,
                stats_active: stats.active[slot],
                transcript_active,
            });
        }
        if !transcript_active {
            continue;
        }
        for (range, expected_values) in [
            (
                RuntimeCoveredRange {
                    begin: VILLAGE_CITY_MINE_HISTORY_BEGIN,
                    end: VILLAGE_CITY_MINE_HISTORY_END,
                },
                &stats.values[slot][..3],
            ),
            (
                RuntimeCoveredRange {
                    begin: CITIES_BUILT_HISTORY_BEGIN,
                    end: CITIES_BUILT_HISTORY_END,
                },
                &stats.values[slot][3..],
            ),
        ] {
            let conditional = init
                .conditional_fixed_slice(slot, range)
                .expect("the complete conditional frontier owns City stats");
            let mut expected = Vec::with_capacity(expected_values.len() * 4);
            for value in expected_values {
                expected.extend_from_slice(&value.to_le_bytes());
            }
            if let Some(byte) = conditional
                .iter()
                .zip(&expected)
                .position(|(conditional, expected)| conditional != expected)
            {
                return Err(FrameZeroCityStatsError::ConditionalDisagreement {
                    slot,
                    begin: range.begin,
                    byte,
                    expected: expected[byte],
                    conditional: conditional[byte],
                });
            }
        }
    }
    Ok(RuntimeLeadersFrameZeroCityStatsFrontier {
        inner: Box::new(inner),
        stats,
    })
}
