// SPDX-License-Identifier: GPL-3.0-or-later

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use don_crossplay::match_bridge::service_epoch;

const TEST_SEED: u32 = 0x89ab_cdef;

#[test]
fn service_start_authorizes_match_start_and_turns_across_processes() {
    let peer = env!("CARGO_BIN_EXE_service-match-peer");
    let mut host = Command::new(peer)
        .args(["host", "--seed", "0x89abcdef"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("launch host service/match process");

    let mut host_stdout = BufReader::new(host.stdout.take().expect("host stdout"));
    let mut service_line = String::new();
    host_stdout
        .read_line(&mut service_line)
        .expect("read host service line");
    let fields: Vec<&str> = service_line.split_whitespace().collect();
    assert_eq!(fields.len(), 3, "unexpected host line: {service_line:?}");
    assert_eq!(
        fields[0], "SERVICE",
        "unexpected host line: {service_line:?}"
    );
    let directory_address = fields[1].to_string();
    let lobby_id = fields[2].to_string();

    let mut client = Command::new(peer)
        .args(["join", &directory_address, &lobby_id])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("launch joining service/match process");

    wait_both(&mut host, &mut client, Duration::from_secs(20));

    let mut host_tail = String::new();
    host_stdout
        .read_to_string(&mut host_tail)
        .expect("read host tail");
    let host_stderr = read_pipe(host.stderr.take());
    let client_stdout = read_pipe(client.stdout.take());
    let client_stderr = read_pipe(client.stderr.take());
    let host_status = host.wait().expect("reap host");
    let client_status = client.wait().expect("reap client");
    assert!(
        host_status.success(),
        "host failed\nstdout: {service_line}{host_tail}\nstderr: {host_stderr}"
    );
    assert!(
        client_status.success(),
        "client failed\nstdout: {client_stdout}\nstderr: {client_stderr}"
    );

    let host_output = format!("{service_line}{host_tail}");
    let expected_epoch = service_epoch(&lobby_id, &lobby_id);
    for (side, output) in [
        ("host", host_output.as_str()),
        ("client", client_stdout.as_str()),
    ] {
        let directory_at = event_offset(output, "directory_started", side);
        let match_at = event_offset(output, "match_confirmed", side);
        let turn_at = event_offset(output, "turn", side);
        assert!(
            directory_at < match_at && match_at < turn_at,
            "{side} crossed lifecycle out of order: {output}"
        );
        assert!(
            output.contains(&format!(r#""reference":"{lobby_id}""#)),
            "{side} did not use the completed StartGame reference: {output}"
        );
        assert!(
            output.contains(&format!(r#""epoch":{expected_epoch},"seed":{TEST_SEED}"#)),
            "{side} did not derive MatchStart from directory state: {output}"
        );
        assert!(
            output.contains(r#""event":"turn","stamp":0,"packages":2"#),
            "{side} did not finish a two-package turn: {output}"
        );
    }

    assert_eq!(
        done_hash(&host_output),
        done_hash(&client_stdout),
        "peers disagreed on the authoritative turn stream"
    );
    assert!(done_hash(&host_output).is_some());
}

#[test]
fn relay_mode_keeps_native_turn_ownership_and_orders_canonical_packages() {
    let peer = env!("CARGO_BIN_EXE_service-match-peer");
    let mut host = Command::new(peer)
        .args(["host", "--seed", "0x89abcdef", "--relay"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("launch relay host");
    let mut host_stdout = BufReader::new(host.stdout.take().expect("host stdout"));
    let mut service_line = String::new();
    host_stdout
        .read_line(&mut service_line)
        .expect("read SERVICE");
    let fields: Vec<&str> = service_line.split_whitespace().collect();
    assert_eq!(fields.len(), 3, "unexpected host line: {service_line:?}");

    let mut client = Command::new(peer)
        .args(["join", fields[1], fields[2], "--relay"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("launch relay client");
    let mut client_stdout = BufReader::new(client.stdout.take().expect("client stdout"));
    read_until(
        &mut host_stdout,
        r#""event":"relay_ready""#,
        "host relay ready",
    );
    read_until(
        &mut client_stdout,
        r#""event":"relay_ready""#,
        "client relay ready",
    );

    host.stdin
        .as_mut()
        .expect("host stdin")
        .write_all(b"TURN 0 0c\n")
        .expect("submit host turn");
    client
        .stdin
        .as_mut()
        .expect("client stdin")
        .write_all(b"TURN 0 0c\n")
        .expect("submit client turn");
    let host_turn = read_until(&mut host_stdout, r#""event":"turn""#, "host turn");
    let client_turn = read_until(&mut client_stdout, r#""event":"turn""#, "client turn");
    for output in [&host_turn, &client_turn] {
        assert!(output.contains(r#""stamp":0,"packages":2"#), "{output}");
        assert!(output.contains(
            r#""ordered":[{"stamp":0,"play":0,"payload":"0c"},{"stamp":0,"play":1,"payload":"0c"}]"#
        ), "{output}");
    }
    assert_eq!(turn_hash(&host_turn), turn_hash(&client_turn));

    // Turn 1 deliberately carries two different, multi-command packages.
    // Each is a canonical Group(single owner-local object) followed by the
    // complete 22-byte MoveTo body. The relay supplies `play` from the
    // confirmed native roster and must preserve each opaque payload exactly.
    const HOST_GROUP_MOVE: &str = "0001000100074bb900007eb9000000000000000000000102003200";
    const CLIENT_GROUP_MOVE: &str = "000101090007e02e0000803e000000000000000000000102003200";
    writeln!(
        host.stdin.as_mut().expect("host stdin"),
        "TURN 1 {HOST_GROUP_MOVE}"
    )
    .expect("submit host Group+Move turn");
    writeln!(
        client.stdin.as_mut().expect("client stdin"),
        "TURN 1 {CLIENT_GROUP_MOVE}"
    )
    .expect("submit client Group+Move turn");
    let host_move = read_until(
        &mut host_stdout,
        r#""event":"turn""#,
        "host Group+Move turn",
    );
    let client_move = read_until(
        &mut client_stdout,
        r#""event":"turn""#,
        "client Group+Move turn",
    );
    let expected = format!(
        r#""ordered":[{{"stamp":1,"play":0,"payload":"{HOST_GROUP_MOVE}"}},{{"stamp":1,"play":1,"payload":"{CLIENT_GROUP_MOVE}"}}]"#
    );
    for output in [&host_move, &client_move] {
        assert!(output.contains(r#""stamp":1,"packages":2"#), "{output}");
        assert!(output.contains(&expected), "{output}");
    }
    assert_eq!(turn_hash(&host_move), turn_hash(&client_move));
    assert_ne!(turn_hash(&host_turn), turn_hash(&host_move));

    host.stdin.as_mut().unwrap().write_all(b"QUIT\n").unwrap();
    client.stdin.as_mut().unwrap().write_all(b"QUIT\n").unwrap();
    wait_both(&mut host, &mut client, Duration::from_secs(5));
    assert!(host.wait().expect("reap host").success());
    assert!(client.wait().expect("reap client").success());
}

fn read_until(reader: &mut impl BufRead, needle: &str, stage: &str) -> String {
    loop {
        let mut line = String::new();
        let count = reader.read_line(&mut line).expect(stage);
        assert_ne!(count, 0, "peer exited before {stage}");
        if line.contains(needle) {
            return line;
        }
    }
}

fn turn_hash(output: &str) -> Option<&str> {
    let tail = output.split_once(r#""hash":""#)?.1;
    tail.split_once('"').map(|(hash, _)| hash)
}

fn event_offset(output: &str, event: &str, side: &str) -> usize {
    output
        .find(&format!(r#""event":"{event}""#))
        .unwrap_or_else(|| panic!("{side} never reported {event}: {output}"))
}

fn done_hash(output: &str) -> Option<&str> {
    let line = output
        .lines()
        .find(|line| line.contains(r#""event":"done""#))?;
    let tail = line.split_once(r#""hash":""#)?.1;
    tail.split_once('"').map(|(hash, _)| hash)
}

fn wait_both(host: &mut Child, client: &mut Child, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        let host_done = host.try_wait().expect("poll host").is_some();
        let client_done = client.try_wait().expect("poll client").is_some();
        if host_done && client_done {
            return;
        }
        if Instant::now() >= deadline {
            let _ = host.kill();
            let _ = client.kill();
            panic!("service/match subprocesses did not finish within {timeout:?}");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn read_pipe(pipe: Option<impl Read>) -> String {
    let mut output = String::new();
    pipe.expect("child pipe")
        .read_to_string(&mut output)
        .expect("read child pipe");
    output
}
