use don_replay::harness::WorldSim;
use don_replay::replay::Replay;
use don_sim::systems::map_terrain::{space, tflag, Coord, TCoord, World};
use std::path::{Path, PathBuf};

#[path = "../src/starting_village_world_schedule.rs"]
mod subject;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn world(xs: i32, ys: i32) -> World {
    World::init(xs, ys, 44, 4, 4)
}

fn request(center_tx: i32, center_ty: i32, radius: i32) -> subject::WallMaskCityRequest {
    subject::WallMaskCityRequest {
        center_tcoord: (center_tx, center_ty),
        radius_tiles: radius,
        on: 1,
        owner: 0,
        object_id: 2000,
        native_call_flags: subject::FRESH_NATIVE_MASK_CITY_FLAGS,
        native_call_city: -1,
        post_activation_flags: subject::FRESH_STARTING_CENTER_FLAGS,
    }
}

#[test]
fn even_circle_table_matches_native_endpoints_and_order() {
    let table = subject::EvenCircleTable::build();
    assert_eq!(table.radius[0], 0);
    assert_eq!(table.radius[1], 4);
    assert_eq!(table.radius[2], 12);
    assert_eq!(table.radius[20], 1232);
    assert_eq!(table.radius[24], 1788);
    assert_eq!(table.radius[64], 12828);
    assert_eq!(table.x.len(), 12828);
    assert_eq!(table.y.len(), 12828);
    assert_eq!(
        table.x[..4]
            .iter()
            .copied()
            .zip(table.y[..4].iter().copied())
            .collect::<Vec<_>>(),
        vec![(-1, -1), (-1, 0), (0, -1), (0, 0)]
    );

    let r20 = table.radius[20] as usize;
    let points = table.x[..r20]
        .iter()
        .copied()
        .zip(table.y[..r20].iter().copied())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(points.len(), r20, "native disc has no duplicate writes");
    assert_eq!(points.iter().next(), Some(&(-20, -4)));
    assert_eq!(points.iter().next_back(), Some(&(19, 3)));
}

#[test]
fn radius_one_installs_the_even_centered_two_by_two_in_instruction_order() {
    let mut world = world(4, 4);
    let terrain_index = world.t_index(2, 2);
    let already_city_index = world.t_index(3, 3);
    world.tdata[terrain_index] = tflag::BEHIND_B;
    world.tdata[already_city_index] = tflag::CITY;

    let receipt = subject::apply_starting_village_city_mask(&mut world, request(3, 3, 1))
        .expect("fresh CITY mask");

    assert_eq!(receipt.table_endpoint, 4);
    assert_eq!(receipt.offsets_considered, 4);
    assert_eq!(receipt.in_bounds_offsets, 4);
    assert_eq!(receipt.out_of_bounds_offsets, 0);
    assert_eq!(receipt.tdata_city_write_instructions, 4);
    assert_eq!(receipt.tdata_cells_changed, 3);
    assert_eq!(receipt.tdata_cells_already_set, 1);
    for (tx, ty) in [(2, 2), (2, 3), (3, 2), (3, 3)] {
        assert_ne!(world.tmask(tx, ty) & tflag::CITY, 0);
    }
    assert_ne!(
        world.tmask(2, 2) & tflag::BEHIND_B,
        0,
        "OR preserves terrain"
    );
    assert_eq!(world.tmask(1, 2) & tflag::CITY, 0);
    assert!(receipt.city_mask_complete);
    assert!(receipt.pre_strategy.schedule_attested);
    assert!(!receipt.pre_strategy.activation_mask_inputs_joined);
    assert!(!receipt.pre_strategy.final_territory_image_joined);
    assert!(!receipt.pre_strategy.unit_derived_city_fields_joined);
    assert!(!receipt.first_checksum_world_image_ready);
    assert!(!receipt.first_checksum_city_image_ready);
    assert_eq!(receipt.main_rng_draws, 0);
}

#[test]
fn ordinary_village_installs_1232_city_cells_and_unblocks_need_city_probe() {
    let mut world = world(16, 16); // 64x64 TData
    let receipt = subject::apply_starting_village_city_mask(&mut world, request(32, 32, 20))
        .expect("radius-20 CITY mask");

    assert_eq!(receipt.table_endpoint, 1232);
    assert_eq!(receipt.in_bounds_offsets, 1232);
    assert_eq!(receipt.tdata_cells_changed, 1232);
    assert_eq!(
        world
            .tdata
            .iter()
            .filter(|&&mask| mask & tflag::CITY != 0)
            .count(),
        1232
    );
    assert_eq!(world.space_at_corner(32, 32, 0, true), space::FULLY_CLEAR);
}

#[test]
fn map_edge_clipping_matches_wall_mask_city() {
    let mut world = world(2, 2);
    let receipt = subject::apply_starting_village_city_mask(&mut world, request(0, 0, 1))
        .expect("native skips off-map offsets");
    assert_eq!(receipt.in_bounds_offsets, 1);
    assert_eq!(receipt.out_of_bounds_offsets, 3);
    assert_ne!(world.tmask(0, 0) & tflag::CITY, 0);
}

#[test]
fn malformed_activation_receipts_fail_before_world_mutation() {
    let mut world = world(4, 4);
    let before = world.tdata.clone();
    let mut bad = request(3, 3, 20);
    bad.on = 0;
    assert_eq!(
        subject::apply_starting_village_city_mask(&mut world, bad),
        Err(subject::StartingVillageWorldError::MaskOperationNotInstall { on: 0 })
    );
    assert_eq!(world.tdata, before);

    let mut bad = request(3, 3, 20);
    bad.native_call_flags = 0x03;
    assert_eq!(
        subject::apply_starting_village_city_mask(&mut world, bad),
        Err(subject::StartingVillageWorldError::NotCityObject {
            native_call_flags: 0x03
        })
    );
    assert_eq!(world.tdata, before);

    let mut bad = request(3, 3, 20);
    bad.native_call_city = 0;
    assert_eq!(
        subject::apply_starting_village_city_mask(&mut world, bad),
        Err(
            subject::StartingVillageWorldError::NativeCityLinkNotSentinel {
                native_call_city: 0
            }
        )
    );
    assert_eq!(world.tdata, before);
}

#[test]
fn real_style_6_and_9_centers_install_the_source_even_circle_disc() {
    let names = [
        "Playback___2018.11.17_13_21_42__Sat_.rcx",
        "Playback___2020.02.08_10_49_15__Sat_.rcx",
        "Playback___2020.02.21_09_48_48__Fri_.rcx",
    ];
    let mut fixtures = 0usize;
    for name in names {
        let path = repo_root().join("ron-data/replays/multi").join(name);
        if !path.exists() {
            continue;
        }
        fixtures += 1;
        let replay = Replay::open(&path)
            .unwrap_or_else(|error| panic!("{name}: replay parse failed: {error}"));
        assert!(matches!(replay.initial.info.settings.map_style, 6 | 9));
        let world_sim = WorldSim::from_replay(&replay);
        let setup = world_sim.initial_setup.as_ref().unwrap_or_else(|| {
            panic!("{name}: setup refused: {:?}", world_sim.initial_setup_error)
        });
        let mut world = setup.sim.map.world.clone();
        for constructor in &setup.receipt.cities {
            let position = constructor.snapped_position;
            let center = (
                TCoord::from_coord(Coord(position.0)).0,
                TCoord::from_coord(Coord(position.1)).0,
            );
            let radius = constructor.constructor.world_fix.radius_tiles;
            let receipt = subject::apply_starting_village_city_mask(
                &mut world,
                subject::WallMaskCityRequest {
                    center_tcoord: center,
                    radius_tiles: radius,
                    on: 1,
                    owner: constructor.owner,
                    object_id: constructor.build.object_id,
                    native_call_flags: subject::FRESH_NATIVE_MASK_CITY_FLAGS,
                    native_call_city: -1,
                    post_activation_flags: subject::FRESH_STARTING_CENTER_FLAGS,
                },
            )
            .unwrap_or_else(|error| {
                panic!(
                    "{name} owner {}: CITY mask refused: {error}",
                    constructor.owner
                )
            });
            let expected = match radius {
                20 => 1232,
                24 => 1788,
                other => panic!("{name}: unexpected starting radius {other}"),
            };
            assert_eq!(receipt.table_endpoint, expected);
            assert_eq!(receipt.in_bounds_offsets as usize, expected);
            assert_eq!(receipt.out_of_bounds_offsets, 0);
            assert_ne!(world.tmask(center.0, center.1) & tflag::CITY, 0);
            assert!(!receipt.first_checksum_city_image_ready);
        }
    }
    if repo_root().join("ron-data/replays/multi").exists() {
        assert_eq!(fixtures, 3);
    }
}

#[test]
fn capstone_and_pdb_schedule_addresses_are_frozen() {
    assert_eq!(subject::SETUP_BUILD_CITIES_VA, 0x005a_b910);
    assert_eq!(subject::SETUP_BUILD_INIT_CALL_VA, 0x005a_ba15);
    assert_eq!(subject::SETUP_BUILD_ACTIVATE_CALL_VA, 0x005a_babf);
    assert_eq!(subject::BUILD_ACTIVATE_VA, 0x0062_3e20);
    assert_eq!(subject::WALL_ACTIVATE_CALL_VA, 0x0062_3f65);
    assert_eq!(subject::WALL_START_MASK_CALL_VA, 0x0063_e86e);
    assert_eq!(subject::WALL_MASK_ME_VA, 0x0064_2fc0);
    assert_eq!(subject::BUILD_TYPE_MASK_ME_VA, 0x0063_12a0);
    assert_eq!(subject::BUILD_TYPE_MASK_CITY_CALL_VA, 0x0063_17c9);
    assert_eq!(subject::WALL_MASK_CITY_VA, 0x0063_e310);
    assert_eq!(subject::WALL_MASK_CITY_OR_VA, 0x0063_e36a);
    assert_eq!(subject::EVEN_CIRCLE_INIT_VA, 0x0068_16d0);
    assert_eq!(subject::OBJECT_UPDATE_SEEN_SETUP_CALL_VA, 0x005a_bae8);
    assert_eq!(subject::OBJECT_UPDATE_SEEN_ALLY_SETUP_CALL_VA, 0x005a_bb04);
    assert_eq!(subject::PER_CITY_COMPUTE_ALL_TERRITORY_CALL_VA, 0x005a_bb25);
    assert_eq!(subject::WORLD_ANALYZE_MAP_SETUP_CALL_VA, 0x005a_d5f4);
    assert_eq!(
        subject::FINAL_COMPUTE_ALL_TERRITORY_SETUP_CALL_VA,
        0x005a_d603
    );
    assert_eq!(subject::WORLD_COMPUTE_ALL_TERRITORY_VA, 0x006b_5700);
    assert_eq!(subject::WORLD_COMPUTE_REG_TERRITORY_VA, 0x006b_0bb0);
    assert_eq!(subject::GAME_DO_FRAME_VA, 0x0059_1ef0);
    assert_eq!(subject::LEADERS_STRATEGY_ALL_CALL_VA, 0x0059_2448);
    assert_eq!(subject::GAME_DAEMON_PROCESS_ALL_CALL_VA, 0x0059_244d);
    assert_eq!(subject::LEADER_PLAN_STRATEGY_CALL_VA, 0x006e_d452);
    assert!(subject::LEADERS_STRATEGY_ALL_CALL_VA < subject::GAME_DAEMON_PROCESS_ALL_CALL_VA);
}
