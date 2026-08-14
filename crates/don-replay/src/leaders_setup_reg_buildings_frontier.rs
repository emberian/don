// SPDX-License-Identifier: GPL-3.0-or-later
//! Frame-zero producer for `LeaderData::reg_buildings[64][129]`.
//!
//! The ordinary starting-town-one transaction owns the complete initial Building census:
//! every admitted player has exactly one active Village center, and the staged [`Sim`]
//! contains no other Build rows. Retail `Wall::increment_stats` maps an active Building
//! into the matrix as `region * 129 + (TypeIndex - 414)`. This module applies that exact
//! mapping to the canonical setup receipts, checks all 16,512 conditional transcript bytes
//! per active row, and promotes the cohort only for that frame-zero setup boundary.
//!
//! This is not a general live owner: the current `Sim` does not yet maintain the matrix
//! through every later Build lifecycle. The receipt therefore stays red and cannot install
//! the Leaders scoreboard channel.

#![forbid(unsafe_code)]

use crate::leader_initial_prefix::CHECKSUM_LEADER_SLOTS;
use crate::leaders_deferred_history_frontier::{
    REG_BUILDINGS_BEGIN, REG_BUILDINGS_END, REG_BUILDING_REGIONS, REG_BUILDING_TYPE_SLOTS,
    REG_BUILDING_VALUES,
};
use crate::leaders_runtime_frontier::{LeadersWalkFrontier, RuntimeCoveredRange};
use crate::leaders_sim_owner_frontier::RuntimeLeadersSimOwnerFrontier;
use crate::setup_cities_builds::{
    StartingPositionEvidence, StartingRegionEvidence, StartingSetupState,
};
use don_sim::objects::{Band, BUILD_BAND_BASE};
use don_sim::systems::bhs_type_table::{BUILD_BEGIN, BUILD_END};
use don_sim::systems::production;
use don_sim::systems::sparse_object_bands_authority_frontier::{RetailBand, RetailObjectAddress};
use don_sim::systems::{leaders, victory_score};
use don_sim::world::WorldObjectIdentity;

pub const WALL_INCREMENT_STATS_VA: u32 = 0x0064_3270;
pub const WALL_INCREMENT_STATS_TOTAL_STORE_VA: u32 = 0x0064_32e2;
pub const WALL_INCREMENT_STATS_REGION_STORE_VA: u32 = 0x0064_3307;
pub const FRAME_ZERO_REG_BUILDINGS_WALKED_BYTES: usize =
    REG_BUILDING_VALUES * std::mem::size_of::<u16>();

const _: () = assert!(BUILD_BEGIN == 414);
const _: () = assert!(BUILD_END - BUILD_BEGIN == REG_BUILDING_TYPE_SLOTS);
const _: () =
    assert!(REG_BUILDINGS_END - REG_BUILDINGS_BEGIN == FRAME_ZERO_REG_BUILDINGS_WALKED_BYTES);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameZeroRegionalBuildingSource {
    CanonicalStartingCityCenterBuilds,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameZeroRegionalBuildingClaim {
    pub slot: u8,
    pub builds_censused: usize,
    pub source: FrameZeroRegionalBuildingSource,
    pub newly_canonical_walked_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameZeroRegionalBuildingCensus {
    rows: [Vec<u16>; CHECKSUM_LEADER_SLOTS],
    active: [bool; CHECKSUM_LEADER_SLOTS],
    claims: Vec<FrameZeroRegionalBuildingClaim>,
}

impl FrameZeroRegionalBuildingCensus {
    pub fn row(&self, slot: usize) -> Option<&[u16]> {
        self.rows.get(slot).map(Vec::as_slice)
    }

    pub fn active(&self, slot: usize) -> Option<bool> {
        self.active.get(slot).copied()
    }

    pub fn claims(&self) -> &[FrameZeroRegionalBuildingClaim] {
        &self.claims
    }

    pub fn newly_canonical_walked_bytes(&self) -> usize {
        self.claims
            .iter()
            .map(|claim| claim.newly_canonical_walked_bytes)
            .sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLeadersFrameZeroRegBuildingsFrontier {
    inner: RuntimeLeadersSimOwnerFrontier,
    census: FrameZeroRegionalBuildingCensus,
}

impl RuntimeLeadersFrameZeroRegBuildingsFrontier {
    pub fn inner(&self) -> &RuntimeLeadersSimOwnerFrontier {
        &self.inner
    }

    pub fn census(&self) -> &FrameZeroRegionalBuildingCensus {
        &self.census
    }

    pub fn newly_canonicalized_walked_bytes(&self) -> usize {
        self.census.newly_canonical_walked_bytes()
    }

    pub fn unique_canonical_walked_bytes(&self) -> usize {
        self.inner.unique_canonical_walked_bytes() + self.newly_canonicalized_walked_bytes()
    }

    pub fn remaining_unsourced_walked_bytes(&self) -> u64 {
        self.inner
            .walk_frontier()
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameZeroRegBuildingsError {
    UnsupportedSetupEvidence,
    SetupCountDisagreement {
        receipt_active: usize,
        city_receipts: usize,
        sim_builds: usize,
    },
    RosterDisagreement {
        slot: usize,
        step8_active: bool,
        victory_active: bool,
        has_city_receipt: bool,
    },
    DuplicateOwner {
        slot: usize,
    },
    DuplicateBuildRow {
        row: usize,
    },
    MissingBuildRow {
        receipt: usize,
        row: usize,
        available: usize,
    },
    BuildIdentityDisagreement {
        receipt: usize,
    },
    ObjectRegistryDisagreement {
        receipt: Option<usize>,
    },
    CityIdentityDisagreement {
        receipt: usize,
    },
    RegionOutOfRange {
        receipt: usize,
        region: i16,
    },
    TypeOutOfRange {
        receipt: usize,
        type_index: i32,
    },
    MatrixOverflow {
        slot: usize,
        region: usize,
        building_slot: usize,
    },
    ConditionalRosterDisagreement {
        slot: usize,
        census_active: bool,
        transcript_active: bool,
    },
    ConditionalDisagreement {
        slot: usize,
        byte: usize,
        census: u8,
        conditional: u8,
    },
}

impl std::fmt::Display for FrameZeroRegBuildingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "frame-zero regional Building census refused: {self:?}")
    }
}

impl std::error::Error for FrameZeroRegBuildingsError {}

/// Derive the complete starting-town-one regional Building matrix from the canonical setup.
/// The staged `Sim` must still contain exactly the Build and City owners named by the setup
/// receipts; any extra, missing, or mutated row refuses instead of being assumed zero.
pub fn derive_frame_zero_regional_building_census(
    setup: &StartingSetupState,
) -> Result<FrameZeroRegionalBuildingCensus, FrameZeroRegBuildingsError> {
    if setup.receipt.position_evidence != StartingPositionEvidence::FirstCameraOnObject2000
        || setup.receipt.region_evidence != StartingRegionEvidence::AllLandSingleComponent
        || setup.receipt.first_checksum_city_image_ready
        || setup.receipt.builds_channel_ready
    {
        return Err(FrameZeroRegBuildingsError::UnsupportedSetupEvidence);
    }

    let city_receipts = setup.receipt.cities.len();
    if setup.receipt.active_players != city_receipts || setup.sim.builds.len() != city_receipts {
        return Err(FrameZeroRegBuildingsError::SetupCountDisagreement {
            receipt_active: setup.receipt.active_players,
            city_receipts,
            sim_builds: setup.sim.builds.len(),
        });
    }
    if !setup.sim.world.object_bands_are_dense_equivalent() {
        return Err(FrameZeroRegBuildingsError::ObjectRegistryDisagreement { receipt: None });
    }

    let mut rows = std::array::from_fn(|_| vec![0u16; REG_BUILDING_VALUES]);
    let mut active = [false; CHECKSUM_LEADER_SLOTS];
    let mut owner_seen = [false; CHECKSUM_LEADER_SLOTS];
    let mut build_seen = vec![false; setup.sim.builds.len()];
    let mut builds_per_owner = [0usize; CHECKSUM_LEADER_SLOTS];

    for (receipt_index, receipt) in setup.receipt.cities.iter().enumerate() {
        let slot = usize::from(receipt.owner);
        if slot >= CHECKSUM_LEADER_SLOTS {
            return Err(FrameZeroRegBuildingsError::BuildIdentityDisagreement {
                receipt: receipt_index,
            });
        }
        if owner_seen[slot] {
            return Err(FrameZeroRegBuildingsError::DuplicateOwner { slot });
        }
        owner_seen[slot] = true;

        let row = receipt.build.row;
        let Some(build) = setup.sim.builds.get(row) else {
            return Err(FrameZeroRegBuildingsError::MissingBuildRow {
                receipt: receipt_index,
                row,
                available: setup.sim.builds.len(),
            });
        };
        if build_seen[row] {
            return Err(FrameZeroRegBuildingsError::DuplicateBuildRow { row });
        }
        build_seen[row] = true;

        let type_index = receipt.build.type_index;
        let registry_index = u32::try_from(build.object_id())
            .ok()
            .and_then(|object_id| object_id.checked_sub(BUILD_BAND_BASE))
            .and_then(|index| usize::try_from(index).ok());
        let dense_registry_row = registry_index.and_then(|index| {
            setup
                .sim
                .world
                .objects
                .slot(slot)
                .band(Band::Build)
                .get(index)
                .copied()
        });
        let sparse_registry_identity =
            setup
                .sim
                .world
                .object_bands()
                .live_identity(RetailObjectAddress::new(
                    receipt.owner,
                    RetailBand::Build,
                    build.object_id() as i32,
                ));
        if receipt.build.dense_registry_row != row as u32
            || receipt.build.sparse_identity != WorldObjectIdentity::BuildRow(row as u32)
            || receipt.build.build_mark_after
                != setup.sim.world.objects.slot(slot).mark(Band::Build)
            || !receipt.build.owner_active_after
            || !setup.sim.world.objects.is_active(slot)
            || dense_registry_row != Some(row as u32)
            || sparse_registry_identity != Some(WorldObjectIdentity::BuildRow(row as u32))
        {
            return Err(FrameZeroRegBuildingsError::ObjectRegistryDisagreement {
                receipt: Some(receipt_index),
            });
        }
        if type_index != receipt.constructor.current_type
            || type_index != receipt.build.registered_ptype
            || setup
                .sim
                .production_runtime
                .build_types
                .get(row)
                .copied()
                .flatten()
                != Some(type_index)
            || build.orig_type != type_index
            || build.who != receipt.owner
            || build.object_id() != receipt.build.object_id
            || build.object_id() != receipt.constructor.object_id
            || build.position() != receipt.snapped_position
            || build.position() != receipt.constructor.position
            || build.city != receipt.city_slot
            || build.flags & (production::flag::VALID | production::flag::ACTIVE)
                != (production::flag::VALID | production::flag::ACTIVE)
        {
            return Err(FrameZeroRegBuildingsError::BuildIdentityDisagreement {
                receipt: receipt_index,
            });
        }

        let city_slot = usize::try_from(receipt.city_slot).map_err(|_| {
            FrameZeroRegBuildingsError::CityIdentityDisagreement {
                receipt: receipt_index,
            }
        })?;
        let city = setup
            .sim
            .cities
            .slots
            .get(slot)
            .and_then(|slots| slots.get(city_slot));
        if city != Some(&receipt.constructor.city)
            || receipt.constructor.city.reg != receipt.region
            || receipt.constructor.region != receipt.region
            || receipt.constructor.city.who != receipt.owner as i8
            || receipt.constructor.city.o != build.object_id()
            || setup.sim.cities.city_mark[slot] != 1
        {
            return Err(FrameZeroRegBuildingsError::CityIdentityDisagreement {
                receipt: receipt_index,
            });
        }

        let region = usize::try_from(receipt.region).map_err(|_| {
            FrameZeroRegBuildingsError::RegionOutOfRange {
                receipt: receipt_index,
                region: receipt.region,
            }
        })?;
        if region >= REG_BUILDING_REGIONS {
            return Err(FrameZeroRegBuildingsError::RegionOutOfRange {
                receipt: receipt_index,
                region: receipt.region,
            });
        }
        let building_slot = usize::try_from(type_index)
            .ok()
            .and_then(|type_index| type_index.checked_sub(BUILD_BEGIN))
            .ok_or_else(|| FrameZeroRegBuildingsError::TypeOutOfRange {
                receipt: receipt_index,
                type_index,
            })?;
        if building_slot >= REG_BUILDING_TYPE_SLOTS || type_index >= BUILD_END as i32 {
            return Err(FrameZeroRegBuildingsError::TypeOutOfRange {
                receipt: receipt_index,
                type_index,
            });
        }
        let index = region * REG_BUILDING_TYPE_SLOTS + building_slot;
        rows[slot][index] =
            rows[slot][index]
                .checked_add(1)
                .ok_or(FrameZeroRegBuildingsError::MatrixOverflow {
                    slot,
                    region,
                    building_slot,
                })?;
        builds_per_owner[slot] += 1;
    }

    if build_seen.iter().any(|seen| !seen) {
        return Err(FrameZeroRegBuildingsError::SetupCountDisagreement {
            receipt_active: setup.receipt.active_players,
            city_receipts,
            sim_builds: setup.sim.builds.len(),
        });
    }

    let mut claims = Vec::with_capacity(city_receipts);
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        let step8_active = setup.sim.step8.leaders[slot].flags & leaders::flag::IN_GAME != 0;
        let victory_active =
            setup.sim.vic_leaders.slots[slot].leader_flags & victory_score::leader_flag::VALID != 0;
        let has_city_receipt = owner_seen[slot];
        if step8_active != victory_active || step8_active != has_city_receipt {
            return Err(FrameZeroRegBuildingsError::RosterDisagreement {
                slot,
                step8_active,
                victory_active,
                has_city_receipt,
            });
        }
        active[slot] = step8_active;
        if active[slot] {
            claims.push(FrameZeroRegionalBuildingClaim {
                slot: slot as u8,
                builds_censused: builds_per_owner[slot],
                source: FrameZeroRegionalBuildingSource::CanonicalStartingCityCenterBuilds,
                newly_canonical_walked_bytes: FRAME_ZERO_REG_BUILDINGS_WALKED_BYTES,
            });
        }
    }

    Ok(FrameZeroRegionalBuildingCensus {
        rows,
        active,
        claims,
    })
}

/// Join a source-derived frame-zero census to the already complete conditional transcript.
pub fn bind_frame_zero_regional_buildings(
    inner: RuntimeLeadersSimOwnerFrontier,
    census: FrameZeroRegionalBuildingCensus,
) -> Result<RuntimeLeadersFrameZeroRegBuildingsFrontier, FrameZeroRegBuildingsError> {
    let deferred = inner.inner().inner().previous();
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        let transcript_active = deferred.rows()[slot].active;
        if census.active[slot] != transcript_active {
            return Err(FrameZeroRegBuildingsError::ConditionalRosterDisagreement {
                slot,
                census_active: census.active[slot],
                transcript_active,
            });
        }
        if !transcript_active {
            continue;
        }

        let mut census_bytes = Vec::with_capacity(FRAME_ZERO_REG_BUILDINGS_WALKED_BYTES);
        for value in &census.rows[slot] {
            census_bytes.extend_from_slice(&value.to_le_bytes());
        }
        let conditional = deferred.rows()[slot]
            .conditionally_admitted_slice(RuntimeCoveredRange {
                begin: REG_BUILDINGS_BEGIN,
                end: REG_BUILDINGS_END,
            })
            .expect("the complete conditional frontier owns reg_buildings");
        if let Some(byte) = conditional
            .iter()
            .zip(&census_bytes)
            .position(|(a, b)| a != b)
        {
            return Err(FrameZeroRegBuildingsError::ConditionalDisagreement {
                slot,
                byte,
                census: census_bytes[byte],
                conditional: conditional[byte],
            });
        }
    }

    Ok(RuntimeLeadersFrameZeroRegBuildingsFrontier { inner, census })
}
