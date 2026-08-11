// SPDX-License-Identifier: GPL-3.0-or-later

use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use don_crossplay::match_bridge::service_epoch;

#[test]
fn service_start_authorizes_match_start_and_turns_across_processes() {
    let peer = env!("CARGO_BIN_EXE_service-match-peer");
    let mut host = Command::new(peer)
        .arg("host")
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
            output.contains(&format!(r#""epoch":{expected_epoch},"seed":3134984190"#)),
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
