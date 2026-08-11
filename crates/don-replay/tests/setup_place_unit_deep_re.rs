#[path = "../src/setup_place_unit_deep_re.rs"]
mod deep;

use deep::*;
use don_sim::rng::Random;
use don_sim::systems::graphics_turret::{ExtractedGuyGraphics, GraphicsProvenance};
use don_sim::systems::unit_inctime::{SUPPORTED_RETAIL_EXE_SHA256, SUPPORTED_UNIT_GRAPHICS_SHA256};

fn tile() -> PlacementTileFacts {
    PlacementTileFacts {
        continent: 7,
        flags: 0,
        land: 0,
        occupied_o: -1,
        collision: 0,
    }
}

fn map(xs: i32, ys: i32) -> PlacementMapSnapshot {
    PlacementMapSnapshot {
        xs,
        ys,
        tiles: vec![tile(); (xs * ys) as usize],
    }
}

fn center(owner: i32) -> CenterBuildFacts {
    CenterBuildFacts {
        owner,
        o: 2_003,
        x: 10 * 0x300 + 0x180,
        y: 11 * 0x300 + 0x180,
    }
}

fn place_inputs(with_center: bool) -> PlaceUnitInputs {
    PlaceUnitInputs {
        owner: 2,
        upgraded_type: 69,
        requested_x: 4 * 0x300 + 0x180,
        requested_y: 5 * 0x300 + 0x180,
        center: with_center.then(|| center(2)),
        starting_town: 1,
        leader_active: 1,
    }
}

#[test]
fn native_extents_and_runtime_circle_tables_are_frozen() {
    assert_eq!(
        (SETUP_PLACE_UNIT_VA, SETUP_PLACE_UNIT_BYTES),
        (0x005a_bca0, 749)
    );
    assert_eq!((CIRCLE_INIT_VA, CIRCLE_INIT_BYTES), (0x0068_17f0, 295));
    assert_eq!(
        (CIRCLE_X_VA, CIRCLE_Y_VA, CIRCLE_END_VA),
        (0x00cb_7e90, 0x00cb_b0e0, 0x00cb_e330)
    );
    assert_eq!((UNIT_INIT_VA, UNIT_INIT_BYTES), (0x0061_2100, 3_732));
    assert_eq!(
        (
            UNIT_GUY_ALLOCATION_BEGIN_VA,
            UNIT_GUY_INIT_LOOP_BEGIN_VA,
            UNIT_GUY_INIT_LOOP_END_VA,
        ),
        (0x0061_2a7a, 0x0061_2c2f, 0x0061_2cc1)
    );
    assert_eq!((GUY_CLEAR_VA, GUY_CLEAR_BYTES), (0x005d_b590, 276));
    assert_eq!(
        (GUY_INIT_REAL_VA, GUY_INIT_REAL_BYTES),
        (0x005d_b6b0, 1_442)
    );
    assert_eq!(
        (
            BUILD_VTABLE_VA,
            BUILD_GET_BUILD_SLOT,
            BUILD_GET_BUILD_CALL_VA,
            BUILD_GET_BUILD_TARGET_VA,
            BUILD_TRAIN_CALL_VA,
            BUILD_TRAIN_VA,
        ),
        (
            0x00b4_2174,
            0xac,
            0x005a_bf2f,
            0x0041_c000,
            0x005a_bf3a,
            0x0062_f9b0,
        )
    );

    let circles = build_circle_tables();
    assert_eq!(circles.end(0), Some(1));
    assert_eq!(circles.end(1), Some(9));
    assert_eq!(circles.end(2), Some(21));
    assert_eq!(circles.end(8), Some(237));
    assert_eq!(circles.end(24), Some(1_905));
    assert_eq!(circles.end(63), Some(12_573));
    assert_eq!(circles.end(64), Some(CIRCLE_STORAGE_LIMIT));
    assert_eq!(circles.offsets().len(), CIRCLE_STORAGE_LIMIT);
    assert_eq!(
        &circles.offsets()[..9],
        &[
            (0, 0),
            (-1, -1),
            (-1, 0),
            (-1, 1),
            (0, -1),
            (0, 1),
            (1, -1),
            (1, 0),
            (1, 1),
        ]
    );
    assert_eq!(circles.offsets().last(), Some(&(46, -41)));
}

#[test]
fn accepted_probe_draws_before_world_gates_and_returns_exact_init_request() {
    let inputs = place_inputs(true);
    let receipt = produce_place_unit_probe_prefix(inputs, &map(32, 32), 0x12345).unwrap();
    assert_eq!((receipt.anchor_tile, receipt.radius), ((10, 11), 2));
    assert_eq!((receipt.circle_count, receipt.attempt_limit), (21, 60));
    assert_eq!(receipt.probes.len(), 1);
    let probe = receipt.probes[0];
    assert_eq!(probe.ordinal, 0);
    assert_eq!(probe.draw.call_va, PLACE_UNIT_RANDOM_CALL_VA);
    assert_eq!(probe.disposition, ProbeDisposition::Accepted);
    assert_eq!(
        probe.candidate_tile,
        (
            receipt.anchor_tile.0 + i32::from(probe.offset.0),
            receipt.anchor_tile.1 + i32::from(probe.offset.1),
        )
    );
    let mut rng = Random::new(0x12345);
    assert_eq!(probe.draw.returned, rng.get(0, 0xffff));
    assert_eq!(probe.draw.state_after, rng.state());
    assert_eq!(receipt.rng_after_probes, rng.state());

    let PlaceUnitExternalResidual::ObjectsInitUnit(call) = receipt.first_external_residual else {
        panic!("accepted candidate must reach Objects::init_unit")
    };
    assert_eq!(
        (call.call_va, call.body_va),
        (OBJECTS_INIT_UNIT_THUNK_VA, OBJECTS_INIT_UNIT_VA)
    );
    assert_eq!((call.owner, call.type_index), (2, 69));
    assert_eq!(
        (call.exact_o, call.external_previous, call.external_next),
        (-1, -1, -1)
    );
    assert_eq!(
        (call.x, call.y),
        (
            probe.candidate_tile.0 * 0x300 + 0x180,
            probe.candidate_tile.1 * 0x300 + 0x180,
        )
    );
    assert!(!call.come_out_zero_after_success);
}

#[test]
fn rejection_precedence_and_consumed_draw_survive_a_later_success() {
    let inputs = place_inputs(true);
    let valid = produce_place_unit_probe_prefix(inputs, &map(32, 32), 42).unwrap();
    let first = valid.probes[0].candidate_tile;
    let mut obstructed = map(32, 32);
    let index = (first.1 * obstructed.xs + first.0) as usize;
    obstructed.tiles[index].continent = 99;
    obstructed.tiles[index].flags = 0x8130;
    obstructed.tiles[index].occupied_o = 7;
    obstructed.tiles[index].collision = 0x4003;

    let receipt = produce_place_unit_probe_prefix(inputs, &obstructed, 42).unwrap();
    assert!(receipt.probes.len() >= 2);
    assert_eq!(
        receipt.probes[0].disposition,
        ProbeDisposition::Rejected(ProbeRejection::DifferentContinent),
        "the native continent gate precedes every obstructed-tile flag gate"
    );
    assert_eq!(
        receipt.probes[1].draw.state_before, receipt.probes[0].draw.state_after,
        "a rejected coordinate still charges its direct draw"
    );
}

#[test]
fn sixty_failed_starting_town_probes_queue_on_the_center_build() {
    let inputs = place_inputs(true);
    let mut blocked = map(32, 32);
    for tile in &mut blocked.tiles {
        tile.flags = 0x30;
    }
    let receipt = produce_place_unit_probe_prefix(inputs, &blocked, -17).unwrap();
    assert_eq!((receipt.radius, receipt.circle_count), (2, 21));
    assert_eq!(receipt.probes.len(), 60);
    assert!(receipt
        .probes
        .iter()
        .all(|p| { p.disposition == ProbeDisposition::Rejected(ProbeRejection::WDataFlags30) }));
    let mut rng = Random::new(-17);
    for _ in 0..60 {
        rng.get(0, 0xffff);
    }
    assert_eq!(receipt.rng_after_probes, rng.state());
    assert_eq!(
        receipt.first_external_residual,
        PlaceUnitExternalResidual::BuildTrain(BuildTrainRequest {
            receiver_vtable_va: BUILD_VTABLE_VA,
            receiver_get_build_slot: BUILD_GET_BUILD_SLOT,
            receiver_get_build_call_va: BUILD_GET_BUILD_CALL_VA,
            receiver_get_build_target_va: BUILD_GET_BUILD_TARGET_VA,
            train_call_va: BUILD_TRAIN_CALL_VA,
            body_va: BUILD_TRAIN_VA,
            center: center(2),
            type_index: 69,
        })
    );
}

#[test]
fn cityless_exhaustion_preserves_the_distinct_come_out_tail() {
    let mut inputs = place_inputs(false);
    inputs.starting_town = 1;
    let mut blocked = map(32, 32);
    for tile in &mut blocked.tiles {
        tile.occupied_o = 1;
    }
    let receipt = produce_place_unit_probe_prefix(inputs, &blocked, 9).unwrap();
    let PlaceUnitExternalResidual::ObjectsInitUnit(call) = receipt.first_external_residual else {
        panic!("cityless exhaustion must retry at the requested coordinate")
    };
    assert_eq!(call.call_va, OBJECTS_INIT_UNIT_VA);
    assert_eq!((call.x, call.y), (inputs.requested_x, inputs.requested_y));
    assert!(call.come_out_zero_after_success);
    assert_eq!(UNIT_COME_OUT_VA, 0x0061_7c10);
}

fn provenance() -> GraphicsProvenance {
    GraphicsProvenance {
        executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
        installed_unit_graphics_sha256: SUPPORTED_UNIT_GRAPHICS_SHA256,
        coherent_capture: true,
    }
}

fn graphics(slot: usize, restrictions: u8) -> GuyGraphicsInitReceipt {
    let node_flags = if restrictions == 0 { 0 } else { 1 };
    GuyGraphicsInitReceipt {
        provenance: provenance(),
        extracted: ExtractedGuyGraphics {
            guy_num: slot as i8,
            gpiece: 100 + slot as i32,
            pivot_graph_name: (restrictions != 0).then(|| "ExactHierarchy".to_owned()),
            track_dx: 300 + slot as i32,
            track_dy: -400 - slot as i32,
            turret_angles: if restrictions == 0 {
                [0; 4]
            } else {
                [11, 22, 0, 0]
            },
            des_turret_angles: if restrictions == 0 {
                [0; 4]
            } else {
                [33, 44, 0, 0]
            },
            node_flags,
            des_node_flags: 0,
        },
        restriction_count: restrictions,
    }
}

fn predicates(restrictions: bool) -> GuyInitPredicateFacts {
    GuyInitPredicateFacts {
        valid_animation: [true, false, false, true],
        has_pivot_restrictions: restrictions,
        animation_22_loaded: false,
        unit_flags2_bit_4: false,
        type_is_0x20: true,
        type_is_0x1000: false,
        unit_flags_bit_0x10: false,
        unit_flags_bit_0x2: false,
        air_predicate: false,
        base_type_is_52_or_53: false,
    }
}

#[test]
fn new_unit_guy_prefix_allocates_exact_array_and_consumes_one_draw_per_slot() {
    let identity = StableUnitIdentity {
        id: 700,
        generation: 9,
        owner: 2,
        o: 17,
        type_index: 62,
    };
    let receipt = produce_unit_guy_init_prefix(
        UnitGuyInitInputs {
            identity,
            squad_size: 1,
            crew_size: 2,
            graphics: vec![graphics(0, 2), graphics(1, 0), graphics(2, 0)],
            predicates: vec![predicates(true), predicates(false), predicates(false)],
        },
        12345,
    )
    .unwrap();

    assert_eq!(
        (
            receipt.array_length,
            receipt.array_capacity,
            receipt.array_increment,
            receipt.array_flags,
            receipt.guy_mark,
        ),
        (3, 3, 1, 0, 1)
    );
    assert_eq!(receipt.guys.len(), 3);
    assert!(receipt.guys.guys.iter().all(Option::is_some));
    assert_eq!(receipt.random.len(), 3);
    assert_eq!(receipt.stable_guys.len(), 3);

    let mut rng = Random::new(12345);
    for (slot, event) in receipt.random.iter().enumerate() {
        assert_eq!(event.slot, slot as i32);
        assert_eq!(event.state_before, rng.state());
        assert_eq!(event.returned, rng.get(0, 0xffff));
        assert_eq!(event.state_after, rng.state());
        let expected_before = match event.remainder {
            70..=79 => 1,
            80..=89 => 2,
            90..=99 => 3,
            _ => 0,
        };
        assert_eq!(event.selected_before_validation, expected_before);
        let expected_after = if [true, false, false, true][expected_before as usize] {
            expected_before
        } else {
            0
        };
        assert_eq!(event.selected_after_validation, expected_after);
    }
    assert_eq!(receipt.rng_after_guys, rng.state());

    for (slot, guy) in receipt.guys.guys.iter().flatten().enumerate() {
        assert_eq!(
            (guy.ty, guy.who, guy.o, guy.guy_num),
            (62, 2, 17, slot as i8)
        );
        assert_eq!((guy.x, guy.y, guy.last_time), (-1_536, -1_536, -1));
        assert_eq!((guy.ox, guy.whom), (-1, -1));
        assert_eq!((guy.stopped, guy.hold_attack, guy.queued_attack), (1, 0, 0));
        assert_eq!((guy.track_dx, guy.track_dy), (0, 0));
        assert_eq!(guy.gpiece, 100 + slot as i32);
        assert_eq!(guy.variation, 0);
    }
    assert_eq!(receipt.guys.guys[0].as_ref().unwrap().guy_flags, 0x110);
    assert_eq!(receipt.guys.guys[1].as_ref().unwrap().guy_flags, 0x10);
    assert_eq!(
        receipt.first_external_residual,
        UnitGuyExternalResidual::UpdateGpieceThenSetNewLocation {
            update_gpiece_va: UNIT_UPDATE_GPIECE_VA,
            set_new_location_va: UNIT_SET_NEW_LOCATION_VA,
        }
    );
}

#[test]
fn graphics_and_stable_identity_fail_closed_before_rng_is_observable() {
    let identity = StableUnitIdentity {
        id: 1,
        generation: 2,
        owner: 0,
        o: 0,
        type_index: 69,
    };
    let mut bad = graphics(0, 0);
    bad.provenance.coherent_capture = false;
    assert_eq!(
        produce_unit_guy_init_prefix(
            UnitGuyInitInputs {
                identity,
                squad_size: 1,
                crew_size: 0,
                graphics: vec![bad],
                predicates: vec![predicates(false)],
            },
            77,
        ),
        Err(UnitGuyInitError::IncoherentGraphicsCapture { slot: 0 })
    );

    let mut wrong = graphics(0, 0);
    wrong.extracted.guy_num = 1;
    assert_eq!(
        produce_unit_guy_init_prefix(
            UnitGuyInitInputs {
                identity,
                squad_size: 1,
                crew_size: 0,
                graphics: vec![wrong],
                predicates: vec![predicates(false)],
            },
            77,
        ),
        Err(UnitGuyInitError::WrongGraphicsSlot { slot: 0 })
    );
}
