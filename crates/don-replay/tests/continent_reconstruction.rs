// SPDX-License-Identifier: GPL-3.0-or-later
//! Mutation-sensitive executable tests for the six replay-corpus continent dispatches.

use don_replay::continent::{
    execute_continent_prefix, execute_continent_prefix_with_regions,
    execute_team_continent_partition, ContinentError, ContinentStop,
    EAST_INDIES_NONPLAYER_ISLANDS_VA, MAP_FILL_CONT_EQUAL_SIZE_RNG_VA, REGIONS_CLEAR_ALL_VA,
};
use don_replay::initial::{InitialWorldgenInputs, ReplayByteSpan, WorldgenSourceSpans};
use don_replay::map_style::{
    MapStyleStaticData, StaticFileEvidence, StaticXmlEntry, SHIPPED_MAP_STYLE_CATALOG,
};
use don_sim::rng::Random;
use don_sim::systems::map_terrain::{land, World, WorldSection};
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
    if ordinal == 19 {
        selected_map_entries.push(entry("AVOID_CONTINENT", "scalevalue", "16 SCALE"));
        selected_map_entries.push(entry("AVOID_CENTER", "scalevalue", "6"));
        for tag in [
            "AVOID_EDGE_0",
            "AVOID_EDGE_1",
            "AVOID_EDGE_2",
            "AVOID_EDGE_3",
        ] {
            selected_map_entries.push(entry(tag, "scalevalue", "0"));
        }
    }
    if ordinal == 14 || ordinal == 18 {
        // Both shipped files override AVOID_CENTER with a plain 3 and all four
        // edge margins; `greatlakes.xml` scales its margins by the world axis.
        selected_map_entries.push(entry("AVOID_CENTER", "scalevalue", "3"));
        for tag in [
            "AVOID_EDGE_0",
            "AVOID_EDGE_1",
            "AVOID_EDGE_2",
            "AVOID_EDGE_3",
        ] {
            selected_map_entries.push(entry(
                tag,
                "scalevalue",
                if ordinal == 14 { "8 SCALE" } else { "2" },
            ));
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
                next_va: REGIONS_CLEAR_ALL_VA
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
    assert_eq!(med.region_growths.len(), 2);
    assert_eq!(med.rng_final, med.region_growths[1].rng_final);
    assert_eq!(
        med.stop,
        ContinentStop::HookComplete {
            next_va: REGIONS_CLEAR_ALL_VA
        }
    );
    let player_land = med
        .player_land
        .as_ref()
        .expect("Mediterranean executes check_player_land");
    assert_eq!(player_land.call.enabled, 1);
    assert_eq!(player_land.call.avoid_continent, 4);
    assert_eq!(player_land.call.radius, 6);
    assert_eq!(player_land.effective_radius, 6);
    assert_eq!(player_land.footprints_visited, 16);
    assert_eq!(player_land.target_searches, 16);
    assert_eq!(player_land.target_regions_found, 0);
    assert_eq!(player_land.seed_cells_written, 0);
    assert!(player_land.outer_cells_written > 0);
    assert_eq!(
        player_land.region_zero_appends,
        player_land.outer_cells_written
    );
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
    let second_growth = &med.region_growths[1];
    assert_eq!(
        (second_growth.call.region, second_growth.call.target_area),
        (32, 150)
    );
    assert!(second_growth.completed);
    assert_eq!(med.region_seeds.len(), 2);
    assert_eq!(
        (
            med.region_seeds[1].call.region,
            med.region_seeds[1].call.area
        ),
        (32, 150)
    );
    assert_eq!(med.pool_eliminations.len(), 3);
    assert_eq!(med.starts_added, 4);
    assert_eq!(med_world.start_x.items.len(), 4);
    assert_eq!(med_world.start_y.items.len(), 4);
    assert!(med_regions.list.iter().any(|region| region.size > 0));
    assert!(med_world
        .wdata
        .iter()
        .all(|cell| (0..128).contains(&i32::from(cell.region))));

    let mut lakes_world = seeded_world(100, seed);
    let mut lakes_regions = Regions::default();
    let lakes = execute_continent_prefix_with_regions(
        &inputs(14, 4, seed, 100, 5),
        &style(14),
        &mut lakes_world,
        &mut lakes_regions,
    )
    .unwrap();
    assert!(matches!(
        lakes.stop,
        ContinentStop::HookComplete {
            next_va: REGIONS_CLEAR_ALL_VA
        }
    ));
    // xs/12 == 8 lakes, plus zero or one from the parity draw.
    assert!(
        (8..=9).contains(&lakes.region_seeds.len()),
        "lakes seeded: {}",
        lakes.region_seeds.len()
    );
    assert_eq!(lakes.region_seeds.len(), lakes.region_growths.len());
    assert_eq!(lakes.lake_candidates.len(), lakes.grow_valid_calls.len());
    // Every accepted candidate cleared its own spacing threshold, which is what
    // `Map::land_dist` exists to decide.
    for candidate in &lakes.lake_candidates {
        assert!(
            candidate.land_distance >= candidate.required_distance,
            "{candidate:?}"
        );
        assert!(candidate.required_distance >= 3);
        assert!((0..100).contains(&candidate.x) && (0..100).contains(&candidate.y));
    }
    // The first pass uses the unshrunk area, so its threshold is fixed by
    // `max((8 + 1) / 2 + isqrt(500 * 7 / 22), 3)`.
    assert_eq!(lakes.lake_candidates[0].required_distance, 16);
    assert_eq!(lakes.lake_candidates[0].area, 500);
    // Regions become lakes: `Map::invert_land` runs after the growth loop, so
    // the seeded cells read as ocean in the returned world.
    for candidate in &lakes.lake_candidates {
        if candidate.grow_valid_return != 0 {
            assert!(
                lakes_world.is_ocean(candidate.x, candidate.y),
                "{candidate:?}"
            );
        }
    }
    assert_eq!(lakes.starts_added, 4);
    assert_eq!(lakes_world.start_x.items.len(), 4);
    for (&sx, &sy) in lakes_world
        .start_x
        .items
        .iter()
        .zip(&lakes_world.start_y.items)
    {
        assert!(!lakes_world.is_ocean(sx, sy));
        assert!(sx > 0 && sy > 0 && sx < 99 && sy < 99);
    }
    assert!(lakes.player_land.is_some());
    assert!(lakes.world_inverted);
    // Every direct hook draw is recorded, and the growth helpers appended
    // theirs, so the final RNG word cannot be reached by five draws any more.
    assert!(lakes.direct_rng_sites.len() > 6);
    assert_eq!(
        lakes.direct_rng_sites[0],
        don_replay::map_style::MAP_MAKE_ORIENTATION_RNG_VA
    );
    // `greatlakes.xml` sets all four AVOID_EDGE margins, so an accepted direct
    // `grow_valid` call draws exactly once per margin at `0x0069d2ab`, while a
    // rejection stops the margin walk early and draws fewer.
    for call in &lakes.grow_valid_calls {
        assert_eq!(call.primitive_va, don_replay::growth::MAP_GROW_VALID_VA);
        assert!(call.rng_sites.len() <= 4, "{call:?}");
        if call.retail_return != 0 {
            assert_eq!(call.rng_sites.len(), 4, "{call:?}");
        }
        assert!((0..=4).contains(&call.edge_jitter_final), "{call:?}");
    }
    assert!(lakes
        .grow_valid_calls
        .iter()
        .any(|call| call.retail_return != 0));

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
    assert!(indies.region_growths.len() > 12);
    assert_eq!(indies.starts_added, 6);
    assert!(matches!(
        indies.stop,
        ContinentStop::HookComplete {
            next_va: REGIONS_CLEAR_ALL_VA
        }
    ));
    let tail_start = indies
        .direct_rng_sites
        .iter()
        .position(|site| *site == EAST_INDIES_NONPLAYER_ISLANDS_VA)
        .expect("canonical receipt retains the tail's first direct draw");
    assert!(
        tail_start > 3,
        "player-region helper draws precede the tail"
    );
    assert!(indies.direct_rng_sites[tail_start..]
        .iter()
        .any(|site| *site == don_replay::continent::EAST_INDIES_ISLAND_X_RNG_VA));
    assert!(indies.direct_rng_sites[tail_start..]
        .iter()
        .any(|site| *site == don_replay::continent::EAST_INDIES_ISLAND_Y_RNG_VA));
    let tail = indies
        .east_indies_tail
        .as_ref()
        .expect("canonical receipt retains the complete East Indies tail");
    assert_eq!(tail.rng_final, indies.rng_final);
    assert_eq!(
        tail.rng_initial, indies.region_growths[11].rng_final,
        "the tail continues from the second player-growth pass without reseeding"
    );
    let tail_sites: Vec<_> = tail
        .rng_chronology
        .iter()
        .flat_map(|span| span.call_sites.iter().copied())
        .collect();
    assert_eq!(
        &indies.direct_rng_sites[tail_start..],
        tail_sites.as_slice()
    );

    let islands = indies.region_growths.len() - 12;
    assert_eq!(indies.region_seeds.len(), 6 + islands);
    assert!(indies.grow_valid_calls.len() >= islands);
    assert_eq!(tail.islands.len(), islands);
    assert_eq!(tail.grow_valid_calls, indies.grow_valid_calls);
    for (index, seed) in indies.region_seeds[..6].iter().enumerate() {
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
    for seed in &indies.region_seeds[6..] {
        let region = seed.call.region as usize;
        assert_eq!(
            (seed.common_factor, seed.goody_factor, seed.flags),
            (5, 5, 0)
        );
        assert_eq!(indies_regions.list[region].climate, 1);
        assert_eq!(
            indies_world.wdata(seed.call.x, seed.call.y).region,
            seed.call.region as i16
        );
    }
    assert_eq!(indies_regions.land as usize, indies.region_seeds.len());

    let mut eastwest_world = seeded_world(100, seed);
    let eastwest_before = eastwest_world.checksum_sections();
    let mut eastwest_regions = Regions::default();
    let eastwest = execute_continent_prefix_with_regions(
        &inputs(19, 4, seed, 100, 5),
        &style(19),
        &mut eastwest_world,
        &mut eastwest_regions,
    )
    .unwrap();
    assert_eq!(
        &eastwest.direct_rng_sites[..5],
        &[
            don_replay::map_style::MAP_MAKE_ORIENTATION_RNG_VA,
            MAP_FILL_CONT_EQUAL_SIZE_RNG_VA,
            MAP_FILL_CONT_EQUAL_SIZE_RNG_VA,
            don_replay::map_style::EAST_MEETS_WEST_DIRECT_RNG_SITES[0],
            don_replay::map_style::EAST_MEETS_WEST_DIRECT_RNG_SITES[1],
        ]
    );
    let partition = eastwest
        .team_partition
        .as_ref()
        .expect("East Meets West executes fill_cont");
    assert_eq!(partition.active_slots, [0, 1, 2, 3]);
    assert_eq!(partition.unused_teams, [2, 3, 4, 5, 6, 7]);
    assert_eq!(partition.continent_counts, [2, 2, 0, 0, 0, 0, 0, 0]);
    assert_eq!(partition.team_to_continent, [1, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(partition.continent_count, 2);
    assert_eq!(partition.random_state_before as u32, 0x7543_2777);
    assert_eq!(partition.random_state_after as u32, 0x25db_fac1);
    assert_eq!(partition.random_draws.len(), 2);
    assert_eq!(partition.random_draws[0].raw, 24_169);
    assert!(partition.random_draws[0].exchanged);
    assert_eq!(partition.random_draws[1].raw, 64_192);
    assert!(!partition.random_draws[1].exchanged);
    assert_eq!(partition.world_walked_bytes_changed, 0);
    assert_eq!(eastwest.region_seeds.len(), 2);
    assert_eq!(eastwest.region_growths.len(), 4);
    assert_eq!(
        eastwest
            .region_seeds
            .iter()
            .map(|seed| (seed.call.region, seed.call.area))
            .collect::<Vec<_>>(),
        [(1, 1_388), (2, 1_388)]
    );
    assert_eq!(
        eastwest
            .region_growths
            .iter()
            .map(|growth| (
                growth.call.region,
                growth.call.target_area,
                growth.call.max_distance,
            ))
            .collect::<Vec<_>>(),
        [
            (1, 1_388, 27),
            (2, 1_388, 27),
            (1, 2_776, 27),
            (2, 2_776, 27)
        ]
    );
    assert!(eastwest
        .region_growths
        .iter()
        .all(|growth| growth.completed));
    assert_eq!(eastwest.starts_added, 4);
    match &eastwest.stop {
        ContinentStop::AddStartingLocation {
            primitive_va,
            caller_va,
            next_va,
            centroids,
            edge_canals,
            call,
            selector,
            mutation,
            remaining,
            player_land,
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
            next_mutator_va,
        } => {
            assert_eq!(
                *primitive_va,
                don_replay::east_meets_west_place_start::WORLD_ADD_STARTING_LOCATION_VA
            );
            assert_eq!(
                *caller_va,
                don_replay::east_meets_west_place_start::EAST_MEETS_WEST_ADD_START_CALL_VA
            );
            assert_eq!(centroids.centroids.len(), 2);
            assert_eq!(centroids.centroid_x.len(), 2);
            assert_eq!(centroids.centroid_y.len(), 2);
            assert!(edge_canals.direct_rng_sites.is_empty());
            assert_eq!(edge_canals.canal_writes(), 2);
            assert_eq!(call.player_slot, 0);
            assert_eq!(call.continent_players, 2);
            assert_eq!(call.min_start_distance, 24);
            assert_eq!(call.random_draw, None);
            assert_eq!(
                selector.random_state_before,
                eastwest.region_growths.last().unwrap().rng_final
            );
            assert_eq!(selector.random_state_after, remaining.random_state_before);
            assert_eq!(remaining.random_state_after, eastwest.rng_final);
            assert_eq!(selector.draws.len(), 1);
            assert_eq!(selector.accepted_pass, Some(1));
            assert_eq!(
                *next_va,
                don_replay::post_continent::MAP_MAKE_PROGRESS_STRING_CONSTRUCTOR_CALL_VA
            );
            assert_eq!(
                *next_mutator_va,
                don_replay::post_continent::STRING_WIDE_CONSTRUCTOR_VA
            );
            assert_eq!(
                post_checksum_string_close.allocation.buffer_after,
                don_replay::continent::MapMakeStringAllocationState::ReturnedToRetailPool
            );
            assert_eq!(post_player_land_cleanup.centroid_y_length, 2);
            assert!(post_player_land_cleanup.centroid_y_list_non_null);
            assert_eq!(
                centroid_y_free_cleanup.free.allocation.elements,
                centroids.centroid_y
            );
            assert_eq!(centroid_y_free_cleanup.centroid_x_length, 2);
            assert!(centroid_y_free_cleanup.centroid_x_list_non_null);
            assert_eq!(
                centroid_x_free_cleanup.free.allocation.elements,
                centroids.centroid_x
            );
            assert!(centroid_x_free_cleanup.exception_registration_restored);
            assert_eq!(regions_clear_all.region_records_visited, 128);
            assert!(regions_clear_all
                .region_records
                .iter()
                .all(|record| record.after.size == 0));
            assert_eq!(regions_find_all.world_before, regions_clear_all.world_after);
            assert_eq!(regions_find_all.world_after, fix_diag_land.world_before);
            assert_eq!(regions_find_all.successful_find_calls, 3);
            assert_eq!(regions_find_all.build.non_input_pumps, 4);
            assert_eq!(territory_limits.stores.len(), 6);
            assert_eq!(territory_limits.world_before, regions_find_all.world_after);
            assert_eq!(territory_limits.world_after, fix_diag_land.world_before);
            assert!(territory_limits.world_sections_changed.is_empty());
            assert_eq!(
                fix_diag_land.body,
                don_replay::post_continent::MAP_FIX_DIAG_LAND_NATIVE_BODY
            );
            assert_eq!(
                fix_diag_land.world_after,
                eastwest_world.checksum_sections()
            );
            assert_eq!(
                post_fix_diag_string_constructor.constructor,
                don_replay::continent::STRING_CONSTRUCTOR_NATIVE_BODY
            );
            assert_eq!(post_fix_diag_string_constructor.local.source, "map.cpp");
            assert_eq!(
                game_log_say_checksum.body,
                don_replay::post_continent::GAME_LOG_SAY_CHECKSUM_NATIVE_BODY
            );
            assert_eq!(game_log_say_checksum.call.line_number, 0x1e9b);
            assert_eq!(game_log_say_checksum.call.mode, 1);
            assert_eq!(
                game_log_say_checksum.owner.source_owner,
                don_replay::continent::MapMakePostFixDiagStringAllocationOwner::CallerLocalMapCpp
            );
            assert_eq!(game_log_say_checksum.cleanup_guard_after, -1);
            assert_eq!(
                post_fix_diag_string_constructor.world_after,
                eastwest_world.checksum_sections()
            );
            assert_eq!(
                territory_limits
                    .stores
                    .iter()
                    .map(|store| store.source_value)
                    .collect::<Vec<_>>(),
                [44, 4, 4, 44, 4, 4]
            );
            assert_eq!(player_land.random_state_before, eastwest.rng_final);
            assert_eq!(player_land.random_state_after, eastwest.rng_final);
            assert!(player_land.direct_rng_sites.is_empty());
            assert_eq!(
                eastwest.player_land.as_ref(),
                Some(&player_land.body_receipt)
            );
            assert_eq!(remaining.entry_va, mutation.caller_return_va);
            assert_eq!(remaining.iterations.len(), 3);
            assert!(remaining
                .iterations
                .iter()
                .all(|iteration| iteration.mutation.is_some()));
            assert_eq!(mutation.returned_start_index, 0);
            assert_eq!(mutation.input, selector.output.unwrap());
            assert_eq!(mutation.arrays_after.start_x.length, 1);
            assert_eq!(mutation.arrays_after.start_city_x.length, 4);
            assert_eq!(
                edge_canals
                    .passes
                    .iter()
                    .flat_map(|pass| pass.writes.iter())
                    .map(|write| (write.side, write.middle, write.depth, write.x, write.y))
                    .collect::<Vec<_>>(),
                [
                    (
                        don_replay::edge_canals::EdgeCanalSide::Bottom,
                        61,
                        1,
                        61,
                        98
                    ),
                    (
                        don_replay::edge_canals::EdgeCanalSide::Bottom,
                        62,
                        2,
                        62,
                        97
                    ),
                ]
            );
        }
        other => panic!("East Meets West stopped at {other:?}"),
    }
    assert_eq!(eastwest.pool_eliminations.len(), 1);
    assert_eq!(
        eastwest.pool_eliminations[0].param,
        don_replay::pools::ElimPoolParam::EntireWorld
    );
    assert_eq!(
        eastwest_before.differing_sections(&eastwest_world.checksum_sections()),
        [WorldSection::StartArrays, WorldSection::WData]
    );
    assert_eq!(
        (
            eastwest_before.full,
            eastwest_world.checksum_sections().full,
            eastwest_before.section(WorldSection::WData).adler,
            eastwest_world
                .checksum_sections()
                .section(WorldSection::WData)
                .adler,
        ),
        (0xd293_cb35, 0x7a34_6ba0, 0x4f28_bf8c, 0xaf72_4f9f)
    );
    assert_eq!(eastwest_regions.land, 0);
}

#[test]
fn fill_cont_replays_both_checksum_bearing_east_meets_west_headers() {
    // These are the independently reconstructed states at the old fill_cont
    // boundary, not values derived from the recorded World checksum.
    let headers = [
        (
            "Playback___2018.12.01_18_33_16__Sat_.rcx",
            [Some(0), Some(0), Some(1), Some(1), None, None, None, None],
            0x13df_fc27u32,
            [(19_289, true), (41_712, false)],
            0x8e53_a2f1u32,
        ),
        (
            "Playback___2019.03.24_11_56_19__Sun_.rcx",
            [Some(0), Some(1), Some(1), Some(0), None, None, None, None],
            0xdcb6_8147u32,
            [(52_729, true), (1_296, false)],
            0x3ac9_0511u32,
        ),
    ];
    for (name, leaders, initial, expected_draws, final_state) in headers {
        let mut rng = Random::new(initial as i32);
        let receipt = execute_team_continent_partition(&leaders, &mut rng).unwrap();

        assert_eq!(receipt.active_slots, [0, 1, 2, 3], "{name}");
        assert_eq!(receipt.unused_teams, [2, 3, 4, 5, 6, 7], "{name}");
        assert_eq!(receipt.continent_counts, [2, 2, 0, 0, 0, 0, 0, 0], "{name}");
        assert_eq!(
            receipt.team_to_continent,
            [1, 0, 0, 0, 0, 0, 0, 0],
            "{name}"
        );
        assert_eq!(receipt.random_state_before as u32, initial, "{name}");
        assert_eq!(receipt.random_state_after as u32, final_state, "{name}");
        assert_eq!(rng.state(), receipt.random_state_after, "{name}");
        assert_eq!(
            receipt
                .random_draws
                .iter()
                .map(|draw| (draw.raw, draw.exchanged))
                .collect::<Vec<_>>(),
            expected_draws,
            "{name}"
        );
        assert_eq!(
            receipt
                .random_draws
                .iter()
                .map(|draw| (draw.left_continent, draw.right_continent))
                .collect::<Vec<_>>(),
            [(0, 1), (1, 0)],
            "{name}"
        );
        assert!(
            receipt
                .random_draws
                .iter()
                .all(|draw| draw.call_va == MAP_FILL_CONT_EQUAL_SIZE_RNG_VA),
            "{name}"
        );
        assert_eq!(receipt.world_walked_bytes_changed, 0, "{name}");
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
