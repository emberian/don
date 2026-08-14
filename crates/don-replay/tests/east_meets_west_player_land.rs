use don_replay::continent::{
    execute_continent_prefix_with_regions_from_rng,
    execute_east_meets_west_centroid_x_free_cleanup,
    execute_east_meets_west_centroid_y_free_cleanup, execute_east_meets_west_player_land,
    execute_east_meets_west_post_player_land_cleanup, execute_map_make_first_regions_clear_all,
    ContinentStop, EastMeetsWestCentroidListOwner, EastMeetsWestCentroidXFreeCleanupNext,
    EastMeetsWestCentroidYFreeCleanupNext, EastMeetsWestPlayerLandCall,
    EastMeetsWestPlayerLandNext, EastMeetsWestPostPlayerLandCleanupNext,
    EastMeetsWestStringClosePath, MapMakeFirstRegionsClearAllNext, RegionsAllocationState,
    RetailAllocationState, CHECK_PLAYER_LAND_NATIVE_BODY, EAST_MEETS_WEST_CENTROID_X_FREE_CALL_VA,
    EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_BODY, EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_END_VA,
    EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_INSTRUCTION_COUNT,
    EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_SHA256, EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_SIZE,
    EAST_MEETS_WEST_CENTROID_Y_FREE_CALL_VA, EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_BODY,
    EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_END_VA,
    EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_INSTRUCTION_COUNT,
    EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_SHA256, EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_SIZE,
    EAST_MEETS_WEST_LOG_STRING, EAST_MEETS_WEST_LOG_STRING_BYTE_OFFSET,
    EAST_MEETS_WEST_LOG_STRING_HASH, EAST_MEETS_WEST_LOG_STRING_ORDINAL,
    EAST_MEETS_WEST_LOG_STRING_UTF16_UNITS, EAST_MEETS_WEST_MAKE_CONTINENTS_ENTRY_VA,
    EAST_MEETS_WEST_MAKE_CONTINENTS_RET_VA, EAST_MEETS_WEST_MAKE_CONTINENTS_SIZE,
    EAST_MEETS_WEST_PLAYER_LAND_CALLER_ENTRY_VA, EAST_MEETS_WEST_PLAYER_LAND_CALL_VA,
    EAST_MEETS_WEST_PLAYER_LAND_GUARD_STORE_VA, EAST_MEETS_WEST_PLAYER_LAND_RESUME_VA,
    EAST_MEETS_WEST_PLAYER_LAND_STRING_CLOSE_CALL_VA,
    EAST_MEETS_WEST_POST_PLAYER_LAND_CLEANUP_BODY, EAST_MEETS_WEST_POST_PLAYER_LAND_CLEANUP_END_VA,
    EAST_MEETS_WEST_POST_PLAYER_LAND_CLEANUP_INSTRUCTION_COUNT,
    EAST_MEETS_WEST_POST_PLAYER_LAND_CLEANUP_SHA256, EAST_MEETS_WEST_POST_PLAYER_LAND_CLEANUP_SIZE,
    FREE_IMPORT_IAT_VA, MAP_CHECK_PLAYER_LAND_END_VA, MAP_CHECK_PLAYER_LAND_INSTRUCTION_COUNT,
    MAP_CHECK_PLAYER_LAND_RET_VA, MAP_CHECK_PLAYER_LAND_SHA256, MAP_CHECK_PLAYER_LAND_SIZE,
    MAP_MAKE_FIRST_REGIONS_CLEAR_CALL_VA, MAP_MAKE_FIRST_REGIONS_FIND_ARGUMENT_PUSH_VA,
    MAP_MAKE_FIRST_REGIONS_FIND_CALL_VA, REGIONS_CLEAR_ALL_COORD_FREE_CALL_VA,
    REGIONS_CLEAR_ALL_END_VA, REGIONS_CLEAR_ALL_FREE_IMPORT_IAT_VA,
    REGIONS_CLEAR_ALL_INSTRUCTION_COUNT, REGIONS_CLEAR_ALL_NATIVE_BODY,
    REGIONS_CLEAR_ALL_RET_NONEMPTY_WORLD_VA, REGIONS_CLEAR_ALL_SHA256, REGIONS_CLEAR_ALL_SIZE,
    REGIONS_CLEAR_ALL_VA, REGIONS_FIND_ALL_NATIVE_BODY, REGIONS_FIND_ALL_VA, RISE_EXE_SHA256,
    SIMPLE_ARRAY_INT_SIZE, STRING_CLOSE_INSTRUCTION_COUNT, STRING_CLOSE_NATIVE_BODY,
    STRING_CLOSE_SHA256, STRING_CLOSE_SIZE, STRING_CLOSE_VA, STRING_GUTS_DESTRUCTOR_CALL_VA,
    STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_VA,
};
use don_replay::fractal_boundary::resolve_tile_selection;
use don_replay::map_style::{ron_data_root_for_replay, MapStyleStaticData};
use don_replay::replay::Replay;
use don_sim::systems::collision::{RING_COUNT, RING_X, RING_Y};
use don_sim::systems::combat::circle_table;
use don_sim::systems::map_terrain::{land, WCoord, World, WorldSection};
use don_sim::systems::regions::Regions;
use std::path::{Path, PathBuf};

fn replay_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("ron-data/replays/multi")
        .join(name)
}

#[test]
fn native_extent_fastcall_and_typed_residual_are_frozen() {
    assert_eq!(RISE_EXE_SHA256.len(), 64);
    assert_eq!(CHECK_PLAYER_LAND_NATIVE_BODY.entry_va, 0x0068_ef00);
    assert_eq!(
        CHECK_PLAYER_LAND_NATIVE_BODY.end_va_exclusive,
        MAP_CHECK_PLAYER_LAND_END_VA
    );
    assert_eq!(
        CHECK_PLAYER_LAND_NATIVE_BODY.ret_va,
        MAP_CHECK_PLAYER_LAND_RET_VA
    );
    assert_eq!(
        CHECK_PLAYER_LAND_NATIVE_BODY.size,
        MAP_CHECK_PLAYER_LAND_SIZE
    );
    assert_eq!(MAP_CHECK_PLAYER_LAND_SIZE, 1_145);
    assert_eq!(
        CHECK_PLAYER_LAND_NATIVE_BODY.instruction_count,
        MAP_CHECK_PLAYER_LAND_INSTRUCTION_COUNT
    );
    assert_eq!(MAP_CHECK_PLAYER_LAND_INSTRUCTION_COUNT, 301);
    assert_eq!(
        CHECK_PLAYER_LAND_NATIVE_BODY.sha256,
        MAP_CHECK_PLAYER_LAND_SHA256
    );
    assert!(CHECK_PLAYER_LAND_NATIVE_BODY.direct_rng_sites.is_empty());
    assert_eq!(
        CHECK_PLAYER_LAND_NATIVE_BODY.direct_calls,
        [(0x0068_f089, 0x0046_d4f0)]
    );
    assert_eq!(
        CHECK_PLAYER_LAND_NATIVE_BODY.indirect_array_grow_calls,
        [0x0068_f0cf, 0x0068_f2ea]
    );
    assert_eq!(
        (
            EAST_MEETS_WEST_PLAYER_LAND_CALLER_ENTRY_VA,
            EAST_MEETS_WEST_PLAYER_LAND_CALL_VA,
            EAST_MEETS_WEST_PLAYER_LAND_RESUME_VA,
        ),
        (0x0069_7484, 0x0069_7492, 0x0069_7497)
    );
    assert_eq!(STRING_CLOSE_NATIVE_BODY.size, STRING_CLOSE_SIZE);
    assert_eq!(STRING_CLOSE_SIZE, 79);
    assert_eq!(STRING_CLOSE_INSTRUCTION_COUNT, 36);
    assert_eq!(STRING_CLOSE_NATIVE_BODY.sha256, STRING_CLOSE_SHA256);
    assert_eq!(
        STRING_CLOSE_NATIVE_BODY.direct_calls,
        [(
            STRING_GUTS_DESTRUCTOR_CALL_VA,
            STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_VA
        )]
    );
    assert_eq!(
        EAST_MEETS_WEST_POST_PLAYER_LAND_CLEANUP_BODY.end_va_exclusive,
        EAST_MEETS_WEST_POST_PLAYER_LAND_CLEANUP_END_VA
    );
    assert_eq!(EAST_MEETS_WEST_POST_PLAYER_LAND_CLEANUP_SIZE, 43);
    assert_eq!(
        EAST_MEETS_WEST_POST_PLAYER_LAND_CLEANUP_INSTRUCTION_COUNT,
        11
    );
    assert_eq!(
        EAST_MEETS_WEST_POST_PLAYER_LAND_CLEANUP_BODY.sha256,
        EAST_MEETS_WEST_POST_PLAYER_LAND_CLEANUP_SHA256
    );
    assert_eq!(
        EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_BODY.end_va_exclusive,
        EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_END_VA
    );
    assert_eq!(EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_SIZE, 64);
    assert_eq!(
        EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_INSTRUCTION_COUNT,
        12
    );
    assert_eq!(
        EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_BODY.sha256,
        EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_SHA256
    );
    assert_eq!(
        EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_BODY.indirect_import_calls,
        [(EAST_MEETS_WEST_CENTROID_Y_FREE_CALL_VA, FREE_IMPORT_IAT_VA)]
    );
    assert_eq!(SIMPLE_ARRAY_INT_SIZE, 28);
    assert_eq!(
        EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_BODY.end_va_exclusive,
        EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_END_VA
    );
    assert_eq!(EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_SIZE, 61);
    assert_eq!(
        EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_INSTRUCTION_COUNT,
        14
    );
    assert_eq!(
        EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_BODY.sha256,
        EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_SHA256
    );
    assert_eq!(
        EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_BODY.ret_va,
        Some(EAST_MEETS_WEST_MAKE_CONTINENTS_RET_VA)
    );
    assert_eq!(
        EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_BODY.indirect_import_calls,
        [(EAST_MEETS_WEST_CENTROID_X_FREE_CALL_VA, FREE_IMPORT_IAT_VA)]
    );
    assert_eq!(
        EAST_MEETS_WEST_MAKE_CONTINENTS_ENTRY_VA + EAST_MEETS_WEST_MAKE_CONTINENTS_SIZE,
        EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_END_VA
    );
    assert_eq!(REGIONS_CLEAR_ALL_NATIVE_BODY.entry_va, REGIONS_CLEAR_ALL_VA);
    assert_eq!(
        REGIONS_CLEAR_ALL_NATIVE_BODY.end_va_exclusive,
        REGIONS_CLEAR_ALL_END_VA
    );
    assert_eq!(REGIONS_CLEAR_ALL_SIZE, 275);
    assert_eq!(REGIONS_CLEAR_ALL_INSTRUCTION_COUNT, 79);
    assert_eq!(
        REGIONS_CLEAR_ALL_NATIVE_BODY.sha256,
        REGIONS_CLEAR_ALL_SHA256
    );
    assert_eq!(
        REGIONS_CLEAR_ALL_NATIVE_BODY.indirect_import_calls,
        [(
            REGIONS_CLEAR_ALL_COORD_FREE_CALL_VA,
            REGIONS_CLEAR_ALL_FREE_IMPORT_IAT_VA
        )]
    );
}

#[test]
fn caller_log_string_is_the_exact_shipped_internal_table_ordinal() {
    assert_eq!(EAST_MEETS_WEST_LOG_STRING_BYTE_OFFSET, 0x17660);
    assert_eq!(EAST_MEETS_WEST_LOG_STRING_ORDINAL, 4_792);
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("ron-data/internal_strings.xml");
    let xml = std::fs::read_to_string(path).unwrap();
    let table = don_content::string_table::parse_string_table_xml(&xml).unwrap();
    let record = &table.records()[EAST_MEETS_WEST_LOG_STRING_ORDINAL];
    assert_eq!(record.text, EAST_MEETS_WEST_LOG_STRING);
    assert_eq!(record.declared_hash, EAST_MEETS_WEST_LOG_STRING_HASH);
    assert_eq!(
        record.text.encode_utf16().count(),
        EAST_MEETS_WEST_LOG_STRING_UTF16_UNITS
    );
}

#[test]
fn map_make_progress_rows_are_the_exact_shipped_default_language_ordinals() {
    use don_replay::post_continent as pc;

    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("ron-data/translated_strings.xml");
    let xml = std::fs::read_to_string(path).unwrap();
    let table = don_content::string_table::parse_string_table_xml(&xml).unwrap();

    let previous = &table.records()[pc::MAP_MAKE_PREVIOUS_PROGRESS_STRING_TABLE_INDEX as usize];
    assert_eq!(previous.text, "Running Built-in Map Gen");
    assert_eq!(
        previous.declared_hash as u32,
        pc::MAP_MAKE_PREVIOUS_PROGRESS_RESOURCE_HASH
    );

    let coastlines = &table.records()[pc::MAP_MAKE_COASTLINES_STRING_TABLE_INDEX as usize];
    assert_eq!(coastlines.text, "Map Coastlines");
    assert_eq!(
        coastlines.declared_hash as u32,
        pc::MAP_MAKE_COASTLINES_RESOURCE_HASH
    );

    let terrain = &table.records()[pc::MAP_MAKE_TERRAIN_PROGRESS_STRING_TABLE_INDEX as usize];
    assert_eq!(terrain.text, "Map Terrain");
    assert_eq!(
        terrain.declared_hash as u32,
        pc::MAP_MAKE_TERRAIN_PROGRESS_RESOURCE_HASH
    );
}

#[test]
fn post_fix_diag_constructor_native_graph_is_frozen() {
    use don_replay::post_continent as pc;

    assert_eq!(pc::STRING_CONSTRUCTOR_NATIVE_BODY.entry_va, 0x00a1_d660);
    assert_eq!(pc::STRING_CONSTRUCTOR_NATIVE_BODY.size, 33);
    assert_eq!(pc::STRING_CONSTRUCTOR_NATIVE_BODY.instruction_count, 15);
    assert_eq!(
        pc::STRING_CONSTRUCTOR_NATIVE_BODY.sha256,
        "354be1ff3375e00afd53c7dd2ce92e7ebccba375f1ddc813fa9034dff6e629fe"
    );
    assert_eq!(
        pc::STRING_CONSTRUCTOR_NATIVE_BODY.direct_calls,
        [(0x00a1_d673, pc::STRING_INIT_CONST_VA)]
    );
    assert_eq!(pc::STRING_INIT_CONST_NATIVE_BODY.size, 119);
    assert_eq!(pc::STRING_INIT_CONST_NATIVE_BODY.instruction_count, 54);
    assert_eq!(pc::STRING_REINIT_NATIVE_BODY.size, 399);
    assert_eq!(pc::STRING_REINIT_NATIVE_BODY.instruction_count, 170);
    assert_eq!(pc::STRING_GET_STRING_GUTS_NATIVE_BODY.size, 152);
    assert_eq!(pc::STRING_GUTS_OPERATOR_NEW_NATIVE_BODY.size, 167);
    assert_eq!(pc::STRING_GUTS_MEM_GET_NATIVE_BODY.size, 372);
    assert_eq!(
        pc::STRING_GUTS_MEM_GET_NATIVE_BODY.indirect_import_calls,
        [(0x00a1_7b6b, pc::MALLOC_IAT_VA)]
    );
    assert_eq!(pc::STRING_CHAR_TO_WCHAR_NATIVE_BODY.size, 70);
    assert_eq!(
        pc::STRING_CHAR_TO_WCHAR_NATIVE_BODY.indirect_import_calls,
        [
            (0x00a1_7c4e, pc::MULTI_BYTE_TO_WIDE_CHAR_IAT_VA),
            (0x00a1_7c60, pc::MULTI_BYTE_TO_WIDE_CHAR_IAT_VA),
        ]
    );
    assert_eq!(pc::MAP_MAKE_POST_FIX_DIAG_LITERAL, "map.cpp");
    assert_eq!(pc::MAP_MAKE_POST_FIX_DIAG_LITERAL_VA, 0x00ad_de58);
    assert_eq!(pc::MAP_MAKE_POST_FIX_DIAG_CONSTRUCTOR_CALLER_BODY.size, 28);
    assert_eq!(
        pc::MAP_MAKE_POST_FIX_DIAG_CONSTRUCTOR_CALLER_BODY.instruction_count,
        7
    );
    assert_eq!(pc::MAP_MAKE_POST_FIX_DIAG_GAME_LOG_CALL_VA, 0x0068_be9e);
    assert_eq!(pc::GAME_LOG_SAY_CHECKSUM_VA, 0x0093_0b30);
}

#[test]
fn post_fix_diag_checksum_log_native_graph_is_frozen() {
    use don_replay::post_continent as pc;

    assert_eq!(pc::GAME_LOG_SAY_CHECKSUM_NATIVE_BODY.entry_va, 0x0093_0b30);
    assert_eq!(
        pc::GAME_LOG_SAY_CHECKSUM_NATIVE_BODY.end_va_exclusive,
        0x0093_1ca8
    );
    assert_eq!(pc::GAME_LOG_SAY_CHECKSUM_NATIVE_BODY.ret_va, 0x0093_1ca5);
    assert_eq!(pc::GAME_LOG_SAY_CHECKSUM_NATIVE_BODY.size, 4_472);
    assert_eq!(
        pc::GAME_LOG_SAY_CHECKSUM_NATIVE_BODY.instruction_count,
        1_379
    );
    assert_eq!(
        pc::GAME_LOG_SAY_CHECKSUM_NATIVE_BODY.sha256,
        "8e58ddabcd6742238aab95311529e9f615c5f322771f070c4805fbc48f2ef868"
    );
    assert_eq!(
        pc::GAME_LOG_SAY_CHECKSUM_NATIVE_BODY.check_accept_call_va,
        0x0093_0b6d
    );
    assert_eq!(pc::GAME_LOG_CHECK_ACCEPT_NATIVE_BODY.size, 390);
    assert_eq!(pc::GAME_LOG_CHECK_ACCEPT_NATIVE_BODY.instruction_count, 98);
    assert_eq!(
        pc::GAME_LOG_CHECK_ACCEPT_NATIVE_BODY.reentrancy_guard_va,
        0x00ee_12c0
    );
    assert_eq!(pc::MAP_MAKE_POST_CHECKSUM_CALLER_BODY.size, 15);
    assert_eq!(pc::MAP_MAKE_POST_CHECKSUM_CALLER_BODY.instruction_count, 3);
    assert_eq!(
        pc::MAP_MAKE_POST_CHECKSUM_CALLER_BODY.sha256,
        "3f130942dec6f49dc4774ad3eacbcee60a43d3181698848f155a44d835a1ace0"
    );
}

#[test]
fn post_checksum_string_close_native_graph_is_frozen() {
    use don_replay::post_continent as pc;

    assert_eq!(pc::MAP_MAKE_POST_CHECKSUM_STRING_CLOSE_CALLER_BODY.size, 5);
    assert_eq!(
        pc::MAP_MAKE_POST_CHECKSUM_STRING_CLOSE_CALLER_BODY.sha256,
        "72e40e78c017521f98c87e6492bd9702157a03bd0114c8f55ee6b60c73122c48"
    );
    assert_eq!(pc::MAP_MAKE_STRING_CLOSE_NATIVE_BODY.entry_va, 0x00a1_cf40);
    assert_eq!(pc::MAP_MAKE_STRING_CLOSE_NATIVE_BODY.size, 79);
    assert_eq!(pc::MAP_MAKE_STRING_CLOSE_NATIVE_BODY.instruction_count, 36);
    assert_eq!(
        pc::MAP_MAKE_STRING_CLOSE_NATIVE_BODY.sha256,
        "80b55b224beca72346bcb001fb415310611060b52a0c49c8db67c66c71fbae7f"
    );
    assert_eq!(
        pc::STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_BODY.entry_va,
        0x004d_3e90
    );
    assert_eq!(pc::STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_BODY.size, 90);
    assert_eq!(
        pc::STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_BODY.instruction_count,
        31
    );
    assert_eq!(pc::STRING_GUTS_MEM_FREE_BODY.entry_va, 0x00a1_79a0);
    assert_eq!(pc::STRING_GUTS_MEM_FREE_BODY.size, 103);
    assert_eq!(pc::STRING_GUTS_MEM_FREE_BODY.instruction_count, 40);
    assert_eq!(pc::STRING_GUTS_OPERATOR_DELETE_BODY.entry_va, 0x00a1_77b0);
    assert_eq!(pc::STRING_GUTS_OPERATOR_DELETE_BODY.size, 307);
    assert_eq!(pc::STRING_GUTS_OPERATOR_DELETE_BODY.instruction_count, 93);
    assert_eq!(pc::MAP_MAKE_POST_CLOSE_PROGRESS_PREP_BODY.size, 18);
    assert_eq!(
        pc::MAP_MAKE_POST_CLOSE_PROGRESS_PREP_BODY.sha256,
        "5287e0907afb9002eb66e3c49abcb1bcdfb8120294e860e019461ae6dc42133f"
    );
    assert_eq!(
        pc::MAP_MAKE_PROGRESS_STRING_CONSTRUCTOR_CALL_VA,
        0x0068_bec4
    );
    assert_eq!(pc::STRING_COPY_CONSTRUCTOR_VA, 0x00a1_d590);
}

#[test]
fn progress_string_const_alias_graph_is_frozen() {
    use don_replay::post_continent as pc;

    assert_eq!(pc::STRING_COPY_CONSTRUCTOR_NATIVE_BODY.size, 200);
    assert_eq!(
        pc::STRING_COPY_CONSTRUCTOR_NATIVE_BODY.instruction_count,
        73
    );
    assert_eq!(
        pc::STRING_COPY_CONSTRUCTOR_NATIVE_BODY.sha256,
        "b870ca7a19b559d52aead2ab867d2c6fc2aacd5cd0131ddbc29ea16b9455cf89"
    );
    assert_eq!(pc::LOCALIZED_STRING_TABLE_INIT_NATIVE_BODY.size, 1358);
    assert_eq!(
        pc::LOCALIZED_STRING_TABLE_INIT_NATIVE_BODY.sha256,
        "fc556322e27480913363b4caa6f6b1360dce73309100d6ef670ba70d4c4cd352"
    );
    assert_eq!(pc::STRING_WIDE_ASSIGN_NATIVE_BODY.size, 110);
    assert_eq!(pc::STRING_COPY_ASSIGNMENT_NATIVE_BODY.size, 482);
    assert_eq!(
        pc::STRING_COPY_ASSIGNMENT_NATIVE_BODY.sha256,
        "1545068535829488cb7a2b77fdaf0633ded575d90e4ee76ee216d7e8a1e76f5a"
    );
    assert_eq!(pc::SPLASH_SCREEN_REFRESH_NATIVE_BODY.size, 199);
    assert_eq!(
        pc::SPLASH_SCREEN_REFRESH_NATIVE_BODY.sha256,
        "c9e741eea1edf51ed91667fcd3d55edf1de92a5353c062f0dffffd62efde4907"
    );
    assert_eq!(pc::MAP_MAKE_COASTLINES_CALL_BODY.entry_va, 0x0068_bef2);
    assert_eq!(pc::MAP_MAKE_COASTLINES_CALL_BODY.primitive_va, 0x0069_47a0);
    assert_eq!(pc::MAP_MAKE_COASTLINES_NATIVE_BODY.size, 774);
    assert_eq!(pc::MAP_MAKE_COASTLINES_NATIVE_BODY.instruction_count, 225);
    assert_eq!(
        pc::MAP_MAKE_COASTLINES_NATIVE_BODY.sha256,
        "dd2f6e2005336caf7e6e33a57422d91e69d6e769a2c48d00e03bbcebe984aa4a"
    );
    assert_eq!(pc::MAP_FIX_LAKES_NATIVE_BODY.size, 391);
    assert_eq!(pc::MAP_FIX_LAKES_NATIVE_BODY.instruction_count, 120);
    assert_eq!(
        pc::MAP_FIX_LAKES_NATIVE_BODY.sha256,
        "bd51e858b7bdafd610f3b1713130987aa98f678c067e13c36e5e7b44f439da46"
    );
}

#[test]
fn shipped_ring_ten_anomaly_is_live_in_the_exclusion_scan() {
    assert_eq!(RING_COUNT[10], 441);
    assert_eq!((RING_X[288], RING_Y[288]), (-8, -16));

    let mut generated = Vec::new();
    generated.push((0, 0));
    for radius in 1..=10 {
        for x in -radius..=radius {
            generated.push((x, -radius));
        }
        for y in (-radius + 1)..=radius {
            generated.push((radius, y));
        }
        for x in (-radius..=(radius - 1)).rev() {
            generated.push((x, radius));
        }
        for y in ((-radius + 1)..=(radius - 1)).rev() {
            generated.push((-radius, y));
        }
    }
    assert_eq!(generated[288], (-8, -7));
    assert!(!generated.contains(&(-8, -16)));

    let mut world = World::init_default_rules(80, 80);
    for cell in &mut world.wdata {
        cell.land = land::FERTILE;
        cell.land_sub = 0;
        cell.region = 1;
    }
    world.add_starting_location(WCoord(40), WCoord(40));
    world.start_city_x.items.fill(40);
    world.start_city_y.items.fill(40);

    let circle = circle_table();
    let candidate = (40 + i32::from(circle.x[1]), 40 + i32::from(circle.y[1]));
    world.wdata_mut(candidate.0, candidate.1).land = land::OCEAN;
    let anomaly = (candidate.0 + RING_X[288], candidate.1 + RING_Y[288]);
    world.wdata_mut(anomaly.0, anomaly.1).land = land::FERTILE;
    world.wdata_mut(anomaly.0, anomaly.1).region = 2;

    let mut regions = Regions::default();
    regions.list[1].coords.items.push((40, 40));
    regions.list[1].coords.capacity = 4;
    regions.list[1].size = 1;
    regions.list[2].coords.items = vec![anomaly, (5, 5), (6, 5), (5, 6), (6, 6)];
    regions.list[2].coords.capacity = 8;
    regions.list[2].size = 5;

    let receipt = execute_east_meets_west_player_land(
        &mut world,
        &mut regions,
        EastMeetsWestPlayerLandCall {
            expected_start_count: 1,
            avoid_continent: 16,
            unread_stack_word: 0x1357_2468,
            random_state: 0x1234_5678,
        },
    )
    .unwrap();

    assert_eq!(receipt.native_call.enabled, 1);
    assert_eq!(receipt.native_call.avoid_continent, 16);
    assert_eq!(receipt.native_call.radius, -1);
    assert_eq!(receipt.body_receipt.effective_radius, 5);
    assert_eq!(receipt.body_receipt.effective_avoid_continent, Some(10));
    assert!(receipt.body_receipt.conflict_rejections >= 4);
    assert_eq!(world.wdata(candidate.0, candidate.1).land, land::OCEAN);
    assert_eq!(receipt.random_state_before, receipt.random_state_after);
    assert!(receipt.direct_rng_sites.is_empty());
    assert_eq!(
        receipt.next,
        EastMeetsWestPlayerLandNext::PostCallCleanup {
            entry_va: EAST_MEETS_WEST_PLAYER_LAND_RESUME_VA,
            guard_store_va: EAST_MEETS_WEST_PLAYER_LAND_GUARD_STORE_VA,
            string_close_call_va: EAST_MEETS_WEST_PLAYER_LAND_STRING_CLOSE_CALL_VA,
            string_close_va: STRING_CLOSE_VA,
        }
    );
    assert_eq!(
        execute_east_meets_west_post_player_land_cleanup(&receipt, &[]),
        Err(don_replay::continent::EastMeetsWestPlayerLandError::EmptyCentroidYArray)
    );
    let cleanup = execute_east_meets_west_post_player_land_cleanup(&receipt, &[40]).unwrap();
    assert_eq!(
        cleanup.stack_argument_pop_va,
        EAST_MEETS_WEST_PLAYER_LAND_RESUME_VA
    );
    assert_eq!(cleanup.guard_before_string_close, 1);
    assert_eq!(cleanup.guard_after_string_close, 0);
    assert_eq!(
        cleanup.string_close.path,
        EastMeetsWestStringClosePath::SharedStringGutsReferenceDecrement
    );
    assert_eq!(cleanup.string_close.table_reference_delta_over_lease, 0);
    assert!(!cleanup.string_close.string_guts_destructor_called);
    assert!(cleanup.string_close.local_data_is_null);
    assert_eq!(cleanup.centroid_y_length, 1);
    assert!(cleanup.centroid_y_list_non_null);
    assert_eq!(
        cleanup.next,
        EastMeetsWestPostPlayerLandCleanupNext::FreeCentroidYList {
            call_va: EAST_MEETS_WEST_CENTROID_Y_FREE_CALL_VA,
            import_iat_va: FREE_IMPORT_IAT_VA,
        }
    );
    assert_eq!(
        execute_east_meets_west_centroid_y_free_cleanup(&cleanup, &[40], &[]),
        Err(don_replay::continent::EastMeetsWestPlayerLandError::EmptyCentroidXArray)
    );
    let free_cleanup =
        execute_east_meets_west_centroid_y_free_cleanup(&cleanup, &[40], &[41]).unwrap();
    assert_eq!(
        free_cleanup.free.allocation.owner,
        EastMeetsWestCentroidListOwner::YCoordinates
    );
    assert_eq!(free_cleanup.free.allocation.elements, [40]);
    assert_eq!(free_cleanup.free.allocation.element_width, 4);
    assert_eq!(free_cleanup.free.state_before, RetailAllocationState::Live);
    assert_eq!(free_cleanup.free.state_after, RetailAllocationState::Freed);
    assert_eq!(
        free_cleanup.body,
        EAST_MEETS_WEST_CENTROID_Y_FREE_CLEANUP_BODY
    );
    assert_eq!(
        free_cleanup.free.call_va,
        EAST_MEETS_WEST_CENTROID_Y_FREE_CALL_VA
    );
    assert_eq!(free_cleanup.free.import_iat_va, FREE_IMPORT_IAT_VA);
    assert_eq!(free_cleanup.stack_argument_bytes_popped, 4);
    assert!(free_cleanup.local_list_is_null);
    assert_eq!(free_cleanup.local_size, 0);
    assert_eq!(free_cleanup.local_length, 0);
    assert_eq!(free_cleanup.local_flags, 0);
    assert_eq!(free_cleanup.cleanup_guard_after, -1);
    assert_eq!(free_cleanup.centroid_x_length, 1);
    assert!(free_cleanup.centroid_x_list_non_null);
    assert_eq!(
        free_cleanup.next,
        EastMeetsWestCentroidYFreeCleanupNext::FreeCentroidXList {
            call_va: EAST_MEETS_WEST_CENTROID_X_FREE_CALL_VA,
            import_iat_va: FREE_IMPORT_IAT_VA,
        }
    );
    assert_eq!(
        execute_east_meets_west_centroid_x_free_cleanup(&free_cleanup, &[]),
        Err(don_replay::continent::EastMeetsWestPlayerLandError::EmptyCentroidXArray)
    );
    let x_cleanup = execute_east_meets_west_centroid_x_free_cleanup(&free_cleanup, &[41]).unwrap();
    assert_eq!(
        x_cleanup.free.allocation.owner,
        EastMeetsWestCentroidListOwner::XCoordinates
    );
    assert_eq!(x_cleanup.free.allocation.elements, [41]);
    assert_eq!(x_cleanup.free.state_before, RetailAllocationState::Live);
    assert_eq!(x_cleanup.free.state_after, RetailAllocationState::Freed);
    assert!(x_cleanup.local_list_is_null);
    assert_eq!(x_cleanup.local_size, 0);
    assert_eq!(x_cleanup.local_length, 0);
    assert_eq!(x_cleanup.local_flags, 0);
    assert!(x_cleanup.exception_registration_restored);
    assert_eq!(
        x_cleanup.next,
        EastMeetsWestCentroidXFreeCleanupNext::ReturnedFromMakeContinents {
            ret_va: EAST_MEETS_WEST_MAKE_CONTINENTS_RET_VA,
            callee_stack_argument_bytes_popped: 4,
        }
    );
    let clear = execute_map_make_first_regions_clear_all(
        &mut world,
        &mut regions,
        x_cleanup.random_state_after,
    );
    assert_eq!(clear.caller_call_va, MAP_MAKE_FIRST_REGIONS_CLEAR_CALL_VA);
    assert_eq!(clear.body, REGIONS_CLEAR_ALL_NATIVE_BODY);
    assert_eq!(
        clear.executed_ret_va,
        REGIONS_CLEAR_ALL_RET_NONEMPTY_WORLD_VA
    );
    assert_eq!(clear.region_records_visited, 128);
    assert_eq!(clear.world_cells_visited, world.wdata.len());
    assert!(world.wdata.iter().all(|cell| cell.region == 0));
    assert!(clear.world_region2_unchanged);
    assert_eq!(regions.land, 0);
    assert_eq!(regions.sea, 64);
    assert!(clear.coordinate_frees.iter().all(|free| {
        free.call_va == REGIONS_CLEAR_ALL_COORD_FREE_CALL_VA
            && free.import_iat_va == REGIONS_CLEAR_ALL_FREE_IMPORT_IAT_VA
            && free.element_width == 8
            && free.state_before == RegionsAllocationState::Live
            && free.state_after == RegionsAllocationState::Freed
    }));
    assert_eq!(
        clear.next,
        MapMakeFirstRegionsClearAllNext::FindAll {
            argument_push_va: MAP_MAKE_FIRST_REGIONS_FIND_ARGUMENT_PUSH_VA,
            call_va: MAP_MAKE_FIRST_REGIONS_FIND_CALL_VA,
            primitive_va: REGIONS_FIND_ALL_VA,
            stack_argument_is_unread: true,
        }
    );
}

#[test]
fn both_real_style19_headers_execute_the_complete_body_without_rng_or_start_rewrite() {
    for (name, expected) in [
        (
            "Playback___2018.12.01_18_33_16__Sat_.rcx",
            (
                0x2048_1b28u32,
                0x4eb6_10df,
                0x885c_07e8,
                0x2eaa_f2e7,
                0x5292_e9f0,
                [
                    (1, 3_164, 3_996, 3_164, 6_328),
                    (2, 2_787, 3_619, 2_787, 5_574),
                ],
            ),
        ),
        (
            "Playback___2019.03.24_11_56_19__Sun_.rcx",
            (
                0x18a5_6595u32,
                0x660c_7097,
                0x2a9f_6868,
                0x69f0_54b2,
                0x25d1_4c83,
                [
                    (1, 2_779, 3_611, 2_779, 5_558),
                    (2, 2_793, 3_625, 2_793, 5_586),
                ],
            ),
        ),
    ] {
        let path = replay_path(name);
        assert!(path.is_file(), "missing real fixture {}", path.display());
        let replay = Replay::open(&path).unwrap();
        let root = ron_data_root_for_replay(&path).unwrap();
        let style =
            MapStyleStaticData::load_from_ron_data(&root, replay.initial.info.settings.map_style)
                .unwrap();
        let plan = replay
            .initial
            .reconstruct_items_with_style(style.clone())
            .unwrap();
        let selection = resolve_tile_selection(
            plan.inputs.seed,
            &style.default_source.path,
            &style.selected_source.path,
        )
        .unwrap();
        let mut map = replay.initial.reconstruct_world().unwrap();
        let prefix = execute_continent_prefix_with_regions_from_rng(
            &plan.inputs,
            &style,
            selection.main_random_state_after,
            &mut map.world,
            &mut map.generation_regions,
        )
        .unwrap();
        let ContinentStop::AddStartingLocation {
            centroids,
            remaining,
            player_land: receipt,
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
            progress_string,
            coastlines,
            post_coastline_string_constructor,
            post_coastline_game_log,
            post_coastline_string_close,
            second_regions_clear_all,
            second_regions_find_all,
            second_regions_tail,
            next_va,
            next_mutator_va,
            ..
        } = &prefix.stop
        else {
            panic!("{name}: unexpected post-call stop {:?}", prefix.stop);
        };
        assert_eq!(remaining.active_slots, plan.inputs.active_slots, "{name}");
        assert_eq!(
            map.world.start_x.items.len(),
            plan.inputs.active_slots.len(),
            "{name}"
        );

        assert_eq!(
            *next_va,
            don_replay::post_continent::MAP_MAKE_FILL_FERTILE_CALL_VA,
            "{name}"
        );
        assert_eq!(
            *next_mutator_va,
            don_replay::post_continent::TERRAIN_GROUPS_FILL_FERTILE_VA,
            "{name}"
        );
        assert_eq!(
            post_player_land_cleanup.centroid_y_length,
            usize::from(prefix.team_partition.as_ref().unwrap().continent_count),
            "{name}"
        );
        assert_eq!(
            centroid_y_free_cleanup.free.allocation.elements, centroids.centroid_y,
            "{name}"
        );
        assert_eq!(
            centroid_y_free_cleanup.centroid_x_length,
            centroids.centroid_x.len(),
            "{name}"
        );
        assert_eq!(
            centroid_y_free_cleanup.free.state_after,
            RetailAllocationState::Freed,
            "{name}"
        );
        assert_eq!(
            centroid_x_free_cleanup.free.allocation.elements, centroids.centroid_x,
            "{name}"
        );
        assert_eq!(
            centroid_x_free_cleanup.free.state_after,
            RetailAllocationState::Freed,
            "{name}"
        );
        assert!(
            centroid_x_free_cleanup.exception_registration_restored,
            "{name}"
        );
        assert_eq!(regions_clear_all.region_records_visited, 128, "{name}");
        assert_eq!(
            regions_clear_all.world_cells_visited,
            map.world.wdata.len(),
            "{name}"
        );
        assert_eq!(
            regions_find_all.body, REGIONS_FIND_ALL_NATIVE_BODY,
            "{name}"
        );
        assert_eq!(
            regions_find_all.world_before, regions_clear_all.world_after,
            "{name}"
        );
        assert_eq!(
            regions_find_all.world_after, fix_diag_land.world_before,
            "{name}"
        );
        assert_eq!(regions_find_all.region_records_visited, 128, "{name}");
        assert!(
            second_regions_find_all
                .region_records
                .iter()
                .all(|record| record.after
                    == map.generation_regions.list[usize::from(record.region)]),
            "{name}"
        );
        assert_eq!(
            regions_find_all.world_mutations.len(),
            map.world.wdata.len(),
            "{name}"
        );
        assert!(
            map.world.wdata.iter().all(|cell| cell.region != 0),
            "{name}"
        );
        assert_eq!(map.generation_regions.land, 0, "{name}");
        assert_eq!(map.generation_regions.sea, 65, "{name}");
        assert_eq!(regions_find_all.successful_find_calls, 3, "{name}");
        assert_eq!(regions_find_all.build.non_input_pumps, 4, "{name}");
        assert!(regions_find_all.scratch.allocation.is_some(), "{name}");
        assert!(regions_find_all.scratch.final_free.is_some(), "{name}");
        assert_eq!(territory_limits.stores.len(), 6, "{name}");
        assert_eq!(
            territory_limits.body,
            don_replay::continent::MAP_MAKE_TERRITORY_LIMITS_NATIVE_BODY,
            "{name}"
        );
        assert_eq!(
            territory_limits
                .stores
                .iter()
                .map(|store| store.map_value_load_va)
                .collect::<Vec<_>>(),
            don_replay::continent::MAP_MAKE_TERRITORY_LIMIT_LOAD_VAS,
            "{name}"
        );
        assert_eq!(
            territory_limits
                .stores
                .iter()
                .map(|store| store.store_va)
                .collect::<Vec<_>>(),
            don_replay::continent::MAP_MAKE_TERRITORY_LIMIT_STORE_VAS,
            "{name}"
        );
        assert_eq!(
            territory_limits
                .stores
                .iter()
                .map(|store| (store.source_value, store.value_before, store.value_after))
                .collect::<Vec<_>>(),
            [
                (44, 44, 44),
                (4, 4, 4),
                (4, 4, 4),
                (44, 44, 44),
                (4, 4, 4),
                (4, 4, 4),
            ],
            "{name}"
        );
        assert_eq!(territory_limits.branch.map_style, 19, "{name}");
        assert!(!territory_limits.branch.taken, "{name}");
        assert_eq!(
            territory_limits.next,
            don_replay::continent::MapMakeTerritoryLimitsNext::FixDiagLand {
                call_va: don_replay::continent::MAP_MAKE_FIRST_FIX_DIAG_LAND_CALL_VA,
                primitive_va: don_replay::post_continent::MAP_FIX_DIAG_LAND_VA,
            },
            "{name}"
        );
        assert_eq!(
            territory_limits.world_before, regions_find_all.world_after,
            "{name}"
        );
        assert_eq!(
            territory_limits.world_after, fix_diag_land.world_before,
            "{name}"
        );
        assert!(territory_limits.world_sections_changed.is_empty(), "{name}");
        assert_eq!(
            fix_diag_land.body,
            don_replay::post_continent::MAP_FIX_DIAG_LAND_NATIVE_BODY,
            "{name}"
        );
        assert_eq!(fix_diag_land.cells_scanned, map.world.wdata.len(), "{name}");
        assert_eq!(fix_diag_land.world_after, coastlines.world_before, "{name}");
        assert!(fix_diag_land.direct_rng_sites.is_empty(), "{name}");
        assert_eq!(
            fix_diag_land.random_state_before, fix_diag_land.random_state_after,
            "{name}"
        );
        assert_eq!(
            fix_diag_land.next,
            don_replay::continent::MapFixDiagLandNext::StringConstructor {
                caller_resume_va: don_replay::continent::MAP_MAKE_FIX_DIAG_LAND_RESUME_VA,
                string_literal_push_va:
                    don_replay::continent::MAP_MAKE_POST_FIX_DIAG_STRING_LITERAL_PUSH_VA,
                string_local_load_va:
                    don_replay::continent::MAP_MAKE_POST_FIX_DIAG_STRING_LOCAL_LOAD_VA,
                call_va: don_replay::continent::MAP_MAKE_POST_FIX_DIAG_STRING_CONSTRUCTOR_CALL_VA,
                primitive_va: don_replay::continent::STRING_CONSTRUCTOR_VA,
            },
            "{name}"
        );
        assert!(
            fix_diag_land.mutations.iter().all(|mutation| {
                let mut expected = mutation.before.clone();
                expected.land = don_sim::systems::map_terrain::land::OCEAN;
                expected.land_sub = 0;
                mutation.before.land == 0 && mutation.after == expected
            }),
            "{name}"
        );
        assert_eq!(
            post_fix_diag_string_constructor.constructor,
            don_replay::continent::STRING_CONSTRUCTOR_NATIVE_BODY,
            "{name}"
        );
        assert_eq!(
            post_fix_diag_string_constructor.init_const,
            don_replay::continent::STRING_INIT_CONST_NATIVE_BODY,
            "{name}"
        );
        assert_eq!(
            post_fix_diag_string_constructor.reinit,
            don_replay::continent::STRING_REINIT_NATIVE_BODY,
            "{name}"
        );
        assert_eq!(
            post_fix_diag_string_constructor.local.source, "map.cpp",
            "{name}"
        );
        assert_eq!(
            post_fix_diag_string_constructor.local.utf16,
            "map.cpp".encode_utf16().collect::<Vec<_>>(),
            "{name}"
        );
        assert_eq!(
            post_fix_diag_string_constructor.cleanup_guard_after, 4,
            "{name}"
        );
        assert_eq!(
            post_fix_diag_string_constructor.allocation.owner,
            don_replay::continent::MapMakePostFixDiagStringAllocationOwner::CallerLocalMapCpp,
            "{name}"
        );
        assert_eq!(
            post_fix_diag_string_constructor
                .allocation
                .utf16_units_allocated_with_nul,
            8,
            "{name}"
        );
        assert!(
            !post_fix_diag_string_constructor
                .allocation
                .host_pointer_recorded,
            "{name}"
        );
        assert_eq!(
            post_fix_diag_string_constructor.executed_indirect_import_calls,
            [(
                0x00a1_7c60,
                don_replay::post_continent::MULTI_BYTE_TO_WIDE_CHAR_IAT_VA,
            )],
            "{name}"
        );
        assert_eq!(
            post_fix_diag_string_constructor.world_before, fix_diag_land.world_after,
            "{name}"
        );
        assert_eq!(
            post_fix_diag_string_constructor.world_after, coastlines.world_before,
            "{name}"
        );
        assert!(
            post_fix_diag_string_constructor
                .world_sections_changed
                .is_empty(),
            "{name}"
        );
        assert_eq!(
            post_fix_diag_string_constructor.next,
            don_replay::continent::MapMakePostFixDiagStringConstructorNext::GameLogSayChecksum(
                don_replay::continent::MapMakePostFixDiagGameLogCall {
                    source_load_va:
                        don_replay::post_continent::MAP_MAKE_POST_FIX_DIAG_GAME_LOG_SOURCE_LOAD_VA,
                    line_push_va:
                        don_replay::post_continent::MAP_MAKE_POST_FIX_DIAG_GAME_LOG_LINE_PUSH_VA,
                    line_number:
                        don_replay::post_continent::MAP_MAKE_POST_FIX_DIAG_GAME_LOG_LINE_NUMBER,
                    source_push_va:
                        don_replay::post_continent::MAP_MAKE_POST_FIX_DIAG_GAME_LOG_SOURCE_PUSH_VA,
                    mode_push_va:
                        don_replay::post_continent::MAP_MAKE_POST_FIX_DIAG_GAME_LOG_MODE_PUSH_VA,
                    mode: 1,
                    this_load_va:
                        don_replay::post_continent::MAP_MAKE_POST_FIX_DIAG_GAME_LOG_THIS_LOAD_VA,
                    game_log_va: don_replay::post_continent::GAME_LOG_GLOBAL_VA,
                    call_va: don_replay::continent::MAP_MAKE_POST_FIX_DIAG_GAME_LOG_CALL_VA,
                    primitive_va: don_replay::continent::GAME_LOG_SAY_CHECKSUM_VA,
                }
            ),
            "{name}"
        );
        assert_eq!(
            game_log_say_checksum.body,
            don_replay::post_continent::GAME_LOG_SAY_CHECKSUM_NATIVE_BODY,
            "{name}"
        );
        assert_eq!(game_log_say_checksum.call.line_number, 0x1e9b, "{name}");
        assert_eq!(game_log_say_checksum.call.mode, 1, "{name}");
        assert_eq!(
            game_log_say_checksum.owner.source_owner,
            don_replay::continent::MapMakePostFixDiagStringAllocationOwner::CallerLocalMapCpp,
            "{name}"
        );
        assert!(game_log_say_checksum.owner.source_preserved, "{name}");
        assert_eq!(
            game_log_say_checksum.owner.checksum_sequence_delta, 1,
            "{name}"
        );
        assert!(!game_log_say_checksum.owner.host_pointer_recorded, "{name}");
        assert_eq!(game_log_say_checksum.cleanup_guard_after, -1, "{name}");
        assert_eq!(
            game_log_say_checksum.world_before, post_fix_diag_string_constructor.world_after,
            "{name}"
        );
        assert_eq!(
            game_log_say_checksum.world_after, coastlines.world_before,
            "{name}"
        );
        assert!(
            game_log_say_checksum.world_sections_changed.is_empty(),
            "{name}"
        );
        assert_eq!(
            game_log_say_checksum.next,
            don_replay::continent::MapMakePostFixDiagGameLogNext::StringClose {
                local_load_va:
                    don_replay::post_continent::MAP_MAKE_POST_CHECKSUM_STRING_LOCAL_LOAD_VA,
                call_va:
                    don_replay::post_continent::MAP_MAKE_POST_CHECKSUM_STRING_CLOSE_CALL_VA,
                primitive_va: don_replay::post_continent::STRING_CLOSE_VA,
                allocation_owner:
                    don_replay::continent::MapMakePostFixDiagStringAllocationOwner::CallerLocalMapCpp,
                may_release_owned_string_guts: true,
            },
            "{name}"
        );
        assert_eq!(
            post_checksum_string_close.body,
            don_replay::post_continent::MAP_MAKE_STRING_CLOSE_NATIVE_BODY,
            "{name}"
        );
        assert_eq!(
            post_checksum_string_close.scalar_deleting_destructor,
            don_replay::post_continent::STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_BODY,
            "{name}"
        );
        assert_eq!(
            post_checksum_string_close.mem_free,
            don_replay::post_continent::STRING_GUTS_MEM_FREE_BODY,
            "{name}"
        );
        assert_eq!(
            post_checksum_string_close.operator_delete,
            don_replay::post_continent::STRING_GUTS_OPERATOR_DELETE_BODY,
            "{name}"
        );
        assert_eq!(
            post_checksum_string_close.allocator_facts,
            don_replay::continent::MapMakeStringAllocatorFacts::RETAIL_GAMEPLAY,
            "{name}"
        );
        assert_eq!(
            post_checksum_string_close.allocation.buffer_before,
            don_replay::continent::MapMakeStringAllocationState::Live,
            "{name}"
        );
        assert_eq!(
            post_checksum_string_close.allocation.buffer_after,
            don_replay::continent::MapMakeStringAllocationState::ReturnedToRetailPool,
            "{name}"
        );
        assert_eq!(
            post_checksum_string_close.allocation.string_guts_before,
            don_replay::continent::MapMakeStringAllocationState::Live,
            "{name}"
        );
        assert_eq!(
            post_checksum_string_close.allocation.string_guts_after,
            don_replay::continent::MapMakeStringAllocationState::ReturnedToRetailPool,
            "{name}"
        );
        assert_eq!(post_checksum_string_close.allocation.buffer_pool_class, 1);
        assert_eq!(
            post_checksum_string_close.allocation.source_utf16,
            "map.cpp".encode_utf16().collect::<Vec<_>>()
        );
        assert!(!post_checksum_string_close.allocation.host_pointer_recorded);
        assert!(post_checksum_string_close.local_after.data_is_null);
        assert_eq!(post_checksum_string_close.local_after.length, 0);
        assert_eq!(post_checksum_string_close.local_after.hash, 0);
        assert_eq!(
            post_checksum_string_close.world_before, game_log_say_checksum.world_after,
            "{name}"
        );
        assert_eq!(
            post_checksum_string_close.world_after, coastlines.world_before,
            "{name}"
        );
        assert!(post_checksum_string_close.world_sections_changed.is_empty());
        assert_eq!(
            post_checksum_string_close.random_state_before,
            post_checksum_string_close.random_state_after,
            "{name}"
        );
        assert_eq!(
            progress_string.source.table_index,
            don_replay::post_continent::MAP_MAKE_COASTLINES_STRING_TABLE_INDEX,
            "{name}"
        );
        assert_eq!(progress_string.source.resource_hash, 27_580_769, "{name}");
        assert_eq!(
            progress_string.previous_subtitle.table_index,
            don_replay::post_continent::MAP_MAKE_PREVIOUS_PROGRESS_STRING_TABLE_INDEX,
            "{name}"
        );
        assert!(progress_string.source.const_backed, "{name}");
        assert!(progress_string.source.content_is_locale_dependent, "{name}");
        assert!(!progress_string.ownership.allocation_performed, "{name}");
        assert!(
            !progress_string.ownership.allocator_return_performed,
            "{name}"
        );
        assert!(!progress_string.ownership.refcount_changed, "{name}");
        assert!(!progress_string.ownership.host_pointer_recorded, "{name}");
        assert!(progress_string.local_after_close.data_is_null, "{name}");
        assert!(
            progress_string
                .local_after_close
                .source_length_field_is_retained,
            "{name}"
        );
        assert!(
            progress_string.presentation_host_clock_and_draw_effects_unmodeled,
            "{name}"
        );
        assert_eq!(
            progress_string.world_before, post_checksum_string_close.world_after,
            "{name}"
        );
        assert_eq!(
            progress_string.world_after, coastlines.world_before,
            "{name}"
        );
        assert!(progress_string.world_sections_changed.is_empty(), "{name}");
        assert_eq!(
            progress_string.next,
            don_replay::post_continent::MapMakeProgressStringNext::MakeCoastlines {
                caller: don_replay::post_continent::MAP_MAKE_COASTLINES_CALL_BODY,
                call_va: don_replay::post_continent::MAP_MAKE_COASTLINES_CALL_VA,
                primitive_va: don_replay::post_continent::MAP_MAKE_COASTLINES_VA,
            },
            "{name}"
        );
        assert_eq!(
            coastlines.body,
            don_replay::post_continent::MAP_MAKE_COASTLINES_NATIVE_BODY,
            "{name}"
        );
        assert_eq!(
            coastlines.fix_lakes,
            don_replay::post_continent::MAP_FIX_LAKES_NATIVE_BODY,
            "{name}"
        );
        assert_eq!(
            coastlines.world_before, progress_string.world_after,
            "{name}"
        );
        assert_eq!(
            coastlines.world_after, second_regions_clear_all.world_before,
            "{name}"
        );
        assert!(coastlines.direct_rng_sites.is_empty(), "{name}");
        assert_eq!(
            post_coastline_string_constructor.world_before, coastlines.world_after,
            "{name}"
        );
        assert_eq!(post_coastline_game_log.call.line_number, 0x1ea3, "{name}");
        assert_eq!(
            post_coastline_string_close.world_after, second_regions_clear_all.world_before,
            "{name}"
        );
        assert_eq!(
            post_coastline_string_close.next,
            don_replay::post_continent::MapMakePostChecksumStringCloseNext::SecondRegionsClearAll {
                call_va: don_replay::post_continent::MAP_MAKE_SECOND_REGIONS_CLEAR_CALL_VA,
                primitive_va: don_replay::post_continent::REGIONS_CLEAR_ALL_VA,
            },
            "{name}"
        );
        assert_eq!(
            second_regions_find_all.world_before, second_regions_clear_all.world_after,
            "{name}"
        );
        assert_eq!(
            second_regions_find_all.world_after,
            map.world.checksum_sections(),
            "{name}"
        );
        assert_eq!(
            second_regions_tail.world_before, second_regions_find_all.world_after,
            "{name}"
        );
        assert_eq!(
            second_regions_tail.checksum_log_call.line_number, 0x1ea6,
            "{name}"
        );
        assert_eq!(
            second_regions_tail.terrain_progress_source.table_index,
            don_replay::post_continent::MAP_MAKE_TERRAIN_PROGRESS_STRING_TABLE_INDEX,
            "{name}"
        );
        assert_eq!(
            second_regions_tail.next.call_va,
            don_replay::post_continent::MAP_MAKE_FILL_FERTILE_CALL_VA,
            "{name}"
        );
        assert_eq!(
            post_checksum_string_close.next,
            don_replay::continent::MapMakePostChecksumStringCloseNext::ProgressStringConstructor {
                prep: don_replay::post_continent::MAP_MAKE_POST_CLOSE_PROGRESS_PREP_BODY,
                progress_test_va: don_replay::post_continent::MAP_MAKE_POST_CLOSE_PROGRESS_TEST_VA,
                progress_branch_va:
                    don_replay::post_continent::MAP_MAKE_POST_CLOSE_PROGRESS_BRANCH_VA,
                no_progress_target_va:
                    don_replay::post_continent::MAP_MAKE_POST_CLOSE_NO_PROGRESS_TARGET_VA,
                progress_requested: true,
                string_table_load_va:
                    don_replay::post_continent::MAP_MAKE_PROGRESS_STRING_TABLE_LOAD_VA,
                string_table_ptr_va:
                    don_replay::post_continent::MAP_MAKE_PROGRESS_STRING_TABLE_PTR_VA,
                string_byte_offset:
                    don_replay::post_continent::MAP_MAKE_PROGRESS_STRING_BYTE_OFFSET,
                local_load_va: don_replay::post_continent::MAP_MAKE_PROGRESS_STRING_LOCAL_LOAD_VA,
                source_push_va: don_replay::post_continent::MAP_MAKE_PROGRESS_STRING_SOURCE_PUSH_VA,
                call_va: don_replay::post_continent::MAP_MAKE_PROGRESS_STRING_CONSTRUCTOR_CALL_VA,
                primitive_va: don_replay::post_continent::STRING_COPY_CONSTRUCTOR_VA,
            },
            "{name}"
        );
        let mut bad_allocator = don_replay::continent::MapMakeStringAllocatorFacts::RETAIL_GAMEPLAY;
        bad_allocator.debug_heap_enabled = true;
        let mut pre_coastline_world = map.world.clone();
        for mutation in &regions_find_all.world_mutations {
            pre_coastline_world.wdata[mutation.cell].region = mutation.region_after;
            pre_coastline_world.wdata[mutation.cell].region2 = mutation.region2_after;
        }
        for mutation in coastlines.mutations.iter().rev() {
            pre_coastline_world.wdata[mutation.cell] = mutation.before.clone();
        }
        let mut first_regions = map.generation_regions.clone();
        for record in &regions_find_all.region_records {
            first_regions.list[usize::from(record.region)] = record.after.clone();
        }
        first_regions.coords = regions_find_all.scratch.after.clone();
        first_regions.land = regions_find_all.regions_land_after;
        first_regions.sea = regions_find_all.regions_sea_after;
        assert_eq!(
            don_replay::execute_map_make_post_checksum_string_close(
                &pre_coastline_world,
                &first_regions,
                receipt.random_state_after,
                regions_clear_all,
                regions_find_all,
                territory_limits,
                fix_diag_land,
                post_fix_diag_string_constructor,
                game_log_say_checksum,
                bad_allocator,
            )
            .unwrap_err(),
            don_replay::continent::MapMakePostChecksumStringCloseError::AllocatorFactsUnavailable,
            "{name}"
        );
        assert!(
            !post_player_land_cleanup
                .string_close
                .string_guts_destructor_called,
            "{name}"
        );
        assert_eq!(
            prefix.player_land.as_ref(),
            Some(&receipt.body_receipt),
            "{name}"
        );
        assert_eq!(receipt.body_receipt.footprints_visited, 16, "{name}");
        assert_eq!(receipt.body_receipt.effective_radius, 5, "{name}");
        assert_eq!(
            receipt.body_receipt.effective_avoid_continent,
            Some(10),
            "{name}"
        );
        assert_eq!(receipt.body_receipt.outer_candidates, 1_664, "{name}");
        assert_eq!(receipt.body_receipt.outer_cells_written, 1_664, "{name}");
        assert_eq!(receipt.body_receipt.conflict_rejections, 0, "{name}");
        assert_eq!(
            receipt.random_state_before, receipt.random_state_after,
            "{name}"
        );
        assert_eq!(receipt.random_state_before as u32, expected.0, "{name}");
        assert!(receipt.direct_rng_sites.is_empty(), "{name}");
        assert_eq!(
            receipt.start_arrays_checksum_before, receipt.start_arrays_checksum_after,
            "{name}"
        );
        assert_eq!(
            receipt.world_sections_changed,
            [WorldSection::WData],
            "{name}"
        );
        assert_eq!(
            (
                receipt.world_before.full,
                receipt.world_after.full,
                receipt.world_before.section(WorldSection::WData).adler,
                receipt.world_after.section(WorldSection::WData).adler,
            ),
            (expected.1, expected.2, expected.3, expected.4),
            "{name}"
        );
        assert_eq!(
            receipt
                .region_mutations
                .iter()
                .map(|mutation| (
                    mutation.region,
                    mutation.before.size,
                    mutation.after.size,
                    mutation.before.coords.capacity,
                    mutation.after.coords.capacity,
                ))
                .collect::<Vec<_>>(),
            expected.5,
            "{name}"
        );
        assert!(!receipt.world_cell_mutations.is_empty(), "{name}");
        assert!(
            receipt.world_cell_mutations.iter().all(|mutation| {
                let mut after_clear = mutation.after.clone();
                after_clear.region = 0;
                let cell = (mutation.y * map.world.xs + mutation.x) as usize;
                let mut after_find_undone =
                    pre_coastline_world.wdata(mutation.x, mutation.y).clone();
                after_find_undone.region = regions_find_all.world_mutations[cell].region_before;
                after_find_undone.region2 = regions_find_all.world_mutations[cell].region2_before;
                mutation.before != mutation.after && after_find_undone == after_clear
            }),
            "{name}"
        );
        assert_eq!(
            regions_clear_all.world_before, receipt.world_after,
            "{name}"
        );
        assert_eq!(coastlines.world_before, fix_diag_land.world_after, "{name}");
    }
}
