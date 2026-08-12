// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact recorded Build-band Scramble selections through the canonical cache/package owner.

use don_sim::order::OrderIndex;
use don_sim::systems::air_group_action_transaction::{AirTransactionStatus, SCRAMBLE_OPCODE};
use don_sim::systems::canonical_air_group_host::{
    commit_canonical_air_package, prepare_canonical_air_package, AirGroupRuntimeAuthority,
    AirGroupUnitAuthority,
};
use don_sim::systems::canonical_group_move_host::{
    BuildSelectionAuthority, BuildSelectionIdentity, CachedSelection, GroupMoveAuthority,
    MoveMemberAuthority,
};
use don_sim::systems::groups_guys::FormationMember;
use don_sim::systems::production::{self, BuildData};
use don_sim::systems::save_load::{load_sim, save_sim};
use don_sim::tick::lifecycle_host::PlayerTable;
use don_sim::tick::Sim;
use don_sim::Handle;

const BUILD_BASE: i16 = 2_000;
const RECORDED_ONE: &[u8] = &[0x00, 0x01, 0x00, 0xdf, 0x07, 0x24];
const RECORDED_TWO: &[u8] = &[0x00, 0x02, 0x00, 0x2a, 0x08, 0x2b, 0x08, 0x24];
const RECORDED_THREE: &[u8] = &[0x00, 0x03, 0x00, 0x2a, 0x08, 0x2b, 0x08, 0x63, 0x08, 0x24];
const CACHED_SCRAMBLE: &[u8] = &[0x00, 0x00, 0x00, SCRAMBLE_OPCODE];

fn plane_authority(handle: Handle) -> MoveMemberAuthority {
    MoveMemberAuthority {
        handle,
        role: 0x40000,
        on_map: false,
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

fn build_record(o: i16, uid: u16, position: (i32, i32), child: Option<(u8, i16)>) -> BuildData {
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
        Some((who, child_o)) => {
            build.other[0x28..0x2a].copy_from_slice(&child_o.to_le_bytes());
            build.other[0x3e] = who;
        }
        None => {
            build.other[0x28..0x2a].copy_from_slice(&(-1i16).to_le_bytes());
            build.other[0x3e] = 0xff;
        }
    }
    build
}

fn install_authorities(sim: &mut Sim, planes: &[Handle], selected: &[i16]) {
    sim.replace_group_move_authority(GroupMoveAuthority {
        revision: 0x6275_696c_64,
        composition_digest: [0x62; 32],
        destination_is_water: false,
        force_formation_facing_zero: false,
        members: planes.iter().copied().map(plane_authority).collect(),
    });
    let builds = selected
        .iter()
        .map(|&o| {
            let row = u32::try_from(i32::from(o - BUILD_BASE)).unwrap();
            let build = &sim.builds[row as usize];
            BuildSelectionAuthority {
                identity: BuildSelectionIdentity {
                    row,
                    who: 0,
                    o,
                    uid: build.uid,
                },
                role: 0x200,
            }
        })
        .collect();
    sim.replace_air_group_authority(AirGroupRuntimeAuthority {
        revision: 0x6275_696c_642d_6169,
        composition_digest: [0xa6; 32],
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
        builds,
        busy_spells: Vec::new(),
    });
}

fn fixture() -> (Sim, Vec<Handle>) {
    let mut sim = Sim::new(0x6275_696c_64, 8);
    let mut players = PlayerTable::new();
    players.seat(0, 1, 0, 0);
    sim.players = Some(players);
    let planes: Vec<_> = (0..4)
        .map(|index| {
            sim.spawn_unit(0, 77 + index, 1_000 + index * 20, 1_200, 4)
                .unwrap()
        })
        .collect();
    let selected = [2_015i16, 2_090, 2_091, 2_147];
    for (plane, &build_o) in planes.iter().zip(&selected) {
        let row = sim.world.row_of(*plane).unwrap();
        sim.world.units.group_mut()[row] = -1;
        sim.world.units.o_down_mut()[row] = -1;
        sim.world.units.inside_up_mut()[row] = build_o;
        sim.world.units.inside_up_who_mut()[row] = 0;
        sim.world.units.inside_down_mut()[row] = -1;
        sim.world.units.inside_down_who_mut()[row] = -1;
    }
    for row in 0..=147usize {
        let o = BUILD_BASE + row as i16;
        let child = selected
            .iter()
            .position(|&candidate| candidate == o)
            .map(|index| {
                let plane_row = sim.world.row_of(planes[index]).unwrap();
                (0, sim.world.units.o()[plane_row])
            });
        let spawned = sim.spawn_build(
            0,
            build_record(o, 0x4000 + row as u16, (4_000 + row as i32, 5_000), child),
        );
        assert_eq!(spawned, row);
    }
    install_authorities(&mut sim, &planes, &selected);
    (sim, planes)
}

fn selected_group(sim: &Sim) -> &don_sim::systems::groups_guys::GroupData {
    &sim.groups.list[sim.groups.last_group[0] as usize]
}

#[test]
fn all_three_recorded_explicit_build_packets_resolve_the_exact_airbase_members() {
    let (mut sim, planes) = fixture();
    for (serial, (packet, expected)) in [
        (RECORDED_ONE, &[2_015i16][..]),
        (RECORDED_TWO, &[2_090i16, 2_091][..]),
        (RECORDED_THREE, &[2_090i16, 2_091, 2_147][..]),
    ]
    .into_iter()
    .enumerate()
    {
        let before_rng = sim.world.random.state();
        let receipt = sim
            .process_air_group_package(0, 0x700 + serial as i32, packet)
            .unwrap();
        assert!(receipt.validates());
        assert!(matches!(receipt.status, AirTransactionStatus::Applied(_)));
        let group = selected_group(&sim);
        assert_eq!(group.buildings, 1);
        assert_eq!(&group.list[..expected.len()], expected);
        assert_eq!(sim.world.random.state(), before_rng);
    }
    for plane in planes {
        let row = sim.world.row_of(plane).unwrap();
        assert_eq!(
            sim.world.units.group()[row],
            -1,
            "contained Unit is not selected"
        );
        assert_eq!(sim.world.orders(row).order_type(), OrderIndex::AirPatrol);
    }
}

#[test]
fn ignored_build_airbase_is_pruned_without_a_fabricated_unit_backlink() {
    let (mut sim, planes) = fixture();
    sim.scenario_ignore_orders.ignore_orders = true;
    sim.scenario_ignore_orders.ignored_by_owner[0] = vec![2_015];
    let build_before = sim.builds[15].image();
    let plane_row = sim.world.row_of(planes[0]).unwrap();
    let order_before = sim.world.orders(plane_row).clone();
    let rng_before = sim.world.random.state();
    let receipt = sim
        .process_air_group_package(0, 0x7f0, RECORDED_ONE)
        .unwrap();
    assert!(receipt.validates());
    assert!(matches!(
        receipt.status,
        AirTransactionStatus::Applied(ref evidence) if evidence.installs.is_empty()
    ));
    assert_eq!(selected_group(&sim).num, 0);
    assert_eq!(sim.builds[15].image(), build_before);
    assert_eq!(sim.world.orders(plane_row), &order_before);
    assert_eq!(sim.world.random.state(), rng_before);
}

#[test]
fn recorded_build_cache_survives_save_reload_and_empty_scramble_reselection() {
    let (mut sim, planes) = fixture();
    let first = sim
        .process_air_group_package(0, 0x800, RECORDED_ONE)
        .unwrap();
    assert!(matches!(first.status, AirTransactionStatus::Applied(_)));
    assert_eq!(
        sim.command_package_state.selection(0),
        Some(
            &[CachedSelection {
                o: 2_015,
                uid: 0x400f,
            }][..]
        )
    );

    let bytes = save_sim(&sim).unwrap();
    let mut resumed = load_sim(&bytes).unwrap();
    assert_eq!(save_sim(&resumed).unwrap(), bytes);
    install_authorities(&mut resumed, &planes, &[2_015, 2_090, 2_091, 2_147]);
    let before_rng = resumed.world.random.state();
    let cached = resumed
        .process_air_group_package(0, 0x801, CACHED_SCRAMBLE)
        .unwrap();
    assert!(cached.validates());
    assert!(matches!(cached.status, AirTransactionStatus::Applied(_)));
    assert!(cached.request.selection.is_cached_reselection());
    assert_eq!(&selected_group(&resumed).list[..1], &[2_015]);
    assert_eq!(resumed.world.random.state(), before_rng);
}

fn prepared_first(
    sim: &Sim,
) -> don_sim::systems::canonical_air_group_host::PreparedCanonicalAirPackage {
    let players = std::array::from_fn(|play| (play == 0).then_some(0));
    prepare_canonical_air_package(
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
        0x900,
        RECORDED_ONE,
    )
    .unwrap()
}

fn commit_prepared(
    sim: &mut Sim,
    prepared: don_sim::systems::canonical_air_group_host::PreparedCanonicalAirPackage,
) -> don_sim::systems::air_group_action_transaction::AirGroupActionReceipt {
    commit_canonical_air_package(
        &mut sim.world,
        &sim.builds,
        &mut sim.groups,
        &mut sim.paths,
        &mut sim.command_package_state,
        &sim.group_move_authority,
        &sim.air_group_authority,
        &sim.scenario_ignore_orders,
        prepared,
    )
}

#[test]
fn build_uid_or_containment_mutation_between_prepare_and_commit_publishes_nothing() {
    for mutate in 0..2 {
        let (mut sim, planes) = fixture();
        let prepared = prepared_first(&sim);
        let before_groups = sim.groups.clone();
        let before_cache = sim.command_package_state.clone();
        let before_rng = sim.world.random.state();
        let plane_row = sim.world.row_of(planes[0]).unwrap();
        let before_order = sim.world.orders(plane_row).clone();
        if mutate == 0 {
            sim.builds[15].uid ^= 1;
        } else {
            sim.builds[15].other[0x28..0x2a].copy_from_slice(&(-1i16).to_le_bytes());
        }
        let receipt = commit_prepared(&mut sim, prepared);
        assert!(receipt.validates());
        assert!(!matches!(receipt.status, AirTransactionStatus::Applied(_)));
        assert_eq!(sim.groups.list, before_groups.list);
        assert_eq!(sim.command_package_state, before_cache);
        assert_eq!(sim.world.random.state(), before_rng);
        assert_eq!(sim.world.orders(plane_row), &before_order);
    }
}

#[test]
fn save_refuses_nonreciprocal_or_cyclic_build_unit_containment() {
    let (mut nonreciprocal, planes) = fixture();
    let plane_row = nonreciprocal.world.row_of(planes[0]).unwrap();
    nonreciprocal.world.units.inside_up_mut()[plane_row] = -1;
    assert!(save_sim(&nonreciprocal).is_err());

    let (mut cyclic, planes) = fixture();
    let plane_row = cyclic.world.row_of(planes[0]).unwrap();
    let plane_o = cyclic.world.units.o()[plane_row];
    cyclic.world.units.inside_down_mut()[plane_row] = plane_o;
    cyclic.world.units.inside_down_who_mut()[plane_row] = 0;
    assert!(save_sim(&cyclic).is_err());
}
