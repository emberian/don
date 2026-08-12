// SPDX-License-Identifier: GPL-3.0-or-later
//! Production packet -> fixed Groups/World -> DoNSave v13 -> resumed tick evidence.

use don_sim::command::economy_group_actions::{
    Fact, MemberFacts, CAST_SPELL_ACTIVE_MASK, QUEUE_LAST, QUEUE_NEW,
};
use don_sim::order::{Order, OrderIndex};
use don_sim::systems::canonical_economy_group_host::{
    EconomyActionAuthority, EconomyObjectBinding, EconomyPackageError, EconomyRuntimeAuthority,
};
use don_sim::systems::canonical_group_move_host::{
    GroupMoveAuthority, MoveMemberAuthority, GROUP_OPCODE,
};
use don_sim::systems::canonical_trade_route_runtime::{
    TradeRouteActorAuthority, TradeRouteRuntimeAuthority,
};
use don_sim::systems::economy_containment_group_host::{
    CanonicalMemberFacts, CanonicalObjectIdentity, EconomyGroupAction, EconomyGroupActionFacts,
    EconomyGroupPreflightError, BOARD_SHIP_OPCODE, REPAIR_OPCODE,
};
use don_sim::systems::economy_order_payload_authority::EconomyOrderPayload;
use don_sim::systems::group_action_trade_frontier as trade;
use don_sim::systems::groups_guys::{CheckSum, FormationMember};
use don_sim::systems::production;
use don_sim::systems::save_load::{load_sim, save_sim};
use don_sim::systems::trade_order_frontier::{
    RoadBuildReceipt, RoadBuildStatus, RouteAdmissionFacts, TradeActorFacts, TradeAtomicSnapshot,
    TradeCaravanFacts, TradeEndpointFacts, TradeFrameFacts, TradeIdentity, TradeOrderState,
    TradeRecoveryFacts,
};
use don_sim::tick::lifecycle_host::PlayerTable;
use don_sim::world::WorldObjectIdentity;
use don_sim::Handle;

fn board_package(passenger_o: i16, ship_o: i16) -> Vec<u8> {
    let mut bytes = vec![GROUP_OPCODE, 1, 2];
    bytes.extend_from_slice(&passenger_o.to_le_bytes());
    bytes.push(BOARD_SHIP_OPCODE);
    bytes.extend_from_slice(&i32::from(ship_o).to_le_bytes());
    bytes.extend_from_slice(&QUEUE_NEW.to_le_bytes());
    bytes
}

fn cached_board_package(ship_o: i16) -> Vec<u8> {
    let mut bytes = vec![GROUP_OPCODE, 0, 2];
    bytes.push(BOARD_SHIP_OPCODE);
    bytes.extend_from_slice(&i32::from(ship_o).to_le_bytes());
    bytes.extend_from_slice(&QUEUE_NEW.to_le_bytes());
    bytes
}

fn repair_package(repairer_o: i16, target_o: i16) -> Vec<u8> {
    let mut bytes = vec![GROUP_OPCODE, 1, 2];
    bytes.extend_from_slice(&repairer_o.to_le_bytes());
    bytes.push(REPAIR_OPCODE);
    bytes.extend_from_slice(&i32::from(target_o).to_le_bytes());
    bytes.extend_from_slice(&2i32.to_le_bytes());
    bytes.extend_from_slice(&QUEUE_LAST.to_le_bytes());
    bytes
}

fn trade_package(actor_o: i16, first_o: i16, second_o: i16, queued: i32) -> Vec<u8> {
    let mut bytes = vec![GROUP_OPCODE, 1, 2];
    bytes.extend_from_slice(&actor_o.to_le_bytes());
    bytes.push(don_sim::systems::economy_containment_group_host::TRADE_OPCODE);
    bytes.extend_from_slice(&i32::from(first_o).to_le_bytes());
    bytes.extend_from_slice(&3i32.to_le_bytes());
    bytes.extend_from_slice(&i32::from(second_o).to_le_bytes());
    bytes.extend_from_slice(&4i32.to_le_bytes());
    bytes.extend_from_slice(&queued.to_le_bytes());
    bytes
}

fn selection_authority(passenger: Handle) -> GroupMoveAuthority {
    GroupMoveAuthority {
        revision: 17,
        composition_digest: [0x31; 32],
        destination_is_water: false,
        force_formation_facing_zero: false,
        members: vec![MoveMemberAuthority {
            handle: passenger,
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

fn canonical_identity(sim: &don_sim::tick::Sim, handle: Handle) -> CanonicalObjectIdentity {
    let row = sim.world.row_of(handle).unwrap();
    CanonicalObjectIdentity {
        owner: sim.world.units.get_who(row),
        o: sim.world.units.o()[row],
        uid: sim.world.units.get_uid(row),
        generation: handle.generation,
    }
}

fn economy_authority(
    sim: &don_sim::tick::Sim,
    passenger: Handle,
    ship: Handle,
) -> EconomyRuntimeAuthority {
    let passenger_id = canonical_identity(sim, passenger);
    let ship_id = canonical_identity(sim, ship);
    let passenger_row = sim.world.row_of(passenger).unwrap();
    let action = EconomyGroupAction::BoardShip {
        ship_o: i32::from(ship_id.o),
        queued: QUEUE_NEW,
    };
    EconomyRuntimeAuthority {
        revision: 23,
        composition_digest: [0xa9; 32],
        ignore_orders: false,
        bindings: vec![
            EconomyObjectBinding {
                identity: passenger_id,
                stable: WorldObjectIdentity::Unit {
                    id: passenger.id,
                    generation: passenger.generation,
                },
            },
            EconomyObjectBinding {
                identity: ship_id,
                stable: WorldObjectIdentity::Unit {
                    id: ship.id,
                    generation: ship.generation,
                },
            },
        ],
        actions: vec![EconomyActionAuthority {
            action,
            facts: EconomyGroupActionFacts::BoardShip {
                ship: Some(ship_id),
                members: vec![CanonicalMemberFacts {
                    identity: passenger_id,
                    retail: MemberFacts {
                        o: passenger_id.o,
                        live_unit: true,
                        on_map: true,
                        type_class: Fact::known(0),
                        domain: Fact::known(0),
                        busy: Fact::known(false),
                        regions_touch: Fact::known(true),
                        repair_spell_castable: Fact::known(false),
                        is_caravan: Fact::known(false),
                        is_sea_trade_member: Fact::known(false),
                        ship_can_carry: Fact::known(true),
                        x: sim.world.units.x_internal()[passenger_row],
                        y: sim.world.units.y_internal()[passenger_row],
                    },
                }],
            },
            trade: None,
        }],
    }
}

fn fixture() -> (don_sim::tick::Sim, Handle, Handle, Vec<u8>) {
    let mut sim = don_sim::tick::Sim::new(0x51a7, 4);
    let mut players = PlayerTable::new();
    players.seat(0, 1, 2, 0);
    sim.players = Some(players);
    let passenger = sim.spawn_unit(2, 17, 2_500, 3_500, 4).unwrap();
    let ship = sim.spawn_unit(2, 19, 2_700, 3_600, 5).unwrap();
    for handle in [passenger, ship] {
        let row = sim.world.row_of(handle).unwrap();
        sim.world.units.group_mut()[row] = -1;
        sim.world.units.o_down_mut()[row] = -1;
        sim.world.units.form_mut()[row] = 0;
        sim.world.units.form_mod_mut()[row] = 50;
    }
    let passenger_o = sim.world.units.o()[sim.world.row_of(passenger).unwrap()];
    let ship_o = sim.world.units.o()[sim.world.row_of(ship).unwrap()];
    let packet = board_package(passenger_o, ship_o);
    sim.replace_group_move_authority(selection_authority(passenger));
    sim.replace_economy_group_authority(economy_authority(&sim, passenger, ship));
    (sim, passenger, ship, packet)
}

fn repair_fixture() -> (don_sim::tick::Sim, Handle, Handle, Vec<u8>) {
    let mut sim = don_sim::tick::Sim::new(0x6e91, 4);
    let mut players = PlayerTable::new();
    players.seat(0, 1, 2, 0);
    sim.players = Some(players);
    let repairer = sim.spawn_unit(2, 23, 4_100, 5_100, 6).unwrap();
    let target = sim.spawn_unit(2, 29, 4_300, 5_200, 7).unwrap();
    for handle in [repairer, target] {
        let row = sim.world.row_of(handle).unwrap();
        sim.world.units.group_mut()[row] = -1;
        sim.world.units.o_down_mut()[row] = -1;
        sim.world.units.form_mut()[row] = 0;
        sim.world.units.form_mod_mut()[row] = 50;
    }
    let repairer_id = canonical_identity(&sim, repairer);
    let target_id = canonical_identity(&sim, target);
    let repairer_row = sim.world.row_of(repairer).unwrap();
    let target_row = sim.world.row_of(target).unwrap();
    sim.world.orders_mut(repairer_row).replace(Order {
        kind: OrderIndex::Think,
        flags: 0x2d,
        ..Order::default()
    });
    sim.world
        .units
        .set_unit_masks(target_row, CAST_SPELL_ACTIVE_MASK | 0x200);
    sim.world.orders_mut(target_row).replace(Order {
        kind: OrderIndex::Think,
        flags: 0x5a,
        ..Order::default()
    });
    let action = EconomyGroupAction::Repair {
        target_o: i32::from(target_id.o),
        target_owner: 2,
        queued: QUEUE_LAST,
    };
    let authority = EconomyRuntimeAuthority {
        revision: 29,
        composition_digest: [0xbc; 32],
        ignore_orders: false,
        bindings: vec![
            EconomyObjectBinding {
                identity: repairer_id,
                stable: WorldObjectIdentity::Unit {
                    id: repairer.id,
                    generation: repairer.generation,
                },
            },
            EconomyObjectBinding {
                identity: target_id,
                stable: WorldObjectIdentity::Unit {
                    id: target.id,
                    generation: target.generation,
                },
            },
        ],
        actions: vec![EconomyActionAuthority {
            action,
            facts: EconomyGroupActionFacts::Repair {
                target: Some(target_id),
                members: vec![CanonicalMemberFacts {
                    identity: repairer_id,
                    retail: MemberFacts {
                        o: repairer_id.o,
                        live_unit: true,
                        on_map: true,
                        type_class: Fact::known(0x32),
                        domain: Fact::known(0),
                        busy: Fact::known(false),
                        regions_touch: Fact::known(true),
                        repair_spell_castable: Fact::known(true),
                        is_caravan: Fact::known(false),
                        is_sea_trade_member: Fact::known(false),
                        ship_can_carry: Fact::known(false),
                        x: sim.world.units.x_internal()[repairer_row],
                        y: sim.world.units.y_internal()[repairer_row],
                    },
                }],
            },
            trade: None,
        }],
    };
    let packet = repair_package(repairer_id.o, target_id.o);
    sim.replace_group_move_authority(selection_authority(repairer));
    sim.replace_economy_group_authority(authority);
    (sim, repairer, target, packet)
}

fn savable_trade_build(uid: u16) -> production::BuildData {
    let mut build = production::BuildData {
        flags: production::flag::VALID | production::flag::STARTED,
        uid,
        gather_down: -1,
        city: -1,
        city_down: -1,
        wonder: -1,
        dock: -1,
        attack_ox: -1,
        attack_whom: -1,
        ..production::BuildData::default()
    };
    build.other[0x28..0x2a].copy_from_slice(&(-1i16).to_le_bytes());
    build
}

fn trade_region(
    object: trade::ObjectIdentity,
    tregion: i32,
    digest: u64,
) -> trade::TerrainRegionReceipt {
    trade::TerrainRegionReceipt {
        object,
        x: object.key.o * 10,
        y: object.key.o * 10 + 1,
        tile_x: object.key.o,
        tile_y: object.key.o + 1,
        tregion,
        world_before_digest: digest,
        world_after_digest: digest,
        complete: true,
    }
}

fn trade_fixture(queued: i32) -> (don_sim::tick::Sim, Handle, Vec<u8>) {
    let mut sim = don_sim::tick::Sim::new(0x7701, 4);
    let mut players = PlayerTable::new();
    players.seat(0, 1, 2, 0);
    sim.players = Some(players);
    let actor = sim.spawn_unit(2, 31, 6_100, 7_100, 8).unwrap();
    let actor_row = sim.world.row_of(actor).unwrap();
    sim.world.units.group_mut()[actor_row] = -1;
    sim.world.units.o_down_mut()[actor_row] = -1;
    sim.world.units.form_mut()[actor_row] = 0;
    sim.world.units.form_mod_mut()[actor_row] = 50;
    sim.world.units.set_unit_masks(
        actor_row,
        trade::UNIT_ACTIVE_ORDER_MASK
            | trade::UNIT_HAS_TRADE_ROUTE_MASK
            | trade::UNIT_TRANSPORT_ROUTE_MASK,
    );
    if queued == QUEUE_LAST {
        sim.world.orders_mut(actor_row).replace(Order {
            kind: OrderIndex::Think,
            flags: 0x1d,
            ..Order::default()
        });
    }

    let first_row = sim.spawn_build(3, savable_trade_build(0x3301));
    let second_row = sim.spawn_build(4, savable_trade_build(0x4401));
    let build_o = don_sim::objects::BUILD_BAND_BASE as i16;
    for &row in &[first_row, second_row] {
        sim.builds[row].other[production::off::OBJECT_ID..production::off::OBJECT_ID + 2]
            .copy_from_slice(&build_o.to_le_bytes());
    }

    let actor_id = canonical_identity(&sim, actor);
    let first_id = CanonicalObjectIdentity {
        owner: 3,
        o: build_o,
        uid: sim.builds[first_row].uid,
        generation: first_row as u32,
    };
    let second_id = CanonicalObjectIdentity {
        owner: 4,
        o: build_o,
        uid: sim.builds[second_row].uid,
        generation: second_row as u32,
    };
    let action = EconomyGroupAction::Trade {
        first_o: i32::from(first_id.o),
        first_owner: 3,
        second_o: i32::from(second_id.o),
        second_owner: 4,
        queued,
    };
    let actor_frontier = trade::ObjectIdentity {
        key: trade::ObjectKey {
            o: i32::from(actor_id.o),
            who: 2,
        },
        uid: actor_id.uid,
    };
    let first_frontier = trade::ObjectIdentity {
        key: trade::ObjectKey {
            o: i32::from(first_id.o),
            who: 3,
        },
        uid: first_id.uid,
    };
    let second_frontier = trade::ObjectIdentity {
        key: trade::ObjectKey {
            o: i32::from(second_id.o),
            who: 4,
        },
        uid: second_id.uid,
    };
    let selected_group = trade::TradeGroupSnapshot {
        group_slot: 2 * 64 + 1,
        id: 2 * 64 + 1,
        owner: 2,
        num: 1,
        form: -1,
        disband: 0,
        members: vec![actor_id.o],
    };
    let trade_facts = trade::GroupActionTradeFacts {
        evidence: trade::RetailTradeEvidence::SHIPPED,
        invocation: trade::TradeInvocationFacts {
            scenario: trade::ScenarioPruneFacts {
                ignore_orders: false,
                ignored_objects: Vec::new(),
                calls: Vec::new(),
                group_after: selected_group,
                state_before_digest: 0x10,
                state_after_digest: 0x11,
                complete: true,
            },
            target: Some(trade::TradeTargetGateFacts {
                identity: first_frontier,
                live_building: true,
                active: Some(true),
                entry_receiver_is_trade: Some(true),
                entry_receiver_digest: Some(0x12),
            }),
            count_land: Some(1),
            count_sea: None,
            direct: Some(trade::DirectTradeFacts {
                target_region: trade_region(first_frontier, 9, 0x20),
                members: vec![trade::TradeMemberFacts {
                    identity: actor_frontier,
                    live_unit: true,
                    on_map: Some(true),
                    region: Some(trade_region(actor_frontier, 5, 0x21)),
                    target_is_trade: Some(true),
                    is_caravan: Some(true),
                    target_is_sea_trade: None,
                    is_sea_trade_member: None,
                    regions_touch: None,
                    install: Some(trade::TradeInstallFacts {
                        actor: actor_frontier,
                        unit_flags_before: sim.world.units.get_unit_masks(actor_row),
                        primary_lookup: Some(first_frontier),
                        secondary_lookup: Some(second_frontier),
                        primary_region: trade_region(first_frontier, 9, 0x22),
                        actor_region: trade_region(actor_frontier, 5, 0x23),
                        leader_flags: Some(0x100),
                        transport_type: Some(3),
                        can_ever_transport: Some(true),
                        orders_before_digest: 0x24,
                        partial_path_before_digest: 0x25,
                        action_before_digest: 0x26,
                    }),
                }],
            }),
            queue_first: None,
        },
    };
    let authority = EconomyRuntimeAuthority {
        revision: 37,
        composition_digest: [0xd2; 32],
        ignore_orders: false,
        bindings: vec![
            EconomyObjectBinding {
                identity: actor_id,
                stable: WorldObjectIdentity::Unit {
                    id: actor.id,
                    generation: actor.generation,
                },
            },
            EconomyObjectBinding {
                identity: first_id,
                stable: WorldObjectIdentity::BuildRow(first_row as u32),
            },
            EconomyObjectBinding {
                identity: second_id,
                stable: WorldObjectIdentity::BuildRow(second_row as u32),
            },
        ],
        actions: vec![EconomyActionAuthority {
            action,
            facts: EconomyGroupActionFacts::Trade {
                first: Some(first_id),
                second: Some(second_id),
                target: don_sim::command::economy_group_actions::TradeTargetFacts {
                    live_building: Fact::known(true),
                    active: Fact::known(true),
                    build_is_trade: Fact::known(true),
                    is_trade: Fact::known(true),
                    is_sea_trade: Fact::known(false),
                    group_has_trader: Fact::known(true),
                },
                members: vec![CanonicalMemberFacts {
                    identity: actor_id,
                    retail: MemberFacts {
                        o: actor_id.o,
                        live_unit: true,
                        on_map: true,
                        type_class: Fact::known(0),
                        domain: Fact::known(0),
                        busy: Fact::known(false),
                        regions_touch: Fact::known(true),
                        repair_spell_castable: Fact::known(false),
                        is_caravan: Fact::known(true),
                        is_sea_trade_member: Fact::known(false),
                        ship_can_carry: Fact::known(false),
                        x: sim.world.units.x_internal()[actor_row],
                        y: sim.world.units.y_internal()[actor_row],
                    },
                }],
            },
            trade: Some(trade_facts),
        }],
    };
    let packet = trade_package(actor_id.o, first_id.o, second_id.o, queued);
    sim.replace_group_move_authority(selection_authority(actor));
    sim.replace_economy_group_authority(authority);
    (sim, actor, packet)
}

fn groups_checksum(sim: &don_sim::tick::Sim) -> u32 {
    let mut checksum = CheckSum::default();
    sim.groups.check_groups(&mut checksum);
    checksum.value
}

fn trade_rejection_authority(
    sim: &don_sim::tick::Sim,
    actor: Handle,
) -> TradeRouteRuntimeAuthority {
    let row = sim.world.row_of(actor).unwrap();
    let order = sim.world.orders(row).current().unwrap();
    assert_eq!(order.kind, OrderIndex::TradeRoute);
    let Some(EconomyOrderPayload::TradeRoute(payload)) = order.economy else {
        panic!("fixture did not install a typed TradeOrder")
    };
    let state = TradeOrderState {
        first: TradeIdentity {
            o: i32::from(order.target_o),
            who: i32::from(order.target_who),
            uid: order.target_uid,
        },
        second: TradeIdentity {
            o: payload.second.o,
            who: payload.second.who,
            uid: payload.second.uid,
        },
        started: payload.started,
        loaded: payload.loaded,
        flags: order.flags,
    };
    let endpoint = |identity: TradeIdentity, city: i32| TradeEndpointFacts {
        identity,
        live: true,
        is_build: true,
        city,
        canonical_o: identity.o,
        city_live: true,
        unloaded_departure_gate: true,
        x: identity.o.wrapping_mul(10),
        y: identity.o.wrapping_mul(10).wrapping_add(1),
        city_x: identity.o.wrapping_mul(10).wrapping_add(2),
        city_y: identity.o.wrapping_mul(10).wrapping_add(3),
        x_size: 2,
        y_size: 3,
        empty_route: true,
    };
    let snapshot = TradeAtomicSnapshot {
        actor_version: 41,
        order_version: 42,
        object_epoch: 43,
        leader_epoch: 44,
        city_epoch: 45,
        caravan_epoch: 46,
        path_epoch: 47,
        terrain_epoch: 48,
        rng_epoch: 49,
        effect_epoch: 50,
        facts: TradeFrameFacts {
            actor: TradeActorFacts {
                identity: TradeIdentity {
                    o: i32::from(sim.world.units.o()[row]),
                    who: i32::from(sim.world.units.get_who(row)),
                    uid: sim.world.units.get_uid(row),
                },
                x: sim.world.units.x_internal()[row],
                y: sim.world.units.y_internal()[row],
                flags: sim.world.units.get_unit_masks(row),
                caravan_slot: 7,
                inside: sim.world.units.get_unit_masks(row) & 1 != 0,
                has_order_after_kill: true,
            },
            order: state,
            caravan: Some(TradeCaravanFacts {
                owner: i32::from(sim.world.units.get_who(row)),
                slot: 7,
                flags: 1,
                making_road: 0,
                route_digest: 0x5152,
            }),
            first: Some(endpoint(state.first, 0)),
            second: Some(endpoint(state.second, 1)),
            candidates: None,
            source_transport_compatible: Some(true),
            transport_compatible: Some(true),
            route_admission: Some(RouteAdmissionFacts {
                // Both endpoints are foreign to the actor, so retail rejects before road or
                // city mutation when prerequisite 0x2AC is absent.
                actor_has_foreign_prereq: false,
                source_empty_route_result: 1,
                destination_empty_route_result: 1,
                actor_distance_to_source: 100,
                actor_distance_to_destination: 200,
                road: RoadBuildReceipt {
                    status: RoadBuildStatus::NoRoad,
                    search_before: 0x61,
                    search_after: 0x61,
                    road_before: 0x62,
                    road_after: 0x62,
                    terrain_epoch_before: 0x63,
                    terrain_epoch_after: 0x63,
                    rng_epoch_before: 0x64,
                    rng_epoch_after: 0x64,
                    draw_count: 0,
                },
            }),
            movement: None,
            recovery: Some(TradeRecoveryFacts {
                local_feedback: false,
                transport_next_city: None,
            }),
        },
    };
    TradeRouteRuntimeAuthority {
        revision: 51,
        composition_digest: [0xe3; 32],
        actors: vec![TradeRouteActorAuthority { actor, snapshot }],
    }
}

#[test]
fn board_packet_save_load_resave_and_resumed_tick_are_deterministic() {
    let (mut direct, passenger, ship, packet) = fixture();
    let receipt = direct
        .process_economy_command_package(0, 91, &packet)
        .unwrap();
    assert_eq!(receipt.opcode, BOARD_SHIP_OPCODE);
    assert_eq!(receipt.installed_orders, 2);
    assert_eq!(receipt.random_state_before, receipt.random_state_after);

    let passenger_row = direct.world.row_of(passenger).unwrap();
    let ship_row = direct.world.row_of(ship).unwrap();
    let passenger_order = direct.world.orders(passenger_row).current().unwrap();
    assert_eq!(passenger_order.kind, OrderIndex::BoardShip);
    assert_eq!(
        passenger_order.economy,
        Some(EconomyOrderPayload::TargetOnly)
    );
    assert_eq!(passenger_order.target_handle, Some(ship));
    let ship_order = direct.world.orders(ship_row).current().unwrap();
    assert_eq!(ship_order.kind, OrderIndex::AwaitBoard);
    assert_eq!(ship_order.economy, Some(EconomyOrderPayload::TargetOnly));
    assert_eq!(ship_order.target_handle, Some(passenger));
    assert_eq!(direct.groups.list[receipt.group_slot].form, -1);
    assert_eq!(
        direct.world.units.group()[passenger_row],
        receipt.group_slot as i16
    );

    let saved = save_sim(&direct).unwrap();
    let mut resumed = load_sim(&saved).unwrap();
    assert!(resumed.group_move_authority.members.is_empty());
    assert!(resumed.economy_group_authority.actions.is_empty());
    assert_eq!(save_sim(&resumed).unwrap(), saved);
    assert_eq!(groups_checksum(&resumed), groups_checksum(&direct));
    assert_eq!(resumed.groups.list, direct.groups.list);
    assert_eq!(
        resumed.world.orders(passenger_row),
        direct.world.orders(passenger_row)
    );
    assert_eq!(
        resumed.world.orders(ship_row),
        direct.world.orders(ship_row)
    );

    resumed.replace_group_move_authority(selection_authority(passenger));
    resumed.replace_economy_group_authority(economy_authority(&resumed, passenger, ship));
    let ship_o = direct.world.units.o()[ship_row];
    let cached_packet = cached_board_package(ship_o);
    let direct_cached = direct
        .process_economy_command_package(0, 92, &cached_packet)
        .unwrap();
    let resumed_cached = resumed
        .process_economy_command_package(0, 92, &cached_packet)
        .unwrap();
    // The play-keyed `(o,uid)` cache is persistent; the transaction revision is explicitly
    // process-local and therefore restarts at zero after load.
    assert_eq!(resumed_cached.command_state_revision, 1);
    assert_eq!(direct_cached.command_state_revision, 2);
    let mut normalized_direct = direct_cached.clone();
    normalized_direct.command_state_revision = resumed_cached.command_state_revision;
    assert_eq!(resumed_cached, normalized_direct);
    assert_eq!(resumed_cached.group_slot, receipt.group_slot);
    assert_eq!(
        resumed.command_package_state.selection(0),
        direct.command_package_state.selection(0)
    );
    assert_eq!(save_sim(&resumed).unwrap(), save_sim(&direct).unwrap());

    let direct_trace = direct.do_frame();
    let resumed_trace = resumed.do_frame();
    assert_eq!(resumed_trace.frame, direct_trace.frame);
    assert_eq!(resumed_trace.steps, direct_trace.steps);
    assert_eq!(resumed_trace.work, direct_trace.work);
    assert_eq!(save_sim(&resumed).unwrap(), save_sim(&direct).unwrap());
}

#[test]
fn refusal_after_detached_selection_is_an_exact_noop() {
    let (mut sim, _, _, packet) = fixture();
    sim.economy_group_authority.ignore_orders = true;
    let before = save_sim(&sim).unwrap();
    let random_before = sim.world.random.state();
    let revision_before = sim.command_package_state.revision();
    let checksum_before = groups_checksum(&sim);

    assert_eq!(
        sim.process_economy_command_package(0, 92, &packet),
        Err(EconomyPackageError::Preflight(
            EconomyGroupPreflightError::ScenarioIgnoreOrdersPrelude
        ))
    );
    assert_eq!(sim.world.random.state(), random_before);
    assert_eq!(sim.command_package_state.revision(), revision_before);
    assert_eq!(sim.command_package_state.selection(0), Some(&[][..]));
    assert_eq!(groups_checksum(&sim), checksum_before);
    assert_eq!(save_sim(&sim).unwrap(), before);
}

#[test]
fn repair_packet_persists_cast_and_target_payloads_across_a_resumed_tick() {
    let (mut direct, repairer, target, packet) = repair_fixture();
    let receipt = direct
        .process_economy_command_package(0, 117, &packet)
        .unwrap();
    assert_eq!(receipt.opcode, REPAIR_OPCODE);
    assert_eq!(receipt.installed_orders, 2);

    let repairer_row = direct.world.row_of(repairer).unwrap();
    let target_row = direct.world.row_of(target).unwrap();
    let orders: Vec<_> = direct
        .world
        .orders(repairer_row)
        .iter()
        .map(|order| (order.kind, order.economy))
        .collect();
    assert_eq!(
        orders,
        vec![
            (OrderIndex::Repair, Some(EconomyOrderPayload::TargetOnly)),
            (
                OrderIndex::CastSpell,
                Some(EconomyOrderPayload::CastSpell(
                    don_sim::systems::economy_order_payload_authority::CastOrderPayload {
                        paid: 0,
                        spell: 0x293,
                    }
                ))
            ),
            (OrderIndex::Think, None),
        ]
    );
    let repair = direct
        .world
        .orders(repairer_row)
        .iter()
        .find(|order| order.kind == OrderIndex::Repair)
        .unwrap();
    assert_eq!(repair.target_handle, Some(target));
    assert!(direct.world.orders(target_row).is_empty());
    assert_eq!(
        direct.world.units.get_unit_masks(target_row) & CAST_SPELL_ACTIVE_MASK,
        0
    );

    let saved = save_sim(&direct).unwrap();
    let mut resumed = load_sim(&saved).unwrap();
    assert_eq!(save_sim(&resumed).unwrap(), saved);
    assert_eq!(
        resumed.world.orders(repairer_row),
        direct.world.orders(repairer_row)
    );
    assert_eq!(
        resumed.world.orders(target_row),
        direct.world.orders(target_row)
    );
    resumed.replace_group_move_authority(selection_authority(repairer));
    resumed.replace_economy_group_authority(direct.economy_group_authority.clone());

    let direct_trace = direct.do_frame();
    let resumed_trace = resumed.do_frame();
    assert_eq!(resumed_trace.frame, direct_trace.frame);
    assert_eq!(resumed_trace.steps, direct_trace.steps);
    assert_eq!(resumed_trace.work, direct_trace.work);
    assert_eq!(save_sim(&resumed).unwrap(), save_sim(&direct).unwrap());
}

#[test]
fn trade_packet_persists_two_banded_endpoints_across_a_resumed_tick() {
    let (mut direct, actor, packet) = trade_fixture(QUEUE_NEW);
    let receipt = direct
        .process_economy_command_package(0, 131, &packet)
        .unwrap();
    assert_eq!(
        receipt.opcode,
        don_sim::systems::economy_containment_group_host::TRADE_OPCODE
    );
    assert_eq!(receipt.installed_orders, 1);

    let actor_row = direct.world.row_of(actor).unwrap();
    let order = direct.world.orders(actor_row).current().unwrap();
    assert_eq!(order.kind, OrderIndex::TradeRoute);
    assert_eq!(order.target_who, 3);
    assert_eq!(order.target_o, don_sim::objects::BUILD_BAND_BASE as i16);
    assert_eq!(order.target_handle, None);
    let Some(EconomyOrderPayload::TradeRoute(payload)) = order.economy else {
        panic!("TRADE_ROUTE did not retain its typed payload")
    };
    assert_eq!(payload.second.who, 4);
    assert_eq!(payload.second.o, don_sim::objects::BUILD_BAND_BASE as i32);
    assert_eq!(payload.second.handle, None);
    assert_eq!(direct.groups.list[receipt.group_slot].form, -1);

    let saved = save_sim(&direct).unwrap();
    let mut resumed = load_sim(&saved).unwrap();
    assert_eq!(save_sim(&resumed).unwrap(), saved);
    assert_eq!(
        resumed.world.orders(actor_row),
        direct.world.orders(actor_row)
    );
    resumed.replace_group_move_authority(selection_authority(actor));
    resumed.replace_economy_group_authority(direct.economy_group_authority.clone());

    let direct_trace = direct.do_frame();
    let resumed_trace = resumed.do_frame();
    assert_eq!(resumed_trace.frame, direct_trace.frame);
    assert_eq!(resumed_trace.steps, direct_trace.steps);
    assert_eq!(resumed_trace.work, direct_trace.work);
    assert_eq!(resumed.world.digest(), direct.world.digest());
    assert_eq!(resumed.groups.list, direct.groups.list);
    assert_eq!(
        resumed.world.orders(actor_row),
        direct.world.orders(actor_row)
    );
    assert_eq!(resumed.builds.len(), direct.builds.len());
    for (left, right) in resumed.builds.iter().zip(&direct.builds) {
        assert_eq!(left.image(), right.image());
    }
}

#[test]
fn queued_trade_packet_reaches_exact_rejection_executor_after_save_resume() {
    let (mut direct, actor, packet) = trade_fixture(QUEUE_LAST);
    let package = direct
        .process_economy_command_package(0, 149, &packet)
        .unwrap();
    assert_eq!(package.installed_orders, 1);
    let actor_row = direct.world.row_of(actor).unwrap();
    assert_eq!(direct.world.orders(actor_row).len(), 2);
    assert_eq!(
        direct.world.orders(actor_row).current().unwrap().kind,
        OrderIndex::TradeRoute
    );

    let trade_authority = trade_rejection_authority(&direct, actor);
    direct.replace_trade_route_authority(trade_authority.clone());
    let saved = save_sim(&direct).unwrap();
    let mut resumed = load_sim(&saved).unwrap();
    assert!(resumed.trade_route_authority.actors.is_empty());
    resumed.replace_group_move_authority(selection_authority(actor));
    resumed.replace_economy_group_authority(direct.economy_group_authority.clone());
    resumed.replace_trade_route_authority(trade_authority);

    let direct_trace = direct.do_frame();
    let resumed_trace = resumed.do_frame();
    assert_eq!(resumed_trace.steps, direct_trace.steps);
    assert_eq!(resumed_trace.work, direct_trace.work);
    assert_eq!(direct.cover.trade_route_committed, 1);
    assert_eq!(resumed.cover.trade_route_committed, 1);
    assert_eq!(direct.cover.trade_route_refused, 0);
    assert_eq!(resumed.cover.trade_route_refused, 0);
    let direct_receipt = direct.last_trade_route_receipt.as_ref().unwrap();
    let resumed_receipt = resumed.last_trade_route_receipt.as_ref().unwrap();
    assert_eq!(resumed_receipt, direct_receipt);
    assert_eq!(
        direct_receipt.branch,
        don_sim::systems::trade_order_frontier::TradeExecutorBranch::EstablishmentRejected
    );
    assert_eq!(direct_receipt.next_order, OrderIndex::Think);
    assert_eq!(
        direct_receipt.random_state_before,
        direct_receipt.random_state_after
    );
    assert_eq!(direct.world.orders(actor_row).len(), 1);
    assert_eq!(
        direct.world.orders(actor_row).current().unwrap().kind,
        OrderIndex::Think
    );
    assert_eq!(
        resumed.world.orders(actor_row),
        direct.world.orders(actor_row)
    );
    assert_eq!(resumed.world.digest(), direct.world.digest());
    assert_eq!(resumed.groups.list, direct.groups.list);
    assert_eq!(resumed.world.random.state(), direct.world.random.state());
}
