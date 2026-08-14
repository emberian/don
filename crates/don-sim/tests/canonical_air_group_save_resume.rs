// SPDX-License-Identifier: GPL-3.0-or-later
//! Bounded Unit-carrier Scramble package -> current DoNSave -> AIR_PATROL -> STRAFE execution.

use don_sim::order::{Order, OrderIndex, SpecialAnimType};
use don_sim::systems::air_busy_authority::{AirBusyAuthorityError, AirBusySpellAuthority};
use don_sim::systems::air_group_action_transaction::{
    AirGroupActionPlan, AirGroupCommand, AirTransactionStatus, CommandPackagePosition,
    LAUNCH_PATROL_OPCODE, SCRAMBLE_OPCODE,
};
use don_sim::systems::canonical_air_group_host::{
    commit_canonical_air_package, prepare_canonical_air_package, AirGroupRuntimeAuthority,
    AirGroupUnitAuthority,
};
use don_sim::systems::canonical_air_patrol_runtime::{
    commit_air_patrol_activation, prepare_air_patrol_activation, CanonicalAirPatrolRuntimeError,
};
use don_sim::systems::canonical_group_move_host::{
    BuildSelectionAuthority, BuildSelectionIdentity, GroupMoveAuthority, MoveMemberAuthority,
    GROUP_OPCODE,
};
use don_sim::systems::canonical_strafe_runtime::{
    StrafeFireMode, StrafeRuntimeAuthority, StrafeSearchObservation, StrafeTypeFacts,
};
use don_sim::systems::economy_order_payload_authority::{CastOrderPayload, EconomyOrderPayload};
use don_sim::systems::groups_guys::FormationMember;
use don_sim::systems::production::{self, BuildData};
use don_sim::systems::save_load::{load_sim, save_sim};
use don_sim::systems::strafe_order_frontier::{AirTargetSearchKind, ObjectIdentity};
use don_sim::tick::lifecycle_host::PlayerTable;
use don_sim::tick::Sim;
use don_sim::Handle;

const PLANE_TYPE: i32 = 77;

// Exact package 2,304 / turn serial 2,305 from retail replay SHA-256
// e8c0103f21dbdb97ecd083c1899065209bdb055581daaceff0c3ee547100ef8d.
// The full opcode chronology is [79, 0, 11, 58, 74, 72]; this bounded transaction consumes
// the adjacent Group/LaunchPatrol pair at indices 1/2 against the canonical pre-Group state.
const RETAIL_BUILD_LAUNCH_GROUP: &[u8] = &[
    0x00, 0x04, 0x02, 0x2d, 0x08, 0x2e, 0x08, 0x2f, 0x08, 0x30, 0x08,
];
const RETAIL_BUILD_LAUNCH_ACTION: &[u8] = &[
    0x0b, 0xeb, 0xb2, 0x00, 0x00, 0xa7, 0x7d, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];
const RETAIL_BUILD_LAUNCH_POSITION: CommandPackagePosition = CommandPackagePosition {
    game_frame: 66_809,
    package_serial: 2_305,
    play: 1,
    group_command_index: 1,
    action_command_index: 2,
};

fn scramble_packet(owner: u8, carrier_o: i16) -> Vec<u8> {
    let mut packet = vec![GROUP_OPCODE, 1, owner];
    packet.extend_from_slice(&carrier_o.to_le_bytes());
    packet.push(SCRAMBLE_OPCODE);
    packet
}

fn scramble_packet_many(owner: u8, carriers: &[i16]) -> Vec<u8> {
    let mut packet = vec![GROUP_OPCODE, carriers.len() as u8, owner];
    for carrier in carriers {
        packet.extend_from_slice(&carrier.to_le_bytes());
    }
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
        composition_digest: [0xb8; 32],
        units: vec![AirGroupUnitAuthority {
            handle: plane,
            object_masks: 0,
            mana_cap: 1_000,
            is_biplane: true,
            is_bomber: false,
            is_helicopter: false,
            is_nuclear_missile: false,
        }],
        builds: Vec::new(),
        busy_spells: Vec::new(),
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
        frame: sim.world.frame,
        kind: AirTargetSearchKind::AirFirst,
        result: Some(target),
    });
    sim.replace_strafe_runtime_authority(authority);
}

fn launch_build_record(
    o: i16,
    uid: u16,
    position: (i32, i32),
    child: Option<(u8, i16)>,
) -> BuildData {
    let mut build = BuildData {
        flags: production::flag::VALID | production::flag::STARTED | production::flag::ACTIVE,
        uid,
        gather_down: -1,
        city: -1,
        city_down: -1,
        wonder: -1,
        dock: -1,
        attack_ox: -1,
        attack_whom: -1,
        ..BuildData::default()
    };
    build.other[production::off::OBJECT_ID..production::off::OBJECT_ID + 2]
        .copy_from_slice(&o.to_le_bytes());
    build.other[production::off::X_INTERNAL..production::off::X_INTERNAL + 4]
        .copy_from_slice(&(position.0 ^ 0x63637).to_le_bytes());
    build.other[production::off::Y_INTERNAL..production::off::Y_INTERNAL + 4]
        .copy_from_slice(&(position.1 ^ 0x63637).to_le_bytes());
    build.other[0x2a..0x2c].copy_from_slice(&(-1i16).to_le_bytes());
    match child {
        Some((who, o)) => {
            build.other[0x28..0x2a].copy_from_slice(&o.to_le_bytes());
            build.other[0x3e] = who;
        }
        None => {
            build.other[0x28..0x2a].copy_from_slice(&(-1i16).to_le_bytes());
            build.other[0x3e] = 0xff;
        }
    }
    build
}

fn retail_build_launch_fixture() -> (Sim, Vec<Handle>, Handle) {
    let mut sim = Sim::new(0x11a0_2305, 32);
    let mut players = PlayerTable::new();
    players.seat(1, 1, 2, 0);
    sim.players = Some(players);
    sim.world.frame = RETAIL_BUILD_LAUNCH_POSITION.game_frame;
    sim.vic_match.frame = RETAIL_BUILD_LAUNCH_POSITION.game_frame;

    // Frame 66,809 is 9 mod 16. Seven inactive-for-this-cohort owner-local rows make the
    // cheapest reached aircraft object 7, so `(o + frame) % 16 == 0` on the first resumed
    // AIR_PATROL tick without changing the recorded package frame.
    for index in 0..7 {
        sim.spawn_unit(2, 90, 10_000 + index * 20, 10_000, 4)
            .unwrap();
    }
    let planes: Vec<_> = (0..4)
        .map(|index| {
            sim.spawn_unit(2, PLANE_TYPE, 40_000 + index * 100, 30_000, 4)
                .unwrap()
        })
        .collect();
    let target = sim.spawn_unit(3, 91, 45_700, 32_100, 4).unwrap();
    let builds = [2_093i16, 2_094, 2_095, 2_096];
    let target_coord = (45_803, 32_167);
    let positions = [
        (target_coord.0 - 200, target_coord.1 - 100),
        (target_coord.0 - 5_000, target_coord.1),
        (target_coord.0 + 8_000, target_coord.1),
        (target_coord.0, target_coord.1 + 11_000),
    ];
    for (plane, &build_o) in planes.iter().zip(&builds) {
        let row = sim.world.row_of(*plane).unwrap();
        sim.world.units.group_mut()[row] = -1;
        sim.world.units.o_down_mut()[row] = -1;
        sim.world.units.inside_up_mut()[row] = build_o;
        sim.world.units.inside_up_who_mut()[row] = 2;
        sim.world.units.inside_down_mut()[row] = -1;
        sim.world.units.inside_down_who_mut()[row] = -1;
    }
    for row in 0..=96usize {
        let o = 2_000 + row as i16;
        let child = builds
            .iter()
            .position(|&candidate| candidate == o)
            .map(|index| {
                let plane_row = sim.world.row_of(planes[index]).unwrap();
                (2, sim.world.units.o()[plane_row])
            });
        let position = builds
            .iter()
            .position(|&candidate| candidate == o)
            .map_or((20_000 + row as i32, 20_000), |index| positions[index]);
        let spawned = sim.spawn_build(
            2,
            launch_build_record(o, 0x5200 + row as u16, position, child),
        );
        assert_eq!(spawned, row);
    }
    sim.replace_group_move_authority(GroupMoveAuthority {
        revision: 0x2305,
        composition_digest: [0x23; 32],
        destination_is_water: false,
        force_formation_facing_zero: false,
        members: planes
            .iter()
            .copied()
            .map(|plane| move_member(plane, 2))
            .collect(),
    });
    sim.replace_air_group_authority(AirGroupRuntimeAuthority {
        revision: 0x66809,
        composition_digest: [0x11; 32],
        units: planes
            .iter()
            .copied()
            .map(|handle| AirGroupUnitAuthority {
                handle,
                object_masks: 0,
                mana_cap: 1_000,
                is_biplane: true,
                is_bomber: false,
                is_helicopter: false,
                is_nuclear_missile: false,
            })
            .collect(),
        builds: builds
            .iter()
            .map(|&o| {
                let row = u32::try_from(o - 2_000).unwrap();
                BuildSelectionAuthority {
                    identity: BuildSelectionIdentity {
                        row,
                        who: 2,
                        o,
                        uid: sim.builds[row as usize].uid,
                    },
                    role: 0x200,
                    is_airbase: true,
                }
            })
            .collect(),
        busy_spells: Vec::new(),
    });
    (sim, planes, target)
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

fn subordinate_ignore_fixture() -> (Sim, Handle, Handle, Handle, Vec<u8>) {
    let (mut sim, plane, carrier, _target, packet) = fixture();
    let subordinate = sim.spawn_unit(0, 92, 2_100, 2_000, 4).unwrap();
    let carrier_row = sim.world.row_of(carrier).unwrap();
    let subordinate_row = sim.world.row_of(subordinate).unwrap();
    let carrier_o = sim.world.units.o()[carrier_row];
    let subordinate_o = sim.world.units.o()[subordinate_row];
    sim.world.units.o_down_mut()[carrier_row] = subordinate_o;
    sim.world.units.o_up_mut()[subordinate_row] = carrier_o;
    sim.world.units.o_down_mut()[subordinate_row] = -1;
    sim.world.units.group_mut()[subordinate_row] = -1;
    let mut subordinate_authority = move_member(subordinate, 0);
    subordinate_authority.is_captain = false;
    sim.group_move_authority.members.push(subordinate_authority);
    sim.scenario_ignore_orders.ignore_orders = true;
    sim.scenario_ignore_orders.ignored_by_owner[0] = vec![i32::from(subordinate_o)];
    (sim, plane, carrier, subordinate, packet)
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

fn cast_order(spell: i32) -> Order {
    Order {
        kind: OrderIndex::CastSpell,
        x: -1,
        y: -1,
        target_uid: u16::MAX,
        economy: Some(EconomyOrderPayload::CastSpell(CastOrderPayload {
            paid: 1,
            spell,
        })),
        ..Order::default()
    }
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
fn retail_build_launch_single_best_current_reload_resumes_real_air_tick() {
    let (mut control, planes, target) = retail_build_launch_fixture();
    let before_rng = control.world.random.state();
    let receipt = control
        .process_air_group_packet_pair(
            RETAIL_BUILD_LAUNCH_POSITION,
            RETAIL_BUILD_LAUNCH_GROUP,
            RETAIL_BUILD_LAUNCH_ACTION,
        )
        .unwrap();
    assert!(receipt.validates());
    assert_eq!(receipt.request.position, RETAIL_BUILD_LAUNCH_POSITION);
    assert!(matches!(
        receipt.request.command,
        AirGroupCommand::LaunchPatrol(request)
            if (request.to_x, request.to_y, request.queue, request.force_all,
                request.bombers_only, request.fighters_only)
                == (45_803, 32_167, 2, 0, 0, 0)
    ));
    let AirGroupActionPlan::LaunchPatrol(plan) = receipt.plan.as_ref().unwrap() else {
        panic!("retail opcode 11 must retain its single-best plan");
    };
    let winner = planes[0];
    let winner_row = control.world.row_of(winner).unwrap();
    let winner_o = control.world.units.o()[winner_row];
    assert_eq!(winner_o, 7);
    assert_eq!(plan.best.unwrap().plane, (2, winner_o));
    assert_eq!(plan.installs.len(), 1);
    assert!(!plan.launched_any, "queue 2 does not enter launch-all");
    assert_eq!(control.world.random.state(), before_rng);
    assert_eq!(
        control.world.orders(winner_row).order_type(),
        OrderIndex::AirPatrol
    );
    let payload = control
        .world
        .orders(winner_row)
        .current()
        .unwrap()
        .air_patrol
        .as_ref()
        .unwrap();
    assert_eq!(payload.x.values, vec![200]);
    assert_eq!(payload.y.values, vec![100]);
    assert_eq!((payload.air.home_o, payload.air.home_who), (2_093, 2));
    for plane in &planes[1..] {
        let row = control.world.row_of(*plane).unwrap();
        assert_eq!(control.world.orders(row).order_type(), OrderIndex::None);
    }

    let bytes = save_sim(&control).unwrap();
    // Magic, root ChunkHeader, CORE ChunkHeader, then CoreState::format_version.
    assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), 19);
    let mut resumed = load_sim(&bytes).unwrap();
    assert_eq!(save_sim(&resumed).unwrap(), bytes);
    let actor = actor_identity(&control, winner);
    let target_id = target_identity(&control, target);
    install_runtime_authority(&mut control, actor, target_id);
    install_runtime_authority(&mut resumed, actor, target_id);

    control.do_frame();
    resumed.do_frame();
    assert_air_state(&control, &resumed, winner);
    assert_eq!(
        control.world.orders(winner_row).order_type(),
        OrderIndex::Strafe,
        "error={:?} receipt={:?}",
        control.last_air_patrol_error,
        control.last_air_patrol_receipt,
    );
    assert!(control.last_air_patrol_error.is_none());
    assert!(control
        .last_air_patrol_receipt
        .as_ref()
        .is_some_and(|air| air.inserted_strafe));

    let after_patrol = (
        control.world.units.x_internal()[winner_row],
        control.world.units.y_internal()[winner_row],
    );
    control.do_frame();
    resumed.do_frame();
    assert_air_state(&control, &resumed, winner);
    assert!(control.last_strafe_error.is_none());
    assert!(control.last_strafe_receipt.is_some());
    assert_ne!(
        (
            control.world.units.x_internal()[winner_row],
            control.world.units.y_internal()[winner_row],
        ),
        after_patrol,
    );
}

#[test]
fn changed_build_home_between_air_prepare_and_commit_publishes_nothing() {
    let (mut sim, planes, target) = retail_build_launch_fixture();
    let receipt = sim
        .process_air_group_packet_pair(
            RETAIL_BUILD_LAUNCH_POSITION,
            RETAIL_BUILD_LAUNCH_GROUP,
            RETAIL_BUILD_LAUNCH_ACTION,
        )
        .unwrap();
    assert!(receipt.validates());
    let winner = planes[0];
    let row = sim.world.row_of(winner).unwrap();
    let actor = actor_identity(&sim, winner);
    let target = target_identity(&sim, target);
    install_runtime_authority(&mut sim, actor, target);
    let prepared = prepare_air_patrol_activation(
        &sim.world,
        &sim.builds,
        &sim.paths,
        &sim.unit_type,
        &sim.strafe_runtime_authority,
        row,
        (sim.map.world.xs * 4, sim.map.world.ys * 4),
    )
    .unwrap();
    let before_order = sim.world.orders(row).clone();
    let before_path = sim.paths[row].clone();
    let before_position = (
        sim.world.units.x_internal()[row],
        sim.world.units.y_internal()[row],
    );
    let before_rng = sim.world.random.state();
    let before_authority = sim.strafe_runtime_authority.clone();
    sim.builds[93].uid ^= 1;

    assert_eq!(
        commit_air_patrol_activation(
            &mut sim.world,
            &sim.builds,
            &mut sim.paths,
            &sim.unit_type,
            &mut sim.strafe_runtime_authority,
            prepared,
        ),
        Err(CanonicalAirPatrolRuntimeError::StaleHome),
    );
    assert_eq!(sim.world.orders(row), &before_order);
    assert_eq!(sim.paths[row], before_path);
    assert_eq!(
        (
            sim.world.units.x_internal()[row],
            sim.world.units.y_internal()[row],
        ),
        before_position,
    );
    assert_eq!(sim.world.random.state(), before_rng);
    assert_eq!(sim.strafe_runtime_authority, before_authority);
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
        &sim.builds,
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
            &sim.builds,
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
fn armed_ignore_orders_duplicate_tombstone_prune_survives_save_and_commits_without_rng() {
    let (mut sim, plane, carrier, _target, packet) = fixture();
    let carrier_row = sim.world.row_of(carrier).unwrap();
    let carrier_o = sim.world.units.o()[carrier_row];
    sim.scenario_ignore_orders.ignore_orders = true;
    sim.scenario_ignore_orders.ignored_by_owner[0] =
        vec![i32::from(carrier_o), -1, i32::from(carrier_o)];

    let bytes = save_sim(&sim).unwrap();
    let mut resumed = load_sim(&bytes).unwrap();
    assert_eq!(save_sim(&resumed).unwrap(), bytes);
    install_package_authorities(&mut resumed, plane, carrier);
    let before_rng = resumed.world.random.state();
    let plane_row = resumed.world.row_of(plane).unwrap();
    let before_order = resumed.world.orders(plane_row).clone();
    let receipt = resumed.process_air_group_package(0, 72, &packet).unwrap();
    assert!(receipt.validates());
    assert!(
        matches!(receipt.status, AirTransactionStatus::Applied(ref evidence) if evidence.installs.is_empty()),
        "{receipt:#?}"
    );
    let group = &resumed.groups.list[resumed.groups.last_group[0] as usize];
    assert_eq!(group.num, 0);
    assert_eq!(resumed.world.units.group()[carrier_row], -1);
    assert_eq!(resumed.world.orders(plane_row), &before_order);
    assert_eq!(resumed.world.random.state(), before_rng);
}

#[test]
fn armed_prune_packet_save_reload_resumes_the_surviving_air_patrol_runtime() {
    let (mut control, plane, carrier, target, _packet) = fixture();
    let ignored_carrier = control.spawn_unit(0, 93, 2_400, 2_000, 4).unwrap();
    let carrier_row = control.world.row_of(carrier).unwrap();
    let ignored_row = control.world.row_of(ignored_carrier).unwrap();
    let carrier_o = control.world.units.o()[carrier_row];
    let ignored_o = control.world.units.o()[ignored_row];
    control.world.units.group_mut()[ignored_row] = -1;
    control.world.units.o_down_mut()[ignored_row] = -1;
    control.world.units.form_mut()[ignored_row] = 0;
    control.world.units.form_mod_mut()[ignored_row] = 50;
    control
        .group_move_authority
        .members
        .push(move_member(ignored_carrier, 0));
    control.scenario_ignore_orders.ignore_orders = true;
    control.scenario_ignore_orders.ignored_by_owner[0] = vec![i32::from(ignored_o)];
    let packet = scramble_packet_many(0, &[ignored_o, carrier_o]);

    let receipt = control.process_air_group_package(0, 0x96, &packet).unwrap();
    assert!(receipt.validates());
    assert!(
        matches!(receipt.status, AirTransactionStatus::Applied(ref evidence) if evidence.installs.len() == 1)
    );
    let group = &control.groups.list[control.groups.last_group[0] as usize];
    assert_eq!(&group.list[..group.num as usize], &[carrier_o]);
    assert_eq!(control.world.units.group()[ignored_row], -1);

    let bytes = save_sim(&control).unwrap();
    let mut resumed = load_sim(&bytes).unwrap();
    assert_eq!(save_sim(&resumed).unwrap(), bytes);
    let actor = actor_identity(&resumed, plane);
    let target_id = target_identity(&resumed, target);
    install_runtime_authority(&mut resumed, actor, target_id);

    control.do_frame();
    resumed.do_frame();
    assert_air_state(&control, &resumed, plane);
    assert_eq!(
        control
            .world
            .orders(control.world.row_of(plane).unwrap())
            .order_type(),
        OrderIndex::Strafe
    );
}

#[test]
fn ignored_subordinate_redirects_to_captain_before_the_air_body() {
    let (mut sim, plane, carrier, subordinate, packet) = subordinate_ignore_fixture();
    let carrier_row = sim.world.row_of(carrier).unwrap();
    let subordinate_row = sim.world.row_of(subordinate).unwrap();
    let before_rng = sim.world.random.state();
    let plane_row = sim.world.row_of(plane).unwrap();
    let before_order = sim.world.orders(plane_row).clone();
    let receipt = sim.process_air_group_package(0, 0x94, &packet).unwrap();
    assert!(receipt.validates());
    assert!(
        matches!(receipt.status, AirTransactionStatus::Applied(ref evidence) if evidence.installs.is_empty()),
        "{receipt:#?}"
    );
    assert_eq!(sim.world.units.group()[carrier_row], -1);
    assert_eq!(sim.world.units.group()[subordinate_row], -1);
    assert_eq!(sim.world.orders(plane_row), &before_order);
    assert_eq!(sim.world.random.state(), before_rng);
}

#[test]
fn stale_scenario_list_between_prepare_and_commit_rolls_back_every_owner() {
    let (mut sim, plane, carrier, _subordinate, packet) = subordinate_ignore_fixture();
    let carrier_row = sim.world.row_of(carrier).unwrap();
    let players = std::array::from_fn(|play| (play == 0).then_some(0));
    let prepared = prepare_canonical_air_package(
        &sim.world,
        &sim.builds,
        &sim.groups,
        &sim.paths,
        &sim.command_package_state,
        &sim.group_move_authority,
        &sim.air_group_authority,
        &sim.scenario_ignore_orders,
        &players,
        sim.world.frame,
        0,
        0x95,
        &packet,
    )
    .unwrap();
    let before_groups = sim.groups.clone();
    let before_cache = sim.command_package_state.clone();
    let before_carrier_group = sim.world.units.group()[carrier_row];
    let before_order = sim.world.orders(sim.world.row_of(plane).unwrap()).clone();
    sim.scenario_ignore_orders.ignored_by_owner[0].push(-1);
    let receipt = commit_canonical_air_package(
        &mut sim.world,
        &sim.builds,
        &mut sim.groups,
        &mut sim.paths,
        &mut sim.command_package_state,
        &sim.group_move_authority,
        &sim.air_group_authority,
        &sim.scenario_ignore_orders,
        prepared,
    );
    assert!(receipt.validates());
    assert!(matches!(
        receipt.status,
        AirTransactionStatus::RolledBack { .. }
    ));
    assert_eq!(sim.groups.list, before_groups.list);
    assert_eq!(sim.command_package_state, before_cache);
    assert_eq!(sim.world.units.group()[carrier_row], before_carrier_group);
    assert_eq!(
        sim.world.orders(sim.world.row_of(plane).unwrap()),
        &before_order
    );
}

#[test]
fn saved_cast_with_both_busy_predicates_clear_launches_identically_after_reload() {
    let (mut control, plane, carrier, _target, packet) = fixture();
    let row = control.world.row_of(plane).unwrap();
    let spell = 0x292;
    control.world.orders_mut(row).replace(cast_order(spell));
    control.air_group_authority.busy_spells = vec![AirBusySpellAuthority {
        spell,
        predicate_50: false,
        predicate_54: false,
    }];

    let bytes = save_sim(&control).unwrap();
    let mut resumed = load_sim(&bytes).unwrap();
    assert_eq!(save_sim(&resumed).unwrap(), bytes);
    install_package_authorities(&mut resumed, plane, carrier);
    resumed.air_group_authority.busy_spells = control.air_group_authority.busy_spells.clone();

    let control_receipt = control.process_air_group_package(0, 0x91, &packet).unwrap();
    let resumed_receipt = resumed.process_air_group_package(0, 0x91, &packet).unwrap();
    assert!(matches!(
        control_receipt.status,
        AirTransactionStatus::Applied(_)
    ));
    assert_eq!(control_receipt, resumed_receipt);
    assert_eq!(control.world.orders(row), resumed.world.orders(row));
    assert_eq!(
        control.world.orders(row).order_type(),
        OrderIndex::AirPatrol
    );
    assert_eq!(control.world.random.state(), resumed.world.random.state());
}

#[test]
fn either_cast_predicate_and_enter_or_exit_special_anim_are_exact_busy_vetoes() {
    let spell = 0x292;
    let cases = [
        (
            cast_order(spell),
            vec![AirBusySpellAuthority {
                spell,
                predicate_50: true,
                predicate_54: false,
            }],
        ),
        (
            cast_order(spell),
            vec![AirBusySpellAuthority {
                spell,
                predicate_50: false,
                predicate_54: true,
            }],
        ),
        (Order::special_anim(SpecialAnimType::Enter, 0, 0), vec![]),
        (Order::special_anim(SpecialAnimType::Exit, 0, 0), vec![]),
    ];
    for (order, busy_spells) in cases {
        let (mut sim, plane, carrier, _target, packet) = fixture();
        let row = sim.world.row_of(plane).unwrap();
        let carrier_row = sim.world.row_of(carrier).unwrap();
        let carrier_o = sim.world.units.o()[carrier_row];
        sim.world.orders_mut(row).replace(order);
        sim.air_group_authority.busy_spells = busy_spells;
        let before_groups = sim.groups.clone();
        let before_cache = sim.command_package_state.clone();
        let before_order = sim.world.orders(row).clone();
        let before_rng = sim.world.random.state();
        let receipt = sim.process_air_group_package(0, 0x92, &packet).unwrap();
        assert!(receipt.validates());
        assert!(matches!(
            receipt.status,
            AirTransactionStatus::Applied(ref evidence) if evidence.installs.is_empty()
        ));
        assert_ne!(sim.groups.list, before_groups.list);
        assert_ne!(sim.command_package_state, before_cache);
        assert_eq!(
            &sim.groups.list[sim.groups.last_group[0] as usize].list[..1],
            &[carrier_o]
        );
        assert_eq!(sim.world.orders(row), &before_order);
        assert_eq!(sim.world.random.state(), before_rng);
    }
}

#[test]
fn reached_cast_and_special_anim_malformed_states_fail_closed_before_selection_publish() {
    let spell = 0x292;
    let cases = [
        (
            cast_order(spell),
            AirBusyAuthorityError::MissingSpell(spell),
        ),
        (
            Order {
                kind: OrderIndex::CastSpell,
                economy: None,
                ..Order::default()
            },
            AirBusyAuthorityError::MalformedCastOrder,
        ),
        (
            Order {
                kind: OrderIndex::SpecialAnim,
                special_anim: None,
                ..Order::default()
            },
            AirBusyAuthorityError::MalformedSpecialAnim,
        ),
    ];
    for (order, error) in cases {
        let (mut sim, plane, _carrier, _target, packet) = fixture();
        let row = sim.world.row_of(plane).unwrap();
        sim.world.orders_mut(row).replace(order);
        let before_groups = sim.groups.clone();
        let before_cache = sim.command_package_state.clone();
        let before_order = sim.world.orders(row).clone();
        assert_eq!(
            sim.process_air_group_package(0, 0x93, &packet),
            Err(
                don_sim::systems::canonical_air_group_host::CanonicalAirPackageError::BusyAuthority(
                    error,
                ),
            )
        );
        assert_eq!(sim.groups.list, before_groups.list);
        assert_eq!(sim.command_package_state, before_cache);
        assert_eq!(sim.world.orders(row), &before_order);
    }
}
