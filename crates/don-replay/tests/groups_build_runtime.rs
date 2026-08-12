use don_replay::build_spawn_runtime::{spawn_canonical_build, CanonicalBuildSpawnRequest};
use don_replay::groups_build_history::QueueUpBuildWire;
use don_replay::groups_build_runtime::*;
use don_sim::order::OrderIndex;
use don_sim::systems::canonical_group_move_host::{
    GroupMoveAuthority, MoveMemberAuthority, UnitIdentity,
};
use don_sim::systems::groups_guys::FormationMember;
use don_sim::systems::map_terrain::{Coord, WCoord};
use don_sim::systems::production::{self, BuildData};
use don_sim::systems::save_load::{load_sim, save_sim};
use don_sim::tick::lifecycle_host::PlayerTable;
use don_sim::tick::Sim;
use don_sim::world::Handle;

fn packet(owner: u8, objects: &[i16], action: QueueUpBuildWire) -> Vec<u8> {
    let mut bytes = vec![0, objects.len() as u8, owner];
    for &o in objects {
        bytes.extend_from_slice(&o.to_le_bytes());
    }
    bytes.push(25);
    bytes.extend_from_slice(&action.x.to_le_bytes());
    bytes.extend_from_slice(&action.y.to_le_bytes());
    bytes.extend_from_slice(&action.x2.to_le_bytes());
    bytes.extend_from_slice(&action.y2.to_le_bytes());
    bytes.extend_from_slice(&action.type_index.to_le_bytes());
    bytes.extend_from_slice(&action.queued.to_le_bytes());
    bytes
}

fn blank_build(type_index: i32, uid: u16) -> BuildData {
    let mut build = BuildData {
        flags: production::flag::VALID,
        uid,
        orig_type: type_index,
        gather_down: -1,
        city: -1,
        city_down: -1,
        wonder: -1,
        dock: -1,
        attack_ox: -1,
        attack_whom: -1,
        founder: 0,
        build_masks: BUILD_BAD_PATH_MASK,
        ..BuildData::default()
    };
    build.other[0x28..0x2a].copy_from_slice(&(-1i16).to_le_bytes());
    build
}

struct Fixture {
    sim: Sim,
    actor: Handle,
    actor_o: i16,
    action: QueueUpBuildWire,
    authority: GroupBuildRuntimeAuthority,
    farm_cell: usize,
}

impl Fixture {
    fn new() -> Self {
        let mut sim = Sim::new(0x51de, 16);
        sim.world.frame = 79;
        sim.vic_match.frame = 79;
        let mut players = PlayerTable::new();
        players.seat(0, 1, 0, 0);
        sim.players = Some(players);

        let actor = sim.spawn_unit(0, 71, 2_200, 3_400, 4).unwrap();
        let actor_row = sim.world.row_of(actor).unwrap();
        sim.world.units.group_mut()[actor_row] = -1;
        sim.world.units.o_down_mut()[actor_row] = -1;
        sim.world.units.set_unit_masks(actor_row, 0);
        let actor_o = sim.world.units.o()[actor_row];
        let identity = UnitIdentity {
            handle: actor,
            who: 0,
            o: actor_o,
            uid: sim.world.units.get_uid(actor_row),
        };
        sim.replace_group_move_authority(GroupMoveAuthority {
            revision: 9,
            composition_digest: [0x91; 32],
            destination_is_water: false,
            force_formation_facing_zero: false,
            members: vec![MoveMemberAuthority {
                handle: actor,
                role: 0,
                on_map: true,
                is_captain: true,
                can_move: true,
                can_install_order: true,
                is_plane: false,
                domain: 0,
                unit_flags: 0,
                speed: 34,
                admits_unsplit_move_near: false,
                land_formation: FormationMember::default(),
                water_formation: FormationMember::default(),
            }],
        });

        let center_position = (2_000, 2_000);
        let center = spawn_canonical_build(
            &mut sim,
            CanonicalBuildSpawnRequest {
                owner: 0,
                type_index: 0x19e,
                snapped_x: center_position.0,
                snapped_y: center_position.1,
                build: blank_build(0x19e, 31),
            },
        )
        .unwrap();
        sim.builds[center.row].city = 0;
        sim.builds[center.row].build_masks = 0;
        let city = &mut sim.cities.slots[0][0];
        city.city_flags = 1;
        city.city = 0;
        city.o = center.object_id;
        city.who = 0;
        sim.cities.city_mark[0] = 1;

        let action = QueueUpBuildWire {
            x: 7_100,
            y: 8_200,
            x2: 7_100,
            y2: 8_200,
            type_index: FARM_TYPE,
            queued: QUEUE_LAST,
        };
        let snapped = (7_296, 8_064);
        let wcoord = (
            WCoord::from_coord(Coord(snapped.0)).0,
            WCoord::from_coord(Coord(snapped.1)).0,
        );
        let farm_cell = sim.map.world.w_index(wcoord.0, wcoord.1);
        let head = (
            sim.map.world.wdata[farm_cell].down,
            sim.map.world.wdata[farm_cell].down_who,
        );
        assert_eq!(head.0, -1);

        let destination = (6_984, 8_232);
        let angle = 0x1234_0000;
        let move_order =
            queue_last_swarm_move_order(OrderIndex::MoveTo, destination, angle).unwrap();
        let farm_uid = 77;
        let build_order = queue_last_build_order(0, 2001, farm_uid);
        let authority = GroupBuildRuntimeAuthority {
            revision: 3,
            composition_digest: [0x5a; 32],
            scenario_ignore_orders: false,
            placements: vec![GroupBuildPlacementAuthority {
                owner: 0,
                requested: action,
                source: PlacementAuthoritySource::BuildTypeValidateSnapAndBlockedSite,
                probes: vec![PlacementProbeAuthority {
                    raw: (action.x, action.y),
                    snapped,
                    blocked_site: 0,
                }],
                chosen_probe: 0,
                world_link: WorldObjectLinkAuthority {
                    wcoord,
                    head_before: head,
                },
                city: CityBuildChainAuthority {
                    owner: 0,
                    city_slot: 0,
                    city_before: sim.cities.slots[0][0].clone(),
                    chain: vec![CityBuildChainNode {
                        row: center.row,
                        o: center.object_id,
                        city_down: -1,
                    }],
                },
                build_source: BuildBodyAuthoritySource::ObjectsInitBuildPeAfterImage,
                initialized_build: blank_build(FARM_TYPE, farm_uid),
                builders: vec![BuilderSwarmAuthority {
                    actor: identity,
                    first_nearby_spot: (7_000, 8_200),
                    second_nearby_spot: Some(destination),
                    final_destination: destination,
                    facing_angle: angle,
                    move_order,
                    build_order,
                    unit_masks_after: BUILD_AT_UNIT_MASK,
                    orders_x_after: destination.0,
                    orders_y_after: destination.1,
                    dest_angle_after: angle,
                }],
            }],
        };

        Self {
            sim,
            actor,
            actor_o,
            action,
            authority,
            farm_cell,
        }
    }

    fn bytes(&self, objects: &[i16]) -> Vec<u8> {
        packet(0, objects, self.action)
    }
}

#[test]
fn exact_opcode25_transaction_publishes_build_city_world_group_and_two_orders_once() {
    let mut fixture = Fixture::new();
    let bytes = fixture.bytes(&[fixture.actor_o]);
    let random_before = fixture.sim.world.random.state();
    let receipt =
        process_group_build_package(&mut fixture.sim, &fixture.authority, 0, 14, &bytes).unwrap();

    assert_eq!((receipt.build.row, receipt.build.object_id), (1, 2001));
    assert_eq!(receipt.city_slot, 0);
    assert_eq!(receipt.city_tail_row, 0);
    assert_eq!(receipt.world_cell, fixture.farm_cell);
    assert_eq!(receipt.installed_move_orders, 1);
    assert_eq!(receipt.installed_build_orders, 1);
    assert!(!receipt.resources_mutated);
    assert!(!receipt.production_queue_mutated);
    assert_eq!(receipt.random_state_before, random_before);
    assert_eq!(receipt.random_state_after, random_before);

    let farm = &fixture.sim.builds[1];
    assert_eq!((farm.who, farm.object_id(), farm.city), (0, 2001, 0));
    assert_eq!(farm.city_down, -1);
    assert_eq!(farm.build_masks & BUILD_BAD_PATH_MASK, 0);
    assert_eq!(fixture.sim.builds[0].city_down, 2001);
    assert_eq!(fixture.sim.map.world.wdata[fixture.farm_cell].down, 2001);
    assert_eq!(fixture.sim.map.world.wdata[fixture.farm_cell].down_who, 0);

    let row = fixture.sim.world.row_of(fixture.actor).unwrap();
    assert_eq!(
        fixture.sim.world.units.group()[row],
        receipt.group_slot as i16
    );
    assert_eq!(fixture.sim.world.units.get_unit_masks(row), 0x400);
    let orders = fixture.sim.world.orders(row).iter().collect::<Vec<_>>();
    assert_eq!(orders.len(), 2);
    assert_eq!(orders[0].kind, OrderIndex::MoveTo);
    assert_eq!(orders[1].kind, OrderIndex::BuildAt);
    assert_eq!((orders[1].target_who, orders[1].target_o), (0, 2001));
    assert_eq!(fixture.sim.groups.list[receipt.group_slot].form, -1);
    assert_eq!(fixture.sim.groups.list[receipt.group_slot].disband, 0);
    assert_eq!(
        fixture.sim.command_package_state.selection(0).unwrap()[0].o,
        fixture.actor_o
    );
}

#[test]
fn blocked_site_and_world_head_mutations_refuse_before_any_owner_changes() {
    let mut blocked = Fixture::new();
    blocked.authority.placements[0].probes[0].blocked_site = 7;
    let bytes = blocked.bytes(&[blocked.actor_o]);
    let groups_before = blocked.sim.groups.clone();
    let state_before = blocked.sim.command_package_state.clone();
    let orders_before = blocked
        .sim
        .world
        .orders(blocked.sim.world.row_of(blocked.actor).unwrap())
        .clone();
    assert_eq!(
        process_group_build_package(&mut blocked.sim, &blocked.authority, 0, 14, &bytes),
        Err(GroupBuildPackageError::ChosenSiteBlocked { code: 7 })
    );
    assert_eq!(blocked.sim.builds.len(), 1);
    assert_eq!(blocked.sim.groups.list, groups_before.list);
    assert_eq!(blocked.sim.command_package_state, state_before);
    assert_eq!(
        blocked
            .sim
            .world
            .orders(blocked.sim.world.row_of(blocked.actor).unwrap()),
        &orders_before
    );

    let mut occupied = Fixture::new();
    occupied.sim.map.world.wdata[occupied.farm_cell].down = 123;
    occupied.authority.placements[0].world_link.head_before.0 = 123;
    let bytes = occupied.bytes(&[occupied.actor_o]);
    assert_eq!(
        process_group_build_package(&mut occupied.sim, &occupied.authority, 0, 14, &bytes),
        Err(GroupBuildPackageError::OccupiedWorldHeadUnsupported { head: (123, 0) })
    );
    assert_eq!(occupied.sim.builds.len(), 1);
    assert_eq!(occupied.sim.builds[0].city_down, -1);
    assert!(occupied.sim.command_package_state.is_empty());
}

#[test]
fn empty_group_reuses_the_canonical_cache_for_the_next_exact_wire_shape() {
    let mut fixture = Fixture::new();
    let first = fixture.bytes(&[fixture.actor_o]);
    process_group_build_package(&mut fixture.sim, &fixture.authority, 0, 14, &first).unwrap();

    let second_action = QueueUpBuildWire {
        x: 8_900,
        y: 9_600,
        x2: 8_900,
        y2: 9_600,
        type_index: FARM_TYPE,
        queued: QUEUE_LAST,
    };
    let snapped = (8_832, 9_600);
    let wcoord = (
        WCoord::from_coord(Coord(snapped.0)).0,
        WCoord::from_coord(Coord(snapped.1)).0,
    );
    let cell = fixture.sim.map.world.w_index(wcoord.0, wcoord.1);
    let head = (
        fixture.sim.map.world.wdata[cell].down,
        fixture.sim.map.world.wdata[cell].down_who,
    );
    let actor_row = fixture.sim.world.row_of(fixture.actor).unwrap();
    let actor = UnitIdentity {
        handle: fixture.actor,
        who: 0,
        o: fixture.actor_o,
        uid: fixture.sim.world.units.get_uid(actor_row),
    };
    let destination = (8_600, 9_400);
    let angle = 0x2345_0000;
    let move_order = queue_last_swarm_move_order(OrderIndex::MoveTo, destination, angle).unwrap();
    let second = GroupBuildPlacementAuthority {
        owner: 0,
        requested: second_action,
        source: PlacementAuthoritySource::BuildTypeValidateSnapAndBlockedSite,
        probes: vec![PlacementProbeAuthority {
            raw: (second_action.x, second_action.y),
            snapped,
            blocked_site: 0,
        }],
        chosen_probe: 0,
        world_link: WorldObjectLinkAuthority {
            wcoord,
            head_before: head,
        },
        city: CityBuildChainAuthority {
            owner: 0,
            city_slot: 0,
            city_before: fixture.sim.cities.slots[0][0].clone(),
            chain: vec![
                CityBuildChainNode {
                    row: 0,
                    o: 2000,
                    city_down: 2001,
                },
                CityBuildChainNode {
                    row: 1,
                    o: 2001,
                    city_down: -1,
                },
            ],
        },
        build_source: BuildBodyAuthoritySource::ObjectsInitBuildPeAfterImage,
        initialized_build: blank_build(FARM_TYPE, 78),
        builders: vec![BuilderSwarmAuthority {
            actor,
            first_nearby_spot: destination,
            second_nearby_spot: None,
            final_destination: destination,
            facing_angle: angle,
            move_order,
            build_order: queue_last_build_order(0, 2002, 78),
            unit_masks_after: BUILD_AT_UNIT_MASK,
            orders_x_after: destination.0,
            orders_y_after: destination.1,
            dest_angle_after: angle,
        }],
    };
    fixture.authority.placements.push(second);
    let saved_after_first = save_sim(&fixture.sim).unwrap();
    let group_authority = fixture.sim.group_move_authority.clone();
    let runtime_authority = fixture.authority.clone();
    let mut resumed = load_sim(&saved_after_first).unwrap();
    resumed.replace_group_move_authority(group_authority);
    let cached = packet(0, &[], second_action);
    let receipt =
        process_group_build_package(&mut fixture.sim, &runtime_authority, 0, 17, &cached).unwrap();
    let resumed_receipt =
        process_group_build_package(&mut resumed, &runtime_authority, 0, 17, &cached).unwrap();
    assert_eq!(receipt.selected[0].o, fixture.actor_o);
    assert_eq!(receipt.build.object_id, 2002);
    assert_eq!(receipt.groups_checksum, resumed_receipt.groups_checksum);
    assert_eq!(fixture.sim.builds[0].city_down, 2001);
    assert_eq!(fixture.sim.builds[1].city_down, 2002);
    assert_eq!(fixture.sim.builds[2].city_down, -1);
    assert_eq!(fixture.sim.world.orders(actor_row).iter().count(), 4);
    assert_eq!(save_sim(&fixture.sim).unwrap(), save_sim(&resumed).unwrap());
}

#[test]
fn decoder_and_pe_entrypoints_are_pinned() {
    let action = QueueUpBuildWire {
        x: 1,
        y: 2,
        x2: 3,
        y2: 4,
        type_index: FARM_TYPE,
        queued: QUEUE_LAST,
    };
    let bytes = packet(0, &[4], action);
    assert_eq!(
        decode_group_build_package(&bytes).unwrap(),
        GroupBuildWire {
            who: 0,
            objects: vec![4],
            action,
        }
    );
    assert_eq!(GROUP_ACTION_BUILD_VA, 0x0070_7510);
    assert_eq!(OBJECTS_INIT_BUILD_VA, 0x0065_d190);
    assert_eq!(BUILD_INIT_VA, 0x0062_9740);
    assert_eq!(OBJECT_ADD_TO_WORLD_VA, 0x0064_d8c0);
    assert_eq!(BUILD_ADD_TO_CITY_VA, 0x0062_2380);
    assert_eq!(GROUP_ACTION_SWARM_AROUND_VA, 0x0070_fbe0);
    assert_eq!(UNIT_ADD_MOVE_FACING_ORDER_VA, 0x005e_55c0);
    assert_eq!(UNIT_ADD_BUILD_ORDER_VA, 0x005e_5210);
}
