//! Mutation pins for inline speed, pause, and player-telemetry commands.
//!
//! Each assertion enters through wire bytes and `Bridge::process_all`; private helpers
//! are never called directly.

use don_sim::command::{Bridge, InlineDef, InlinePort, ObjectTable, Package, PLAYER_SPEED_FIELDS};

fn fixed_i32(op: u8, value: i32) -> Vec<u8> {
    let mut bytes = vec![op];
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

fn issue(bridge: &mut Bridge, package: &mut Package, bytes: &[u8]) {
    bridge
        .process_all(package, bytes, &mut ObjectTable::new(0))
        .unwrap();
}

#[test]
fn speed_family_and_player_speed_decode_exact_fields_and_wrap_like_x86() {
    for op in [52, 53, 54, 79] {
        assert_eq!(InlineDef::find(op).unwrap().port, InlinePort::Complete);
    }
    assert_eq!(InlineDef::find(76).unwrap().port, InlinePort::StateWired);

    let mut bridge = Bridge::new();
    let mut package = Package::new(3, 99);
    assert_eq!(bridge.inline.speed, 2);

    issue(&mut bridge, &mut package, &[53]);
    assert_eq!(bridge.inline.speed, 3);
    issue(&mut bridge, &mut package, &[53]);
    assert_eq!(bridge.inline.speed, 3, "wire speed-up caps at Fast");
    issue(&mut bridge, &mut package, &[54]);
    assert_eq!(bridge.inline.speed, 2);

    issue(&mut bridge, &mut package, &fixed_i32(52, 4));
    assert_eq!(bridge.inline.speed, 4, "SpeedSet accepts Hyper Fast");
    bridge.inline.network = true;
    bridge.inline.speed_locked = true;
    issue(&mut bridge, &mut package, &fixed_i32(52, 1));
    issue(&mut bridge, &mut package, &[54]);
    assert_eq!(
        bridge.inline.speed, 4,
        "network speed lock gates set and down"
    );
    bridge.inline.speed_locked = false;
    issue(&mut bridge, &mut package, &fixed_i32(52, 1));
    assert_eq!(bridge.inline.speed, 1);

    bridge.inline.player_speed[3][0] = u32::MAX;
    let mut player_speed = vec![79];
    player_speed.extend(1u8..=PLAYER_SPEED_FIELDS as u8);
    issue(&mut bridge, &mut package, &player_speed);
    assert_eq!(bridge.inline.player_speed[3], [0, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(bridge.stats.inline_state, 8);
    assert_eq!(bridge.stats.inert, 0);
}

#[test]
fn pause_pins_raw_state_comparison_solo_edges_and_network_allowance() {
    let mut bridge = Bridge::new();
    let mut package = Package::new(2, 0);

    issue(&mut bridge, &mut package, &[76, 1]);
    assert!(bridge.inline.paused);
    assert_eq!(bridge.inline.pause_delay, 0);
    issue(&mut bridge, &mut package, &[76, 1]);
    assert!(bridge.inline.paused, "duplicate requested state is inert");
    issue(&mut bridge, &mut package, &[76, 0]);
    assert!(!bridge.inline.paused);
    assert_eq!(bridge.inline.pause_delay, 2);

    bridge.inline.network = true;
    bridge.inline.pauses[2] = 10;
    issue(&mut bridge, &mut package, &[76, 1]);
    assert!(
        !bridge.inline.paused,
        "the eleventh network pause is refused"
    );
    bridge.inline.pause_override = true;
    issue(&mut bridge, &mut package, &[76, 1]);
    assert!(bridge.inline.paused);
    assert_eq!(bridge.inline.pauses[2], 11);

    bridge.inline.immediate_process = true;
    issue(&mut bridge, &mut package, &[76, 0]);
    assert!(
        bridge.inline.paused,
        "immediate-process gate refuses unpause"
    );
    assert_eq!(bridge.stats.inline_state, 6);
}
