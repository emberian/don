// SPDX-License-Identifier: GPL-3.0-or-later
//! Two-process fixture for Crossplay StartGame → don-net MatchStart → turn.

use std::io::{self, Write};
use std::net::SocketAddr;
use std::process;
use std::thread;
use std::time::{Duration, Instant};

use don_crossplay::abi::{SESSION_STATUS_STARTED, VISIBILITY_PUBLIC};
use don_crossplay::directory_rpc::DirectoryRpcClient;
use don_crossplay::match_bridge::{
    ServiceMatch, DON_MATCH_ENDPOINT_ATTRIBUTE, GAME_SEED_ATTRIBUTE,
};
use don_crossplay::{AsyncDirectory, Attributes, Backend, Emission, Lobby, Outcome, ReqId};
use don_net::{LocalMatch, Role, Session, TcpTransport};

const HOST_ID: i32 = 101;
const CLIENT_ID: i32 = 202;
const DEFAULT_GAME_SEED: u32 = 3_134_984_190;
const MATCH_TIMEOUT: Duration = Duration::from_secs(15);

fn main() {
    if let Err(error) = run() {
        eprintln!("service-match-peer: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("host") if args.len() == 2 => host(DEFAULT_GAME_SEED),
        Some("host") if args.len() == 4 && args[2] == "--seed" => host(parse_seed(&args[3])?),
        Some("join") if args.len() == 4 => join(&args[2], &args[3]),
        _ => Err("usage: service-match-peer host [--seed U32] | join DIRECTORY LOBBY_ID".into()),
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

fn host(game_seed: u32) -> Result<(), String> {
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
        }

        if published && !sent_turn {
            send_fixture_turn(&mut match_)?;
            sent_turn = true;
        }
        if sent_turn && match_.turn_ready(0) {
            report_turn(&mut match_, HOST_ID)?;
            return Ok(());
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn join(directory_address: &str, lobby_id: &str) -> Result<(), String> {
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
            }
        }
        if reported_match && !sent_turn {
            send_fixture_turn(&mut match_)?;
            sent_turn = true;
        }
        if sent_turn && match_.turn_ready(0) {
            report_turn(&mut match_, CLIENT_ID)?;
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

fn report_start(event: &str, start: &don_crossplay::match_bridge::ServiceStart) {
    println!(
        r#"{{"event":"{event}","lobby":"{}","reference":"{}","epoch":{},"seed":{}}}"#,
        start.lobby_id, start.session_reference, start.epoch, start.seed
    );
}

fn report_turn(match_: &mut ServiceMatch<TcpTransport>, id: i32) -> Result<(), String> {
    let packages = match_.take_turn(0);
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
    println!(
        r#"{{"event":"turn","stamp":0,"packages":{},"hash":"{hash:016x}"}}"#,
        packages.len()
    );
    println!(r#"{{"event":"done","id":{id},"hash":"{hash:016x}"}}"#);
    Ok(())
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
