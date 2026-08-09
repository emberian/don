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
        "map_post_continent_regions",
        "continent execution error: {:?}",
        sim.initial_item_error
    );
    let continent = sim
        .initial_continent
        .as_ref()
        .expect("Mediterranean must execute through its first make_region call");
    assert_eq!(continent.map_style, 12);
    assert_eq!(continent.direct_rng_sites.len(), 20_728);
    assert_eq!(continent.rng_final as u32, 0x3e25_9e29);
    assert_eq!(continent.region_seeds.len(), 2);
    assert_eq!(continent.region_growths.len(), 2);
    assert_eq!(continent.pool_eliminations.len(), 3);
    assert!(continent.player_land.is_some());
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
    assert_eq!(continent.map_style, 18);
    assert_eq!(continent.starts_added, 6);
    assert_eq!(continent.region_seeds.len(), 6);
    assert_eq!(continent.region_growths.len(), 12);
    assert_eq!(continent.direct_rng_sites.len(), 8_517);
    assert_eq!(continent.rng_final as u32, 0x950f_a373);
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
