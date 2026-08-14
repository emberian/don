//! Golden Build-band `Wall::process` territory authority through frame 31.
//!
//! Retail reaches the cone after the helper latch at `0x00640862`. The selected replay's
//! source `GameInfo::rush_rules` byte is zero. Consequently Market frame 31 returns at the
//! alternating phase-32 gate before reading its position, World, or type. Market frame 15
//! and Village frame 16 both reach a live `WData::who` read followed by
//! `BuildTypeData::is_dock()`. This module evaluates that type child from the serialized
//! replay Rules and installs only the stable result plus exact Build identities. Mutable
//! Leader, Build and World fields remain live canonical Sim reads.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::systems::tech_cities;
use don_sim::systems::unit_inctime::SUPPORTED_RETAIL_EXE_SHA256;
use don_sim::systems::walls::{
    build_wall_territory_authority_digest, BuildWallTerritoryAuthority, BuildWallTerritoryIdentity,
    GOLDEN_BUILD_WALL_TERRITORY_FRAMES,
};
use don_sim::tick::Sim;

use crate::groups_pre_pair_unit_authority::{
    replay_build_type_is_dock_facts, PrePairUnitAuthorityError, ReplayBuildTypeIsDockFacts,
};
use crate::replay::{load_payload, Replay};
use crate::setup_2024_frame379::{Frame379SetupEntryReceipt, OWNER, REPLAY_FILE_SHA256};
use crate::setup_2024_golden_capture::{
    validate_frame1_post_command_authority, Frame1GoldenBindError, Frame1PostCommandAuthority,
    StartingBuildInventory,
};
use crate::setup_2024_starting_market::{DUTCH_STARTING_MARKET_O, DUTCH_STARTING_MARKET_TYPE};
use crate::world_owner_frontier::sha256;

pub const WALL_TERRITORY_ENTRY_VA: u32 = 0x0064_0862;
pub const WALL_TERRITORY_PHASE16_RETURN_GATE_VA: u32 = 0x0064_0885;
pub const WALL_TERRITORY_DISABLE_ATTRITION_GATE_VA: u32 = 0x0064_0895;
pub const WALL_TERRITORY_RUSH_RULES_GATE_VA: u32 = 0x0064_08a2;
pub const WALL_TERRITORY_WAR_ALLOWED_CALL_VA: u32 = 0x0064_08ae;
pub const WALL_TERRITORY_PHASE32_GATE_VA: u32 = 0x0064_09e3;
pub const WALL_TERRITORY_WORLD_OWNER_READ_VA: u32 = 0x0064_0a30;
pub const WALL_TERRITORY_IS_DOCK_CALL_VA: u32 = 0x0064_0a3a;
pub const BUILD_TYPE_IS_DOCK_VA: u32 = 0x0047_2a70;
pub const OBJECT_TYPE_IS_VA: u32 = 0x0065_f7d0;
pub const WALL_TERRITORY_IS_ALLY_CALL_VA: u32 = 0x0064_0ab2;
pub const WALL_PROCESS_RETURN_VA: u32 = 0x0064_0bbd;
pub const GOLDEN_RUSH_RULES: u8 = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoldenBuildWallTerritorySource {
    Frame1AfterImageReplayGameInfoAndSerializedRules,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenBuildWallTerritoryTypeReceipt {
    pub identity: BuildWallTerritoryIdentity,
    pub is_dock: ReplayBuildTypeIsDockFacts,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenBuildWallTerritoryReceipt {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub source: GoldenBuildWallTerritorySource,
    pub replay_file_sha256: [u8; 32],
    pub replay_payload_sha256: [u8; 32],
    pub executable_sha256: [u8; 32],
    pub frame1_composition_digest: [u8; 32],
    pub entry_frame: i32,
    pub rush_rules: u8,
    pub building_attrition_disabled: i32,
    pub frames: [i32; 3],
    pub builds: [GoldenBuildWallTerritoryTypeReceipt; 2],
    pub installed_authority: BuildWallTerritoryAuthority,
    pub next_exact_boundary: &'static str,
}

#[derive(Debug)]
pub enum GoldenBuildWallTerritoryError {
    ReplayRead(String),
    ReplayMismatch,
    PayloadMismatch,
    MissingRules,
    Frame1(Frame1GoldenBindError),
    Rules(PrePairUnitAuthorityError),
    WrongRushRules { replay: u8, live: u8 },
    MissingOwner,
    BuildingAttritionDisabled(i32),
    MissingBuild(usize),
    BuildIdentityMismatch(usize),
    DockTypeReached(i32),
    EmptyDigest,
}

impl fmt::Display for GoldenBuildWallTerritoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "2024 golden Build/Wall territory bind refused: {self:?}")
    }
}

impl std::error::Error for GoldenBuildWallTerritoryError {}

impl From<Frame1GoldenBindError> for GoldenBuildWallTerritoryError {
    fn from(value: Frame1GoldenBindError) -> Self {
        Self::Frame1(value)
    }
}

impl From<PrePairUnitAuthorityError> for GoldenBuildWallTerritoryError {
    fn from(value: PrePairUnitAuthorityError) -> Self {
        Self::Rules(value)
    }
}

fn identity(
    inventory: &StartingBuildInventory,
    row: usize,
    expected_o: i32,
    expected_uid: u16,
    expected_type: i32,
    sim: &Sim,
) -> Result<BuildWallTerritoryIdentity, GoldenBuildWallTerritoryError> {
    let build = sim
        .builds
        .get(row)
        .ok_or(GoldenBuildWallTerritoryError::MissingBuild(row))?;
    let live_type = sim
        .production_runtime
        .build_types
        .get(row)
        .copied()
        .flatten();
    let inventory_matches = if expected_type == tech_cities::ty::VILLAGE {
        inventory.center_build_row == row
            && inventory.center_build_o == expected_o
            && inventory.center_uid == expected_uid
            && inventory.center_type == expected_type
    } else {
        inventory.market_build_row == row
            && inventory.market_build_o == expected_o
            && inventory.market_uid == expected_uid
            && inventory.market_type == expected_type
    };
    if !inventory_matches
        || build.who != OWNER
        || i32::from(build.object_id()) != expected_o
        || build.uid != expected_uid
        || live_type != Some(expected_type)
        || !build.is_started()
        || !build.is_active()
    {
        return Err(GoldenBuildWallTerritoryError::BuildIdentityMismatch(row));
    }
    Ok(BuildWallTerritoryIdentity {
        row,
        who: OWNER,
        o: i16::try_from(expected_o)
            .map_err(|_| GoldenBuildWallTerritoryError::BuildIdentityMismatch(row))?,
        uid: expected_uid,
        type_index: expected_type,
        is_dock: false,
    })
}

fn append_span(image: &mut Vec<u8>, span: crate::initial::ReplayByteSpan) {
    image.extend_from_slice(&(span.offset as u64).to_le_bytes());
    image.extend_from_slice(&(span.bytes as u64).to_le_bytes());
}

fn append_type(image: &mut Vec<u8>, receipt: &GoldenBuildWallTerritoryTypeReceipt) {
    let identity = receipt.identity;
    image.extend_from_slice(&(identity.row as u64).to_le_bytes());
    image.push(identity.who);
    image.extend_from_slice(&identity.o.to_le_bytes());
    image.extend_from_slice(&identity.uid.to_le_bytes());
    image.extend_from_slice(&identity.type_index.to_le_bytes());
    image.push(u8::from(identity.is_dock));
    image.extend_from_slice(&receipt.is_dock.type_index.to_le_bytes());
    image.extend_from_slice(&receipt.is_dock.dock_type_index.to_le_bytes());
    append_span(image, receipt.is_dock.is_list_span);
    image.extend_from_slice(&(receipt.is_dock.is_list.len() as u64).to_le_bytes());
    for value in &receipt.is_dock.is_list {
        image.extend_from_slice(&value.to_le_bytes());
    }
    image.extend_from_slice(&(receipt.is_dock.ancestry.len() as u64).to_le_bytes());
    for node in &receipt.is_dock.ancestry {
        image.extend_from_slice(&node.type_index.to_le_bytes());
        image.extend_from_slice(&node.from.to_le_bytes());
        image.extend_from_slice(&node.graft.to_le_bytes());
        append_span(image, node.type_base);
    }
    image.push(u8::from(receipt.is_dock.is_dock));
}

pub fn golden_build_wall_territory_composition_digest(
    receipt: &GoldenBuildWallTerritoryReceipt,
) -> [u8; 32] {
    let mut image = b"don-2024-golden-build-wall-territory-v1".to_vec();
    image.extend_from_slice(&receipt.revision.to_le_bytes());
    image.push(receipt.source as u8);
    image.extend_from_slice(&receipt.replay_file_sha256);
    image.extend_from_slice(&receipt.replay_payload_sha256);
    image.extend_from_slice(&receipt.executable_sha256);
    image.extend_from_slice(&receipt.frame1_composition_digest);
    image.extend_from_slice(&receipt.entry_frame.to_le_bytes());
    image.push(receipt.rush_rules);
    image.extend_from_slice(&receipt.building_attrition_disabled.to_le_bytes());
    for frame in receipt.frames {
        image.extend_from_slice(&frame.to_le_bytes());
    }
    for build in &receipt.builds {
        append_type(&mut image, build);
    }
    image.extend_from_slice(&receipt.installed_authority.composition_digest.to_le_bytes());
    image.extend_from_slice(&WALL_TERRITORY_ENTRY_VA.to_le_bytes());
    image.extend_from_slice(&WALL_TERRITORY_PHASE16_RETURN_GATE_VA.to_le_bytes());
    image.extend_from_slice(&WALL_TERRITORY_DISABLE_ATTRITION_GATE_VA.to_le_bytes());
    image.extend_from_slice(&WALL_TERRITORY_RUSH_RULES_GATE_VA.to_le_bytes());
    image.extend_from_slice(&WALL_TERRITORY_WAR_ALLOWED_CALL_VA.to_le_bytes());
    image.extend_from_slice(&WALL_TERRITORY_PHASE32_GATE_VA.to_le_bytes());
    image.extend_from_slice(&WALL_TERRITORY_WORLD_OWNER_READ_VA.to_le_bytes());
    image.extend_from_slice(&WALL_TERRITORY_IS_DOCK_CALL_VA.to_le_bytes());
    image.extend_from_slice(&BUILD_TYPE_IS_DOCK_VA.to_le_bytes());
    image.extend_from_slice(&OBJECT_TYPE_IS_VA.to_le_bytes());
    image.extend_from_slice(&WALL_TERRITORY_IS_ALLY_CALL_VA.to_le_bytes());
    image.extend_from_slice(&WALL_PROCESS_RETURN_VA.to_le_bytes());
    image.extend_from_slice(receipt.next_exact_boundary.as_bytes());
    sha256(&image)
}

/// Validate the frame-one after-image and atomically install the exact golden territory
/// sidecar. No checksummed Sim field is changed.
pub fn mount_golden_build_wall_territory(
    replay: &Replay,
    setup_entry: &Frame379SetupEntryReceipt,
    frame1: &Frame1PostCommandAuthority,
    sim: &mut Sim,
) -> Result<GoldenBuildWallTerritoryReceipt, GoldenBuildWallTerritoryError> {
    let replay_bytes = std::fs::read(&replay.path)
        .map_err(|error| GoldenBuildWallTerritoryError::ReplayRead(error.to_string()))?;
    if sha256(&replay_bytes) != REPLAY_FILE_SHA256
        || frame1.replay_file_sha256 != REPLAY_FILE_SHA256
    {
        return Err(GoldenBuildWallTerritoryError::ReplayMismatch);
    }
    validate_frame1_post_command_authority(setup_entry, frame1, sim)?;

    let payload = load_payload(&replay.path)
        .map_err(|error| GoldenBuildWallTerritoryError::ReplayRead(error.to_string()))?;
    if sha256(&payload) != replay.initial.payload_sha256 {
        return Err(GoldenBuildWallTerritoryError::PayloadMismatch);
    }
    let rules = replay
        .initial
        .rules
        .ok_or(GoldenBuildWallTerritoryError::MissingRules)?;
    let replay_rush = replay.initial.info.settings.rush_rules;
    let live_rush = sim.vic_match.options.rush_rules;
    if replay_rush != GOLDEN_RUSH_RULES || live_rush != replay_rush {
        return Err(GoldenBuildWallTerritoryError::WrongRushRules {
            replay: replay_rush,
            live: live_rush,
        });
    }
    let building_attrition_disabled = sim
        .vic_leaders
        .slots
        .get(usize::from(OWNER))
        .ok_or(GoldenBuildWallTerritoryError::MissingOwner)?
        .building_attrition_disabled;
    if building_attrition_disabled != 0 {
        return Err(GoldenBuildWallTerritoryError::BuildingAttritionDisabled(
            building_attrition_disabled,
        ));
    }

    let inventory = &frame1.starting_builds;
    let center_identity = identity(
        inventory,
        inventory.center_build_row,
        inventory.center_build_o,
        inventory.center_uid,
        tech_cities::ty::VILLAGE,
        sim,
    )?;
    let market_identity = identity(
        inventory,
        inventory.market_build_row,
        DUTCH_STARTING_MARKET_O,
        inventory.market_uid,
        DUTCH_STARTING_MARKET_TYPE,
        sim,
    )?;
    let center_is_dock =
        replay_build_type_is_dock_facts(&payload, &rules, center_identity.type_index)?;
    let market_is_dock =
        replay_build_type_is_dock_facts(&payload, &rules, market_identity.type_index)?;
    if center_is_dock.is_dock {
        return Err(GoldenBuildWallTerritoryError::DockTypeReached(
            center_identity.type_index,
        ));
    }
    if market_is_dock.is_dock {
        return Err(GoldenBuildWallTerritoryError::DockTypeReached(
            market_identity.type_index,
        ));
    }

    let mut authority = BuildWallTerritoryAuthority {
        revision: frame1.revision,
        composition_digest: 0,
        rush_rules: replay_rush,
        frames: GOLDEN_BUILD_WALL_TERRITORY_FRAMES,
        builds: [center_identity, market_identity],
    };
    authority.composition_digest = build_wall_territory_authority_digest(&authority);
    let builds = [
        GoldenBuildWallTerritoryTypeReceipt {
            identity: center_identity,
            is_dock: center_is_dock,
        },
        GoldenBuildWallTerritoryTypeReceipt {
            identity: market_identity,
            is_dock: market_is_dock,
        },
    ];
    let mut receipt = GoldenBuildWallTerritoryReceipt {
        revision: frame1.revision,
        composition_digest: [0; 32],
        source: GoldenBuildWallTerritorySource::Frame1AfterImageReplayGameInfoAndSerializedRules,
        replay_file_sha256: REPLAY_FILE_SHA256,
        replay_payload_sha256: replay.initial.payload_sha256,
        executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
        frame1_composition_digest: frame1.composition_digest,
        entry_frame: sim.world.frame,
        rush_rules: replay_rush,
        building_attrition_disabled,
        frames: GOLDEN_BUILD_WALL_TERRITORY_FRAMES,
        builds,
        installed_authority: authority.clone(),
        next_exact_boundary:
            "non-friendly WData owner -> LeaderData::is_ally at 0x00640AB2; dock -> tile_corner at 0x00640A50",
    };
    receipt.composition_digest = golden_build_wall_territory_composition_digest(&receipt);
    if receipt.composition_digest == [0; 32] || authority.composition_digest == 0 {
        return Err(GoldenBuildWallTerritoryError::EmptyDigest);
    }
    sim.replace_build_wall_territory_authority(authority);
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn installed_replay_source_proves_village_and_market_are_not_docks() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../ron-data/replays/multi/Playback___2024.02.23_20_49_35__Fri_.rcx");
        if !path.exists() {
            eprintln!("SKIPPED -- NOT A PASS: missing {}", path.display());
            return;
        }
        let replay = Replay::open(&path).expect("installed golden replay must decode");
        let payload = load_payload(&path).expect("installed golden replay must inflate");
        let rules = replay
            .initial
            .rules
            .expect("golden replay serializes Rules");
        for type_index in [tech_cities::ty::VILLAGE, DUTCH_STARTING_MARKET_TYPE] {
            let facts = replay_build_type_is_dock_facts(&payload, &rules, type_index)
                .expect("source relation must decode");
            assert_eq!(facts.type_index, type_index);
            assert!(!facts.is_dock);
        }
    }
}
