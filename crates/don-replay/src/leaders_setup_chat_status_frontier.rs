// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact frame-zero Leader `chat_status[8]` from the canonical team-setup transaction.
//!
//! `Game::init_teams` seeds every cell of each active row to one, then clears the admitted
//! target cells according to the exact team/cooperative predicates. The starting setup receipt
//! retains that product-owned state and ordered mutation receipt, so this frontier does not
//! assume an all-zero row. Later diplomacy/team mutation expires the claim.

#![forbid(unsafe_code)]

use crate::leader_initial_prefix::CHECKSUM_LEADER_SLOTS;
use crate::leaders_runtime_frontier::{LeadersWalkFrontier, RuntimeCoveredRange};
use crate::leaders_setup_human_personality_frontier::{
    derive_frame_zero_human_personality, FrameZeroHumanPersonalityError,
    RuntimeLeadersFrameZeroHumanPersonalityFrontier,
};
use crate::setup_cities_builds::{StartingSetupState, UNTEAMED};
use don_sim::systems::team_setup_mutation::{init_teams_atomic, InitTeamsError, InitTeamsRequest};

pub const CHAT_STATUS_BEGIN: usize = 0x54;
pub const CHAT_STATUS_END: usize = 0x74;
pub const FRAME_ZERO_CHAT_STATUS_WALKED_BYTES: usize = CHAT_STATUS_END - CHAT_STATUS_BEGIN;

pub const GAME_INIT_TEAMS_VA: u32 = 0x0058_ae70;
pub const GAME_INIT_TEAMS_CHAT_BEGIN_VA: u32 = 0x0058_c020;
pub const GAME_INIT_TEAMS_CHAT_END_VA: u32 = 0x0058_c184;

const _: () = assert!(FRAME_ZERO_CHAT_STATUS_WALKED_BYTES == 32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameZeroChatStatusSource {
    CanonicalInitTeamsReceipt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameZeroChatStatusClaim {
    pub slot: u8,
    pub newly_canonical_walked_bytes: usize,
    pub source: FrameZeroChatStatusSource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameZeroChatStatus {
    active: [bool; CHECKSUM_LEADER_SLOTS],
    rows: [[i32; CHECKSUM_LEADER_SLOTS]; CHECKSUM_LEADER_SLOTS],
    claims: Vec<FrameZeroChatStatusClaim>,
}

impl FrameZeroChatStatus {
    pub fn claims(&self) -> &[FrameZeroChatStatusClaim] {
        &self.claims
    }

    pub fn row(&self, slot: usize) -> Option<&[i32; CHECKSUM_LEADER_SLOTS]> {
        self.active
            .get(slot)
            .copied()
            .unwrap_or(false)
            .then_some(&self.rows[slot])
    }

    pub fn newly_canonical_walked_bytes(&self) -> usize {
        self.claims.len() * FRAME_ZERO_CHAT_STATUS_WALKED_BYTES
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeLeadersFrameZeroChatStatusFrontier {
    inner: Box<RuntimeLeadersFrameZeroHumanPersonalityFrontier>,
    chat_status: FrameZeroChatStatus,
}

impl RuntimeLeadersFrameZeroChatStatusFrontier {
    pub fn inner(&self) -> &RuntimeLeadersFrameZeroHumanPersonalityFrontier {
        self.inner.as_ref()
    }

    pub fn chat_status(&self) -> &FrameZeroChatStatus {
        &self.chat_status
    }

    pub fn newly_canonicalized_walked_bytes(&self) -> usize {
        self.chat_status.newly_canonical_walked_bytes()
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

    /// Expose the complete conditional fixed-body image only as an agreement gate for later
    /// setup owners. Presence in that transcript never promotes a byte by itself.
    pub fn conditional_fixed_slice(
        &self,
        slot: usize,
        range: RuntimeCoveredRange,
    ) -> Option<&[u8]> {
        let init = self
            .inner()
            .inner()
            .inner()
            .inner()
            .inner()
            .inner()
            .inner()
            .inner();
        init.conditional_fixed_slice(slot, range)
    }

    pub fn conditional_row_active(&self, slot: usize) -> Option<bool> {
        let init = self
            .inner()
            .inner()
            .inner()
            .inner()
            .inner()
            .inner()
            .inner()
            .inner();
        init.conditional_row_active(slot)
    }

    pub fn checksum(&self) -> Result<(u32, u64), LeadersWalkFrontier> {
        Err(self.walk_frontier())
    }

    pub const fn installed_in_scoreboard(&self) -> bool {
        false
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameZeroChatStatusError {
    Setup(FrameZeroHumanPersonalityError),
    MissingStartingPlayer {
        slot: usize,
    },
    TeamSetupIdentityDisagreement {
        slot: usize,
    },
    TeamSetupSourceDisagreement,
    TeamSetup(InitTeamsError),
    ConditionalRosterDisagreement {
        slot: usize,
        chat_active: bool,
        transcript_active: bool,
    },
    ConditionalDisagreement {
        slot: usize,
        byte: usize,
        expected: u8,
        conditional: u8,
    },
}

impl std::fmt::Display for FrameZeroChatStatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "frame-zero Leader chat status refused: {self:?}")
    }
}

impl std::error::Error for FrameZeroChatStatusError {}

impl From<FrameZeroHumanPersonalityError> for FrameZeroChatStatusError {
    fn from(value: FrameZeroHumanPersonalityError) -> Self {
        Self::Setup(value)
    }
}

impl From<InitTeamsError> for FrameZeroChatStatusError {
    fn from(value: InitTeamsError) -> Self {
        Self::TeamSetup(value)
    }
}

pub fn derive_frame_zero_chat_status(
    setup: &StartingSetupState,
) -> Result<FrameZeroChatStatus, FrameZeroChatStatusError> {
    let personality = derive_frame_zero_human_personality(setup)?;
    let mut regenerated = setup.receipt.team_setup_before.clone();
    let local_player_setup_slot = usize::from(
        setup
            .receipt
            .cities
            .first()
            .ok_or(FrameZeroChatStatusError::MissingStartingPlayer { slot: 0 })?
            .replay_player_slot,
    );
    let regenerated_receipt = init_teams_atomic(
        &mut regenerated,
        InitTeamsRequest {
            local_player_setup_slot,
            ranked: false,
        },
    )?;
    if regenerated != setup.receipt.team_setup
        || regenerated_receipt != setup.receipt.team_setup_receipt
    {
        return Err(FrameZeroChatStatusError::TeamSetupSourceDisagreement);
    }

    let mut active = [false; CHECKSUM_LEADER_SLOTS];
    let mut claims = Vec::with_capacity(personality.claims().len());
    for claim in personality.claims() {
        let slot = usize::from(claim.slot);
        let city = setup
            .receipt
            .cities
            .iter()
            .find(|city| usize::from(city.owner) == slot)
            .ok_or(FrameZeroChatStatusError::MissingStartingPlayer { slot })?;
        let player = setup.receipt.team_setup.setup.players[usize::from(city.replay_player_slot)];
        let leader = setup.receipt.team_setup.setup.leaders[slot];
        if !player.is_present()
            || usize::from(player.who) != slot
            || player.team != UNTEAMED as i8
            || !leader.is_present()
            || leader.who != slot as i32
        {
            return Err(FrameZeroChatStatusError::TeamSetupIdentityDisagreement { slot });
        }
        active[slot] = true;
        claims.push(FrameZeroChatStatusClaim {
            slot: claim.slot,
            newly_canonical_walked_bytes: FRAME_ZERO_CHAT_STATUS_WALKED_BYTES,
            source: FrameZeroChatStatusSource::CanonicalInitTeamsReceipt,
        });
    }
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        if setup.receipt.team_setup.setup.leaders[slot].is_present() != active[slot] {
            return Err(FrameZeroChatStatusError::TeamSetupIdentityDisagreement { slot });
        }
    }
    Ok(FrameZeroChatStatus {
        active,
        rows: setup.receipt.team_setup.chat_status,
        claims,
    })
}

pub fn bind_frame_zero_chat_status(
    inner: RuntimeLeadersFrameZeroHumanPersonalityFrontier,
    chat_status: FrameZeroChatStatus,
) -> Result<RuntimeLeadersFrameZeroChatStatusFrontier, FrameZeroChatStatusError> {
    let init = inner
        .inner()
        .inner()
        .inner()
        .inner()
        .inner()
        .inner()
        .inner();
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        let transcript_active = init
            .conditional_row_active(slot)
            .expect("the complete Leader transcript has ten rows");
        if chat_status.active[slot] != transcript_active {
            return Err(FrameZeroChatStatusError::ConditionalRosterDisagreement {
                slot,
                chat_active: chat_status.active[slot],
                transcript_active,
            });
        }
        if !transcript_active {
            continue;
        }
        let conditional = init
            .conditional_fixed_slice(
                slot,
                RuntimeCoveredRange {
                    begin: CHAT_STATUS_BEGIN,
                    end: CHAT_STATUS_END,
                },
            )
            .expect("the complete conditional frontier owns chat_status");
        let expected: Vec<u8> = chat_status.rows[slot]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        if let Some(byte) = conditional
            .iter()
            .zip(&expected)
            .position(|(conditional, expected)| conditional != expected)
        {
            return Err(FrameZeroChatStatusError::ConditionalDisagreement {
                slot,
                byte,
                expected: expected[byte],
                conditional: conditional[byte],
            });
        }
    }
    Ok(RuntimeLeadersFrameZeroChatStatusFrontier {
        inner: Box::new(inner),
        chat_status,
    })
}
