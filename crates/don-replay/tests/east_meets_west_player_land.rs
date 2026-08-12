use don_replay::continent::{
    execute_continent_prefix_with_regions_from_rng,
    execute_east_meets_west_centroid_x_free_cleanup,
    execute_east_meets_west_centroid_y_free_cleanup, execute_east_meets_west_player_land,
    execute_east_meets_west_post_player_land_cleanup, ContinentStop,
    EastMeetsWestCentroidListOwner, EastMeetsWestCentroidXFreeCleanupNext,
    EastMeetsWestCentroidYFreeCleanupNext, EastMeetsWestPlayerLandCall,
    EastMeetsWestPlayerLandNext, EastMeetsWestPostPlayerLandCleanupNext,
    EastMeetsWestStringClosePath, RetailAllocationState, CHECK_PLAYER_LAND_NATIVE_BODY,
    EAST_MEETS_WEST_CENTROID_X_FREE_CALL_VA, EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_BODY,
    EAST_MEETS_WEST_CENTROID_X_FREE_CLEANUP_END_VA,
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
    REGIONS_CLEAR_ALL_VA, RISE_EXE_SHA256, SIMPLE_ARRAY_INT_SIZE, STRING_CLOSE_INSTRUCTION_COUNT,
    STRING_CLOSE_NATIVE_BODY, STRING_CLOSE_SHA256, STRING_CLOSE_SIZE, STRING_CLOSE_VA,
    STRING_GUTS_DESTRUCTOR_CALL_VA, STRING_GUTS_SCALAR_DELETING_DESTRUCTOR_VA,
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

        assert_eq!(*next_va, REGIONS_CLEAR_ALL_VA, "{name}");
        assert_eq!(*next_mutator_va, REGIONS_CLEAR_ALL_VA, "{name}");
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
            receipt
                .world_cell_mutations
                .iter()
                .all(|mutation| mutation.before != mutation.after
                    && map.world.wdata(mutation.x, mutation.y) == &mutation.after),
            "{name}"
        );
        assert_eq!(map.world.checksum_sections(), receipt.world_after, "{name}");
    }
}
