use don_replay::cities_runtime::check_sim_owned_cities;
use don_replay::groups_pre_pair_unit_authority::{ReplayUnitTypeFacts, ReplayUnitTypeSpans};
use don_replay::initial::ReplayByteSpan;
use don_replay::setup_units_producer::{
    BuildUnitsPlan, BuildUnitsPrefixReceipt, DirectRandomDrawReceipt, EngineContainerShapeReceipt,
    InitUnitAuthorityReceipt, InitUnitRngSpan, PlaceUnitCall, PlaceUnitReceipt,
    PlacementOutcomeReceipt, PlacementRngEvent, StableUnitIdentityReceipt, StartingCitizenCounts,
    StartingUnitPhase, UnitMemberAuthorityReceipt, OBJECTS_INIT_UNIT_BYTES, OBJECTS_INIT_UNIT_VA,
    PLACE_UNIT_DIRECT_RANDOM_CALL_VA,
};
use don_replay::starting_city_unit_census::{
    apply_starting_city_unit_census, StartingCityUnitCensusAuthority, StartingCityUnitCensusError,
    StartingCityUnitDisposition, CITY_CENSUS_CLEAR_LOOP_BEGIN_VA, CITY_CENSUS_UNIT_LOOP_BEGIN_VA,
    LEADER_PLAN_STRATEGY_VA, OBJECTS_FIND_CITY_VA, UNIT_GET_ACTION_VA, VECTOR_DIST_VA,
};
use don_sim::objects::{Band, BUILD_BAND_BASE};
use don_sim::order::Order;
use don_sim::rng::Random;
use don_sim::systems::production::{self, BuildData};
use don_sim::systems::tech_cities::{CityPool, CityRecord};
use don_sim::tick::Sim;
use don_sim::world::Handle;

fn set_build_identity(build: &mut BuildData, object_id: u32, x: i32, y: i32) {
    build.other[production::off::OBJECT_ID..production::off::OBJECT_ID + 2]
        .copy_from_slice(&(object_id as i16).to_le_bytes());
    build.other[production::off::X_INTERNAL..production::off::X_INTERNAL + 4]
        .copy_from_slice(&(x ^ 0x63637).to_le_bytes());
    build.other[production::off::Y_INTERNAL..production::off::Y_INTERNAL + 4]
        .copy_from_slice(&(y ^ 0x63637).to_le_bytes());
}

fn install_center(sim: &mut Sim, owner: usize, x: i32, y: i32) {
    let object_id = BUILD_BAND_BASE + sim.world.objects.slot(owner).band(Band::Build).len() as u32;
    let mut build = BuildData {
        flags: production::flag::VALID | production::flag::ACTIVE,
        city: 0,
        ..BuildData::default()
    };
    set_build_identity(&mut build, object_id, x, y);
    let row = sim.spawn_build(owner, build);
    sim.production_runtime.register_build(row, 414);
    sim.cities = CityPool::new();
    sim.cities.slots[owner][0] = CityRecord {
        city_flags: 1,
        city: 0,
        o: object_id as i16,
        reg: 64,
        x,
        y,
        peasant_dist: -12,
        free: 9,
        busy: 8,
        gatherers: 7,
        ocean: 11,
        land: 22,
        filled: 33,
        who: owner as i8,
        ..CityRecord::default()
    };
    sim.cities.city_mark[owner] = 1;
}

fn type_facts(type_index: i32, control_cost: i32, offset: usize) -> ReplayUnitTypeFacts {
    ReplayUnitTypeFacts {
        spans: ReplayUnitTypeSpans {
            type_base: ReplayByteSpan { offset, bytes: 90 },
            object: ReplayByteSpan {
                offset: offset + 90,
                bytes: 152,
            },
            unit: ReplayByteSpan {
                offset: offset + 242,
                bytes: 792,
            },
        },
        type_index,
        from_type: -1,
        where_type: -1,
        upgrade: type_index,
        jump: type_index,
        obj_masks: 0,
        attack: 0,
        max_range: 0,
        domain: 0,
        guy_spacing: 0,
        x_spacing: 0,
        y_spacing: 0,
        new_block_radius: 0,
        graft: type_index,
        age: 0,
        unit_flags: 0,
        unit_flags2: 0,
        mode: 0,
        moves: 0,
        turn_speed: 0,
        role: 0,
        mana: 0,
        control_cost,
        military_level: 0,
        squad_size: 0,
        uber_size: 1,
        crew_size: 0,
        base_form: 0,
    }
}

fn call(ordinal: u32, phase: StartingUnitPhase, type_index: i32) -> PlaceUnitCall {
    PlaceUnitCall {
        ordinal,
        call_va: 0x005a_b116 + ordinal,
        phase,
        owner: 0,
        center_city_o: 2000,
        requested_x: 2_000,
        requested_y: 2_000,
        base_type: type_index,
        selected_before_upgrade: type_index,
        build_units_upgrade: type_index,
        place_unit_upgrade: type_index,
        uber_size: 1,
        squad_size: 0,
        crew_size: 0,
    }
}

fn setup_authority(handles: &[Handle]) -> StartingCityUnitCensusAuthority {
    let calls = vec![
        call(0, StartingUnitPhase::BaseScout, 69),
        call(1, StartingUnitPhase::Citizen { index: 0 }, 50),
        call(2, StartingUnitPhase::Citizen { index: 1 }, 50),
    ];
    let plan = BuildUnitsPlan {
        calls: calls.clone(),
        starting_citizens: StartingCitizenCounts {
            total: 2,
            fixed_building_prefix: 0,
        },
        citizens_after_modifiers: 2,
        stop: None,
    };
    let mut rng = 7i32;
    let mut placements = Vec::new();
    for (ordinal, (call, handle)) in calls.into_iter().zip(handles).enumerate() {
        let mut random = Random::new(rng);
        let returned = random.get(0, 0xffff);
        let after_draw = random.state();
        let identity = StableUnitIdentityReceipt {
            id: handle.id,
            generation: handle.generation,
            owner: 0,
            o: ordinal as i32,
        };
        placements.push(PlaceUnitReceipt {
            call,
            rng_before: rng,
            rng_after: after_draw,
            rng_events: vec![
                PlacementRngEvent::DirectOffset(DirectRandomDrawReceipt {
                    call_va: PLACE_UNIT_DIRECT_RANDOM_CALL_VA,
                    state_before: rng,
                    returned,
                    state_after: after_draw,
                }),
                PlacementRngEvent::InitUnit(InitUnitRngSpan {
                    body_va: OBJECTS_INIT_UNIT_VA,
                    body_bytes: OBJECTS_INIT_UNIT_BYTES,
                    state_before: after_draw,
                    state_after: after_draw,
                }),
            ],
            outcome: PlacementOutcomeReceipt::Spawned(InitUnitAuthorityReceipt {
                validated_body_va: OBJECTS_INIT_UNIT_VA,
                validated_body_bytes: OBJECTS_INIT_UNIT_BYTES,
                unit_mark_before: ordinal as i32,
                unit_mark_after: ordinal as i32 + 1,
                returned_captain_o: ordinal as i32,
                members: vec![UnitMemberAuthorityReceipt {
                    identity,
                    ptype_index: call.place_unit_upgrade,
                    launching_is_null: true,
                    path: EngineContainerShapeReceipt {
                        length: 0,
                        capacity: 10,
                        increment: -1,
                        flags: 0,
                    },
                    order_count: 0,
                    guys: EngineContainerShapeReceipt {
                        length: 0,
                        capacity: 0,
                        increment: -1,
                        flags: 0,
                    },
                    guy_mark: 0,
                    guy_identities: Vec::new(),
                    units_authority_key: (handle.id, handle.generation),
                    guys_authority_key: (handle.id, handle.generation),
                }],
            }),
        });
        rng = after_draw;
    }
    let receipt = BuildUnitsPrefixReceipt {
        rng_initial: 7,
        rng_final: rng,
        placements,
    };
    StartingCityUnitCensusAuthority::from_build_units(
        &[plan],
        &[receipt],
        &[type_facts(69, 1, 10), type_facts(50, 1, 2_000)],
    )
    .expect("validated setup allocation authority")
}

fn fixture() -> (Sim, StartingCityUnitCensusAuthority) {
    let mut sim = Sim::new(0x9137, 8);
    sim.activate(0);
    install_center(&mut sim, 0, 2_000, 2_000);
    let handles = [
        sim.spawn_unit(0, 69, 2_000, 2_000, 1).unwrap(),
        sim.spawn_unit(0, 50, 2_768, 2_000, 1).unwrap(),
        sim.spawn_unit(0, 50, 3_536, 2_000, 1).unwrap(),
    ];
    for handle in handles {
        let row = sim.world.row_of(handle).unwrap();
        sim.world.units.inside_up_mut()[row] = -1;
        sim.world.units.set_unit_masks(row, 0);
    }
    let authority = setup_authority(&handles);
    (sim, authority)
}

#[test]
fn retail_city_census_entry_points_are_frozen() {
    assert_eq!(LEADER_PLAN_STRATEGY_VA, 0x006b_9620);
    assert_eq!(CITY_CENSUS_CLEAR_LOOP_BEGIN_VA, 0x006b_9746);
    assert_eq!(CITY_CENSUS_UNIT_LOOP_BEGIN_VA, 0x006b_9e2c);
    assert_eq!(OBJECTS_FIND_CITY_VA, 0x0065_ba90);
    assert_eq!(UNIT_GET_ACTION_VA, 0x0060_8450);
    assert_eq!(VECTOR_DIST_VA, 0x0046_cff0);
}

#[test]
fn validated_scout_and_empty_action_citizens_drive_the_exact_city_bytes() {
    let (mut sim, authority) = fixture();
    let before_channel = check_sim_owned_cities(&sim).unwrap();
    let terrain_before = {
        let city = &sim.cities.slots[0][0];
        (city.ocean, city.land, city.filled)
    };

    let receipt = apply_starting_city_unit_census(&mut sim, &authority).unwrap();
    let city = &sim.cities.slots[0][0];
    assert_eq!(city.peasant_dist, 1);
    assert_eq!(city.free, 2);
    assert_eq!(city.busy, 0);
    assert_eq!(city.gatherers, 0);
    assert_eq!((city.ocean, city.land, city.filled), terrain_before);
    assert_eq!(receipt.owners_walked, 1);
    assert_eq!(receipt.cities_cleared, 1);
    assert_eq!(receipt.units_walked, 3);
    assert_eq!(receipt.scouts_joined, 1);
    assert_eq!(receipt.citizens_joined, 2);
    assert_eq!(receipt.free_citizens, 2);
    assert_eq!(receipt.busy_citizens, 0);
    assert_eq!(receipt.gatherers, 0);
    assert_eq!(receipt.city_assignments, 2);
    assert!(receipt.city_unit_census_complete);
    assert!(!receipt.first_checksum_city_image_ready);
    assert_eq!(receipt.world_writes, 0);
    assert_eq!(receipt.build_or_registry_writes, 0);
    assert_eq!(receipt.main_rng_draws, 0);
    assert_eq!(
        receipt.units[0].disposition,
        StartingCityUnitDisposition::NonCitizen
    );
    assert!(receipt.units[1..].iter().all(|unit| {
        unit.disposition == StartingCityUnitDisposition::FreeCitizenAtCity
            && unit.city_slot == Some(0)
    }));
    assert_ne!(
        check_sim_owned_cities(&sim).unwrap().checksum,
        before_channel.checksum
    );
}

#[test]
fn unsupported_citizen_order_refuses_without_leaking_the_clear_prefix() {
    let (mut sim, authority) = fixture();
    let citizen_row = sim.world.unit_row_at(0, 1).unwrap();
    sim.world
        .orders_mut(citizen_row)
        .replace(Order::move_to(4_000, 4_000, 0));
    let before = sim.cities.clone();

    assert!(matches!(
        apply_starting_city_unit_census(&mut sim, &authority),
        Err(StartingCityUnitCensusError::UnsupportedCitizenOrder { owner: 0, o: 1, .. })
    ));
    assert_eq!(sim.cities.slots, before.slots);
    assert_eq!(sim.cities.city_mark, before.city_mark);
}

#[test]
fn a_stale_setup_identity_refuses_before_any_city_mutation() {
    let (mut sim, authority) = fixture();
    let before = sim.cities.clone();
    assert!(sim.world.despawn(Handle {
        id: 1,
        generation: 0,
    }));

    assert!(apply_starting_city_unit_census(&mut sim, &authority).is_err());
    assert_eq!(sim.cities.slots, before.slots);
    assert_eq!(sim.cities.city_mark, before.city_mark);
}
