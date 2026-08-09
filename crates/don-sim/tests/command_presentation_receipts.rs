//! Wire and delivery pins for presentation-only command receipts.

use don_sim::command::{
    Bridge, CommandSideEffectReceipt, InlineDef, InlinePort, ObjectTable, Package,
};

fn issue(bridge: &mut Bridge, package: &mut Package, bytes: &[u8]) {
    bridge
        .process_all(package, bytes, &mut ObjectTable::new(0))
        .unwrap();
}

fn ping(x: i32, y: i32) -> Vec<u8> {
    let mut bytes = vec![50];
    bytes.extend_from_slice(&x.to_le_bytes());
    bytes.extend_from_slice(&y.to_le_bytes());
    bytes
}

fn spline(kind: u8, flags: u8, command: u8, points: &[(i32, i32)]) -> Vec<u8> {
    let mut bytes = vec![51, kind, flags, command];
    bytes.extend_from_slice(&(points.len() as u16).to_le_bytes());
    for &(x, y) in points {
        bytes.extend_from_slice(&x.to_le_bytes());
        bytes.extend_from_slice(&y.to_le_bytes());
    }
    bytes
}

fn chat(bits: u32, taunt: i32, taunt_num: i32, text_without_nul: &[u16]) -> Vec<u8> {
    let mut bytes = vec![68];
    bytes.extend_from_slice(&bits.to_le_bytes());
    bytes.extend_from_slice(&taunt.to_le_bytes());
    bytes.extend_from_slice(&taunt_num.to_le_bytes());
    bytes.extend_from_slice(&(text_without_nul.len() as i32).to_le_bytes());
    for &unit in text_without_nul {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes
}

#[test]
fn ping_uses_both_directed_chat_gates_and_reveal_bypasses_them() {
    assert_eq!(InlineDef::find(50).unwrap().port, InlinePort::Complete);
    let mut bridge = Bridge::new();
    let mut package = Package::new(3, 0);
    bridge.inline.player_who[3] = 2;
    for recipient in [0, 1, 3, 4] {
        bridge.inline.leader_valid[recipient] = true;
    }
    bridge.inline.chat_status[2][1] = 1;
    bridge.inline.chat_status[4][2] = 2;

    issue(&mut bridge, &mut package, &ping(-17, i32::MIN));
    assert_eq!(
        bridge.take_command_side_effect_receipts(),
        vec![CommandSideEffectReceipt::Ping {
            package_play: 3,
            sender_who: Some(2),
            x: -17,
            y: i32::MIN,
            delivered_to: vec![0, 3],
        }]
    );

    bridge.inline.chat_filter_bypass = true;
    issue(&mut bridge, &mut package, &ping(9, 11));
    assert_eq!(
        bridge.take_command_side_effect_receipts(),
        vec![CommandSideEffectReceipt::Ping {
            package_play: 3,
            sender_who: Some(2),
            x: 9,
            y: 11,
            delivered_to: vec![0, 1, 3, 4],
        }]
    );
}

#[test]
fn spline_preserves_raw_header_points_and_local_recipient_filter() {
    assert_eq!(InlineDef::find(51).unwrap().port, InlinePort::Complete);
    let mut bridge = Bridge::new();
    let mut package = Package::new(6, 0);
    bridge.inline.player_who[6] = 2;
    bridge.inline.leader_valid[2] = true;
    bridge.inline.leader_valid[0] = true;
    bridge.inline.leader_valid[1] = true;
    bridge.inline.leader_valid[3] = true;
    bridge.inline.display_play = 7;
    bridge.inline.leader_play[0] = 7;
    bridge.inline.leader_play[1] = 6;
    bridge.inline.leader_play[3] = 7;
    bridge.inline.chat_status[3][2] = 2;
    let points = [(i32::MIN, -1), (0x1234_5678, i32::MAX)];

    issue(&mut bridge, &mut package, &spline(0xff, 0x80, 3, &points));
    assert_eq!(
        bridge.take_command_side_effect_receipts(),
        vec![CommandSideEffectReceipt::Spline {
            package_play: 6,
            sender_who: Some(2),
            spline_type: 0xff,
            spline_flags: 0x80,
            spline_cmd: 3,
            points: points.to_vec(),
            delivered_to: vec![0],
        }]
    );
}

#[test]
fn chat_camera_and_diagnostics_preserve_raw_wire_values_in_order() {
    for op in [56, 68, 72, 81] {
        assert_eq!(InlineDef::find(op).unwrap().port, InlinePort::Complete);
    }
    let mut bridge = Bridge::new();
    let mut package = Package::new(4, 0);
    bridge.inline.player_who[4] = 7;
    bridge.inline.local_play = 4;
    let chat = chat(0x8000_0001, -2, i32::MIN, &[0x0041, 0xd800]);
    let mut camera = vec![72, 6];
    camera.extend_from_slice(&i32::MIN.to_le_bytes());
    camera.extend_from_slice(&0x7fff_ffffi32.to_le_bytes());

    issue(&mut bridge, &mut package, &[56, 0x78, 0x56, 0x34, 0x12]);
    issue(&mut bridge, &mut package, &chat);
    issue(&mut bridge, &mut package, &camera);
    issue(&mut bridge, &mut package, &[81, 0xff]);

    assert_eq!(
        bridge.take_command_side_effect_receipts(),
        vec![
            CommandSideEffectReceipt::CheckRandom { seed: 0x1234_5678 },
            CommandSideEffectReceipt::Chat {
                package_play: 4,
                sender_who: Some(7),
                bits: 0x8000_0001,
                taunt: -2,
                taunt_num: i32::MIN,
                text_len: 2,
                utf16_with_nul: vec![0x0041, 0xd800, 0],
            },
            CommandSideEffectReceipt::Camera {
                package_play: 4,
                zoom: 6,
                x: i32::MIN,
                y: i32::MAX,
                local_sender: true,
            },
            CommandSideEffectReceipt::Marwan { start: 0xff },
        ]
    );
    assert_eq!(bridge.stats.inline_state, 4);
    assert_eq!(bridge.stats.inert, 0);
}
