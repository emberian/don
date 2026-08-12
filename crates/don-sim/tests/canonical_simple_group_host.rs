// SPDX-License-Identifier: GPL-3.0-or-later
//! Focused canonical `[Group][Unitmask]` transaction tests.

use don_sim::order::{Order, OrderIndex};
use don_sim::systems::canonical_group_move_host::{
    retail_fresh_groups, CommandPackageState, GroupMoveAuthority, MoveMemberAuthority,
    NETWORK_PLAYERS,
};
use don_sim::systems::canonical_simple_group_host::*;
use don_sim::systems::groups_guys::FormationMember;
use don_sim::systems::movement::{PathData, PathStack};
use don_sim::systems::save_load::{load_sim, save_sim};
use don_sim::tick::lifecycle_host::PlayerTable;
use don_sim::world::{Handle, World, OBJ_FLAG_ACTIVE};

fn packet(who: u8, objects: &[i16], mask: u32, set: i32) -> Vec<u8> {
    let mut bytes = vec![GROUP_OPCODE, objects.len() as u8, who];
    for &o in objects {
        bytes.extend_from_slice(&o.to_le_bytes());
    }
    bytes.push(UNITMASK_OPCODE);
    bytes.extend_from_slice(&mask.to_le_bytes());
    bytes.extend_from_slice(&set.to_le_bytes());
    bytes
}

struct Fixture {
    world: World,
    groups: don_sim::systems::groups_guys::Groups,
    paths: Vec<PathStack>,
    state: CommandPackageState,
    authority: GroupMoveAuthority,
    players: [Option<u8>; NETWORK_PLAYERS],
    handles: Vec<Handle>,
    objects: Vec<i16>,
}

impl Fixture {
    fn new(count: usize) -> Self {
        let mut world = World::new(0x4455);
        world.frame = 25;
        let mut handles = Vec::new();
        let mut objects = Vec::new();
        for index in 0..count {
            let handle = world
                .allocate_typed_at(0, 30 + index as i32, 2_500 + index as i32 * 100, 3_500)
                .unwrap();
            let row = world.row_of(handle).unwrap();
            world.units.group_mut()[row] = -1;
            world.units.o_down_mut()[row] = -1;
            world.units.angle_mut()[row] = 0x1100_0000 + index as i32 * 0x0100_0000;
            world.units.dest_angle_mut()[row] = -9;
            world.units.orders_x_mut()[row] = -10;
            world.units.orders_y_mut()[row] = -11;
            world.units.set_unit_masks(row, 0x0400_0000);
            handles.push(handle);
            objects.push(world.units.o()[row]);
        }
        let authority = GroupMoveAuthority {
            revision: 11,
            composition_digest: [0x87; 32],
            destination_is_water: false,
            force_formation_facing_zero: false,
            members: handles
                .iter()
                .map(|&handle| MoveMemberAuthority {
                    handle,
                    role: 0,
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
                })
                .collect(),
        };
        let mut players = [None; NETWORK_PLAYERS];
        players[0] = Some(0);
        Self {
            paths: vec![PathStack::default(); world.live_count() as usize],
            world,
            groups: retail_fresh_groups(),
            state: CommandPackageState::default(),
            authority,
            players,
            handles,
            objects,
        }
    }

    fn prepare(
        &self,
        serial: i32,
        bytes: &[u8],
    ) -> Result<PreparedSimpleGroupPackage, SimpleGroupPackageError> {
        prepare_simple_group_package(
            &self.world,
            &self.groups,
            &self.paths,
            &self.state,
            &self.authority,
            &self.players,
            self.world.frame,
            0,
            serial,
            bytes,
        )
    }

    fn process(
        &mut self,
        serial: i32,
        bytes: &[u8],
    ) -> Result<SimpleGroupPackageReceipt, SimpleGroupPackageError> {
        let prepared = self.prepare(serial, bytes)?;
        commit_simple_group_package(
            &mut self.world,
            &mut self.groups,
            &mut self.paths,
            &mut self.state,
            &self.authority,
            &self.players,
            prepared,
        )
    }
}

#[test]
fn decoder_accepts_the_exact_retail_fixture_and_refuses_any_suffix() {
    // Playback - 2026.08.11 11'44'38 (Tue).rcx, turn index 52 / turn 53 / frame 50.
    let retail = [
        0x00, 0x01, 0x00, 0x01, 0x00, // Group(owner 0, object 1)
        0x20, 0x00, 0x01, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, // Unitmask(0x100,-1)
    ];
    assert_eq!(
        decode_simple_group_package(&retail).unwrap(),
        SimpleGroupWire {
            who: 0,
            objects: vec![1],
            action: SimpleGroupActionWire::UnitMask {
                mask: 0x100,
                set: -1,
            },
        }
    );
    let mut trailing = retail.to_vec();
    trailing.push(79);
    assert_eq!(
        decode_simple_group_package(&trailing),
        Err(SimpleGroupPackageError::TrailingBytes {
            expected: retail.len(),
            got: retail.len() + 1,
        })
    );
    let unsupported = packet(0, &[1], 0x100, -1)
        .into_iter()
        .enumerate()
        .map(|(index, byte)| if index == 5 { 29 } else { byte })
        .collect::<Vec<_>>();
    assert_eq!(
        decode_simple_group_package(&unsupported),
        Err(SimpleGroupPackageError::UnsupportedActionOpcode { got: 29 })
    );
}

#[test]
fn retail_mask_100_commits_group_cache_flags_order_path_and_action_endpoint_once() {
    let mut fixture = Fixture::new(2);
    let handle = fixture.handles[1];
    let row = fixture.world.row_of(handle).unwrap();
    fixture.world.orders_mut(row).replace(Order {
        kind: OrderIndex::Think,
        flags: 0x5a,
        ..Order::default()
    });
    fixture.paths[row].push(PathData {
        to_x: 1,
        to_y: 2,
        tolerance: 3,
        flags: 4,
    });
    let random_before = fixture.world.random.state();
    let receipt = fixture
        .process(91, &packet(0, &[fixture.objects[1]], 0x100, -1))
        .unwrap();
    assert_eq!(receipt.opcode, UNITMASK_OPCODE);
    assert_eq!(receipt.random_state_before, random_before);
    assert_eq!(receipt.random_state_after, random_before);
    assert!(receipt.final_set);
    assert_eq!(fixture.world.units.group()[row], receipt.group_slot as i16);
    assert_eq!(fixture.world.units.get_unit_masks(row), 0x100);
    assert_eq!(fixture.world.units.get_flags(row), OBJ_FLAG_ACTIVE | 0x10);
    assert!(fixture.world.orders(row).is_empty());
    assert!(fixture.paths[row].is_empty());
    assert_eq!(
        fixture.world.units.orders_x()[row],
        fixture.world.units.x_internal()[row]
    );
    assert_eq!(
        fixture.world.units.orders_y()[row],
        fixture.world.units.y_internal()[row]
    );
    assert_eq!(
        fixture.world.units.dest_angle()[row],
        fixture.world.units.angle()[row]
    );
    assert_eq!(fixture.state.selection(0).unwrap()[0].o, fixture.objects[1]);
}

#[test]
fn mask_100_skips_a_plane_without_suppressing_the_canonical_selection() {
    let mut fixture = Fixture::new(1);
    fixture.authority.members[0].is_plane = true;
    let row = fixture.world.row_of(fixture.handles[0]).unwrap();
    let masks_before = fixture.world.units.get_unit_masks(row);
    let receipt = fixture
        .process(92, &packet(0, &fixture.objects.clone(), 0x100, -1))
        .unwrap();
    assert_eq!(fixture.world.units.get_unit_masks(row), masks_before);
    assert_eq!(fixture.world.units.group()[row], receipt.group_slot as i16);
    assert!(fixture.state.revision() > 0);
}

#[test]
fn simple_unit_state_does_not_invent_an_order_install_capability_read() {
    let mut fixture = Fixture::new(1);
    fixture.authority.members[0].can_install_order = false;
    let row = fixture.world.row_of(fixture.handles[0]).unwrap();
    fixture.world.units.set_unit_masks(row, 0);
    let objects = fixture.objects.clone();
    fixture.process(99, &packet(0, &objects, 0x20, -1)).unwrap();
    assert_eq!(fixture.world.units.get_unit_masks(row), 0x20);
}

#[test]
fn cached_empty_group_reuses_the_saved_selection_for_a_second_real_wire_shape() {
    let mut fixture = Fixture::new(1);
    let row = fixture.world.row_of(fixture.handles[0]).unwrap();
    fixture
        .process(93, &packet(0, &fixture.objects.clone(), 0x100, -1))
        .unwrap();
    fixture.world.units.set_unit_masks(row, 0);
    let receipt = fixture
        .process(94, &packet(0, &[], 0x0020_0000, 1))
        .unwrap();
    assert_eq!(receipt.selected.len(), 1);
    assert_eq!(fixture.world.units.get_unit_masks(row), 0x0020_0000);
    assert!(receipt.final_set);
}

#[test]
fn loop_carried_set_to_clear_transition_uses_the_canonical_member_order() {
    let mut fixture = Fixture::new(2);
    let first = fixture.world.row_of(fixture.handles[0]).unwrap();
    let second = fixture.world.row_of(fixture.handles[1]).unwrap();
    fixture.world.units.set_unit_masks(first, 0);
    fixture.world.units.set_unit_masks(second, 0x20);
    let objects = fixture.objects.clone();
    let receipt = fixture
        .process(95, &packet(0, &objects, 0x20, 123))
        .unwrap();
    assert_eq!(fixture.world.units.get_unit_masks(first), 0x20);
    assert_eq!(fixture.world.units.get_unit_masks(second), 0);
    assert!(!receipt.final_set);
}

#[test]
fn player_clock_rng_and_unit_scalar_staleness_publish_nothing() {
    let fixture = Fixture::new(1);
    let bytes = packet(0, &fixture.objects, 0x100, -1);

    let prepared = fixture.prepare(96, &bytes).unwrap();
    let mut stale = fixture;
    let before_groups = stale.groups.clone();
    let before_state = stale.state.clone();
    stale.world.random.advance();
    assert_eq!(
        commit_simple_group_package(
            &mut stale.world,
            &mut stale.groups,
            &mut stale.paths,
            &mut stale.state,
            &stale.authority,
            &stale.players,
            prepared,
        ),
        Err(SimpleGroupPackageError::StaleRng)
    );
    assert_eq!(stale.groups.list, before_groups.list);
    assert_eq!(stale.state, before_state);

    let prepared = stale.prepare(97, &bytes).unwrap();
    let handle = stale.handles[0];
    let row = stale.world.row_of(handle).unwrap();
    stale.world.units.set_flags(row, OBJ_FLAG_ACTIVE | 0x40);
    assert_eq!(
        commit_simple_group_package(
            &mut stale.world,
            &mut stale.groups,
            &mut stale.paths,
            &mut stale.state,
            &stale.authority,
            &stale.players,
            prepared,
        ),
        Err(SimpleGroupPackageError::StaleUnitFlags { handle })
    );
}

#[test]
fn a_mixed_unit_build_selection_is_refused_instead_of_narrowed() {
    let fixture = Fixture::new(1);
    let bytes = packet(0, &[fixture.objects[0], 2_000], 0x100, -1);
    assert_eq!(
        fixture.prepare(98, &bytes).unwrap_err(),
        SimpleGroupPackageError::UnsupportedSelectionBand { o: 2_000 }
    );
    assert!(fixture.state.is_empty());
    assert_eq!(fixture.world.units.group()[0], -1);
}

#[test]
fn exact_retail_packet_survives_donsave_and_cached_resume() {
    let make = || {
        let mut sim = don_sim::tick::Sim::new(0x4455, 4);
        sim.world.frame = 50;
        sim.vic_match.frame = 50;
        let mut players = PlayerTable::new();
        players.seat(0, 1, 0, 0);
        sim.players = Some(players);
        let _zero = sim.spawn_unit(0, 11, 2_300, 3_300, 3).unwrap();
        let actor = sim.spawn_unit(0, 17, 2_500, 3_500, 4).unwrap();
        let row = sim.world.row_of(actor).unwrap();
        sim.world.units.group_mut()[row] = -1;
        sim.world.units.o_down_mut()[row] = -1;
        sim.world.units.set_unit_masks(row, 0x0400_0000);
        sim.replace_group_move_authority(GroupMoveAuthority {
            revision: 19,
            composition_digest: [0xb2; 32],
            destination_is_water: false,
            force_formation_facing_zero: false,
            members: vec![MoveMemberAuthority {
                handle: actor,
                role: 0,
                on_map: true,
                is_captain: true,
                can_move: false,
                can_install_order: false,
                is_plane: false,
                domain: 0,
                unit_flags: 0,
                speed: 0,
                admits_unsplit_move_near: false,
                land_formation: FormationMember::default(),
                water_formation: FormationMember::default(),
            }],
        });
        (sim, actor)
    };
    let (mut direct, actor) = make();
    let (mut resumed, resumed_actor) = make();
    let retail = packet(0, &[1], 0x100, -1);
    direct.process_simple_group_package(0, 53, &retail).unwrap();
    resumed
        .process_simple_group_package(0, 53, &retail)
        .unwrap();
    let saved = save_sim(&resumed).unwrap();
    let mut resumed = load_sim(&saved).unwrap();
    resumed.replace_group_move_authority(GroupMoveAuthority {
        revision: 19,
        composition_digest: [0xb2; 32],
        destination_is_water: false,
        force_formation_facing_zero: false,
        members: vec![MoveMemberAuthority {
            handle: resumed_actor,
            role: 0,
            on_map: true,
            is_captain: true,
            can_move: false,
            can_install_order: false,
            is_plane: false,
            domain: 0,
            unit_flags: 0,
            speed: 0,
            admits_unsplit_move_near: false,
            land_formation: FormationMember::default(),
            water_formation: FormationMember::default(),
        }],
    });
    let cached = packet(0, &[], 0x0020_0000, 1);
    let direct_receipt = direct.process_simple_group_package(0, 54, &cached).unwrap();
    let resumed_receipt = resumed
        .process_simple_group_package(0, 54, &cached)
        .unwrap();
    assert_eq!(
        direct_receipt.groups_checksum,
        resumed_receipt.groups_checksum
    );
    assert_eq!(
        direct_receipt.random_state_before,
        direct_receipt.random_state_after
    );
    assert_eq!(
        resumed_receipt.random_state_before,
        resumed_receipt.random_state_after
    );
    let direct_row = direct.world.row_of(actor).unwrap();
    let resumed_row = resumed.world.row_of(resumed_actor).unwrap();
    assert_eq!(direct.groups.list, resumed.groups.list);
    assert_eq!(
        direct.world.units.group()[direct_row],
        resumed.world.units.group()[resumed_row]
    );
    assert_eq!(
        direct.world.units.get_unit_masks(direct_row),
        resumed.world.units.get_unit_masks(resumed_row)
    );
    assert_eq!(save_sim(&direct).unwrap(), save_sim(&resumed).unwrap());
}
