//! Exact East Meets West caller transaction for `Map::check_player_land`.
//!
//! This module begins at the native loop-exit edge after every active player
//! has a starting location.  The 1,145-byte leaf itself is owned by
//! [`crate::player_land`]; this layer fixes the style's fastcall arguments,
//! proves that the leaf consumes no RNG, records the only legal World/Regions
//! mutations, and leaves a typed post-call cleanup residual.

use crate::player_land::{
    execute_check_player_land, CheckPlayerLandCall, CheckPlayerLandError, CheckPlayerLandReceipt,
    MAP_CHECK_PLAYER_LAND_VA,
};
use don_sim::systems::map_terrain::{WData, World, WorldChecksum, WorldSection};
use don_sim::systems::regions::{Region, Regions, REGION_COUNT};

pub const EAST_MEETS_WEST_PLAYER_LAND_CALLER_ENTRY_VA: u32 = 0x0069_7484;
pub const EAST_MEETS_WEST_PLAYER_LAND_CALL_VA: u32 = 0x0069_7492;
pub const EAST_MEETS_WEST_PLAYER_LAND_RESUME_VA: u32 = 0x0069_7497;
pub const EAST_MEETS_WEST_PLAYER_LAND_GUARD_STORE_VA: u32 = 0x0069_749a;
pub const EAST_MEETS_WEST_PLAYER_LAND_STRING_CLOSE_CALL_VA: u32 = 0x0069_74a1;
pub const STRING_CLOSE_VA: u32 = 0x00a1_cf40;
pub const STRING_CLOSE_END_VA: u32 = 0x00a1_cf8f;
pub const STRING_CLOSE_RET_VA: u32 = 0x00a1_cf8e;
pub const STRING_CLOSE_SIZE: u32 = 79;
pub const STRING_CLOSE_INSTRUCTION_COUNT: u32 = 36;
pub const STRING_CLOSE_SHA256: &str =
    "80b55b224beca72346bcb001fb415310611060b52a0c49c8db67c66c71fbae7f";
pub const STRING_GUTS_DESTRUCTOR_CALL_VA: u32 = 0x00a1_cf71;
pub const STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_VA: u32 = 0x004d_3e90;

pub const EAST_MEETS_WEST_POST_PLAYER_LAND_CLEANUP_END_VA: u32 = 0x0069_74c2;
pub const EAST_MEETS_WEST_POST_PLAYER_LAND_CLEANUP_SIZE: u32 = 43;
pub const EAST_MEETS_WEST_POST_PLAYER_LAND_CLEANUP_INSTRUCTION_COUNT: u32 = 11;
pub const EAST_MEETS_WEST_POST_PLAYER_LAND_CLEANUP_SHA256: &str =
    "7d4bc060ac077efe0ba99a2900ff83acf9ab8f8887cccdfe5078b94ee17dc987";
pub const EAST_MEETS_WEST_PLAYER_LAND_GUARD_CLEAR_VA: u32 = 0x0069_74a6;
pub const EAST_MEETS_WEST_CENTROID_Y_LIST_LOAD_VA: u32 = 0x0069_74aa;
pub const EAST_MEETS_WEST_FREE_IMPORT_LOAD_VA: u32 = 0x0069_74ad;
pub const EAST_MEETS_WEST_CENTROID_Y_VFTABLE_STORE_VA: u32 = 0x0069_74b3;
pub const EAST_MEETS_WEST_CENTROID_Y_LIST_TEST_VA: u32 = 0x0069_74bd;
pub const EAST_MEETS_WEST_CENTROID_Y_LIST_PUSH_VA: u32 = 0x0069_74c1;
pub const EAST_MEETS_WEST_CENTROID_Y_FREE_CALL_VA: u32 = 0x0069_74c2;
pub const FREE_IMPORT_IAT_VA: u32 = 0x00ac_5500;
pub const SIMPLE_ARRAY_INT_VFTABLE_VA: u32 = 0x00b2_2a60;
pub const SIMPLE_ARRAY_INT_SIZE: u32 = 28;

pub const EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_END_VA: u32 = 0x0069_7502;
pub const EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_SIZE: u32 = 64;
pub const EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_INSTRUCTION_COUNT: u32 = 12;
pub const EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_SHA256: &str =
    "158e1582fb9c93e1485c5929ac0a4a3888878bca2b8c3b6b7f5182a9854674b4";
pub const EAST_MEETS_WEST_CENTROID_Y_LIST_CLEAR_VA: u32 = 0x0069_74c7;
pub const EAST_MEETS_WEST_CENTROID_Y_SIZE_CLEAR_VA: u32 = 0x0069_74ce;
pub const EAST_MEETS_WEST_CENTROID_Y_LENGTH_CLEAR_VA: u32 = 0x0069_74d8;
pub const EAST_MEETS_WEST_CENTROID_Y_FLAGS_CLEAR_VA: u32 = 0x0069_74e2;
pub const EAST_MEETS_WEST_CENTROID_Y_GUARD_CLEAR_VA: u32 = 0x0069_74e6;
pub const EAST_MEETS_WEST_CENTROID_X_LIST_LOAD_VA: u32 = 0x0069_74ed;
pub const EAST_MEETS_WEST_CENTROID_X_VFTABLE_STORE_VA: u32 = 0x0069_74f3;
pub const EAST_MEETS_WEST_CENTROID_X_LIST_TEST_VA: u32 = 0x0069_74fd;
pub const EAST_MEETS_WEST_CENTROID_X_LIST_PUSH_VA: u32 = 0x0069_7501;
pub const EAST_MEETS_WEST_CENTROID_X_FREE_CALL_VA: u32 = 0x0069_7502;
pub const EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_END_VA: u32 = 0x0069_753f;
pub const EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_SIZE: u32 = 61;
pub const EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_INSTRUCTION_COUNT: u32 = 14;
pub const EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_SHA256: &str =
    "8dfe4067651d56aabcf8819c42d9a569df1ac1d28cef3efe954f245aa38d2535";
pub const EAST_MEETS_WEST_CENTROID_X_STACK_POP_VA: u32 = 0x0069_7504;
pub const EAST_MEETS_WEST_EXCEPTION_REGISTRATION_LOAD_VA: u32 = 0x0069_7507;
pub const EAST_MEETS_WEST_EDI_POP_VA: u32 = 0x0069_750a;
pub const EAST_MEETS_WEST_ESI_POP_VA: u32 = 0x0069_750b;
pub const EAST_MEETS_WEST_CENTROID_X_LIST_CLEAR_VA: u32 = 0x0069_750c;
pub const EAST_MEETS_WEST_CENTROID_X_SIZE_CLEAR_VA: u32 = 0x0069_7516;
pub const EAST_MEETS_WEST_CENTROID_X_LENGTH_CLEAR_VA: u32 = 0x0069_7520;
pub const EAST_MEETS_WEST_CENTROID_X_FLAGS_CLEAR_VA: u32 = 0x0069_752a;
pub const EAST_MEETS_WEST_EXCEPTION_REGISTRATION_RESTORE_VA: u32 = 0x0069_7531;
pub const EAST_MEETS_WEST_EBX_POP_VA: u32 = 0x0069_7538;
pub const EAST_MEETS_WEST_STACK_FRAME_RESTORE_VA: u32 = 0x0069_7539;
pub const EAST_MEETS_WEST_FRAME_POINTER_POP_VA: u32 = 0x0069_753b;
pub const EAST_MEETS_WEST_MAKE_CONTINENTS_RET_VA: u32 = 0x0069_753c;
pub const EAST_MEETS_WEST_MAKE_CONTINENTS_ENTRY_VA: u32 = 0x0069_6640;
pub const EAST_MEETS_WEST_MAKE_CONTINENTS_SIZE: u32 = 3_839;

pub const EAST_MEETS_WEST_LOG_STRING_ASSIGN_CALL_VA: u32 = 0x0069_67cc;
pub const STRING_ASSIGN_VA: u32 = 0x00a1_eeb0;
pub const INT_STR_ARRAY_VA: u32 = 0x00c0_6378;
pub const EAST_MEETS_WEST_LOG_STRING_BYTE_OFFSET: u32 = 0x0001_7660;
pub const STRING_SIZE: u32 = 20;
pub const EAST_MEETS_WEST_LOG_STRING_ORDINAL: usize =
    (EAST_MEETS_WEST_LOG_STRING_BYTE_OFFSET / STRING_SIZE) as usize;
pub const EAST_MEETS_WEST_LOG_STRING_HASH: i32 = 39_143_929;
pub const EAST_MEETS_WEST_LOG_STRING_UTF16_UNITS: usize = 26;
pub const EAST_MEETS_WEST_LOG_STRING: &str = "MapGen:EastMeetsWest enter";

pub const MAP_CHECK_PLAYER_LAND_END_VA: u32 = 0x0068_f379;
pub const MAP_CHECK_PLAYER_LAND_RET_VA: u32 = 0x0068_f378;
pub const MAP_CHECK_PLAYER_LAND_SIZE: u32 = 1_145;
pub const MAP_CHECK_PLAYER_LAND_INSTRUCTION_COUNT: u32 = 301;
pub const MAP_CHECK_PLAYER_LAND_SHA256: &str =
    "9577c20dbe3e6cd13409355c2f45983cbc01e573c30e3b0e16958967fb109140";
pub const RISE_EXE_SHA256: &str =
    "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079";

pub const ARRAY_WCOORD_REMOVE_CALL_VA: u32 = 0x0068_f089;
pub const ARRAY_WCOORD_REMOVE_VA: u32 = 0x0046_d4f0;
pub const SEED_ARRAY_GROW_INDIRECT_CALL_VA: u32 = 0x0068_f0cf;
pub const OUTER_ARRAY_GROW_INDIRECT_CALL_VA: u32 = 0x0068_f2ea;

/// Frozen instruction-level provenance for the complete native body.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CheckPlayerLandNativeBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub ret_va: u32,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
    pub direct_rng_sites: &'static [u32],
    pub direct_calls: &'static [(u32, u32)],
    pub indirect_array_grow_calls: &'static [u32],
}

pub const CHECK_PLAYER_LAND_NATIVE_BODY: CheckPlayerLandNativeBody = CheckPlayerLandNativeBody {
    entry_va: MAP_CHECK_PLAYER_LAND_VA,
    end_va_exclusive: MAP_CHECK_PLAYER_LAND_END_VA,
    ret_va: MAP_CHECK_PLAYER_LAND_RET_VA,
    size: MAP_CHECK_PLAYER_LAND_SIZE,
    instruction_count: MAP_CHECK_PLAYER_LAND_INSTRUCTION_COUNT,
    sha256: MAP_CHECK_PLAYER_LAND_SHA256,
    direct_rng_sites: &[],
    direct_calls: &[(ARRAY_WCOORD_REMOVE_CALL_VA, ARRAY_WCOORD_REMOVE_VA)],
    indirect_array_grow_calls: &[
        SEED_ARRAY_GROW_INDIRECT_CALL_VA,
        OUTER_ARRAY_GROW_INDIRECT_CALL_VA,
    ],
};

/// Frozen executable bytes for one exact native function or caller slice.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CleanupNativeBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub ret_va: Option<u32>,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
    pub direct_calls: &'static [(u32, u32)],
    pub indirect_import_calls: &'static [(u32, u32)],
}

pub const STRING_CLOSE_NATIVE_BODY: CleanupNativeBody = CleanupNativeBody {
    entry_va: STRING_CLOSE_VA,
    end_va_exclusive: STRING_CLOSE_END_VA,
    ret_va: Some(STRING_CLOSE_RET_VA),
    size: STRING_CLOSE_SIZE,
    instruction_count: STRING_CLOSE_INSTRUCTION_COUNT,
    sha256: STRING_CLOSE_SHA256,
    direct_calls: &[(
        STRING_GUTS_DESTRUCTOR_CALL_VA,
        STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_VA,
    )],
    indirect_import_calls: &[],
};

pub const EAST_MEETS_WEST_POST_PLAYER_LAND_CLEANUP_BODY: CleanupNativeBody = CleanupNativeBody {
    entry_va: EAST_MEETS_WEST_PLAYER_LAND_RESUME_VA,
    end_va_exclusive: EAST_MEETS_WEST_POST_PLAYER_LAND_CLEANUP_END_VA,
    ret_va: None,
    size: EAST_MEETS_WEST_POST_PLAYER_LAND_CLEANUP_SIZE,
    instruction_count: EAST_MEETS_WEST_POST_PLAYER_LAND_CLEANUP_INSTRUCTION_COUNT,
    sha256: EAST_MEETS_WEST_POST_PLAYER_LAND_CLEANUP_SHA256,
    direct_calls: &[(
        EAST_MEETS_WEST_PLAYER_LAND_STRING_CLOSE_CALL_VA,
        STRING_CLOSE_VA,
    )],
    indirect_import_calls: &[],
};

pub const EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_BODY: CleanupNativeBody = CleanupNativeBody {
    entry_va: EAST_MEETS_WEST_CENTROID_Y_FREE_CALL_VA,
    end_va_exclusive: EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_END_VA,
    ret_va: None,
    size: EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_SIZE,
    instruction_count: EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_INSTRUCTION_COUNT,
    sha256: EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_SHA256,
    direct_calls: &[],
    indirect_import_calls: &[(EAST_MEETS_WEST_CENTROID_Y_FREE_CALL_VA, FREE_IMPORT_IAT_VA)],
};

pub const EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_BODY: CleanupNativeBody = CleanupNativeBody {
    entry_va: EAST_MEETS_WEST_CENTROID_X_FREE_CALL_VA,
    end_va_exclusive: EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_END_VA,
    ret_va: Some(EAST_MEETS_WEST_MAKE_CONTINENTS_RET_VA),
    size: EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_SIZE,
    instruction_count: EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_INSTRUCTION_COUNT,
    sha256: EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_SHA256,
    direct_calls: &[],
    indirect_import_calls: &[(EAST_MEETS_WEST_CENTROID_X_FREE_CALL_VA, FREE_IMPORT_IAT_VA)],
};

/// Values live at the exact caller edge.  `unread_stack_word` is retained
/// because retail pushes ECX at `0x00697487`, although the leaf never reads
/// its fourth formal parameter.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct EastMeetsWestPlayerLandCall {
    pub expected_start_count: usize,
    pub avoid_continent: i32,
    pub unread_stack_word: i32,
    pub random_state: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionMutation {
    pub region: u8,
    pub before: Region,
    pub after: Region,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldCellMutation {
    pub x: i32,
    pub y: i32,
    pub before: WData,
    pub after: WData,
}

/// The native leaf is `void`; execution resumes in the style virtual and its
/// first still-unported work is local-string cleanup.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EastMeetsWestPlayerLandNext {
    PostCallCleanup {
        entry_va: u32,
        guard_store_va: u32,
        string_close_call_va: u32,
        string_close_va: u32,
    },
}

/// The installed internal-string table owns heap-backed `StringGuts`; the
/// assignment at `0x006967cc` adds one local reference.  Closing that local
/// therefore takes the decrement arm, never the deleting-destructor arm.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EastMeetsWestStringClosePath {
    SharedStringGutsReferenceDecrement,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct EastMeetsWestLogStringLease {
    pub table_va: u32,
    pub byte_offset: u32,
    pub ordinal: usize,
    pub declared_hash: i32,
    pub text: &'static str,
    pub utf16_units: usize,
    pub assignment_call_va: u32,
    pub assignment_va: u32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct EastMeetsWestStringCloseReceipt {
    pub call_va: u32,
    pub body: CleanupNativeBody,
    pub path: EastMeetsWestStringClosePath,
    pub source: EastMeetsWestLogStringLease,
    /// Net over assignment plus close; the table keeps its original owner.
    pub table_reference_delta_over_lease: i32,
    pub string_guts_destructor_called: bool,
    pub local_data_is_null: bool,
    pub local_offset: u16,
    pub local_length: u16,
    pub local_hash: u32,
    pub local_insensitive_hash: u32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EastMeetsWestPostPlayerLandCleanupNext {
    FreeCentroidYList { call_va: u32, import_iat_va: u32 },
}

/// Exact caller-local work after `Map::check_player_land` returns and before
/// the first heap mutation in the two `SimpleArray<int>` destructors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EastMeetsWestPostPlayerLandCleanupReceipt {
    pub body: CleanupNativeBody,
    pub stack_argument_pop_va: u32,
    pub stack_argument_bytes_popped: u8,
    pub guard_store_va: u32,
    pub guard_before_string_close: u8,
    pub string_close: EastMeetsWestStringCloseReceipt,
    pub guard_clear_va: u32,
    pub guard_after_string_close: u8,
    pub centroid_y_length: usize,
    pub centroid_y_list_non_null: bool,
    pub centroid_y_list_load_va: u32,
    pub free_import_load_va: u32,
    pub centroid_y_vftable_store_va: u32,
    pub centroid_y_vftable_va: u32,
    pub centroid_y_list_test_va: u32,
    pub centroid_y_list_push_va: u32,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub next: EastMeetsWestPostPlayerLandCleanupNext,
}

/// Logical owner of one retail centroid-list allocation.  This identity is
/// derived from the native array and its contents; no host address is stored
/// or compared.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EastMeetsWestCentroidListOwner {
    YCoordinates,
    XCoordinates,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RetailAllocationState {
    Live,
    Freed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EastMeetsWestCentroidListAllocation {
    pub owner: EastMeetsWestCentroidListOwner,
    pub element_width: u8,
    pub elements: Vec<i32>,
}

/// The allocator-visible side effect of the imported `_free` call.  Native
/// pointer identity is deliberately absent: the caller-owned array role and
/// exact values are sufficient to name the allocation transactionally.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EastMeetsWestImportedFreeReceipt {
    pub call_va: u32,
    pub import_iat_va: u32,
    pub allocation: EastMeetsWestCentroidListAllocation,
    pub state_before: RetailAllocationState,
    pub state_after: RetailAllocationState,
    pub native_return: (),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EastMeetsWestCentroidYFreeCleanupNext {
    FreeCentroidXList { call_va: u32, import_iat_va: u32 },
}

/// Exact caller work beginning with the centroid-Y `_free` and ending before
/// the centroid-X `_free` at `0x00697502`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EastMeetsWestCentroidYFreeCleanupReceipt {
    pub body: CleanupNativeBody,
    pub free: EastMeetsWestImportedFreeReceipt,
    pub stack_argument_bytes_popped: u8,
    pub list_clear_va: u32,
    pub size_clear_va: u32,
    pub length_clear_va: u32,
    pub flags_clear_va: u32,
    pub local_list_is_null: bool,
    pub local_size: i32,
    pub local_length: i32,
    pub local_flags: u8,
    pub guard_clear_va: u32,
    pub cleanup_guard_after: i32,
    pub centroid_x_length: usize,
    pub centroid_x_list_non_null: bool,
    pub centroid_x_list_load_va: u32,
    pub centroid_x_vftable_store_va: u32,
    pub centroid_x_vftable_va: u32,
    pub centroid_x_list_test_va: u32,
    pub centroid_x_list_push_va: u32,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub next: EastMeetsWestCentroidYFreeCleanupNext,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EastMeetsWestCentroidXFreeCleanupNext {
    ReturnedFromMakeContinents {
        ret_va: u32,
        callee_stack_argument_bytes_popped: u8,
    },
}

/// Exact final caller tranche: centroid-X `_free`, the remaining inlined
/// `SimpleArray<int>` destructor stores, SEH/frame restoration, and the
/// `MapEastMeetsWest::make_continents(int)` return.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EastMeetsWestCentroidXFreeCleanupReceipt {
    pub body: CleanupNativeBody,
    pub free: EastMeetsWestImportedFreeReceipt,
    pub stack_argument_pop_va: u32,
    pub stack_argument_bytes_popped: u8,
    pub exception_registration_load_va: u32,
    pub callee_saved_register_pop_vas: [u32; 3],
    pub list_clear_va: u32,
    pub size_clear_va: u32,
    pub length_clear_va: u32,
    pub flags_clear_va: u32,
    pub local_list_is_null: bool,
    pub local_size: i32,
    pub local_length: i32,
    pub local_flags: u8,
    pub exception_registration_restore_va: u32,
    pub exception_registration_restored: bool,
    pub stack_frame_restore_va: u32,
    pub frame_pointer_pop_va: u32,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub next: EastMeetsWestCentroidXFreeCleanupNext,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EastMeetsWestPlayerLandReceipt {
    pub caller_entry_va: u32,
    pub call_va: u32,
    pub body: CheckPlayerLandNativeBody,
    pub native_call: CheckPlayerLandCall,
    pub native_return: (),
    pub body_receipt: CheckPlayerLandReceipt,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub direct_rng_sites: Vec<u32>,
    pub world_before: WorldChecksum,
    pub world_after: WorldChecksum,
    pub world_sections_changed: Vec<WorldSection>,
    pub start_arrays_checksum_before: u32,
    pub start_arrays_checksum_after: u32,
    pub world_cell_mutations: Vec<WorldCellMutation>,
    pub region_mutations: Vec<RegionMutation>,
    pub next: EastMeetsWestPlayerLandNext,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EastMeetsWestPlayerLandError {
    StartArrayShape {
        expected_starts: usize,
        start_x: usize,
        start_y: usize,
        start_city_x: usize,
        start_city_y: usize,
    },
    PlayerLand(CheckPlayerLandError),
    UnexpectedWorldMutation {
        sections: Vec<WorldSection>,
    },
    UnexpectedRegionsContainerMutation,
    PostCallResidualMismatch,
    EmptyCentroidYArray,
    EmptyCentroidXArray,
    PostStringCleanupResidualMismatch,
    CentroidYLengthMismatch {
        cleanup: usize,
        allocation: usize,
    },
    PostCentroidYCleanupResidualMismatch,
    CentroidXLengthMismatch {
        cleanup: usize,
        allocation: usize,
    },
}

/// Execute `0x00697497..0x006974c2` and freeze before the conditional imported
/// `free`.  The centroid loop has already appended one X per continent, so its
/// first native `SimpleArray<int>` necessarily owns a non-null backing list.
pub fn execute_east_meets_west_post_player_land_cleanup(
    player_land: &EastMeetsWestPlayerLandReceipt,
    centroid_y: &[i32],
) -> Result<EastMeetsWestPostPlayerLandCleanupReceipt, EastMeetsWestPlayerLandError> {
    if player_land.next
        != (EastMeetsWestPlayerLandNext::PostCallCleanup {
            entry_va: EAST_MEETS_WEST_PLAYER_LAND_RESUME_VA,
            guard_store_va: EAST_MEETS_WEST_PLAYER_LAND_GUARD_STORE_VA,
            string_close_call_va: EAST_MEETS_WEST_PLAYER_LAND_STRING_CLOSE_CALL_VA,
            string_close_va: STRING_CLOSE_VA,
        })
    {
        return Err(EastMeetsWestPlayerLandError::PostCallResidualMismatch);
    }
    if centroid_y.is_empty() {
        return Err(EastMeetsWestPlayerLandError::EmptyCentroidYArray);
    }

    Ok(EastMeetsWestPostPlayerLandCleanupReceipt {
        body: EAST_MEETS_WEST_POST_PLAYER_LAND_CLEANUP_BODY,
        stack_argument_pop_va: EAST_MEETS_WEST_PLAYER_LAND_RESUME_VA,
        stack_argument_bytes_popped: 8,
        guard_store_va: EAST_MEETS_WEST_PLAYER_LAND_GUARD_STORE_VA,
        guard_before_string_close: 1,
        string_close: EastMeetsWestStringCloseReceipt {
            call_va: EAST_MEETS_WEST_PLAYER_LAND_STRING_CLOSE_CALL_VA,
            body: STRING_CLOSE_NATIVE_BODY,
            path: EastMeetsWestStringClosePath::SharedStringGutsReferenceDecrement,
            source: EastMeetsWestLogStringLease {
                table_va: INT_STR_ARRAY_VA,
                byte_offset: EAST_MEETS_WEST_LOG_STRING_BYTE_OFFSET,
                ordinal: EAST_MEETS_WEST_LOG_STRING_ORDINAL,
                declared_hash: EAST_MEETS_WEST_LOG_STRING_HASH,
                text: EAST_MEETS_WEST_LOG_STRING,
                utf16_units: EAST_MEETS_WEST_LOG_STRING_UTF16_UNITS,
                assignment_call_va: EAST_MEETS_WEST_LOG_STRING_ASSIGN_CALL_VA,
                assignment_va: STRING_ASSIGN_VA,
            },
            table_reference_delta_over_lease: 0,
            string_guts_destructor_called: false,
            local_data_is_null: true,
            local_offset: 0,
            local_length: 0,
            local_hash: 0,
            local_insensitive_hash: 0,
        },
        guard_clear_va: EAST_MEETS_WEST_PLAYER_LAND_GUARD_CLEAR_VA,
        guard_after_string_close: 0,
        centroid_y_length: centroid_y.len(),
        centroid_y_list_non_null: true,
        centroid_y_list_load_va: EAST_MEETS_WEST_CENTROID_Y_LIST_LOAD_VA,
        free_import_load_va: EAST_MEETS_WEST_FREE_IMPORT_LOAD_VA,
        centroid_y_vftable_store_va: EAST_MEETS_WEST_CENTROID_Y_VFTABLE_STORE_VA,
        centroid_y_vftable_va: SIMPLE_ARRAY_INT_VFTABLE_VA,
        centroid_y_list_test_va: EAST_MEETS_WEST_CENTROID_Y_LIST_TEST_VA,
        centroid_y_list_push_va: EAST_MEETS_WEST_CENTROID_Y_LIST_PUSH_VA,
        random_state_before: player_land.random_state_after,
        random_state_after: player_land.random_state_after,
        next: EastMeetsWestPostPlayerLandCleanupNext::FreeCentroidYList {
            call_va: EAST_MEETS_WEST_CENTROID_Y_FREE_CALL_VA,
            import_iat_va: FREE_IMPORT_IAT_VA,
        },
    })
}

/// Execute `0x006974c2..0x00697502`: release the caller-owned centroid-Y
/// allocation, zero the fields `SimpleArray<int>::~SimpleArray<int>` clears,
/// dismiss its EH cleanup guard, and prepare the non-null centroid-X argument.
/// The second imported `_free` remains unexecuted.
pub fn execute_east_meets_west_centroid_y_free_cleanup(
    cleanup: &EastMeetsWestPostPlayerLandCleanupReceipt,
    centroid_y: &[i32],
    centroid_x: &[i32],
) -> Result<EastMeetsWestCentroidYFreeCleanupReceipt, EastMeetsWestPlayerLandError> {
    if cleanup.next
        != (EastMeetsWestPostPlayerLandCleanupNext::FreeCentroidYList {
            call_va: EAST_MEETS_WEST_CENTROID_Y_FREE_CALL_VA,
            import_iat_va: FREE_IMPORT_IAT_VA,
        })
        || !cleanup.centroid_y_list_non_null
    {
        return Err(EastMeetsWestPlayerLandError::PostStringCleanupResidualMismatch);
    }
    if centroid_y.is_empty() {
        return Err(EastMeetsWestPlayerLandError::EmptyCentroidYArray);
    }
    if cleanup.centroid_y_length != centroid_y.len() {
        return Err(EastMeetsWestPlayerLandError::CentroidYLengthMismatch {
            cleanup: cleanup.centroid_y_length,
            allocation: centroid_y.len(),
        });
    }
    if centroid_x.is_empty() {
        return Err(EastMeetsWestPlayerLandError::EmptyCentroidXArray);
    }

    Ok(EastMeetsWestCentroidYFreeCleanupReceipt {
        body: EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_BODY,
        free: EastMeetsWestImportedFreeReceipt {
            call_va: EAST_MEETS_WEST_CENTROID_Y_FREE_CALL_VA,
            import_iat_va: FREE_IMPORT_IAT_VA,
            allocation: EastMeetsWestCentroidListAllocation {
                owner: EastMeetsWestCentroidListOwner::YCoordinates,
                element_width: 4,
                elements: centroid_y.to_vec(),
            },
            state_before: RetailAllocationState::Live,
            state_after: RetailAllocationState::Freed,
            native_return: (),
        },
        stack_argument_bytes_popped: 4,
        list_clear_va: EAST_MEETS_WEST_CENTROID_Y_LIST_CLEAR_VA,
        size_clear_va: EAST_MEETS_WEST_CENTROID_Y_SIZE_CLEAR_VA,
        length_clear_va: EAST_MEETS_WEST_CENTROID_Y_LENGTH_CLEAR_VA,
        flags_clear_va: EAST_MEETS_WEST_CENTROID_Y_FLAGS_CLEAR_VA,
        local_list_is_null: true,
        local_size: 0,
        local_length: 0,
        local_flags: 0,
        guard_clear_va: EAST_MEETS_WEST_CENTROID_Y_GUARD_CLEAR_VA,
        cleanup_guard_after: -1,
        centroid_x_length: centroid_x.len(),
        centroid_x_list_non_null: true,
        centroid_x_list_load_va: EAST_MEETS_WEST_CENTROID_X_LIST_LOAD_VA,
        centroid_x_vftable_store_va: EAST_MEETS_WEST_CENTROID_X_VFTABLE_STORE_VA,
        centroid_x_vftable_va: SIMPLE_ARRAY_INT_VFTABLE_VA,
        centroid_x_list_test_va: EAST_MEETS_WEST_CENTROID_X_LIST_TEST_VA,
        centroid_x_list_push_va: EAST_MEETS_WEST_CENTROID_X_LIST_PUSH_VA,
        random_state_before: cleanup.random_state_after,
        random_state_after: cleanup.random_state_after,
        next: EastMeetsWestCentroidYFreeCleanupNext::FreeCentroidXList {
            call_va: EAST_MEETS_WEST_CENTROID_X_FREE_CALL_VA,
            import_iat_va: FREE_IMPORT_IAT_VA,
        },
    })
}

/// Execute `0x00697502..0x0069753f`, including the final imported `_free`,
/// every remaining local-array clear, the SEH/frame epilogue, and `ret 4`.
pub fn execute_east_meets_west_centroid_x_free_cleanup(
    cleanup: &EastMeetsWestCentroidYFreeCleanupReceipt,
    centroid_x: &[i32],
) -> Result<EastMeetsWestCentroidXFreeCleanupReceipt, EastMeetsWestPlayerLandError> {
    if cleanup.next
        != (EastMeetsWestCentroidYFreeCleanupNext::FreeCentroidXList {
            call_va: EAST_MEETS_WEST_CENTROID_X_FREE_CALL_VA,
            import_iat_va: FREE_IMPORT_IAT_VA,
        })
        || !cleanup.centroid_x_list_non_null
    {
        return Err(EastMeetsWestPlayerLandError::PostCentroidYCleanupResidualMismatch);
    }
    if centroid_x.is_empty() {
        return Err(EastMeetsWestPlayerLandError::EmptyCentroidXArray);
    }
    if cleanup.centroid_x_length != centroid_x.len() {
        return Err(EastMeetsWestPlayerLandError::CentroidXLengthMismatch {
            cleanup: cleanup.centroid_x_length,
            allocation: centroid_x.len(),
        });
    }

    Ok(EastMeetsWestCentroidXFreeCleanupReceipt {
        body: EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_BODY,
        free: EastMeetsWestImportedFreeReceipt {
            call_va: EAST_MEETS_WEST_CENTROID_X_FREE_CALL_VA,
            import_iat_va: FREE_IMPORT_IAT_VA,
            allocation: EastMeetsWestCentroidListAllocation {
                owner: EastMeetsWestCentroidListOwner::XCoordinates,
                element_width: 4,
                elements: centroid_x.to_vec(),
            },
            state_before: RetailAllocationState::Live,
            state_after: RetailAllocationState::Freed,
            native_return: (),
        },
        stack_argument_pop_va: EAST_MEETS_WEST_CENTROID_X_STACK_POP_VA,
        stack_argument_bytes_popped: 4,
        exception_registration_load_va: EAST_MEETS_WEST_EXCEPTION_REGISTRATION_LOAD_VA,
        callee_saved_register_pop_vas: [
            EAST_MEETS_WEST_EDI_POP_VA,
            EAST_MEETS_WEST_ESI_POP_VA,
            EAST_MEETS_WEST_EBX_POP_VA,
        ],
        list_clear_va: EAST_MEETS_WEST_CENTROID_X_LIST_CLEAR_VA,
        size_clear_va: EAST_MEETS_WEST_CENTROID_X_SIZE_CLEAR_VA,
        length_clear_va: EAST_MEETS_WEST_CENTROID_X_LENGTH_CLEAR_VA,
        flags_clear_va: EAST_MEETS_WEST_CENTROID_X_FLAGS_CLEAR_VA,
        local_list_is_null: true,
        local_size: 0,
        local_length: 0,
        local_flags: 0,
        exception_registration_restore_va: EAST_MEETS_WEST_EXCEPTION_REGISTRATION_RESTORE_VA,
        exception_registration_restored: true,
        stack_frame_restore_va: EAST_MEETS_WEST_STACK_FRAME_RESTORE_VA,
        frame_pointer_pop_va: EAST_MEETS_WEST_FRAME_POINTER_POP_VA,
        random_state_before: cleanup.random_state_after,
        random_state_after: cleanup.random_state_after,
        next: EastMeetsWestCentroidXFreeCleanupNext::ReturnedFromMakeContinents {
            ret_va: EAST_MEETS_WEST_MAKE_CONTINENTS_RET_VA,
            callee_stack_argument_bytes_popped: 4,
        },
    })
}

/// Execute the exact `0x00697484..0x00697497` East Meets West continuation.
///
/// The native call is `check_player_land(1, Map+0x30, -1, unread)`: `-1`
/// selects the leaf's radius-five default, while the exclusion radius is
/// capped to ten inside the leaf.  No RNG object is passed by native code, so
/// the supplied chronology word is copied through unchanged and the receipt's
/// direct-site list is necessarily empty.
pub fn execute_east_meets_west_player_land(
    world: &mut World,
    regions: &mut Regions,
    call: EastMeetsWestPlayerLandCall,
) -> Result<EastMeetsWestPlayerLandReceipt, EastMeetsWestPlayerLandError> {
    let starts = call.expected_start_count;
    if world.start_x.items.len() != starts
        || world.start_y.items.len() != starts
        || world.start_city_x.items.len() != starts.saturating_mul(4)
        || world.start_city_y.items.len() != starts.saturating_mul(4)
    {
        return Err(EastMeetsWestPlayerLandError::StartArrayShape {
            expected_starts: starts,
            start_x: world.start_x.items.len(),
            start_y: world.start_y.items.len(),
            start_city_x: world.start_city_x.items.len(),
            start_city_y: world.start_city_y.items.len(),
        });
    }

    let world_before = world.checksum_sections();
    let wdata_before = world.wdata.clone();
    let regions_before = regions.clone();
    let native_call = CheckPlayerLandCall {
        enabled: 1,
        avoid_continent: call.avoid_continent,
        radius: -1,
        unused: call.unread_stack_word,
    };
    let body_receipt = execute_check_player_land(world, regions, native_call)
        .map_err(EastMeetsWestPlayerLandError::PlayerLand)?;
    let world_after = world.checksum_sections();
    let world_sections_changed = world_before.differing_sections(&world_after);
    if world_sections_changed
        .iter()
        .any(|&section| section != WorldSection::WData)
    {
        return Err(EastMeetsWestPlayerLandError::UnexpectedWorldMutation {
            sections: world_sections_changed,
        });
    }
    if regions_before.sea != regions.sea
        || regions_before.land != regions.land
        || regions_before.coords != regions.coords
    {
        return Err(EastMeetsWestPlayerLandError::UnexpectedRegionsContainerMutation);
    }

    let region_mutations = (0..REGION_COUNT)
        .filter(|&index| regions_before.list[index] != regions.list[index])
        .map(|index| RegionMutation {
            region: index as u8,
            before: regions_before.list[index].clone(),
            after: regions.list[index].clone(),
        })
        .collect();
    let world_cell_mutations = wdata_before
        .into_iter()
        .zip(world.wdata.iter().cloned())
        .enumerate()
        .filter(|(_, (before, after))| before != after)
        .map(|(index, (before, after))| WorldCellMutation {
            x: index as i32 % world.xs,
            y: index as i32 / world.xs,
            before,
            after,
        })
        .collect();

    Ok(EastMeetsWestPlayerLandReceipt {
        caller_entry_va: EAST_MEETS_WEST_PLAYER_LAND_CALLER_ENTRY_VA,
        call_va: EAST_MEETS_WEST_PLAYER_LAND_CALL_VA,
        body: CHECK_PLAYER_LAND_NATIVE_BODY,
        native_call,
        native_return: (),
        body_receipt,
        random_state_before: call.random_state,
        random_state_after: call.random_state,
        direct_rng_sites: Vec::new(),
        start_arrays_checksum_before: world_before.section(WorldSection::StartArrays).adler,
        start_arrays_checksum_after: world_after.section(WorldSection::StartArrays).adler,
        world_cell_mutations,
        world_before,
        world_after,
        world_sections_changed,
        region_mutations,
        next: EastMeetsWestPlayerLandNext::PostCallCleanup {
            entry_va: EAST_MEETS_WEST_PLAYER_LAND_RESUME_VA,
            guard_store_va: EAST_MEETS_WEST_PLAYER_LAND_GUARD_STORE_VA,
            string_close_call_va: EAST_MEETS_WEST_PLAYER_LAND_STRING_CLOSE_CALL_VA,
            string_close_va: STRING_CLOSE_VA,
        },
    })
}
