//! First canonical `Sim` owner used by the `Cities::capture_city` braid.
//!
//! `Sim` now owns the existing checksum-authentic [`CityPool`]. This module joins that
//! pool to the live Build band and victory/diplomacy leader owner for the nested
//! `City::capture` call at `0x00736C40`. It introduces no parallel city, build, leader, or
//! resource image.

use super::cities_capture_prefix::{CaptureCounter, IncrementCaptureCounterRequest};
use super::cities_capture_swap_fork::{
    CityCaptureEcxResidue, CityRecordCaptureReceipt, CityRecordCaptureRequest,
};
use super::damage_world::ObjectKey;
use crate::objects::{Band, BUILD_BAND_BASE};
use crate::systems::tech_cities;
use crate::tick::Sim;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimCityRecordCaptureError {
    InvalidOwner,
    InvalidObjectIdentity,
    MissingBuild,
    BuildOwnerMismatch,
    CityIdentityMismatch,
    OldCityInactive,
    NewCitySlotNotReserved,
    NewCenterCityMismatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimCaptureCounterError {
    InvalidOwner,
}

/// Apply the first checksum-visible prefix mutation whose owner was previously absent.
///
/// The PDB fixes these fields at `LeaderData +0x824/+0x828`; retail emits `inc dword`
/// at `0x0073353F`/`0x00733530`, so debug and release hosts must both preserve wrapping.
pub fn apply_sim_capture_counter(
    sim: &mut Sim,
    request: IncrementCaptureCounterRequest,
) -> Result<(), SimCaptureCounterError> {
    let leader = sim
        .vic_leaders
        .slots
        .get_mut(usize::from(request.who))
        .ok_or(SimCaptureCounterError::InvalidOwner)?;
    let counter = match request.counter {
        CaptureCounter::CitiesCaptured => &mut leader.cities_captured,
        CaptureCounter::CitiesLost => &mut leader.cities_lost,
    };
    *counter = counter.wrapping_add(request.amount);
    Ok(())
}

fn build_row(sim: &Sim, key: ObjectKey) -> Result<usize, SimCityRecordCaptureError> {
    let owner = usize::from(key.who);
    if owner >= tech_cities::NUM_PLAYERS {
        return Err(SimCityRecordCaptureError::InvalidOwner);
    }
    let object = u32::try_from(key.o)
        .ok()
        .and_then(|object| object.checked_sub(BUILD_BAND_BASE))
        .ok_or(SimCityRecordCaptureError::InvalidObjectIdentity)?;
    sim.world
        .objects
        .slot(owner)
        .band(Band::Build)
        .get(object as usize)
        .copied()
        .map(|row| row as usize)
        .ok_or(SimCityRecordCaptureError::MissingBuild)
}

/// Commit the checksum-visible part of `City::capture` against the live `Sim` stores.
///
/// The return-site ECX residue is not guessed: `City::capture` ends by calling the new
/// center's `hits(0)`, storing `hits - 10` as damage, and returning with ECX still holding
/// that same value (`0x00737014..0x0073702C`). `Cities::capture_city` immediately pushes
/// it as the ignored fifth argument to `Armies::update_city`.
pub fn apply_sim_city_record_capture(
    sim: &mut Sim,
    request: CityRecordCaptureRequest,
) -> Result<CityRecordCaptureReceipt, SimCityRecordCaptureError> {
    let old_owner = usize::from(request.old_city.who);
    let new_owner = usize::from(request.new_city.who);
    if old_owner >= tech_cities::NUM_PLAYERS || new_owner >= tech_cities::NUM_PLAYERS {
        return Err(SimCityRecordCaptureError::InvalidOwner);
    }

    let old_row = build_row(sim, request.old_center)?;
    let new_row = build_row(sim, request.new_center)?;
    let old_build = sim
        .builds
        .get(old_row)
        .ok_or(SimCityRecordCaptureError::MissingBuild)?;
    let new_build = sim
        .builds
        .get(new_row)
        .ok_or(SimCityRecordCaptureError::MissingBuild)?;
    if old_build.who != request.old_center.who || new_build.who != request.new_center.who {
        return Err(SimCityRecordCaptureError::BuildOwnerMismatch);
    }

    let old_index = usize::try_from(request.old_city.city)
        .map_err(|_| SimCityRecordCaptureError::CityIdentityMismatch)?;
    let new_index = usize::try_from(request.new_city.city)
        .map_err(|_| SimCityRecordCaptureError::CityIdentityMismatch)?;
    let old_city = sim
        .cities
        .slots
        .get(old_owner)
        .and_then(|slots| slots.get(old_index))
        .cloned()
        .ok_or(SimCityRecordCaptureError::CityIdentityMismatch)?;
    if !old_city.active() {
        return Err(SimCityRecordCaptureError::OldCityInactive);
    }
    if old_city.city != request.old_city.city
        || old_city.who != request.old_city.who as i8
        || old_city.o != request.old_center.o as i16
    {
        return Err(SimCityRecordCaptureError::CityIdentityMismatch);
    }
    if sim.cities.city_mark[new_owner] <= i32::from(request.new_city.city)
        || sim.cities.slots[new_owner].get(new_index).is_none()
    {
        return Err(SimCityRecordCaptureError::NewCitySlotNotReserved);
    }
    if new_build.city != request.new_city.city {
        return Err(SimCityRecordCaptureError::NewCenterCityMismatch);
    }

    let (new_x, new_y) = new_build.position();
    let same_player = old_owner == new_owner;
    let mutual_allied = sim.vic_leaders.slots[old_owner].diplos[new_owner] == 2
        && sim.vic_leaders.slots[new_owner].diplos[old_owner] == 2;
    let context = tech_cities::CaptureContext {
        same_player,
        mutual_allied,
        leader_flag_200000: sim.vic_leaders.slots[new_owner].leader_flags & 0x20_0000 != 0,
        has_preq_2b9: sim.vic_leaders.slots[new_owner].has_preq_2b9,
    };
    let captured = tech_cities::capture_city(
        &old_city,
        request.new_city.city,
        request.new_city.who as i8,
        request.new_center.o as i16,
        new_x,
        new_y,
        sim.world.frame,
        context,
    );

    let center_hits = sim.builds[new_row].construct_hits;
    let captured_damage = tech_cities::hits_after_capture(center_hits);
    sim.cities.slots[new_owner][new_index] = captured;
    sim.builds[new_row].damage = captured_damage;

    Ok(CityRecordCaptureReceipt {
        request,
        capture_applied: true,
        ignored_ecx_residue: CityCaptureEcxResidue(captured_damage),
    })
}
