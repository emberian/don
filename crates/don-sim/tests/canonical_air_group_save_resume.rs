// SPDX-License-Identifier: GPL-3.0-or-later
//! Bounded Unit-carrier Scramble package -> DoNSave v13 -> AIR_PATROL -> STRAFE execution.

use don_sim::order::OrderIndex;
use don_sim::systems::air_group_action_transaction::{
    AirGroupCommand, AirTransactionStatus, LAUNCH_PATROL_OPCODE, SCRAMBLE_OPCODE,
};
use don_sim::systems::canonical_air_group_host::{AirGroupRuntimeAuthority, AirGroupUnitAuthority};
use don_sim::systems::canonical_air_patrol_runtime::{
    commit_air_patrol_activation, prepare_air_patrol_activation, CanonicalAirPatrolRuntimeError,
};
use don_sim::systems::canonical_group_move_host::{
    GroupMoveAuthority, MoveMemberAuthority, GROUP_OPCODE,
};
use don_sim::systems::canonical_strafe_runtime::{
    StrafeFireMode, StrafeRuntimeAuthority, StrafeSearchObservation, StrafeTypeFacts,
};
use don_sim::systems::groups_guys::FormationMember;
use don_sim::systems::save_load::{load_sim, save_sim};
use don_sim::systems::strafe_order_frontier::{AirTargetSearchKind, ObjectIdentity};
use don_sim::tick::lifecycle_host::PlayerTable;
use don_sim::tick::Sim;
use don_sim::Handle;

const PLANE_TYPE: i32 = 77;

fn scramble_packet(owner: u8, carrier_o: i16) -> Vec<u8> {
    let mut packet = vec![GROUP_OPCODE, 1, owner];
    packet.extend_from_slice(&carrier_o.to_le_bytes());
    packet.push(SCRAMBLE_OPCODE);
    packet
}

fn launch_patrol_packet(owner: u8, carrier_o: i16, to: (i32, i32)) -> Vec<u8> {
    let mut packet = vec![GROUP_OPCODE, 1, owner];
    packet.extend_from_slice(&carrier_o.to_le_bytes());
    packet.push(LAUNCH_PATROL_OPCODE);
    for value in [to.0, to.1, 1, 1, 0, 1] {
        packet.extend_from_slice(&value.to_le_bytes());
    }
    packet
}

fn move_member(handle: Handle, domain: i32) -> MoveMemberAuthority {
    MoveMemberAuthority {
        handle,
        role: 0x40000,
        on_map: true,
        is_captain: true,
        can_move: true,
        can_install_order: true,
        is_plane: domain == 2,
        domain,
        unit_flags: 0,
        speed: 16,
        admits_unsplit_move_near: true,
        land_formation: FormationMember::default(),
        water_formation: FormationMember::default(),
    }
}

fn install_package_authorities(sim: &mut Sim, plane: Handle, carrier: Handle) {
    sim.replace_group_move_authority(GroupMoveAuthority {
        revision: 0x47,
        composition_digest: [0xa7; 32],
        destination_is_water: false,
        force_formation_facing_zero: false,
        members: vec![move_member(carrier, 0), move_member(plane, 2)],
    });
    sim.replace_air_group_authority(AirGroupRuntimeAuthority {
        revision: 0x48,
        scenario_revision: 0x49,
        composition_digest: [0xb8; 32],
        ignore_orders: false,
        units: vec![AirGroupUnitAuthority {
            handle: plane,
            object_masks: 0,
            busy: false,
            is_biplane: true,
            is_bomber: false,
            is_helicopter: false,
        }],
    });
}

fn actor_identity(sim: &Sim, plane: Handle) -> ObjectIdentity {
    let row = sim.world.row_of(plane).unwrap();
    ObjectIdentity {
        o: i32::from(sim.world.units.o()[row]),
        who: i32::from(sim.world.units.get_who(row)),
        uid: sim.world.units.get_uid(row),
    }
}

fn target_identity(sim: &Sim, target: Handle) -> ObjectIdentity {
    let row = sim.world.row_of(target).unwrap();
    ObjectIdentity {
        o: i32::from(sim.world.units.o()[row]),
        who: i32::from(sim.world.units.get_who(row)),
        uid: sim.world.units.get_uid(row),
    }
}

fn install_runtime_authority(sim: &mut Sim, actor: ObjectIdentity, target: ObjectIdentity) {
    let mut authority = StrafeRuntimeAuthority {
        revision: 0x4149_5250_4154_524f,
        ..Default::default()
    };
    authority.insert_type(
        PLANE_TYPE,
        StrafeTypeFacts {
            animal: false,
            missile: false,
            helicopter: false,
            bomber: false,
            strafes: false,
            speed: 16,
            min_range: 0,
            max_range: 0x400,
            fire_mode: StrafeFireMode::Hold,
            attack_call_al: 0,
            bomber_spell_delta: None,
        },
    );
    authority.searches.push(StrafeSearchObservation {
        actor,
        frame: 0,
        kind: AirTargetSearchKind::AirFirst,
        result: Some(target),
    });
    sim.replace_strafe_runtime_authority(authority);
}

fn fixture() -> (Sim, Handle, Handle, Handle, Vec<u8>) {
    let mut sim = Sim::new(0x6a17, 4);
    let mut players = PlayerTable::new();
    players.seat(0, 1, 0, 0);
    sim.players = Some(players);

    // Plane must be owner-0 object 0 so frame 0 reaches retail's mod-16 search.
    let plane = sim.spawn_unit(0, PLANE_TYPE, 1_000, 1_000, 4).unwrap();
    let carrier = sim.spawn_unit(0, 90, 2_000, 2_000, 4).unwrap();
    let target = sim.spawn_unit(1, 91, 2_000, 1_800, 4).unwrap();
    let plane_row = sim.world.row_of(plane).unwrap();
    let carrier_row = sim.world.row_of(carrier).unwrap();
    let plane_o = sim.world.units.o()[plane_row];
    let carrier_o = sim.world.units.o()[carrier_row];
    sim.world.units.inside_down_mut()[carrier_row] = plane_o;
    sim.world.units.inside_down_who_mut()[carrier_row] = 0;
    sim.world.units.inside_up_mut()[plane_row] = carrier_o;
    sim.world.units.inside_up_who_mut()[plane_row] = 0;
    sim.world.units.inside_down_mut()[plane_row] = -1;
    sim.world.units.inside_down_who_mut()[plane_row] = -1;
    for handle in [plane, carrier] {
        let row = sim.world.row_of(handle).unwrap();
        sim.world.units.group_mut()[row] = -1;
        sim.world.units.o_down_mut()[row] = -1;
        sim.world.units.form_mut()[row] = 0;
        sim.world.units.form_mod_mut()[row] = 50;
    }
    install_package_authorities(&mut sim, plane, carrier);
    let actor = actor_identity(&sim, plane);
    let target_id = target_identity(&sim, target);
    install_runtime_authority(&mut sim, actor, target_id);
    let packet = scramble_packet(0, carrier_o);
    (sim, plane, carrier, target, packet)
}

fn assert_air_state(left: &Sim, right: &Sim, plane: Handle) {
    let left_row = left.world.row_of(plane).unwrap();
    let right_row = right.world.row_of(plane).unwrap();
    assert_eq!(left.world.frame, right.world.frame);
    assert_eq!(left.world.random.state(), right.world.random.state());
    assert_eq!(left.world.orders(left_row), right.world.orders(right_row));
    assert_eq!(left.paths[left_row], right.paths[right_row]);
    assert_eq!(
        left.world.units.x_internal()[left_row],
        right.world.units.x_internal()[right_row]
    );
    assert_eq!(
        left.world.units.y_internal()[left_row],
        right.world.units.y_internal()[right_row]
    );
    assert_eq!(
        left.world.units.angle()[left_row],
        right.world.units.angle()[right_row]
    );
}

#[test]
fn scramble_package_v13_reload_resumes_air_patrol_then_real_strafe() {
    let (mut control, plane, carrier, target, packet) = fixture();
    let receipt = control.process_air_group_package(0, 71, &packet).unwrap();
    assert!(receipt.validates());
    assert!(matches!(receipt.status, AirTransactionStatus::Applied(_)));
    let plane_row = control.world.row_of(plane).unwrap();
    assert_eq!(
        control.world.orders(plane_row).order_type(),
        OrderIndex::AirPatrol
    );
    assert!(control
        .world
        .orders(plane_row)
        .current()
        .unwrap()
        .air_patrol
        .is_some());
    let rng_after_package = control.world.random.state();

    let bytes = save_sim(&control).unwrap();
    let mut resumed = load_sim(&bytes).unwrap();
    assert_eq!(save_sim(&resumed).unwrap(), bytes);
    install_package_authorities(&mut resumed, plane, carrier);
    let actor = actor_identity(&resumed, plane);
    let target_id = target_identity(&resumed, target);
    install_runtime_authority(&mut resumed, actor, target_id);

    control.do_frame();
    resumed.do_frame();
    assert_air_state(&control, &resumed, plane);
    assert_eq!(
        control.world.orders(plane_row).order_type(),
        OrderIndex::Strafe
    );
    assert!(control
        .world
        .orders(plane_row)
        .current()
        .unwrap()
        .strafe
        .is_some());
    assert!(control.last_air_patrol_error.is_none());
    let patrol_receipt = control.last_air_patrol_receipt.as_ref().unwrap();
    assert!(patrol_receipt.inserted_strafe);
    assert_eq!(
        patrol_receipt.rng_epoch_after - patrol_receipt.rng_epoch_before,
        1
    );
    assert_ne!(control.world.random.state(), rng_after_package);

    let after_patrol_pos = (
        control.world.units.x_internal()[plane_row],
        control.world.units.y_internal()[plane_row],
    );
    control.do_frame();
    resumed.do_frame();
    assert_air_state(&control, &resumed, plane);
    assert!(control.last_strafe_error.is_none());
    assert!(control.last_strafe_receipt.is_some());
    assert_ne!(
        (
            control.world.units.x_internal()[plane_row],
            control.world.units.y_internal()[plane_row]
        ),
        after_patrol_pos
    );
}

#[test]
fn launch_patrol_six_dword_packet_commits_the_same_typed_air_order_owner() {
    let (mut sim, plane, carrier, _target, _) = fixture();
    let carrier_row = sim.world.row_of(carrier).unwrap();
    let carrier_o = sim.world.units.o()[carrier_row];
    let packet = launch_patrol_packet(0, carrier_o, (3_200, 3_400));
    let before_rng = sim.world.random.state();
    let receipt = sim.process_air_group_package(0, 73, &packet).unwrap();
    assert!(receipt.validates());
    assert!(matches!(receipt.status, AirTransactionStatus::Applied(_)));
    assert!(matches!(
        receipt.request.command,
        AirGroupCommand::LaunchPatrol(request)
            if (request.to_x, request.to_y, request.queue, request.force_all,
                request.bombers_only, request.fighters_only)
                == (3_200, 3_400, 1, 1, 0, 1)
    ));
    let row = sim.world.row_of(plane).unwrap();
    let payload = sim
        .world
        .orders(row)
        .current()
        .unwrap()
        .air_patrol
        .as_ref()
        .unwrap();
    assert_eq!(payload.x.values, vec![1_200]);
    assert_eq!(payload.y.values, vec![1_400]);
    assert_eq!(
        (payload.air.home_o, payload.air.home_who),
        (i32::from(carrier_o), 0)
    );
    assert_eq!(sim.world.random.state(), before_rng);
}

#[test]
fn scramble_helicopter_branch_publishes_the_exact_move_facing_action_columns() {
    let (mut sim, plane, _carrier, _target, packet) = fixture();
    let row = sim.world.row_of(plane).unwrap();
    sim.world.units.set_unit_masks(row, 0x0400_0123);
    sim.group_move_authority
        .members
        .iter_mut()
        .find(|member| member.handle == plane)
        .unwrap()
        .unit_flags = don_sim::command::air_launch_receivers::HELICOPTER_TYPE_FLAG;
    sim.air_group_authority
        .units
        .iter_mut()
        .find(|member| member.handle == plane)
        .unwrap()
        .is_helicopter = true;
    let receipt = sim.process_air_group_package(0, 75, &packet).unwrap();
    assert!(receipt.validates());
    assert!(matches!(receipt.status, AirTransactionStatus::Applied(_)));
    let order = sim.world.orders(row).current().unwrap();
    assert_eq!(order.kind, OrderIndex::MoveTo);
    assert_eq!(
        sim.world.units.dest_angle()[row],
        sim.world.units.angle()[row]
    );
    assert_ne!(
        sim.world.units.dest_angle()[row],
        order.move_state.unwrap().angle,
        "group MOVE_TO remains the action, so update_action retains the actor facing"
    );
    assert_eq!(sim.world.units.get_unit_masks(row), 0x123);
    assert!(sim.paths[row].is_empty());
}

#[test]
fn air_patrol_commit_rejects_a_changed_canonical_world_without_publishing_after_image() {
    let (mut sim, plane, _carrier, _target, packet) = fixture();
    let receipt = sim.process_air_group_package(0, 74, &packet).unwrap();
    assert!(matches!(receipt.status, AirTransactionStatus::Applied(_)));
    let row = sim.world.row_of(plane).unwrap();
    let prepared = prepare_air_patrol_activation(
        &sim.world,
        &sim.paths,
        &sim.unit_type,
        &sim.strafe_runtime_authority,
        row,
        (sim.map.world.xs * 4, sim.map.world.ys * 4),
    )
    .unwrap();
    sim.world.units.x_internal_mut()[row] += 1;
    let changed_x = sim.world.units.x_internal()[row];
    assert_eq!(
        commit_air_patrol_activation(
            &mut sim.world,
            &mut sim.paths,
            &sim.unit_type,
            &mut sim.strafe_runtime_authority,
            prepared,
        ),
        Err(CanonicalAirPatrolRuntimeError::StaleCanonicalState)
    );
    assert_eq!(sim.world.units.x_internal()[row], changed_x);
    assert_eq!(sim.world.orders(row).order_type(), OrderIndex::AirPatrol);
}

#[test]
fn armed_ignore_orders_refuses_without_group_order_or_rng_mutation() {
    let (mut sim, plane, _carrier, _target, packet) = fixture();
    sim.air_group_authority.ignore_orders = true;
    let before_rng = sim.world.random.state();
    let before_groups = sim.groups.clone();
    let plane_row = sim.world.row_of(plane).unwrap();
    let before_order = sim.world.orders(plane_row).clone();
    let receipt = sim.process_air_group_package(0, 72, &packet).unwrap();
    assert!(receipt.validates());
    assert!(matches!(receipt.status, AirTransactionStatus::Blocked(_)));
    assert_eq!(sim.world.random.state(), before_rng);
    assert_eq!(sim.groups.list, before_groups.list);
    assert_eq!(sim.world.orders(plane_row), &before_order);
}
