// SPDX-License-Identifier: GPL-3.0-or-later
//! Human-only setup ownership for the raw 96-byte Leader `Personality` child.
//!
//! `Personality::init` clears all 24 dwords. Active human leaders take the `flags & 0x0c == 4`
//! gate around the AI personality-selection body in `Leader::init`, so the exact zero image
//! survives ordinary all-human setup. The already-owned `raid` dword is duplicate-checked and
//! is not counted again; this frontier promotes the remaining 92 bytes.

#![forbid(unsafe_code)]

use crate::leader_initial_prefix::CHECKSUM_LEADER_SLOTS;
use crate::leaders_runtime_frontier::{
    LeadersWalkFrontier, LEADER_PERSONALITY_BEGIN, LEADER_PERSONALITY_END,
};
use crate::leaders_setup_city_stat_frontier::{
    derive_frame_zero_city_stats, FrameZeroCityStatsError, RuntimeLeadersFrameZeroCityStatsFrontier,
};
use crate::setup_cities_builds::StartingSetupState;

pub const PERSONALITY_INIT_VA: u32 = 0x006d_8640;
pub const PERSONALITY_CLEAR_VA: u32 = 0x006d_8650;
pub const LEADER_INIT_HUMAN_PERSONALITY_GATE_BEGIN_VA: u32 = 0x006e_4cb9;
pub const LEADER_INIT_HUMAN_PERSONALITY_GATE_END_VA: u32 = 0x006e_4cc5;
pub const LEADER_INIT_AI_PERSONALITY_BODY_END_VA: u32 = 0x006e_4df1;

pub const HUMAN_PERSONALITY_WALKED_BYTES: usize = LEADER_PERSONALITY_END - LEADER_PERSONALITY_BEGIN;
pub const HUMAN_PERSONALITY_DUPLICATE_WALKED_BYTES: usize = 4;
pub const HUMAN_PERSONALITY_NEWLY_CANONICAL_WALKED_BYTES: usize =
    HUMAN_PERSONALITY_WALKED_BYTES - HUMAN_PERSONALITY_DUPLICATE_WALKED_BYTES;

const _: () = assert!(HUMAN_PERSONALITY_WALKED_BYTES == 96);
const _: () = assert!(HUMAN_PERSONALITY_NEWLY_CANONICAL_WALKED_BYTES == 92);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameZeroHumanPersonalitySource {
    PersonalityInitThenHumanLeaderGate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameZeroHumanPersonalityClaim {
    pub slot: u8,
    pub walked_bytes: usize,
    pub duplicate_checked_walked_bytes: usize,
    pub newly_canonical_walked_bytes: usize,
    pub source: FrameZeroHumanPersonalitySource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameZeroHumanPersonality {
    active: [bool; CHECKSUM_LEADER_SLOTS],
    claims: Vec<FrameZeroHumanPersonalityClaim>,
}

impl FrameZeroHumanPersonality {
    pub fn claims(&self) -> &[FrameZeroHumanPersonalityClaim] {
        &self.claims
    }

    pub fn newly_canonical_walked_bytes(&self) -> usize {
        self.claims.len() * HUMAN_PERSONALITY_NEWLY_CANONICAL_WALKED_BYTES
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeLeadersFrameZeroHumanPersonalityFrontier {
    inner: Box<RuntimeLeadersFrameZeroCityStatsFrontier>,
    personality: FrameZeroHumanPersonality,
}

impl RuntimeLeadersFrameZeroHumanPersonalityFrontier {
    pub fn inner(&self) -> &RuntimeLeadersFrameZeroCityStatsFrontier {
        self.inner.as_ref()
    }

    pub fn personality(&self) -> &FrameZeroHumanPersonality {
        &self.personality
    }

    pub fn newly_canonicalized_walked_bytes(&self) -> usize {
        self.personality.newly_canonical_walked_bytes()
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
pub enum FrameZeroHumanPersonalityError {
    Setup(FrameZeroCityStatsError),
    ConditionalRosterDisagreement {
        slot: usize,
        personality_active: bool,
        transcript_active: bool,
    },
    MissingConditionalPersonality {
        slot: usize,
    },
    ConditionalDisagreement {
        slot: usize,
        byte: usize,
        conditional: u8,
    },
}

impl std::fmt::Display for FrameZeroHumanPersonalityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "frame-zero human Leader Personality refused: {self:?}")
    }
}

impl std::error::Error for FrameZeroHumanPersonalityError {}

impl From<FrameZeroCityStatsError> for FrameZeroHumanPersonalityError {
    fn from(value: FrameZeroCityStatsError) -> Self {
        Self::Setup(value)
    }
}

pub fn derive_frame_zero_human_personality(
    setup: &StartingSetupState,
) -> Result<FrameZeroHumanPersonality, FrameZeroHumanPersonalityError> {
    // StartingSetupState is constructible only through the all-human ordinary setup gate.
    let city_stats = derive_frame_zero_city_stats(setup)?;
    let mut active = [false; CHECKSUM_LEADER_SLOTS];
    let mut claims = Vec::with_capacity(city_stats.claims().len());
    for claim in city_stats.claims() {
        let slot = usize::from(claim.slot);
        active[slot] = true;
        claims.push(FrameZeroHumanPersonalityClaim {
            slot: claim.slot,
            walked_bytes: HUMAN_PERSONALITY_WALKED_BYTES,
            duplicate_checked_walked_bytes: HUMAN_PERSONALITY_DUPLICATE_WALKED_BYTES,
            newly_canonical_walked_bytes: HUMAN_PERSONALITY_NEWLY_CANONICAL_WALKED_BYTES,
            source: FrameZeroHumanPersonalitySource::PersonalityInitThenHumanLeaderGate,
        });
    }
    Ok(FrameZeroHumanPersonality { active, claims })
}

pub fn bind_frame_zero_human_personality(
    inner: RuntimeLeadersFrameZeroCityStatsFrontier,
    personality: FrameZeroHumanPersonality,
) -> Result<RuntimeLeadersFrameZeroHumanPersonalityFrontier, FrameZeroHumanPersonalityError> {
    let init = inner.inner().inner().inner().inner().inner().inner();
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        let transcript_active = init
            .conditional_row_active(slot)
            .expect("the complete Leader transcript has ten rows");
        if personality.active[slot] != transcript_active {
            return Err(
                FrameZeroHumanPersonalityError::ConditionalRosterDisagreement {
                    slot,
                    personality_active: personality.active[slot],
                    transcript_active,
                },
            );
        }
        if !transcript_active {
            continue;
        }
        let conditional = init
            .conditional_dynamic_field(slot, "Personality")
            .filter(|bytes| bytes.len() == HUMAN_PERSONALITY_WALKED_BYTES)
            .ok_or(FrameZeroHumanPersonalityError::MissingConditionalPersonality { slot })?;
        if let Some(byte) = conditional.iter().position(|value| *value != 0) {
            return Err(FrameZeroHumanPersonalityError::ConditionalDisagreement {
                slot,
                byte,
                conditional: conditional[byte],
            });
        }
    }
    Ok(RuntimeLeadersFrameZeroHumanPersonalityFrontier {
        inner: Box::new(inner),
        personality,
    })
}
