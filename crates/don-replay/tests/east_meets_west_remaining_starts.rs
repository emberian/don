use don_replay::continent::{
    execute_continent_prefix_with_regions_from_rng, execute_remaining_active_starts, ContinentStop,
    EastMeetsWestRemainingStartsNext, MAP_CHECK_PLAYER_LAND_VA,
};
use don_replay::east_meets_west_place_start::{
    EAST_MEETS_WEST_PLACE_START_FAILURE_VA, MAP_PLACE_START_RNG_CALL_VA,
};
use don_replay::fractal_boundary::resolve_tile_selection;
use don_replay::map_style::{ron_data_root_for_replay, MapStyleStaticData};
use don_replay::replay::Replay;
use don_sim::rng::Random;
use don_sim::systems::map_terrain::WorldSection;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
struct ExpectedIteration {
    slot: u8,
    team: u8,
    continent: i32,
    region: i32,
    angle: i32,
    rng_before: u32,
    raw: i32,
    anchor: i32,
    rng_after: u32,
    output: (i32, i32),
    full_after: u32,
    starts_after: u32,
    city_capacity_after: i32,
}

struct ExpectedFixture {
    name: &'static str,
    starts: [(i32, i32); 4],
    rng_before: u32,
    rng_after: u32,
    full_before: u32,
    full_after: u32,
    starts_before: u32,
    starts_after: u32,
    iterations: [ExpectedIteration; 3],
}

const FIXTURES: [ExpectedFixture; 2] = [
    ExpectedFixture {
        name: "Playback___2018.12.01_18_33_16__Sat_.rcx",
        starts: [(26, 56), (26, 81), (58, 63), (81, 71)],
        rng_before: 0x751b_5283,
        rng_after: 0x2048_1b28,
        full_before: 0x35db_0949,
        full_after: 0x4eb6_10df,
        starts_before: 0xbfa8_09a9,
        starts_after: 0x8446_113f,
        iterations: [
            ExpectedIteration {
                slot: 1,
                team: 0,
                continent: 1,
                region: 1,
                angle: -1_707_059_882,
                rng_before: 0x751b_5283,
                raw: 22_021,
                anchor: 3_037,
                rng_after: 0x207d_5606,
                output: (26, 81),
                full_after: 0x9634_0b6e,
                starts_after: 0x2e99_0bce,
                city_capacity_after: 8,
            },
            ExpectedIteration {
                slot: 2,
                team: 1,
                continent: 0,
                region: 2,
                angle: 357_913_942,
                rng_before: 0x207d_5606,
                raw: 46_508,
                anchor: 1_916,
                rng_after: 0x39a8_b5ad,
                output: (58, 63),
                full_after: 0x251f_0de1,
                starts_after: 0x117e_0e41,
                city_capacity_after: 16,
            },
            ExpectedIteration {
                slot: 3,
                team: 1,
                continent: 0,
                region: 2,
                angle: 357_913_942,
                rng_before: 0x39a8_b5ad,
                raw: 6_951,
                anchor: 1_377,
                rng_after: 0x2048_1b28,
                output: (81, 71),
                full_after: 0x4eb6_10df,
                starts_after: 0x8446_113f,
                city_capacity_after: 16,
            },
        ],
    },
    ExpectedFixture {
        name: "Playback___2019.03.24_11_56_19__Sun_.rcx",
        starts: [(87, 78), (52, 34), (18, 27), (19, 74)],
        rng_before: 0x8e6c_f4fc,
        rng_after: 0x18a5_6595,
        full_before: 0xfb5e_6c0d,
        full_after: 0x660c_7097,
        starts_before: 0xfcbd_0b48,
        starts_after: 0x5c97_0fd2,
        iterations: [
            ExpectedIteration {
                slot: 1,
                team: 1,
                continent: 0,
                region: 1,
                angle: -869_509_802,
                rng_before: 0x8e6c_f4fc,
                raw: 52_266,
                anchor: 2_244,
                rng_after: 0xce2f_cc2b,
                output: (52, 34),
                full_after: 0x5ef4_6dc9,
                starts_after: 0xa50a_0d04,
                city_capacity_after: 8,
            },
            ExpectedIteration {
                slot: 2,
                team: 1,
                continent: 0,
                region: 1,
                angle: -869_509_802,
                rng_before: 0xce2f_cc2b,
                raw: 29_581,
                anchor: 1_791,
                rng_after: 0xb068_738e,
                output: (18, 27),
                full_after: 0x3140_6ec0,
                starts_after: 0x5eb3_0dfb,
                city_capacity_after: 16,
            },
            ExpectedIteration {
                slot: 3,
                team: 0,
                continent: 1,
                region: 2,
                angle: 1_389_843_798,
                rng_before: 0xb068_738e,
                raw: 26_004,
                anchor: 867,
                rng_after: 0x18a5_6595,
                output: (19, 74),
                full_after: 0x660c_7097,
                starts_after: 0x5c97_0fd2,
                city_capacity_after: 16,
            },
        ],
    },
];

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
fn both_style19_headers_execute_every_remaining_active_start() {
    for expected in &FIXTURES {
        let path = replay_path(expected.name);
        if !path.is_file() {
            eprintln!("SKIPPED — NOT A PASS: missing {}", path.display());
            return;
        }
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
            next_va,
            mutation: first_mutation,
            remaining,
            player_land,
            regions_clear_all,
            regions_find_all,
            next_mutator_va,
            ..
        } = &prefix.stop
        else {
            panic!("{}: unexpected stop {:?}", expected.name, prefix.stop);
        };

        assert_eq!(remaining.entry_va, 0x0069_7466, "{}", expected.name);
        assert_eq!(remaining.record_x_va, 0x0069_746c, "{}", expected.name);
        assert_eq!(remaining.record_y_va, 0x0069_7473, "{}", expected.name);
        assert_eq!(
            remaining.start_count_increment_va, 0x0069_747a,
            "{}",
            expected.name
        );
        assert_eq!(
            remaining.player_increment_va, 0x0069_747e,
            "{}",
            expected.name
        );
        assert_eq!(remaining.loop_jump_va, 0x0069_747f, "{}", expected.name);
        assert_eq!(remaining.loop_head_va, 0x0069_6d28, "{}", expected.name);
        assert_eq!(remaining.active_test_va, 0x0069_6d3a, "{}", expected.name);
        assert_eq!(remaining.loop_exit_va, 0x0069_7484, "{}", expected.name);
        assert_eq!(
            remaining.active_slots, plan.inputs.active_slots,
            "{}",
            expected.name
        );
        assert_eq!(remaining.active_slots, [0, 1, 2, 3], "{}", expected.name);
        assert_eq!(
            remaining.inactive_tail_slots,
            [4, 5, 6, 7],
            "{}",
            expected.name
        );
        assert_eq!(
            remaining.first_recorded_start, expected.starts[0],
            "{}",
            expected.name
        );
        assert_eq!(remaining.initial_start_count, 1, "{}", expected.name);
        assert_eq!(
            remaining.recorded_starts, expected.starts,
            "{}",
            expected.name
        );
        assert_eq!(remaining.iterations.len(), 3, "{}", expected.name);
        assert_eq!(prefix.starts_added, 4, "{}", expected.name);
        assert_eq!(
            *next_va,
            don_replay::continent::MAP_MAKE_FIRST_REGIONS_FIND_RESUME_VA,
            "{}",
            expected.name
        );
        assert_eq!(
            *next_mutator_va,
            don_replay::continent::MAP_MAKE_FIRST_TERRITORY_STORE_VA,
            "{}",
            expected.name
        );
        assert_eq!(
            remaining.next,
            EastMeetsWestRemainingStartsNext::CheckPlayerLand {
                caller_va: 0x0069_7492,
                primitive_va: MAP_CHECK_PLAYER_LAND_VA,
            },
            "{}",
            expected.name
        );

        let mut full_before = expected.full_before;
        let mut starts_before = expected.starts_before;
        for (index, (iteration, expected_iteration)) in remaining
            .iterations
            .iter()
            .zip(expected.iterations)
            .enumerate()
        {
            assert_eq!(
                iteration.player_slot, expected_iteration.slot,
                "{}",
                expected.name
            );
            assert!(
                iteration.skipped_inactive_slots_before.is_empty(),
                "{}",
                expected.name
            );
            assert_eq!(iteration.start_index_before, index + 1, "{}", expected.name);
            assert_eq!(
                iteration.call.team, expected_iteration.team,
                "{}",
                expected.name
            );
            assert_eq!(
                iteration.call.assigned_continent, expected_iteration.continent,
                "{}",
                expected.name
            );
            assert_eq!(
                iteration.call.region, expected_iteration.region,
                "{}",
                expected.name
            );
            assert_eq!(
                iteration.call.angle, expected_iteration.angle,
                "{}",
                expected.name
            );
            assert_eq!(iteration.call.continent_players, 2, "{}", expected.name);
            assert_eq!(iteration.call.min_start_distance, 24, "{}", expected.name);
            assert_eq!(iteration.call.random_draw, None, "{}", expected.name);
            assert_eq!(
                iteration.selector.existing_starts,
                index + 1,
                "{}",
                expected.name
            );
            assert_eq!(
                iteration.selector.random_state_before as u32, expected_iteration.rng_before,
                "{}",
                expected.name
            );
            assert_eq!(iteration.selector.draws.len(), 1, "{}", expected.name);
            assert_eq!(
                iteration.selector.draws[0].call_va,
                MAP_PLACE_START_RNG_CALL_VA
            );
            assert_eq!(
                (
                    iteration.selector.draws[0].raw,
                    iteration.selector.draws[0].anchor
                ),
                (expected_iteration.raw, expected_iteration.anchor),
                "{}",
                expected.name
            );
            assert_eq!(
                iteration.selector.random_state_after as u32, expected_iteration.rng_after,
                "{}",
                expected.name
            );
            assert_eq!(
                iteration.selector.output,
                Some(expected_iteration.output),
                "{}",
                expected.name
            );
            assert_eq!(
                iteration.selector.accepted_pass,
                Some(1),
                "{}",
                expected.name
            );
            let mutation = iteration.mutation.as_ref().expect("real selector succeeds");
            assert_eq!(
                mutation.input, expected_iteration.output,
                "{}",
                expected.name
            );
            assert_eq!(
                mutation.returned_start_index,
                (index + 1) as i32,
                "{}",
                expected.name
            );
            assert_eq!(
                mutation.full_checksum_before, full_before,
                "{}",
                expected.name
            );
            assert_eq!(
                mutation.full_checksum_after, expected_iteration.full_after,
                "{}",
                expected.name
            );
            assert_eq!(
                mutation.start_arrays_checksum_before, starts_before,
                "{}",
                expected.name
            );
            assert_eq!(
                mutation.start_arrays_checksum_after, expected_iteration.starts_after,
                "{}",
                expected.name
            );
            assert_eq!(
                mutation.arrays_after.start_x.capacity, 4,
                "{}",
                expected.name
            );
            assert_eq!(
                mutation.arrays_after.start_city_x.capacity, expected_iteration.city_capacity_after,
                "{}",
                expected.name
            );
            full_before = expected_iteration.full_after;
            starts_before = expected_iteration.starts_after;
        }

        assert_eq!(
            remaining.random_state_before as u32, expected.rng_before,
            "{}",
            expected.name
        );
        assert_eq!(
            remaining.random_state_after as u32, expected.rng_after,
            "{}",
            expected.name
        );
        assert_eq!(
            prefix.rng_final as u32, expected.rng_after,
            "{}",
            expected.name
        );
        assert_eq!(remaining.direct_rng_sites, [MAP_PLACE_START_RNG_CALL_VA; 3]);
        assert_eq!(
            remaining.full_checksum_before, expected.full_before,
            "{}",
            expected.name
        );
        assert_eq!(
            remaining.full_checksum_after, expected.full_after,
            "{}",
            expected.name
        );
        assert_eq!(
            remaining.start_arrays_checksum_before, expected.starts_before,
            "{}",
            expected.name
        );
        assert_eq!(
            remaining.start_arrays_checksum_after, expected.starts_after,
            "{}",
            expected.name
        );
        assert_eq!(
            first_mutation.full_checksum_after, expected.full_before,
            "{}",
            expected.name
        );
        assert_eq!(
            player_land.world_before.full, expected.full_after,
            "{}",
            expected.name
        );
        assert_eq!(
            player_land.start_arrays_checksum_before, expected.starts_after,
            "{}",
            expected.name
        );
        assert_eq!(
            player_land.start_arrays_checksum_after, expected.starts_after,
            "{}",
            expected.name
        );
        assert_eq!(
            map.world
                .start_x
                .items
                .iter()
                .copied()
                .zip(map.world.start_y.items.iter().copied())
                .collect::<Vec<_>>(),
            expected.starts,
            "{}",
            expected.name
        );
        let checksum = map.world.checksum_sections();
        assert_eq!(
            regions_clear_all.world_before, player_land.world_after,
            "{}",
            expected.name
        );
        assert_eq!(regions_find_all.world_before, regions_clear_all.world_after);
        assert_eq!(checksum, regions_find_all.world_after, "{}", expected.name);
        assert_eq!(
            regions_find_all.successful_find_calls, 3,
            "{}",
            expected.name
        );
        assert_eq!(
            checksum.section(WorldSection::StartArrays).adler,
            expected.starts_after,
            "{}",
            expected.name
        );
    }
}

#[test]
fn later_selector_failure_stops_before_visiting_another_active_slot() {
    let name = FIXTURES[0].name;
    let path = replay_path(name);
    if !path.is_file() {
        eprintln!("SKIPPED — NOT A PASS: missing {}", path.display());
        return;
    }
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
    let partition = prefix.team_partition.as_ref().unwrap().clone();
    let ContinentStop::AddStartingLocation {
        centroids,
        call,
        selector,
        mutation,
        remaining,
        player_land,
        regions_clear_all,
        regions_find_all,
        ..
    } = prefix.stop
    else {
        panic!("unexpected stop {:?}", prefix.stop);
    };

    for mutation in &regions_find_all.world_mutations {
        map.world.wdata[mutation.cell].region = mutation.region_before;
        map.world.wdata[mutation.cell].region2 = mutation.region2_before;
    }
    for record in &regions_find_all.region_records {
        map.generation_regions.list[usize::from(record.region)] = record.before.clone();
    }
    map.generation_regions.coords = regions_find_all.scratch.before.clone();
    map.generation_regions.land = regions_find_all.regions_land_before;
    map.generation_regions.sea = regions_find_all.regions_sea_before;
    assert_eq!(map.world.checksum_sections(), regions_clear_all.world_after);

    for mutation in &regions_clear_all.world_region_mutations {
        map.world.wdata[mutation.cell].region = mutation.region_before;
    }
    for record in &regions_clear_all.region_records {
        map.generation_regions.list[usize::from(record.region)] = record.before.clone();
    }
    map.generation_regions.coords = regions_clear_all.regions_coords_before.clone();
    map.generation_regions.land = regions_clear_all.regions_land_before;
    map.generation_regions.sea = regions_clear_all.regions_sea_before;
    assert_eq!(map.world.checksum_sections(), player_land.world_after);

    for write in &player_land.world_cell_mutations {
        assert_eq!(map.world.wdata(write.x, write.y), &write.after);
        *map.world.wdata_mut(write.x, write.y) = write.before.clone();
    }
    for mutation in &player_land.region_mutations {
        let region = &mut map.generation_regions.list[mutation.region as usize];
        assert_eq!(region, &mutation.after);
        *region = mutation.before.clone();
    }
    assert_eq!(map.world.checksum_sections(), player_land.world_before);

    for later in remaining.iterations.iter().rev() {
        for write in later
            .mutation
            .as_ref()
            .unwrap()
            .occupancy_writes
            .iter()
            .rev()
        {
            map.world.start_city_locs[write.byte_index] = write.byte_before;
        }
    }
    map.world.start_x.items.truncate(1);
    map.world.start_y.items.truncate(1);
    map.world.start_city_x.items.truncate(4);
    map.world.start_city_y.items.truncate(4);
    map.world.start_x.capacity = mutation.arrays_after.start_x.capacity;
    map.world.start_y.capacity = mutation.arrays_after.start_y.capacity;
    map.world.start_city_x.capacity = mutation.arrays_after.start_city_x.capacity;
    map.world.start_city_y.capacity = mutation.arrays_after.start_city_y.capacity;
    assert_eq!(
        map.world.checksum_sections().full,
        mutation.full_checksum_after
    );

    let region_index = remaining.iterations[0].call.region as usize;
    let region_size = map.generation_regions.list[region_index].size as usize;
    map.generation_regions.list[region_index].coords.items[..region_size].fill(mutation.input);
    let mut rng = Random::new(remaining.random_state_before);
    let before = map.world.checksum_sections();
    let got = execute_remaining_active_starts(
        &plan.inputs,
        &mut map.world,
        &map.generation_regions,
        &partition,
        &centroids,
        &call,
        &selector,
        &mutation,
        &mut rng,
    )
    .unwrap();

    assert_eq!(got.iterations.len(), 1);
    assert_eq!(got.iterations[0].player_slot, 1);
    assert_eq!(got.iterations[0].selector.draws.len(), 2);
    assert_eq!(got.iterations[0].selector.output, None);
    assert_eq!(got.iterations[0].mutation, None);
    assert_eq!(got.recorded_starts, [mutation.input]);
    assert_eq!(got.direct_rng_sites, [MAP_PLACE_START_RNG_CALL_VA; 2]);
    assert_eq!(
        got.next,
        EastMeetsWestRemainingStartsNext::CallerFallback {
            player_slot: 1,
            next_va: EAST_MEETS_WEST_PLACE_START_FAILURE_VA,
        }
    );
    assert_eq!(map.world.checksum_sections(), before);
    assert_eq!(map.world.start_x.items, [mutation.input.0]);
    assert_eq!(map.world.start_y.items, [mutation.input.1]);
    assert_eq!(rng.state(), got.random_state_after);
}
