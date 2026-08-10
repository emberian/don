// SPDX-License-Identifier: GPL-3.0-or-later
//! Static catalog resolution and typed live-fact admission.

use don_replay::fractal_boundary::{FertilityBoundary, RetailFractalPlane};
use don_replay::initial::{
    InitialItemBoundary, InitialItemReconstruction, InitialWorld, InitialWorldgenInputs,
    ReplayByteSpan, WorldgenSourceSpans,
};
use don_replay::map_style::{
    MapStyleStaticData, StaticFileEvidence, StaticXmlEntry, SHIPPED_EXE_SHA256,
    SHIPPED_MAP_STYLE_CATALOG,
};
use don_replay::place_all_boundary::{ReplayPlaceAllHostFacts, ReplayPlaceAllTDataFacts};
use don_replay::place_all_facts::{
    resolve_place_all_prerequisites, resolve_terrain_group_catalog, CapturedHelpingFacts,
    PlaceAllFactKind, PlaceAllLiveCaptureEvidence, PlaceAllLiveFacts, TerrainGroupCatalogError,
    TERRAIN_GROUPS_REPORTING_VA, WORLD_GLOBAL_VA,
};
use don_replay::TERRAIN_GROUPS_PLACE_ALL_VA;
use don_sim::systems::map_terrain::World;
use don_sim::systems::mountains::Mountains;
use don_sim::systems::regions::Regions;
use don_sim::systems::terrain_doobers::DooberTilesetRules;
use don_sim::systems::terrain_groups::FillFertileReceipt;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn evidence(name: &str) -> StaticFileEvidence {
    StaticFileEvidence {
        path: PathBuf::from(name),
        bytes: 123,
        adler32: 0x1234_5678,
    }
}

fn group_row() -> StaticXmlEntry {
    StaticXmlEntry {
        tag: "GROUPENTRY".to_owned(),
        attributes: BTreeMap::from([
            ("type".to_owned(), "trees".to_owned()),
            ("chance".to_owned(), "100".to_owned()),
            ("grouping".to_owned(), "7".to_owned()),
            ("min_clumps".to_owned(), "7 SCALE".to_owned()),
            ("max_clumps".to_owned(), "3 AREA".to_owned()),
            ("pattern".to_owned(), "world".to_owned()),
            ("min_size".to_owned(), "5 minsize".to_owned()),
            ("max_size".to_owned(), "9".to_owned()),
            ("city_keep_away".to_owned(), "8 SCALE".to_owned()),
            ("city_stay_near".to_owned(), "80 maxdist".to_owned()),
            ("mount_space".to_owned(), "70 mntspace".to_owned()),
            ("forest_space".to_owned(), "-3 forestspace".to_owned()),
            ("rock_space".to_owned(), "1 rockspacee".to_owned()),
            ("coast_space".to_owned(), "2 coastspace".to_owned()),
            ("cent_keep_away".to_owned(), "4 mindist".to_owned()),
            ("cent_stay_near".to_owned(), "70 maxdist".to_owned()),
            ("edge_keep_away".to_owned(), "3 mindist".to_owned()),
            ("edge_stay_near".to_owned(), "4 maxdist".to_owned()),
            ("corner_keep_away".to_owned(), "2 mindist".to_owned()),
            ("corner_stay_near".to_owned(), "5 maxdist".to_owned()),
        ]),
    }
}

fn style(row: StaticXmlEntry) -> MapStyleStaticData {
    MapStyleStaticData {
        identity: SHIPPED_MAP_STYLE_CATALOG[6],
        catalog_source: evidence("rules.xml"),
        default_source: evidence("mapstyles/default.xml"),
        selected_source: evidence("mapstyles/oldworld.xml"),
        default_map_entries: Vec::new(),
        selected_map_entries: Vec::new(),
        selected_map_section_present: true,
        default_terrain_groups: Vec::new(),
        selected_terrain_groups: vec![row],
        selected_terrain_groups_section_present: true,
        default_goodies: Vec::new(),
        selected_goodies: Vec::new(),
        selected_goodies_section_present: false,
        make_continents_direct_rng_sites: Vec::new(),
    }
}

fn span() -> ReplayByteSpan {
    ReplayByteSpan {
        offset: 0,
        bytes: 1,
    }
}

fn inputs(edge: i32) -> InitialWorldgenInputs {
    InitialWorldgenInputs {
        seed: 17,
        map_style: 6,
        map_size: 0,
        map_edge_world_cells: Some(edge),
        players: 2,
        game_rules: 0,
        starting_town: 0,
        scenario_type: 0,
        active_slots: vec![0, 1],
        active_who: vec![0, 1],
        active_teams: vec![0, 1],
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

fn plan(edge: i32) -> InitialItemReconstruction {
    let columns = vec![vec![0; edge as usize + 1]; edge as usize + 1];
    InitialItemReconstruction {
        inputs: inputs(edge),
        rules: None,
        style: Some(style(group_row())),
        post_continent: None,
        tile_selection: None,
        fertility: Some(FertilityBoundary {
            map_style: 6,
            tileset: "Dirty".to_owned(),
            tile_chances: Vec::new(),
            tile_selection_draw: None,
            tile_selection_bucket: 0,
            main_random_state_after_tileset: 17,
            tile_selection_passes: Vec::new(),
            baseland_frequencies: vec![100],
            partitions: Vec::new(),
            clump_factor: 1,
            plane: RetailFractalPlane {
                xs: edge,
                ys: edge,
                smooth: 1,
                seed: 17,
                columns,
                random_draws: 0,
                random_state_after: 17,
            },
        }),
        fertility_error: None,
        fill_fertile: Some(FillFertileReceipt::default()),
        boundary: InitialItemBoundary::MapTerrainGroupsPlaceAllUnavailable {
            next_va: TERRAIN_GROUPS_PLACE_ALL_VA,
        },
    }
}

fn map(edge: i32) -> InitialWorld {
    let world = World::init_default_rules(edge, edge);
    let checksum = world.checksum_sections();
    InitialWorld {
        world,
        generation_regions: Regions::default(),
        checksum,
        ownership: None,
        sourced_walked_bytes: 0,
    }
}

fn full_capture(map: &InitialWorld) -> PlaceAllLiveFacts {
    PlaceAllLiveFacts {
        evidence: PlaceAllLiveCaptureEvidence {
            executable_sha256: SHIPPED_EXE_SHA256.to_owned(),
            entry_va: TERRAIN_GROUPS_PLACE_ALL_VA,
            reporting_va: TERRAIN_GROUPS_REPORTING_VA,
            map_style: 6,
            tileset: "Dirty".to_owned(),
        },
        subtype_freqs: Some([vec![1, 2], vec![3], vec![4, 5]]),
        console_info: Some(0),
        mountains: Some(Mountains::default()),
        tdata: Some(ReplayPlaceAllTDataFacts {
            tile_xs: map.world.tile_xs,
            tile_ys: map.world.tile_ys,
            tile_size: map.world.tile_size,
            cells: map.world.tdata.clone(),
        }),
        doober_rules: Some(DooberTilesetRules::default()),
        progress: Some(1),
        helping: Some(CapturedHelpingFacts::Disabled),
        reporting_scores: Some([[0; 5]; 8]),
        host: Some(ReplayPlaceAllHostFacts::default()),
    }
}

#[test]
fn shipped_expression_domain_resolves_every_native_group_scalar() {
    let style = style(group_row());
    let small = World::init_default_rules(35, 35);
    let receipt = resolve_terrain_group_catalog(&style, &small).unwrap();
    let group = &receipt.groups[0];

    assert!(receipt.selected_section);
    assert_eq!(receipt.source.path, PathBuf::from("mapstyles/oldworld.xml"));
    assert_eq!(group.group_type, 4);
    assert_eq!(group.pattern, 2);
    assert_eq!(group.min_clumps, 4); // axis SCALE, rounded by standard edge 70
    assert_eq!(group.max_clumps, 1); // AREA, clamped to native minimum one
    assert_eq!(group.min_size, 5); // annotation suffix is ignored by retail
    assert_eq!(group.start_min, 4);
    assert_eq!(group.start_max, 64);
    assert_eq!(group.forest_space, 0);
    assert_eq!(group.mount_space, 64);
    assert_eq!(group.rock_space, 1);
    assert_eq!(group.cent_max, 64);
    assert_eq!(group.touching_mountain, -1);

    let standard = World::init_default_rules(70, 70);
    let mutated = resolve_terrain_group_catalog(&style, &standard).unwrap();
    assert_eq!(mutated.groups[0].min_clumps, 7);
    assert_eq!(mutated.groups[0].max_clumps, 3);
}

#[test]
fn unknown_expression_suffix_fails_closed_instead_of_becoming_a_literal() {
    let mut row = group_row();
    row.attributes
        .insert("min_clumps".to_owned(), "7 guessedscale".to_owned());
    let error =
        resolve_terrain_group_catalog(&style(row), &World::init_default_rules(35, 35)).unwrap_err();
    assert_eq!(
        error,
        TerrainGroupCatalogError::UnsupportedExpressionSuffix {
            row: 0,
            attribute: "min_clumps",
            value: "7 guessedscale".to_owned(),
            suffix: "guessedscale".to_owned(),
        }
    );
}

#[test]
fn absent_live_capture_preserves_exact_unavailable_sources_and_addresses() {
    let plan = plan(8);
    let map = map(8);
    let resolution = resolve_place_all_prerequisites(&plan, &map, None).unwrap();

    assert!(resolution.ready.is_none());
    assert_eq!(resolution.catalog.groups.len(), 1);
    assert_eq!(
        resolution.available,
        vec![
            PlaceAllFactKind::TerrainGroupCatalog,
            PlaceAllFactKind::FertilityPlaneAndPartitions,
            PlaceAllFactKind::PlayerCountAndPlacePlayers,
        ]
    );
    assert_eq!(resolution.unavailable.len(), 9);
    let tdata = resolution
        .unavailable
        .iter()
        .find(|fact| fact.kind == PlaceAllFactKind::TDataPlane)
        .unwrap();
    assert!(tdata.addresses.contains(&WORLD_GLOBAL_VA));
    assert!(tdata.addresses.contains(&TERRAIN_GROUPS_PLACE_ALL_VA));
}

#[test]
fn complete_shipped_build_capture_materializes_bridge_inputs_without_defaults() {
    let plan = plan(8);
    let map = map(8);
    let capture = full_capture(&map);
    let resolution = resolve_place_all_prerequisites(&plan, &map, Some(&capture)).unwrap();

    assert!(resolution.unavailable.is_empty());
    let prepared = resolution.ready.unwrap();
    assert_eq!(prepared.runtime.terrain_groups.groups.len(), 1);
    assert_eq!(
        prepared.runtime.terrain_groups.subtype_freqs,
        [vec![1, 2], vec![3], vec![4, 5]]
    );
    assert_eq!(prepared.runtime.terrain_groups.fractal.columns.len(), 9);
    assert_eq!(prepared.facts.players.progress, 1);
    assert_eq!(prepared.facts.players.place_players, 1);
    assert_eq!(prepared.facts.players.reporting.num_players, 2);
    assert!(prepared.facts.players.helping.is_none());
    assert!(prepared.facts.host.group_inputs.is_empty());
}
