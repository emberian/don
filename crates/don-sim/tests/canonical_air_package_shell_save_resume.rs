// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact retail package shell -> cached Build LaunchPatrol -> current AIR/STRAFE resume.

use don_sim::command::air_launch_receivers::MISSILE_OBJECT_MASK;
use don_sim::order::{Order, OrderIndex, ORDER_GROUP};
use don_sim::systems::air_group_action_transaction::{
    AirGroupActionPlan, AirTransactionStatus, CommandPackagePosition,
};
use don_sim::systems::canonical_air_group_host::{AirGroupRuntimeAuthority, AirGroupUnitAuthority};
use don_sim::systems::canonical_air_package_shell::{
    commit_canonical_air_replay_batch, commit_canonical_air_replay_package,
    prepare_canonical_air_replay_batch, prepare_canonical_air_replay_package,
    AirReplayPackageIdentity, AirReplayShellCommand, CanonicalAirPackageShellError,
};
use don_sim::systems::canonical_flight_strafe_host::{
    FlightStrafeActorEffect, FlightStrafeSkipReason,
};
use don_sim::systems::canonical_group_move_host::{
    BuildSelectionAuthority, BuildSelectionIdentity, CachedSelection, CommandPackageState,
    GroupMoveAuthority, MoveMemberAuthority,
};
use don_sim::systems::canonical_strafe_runtime::{
    StrafeFireMode, StrafeRuntimeAuthority, StrafeSearchObservation, StrafeTypeFacts,
};
use don_sim::systems::groups_guys::FormationMember;
use don_sim::systems::patrol::StrafeOrder;
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
const RETAIL_TRIPLE_IDENTITY: AirReplayPackageIdentity = AirReplayPackageIdentity {
    game_frame: 53_053,
    package_serial: 8_853,
    play: 1,
};
const RETAIL_FLIGHT_AIR_IDENTITY: AirReplayPackageIdentity = AirReplayPackageIdentity {
    game_frame: 131_771,
    package_serial: 21_991,
    play: 3,
};
const RETAIL_UNIT_FLIGHT_IDENTITY: AirReplayPackageIdentity = AirReplayPackageIdentity {
    game_frame: 43_963,
    package_serial: 44_295,
    play: 0,
};
const RETAIL_BUILD_FLIGHT_IDENTITY: AirReplayPackageIdentity = AirReplayPackageIdentity {
    game_frame: 54_211,
    package_serial: 54_617,
    play: 0,
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

fn retail_triple_commands() -> Vec<Vec<u8>> {
    [
        "4f0008030000030000",
        "000002",
        "0b3d9700003293000002000000000000000000000000000000",
        "000002",
        "0b3d9700003293000002000000000000000000000000000000",
        "000002",
        "0b3d9700003293000002000000000000000000000000000000",
        "3973a3bbc3e077860b01000000d57d28869bce92f336b1e1442dcfc45b842dde4b68127735010000009ce18c5f3887b6a70431ba12db1c2db801000400c8de273d",
        "4a48001000000000000000",
        "48049a940000398a0000",
    ]
    .map(hex)
    .to_vec()
}

fn retail_flight_air_commands() -> Vec<Vec<u8>> {
    [
        "000003",
        "1c36070000000000000000000000000000000000000a000000",
        "4f0008020000020000",
        "000003",
        "0b5fcb00009d90000002000000000000000000000000000000",
        "3a0e9465e403",
        "4a35001500000000000000",
        "4804afce00005ea30000",
    ]
    .map(hex)
    .to_vec()
}

fn retail_unit_flight_commands() -> Vec<Vec<u8>> {
    [
        "0016000e001500400061009700bd0055007800840088009100a900b600bf0090008300ca00cb00cd00ce00cf00d000",
        "1c20080000000000000000000000000000000000000a000000",
    ]
    .map(hex)
    .to_vec()
}

fn retail_build_flight_commands() -> Vec<Vec<u8>> {
    [
        "000000",
        "1c21080000030000000000000000000000000000000a000000",
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

fn build_record(who: u8, o: i16, uid: u16, position: (i32, i32), child: Option<i16>) -> BuildData {
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
    build.who = who;
    build.other[production::off::OBJECT_ID..production::off::OBJECT_ID + 2]
        .copy_from_slice(&o.to_le_bytes());
    build.other[production::off::X_INTERNAL..production::off::X_INTERNAL + 4]
        .copy_from_slice(&(position.0 ^ 0x63637).to_le_bytes());
    build.other[production::off::Y_INTERNAL..production::off::Y_INTERNAL + 4]
        .copy_from_slice(&(position.1 ^ 0x63637).to_le_bytes());
    build.other[0x2a..0x2c].copy_from_slice(&(-1i16).to_le_bytes());
    if let Some(child) = child {
        build.other[0x28..0x2a].copy_from_slice(&child.to_le_bytes());
        build.other[0x3e] = who;
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
                        who: 5,
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
            sim.spawn_build(5, build_record(5, o, 0x6100 + row as u16, position, child)),
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

fn batch_member(handle: Handle, plane: bool) -> MoveMemberAuthority {
    let mut member = move_member(handle);
    member.is_plane = plane;
    member.domain = if plane { 2 } else { 1 };
    member
}

fn install_batch_package_authority(sim: &mut Sim, carriers: &[Handle], planes: &[Handle]) {
    sim.replace_group_move_authority(GroupMoveAuthority {
        revision: 0x8853,
        composition_digest: [0x53; 32],
        destination_is_water: false,
        force_formation_facing_zero: false,
        members: carriers
            .iter()
            .copied()
            .map(|handle| batch_member(handle, false))
            .chain(
                planes
                    .iter()
                    .copied()
                    .map(|handle| batch_member(handle, true)),
            )
            .collect(),
    });
    sim.replace_air_group_authority(AirGroupRuntimeAuthority {
        revision: 0x53053,
        composition_digest: [0x85; 32],
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
        builds: Vec::new(),
        busy_spells: Vec::new(),
    });
}

fn triple_batch_fixture() -> (Sim, Vec<Handle>, Vec<Handle>, Handle) {
    let mut sim = Sim::new(0x18_8853, 256);
    let mut players = PlayerTable::new();
    players.seat(1, 1, 2, 0);
    sim.players = Some(players);
    sim.world.frame = RETAIL_TRIPLE_IDENTITY.game_frame;
    sim.vic_match.frame = RETAIL_TRIPLE_IDENTITY.game_frame;

    let target_coord = (38_717, 37_682);
    let mut owner_units = Vec::new();
    for o in 0..150i16 {
        let position = match o {
            134 => (target_coord.0 - 100, target_coord.1),
            117 => (target_coord.0 - 1_000, target_coord.1),
            125 => (target_coord.0 - 2_000, target_coord.1),
            147 => (target_coord.0 - 100, target_coord.1),
            148 => (target_coord.0 - 1_000, target_coord.1),
            149 => (target_coord.0 - 2_000, target_coord.1),
            _ => (5_000 + i32::from(o) * 10, 5_000),
        };
        let type_id = if o >= 147 { PLANE_TYPE } else { 90 };
        let handle = sim
            .spawn_unit(2, type_id, position.0, position.1, 4)
            .unwrap();
        let row = sim.world.row_of(handle).unwrap();
        assert_eq!(sim.world.units.o()[row], o);
        owner_units.push(handle);
    }
    let target = sim
        .spawn_unit(3, 91, target_coord.0, target_coord.1, 4)
        .unwrap();
    let carrier_os = [134i16, 117, 125];
    let plane_os = [147i16, 148, 149];
    let carriers: Vec<_> = carrier_os
        .iter()
        .map(|&o| owner_units[o as usize])
        .collect();
    let planes: Vec<_> = plane_os.iter().map(|&o| owner_units[o as usize]).collect();
    for (&carrier_o, &plane_o) in carrier_os.iter().zip(&plane_os) {
        let carrier_row = sim.world.row_of(owner_units[carrier_o as usize]).unwrap();
        let plane_row = sim.world.row_of(owner_units[plane_o as usize]).unwrap();
        sim.world.units.inside_down_mut()[carrier_row] = plane_o;
        sim.world.units.inside_down_who_mut()[carrier_row] = 2;
        sim.world.units.inside_up_mut()[plane_row] = carrier_o;
        sim.world.units.inside_up_who_mut()[plane_row] = 2;
        sim.world.units.inside_down_mut()[plane_row] = -1;
        sim.world.units.inside_down_who_mut()[plane_row] = -1;
    }
    for handle in carriers.iter().chain(&planes) {
        let row = sim.world.row_of(*handle).unwrap();
        sim.world.units.group_mut()[row] = -1;
        sim.world.units.o_down_mut()[row] = -1;
        sim.world.units.form_mut()[row] = 0;
        sim.world.units.form_mod_mut()[row] = 50;
    }

    let mut cache: [Vec<CachedSelection>; 8] = std::array::from_fn(|_| Vec::new());
    cache[1] = carrier_os
        .iter()
        .map(|&o| {
            let row = sim.world.row_of(owner_units[o as usize]).unwrap();
            CachedSelection {
                o,
                uid: sim.world.units.get_uid(row),
            }
        })
        .collect();
    sim.command_package_state = CommandPackageState::from_saved_selections(cache).unwrap();
    install_batch_package_authority(&mut sim, &carriers, &planes);
    (sim, carriers, planes, target)
}

fn flight_then_launch_fixture() -> (Sim, Vec<Handle>, Vec<i16>, Handle) {
    let mut sim = Sim::new(0x1c_21991, 256);
    let mut players = PlayerTable::new();
    players.seat(3, 1, 3, 0);
    sim.players = Some(players);
    sim.world.frame = RETAIL_FLIGHT_AIR_IDENTITY.game_frame;
    sim.vic_match.frame = RETAIL_FLIGHT_AIR_IDENTITY.game_frame;

    // The exact Flight target is owner 0, object 1846. Object addresses are per owner,
    // so retain the complete dense prefix instead of manufacturing that identity.
    let mut target = None;
    for o in 0..=1_846i16 {
        let handle = sim
            .spawn_unit(0, 91, 28_000 + i32::from(o % 100), 28_000, 4)
            .unwrap();
        assert_eq!(sim.world.units.o()[sim.world.row_of(handle).unwrap()], o);
        target = Some(handle);
    }
    let target = target.unwrap();

    let planes: Vec<_> = (0..4)
        .map(|index| {
            sim.spawn_unit(3, PLANE_TYPE, 52_000 + index * 100, 37_000, 4)
                .unwrap()
        })
        .collect();
    let builds = vec![2_280i16, 2_281, 2_282, 2_283];
    for (plane, &build_o) in planes.iter().zip(&builds) {
        let row = sim.world.row_of(*plane).unwrap();
        sim.world.units.group_mut()[row] = -1;
        sim.world.units.o_down_mut()[row] = -1;
        sim.world.units.inside_up_mut()[row] = build_o;
        sim.world.units.inside_up_who_mut()[row] = 3;
        sim.world.units.inside_down_mut()[row] = -1;
        sim.world.units.inside_down_who_mut()[row] = -1;
    }
    for row in 0..=283usize {
        let o = 2_000 + row as i16;
        let selected = builds.iter().position(|&candidate| candidate == o);
        let child = selected.map(|index| {
            let row = sim.world.row_of(planes[index]).unwrap();
            sim.world.units.o()[row]
        });
        let position = selected.map_or((20_000 + row as i32, 20_000), |index| {
            (51_900 + index as i32 * 150, 36_900)
        });
        assert_eq!(
            sim.spawn_build(3, build_record(3, o, 0x7100 + row as u16, position, child)),
            row,
        );
    }

    let mut cache: [Vec<CachedSelection>; 8] = std::array::from_fn(|_| Vec::new());
    cache[3] = builds
        .iter()
        .map(|&o| CachedSelection {
            o,
            uid: sim.builds[(o - 2_000) as usize].uid,
        })
        .collect();
    sim.command_package_state = CommandPackageState::from_saved_selections(cache).unwrap();
    sim.replace_group_move_authority(GroupMoveAuthority {
        revision: 0x21991,
        composition_digest: [0x91; 32],
        destination_is_water: false,
        force_formation_facing_zero: false,
        members: planes.iter().copied().map(move_member).collect(),
    });
    sim.replace_air_group_authority(AirGroupRuntimeAuthority {
        revision: 0x131771,
        composition_digest: [0x77; 32],
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
                        who: 3,
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
    (sim, planes, builds, target)
}

fn unit_flight_fixture() -> (Sim, Vec<Handle>) {
    const SELECTED: [usize; 22] = [
        14, 21, 64, 97, 151, 189, 85, 120, 132, 136, 145, 169, 182, 191, 144, 131, 202, 203, 205,
        206, 207, 208,
    ];
    let mut sim = Sim::new(0x1c_44295, 256);
    let mut players = PlayerTable::new();
    players.seat(0, 1, 0, 0);
    sim.players = Some(players);
    sim.world.frame = RETAIL_UNIT_FLIGHT_IDENTITY.game_frame;
    sim.vic_match.frame = RETAIL_UNIT_FLIGHT_IDENTITY.game_frame;

    let units: Vec<_> = (0..=208)
        .map(|o| {
            sim.spawn_unit(0, PLANE_TYPE, 30_000 + o * 16, 32_000, 4)
                .unwrap()
        })
        .collect();
    for row in 0..=80usize {
        let o = 2_000 + row as i16;
        assert_eq!(
            sim.spawn_build(
                0,
                build_record(
                    0,
                    o,
                    0x5100 + row as u16,
                    (55_000 + row as i32, 41_000),
                    None
                ),
            ),
            row,
        );
    }
    let selected = SELECTED.map(|o| units[o]);
    for handle in selected {
        let row = sim.world.row_of(handle).unwrap();
        sim.world.units.group_mut()[row] = -1;
        sim.world.units.o_down_mut()[row] = -1;
        let payload = StrafeOrder {
            target_o: 2_000,
            target_who: 0,
            target_uid: sim.builds[0].uid,
            air: don_sim::systems::air::AirOrderWalk {
                oxx: -1,
                whose: -1,
                cruising_alt: 0x640,
                ..Default::default()
            },
            xx: sim.builds[0].position().0,
            yy: sim.builds[0].position().1,
            ..Default::default()
        };
        sim.world
            .orders_mut(row)
            .replace(Order::strafe(payload, false).unwrap());
    }
    install_package_authority(&mut sim, &selected, &[]);
    (sim, selected.to_vec())
}

fn build_flight_fixture() -> Sim {
    let mut sim = Sim::new(0x1c_54617, 16);
    let mut players = PlayerTable::new();
    players.seat(0, 1, 0, 0);
    sim.players = Some(players);
    sim.world.frame = RETAIL_BUILD_FLIGHT_IDENTITY.game_frame;
    sim.vic_match.frame = RETAIL_BUILD_FLIGHT_IDENTITY.game_frame;

    for local in 0..=64u16 {
        assert_eq!(
            sim.spawn_build(
                0,
                build_record(
                    0,
                    2_000 + local as i16,
                    0x6400 + local,
                    (31_000 + i32::from(local), 32_000),
                    None,
                ),
            ),
            usize::from(local),
        );
    }
    for local in 0..=81u16 {
        assert_eq!(
            sim.spawn_build(
                3,
                build_record(
                    3,
                    2_000 + local as i16,
                    0x8100 + local,
                    (41_000 + i32::from(local), 42_000),
                    None,
                ),
            ),
            65 + usize::from(local),
        );
    }
    const SELECTED_ROW: usize = 64;
    let mut cache: [Vec<CachedSelection>; 8] = std::array::from_fn(|_| Vec::new());
    cache[0] = vec![CachedSelection {
        o: 2_064,
        uid: sim.builds[SELECTED_ROW].uid,
    }];
    sim.command_package_state = CommandPackageState::from_saved_selections(cache).unwrap();
    sim.replace_group_move_authority(GroupMoveAuthority {
        revision: 0x54617,
        composition_digest: [0x17; 32],
        destination_is_water: false,
        force_formation_facing_zero: false,
        members: Vec::new(),
    });
    sim.replace_air_group_authority(AirGroupRuntimeAuthority {
        revision: 0x54211,
        composition_digest: [0x11; 32],
        units: Vec::new(),
        builds: vec![BuildSelectionAuthority {
            identity: BuildSelectionIdentity {
                row: SELECTED_ROW as u32,
                who: 0,
                o: 2_064,
                uid: sim.builds[SELECTED_ROW].uid,
            },
            role: 0x200,
            is_airbase: true,
        }],
        busy_spells: Vec::new(),
    });
    sim
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
fn exact_retail_cached_airbase_flight_to_build_is_no_action_and_save_resumable() {
    let mut sim = build_flight_fixture();
    let commands = retail_build_flight_commands();
    let before_rng = sim.world.random.state();
    let before_builds = sim.builds.iter().map(BuildData::image).collect::<Vec<_>>();

    let receipt = sim
        .process_air_replay_batch(RETAIL_BUILD_FLIGHT_IDENTITY, &commands)
        .unwrap();
    assert!(receipt.validates());
    assert_eq!(receipt.command_image, commands);
    assert_eq!(receipt.flight_no_action.len(), 1);
    assert_eq!(receipt.flight_strafe.len(), 0);
    assert_eq!(receipt.air.len(), 0);
    let flight = &receipt.flight_no_action[0];
    assert_eq!(flight.position.group_command_index, 0);
    assert_eq!(flight.target.address(), (3, 2_081));
    assert!(matches!(
        flight.target.generation,
        don_sim::systems::air_group_action_transaction::CanonicalObjectGeneration::BuildRow(146)
    ));
    assert_eq!(flight.target_uid, sim.builds[146].uid);
    assert_eq!(flight.target_position, sim.builds[146].position());
    assert_eq!(flight.selected_airbases.len(), 1);
    assert_eq!(
        (
            flight.selected_airbases[0].who,
            flight.selected_airbases[0].o
        ),
        (0, 2_064)
    );
    assert!(flight.contained_non_missiles.is_empty());
    assert_eq!(sim.world.random.state(), before_rng);
    assert_eq!(
        sim.builds.iter().map(BuildData::image).collect::<Vec<_>>(),
        before_builds
    );

    let saved = save_sim(&sim).unwrap();
    let loaded = load_sim(&saved).unwrap();
    assert_eq!(save_sim(&loaded).unwrap(), saved);
}

#[test]
fn changed_no_action_build_target_rolls_back_cached_airbase_selection() {
    let mut sim = build_flight_fixture();
    let commands = retail_build_flight_commands();
    let players = std::array::from_fn(|play| (play == 0).then_some(0));
    let prepared = prepare_canonical_air_replay_batch(
        &sim.world,
        &sim.builds,
        &sim.groups,
        &sim.paths,
        &sim.command_package_state,
        &sim.group_move_authority,
        &sim.air_group_authority,
        &sim.scenario_ignore_orders,
        &players,
        RETAIL_BUILD_FLIGHT_IDENTITY,
        &commands,
    )
    .unwrap();
    let before_groups = sim.groups.clone();
    let before_cache = sim.command_package_state.clone();
    sim.builds[146].uid ^= 1;

    assert_eq!(
        commit_canonical_air_replay_batch(
            &mut sim.world,
            &sim.builds,
            &mut sim.groups,
            &mut sim.paths,
            &mut sim.command_package_state,
            &sim.group_move_authority,
            &sim.air_group_authority,
            &sim.scenario_ignore_orders,
            &players,
            &commands,
            prepared,
        ),
        Err(CanonicalAirPackageShellError::StaleCanonicalState),
    );
    assert_eq!(sim.groups.list, before_groups.list);
    assert_eq!(sim.groups.last_group, before_groups.last_group);
    assert_eq!(sim.groups.proc_group, before_groups.proc_group);
    assert_eq!(sim.command_package_state, before_cache);
}

#[test]
fn exact_retail_unit_flight_retargets_current_strafes_and_round_trips_save() {
    let (mut sim, planes) = unit_flight_fixture();
    let commands = retail_unit_flight_commands();
    let before_rng = sim.world.random.state();

    let receipt = sim
        .process_air_replay_batch(RETAIL_UNIT_FLIGHT_IDENTITY, &commands)
        .unwrap();
    assert!(receipt.validates());
    assert_eq!(receipt.flight_no_action.len(), 0);
    assert_eq!(receipt.flight_strafe.len(), 1);
    assert_eq!(receipt.air.len(), 0);
    let flight = &receipt.flight_strafe[0];
    assert_eq!(flight.actors.len(), 22);
    assert!(flight.skipped_actors.is_empty());
    assert_eq!(flight.target.address(), (0, 2_080));
    assert_eq!(flight.target_uid, sim.builds[80].uid);
    assert_eq!(flight.target_position, sim.builds[80].position());
    assert!(flight
        .actors
        .iter()
        .all(|actor| actor.effect == FlightStrafeActorEffect::RetargetedAndArmed));
    assert_eq!(sim.world.random.state(), before_rng);
    for plane in &planes {
        let row = sim.world.row_of(*plane).unwrap();
        let current = sim.world.orders(row).current().unwrap();
        let strafe = current.strafe.as_ref().unwrap();
        assert_eq!(current.kind, OrderIndex::Strafe);
        assert_eq!((strafe.target_who, strafe.target_o), (0, 2_080));
        assert_eq!(strafe.target_uid, sim.builds[80].uid);
        assert_eq!((strafe.xx, strafe.yy), sim.builds[80].position());
        assert_eq!(strafe.mandatory, 1);
        assert_eq!(strafe.air.returning, 0);
    }

    let saved = save_sim(&sim).unwrap();
    let loaded = load_sim(&saved).unwrap();
    assert_eq!(save_sim(&loaded).unwrap(), saved);
    for plane in &planes {
        let row = sim.world.row_of(*plane).unwrap();
        assert_eq!(sim.world.orders(row), loaded.world.orders(row));
    }
}

#[test]
fn current_strafe_fuel_exhaustion_retargets_only_the_native_target_fields() {
    let (mut sim, planes) = unit_flight_fixture();
    let exhausted = planes[0];
    let exhausted_row = sim.world.row_of(exhausted).unwrap();
    sim.world.units.mana_burn_mut()[exhausted_row] = 1_000;
    let before = sim.world.orders(exhausted_row).current().unwrap().clone();
    let before_rng = sim.world.random.state();

    let receipt = sim
        .process_air_replay_batch(RETAIL_UNIT_FLIGHT_IDENTITY, &retail_unit_flight_commands())
        .unwrap();
    assert!(receipt.validates());
    let flight = &receipt.flight_strafe[0];
    let actor = flight
        .actors
        .iter()
        .find(|actor| actor.identity.handle == exhausted)
        .unwrap();
    assert_eq!(
        actor.effect,
        FlightStrafeActorEffect::RetargetedFuelExhausted
    );
    let after = sim.world.orders(exhausted_row).current().unwrap();
    let payload = after.strafe.as_ref().unwrap();
    let before_payload = before.strafe.as_ref().unwrap();
    assert_eq!((after.target_who, after.target_o), (0, 2_080));
    assert_eq!(after.target_uid, sim.builds[80].uid);
    assert_eq!((payload.target_who, payload.target_o), (0, 2_080));
    assert_eq!(payload.target_uid, sim.builds[80].uid);
    assert_eq!((payload.xx, payload.yy), sim.builds[80].position());
    assert_eq!(payload.mandatory, before_payload.mandatory);
    assert_eq!(payload.air.returning, before_payload.air.returning);
    assert_eq!(after.flags, before.flags);
    assert_eq!(after.flags & ORDER_GROUP, 0);
    assert_eq!(sim.world.random.state(), before_rng);

    let saved = save_sim(&sim).unwrap();
    let loaded = load_sim(&saved).unwrap();
    assert_eq!(
        loaded.world.orders(exhausted_row),
        sim.world.orders(exhausted_row)
    );
}

#[test]
fn current_strafe_missile_mask_skips_that_actor_and_retargets_its_peers() {
    let (mut sim, planes) = unit_flight_fixture();
    let masked = planes[0];
    sim.air_group_authority
        .units
        .iter_mut()
        .find(|unit| unit.handle == masked)
        .unwrap()
        .object_masks = MISSILE_OBJECT_MASK;
    let masked_row = sim.world.row_of(masked).unwrap();
    let before = sim.world.orders(masked_row).clone();

    let receipt = sim
        .process_air_replay_batch(RETAIL_UNIT_FLIGHT_IDENTITY, &retail_unit_flight_commands())
        .unwrap();
    assert!(receipt.validates());
    let flight = &receipt.flight_strafe[0];
    let actor = flight
        .actors
        .iter()
        .find(|actor| actor.identity.handle == masked)
        .unwrap();
    assert_eq!(actor.effect, FlightStrafeActorEffect::SkippedMissileMask);
    assert_eq!(sim.world.orders(masked_row), &before);
    assert_eq!(
        flight
            .actors
            .iter()
            .filter(|actor| actor.effect == FlightStrafeActorEffect::RetargetedAndArmed)
            .count(),
        21
    );
}

#[test]
fn nuclear_group_skips_non_nuclear_actors_before_reading_their_order() {
    let (mut sim, planes) = unit_flight_fixture();
    let nuclear = planes[0];
    let authority = sim
        .air_group_authority
        .units
        .iter_mut()
        .find(|unit| unit.handle == nuclear)
        .unwrap();
    authority.is_nuclear_missile = true;
    authority.object_masks = MISSILE_OBJECT_MASK;
    for plane in &planes[1..] {
        let row = sim.world.row_of(*plane).unwrap();
        sim.world.orders_mut(row).clear();
    }
    let expected_orders = planes
        .iter()
        .map(|plane| sim.world.orders(sim.world.row_of(*plane).unwrap()).clone())
        .collect::<Vec<_>>();

    let receipt = sim
        .process_air_replay_batch(RETAIL_UNIT_FLIGHT_IDENTITY, &retail_unit_flight_commands())
        .unwrap();
    assert!(receipt.validates());
    let flight = &receipt.flight_strafe[0];
    assert_eq!(flight.actors.len(), 1);
    assert_eq!(
        flight.actors[0].effect,
        FlightStrafeActorEffect::SkippedMissileMask
    );
    assert_eq!(flight.skipped_actors.len(), 21);
    assert!(flight
        .skipped_actors
        .iter()
        .all(|actor| { actor.reason == FlightStrafeSkipReason::NonNuclearActorInNuclearGroup }));
    for (plane, expected) in planes.iter().zip(expected_orders) {
        assert_eq!(
            sim.world.orders(sim.world.row_of(*plane).unwrap()),
            &expected
        );
    }
}

#[test]
fn stale_flight_build_target_rejects_without_publishing_any_actor_retarget() {
    let (mut sim, planes) = unit_flight_fixture();
    let commands = retail_unit_flight_commands();
    let players = std::array::from_fn(|play| (play == 0).then_some(0));
    let prepared = prepare_canonical_air_replay_batch(
        &sim.world,
        &sim.builds,
        &sim.groups,
        &sim.paths,
        &sim.command_package_state,
        &sim.group_move_authority,
        &sim.air_group_authority,
        &sim.scenario_ignore_orders,
        &players,
        RETAIL_UNIT_FLIGHT_IDENTITY,
        &commands,
    )
    .unwrap();
    let before_orders = planes
        .iter()
        .map(|plane| sim.world.orders(sim.world.row_of(*plane).unwrap()).clone())
        .collect::<Vec<_>>();
    sim.builds[80].uid ^= 1;

    assert_eq!(
        commit_canonical_air_replay_batch(
            &mut sim.world,
            &sim.builds,
            &mut sim.groups,
            &mut sim.paths,
            &mut sim.command_package_state,
            &sim.group_move_authority,
            &sim.air_group_authority,
            &sim.scenario_ignore_orders,
            &players,
            &commands,
            prepared,
        ),
        Err(CanonicalAirPackageShellError::StaleCanonicalState),
    );
    for (plane, before) in planes.iter().zip(before_orders) {
        assert_eq!(sim.world.orders(sim.world.row_of(*plane).unwrap()), &before);
    }
}

#[test]
fn flight_fresh_install_and_modifier_branches_remain_explicit_boundaries() {
    let (mut sim, planes) = unit_flight_fixture();
    let first_row = sim.world.row_of(planes[0]).unwrap();
    sim.world.orders_mut(first_row).clear();
    let before_world = sim.world.digest();
    let before_groups = sim.groups.clone();
    let before_cache = sim.command_package_state.clone();
    assert!(matches!(
        sim.process_air_replay_batch(
            RETAIL_UNIT_FLIGHT_IDENTITY,
            &retail_unit_flight_commands(),
        ),
        Err(CanonicalAirPackageShellError::FlightStrafe(
            don_sim::systems::canonical_flight_strafe_host::CanonicalFlightStrafeError::CurrentOrderNotStrafe(_)
        )),
    ));
    assert_eq!(sim.world.digest(), before_world);
    assert_eq!(sim.groups.list, before_groups.list);
    assert_eq!(sim.command_package_state, before_cache);

    let (mut sim, _planes) = unit_flight_fixture();
    let mut shifted = retail_unit_flight_commands();
    shifted[1][9..13].copy_from_slice(&1i32.to_le_bytes());
    assert!(matches!(
        sim.process_air_replay_batch(RETAIL_UNIT_FLIGHT_IDENTITY, &shifted),
        Err(CanonicalAirPackageShellError::Flight(
            don_sim::systems::canonical_air_package_shell::CanonicalFlightNoActionError::UnsupportedRequest(_)
        )),
    ));
}

#[test]
fn exact_retail_flight_no_action_then_launch_is_atomic_and_save_resumable() {
    let (mut control, planes, builds, target) = flight_then_launch_fixture();
    let commands = retail_flight_air_commands();
    let before_rng = control.world.random.state();
    let before_orders = planes
        .iter()
        .map(|&plane| {
            let row = control.world.row_of(plane).unwrap();
            control.world.orders(row).clone()
        })
        .collect::<Vec<_>>();

    let receipt = control
        .process_air_replay_batch(RETAIL_FLIGHT_AIR_IDENTITY, &commands)
        .unwrap();
    assert!(receipt.validates());
    assert_eq!(receipt.command_image, commands);
    assert_eq!(receipt.flight_no_action.len(), 1);
    assert_eq!(receipt.air.len(), 1);
    let flight = &receipt.flight_no_action[0];
    assert_eq!(flight.position.group_command_index, 0);
    assert_eq!(
        (flight.request.target_who, flight.request.target_o),
        (0, 1_846)
    );
    assert_eq!(flight.request.orders, 10);
    assert_eq!(flight.selected_airbases.len(), 4);
    assert_eq!(flight.contained_non_missiles.len(), 4);
    assert_eq!(
        flight
            .selected_airbases
            .iter()
            .map(|identity| identity.o)
            .collect::<Vec<_>>(),
        builds,
    );
    assert_eq!(receipt.air[0].request.position.group_command_index, 3);
    let AirGroupActionPlan::LaunchPatrol(plan) = receipt.air[0].plan.as_ref().unwrap() else {
        panic!("following opcode 11 must retain its LaunchPatrol plan");
    };
    assert_eq!(plan.installs.len(), 1);
    assert_eq!(control.world.random.state(), before_rng);
    assert_eq!(
        planes
            .iter()
            .filter(|&&plane| {
                let row = control.world.row_of(plane).unwrap();
                control.world.orders(row).order_type() == OrderIndex::AirPatrol
            })
            .count(),
        1,
    );
    // The Flight arm itself installed nothing: only the following single-best LaunchPatrol
    // may differ from the complete pre-package order image.
    assert_eq!(
        planes
            .iter()
            .zip(&before_orders)
            .filter(|(plane, before)| {
                let row = control.world.row_of(**plane).unwrap();
                control.world.orders(row) != *before
            })
            .count(),
        1,
    );

    let bytes = save_sim(&control).unwrap();
    let mut resumed = load_sim(&bytes).unwrap();
    assert_eq!(save_sim(&resumed).unwrap(), bytes);
    install_runtime_authority(&mut control, &planes, target);
    install_runtime_authority(&mut resumed, &planes, target);
    // External group/type facts are intentionally reinstalled after load; the checksum-visible
    // order/cache image must nevertheless resume identically.
    control.do_frame();
    resumed.do_frame();
    assert_air_state(&control, &resumed, &planes);
}

#[test]
fn stale_following_launch_rolls_back_the_preceding_flight_selection() {
    let (mut sim, planes, _builds, _target) = flight_then_launch_fixture();
    let commands = retail_flight_air_commands();
    let players = std::array::from_fn(|play| (play == 3).then_some(3));
    let prepared = prepare_canonical_air_replay_batch(
        &sim.world,
        &sim.builds,
        &sim.groups,
        &sim.paths,
        &sim.command_package_state,
        &sim.group_move_authority,
        &sim.air_group_authority,
        &sim.scenario_ignore_orders,
        &players,
        RETAIL_FLIGHT_AIR_IDENTITY,
        &commands,
    )
    .unwrap();
    let before_groups = sim.groups.clone();
    let before_paths = sim.paths.clone();
    let before_cache = sim.command_package_state.clone();
    let before_rng = sim.world.random.state();

    // Change a contained plane after the complete package was prepared. The package-level
    // world CAS rejects before publishing even the opcode-0 selection from Flight.
    let stale_row = sim.world.row_of(planes[3]).unwrap();
    sim.world.units.mana_burn_mut()[stale_row] = 1;
    assert_eq!(
        commit_canonical_air_replay_batch(
            &mut sim.world,
            &sim.builds,
            &mut sim.groups,
            &mut sim.paths,
            &mut sim.command_package_state,
            &sim.group_move_authority,
            &sim.air_group_authority,
            &sim.scenario_ignore_orders,
            &players,
            &commands,
            prepared,
        ),
        Err(CanonicalAirPackageShellError::StaleCanonicalState),
    );
    assert_eq!(sim.groups.list, before_groups.list);
    assert_eq!(sim.command_package_state, before_cache);
    assert_eq!(sim.paths, before_paths);
    assert_eq!(sim.world.random.state(), before_rng);
    assert!(planes.iter().all(|&plane| {
        let row = sim.world.row_of(plane).unwrap();
        sim.world.orders(row).order_type() == OrderIndex::None
    }));
}

#[test]
fn nuclear_missile_child_refuses_the_narrow_flight_arm_without_selection_mutation() {
    let (mut sim, planes, _builds, _target) = flight_then_launch_fixture();
    sim.air_group_authority.units[0].is_nuclear_missile = true;
    let before_world = sim.world.digest();
    let before_groups = sim.groups.clone();
    let before_paths = sim.paths.clone();
    let before_cache = sim.command_package_state.clone();
    let before_rng = sim.world.random.state();

    assert!(matches!(
        sim.process_air_replay_batch(RETAIL_FLIGHT_AIR_IDENTITY, &retail_flight_air_commands()),
        Err(CanonicalAirPackageShellError::Flight(
            don_sim::systems::canonical_air_package_shell::CanonicalFlightNoActionError::NuclearMissileTail {
                who: 3,
                o: 0,
            }
        )),
    ));
    assert_eq!(sim.world.digest(), before_world);
    assert_eq!(sim.groups.list, before_groups.list);
    assert_eq!(sim.paths, before_paths);
    assert_eq!(sim.command_package_state, before_cache);
    assert_eq!(sim.world.random.state(), before_rng);
    assert!(planes.iter().all(|&plane| {
        let row = sim.world.row_of(plane).unwrap();
        sim.world.orders(row).order_type() == OrderIndex::None
    }));
}

#[test]
fn exact_retail_triple_cached_package_is_atomic_and_current_save_resumable() {
    let (mut control, carriers, planes, target) = triple_batch_fixture();
    let commands = retail_triple_commands();
    let before_rng = control.world.random.state();
    let receipt = control
        .process_air_replay_batch(RETAIL_TRIPLE_IDENTITY, &commands)
        .unwrap();
    assert!(receipt.validates());
    assert_eq!(receipt.identity, RETAIL_TRIPLE_IDENTITY);
    assert_eq!(receipt.command_image, commands);
    assert_eq!(receipt.shell.len(), 4);
    assert_eq!(receipt.air.len(), 3);
    assert_eq!(
        receipt
            .air
            .iter()
            .map(|air| air.request.position.group_command_index)
            .collect::<Vec<_>>(),
        [1, 3, 5],
    );
    assert!(receipt
        .air
        .iter()
        .all(|air| matches!(air.status, AirTransactionStatus::Applied(_))));
    assert_eq!(control.world.random.state(), before_rng);
    for plane in &planes {
        let row = control.world.row_of(*plane).unwrap();
        assert_eq!(
            control.world.orders(row).order_type(),
            OrderIndex::AirPatrol
        );
    }

    let bytes = save_sim(&control).unwrap();
    assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), 21);
    let mut resumed = load_sim(&bytes).unwrap();
    assert_eq!(save_sim(&resumed).unwrap(), bytes);
    install_batch_package_authority(&mut resumed, &carriers, &planes);
    install_runtime_authority(&mut control, &planes, target);
    install_runtime_authority(&mut resumed, &planes, target);

    control.do_frame();
    resumed.do_frame();
    assert_air_state(&control, &resumed, &planes);
    assert!(planes.iter().any(|plane| {
        let row = control.world.row_of(*plane).unwrap();
        control.world.orders(row).order_type() == OrderIndex::Strafe
    }));
}

#[test]
fn third_pair_staleness_rolls_back_the_first_two_pairs() {
    let (mut sim, carriers, planes, _target) = triple_batch_fixture();
    let commands = retail_triple_commands();
    let players = std::array::from_fn(|play| (play == 1).then_some(2));
    let prepared = prepare_canonical_air_replay_batch(
        &sim.world,
        &sim.builds,
        &sim.groups,
        &sim.paths,
        &sim.command_package_state,
        &sim.group_move_authority,
        &sim.air_group_authority,
        &sim.scenario_ignore_orders,
        &players,
        RETAIL_TRIPLE_IDENTITY,
        &commands,
    )
    .unwrap();
    let before_world = sim.world.digest();
    let before_groups = sim.groups.clone();
    let before_paths = sim.paths.clone();
    let before_cache = sim.command_package_state.clone();
    let before_rng = sim.world.random.state();

    // The third selected container changes after package prepare. The package-level CAS must
    // reject before any of the first two installs or cache revisions become observable.
    let third_row = sim.world.row_of(carriers[2]).unwrap();
    sim.world.units.inside_down_mut()[third_row] = -1;
    assert_eq!(
        commit_canonical_air_replay_batch(
            &mut sim.world,
            &sim.builds,
            &mut sim.groups,
            &mut sim.paths,
            &mut sim.command_package_state,
            &sim.group_move_authority,
            &sim.air_group_authority,
            &sim.scenario_ignore_orders,
            &players,
            &commands,
            prepared,
        ),
        Err(CanonicalAirPackageShellError::StaleCanonicalState),
    );
    // The caller's intervening mutation remains; package publication itself is absent.
    assert_eq!(sim.world.digest(), before_world);
    assert_eq!(sim.groups.list.len(), before_groups.list.len());
    assert_eq!(sim.groups.last_group, before_groups.last_group);
    assert_eq!(sim.groups.proc_group, before_groups.proc_group);
    assert_eq!(sim.paths, before_paths);
    assert_eq!(sim.command_package_state, before_cache);
    assert_eq!(sim.world.random.state(), before_rng);
    for plane in planes {
        let row = sim.world.row_of(plane).unwrap();
        assert_eq!(sim.world.orders(row).order_type(), OrderIndex::None);
    }
}

#[test]
fn exact_cached_force_all_shell_current_reload_resumes_four_build_home_aircraft() {
    let (control, planes, builds, target) = fixture();
    let pre_package = save_sim(&control).unwrap();
    assert_eq!(
        u32::from_le_bytes(pre_package[24..28].try_into().unwrap()),
        21
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
    assert_eq!(u32::from_le_bytes(applied[24..28].try_into().unwrap()), 21);
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
