// SPDX-License-Identifier: GPL-3.0-or-later
//! Mutation-sensitive executable tests for the six replay-corpus continent dispatches.

use don_replay::continent::{
    execute_continent_prefix, execute_continent_prefix_with_regions, ContinentError, ContinentStop,
    EAST_INDIES_NONPLAYER_ISLANDS_VA, MAP_CHECK_PLAYER_LAND_VA, MAP_FILL_CONT_VA, MAP_LAND_DIST_VA,
    REGIONS_FIND_ALL_VA,
};
use don_replay::initial::{InitialWorldgenInputs, ReplayByteSpan, WorldgenSourceSpans};
use don_replay::map_style::{
    MapStyleStaticData, StaticFileEvidence, StaticXmlEntry, SHIPPED_MAP_STYLE_CATALOG,
};
use don_sim::systems::map_terrain::{land, World};
use don_sim::systems::naval::naval_roster;
use don_sim::systems::regions::Regions;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn span() -> ReplayByteSpan {
    ReplayByteSpan {
        offset: 0,
        bytes: 1,
    }
}

fn inputs(style: u8, players: u8, seed: u32, edge: i32, map_size: u8) -> InitialWorldgenInputs {
    InitialWorldgenInputs {
        seed,
        map_style: style,
        map_size,
        map_edge_world_cells: Some(edge),
        players,
        game_rules: 0,
        starting_town: 0,
        scenario_type: 0,
        active_slots: (0..players).collect(),
        active_who: (0..players).collect(),
        active_teams: (0..players).map(|p| p & 1).collect(),
        sources: WorldgenSourceSpans {
            seed: ReplayByteSpan {
                offset: 0,
                bytes: 4,
            },
            map_style: span(),
            map_size: span(),
            players: span(),
            game_rules: span(),
            starting_town: span(),
            scenario_type: span(),
            player_flags: [span(); 8],
            player_bodies: [None; 8],
        },
    }
}

fn entry(tag: &str, attribute: &str, value: &str) -> StaticXmlEntry {
    StaticXmlEntry {
        tag: tag.to_owned(),
        attributes: BTreeMap::from([(attribute.to_owned(), value.to_owned())]),
    }
}

fn style(ordinal: u8) -> MapStyleStaticData {
    let mut selected_map_entries = Vec::new();
    if ordinal == 14 || ordinal == 18 {
        selected_map_entries.push(entry(
            "AVOID_CONTINENT",
            "scalevalue",
            if ordinal == 14 { "8" } else { "4" },
        ));
    }
    if ordinal == 18 {
        selected_map_entries.push(entry("AVOID_CENTER", "scalevalue", "3"));
        for tag in [
            "AVOID_EDGE_0",
            "AVOID_EDGE_1",
            "AVOID_EDGE_2",
            "AVOID_EDGE_3",
        ] {
            selected_map_entries.push(entry(tag, "scalevalue", "2"));
        }
    }
    let evidence = || StaticFileEvidence {
        path: PathBuf::from("fixture.xml"),
        bytes: 1,
        adler32: 1,
    };
    MapStyleStaticData {
        identity: SHIPPED_MAP_STYLE_CATALOG[ordinal as usize],
        catalog_source: evidence(),
        default_source: evidence(),
        selected_source: evidence(),
        default_map_entries: vec![
            entry("BASE_EDGE", "value", "-1"),
            entry("COMMON_RESOURCES", "value", "4"),
            entry("GOODY_BOXES", "value", "8"),
            entry("AVOID_CENTER", "scalevalue", "4 SCALE"),
            entry("AVOID_CONTINENT", "scalevalue", "4"),
            entry("AVOID_EDGE_0", "scalevalue", "0"),
            entry("AVOID_EDGE_1", "scalevalue", "0"),
            entry("AVOID_EDGE_2", "scalevalue", "0"),
            entry("AVOID_EDGE_3", "scalevalue", "0"),
        ],
        selected_map_entries,
        selected_map_section_present: true,
        default_terrain_groups: Vec::new(),
        selected_terrain_groups: Vec::new(),
        selected_terrain_groups_section_present: true,
        default_goodies: Vec::new(),
        selected_goodies: Vec::new(),
        selected_goodies_section_present: true,
        make_continents_direct_rng_sites: Vec::new(),
    }
}

fn seeded_world(edge: i32, seed: u32) -> World {
    let mut world = World::init_default_rules(edge, edge);
    assert_eq!(world.seed_map_generation(seed as i32), Some(seed as i32));
    world
}

fn lcg_after(mut state: i32, draws: usize) -> i32 {
    for _ in 0..draws {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    }
    state
}

#[test]
fn old_world_and_himalayas_finish_the_identical_hook_and_mutate_the_map() {
    for ordinal in [6, 9] {
        let seed = 0x0030_1a65;
        let input = inputs(ordinal, 2, seed, 70, 3);
        let mut world = seeded_world(70, seed);
        let receipt = execute_continent_prefix(&input, &style(ordinal), &mut world).unwrap();

        assert_eq!(receipt.direct_rng_sites.len(), 5); // orientation + four hook draws
        assert_eq!(receipt.rng_final, lcg_after(seed as i32, 5));
        assert!(receipt.world_wiped && receipt.world_inverted);
        assert_eq!(receipt.regions_cleared, 2);
        assert_eq!(receipt.starts_added, 2);
        assert_eq!(receipt.start_min, Some(4));
        assert_eq!(
            receipt.stop,
            ContinentStop::HookComplete {
                next_va: REGIONS_FIND_ALL_VA
            }
        );
        assert_eq!(world.start_x.items.len(), 2);
        assert_eq!(world.start_y.items.len(), 2);
        assert_eq!(world.start_city_x.items.len(), 8);
        assert!(world
            .wdata
            .iter()
            .all(|cell| cell.land == land::FERTILE && cell.region == 0 && cell.region2 == 0));
    }
}

#[test]
fn four_complex_styles_reach_distinct_concrete_calls_without_skipping_draws() {
    let seed = 0x1234_5678;

    let mut med_world = seeded_world(70, seed);
    let mut med_regions = Regions::default();
    let med = execute_continent_prefix_with_regions(
        &inputs(12, 4, seed, 70, 3),
        &style(12),
        &mut med_world,
        &mut med_regions,
    )
    .unwrap();
    assert!(med.direct_rng_sites.len() > 5);
    assert_eq!(med.region_growths.len(), 1);
    assert_eq!(med.rng_final, med.region_growths[0].rng_final);
    match med.stop {
        ContinentStop::CheckPlayerLand {
            primitive_va,
            first,
            avoid_continent,
            radius,
            unit_type_index,
            unit_type_field_offset,
            source_max_range_tiles,
        } => {
            assert_eq!(primitive_va, MAP_CHECK_PLAYER_LAND_VA);
            assert_eq!(first, 1);
            assert_eq!(avoid_continent, 4);
            assert_eq!(radius, 6);
            assert_eq!(unit_type_index, 349);
            assert_eq!(unit_type_field_offset, 0x1fc);
            assert_eq!(source_max_range_tiles, 24);
            assert_eq!(
                naval_roster(unit_type_index as i32)
                    .expect("Battleship static row")
                    .max_range_tiles,
                source_max_range_tiles
            );
        }
        other => panic!("Mediterranean stopped at {other:?}"),
    }
    let med_growth = &med.region_growths[0];
    assert_eq!(
        (med_growth.call.region, med_growth.call.target_area),
        (1, 1_587)
    );
    assert_eq!(med_growth.call.max_distance, 23);
    assert!(med_growth.completed && med_growth.points_added > 1_500);
    let med_seed = &med.region_seeds[0];
    assert_eq!((med_seed.call.region, med_seed.call.area), (1, 1_587));
    assert_eq!(
        (
            med_seed.coord_capacity_before,
            med_seed.coord_capacity_after
        ),
        (0, 4)
    );
    assert_eq!(
        (
            med_seed.common_factor,
            med_seed.goody_factor,
            med_seed.flags
        ),
        (4, 8, 0)
    );
    assert_eq!(med.pool_eliminations.len(), 2);
    assert_eq!(med.starts_added, 4);
    assert_eq!(med_world.start_x.items.len(), 4);
    assert_eq!(med_world.start_y.items.len(), 4);
    assert!(med_regions.list.iter().all(|region| region.size == 0));
    let med_cell = med_world.wdata(med_seed.call.x, med_seed.call.y);
    assert_eq!(
        (med_cell.land, med_cell.land_sub, med_cell.region),
        (land::OCEAN, 0, 0)
    );
    assert!(med_world.wdata.iter().all(|cell| cell.region == 0));

    let mut lakes_world = seeded_world(100, seed);
    let lakes =
        execute_continent_prefix(&inputs(14, 4, seed, 100, 5), &style(14), &mut lakes_world)
            .unwrap();
    assert_eq!(lakes.direct_rng_sites.len(), 6); // orientation + five hook draws
    assert_eq!(lakes.rng_final, lcg_after(seed as i32, 6));
    match lakes.stop {
        ContinentStop::LandDistance { primitive_va, call } => {
            assert_eq!(primitive_va, MAP_LAND_DIST_VA);
            assert_eq!(call.region, 1);
            assert!(call.x >= 0 && call.x < 100 && call.y >= 0 && call.y < 100);
            assert_eq!(call.required_distance, 16);
        }
        other => panic!("Great Lakes stopped at {other:?}"),
    }

    let mut indies_world = seeded_world(90, seed);
    let mut indies_regions = Regions::default();
    let indies = execute_continent_prefix_with_regions(
        &inputs(18, 6, seed, 90, 1),
        &style(18),
        &mut indies_world,
        &mut indies_regions,
    )
    .unwrap();
    assert!(indies.direct_rng_sites.len() > 3);
    assert_eq!(indies.region_growths.len(), 12);
    assert_eq!(
        indies.rng_final,
        indies.region_growths.last().unwrap().rng_final
    );
    assert_eq!(indies.starts_added, 6);
    assert_eq!(indies.region_seeds.len(), 6);
    match indies.stop {
        ContinentStop::EastIndiesNonplayerIslands { next_rng_va } => {
            assert_eq!(next_rng_va, EAST_INDIES_NONPLAYER_ISLANDS_VA);
        }
        other => panic!("East Indies stopped at {other:?}"),
    }
    for (index, seed) in indies.region_seeds.iter().enumerate() {
        let region = index + 1;
        assert_eq!((seed.call.region, seed.call.area), (region as i32, 125));
        assert_eq!(
            (seed.common_factor, seed.goody_factor, seed.flags),
            (8, 8, 2)
        );
        assert!(indies_regions.list[region].size >= 125);
        assert_eq!(
            indies_regions.list[region].coords.items.len(),
            indies_regions.list[region].size as usize
        );
        assert_eq!(indies_world.start_x.items[index], seed.call.x);
        assert_eq!(indies_world.start_y.items[index], seed.call.y);
        assert_eq!(
            indies_world.wdata(seed.call.x, seed.call.y).region,
            region as i16
        );
    }
    assert_eq!(indies_regions.land, 6);

    let mut eastwest_world = seeded_world(100, seed);
    let eastwest = execute_continent_prefix(
        &inputs(19, 4, seed, 100, 5),
        &style(19),
        &mut eastwest_world,
    )
    .unwrap();
    assert_eq!(eastwest.direct_rng_sites.len(), 1); // orientation only; fill_cont precedes hook RNG
    assert_eq!(eastwest.rng_final, lcg_after(seed as i32, 1));
    match eastwest.stop {
        ContinentStop::FillCont {
            primitive_va,
            active_teams,
        } => {
            assert_eq!(primitive_va, MAP_FILL_CONT_VA);
            assert_eq!(active_teams, vec![0, 1, 0, 1]);
        }
        other => panic!("East Meets West stopped at {other:?}"),
    }
}

#[test]
fn fail_closed_validation_is_transactional_and_seed_mutation_changes_projection() {
    let seed = 7;
    let input = inputs(6, 2, seed, 70, 3);
    let mut wrong = seeded_world(70, seed + 1);
    let before = wrong.clone();
    assert!(matches!(
        execute_continent_prefix(&input, &style(6), &mut wrong),
        Err(ContinentError::MapPrefixMismatch { .. })
    ));
    assert_eq!(wrong.wdata, before.wdata);
    assert_eq!(wrong.start_x, before.start_x);

    let mut undersized = seeded_world(1, seed);
    let undersized_before = undersized.clone();
    let undersized_input = inputs(14, 2, seed, 1, 1);
    assert_eq!(
        execute_continent_prefix(&undersized_input, &style(14), &mut undersized),
        Err(ContinentError::InvalidMapDimensions { xs: 1, ys: 1 })
    );
    assert_eq!(undersized.wdata, undersized_before.wdata);
    assert_eq!(undersized.start_x, undersized_before.start_x);

    let mut missing_defaults = style(12);
    missing_defaults
        .default_map_entries
        .retain(|entry| entry.tag != "COMMON_RESOURCES");
    let mut unmodified_world = seeded_world(70, seed);
    let unmodified_world_before = unmodified_world.clone();
    let mut unmodified_regions = Regions::default();
    let unmodified_regions_before = unmodified_regions.clone();
    assert!(matches!(
        execute_continent_prefix_with_regions(
            &inputs(12, 2, seed, 70, 3),
            &missing_defaults,
            &mut unmodified_world,
            &mut unmodified_regions,
        ),
        Err(ContinentError::MissingMapParameter {
            tag: "COMMON_RESOURCES",
            attribute: "value"
        })
    ));
    assert_eq!(unmodified_world.wdata, unmodified_world_before.wdata);
    assert_eq!(unmodified_regions, unmodified_regions_before);

    let mut altered_defaults = style(12);
    altered_defaults
        .default_map_entries
        .iter_mut()
        .find(|entry| entry.tag == "COMMON_RESOURCES")
        .unwrap()
        .attributes
        .insert("value".into(), "5".into());
    let mut altered_world = seeded_world(70, seed);
    let mut altered_regions = Regions::default();
    let altered = execute_continent_prefix_with_regions(
        &inputs(12, 2, seed, 70, 3),
        &altered_defaults,
        &mut altered_world,
        &mut altered_regions,
    )
    .unwrap();
    assert_eq!(altered.region_seeds[0].common_factor, 5);
    assert_eq!(altered_regions.list[1].common_factor, 8);

    let mut corrupt_world = seeded_world(70, seed);
    let corrupt_world_before = corrupt_world.clone();
    let mut corrupt_regions = Regions::default();
    corrupt_regions.list[1].coords.capacity = -1;
    let corrupt_regions_before = corrupt_regions.clone();
    assert!(matches!(
        execute_continent_prefix_with_regions(
            &inputs(12, 2, seed, 70, 3),
            &style(12),
            &mut corrupt_world,
            &mut corrupt_regions,
        ),
        Err(ContinentError::InvalidRegionCoordStorage { region: 1, .. })
    ));
    assert_eq!(corrupt_world.wdata, corrupt_world_before.wdata);
    assert_eq!(corrupt_regions, corrupt_regions_before);

    let mut a = seeded_world(70, seed);
    let mut b = seeded_world(70, seed + 1);
    let ra = execute_continent_prefix(&input, &style(6), &mut a).unwrap();
    let changed = inputs(6, 2, seed + 1, 70, 3);
    let rb = execute_continent_prefix(&changed, &style(6), &mut b).unwrap();
    assert_ne!(ra.rng_final, rb.rng_final);
    assert_ne!(a.start_x.items, b.start_x.items);
}
