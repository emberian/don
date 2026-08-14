//! Exact regional-visibility continuation for the golden frame-one Citizen.
//!
//! `WorldData::was_seen` reaches `LeaderData::reg_forts[region]` only after the live City
//! registry has proved `reg_cities[region] == 0`.  The canonical Sim does not carry the two
//! compact regional arrays, so this module joins their exact frame-zero setup census to the
//! complete frame-one owner-zero Build band.  The band must still contain exactly the admitted
//! Village and Dutch starting Market; any allocation, tombstone, replacement, or type drift
//! refuses the join.  The resulting zero Fort read is fed back into the read-only goody scan.
//!
//! No Unit, Leader, Group, map, or RNG owner is published.  The parent Leader pending write and
//! outer `unit_masks2 |= 0x8000` restore remain armed in the detached SetIdle transaction.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::objects::{Band, BUILD_BAND_BASE};
use don_sim::systems::sparse_object_bands_authority_frontier::{
    RetailBand, RetailObjectAddress, SparseSlotLifecycle,
};
use don_sim::tick::Sim;
use don_sim::world::WorldObjectIdentity;

use crate::leaders_deferred_history_frontier::REG_BUILDING_REGIONS;
use crate::leaders_setup_build_registry_frontier::{
    derive_frame_zero_build_registry_census, FrameZeroBuildRegistryError,
};
use crate::replay::Replay;
use crate::setup_2024_frame1_citizen_find_goody::{
    frame1_citizen_region_seen_request_digest,
    plan_frame1_citizen_find_goody_with_regional_authority,
    validate_frame1_citizen_find_goody_plan, Frame1CitizenFindGoodyError,
    Frame1CitizenFindGoodyOpenRequest, Frame1CitizenFindGoodyPlan,
    Frame1CitizenRegionSeenAuthority, Frame1CitizenRegionSeenRequest, WORLD_WAS_SEEN_VA,
};
use crate::setup_2024_frame379::Frame379SetupEntryReceipt;
use crate::setup_2024_golden_capture::Frame1PostCommandAuthority;
use crate::setup_2024_starting_market::{DUTCH_STARTING_MARKET_O, DUTCH_STARTING_MARKET_TYPE};
use crate::setup_cities_builds::{StartingSetupState, CITY_CENTER_TYPE};
use crate::world_owner_frontier::sha256;
use don_sim::systems::setup_idle_prefix::GoldenFrame1EntryAuthority;

pub const REG_FORTS_FIELD_OFFSET: usize = 0x12de;
pub const GOLDEN_OWNER: u8 = 0;
pub const PROOF_DOCUMENT: &str = "docs/assembly/replay-2024-frame1-citizen-region-seen.md";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1CitizenRegionSeenContinuation {
    pub composition_digest: [u8; 32],
    pub parent: Frame1CitizenFindGoodyPlan,
    pub authority: Frame1CitizenRegionSeenAuthority,
    pub resumed: Frame1CitizenFindGoodyPlan,
    pub restore_mask2_bit8000: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame1CitizenRegionSeenError {
    Parent(Frame1CitizenFindGoodyError),
    Census(FrameZeroBuildRegistryError),
    NotRegionalRequest,
    StaleRequest,
    UnsupportedGoldenOwner { player: u8, territory_owner: u8 },
    InvalidRegion { region: i16 },
    SetupReplayMismatch,
    MissingStartingCenter,
    StartingCenterMismatch,
    RegionalCensusMismatch { reg_cities: u16, reg_forts: u16 },
    BuildBandMismatch,
    BuildIdentityMismatch { object: i32 },
    RestoreNotArmed,
    RequestNotConsumed,
    StaleAuthority,
    StaleContinuation,
}

impl fmt::Display for Frame1CitizenRegionSeenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "2024 frame-1 Citizen regional visibility refused: {self:?}"
        )
    }
}

impl std::error::Error for Frame1CitizenRegionSeenError {}

impl From<Frame1CitizenFindGoodyError> for Frame1CitizenRegionSeenError {
    fn from(value: Frame1CitizenFindGoodyError) -> Self {
        Self::Parent(value)
    }
}

impl From<FrameZeroBuildRegistryError> for Frame1CitizenRegionSeenError {
    fn from(value: FrameZeroBuildRegistryError) -> Self {
        Self::Census(value)
    }
}

pub fn frame1_citizen_region_seen_authority_digest(
    authority: &Frame1CitizenRegionSeenAuthority,
) -> [u8; 32] {
    let mut image = b"don-frame1-citizen-region-seen-authority-v1".to_vec();
    image.extend_from_slice(&authority.request_sha256);
    image.extend_from_slice(&authority.replay_payload_sha256);
    image.extend_from_slice(&authority.post_command_sim_sha256);
    image.extend_from_slice(&authority.set_anim_return_sim_sha256);
    image.push(authority.territory_owner);
    image.extend_from_slice(&authority.region.to_le_bytes());
    image.extend_from_slice(&authority.reg_cities.to_le_bytes());
    image.extend_from_slice(&authority.reg_forts.to_le_bytes());
    image.extend_from_slice(&authority.build_mark.to_le_bytes());
    image.extend_from_slice(&(authority.center_build_row as u64).to_le_bytes());
    image.extend_from_slice(&authority.center_build_o.to_le_bytes());
    image.extend_from_slice(&authority.center_build_uid.to_le_bytes());
    image.extend_from_slice(&authority.center_build_type.to_le_bytes());
    image.extend_from_slice(&authority.center_region.to_le_bytes());
    image.extend_from_slice(&(authority.market_build_row as u64).to_le_bytes());
    image.extend_from_slice(&authority.market_build_o.to_le_bytes());
    image.extend_from_slice(&authority.market_build_uid.to_le_bytes());
    image.extend_from_slice(&authority.market_build_type.to_le_bytes());
    image.push(u8::from(authority.restore_mask2_bit8000));
    sha256(&image)
}

fn continuation_digest(continuation: &Frame1CitizenRegionSeenContinuation) -> [u8; 32] {
    let mut image = b"don-frame1-citizen-region-seen-continuation-v1".to_vec();
    image.extend_from_slice(&continuation.parent.composition_digest);
    image.extend_from_slice(&continuation.authority.authority_sha256);
    image.extend_from_slice(&continuation.resumed.composition_digest);
    image.push(u8::from(continuation.restore_mask2_bit8000));
    sha256(&image)
}

fn regional_request(
    plan: &Frame1CitizenFindGoodyPlan,
) -> Result<&Frame1CitizenRegionSeenRequest, Frame1CitizenRegionSeenError> {
    let Frame1CitizenFindGoodyOpenRequest::RegionSeen(request) = &plan.open else {
        return Err(Frame1CitizenRegionSeenError::NotRegionalRequest);
    };
    if request.request_sha256 != frame1_citizen_region_seen_request_digest(request) {
        return Err(Frame1CitizenRegionSeenError::StaleRequest);
    }
    if request.body_va != WORLD_WAS_SEEN_VA
        || request.needed_field_offset != REG_FORTS_FIELD_OFFSET
        || request.reg_cities != 0
    {
        return Err(Frame1CitizenRegionSeenError::StaleRequest);
    }
    if !request.restore_mask2_bit8000
        || !plan.restore_mask2_bit8000
        || plan.after_local.unit_masks2 & 0x8000 != 0
    {
        return Err(Frame1CitizenRegionSeenError::RestoreNotArmed);
    }
    Ok(request)
}

fn build_at(sim: &Sim, object: i32) -> Result<(usize, u16, i32), Frame1CitizenRegionSeenError> {
    let address = RetailObjectAddress::new(GOLDEN_OWNER, RetailBand::Build, object);
    let slot = sim
        .world
        .object_bands()
        .slot(address)
        .ok_or(Frame1CitizenRegionSeenError::BuildIdentityMismatch { object })?;
    let SparseSlotLifecycle::Live(WorldObjectIdentity::BuildRow(row)) = slot.lifecycle else {
        return Err(Frame1CitizenRegionSeenError::BuildIdentityMismatch { object });
    };
    let row = row as usize;
    let build = sim
        .builds
        .get(row)
        .ok_or(Frame1CitizenRegionSeenError::BuildIdentityMismatch { object })?;
    let type_index = sim
        .production_runtime
        .build_types
        .get(row)
        .and_then(|value| *value)
        .ok_or(Frame1CitizenRegionSeenError::BuildIdentityMismatch { object })?;
    if build.who != GOLDEN_OWNER || i32::from(build.object_id()) != object {
        return Err(Frame1CitizenRegionSeenError::BuildIdentityMismatch { object });
    }
    Ok((row, build.uid, type_index))
}

#[allow(clippy::too_many_arguments)]
pub fn bind_frame1_citizen_region_seen_authority(
    replay: &Replay,
    starting_setup: &StartingSetupState,
    setup_entry: &Frame379SetupEntryReceipt,
    post_authority: &Frame1PostCommandAuthority,
    post_command: &Sim,
    set_anim_return: &Sim,
    entry_authority: &GoldenFrame1EntryAuthority,
    plan: &Frame1CitizenFindGoodyPlan,
) -> Result<Frame1CitizenRegionSeenAuthority, Frame1CitizenRegionSeenError> {
    validate_frame1_citizen_find_goody_plan(
        replay,
        setup_entry,
        post_authority,
        post_command,
        set_anim_return,
        entry_authority,
        plan,
    )?;
    let request = regional_request(plan)?;
    if request.player != GOLDEN_OWNER || request.territory_owner != GOLDEN_OWNER {
        return Err(Frame1CitizenRegionSeenError::UnsupportedGoldenOwner {
            player: request.player,
            territory_owner: request.territory_owner,
        });
    }
    let region = usize::try_from(request.region)
        .ok()
        .filter(|region| *region < REG_BUILDING_REGIONS)
        .ok_or(Frame1CitizenRegionSeenError::InvalidRegion {
            region: request.region,
        })?;
    if starting_setup.receipt.replay_payload_sha256 != replay.initial.payload_sha256 {
        return Err(Frame1CitizenRegionSeenError::SetupReplayMismatch);
    }
    let center = starting_setup
        .receipt
        .cities
        .iter()
        .find(|city| city.owner == GOLDEN_OWNER)
        .ok_or(Frame1CitizenRegionSeenError::MissingStartingCenter)?;
    if i32::from(center.build.object_id) != setup_entry.center_build_o
        || center.build.type_index != CITY_CENTER_TYPE
        || center.region != setup_entry.center_region
    {
        return Err(Frame1CitizenRegionSeenError::StartingCenterMismatch);
    }

    let census = derive_frame_zero_build_registry_census(starting_setup, replay)?;
    let regional = census
        .regional_registries(usize::from(GOLDEN_OWNER))
        .ok_or(Frame1CitizenRegionSeenError::MissingStartingCenter)?;
    let reg_cities = regional[region];
    let reg_forts = regional[REG_BUILDING_REGIONS + region];
    if reg_cities != request.reg_cities || reg_forts != 0 {
        return Err(Frame1CitizenRegionSeenError::RegionalCensusMismatch {
            reg_cities,
            reg_forts,
        });
    }

    let expected_mark = DUTCH_STARTING_MARKET_O + 1;
    if !post_command.world.object_bands_are_dense_equivalent()
        || post_command
            .world
            .object_bands()
            .mark(usize::from(GOLDEN_OWNER), RetailBand::Build)
            != Some(expected_mark)
        || post_command
            .world
            .objects
            .slot(usize::from(GOLDEN_OWNER))
            .mark(Band::Build)
            != expected_mark as u32
        || post_command
            .world
            .object_bands()
            .retained_slots(usize::from(GOLDEN_OWNER), RetailBand::Build)
            != Some((expected_mark - BUILD_BAND_BASE as i32) as usize)
    {
        return Err(Frame1CitizenRegionSeenError::BuildBandMismatch);
    }
    let (center_row, center_uid, center_type) = build_at(post_command, BUILD_BAND_BASE as i32)?;
    let (market_row, market_uid, market_type) = build_at(post_command, DUTCH_STARTING_MARKET_O)?;
    let inventory = &post_authority.starting_builds;
    if center_row != inventory.center_build_row
        || BUILD_BAND_BASE as i32 != inventory.center_build_o
        || center_uid != inventory.center_uid
        || center_type != inventory.center_type
        || center_type != CITY_CENTER_TYPE
        || market_row != inventory.market_build_row
        || DUTCH_STARTING_MARKET_O != inventory.market_build_o
        || market_uid != inventory.market_uid
        || market_type != inventory.market_type
        || market_type != DUTCH_STARTING_MARKET_TYPE
    {
        return Err(Frame1CitizenRegionSeenError::BuildBandMismatch);
    }
    if !set_anim_return.world.object_bands_are_dense_equivalent()
        || set_anim_return
            .world
            .object_bands()
            .mark(usize::from(GOLDEN_OWNER), RetailBand::Build)
            != Some(expected_mark)
        || set_anim_return
            .world
            .objects
            .slot(usize::from(GOLDEN_OWNER))
            .mark(Band::Build)
            != expected_mark as u32
        || set_anim_return
            .world
            .object_bands()
            .retained_slots(usize::from(GOLDEN_OWNER), RetailBand::Build)
            != Some((expected_mark - BUILD_BAND_BASE as i32) as usize)
        || build_at(set_anim_return, BUILD_BAND_BASE as i32)?
            != (center_row, center_uid, center_type)
        || build_at(set_anim_return, DUTCH_STARTING_MARKET_O)?
            != (market_row, market_uid, market_type)
    {
        return Err(Frame1CitizenRegionSeenError::BuildBandMismatch);
    }

    let mut authority = Frame1CitizenRegionSeenAuthority {
        authority_sha256: [0; 32],
        request_sha256: request.request_sha256,
        replay_payload_sha256: starting_setup.receipt.replay_payload_sha256,
        post_command_sim_sha256: post_authority.post_command_sim_sha256,
        set_anim_return_sim_sha256: plan.source_sim_sha256,
        territory_owner: request.territory_owner,
        region: request.region,
        reg_cities,
        reg_forts,
        build_mark: expected_mark,
        center_build_row: center_row,
        center_build_o: BUILD_BAND_BASE as i16,
        center_build_uid: center_uid,
        center_build_type: center_type,
        center_region: center.region,
        market_build_row: market_row,
        market_build_o: DUTCH_STARTING_MARKET_O as i16,
        market_build_uid: market_uid,
        market_build_type: market_type,
        restore_mask2_bit8000: true,
    };
    authority.authority_sha256 = frame1_citizen_region_seen_authority_digest(&authority);
    Ok(authority)
}

#[allow(clippy::too_many_arguments)]
pub fn continue_frame1_citizen_region_seen(
    replay: &Replay,
    starting_setup: &StartingSetupState,
    setup_entry: &Frame379SetupEntryReceipt,
    post_authority: &Frame1PostCommandAuthority,
    post_command: &Sim,
    set_anim_return: &Sim,
    entry_authority: &GoldenFrame1EntryAuthority,
    plan: &Frame1CitizenFindGoodyPlan,
) -> Result<Frame1CitizenRegionSeenContinuation, Frame1CitizenRegionSeenError> {
    let authority = bind_frame1_citizen_region_seen_authority(
        replay,
        starting_setup,
        setup_entry,
        post_authority,
        post_command,
        set_anim_return,
        entry_authority,
        plan,
    )?;
    let resumed = plan_frame1_citizen_find_goody_with_regional_authority(
        replay,
        setup_entry,
        post_authority,
        post_command,
        set_anim_return,
        entry_authority,
        &plan.set_idle,
        Some(&authority),
    )?;
    if matches!(
        &resumed.open,
        Frame1CitizenFindGoodyOpenRequest::RegionSeen(request)
            if request.request_sha256 == authority.request_sha256
    ) {
        return Err(Frame1CitizenRegionSeenError::RequestNotConsumed);
    }
    if !resumed.restore_mask2_bit8000
        || resumed.after_local != plan.after_local
        || resumed.source_sim_sha256 != plan.source_sim_sha256
    {
        return Err(Frame1CitizenRegionSeenError::RestoreNotArmed);
    }
    let mut continuation = Frame1CitizenRegionSeenContinuation {
        composition_digest: [0; 32],
        parent: plan.clone(),
        authority,
        resumed,
        restore_mask2_bit8000: true,
    };
    continuation.composition_digest = continuation_digest(&continuation);
    Ok(continuation)
}

#[allow(clippy::too_many_arguments)]
pub fn validate_frame1_citizen_region_seen_continuation(
    replay: &Replay,
    starting_setup: &StartingSetupState,
    setup_entry: &Frame379SetupEntryReceipt,
    post_authority: &Frame1PostCommandAuthority,
    post_command: &Sim,
    set_anim_return: &Sim,
    entry_authority: &GoldenFrame1EntryAuthority,
    continuation: &Frame1CitizenRegionSeenContinuation,
) -> Result<(), Frame1CitizenRegionSeenError> {
    let expected = continue_frame1_citizen_region_seen(
        replay,
        starting_setup,
        setup_entry,
        post_authority,
        post_command,
        set_anim_return,
        entry_authority,
        &continuation.parent,
    )?;
    if expected != *continuation
        || continuation.composition_digest != continuation_digest(continuation)
        || continuation.authority.authority_sha256
            != frame1_citizen_region_seen_authority_digest(&continuation.authority)
    {
        return Err(Frame1CitizenRegionSeenError::StaleContinuation);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_2024_frame1_citizen_region_seen_exact_offsets() {
        assert_eq!(WORLD_WAS_SEEN_VA, 0x006b_53f0);
        assert_eq!(REG_FORTS_FIELD_OFFSET, 0x12de);
        assert_eq!(REG_BUILDING_REGIONS, 64);
        assert_eq!(DUTCH_STARTING_MARKET_O, 2001);
    }

    #[test]
    fn setup_2024_frame1_citizen_region_seen_authority_digest_is_mutation_sensitive() {
        let mut authority = Frame1CitizenRegionSeenAuthority {
            authority_sha256: [0; 32],
            request_sha256: [1; 32],
            replay_payload_sha256: [2; 32],
            post_command_sim_sha256: [3; 32],
            set_anim_return_sim_sha256: [4; 32],
            territory_owner: 0,
            region: 4,
            reg_cities: 0,
            reg_forts: 0,
            build_mark: 2002,
            center_build_row: 0,
            center_build_o: 2000,
            center_build_uid: 11,
            center_build_type: CITY_CENTER_TYPE,
            center_region: 1,
            market_build_row: 1,
            market_build_o: 2001,
            market_build_uid: 12,
            market_build_type: DUTCH_STARTING_MARKET_TYPE,
            restore_mask2_bit8000: true,
        };
        authority.authority_sha256 = frame1_citizen_region_seen_authority_digest(&authority);
        let digest = authority.authority_sha256;
        authority.region += 1;
        assert_ne!(
            frame1_citizen_region_seen_authority_digest(&authority),
            digest
        );
    }
}
