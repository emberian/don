#[path = "../src/unit_init_location_deep_re.rs"]
mod location;
#[allow(dead_code)]
#[path = "../src/setup_place_unit_deep_re.rs"]
mod setup_place_unit_deep_re;

use don_sim::systems::graphics_turret::{ExtractedGuyGraphics, GraphicsProvenance};
use don_sim::systems::groups_guys::{sinx, UnitGuys, UnitTypeStats};
use don_sim::systems::unit_inctime::{SUPPORTED_RETAIL_EXE_SHA256, SUPPORTED_UNIT_GRAPHICS_SHA256};
use location::*;
use setup_place_unit_deep_re::{
    produce_unit_guy_init_prefix, GuyGraphicsInitReceipt, GuyInitPredicateFacts,
    StableUnitIdentity, UnitGuyInitInputs, UNIT_SET_NEW_LOCATION_VA, UNIT_UPDATE_GPIECE_VA,
};

fn provenance() -> GraphicsProvenance {
    GraphicsProvenance {
        executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
        installed_unit_graphics_sha256: SUPPORTED_UNIT_GRAPHICS_SHA256,
        coherent_capture: true,
    }
}

fn graphics(slot: usize, track_dx: i32, track_dy: i32) -> GuyGraphicsInitReceipt {
    GuyGraphicsInitReceipt {
        provenance: provenance(),
        extracted: ExtractedGuyGraphics {
            guy_num: slot as i8,
            gpiece: 500 + slot as i32,
            pivot_graph_name: None,
            track_dx,
            track_dy,
            turret_angles: [0; 4],
            des_turret_angles: [0; 4],
            node_flags: 0,
            des_node_flags: 0,
        },
        restriction_count: 0,
    }
}

fn predicates() -> GuyInitPredicateFacts {
    GuyInitPredicateFacts {
        valid_animation: [true; 4],
        ..GuyInitPredicateFacts::default()
    }
}

fn identity() -> StableUnitIdentity {
    StableUnitIdentity {
        id: 700,
        generation: 9,
        owner: 2,
        o: 17,
        type_index: 62,
    }
}

fn unit_type(squad_size: i32, crew_size: i32) -> UnitTypeStats {
    UnitTypeStats {
        domain: 0,
        guy_spacing: 192,
        new_block_radius: 3,
        squad_size,
        crew_size,
        ..UnitTypeStats::default()
    }
}

fn prefix(
    squad_size: i32,
    crew_size: i32,
    graphics: &[GuyGraphicsInitReceipt],
) -> setup_place_unit_deep_re::UnitGuyInitPrefixReceipt {
    produce_unit_guy_init_prefix(
        UnitGuyInitInputs {
            identity: identity(),
            squad_size,
            crew_size,
            graphics: graphics.to_vec(),
            predicates: vec![predicates(); (squad_size + crew_size) as usize],
        },
        12_345,
    )
    .unwrap()
}

fn unit_height(ordinal: u16, x: i32, y: i32, z: i32) -> TerrainHeightReceipt {
    TerrainHeightReceipt {
        ordinal,
        call_va: UNIT_TERRAIN_Z_CALL_VA,
        body_va: TERRAIN_FIND_TCOORD_Z_VA,
        kind: TerrainQueryKind::UnitTcoord,
        x,
        y,
        final_arg: 1,
        returned_z: z,
    }
}

fn guy_height(ordinal: u16, x: i32, y: i32, z: i32) -> TerrainHeightReceipt {
    TerrainHeightReceipt {
        ordinal,
        call_va: GUY_TERRAIN_Z_CALL_VA,
        body_va: TERRAIN_FIND_DATA_Z_VA,
        kind: TerrainQueryKind::GuyCoord,
        x,
        y,
        final_arg: 0,
        returned_z: z,
    }
}

fn clamp(v: i32, max: i32) -> i32 {
    v.clamp(0, max - 1)
}

fn crew_destination(
    base: (i32, i32),
    angle: i32,
    track: (i32, i32),
    world: (i32, i32),
) -> (i32, i32) {
    let (dx, dy) = track;
    let mut x = base.0;
    let mut y = base.1;
    if dx != 0 {
        x = x.wrapping_add(sinx(angle.wrapping_add(0x4000_0000), dx));
        y = y.wrapping_add(sinx(angle, dx));
    }
    if dy != 0 {
        x = x.wrapping_add(sinx(angle.wrapping_add(i32::MIN), dy));
        y = y.wrapping_add(sinx(angle.wrapping_add(0x4000_0000), dy));
    }
    if dx != 0 || dy != 0 {
        x = clamp(x, world.0);
        y = clamp(y, world.1);
    }
    (x, y)
}

#[test]
fn native_extents_and_call_sites_are_frozen() {
    assert_eq!(
        (
            UNIT_INIT_LOCATION_CONTINUATION_BEGIN_VA,
            UNIT_INIT_LOCATION_CONTINUATION_END_VA,
            UNIT_UPDATE_GPIECE_VA,
            UNIT_UPDATE_GPIECE_BYTES,
            UNIT_SET_NEW_LOCATION_VA,
            UNIT_SET_NEW_LOCATION_BYTES,
        ),
        (
            0x0061_2cc1,
            0x0061_2cd9,
            0x005e_2920,
            138,
            0x005f_8d20,
            1_757,
        )
    );
    assert_eq!(
        (
            GUY_UPDATE_GPIECE_VA,
            GUY_UPDATE_GPIECE_BYTES,
            GUY_SET_ANGLE_VA,
            GUY_SET_ANGLE_BYTES,
            GUY_SET_NEW_LOCATION_VA,
            GUY_SET_NEW_LOCATION_BYTES,
        ),
        (0x005d_8530, 363, 0x005d_9010, 550, 0x005d_86f0, 899,)
    );
    assert_eq!(UNIT_INIT_SET_ANGLE, 0x5555_5555);
}

#[test]
fn single_squad_guy_runs_both_recursive_crew_waves_in_native_order() {
    let anchor = (10 * 48 + 24, 11 * 48 + 24);
    let world = (32 * 0x300, 32 * 0x300);
    let graphics = vec![graphics(0, 0, 0), graphics(1, 192, 0), graphics(2, 0, 192)];
    let angle = UNIT_INIT_SET_ANGLE;
    let pre1 = crew_destination((-1_536, -1_536), angle, (192, 0), world);
    let pre2 = crew_destination((-1_536, -1_536), angle, (0, 192), world);
    let final1 = crew_destination(anchor, angle, (192, 0), world);
    let final2 = crew_destination(anchor, angle, (0, 192), world);
    let terrain = vec![
        unit_height(0, anchor.0 / 192, anchor.1 / 192, 100),
        guy_height(1, pre1.0, pre1.1, 201),
        guy_height(2, pre2.0, pre2.1, 202),
        guy_height(3, anchor.0, anchor.1, 300),
        guy_height(4, final1.0, final1.1, 401),
        guy_height(5, final2.0, final2.1, 402),
    ];
    let receipt = produce_unit_init_location_continuation(UnitInitLocationInputs {
        prefix: prefix(1, 2, &graphics),
        graphics,
        unit_type: unit_type(1, 2),
        formation: 0,
        unit_masks: 0,
        domain_two_tracks_ground: false,
        anchor_x: anchor.0,
        anchor_y: anchor.1,
        world_max_x: world.0,
        world_max_y: world.1,
        terrain,
    })
    .unwrap();

    assert_eq!(receipt.rng_before, receipt.rng_after);
    assert_eq!(
        (receipt.unit.x, receipt.unit.y, receipt.unit.z),
        (anchor.0, anchor.1, 100)
    );
    assert_eq!(
        (receipt.unit.moved_wcoord, receipt.unit.moved_tcoord),
        (false, false)
    );
    assert_eq!(receipt.graphics_calls.len(), 3);
    assert!(receipt
        .graphics_calls
        .iter()
        .all(|call| call.presentation_hint_reset));
    assert_eq!(
        receipt.graphics_calls[0].caller_va,
        UNIT_UPDATE_SQUAD_GUY_CALL_VA
    );
    assert!(receipt.graphics_calls[1..]
        .iter()
        .all(|call| call.caller_va == UNIT_UPDATE_CREW_GUY_CALL_VA));

    let guys = &receipt.guys.guys;
    let g0 = guys[0].as_ref().unwrap();
    let g1 = guys[1].as_ref().unwrap();
    let g2 = guys[2].as_ref().unwrap();
    assert_eq!((g0.x, g0.y, g0.z), (anchor.0, anchor.1, 300));
    assert_eq!((g1.x, g1.y, g1.z), (final1.0, final1.1, 401));
    assert_eq!((g2.x, g2.y, g2.z), (final2.0, final2.1, 402));
    assert_eq!((g1.track_dx, g1.track_dy), (192, 0));
    assert_eq!((g2.track_dx, g2.track_dy), (0, 192));
    for guy in guys.iter().flatten() {
        assert_eq!(
            (guy.angle, guy.last_angle, guy.des_angle),
            (angle, angle, angle)
        );
        assert_eq!((guy.x, guy.y, guy.z), (guy.last_x, guy.last_y, guy.last_z));
    }

    let phases = receipt
        .location_calls
        .iter()
        .filter(|call| call.kind == GuyCallKind::SetNewLocation)
        .map(|call| (call.slot, call.phase.unwrap(), call.caller_va))
        .collect::<Vec<_>>();
    assert_eq!(
        phases,
        vec![
            (
                1,
                GuyLocationPhase::CrewFromSetAngle,
                SET_ANGLE_CREW_LOCATION_CALL_VA
            ),
            (
                2,
                GuyLocationPhase::CrewFromSetAngle,
                SET_ANGLE_CREW_LOCATION_CALL_VA
            ),
            (
                0,
                GuyLocationPhase::SquadFinal,
                UNIT_SINGLE_GUY_SET_LOCATION_CALL_VA
            ),
            (
                1,
                GuyLocationPhase::CrewFromSetLocation,
                SET_LOCATION_CREW_LOCATION_CALL_VA,
            ),
            (
                2,
                GuyLocationPhase::CrewFromSetLocation,
                SET_LOCATION_CREW_LOCATION_CALL_VA,
            ),
        ]
    );
    assert_eq!(receipt.collision_requests.len(), 1);
    assert_eq!(receipt.collision_requests[0].slot, 0);
    assert_eq!(
        receipt.first_unapplied_shared_mutation,
        Some(receipt.collision_requests[0])
    );
    assert_eq!(
        receipt.next_external_residual,
        UnitInitLocationExternalResidual::UnitInitPostLocationStores {
            next_va: 0x0061_2cd9
        }
    );
}

#[test]
fn multi_squad_lattice_places_squad_after_guy_zero_crew_recursion() {
    let anchor = (20 * 48 + 24, 20 * 48 + 24);
    let world = (64 * 0x300, 64 * 0x300);
    let graphics = vec![
        graphics(0, 0, 0),
        graphics(1, 0, 0),
        graphics(2, 0, 0),
        graphics(3, 96, -48),
    ];
    let ut = unit_type(3, 1);
    let expected_squad = UnitGuys::initial_squad_locations(
        3,
        anchor.0,
        anchor.1,
        UNIT_INIT_SET_ANGLE,
        5,
        2,
        world.0,
        world.1,
        &ut,
    );
    let pre_crew = crew_destination((-1_536, -1_536), UNIT_INIT_SET_ANGLE, (96, -48), world);
    let final_crew = crew_destination(expected_squad[0], UNIT_INIT_SET_ANGLE, (96, -48), world);
    let terrain = vec![
        unit_height(0, anchor.0 / 192, anchor.1 / 192, 10),
        guy_height(1, pre_crew.0, pre_crew.1, 11),
        guy_height(2, expected_squad[0].0, expected_squad[0].1, 12),
        guy_height(3, final_crew.0, final_crew.1, 13),
        guy_height(4, expected_squad[1].0, expected_squad[1].1, 14),
        guy_height(5, expected_squad[2].0, expected_squad[2].1, 15),
    ];
    let receipt = produce_unit_init_location_continuation(UnitInitLocationInputs {
        prefix: prefix(3, 1, &graphics),
        graphics,
        unit_type: ut,
        formation: 5,
        unit_masks: 2,
        domain_two_tracks_ground: false,
        anchor_x: anchor.0,
        anchor_y: anchor.1,
        world_max_x: world.0,
        world_max_y: world.1,
        terrain,
    })
    .unwrap();

    for (slot, expected) in expected_squad.iter().enumerate() {
        let guy = receipt.guys.guys[slot].as_ref().unwrap();
        assert_eq!((guy.x, guy.y), *expected);
    }
    let crew = receipt.guys.guys[3].as_ref().unwrap();
    assert_eq!((crew.x, crew.y, crew.z), (final_crew.0, final_crew.1, 13));
    assert_eq!(receipt.collision_requests.len(), 3);
    assert_eq!(
        receipt
            .collision_requests
            .iter()
            .map(|request| request.slot)
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert!(receipt
        .location_calls
        .iter()
        .filter(|call| call.kind == GuyCallKind::SetAngle
            && call.phase == Some(GuyLocationPhase::SquadFinal))
        .all(|call| call.caller_va == UNIT_LATTICE_SET_ANGLE_CALL_VA));
}

#[test]
fn sea_domain_needs_only_the_unit_height_receipt_but_still_stamps_squad_collision() {
    let anchor = (24, 24);
    let graphics = vec![graphics(0, 0, 0), graphics(1, 0, 0)];
    let mut ut = unit_type(2, 0);
    ut.domain = 1;
    let receipt = produce_unit_init_location_continuation(UnitInitLocationInputs {
        prefix: prefix(2, 0, &graphics),
        graphics,
        unit_type: ut,
        formation: 0,
        unit_masks: 0,
        domain_two_tracks_ground: false,
        anchor_x: anchor.0,
        anchor_y: anchor.1,
        world_max_x: 8 * 0x300,
        world_max_y: 8 * 0x300,
        terrain: vec![unit_height(0, 0, 0, 55)],
    })
    .unwrap();
    assert_eq!(receipt.collision_requests.len(), 2);
    assert!(receipt.guys.guys.iter().flatten().all(|guy| guy.z == 0));
}

#[test]
fn air_domain_climbs_thirty_per_recursive_wave_and_suppresses_collision() {
    let anchor = (24, 24);
    let graphics = vec![graphics(0, 0, 0), graphics(1, 0, 0)];
    let mut ut = unit_type(1, 1);
    ut.domain = 2;
    let terrain = vec![
        unit_height(0, 0, 0, 5),
        // Zero track offsets intentionally preserve the unclamped pre-wave sentinel.
        guy_height(1, -1_536, -1_536, 100),
        guy_height(2, 24, 24, 200),
        guy_height(3, 24, 24, 300),
    ];
    let receipt = produce_unit_init_location_continuation(UnitInitLocationInputs {
        prefix: prefix(1, 1, &graphics),
        graphics,
        unit_type: ut,
        formation: 0,
        unit_masks: 0,
        domain_two_tracks_ground: true,
        anchor_x: anchor.0,
        anchor_y: anchor.1,
        world_max_x: 8 * 0x300,
        world_max_y: 8 * 0x300,
        terrain,
    })
    .unwrap();
    assert!(receipt.collision_requests.is_empty());
    let squad = receipt.guys.guys[0].as_ref().unwrap();
    let crew = receipt.guys.guys[1].as_ref().unwrap();
    assert_eq!(squad.z, 30);
    assert_eq!(crew.z, 60, "crew climbed once in each recursive wave");
}

#[test]
fn terrain_receipts_are_chronological_and_fail_closed() {
    let graphics = vec![graphics(0, 0, 0)];
    let error = produce_unit_init_location_continuation(UnitInitLocationInputs {
        prefix: prefix(1, 0, &graphics),
        graphics,
        unit_type: unit_type(1, 0),
        formation: 0,
        unit_masks: 0,
        domain_two_tracks_ground: false,
        anchor_x: 24,
        anchor_y: 24,
        world_max_x: 0x300,
        world_max_y: 0x300,
        terrain: vec![unit_height(0, 0, 0, 5), guy_height(7, 24, 24, 6)],
    })
    .unwrap_err();
    assert_eq!(
        error,
        UnitInitLocationError::TerrainReceiptMismatch { ordinal: 1 }
    );
}
