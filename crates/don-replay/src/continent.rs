//! Executable replay reconstruction of the admitted map-style continent prefix.
//!
//! The retail driver seeds the main `Random`, selects the tileset while loading
//! map data, resolves `BASE_EDGE`, then calls one virtual
//! `Map*::make_continents` implementation. This module executes
//! every instruction whose inputs and effects are represented by the replay,
//! the admitted map-style XML, and `don_sim::systems::map_terrain::World`.
//! It stops at the first unported geometry/region primitive and returns that
//! call as data; it never skips the primitive and consumes later RNG draws.

#[path = "east_indies_tail.rs"]
mod east_indies_tail;
#[path = "east_meets_west_add_start.rs"]
mod east_meets_west_add_start;
#[path = "east_meets_west_player_land.rs"]
mod east_meets_west_player_land;
#[path = "east_meets_west_remaining_starts.rs"]
mod east_meets_west_remaining_starts;
#[path = "team_continent_partition.rs"]
mod team_continent_partition;

pub use east_indies_tail::{
    execute_east_indies_tail, EastIndiesAreaAdjustment, EastIndiesAreaAdjustmentKind,
    EastIndiesDirectDraw, EastIndiesDirectDrawKind, EastIndiesIslandReceipt,
    EastIndiesRegionDefaults, EastIndiesRngSpan, EastIndiesRngSpanKind, EastIndiesTailCall,
    EastIndiesTailError, EastIndiesTailReceipt, EastIndiesTailReturn,
    EAST_INDIES_ISLAND_AREA_RNG_VA, EAST_INDIES_ISLAND_X_RNG_VA, EAST_INDIES_ISLAND_Y_RNG_VA,
    EAST_INDIES_NONPLAYER_ISLANDS_VA, EAST_INDIES_RETURN_VA,
};

pub use east_meets_west_add_start::{
    EastMeetsWestAddStartError, EastMeetsWestAddStartInput, EastMeetsWestAddStartReceipt,
    StartArrayState, StartArraysState, StartCityOccupancyWrite,
};

pub use east_meets_west_player_land::{
    execute_east_meets_west_centroid_x_free_cleanup,
    execute_east_meets_west_centroid_y_free_cleanup, execute_east_meets_west_player_land,
    execute_east_meets_west_post_player_land_cleanup, CheckPlayerLandNativeBody, CleanupNativeBody,
    EastMeetsWestCentroidListAllocation, EastMeetsWestCentroidListOwner,
    EastMeetsWestCentroidXFreeCleanupNext, EastMeetsWestCentroidXFreeCleanupReceipt,
    EastMeetsWestCentroidYFreeCleanupNext, EastMeetsWestCentroidYFreeCleanupReceipt,
    EastMeetsWestImportedFreeReceipt, EastMeetsWestLogStringLease, EastMeetsWestPlayerLandCall,
    EastMeetsWestPlayerLandError, EastMeetsWestPlayerLandNext, EastMeetsWestPlayerLandReceipt,
    EastMeetsWestPostPlayerLandCleanupNext, EastMeetsWestPostPlayerLandCleanupReceipt,
    EastMeetsWestStringClosePath, EastMeetsWestStringCloseReceipt, RegionMutation,
    RetailAllocationState, WorldCellMutation, CHECK_PLAYER_LAND_NATIVE_BODY,
    EAST_MEETS_WEST_CENTROID_X_FLAGS_CLEAR_VA, EAST_MEETS_WEST_CENTROID_X_FREE_CALL_VA,
    EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_BODY, EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_END_VA,
    EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_INSTRUCTION_COUNT,
    EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_SHA256, EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_SIZE,
    EAST_MEETS_WEST_CENTROID_X_LENGTH_CLEAR_VA, EAST_MEETS_WEST_CENTROID_X_LIST_CLEAR_VA,
    EAST_MEETS_WEST_CENTROID_X_LIST_LOAD_VA, EAST_MEETS_WEST_CENTROID_X_LIST_PUSH_VA,
    EAST_MEETS_WEST_CENTROID_X_LIST_TEST_VA, EAST_MEETS_WEST_CENTROID_X_SIZE_CLEAR_VA,
    EAST_MEETS_WEST_CENTROID_X_STACK_POP_VA, EAST_MEETS_WEST_CENTROID_X_VFTABLE_STORE_VA,
    EAST_MEETS_WEST_CENTROID_Y_FLAGS_CLEAR_VA, EAST_MEETS_WEST_CENTROID_Y_FREE_CALL_VA,
    EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_BODY, EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_END_VA,
    EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_INSTRUCTION_COUNT,
    EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_SHA256, EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_SIZE,
    EAST_MEETS_WEST_CENTROID_Y_GUARD_CLEAR_VA, EAST_MEETS_WEST_CENTROID_Y_LENGTH_CLEAR_VA,
    EAST_MEETS_WEST_CENTROID_Y_LIST_CLEAR_VA, EAST_MEETS_WEST_CENTROID_Y_LIST_LOAD_VA,
    EAST_MEETS_WEST_CENTROID_Y_LIST_PUSH_VA, EAST_MEETS_WEST_CENTROID_Y_LIST_TEST_VA,
    EAST_MEETS_WEST_CENTROID_Y_SIZE_CLEAR_VA, EAST_MEETS_WEST_CENTROID_Y_VFTABLE_STORE_VA,
    EAST_MEETS_WEST_EBX_POP_VA, EAST_MEETS_WEST_EDI_POP_VA, EAST_MEETS_WEST_ESI_POP_VA,
    EAST_MEETS_WEST_EXCEPTION_REGISTRATION_LOAD_VA,
    EAST_MEETS_WEST_EXCEPTION_REGISTRATION_RESTORE_VA, EAST_MEETS_WEST_FRAME_POINTER_POP_VA,
    EAST_MEETS_WEST_FREE_IMPORT_LOAD_VA, EAST_MEETS_WEST_LOG_STRING,
    EAST_MEETS_WEST_LOG_STRING_ASSIGN_CALL_VA, EAST_MEETS_WEST_LOG_STRING_BYTE_OFFSET,
    EAST_MEETS_WEST_LOG_STRING_HASH, EAST_MEETS_WEST_LOG_STRING_ORDINAL,
    EAST_MEETS_WEST_LOG_STRING_UTF16_UNITS, EAST_MEETS_WEST_MAKE_CONTINENTS_ENTRY_VA,
    EAST_MEETS_WEST_MAKE_CONTINENTS_RET_VA, EAST_MEETS_WEST_MAKE_CONTINENTS_SIZE,
    EAST_MEETS_WEST_PLAYER_LAND_CALLER_ENTRY_VA, EAST_MEETS_WEST_PLAYER_LAND_CALL_VA,
    EAST_MEETS_WEST_PLAYER_LAND_GUARD_CLEAR_VA, EAST_MEETS_WEST_PLAYER_LAND_GUARD_STORE_VA,
    EAST_MEETS_WEST_PLAYER_LAND_RESUME_VA, EAST_MEETS_WEST_PLAYER_LAND_STRING_CLOSE_CALL_VA,
    EAST_MEETS_WEST_POST_PLAYER_LAND_CLEANUP_BODY, EAST_MEETS_WEST_POST_PLAYER_LAND_CLEANUP_END_VA,
    EAST_MEETS_WEST_POST_PLAYER_LAND_CLEANUP_INSTRUCTION_COUNT,
    EAST_MEETS_WEST_POST_PLAYER_LAND_CLEANUP_SHA256, EAST_MEETS_WEST_POST_PLAYER_LAND_CLEANUP_SIZE,
    EAST_MEETS_WEST_STACK_FRAME_RESTORE_VA, FREE_IMPORT_IAT_VA, INT_STR_ARRAY_VA,
    MAP_CHECK_PLAYER_LAND_END_VA, MAP_CHECK_PLAYER_LAND_INSTRUCTION_COUNT,
    MAP_CHECK_PLAYER_LAND_RET_VA, MAP_CHECK_PLAYER_LAND_SHA256, MAP_CHECK_PLAYER_LAND_SIZE,
    RISE_EXE_SHA256, SIMPLE_ARRAY_INT_SIZE, SIMPLE_ARRAY_INT_VFTABLE_VA, STRING_ASSIGN_VA,
    STRING_CLOSE_END_VA, STRING_CLOSE_INSTRUCTION_COUNT, STRING_CLOSE_NATIVE_BODY,
    STRING_CLOSE_RET_VA, STRING_CLOSE_SHA256, STRING_CLOSE_SIZE, STRING_CLOSE_VA,
    STRING_GUTS_DESTRUCTOR_CALL_VA, STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_VA, STRING_SIZE,
};

pub use east_meets_west_remaining_starts::{
    execute_remaining_active_starts, EastMeetsWestRemainingStartIteration,
    EastMeetsWestRemainingStartsError, EastMeetsWestRemainingStartsNext,
    EastMeetsWestRemainingStartsReceipt, EAST_MEETS_WEST_CHECK_PLAYER_LAND_CALL_VA,
    EAST_MEETS_WEST_REMAINING_STARTS_ENTRY_VA,
};

pub use team_continent_partition::{
    execute_team_continent_partition, TeamContinentPartitionError, TeamContinentPartitionReceipt,
    TeamContinentRandomDraw, MAP_FILL_CONT_END_VA, MAP_FILL_CONT_EQUAL_SIZE_RNG_VA,
    MAP_FILL_CONT_RET_VA, MAP_FILL_CONT_VA,
};

use crate::east_meets_west_place_start::{
    execute_first_place_start_selector, PlaceStartSelectorError, PlaceStartSelectorNext,
    PlaceStartSelectorReceipt,
};
use crate::east_meets_west_start_boundary::{
    execute_first_place_start_boundary, EastMeetsWestPlaceStartBoundary,
    EastMeetsWestStartBoundaryError, EAST_MEETS_WEST_FIRST_PLACE_START_CALL_VA,
    MAP_PLACE_START_IN_REGION_VA,
};
use crate::edge_canals::{
    execute_eliminate_edge_canals, EliminateEdgeCanalsError, EliminateEdgeCanalsReceipt,
};
use crate::growth::{
    execute_grow_region, execute_grow_valid, GrowRegionCall, GrowRegionError, GrowRegionReceipt,
    GrowValidCall, GrowValidReceipt, MapGrowthConfig,
};
use crate::initial::InitialWorldgenInputs;
use crate::map_style::{MapStyleStaticData, StaticXmlEntry, MAP_MAKE_ORIENTATION_RNG_VA};
pub use crate::player_land::MAP_CHECK_PLAYER_LAND_VA;
use crate::player_land::{
    execute_check_player_land, CheckPlayerLandCall, CheckPlayerLandError, CheckPlayerLandReceipt,
};
use crate::pools::{
    execute_eliminate_pools, ElimPoolParam, EliminatePoolsError, EliminatePoolsReceipt,
};
pub use crate::post_continent::{
    execute_map_fix_diag_land, execute_map_make_first_regions_clear_all,
    execute_map_make_first_regions_find_all, execute_map_make_post_checksum_string_close,
    execute_map_make_post_fix_diag_game_log_say_checksum,
    execute_map_make_post_fix_diag_string_constructor, execute_map_make_territory_limits,
    GameLogCheckAcceptNativeBody, GameLogSayChecksumNativeBody, MapFixDiagLandError,
    MapFixDiagLandNativeBody, MapFixDiagLandNext, MapFixDiagLandReceipt,
    MapFixDiagLandWorldMutation, MapMakeFirstRegionsClearAllNext,
    MapMakeFirstRegionsClearAllReceipt, MapMakeFirstRegionsFindAllCallerBody,
    MapMakeFirstRegionsFindAllError, MapMakeFirstRegionsFindAllNext,
    MapMakeFirstRegionsFindAllReceipt, MapMakeFirstTerritoryPrepBody,
    MapMakePostChecksumCallerBody, MapMakePostChecksumClosedString,
    MapMakePostChecksumStringCloseAllocationReceipt, MapMakePostChecksumStringCloseCallerBody,
    MapMakePostChecksumStringCloseError, MapMakePostChecksumStringCloseNext,
    MapMakePostChecksumStringCloseReceipt, MapMakePostFixDiagGameLogCall,
    MapMakePostFixDiagGameLogError, MapMakePostFixDiagGameLogNext,
    MapMakePostFixDiagGameLogOwnerReceipt, MapMakePostFixDiagGameLogReceipt,
    MapMakePostFixDiagLocalString, MapMakePostFixDiagStringAllocationOwner,
    MapMakePostFixDiagStringAllocationReceipt, MapMakePostFixDiagStringConstructorError,
    MapMakePostFixDiagStringConstructorNext, MapMakePostFixDiagStringConstructorReceipt,
    MapMakeStringAllocationState, MapMakeStringAllocatorFacts, MapMakeTerritoryLimitsError,
    MapMakeTerritoryLimitsNativeBody, MapMakeTerritoryLimitsNext, MapMakeTerritoryLimitsReceipt,
    MapMakeTerritoryStyleBranchReceipt, RegionsAllocationState, RegionsClearAllCoordFreeReceipt,
    RegionsClearAllNativeBody, RegionsClearAllRegionReceipt, RegionsClearAllWorldMutation,
    RegionsFindAllNativeBody, RegionsFindAllRegionReceipt, RegionsFindAllScratchAllocatorCall,
    RegionsFindAllScratchReceipt, RegionsFindAllWorldMutation, StringCloseChildNativeBody,
    StringCloseNativeBody, StringConstructorCallerBody, StringConstructorHelperNativeBody,
    StringConstructorNativeBody, StringInitConstNativeBody, StringReinitNativeBody,
    FREE_IMPORT_IAT_VA as REGIONS_CLEAR_ALL_FREE_IMPORT_IAT_VA, GAME_LOG_SAY_CHECKSUM_VA,
    MALLOC_IMPORT_IAT_VA as REGIONS_FIND_ALL_MALLOC_IMPORT_IAT_VA, MAP_FIX_DIAG_LAND_CORNER_X,
    MAP_FIX_DIAG_LAND_CORNER_X_VA, MAP_FIX_DIAG_LAND_CORNER_Y, MAP_FIX_DIAG_LAND_CORNER_Y_VA,
    MAP_FIX_DIAG_LAND_END_VA, MAP_FIX_DIAG_LAND_INSTRUCTION_COUNT, MAP_FIX_DIAG_LAND_NATIVE_BODY,
    MAP_FIX_DIAG_LAND_RET_VA, MAP_FIX_DIAG_LAND_SHA256, MAP_FIX_DIAG_LAND_SIZE,
    MAP_MAKE_FIRST_FIX_DIAG_LAND_CALL_VA, MAP_MAKE_FIRST_REGIONS_CLEAR_CALL_VA,
    MAP_MAKE_FIRST_REGIONS_CLEAR_RESUME_VA, MAP_MAKE_FIRST_REGIONS_FIND_ALL_CALLER_BODY,
    MAP_MAKE_FIRST_REGIONS_FIND_ARGUMENT_PUSH_VA, MAP_MAKE_FIRST_REGIONS_FIND_CALLER_END_VA,
    MAP_MAKE_FIRST_REGIONS_FIND_CALLER_INSTRUCTION_COUNT,
    MAP_MAKE_FIRST_REGIONS_FIND_CALLER_SHA256, MAP_MAKE_FIRST_REGIONS_FIND_CALLER_SIZE,
    MAP_MAKE_FIRST_REGIONS_FIND_CALL_VA, MAP_MAKE_FIRST_REGIONS_FIND_RESUME_VA,
    MAP_MAKE_FIRST_TERRITORY_MAP_LOAD_VA, MAP_MAKE_FIRST_TERRITORY_PREP_BODY,
    MAP_MAKE_FIRST_TERRITORY_PREP_END_VA, MAP_MAKE_FIRST_TERRITORY_PREP_INSTRUCTION_COUNT,
    MAP_MAKE_FIRST_TERRITORY_PREP_SHA256, MAP_MAKE_FIRST_TERRITORY_PREP_SIZE,
    MAP_MAKE_FIRST_TERRITORY_STORE_VA, MAP_MAKE_FIRST_TERRITORY_WORLD_LOAD_VA,
    MAP_MAKE_FIX_DIAG_LAND_RESUME_VA, MAP_MAKE_POST_FIX_DIAG_CONSTRUCTOR_CALLER_BODY,
    MAP_MAKE_POST_FIX_DIAG_GAME_LOG_CALL_VA, MAP_MAKE_POST_FIX_DIAG_STRING_CONSTRUCTOR_CALL_VA,
    MAP_MAKE_POST_FIX_DIAG_STRING_LITERAL_PUSH_VA, MAP_MAKE_POST_FIX_DIAG_STRING_LOCAL_LOAD_VA,
    MAP_MAKE_STYLE_BRANCH_TARGET_VA, MAP_MAKE_STYLE_BRANCH_VA, MAP_MAKE_STYLE_BRANCH_VALUE,
    MAP_MAKE_STYLE_COMPARE_VA, MAP_MAKE_TERRITORY_LIMITS_END_VA,
    MAP_MAKE_TERRITORY_LIMITS_INSTRUCTION_COUNT, MAP_MAKE_TERRITORY_LIMITS_NATIVE_BODY,
    MAP_MAKE_TERRITORY_LIMITS_SHA256, MAP_MAKE_TERRITORY_LIMITS_SIZE,
    MAP_MAKE_TERRITORY_LIMIT_LOAD_VAS, MAP_MAKE_TERRITORY_LIMIT_STORE_VAS,
    MAP_PLAYER_TERRITORY_LIMIT_OFFSET, MAP_TERRITORY_LIMIT_OFFSETS,
    REGIONS_CLEAR_ALL_COORD_FREE_CALL_VA, REGIONS_CLEAR_ALL_END_VA,
    REGIONS_CLEAR_ALL_INSTRUCTION_COUNT, REGIONS_CLEAR_ALL_NATIVE_BODY,
    REGIONS_CLEAR_ALL_RET_EMPTY_WORLD_VA, REGIONS_CLEAR_ALL_RET_NONEMPTY_WORLD_VA,
    REGIONS_CLEAR_ALL_RET_NULL_WORLD_DATA_VA, REGIONS_CLEAR_ALL_SHA256, REGIONS_CLEAR_ALL_SIZE,
    REGIONS_CLEAR_ALL_VA, REGIONS_FIND_ALL_END_VA, REGIONS_FIND_ALL_INSTRUCTION_COUNT,
    REGIONS_FIND_ALL_NATIVE_BODY, REGIONS_FIND_ALL_RET_VA, REGIONS_FIND_ALL_SHA256,
    REGIONS_FIND_ALL_SIZE, REGIONS_FIND_ALL_VA, STRING_CHAR_TO_WCHAR_NATIVE_BODY,
    STRING_CONSTRUCTOR_NATIVE_BODY, STRING_CONSTRUCTOR_VA, STRING_GET_STRING_GUTS_NATIVE_BODY,
    STRING_GUTS_MEM_GET_NATIVE_BODY, STRING_GUTS_OPERATOR_NEW_NATIVE_BODY,
    STRING_INIT_CONST_NATIVE_BODY, STRING_REINIT_NATIVE_BODY, TERRITORY_LIMIT_FIELDS,
    WORLD_PLAYER_TERRITORY_LIMIT_OFFSET, WORLD_TERRITORY_LIMIT_OFFSETS,
};
pub use crate::post_continent::{TerritoryLimitField, TerritoryLimitStoreReceipt, TerritoryLimits};
use crate::region_centroid::{
    execute_east_meets_west_centroids, EastMeetsWestCentroidReceipt, RegionCentroidError,
    MAP_ELIMINATE_EDGE_CANALS_VA,
};
use don_sim::rng::Random;
use don_sim::systems::combat::circle_table;
use don_sim::systems::map_terrain::{land, wflag, WCoord, World};
use don_sim::systems::regions::Regions;
use don_sim::trig::{cosx, sinx};
use east_meets_west_add_start::{execute_first_add_start, EAST_MEETS_WEST_ADD_START_RETURN_VA};

pub const MAP_LAND_DIST_VA: u32 = 0x0069_d970;
pub const MAP_MAKE_REGION_VA: u32 = 0x0069_d3f0;
pub const MAP_GROW_REGION_VA: u32 = 0x0069_c600;
pub use crate::region_centroid::MAP_FIND_REGION_CENTROID_VA;

/// `MapGreatLakes::make_continents` `0x00699e40` derives the
/// `Map::check_player_land` radius from a shipped unit type rather than from map
/// XML: `[[[0x00c061fc]+0x10]+0x574]+0x1fc`, rounded up by four
/// (`0x00699e92`–`0x00699eb1`). `MapMediterranean::make_continents` reads the
/// identical chain at `0x0069ae2a`; that lane recorded the type as
/// `unittypes.items[349]` (Battleship) with PDB `ObjectTypeData::max_range`
/// `+0x1fc` fixed at 24 TCoords by `NAVAL_ROSTER`/`unitrules.xml`, so
/// `ceil(24 / 4) == 6`. Nothing in this crate loads unit types yet, so the
/// derived value is carried as a named constant instead of a bare literal.
pub const MAP_PLAYER_LAND_RADIUS: i32 = 6;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionSeedCall {
    pub region: i32,
    pub x: i32,
    pub y: i32,
    pub area: i32,
}

/// One accepted `MapGreatLakes` lake seed, with the exact rejection budget the
/// candidate loop at `0x0069a150`–`0x0069a272` spent reaching it. Each rejected
/// candidate consumed two `Random::get(0, 0xffff)` draws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LakeCandidateReceipt {
    pub region: i32,
    pub x: i32,
    pub y: i32,
    pub area: i32,
    pub required_distance: i32,
    pub land_distance: i32,
    pub rejected_candidates: i32,
    /// `Map::grow_valid`'s `int` return for this candidate.
    pub grow_valid_return: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionSeedReceipt {
    pub call: RegionSeedCall,
    pub coord_capacity_before: i32,
    pub coord_capacity_after: i32,
    pub common_factor: i32,
    pub goody_factor: i32,
    pub flags: i32,
}

/// First call not executed by a style prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContinentStop {
    /// The style virtual returned; the common driver next rebuilds regions.
    HookComplete { next_va: u32 },
    /// `Map::make_region` is the first unavailable map mutator.
    MakeRegion {
        primitive_va: u32,
        call: RegionSeedCall,
    },
    /// The exact seed-cell helper completed; region flood/growth is next.
    GrowRegion {
        primitive_va: u32,
        call: GrowRegionCall,
    },
    /// Compatibility boundary emitted by the earlier East Indies prefix after
    /// its player-region growth passes and before the now-admitted island tail.
    EastIndiesNonplayerIslands { next_rng_va: u32 },
    /// Retail abandons this generation pass and restarts the style virtual.
    RetryGeneration {
        failed_region: i32,
        retail_return: i32,
    },
    /// Compatibility boundary emitted by older reconstructions before the
    /// exact `Map::fill_cont` runtime was admitted.
    FillCont {
        primitive_va: u32,
        active_teams: Vec<u8>,
    },
    /// Compatibility boundary emitted by older reconstructions before the
    /// exact centroid loop was admitted.
    FindRegionCentroid { primitive_va: u32, region: i32 },
    /// Compatibility boundary emitted before the exact edge-canal body was
    /// admitted.
    EliminateEdgeCanals {
        primitive_va: u32,
        centroids: EastMeetsWestCentroidReceipt,
    },
    /// East Meets West completed its centroid loop, pool cleanup, all four
    /// edge-canal passes and the mandatory region rebuild. Caller execution is
    /// now parked at its first concrete start-selection call.
    PlaceStartInRegion {
        primitive_va: u32,
        first_call_va: u32,
        centroids: EastMeetsWestCentroidReceipt,
        edge_canals: EliminateEdgeCanalsReceipt,
        call: EastMeetsWestPlaceStartBoundary,
    },
    /// Every active selector and World append completed, followed by the exact
    /// post-loop `Map::check_player_land`, local-string cleanup, both centroid
    /// array `_free` calls, the complete style-virtual epilogue, and the common
    /// driver's first `Regions::clear_all` / `Regions::find_all` pair, all six
    /// World territory-limit stores, complete `Map::fix_diag_land`, and the
    /// caller-local log `String` constructor, the typed
    /// `GameLog::say_checksum` host observation, and the sole-owner
    /// `String::close` allocator transaction. Execution is frozen at the
    /// progress-message wide-string constructor selected by the replay's
    /// exact `progress = 1` call chain.
    AddStartingLocation {
        primitive_va: u32,
        caller_va: u32,
        next_va: u32,
        centroids: EastMeetsWestCentroidReceipt,
        edge_canals: EliminateEdgeCanalsReceipt,
        call: EastMeetsWestPlaceStartBoundary,
        selector: PlaceStartSelectorReceipt,
        mutation: EastMeetsWestAddStartReceipt,
        remaining: EastMeetsWestRemainingStartsReceipt,
        player_land: EastMeetsWestPlayerLandReceipt,
        post_player_land_cleanup: EastMeetsWestPostPlayerLandCleanupReceipt,
        centroid_y_free_cleanup: EastMeetsWestCentroidYFreeCleanupReceipt,
        centroid_x_free_cleanup: EastMeetsWestCentroidXFreeCleanupReceipt,
        regions_clear_all: MapMakeFirstRegionsClearAllReceipt,
        regions_find_all: MapMakeFirstRegionsFindAllReceipt,
        territory_limits: MapMakeTerritoryLimitsReceipt,
        fix_diag_land: MapFixDiagLandReceipt,
        post_fix_diag_string_constructor: MapMakePostFixDiagStringConstructorReceipt,
        game_log_say_checksum: MapMakePostFixDiagGameLogReceipt,
        post_checksum_string_close: MapMakePostChecksumStringCloseReceipt,
        next_mutator_va: u32,
    },
    /// One selector exhausted both passes and returned zero. When it was a
    /// later active player, the successfully appended first start and the
    /// exact intervening loop receipt remain attached.
    EastMeetsWestStartFallback {
        next_va: u32,
        centroids: EastMeetsWestCentroidReceipt,
        edge_canals: EliminateEdgeCanalsReceipt,
        call: EastMeetsWestPlaceStartBoundary,
        selector: PlaceStartSelectorReceipt,
        first_mutation: Option<EastMeetsWestAddStartReceipt>,
        remaining: Option<EastMeetsWestRemainingStartsReceipt>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContinentReceipt {
    pub map_style: u8,
    pub make_continents_va: u32,
    pub orientation: i32,
    pub rng_initial: i32,
    pub rng_final: i32,
    /// Call sites actually executed, in order. Called helpers are deliberately
    /// absent until the helper itself is executed.
    pub direct_rng_sites: Vec<u32>,
    /// One-based pass through the style's generation path. Every represented
    /// stop occurs during the first pass, before a retry decision.
    pub retry_attempt: u32,
    pub world_wiped: bool,
    pub world_inverted: bool,
    pub regions_cleared: u32,
    pub region_seeds: Vec<RegionSeedReceipt>,
    pub region_growths: Vec<GrowRegionReceipt>,
    /// Direct `Map::grow_valid` calls issued by the style virtual itself. The
    /// calls `Map::grow_region` makes internally stay inside its own receipt.
    pub grow_valid_calls: Vec<GrowValidReceipt>,
    /// Accepted `MapGreatLakes` lake seeds, in creation order.
    pub lake_candidates: Vec<LakeCandidateReceipt>,
    pub pool_eliminations: Vec<EliminatePoolsReceipt>,
    pub player_land: Option<CheckPlayerLandReceipt>,
    /// Present only for East Meets West after executing `Map::fill_cont`.
    pub team_partition: Option<TeamContinentPartitionReceipt>,
    /// Present only for East Indies after the privately mounted residual has
    /// returned from the style virtual. This retains native bounded-exit and
    /// exact interleaved RNG-span evidence for owner-transition provenance.
    pub east_indies_tail: Option<EastIndiesTailReceipt>,
    pub starts_added: usize,
    pub start_min: Option<i32>,
    pub stop: ContinentStop,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContinentError {
    SelectorMismatch {
        replay: u8,
        style: u8,
    },
    UnsupportedStyle {
        map_style: u8,
    },
    ZeroPlayers,
    PlayerLayoutMismatch {
        declared: u8,
        active: usize,
    },
    MapPrefixMismatch {
        expected_edge: i32,
        actual_xs: i32,
        actual_ys: i32,
        expected_seed: i32,
        actual_seed: i32,
    },
    InvalidMapDimensions {
        xs: i32,
        ys: i32,
    },
    InvalidRegionSeed {
        region: i32,
        x: i32,
        y: i32,
    },
    InvalidRegionCoordStorage {
        region: i32,
        length: usize,
        capacity: i32,
        increment: i16,
    },
    MissingMapParameter {
        tag: &'static str,
        attribute: &'static str,
    },
    InvalidMapParameter {
        tag: &'static str,
        attribute: &'static str,
        value: String,
    },
    RegionGrowth(GrowRegionError),
    RegionRebuild(don_sim::systems::regions::RegionsError),
    PoolElimination(EliminatePoolsError),
    RegionCentroid(RegionCentroidError),
    EdgeCanals(EliminateEdgeCanalsError),
    StartBoundary(EastMeetsWestStartBoundaryError),
    PlaceStartSelector(PlaceStartSelectorError),
    AddStartingLocation(EastMeetsWestAddStartError),
    RemainingStarts(EastMeetsWestRemainingStartsError),
    EastMeetsWestPlayerLand(EastMeetsWestPlayerLandError),
    RegionsFindAll(MapMakeFirstRegionsFindAllError),
    TerritoryLimits(MapMakeTerritoryLimitsError),
    FixDiagLand(MapFixDiagLandError),
    PostFixDiagStringConstructor(MapMakePostFixDiagStringConstructorError),
    PostFixDiagGameLog(MapMakePostFixDiagGameLogError),
    PostChecksumStringClose(MapMakePostChecksumStringCloseError),
    PlayerLand(CheckPlayerLandError),
    EastIndiesTail(EastIndiesTailError),
    TeamPartition(TeamContinentPartitionError),
    InvalidActiveSlot {
        slot: u8,
    },
    DuplicateActiveSlot {
        slot: u8,
    },
    StartPlacementUnavailable {
        player: usize,
        attempts: usize,
    },
}

/// Execute the largest deterministic prefix of one admitted style virtual.
///
/// Validation is transactional: every fail-closed input check happens before
/// either `World::wipe` or an RNG draw. Once execution begins the receipt names
/// the exact next primitive, so no caller can mistake a partial map for a
/// completed one.
pub fn execute_continent_prefix(
    inputs: &InitialWorldgenInputs,
    style: &MapStyleStaticData,
    world: &mut World,
) -> Result<ContinentReceipt, ContinentError> {
    let mut regions = Regions::default();
    execute_continent_prefix_with_regions_from_rng(
        inputs,
        style,
        inputs.seed as i32,
        world,
        &mut regions,
    )
}

/// Execute from an explicitly proven main-RNG state. Replay integration uses
/// the post-tileset-selection state; the legacy wrapper above remains useful
/// for isolated style-virtual tests which intentionally begin at the seed.
pub fn execute_continent_prefix_from_rng(
    inputs: &InitialWorldgenInputs,
    style: &MapStyleStaticData,
    rng_initial: i32,
    world: &mut World,
) -> Result<ContinentReceipt, ContinentError> {
    let mut regions = Regions::default();
    execute_continent_prefix_with_regions_from_rng(inputs, style, rng_initial, world, &mut regions)
}

/// Stateful form used by replay reconstruction. `Regions` is retained beside
/// the generated world so `make_region` and later growth stages have one
/// authoritative caller-owned store.
pub fn execute_continent_prefix_with_regions(
    inputs: &InitialWorldgenInputs,
    style: &MapStyleStaticData,
    world: &mut World,
    regions: &mut Regions,
) -> Result<ContinentReceipt, ContinentError> {
    execute_continent_prefix_with_regions_from_rng(
        inputs,
        style,
        inputs.seed as i32,
        world,
        regions,
    )
}

/// Stateful, explicit-RNG form used by the real replay map flow.
pub fn execute_continent_prefix_with_regions_from_rng(
    inputs: &InitialWorldgenInputs,
    style: &MapStyleStaticData,
    rng_initial: i32,
    world: &mut World,
    regions: &mut Regions,
) -> Result<ContinentReceipt, ContinentError> {
    if style.identity.ordinal != inputs.map_style {
        return Err(ContinentError::SelectorMismatch {
            replay: inputs.map_style,
            style: style.identity.ordinal,
        });
    }
    let make_continents_va =
        style
            .identity
            .make_continents_va
            .ok_or(ContinentError::UnsupportedStyle {
                map_style: inputs.map_style,
            })?;
    if !matches!(inputs.map_style, 6 | 9 | 12 | 14 | 18 | 19) {
        return Err(ContinentError::UnsupportedStyle {
            map_style: inputs.map_style,
        });
    }
    let players = inputs.active_slots.len();
    if players == 0 {
        return Err(ContinentError::ZeroPlayers);
    }
    if players > u8::MAX as usize
        || inputs.active_who.len() != players
        || inputs.active_teams.len() != players
    {
        return Err(ContinentError::PlayerLayoutMismatch {
            declared: players.min(u8::MAX as usize) as u8,
            active: inputs.active_who.len(),
        });
    }
    let players = players as u8;
    let edge = inputs.map_edge_world_cells.unwrap_or(0);
    let seed = inputs.seed as i32;
    if (world.xs, world.ys, world.seed) != (edge, edge, seed) {
        return Err(ContinentError::MapPrefixMismatch {
            expected_edge: edge,
            actual_xs: world.xs,
            actual_ys: world.ys,
            expected_seed: seed,
            actual_seed: world.seed,
        });
    }
    // Every supported retail map is much larger, but two cells is the exact
    // safety floor for the first style operations (`% xs`, radii, and edge
    // tests). Reject malformed direct API inputs before wipe or RNG mutation.
    if world.xs < 2 || world.ys < 2 {
        return Err(ContinentError::InvalidMapDimensions {
            xs: world.xs,
            ys: world.ys,
        });
    }

    // `Map::make_region` reads these installed Map fields. Resolve them before
    // any wipe or RNG draw so missing static data remains transactional.
    let region_defaults = if matches!(inputs.map_style, 12 | 14 | 18 | 19) {
        RegionSeedDefaults {
            common_factor: map_int(style, "COMMON_RESOURCES", "value")?,
            goody_factor: map_int(style, "GOODY_BOXES", "value")?,
            flags: 0,
        }
    } else {
        RegionSeedDefaults {
            common_factor: 0,
            goody_factor: 0,
            flags: 0,
        }
    };

    // Map::Map initializes orientation to -1. load_map_data resolves BASE_EDGE
    // from the selected/default MAP data before this branch.
    let base_edge = map_int(style, "BASE_EDGE", "value")?;
    if !(-1..=3).contains(&base_edge) {
        return Err(ContinentError::InvalidMapParameter {
            tag: "BASE_EDGE",
            attribute: "value",
            value: base_edge.to_string(),
        });
    }
    let mut rng = Random::new(rng_initial);
    let mut sites = Vec::new();
    let orientation = if base_edge < 0 {
        sites.push(MAP_MAKE_ORIENTATION_RNG_VA);
        rng.get(0, 0xffff) & 3
    } else {
        base_edge
    };
    // The geometry algorithms are written against clones so every Rust-side
    // validation failure leaves both authoritative stores unchanged.
    let mut next_world = world.clone();
    let mut next_regions = regions.clone();
    let partial = match inputs.map_style {
        6 | 9 => old_world_or_himalayas(
            inputs.map_style,
            players,
            &mut next_world,
            &mut next_regions,
            &mut rng,
            &mut sites,
        ),
        12 => mediterranean(
            inputs,
            orientation,
            &mut next_world,
            &mut next_regions,
            &mut rng,
            &mut sites,
            style,
            region_defaults,
        )?,
        14 => great_lakes(
            players,
            inputs.map_size,
            orientation,
            &mut next_world,
            &mut next_regions,
            &mut rng,
            &mut sites,
            style,
            region_defaults,
        )?,
        18 => east_indies(
            players,
            inputs.map_size,
            orientation,
            &mut next_world,
            &mut next_regions,
            &mut rng,
            &mut sites,
            style,
            region_defaults,
        )?,
        19 => east_meets_west(
            inputs,
            orientation,
            &mut next_world,
            &mut next_regions,
            &mut rng,
            &mut sites,
            style,
            region_defaults,
        )?,
        _ => unreachable!("admitted above"),
    };

    *world = next_world;
    *regions = next_regions;

    Ok(ContinentReceipt {
        map_style: inputs.map_style,
        make_continents_va,
        orientation,
        rng_initial,
        rng_final: rng.state(),
        direct_rng_sites: sites,
        retry_attempt: 1,
        world_wiped: true,
        world_inverted: partial.world_inverted,
        regions_cleared: partial.regions_cleared,
        region_seeds: partial.region_seeds,
        region_growths: partial.region_growths,
        grow_valid_calls: partial.grow_valid_calls,
        lake_candidates: partial.lake_candidates,
        pool_eliminations: partial.pool_eliminations,
        player_land: partial.player_land,
        team_partition: partial.team_partition,
        east_indies_tail: partial.east_indies_tail,
        starts_added: partial.starts_added,
        start_min: partial.start_min,
        stop: partial.stop,
    })
}

struct PartialReceipt {
    world_inverted: bool,
    regions_cleared: u32,
    region_seeds: Vec<RegionSeedReceipt>,
    region_growths: Vec<GrowRegionReceipt>,
    grow_valid_calls: Vec<GrowValidReceipt>,
    lake_candidates: Vec<LakeCandidateReceipt>,
    pool_eliminations: Vec<EliminatePoolsReceipt>,
    player_land: Option<CheckPlayerLandReceipt>,
    team_partition: Option<TeamContinentPartitionReceipt>,
    east_indies_tail: Option<EastIndiesTailReceipt>,
    starts_added: usize,
    start_min: Option<i32>,
    stop: ContinentStop,
}

#[derive(Copy, Clone)]
struct RegionSeedDefaults {
    common_factor: i32,
    goody_factor: i32,
    flags: i32,
}

fn old_world_or_himalayas(
    map_style: u8,
    players: u8,
    world: &mut World,
    regions: &mut Regions,
    rng: &mut Random,
    sites: &mut Vec<u32>,
) -> PartialReceipt {
    world.wipe();
    regions.clear_all(world);
    invert_land(world);
    // `Map::invert_land` ends with its own `Regions::clear_all` call.
    regions.clear_all(world);

    let radius_half = world.xs.min(world.ys) / 2;
    let radius = ((radius_half * 80) / 100).min(radius_half - 4);
    let increment = (u32::MAX / players as u32) as i32;
    let style_sites: &[u32] = if map_style == 6 {
        &crate::map_style::OLD_WORLD_DIRECT_RNG_SITES
    } else {
        &crate::map_style::HIMALAYAS_DIRECT_RNG_SITES
    };
    let a = draw(rng, sites, style_sites[0]);
    let b = draw(rng, sites, style_sites[1]);
    let mut angle = a.wrapping_shl(16).wrapping_add(b);
    let center_x = world.xs / 2 + (draw(rng, sites, style_sites[2]) & 1);
    let center_y = world.ys / 2 + (draw(rng, sites, style_sites[3]) & 1);
    let start_min = (world.xs / 16).max(4);

    for _ in 0..players {
        angle = angle.wrapping_add(increment);
        let (x, y) = project(center_x, center_y, angle, radius);
        world.add_starting_location(WCoord(x), WCoord(y));
    }
    PartialReceipt {
        world_inverted: true,
        regions_cleared: 2,
        region_seeds: Vec::new(),
        region_growths: Vec::new(),
        grow_valid_calls: Vec::new(),
        lake_candidates: Vec::new(),
        pool_eliminations: Vec::new(),
        player_land: None,
        team_partition: None,
        east_indies_tail: None,
        starts_added: players as usize,
        start_min: Some(start_min),
        stop: ContinentStop::HookComplete {
            next_va: REGIONS_CLEAR_ALL_VA,
        },
    }
}

fn mediterranean(
    inputs: &InitialWorldgenInputs,
    orientation: i32,
    world: &mut World,
    regions: &mut Regions,
    rng: &mut Random,
    sites: &mut Vec<u32>,
    style: &MapStyleStaticData,
    defaults: RegionSeedDefaults,
) -> Result<PartialReceipt, ContinentError> {
    world.wipe();
    regions.clear_all(world);
    let style_sites = &crate::map_style::MEDITERRANEAN_DIRECT_RNG_SITES;
    let min_dim = (world.xs.min(world.ys) & !1) / 3;
    let a = draw(rng, sites, style_sites[0]);
    let b = draw(rng, sites, style_sites[1]);
    let mut angle = a.wrapping_shl(16).wrapping_add(b);
    let x = world.xs / 2 + (draw(rng, sites, style_sites[2]) & 1);
    let y = world.ys / 2 + (draw(rng, sites, style_sites[3]) & 1);
    let area = min_dim.wrapping_mul(min_dim).wrapping_mul(3);
    let seed = RegionSeedCall {
        region: 1,
        x,
        y,
        area,
    };
    let seed_receipt = apply_make_region(world, regions, &seed, defaults)?;
    let mut growth_config = map_growth_config(
        style,
        world,
        inputs.active_slots.len() as u8,
        inputs.map_size,
        orientation,
    )?;
    let growth_call = GrowRegionCall {
        region: 1,
        target_area: area,
        max_distance: min_dim,
        anchor_x: -1,
        anchor_y: -1,
        return_partial_size: 0,
    };
    let growth = execute_grow_region(world, regions, rng, &mut growth_config, &growth_call)
        .map_err(ContinentError::RegionGrowth)?;
    sites.extend_from_slice(&growth.rng_sites);
    if growth.retail_return != 0 {
        return Ok(PartialReceipt {
            world_inverted: false,
            regions_cleared: 1,
            region_seeds: vec![seed_receipt],
            region_growths: vec![growth.clone()],
            grow_valid_calls: Vec::new(),
            lake_candidates: Vec::new(),
            pool_eliminations: Vec::new(),
            player_land: None,
            team_partition: None,
            east_indies_tail: None,
            starts_added: 0,
            start_min: None,
            stop: ContinentStop::RetryGeneration {
                failed_region: 1,
                retail_return: growth.retail_return,
            },
        });
    }
    regions
        .rebuild_after_coastlines(world)
        .map_err(ContinentError::RegionRebuild)?;
    let first_pools = execute_eliminate_pools(world, regions, ElimPoolParam::EntireWorld)
        .map_err(ContinentError::PoolElimination)?;
    regions.clear_all(world);
    invert_land(world);
    // `Map::invert_land` ends with `Regions::clear_all`; its Rust leaf owns
    // only World, so retain the authoritative Regions mutation here.
    regions.clear_all(world);
    // Retail now calls find_all directly on the just-cleared state. The public
    // composite performs one idempotent clear before the exact find body.
    regions
        .rebuild_after_coastlines(world)
        .map_err(ContinentError::RegionRebuild)?;
    let second_pools = execute_eliminate_pools(world, regions, ElimPoolParam::EntireWorld)
        .map_err(ContinentError::PoolElimination)?;
    regions.clear_all(world);

    let players = inputs.active_slots.len();
    let increment = (u32::MAX / players as u32) as i32;
    let circle = circle_table();
    let initial_radius = world.xs.wrapping_mul(3) / 2;
    let max_attempts = world.wdata.len().saturating_mul(32).max(1);
    for player in 0..players {
        angle = angle.wrapping_add(increment);
        let mut radius = initial_radius;
        let mut near_ocean_seen = false;
        let mut cross_touches_edge = false;
        let mut accepted = None;
        for _ in 0..max_attempts {
            let candidate = project(seed.x, seed.y, angle, radius);
            radius = radius.wrapping_sub(3);
            if radius < min_dim {
                angle = angle.wrapping_add(0x071c_71c6);
                radius = initial_radius;
            } else {
                let (x, y) = candidate;
                cross_touches_edge =
                    [(0, 0), (-1, 0), (0, 1), (1, 0), (0, -1)]
                        .into_iter()
                        .any(|(dx, dy)| {
                            let nx = x.wrapping_add(dx);
                            let ny = y.wrapping_add(dy);
                            !world.valid_w(nx, ny)
                                || nx == 0
                                || ny == 0
                                || nx == world.xs - 1
                                || ny == world.ys - 1
                        });
                if !cross_touches_edge
                    && world.valid_w(x, y)
                    && {
                        let cell = world.wdata(x, y);
                        cell.flags & wflag::WATERHALF != 0
                            || (cell.land != land::OCEAN && cell.land != land::COASTAL)
                    }
                    && world.is_near_ocean(&circle, WCoord(x), WCoord(y), 3, 9)
                {
                    // Retail does not reset this flag between candidates for
                    // one player.
                    near_ocean_seen = true;
                }
            }
            let (x, y) = candidate;
            if cross_touches_edge || !world.valid_w(x, y) {
                continue;
            }
            let cell = world.wdata(x, y);
            let water = cell.flags & wflag::WATERHALF == 0
                && (cell.land == land::OCEAN || cell.land == land::COASTAL);
            if water {
                continue;
            }
            if near_ocean_seen {
                accepted = Some((x, y));
                break;
            }
        }
        let Some((x, y)) = accepted else {
            return Err(ContinentError::StartPlacementUnavailable {
                player,
                attempts: max_attempts,
            });
        };
        world.add_starting_location(WCoord(x), WCoord(y));
    }
    // unittypes.items[349] is Battleship. PDB +0x1fc names
    // ObjectTypeData::max_range; NAVAL_ROSTER and unitrules.xml both fix it at
    // 24 TCoords. Retail rounds signed division up to six before this call.
    let player_land = execute_check_player_land(
        world,
        regions,
        CheckPlayerLandCall {
            enabled: 1,
            avoid_continent: growth_config.avoid_continent,
            radius: 6,
            unused: 0,
        },
    )
    .map_err(ContinentError::PlayerLand)?;

    // The complete caller tail creates region 32, grows it, and removes only
    // edge-connected pools before returning from the style virtual.
    let second_area =
        scale_land_area(150, inputs.active_slots.len() as u8, inputs.map_size).max(75);
    let second_seed = RegionSeedCall {
        region: 32,
        x: world.xs / 2 + (draw(rng, sites, style_sites[4]) & 1),
        y: world.ys / 2 + (draw(rng, sites, style_sites[5]) & 1),
        area: second_area,
    };
    let second_seed_receipt = apply_make_region(world, regions, &second_seed, defaults)?;
    let second_growth_call = GrowRegionCall {
        region: 32,
        target_area: second_area,
        max_distance: min_dim,
        anchor_x: -1,
        anchor_y: -1,
        return_partial_size: 0,
    };
    let second_growth =
        execute_grow_region(world, regions, rng, &mut growth_config, &second_growth_call)
            .map_err(ContinentError::RegionGrowth)?;
    sites.extend_from_slice(&second_growth.rng_sites);
    let second_failed = second_growth.retail_return != 0;
    let second_retail_return = second_growth.retail_return;
    if second_failed {
        return Ok(PartialReceipt {
            world_inverted: true,
            regions_cleared: 7,
            region_seeds: vec![seed_receipt, second_seed_receipt],
            region_growths: vec![growth, second_growth],
            grow_valid_calls: Vec::new(),
            lake_candidates: Vec::new(),
            pool_eliminations: vec![first_pools, second_pools],
            player_land: Some(player_land),
            team_partition: None,
            east_indies_tail: None,
            starts_added: players,
            start_min: None,
            stop: ContinentStop::RetryGeneration {
                failed_region: 32,
                retail_return: second_retail_return,
            },
        });
    }
    let final_pools = execute_eliminate_pools(world, regions, ElimPoolParam::EdgesOnly)
        .map_err(ContinentError::PoolElimination)?;
    Ok(PartialReceipt {
        world_inverted: true,
        regions_cleared: 8,
        region_seeds: vec![seed_receipt, second_seed_receipt],
        region_growths: vec![growth, second_growth],
        grow_valid_calls: Vec::new(),
        lake_candidates: Vec::new(),
        pool_eliminations: vec![first_pools, second_pools, final_pools],
        player_land: Some(player_land),
        team_partition: None,
        east_indies_tail: None,
        starts_added: players,
        start_min: None,
        stop: ContinentStop::HookComplete {
            next_va: REGIONS_CLEAR_ALL_VA,
        },
    })
}

/// `MapGreatLakes::make_continents` `0x00699e40`–`0x0069a641`.
///
/// The style grows ordinary land regions on an all-ocean world and then calls
/// `Map::invert_land`, so the regions it created *become* the lakes. Structure,
/// read off the instruction stream:
///
/// ```text
/// 0x00699e80  avoid_continent = max(scale_land_area(Map+0x30, players), 6)  -> Map+0x30
/// 0x00699f63  World::wipe ; Regions::clear_all
/// 0x00699f88  angle       = (draw << 16) + draw
/// 0x00699fb5  area_max    = max(size / 20, 90) ; area_min = max(size / 40, 24)
/// 0x0069a002  remaining   = xs / 12 + (draw & 1)
/// 0x0069a043  loop:  attempts += 1 ; if attempts >= 100 -> done
/// 0x0069a060         required = max((avoid_continent + 1) / 2 + isqrt(area * 7 / 22), 3)
/// 0x0069a150         candidate: x = draw % xs ; y = draw % ys
/// 0x0069a255                    if Map::land_dist(x, y, 1) < required and
///                               fewer than 1000 rejections -> candidate
/// 0x0069a272         1000 rejections -> shrink
/// 0x0069a32d         if Map::grow_valid(last + 1, x, y):
/// 0x0069a3fe             Map::make_region(region, x, y, area)
/// 0x0069a415             Map::grow_region(region, area, xs, -1, -1, 0)   [return ignored]
/// 0x0069a41a             remaining -= 1 ; attempts = 0
/// 0x0069a424         area = area_min + (draw % (area_max - area_min)) when that span > 1
/// 0x0069a45a  shrink: area > area_min ? area_max = area = area_min
///                                     : area_min = area_min * 13 / 16, and stop below 20
/// 0x0069a489         repeat while remaining != 0
/// 0x0069a496  done:  Map::eliminate_pools(EntireWorld) ; Map::invert_land
/// 0x0069a4c0         per player: spiral a start in from radius (xs * 3) / 2, step -3
/// 0x0069a617         Map::check_player_land(1, avoid_continent, 6, _)
/// ```
///
/// `Map::grow_region`'s return value is deliberately *not* tested at
/// `0x0069a41a`, so unlike Mediterranean and East Indies this style has no
/// whole-pass retry branch.
#[allow(clippy::too_many_arguments)]
fn great_lakes(
    players: u8,
    map_size: u8,
    orientation: i32,
    world: &mut World,
    regions: &mut Regions,
    rng: &mut Random,
    sites: &mut Vec<u32>,
    style: &MapStyleStaticData,
    defaults: RegionSeedDefaults,
) -> Result<PartialReceipt, ContinentError> {
    // `Map+0x30` already holds the resolved AVOID_CONTINENT expression when the
    // virtual is entered; the style rescales it and writes it back, so every
    // later `grow_valid`/`check_player_land` read sees the clamped value.
    let mut growth_config = map_growth_config(style, world, players, map_size, orientation)?;
    let avoid_continent = scale_land_area(
        map_scaled_int(style, "AVOID_CONTINENT", "scalevalue", world)?,
        players,
        map_size,
    )
    .max(6);
    growth_config.avoid_continent = avoid_continent;

    world.wipe();
    regions.clear_all(world);
    let style_sites = &crate::map_style::GREAT_LAKES_DIRECT_RNG_SITES;
    let a = draw(rng, sites, style_sites[0]);
    let b = draw(rng, sites, style_sites[1]);
    // Retail takes `% 0xffff` of each draw. `Random::get(0, 0xffff)` is
    // half-open, so the remainder is the identity on the whole output range.
    let mut angle = a.wrapping_shl(16).wrapping_add(b);
    let mut area_max = world.size / 20;
    if area_max < 90 {
        area_max = 90;
    }
    let mut area_min = world.size / 40;
    if area_min < 24 {
        area_min = 24;
    }
    let mut area = area_max;
    let mut remaining = world.xs / 12 + (draw(rng, sites, style_sites[2]) & 1);

    let circle = circle_table();
    let mut attempts = 0;
    let mut last_region = 0;
    let mut region_seeds = Vec::new();
    let mut region_growths = Vec::new();
    let mut grow_valid_calls = Vec::new();
    let mut lake_candidates = Vec::new();
    while remaining != 0 {
        attempts += 1;
        if attempts >= 100 {
            break;
        }
        let shrunk = area.wrapping_mul(7) / 22;
        let required_distance = ((avoid_continent + 1) / 2 + integer_sqrt_floor(shrunk)).max(3);

        let mut rejected = 0;
        let accepted = loop {
            // Retail skips the draw entirely on a degenerate axis; the entry
            // guard already rejects maps below two cells, but the branch is
            // reproduced because it changes the draw count, not just the value.
            let x = if world.xs > 1 {
                draw(rng, sites, style_sites[3]) % world.xs
            } else {
                0
            };
            let y = if world.ys > 1 {
                draw(rng, sites, style_sites[4]) % world.ys
            } else {
                0
            };
            let distance = world.land_dist(&circle, WCoord(x), WCoord(y), true);
            if distance >= required_distance {
                break Some((x, y, distance));
            }
            rejected += 1;
            if rejected >= 1000 {
                break None;
            }
        };

        let Some((x, y, land_distance)) = accepted else {
            // 0x0069a45a. No draw is consumed on this arm.
            if area > area_min {
                area_max = area_min;
                area = area_min;
            } else {
                area_min = area_min.wrapping_mul(13) / 16;
                area_max = area_min;
                area = area_min;
                if area_min < 20 {
                    break;
                }
            }
            continue;
        };

        let region = last_region + 1;
        let valid = execute_grow_valid(
            world,
            regions,
            rng,
            &mut growth_config,
            &GrowValidCall { region, x, y },
        )
        .map_err(ContinentError::RegionGrowth)?;
        sites.extend_from_slice(&valid.rng_sites);
        let grow_valid_return = valid.retail_return;
        grow_valid_calls.push(valid);
        if grow_valid_return != 0 {
            last_region = region;
            let call = RegionSeedCall { region, x, y, area };
            region_seeds.push(apply_make_region(world, regions, &call, defaults)?);
            let growth = execute_grow_region(
                world,
                regions,
                rng,
                &mut growth_config,
                &GrowRegionCall {
                    region,
                    target_area: area,
                    max_distance: world.xs,
                    anchor_x: -1,
                    anchor_y: -1,
                    return_partial_size: 0,
                },
            )
            .map_err(ContinentError::RegionGrowth)?;
            sites.extend_from_slice(&growth.rng_sites);
            region_growths.push(growth);
            remaining -= 1;
            attempts = 0;
        }
        lake_candidates.push(LakeCandidateReceipt {
            region,
            x,
            y,
            area,
            required_distance,
            land_distance,
            rejected_candidates: rejected,
            grow_valid_return,
        });

        // 0x0069a424. The span is the *current* max minus min, and the draw is
        // taken only when it exceeds one.
        let span = area_max - area_min;
        area = if span > 1 {
            area_min + (draw(rng, sites, style_sites[5]) % span)
        } else {
            area_min
        };
    }

    let pools = execute_eliminate_pools(world, regions, ElimPoolParam::EntireWorld)
        .map_err(ContinentError::PoolElimination)?;
    invert_land(world);
    // `Map::invert_land` ends with its own `Regions::clear_all`; the Rust leaf
    // owns only World, so the authoritative Regions mutation stays here.
    regions.clear_all(world);

    let players = usize::from(players);
    let increment = (u32::MAX / players as u32) as i32;
    let center = world.xs / 2;
    let max_attempts = world.wdata.len().saturating_mul(32).max(1);
    for player in 0..players {
        angle = angle.wrapping_add(increment);
        let mut radius = world.xs.wrapping_mul(3) / 2;
        let mut accepted = None;
        for _ in 0..max_attempts {
            // Retail projects from (xs/2, xs/2): both centre arguments are the
            // same `xs >> 1`, not the y axis (`0x0069a4ae`, `0x0069a4d9`).
            let (x, y) = project(center, center, angle, radius);
            radius = radius.wrapping_sub(3);
            let touches_edge = GREAT_LAKES_START_CROSS.into_iter().any(|(dx, dy)| {
                let nx = x.wrapping_add(dx);
                let ny = y.wrapping_add(dy);
                !world.valid_w(nx, ny)
                    || nx == 0
                    || ny == 0
                    || nx == world.xs - 1
                    || ny == world.ys - 1
            });
            if !world.valid_w(x, y) || world.is_ocean(x, y) || touches_edge {
                continue;
            }
            accepted = Some((x, y));
            break;
        }
        let Some((x, y)) = accepted else {
            return Err(ContinentError::StartPlacementUnavailable {
                player,
                attempts: max_attempts,
            });
        };
        world.add_starting_location(WCoord(x), WCoord(y));
    }

    let player_land = execute_check_player_land(
        world,
        regions,
        CheckPlayerLandCall {
            enabled: 1,
            avoid_continent,
            radius: MAP_PLAYER_LAND_RADIUS,
            unused: 0,
        },
    )
    .map_err(ContinentError::PlayerLand)?;

    Ok(PartialReceipt {
        world_inverted: true,
        // clear_all, eliminate_pools' find_all rebuild, invert_land's clear_all.
        regions_cleared: 3,
        region_seeds,
        region_growths,
        grow_valid_calls,
        lake_candidates,
        pool_eliminations: vec![pools],
        player_land: Some(player_land),
        team_partition: None,
        east_indies_tail: None,
        starts_added: players,
        start_min: None,
        stop: ContinentStop::HookComplete {
            next_va: REGIONS_CLEAR_ALL_VA,
        },
    })
}

/// The five-entry `int` cross retail tests around a Great Lakes start
/// candidate: `x` offsets at `0x00add250` and `y` offsets at `0x00add210`, read
/// from the image as `[(0,0), (0,-1), (1,0), (0,1), (-1,0)]`. These are the
/// four-ring tables shifted back by one entry, so the candidate itself is
/// included.
const GREAT_LAKES_START_CROSS: [(i32, i32); 5] = [(0, 0), (0, -1), (1, 0), (0, 1), (-1, 0)];

fn east_indies(
    players: u8,
    map_size: u8,
    orientation: i32,
    world: &mut World,
    regions: &mut Regions,
    rng: &mut Random,
    sites: &mut Vec<u32>,
    style: &MapStyleStaticData,
    defaults: RegionSeedDefaults,
) -> Result<PartialReceipt, ContinentError> {
    // The selected East Indies XML supplies plain integer edge values; this
    // exact maximum is read before the first make_region call.
    let margin = [
        "AVOID_EDGE_0",
        "AVOID_EDGE_1",
        "AVOID_EDGE_2",
        "AVOID_EDGE_3",
    ]
    .into_iter()
    .map(|tag| map_int(style, tag, "scalevalue"))
    .collect::<Result<Vec<_>, _>>()?
    .into_iter()
    .max()
    .unwrap_or(0)
    .wrapping_add(4);
    let _land_area = scale_land_area(
        map_int(style, "AVOID_CONTINENT", "scalevalue")?,
        players,
        map_size,
    )
    .max(4);
    let region_area = scale_land_area(250, players, map_size);
    let mut growth_config = map_growth_config(style, world, players, map_size, orientation)?;

    world.wipe();
    regions.clear_all(world);
    let style_sites = &crate::map_style::EAST_INDIES_DIRECT_RNG_SITES;
    let first = draw(rng, sites, style_sites[0]);
    let min_dim = world.xs.min(world.ys);
    let (mut angle, mut radius) = if players < 4 {
        let rem = first & 3;
        let angle = rem.wrapping_mul(0x3fff_ffff).wrapping_add(0x1fff_ffff);
        let radius = if players == 3 {
            (min_dim / 2).wrapping_mul(7) / 8
        } else {
            min_dim / 2
        };
        (angle, radius)
    } else {
        let second = draw(rng, sites, style_sites[1]);
        (
            first.wrapping_shl(16).wrapping_add(second),
            (min_dim / 2).wrapping_mul(7) / 8,
        )
    };
    let initial_radius = radius;
    let increment = (u32::MAX / players as u32) as i32;
    let center_x = world.xs / 2;
    let center_y = world.ys / 2;
    let mut seeds = Vec::with_capacity(players as usize);
    for region in 1..=players {
        angle = angle.wrapping_add(increment);
        let (x, y) = loop {
            let candidate = project(center_x, center_y, angle, radius);
            if candidate.0 > margin
                && world.xs - candidate.0 > margin
                && candidate.1 > margin
                && world.ys - candidate.1 > margin
            {
                break candidate;
            }
            radius -= 1;
        };
        world.add_starting_location(WCoord(x), WCoord(y));
        let call = RegionSeedCall {
            region: i32::from(region),
            x,
            y,
            area: region_area,
        };
        let mut receipt = apply_make_region(world, regions, &call, defaults)?;
        // Exact caller writes immediately following each make_region call.
        let seeded = &mut regions.list[region as usize];
        seeded.common_factor = 8;
        seeded.goody_factor = 8;
        seeded.flags |= 2;
        seeded.climate = 0;
        receipt.common_factor = seeded.common_factor;
        receipt.goody_factor = seeded.goody_factor;
        receipt.flags = seeded.flags;
        seeds.push(receipt);
    }
    let mut growths = Vec::with_capacity(players as usize * 2);
    let max_distance = initial_radius / 2;
    for target_area in [region_area / 3, region_area] {
        for region in 1..=players {
            let call = GrowRegionCall {
                region: i32::from(region),
                target_area,
                max_distance,
                anchor_x: -1,
                anchor_y: -1,
                return_partial_size: 0,
            };
            let growth = execute_grow_region(world, regions, rng, &mut growth_config, &call)
                .map_err(ContinentError::RegionGrowth)?;
            sites.extend_from_slice(&growth.rng_sites);
            let failed = growth.retail_return != 0;
            let retail_return = growth.retail_return;
            growths.push(growth);
            if failed {
                return Ok(PartialReceipt {
                    world_inverted: false,
                    regions_cleared: 1,
                    region_seeds: seeds,
                    region_growths: growths,
                    grow_valid_calls: Vec::new(),
                    lake_candidates: Vec::new(),
                    pool_eliminations: Vec::new(),
                    player_land: None,
                    team_partition: None,
                    east_indies_tail: None,
                    starts_added: players as usize,
                    start_min: None,
                    stop: ContinentStop::RetryGeneration {
                        failed_region: i32::from(region),
                        retail_return,
                    },
                });
            }
        }
    }
    let tail = execute_east_indies_tail(
        world,
        regions,
        rng,
        &mut growth_config,
        EastIndiesTailCall {
            player_regions: i32::from(players),
            region_defaults: EastIndiesRegionDefaults {
                common_factor: defaults.common_factor,
                goody_factor: defaults.goody_factor,
                flags: defaults.flags,
            },
        },
    )
    .map_err(ContinentError::EastIndiesTail)?;

    // Preserve the tail's exact direct/helper interleaving in the canonical
    // receipt stream. Successful island seeds and growths follow the player
    // regions in their native execution order; grow-valid also records the
    // rejected candidates which did not create a region.
    sites.extend(
        tail.rng_chronology
            .iter()
            .flat_map(|span| span.call_sites.iter().copied()),
    );
    seeds.extend(tail.islands.iter().map(|island| RegionSeedReceipt {
        call: RegionSeedCall {
            region: island.region,
            x: island.x,
            y: island.y,
            area: island.target_area,
        },
        coord_capacity_before: island.coord_capacity_before,
        coord_capacity_after: island.coord_capacity_after,
        common_factor: island.common_factor,
        goody_factor: island.goody_factor,
        flags: island.flags,
    }));
    growths.extend(tail.islands.iter().map(|island| island.growth.clone()));

    Ok(PartialReceipt {
        world_inverted: false,
        regions_cleared: 1,
        region_seeds: seeds,
        region_growths: growths,
        grow_valid_calls: tail.grow_valid_calls.clone(),
        lake_candidates: Vec::new(),
        pool_eliminations: Vec::new(),
        player_land: None,
        team_partition: None,
        east_indies_tail: Some(tail),
        starts_added: players as usize,
        start_min: None,
        // Both bounded native escape paths return from the style virtual even
        // if one or more requested islands remain unplaced.
        stop: ContinentStop::HookComplete {
            next_va: REGIONS_CLEAR_ALL_VA,
        },
    })
}

#[allow(clippy::too_many_arguments)]
fn east_meets_west(
    inputs: &InitialWorldgenInputs,
    orientation: i32,
    world: &mut World,
    regions: &mut Regions,
    rng: &mut Random,
    sites: &mut Vec<u32>,
    style: &MapStyleStaticData,
    defaults: RegionSeedDefaults,
) -> Result<PartialReceipt, ContinentError> {
    let players = inputs.active_slots.len() as u8;
    let mut growth_config = map_growth_config(style, world, players, inputs.map_size, orientation)?;
    // `Map+0x30` enters as the XML-resolved AVOID_CONTINENT value. The style
    // rescales, floors at four, caps at sixteen, and every following
    // `grow_valid` reads that stored value.
    let avoid_continent = scale_land_area(
        map_scaled_int(style, "AVOID_CONTINENT", "scalevalue", world)?,
        players,
        inputs.map_size,
    )
    .max(4)
    .min(16);
    growth_config.avoid_continent = avoid_continent;

    world.wipe();
    regions.clear_all(world);

    let mut leaders = [None; 8];
    for (&slot, &team) in inputs.active_slots.iter().zip(&inputs.active_teams) {
        let Some(entry) = leaders.get_mut(slot as usize) else {
            return Err(ContinentError::InvalidActiveSlot { slot });
        };
        if entry.is_some() {
            return Err(ContinentError::DuplicateActiveSlot { slot });
        }
        *entry = Some(team);
    }
    let partition =
        execute_team_continent_partition(&leaders, rng).map_err(ContinentError::TeamPartition)?;
    sites.extend(partition.random_draws.iter().map(|draw| draw.call_va));

    let sides = usize::from(partition.continent_count);
    debug_assert!(sides > 0 && sides <= 8);
    let style_sites = &crate::map_style::EAST_MEETS_WEST_DIRECT_RNG_SITES;
    let min_dim = world.xs.min(world.ys);
    let radius = (min_dim / 2).wrapping_mul(3) / 4;
    let first = draw(rng, sites, style_sites[0]) % 0xffff;
    let second = draw(rng, sites, style_sites[1]) % 0xffff;
    let mut angle = first.wrapping_shl(16).wrapping_add(second);
    let per_player_area = world.size / i32::from(players);
    let region_area = scale_land_area(
        per_player_area.wrapping_mul(5) / 9,
        players,
        inputs.map_size,
    );
    let increment = (u32::MAX / u32::from(players)) as i32;
    let center_x = world.xs / 2;
    let center_y = world.ys / 2;
    let mut region_seeds = Vec::with_capacity(sides);
    for continent in 0..sides {
        let (x, y) = project(center_x, center_y, angle, radius);
        let call = RegionSeedCall {
            region: continent as i32 + 1,
            x,
            y,
            area: region_area,
        };
        region_seeds.push(apply_make_region(world, regions, &call, defaults)?);

        // Native uses logical `shr 1` on both wrapped signed products before
        // adding the two half-arcs to the current angle.
        let next = (continent + 1) % sides;
        let left = (partition.continent_counts[continent].wrapping_mul(increment) as u32) >> 1;
        let right = (partition.continent_counts[next].wrapping_mul(increment) as u32) >> 1;
        angle = angle.wrapping_add(left as i32).wrapping_add(right as i32);
    }

    let max_distance = radius.wrapping_mul(3) / 4;
    let mut region_growths = Vec::with_capacity(sides * 2);
    for base_area in [region_area / 2, region_area] {
        for continent in 0..sides {
            let region = continent as i32 + 1;
            let call = GrowRegionCall {
                region,
                target_area: base_area.wrapping_mul(partition.continent_counts[continent]),
                max_distance,
                anchor_x: -1,
                anchor_y: -1,
                return_partial_size: 0,
            };
            let growth = execute_grow_region(world, regions, rng, &mut growth_config, &call)
                .map_err(ContinentError::RegionGrowth)?;
            sites.extend_from_slice(&growth.rng_sites);
            let failed = growth.retail_return != 0;
            let retail_return = growth.retail_return;
            region_growths.push(growth);
            if failed {
                return Ok(PartialReceipt {
                    world_inverted: false,
                    regions_cleared: 1,
                    region_seeds,
                    region_growths,
                    grow_valid_calls: Vec::new(),
                    lake_candidates: Vec::new(),
                    pool_eliminations: Vec::new(),
                    player_land: None,
                    team_partition: Some(partition),
                    east_indies_tail: None,
                    starts_added: 0,
                    start_min: None,
                    stop: ContinentStop::RetryGeneration {
                        failed_region: region,
                        retail_return,
                    },
                });
            }
        }
    }

    let centroids = execute_east_meets_west_centroids(regions, sides as i32)
        .map_err(ContinentError::RegionCentroid)?;
    debug_assert_eq!(centroids.next_va, crate::pools::MAP_ELIMINATE_POOLS_VA);
    debug_assert_eq!(centroids.next_pool_param, ElimPoolParam::EntireWorld as i32);
    debug_assert_eq!(centroids.after_pools_va, MAP_ELIMINATE_EDGE_CANALS_VA);
    let pools = execute_eliminate_pools(world, regions, ElimPoolParam::EntireWorld)
        .map_err(ContinentError::PoolElimination)?;
    let edge_canals =
        execute_eliminate_edge_canals(world, regions).map_err(ContinentError::EdgeCanals)?;
    debug_assert_eq!(edge_canals.next_external_va, MAP_PLACE_START_IN_REGION_VA);
    debug_assert_eq!(
        edge_canals.next_caller_va,
        crate::edge_canals::EAST_MEETS_WEST_AFTER_EDGE_CANALS_VA
    );
    let place_start =
        execute_first_place_start_boundary(inputs, world, &partition, &centroids, rng)
            .map_err(ContinentError::StartBoundary)?;
    sites.extend_from_slice(&place_start.direct_rng_sites);
    debug_assert_eq!(place_start.primitive_va, MAP_PLACE_START_IN_REGION_VA);
    debug_assert_eq!(
        place_start.caller_va,
        EAST_MEETS_WEST_FIRST_PLACE_START_CALL_VA
    );
    let selector = execute_first_place_start_selector(
        world,
        regions,
        rng,
        place_start.region,
        place_start.min_start_distance,
        place_start.unread_argument,
    )
    .map_err(ContinentError::PlaceStartSelector)?;
    sites.extend(selector.draws.iter().map(|draw| draw.call_va));

    let selector_next = selector.next.clone();
    let (stop, starts_added, player_land_body) = match selector_next {
        PlaceStartSelectorNext::AddStartingLocation {
            caller_va,
            primitive_va,
        } => {
            let mutation = execute_first_add_start(
                world,
                EastMeetsWestAddStartInput {
                    selector_return_va: selector.return_va,
                    caller_va,
                    primitive_va,
                    output: selector.output,
                },
            )
            .map_err(ContinentError::AddStartingLocation)?;
            debug_assert_eq!(
                mutation.caller_return_va,
                EAST_MEETS_WEST_ADD_START_RETURN_VA
            );
            let remaining = execute_remaining_active_starts(
                inputs,
                world,
                regions,
                &partition,
                &centroids,
                &place_start,
                &selector,
                &mutation,
                rng,
            )
            .map_err(ContinentError::RemainingStarts)?;
            sites.extend_from_slice(&remaining.direct_rng_sites);
            let starts_added = 1 + remaining
                .iterations
                .iter()
                .filter(|iteration| iteration.mutation.is_some())
                .count();
            match remaining.next.clone() {
                EastMeetsWestRemainingStartsNext::CheckPlayerLand {
                    caller_va: player_land_call_va,
                    primitive_va: player_land_primitive_va,
                } => {
                    debug_assert_eq!(player_land_call_va, EAST_MEETS_WEST_PLAYER_LAND_CALL_VA);
                    debug_assert_eq!(player_land_primitive_va, MAP_CHECK_PLAYER_LAND_VA);
                    let player_land = execute_east_meets_west_player_land(
                        world,
                        regions,
                        EastMeetsWestPlayerLandCall {
                            expected_start_count: inputs.active_slots.len(),
                            avoid_continent,
                            // Retail pushes the current ECX value, but the complete
                            // leaf never reads formal argument four.
                            unread_stack_word: 0,
                            random_state: rng.state(),
                        },
                    )
                    .map_err(ContinentError::EastMeetsWestPlayerLand)?;
                    let EastMeetsWestPlayerLandNext::PostCallCleanup { entry_va, .. } =
                        player_land.next;
                    debug_assert_eq!(entry_va, EAST_MEETS_WEST_PLAYER_LAND_RESUME_VA);
                    let post_player_land_cleanup =
                        execute_east_meets_west_post_player_land_cleanup(
                            &player_land,
                            &centroids.centroid_y,
                        )
                        .map_err(ContinentError::EastMeetsWestPlayerLand)?;
                    let EastMeetsWestPostPlayerLandCleanupNext::FreeCentroidYList {
                        call_va,
                        import_iat_va,
                    } = post_player_land_cleanup.next;
                    debug_assert_eq!(call_va, EAST_MEETS_WEST_CENTROID_Y_FREE_CALL_VA);
                    debug_assert_eq!(import_iat_va, FREE_IMPORT_IAT_VA);
                    let centroid_y_free_cleanup = execute_east_meets_west_centroid_y_free_cleanup(
                        &post_player_land_cleanup,
                        &centroids.centroid_y,
                        &centroids.centroid_x,
                    )
                    .map_err(ContinentError::EastMeetsWestPlayerLand)?;
                    let EastMeetsWestCentroidYFreeCleanupNext::FreeCentroidXList {
                        call_va,
                        import_iat_va,
                    } = centroid_y_free_cleanup.next;
                    debug_assert_eq!(call_va, EAST_MEETS_WEST_CENTROID_X_FREE_CALL_VA);
                    debug_assert_eq!(import_iat_va, FREE_IMPORT_IAT_VA);
                    let centroid_x_free_cleanup = execute_east_meets_west_centroid_x_free_cleanup(
                        &centroid_y_free_cleanup,
                        &centroids.centroid_x,
                    )
                    .map_err(ContinentError::EastMeetsWestPlayerLand)?;
                    let EastMeetsWestCentroidXFreeCleanupNext::ReturnedFromMakeContinents {
                        ret_va,
                        callee_stack_argument_bytes_popped,
                    } = centroid_x_free_cleanup.next;
                    debug_assert_eq!(ret_va, EAST_MEETS_WEST_MAKE_CONTINENTS_RET_VA);
                    debug_assert_eq!(callee_stack_argument_bytes_popped, 4);
                    let regions_clear_all =
                        execute_map_make_first_regions_clear_all(world, regions, rng.state());
                    let MapMakeFirstRegionsClearAllNext::FindAll { call_va, .. } =
                        regions_clear_all.next;
                    debug_assert_eq!(call_va, MAP_MAKE_FIRST_REGIONS_FIND_CALL_VA);
                    let regions_find_all = execute_map_make_first_regions_find_all(
                        world,
                        regions,
                        rng.state(),
                        &regions_clear_all,
                    )
                    .map_err(ContinentError::RegionsFindAll)?;
                    let MapMakeFirstRegionsFindAllNext::TerritoryLimitStore { store_va, .. } =
                        regions_find_all.next;
                    debug_assert_eq!(store_va, MAP_MAKE_FIRST_TERRITORY_STORE_VA);
                    let territory_limit_source = TerritoryLimits::from_world_prefix(world);
                    let territory_limits = execute_map_make_territory_limits(
                        world,
                        regions,
                        inputs.map_style,
                        territory_limit_source,
                        rng.state(),
                        &regions_clear_all,
                        &regions_find_all,
                    )
                    .map_err(ContinentError::TerritoryLimits)?;
                    let MapMakeTerritoryLimitsNext::FixDiagLand {
                        call_va,
                        primitive_va: fix_diag_primitive_va,
                    } = territory_limits.next;
                    debug_assert_eq!(call_va, MAP_MAKE_FIRST_FIX_DIAG_LAND_CALL_VA);
                    debug_assert_eq!(
                        fix_diag_primitive_va,
                        crate::post_continent::MAP_FIX_DIAG_LAND_VA
                    );
                    let fix_diag_land = execute_map_fix_diag_land(
                        world,
                        regions,
                        rng.state(),
                        &regions_clear_all,
                        &regions_find_all,
                        &territory_limits,
                    )
                    .map_err(ContinentError::FixDiagLand)?;
                    let MapFixDiagLandNext::StringConstructor {
                        call_va,
                        primitive_va: string_constructor_va,
                        ..
                    } = fix_diag_land.next;
                    debug_assert_eq!(call_va, MAP_MAKE_POST_FIX_DIAG_STRING_CONSTRUCTOR_CALL_VA);
                    debug_assert_eq!(string_constructor_va, STRING_CONSTRUCTOR_VA);
                    let post_fix_diag_string_constructor =
                        execute_map_make_post_fix_diag_string_constructor(
                            world,
                            regions,
                            rng.state(),
                            &regions_clear_all,
                            &regions_find_all,
                            &territory_limits,
                            &fix_diag_land,
                        )
                        .map_err(ContinentError::PostFixDiagStringConstructor)?;
                    let MapMakePostFixDiagStringConstructorNext::GameLogSayChecksum(next) =
                        post_fix_diag_string_constructor.next;
                    debug_assert_eq!(next.call_va, MAP_MAKE_POST_FIX_DIAG_GAME_LOG_CALL_VA);
                    debug_assert_eq!(next.primitive_va, GAME_LOG_SAY_CHECKSUM_VA);
                    let game_log_say_checksum =
                        execute_map_make_post_fix_diag_game_log_say_checksum(
                            world,
                            regions,
                            rng.state(),
                            &regions_clear_all,
                            &regions_find_all,
                            &territory_limits,
                            &fix_diag_land,
                            &post_fix_diag_string_constructor,
                        )
                        .map_err(ContinentError::PostFixDiagGameLog)?;
                    let MapMakePostFixDiagGameLogNext::StringClose {
                        call_va,
                        primitive_va: string_close_primitive_va,
                        ..
                    } = game_log_say_checksum.next;
                    debug_assert_eq!(
                        call_va,
                        crate::post_continent::MAP_MAKE_POST_CHECKSUM_STRING_CLOSE_CALL_VA
                    );
                    debug_assert_eq!(string_close_primitive_va, STRING_CLOSE_VA);
                    let post_checksum_string_close = execute_map_make_post_checksum_string_close(
                        world,
                        regions,
                        rng.state(),
                        &regions_clear_all,
                        &regions_find_all,
                        &territory_limits,
                        &fix_diag_land,
                        &post_fix_diag_string_constructor,
                        &game_log_say_checksum,
                        MapMakeStringAllocatorFacts::RETAIL_GAMEPLAY,
                    )
                    .map_err(ContinentError::PostChecksumStringClose)?;
                    let MapMakePostChecksumStringCloseNext::ProgressStringConstructor {
                        call_va: next_va,
                        primitive_va: next_mutator_va,
                        ..
                    } = post_checksum_string_close.next;
                    let body_receipt = player_land.body_receipt.clone();
                    (
                        ContinentStop::AddStartingLocation {
                            primitive_va,
                            caller_va,
                            next_va,
                            centroids,
                            edge_canals,
                            call: place_start,
                            selector,
                            mutation,
                            remaining,
                            player_land,
                            post_player_land_cleanup,
                            centroid_y_free_cleanup,
                            centroid_x_free_cleanup,
                            regions_clear_all,
                            regions_find_all,
                            territory_limits,
                            fix_diag_land,
                            post_fix_diag_string_constructor,
                            game_log_say_checksum,
                            post_checksum_string_close,
                            next_mutator_va,
                        },
                        starts_added,
                        Some(body_receipt),
                    )
                }
                EastMeetsWestRemainingStartsNext::CallerFallback { next_va, .. } => (
                    ContinentStop::EastMeetsWestStartFallback {
                        next_va,
                        centroids,
                        edge_canals,
                        call: place_start,
                        selector,
                        first_mutation: Some(mutation),
                        remaining: Some(remaining),
                    },
                    starts_added,
                    None,
                ),
            }
        }
        PlaceStartSelectorNext::CallerFallback { next_va } => (
            ContinentStop::EastMeetsWestStartFallback {
                next_va,
                centroids,
                edge_canals,
                call: place_start,
                selector,
                first_mutation: None,
                remaining: None,
            },
            0,
            None,
        ),
    };

    Ok(PartialReceipt {
        world_inverted: false,
        regions_cleared: 3,
        region_seeds,
        region_growths,
        grow_valid_calls: Vec::new(),
        lake_candidates: Vec::new(),
        pool_eliminations: vec![pools],
        player_land: player_land_body,
        team_partition: Some(partition),
        east_indies_tail: None,
        starts_added,
        start_min: None,
        stop,
    })
}

fn draw(rng: &mut Random, sites: &mut Vec<u32>, site: u32) -> i32 {
    sites.push(site);
    rng.get(0, 0xffff)
}

fn project(cx: i32, cy: i32, angle: i32, radius: i32) -> (i32, i32) {
    (
        cx.wrapping_add(sinx(angle, radius)),
        cy.wrapping_sub(cosx(angle, radius)),
    )
}

/// `Map::invert_land` `0x0069d880`, including the overlapping 16-bit writes.
fn invert_land(world: &mut World) {
    for cell in &mut world.wdata {
        if cell.flags & wflag::WATERHALF == 0
            && (cell.land == land::COASTAL || cell.land == land::OCEAN)
        {
            cell.land = land::FERTILE;
        } else {
            cell.land = land::OCEAN;
        }
        cell.land_sub = 0;
        cell.region = 0;
        // The 16-bit store at WData+4 does not touch WATERHALF's region2 at
        // +6. Regions::clear_all has the same deliberate asymmetry.
    }
}

/// Complete `Map::make_region` (`0x0069d3f0`). The final `area` argument is
/// diagnostic/caller state only; the shipped body seeds exactly one cell and
/// one coordinate, while the following `grow_region` consumes the target.
fn apply_make_region(
    world: &mut World,
    regions: &mut Regions,
    call: &RegionSeedCall,
    defaults: RegionSeedDefaults,
) -> Result<RegionSeedReceipt, ContinentError> {
    let Ok(region_index) = usize::try_from(call.region) else {
        return Err(ContinentError::InvalidRegionSeed {
            region: call.region,
            x: call.x,
            y: call.y,
        });
    };
    if region_index >= regions.list.len() || !world.valid_w(call.x, call.y) {
        return Err(ContinentError::InvalidRegionSeed {
            region: call.region,
            x: call.x,
            y: call.y,
        });
    }

    let region = &mut regions.list[region_index];
    let capacity_before = region.coords.capacity;
    let length = region.coords.items.len();
    if capacity_before < 0 || length > i32::MAX as usize {
        return Err(ContinentError::InvalidRegionCoordStorage {
            region: call.region,
            length,
            capacity: capacity_before,
            increment: region.coords.increment,
        });
    }
    if length as i32 >= region.coords.capacity {
        let increase = if region.coords.increment < 0 {
            region.coords.capacity.max(4)
        } else {
            i32::from(region.coords.increment)
        };
        let Some(next_capacity) = region.coords.capacity.checked_add(increase) else {
            return Err(ContinentError::InvalidRegionCoordStorage {
                region: call.region,
                length,
                capacity: capacity_before,
                increment: region.coords.increment,
            });
        };
        if increase <= 0 || next_capacity <= length as i32 {
            return Err(ContinentError::InvalidRegionCoordStorage {
                region: call.region,
                length,
                capacity: capacity_before,
                increment: region.coords.increment,
            });
        }
        region.coords.capacity = next_capacity;
    }

    let cell = world.wdata_mut(call.x, call.y);
    cell.land = land::FERTILE;
    cell.land_sub = 0;
    cell.region = call.region as i16;
    region.coords.items.push((call.x, call.y));
    region.size = 1;
    region.flags = defaults.flags;
    region.common_factor = defaults.common_factor;
    region.goody_factor = defaults.goody_factor;
    regions.land = regions.land.max(call.region);

    Ok(RegionSeedReceipt {
        call: call.clone(),
        coord_capacity_before: capacity_before,
        coord_capacity_after: region.coords.capacity,
        common_factor: region.common_factor,
        goody_factor: region.goody_factor,
        flags: region.flags,
    })
}

fn map_int(
    style: &MapStyleStaticData,
    tag: &'static str,
    attribute: &'static str,
) -> Result<i32, ContinentError> {
    let entry = find_entry(&style.selected_map_entries, tag)
        .or_else(|| find_entry(&style.default_map_entries, tag))
        .ok_or(ContinentError::MissingMapParameter { tag, attribute })?;
    let raw = entry
        .attribute(attribute)
        .ok_or(ContinentError::MissingMapParameter { tag, attribute })?;
    // Plain integer parameters are sufficient for every value consumed before
    // the currently represented stops. SCALE expressions are rejected rather
    // than guessed; styles whose early calls do not consume them need not parse
    // them here.
    raw.trim()
        .parse::<i32>()
        .map_err(|_| ContinentError::InvalidMapParameter {
            tag,
            attribute,
            value: raw.to_owned(),
        })
}

fn map_growth_config(
    style: &MapStyleStaticData,
    world: &World,
    players: u8,
    map_size: u8,
    orientation: i32,
) -> Result<MapGrowthConfig, ContinentError> {
    let avoid_center = map_scaled_int(style, "AVOID_CENTER", "scalevalue", world)?;
    let avoid_continent = scale_land_area(
        map_scaled_int(style, "AVOID_CONTINENT", "scalevalue", world)?,
        players,
        map_size,
    )
    .max(4);
    let mut edge_avoid = [0; 4];
    for (index, value) in edge_avoid.iter_mut().enumerate() {
        let tag = match index {
            0 => "AVOID_EDGE_0",
            1 => "AVOID_EDGE_1",
            2 => "AVOID_EDGE_2",
            3 => "AVOID_EDGE_3",
            _ => unreachable!(),
        };
        *value = map_scaled_int(style, tag, "scalevalue", world)?;
    }
    Ok(MapGrowthConfig {
        avoid_center,
        avoid_continent,
        edge_avoid,
        base_edge: orientation,
        edge_jitter: 0,
    })
}

fn map_scaled_int(
    style: &MapStyleStaticData,
    tag: &'static str,
    attribute: &'static str,
    world: &World,
) -> Result<i32, ContinentError> {
    let entry = find_entry(&style.selected_map_entries, tag)
        .or_else(|| find_entry(&style.default_map_entries, tag))
        .ok_or(ContinentError::MissingMapParameter { tag, attribute })?;
    let raw = entry
        .attribute(attribute)
        .ok_or(ContinentError::MissingMapParameter { tag, attribute })?;
    let fields = raw.split_whitespace().collect::<Vec<_>>();
    let value = fields
        .first()
        .and_then(|field| field.parse::<i32>().ok())
        .ok_or_else(|| ContinentError::InvalidMapParameter {
            tag,
            attribute,
            value: raw.to_owned(),
        })?;
    match fields.as_slice() {
        [_] => Ok(value),
        [_, "SCALE"] => Ok(scale_by_axis(world, value)),
        [_, "AREA"] => Ok(scale_by_area(world, value)),
        _ => Err(ContinentError::InvalidMapParameter {
            tag,
            attribute,
            value: raw.to_owned(),
        }),
    }
}

fn scale_by_axis(world: &World, value: i32) -> i32 {
    if value < 1 {
        0
    } else {
        ((STANDARD_MAP_EDGE / 2).wrapping_add(world.xs.wrapping_mul(value)) / STANDARD_MAP_EDGE)
            .max(1)
    }
}

fn scale_by_area(world: &World, value: i32) -> i32 {
    if value < 1 {
        0
    } else {
        let standard_area = STANDARD_MAP_EDGE * STANDARD_MAP_EDGE;
        ((standard_area / 2).wrapping_add(world.size.wrapping_mul(value)) / standard_area).max(1)
    }
}

const STANDARD_MAP_EDGE: i32 = 70;

fn find_entry<'a>(entries: &'a [StaticXmlEntry], tag: &str) -> Option<&'a StaticXmlEntry> {
    entries.iter().find(|entry| entry.tag == tag)
}

fn player_baseline(map_size: u8) -> i32 {
    match map_size {
        0 | 1 => 2,
        2 => 3,
        3 => 4,
        4 => 6,
        _ => 8,
    }
}

/// `Map::scale_land_area` `0x0068b1e0` / `get_player_overage` `0x0068b280`.
fn scale_land_area(value: i32, players: u8, map_size: u8) -> i32 {
    let baseline = player_baseline(map_size);
    let overage = if players == 0 {
        baseline
    } else {
        (players as i32 - baseline).max(0)
    };
    if overage == 0 {
        return value;
    }
    let ratio = overage as f32 / baseline as f32;
    (value as f32 / ratio) as i32
}

fn integer_sqrt_floor(value: i32) -> i32 {
    if value <= 1 {
        return value.max(0);
    }
    let mut current = value >> 1;
    loop {
        let next = (current + value / current) >> 1;
        if next >= current {
            return current;
        }
        current = next;
    }
}
