//! Executable common `Map::make` tail through coastline reconstruction.
//!
//! Every supported style virtual returns into the same sequence:
//! `Regions::clear_all/find_all`, copy the six Map territory fields into
//! `World`, `Map::fix_diag_land`, `Map::make_coastlines`, then rebuild the
//! regions again.  The next call is `TerrainGroups::fill_fertile`.

use don_sim::systems::map_terrain::{land, wflag, WData, World, WorldChecksum, WorldSection};
use don_sim::systems::regions::{
    make_coastlines_and_rebuild_regions, Region, RegionBuildReceipt, Regions, RegionsError,
    WCoordList, REGION_COUNT,
};

pub const REGIONS_CLEAR_ALL_VA: u32 = 0x0068_0060;
pub const REGIONS_FIND_ALL_VA: u32 = 0x0067_eff0;
pub const REGIONS_FIND_ALL_END_VA: u32 = 0x0067_f7c9;
pub const REGIONS_FIND_ALL_SIZE: u32 = 2_009;
pub const REGIONS_FIND_ALL_INSTRUCTION_COUNT: u32 = 558;
pub const REGIONS_FIND_ALL_SHA256: &str =
    "3e356054293f63e1be4b36681473d000c1ec5bd6e4eb3fa368d8fce98b2cc1cd";
pub const REGIONS_FIND_ALL_RET_VA: u32 = 0x0067_f7c6;
pub const REGIONS_FIND_ALL_OLD_SCRATCH_FREE_CALL_VA: u32 = 0x0067_f042;
pub const REGIONS_FIND_ALL_SCRATCH_MALLOC_CALL_VA: u32 = 0x0067_f06b;
pub const REGIONS_FIND_ALL_STRING_CONSTRUCTOR_VA: u32 = 0x00a1_d660;
pub const REGIONS_FIND_ALL_ERROR_REPORT_VA: u32 = 0x00a2_e550;
pub const REGIONS_FIND_ALL_STRING_CLOSE_VA: u32 = 0x00a1_cf40;
pub const REGIONS_FIND_VA: u32 = 0x0068_0180;
pub const REGIONS_FIND_ALL_FIND_CALL_VA: u32 = 0x0067_f549;
pub const REGIONS_SET_COASTALS_VA: u32 = 0x0067_fd70;
pub const REGIONS_SORT_REGIONS_VA: u32 = 0x0067_fb90;
pub const REGIONS_REBUILD_COORDS_VA: u32 = 0x0067_f800;
pub const DO_ALL_NON_INPUT_VA: u32 = 0x0053_8810;
pub const REGIONS_FIND_ALL_SET_COASTALS_CALL_VA: u32 = 0x0067_f774;
pub const REGIONS_FIND_ALL_SORT_REGIONS_CALL_VA: u32 = 0x0067_f77b;
pub const REGIONS_FIND_ALL_FINAL_SCRATCH_FREE_CALL_VA: u32 = 0x0067_f788;
pub const REGIONS_FIND_ALL_REBUILD_COORDS_CALL_VA: u32 = 0x0067_f7ac;
pub const REGIONS_FIND_ALL_NON_INPUT_CALL_VA: u32 = 0x0067_f7b1;
pub const REGIONS_CLEAR_ALL_END_VA: u32 = 0x0068_0173;
pub const REGIONS_CLEAR_ALL_SIZE: u32 = 275;
pub const REGIONS_CLEAR_ALL_INSTRUCTION_COUNT: u32 = 79;
pub const REGIONS_CLEAR_ALL_SHA256: &str =
    "86333bac51dacb209b15048ecb33aa57b77c2d143a1ad797631e1992994b7317";
pub const REGIONS_CLEAR_ALL_RET_NONEMPTY_WORLD_VA: u32 = 0x0068_014e;
pub const REGIONS_CLEAR_ALL_RET_EMPTY_WORLD_VA: u32 = 0x0068_015e;
pub const REGIONS_CLEAR_ALL_RET_NULL_WORLD_DATA_VA: u32 = 0x0068_0172;
pub const REGIONS_CLEAR_ALL_COORD_FREE_CALL_VA: u32 = 0x0068_00c5;
pub const FREE_IMPORT_IAT_VA: u32 = 0x00ac_5500;
pub const MALLOC_IMPORT_IAT_VA: u32 = 0x00ac_54f0;
pub const MAP_MAKE_FIRST_REGIONS_CLEAR_CALL_VA: u32 = 0x0068_be36;
pub const MAP_MAKE_FIRST_REGIONS_CLEAR_RESUME_VA: u32 = 0x0068_be3b;
pub const MAP_MAKE_FIRST_REGIONS_FIND_ARGUMENT_PUSH_VA: u32 = 0x0068_be3b;
pub const MAP_MAKE_FIRST_REGIONS_FIND_CALL_VA: u32 = 0x0068_be3c;
pub const MAP_MAKE_FIRST_REGIONS_FIND_RESUME_VA: u32 = 0x0068_be41;
pub const MAP_MAKE_FIRST_REGIONS_FIND_CALLER_END_VA: u32 = MAP_MAKE_FIRST_REGIONS_FIND_RESUME_VA;
pub const MAP_MAKE_FIRST_REGIONS_FIND_CALLER_SIZE: u32 = 6;
pub const MAP_MAKE_FIRST_REGIONS_FIND_CALLER_INSTRUCTION_COUNT: u32 = 2;
pub const MAP_MAKE_FIRST_REGIONS_FIND_CALLER_SHA256: &str =
    "ff5c2df80ce0958c86143fb098cb30488633cc03971c09d64b1bf9f918a9b1da";
pub const MAP_MAKE_FIRST_TERRITORY_WORLD_LOAD_VA: u32 = 0x0068_be41;
pub const MAP_MAKE_FIRST_TERRITORY_MAP_LOAD_VA: u32 = 0x0068_be47;
pub const MAP_MAKE_FIRST_TERRITORY_STORE_VA: u32 = 0x0068_be4a;
pub const MAP_MAKE_FIRST_TERRITORY_PREP_END_VA: u32 = MAP_MAKE_FIRST_TERRITORY_STORE_VA;
pub const MAP_MAKE_FIRST_TERRITORY_PREP_SIZE: u32 = 9;
pub const MAP_MAKE_FIRST_TERRITORY_PREP_INSTRUCTION_COUNT: u32 = 2;
pub const MAP_MAKE_FIRST_TERRITORY_PREP_SHA256: &str =
    "cf0aa5dda08cbce5603defb8fddc99471fdbb7d6f4bd348812170223b59ea05f";
pub const MAP_PLAYER_TERRITORY_LIMIT_OFFSET: u32 = 0x4c;
pub const WORLD_PLAYER_TERRITORY_LIMIT_OFFSET: u32 = 0x38;
pub const MAP_MAKE_TERRITORY_LIMITS_END_VA: u32 = 0x0068_be75;
pub const MAP_MAKE_TERRITORY_LIMITS_SIZE: u32 = 43;
pub const MAP_MAKE_TERRITORY_LIMITS_INSTRUCTION_COUNT: u32 = 13;
pub const MAP_MAKE_TERRITORY_LIMITS_SHA256: &str =
    "9cd159ea34e9aeebc982230cc0a6234d628c2b4588e0e01261611d85bf0cb5fa";
pub const MAP_MAKE_TERRITORY_LIMIT_LOAD_VAS: [u32; 6] = [
    0x0068_be47,
    0x0068_be4d,
    0x0068_be53,
    0x0068_be59,
    0x0068_be5f,
    0x0068_be65,
];
pub const MAP_MAKE_TERRITORY_LIMIT_STORE_VAS: [u32; 6] = [
    0x0068_be4a,
    0x0068_be50,
    0x0068_be56,
    0x0068_be5c,
    0x0068_be62,
    0x0068_be68,
];
pub const MAP_TERRITORY_LIMIT_OFFSETS: [u32; 6] = [0x4c, 0x50, 0x54, 0x58, 0x5c, 0x60];
pub const WORLD_TERRITORY_LIMIT_OFFSETS: [u32; 6] = [0x38, 0x3c, 0x40, 0x44, 0x48, 0x4c];
pub const MAP_MAKE_STYLE_COMPARE_VA: u32 = 0x0068_be6b;
pub const MAP_MAKE_STYLE_BRANCH_VA: u32 = 0x0068_be6f;
pub const MAP_MAKE_STYLE_BRANCH_VALUE: u8 = 23;
pub const MAP_MAKE_STYLE_BRANCH_TARGET_VA: u32 = 0x0068_c84a;
pub const MAP_MAKE_FIRST_FIX_DIAG_LAND_CALL_VA: u32 = 0x0068_be75;
pub const MAP_FIX_DIAG_LAND_VA: u32 = 0x0069_c250;
pub const MAP_FIX_DIAG_LAND_END_VA: u32 = 0x0069_c458;
pub const MAP_FIX_DIAG_LAND_RET_VA: u32 = 0x0069_c457;
pub const MAP_FIX_DIAG_LAND_SIZE: u32 = 520;
pub const MAP_FIX_DIAG_LAND_INSTRUCTION_COUNT: u32 = 173;
pub const MAP_FIX_DIAG_LAND_SHA256: &str =
    "cf6610b0c5d5df3e5bfeb40010cdf729e587f69cdf1c3c3eaefd8c72c4fe65dd";
pub const MAP_FIX_DIAG_LAND_CORNER_X_VA: u32 = 0x00ad_c3c4;
pub const MAP_FIX_DIAG_LAND_CORNER_Y_VA: u32 = 0x00ad_c3e4;
pub const MAP_FIX_DIAG_LAND_CORNER_X: [i32; 4] = [-1, 1, 1, -1];
pub const MAP_FIX_DIAG_LAND_CORNER_Y: [i32; 4] = [-1, -1, 1, 1];
pub const MAP_MAKE_FIX_DIAG_LAND_RESUME_VA: u32 = 0x0068_be7a;
pub const MAP_MAKE_POST_FIX_DIAG_STRING_LITERAL_PUSH_VA: u32 = 0x0068_be7a;
pub const MAP_MAKE_POST_FIX_DIAG_STRING_LOCAL_LOAD_VA: u32 = 0x0068_be7f;
pub const MAP_MAKE_POST_FIX_DIAG_STRING_CONSTRUCTOR_CALL_VA: u32 = 0x0068_be82;
pub const STRING_CONSTRUCTOR_VA: u32 = 0x00a1_d660;
pub const STRING_CONSTRUCTOR_END_VA: u32 = 0x00a1_d681;
pub const STRING_CONSTRUCTOR_RET_VA: u32 = 0x00a1_d67e;
pub const STRING_CONSTRUCTOR_SIZE: u32 = 33;
pub const STRING_CONSTRUCTOR_INSTRUCTION_COUNT: u32 = 15;
pub const STRING_CONSTRUCTOR_SHA256: &str =
    "354be1ff3375e00afd53c7dd2ce92e7ebccba375f1ddc813fa9034dff6e629fe";
pub const STRING_INIT_CONST_VA: u32 = 0x00a1_6ff0;
pub const STRING_INIT_CONST_END_VA: u32 = 0x00a1_7067;
pub const STRING_INIT_CONST_SIZE: u32 = 119;
pub const STRING_INIT_CONST_INSTRUCTION_COUNT: u32 = 54;
pub const STRING_INIT_CONST_SHA256: &str =
    "9010bb01781bb0aa7455b513c941deb4f9e1e0be741a96e3ecc3cd7b56b4788d";
pub const STRING_REINIT_VA: u32 = 0x00a1_6120;
pub const STRING_REINIT_END_VA: u32 = 0x00a1_62af;
pub const STRING_REINIT_SIZE: u32 = 399;
pub const STRING_REINIT_INSTRUCTION_COUNT: u32 = 170;
pub const STRING_REINIT_SHA256: &str =
    "598c171ba48987ff15049dcd75b2cf76d942fd14e6cf0e601c32b5c499e1a00b";
pub const STRING_GET_STRING_GUTS_VA: u32 = 0x00a1_7b90;
pub const STRING_GET_STRING_GUTS_END_VA: u32 = 0x00a1_7c28;
pub const STRING_GET_STRING_GUTS_SIZE: u32 = 152;
pub const STRING_GET_STRING_GUTS_INSTRUCTION_COUNT: u32 = 52;
pub const STRING_GET_STRING_GUTS_SHA256: &str =
    "732194bd936cb0f2976f2e603851ad5cb2f0078ed6c4116319690215a07cd8ab";
pub const STRING_CHAR_TO_WCHAR_VA: u32 = 0x00a1_7c30;
pub const STRING_CHAR_TO_WCHAR_END_VA: u32 = 0x00a1_7c76;
pub const STRING_CHAR_TO_WCHAR_SIZE: u32 = 70;
pub const STRING_CHAR_TO_WCHAR_INSTRUCTION_COUNT: u32 = 32;
pub const STRING_CHAR_TO_WCHAR_SHA256: &str =
    "0a3dd4cd0a8a50ec36810f992681c933a85b8376b9d8c90bd96b3a2b13de167e";
pub const STRING_GUTS_OPERATOR_NEW_VA: u32 = 0x00a1_78f0;
pub const STRING_GUTS_OPERATOR_NEW_END_VA: u32 = 0x00a1_7997;
pub const STRING_GUTS_OPERATOR_NEW_SIZE: u32 = 167;
pub const STRING_GUTS_OPERATOR_NEW_INSTRUCTION_COUNT: u32 = 54;
pub const STRING_GUTS_OPERATOR_NEW_SHA256: &str =
    "3f434b4f68c546c6d3629fba4c35b2751ca4a2b03e27359a5df574e8b2d1ca56";
pub const STRING_GUTS_MEM_GET_VA: u32 = 0x00a1_7a10;
pub const STRING_GUTS_MEM_GET_END_VA: u32 = 0x00a1_7b84;
pub const STRING_GUTS_MEM_GET_SIZE: u32 = 372;
pub const STRING_GUTS_MEM_GET_INSTRUCTION_COUNT: u32 = 112;
pub const STRING_GUTS_MEM_GET_SHA256: &str =
    "1f2cc84918f8b89315969c39d49702222848595e1dcc3cec667e201244e0b024";
pub const STRING_GUTS_RESIZE_VA: u32 = 0x00a1_7520;
pub const STRING_CLOSE_VA: u32 = 0x00a1_cf40;
pub const MEMCPY_VA: u32 = 0x0055_e0ac;
pub const MULTI_BYTE_TO_WIDE_CHAR_IAT_VA: u32 = 0x00c8_dc44;
pub const MALLOC_IAT_VA: u32 = 0x00ac_54f0;
pub const MAP_MAKE_POST_FIX_DIAG_LITERAL_VA: u32 = 0x00ad_de58;
pub const MAP_MAKE_POST_FIX_DIAG_LITERAL: &str = "map.cpp";
pub const MAP_MAKE_POST_FIX_DIAG_LITERAL_BYTES_WITH_NUL: usize = 8;
pub const MAP_MAKE_POST_FIX_DIAG_LITERAL_SHA256: &str =
    "e53a11bee43ce20f44c4a24166157bbaa9c16f4389ce6ffac46db16832503b16";
pub const MAP_MAKE_POST_FIX_DIAG_CONSTRUCTOR_CALLER_END_VA: u32 = 0x0068_be9e;
pub const MAP_MAKE_POST_FIX_DIAG_CONSTRUCTOR_CALLER_SIZE: u32 = 28;
pub const MAP_MAKE_POST_FIX_DIAG_CONSTRUCTOR_CALLER_INSTRUCTION_COUNT: u32 = 7;
pub const MAP_MAKE_POST_FIX_DIAG_CONSTRUCTOR_CALLER_SHA256: &str =
    "98eba84d611b7e7c9ed1a9244a4b1f9712ba709e36426e8e0439020d9382497c";
pub const MAP_MAKE_POST_FIX_DIAG_GUARD_STORE_VA: u32 = 0x0068_be87;
pub const MAP_MAKE_POST_FIX_DIAG_GAME_LOG_SOURCE_LOAD_VA: u32 = 0x0068_be8e;
pub const MAP_MAKE_POST_FIX_DIAG_GAME_LOG_LINE_PUSH_VA: u32 = 0x0068_be91;
pub const MAP_MAKE_POST_FIX_DIAG_GAME_LOG_LINE_NUMBER: u32 = 0x1e9b;
pub const MAP_MAKE_POST_FIX_DIAG_GAME_LOG_SOURCE_PUSH_VA: u32 = 0x0068_be96;
pub const MAP_MAKE_POST_FIX_DIAG_GAME_LOG_MODE_PUSH_VA: u32 = 0x0068_be97;
pub const MAP_MAKE_POST_FIX_DIAG_GAME_LOG_MODE: i32 = 1;
pub const MAP_MAKE_POST_FIX_DIAG_GAME_LOG_THIS_LOAD_VA: u32 = 0x0068_be99;
pub const GAME_LOG_GLOBAL_VA: u32 = 0x00eb_1360;
pub const MAP_MAKE_POST_FIX_DIAG_GAME_LOG_CALL_VA: u32 = 0x0068_be9e;
pub const GAME_LOG_SAY_CHECKSUM_VA: u32 = 0x0093_0b30;
pub const GAME_LOG_SAY_CHECKSUM_END_VA: u32 = 0x0093_1ca8;
pub const GAME_LOG_SAY_CHECKSUM_RET_VA: u32 = 0x0093_1ca5;
pub const GAME_LOG_SAY_CHECKSUM_SIZE: u32 = 4_472;
pub const GAME_LOG_SAY_CHECKSUM_INSTRUCTION_COUNT: u32 = 1_379;
pub const GAME_LOG_SAY_CHECKSUM_SHA256: &str =
    "8e58ddabcd6742238aab95311529e9f615c5f322771f070c4805fbc48f2ef868";
pub const GAME_LOG_CHECK_ACCEPT_CALL_VA: u32 = 0x0093_0b6d;
pub const GAME_LOG_CHECK_ACCEPT_VA: u32 = 0x0093_09a0;
pub const GAME_LOG_CHECK_ACCEPT_END_VA: u32 = 0x0093_0b26;
pub const GAME_LOG_CHECK_ACCEPT_RET_VA: u32 = 0x0093_0b25;
pub const GAME_LOG_CHECK_ACCEPT_SIZE: u32 = 390;
pub const GAME_LOG_CHECK_ACCEPT_INSTRUCTION_COUNT: u32 = 98;
pub const GAME_LOG_CHECK_ACCEPT_SHA256: &str =
    "bdc002281b2d9ee6024de7fbdb549e7580b87f7a7d6a469f96d91171fffe51f6";
pub const GAME_LOG_REENTRANCY_GUARD_VA: u32 = 0x00ee_12c0;
pub const GAME_LOG_CATEGORY_OFFSET: u32 = 0x48;
pub const GAME_LOG_MODE_OFFSET: u32 = 0x4c;
pub const GAME_LOG_FRAME_CALLBACK_FLAG_OFFSET: u32 = 0x50;
pub const GAME_LOG_CHECKSUM_SEQUENCE_OFFSET: u32 = 0x68;
pub const GAME_LOG_ROLLOVER_SEQUENCE_OFFSET: u32 = 0x6c;
pub const GAME_LOG_BREAK_SEQUENCE_OFFSET: u32 = 0x70;
pub const GAME_LOG_CHECKSUM_CATEGORY: i32 = 0x1e;
pub const GAME_LOG_FIRST_VIRTUAL_SINK_CALL_VA: u32 = 0x0093_0c25;
pub const GAME_LOG_FRAME_ROLLOVER_CALL_VA: u32 = 0x0093_1c86;
pub const GAME_LOG_FRAME_ROLLOVER_VA: u32 = 0x0092_f2d0;
pub const MAP_MAKE_POST_CHECKSUM_GUARD_STORE_VA: u32 = 0x0068_bea3;
pub const MAP_MAKE_POST_CHECKSUM_STRING_LOCAL_LOAD_VA: u32 = 0x0068_beaa;
pub const MAP_MAKE_POST_CHECKSUM_STRING_CLOSE_CALL_VA: u32 = 0x0068_bead;
pub const MAP_MAKE_POST_CHECKSUM_CALLER_END_VA: u32 = MAP_MAKE_POST_CHECKSUM_STRING_CLOSE_CALL_VA;
pub const MAP_MAKE_POST_CHECKSUM_CALLER_SIZE: u32 = 15;
pub const MAP_MAKE_POST_CHECKSUM_CALLER_INSTRUCTION_COUNT: u32 = 3;
pub const MAP_MAKE_POST_CHECKSUM_CALLER_SHA256: &str =
    "3f130942dec6f49dc4774ad3eacbcee60a43d3181698848f155a44d835a1ace0";
pub const MAP_MAKE_POST_CHECKSUM_STRING_CLOSE_RESUME_VA: u32 = 0x0068_beb2;
pub const MAP_MAKE_POST_CHECKSUM_STRING_CLOSE_CALL_SIZE: u32 = 5;
pub const MAP_MAKE_POST_CHECKSUM_STRING_CLOSE_CALL_INSTRUCTION_COUNT: u32 = 1;
pub const MAP_MAKE_POST_CHECKSUM_STRING_CLOSE_CALL_SHA256: &str =
    "72e40e78c017521f98c87e6492bd9702157a03bd0114c8f55ee6b60c73122c48";
pub const STRING_CLOSE_END_VA: u32 = 0x00a1_cf8f;
pub const STRING_CLOSE_RET_VA: u32 = 0x00a1_cf8e;
pub const STRING_CLOSE_SIZE: u32 = 79;
pub const STRING_CLOSE_INSTRUCTION_COUNT: u32 = 36;
pub const STRING_CLOSE_SHA256: &str =
    "80b55b224beca72346bcb001fb415310611060b52a0c49c8db67c66c71fbae7f";
pub const STRING_CLOSE_GUTS_DESTRUCTOR_CALL_VA: u32 = 0x00a1_cf71;
pub const STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_VA: u32 = 0x004d_3e90;
pub const STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_END_VA: u32 = 0x004d_3eea;
pub const STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_RET_VA: u32 = 0x004d_3ee7;
pub const STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_SIZE: u32 = 90;
pub const STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_INSTRUCTION_COUNT: u32 = 31;
pub const STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_SHA256: &str =
    "ddcc915eb021bbab83cb63a23fca873d18a8a73e3f5d892c2c19b9b8d7340e47";
pub const STRING_GUTS_MEM_FREE_CALL_VA: u32 = 0x004d_3ec4;
pub const STRING_GUTS_MEM_FREE_VA: u32 = 0x00a1_79a0;
pub const STRING_GUTS_MEM_FREE_END_VA: u32 = 0x00a1_7a07;
pub const STRING_GUTS_MEM_FREE_RET_VA: u32 = 0x00a1_7a06;
pub const STRING_GUTS_MEM_FREE_SIZE: u32 = 103;
pub const STRING_GUTS_MEM_FREE_INSTRUCTION_COUNT: u32 = 40;
pub const STRING_GUTS_MEM_FREE_SHA256: &str =
    "10daec99334e08a0e9fc11d884a1a63407cd88f89102467fe45c864b62271361";
pub const STRING_GUTS_OPERATOR_DELETE_CALL_VA: u32 = 0x004d_3ed1;
pub const STRING_GUTS_OPERATOR_DELETE_VA: u32 = 0x00a1_77b0;
pub const STRING_GUTS_OPERATOR_DELETE_END_VA: u32 = 0x00a1_78e3;
pub const STRING_GUTS_OPERATOR_DELETE_RET_VAS: [u32; 2] = [0x00a1_78a8, 0x00a1_78e2];
pub const STRING_GUTS_OPERATOR_DELETE_SIZE: u32 = 307;
pub const STRING_GUTS_OPERATOR_DELETE_INSTRUCTION_COUNT: u32 = 93;
pub const STRING_GUTS_OPERATOR_DELETE_SHA256: &str =
    "f23feb8190572011d058ba1174def4969c3905ef7e21bd76c346c9abeb79f1d9";
pub const STRING_GUTS_BUFFER_POOL_CLASS: u8 = 1;
pub const STRING_GUTS_LOGICAL_SIZE: u8 = 16;
pub const MAP_MAKE_POST_CLOSE_PROGRESS_TEST_VA: u32 = 0x0068_beb2;
pub const MAP_MAKE_POST_CLOSE_PROGRESS_BRANCH_VA: u32 = 0x0068_beb4;
pub const MAP_MAKE_POST_CLOSE_NO_PROGRESS_TARGET_VA: u32 = 0x0068_bef2;
pub const MAP_MAKE_POST_CLOSE_PROGRESS_PREP_END_VA: u32 = 0x0068_bec4;
pub const MAP_MAKE_POST_CLOSE_PROGRESS_PREP_SIZE: u32 = 18;
pub const MAP_MAKE_POST_CLOSE_PROGRESS_PREP_INSTRUCTION_COUNT: u32 = 5;
pub const MAP_MAKE_POST_CLOSE_PROGRESS_PREP_SHA256: &str =
    "5287e0907afb9002eb66e3c49abcb1bcdfb8120294e860e019461ae6dc42133f";
pub const MAP_MAKE_PROGRESS_STRING_TABLE_LOAD_VA: u32 = 0x0068_beb6;
pub const MAP_MAKE_PROGRESS_STRING_TABLE_PTR_VA: u32 = 0x00c8_cd00;
pub const MAP_MAKE_PROGRESS_STRING_BYTE_OFFSET: u32 = 0x0000_ccec;
pub const MAP_MAKE_PROGRESS_STRING_LOCAL_LOAD_VA: u32 = 0x0068_bebb;
pub const MAP_MAKE_PROGRESS_STRING_SOURCE_PUSH_VA: u32 = 0x0068_bec3;
pub const MAP_MAKE_PROGRESS_STRING_CONSTRUCTOR_CALL_VA: u32 = 0x0068_bec4;
pub const STRING_COPY_CONSTRUCTOR_VA: u32 = 0x00a1_d590;
pub const STRING_COPY_CONSTRUCTOR_END_VA: u32 = 0x00a1_d658;
pub const STRING_COPY_CONSTRUCTOR_CONST_RET_VA: u32 = 0x00a1_d5e7;
pub const STRING_COPY_CONSTRUCTOR_SIZE: u32 = 200;
pub const STRING_COPY_CONSTRUCTOR_INSTRUCTION_COUNT: u32 = 73;
pub const STRING_COPY_CONSTRUCTOR_SHA256: &str =
    "b870ca7a19b559d52aead2ab867d2c6fc2aacd5cd0131ddbc29ea16b9455cf89";
pub const STRING_COPY_CONSTRUCTOR_SOURCE_FLAGS_TEST_VA: u32 = 0x00a1_d5af;
pub const STRING_COPY_CONSTRUCTOR_CONST_PATH_VA: u32 = 0x00a1_d5b5;
pub const MAP_MAKE_PROGRESS_CONSTRUCTOR_RESUME_VA: u32 = 0x0068_bec9;
pub const MAP_MAKE_PROGRESS_CONSTRUCTOR_CALL_SIZE: u32 = 5;
pub const MAP_MAKE_PROGRESS_CONSTRUCTOR_CALL_INSTRUCTION_COUNT: u32 = 1;
pub const MAP_MAKE_PROGRESS_CONSTRUCTOR_CALL_SHA256: &str =
    "e6f0feced96bc9e6894044d901a133a0fe187352d497b23bc71d0fc3498362b2";
pub const MAP_MAKE_PROGRESS_GUARD_STORE_VA: u32 = 0x0068_bec9;
pub const MAP_MAKE_PROGRESS_ASSIGN_SOURCE_LOAD_VA: u32 = 0x0068_bed0;
pub const MAP_MAKE_PROGRESS_ASSIGN_SOURCE_PUSH_VA: u32 = 0x0068_bed3;
pub const MAP_MAKE_PROGRESS_ASSIGN_THIS_LOAD_VA: u32 = 0x0068_bed4;
pub const MAP_MAKE_PROGRESS_ASSIGN_CALL_VA: u32 = 0x0068_bed9;
pub const MAP_MAKE_PROGRESS_POST_CONSTRUCTOR_PREP_SIZE: u32 = 16;
pub const MAP_MAKE_PROGRESS_POST_CONSTRUCTOR_PREP_INSTRUCTION_COUNT: u32 = 4;
pub const MAP_MAKE_PROGRESS_POST_CONSTRUCTOR_PREP_SHA256: &str =
    "74c88025da7529020be3e5d22fd85ea7f6e4c27072f513a1bd892b35429ecfb3";
pub const STRING_COPY_ASSIGN_VA: u32 = 0x00a1_eeb0;
pub const STRING_COPY_ASSIGN_END_VA: u32 = 0x00a1_f092;
pub const STRING_COPY_ASSIGN_CONST_RET_VA: u32 = 0x00a1_f040;
pub const STRING_COPY_ASSIGN_SIZE: u32 = 482;
pub const STRING_COPY_ASSIGN_INSTRUCTION_COUNT: u32 = 178;
pub const STRING_COPY_ASSIGN_SHA256: &str =
    "1545068535829488cb7a2b77fdaf0633ded575d90e4ee76ee216d7e8a1e76f5a";
pub const STRING_COPY_ASSIGN_SELF_TEST_VA: u32 = 0x00a1_eeba;
pub const STRING_COPY_ASSIGN_SOURCE_LENGTH_TEST_VA: u32 = 0x00a1_eec6;
pub const STRING_COPY_ASSIGN_NONEMPTY_SOURCE_VA: u32 = 0x00a1_ef49;
pub const STRING_COPY_ASSIGN_DEST_DATA_TEST_VA: u32 = 0x00a1_ef4b;
pub const STRING_COPY_ASSIGN_DEST_CONST_TEST_VA: u32 = 0x00a1_efb6;
pub const STRING_COPY_ASSIGN_REPLACE_VA: u32 = 0x00a1_effe;
pub const STRING_COPY_ASSIGN_DEST_CLOSE_CALL_VA: u32 = 0x00a1_f000;
pub const STRING_COPY_ASSIGN_SOURCE_CONST_TEST_VA: u32 = 0x00a1_f008;
pub const MAP_MAKE_PROGRESS_SPLASH_REFRESH_CALL_VA: u32 = 0x0068_bede;
pub const SPLASH_SCREEN_REFRESH_VA: u32 = 0x0083_8ce0;
pub const SPLASH_SCREEN_REFRESH_END_VA: u32 = 0x0083_8da7;
pub const SPLASH_SCREEN_REFRESH_RET_VA: u32 = 0x0083_8da6;
pub const SPLASH_SCREEN_REFRESH_SIZE: u32 = 199;
pub const SPLASH_SCREEN_REFRESH_INSTRUCTION_COUNT: u32 = 52;
pub const SPLASH_SCREEN_REFRESH_SHA256: &str =
    "c9e741eea1edf51ed91667fcd3d55edf1de92a5353c062f0dffffd62efde4907";
pub const MAP_MAKE_PROGRESS_LOCAL_GUARD_CLEAR_VA: u32 = 0x0068_bee3;
pub const MAP_MAKE_PROGRESS_LOCAL_CLOSE_LOAD_VA: u32 = 0x0068_beea;
pub const MAP_MAKE_PROGRESS_LOCAL_CLOSE_CALL_VA: u32 = 0x0068_beed;
pub const MAP_MAKE_PROGRESS_PRESENTATION_END_VA: u32 = 0x0068_bef2;
pub const MAP_MAKE_PROGRESS_PRESENTATION_SIZE: u32 = 25;
pub const MAP_MAKE_PROGRESS_PRESENTATION_INSTRUCTION_COUNT: u32 = 5;
pub const MAP_MAKE_PROGRESS_PRESENTATION_SHA256: &str =
    "8df9d8f37fc623d0b1ffc2b6876bbd861f7f75b6f81e49263a14e4a0ec596b99";
pub const MAP_MAKE_COASTLINES_CALL_VA: u32 = 0x0068_bef2;
pub const MAP_MAKE_COASTLINES_CALL_RESUME_VA: u32 = 0x0068_bef7;
pub const MAP_MAKE_COASTLINES_CALL_SIZE: u32 = 5;
pub const MAP_MAKE_COASTLINES_CALL_INSTRUCTION_COUNT: u32 = 1;
pub const MAP_MAKE_COASTLINES_CALL_SHA256: &str =
    "ec418cbf54e1f09a3f8e4b1811523000b5934f85e38ab1572407a2a494056212";
pub const LOCALIZED_STRING_TABLE_ENTRY_SIZE: u32 = 20;
pub const MAP_MAKE_PREVIOUS_PROGRESS_STRING_TABLE_INDEX: u32 = 2620;
pub const MAP_MAKE_PREVIOUS_PROGRESS_STRING_BYTE_OFFSET: u32 = 0x0000_ccb0;
pub const MAP_MAKE_PREVIOUS_PROGRESS_RESOURCE_HASH: u32 = 60_488_566;
pub const MAP_MAKE_COASTLINES_STRING_TABLE_INDEX: u32 = 2623;
pub const MAP_MAKE_COASTLINES_RESOURCE_HASH: u32 = 27_580_769;
pub const LOCALIZED_STRING_TABLE_INIT_VA: u32 = 0x00a2_8520;
pub const LOCALIZED_STRING_TABLE_INIT_END_VA: u32 = 0x00a2_8a6e;
pub const LOCALIZED_STRING_TABLE_INIT_RET_VA: u32 = 0x00a2_8a6b;
pub const LOCALIZED_STRING_TABLE_INIT_SIZE: u32 = 1358;
pub const LOCALIZED_STRING_TABLE_INIT_INSTRUCTION_COUNT: u32 = 357;
pub const LOCALIZED_STRING_TABLE_INIT_SHA256: &str =
    "fc556322e27480913363b4caa6f6b1360dce73309100d6ef670ba70d4c4cd352";
pub const LOCALIZED_STRING_TABLE_CONST_ASSIGN_CALL_VA: u32 = 0x00a2_8913;
pub const STRING_WIDE_ASSIGN_VA: u32 = 0x00a1_db60;
pub const STRING_WIDE_ASSIGN_END_VA: u32 = 0x00a1_dbce;
pub const STRING_WIDE_ASSIGN_RET_VA: u32 = 0x00a1_dbcb;
pub const STRING_WIDE_ASSIGN_SIZE: u32 = 110;
pub const STRING_WIDE_ASSIGN_INSTRUCTION_COUNT: u32 = 46;
pub const STRING_WIDE_ASSIGN_SHA256: &str =
    "c471dd1f943e71f204437136e9d57a184ebfad0204f7258f2737789a2de1c408";
pub const SPLASH_SCREEN_SUBTITLE_STRING_VA: u32 = 0x00e8_0314;
pub const MAP_MAKE_PREVIOUS_PROGRESS_CONSTRUCTOR_CALL_VA: u32 = 0x0068_bdc8;
pub const MAP_MAKE_PREVIOUS_PROGRESS_ASSIGN_CALL_VA: u32 = 0x0068_bddd;
pub const MAP_MAKE_PREVIOUS_PROGRESS_LOCAL_CLOSE_CALL_VA: u32 = 0x0068_bdf1;
pub const MAP_MAKE_COASTLINES_VA: u32 = 0x0069_47a0;
pub const TERRAIN_GROUPS_FILL_FERTILE_VA: u32 = 0x006a_6f90;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RegionsClearAllNativeBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub ret_vas: &'static [u32],
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
    pub direct_calls: &'static [(u32, u32)],
    pub indirect_import_calls: &'static [(u32, u32)],
}

pub const REGIONS_CLEAR_ALL_NATIVE_BODY: RegionsClearAllNativeBody = RegionsClearAllNativeBody {
    entry_va: REGIONS_CLEAR_ALL_VA,
    end_va_exclusive: REGIONS_CLEAR_ALL_END_VA,
    ret_vas: &[
        REGIONS_CLEAR_ALL_RET_NONEMPTY_WORLD_VA,
        REGIONS_CLEAR_ALL_RET_EMPTY_WORLD_VA,
        REGIONS_CLEAR_ALL_RET_NULL_WORLD_DATA_VA,
    ],
    size: REGIONS_CLEAR_ALL_SIZE,
    instruction_count: REGIONS_CLEAR_ALL_INSTRUCTION_COUNT,
    sha256: REGIONS_CLEAR_ALL_SHA256,
    direct_calls: &[],
    indirect_import_calls: &[(REGIONS_CLEAR_ALL_COORD_FREE_CALL_VA, FREE_IMPORT_IAT_VA)],
};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RegionsFindAllNativeBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub ret_va: u32,
    pub callee_stack_argument_bytes_popped: u8,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
    pub direct_calls: &'static [(u32, u32)],
    pub indirect_import_calls: &'static [(u32, u32)],
    pub indirect_virtual_calls: &'static [u32],
}

pub const REGIONS_FIND_ALL_NATIVE_BODY: RegionsFindAllNativeBody = RegionsFindAllNativeBody {
    entry_va: REGIONS_FIND_ALL_VA,
    end_va_exclusive: REGIONS_FIND_ALL_END_VA,
    ret_va: REGIONS_FIND_ALL_RET_VA,
    callee_stack_argument_bytes_popped: 4,
    size: REGIONS_FIND_ALL_SIZE,
    instruction_count: REGIONS_FIND_ALL_INSTRUCTION_COUNT,
    sha256: REGIONS_FIND_ALL_SHA256,
    direct_calls: &[
        (0x0067_f252, REGIONS_FIND_ALL_STRING_CONSTRUCTOR_VA),
        (0x0067_f26f, REGIONS_FIND_ALL_ERROR_REPORT_VA),
        (0x0067_f27d, REGIONS_FIND_ALL_STRING_CLOSE_VA),
        (0x0067_f28c, REGIONS_FIND_ALL_STRING_CLOSE_VA),
        (0x0067_f4e0, REGIONS_FIND_ALL_STRING_CONSTRUCTOR_VA),
        (0x0067_f500, REGIONS_FIND_ALL_ERROR_REPORT_VA),
        (0x0067_f511, REGIONS_FIND_ALL_STRING_CLOSE_VA),
        (0x0067_f520, REGIONS_FIND_ALL_STRING_CLOSE_VA),
        (REGIONS_FIND_ALL_FIND_CALL_VA, REGIONS_FIND_VA),
        (
            REGIONS_FIND_ALL_SET_COASTALS_CALL_VA,
            REGIONS_SET_COASTALS_VA,
        ),
        (
            REGIONS_FIND_ALL_SORT_REGIONS_CALL_VA,
            REGIONS_SORT_REGIONS_VA,
        ),
        (
            REGIONS_FIND_ALL_REBUILD_COORDS_CALL_VA,
            REGIONS_REBUILD_COORDS_VA,
        ),
        (REGIONS_FIND_ALL_NON_INPUT_CALL_VA, DO_ALL_NON_INPUT_VA),
    ],
    indirect_import_calls: &[
        (
            REGIONS_FIND_ALL_OLD_SCRATCH_FREE_CALL_VA,
            FREE_IMPORT_IAT_VA,
        ),
        (
            REGIONS_FIND_ALL_SCRATCH_MALLOC_CALL_VA,
            MALLOC_IMPORT_IAT_VA,
        ),
        (
            REGIONS_FIND_ALL_FINAL_SCRATCH_FREE_CALL_VA,
            FREE_IMPORT_IAT_VA,
        ),
    ],
    indirect_virtual_calls: &[0x0067_f2f5],
};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MapMakeFirstRegionsFindAllCallerBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
    pub direct_calls: &'static [(u32, u32)],
}

pub const MAP_MAKE_FIRST_REGIONS_FIND_ALL_CALLER_BODY: MapMakeFirstRegionsFindAllCallerBody =
    MapMakeFirstRegionsFindAllCallerBody {
        entry_va: MAP_MAKE_FIRST_REGIONS_FIND_ARGUMENT_PUSH_VA,
        end_va_exclusive: MAP_MAKE_FIRST_REGIONS_FIND_CALLER_END_VA,
        size: MAP_MAKE_FIRST_REGIONS_FIND_CALLER_SIZE,
        instruction_count: MAP_MAKE_FIRST_REGIONS_FIND_CALLER_INSTRUCTION_COUNT,
        sha256: MAP_MAKE_FIRST_REGIONS_FIND_CALLER_SHA256,
        direct_calls: &[(MAP_MAKE_FIRST_REGIONS_FIND_CALL_VA, REGIONS_FIND_ALL_VA)],
    };

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MapMakeFirstTerritoryPrepBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
}

pub const MAP_MAKE_FIRST_TERRITORY_PREP_BODY: MapMakeFirstTerritoryPrepBody =
    MapMakeFirstTerritoryPrepBody {
        entry_va: MAP_MAKE_FIRST_TERRITORY_WORLD_LOAD_VA,
        end_va_exclusive: MAP_MAKE_FIRST_TERRITORY_PREP_END_VA,
        size: MAP_MAKE_FIRST_TERRITORY_PREP_SIZE,
        instruction_count: MAP_MAKE_FIRST_TERRITORY_PREP_INSTRUCTION_COUNT,
        sha256: MAP_MAKE_FIRST_TERRITORY_PREP_SHA256,
    };

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RegionsAllocationState {
    Unallocated,
    Live,
    Freed,
}

/// One logical `WCoordList` allocation released by the loop's imported
/// `_free`. Native pointer identity is intentionally not represented.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionsClearAllCoordFreeReceipt {
    pub call_va: u32,
    pub import_iat_va: u32,
    pub region: u8,
    pub elements: Vec<(i32, i32)>,
    pub capacity: i32,
    pub element_width: u8,
    pub state_before: RegionsAllocationState,
    pub state_after: RegionsAllocationState,
    pub native_return: (),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionsClearAllRegionReceipt {
    pub region: u8,
    pub before: Region,
    pub after: Region,
    pub size_nonzero_path: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RegionsClearAllWorldMutation {
    pub cell: usize,
    pub region_before: i16,
    pub region_after: i16,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MapMakeFirstRegionsClearAllNext {
    FindAll {
        argument_push_va: u32,
        call_va: u32,
        primitive_va: u32,
        stack_argument_is_unread: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapMakeFirstRegionsClearAllReceipt {
    pub caller_call_va: u32,
    pub body: RegionsClearAllNativeBody,
    pub executed_ret_va: u32,
    pub caller_resume_va: u32,
    pub region_records_visited: usize,
    pub region_records: Vec<RegionsClearAllRegionReceipt>,
    pub coordinate_frees: Vec<RegionsClearAllCoordFreeReceipt>,
    pub regions_coords_before: WCoordList,
    pub regions_coords_after: WCoordList,
    pub regions_land_before: i32,
    pub regions_land_after: i32,
    pub regions_sea_before: i32,
    pub regions_sea_after: i32,
    pub world_cells_visited: usize,
    pub world_region_mutations: Vec<RegionsClearAllWorldMutation>,
    pub world_region2_unchanged: bool,
    pub world_before: WorldChecksum,
    pub world_after: WorldChecksum,
    pub world_sections_changed: Vec<WorldSection>,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub direct_rng_sites: Vec<u32>,
    pub next: MapMakeFirstRegionsClearAllNext,
}

/// Execute the first common-driver `Regions::clear_all` after a style virtual
/// returns, and freeze before `Regions::find_all(int)`.
pub fn execute_map_make_first_regions_clear_all(
    world: &mut World,
    regions: &mut Regions,
    random_state: i32,
) -> MapMakeFirstRegionsClearAllReceipt {
    let world_before = world.checksum_sections();
    let region2_before = world
        .wdata
        .iter()
        .map(|cell| cell.region2)
        .collect::<Vec<_>>();
    let region_labels_before = world
        .wdata
        .iter()
        .map(|cell| cell.region)
        .collect::<Vec<_>>();
    let regions_before = regions.clone();

    regions.clear_all(world);

    let world_after = world.checksum_sections();
    let region_records = regions_before
        .list
        .iter()
        .zip(regions.list.iter())
        .enumerate()
        .map(|(region, (before, after))| RegionsClearAllRegionReceipt {
            region: region as u8,
            before: before.clone(),
            after: after.clone(),
            size_nonzero_path: before.size != 0,
        })
        .collect();
    let coordinate_frees = regions_before
        .list
        .iter()
        .enumerate()
        .filter(|(_, region)| region.size != 0 && region.coords.capacity != 0)
        .map(|(region, record)| RegionsClearAllCoordFreeReceipt {
            call_va: REGIONS_CLEAR_ALL_COORD_FREE_CALL_VA,
            import_iat_va: FREE_IMPORT_IAT_VA,
            region: region as u8,
            elements: record.coords.items.clone(),
            capacity: record.coords.capacity,
            element_width: 8,
            state_before: RegionsAllocationState::Live,
            state_after: RegionsAllocationState::Freed,
            native_return: (),
        })
        .collect();
    let world_region_mutations = region_labels_before
        .into_iter()
        .zip(world.wdata.iter().map(|cell| cell.region))
        .enumerate()
        .filter_map(|(cell, (region_before, region_after))| {
            (region_before != region_after).then_some(RegionsClearAllWorldMutation {
                cell,
                region_before,
                region_after,
            })
        })
        .collect();
    let world_sections_changed = WorldSection::all()
        .into_iter()
        .filter(|section| world_before.section(*section) != world_after.section(*section))
        .collect();

    MapMakeFirstRegionsClearAllReceipt {
        caller_call_va: MAP_MAKE_FIRST_REGIONS_CLEAR_CALL_VA,
        body: REGIONS_CLEAR_ALL_NATIVE_BODY,
        executed_ret_va: REGIONS_CLEAR_ALL_RET_NONEMPTY_WORLD_VA,
        caller_resume_va: MAP_MAKE_FIRST_REGIONS_CLEAR_RESUME_VA,
        region_records_visited: REGION_COUNT,
        region_records,
        coordinate_frees,
        regions_coords_before: regions_before.coords.clone(),
        regions_coords_after: regions.coords.clone(),
        regions_land_before: regions_before.land,
        regions_land_after: regions.land,
        regions_sea_before: regions_before.sea,
        regions_sea_after: regions.sea,
        world_cells_visited: world.wdata.len(),
        world_region_mutations,
        world_region2_unchanged: region2_before
            == world
                .wdata
                .iter()
                .map(|cell| cell.region2)
                .collect::<Vec<_>>(),
        world_before,
        world_after,
        world_sections_changed,
        random_state_before: random_state,
        random_state_after: random_state,
        direct_rng_sites: Vec::new(),
        next: MapMakeFirstRegionsClearAllNext::FindAll {
            argument_push_va: MAP_MAKE_FIRST_REGIONS_FIND_ARGUMENT_PUSH_VA,
            call_va: MAP_MAKE_FIRST_REGIONS_FIND_CALL_VA,
            primitive_va: REGIONS_FIND_ALL_VA,
            stack_argument_is_unread: true,
        },
    }
}

pub(crate) fn validate_map_make_first_regions_clear_all_receipt(
    world: &World,
    regions: &Regions,
    receipt: &MapMakeFirstRegionsClearAllReceipt,
) -> bool {
    if receipt.caller_call_va != MAP_MAKE_FIRST_REGIONS_CLEAR_CALL_VA
        || receipt.body != REGIONS_CLEAR_ALL_NATIVE_BODY
        || receipt.executed_ret_va != REGIONS_CLEAR_ALL_RET_NONEMPTY_WORLD_VA
        || receipt.caller_resume_va != MAP_MAKE_FIRST_REGIONS_CLEAR_RESUME_VA
        || receipt.region_records_visited != REGION_COUNT
        || receipt.region_records.len() != REGION_COUNT
        || receipt.regions_coords_before != receipt.regions_coords_after
        || receipt.regions_coords_after != regions.coords
        || receipt.regions_land_after != 0
        || receipt.regions_land_after != regions.land
        || receipt.regions_sea_after != 64
        || receipt.regions_sea_after != regions.sea
        || receipt.world_cells_visited != world.wdata.len()
        || !receipt.world_region2_unchanged
        || world.wdata.iter().any(|cell| cell.region != 0)
        || receipt.world_after != world.checksum_sections()
        || receipt.world_sections_changed
            != receipt
                .world_before
                .differing_sections(&receipt.world_after)
        || receipt.random_state_before != receipt.random_state_after
        || !receipt.direct_rng_sites.is_empty()
        || receipt.next
            != (MapMakeFirstRegionsClearAllNext::FindAll {
                argument_push_va: MAP_MAKE_FIRST_REGIONS_FIND_ARGUMENT_PUSH_VA,
                call_va: MAP_MAKE_FIRST_REGIONS_FIND_CALL_VA,
                primitive_va: REGIONS_FIND_ALL_VA,
                stack_argument_is_unread: true,
            })
    {
        return false;
    }

    for (region, record) in receipt.region_records.iter().enumerate() {
        if usize::from(record.region) != region
            || record.size_nonzero_path != (record.before.size != 0)
            || record.after != regions.list[region]
        {
            return false;
        }
        let mut expected = record.before.clone();
        expected.flags = 0;
        expected.climate = 0;
        expected.goodies = 0;
        expected.common_factor = 8;
        expected.goody_factor = 8;
        if expected.size != 0 {
            expected.size = 0;
            expected.coords.items.clear();
            expected.coords.capacity = 0;
            expected.coords.flags = 0;
            expected.borders = 0;
            expected.border_id = 0;
        }
        if record.after != expected {
            return false;
        }
    }

    let expected_frees = receipt
        .region_records
        .iter()
        .filter(|record| record.before.size != 0 && record.before.coords.capacity != 0)
        .map(|record| record.region)
        .collect::<Vec<_>>();
    if receipt.coordinate_frees.len() != expected_frees.len() {
        return false;
    }
    for (free, expected_region) in receipt.coordinate_frees.iter().zip(expected_frees) {
        if free.region != expected_region {
            return false;
        }
        let Some(record) = receipt.region_records.get(usize::from(free.region)) else {
            return false;
        };
        if free.call_va != REGIONS_CLEAR_ALL_COORD_FREE_CALL_VA
            || free.import_iat_va != FREE_IMPORT_IAT_VA
            || free.elements != record.before.coords.items
            || free.capacity != record.before.coords.capacity
            || free.element_width != 8
            || free.state_before != RegionsAllocationState::Live
            || free.state_after != RegionsAllocationState::Freed
        {
            return false;
        }
    }
    receipt
        .world_region_mutations
        .iter()
        .enumerate()
        .all(|(index, mutation)| {
            mutation.cell < world.wdata.len()
                && (index == 0 || receipt.world_region_mutations[index - 1].cell < mutation.cell)
                && mutation.region_before != 0
                && mutation.region_after == 0
        })
}

/// One imported allocator call over the logical shared `Regions::coords`
/// scratch queue. No native pointer value is retained or compared.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionsFindAllScratchAllocatorCall {
    pub call_va: u32,
    pub import_iat_va: u32,
    pub elements: i32,
    pub bytes: u64,
    pub state_before: RegionsAllocationState,
    pub state_after: RegionsAllocationState,
    pub native_return: (),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionsFindAllScratchReceipt {
    pub before: WCoordList,
    pub old_allocation_free: Option<RegionsFindAllScratchAllocatorCall>,
    pub allocation: Option<RegionsFindAllScratchAllocatorCall>,
    pub capacity_during_body: i32,
    pub final_free: Option<RegionsFindAllScratchAllocatorCall>,
    pub after: WCoordList,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionsFindAllRegionReceipt {
    pub region: u8,
    pub before: Region,
    pub after: Region,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RegionsFindAllWorldMutation {
    pub cell: usize,
    pub region_before: i16,
    pub region_after: i16,
    pub region2_before: i16,
    pub region2_after: i16,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MapMakeFirstRegionsFindAllNext {
    TerritoryLimitStore {
        prep: MapMakeFirstTerritoryPrepBody,
        world_owner_load_va: u32,
        map_value_load_va: u32,
        store_va: u32,
        map_field_offset: u32,
        world_field_offset: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapMakeFirstRegionsFindAllReceipt {
    pub caller: MapMakeFirstRegionsFindAllCallerBody,
    pub body: RegionsFindAllNativeBody,
    pub executed_ret_va: u32,
    pub callee_stack_argument_bytes_popped: u8,
    pub caller_resume_va: u32,
    pub stack_argument_is_unread: bool,
    pub build: RegionBuildReceipt,
    pub successful_find_calls: i32,
    pub diagnostic_calls_executed: Vec<u32>,
    pub scratch: RegionsFindAllScratchReceipt,
    pub region_records_visited: usize,
    pub region_records: Vec<RegionsFindAllRegionReceipt>,
    pub regions_land_before: i32,
    pub regions_land_after: i32,
    pub regions_sea_before: i32,
    pub regions_sea_after: i32,
    pub world_cells_visited: usize,
    pub world_mutations: Vec<RegionsFindAllWorldMutation>,
    pub world_before: WorldChecksum,
    pub world_after: WorldChecksum,
    pub world_sections_changed: Vec<WorldSection>,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub direct_rng_sites: Vec<u32>,
    pub next: MapMakeFirstRegionsFindAllNext,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapMakeFirstRegionsFindAllError {
    PriorClearReceiptMismatch,
    FindAll(RegionsError),
}

/// Execute the exact first common-driver `Regions::find_all(int)` body and
/// freeze before the first World territory-limit store at `0x0068be4a`.
pub fn execute_map_make_first_regions_find_all(
    world: &mut World,
    regions: &mut Regions,
    random_state: i32,
    prior_clear: &MapMakeFirstRegionsClearAllReceipt,
) -> Result<MapMakeFirstRegionsFindAllReceipt, MapMakeFirstRegionsFindAllError> {
    if !validate_map_make_first_regions_clear_all_receipt(world, regions, prior_clear)
        || prior_clear.random_state_after != random_state
    {
        return Err(MapMakeFirstRegionsFindAllError::PriorClearReceiptMismatch);
    }

    let world_before = world.checksum_sections();
    let world_region_before = world
        .wdata
        .iter()
        .map(|cell| (cell.region, cell.region2))
        .collect::<Vec<_>>();
    let regions_before = regions.clone();
    let build = regions
        .find_all_after_clear(world)
        .map_err(MapMakeFirstRegionsFindAllError::FindAll)?;
    let world_after = world.checksum_sections();

    let scratch_grows = regions_before.coords.capacity < world.size;
    let old_scratch_live = regions_before.coords.capacity > 0;
    let allocated_elements = if scratch_grows {
        world.size.max(0)
    } else {
        regions_before.coords.capacity.max(0)
    };
    let allocated_bytes = u64::try_from(allocated_elements).unwrap_or(0) * 8;
    let old_allocation_free =
        (scratch_grows && old_scratch_live).then(|| RegionsFindAllScratchAllocatorCall {
            call_va: REGIONS_FIND_ALL_OLD_SCRATCH_FREE_CALL_VA,
            import_iat_va: FREE_IMPORT_IAT_VA,
            elements: regions_before.coords.capacity,
            bytes: u64::try_from(regions_before.coords.capacity).unwrap_or(0) * 8,
            state_before: RegionsAllocationState::Live,
            state_after: RegionsAllocationState::Freed,
            native_return: (),
        });
    let allocation =
        (scratch_grows && world.size > 0).then(|| RegionsFindAllScratchAllocatorCall {
            call_va: REGIONS_FIND_ALL_SCRATCH_MALLOC_CALL_VA,
            import_iat_va: MALLOC_IMPORT_IAT_VA,
            elements: world.size,
            bytes: u64::try_from(world.size).unwrap_or(0) * 8,
            state_before: RegionsAllocationState::Unallocated,
            state_after: RegionsAllocationState::Live,
            native_return: (),
        });
    let scratch_live_during_body = allocated_elements > 0;
    let final_free = scratch_live_during_body.then(|| RegionsFindAllScratchAllocatorCall {
        call_va: REGIONS_FIND_ALL_FINAL_SCRATCH_FREE_CALL_VA,
        import_iat_va: FREE_IMPORT_IAT_VA,
        elements: allocated_elements,
        bytes: allocated_bytes,
        state_before: RegionsAllocationState::Live,
        state_after: RegionsAllocationState::Freed,
        native_return: (),
    });
    let scratch = RegionsFindAllScratchReceipt {
        before: regions_before.coords.clone(),
        old_allocation_free,
        allocation,
        capacity_during_body: allocated_elements,
        final_free,
        after: regions.coords.clone(),
    };
    let region_records = regions_before
        .list
        .iter()
        .zip(regions.list.iter())
        .enumerate()
        .map(|(region, (before, after))| RegionsFindAllRegionReceipt {
            region: region as u8,
            before: before.clone(),
            after: after.clone(),
        })
        .collect();
    let world_mutations = world_region_before
        .into_iter()
        .zip(world.wdata.iter().map(|cell| (cell.region, cell.region2)))
        .enumerate()
        .filter_map(
            |(cell, ((region_before, region2_before), (region_after, region2_after)))| {
                (region_before != region_after || region2_before != region2_after).then_some(
                    RegionsFindAllWorldMutation {
                        cell,
                        region_before,
                        region_after,
                        region2_before,
                        region2_after,
                    },
                )
            },
        )
        .collect();
    let world_sections_changed = WorldSection::all()
        .into_iter()
        .filter(|section| world_before.section(*section) != world_after.section(*section))
        .collect();

    Ok(MapMakeFirstRegionsFindAllReceipt {
        caller: MAP_MAKE_FIRST_REGIONS_FIND_ALL_CALLER_BODY,
        body: REGIONS_FIND_ALL_NATIVE_BODY,
        executed_ret_va: REGIONS_FIND_ALL_RET_VA,
        callee_stack_argument_bytes_popped: 4,
        caller_resume_va: MAP_MAKE_FIRST_REGIONS_FIND_RESUME_VA,
        stack_argument_is_unread: true,
        successful_find_calls: build.land_components_found + build.sea_components_found,
        diagnostic_calls_executed: Vec::new(),
        build,
        scratch,
        region_records_visited: REGION_COUNT,
        region_records,
        regions_land_before: regions_before.land,
        regions_land_after: regions.land,
        regions_sea_before: regions_before.sea,
        regions_sea_after: regions.sea,
        world_cells_visited: world.wdata.len(),
        world_mutations,
        world_before,
        world_after,
        world_sections_changed,
        random_state_before: random_state,
        random_state_after: random_state,
        direct_rng_sites: Vec::new(),
        next: MapMakeFirstRegionsFindAllNext::TerritoryLimitStore {
            prep: MAP_MAKE_FIRST_TERRITORY_PREP_BODY,
            world_owner_load_va: MAP_MAKE_FIRST_TERRITORY_WORLD_LOAD_VA,
            map_value_load_va: MAP_MAKE_FIRST_TERRITORY_MAP_LOAD_VA,
            store_va: MAP_MAKE_FIRST_TERRITORY_STORE_VA,
            map_field_offset: MAP_PLAYER_TERRITORY_LIMIT_OFFSET,
            world_field_offset: WORLD_PLAYER_TERRITORY_LIMIT_OFFSET,
        },
    })
}

pub(crate) fn validate_map_make_first_regions_find_all_receipt(
    world: &World,
    regions: &Regions,
    prior_clear: &MapMakeFirstRegionsClearAllReceipt,
    receipt: &MapMakeFirstRegionsFindAllReceipt,
) -> bool {
    if prior_clear.region_records.len() != REGION_COUNT
        || receipt.world_mutations.len() != world.wdata.len()
    {
        return false;
    }
    let mut clear_world = world.clone();
    for mutation in &receipt.world_mutations {
        if mutation.cell >= clear_world.wdata.len() {
            return false;
        }
        clear_world.wdata[mutation.cell].region = mutation.region_before;
        clear_world.wdata[mutation.cell].region2 = mutation.region2_before;
    }
    let mut clear_regions = regions.clone();
    for (region, record) in prior_clear.region_records.iter().enumerate() {
        if usize::from(record.region) != region {
            return false;
        }
        clear_regions.list[region] = record.after.clone();
    }
    clear_regions.coords = prior_clear.regions_coords_after.clone();
    clear_regions.land = prior_clear.regions_land_after;
    clear_regions.sea = prior_clear.regions_sea_after;
    if !validate_map_make_first_regions_clear_all_receipt(&clear_world, &clear_regions, prior_clear)
    {
        return false;
    }

    let expected_next = MapMakeFirstRegionsFindAllNext::TerritoryLimitStore {
        prep: MAP_MAKE_FIRST_TERRITORY_PREP_BODY,
        world_owner_load_va: MAP_MAKE_FIRST_TERRITORY_WORLD_LOAD_VA,
        map_value_load_va: MAP_MAKE_FIRST_TERRITORY_MAP_LOAD_VA,
        store_va: MAP_MAKE_FIRST_TERRITORY_STORE_VA,
        map_field_offset: MAP_PLAYER_TERRITORY_LIMIT_OFFSET,
        world_field_offset: WORLD_PLAYER_TERRITORY_LIMIT_OFFSET,
    };
    if receipt.caller != MAP_MAKE_FIRST_REGIONS_FIND_ALL_CALLER_BODY
        || receipt.body != REGIONS_FIND_ALL_NATIVE_BODY
        || receipt.executed_ret_va != REGIONS_FIND_ALL_RET_VA
        || receipt.callee_stack_argument_bytes_popped != 4
        || receipt.caller_resume_va != MAP_MAKE_FIRST_REGIONS_FIND_RESUME_VA
        || !receipt.stack_argument_is_unread
        || receipt.successful_find_calls
            != receipt.build.land_components_found + receipt.build.sea_components_found
        || receipt.build.non_input_pumps != receipt.successful_find_calls + 1
        || !receipt.diagnostic_calls_executed.is_empty()
        || receipt.region_records_visited != REGION_COUNT
        || receipt.region_records.len() != REGION_COUNT
        || receipt.regions_land_before != prior_clear.regions_land_after
        || receipt.regions_land_after != regions.land
        || receipt.regions_sea_before != prior_clear.regions_sea_after
        || receipt.regions_sea_after != regions.sea
        || receipt.world_cells_visited != world.wdata.len()
        || receipt.world_mutations.len() != world.wdata.len()
        || receipt.world_before != prior_clear.world_after
        || receipt.world_after != world.checksum_sections()
        || receipt.world_sections_changed
            != receipt
                .world_before
                .differing_sections(&receipt.world_after)
        || receipt.world_sections_changed != [WorldSection::WData]
        || receipt.random_state_before != prior_clear.random_state_after
        || receipt.random_state_before != receipt.random_state_after
        || !receipt.direct_rng_sites.is_empty()
        || receipt.next != expected_next
    {
        return false;
    }

    if receipt
        .region_records
        .iter()
        .enumerate()
        .any(|(region, record)| {
            usize::from(record.region) != region
                || record.before != prior_clear.region_records[region].after
                || record.after != regions.list[region]
        })
    {
        return false;
    }
    if receipt
        .world_mutations
        .iter()
        .enumerate()
        .any(|(cell, mutation)| {
            mutation.cell != cell
                || mutation.region_before != 0
                || mutation.region_after != world.wdata[cell].region
                || mutation.region2_after != world.wdata[cell].region2
                || (mutation.region_before == mutation.region_after
                    && mutation.region2_before == mutation.region2_after)
        })
    {
        return false;
    }

    let before_capacity = receipt.scratch.before.capacity;
    let scratch_grows = before_capacity < world.size;
    let expected_capacity = if scratch_grows {
        world.size.max(0)
    } else {
        before_capacity.max(0)
    };
    if receipt.scratch.before != prior_clear.regions_coords_after
        || receipt.scratch.after != regions.coords
        || !receipt.scratch.after.items.is_empty()
        || receipt.scratch.after.capacity != 0
        || receipt.scratch.after.flags != 0
        || receipt.scratch.capacity_during_body != expected_capacity
        || receipt.scratch.old_allocation_free.is_some() != (scratch_grows && before_capacity > 0)
        || receipt.scratch.allocation.is_some() != (scratch_grows && world.size > 0)
        || receipt.scratch.final_free.is_some() != (expected_capacity > 0)
    {
        return false;
    }
    if let Some(call) = &receipt.scratch.old_allocation_free {
        if call.call_va != REGIONS_FIND_ALL_OLD_SCRATCH_FREE_CALL_VA
            || call.import_iat_va != FREE_IMPORT_IAT_VA
            || call.elements != before_capacity
            || call.bytes != u64::try_from(before_capacity).unwrap_or(0) * 8
            || call.state_before != RegionsAllocationState::Live
            || call.state_after != RegionsAllocationState::Freed
        {
            return false;
        }
    }
    if let Some(call) = &receipt.scratch.allocation {
        if call.call_va != REGIONS_FIND_ALL_SCRATCH_MALLOC_CALL_VA
            || call.import_iat_va != MALLOC_IMPORT_IAT_VA
            || call.elements != world.size
            || call.bytes != u64::try_from(world.size).unwrap_or(0) * 8
            || call.state_before != RegionsAllocationState::Unallocated
            || call.state_after != RegionsAllocationState::Live
        {
            return false;
        }
    }
    if let Some(call) = &receipt.scratch.final_free {
        if call.call_va != REGIONS_FIND_ALL_FINAL_SCRATCH_FREE_CALL_VA
            || call.import_iat_va != FREE_IMPORT_IAT_VA
            || call.elements != expected_capacity
            || call.bytes != u64::try_from(expected_capacity).unwrap_or(0) * 8
            || call.state_before != RegionsAllocationState::Live
            || call.state_after != RegionsAllocationState::Freed
        {
            return false;
        }
    }
    true
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TerritoryLimits {
    pub player_base: i32,
    pub player_civic: i32,
    pub player_city: i32,
    pub colonized_base: i32,
    pub colonized_civic: i32,
    pub colonized_city: i32,
}

impl TerritoryLimits {
    /// Map's six constructor fields come from Constants+0x118/+0x11c/+0x120,
    /// with the same triplet copied to the colonized fields.
    pub fn from_world_prefix(world: &World) -> Self {
        Self {
            player_base: world.player_territory_limit,
            player_civic: world.player_territory_limit_civic,
            player_city: world.player_territory_limit_city,
            colonized_base: world.colonized_territory_limit,
            colonized_civic: world.colonized_territory_limit_civic,
            colonized_city: world.colonized_territory_limit_city,
        }
    }

    fn values(self) -> [i32; 6] {
        [
            self.player_base,
            self.player_civic,
            self.player_city,
            self.colonized_base,
            self.colonized_civic,
            self.colonized_city,
        ]
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MapMakeTerritoryLimitsNativeBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
    pub direct_calls: &'static [(u32, u32)],
}

pub const MAP_MAKE_TERRITORY_LIMITS_NATIVE_BODY: MapMakeTerritoryLimitsNativeBody =
    MapMakeTerritoryLimitsNativeBody {
        entry_va: MAP_MAKE_FIRST_TERRITORY_STORE_VA,
        end_va_exclusive: MAP_MAKE_TERRITORY_LIMITS_END_VA,
        size: MAP_MAKE_TERRITORY_LIMITS_SIZE,
        instruction_count: MAP_MAKE_TERRITORY_LIMITS_INSTRUCTION_COUNT,
        sha256: MAP_MAKE_TERRITORY_LIMITS_SHA256,
        direct_calls: &[],
    };

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TerritoryLimitField {
    PlayerBase,
    PlayerCivic,
    PlayerCity,
    ColonizedBase,
    ColonizedCivic,
    ColonizedCity,
}

pub const TERRITORY_LIMIT_FIELDS: [TerritoryLimitField; 6] = [
    TerritoryLimitField::PlayerBase,
    TerritoryLimitField::PlayerCivic,
    TerritoryLimitField::PlayerCity,
    TerritoryLimitField::ColonizedBase,
    TerritoryLimitField::ColonizedCivic,
    TerritoryLimitField::ColonizedCity,
];

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TerritoryLimitStoreReceipt {
    pub field: TerritoryLimitField,
    pub map_value_load_va: u32,
    pub store_va: u32,
    pub map_field_offset: u32,
    pub world_field_offset: u32,
    pub source_value: i32,
    pub value_before: i32,
    pub value_after: i32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MapMakeTerritoryStyleBranchReceipt {
    pub compare_va: u32,
    pub branch_va: u32,
    pub map_style: u8,
    pub compared_value: u8,
    pub taken: bool,
    pub target_va: u32,
    pub fallthrough_va: u32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MapMakeTerritoryLimitsNext {
    FixDiagLand { call_va: u32, primitive_va: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapMakeTerritoryLimitsReceipt {
    pub body: MapMakeTerritoryLimitsNativeBody,
    pub source: TerritoryLimits,
    pub stores: Vec<TerritoryLimitStoreReceipt>,
    pub branch: MapMakeTerritoryStyleBranchReceipt,
    pub world_before: WorldChecksum,
    pub world_after: WorldChecksum,
    pub world_sections_changed: Vec<WorldSection>,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub direct_rng_sites: Vec<u32>,
    pub next: MapMakeTerritoryLimitsNext,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapMakeTerritoryLimitsError {
    PriorFindAllReceiptMismatch,
    AlternateStyleBranch { map_style: u8, target_va: u32 },
}

fn world_territory_limits(world: &World) -> TerritoryLimits {
    TerritoryLimits {
        player_base: world.player_territory_limit,
        player_civic: world.player_territory_limit_civic,
        player_city: world.player_territory_limit_city,
        colonized_base: world.colonized_territory_limit,
        colonized_civic: world.colonized_territory_limit_civic,
        colonized_city: world.colonized_territory_limit_city,
    }
}

fn set_world_territory_limit(world: &mut World, field: TerritoryLimitField, value: i32) {
    match field {
        TerritoryLimitField::PlayerBase => world.player_territory_limit = value,
        TerritoryLimitField::PlayerCivic => world.player_territory_limit_civic = value,
        TerritoryLimitField::PlayerCity => world.player_territory_limit_city = value,
        TerritoryLimitField::ColonizedBase => world.colonized_territory_limit = value,
        TerritoryLimitField::ColonizedCivic => world.colonized_territory_limit_civic = value,
        TerritoryLimitField::ColonizedCity => world.colonized_territory_limit_city = value,
    }
}

/// Execute the six exact `Map::make` territory-limit stores and the style-23
/// branch. The admitted East Meets West path freezes before `Map::fix_diag_land`.
pub fn execute_map_make_territory_limits(
    world: &mut World,
    regions: &Regions,
    map_style: u8,
    source: TerritoryLimits,
    random_state: i32,
    prior_clear: &MapMakeFirstRegionsClearAllReceipt,
    prior_find_all: &MapMakeFirstRegionsFindAllReceipt,
) -> Result<MapMakeTerritoryLimitsReceipt, MapMakeTerritoryLimitsError> {
    if !validate_map_make_first_regions_find_all_receipt(
        world,
        regions,
        prior_clear,
        prior_find_all,
    ) || prior_find_all.random_state_after != random_state
    {
        return Err(MapMakeTerritoryLimitsError::PriorFindAllReceiptMismatch);
    }
    if map_style == MAP_MAKE_STYLE_BRANCH_VALUE {
        return Err(MapMakeTerritoryLimitsError::AlternateStyleBranch {
            map_style,
            target_va: MAP_MAKE_STYLE_BRANCH_TARGET_VA,
        });
    }

    let world_before = world.checksum_sections();
    let before_values = world_territory_limits(world).values();
    let source_values = source.values();
    let mut stores = Vec::with_capacity(TERRITORY_LIMIT_FIELDS.len());
    for index in 0..TERRITORY_LIMIT_FIELDS.len() {
        let field = TERRITORY_LIMIT_FIELDS[index];
        set_world_territory_limit(world, field, source_values[index]);
        stores.push(TerritoryLimitStoreReceipt {
            field,
            map_value_load_va: MAP_MAKE_TERRITORY_LIMIT_LOAD_VAS[index],
            store_va: MAP_MAKE_TERRITORY_LIMIT_STORE_VAS[index],
            map_field_offset: MAP_TERRITORY_LIMIT_OFFSETS[index],
            world_field_offset: WORLD_TERRITORY_LIMIT_OFFSETS[index],
            source_value: source_values[index],
            value_before: before_values[index],
            value_after: source_values[index],
        });
    }
    let world_after = world.checksum_sections();
    let world_sections_changed = world_before.differing_sections(&world_after);

    Ok(MapMakeTerritoryLimitsReceipt {
        body: MAP_MAKE_TERRITORY_LIMITS_NATIVE_BODY,
        source,
        stores,
        branch: MapMakeTerritoryStyleBranchReceipt {
            compare_va: MAP_MAKE_STYLE_COMPARE_VA,
            branch_va: MAP_MAKE_STYLE_BRANCH_VA,
            map_style,
            compared_value: MAP_MAKE_STYLE_BRANCH_VALUE,
            taken: false,
            target_va: MAP_MAKE_STYLE_BRANCH_TARGET_VA,
            fallthrough_va: MAP_MAKE_FIRST_FIX_DIAG_LAND_CALL_VA,
        },
        world_before,
        world_after,
        world_sections_changed,
        random_state_before: random_state,
        random_state_after: random_state,
        direct_rng_sites: Vec::new(),
        next: MapMakeTerritoryLimitsNext::FixDiagLand {
            call_va: MAP_MAKE_FIRST_FIX_DIAG_LAND_CALL_VA,
            primitive_va: MAP_FIX_DIAG_LAND_VA,
        },
    })
}

pub(crate) fn validate_map_make_territory_limits_receipt(
    world: &World,
    regions: &Regions,
    prior_clear: &MapMakeFirstRegionsClearAllReceipt,
    prior_find_all: &MapMakeFirstRegionsFindAllReceipt,
    receipt: &MapMakeTerritoryLimitsReceipt,
) -> bool {
    if receipt.stores.len() != TERRITORY_LIMIT_FIELDS.len() {
        return false;
    }
    let mut find_world = world.clone();
    for store in &receipt.stores {
        set_world_territory_limit(&mut find_world, store.field, store.value_before);
    }
    if !validate_map_make_first_regions_find_all_receipt(
        &find_world,
        regions,
        prior_clear,
        prior_find_all,
    ) {
        return false;
    }

    let source_values = receipt.source.values();
    let current_values = world_territory_limits(world).values();
    for index in 0..TERRITORY_LIMIT_FIELDS.len() {
        let store = receipt.stores[index];
        if store.field != TERRITORY_LIMIT_FIELDS[index]
            || store.map_value_load_va != MAP_MAKE_TERRITORY_LIMIT_LOAD_VAS[index]
            || store.store_va != MAP_MAKE_TERRITORY_LIMIT_STORE_VAS[index]
            || store.map_field_offset != MAP_TERRITORY_LIMIT_OFFSETS[index]
            || store.world_field_offset != WORLD_TERRITORY_LIMIT_OFFSETS[index]
            || store.source_value != source_values[index]
            || store.value_after != source_values[index]
            || store.value_after != current_values[index]
        {
            return false;
        }
    }

    receipt.body == MAP_MAKE_TERRITORY_LIMITS_NATIVE_BODY
        && receipt.branch
            == (MapMakeTerritoryStyleBranchReceipt {
                compare_va: MAP_MAKE_STYLE_COMPARE_VA,
                branch_va: MAP_MAKE_STYLE_BRANCH_VA,
                map_style: 19,
                compared_value: MAP_MAKE_STYLE_BRANCH_VALUE,
                taken: false,
                target_va: MAP_MAKE_STYLE_BRANCH_TARGET_VA,
                fallthrough_va: MAP_MAKE_FIRST_FIX_DIAG_LAND_CALL_VA,
            })
        && receipt.world_before == prior_find_all.world_after
        && receipt.world_before == find_world.checksum_sections()
        && receipt.world_after == world.checksum_sections()
        && receipt.world_sections_changed
            == receipt
                .world_before
                .differing_sections(&receipt.world_after)
        && receipt.random_state_before == prior_find_all.random_state_after
        && receipt.random_state_before == receipt.random_state_after
        && receipt.direct_rng_sites.is_empty()
        && receipt.next
            == (MapMakeTerritoryLimitsNext::FixDiagLand {
                call_va: MAP_MAKE_FIRST_FIX_DIAG_LAND_CALL_VA,
                primitive_va: MAP_FIX_DIAG_LAND_VA,
            })
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MapFixDiagLandNativeBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub ret_va: u32,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
    pub direct_calls: &'static [(u32, u32)],
    pub corner_x_va: u32,
    pub corner_y_va: u32,
    pub corner_x: [i32; 4],
    pub corner_y: [i32; 4],
}

pub const MAP_FIX_DIAG_LAND_NATIVE_BODY: MapFixDiagLandNativeBody = MapFixDiagLandNativeBody {
    entry_va: MAP_FIX_DIAG_LAND_VA,
    end_va_exclusive: MAP_FIX_DIAG_LAND_END_VA,
    ret_va: MAP_FIX_DIAG_LAND_RET_VA,
    size: MAP_FIX_DIAG_LAND_SIZE,
    instruction_count: MAP_FIX_DIAG_LAND_INSTRUCTION_COUNT,
    sha256: MAP_FIX_DIAG_LAND_SHA256,
    direct_calls: &[],
    corner_x_va: MAP_FIX_DIAG_LAND_CORNER_X_VA,
    corner_y_va: MAP_FIX_DIAG_LAND_CORNER_Y_VA,
    corner_x: MAP_FIX_DIAG_LAND_CORNER_X,
    corner_y: MAP_FIX_DIAG_LAND_CORNER_Y,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapFixDiagLandWorldMutation {
    pub x: i32,
    pub y: i32,
    pub cell: usize,
    pub before: WData,
    pub after: WData,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MapFixDiagLandNext {
    StringConstructor {
        caller_resume_va: u32,
        string_literal_push_va: u32,
        string_local_load_va: u32,
        call_va: u32,
        primitive_va: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapFixDiagLandReceipt {
    pub caller_call_va: u32,
    pub body: MapFixDiagLandNativeBody,
    pub cells_scanned: usize,
    pub mutations: Vec<MapFixDiagLandWorldMutation>,
    pub world_before: WorldChecksum,
    pub world_after: WorldChecksum,
    pub world_sections_changed: Vec<WorldSection>,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub direct_rng_sites: Vec<u32>,
    pub next: MapFixDiagLandNext,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapFixDiagLandError {
    PriorTerritoryLimitsReceiptMismatch,
}

/// Execute the complete call-free retail `Map::fix_diag_land` body. The scan
/// is X-major and in-place; every changed WData record is retained verbatim.
pub fn execute_map_fix_diag_land(
    world: &mut World,
    regions: &Regions,
    random_state: i32,
    prior_clear: &MapMakeFirstRegionsClearAllReceipt,
    prior_find_all: &MapMakeFirstRegionsFindAllReceipt,
    prior_limits: &MapMakeTerritoryLimitsReceipt,
) -> Result<MapFixDiagLandReceipt, MapFixDiagLandError> {
    if !validate_map_make_territory_limits_receipt(
        world,
        regions,
        prior_clear,
        prior_find_all,
        prior_limits,
    ) || prior_limits.random_state_after != random_state
    {
        return Err(MapFixDiagLandError::PriorTerritoryLimitsReceiptMismatch);
    }

    let before_world = world.clone();
    let world_before = before_world.checksum_sections();
    world.fix_diag_land();
    let world_after = world.checksum_sections();
    let mut mutations = Vec::new();
    for x in 0..world.xs {
        for y in 0..world.ys {
            let cell = world.w_index(x, y);
            if before_world.wdata[cell] != world.wdata[cell] {
                mutations.push(MapFixDiagLandWorldMutation {
                    x,
                    y,
                    cell,
                    before: before_world.wdata[cell].clone(),
                    after: world.wdata[cell].clone(),
                });
            }
        }
    }

    Ok(MapFixDiagLandReceipt {
        caller_call_va: MAP_MAKE_FIRST_FIX_DIAG_LAND_CALL_VA,
        body: MAP_FIX_DIAG_LAND_NATIVE_BODY,
        cells_scanned: world.wdata.len(),
        mutations,
        world_sections_changed: world_before.differing_sections(&world_after),
        world_before,
        world_after,
        random_state_before: random_state,
        random_state_after: random_state,
        direct_rng_sites: Vec::new(),
        next: MapFixDiagLandNext::StringConstructor {
            caller_resume_va: MAP_MAKE_FIX_DIAG_LAND_RESUME_VA,
            string_literal_push_va: MAP_MAKE_POST_FIX_DIAG_STRING_LITERAL_PUSH_VA,
            string_local_load_va: MAP_MAKE_POST_FIX_DIAG_STRING_LOCAL_LOAD_VA,
            call_va: MAP_MAKE_POST_FIX_DIAG_STRING_CONSTRUCTOR_CALL_VA,
            primitive_va: STRING_CONSTRUCTOR_VA,
        },
    })
}

pub(crate) fn validate_map_fix_diag_land_receipt(
    world: &World,
    regions: &Regions,
    prior_clear: &MapMakeFirstRegionsClearAllReceipt,
    prior_find_all: &MapMakeFirstRegionsFindAllReceipt,
    prior_limits: &MapMakeTerritoryLimitsReceipt,
    receipt: &MapFixDiagLandReceipt,
) -> bool {
    let mut before_world = world.clone();
    let mut previous = None;
    for mutation in &receipt.mutations {
        if mutation.x < 0
            || mutation.x >= world.xs
            || mutation.y < 0
            || mutation.y >= world.ys
            || mutation.cell != world.w_index(mutation.x, mutation.y)
            || previous.is_some_and(|(x, y)| (mutation.x, mutation.y) <= (x, y))
            || world.wdata[mutation.cell] != mutation.after
        {
            return false;
        }
        let mut expected = mutation.before.clone();
        expected.land = land::OCEAN;
        expected.land_sub = 0;
        if mutation.before.land != 0 || mutation.after != expected {
            return false;
        }
        before_world.wdata[mutation.cell] = mutation.before.clone();
        previous = Some((mutation.x, mutation.y));
    }
    if !validate_map_make_territory_limits_receipt(
        &before_world,
        regions,
        prior_clear,
        prior_find_all,
        prior_limits,
    ) {
        return false;
    }
    let mut replayed = before_world.clone();
    replayed.fix_diag_land();

    receipt.caller_call_va == MAP_MAKE_FIRST_FIX_DIAG_LAND_CALL_VA
        && receipt.body == MAP_FIX_DIAG_LAND_NATIVE_BODY
        && receipt.cells_scanned == world.wdata.len()
        && receipt.world_before == before_world.checksum_sections()
        && receipt.world_before == prior_limits.world_after
        && receipt.world_after == world.checksum_sections()
        && receipt.world_after == replayed.checksum_sections()
        && replayed.wdata == world.wdata
        && receipt.world_sections_changed
            == receipt
                .world_before
                .differing_sections(&receipt.world_after)
        && receipt
            .world_sections_changed
            .iter()
            .all(|section| *section == WorldSection::WData)
        && receipt.random_state_before == prior_limits.random_state_after
        && receipt.random_state_before == receipt.random_state_after
        && receipt.direct_rng_sites.is_empty()
        && receipt.next
            == (MapFixDiagLandNext::StringConstructor {
                caller_resume_va: MAP_MAKE_FIX_DIAG_LAND_RESUME_VA,
                string_literal_push_va: MAP_MAKE_POST_FIX_DIAG_STRING_LITERAL_PUSH_VA,
                string_local_load_va: MAP_MAKE_POST_FIX_DIAG_STRING_LOCAL_LOAD_VA,
                call_va: MAP_MAKE_POST_FIX_DIAG_STRING_CONSTRUCTOR_CALL_VA,
                primitive_va: STRING_CONSTRUCTOR_VA,
            })
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StringConstructorNativeBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub ret_va: u32,
    pub callee_stack_argument_bytes_popped: u8,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
    pub direct_calls: &'static [(u32, u32)],
}

pub const STRING_CONSTRUCTOR_NATIVE_BODY: StringConstructorNativeBody =
    StringConstructorNativeBody {
        entry_va: STRING_CONSTRUCTOR_VA,
        end_va_exclusive: STRING_CONSTRUCTOR_END_VA,
        ret_va: STRING_CONSTRUCTOR_RET_VA,
        callee_stack_argument_bytes_popped: 4,
        size: STRING_CONSTRUCTOR_SIZE,
        instruction_count: STRING_CONSTRUCTOR_INSTRUCTION_COUNT,
        sha256: STRING_CONSTRUCTOR_SHA256,
        direct_calls: &[(0x00a1_d673, STRING_INIT_CONST_VA)],
    };

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StringInitConstNativeBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub ret_vas: &'static [u32],
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
    pub direct_calls: &'static [(u32, u32)],
    pub indirect_import_calls: &'static [(u32, u32)],
}

pub const STRING_INIT_CONST_NATIVE_BODY: StringInitConstNativeBody = StringInitConstNativeBody {
    entry_va: STRING_INIT_CONST_VA,
    end_va_exclusive: STRING_INIT_CONST_END_VA,
    ret_vas: &[0x00a1_7057, 0x00a1_7064],
    size: STRING_INIT_CONST_SIZE,
    instruction_count: STRING_INIT_CONST_INSTRUCTION_COUNT,
    sha256: STRING_INIT_CONST_SHA256,
    direct_calls: &[
        (0x00a1_702f, STRING_REINIT_VA),
        (0x00a1_703c, STRING_CHAR_TO_WCHAR_VA),
    ],
    indirect_import_calls: &[],
};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StringReinitNativeBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
    pub direct_calls: &'static [(u32, u32)],
}

pub const STRING_REINIT_NATIVE_BODY: StringReinitNativeBody = StringReinitNativeBody {
    entry_va: STRING_REINIT_VA,
    end_va_exclusive: STRING_REINIT_END_VA,
    size: STRING_REINIT_SIZE,
    instruction_count: STRING_REINIT_INSTRUCTION_COUNT,
    sha256: STRING_REINIT_SHA256,
    direct_calls: &[
        (0x00a1_6151, STRING_CLOSE_VA),
        (0x00a1_6163, STRING_GET_STRING_GUTS_VA),
        (0x00a1_6182, MEMCPY_VA),
        (0x00a1_61f0, STRING_GUTS_RESIZE_VA),
        (0x00a1_6254, STRING_GET_STRING_GUTS_VA),
        (0x00a1_627e, MEMCPY_VA),
    ],
};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StringConstructorHelperNativeBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub ret_vas: &'static [u32],
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
    pub direct_calls: &'static [(u32, u32)],
    pub indirect_import_calls: &'static [(u32, u32)],
}

pub const STRING_GET_STRING_GUTS_NATIVE_BODY: StringConstructorHelperNativeBody =
    StringConstructorHelperNativeBody {
        entry_va: STRING_GET_STRING_GUTS_VA,
        end_va_exclusive: STRING_GET_STRING_GUTS_END_VA,
        ret_vas: &[0x00a1_7c09, 0x00a1_7c25],
        size: STRING_GET_STRING_GUTS_SIZE,
        instruction_count: STRING_GET_STRING_GUTS_INSTRUCTION_COUNT,
        sha256: STRING_GET_STRING_GUTS_SHA256,
        direct_calls: &[
            (0x00a1_7bb7, STRING_GUTS_OPERATOR_NEW_VA),
            (0x00a1_7be3, STRING_GUTS_MEM_GET_VA),
        ],
        indirect_import_calls: &[],
    };

pub const STRING_CHAR_TO_WCHAR_NATIVE_BODY: StringConstructorHelperNativeBody =
    StringConstructorHelperNativeBody {
        entry_va: STRING_CHAR_TO_WCHAR_VA,
        end_va_exclusive: STRING_CHAR_TO_WCHAR_END_VA,
        ret_vas: &[0x00a1_7c75],
        size: STRING_CHAR_TO_WCHAR_SIZE,
        instruction_count: STRING_CHAR_TO_WCHAR_INSTRUCTION_COUNT,
        sha256: STRING_CHAR_TO_WCHAR_SHA256,
        direct_calls: &[],
        indirect_import_calls: &[
            (0x00a1_7c4e, MULTI_BYTE_TO_WIDE_CHAR_IAT_VA),
            (0x00a1_7c60, MULTI_BYTE_TO_WIDE_CHAR_IAT_VA),
        ],
    };

pub const STRING_GUTS_OPERATOR_NEW_NATIVE_BODY: StringConstructorHelperNativeBody =
    StringConstructorHelperNativeBody {
        entry_va: STRING_GUTS_OPERATOR_NEW_VA,
        end_va_exclusive: STRING_GUTS_OPERATOR_NEW_END_VA,
        ret_vas: &[0x00a1_793c, 0x00a1_7996],
        size: STRING_GUTS_OPERATOR_NEW_SIZE,
        instruction_count: STRING_GUTS_OPERATOR_NEW_INSTRUCTION_COUNT,
        sha256: STRING_GUTS_OPERATOR_NEW_SHA256,
        direct_calls: &[(0x00a1_7975, 0x0055_d14a)],
        indirect_import_calls: &[(0x00a1_7946, MALLOC_IAT_VA)],
    };

pub const STRING_GUTS_MEM_GET_NATIVE_BODY: StringConstructorHelperNativeBody =
    StringConstructorHelperNativeBody {
        entry_va: STRING_GUTS_MEM_GET_VA,
        end_va_exclusive: STRING_GUTS_MEM_GET_END_VA,
        ret_vas: &[0x00a1_7aff, 0x00a1_7b46, 0x00a1_7b83],
        size: STRING_GUTS_MEM_GET_SIZE,
        instruction_count: STRING_GUTS_MEM_GET_INSTRUCTION_COUNT,
        sha256: STRING_GUTS_MEM_GET_SHA256,
        direct_calls: &[
            (0x00a1_7aaa, STRING_CONSTRUCTOR_VA),
            (0x00a1_7ac7, 0x00a2_e550),
            (0x00a1_7ad5, STRING_CLOSE_VA),
            (0x00a1_7ae4, STRING_CLOSE_VA),
        ],
        indirect_import_calls: &[(0x00a1_7b6b, MALLOC_IAT_VA)],
    };

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StringConstructorCallerBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
    pub direct_calls: &'static [(u32, u32)],
}

pub const MAP_MAKE_POST_FIX_DIAG_CONSTRUCTOR_CALLER_BODY: StringConstructorCallerBody =
    StringConstructorCallerBody {
        entry_va: MAP_MAKE_POST_FIX_DIAG_STRING_CONSTRUCTOR_CALL_VA,
        end_va_exclusive: MAP_MAKE_POST_FIX_DIAG_CONSTRUCTOR_CALLER_END_VA,
        size: MAP_MAKE_POST_FIX_DIAG_CONSTRUCTOR_CALLER_SIZE,
        instruction_count: MAP_MAKE_POST_FIX_DIAG_CONSTRUCTOR_CALLER_INSTRUCTION_COUNT,
        sha256: MAP_MAKE_POST_FIX_DIAG_CONSTRUCTOR_CALLER_SHA256,
        direct_calls: &[(
            MAP_MAKE_POST_FIX_DIAG_STRING_CONSTRUCTOR_CALL_VA,
            STRING_CONSTRUCTOR_VA,
        )],
    };

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapMakePostFixDiagLocalString {
    pub source_va: u32,
    pub source: &'static str,
    pub source_bytes_with_nul: usize,
    pub source_sha256: &'static str,
    pub code_page: u32,
    pub utf16: Vec<u16>,
    pub length: u16,
    pub capacity: u16,
    pub offset: u16,
    pub flags: u8,
    pub module_id: u8,
    pub hash: u32,
    pub insensitive_hash: u32,
    pub owns_typed_string_guts: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MapMakePostFixDiagStringAllocationOwner {
    CallerLocalMapCpp,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MapMakePostFixDiagStringAllocationReceipt {
    pub owner: MapMakePostFixDiagStringAllocationOwner,
    pub string_guts_length: u16,
    pub string_guts_capacity: u16,
    pub utf16_units_allocated_with_nul: usize,
    pub host_pointer_recorded: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MapMakePostFixDiagGameLogCall {
    pub source_load_va: u32,
    pub line_push_va: u32,
    pub line_number: u32,
    pub source_push_va: u32,
    pub mode_push_va: u32,
    pub mode: i32,
    pub this_load_va: u32,
    pub game_log_va: u32,
    pub call_va: u32,
    pub primitive_va: u32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MapMakePostFixDiagStringConstructorNext {
    GameLogSayChecksum(MapMakePostFixDiagGameLogCall),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapMakePostFixDiagStringConstructorReceipt {
    pub caller: StringConstructorCallerBody,
    pub constructor: StringConstructorNativeBody,
    pub init_const: StringInitConstNativeBody,
    pub reinit: StringReinitNativeBody,
    pub get_string_guts: StringConstructorHelperNativeBody,
    pub string_guts_operator_new: StringConstructorHelperNativeBody,
    pub string_guts_mem_get: StringConstructorHelperNativeBody,
    pub char_to_wchar: StringConstructorHelperNativeBody,
    pub multi_byte_to_wide_char_iat_va: u32,
    pub executed_direct_calls: Vec<(u32, u32)>,
    pub executed_indirect_import_calls: Vec<(u32, u32)>,
    pub local: MapMakePostFixDiagLocalString,
    pub allocation: MapMakePostFixDiagStringAllocationReceipt,
    pub cleanup_guard_store_va: u32,
    pub cleanup_guard_after: u8,
    pub world_before: WorldChecksum,
    pub world_after: WorldChecksum,
    pub world_sections_changed: Vec<WorldSection>,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub direct_rng_sites: Vec<u32>,
    pub next: MapMakePostFixDiagStringConstructorNext,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapMakePostFixDiagStringConstructorError {
    PriorFixDiagLandReceiptMismatch,
}

fn map_make_post_fix_diag_local_string() -> MapMakePostFixDiagLocalString {
    let utf16 = MAP_MAKE_POST_FIX_DIAG_LITERAL
        .encode_utf16()
        .collect::<Vec<_>>();
    MapMakePostFixDiagLocalString {
        source_va: MAP_MAKE_POST_FIX_DIAG_LITERAL_VA,
        source: MAP_MAKE_POST_FIX_DIAG_LITERAL,
        source_bytes_with_nul: MAP_MAKE_POST_FIX_DIAG_LITERAL_BYTES_WITH_NUL,
        source_sha256: MAP_MAKE_POST_FIX_DIAG_LITERAL_SHA256,
        code_page: 0x0000_fde9,
        length: u16::try_from(utf16.len()).expect("map.cpp length fits retail String"),
        capacity: u16::try_from(utf16.len()).expect("map.cpp capacity fits retail String"),
        offset: 0,
        flags: 0,
        module_id: 0,
        hash: 0,
        insensitive_hash: 0,
        owns_typed_string_guts: true,
        utf16,
    }
}

/// Execute the exact post-diagonal `String::String(char const*)` constructor,
/// its complete internal init path for the shipped nonempty `map.cpp` literal,
/// and the caller-local guard/argument staging. Freeze before GameLog mutates.
pub fn execute_map_make_post_fix_diag_string_constructor(
    world: &World,
    regions: &Regions,
    random_state: i32,
    prior_clear: &MapMakeFirstRegionsClearAllReceipt,
    prior_find_all: &MapMakeFirstRegionsFindAllReceipt,
    prior_limits: &MapMakeTerritoryLimitsReceipt,
    prior_fix_diag: &MapFixDiagLandReceipt,
) -> Result<MapMakePostFixDiagStringConstructorReceipt, MapMakePostFixDiagStringConstructorError> {
    if !validate_map_fix_diag_land_receipt(
        world,
        regions,
        prior_clear,
        prior_find_all,
        prior_limits,
        prior_fix_diag,
    ) || prior_fix_diag.random_state_after != random_state
    {
        return Err(MapMakePostFixDiagStringConstructorError::PriorFixDiagLandReceiptMismatch);
    }
    let world_before = world.checksum_sections();
    let local = map_make_post_fix_diag_local_string();
    let world_after = world.checksum_sections();
    Ok(MapMakePostFixDiagStringConstructorReceipt {
        caller: MAP_MAKE_POST_FIX_DIAG_CONSTRUCTOR_CALLER_BODY,
        constructor: STRING_CONSTRUCTOR_NATIVE_BODY,
        init_const: STRING_INIT_CONST_NATIVE_BODY,
        reinit: STRING_REINIT_NATIVE_BODY,
        get_string_guts: STRING_GET_STRING_GUTS_NATIVE_BODY,
        string_guts_operator_new: STRING_GUTS_OPERATOR_NEW_NATIVE_BODY,
        string_guts_mem_get: STRING_GUTS_MEM_GET_NATIVE_BODY,
        char_to_wchar: STRING_CHAR_TO_WCHAR_NATIVE_BODY,
        multi_byte_to_wide_char_iat_va: MULTI_BYTE_TO_WIDE_CHAR_IAT_VA,
        executed_direct_calls: vec![
            (
                MAP_MAKE_POST_FIX_DIAG_STRING_CONSTRUCTOR_CALL_VA,
                STRING_CONSTRUCTOR_VA,
            ),
            (0x00a1_d673, STRING_INIT_CONST_VA),
            (0x00a1_702f, STRING_REINIT_VA),
            (0x00a1_6254, STRING_GET_STRING_GUTS_VA),
            (0x00a1_7bb7, STRING_GUTS_OPERATOR_NEW_VA),
            (0x00a1_7be3, STRING_GUTS_MEM_GET_VA),
            (0x00a1_703c, STRING_CHAR_TO_WCHAR_VA),
        ],
        executed_indirect_import_calls: vec![(0x00a1_7c60, MULTI_BYTE_TO_WIDE_CHAR_IAT_VA)],
        allocation: MapMakePostFixDiagStringAllocationReceipt {
            owner: MapMakePostFixDiagStringAllocationOwner::CallerLocalMapCpp,
            string_guts_length: local.length,
            string_guts_capacity: local.capacity,
            utf16_units_allocated_with_nul: local.utf16.len() + 1,
            host_pointer_recorded: false,
        },
        local,
        cleanup_guard_store_va: MAP_MAKE_POST_FIX_DIAG_GUARD_STORE_VA,
        cleanup_guard_after: 4,
        world_sections_changed: world_before.differing_sections(&world_after),
        world_before,
        world_after,
        random_state_before: random_state,
        random_state_after: random_state,
        direct_rng_sites: Vec::new(),
        next: MapMakePostFixDiagStringConstructorNext::GameLogSayChecksum(
            MapMakePostFixDiagGameLogCall {
                source_load_va: MAP_MAKE_POST_FIX_DIAG_GAME_LOG_SOURCE_LOAD_VA,
                line_push_va: MAP_MAKE_POST_FIX_DIAG_GAME_LOG_LINE_PUSH_VA,
                line_number: MAP_MAKE_POST_FIX_DIAG_GAME_LOG_LINE_NUMBER,
                source_push_va: MAP_MAKE_POST_FIX_DIAG_GAME_LOG_SOURCE_PUSH_VA,
                mode_push_va: MAP_MAKE_POST_FIX_DIAG_GAME_LOG_MODE_PUSH_VA,
                mode: MAP_MAKE_POST_FIX_DIAG_GAME_LOG_MODE,
                this_load_va: MAP_MAKE_POST_FIX_DIAG_GAME_LOG_THIS_LOAD_VA,
                game_log_va: GAME_LOG_GLOBAL_VA,
                call_va: MAP_MAKE_POST_FIX_DIAG_GAME_LOG_CALL_VA,
                primitive_va: GAME_LOG_SAY_CHECKSUM_VA,
            },
        ),
    })
}

pub(crate) fn validate_map_make_post_fix_diag_string_constructor_receipt(
    world: &World,
    regions: &Regions,
    prior_clear: &MapMakeFirstRegionsClearAllReceipt,
    prior_find_all: &MapMakeFirstRegionsFindAllReceipt,
    prior_limits: &MapMakeTerritoryLimitsReceipt,
    prior_fix_diag: &MapFixDiagLandReceipt,
    receipt: &MapMakePostFixDiagStringConstructorReceipt,
) -> bool {
    validate_map_fix_diag_land_receipt(
        world,
        regions,
        prior_clear,
        prior_find_all,
        prior_limits,
        prior_fix_diag,
    ) && receipt.caller == MAP_MAKE_POST_FIX_DIAG_CONSTRUCTOR_CALLER_BODY
        && receipt.constructor == STRING_CONSTRUCTOR_NATIVE_BODY
        && receipt.init_const == STRING_INIT_CONST_NATIVE_BODY
        && receipt.reinit == STRING_REINIT_NATIVE_BODY
        && receipt.get_string_guts == STRING_GET_STRING_GUTS_NATIVE_BODY
        && receipt.string_guts_operator_new == STRING_GUTS_OPERATOR_NEW_NATIVE_BODY
        && receipt.string_guts_mem_get == STRING_GUTS_MEM_GET_NATIVE_BODY
        && receipt.char_to_wchar == STRING_CHAR_TO_WCHAR_NATIVE_BODY
        && receipt.multi_byte_to_wide_char_iat_va == MULTI_BYTE_TO_WIDE_CHAR_IAT_VA
        && receipt.executed_direct_calls
            == [
                (
                    MAP_MAKE_POST_FIX_DIAG_STRING_CONSTRUCTOR_CALL_VA,
                    STRING_CONSTRUCTOR_VA,
                ),
                (0x00a1_d673, STRING_INIT_CONST_VA),
                (0x00a1_702f, STRING_REINIT_VA),
                (0x00a1_6254, STRING_GET_STRING_GUTS_VA),
                (0x00a1_7bb7, STRING_GUTS_OPERATOR_NEW_VA),
                (0x00a1_7be3, STRING_GUTS_MEM_GET_VA),
                (0x00a1_703c, STRING_CHAR_TO_WCHAR_VA),
            ]
        && receipt.executed_indirect_import_calls == [(0x00a1_7c60, MULTI_BYTE_TO_WIDE_CHAR_IAT_VA)]
        && receipt.local == map_make_post_fix_diag_local_string()
        && receipt.allocation
            == (MapMakePostFixDiagStringAllocationReceipt {
                owner: MapMakePostFixDiagStringAllocationOwner::CallerLocalMapCpp,
                string_guts_length: 7,
                string_guts_capacity: 7,
                utf16_units_allocated_with_nul: 8,
                host_pointer_recorded: false,
            })
        && receipt.cleanup_guard_store_va == MAP_MAKE_POST_FIX_DIAG_GUARD_STORE_VA
        && receipt.cleanup_guard_after == 4
        && receipt.world_before == prior_fix_diag.world_after
        && receipt.world_before == receipt.world_after
        && receipt.world_after == world.checksum_sections()
        && receipt.world_sections_changed.is_empty()
        && receipt.random_state_before == prior_fix_diag.random_state_after
        && receipt.random_state_before == receipt.random_state_after
        && receipt.direct_rng_sites.is_empty()
        && receipt.next
            == MapMakePostFixDiagStringConstructorNext::GameLogSayChecksum(
                MapMakePostFixDiagGameLogCall {
                    source_load_va: MAP_MAKE_POST_FIX_DIAG_GAME_LOG_SOURCE_LOAD_VA,
                    line_push_va: MAP_MAKE_POST_FIX_DIAG_GAME_LOG_LINE_PUSH_VA,
                    line_number: MAP_MAKE_POST_FIX_DIAG_GAME_LOG_LINE_NUMBER,
                    source_push_va: MAP_MAKE_POST_FIX_DIAG_GAME_LOG_SOURCE_PUSH_VA,
                    mode_push_va: MAP_MAKE_POST_FIX_DIAG_GAME_LOG_MODE_PUSH_VA,
                    mode: MAP_MAKE_POST_FIX_DIAG_GAME_LOG_MODE,
                    this_load_va: MAP_MAKE_POST_FIX_DIAG_GAME_LOG_THIS_LOAD_VA,
                    game_log_va: GAME_LOG_GLOBAL_VA,
                    call_va: MAP_MAKE_POST_FIX_DIAG_GAME_LOG_CALL_VA,
                    primitive_va: GAME_LOG_SAY_CHECKSUM_VA,
                },
            )
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct GameLogSayChecksumNativeBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub ret_va: u32,
    pub callee_stack_argument_bytes_popped: u8,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
    pub check_accept_call_va: u32,
    pub check_accept_va: u32,
    pub first_virtual_sink_call_va: u32,
    pub frame_rollover_call_va: u32,
    pub frame_rollover_va: u32,
}

pub const GAME_LOG_SAY_CHECKSUM_NATIVE_BODY: GameLogSayChecksumNativeBody =
    GameLogSayChecksumNativeBody {
        entry_va: GAME_LOG_SAY_CHECKSUM_VA,
        end_va_exclusive: GAME_LOG_SAY_CHECKSUM_END_VA,
        ret_va: GAME_LOG_SAY_CHECKSUM_RET_VA,
        callee_stack_argument_bytes_popped: 12,
        size: GAME_LOG_SAY_CHECKSUM_SIZE,
        instruction_count: GAME_LOG_SAY_CHECKSUM_INSTRUCTION_COUNT,
        sha256: GAME_LOG_SAY_CHECKSUM_SHA256,
        check_accept_call_va: GAME_LOG_CHECK_ACCEPT_CALL_VA,
        check_accept_va: GAME_LOG_CHECK_ACCEPT_VA,
        first_virtual_sink_call_va: GAME_LOG_FIRST_VIRTUAL_SINK_CALL_VA,
        frame_rollover_call_va: GAME_LOG_FRAME_ROLLOVER_CALL_VA,
        frame_rollover_va: GAME_LOG_FRAME_ROLLOVER_VA,
    };

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct GameLogCheckAcceptNativeBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub ret_va: u32,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
    pub reentrancy_guard_va: u32,
}

pub const GAME_LOG_CHECK_ACCEPT_NATIVE_BODY: GameLogCheckAcceptNativeBody =
    GameLogCheckAcceptNativeBody {
        entry_va: GAME_LOG_CHECK_ACCEPT_VA,
        end_va_exclusive: GAME_LOG_CHECK_ACCEPT_END_VA,
        ret_va: GAME_LOG_CHECK_ACCEPT_RET_VA,
        size: GAME_LOG_CHECK_ACCEPT_SIZE,
        instruction_count: GAME_LOG_CHECK_ACCEPT_INSTRUCTION_COUNT,
        sha256: GAME_LOG_CHECK_ACCEPT_SHA256,
        reentrancy_guard_va: GAME_LOG_REENTRANCY_GUARD_VA,
    };

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MapMakePostChecksumCallerBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
}

pub const MAP_MAKE_POST_CHECKSUM_CALLER_BODY: MapMakePostChecksumCallerBody =
    MapMakePostChecksumCallerBody {
        entry_va: MAP_MAKE_POST_FIX_DIAG_GAME_LOG_CALL_VA,
        end_va_exclusive: MAP_MAKE_POST_CHECKSUM_CALLER_END_VA,
        size: MAP_MAKE_POST_CHECKSUM_CALLER_SIZE,
        instruction_count: MAP_MAKE_POST_CHECKSUM_CALLER_INSTRUCTION_COUNT,
        sha256: MAP_MAKE_POST_CHECKSUM_CALLER_SHA256,
    };

/// Exact owner-relative effects of `GameLog::say_checksum`. The replay does
/// not have a live `GameLog` singleton, so data-dependent acceptance, output,
/// and frame rollover are retained as host observations rather than invented
/// concrete state. The unconditional sequence delta and temporary field
/// schedule are exact.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MapMakePostFixDiagGameLogOwnerReceipt {
    pub owner_va: u32,
    pub source_owner: MapMakePostFixDiagStringAllocationOwner,
    pub source_passed_by_const_reference: bool,
    pub source_preserved: bool,
    pub category_offset: u32,
    pub temporary_category: i32,
    pub previous_category_restored: bool,
    pub mode_offset: u32,
    pub temporary_mode: i32,
    pub previous_nonnegative_mode_restored: bool,
    pub frame_callback_flag_offset: u32,
    pub checksum_sequence_offset: u32,
    pub checksum_sequence_delta: u32,
    pub rollover_sequence_offset: u32,
    pub break_sequence_offset: u32,
    pub acceptance_is_host_state_dependent: bool,
    pub sink_output_is_host_state_dependent: bool,
    pub frame_rollover_is_host_state_dependent: bool,
    pub host_pointer_recorded: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MapMakePostFixDiagGameLogNext {
    StringClose {
        local_load_va: u32,
        call_va: u32,
        primitive_va: u32,
        allocation_owner: MapMakePostFixDiagStringAllocationOwner,
        may_release_owned_string_guts: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapMakePostFixDiagGameLogReceipt {
    pub caller: MapMakePostChecksumCallerBody,
    pub body: GameLogSayChecksumNativeBody,
    pub check_accept: GameLogCheckAcceptNativeBody,
    pub call: MapMakePostFixDiagGameLogCall,
    pub owner: MapMakePostFixDiagGameLogOwnerReceipt,
    pub source_before: MapMakePostFixDiagLocalString,
    pub source_after: MapMakePostFixDiagLocalString,
    pub cleanup_guard_store_va: u32,
    pub cleanup_guard_after: i32,
    pub world_before: WorldChecksum,
    pub world_after: WorldChecksum,
    pub world_sections_changed: Vec<WorldSection>,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub direct_rng_sites: Vec<u32>,
    pub next: MapMakePostFixDiagGameLogNext,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapMakePostFixDiagGameLogError {
    PriorStringConstructorReceiptMismatch,
}

fn map_make_post_fix_diag_game_log_call() -> MapMakePostFixDiagGameLogCall {
    MapMakePostFixDiagGameLogCall {
        source_load_va: MAP_MAKE_POST_FIX_DIAG_GAME_LOG_SOURCE_LOAD_VA,
        line_push_va: MAP_MAKE_POST_FIX_DIAG_GAME_LOG_LINE_PUSH_VA,
        line_number: MAP_MAKE_POST_FIX_DIAG_GAME_LOG_LINE_NUMBER,
        source_push_va: MAP_MAKE_POST_FIX_DIAG_GAME_LOG_SOURCE_PUSH_VA,
        mode_push_va: MAP_MAKE_POST_FIX_DIAG_GAME_LOG_MODE_PUSH_VA,
        mode: MAP_MAKE_POST_FIX_DIAG_GAME_LOG_MODE,
        this_load_va: MAP_MAKE_POST_FIX_DIAG_GAME_LOG_THIS_LOAD_VA,
        game_log_va: GAME_LOG_GLOBAL_VA,
        call_va: MAP_MAKE_POST_FIX_DIAG_GAME_LOG_CALL_VA,
        primitive_va: GAME_LOG_SAY_CHECKSUM_VA,
    }
}

fn map_make_post_fix_diag_game_log_owner() -> MapMakePostFixDiagGameLogOwnerReceipt {
    MapMakePostFixDiagGameLogOwnerReceipt {
        owner_va: GAME_LOG_GLOBAL_VA,
        source_owner: MapMakePostFixDiagStringAllocationOwner::CallerLocalMapCpp,
        source_passed_by_const_reference: true,
        source_preserved: true,
        category_offset: GAME_LOG_CATEGORY_OFFSET,
        temporary_category: GAME_LOG_CHECKSUM_CATEGORY,
        previous_category_restored: true,
        mode_offset: GAME_LOG_MODE_OFFSET,
        temporary_mode: MAP_MAKE_POST_FIX_DIAG_GAME_LOG_MODE,
        previous_nonnegative_mode_restored: true,
        frame_callback_flag_offset: GAME_LOG_FRAME_CALLBACK_FLAG_OFFSET,
        checksum_sequence_offset: GAME_LOG_CHECKSUM_SEQUENCE_OFFSET,
        checksum_sequence_delta: 1,
        rollover_sequence_offset: GAME_LOG_ROLLOVER_SEQUENCE_OFFSET,
        break_sequence_offset: GAME_LOG_BREAK_SEQUENCE_OFFSET,
        acceptance_is_host_state_dependent: true,
        sink_output_is_host_state_dependent: true,
        frame_rollover_is_host_state_dependent: true,
        host_pointer_recorded: false,
    }
}

/// Execute the exact checksum-log invocation as a typed host observation,
/// then execute the caller guard clear and local-address load. The local
/// `map.cpp` allocation is preserved across the const-reference call. Freeze
/// before `String::close`, whose sole-owner path may release that allocation.
pub fn execute_map_make_post_fix_diag_game_log_say_checksum(
    world: &World,
    regions: &Regions,
    random_state: i32,
    prior_clear: &MapMakeFirstRegionsClearAllReceipt,
    prior_find_all: &MapMakeFirstRegionsFindAllReceipt,
    prior_limits: &MapMakeTerritoryLimitsReceipt,
    prior_fix_diag: &MapFixDiagLandReceipt,
    prior_string: &MapMakePostFixDiagStringConstructorReceipt,
) -> Result<MapMakePostFixDiagGameLogReceipt, MapMakePostFixDiagGameLogError> {
    if !validate_map_make_post_fix_diag_string_constructor_receipt(
        world,
        regions,
        prior_clear,
        prior_find_all,
        prior_limits,
        prior_fix_diag,
        prior_string,
    ) || prior_string.random_state_after != random_state
    {
        return Err(MapMakePostFixDiagGameLogError::PriorStringConstructorReceiptMismatch);
    }
    let world_before = world.checksum_sections();
    let source_before = prior_string.local.clone();
    let source_after = source_before.clone();
    let world_after = world.checksum_sections();
    Ok(MapMakePostFixDiagGameLogReceipt {
        caller: MAP_MAKE_POST_CHECKSUM_CALLER_BODY,
        body: GAME_LOG_SAY_CHECKSUM_NATIVE_BODY,
        check_accept: GAME_LOG_CHECK_ACCEPT_NATIVE_BODY,
        call: map_make_post_fix_diag_game_log_call(),
        owner: map_make_post_fix_diag_game_log_owner(),
        source_before,
        source_after,
        cleanup_guard_store_va: MAP_MAKE_POST_CHECKSUM_GUARD_STORE_VA,
        cleanup_guard_after: -1,
        world_sections_changed: world_before.differing_sections(&world_after),
        world_before,
        world_after,
        random_state_before: random_state,
        random_state_after: random_state,
        direct_rng_sites: Vec::new(),
        next: MapMakePostFixDiagGameLogNext::StringClose {
            local_load_va: MAP_MAKE_POST_CHECKSUM_STRING_LOCAL_LOAD_VA,
            call_va: MAP_MAKE_POST_CHECKSUM_STRING_CLOSE_CALL_VA,
            primitive_va: STRING_CLOSE_VA,
            allocation_owner: MapMakePostFixDiagStringAllocationOwner::CallerLocalMapCpp,
            may_release_owned_string_guts: true,
        },
    })
}

pub(crate) fn validate_map_make_post_fix_diag_game_log_receipt(
    world: &World,
    regions: &Regions,
    prior_clear: &MapMakeFirstRegionsClearAllReceipt,
    prior_find_all: &MapMakeFirstRegionsFindAllReceipt,
    prior_limits: &MapMakeTerritoryLimitsReceipt,
    prior_fix_diag: &MapFixDiagLandReceipt,
    prior_string: &MapMakePostFixDiagStringConstructorReceipt,
    receipt: &MapMakePostFixDiagGameLogReceipt,
) -> bool {
    validate_map_make_post_fix_diag_string_constructor_receipt(
        world,
        regions,
        prior_clear,
        prior_find_all,
        prior_limits,
        prior_fix_diag,
        prior_string,
    ) && receipt.caller == MAP_MAKE_POST_CHECKSUM_CALLER_BODY
        && receipt.body == GAME_LOG_SAY_CHECKSUM_NATIVE_BODY
        && receipt.check_accept == GAME_LOG_CHECK_ACCEPT_NATIVE_BODY
        && receipt.call == map_make_post_fix_diag_game_log_call()
        && receipt.owner == map_make_post_fix_diag_game_log_owner()
        && receipt.source_before == prior_string.local
        && receipt.source_before == receipt.source_after
        && receipt.source_after.owns_typed_string_guts
        && receipt.cleanup_guard_store_va == MAP_MAKE_POST_CHECKSUM_GUARD_STORE_VA
        && receipt.cleanup_guard_after == -1
        && receipt.world_before == prior_string.world_after
        && receipt.world_before == receipt.world_after
        && receipt.world_after == world.checksum_sections()
        && receipt.world_sections_changed.is_empty()
        && receipt.random_state_before == prior_string.random_state_after
        && receipt.random_state_before == receipt.random_state_after
        && receipt.direct_rng_sites.is_empty()
        && receipt.next
            == (MapMakePostFixDiagGameLogNext::StringClose {
                local_load_va: MAP_MAKE_POST_CHECKSUM_STRING_LOCAL_LOAD_VA,
                call_va: MAP_MAKE_POST_CHECKSUM_STRING_CLOSE_CALL_VA,
                primitive_va: STRING_CLOSE_VA,
                allocation_owner: MapMakePostFixDiagStringAllocationOwner::CallerLocalMapCpp,
                may_release_owned_string_guts: true,
            })
}

/// Frozen instruction-level body for one reached `String::close` child.
/// Calls listed here are possible in the whole body; the receipt below also
/// pins the narrower direct-call sequence reached by the ordinary pool path.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StringCloseChildNativeBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub ret_vas: &'static [u32],
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
}

pub const STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_BODY: StringCloseChildNativeBody =
    StringCloseChildNativeBody {
        entry_va: STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_VA,
        end_va_exclusive: STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_END_VA,
        ret_vas: &[STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_RET_VA],
        size: STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_SIZE,
        instruction_count: STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_INSTRUCTION_COUNT,
        sha256: STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_SHA256,
    };

pub const STRING_GUTS_MEM_FREE_BODY: StringCloseChildNativeBody = StringCloseChildNativeBody {
    entry_va: STRING_GUTS_MEM_FREE_VA,
    end_va_exclusive: STRING_GUTS_MEM_FREE_END_VA,
    ret_vas: &[STRING_GUTS_MEM_FREE_RET_VA],
    size: STRING_GUTS_MEM_FREE_SIZE,
    instruction_count: STRING_GUTS_MEM_FREE_INSTRUCTION_COUNT,
    sha256: STRING_GUTS_MEM_FREE_SHA256,
};

pub const STRING_GUTS_OPERATOR_DELETE_BODY: StringCloseChildNativeBody =
    StringCloseChildNativeBody {
        entry_va: STRING_GUTS_OPERATOR_DELETE_VA,
        end_va_exclusive: STRING_GUTS_OPERATOR_DELETE_END_VA,
        ret_vas: &STRING_GUTS_OPERATOR_DELETE_RET_VAS,
        size: STRING_GUTS_OPERATOR_DELETE_SIZE,
        instruction_count: STRING_GUTS_OPERATOR_DELETE_INSTRUCTION_COUNT,
        sha256: STRING_GUTS_OPERATOR_DELETE_SHA256,
    };

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MapMakePostChecksumStringCloseCallerBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
}

pub const MAP_MAKE_POST_CHECKSUM_STRING_CLOSE_CALLER_BODY:
    MapMakePostChecksumStringCloseCallerBody = MapMakePostChecksumStringCloseCallerBody {
    entry_va: MAP_MAKE_POST_CHECKSUM_STRING_CLOSE_CALL_VA,
    end_va_exclusive: MAP_MAKE_POST_CHECKSUM_STRING_CLOSE_RESUME_VA,
    size: MAP_MAKE_POST_CHECKSUM_STRING_CLOSE_CALL_SIZE,
    instruction_count: MAP_MAKE_POST_CHECKSUM_STRING_CLOSE_CALL_INSTRUCTION_COUNT,
    sha256: MAP_MAKE_POST_CHECKSUM_STRING_CLOSE_CALL_SHA256,
};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StringCloseNativeBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub ret_va: u32,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
    pub scalar_deleting_destructor_call_va: u32,
    pub scalar_deleting_destructor_va: u32,
}

pub const MAP_MAKE_STRING_CLOSE_NATIVE_BODY: StringCloseNativeBody = StringCloseNativeBody {
    entry_va: STRING_CLOSE_VA,
    end_va_exclusive: STRING_CLOSE_END_VA,
    ret_va: STRING_CLOSE_RET_VA,
    size: STRING_CLOSE_SIZE,
    instruction_count: STRING_CLOSE_INSTRUCTION_COUNT,
    sha256: STRING_CLOSE_SHA256,
    scalar_deleting_destructor_call_va: STRING_CLOSE_GUTS_DESTRUCTOR_CALL_VA,
    scalar_deleting_destructor_va: STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_VA,
};

/// These are the initialized retail allocator flags on the ordinary game
/// path. They select the two return-to-pool arms. Pool growth remains an
/// allocator-local observation and does not expose a native pointer.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MapMakeStringAllocatorFacts {
    pub shutdown_in_progress: bool,
    pub debug_heap_enabled: bool,
    pub duplicate_guts_guard_raised: bool,
}

impl MapMakeStringAllocatorFacts {
    pub const RETAIL_GAMEPLAY: Self = Self {
        shutdown_in_progress: false,
        debug_heap_enabled: false,
        duplicate_guts_guard_raised: false,
    };
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MapMakeStringAllocationState {
    Live,
    ReturnedToRetailPool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapMakePostChecksumStringCloseAllocationReceipt {
    pub owner: MapMakePostFixDiagStringAllocationOwner,
    pub source_utf16: Vec<u16>,
    pub buffer_units_with_nul: usize,
    pub buffer_pool_class: u8,
    pub string_guts_logical_size: u8,
    pub buffer_before: MapMakeStringAllocationState,
    pub buffer_after: MapMakeStringAllocationState,
    pub string_guts_before: MapMakeStringAllocationState,
    pub string_guts_after: MapMakeStringAllocationState,
    pub buffer_pool_growth_is_allocator_state_dependent: bool,
    pub string_guts_pool_growth_is_allocator_state_dependent: bool,
    pub host_pointer_recorded: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MapMakePostChecksumClosedString {
    pub data_is_null: bool,
    pub offset: u16,
    pub length: u16,
    pub flags: u8,
    pub module_id: u8,
    pub hash: u32,
    pub insensitive_hash: u32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MapMakePostCloseProgressPrepBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
}

pub const MAP_MAKE_POST_CLOSE_PROGRESS_PREP_BODY: MapMakePostCloseProgressPrepBody =
    MapMakePostCloseProgressPrepBody {
        entry_va: MAP_MAKE_POST_CLOSE_PROGRESS_TEST_VA,
        end_va_exclusive: MAP_MAKE_POST_CLOSE_PROGRESS_PREP_END_VA,
        size: MAP_MAKE_POST_CLOSE_PROGRESS_PREP_SIZE,
        instruction_count: MAP_MAKE_POST_CLOSE_PROGRESS_PREP_INSTRUCTION_COUNT,
        sha256: MAP_MAKE_POST_CLOSE_PROGRESS_PREP_SHA256,
    };

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MapMakePostChecksumStringCloseNext {
    ProgressStringConstructor {
        prep: MapMakePostCloseProgressPrepBody,
        progress_test_va: u32,
        progress_branch_va: u32,
        no_progress_target_va: u32,
        progress_requested: bool,
        string_table_load_va: u32,
        string_table_ptr_va: u32,
        string_byte_offset: u32,
        local_load_va: u32,
        source_push_va: u32,
        call_va: u32,
        primitive_va: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapMakePostChecksumStringCloseReceipt {
    pub caller: MapMakePostChecksumStringCloseCallerBody,
    pub body: StringCloseNativeBody,
    pub scalar_deleting_destructor: StringCloseChildNativeBody,
    pub mem_free: StringCloseChildNativeBody,
    pub operator_delete: StringCloseChildNativeBody,
    pub allocator_facts: MapMakeStringAllocatorFacts,
    pub executed_direct_calls: Vec<(u32, u32)>,
    pub executed_indirect_import_calls: Vec<(u32, u32)>,
    pub source_before: MapMakePostFixDiagLocalString,
    pub local_after: MapMakePostChecksumClosedString,
    pub allocation: MapMakePostChecksumStringCloseAllocationReceipt,
    pub world_before: WorldChecksum,
    pub world_after: WorldChecksum,
    pub world_sections_changed: Vec<WorldSection>,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub direct_rng_sites: Vec<u32>,
    pub next: MapMakePostChecksumStringCloseNext,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapMakePostChecksumStringCloseError {
    PriorGameLogReceiptMismatch,
    AllocatorFactsUnavailable,
}

fn map_make_post_checksum_closed_string() -> MapMakePostChecksumClosedString {
    MapMakePostChecksumClosedString {
        data_is_null: true,
        offset: 0,
        length: 0,
        flags: 0,
        module_id: 0,
        hash: 0,
        insensitive_hash: 0,
    }
}

fn map_make_post_checksum_string_close_next() -> MapMakePostChecksumStringCloseNext {
    MapMakePostChecksumStringCloseNext::ProgressStringConstructor {
        prep: MAP_MAKE_POST_CLOSE_PROGRESS_PREP_BODY,
        progress_test_va: MAP_MAKE_POST_CLOSE_PROGRESS_TEST_VA,
        progress_branch_va: MAP_MAKE_POST_CLOSE_PROGRESS_BRANCH_VA,
        no_progress_target_va: MAP_MAKE_POST_CLOSE_NO_PROGRESS_TARGET_VA,
        progress_requested: true,
        string_table_load_va: MAP_MAKE_PROGRESS_STRING_TABLE_LOAD_VA,
        string_table_ptr_va: MAP_MAKE_PROGRESS_STRING_TABLE_PTR_VA,
        string_byte_offset: MAP_MAKE_PROGRESS_STRING_BYTE_OFFSET,
        local_load_va: MAP_MAKE_PROGRESS_STRING_LOCAL_LOAD_VA,
        source_push_va: MAP_MAKE_PROGRESS_STRING_SOURCE_PUSH_VA,
        call_va: MAP_MAKE_PROGRESS_STRING_CONSTRUCTOR_CALL_VA,
        primitive_va: STRING_COPY_CONSTRUCTOR_VA,
    }
}

/// Execute the sole-owner `String::close` reached at `0x0068bead`.
///
/// The preceding constructor created an ordinary, seven-unit heap string with
/// refcount zero. Retail therefore reaches the scalar-deleting destructor,
/// returns the sixteen-byte UTF-16 allocation to buffer pool class one, then
/// returns the sixteen-byte `StringGuts` object to its own pool. Native pointer
/// identity and allocator freelist contents deliberately remain outside the
/// receipt. The replay call chain fixes `Map::make`'s progress parameter to
/// one, so the read-only branch preparation is also executed and the next
/// mutator is the progress-message `String` copy constructor at `0x0068bec4`.
pub fn execute_map_make_post_checksum_string_close(
    world: &World,
    regions: &Regions,
    random_state: i32,
    prior_clear: &MapMakeFirstRegionsClearAllReceipt,
    prior_find_all: &MapMakeFirstRegionsFindAllReceipt,
    prior_limits: &MapMakeTerritoryLimitsReceipt,
    prior_fix_diag: &MapFixDiagLandReceipt,
    prior_string: &MapMakePostFixDiagStringConstructorReceipt,
    prior_log: &MapMakePostFixDiagGameLogReceipt,
    allocator_facts: MapMakeStringAllocatorFacts,
) -> Result<MapMakePostChecksumStringCloseReceipt, MapMakePostChecksumStringCloseError> {
    if !validate_map_make_post_fix_diag_game_log_receipt(
        world,
        regions,
        prior_clear,
        prior_find_all,
        prior_limits,
        prior_fix_diag,
        prior_string,
        prior_log,
    ) || prior_log.random_state_after != random_state
    {
        return Err(MapMakePostChecksumStringCloseError::PriorGameLogReceiptMismatch);
    }
    if allocator_facts != MapMakeStringAllocatorFacts::RETAIL_GAMEPLAY {
        return Err(MapMakePostChecksumStringCloseError::AllocatorFactsUnavailable);
    }
    let world_before = world.checksum_sections();
    let source_before = prior_log.source_after.clone();
    let world_after = world.checksum_sections();
    Ok(MapMakePostChecksumStringCloseReceipt {
        caller: MAP_MAKE_POST_CHECKSUM_STRING_CLOSE_CALLER_BODY,
        body: MAP_MAKE_STRING_CLOSE_NATIVE_BODY,
        scalar_deleting_destructor: STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_BODY,
        mem_free: STRING_GUTS_MEM_FREE_BODY,
        operator_delete: STRING_GUTS_OPERATOR_DELETE_BODY,
        allocator_facts,
        executed_direct_calls: vec![
            (MAP_MAKE_POST_CHECKSUM_STRING_CLOSE_CALL_VA, STRING_CLOSE_VA),
            (
                STRING_CLOSE_GUTS_DESTRUCTOR_CALL_VA,
                STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_VA,
            ),
            (STRING_GUTS_MEM_FREE_CALL_VA, STRING_GUTS_MEM_FREE_VA),
            (
                STRING_GUTS_OPERATOR_DELETE_CALL_VA,
                STRING_GUTS_OPERATOR_DELETE_VA,
            ),
        ],
        executed_indirect_import_calls: Vec::new(),
        allocation: MapMakePostChecksumStringCloseAllocationReceipt {
            owner: MapMakePostFixDiagStringAllocationOwner::CallerLocalMapCpp,
            source_utf16: source_before.utf16.clone(),
            buffer_units_with_nul: source_before.utf16.len() + 1,
            buffer_pool_class: STRING_GUTS_BUFFER_POOL_CLASS,
            string_guts_logical_size: STRING_GUTS_LOGICAL_SIZE,
            buffer_before: MapMakeStringAllocationState::Live,
            buffer_after: MapMakeStringAllocationState::ReturnedToRetailPool,
            string_guts_before: MapMakeStringAllocationState::Live,
            string_guts_after: MapMakeStringAllocationState::ReturnedToRetailPool,
            buffer_pool_growth_is_allocator_state_dependent: true,
            string_guts_pool_growth_is_allocator_state_dependent: true,
            host_pointer_recorded: false,
        },
        source_before,
        local_after: map_make_post_checksum_closed_string(),
        world_sections_changed: world_before.differing_sections(&world_after),
        world_before,
        world_after,
        random_state_before: random_state,
        random_state_after: random_state,
        direct_rng_sites: Vec::new(),
        next: map_make_post_checksum_string_close_next(),
    })
}

pub(crate) fn validate_map_make_post_checksum_string_close_receipt(
    world: &World,
    regions: &Regions,
    prior_clear: &MapMakeFirstRegionsClearAllReceipt,
    prior_find_all: &MapMakeFirstRegionsFindAllReceipt,
    prior_limits: &MapMakeTerritoryLimitsReceipt,
    prior_fix_diag: &MapFixDiagLandReceipt,
    prior_string: &MapMakePostFixDiagStringConstructorReceipt,
    prior_log: &MapMakePostFixDiagGameLogReceipt,
    receipt: &MapMakePostChecksumStringCloseReceipt,
) -> bool {
    validate_map_make_post_fix_diag_game_log_receipt(
        world,
        regions,
        prior_clear,
        prior_find_all,
        prior_limits,
        prior_fix_diag,
        prior_string,
        prior_log,
    ) && receipt.caller == MAP_MAKE_POST_CHECKSUM_STRING_CLOSE_CALLER_BODY
        && receipt.body == MAP_MAKE_STRING_CLOSE_NATIVE_BODY
        && receipt.scalar_deleting_destructor == STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_BODY
        && receipt.mem_free == STRING_GUTS_MEM_FREE_BODY
        && receipt.operator_delete == STRING_GUTS_OPERATOR_DELETE_BODY
        && receipt.allocator_facts == MapMakeStringAllocatorFacts::RETAIL_GAMEPLAY
        && receipt.executed_direct_calls
            == [
                (MAP_MAKE_POST_CHECKSUM_STRING_CLOSE_CALL_VA, STRING_CLOSE_VA),
                (
                    STRING_CLOSE_GUTS_DESTRUCTOR_CALL_VA,
                    STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_VA,
                ),
                (STRING_GUTS_MEM_FREE_CALL_VA, STRING_GUTS_MEM_FREE_VA),
                (
                    STRING_GUTS_OPERATOR_DELETE_CALL_VA,
                    STRING_GUTS_OPERATOR_DELETE_VA,
                ),
            ]
        && receipt.executed_indirect_import_calls.is_empty()
        && receipt.source_before == prior_log.source_after
        && receipt.source_before.owns_typed_string_guts
        && receipt.local_after == map_make_post_checksum_closed_string()
        && receipt.allocation
            == (MapMakePostChecksumStringCloseAllocationReceipt {
                owner: MapMakePostFixDiagStringAllocationOwner::CallerLocalMapCpp,
                source_utf16: prior_log.source_after.utf16.clone(),
                buffer_units_with_nul: prior_log.source_after.utf16.len() + 1,
                buffer_pool_class: STRING_GUTS_BUFFER_POOL_CLASS,
                string_guts_logical_size: STRING_GUTS_LOGICAL_SIZE,
                buffer_before: MapMakeStringAllocationState::Live,
                buffer_after: MapMakeStringAllocationState::ReturnedToRetailPool,
                string_guts_before: MapMakeStringAllocationState::Live,
                string_guts_after: MapMakeStringAllocationState::ReturnedToRetailPool,
                buffer_pool_growth_is_allocator_state_dependent: true,
                string_guts_pool_growth_is_allocator_state_dependent: true,
                host_pointer_recorded: false,
            })
        && receipt.world_before == prior_log.world_after
        && receipt.world_before == receipt.world_after
        && receipt.world_after == world.checksum_sections()
        && receipt.world_sections_changed.is_empty()
        && receipt.random_state_before == prior_log.random_state_after
        && receipt.random_state_before == receipt.random_state_after
        && receipt.direct_rng_sites.is_empty()
        && receipt.next == map_make_post_checksum_string_close_next()
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StringCopyConstructorNativeBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub const_source_ret_va: u32,
    pub callee_stack_argument_bytes_popped: u8,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
    pub source_flags_test_va: u32,
    pub const_source_path_va: u32,
}

pub const STRING_COPY_CONSTRUCTOR_NATIVE_BODY: StringCopyConstructorNativeBody =
    StringCopyConstructorNativeBody {
        entry_va: STRING_COPY_CONSTRUCTOR_VA,
        end_va_exclusive: STRING_COPY_CONSTRUCTOR_END_VA,
        const_source_ret_va: STRING_COPY_CONSTRUCTOR_CONST_RET_VA,
        callee_stack_argument_bytes_popped: 4,
        size: STRING_COPY_CONSTRUCTOR_SIZE,
        instruction_count: STRING_COPY_CONSTRUCTOR_INSTRUCTION_COUNT,
        sha256: STRING_COPY_CONSTRUCTOR_SHA256,
        source_flags_test_va: STRING_COPY_CONSTRUCTOR_SOURCE_FLAGS_TEST_VA,
        const_source_path_va: STRING_COPY_CONSTRUCTOR_CONST_PATH_VA,
    };

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MapMakeProgressConstructorCallerBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
    pub call_va: u32,
    pub primitive_va: u32,
}

pub const MAP_MAKE_PROGRESS_CONSTRUCTOR_CALLER_BODY: MapMakeProgressConstructorCallerBody =
    MapMakeProgressConstructorCallerBody {
        entry_va: MAP_MAKE_PROGRESS_STRING_CONSTRUCTOR_CALL_VA,
        end_va_exclusive: MAP_MAKE_PROGRESS_CONSTRUCTOR_RESUME_VA,
        size: MAP_MAKE_PROGRESS_CONSTRUCTOR_CALL_SIZE,
        instruction_count: MAP_MAKE_PROGRESS_CONSTRUCTOR_CALL_INSTRUCTION_COUNT,
        sha256: MAP_MAKE_PROGRESS_CONSTRUCTOR_CALL_SHA256,
        call_va: MAP_MAKE_PROGRESS_STRING_CONSTRUCTOR_CALL_VA,
        primitive_va: STRING_COPY_CONSTRUCTOR_VA,
    };

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MapMakeProgressPostConstructorPrepBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
}

pub const MAP_MAKE_PROGRESS_POST_CONSTRUCTOR_PREP_BODY: MapMakeProgressPostConstructorPrepBody =
    MapMakeProgressPostConstructorPrepBody {
        entry_va: MAP_MAKE_PROGRESS_CONSTRUCTOR_RESUME_VA,
        end_va_exclusive: MAP_MAKE_PROGRESS_ASSIGN_CALL_VA,
        size: MAP_MAKE_PROGRESS_POST_CONSTRUCTOR_PREP_SIZE,
        instruction_count: MAP_MAKE_PROGRESS_POST_CONSTRUCTOR_PREP_INSTRUCTION_COUNT,
        sha256: MAP_MAKE_PROGRESS_POST_CONSTRUCTOR_PREP_SHA256,
    };

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct LocalizedStringTableInitNativeBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub ret_va: u32,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
    pub const_assign_call_va: u32,
    pub const_assign_va: u32,
}

pub const LOCALIZED_STRING_TABLE_INIT_NATIVE_BODY: LocalizedStringTableInitNativeBody =
    LocalizedStringTableInitNativeBody {
        entry_va: LOCALIZED_STRING_TABLE_INIT_VA,
        end_va_exclusive: LOCALIZED_STRING_TABLE_INIT_END_VA,
        ret_va: LOCALIZED_STRING_TABLE_INIT_RET_VA,
        size: LOCALIZED_STRING_TABLE_INIT_SIZE,
        instruction_count: LOCALIZED_STRING_TABLE_INIT_INSTRUCTION_COUNT,
        sha256: LOCALIZED_STRING_TABLE_INIT_SHA256,
        const_assign_call_va: LOCALIZED_STRING_TABLE_CONST_ASSIGN_CALL_VA,
        const_assign_va: STRING_WIDE_ASSIGN_VA,
    };

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StringWideAssignNativeBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub ret_va: u32,
    pub callee_stack_argument_bytes_popped: u8,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
}

pub const STRING_WIDE_ASSIGN_NATIVE_BODY: StringWideAssignNativeBody = StringWideAssignNativeBody {
    entry_va: STRING_WIDE_ASSIGN_VA,
    end_va_exclusive: STRING_WIDE_ASSIGN_END_VA,
    ret_va: STRING_WIDE_ASSIGN_RET_VA,
    callee_stack_argument_bytes_popped: 4,
    size: STRING_WIDE_ASSIGN_SIZE,
    instruction_count: STRING_WIDE_ASSIGN_INSTRUCTION_COUNT,
    sha256: STRING_WIDE_ASSIGN_SHA256,
};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StringCopyAssignmentNativeBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub const_source_ret_va: u32,
    pub callee_stack_argument_bytes_popped: u8,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
    pub dest_close_call_va: u32,
    pub dest_close_va: u32,
}

pub const STRING_COPY_ASSIGNMENT_NATIVE_BODY: StringCopyAssignmentNativeBody =
    StringCopyAssignmentNativeBody {
        entry_va: STRING_COPY_ASSIGN_VA,
        end_va_exclusive: STRING_COPY_ASSIGN_END_VA,
        const_source_ret_va: STRING_COPY_ASSIGN_CONST_RET_VA,
        callee_stack_argument_bytes_popped: 4,
        size: STRING_COPY_ASSIGN_SIZE,
        instruction_count: STRING_COPY_ASSIGN_INSTRUCTION_COUNT,
        sha256: STRING_COPY_ASSIGN_SHA256,
        dest_close_call_va: STRING_COPY_ASSIGN_DEST_CLOSE_CALL_VA,
        dest_close_va: STRING_CLOSE_VA,
    };

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SplashScreenRefreshNativeBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub ret_va: u32,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
}

pub const SPLASH_SCREEN_REFRESH_NATIVE_BODY: SplashScreenRefreshNativeBody =
    SplashScreenRefreshNativeBody {
        entry_va: SPLASH_SCREEN_REFRESH_VA,
        end_va_exclusive: SPLASH_SCREEN_REFRESH_END_VA,
        ret_va: SPLASH_SCREEN_REFRESH_RET_VA,
        size: SPLASH_SCREEN_REFRESH_SIZE,
        instruction_count: SPLASH_SCREEN_REFRESH_INSTRUCTION_COUNT,
        sha256: SPLASH_SCREEN_REFRESH_SHA256,
    };

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MapMakeProgressPresentationCallerBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
}

pub const MAP_MAKE_PROGRESS_PRESENTATION_CALLER_BODY: MapMakeProgressPresentationCallerBody =
    MapMakeProgressPresentationCallerBody {
        entry_va: MAP_MAKE_PROGRESS_ASSIGN_CALL_VA,
        end_va_exclusive: MAP_MAKE_PROGRESS_PRESENTATION_END_VA,
        size: MAP_MAKE_PROGRESS_PRESENTATION_SIZE,
        instruction_count: MAP_MAKE_PROGRESS_PRESENTATION_INSTRUCTION_COUNT,
        sha256: MAP_MAKE_PROGRESS_PRESENTATION_SHA256,
    };

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MapMakeCoastlinesCallBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
    pub primitive_va: u32,
}

pub const MAP_MAKE_COASTLINES_CALL_BODY: MapMakeCoastlinesCallBody = MapMakeCoastlinesCallBody {
    entry_va: MAP_MAKE_COASTLINES_CALL_VA,
    end_va_exclusive: MAP_MAKE_COASTLINES_CALL_RESUME_VA,
    size: MAP_MAKE_COASTLINES_CALL_SIZE,
    instruction_count: MAP_MAKE_COASTLINES_CALL_INSTRUCTION_COUNT,
    sha256: MAP_MAKE_COASTLINES_CALL_SHA256,
    primitive_va: MAP_MAKE_COASTLINES_VA,
};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MapMakeLocalizedStringBufferOwner {
    LocalizedStringTable,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MapMakeLocalizedStringTableRow {
    pub table_ptr_va: u32,
    pub entry_size: u32,
    pub table_index: u32,
    pub byte_offset: u32,
    pub resource_hash: u32,
    pub nonempty_in_supported_locales: bool,
    pub const_backed: bool,
    pub content_is_locale_dependent: bool,
    pub buffer_owner: MapMakeLocalizedStringBufferOwner,
    pub host_pointer_recorded: bool,
}

fn map_make_previous_progress_row() -> MapMakeLocalizedStringTableRow {
    MapMakeLocalizedStringTableRow {
        table_ptr_va: MAP_MAKE_PROGRESS_STRING_TABLE_PTR_VA,
        entry_size: LOCALIZED_STRING_TABLE_ENTRY_SIZE,
        table_index: MAP_MAKE_PREVIOUS_PROGRESS_STRING_TABLE_INDEX,
        byte_offset: MAP_MAKE_PREVIOUS_PROGRESS_STRING_BYTE_OFFSET,
        resource_hash: MAP_MAKE_PREVIOUS_PROGRESS_RESOURCE_HASH,
        nonempty_in_supported_locales: true,
        const_backed: true,
        content_is_locale_dependent: true,
        buffer_owner: MapMakeLocalizedStringBufferOwner::LocalizedStringTable,
        host_pointer_recorded: false,
    }
}

fn map_make_coastlines_progress_row() -> MapMakeLocalizedStringTableRow {
    MapMakeLocalizedStringTableRow {
        table_ptr_va: MAP_MAKE_PROGRESS_STRING_TABLE_PTR_VA,
        entry_size: LOCALIZED_STRING_TABLE_ENTRY_SIZE,
        table_index: MAP_MAKE_COASTLINES_STRING_TABLE_INDEX,
        byte_offset: MAP_MAKE_PROGRESS_STRING_BYTE_OFFSET,
        resource_hash: MAP_MAKE_COASTLINES_RESOURCE_HASH,
        nonempty_in_supported_locales: true,
        const_backed: true,
        content_is_locale_dependent: true,
        buffer_owner: MapMakeLocalizedStringBufferOwner::LocalizedStringTable,
        host_pointer_recorded: false,
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MapMakeProgressAliasState {
    TableOwnedLive,
    BorrowedConstAlias,
    ClosedBorrowedAlias,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MapMakeProgressStringOwnershipReceipt {
    pub source_buffer_before: MapMakeProgressAliasState,
    pub source_buffer_after: MapMakeProgressAliasState,
    pub local_before_close: MapMakeProgressAliasState,
    pub local_after_close: MapMakeProgressAliasState,
    pub prior_subtitle_before: MapMakeProgressAliasState,
    pub subtitle_after: MapMakeProgressAliasState,
    pub allocation_performed: bool,
    pub allocator_return_performed: bool,
    pub refcount_changed: bool,
    pub host_pointer_recorded: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MapMakeProgressClosedConstString {
    pub data_is_null: bool,
    pub current_length_is_zero: bool,
    pub flags_are_zero: bool,
    pub source_length_field_is_retained: bool,
    pub source_offset_is_retained: bool,
    pub source_cached_hashes_are_retained: bool,
    pub current_module_is_retained: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MapMakeProgressStringNext {
    MakeCoastlines {
        caller: MapMakeCoastlinesCallBody,
        call_va: u32,
        primitive_va: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapMakeProgressStringReceipt {
    pub caller: MapMakeProgressConstructorCallerBody,
    pub constructor: StringCopyConstructorNativeBody,
    pub string_table_init: LocalizedStringTableInitNativeBody,
    pub string_table_const_assign: StringWideAssignNativeBody,
    pub source: MapMakeLocalizedStringTableRow,
    pub previous_subtitle: MapMakeLocalizedStringTableRow,
    pub guard_store_va: u32,
    pub guard_after: u8,
    pub post_constructor_prep: MapMakeProgressPostConstructorPrepBody,
    pub assignment: StringCopyAssignmentNativeBody,
    pub presentation_caller: MapMakeProgressPresentationCallerBody,
    pub splash_refresh: SplashScreenRefreshNativeBody,
    pub string_close: StringCloseNativeBody,
    pub executed_assignment_branch_vas: Vec<u32>,
    pub executed_direct_calls: Vec<(u32, u32)>,
    pub ownership: MapMakeProgressStringOwnershipReceipt,
    pub local_after_close: MapMakeProgressClosedConstString,
    pub splash_subtitle_va: u32,
    pub splash_subtitle_after: MapMakeLocalizedStringTableRow,
    pub presentation_host_clock_and_draw_effects_unmodeled: bool,
    pub world_before: WorldChecksum,
    pub world_after: WorldChecksum,
    pub world_sections_changed: Vec<WorldSection>,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub direct_rng_sites: Vec<u32>,
    pub next: MapMakeProgressStringNext,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapMakeProgressStringError {
    PriorStringCloseReceiptMismatch,
}

fn map_make_progress_ownership() -> MapMakeProgressStringOwnershipReceipt {
    MapMakeProgressStringOwnershipReceipt {
        source_buffer_before: MapMakeProgressAliasState::TableOwnedLive,
        source_buffer_after: MapMakeProgressAliasState::TableOwnedLive,
        local_before_close: MapMakeProgressAliasState::BorrowedConstAlias,
        local_after_close: MapMakeProgressAliasState::ClosedBorrowedAlias,
        prior_subtitle_before: MapMakeProgressAliasState::BorrowedConstAlias,
        subtitle_after: MapMakeProgressAliasState::BorrowedConstAlias,
        allocation_performed: false,
        allocator_return_performed: false,
        refcount_changed: false,
        host_pointer_recorded: false,
    }
}

fn map_make_progress_closed_local() -> MapMakeProgressClosedConstString {
    MapMakeProgressClosedConstString {
        data_is_null: true,
        current_length_is_zero: true,
        flags_are_zero: true,
        source_length_field_is_retained: true,
        source_offset_is_retained: true,
        source_cached_hashes_are_retained: true,
        current_module_is_retained: true,
    }
}

fn map_make_progress_next() -> MapMakeProgressStringNext {
    MapMakeProgressStringNext::MakeCoastlines {
        caller: MAP_MAKE_COASTLINES_CALL_BODY,
        call_va: MAP_MAKE_COASTLINES_CALL_VA,
        primitive_va: MAP_MAKE_COASTLINES_VA,
    }
}

/// Execute the progress-caption `String::String(const String&)` and close its
/// complete borrowed ownership cone through the subtitle assignment, splash
/// refresh acknowledgement, and local `String::close`.
///
/// `StringTable::init` installs its rows with the const-backed wide assignment
/// at `0x00a28913 -> 0x00a1db60`. Row 2623 is therefore borrowed directly: the
/// constructor does not allocate and does not touch a `StringGuts` refcount.
/// The prior progress block left the subtitle as the same kind of alias to row
/// 2620, so replacement and both closes are allocator-free. Locale-specific
/// text and native addresses remain with the table owner. Splash refresh is a
/// presentation boundary whose clock/draw effects are deliberately not
/// promoted into replay state; the next canonical World mutation is
/// `Map::make_coastlines` at `0x0068bef2 -> 0x006947a0`.
pub fn execute_map_make_progress_string(
    world: &World,
    regions: &Regions,
    random_state: i32,
    prior_clear: &MapMakeFirstRegionsClearAllReceipt,
    prior_find_all: &MapMakeFirstRegionsFindAllReceipt,
    prior_limits: &MapMakeTerritoryLimitsReceipt,
    prior_fix_diag: &MapFixDiagLandReceipt,
    prior_string: &MapMakePostFixDiagStringConstructorReceipt,
    prior_log: &MapMakePostFixDiagGameLogReceipt,
    prior_close: &MapMakePostChecksumStringCloseReceipt,
) -> Result<MapMakeProgressStringReceipt, MapMakeProgressStringError> {
    if !validate_map_make_post_checksum_string_close_receipt(
        world,
        regions,
        prior_clear,
        prior_find_all,
        prior_limits,
        prior_fix_diag,
        prior_string,
        prior_log,
        prior_close,
    ) || prior_close.random_state_after != random_state
    {
        return Err(MapMakeProgressStringError::PriorStringCloseReceiptMismatch);
    }
    let world_before = world.checksum_sections();
    let source = map_make_coastlines_progress_row();
    let world_after = world.checksum_sections();
    Ok(MapMakeProgressStringReceipt {
        caller: MAP_MAKE_PROGRESS_CONSTRUCTOR_CALLER_BODY,
        constructor: STRING_COPY_CONSTRUCTOR_NATIVE_BODY,
        string_table_init: LOCALIZED_STRING_TABLE_INIT_NATIVE_BODY,
        string_table_const_assign: STRING_WIDE_ASSIGN_NATIVE_BODY,
        source,
        previous_subtitle: map_make_previous_progress_row(),
        guard_store_va: MAP_MAKE_PROGRESS_GUARD_STORE_VA,
        guard_after: 5,
        post_constructor_prep: MAP_MAKE_PROGRESS_POST_CONSTRUCTOR_PREP_BODY,
        assignment: STRING_COPY_ASSIGNMENT_NATIVE_BODY,
        presentation_caller: MAP_MAKE_PROGRESS_PRESENTATION_CALLER_BODY,
        splash_refresh: SPLASH_SCREEN_REFRESH_NATIVE_BODY,
        string_close: MAP_MAKE_STRING_CLOSE_NATIVE_BODY,
        executed_assignment_branch_vas: vec![
            STRING_COPY_ASSIGN_SELF_TEST_VA,
            STRING_COPY_ASSIGN_SOURCE_LENGTH_TEST_VA,
            STRING_COPY_ASSIGN_NONEMPTY_SOURCE_VA,
            STRING_COPY_ASSIGN_DEST_DATA_TEST_VA,
            STRING_COPY_ASSIGN_DEST_CONST_TEST_VA,
            STRING_COPY_ASSIGN_REPLACE_VA,
            STRING_COPY_ASSIGN_SOURCE_CONST_TEST_VA,
            STRING_COPY_ASSIGN_CONST_RET_VA,
        ],
        executed_direct_calls: vec![
            (
                MAP_MAKE_PROGRESS_STRING_CONSTRUCTOR_CALL_VA,
                STRING_COPY_CONSTRUCTOR_VA,
            ),
            (MAP_MAKE_PROGRESS_ASSIGN_CALL_VA, STRING_COPY_ASSIGN_VA),
            (STRING_COPY_ASSIGN_DEST_CLOSE_CALL_VA, STRING_CLOSE_VA),
            (
                MAP_MAKE_PROGRESS_SPLASH_REFRESH_CALL_VA,
                SPLASH_SCREEN_REFRESH_VA,
            ),
            (MAP_MAKE_PROGRESS_LOCAL_CLOSE_CALL_VA, STRING_CLOSE_VA),
        ],
        ownership: map_make_progress_ownership(),
        local_after_close: map_make_progress_closed_local(),
        splash_subtitle_va: SPLASH_SCREEN_SUBTITLE_STRING_VA,
        splash_subtitle_after: source,
        presentation_host_clock_and_draw_effects_unmodeled: true,
        world_sections_changed: world_before.differing_sections(&world_after),
        world_before,
        world_after,
        random_state_before: random_state,
        random_state_after: random_state,
        direct_rng_sites: Vec::new(),
        next: map_make_progress_next(),
    })
}

pub(crate) fn validate_map_make_progress_string_receipt(
    world: &World,
    regions: &Regions,
    prior_clear: &MapMakeFirstRegionsClearAllReceipt,
    prior_find_all: &MapMakeFirstRegionsFindAllReceipt,
    prior_limits: &MapMakeTerritoryLimitsReceipt,
    prior_fix_diag: &MapFixDiagLandReceipt,
    prior_string: &MapMakePostFixDiagStringConstructorReceipt,
    prior_log: &MapMakePostFixDiagGameLogReceipt,
    prior_close: &MapMakePostChecksumStringCloseReceipt,
    receipt: &MapMakeProgressStringReceipt,
) -> bool {
    validate_map_make_post_checksum_string_close_receipt(
        world,
        regions,
        prior_clear,
        prior_find_all,
        prior_limits,
        prior_fix_diag,
        prior_string,
        prior_log,
        prior_close,
    ) && receipt.caller == MAP_MAKE_PROGRESS_CONSTRUCTOR_CALLER_BODY
        && receipt.constructor == STRING_COPY_CONSTRUCTOR_NATIVE_BODY
        && receipt.string_table_init == LOCALIZED_STRING_TABLE_INIT_NATIVE_BODY
        && receipt.string_table_const_assign == STRING_WIDE_ASSIGN_NATIVE_BODY
        && receipt.source == map_make_coastlines_progress_row()
        && receipt.previous_subtitle == map_make_previous_progress_row()
        && receipt.guard_store_va == MAP_MAKE_PROGRESS_GUARD_STORE_VA
        && receipt.guard_after == 5
        && receipt.post_constructor_prep == MAP_MAKE_PROGRESS_POST_CONSTRUCTOR_PREP_BODY
        && receipt.assignment == STRING_COPY_ASSIGNMENT_NATIVE_BODY
        && receipt.presentation_caller == MAP_MAKE_PROGRESS_PRESENTATION_CALLER_BODY
        && receipt.splash_refresh == SPLASH_SCREEN_REFRESH_NATIVE_BODY
        && receipt.string_close == MAP_MAKE_STRING_CLOSE_NATIVE_BODY
        && receipt.executed_assignment_branch_vas
            == [
                STRING_COPY_ASSIGN_SELF_TEST_VA,
                STRING_COPY_ASSIGN_SOURCE_LENGTH_TEST_VA,
                STRING_COPY_ASSIGN_NONEMPTY_SOURCE_VA,
                STRING_COPY_ASSIGN_DEST_DATA_TEST_VA,
                STRING_COPY_ASSIGN_DEST_CONST_TEST_VA,
                STRING_COPY_ASSIGN_REPLACE_VA,
                STRING_COPY_ASSIGN_SOURCE_CONST_TEST_VA,
                STRING_COPY_ASSIGN_CONST_RET_VA,
            ]
        && receipt.executed_direct_calls
            == [
                (
                    MAP_MAKE_PROGRESS_STRING_CONSTRUCTOR_CALL_VA,
                    STRING_COPY_CONSTRUCTOR_VA,
                ),
                (MAP_MAKE_PROGRESS_ASSIGN_CALL_VA, STRING_COPY_ASSIGN_VA),
                (STRING_COPY_ASSIGN_DEST_CLOSE_CALL_VA, STRING_CLOSE_VA),
                (
                    MAP_MAKE_PROGRESS_SPLASH_REFRESH_CALL_VA,
                    SPLASH_SCREEN_REFRESH_VA,
                ),
                (MAP_MAKE_PROGRESS_LOCAL_CLOSE_CALL_VA, STRING_CLOSE_VA),
            ]
        && receipt.ownership == map_make_progress_ownership()
        && receipt.local_after_close == map_make_progress_closed_local()
        && receipt.splash_subtitle_va == SPLASH_SCREEN_SUBTITLE_STRING_VA
        && receipt.splash_subtitle_after == map_make_coastlines_progress_row()
        && receipt.presentation_host_clock_and_draw_effects_unmodeled
        && receipt.world_before == prior_close.world_after
        && receipt.world_before == receipt.world_after
        && receipt.world_after == world.checksum_sections()
        && receipt.world_sections_changed.is_empty()
        && receipt.random_state_before == prior_close.random_state_after
        && receipt.random_state_before == receipt.random_state_after
        && receipt.direct_rng_sites.is_empty()
        && receipt.next == map_make_progress_next()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostContinentReceipt {
    pub first_clear_va: u32,
    pub first_find_va: u32,
    pub first_regions: RegionBuildReceipt,
    pub limits: TerritoryLimits,
    pub fix_diag_va: u32,
    pub fix_diag_cells_changed: usize,
    pub make_coastlines_va: u32,
    pub coastline_cells_changed: usize,
    pub second_clear_va: u32,
    pub second_find_va: u32,
    pub second_regions: RegionBuildReceipt,
    pub final_land_cells: usize,
    pub final_coast_cells: usize,
    pub final_ocean_cells: usize,
    pub final_half_water_cells: usize,
    pub next_va: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PostContinentError {
    FirstRegionBuild(RegionsError),
    SecondRegionBuild(RegionsError),
}

/// Execute the common post-style chain transactionally.
pub fn execute_post_continent(
    world: &mut World,
    regions: &mut Regions,
    limits: TerritoryLimits,
) -> Result<PostContinentReceipt, PostContinentError> {
    let mut next_world = world.clone();
    let mut next_regions = regions.clone();

    // At 0x0068be36 the caller invokes clear_all then find_all before copying
    // the Map fields. This public composite is exactly that pair.
    let first_regions = next_regions
        .rebuild_after_coastlines(&mut next_world)
        .map_err(PostContinentError::FirstRegionBuild)?;

    next_world.player_territory_limit = limits.player_base;
    next_world.player_territory_limit_civic = limits.player_civic;
    next_world.player_territory_limit_city = limits.player_city;
    next_world.colonized_territory_limit = limits.colonized_base;
    next_world.colonized_territory_limit_civic = limits.colonized_civic;
    next_world.colonized_territory_limit_city = limits.colonized_city;

    let before_diag = terrain_words(&next_world);
    next_world.fix_diag_land();
    let after_diag = terrain_words(&next_world);
    let fix_diag_cells_changed = changed_cells(&before_diag, &after_diag);

    // The don-sim composite owns the exact make_coastlines + second
    // clear_all/find_all transaction. Region rebuilding changes region fields,
    // not the terrain words measured here.
    let before_coastline = after_diag;
    let second_regions = make_coastlines_and_rebuild_regions(&mut next_world, &mut next_regions)
        .map_err(PostContinentError::SecondRegionBuild)?;
    let after_coastline = terrain_words(&next_world);
    let coastline_cells_changed = changed_cells(&before_coastline, &after_coastline);

    let final_land_cells = next_world
        .wdata
        .iter()
        .filter(|cell| !cell_is_ocean(cell))
        .count();
    let final_coast_cells = next_world
        .wdata
        .iter()
        .filter(|cell| cell.flags & wflag::COAST != 0)
        .count();
    let final_ocean_cells = next_world
        .wdata
        .iter()
        .filter(|cell| cell_is_ocean(cell))
        .count();
    let final_half_water_cells = next_world
        .wdata
        .iter()
        .filter(|cell| cell.flags & wflag::WATERHALF != 0)
        .count();

    *world = next_world;
    *regions = next_regions;
    Ok(PostContinentReceipt {
        first_clear_va: REGIONS_CLEAR_ALL_VA,
        first_find_va: REGIONS_FIND_ALL_VA,
        first_regions,
        limits,
        fix_diag_va: MAP_FIX_DIAG_LAND_VA,
        fix_diag_cells_changed,
        make_coastlines_va: MAP_MAKE_COASTLINES_VA,
        coastline_cells_changed,
        second_clear_va: REGIONS_CLEAR_ALL_VA,
        second_find_va: REGIONS_FIND_ALL_VA,
        second_regions,
        final_land_cells,
        final_coast_cells,
        final_ocean_cells,
        final_half_water_cells,
        next_va: TERRAIN_GROUPS_FILL_FERTILE_VA,
    })
}

fn terrain_words(world: &World) -> Vec<(u16, i8, u8)> {
    world
        .wdata
        .iter()
        .map(|cell| (cell.flags, cell.land, cell.land_sub))
        .collect()
}

fn changed_cells(before: &[(u16, i8, u8)], after: &[(u16, i8, u8)]) -> usize {
    before
        .iter()
        .zip(after)
        .filter(|(before, after)| before != after)
        .count()
}

fn cell_is_ocean(cell: &don_sim::systems::map_terrain::WData) -> bool {
    cell.flags & wflag::WATERHALF == 0 && matches!(cell.land, land::COASTAL | land::OCEAN)
}
