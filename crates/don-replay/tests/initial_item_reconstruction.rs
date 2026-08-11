// SPDX-License-Identifier: GPL-3.0-or-later
//! Retail replay evidence for the initial goody reconstruction boundary.

use don_replay::checksum::Channel;
use don_replay::harness::{Simulation, WorldSim};
use don_replay::initial::{
    parse_initial_state, InitialItemBoundary, InitialItemReconstructionError,
    ABSENT_REPLAY_ITEM_INPUTS,
};
use don_replay::map_style::{
    ron_data_root_for_replay, MapStyleStaticData, EAST_INDIES_DIRECT_RNG_SITES,
    MAP_MAKE_ORIENTATION_RNG_VA, MAP_PLACE_RESOURCES_DIRECT_RNG_SITES,
    MEDITERRANEAN_DIRECT_RNG_SITES,
};
use don_replay::replay::{load_payload, Replay};
use don_sim::item_runtime::ItemRuntimeError;
use std::path::{Path, PathBuf};

fn supported_replay() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("ron-data/replays/multi/Playback___2025.02.10_21_26_50__Mon_.rcx")
}

fn open_supported() -> Option<Replay> {
    let path = supported_replay();
    if !path.is_file() {
        eprintln!(
            "\n  SKIPPED — NOT A PASS. The supported retail replay is absent; \
             initial-item reconstruction was not exercised.\n"
        );
        return None;
    }
    Some(Replay::open(&path).expect("supported retail replay must decode"))
}

fn open_east_indies() -> Option<Replay> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("ron-data/replays/multi/Playback___2024.04.10_17_05_19__Wed_.rcx");
    if !path.is_file() {
        eprintln!("\n  SKIPPED — NOT A PASS. East Indies retail replay is absent.\n");
        return None;
    }
    Some(Replay::open(&path).expect("East Indies retail replay must decode"))
}

#[test]
fn supported_replay_admits_mediterranean_content_and_reaches_the_generator_boundary() {
    let Some(rep) = open_supported() else {
        return;
    };
    let root = ron_data_root_for_replay(&rep.path).expect("replay is owned by ron-data");
    let style = MapStyleStaticData::load_from_ron_data(&root, rep.initial.info.settings.map_style)
        .expect("lawful local shipped map-style capture must parse");
    let plan = rep
        .initial
        .reconstruct_items_with_style(style)
        .expect("static style must match replay selector");
    assert_eq!(
        plan.boundary,
        InitialItemBoundary::MapContinentGenerationUnavailable {
            map_style: 12,
            make_continents_va: 0x0069_add0,
        }
    );
    let style = plan.style.as_ref().unwrap();
    assert_eq!(style.identity.key, "Mediterranean");
    assert_eq!(style.identity.filename, Some("mediterranean.xml"));
    assert_eq!(style.default_terrain_groups.len(), 11);
    assert_eq!(style.selected_terrain_groups.len(), 15);
    assert_eq!(style.default_goodies.len(), 5);
    assert_eq!(style.selected_goodies.len(), 6);
    assert_eq!(
        style.selected_terrain_groups[0].attribute("type"),
        Some("mountains")
    );
    assert_eq!(
        style.selected_terrain_groups[0].attribute("pattern"),
        Some("nonplayer")
    );
    assert_eq!(style.selected_goodies[0].attribute("numrare"), Some("3"));
    assert_eq!(
        style.selected_goodies[5].attribute("pattern"),
        Some("corner")
    );
    assert_eq!(
        style.make_continents_direct_rng_sites,
        MEDITERRANEAN_DIRECT_RNG_SITES
    );
    let sites = style.known_direct_rng_sites();
    assert_eq!(sites.first(), Some(&MAP_MAKE_ORIENTATION_RNG_VA));
    assert_eq!(sites.last(), MAP_PLACE_RESOURCES_DIRECT_RNG_SITES.last());
    assert_eq!(sites.len(), 8);
    let wrong_style = MapStyleStaticData::load_from_ron_data(&root, 9).unwrap();
    assert_eq!(wrong_style.identity.key, "Himalayas");
    assert_eq!(
        rep.initial.reconstruct_items_with_style(wrong_style),
        Err(InitialItemReconstructionError::StyleSelectorMismatch {
            replay_map_style: 12,
            static_map_style: 9,
        })
    );
    assert!(plan.rules.is_some(), "static replay Rules were admitted");
    assert_eq!(plan.scalar_source_bytes(), 10);
    assert_eq!(plan.absent_replay_inputs(), &ABSENT_REPLAY_ITEM_INPUTS);
    assert!(ABSENT_REPLAY_ITEM_INPUTS
        .iter()
        .all(|missing| missing.replay_bytes == 0));

    // Exact decompressed-payload evidence for this supported specimen. The
    // 24 bytes between the parsed Game prefix and the admitted static Rules
    // section cannot be a generated 100x100 map/item/RNG snapshot.
    let sources = plan.inputs.sources;
    assert_eq!((sources.seed.offset, sources.seed.bytes), (0x3e, 4));
    assert_eq!(
        (sources.map_style.offset, sources.map_style.bytes),
        (0x53, 1)
    );
    assert_eq!((sources.map_size.offset, sources.map_size.bytes), (0x54, 1));
    assert_eq!(rep.initial.bytes_walked, 0x392);
    assert_eq!(plan.rules.unwrap().serialized_offset, 0x3aa);
    assert_eq!(
        plan.rules.unwrap().serialized_offset - rep.initial.bytes_walked,
        24
    );

    let sim = WorldSim::from_replay(&rep);
    let executed = sim.initial_items.as_ref().unwrap();
    assert_eq!(executed.inputs, plan.inputs);
    assert_eq!(executed.style, plan.style);
    assert_eq!(
        executed.boundary.name(),
        // `ron-data/tilesets.xml` and `ron-data/effects_graphics.xml` are extracted, so the
        // generator now runs INTO `place_all` 0x006a70d0. Mediterranean's group 0 is
        // `pattern="nonplayer"`, so it enters `place_region_group` 0x006a2f60 and stops at
        // `Mountains::add_mountain` 0x0089c2e0 -- the first genuinely unported retail leaf.
        "place_all_mountains_add_mountain",
        "continent execution error: {:?}",
        sim.initial_item_error
    );
    let continent = sim
        .initial_continent
        .as_ref()
        .expect("Mediterranean must execute through its first make_region call");
    assert_eq!(continent.map_style, 12);
    let tile_selection = executed
        .tile_selection
        .as_ref()
        .expect("load_map_data must resolve its tileset before orientation");
    assert_eq!(tile_selection.passes.len(), 1);
    assert_eq!(
        continent.rng_initial,
        tile_selection.main_random_state_after
    );
    assert_ne!(continent.rng_initial, executed.inputs.seed as i32);
    // This used to pin a missing `ron-data/tilesets.xml`. The file is extracted now, so the
    // fertility stage reads it and runs instead of aborting, and the generator proceeds to
    // `TerrainGroups::place_all`. Asserting the absence of the read error is what keeps the
    // extraction from silently regressing back into a masked boundary.
    assert!(
        executed.fertility_error.is_none(),
        "tilesets.xml is extracted; fertility must not fail: {:?}",
        executed.fertility_error
    );
    assert!(executed.fertility.is_some());
    assert!(executed.fill_fertile.is_some());
    assert_eq!(continent.region_seeds.len(), 2);
    assert_eq!(continent.region_growths.len(), 2);
    assert_eq!(continent.pool_eliminations.len(), 3);
    assert!(continent.player_land.is_some());
    let post = executed
        .post_continent
        .as_ref()
        .expect("Mediterranean must execute the common coastline chain");
    assert_eq!(
        post.next_va,
        don_replay::post_continent::TERRAIN_GROUPS_FILL_FERTILE_VA
    );
    assert_eq!(
        post.second_clear_va,
        don_replay::continent::REGIONS_CLEAR_ALL_VA
    );
    assert_eq!(
        post.second_find_va,
        don_replay::continent::REGIONS_FIND_ALL_VA
    );
    assert!(post.coastline_cells_changed > 0);
    assert!(post.second_regions.land_components_found > 0);
    assert_eq!(continent.starts_added, 4);
    assert!(continent.region_growths[0].completed);
    assert_eq!(continent.region_growths[0].retail_return, 0);
    assert_eq!(sim.initial_item_style_error, None);
    assert_eq!(
        sim.initial_item_error,
        Some(InitialItemReconstructionError::Blocked(executed.boundary))
    );
    assert_eq!(
        sim.world.items_channel(),
        Err(ItemRuntimeError::Unavailable)
    );
    assert!(!sim.installed_channels()[Channel::Items as usize]);
}

#[test]
fn changing_the_source_style_byte_changes_the_plan_without_installing_items() {
    let Some(rep) = open_supported() else {
        return;
    };
    let mut payload = load_payload(&rep.path).unwrap();
    let at = rep.initial.worldgen_sources.map_style.offset;
    let original_style = payload[at];
    payload[at] = original_style ^ 1;

    let changed = parse_initial_state(&payload).expect("style selector mutation stays structural");
    let plan = changed.reconstruct_items();
    assert_eq!(plan.inputs.sources.map_style.offset, at);
    assert_eq!(plan.inputs.map_style, original_style ^ 1);
    assert_eq!(
        plan.boundary,
        InitialItemBoundary::MapStyleContentUnavailable {
            map_style: original_style ^ 1
        }
    );

    let mut map = changed.reconstruct_world().unwrap();
    let mut sim = don_sim::World::with_capacity(16, 1);
    assert_eq!(
        plan.apply(&mut sim, &mut map.world),
        Err(InitialItemReconstructionError::Blocked(plan.boundary))
    );
    assert_eq!(sim.items_channel(), Err(ItemRuntimeError::Unavailable));
}

fn open_great_lakes() -> Option<Replay> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("ron-data/replays/multi/Playback___2024.02.24_21_25_53__Sat_.rcx");
    if !path.is_file() {
        eprintln!("\n  SKIPPED — NOT A PASS. Great Lakes retail replay is absent.\n");
        return None;
    }
    Some(Replay::open(&path).expect("Great Lakes retail replay must decode"))
}

/// The second most common corpus style now runs its whole virtual.
///
/// Before `Map::land_dist` `0x0069d970` existed as a port, this replay stopped
/// inside `MapGreatLakes::make_continents` at the first spacing test, with no
/// lake, no start and an unwiped candidate. It now leaves the hook at
/// `0x0069a641` and joins Mediterranean at the common `TerrainGroups` boundary.
#[test]
fn checksum_bearing_great_lakes_replay_runs_its_whole_style_virtual() {
    let Some(rep) = open_great_lakes() else {
        return;
    };
    assert!(rep.checksum_packets > 0);
    assert_eq!(rep.initial.info.settings.map_style, 14);
    let root = ron_data_root_for_replay(&rep.path).unwrap();
    let style = MapStyleStaticData::load_from_ron_data(&root, 14).unwrap();
    assert_eq!(style.identity.key, "Great Lakes");
    assert_eq!(style.identity.make_continents_va, Some(0x0069_9e40));

    let plan = rep.initial.reconstruct_items_with_style(style).unwrap();
    assert_eq!(
        plan.boundary,
        InitialItemBoundary::MapContinentGenerationUnavailable {
            map_style: 14,
            make_continents_va: 0x0069_9e40,
        }
    );

    let sim = WorldSim::from_replay(&rep);
    assert_eq!(sim.initial_item_style_error, None);
    let executed = sim.initial_items.as_ref().unwrap();
    assert_eq!(
        executed.boundary.name(),
        // `ron-data/tilesets.xml` and `ron-data/effects_graphics.xml` are extracted, so the
        // generator now runs INTO `place_all` 0x006a70d0. Great Lakes' group 0 is
        // `type="trees" pattern="player"`, whose growth tail calls `World::set_oil_at`
        // 0x006b2a10; its Good-object create/close belongs to the unmodelled `goods`
        // channel, so the strict oil policy stops there.
        "place_all_world_set_oil_at",
        "continent execution error: {:?}",
        sim.initial_item_error
    );
    let continent = sim.initial_continent.as_ref().unwrap();
    assert_eq!(continent.map_style, 14);
    assert!(matches!(
        continent.stop,
        don_replay::ContinentStop::HookComplete { .. }
    ));
    // xs/12 lakes plus the parity draw, each seeded and grown once.
    let world = sim.initial_world.as_ref().expect("prefix world");
    assert_eq!(world.world.xs, 100);
    assert!((8..=9).contains(&continent.region_seeds.len()));
    assert_eq!(continent.region_seeds.len(), continent.region_growths.len());
    assert!(continent.lake_candidates.len() >= continent.region_seeds.len());
    for candidate in &continent.lake_candidates {
        assert!(candidate.land_distance >= candidate.required_distance);
    }
    assert_eq!(continent.starts_added, rep.initial.active_players().count());
    assert!(continent.world_inverted);
    assert_eq!(continent.pool_eliminations.len(), 1);
    assert!(continent.player_land.is_some());
    // `Map::grow_region`'s return is not tested by this style, so a stalled
    // growth must not turn into a retry stop.
    assert_eq!(continent.retry_attempt, 1);
    let post = executed
        .post_continent
        .as_ref()
        .expect("the common coastline chain runs once the hook returns");
    assert_eq!(post.next_va, don_replay::TERRAIN_GROUPS_FILL_FERTILE_VA);
}

#[test]
fn checksum_bearing_east_indies_replay_closes_the_last_corpus_style_hole() {
    let Some(rep) = open_east_indies() else {
        return;
    };
    assert!(rep.checksum_packets > 0);
    assert_eq!(rep.initial.info.settings.map_style, 18);
    let root = ron_data_root_for_replay(&rep.path).unwrap();
    let style = MapStyleStaticData::load_from_ron_data(&root, 18).unwrap();
    assert_eq!(style.identity.key, "East Indies");
    assert_eq!(style.identity.make_continents_va, Some(0x0069_7540));
    assert_eq!(style.selected_terrain_groups.len(), 11);
    assert_eq!(style.selected_goodies.len(), 2);
    assert_eq!(
        style.make_continents_direct_rng_sites,
        EAST_INDIES_DIRECT_RNG_SITES
    );
    assert_eq!(style.known_direct_rng_sites().len(), 8);

    let plan = rep.initial.reconstruct_items_with_style(style).unwrap();
    assert_eq!(
        plan.boundary,
        InitialItemBoundary::MapContinentGenerationUnavailable {
            map_style: 18,
            make_continents_va: 0x0069_7540,
        }
    );
    let sim = WorldSim::from_replay(&rep);
    assert_eq!(sim.initial_item_style_error, None);
    assert_eq!(
        sim.initial_items.as_ref().unwrap().boundary.name(),
        "map_east_indies_nonplayer_islands",
        "continent execution error: {:?}",
        sim.initial_item_error
    );
    let continent = sim.initial_continent.as_ref().unwrap();
    let tile_selection = sim
        .initial_items
        .as_ref()
        .unwrap()
        .tile_selection
        .as_ref()
        .unwrap();
    assert_eq!(tile_selection.passes.len(), 2);
    assert_eq!(
        continent.rng_initial,
        tile_selection.main_random_state_after
    );
    assert_ne!(continent.rng_initial, rep.initial.info.seed as i32);
    assert_eq!(continent.map_style, 18);
    assert_eq!(continent.starts_added, 6);
    assert_eq!(continent.region_seeds.len(), 6);
    assert_eq!(continent.region_growths.len(), 12);
    assert!(continent
        .region_growths
        .iter()
        .all(|growth| growth.completed && growth.retail_return == 0));
    assert_ne!(sim.initial_items.as_ref().unwrap().boundary, plan.boundary);
    assert_eq!(
        sim.world.items_channel(),
        Err(ItemRuntimeError::Unavailable)
    );
}
