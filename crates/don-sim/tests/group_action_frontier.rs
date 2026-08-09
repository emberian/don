//! Executable pins for the six-row group-action frontier.

use don_sim::command::group_action_frontier::{
    decode_open_group_action, plan_open_group_action, plan_stop_spell, OpenGroupActionCommand,
    StopSpellMemberFacts, StopSpellStep,
};
use don_sim::command::{Bridge, ObjectTable, Package, Port, Slot, GROUP_ACTIONS};
use don_sim::order::OrderIndex;
use don_sim::systems::groups_guys::GroupData;
use don_sim::systems::order_dispatch::OrderRec;

fn selection(who: u8, members: &[i16]) -> Vec<u8> {
    let mut wire = vec![0, members.len() as u8, who];
    for member in members {
        wire.extend_from_slice(&member.to_le_bytes());
    }
    wire
}

fn i32_wire(opcode: u8, fields: &[i32]) -> Vec<u8> {
    let mut wire = vec![opcode];
    for field in fields {
        wire.extend_from_slice(&field.to_le_bytes());
    }
    wire
}

#[test]
fn all_six_wire_rows_decode_without_aliasing_neighbors() {
    assert_eq!(
        decode_open_group_action(&[13]),
        Some(OpenGroupActionCommand::Transport)
    );
    assert_eq!(
        decode_open_group_action(&i32_wire(18, &[0x1a1, 99])),
        Some(OpenGroupActionCommand::CityGather {
            wanted_type: 0x1a1,
            queued_unread: 99,
        })
    );
    assert_eq!(
        decode_open_group_action(&i32_wire(22, &[10, 20, 3, 1])),
        Some(OpenGroupActionCommand::GatherPoint {
            x: 10,
            y: 20,
            action: 3,
            add_to_end: 1,
        })
    );
    assert_eq!(
        decode_open_group_action(&i32_wire(26, &[1, -1, 7, 2])),
        Some(OpenGroupActionCommand::EjectAll {
            back_to_work: 1,
            who: -1,
            eject_o: 7,
            eject_who: 2,
        })
    );
    assert_eq!(
        decode_open_group_action(&[27]),
        Some(OpenGroupActionCommand::Alarm)
    );
    assert!(decode_open_group_action(&[12]).is_none());
}

#[test]
fn open_rows_commit_only_the_exact_group_prefix() {
    let mut group = GroupData::default();
    group.who = 3;
    group.num = 2;
    group.disband = 9;
    group.form = 4;

    let transport = plan_open_group_action(&group, OpenGroupActionCommand::Transport);
    assert_eq!(transport.group.disband, 0);
    assert_eq!(transport.group.form, -1);

    let eject = plan_open_group_action(
        &group,
        OpenGroupActionCommand::EjectAll {
            back_to_work: 0,
            who: -1,
            eject_o: -1,
            eject_who: -1,
        },
    );
    assert_eq!(eject.group.disband, 0);
    assert_eq!(eject.group.form, -1);

    let gather = plan_open_group_action(
        &group,
        OpenGroupActionCommand::GatherPoint {
            x: 1,
            y: 2,
            action: 0,
            add_to_end: 0,
        },
    );
    assert_eq!(gather.group.disband, 0);
    assert_eq!(gather.group.form, 4);
}

#[test]
fn stop_spell_planner_preserves_member_order_and_special_tail() {
    let mut group = GroupData::default();
    group.who = 2;
    group.num = 2;
    group.list[..2].copy_from_slice(&[4, 7]);
    group.disband = 12;
    let members = [
        StopSpellMemberFacts {
            o: 4,
            valid_unit: true,
            on_map: true,
            current_order: Some(OrderIndex::CastSpell),
            unit_masks: 0x0400_0011,
            type_index: 400,
        },
        StopSpellMemberFacts {
            o: 7,
            valid_unit: true,
            on_map: true,
            current_order: Some(OrderIndex::MoveTo),
            unit_masks: u32::MAX,
            type_index: 61,
        },
    ];

    let plan = plan_stop_spell(&group, &members).unwrap();
    assert_eq!(plan.group.disband, 0);
    assert_eq!(
        plan.steps,
        vec![
            StopSpellStep::SetUnitMasks { o: 4, value: 0x11 },
            StopSpellStep::ClearPathAnchor { o: 4 },
            StopSpellStep::CloseOrders { o: 4, argument: 0 },
            StopSpellStep::ClearPartialPath { o: 4 },
            StopSpellStep::UpdateAction { o: 4 },
            StopSpellStep::ClearSpellWord98 { o: 4 },
            StopSpellStep::SetObjectsFlag22c,
            StopSpellStep::UpdateGpiece,
        ]
    );

    let mut building_group = group;
    building_group.buildings = 1;
    assert!(plan_stop_spell(&building_group, &members)
        .unwrap()
        .steps
        .is_empty());
}

#[test]
fn stop_spell_dispatches_the_complete_object_table_transaction() {
    let mut bridge = Bridge::new();
    bridge.frame = 44;
    let mut package = Package::new(1, 0);
    let mut fleet = ObjectTable::new(4);
    let mut caster = Slot::unit(10, 0, 0);
    caster.type_index = 61;
    caster.unit_masks = 0x0400_0007;
    caster.spell_word_0x98 = 55;
    caster
        .orders
        .push_back(OrderRec::of_kind(OrderIndex::CastSpell));
    fleet.put(1, 0, caster);
    fleet.put(1, 1, Slot::unit(11, 0, 0));
    bridge
        .process_all(&mut package, &selection(1, &[0, 1]), &mut fleet)
        .unwrap();
    bridge.groups.get_mut(package.group).unwrap().disband = 8;

    bridge.process_all(&mut package, &[29], &mut fleet).unwrap();

    let caster = fleet.get(1, 0).unwrap();
    assert_eq!(caster.unit_masks, 7);
    assert_eq!(caster.spell_word_0x98, 0);
    assert!(caster.orders.is_empty());
    assert!(fleet.take_stop_spell_gpiece_update());
    assert_eq!(bridge.groups.get(package.group).unwrap().disband, 0);
}

#[test]
fn closure_classes_freeze_one_green_and_five_state_wired() {
    assert_eq!(
        GROUP_ACTIONS
            .iter()
            .find(|a| a.name == "stop_spell")
            .unwrap()
            .port,
        Port::Complete
    );
    for name in [
        "transport",
        "city_gather",
        "gather_point",
        "eject_all",
        "alarm",
    ] {
        assert_eq!(
            GROUP_ACTIONS.iter().find(|a| a.name == name).unwrap().port,
            Port::StateWired
        );
    }
}
