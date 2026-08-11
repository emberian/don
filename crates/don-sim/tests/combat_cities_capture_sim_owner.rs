use don_sim::objects::BUILD_BAND_BASE;
use don_sim::systems::combat::build_check_capture::CityKey;
use don_sim::systems::combat::cities_capture_prefix::{
    CaptureCounter, IncrementCaptureCounterRequest,
};
use don_sim::systems::combat::cities_capture_sim_owner::{
    apply_sim_capture_counter, apply_sim_city_record_capture, SimCaptureCounterError,
    SimCityRecordCaptureError,
};
use don_sim::systems::combat::cities_capture_swap_fork::CityRecordCaptureRequest;
use don_sim::systems::combat::damage_world::ObjectKey;
use don_sim::systems::{production, save_load, tech_cities};
use don_sim::tick::Sim;

fn set_position(build: &mut production::BuildData, x: i32, y: i32) {
    build.other[production::off::X_INTERNAL..production::off::X_INTERNAL + 4]
        .copy_from_slice(&(x ^ 0x63637).to_le_bytes());
    build.other[production::off::Y_INTERNAL..production::off::Y_INTERNAL + 4]
        .copy_from_slice(&(y ^ 0x63637).to_le_bytes());
}

#[test]
fn real_sim_city_owner_captures_checksum_record_and_center_damage() {
    let mut sim = Sim::new(7, 8);
    sim.vic_leaders.slots[1].leader_flags = 3;
    sim.vic_leaders.slots[2].leader_flags = 3;

    let mut old_build = production::BuildData {
        flags: production::flag::VALID | production::flag::ACTIVE,
        construct_hits: 100,
        city: 0,
        ..Default::default()
    };
    set_position(&mut old_build, 1_000, 2_000);
    let old_row = sim.spawn_build(1, old_build);
    let mut new_build = production::BuildData {
        flags: production::flag::VALID | production::flag::ACTIVE,
        construct_hits: 160,
        city: 0,
        ..Default::default()
    };
    set_position(&mut new_build, 3_000, 4_000);
    let new_row = sim.spawn_build(2, new_build);

    assert_eq!(sim.cities.alloc_slot(1), 0);
    sim.cities.slots[1][0] = tech_cities::CityRecord {
        city_flags: 1,
        city: 0,
        o: BUILD_BAND_BASE as i16,
        who: 1,
        race: 1,
        founder: 1,
        x: 1_000,
        y: 2_000,
        ..Default::default()
    };
    assert_eq!(sim.cities.alloc_slot(2), 0);

    let before_digest = sim.channel_digest();
    let request = CityRecordCaptureRequest {
        old_city: CityKey { who: 1, city: 0 },
        new_city: CityKey { who: 2, city: 0 },
        old_center: ObjectKey {
            who: 1,
            o: BUILD_BAND_BASE as i16,
        },
        new_center: ObjectKey {
            who: 2,
            o: BUILD_BAND_BASE as i16,
        },
    };
    let receipt = apply_sim_city_record_capture(&mut sim, request).unwrap();

    assert!(receipt.capture_applied);
    assert_eq!(receipt.ignored_ecx_residue.0, 150);
    assert_eq!(sim.builds[old_row].damage, 0);
    assert_eq!(sim.builds[new_row].damage, 150);
    let captured = &sim.cities.slots[2][0];
    assert!(captured.active());
    assert_eq!((captured.who, captured.city, captured.o), (2, 0, 2000));
    assert_eq!((captured.x, captured.y), (3_000, 4_000));
    assert_eq!(
        captured.race, 1,
        "hostile capture preserves the city's race"
    );
    assert_ne!(sim.channel_digest(), before_digest);
    assert_eq!(
        save_load::save_sim(&sim),
        Err(save_load::SaveError::Unsupported("Cities pool"))
    );
}

#[test]
fn unreserved_destination_fails_before_mutating_the_sim() {
    let mut sim = Sim::new(9, 8);
    let old_row = sim.spawn_build(
        1,
        production::BuildData {
            flags: production::flag::VALID,
            city: 0,
            ..Default::default()
        },
    );
    let new_row = sim.spawn_build(
        2,
        production::BuildData {
            flags: production::flag::VALID,
            city: 0,
            ..Default::default()
        },
    );
    assert_eq!(sim.cities.alloc_slot(1), 0);
    sim.cities.slots[1][0] = tech_cities::CityRecord {
        city_flags: 1,
        city: 0,
        o: 2000,
        who: 1,
        ..Default::default()
    };
    let before_damage = sim.builds[new_row].damage;
    let request = CityRecordCaptureRequest {
        old_city: CityKey { who: 1, city: 0 },
        new_city: CityKey { who: 2, city: 0 },
        old_center: ObjectKey { who: 1, o: 2000 },
        new_center: ObjectKey { who: 2, o: 2000 },
    };
    assert_eq!(
        apply_sim_city_record_capture(&mut sim, request),
        Err(SimCityRecordCaptureError::NewCitySlotNotReserved)
    );
    assert_eq!(sim.builds[old_row].damage, 0);
    assert_eq!(sim.builds[new_row].damage, before_damage);
}

#[test]
fn prefix_counters_use_the_canonical_victory_leader_rows_and_wrap() {
    let mut sim = Sim::new(11, 8);
    let before_digest = sim.channel_digest();
    sim.vic_leaders.slots[2].cities_lost = i32::MAX;
    apply_sim_capture_counter(
        &mut sim,
        IncrementCaptureCounterRequest {
            who: 2,
            counter: CaptureCounter::CitiesLost,
            amount: 1,
        },
    )
    .unwrap();
    apply_sim_capture_counter(
        &mut sim,
        IncrementCaptureCounterRequest {
            who: 3,
            counter: CaptureCounter::CitiesCaptured,
            amount: 7,
        },
    )
    .unwrap();
    assert_eq!(sim.vic_leaders.slots[2].cities_lost, i32::MIN);
    assert_eq!(sim.vic_leaders.slots[3].cities_captured, 7);
    assert_ne!(sim.channel_digest(), before_digest);
    assert_eq!(
        apply_sim_capture_counter(
            &mut sim,
            IncrementCaptureCounterRequest {
                who: 8,
                counter: CaptureCounter::CitiesCaptured,
                amount: 1,
            },
        ),
        Err(SimCaptureCounterError::InvalidOwner)
    );
    assert_eq!(
        save_load::save_sim(&sim),
        Err(save_load::SaveError::Unsupported("City capture counters"))
    );
}
