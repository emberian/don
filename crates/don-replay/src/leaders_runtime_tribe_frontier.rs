// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact live-owner extension for `LeaderData::tribe` at fixed-body offset `+0x0c`.
//!
//! [`crate::leaders_runtime_frontier`] stops at the first active row's tribe because the
//! replay setup value is not, by itself, a current-state owner.  The canonical BHS type
//! owner already retains the live `LeaderData` flags and tribe used by builtins 815..=819.
//! This module admits the four-byte dword only when that live owner agrees with the
//! replay-bound setup row and with the runtime frontier's current validity gate. A setup
//! selector of 24 means "random nation": it establishes selection lineage, while the
//! post-resolution value comes exclusively from the current live type owner.
//!
//! The extension still is not a complete Leaders checksum producer.  It merely advances
//! the exact prefix through the already-owned `defeated_by` dword to the next gap,
//! `LeaderData::gov` at `+0x14`, and never installs a harness channel.

#![forbid(unsafe_code)]

use crate::initial::ReplayByteSpan;
use crate::leader_initial_prefix::{InitialLeaderPrefix, CHECKSUM_LEADER_SLOTS};
use crate::leaders_runtime_frontier::{
    LeadersWalkBoundary, LeadersWalkFrontier, RuntimeCoveredRange, RuntimeLeadersFrontier,
    LEADER_FIXED_BODY_BEGIN, LEADER_FIXED_BODY_END,
};
use don_sim::systems::bhs_type_table::{TypeBuiltinState, NUM_TRIBES};

pub const LEADER_TRIBE_OFFSET: usize = 0x0c;
pub const LEADER_TRIBE_BYTES: usize = 4;
pub const LEADER_TRIBE_WRITER_VA: u32 = 0x006e_3aff;
pub const NEXT_FIXED_BODY_GAP: usize = 0x14;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveLeaderTribeProvenance {
    /// `Leader::init` writes the replay-selected tribe; the canonical live BHS type owner
    /// independently supplies the value currently queried by retail type builtins.
    ReplaySetupAndTypeBuiltinState,
    /// The replay selected the one-past-the-table random-nation sentinel (24), so the
    /// concrete current value is owned solely by the canonical live BHS type owner.
    ReplayRandomSelectorResolvedByTypeBuiltinState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveLeaderTribeClaim {
    pub slot: u8,
    pub tribe: i32,
    pub setup_selector: u8,
    pub setup_player_slot: u8,
    pub replay_player_body: ReplayByteSpan,
    pub replay_payload_sha256: [u8; 32],
    pub writer_va: u32,
    pub provenance: LiveLeaderTribeProvenance,
}

/// The current runtime projection plus one exact dword for every active row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLeadersTribeFrontier {
    base: RuntimeLeadersFrontier,
    claims: Vec<LiveLeaderTribeClaim>,
}

impl RuntimeLeadersTribeFrontier {
    pub fn base(&self) -> &RuntimeLeadersFrontier {
        &self.base
    }

    pub fn claims(&self) -> &[LiveLeaderTribeClaim] {
        &self.claims
    }

    pub fn newly_owned_walked_bytes(&self) -> usize {
        self.claims.len() * LEADER_TRIBE_BYTES
    }

    pub fn runtime_claimed_walked_bytes(&self) -> usize {
        self.base.runtime_claimed_walked_bytes() + self.newly_owned_walked_bytes()
    }

    pub fn tribe_for(&self, slot: usize) -> Option<i32> {
        self.claims
            .iter()
            .find(|claim| usize::from(claim.slot) == slot)
            .map(|claim| claim.tribe)
    }

    /// Walk the now-contiguous exact prefix. The newly admitted tribe dword unlocks the
    /// base frontier's already-owned `defeated_by` dword at `+0x10`; the next refusal is
    /// therefore `gov` at `+0x14`.
    pub fn walk_frontier(&self) -> LeadersWalkFrontier {
        let mut checksum = 1u32;
        let mut bytes_walked = 0u64;
        for (slot, row) in self.base.rows.iter().enumerate() {
            let header = row
                .owned_slice(RuntimeCoveredRange {
                    begin: 0,
                    end: LEADER_FIXED_BODY_BEGIN,
                })
                .expect("binding validated the current eight-byte header");
            checksum = don_sim::checksum::adler32(checksum, header);
            bytes_walked += header.len() as u64;
            if !row.active {
                continue;
            }

            let who = row
                .owned_slice(RuntimeCoveredRange {
                    begin: LEADER_FIXED_BODY_BEGIN,
                    end: LEADER_TRIBE_OFFSET,
                })
                .expect("binding validated current who ownership");
            checksum = don_sim::checksum::adler32(checksum, who);
            bytes_walked += who.len() as u64;

            let tribe = self
                .tribe_for(slot)
                .expect("binding issued one tribe claim per active row");
            checksum = don_sim::checksum::adler32(checksum, &tribe.to_le_bytes());
            bytes_walked += LEADER_TRIBE_BYTES as u64;

            let tail = row
                .covered_ranges()
                .into_iter()
                .find(|range| {
                    range.begin <= LEADER_TRIBE_OFFSET + LEADER_TRIBE_BYTES
                        && LEADER_TRIBE_OFFSET + LEADER_TRIBE_BYTES < range.end
                })
                .expect("binding validated the contiguous current tail after tribe");
            let tail_begin = LEADER_TRIBE_OFFSET + LEADER_TRIBE_BYTES;
            let tail_end = tail.end.min(LEADER_FIXED_BODY_END);
            let bytes = row
                .owned_slice(RuntimeCoveredRange {
                    begin: tail_begin,
                    end: tail_end,
                })
                .expect("covered-range bytes remain owned");
            checksum = don_sim::checksum::adler32(checksum, bytes);
            bytes_walked += bytes.len() as u64;

            if tail_end != LEADER_FIXED_BODY_END {
                return LeadersWalkFrontier {
                    checksum,
                    bytes_walked,
                    boundary: LeadersWalkBoundary::FixedBody {
                        slot,
                        offset: tail_end,
                    },
                };
            }
            return LeadersWalkFrontier {
                checksum,
                bytes_walked,
                boundary: LeadersWalkBoundary::DynamicChildren {
                    slot,
                    child: "Diplomacy[8] and length-bearing LeaderData children",
                },
            };
        }
        LeadersWalkFrontier {
            checksum,
            bytes_walked,
            boundary: LeadersWalkBoundary::Complete,
        }
    }

    /// A checksum is still issued only for a complete traversal. Real replay setups have
    /// active rows and therefore continue to return a frontier, never a plausible value.
    pub fn checksum(&self) -> Result<(u32, u64), LeadersWalkFrontier> {
        let frontier = self.walk_frontier();
        if frontier.boundary == LeadersWalkBoundary::Complete {
            Ok((frontier.checksum, frontier.bytes_walked))
        } else {
            Err(frontier)
        }
    }

    /// This adapter has no `SimState` installation hook. A future hook must additionally
    /// require [`Self::checksum`] to return `Ok` before it can admit substantive evidence.
    pub const fn installed_in_scoreboard(&self) -> bool {
        false
    }
}

/// Bind the setup-origin tribe to the current canonical BHS type owner and extend an
/// already-coherent live Leaders frontier.
pub fn bind_live_tribes(
    setup: &InitialLeaderPrefix,
    base: RuntimeLeadersFrontier,
    type_state: &TypeBuiltinState,
) -> Result<RuntimeLeadersTribeFrontier, LeadersTribeFrontierError> {
    if base.setup_claimed_walked_bytes != setup.claimed_walked_bytes()
        || base.setup_human_only_first_checksum_candidate
            != setup.is_human_only_first_checksum_candidate()
    {
        return Err(LeadersTribeFrontierError::SetupFrontierMismatch);
    }

    let mut claims = Vec::with_capacity(setup.active_mask.count_ones() as usize);
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        let setup_row = &setup.rows[slot];
        let runtime_row = &base.rows[slot];
        if runtime_row.slot != slot as u8
            || runtime_row.active != setup_row.active
            || runtime_row.setup_claimed_walked_bytes != setup_row.claimed_walked_bytes()
        {
            return Err(LeadersTribeFrontierError::SetupRuntimeRosterMismatch {
                slot,
                setup_active: setup_row.active,
                runtime_active: runtime_row.active,
            });
        }

        let type_row = &type_state.leaders[slot];
        let type_active = type_row.leader_flags & 1 != 0;
        if type_active != runtime_row.active {
            return Err(LeadersTribeFrontierError::TypeRuntimeRosterMismatch {
                slot,
                type_active,
                runtime_active: runtime_row.active,
            });
        }

        let header = runtime_row.owned_slice(RuntimeCoveredRange {
            begin: 0,
            end: LEADER_FIXED_BODY_BEGIN,
        });
        if header.is_none() {
            return Err(LeadersTribeFrontierError::MissingBaseRange {
                slot,
                begin: 0,
                end: LEADER_FIXED_BODY_BEGIN,
            });
        }
        if !runtime_row.active {
            continue;
        }

        for (begin, end) in [
            (LEADER_FIXED_BODY_BEGIN, LEADER_TRIBE_OFFSET),
            (
                LEADER_TRIBE_OFFSET + LEADER_TRIBE_BYTES,
                NEXT_FIXED_BODY_GAP,
            ),
        ] {
            if runtime_row
                .owned_slice(RuntimeCoveredRange { begin, end })
                .is_none()
            {
                return Err(LeadersTribeFrontierError::MissingBaseRange { slot, begin, end });
            }
        }
        if runtime_row
            .owned_slice(RuntimeCoveredRange {
                begin: LEADER_TRIBE_OFFSET,
                end: LEADER_TRIBE_OFFSET + LEADER_TRIBE_BYTES,
            })
            .is_some()
            || runtime_row.first_unowned_fixed_offset() != Some(LEADER_TRIBE_OFFSET)
        {
            return Err(LeadersTribeFrontierError::BaseGapChanged { slot });
        }

        let setup_tribe = setup_row
            .tribe
            .ok_or(LeadersTribeFrontierError::MissingSetupTribe { slot })?;
        let live_tribe = type_row.tribe;
        if !(0..NUM_TRIBES as i32).contains(&live_tribe) {
            return Err(LeadersTribeFrontierError::LiveTribeOutOfRange {
                slot,
                tribe: live_tribe,
            });
        }
        let provenance = match usize::from(setup_tribe) {
            setup if setup < NUM_TRIBES => {
                if live_tribe != i32::from(setup_tribe) {
                    return Err(LeadersTribeFrontierError::TribeDrift {
                        slot,
                        setup: i32::from(setup_tribe),
                        live: live_tribe,
                    });
                }
                LiveLeaderTribeProvenance::ReplaySetupAndTypeBuiltinState
            }
            setup if setup == NUM_TRIBES => {
                LiveLeaderTribeProvenance::ReplayRandomSelectorResolvedByTypeBuiltinState
            }
            _ => {
                return Err(LeadersTribeFrontierError::SetupTribeSelectorOutOfRange {
                    slot,
                    selector: setup_tribe,
                });
            }
        };

        let setup_player_slot = setup_row
            .setup_player_slot
            .ok_or(LeadersTribeFrontierError::MissingSetupPlayer { slot })?;
        let replay_player_body = setup
            .sources
            .players
            .get(usize::from(setup_player_slot))
            .and_then(|source| source.body)
            .ok_or(LeadersTribeFrontierError::MissingReplayPlayerBody {
                slot,
                player_slot: setup_player_slot,
            })?;
        claims.push(LiveLeaderTribeClaim {
            slot: slot as u8,
            tribe: live_tribe,
            setup_selector: setup_tribe,
            setup_player_slot,
            replay_player_body,
            replay_payload_sha256: setup.sources.payload_sha256,
            writer_va: LEADER_TRIBE_WRITER_VA,
            provenance,
        });
    }

    Ok(RuntimeLeadersTribeFrontier { base, claims })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeadersTribeFrontierError {
    SetupFrontierMismatch,
    SetupRuntimeRosterMismatch {
        slot: usize,
        setup_active: bool,
        runtime_active: bool,
    },
    TypeRuntimeRosterMismatch {
        slot: usize,
        type_active: bool,
        runtime_active: bool,
    },
    MissingBaseRange {
        slot: usize,
        begin: usize,
        end: usize,
    },
    BaseGapChanged {
        slot: usize,
    },
    MissingSetupTribe {
        slot: usize,
    },
    LiveTribeOutOfRange {
        slot: usize,
        tribe: i32,
    },
    SetupTribeSelectorOutOfRange {
        slot: usize,
        selector: u8,
    },
    TribeDrift {
        slot: usize,
        setup: i32,
        live: i32,
    },
    MissingSetupPlayer {
        slot: usize,
    },
    MissingReplayPlayerBody {
        slot: usize,
        player_slot: u8,
    },
}

impl std::fmt::Display for LeadersTribeFrontierError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Leader tribe frontier refused: {self:?}")
    }
}

impl std::error::Error for LeadersTribeFrontierError {}
