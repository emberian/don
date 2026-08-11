// SPDX-License-Identifier: GPL-3.0-or-later
//! `Group::action_scramble` `0x007111C0` and `Group::action_launch_patrol` `0x00703580`,
//! driven through the real wire path.
//!
//! Every case goes `Bridge::process_all(build::group(..))` and then opcode 36 or opcode 11,
//! i.e. through `CommandPackage`'s decoders and `Action::run`'s entry program — never
//! through a planner call.
//!
//! The load-bearing claim these tests pin is the one the previous port had wrong: both
//! receivers order the aircraft each selected object **contains**
//! (`ObjectData::inside_down` `+0x28` / `inside_down_who` `+0x3E`), not the selected
//! objects themselves.

use don_sim::command::air_containment_host::AirObject;
use don_sim::command::air_launch_receivers::{
    AirLaunchBoundary, ContainedFacts, LaunchPatrolRequest, LaunchReason, HELICOPTER_TYPE_FLAG,
    LAUNCH_ALL_QUEUE_POS, MISSILE_OBJECT_MASK, TYPE_BIPLANE, TYPE_BOMBER, TYPE_HELICOPTER,
    UNIT_MASK_LAUNCH_CLEAR,
};
use don_sim::command::{
    build, ActionDef, Bridge, Fleet, ObjectTable, Package, Port, QueuePos, Slot,
};
use don_sim::order::{OrderIndex, ORDER_GROUP, ORDER_PATHED};
use don_sim::systems::order_dispatch::PatrolPayload;

const WHO: u8 = 1;

/// An airbase: a selectable object whose containment chain holds `plane`.
fn hangar(fleet: &mut ObjectTable, o: i16, pos: (i32, i32), plane: Option<i16>) {
    let mut slot = Slot::unit(100 + o as u16, pos.0, pos.1);
    slot.follow_inside_down = plane.map(|p| (i32::from(p), i32::from(WHO)));
    slot.object_type_masks = Some(0);
    slot.mana_burn = Some(0);
    slot.type_is = Some(Vec::new());
    fleet.put(WHO, o, slot);
}

/// A launchable aircraft: `is_unit`, `domain == 2`, not busy, no missile mask, `+0x96 == 0`.
///
/// `inside` is `ObjectData::get_inside`'s answer — the hangar it sits in — and is what the
/// receiver hands to `Unit::add_air_patrol_order` as its home.
fn aircraft(fleet: &mut ObjectTable, o: i16, pos: (i32, i32), inside: i16, types: &[i32]) {
    let mut slot = Slot::unit(200 + o as u16, pos.0, pos.1);
    slot.domain = 2;
    slot.busy = Some(false);
    slot.object_type_masks = Some(0);
    slot.mana_burn = Some(0);
    slot.type_is = Some(types.to_vec());
    // Nothing is inside an aircraft: its own `inside_down` chain terminates.
    slot.follow_inside_down = None;
    fleet.put(WHO, o, slot);
    fleet.set_air_object(
        WHO,
        o,
        AirObject {
            inside: Some((i32::from(inside), i32::from(WHO))),
            ..AirObject::default()
        },
    );
}

/// The waypoint an installed `AIR_PATROL` carries, which `Unit::add_air_patrol_order`
/// stores relative to the home object.
fn air_waypoint(fleet: &ObjectTable, o: i16) -> Option<(i32, i32)> {
    let order = fleet.get(WHO, o)?.orders.front()?;
    let PatrolPayload::Air(air) = &order.patrol_payload else {
        return None;
    };
    Some((air.points.x[0], air.points.y[0]))
}

fn select(bridge: &mut Bridge, package: &mut Package, fleet: &mut ObjectTable, list: &[i16]) {
    bridge
        .process_all(package, &build::group(WHO as i8, list), fleet)
        .expect("opcode 0 decodes");
}

fn orders(fleet: &ObjectTable, o: i16) -> Vec<(OrderIndex, i32, i32)> {
    fleet
        .get(WHO, o)
        .expect("slot")
        .orders
        .iter()
        .map(|order| (order.kind, order.x, order.y))
        .collect()
}

#[test]
fn opcode_36_orders_the_contained_aircraft_and_never_the_selected_object() {
    let mut fleet = ObjectTable::new(8);
    hangar(&mut fleet, 0, (4_800, 9_600), Some(3));
    aircraft(&mut fleet, 3, (4_800, 9_600), 0, &[]);

    let mut bridge = Bridge::new();
    let mut package = Package::new(WHO as i32, 0);
    select(&mut bridge, &mut package, &mut fleet, &[0]);
    bridge
        .process_all(&mut package, &build::scramble(), &mut fleet)
        .expect("opcode 36 decodes");

    assert_eq!(
        orders(&fleet, 3).iter().map(|o| o.0).collect::<Vec<_>>(),
        vec![OrderIndex::AirPatrol],
        "the aircraft inside the hangar patrols over the hangar"
    );
    assert_eq!(
        air_waypoint(&fleet, 3),
        Some((0, 0)),
        "the patrol point is the hangar's own Coord, stored relative to home"
    );
    assert!(
        orders(&fleet, 0).is_empty(),
        "the selected hangar itself receives nothing"
    );
    assert_eq!(bridge.stats.orders_installed, 1);
    assert_eq!(bridge.stats.by_order[OrderIndex::AirPatrol.index()], 1);
    assert_eq!(bridge.stats.unported, 0);
}

#[test]
fn a_selection_of_aircraft_sitting_on_the_map_scrambles_nothing() {
    // This is the case the previous port served: it iterated the group's own members and
    // installed on every `is_plane` one. Retail walks `inside_down`, which is empty here.
    let mut fleet = ObjectTable::new(8);
    aircraft(&mut fleet, 0, (4_800, 9_600), -1, &[]);
    aircraft(&mut fleet, 1, (4_800, 9_600), -1, &[]);

    let mut bridge = Bridge::new();
    let mut package = Package::new(WHO as i32, 0);
    select(&mut bridge, &mut package, &mut fleet, &[0, 1]);
    bridge
        .process_all(&mut package, &build::scramble(), &mut fleet)
        .expect("opcode 36 decodes");

    assert!(orders(&fleet, 0).is_empty());
    assert!(orders(&fleet, 1).is_empty());
    assert_eq!(bridge.stats.orders_installed, 0);
    assert_eq!(bridge.stats.unported, 0, "an empty chain is not a refusal");
}

#[test]
fn a_withheld_column_refuses_the_whole_command_instead_of_launching_a_guess() {
    for withhold in 0..3 {
        let mut fleet = ObjectTable::new(8);
        hangar(&mut fleet, 0, (4_800, 9_600), Some(3));
        aircraft(&mut fleet, 3, (4_800, 9_600), 0, &[]);
        let slot = fleet.get_mut(WHO, 3).unwrap();
        match withhold {
            0 => slot.busy = None,
            1 => slot.object_type_masks = None,
            _ => slot.mana_burn = None,
        }

        let mut bridge = Bridge::new();
        let mut package = Package::new(WHO as i32, 0);
        select(&mut bridge, &mut package, &mut fleet, &[0]);
        bridge
            .process_all(&mut package, &build::scramble(), &mut fleet)
            .expect("opcode 36 decodes");

        assert!(orders(&fleet, 3).is_empty(), "case {withhold}");
        assert_eq!(bridge.stats.orders_installed, 0, "case {withhold}");
        assert_eq!(bridge.stats.unported, 1, "case {withhold}");
        assert_eq!(bridge.stats.open_group_action_tails, 1, "case {withhold}");
    }
}

#[test]
fn each_of_the_four_common_predicates_removes_the_aircraft_from_the_launch() {
    let cases: Vec<(&str, Box<dyn Fn(&mut Slot)>)> = vec![
        ("not a unit", Box::new(|s: &mut Slot| s.is_unit = false)),
        ("wrong domain", Box::new(|s: &mut Slot| s.domain = 0)),
        ("busy", Box::new(|s: &mut Slot| s.busy = Some(true))),
        (
            "missile mask",
            Box::new(|s: &mut Slot| s.object_type_masks = Some(MISSILE_OBJECT_MASK)),
        ),
        (
            "mana_burn set",
            Box::new(|s: &mut Slot| s.mana_burn = Some(1)),
        ),
    ];
    for (name, mutate) in cases {
        let mut fleet = ObjectTable::new(8);
        hangar(&mut fleet, 0, (4_800, 9_600), Some(3));
        aircraft(&mut fleet, 3, (4_800, 9_600), 0, &[]);
        mutate(fleet.get_mut(WHO, 3).unwrap());

        let mut bridge = Bridge::new();
        let mut package = Package::new(WHO as i32, 0);
        select(&mut bridge, &mut package, &mut fleet, &[0]);
        bridge
            .process_all(&mut package, &build::scramble(), &mut fleet)
            .expect("opcode 36 decodes");

        assert!(orders(&fleet, 3).is_empty(), "{name}");
        assert_eq!(bridge.stats.unported, 0, "{name} is a skip, not a refusal");
    }
}

#[test]
fn a_helicopter_gets_the_inlined_centred_move_to_instead_of_an_air_patrol() {
    let mut fleet = ObjectTable::new(8);
    hangar(&mut fleet, 0, (4_800, 9_600), Some(3));
    aircraft(&mut fleet, 3, (4_000, 9_000), 0, &[TYPE_HELICOPTER]);
    let slot = fleet.get_mut(WHO, 3).unwrap();
    slot.unit_flags = HELICOPTER_TYPE_FLAG;
    slot.unit_masks = UNIT_MASK_LAUNCH_CLEAR | 1;

    let mut bridge = Bridge::new();
    let mut package = Package::new(WHO as i32, 0);
    select(&mut bridge, &mut package, &mut fleet, &[0]);
    bridge
        .process_all(&mut package, &build::scramble(), &mut fleet)
        .expect("opcode 36 decodes");

    // 4800 >> 4 = 300; div_3_table[300] = 100; 100 * 0x30 + 0x18 = 4824.
    assert_eq!(orders(&fleet, 3), vec![(OrderIndex::MoveTo, 4_824, 9_624)]);
    let installed = fleet.get(WHO, 3).unwrap().orders.front().unwrap().clone();
    assert_eq!((installed.dest_x, installed.dest_y), (4_824, 9_624));
    assert_eq!((installed.last_x, installed.last_y), (-1, -1));
    assert_eq!((installed.orig_x, installed.orig_y), (-1, -1));
    assert_eq!(installed.facing, -1);
    assert_eq!(installed.flags & ORDER_GROUP, ORDER_GROUP);
    assert_eq!(installed.flags & ORDER_PATHED, 0);
    assert_eq!(
        fleet.unit_masks(WHO, 3),
        1,
        "the inline branch clears unit_masks bit 0x04000000 first"
    );
}

fn launch_fleet() -> ObjectTable {
    let mut fleet = ObjectTable::new(8);
    // Two hangars, the second much closer to the patrol point.
    hangar(&mut fleet, 0, (100_000, 0), Some(3));
    hangar(&mut fleet, 1, (1_000, 0), Some(4));
    aircraft(&mut fleet, 3, (100_000, 0), 0, &[TYPE_BOMBER]);
    aircraft(&mut fleet, 4, (1_000, 0), 1, &[TYPE_BIPLANE]);
    fleet
}

#[test]
fn opcode_11_without_launch_all_orders_only_the_cheapest_aircraft() {
    let mut fleet = launch_fleet();
    let mut bridge = Bridge::new();
    let mut package = Package::new(WHO as i32, 0);
    select(&mut bridge, &mut package, &mut fleet, &[0, 1]);
    bridge
        .process_all(
            &mut package,
            &build::launch_patrol_full(0, 0, 0, 0, 0, 0),
            &mut fleet,
        )
        .expect("opcode 11 decodes");

    assert!(orders(&fleet, 3).is_empty(), "the far bomber stays put");
    assert_eq!(
        orders(&fleet, 4).iter().map(|o| o.0).collect::<Vec<_>>(),
        vec![OrderIndex::AirPatrol]
    );
    assert_eq!(air_waypoint(&fleet, 4), Some((-1_000, 0)));
    assert_eq!(bridge.stats.orders_installed, 1);
}

#[test]
fn queue_pos_one_launches_every_candidate_and_homes_each_on_its_own_member() {
    let mut fleet = launch_fleet();
    let mut bridge = Bridge::new();
    let mut package = Package::new(WHO as i32, 0);
    select(&mut bridge, &mut package, &mut fleet, &[0, 1]);
    bridge
        .process_all(
            &mut package,
            &build::launch_patrol_full(500, 600, LAUNCH_ALL_QUEUE_POS, 0, 0, 0),
            &mut fleet,
        )
        .expect("opcode 11 decodes");

    assert_eq!(air_waypoint(&fleet, 3), Some((500 - 100_000, 600)));
    assert_eq!(air_waypoint(&fleet, 4), Some((500 - 1_000, 600)));
    assert_eq!(bridge.stats.orders_installed, 2);
}

#[test]
fn the_type_filters_live_in_the_last_two_wire_dwords_and_fighters_shadows_bombers() {
    // fighters_only at cmd+0x15 keeps only the BIPLANE; bombers_only at cmd+0x11 keeps
    // only the BOMBER; both set behaves as fighters_only alone.
    for (force_all, bombers, fighters, expect) in [
        (0, 0, 1, vec![4i16]),
        (0, 1, 0, vec![3]),
        (0, 1, 1, vec![4]),
        (1, 0, 0, vec![3, 4]),
    ] {
        let mut fleet = launch_fleet();
        let mut bridge = Bridge::new();
        let mut package = Package::new(WHO as i32, 0);
        select(&mut bridge, &mut package, &mut fleet, &[0, 1]);
        bridge
            .process_all(
                &mut package,
                &build::launch_patrol_full(
                    0,
                    0,
                    LAUNCH_ALL_QUEUE_POS,
                    force_all,
                    bombers,
                    fighters,
                ),
                &mut fleet,
            )
            .expect("opcode 11 decodes");

        let launched: Vec<i16> = [3i16, 4]
            .into_iter()
            .filter(|&o| !orders(&fleet, o).is_empty())
            .collect();
        assert_eq!(launched, expect, "({force_all},{bombers},{fighters})");
    }
}

#[test]
fn force_all_bypasses_the_mana_burn_gate_that_otherwise_refuses_every_aircraft() {
    for (force_all, expected) in [(0, 0usize), (1, 2)] {
        let mut fleet = launch_fleet();
        for o in [3i16, 4] {
            fleet.get_mut(WHO, o).unwrap().mana_burn = Some(7);
        }
        let mut bridge = Bridge::new();
        let mut package = Package::new(WHO as i32, 0);
        select(&mut bridge, &mut package, &mut fleet, &[0, 1]);
        bridge
            .process_all(
                &mut package,
                &build::launch_patrol_full(0, 0, LAUNCH_ALL_QUEUE_POS, force_all, 0, 0),
                &mut fleet,
            )
            .expect("opcode 11 decodes");
        assert_eq!(bridge.stats.orders_installed as usize, expected);
    }
}

#[test]
fn the_planner_agrees_with_the_wire_path_on_the_refusal_reason_and_its_string_ordinal() {
    let plan = don_sim::command::air_launch_receivers::plan_launch_patrol(
        &LaunchPatrolRequest::default(),
        &don_sim::command::air_launch_receivers::AirLaunchFacts {
            who: WHO,
            members: vec![don_sim::command::air_launch_receivers::MemberFacts {
                object: (WHO, 0),
                pos: (0, 0),
                contained: vec![ContainedFacts {
                    mana_burn: Some(4),
                    ..ContainedFacts::eligible((WHO, 3), (0, 0))
                }],
                containment_answered: true,
            }],
        },
    )
    .expect("every column answered");
    assert!(plan.installs.is_empty());
    assert_eq!(plan.reason, LaunchReason::MANA_BURN);
    assert_eq!(plan.feedback_ordinal, Some(1_690));
}

#[test]
fn an_unanswered_containment_column_is_a_named_boundary_not_an_empty_launch() {
    let facts = don_sim::command::air_launch_receivers::AirLaunchFacts {
        who: WHO,
        members: vec![don_sim::command::air_launch_receivers::MemberFacts {
            object: (WHO, 0),
            pos: (0, 0),
            contained: Vec::new(),
            containment_answered: false,
        }],
    };
    assert_eq!(
        don_sim::command::air_launch_receivers::plan_scramble(&facts).unwrap_err(),
        AirLaunchBoundary::ContainmentUnanswered { member: (WHO, 0) }
    );
}

#[test]
fn both_rows_report_the_port_this_lane_left_them_at() {
    for name in ["scramble", "launch_patrol"] {
        let row = ActionDef::find(name).expect("row exists");
        assert_eq!(row.port, Port::Orders, "{name}");
        assert!(row.delegates.is_empty(), "{name} delegates to nothing");
    }
    assert_eq!(ActionDef::find("scramble").unwrap().va, 0x0071_11c0);
    assert_eq!(ActionDef::find("launch_patrol").unwrap().va, 0x0070_3580);
}

#[test]
fn a_scramble_does_not_clear_disband_because_action_begin_is_never_called() {
    let mut fleet = ObjectTable::new(8);
    hangar(&mut fleet, 0, (4_800, 9_600), Some(3));
    aircraft(&mut fleet, 3, (4_800, 9_600), 0, &[]);

    let mut bridge = Bridge::new();
    let mut package = Package::new(WHO as i32, 0);
    select(&mut bridge, &mut package, &mut fleet, &[0]);
    bridge.groups.get_mut(package.group).unwrap().disband = 9;
    bridge
        .process_all(&mut package, &build::scramble(), &mut fleet)
        .expect("opcode 36 decodes");

    assert_eq!(
        bridge.groups.get(package.group).unwrap().disband,
        9,
        "vtable +0x14 never appears in the 894-byte body"
    );
}

#[test]
fn the_scenario_ignore_orders_prelude_fails_the_scramble_closed() {
    let mut fleet = ObjectTable::new(8);
    hangar(&mut fleet, 0, (4_800, 9_600), Some(3));
    aircraft(&mut fleet, 3, (4_800, 9_600), 0, &[]);
    fleet.set_scenario_ignore_orders(true, false);

    let mut bridge = Bridge::new();
    let mut package = Package::new(WHO as i32, 0);
    select(&mut bridge, &mut package, &mut fleet, &[0]);
    bridge
        .process_all(&mut package, &build::scramble(), &mut fleet)
        .expect("opcode 36 decodes");

    assert!(orders(&fleet, 3).is_empty());
    assert_eq!(bridge.stats.unported, 1);

    // With the prune committed the receiver runs.
    fleet.set_scenario_ignore_orders(true, true);
    bridge
        .process_all(&mut package, &build::scramble(), &mut fleet)
        .expect("opcode 36 decodes");
    assert_eq!(orders(&fleet, 3).len(), 1);
}

#[test]
fn a_queue_pos_value_other_than_one_never_reaches_the_launch_all_arm() {
    for queue in [QueuePos::New as i32, QueuePos::Last as i32, 3, -1] {
        if queue == LAUNCH_ALL_QUEUE_POS {
            continue;
        }
        let mut fleet = launch_fleet();
        let mut bridge = Bridge::new();
        let mut package = Package::new(WHO as i32, 0);
        select(&mut bridge, &mut package, &mut fleet, &[0, 1]);
        bridge
            .process_all(
                &mut package,
                &build::launch_patrol_full(0, 0, queue, 0, 0, 0),
                &mut fleet,
            )
            .expect("opcode 11 decodes");
        assert_eq!(bridge.stats.orders_installed, 1, "queue = {queue}");
    }
}
