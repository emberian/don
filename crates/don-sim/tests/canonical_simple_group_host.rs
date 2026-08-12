// SPDX-License-Identifier: GPL-3.0-or-later
//! Focused canonical `[Group][one simple action]` transaction tests.

use don_sim::objects::BUILD_BAND_BASE;
use don_sim::order::{Order, OrderIndex, SpecialAnimType, ORDER_GROUP};
use don_sim::systems::canonical_group_move_host::{
    retail_fresh_groups, CommandPackageState, GroupMoveAuthority, MoveMemberAuthority,
    NETWORK_PLAYERS,
};
use don_sim::systems::canonical_simple_group_host::*;
use don_sim::systems::economy_order_payload_authority::{CastOrderPayload, EconomyOrderPayload};
use don_sim::systems::groups_guys::{FormationMember, NUM_LEADERS};
use don_sim::systems::movement::{PathData, PathStack};
use don_sim::systems::production::{self, BuildData};
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

fn stop_packet(who: u8, objects: &[i16]) -> Vec<u8> {
    let mut bytes = vec![GROUP_OPCODE, objects.len() as u8, who];
    for &o in objects {
        bytes.extend_from_slice(&o.to_le_bytes());
    }
    bytes.push(STOP_SPELL_OPCODE);
    bytes
}

fn halt_packet(who: u8, objects: &[i16]) -> Vec<u8> {
    let mut bytes = vec![GROUP_OPCODE, objects.len() as u8, who];
    for &o in objects {
        bytes.extend_from_slice(&o.to_le_bytes());
    }
    bytes.push(HALT_OPCODE);
    bytes
}

fn set_transport_packet(who: u8, objects: &[i16], flag: i32) -> Vec<u8> {
    let mut bytes = vec![GROUP_OPCODE, objects.len() as u8, who];
    for &o in objects {
        bytes.extend_from_slice(&o.to_le_bytes());
    }
    bytes.push(SET_TRANSPORT_OPCODE);
    bytes.extend_from_slice(&flag.to_le_bytes());
    bytes
}

fn buildmask_packet(who: u8, objects: &[i16], mask: u32, set: i32) -> Vec<u8> {
    let mut bytes = vec![GROUP_OPCODE, objects.len() as u8, who];
    for &o in objects {
        bytes.extend_from_slice(&o.to_le_bytes());
    }
    bytes.push(BUILDMASK_OPCODE);
    bytes.extend_from_slice(&mask.to_le_bytes());
    bytes.extend_from_slice(&set.to_le_bytes());
    bytes
}

fn follow_packet(who: u8, objects: &[i16], target_o: i32, target_who: i32, queued: i32) -> Vec<u8> {
    let mut bytes = vec![GROUP_OPCODE, objects.len() as u8, who];
    for &o in objects {
        bytes.extend_from_slice(&o.to_le_bytes());
    }
    bytes.push(FOLLOW_OPCODE);
    bytes.extend_from_slice(&target_o.to_le_bytes());
    bytes.extend_from_slice(&target_who.to_le_bytes());
    bytes.extend_from_slice(&queued.to_le_bytes());
    bytes
}

const RETAIL_HALT_OBJECTS: [i16; 24] = [
    0x55, 0x74, 0x42, 0x44, 0x45, 0x5e, 0x69, 0x71, 0x75, 0x7d, 0x7c, 0x80, 0x40, 0x4b, 0x5d, 0x01,
    0x03, 0x05, 0x06, 0x0a, 0x0c, 0x5c, 0x6e, 0x87,
];

fn cast_spell_order(spell: i32) -> Order {
    Order {
        kind: OrderIndex::CastSpell,
        flags: ORDER_GROUP,
        economy: Some(EconomyOrderPayload::CastSpell(CastOrderPayload {
            paid: 0,
            spell,
        })),
        ..Order::default()
    }
}

fn simple_authority(handles: &[Handle], revision: u64, digest: [u8; 32]) -> GroupMoveAuthority {
    GroupMoveAuthority {
        revision,
        composition_digest: digest,
        destination_is_water: false,
        force_formation_facing_zero: false,
        members: handles
            .iter()
            .copied()
            .map(|handle| MoveMemberAuthority {
                handle,
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
            })
            .collect(),
    }
}

fn simple_action_authority(
    handles: &[Handle],
    revision: u64,
    digest: [u8; 32],
) -> SimpleGroupActionAuthority {
    SimpleGroupActionAuthority {
        revision,
        composition_digest: digest,
        members: handles
            .iter()
            .copied()
            .map(|handle| SimpleGroupActionMemberAuthority {
                handle,
                can_ever_transport: true,
            })
            .collect(),
        builds: Vec::new(),
        follows: Vec::new(),
    }
}

struct Fixture {
    world: World,
    unit_types: Vec<i32>,
    groups: don_sim::systems::groups_guys::Groups,
    paths: Vec<PathStack>,
    state: CommandPackageState,
    authority: GroupMoveAuthority,
    action_authority: SimpleGroupActionAuthority,
    leader_flags: [i32; NUM_LEADERS],
    players: [Option<u8>; NETWORK_PLAYERS],
    handles: Vec<Handle>,
    objects: Vec<i16>,
}

impl Fixture {
    fn new(count: usize) -> Self {
        Self::new_for(0, count)
    }

    fn new_for(who: u8, count: usize) -> Self {
        let mut world = World::new(0x4455);
        world.frame = 25;
        let mut handles = Vec::new();
        let mut objects = Vec::new();
        for index in 0..count {
            let handle = world
                .allocate_typed_at(who, 30 + index as i32, 2_500 + index as i32 * 100, 3_500)
                .unwrap();
            let row = world.row_of(handle).unwrap();
            world.units.group_mut()[row] = -1;
            world.units.o_down_mut()[row] = -1;
            world.units.angle_mut()[row] =
                0x1100_0000_i32.wrapping_add((index as i32).wrapping_mul(0x0100_0000));
            world.units.dest_angle_mut()[row] = -9;
            world.units.orders_x_mut()[row] = -10;
            world.units.orders_y_mut()[row] = -11;
            world.units.set_unit_masks(row, 0x0400_0000);
            handles.push(handle);
            objects.push(world.units.o()[row]);
        }
        let unit_types = handles.iter().map(|_| 30).collect();
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
        let action_authority = simple_action_authority(&handles, 12, [0x14; 32]);
        let mut players = [None; NETWORK_PLAYERS];
        players[0] = Some(who);
        Self {
            paths: vec![PathStack::default(); world.live_count() as usize],
            world,
            unit_types,
            groups: retail_fresh_groups(),
            state: CommandPackageState::default(),
            authority,
            action_authority,
            leader_flags: [0; NUM_LEADERS],
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
        prepare_simple_group_package_with_action_authority(
            &self.world,
            &self.unit_types,
            &self.groups,
            &self.paths,
            &self.state,
            &self.authority,
            &self.action_authority,
            &self.leader_flags,
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
        commit_simple_group_package_with_action_authority(
            &mut self.world,
            &self.unit_types,
            &mut self.groups,
            &mut self.paths,
            &mut self.state,
            &self.authority,
            &self.action_authority,
            &self.leader_flags,
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
    // Playback___2017.07.20_20_46_23__Thu_.rcx, turn index 9162 / turn 9163 / frame 135624.
    let retail_stop_spell = [0x00, 0x03, 0x02, 0x07, 0x00, 0x38, 0x00, 0xe3, 0x00, 0x1d];
    assert_eq!(
        decode_simple_group_package(&retail_stop_spell).unwrap(),
        SimpleGroupWire {
            who: 2,
            objects: vec![7, 56, 227],
            action: SimpleGroupActionWire::StopSpell,
        }
    );
    // Playback___2024.03.23_21_16_13__Sat_.rcx, turn index 8864 / turn 8865 / frame 35465.
    let retail_halt = halt_packet(0, &RETAIL_HALT_OBJECTS);
    assert_eq!(
        decode_simple_group_package(&retail_halt).unwrap(),
        SimpleGroupWire {
            who: 0,
            objects: RETAIL_HALT_OBJECTS.to_vec(),
            action: SimpleGroupActionWire::Halt,
        }
    );
    // Playback___2024.02.23_20_49_35__Fri_.rcx, turn 119 / frame 709.
    let retail_set_transport = [0x00, 0x01, 0x01, 0x00, 0x00, 0x0e, 1, 0, 0, 0];
    assert_eq!(
        decode_simple_group_package(&retail_set_transport).unwrap(),
        SimpleGroupWire {
            who: 1,
            objects: vec![0],
            action: SimpleGroupActionWire::SetTransport { flag: 1 },
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
        .map(|(index, byte)| if index == 5 { 28 } else { byte })
        .collect::<Vec<_>>();
    assert_eq!(
        decode_simple_group_package(&unsupported),
        Err(SimpleGroupPackageError::UnsupportedActionOpcode { got: 28 })
    );
}

#[test]
fn exact_retail_stop_spell_packet_closes_the_complete_ordinary_unit_cone() {
    let mut fixture = Fixture::new_for(2, 228);
    let selected = [7_i16, 56, 227];
    for &o in &selected {
        let handle = fixture.handles[o as usize];
        let row = fixture.world.row_of(handle).unwrap();
        fixture.world.units.set_unit_masks(row, 0x0400_1234);
        fixture.world.units.spell_time_mut()[row] = 91;
        fixture.world.units.orders_x_mut()[row] = -10;
        fixture.world.units.orders_y_mut()[row] = -11;
        fixture.world.units.dest_angle_mut()[row] = -12;
        fixture.world.orders_mut(row).replace(cast_spell_order(630));
        fixture.paths[row].push(PathData {
            to_x: 1,
            to_y: 2,
            tolerance: 3,
            flags: 4,
        });
    }
    let retail = stop_packet(2, &selected);
    assert_eq!(
        retail,
        [0x00, 0x03, 0x02, 0x07, 0x00, 0x38, 0x00, 0xe3, 0x00, 0x1d]
    );
    let random_before = fixture.world.random.state();
    let receipt = fixture.process(135_624, &retail).unwrap();
    assert_eq!(receipt.opcode, STOP_SPELL_OPCODE);
    assert_eq!(
        receipt.action_result,
        SimpleGroupActionResult::StopSpell { stopped_units: 3 }
    );
    assert_eq!(receipt.random_state_before, random_before);
    assert_eq!(receipt.random_state_after, random_before);
    assert_eq!(fixture.state.selection(0).unwrap().len(), 3);
    for &o in &selected {
        let row = fixture.world.row_of(fixture.handles[o as usize]).unwrap();
        assert_eq!(fixture.world.units.get_unit_masks(row), 0x1234);
        assert_eq!(fixture.world.units.spell_time()[row], 0);
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
    }
}

#[test]
fn stop_spell_special_gpiece_types_refuse_before_selection_or_unit_publication() {
    let mut fixture = Fixture::new(1);
    fixture.unit_types[0] = 61;
    let handle = fixture.handles[0];
    let row = fixture.world.row_of(handle).unwrap();
    fixture.world.orders_mut(row).replace(cast_spell_order(630));
    fixture.world.units.spell_time_mut()[row] = 17;
    let groups_before = fixture.groups.clone();
    let state_before = fixture.state.clone();
    assert_eq!(
        fixture.prepare(136, &stop_packet(0, &[0])).unwrap_err(),
        SimpleGroupPackageError::MissingStopSpellGpieceAuthority {
            handle,
            type_index: 61,
        }
    );
    assert_eq!(fixture.groups.list, groups_before.list);
    assert_eq!(fixture.state, state_before);
    assert_eq!(fixture.world.units.spell_time()[row], 17);
    assert_eq!(
        fixture.world.orders(row).order_type(),
        OrderIndex::CastSpell
    );
}

#[test]
fn stop_spell_spell_clock_and_type_staleness_publish_nothing() {
    let mut fixture = Fixture::new(1);
    let handle = fixture.handles[0];
    let row = fixture.world.row_of(handle).unwrap();
    fixture.world.orders_mut(row).replace(cast_spell_order(630));
    fixture.world.units.spell_time_mut()[row] = 17;
    let bytes = stop_packet(0, &[0]);
    let prepared = fixture.prepare(137, &bytes).unwrap();
    let groups_before = fixture.groups.clone();
    let state_before = fixture.state.clone();
    fixture.world.units.spell_time_mut()[row] = 18;
    assert_eq!(
        commit_simple_group_package(
            &mut fixture.world,
            &fixture.unit_types,
            &mut fixture.groups,
            &mut fixture.paths,
            &mut fixture.state,
            &fixture.authority,
            &fixture.players,
            prepared,
        ),
        Err(SimpleGroupPackageError::StaleUnitSpellTime { handle })
    );
    assert_eq!(fixture.groups.list, groups_before.list);
    assert_eq!(fixture.state, state_before);
    assert_eq!(
        fixture.world.orders(row).order_type(),
        OrderIndex::CastSpell
    );

    fixture.world.units.spell_time_mut()[row] = 17;
    let prepared = fixture.prepare(138, &bytes).unwrap();
    fixture.unit_types[row] = 31;
    assert_eq!(
        commit_simple_group_package(
            &mut fixture.world,
            &fixture.unit_types,
            &mut fixture.groups,
            &mut fixture.paths,
            &mut fixture.state,
            &fixture.authority,
            &fixture.players,
            prepared,
        ),
        Err(SimpleGroupPackageError::StaleUnitType { handle })
    );
    assert_eq!(fixture.groups.list, groups_before.list);
    assert_eq!(fixture.state, state_before);
}

#[test]
fn exact_retail_halt_packet_closes_orders_paths_masks_and_action_endpoints() {
    let mut fixture = Fixture::new(136);
    let untouched = fixture.world.row_of(fixture.handles[2]).unwrap();
    fixture.world.orders_mut(untouched).replace(Order {
        kind: OrderIndex::Think,
        ..Order::default()
    });
    for &o in &RETAIL_HALT_OBJECTS {
        let row = fixture.world.row_of(fixture.handles[o as usize]).unwrap();
        fixture.world.units.set_unit_masks(row, 0x0400_0123);
        fixture.world.units.orders_x_mut()[row] = -10;
        fixture.world.units.orders_y_mut()[row] = -11;
        fixture.world.units.dest_angle_mut()[row] = -12;
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
    }
    let retail = halt_packet(0, &RETAIL_HALT_OBJECTS);
    assert_eq!(retail[0..3], [0, 24, 0]);
    assert_eq!(retail.last(), Some(&0x0c));
    let random_before = fixture.world.random.state();
    let receipt = fixture.process(35_465, &retail).unwrap();
    assert_eq!(receipt.opcode, HALT_OPCODE);
    assert_eq!(
        receipt.action_result,
        SimpleGroupActionResult::Halt { halted_units: 24 }
    );
    assert_eq!(receipt.random_state_before, random_before);
    assert_eq!(receipt.random_state_after, random_before);
    assert_eq!(fixture.state.selection(0).unwrap().len(), 24);
    assert_eq!(fixture.groups.list[receipt.group_slot].form, -1);
    assert_eq!(fixture.groups.list[receipt.group_slot].disband, 0);
    for &o in &RETAIL_HALT_OBJECTS {
        let row = fixture.world.row_of(fixture.handles[o as usize]).unwrap();
        assert_eq!(fixture.world.units.get_unit_masks(row), 0x23);
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
    }
    assert_eq!(
        fixture.world.orders(untouched).order_type(),
        OrderIndex::Think
    );
}

#[test]
fn halt_skips_entering_and_airborne_plane_members_without_losing_selection() {
    let mut fixture = Fixture::new(2);
    let entering = fixture.world.row_of(fixture.handles[0]).unwrap();
    let plane = fixture.world.row_of(fixture.handles[1]).unwrap();
    fixture
        .world
        .orders_mut(entering)
        .replace(Order::special_anim(SpecialAnimType::Enter, 7, 8));
    fixture.world.orders_mut(plane).replace(Order {
        kind: OrderIndex::Think,
        ..Order::default()
    });
    fixture.authority.members[1].is_plane = true;
    fixture.authority.members[1].domain = 2;
    fixture.authority.members[1].unit_flags = 0;
    let objects = fixture.objects.clone();
    let receipt = fixture.process(139, &halt_packet(0, &objects)).unwrap();
    assert_eq!(
        receipt.action_result,
        SimpleGroupActionResult::Halt { halted_units: 0 }
    );
    assert_eq!(receipt.selected.len(), 2);
    assert_eq!(
        fixture.world.orders(entering).order_type(),
        OrderIndex::SpecialAnim
    );
    assert_eq!(fixture.world.orders(plane).order_type(), OrderIndex::Think);
}

#[test]
fn halt_malformed_special_anim_payload_refuses_before_publication() {
    let mut fixture = Fixture::new(1);
    let handle = fixture.handles[0];
    let row = fixture.world.row_of(handle).unwrap();
    fixture.world.orders_mut(row).replace(Order {
        kind: OrderIndex::SpecialAnim,
        special_anim: None,
        ..Order::default()
    });
    let groups_before = fixture.groups.clone();
    let state_before = fixture.state.clone();
    assert_eq!(
        fixture.prepare(140, &halt_packet(0, &[0])).unwrap_err(),
        SimpleGroupPackageError::MissingSpecialAnimPayload { handle }
    );
    assert_eq!(fixture.groups.list, groups_before.list);
    assert_eq!(fixture.state, state_before);
    assert_eq!(
        fixture.world.orders(row).order_type(),
        OrderIndex::SpecialAnim
    );
}

#[test]
fn exact_retail_set_transport_packet_uses_leader_ladder_and_handle_capability() {
    let mut fixture = Fixture::new_for(1, 2);
    fixture.leader_flags[1] = 0x200;
    fixture.action_authority.members[1].can_ever_transport = false;
    for &handle in &fixture.handles {
        let row = fixture.world.row_of(handle).unwrap();
        fixture.world.units.set_unit_masks(row, 0x20);
    }
    let retail = set_transport_packet(1, &[0], 1);
    assert_eq!(retail, [0x00, 0x01, 0x01, 0, 0, 0x0e, 1, 0, 0, 0]);
    let random_before = fixture.world.random.state();
    let receipt = fixture.process(709, &retail).unwrap();
    assert_eq!(receipt.opcode, SET_TRANSPORT_OPCODE);
    assert_eq!(
        receipt.action_result,
        SimpleGroupActionResult::SetTransport {
            enabled: true,
            changed_units: 1,
        }
    );
    assert_eq!(receipt.random_state_before, random_before);
    assert_eq!(receipt.random_state_after, random_before);
    let selected = fixture.world.row_of(fixture.handles[0]).unwrap();
    let untouched = fixture.world.row_of(fixture.handles[1]).unwrap();
    assert_eq!(fixture.world.units.get_unit_masks(selected), 0x0080_0020);
    assert_eq!(fixture.world.units.get_unit_masks(untouched), 0x20);
    assert_eq!(fixture.groups.list[receipt.group_slot].disband, 0);

    let cached = set_transport_packet(1, &[], 0);
    assert_eq!(cached, [0x00, 0x00, 0x01, 0x0e, 0, 0, 0, 0]);
    let receipt = fixture.process(710, &cached).unwrap();
    assert_eq!(
        receipt.action_result,
        SimpleGroupActionResult::SetTransport {
            enabled: false,
            changed_units: 1,
        }
    );
    assert_eq!(fixture.world.units.get_unit_masks(selected), 0x20);
}

#[test]
fn set_transport_missing_or_stale_action_facts_publish_nothing() {
    let mut fixture = Fixture::new(1);
    fixture.leader_flags[0] = 0x100;
    let handle = fixture.handles[0];
    let row = fixture.world.row_of(handle).unwrap();
    let bytes = set_transport_packet(0, &[0], 1);
    fixture.action_authority.members.clear();
    assert_eq!(
        fixture.prepare(711, &bytes).unwrap_err(),
        SimpleGroupPackageError::MissingSetTransportAuthority { handle }
    );
    assert!(fixture.state.is_empty());
    assert_eq!(fixture.world.units.get_unit_masks(row), 0x0400_0000);

    fixture.action_authority = simple_action_authority(&fixture.handles, 12, [0x14; 32]);
    let prepared = fixture.prepare(712, &bytes).unwrap();
    fixture.action_authority.revision += 1;
    assert_eq!(
        commit_simple_group_package_with_action_authority(
            &mut fixture.world,
            &fixture.unit_types,
            &mut fixture.groups,
            &mut fixture.paths,
            &mut fixture.state,
            &fixture.authority,
            &fixture.action_authority,
            &fixture.leader_flags,
            &fixture.players,
            prepared,
        ),
        Err(SimpleGroupPackageError::StaleActionAuthority)
    );
    assert!(fixture.state.is_empty());
    assert_eq!(fixture.world.units.get_unit_masks(row), 0x0400_0000);

    fixture.action_authority.revision -= 1;
    let prepared = fixture.prepare(713, &bytes).unwrap();
    fixture.leader_flags[0] ^= 0x100;
    assert_eq!(
        commit_simple_group_package_with_action_authority(
            &mut fixture.world,
            &fixture.unit_types,
            &mut fixture.groups,
            &mut fixture.paths,
            &mut fixture.state,
            &fixture.authority,
            &fixture.action_authority,
            &fixture.leader_flags,
            &fixture.players,
            prepared,
        ),
        Err(SimpleGroupPackageError::StaleLeaderFlags { who: 0 })
    );
    assert!(fixture.state.is_empty());
    assert_eq!(fixture.world.units.get_unit_masks(row), 0x0400_0000);
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
    assert_eq!(
        receipt.action_result,
        SimpleGroupActionResult::UnitMask { final_set: true }
    );
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
    assert_eq!(
        receipt.action_result,
        SimpleGroupActionResult::UnitMask { final_set: true }
    );
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
    assert_eq!(
        receipt.action_result,
        SimpleGroupActionResult::UnitMask { final_set: false }
    );
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
            &stale.unit_types,
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
            &stale.unit_types,
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

#[test]
fn exact_retail_stop_spell_packet_survives_donsave_and_cached_resume() {
    let make = || {
        let mut sim = don_sim::tick::Sim::new(0x7788, 4);
        sim.world.frame = 135_624;
        sim.vic_match.frame = 135_624;
        let mut players = PlayerTable::new();
        players.seat(1, 1, 2, 0);
        sim.players = Some(players);
        let mut handles = Vec::new();
        for index in 0..228 {
            let handle = sim.spawn_unit(2, 30, 2_000 + index * 4, 3_000, 3).unwrap();
            let row = sim.world.row_of(handle).unwrap();
            sim.world.units.group_mut()[row] = -1;
            sim.world.units.o_down_mut()[row] = -1;
            handles.push(handle);
        }
        sim.replace_group_move_authority(simple_authority(&handles, 29, [0x29; 32]));
        (sim, handles)
    };
    let arm_cast = |sim: &mut don_sim::tick::Sim, handles: &[Handle]| {
        for o in [7_usize, 56, 227] {
            let row = sim.world.row_of(handles[o]).unwrap();
            sim.world.units.set_unit_masks(row, 0x0400_0040);
            sim.world.units.spell_time_mut()[row] = 33;
            sim.world.orders_mut(row).replace(cast_spell_order(630));
            sim.paths[row].push(PathData {
                to_x: 4,
                to_y: 5,
                tolerance: 6,
                flags: 7,
            });
        }
    };

    let (mut direct, direct_handles) = make();
    let (mut resumed, resumed_handles) = make();
    arm_cast(&mut direct, &direct_handles);
    arm_cast(&mut resumed, &resumed_handles);
    let retail = stop_packet(2, &[7, 56, 227]);
    direct
        .process_simple_group_package(1, 9_163, &retail)
        .unwrap();
    resumed
        .process_simple_group_package(1, 9_163, &retail)
        .unwrap();
    let saved = save_sim(&resumed).unwrap();
    let mut resumed = load_sim(&saved).unwrap();
    resumed.replace_group_move_authority(simple_authority(&resumed_handles, 29, [0x29; 32]));

    arm_cast(&mut direct, &direct_handles);
    arm_cast(&mut resumed, &resumed_handles);
    let cached = stop_packet(2, &[]);
    let direct_receipt = direct
        .process_simple_group_package(1, 9_164, &cached)
        .unwrap();
    let resumed_receipt = resumed
        .process_simple_group_package(1, 9_164, &cached)
        .unwrap();
    assert_eq!(
        direct_receipt.action_result,
        SimpleGroupActionResult::StopSpell { stopped_units: 3 }
    );
    assert_eq!(direct_receipt.action_result, resumed_receipt.action_result);
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
    for o in [7_usize, 56, 227] {
        let direct_row = direct.world.row_of(direct_handles[o]).unwrap();
        let resumed_row = resumed.world.row_of(resumed_handles[o]).unwrap();
        assert_eq!(direct.world.units.get_unit_masks(direct_row), 0x40);
        assert_eq!(direct.world.units.spell_time()[direct_row], 0);
        assert!(direct.world.orders(direct_row).is_empty());
        assert_eq!(
            direct.world.units.get_unit_masks(direct_row),
            resumed.world.units.get_unit_masks(resumed_row)
        );
        assert_eq!(
            direct.world.units.spell_time()[direct_row],
            resumed.world.units.spell_time()[resumed_row]
        );
        assert_eq!(direct.paths[direct_row], resumed.paths[resumed_row]);
    }
    assert_eq!(direct.groups.list, resumed.groups.list);
    assert_eq!(save_sim(&direct).unwrap(), save_sim(&resumed).unwrap());
}

#[test]
fn retail_halt_survives_donsave_cached_resume_and_one_canonical_tick() {
    let make = || {
        let mut sim = don_sim::tick::Sim::new(0x0c12, 4);
        sim.world.frame = 35_465;
        sim.vic_match.frame = 35_465;
        let mut players = PlayerTable::new();
        players.seat(1, 1, 0, 0);
        sim.players = Some(players);
        let mut handles = Vec::new();
        for index in 0..136 {
            let handle = sim.spawn_unit(0, 30, 2_000 + index * 4, 3_000, 3).unwrap();
            let row = sim.world.row_of(handle).unwrap();
            sim.world.units.group_mut()[row] = -1;
            sim.world.units.o_down_mut()[row] = -1;
            handles.push(handle);
        }
        sim.replace_group_move_authority(simple_authority(&handles, 12, [0x0c; 32]));
        (sim, handles)
    };
    let arm_orders = |sim: &mut don_sim::tick::Sim, handles: &[Handle]| {
        for &o in &RETAIL_HALT_OBJECTS {
            let row = sim.world.row_of(handles[o as usize]).unwrap();
            sim.world.units.set_unit_masks(row, 0x0400_0123);
            sim.world.orders_mut(row).replace(Order {
                kind: OrderIndex::Think,
                ..Order::default()
            });
            sim.paths[row].push(PathData {
                to_x: 4,
                to_y: 5,
                tolerance: 6,
                flags: 7,
            });
        }
    };

    let (mut direct, direct_handles) = make();
    let (mut resumed, resumed_handles) = make();
    arm_orders(&mut direct, &direct_handles);
    arm_orders(&mut resumed, &resumed_handles);
    let retail = halt_packet(0, &RETAIL_HALT_OBJECTS);
    direct
        .process_simple_group_package(1, 8_865, &retail)
        .unwrap();
    resumed
        .process_simple_group_package(1, 8_865, &retail)
        .unwrap();
    let saved = save_sim(&resumed).unwrap();
    let mut resumed = load_sim(&saved).unwrap();
    resumed.replace_group_move_authority(simple_authority(&resumed_handles, 12, [0x0c; 32]));

    arm_orders(&mut direct, &direct_handles);
    arm_orders(&mut resumed, &resumed_handles);
    // The same artifact later carries this exact persistent-cache Group wire.
    let cached = halt_packet(0, &[]);
    assert_eq!(cached, [0x00, 0x00, 0x00, 0x0c]);
    let direct_receipt = direct
        .process_simple_group_package(1, 9_575, &cached)
        .unwrap();
    let resumed_receipt = resumed
        .process_simple_group_package(1, 9_575, &cached)
        .unwrap();
    assert_eq!(
        direct_receipt.action_result,
        SimpleGroupActionResult::Halt { halted_units: 24 }
    );
    assert_eq!(direct_receipt.action_result, resumed_receipt.action_result);
    assert_eq!(
        direct_receipt.groups_checksum,
        resumed_receipt.groups_checksum
    );
    let frame_before = direct.world.frame;
    direct.do_frame();
    resumed.do_frame();
    assert_eq!(direct.world.frame, frame_before + 1);
    assert_eq!(direct.world.frame, resumed.world.frame);
    for &o in &RETAIL_HALT_OBJECTS {
        let direct_row = direct.world.row_of(direct_handles[o as usize]).unwrap();
        let resumed_row = resumed.world.row_of(resumed_handles[o as usize]).unwrap();
        assert_eq!(direct.world.units.get_unit_masks(direct_row), 0x23);
        assert!(direct.world.orders(direct_row).is_empty());
        assert!(direct.paths[direct_row].is_empty());
        assert_eq!(
            direct.world.units.get_unit_masks(direct_row),
            resumed.world.units.get_unit_masks(resumed_row)
        );
        assert_eq!(direct.paths[direct_row], resumed.paths[resumed_row]);
    }
    assert_eq!(direct.groups.list, resumed.groups.list);
    assert_eq!(save_sim(&direct).unwrap(), save_sim(&resumed).unwrap());
}

#[test]
fn retail_set_transport_survives_donsave_cached_resume_and_one_canonical_tick() {
    let make = || {
        let mut sim = don_sim::tick::Sim::new(0x140e, 4);
        sim.world.frame = 709;
        sim.vic_match.frame = 709;
        let mut players = PlayerTable::new();
        players.seat(1, 1, 1, 0);
        sim.players = Some(players);
        sim.production_runtime.local_player = 1;
        sim.vic_leaders.slots[1].leader_flags = 0x100;
        let actor = sim.spawn_unit(1, 30, 2_000, 3_000, 3).unwrap();
        let row = sim.world.row_of(actor).unwrap();
        sim.world.units.group_mut()[row] = -1;
        sim.world.units.o_down_mut()[row] = -1;
        sim.world.units.set_unit_masks(row, 0x20);
        sim.replace_group_move_authority(simple_authority(&[actor], 14, [0x14; 32]));
        sim.replace_simple_group_action_authority(simple_action_authority(
            &[actor],
            14,
            [0x5e; 32],
        ));
        (sim, actor)
    };
    let install = |sim: &mut don_sim::tick::Sim, actor: Handle| {
        sim.production_runtime.local_player = 1;
        sim.replace_group_move_authority(simple_authority(&[actor], 14, [0x14; 32]));
        sim.replace_simple_group_action_authority(simple_action_authority(
            &[actor],
            14,
            [0x5e; 32],
        ));
    };

    let (mut direct, direct_actor) = make();
    let (mut resumed, resumed_actor) = make();
    let retail = set_transport_packet(1, &[0], 1);
    direct
        .process_simple_group_package(1, 119, &retail)
        .unwrap();
    resumed
        .process_simple_group_package(1, 119, &retail)
        .unwrap();
    let saved = save_sim(&resumed).unwrap();
    let mut resumed = load_sim(&saved).unwrap();
    install(&mut resumed, resumed_actor);

    // Four of the five corpus packets use this empty Group plus flag-zero action image.
    let cached = set_transport_packet(1, &[], 0);
    let direct_receipt = direct
        .process_simple_group_package(1, 120, &cached)
        .unwrap();
    let resumed_receipt = resumed
        .process_simple_group_package(1, 120, &cached)
        .unwrap();
    assert_eq!(direct_receipt.action_result, resumed_receipt.action_result);
    assert_eq!(
        direct_receipt.action_result,
        SimpleGroupActionResult::SetTransport {
            enabled: false,
            changed_units: 1,
        }
    );
    assert_eq!(
        direct_receipt.groups_checksum,
        resumed_receipt.groups_checksum
    );
    let direct_row = direct.world.row_of(direct_actor).unwrap();
    let resumed_row = resumed.world.row_of(resumed_actor).unwrap();
    assert_eq!(direct.world.units.get_unit_masks(direct_row), 0x20);
    assert_eq!(
        direct.world.units.get_unit_masks(direct_row),
        resumed.world.units.get_unit_masks(resumed_row)
    );
    let frame_before = direct.world.frame;
    direct.do_frame();
    resumed.do_frame();
    assert_eq!(direct.world.frame, frame_before + 1);
    assert_eq!(direct.world.frame, resumed.world.frame);
    assert_eq!(direct.groups.list, resumed.groups.list);
    assert_eq!(save_sim(&direct).unwrap(), save_sim(&resumed).unwrap());
}

fn savable_build_for_object(object: i16, uid: u16) -> BuildData {
    let mut build = BuildData {
        flags: production::flag::VALID,
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
        .copy_from_slice(&object.to_le_bytes());
    build.other[0x28..0x2a].copy_from_slice(&(-1_i16).to_le_bytes());
    build
}

fn buildmask_authority(row: usize, o: i16, uid: u16) -> SimpleGroupActionAuthority {
    SimpleGroupActionAuthority {
        revision: 33,
        composition_digest: [0x33; 32],
        members: Vec::new(),
        builds: vec![SimpleBuildMaskMemberAuthority {
            who: 2,
            o,
            uid,
            row,
            role: 0x0240,
            admits_0x40: true,
            admits_0x80: true,
        }],
        follows: Vec::new(),
    }
}

#[test]
fn retail_buildmask_survives_donsave_cached_resume_and_one_canonical_tick() {
    const RETAIL_O: i16 = 0x0837;
    const TARGET_ROW: usize = (RETAIL_O as u32 - BUILD_BAND_BASE) as usize;
    const TARGET_UID: u16 = 0x4337;

    let make = || {
        let mut sim = don_sim::tick::Sim::new(0x3321, 4);
        sim.world.frame = 105_512;
        sim.vic_match.frame = 105_512;
        let mut players = PlayerTable::new();
        players.seat(1, 1, 2, 0);
        sim.players = Some(players);
        sim.production_runtime.local_player = 2;
        for row in 0..=TARGET_ROW {
            let o = BUILD_BAND_BASE as i16 + row as i16;
            let uid = if row == TARGET_ROW {
                TARGET_UID
            } else {
                0x2000 + row as u16
            };
            assert_eq!(sim.spawn_build(2, savable_build_for_object(o, uid)), row);
        }
        sim.builds[TARGET_ROW].flags |= production::flag::STARTED | production::flag::ACTIVE;
        sim.builds[TARGET_ROW].queue = production::BuildQueue {
            queued: 1,
            entries: vec![production::BuildQueueEntry {
                type_index: 30,
                res: [-1; 3],
                ..production::BuildQueueEntry::default()
            }],
        };
        sim.replace_simple_group_action_authority(buildmask_authority(
            TARGET_ROW, RETAIL_O, TARGET_UID,
        ));
        sim
    };
    let reinstall = |sim: &mut don_sim::tick::Sim| {
        sim.production_runtime.local_player = 2;
        sim.replace_simple_group_action_authority(buildmask_authority(
            TARGET_ROW, RETAIL_O, TARGET_UID,
        ));
    };

    let mut direct = make();
    let mut resumed = make();
    // Playback___2017.07.20_20_46_23__Thu_.rcx, package index 5297 / frame 105512.
    let retail = buildmask_packet(2, &[RETAIL_O], 0x40, 1);
    assert_eq!(
        retail,
        [0x00, 0x01, 0x02, 0x37, 0x08, 0x21, 0x40, 0, 0, 0, 1, 0, 0, 0]
    );
    let first = direct
        .process_simple_group_package(1, 5_298, &retail)
        .unwrap();
    resumed
        .process_simple_group_package(1, 5_298, &retail)
        .unwrap();
    assert_eq!(
        first.action_result,
        SimpleGroupActionResult::BuildMask {
            final_set: true,
            changed_builds: 1,
            feedback: true,
        }
    );
    assert_eq!(first.selected_builds[0].o, RETAIL_O);
    assert_eq!(direct.groups.list[first.group_slot].role, 0x0240);
    assert_eq!(direct.builds[TARGET_ROW].build_masks & 0x40, 0x40);

    let saved = save_sim(&resumed).unwrap();
    let mut resumed = load_sim(&saved).unwrap();
    reinstall(&mut resumed);

    // The same artifact repeatedly uses the persistent empty Group wire for mask 0x40.
    let cached = buildmask_packet(2, &[], 0x40, 1);
    assert_eq!(cached, [0x00, 0x00, 0x02, 0x21, 0x40, 0, 0, 0, 1, 0, 0, 0]);
    let direct_receipt = direct
        .process_simple_group_package(1, 5_332, &cached)
        .unwrap();
    let resumed_receipt = resumed
        .process_simple_group_package(1, 5_332, &cached)
        .unwrap();
    assert_eq!(direct_receipt.action_result, resumed_receipt.action_result);
    assert_eq!(
        direct_receipt.action_result,
        SimpleGroupActionResult::BuildMask {
            final_set: false,
            changed_builds: 1,
            feedback: true,
        }
    );
    assert_eq!(direct.builds[TARGET_ROW].build_masks & 0x40, 0);
    assert_eq!(
        direct.builds[TARGET_ROW].build_masks,
        resumed.builds[TARGET_ROW].build_masks
    );
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

    let frame_before = direct.world.frame;
    direct.do_frame();
    resumed.do_frame();
    assert_eq!(direct.world.frame, frame_before + 1);
    assert_eq!(direct.world.frame, resumed.world.frame);
    assert_eq!(direct.groups.list, resumed.groups.list);
    assert_eq!(
        direct.builds[TARGET_ROW].build_masks,
        resumed.builds[TARGET_ROW].build_masks
    );
    assert_eq!(direct.world.random.state(), resumed.world.random.state());
}

#[test]
fn buildmask_revalidates_the_canonical_build_before_any_group_or_cache_write() {
    let o = BUILD_BAND_BASE as i16;
    let uid = 0x3344;
    let mut sim = don_sim::tick::Sim::new(0x3344, 4);
    sim.world.frame = 44;
    assert_eq!(sim.spawn_build(2, savable_build_for_object(o, uid)), 0);
    sim.replace_simple_group_action_authority(buildmask_authority(0, o, uid));
    let mut player_who = [None; NETWORK_PLAYERS];
    player_who[1] = Some(2);
    let bytes = buildmask_packet(2, &[o], 0x80, 1);
    let prepared = prepare_simple_group_package_with_builds(
        &sim.world,
        &sim.unit_type,
        &sim.builds,
        &sim.groups,
        &sim.paths,
        &sim.command_package_state,
        &sim.group_move_authority,
        &sim.simple_group_action_authority,
        &sim.scenario_ignore_orders,
        &[0; NUM_LEADERS],
        Some(2),
        &player_who,
        sim.world.frame,
        1,
        44,
        &bytes,
    )
    .unwrap();
    let groups_before = sim.groups.clone();
    let cache_before = sim.command_package_state.clone();
    sim.builds[0].build_masks = 0x20;
    assert_eq!(
        commit_simple_group_package_with_builds(
            &mut sim.world,
            &sim.unit_type,
            &mut sim.builds,
            &mut sim.groups,
            &mut sim.paths,
            &mut sim.command_package_state,
            &sim.group_move_authority,
            &sim.simple_group_action_authority,
            &sim.scenario_ignore_orders,
            &[0; NUM_LEADERS],
            Some(2),
            &player_who,
            prepared,
        ),
        Err(SimpleGroupPackageError::StaleBuild { who: 2, o })
    );
    assert_eq!(sim.groups.list, groups_before.list);
    assert_eq!(sim.command_package_state, cache_before);
    assert_eq!(sim.builds[0].build_masks, 0x20);
}

#[test]
fn retail_follow_queue_new_survives_donsave_cache_resume_and_near_target_frame() {
    const ACTOR_O: i16 = 0x3e;
    const TARGET_O: i16 = 10;
    let make = || {
        let mut sim = don_sim::tick::Sim::new(0x301e, 4);
        sim.world.frame = 29_421;
        sim.vic_match.frame = 29_421;
        let mut players = PlayerTable::new();
        players.seat(1, 1, 2, 0);
        sim.players = Some(players);
        let mut actor = None;
        for o in 0..=ACTOR_O {
            let handle = sim.spawn_unit(2, 30, 4_000, 4_000, 4).unwrap();
            assert_eq!(sim.world.units.o()[sim.world.row_of(handle).unwrap()], o);
            actor = Some(handle);
        }
        let actor = actor.unwrap();
        let mut target = None;
        for o in 0..=TARGET_O {
            let handle = sim.spawn_unit(7, 30, 4_010, 4_010, 4).unwrap();
            assert_eq!(sim.world.units.o()[sim.world.row_of(handle).unwrap()], o);
            target = Some(handle);
        }
        let target = target.unwrap();
        let actor_row = sim.world.row_of(actor).unwrap();
        let target_row = sim.world.row_of(target).unwrap();
        sim.world.units.o_down_mut()[actor_row] = -1;
        sim.world.units.inside_down_mut()[target_row] = -1;
        sim.world.units.inside_down_who_mut()[target_row] = -1;
        sim.world.units.set_idle(actor_row, 0);
        let install = |sim: &mut don_sim::tick::Sim| {
            sim.replace_group_move_authority(simple_authority(&[actor], 30, [0x30; 32]));
            sim.replace_simple_group_action_authority(SimpleGroupActionAuthority {
                revision: 30,
                composition_digest: [0x1e; 32],
                members: Vec::new(),
                builds: Vec::new(),
                follows: vec![
                    SimpleFollowUnitAuthority {
                        handle: actor,
                        canonical_o: i32::from(ACTOR_O),
                        is_plane: false,
                        speed: 0,
                        los: 4,
                        seen: true,
                        moving: false,
                        admits_idle_animation: true,
                    },
                    SimpleFollowUnitAuthority {
                        handle: target,
                        canonical_o: i32::from(TARGET_O),
                        is_plane: false,
                        speed: 0,
                        los: 4,
                        seen: true,
                        moving: false,
                        admits_idle_animation: false,
                    },
                ],
            });
        };
        install(&mut sim);
        (sim, actor, target)
    };
    let reinstall = |sim: &mut don_sim::tick::Sim, actor: Handle, target: Handle| {
        let mut move_authority = simple_authority(&[actor], 30, [0x30; 32]);
        move_authority.members[0].can_move = true;
        move_authority.members[0].can_install_order = true;
        move_authority.members[0].admits_unsplit_move_near = true;
        sim.replace_group_move_authority(move_authority);
        sim.replace_simple_group_action_authority(SimpleGroupActionAuthority {
            revision: 30,
            composition_digest: [0x1e; 32],
            members: Vec::new(),
            builds: Vec::new(),
            follows: vec![
                SimpleFollowUnitAuthority {
                    handle: actor,
                    canonical_o: i32::from(ACTOR_O),
                    is_plane: false,
                    speed: 0,
                    los: 4,
                    seen: true,
                    moving: false,
                    admits_idle_animation: true,
                },
                SimpleFollowUnitAuthority {
                    handle: target,
                    canonical_o: i32::from(TARGET_O),
                    is_plane: false,
                    speed: 0,
                    los: 4,
                    seen: true,
                    moving: false,
                    admits_idle_animation: false,
                },
            ],
        });
    };

    let (mut direct, actor, target) = make();
    let (mut resumed, resumed_actor, resumed_target) = make();
    // Playback___2017.07.15_00_10_31__Sat_.rcx package 9635: the actual cache origin
    // immediately before its later empty-Group FOLLOW package. The shared command state is
    // therefore seeded by the canonical Group+Move host, not by a synthetic FOLLOW packet.
    let retail_cache_seed = [
        0x00, 0x01, 0x01, 0x63, 0x00, 0x07, 0xb0, 0xda, 0x00, 0x00, 0xd4, 0xc9, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x00, 0x32, 0x00,
    ];
    let make_cached = || {
        let mut sim = don_sim::tick::Sim::new(0x301e, 4);
        sim.world.frame = 55_699;
        sim.vic_match.frame = 55_699;
        let mut players = PlayerTable::new();
        players.seat(0, 1, 1, 0);
        sim.players = Some(players);
        let mut actor = None;
        for o in 0..=0x63 {
            actor = Some(sim.spawn_unit(1, 30, 4_000, 4_000, 4).unwrap());
            assert_eq!(
                sim.world.units.o()[sim.world.row_of(actor.unwrap()).unwrap()],
                o
            );
        }
        let actor = actor.unwrap();
        let actor_row = sim.world.row_of(actor).unwrap();
        sim.world.units.o_down_mut()[actor_row] = -1;
        let mut target = None;
        for o in 0..=42 {
            target = Some(sim.spawn_unit(2, 30, 4_010, 4_010, 4).unwrap());
            assert_eq!(
                sim.world.units.o()[sim.world.row_of(target.unwrap()).unwrap()],
                o
            );
        }
        let target = target.unwrap();
        let target_row = sim.world.row_of(target).unwrap();
        sim.world.units.inside_down_mut()[target_row] = -1;
        sim.world.units.inside_down_who_mut()[target_row] = -1;
        let mut move_authority = simple_authority(&[actor], 30, [0x30; 32]);
        move_authority.members[0].can_move = true;
        move_authority.members[0].can_install_order = true;
        move_authority.members[0].admits_unsplit_move_near = true;
        sim.replace_group_move_authority(move_authority);
        sim.replace_simple_group_action_authority(SimpleGroupActionAuthority {
            revision: 30,
            composition_digest: [0x1e; 32],
            members: Vec::new(),
            builds: Vec::new(),
            follows: vec![
                SimpleFollowUnitAuthority {
                    handle: actor,
                    canonical_o: 0x63,
                    is_plane: false,
                    speed: 0,
                    los: 4,
                    seen: true,
                    moving: false,
                    admits_idle_animation: true,
                },
                SimpleFollowUnitAuthority {
                    handle: target,
                    canonical_o: 42,
                    is_plane: false,
                    speed: 0,
                    los: 4,
                    seen: true,
                    moving: false,
                    admits_idle_animation: false,
                },
            ],
        });
        (sim, actor, target)
    };
    let reinstall_cached = |sim: &mut don_sim::tick::Sim, actor: Handle, target: Handle| {
        sim.replace_group_move_authority(simple_authority(&[actor], 30, [0x30; 32]));
        sim.replace_simple_group_action_authority(SimpleGroupActionAuthority {
            revision: 30,
            composition_digest: [0x1e; 32],
            members: Vec::new(),
            builds: Vec::new(),
            follows: vec![
                SimpleFollowUnitAuthority {
                    handle: actor,
                    canonical_o: 0x63,
                    is_plane: false,
                    speed: 0,
                    los: 4,
                    seen: true,
                    moving: false,
                    admits_idle_animation: true,
                },
                SimpleFollowUnitAuthority {
                    handle: target,
                    canonical_o: 42,
                    is_plane: false,
                    speed: 0,
                    los: 4,
                    seen: true,
                    moving: false,
                    admits_idle_animation: false,
                },
            ],
        });
    };
    let (mut cached_direct, cached_actor, cached_target) = make_cached();
    let (mut cached_resumed, cached_resumed_actor, cached_resumed_target) = make_cached();
    cached_direct
        .process_command_package(0, 9_636, &retail_cache_seed)
        .unwrap();
    cached_resumed
        .process_command_package(0, 9_636, &retail_cache_seed)
        .unwrap();
    let saved_cache_seed = save_sim(&cached_resumed).unwrap();
    let mut cached_resumed = load_sim(&saved_cache_seed).unwrap();
    reinstall_cached(
        &mut cached_resumed,
        cached_resumed_actor,
        cached_resumed_target,
    );
    let retail_cached = follow_packet(1, &[], 42, 2, 2);
    assert_eq!(
        retail_cached,
        [0x00, 0x00, 0x01, 0x1e, 0x2a, 0, 0, 0, 0x02, 0, 0, 0, 0x02, 0, 0, 0]
    );
    let cached_direct_receipt = cached_direct
        .process_simple_group_package(0, 9_657, &retail_cached)
        .unwrap();
    let cached_resumed_receipt = cached_resumed
        .process_simple_group_package(0, 9_657, &retail_cached)
        .unwrap();
    assert_eq!(
        cached_direct_receipt.action_result,
        SimpleGroupActionResult::Follow { installed_units: 1 }
    );
    assert_eq!(
        cached_direct_receipt.groups_checksum,
        cached_resumed_receipt.groups_checksum
    );
    assert_eq!(
        cached_direct
            .world
            .orders(cached_direct.world.row_of(cached_actor).unwrap()),
        cached_resumed
            .world
            .orders(cached_resumed.world.row_of(cached_resumed_actor).unwrap())
    );
    assert!(cached_direct.world.row_of(cached_target).is_some());

    // Playback___2024.03.18_18_18_49__Mon_.rcx, package 7353 / frame 29421.
    let retail = follow_packet(2, &[ACTOR_O], 10, 7, 2);
    assert_eq!(
        retail,
        [0x00, 0x01, 0x02, 0x3e, 0x00, 0x1e, 0x0a, 0, 0, 0, 0x07, 0, 0, 0, 0x02, 0, 0, 0]
    );
    let (mut ignored, ignored_actor, _) = make();
    ignored.scenario_ignore_orders.ignore_orders = true;
    let ignored_groups = ignored.groups.clone();
    let ignored_cache = ignored.command_package_state.clone();
    assert_eq!(
        ignored.process_simple_group_package(1, 7_354, &retail),
        Err(SimpleGroupPackageError::FollowIgnoreOrdersPrelude)
    );
    assert_eq!(ignored.groups.list, ignored_groups.list);
    assert_eq!(ignored.command_package_state, ignored_cache);
    assert!(ignored
        .world
        .orders(ignored.world.row_of(ignored_actor).unwrap())
        .is_empty());
    let (mut unsupported, unsupported_actor, _) = make();
    let unsupported_row = unsupported.world.row_of(unsupported_actor).unwrap();
    unsupported
        .world
        .orders_mut(unsupported_row)
        .push(cast_spell_order(0x293));
    let unsupported_groups = unsupported.groups.clone();
    let unsupported_cache = unsupported.command_package_state.clone();
    assert_eq!(
        unsupported.process_simple_group_package(1, 7_354, &retail),
        Err(SimpleGroupPackageError::UnsupportedFollowOrderRetirement {
            handle: unsupported_actor,
            kind: OrderIndex::CastSpell,
        })
    );
    assert_eq!(unsupported.groups.list, unsupported_groups.list);
    assert_eq!(unsupported.command_package_state, unsupported_cache);
    assert_eq!(
        unsupported.world.orders(unsupported_row).order_type(),
        OrderIndex::CastSpell
    );
    let first = direct
        .process_simple_group_package(1, 7_354, &retail)
        .unwrap();
    resumed
        .process_simple_group_package(1, 7_354, &retail)
        .unwrap();
    assert_eq!(
        first.action_result,
        SimpleGroupActionResult::Follow { installed_units: 1 }
    );
    let actor_row = direct.world.row_of(actor).unwrap();
    let payload = direct
        .world
        .orders(actor_row)
        .current()
        .unwrap()
        .follow
        .unwrap();
    assert_eq!(
        (payload.ox, payload.whom, payload.oxx, payload.whose),
        (10, 7, 10, 7)
    );
    assert_eq!(direct.groups.list[first.group_slot].form, -1);

    let saved = save_sim(&resumed).unwrap();
    let mut resumed = load_sim(&saved).unwrap();
    reinstall(&mut resumed, resumed_actor, resumed_target);
    let cached = follow_packet(2, &[], 10, 7, 2);
    let direct_cached = direct
        .process_simple_group_package(1, 7_355, &cached)
        .unwrap();
    let resumed_cached = resumed
        .process_simple_group_package(1, 7_355, &cached)
        .unwrap();
    assert_eq!(direct_cached.action_result, resumed_cached.action_result);
    assert_eq!(
        direct_cached.groups_checksum,
        resumed_cached.groups_checksum
    );
    assert_eq!(direct.world.random.state(), resumed.world.random.state());
    prepare_simple_follow_activation(
        &direct.world,
        &direct.simple_group_action_authority,
        direct.world.row_of(actor).unwrap(),
    )
    .unwrap();
    prepare_simple_follow_activation(
        &resumed.world,
        &resumed.simple_group_action_authority,
        resumed.world.row_of(resumed_actor).unwrap(),
    )
    .unwrap();

    direct.do_frame();
    resumed.do_frame();
    let direct_row = direct.world.row_of(actor).unwrap();
    let resumed_row = resumed.world.row_of(resumed_actor).unwrap();
    assert_eq!(direct.world.units.get_idle(direct_row), 1);
    assert_eq!(resumed.world.units.get_idle(resumed_row), 1);
    assert_eq!(
        direct.world.orders(direct_row),
        resumed.world.orders(resumed_row)
    );
    assert_eq!(direct.groups.list, resumed.groups.list);
    assert_eq!(direct.world.random.state(), resumed.world.random.state());
    assert_eq!(direct.world.row_of(target).is_some(), true);
}

#[test]
fn follow_queue_first_refuses_before_group_cache_or_unit_publication() {
    let fixture = Fixture::new(2);
    let actor_o = fixture.objects[0];
    let bytes = follow_packet(0, &[actor_o], i32::from(actor_o), 0, 0);
    let groups_before = fixture.groups.clone();
    let state_before = fixture.state.clone();
    let group_before = fixture.world.units.group()[0];
    assert_eq!(
        fixture.prepare(30, &bytes).unwrap_err(),
        SimpleGroupPackageError::UnsupportedFollowQueue { queued: 0 }
    );
    assert_eq!(fixture.groups.list, groups_before.list);
    assert_eq!(fixture.state, state_before);
    assert_eq!(fixture.world.units.group()[0], group_before);
    assert!(fixture.world.orders(0).is_empty());
}
