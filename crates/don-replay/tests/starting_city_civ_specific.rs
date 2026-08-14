//! Replay-backed proof that ordinary starting Cities issue no civ-specific free Build.

use std::path::{Path, PathBuf};

use don_replay::checksum::Channel;
use don_replay::harness::WorldSim;
use don_replay::replay::{load_payload, Replay};
use don_replay::starting_city_civ_specific::{
    prove_empty_starting_civ_specific_cohort, StartingCivSpecificError, CIV_SPECIFIC_BONUS_ORDER,
    LEADER_INIT_FLAGS2,
};
use don_sim::systems::leader_tribe_bonus_runtime::TribeBonusExit;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn fixture(name: &str) -> (Replay, Vec<u8>) {
    let path = repo_root().join("ron-data/replays/multi").join(name);
    let replay = Replay::open(&path).unwrap();
    let payload = load_payload(&path).unwrap();
    (replay, payload)
}

#[test]
fn admitted_corpus_has_six_exact_empty_civ_specific_schedules() {
    let cases = [
        (
            "Playback___2018.11.17_13_21_42__Sat_.rcx",
            [12u8, 14],
            0x7102_0a46,
            0x5313_0d2c,
        ),
        (
            "Playback___2020.02.08_10_49_15__Sat_.rcx",
            [12u8, 8],
            0x247c_0992,
            0xdd17_0c24,
        ),
        (
            "Playback___2020.02.21_09_48_48__Fri_.rcx",
            [12u8, 8],
            0xcd97_0956,
            0x7d2d_0bd4,
        ),
    ];

    let mut cities = 0usize;
    for (name, mut expected_bonuses, candidate, recorded) in cases {
        let (replay, payload) = fixture(name);
        let world = WorldSim::from_replay(&replay);
        assert!(
            world.initial_setup_error.is_none(),
            "{name}: {:?}",
            world.initial_setup_error
        );
        let setup = world.initial_setup.as_ref().unwrap();
        let receipt = prove_empty_starting_civ_specific_cohort(&replay, &payload, setup).unwrap();
        assert_eq!(receipt.cities.len(), 2);
        assert_eq!(receipt.cities_before, receipt.cities_after);
        assert_eq!(receipt.source_produced_city_bytes, 0);
        assert_eq!(receipt.city_mutations, 0);
        assert!(!receipt.installed_in_scoreboard);
        let mut actual_bonuses = receipt
            .cities
            .iter()
            .map(|city| city.tribe_default_bonus as u8)
            .collect::<Vec<_>>();
        actual_bonuses.sort_unstable();
        expected_bonuses.sort_unstable();
        assert_eq!(actual_bonuses, expected_bonuses);
        for city in &receipt.cities {
            cities += 1;
            assert_eq!(city.city_num, 1);
            assert_eq!(city.leader_flags2, LEADER_INIT_FLAGS2);
            assert_eq!(city.conquest_racial_powers, [0; 3]);
            assert_eq!(city.center_object_id, 2000);
            assert_eq!(
                payload[city.player_tribe_source.offset],
                city.tribe_selector
            );
            assert_eq!(city.player_tribe_source.bytes, 1);
            assert!(city.tribe_rules_source.bytes > 4);
            assert!(city.rule_reads.is_empty());
            assert!(city.free_build_types.is_empty());
            assert_eq!(
                city.probes
                    .iter()
                    .map(|probe| probe.bonus)
                    .collect::<Vec<_>>(),
                CIV_SPECIFIC_BONUS_ORDER
            );
            assert!(city.probes.iter().all(|probe| {
                !probe.granted
                    && probe.receipt.exit == TribeBonusExit::NotGranted
                    && probe.receipt.read_tribe_default
            }));
        }

        let (_, first_recorded) = replay
            .turns
            .iter()
            .find_map(|turn| turn.any_checksums())
            .expect("fixture has a checksum");
        assert_eq!(first_recorded.get(Channel::Cities), recorded);
        assert_eq!(receipt.cities_before.checksum, candidate);
        assert_ne!(receipt.cities_after.checksum, recorded);
    }
    assert_eq!(cities, 6);
}

#[test]
fn provenance_and_city_count_mutations_refuse_without_publication() {
    let name = "Playback___2018.11.17_13_21_42__Sat_.rcx";
    let (replay, payload) = fixture(name);
    let mut world = WorldSim::from_replay(&replay);
    let setup = world.initial_setup.as_mut().unwrap();
    let baseline = setup.sim.cities.clone();

    setup.receipt.replay_payload_sha256[0] ^= 1;
    assert_eq!(
        prove_empty_starting_civ_specific_cohort(&replay, &payload, setup).unwrap_err(),
        StartingCivSpecificError::SetupPayloadSha256Mismatch
    );
    assert_eq!(setup.sim.cities.slots, baseline.slots);
    setup.receipt.replay_payload_sha256[0] ^= 1;

    let mut changed_header = replay.clone();
    let player_slot = usize::from(setup.receipt.cities[0].replay_player_slot);
    changed_header.initial.info.players[player_slot].tribe = 22;
    assert_eq!(
        prove_empty_starting_civ_specific_cohort(&changed_header, &payload, setup).unwrap_err(),
        StartingCivSpecificError::ReplayPlayerSourceMismatch { slot: player_slot }
    );

    let owner = usize::from(setup.receipt.cities[0].owner);
    let extra_slot = setup.sim.cities.alloc_slot(owner);
    setup.sim.cities.slots[owner][extra_slot] = setup.receipt.cities[0].constructor.city.clone();
    let mutated = setup.sim.cities.clone();
    assert!(matches!(
        prove_empty_starting_civ_specific_cohort(&replay, &payload, setup).unwrap_err(),
        StartingCivSpecificError::Cities(_)
    ));
    assert_eq!(setup.sim.cities.slots, mutated.slots);
}

#[test]
fn payload_mutation_is_rejected_before_any_rules_projection() {
    let name = "Playback___2020.02.08_10_49_15__Sat_.rcx";
    let (replay, mut payload) = fixture(name);
    let world = WorldSim::from_replay(&replay);
    let setup = world.initial_setup.as_ref().unwrap();
    let source = replay.initial.rules.unwrap().serialized_offset;
    payload[source + 1] ^= 1;
    assert_eq!(
        prove_empty_starting_civ_specific_cohort(&replay, &payload, setup).unwrap_err(),
        StartingCivSpecificError::PayloadSha256Mismatch
    );
}
