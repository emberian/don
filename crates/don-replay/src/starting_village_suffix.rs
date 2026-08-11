//! Source-bounded attestation for the fresh-City suffix of `Build::activate`.
//!
//! The ordinary starting Village calls `City::find_buildings`, then later
//! `City::regen_roads`.  Those names sound mutating, but the shipped one-center path is
//! deliberately empty: every starting center has Object flag `0x20`, so the global
//! Build/Wall scan rejects it before `Build::add_to_city`, and the center's
//! `BuildData::city_down` chain is empty.  `City::check_upgrade` is also inert because a
//! Village needs six distinct active building kinds before becoming a Town.
//!
//! This module does not manufacture that conclusion from a caller boolean.  It joins the
//! canonical `Sim::builds` rows, production ptypes, City identity, and center chain and
//! refuses any state in which a scanned non-center Build could take the mutating arm.  Its
//! receipt is intended to be consumed by the later atomic starting-Village constructor.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::systems::production::BuildData;
use don_sim::systems::tech_cities::{self, CityRecord};
use don_sim::tick::Sim;

pub const CITY_FIND_BUILDINGS_VA: u32 = 0x0073_84c0;
pub const CITY_READY_TO_UPGRADE_VA: u32 = 0x0073_6480;
pub const CITY_CAN_UPGRADE_VA: u32 = 0x0073_83c0;
pub const CITY_ENOUGH_KINDS_VA: u32 = 0x0073_6540;
pub const CITY_CHECK_UPGRADE_VA: u32 = 0x0073_8b20;
pub const CITY_REGEN_ROADS_VA: u32 = 0x0073_8aa0;
pub const BUILD_ADD_TO_CITY_VA: u32 = 0x0062_2380;
pub const LEADER_PLAN_STRATEGY_VA: u32 = 0x006b_9620;

/// `SubObject::init` sets this Object bit from `BuildTypeData::is_city`.
pub const CITY_CENTER_OBJECT_FLAG: u8 = 0x20;
/// Ordinary post-`Wall::activate` center state: VALID | STARTED | ACTIVE | CITY.
pub const FRESH_CENTER_FLAGS: u8 = 0x27;
/// `rules.xml` `CITY_BUILDINGS=5`; `CityData::can_upgrade(TOWN)` passes value + 1.
pub const SHIPPED_CITY_BUILDINGS: u32 = 5;
pub const TOWN_REQUIRED_DISTINCT_KINDS: u32 = SHIPPED_CITY_BUILDINGS + 1;
pub const FRESH_CAPITAL_CITY_FLAGS: u16 = 0x4011;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FreshVillageUpgradeFacts {
    /// Exact result of `LeaderData::type_avail(TOWN, 1)`.  It controls whether retail
    /// enters `CityData::can_upgrade`; either arm is inert for the one-kind chain.
    pub town_type_available: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FreshVillageFindBuildingsReceipt {
    pub owner: u8,
    pub city_slot: i16,
    pub center_object_id: i16,
    pub scanned_build_rows: u32,
    pub rejected_city_center_rows: u32,
    pub add_to_city_calls: u32,
    pub city_pod_writes: u32,
    pub build_city_link_writes: u32,
    pub ready_to_upgrade_calls: u32,
    pub type_avail_calls: u32,
    pub can_upgrade_calls: u32,
    pub distinct_active_kinds: u32,
    pub required_distinct_kinds: u32,
    pub check_upgrade_body_entered: bool,
    pub regen_members_walked: u32,
    pub regen_build_mask_writes: u32,
    pub leader_writes: u32,
    pub world_writes: u32,
    pub main_rng_draws: u32,
    pub city_suffix_complete: bool,
    /// `Leader::plan_strategy` later clears and recomputes fourteen terrain-census bytes
    /// within City object offsets `+0x62..=+0x71` before the first known replay checkpoint.
    /// This constructor-suffix receipt does not claim that downstream image.
    pub first_checksum_city_image_ready: bool,
}

impl FreshVillageFindBuildingsReceipt {
    /// This receipt settles the City scan/road suffix only.  It cannot by itself certify
    /// the earlier Object/Wall/Build initializer or publish either checksum channel.
    #[inline]
    pub const fn constructor_transaction_ready(self) -> bool {
        false
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FreshVillageSuffixError {
    CityNotFreshCapital {
        city_flags: u16,
    },
    CityOwnerOutOfRange {
        owner: i8,
    },
    CitySlotNegative {
        city_slot: i16,
    },
    CenterObjectIdNegative {
        object_id: i16,
    },
    MissingCenterBuild {
        owner: u8,
        object_id: i16,
    },
    DuplicateCenterBuild {
        owner: u8,
        object_id: i16,
    },
    CenterFlagsNotFresh {
        flags: u8,
    },
    CenterCityMismatch {
        expected: i16,
        actual: i16,
    },
    CenterChainNotEmpty {
        city_down: i16,
    },
    CenterOwnerMismatch {
        expected: u8,
        actual: u8,
    },
    CenterPositionMismatch {
        city: (i32, i32),
        build: (i32, i32),
    },
    MissingCenterType {
        row: usize,
    },
    CenterTypeNotVillage {
        current_type: i32,
    },
    /// A row without Object flag `0x20` reaches the distance/ownership/AddToCity fork in
    /// retail.  Its exact effects require the general transaction and are not fresh setup.
    ScannedNonCenterBuild {
        row: usize,
        owner: u8,
        object_id: i16,
        flags: u8,
    },
}

impl fmt::Display for FreshVillageSuffixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fresh starting-Village suffix refused: {self:?}")
    }
}

impl std::error::Error for FreshVillageSuffixError {}

fn matching_center(build: &BuildData, city: &CityRecord) -> bool {
    build.who == city.who as u8 && build.object_id() == city.o
}

/// Attest the exact `City::find_buildings -> check_upgrade` and `City::regen_roads`
/// result for an ordinary starting-town-1 center.
///
/// This is read-only because the source-proven result is no mutation.  Every Build row is
/// nevertheless inspected: `City::find_buildings` scans the Build and Wall object bands
/// for all eight Leaders, and certifying an empty result from the center alone would miss
/// a non-center row.  DoN stores starting buildings in the canonical Build owner; a future
/// distinct Wall owner must extend this preflight before this receipt is promoted there.
pub fn attest_fresh_starting_village_suffix(
    sim: &Sim,
    city: &CityRecord,
    upgrade: FreshVillageUpgradeFacts,
) -> Result<FreshVillageFindBuildingsReceipt, FreshVillageSuffixError> {
    if city.city_flags != FRESH_CAPITAL_CITY_FLAGS {
        return Err(FreshVillageSuffixError::CityNotFreshCapital {
            city_flags: city.city_flags,
        });
    }
    let owner = u8::try_from(city.who)
        .ok()
        .filter(|owner| usize::from(*owner) < tech_cities::NUM_PLAYERS)
        .ok_or(FreshVillageSuffixError::CityOwnerOutOfRange { owner: city.who })?;
    if city.city < 0 {
        return Err(FreshVillageSuffixError::CitySlotNegative {
            city_slot: city.city,
        });
    }
    if city.o < 0 {
        return Err(FreshVillageSuffixError::CenterObjectIdNegative { object_id: city.o });
    }

    let mut center_row = None;
    let mut rejected_city_center_rows = 0u32;
    for (row, build) in sim.builds.iter().enumerate() {
        if build.flags & CITY_CENTER_OBJECT_FLAG == 0 {
            return Err(FreshVillageSuffixError::ScannedNonCenterBuild {
                row,
                owner: build.who,
                object_id: build.object_id(),
                flags: build.flags,
            });
        }
        rejected_city_center_rows = rejected_city_center_rows.saturating_add(1);
        if matching_center(build, city) && center_row.replace(row).is_some() {
            return Err(FreshVillageSuffixError::DuplicateCenterBuild {
                owner,
                object_id: city.o,
            });
        }
    }

    let row = center_row.ok_or(FreshVillageSuffixError::MissingCenterBuild {
        owner,
        object_id: city.o,
    })?;
    let center = &sim.builds[row];
    if center.flags != FRESH_CENTER_FLAGS {
        return Err(FreshVillageSuffixError::CenterFlagsNotFresh {
            flags: center.flags,
        });
    }
    if center.who != owner {
        return Err(FreshVillageSuffixError::CenterOwnerMismatch {
            expected: owner,
            actual: center.who,
        });
    }
    if center.city != city.city {
        return Err(FreshVillageSuffixError::CenterCityMismatch {
            expected: city.city,
            actual: center.city,
        });
    }
    if center.city_down != -1 {
        return Err(FreshVillageSuffixError::CenterChainNotEmpty {
            city_down: center.city_down,
        });
    }
    if center.position() != (city.x, city.y) {
        return Err(FreshVillageSuffixError::CenterPositionMismatch {
            city: (city.x, city.y),
            build: center.position(),
        });
    }
    let current_type = sim
        .production_runtime
        .build_types
        .get(row)
        .copied()
        .flatten()
        .ok_or(FreshVillageSuffixError::MissingCenterType { row })?;
    if current_type != tech_cities::ty::VILLAGE || center.orig_type != tech_cities::ty::VILLAGE {
        return Err(FreshVillageSuffixError::CenterTypeNotVillage { current_type });
    }

    // `CityData::enough_kinds` starts at the center and follows `city_down`.  The fresh
    // chain contains exactly the active Village center, hence one distinct kind.  The
    // shipped Town threshold is CITY_BUILDINGS + 1 == 6, so `ready_to_upgrade` is false
    // even when type availability admits the second half of its short-circuit.
    let can_upgrade_calls = u32::from(upgrade.town_type_available);
    Ok(FreshVillageFindBuildingsReceipt {
        owner,
        city_slot: city.city,
        center_object_id: city.o,
        scanned_build_rows: u32::try_from(sim.builds.len()).unwrap_or(u32::MAX),
        rejected_city_center_rows,
        add_to_city_calls: 0,
        city_pod_writes: 0,
        build_city_link_writes: 0,
        ready_to_upgrade_calls: 1,
        type_avail_calls: 1,
        can_upgrade_calls,
        distinct_active_kinds: 1,
        required_distinct_kinds: TOWN_REQUIRED_DISTINCT_KINDS,
        check_upgrade_body_entered: false,
        regen_members_walked: 0,
        regen_build_mask_writes: 0,
        leader_writes: 0,
        world_writes: 0,
        main_rng_draws: 0,
        city_suffix_complete: true,
        first_checksum_city_image_ready: false,
    })
}
