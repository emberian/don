// SPDX-License-Identifier: GPL-3.0-or-later
//! Canonical packet -> Sim owners -> DoNSave v13 -> empty-cache resume.

use don_sim::systems::canonical_group_move_host::{
    CachedSelection, CommandPackageState, GroupMoveAuthority, GroupMovePackageReceipt,
    MoveMemberAuthority, MoveToWire, GROUP_OPCODE, MOVE_TO_OPCODE,
};
use don_sim::systems::groups_guys::{CheckSum, FormationMember};
use don_sim::systems::save_load::{load_sim, save_sim, SaveError};
use don_sim::tick::lifecycle_host::PlayerTable;
use don_sim::Handle;

fn package(objects: &[i16], movement: MoveToWire) -> Vec<u8> {
    let mut bytes = vec![GROUP_OPCODE, objects.len() as u8, 2];
    for &o in objects {
        bytes.extend_from_slice(&o.to_le_bytes());
    }
    bytes.push(MOVE_TO_OPCODE);
    bytes.extend_from_slice(&movement.x.to_le_bytes());
    bytes.extend_from_slice(&movement.y.to_le_bytes());
    bytes.extend_from_slice(&movement.set_angle.to_le_bytes());
    bytes.extend_from_slice(&movement.angle.to_le_bytes());
    bytes.extend([
        movement.orders as u8,
        movement.queued as u8,
        movement.form as u8,
        movement.width as u8,
        movement.disembark as u8,
    ]);
    bytes
}

fn movement(x: i32) -> MoveToWire {
    MoveToWire {
        x,
        y: 6_500,
        set_angle: 1,
        angle: 0x2300_0000,
        orders: 1,
        queued: 2,
        form: 0,
        width: 50,
        disembark: 1,
    }
}

fn authority(handle: Handle) -> GroupMoveAuthority {
    GroupMoveAuthority {
        revision: 7,
        composition_digest: [0x3c; 32],
        destination_is_water: false,
        force_formation_facing_zero: false,
        members: vec![MoveMemberAuthority {
            handle,
            role: 0x40000,
            on_map: true,
            is_captain: true,
            can_move: true,
            can_install_order: true,
            is_plane: false,
            domain: 0,
            unit_flags: 0,
            speed: 24,
            admits_unsplit_move_near: true,
            land_formation: FormationMember::default(),
            water_formation: FormationMember::default(),
        }],
    }
}

fn sim() -> (don_sim::tick::Sim, Handle, i16) {
    let mut sim = don_sim::tick::Sim::new(0x3344, 4);
    let mut players = PlayerTable::new();
    players.seat(0, 1, 2, 0);
    sim.players = Some(players);
    let handle = sim.spawn_unit(2, 17, 2_500, 3_500, 4).unwrap();
    let row = sim.world.row_of(handle).unwrap();
    sim.world.units.group_mut()[row] = -1;
    sim.world.units.o_down_mut()[row] = -1;
    sim.world.units.form_mut()[row] = 0;
    sim.world.units.form_mod_mut()[row] = 50;
    let o = sim.world.units.o()[row];
    sim.replace_group_move_authority(authority(handle));
    (sim, handle, o)
}

fn groups_checksum(sim: &don_sim::tick::Sim) -> u32 {
    let mut checksum = CheckSum::default();
    sim.groups.check_groups(&mut checksum);
    checksum.value
}

fn compare_receipt_without_process_revision(
    left: &GroupMovePackageReceipt,
    right: &GroupMovePackageReceipt,
) {
    assert_eq!(left.play, right.play);
    assert_eq!(left.lockstep_serial, right.lockstep_serial);
    assert_eq!(left.frame, right.frame);
    assert_eq!(left.who, right.who);
    assert_eq!(left.group_slot, right.group_slot);
    assert_eq!(left.selected, right.selected);
    assert_eq!(left.groups_checksum, right.groups_checksum);
    assert_eq!(left.random_state_before, right.random_state_before);
    assert_eq!(left.random_state_after, right.random_state_after);
}

#[test]
fn explicit_packet_save_load_empty_packet_resumes_identically() {
    let (mut direct, direct_handle, direct_o) = sim();
    let (mut resumed, resumed_handle, resumed_o) = sim();
    assert_eq!(direct_handle, resumed_handle);
    assert_eq!(direct_o, resumed_o);

    let explicit = package(&[direct_o], movement(7_500));
    direct.process_command_package(0, 10, &explicit).unwrap();
    resumed.process_command_package(0, 10, &explicit).unwrap();
    let saved_cache = resumed.command_package_state.saved_selections();
    let bytes = save_sim(&resumed).unwrap();
    let mut resumed = load_sim(&bytes).unwrap();
    assert_eq!(
        resumed.command_package_state.saved_selections(),
        saved_cache
    );
    assert_eq!(save_sim(&resumed).unwrap(), bytes);
    resumed.replace_group_move_authority(authority(resumed_handle));

    let empty = package(&[], movement(9_500));
    let direct_receipt = direct.process_command_package(0, 11, &empty).unwrap();
    let resumed_receipt = resumed.process_command_package(0, 11, &empty).unwrap();
    compare_receipt_without_process_revision(&direct_receipt, &resumed_receipt);
    assert_eq!(groups_checksum(&direct), groups_checksum(&resumed));
    assert_eq!(direct.groups.list, resumed.groups.list);
    let direct_row = direct.world.row_of(direct_handle).unwrap();
    let resumed_row = resumed.world.row_of(resumed_handle).unwrap();
    assert_eq!(
        direct.world.orders(direct_row),
        resumed.world.orders(resumed_row)
    );
    assert_eq!(direct.paths[direct_row], resumed.paths[resumed_row]);
    assert_eq!(
        direct.world.units.group()[direct_row],
        resumed.world.units.group()[resumed_row]
    );
}

#[test]
fn nonempty_cache_without_player_mapping_cannot_escape_in_a_save() {
    let mut sim = don_sim::tick::Sim::new(4, 4);
    let cache = std::array::from_fn(|play| {
        if play == 0 {
            vec![CachedSelection { o: 3, uid: 9 }]
        } else {
            Vec::new()
        }
    });
    sim.command_package_state = CommandPackageState::from_saved_selections(cache).unwrap();
    assert_eq!(
        save_sim(&sim),
        Err(SaveError::Unsupported(
            "command selection cache without player mapping"
        ))
    );
}

#[test]
fn loaded_cache_stays_retail_shaped_and_authority_is_deliberately_reinstalled() {
    let (mut sim, handle, o) = sim();
    sim.process_command_package(0, 1, &package(&[o, o], movement(8_000)))
        .unwrap();
    let bytes = save_sim(&sim).unwrap();
    let mut loaded = load_sim(&bytes).unwrap();
    assert_eq!(
        loaded.command_package_state.selection(0).unwrap(),
        &[
            CachedSelection {
                o,
                uid: loaded.world.units.get_uid(0),
            },
            CachedSelection {
                o,
                uid: loaded.world.units.get_uid(0),
            },
        ]
    );
    assert!(loaded.group_move_authority.members.is_empty());
    loaded.replace_group_move_authority(authority(handle));
    loaded
        .process_command_package(0, 2, &package(&[], movement(8_500)))
        .unwrap();
}
