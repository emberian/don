// SPDX-License-Identifier: GPL-3.0-or-later

use don_ai::arena::{Map, MapParams, Spatial};
use don_sim::systems::gather_terrain::{
    GatherTerrainMaterializationError, GatherTerrainSourceStamp, SUPPORTED_RULES_XML_SHA256,
};
use don_sim::systems::gathering::{AuthoritativeGatherTerrain, GatherTile};
use don_sim::systems::map_terrain::World;

const RULES_XML: &[u8] = include_bytes!("../../../ron-data/rules.xml");

fn retained_map() -> Map {
    let spatial = Spatial {
        city_center_radius: 20,
        city_center_pop_radius: 4,
        city_capture_radius: 10,
        woodcutter_radius: 8,
        mine_radius: 6,
    };
    let mut map = Map::generate(MapParams::default(), spatial);
    map.retain_gather_terrain_sources(
        RULES_XML.to_vec(),
        GatherTerrainSourceStamp {
            installed_rules_sha256: SUPPORTED_RULES_XML_SHA256,
            world_seed: map.seed as i32,
            coherent_generation: true,
        },
        Vec::new(),
        Vec::new(),
    )
    .unwrap();
    map
}

#[test]
fn retained_sources_construct_only_against_the_exact_world_identity() {
    let map = retained_map();
    let sources = map.gather_terrain_sources().unwrap();
    let diplomacy = |_: i32, _: i32| Some(false);
    let mut world = World::init_default_rules(map.w / 4, map.h / 4);
    world.seed = map.seed as i32;

    let host = sources.executable_host(&world, &diplomacy).unwrap();
    assert_eq!(host.world_cell_dimensions(), (map.w / 4, map.h / 4));
    assert_eq!(host.tile_dimensions(), (map.w, map.h));
    assert!(host.tile_mask(GatherTile { tx: 0, ty: 0 }).is_some());
    drop(host);

    world.seed ^= 1;
    assert!(matches!(
        sources.executable_host(&world, &diplomacy),
        Err(GatherTerrainMaterializationError::StaleWorldSeed { .. })
    ));
}

#[test]
fn supported_rules_identity_exposes_recovered_capacity_rules_not_a_site_table() {
    let map = retained_map();
    let rules = map.gather_terrain_sources().unwrap().capacity_rules();
    assert_eq!(rules.mountain_size_thresholds, [100, 210, 275, 400]);
    assert_eq!(rules.mountain_capacity_bases, [3, 5, 6, 8, 10]);
    assert_eq!(rules.french_woodies, 1);
}
