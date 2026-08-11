// SPDX-License-Identifier: GPL-3.0-or-later
//! Self-contained fixture for load-map-data through `fill_fertile` integration.

use don_replay::continent::ContinentReceipt;
use don_replay::fractal_boundary::{
    FractalBoundaryError, TERRAIN_GROUPS_FILL_FERTILE_VA, TERRAIN_GROUPS_PLACE_ALL_VA,
};
use don_replay::initial::{
    InitialItemBoundary, InitialItemReconstruction, InitialWorld, InitialWorldgenInputs,
    ReplayByteSpan, WorldgenSourceSpans,
};
use don_replay::map_style::{
    MapStyleStaticData, StaticFileEvidence, StaticXmlEntry, SHIPPED_MAP_STYLE_CATALOG,
};
use don_sim::systems::map_terrain::{land, World};
use don_sim::systems::regions::Regions;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

const DEFAULT_XML: &str = r#"
<ROOT>
  <MAP>
    <TILESET>
      <TILECHANCE type="Dirty" chance="18"/>
      <TILECHANCE type="Snowy" chance="82"/>
    </TILESET>
  </MAP>
  <TILESET_DATA>
    <DIRTY>
      <LANDKEY name="baseland" frequency_0="45" frequency_1="25" frequency_2="20" frequency_3="10"/>
    </DIRTY>
    <SNOWY>
      <LANDKEY name="baseland" frequency_0="50" frequency_1="0" frequency_2="0" frequency_3="50"/>
    </SNOWY>
  </TILESET_DATA>
</ROOT>
"#;

const SELECTED_XML: &str = "<ROOT><MAP><SEA_MAP value=\"0\"/></MAP></ROOT>";

const TILESETS_XML: &str = r#"
<TILESETS>
  <TILESET name="Dirty">
    <TERRAINGROUP>
      <CLUMP_FACTOR value="2"/>
      <FOREST_BASE value="21"/>
      <FOREST_GROWTH_PROB value="22"/>
      <MOUNTAIN_BASE value="23"/>
      <MOUNTAIN_GROWTH_PROB value="24"/>
      <MOUNTAIN_FRINGE_TREE_PROB value="25"/>
      <COAST_BASE value="26"/>
      <COAST_GROWTH_PROB value="27"/>
      <BUSH_CLUMP_PROB value="28"/>
      <BUSH_SPACING value="29"/>
      <BUSH_MIN value="30"/>
      <BUSH_MAX value="31"/>
      <MOUNTAIN_ROCK_CLUMP_PROB value="32"/>
      <MOUNTAIN_ROCK_SPACING value="33"/>
      <MOUNTAIN_ROCK_MIN value="34"/>
      <MOUNTAIN_ROCK_MAX value="35"/>
    </TERRAINGROUP>
    <BASELAND><BASE/><BASE/><BASE/><BASE/></BASELAND>
  </TILESET>
  <TILESET name="Snowy">
    <TERRAINGROUP>
      <CLUMP_FACTOR value="3"/>
      <FOREST_BASE value="41"/>
      <FOREST_GROWTH_PROB value="42"/>
      <MOUNTAIN_BASE value="43"/>
      <MOUNTAIN_GROWTH_PROB value="44"/>
      <MOUNTAIN_FRINGE_TREE_PROB value="45"/>
      <COAST_BASE value="46"/>
      <COAST_GROWTH_PROB value="47"/>
      <BUSH_CLUMP_PROB value="48"/>
      <BUSH_SPACING value="49"/>
      <BUSH_MIN value="50"/>
      <BUSH_MAX value="51"/>
      <MOUNTAIN_ROCK_CLUMP_PROB value="52"/>
      <MOUNTAIN_ROCK_SPACING value="53"/>
      <MOUNTAIN_ROCK_MIN value="54"/>
      <MOUNTAIN_ROCK_MAX value="55"/>
    </TERRAINGROUP>
    <BASELAND><BASE/><BASE/><BASE/><BASE/></BASELAND>
  </TILESET>
</TILESETS>
"#;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = loop {
            let id = FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
            let candidate = std::env::temp_dir().join(format!(
                "don-fractal-integration-{}-{id}",
                std::process::id()
            ));
            match fs::create_dir(&candidate) {
                Ok(()) => break candidate,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("cannot create fixture directory: {error}"),
            }
        };
        fs::write(root.join("default.xml"), DEFAULT_XML).unwrap();
        fs::write(root.join("oldworld.xml"), SELECTED_XML).unwrap();
        fs::write(root.join("tilesets.xml"), TILESETS_XML).unwrap();
        Self { root }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn span() -> ReplayByteSpan {
    ReplayByteSpan {
        offset: 0,
        bytes: 1,
    }
}

fn inputs() -> InitialWorldgenInputs {
    InitialWorldgenInputs {
        seed: 108,
        map_style: 6,
        map_size: 0,
        map_edge_world_cells: Some(16),
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

fn entry(tag: &str, attribute: &str, value: &str) -> StaticXmlEntry {
    StaticXmlEntry {
        tag: tag.to_owned(),
        attributes: BTreeMap::from([(attribute.to_owned(), value.to_owned())]),
    }
}

fn evidence(path: PathBuf) -> StaticFileEvidence {
    StaticFileEvidence {
        path,
        bytes: 1,
        adler32: 1,
    }
}

fn style(fixture: &Fixture) -> MapStyleStaticData {
    MapStyleStaticData {
        identity: SHIPPED_MAP_STYLE_CATALOG[6],
        catalog_source: evidence(fixture.path("rules.xml")),
        default_source: evidence(fixture.path("default.xml")),
        selected_source: evidence(fixture.path("oldworld.xml")),
        default_map_entries: vec![entry("BASE_EDGE", "value", "-1")],
        selected_map_entries: Vec::new(),
        selected_map_section_present: true,
        default_terrain_groups: Vec::new(),
        selected_terrain_groups: Vec::new(),
        selected_terrain_groups_section_present: false,
        default_goodies: Vec::new(),
        selected_goodies: Vec::new(),
        selected_goodies_section_present: false,
        make_continents_direct_rng_sites: Vec::new(),
    }
}

fn initial_world() -> InitialWorld {
    let mut world = World::init_default_rules(16, 16);
    assert_eq!(world.seed_map_generation(108), Some(108));
    let checksum = world.checksum_sections();
    InitialWorld {
        world,
        generation_regions: Regions::default(),
        checksum,
        ownership: None,
        sourced_walked_bytes: 0,
    }
}

fn plan(fixture: &Fixture) -> InitialItemReconstruction {
    InitialItemReconstruction {
        inputs: inputs(),
        rules: None,
        style: Some(style(fixture)),
        post_continent: None,
        tile_selection: None,
        fertility: None,
        fertility_error: None,
        fill_fertile: None,
        place_all_advance: None,
        place_all_advance_error: None,
        mountain_range_error: None,
        boundary: InitialItemBoundary::MapContinentGenerationUnavailable {
            map_style: 6,
            make_continents_va: SHIPPED_MAP_STYLE_CATALOG[6].make_continents_va.unwrap(),
        },
    }
}

fn execute(fixture: &Fixture) -> (InitialItemReconstruction, InitialWorld, ContinentReceipt) {
    let mut plan = plan(fixture);
    let mut world = initial_world();
    let receipt = plan
        .advance_continent_prefix_with_tilesets(&mut world, &fixture.path("tilesets.xml"))
        .unwrap();
    (plan, world, receipt)
}

#[test]
fn admitted_install_data_reaches_place_all_with_correct_rng_handoff_and_land_subtypes() {
    let fixture = Fixture::new();
    let (plan, world, continent) = execute(&fixture);

    let selection = plan.tile_selection.as_ref().unwrap();
    assert_eq!(selection.draw, Some(218));
    assert_eq!(selection.passes.len(), 1);
    assert_eq!(selection.bucket, 18);
    assert_eq!(selection.tileset, "Dirty");
    assert_eq!(selection.main_random_state_after, 1_193_672_923);
    assert_eq!(continent.rng_initial, selection.main_random_state_after);
    assert_ne!(continent.rng_initial, plan.inputs.seed as i32);

    let post = plan.post_continent.as_ref().unwrap();
    assert_eq!(post.next_va, TERRAIN_GROUPS_FILL_FERTILE_VA);
    // fill_fertile now runs and the plan enters `TerrainGroups::place_all`
    // 0x006a70d0 itself. This fixture's tilesets path has no sibling
    // effects_graphics.xml, so the survey stops at the first call in the body,
    // Mountains::randomize_mountains 0x0089ca70, and names it.
    assert_eq!(
        plan.boundary,
        InitialItemBoundary::MapTerrainGroupsPlaceAllPrimitiveUnavailable {
            boundary: "place_all_mountain_range_lists",
            place_all_va: TERRAIN_GROUPS_PLACE_ALL_VA,
            primitive_va: don_replay::MOUNTAINS_RANDOMIZE_MOUNTAINS_VA,
            group_index: None,
            completed_groups: 0,
        }
    );
    assert!(plan.mountain_range_error.is_some());
    assert!(plan.fertility_error.is_none());

    let fertility = plan.fertility.as_ref().unwrap();
    let fill = plan.fill_fertile.unwrap();
    let fertile_after = world
        .world
        .wdata
        .iter()
        .filter(|cell| cell.land == land::FERTILE)
        .count() as i32;
    assert_eq!(fill.fertile_cells, fertile_after);
    assert!(fill.fertile_cells > 0);

    for x in 0..world.world.xs {
        for y in 0..world.world.ys {
            let cell = world.world.wdata(x, y);
            if cell.land != land::FERTILE {
                continue;
            }
            let byte = fertility.plane.columns[x as usize][y as usize];
            let expected = fertility
                .partitions
                .iter()
                .take_while(|partition| byte >= **partition)
                .count() as u8;
            assert_eq!(cell.land_sub, expected, "wrong bucket at ({x}, {y})");
        }
    }
}

#[test]
fn missing_installed_source_preserves_the_exact_fill_fertile_boundary() {
    let fixture = Fixture::new();
    let mut plan = plan(&fixture);
    let mut world = initial_world();
    let receipt = plan.advance_continent_prefix(&mut world).unwrap();

    assert_eq!(
        receipt.rng_initial,
        plan.tile_selection
            .as_ref()
            .unwrap()
            .main_random_state_after
    );
    assert_eq!(
        plan.boundary,
        InitialItemBoundary::MapTerrainGroupsUnavailable {
            next_va: TERRAIN_GROUPS_FILL_FERTILE_VA,
        }
    );
    assert_eq!(
        plan.fertility_error,
        Some(FractalBoundaryError::MissingInstalledTilesetsSource)
    );
    assert!(plan.fertility.is_none());
    assert!(plan.fill_fertile.is_none());
}

#[test]
fn malformed_installed_source_fails_closed_after_independent_continent_work() {
    let fixture = Fixture::new();
    let malformed = fixture.path("malformed-tilesets.xml");
    fs::write(&malformed, "<TILESETS><TILESET").unwrap();
    let mut plan = plan(&fixture);
    let mut world = initial_world();
    plan.advance_continent_prefix_with_tilesets(&mut world, &malformed)
        .unwrap();

    assert!(matches!(
        plan.fertility_error.as_ref(),
        Some(FractalBoundaryError::MalformedXml { .. })
            | Some(FractalBoundaryError::TilesetNotInstalled { .. })
    ));
    assert_eq!(plan.boundary.name(), "terrain_groups_fill_fertile");
    assert!(plan.fill_fertile.is_none());
}
