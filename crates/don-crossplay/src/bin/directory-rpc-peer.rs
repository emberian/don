// SPDX-License-Identifier: GPL-3.0-or-later
//! Subprocess fixture for the cross-process directory lifecycle test.
//!
//! Each invocation owns one `Backend`, which is the per-instance semantic
//! state behind `LocalCrossPlayService`. The host invocation also owns the
//! loopback directory authority. The integration test intentionally keeps this
//! binary tiny so a passing result is evidence about the real RPC and Tick
//! adapter, not a second implementation of lobby semantics.

use std::io::{self, BufRead, Write};
use std::process;
use std::thread;
use std::time::{Duration, Instant};

use don_crossplay::abi::{SESSION_STATUS_STARTED, SESSION_STATUS_STARTING, VISIBILITY_PUBLIC};
use don_crossplay::directory_rpc::DirectoryRpcClient;
use don_crossplay::{AsyncDirectory, Attributes, Backend, Emission, Outcome, ReqId};

fn main() {
    if let Err(error) = run() {
        eprintln!("directory-rpc-peer: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("host") if args.len() == 2 => host(),
        Some("join") if args.len() == 4 => join(&args[2], &args[3]),
        _ => Err("usage: directory-rpc-peer host | join ADDRESS LOBBY_ID".to_string()),
    }
}

fn host() -> Result<(), String> {
    let mut directory = DirectoryRpcClient::listen("127.0.0.1:0")
        .map_err(|error| format!("could not listen: {error}"))?;
    let address = directory
        .local_addr()
        .ok_or_else(|| "listening client has no local address".to_string())?;
    let mut service = started("host", "Host", &mut directory)?;

    let mut attributes = Attributes::new();
    attributes.insert("game_seed".to_string(), "3735928559".to_string());
    let request = service.create_lobby(4, VISIBILITY_PUBLIC, attributes);
    let outcome = complete_later(&mut service, &mut directory, request)?;
    let lobby = match outcome {
        Outcome::Lobby(lobby) => lobby,
        other => return Err(format!("CreateLobby returned {other:?}")),
    };
    println!("LOBBY {address} {}", lobby.id);
    io::stdout().flush().map_err(|error| error.to_string())?;

    // Keep the authority and host service alive until the parent confirms the
    // independent join process completed.
    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|error| error.to_string())?;
    if line.trim() != "done" {
        return Err("host did not receive the completion marker".to_string());
    }
    Ok(())
}

fn join(address: &str, lobby_id: &str) -> Result<(), String> {
    let address: std::net::SocketAddr = address
        .parse()
        .map_err(|error| format!("invalid address: {error}"))?;
    let mut directory = DirectoryRpcClient::connect(address)
        .map_err(|error| format!("could not connect: {error}"))?;
    let mut service = started("peer", "Peer", &mut directory)?;

    let request = service.find_lobbies(16, 1);
    let outcome = complete_later(&mut service, &mut directory, request)?;
    let found = match outcome {
        Outcome::Lobbies(lobbies) => lobbies,
        other => return Err(format!("FindLobbies returned {other:?}")),
    };
    if !found.iter().any(|lobby| lobby.id == lobby_id) {
        return Err(format!(
            "FindLobbies did not observe host lobby {lobby_id:?}: {found:?}"
        ));
    }

    let request = service.join_lobby(lobby_id);
    let outcome = complete_later(&mut service, &mut directory, request)?;
    let joined = match outcome {
        Outcome::Lobby(lobby) => lobby,
        other => return Err(format!("JoinLobby returned {other:?}")),
    };
    let ids: Vec<&str> = joined
        .members
        .iter()
        .map(|member| member.user_id.as_str())
        .collect();
    if ids != ["host", "peer"] {
        return Err(format!("joined roster did not converge: {ids:?}"));
    }
    println!("JOINED {} {}", joined.id, joined.members.len());
    Ok(())
}

fn started(id: &str, name: &str, directory: &mut dyn AsyncDirectory) -> Result<Backend, String> {
    let mut service = Backend::new(id, name);
    service.init();
    let request = service.start_session(id);
    if service.session_status() != SESSION_STATUS_STARTING {
        return Err("StartSession did not synchronously enter Starting".to_string());
    }
    let outcome = complete_later(&mut service, directory, request)?;
    if !matches!(outcome, Outcome::SessionStarted(ref user) if user == id) {
        return Err(format!("StartSession returned {outcome:?}"));
    }
    if service.session_status() != SESSION_STATUS_STARTED {
        return Err("StartSession completion did not enter Started".to_string());
    }
    Ok(service)
}

fn complete_later(
    service: &mut Backend,
    directory: &mut dyn AsyncDirectory,
    request: ReqId,
) -> Result<Outcome, String> {
    // The first Tick may submit this request, but must never complete it: the
    // network worker owns the socket and answers only through a later poll.
    if let Some(outcome) = completion(service.tick_remote(directory), request)? {
        return Err(format!(
            "request completed on its submission Tick: {outcome:?}"
        ));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(outcome) = completion(service.tick_remote(directory), request)? {
            return Ok(outcome);
        }
        thread::sleep(Duration::from_millis(1));
    }
    Err(format!("request {request:?} did not complete"))
}

fn completion(emissions: Vec<Emission>, request: ReqId) -> Result<Option<Outcome>, String> {
    let mut completion = None;
    let mut completion_index = None;
    let mut first_notice = None;
    for (index, emission) in emissions.into_iter().enumerate() {
        match emission {
            Emission::Completion(id, outcome) if id == request => {
                completion = Some(outcome);
                completion_index = Some(index);
            }
            Emission::Notice(notice) if first_notice.is_none() => {
                first_notice = Some((index, notice))
            }
            _ => {}
        }
    }
    if let (Some(completed_at), Some((noticed_at, notice))) = (completion_index, first_notice) {
        if noticed_at < completed_at {
            return Err(format!(
                "directory notice preceded same-Tick request completion: {notice:?}"
            ));
        }
    }
    Ok(completion)
}
