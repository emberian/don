// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact retail package shell -> cached Build LaunchPatrol -> v17 AIR/STRAFE resume.

use don_sim::order::OrderIndex;
use don_sim::systems::air_group_action_transaction::{
    AirGroupActionPlan, AirTransactionStatus, CommandPackagePosition,
};
use don_sim::systems::canonical_air_group_host::{AirGroupRuntimeAuthority, AirGroupUnitAuthority};
use don_sim::systems::canonical_air_package_shell::{
    commit_canonical_air_replay_package, prepare_canonical_air_replay_package,
    AirReplayShellCommand, CanonicalAirPackageShellError,
};
use don_sim::systems::canonical_group_move_host::{
    BuildSelectionAuthority, BuildSelectionIdentity, CachedSelection, CommandPackageState,
    GroupMoveAuthority, MoveMemberAuthority,
};
use don_sim::systems::canonical_strafe_runtime::{
    StrafeFireMode, StrafeRuntimeAuthority, StrafeSearchObservation, StrafeTypeFacts,
};
use don_sim::systems::groups_guys::FormationMember;
use don_sim::systems::production::{self, BuildData};
use don_sim::systems::save_load::{load_sim, save_sim};
use don_sim::systems::strafe_order_frontier::{AirTargetSearchKind, ObjectIdentity};
use don_sim::tick::lifecycle_host::PlayerTable;
use don_sim::tick::Sim;
use don_sim::Handle;

const PLANE_TYPE: i32 = 77;
const RETAIL_POSITION: CommandPackagePosition = CommandPackagePosition {
    game_frame: 65_929,
    package_serial: 10_989,
    play: 2,
    group_command_index: 0,
    action_command_index: 1,
};

fn hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).expect("ASCII fixture"), 16)
                .expect("hex fixture")
        })
        .collect()
}

fn retail_commands() -> Vec<Vec<u8>> {
    [
        "000005",
        "0be90d01007523000001000000010000000000000000000000",
        "4f0008010000010000",
        "390a68becf7d2f714a01000000ca9d89bd7479fcd0d348ea71dd7f2b0b2c1d3cbd092a209101000000e5e3811761ae81b40431ba12e01deeb601000400d79fd709",
        "4a48001000000000000000",
        "4804460e01000d130000",
    ]
    .map(hex)
    .to_vec()
}

fn move_member(handle: Handle) -> MoveMemberAuthority {
    MoveMemberAuthority {
        handle,
        role: 0x40000,
        on_map: true,
        is_captain: true,
        can_move: true,
        can_install_order: true,
        is_plane: true,
        domain: 2,
        unit_flags: 0,
        speed: 16,
        admits_unsplit_move_near: true,
        land_formation: FormationMember::default(),
        water_formation: FormationMember::default(),
    }
}

fn build_record(o: i16, uid: u16, position: (i32, i32), child: Option<i16>) -> BuildData {
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
    build.who = 5;
    build.other[production::off::OBJECT_ID..production::off::OBJECT_ID + 2]
        .copy_from_slice(&o.to_le_bytes());
    build.other[production::off::X_INTERNAL..production::off::X_INTERNAL + 4]
        .copy_from_slice(&(position.0 ^ 0x63637).to_le_bytes());
    build.other[production::off::Y_INTERNAL..production::off::Y_INTERNAL + 4]
        .copy_from_slice(&(position.1 ^ 0x63637).to_le_bytes());
    build.other[0x2a..0x2c].copy_from_slice(&(-1i16).to_le_bytes());
    if let Some(child) = child {
        build.other[0x28..0x2a].copy_from_slice(&child.to_le_bytes());
        build.other[0x3e] = 5;
    } else {
        build.other[0x28..0x2a].copy_from_slice(&(-1i16).to_le_bytes());
        build.other[0x3e] = 0xff;
    }
    build
}

fn identity(sim: &Sim, handle: Handle) -> ObjectIdentity {
    let row = sim.world.row_of(handle).unwrap();
    ObjectIdentity {
        o: i32::from(sim.world.units.o()[row]),
        who: i32::from(sim.world.units.get_who(row)),
        uid: sim.world.units.get_uid(row),
    }
}

fn install_package_authority(sim: &mut Sim, planes: &[Handle], builds: &[i16]) {
    sim.replace_group_move_authority(GroupMoveAuthority {
        revision: 0x10989,
        composition_digest: [0x89; 32],
        destination_is_water: false,
        force_formation_facing_zero: false,
        members: planes.iter().copied().map(move_member).collect(),
    });
    sim.replace_air_group_authority(AirGroupRuntimeAuthority {
        revision: 0x65929,
        composition_digest: [0x29; 32],
        units: planes
            .iter()
            .copied()
            .map(|handle| AirGroupUnitAuthority {
                handle,
                object_masks: 0,
                is_biplane: true,
                is_bomber: false,
                is_helicopter: false,
            })
            .collect(),
        builds: builds
            .iter()
            .map(|&o| {
                let row = u32::try_from(o - 2_000).unwrap();
                BuildSelectionAuthority {
                    identity: BuildSelectionIdentity {
                        row,
                        who: 5,
                        o,
                        uid: sim.builds[row as usize].uid,
                    },
                    role: 0x200,
                }
            })
            .collect(),
        busy_spells: Vec::new(),
    });
}

fn install_runtime_authority(sim: &mut Sim, planes: &[Handle], target: Handle) {
    let mut authority = StrafeRuntimeAuthority {
        revision: 0x11_65929,
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
        actor: identity(sim, planes[0]),
        frame: sim.world.frame,
        kind: AirTargetSearchKind::AirFirst,
        result: Some(identity(sim, target)),
    });
    sim.replace_strafe_runtime_authority(authority);
}

fn fixture() -> (Sim, Vec<Handle>, Vec<i16>, Handle) {
    let mut sim = Sim::new(0x11_10989, 32);
    let mut players = PlayerTable::new();
    players.seat(2, 1, 5, 0);
    sim.players = Some(players);
    sim.world.frame = RETAIL_POSITION.game_frame;
    sim.vic_match.frame = RETAIL_POSITION.game_frame;

    for index in 0..7 {
        sim.spawn_unit(5, 90, 30_000 + index * 20, 30_000, 4)
            .unwrap();
    }
    let planes: Vec<_> = (0..4)
        .map(|index| {
            sim.spawn_unit(5, PLANE_TYPE, 68_000 + index * 100, 8_000, 4)
                .unwrap()
        })
        .collect();
    let target = sim.spawn_unit(6, 91, 69_000, 9_000, 4).unwrap();
    let builds = vec![2_095i16, 2_096, 2_097, 2_098];
    let positions = [
        (68_500, 8_500),
        (68_000, 8_000),
        (67_500, 7_500),
        (67_000, 7_000),
    ];
    for (plane, &build_o) in planes.iter().zip(&builds) {
        let row = sim.world.row_of(*plane).unwrap();
        sim.world.units.group_mut()[row] = -1;
        sim.world.units.o_down_mut()[row] = -1;
        sim.world.units.inside_up_mut()[row] = build_o;
        sim.world.units.inside_up_who_mut()[row] = 5;
        sim.world.units.inside_down_mut()[row] = -1;
        sim.world.units.inside_down_who_mut()[row] = -1;
    }
    for row in 0..=98usize {
        let o = 2_000 + row as i16;
        let selected = builds.iter().position(|&candidate| candidate == o);
        let child = selected.map(|index| {
            let row = sim.world.row_of(planes[index]).unwrap();
            sim.world.units.o()[row]
        });
        let position = selected.map_or((20_000 + row as i32, 20_000), |index| positions[index]);
        assert_eq!(
            sim.spawn_build(5, build_record(o, 0x6100 + row as u16, position, child)),
            row,
        );
    }
    let mut cache: [Vec<CachedSelection>; 8] = std::array::from_fn(|_| Vec::new());
    cache[2] = builds
        .iter()
        .map(|&o| CachedSelection {
            o,
            uid: sim.builds[(o - 2_000) as usize].uid,
        })
        .collect();
    sim.command_package_state = CommandPackageState::from_saved_selections(cache).unwrap();
    // The recorded force-all bit must bypass this otherwise-ineligible aircraft.
    let mana_row = sim.world.row_of(planes[1]).unwrap();
    sim.world.units.mana_burn_mut()[mana_row] = 19;
    install_package_authority(&mut sim, &planes, &builds);
    (sim, planes, builds, target)
}

fn assert_air_state(left: &Sim, right: &Sim, planes: &[Handle]) {
    assert_eq!(left.world.frame, right.world.frame);
    assert_eq!(left.world.random.state(), right.world.random.state());
    assert_eq!(
        left.strafe_runtime_authority,
        right.strafe_runtime_authority
    );
    for plane in planes {
        let left_row = left.world.row_of(*plane).unwrap();
        let right_row = right.world.row_of(*plane).unwrap();
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
}

#[test]
fn exact_cached_force_all_shell_v17_reload_resumes_four_build_home_aircraft() {
    let (control, planes, builds, target) = fixture();
    let pre_package = save_sim(&control).unwrap();
    assert_eq!(
        u32::from_le_bytes(pre_package[24..28].try_into().unwrap()),
        17
    );
    let mut control = load_sim(&pre_package).unwrap();
    assert_eq!(save_sim(&control).unwrap(), pre_package);
    install_package_authority(&mut control, &planes, &builds);

    let commands = retail_commands();
    let before_rng = control.world.random.state();
    let receipt = control
        .process_air_replay_package(RETAIL_POSITION, &commands)
        .unwrap();
    assert!(receipt.validates());
    assert_eq!(receipt.command_image, commands);
    assert_eq!(receipt.position, RETAIL_POSITION);
    assert_eq!(receipt.shell.len(), 4);
    assert!(matches!(
        receipt.shell[0],
        AirReplayShellCommand::PlayerSpeed { index: 2, .. }
    ));
    assert!(matches!(
        receipt.shell[1],
        AirReplayShellCommand::CheckSums { index: 3, .. }
    ));
    assert!(matches!(
        receipt.shell[2],
        AirReplayShellCommand::TurnData { index: 4, .. }
    ));
    assert_eq!(
        receipt.shell[3],
        AirReplayShellCommand::Camera {
            index: 5,
            zoom: 4,
            x: 69_190,
            y: 4_877,
        },
    );
    assert!(matches!(
        receipt.air.status,
        AirTransactionStatus::Applied(_)
    ));
    let AirGroupActionPlan::LaunchPatrol(plan) = receipt.air.plan.as_ref().unwrap() else {
        panic!("exact opcode 11 must retain LaunchPatrol plan");
    };
    assert!(plan.launched_any);
    assert_eq!(plan.installs.len(), 4);
    assert_eq!(control.world.random.state(), before_rng);
    for (plane, build_o) in planes.iter().zip(&builds) {
        let row = control.world.row_of(*plane).unwrap();
        assert_eq!(
            control.world.orders(row).order_type(),
            OrderIndex::AirPatrol
        );
        let payload = control
            .world
            .orders(row)
            .current()
            .unwrap()
            .air_patrol
            .as_ref()
            .unwrap();
        assert_eq!(
            (payload.air.home_o, payload.air.home_who),
            (i32::from(*build_o), 5)
        );
    }

    let applied = save_sim(&control).unwrap();
    assert_eq!(u32::from_le_bytes(applied[24..28].try_into().unwrap()), 17);
    let mut resumed = load_sim(&applied).unwrap();
    assert_eq!(save_sim(&resumed).unwrap(), applied);
    install_runtime_authority(&mut control, &planes, target);
    install_runtime_authority(&mut resumed, &planes, target);

    control.do_frame();
    resumed.do_frame();
    assert_air_state(&control, &resumed, &planes);
    let first_row = control.world.row_of(planes[0]).unwrap();
    assert_eq!(
        control.world.orders(first_row).order_type(),
        OrderIndex::Strafe
    );
    for plane in &planes[1..] {
        let row = control.world.row_of(*plane).unwrap();
        assert_eq!(
            control.world.orders(row).order_type(),
            OrderIndex::AirPatrol
        );
    }

    control.do_frame();
    resumed.do_frame();
    assert_air_state(&control, &resumed, &planes);
    assert!(control.last_strafe_error.is_none());
    assert!(control.last_strafe_receipt.is_some());
}

#[test]
fn changed_shell_command_between_prepare_and_commit_rejects_without_sim_mutation() {
    let (mut sim, planes, builds, _target) = fixture();
    let players = std::array::from_fn(|play| (play == 2).then_some(5));
    let mut commands = retail_commands();
    let prepared = prepare_canonical_air_replay_package(
        &sim.world,
        &sim.builds,
        &sim.groups,
        &sim.paths,
        &sim.command_package_state,
        &sim.group_move_authority,
        &sim.air_group_authority,
        &sim.scenario_ignore_orders,
        &players,
        RETAIL_POSITION,
        &commands,
    )
    .unwrap();
    let before_world = sim.world.digest();
    let before_groups = sim.groups.clone();
    let before_paths = sim.paths.clone();
    let before_cache = sim.command_package_state.clone();
    let before_rng = sim.world.random.state();
    commands[2][1] ^= 1;

    assert_eq!(
        commit_canonical_air_replay_package(
            &mut sim.world,
            &sim.builds,
            &mut sim.groups,
            &mut sim.paths,
            &mut sim.command_package_state,
            &sim.group_move_authority,
            &sim.air_group_authority,
            &sim.scenario_ignore_orders,
            &commands,
            prepared,
        ),
        Err(CanonicalAirPackageShellError::StaleCommandImage),
    );
    assert_eq!(sim.world.digest(), before_world);
    assert_eq!(sim.groups.list, before_groups.list);
    assert_eq!(sim.paths, before_paths);
    assert_eq!(sim.command_package_state, before_cache);
    assert_eq!(sim.world.random.state(), before_rng);
    for plane in planes {
        assert_eq!(
            sim.world
                .orders(sim.world.row_of(plane).unwrap())
                .order_type(),
            OrderIndex::None,
        );
    }
    assert_eq!(builds, [2_095, 2_096, 2_097, 2_098]);
}

#[test]
fn shell_rejects_truncation_extra_air_commands_and_nonadjacent_positions() {
    let (sim, _planes, _builds, _target) = fixture();
    let players = std::array::from_fn(|play| (play == 2).then_some(5));
    let prepare = |position, commands: &[Vec<u8>]| {
        prepare_canonical_air_replay_package(
            &sim.world,
            &sim.builds,
            &sim.groups,
            &sim.paths,
            &sim.command_package_state,
            &sim.group_move_authority,
            &sim.air_group_authority,
            &sim.scenario_ignore_orders,
            &players,
            position,
            commands,
        )
    };

    let mut truncated = retail_commands();
    truncated[4].pop();
    assert!(matches!(
        prepare(RETAIL_POSITION, &truncated),
        Err(CanonicalAirPackageShellError::WrongWireSize {
            index: 4,
            opcode: 74,
            expected: 11,
            actual: 10,
        })
    ));

    let mut extra_air = retail_commands();
    extra_air[2] = vec![0, 0, 5];
    assert_eq!(
        prepare(RETAIL_POSITION, &extra_air).unwrap_err(),
        CanonicalAirPackageShellError::UnexpectedAirCommand {
            index: 2,
            opcode: 0,
        },
    );

    let nonadjacent = CommandPackagePosition {
        action_command_index: 2,
        ..RETAIL_POSITION
    };
    assert_eq!(
        prepare(nonadjacent, &retail_commands()).unwrap_err(),
        CanonicalAirPackageShellError::NonAdjacentAirPair {
            group_command_index: 0,
            action_command_index: 2,
        },
    );
}
