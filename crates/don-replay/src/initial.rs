//! Authoritative initial-game setup from the `.rcx` prefix.
//!
//! A recording begins with `Game::walk_data` (`0x00589600`), not an ad-hoc
//! replay header.  The first child is `GameInfo::walk_data` (`0x005d6570`), so
//! the seed, map selectors and all eight `Player` setup records are available
//! before the first command package. This module consumes that prefix in the
//! exact retail walk order. Dynamic initial-world state after
//! `game.info.save_name` is still not presented as a save snapshot. The static
//! Rules section at the end of that span is now independently bounded and
//! projected through its exact checksum-only traversal.

use crate::checksum::adler32;
use crate::map_style::MapStyleStaticData;
use crate::rules_channel::{
    BALANCE_BYTES, RETAIL_AFTER_BALANCE, RETAIL_AFTER_CONSTANTS, RETAIL_AFTER_TRIBES,
    RETAIL_AFTER_TYPES, RETAIL_WALKED_BYTES, RULES_BLOCK_BYTES, RULES_DUPLICATE_OFFSET,
    SHIPPED_RULES_CHANNEL, TRIBE_COUNT, TRIBE_SIZE, TYPE_SLOTS,
};
use crate::world_owner_frontier::{
    sha256, ExactPortTransitionProof, ExactPortWrittenRange, InitialWorldPrefixEvidence,
    ReplaySpan, RulesWorldEvidence, WorldOwnerLedger, WorldSectionMask,
};
use don_sim::systems::map_terrain::{World, WorldChecksum, WorldSection};
use don_sim::systems::regions::Regions;
use std::path::Path;

#[path = "replay_place_all_owners.rs"]
pub mod replay_place_all_owners;

pub const TAG_GAME: u8 = 0x16;
pub const TAG_GAME_INFO: u8 = 0x42;
pub const TAG_PLAYER: u8 = 0x50;
/// `Game::walk_rules_data`'s section tag in every supported-corpus recording.
///
/// The byte is written by `SaveGame::walk_tag` before `Types::walk_rules_data`
/// (`0x00589550`); `CheckSum::walk_tag` is a no-op, so it is parsed but never
/// handed to the checksum primitive.
pub const TAG_RULES: u8 = 0x92;
/// The tag emitted before each of the 24 `Tribe::walk_rules_data` bodies.
pub const TAG_TRIBE: u8 = 0x8f;

/// Exact serialized length of the unmodded shipped Rules section.
///
/// Measured independently in the 2024.06.20 solo and multiplayer recordings
/// and in the 2017.11.29 corpus: the section begins at its `0x92` tag and ends
/// exactly at the first command-package byte in all specimens.
pub const SHIPPED_RULES_SERIALIZED_BYTES: usize = 1_024_221;
/// Serialized `Types::walk_rules_data` body, excluding the outer Rules tag.
/// The difference from `RETAIL_TYPE_WALKED_BYTES` is exactly the save-only
/// Type-name and Tech string encodings.
pub const SHIPPED_TYPES_SERIALIZED_BYTES: usize = 500_334;

/// Search boundary after the already-parsed `Game` prefix. The only
/// intervening writers are one `String::walk_data` and at most 44 bytes of
/// per-player extras (`0x009534e0`). The deliberately generous bound keeps a
/// corrupt length from turning parsing into an unbounded command-stream scan;
/// failure simply leaves the channel absent.
const RULES_SEARCH_BYTES: usize = 256 * 1024;

/// The seven shipped `MapSizeData::data[0]` world-cell edges, in list order.
///
/// The list is `[40, 50, 60, 70, 80, 90, 100]`; `GameInfo::map_size` is its
/// index.  `Game::wonder_timer` independently reads index 3 as the 70-cell
/// Standard-map reference (`0x005944f0`), pinning both the unit and the order.
pub const MAP_SIZE_WORLD_EDGES: [i32; 7] = [40, 50, 60, 70, 80, 90, 100];

/// A byte range in the decompressed `.rcx` payload which proves one input.
///
/// Keeping these offsets with the parsed values makes the reconstruction
/// boundary mutation-sensitive: a caller can identify the exact replay bytes
/// which supplied the world-generation tuple instead of trusting a copied
/// summary of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayByteSpan {
    pub offset: usize,
    pub bytes: usize,
}

impl ReplayByteSpan {
    fn new(offset: usize, bytes: usize) -> Self {
        Self { offset, bytes }
    }

    pub fn end(self) -> usize {
        self.offset.saturating_add(self.bytes)
    }
}

/// Exact source locations for the replay-varying `Map::make` inputs.
///
/// The six scalar fields are the reproducibility tuple recovered from
/// `Map::make`: seed, map style, map size, player count, game rules, and
/// starting-town selector. Player gates/bodies are retained as well because
/// the later start-placement path enumerates the eight setup records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldgenSourceSpans {
    pub seed: ReplayByteSpan,
    pub map_style: ReplayByteSpan,
    pub map_size: ReplayByteSpan,
    pub players: ReplayByteSpan,
    pub game_rules: ReplayByteSpan,
    pub starting_town: ReplayByteSpan,
    pub scenario_type: ReplayByteSpan,
    pub player_flags: [ReplayByteSpan; 8],
    pub player_bodies: [Option<ReplayByteSpan>; 8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameSettings {
    pub team_style: u8,
    pub map_style: u8,
    pub map_size: u8,
    pub players: u8,
    pub max_observers: u8,
    pub game_speed: u8,
    pub game_rules: u8,
    pub difficulty: u8,
    pub starting_town: u8,
    pub starting_resources: u8,
    pub starting_resources2: u8,
    pub tech_cost: u8,
    pub reveal_map: u8,
    pub pop_limit: u8,
    pub rush_rules: u8,
    pub cannon_times: u8,
    pub starting_technology: u8,
    pub starting_technology2: u8,
    pub ending_technology: u8,
    pub elimination: u8,
    pub victory: u8,
    pub wonderwin: u8,
    pub score_goal: u8,
    pub popwin: u8,
    pub time_limit: u8,
    pub chairs: u8,
    pub econwin: u8,
    pub scenario_type: u8,
    pub script_type: u8,
    pub mods: u8,
}

impl GameSettings {
    fn from_bytes(b: [u8; 30]) -> Self {
        Self {
            team_style: b[0],
            map_style: b[1],
            map_size: b[2],
            players: b[3],
            max_observers: b[4],
            game_speed: b[5],
            game_rules: b[6],
            difficulty: b[7],
            starting_town: b[8],
            starting_resources: b[9],
            starting_resources2: b[10],
            tech_cost: b[11],
            reveal_map: b[12],
            pop_limit: b[13],
            rush_rules: b[14],
            cannon_times: b[15],
            starting_technology: b[16],
            starting_technology2: b[17],
            ending_technology: b[18],
            elimination: b[19],
            victory: b[20],
            wonderwin: b[21],
            score_goal: b[22],
            popwin: b[23],
            time_limit: b[24],
            chairs: b[25],
            econwin: b[26],
            scenario_type: b[27],
            script_type: b[28],
            mods: b[29],
        }
    }

    pub fn map_edge_world_cells(self) -> Option<i32> {
        MAP_SIZE_WORLD_EDGES.get(self.map_size as usize).copied()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitialPlayer {
    pub slot: u8,
    pub present: bool,
    /// The two-byte `Player::flags` gate written before the conditional body.
    pub flags: u16,
    /// Ten synchronized input counters, then `caravan_frame` and `pop_cap_frame`.
    pub counters_and_frames: [u32; 12],
    pub tribe: u8,
    pub who: u8,
    pub team: u8,
    pub handicap: u8,
    pub play: u8,
    pub pauses: u8,
    pub difficulty: u8,
    pub name: String,
}

impl InitialPlayer {
    fn absent(slot: u8, flags: u16) -> Self {
        Self {
            slot,
            present: false,
            flags,
            counters_and_frames: [0; 12],
            tribe: 0,
            who: 0,
            team: 0,
            handicap: 0,
            play: 0,
            pauses: 0,
            difficulty: 0,
            name: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModBlock {
    V16 {
        checksum: u32,
        total_size: u32,
        scenario_script: String,
        scenario_path: String,
        mod_name: String,
        checksum2: u32,
        total_size2: u32,
        mod_name2: String,
    },
    V15 {
        scenario_script: String,
        scenario_path: String,
        scenario_dir: String,
        mod_name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitialGameInfo {
    pub version_string: String,
    pub version: u32,
    pub seed: u32,
    pub checksum_deep: i32,
    pub checksum_window_size: i32,
    pub checksum_failure_threshold: i32,
    pub flags: u32,
    pub settings: GameSettings,
    pub players: Vec<InitialPlayer>,
    pub mods: ModBlock,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitialGame {
    pub frame: i32,
    pub frame_to_break: i32,
    pub playing: i32,
    pub loading: i32,
    pub tick: i32,
    pub market_tick: i32,
    pub market: [i32; 6],
    /// `Game::start_list` storage at `Game + 0x65c`. The replay snapshot precedes
    /// `Setup::build_game`, so ordinary recordings retain constructor zeroes here rather
    /// than the later shuffled Leader traversal.
    pub start_list: [i32; 8],
    /// `Game::start_index` storage at `Game + 0x67c`, likewise captured before the later
    /// inverse traversal ordinals are written.
    pub start_index: [i32; 8],
    /// `Game::num_players` storage at `Game + 0x69c`; ordinary replay initial images carry
    /// zero because setup has not counted the Player bodies yet.
    pub num_players: i32,
    pub world_cities: i32,
    pub world_villages: i32,
    pub total_units: i32,
    pub everyone_mask: i32,
    pub armageddon: i32,
    pub semaphore_bits: i32,
    pub semaphore: Vec<u8>,
    pub graphic_tick: i32,
}

/// Parsed setup and the exact number of prefix bytes consumed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitialState {
    /// `sGameSaveVersion`, inferred as retail does not put the global version
    /// word in a recording.  The v15/v16 tail is selected only when the
    /// following `Game::semaphore` satisfies its PDB layout invariant.
    pub save_format: u32,
    pub info: InitialGameInfo,
    pub game: InitialGame,
    pub save_name: String,
    pub bytes_walked: usize,
    /// SHA-256 of the complete decompressed replay payload. Ownership claims
    /// bind to content, never to a filename or corpus position.
    pub payload_sha256: [u8; 32],
    /// Exact decompressed-payload locations of the procedural world inputs.
    pub worldgen_sources: WorldgenSourceSpans,
    /// Static rules recovered from the replay's own SaveGame section.
    ///
    /// `None` is fail-closed: conquest/custom recordings may omit this section,
    /// and a structurally valid section is still rejected unless every
    /// independently captured cumulative checkpoint agrees with retail.
    pub rules: Option<InitialRules>,
}

/// Checksum-visible projection of the replay-carried static Rules section.
///
/// This is not a copied wire checksum. The parser independently replays the
/// exact `Game::walk_rules_data` ordering over the bytes written by the shared
/// `SaveGame` visitor, skipping only section tags and save-only strings exactly
/// where the shipped walkers gate on `DataWalk::is_checksum`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InitialRules {
    pub serialized_offset: usize,
    pub serialized_bytes: usize,
    /// SHA-256 of the exact serialized Rules span, including its outer tag.
    pub serialized_sha256: [u8; 32],
    pub walked_bytes: u64,
    pub checksum: u32,
    pub after_types: u32,
    pub after_constants: u32,
    pub after_balance: u32,
}

/// The largest world reconstruction justified by the prefix alone.
///
/// `world` follows the shipped map-size table and exact `World::init` dimension
/// arithmetic, then installs the replay seed through the oracle-backed
/// `Map::make` entry semantics.  Terrain, resource placement and starting
/// coordinates begin zero. The replay harness advances this same object through
/// exact procedural prefixes, but provisional bytes remain counted as
/// unsourced until the downstream generator proves their final values; this is
/// a divergence-producing checksum slice, not a fabricated complete save.
#[derive(Clone, Debug)]
pub struct InitialWorld {
    pub world: World,
    /// Authoritative procedural-generation region store. It is retained even
    /// before a region checksum producer is installed because map geometry and
    /// later terrain/item placement consume these exact coordinate lists.
    pub generation_regions: Regions,
    pub checksum: WorldChecksum,
    /// Exact byte-level provenance for the canonical replay-derived world.
    ///
    /// Synthetic isolated fixtures may leave this absent, but production
    /// replay construction always installs it and derives coverage from it.
    pub ownership: Option<WorldOwnerLedger>,
    pub sourced_walked_bytes: u64,
}

/// Replay-carried, replay-varying inputs needed by the procedural map path.
///
/// This is deliberately not called a generated map. It is the exact tuple
/// which selects and seeds that map, plus the active player setup consumed by
/// start placement. Static map-style/tileset content is external to `.rcx`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitialWorldgenInputs {
    pub seed: u32,
    pub map_style: u8,
    pub map_size: u8,
    pub map_edge_world_cells: Option<i32>,
    pub players: u8,
    pub game_rules: u8,
    pub starting_town: u8,
    pub scenario_type: u8,
    pub active_slots: Vec<u8>,
    pub active_who: Vec<u8>,
    pub active_teams: Vec<u8>,
    pub sources: WorldgenSourceSpans,
}

/// Inputs required by initial goody placement for which a recording supplies
/// no serialized field at all.
///
/// `replay_bytes` is zero for every entry, not an unknown byte count: ordinary
/// `.rcx` setup stores selectors and a seed, then expects retail to regenerate
/// these runtime objects from installed static content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbsentReplayItemInput {
    pub name: &'static str,
    pub replay_bytes: usize,
    pub required_source: &'static str,
}

pub const ABSENT_REPLAY_ITEM_INPUTS: [AbsentReplayItemInput; 4] = [
    AbsentReplayItemInput {
        name: "selected_map_style",
        replay_bytes: 0,
        required_source: "ordered shipped map-style catalog and selected mapstyles/*.xml",
    },
    AbsentReplayItemInput {
        name: "terrain_group_tables",
        replay_bytes: 0,
        required_source: "resolved tileset, TerrainGroups, fractal, and partition data",
    },
    AbsentReplayItemInput {
        name: "generated_item_candidates",
        replay_bytes: 0,
        required_source: "generated WData/regions/starts/heights and goody placement schedule",
    },
    AbsentReplayItemInput {
        name: "post_worldgen_rng",
        replay_bytes: 0,
        required_source: "main Random state after all preceding map-generation draws",
    },
];

/// First fail-closed boundary reached while reconstructing initial items.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitialItemBoundary {
    /// Scenario setup may carry custom dimensions/state outside the procedural
    /// selector tuple. No generated scenario snapshot is present in `.rcx`.
    CustomScenarioState { scenario_type: u8 },
    /// The selector does not index the shipped seven-entry map-size table.
    UnknownMapSize { map_size: u8 },
    /// `Map::make` preserves prior RNG/world seed state for signed-negative
    /// arguments; that prior state is not serialized by replay setup.
    PriorSeedState { seed: u32 },
    /// The replay's exact static Rules section could not be admitted.
    StaticRulesUnavailable,
    /// The normal supported path reaches the external content boundary. The
    /// ordinal is exact; its ordered catalog entry and XML are not in `.rcx`.
    MapStyleContentUnavailable { map_style: u8 },
    /// The catalog entry, default XML, selected XML, exact terrain/goody
    /// expressions and known direct RNG sites are admitted. Producing the
    /// WData/regions/starts on which those placements operate now requires the
    /// concrete style's `make_continents` path and its branch-dependent draws.
    MapContinentGenerationUnavailable {
        map_style: u8,
        make_continents_va: u32,
    },
    /// The style prefix reached a concrete, unported geometry helper. The
    /// boundary string is stable report vocabulary, while the two addresses
    /// distinguish the virtual dispatch from its first unavailable callee.
    MapContinentPrimitiveUnavailable {
        boundary: &'static str,
        map_style: u8,
        make_continents_va: u32,
        primitive_va: u32,
    },
    /// Both common region passes, diagonal repair and coastline construction
    /// completed. TerrainGroups::fill_fertile is the next call.
    MapTerrainGroupsUnavailable { next_va: u32 },
    /// The exact fertility byte plane and partitions were consumed. Retail
    /// next enters `TerrainGroups::place_all` with terrain-group tables whose
    /// expression-resolved runtime representation remains upstream.
    MapTerrainGroupsPlaceAllUnavailable { next_va: u32 },
    /// `TerrainGroups::place_all` `0x006a70d0` was entered and executed from the
    /// replay-owned World and RNG until an exact retail primitive inside it
    /// could not be run. `boundary` is stable report vocabulary; `primitive_va`
    /// distinguishes which callee, and `group_index` which terrain-group arm
    /// reached it. Nothing is committed — see `crate::place_all_advance`.
    MapTerrainGroupsPlaceAllPrimitiveUnavailable {
        boundary: &'static str,
        place_all_va: u32,
        primitive_va: u32,
        group_index: Option<usize>,
        completed_groups: usize,
    },
}

impl InitialItemBoundary {
    pub fn name(self) -> &'static str {
        match self {
            Self::CustomScenarioState { .. } => "custom_scenario_state",
            Self::UnknownMapSize { .. } => "unknown_map_size",
            Self::PriorSeedState { .. } => "prior_seed_state",
            Self::StaticRulesUnavailable => "static_rules",
            Self::MapStyleContentUnavailable { .. } => "map_style_content",
            Self::MapContinentGenerationUnavailable { .. } => "map_continent_generation",
            Self::MapContinentPrimitiveUnavailable { boundary, .. } => boundary,
            Self::MapTerrainGroupsUnavailable { .. } => "terrain_groups_fill_fertile",
            Self::MapTerrainGroupsPlaceAllUnavailable { .. } => "terrain_groups_place_all",
            Self::MapTerrainGroupsPlaceAllPrimitiveUnavailable { boundary, .. } => boundary,
        }
    }
}

/// Executable prefix of initial item reconstruction.
///
/// A plan proves every replay-carried scalar and identifies the first input
/// which prevents `World::configure_items`/`place_goody` from being called.
/// The harness stores and executes this plan; a blocked plan never installs an
/// empty registry, because that would turn an absent producer into a false
/// channel-10 agreement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitialItemReconstruction {
    pub inputs: InitialWorldgenInputs,
    pub rules: Option<InitialRules>,
    /// Independently catalog-validated local install data. `None` is distinct
    /// from an initialized style whose `GOODIES` section is genuinely empty.
    pub style: Option<MapStyleStaticData>,
    /// Common `Map::make` continuation after a completed style virtual.
    pub post_continent: Option<crate::post_continent::PostContinentReceipt>,
    /// Tileset selection occurs during `load_map_data`, before orientation.
    pub tile_selection: Option<crate::fractal_boundary::TileSelectionBoundary>,
    /// Full installed static input and generated byte plane, when admitted.
    pub fertility: Option<crate::fractal_boundary::FertilityBoundary>,
    /// Exact reason the full fertility stage could not be admitted. Earlier
    /// independent continent work can still advance from `tile_selection`.
    pub fertility_error: Option<crate::fractal_boundary::FractalBoundaryError>,
    pub fill_fertile: Option<don_sim::systems::terrain_groups::FillFertileReceipt>,
    /// Executed `TerrainGroups::place_all` `0x006a70d0` survey: which group arms
    /// the transaction reached and the exact primitive it stopped at. Present
    /// only once `fill_fertile` has run; it commits no world state.
    pub place_all_advance: Option<crate::place_all_advance::PlaceAllAdvanceReceipt>,
    /// Why the executed survey could not be produced, if it could not.
    pub place_all_advance_error: Option<crate::place_all_advance::PlaceAllAdvanceError>,
    /// Why the shipped `MOUNTAINS` section could not be read, if it could not.
    /// `None` with a `place_all_advance` present means the three range lists
    /// were derived and the survey ran with them installed.
    pub mountain_range_error: Option<crate::place_all_advance::MountainRangeSourceError>,
    pub boundary: InitialItemBoundary,
}

/// Result of trying to attach initial items to the replay-derived terrain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InitialItemReconstructionError {
    StyleSelectorMismatch {
        replay_map_style: u8,
        static_map_style: u8,
    },
    StyleIdentityMismatch {
        map_style: u8,
    },
    MapPrefixMismatch {
        expected_xs: i32,
        expected_ys: i32,
        actual_xs: i32,
        actual_ys: i32,
        expected_seed: i32,
        actual_seed: i32,
    },
    ContinentPrefix(crate::continent::ContinentError),
    WorldOwnership(crate::replay_world_owner_transitions::ReplayWorldOwnerTransitionError),
    TileSelection(crate::fractal_boundary::FractalBoundaryError),
    PostContinent(crate::post_continent::PostContinentError),
    FillFertile(don_sim::systems::terrain_groups::FillFertileError),
    Blocked(InitialItemBoundary),
}

/// Atomic receipt chain for the reconstructed post-placement portion of
/// `Map::make` through source token `0x1ebe`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapTerrainRepairReceipt {
    pub check_player_forest: crate::check_player_forest::CheckPlayerForestReceipt,
    pub nubify_forest: crate::nubify_forest_frontier::NubifyForestReceipt,
    pub post_nubify_transitions:
        crate::post_nubify_transition_frontier::PostNubifyTransitionReceipt,
    /// Byte ranges newly owned by each exact World-mutating stage. Empty only
    /// for isolated synthetic maps which intentionally carry no owner ledger.
    pub ownership_transitions: Vec<crate::world_owner_frontier::TransitionReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapTerrainRepairError {
    CheckPlayerForest(crate::check_player_forest::CheckPlayerForestError),
    NubifyForest(crate::nubify_forest_frontier::NubifyForestError),
    PostNubifyTransitions(crate::post_nubify_transition_frontier::PostNubifyTransitionError),
    Ownership(crate::world_owner_frontier::WorldOwnerError),
}

fn receipt_digest(label: &str, receipt: &impl std::fmt::Debug) -> [u8; 32] {
    let encoded = format!("{label}\0{receipt:?}");
    sha256(encoded.as_bytes())
}

fn advance_world_ownership(
    map: &mut InitialWorld,
    proof: crate::world_owner_frontier::ExactPortTransitionProof,
) -> Result<
    (u64, Option<crate::world_owner_frontier::TransitionReceipt>),
    crate::world_owner_frontier::WorldOwnerError,
> {
    let Some(ledger) = map.ownership.as_mut() else {
        return Ok((map.sourced_walked_bytes, None));
    };
    let transition = ledger.advance_exact_port(&map.world, proof)?;
    let coverage = ledger.coverage();
    map.checksum = ledger.snapshot().checksum.clone();
    map.sourced_walked_bytes = coverage.owned_bytes as u64;
    Ok((map.sourced_walked_bytes, Some(transition)))
}

impl InitialItemReconstruction {
    /// Number of distinct replay bytes which directly carry the scalar tuple
    /// (4-byte seed plus six one-byte selectors/gates).
    pub const fn scalar_source_bytes(&self) -> usize {
        10
    }

    pub fn absent_replay_inputs(&self) -> &'static [AbsentReplayItemInput] {
        &ABSENT_REPLAY_ITEM_INPUTS
    }

    /// Continue the real `Map::make` schedule from a successful `place_all`
    /// receipt through the post-transition checksum deadline.
    ///
    /// World, checksum, and main-RNG handoffs are chained only through typed
    /// receipts and committed as one replay-side transaction. A late missing
    /// or stale selected-tileset fact therefore cannot expose the preceding
    /// forest writes. Observations made by `edge_host` remain outside this
    /// transaction, as they do for the standalone nubify adapter.
    pub fn advance_map_make_terrain_repairs<H: crate::nubify_forest_frontier::EdgeOfRegionHost>(
        &self,
        map: &mut InitialWorld,
        place_all: &crate::place_all_boundary::ReplayPlaceAllReceipt,
        forest_facts: crate::check_player_forest::CheckPlayerForestFacts,
        edge_host: &mut H,
        transition_facts: &crate::post_nubify_transition_frontier::TerrainTransitionLiveFacts,
    ) -> Result<MapTerrainRepairReceipt, MapTerrainRepairError> {
        let mut staged = map.clone();
        let mut check_player_forest = crate::check_player_forest::execute_check_player_forest(
            self,
            &mut staged,
            place_all,
            forest_facts,
        )
        .map_err(MapTerrainRepairError::CheckPlayerForest)?;
        let (sourced, check_player_ownership) = advance_world_ownership(
            &mut staged,
            crate::world_owner_frontier::ExactPortTransitionProof {
                entry_va: check_player_forest.entry_va,
                resume_va: check_player_forest.caller_resume_va,
                implementation_sha256: sha256(include_bytes!("check_player_forest.rs")),
                receipt_sha256: receipt_digest("Map::check_player_forest", &check_player_forest),
                proof_document: "docs/assembly/replay-check-player-forest.md",
                input_checksum: check_player_forest.checksum_before.full,
                output_checksum: check_player_forest.checksum_after.full,
                allowed_sections: crate::world_owner_frontier::WorldSectionMask::only(
                    don_sim::systems::map_terrain::WorldSection::WData,
                ),
            },
        )
        .map_err(MapTerrainRepairError::Ownership)?;
        check_player_forest.sourced_walked_bytes = sourced;
        let mut nubify_forest = crate::nubify_forest_frontier::execute_nubify_forest_frontier(
            &mut staged,
            &check_player_forest,
            edge_host,
        )
        .map_err(MapTerrainRepairError::NubifyForest)?;
        let (sourced, nubify_ownership) = advance_world_ownership(
            &mut staged,
            crate::world_owner_frontier::ExactPortTransitionProof {
                entry_va: nubify_forest.entry_va,
                resume_va: nubify_forest.caller_resume_va,
                implementation_sha256: sha256(include_bytes!("nubify_forest_frontier.rs")),
                receipt_sha256: receipt_digest("TerrainGroups::nubify_forest", &nubify_forest),
                proof_document: "docs/assembly/replay-nubify-forest-frontier.md",
                input_checksum: nubify_forest.checksum_before.full,
                output_checksum: nubify_forest.checksum_after.full,
                allowed_sections: crate::world_owner_frontier::WorldSectionMask::only(
                    don_sim::systems::map_terrain::WorldSection::WData,
                ),
            },
        )
        .map_err(MapTerrainRepairError::Ownership)?;
        nubify_forest.sourced_walked_bytes = sourced;
        let mut post_nubify_transitions =
            crate::post_nubify_transition_frontier::execute_post_nubify_transitions(
                &mut staged,
                &nubify_forest,
                transition_facts,
            )
            .map_err(MapTerrainRepairError::PostNubifyTransitions)?;
        let (sourced, post_nubify_ownership) = advance_world_ownership(
            &mut staged,
            crate::world_owner_frontier::ExactPortTransitionProof {
                entry_va: post_nubify_transitions.caller_resume_va,
                resume_va: post_nubify_transitions.next_checkpoint_call_va,
                implementation_sha256: sha256(include_bytes!("post_nubify_transition_frontier.rs")),
                receipt_sha256: receipt_digest(
                    "Map::post_nubify_transitions",
                    &post_nubify_transitions,
                ),
                proof_document: "docs/assembly/replay-post-nubify-transition-frontier.md",
                input_checksum: post_nubify_transitions.checksum_before.full,
                output_checksum: post_nubify_transitions.checksum_after.full,
                allowed_sections: crate::world_owner_frontier::WorldSectionMask::only(
                    don_sim::systems::map_terrain::WorldSection::WData,
                ),
            },
        )
        .map_err(MapTerrainRepairError::Ownership)?;
        post_nubify_transitions.sourced_walked_bytes = sourced;

        let ownership_transitions = [
            check_player_ownership,
            nubify_ownership,
            post_nubify_ownership,
        ]
        .into_iter()
        .flatten()
        .collect();

        *map = staged;
        Ok(MapTerrainRepairReceipt {
            check_player_forest,
            nubify_forest,
            post_nubify_transitions,
            ownership_transitions,
        })
    }

    /// Execute the admitted prefix against the exact terrain owner.
    ///
    /// Until the selected map-style content and upstream generator are wired,
    /// success is intentionally impossible. The important executable contract
    /// is that a shape/seed mismatch is distinguished from the named content
    /// boundary and neither path mutates `sim` or installs its item runtime.
    pub fn apply(
        &self,
        _sim: &mut don_sim::World,
        map: &mut World,
    ) -> Result<(), InitialItemReconstructionError> {
        if !matches!(
            self.boundary,
            InitialItemBoundary::MapStyleContentUnavailable { .. }
                | InitialItemBoundary::MapContinentGenerationUnavailable { .. }
                | InitialItemBoundary::MapContinentPrimitiveUnavailable { .. }
                | InitialItemBoundary::MapTerrainGroupsUnavailable { .. }
                | InitialItemBoundary::MapTerrainGroupsPlaceAllUnavailable { .. }
                | InitialItemBoundary::MapTerrainGroupsPlaceAllPrimitiveUnavailable { .. }
        ) {
            return Err(InitialItemReconstructionError::Blocked(self.boundary));
        }
        let expected_edge = self.inputs.map_edge_world_cells.unwrap_or(0);
        let expected_seed = self.inputs.seed as i32;
        if (map.xs, map.ys, map.seed) != (expected_edge, expected_edge, expected_seed) {
            return Err(InitialItemReconstructionError::MapPrefixMismatch {
                expected_xs: expected_edge,
                expected_ys: expected_edge,
                actual_xs: map.xs,
                actual_ys: map.ys,
                expected_seed,
                actual_seed: map.seed,
            });
        }
        Err(InitialItemReconstructionError::Blocked(self.boundary))
    }

    /// Execute the admitted style virtual up to its first unported primitive,
    /// then replace the generic continent boundary with that exact stop.
    pub fn advance_continent_prefix(
        &mut self,
        map: &mut InitialWorld,
    ) -> Result<crate::continent::ContinentReceipt, InitialItemReconstructionError> {
        self.advance_continent_prefix_with_optional_tilesets(map, None)
    }

    /// Execute the real `Map::load_map_data` -> continent -> coastline ->
    /// `fill_fertile` chain. The caller owns the lawful installed
    /// `Data/tilesets.xml` path; shipped bytes are never embedded in replay
    /// state or this crate.
    pub fn advance_continent_prefix_with_tilesets(
        &mut self,
        map: &mut InitialWorld,
        tilesets_xml: &Path,
    ) -> Result<crate::continent::ContinentReceipt, InitialItemReconstructionError> {
        self.advance_continent_prefix_with_optional_tilesets(map, Some(tilesets_xml))
    }

    fn advance_continent_prefix_with_optional_tilesets(
        &mut self,
        map: &mut InitialWorld,
        tilesets_xml: Option<&Path>,
    ) -> Result<crate::continent::ContinentReceipt, InitialItemReconstructionError> {
        if !matches!(
            self.boundary,
            InitialItemBoundary::MapContinentGenerationUnavailable { .. }
        ) {
            return Err(InitialItemReconstructionError::Blocked(self.boundary));
        }
        let style =
            self.style
                .as_ref()
                .ok_or(InitialItemReconstructionError::StyleIdentityMismatch {
                    map_style: self.inputs.map_style,
                })?;
        let replay_inputs = crate::fractal_boundary::FractalReplayInputs {
            seed: self.inputs.seed,
            map_style: self.inputs.map_style,
            scenario_type: self.inputs.scenario_type,
            world_edge: self.inputs.map_edge_world_cells,
        };
        let (tile_selection, fertility, fertility_error) = match tilesets_xml {
            Some(tilesets_xml) => {
                let sources = crate::fractal_boundary::FractalBoundarySources {
                    default_map_style_xml: style.default_source.path.clone(),
                    selected_map_style_xml: style.selected_source.path.clone(),
                    tilesets_xml: tilesets_xml.to_path_buf(),
                };
                match crate::fractal_boundary::resolve_fertility_boundary(replay_inputs, &sources) {
                    Ok(fertility) => (fertility.tile_selection(), Some(fertility), None),
                    Err(error) => {
                        let selection = crate::fractal_boundary::resolve_tile_selection(
                            self.inputs.seed,
                            &style.default_source.path,
                            &style.selected_source.path,
                        )
                        .map_err(InitialItemReconstructionError::TileSelection)?;
                        (selection, None, Some(error))
                    }
                }
            }
            None => {
                let selection = crate::fractal_boundary::resolve_tile_selection(
                    self.inputs.seed,
                    &style.default_source.path,
                    &style.selected_source.path,
                )
                .map_err(InitialItemReconstructionError::TileSelection)?;
                (
                    selection,
                    None,
                    Some(
                        crate::fractal_boundary::FractalBoundaryError::MissingInstalledTilesetsSource,
                    ),
                )
            }
        };
        // The style virtual and its common continuation are one Map::make
        // transaction from the replay reconstructor's perspective.  Stage both
        // authoritative stores so a late region-shape failure cannot expose a
        // partially generated world while leaving this plan at its old boundary.
        let mut staged_map = map.clone();
        let receipt = crate::continent::execute_continent_prefix_with_regions_from_rng(
            &self.inputs,
            style,
            tile_selection.main_random_state_after,
            &mut staged_map.world,
            &mut staged_map.generation_regions,
        )
        .map_err(InitialItemReconstructionError::ContinentPrefix)?;
        crate::replay_world_owner_transitions::advance_continent_world_ownership(
            &mut staged_map,
            &receipt,
        )
        .map_err(InitialItemReconstructionError::WorldOwnership)?;
        let mut post_continent = None;
        let mut fill_fertile = None;
        let boundary = match &receipt.stop {
            crate::continent::ContinentStop::HookComplete { next_va } => {
                debug_assert_eq!(*next_va, crate::post_continent::REGIONS_CLEAR_ALL_VA);
                let limits =
                    crate::post_continent::TerritoryLimits::from_world_prefix(&staged_map.world);
                let post = crate::post_continent::execute_post_continent(
                    &mut staged_map.world,
                    &mut staged_map.generation_regions,
                    limits,
                )
                .map_err(InitialItemReconstructionError::PostContinent)?;
                crate::replay_world_owner_transitions::advance_post_continent_world_ownership(
                    &mut staged_map,
                    &post,
                )
                .map_err(InitialItemReconstructionError::WorldOwnership)?;
                debug_assert_eq!(
                    post.next_va,
                    crate::fractal_boundary::TERRAIN_GROUPS_FILL_FERTILE_VA
                );
                post_continent = Some(post);
                if let Some(fertility) = &fertility {
                    let terrain_groups = fertility.terrain_groups_input();
                    let fill = terrain_groups
                        .fill_fertile(&mut staged_map.world)
                        .map_err(InitialItemReconstructionError::FillFertile)?;
                    crate::replay_world_owner_transitions::advance_fill_fertile_world_ownership(
                        &mut staged_map,
                        &fill,
                    )
                    .map_err(InitialItemReconstructionError::WorldOwnership)?;
                    fill_fertile = Some(fill);
                    InitialItemBoundary::MapTerrainGroupsPlaceAllUnavailable {
                        next_va: crate::fractal_boundary::TERRAIN_GROUPS_PLACE_ALL_VA,
                    }
                } else {
                    InitialItemBoundary::MapTerrainGroupsUnavailable {
                        next_va: crate::fractal_boundary::TERRAIN_GROUPS_FILL_FERTILE_VA,
                    }
                }
            }
            crate::continent::ContinentStop::MakeRegion { primitive_va, .. } => {
                InitialItemBoundary::MapContinentPrimitiveUnavailable {
                    boundary: "map_region_seed",
                    map_style: receipt.map_style,
                    make_continents_va: receipt.make_continents_va,
                    primitive_va: *primitive_va,
                }
            }
            crate::continent::ContinentStop::GrowRegion { primitive_va, .. } => {
                InitialItemBoundary::MapContinentPrimitiveUnavailable {
                    boundary: "map_region_growth",
                    map_style: receipt.map_style,
                    make_continents_va: receipt.make_continents_va,
                    primitive_va: *primitive_va,
                }
            }
            crate::continent::ContinentStop::EastIndiesNonplayerIslands { next_rng_va } => {
                InitialItemBoundary::MapContinentPrimitiveUnavailable {
                    boundary: "map_east_indies_nonplayer_islands",
                    map_style: receipt.map_style,
                    make_continents_va: receipt.make_continents_va,
                    primitive_va: *next_rng_va,
                }
            }
            crate::continent::ContinentStop::RetryGeneration { .. } => {
                InitialItemBoundary::MapContinentPrimitiveUnavailable {
                    boundary: "map_continent_retry",
                    map_style: receipt.map_style,
                    make_continents_va: receipt.make_continents_va,
                    primitive_va: crate::continent::MAP_GROW_REGION_VA,
                }
            }
            crate::continent::ContinentStop::FillCont { primitive_va, .. } => {
                InitialItemBoundary::MapContinentPrimitiveUnavailable {
                    boundary: "map_team_continent_partition",
                    map_style: receipt.map_style,
                    make_continents_va: receipt.make_continents_va,
                    primitive_va: *primitive_va,
                }
            }
            crate::continent::ContinentStop::FindRegionCentroid { primitive_va, .. } => {
                InitialItemBoundary::MapContinentPrimitiveUnavailable {
                    boundary: "map_team_continent_centroid",
                    map_style: receipt.map_style,
                    make_continents_va: receipt.make_continents_va,
                    primitive_va: *primitive_va,
                }
            }
            crate::continent::ContinentStop::EliminateEdgeCanals { primitive_va, .. } => {
                InitialItemBoundary::MapContinentPrimitiveUnavailable {
                    boundary: "map_team_continent_edge_canals",
                    map_style: receipt.map_style,
                    make_continents_va: receipt.make_continents_va,
                    primitive_va: *primitive_va,
                }
            }
            crate::continent::ContinentStop::PlaceStartInRegion { primitive_va, .. } => {
                InitialItemBoundary::MapContinentPrimitiveUnavailable {
                    boundary: "map_team_continent_place_start",
                    map_style: receipt.map_style,
                    make_continents_va: receipt.make_continents_va,
                    primitive_va: *primitive_va,
                }
            }
            crate::continent::ContinentStop::AddStartingLocation {
                next_mutator_va, ..
            } => InitialItemBoundary::MapContinentPrimitiveUnavailable {
                boundary: "map_team_continent_post_checksum_string_close",
                map_style: receipt.map_style,
                make_continents_va: receipt.make_continents_va,
                primitive_va: *next_mutator_va,
            },
            crate::continent::ContinentStop::EastMeetsWestStartFallback { next_va, .. } => {
                InitialItemBoundary::MapContinentPrimitiveUnavailable {
                    boundary: "map_team_continent_start_fallback",
                    map_style: receipt.map_style,
                    make_continents_va: receipt.make_continents_va,
                    primitive_va: *next_va,
                }
            }
        };
        *map = staged_map;
        self.post_continent = post_continent;
        self.tile_selection = Some(tile_selection);
        self.fertility = fertility;
        self.fertility_error = fertility_error;
        self.fill_fertile = fill_fertile;
        self.boundary = boundary;
        // Enter `TerrainGroups::place_all` 0x006a70d0 and replace its entry
        // address with the exact primitive inside it that cannot be executed.
        // The call is fail-closed in don-sim, so this reads state and commits
        // none: `map.world` is byte-identical afterwards either way.
        let mountain_source = tilesets_xml
            .and_then(Path::parent)
            .map(|data_dir| data_dir.join(crate::place_all_advance::MOUNTAIN_RANGE_SOURCE_FILE));
        let mut mountain_error = None;
        if matches!(
            self.boundary,
            InitialItemBoundary::MapTerrainGroupsPlaceAllUnavailable { .. }
        ) {
            // The three `Mountains` range lists survive `World::wipe`'s
            // `Mountains::clear` call at 0x006b2d97 and are built by
            // `Mountains::init` 0x0089ad70 from the shipped MOUNTAINS section,
            // which sits beside tilesets.xml in the same installed Data
            // directory. Their lengths decide how many words
            // `randomize_mountains` draws from the main stream.
            let mountains = mountain_source.and_then(|path| {
                match crate::place_all_advance::resolve_mountain_ranges(&path) {
                    Ok(mountains) => Some(mountains),
                    Err(error) => {
                        mountain_error = Some(error);
                        None
                    }
                }
            });
            let facts = crate::place_all_advance::PlaceAllAdvanceFacts {
                mountains,
                helping: Some(crate::place_all_advance::initial_region_helping_state(
                    &map.world,
                )),
                doober_rules: self.fertility.as_ref().map(|f| f.doober_rules),
                // The reporting tail reads `player_scores` after
                // `place_region_group` has accumulated into it; the shipped
                // port does not model that accumulation yet.
                reporting: None,
                progress: crate::place_all_advance::RETAIL_GAME_START_PROGRESS,
                oil_good_policy: crate::place_all_advance::OilGoodPolicy::Stop,
            };
            // This is a read-only survey: it retains exact leaf receipts but
            // commits neither World nor owner state. Reconstruct the lawful
            // fresh-process entry owner locally; a future mutating schedule
            // must retain its owner runtime across subsequent Good producers.
            let owner_initialization =
                replay_place_all_owners::ReplayPlaceAllOwnerInitialization::cold_process();
            let entry_owners = owner_initialization.entry_owners(map.world.wdata.len());
            match crate::place_all_advance::advance_place_all_boundary_owned(
                self,
                map,
                &receipt,
                &facts,
                &entry_owners,
            ) {
                Ok(advance) => {
                    self.boundary =
                        InitialItemBoundary::MapTerrainGroupsPlaceAllPrimitiveUnavailable {
                            boundary: advance.stop.name(),
                            place_all_va: advance.entry_va,
                            primitive_va: advance.stop.primitive_va(),
                            group_index: advance.stop.group_index(),
                            completed_groups: advance.completed_groups.len(),
                        };
                    self.place_all_advance = Some(advance);
                }
                Err(error) => self.place_all_advance_error = Some(error),
            }
        }
        self.mountain_range_error = mountain_error;
        Ok(receipt)
    }
}

impl InitialWorld {
    /// Canonical byte coverage. Production replay worlds carry a ledger; the
    /// scalar remains only as a compatibility mirror for isolated fixtures and
    /// receipt types which have not yet moved the full owner map.
    pub fn exact_sourced_walked_bytes(&self) -> u64 {
        self.ownership
            .as_ref()
            .map_or(self.sourced_walked_bytes, |ledger| {
                ledger.coverage().owned_bytes as u64
            })
    }

    pub fn ownership_is_coherent(&self) -> bool {
        match &self.ownership {
            None => true,
            Some(ledger) => {
                let coverage = ledger.coverage();
                coverage.walked_bytes as u64 == self.checksum.bytes
                    && coverage.owned_bytes as u64 == self.sourced_walked_bytes
                    && ledger.snapshot().checksum == self.checksum
            }
        }
    }

    pub fn unsourced_walked_bytes(&self) -> u64 {
        self.checksum
            .bytes
            .saturating_sub(self.exact_sourced_walked_bytes())
    }
}

impl InitialState {
    pub fn active_players(&self) -> impl Iterator<Item = &InitialPlayer> {
        self.info.players.iter().filter(|p| p.present)
    }

    /// Project the exact replay-varying tuple consumed by procedural world
    /// generation, retaining the source byte locations for mutation tests and
    /// diagnostics.
    pub fn worldgen_inputs(&self) -> InitialWorldgenInputs {
        InitialWorldgenInputs {
            seed: self.info.seed,
            map_style: self.info.settings.map_style,
            map_size: self.info.settings.map_size,
            map_edge_world_cells: self.info.settings.map_edge_world_cells(),
            players: self.info.settings.players,
            game_rules: self.info.settings.game_rules,
            starting_town: self.info.settings.starting_town,
            scenario_type: self.info.settings.scenario_type,
            active_slots: self.active_players().map(|p| p.slot).collect(),
            active_who: self.active_players().map(|p| p.who).collect(),
            active_teams: self.active_players().map(|p| p.team).collect(),
            sources: self.worldgen_sources,
        }
    }

    /// Build the executable initial-item reconstruction prefix.
    ///
    /// Ordinary shipped recordings reach `MapStyleContentUnavailable`: their
    /// selector tuple and static Rules bytes are exact, but `.rcx` contains no
    /// selected map-style/tileset runtime data and no generated-map snapshot.
    pub fn reconstruct_items(&self) -> InitialItemReconstruction {
        let inputs = self.worldgen_inputs();
        let boundary = if inputs.scenario_type != 0 {
            InitialItemBoundary::CustomScenarioState {
                scenario_type: inputs.scenario_type,
            }
        } else if inputs.map_edge_world_cells.is_none() {
            InitialItemBoundary::UnknownMapSize {
                map_size: inputs.map_size,
            }
        } else if (inputs.seed as i32) < 0 {
            InitialItemBoundary::PriorSeedState { seed: inputs.seed }
        } else if self.rules.is_none() {
            InitialItemBoundary::StaticRulesUnavailable
        } else {
            InitialItemBoundary::MapStyleContentUnavailable {
                map_style: inputs.map_style,
            }
        };
        InitialItemReconstruction {
            inputs,
            rules: self.rules,
            style: None,
            post_continent: None,
            tile_selection: None,
            fertility: None,
            fertility_error: None,
            fill_fertile: None,
            place_all_advance: None,
            place_all_advance_error: None,
            mountain_range_error: None,
            boundary,
        }
    }

    /// Advance the reconstruction through the selected static content.
    ///
    /// The supplied style has already validated all 23 catalog keys and parsed
    /// both default and selected XML. The replay ordinal is checked again here
    /// so callers cannot accidentally attach a valid-but-wrong map style.
    pub fn reconstruct_items_with_style(
        &self,
        style: MapStyleStaticData,
    ) -> Result<InitialItemReconstruction, InitialItemReconstructionError> {
        let mut plan = self.reconstruct_items();
        if !matches!(
            plan.boundary,
            InitialItemBoundary::MapStyleContentUnavailable { .. }
        ) {
            return Ok(plan);
        }
        if style.identity.ordinal != plan.inputs.map_style {
            return Err(InitialItemReconstructionError::StyleSelectorMismatch {
                replay_map_style: plan.inputs.map_style,
                static_map_style: style.identity.ordinal,
            });
        }
        if crate::map_style::SHIPPED_MAP_STYLE_CATALOG.get(plan.inputs.map_style as usize)
            != Some(&style.identity)
        {
            return Err(InitialItemReconstructionError::StyleIdentityMismatch {
                map_style: plan.inputs.map_style,
            });
        }
        let make_continents_va = style.identity.make_continents_va.ok_or(
            InitialItemReconstructionError::StyleIdentityMismatch {
                map_style: plan.inputs.map_style,
            },
        )?;
        plan.boundary = InitialItemBoundary::MapContinentGenerationUnavailable {
            map_style: plan.inputs.map_style,
            make_continents_va,
        };
        plan.style = Some(style);
        Ok(plan)
    }

    /// Apply only prefix-proven setup to the sim's exact world checksum owner.
    pub fn reconstruct_world(&self) -> Option<InitialWorld> {
        // Non-zero scenario types may carry custom dimensions in the still-
        // opaque state block. `map_size` is then UI/setup metadata, not proof
        // that `World::xs/ys` equal the shipped procedural size entry.
        if self.info.settings.scenario_type != 0 {
            return None;
        }
        let edge = self.info.settings.map_edge_world_cells()?;
        let mut world = World::init_default_rules(edge, edge);
        // `GameInfo::seed` is unsigned, but Map::make's argument is signed and
        // its exact prefix preserves prior state for negative values.
        world.seed_map_generation(self.info.seed as i32)?;
        let checksum_before_wipe = world.checksum_sections();
        let rules = self.rules.map(|rules| RulesWorldEvidence {
            serialized_span: ReplaySpan::new(rules.serialized_offset, rules.serialized_bytes),
            serialized_sha256: rules.serialized_sha256,
            checksum: rules.checksum,
            after_constants: rules.after_constants,
            player_base: 44,
            player_civic: 4,
            player_city: 4,
        });
        let mut ownership = WorldOwnerLedger::from_initial_prefix(
            &world,
            InitialWorldPrefixEvidence {
                replay_sha256: self.payload_sha256,
                map_size: self.info.settings.map_size,
                map_size_span: ReplaySpan::new(
                    self.worldgen_sources.map_size.offset,
                    self.worldgen_sources.map_size.bytes,
                ),
                seed: self.info.seed,
                seed_span: ReplaySpan::new(
                    self.worldgen_sources.seed.offset,
                    self.worldgen_sources.seed.bytes,
                ),
                rules,
            },
        )
        .ok()?;
        let wipe = crate::world_tdata_frontier::execute_tdata_and_fog_wipe(&mut world).ok()?;
        let written_ranges: Vec<_> = wipe
            .written_ranges
            .iter()
            .map(|written| ExactPortWrittenRange {
                section: written.plane.section(),
                offset: written.range.start,
                bytes: written.range.len(),
                producer_va: written.producer_va,
            })
            .collect();
        let checksum_after_wipe = world.checksum_sections();
        ownership
            .advance_exact_written_port(
                &world,
                ExactPortTransitionProof {
                    entry_va: wipe.entry_va,
                    resume_va: wipe.resume_va,
                    implementation_sha256: sha256(include_bytes!("world_tdata_frontier.rs")),
                    receipt_sha256: receipt_digest("World::wipe", &wipe),
                    proof_document: crate::world_tdata_frontier::PROOF_DOCUMENT,
                    input_checksum: checksum_before_wipe.full,
                    output_checksum: checksum_after_wipe.full,
                    allowed_sections: WorldSectionMask::only(WorldSection::TDataAndFog)
                        .with(WorldSection::WCoordSeen),
                },
                &written_ranges,
            )
            .ok()?;
        let sourced_walked_bytes = ownership.coverage().owned_bytes as u64;
        Some(InitialWorld {
            world,
            generation_regions: Regions::default(),
            checksum: ownership.snapshot().checksum.clone(),
            ownership: Some(ownership),
            sourced_walked_bytes,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub offset: usize,
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "initial prefix at {:#x}: {}", self.offset, self.message)
    }
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn new(b: &'a [u8]) -> Self {
        Self { b, p: 0 }
    }

    fn err(&self, message: impl Into<String>) -> ParseError {
        ParseError {
            offset: self.p,
            message: message.into(),
        }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], ParseError> {
        let end = self
            .p
            .checked_add(n)
            .ok_or_else(|| self.err("offset overflow"))?;
        if end > self.b.len() {
            return Err(self.err(format!(
                "need {n} bytes, only {} remain",
                self.b.len() - self.p
            )));
        }
        let out = &self.b[self.p..end];
        self.p = end;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, ParseError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, ParseError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32, ParseError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i32(&mut self) -> Result<i32, ParseError> {
        Ok(self.u32()? as i32)
    }
    fn string(&mut self) -> Result<String, ParseError> {
        let n = self.u32()? as usize;
        if n > 32_768 {
            return Err(self.err(format!("absurd UTF-16 string length {n}")));
        }
        let raw = self.take(
            n.checked_mul(2)
                .ok_or_else(|| self.err("string overflow"))?,
        )?;
        let words = raw
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]));
        String::from_utf16(&words.collect::<Vec<_>>())
            .map_err(|_| self.err("invalid UTF-16 string"))
    }
    fn tag(&mut self, want: u8, name: &str) -> Result<(), ParseError> {
        let got = self.u8()?;
        if got == want {
            Ok(())
        } else {
            Err(self.err(format!("{name} tag {got:#04x}, expected {want:#04x}")))
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct RulesAdler {
    checksum: u32,
    bytes: u64,
}

impl RulesAdler {
    fn new() -> Self {
        Self {
            checksum: 1,
            bytes: 0,
        }
    }

    fn update(&mut self, bytes: &[u8]) {
        self.checksum = adler32(self.checksum, bytes);
        self.bytes += bytes.len() as u64;
    }
}

fn rules_walk(r: &mut Reader<'_>, adler: &mut RulesAdler, n: usize) -> Result<(), ParseError> {
    let bytes = r.take(n)?;
    adler.update(bytes);
    Ok(())
}

fn rules_skip_string(r: &mut Reader<'_>) -> Result<(), ParseError> {
    let n = r.u32()? as usize;
    if n > 32_768 {
        return Err(r.err(format!("absurd Rules UTF-16 string length {n}")));
    }
    let raw = r.take(
        n.checked_mul(2)
            .ok_or_else(|| r.err("Rules string overflow"))?,
    )?;
    let words = raw
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]));
    if std::char::decode_utf16(words).any(|c| c.is_err()) {
        return Err(r.err("invalid Rules UTF-16 string"));
    }
    Ok(())
}

fn rules_walk_u16_array(r: &mut Reader<'_>, adler: &mut RulesAdler) -> Result<usize, ParseError> {
    let raw_count = r.take(4)?;
    let count = i32::from_le_bytes(raw_count.try_into().unwrap());
    adler.update(raw_count);
    if !(0..=TYPE_SLOTS as i32).contains(&count) {
        return Err(r.err(format!(
            "Rules u16 array count {count} outside 0..={TYPE_SLOTS}"
        )));
    }
    if count == 0 {
        return Ok(0);
    }

    // `SimpleArray<unsigned short>::walk_data` (`0x00476610`) writes these
    // seven bytes only for a non-empty array: capacity, grow, flags & 0xbf.
    let metadata = r.take(7)?;
    let capacity = i32::from_le_bytes(metadata[..4].try_into().unwrap());
    if capacity < count {
        return Err(r.err(format!(
            "Rules u16 array capacity {capacity} below count {count}"
        )));
    }
    if metadata[6] & 0x40 != 0 {
        return Err(r.err("Rules u16 array retained masked flag 0x40"));
    }
    adler.update(metadata);
    rules_walk(r, adler, count as usize * 2)?;
    Ok(count as usize)
}

/// Parse one candidate `Game::walk_rules_data` SaveGame section and project it
/// through the checksum-only traversal.
///
/// The four checkpoint comparisons are admission gates, not values substituted
/// into the result. A one-byte mutation in Types, Constants, Balance, or Tribes
/// changes the independently computed accumulator and makes this function
/// refuse the section.
pub fn parse_serialized_rules_at(
    payload: &[u8],
    offset: usize,
) -> Result<InitialRules, ParseError> {
    let section = payload.get(offset..).ok_or(ParseError {
        offset,
        message: "Rules offset outside payload".into(),
    })?;
    let result = (|| {
        let mut r = Reader::new(section);
        let mut adler = RulesAdler::new();
        r.tag(TAG_RULES, "Rules")?;

        // `Types::walk_rules_data` (`0x00669800`) dispatches 806 records in
        // global TypeIndex order. The dynamic kind intervals are fixed by the
        // shipped registries and independently checked by the live Type capture.
        let mut array_elements = 0usize;
        for slot in 0..TYPE_SLOTS {
            let kind = match slot {
                0..=49 => 0,    // GoodType
                50..=413 => 1,  // UnitType
                414..=542 => 2, // BuildType
                543 => 3,       // ItemType -> ObjectType walker
                544..=628 => 4, // TechType
                629..=683 => 5, // SpellType
                _ => 6,         // BonusType -> Type walker
            };

            // Type::walk_rules_data (`0x00663190`): [this+4,this+0x5e),
            // followed in SaveGame only by `name` String::walk_data.
            rules_walk(&mut r, &mut adler, 90)?;
            rules_skip_string(&mut r)?;

            if kind <= 3 {
                // ObjectType::walk_rules_data (`0x0065fba0`).
                rules_walk(&mut r, &mut adler, 152)?;
                array_elements += rules_walk_u16_array(&mut r, &mut adler)?;
                array_elements += rules_walk_u16_array(&mut r, &mut adler)?;
            }

            match kind {
                0 => rules_walk(&mut r, &mut adler, 68)?,
                // Unit's four consecutive calls total 24 + 8 + 4 + 756.
                1 => rules_walk(&mut r, &mut adler, 792)?,
                2 => rules_walk(&mut r, &mut adler, 49)?,
                4 => {
                    rules_walk(&mut r, &mut adler, 27)?;
                    // TechType saves eight additional strings, all gated out
                    // of CheckSum by `DataWalk+0x08`.
                    for _ in 0..8 {
                        rules_skip_string(&mut r)?;
                    }
                }
                5 => rules_walk(&mut r, &mut adler, 48)?,
                3 | 6 => {}
                _ => unreachable!(),
            }
        }
        if array_elements != 2_363 {
            return Err(r.err(format!(
                "Rules Type arrays contain {array_elements} elements, expected 2363"
            )));
        }
        if r.p != 1 + SHIPPED_TYPES_SERIALIZED_BYTES {
            return Err(r.err(format!(
                "Rules Types serialized width {}, expected {}",
                r.p - 1,
                SHIPPED_TYPES_SERIALIZED_BYTES
            )));
        }
        let after_types = adler.checksum;
        if after_types != RETAIL_AFTER_TYPES {
            return Err(r.err(format!(
                "Rules Types checkpoint {after_types:#010x}, expected {RETAIL_AFTER_TYPES:#010x}"
            )));
        }

        let constants_at = r.p;
        rules_walk(&mut r, &mut adler, RULES_BLOCK_BYTES)?;
        let duplicate: [u8; 4] = r.take(4)?.try_into().unwrap();
        let source: [u8; 4] = section
            [constants_at + RULES_DUPLICATE_OFFSET..constants_at + RULES_DUPLICATE_OFFSET + 4]
            .try_into()
            .unwrap();
        if duplicate != source {
            return Err(r.err("Rules Constants duplicate visit does not repeat +0x804"));
        }
        adler.update(&duplicate);
        let after_constants = adler.checksum;
        if after_constants != RETAIL_AFTER_CONSTANTS {
            return Err(r.err(format!(
                "Rules Constants checkpoint {after_constants:#010x}, expected {RETAIL_AFTER_CONSTANTS:#010x}"
            )));
        }

        rules_walk(&mut r, &mut adler, BALANCE_BYTES)?;
        let after_balance = adler.checksum;
        if after_balance != RETAIL_AFTER_BALANCE {
            return Err(r.err(format!(
                "Rules Balance checkpoint {after_balance:#010x}, expected {RETAIL_AFTER_BALANCE:#010x}"
            )));
        }

        for tribe in 0..TRIBE_COUNT {
            r.tag(TAG_TRIBE, &format!("Tribe[{tribe}]"))?;
            rules_walk(&mut r, &mut adler, 0x18)?;
            rules_walk(&mut r, &mut adler, TRIBE_SIZE - 0x70)?;
        }

        if r.p != SHIPPED_RULES_SERIALIZED_BYTES {
            return Err(r.err(format!(
                "Rules serialized width {}, expected {SHIPPED_RULES_SERIALIZED_BYTES}",
                r.p
            )));
        }
        if adler.bytes != RETAIL_WALKED_BYTES {
            return Err(r.err(format!(
                "Rules walked {} bytes, expected {RETAIL_WALKED_BYTES}",
                adler.bytes
            )));
        }
        if adler.checksum != RETAIL_AFTER_TRIBES || adler.checksum != SHIPPED_RULES_CHANNEL {
            return Err(r.err(format!(
                "Rules final checkpoint {:#010x}, expected {SHIPPED_RULES_CHANNEL:#010x}",
                adler.checksum
            )));
        }

        Ok(InitialRules {
            serialized_offset: offset,
            serialized_bytes: r.p,
            serialized_sha256: sha256(&section[..r.p]),
            walked_bytes: adler.bytes,
            checksum: adler.checksum,
            after_types,
            after_constants,
            after_balance,
        })
    })();
    result.map_err(|mut e: ParseError| {
        e.offset = e.offset.saturating_add(offset);
        e
    })
}

fn find_serialized_rules(
    payload: &[u8],
    search_from: usize,
) -> Result<Option<InitialRules>, ParseError> {
    let available_end = payload
        .len()
        .checked_sub(SHIPPED_RULES_SERIALIZED_BYTES)
        .map(|v| v + 1)
        .unwrap_or(0);
    let scan_end = search_from
        .saturating_add(RULES_SEARCH_BYTES)
        .min(available_end);
    let mut found = None;
    for offset in search_from.min(scan_end)..scan_end {
        if payload[offset] != TAG_RULES {
            continue;
        }
        let Ok(candidate) = parse_serialized_rules_at(payload, offset) else {
            continue;
        };
        if found.is_some() {
            return Err(ParseError {
                offset,
                message: "ambiguous duplicate shipped Rules sections".into(),
            });
        }
        found = Some(candidate);
    }
    Ok(found)
}

fn at_i32(body: &[u8], at: usize) -> i32 {
    i32::from_le_bytes([body[at], body[at + 1], body[at + 2], body[at + 3]])
}

fn parse_mod_block(r: &mut Reader<'_>, format: u32) -> Result<ModBlock, ParseError> {
    if format >= 16 {
        Ok(ModBlock::V16 {
            checksum: r.u32()?,
            total_size: r.u32()?,
            scenario_script: r.string()?,
            scenario_path: r.string()?,
            mod_name: r.string()?,
            checksum2: r.u32()?,
            total_size2: r.u32()?,
            mod_name2: r.string()?,
        })
    } else {
        Ok(ModBlock::V15 {
            scenario_script: r.string()?,
            scenario_path: r.string()?,
            scenario_dir: r.string()?,
            mod_name: r.string()?,
        })
    }
}

fn parse_candidate(payload: &[u8], format: u32) -> Result<InitialState, ParseError> {
    let mut r = Reader::new(payload);
    r.tag(TAG_GAME, "Game")?;
    r.tag(TAG_GAME_INFO, "GameInfo")?;
    let version_string = r.string()?;
    let version = r.u32()?;
    let seed_source = ReplayByteSpan::new(r.p, 4);
    let seed = r.u32()?;
    let checksum_deep = r.i32()?;
    let checksum_window_size = r.i32()?;
    let checksum_failure_threshold = r.i32()?;
    let flags = r.u32()?;
    let settings_offset = r.p;
    let settings = GameSettings::from_bytes(
        r.take(30)?
            .try_into()
            .map_err(|_| r.err("GameInfo settings width"))?,
    );

    let mut players = Vec::with_capacity(8);
    let mut player_flags = [ReplayByteSpan::new(0, 0); 8];
    let mut player_bodies = [None; 8];
    for slot in 0..8u8 {
        r.tag(TAG_PLAYER, "Player")?;
        player_flags[slot as usize] = ReplayByteSpan::new(r.p, 2);
        let flags = r.u16()?;
        if flags & 1 == 0 {
            players.push(InitialPlayer::absent(slot, flags));
            continue;
        }
        player_bodies[slot as usize] = Some(ReplayByteSpan::new(r.p, 0x39));
        let body = r.take(0x39)?;
        let mut counters_and_frames = [0u32; 12];
        for (i, v) in counters_and_frames.iter_mut().enumerate() {
            let at = i * 4;
            *v = u32::from_le_bytes([body[at], body[at + 1], body[at + 2], body[at + 3]]);
        }
        let body_flags = u16::from_le_bytes([body[0x30], body[0x31]]);
        if body_flags != flags {
            return Err(r.err(format!(
                "Player[{slot}] flags disagree: gate={flags:#06x}, body={body_flags:#06x}"
            )));
        }
        players.push(InitialPlayer {
            slot,
            present: true,
            flags,
            counters_and_frames,
            tribe: body[0x32],
            who: body[0x33],
            team: body[0x34],
            handicap: body[0x35],
            play: body[0x36],
            pauses: body[0x37],
            difficulty: body[0x38],
            name: r.string()?,
        });
    }
    let mods = parse_mod_block(&mut r, format)?;

    let body = r.take(404)?;
    let mut market = [0i32; 6];
    for (i, v) in market.iter_mut().enumerate() {
        *v = at_i32(body, 0x18 + i * 4);
    }
    let mut start_list = [0i32; 8];
    for (i, v) in start_list.iter_mut().enumerate() {
        *v = at_i32(body, 0x10c + i * 4);
    }
    let mut start_index = [0i32; 8];
    for (i, v) in start_index.iter_mut().enumerate() {
        *v = at_i32(body, 0x12c + i * 4);
    }
    let frame = at_i32(body, 0);
    let frame_to_break = at_i32(body, 4);
    let playing = at_i32(body, 8);
    let loading = at_i32(body, 12);
    let tick = at_i32(body, 16);
    let market_tick = at_i32(body, 20);
    let world_cities = at_i32(body, 0x180);
    let world_villages = at_i32(body, 0x184);
    let total_units = at_i32(body, 0x188);
    let everyone_mask = at_i32(body, 0x18c);
    let armageddon = at_i32(body, 0x190);

    let semaphore_bits = r.i32()?;
    let semaphore_size = r.i32()?;
    if !(0..=32).contains(&semaphore_size) || semaphore_bits != semaphore_size * 8 {
        return Err(r.err(format!(
            "invalid Game semaphore bits={semaphore_bits}, size={semaphore_size}"
        )));
    }
    let semaphore = r.take(semaphore_size as usize)?.to_vec();
    let graphic_tick = r.i32()?;
    let save_name = r.string()?;
    let bytes_walked = r.p;
    let rules = find_serialized_rules(payload, bytes_walked)?;

    Ok(InitialState {
        save_format: format,
        info: InitialGameInfo {
            version_string,
            version,
            seed,
            checksum_deep,
            checksum_window_size,
            checksum_failure_threshold,
            flags,
            settings,
            players,
            mods,
        },
        game: InitialGame {
            frame,
            frame_to_break,
            playing,
            loading,
            tick,
            market_tick,
            market,
            start_list,
            start_index,
            num_players: at_i32(body, 0x14c),
            world_cities,
            world_villages,
            total_units,
            everyone_mask,
            armageddon,
            semaphore_bits,
            semaphore,
            graphic_tick,
        },
        save_name,
        bytes_walked,
        payload_sha256: sha256(payload),
        worldgen_sources: WorldgenSourceSpans {
            seed: seed_source,
            map_style: ReplayByteSpan::new(settings_offset + 1, 1),
            map_size: ReplayByteSpan::new(settings_offset + 2, 1),
            players: ReplayByteSpan::new(settings_offset + 3, 1),
            game_rules: ReplayByteSpan::new(settings_offset + 6, 1),
            starting_town: ReplayByteSpan::new(settings_offset + 8, 1),
            scenario_type: ReplayByteSpan::new(settings_offset + 27, 1),
            player_flags,
            player_bodies,
        },
        rules,
    })
}

/// Parse the retail `Game`/`GameInfo` prefix and infer its v15/v16 tail.
pub fn parse_initial_state(payload: &[u8]) -> Result<InitialState, ParseError> {
    let mut last = None;
    for format in [16, 15] {
        match parse_candidate(payload, format) {
            Ok(s) => return Ok(s),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or(ParseError {
        offset: 0,
        message: "no supported GameInfo format".into(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wstr(out: &mut Vec<u8>, s: &str) {
        let w: Vec<u16> = s.encode_utf16().collect();
        out.extend_from_slice(&(w.len() as u32).to_le_bytes());
        for c in w {
            out.extend_from_slice(&c.to_le_bytes());
        }
    }

    fn fixture(format: u32) -> Vec<u8> {
        let mut b = vec![TAG_GAME, TAG_GAME_INFO];
        wstr(&mut b, "(Version: test)");
        b.extend_from_slice(&7u32.to_le_bytes());
        b.extend_from_slice(&0x1234_5678u32.to_le_bytes());
        b.extend_from_slice(&(-2i32).to_le_bytes());
        b.extend_from_slice(&3i32.to_le_bytes());
        b.extend_from_slice(&4i32.to_le_bytes());
        b.extend_from_slice(&0x60u32.to_le_bytes());
        let mut settings = [0u8; 30];
        settings[1] = 12;
        settings[2] = 3;
        settings[3] = 2;
        b.extend_from_slice(&settings);
        for slot in 0..8u8 {
            b.push(TAG_PLAYER);
            let present = slot < 2;
            let flags = if present { 1u16 } else { 0 };
            b.extend_from_slice(&flags.to_le_bytes());
            if present {
                let mut body = [0u8; 0x39];
                body[0x30..0x32].copy_from_slice(&flags.to_le_bytes());
                body[0x32] = 20 + slot;
                body[0x33] = slot;
                body[0x34] = slot + 1;
                body[0x36] = slot;
                b.extend_from_slice(&body);
                wstr(&mut b, if slot == 0 { "one" } else { "two" });
            }
        }
        if format >= 16 {
            b.extend_from_slice(&0u32.to_le_bytes());
            b.extend_from_slice(&0u32.to_le_bytes());
            wstr(&mut b, "");
            wstr(&mut b, "");
            wstr(&mut b, "");
            b.extend_from_slice(&0u32.to_le_bytes());
            b.extend_from_slice(&0u32.to_le_bytes());
            wstr(&mut b, "");
        } else {
            for _ in 0..4 {
                wstr(&mut b, "");
            }
        }
        let mut game = [0u8; 404];
        game[4..8].copy_from_slice(&(-1i32).to_le_bytes());
        b.extend_from_slice(&game);
        b.extend_from_slice(&16i32.to_le_bytes());
        b.extend_from_slice(&2i32.to_le_bytes());
        b.extend_from_slice(&[0xaa, 0x55]);
        b.extend_from_slice(&9i32.to_le_bytes());
        wstr(&mut b, "fixture");
        b
    }

    #[test]
    fn parses_both_retail_tail_formats_and_player_setup() {
        for format in [15, 16] {
            let b = fixture(format);
            let s = parse_initial_state(&b).unwrap();
            assert_eq!(s.save_format, format);
            assert_eq!(s.bytes_walked, b.len());
            assert_eq!(s.info.seed, 0x1234_5678);
            assert_eq!(s.info.settings.map_size, 3);
            assert_eq!(s.info.settings.map_edge_world_cells(), Some(70));
            let p: Vec<_> = s.active_players().collect();
            assert_eq!(p.len(), 2);
            assert_eq!((p[1].who, p[1].team, p[1].name.as_str()), (1, 2, "two"));
            assert_eq!(s.game.semaphore, [0xaa, 0x55]);
        }
    }

    #[test]
    fn reconstruction_is_nonempty_and_marks_generated_bytes_unsourced() {
        let s = parse_initial_state(&fixture(16)).unwrap();
        let w = s.reconstruct_world().unwrap();
        assert_eq!((w.world.xs, w.world.ys), (70, 70));
        assert_eq!(w.world.seed, 0x1234_5678);
        assert_eq!(w.sourced_walked_bytes, 52 + 45 * 70 * 70);
        assert_eq!(w.exact_sourced_walked_bytes(), 52 + 45 * 70 * 70);
        assert!(w.ownership.is_some());
        assert!(w.ownership_is_coherent());
        assert!(w.checksum.bytes > w.sourced_walked_bytes);
        assert!(w.unsourced_walked_bytes() > 100_000);
        assert_ne!(w.checksum.full, 1);
    }

    #[test]
    fn item_prefix_retains_exact_source_bytes_and_is_mutation_sensitive() {
        let b = fixture(16);
        let s = parse_initial_state(&b).unwrap();
        let inputs = s.worldgen_inputs();
        let spans = inputs.sources;

        assert_eq!(
            &b[spans.seed.offset..spans.seed.end()],
            &0x1234_5678u32.to_le_bytes()
        );
        assert_eq!(b[spans.map_style.offset], 12);
        assert_eq!(b[spans.map_size.offset], 3);
        assert_eq!(b[spans.players.offset], 2);
        assert_eq!(spans.player_flags[0].bytes, 2);
        assert_eq!(spans.player_bodies[0].unwrap().bytes, 0x39);
        assert_eq!(spans.player_bodies[7], None);
        assert_eq!(inputs.active_slots, [0, 1]);
        assert_eq!(inputs.active_who, [0, 1]);
        assert_eq!(inputs.active_teams, [1, 2]);

        let mut mutant = b;
        mutant[spans.map_style.offset] ^= 1;
        let changed = parse_initial_state(&mutant).unwrap().worldgen_inputs();
        assert_eq!(changed.map_style, inputs.map_style ^ 1);
        assert_eq!(changed.sources.map_style, spans.map_style);
        assert_eq!(changed.seed, inputs.seed);
    }

    #[test]
    fn admitted_item_prefix_fails_closed_before_installing_an_empty_channel() {
        let mut s = parse_initial_state(&fixture(16)).unwrap();
        // Unit fixtures omit the megabyte shipped Rules body. Supplying the
        // already independently admitted descriptor lets this test exercise
        // the next, map-style-content boundary without weakening that parser.
        s.rules = Some(InitialRules {
            serialized_offset: s.bytes_walked,
            serialized_bytes: SHIPPED_RULES_SERIALIZED_BYTES,
            serialized_sha256: [1; 32],
            walked_bytes: RETAIL_WALKED_BYTES,
            checksum: SHIPPED_RULES_CHANNEL,
            after_types: RETAIL_AFTER_TYPES,
            after_constants: RETAIL_AFTER_CONSTANTS,
            after_balance: RETAIL_AFTER_BALANCE,
        });
        let plan = s.reconstruct_items();
        assert_eq!(
            plan.boundary,
            InitialItemBoundary::MapStyleContentUnavailable { map_style: 12 }
        );
        assert_eq!(plan.scalar_source_bytes(), 10);
        assert!(plan
            .absent_replay_inputs()
            .iter()
            .all(|input| input.replay_bytes == 0));

        let mut initial = s.reconstruct_world().unwrap();
        assert_eq!(initial.sourced_walked_bytes, 76 + 45 * 70 * 70);
        assert!(initial.ownership_is_coherent());
        let mut sim = don_sim::World::with_capacity(16, 1);
        assert_eq!(
            plan.apply(&mut sim, &mut initial.world),
            Err(InitialItemReconstructionError::Blocked(plan.boundary))
        );
        assert_eq!(
            sim.items_channel(),
            Err(don_sim::item_runtime::ItemRuntimeError::Unavailable)
        );

        initial.world.xs += 1;
        assert!(matches!(
            plan.apply(&mut sim, &mut initial.world),
            Err(InitialItemReconstructionError::MapPrefixMismatch { .. })
        ));
        assert_eq!(
            sim.items_channel(),
            Err(don_sim::item_runtime::ItemRuntimeError::Unavailable)
        );
    }

    #[test]
    fn custom_scenario_does_not_invent_dimensions_from_the_ui_map_size() {
        let mut b = fixture(16);
        // GameInfo settings begin after two tags, the String, and 24 scalar
        // bytes. Locate the known 30-byte fixture block by its map style/size.
        let at = b.windows(3).position(|w| w == [0, 12, 3]).unwrap();
        b[at + 27] = 5;
        let s = parse_initial_state(&b).unwrap();
        assert_eq!(s.info.settings.scenario_type, 5);
        assert!(s.reconstruct_world().is_none());
    }
}
