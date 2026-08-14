use don_replay::build_spawn_runtime::{spawn_canonical_build, CanonicalBuildSpawnRequest};
use don_replay::harness::WorldSim;
use don_replay::replay::Replay;
use don_sim::systems::map_terrain::{land, tflag, wflag, World};
use don_sim::systems::production::{self, BuildData};
use don_sim::systems::regions::Regions;
use don_sim::systems::tech_cities::{self, CityRecord};
use don_sim::tick::Sim;
use std::path::{Path, PathBuf};

#[path = "../src/starting_village_suffix.rs"]
mod subject;

const OWNER: u8 = 0;
const POSITION: (i32, i32) = (0x1260, 0x1ce0);

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn all_land_world() -> (World, Regions) {
    let mut world = World::init(32, 32, 44, 4, 4);
    for w in &mut world.wdata {
        w.flags = 0;
        w.land = land::FERTILE;
        w.region = 1;
        w.who = -1;
        w.blocked = 0;
    }
    let mut regions = Regions::default();
    regions.list[1].size = world.size;
    (world, regions)
}

fn fresh_sim_and_city() -> (Sim, CityRecord) {
    let mut sim = Sim::new(7, 64);
    sim.activate(usize::from(OWNER));
    let spawn = spawn_canonical_build(
        &mut sim,
        CanonicalBuildSpawnRequest {
            owner: OWNER,
            type_index: tech_cities::ty::VILLAGE,
            snapped_x: POSITION.0,
            snapped_y: POSITION.1,
            build: BuildData {
                flags: subject::FRESH_CENTER_FLAGS,
                orig_type: tech_cities::ty::VILLAGE,
                city: -1,
                city_down: -1,
                wonder: -1,
                dock: -1,
                attack_ox: -1,
                attack_whom: -1,
                ..BuildData::default()
            },
        },
    )
    .expect("fresh center spawn");
    sim.builds[spawn.row].city = 0;

    let city = CityRecord {
        city_flags: subject::FRESH_CAPITAL_CITY_FLAGS,
        city: 0,
        o: spawn.object_id,
        reg: 1,
        x: POSITION.0,
        y: POSITION.1,
        conquest_node: -1,
        pop: 1,
        who: OWNER as i8,
        race: OWNER as i8,
        founder: OWNER as i8,
        land: 9,
        filled: 1,
        ..CityRecord::default()
    };
    (sim, city)
}

#[test]
fn one_center_path_has_no_city_build_world_leader_or_rng_suffix() {
    let (sim, city) = fresh_sim_and_city();
    let city_before = city.pod_bytes();

    let receipt = subject::attest_fresh_starting_village_suffix(
        &sim,
        &city,
        subject::FreshVillageUpgradeFacts {
            town_type_available: true,
        },
    )
    .expect("source-bounded fresh suffix");

    assert_eq!(receipt.scanned_build_rows, 1);
    assert_eq!(receipt.rejected_city_center_rows, 1);
    assert_eq!(receipt.add_to_city_calls, 0);
    assert_eq!(receipt.city_pod_writes, 0);
    assert_eq!(receipt.build_city_link_writes, 0);
    assert_eq!(receipt.distinct_active_kinds, 1);
    assert_eq!(receipt.required_distinct_kinds, 6);
    assert_eq!(receipt.can_upgrade_calls, 1);
    assert!(!receipt.check_upgrade_body_entered);
    assert_eq!(receipt.regen_members_walked, 0);
    assert_eq!(receipt.regen_build_mask_writes, 0);
    assert_eq!((receipt.leader_writes, receipt.world_writes), (0, 0));
    assert_eq!(receipt.main_rng_draws, 0);
    assert!(receipt.city_suffix_complete);
    assert!(!receipt.first_checksum_city_image_ready);
    assert!(!receipt.constructor_transaction_ready());
    assert_eq!(city.pod_bytes(), city_before);
}

#[test]
fn unavailable_town_short_circuits_before_can_upgrade_but_is_still_inert() {
    let (sim, city) = fresh_sim_and_city();
    let receipt = subject::attest_fresh_starting_village_suffix(
        &sim,
        &city,
        subject::FreshVillageUpgradeFacts {
            town_type_available: false,
        },
    )
    .expect("unavailable Town is inert");

    assert_eq!(receipt.type_avail_calls, 1);
    assert_eq!(receipt.can_upgrade_calls, 0);
    assert!(!receipt.check_upgrade_body_entered);
}

#[test]
fn a_non_center_build_refuses_the_empty_scan_receipt() {
    let (mut sim, city) = fresh_sim_and_city();
    let spawn = spawn_canonical_build(
        &mut sim,
        CanonicalBuildSpawnRequest {
            owner: OWNER,
            type_index: tech_cities::ty::FARM,
            snapped_x: POSITION.0 + 0x300,
            snapped_y: POSITION.1,
            build: BuildData {
                flags: production::flag::VALID
                    | production::flag::STARTED
                    | production::flag::ACTIVE,
                orig_type: tech_cities::ty::FARM,
                city: -1,
                city_down: -1,
                ..BuildData::default()
            },
        },
    )
    .expect("non-center spawn");

    let err = subject::attest_fresh_starting_village_suffix(
        &sim,
        &city,
        subject::FreshVillageUpgradeFacts {
            town_type_available: true,
        },
    )
    .expect_err("Farm reaches the general find_buildings fork");
    assert_eq!(
        err,
        subject::FreshVillageSuffixError::ScannedNonCenterBuild {
            row: spawn.row,
            owner: OWNER,
            object_id: spawn.object_id,
            flags: 0x07,
        }
    );
}

#[test]
fn a_nonempty_city_down_chain_refuses_the_regen_receipt() {
    let (mut sim, city) = fresh_sim_and_city();
    sim.builds[0].city_down = 2001;

    let err = subject::attest_fresh_starting_village_suffix(
        &sim,
        &city,
        subject::FreshVillageUpgradeFacts {
            town_type_available: true,
        },
    )
    .expect_err("regen_roads would visit a member");
    assert_eq!(
        err,
        subject::FreshVillageSuffixError::CenterChainNotEmpty { city_down: 2001 }
    );
}

#[test]
fn source_addresses_and_shipped_threshold_are_frozen() {
    assert_eq!(subject::CITY_FIND_BUILDINGS_VA, 0x0073_84c0);
    assert_eq!(subject::CITY_READY_TO_UPGRADE_VA, 0x0073_6480);
    assert_eq!(subject::CITY_CAN_UPGRADE_VA, 0x0073_83c0);
    assert_eq!(subject::CITY_ENOUGH_KINDS_VA, 0x0073_6540);
    assert_eq!(subject::CITY_CHECK_UPGRADE_VA, 0x0073_8b20);
    assert_eq!(subject::CITY_REGEN_ROADS_VA, 0x0073_8aa0);
    assert_eq!(subject::BUILD_ADD_TO_CITY_VA, 0x0062_2380);
    assert_eq!(subject::LEADER_PLAN_STRATEGY_VA, 0x006b_9620);
    assert_eq!(subject::SHIPPED_CITY_BUILDINGS, 5);
    assert_eq!(subject::TOWN_REQUIRED_DISTINCT_KINDS, 6);
    assert_eq!(subject::LEADER_CITY_TERRAIN_CENSUS_VA, 0x006b_b400);
    assert_eq!(subject::WORLD_CHECK_BUILDING_WCOORD_VA, 0x006b_26e0);
    assert_eq!(subject::WORLD_SPACE_AT_CORNER_VA, 0x006b_27f0);
    assert_eq!(subject::WORLD_GATHER_AT_VA, 0x006b_07f0);
    assert_eq!(subject::REGION_NUM_COASTS_VA, 0x0068_0760);
    assert_eq!(subject::BUILD_TYPE_IS_DOCK_TILE_VA, 0x0063_6700);
    assert_eq!(subject::LEADER_HAS_TRIBE_BONUS_VA, 0x006e_1370);
    assert_eq!(subject::CITY_COUNT_GATHER_SLOTS_VA, 0x0073_7dc0);
}

#[test]
fn region_num_coasts_walks_only_sea_identities_65_through_126() {
    let mut regions = Regions::default();
    let land = &mut regions.list[1];
    land.coast.bytes[0] |= 1 << 0; // sea 64: excluded
    land.coast.bytes[0] |= 1 << 1; // sea 65: included
    land.coast.bytes[7] |= 1 << 6; // sea 126: included
    land.coast.bytes[7] |= 1 << 7; // sea 127: excluded
    assert_eq!(subject::retail_region_num_coasts(land), 2);
    assert_eq!(subject::retail_region_num_coasts(&regions.list[65]), 1);
    assert_eq!(subject::retail_region_num_coasts(&regions.list[127]), 0);
}

#[test]
fn village_census_skips_table_zero_and_recomputes_the_fourteen_bytes() {
    let (sim, mut city) = fresh_sim_and_city();
    let (world, regions) = all_land_world();
    city.ocean = 11;
    city.ocean_filled = 12;
    city.dock_tile = 13;
    city.space = [14, 15, 16];
    city.ter = [17, 18, 19, 20, 21, 22];
    city.bordering = 23;
    city.was_capital_flags = 24;

    let receipt = subject::apply_fresh_starting_village_terrain_census(
        &sim,
        &mut city,
        &world,
        &regions,
        subject::FreshVillageTerrainCensusFacts {
            upgrade: subject::FreshVillageUpgradeFacts {
                town_type_available: true,
            },
            indian_radius_bonus: false,
            gather: &subject::CityTerrainGatherFacts::default(),
        },
    )
    .expect("all-land census needs no content-backed gather row");

    assert_eq!(receipt.radius_tiles, 20);
    assert_eq!(receipt.circle_index, 5);
    assert_eq!(
        (receipt.inner_table_end, receipt.outer_table_end),
        (105, 145)
    );
    assert_eq!(
        (receipt.first_table_index, receipt.last_table_index),
        (1, 144)
    );
    assert_eq!(receipt.table_entries_scanned, 144);
    assert_eq!(receipt.in_bounds_entries, 144);
    assert_eq!(receipt.inner_entries, 104);
    assert_eq!(receipt.inner_region_matches, 104);
    assert_eq!(receipt.placement_calls, 104);
    assert_eq!(receipt.placement_grades, [104, 0, 0, 0, 0]);
    assert_eq!(receipt.gather_at_calls, 0);
    assert_eq!(receipt.water_entries, 0);
    assert_eq!(receipt.bytes_after.ocean, 0);
    assert_eq!(receipt.bytes_after.land, 104);
    assert_eq!(receipt.bytes_after.filled, 104);
    assert_eq!(receipt.bytes_after.ocean_filled, 0);
    assert_eq!(receipt.bytes_after.dock_tile, 0);
    assert_eq!(receipt.bytes_after.space, [0; 3]);
    assert_eq!(receipt.bytes_after.ter, [0; 6]);
    assert_eq!((city.bordering, city.was_capital_flags), (23, 24));
    assert_eq!(receipt.leader.gather_slots_for_region, 0);
    assert_eq!(receipt.leader.flag_2_city_count, 0);
    assert_eq!(receipt.leader.low_open_land_city_count, 1);
    assert_eq!(receipt.leader.open_land_for_region, 0);
    assert_eq!(
        (receipt.world_writes, receipt.build_or_registry_writes),
        (0, 0)
    );
    assert_eq!(receipt.main_rng_draws, 0);
    assert!(receipt.city_terrain_census_complete);
    assert!(!receipt.first_checksum_city_image_ready);
}

#[test]
fn fully_clear_city_tiles_take_grade_four_space_and_content_gather_path() {
    let (sim, mut city) = fresh_sim_and_city();
    let (mut world, regions) = all_land_world();
    world.tdata.fill(tflag::CITY);
    let mut gather = subject::CityTerrainGatherFacts::default();
    for wy in 0..world.ys {
        for wx in 0..world.xs {
            gather.by_wcoord.insert((wx, wy), [5, 300, -1, 7, 0, 260]);
        }
    }

    let receipt = subject::apply_fresh_starting_village_terrain_census(
        &sim,
        &mut city,
        &world,
        &regions,
        subject::FreshVillageTerrainCensusFacts {
            upgrade: subject::FreshVillageUpgradeFacts {
                town_type_available: true,
            },
            indian_radius_bonus: false,
            gather: &gather,
        },
    )
    .expect("every reached gather call has content facts");

    assert_eq!(receipt.placement_grades, [0, 0, 0, 0, 104]);
    assert_eq!(receipt.gather_at_calls, 104);
    assert_eq!(receipt.bytes_after.land, 104);
    assert_eq!(receipt.bytes_after.filled, 0);
    assert_eq!(receipt.bytes_after.space, [104; 3]);
    assert_eq!(receipt.bytes_after.ter, [5, 44, 0, 7, 0, 4]);
    assert_eq!(receipt.leader.open_land_for_region, 104);
    assert_eq!(receipt.leader.low_open_land_city_count, 0);
}

#[test]
fn a_missing_reached_gather_fact_refuses_without_partial_city_writes() {
    let (sim, mut city) = fresh_sim_and_city();
    let (mut world, regions) = all_land_world();
    world.tdata.fill(tflag::CITY);
    let before = city.clone();

    let err = subject::apply_fresh_starting_village_terrain_census(
        &sim,
        &mut city,
        &world,
        &regions,
        subject::FreshVillageTerrainCensusFacts {
            upgrade: subject::FreshVillageUpgradeFacts {
                town_type_available: true,
            },
            indian_radius_bonus: false,
            gather: &subject::CityTerrainGatherFacts::default(),
        },
    )
    .expect_err("content-backed gather result is mandatory when reached");
    assert!(matches!(
        err,
        subject::FreshVillageTerrainCensusError::MissingGatherAtFact { .. }
    ));
    assert_eq!(city, before);
}

#[test]
fn open_water_uses_the_outer_arm_and_never_enters_inner_land() {
    let (sim, mut city) = fresh_sim_and_city();
    let (mut world, mut regions) = all_land_world();
    for w in &mut world.wdata {
        w.land = land::OCEAN;
        w.region = 64;
    }
    regions.list[64].size = world.size;

    let receipt = subject::apply_fresh_starting_village_terrain_census(
        &sim,
        &mut city,
        &world,
        &regions,
        subject::FreshVillageTerrainCensusFacts {
            upgrade: subject::FreshVillageUpgradeFacts {
                town_type_available: true,
            },
            indian_radius_bonus: false,
            gather: &subject::CityTerrainGatherFacts::default(),
        },
    )
    .expect("open water does not call gather_at");

    assert_eq!(receipt.water_entries, 144);
    assert_eq!(receipt.inner_entries, 0);
    assert_eq!(receipt.placement_calls, 0);
    assert_eq!(receipt.gather_at_calls, 0);
    assert_eq!(receipt.dock_tile_calls, 144);
    assert_eq!(receipt.dock_tile_successes, 0);
    assert_eq!(receipt.bytes_after.ocean, 144);
    assert_eq!(receipt.bytes_after.land, 0);
    assert_eq!(receipt.bytes_after.filled, 0);
}

#[test]
fn waterhalf_skips_ocean_but_retains_the_inner_land_region_path() {
    let (sim, mut city) = fresh_sim_and_city();
    let (mut world, regions) = all_land_world();
    for w in &mut world.wdata {
        w.land = land::OCEAN;
        w.flags = wflag::WATERHALF;
    }

    let receipt = subject::apply_fresh_starting_village_terrain_census(
        &sim,
        &mut city,
        &world,
        &regions,
        subject::FreshVillageTerrainCensusFacts {
            upgrade: subject::FreshVillageUpgradeFacts {
                town_type_available: true,
            },
            indian_radius_bonus: false,
            gather: &subject::CityTerrainGatherFacts::default(),
        },
    )
    .expect("WATERHALF is admitted to the inner placement path");

    assert_eq!(receipt.water_entries, 0);
    assert_eq!(receipt.waterhalf_entries, 144);
    assert_eq!(receipt.inner_entries, 104);
    assert_eq!(receipt.bytes_after.ocean, 0);
    assert_eq!(receipt.bytes_after.land, 104);
    assert_eq!(receipt.bytes_after.filled, 104);
}

#[test]
fn real_style_6_and_9_worlds_reach_the_missing_content_fact_boundary() {
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
        let initial_world = world_sim.initial_world.as_ref().expect("generated world");
        let setup = world_sim.initial_setup.as_ref().unwrap_or_else(|| {
            panic!("{name}: setup refused: {:?}", world_sim.initial_setup_error)
        });
        for constructor in &setup.receipt.cities {
            let owner = usize::from(constructor.owner);
            let slot = constructor.city_slot as usize;
            let city = &setup.cities.slots[owner][slot];
            let mut staged = city.clone();
            let error = subject::apply_fresh_starting_village_terrain_census(
                &setup.sim,
                &mut staged,
                &setup.sim.map.world,
                &initial_world.generation_regions,
                subject::FreshVillageTerrainCensusFacts {
                    upgrade: subject::FreshVillageUpgradeFacts {
                        town_type_available: true,
                    },
                    indian_radius_bonus: constructor.constructor.world_fix.radius_tiles > 20,
                    gather: &subject::CityTerrainGatherFacts::default(),
                },
            )
            .expect_err("the generated LandData/Good gather facts are not yet owned");
            assert!(matches!(
                error,
                subject::FreshVillageTerrainCensusError::MissingGatherAtFact { .. }
            ));
            assert_eq!(
                staged, *city,
                "{name} owner {owner}: a missing terrain-content fact must not publish City bytes"
            );
        }
    }
    // The checkout containing the canonical corpus must exercise all three source fixtures;
    // clean packaging environments may intentionally omit `ron-data/replays`.
    if repo_root().join("ron-data/replays/multi").exists() {
        assert_eq!(fixtures, 3);
    }
}
