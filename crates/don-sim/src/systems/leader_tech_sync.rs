// SPDX-License-Identifier: GPL-3.0-or-later
//! Atomic synchronization of the canonical production `TechState` into duplicate Leader views.
//!
//! Production completion owns current tech mutation. Victory queries retain a separate
//! `has_tech[806]` view, while the step-8 and compatibility economy views retain decoded
//! `LeaderDataEncrypt::ages`. This adapter preflights every destination before writing any
//! of them, then publishes one committed production state to all duplicate consumers.

#![forbid(unsafe_code)]

use crate::systems::production::runtime::LiveProductionRuntime;
use crate::systems::tech_cities::ty::NUM_TYPES;
use crate::tick::Sim;

pub const RETAIL_TECH_BITS: usize = NUM_TYPES;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaderTechSyncError {
    SlotOutOfRange {
        slot: usize,
    },
    MissingProductionLeader {
        slot: usize,
        available: usize,
    },
    VictoryTechLength {
        slot: usize,
        expected: usize,
        actual: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeaderTechSyncReceipt {
    pub slot: u8,
    pub changed_tech_bits: usize,
    pub step8_ages_before: i32,
    pub facade_ages_before: i32,
    pub ages_after: i32,
}

/// Validate every dynamically shaped destination. A production transaction calls this
/// before its first tech mutation, making a malformed duplicate view a refusal rather than
/// a partially published completion.
pub fn preflight_production_tech_views(
    sim: &Sim,
    runtime: &LiveProductionRuntime,
    slot: usize,
) -> Result<(), LeaderTechSyncError> {
    if slot >= sim.step8.leaders.len()
        || slot >= sim.leaders.len()
        || slot >= sim.vic_leaders.slots.len()
    {
        return Err(LeaderTechSyncError::SlotOutOfRange { slot });
    }
    if runtime.leaders.get(slot).is_none() {
        return Err(LeaderTechSyncError::MissingProductionLeader {
            slot,
            available: runtime.leaders.len(),
        });
    }
    let actual = sim.vic_leaders.slots[slot].has_tech.len();
    if actual != RETAIL_TECH_BITS {
        return Err(LeaderTechSyncError::VictoryTechLength {
            slot,
            expected: RETAIL_TECH_BITS,
            actual,
        });
    }
    Ok(())
}

/// Publish the already-committed production tech state atomically to its duplicate views.
/// All validation precedes all writes, so `Err` leaves every destination unchanged.
pub fn synchronize_production_tech_views(
    sim: &mut Sim,
    runtime: &LiveProductionRuntime,
    slot: usize,
) -> Result<LeaderTechSyncReceipt, LeaderTechSyncError> {
    preflight_production_tech_views(sim, runtime, slot)?;

    let production = &runtime.leaders[slot].tech;
    let desired: Vec<bool> = (0..RETAIL_TECH_BITS)
        .map(|type_index| production.tech.get(type_index as i32))
        .collect();
    let changed_tech_bits = sim.vic_leaders.slots[slot]
        .has_tech
        .iter()
        .zip(&desired)
        .filter(|(before, after)| before != after)
        .count();
    let ages_after = production.counters.ages;
    let receipt = LeaderTechSyncReceipt {
        slot: slot as u8,
        changed_tech_bits,
        step8_ages_before: sim.step8.leaders[slot].econ.age_alt,
        facade_ages_before: sim.leaders[slot].econ.age_alt,
        ages_after,
    };

    sim.vic_leaders.slots[slot]
        .has_tech
        .copy_from_slice(&desired);
    sim.step8.leaders[slot].econ.age_alt = ages_after;
    sim.leaders[slot].econ.age_alt = ages_after;
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_production_state_updates_every_duplicate_owner() {
        let mut sim = Sim::new(7, 4);
        let mut runtime = std::mem::take(&mut sim.production_runtime);
        runtime.leaders[0].tech.gain(100);
        runtime.leaders[0].tech.counters.ages = 3;

        let receipt = synchronize_production_tech_views(&mut sim, &runtime, 0).unwrap();
        assert_eq!(receipt.changed_tech_bits, 1);
        assert_eq!(receipt.ages_after, 3);
        assert!(sim.vic_leaders.slots[0].has_tech[100]);
        assert_eq!(sim.step8.leaders[0].econ.age_alt, 3);
        assert_eq!(sim.leaders[0].econ.age_alt, 3);
    }

    #[test]
    fn malformed_destination_refuses_without_partial_publication() {
        let mut sim = Sim::new(7, 4);
        let mut runtime = std::mem::take(&mut sim.production_runtime);
        runtime.leaders[0].tech.gain(100);
        runtime.leaders[0].tech.counters.ages = 3;
        sim.vic_leaders.slots[0].has_tech.pop();
        sim.step8.leaders[0].econ.age_alt = 11;
        sim.leaders[0].econ.age_alt = 12;
        let tech_before = sim.vic_leaders.slots[0].has_tech.clone();

        assert_eq!(
            synchronize_production_tech_views(&mut sim, &runtime, 0),
            Err(LeaderTechSyncError::VictoryTechLength {
                slot: 0,
                expected: RETAIL_TECH_BITS,
                actual: RETAIL_TECH_BITS - 1,
            })
        );
        assert_eq!(sim.vic_leaders.slots[0].has_tech, tech_before);
        assert_eq!(sim.step8.leaders[0].econ.age_alt, 11);
        assert_eq!(sim.leaders[0].econ.age_alt, 12);
    }
}
