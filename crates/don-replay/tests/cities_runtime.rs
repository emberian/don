//! Exact Cities-channel adapter and first-checkpoint producer-boundary tests.

#[path = "../src/cities_runtime.rs"]
mod cities_runtime;

use cities_runtime::*;
use don_replay::checksum::{Channel, CheckSum};
use don_replay::replay::{corpus, Replay};
use don_replay::walk::walk_class;
use don_replay::walk_gen::class_index;
use don_sim::objects::{Band, BUILD_BAND_BASE};
use don_sim::systems::production::{self, BuildData};
use don_sim::systems::tech_cities::{CaravanLink, CityPool, CityRecord};
use don_sim::tick::Sim;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn skip(reason: &str) {
    eprintln!("\n  SKIPPED — NOT A PASS. {reason}\n  Nothing corpus-backed was established.\n");
}

fn set_build_identity(build: &mut BuildData, object_id: u32, x: i32, y: i32) {
    build.other[production::off::OBJECT_ID..production::off::OBJECT_ID + 2]
        .copy_from_slice(&(object_id as i16).to_le_bytes());
    build.other[production::off::X_INTERNAL..production::off::X_INTERNAL + 4]
        .copy_from_slice(&(x ^ 0x63637).to_le_bytes());
    build.other[production::off::Y_INTERNAL..production::off::Y_INTERNAL + 4]
        .copy_from_slice(&(y ^ 0x63637).to_le_bytes());
}

fn spawn_center(
    sim: &mut Sim,
    owner: usize,
    city_slot: usize,
    type_index: i32,
    x: i32,
    y: i32,
) -> (usize, u32) {
    sim.activate(owner);
    let object_id = BUILD_BAND_BASE + sim.world.objects.slot(owner).band(Band::Build).len() as u32;
    let mut build = BuildData::default();
    build.flags = production::flag::VALID | production::flag::ACTIVE;
    build.city = city_slot as i16;
    set_build_identity(&mut build, object_id, x, y);
    let row = sim.spawn_build(owner, build);
    sim.production_runtime.register_build(row, type_index);
    (row, object_id)
}

fn install_city(pool: &mut CityPool, owner: usize, slot: usize, object_id: u32, x: i32, y: i32) {
    pool.slots[owner][slot] = CityRecord {
        city_flags: 1,
        city: slot as i16,
        o: object_id as i16,
        x,
        y,
        who: owner as i8,
        ..CityRecord::default()
    };
    pool.city_mark[owner] = pool.city_mark[owner].max(slot as i32 + 1);
}

#[test]
fn exact_city_stream_skips_names_and_owns_the_dynamic_caravan_array() {
    let mut city = CityRecord {
        city_flags: 0x9131,
        city: 3,
        o: 2004,
        reg: 11,
        x: 0x1122_3344,
        y: -0x1020_304,
        attack_stamp: 7,
        who: 2,
        name: "checksum must skip me".into(),
        id: "and me".into(),
        ..CityRecord::default()
    };
    let empty = city_walk_bytes(&city).unwrap();
    assert_eq!(empty.len() as u64, EMPTY_CARAVAN_CITY_WALK_BYTES);
    assert_eq!(&empty[0..2], &city.city_flags.to_le_bytes());
    assert_eq!(&empty[2..110], &city.pod_bytes());
    assert_eq!(&empty[110..114], &0i32.to_le_bytes());

    let mut dead = city.clone();
    dead.city_flags &= !1;
    assert_eq!(
        city_walk_bytes(&dead).unwrap(),
        dead.city_flags.to_le_bytes()
    );

    let mut renamed = city.clone();
    renamed.name = "different".into();
    renamed.id = "different-id".into();
    assert_eq!(city_walk_bytes(&renamed).unwrap(), empty);

    city.vans.items.push(CaravanLink {
        cara: 0x1020_3040,
        who: 6,
    });
    city.vans.capacity = 10;
    city.vans.grow = -1;
    city.vans.flags = 0x45;
    let linked = city_walk_bytes(&city).unwrap();
    assert_eq!(linked.len(), 129);
    assert_eq!(&linked[110..114], &1i32.to_le_bytes());
    assert_eq!(&linked[114..118], &10i32.to_le_bytes());
    assert_eq!(&linked[118..120], &(-1i16).to_le_bytes());
    assert_eq!(linked[120], 0x05, "Array masks flag bit 0x40");
    assert_eq!(&linked[121..125], &0x1020_3040i32.to_le_bytes());
    assert_eq!(&linked[125..129], &6i32.to_le_bytes());
    assert_ne!(
        city_walk_value(&city).unwrap().checksum,
        city_walk_value(&renamed).unwrap().checksum
    );
}

#[test]
fn generic_flat_image_walker_is_not_a_complete_city_checksum_walker() {
    assert_eq!(CHECK_CITIES_VA, 0x0093_7600);
    assert_eq!(CITY_WALK_DATA_VA, 0x0048_9220);
    assert_eq!(CARAVAN_ARRAY_WALK_VA, 0x0048_9040);
    assert_eq!(CITIES_INIT_VA, 0x0073_58c0);
    assert_eq!(CITIES_INIT_CITY_VA, 0x0073_52c0);
    assert_eq!(CITY_INIT_VA, 0x0073_7050);
    assert_eq!(SETUP_BUILD_CITIES_VA, 0x005a_b910);
    assert_eq!(SETUP_BUILD_GAME_VA, 0x005a_c190);

    let class = class_index("City").expect("generated City walk exists");
    let mut image = vec![0u8; 192];
    image[4] = 1;
    let mut checksum = CheckSum::new();
    let outcome = walk_class(class, &image, &mut checksum, 8);

    assert!(!outcome.is_complete());
    assert!(outcome.ops_unresolved > 0);
    assert_eq!(outcome.bytes_walked, 112);
    assert_eq!(checksum.bytes, 112);

    let exact = city_walk_bytes(&CityRecord {
        city_flags: 1,
        ..CityRecord::default()
    })
    .unwrap();
    assert_eq!(exact.len() as u64, EMPTY_CARAVAN_CITY_WALK_BYTES);
    assert_ne!(checksum.checksum, don_sim::checksum::adler32(1, &exact));
}

#[test]
fn channel_uses_leader_then_full_ptrarray_order_not_dense_build_row_order() {
    let mut sim = Sim::new(0x51, 8);
    let mut cities = CityPool::new();

    let (_owner1_row, owner1_object) = spawn_center(&mut sim, 1, 0, 414, 900, 1200);
    install_city(&mut cities, 1, 0, owner1_object, 900, 1200);
    cities.slots[1][0].capture_stamp = 101;

    let (_owner0_row, owner0_object) = spawn_center(&mut sim, 0, 0, 414, 300, 600);
    install_city(&mut cities, 0, 0, owner0_object, 300, 600);
    cities.slots[0][0].capture_stamp = 202;

    let result = check_sim_cities(&sim, &cities).unwrap();
    let owner0 = city_walk_bytes(&cities.slots[0][0]).unwrap();
    let owner1 = city_walk_bytes(&cities.slots[1][0]).unwrap();
    let expected = don_sim::checksum::adler32(don_sim::checksum::adler32(1, &owner0), &owner1);
    let build_row_order =
        don_sim::checksum::adler32(don_sim::checksum::adler32(1, &owner1), &owner0);

    assert_eq!(result.checksum, expected);
    assert_ne!(result.checksum, build_row_order);
    assert_eq!(result.bytes_walked, 2 * EMPTY_CARAVAN_CITY_WALK_BYTES);
    assert_eq!(result.cities_walked, 2);
    assert_eq!(
        result.slots_scanned, 40,
        "retail scans all 20 logical slots per valid owner"
    );
}

#[test]
fn inactive_leader_rows_are_validated_but_contribute_no_city_bytes() {
    let mut sim = Sim::new(0x52, 8);
    let mut cities = CityPool::new();
    let object_id = BUILD_BAND_BASE;
    let mut build = BuildData::default();
    build.flags = production::flag::VALID | production::flag::ACTIVE;
    build.city = 0;
    set_build_identity(&mut build, object_id, 300, 600);
    let row = sim.spawn_build(0, build);
    sim.production_runtime.register_build(row, 414);
    assert!(sim.world.set_object_owner_active(0, false));
    install_city(&mut cities, 0, 0, object_id, 300, 600);

    let result = check_sim_cities(&sim, &cities).unwrap();
    assert_eq!(result.checksum, 1);
    assert_eq!(result.bytes_walked, 0);
    assert_eq!(result.cities_walked, 0);
    assert_eq!(result.slots_scanned, 0);
}

#[test]
fn split_leader_owner_and_current_spawn_build_identity_gap_fail_closed() {
    let mut split = Sim::new(0x53, 8);
    split.activate(0);
    split.leaders[0].active = false;
    assert_eq!(
        check_sim_cities(&split, &CityPool::new()),
        Err(CitiesRuntimeError::LeaderValidityMismatch {
            owner: 0,
            victory: true,
            step8: true,
            economy: false,
            object_registry: true,
        })
    );

    let mut identity = Sim::new(0x54, 8);
    identity.activate(0);
    let mut build = BuildData::default();
    build.flags = production::flag::VALID | production::flag::ACTIVE;
    build.city = 0;
    // Sim::spawn_build currently registers the band object but does not write
    // SubObjectData::o or the encoded position into BuildData.
    let row = identity.spawn_build(0, build);
    identity.production_runtime.register_build(row, 414);
    let mut cities = CityPool::new();
    install_city(&mut cities, 0, 0, BUILD_BAND_BASE, 300, 600);
    assert_eq!(
        check_sim_cities(&identity, &cities),
        Err(CitiesRuntimeError::CenterBuildObjectIdMismatch {
            owner: 0,
            slot: 0,
            registry_object_id: BUILD_BAND_BASE,
            build_object_id: 0,
        })
    );
}

#[test]
fn center_type_city_index_position_and_unique_identity_are_mandatory() {
    let mut missing_type = Sim::new(0x55, 8);
    let mut cities = CityPool::new();
    let (row, object_id) = spawn_center(&mut missing_type, 0, 0, 414, 300, 600);
    install_city(&mut cities, 0, 0, object_id, 300, 600);
    missing_type.production_runtime.build_types[row] = None;
    assert_eq!(
        check_sim_cities(&missing_type, &cities),
        Err(CitiesRuntimeError::MissingCenterBuildType {
            owner: 0,
            slot: 0,
            object_id,
            build_row: row,
        })
    );

    let mut wrong_city = Sim::new(0x56, 8);
    let mut cities = CityPool::new();
    let (row, object_id) = spawn_center(&mut wrong_city, 0, 0, 414, 300, 600);
    install_city(&mut cities, 0, 0, object_id, 300, 600);
    wrong_city.builds[row].city = 4;
    assert!(matches!(
        check_sim_cities(&wrong_city, &cities),
        Err(CitiesRuntimeError::CenterBuildCityMismatch { build_city: 4, .. })
    ));

    let mut wrong_position = Sim::new(0x57, 8);
    let mut cities = CityPool::new();
    let (_row, object_id) = spawn_center(&mut wrong_position, 0, 0, 414, 300, 600);
    install_city(&mut cities, 0, 0, object_id, 301, 600);
    assert!(matches!(
        check_sim_cities(&wrong_position, &cities),
        Err(CitiesRuntimeError::CenterBuildPositionMismatch { .. })
    ));

    let mut duplicate = Sim::new(0x58, 8);
    let mut cities = CityPool::new();
    let (_row, object_id) = spawn_center(&mut duplicate, 0, 0, 414, 300, 600);
    install_city(&mut cities, 0, 0, object_id, 300, 600);
    install_city(&mut cities, 0, 1, object_id, 300, 600);
    assert_eq!(
        check_sim_cities(&duplicate, &cities),
        Err(CitiesRuntimeError::DuplicateCenterBuild {
            owner: 0,
            object_id,
            first_slot: 0,
            second_slot: 1,
        })
    );
}

#[test]
fn city_mark_and_slot_identity_are_owner_state_not_advisory_metadata() {
    let mut sim = Sim::new(0x59, 8);
    let mut cities = CityPool::new();
    let (_row, object_id) = spawn_center(&mut sim, 0, 0, 414, 300, 600);
    install_city(&mut cities, 0, 0, object_id, 300, 600);
    cities.city_mark[0] = 0;
    assert_eq!(
        check_sim_cities(&sim, &cities),
        Err(CitiesRuntimeError::ActiveCityBeyondMark {
            owner: 0,
            slot: 0,
            mark: 0,
        })
    );

    cities.city_mark[0] = 1;
    cities.slots[0][0].city = 3;
    assert_eq!(
        check_sim_cities(&sim, &cities),
        Err(CitiesRuntimeError::CitySlotMismatch {
            owner: 0,
            slot: 0,
            city_slot: 3,
        })
    );
}

/// Corpus boundary only. Recorded Cities checksums are outputs used to prove the exact
/// adapter is still missing its setup producer; no recorded word enters `check_sim_cities`.
#[test]
fn corpus_first_cities_is_nonempty_while_the_replay_bridge_has_no_producer() {
    let files = corpus(&repo_root());
    if files.is_empty() {
        skip("ron-data/replays contains no .rcx files.");
        return;
    }
    assert_eq!(
        don_replay::check_all::CHANNEL_SOURCE[Channel::Cities as usize],
        don_replay::check_all::ChannelSource::Absent,
        "an adapter without Setup::build_cities must not be advertised as a producer"
    );

    let mut checksummed = 0usize;
    let mut first_values = BTreeSet::new();
    let mut first_turns = BTreeSet::new();
    for path in &files {
        let Ok(rep) = Replay::open(path) else {
            continue;
        };
        let Some((turn, recorded)) = rep
            .turns
            .iter()
            .find_map(|turn| turn.any_checksums().map(|(_, sums)| (turn.turn, sums)))
        else {
            continue;
        };
        checksummed += 1;
        first_turns.insert(turn);
        let retail_cities = recorded.get(Channel::Cities);
        first_values.insert(retail_cities);
        assert_ne!(
            retail_cities,
            1,
            "{}: retail's first Cities checkpoint is unexpectedly empty",
            path.display()
        );
    }

    assert!(checksummed > 0, "no replay carried CheckSumsCommand 0x39");
    if files.len() == 61 {
        assert_eq!(checksummed, 21, "the known 61-file corpus changed");
        assert_eq!(first_turns, BTreeSet::from([2]));
        assert_eq!(
            first_values.len(),
            21,
            "first Cities values stopped varying"
        );
    }
    eprintln!(
        "  {checksummed} checksum-bearing recordings, {} distinct first Cities values; all are nonempty",
        first_values.len()
    );
}
