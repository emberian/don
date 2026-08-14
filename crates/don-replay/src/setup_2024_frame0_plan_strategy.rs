// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact local prefix of the first golden frame-zero `Leader::plan_strategy` call.
//!
//! Step 11 reaches owner zero before any frame-zero Unit work.  The completed setup Sim is not
//! a legal call-entry image: steps 0 through 10 and owner zero's preceding `check_explore` call
//! have already run.  This module therefore requires an independently captured call-entry
//! authority and never derives it from the setup receipts.
//!
//! The bounded prefix owns the stores at `0x006b96f9..0x006b98cf`: the conditional six-resource
//! escrow-rate reset, the active-City scratch reset, and twenty-six named Leader planning
//! counters.  Planning is detached, so refusal cannot publish the native prefix's partial
//! writes.  The first remaining child is the call at `0x006b98d0` to
//! `LeaderData::get_team_terr` (`0x006d62e0`).  Although the simulator has a useful common-case
//! implementation, retail's complete function also reads Game team mode, Player rows, and the
//! full Leader table.  No golden call-entry authority currently joins that complete input
//! surface, so this module emits a typed request instead of assuming the common case.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::systems::unit_inctime::SUPPORTED_RETAIL_EXE_SHA256;

use crate::setup_2024_frame379::REPLAY_FILE_SHA256;
use crate::world_owner_frontier::sha256;

pub const PLAN_STRATEGY_VA: u32 = 0x006b_9620;
pub const PLAN_STRATEGY_BODY_VA: u32 = 0x006b_96f9;
pub const GET_TEAM_TERR_CALL_VA: u32 = 0x006b_98d0;
pub const GET_TEAM_TERR_VA: u32 = 0x006d_62e0;

pub const GOLDEN_FRAME: i32 = 0;
pub const GOLDEN_STEP: u8 = 11;
pub const GOLDEN_FIRST_STRATEGY_OWNER: u8 = 0;
pub const GOLDEN_FIRST_STRATEGY_ORDINAL: u8 = 0;
pub const GOLDEN_CENTER_CITY_SLOT: i16 = 0;
pub const GOLDEN_CENTER_BUILD_O: i16 = 2_000;
pub const RESOURCE_SLOTS: usize = 6;
pub const PLAN_ENTRY_SCRATCH_DWORDS: usize = 26;

const LEADER_ACTIVE_HUMAN_MASK: i32 = 0x7;
const CITY_ACTIVE_MASK: u16 = 0x1;

const ESCROW_RATE_STORE_VAS: [u32; RESOURCE_SLOTS] = [
    0x006b_970a,
    0x006b_9714,
    0x006b_971e,
    0x006b_9728,
    0x006b_9732,
    0x006b_973c,
];

const SCRATCH_STORE_VAS: [u32; PLAN_ENTRY_SCRATCH_DWORDS] = [
    0x006b_97cc,
    0x006b_97d6,
    0x006b_97e0,
    0x006b_97ea,
    0x006b_97f4,
    0x006b_97fe,
    0x006b_9808,
    0x006b_9812,
    0x006b_981c,
    0x006b_9826,
    0x006b_9830,
    0x006b_983a,
    0x006b_9844,
    0x006b_984e,
    0x006b_9858,
    0x006b_9862,
    0x006b_986c,
    0x006b_9876,
    0x006b_9880,
    0x006b_988a,
    0x006b_9894,
    0x006b_989e,
    0x006b_98a8,
    0x006b_98b2,
    0x006b_98bc,
    0x006b_98c6,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Frame0PlanStrategyEntrySource {
    /// Native supported retail captured owner zero immediately after its frame-zero
    /// `check_explore` return and immediately before `Leader::plan_strategy` entry.
    CompleteRetailOwnerZeroPlanStrategyEntry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Frame0PlanStrategyScratchField {
    Active = 0,
    Combat,
    Siege,
    NonSiege,
    SeaCombat,
    Defense,
    Attack,
    Naval,
    Air,
    Missile,
    Transports,
    Fishermen,
    IdleFishermen,
    Peasants,
    Scholars,
    Merchants,
    Fighters,
    Bombers,
    Cruise,
    Nuke,
    Caras,
    FreePeasants,
    XportPeasants,
    Gatherers,
    Attacked,
    FullCities,
}

pub const PLAN_ENTRY_SCRATCH_FIELDS: [Frame0PlanStrategyScratchField; PLAN_ENTRY_SCRATCH_DWORDS] = [
    Frame0PlanStrategyScratchField::Active,
    Frame0PlanStrategyScratchField::Combat,
    Frame0PlanStrategyScratchField::Siege,
    Frame0PlanStrategyScratchField::NonSiege,
    Frame0PlanStrategyScratchField::SeaCombat,
    Frame0PlanStrategyScratchField::Defense,
    Frame0PlanStrategyScratchField::Attack,
    Frame0PlanStrategyScratchField::Naval,
    Frame0PlanStrategyScratchField::Air,
    Frame0PlanStrategyScratchField::Missile,
    Frame0PlanStrategyScratchField::Transports,
    Frame0PlanStrategyScratchField::Fishermen,
    Frame0PlanStrategyScratchField::IdleFishermen,
    Frame0PlanStrategyScratchField::Peasants,
    Frame0PlanStrategyScratchField::Scholars,
    Frame0PlanStrategyScratchField::Merchants,
    Frame0PlanStrategyScratchField::Fighters,
    Frame0PlanStrategyScratchField::Bombers,
    Frame0PlanStrategyScratchField::Cruise,
    Frame0PlanStrategyScratchField::Nuke,
    Frame0PlanStrategyScratchField::Caras,
    Frame0PlanStrategyScratchField::FreePeasants,
    Frame0PlanStrategyScratchField::XportPeasants,
    Frame0PlanStrategyScratchField::Gatherers,
    Frame0PlanStrategyScratchField::Attacked,
    Frame0PlanStrategyScratchField::FullCities,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame0PlanStrategyCityImage {
    pub slot: i16,
    pub city_flags: u16,
    pub city: i16,
    pub center_o: i16,
    pub who: i8,
    pub peasant_dist: i16,
    pub free: u8,
    pub busy: u8,
    pub gatherers: u8,
}

impl Frame0PlanStrategyCityImage {
    pub const fn active(self) -> bool {
        self.city_flags & CITY_ACTIVE_MASK != 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0PlanStrategyLeaderImage {
    pub leader_flags: i32,
    pub who: i32,
    pub city_num: i32,
    pub village_num: i32,
    /// `LeaderData::city_mark +0x408`, the logical City pointer-array length walked here.
    pub city_mark: i32,
    pub escrow_rate: [i32; RESOURCE_SLOTS],
    /// Values in [`PLAN_ENTRY_SCRATCH_FIELDS`] order.
    pub scratch: [i32; PLAN_ENTRY_SCRATCH_DWORDS],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0PlanStrategyEntryCapture {
    pub revision: u64,
    pub source: Frame0PlanStrategyEntrySource,
    pub replay_file_sha256: [u8; 32],
    pub executable_sha256: [u8; 32],
    /// Exact final seven-Unit setup image. This is provenance only and is never relabelled as
    /// the later strategy call entry.
    pub completed_setup_sim_sha256: [u8; 32],
    pub setup_composition_digest: [u8; 32],
    /// Source digest covering frame-zero steps 0..10 and owner zero `check_explore`.
    pub preceding_chronology_digest: [u8; 32],
    /// Exact supported-retail Sim at the native `Leader::plan_strategy` call boundary.
    pub call_entry_sim_sha256: [u8; 32],
    pub native_trace_sha256: [u8; 32],
    pub frame: i32,
    pub step: u8,
    pub owner: u8,
    pub strategy_ordinal: u8,
    pub leader: Frame0PlanStrategyLeaderImage,
    /// Complete logical owner-zero City pointer-array image, in native index order.
    pub cities: Vec<Frame0PlanStrategyCityImage>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0PlanStrategyEntryAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub capture: Frame0PlanStrategyEntryCapture,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Frame0PlanStrategyCityField {
    Gatherers,
    Busy,
    Free,
    PeasantDist,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame0PlanStrategyWriteTarget {
    EscrowRate {
        resource: u8,
    },
    City {
        slot: i16,
        field: Frame0PlanStrategyCityField,
    },
    Scratch(Frame0PlanStrategyScratchField),
}

/// An instruction-ordered native store. Same-value writes are retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame0PlanStrategyScalarWrite {
    pub instruction_va: u32,
    pub target: Frame0PlanStrategyWriteTarget,
    pub before: i32,
    pub after: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Frame0GetTeamTerrInputSurface {
    /// The complete retail child needs the eight Leader rows plus the Game team-mode and
    /// Player-row projection used by `0x006d62e0`; the common-case simulator helper is not a
    /// substitute for this source-bound golden input.
    CompleteLeaderGameTeamAndPlayerProjection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0GetTeamTerrRequest {
    pub request_sha256: [u8; 32],
    pub parent_authority_digest: [u8; 32],
    pub local_prefix_digest: [u8; 32],
    pub call_entry_sim_sha256: [u8; 32],
    pub receiver_owner: u8,
    pub callsite_va: u32,
    pub callee_va: u32,
    pub input_surface: Frame0GetTeamTerrInputSurface,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0PlanStrategyPrefixPlan {
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub call_entry_sim_sha256: [u8; 32],
    pub leader_before: Frame0PlanStrategyLeaderImage,
    pub leader_after_local: Frame0PlanStrategyLeaderImage,
    pub cities_before: Vec<Frame0PlanStrategyCityImage>,
    pub cities_after_local: Vec<Frame0PlanStrategyCityImage>,
    pub writes: Vec<Frame0PlanStrategyScalarWrite>,
    pub local_prefix_digest: [u8; 32],
    pub open: Frame0GetTeamTerrRequest,
    /// Owner one cannot start until owner zero's complete `plan_strategy` call returns.
    pub next_strategy_owner_if_complete: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame0PlanStrategyError {
    MissingCaptureRevision,
    WrongCaptureSource,
    ReplayMismatch,
    UnsupportedExecutable,
    MissingSetupSnapshot,
    MissingSetupCompositionDigest,
    MissingPrecedingChronologyDigest,
    MissingCallEntrySnapshot,
    MissingNativeTrace,
    WrongFrame { expected: i32, actual: i32 },
    WrongStep { expected: u8, actual: u8 },
    WrongOwner { expected: u8, actual: u8 },
    WrongStrategyOrdinal { expected: u8, actual: u8 },
    OwnerNotActiveHuman,
    LeaderWhoMismatch,
    InvalidCityMark,
    CityCountMismatch,
    WrongGoldenCity,
    StaleAuthority,
}

impl fmt::Display for Frame0PlanStrategyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "2024 frame-zero plan_strategy prefix refused: {self:?}")
    }
}

impl std::error::Error for Frame0PlanStrategyError {}

fn append_city(image: &mut Vec<u8>, city: Frame0PlanStrategyCityImage) {
    image.extend_from_slice(&city.slot.to_le_bytes());
    image.extend_from_slice(&city.city_flags.to_le_bytes());
    image.extend_from_slice(&city.city.to_le_bytes());
    image.extend_from_slice(&city.center_o.to_le_bytes());
    image.push(city.who as u8);
    image.extend_from_slice(&city.peasant_dist.to_le_bytes());
    image.extend_from_slice(&[city.free, city.busy, city.gatherers]);
}

fn append_leader(image: &mut Vec<u8>, leader: &Frame0PlanStrategyLeaderImage) {
    for value in [
        leader.leader_flags,
        leader.who,
        leader.city_num,
        leader.village_num,
        leader.city_mark,
    ] {
        image.extend_from_slice(&value.to_le_bytes());
    }
    for value in leader.escrow_rate.into_iter().chain(leader.scratch) {
        image.extend_from_slice(&value.to_le_bytes());
    }
}

pub fn frame0_plan_strategy_entry_authority_digest(
    capture: &Frame0PlanStrategyEntryCapture,
) -> [u8; 32] {
    let mut image = b"don-2024-frame0-plan-strategy-entry-v1".to_vec();
    image.extend_from_slice(&capture.revision.to_le_bytes());
    image.push(capture.source as u8);
    image.extend_from_slice(&capture.replay_file_sha256);
    image.extend_from_slice(&capture.executable_sha256);
    image.extend_from_slice(&capture.completed_setup_sim_sha256);
    image.extend_from_slice(&capture.setup_composition_digest);
    image.extend_from_slice(&capture.preceding_chronology_digest);
    image.extend_from_slice(&capture.call_entry_sim_sha256);
    image.extend_from_slice(&capture.native_trace_sha256);
    image.extend_from_slice(&capture.frame.to_le_bytes());
    image.extend_from_slice(&[capture.step, capture.owner, capture.strategy_ordinal]);
    append_leader(&mut image, &capture.leader);
    image.extend_from_slice(&(capture.cities.len() as u64).to_le_bytes());
    for city in &capture.cities {
        append_city(&mut image, *city);
    }
    sha256(&image)
}

pub fn bind_golden_frame0_owner0_plan_strategy_entry(
    capture: Frame0PlanStrategyEntryCapture,
) -> Result<Frame0PlanStrategyEntryAuthority, Frame0PlanStrategyError> {
    if capture.revision == 0 {
        return Err(Frame0PlanStrategyError::MissingCaptureRevision);
    }
    if capture.source != Frame0PlanStrategyEntrySource::CompleteRetailOwnerZeroPlanStrategyEntry {
        return Err(Frame0PlanStrategyError::WrongCaptureSource);
    }
    if capture.replay_file_sha256 != REPLAY_FILE_SHA256 {
        return Err(Frame0PlanStrategyError::ReplayMismatch);
    }
    if capture.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256 {
        return Err(Frame0PlanStrategyError::UnsupportedExecutable);
    }
    if capture.completed_setup_sim_sha256 == [0; 32] {
        return Err(Frame0PlanStrategyError::MissingSetupSnapshot);
    }
    if capture.setup_composition_digest == [0; 32] {
        return Err(Frame0PlanStrategyError::MissingSetupCompositionDigest);
    }
    if capture.preceding_chronology_digest == [0; 32] {
        return Err(Frame0PlanStrategyError::MissingPrecedingChronologyDigest);
    }
    if capture.call_entry_sim_sha256 == [0; 32] {
        return Err(Frame0PlanStrategyError::MissingCallEntrySnapshot);
    }
    if capture.native_trace_sha256 == [0; 32] {
        return Err(Frame0PlanStrategyError::MissingNativeTrace);
    }
    if capture.frame != GOLDEN_FRAME {
        return Err(Frame0PlanStrategyError::WrongFrame {
            expected: GOLDEN_FRAME,
            actual: capture.frame,
        });
    }
    if capture.step != GOLDEN_STEP {
        return Err(Frame0PlanStrategyError::WrongStep {
            expected: GOLDEN_STEP,
            actual: capture.step,
        });
    }
    if capture.owner != GOLDEN_FIRST_STRATEGY_OWNER {
        return Err(Frame0PlanStrategyError::WrongOwner {
            expected: GOLDEN_FIRST_STRATEGY_OWNER,
            actual: capture.owner,
        });
    }
    if capture.strategy_ordinal != GOLDEN_FIRST_STRATEGY_ORDINAL {
        return Err(Frame0PlanStrategyError::WrongStrategyOrdinal {
            expected: GOLDEN_FIRST_STRATEGY_ORDINAL,
            actual: capture.strategy_ordinal,
        });
    }
    if capture.leader.leader_flags & LEADER_ACTIVE_HUMAN_MASK != LEADER_ACTIVE_HUMAN_MASK {
        return Err(Frame0PlanStrategyError::OwnerNotActiveHuman);
    }
    if capture.leader.who != i32::from(capture.owner) {
        return Err(Frame0PlanStrategyError::LeaderWhoMismatch);
    }
    let city_mark = usize::try_from(capture.leader.city_mark)
        .map_err(|_| Frame0PlanStrategyError::InvalidCityMark)?;
    if city_mark != capture.cities.len() {
        return Err(Frame0PlanStrategyError::CityCountMismatch);
    }
    // The supported owner-zero setup has one City record. The mandatory Market is linked to
    // that City and does not allocate a second City row.
    if capture.cities.len() != 1 {
        return Err(Frame0PlanStrategyError::WrongGoldenCity);
    }
    let city = capture.cities[0];
    if !city.active()
        || city.slot != GOLDEN_CENTER_CITY_SLOT
        || city.city != GOLDEN_CENTER_CITY_SLOT
        || city.center_o != GOLDEN_CENTER_BUILD_O
        || city.who != capture.owner as i8
    {
        return Err(Frame0PlanStrategyError::WrongGoldenCity);
    }

    let composition_digest = frame0_plan_strategy_entry_authority_digest(&capture);
    Ok(Frame0PlanStrategyEntryAuthority {
        revision: capture.revision,
        composition_digest,
        capture,
    })
}

fn append_write(image: &mut Vec<u8>, write: Frame0PlanStrategyScalarWrite) {
    image.extend_from_slice(&write.instruction_va.to_le_bytes());
    match write.target {
        Frame0PlanStrategyWriteTarget::EscrowRate { resource } => {
            image.extend_from_slice(&[0, resource]);
        }
        Frame0PlanStrategyWriteTarget::City { slot, field } => {
            image.push(1);
            image.extend_from_slice(&slot.to_le_bytes());
            image.push(field as u8);
        }
        Frame0PlanStrategyWriteTarget::Scratch(field) => {
            image.extend_from_slice(&[2, field as u8]);
        }
    }
    image.extend_from_slice(&write.before.to_le_bytes());
    image.extend_from_slice(&write.after.to_le_bytes());
}

fn local_prefix_digest(
    authority: &Frame0PlanStrategyEntryAuthority,
    leader: &Frame0PlanStrategyLeaderImage,
    cities: &[Frame0PlanStrategyCityImage],
    writes: &[Frame0PlanStrategyScalarWrite],
) -> [u8; 32] {
    let mut image = b"don-2024-frame0-plan-strategy-local-prefix-v1".to_vec();
    image.extend_from_slice(&authority.composition_digest);
    append_leader(&mut image, leader);
    image.extend_from_slice(&(cities.len() as u64).to_le_bytes());
    for city in cities {
        append_city(&mut image, *city);
    }
    image.extend_from_slice(&(writes.len() as u64).to_le_bytes());
    for write in writes {
        append_write(&mut image, *write);
    }
    sha256(&image)
}

fn get_team_terr_request_digest(
    authority: &Frame0PlanStrategyEntryAuthority,
    prefix_digest: [u8; 32],
) -> [u8; 32] {
    let mut image = b"don-2024-frame0-get-team-terr-request-v1".to_vec();
    image.extend_from_slice(&authority.composition_digest);
    image.extend_from_slice(&prefix_digest);
    image.extend_from_slice(&authority.capture.call_entry_sim_sha256);
    image.push(authority.capture.owner);
    image.extend_from_slice(&GET_TEAM_TERR_CALL_VA.to_le_bytes());
    image.extend_from_slice(&GET_TEAM_TERR_VA.to_le_bytes());
    image.push(Frame0GetTeamTerrInputSurface::CompleteLeaderGameTeamAndPlayerProjection as u8);
    sha256(&image)
}

/// Revalidate a detached child request before a source-owned `get_team_terr` adapter consumes
/// it. This deliberately validates only the parent-produced request; the child remains
/// responsible for binding its complete Game/Player/Leader input projection.
pub fn validate_frame0_get_team_terr_request(request: &Frame0GetTeamTerrRequest) -> bool {
    if request.parent_authority_digest == [0; 32]
        || request.local_prefix_digest == [0; 32]
        || request.call_entry_sim_sha256 == [0; 32]
        || request.receiver_owner != GOLDEN_FIRST_STRATEGY_OWNER
        || request.callsite_va != GET_TEAM_TERR_CALL_VA
        || request.callee_va != GET_TEAM_TERR_VA
        || request.input_surface
            != Frame0GetTeamTerrInputSurface::CompleteLeaderGameTeamAndPlayerProjection
    {
        return false;
    }
    let mut image = b"don-2024-frame0-get-team-terr-request-v1".to_vec();
    image.extend_from_slice(&request.parent_authority_digest);
    image.extend_from_slice(&request.local_prefix_digest);
    image.extend_from_slice(&request.call_entry_sim_sha256);
    image.push(request.receiver_owner);
    image.extend_from_slice(&request.callsite_va.to_le_bytes());
    image.extend_from_slice(&request.callee_va.to_le_bytes());
    image.push(request.input_surface as u8);
    request.request_sha256 == sha256(&image)
}

/// Plan owner zero's exact local prefix without publishing any of its partial native writes.
///
/// The returned candidate may not be installed until a later transaction closes
/// [`Frame0GetTeamTerrRequest`] and the remainder of the same native call. Consequently owner
/// one, Scout o0, and both Merchants remain unreachable from this receipt.
pub fn plan_golden_frame0_owner0_plan_strategy_prefix(
    authority: &Frame0PlanStrategyEntryAuthority,
) -> Result<Frame0PlanStrategyPrefixPlan, Frame0PlanStrategyError> {
    if authority.revision != authority.capture.revision
        || authority.composition_digest
            != frame0_plan_strategy_entry_authority_digest(&authority.capture)
    {
        return Err(Frame0PlanStrategyError::StaleAuthority);
    }

    let mut leader = authority.capture.leader.clone();
    let mut cities = authority.capture.cities.clone();
    let mut writes = Vec::with_capacity(RESOURCE_SLOTS + 4 + PLAN_ENTRY_SCRATCH_DWORDS);

    if leader.city_num.wrapping_add(leader.village_num) > 2 {
        for resource in 0..RESOURCE_SLOTS {
            writes.push(Frame0PlanStrategyScalarWrite {
                instruction_va: ESCROW_RATE_STORE_VAS[resource],
                target: Frame0PlanStrategyWriteTarget::EscrowRate {
                    resource: resource as u8,
                },
                before: leader.escrow_rate[resource],
                after: 40,
            });
            leader.escrow_rate[resource] = 40;
        }
    }

    for city in &mut cities {
        if !city.active() {
            continue;
        }
        let slot = city.slot;
        writes.push(Frame0PlanStrategyScalarWrite {
            instruction_va: 0x006b_976f,
            target: Frame0PlanStrategyWriteTarget::City {
                slot,
                field: Frame0PlanStrategyCityField::Gatherers,
            },
            before: i32::from(city.gatherers),
            after: 0,
        });
        city.gatherers = 0;
        writes.push(Frame0PlanStrategyScalarWrite {
            instruction_va: 0x006b_9789,
            target: Frame0PlanStrategyWriteTarget::City {
                slot,
                field: Frame0PlanStrategyCityField::Busy,
            },
            before: i32::from(city.busy),
            after: 0,
        });
        city.busy = 0;
        writes.push(Frame0PlanStrategyScalarWrite {
            instruction_va: 0x006b_97a3,
            target: Frame0PlanStrategyWriteTarget::City {
                slot,
                field: Frame0PlanStrategyCityField::Free,
            },
            before: i32::from(city.free),
            after: 0,
        });
        city.free = 0;
        let peasant_dist_after = slot.wrapping_add(100);
        writes.push(Frame0PlanStrategyScalarWrite {
            instruction_va: 0x006b_97bd,
            target: Frame0PlanStrategyWriteTarget::City {
                slot,
                field: Frame0PlanStrategyCityField::PeasantDist,
            },
            before: i32::from(city.peasant_dist),
            after: i32::from(peasant_dist_after),
        });
        city.peasant_dist = peasant_dist_after;
    }

    for index in 0..PLAN_ENTRY_SCRATCH_DWORDS {
        writes.push(Frame0PlanStrategyScalarWrite {
            instruction_va: SCRATCH_STORE_VAS[index],
            target: Frame0PlanStrategyWriteTarget::Scratch(PLAN_ENTRY_SCRATCH_FIELDS[index]),
            before: leader.scratch[index],
            after: 0,
        });
        leader.scratch[index] = 0;
    }

    let prefix_digest = local_prefix_digest(authority, &leader, &cities, &writes);
    let request = Frame0GetTeamTerrRequest {
        request_sha256: get_team_terr_request_digest(authority, prefix_digest),
        parent_authority_digest: authority.composition_digest,
        local_prefix_digest: prefix_digest,
        call_entry_sim_sha256: authority.capture.call_entry_sim_sha256,
        receiver_owner: authority.capture.owner,
        callsite_va: GET_TEAM_TERR_CALL_VA,
        callee_va: GET_TEAM_TERR_VA,
        input_surface: Frame0GetTeamTerrInputSurface::CompleteLeaderGameTeamAndPlayerProjection,
    };

    Ok(Frame0PlanStrategyPrefixPlan {
        authority_revision: authority.revision,
        authority_digest: authority.composition_digest,
        call_entry_sim_sha256: authority.capture.call_entry_sim_sha256,
        leader_before: authority.capture.leader.clone(),
        leader_after_local: leader,
        cities_before: authority.capture.cities.clone(),
        cities_after_local: cities,
        writes,
        local_prefix_digest: prefix_digest,
        open: request,
        next_strategy_owner_if_complete: 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(byte: u8) -> [u8; 32] {
        [byte; 32]
    }

    fn capture() -> Frame0PlanStrategyEntryCapture {
        Frame0PlanStrategyEntryCapture {
            revision: 7,
            source: Frame0PlanStrategyEntrySource::CompleteRetailOwnerZeroPlanStrategyEntry,
            replay_file_sha256: REPLAY_FILE_SHA256,
            executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
            completed_setup_sim_sha256: hash(1),
            setup_composition_digest: hash(2),
            preceding_chronology_digest: hash(3),
            call_entry_sim_sha256: hash(4),
            native_trace_sha256: hash(5),
            frame: GOLDEN_FRAME,
            step: GOLDEN_STEP,
            owner: GOLDEN_FIRST_STRATEGY_OWNER,
            strategy_ordinal: GOLDEN_FIRST_STRATEGY_ORDINAL,
            leader: Frame0PlanStrategyLeaderImage {
                leader_flags: LEADER_ACTIVE_HUMAN_MASK,
                who: 0,
                city_num: 2,
                village_num: 1,
                city_mark: 1,
                escrow_rate: [1, 2, 3, 4, 5, 6],
                scratch: std::array::from_fn(|index| index as i32 + 1),
            },
            cities: vec![Frame0PlanStrategyCityImage {
                slot: GOLDEN_CENTER_CITY_SLOT,
                city_flags: CITY_ACTIVE_MASK,
                city: GOLDEN_CENTER_CITY_SLOT,
                center_o: GOLDEN_CENTER_BUILD_O,
                who: GOLDEN_FIRST_STRATEGY_OWNER as i8,
                peasant_dist: -1,
                free: 3,
                busy: 4,
                gatherers: 5,
            }],
        }
    }

    #[test]
    fn golden_owner_zero_stops_at_exact_team_territory_child() {
        let capture = capture();
        let unchanged = capture.clone();
        let authority = bind_golden_frame0_owner0_plan_strategy_entry(capture).unwrap();
        let plan = plan_golden_frame0_owner0_plan_strategy_prefix(&authority).unwrap();

        assert_eq!(
            authority.capture, unchanged,
            "planning mutated its authority"
        );
        assert_eq!(
            plan.writes.len(),
            RESOURCE_SLOTS + 4 + PLAN_ENTRY_SCRATCH_DWORDS
        );
        assert_eq!(plan.leader_after_local.escrow_rate, [40; RESOURCE_SLOTS]);
        assert_eq!(
            plan.leader_after_local.scratch,
            [0; PLAN_ENTRY_SCRATCH_DWORDS]
        );
        assert_eq!(plan.cities_after_local[0].gatherers, 0);
        assert_eq!(plan.cities_after_local[0].busy, 0);
        assert_eq!(plan.cities_after_local[0].free, 0);
        assert_eq!(plan.cities_after_local[0].peasant_dist, 100);
        assert_eq!(plan.writes[0].instruction_va, 0x006b_970a);
        assert_eq!(plan.writes[RESOURCE_SLOTS].instruction_va, 0x006b_976f);
        assert_eq!(plan.writes[RESOURCE_SLOTS + 4].instruction_va, 0x006b_97cc);
        assert_eq!(plan.writes.last().unwrap().instruction_va, 0x006b_98c6);
        assert_eq!(plan.open.callsite_va, GET_TEAM_TERR_CALL_VA);
        assert_eq!(plan.open.callee_va, GET_TEAM_TERR_VA);
        assert_eq!(plan.open.receiver_owner, 0);
        assert_ne!(plan.open.request_sha256, [0; 32]);
        assert_eq!(plan.next_strategy_owner_if_complete, 1);
    }

    #[test]
    fn retail_threshold_does_not_rewrite_escrow_rate_at_two() {
        let mut capture = capture();
        capture.leader.city_num = 1;
        capture.leader.village_num = 1;
        let authority = bind_golden_frame0_owner0_plan_strategy_entry(capture).unwrap();
        let plan = plan_golden_frame0_owner0_plan_strategy_prefix(&authority).unwrap();

        assert_eq!(plan.writes.len(), 4 + PLAN_ENTRY_SCRATCH_DWORDS);
        assert_eq!(plan.leader_after_local.escrow_rate, [1, 2, 3, 4, 5, 6]);
        assert_eq!(plan.writes[0].instruction_va, 0x006b_976f);
    }

    #[test]
    fn owner_one_cannot_jump_over_owner_zero_child() {
        let mut capture = capture();
        capture.owner = 1;
        capture.strategy_ordinal = 1;
        capture.leader.who = 1;
        capture.cities[0].who = 1;
        assert_eq!(
            bind_golden_frame0_owner0_plan_strategy_entry(capture).unwrap_err(),
            Frame0PlanStrategyError::WrongOwner {
                expected: 0,
                actual: 1,
            }
        );
    }

    #[test]
    fn setup_image_cannot_be_relabelled_as_missing_call_entry_authority() {
        let mut capture = capture();
        capture.preceding_chronology_digest = [0; 32];
        assert_eq!(
            bind_golden_frame0_owner0_plan_strategy_entry(capture).unwrap_err(),
            Frame0PlanStrategyError::MissingPrecedingChronologyDigest
        );
    }

    #[test]
    fn authority_and_city_mutations_bite() {
        let authority = bind_golden_frame0_owner0_plan_strategy_entry(capture()).unwrap();
        let mut stale = authority.clone();
        stale.capture.leader.scratch[0] ^= 1;
        assert_eq!(
            plan_golden_frame0_owner0_plan_strategy_prefix(&stale).unwrap_err(),
            Frame0PlanStrategyError::StaleAuthority
        );

        let mut wrong_city = capture();
        wrong_city.cities[0].center_o = 2_001;
        assert_eq!(
            bind_golden_frame0_owner0_plan_strategy_entry(wrong_city).unwrap_err(),
            Frame0PlanStrategyError::WrongGoldenCity
        );
    }
}
