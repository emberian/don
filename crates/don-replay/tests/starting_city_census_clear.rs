use std::path::{Path, PathBuf};

use don_replay::checksum::Channel;
use don_replay::cities_runtime::{check_sim_owned_cities, CitiesRuntimeError};
use don_replay::harness::WorldSim;
use don_replay::replay::Replay;
use don_replay::starting_city_census_clear::{
    apply_starting_city_census_clear, initial_peasant_dist_for_slot, StartingCityCensusClearError,
    CITY_BUSY_CLEAR_STORE_VA, CITY_CENSUS_CLEAR_LOOP_BEGIN_VA, CITY_CENSUS_CLEAR_LOOP_END_VA,
    CITY_FREE_CLEAR_STORE_VA, CITY_GATHERERS_CLEAR_STORE_VA, CITY_PEASANT_DIST_INIT_STORE_VA,
    INITIAL_PEASANT_DIST_BASE, LEADER_PLAN_STRATEGY_VA,
};
use don_replay::starting_city_territory::project_starting_city_territory;
use don_replay::starting_village_suffix::{
    apply_fresh_starting_village_terrain_census, CityTerrainGatherFacts,
    FreshVillageTerrainCensusFacts, FreshVillageUpgradeFacts,
};
use don_replay::world_owner_frontier::sha256;
use don_sim::objects::{Band, BUILD_BAND_BASE};
use don_sim::systems::gather_terrain::{InstalledLandCatalog, SUPPORTED_RULES_XML_SHA256};
use don_sim::systems::production::{self, BuildData};
use don_sim::systems::tech_cities::CityRecord;
use don_sim::tick::Sim;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn set_build_identity(build: &mut BuildData, object_id: u32, x: i32, y: i32) {
    build.other[production::off::OBJECT_ID..production::off::OBJECT_ID + 2]
        .copy_from_slice(&(object_id as i16).to_le_bytes());
    build.other[production::off::X_INTERNAL..production::off::X_INTERNAL + 4]
        .copy_from_slice(&(x ^ 0x63637).to_le_bytes());
    build.other[production::off::Y_INTERNAL..production::off::Y_INTERNAL + 4]
        .copy_from_slice(&(y ^ 0x63637).to_le_bytes());
}

fn spawn_city(sim: &mut Sim, slot: usize, x: i32, y: i32) {
    let object_id = BUILD_BAND_BASE + sim.world.objects.slot(0).band(Band::Build).len() as u32;
    let mut build = BuildData::default();
    build.flags = production::flag::VALID | production::flag::ACTIVE;
    build.city = slot as i16;
    set_build_identity(&mut build, object_id, x, y);
    let row = sim.spawn_build(0, build);
    sim.production_runtime.register_build(row, 414);
    sim.cities.slots[0][slot] = CityRecord {
        city_flags: 1,
        city: slot as i16,
        o: object_id as i16,
        x,
        y,
        who: 0,
        peasant_dist: -12,
        free: 9,
        busy: 8,
        gatherers: 7,
        ..CityRecord::default()
    };
    sim.cities.city_mark[0] = sim.cities.city_mark[0].max(slot as i32 + 1);
}

fn two_city_sim() -> Sim {
    let mut sim = Sim::new(0x64, 8);
    sim.activate(0);
    spawn_city(&mut sim, 0, 768, 1536);
    // Preserve an inactive slot inside city_mark to freeze the exact scan bound.
    spawn_city(&mut sim, 2, 2304, 3072);
    sim
}

#[test]
fn exact_clear_is_slot_dependent_atomic_and_touches_only_four_fields() {
    assert_eq!(LEADER_PLAN_STRATEGY_VA, 0x006b_9620);
    assert_eq!(CITY_CENSUS_CLEAR_LOOP_BEGIN_VA, 0x006b_9746);
    assert_eq!(CITY_GATHERERS_CLEAR_STORE_VA, 0x006b_976f);
    assert_eq!(CITY_BUSY_CLEAR_STORE_VA, 0x006b_9789);
    assert_eq!(CITY_FREE_CLEAR_STORE_VA, 0x006b_97a3);
    assert_eq!(CITY_PEASANT_DIST_INIT_STORE_VA, 0x006b_97bd);
    assert_eq!(CITY_CENSUS_CLEAR_LOOP_END_VA, 0x006b_97ca);
    assert_eq!(INITIAL_PEASANT_DIST_BASE, 100);
    assert_eq!(initial_peasant_dist_for_slot(0), 100);
    assert_eq!(initial_peasant_dist_for_slot(2), 102);
    assert_eq!(initial_peasant_dist_for_slot(i16::MAX), i16::MIN + 99);

    let mut sim = two_city_sim();
    let before = [
        sim.cities.slots[0][0].pod_bytes(),
        sim.cities.slots[0][2].pod_bytes(),
    ];
    let receipt = apply_starting_city_census_clear(&mut sim).unwrap();
    assert_eq!(receipt.owners_walked, 1);
    assert_eq!(receipt.slots_scanned, 3);
    assert_eq!(receipt.cities_cleared, 2);
    assert_eq!(receipt.city_pod_bytes_changed, 10);
    assert!(receipt.city_census_clear_complete);
    assert!(!receipt.city_unit_census_complete);
    assert!(!receipt.first_checksum_city_image_ready);
    assert_eq!(
        (
            receipt.world_writes,
            receipt.build_or_registry_writes,
            receipt.main_rng_draws
        ),
        (0, 0, 0)
    );
    for (index, slot) in [0usize, 2].into_iter().enumerate() {
        let city = &sim.cities.slots[0][slot];
        assert_eq!(city.peasant_dist, 100 + slot as i16);
        assert_eq!((city.free, city.busy, city.gatherers), (0, 0, 0));
        let after = city.pod_bytes();
        let changed_offsets: Vec<_> = before[index]
            .iter()
            .zip(after.iter())
            .enumerate()
            .filter_map(|(offset, (lhs, rhs))| (lhs != rhs).then_some(offset))
            .collect();
        assert_eq!(changed_offsets, [74, 75, 84, 85, 86]);
    }

    let mut refused = two_city_sim();
    refused.cities.slots[0][2].city = 3;
    assert!(matches!(
        apply_starting_city_census_clear(&mut refused),
        Err(StartingCityCensusClearError::Cities(
            CitiesRuntimeError::CitySlotMismatch {
                slot: 2,
                city_slot: 3,
                ..
            }
        ))
    ));
    assert_eq!(refused.cities.slots[0][0].peasant_dist, -12);
    assert_eq!(refused.cities.slots[0][0].free, 9);
}

#[test]
fn current_real_candidates_advance_by_six_exact_clear_bytes_without_promotion() {
    let fixtures_expected = [
        (
            "Playback___2018.11.17_13_21_42__Sat_.rcx",
            0x7102_0a46,
            0xbb3a_0b0e,
            0x5313_0d2c,
        ),
        (
            "Playback___2020.02.08_10_49_15__Sat_.rcx",
            0x247c_0992,
            0x6eb4_0a5a,
            0xdd17_0c24,
        ),
        (
            "Playback___2020.02.21_09_48_48__Fri_.rcx",
            0xcd97_0956,
            0x17de_0a1e,
            0x7d2d_0bd4,
        ),
    ];
    let mut fixtures = 0usize;
    let mut cities = 0u32;
    let mut changed_bytes = 0u32;
    let mut candidate_changes = 0usize;
    let mut matches = 0usize;

    for (name, expected_before, expected_after, expected_retail) in fixtures_expected {
        let path = repo_root().join("ron-data/replays/multi").join(name);
        if !path.exists() {
            continue;
        }
        fixtures += 1;
        let replay = Replay::open(&path)
            .unwrap_or_else(|error| panic!("{name}: replay parse failed: {error}"));
        let mut world_sim = WorldSim::from_replay(&replay);
        let setup = world_sim.initial_setup.as_mut().unwrap_or_else(|| {
            panic!("{name}: setup refused: {:?}", world_sim.initial_setup_error)
        });
        let before = check_sim_owned_cities(&setup.sim).expect("constructor Cities");
        let receipt = apply_starting_city_census_clear(&mut setup.sim)
            .unwrap_or_else(|error| panic!("{name}: census clear refused: {error}"));
        let after = check_sim_owned_cities(&setup.sim).expect("clear-prefix Cities");
        let retail = replay
            .turns
            .iter()
            .find_map(|turn| {
                turn.any_checksums()
                    .map(|(_, channels)| channels.get(Channel::Cities))
            })
            .expect("fixture has a first recorded Cities checkpoint");

        assert_eq!(before.checksum, expected_before);
        assert_eq!(after.checksum, expected_after);
        assert_eq!(retail, expected_retail);

        assert_eq!(receipt.owners_walked, 2);
        assert_eq!(receipt.slots_scanned, 2);
        assert_eq!(receipt.cities_cleared, 2);
        assert_eq!(receipt.city_pod_bytes_changed, 2);
        assert!(receipt.cities.iter().all(|city| {
            city.before[74..76] == [0, 0]
                && city.after[74..76] == [100, 0]
                && city.before[84..87] == [0, 0, 0]
                && city.after[84..87] == [0, 0, 0]
        }));
        cities = cities.wrapping_add(receipt.cities_cleared);
        changed_bytes = changed_bytes.wrapping_add(receipt.city_pod_bytes_changed);
        candidate_changes += usize::from(before.checksum != after.checksum);
        matches += usize::from(after.checksum == retail);
        eprintln!(
            "{name}: clear-prefix candidate {:08x}->{:08x} retail={retail:08x} changed_bytes={}",
            before.checksum, after.checksum, receipt.city_pod_bytes_changed
        );
    }

    if repo_root().join("ron-data/replays/multi").exists() {
        assert_eq!(fixtures, 3);
        assert_eq!(cities, 6);
        assert_eq!(changed_bytes, 6);
        assert_eq!(candidate_changes, 3);
        assert_eq!(matches, 0);
        eprintln!(
            "starting City census clear: fixtures=3 cities=6 changed_bytes=6 candidate_changes=3 matches=0"
        );
    }
}

#[test]
fn exact_clear_composes_with_the_existing_terrain_diagnostic_without_claiming_final_state() {
    let fixtures_expected = [
        (
            "Playback___2018.11.17_13_21_42__Sat_.rcx",
            0x34a1_0d02,
            0x7ed9_0dca,
            0x5313_0d2c,
        ),
        (
            "Playback___2020.02.08_10_49_15__Sat_.rcx",
            0xe80c_0c4e,
            0x3253_0d16,
            0xdd17_0c24,
        ),
        (
            "Playback___2020.02.21_09_48_48__Fri_.rcx",
            0x9136_0c12,
            0xdb6e_0cda,
            0x7d2d_0bd4,
        ),
    ];
    let rules_path = repo_root().join("ron-data/rules.xml");
    let Ok(rules_xml) = std::fs::read(&rules_path) else {
        return;
    };
    let rules_sha256 = sha256(&rules_xml);
    assert_eq!(rules_sha256, SUPPORTED_RULES_XML_SHA256);
    let catalog = InstalledLandCatalog::from_supported_source(&rules_xml, rules_sha256)
        .expect("supported installed LandData catalog");
    let mut fixtures = 0usize;
    let mut matches = 0usize;

    for (name, expected_before, expected_after, expected_retail) in fixtures_expected {
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
        assert!(!projection.receipt.input_world_complete);
        assert!(!projection.receipt.first_checksum_city_image_ready);
        let gather =
            CityTerrainGatherFacts::from_installed_land_catalog(&projection.world, &catalog)
                .expect("territory-bound mode-one gather facts");
        let mut cities = projection.cities;
        for constructor in &setup.receipt.cities {
            let city =
                &mut cities.slots[usize::from(constructor.owner)][constructor.city_slot as usize];
            apply_fresh_starting_village_terrain_census(
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
            .expect("terrain census");
        }
        setup.sim.cities = cities;
        let before = check_sim_owned_cities(&setup.sim).expect("terrain-census Cities");
        let clear = apply_starting_city_census_clear(&mut setup.sim).expect("clear prefix");
        let after = check_sim_owned_cities(&setup.sim).expect("terrain plus clear Cities");
        let retail = replay
            .turns
            .iter()
            .find_map(|turn| {
                turn.any_checksums()
                    .map(|(_, channels)| channels.get(Channel::Cities))
            })
            .expect("fixture has a first recorded Cities checkpoint");
        assert_eq!(before.checksum, expected_before);
        assert_eq!(after.checksum, expected_after);
        assert_eq!(retail, expected_retail);
        assert_eq!(clear.city_pod_bytes_changed, 2);
        matches += usize::from(after.checksum == retail);
        eprintln!(
            "{name}: diagnostic terrain+clear {expected_before:08x}->{expected_after:08x} retail={retail:08x}"
        );
    }

    if repo_root().join("ron-data/replays/multi").exists() {
        assert_eq!(fixtures, 3);
        assert_eq!(matches, 0);
        eprintln!("diagnostic terrain+clear: fixtures=3 changed_bytes=6 matches=0");
    }
}
