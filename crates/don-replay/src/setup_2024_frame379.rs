//! Strict downstream setup authority for the shortest 2024 clear-Groups witness.
//!
//! The replay does not serialize the generated World or the post-worldgen RNG. This module
//! therefore begins at an explicit completed-worldgen/starting-Village authority and owns the
//! deterministic work immediately downstream of that seam: replay/Rules-derived setup planning,
//! all seven `Setup::place_unit` probe schedules, exact RNG/Unit-mark chronology, and the join to
//! one canonical frame-zero Sim after every `Objects::init_unit` receiver completed.
//!
//! No Unit is allocated here. Every completed call must arrive as an exact receipt whose stable
//! identity already exists in the canonical Sim. The output is consequently useful to Groups,
//! Cities, Units, and Guys without allowing an object range or recorded checksum to stand in for
//! actual setup execution.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::systems::map_terrain::{World, WorldChecksum};
use don_sim::systems::objects_init_unit_authority_frontier::{
    BhsInitUnitRequest, DetailedInitUnitReceipt, InitUnitReceiptError, InitUnitStep, UnitAfterInit,
};
use don_sim::tick::Sim;
use don_sim::world::{WorldObjectIdentity, OBJ_FLAG_ACTIVE};

use crate::groups_pre_pair_unit_authority::{
    replay_tribe_type_facts, replay_unit_type_facts, PrePairUnitAuthorityError, ReplayUnitTypeFacts,
};
use crate::replay::{load_payload, Replay};
use crate::setup_cities_builds::{
    CAMERA_COMMAND_OPCODE, CITY_CENTER_TYPE, VILLAGE_CENTER_OFFSET, WORLD_TO_COORD,
};
use crate::setup_place_unit_deep_re::{
    produce_place_unit_probe_prefix, CenterBuildFacts, PlaceUnitExternalResidual, PlaceUnitInputs,
    PlaceUnitProducerError, PlaceUnitProducerReceipt, PlacementMapSnapshot, PlacementTileFacts,
};
use crate::setup_units_producer::{
    build_units_plan, validate_build_units_prefix_receipt, BuildUnitsInputs, BuildUnitsPlan,
    BuildUnitsPlanError, BuildUnitsPrefixReceipt, BuildUnitsReceiptError, DirectRandomDrawReceipt,
    InitUnitAuthorityReceipt, InitUnitRngSpan, PlaceUnitReceipt, PlacementOutcomeReceipt,
    PlacementRngEvent, StartingUnitBonuses, StartingUnitRuleFacts, StartingUnitTypeFacts,
    TypeResolutionFacts, BASE_PEASANT_TYPE, BASE_SCOUT_TYPE, DUTCH_MERCHANT_TYPE,
    OBJECTS_INIT_UNIT_BYTES, OBJECTS_INIT_UNIT_VA, PLACE_UNIT_DIRECT_RANDOM_CALL_VA,
};
use crate::wire::CommandView;
use crate::world_owner_frontier::sha256;

pub const REPLAY_FILE_SHA256: [u8; 32] = [
    0x16, 0x90, 0x43, 0x1a, 0x5e, 0xf1, 0x9b, 0x38, 0xa3, 0x42, 0x5d, 0x3d, 0xd7, 0x31, 0x1e, 0x8e,
    0x83, 0xca, 0x0d, 0x27, 0xc5, 0x6f, 0xab, 0xe4, 0x9d, 0x77, 0x6a, 0x9f, 0x14, 0x21, 0xb2, 0x51,
];
pub const REPLAY_SEED: u32 = 0x00bb_97d3;
pub const MAP_STYLE: u8 = 14;
pub const MAP_SIZE: u8 = 6;
pub const OWNER: u8 = 0;
pub const PLAY: u8 = 0;
pub const TRIBE: u8 = 22;
pub const SETUP_CALLS: usize = 7;
pub const CITIZEN_ORDINALS: [usize; 4] = [3, 4, 5, 6];
pub const GROUP_MOVE_SERIAL: i32 = 64;
pub const GROUP_MOVE_FRAME: i32 = 379;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame379WorldgenSource {
    /// The normal procedural pipeline completed from this replay and installed the starting
    /// Village before `Setup::build_units` began.
    CompletedGreatLakesWorldgenAndStartingVillage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame379LeaderSetupSource {
    /// Exact `tribe_can_type`, `has_tribe_bonus`, and both `current_upgrade` calls read from
    /// the newly initialized owner-0 Leader/type bitsets.
    CanonicalNewGameLeaderTypeState,
}

/// Dynamic Leader facts are deliberately not inferred from static Rules rows. In particular,
/// `current_upgrade` depends on runtime bitsets which the replay does not serialize.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame379LeaderSetupAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub source: Frame379LeaderSetupSource,
    pub replay_file_sha256: [u8; 32],
    pub owner: i32,
    pub tribe: i32,
    pub bonuses: StartingUnitBonuses,
    pub types: StartingUnitTypeFacts,
}

/// The only admitted injection seam. World checksum alone is insufficient because placement
/// also reads collision words, center-Build state, and the main RNG.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame379WorldgenAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub source: Frame379WorldgenSource,
    pub replay_file_sha256: [u8; 32],
    pub world_checksum: WorldChecksum,
    pub random_state: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame379SetupFacts {
    pub replay_file_sha256: [u8; 32],
    pub replay_payload_sha256: [u8; 32],
    pub owner: u8,
    pub play: u8,
    pub tribe: u8,
    pub center_position: (i32, i32),
    pub center_build_o: i32,
    pub start_tile: (i32, i32),
    pub scout: ReplayUnitTypeFacts,
    pub merchant: ReplayUnitTypeFacts,
    pub citizen: ReplayUnitTypeFacts,
    pub plan: BuildUnitsPlan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame379PlaceUnitReceipt {
    pub setup_ordinal: usize,
    pub map_checksum_before: WorldChecksum,
    pub placement_snapshot_sha256: [u8; 32],
    pub placement: PlaceUnitProducerReceipt,
    pub completed_init_revision: u64,
    pub completed_init_digest: [u8; 32],
    pub init: InitUnitAuthorityReceipt,
    pub canonical_after: UnitAfterInit,
    pub rng_after_init: i32,
    pub map_checksum_after: WorldChecksum,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame379CompletedInitSource {
    /// A complete retail `Objects::init_unit` receiver, including every nested Unit/Guy,
    /// collision, visibility, graphics, Leader, and RNG effect, committed this after-image.
    CompleteRetailObjectsInitUnitReceiver,
}

/// Per-call authority consumed by the chronology adapter. The detailed 1,603-byte receipt is
/// validated again here; the digest and before/after owner facts bind the receiver effects which
/// that source-only receipt intentionally does not model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame379CompletedInitAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub source: Frame379CompletedInitSource,
    pub replay_file_sha256: [u8; 32],
    pub setup_ordinal: usize,
    pub detailed: DetailedInitUnitReceipt,
    pub projected: InitUnitAuthorityReceipt,
    pub world_checksum_before: WorldChecksum,
    pub world_checksum_after: WorldChecksum,
    pub rng_before: i32,
    pub rng_after: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame379SetupReceipt {
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub source: Frame379WorldgenSource,
    pub leader_authority_revision: u64,
    pub leader_authority_digest: [u8; 32],
    pub leader_source: Frame379LeaderSetupSource,
    pub replay_file_sha256: [u8; 32],
    pub replay_payload_sha256: [u8; 32],
    pub world_checksum_before: WorldChecksum,
    pub world_checksum_after: WorldChecksum,
    pub rng_before: i32,
    pub rng_after: i32,
    pub calls: Vec<Frame379PlaceUnitReceipt>,
    pub setup: BuildUnitsPrefixReceipt,
    /// Exact frame-zero canonical owner after all seven completed receivers.
    pub canonical_frame: i32,
    pub canonical_world_checksum: WorldChecksum,
    pub canonical_random_state: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame379SetupError {
    ReplayRead(String),
    WrongReplayFile,
    PayloadRead(String),
    MissingRules,
    WrongReplaySettings,
    MissingPlayer,
    WrongPlayer,
    MissingFirstTurn,
    MissingFirstCamera,
    DuplicateFirstCamera,
    MalformedFirstCamera,
    CameraNotVillageCenter,
    TypeFacts(PrePairUnitAuthorityError),
    WrongResolvedType,
    MissingLeaderAuthorityRevision,
    MissingLeaderCompositionDigest,
    LeaderAuthorityMismatch,
    Plan(BuildUnitsPlanError),
    WrongPlan,
    MissingAuthorityRevision,
    MissingCompositionDigest,
    AuthorityReplayMismatch,
    WrongFrame {
        expected: i32,
        actual: i32,
    },
    WorldChecksumMismatch {
        ordinal: usize,
    },
    RandomStateMismatch {
        ordinal: usize,
    },
    MapDimensionMismatch,
    MapShapeOverflow,
    MissingCenterBuild,
    CenterBuildMismatch,
    Placement(PlaceUnitProducerError),
    PlacementDidNotReachInit {
        ordinal: usize,
    },
    InitRequestMismatch {
        ordinal: usize,
    },
    WrongReceiptCount,
    InitReceiptInvalid {
        ordinal: usize,
    },
    DetailedInitReceipt {
        ordinal: usize,
        source: InitUnitReceiptError,
    },
    CompletedInitAuthorityMismatch {
        ordinal: usize,
    },
    WrongAllocationSequence {
        ordinal: usize,
    },
    MissingCanonicalUnit {
        ordinal: usize,
        o: i32,
    },
    InactiveCanonicalUnit {
        ordinal: usize,
        o: i32,
    },
    CanonicalIdentityMismatch {
        ordinal: usize,
        o: i32,
    },
    CanonicalTypeMismatch {
        ordinal: usize,
        o: i32,
    },
    CanonicalPositionMismatch {
        ordinal: usize,
        o: i32,
    },
    CanonicalContainerMismatch {
        ordinal: usize,
        o: i32,
    },
    PriorUnitChanged {
        ordinal: usize,
        o: i32,
    },
    RegistryNotDenseEquivalent,
    SetupReceipt(BuildUnitsReceiptError),
}

impl fmt::Display for Frame379SetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "2024 frame-379 setup refused: {self:?}")
    }
}

impl std::error::Error for Frame379SetupError {}

impl From<PrePairUnitAuthorityError> for Frame379SetupError {
    fn from(value: PrePairUnitAuthorityError) -> Self {
        Self::TypeFacts(value)
    }
}

impl From<BuildUnitsPlanError> for Frame379SetupError {
    fn from(value: BuildUnitsPlanError) -> Self {
        Self::Plan(value)
    }
}

impl From<PlaceUnitProducerError> for Frame379SetupError {
    fn from(value: PlaceUnitProducerError) -> Self {
        Self::Placement(value)
    }
}

impl From<BuildUnitsReceiptError> for Frame379SetupError {
    fn from(value: BuildUnitsReceiptError) -> Self {
        Self::SetupReceipt(value)
    }
}

fn dynamic_type_matches(
    dynamic: TypeResolutionFacts,
    base: i32,
    nation_variant: i32,
    final_type: ReplayUnitTypeFacts,
) -> bool {
    dynamic.base == base
        && dynamic.nation_variant == nation_variant
        && dynamic.build_units_upgrade >= 0
        && dynamic.place_unit_upgrade == final_type.type_index
        && dynamic.uber_size == final_type.uber_size
        && dynamic.squad_size == final_type.squad_size
        && dynamic.crew_size == final_type.crew_size
}

fn first_camera(replay: &Replay) -> Result<(i32, i32), Frame379SetupError> {
    let turn = replay
        .turns
        .first()
        .ok_or(Frame379SetupError::MissingFirstTurn)?;
    let player = turn
        .players
        .iter()
        .find(|player| player.play == i32::from(PLAY))
        .ok_or(Frame379SetupError::MissingFirstCamera)?;
    let mut cameras = player
        .commands
        .iter()
        .filter(|command| command.opcode == CAMERA_COMMAND_OPCODE);
    let camera = cameras
        .next()
        .ok_or(Frame379SetupError::MissingFirstCamera)?;
    if cameras.next().is_some() {
        return Err(Frame379SetupError::DuplicateFirstCamera);
    }
    let view = CommandView::new(camera.opcode, &camera.bytes);
    let x = view
        .get("x_loc")
        .and_then(|value| i32::try_from(value).ok())
        .ok_or(Frame379SetupError::MalformedFirstCamera)?;
    let y = view
        .get("y_loc")
        .and_then(|value| i32::try_from(value).ok())
        .ok_or(Frame379SetupError::MalformedFirstCamera)?;
    if x.rem_euclid(WORLD_TO_COORD) != VILLAGE_CENTER_OFFSET
        || y.rem_euclid(WORLD_TO_COORD) != VILLAGE_CENTER_OFFSET
    {
        return Err(Frame379SetupError::CameraNotVillageCenter);
    }
    Ok((x, y))
}

fn constants_i32(
    payload: &[u8],
    rules: &crate::initial::InitialRules,
    offset: usize,
) -> Result<i32, Frame379SetupError> {
    let constants = rules.serialized_offset + 1 + crate::initial::SHIPPED_TYPES_SERIALIZED_BYTES;
    let bytes = payload
        .get(constants + offset..constants + offset + 4)
        .ok_or(Frame379SetupError::WrongReplaySettings)?;
    Ok(i32::from_le_bytes(
        bytes.try_into().expect("four-byte slice"),
    ))
}

/// Discover every static setup-plan input supplied by the replay/Rules pair and join the exact
/// dynamic Leader selectors. The Dutch Merchant's replay-carried 1+2 Guy shape is load-bearing.
pub fn discover_frame379_setup(
    replay: &Replay,
    leader: &Frame379LeaderSetupAuthority,
) -> Result<Frame379SetupFacts, Frame379SetupError> {
    let raw = std::fs::read(&replay.path)
        .map_err(|error| Frame379SetupError::ReplayRead(error.to_string()))?;
    let replay_file_sha256 = sha256(&raw);
    if replay_file_sha256 != REPLAY_FILE_SHA256 {
        return Err(Frame379SetupError::WrongReplayFile);
    }
    if leader.revision == 0 {
        return Err(Frame379SetupError::MissingLeaderAuthorityRevision);
    }
    if leader.composition_digest == [0; 32] {
        return Err(Frame379SetupError::MissingLeaderCompositionDigest);
    }
    if leader.replay_file_sha256 != replay_file_sha256
        || leader.owner != i32::from(OWNER)
        || leader.tribe != i32::from(TRIBE)
        || leader.bonuses
            != (StartingUnitBonuses {
                dutch_merchants: true,
                ..StartingUnitBonuses::default()
            })
    {
        return Err(Frame379SetupError::LeaderAuthorityMismatch);
    }
    if replay.initial.info.seed != REPLAY_SEED
        || replay.initial.info.settings.map_style != MAP_STYLE
        || replay.initial.info.settings.map_size != MAP_SIZE
        || replay.initial.info.settings.starting_town != 1
        || replay.initial.info.settings.starting_resources != 0
        || replay.initial.info.settings.reveal_map != 1
    {
        return Err(Frame379SetupError::WrongReplaySettings);
    }
    let player = replay
        .initial
        .info
        .players
        .iter()
        .find(|player| player.present && player.who == OWNER)
        .ok_or(Frame379SetupError::MissingPlayer)?;
    if player.play != PLAY || player.tribe != TRIBE {
        return Err(Frame379SetupError::WrongPlayer);
    }
    let payload = load_payload(&replay.path)
        .map_err(|error| Frame379SetupError::PayloadRead(error.to_string()))?;
    if sha256(&payload) != replay.initial.payload_sha256 {
        return Err(Frame379SetupError::WrongReplayFile);
    }
    let rules = replay
        .initial
        .rules
        .ok_or(Frame379SetupError::MissingRules)?;
    let scout_variant = replay_tribe_type_facts(&payload, &rules, TRIBE as usize, BASE_SCOUT_TYPE)?;
    let citizen_variant =
        replay_tribe_type_facts(&payload, &rules, TRIBE as usize, BASE_PEASANT_TYPE)?;
    let merchant_variant =
        replay_tribe_type_facts(&payload, &rules, TRIBE as usize, DUTCH_MERCHANT_TYPE)?;
    let scout = replay_unit_type_facts(&payload, &rules, leader.types.scout.place_unit_upgrade)?;
    let merchant = replay_unit_type_facts(
        &payload,
        &rules,
        leader.types.dutch_merchant.place_unit_upgrade,
    )?;
    let citizen =
        replay_unit_type_facts(&payload, &rules, leader.types.citizen.place_unit_upgrade)?;
    if scout_variant.tribe_id != i32::from(TRIBE)
        || citizen_variant.tribe_id != i32::from(TRIBE)
        || merchant_variant.tribe_id != i32::from(TRIBE)
        || !dynamic_type_matches(
            leader.types.scout,
            BASE_SCOUT_TYPE,
            scout_variant.nation_variant,
            scout,
        )
        || !dynamic_type_matches(
            leader.types.dutch_merchant,
            DUTCH_MERCHANT_TYPE,
            merchant_variant.nation_variant,
            merchant,
        )
        || !dynamic_type_matches(
            leader.types.citizen,
            BASE_PEASANT_TYPE,
            citizen_variant.nation_variant,
            citizen,
        )
        || [scout.type_index, merchant.type_index, citizen.type_index]
            != [BASE_SCOUT_TYPE, DUTCH_MERCHANT_TYPE, BASE_PEASANT_TYPE]
        || (scout.squad_size, scout.uber_size, scout.crew_size) != (1, 1, 1)
        || (merchant.squad_size, merchant.uber_size, merchant.crew_size) != (1, 1, 2)
        || (citizen.squad_size, citizen.uber_size, citizen.crew_size) != (1, 1, 0)
    {
        return Err(Frame379SetupError::WrongResolvedType);
    }
    let center_position = first_camera(replay)?;
    let start_tile = (
        (center_position.0 - VILLAGE_CENTER_OFFSET) / WORLD_TO_COORD,
        (center_position.1 - VILLAGE_CENTER_OFFSET) / WORLD_TO_COORD,
    );
    let plan = build_units_plan(BuildUnitsInputs {
        owner: i32::from(OWNER),
        start_index: i32::from(OWNER),
        center_city_o: 2_000,
        start_tile_x: start_tile.0,
        start_tile_y: start_tile.1,
        starting_town: i32::from(replay.initial.info.settings.starting_town),
        starting_resources: replay.initial.info.settings.starting_resources,
        reveal_map: replay.initial.info.settings.reveal_map,
        bonuses: leader.bonuses,
        rules: StartingUnitRuleFacts {
            bonus_scouts: constants_i32(&payload, &rules, 0x680)?,
            bonus_scholars: constants_i32(&payload, &rules, 0x5e4)?,
            bonus_citizens: constants_i32(&payload, &rules, 0x7b0)?,
            starting_town_citizens: constants_i32(&payload, &rules, 0x878)?,
        },
        types: leader.types,
    })?;
    if plan.stop.is_some()
        || plan.calls.len() != SETUP_CALLS
        || plan
            .calls
            .iter()
            .map(|call| call.place_unit_upgrade)
            .collect::<Vec<_>>()
            != [69, 62, 62, 50, 50, 50, 50]
    {
        return Err(Frame379SetupError::WrongPlan);
    }
    Ok(Frame379SetupFacts {
        replay_file_sha256,
        replay_payload_sha256: replay.initial.payload_sha256,
        owner: OWNER,
        play: PLAY,
        tribe: TRIBE,
        center_position,
        center_build_o: 2_000,
        start_tile,
        scout,
        merchant,
        citizen,
        plan,
    })
}

fn capture_placement_map(
    world: &World,
) -> Result<(PlacementMapSnapshot, [u8; 32]), Frame379SetupError> {
    if world.xs <= 0
        || world.ys <= 0
        || world.tile_xs != world.xs.checked_mul(4).unwrap_or(i32::MIN)
        || world.tile_ys != world.ys.checked_mul(4).unwrap_or(i32::MIN)
    {
        return Err(Frame379SetupError::MapDimensionMismatch);
    }
    let cells = world
        .xs
        .checked_mul(world.ys)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or(Frame379SetupError::MapShapeOverflow)?;
    if world.wdata.len() != cells {
        return Err(Frame379SetupError::MapDimensionMismatch);
    }
    let mut tiles = Vec::with_capacity(cells);
    let mut image = Vec::with_capacity(8 + cells.saturating_mul(9));
    image.extend_from_slice(&world.xs.to_le_bytes());
    image.extend_from_slice(&world.ys.to_le_bytes());
    for y in 0..world.ys {
        for x in 0..world.xs {
            let cell = &world.wdata[(y * world.xs + x) as usize];
            let tile = PlacementTileFacts {
                continent: cell.region as u16,
                flags: cell.flags,
                land: cell.land,
                occupied_o: cell.down,
                collision: world.tmask(x * 4 + 2, y * 4 + 2),
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

fn validate_canonical_prefix(
    sim: &Sim,
    facts: &Frame379SetupFacts,
    receipts: &[Frame379PlaceUnitReceipt],
) -> Result<(), Frame379SetupError> {
    if !sim.world.object_bands_are_dense_equivalent() {
        return Err(Frame379SetupError::RegistryNotDenseEquivalent);
    }
    for (ordinal, receipt) in receipts.iter().enumerate() {
        let member = receipt
            .init
            .members
            .first()
            .ok_or(Frame379SetupError::WrongAllocationSequence { ordinal })?;
        let o = ordinal as i32;
        let row = sim
            .world
            .unit_row_at(i32::from(OWNER), o)
            .ok_or(Frame379SetupError::MissingCanonicalUnit { ordinal, o })?;
        if sim.world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
            return Err(Frame379SetupError::InactiveCanonicalUnit { ordinal, o });
        }
        let handle = sim.world.handle_at_row(row);
        if handle.is_none_or(|handle| {
            (handle.id, handle.generation) != (member.identity.id, member.identity.generation)
        }) || sim.world.units.get_who(row) != OWNER
            || i32::from(sim.world.units.o()[row]) != o
        {
            return Err(Frame379SetupError::CanonicalIdentityMismatch { ordinal, o });
        }
        let expected_type = facts.plan.calls[ordinal].place_unit_upgrade;
        if sim.world.unit_type_id(row) != Some(expected_type)
            || sim.unit_type.get(row).copied() != Some(expected_type)
        {
            return Err(Frame379SetupError::CanonicalTypeMismatch { ordinal, o });
        }
        let canonical = receipt.canonical_after;
        if canonical.owner != i32::from(OWNER)
            || canonical.o != o
            || canonical.type_index != expected_type
            || sim.world.units.x_internal()[row] != canonical.x
            || sim.world.units.y_internal()[row] != canonical.y
            || sim.world.units.angle()[row] != canonical.angle
            || sim.world.units.get_unit_masks(row) != canonical.unit_masks
        {
            return Err(Frame379SetupError::CanonicalPositionMismatch { ordinal, o });
        }
        if !sim.world.orders(row).is_empty()
            || sim.paths.get(row).is_none_or(|path| !path.is_empty())
            || i32::from(sim.world.units.guy_mark()[row]) != facts.plan.calls[ordinal].squad_size
        {
            return Err(Frame379SetupError::CanonicalContainerMismatch { ordinal, o });
        }
    }
    Ok(())
}

/// Execute and validate the complete seven-call setup chronology downstream of world generation.
///
/// `states[0]` is the completed-worldgen/starting-Village seam. `states[n+1]` must be the exact
/// canonical Sim immediately after call `n` completed. Every call supplies the native detailed
/// receipt plus a complete receiver authority; placement probes and direct RNG draws are
/// recomputed here.
pub fn produce_frame379_setup(
    replay: &Replay,
    states: &[&Sim],
    completed_inits: &[Frame379CompletedInitAuthority],
    worldgen: &Frame379WorldgenAuthority,
    leader: &Frame379LeaderSetupAuthority,
) -> Result<Frame379SetupReceipt, Frame379SetupError> {
    let facts = discover_frame379_setup(replay, leader)?;
    if worldgen.revision == 0 {
        return Err(Frame379SetupError::MissingAuthorityRevision);
    }
    if worldgen.composition_digest == [0; 32] {
        return Err(Frame379SetupError::MissingCompositionDigest);
    }
    if worldgen.replay_file_sha256 != facts.replay_file_sha256 {
        return Err(Frame379SetupError::AuthorityReplayMismatch);
    }
    if states.len() != SETUP_CALLS + 1 || completed_inits.len() != SETUP_CALLS {
        return Err(Frame379SetupError::WrongReceiptCount);
    }
    let edge = replay
        .initial
        .info
        .settings
        .map_edge_world_cells()
        .ok_or(Frame379SetupError::MapDimensionMismatch)?;
    for state in states {
        if state.world.frame != 0 {
            return Err(Frame379SetupError::WrongFrame {
                expected: 0,
                actual: state.world.frame,
            });
        }
        if state.map.world.xs != edge || state.map.world.ys != edge {
            return Err(Frame379SetupError::MapDimensionMismatch);
        }
    }
    let entry = states[0];
    if entry.map.world.checksum_sections() != worldgen.world_checksum {
        return Err(Frame379SetupError::WorldChecksumMismatch { ordinal: 0 });
    }
    if entry.world.random.state() != worldgen.random_state {
        return Err(Frame379SetupError::RandomStateMismatch { ordinal: 0 });
    }
    if entry.world.unit_mark(OWNER as usize).unwrap_or(-1) != 0 {
        return Err(Frame379SetupError::WrongAllocationSequence { ordinal: 0 });
    }
    let center_identity = entry
        .world
        .object_bands()
        .live_identity(
            don_sim::systems::sparse_object_bands_authority_frontier::RetailObjectAddress::new(
                OWNER,
                don_sim::systems::sparse_object_bands_authority_frontier::RetailBand::Build,
                facts.center_build_o,
            ),
        )
        .ok_or(Frame379SetupError::MissingCenterBuild)?;
    let WorldObjectIdentity::BuildRow(center_row) = center_identity else {
        return Err(Frame379SetupError::MissingCenterBuild);
    };
    let center = entry
        .builds
        .get(center_row as usize)
        .ok_or(Frame379SetupError::MissingCenterBuild)?;
    if center.flags & don_sim::systems::production::flag::VALID == 0
        || center.who != OWNER
        || i32::from(center.object_id()) != facts.center_build_o
        || center.position() != facts.center_position
        || entry
            .production_runtime
            .build_types
            .get(center_row as usize)
            .and_then(|value| *value)
            != Some(CITY_CENTER_TYPE)
    {
        return Err(Frame379SetupError::CenterBuildMismatch);
    }

    let mut calls = Vec::with_capacity(SETUP_CALLS);
    let mut placements = Vec::with_capacity(SETUP_CALLS);
    let mut rng = worldgen.random_state;
    for ordinal in 0..SETUP_CALLS {
        let before = states[ordinal];
        let after = states[ordinal + 1];
        if before.world.random.state() != rng {
            return Err(Frame379SetupError::RandomStateMismatch { ordinal });
        }
        validate_canonical_prefix(before, &facts, &calls)?;
        let map_checksum_before = before.map.world.checksum_sections();
        let (map, placement_snapshot_sha256) = capture_placement_map(&before.map.world)?;
        let call = facts.plan.calls[ordinal];
        let placement = produce_place_unit_probe_prefix(
            PlaceUnitInputs {
                owner: call.owner,
                upgraded_type: call.place_unit_upgrade,
                requested_x: call.requested_x,
                requested_y: call.requested_y,
                center: Some(CenterBuildFacts {
                    owner: i32::from(OWNER),
                    o: facts.center_build_o,
                    x: facts.center_position.0,
                    y: facts.center_position.1,
                }),
                starting_town: replay.initial.info.settings.starting_town,
                leader_active: 0,
            },
            &map,
            rng,
        )?;
        let PlaceUnitExternalResidual::ObjectsInitUnit(request) = placement.first_external_residual
        else {
            return Err(Frame379SetupError::PlacementDidNotReachInit { ordinal });
        };
        let completed = &completed_inits[ordinal];
        if completed.revision == 0
            || completed.composition_digest == [0; 32]
            || completed.replay_file_sha256 != facts.replay_file_sha256
            || completed.setup_ordinal != ordinal
            || completed.world_checksum_before != map_checksum_before
            || completed.rng_before != placement.rng_after_probes
        {
            return Err(Frame379SetupError::CompletedInitAuthorityMismatch { ordinal });
        }
        let expected_request = BhsInitUnitRequest {
            owner: request.owner,
            type_index: request.type_index,
            x: request.x,
            y: request.y,
            exact_o: request.exact_o,
            external_previous: request.external_previous,
            external_next: request.external_next,
        };
        if completed.detailed.request != expected_request
            || completed.detailed.type_facts.uber_size != call.uber_size
        {
            return Err(Frame379SetupError::InitRequestMismatch { ordinal });
        }
        let effects = completed
            .detailed
            .validate()
            .map_err(|source| Frame379SetupError::DetailedInitReceipt { ordinal, source })?;
        if effects.initialized_members.as_slice() != [ordinal as i32]
            || effects.terminal_find_free_failure.is_some()
            || effects.returned_captain_or_failure != ordinal as i32
            || effects.unit_mark_before != ordinal as i32
            || effects.unit_mark_after != ordinal as i32 + 1
        {
            return Err(Frame379SetupError::InitReceiptInvalid { ordinal });
        }
        let init = completed.projected.clone();
        if request.come_out_zero_after_success
            || request.owner != call.owner
            || request.type_index != call.place_unit_upgrade
            || init.validated_body_va != OBJECTS_INIT_UNIT_VA
            || init.validated_body_bytes != OBJECTS_INIT_UNIT_BYTES
            || init.unit_mark_before != ordinal as i32
            || init.unit_mark_after != ordinal as i32 + 1
            || init.returned_captain_o != ordinal as i32
            || init.members.len() != 1
        {
            return Err(Frame379SetupError::InitReceiptInvalid { ordinal });
        }
        let member = &init.members[0];
        if member.identity.owner != call.owner
            || member.identity.o != ordinal as i32
            || member.ptype_index != call.place_unit_upgrade
            || member.guys.length != call.squad_size + call.crew_size
            || member.guy_identities.len() != (call.squad_size + call.crew_size) as usize
        {
            return Err(Frame379SetupError::WrongAllocationSequence { ordinal });
        }
        let probe_rng = placement.rng_after_probes;
        let after_rng = after.world.random.state();
        let map_checksum_after = after.map.world.checksum_sections();
        if completed.rng_after != after_rng
            || completed.world_checksum_after != map_checksum_after
            || after.world.unit_mark(OWNER as usize).unwrap_or(-1) != ordinal as i32 + 1
        {
            return Err(Frame379SetupError::CompletedInitAuthorityMismatch { ordinal });
        }
        let row = after
            .world
            .unit_row_at(i32::from(OWNER), ordinal as i32)
            .ok_or(Frame379SetupError::MissingCanonicalUnit {
                ordinal,
                o: ordinal as i32,
            })?;
        let canonical_after = UnitAfterInit {
            owner: i32::from(OWNER),
            o: ordinal as i32,
            type_index: call.place_unit_upgrade,
            x: after.world.units.x_internal()[row],
            y: after.world.units.y_internal()[row],
            angle: after.world.units.angle()[row],
            unit_masks: after.world.units.get_unit_masks(row),
        };
        let detailed_after = completed.detailed.steps.iter().find_map(|step| match step {
            InitUnitStep::UnitInit(receipt) => Some(receipt.after),
            _ => None,
        });
        let captain = completed.detailed.steps.last().and_then(|step| match step {
            InitUnitStep::ResolveCaptain(receipt) => Some(receipt.captain),
            _ => None,
        });
        let expected_radius = if call.place_unit_upgrade == facts.scout.type_index {
            facts.scout.new_block_radius
        } else if call.place_unit_upgrade == facts.merchant.type_index {
            facts.merchant.new_block_radius
        } else {
            facts.citizen.new_block_radius
        };
        if detailed_after != Some(canonical_after)
            || captain.is_none_or(|captain| {
                captain.owner != canonical_after.owner
                    || captain.o != canonical_after.o
                    || captain.x != canonical_after.x
                    || captain.y != canonical_after.y
                    || captain.angle != canonical_after.angle
                    || captain.new_block_radius != expected_radius
            })
        {
            return Err(Frame379SetupError::CompletedInitAuthorityMismatch { ordinal });
        }
        let mut rng_events = placement
            .probes
            .iter()
            .map(|probe| {
                PlacementRngEvent::DirectOffset(DirectRandomDrawReceipt {
                    call_va: PLACE_UNIT_DIRECT_RANDOM_CALL_VA,
                    state_before: probe.draw.state_before,
                    returned: probe.draw.returned,
                    state_after: probe.draw.state_after,
                })
            })
            .collect::<Vec<_>>();
        rng_events.push(PlacementRngEvent::InitUnit(InitUnitRngSpan {
            body_va: OBJECTS_INIT_UNIT_VA,
            body_bytes: OBJECTS_INIT_UNIT_BYTES,
            state_before: probe_rng,
            state_after: after_rng,
        }));
        let placement_receipt = PlaceUnitReceipt {
            call,
            rng_before: rng,
            rng_after: after_rng,
            rng_events,
            outcome: PlacementOutcomeReceipt::Spawned(init.clone()),
        };
        placements.push(placement_receipt);
        calls.push(Frame379PlaceUnitReceipt {
            setup_ordinal: ordinal,
            map_checksum_before,
            placement_snapshot_sha256,
            placement,
            completed_init_revision: completed.revision,
            completed_init_digest: completed.composition_digest,
            init,
            canonical_after,
            rng_after_init: after_rng,
            map_checksum_after,
        });
        validate_canonical_prefix(after, &facts, &calls)?;
        rng = after_rng;
    }
    let setup = BuildUnitsPrefixReceipt {
        rng_initial: worldgen.random_state,
        rng_final: rng,
        placements,
    };
    validate_build_units_prefix_receipt(&facts.plan, &setup)?;
    let final_state = states[SETUP_CALLS];
    Ok(Frame379SetupReceipt {
        authority_revision: worldgen.revision,
        authority_digest: worldgen.composition_digest,
        source: worldgen.source,
        leader_authority_revision: leader.revision,
        leader_authority_digest: leader.composition_digest,
        leader_source: leader.source,
        replay_file_sha256: facts.replay_file_sha256,
        replay_payload_sha256: facts.replay_payload_sha256,
        world_checksum_before: worldgen.world_checksum.clone(),
        world_checksum_after: final_state.map.world.checksum_sections(),
        rng_before: worldgen.random_state,
        rng_after: rng,
        calls,
        setup,
        canonical_frame: final_state.world.frame,
        canonical_world_checksum: final_state.map.world.checksum_sections(),
        canonical_random_state: final_state.world.random.state(),
    })
}
