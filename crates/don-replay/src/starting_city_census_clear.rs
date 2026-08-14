//! Exact clear prefix of the frame-zero `Leader::plan_strategy` City census.
//!
//! Before reading any Build or Unit, retail walks the current Leader's live Cities and
//! clears `gatherers`, `busy`, and `free`, then initializes `peasant_dist` to the City
//! slot plus 100. This bounded writer is independent of the later canonical Unit receiver
//! images, so it can be owned without weakening the complete Unit-census transaction.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::systems::tech_cities::{CITY_POD_LEN, NUM_PLAYERS};
use don_sim::tick::Sim;

use crate::cities_runtime::{check_sim_owned_cities, CitiesRuntimeError};

pub const LEADER_PLAN_STRATEGY_VA: u32 = 0x006b_9620;
pub const CITY_CENSUS_CLEAR_LOOP_BEGIN_VA: u32 = 0x006b_9746;
pub const CITY_GATHERERS_CLEAR_STORE_VA: u32 = 0x006b_976f;
pub const CITY_BUSY_CLEAR_STORE_VA: u32 = 0x006b_9789;
pub const CITY_FREE_CLEAR_STORE_VA: u32 = 0x006b_97a3;
pub const CITY_PEASANT_DIST_INIT_STORE_VA: u32 = 0x006b_97bd;
pub const CITY_CENSUS_CLEAR_LOOP_END_VA: u32 = 0x006b_97ca;
pub const INITIAL_PEASANT_DIST_BASE: i16 = 100;

/// Value in `si` at `0x006B97BD`: the low signed-short image of `city_slot + 100`.
pub const fn initial_peasant_dist_for_slot(city_slot: i16) -> i16 {
    city_slot.wrapping_add(INITIAL_PEASANT_DIST_BASE)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartingCityCensusClearMutation {
    pub owner: usize,
    pub slot: usize,
    pub before: [u8; CITY_POD_LEN],
    pub after: [u8; CITY_POD_LEN],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartingCityCensusClearReceipt {
    pub owners_walked: u32,
    pub slots_scanned: u32,
    pub cities_cleared: u32,
    pub city_pod_bytes_changed: u32,
    pub cities: Vec<StartingCityCensusClearMutation>,
    pub city_census_clear_complete: bool,
    /// The later Unit walk can still rewrite all four fields.
    pub city_unit_census_complete: bool,
    pub first_checksum_city_image_ready: bool,
    pub world_writes: u32,
    pub build_or_registry_writes: u32,
    pub main_rng_draws: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartingCityCensusClearError {
    Cities(CitiesRuntimeError),
    CountOverflow,
}

impl fmt::Display for StartingCityCensusClearError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "starting City census clear refused: {self:?}")
    }
}

impl std::error::Error for StartingCityCensusClearError {}

impl From<CitiesRuntimeError> for StartingCityCensusClearError {
    fn from(value: CitiesRuntimeError) -> Self {
        Self::Cities(value)
    }
}

fn add_count(value: &mut u32) -> Result<(), StartingCityCensusClearError> {
    *value = value
        .checked_add(1)
        .ok_or(StartingCityCensusClearError::CountOverflow)?;
    Ok(())
}

/// Execute only the source-separable clear prefix against the canonical Sim City owner.
///
/// Validation precedes staging and publication, so a stale City/Leader/center-Build join
/// cannot leak even one clear. The receipt deliberately does not claim the following Unit
/// walk, whose empty-action Citizen arm needs complete canonical setup receiver images.
pub fn apply_starting_city_census_clear(
    sim: &mut Sim,
) -> Result<StartingCityCensusClearReceipt, StartingCityCensusClearError> {
    check_sim_owned_cities(sim)?;

    let mut staged = sim.cities.clone();
    let mut receipt = StartingCityCensusClearReceipt {
        owners_walked: 0,
        slots_scanned: 0,
        cities_cleared: 0,
        city_pod_bytes_changed: 0,
        cities: Vec::new(),
        city_census_clear_complete: false,
        city_unit_census_complete: false,
        first_checksum_city_image_ready: false,
        world_writes: 0,
        build_or_registry_writes: 0,
        main_rng_draws: 0,
    };

    for owner in 0..NUM_PLAYERS {
        if !sim.leaders[owner].active {
            continue;
        }
        add_count(&mut receipt.owners_walked)?;
        // Canonical Cities validation proved the mark nonnegative and in bounds.
        let mark = staged.city_mark[owner] as usize;
        for slot in 0..mark {
            add_count(&mut receipt.slots_scanned)?;
            let city = &mut staged.slots[owner][slot];
            if !city.active() {
                continue;
            }
            let before = city.pod_bytes();
            city.gatherers = 0;
            city.busy = 0;
            city.free = 0;
            // The join proved `city.city == slot as i16`; using the owned field preserves
            // the retail 16-bit wrap of `lea esi,[edx+0x64]` followed by a word store.
            city.peasant_dist = initial_peasant_dist_for_slot(city.city);
            let after = city.pod_bytes();
            let changed = before
                .iter()
                .zip(after.iter())
                .filter(|(lhs, rhs)| lhs != rhs)
                .count() as u32;
            receipt.city_pod_bytes_changed = receipt
                .city_pod_bytes_changed
                .checked_add(changed)
                .ok_or(StartingCityCensusClearError::CountOverflow)?;
            add_count(&mut receipt.cities_cleared)?;
            receipt.cities.push(StartingCityCensusClearMutation {
                owner,
                slot,
                before,
                after,
            });
        }
    }

    receipt.city_census_clear_complete = true;
    sim.cities = staged;
    Ok(receipt)
}
