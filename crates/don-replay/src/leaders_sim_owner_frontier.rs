// SPDX-License-Identifier: GPL-3.0-or-later
//! Largest current same-frame `Sim` owner cohort for checksum channel 7 (`leaders`).
//!
//! The complete conditional `LeaderData::walk_data` transcript is useful only if its bytes
//! can be tied back to mutable runtime owners.  [`bind_sim_owner_frontier`] rebinds the
//! established victory/step-8 projection to the supplied [`Sim`], retains the production-tech
//! join, and admits every additional fixed-body field currently owned by the City,
//! diplomacy, and production runtimes.  It compares duplicate owners byte-for-byte and
//! refuses malformed or stale views before returning a receipt.
//!
//! This remains a bounded producer.  Large Leader history planes, Personality, mask headers,
//! containers, strings, and several decoded economy fields still have no canonical owner.
//! Consequently this module never installs a scoreboard channel or returns a retail checksum.

#![forbid(unsafe_code)]

use crate::leader_initial_prefix::{InitialLeaderPrefix, CHECKSUM_LEADER_SLOTS};
use crate::leaders_runtime_frontier::{
    bind_live, LeadersRuntimeError, LeadersWalkFrontier, RuntimeCoveredRange,
};
use crate::leaders_sim_tech_frontier::RuntimeLeadersSimTechFrontier;
use don_sim::systems::bhs_type_table::{NUM_TYPES, REGULAR_UNIT_BEGIN, REGULAR_UNIT_END};
use don_sim::tick::Sim;

pub const BLACKEN_OFFSET: usize = 0x020c;
pub const CITY_MARK_OFFSET: usize = 0x0408;
pub const CITIES_CAPTURED_OFFSET: usize = 0x0824;
pub const CITIES_LOST_OFFSET: usize = 0x0828;
pub const TRIBUTE_SENT_OFFSET: usize = 0x0860;
pub const TRIBUTE_RECEIVED_OFFSET: usize = 0x0864;
pub const AGE_STAMP_OFFSET: usize = 0x08f8;
pub const CONTROL_OFFSET: usize = 0x0940;
pub const POP_OFFSET: usize = 0x095c;
pub const TRAINING_QUEUED_OFFSET: usize = 0x0a10;
pub const REG_POP_OFFSET: usize = 0x0e62;
pub const NUM_UNITS_OFFSET: usize = 0x5762;
pub const NUM_QUEUED_OFFSET: usize = 0x5a22;
pub const LAST_UNIT_FINISHED_OFFSET: usize = 0x6274;
pub const AGES_QUEUED_OFFSET: usize = 0x67f4;
pub const EPOCHS_QUEUED_OFFSET: usize = 0x67f5;

pub const REG_POP_VALUES: usize = 64;
pub const AGE_STAMP_VALUES: usize = 7;
pub const TRAINING_QUEUED_VALUES: usize = 6;
pub const NUM_UNIT_VALUES: usize = REGULAR_UNIT_END - REGULAR_UNIT_BEGIN;
pub const LAST_UNIT_FINISHED_VALUES: usize = NUM_UNIT_VALUES;

/// Same-frame bytes observed by this cohort per active row.  This includes duplicate
/// observations which strengthen the agreement gate without increasing unique ownership.
pub const SIM_OWNER_SOURCE_BYTES: usize = 3_962;
/// Bytes already owned by the victory/step-8 runtime frontier and checked against a second
/// canonical `Sim` owner here: unit/queue counts, resource buckets, and control.
pub const SIM_OWNER_DUPLICATE_BYTES: usize = 2_344;
/// Previously conditional bytes which become tied to current `Sim` state in this cohort.
pub const SIM_OWNER_NEWLY_CANONICAL_BYTES: usize = 1_618;

const _: () = assert!(NUM_UNIT_VALUES == 352);
const _: () = assert!(LAST_UNIT_FINISHED_OFFSET + LAST_UNIT_FINISHED_VALUES * 4 == 0x67f4);
const _: () =
    assert!(SIM_OWNER_DUPLICATE_BYTES + SIM_OWNER_NEWLY_CANONICAL_BYTES == SIM_OWNER_SOURCE_BYTES);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimLeaderOwnerSource {
    CityPool,
    VictoryScore,
    DiplomacyPersistent,
    ProductionRuntime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimLeaderOwnerClaim {
    pub slot: u8,
    pub field: &'static str,
    pub source: SimLeaderOwnerSource,
    pub source_bytes: usize,
    pub duplicate_checked_bytes: usize,
    pub newly_canonical_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLeadersSimOwnerFrontier {
    inner: RuntimeLeadersSimTechFrontier,
    claims: Vec<SimLeaderOwnerClaim>,
}

impl RuntimeLeadersSimOwnerFrontier {
    pub fn inner(&self) -> &RuntimeLeadersSimTechFrontier {
        &self.inner
    }

    pub fn claims(&self) -> &[SimLeaderOwnerClaim] {
        &self.claims
    }

    pub fn cohort_source_produced_walked_bytes(&self) -> usize {
        self.claims.iter().map(|claim| claim.source_bytes).sum()
    }

    pub fn cohort_duplicate_checked_walked_bytes(&self) -> usize {
        self.claims
            .iter()
            .map(|claim| claim.duplicate_checked_bytes)
            .sum()
    }

    pub fn cohort_newly_canonicalized_walked_bytes(&self) -> usize {
        self.claims
            .iter()
            .map(|claim| claim.newly_canonical_bytes)
            .sum()
    }

    /// Unique current bytes tied to a same-frame owner across the base runtime, production
    /// tech, and this cohort. Duplicate observations are deliberately counted once.
    pub fn unique_canonical_walked_bytes(&self) -> usize {
        let tribe = self.inner.inner().previous().previous().previous();
        tribe.runtime_claimed_walked_bytes()
            + self.inner.newly_canonicalized_walked_bytes()
            + self.cohort_newly_canonicalized_walked_bytes()
    }

    pub fn remaining_unsourced_walked_bytes(&self) -> u64 {
        self.walk_frontier()
            .bytes_walked
            .saturating_sub(self.unique_canonical_walked_bytes() as u64)
    }

    pub fn walk_frontier(&self) -> LeadersWalkFrontier {
        self.inner.walk_frontier()
    }

    /// Complete representation is not complete ownership. Keep checksum issuance red until
    /// [`Self::remaining_unsourced_walked_bytes`] reaches zero from canonical producers.
    pub fn checksum(&self) -> Result<(u32, u64), LeadersWalkFrontier> {
        Err(self.walk_frontier())
    }

    pub const fn installed_in_scoreboard(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimOwnerFrontierError {
    RuntimeRebind(LeadersRuntimeError),
    RuntimeBaseDisagreement,
    SourceLength {
        slot: usize,
        source: &'static str,
        expected: usize,
        got: usize,
    },
    SourceValueOutOfRange {
        slot: usize,
        field: &'static str,
        index: usize,
        value: i32,
    },
    MissingConditionalRange {
        slot: usize,
        field: &'static str,
        begin: usize,
        end: usize,
    },
    ConditionalDisagreement {
        slot: usize,
        field: &'static str,
        byte: usize,
        conditional: u8,
        sim: u8,
    },
    RuntimeDuplicateDisagreement {
        slot: usize,
        field: &'static str,
        byte: usize,
        established: u8,
        sim: u8,
    },
    EconomyDuplicateDisagreement {
        slot: usize,
        resource: usize,
        established: i32,
        sim: i32,
    },
}

impl std::fmt::Display for SimOwnerFrontierError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "same-frame Leader owner frontier refused: {self:?}")
    }
}

impl std::error::Error for SimOwnerFrontierError {}

impl From<LeadersRuntimeError> for SimOwnerFrontierError {
    fn from(value: LeadersRuntimeError) -> Self {
        Self::RuntimeRebind(value)
    }
}

fn push_claim(
    claims: &mut Vec<SimLeaderOwnerClaim>,
    slot: usize,
    field: &'static str,
    source: SimLeaderOwnerSource,
    source_bytes: usize,
    duplicate_checked_bytes: usize,
) {
    claims.push(SimLeaderOwnerClaim {
        slot: slot as u8,
        field,
        source,
        source_bytes,
        duplicate_checked_bytes,
        newly_canonical_bytes: source_bytes - duplicate_checked_bytes,
    });
}

fn compare_conditional(
    prior: &crate::leaders_deferred_history_frontier::DeferredHistoryRow,
    slot: usize,
    field: &'static str,
    begin: usize,
    sim: &[u8],
) -> Result<(), SimOwnerFrontierError> {
    let end = begin + sim.len();
    let conditional = prior
        .conditionally_admitted_slice(RuntimeCoveredRange { begin, end })
        .ok_or(SimOwnerFrontierError::MissingConditionalRange {
            slot,
            field,
            begin,
            end,
        })?;
    if let Some(byte) = conditional.iter().zip(sim).position(|(a, b)| a != b) {
        return Err(SimOwnerFrontierError::ConditionalDisagreement {
            slot,
            field,
            byte,
            conditional: conditional[byte],
            sim: sim[byte],
        });
    }
    Ok(())
}

fn compare_runtime(
    prior: &crate::leaders_runtime_frontier::RuntimeLeaderRow,
    slot: usize,
    field: &'static str,
    begin: usize,
    sim: &[u8],
) -> Result<(), SimOwnerFrontierError> {
    let end = begin + sim.len();
    let established = prior
        .owned_slice(RuntimeCoveredRange { begin, end })
        .ok_or(SimOwnerFrontierError::MissingConditionalRange {
            slot,
            field,
            begin,
            end,
        })?;
    if let Some(byte) = established.iter().zip(sim).position(|(a, b)| a != b) {
        return Err(SimOwnerFrontierError::RuntimeDuplicateDisagreement {
            slot,
            field,
            byte,
            established: established[byte],
            sim: sim[byte],
        });
    }
    Ok(())
}

fn checked_u16_bytes(
    slot: usize,
    field: &'static str,
    values: &[i32],
) -> Result<Vec<u8>, SimOwnerFrontierError> {
    let mut out = Vec::with_capacity(values.len() * 2);
    for (index, value) in values.iter().copied().enumerate() {
        let value =
            u16::try_from(value).map_err(|_| SimOwnerFrontierError::SourceValueOutOfRange {
                slot,
                field,
                index,
                value,
            })?;
        out.extend_from_slice(&value.to_le_bytes());
    }
    Ok(out)
}

/// Join every currently usable same-frame `Sim` Leader owner to a structurally complete
/// transcript. `setup` is required again so the base victory/step-8 frontier can be rebuilt
/// from this exact `Sim`; a receipt assembled from a stale prior frame is rejected.
pub fn bind_sim_owner_frontier(
    setup: &InitialLeaderPrefix,
    inner: RuntimeLeadersSimTechFrontier,
    sim: &Sim,
) -> Result<RuntimeLeadersSimOwnerFrontier, SimOwnerFrontierError> {
    let deferred = inner.inner().previous();
    let base = deferred.previous().previous().base();
    let rebound = bind_live(setup, &sim.vic_leaders, &sim.step8)?;
    if &rebound != base {
        return Err(SimOwnerFrontierError::RuntimeBaseDisagreement);
    }

    if sim.production_runtime.leaders.len() < CHECKSUM_LEADER_SLOTS {
        return Err(SimOwnerFrontierError::SourceLength {
            slot: sim.production_runtime.leaders.len(),
            source: "production_runtime.leaders",
            expected: CHECKSUM_LEADER_SLOTS,
            got: sim.production_runtime.leaders.len(),
        });
    }

    let mut claims = Vec::new();
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        if !base.rows[slot].active {
            continue;
        }
        let fixed = &deferred.rows()[slot];
        let runtime = &base.rows[slot];
        let production = &sim.production_runtime.leaders[slot];

        let city_mark = sim.cities.city_mark[slot].to_le_bytes();
        compare_conditional(fixed, slot, "city_mark", CITY_MARK_OFFSET, &city_mark)?;
        push_claim(
            &mut claims,
            slot,
            "city_mark",
            SimLeaderOwnerSource::CityPool,
            4,
            0,
        );

        for (field, offset, value) in [
            (
                "cities_captured",
                CITIES_CAPTURED_OFFSET,
                sim.vic_leaders.slots[slot].cities_captured,
            ),
            (
                "cities_lost",
                CITIES_LOST_OFFSET,
                sim.vic_leaders.slots[slot].cities_lost,
            ),
        ] {
            compare_conditional(fixed, slot, field, offset, &value.to_le_bytes())?;
            push_claim(
                &mut claims,
                slot,
                field,
                SimLeaderOwnerSource::VictoryScore,
                4,
                0,
            );
        }

        let diplomacy = &sim.diplomacy.leaders[slot];
        for (field, offset, value) in [
            ("blacken", BLACKEN_OFFSET, diplomacy.repeated_targets),
            ("tribute_sent", TRIBUTE_SENT_OFFSET, diplomacy.sent_raw),
            (
                "tribute_received",
                TRIBUTE_RECEIVED_OFFSET,
                diplomacy.received_scaled,
            ),
        ] {
            compare_conditional(fixed, slot, field, offset, &value.to_le_bytes())?;
            push_claim(
                &mut claims,
                slot,
                field,
                SimLeaderOwnerSource::DiplomacyPersistent,
                4,
                0,
            );
        }

        compare_conditional(
            fixed,
            slot,
            "pop",
            POP_OFFSET,
            &production.population.to_le_bytes(),
        )?;
        push_claim(
            &mut claims,
            slot,
            "pop",
            SimLeaderOwnerSource::ProductionRuntime,
            4,
            0,
        );

        let mut reg_pop = Vec::with_capacity(REG_POP_VALUES * 2);
        for value in production.region_population {
            reg_pop.extend_from_slice(&value.to_le_bytes());
        }
        compare_conditional(fixed, slot, "reg_pop", REG_POP_OFFSET, &reg_pop)?;
        push_claim(
            &mut claims,
            slot,
            "reg_pop",
            SimLeaderOwnerSource::ProductionRuntime,
            reg_pop.len(),
            0,
        );

        let mut age_stamps = Vec::with_capacity(AGE_STAMP_VALUES * 4);
        for value in production.age_stamp {
            age_stamps.extend_from_slice(&value.to_le_bytes());
        }
        compare_conditional(fixed, slot, "age_stamp", AGE_STAMP_OFFSET, &age_stamps)?;
        push_claim(
            &mut claims,
            slot,
            "age_stamp",
            SimLeaderOwnerSource::ProductionRuntime,
            age_stamps.len(),
            0,
        );

        let training = production.carrier_training_queued;
        let mut training_bytes = Vec::with_capacity(TRAINING_QUEUED_VALUES * 4);
        for value in [
            training.barracks,
            training.stable,
            training.factory,
            training.combat,
            training.dock,
            training.air,
        ] {
            training_bytes.extend_from_slice(&value.to_le_bytes());
        }
        compare_conditional(
            fixed,
            slot,
            "training_queued",
            TRAINING_QUEUED_OFFSET,
            &training_bytes,
        )?;
        push_claim(
            &mut claims,
            slot,
            "training_queued",
            SimLeaderOwnerSource::ProductionRuntime,
            training_bytes.len(),
            0,
        );

        for (field, offset, value) in [
            ("ages_queued", AGES_QUEUED_OFFSET, production.ages_queued),
            (
                "epochs_queued",
                EPOCHS_QUEUED_OFFSET,
                production.epochs_queued,
            ),
        ] {
            compare_conditional(fixed, slot, field, offset, &[value])?;
            push_claim(
                &mut claims,
                slot,
                field,
                SimLeaderOwnerSource::ProductionRuntime,
                1,
                0,
            );
        }

        if production.last_unit_finished.len() != LAST_UNIT_FINISHED_VALUES {
            return Err(SimOwnerFrontierError::SourceLength {
                slot,
                source: "production_runtime.last_unit_finished",
                expected: LAST_UNIT_FINISHED_VALUES,
                got: production.last_unit_finished.len(),
            });
        }
        let mut last_finished = Vec::with_capacity(LAST_UNIT_FINISHED_VALUES * 4);
        for value in &production.last_unit_finished {
            last_finished.extend_from_slice(&value.to_le_bytes());
        }
        compare_conditional(
            fixed,
            slot,
            "last_unit_finished",
            LAST_UNIT_FINISHED_OFFSET,
            &last_finished,
        )?;
        push_claim(
            &mut claims,
            slot,
            "last_unit_finished",
            SimLeaderOwnerSource::ProductionRuntime,
            last_finished.len(),
            0,
        );

        if production.unit_counts.len() != NUM_TYPES {
            return Err(SimOwnerFrontierError::SourceLength {
                slot,
                source: "production_runtime.unit_counts",
                expected: NUM_TYPES,
                got: production.unit_counts.len(),
            });
        }
        let unit_counts = checked_u16_bytes(
            slot,
            "num_units",
            &production.unit_counts[REGULAR_UNIT_BEGIN..REGULAR_UNIT_END],
        )?;
        compare_runtime(runtime, slot, "num_units", NUM_UNITS_OFFSET, &unit_counts)?;
        push_claim(
            &mut claims,
            slot,
            "num_units",
            SimLeaderOwnerSource::ProductionRuntime,
            unit_counts.len(),
            unit_counts.len(),
        );

        if production.queued_counts.len() != NUM_TYPES {
            return Err(SimOwnerFrontierError::SourceLength {
                slot,
                source: "production_runtime.queued_counts",
                expected: NUM_TYPES,
                got: production.queued_counts.len(),
            });
        }
        let queued_counts = checked_u16_bytes(slot, "num_queued", &production.queued_counts)?;
        compare_runtime(
            runtime,
            slot,
            "num_queued",
            NUM_QUEUED_OFFSET,
            &queued_counts,
        )?;
        push_claim(
            &mut claims,
            slot,
            "num_queued",
            SimLeaderOwnerSource::ProductionRuntime,
            queued_counts.len(),
            queued_counts.len(),
        );

        for resource in 0..production.resources.len() {
            let index = resource * 9;
            let established = runtime.econ_plaintext()[index]
                .expect("the base runtime owns all six decoded resource buckets");
            let value = production.resources[resource];
            if established != value {
                return Err(SimOwnerFrontierError::EconomyDuplicateDisagreement {
                    slot,
                    resource,
                    established,
                    sim: value,
                });
            }
        }
        push_claim(
            &mut claims,
            slot,
            "resource_buckets",
            SimLeaderOwnerSource::ProductionRuntime,
            24,
            24,
        );

        compare_runtime(
            runtime,
            slot,
            "control",
            CONTROL_OFFSET,
            &production.control.to_le_bytes(),
        )?;
        push_claim(
            &mut claims,
            slot,
            "control",
            SimLeaderOwnerSource::ProductionRuntime,
            4,
            4,
        );
    }

    debug_assert_eq!(
        claims.iter().map(|claim| claim.source_bytes).sum::<usize>(),
        base.rows.iter().filter(|row| row.active).count() * SIM_OWNER_SOURCE_BYTES,
    );
    debug_assert_eq!(
        claims
            .iter()
            .map(|claim| claim.duplicate_checked_bytes)
            .sum::<usize>(),
        base.rows.iter().filter(|row| row.active).count() * SIM_OWNER_DUPLICATE_BYTES,
    );

    Ok(RuntimeLeadersSimOwnerFrontier { inner, claims })
}
