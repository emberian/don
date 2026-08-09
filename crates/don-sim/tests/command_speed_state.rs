//! Mutation pins for inline speed, pause, and player-telemetry commands.
//!
//! Each assertion enters through wire bytes and `Bridge::process_all`; private helpers
//! are never called directly.

use don_sim::command::{
    Bridge, InlineDef, InlinePort, ObjectTable, Package, Slot, PLAYER_SPEED_FIELDS,
};

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

fn hotkey(group: i32, clear: i32, valid: i32, x_bits: u32, y_bits: u32, zoom: i32) -> Vec<u8> {
    let mut bytes = vec![34];
    for word in [group, clear, valid] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.extend_from_slice(&x_bits.to_le_bytes());
    bytes.extend_from_slice(&y_bits.to_le_bytes());
    bytes.extend_from_slice(&zoom.to_le_bytes());
    bytes
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

#[test]
fn hotkey_copy_camera_clear_and_mp_log_toggle_are_exact_inline_state() {
    assert_eq!(InlineDef::find(34).unwrap().port, InlinePort::Complete);
    assert_eq!(InlineDef::find(55).unwrap().port, InlinePort::Complete);

    let mut bridge = Bridge::new();
    bridge.frame = 321;
    let mut package = Package::new(3, 0);
    let mut fleet = ObjectTable::new(4);
    fleet.put(3, 0, Slot::unit(10, 7, 9));
    fleet.put(3, 1, Slot::unit(11, 13, 15));
    let mut selection = vec![0, 2, 3];
    selection.extend_from_slice(&0i16.to_le_bytes());
    selection.extend_from_slice(&1i16.to_le_bytes());
    bridge
        .process_all(&mut package, &selection, &mut fleet)
        .unwrap();
    let current = bridge.groups.get_mut(package.group).unwrap();
    current.ox = 100;
    current.oy = -200;
    current.o_dist = 17;
    current.o_angle = 29;
    current.speed = 41;

    bridge.inline.hotkeys[5].group.disband = 77;
    bridge
        .process_all(
            &mut package,
            &hotkey(5, 0, 1, 0x7fc0_1234, 0x8000_0000, 88),
            &mut fleet,
        )
        .unwrap();
    let saved = &bridge.inline.hotkeys[5];
    assert_eq!(
        saved.group.id, 5,
        "copy_group preserves destination identity"
    );
    assert_eq!(
        saved.group.disband, 77,
        "copy_group preserves unlisted fields"
    );
    assert_eq!((saved.group.who, saved.group.num), (3, 2));
    assert_eq!((saved.group.ox, saved.group.oy), (100, -200));
    assert_eq!((saved.group.o_dist, saved.group.o_angle), (17, 29));
    assert_eq!(saved.group.speed, 41);
    assert_eq!(saved.group.stamp, 321);
    assert_eq!(&saved.group.list[..2], &[0, 1]);
    assert_eq!(saved.camera, None, "selection copy always clears camera");

    issue(
        &mut bridge,
        &mut package,
        &hotkey(5, 1, 1, 0x7fc0_1234, 0x8000_0000, 88),
    );
    let saved = &bridge.inline.hotkeys[5];
    assert_eq!(saved.group.num, 0);
    let camera = saved.camera.unwrap();
    assert_eq!(camera.x_bits, 0x7fc0_1234, "NaN payload bits survive");
    assert_eq!(camera.y_bits, 0x8000_0000, "negative-zero bits survive");
    assert_eq!(camera.zoom, 88);

    issue(&mut bridge, &mut package, &hotkey(5, 1, 0, 1, 2, 3));
    assert_eq!(bridge.inline.hotkeys[5].camera, None);

    bridge.inline.restart_delay = 9;
    issue(&mut bridge, &mut package, &[55]);
    assert!(bridge.inline.mp_log);
    assert_eq!(bridge.inline.restart_delay, 0);
    issue(&mut bridge, &mut package, &[55]);
    assert!(!bridge.inline.mp_log);
    assert_eq!(bridge.inline.restart_delay, 2);
    assert_eq!(bridge.stats.inline_state, 5);
}

#[test]
fn check_random_ai_controls_and_marwan_preserve_exact_simulation_effects() {
    for op in [56, 62, 63, 64, 81] {
        assert_eq!(InlineDef::find(op).unwrap().port, InlinePort::Complete);
    }

    let mut bridge = Bridge::new();
    let mut package = Package::new(4, 0);

    let before = bridge.inline.clone();
    issue(
        &mut bridge,
        &mut package,
        &[56, 0x78, 0x56, 0x34, 0x12, 81, 0xff],
    );
    assert_eq!(
        bridge.inline, before,
        "CheckRandom and Marwan have only diagnostic/presentation effects"
    );

    issue(&mut bridge, &mut package, &[62]);
    assert_eq!(bridge.inline.ai_speed, 2);
    bridge.inline.ai_speed = 10;
    issue(&mut bridge, &mut package, &[62]);
    assert_eq!(bridge.inline.ai_speed, 10, "increase clamps at ten");
    bridge.inline.ai_speed = i32::MAX;
    issue(&mut bridge, &mut package, &[62]);
    assert_eq!(
        bridge.inline.ai_speed,
        i32::MIN,
        "the x86 increment wraps before the signed upper clamp"
    );
    issue(&mut bridge, &mut package, &[63]);
    assert_eq!(bridge.inline.ai_speed, 1);

    bridge.inline.ai_off = -7;
    issue(&mut bridge, &mut package, &[64]);
    assert_eq!(bridge.inline.ai_off, 0, "any non-zero int toggles to zero");
    issue(&mut bridge, &mut package, &[64]);
    assert_eq!(bridge.inline.ai_off, 1);

    bridge.inline.network = true;
    bridge.inline.ai_speed = 7;
    bridge.inline.ai_off = 1;
    issue(&mut bridge, &mut package, &[62, 63, 64]);
    assert_eq!((bridge.inline.ai_speed, bridge.inline.ai_off), (7, 1));
    assert_eq!(bridge.stats.inline_state, 11);
    assert_eq!(bridge.stats.inert, 0);
}

#[test]
fn checksum_chat_status_and_camera_paths_preserve_exact_state_boundaries() {
    for op in [57, 58, 69, 72] {
        assert_eq!(InlineDef::find(op).unwrap().port, InlinePort::Complete);
    }

    let mut bridge = Bridge::new();
    let mut package = Package::new(4, 0);

    let mut all = vec![57];
    for word in 0x1020_3040u32..0x1020_3050 {
        all.extend_from_slice(&word.to_le_bytes());
    }
    issue(&mut bridge, &mut package, &all);
    assert_eq!(
        bridge.inline.checksums[4], 0x1020_304f,
        "CheckSums stores only the sixteenth total word at wire +61"
    );

    let mut next = vec![58, 14];
    next.extend_from_slice(&0xfedc_ba98u32.to_le_bytes());
    issue(&mut bridge, &mut package, &next);
    assert_eq!(bridge.inline.checksums[4], 0xfedc_ba98);

    bridge.inline.checksum_recheck = 1;
    next[1] = 3;
    next[2..].copy_from_slice(&0x1122_3344u32.to_le_bytes());
    issue(&mut bridge, &mut package, &next);
    assert_eq!(
        bridge.inline.checksums[4], 0xfedc_ba98,
        "active checksum recheck suppresses the opcode-58 store"
    );

    let before_camera = bridge.inline.clone();
    let mut camera = vec![72, 6];
    camera.extend_from_slice(&0x7fff_ffffu32.to_le_bytes());
    camera.extend_from_slice(&0x8000_0000u32.to_le_bytes());
    issue(&mut bridge, &mut package, &camera);
    assert_eq!(
        bridge.inline, before_camera,
        "CameraCommand mutates only local presentation state"
    );

    bridge.inline.player_who[4] = 2;
    issue(&mut bridge, &mut package, &[69, 0, 1, 2, 0xff, 7, 6, 5, 4]);
    assert_eq!(
        bridge.inline.chat_status[2],
        [0, 1, 2, 255, 7, 6, 5, 4],
        "ChatSet zero-extends every status byte into the sender's Player::who row"
    );
    assert_eq!(bridge.inline.chat_status[4], [0; 8]);
    assert_eq!(bridge.stats.inline_state, 5);
    assert_eq!(bridge.stats.inert, 0);
}

fn turn_data(ping: u16, average: u16, wait: u16, lag: u16, forced: u16) -> Vec<u8> {
    let mut bytes = vec![74];
    for value in [ping, average, wait, lag, forced] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[test]
fn reveal_map_and_turn_data_decode_exact_unsigned_fields_and_remote_gate() {
    for op in [59, 74] {
        assert_eq!(InlineDef::find(op).unwrap().port, InlinePort::Complete);
    }

    let mut bridge = Bridge::new();
    bridge.inline.local_play = 1;
    bridge.inline.player_who[3] = 6;
    let mut package = Package::new(3, 0);

    issue(
        &mut bridge,
        &mut package,
        &turn_data(0x80f2, 0x1234, 0xabcd, 0x8001, 0xffff),
    );
    assert!(bridge.inline.reveal_map, "remote ping bit 0x80 reveals");
    assert_eq!(bridge.inline.restart_delay, 0);
    assert_eq!(bridge.inline.accum_cheated[6], 1);
    assert_eq!(bridge.inline.turn_data.flags, 1 << 3);
    assert_eq!(bridge.inline.turn_data.last_ping_times[3], 0x80f2);
    assert_eq!(bridge.inline.turn_data.last_average_frame_times[3], 0x1234);
    assert_eq!(bridge.inline.turn_data.last_wait_times[3], 0xabcd);
    assert_eq!(bridge.inline.turn_data.last_lag_times[3], 0x8001);
    assert_eq!(bridge.inline.turn_data.last_forced_loads[3], 0xffff);

    issue(&mut bridge, &mut package, &turn_data(0x0080, 2, 3, 4, 5));
    assert!(
        bridge.inline.reveal_map,
        "already revealed does not retoggle"
    );
    assert_eq!(bridge.inline.accum_cheated[6], 1);

    issue(&mut bridge, &mut package, &fixed_i32(59, 6));
    assert!(!bridge.inline.reveal_map);
    assert_eq!(bridge.inline.restart_delay, 2);
    assert_eq!(bridge.inline.accum_cheated[6], 2);
    issue(&mut bridge, &mut package, &fixed_i32(59, 6));
    assert!(bridge.inline.reveal_map);
    assert_eq!(bridge.inline.restart_delay, 0);
    assert_eq!(bridge.inline.accum_cheated[6], 3);

    package.play = 1;
    bridge.inline.reveal_map = false;
    issue(&mut bridge, &mut package, &turn_data(0x0080, 0, 0, 0, 0));
    assert!(
        !bridge.inline.reveal_map,
        "local sender cannot trigger reveal"
    );
    assert_eq!(bridge.stats.inline_state, 5);
    assert_eq!(bridge.stats.inert, 0);
}
