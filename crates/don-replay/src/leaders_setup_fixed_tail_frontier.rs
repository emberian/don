// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact ordinary-setup ownership for the final 51 walked fixed-row bytes.
//!
//! `Leader::init` clears both bonus-card arrays, the walked alignment bytes, and
//! `defeat_stamp`, then writes `team_color = who`. Ordinary non-scenario setup reaches no
//! conquest-card/rate or defeat writer. The existing runtime conquest-byte owner at `+0x6900`
//! is duplicate-checked, leaving 50 newly canonical bytes per active row.

#![forbid(unsafe_code)]

use crate::leader_initial_prefix::CHECKSUM_LEADER_SLOTS;
use crate::leaders_runtime_frontier::{LeadersWalkFrontier, RuntimeCoveredRange};
use crate::leaders_setup_chat_status_frontier::{
    derive_frame_zero_chat_status, FrameZeroChatStatusError,
    RuntimeLeadersFrameZeroChatStatusFrontier,
};
use crate::setup_cities_builds::StartingSetupState;

pub const SETUP_FIXED_TAIL_BEGIN: usize = 0x68f6;
pub const SETUP_FIXED_TAIL_END: usize = 0x6929;
pub const CONQUEST_BYTE_OFFSET: usize = 0x6900;
pub const TEAM_COLOR_OFFSET: usize = 0x6928;
pub const FRAME_ZERO_FIXED_TAIL_WALKED_BYTES: usize = SETUP_FIXED_TAIL_END - SETUP_FIXED_TAIL_BEGIN;
pub const FRAME_ZERO_FIXED_TAIL_DUPLICATE_WALKED_BYTES: usize = 1;
pub const FRAME_ZERO_FIXED_TAIL_NEWLY_CANONICAL_WALKED_BYTES: usize =
    FRAME_ZERO_FIXED_TAIL_WALKED_BYTES - FRAME_ZERO_FIXED_TAIL_DUPLICATE_WALKED_BYTES;

pub const LEADER_INIT_VA: u32 = 0x006e_3930;
pub const LEADER_INIT_BONUS_CARDS_CLEAR_BEGIN_VA: u32 = 0x006e_3a80;
pub const LEADER_INIT_BONUS_CARDS_CLEAR_END_VA: u32 = 0x006e_3a9f;
pub const LEADER_INIT_CTW_RATES_CLEAR_END_VA: u32 = 0x006e_3aa6;
pub const LEADER_INIT_TEAM_COLOR_STORE_VA: u32 = 0x006e_3b02;
pub const LEADER_INIT_DEFEAT_STAMP_STORE_VA: u32 = 0x006e_3e0f;
pub const SETUP_BUILD_GAME_ACTIVE_LEADER_INIT_VA: u32 = 0x005a_c190;

const _: () = assert!(FRAME_ZERO_FIXED_TAIL_WALKED_BYTES == 51);
const _: () = assert!(FRAME_ZERO_FIXED_TAIL_NEWLY_CANONICAL_WALKED_BYTES == 50);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameZeroFixedTailSource {
    LeaderInitOrdinaryNonScenario,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameZeroFixedTailClaim {
    pub slot: u8,
    pub walked_bytes: usize,
    pub duplicate_checked_walked_bytes: usize,
    pub newly_canonical_walked_bytes: usize,
    pub source: FrameZeroFixedTailSource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameZeroFixedTail {
    active: [bool; CHECKSUM_LEADER_SLOTS],
    claims: Vec<FrameZeroFixedTailClaim>,
}

impl FrameZeroFixedTail {
    pub fn claims(&self) -> &[FrameZeroFixedTailClaim] {
        &self.claims
    }

    pub fn newly_canonical_walked_bytes(&self) -> usize {
        self.claims.len() * FRAME_ZERO_FIXED_TAIL_NEWLY_CANONICAL_WALKED_BYTES
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeLeadersFrameZeroFixedTailFrontier {
    inner: Box<RuntimeLeadersFrameZeroChatStatusFrontier>,
    tail: FrameZeroFixedTail,
}

impl RuntimeLeadersFrameZeroFixedTailFrontier {
    pub fn inner(&self) -> &RuntimeLeadersFrameZeroChatStatusFrontier {
        self.inner.as_ref()
    }

    pub fn tail(&self) -> &FrameZeroFixedTail {
        &self.tail
    }

    pub fn newly_canonicalized_walked_bytes(&self) -> usize {
        self.tail.newly_canonical_walked_bytes()
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
pub enum FrameZeroFixedTailError {
    Setup(FrameZeroChatStatusError),
    ConditionalRosterDisagreement {
        slot: usize,
        tail_active: bool,
        transcript_active: bool,
    },
    ConditionalDisagreement {
        slot: usize,
        byte: usize,
        expected: u8,
        conditional: u8,
    },
}

impl std::fmt::Display for FrameZeroFixedTailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "frame-zero Leader fixed tail refused: {self:?}")
    }
}

impl std::error::Error for FrameZeroFixedTailError {}

impl From<FrameZeroChatStatusError> for FrameZeroFixedTailError {
    fn from(value: FrameZeroChatStatusError) -> Self {
        Self::Setup(value)
    }
}

pub fn derive_frame_zero_fixed_tail(
    setup: &StartingSetupState,
) -> Result<FrameZeroFixedTail, FrameZeroFixedTailError> {
    // StartingSetupState admits only ordinary scenario_type=0, human setup rows. Re-run the
    // canonical team/chat source first so the active roster and its retained replay identity
    // cannot be substituted by this narrower constructor owner.
    let chat = derive_frame_zero_chat_status(setup)?;
    let mut active = [false; CHECKSUM_LEADER_SLOTS];
    let mut claims = Vec::with_capacity(chat.claims().len());
    for claim in chat.claims() {
        active[usize::from(claim.slot)] = true;
        claims.push(FrameZeroFixedTailClaim {
            slot: claim.slot,
            walked_bytes: FRAME_ZERO_FIXED_TAIL_WALKED_BYTES,
            duplicate_checked_walked_bytes: FRAME_ZERO_FIXED_TAIL_DUPLICATE_WALKED_BYTES,
            newly_canonical_walked_bytes: FRAME_ZERO_FIXED_TAIL_NEWLY_CANONICAL_WALKED_BYTES,
            source: FrameZeroFixedTailSource::LeaderInitOrdinaryNonScenario,
        });
    }
    Ok(FrameZeroFixedTail { active, claims })
}

pub fn bind_frame_zero_fixed_tail(
    inner: RuntimeLeadersFrameZeroChatStatusFrontier,
    tail: FrameZeroFixedTail,
) -> Result<RuntimeLeadersFrameZeroFixedTailFrontier, FrameZeroFixedTailError> {
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        let transcript_active = inner
            .conditional_row_active(slot)
            .expect("the complete Leader transcript has ten rows");
        if tail.active[slot] != transcript_active {
            return Err(FrameZeroFixedTailError::ConditionalRosterDisagreement {
                slot,
                tail_active: tail.active[slot],
                transcript_active,
            });
        }
        if !transcript_active {
            continue;
        }
        let conditional = inner
            .conditional_fixed_slice(
                slot,
                RuntimeCoveredRange {
                    begin: SETUP_FIXED_TAIL_BEGIN,
                    end: SETUP_FIXED_TAIL_END,
                },
            )
            .expect("the complete conditional frontier owns the fixed tail");
        let mut expected = [0u8; FRAME_ZERO_FIXED_TAIL_WALKED_BYTES];
        expected[TEAM_COLOR_OFFSET - SETUP_FIXED_TAIL_BEGIN] = slot as u8;
        if let Some(byte) = conditional
            .iter()
            .zip(expected)
            .position(|(conditional, expected)| *conditional != expected)
        {
            return Err(FrameZeroFixedTailError::ConditionalDisagreement {
                slot,
                byte,
                expected: expected[byte],
                conditional: conditional[byte],
            });
        }
    }
    Ok(RuntimeLeadersFrameZeroFixedTailFrontier {
        inner: Box::new(inner),
        tail,
    })
}
