use don_net::{LocalMatch, LocalMatchError, LocalMatchPhase, Role, Session, TcpTransport};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const HOST_ID: i32 = 101;
const CLIENT_ID: i32 = 202;
const EPOCH: u32 = 7;
const SEED: u32 = 0x0d0a_11ce;

#[derive(Debug)]
struct PeerResult {
    role: Role,
    started: don_net::AnnouncedMatchStart,
    packages: Vec<don_net::TurnPackage>,
}

fn checksum_payload() -> Vec<u8> {
    let mut payload = vec![0x39];
    for channel in 0..16u32 {
        payload.extend_from_slice(&SEED.wrapping_add(channel).to_le_bytes());
    }
    payload
}

fn wait_until(
    match_: &mut LocalMatch<TcpTransport>,
    start: Instant,
    predicate: impl Fn(&LocalMatch<TcpTransport>) -> bool,
) -> Result<(), String> {
    while !predicate(match_) {
        match_
            .poll(start.elapsed().as_millis() as u64, Duration::from_millis(5))
            .map_err(|error| error.to_string())?;
        if start.elapsed() > Duration::from_secs(10) {
            return Err(format!(
                "local lifecycle timeout: role={:?} phase={:?} players={:?}",
                match_.role(),
                match_.phase(),
                match_.session().players()
            ));
        }
    }
    Ok(())
}

fn run_host(transport: TcpTransport) -> Result<PeerResult, String> {
    let start = Instant::now();
    let mut match_ = LocalMatch::new(Session::new(transport, Role::Host, "host"));
    wait_until(&mut match_, start, |m| m.session().players().len() == 2)?;
    match_.set_ready(true).map_err(|error| error.to_string())?;
    wait_until(&mut match_, start, |m| {
        m.phase() == LocalMatchPhase::AllReady
    })?;

    assert!(matches!(
        match_.send_turn(0, 0, &checksum_payload()),
        Err(LocalMatchError::TurnBeforeStart)
    ));
    let started = match_
        .start(EPOCH, SEED)
        .map_err(|error| error.to_string())?;
    match_
        .send_turn(0, 0, &checksum_payload())
        .map_err(|error| error.to_string())?;
    wait_until(&mut match_, start, |m| m.turn_ready(0))?;
    Ok(PeerResult {
        role: Role::Host,
        started,
        packages: match_.take_turn(0),
    })
}

fn run_client(transport: TcpTransport) -> Result<PeerResult, String> {
    let start = Instant::now();
    let mut match_ = LocalMatch::new(Session::new(transport, Role::Client, "client"));
    wait_until(&mut match_, start, |m| m.session().players().len() == 2)?;
    match_.set_ready(true).map_err(|error| error.to_string())?;
    wait_until(&mut match_, start, |m| {
        matches!(m.phase(), LocalMatchPhase::Started(_))
    })?;
    let LocalMatchPhase::Started(started) = match_.phase() else {
        unreachable!()
    };
    match_
        .send_turn(0, 1, &checksum_payload())
        .map_err(|error| error.to_string())?;
    wait_until(&mut match_, start, |m| m.turn_ready(0))?;
    Ok(PeerResult {
        role: Role::Client,
        started,
        packages: match_.take_turn(0),
    })
}

#[test]
fn tcp_peers_host_join_ready_start_and_exchange_the_first_turn_locally() {
    let host_transport = TcpTransport::host(HOST_ID, "127.0.0.1:0").unwrap();
    let address = host_transport.local_addr().unwrap();
    assert!(address.ip().is_loopback());
    let client_transport = TcpTransport::join(CLIENT_ID, address).unwrap();

    let (tx, rx) = mpsc::channel();
    let host_tx = tx.clone();
    let host = std::thread::spawn(move || {
        let _ = host_tx.send(run_host(host_transport));
    });
    let client = std::thread::spawn(move || {
        let _ = tx.send(run_client(client_transport));
    });

    let first = rx.recv_timeout(Duration::from_secs(15)).unwrap().unwrap();
    let second = rx.recv_timeout(Duration::from_secs(15)).unwrap().unwrap();
    host.join().unwrap();
    client.join().unwrap();

    let (host, client) = if first.role == Role::Host {
        (first, second)
    } else {
        (second, first)
    };
    assert_eq!(host.started, client.started);
    assert_eq!(host.started.from, HOST_ID);
    assert_eq!(host.started.epoch, EPOCH);
    assert_eq!(host.started.seed, SEED);
    assert_eq!(host.packages, client.packages);
    assert_eq!(host.packages.len(), 2);
    assert_eq!(host.packages[0].play, 0);
    assert_eq!(host.packages[1].play, 1);
    assert_eq!(host.packages[0].payload, checksum_payload());
    assert_eq!(host.packages[1].payload, checksum_payload());
}
