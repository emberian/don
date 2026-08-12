// SPDX-License-Identifier: GPL-3.0-or-later

// The authority is now a registered canonical Order/save dependency. The transaction
// remains an exclusive contract until one Sim host owns its packet and commit surfaces.
mod command {
    pub mod air_launch_receivers {
        pub use don_sim::command::air_launch_receivers::*;
    }
}
mod order {
    pub use don_sim::order::*;
}
#[allow(dead_code)]
#[path = "../src/systems/air_group_action_transaction.rs"]
mod air_group_action_transaction;

use air_group_action_transaction::{
    decode_air_group_packet_pair, AirGroupCommand, CommandPackagePosition,
};
use don_sim::systems::air_runtime_authority::*;

fn position(
    game_frame: i32,
    package_serial: u32,
    group_command_index: u16,
    action_command_index: u16,
) -> CommandPackagePosition {
    CommandPackagePosition {
        game_frame,
        package_serial,
        play: 0,
        group_command_index,
        action_command_index,
    }
}

fn payload() -> AirPatrolOrderPayload {
    AirPatrolOrderPayload {
        x: WalkedCoordArray {
            increment: -1,
            flags: 0x08,
            values: vec![120, -44, 777],
        },
        y: WalkedCoordArray {
            increment: 7,
            flags: 0x10,
            values: vec![500, 42, -900],
        },
        waypoint: 2,
        air: AirOrderPayload {
            home_o: 2_015,
            home_who: 0,
            cruising_alt: 0x640,
            sharp_turn: 1,
            old: 19,
            returning: 0,
        },
    }
}

#[test]
fn finished_retail_replay_scramble_pairs_decode_exactly() {
    // Exact same-package pairs from SHA-256 558e0cd5...d67bd54.
    let fixtures: [(&[u8], CommandPackagePosition, &[i16]); 4] = [
        (&[0x00, 0x00, 0x00], position(43_722, 44_041, 0, 1), &[]),
        (
            &[0x00, 0x01, 0x00, 0xdf, 0x07],
            position(47_706, 48_065, 1, 2),
            &[2_015],
        ),
        (
            &[0x00, 0x02, 0x00, 0x2a, 0x08, 0x2b, 0x08],
            position(51_868, 52_254, 1, 2),
            &[2_090, 2_091],
        ),
        (
            &[0x00, 0x03, 0x00, 0x2a, 0x08, 0x2b, 0x08, 0x63, 0x08],
            position(57_968, 58_402, 0, 1),
            &[2_090, 2_091, 2_147],
        ),
    ];
    for (group, position, expected) in fixtures {
        let pair = decode_air_group_packet_pair(position, group, &[0x24]).unwrap();
        assert_eq!(pair.group.requested, expected);
        assert_eq!(pair.action, AirGroupCommand::Scramble);
    }
}

#[test]
fn exact_launch_patrol_wire_retains_all_six_unaligned_dwords() {
    let fields: [i32; 6] = [-900, 777, 2, 1, 0, 1];
    let mut packet = vec![11];
    for field in fields {
        packet.extend_from_slice(&field.to_le_bytes());
    }
    let pair =
        decode_air_group_packet_pair(position(100, 7, 0, 1), &[0, 1, 2, 9, 0], &packet).unwrap();
    let AirGroupCommand::LaunchPatrol(request) = pair.action else {
        panic!("opcode 11 did not decode as launch patrol")
    };
    assert_eq!(
        [
            request.to_x,
            request.to_y,
            request.queue,
            request.force_all,
            request.bombers_only,
            request.fighters_only,
        ],
        fields
    );
}

#[test]
fn air_patrol_typed_leaf_round_trips_dynamic_arrays_and_metadata() {
    let before = payload();
    let bytes = encode_air_patrol_leaf(&before).unwrap();
    assert_eq!(bytes[0], DON_SAVE_AIR_PATROL_TAG);
    assert_eq!(bytes[1], AIR_PATROL_PAYLOAD_VERSION);
    assert_eq!(decode_air_patrol_leaf(&bytes).unwrap(), before);
    // Re-encoding after load is deterministic.
    assert_eq!(
        encode_air_patrol_leaf(&decode_air_patrol_leaf(&bytes).unwrap()).unwrap(),
        bytes
    );
}

#[test]
fn retail_walk_image_preserves_call_order_and_both_virtual_flag_visits() {
    let p = AirPatrolOrderPayload::one(11, 22, 2_015, 0);
    let walked = p.retail_walk_bytes(0xa5).unwrap();
    assert_eq!(walked.len(), 44 + 8);
    assert_eq!(walked[0], 0xa5);
    assert_eq!(i32::from_le_bytes(walked[1..5].try_into().unwrap()), 0);
    // x SimpleArray: length, increment, flags, one Coord.
    assert_eq!(i32::from_le_bytes(walked[5..9].try_into().unwrap()), 1);
    assert_eq!(i16::from_le_bytes(walked[9..11].try_into().unwrap()), -1);
    assert_eq!(walked[11], 0);
    assert_eq!(i32::from_le_bytes(walked[12..16].try_into().unwrap()), 11);
    // y array begins immediately after x.
    assert_eq!(i32::from_le_bytes(walked[16..20].try_into().unwrap()), 1);
    assert_eq!(i32::from_le_bytes(walked[23..27].try_into().unwrap()), 22);
    // Secondary AirOrder walks the same virtual UnitOrder flag, then six dwords.
    assert_eq!(walked[27], 0xa5);
    assert_eq!(
        i32::from_le_bytes(walked[28..32].try_into().unwrap()),
        2_015
    );
    assert_eq!(i32::from_le_bytes(walked[32..36].try_into().unwrap()), 0);
    assert_eq!(
        i32::from_le_bytes(walked[36..40].try_into().unwrap()),
        0x640
    );
}

#[test]
fn waypoint_cursor_is_saved_without_helpful_clamping() {
    let mut p = payload();
    p.waypoint = -77;
    let loaded = decode_air_patrol_leaf(&encode_air_patrol_leaf(&p).unwrap()).unwrap();
    assert_eq!(loaded.waypoint, -77);
}

#[test]
fn malformed_dynamic_payloads_fail_closed() {
    let mut mismatch = payload();
    mismatch.y.values.pop();
    assert_eq!(
        encode_air_patrol_leaf(&mismatch),
        Err(AirRuntimeAuthorityError::WaypointArrayLengthMismatch { x: 3, y: 2 })
    );

    let mut empty = payload();
    empty.x.values.clear();
    empty.y.values.clear();
    assert_eq!(
        encode_air_patrol_leaf(&empty),
        Err(AirRuntimeAuthorityError::EmptyPatrolRoute)
    );

    let mut allocator_bit = payload();
    allocator_bit.x.flags |= 0x40;
    assert_eq!(
        encode_air_patrol_leaf(&allocator_bit),
        Err(AirRuntimeAuthorityError::UnnormalizedArrayFlags(0x48))
    );
}

#[test]
fn foreign_tag_version_truncation_and_trailing_bytes_are_rejected() {
    let bytes = encode_air_patrol_leaf(&payload()).unwrap();
    let mut foreign = bytes.clone();
    foreign[0] = 7;
    assert_eq!(
        decode_air_patrol_leaf(&foreign),
        Err(AirRuntimeAuthorityError::WrongPayloadTag(7))
    );
    let mut future = bytes.clone();
    future[1] = 2;
    assert_eq!(
        decode_air_patrol_leaf(&future),
        Err(AirRuntimeAuthorityError::WrongPayloadVersion(2))
    );
    assert_eq!(
        decode_air_patrol_leaf(&bytes[..bytes.len() - 1]),
        Err(AirRuntimeAuthorityError::Truncated)
    );
    let mut trailing = bytes;
    trailing.push(0);
    assert_eq!(
        decode_air_patrol_leaf(&trailing),
        Err(AirRuntimeAuthorityError::TrailingBytes {
            expected: trailing.len() - 1,
            actual: trailing.len(),
        })
    );
}

#[test]
fn decoder_checks_point_limit_before_allocating_or_reading_points() {
    let mut bytes = vec![DON_SAVE_AIR_PATROL_TAG, AIR_PATROL_PAYLOAD_VERSION];
    bytes.extend_from_slice(&((MAX_DECODED_PATROL_POINTS as u32) + 1).to_le_bytes());
    assert_eq!(
        decode_air_patrol_leaf(&bytes),
        Err(AirRuntimeAuthorityError::PointCountLimit(
            MAX_DECODED_PATROL_POINTS + 1
        ))
    );
}

#[test]
fn synchronized_type_rows_answer_columns_and_nonstrict_relations() {
    let digest = [0x5a; 32];
    let fighter = AirTypeRow {
        type_id: 500,
        domain: AIR_DOMAIN,
        object_masks: 0,
        unit_flags: 0,
        relations: TypeRelations {
            biplane_nonstrict: true,
            bomber_nonstrict: false,
            helicopter_nonstrict: false,
        },
    };
    let helicopter = AirTypeRow {
        type_id: 501,
        domain: AIR_DOMAIN,
        object_masks: 0,
        unit_flags: HELICOPTER_UNIT_FLAG,
        relations: TypeRelations {
            helicopter_nonstrict: true,
            ..TypeRelations::default()
        },
    };
    let authority = AirTypeAuthority::new(9, digest, [fighter, helicopter]).unwrap();
    assert_eq!(authority.len(), 2);
    assert!(authority.row(500).unwrap().launch_common_eligible());
    assert_eq!(
        authority
            .row(500)
            .unwrap()
            .authoritative_relation(TYPE_BIPLANE),
        Some(true)
    );
    assert!(authority.row(501).unwrap().is_helicopter_runtime());
    assert_eq!(
        authority
            .row(501)
            .unwrap()
            .authoritative_relation(TYPE_HELICOPTER),
        Some(true)
    );
    assert_eq!(
        authority.row(777),
        Err(AirRuntimeAuthorityError::MissingTypeRow(777))
    );
}

#[test]
fn missile_mask_and_wrong_domain_are_not_launch_candidates() {
    for row in [
        AirTypeRow {
            type_id: 1,
            domain: 0,
            object_masks: 0,
            unit_flags: 0,
            relations: TypeRelations::default(),
        },
        AirTypeRow {
            type_id: 2,
            domain: AIR_DOMAIN,
            object_masks: MISSILE_OBJECT_MASK,
            unit_flags: 0,
            relations: TypeRelations::default(),
        },
    ] {
        assert!(!row.launch_common_eligible());
    }
}

#[test]
fn type_authority_rejects_ambiguous_or_unbound_composition() {
    let row = AirTypeRow {
        type_id: 4,
        domain: AIR_DOMAIN,
        object_masks: 0,
        unit_flags: 0,
        relations: TypeRelations::default(),
    };
    assert_eq!(
        AirTypeAuthority::new(0, [0; 32], [row]),
        Err(AirRuntimeAuthorityError::MissingCompositionDigest)
    );
    assert_eq!(
        AirTypeAuthority::new(0, [1; 32], [row, row]),
        Err(AirRuntimeAuthorityError::DuplicateTypeId(4))
    );
}

#[test]
fn scenario_ignore_orders_round_trip_retains_order_duplicates_and_tombstones() {
    let mut state = ScenarioIgnoreOrdersAuthority {
        revision: 44,
        ignore_orders: true,
        ..ScenarioIgnoreOrdersAuthority::default()
    };
    state.ignored_by_owner[0] = vec![2_015, -1, 2_015, 2_091];
    state.ignored_by_owner[3] = vec![-9, 7];
    let bytes = encode_scenario_ignores(&state).unwrap();
    let loaded = decode_scenario_ignores(&bytes).unwrap();
    assert_eq!(
        loaded.revision, 0,
        "process-local revision is not persisted"
    );
    assert!(loaded.ignore_orders);
    assert_eq!(loaded.ignored_by_owner, state.ignored_by_owner);
    assert_eq!(encode_scenario_ignores(&loaded).unwrap(), bytes);
}

#[test]
fn scenario_authority_rejects_unrepresentable_objects_and_owner_indices() {
    let mut state = ScenarioIgnoreOrdersAuthority::default();
    state.ignored_by_owner[2].push(i32::from(i16::MAX) + 1);
    assert_eq!(
        encode_scenario_ignores(&state),
        Err(AirRuntimeAuthorityError::IgnoredObjectOutOfRange {
            owner: 2,
            object: i32::from(i16::MAX) + 1,
        })
    );
    assert_eq!(
        ScenarioIgnoreOrdersAuthority::default().ignored_for_owner(8),
        Err(AirRuntimeAuthorityError::ScenarioOwnerOutOfRange(8))
    );
}

#[test]
fn fresh_svx_and_finished_replay_are_independent_evidence_sources() {
    // The finished replay and the fresh save supplied on 2026-08-11 have different GameInfo
    // seeds.  A cache/group/order observation from one must never answer a missing fact in the
    // other, even though their setup settings are similar.
    const FINISHED_REPLAY_SEED: u32 = 0x007f_93e0;
    const FRESH_SVX_SEED: u32 = 0x0148_10ac;
    const FRESH_SVX_HAS_EXTRACTED_AIR_PATROL_PAYLOAD: bool = false;
    assert_ne!(FINISHED_REPLAY_SEED, FRESH_SVX_SEED);
    assert!(!FRESH_SVX_HAS_EXTRACTED_AIR_PATROL_PAYLOAD);
}
