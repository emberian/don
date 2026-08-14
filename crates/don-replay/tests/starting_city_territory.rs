use std::path::{Path, PathBuf};

use don_replay::checksum::Channel;
use don_replay::cities_runtime::check_sim_cities;
use don_replay::harness::WorldSim;
use don_replay::replay::Replay;
use don_replay::starting_city_territory::{
    project_starting_city_territory, StartingCityTerritoryError, CITY_BORDERING_CLEAR_VA,
    CITY_BORDERING_SET_VA, WORLD_COMPUTE_ALL_TERRITORY_VA, WORLD_COMPUTE_REG_TERRITORY_VA,
};
use don_replay::starting_village_suffix::{
    apply_fresh_starting_village_terrain_census, CityTerrainGatherFacts,
    FreshVillageTerrainCensusFacts, FreshVillageUpgradeFacts,
};
use don_replay::world_owner_frontier::sha256;
use don_sim::systems::gather_terrain::{InstalledLandCatalog, SUPPORTED_RULES_XML_SHA256};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn source_addresses_and_fail_closed_status_are_frozen() {
    assert_eq!(WORLD_COMPUTE_REG_TERRITORY_VA, 0x006b_0bb0);
    assert_eq!(WORLD_COMPUTE_ALL_TERRITORY_VA, 0x006b_5700);
    assert_eq!(CITY_BORDERING_CLEAR_VA, 0x006b_0c40);
    assert_eq!(CITY_BORDERING_SET_VA, 0x006b_17c8);
}

#[test]
fn current_real_worlds_measure_the_exact_territory_projection_without_promotion() {
    let names = [
        "Playback___2018.11.17_13_21_42__Sat_.rcx",
        "Playback___2020.02.08_10_49_15__Sat_.rcx",
        "Playback___2020.02.21_09_48_48__Fri_.rcx",
    ];
    let rules_path = repo_root().join("ron-data/rules.xml");
    let Ok(rules_xml) = std::fs::read(&rules_path) else {
        eprintln!("skipping local retail input: {}", rules_path.display());
        return;
    };
    let rules_sha256 = sha256(&rules_xml);
    assert_eq!(rules_sha256, SUPPORTED_RULES_XML_SHA256);
    let catalog = InstalledLandCatalog::from_supported_source(&rules_xml, rules_sha256)
        .expect("supported installed LandData catalog");

    let mut fixtures = 0usize;
    let mut cities = 0usize;
    let mut foreign_rejections = 0u32;
    let mut bordering_sets = 0u32;
    let mut territory_matches = 0usize;
    let mut territory_census_matches = 0usize;
    for name in names {
        let path = repo_root().join("ron-data/replays/multi").join(name);
        if !path.exists() {
            continue;
        }
        fixtures += 1;
        let replay = Replay::open(&path)
            .unwrap_or_else(|error| panic!("{name}: replay parse failed: {error}"));
        let mut world_sim = WorldSim::from_replay(&replay);
        let initial_world = world_sim.initial_world.as_ref().expect("generated world");
        let setup = world_sim.initial_setup.as_mut().unwrap_or_else(|| {
            panic!("{name}: setup refused: {:?}", world_sim.initial_setup_error)
        });
        let projection = project_starting_city_territory(&replay, setup, initial_world)
            .unwrap_or_else(|error| panic!("{name}: territory refused: {error}"));
        assert!(projection.receipt.city_world_projection_complete);
        assert!(!projection.receipt.first_checksum_city_image_ready);
        assert!(!projection.receipt.input_world_complete);
        assert!(projection.receipt.input_world_unsourced_walked_bytes > 0);
        assert!(projection.receipt.unpublished_region_state);
        assert_eq!(projection.receipt.unpublished_leader_flag_or, 0x0200_0000);
        assert_eq!(projection.receipt.active_owners.len(), 2);
        assert_eq!(
            projection.receipt.bordering_cells_exhaustively_scanned,
            initial_world.world.size as u32
        );
        assert_eq!(projection.receipt.potential_contested_city_cells, 0);
        assert!(projection.receipt.bordering_zero_independent_of_final_map);
        let bordering = projection
            .bordering_authority
            .as_ref()
            .expect("disjoint starting influence discs own final bordering bytes");
        assert_eq!(bordering.values, [0; 8]);
        assert_eq!(
            bordering.cells_exhaustively_scanned,
            initial_world.world.size as u32
        );
        assert_eq!(
            projection.receipt.land_cells_claimed,
            initial_world.world.size as u32
        );
        bordering_sets = bordering_sets.wrapping_add(projection.receipt.city_bordering_sets);

        let territory_checksum = check_sim_cities(&setup.sim, &projection.cities)
            .expect("territory Cities")
            .checksum;
        let gather =
            CityTerrainGatherFacts::from_installed_land_catalog(&projection.world, &catalog)
                .expect("territory-bound mode-one gather facts");
        let mut census_cities = projection.cities.clone();
        for constructor in &setup.receipt.cities {
            let owner = usize::from(constructor.owner);
            let slot = constructor.city_slot as usize;
            let city = &mut census_cities.slots[owner][slot];
            let receipt = apply_fresh_starting_village_terrain_census(
                &setup.sim,
                city,
                &projection.world,
                &projection.regions,
                FreshVillageTerrainCensusFacts {
                    upgrade: FreshVillageUpgradeFacts {
                        town_type_available: true,
                    },
                    indian_radius_bonus: constructor.constructor.world_fix.radius_tiles > 20,
                    gather: &gather,
                },
            )
            .expect("territory-aware starting City census");
            cities += 1;
            foreign_rejections = foreign_rejections.wrapping_add(receipt.foreign_owner_rejections);
        }
        let territory_census_checksum = check_sim_cities(&setup.sim, &census_cities)
            .expect("territory-census Cities")
            .checksum;
        let retail_checksum = replay
            .turns
            .iter()
            .find_map(|turn| {
                turn.any_checksums()
                    .map(|(_, channels)| channels.get(Channel::Cities))
            })
            .expect("fixture has a first recorded Cities checkpoint");
        territory_matches += usize::from(territory_checksum == retail_checksum);
        territory_census_matches += usize::from(territory_census_checksum == retail_checksum);
        eprintln!(
            "{name}: territory={territory_checksum:08x} territory_census={territory_census_checksum:08x} retail={retail_checksum:08x} bordering={:?} territory_total={:?} cells={} unsourced={}",
            projection.receipt.bordering_after,
            projection.receipt.territory_total,
            projection.receipt.bordering_cells_exhaustively_scanned,
            projection.receipt.input_world_unsourced_walked_bytes,
        );

        // Mutation gate: the generation-Region owner is part of the source identity, not
        // interchangeable geometry supplied by a caller.
        let mut stale_map = initial_world.clone();
        let region = stale_map
            .generation_regions
            .list
            .iter_mut()
            .find(|region| region.size > 0)
            .expect("nonempty generated region");
        region.coords.items.pop();
        assert!(matches!(
            project_starting_city_territory(&replay, setup, &stale_map),
            Err(StartingCityTerritoryError::RegionCoordinateCount { .. })
        ));

        let owner = usize::from(setup.receipt.cities[0].owner);
        setup.sim.cities.slots[owner][0].x += 768;
        setup.cities.slots[owner][0].x += 768;
        assert!(matches!(
            project_starting_city_territory(&replay, setup, initial_world),
            Err(StartingCityTerritoryError::CityImageMismatch { owner: bad_owner })
                if bad_owner == owner
        ));
        setup.sim.cities.slots[owner][0].x -= 768;
        setup.cities.slots[owner][0].x -= 768;
    }

    if repo_root().join("ron-data/replays/multi").exists() {
        assert_eq!(fixtures, 3);
        assert_eq!(cities, 6);
        eprintln!(
            "territory projection boundary: fixtures={fixtures} cities={cities} bordering_sets={bordering_sets} foreign_rejections={foreign_rejections} matches territory={territory_matches} territory+census={territory_census_matches}"
        );
    }
}
