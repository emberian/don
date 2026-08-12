//! Replay-pinned authority discovery for the first 2018 opcode-25 Farm transaction.
//!
//! The recording carries the exact command, checksum tuple, setup selectors, static Rules,
//! and first-camera City center. It does not carry the generated World or the post-worldgen
//! RNG state from which `Setup::place_unit` created object 4. This adapter advances every
//! source-owned stage and stops at that first absent state rather than manufacturing a Unit,
//! Build body, blocked-site result, or swarm search after-image.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::systems::map_terrain::{Coord, WCoord};
use don_sim::systems::production::Footprint;

use crate::groups_build_history::{
    strict_group_build_history_before, GroupBuildHistoryError, ReplayGroupBuildPackage,
};
use crate::groups_channel::InitialGroupsChannel;
use crate::groups_pre_pair_unit_authority::{
    replay_build_type_facts, replay_tribe_type_facts, replay_unit_type_facts,
    PrePairUnitAuthorityError, ReplayBuildTypeFacts, ReplayTribeTypeFacts, ReplayUnitTypeFacts,
};
use crate::replay::{load_payload, Replay};
use crate::setup_cities_builds::{CAMERA_COMMAND_OPCODE, VILLAGE_CENTER_OFFSET, WORLD_TO_COORD};
use crate::setup_units_producer::{
    starting_citizen_counts, CITIZEN_SIMPLE_CALL_VA, SCOUT_BASE_CALL_VA,
};
use crate::wire::CommandView;
use crate::world_owner_frontier::sha256;

pub const STRICT_REPLAY_SHA256: [u8; 32] = [
    0xc0, 0x06, 0xec, 0xb8, 0x60, 0x27, 0x36, 0x05, 0xd2, 0xb4, 0x8b, 0xf6, 0x9f, 0x5d, 0xcb, 0x04,
    0x85, 0x96, 0xde, 0x5f, 0xc7, 0x48, 0xaa, 0x66, 0x4f, 0xa0, 0xa0, 0x44, 0x52, 0xdf, 0x2d, 0xa0,
];
pub const FIRST_SERIAL: i32 = 14;
pub const FIRST_FRAME: i32 = 79;
pub const FIRST_PLAY: usize = 1;
pub const FIRST_OWNER: u8 = 0;
pub const FIRST_SELECTED_O: i16 = 4;
pub const FARM_TYPE: i32 = 0x1a1;

/// Exact setup schedule facts which precede object allocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FirstFarmBuilderSchedule {
    pub base_scout_call_va: u32,
    pub citizen_call_va: u32,
    pub base_scout_calls: u8,
    pub citizen_calls: u8,
    pub selected_o: i16,
    pub selected_call_ordinal: u8,
    pub selected_citizen_index: u8,
    /// The outer schedule alone cannot prove that each `Objects::init_unit` call returned one
    /// stable Unit or that no slot was recycled before frame 79.
    pub allocation_receipts_bound: bool,
    /// `LeaderData::current_upgrade` consumes live Leader bitsets absent from replay setup.
    pub current_upgrade_bound: bool,
}

/// Source-owned geometry before `GroupData::validate_build` reaches terrain and occupancy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FirstFarmGeometry {
    pub requested: (i32, i32),
    pub requested_second: (i32, i32),
    pub footprint: Footprint,
    pub corner_tcoord: (i32, i32),
    pub snapped: (i32, i32),
    pub world_cell: (i32, i32),
}

/// Remaining absent inputs in native call order. Every item is required before
/// `GroupBuildRuntimeAuthority` can be constructed for this real package.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirstFarmAuthorityBlocker {
    PostWorldgenRandomState,
    SetupPlacementWorldSnapshot,
    ObjectsInitUnitReceipts,
    Frame79UnitHandleAndState,
    InterveningFrameChronology,
    Frame79CityAfterImage,
    Frame79WorldObjectHead,
    ValidateBuildProbeChronology,
    ObjectsInitBuildAfterImage,
    BuilderSwarmSearchAfterImage,
}

/// Exact green stages for the first packet plus the ordered red boundary.
#[derive(Clone, Debug)]
pub struct FirstFarmAuthorityDiscovery {
    pub replay_file_sha256: [u8; 32],
    pub replay_payload_sha256: [u8; 32],
    pub package: ReplayGroupBuildPackage,
    pub player_slot: u8,
    pub tribe_index: u8,
    pub tribe: ReplayTribeTypeFacts,
    pub scout: ReplayUnitTypeFacts,
    pub citizen: ReplayUnitTypeFacts,
    pub farm: ReplayBuildTypeFacts,
    pub center_build_o: i16,
    pub center_position: (i32, i32),
    pub center_world_cell: (i32, i32),
    /// Region present in the largest replay-prefix reconstruction. It is deliberately not
    /// accepted as the later `City::init` input.
    pub reconstructed_center_region: i16,
    /// Region produced by the later source-proven Himalayas `Regions::find_all` schedule.
    pub expected_center_region: i16,
    pub city_slot: i16,
    pub builder_schedule: FirstFarmBuilderSchedule,
    pub geometry: FirstFarmGeometry,
    pub initial_groups_checksum: u32,
    /// The checksum serialized in the package is provenance, not a post-action oracle. On this
    /// first packet it still equals the independent `Groups::clear` checksum.
    pub recorded_groups_checksum: u32,
    pub recorded_units_checksum: u32,
    pub blockers: Vec<FirstFarmAuthorityBlocker>,
}

impl FirstFarmAuthorityDiscovery {
    /// The discovery cannot be promoted into the runtime authority while any native input is
    /// absent. This is deliberately stronger than checking only the placement body.
    pub fn runtime_authority_ready(&self) -> bool {
        self.blockers.is_empty()
            && self.builder_schedule.allocation_receipts_bound
            && self.builder_schedule.current_upgrade_bound
    }

    /// The first serialized Groups value is a pre-issue chronology check, not evidence that the
    /// opcode-25 transaction has executed.
    pub fn recorded_groups_matches_pre_issue_state(&self) -> bool {
        self.recorded_groups_checksum == self.initial_groups_checksum
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FirstFarmAuthorityError {
    FileRead(String),
    WrongReplayFileSha256,
    PayloadRead(String),
    WrongReplayPayloadSha256,
    MissingRules,
    WrongReplaySettings,
    MissingPlayer,
    WrongPlayer,
    History(GroupBuildHistoryError),
    WrongFirstPackage,
    Content(PrePairUnitAuthorityError),
    WorldUnavailable,
    WrongFirstCamera,
    InvalidFarmFootprint,
}

impl fmt::Display for FirstFarmAuthorityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "first 2018 Farm authority discovery refused: {self:?}")
    }
}

impl std::error::Error for FirstFarmAuthorityError {}

impl From<GroupBuildHistoryError> for FirstFarmAuthorityError {
    fn from(value: GroupBuildHistoryError) -> Self {
        Self::History(value)
    }
}

impl From<PrePairUnitAuthorityError> for FirstFarmAuthorityError {
    fn from(value: PrePairUnitAuthorityError) -> Self {
        Self::Content(value)
    }
}

/// Discover every exact source-owned input for serial 14 without fitting recorded checksums.
pub fn discover_first_2018_farm(
    replay: &Replay,
) -> Result<FirstFarmAuthorityDiscovery, FirstFarmAuthorityError> {
    let raw = std::fs::read(&replay.path)
        .map_err(|error| FirstFarmAuthorityError::FileRead(error.to_string()))?;
    let replay_file_sha256 = sha256(&raw);
    if replay_file_sha256 != STRICT_REPLAY_SHA256 {
        return Err(FirstFarmAuthorityError::WrongReplayFileSha256);
    }
    let payload = load_payload(&replay.path)
        .map_err(|error| FirstFarmAuthorityError::PayloadRead(error.to_string()))?;
    if sha256(&payload) != replay.initial.payload_sha256 {
        return Err(FirstFarmAuthorityError::WrongReplayPayloadSha256);
    }
    if replay.initial.info.seed != 0x0155_8db7
        || replay.initial.info.settings.map_style != 9
        || replay.initial.info.settings.map_size != 6
        || replay.initial.info.settings.starting_town != 1
        || replay.initial.info.settings.starting_resources != 2
        || replay.initial.info.settings.reveal_map != 1
    {
        return Err(FirstFarmAuthorityError::WrongReplaySettings);
    }
    let player = replay
        .initial
        .info
        .players
        .iter()
        .find(|player| player.present && player.who == FIRST_OWNER)
        .ok_or(FirstFarmAuthorityError::MissingPlayer)?;
    if usize::from(player.play) != FIRST_PLAY || player.tribe != 14 {
        return Err(FirstFarmAuthorityError::WrongPlayer);
    }
    let rules = replay
        .initial
        .rules
        .ok_or(FirstFarmAuthorityError::MissingRules)?;
    let tribe = replay_tribe_type_facts(&payload, &rules, usize::from(player.tribe), 50)?;
    let scout_tribe = replay_tribe_type_facts(&payload, &rules, usize::from(player.tribe), 69)?;
    let scout = replay_unit_type_facts(&payload, &rules, scout_tribe.nation_variant)?;
    let citizen = replay_unit_type_facts(&payload, &rules, tribe.nation_variant)?;
    let farm = replay_build_type_facts(&payload, &rules, FARM_TYPE)?;

    let history = strict_group_build_history_before(replay, FIRST_SERIAL + 1)?;
    let [package] = history.as_slice() else {
        return Err(FirstFarmAuthorityError::WrongFirstPackage);
    };
    if package.lockstep_serial != FIRST_SERIAL
        || package.package_frame != FIRST_FRAME
        || package.play != FIRST_PLAY
        || package.command_index != 0
        || package.who != FIRST_OWNER
        || package.objects != [FIRST_SELECTED_O]
        || package.action.type_index != FARM_TYPE
        || package.action.queued != 1
        || package.action.x != 22_286
        || package.action.y != 66_184
        || package.action.x2 != 22_286
        || package.action.y2 != 66_184
    {
        return Err(FirstFarmAuthorityError::WrongFirstPackage);
    }

    let world = replay
        .initial
        .reconstruct_world()
        .ok_or(FirstFarmAuthorityError::WorldUnavailable)?;
    // The first camera is the replay-carried output of `Game::zoom_to_first_unit`: the
    // canonical setup owner proves that this position is the fresh center Build at object
    // 2000. Do not run its constructor on `reconstruct_world`: that World is intentionally a
    // replay-prefix image and still carries the pre-`Regions::find_all` region byte here.
    let first_turn = replay
        .turns
        .first()
        .ok_or(FirstFarmAuthorityError::WrongFirstCamera)?;
    let player_turn = first_turn
        .players
        .iter()
        .find(|turn| turn.play == FIRST_PLAY as i32)
        .ok_or(FirstFarmAuthorityError::WrongFirstCamera)?;
    let cameras: Vec<_> = player_turn
        .commands
        .iter()
        .filter(|command| command.opcode == CAMERA_COMMAND_OPCODE)
        .collect();
    let [camera] = cameras.as_slice() else {
        return Err(FirstFarmAuthorityError::WrongFirstCamera);
    };
    let camera = CommandView::new(camera.opcode, &camera.bytes);
    let center_position = (
        camera
            .get("x_loc")
            .and_then(|value| i32::try_from(value).ok())
            .ok_or(FirstFarmAuthorityError::WrongFirstCamera)?,
        camera
            .get("y_loc")
            .and_then(|value| i32::try_from(value).ok())
            .ok_or(FirstFarmAuthorityError::WrongFirstCamera)?,
    );
    if center_position.0.rem_euclid(WORLD_TO_COORD) != VILLAGE_CENTER_OFFSET
        || center_position.1.rem_euclid(WORLD_TO_COORD) != VILLAGE_CENTER_OFFSET
    {
        return Err(FirstFarmAuthorityError::WrongFirstCamera);
    }
    let center_world_cell = (
        (center_position.0 - VILLAGE_CENTER_OFFSET) / WORLD_TO_COORD,
        (center_position.1 - VILLAGE_CENTER_OFFSET) / WORLD_TO_COORD,
    );
    if center_world_cell.0 < 0
        || center_world_cell.1 < 0
        || center_world_cell.0 >= world.world.xs
        || center_world_cell.1 >= world.world.ys
    {
        return Err(FirstFarmAuthorityError::WrongFirstCamera);
    }
    let reconstructed_center_region = world
        .world
        .wdata(center_world_cell.0, center_world_cell.1)
        .region;

    let footprint = Footprint {
        x_size: farm.x_size,
        y_size: farm.y_size,
    };
    if !(1..=16).contains(&footprint.x_size) || !(1..=16).contains(&footprint.y_size) {
        return Err(FirstFarmAuthorityError::InvalidFarmFootprint);
    }
    let corner_tcoord = footprint.tile_corner(package.action.x, package.action.y);
    let snapped = footprint.corner_tile(corner_tcoord.0, corner_tcoord.1);
    let world_cell = (
        WCoord::from_coord(Coord(snapped.0)).0,
        WCoord::from_coord(Coord(snapped.1)).0,
    );
    let citizens = starting_citizen_counts(i32::from(replay.initial.info.settings.starting_town))
        .ok_or(FirstFarmAuthorityError::WrongReplaySettings)?;
    if citizens.total != 4 || citizens.fixed_building_prefix != 0 || tribe.tribe_id != 14 {
        return Err(FirstFarmAuthorityError::WrongReplaySettings);
    }
    let initial_groups_checksum = InitialGroupsChannel::derive().checksum;

    Ok(FirstFarmAuthorityDiscovery {
        replay_file_sha256,
        replay_payload_sha256: replay.initial.payload_sha256,
        package: package.clone(),
        player_slot: player.slot,
        tribe_index: player.tribe,
        tribe,
        scout,
        citizen,
        farm,
        center_build_o: 2_000,
        center_position,
        center_world_cell,
        reconstructed_center_region,
        expected_center_region: 1,
        city_slot: 0,
        builder_schedule: FirstFarmBuilderSchedule {
            base_scout_call_va: SCOUT_BASE_CALL_VA,
            citizen_call_va: CITIZEN_SIMPLE_CALL_VA,
            base_scout_calls: 1,
            citizen_calls: citizens.total as u8,
            selected_o: FIRST_SELECTED_O,
            selected_call_ordinal: 4,
            selected_citizen_index: 3,
            allocation_receipts_bound: false,
            current_upgrade_bound: false,
        },
        geometry: FirstFarmGeometry {
            requested: (package.action.x, package.action.y),
            requested_second: (package.action.x2, package.action.y2),
            footprint,
            corner_tcoord,
            snapped,
            world_cell,
        },
        initial_groups_checksum,
        recorded_groups_checksum: package.groups_checksum,
        recorded_units_checksum: package.units_checksum,
        blockers: vec![
            FirstFarmAuthorityBlocker::PostWorldgenRandomState,
            FirstFarmAuthorityBlocker::SetupPlacementWorldSnapshot,
            FirstFarmAuthorityBlocker::ObjectsInitUnitReceipts,
            FirstFarmAuthorityBlocker::Frame79UnitHandleAndState,
            FirstFarmAuthorityBlocker::InterveningFrameChronology,
            FirstFarmAuthorityBlocker::Frame79CityAfterImage,
            FirstFarmAuthorityBlocker::Frame79WorldObjectHead,
            FirstFarmAuthorityBlocker::ValidateBuildProbeChronology,
            FirstFarmAuthorityBlocker::ObjectsInitBuildAfterImage,
            FirstFarmAuthorityBlocker::BuilderSwarmSearchAfterImage,
        ],
    })
}
