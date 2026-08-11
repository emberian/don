// SPDX-License-Identifier: GPL-3.0-or-later
//! First same-frame `Sim` ownership cohort for the complete conditional Leaders walk.
//!
//! [`don_sim::tick::Sim`] retains three independently useful Leader views in one frame:
//! the step-8 Leader owner, the victory/score owner, and the production runtime.  The
//! first two already feed [`crate::leaders_runtime_frontier`].  The production runtime's
//! [`don_sim::systems::tech_cities::TechState`] additionally owns the current 806-bit
//! tech payload and all seven decoded `LeaderDataEncrypt` tech counters.
//!
//! This binder requires all overlapping views and the conditional transcript to agree.
//! It promotes only the six counter dwords which were previously conditional:
//! `epoch[4]`, aggregate `epochs`, and `discovered`.  The current tech payload and `ages`
//! are duplicate-checked against established runtime owners.  No other generated,
//! deferred, Personality, mask-header, container, string, or economy fact is promoted,
//! so checksum issuance and scoreboard installation remain deliberately unavailable.

#![forbid(unsafe_code)]

use crate::leader_initial_prefix::CHECKSUM_LEADER_SLOTS;
use crate::leaders_deferred_history_frontier::RuntimeLeadersDeferredHistoryFrontier;
use crate::leaders_dynamic_children_frontier::{
    bind_dynamic_children_frontier, DynamicChildrenFrontierError, DynamicLeadersAuthority,
    RuntimeLeadersDynamicChildrenFrontier, TECH_MASK_BYTES,
};
use crate::leaders_runtime_frontier::{LeadersWalkFrontier, LEADER_DATA_ENCRYPT_DWORDS};
use don_sim::systems::{leaders, victory_score};
use don_sim::tick::Sim;

pub const ECON_RESOURCE_CAP_6_INDEX: usize = 54;
pub const ECON_EPOCH_BEGIN: usize = 55;
pub const ECON_EPOCH_COUNT: usize = 4;
pub const ECON_AGES_INDEX: usize = 59;
pub const ECON_EPOCHS_INDEX: usize = 60;
pub const ECON_DISCOVERED_INDEX: usize = 61;
pub const SIM_TECH_PAYLOAD_SOURCE_BYTES: usize = TECH_MASK_BYTES;
pub const SIM_TECH_COUNTER_SOURCE_BYTES: usize = 7 * 4;
pub const SIM_TECH_SOURCE_BYTES: usize =
    SIM_TECH_PAYLOAD_SOURCE_BYTES + SIM_TECH_COUNTER_SOURCE_BYTES;
pub const SIM_TECH_EXISTING_DUPLICATE_BYTES: usize = TECH_MASK_BYTES + 4;
pub const SIM_TECH_NEWLY_CANONICAL_BYTES: usize = ECON_EPOCH_COUNT * 4 + 2 * 4;

const _: () = assert!(ECON_DISCOVERED_INDEX + 1 == LEADER_DATA_ENCRYPT_DWORDS);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimTechCounterClaim {
    pub slot: u8,
    /// The current `tech` payload is produced by `Sim::production_runtime`, but it was
    /// already owned by the victory/tech runtime projection.
    pub current_tech_duplicate_bytes: usize,
    /// `ages` is produced by the production runtime and already owned as
    /// `step8.econ.age_alt`.
    pub ages_duplicate_bytes: usize,
    /// `epoch[4]`, `epochs`, and `discovered` were conditional before this join.
    pub newly_canonical_counter_bytes: usize,
}

impl SimTechCounterClaim {
    pub const fn source_produced_bytes(self) -> usize {
        self.current_tech_duplicate_bytes
            + self.ages_duplicate_bytes
            + self.newly_canonical_counter_bytes
    }

    pub const fn duplicate_checked_bytes(self) -> usize {
        self.current_tech_duplicate_bytes + self.ages_duplicate_bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLeadersSimTechFrontier {
    inner: RuntimeLeadersDynamicChildrenFrontier,
    claims: Vec<SimTechCounterClaim>,
}

impl RuntimeLeadersSimTechFrontier {
    pub fn inner(&self) -> &RuntimeLeadersDynamicChildrenFrontier {
        &self.inner
    }

    pub fn claims(&self) -> &[SimTechCounterClaim] {
        &self.claims
    }

    /// All bytes directly supplied by the same-frame production `TechState`, including
    /// bytes independently owned by the established runtime frontier.
    pub fn source_produced_walked_bytes(&self) -> usize {
        self.claims
            .iter()
            .copied()
            .map(SimTechCounterClaim::source_produced_bytes)
            .sum()
    }

    pub fn newly_canonicalized_walked_bytes(&self) -> usize {
        self.claims
            .iter()
            .map(|claim| claim.newly_canonical_counter_bytes)
            .sum()
    }

    pub fn sim_duplicate_checked_walked_bytes(&self) -> usize {
        self.claims
            .iter()
            .copied()
            .map(SimTechCounterClaim::duplicate_checked_bytes)
            .sum()
    }

    /// Conditional bytes remaining after subtracting only the six dwords newly owned by
    /// this cohort. The current tech payload and `ages` were already non-conditional.
    pub fn remaining_dynamic_conditionally_admitted_walked_bytes(&self) -> usize {
        self.inner
            .conditionally_admitted_walked_bytes()
            .checked_sub(self.newly_canonicalized_walked_bytes())
            .expect("the bound transcript accounted every promoted counter byte")
    }

    pub fn walk_frontier(&self) -> LeadersWalkFrontier {
        self.inner.walk_frontier()
    }

    /// A six-dword producer cohort does not authorize the remaining conditional channel.
    pub fn checksum(&self) -> Result<(u32, u64), LeadersWalkFrontier> {
        Err(self.walk_frontier())
    }

    pub const fn installed_in_scoreboard(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimTechFrontierError {
    MissingProductionLeader {
        slot: usize,
        available: usize,
    },
    RosterDisagreement {
        slot: usize,
        previous_active: bool,
        step8_active: bool,
        victory_active: bool,
    },
    TechPayloadDisagreement {
        slot: usize,
        byte: usize,
        sim: u8,
        conditional: u8,
    },
    CounterDisagreement {
        slot: usize,
        field: &'static str,
        index: usize,
        sim: i32,
        conditional: i32,
    },
    AgeOwnerDisagreement {
        slot: usize,
        production: i32,
        step8: i32,
    },
    Dynamic(DynamicChildrenFrontierError),
}

impl std::fmt::Display for SimTechFrontierError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "same-frame Leader tech frontier refused: {self:?}")
    }
}

impl std::error::Error for SimTechFrontierError {}

impl From<DynamicChildrenFrontierError> for SimTechFrontierError {
    fn from(value: DynamicChildrenFrontierError) -> Self {
        Self::Dynamic(value)
    }
}

fn require_counter(
    slot: usize,
    field: &'static str,
    index: usize,
    sim: i32,
    authority: &[i32; LEADER_DATA_ENCRYPT_DWORDS],
) -> Result<(), SimTechFrontierError> {
    let conditional = authority[index];
    if sim != conditional {
        return Err(SimTechFrontierError::CounterDisagreement {
            slot,
            field,
            index,
            sim,
            conditional,
        });
    }
    Ok(())
}

/// Join the production runtime's complete current tech state to the conditional child
/// transcript. `previous` and `sim` must describe the same frame; every field this cohort
/// can compare is checked before the completed inner frontier is returned.
pub fn bind_sim_tech_frontier(
    previous: RuntimeLeadersDeferredHistoryFrontier,
    authority: &DynamicLeadersAuthority,
    sim: &Sim,
) -> Result<RuntimeLeadersSimTechFrontier, SimTechFrontierError> {
    let available = sim.production_runtime.leaders.len();
    if available < CHECKSUM_LEADER_SLOTS {
        return Err(SimTechFrontierError::MissingProductionLeader {
            slot: available,
            available,
        });
    }

    let mut claims = Vec::new();
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        let previous_active = previous.rows()[slot].active;
        let step8_active = sim.step8.leaders[slot].flags & leaders::flag::IN_GAME != 0;
        let victory_active =
            sim.vic_leaders.slots[slot].leader_flags & victory_score::leader_flag::VALID != 0;
        if previous_active != step8_active || previous_active != victory_active {
            return Err(SimTechFrontierError::RosterDisagreement {
                slot,
                previous_active,
                step8_active,
                victory_active,
            });
        }
        if !previous_active {
            continue;
        }

        let production = &sim.production_runtime.leaders[slot].tech;
        let row = &authority.rows[slot];
        if let Some(byte) = production
            .tech
            .bytes
            .iter()
            .zip(&row.tech.payload)
            .position(|(sim, conditional)| sim != conditional)
        {
            return Err(SimTechFrontierError::TechPayloadDisagreement {
                slot,
                byte,
                sim: production.tech.bytes[byte],
                conditional: row.tech.payload[byte],
            });
        }

        let counters = production.counters;
        if counters.ages != sim.step8.leaders[slot].econ.age_alt {
            return Err(SimTechFrontierError::AgeOwnerDisagreement {
                slot,
                production: counters.ages,
                step8: sim.step8.leaders[slot].econ.age_alt,
            });
        }
        for (category, value) in counters.epoch.into_iter().enumerate() {
            require_counter(
                slot,
                "epoch",
                ECON_EPOCH_BEGIN + category,
                value,
                &row.economy_plaintext,
            )?;
        }
        for (field, index, value) in [
            ("ages", ECON_AGES_INDEX, counters.ages),
            ("epochs", ECON_EPOCHS_INDEX, counters.epochs),
            ("discovered", ECON_DISCOVERED_INDEX, counters.discovered),
        ] {
            require_counter(slot, field, index, value, &row.economy_plaintext)?;
        }

        claims.push(SimTechCounterClaim {
            slot: slot as u8,
            current_tech_duplicate_bytes: SIM_TECH_PAYLOAD_SOURCE_BYTES,
            ages_duplicate_bytes: 4,
            newly_canonical_counter_bytes: SIM_TECH_NEWLY_CANONICAL_BYTES,
        });
    }

    let inner = bind_dynamic_children_frontier(previous, authority)?;
    Ok(RuntimeLeadersSimTechFrontier { inner, claims })
}
