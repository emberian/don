use std::collections::BTreeMap;

use don_replay::groups_pre_pair_unit_authority::{ReplayUnitTypeFacts, ReplayUnitTypeSpans};
use don_replay::initial::ReplayByteSpan;
use don_replay::setup_group_move_authority::{
    produce_setup_group_move_authority, SetupGroupMoveAuthorityError, SetupGroupMoveAuthoritySource,
};
use don_replay::setup_unit_member_authority::{
    CanonicalSetupMemberSource, CanonicalSetupUnitMemberReceipt,
};
use don_replay::setup_units_producer::{
    EngineContainerShapeReceipt, GuyIdentityReceipt, InitUnitAuthorityReceipt, PlaceUnitCall,
    StableUnitIdentityReceipt, StartingUnitPhase, UnitMemberAuthorityReceipt,
    OBJECTS_INIT_UNIT_BYTES, OBJECTS_INIT_UNIT_VA,
};
use don_sim::systems::canonical_group_move_host::{UnitIdentity, UnitImage};
use don_sim::systems::land_speed_authority::{
    produce_resolved_land_speed_authority, LandSpeedConstants, LandSpeedContent, LandSpeedTypeFacts,
};
use don_sim::systems::movement::PathStack;
use don_sim::tick::Sim;
use don_sim::world::Handle;

#[derive(Clone)]
struct Content {
    revision: u64,
    digest: [u8; 32],
    speed_types: BTreeMap<i32, LandSpeedTypeFacts>,
}

impl LandSpeedContent for Content {
    fn land_speed_revision(&self) -> u64 {
        self.revision
    }

    fn land_speed_composition_digest(&self) -> [u8; 32] {
        self.digest
    }

    fn land_speed_type(&self, type_id: i32) -> Option<LandSpeedTypeFacts> {
        self.speed_types.get(&type_id).copied()
    }

    fn land_speed_constants(&self) -> Option<LandSpeedConstants> {
        Some(LandSpeedConstants {
            coord_scale: 1,
            irq_spear_bonus: 1,
            irq_mo_spear_bonus: 2,
            irq_hmo_spear_bonus: 3,
            irq_emo_spear_bonus: 4,
            alexander_napoleon_aura_256: 256,
            spitamenes_stable_256: 256,
            porus_elephant_256: 256,
            napoleon_siege_percent: 100,
            charles_percent: 100,
            blucher_stable_percent: 100,
            hero_aura_speed: 1,
        })
    }
}

fn type_facts() -> ReplayUnitTypeFacts {
    ReplayUnitTypeFacts {
        spans: ReplayUnitTypeSpans {
            type_base: ReplayByteSpan {
                offset: 100,
                bytes: 90,
            },
            object: ReplayByteSpan {
                offset: 208,
                bytes: 152,
            },
            unit: ReplayByteSpan {
                offset: 386,
                bytes: 792,
            },
        },
        type_index: 50,
        upgrade: -1,
        jump: -2,
        obj_masks: 4_227_108,
        attack: 40,
        max_range: 0,
        domain: 0,
        guy_spacing: 144,
        x_spacing: 144,
        y_spacing: 144,
        new_block_radius: 1,
        graft: -1,
        age: 0,
        unit_flags: 6_273,
        unit_flags2: 2,
        mode: 0,
        moves: 25,
        turn_speed: 536_870_912,
        role: 262_912,
        control_cost: 1,
        military_level: 0,
        squad_size: 1,
        uber_size: 1,
        crew_size: 0,
        base_form: 0,
    }
}

fn unit_image(sim: &Sim, row: usize, handle: Handle) -> UnitImage {
    UnitImage {
        identity: UnitIdentity {
            handle,
            who: sim.world.units.get_who(row),
            o: sim.world.units.o()[row],
            uid: sim.world.units.get_uid(row),
        },
        group: sim.world.units.group()[row],
        unit_masks: sim.world.units.get_unit_masks(row),
        form: sim.world.units.form()[row],
        form_mod: sim.world.units.form_mod()[row],
        angle: sim.world.units.angle()[row],
        x: sim.world.units.x_internal()[row],
        y: sim.world.units.y_internal()[row],
        orders_x: sim.world.units.orders_x()[row],
        orders_y: sim.world.units.orders_y()[row],
        dest_angle: sim.world.units.dest_angle()[row],
        orders: sim.world.orders(row).clone(),
        path: sim.paths[row].clone(),
    }
}

fn setup_member(
    sim: &Sim,
    row: usize,
    handle: Handle,
    ordinal: usize,
) -> CanonicalSetupUnitMemberReceipt {
    let o = sim.world.units.o()[row];
    let identity = StableUnitIdentityReceipt {
        id: handle.id,
        generation: handle.generation,
        owner: 0,
        o: i32::from(o),
    };
    let allocation = UnitMemberAuthorityReceipt {
        identity,
        ptype_index: 50,
        launching_is_null: true,
        path: EngineContainerShapeReceipt {
            length: 0,
            capacity: 10,
            increment: -1,
            flags: 0,
        },
        order_count: 0,
        guys: EngineContainerShapeReceipt {
            length: 1,
            capacity: 1,
            increment: -1,
            flags: 0,
        },
        guy_mark: 1,
        guy_identities: vec![GuyIdentityReceipt {
            slot: 0,
            who: 0,
            o,
            guy_num: 0,
        }],
        units_authority_key: (handle.id, handle.generation),
        guys_authority_key: (handle.id, handle.generation),
    };
    let init = InitUnitAuthorityReceipt {
        validated_body_va: OBJECTS_INIT_UNIT_VA,
        validated_body_bytes: OBJECTS_INIT_UNIT_BYTES,
        unit_mark_before: ordinal as i32,
        unit_mark_after: ordinal as i32 + 1,
        returned_captain_o: i32::from(o),
        members: vec![allocation.clone()],
    };
    CanonicalSetupUnitMemberReceipt {
        authority_revision: 19,
        authority_digest: [0x5a; 32],
        source: CanonicalSetupMemberSource::ReplayRulesCompleteInitReceiptAndCanonicalSim,
        replay_file_sha256: [0x24; 32],
        setup_ordinal: ordinal,
        member_ordinal: 0,
        call: PlaceUnitCall {
            ordinal: ordinal as u32,
            call_va: 0x005a_b116,
            phase: StartingUnitPhase::Citizen {
                index: ordinal as i32,
            },
            owner: 0,
            center_city_o: 2_000,
            requested_x: 10_000,
            requested_y: 10_000,
            base_type: 50,
            selected_before_upgrade: 50,
            build_units_upgrade: 50,
            place_unit_upgrade: 50,
            uber_size: 1,
            squad_size: 1,
            crew_size: 0,
        },
        init,
        allocation,
        frame: sim.world.frame,
        row,
        current_type: 50,
        type_facts: type_facts(),
        unit: unit_image(sim, row, handle),
    }
}

fn fixture() -> (Sim, Vec<CanonicalSetupUnitMemberReceipt>, Content) {
    let mut sim = Sim::new(0x9137, 100);
    sim.activate(0);
    sim.world.frame = 379;
    let handles: Vec<_> = (0..4)
        .map(|index| {
            sim.spawn_unit(0, 50, 10_000 + index * 192, 10_000, 1)
                .unwrap()
        })
        .collect();
    for row in 0..handles.len() {
        sim.world.units.myspeed_mut()[row] = 25;
    }
    sim.paths
        .resize(sim.world.live_count() as usize, PathStack::default());
    let members = handles
        .into_iter()
        .enumerate()
        .map(|(row, handle)| setup_member(&sim, row, handle, row + 3))
        .collect();
    let facts = type_facts();
    let content = Content {
        revision: 23,
        digest: [0x6b; 32],
        speed_types: BTreeMap::from([(
            50,
            LandSpeedTypeFacts {
                type_id: 50,
                from: -1,
                where_type: -1,
                graft: facts.graft,
                domain: facts.domain,
                unit_flags: facts.unit_flags,
                unit_flags2: facts.unit_flags2,
            },
        )]),
    };
    (sim, members, content)
}

#[test]
fn complete_setup_snapshot_and_exact_land_speed_produce_one_group_authority() {
    let (sim, members, content) = fixture();
    let speeds = produce_resolved_land_speed_authority(&sim, &content).unwrap();
    let receipt = produce_setup_group_move_authority(
        &sim,
        &members,
        &content,
        &speeds,
        (5_186, 72_095),
        false,
    )
    .unwrap();

    assert_eq!(
        receipt.source,
        SetupGroupMoveAuthoritySource::CompleteCanonicalSetupSnapshotAndBoundLandSpeed
    );
    assert_eq!(receipt.frame, 379);
    assert_eq!(receipt.setup_members, 4);
    assert_eq!(receipt.land_speed_revision, 23);
    assert_eq!(receipt.land_speed_digest, [0x6b; 32]);
    assert_ne!(receipt.land_speed_state_digest, 0);
    assert_eq!(receipt.authority.members.len(), 4);
    assert_ne!(receipt.authority.composition_digest, [0; 32]);
    assert!(receipt.authority.members.iter().all(|member| {
        member.speed == 25
            && member.can_move
            && member.can_install_order
            && member.land_formation.x_spacing == 144
            && member.role == 262_912
    }));
}

#[test]
fn partial_stale_or_cross_wired_authority_is_rejected() {
    let (mut sim, members, content) = fixture();
    let speeds = produce_resolved_land_speed_authority(&sim, &content).unwrap();
    assert_eq!(
        produce_setup_group_move_authority(
            &sim,
            &members[..3],
            &content,
            &speeds,
            (5_186, 72_095),
            false,
        ),
        Err(SetupGroupMoveAuthorityError::SetupCoverageMismatch {
            active_rows: 4,
            receipts: 3,
        })
    );

    let mut stale = members.clone();
    stale[2].unit.angle = stale[2].unit.angle.wrapping_add(1);
    assert_eq!(
        produce_setup_group_move_authority(&sim, &stale, &content, &speeds, (5_186, 72_095), false,),
        Err(SetupGroupMoveAuthorityError::StaleCanonicalUnit { row: 2 })
    );

    let mut wrong_static = content.clone();
    wrong_static.speed_types.get_mut(&50).unwrap().unit_flags ^= 1;
    assert_eq!(
        produce_setup_group_move_authority(
            &sim,
            &members,
            &wrong_static,
            &speeds,
            (5_186, 72_095),
            false,
        ),
        Err(SetupGroupMoveAuthorityError::LandSpeedTypeMismatch { type_id: 50 })
    );

    sim.world.units.myspeed_mut()[0] += 1;
    assert!(matches!(
        produce_setup_group_move_authority(
            &sim,
            &members,
            &content,
            &speeds,
            (5_186, 72_095),
            false,
        ),
        Err(SetupGroupMoveAuthorityError::LandSpeed(_))
    ));
}
