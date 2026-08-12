use don_replay::continent::{
    execute_continent_prefix_with_regions_from_rng, execute_east_meets_west_player_land,
    ContinentStop, EastMeetsWestPlayerLandCall, EastMeetsWestPlayerLandNext,
    CHECK_PLAYER_LAND_NATIVE_BODY, EAST_MEETS_WEST_PLAYER_LAND_CALLER_ENTRY_VA,
    EAST_MEETS_WEST_PLAYER_LAND_CALL_VA, EAST_MEETS_WEST_PLAYER_LAND_GUARD_STORE_VA,
    EAST_MEETS_WEST_PLAYER_LAND_RESUME_VA, EAST_MEETS_WEST_PLAYER_LAND_STRING_CLOSE_CALL_VA,
    MAP_CHECK_PLAYER_LAND_END_VA, MAP_CHECK_PLAYER_LAND_INSTRUCTION_COUNT,
    MAP_CHECK_PLAYER_LAND_RET_VA, MAP_CHECK_PLAYER_LAND_SHA256, MAP_CHECK_PLAYER_LAND_SIZE,
    RISE_EXE_SHA256, STRING_CLOSE_VA,
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
            remaining,
            player_land: receipt,
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

        assert_eq!(*next_va, EAST_MEETS_WEST_PLAYER_LAND_RESUME_VA, "{name}");
        assert_eq!(*next_mutator_va, STRING_CLOSE_VA, "{name}");
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
