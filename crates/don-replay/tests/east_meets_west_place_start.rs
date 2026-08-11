#[path = "../src/east_meets_west_place_start.rs"]
mod east_meets_west_place_start;

use don_replay::continent::{execute_continent_prefix_with_regions_from_rng, ContinentStop};
use don_replay::fractal_boundary::resolve_tile_selection;
use don_replay::map_style::{ron_data_root_for_replay, MapStyleStaticData};
use don_replay::replay::Replay;
use don_sim::rng::Random;
use don_sim::systems::map_terrain::{land, World};
use don_sim::systems::regions::Regions;
use east_meets_west_place_start::{
    execute_first_place_start_selector, PlaceStartSelectorError, PlaceStartSelectorNext,
    EAST_MEETS_WEST_ADD_START_CALL_VA, EAST_MEETS_WEST_FIRST_PLACE_START_CALL_VA,
    EAST_MEETS_WEST_PLACE_START_FAILURE_VA, EAST_MEETS_WEST_PLACE_START_TEST_VA,
    MAP_PLACE_START_END_VA, MAP_PLACE_START_FAILURE_RETURN_VA, MAP_PLACE_START_IN_REGION_VA,
    MAP_PLACE_START_RNG_CALL_VA, MAP_PLACE_START_SUCCESS_RETURN_VA, WORLD_ADD_STARTING_LOCATION_VA,
};
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
fn both_style19_headers_execute_the_first_exact_selector() {
    for (name, expected) in [
        (
            "Playback___2018.12.01_18_33_16__Sat_.rcx",
            (
                1,
                3_164,
                0xd4d9_96b4,
                0x751b_5283,
                21_122,
                2_138,
                (26, 56),
                0xb2ee_ff92,
                0x2eaa_f2e7,
            ),
        ),
        (
            "Playback___2019.03.24_11_56_19__Sun_.rcx",
            (
                2,
                2_793,
                0x9865_35d1,
                0x8e6c_f4fc,
                62_715,
                1_269,
                (87, 78),
                0xeb49_60c6,
                0x69f0_54b2,
            ),
        ),
    ] {
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
        let ContinentStop::AddStartingLocation {
            selector: got,
            remaining,
            ..
        } = &prefix.stop
        else {
            panic!("{name}: unexpected stop {:?}", prefix.stop);
        };
        assert_eq!(got.primitive_va, MAP_PLACE_START_IN_REGION_VA, "{name}");
        assert_eq!(
            got.caller_va, EAST_MEETS_WEST_FIRST_PLACE_START_CALL_VA,
            "{name}"
        );
        assert_eq!(
            got.caller_test_va, EAST_MEETS_WEST_PLACE_START_TEST_VA,
            "{name}"
        );
        assert_eq!(got.return_va, MAP_PLACE_START_SUCCESS_RETURN_VA, "{name}");
        assert_eq!(got.end_va, MAP_PLACE_START_END_VA, "{name}");
        assert_eq!(got.min_distance, 24, "{name}");
        assert_eq!(got.unread_argument, 12, "{name}");
        assert!(got.used_world_start_arrays, "{name}");
        assert_eq!(got.existing_starts, 0, "{name}");
        assert_eq!(
            got.next,
            don_replay::east_meets_west_place_start::PlaceStartSelectorNext::AddStartingLocation {
                caller_va: EAST_MEETS_WEST_ADD_START_CALL_VA,
                primitive_va: WORLD_ADD_STARTING_LOCATION_VA,
            },
            "{name}"
        );
        assert_eq!(got.full_checksum_before, got.full_checksum_after, "{name}");
        assert_eq!(
            got.wdata_checksum_before, got.wdata_checksum_after,
            "{name}"
        );
        assert_eq!(
            (
                got.region,
                got.region_size,
                got.random_state_before as u32,
                got.random_state_after as u32,
                got.draws[0].raw,
                got.draws[0].anchor,
                got.output.unwrap(),
                got.full_checksum_after,
                got.wdata_checksum_after,
            ),
            expected,
            "{name}"
        );
        assert_eq!(got.draws.len(), 1, "{name}");
        assert_eq!(got.draws[0].pass, 1, "{name}");
        assert_eq!(got.draws[0].call_va, MAP_PLACE_START_RNG_CALL_VA, "{name}");
        assert_eq!(got.accepted_pass, Some(1), "{name}");
        assert_eq!(
            remaining.random_state_before, got.random_state_after,
            "{name}"
        );
        assert_eq!(prefix.rng_final, remaining.random_state_after, "{name}");
        assert_eq!(
            prefix.direct_rng_sites.last(),
            Some(&MAP_PLACE_START_RNG_CALL_VA)
        );
    }
}

fn one_coord_world(x: i32, y: i32) -> (World, Regions) {
    let mut world = World::init_default_rules(7, 7);
    world.seed_map_generation(7);
    for cell in &mut world.wdata {
        cell.land = land::FERTILE;
        cell.flags = 0;
    }
    let mut regions = Regions::default();
    regions.list[1].size = 1;
    regions.list[1].coords.capacity = 1;
    regions.list[1].coords.items.push((x, y));
    (world, regions)
}

#[test]
fn singleton_second_pass_and_failure_edges_consume_no_rng() {
    let (world, regions) = one_coord_world(3, 3);
    let mut rng = Random::new(0x1234_5678);
    let second = execute_first_place_start_selector(&world, &regions, &mut rng, 1, 24, 12).unwrap();
    assert_eq!(second.output, Some((3, 3)));
    assert_eq!(second.accepted_pass, Some(2));
    assert_eq!(second.return_va, MAP_PLACE_START_SUCCESS_RETURN_VA);
    assert!(second.draws.is_empty());
    assert_eq!(rng.state() as u32, 0x1234_5678);

    let (world, regions) = one_coord_world(0, 0);
    let failed = execute_first_place_start_selector(&world, &regions, &mut rng, 1, 24, 12).unwrap();
    assert_eq!(failed.output, None);
    assert_eq!(failed.accepted_pass, None);
    assert_eq!(failed.return_va, MAP_PLACE_START_FAILURE_RETURN_VA);
    assert_eq!(
        failed.next,
        PlaceStartSelectorNext::CallerFallback {
            next_va: EAST_MEETS_WEST_PLACE_START_FAILURE_VA,
        }
    );
    assert!(failed.draws.is_empty());
    assert_eq!(rng.state() as u32, 0x1234_5678);
}

#[test]
fn malformed_region_is_rejected_before_rng() {
    let (world, mut regions) = one_coord_world(3, 3);
    regions.list[1].size = 2;
    let mut rng = Random::new(99);
    assert_eq!(
        execute_first_place_start_selector(&world, &regions, &mut rng, 1, 24, 12),
        Err(PlaceStartSelectorError::CoordinateStorageShort {
            region: 1,
            size: 2,
            coordinates: 1,
        })
    );
    assert_eq!(rng.state(), 99);
}
