use std::path::{Path, PathBuf};

use don_replay::checksum::Channel;
use don_replay::cities_runtime::{check_sim_owned_cities, CitiesRuntimeError};
use don_replay::harness::WorldSim;
use don_replay::replay::Replay;
use don_replay::starting_city_in_port::{
    apply_starting_city_in_port_clear, StartingCityInPortError, CITY_IN_PORT_CLEAR_LOOP_END_VA,
    CITY_IN_PORT_CLEAR_LOOP_VA, CITY_IN_PORT_CLEAR_STORE_VA, LEADER_PLAN_STRATEGY_VA,
};
use don_sim::objects::{Band, BUILD_BAND_BASE};
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

fn one_city_sim() -> Sim {
    let mut sim = Sim::new(0x4e, 8);
    sim.activate(0);
    let object_id = BUILD_BAND_BASE + sim.world.objects.slot(0).band(Band::Build).len() as u32;
    let mut build = BuildData::default();
    build.flags = production::flag::VALID | production::flag::ACTIVE;
    build.city = 0;
    set_build_identity(&mut build, object_id, 768, 1536);
    let row = sim.spawn_build(0, build);
    sim.production_runtime.register_build(row, 414);
    sim.cities.slots[0][0] = CityRecord {
        city_flags: 1,
        city: 0,
        o: object_id as i16,
        x: 768,
        y: 1536,
        who: 0,
        in_port: 0x1234,
        ..CityRecord::default()
    };
    sim.cities.city_mark[0] = 1;
    sim
}

#[test]
fn exact_store_is_atomic_and_changes_only_the_two_in_port_bytes() {
    assert_eq!(LEADER_PLAN_STRATEGY_VA, 0x006b_9620);
    assert_eq!(CITY_IN_PORT_CLEAR_LOOP_VA, 0x006b_9df7);
    assert_eq!(CITY_IN_PORT_CLEAR_STORE_VA, 0x006b_9e1f);
    assert_eq!(CITY_IN_PORT_CLEAR_LOOP_END_VA, 0x006b_9e2c);

    let mut sim = one_city_sim();
    let before = sim.cities.slots[0][0].pod_bytes();
    let receipt = apply_starting_city_in_port_clear(&mut sim).unwrap();
    let after = sim.cities.slots[0][0].pod_bytes();
    assert_eq!(sim.cities.slots[0][0].in_port, 0);
    assert_eq!(receipt.owners_walked, 1);
    assert_eq!(receipt.slots_scanned, 1);
    assert_eq!(receipt.cities_cleared, 1);
    assert_eq!(receipt.city_pod_bytes_changed, 2);
    assert_eq!(receipt.cities[0].before, before);
    assert_eq!(receipt.cities[0].after, after);
    assert!(receipt.city_in_port_clear_complete);
    assert!(!receipt.first_checksum_city_image_ready);
    assert_eq!(
        (
            receipt.world_writes,
            receipt.build_or_registry_writes,
            receipt.main_rng_draws
        ),
        (0, 0, 0)
    );
    let changed_offsets: Vec<_> = before
        .iter()
        .zip(after.iter())
        .enumerate()
        .filter_map(|(offset, (lhs, rhs))| (lhs != rhs).then_some(offset))
        .collect();
    assert_eq!(changed_offsets, [72, 73]);

    let mut refused = one_city_sim();
    refused.cities.slots[0][0].city = 3;
    assert!(matches!(
        apply_starting_city_in_port_clear(&mut refused),
        Err(StartingCityInPortError::Cities(
            CitiesRuntimeError::CitySlotMismatch { city_slot: 3, .. }
        ))
    ));
    assert_eq!(refused.cities.slots[0][0].in_port, 0x1234);
}

#[test]
fn current_real_candidates_prove_six_zero_after_images_and_no_checksum_change() {
    let fixtures_expected = [
        (
            "Playback___2018.11.17_13_21_42__Sat_.rcx",
            0x7102_0a46,
            0x5313_0d2c,
        ),
        (
            "Playback___2020.02.08_10_49_15__Sat_.rcx",
            0x247c_0992,
            0xdd17_0c24,
        ),
        (
            "Playback___2020.02.21_09_48_48__Fri_.rcx",
            0xcd97_0956,
            0x7d2d_0bd4,
        ),
    ];
    let mut fixtures = 0usize;
    let mut cities = 0u32;
    let mut changed_bytes = 0u32;
    let mut candidate_changes = 0usize;
    let mut matches = 0usize;

    for (name, expected_candidate, expected_retail) in fixtures_expected {
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
        let receipt = apply_starting_city_in_port_clear(&mut setup.sim)
            .unwrap_or_else(|error| panic!("{name}: in-port clear refused: {error}"));
        let after = check_sim_owned_cities(&setup.sim).expect("in-port-cleared Cities");
        let retail = replay
            .turns
            .iter()
            .find_map(|turn| {
                turn.any_checksums()
                    .map(|(_, channels)| channels.get(Channel::Cities))
            })
            .expect("fixture has a first recorded Cities checkpoint");

        assert_eq!(before.checksum, expected_candidate);
        assert_eq!(retail, expected_retail);

        assert_eq!(receipt.owners_walked, 2);
        assert_eq!(receipt.slots_scanned, 2);
        assert_eq!(receipt.cities_cleared, 2);
        assert!(receipt
            .cities
            .iter()
            .all(|city| city.before[72..74] == [0, 0] && city.after[72..74] == [0, 0]));
        cities = cities.wrapping_add(receipt.cities_cleared);
        changed_bytes = changed_bytes.wrapping_add(receipt.city_pod_bytes_changed);
        candidate_changes += usize::from(before.checksum != after.checksum);
        matches += usize::from(after.checksum == retail);
        eprintln!(
            "{name}: in_port candidate {:08x}->{:08x} retail={retail:08x} changed_bytes={}",
            before.checksum, after.checksum, receipt.city_pod_bytes_changed
        );
    }

    if repo_root().join("ron-data/replays/multi").exists() {
        assert_eq!(fixtures, 3);
        assert_eq!(cities, 6);
        assert_eq!(changed_bytes, 0);
        assert_eq!(candidate_changes, 0);
        assert_eq!(matches, 0);
        eprintln!(
            "starting City in_port: fixtures=3 cities=6 exact_zero_after_images=6 changed_bytes=0 candidate_changes=0 matches=0"
        );
    }
}
