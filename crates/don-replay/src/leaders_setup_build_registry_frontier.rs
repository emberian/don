// SPDX-License-Identifier: GPL-3.0-or-later
//! Frame-zero Building high-water and regional City/Fort/Dock census.
//!
//! The ordinary setup receipt exhaustively owns one active Village per admitted Leader and no
//! other Build. Retail `Build::activate` records the recursive current/upgrade count in
//! `high_buildings[basic_type]`; the replay-carried Rules section owns both `TypeData::from`
//! and `BuildTypeData::to`, so neither chain is caller supplied. The same exhaustive census
//! yields `reg_cities`, while proving that the Fort and Dock registry writers were not reached.
//! These historical values expire with the constructor receipt and never install channel 7.

#![forbid(unsafe_code)]

use crate::groups_pre_pair_unit_authority::{replay_build_type_facts, PrePairUnitAuthorityError};
use crate::leader_initial_prefix::CHECKSUM_LEADER_SLOTS;
use crate::leaders_deferred_history_frontier::{
    HIGH_BUILDINGS_BEGIN, HIGH_BUILDINGS_END, HIGH_BUILDINGS_VALUES, REG_BUILDING_REGIONS,
    REG_BUILDING_TYPE_SLOTS, REG_BUILD_REGISTRY_END, REG_BUILD_REGISTRY_VALUES, REG_CITIES_BEGIN,
};
use crate::leaders_runtime_frontier::{LeadersWalkFrontier, RuntimeCoveredRange};
use crate::leaders_setup_reg_buildings_frontier::{
    derive_frame_zero_regional_building_census, FrameZeroRegBuildingsError,
};
use crate::leaders_setup_region_history_frontier::{
    derive_frame_zero_region_strategy_history, FrameZeroRegionHistoryError,
    RuntimeLeadersFrameZeroRegionHistoryFrontier,
};
use crate::replay::{load_payload, Replay};
use crate::setup_cities_builds::StartingSetupState;
use crate::world_owner_frontier::sha256;
use don_sim::systems::bhs_type_table::{BUILD_BEGIN, BUILD_END};
use don_sim::systems::tech_cities::ty;

pub const BUILD_ACTIVATE_HIGH_WATER_BEGIN_VA: u32 = 0x0062_4ba4;
pub const BUILD_ACTIVATE_TO_LOAD_VA: u32 = 0x0062_4baa;
pub const BUILD_ACTIVATE_HIGH_WATER_END_VA: u32 = 0x0062_4c16;
pub const LEADER_GET_BUILDINGS_VA: u32 = 0x006e_0680;
pub const BUILD_TYPE_BASIC_TYPE_VA: u32 = 0x0063_9970;
pub const FORT_INIT_REGISTRY_STORE_VA: u32 = 0x0073_eb5f;
pub const DOCK_INIT_REGISTRY_STORE_VA: u32 = 0x0074_0b17;

pub const FRAME_ZERO_HIGH_BUILDINGS_WALKED_BYTES: usize =
    HIGH_BUILDINGS_VALUES * std::mem::size_of::<u16>();
pub const FRAME_ZERO_BUILD_REGISTRY_WALKED_BYTES: usize =
    REG_BUILD_REGISTRY_VALUES * std::mem::size_of::<u16>();
pub const FRAME_ZERO_BUILD_CENSUS_WALKED_BYTES: usize =
    FRAME_ZERO_HIGH_BUILDINGS_WALKED_BYTES + FRAME_ZERO_BUILD_REGISTRY_WALKED_BYTES;

const _: () = assert!(FRAME_ZERO_HIGH_BUILDINGS_WALKED_BYTES == 258);
const _: () = assert!(FRAME_ZERO_BUILD_REGISTRY_WALKED_BYTES == 384);
const _: () = assert!(FRAME_ZERO_BUILD_CENSUS_WALKED_BYTES == 642);
const _: () = assert!(HIGH_BUILDINGS_END - HIGH_BUILDINGS_BEGIN == 258);
const _: () = assert!(REG_BUILD_REGISTRY_END - REG_CITIES_BEGIN == 384);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameZeroBuildCensusSource {
    SetupActivationAndReplayRules,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameZeroBuildCensusClaim {
    pub slot: u8,
    pub basic_type_chain: Vec<i32>,
    pub upgrade_chain: Vec<i32>,
    pub basic_type: i32,
    pub source: FrameZeroBuildCensusSource,
    pub newly_canonical_walked_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameZeroBuildRegistryCensus {
    high_buildings: [[u16; HIGH_BUILDINGS_VALUES]; CHECKSUM_LEADER_SLOTS],
    /// Storage order is `reg_cities[64]`, `reg_forts[64]`, `reg_docks[64]`.
    regional_registries: [[u16; REG_BUILD_REGISTRY_VALUES]; CHECKSUM_LEADER_SLOTS],
    active: [bool; CHECKSUM_LEADER_SLOTS],
    claims: Vec<FrameZeroBuildCensusClaim>,
}

impl FrameZeroBuildRegistryCensus {
    pub fn high_buildings(&self, slot: usize) -> Option<&[u16; HIGH_BUILDINGS_VALUES]> {
        self.high_buildings.get(slot)
    }

    pub fn regional_registries(&self, slot: usize) -> Option<&[u16; REG_BUILD_REGISTRY_VALUES]> {
        self.regional_registries.get(slot)
    }

    pub fn claims(&self) -> &[FrameZeroBuildCensusClaim] {
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
pub struct RuntimeLeadersFrameZeroBuildRegistryFrontier {
    inner: Box<RuntimeLeadersFrameZeroRegionHistoryFrontier>,
    census: FrameZeroBuildRegistryCensus,
}

impl RuntimeLeadersFrameZeroBuildRegistryFrontier {
    pub fn inner(&self) -> &RuntimeLeadersFrameZeroRegionHistoryFrontier {
        self.inner.as_ref()
    }

    pub fn census(&self) -> &FrameZeroBuildRegistryCensus {
        &self.census
    }

    pub fn newly_canonicalized_walked_bytes(&self) -> usize {
        self.census.newly_canonical_walked_bytes()
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
pub enum FrameZeroBuildRegistryError {
    RegionalBuildings(FrameZeroRegBuildingsError),
    RegionHistory(FrameZeroRegionHistoryError),
    ReplaySourceDisagreement,
    MissingReplayRules,
    PayloadLoad(String),
    PayloadSha256Disagreement,
    TypeFacts(PrePairUnitAuthorityError),
    BuildLinkOutOfRange {
        field: &'static str,
        from: i32,
        to: i32,
    },
    BuildLinkCycle {
        field: &'static str,
        at: i32,
    },
    MissingOwnerReceipt {
        slot: usize,
    },
    RegionalCityAgreement {
        slot: usize,
        region: usize,
        census: u16,
        constructor_delta: i16,
    },
    ConditionalRosterDisagreement {
        slot: usize,
        census_active: bool,
        transcript_active: bool,
    },
    ConditionalDisagreement {
        slot: usize,
        field: &'static str,
        byte: usize,
        census: u8,
        conditional: u8,
    },
}

impl std::fmt::Display for FrameZeroBuildRegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "frame-zero Building registry census refused: {self:?}")
    }
}

impl std::error::Error for FrameZeroBuildRegistryError {}

impl From<FrameZeroRegBuildingsError> for FrameZeroBuildRegistryError {
    fn from(value: FrameZeroRegBuildingsError) -> Self {
        Self::RegionalBuildings(value)
    }
}

impl From<FrameZeroRegionHistoryError> for FrameZeroBuildRegistryError {
    fn from(value: FrameZeroRegionHistoryError) -> Self {
        Self::RegionHistory(value)
    }
}

impl From<PrePairUnitAuthorityError> for FrameZeroBuildRegistryError {
    fn from(value: PrePairUnitAuthorityError) -> Self {
        Self::TypeFacts(value)
    }
}

fn build_link_chain(
    payload: &[u8],
    rules: &crate::initial::InitialRules,
    start: i32,
    field: &'static str,
) -> Result<Vec<i32>, FrameZeroBuildRegistryError> {
    let mut chain = Vec::new();
    let mut current = start;
    loop {
        if chain.contains(&current) {
            return Err(FrameZeroBuildRegistryError::BuildLinkCycle { field, at: current });
        }
        chain.push(current);
        let facts = replay_build_type_facts(payload, rules, current)?;
        let next = match field {
            "from" => facts.from,
            "to" => facts.to,
            _ => unreachable!("the caller selects one executable Build link"),
        };
        if next < 0 {
            return Ok(chain);
        }
        if !(BUILD_BEGIN as i32..BUILD_END as i32).contains(&next) {
            return Err(FrameZeroBuildRegistryError::BuildLinkOutOfRange {
                field,
                from: current,
                to: next,
            });
        }
        current = next;
    }
}

/// Derive historical setup values from the exhaustive Build census and replay-carried type
/// graph. The replay payload is reloaded and digest-bound to the receipt before any type link is
/// followed; a caller cannot substitute a convenient `basic_type` or upgrade chain.
pub fn derive_frame_zero_build_registry_census(
    setup: &StartingSetupState,
    replay: &Replay,
) -> Result<FrameZeroBuildRegistryCensus, FrameZeroBuildRegistryError> {
    let regional = derive_frame_zero_regional_building_census(setup)?;
    derive_frame_zero_region_strategy_history(setup)?;
    if setup.receipt.replay_payload_sha256 != replay.initial.payload_sha256 {
        return Err(FrameZeroBuildRegistryError::ReplaySourceDisagreement);
    }
    let rules = replay
        .initial
        .rules
        .as_ref()
        .ok_or(FrameZeroBuildRegistryError::MissingReplayRules)?;
    let payload = load_payload(&replay.path)
        .map_err(|error| FrameZeroBuildRegistryError::PayloadLoad(error.to_string()))?;
    if sha256(&payload) != setup.receipt.replay_payload_sha256 {
        return Err(FrameZeroBuildRegistryError::PayloadSha256Disagreement);
    }

    let mut high_buildings = [[0u16; HIGH_BUILDINGS_VALUES]; CHECKSUM_LEADER_SLOTS];
    let mut regional_registries = [[0u16; REG_BUILD_REGISTRY_VALUES]; CHECKSUM_LEADER_SLOTS];
    let mut active = [false; CHECKSUM_LEADER_SLOTS];
    let mut claims = Vec::with_capacity(regional.claims().len());

    for building in regional.claims() {
        let slot = usize::from(building.slot);
        let receipt = setup
            .receipt
            .cities
            .iter()
            .find(|city| usize::from(city.owner) == slot)
            .ok_or(FrameZeroBuildRegistryError::MissingOwnerReceipt { slot })?;
        let type_index = receipt.build.type_index;
        let basic_type_chain = build_link_chain(&payload, rules, type_index, "from")?;
        let upgrade_chain = build_link_chain(&payload, rules, type_index, "to")?;
        let basic_type = *basic_type_chain
            .last()
            .expect("a Build link trace contains its starting type");

        let row = regional
            .row(slot)
            .expect("the exhaustive setup census has every Leader row");
        let recursive_count = upgrade_chain.iter().fold(0u16, |count, type_index| {
            let type_slot = usize::try_from(*type_index)
                .expect("the checked Build chain is nonnegative")
                - BUILD_BEGIN;
            let current = (0..REG_BUILDING_REGIONS).fold(0u16, |sum, region| {
                sum.wrapping_add(row[region * REG_BUILDING_TYPE_SLOTS + type_slot])
            });
            count.wrapping_add(current)
        });
        let basic_slot = usize::try_from(basic_type)
            .expect("the checked Build chain is nonnegative")
            - BUILD_BEGIN;
        high_buildings[slot][basic_slot] = recursive_count;

        for region in 0..REG_BUILDING_REGIONS {
            let region_begin = region * REG_BUILDING_TYPE_SLOTS;
            let city_count = [ty::VILLAGE, ty::TOWN, ty::METROPOLIS, ty::FORBIDDEN_CITY]
                .into_iter()
                .fold(0u16, |count, type_index| {
                    count.wrapping_add(
                        row[region_begin + usize::try_from(type_index).unwrap() - BUILD_BEGIN],
                    )
                });
            regional_registries[slot][region] = city_count;
        }
        let region = usize::try_from(receipt.region).map_err(|_| {
            FrameZeroBuildRegistryError::RegionalCityAgreement {
                slot,
                region: usize::MAX,
                census: 0,
                constructor_delta: receipt.constructor.effects.leader_region_cities_delta,
            }
        })?;
        let census = regional_registries[slot][region];
        let constructor_delta = receipt.constructor.effects.leader_region_cities_delta;
        if i32::from(census) != i32::from(constructor_delta) {
            return Err(FrameZeroBuildRegistryError::RegionalCityAgreement {
                slot,
                region,
                census,
                constructor_delta,
            });
        }

        active[slot] = true;
        claims.push(FrameZeroBuildCensusClaim {
            slot: building.slot,
            basic_type_chain,
            upgrade_chain,
            basic_type,
            source: FrameZeroBuildCensusSource::SetupActivationAndReplayRules,
            newly_canonical_walked_bytes: FRAME_ZERO_BUILD_CENSUS_WALKED_BYTES,
        });
    }

    Ok(FrameZeroBuildRegistryCensus {
        high_buildings,
        regional_registries,
        active,
        claims,
    })
}

fn census_bytes(values: &[u16]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

/// Bind the two non-contiguous source-derived cohorts to the independent conditional transcript.
pub fn bind_frame_zero_build_registry_census(
    inner: RuntimeLeadersFrameZeroRegionHistoryFrontier,
    census: FrameZeroBuildRegistryCensus,
) -> Result<RuntimeLeadersFrameZeroBuildRegistryFrontier, FrameZeroBuildRegistryError> {
    let deferred = inner.inner().inner().inner().inner().inner().previous();
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        let transcript_active = deferred.rows()[slot].active;
        if census.active[slot] != transcript_active {
            return Err(FrameZeroBuildRegistryError::ConditionalRosterDisagreement {
                slot,
                census_active: census.active[slot],
                transcript_active,
            });
        }
        if !transcript_active {
            continue;
        }

        for (field, range, bytes) in [
            (
                "regional_build_registries",
                RuntimeCoveredRange {
                    begin: REG_CITIES_BEGIN,
                    end: REG_BUILD_REGISTRY_END,
                },
                census_bytes(&census.regional_registries[slot]),
            ),
            (
                "high_buildings",
                RuntimeCoveredRange {
                    begin: HIGH_BUILDINGS_BEGIN,
                    end: HIGH_BUILDINGS_END,
                },
                census_bytes(&census.high_buildings[slot]),
            ),
        ] {
            let conditional = deferred.rows()[slot]
                .conditionally_admitted_slice(range)
                .expect("the complete conditional frontier owns the setup Building census");
            if let Some(byte) = conditional.iter().zip(&bytes).position(|(a, b)| a != b) {
                return Err(FrameZeroBuildRegistryError::ConditionalDisagreement {
                    slot,
                    field,
                    byte,
                    census: bytes[byte],
                    conditional: conditional[byte],
                });
            }
        }
    }

    Ok(RuntimeLeadersFrameZeroBuildRegistryFrontier {
        inner: Box::new(inner),
        census,
    })
}
