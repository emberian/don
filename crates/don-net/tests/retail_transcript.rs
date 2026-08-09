//! End-to-end acceptance for the owned retail-style transcript. Unlike the
//! standalone driver, this runs the bytes through shared Session membership,
//! readiness, turn storage and checksum decoding, including a reconnect epoch.

use don_net::msg::NetMsg;
use don_net::obfuscate::xor_payload;
use don_net::session::{Event, Role, Session, TurnPackage};
use don_net::transport::TcpTransport;
use don_net::{
    decode_retail_checksum_package, encode_commands, CheckSums, Command, EpochCause, EpochMember,
    LockstepRunner, LockstepStatus, Obfuscation, PersistedLockstepTranscript, ReplayAction,
};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

const HOST_ID: i32 = 1;
const CLIENT_ID: i32 = 2;
const GAME_KEY: u32 = 0x005a_c33d;

// Exact checksum words emitted by the proven Phase-E mock-retail transcript
// for stamps 23, 24 and 25. The final word is the wrapping sum of channels
// 0..14, as retail's CheckSumsCommand requires.
const TRANSCRIPT: [(u32, [u32; 16]); 3] = [
    (
        23,
        [
            0x017c002f, 0x01860032, 0x01900035, 0x019a0038, 0x01a4003b, 0x01ae003e, 0x01b80041,
            0x01c20044, 0x020c0057, 0x0216005a, 0x0220005d, 0x022a0060, 0x02340063, 0x023e0066,
            0x02480069, 0x1c1e046c,
        ],
    ),
    (
        24,
        [
            0x018c0031, 0x019e0036, 0x01b0003b, 0x01c20040, 0x01d40045, 0x01e6004a, 0x01f8004f,
            0x020a0054, 0x01dc0049, 0x01ee004e, 0x02000053, 0x02120058, 0x0224005d, 0x02360062,
            0x02480067, 0x1cd6047c,
        ],
    ),
    (
        25,
        [
            0x019c0033, 0x01a60036, 0x01c0003d, 0x01ca0040, 0x01e40047, 0x01ee004a, 0x02080051,
            0x02120054, 0x01ec004b, 0x01f6004e, 0x02100055, 0x021a0058, 0x0234005f, 0x023e0062,
            0x02580069, 0x1d8e048c,
        ],
    ),
];

fn retail_payload(words: [u32; 16]) -> Vec<u8> {
    let mut checksum = Vec::with_capacity(CheckSums::WIRE_LEN);
    checksum.push(0x39);
    for word in words {
        checksum.extend_from_slice(&word.to_le_bytes());
    }
    assert_eq!(checksum.len(), 65);
    let command = Command {
        opcode: 0x39,
        bytes: &checksum,
    };
    let mut payload = Vec::new();
    encode_commands(
        &[command],
        &mut Obfuscation::multiplayer(GAME_KEY),
        &mut payload,
    );
    xor_payload(&mut payload, Obfuscation::xor_key(GAME_KEY));
    assert_eq!(payload.len(), 65);
    payload
}

fn assert_retail_package(package: &TurnPackage, stamp: u32, play: i8, words: [u32; 16]) {
    assert_eq!((package.stamp, package.play), (stamp, play));
    let decoded = decode_retail_checksum_package(package, GAME_KEY).unwrap();
    assert_eq!((decoded.stamp, decoded.play), (stamp, play));
    assert_eq!(decoded.command_count, 1);
    assert_eq!(decoded.opcodes, [0x39]);
    assert_eq!(decoded.checksums, CheckSums(words));
}

fn roster_ready(session: &Session<TcpTransport>) -> bool {
    let players = session.players();
    players.len() == 2
        && players[0].unique_id == HOST_ID
        && players[0].slot == 0
        && players[0].name == "Ai"
        && players[1].unique_id == CLIENT_ID
        && players[1].slot == 1
        && players[1].name == "Ai"
}

fn run_reactive_client(addr: SocketAddr, transcript: Vec<(u32, [u32; 16])>) -> (usize, usize) {
    let transport = TcpTransport::join(CLIENT_ID, addr).unwrap();
    let mut client = Session::new(transport, Role::Client, "Ai");
    let start = Instant::now();
    let now = || start.elapsed().as_millis() as u64;
    let mut sequence = 0usize;
    let mut joined = None;
    let mut host_ready = None;

    while !roster_ready(&client) {
        client.poll(now(), Duration::from_millis(5)).unwrap();
        for event in client.drain_events() {
            sequence += 1;
            match event {
                Event::PlayerJoined(HOST_ID) => {
                    joined.get_or_insert(sequence);
                }
                Event::ReadyChanged {
                    unique_id: HOST_ID,
                    ready: true,
                } => {
                    host_ready.get_or_insert(sequence);
                }
                _ => {}
            }
        }
        assert!(start.elapsed() < Duration::from_secs(5));
    }
    client.send_ready_flag(true).unwrap();
    while !client.all_ready() {
        client.poll(now(), Duration::from_millis(5)).unwrap();
        for event in client.drain_events() {
            sequence += 1;
            match event {
                Event::PlayerJoined(HOST_ID) => {
                    joined.get_or_insert(sequence);
                }
                Event::ReadyChanged {
                    unique_id: HOST_ID,
                    ready: true,
                } => {
                    host_ready.get_or_insert(sequence);
                }
                _ => {}
            }
        }
        assert!(start.elapsed() < Duration::from_secs(5));
    }
    assert!(joined.is_some() && host_ready.is_some() && joined < host_ready);

    for (stamp, words) in transcript {
        while client.package_for_turn(stamp, 0).is_none() {
            client.poll(now(), Duration::from_millis(5)).unwrap();
            client.drain_events();
            assert!(start.elapsed() < Duration::from_secs(8));
        }
        let host_payload = {
            let package = client.package_for_turn(stamp, 0).unwrap();
            assert_retail_package(package, stamp, 0, words);
            package.payload.clone()
        };
        client
            .send_command_package(stamp, 1, &host_payload)
            .unwrap();
        assert!(client.turn_ready(stamp));
        let packages = client.take_turn(stamp);
        assert_eq!(packages.len(), 2);
        assert_retail_package(&packages[0], stamp, 0, words);
        assert_retail_package(&packages[1], stamp, 1, words);
    }
    client.announce_disconnect().unwrap();
    (joined.unwrap(), host_ready.unwrap())
}

fn poll_host(host: &mut Session<TcpTransport>, start: &Instant, events: &mut Vec<Event>) {
    host.poll(start.elapsed().as_millis() as u64, Duration::from_millis(5))
        .unwrap();
    events.extend(host.drain_events());
}

#[test]
fn retail_transcript_crosses_lockstep_remove_and_reconnect_epochs() {
    let transport = TcpTransport::host(HOST_ID, "127.0.0.1:0").unwrap();
    let addr = transport.local_addr().unwrap();
    let mut host = Session::new(transport, Role::Host, "Ai");
    let start = Instant::now();
    let first = std::thread::spawn(move || run_reactive_client(addr, TRANSCRIPT.to_vec()));
    let mut events = Vec::new();

    while !roster_ready(&host) {
        poll_host(&mut host, &start, &mut events);
        assert!(start.elapsed() < Duration::from_secs(5));
    }
    host.send_ready_flag(true).unwrap();
    while !host.all_ready() {
        poll_host(&mut host, &start, &mut events);
        assert!(start.elapsed() < Duration::from_secs(5));
    }
    let joined = events
        .iter()
        .position(|event| *event == Event::PlayerJoined(CLIENT_ID))
        .unwrap();
    let ready = events
        .iter()
        .position(|event| {
            *event
                == Event::ReadyChanged {
                    unique_id: CLIENT_ID,
                    ready: true,
                }
        })
        .unwrap();
    assert!(joined < ready);

    let mut lockstep = LockstepRunner::new(
        GAME_KEY,
        [0, 1],
        TRANSCRIPT[0].0,
        start.elapsed().as_millis() as u64,
        2_000,
    )
    .unwrap();
    let mut replay_ms = 0u64;
    let mut replay_actions = vec![ReplayAction::Initial {
        at_ms: replay_ms,
        first_stamp: TRANSCRIPT[0].0,
        members: vec![
            EpochMember {
                play: 0,
                unique_id: HOST_ID,
            },
            EpochMember {
                play: 1,
                unique_id: CLIENT_ID,
            },
        ],
    }];

    for (stamp, words) in TRANSCRIPT {
        let payload = retail_payload(words);
        let mut exact_wire = Vec::new();
        NetMsg::CommandPackage {
            stamp,
            play: 0,
            payload: &payload,
        }
        .encode(&mut exact_wire);
        assert_eq!(exact_wire.len(), 73);
        host.send_command_package(stamp, 0, &payload).unwrap();
        while !host.turn_ready(stamp) {
            poll_host(&mut host, &start, &mut events);
            assert!(start.elapsed() < Duration::from_secs(8));
        }
        let packages = host.take_turn(stamp);
        assert_eq!(packages.len(), 2);
        assert_retail_package(&packages[0], stamp, 0, words);
        assert_retail_package(&packages[1], stamp, 1, words);
        for package in packages {
            replay_ms += 1;
            replay_actions.push(ReplayAction::Package {
                at_ms: replay_ms,
                package: package.clone(),
            });
            lockstep
                .submit(package, start.elapsed().as_millis() as u64)
                .unwrap();
        }
        assert!(matches!(
            lockstep.status(start.elapsed().as_millis() as u64),
            LockstepStatus::Ready { stamp: ready, .. } if ready == stamp
        ));
        let evidence = lockstep
            .commit_ready(start.elapsed().as_millis() as u64)
            .unwrap();
        assert_eq!(evidence.stamp, stamp);
        assert!(evidence.checksums_agree());
        replay_ms += 1;
        replay_actions.push(ReplayAction::Commit { at_ms: replay_ms });
    }
    let (client_joined, client_ready) = first.join().unwrap();
    assert!(client_joined < client_ready);
    while !events.contains(&Event::PlayerLeft(CLIENT_ID)) {
        poll_host(&mut host, &start, &mut events);
        assert!(start.elapsed() < Duration::from_secs(9));
    }
    assert_eq!(host.players().len(), 1);
    lockstep
        .begin_epoch([0], EpochCause::Drop, start.elapsed().as_millis() as u64)
        .unwrap();
    replay_ms += 1;
    replay_actions.push(ReplayAction::Epoch {
        at_ms: replay_ms,
        cause: EpochCause::Drop,
        members: vec![EpochMember {
            play: 0,
            unique_id: HOST_ID,
        }],
    });

    // Same owned id, new membership epoch. Resetting announced_to on the
    // destroy packet makes the host publish AddPlayer then its current ready
    // state to the fresh channel without harness-specific intervention.
    events.clear();
    let reconnect =
        std::thread::spawn(move || run_reactive_client(addr, vec![(26, TRANSCRIPT[0].1)]));
    while !roster_ready(&host) || !host.all_ready() {
        poll_host(&mut host, &start, &mut events);
        assert!(start.elapsed() < Duration::from_secs(12));
    }
    let joined = events
        .iter()
        .position(|event| *event == Event::PlayerJoined(CLIENT_ID))
        .unwrap();
    let ready = events
        .iter()
        .position(|event| {
            *event
                == Event::ReadyChanged {
                    unique_id: CLIENT_ID,
                    ready: true,
                }
        })
        .unwrap();
    assert!(joined < ready);
    lockstep
        .begin_epoch(
            [0, 1],
            EpochCause::Reconnect,
            start.elapsed().as_millis() as u64,
        )
        .unwrap();
    replay_ms += 1;
    replay_actions.push(ReplayAction::Epoch {
        at_ms: replay_ms,
        cause: EpochCause::Reconnect,
        members: vec![
            EpochMember {
                play: 0,
                unique_id: HOST_ID,
            },
            EpochMember {
                play: 1,
                unique_id: CLIENT_ID,
            },
        ],
    });

    let payload = retail_payload(TRANSCRIPT[0].1);
    host.send_command_package(26, 0, &payload).unwrap();
    while !host.turn_ready(26) {
        poll_host(&mut host, &start, &mut events);
        assert!(start.elapsed() < Duration::from_secs(14));
    }
    let packages = host.take_turn(26);
    assert_retail_package(&packages[0], 26, 0, TRANSCRIPT[0].1);
    assert_retail_package(&packages[1], 26, 1, TRANSCRIPT[0].1);
    for package in packages {
        replay_ms += 1;
        replay_actions.push(ReplayAction::Package {
            at_ms: replay_ms,
            package: package.clone(),
        });
        lockstep
            .submit(package, start.elapsed().as_millis() as u64)
            .unwrap();
    }
    assert!(lockstep
        .commit_ready(start.elapsed().as_millis() as u64)
        .unwrap()
        .checksums_agree());
    replay_ms += 1;
    replay_actions.push(ReplayAction::Commit { at_ms: replay_ms });
    assert_eq!(lockstep.expected_stamp(), 27);
    let transcript = lockstep.export_json();
    assert!(transcript.contains("\"stamp\":23"));
    assert!(transcript.contains("\"stamp\":26"));
    assert!(transcript.contains("\"cause\":\"drop\""));
    assert!(transcript.contains("\"cause\":\"reconnect\""));
    assert!(!transcript.contains("\"type\":\"timeout\""));
    assert_eq!(lockstep.transcript_fnv1a64(), 0x438b_c8d3_1e90_3ea7);
    let persisted = PersistedLockstepTranscript::record(GAME_KEY, 2_000, replay_actions).unwrap();
    assert_eq!(persisted.outcome_json(), transcript);
    assert_eq!(persisted.outcome_fnv1a64(), lockstep.transcript_fnv1a64());
    let binary = persisted.encode().unwrap();
    let decoded = PersistedLockstepTranscript::decode(&binary).unwrap();
    assert_eq!(decoded.encode().unwrap(), binary);
    assert_eq!(decoded.replay().unwrap().evidence, lockstep.evidence());
    assert_eq!(decoded.binary_fnv1a64().unwrap(), 0x26d9_4d66_3d14_0bba);
    let (client_joined, client_ready) = reconnect.join().unwrap();
    assert!(client_joined < client_ready);
}
