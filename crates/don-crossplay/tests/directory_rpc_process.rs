// SPDX-License-Identifier: GPL-3.0-or-later

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn create_find_and_join_cross_two_service_processes() {
    let peer = env!("CARGO_BIN_EXE_directory-rpc-peer");
    let mut host = Command::new(peer)
        .arg("host")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("launch host service process");

    let line = {
        let stdout = host.stdout.take().expect("host stdout");
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).expect("read host lobby line");
        line
    };
    let fields: Vec<&str> = line.split_whitespace().collect();
    assert_eq!(fields.len(), 3, "unexpected host line: {line:?}");
    assert_eq!(fields[0], "LOBBY", "unexpected host line: {line:?}");
    let address = fields[1];
    let lobby_id = fields[2];

    let join = Command::new(peer)
        .args(["join", address, lobby_id])
        .output()
        .expect("launch joining service process");
    assert!(
        join.status.success(),
        "join service failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&join.stdout),
        String::from_utf8_lossy(&join.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&join.stdout).trim(),
        format!("JOINED {lobby_id} 2")
    );

    writeln!(host.stdin.take().expect("host stdin"), "done").expect("release host");
    let status = wait_bounded(&mut host, Duration::from_secs(5));
    assert!(status.success(), "host service failed with {status}");
}

fn wait_bounded(child: &mut Child, timeout: Duration) -> std::process::ExitStatus {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().expect("poll child") {
            return status;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let _ = child.kill();
    child.wait().expect("reap timed-out child")
}
