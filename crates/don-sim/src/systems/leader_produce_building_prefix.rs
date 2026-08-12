//! First exact terminal/prefix tranche of `Leader::produce_building` (`0x006E1400`).
//!
//! Retail first resolves the supplied origin Build three times.  An active Build carrying
//! the object CITY bit uses `BuildData::city` and admits placement only when that City row
//! is active.  Every other origin falls back to `BuildTypeData::build_flags & 0x10` on the
//! requested target Type.  If neither route admits the request, the native function returns
//! one through its common epilogue.  Builtin 521 inverts that native convention, so the BHS
//! result is zero.  This happens before coordinate decoding, search, RNG, terrain edits,
//! resource payment, Build allocation, or Group orders.

use crate::objects::{Band, BUILD_BAND_BASE, WALL_BAND_BASE};
use crate::tick::{Sim, NUM_LEADERS};

use super::production::{
    flag,
    runtime::{LiveProductionRuntime, LiveTypeClass},
};

pub const LEADER_PRODUCE_BUILDING_VA: u32 = 0x006e_1400;
pub const LEADER_PRODUCE_BUILDING_BYTES: u32 = 7_406;
pub const LEADER_PRODUCE_BUILDING_CITY_GATE_CONTINUATION_VA: u32 = 0x006e_150d;
pub const LEADER_PRODUCE_BUILDING_CITY_GATE_PREFIX_BYTES: u32 =
    LEADER_PRODUCE_BUILDING_CITY_GATE_CONTINUATION_VA - LEADER_PRODUCE_BUILDING_VA;
pub const LEADER_PRODUCE_BUILDING_REJECT_EPILOGUE_VA: u32 = 0x006e_3076;

/// Target Build Types carrying this bit may search without an active City-linked origin.
pub const BUILD_FLAG_NO_ACTIVE_CITY_REQUIRED: u32 = 0x10;
/// The shared Object flag byte's `0x20` bit is the native City-membership gate here.
pub const ORIGIN_BUILD_CITY_FLAG: u8 = flag::CAPTURED;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaderProduceBuildingPrefixRequest {
    pub owner: u8,
    pub type_index: i32,
    pub origin_build_object: i16,
    pub mode: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaderProduceBuildingPrefixStatus {
    RejectedBeforePlacementSearch,
    ReadyForPlacementSearch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaderProduceBuildingSearchBoundary {
    pub va: u32,
    pub bytes_remaining: u32,
    pub owner: u8,
    pub type_index: i32,
    pub origin_build_object: i16,
    pub mode: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaderProduceBuildingPrefixReceipt {
    pub request: LeaderProduceBuildingPrefixRequest,
    pub status: LeaderProduceBuildingPrefixStatus,
    pub origin_build_row: usize,
    pub origin_flags: u8,
    pub origin_city: i16,
    pub active_city_origin: bool,
    pub city_slot: Option<usize>,
    pub city_active: Option<bool>,
    pub target_build_flags: u32,
    /// True only when retail selected the target-Type fallback instead of a City row.
    pub used_target_fallback: bool,
    pub target_allows_without_active_city: bool,
    /// Native `Leader::produce_building` convention: zero succeeds and one fails.
    pub native_returned: Option<i32>,
    /// Builtin 521 computes `native == 0`; this is therefore zero on the owned rejection.
    pub scenario_returned: Option<i32>,
    pub continuation: Option<LeaderProduceBuildingSearchBoundary>,
}

impl LeaderProduceBuildingPrefixReceipt {
    pub fn validates(&self) -> bool {
        let admitted = if self.used_target_fallback {
            self.target_allows_without_active_city
        } else {
            self.active_city_origin
        };
        match self.status {
            LeaderProduceBuildingPrefixStatus::RejectedBeforePlacementSearch => {
                !admitted
                    && self.native_returned == Some(1)
                    && self.scenario_returned == Some(0)
                    && self.continuation.is_none()
            }
            LeaderProduceBuildingPrefixStatus::ReadyForPlacementSearch => {
                admitted
                    && self.native_returned.is_none()
                    && self.scenario_returned.is_none()
                    && self.continuation.is_some()
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeaderProduceBuildingPrefixError {
    InvalidOwner {
        owner: usize,
    },
    InvalidType {
        type_index: i32,
    },
    MissingBuildObject {
        owner: usize,
        object_index: i16,
    },
    BuildRowOutOfRange {
        owner: usize,
        object_index: i16,
        row: usize,
        builds: usize,
    },
    BuildIdentityMismatch {
        owner: usize,
        object_index: i16,
        row: usize,
        build_owner: u8,
        build_object: i16,
    },
    InvalidBuild {
        owner: usize,
        object_index: i16,
        row: usize,
    },
    TargetTypeMismatch {
        type_index: usize,
    },
    CitySlotOutOfRange {
        owner: usize,
        city: i16,
        mark: i32,
        slots: usize,
    },
    CityIdentityMismatch {
        owner: usize,
        city_slot: usize,
        city_owner: i8,
        city_origin: i16,
        expected_origin: i16,
    },
}

/// Execute the instruction-exact read-only City/target-Type gate at the start of
/// `Leader::produce_building`.
pub fn apply_sim_leader_produce_building_prefix(
    sim: &Sim,
    production: &LiveProductionRuntime,
    request: LeaderProduceBuildingPrefixRequest,
) -> Result<LeaderProduceBuildingPrefixReceipt, LeaderProduceBuildingPrefixError> {
    let owner = request.owner as usize;
    if owner >= NUM_LEADERS {
        return Err(LeaderProduceBuildingPrefixError::InvalidOwner { owner });
    }
    let type_index = usize::try_from(request.type_index).map_err(|_| {
        LeaderProduceBuildingPrefixError::InvalidType {
            type_index: request.type_index,
        }
    })?;
    let Some(type_facts) = production.types.get(type_index).and_then(Option::as_ref) else {
        return Err(LeaderProduceBuildingPrefixError::InvalidType {
            type_index: request.type_index,
        });
    };
    if type_facts.type_index != request.type_index || type_facts.class != LiveTypeClass::Building {
        return Err(LeaderProduceBuildingPrefixError::TargetTypeMismatch { type_index });
    }

    let object_index = request.origin_build_object;
    if !(BUILD_BAND_BASE as i16..WALL_BAND_BASE as i16).contains(&object_index) {
        return Err(LeaderProduceBuildingPrefixError::MissingBuildObject {
            owner,
            object_index,
        });
    }
    let object_slot = (object_index - BUILD_BAND_BASE as i16) as usize;
    let Some(&row) = sim
        .world
        .objects
        .slot(owner)
        .band(Band::Build)
        .get(object_slot)
    else {
        return Err(LeaderProduceBuildingPrefixError::MissingBuildObject {
            owner,
            object_index,
        });
    };
    let row = row as usize;
    let Some(build) = sim.builds.get(row) else {
        return Err(LeaderProduceBuildingPrefixError::BuildRowOutOfRange {
            owner,
            object_index,
            row,
            builds: sim.builds.len(),
        });
    };
    if build.who as usize != owner || build.object_id() != object_index {
        return Err(LeaderProduceBuildingPrefixError::BuildIdentityMismatch {
            owner,
            object_index,
            row,
            build_owner: build.who,
            build_object: build.object_id(),
        });
    }
    if build.flags & flag::VALID == 0 {
        return Err(LeaderProduceBuildingPrefixError::InvalidBuild {
            owner,
            object_index,
            row,
        });
    }

    let city_routed = build.flags & (flag::ACTIVE | ORIGIN_BUILD_CITY_FLAG)
        == (flag::ACTIVE | ORIGIN_BUILD_CITY_FLAG)
        && build.city >= 0;
    let (city_slot, city_active, active_city_origin) = if city_routed {
        let city_slot = build.city as usize;
        let mark = sim.cities.city_mark[owner];
        if city_slot >= sim.cities.slots[owner].len() || mark < 0 || city_slot >= mark as usize {
            return Err(LeaderProduceBuildingPrefixError::CitySlotOutOfRange {
                owner,
                city: build.city,
                mark,
                slots: sim.cities.slots[owner].len(),
            });
        }
        let city = &sim.cities.slots[owner][city_slot];
        if city.who != owner as i8 || city.o != object_index {
            return Err(LeaderProduceBuildingPrefixError::CityIdentityMismatch {
                owner,
                city_slot,
                city_owner: city.who,
                city_origin: city.o,
                expected_origin: object_index,
            });
        }
        (Some(city_slot), Some(city.active()), city.active())
    } else {
        (None, None, false)
    };
    let target_build_flags = type_facts.build_flags;
    let target_allows_without_active_city =
        target_build_flags & BUILD_FLAG_NO_ACTIVE_CITY_REQUIRED != 0;
    let used_target_fallback = !city_routed;
    // A selected but inactive City does not fall through to the target-Type bit. Retail
    // reaches the target fallback only before a nonnegative City index has been selected.
    let admitted = if used_target_fallback {
        target_allows_without_active_city
    } else {
        active_city_origin
    };
    let (status, native_returned, scenario_returned, continuation) = if admitted {
        (
            LeaderProduceBuildingPrefixStatus::ReadyForPlacementSearch,
            None,
            None,
            Some(LeaderProduceBuildingSearchBoundary {
                va: LEADER_PRODUCE_BUILDING_CITY_GATE_CONTINUATION_VA,
                bytes_remaining: LEADER_PRODUCE_BUILDING_BYTES
                    - LEADER_PRODUCE_BUILDING_CITY_GATE_PREFIX_BYTES,
                owner: request.owner,
                type_index: request.type_index,
                origin_build_object: request.origin_build_object,
                mode: request.mode,
            }),
        )
    } else {
        (
            LeaderProduceBuildingPrefixStatus::RejectedBeforePlacementSearch,
            Some(1),
            Some(0),
            None,
        )
    };
    let receipt = LeaderProduceBuildingPrefixReceipt {
        request,
        status,
        origin_build_row: row,
        origin_flags: build.flags,
        origin_city: build.city,
        active_city_origin,
        city_slot,
        city_active,
        target_build_flags,
        used_target_fallback,
        target_allows_without_active_city,
        native_returned,
        scenario_returned,
        continuation,
    };
    debug_assert!(receipt.validates());
    Ok(receipt)
}
