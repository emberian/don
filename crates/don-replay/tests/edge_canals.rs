use don_replay::continent::{execute_continent_prefix_with_regions_from_rng, ContinentStop};
use don_replay::east_meets_west_start_boundary::{
    execute_first_place_start_boundary, EAST_MEETS_WEST_START_ANGLE_RNG_VA,
};
use don_replay::edge_canals::{
    execute_eliminate_edge_canals, EdgeCanalSide, EliminateEdgeCanalsError,
    EAST_MEETS_WEST_AFTER_EDGE_CANALS_VA, EAST_MEETS_WEST_EDGE_CANALS_CALL_VA,
    EAST_MEETS_WEST_FIRST_PLACE_START_CALL_VA, MAP_ELIMINATE_EDGE_CANALS_RETURN_VA,
    MAP_ELIMINATE_EDGE_CANALS_VA, MAP_PLACE_START_IN_REGION_VA,
};
use don_replay::fractal_boundary::resolve_tile_selection;
use don_replay::map_style::{ron_data_root_for_replay, MapStyleStaticData};
use don_replay::replay::Replay;
use don_sim::rng::Random;
use don_sim::systems::map_terrain::{land, wflag, World};
use don_sim::systems::regions::Regions;
use std::path::{Path, PathBuf};

fn seeded_world(edge: i32) -> World {
    let mut world = World::init_default_rules(edge, edge);
    world.seed_map_generation(7);
    for cell in &mut world.wdata {
        cell.land = land::FERTILE;
        cell.land_sub = 9;
        cell.region = 0;
        cell.region2 = 0;
    }
    world
}

fn set_ocean(world: &mut World, x: i32, y: i32) {
    let cell = world.wdata_mut(x, y);
    cell.land = land::OCEAN;
    cell.land_sub = 0;
}

#[test]
fn each_side_executes_both_inward_depths_in_native_order() {
    let fixtures = [
        (
            EdgeCanalSide::Left,
            [(0, 2), (0, 3), (0, 4)],
            [(1, 2), (1, 4)],
            [(1, 3), (2, 3)],
        ),
        (
            EdgeCanalSide::Right,
            [(6, 2), (6, 3), (6, 4)],
            [(5, 2), (5, 4)],
            [(5, 3), (4, 3)],
        ),
        (
            EdgeCanalSide::Top,
            [(2, 0), (3, 0), (4, 0)],
            [(2, 1), (4, 1)],
            [(3, 1), (3, 2)],
        ),
        (
            EdgeCanalSide::Bottom,
            [(2, 6), (3, 6), (4, 6)],
            [(2, 5), (4, 5)],
            [(3, 5), (3, 4)],
        ),
    ];
    for (wanted_side, outer, inner_neighbours, expected) in fixtures {
        let mut world = seeded_world(7);
        let mut regions = Regions::default();
        for (x, y) in outer.into_iter().chain(inner_neighbours) {
            set_ocean(&mut world, x, y);
        }
        let got = execute_eliminate_edge_canals(&mut world, &mut regions).unwrap();
        assert_eq!(got.primitive_va, MAP_ELIMINATE_EDGE_CANALS_VA);
        assert_eq!(got.return_va, MAP_ELIMINATE_EDGE_CANALS_RETURN_VA);
        assert_eq!(got.caller_va, EAST_MEETS_WEST_EDGE_CANALS_CALL_VA);
        assert_eq!(got.next_caller_va, EAST_MEETS_WEST_AFTER_EDGE_CANALS_VA);
        assert_eq!(got.next_external_va, MAP_PLACE_START_IN_REGION_VA);
        assert!(got.direct_rng_sites.is_empty());
        let pass = got
            .passes
            .iter()
            .find(|pass| pass.side == wanted_side)
            .unwrap();
        assert_eq!(
            pass.writes
                .iter()
                .map(|write| (write.x, write.y))
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            pass.writes
                .iter()
                .map(|write| (write.depth, write.prior_land, write.prior_land_sub))
                .collect::<Vec<_>>(),
            [(1, land::FERTILE, 9), (2, land::FERTILE, 9)]
        );
        assert_eq!(world.wdata(expected[0].0, expected[0].1).land, land::OCEAN);
        assert_eq!(world.wdata(expected[1].0, expected[1].1).land, land::OCEAN);
        assert_eq!(got.region_rebuild.land_components_found, 1);
        assert_eq!(got.region_rebuild.sea_components_found, 1);
    }
}

#[test]
fn raw_land_byte_controls_detection_and_only_fertile_targets_are_rewritten() {
    let mut world = seeded_world(7);
    let mut regions = Regions::default();
    for y in 2..=4 {
        set_ocean(&mut world, 0, y);
        world.wdata_mut(0, y).flags |= wflag::WATERHALF;
    }
    set_ocean(&mut world, 1, 2);
    set_ocean(&mut world, 1, 4);
    world.wdata_mut(1, 3).land = land::COASTAL;

    let got = execute_eliminate_edge_canals(&mut world, &mut regions).unwrap();
    let left = &got.passes[0];
    assert!(left.outer_ocean[2..=4].iter().all(|value| *value));
    assert!(left.writes.is_empty());
    assert_eq!(world.wdata(1, 3).land, land::COASTAL);
    assert_eq!(world.wdata(2, 3).land, land::FERTILE);
}

#[test]
fn invalid_stack_array_domains_are_transactional() {
    let mut nonsquare = World::init_default_rules(7, 6);
    nonsquare.seed_map_generation(7);
    let before = nonsquare.checksum_sections();
    let before_wdata = nonsquare.wdata.clone();
    let mut regions = Regions::default();
    let regions_before = regions.clone();
    assert_eq!(
        execute_eliminate_edge_canals(&mut nonsquare, &mut regions),
        Err(EliminateEdgeCanalsError::InvalidWorldShape {
            xs: 7,
            ys: 6,
            size: 42,
            wdata_len: 42,
        })
    );
    assert_eq!(nonsquare.checksum_sections(), before);
    assert_eq!(nonsquare.wdata, before_wdata);
    assert_eq!(regions, regions_before);

    let mut too_wide = World::init_default_rules(101, 101);
    too_wide.seed_map_generation(7);
    assert_eq!(
        execute_eliminate_edge_canals(&mut too_wide, &mut regions),
        Err(EliminateEdgeCanalsError::UnsupportedRetailEdge {
            edge: 101,
            maximum: 100,
        })
    );
}

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
fn both_checksum_bearing_style19_replays_execute_the_whole_body() {
    for (name, expected_checksums, expected_call) in [
        (
            "Playback___2018.12.01_18_33_16__Sat_.rcx",
            (0x32b6_0f72, 0xb2ee_ff92, 0x69cc_02c7, 0x2eaa_f2e7),
            (0, 0, 1, 1, 1, 19, 46, -991_232_000, -1_707_059_882),
        ),
        (
            "Playback___2019.03.24_11_56_19__Sun_.rcx",
            (0xeb49_60c6, 0xeb49_60c6, 0x69f0_54b2, 0x69f0_54b2),
            (0, 0, 1, 1, 2, 52, 80, 2_105_671_680, 1_389_843_798),
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
            next_va,
            centroids,
            edge_canals: got,
            call,
            selector,
            mutation,
            remaining,
        } = &prefix.stop
        else {
            panic!("{name}: unexpected stop {:?}", prefix.stop);
        };
        assert_eq!(
            *primitive_va,
            don_replay::east_meets_west_place_start::WORLD_ADD_STARTING_LOCATION_VA,
            "{name}"
        );
        assert_eq!(
            call.caller_va, EAST_MEETS_WEST_FIRST_PLACE_START_CALL_VA,
            "{name}"
        );
        assert_eq!(
            selector.primitive_va, MAP_PLACE_START_IN_REGION_VA,
            "{name}"
        );
        assert_eq!(
            *caller_va,
            don_replay::east_meets_west_place_start::EAST_MEETS_WEST_ADD_START_CALL_VA,
            "{name}"
        );
        assert_eq!(got.canal_writes(), 0, "{name}");
        assert_eq!(
            got.passes
                .iter()
                .map(|pass| pass.writes.len())
                .collect::<Vec<_>>(),
            [0, 0, 0, 0],
            "{name}"
        );
        assert!(got.direct_rng_sites.is_empty(), "{name}");
        assert_eq!(
            (
                got.full_checksum_before,
                got.full_checksum_after,
                got.wdata_checksum_before,
                got.wdata_checksum_after,
            ),
            expected_checksums,
            "{name}"
        );
        assert_eq!(got.region_rebuild.land_components_found, 2, "{name}");
        assert_eq!(got.region_rebuild.sea_components_found, 1, "{name}");
        assert_eq!(got.region_rebuild.land_consolidations, 0, "{name}");
        assert_eq!(got.region_rebuild.sea_consolidations, 0, "{name}");
        assert_eq!(got.region_rebuild.non_input_pumps, 4, "{name}");
        assert_eq!(
            (
                call.player_slot,
                call.team,
                call.assigned_continent,
                call.selected_continent,
                call.region,
                call.centroid_x,
                call.centroid_y,
                call.base_angle,
                call.angle,
            ),
            expected_call,
            "{name}"
        );
        assert_eq!(call.continent_players, 2, "{name}");
        assert_eq!(call.angle_spacing, 0x2aaa_aaaa, "{name}");
        assert_eq!(call.angle_adjustment, -0x2aaa_aaaa, "{name}");
        assert_eq!(call.radius, 37, "{name}");
        assert_eq!(call.search_distance, 27, "{name}");
        assert_eq!(call.min_start_distance, 24, "{name}");
        assert_eq!(call.unread_argument, 12, "{name}");
        assert!(call.optional_start_arrays_are_null, "{name}");
        assert_eq!(call.random_draw, None, "{name}");
        assert!(call.direct_rng_sites.is_empty(), "{name}");
        if name == "Playback___2018.12.01_18_33_16__Sat_.rcx" {
            let mut singleton_partition = prefix.team_partition.as_ref().unwrap().clone();
            singleton_partition.continent_counts[call.selected_continent as usize] = 1;
            let mut singleton_rng = Random::new(0x1234_5678);
            let singleton = execute_first_place_start_boundary(
                &plan.inputs,
                &map.world,
                &singleton_partition,
                centroids,
                &mut singleton_rng,
            )
            .unwrap();
            assert_eq!(singleton.continent_players, 1);
            assert_eq!(singleton.angle_spacing, 0);
            assert_eq!(singleton.angle_adjustment, 178_946_868);
            assert_eq!(singleton.angle, -812_285_132);
            assert_eq!(
                singleton.direct_rng_sites,
                [EAST_MEETS_WEST_START_ANGLE_RNG_VA]
            );
            let draw = singleton.random_draw.unwrap();
            assert_eq!(draw.call_va, EAST_MEETS_WEST_START_ANGLE_RNG_VA);
            assert_eq!(draw.state_before as u32, 0x1234_5678);
            assert_eq!(draw.raw, 10_102);
            assert_eq!(draw.state_after as u32, 0x7543_2777);
            assert_eq!(singleton_rng.state(), draw.state_after);
        }
        assert_eq!(
            selector.random_state_before,
            prefix.region_growths.last().unwrap().rng_final,
            "edge and first-call setup must not advance the main RNG: {name}"
        );
        assert_eq!(
            remaining.random_state_before, selector.random_state_after,
            "{name}"
        );
        assert_eq!(prefix.rng_final, remaining.random_state_after, "{name}");
        assert_eq!(
            *next_va,
            don_replay::continent::MAP_CHECK_PLAYER_LAND_VA,
            "{name}"
        );
        assert_eq!(remaining.entry_va, mutation.caller_return_va, "{name}");
        assert_eq!(mutation.returned_start_index, 0, "{name}");
        assert_eq!(mutation.input, selector.output.unwrap(), "{name}");
    }
}
