//! Exact frame-zero owner for `CityData::in_port +0x4e`.
//!
//! The first `Leader::plan_strategy` clears this signed short for every live City before
//! walking the Leader's Builds and Units.  The loop reads only the current Leader's
//! `city_mark`, City pointer array, and City active bit, so it is independent of the
//! unfinished procedural World and starting-Unit receiver images.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::systems::tech_cities::{CITY_POD_LEN, NUM_PLAYERS};
use don_sim::tick::Sim;

use crate::cities_runtime::{check_sim_owned_cities, CitiesRuntimeError};

pub const LEADER_PLAN_STRATEGY_VA: u32 = 0x006b_9620;
pub const CITY_IN_PORT_CLEAR_LOOP_VA: u32 = 0x006b_9df7;
pub const CITY_IN_PORT_CLEAR_STORE_VA: u32 = 0x006b_9e1f;
pub const CITY_IN_PORT_CLEAR_LOOP_END_VA: u32 = 0x006b_9e2c;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartingCityInPortMutation {
    pub owner: usize,
    pub slot: usize,
    pub before: [u8; CITY_POD_LEN],
    pub after: [u8; CITY_POD_LEN],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartingCityInPortReceipt {
    pub owners_walked: u32,
    pub slots_scanned: u32,
    pub cities_cleared: u32,
    pub city_pod_bytes_changed: u32,
    pub cities: Vec<StartingCityInPortMutation>,
    pub city_in_port_clear_complete: bool,
    /// Other `plan_strategy` City writers remain separately gated.
    pub first_checksum_city_image_ready: bool,
    pub world_writes: u32,
    pub build_or_registry_writes: u32,
    pub main_rng_draws: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartingCityInPortError {
    Cities(CitiesRuntimeError),
    CountOverflow,
}

impl fmt::Display for StartingCityInPortError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "starting City in-port clear refused: {self:?}")
    }
}

impl std::error::Error for StartingCityInPortError {}

impl From<CitiesRuntimeError> for StartingCityInPortError {
    fn from(value: CitiesRuntimeError) -> Self {
        Self::Cities(value)
    }
}

fn add_count(value: &mut u32) -> Result<(), StartingCityInPortError> {
    *value = value
        .checked_add(1)
        .ok_or(StartingCityInPortError::CountOverflow)?;
    Ok(())
}

/// Execute the frame-zero `in_port` clear over the canonical Sim-owned City pool.
///
/// Retail executes the loop once per active Leader.  This adapter composes those calls in
/// owner order.  The canonical Cities validation happens before staging, and the caller's
/// pool is replaced only after every counter and after-image is complete.
pub fn apply_starting_city_in_port_clear(
    sim: &mut Sim,
) -> Result<StartingCityInPortReceipt, StartingCityInPortError> {
    check_sim_owned_cities(sim)?;

    let mut staged = sim.cities.clone();
    let mut receipt = StartingCityInPortReceipt {
        owners_walked: 0,
        slots_scanned: 0,
        cities_cleared: 0,
        city_pod_bytes_changed: 0,
        cities: Vec::new(),
        city_in_port_clear_complete: false,
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
        // The canonical City join proved this mark nonnegative and in bounds.
        let mark = staged.city_mark[owner] as usize;
        for slot in 0..mark {
            add_count(&mut receipt.slots_scanned)?;
            let city = &mut staged.slots[owner][slot];
            if !city.active() {
                continue;
            }
            let before = city.pod_bytes();
            city.in_port = 0;
            let after = city.pod_bytes();
            let changed = before
                .iter()
                .zip(after.iter())
                .filter(|(lhs, rhs)| lhs != rhs)
                .count() as u32;
            receipt.city_pod_bytes_changed = receipt
                .city_pod_bytes_changed
                .checked_add(changed)
                .ok_or(StartingCityInPortError::CountOverflow)?;
            add_count(&mut receipt.cities_cleared)?;
            receipt.cities.push(StartingCityInPortMutation {
                owner,
                slot,
                before,
                after,
            });
        }
    }

    receipt.city_in_port_clear_complete = true;
    sim.cities = staged;
    Ok(receipt)
}
