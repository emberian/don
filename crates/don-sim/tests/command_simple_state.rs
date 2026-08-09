//! Mutation pins for the simple state-command tranche.
//!
//! These are wire bytes, not calls to private action helpers: every assertion therefore
//! covers `wire_len` → `process_one` field offsets → current-group routing → state effect.

use don_sim::command::{
    ActionDef, Bridge, Fleet, ObjectTable, Package, Port, Receiver, Slot, OPCODES,
};

fn fixed(op: u8, words: &[i32]) -> Vec<u8> {
    let mut bytes = vec![op];
    for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes
}

#[test]
fn begin_transport_unitmask_and_buildmask_decode_and_mutate_their_exact_state() {
    assert_eq!(OPCODES[1].receiver, Receiver::Group);
    assert_eq!(OPCODES[1].action, Some("begin"));
    assert_eq!(ActionDef::find("begin").unwrap().port, Port::Complete);
    for action in ["set_transport", "unitmask", "buildmask"] {
        assert_eq!(ActionDef::find(action).unwrap().port, Port::Complete);
    }

    let mut bridge = Bridge::new();
    let mut package = Package::new(1, 44);
    let mut fleet = ObjectTable::new(8);
    fleet.set_leader_flags(1, 0x200);

    let a = Slot::unit(10, 0, 0);
    let mut b = Slot::unit(11, 0, 0);
    b.unit_masks = 0x20;
    b.can_ever_transport = false;
    let c = Slot::unit(12, 0, 0);
    fleet.put(1, 0, a);
    fleet.put(1, 1, b);
    fleet.put(1, 2, c);

    let mut select_units = vec![0, 3, 1];
    for object in [0i16, 1, 2] {
        select_units.extend_from_slice(&object.to_le_bytes());
    }
    bridge
        .process_all(&mut package, &select_units, &mut fleet)
        .unwrap();
    bridge.groups.get_mut(package.group).unwrap().disband = 91;

    // BeginCommand is exactly one byte. Its indirect vtable call reaches
    // Group::action_begin and clears GroupData::disband.
    bridge.process_all(&mut package, &[1], &mut fleet).unwrap();
    assert_eq!(bridge.groups.get(package.group).unwrap().disband, 0);

    // SetTransportCommand: flag@+1. The owner transport level gates setting 0x800000,
    // a member which fails can_ever_transport remains untouched, and the action's
    // virtual BEGIN prerequisite clears disband before walking members.
    bridge.groups.get_mut(package.group).unwrap().disband = 37;
    bridge
        .process_all(&mut package, &fixed(14, &[1]), &mut fleet)
        .unwrap();
    assert_eq!(bridge.groups.get(package.group).unwrap().disband, 0);
    assert_eq!(fleet.get(1, 0).unwrap().unit_masks, 0x0080_0000);
    assert_eq!(fleet.get(1, 1).unwrap().unit_masks, 0x20);
    assert_eq!(fleet.get(1, 2).unwrap().unit_masks, 0x0080_0000);

    // UnitmaskCommand: mask@+1, ignored set@+5. This mixed starting state pins the
    // retail carry rule: absent -> set, then present -> clear and keep clearing.
    bridge
        .process_all(&mut package, &fixed(32, &[0x20, -777]), &mut fleet)
        .unwrap();
    assert_eq!(fleet.get(1, 0).unwrap().unit_masks & 0x20, 0x20);
    assert_eq!(fleet.get(1, 1).unwrap().unit_masks & 0x20, 0);
    assert_eq!(fleet.get(1, 2).unwrap().unit_masks & 0x20, 0);

    let mut d = Slot::building(20, 0, 0);
    d.build_mask_capabilities = 0x40;
    let mut e = Slot::building(21, 0, 0);
    e.build_masks = 0x40;
    e.build_mask_capabilities = 0x40;
    let mut g = Slot::building(22, 0, 0);
    g.build_mask_capabilities = 0x40;
    fleet.put(1, 3, d);
    fleet.put(1, 4, e);
    fleet.put(1, 5, g);

    let mut select_buildings = vec![0, 3, 1];
    for object in [3i16, 4, 5] {
        select_buildings.extend_from_slice(&object.to_le_bytes());
    }
    bridge
        .process_all(&mut package, &select_buildings, &mut fleet)
        .unwrap();
    bridge
        .process_all(&mut package, &fixed(33, &[0x40, 12345]), &mut fleet)
        .unwrap();
    assert_eq!(fleet.build_masks(1, 3), Some(0x40));
    assert_eq!(fleet.build_masks(1, 4), Some(0));
    assert_eq!(fleet.build_masks(1, 5), Some(0));

    for op in [1usize, 14, 32, 33] {
        assert_eq!(bridge.stats.by_opcode[op], 1, "opcode {op}");
    }
    assert_eq!(bridge.stats.inert, 0);
}
