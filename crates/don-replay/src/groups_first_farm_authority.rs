//! Replay-pinned authority discovery for the first 2018 opcode-25 Farm transaction.
//!
//! The recording carries the exact command, checksum tuple, setup selectors, static Rules,
//! and first-camera City center. It does not carry the generated World or the post-worldgen
//! RNG state from which `Setup::place_unit` created object 4. This adapter advances every
//! source-owned stage and stops at that first absent state rather than manufacturing a Unit,
//! Build body, blocked-site result, or swarm search after-image.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::systems::canonical_group_move_host::{UnitIdentity, UnitImage};
use don_sim::systems::map_terrain::{Coord, WCoord};
use don_sim::systems::objects_init_unit_authority_frontier::{
    BhsInitUnitRequest, DetailedInitUnitReceipt, InitUnitReceiptError, InitUnitStep,
    ValidatedInitUnitEffects,
};
use don_sim::systems::production::{self, Footprint};
use don_sim::systems::sparse_object_bands_authority_frontier::{RetailBand, RetailObjectAddress};
use don_sim::tick::Sim;
use don_sim::world::{WorldObjectIdentity, OBJ_FLAG_ACTIVE};

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
use crate::setup_place_unit_deep_re::{
    produce_place_unit_probe_prefix, produce_unit_guy_init_prefix, CenterBuildFacts,
    GuyGraphicsInitReceipt, GuyInitPredicateFacts, ObjectsInitUnitRequest,
    PlaceUnitExternalResidual, PlaceUnitInputs, PlaceUnitProducerError, PlaceUnitProducerReceipt,
    PlacementMapSnapshot, PlacementTileFacts, StableUnitIdentity, UnitGuyInitError,
    UnitGuyInitInputs, UnitGuyInitPrefixReceipt,
};
use crate::setup_units_producer::{
    starting_citizen_counts, validate_build_units_prefix_receipt, BuildUnitsPlan,
    BuildUnitsPrefixReceipt, BuildUnitsReceiptError, PlacementOutcomeReceipt, PlacementRngEvent,
    StableUnitIdentityReceipt, StartingUnitPhase, CITIZEN_SIMPLE_CALL_VA, SCOUT_BASE_CALL_VA,
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
pub const FIRST_BUILDER_SETUP_ORDINAL: usize = 4;
pub const FIRST_SETUP_PLACEMENT_ORDINAL: usize = 0;

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

/// External provenance admitted at the first `Setup::place_unit` call.
///
/// The replay does not contain this state. The source means that an external canonical map
/// transaction completed all world-generation calls, then executed the starting-City setup in
/// native order. Merely reconstructing the replay-prefix World is not this source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirstFarmSetupEntrySource {
    CompletedCanonicalWorldgenAndStartingCitySetup,
}

/// Revisioned external authority for the canonical setup-entry Sim.
///
/// `world_checksum` binds every synchronized map owner inspected by the placement probe. The RNG
/// state is checked independently against the canonical object World because main RNG is not part
/// of the map checksum. `composition_digest` attests the omitted transaction receipts and must be
/// independently produced; this adapter never derives it from a caller-created snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirstFarmSetupEntryAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub source: FirstFarmSetupEntrySource,
    pub world_checksum: don_sim::systems::map_terrain::WorldChecksum,
    pub post_worldgen_rng: i32,
}

/// Exact source-produced first setup placement through its first unexecuted native mutation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirstFarmFirstPlacementReceipt {
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub source: FirstFarmSetupEntrySource,
    pub replay_file_sha256: [u8; 32],
    pub setup_ordinal: usize,
    pub map_checksum: don_sim::systems::map_terrain::WorldChecksum,
    pub placement_snapshot_sha256: [u8; 32],
    pub center_row: usize,
    pub post_worldgen_rng: i32,
    pub placement: PlaceUnitProducerReceipt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FirstFarmFirstPlacementError {
    Discovery(FirstFarmAuthorityError),
    MissingAuthorityRevision,
    MissingCompositionDigest,
    WrongFrame { expected: i32, actual: i32 },
    WrongSetupPlan,
    WorldChecksumMismatch,
    PostWorldgenRngMismatch,
    RegistryNotDenseEquivalent,
    ExistingOwnerUnits { mark: i32 },
    MissingCenterBuild,
    CenterBuildRowOutOfRange,
    CenterBuildMismatch,
    MapDimensionMismatch,
    MapShapeOverflow,
    Placement(PlaceUnitProducerError),
}

impl fmt::Display for FirstFarmFirstPlacementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "first 2018 Farm setup placement refused: {self:?}")
    }
}

impl std::error::Error for FirstFarmFirstPlacementError {}

impl From<FirstFarmAuthorityError> for FirstFarmFirstPlacementError {
    fn from(value: FirstFarmAuthorityError) -> Self {
        Self::Discovery(value)
    }
}

impl From<PlaceUnitProducerError> for FirstFarmFirstPlacementError {
    fn from(value: PlaceUnitProducerError) -> Self {
        Self::Placement(value)
    }
}

/// External provenance for the complete first Scout `Objects::init_unit` body and its
/// canonical after-image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirstFarmFirstInitUnitSource {
    CompleteRetailBodyAndCanonicalAfterImage,
}

/// Revisioned authority for effects below the first placement residual.
///
/// This is intentionally not constructible from `World::allocate_typed_at` alone. The digest
/// must attest the complete retail 1,603-byte receiver, including its nested 3,732-byte
/// `Unit::init`, Guy/graphics/RNG work, collision, visibility, Leader accounting, and object-band
/// publication. The explicit after checksum and RNG are independently checked here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirstFarmFirstInitUnitAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub source: FirstFarmFirstInitUnitSource,
    pub map_checksum_after: don_sim::systems::map_terrain::WorldChecksum,
    pub rng_after: i32,
}

/// Read-only binding of the complete retail receipt to the canonical first Scout row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirstFarmFirstInitUnitReceipt {
    pub placement_authority_revision: u64,
    pub placement_authority_digest: [u8; 32],
    pub init_authority_revision: u64,
    pub init_authority_digest: [u8; 32],
    pub source: FirstFarmFirstInitUnitSource,
    pub request: ObjectsInitUnitRequest,
    pub effects: ValidatedInitUnitEffects,
    pub rng_before: i32,
    pub rng_after: i32,
    pub map_checksum_after: don_sim::systems::map_terrain::WorldChecksum,
    pub allocation: StableUnitIdentityReceipt,
    pub row: usize,
    pub unit: UnitImage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FirstFarmFirstInitUnitError {
    Placement(FirstFarmFirstPlacementError),
    MissingAuthorityRevision,
    MissingCompositionDigest,
    PlacementDidNotReachObjectsInitUnit,
    InitRequestMismatch,
    WrongTypeFacts,
    DetailedReceipt(InitUnitReceiptError),
    WrongEffects,
    WrongAfterFrame { expected: i32, actual: i32 },
    AfterMapChecksumMismatch,
    AfterRngMismatch,
    WrongAfterUnitMark { expected: i32, actual: i32 },
    MissingCanonicalUnit,
    InactiveCanonicalUnit,
    MissingCanonicalHandle,
    MissingCanonicalType,
    CanonicalAfterImageMismatch,
    MissingCanonicalPath,
}

impl fmt::Display for FirstFarmFirstInitUnitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "first 2018 Farm Objects::init_unit receipt refused: {self:?}"
        )
    }
}

impl std::error::Error for FirstFarmFirstInitUnitError {}

impl From<FirstFarmFirstPlacementError> for FirstFarmFirstInitUnitError {
    fn from(value: FirstFarmFirstPlacementError) -> Self {
        Self::Placement(value)
    }
}

impl From<InitUnitReceiptError> for FirstFarmFirstInitUnitError {
    fn from(value: InitUnitReceiptError) -> Self {
        Self::DetailedReceipt(value)
    }
}

/// Exact source-produced Guy allocation/initializer segment nested inside the first Scout.
///
/// The complete initializer receipt remains attached because native `(owner,o)` is not a stable
/// host identity by itself. The prefix is the independently reproduced synchronized Guy image,
/// including its two game-RNG draws; it is not an assertion that graphics or the 2018 setup state
/// can be recovered from replay bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct FirstFarmFirstScoutGuyReceipt {
    pub init: FirstFarmFirstInitUnitReceipt,
    pub squad_size: i32,
    pub crew_size: i32,
    pub prefix: UnitGuyInitPrefixReceipt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FirstFarmFirstScoutGuyError {
    Init(FirstFarmFirstInitUnitError),
    Discovery(FirstFarmAuthorityError),
    WrongScoutGuyCounts,
    Guy(UnitGuyInitError),
    /// The first Scout's only game-RNG calls inside the complete initializer are the two direct
    /// draws in `Guy::init_real`. A disagreement therefore invalidates either the graphics-bound
    /// prefix or the externally attested complete-body chronology.
    InitializerRngMismatch {
        prefix_after: i32,
        init_after: i32,
    },
}

impl fmt::Display for FirstFarmFirstScoutGuyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "first 2018 Farm Scout Guy prefix refused: {self:?}")
    }
}

impl std::error::Error for FirstFarmFirstScoutGuyError {}

impl From<FirstFarmFirstInitUnitError> for FirstFarmFirstScoutGuyError {
    fn from(value: FirstFarmFirstInitUnitError) -> Self {
        Self::Init(value)
    }
}

impl From<FirstFarmAuthorityError> for FirstFarmFirstScoutGuyError {
    fn from(value: FirstFarmAuthorityError) -> Self {
        Self::Discovery(value)
    }
}

impl From<UnitGuyInitError> for FirstFarmFirstScoutGuyError {
    fn from(value: UnitGuyInitError) -> Self {
        Self::Guy(value)
    }
}

/// External state provenance admitted by the frame-79 join.
///
/// This is intentionally narrower than "loaded Sim": the state must be the output of the
/// validated setup receipts followed by the complete canonical frame/package schedule up to the
/// first opcode-25 package stamp. The join checks every owner it can inspect, but it cannot turn a
/// caller-created frame-79 snapshot into retail chronology merely because its scalar frame is 79.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirstFarmFrame79Source {
    ValidatedSetupAndCanonicalReplayExecution,
}

/// Revisioned external authority for the canonical frame-79 Sim supplied to the builder join.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FirstFarmFrame79Authority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub source: FirstFarmFrame79Source,
}

/// Exact read-only join of the fifth setup allocation to the live canonical Unit row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirstFarmBuilderBindingReceipt {
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub source: FirstFarmFrame79Source,
    pub setup_ordinal: usize,
    pub allocation: StableUnitIdentityReceipt,
    pub frame: i32,
    pub row: usize,
    pub current_type: i32,
    pub unit: UnitImage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FirstFarmBuilderBindingError {
    Discovery(FirstFarmAuthorityError),
    MissingAuthorityRevision,
    MissingCompositionDigest,
    WrongFrame { expected: i32, actual: i32 },
    WrongSetupPlan,
    SetupReceipt(BuildUnitsReceiptError),
    MissingDirectPlacementDraw { ordinal: usize },
    WrongAllocationSequence { ordinal: usize },
    MissingBuilderAllocation,
    MissingCanonicalUnit,
    InactiveCanonicalUnit,
    MissingCanonicalHandle,
    StaleCanonicalHandle,
    MissingCanonicalType,
    CanonicalTypeMismatch,
    MissingCanonicalPath,
}

impl fmt::Display for FirstFarmBuilderBindingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "first 2018 Farm builder binding refused: {self:?}")
    }
}

impl std::error::Error for FirstFarmBuilderBindingError {}

impl From<FirstFarmAuthorityError> for FirstFarmBuilderBindingError {
    fn from(value: FirstFarmAuthorityError) -> Self {
        Self::Discovery(value)
    }
}

impl From<BuildUnitsReceiptError> for FirstFarmBuilderBindingError {
    fn from(value: BuildUnitsReceiptError) -> Self {
        Self::SetupReceipt(value)
    }
}

fn strict_first_builder_plan(
    plan: &BuildUnitsPlan,
    discovery: &FirstFarmAuthorityDiscovery,
) -> bool {
    if plan.stop.is_some()
        || plan.calls.len() != FIRST_BUILDER_SETUP_ORDINAL + 1
        || plan.citizens_after_modifiers != 4
    {
        return false;
    }
    plan.calls.iter().enumerate().all(|(ordinal, call)| {
        let expected_phase = if ordinal == 0 {
            StartingUnitPhase::BaseScout
        } else {
            StartingUnitPhase::Citizen {
                index: ordinal as i32 - 1,
            }
        };
        let expected_type = if ordinal == 0 {
            discovery.scout
        } else {
            discovery.citizen
        };
        call.ordinal == ordinal as u32
            && call.owner == i32::from(FIRST_OWNER)
            && call.center_city_o == discovery.center_build_o as i32
            && call.phase == expected_phase
            && call.call_va
                == if ordinal == 0 {
                    SCOUT_BASE_CALL_VA
                } else {
                    CITIZEN_SIMPLE_CALL_VA
                }
            && call.place_unit_upgrade
                == if ordinal == 0 {
                    discovery.scout.type_index
                } else {
                    discovery.citizen.type_index
                }
            && call.uber_size == expected_type.uber_size
            && call.squad_size == expected_type.squad_size
            && call.crew_size == expected_type.crew_size
    })
}

fn capture_placement_map(
    sim: &Sim,
) -> Result<(PlacementMapSnapshot, [u8; 32]), FirstFarmFirstPlacementError> {
    let world = &sim.map.world;
    if world.xs <= 0
        || world.ys <= 0
        || world.tile_xs != world.xs.checked_mul(4).unwrap_or(i32::MIN)
        || world.tile_ys != world.ys.checked_mul(4).unwrap_or(i32::MIN)
    {
        return Err(FirstFarmFirstPlacementError::MapDimensionMismatch);
    }
    let cells = world
        .xs
        .checked_mul(world.ys)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or(FirstFarmFirstPlacementError::MapShapeOverflow)?;
    if world.wdata.len() != cells {
        return Err(FirstFarmFirstPlacementError::MapDimensionMismatch);
    }

    let mut tiles = Vec::with_capacity(cells);
    let mut image = Vec::with_capacity(8 + cells.saturating_mul(9));
    image.extend_from_slice(&world.xs.to_le_bytes());
    image.extend_from_slice(&world.ys.to_le_bytes());
    for y in 0..world.ys {
        for x in 0..world.xs {
            let cell = &world.wdata[(y * world.xs + x) as usize];
            let collision = world.tmask(x * 4 + 2, y * 4 + 2);
            let tile = PlacementTileFacts {
                continent: cell.region as u16,
                flags: cell.flags,
                land: cell.land,
                occupied_o: cell.down,
                collision,
            };
            image.extend_from_slice(&tile.continent.to_le_bytes());
            image.extend_from_slice(&tile.flags.to_le_bytes());
            image.push(tile.land as u8);
            image.extend_from_slice(&tile.occupied_o.to_le_bytes());
            image.extend_from_slice(&tile.collision.to_le_bytes());
            tiles.push(tile);
        }
    }
    Ok((
        PlacementMapSnapshot {
            xs: world.xs,
            ys: world.ys,
            tiles,
        },
        sha256(&image),
    ))
}

/// Produce the exact first setup placement from a canonical post-worldgen Sim.
///
/// This is the first real state-consuming step toward the five allocation receipts. It captures
/// the placement WData/TData projection directly from canonical map owners, binds the starting
/// Village through the sparse Build band, and executes every `Setup::place_unit` RNG/probe gate.
/// The returned receipt still stops at `Objects::init_unit` (or the exact centered Build-train
/// fallback); it never invents an allocation, Unit Handle, or 2018 worldgen transcript.
pub fn produce_first_farm_first_placement(
    replay: &Replay,
    plan: &BuildUnitsPlan,
    sim: &Sim,
    authority: &FirstFarmSetupEntryAuthority,
) -> Result<FirstFarmFirstPlacementReceipt, FirstFarmFirstPlacementError> {
    if authority.revision == 0 {
        return Err(FirstFarmFirstPlacementError::MissingAuthorityRevision);
    }
    if authority.composition_digest == [0; 32] {
        return Err(FirstFarmFirstPlacementError::MissingCompositionDigest);
    }
    if sim.world.frame != 0 {
        return Err(FirstFarmFirstPlacementError::WrongFrame {
            expected: 0,
            actual: sim.world.frame,
        });
    }
    let discovery = discover_first_2018_farm(replay)?;
    if !strict_first_builder_plan(plan, &discovery) {
        return Err(FirstFarmFirstPlacementError::WrongSetupPlan);
    }
    let expected_edge = replay
        .initial
        .info
        .settings
        .map_edge_world_cells()
        .ok_or(FirstFarmFirstPlacementError::MapDimensionMismatch)?;
    if sim.map.world.xs != expected_edge || sim.map.world.ys != expected_edge {
        return Err(FirstFarmFirstPlacementError::MapDimensionMismatch);
    }
    let map_checksum = sim.map.world.checksum_sections();
    if map_checksum != authority.world_checksum {
        return Err(FirstFarmFirstPlacementError::WorldChecksumMismatch);
    }
    if sim.world.random.state() != authority.post_worldgen_rng {
        return Err(FirstFarmFirstPlacementError::PostWorldgenRngMismatch);
    }
    if !sim.world.object_bands_are_dense_equivalent() {
        return Err(FirstFarmFirstPlacementError::RegistryNotDenseEquivalent);
    }
    let unit_mark = sim.world.unit_mark(FIRST_OWNER as usize).unwrap_or(-1);
    if unit_mark != 0 {
        return Err(FirstFarmFirstPlacementError::ExistingOwnerUnits { mark: unit_mark });
    }

    let center_identity = sim
        .world
        .object_bands()
        .live_identity(RetailObjectAddress::new(
            FIRST_OWNER,
            RetailBand::Build,
            discovery.center_build_o as i32,
        ))
        .ok_or(FirstFarmFirstPlacementError::MissingCenterBuild)?;
    let WorldObjectIdentity::BuildRow(center_row) = center_identity else {
        return Err(FirstFarmFirstPlacementError::MissingCenterBuild);
    };
    let center_row = center_row as usize;
    let center = sim
        .builds
        .get(center_row)
        .ok_or(FirstFarmFirstPlacementError::CenterBuildRowOutOfRange)?;
    let center_type = sim
        .production_runtime
        .build_types
        .get(center_row)
        .and_then(|value| *value);
    if center.flags & production::flag::VALID == 0
        || center.who != FIRST_OWNER
        || center.object_id() != discovery.center_build_o
        || center.position() != discovery.center_position
        || center_type != Some(crate::setup_cities_builds::CITY_CENTER_TYPE)
    {
        return Err(FirstFarmFirstPlacementError::CenterBuildMismatch);
    }

    let (map, placement_snapshot_sha256) = capture_placement_map(sim)?;
    let call = plan.calls[FIRST_SETUP_PLACEMENT_ORDINAL];
    let placement = produce_place_unit_probe_prefix(
        PlaceUnitInputs {
            owner: call.owner,
            upgraded_type: call.place_unit_upgrade,
            requested_x: call.requested_x,
            requested_y: call.requested_y,
            center: Some(CenterBuildFacts {
                owner: i32::from(FIRST_OWNER),
                o: discovery.center_build_o as i32,
                x: discovery.center_position.0,
                y: discovery.center_position.1,
            }),
            starting_town: replay.initial.info.settings.starting_town,
            leader_active: 0,
        },
        &map,
        authority.post_worldgen_rng,
    )?;

    Ok(FirstFarmFirstPlacementReceipt {
        authority_revision: authority.revision,
        authority_digest: authority.composition_digest,
        source: authority.source,
        replay_file_sha256: discovery.replay_file_sha256,
        setup_ordinal: FIRST_SETUP_PLACEMENT_ORDINAL,
        map_checksum,
        placement_snapshot_sha256,
        center_row,
        post_worldgen_rng: authority.post_worldgen_rng,
        placement,
    })
}

/// Bind the first exact placement residual to a complete retail `Objects::init_unit` receipt.
///
/// Both simulations are read-only inputs. `before` is re-run through the exact placement
/// producer; `after` must be the canonical state immediately after the supplied complete retail
/// receipt. This adapter validates chronology and publishes a stable identity/image receipt, but
/// it does not execute the nested initializer or construct Guy/graphics bytes locally.
#[allow(clippy::too_many_arguments)]
pub fn bind_first_farm_first_init_unit(
    replay: &Replay,
    plan: &BuildUnitsPlan,
    before: &Sim,
    placement_authority: &FirstFarmSetupEntryAuthority,
    detailed: &DetailedInitUnitReceipt,
    after: &Sim,
    init_authority: &FirstFarmFirstInitUnitAuthority,
) -> Result<FirstFarmFirstInitUnitReceipt, FirstFarmFirstInitUnitError> {
    if init_authority.revision == 0 {
        return Err(FirstFarmFirstInitUnitError::MissingAuthorityRevision);
    }
    if init_authority.composition_digest == [0; 32] {
        return Err(FirstFarmFirstInitUnitError::MissingCompositionDigest);
    }
    let placement = produce_first_farm_first_placement(replay, plan, before, placement_authority)?;
    let PlaceUnitExternalResidual::ObjectsInitUnit(request) =
        placement.placement.first_external_residual
    else {
        return Err(FirstFarmFirstInitUnitError::PlacementDidNotReachObjectsInitUnit);
    };
    if request.come_out_zero_after_success
        || detailed.request
            != (BhsInitUnitRequest {
                owner: request.owner,
                type_index: request.type_index,
                x: request.x,
                y: request.y,
                exact_o: request.exact_o,
                external_previous: request.external_previous,
                external_next: request.external_next,
            })
    {
        return Err(FirstFarmFirstInitUnitError::InitRequestMismatch);
    }
    let discovery = discover_first_2018_farm(replay).map_err(FirstFarmFirstPlacementError::from)?;
    if detailed.type_facts.uber_size != discovery.scout.uber_size || discovery.scout.uber_size != 1
    {
        return Err(FirstFarmFirstInitUnitError::WrongTypeFacts);
    }
    let effects = detailed.validate()?;
    if effects.initialized_members.as_slice() != [0]
        || effects.terminal_find_free_failure.is_some()
        || effects.returned_captain_or_failure != 0
        || effects.unit_mark_before != 0
        || effects.unit_mark_after != 1
    {
        return Err(FirstFarmFirstInitUnitError::WrongEffects);
    }
    if after.world.frame != 0 {
        return Err(FirstFarmFirstInitUnitError::WrongAfterFrame {
            expected: 0,
            actual: after.world.frame,
        });
    }
    let map_checksum_after = after.map.world.checksum_sections();
    if map_checksum_after != init_authority.map_checksum_after {
        return Err(FirstFarmFirstInitUnitError::AfterMapChecksumMismatch);
    }
    if after.world.random.state() != init_authority.rng_after {
        return Err(FirstFarmFirstInitUnitError::AfterRngMismatch);
    }
    let mark = after.world.unit_mark(FIRST_OWNER as usize).unwrap_or(-1);
    if mark != 1 {
        return Err(FirstFarmFirstInitUnitError::WrongAfterUnitMark {
            expected: 1,
            actual: mark,
        });
    }
    let row = after
        .world
        .unit_row_at(i32::from(FIRST_OWNER), 0)
        .ok_or(FirstFarmFirstInitUnitError::MissingCanonicalUnit)?;
    if after.world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
        return Err(FirstFarmFirstInitUnitError::InactiveCanonicalUnit);
    }
    let handle = after
        .world
        .handle_at_row(row)
        .ok_or(FirstFarmFirstInitUnitError::MissingCanonicalHandle)?;
    let world_type = after
        .world
        .unit_type_id(row)
        .ok_or(FirstFarmFirstInitUnitError::MissingCanonicalType)?;
    let sim_type = after
        .unit_type
        .get(row)
        .copied()
        .ok_or(FirstFarmFirstInitUnitError::MissingCanonicalType)?;
    let unit_init_after = detailed.steps.iter().find_map(|step| match step {
        InitUnitStep::UnitInit(receipt) => Some(receipt.after),
        _ => None,
    });
    let final_captain = detailed.steps.last().and_then(|step| match step {
        InitUnitStep::ResolveCaptain(receipt) => Some(receipt.captain),
        _ => None,
    });
    let current_x = after.world.units.x_internal()[row];
    let current_y = after.world.units.y_internal()[row];
    let current_angle = after.world.units.angle()[row];
    if world_type != discovery.scout.type_index
        || sim_type != world_type
        || unit_init_after
            != Some(
                don_sim::systems::objects_init_unit_authority_frontier::UnitAfterInit {
                    owner: i32::from(FIRST_OWNER),
                    o: 0,
                    type_index: world_type,
                    x: current_x,
                    y: current_y,
                    angle: current_angle,
                    unit_masks: after.world.units.get_unit_masks(row),
                },
            )
        || final_captain.is_none_or(|captain| {
            captain.owner != i32::from(FIRST_OWNER)
                || captain.o != 0
                || captain.x != current_x
                || captain.y != current_y
                || captain.angle != current_angle
        })
        || !after.world.orders(row).is_empty()
    {
        return Err(FirstFarmFirstInitUnitError::CanonicalAfterImageMismatch);
    }
    let path = after
        .paths
        .get(row)
        .cloned()
        .ok_or(FirstFarmFirstInitUnitError::MissingCanonicalPath)?;
    if !path.is_empty() {
        return Err(FirstFarmFirstInitUnitError::CanonicalAfterImageMismatch);
    }
    let unit = UnitImage {
        identity: UnitIdentity {
            handle,
            who: FIRST_OWNER,
            o: 0,
            uid: after.world.units.get_uid(row),
        },
        group: after.world.units.group()[row],
        unit_masks: after.world.units.get_unit_masks(row),
        form: after.world.units.form()[row],
        form_mod: after.world.units.form_mod()[row],
        angle: after.world.units.angle()[row],
        x: after.world.units.x_internal()[row],
        y: after.world.units.y_internal()[row],
        orders_x: after.world.units.orders_x()[row],
        orders_y: after.world.units.orders_y()[row],
        dest_angle: after.world.units.dest_angle()[row],
        orders: after.world.orders(row).clone(),
        path,
    };
    Ok(FirstFarmFirstInitUnitReceipt {
        placement_authority_revision: placement.authority_revision,
        placement_authority_digest: placement.authority_digest,
        init_authority_revision: init_authority.revision,
        init_authority_digest: init_authority.composition_digest,
        source: init_authority.source,
        request,
        effects,
        rng_before: placement.placement.rng_after_probes,
        rng_after: init_authority.rng_after,
        map_checksum_after,
        allocation: StableUnitIdentityReceipt {
            id: handle.id,
            generation: handle.generation,
            owner: i32::from(FIRST_OWNER),
            o: 0,
        },
        row,
        unit,
    })
}

/// Reproduce the exact `Unit::init` Guy-allocation prefix for the first setup Scout.
///
/// This composes, rather than replaces, the complete externally authorized initializer join. The
/// replay-carried Rules row supplies the exact one-squad/one-crew shape; the caller must supply
/// coherent installed-graphics extraction and PE predicate receipts for both Guys. The
/// producer begins at the placement receipt's `Objects::init_unit` RNG state and requires its
/// independently computed end state to equal the complete initializer after-image authority.
///
/// No Unit/Guy state is written here. In particular, a caller-created `Sim`, guessed gpiece, or
/// checksum match cannot manufacture either of the two upstream authority digests.
#[allow(clippy::too_many_arguments)]
pub fn produce_first_farm_first_scout_guy_prefix(
    replay: &Replay,
    plan: &BuildUnitsPlan,
    before: &Sim,
    placement_authority: &FirstFarmSetupEntryAuthority,
    detailed: &DetailedInitUnitReceipt,
    after: &Sim,
    init_authority: &FirstFarmFirstInitUnitAuthority,
    graphics: Vec<GuyGraphicsInitReceipt>,
    predicates: Vec<GuyInitPredicateFacts>,
) -> Result<FirstFarmFirstScoutGuyReceipt, FirstFarmFirstScoutGuyError> {
    let init = bind_first_farm_first_init_unit(
        replay,
        plan,
        before,
        placement_authority,
        detailed,
        after,
        init_authority,
    )?;
    let discovery = discover_first_2018_farm(replay)?;
    if discovery.scout.squad_size != 1
        || discovery.scout.crew_size != 1
        || discovery.scout.uber_size != 1
    {
        return Err(FirstFarmFirstScoutGuyError::WrongScoutGuyCounts);
    }
    let prefix = produce_unit_guy_init_prefix(
        UnitGuyInitInputs {
            identity: StableUnitIdentity {
                id: init.allocation.id,
                generation: init.allocation.generation,
                owner: init.allocation.owner,
                o: init.allocation.o,
                type_index: init.request.type_index,
            },
            squad_size: discovery.scout.squad_size,
            crew_size: discovery.scout.crew_size,
            graphics,
            predicates,
        },
        init.rng_before,
    )?;
    if prefix.rng_after_guys != init.rng_after {
        return Err(FirstFarmFirstScoutGuyError::InitializerRngMismatch {
            prefix_after: prefix.rng_after_guys,
            init_after: init.rng_after,
        });
    }
    Ok(FirstFarmFirstScoutGuyReceipt {
        init,
        squad_size: discovery.scout.squad_size,
        crew_size: discovery.scout.crew_size,
        prefix,
    })
}

/// Bind the exact setup-produced object 4 to its current canonical Unit/Handle at frame 79.
///
/// This function is read-only. It first validates the complete five-call setup receipt and
/// requires every call to have spawned exactly one consecutive owner-0 captain (`o=0..4`). It
/// then resolves the fifth allocation's generational identity against `Sim::world`, requires the
/// Unit to remain active with the same type, and captures the full current order/path image. A
/// stale/recycled object-4 slot therefore cannot pass by address alone.
pub fn bind_first_farm_builder_at_frame79(
    replay: &Replay,
    plan: &BuildUnitsPlan,
    setup: &BuildUnitsPrefixReceipt,
    sim: &Sim,
    authority: FirstFarmFrame79Authority,
) -> Result<FirstFarmBuilderBindingReceipt, FirstFarmBuilderBindingError> {
    if authority.revision == 0 {
        return Err(FirstFarmBuilderBindingError::MissingAuthorityRevision);
    }
    if authority.composition_digest == [0; 32] {
        return Err(FirstFarmBuilderBindingError::MissingCompositionDigest);
    }
    if sim.world.frame != FIRST_FRAME {
        return Err(FirstFarmBuilderBindingError::WrongFrame {
            expected: FIRST_FRAME,
            actual: sim.world.frame,
        });
    }
    let discovery = discover_first_2018_farm(replay)?;
    if !strict_first_builder_plan(plan, &discovery) {
        return Err(FirstFarmBuilderBindingError::WrongSetupPlan);
    }
    validate_build_units_prefix_receipt(plan, setup)?;

    let mut builder_allocation = None;
    for (ordinal, placement) in setup.placements.iter().enumerate() {
        if !placement
            .rng_events
            .iter()
            .any(|event| matches!(event, PlacementRngEvent::DirectOffset(_)))
        {
            return Err(FirstFarmBuilderBindingError::MissingDirectPlacementDraw { ordinal });
        }
        let PlacementOutcomeReceipt::Spawned(init) = &placement.outcome else {
            return Err(FirstFarmBuilderBindingError::WrongAllocationSequence { ordinal });
        };
        let [member] = init.members.as_slice() else {
            return Err(FirstFarmBuilderBindingError::WrongAllocationSequence { ordinal });
        };
        if init.returned_captain_o != ordinal as i32
            || member.identity.owner != i32::from(FIRST_OWNER)
            || member.identity.o != ordinal as i32
            || member.ptype_index != plan.calls[ordinal].place_unit_upgrade
        {
            return Err(FirstFarmBuilderBindingError::WrongAllocationSequence { ordinal });
        }
        if ordinal == FIRST_BUILDER_SETUP_ORDINAL {
            builder_allocation = Some(member.identity);
        }
    }
    let allocation =
        builder_allocation.ok_or(FirstFarmBuilderBindingError::MissingBuilderAllocation)?;
    let row = sim
        .world
        .unit_row_at(i32::from(FIRST_OWNER), i32::from(FIRST_SELECTED_O))
        .ok_or(FirstFarmBuilderBindingError::MissingCanonicalUnit)?;
    if sim.world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
        return Err(FirstFarmBuilderBindingError::InactiveCanonicalUnit);
    }
    let handle = sim
        .world
        .handle_at_row(row)
        .ok_or(FirstFarmBuilderBindingError::MissingCanonicalHandle)?;
    if (handle.id, handle.generation) != (allocation.id, allocation.generation) {
        return Err(FirstFarmBuilderBindingError::StaleCanonicalHandle);
    }
    let world_type = sim
        .world
        .unit_type_id(row)
        .ok_or(FirstFarmBuilderBindingError::MissingCanonicalType)?;
    let sim_type = sim
        .unit_type
        .get(row)
        .copied()
        .ok_or(FirstFarmBuilderBindingError::MissingCanonicalType)?;
    if world_type != discovery.citizen.type_index || sim_type != world_type {
        return Err(FirstFarmBuilderBindingError::CanonicalTypeMismatch);
    }
    let path = sim
        .paths
        .get(row)
        .cloned()
        .ok_or(FirstFarmBuilderBindingError::MissingCanonicalPath)?;
    let unit = UnitImage {
        identity: UnitIdentity {
            handle,
            who: FIRST_OWNER,
            o: FIRST_SELECTED_O,
            uid: sim.world.units.get_uid(row),
        },
        group: sim.world.units.group()[row],
        unit_masks: sim.world.units.get_unit_masks(row),
        form: sim.world.units.form()[row],
        form_mod: sim.world.units.form_mod()[row],
        angle: sim.world.units.angle()[row],
        x: sim.world.units.x_internal()[row],
        y: sim.world.units.y_internal()[row],
        orders_x: sim.world.units.orders_x()[row],
        orders_y: sim.world.units.orders_y()[row],
        dest_angle: sim.world.units.dest_angle()[row],
        orders: sim.world.orders(row).clone(),
        path,
    };
    Ok(FirstFarmBuilderBindingReceipt {
        authority_revision: authority.revision,
        authority_digest: authority.composition_digest,
        source: authority.source,
        setup_ordinal: FIRST_BUILDER_SETUP_ORDINAL,
        allocation,
        frame: sim.world.frame,
        row,
        current_type: world_type,
        unit,
    })
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
