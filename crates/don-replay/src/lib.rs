//! Replay-driven validation for the Descent of Nations simulation.
//!
//! # What this crate does
//!
//! A multiplayer `.rcx` is a lockstep command stream plus, on **every turn, for
//! every player**, a 65-byte `CheckSumsCommand` carrying sixteen 32-bit words:
//! fifteen `DataWalk` adler-32 channels over the whole simulation state, and
//! their wrapping sum. That is a per-turn, per-subsystem ground-truth oracle
//! that costs a parser rather than an emulator.
//!
//! This crate feeds that stream to a simulation, computes the same sixteen
//! channels over *our* state with the traversal generated from
//! `schema/state-schema.json`, and reports the **first diverging turn and which
//! channel diverged**.
//!
//! ```text
//!   .rcx ──gzip──▶ payload ──find_stream──▶ CommandPackage records
//!                                              │
//!                    ┌─────────────────────────┴──────────────────────┐
//!                    ▼                                                ▼
//!            commands ──▶ Order ──▶ Simulation::apply            CheckSumsCommand
//!                                        │                        (16 × u32)
//!                                        ▼                             │
//!                                 Simulation::check_all ──── compare ──┘
//!                                                              │
//!                                                    per-channel survival
//! ```
//!
//! # What the number means
//!
//! `survived` is the count of consecutive turns, from the recording's first
//! checksummed turn, on which our channel value equalled retail's. It is
//! reported per channel, with the subset that agreed **only because both sides
//! walked zero bytes** broken out as `trivial`. Early divergence is the
//! expected result today and is not hidden; the number is the project's
//! progress metric, so flattering it would defeat the point.
//!
//! # Fidelity
//!
//! Tier C. The checksum *primitive* is Tier B (500,000 differential calls into
//! the retail `adler32` at `0x00a46830`, 0 mismatches). The *traversal* is
//! generated from a static extraction with documented gaps. Nothing here is
//! verified in the proof-assistant sense.

#![forbid(unsafe_code)]

pub mod armies_runtime;
pub mod build_init_prefix;
pub mod build_spawn_runtime;
pub mod builds_runtime;
pub mod check_all;
pub mod check_player_forest;
pub mod checksum;
pub mod cities_runtime;
pub mod city_build_constructor_runtime;
pub mod continent;
pub mod east_meets_west_place_start;
pub mod east_meets_west_start_boundary;
pub mod edge_canals;
pub mod fractal_boundary;
pub mod groups_build_history;
pub mod groups_build_runtime;
pub mod groups_channel;
pub mod groups_dynamic;
pub mod groups_first_farm_authority;
pub mod groups_pre_pair_unit_authority;
pub mod groups_sim_channel;
pub mod growth;
pub mod guys_runtime;
pub mod harness;
pub mod image;
pub mod initial;
pub mod leader_initial_prefix;
pub mod leader_prefix_ledger;
pub mod leaders_deferred_history_frontier;
pub mod leaders_dynamic_children_frontier;
pub mod leaders_generated_fixed_frontier;
pub mod leaders_runtime_frontier;
pub mod leaders_runtime_tribe_frontier;
pub mod leaders_setup_reg_buildings_frontier;
pub mod leaders_sim_owner_frontier;
pub mod leaders_sim_tech_frontier;
pub mod map_make_resource_caller_gap_frontier;
pub mod map_make_resource_schedule_integration;
pub mod map_style;
pub mod next_checksum;
pub mod nubify_forest_frontier;
pub mod place_all_advance;
pub mod place_all_boundary;
pub mod place_all_facts;
pub mod place_player_resource_body_frontier;
pub mod place_region_resource_world_body_frontier;
pub mod place_resources_bonus_mutation_frontier;
pub mod place_resources_bonus_rows_mutation_frontier;
pub mod place_resources_canonical_transaction;
pub mod place_resources_category_frontier;
pub mod place_resources_pool_frontier;
pub mod place_resources_xml_frontier;
pub mod player_land;
pub mod pools;
pub mod post_continent;
pub mod post_nubify_transition_frontier;
pub mod region_centroid;
pub mod replay;
pub mod replay_bhs_live_bindings;
pub mod replay_bhs_research_runtime;
pub mod replay_bhs_runtime;
pub mod replay_goods_initial;
pub mod replay_goods_resource_schedule;
pub mod replay_world_owner_transitions;
pub mod report;
pub mod resource_divvy_pool_selection_frontier;
pub mod rules_channel;
pub mod scenario_channel;
pub mod script_channel;
pub mod setup_cities_builds;
pub mod setup_group_move_authority;
pub mod setup_place_unit_deep_re;
pub mod setup_unit_member_authority;
pub mod setup_unit_visibility_deep_re;
pub mod setup_units_producer;
pub mod starting_city_unit_census;
pub mod starting_village_suffix;
pub mod starting_village_world_schedule;
pub mod state;
pub mod terrain_height_runtime;
pub mod unit_init_collision_tail_deep_re;
pub mod unit_init_location_deep_re;
pub mod units_runtime;
pub mod walk;
pub mod wire;
pub mod world_owner_frontier;
pub mod world_tdata_frontier;

pub mod walk_gen;
mod wire_gen;

pub use check_all::{check_all, CheckAll, CheckSumsRecord};
pub use check_player_forest::{
    execute_check_player_forest, CheckPlayerForestError, CheckPlayerForestFacts,
    CheckPlayerForestReceipt, ForestPatch, PlayerForestResult, MAP_CHECK_PLAYER_FOREST_VA,
    TERRAIN_GROUPS_NUBIFY_FOREST_VA,
};
pub use checksum::{adler32, Channel, Channels, CheckSum, DataWalk, CHANNEL_NAMES};
pub use continent::{
    execute_continent_prefix, execute_continent_prefix_from_rng,
    execute_continent_prefix_with_regions, execute_continent_prefix_with_regions_from_rng,
    ContinentError, ContinentReceipt, ContinentStop, LakeCandidateReceipt, RegionSeedCall,
    RegionSeedReceipt, MAP_PLAYER_LAND_RADIUS,
};
pub use fractal_boundary::{
    resolve_fertility_boundary, resolve_tile_selection, FertilityBoundary, FractalBoundaryError,
    FractalBoundarySources, FractalReplayInputs, RetailFractalPlane, TileSelectionBoundary,
    TileSelectionPass, TileSelectionSource, TERRAIN_GROUPS_PLACE_ALL_VA,
};
pub use groups_channel::{
    groups_checksum, GroupMembers, GroupRecord, GroupWindow, GroupsChannelError, GroupsChecksum,
    InitialGroupsChannel, RetailInitialGroups, CHECK_GROUPS_VA, CORPUS_INITIAL_GROUPS_CHANNEL,
    GROUPS_CLEAR_VA, GROUP_CLEAR_VA, GROUP_SLOTS, GROUP_WALK_DATA_VA,
};
pub use growth::{
    execute_grow_region, execute_grow_valid, GrowRegionCall, GrowRegionError, GrowRegionReceipt,
    GrowValidCall, GrowValidReceipt, MapGrowthConfig,
};
pub use harness::{format_table, run, NullSim, Phase, RunResult, Simulation};
pub use initial::{
    InitialGame, InitialGameInfo, InitialItemBoundary, InitialItemReconstruction,
    InitialItemReconstructionError, InitialPlayer, InitialState, InitialWorld,
    InitialWorldgenInputs, MapTerrainRepairError, MapTerrainRepairReceipt, ReplayByteSpan,
    WorldgenSourceSpans,
};
pub use map_make_resource_schedule_integration::{
    execute_map_make_resource_schedule, execute_map_make_resource_schedule_with_xml,
    MapMakeResourceOwnerProvenance, MapMakeResourcePlacementReceipt, MapMakeResourceScheduleError,
    MapMakeResourceScheduleReceipt, PlaceResourcesBodyBoundary, PlaceResourcesEntryBoundary,
    PlaceResourcesSkippedBoundary, PlaceResourcesXmlBoundary,
};
pub use map_style::{
    MapGenerationCheckpoint, MapGenerationStage, MapStyleIdentity, MapStyleLoadError,
    MapStyleStaticData, StaticFileEvidence, StaticXmlEntry, MAP_MAKE_SCHEDULE,
    SHIPPED_MAP_STYLE_CATALOG,
};
pub use nubify_forest_frontier::{
    execute_nubify_forest_frontier, EdgeOfRegionHost, EdgeOfRegionProvenance, EdgeOfRegionReceipt,
    EdgeOfRegionRequest, NubifyForestError, NubifyForestReceipt,
};
pub use place_all_advance::{
    advance_place_all_boundary, OilGoodPolicy, PlaceAllAdvanceError, PlaceAllAdvanceFacts,
    PlaceAllAdvanceReceipt, PlaceAllStop, SelectedGroupRow, CLIFFS_POSITION_CLIFF_VA,
    CLIFFS_VERIFY_DEFENSIVE_POSITION_VA, MOUNTAINS_ADD_MOUNTAIN_VA, MOUNTAINS_RANDOMIZE_CALL_VA,
    MOUNTAINS_RANDOMIZE_MOUNTAINS_VA, TERRAIN_GROUPS_ADD_DOOBERS_VA,
    TERRAIN_GROUPS_TREEIFY_MOUNTAINS_VA, TERRAIN_GROUP_PLACE_PLAYER_GROUP_VA,
    TERRAIN_GROUP_PLACE_REGION_GROUP_VA, WORLD_SET_OIL_AT_VA,
};
pub use place_all_boundary::{
    execute_replay_place_all, ReplayPlaceAllError, ReplayPlaceAllFacts, ReplayPlaceAllHostFacts,
    ReplayPlaceAllPlayerFacts, ReplayPlaceAllReceipt, ReplayPlaceAllRuntime,
    ReplayPlaceAllTDataFacts, TERRAIN_GROUPS_PLACE_ALL_RETURN_VA,
};
pub use place_all_facts::{
    resolve_place_all_prerequisites, resolve_terrain_group_catalog, CapturedHelpingFacts,
    PlaceAllFactKind, PlaceAllLiveCaptureEvidence, PlaceAllLiveFacts, PlaceAllPrerequisiteError,
    PlaceAllPrerequisiteResolution, PreparedReplayPlaceAll, TerrainGroupCatalogError,
    TerrainGroupCatalogReceipt, UnavailablePlaceAllFact,
};
pub use player_land::{
    execute_check_player_land, CheckPlayerLandCall, CheckPlayerLandError, CheckPlayerLandReceipt,
    MAP_CHECK_PLAYER_LAND_VA,
};
pub use pools::{
    execute_eliminate_pools, ElimPoolParam, EliminatePoolsError, EliminatePoolsReceipt,
    PoolMergeReceipt,
};
pub use post_continent::{
    execute_map_fix_diag_land, execute_map_make_post_checksum_string_close,
    execute_map_make_post_fix_diag_game_log_say_checksum,
    execute_map_make_post_fix_diag_string_constructor, execute_map_make_progress_string,
    execute_post_continent, GameLogCheckAcceptNativeBody, GameLogSayChecksumNativeBody,
    MapFixDiagLandError, MapFixDiagLandNativeBody, MapFixDiagLandNext, MapFixDiagLandReceipt,
    MapFixDiagLandWorldMutation, MapMakeLocalizedStringBufferOwner, MapMakeLocalizedStringTableRow,
    MapMakePostChecksumCallerBody, MapMakePostChecksumClosedString,
    MapMakePostChecksumStringCloseAllocationReceipt, MapMakePostChecksumStringCloseCallerBody,
    MapMakePostChecksumStringCloseError, MapMakePostChecksumStringCloseNext,
    MapMakePostChecksumStringCloseReceipt, MapMakePostFixDiagGameLogCall,
    MapMakePostFixDiagGameLogError, MapMakePostFixDiagGameLogNext,
    MapMakePostFixDiagGameLogOwnerReceipt, MapMakePostFixDiagGameLogReceipt,
    MapMakePostFixDiagLocalString, MapMakePostFixDiagStringAllocationOwner,
    MapMakePostFixDiagStringAllocationReceipt, MapMakePostFixDiagStringConstructorError,
    MapMakePostFixDiagStringConstructorNext, MapMakePostFixDiagStringConstructorReceipt,
    MapMakeProgressAliasState, MapMakeProgressClosedConstString, MapMakeProgressStringError,
    MapMakeProgressStringNext, MapMakeProgressStringOwnershipReceipt, MapMakeProgressStringReceipt,
    MapMakeStringAllocationState, MapMakeStringAllocatorFacts, PostContinentError,
    PostContinentReceipt, StringCloseChildNativeBody, StringCloseNativeBody,
    StringConstructorCallerBody, StringConstructorHelperNativeBody, StringConstructorNativeBody,
    StringCopyAssignmentNativeBody, StringCopyConstructorNativeBody, StringInitConstNativeBody,
    StringReinitNativeBody, TerritoryLimits, GAME_LOG_CHECK_ACCEPT_NATIVE_BODY,
    GAME_LOG_SAY_CHECKSUM_NATIVE_BODY, MAP_FIX_DIAG_LAND_NATIVE_BODY, MAP_FIX_DIAG_LAND_VA,
    MAP_MAKE_COASTLINES_CALL_BODY, MAP_MAKE_COASTLINES_VA, MAP_MAKE_POST_CHECKSUM_CALLER_BODY,
    MAP_MAKE_POST_FIX_DIAG_CONSTRUCTOR_CALLER_BODY, STRING_CHAR_TO_WCHAR_NATIVE_BODY,
    STRING_CONSTRUCTOR_NATIVE_BODY, STRING_COPY_ASSIGNMENT_NATIVE_BODY,
    STRING_COPY_CONSTRUCTOR_NATIVE_BODY, STRING_GET_STRING_GUTS_NATIVE_BODY,
    STRING_GUTS_MEM_GET_NATIVE_BODY, STRING_GUTS_OPERATOR_NEW_NATIVE_BODY,
    STRING_INIT_CONST_NATIVE_BODY, STRING_REINIT_NATIVE_BODY, TERRAIN_GROUPS_FILL_FERTILE_VA,
};
pub use post_nubify_transition_frontier::{
    execute_post_nubify_transitions, PostNubifyTransitionError, PostNubifyTransitionReceipt,
    TerrainTransitionEvidence, TerrainTransitionLiveFacts, TerrainTransitionTuning,
};
pub use replay::{corpus, Replay};
pub use state::SimState;
pub use walk::{walk_class, WalkOp, WalkOutcome, WalkSpec};
pub use walk_gen::{class_index, SPECS};
pub use wire::{classify, CommandClass, CommandView, Order};
