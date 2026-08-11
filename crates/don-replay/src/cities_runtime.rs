//! Exact, fail-closed adapter from the canonical typed City owner to the retail Cities
//! checksum walk.
//!
//! `don_sim::systems::tech_cities::CityPool` owns the complete fixed City bytes and the
//! dynamic `Array<CaravanLink>` payload, but `don_sim::tick::Sim` does not yet retain a
//! `CityPool`.  Conversely, `Sim` owns the center Build registry, current Build types, and
//! several mirrors of `LeaderData::leader_flags`.  This module joins those owners without
//! installing a replay channel or pretending setup has created the starting cities.
//!
//! The generic fixed-image replay walker is not sufficient for City.  `City::walk_data`
//! skips both `String` members for a checksum visitor, while `Array<CaravanLink>::walk_data`
//! emits stack temporaries plus a pointer-owned element array.  Those branches cannot be
//! reconstructed from a flat 192-byte City image.  The typed adapter below emits the exact
//! checksum-direction stream instead.

use std::collections::BTreeMap;
use std::fmt;

use don_sim::objects::{Band, BUILD_BAND_BASE};
use don_sim::systems::leaders;
use don_sim::systems::tech_cities::{CityPool, CityRecord, NUM_PLAYERS};
use don_sim::systems::victory_score;
use don_sim::tick::Sim;

/// `CheckSums::check_cities(CheckSum*)` (checksums.cpp).
pub const CHECK_CITIES_VA: u32 = 0x0093_7600;
/// `City::walk_data(DataWalk*)`.
pub const CITY_WALK_DATA_VA: u32 = 0x0048_9220;
/// `Array<CaravanLink>::walk_data(DataWalk*)`.
pub const CARAVAN_ARRAY_WALK_VA: u32 = 0x0048_9040;
/// `Cities::init()` constructs the initial 8 x 20 pool.
pub const CITIES_INIT_VA: u32 = 0x0073_58c0;
/// `Cities::init_city(int, int, int, int)` allocates/reuses a slot and calls `City::init`.
pub const CITIES_INIT_CITY_VA: u32 = 0x0073_52c0;
/// `City::init(int, char, short, int, int)`.
pub const CITY_INIT_VA: u32 = 0x0073_7050;
/// Earliest ordinary setup caller which creates the starting center Build.
pub const SETUP_BUILD_CITIES_VA: u32 = 0x005a_b910;
/// Caller which supplies `Setup::build_cities`' unresolved start-array index.
pub const SETUP_BUILD_GAME_VA: u32 = 0x005a_c190;

/// One active City with an empty caravan array walks two bytes of flags, the 108-byte POD
/// body, and a four-byte zero caravan count.
pub const EMPTY_CARAVAN_CITY_WALK_BYTES: u64 = 114;

/// Result of walking one active City record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityWalkValue {
    pub checksum: u32,
    pub bytes_walked: u64,
}

/// Complete isolated Cities-channel value over an explicitly supplied canonical pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CitiesChannelValue {
    pub checksum: u32,
    pub bytes_walked: u64,
    pub cities_walked: u32,
    /// Logical City slots inspected for Leader rows whose retail `leader_flags & 1` gate
    /// was set. Dead slots do not contribute bytes but still consume traversal positions.
    pub slots_scanned: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CityWalkError {
    CaravanLengthOverflow { length: usize },
}

impl fmt::Display for CityWalkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CaravanLengthOverflow { length } => write!(
                f,
                "City caravan length {length} does not fit retail's signed 32-bit field"
            ),
        }
    }
}

impl std::error::Error for CityWalkError {}

/// Refusal while joining the City, Leader, object-registry, and production owners.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CitiesRuntimeError {
    CityOwnerSlotCount {
        slots: usize,
    },
    LeaderOwnerSlotCount {
        slots: usize,
    },
    /// DoN currently mirrors the one retail `leader_flags & 1` fact four ways. Choosing
    /// whichever copy is convenient would make the channel dependent on adapter order.
    LeaderValidityMismatch {
        owner: usize,
        victory: bool,
        step8: bool,
        economy: bool,
        object_registry: bool,
    },
    NegativeCityMark {
        owner: usize,
        mark: i32,
    },
    CityMarkOutsidePool {
        owner: usize,
        mark: usize,
        length: usize,
    },
    ActiveCityBeyondMark {
        owner: usize,
        slot: usize,
        mark: usize,
    },
    CitySlotIndexOverflow {
        owner: usize,
        slot: usize,
    },
    CitySlotMismatch {
        owner: usize,
        slot: usize,
        city_slot: i16,
    },
    CityOwnerMismatch {
        owner: usize,
        slot: usize,
        city_owner: i8,
    },
    InvalidCenterObjectId {
        owner: usize,
        slot: usize,
        object_id: i16,
    },
    CenterObjectOutsideBuildBand {
        owner: usize,
        slot: usize,
        object_id: u32,
        build_band_length: usize,
    },
    CenterBuildRowOutOfRange {
        owner: usize,
        slot: usize,
        object_id: u32,
        build_row: usize,
        build_rows: usize,
    },
    DuplicateCenterBuild {
        owner: usize,
        object_id: u32,
        first_slot: usize,
        second_slot: usize,
    },
    CenterBuildInactive {
        owner: usize,
        slot: usize,
        object_id: u32,
        flags: u8,
    },
    CenterBuildOwnerMismatch {
        owner: usize,
        slot: usize,
        object_id: u32,
        build_owner: u8,
    },
    CenterBuildObjectIdMismatch {
        owner: usize,
        slot: usize,
        registry_object_id: u32,
        build_object_id: i16,
    },
    CenterBuildCityMismatch {
        owner: usize,
        slot: usize,
        object_id: u32,
        build_city: i16,
        city_slot: i16,
    },
    CenterBuildPositionMismatch {
        owner: usize,
        slot: usize,
        object_id: u32,
        build_position: (i32, i32),
        city_position: (i32, i32),
    },
    MissingCenterBuildType {
        owner: usize,
        slot: usize,
        object_id: u32,
        build_row: usize,
    },
    CityWalk {
        owner: usize,
        slot: usize,
        source: CityWalkError,
    },
    CountOverflow,
}

impl fmt::Display for CitiesRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Cities runtime refused: {self:?}")
    }
}

impl std::error::Error for CitiesRuntimeError {}

/// Produce the exact bytes `City::walk_data` hands to a checksum visitor.
///
/// Every invocation emits `city_flags`; a dead row then returns. The two save-only `String`
/// calls on a live row are absent by construction: retail gates them on
/// `DataWalk::checksum == 0`, while `CheckSum` stores one at `+8`. For a non-empty caravan
/// array the engine emits length, capacity, growth short, `flags & 0xbf`, then each
/// `{cara, who}` pair in logical order. For an empty array it emits only the length.
pub fn city_walk_bytes(city: &CityRecord) -> Result<Vec<u8>, CityWalkError> {
    if !city.active() {
        return Ok(city.city_flags.to_le_bytes().to_vec());
    }
    let caravan_len =
        i32::try_from(city.vans.items.len()).map_err(|_| CityWalkError::CaravanLengthOverflow {
            length: city.vans.items.len(),
        })?;

    let extra = if caravan_len == 0 {
        4
    } else {
        4 + 4 + 2 + 1 + city.vans.items.len().saturating_mul(8)
    };
    let mut out = Vec::with_capacity(110usize.saturating_add(extra));
    out.extend_from_slice(&city.city_flags.to_le_bytes());
    out.extend_from_slice(&city.pod_bytes());
    out.extend_from_slice(&caravan_len.to_le_bytes());
    if caravan_len != 0 {
        out.extend_from_slice(&city.vans.capacity.to_le_bytes());
        out.extend_from_slice(&city.vans.grow.to_le_bytes());
        out.push(city.vans.flags & 0xbf);
        for link in &city.vans.items {
            out.extend_from_slice(&link.cara.to_le_bytes());
            out.extend_from_slice(&link.who.to_le_bytes());
        }
    }
    Ok(out)
}

pub fn city_walk_value(city: &CityRecord) -> Result<CityWalkValue, CityWalkError> {
    let bytes = city_walk_bytes(city)?;
    Ok(CityWalkValue {
        checksum: don_sim::checksum::adler32(1, &bytes),
        bytes_walked: bytes.len() as u64,
    })
}

fn validate_leader_mirrors(sim: &Sim) -> Result<[bool; NUM_PLAYERS], CitiesRuntimeError> {
    if sim.vic_leaders.slots.len() != NUM_PLAYERS {
        return Err(CitiesRuntimeError::LeaderOwnerSlotCount {
            slots: sim.vic_leaders.slots.len(),
        });
    }
    let mut valid = [false; NUM_PLAYERS];
    for owner in 0..NUM_PLAYERS {
        let victory =
            sim.vic_leaders.slots[owner].leader_flags & victory_score::leader_flag::VALID != 0;
        let step8 = sim.step8.leaders[owner].flags & leaders::flag::IN_GAME != 0;
        let economy = sim.leaders[owner].active;
        let object_registry = sim.world.objects.is_active(owner);
        if victory != step8 || victory != economy || victory != object_registry {
            return Err(CitiesRuntimeError::LeaderValidityMismatch {
                owner,
                victory,
                step8,
                economy,
                object_registry,
            });
        }
        valid[owner] = victory;
    }
    Ok(valid)
}

fn validate_city_join(
    sim: &Sim,
    owner: usize,
    slot: usize,
    city: &CityRecord,
    center_builds: &mut BTreeMap<(usize, u32), usize>,
) -> Result<(), CitiesRuntimeError> {
    let expected_slot = i16::try_from(slot)
        .map_err(|_| CitiesRuntimeError::CitySlotIndexOverflow { owner, slot })?;
    if city.city != expected_slot {
        return Err(CitiesRuntimeError::CitySlotMismatch {
            owner,
            slot,
            city_slot: city.city,
        });
    }
    if city.who != owner as i8 {
        return Err(CitiesRuntimeError::CityOwnerMismatch {
            owner,
            slot,
            city_owner: city.who,
        });
    }
    let object_id =
        u32::try_from(city.o).map_err(|_| CitiesRuntimeError::InvalidCenterObjectId {
            owner,
            slot,
            object_id: city.o,
        })?;
    let Some(offset) = object_id.checked_sub(BUILD_BAND_BASE) else {
        return Err(CitiesRuntimeError::InvalidCenterObjectId {
            owner,
            slot,
            object_id: city.o,
        });
    };
    let build_band = sim.world.objects.slot(owner).band(Band::Build);
    let Some(&build_row_u32) = build_band.get(offset as usize) else {
        return Err(CitiesRuntimeError::CenterObjectOutsideBuildBand {
            owner,
            slot,
            object_id,
            build_band_length: build_band.len(),
        });
    };
    let build_row = build_row_u32 as usize;
    let Some(build) = sim.builds.get(build_row) else {
        return Err(CitiesRuntimeError::CenterBuildRowOutOfRange {
            owner,
            slot,
            object_id,
            build_row,
            build_rows: sim.builds.len(),
        });
    };
    if let Some(first_slot) = center_builds.insert((owner, object_id), slot) {
        return Err(CitiesRuntimeError::DuplicateCenterBuild {
            owner,
            object_id,
            first_slot,
            second_slot: slot,
        });
    }
    if !build.is_valid() || !build.is_active() {
        return Err(CitiesRuntimeError::CenterBuildInactive {
            owner,
            slot,
            object_id,
            flags: build.flags,
        });
    }
    if build.who as usize != owner {
        return Err(CitiesRuntimeError::CenterBuildOwnerMismatch {
            owner,
            slot,
            object_id,
            build_owner: build.who,
        });
    }
    if build.object_id() != object_id as i16 {
        return Err(CitiesRuntimeError::CenterBuildObjectIdMismatch {
            owner,
            slot,
            registry_object_id: object_id,
            build_object_id: build.object_id(),
        });
    }
    if build.city != city.city {
        return Err(CitiesRuntimeError::CenterBuildCityMismatch {
            owner,
            slot,
            object_id,
            build_city: build.city,
            city_slot: city.city,
        });
    }
    let build_position = build.position();
    let city_position = (city.x, city.y);
    if build_position != city_position {
        return Err(CitiesRuntimeError::CenterBuildPositionMismatch {
            owner,
            slot,
            object_id,
            build_position,
            city_position,
        });
    }
    if sim
        .production_runtime
        .build_types
        .get(build_row)
        .copied()
        .flatten()
        .is_none()
    {
        return Err(CitiesRuntimeError::MissingCenterBuildType {
            owner,
            slot,
            object_id,
            build_row,
        });
    }
    Ok(())
}

/// Produce the isolated retail Cities checksum over `cities`, after validating its joins
/// to the canonical Sim-owned Leader and center-Build state.
///
/// Validation covers every active City, including one under a currently invalid Leader;
/// hashing follows retail and scans only the eight `leader_flags & 1` owners, in fixed
/// owner order and full logical `PtrArray<City>` slot order. No recorded checksum enters
/// this function.
pub fn check_sim_cities(
    sim: &Sim,
    cities: &CityPool,
) -> Result<CitiesChannelValue, CitiesRuntimeError> {
    if cities.slots.len() != NUM_PLAYERS {
        return Err(CitiesRuntimeError::CityOwnerSlotCount {
            slots: cities.slots.len(),
        });
    }
    let leader_valid = validate_leader_mirrors(sim)?;
    let mut center_builds = BTreeMap::new();

    for owner in 0..NUM_PLAYERS {
        let raw_mark = cities.city_mark[owner];
        let mark = usize::try_from(raw_mark).map_err(|_| CitiesRuntimeError::NegativeCityMark {
            owner,
            mark: raw_mark,
        })?;
        let length = cities.slots[owner].len();
        if mark > length {
            return Err(CitiesRuntimeError::CityMarkOutsidePool {
                owner,
                mark,
                length,
            });
        }
        for (slot, city) in cities.slots[owner].iter().enumerate() {
            if !city.active() {
                continue;
            }
            if slot >= mark {
                return Err(CitiesRuntimeError::ActiveCityBeyondMark { owner, slot, mark });
            }
            validate_city_join(sim, owner, slot, city, &mut center_builds)?;
            city_walk_bytes(city).map_err(|source| CitiesRuntimeError::CityWalk {
                owner,
                slot,
                source,
            })?;
        }
    }

    let mut checksum = 1u32;
    let mut bytes_walked = 0u64;
    let mut cities_walked = 0u32;
    let mut slots_scanned = 0u32;
    for owner in 0..NUM_PLAYERS {
        if !leader_valid[owner] {
            continue;
        }
        slots_scanned = slots_scanned
            .checked_add(
                u32::try_from(cities.slots[owner].len())
                    .map_err(|_| CitiesRuntimeError::CountOverflow)?,
            )
            .ok_or(CitiesRuntimeError::CountOverflow)?;
        for (slot, city) in cities.slots[owner].iter().enumerate() {
            if !city.active() {
                continue;
            }
            let bytes = city_walk_bytes(city).map_err(|source| CitiesRuntimeError::CityWalk {
                owner,
                slot,
                source,
            })?;
            checksum = don_sim::checksum::adler32(checksum, &bytes);
            bytes_walked = bytes_walked
                .checked_add(bytes.len() as u64)
                .ok_or(CitiesRuntimeError::CountOverflow)?;
            cities_walked = cities_walked
                .checked_add(1)
                .ok_or(CitiesRuntimeError::CountOverflow)?;
        }
    }

    Ok(CitiesChannelValue {
        checksum,
        bytes_walked,
        cities_walked,
        slots_scanned,
    })
}
