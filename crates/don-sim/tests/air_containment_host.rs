// SPDX-License-Identifier: GPL-3.0-or-later
//! Opcode 35 driven end to end against the reference `ObjectTable`, plus the local-UI
//! `Group::action_hotkey` receiver.
//!
//! Every test here goes through `Bridge::process_all` / `Bridge::process_one` with the real
//! object table as the `Fleet`. Nothing constructs a receipt by hand: if the recall/return
//! host stops writing state, or writes different state, these fail.

use don_sim::command::air_containment_host::{AirObject, UnmodelledChild};
use don_sim::command::recall_action_frontier::{RecallBoundary, HELICOPTER_TYPE_FLAG};
use don_sim::command::{build, ActionDef, Bridge, ObjectTable, Package, Port, Slot};
use don_sim::order::OrderIndex;
use don_sim::systems::air::AirOrderWalk;
use don_sim::systems::order_dispatch::PatrolPayload;

const RECALL: [u8; 1] = [35];
const AIR: i32 = 2;
const RECALL_MASK: u32 = 0x0400_0000;

fn plane(uid: u16, x: i32, y: i32) -> Slot {
    let mut slot = Slot::plane(uid, x, y);
    slot.domain = AIR;
    slot.unit_masks = RECALL_MASK | 1;
    slot
}

fn helicopter(uid: u16, x: i32, y: i32) -> Slot {
    let mut slot = Slot::unit(uid, x, y);
    slot.domain = AIR;
    slot.unit_flags = HELICOPTER_TYPE_FLAG;
    slot.unit_masks = RECALL_MASK | 1;
    slot
}

fn airbase(uid: u16, x: i32, y: i32) -> Slot {
    let mut slot = Slot::building(uid, x, y);
    slot.build_masks = 0x08;
    slot
}

fn select(bridge: &mut Bridge, package: &mut Package, table: &mut ObjectTable, list: &[i16]) {
    bridge
        .process_all(package, &build::group(2, list), table)
        .unwrap();
    // A sentinel `Group::action_begin` must clear; it also proves nothing published when
    // the transaction fails closed.
    bridge.groups.get_mut(package.group).unwrap().disband = 9;
}

fn strafe_target(table: &ObjectTable, o: i16) -> Option<(i32, i32, u8)> {
    let order = table.get(2, o)?.orders.front()?;
    assert_eq!(order.kind, OrderIndex::Strafe);
    match &order.patrol_payload {
        PatrolPayload::Strafe(strafe) => {
            Some((strafe.target_o, strafe.target_who, strafe.mandatory))
        }
        _ => None,
    }
}

/// The air-domain leader branch: RECALL delegates to RETURN before `action_begin`, and the
/// helicopter route installs the targetless STRAFE described at `0x006FAF9E`.
#[test]
fn opcode_35_runs_the_return_helicopter_route_on_the_real_object_table() {
    let mut table = ObjectTable::new(1);
    table.put(2, 0, helicopter(11, 80, 96));
    table.set_air_object(
        2,
        0,
        AirObject {
            path_length: 17,
            ..AirObject::default()
        },
    );

    let mut bridge = Bridge::new();
    let mut package = Package::new(2, 0);
    select(&mut bridge, &mut package, &mut table, &[0]);
    bridge.process_one(&mut package, &RECALL, &mut table);

    assert_eq!(bridge.stats.acted, 1);
    assert_eq!(bridge.stats.unported, 0);
    assert_eq!(bridge.stats.open_group_action_tails, 0);
    // `action_return`'s own `action_begin` is what clears the sentinel.
    assert_eq!(bridge.groups.get(package.group).unwrap().disband, 0);

    let slot = table.get(2, 0).unwrap();
    assert_eq!(slot.unit_masks, 1, "unit_masks & 0x04000000 must be cleared");
    assert_eq!(strafe_target(&table, 0), Some((-1, -1, 1)));
    assert_eq!(table.air_object(2, 0).unwrap().path_length, 0);
    assert_eq!(
        table.take_air_unmodelled_children(),
        vec![
            UnmodelledChild::ClearPartialPath { who: 2, o: 0 },
            UnmodelledChild::UpdateAction { who: 2, o: 0 },
        ]
    );
}

/// The main body: a selected airbase clears its gather list, then the owner-unit scan
/// recalls the airborne aircraft whose `AirOrder` home is that base and clears the orders
/// of the one sitting inside it.
#[test]
fn opcode_35_recalls_owner_aircraft_and_clears_the_selected_airbase_gather_list() {
    let mut table = ObjectTable::new(3);
    table.put(2, 0, airbase(1, 48, 48));
    table.put(2, 1, plane(2, 400, 400));
    let mut contained = plane(3, 48, 48);
    contained.is_on_map = false;
    table.put(2, 2, contained);

    table.set_air_object(
        2,
        0,
        AirObject {
            valid_wall: true,
            is_build: true,
            airbase_class: true,
            launching: Some(vec![1, 2]),
            gather_points: vec![10, 11],
            ..AirObject::default()
        },
    );
    table.set_air_object(
        2,
        1,
        AirObject {
            air: Some(AirOrderWalk {
                oxx: 0,
                whose: 2,
                cruising_alt: 0x640,
                sharp_turn: 7,
                ..AirOrderWalk::default()
            }),
            home_base: Some((0, 2)),
            path_length: 9,
            ..AirObject::default()
        },
    );
    table.set_air_object(
        2,
        2,
        AirObject {
            inside: Some((0, 2)),
            home_base: Some((0, 2)),
            ..AirObject::default()
        },
    );

    let mut bridge = Bridge::new();
    let mut package = Package::new(2, 0);
    select(&mut bridge, &mut package, &mut table, &[0]);
    bridge.process_one(&mut package, &RECALL, &mut table);

    assert_eq!(bridge.stats.acted, 1);
    assert_eq!(bridge.stats.unported, 0);
    let group = bridge.groups.get(package.group).unwrap();
    assert_eq!(group.disband, 0, "action_begin");
    assert_eq!(group.form, -1, "form = -1 after the clear_gather pass");

    // Build::clear_gather drained the list.
    let base = table.air_object(2, 0).unwrap();
    assert!(base.gather_points.is_empty());
    // Its off-map arm removed the contained aircraft's slot from `launching`.
    assert_eq!(base.launching.as_deref(), Some([1].as_slice()));

    // The airborne aircraft was recalled to its home base.
    let airborne = table.air_object(2, 1).unwrap();
    assert_eq!(airborne.air.unwrap().returning, 1);
    assert_eq!(airborne.air.unwrap().cruising_alt, 0x640);
    assert_eq!(airborne.air.unwrap().sharp_turn, 7);
    assert_eq!(airborne.path_length, 0);
    assert_eq!(table.get(2, 1).unwrap().unit_masks, 1);
    assert_eq!(strafe_target(&table, 1), Some((0, 2, 0)));

    // The contained aircraft only lost its orders: no STRAFE is installed on that arm.
    assert!(table.get(2, 2).unwrap().orders.is_empty());
}

/// The recovered second arm of `Build::clear_gather` `0x006231E7`: gated on
/// `build_masks & 8` *and* the `0x1BF` class, it re-bases every same-owner air unit whose
/// `home_base` is this build. Dropping either gate changes the outcome.
#[test]
fn build_clear_gather_second_arm_is_gated_on_both_build_mask_8_and_the_airbase_class() {
    for (mask, class, expect_rebased) in [
        (0x08u16, true, true),
        (0x00, true, false),
        (0x08, false, false),
    ] {
        let mut table = ObjectTable::new(2);
        let mut base = airbase(1, 48, 48);
        base.build_masks = mask;
        table.put(2, 0, base);
        table.put(2, 1, plane(2, 400, 400));
        table.set_air_object(
            2,
            0,
            AirObject {
                valid_wall: true,
                is_build: true,
                airbase_class: class,
                launching: Some(vec![1]),
                ..AirObject::default()
            },
        );
        // No AirOrder: the owner-unit scan resolves no home pair and skips this aircraft,
        // so any STRAFE it ends up holding came from `Build::clear_gather` alone.
        table.set_air_object(
            2,
            1,
            AirObject {
                home_base: Some((0, 2)),
                ..AirObject::default()
            },
        );

        let mut bridge = Bridge::new();
        let mut package = Package::new(2, 0);
        select(&mut bridge, &mut package, &mut table, &[0]);
        bridge.process_one(&mut package, &RECALL, &mut table);

        assert_eq!(bridge.stats.acted, 1);
        assert_eq!(
            strafe_target(&table, 1).is_some(),
            expect_rebased,
            "mask={mask:#x} class={class}"
        );
        assert_eq!(table.get(2, 1).unwrap().unit_masks, RECALL_MASK | 1);
    }
}

/// Fail-closed atomicity: an owner slot the planner refuses (`OwnerOutOfRange`) leaves the
/// group sentinel, the order queues and the air columns exactly as they were.
#[test]
fn a_refused_plan_publishes_no_group_order_or_air_state() {
    let mut table = ObjectTable::new(1);
    table.put(9, 0, helicopter(11, 80, 96));
    table.set_air_object(
        9,
        0,
        AirObject {
            path_length: 17,
            ..AirObject::default()
        },
    );

    let mut bridge = Bridge::new();
    let mut package = Package::new(9, 0);
    bridge
        .process_all(&mut package, &build::group(9, &[0]), &mut table)
        .unwrap();
    bridge.groups.get_mut(package.group).unwrap().disband = 9;
    let before = table.clone();

    bridge.process_one(&mut package, &RECALL, &mut table);

    assert_eq!(bridge.stats.acted, 0);
    assert_eq!(bridge.stats.unported, 1);
    assert_eq!(bridge.stats.open_group_action_tails, 1);
    assert_eq!(bridge.groups.get(package.group).unwrap().disband, 9);
    assert_eq!(table.get(9, 0).unwrap().unit_masks, RECALL_MASK | 1);
    assert!(table.get(9, 0).unwrap().orders.is_empty());
    assert_eq!(table.air_object(9, 0).unwrap().path_length, 17);
    assert_eq!(
        format!("{:?}", table.get(9, 0)),
        format!("{:?}", before.get(9, 0))
    );
}

/// The published receipt must recompute; `RecallBoundary::MainBody` is the branch a
/// building-led selection takes, and it carries no RETURN plan.
#[test]
fn the_two_recall_boundaries_are_reached_by_the_leaders_domain_alone() {
    for (air_leader, boundary) in [
        (true, RecallBoundary::OpenActionReturn),
        (false, RecallBoundary::MainBody),
    ] {
        let mut table = ObjectTable::new(1);
        let mut leader = plane(4, 64, 64);
        if !air_leader {
            leader.domain = 0;
            leader.is_plane = false;
        }
        table.put(2, 0, leader);

        let mut bridge = Bridge::new();
        let mut package = Package::new(2, 0);
        select(&mut bridge, &mut package, &mut table, &[0]);
        // `CommandPackage::process_group` `0x0094A0C0` runs `Group::clear(-1)` `0x00713E80`
        // over its stack `Group` at `0x0094A0FF`, and `clear` writes `form = -1`
        // (`+0x10`). So a fresh selection already carries `-1` and cannot witness RECALL's
        // store; plant a distinct value so the store is still observable.
        bridge.groups.get_mut(package.group).unwrap().form = 7;
        bridge.process_one(&mut package, &RECALL, &mut table);

        assert_eq!(bridge.stats.acted, 1, "air_leader={air_leader}");
        assert_eq!(bridge.groups.get(package.group).unwrap().disband, 0);
        let group = bridge.groups.get(package.group).unwrap();
        // `form = -1` is written at `0x006FA987`, in RECALL's main body only. RETURN never
        // touches `form`, so the leader's domain alone decides which store happened.
        assert_eq!(
            group.form,
            match boundary {
                RecallBoundary::MainBody => -1,
                _ => 7,
            },
            "air_leader={air_leader}"
        );
    }
}

/// `Group::action_hotkey` `0x006FA7A0`: copy the addressed group into the control-group
/// slot and invalidate its stored camera.
#[test]
fn action_hotkey_copies_the_group_and_clears_the_stored_camera() {
    let mut table = ObjectTable::new(2);
    table.put(2, 0, plane(5, 32, 64));
    table.put(2, 1, plane(6, 96, 128));

    let mut bridge = Bridge::new();
    let mut package = Package::new(2, 0);
    bridge.frame = 4242;
    select(&mut bridge, &mut package, &mut table, &[0, 1]);

    // Park a camera in slot 5 through the wire row, so the `+0x9D8 = 0` store is visible.
    let mut camera = vec![34u8];
    camera.extend_from_slice(&5i32.to_le_bytes());
    camera.extend_from_slice(&1i32.to_le_bytes());
    camera.extend_from_slice(&1i32.to_le_bytes());
    camera.extend_from_slice(&0x7fc0_1234u32.to_le_bytes());
    camera.extend_from_slice(&0x8000_0000u32.to_le_bytes());
    camera.extend_from_slice(&88i32.to_le_bytes());
    bridge.process_one(&mut package, &camera, &mut table);
    assert!(bridge.inline.hotkeys[5].camera.is_some());

    let plan = bridge
        .action_hotkey(package.group, 5)
        .expect("in-range control-group slot");
    assert_eq!(plan.slot, 5);
    assert_eq!(plan.steps.len(), 3);

    let source = bridge.groups.get(package.group).unwrap().clone();
    let saved = &bridge.inline.hotkeys[5];
    assert_eq!(saved.camera, None, "+0x9D8 camera-valid store");
    assert_eq!(saved.group.num, source.num);
    assert_eq!(saved.group.who, source.who);
    assert_eq!(saved.group.list[..2], source.list[..2]);
    assert_eq!(saved.group.stamp, 4242, "copy_group stamps Game::frame");
    // `copy_group` copies neither the id nor `disband`: the destination keeps its own.
    assert_eq!(saved.group.id, 5);
}

#[test]
fn action_hotkey_refuses_an_out_of_range_slot_instead_of_writing_past_the_array() {
    let mut table = ObjectTable::new(1);
    table.put(2, 0, plane(5, 32, 64));
    let mut bridge = Bridge::new();
    let mut package = Package::new(2, 0);
    select(&mut bridge, &mut package, &mut table, &[0]);

    assert!(bridge.action_hotkey(package.group, 162).is_none());
    assert!(bridge.action_hotkey(package.group, -1).is_none());
    assert!(bridge.inline.hotkeys.iter().all(|slot| slot.group.num == 0));
}

/// The closure ledger rows these tests are the evidence for.
#[test]
fn the_four_receivers_report_their_recovered_port() {
    assert_eq!(ActionDef::find("hotkey").unwrap().port, Port::Complete);
    assert_eq!(ActionDef::find("recall").unwrap().port, Port::Complete);
    assert_eq!(ActionDef::find("return").unwrap().port, Port::Complete);
    // Still red: `Group::action_eject_all`'s two arms both bottom out in `Unit::come_out`
    // `0x00617C10`, of which 7,201 of 9,925 bytes are unrecovered, and in the general body
    // of `Object::eject_contents` `0x0064CD20` (only its step-8 slice is recovered).
    assert_eq!(ActionDef::find("eject_all").unwrap().port, Port::StateWired);
}
