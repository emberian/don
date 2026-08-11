//! `donnet-peer` — a headless lockstep peer over TCP.
//!
//! Two of these hand each other turns: they discover, agree a roster, run the
//! readiness handshake, and then exchange one `NETMSG_COMMANDPACKAGEDATA` per
//! turn, refusing to advance until every peer's package for that turn is in
//! hand. The transport is plain TCP with a length prefix, so it works on
//! loopback, on a LAN, and across the internet with one forwarded port — the
//! host binds whatever address you give it.
//!
//! ```text
//! donnet-peer host --bind 0.0.0.0:31337 --id 1 --turns 20
//! donnet-peer join --addr 203.0.113.7:31337 --id 2 --turns 20
//! ```
//!
//! It prints one JSON object per line so a harness can diff two runs.

use don_net::session::{Event, Role, Session};
use don_net::transport::TcpTransport;
use don_net::{LocalMatch, LocalMatchPhase};
use std::time::{Duration, Instant};

fn local_error(error: don_net::LocalMatchError) -> std::io::Error {
    std::io::Error::other(error)
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("");
    let id: i32 = arg(&args, "--id").and_then(|v| v.parse().ok()).unwrap_or(1);
    let turns: u32 = arg(&args, "--turns")
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let expect_peers: usize = arg(&args, "--peers")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let name = arg(&args, "--name").unwrap_or_else(|| format!("peer{id}"));

    let (transport, role) = match mode {
        "host" => {
            let bind = arg(&args, "--bind").unwrap_or_else(|| "127.0.0.1:0".into());
            (TcpTransport::host(id, bind.as_str())?, Role::Host)
        }
        "join" => {
            let addr = arg(&args, "--addr").expect("join needs --addr HOST:PORT");
            (TcpTransport::join(id, addr.as_str())?, Role::Client)
        }
        _ => {
            eprintln!(
                "usage:\n  donnet-peer host --bind ADDR [--id N] [--turns N] [--peers N]\n  \
                 donnet-peer join --addr ADDR [--id N] [--turns N] [--peers N]"
            );
            std::process::exit(2);
        }
    };
    let addr = transport.local_addr()?;
    println!(r#"{{"event":"listening","id":{id},"addr":"{addr}"}}"#);

    let mut match_ = LocalMatch::new(Session::new(transport, role, name));
    let start = Instant::now();
    let now = |start: Instant| start.elapsed().as_millis() as u64;

    // 1. wait for the roster to fill
    while match_.session().players().len() < expect_peers + 1 {
        match_
            .poll(now(start), Duration::from_millis(10))
            .map_err(local_error)?;
        for e in match_.session_mut().drain_events() {
            if let Event::PlayerJoined(p) = e {
                println!(r#"{{"event":"joined","peer":{p}}}"#);
            }
        }
        if start.elapsed() > Duration::from_secs(30) {
            eprintln!("timed out waiting for peers");
            std::process::exit(1);
        }
    }
    println!(
        r#"{{"event":"roster","players":{}}}"#,
        match_.session().players().len()
    );

    // 2. readiness handshake
    match_.set_ready(true).map_err(local_error)?;
    while !match_.session().all_ready() {
        match_
            .poll(now(start), Duration::from_millis(10))
            .map_err(local_error)?;
        match_.session_mut().drain_events();
        if start.elapsed() > Duration::from_secs(30) {
            eprintln!("timed out waiting for ready");
            std::process::exit(1);
        }
    }
    println!(
        r#"{{"event":"all_ready","t_ms":{}}}"#,
        start.elapsed().as_millis()
    );

    // 3. explicit match start. This is DoN's own local-service transaction,
    // not a recovered retail packet. The host publishes one non-zero attempt
    // epoch and the simulation seed; clients refuse turn submission until
    // they have accepted it through `LocalMatch`.
    const MATCH_EPOCH: u32 = 1;
    const MATCH_SEED: u32 = 0x0d0a_11ce;
    let started = if role == Role::Host {
        match_.start(MATCH_EPOCH, MATCH_SEED).map_err(local_error)?
    } else {
        while !matches!(match_.phase(), LocalMatchPhase::Started(_)) {
            match_
                .poll(now(start), Duration::from_millis(10))
                .map_err(local_error)?;
            match_.session_mut().drain_events();
            if start.elapsed() > Duration::from_secs(30) {
                eprintln!("timed out waiting for match start");
                std::process::exit(1);
            }
        }
        let LocalMatchPhase::Started(started) = match_.phase() else {
            unreachable!()
        };
        started
    };
    println!(
        r#"{{"event":"match_started","from":{},"epoch":{},"seed":{}}}"#,
        started.from, started.epoch, started.seed
    );

    // 4. lockstep turn loop. The payload is a real CheckSumsCommand (opcode
    //    0x39, 65 bytes) so the bytes on the wire are a shape the engine's own
    //    decoder accepts; the values are ours.
    let slot = match_
        .session()
        .players()
        .iter()
        .position(|p| p.is_local)
        .unwrap_or(0) as i8;
    let mut hash: u64 = 0;
    for stamp in 0..turns {
        let mut payload = vec![0x39u8];
        for ch in 0..16u32 {
            payload.extend_from_slice(&(stamp.wrapping_mul(31).wrapping_add(ch)).to_le_bytes());
        }
        match_
            .send_turn(stamp, slot, &payload)
            .map_err(local_error)?;

        let t0 = Instant::now();
        while !match_.turn_ready(stamp) {
            match_
                .poll(now(start), Duration::from_millis(5))
                .map_err(local_error)?;
            match_.session_mut().drain_events();
            if t0.elapsed() > Duration::from_secs(20) {
                eprintln!("turn {stamp}: timed out waiting for packages");
                std::process::exit(1);
            }
        }
        let pkgs = match_.take_turn(stamp);
        // Fold every package into a running hash. Two peers that agree on the
        // turn stream end on the same number; that is the whole assertion.
        for p in &pkgs {
            hash = hash.wrapping_mul(1_000_003).wrapping_add(p.stamp as u64);
            hash = hash.wrapping_mul(1_000_003).wrapping_add(p.play as u64);
            for b in &p.payload {
                hash = hash.wrapping_mul(1_000_003).wrapping_add(*b as u64);
            }
        }
        println!(
            r#"{{"event":"turn","stamp":{stamp},"packages":{},"hash":"{hash:016x}"}}"#,
            pkgs.len()
        );
    }
    println!(
        r#"{{"event":"done","id":{id},"turns":{turns},"final_hash":"{hash:016x}","ms":{}}}"#,
        start.elapsed().as_millis()
    );
    Ok(())
}
