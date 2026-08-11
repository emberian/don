use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};

fn json_string_field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let marker = format!(r#""{name}":""#);
    let tail = line.split_once(&marker)?.1;
    tail.split_once('"').map(|(value, _)| value)
}

fn final_hash(output: &str) -> Option<&str> {
    output
        .lines()
        .find(|line| line.contains(r#""event":"done""#))
        .and_then(|line| json_string_field(line, "final_hash"))
}

#[test]
fn executable_crosses_the_explicit_start_before_its_first_turn() {
    let exe = env!("CARGO_BIN_EXE_donnet-peer");
    let mut host = Command::new(exe)
        .args([
            "host",
            "--bind",
            "127.0.0.1:0",
            "--id",
            "101",
            "--name",
            "host",
            "--turns",
            "3",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let mut host_stdout = BufReader::new(host.stdout.take().unwrap());
    let mut listening = String::new();
    host_stdout.read_line(&mut listening).unwrap();
    let address = json_string_field(&listening, "addr")
        .unwrap_or_else(|| panic!("host did not publish an address: {listening:?}"))
        .to_string();

    let client = Command::new(exe)
        .args([
            "join", "--addr", &address, "--id", "202", "--name", "client", "--turns", "3",
        ])
        .output()
        .unwrap();
    assert!(
        client.status.success(),
        "client failed: {}",
        String::from_utf8_lossy(&client.stderr)
    );

    let host_status = host.wait().unwrap();
    let mut host_tail = String::new();
    host_stdout.read_to_string(&mut host_tail).unwrap();
    let host_stderr = {
        let mut bytes = Vec::new();
        host.stderr.take().unwrap().read_to_end(&mut bytes).unwrap();
        String::from_utf8_lossy(&bytes).into_owned()
    };
    assert!(host_status.success(), "host failed: {host_stderr}");

    let host_output = format!("{listening}{host_tail}");
    let client_output = String::from_utf8(client.stdout).unwrap();
    for (side, output) in [
        ("host", host_output.as_str()),
        ("client", client_output.as_str()),
    ] {
        let start_at = output
            .find(r#""event":"match_started""#)
            .unwrap_or_else(|| panic!("{side} never reported MatchStart: {output}"));
        let turn_at = output
            .find(r#""event":"turn""#)
            .unwrap_or_else(|| panic!("{side} never reported a turn: {output}"));
        assert!(
            start_at < turn_at,
            "{side} emitted a turn before MatchStart: {output}"
        );
        assert!(output.contains(r#""epoch":1,"seed":218763726"#));
    }
    assert_eq!(final_hash(&host_output), final_hash(&client_output));
    assert!(final_hash(&host_output).is_some());
}
