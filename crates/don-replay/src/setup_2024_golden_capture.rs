//! Minimal retail-oracle contract for the supported 2024 golden replay.
//!
//! The replay file does not contain the generated setup state.  This module therefore makes
//! the smallest complete capture bundle explicit: setup entry, seven adjacent completed Unit
//! receivers, the post-frame-zero/pre-command image, the serial-1 after-image, and the first
//! post-command tick at frame 2.  Later frame-379/384/391 captures are separate, optional
//! manifests and cannot substitute for this prefix.
//!
//! Every Sim hash is over canonical DoNSave bytes from the supported retail executable.  The
//! command-entry image is intentionally independent of the frame-zero setup image: retail's
//! frame-zero scheduler can change Scout/Merchant orders, masks, paths, and object-search scratch
//! by this boundary. Scout spellcaster work cannot mutate `CasterData::active_spells`; that empty
//! queue is separately derivable from setup and need not inflate the capture contract.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::systems::map_terrain::WorldChecksum;
use don_sim::systems::production;
use don_sim::systems::save_load::{load_sim, save_sim, SaveError};
use don_sim::systems::sparse_object_bands_authority_frontier::{RetailBand, RetailObjectAddress};
use don_sim::systems::unit_inctime::SUPPORTED_RETAIL_EXE_SHA256;
use don_sim::tick::Sim;
use don_sim::world::WorldObjectIdentity;

use crate::replay::Replay;
use crate::setup_2024_frame1_leader_options::{
    mount_frame1_setup_leader_options, plan_frame1_setup_leader_options,
    Frame1CommandEntryAuthority, Frame1CommandEntryCapture, Frame1LeaderOptionsError,
    Frame1LeaderOptionsMountError, Frame1LeaderOptionsMountReceipt, Frame1StanceTarget,
    LEADER_OPTIONS_FRAME,
};
use crate::setup_2024_frame379::{
    Frame379CompletedInitCapture, Frame379CompletedInitSource, Frame379SetupEntryCapture,
    Frame379SetupEntryReceipt, Frame379SetupEntrySource, Frame379SetupReceipt,
    DUTCH_STARTING_MARKET_O, DUTCH_STARTING_MARKET_TYPE, OWNER, REPLAY_FILE_SHA256, SETUP_CALLS,
};
use crate::setup_unit_member_authority::{
    bind_canonical_setup_members, CanonicalSetupMemberError, CanonicalSetupMemberSource,
    CanonicalSetupSnapshotAuthority,
};
use crate::setup_units_producer::StableUnitIdentityReceipt;
use crate::world_owner_frontier::sha256;

/// Schema 2 makes the unconditional Dutch Market after-image mandatory.  A historical
/// center-plus-seven capture description cannot validate as the golden setup.
pub const GOLDEN_CAPTURE_SCHEMA_VERSION: u64 = 2;
pub const MINIMAL_UNIQUE_SIM_SNAPSHOTS: usize = 11;
pub const FIRST_POST_COMMAND_TICK_FRAME: i32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame1PostCommandSource {
    /// Supported retail processed the complete serial-1 LeaderOptions pair and returned.
    CompleteRetailSerialOneLeaderOptionsReturn,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1PostCommandCapture {
    pub revision: u64,
    pub source: Frame1PostCommandSource,
    pub replay_file_sha256: [u8; 32],
    pub executable_sha256: [u8; 32],
    pub command_entry_sim_sha256: [u8; 32],
    pub post_command_sim_sha256: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame2PostTickSource {
    /// Supported retail completed exactly the post-command frame-1 `Game::do_frame`.
    CompleteRetailPostCommandFrameOneDoFrame,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame2PostTickCapture {
    pub revision: u64,
    pub source: Frame2PostTickSource,
    pub replay_file_sha256: [u8; 32],
    pub executable_sha256: [u8; 32],
    pub post_command_sim_sha256: [u8; 32],
    pub checkpoint_sim_sha256: [u8; 32],
}

/// Complete minimal capture manifest.  The seven receiver captures form eight adjacent
/// frame-zero images; the remaining three images are frame-1 command entry, frame-1 command
/// return, and frame-2 post-tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenCaptureManifest {
    pub schema_version: u64,
    pub starting_market_o: i32,
    pub starting_market_type: i32,
    pub setup_entry: Frame379SetupEntryCapture,
    pub completed_inits: [Frame379CompletedInitCapture; SETUP_CALLS],
    pub frame1_entry: Frame1CommandEntryCapture,
    pub frame1_post_command: Frame1PostCommandCapture,
    pub frame2_post_tick: Frame2PostTickCapture,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoldenCaptureManifestError {
    WrongSchemaVersion,
    MissingStartingMarketContract,
    MissingCaptureRevision,
    WrongCaptureSource,
    ReplayMismatch,
    UnsupportedExecutable,
    WrongReceiverOrdinal { expected: usize, actual: usize },
    MissingSnapshotDigest,
    MissingDetailedReceiptDigest { ordinal: usize },
    BrokenReceiverChain { ordinal: usize },
    BrokenFrame1EntryLink,
    BrokenFrame1CommandLink,
    BrokenFrame2TickLink,
}

impl fmt::Display for GoldenCaptureManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "2024 golden capture manifest refused: {self:?}")
    }
}

impl std::error::Error for GoldenCaptureManifestError {}

/// Validate only the capture topology and immutable source identities.  Admission of the
/// contents remains the job of the setup/receiver/frame binders.
pub fn validate_golden_capture_manifest(
    manifest: &GoldenCaptureManifest,
) -> Result<(), GoldenCaptureManifestError> {
    if manifest.schema_version != GOLDEN_CAPTURE_SCHEMA_VERSION {
        return Err(GoldenCaptureManifestError::WrongSchemaVersion);
    }
    if manifest.starting_market_o != DUTCH_STARTING_MARKET_O
        || manifest.starting_market_type != DUTCH_STARTING_MARKET_TYPE
    {
        return Err(GoldenCaptureManifestError::MissingStartingMarketContract);
    }
    if manifest.setup_entry.revision == 0
        || manifest
            .completed_inits
            .iter()
            .any(|capture| capture.revision == 0)
        || manifest.frame1_entry.revision == 0
        || manifest.frame1_post_command.revision == 0
        || manifest.frame2_post_tick.revision == 0
    {
        return Err(GoldenCaptureManifestError::MissingCaptureRevision);
    }
    if manifest.setup_entry.source != Frame379SetupEntrySource::CompleteRetailBuildUnitsEntry
        || manifest.completed_inits.iter().any(|capture| {
            capture.source
                != Frame379CompletedInitSource::CompleteRetailObjectsInitUnitReceiver
        })
        || manifest.frame1_entry.source
            != crate::setup_2024_frame1_leader_options::Frame1CommandEntrySource::AuthoritativePlaybackChronologyFrameZeroThroughOne
        || manifest.frame1_post_command.source
            != Frame1PostCommandSource::CompleteRetailSerialOneLeaderOptionsReturn
        || manifest.frame2_post_tick.source
            != Frame2PostTickSource::CompleteRetailPostCommandFrameOneDoFrame
    {
        return Err(GoldenCaptureManifestError::WrongCaptureSource);
    }
    if manifest.setup_entry.replay_file_sha256 != REPLAY_FILE_SHA256
        || manifest
            .completed_inits
            .iter()
            .any(|capture| capture.replay_file_sha256 != REPLAY_FILE_SHA256)
        || manifest.frame1_entry.replay_file_sha256 != REPLAY_FILE_SHA256
        || manifest.frame1_post_command.replay_file_sha256 != REPLAY_FILE_SHA256
        || manifest.frame2_post_tick.replay_file_sha256 != REPLAY_FILE_SHA256
    {
        return Err(GoldenCaptureManifestError::ReplayMismatch);
    }
    if manifest.setup_entry.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256
        || manifest
            .completed_inits
            .iter()
            .any(|capture| capture.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256)
        || manifest.frame1_entry.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256
        || manifest.frame1_post_command.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256
        || manifest.frame2_post_tick.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256
    {
        return Err(GoldenCaptureManifestError::UnsupportedExecutable);
    }

    let zero = [0; 32];
    if manifest.setup_entry.entry_sim_sha256 == zero
        || manifest.frame1_entry.setup_sim_sha256 == zero
        || manifest.frame1_entry.command_entry_sim_sha256 == zero
        || manifest.frame1_post_command.command_entry_sim_sha256 == zero
        || manifest.frame1_post_command.post_command_sim_sha256 == zero
        || manifest.frame2_post_tick.post_command_sim_sha256 == zero
        || manifest.frame2_post_tick.checkpoint_sim_sha256 == zero
    {
        return Err(GoldenCaptureManifestError::MissingSnapshotDigest);
    }
    for (ordinal, capture) in manifest.completed_inits.iter().enumerate() {
        if capture.setup_ordinal != ordinal {
            return Err(GoldenCaptureManifestError::WrongReceiverOrdinal {
                expected: ordinal,
                actual: capture.setup_ordinal,
            });
        }
        if capture.before_sim_sha256 == zero || capture.after_sim_sha256 == zero {
            return Err(GoldenCaptureManifestError::MissingSnapshotDigest);
        }
        if capture.detailed_receipt_sha256 == zero {
            return Err(GoldenCaptureManifestError::MissingDetailedReceiptDigest { ordinal });
        }
        let expected_before = if ordinal == 0 {
            manifest.setup_entry.entry_sim_sha256
        } else {
            manifest.completed_inits[ordinal - 1].after_sim_sha256
        };
        if capture.before_sim_sha256 != expected_before {
            return Err(GoldenCaptureManifestError::BrokenReceiverChain { ordinal });
        }
    }
    let completed_setup = manifest.completed_inits[SETUP_CALLS - 1].after_sim_sha256;
    if manifest.frame1_entry.setup_sim_sha256 != completed_setup {
        return Err(GoldenCaptureManifestError::BrokenFrame1EntryLink);
    }
    if manifest.frame1_entry.command_entry_sim_sha256
        != manifest.frame1_post_command.command_entry_sim_sha256
    {
        return Err(GoldenCaptureManifestError::BrokenFrame1CommandLink);
    }
    if manifest.frame1_post_command.post_command_sim_sha256
        != manifest.frame2_post_tick.post_command_sim_sha256
    {
        return Err(GoldenCaptureManifestError::BrokenFrame2TickLink);
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartingBuildInventory {
    pub center_build_row: usize,
    pub center_build_o: i32,
    pub center_uid: u16,
    pub center_type: i32,
    pub center_city_slot: i16,
    pub market_build_row: usize,
    pub market_build_o: i32,
    pub market_uid: u16,
    pub market_type: i32,
    pub market_city_slot: i16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1PostCommandAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub source: Frame1PostCommandSource,
    pub replay_file_sha256: [u8; 32],
    pub executable_sha256: [u8; 32],
    pub setup_composition_digest: [u8; 32],
    pub command_entry_composition_digest: [u8; 32],
    pub command_frame: i32,
    pub command_entry_sim_sha256: [u8; 32],
    pub post_command_sim_sha256: [u8; 32],
    pub world_checksum: WorldChecksum,
    pub random_state: i32,
    pub setup_members: [StableUnitIdentityReceipt; SETUP_CALLS],
    pub starting_builds: StartingBuildInventory,
    pub mount: Frame1LeaderOptionsMountReceipt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame2PostTickAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub source: Frame2PostTickSource,
    pub replay_file_sha256: [u8; 32],
    pub executable_sha256: [u8; 32],
    pub setup_composition_digest: [u8; 32],
    pub frame1_post_command_digest: [u8; 32],
    pub entry_frame: i32,
    pub checkpoint_frame: i32,
    pub post_command_sim_sha256: [u8; 32],
    pub checkpoint_sim_sha256: [u8; 32],
    pub world_checksum: WorldChecksum,
    pub random_state: i32,
    pub setup_members: [StableUnitIdentityReceipt; SETUP_CALLS],
    pub starting_builds: StartingBuildInventory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame1GoldenBindError {
    Plan(Frame1LeaderOptionsError),
    Mount(Frame1LeaderOptionsMountError),
    SetupMember(CanonicalSetupMemberError),
    Snapshot(SaveError),
    MissingCaptureRevision,
    MissingAuthorityDigest,
    WrongCaptureSource,
    ReplayMismatch,
    UnsupportedExecutable,
    SetupMismatch,
    WrongFrame { expected: i32, actual: i32 },
    CommandEntrySnapshotMismatch,
    PostCommandSnapshotMismatch,
    OfflineCommandAfterImageMismatch,
    Frame1AuthorityMismatch,
    SetupIdentityChanged,
    MissingStartingBuild,
    StartingBuildMismatch,
    StartingCityMismatch,
    InterveningSimCommand,
}

impl fmt::Display for Frame1GoldenBindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "2024 golden frame-prefix bind refused: {self:?}")
    }
}

impl std::error::Error for Frame1GoldenBindError {}

impl From<Frame1LeaderOptionsError> for Frame1GoldenBindError {
    fn from(value: Frame1LeaderOptionsError) -> Self {
        Self::Plan(value)
    }
}

impl From<CanonicalSetupMemberError> for Frame1GoldenBindError {
    fn from(value: CanonicalSetupMemberError) -> Self {
        Self::SetupMember(value)
    }
}

fn append_world_checksum(image: &mut Vec<u8>, checksum: &WorldChecksum) {
    image.extend_from_slice(&checksum.full.to_le_bytes());
    image.extend_from_slice(&checksum.bytes.to_le_bytes());
    for section in &checksum.per_section {
        image.extend_from_slice(&section.adler.to_le_bytes());
        image.extend_from_slice(&section.bytes.to_le_bytes());
    }
}

fn append_identity(image: &mut Vec<u8>, identity: StableUnitIdentityReceipt) {
    image.extend_from_slice(&identity.id.to_le_bytes());
    image.extend_from_slice(&identity.generation.to_le_bytes());
    image.extend_from_slice(&identity.owner.to_le_bytes());
    image.extend_from_slice(&identity.o.to_le_bytes());
}

fn append_starting_builds(image: &mut Vec<u8>, builds: &StartingBuildInventory) {
    image.extend_from_slice(&(builds.center_build_row as u64).to_le_bytes());
    image.extend_from_slice(&builds.center_build_o.to_le_bytes());
    image.extend_from_slice(&builds.center_uid.to_le_bytes());
    image.extend_from_slice(&builds.center_type.to_le_bytes());
    image.extend_from_slice(&builds.center_city_slot.to_le_bytes());
    image.extend_from_slice(&(builds.market_build_row as u64).to_le_bytes());
    image.extend_from_slice(&builds.market_build_o.to_le_bytes());
    image.extend_from_slice(&builds.market_uid.to_le_bytes());
    image.extend_from_slice(&builds.market_type.to_le_bytes());
    image.extend_from_slice(&builds.market_city_slot.to_le_bytes());
}

fn append_mount(image: &mut Vec<u8>, mount: &Frame1LeaderOptionsMountReceipt) {
    image.extend_from_slice(&mount.chronology_revision.to_le_bytes());
    image.extend_from_slice(&mount.chronology_digest);
    image.push(match mount.source {
        crate::setup_2024_frame1_leader_options::Frame1CommandEntrySource::AuthoritativePlaybackChronologyFrameZeroThroughOne => 1,
    });
    image.extend_from_slice(&mount.replay_file_sha256);
    image.extend_from_slice(&mount.setup_composition_digest);
    image.extend_from_slice(&mount.command_entry_sim_sha256);
    for command in &mount.commands {
        image.extend_from_slice(&(command.turn_index as u64).to_le_bytes());
        image.extend_from_slice(&(command.player_index as u64).to_le_bytes());
        image.extend_from_slice(&(command.command_index as u64).to_le_bytes());
        image.extend_from_slice(&command.lockstep_serial.to_le_bytes());
        image.extend_from_slice(&command.frame.to_le_bytes());
        image.extend_from_slice(&command.play.to_le_bytes());
        let data = command.command.data;
        image.extend_from_slice(&data.who.to_le_bytes());
        image.extend_from_slice(&data.peasants.to_le_bytes());
        image.extend_from_slice(&data.peasants_wait.to_le_bytes());
        image.extend_from_slice(&data.buildings.to_le_bytes());
        image.extend_from_slice(&data.flags.bits.to_le_bytes());
        image.extend_from_slice(&data.flags.size.to_le_bytes());
        image.extend_from_slice(&data.flags.flags.to_le_bytes());
        image.extend_from_slice(&data.flags.inline);
    }
    for row in &mount.rows_after {
        image.extend_from_slice(&row.who.to_le_bytes());
        image.extend_from_slice(&row.peasants.to_le_bytes());
        image.extend_from_slice(&row.peasants_wait.to_le_bytes());
        image.extend_from_slice(&row.buildings.to_le_bytes());
        image.extend_from_slice(&row.flags.bits.to_le_bytes());
        image.extend_from_slice(&row.flags.size.to_le_bytes());
        image.extend_from_slice(&row.flags.flags.to_le_bytes());
        image.extend_from_slice(&row.flags.inline);
    }
    for mutation in &mount.mutations {
        match mutation.target {
            Frame1StanceTarget::SetupUnit { setup_ordinal } => {
                image.push(1);
                image.extend_from_slice(&(setup_ordinal as u64).to_le_bytes());
            }
            Frame1StanceTarget::StartingVillage => {
                image.push(2);
                image.extend_from_slice(&0_u64.to_le_bytes());
            }
        }
        image.extend_from_slice(&(mutation.row as u64).to_le_bytes());
        match mutation.handle {
            Some(handle) => {
                image.push(1);
                image.extend_from_slice(&handle.id.to_le_bytes());
                image.extend_from_slice(&handle.generation.to_le_bytes());
            }
            None => image.push(0),
        }
        image.extend_from_slice(&mutation.who.to_le_bytes());
        image.extend_from_slice(&mutation.o.to_le_bytes());
        image.extend_from_slice(&mutation.type_index.to_le_bytes());
        image.extend_from_slice(&mutation.before.to_le_bytes());
        image.extend_from_slice(&mutation.after.to_le_bytes());
    }
    image.extend_from_slice(mount.next_exact_boundary.as_bytes());
}

/// Stable digest over every public frame-1 authority claim other than the digest itself.
pub fn frame1_post_command_composition_digest(authority: &Frame1PostCommandAuthority) -> [u8; 32] {
    let mut image = b"don-2024-golden-frame1-post-command-v2".to_vec();
    image.extend_from_slice(&authority.revision.to_le_bytes());
    image.push(match authority.source {
        Frame1PostCommandSource::CompleteRetailSerialOneLeaderOptionsReturn => 1,
    });
    image.extend_from_slice(&authority.replay_file_sha256);
    image.extend_from_slice(&authority.executable_sha256);
    image.extend_from_slice(&authority.setup_composition_digest);
    image.extend_from_slice(&authority.command_entry_composition_digest);
    image.extend_from_slice(&authority.command_frame.to_le_bytes());
    image.extend_from_slice(&authority.command_entry_sim_sha256);
    image.extend_from_slice(&authority.post_command_sim_sha256);
    append_world_checksum(&mut image, &authority.world_checksum);
    image.extend_from_slice(&authority.random_state.to_le_bytes());
    for identity in authority.setup_members {
        append_identity(&mut image, identity);
    }
    append_starting_builds(&mut image, &authority.starting_builds);
    append_mount(&mut image, &authority.mount);
    sha256(&image)
}

/// Stable digest over every public frame-2 authority claim other than the digest itself.
pub fn frame2_post_tick_composition_digest(authority: &Frame2PostTickAuthority) -> [u8; 32] {
    let mut image = b"don-2024-golden-frame2-post-tick-v2".to_vec();
    image.extend_from_slice(&authority.revision.to_le_bytes());
    image.push(match authority.source {
        Frame2PostTickSource::CompleteRetailPostCommandFrameOneDoFrame => 1,
    });
    image.extend_from_slice(&authority.replay_file_sha256);
    image.extend_from_slice(&authority.executable_sha256);
    image.extend_from_slice(&authority.setup_composition_digest);
    image.extend_from_slice(&authority.frame1_post_command_digest);
    image.extend_from_slice(&authority.entry_frame.to_le_bytes());
    image.extend_from_slice(&authority.checkpoint_frame.to_le_bytes());
    image.extend_from_slice(&authority.post_command_sim_sha256);
    image.extend_from_slice(&authority.checkpoint_sim_sha256);
    append_world_checksum(&mut image, &authority.world_checksum);
    image.extend_from_slice(&authority.random_state.to_le_bytes());
    for identity in authority.setup_members {
        append_identity(&mut image, identity);
    }
    append_starting_builds(&mut image, &authority.starting_builds);
    sha256(&image)
}

fn starting_build_inventory(
    setup_entry: &Frame379SetupEntryReceipt,
    sim: &Sim,
) -> Result<StartingBuildInventory, Frame1GoldenBindError> {
    let center_identity = sim
        .world
        .object_bands()
        .live_identity(RetailObjectAddress::new(
            OWNER,
            RetailBand::Build,
            setup_entry.center_build_o,
        ))
        .ok_or(Frame1GoldenBindError::MissingStartingBuild)?;
    let market_identity = sim
        .world
        .object_bands()
        .live_identity(RetailObjectAddress::new(
            OWNER,
            RetailBand::Build,
            DUTCH_STARTING_MARKET_O,
        ))
        .ok_or(Frame1GoldenBindError::MissingStartingBuild)?;
    let (WorldObjectIdentity::BuildRow(center_row), WorldObjectIdentity::BuildRow(market_row)) =
        (center_identity, market_identity)
    else {
        return Err(Frame1GoldenBindError::MissingStartingBuild);
    };
    let center_build_row = center_row as usize;
    let market_build_row = market_row as usize;
    if center_build_row != setup_entry.center_build_row
        || market_build_row != setup_entry.market_build_row
    {
        return Err(Frame1GoldenBindError::StartingBuildMismatch);
    }
    let center = sim
        .builds
        .get(center_build_row)
        .ok_or(Frame1GoldenBindError::MissingStartingBuild)?;
    let market = sim
        .builds
        .get(market_build_row)
        .ok_or(Frame1GoldenBindError::MissingStartingBuild)?;
    let center_type = sim
        .production_runtime
        .build_types
        .get(center_build_row)
        .and_then(|value| *value)
        .ok_or(Frame1GoldenBindError::MissingStartingBuild)?;
    let market_type = sim
        .production_runtime
        .build_types
        .get(market_build_row)
        .and_then(|value| *value)
        .ok_or(Frame1GoldenBindError::MissingStartingBuild)?;
    let active = production::flag::VALID | production::flag::STARTED | production::flag::ACTIVE;
    if center.flags & (active | 0x20) != active | 0x20
        || center.who != OWNER
        || i32::from(center.object_id()) != setup_entry.center_build_o
        || center_type != crate::setup_cities_builds::CITY_CENTER_TYPE
        || center.city != setup_entry.center_city_slot
        || center.city_down != DUTCH_STARTING_MARKET_O as i16
        || market.flags & active != active
        || market.flags & 0x20 != 0
        || market.who != OWNER
        || i32::from(market.object_id()) != DUTCH_STARTING_MARKET_O
        || market_type != DUTCH_STARTING_MARKET_TYPE
        || market.city != setup_entry.market_city_slot
        || market.city != center.city
        || market.city_down != -1
    {
        return Err(Frame1GoldenBindError::StartingBuildMismatch);
    }
    let city = usize::try_from(center.city)
        .ok()
        .and_then(|slot| sim.cities.slots.get(usize::from(OWNER))?.get(slot))
        .ok_or(Frame1GoldenBindError::StartingCityMismatch)?;
    if !city.active()
        || city.who != OWNER as i8
        || city.city != center.city
        || city.o != center.object_id()
    {
        return Err(Frame1GoldenBindError::StartingCityMismatch);
    }
    Ok(StartingBuildInventory {
        center_build_row,
        center_build_o: i32::from(center.object_id()),
        center_uid: center.uid,
        center_type,
        center_city_slot: center.city,
        market_build_row,
        market_build_o: i32::from(market.object_id()),
        market_uid: market.uid,
        market_type,
        market_city_slot: market.city,
    })
}

fn bind_members(
    replay: &Replay,
    setup: &Frame379SetupReceipt,
    sim: &Sim,
    revision: u64,
    digest: [u8; 32],
) -> Result<[StableUnitIdentityReceipt; SETUP_CALLS], Frame1GoldenBindError> {
    let ordinals = (0..SETUP_CALLS).collect::<Vec<_>>();
    let authority = CanonicalSetupSnapshotAuthority {
        revision,
        composition_digest: digest,
        source: CanonicalSetupMemberSource::ReplayRulesCompleteInitReceiptAndCanonicalSim,
        replay_file_sha256: REPLAY_FILE_SHA256,
        frame: sim.world.frame,
        world_checksum: sim.map.world.checksum_sections(),
        random_state: sim.world.random.state(),
    };
    let members = bind_canonical_setup_members(
        replay,
        &setup.plan,
        &setup.setup,
        sim,
        &ordinals,
        &authority,
    )?;
    let identities = members
        .into_iter()
        .map(|member| member.stable_identity())
        .collect::<Vec<_>>();
    identities
        .try_into()
        .map_err(|_| Frame1GoldenBindError::SetupIdentityChanged)
}

/// Bind the exact serial-1 return image.  The admitted after-image must equal the independently
/// mounted five-write LeaderOptions transition byte for byte.
pub fn bind_captured_frame1_post_command(
    replay: &Replay,
    setup_entry: &Frame379SetupEntryReceipt,
    setup: &Frame379SetupReceipt,
    chronology: &Frame1CommandEntryAuthority,
    command_entry: &Sim,
    post_command: &Sim,
    capture: &Frame1PostCommandCapture,
) -> Result<Frame1PostCommandAuthority, Frame1GoldenBindError> {
    if capture.revision == 0 {
        return Err(Frame1GoldenBindError::MissingCaptureRevision);
    }
    if capture.source != Frame1PostCommandSource::CompleteRetailSerialOneLeaderOptionsReturn {
        return Err(Frame1GoldenBindError::WrongCaptureSource);
    }
    if capture.replay_file_sha256 != REPLAY_FILE_SHA256
        || chronology.replay_file_sha256 != REPLAY_FILE_SHA256
        || setup.replay_file_sha256 != REPLAY_FILE_SHA256
    {
        return Err(Frame1GoldenBindError::ReplayMismatch);
    }
    if capture.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256 {
        return Err(Frame1GoldenBindError::UnsupportedExecutable);
    }
    if chronology.setup_composition_digest != setup.canonical_composition_digest
        || setup_entry.worldgen.composition_digest != setup.authority_digest
    {
        return Err(Frame1GoldenBindError::SetupMismatch);
    }
    if command_entry.world.frame != LEADER_OPTIONS_FRAME as i32 {
        return Err(Frame1GoldenBindError::WrongFrame {
            expected: LEADER_OPTIONS_FRAME as i32,
            actual: command_entry.world.frame,
        });
    }
    if post_command.world.frame != LEADER_OPTIONS_FRAME as i32 {
        return Err(Frame1GoldenBindError::WrongFrame {
            expected: LEADER_OPTIONS_FRAME as i32,
            actual: post_command.world.frame,
        });
    }
    let command_entry_bytes = save_sim(command_entry).map_err(Frame1GoldenBindError::Snapshot)?;
    if sha256(&command_entry_bytes) != capture.command_entry_sim_sha256
        || capture.command_entry_sim_sha256 != chronology.command_entry_sim_sha256
    {
        return Err(Frame1GoldenBindError::CommandEntrySnapshotMismatch);
    }
    let post_command_bytes = save_sim(post_command).map_err(Frame1GoldenBindError::Snapshot)?;
    if sha256(&post_command_bytes) != capture.post_command_sim_sha256 {
        return Err(Frame1GoldenBindError::PostCommandSnapshotMismatch);
    }

    let candidate = load_sim(&command_entry_bytes).map_err(Frame1GoldenBindError::Snapshot)?;
    let (expected, mount) =
        mount_frame1_setup_leader_options(replay, setup_entry, setup, candidate, chronology)
            .map_err(|(error, _)| Frame1GoldenBindError::Mount(error))?;
    let expected_bytes = save_sim(&expected).map_err(Frame1GoldenBindError::Snapshot)?;
    if expected_bytes != post_command_bytes {
        return Err(Frame1GoldenBindError::OfflineCommandAfterImageMismatch);
    }

    let before_members = bind_members(
        replay,
        setup,
        command_entry,
        chronology.revision,
        chronology.command_entry_sim_sha256,
    )?;
    let setup_members = bind_members(
        replay,
        setup,
        post_command,
        capture.revision,
        capture.post_command_sim_sha256,
    )?;
    if before_members != setup_members {
        return Err(Frame1GoldenBindError::SetupIdentityChanged);
    }
    let _before_builds = starting_build_inventory(setup_entry, command_entry)?;
    let starting_builds = starting_build_inventory(setup_entry, post_command)?;
    let world_checksum = post_command.map.world.checksum_sections();
    let random_state = post_command.world.random.state();
    let mut authority = Frame1PostCommandAuthority {
        revision: capture.revision,
        composition_digest: [0; 32],
        source: capture.source,
        replay_file_sha256: capture.replay_file_sha256,
        executable_sha256: capture.executable_sha256,
        setup_composition_digest: setup.canonical_composition_digest,
        command_entry_composition_digest: chronology.composition_digest,
        command_frame: post_command.world.frame,
        command_entry_sim_sha256: capture.command_entry_sim_sha256,
        post_command_sim_sha256: capture.post_command_sim_sha256,
        world_checksum,
        random_state,
        setup_members,
        starting_builds,
        mount,
    };
    authority.composition_digest = frame1_post_command_composition_digest(&authority);
    Ok(authority)
}

/// Revalidate a post-command authority against the supplied immutable Sim.  Downstream frame-1
/// process lanes use this instead of duplicating the full-save, World/RNG, setup-member, and
/// Village/Market inventory gates.
pub fn validate_frame1_post_command_authority(
    setup_entry: &Frame379SetupEntryReceipt,
    authority: &Frame1PostCommandAuthority,
    sim: &Sim,
) -> Result<(), Frame1GoldenBindError> {
    if authority.revision == 0 || authority.composition_digest == [0; 32] {
        return Err(Frame1GoldenBindError::MissingAuthorityDigest);
    }
    if authority.source != Frame1PostCommandSource::CompleteRetailSerialOneLeaderOptionsReturn {
        return Err(Frame1GoldenBindError::WrongCaptureSource);
    }
    if authority.replay_file_sha256 != REPLAY_FILE_SHA256 {
        return Err(Frame1GoldenBindError::ReplayMismatch);
    }
    if authority.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256 {
        return Err(Frame1GoldenBindError::UnsupportedExecutable);
    }
    if authority.composition_digest != frame1_post_command_composition_digest(authority) {
        return Err(Frame1GoldenBindError::Frame1AuthorityMismatch);
    }
    if sim.world.frame != LEADER_OPTIONS_FRAME as i32 {
        return Err(Frame1GoldenBindError::WrongFrame {
            expected: LEADER_OPTIONS_FRAME as i32,
            actual: sim.world.frame,
        });
    }
    let bytes = save_sim(sim).map_err(Frame1GoldenBindError::Snapshot)?;
    if sha256(&bytes) != authority.post_command_sim_sha256
        || sim.map.world.checksum_sections() != authority.world_checksum
        || sim.world.random.state() != authority.random_state
        || starting_build_inventory(setup_entry, sim)? != authority.starting_builds
    {
        return Err(Frame1GoldenBindError::Frame1AuthorityMismatch);
    }
    Ok(())
}

/// Admit the earliest post-command tick oracle.  This is intentionally a retail capture
/// boundary, not a claim that every `do_frame` child is already reconstructed offline.
pub fn bind_captured_frame2_post_tick(
    replay: &Replay,
    setup_entry: &Frame379SetupEntryReceipt,
    setup: &Frame379SetupReceipt,
    frame1: &Frame1PostCommandAuthority,
    post_command: &Sim,
    checkpoint: &Sim,
    capture: &Frame2PostTickCapture,
) -> Result<Frame2PostTickAuthority, Frame1GoldenBindError> {
    let plan = plan_frame1_setup_leader_options(replay)?;
    if plan.source.next_sim_frame <= FIRST_POST_COMMAND_TICK_FRAME {
        return Err(Frame1GoldenBindError::InterveningSimCommand);
    }
    if capture.revision == 0 {
        return Err(Frame1GoldenBindError::MissingCaptureRevision);
    }
    if capture.source != Frame2PostTickSource::CompleteRetailPostCommandFrameOneDoFrame {
        return Err(Frame1GoldenBindError::WrongCaptureSource);
    }
    if capture.replay_file_sha256 != REPLAY_FILE_SHA256
        || frame1.replay_file_sha256 != REPLAY_FILE_SHA256
        || setup.replay_file_sha256 != REPLAY_FILE_SHA256
    {
        return Err(Frame1GoldenBindError::ReplayMismatch);
    }
    if capture.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256 {
        return Err(Frame1GoldenBindError::UnsupportedExecutable);
    }
    if frame1.setup_composition_digest != setup.canonical_composition_digest {
        return Err(Frame1GoldenBindError::SetupMismatch);
    }
    validate_frame1_post_command_authority(setup_entry, frame1, post_command)?;
    if checkpoint.world.frame != FIRST_POST_COMMAND_TICK_FRAME {
        return Err(Frame1GoldenBindError::WrongFrame {
            expected: FIRST_POST_COMMAND_TICK_FRAME,
            actual: checkpoint.world.frame,
        });
    }
    let post_bytes = save_sim(post_command).map_err(Frame1GoldenBindError::Snapshot)?;
    if sha256(&post_bytes) != capture.post_command_sim_sha256
        || capture.post_command_sim_sha256 != frame1.post_command_sim_sha256
    {
        return Err(Frame1GoldenBindError::PostCommandSnapshotMismatch);
    }
    let checkpoint_bytes = save_sim(checkpoint).map_err(Frame1GoldenBindError::Snapshot)?;
    if sha256(&checkpoint_bytes) != capture.checkpoint_sim_sha256 {
        return Err(Frame1GoldenBindError::PostCommandSnapshotMismatch);
    }
    let setup_members = bind_members(
        replay,
        setup,
        checkpoint,
        capture.revision,
        capture.checkpoint_sim_sha256,
    )?;
    if setup_members != frame1.setup_members {
        return Err(Frame1GoldenBindError::SetupIdentityChanged);
    }
    let starting_builds = starting_build_inventory(setup_entry, checkpoint)?;
    if starting_builds != frame1.starting_builds {
        return Err(Frame1GoldenBindError::StartingBuildMismatch);
    }
    let world_checksum = checkpoint.map.world.checksum_sections();
    let random_state = checkpoint.world.random.state();
    let mut authority = Frame2PostTickAuthority {
        revision: capture.revision,
        composition_digest: [0; 32],
        source: capture.source,
        replay_file_sha256: capture.replay_file_sha256,
        executable_sha256: capture.executable_sha256,
        setup_composition_digest: setup.canonical_composition_digest,
        frame1_post_command_digest: frame1.composition_digest,
        entry_frame: post_command.world.frame,
        checkpoint_frame: checkpoint.world.frame,
        post_command_sim_sha256: capture.post_command_sim_sha256,
        checkpoint_sim_sha256: capture.checkpoint_sim_sha256,
        world_checksum,
        random_state,
        setup_members,
        starting_builds,
    };
    authority.composition_digest = frame2_post_tick_composition_digest(&authority);
    Ok(authority)
}

/// Revalidate the mandatory frame-2 checkpoint for consumers which do not own setup replay.
pub fn validate_frame2_post_tick_authority(
    setup_entry: &Frame379SetupEntryReceipt,
    authority: &Frame2PostTickAuthority,
    sim: &Sim,
) -> Result<(), Frame1GoldenBindError> {
    if authority.revision == 0 || authority.composition_digest == [0; 32] {
        return Err(Frame1GoldenBindError::MissingAuthorityDigest);
    }
    if authority.source != Frame2PostTickSource::CompleteRetailPostCommandFrameOneDoFrame {
        return Err(Frame1GoldenBindError::WrongCaptureSource);
    }
    if authority.replay_file_sha256 != REPLAY_FILE_SHA256 {
        return Err(Frame1GoldenBindError::ReplayMismatch);
    }
    if authority.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256 {
        return Err(Frame1GoldenBindError::UnsupportedExecutable);
    }
    if authority.composition_digest != frame2_post_tick_composition_digest(authority) {
        return Err(Frame1GoldenBindError::Frame1AuthorityMismatch);
    }
    if sim.world.frame != FIRST_POST_COMMAND_TICK_FRAME {
        return Err(Frame1GoldenBindError::WrongFrame {
            expected: FIRST_POST_COMMAND_TICK_FRAME,
            actual: sim.world.frame,
        });
    }
    let bytes = save_sim(sim).map_err(Frame1GoldenBindError::Snapshot)?;
    if sha256(&bytes) != authority.checkpoint_sim_sha256
        || sim.map.world.checksum_sections() != authority.world_checksum
        || sim.world.random.state() != authority.random_state
        || starting_build_inventory(setup_entry, sim)? != authority.starting_builds
    {
        return Err(Frame1GoldenBindError::Frame1AuthorityMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup_2024_frame1_leader_options::Frame1CommandEntrySource;
    use crate::setup_2024_frame379::{Frame379CompletedInitSource, Frame379SetupEntrySource};
    use don_sim::systems::map_terrain::{SectionDigest, WorldSection};

    fn hash(byte: u8) -> [u8; 32] {
        [byte; 32]
    }

    fn manifest() -> GoldenCaptureManifest {
        let images: [[u8; 32]; 11] = std::array::from_fn(|index| hash(index as u8 + 1));
        GoldenCaptureManifest {
            schema_version: GOLDEN_CAPTURE_SCHEMA_VERSION,
            starting_market_o: DUTCH_STARTING_MARKET_O,
            starting_market_type: DUTCH_STARTING_MARKET_TYPE,
            setup_entry: Frame379SetupEntryCapture {
                revision: 1,
                source: Frame379SetupEntrySource::CompleteRetailBuildUnitsEntry,
                replay_file_sha256: REPLAY_FILE_SHA256,
                executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
                entry_sim_sha256: images[0],
                post_place_all_world_checksum: WorldChecksum {
                    per_section: [SectionDigest::default(); WorldSection::COUNT],
                    full: 0,
                    bytes: 0,
                },
                post_place_all_random_state: 1,
                terrain_source_digest: hash(40),
                mountain_height_receipt_sha256: hash(41),
            },
            completed_inits: std::array::from_fn(|ordinal| Frame379CompletedInitCapture {
                revision: 1,
                source: Frame379CompletedInitSource::CompleteRetailObjectsInitUnitReceiver,
                replay_file_sha256: REPLAY_FILE_SHA256,
                executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
                setup_ordinal: ordinal,
                before_sim_sha256: images[ordinal],
                after_sim_sha256: images[ordinal + 1],
                detailed_receipt_sha256: hash(50 + ordinal as u8),
            }),
            frame1_entry: Frame1CommandEntryCapture {
                revision: 1,
                source:
                    Frame1CommandEntrySource::AuthoritativePlaybackChronologyFrameZeroThroughOne,
                replay_file_sha256: REPLAY_FILE_SHA256,
                executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
                setup_sim_sha256: images[7],
                command_entry_sim_sha256: images[8],
            },
            frame1_post_command: Frame1PostCommandCapture {
                revision: 1,
                source: Frame1PostCommandSource::CompleteRetailSerialOneLeaderOptionsReturn,
                replay_file_sha256: REPLAY_FILE_SHA256,
                executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
                command_entry_sim_sha256: images[8],
                post_command_sim_sha256: images[9],
            },
            frame2_post_tick: Frame2PostTickCapture {
                revision: 1,
                source: Frame2PostTickSource::CompleteRetailPostCommandFrameOneDoFrame,
                replay_file_sha256: REPLAY_FILE_SHA256,
                executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
                post_command_sim_sha256: images[9],
                checkpoint_sim_sha256: images[10],
            },
        }
    }

    #[test]
    fn minimal_manifest_is_exactly_eleven_adjacent_sim_images() {
        assert_eq!(MINIMAL_UNIQUE_SIM_SNAPSHOTS, 11);
        validate_golden_capture_manifest(&manifest()).unwrap();
    }

    #[test]
    fn manifest_rejects_pre_market_schema_and_broken_adjacency() {
        let mut candidate = manifest();
        candidate.schema_version = 1;
        assert_eq!(
            validate_golden_capture_manifest(&candidate),
            Err(GoldenCaptureManifestError::WrongSchemaVersion)
        );

        let mut candidate = manifest();
        candidate.starting_market_o = -1;
        assert_eq!(
            validate_golden_capture_manifest(&candidate),
            Err(GoldenCaptureManifestError::MissingStartingMarketContract)
        );

        let mut candidate = manifest();
        candidate.completed_inits[3].before_sim_sha256 = hash(99);
        assert_eq!(
            validate_golden_capture_manifest(&candidate),
            Err(GoldenCaptureManifestError::BrokenReceiverChain { ordinal: 3 })
        );

        let mut candidate = manifest();
        candidate.frame1_entry.setup_sim_sha256 = hash(98);
        assert_eq!(
            validate_golden_capture_manifest(&candidate),
            Err(GoldenCaptureManifestError::BrokenFrame1EntryLink)
        );
    }
}
