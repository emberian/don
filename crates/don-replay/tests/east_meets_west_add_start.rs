#[path = "../src/east_meets_west_add_start.rs"]
mod east_meets_west_add_start;

use don_replay::continent::{execute_continent_prefix_with_regions_from_rng, ContinentStop};
use don_replay::fractal_boundary::resolve_tile_selection;
use don_replay::map_style::{ron_data_root_for_replay, MapStyleStaticData};
use don_replay::replay::Replay;
use don_sim::systems::map_terrain::World;
use east_meets_west_add_start::{
    execute_first_add_start, EastMeetsWestAddStartError, EastMeetsWestAddStartInput,
    EAST_MEETS_WEST_ADD_START_CALL_VA, EAST_MEETS_WEST_ADD_START_RETURN_VA,
    EAST_MEETS_WEST_RECORD_START_X_VA, EAST_MEETS_WEST_RECORD_START_Y_VA,
    EAST_MEETS_WEST_START_LOOP_HEAD_VA, EAST_MEETS_WEST_START_LOOP_JUMP_VA,
    MAP_PLACE_START_SUCCESS_RETURN_VA, WORLD_ADD_STARTING_LOCATION_END_VA,
    WORLD_ADD_STARTING_LOCATION_RETURN_VA, WORLD_ADD_STARTING_LOCATION_VA,
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
fn both_style19_headers_execute_the_first_exact_start_append() {
    for (name, expected) in [
        (
            "Playback___2018.12.01_18_33_16__Sat_.rcx",
            ((26, 56), 0xb2ee_ff92, 0x35db_0949, 0xbfa8_09a9),
        ),
        (
            "Playback___2019.03.24_11_56_19__Sun_.rcx",
            ((87, 78), 0xeb49_60c6, 0xfb5e_6c0d, 0xfcbd_0b48),
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
            primitive_va,
            caller_va,
            selector,
            mutation: got,
            ..
        } = &prefix.stop
        else {
            panic!("{name}: unexpected stop {:?}", prefix.stop);
        };
        assert_eq!(got.primitive_va, WORLD_ADD_STARTING_LOCATION_VA, "{name}");
        assert_eq!(*primitive_va, got.primitive_va, "{name}");
        assert_eq!(
            got.return_va, WORLD_ADD_STARTING_LOCATION_RETURN_VA,
            "{name}"
        );
        assert_eq!(got.end_va, WORLD_ADD_STARTING_LOCATION_END_VA, "{name}");
        assert_eq!(got.caller_va, EAST_MEETS_WEST_ADD_START_CALL_VA, "{name}");
        assert_eq!(*caller_va, got.caller_va, "{name}");
        assert_eq!(
            got.caller_return_va, EAST_MEETS_WEST_ADD_START_RETURN_VA,
            "{name}"
        );
        assert_eq!(
            got.caller_record_x_va, EAST_MEETS_WEST_RECORD_START_X_VA,
            "{name}"
        );
        assert_eq!(
            got.caller_record_y_va, EAST_MEETS_WEST_RECORD_START_Y_VA,
            "{name}"
        );
        assert_eq!(
            got.caller_loop_jump_va, EAST_MEETS_WEST_START_LOOP_JUMP_VA,
            "{name}"
        );
        assert_eq!(
            got.caller_loop_head_va, EAST_MEETS_WEST_START_LOOP_HEAD_VA,
            "{name}"
        );
        assert_eq!(got.returned_start_index, 0, "{name}");
        assert_eq!(got.input, selector.output.unwrap(), "{name}");
        assert_eq!(got.input, expected.0, "{name}");
        assert_eq!(got.appended_start, got.input, "{name}");
        assert_eq!(
            got.appended_city_x,
            [got.input.0, got.input.0 - 1, got.input.0, got.input.0 - 1],
            "{name}"
        );
        assert_eq!(
            got.appended_city_y,
            [got.input.1, got.input.1, got.input.1 - 1, got.input.1 - 1],
            "{name}"
        );
        assert_eq!(got.arrays_before.start_x.length, 0, "{name}");
        assert_eq!(got.arrays_before.start_y.length, 0, "{name}");
        assert_eq!(got.arrays_before.start_city_x.length, 0, "{name}");
        assert_eq!(got.arrays_before.start_city_y.length, 0, "{name}");
        for state in [
            got.arrays_before.start_x,
            got.arrays_before.start_y,
            got.arrays_before.start_city_x,
            got.arrays_before.start_city_y,
        ] {
            assert_eq!((state.capacity, state.increment, state.flags), (0, -1, 0));
        }
        assert_eq!(got.arrays_after.start_x.length, 1, "{name}");
        assert_eq!(got.arrays_after.start_y.length, 1, "{name}");
        assert_eq!(got.arrays_after.start_city_x.length, 4, "{name}");
        assert_eq!(got.arrays_after.start_city_y.length, 4, "{name}");
        for state in [
            got.arrays_after.start_x,
            got.arrays_after.start_y,
            got.arrays_after.start_city_x,
            got.arrays_after.start_city_y,
        ] {
            assert_eq!((state.capacity, state.increment, state.flags), (4, -1, 0));
        }
        assert_eq!(map.world.start_x.items, [got.input.0], "{name}");
        assert_eq!(map.world.start_y.items, [got.input.1], "{name}");
        assert_eq!(map.world.start_city_x.items, got.appended_city_x, "{name}");
        assert_eq!(map.world.start_city_y.items, got.appended_city_y, "{name}");
        for write in got.occupancy_writes {
            assert_ne!(write.mask, 0, "{name}");
            assert_eq!(write.byte_after, write.byte_before | write.mask, "{name}");
            assert_ne!(map.world.start_city_locs[write.byte_index] & write.mask, 0);
        }
        let occupancy = got.occupancy_writes.map(|write| {
            (
                write.flat_index,
                write.byte_index,
                write.mask,
                write.byte_before,
                write.byte_after,
            )
        });
        let expected_occupancy = if got.input == (26, 56) {
            [
                (5_626, 703, 0x04, 0x00, 0x04),
                (5_625, 703, 0x02, 0x04, 0x06),
                (5_526, 690, 0x40, 0x00, 0x40),
                (5_525, 690, 0x20, 0x40, 0x60),
            ]
        } else {
            [
                (7_887, 985, 0x80, 0x00, 0x80),
                (7_886, 985, 0x40, 0x80, 0xc0),
                (7_787, 973, 0x08, 0x00, 0x08),
                (7_786, 973, 0x04, 0x08, 0x0c),
            ]
        };
        assert_eq!(occupancy, expected_occupancy, "{name}");
        assert_eq!(
            (
                got.full_checksum_before,
                got.full_checksum_after,
                got.start_arrays_checksum_before,
                got.start_arrays_checksum_after,
            ),
            (expected.1, expected.2, 0x0010_0001, expected.3),
            "{name}"
        );
        assert_eq!(prefix.rng_final, selector.random_state_after, "{name}");
    }
}

fn exact_input(output: Option<(i32, i32)>) -> EastMeetsWestAddStartInput {
    EastMeetsWestAddStartInput {
        selector_return_va: MAP_PLACE_START_SUCCESS_RETURN_VA,
        caller_va: EAST_MEETS_WEST_ADD_START_CALL_VA,
        primitive_va: WORLD_ADD_STARTING_LOCATION_VA,
        output,
    }
}

#[test]
fn selector_failure_preserves_the_world_and_fallback_semantics() {
    let mut world = World::init_default_rules(16, 16);
    let checksum = world.checksum_sections();
    let before = format!("{world:?}");
    assert_eq!(
        execute_first_add_start(&mut world, exact_input(None)),
        Err(EastMeetsWestAddStartError::SelectorDidNotReachCall {
            selector_return_va: MAP_PLACE_START_SUCCESS_RETURN_VA,
            caller_va: EAST_MEETS_WEST_ADD_START_CALL_VA,
            primitive_va: WORLD_ADD_STARTING_LOCATION_VA,
            output: None,
        })
    );
    assert_eq!(world.checksum_sections(), checksum);
    assert_eq!(format!("{world:?}"), before);
}

#[test]
fn malformed_storage_is_rejected_transactionally() {
    let mut world = World::init_default_rules(16, 16);
    world.start_x.increment = 0;
    let checksum = world.checksum_sections();
    let before = format!("{world:?}");
    assert_eq!(
        execute_first_add_start(&mut world, exact_input(Some((8, 9)))),
        Err(EastMeetsWestAddStartError::InvalidArrayMetadata {
            array: "start_x",
            length: 0,
            capacity: 0,
            increment: 0,
            additions: 1,
        })
    );
    assert_eq!(world.checksum_sections(), checksum);
    assert_eq!(format!("{world:?}"), before);
}
