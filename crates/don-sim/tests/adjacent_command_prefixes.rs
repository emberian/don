#[path = "../src/systems/adjacent_command_prefixes.rs"]
mod adjacent_command_prefixes;

use adjacent_command_prefixes::*;

fn leader_wire() -> [u8; LEADER_OPTIONS_WIRE_BYTES] {
    let mut wire = [0u8; LEADER_OPTIONS_WIRE_BYTES];
    wire[0] = LEADER_OPTIONS_OPCODE;
    for (offset, value) in [
        (1, 2i32),
        (5, 7),
        (9, -8),
        (13, 9),
        (17, 32),
        (21, 4),
        (25, 42),
    ] {
        wire[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    wire[29..33].copy_from_slice(&[0x1a, 0xbb, 0xcc, 0xdd]);
    wire
}

fn previous_leader(who: i32) -> LeaderOptionRowReceipt {
    LeaderOptionRowReceipt {
        who,
        state: LeaderOptionDataState {
            who,
            peasants: 1,
            peasants_wait: 3,
            buildings: 4,
            flags: BitMask32State {
                bits: 32,
                size: 2,
                flags: 10,
                inline: [0x02, 0x11, 0x22, 0x33],
            },
        },
    }
}

#[test]
fn leader_options_decoder_and_bitmask_assignment_pin_all_33_bytes() {
    let wire = leader_wire();
    let command = decode_leader_options(&wire).unwrap();
    assert_eq!(command.data.who, 2);
    assert_eq!(command.data.peasants, 7);
    assert_eq!(command.data.peasants_wait, -8);
    assert_eq!(command.data.buildings, 9);
    assert_eq!(command.data.flags.bits, 32);
    assert_eq!(command.data.flags.size, 4);
    assert_eq!(command.data.flags.flags, 42);
    assert_eq!(command.data.flags.inline, [0x1a, 0xbb, 0xcc, 0xdd]);

    let plan = plan_leader_options_prefix(&command, previous_leader(2), -1).unwrap();
    assert_eq!(plan.stored.flags.inline, [0x1a, 0xbb, 0xcc, 0xdd]);
    assert_eq!(plan.stored.peasants, 7);
    assert_eq!(plan.stored.peasants_wait, -8);
    assert_eq!(plan.stored.buildings, 9);
    let Some(AdjacentOpenTailRequest::LeaderOptionsCascade(tail)) = plan.open_tail else {
        panic!("changed state must expose the open cascade");
    };
    assert!(tail.changes.peasants_changed);
    assert!(tail.changes.buildings_changed);
    assert!(!tail.changes.flag_bit_1_changed);
    assert!(tail.changes.flag_bit_3_changed);
    assert!(tail.changes.flag_bit_4_changed);
}

#[test]
fn short_bitmask_copy_preserves_uncopied_destination_bytes() {
    let mut wire = leader_wire();
    wire[5..9].copy_from_slice(&1i32.to_le_bytes());
    wire[13..17].copy_from_slice(&4i32.to_le_bytes());
    wire[21..25].copy_from_slice(&1i32.to_le_bytes());
    wire[29..33].copy_from_slice(&[0x02, 0xaa, 0xbb, 0xcc]);
    let command = decode_leader_options(&wire).unwrap();
    let plan = plan_leader_options_prefix(&command, previous_leader(2), -1).unwrap();
    assert_eq!(plan.stored.flags.inline, [0x02, 0x11, 0x22, 0x33]);
    assert!(
        plan.open_tail.is_none(),
        "no observed cascade column changed"
    );
}

#[test]
fn malformed_owner_and_bitmask_overread_fail_closed() {
    let mut wire = leader_wire();
    wire[1..5].copy_from_slice(&10i32.to_le_bytes());
    let command = decode_leader_options(&wire).unwrap();
    assert_eq!(
        plan_leader_options_prefix(&command, previous_leader(10), 0),
        Err(LeaderOptionsPrefixError::UnsafeOwner { who: 10 })
    );

    wire[1..5].copy_from_slice(&2i32.to_le_bytes());
    wire[21..25].copy_from_slice(&5i32.to_le_bytes());
    let command = decode_leader_options(&wire).unwrap();
    assert_eq!(
        plan_leader_options_prefix(&command, previous_leader(2), 0),
        Err(LeaderOptionsPrefixError::UnsafeBitMaskSize { size: 5 })
    );
}

#[test]
fn console_null_gate_controls_coordinate_store_and_parse_handoff() {
    let mut wire = [0u8; CONSOLE_COMMAND_WIRE_BYTES];
    wire[0] = CONSOLE_COMMAND_OPCODE;
    wire[1..5].copy_from_slice(&(-100i32).to_le_bytes());
    wire[5..9].copy_from_slice(&200i32.to_le_bytes());
    for index in 0..CONSOLE_COMMAND_UNITS {
        wire[9 + index * 2..11 + index * 2].copy_from_slice(&(0x4000 + index as u16).to_le_bytes());
    }
    let command = decode_console_command(&wire).unwrap();
    assert_eq!(command.mouse_x, -100);
    assert_eq!(command.mouse_y, 200);
    assert_eq!(command.command[255], 0x40ff);

    let absent = plan_console_command_prefix(&command, false);
    assert_eq!(absent.presentation.len(), 1);
    assert!(absent.open_tail.is_none());

    let present = plan_console_command_prefix(&command, true);
    assert_eq!(present.presentation.len(), 2);
    assert_eq!(
        present.open_tail,
        Some(AdjacentOpenTailRequest::ParseConsoleCommand {
            mouse_x: -100,
            mouse_y: 200,
            command: command.command,
            first_mode: 1,
            second_mode: 1,
        })
    );
}

#[test]
fn each_decoder_rejects_wrong_lengths_and_neighbor_opcodes() {
    assert_eq!(
        decode_console_command(&[CONSOLE_COMMAND_OPCODE]),
        Err(AdjacentCommandDecodeError::WrongLength {
            expected: CONSOLE_COMMAND_WIRE_BYTES,
            actual: 1,
        })
    );
    let mut wrong_opcode = [0u8; LEADER_OPTIONS_WIRE_BYTES];
    wrong_opcode[0] = 72;
    assert_eq!(
        decode_leader_options(&wrong_opcode),
        Err(AdjacentCommandDecodeError::WrongOpcode {
            expected: LEADER_OPTIONS_OPCODE,
            actual: 72,
        })
    );
    assert!(ADJACENT_ROW_CLOSURE
        .iter()
        .all(|row| !row.dispatcher_complete && !row.whole_row_complete));
}
