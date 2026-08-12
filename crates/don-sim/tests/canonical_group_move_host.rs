// SPDX-License-Identifier: GPL-3.0-or-later
//! Focused contract tests for the unregistered canonical Group -> Move host.

mod order {
    pub use don_sim::order::*;
}
mod world {
    pub use don_sim::world::*;
}
mod systems {
    pub mod groups_guys {
        pub use don_sim::systems::groups_guys::*;
    }
    pub mod movement {
        pub use don_sim::systems::movement::*;
    }
    pub mod production {
        pub use don_sim::systems::production::*;
    }
    pub mod sparse_object_bands_authority_frontier {
        pub use don_sim::systems::sparse_object_bands_authority_frontier::*;
    }
}

#[path = "../src/systems/canonical_group_move_host.rs"]
mod subject;

use don_sim::order::{Order, OrderIndex};
use don_sim::systems::groups_guys::FormationMember;
use don_sim::systems::movement::{PathData, PathStack};
use don_sim::world::{Handle, World};
use subject::*;

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
        let mut world = World::new(9);
        let mut handles = Vec::new();
        let mut objects = Vec::new();
        for index in 0..count {
            let handle = world
                .allocate_typed_at(3, 100 + index as i32, 2_000 + index as i32 * 240, 3_000)
                .unwrap();
            let row = world.row_of(handle).unwrap();
            world.units.group_mut()[row] = -1;
            world.units.o_down_mut()[row] = -1;
            world.units.form_mut()[row] = 0;
            world.units.form_mod_mut()[row] = 50;
            world.units.angle_mut()[row] = 0x1200_0000 + index as i32 * 0x0100_0000;
            world.units.set_unit_masks(row, 0x0400_0400);
            handles.push(handle);
            objects.push(world.units.o()[row]);
        }
        let paths = vec![PathStack::default(); world.live_count() as usize];
        let members = handles
            .iter()
            .enumerate()
            .map(|(index, &handle)| MoveMemberAuthority {
                handle,
                role: 0x100 << index,
                on_map: true,
                is_captain: true,
                can_move: true,
                can_install_order: true,
                is_plane: false,
                domain: 0,
                unit_flags: 0,
                speed: 20 + index as i32,
                admits_unsplit_move_near: true,
                land_formation: FormationMember {
                    category: 0,
                    x_spacing: 48,
                    y_spacing: 48,
                    formation_size: 1,
                    guy_spacing: 48,
                    modern_infantry: false,
                    width: 50,
                    angle: 0,
                },
                water_formation: FormationMember {
                    category: 0,
                    ..FormationMember::default()
                },
            })
            .collect();
        let mut players = [None; NETWORK_PLAYERS];
        players[0] = Some(3);
        players[1] = Some(3);
        Self {
            world,
            groups: retail_fresh_groups(),
            paths,
            state: CommandPackageState::default(),
            authority: GroupMoveAuthority {
                revision: 4,
                composition_digest: [0xa5; 32],
                destination_is_water: false,
                force_formation_facing_zero: false,
                members,
            },
            players,
            handles,
            objects,
        }
    }

    fn prepare(
        &self,
        play: usize,
        serial: i32,
        bytes: &[u8],
    ) -> Result<PreparedGroupMovePackage, PackageError> {
        prepare_group_move_package(
            &self.world,
            &self.groups,
            &self.paths,
            &self.state,
            &self.authority,
            &self.players,
            (256, 256),
            77,
            play,
            serial,
            bytes,
        )
    }

    fn process(
        &mut self,
        play: usize,
        serial: i32,
        bytes: &[u8],
    ) -> Result<GroupMovePackageReceipt, PackageError> {
        let prepared = self.prepare(play, serial, bytes)?;
        commit_group_move_package(
            &mut self.world,
            &mut self.groups,
            &mut self.paths,
            &mut self.state,
            &self.authority,
            prepared,
        )
    }
}

fn package(objects: &[i16], movement: MoveToWire) -> Vec<u8> {
    let mut bytes = vec![GROUP_OPCODE, objects.len() as u8, 3];
    for &o in objects {
        bytes.extend_from_slice(&o.to_le_bytes());
    }
    bytes.push(MOVE_TO_OPCODE);
    bytes.extend_from_slice(&movement.x.to_le_bytes());
    bytes.extend_from_slice(&movement.y.to_le_bytes());
    bytes.extend_from_slice(&movement.set_angle.to_le_bytes());
    bytes.extend_from_slice(&movement.angle.to_le_bytes());
    bytes.push(movement.orders as u8);
    bytes.push(movement.queued as u8);
    bytes.push(movement.form as u8);
    bytes.push(movement.width as u8);
    bytes.push(movement.disembark as u8);
    bytes
}

fn movement() -> MoveToWire {
    MoveToWire {
        x: 12_000,
        y: 16_000,
        set_angle: 0,
        angle: 0,
        orders: 1,
        queued: 2,
        form: 0,
        width: 50,
        disembark: 0,
    }
}

#[test]
fn decoder_retains_all_nine_fields_and_requires_exact_chronology() {
    let fields = MoveToWire {
        x: -12,
        y: 99,
        set_angle: 0x1020_3040,
        angle: 0x5566_7788,
        orders: 4,
        queued: 1,
        form: -1,
        width: -1,
        disembark: -7,
    };
    let bytes = package(&[12, 31], fields);
    assert_eq!(
        decode_group_move_package(&bytes).unwrap(),
        GroupMoveWire {
            who: 3,
            objects: vec![12, 31],
            movement: fields,
        }
    );
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert_eq!(
        decode_group_move_package(&trailing),
        Err(PackageError::TrailingBytes {
            expected: bytes.len(),
            got: bytes.len() + 1,
        })
    );
    let mut reversed = bytes;
    reversed[0] = MOVE_TO_OPCODE;
    assert_eq!(
        decode_group_move_package(&reversed),
        Err(PackageError::WrongFirstOpcode {
            got: MOVE_TO_OPCODE
        })
    );
}

#[test]
fn explicit_package_publishes_canonical_group_backlink_order_path_and_cache() {
    let mut fixture = Fixture::new(1);
    let row = fixture.world.row_of(fixture.handles[0]).unwrap();
    fixture.paths[row].push(PathData {
        to_x: 1,
        to_y: 2,
        tolerance: 3,
        flags: 4,
    });
    let bytes = package(&fixture.objects, movement());
    let receipt = fixture.process(0, 91, &bytes).unwrap();
    assert_eq!(receipt.play, 0);
    assert_eq!(receipt.lockstep_serial, 91);
    assert_eq!(receipt.frame, 77);
    assert_eq!(receipt.random_state_before, receipt.random_state_after);
    assert_eq!(receipt.selected.len(), 1);

    assert_eq!(fixture.groups.last_group[3], 3 * 64 + 1);
    let slot = fixture.groups.last_group[3] as usize;
    let group = &fixture.groups.list[slot];
    assert_eq!(group.id, slot as i32);
    assert_eq!(group.list[..group.num as usize], fixture.objects);
    assert_eq!(group.stamp, 77);
    assert_eq!(group.order_num, 1);
    assert_eq!(fixture.world.units.group()[row], slot as i16);
    assert_eq!(fixture.world.units.get_unit_masks(row), 0);
    assert_eq!(fixture.paths[row].len(), 1);
    assert_eq!(
        fixture.state.selection(0).unwrap(),
        &[CachedSelection {
            o: fixture.objects[0],
            uid: fixture.world.units.get_uid(row),
        }]
    );
    let order = fixture.world.orders(row).current().unwrap();
    assert_eq!(
        fixture.paths[row].peek(),
        Some(PathData {
            to_x: order.x,
            to_y: order.y,
            tolerance: 0,
            flags: PathData::FLAG_MORE,
        })
    );
    assert_eq!(order.kind, OrderIndex::MoveTo);
    assert_eq!(order.flags, ORDER_GROUP | ORDER_FORM);
    let state = order.move_state.unwrap();
    assert_eq!((state.orig_x, state.orig_y), (12_000, 16_000));
    assert_eq!((state.dest_x, state.dest_y), (order.x, order.y));
    assert_eq!(fixture.world.units.orders_x()[row], order.x);
    assert_eq!(fixture.world.units.orders_y()[row], order.y);
}

#[test]
fn command_coordinates_are_clamped_in_the_pathfinder_tile_scale() {
    let mut fixture = Fixture::new(1);
    let bytes = package(
        &fixture.objects,
        MoveToWire {
            x: 100_000,
            y: 200_000,
            ..movement()
        },
    );
    fixture.process(0, 1, &bytes).unwrap();
    let row = fixture.world.row_of(fixture.handles[0]).unwrap();
    let order = fixture.world.orders(row).current().unwrap();
    // Formation destinations are snapped to the final 48-unit cell centre inside the
    // 256 * 192 authoritative terrain span.
    assert_eq!((order.x, order.y), (256 * 192 - 24, 256 * 192 - 24));
    assert_eq!(
        (
            order.move_state.unwrap().orig_x,
            order.move_state.unwrap().orig_y
        ),
        (256 * 192 - 1, 256 * 192 - 1)
    );
    assert_eq!(
        fixture.paths[row].peek().map(|path| (path.to_x, path.to_y)),
        Some((order.x, order.y))
    );
}

#[test]
fn empty_group_reuses_the_play_cache_without_rewriting_it() {
    let mut fixture = Fixture::new(1);
    let explicit = package(&fixture.objects, movement());
    fixture.process(0, 1, &explicit).unwrap();
    let cache = fixture.state.saved_selections();
    let slot = fixture.groups.last_group[3];

    let empty = package(
        &[],
        MoveToWire {
            x: 22_000,
            ..movement()
        },
    );
    fixture.process(0, 2, &empty).unwrap();
    assert_eq!(fixture.state.saved_selections(), cache);
    assert_eq!(fixture.groups.last_group[3], slot);
    assert_eq!(fixture.groups.list[slot as usize].order_num, 2);
    let row = fixture.world.row_of(fixture.handles[0]).unwrap();
    assert_eq!(
        fixture
            .world
            .orders(row)
            .current()
            .unwrap()
            .move_state
            .unwrap()
            .orig_x,
        22_000
    );
}

#[test]
fn cache_is_keyed_by_play_even_when_two_plays_map_to_the_same_owner() {
    let mut fixture = Fixture::new(1);
    let explicit = package(&fixture.objects, movement());
    fixture.process(0, 1, &explicit).unwrap();
    let empty = package(&[], movement());
    assert!(matches!(
        fixture.prepare(1, 2, &empty),
        Err(PackageError::EmptyEffectiveSelection)
    ));
    fixture.process(0, 3, &empty).unwrap();
}

#[test]
fn duplicates_survive_in_the_cache_but_not_in_the_effective_group() {
    let mut fixture = Fixture::new(1);
    let objects = [fixture.objects[0], fixture.objects[0]];
    fixture
        .process(0, 1, &package(&objects, movement()))
        .unwrap();
    assert_eq!(fixture.state.selection(0).unwrap().len(), 2);
    let group = &fixture.groups.list[fixture.groups.last_group[3] as usize];
    assert_eq!(group.num, 1);
}

#[test]
fn changed_before_image_rejects_the_whole_commit() {
    let mut fixture = Fixture::new(1);
    let bytes = package(&fixture.objects, movement());
    let prepared = fixture.prepare(0, 1, &bytes).unwrap();
    let groups_before = fixture.groups.clone();
    let state_before = fixture.state.clone();
    let row = fixture.world.row_of(fixture.handles[0]).unwrap();
    fixture.world.orders_mut(row).push(Order::move_to(1, 2, 0));
    let changed_orders = fixture.world.orders(row).clone();

    assert_eq!(
        commit_group_move_package(
            &mut fixture.world,
            &mut fixture.groups,
            &mut fixture.paths,
            &mut fixture.state,
            &fixture.authority,
            prepared,
        ),
        Err(PackageError::StaleUnit {
            handle: fixture.handles[0]
        })
    );
    assert_eq!(fixture.groups.list, groups_before.list);
    assert_eq!(fixture.groups.last_group, groups_before.last_group);
    assert_eq!(fixture.state, state_before);
    assert_eq!(fixture.world.orders(row), &changed_orders);
    assert_eq!(fixture.world.units.group()[row], -1);
}

#[test]
fn recycled_handle_is_rejected_even_when_retail_uid_and_address_repeat() {
    let mut fixture = Fixture::new(1);
    let bytes = package(&fixture.objects, movement());
    let prepared = fixture.prepare(0, 1, &bytes).unwrap();
    let old = fixture.handles[0];
    let old_uid = fixture
        .world
        .units
        .get_uid(fixture.world.row_of(old).unwrap());
    assert!(fixture.world.despawn(old));
    let replacement = fixture
        .world
        .allocate_typed_at(3, 100, 4_000, 4_000)
        .unwrap();
    let row = fixture.world.row_of(replacement).unwrap();
    fixture.world.units.group_mut()[row] = -1;
    fixture.world.units.o_down_mut()[row] = -1;
    assert_eq!(fixture.world.units.o()[row], fixture.objects[0]);
    assert_eq!(fixture.world.units.get_uid(row), old_uid);
    assert_ne!(replacement, old);

    assert_eq!(
        commit_group_move_package(
            &mut fixture.world,
            &mut fixture.groups,
            &mut fixture.paths,
            &mut fixture.state,
            &fixture.authority,
            prepared,
        ),
        Err(PackageError::StaleUnit { handle: old })
    );
    assert_eq!(fixture.groups.last_group[3], 3 * 64);
    assert!(fixture.state.selection(0).unwrap().is_empty());
}

#[test]
fn live_member_without_exact_authority_fails_closed_instead_of_becoming_an_empty_selection() {
    let mut fixture = Fixture::new(1);
    fixture.authority.members.clear();
    let bytes = package(&fixture.objects, movement());
    assert_eq!(
        fixture.process(0, 1, &bytes),
        Err(PackageError::MissingAuthority {
            handle: fixture.handles[0]
        })
    );
    assert_eq!(fixture.world.units.group()[0], -1);
    assert!(fixture.state.selection(0).unwrap().is_empty());
}

#[test]
fn move_field_mutations_change_the_published_state() {
    fn outcome(movement: MoveToWire) -> (Order, don_sim::systems::groups_guys::GroupData) {
        let mut fixture = Fixture::new(5);
        fixture
            .process(0, 1, &package(&fixture.objects.clone(), movement))
            .unwrap();
        let row = fixture.world.row_of(fixture.handles[0]).unwrap();
        (
            fixture.world.orders(row).current().unwrap().clone(),
            fixture.groups.list[fixture.groups.last_group[3] as usize].clone(),
        )
    }

    let base = outcome(movement());
    assert_ne!(
        outcome(MoveToWire {
            x: 13_000,
            ..movement()
        }),
        base
    );
    assert_ne!(
        outcome(MoveToWire {
            y: 17_000,
            ..movement()
        }),
        base
    );
    assert_ne!(
        outcome(MoveToWire {
            set_angle: 1,
            angle: 0x2300_0000,
            ..movement()
        }),
        base
    );
    assert_eq!(
        outcome(MoveToWire {
            orders: 3,
            ..movement()
        })
        .0
        .kind,
        OrderIndex::ExploreTo
    );
    assert_ne!(
        outcome(MoveToWire {
            form: 1,
            ..movement()
        }),
        base
    );
    assert_ne!(
        outcome(MoveToWire {
            width: 0,
            ..movement()
        }),
        base
    );
    assert_eq!(
        outcome(MoveToWire {
            disembark: 1,
            ..movement()
        })
        .0
        .flags
            & ORDER_DISEMBARK,
        ORDER_DISEMBARK
    );
}

#[test]
fn queue_last_appends_and_queue_first_fails_without_mutation() {
    let mut fixture = Fixture::new(1);
    let row = fixture.world.row_of(fixture.handles[0]).unwrap();
    fixture
        .world
        .orders_mut(row)
        .push(Order::move_to(100, 200, 0));
    fixture
        .process(
            0,
            1,
            &package(
                &fixture.objects.clone(),
                MoveToWire {
                    queued: 1,
                    ..movement()
                },
            ),
        )
        .unwrap();
    assert_eq!(fixture.world.orders(row).len(), 2);

    let groups_before = fixture.groups.clone();
    let state_before = fixture.state.clone();
    let orders_before = fixture.world.orders(row).clone();
    assert_eq!(
        fixture.process(
            0,
            2,
            &package(
                &fixture.objects.clone(),
                MoveToWire {
                    queued: 0,
                    ..movement()
                }
            ),
        ),
        Err(PackageError::QueueFirstBoundary)
    );
    assert_eq!(fixture.groups.list, groups_before.list);
    assert_eq!(fixture.groups.last_group, groups_before.last_group);
    assert_eq!(fixture.state, state_before);
    assert_eq!(fixture.world.orders(row), &orders_before);
}

#[test]
fn saved_cache_validation_preserves_stale_entries_but_rejects_bad_shape() {
    let stale = CachedSelection { o: 17, uid: 0xffff };
    let state = CommandPackageState::from_saved_selections(std::array::from_fn(|play| {
        if play == 4 {
            vec![stale, stale]
        } else {
            Vec::new()
        }
    }))
    .unwrap();
    assert_eq!(state.selection(4).unwrap(), &[stale, stale]);
    let too_long = std::array::from_fn(|play| {
        if play == 0 {
            vec![CachedSelection { o: 1, uid: 0 }; RECEIVED_SELECTION_CAPACITY + 1]
        } else {
            Vec::new()
        }
    });
    assert_eq!(
        CommandPackageState::from_saved_selections(too_long),
        Err(PackageError::SavedCacheTooLong {
            play: 0,
            len: RECEIVED_SELECTION_CAPACITY + 1,
        })
    );
}
