// SPDX-License-Identifier: GPL-3.0-or-later
//! Two-process fixture for Crossplay StartGame → don-net MatchStart → turn.

use std::io::{self, BufRead, Write};
use std::net::SocketAddr;
use std::process;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use don_crossplay::abi::{SESSION_STATUS_STARTED, VISIBILITY_PUBLIC};
use don_crossplay::directory_rpc::DirectoryRpcClient;
use don_crossplay::match_bridge::{
    ServiceMatch, DON_MATCH_ENDPOINT_ATTRIBUTE, GAME_SEED_ATTRIBUTE,
};
use don_crossplay::{AsyncDirectory, Attributes, Backend, Emission, Lobby, Outcome, ReqId};
use don_net::{
    decode_commands, encode_commands, LocalMatch, Obfuscation, Role, Session, TcpTransport,
    MAX_COMMAND_PACKAGE_PAYLOAD,
};

const HOST_ID: i32 = 101;
const CLIENT_ID: i32 = 202;
const DEFAULT_GAME_SEED: u32 = 3_134_984_190;
const MATCH_TIMEOUT: Duration = Duration::from_secs(15);
const RELAY_LIFETIME: Duration = Duration::from_secs(10 * 60);
// `TURN ` + max u32 + one separator + a complete 512-byte retail payload in hex.
const MAX_RELAY_LINE_BYTES: usize = 4 + 1 + 10 + 1 + MAX_COMMAND_PACKAGE_PAYLOAD * 2;

fn main() {
    if let Err(error) = run() {
        eprintln!("service-match-peer: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("host") if args.len() == 2 => host(DEFAULT_GAME_SEED, false),
        Some("host") if args.len() == 3 && args[2] == "--relay" => {
            host(DEFAULT_GAME_SEED, true)
        }
        Some("host") if args.len() == 4 && args[2] == "--seed" => {
            host(parse_seed(&args[3])?, false)
        }
        Some("host") if args.len() == 5 && args[2] == "--seed" && args[4] == "--relay" => {
            host(parse_seed(&args[3])?, true)
        }
        Some("join") if args.len() == 4 => join(&args[2], &args[3], false),
        Some("join") if args.len() == 5 && args[4] == "--relay" => {
            join(&args[2], &args[3], true)
        }
        _ => Err("usage: service-match-peer host [--seed U32] [--relay] | join DIRECTORY LOBBY_ID [--relay]".into()),
    }
}

fn parse_seed(text: &str) -> Result<u32, String> {
    let parsed = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .map_or_else(|| text.parse::<u32>(), |hex| u32::from_str_radix(hex, 16))
        .map_err(|_| format!("invalid u32 game seed {text:?}"))?;
    Ok(parsed)
}

fn host(game_seed: u32, relay: bool) -> Result<(), String> {
    let mut directory = DirectoryRpcClient::listen("127.0.0.1:0")
        .map_err(|error| format!("listen for directory: {error}"))?;
    let directory_address = directory
        .local_addr()
        .ok_or_else(|| "directory listener did not expose its address".to_string())?;
    let mut service = started(&HOST_ID.to_string(), "Host", &mut directory)?;

    let transport = TcpTransport::host(HOST_ID, "127.0.0.1:0")
        .map_err(|error| format!("listen for don-net: {error}"))?;
    let match_address = transport
        .local_addr()
        .map_err(|error| format!("read don-net address: {error}"))?;
    let mut match_ =
        ServiceMatch::new(LocalMatch::new(Session::new(transport, Role::Host, "Host")));

    let mut attributes = Attributes::new();
    attributes.insert(GAME_SEED_ATTRIBUTE.into(), game_seed.to_string());
    attributes.insert(
        DON_MATCH_ENDPOINT_ATTRIBUTE.into(),
        match_address.to_string(),
    );
    let request = service.create_lobby(2, VISIBILITY_PUBLIC, attributes);
    let created = expect_lobby(
        complete(&mut service, &mut directory, request)?,
        "CreateLobby",
    )?;

    println!("SERVICE {directory_address} {}", created.id);
    io::stdout().flush().map_err(|error| error.to_string())?;

    let clock = Instant::now();
    let mut bound = false;
    let mut readied = false;
    let mut published = false;
    let mut sent_turn = false;
    loop {
        ensure_deadline(clock, "host lifecycle")?;
        match_
            .poll(elapsed_ms(clock), Duration::from_millis(2))
            .map_err(|error| format!("poll don-net host: {error}"))?;

        if !bound && match_.players().len() == 2 {
            let request = service.get_lobby(&created.id);
            let lobby = expect_lobby(
                complete(&mut service, &mut directory, request)?,
                "GetLobby before StartGame",
            )?;
            match_
                .observe_lobby(&lobby)
                .map_err(|error| format!("bind host roster: {error}"))?;
            bound = true;
        }

        if bound && !readied {
            match_
                .set_ready(true)
                .map_err(|error| format!("ready host: {error}"))?;
            readied = true;
        }

        if readied && match_.all_ready() && !published {
            // This is the same DoN policy the vtable thunk uses: the lobby id
            // is the session reference returned by StartGame's callback.
            let request = service.start_game(&created.id, &created.id);
            let reference = match complete(&mut service, &mut directory, request)? {
                Outcome::SessionReference(reference) => reference,
                other => return Err(format!("StartGame returned {other:?}")),
            };
            if reference != created.id {
                return Err(format!(
                    "StartGame reference {reference:?} did not equal requested {:?}",
                    created.id
                ));
            }
            let request = service.get_lobby(&created.id);
            let lobby = expect_lobby(
                complete(&mut service, &mut directory, request)?,
                "GetLobby after StartGame",
            )?;
            match_
                .observe_lobby(&lobby)
                .map_err(|error| format!("observe host StartGame: {error}"))?;
            let start = match_
                .publish_match_start()
                .map_err(|error| format!("publish MatchStart: {error}"))?;
            report_start("directory_started", &start);
            report_start("match_confirmed", &start);
            published = true;
            if relay {
                return run_relay(&mut match_, HOST_ID, clock);
            }
        }

        if published && !sent_turn {
            send_fixture_turn(&mut match_)?;
            sent_turn = true;
        }
        if sent_turn && match_.turn_ready(0) {
            let hash = report_turn(&mut match_, 0)?;
            println!(r#"{{"event":"done","id":{HOST_ID},"hash":"{hash:016x}"}}"#);
            return Ok(());
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn join(directory_address: &str, lobby_id: &str, relay: bool) -> Result<(), String> {
    let directory_address: SocketAddr = directory_address
        .parse()
        .map_err(|error| format!("invalid directory address: {error}"))?;
    if !directory_address.ip().is_loopback() {
        return Err("directory address must be loopback".into());
    }
    let mut directory = DirectoryRpcClient::connect(directory_address)
        .map_err(|error| format!("connect directory: {error}"))?;
    let mut service = started(&CLIENT_ID.to_string(), "Peer", &mut directory)?;

    let request = service.find_lobbies(16, 1);
    let found = match complete(&mut service, &mut directory, request)? {
        Outcome::Lobbies(lobbies) => lobbies,
        other => return Err(format!("FindLobbies returned {other:?}")),
    };
    let discovered = found
        .into_iter()
        .find(|lobby| lobby.id == lobby_id)
        .ok_or_else(|| format!("FindLobbies did not expose {lobby_id:?}"))?;
    if discovered.game_started {
        return Err("FindLobbies exposed an already-started match".into());
    }

    let request = service.join_lobby(lobby_id);
    let joined = expect_lobby(
        complete(&mut service, &mut directory, request)?,
        "JoinLobby",
    )?;
    let match_address = match_endpoint(&joined)?;
    let transport = TcpTransport::join(CLIENT_ID, match_address)
        .map_err(|error| format!("connect don-net: {error}"))?;
    let mut match_ = ServiceMatch::new(LocalMatch::new(Session::new(
        transport,
        Role::Client,
        "Peer",
    )));

    let clock = Instant::now();
    while match_.players().len() != 2 {
        ensure_deadline(clock, "client roster")?;
        match_
            .poll(elapsed_ms(clock), Duration::from_millis(2))
            .map_err(|error| format!("poll client roster: {error}"))?;
        thread::sleep(Duration::from_millis(1));
    }
    match_
        .observe_lobby(&joined)
        .map_err(|error| format!("bind client roster: {error}"))?;
    match_
        .set_ready(true)
        .map_err(|error| format!("ready client: {error}"))?;

    let mut reported_directory = false;
    let mut reported_match = false;
    let mut sent_turn = false;
    loop {
        ensure_deadline(clock, "client lifecycle")?;
        match_
            .poll(elapsed_ms(clock), Duration::from_millis(2))
            .map_err(|error| format!("poll don-net client: {error}"))?;

        if !reported_directory {
            let request = service.get_lobby(lobby_id);
            let lobby = expect_lobby(
                complete(&mut service, &mut directory, request)?,
                "GetLobby while waiting for StartGame",
            )?;
            match_
                .observe_lobby(&lobby)
                .map_err(|error| format!("observe client lobby: {error}"))?;
            if let Some(start) = match_.directory_start() {
                report_start("directory_started", start);
                reported_directory = true;
            }
        }

        if !reported_match {
            if let Some(start) = match_.confirmed_start() {
                report_start("match_confirmed", start);
                reported_match = true;
                if relay {
                    return run_relay(&mut match_, CLIENT_ID, clock);
                }
            }
        }
        if reported_match && !sent_turn {
            send_fixture_turn(&mut match_)?;
            sent_turn = true;
        }
        if sent_turn && match_.turn_ready(0) {
            let hash = report_turn(&mut match_, 0)?;
            println!(r#"{{"event":"done","id":{CLIENT_ID},"hash":"{hash:016x}"}}"#);
            return Ok(());
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn started(id: &str, name: &str, directory: &mut dyn AsyncDirectory) -> Result<Backend, String> {
    let mut service = Backend::new(id, name);
    service.init();
    let request = service.start_session(id);
    match complete(&mut service, directory, request)? {
        Outcome::SessionStarted(ref observed) if observed == id => {}
        other => return Err(format!("StartSession returned {other:?}")),
    }
    if service.session_status() != SESSION_STATUS_STARTED {
        return Err("StartSession did not reach Started".into());
    }
    Ok(service)
}

fn complete(
    service: &mut Backend,
    directory: &mut dyn AsyncDirectory,
    request: ReqId,
) -> Result<Outcome, String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        for emission in service.tick_remote(directory) {
            if let Emission::Completion(id, outcome) = emission {
                if id == request {
                    return Ok(outcome);
                }
            }
        }
        thread::sleep(Duration::from_millis(1));
    }
    Err(format!("directory request {request:?} did not complete"))
}

fn expect_lobby(outcome: Outcome, operation: &str) -> Result<Lobby, String> {
    match outcome {
        Outcome::Lobby(lobby) => Ok(lobby),
        other => Err(format!("{operation} returned {other:?}")),
    }
}

fn match_endpoint(lobby: &Lobby) -> Result<SocketAddr, String> {
    let text = lobby
        .attributes
        .get(DON_MATCH_ENDPOINT_ATTRIBUTE)
        .ok_or_else(|| format!("lobby is missing {DON_MATCH_ENDPOINT_ATTRIBUTE:?}"))?;
    let address: SocketAddr = text
        .parse()
        .map_err(|error| format!("invalid don-net endpoint {text:?}: {error}"))?;
    if !address.ip().is_loopback() {
        return Err(format!("don-net endpoint is not loopback: {address}"));
    }
    Ok(address)
}

fn send_fixture_turn(match_: &mut ServiceMatch<TcpTransport>) -> Result<(), String> {
    let slot = match_
        .players()
        .iter()
        .position(|player| player.unique_id == match_.local_id())
        .ok_or_else(|| "local player is absent from don-net roster".to_string())?
        as i8;
    let mut payload = vec![0x39u8];
    for channel in 0..16u32 {
        payload.extend_from_slice(&(0x5100_0000u32 + channel).to_le_bytes());
    }
    match_
        .send_turn(0, slot, &payload)
        .map_err(|error| format!("submit turn: {error}"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RelayCommand {
    Turn(u32, Vec<u8>),
    Quit,
}

fn relay_input() -> Receiver<Result<RelayCommand, String>> {
    let (send, receive) = mpsc::channel();
    thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let result = line
                .map_err(|error| format!("read relay input: {error}"))
                .and_then(|line| parse_relay_command(&line));
            let stop = result
                .as_ref()
                .is_ok_and(|command| matches!(command, RelayCommand::Quit))
                || result.is_err();
            if send.send(result).is_err() || stop {
                return;
            }
        }
        let _ = send.send(Ok(RelayCommand::Quit));
    });
    receive
}

fn parse_relay_command(line: &str) -> Result<RelayCommand, String> {
    if line.len() > MAX_RELAY_LINE_BYTES {
        return Err(format!("relay input exceeded {MAX_RELAY_LINE_BYTES} bytes"));
    }
    if line == "QUIT" {
        return Ok(RelayCommand::Quit);
    }
    let mut fields = line.split(' ');
    if fields.next() != Some("TURN") {
        return Err("relay input must be TURN U32 HEX or QUIT".to_string());
    }
    let stamp = fields
        .next()
        .ok_or_else(|| "relay TURN is missing its stamp".to_string())?
        .parse::<u32>()
        .map_err(|_| "relay TURN stamp must be a u32".to_string())?;
    let payload = decode_hex(
        fields
            .next()
            .ok_or_else(|| "relay TURN is missing its command payload".to_string())?,
    )?;
    if fields.next().is_some() {
        return Err("relay TURN has trailing fields".to_string());
    }
    validate_browser_turn(&payload)?;
    Ok(RelayCommand::Turn(stamp, payload))
}

fn run_relay(
    match_: &mut ServiceMatch<TcpTransport>,
    id: i32,
    poll_clock: Instant,
) -> Result<(), String> {
    let input = relay_input();
    let relay_clock = Instant::now();
    let mut expected_stamp = 0u32;
    let mut submitted = false;
    println!(r#"{{"event":"relay_ready","id":{id},"nextStamp":0}}"#);
    io::stdout().flush().map_err(|error| error.to_string())?;

    loop {
        if relay_clock.elapsed() > RELAY_LIFETIME {
            return Err(format!(
                "relay exceeded bounded lifetime of {} seconds",
                RELAY_LIFETIME.as_secs()
            ));
        }
        match_
            .poll(elapsed_ms(poll_clock), Duration::from_millis(2))
            .map_err(|error| format!("poll don-net relay: {error}"))?;

        match input.try_recv() {
            Ok(Ok(RelayCommand::Quit)) => return Ok(()),
            Ok(Ok(RelayCommand::Turn(stamp, payload))) => {
                if submitted || stamp != expected_stamp {
                    return Err(format!(
                        "relay refused TURN {stamp}; expected {expected_stamp} with no pending turn"
                    ));
                }
                send_browser_turn(match_, stamp, &payload)?;
                submitted = true;
            }
            Ok(Err(error)) => return Err(error),
            Err(TryRecvError::Disconnected) => return Ok(()),
            Err(TryRecvError::Empty) => {}
        }

        if submitted && match_.turn_ready(expected_stamp) {
            let hash = report_turn(match_, expected_stamp)?;
            println!(
                r#"{{"event":"turn_complete","stamp":{expected_stamp},"id":{id},"hash":"{hash:016x}"}}"#
            );
            io::stdout().flush().map_err(|error| error.to_string())?;
            expected_stamp = expected_stamp
                .checked_add(1)
                .ok_or_else(|| "relay turn stamp overflow".to_string())?;
            submitted = false;
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn send_browser_turn(
    match_: &mut ServiceMatch<TcpTransport>,
    stamp: u32,
    payload: &[u8],
) -> Result<(), String> {
    let slot = match_
        .players()
        .iter()
        .position(|player| player.unique_id == match_.local_id())
        .ok_or_else(|| "local player is absent from don-net roster".to_string())?
        as i8;
    match_
        .send_turn(stamp, slot, payload)
        .map_err(|error| format!("submit browser command turn {stamp}: {error}"))
}

fn decode_hex(text: &str) -> Result<Vec<u8>, String> {
    if text.is_empty() || text.len() % 2 != 0 || !text.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("relay TURN payload must be nonempty even-length hexadecimal".to_string());
    }
    (0..text.len())
        .step_by(2)
        .map(|offset| {
            u8::from_str_radix(&text[offset..offset + 2], 16)
                .map_err(|_| "relay TURN payload is not hexadecimal".to_string())
        })
        .collect()
}

fn validate_browser_turn(payload: &[u8]) -> Result<(), String> {
    if payload.is_empty() {
        return Err("relay TURN command package must be nonempty".to_string());
    }
    if payload.len() > MAX_COMMAND_PACKAGE_PAYLOAD {
        return Err(format!(
            "relay TURN command package is {} bytes; retail capacity is {}",
            payload.len(),
            MAX_COMMAND_PACKAGE_PAYLOAD,
        ));
    }
    let mut obfuscation = Obfuscation::none();
    let commands = decode_commands(payload, &mut obfuscation)
        .map_err(|error| format!("relay TURN command payload is malformed: {error}"))?;
    let mut canonical = Vec::with_capacity(payload.len());
    encode_commands(&commands, &mut Obfuscation::none(), &mut canonical);
    if canonical != payload {
        return Err(
            "relay TURN command package did not survive canonical decode/re-encode".to_string(),
        );
    }
    Ok(())
}

fn report_start(event: &str, start: &don_crossplay::match_bridge::ServiceStart) {
    println!(
        r#"{{"event":"{event}","lobby":"{}","reference":"{}","epoch":{},"seed":{}}}"#,
        start.lobby_id, start.session_reference, start.epoch, start.seed
    );
}

fn report_turn(match_: &mut ServiceMatch<TcpTransport>, stamp: u32) -> Result<u64, String> {
    let packages = match_.take_turn(stamp);
    if packages.len() != 2 {
        return Err(format!("turn contained {} packages", packages.len()));
    }
    let mut hash = 0u64;
    for package in &packages {
        hash = hash
            .wrapping_mul(1_000_003)
            .wrapping_add(package.stamp as u64);
        hash = hash
            .wrapping_mul(1_000_003)
            .wrapping_add(package.play as u64);
        for byte in &package.payload {
            hash = hash.wrapping_mul(1_000_003).wrapping_add(u64::from(*byte));
        }
    }
    let ordered = packages
        .iter()
        .map(|package| {
            format!(
                r#"{{"stamp":{},"play":{},"payload":"{}"}}"#,
                package.stamp,
                package.play,
                hex(&package.payload)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    println!(
        r#"{{"event":"turn","stamp":{stamp},"packages":{},"ordered":[{ordered}],"hash":"{hash:016x}"}}"#,
        packages.len()
    );
    io::stdout().flush().map_err(|error| error.to_string())?;
    Ok(hash)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(DIGITS[usize::from(byte >> 4)] as char);
        text.push(DIGITS[usize::from(byte & 0xf)] as char);
    }
    text
}

fn elapsed_ms(start: Instant) -> u64 {
    start.elapsed().as_millis() as u64
}

fn ensure_deadline(start: Instant, stage: &str) -> Result<(), String> {
    if start.elapsed() > MATCH_TIMEOUT {
        Err(format!("timed out during {stage}"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GROUP_MOVE: &[u8] = &[
        0x00, 0x01, 0x00, 0x01, 0x00, // Group(who=0, o=1)
        0x07, 0x4b, 0xb9, 0x00, 0x00, 0x7e, 0xb9, 0x00, 0x00, // x/y
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // set_angle/angle
        0x01, 0x02, 0x00, 0x32, 0x00, // orders/queued/form/width/disembark
    ];

    #[test]
    fn browser_relay_validation_is_package_generic_and_byte_exact() {
        validate_browser_turn(&[0x0c]).expect("Halt remains canonical");
        validate_browser_turn(GROUP_MOVE).expect("Group+Move is a canonical command list");
        validate_browser_turn(&[0x0c, 0x0c])
            .expect("transport admits more than one recognized command");

        assert!(validate_browser_turn(&[]).unwrap_err().contains("nonempty"));
        assert!(validate_browser_turn(&[0x52])
            .unwrap_err()
            .contains("unknown opcode"));
        assert!(validate_browser_turn(&GROUP_MOVE[..GROUP_MOVE.len() - 1])
            .unwrap_err()
            .contains("truncated"));
    }

    #[test]
    fn browser_relay_validation_enforces_retail_data_capacity() {
        validate_browser_turn(&vec![0x0c; MAX_COMMAND_PACKAGE_PAYLOAD])
            .expect("512 one-byte commands exactly fill retail data[]");
        assert!(
            validate_browser_turn(&vec![0x0c; MAX_COMMAND_PACKAGE_PAYLOAD + 1])
                .unwrap_err()
                .contains("retail capacity")
        );
    }
}
