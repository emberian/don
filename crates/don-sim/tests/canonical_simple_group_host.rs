// SPDX-License-Identifier: GPL-3.0-or-later
//! Focused canonical `[Group][one simple action]` transaction tests.

use don_sim::order::{Order, OrderIndex, SpecialAnimType, ORDER_GROUP};
use don_sim::systems::canonical_group_move_host::{
    retail_fresh_groups, CommandPackageState, GroupMoveAuthority, MoveMemberAuthority,
    NETWORK_PLAYERS,
};
use don_sim::systems::canonical_simple_group_host::*;
use don_sim::systems::economy_order_payload_authority::{CastOrderPayload, EconomyOrderPayload};
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

struct Fixture {
    world: World,
    unit_types: Vec<i32>,
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
        let mut players = [None; NETWORK_PLAYERS];
        players[0] = Some(who);
        Self {
            paths: vec![PathStack::default(); world.live_count() as usize],
            world,
            unit_types,
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
            &self.unit_types,
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
            &self.unit_types,
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
