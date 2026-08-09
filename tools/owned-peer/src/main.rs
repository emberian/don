use don_net::internal::{InternalPacket, MAX_PLAYERS};
use don_net::lobby::{
    attributes_to_game, attributes_to_player, game_to_attributes, player_to_attributes,
};
use don_net::msg::NetMsg;
use don_net::obfuscate::{rank_xor_keys, xor_payload};
use don_net::session::{Event, Role, Session};
use don_net::setup::{GameConnectionData, PlayerSlotPod};
use don_net::transport::{Dest, TcpTransport, Transport};
use don_net::{decode_commands, encode_commands, CheckSums, Command, Obfuscation};
use std::collections::BTreeSet;
use std::env;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const DEFAULT_TURNS: u32 = 50;
const MAX_TURNS: u32 = 10_000;
const HOST_ID: i32 = 1;
const CLIENT_ID: i32 = 2;
const PEER_NAME: &str = "Ai";
const CHECKSUM_OPCODE: u8 = 0x39;
const CHECKSUM_CHANNELS: usize = 15;
const CHECKSUM_WORDS: usize = 16;
const COMMAND_PACKAGE_WIRE_LEN: usize = 8 + CheckSums::WIRE_LEN;
const DEFAULT_RETAIL_TIMEOUT_SECS: u64 = 120;
const MAX_RETAIL_TIMEOUT_SECS: u64 = 3_600;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Synthetic { turns: u32 },
    Retail(RetailOptions),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RetailOptions {
    addr: String,
    id: i32,
    turns: u32,
    timeout_secs: u64,
    game_key: Option<u32>,
    passive: bool,
}

#[derive(Debug, Clone, Copy)]
struct ShapeProof {
    game_record_bytes: usize,
    game_attribute_keys: usize,
    player_attribute_keys: usize,
    add_player_bytes: usize,
    player_list_bytes: usize,
    ready_flag_bytes: usize,
    command_package_bytes: usize,
}

#[derive(Debug)]
struct PeerReport {
    role: Role,
    local_id: i32,
    roster_ids: Vec<i32>,
    checksum_hash: u64,
    turns: u32,
}

#[derive(Debug)]
struct RetailReport {
    local_id: i32,
    host_id: i32,
    local_slot: usize,
    all_ready_observed: bool,
    packages_seen: u32,
    checksum_turns: u32,
    packages_sent: u32,
    orderly_disconnect_sent: bool,
    transcript_hash: u64,
    game_key: u32,
}

#[derive(Debug)]
struct DecodedTraffic {
    command_count: usize,
    opcodes: Vec<u8>,
    checksum: Option<CheckSums>,
    checksum_bytes: Option<Vec<u8>>,
}

fn main() {
    let mode = match parse_args(env::args().skip(1)) {
        Ok(Some(mode)) => mode,
        Ok(None) => return,
        Err(e) => fail(&e),
    };

    match mode {
        Mode::Synthetic { turns } => match run(turns) {
            Ok((shape, host, client)) => {
            println!(
                "{{\"schema\":\"don.owned-peer.v2\",\"status\":\"pass\",\"mode\":\"synthetic\",\"transport\":\"tcp-loopback\",\"peers\":2,\"peer_name\":\"Ai\",\"turns\":{},\"setup\":{{\"transport\":\"offline PlayFab lobby-attribute roundtrip\",\"game_record_bytes\":{},\"game_attribute_keys\":{},\"player_attribute_keys\":{}}},\"packet_shapes\":{{\"add_player_bytes\":{},\"player_list_bytes\":{},\"ready_flag_bytes\":{},\"command_package_bytes\":{},\"checksum_command_bytes\":{}}},\"checks\":{{\"authoritative_roster\":true,\"all_ready_both_peers\":true,\"one_package_per_peer_per_turn\":true,\"checksum_opcode\":\"0x39\",\"checksum_total_relation\":true,\"checksum_adler32_values\":true,\"peer_turn_hash_equal\":true}},\"turn_hash\":\"{:016x}\",\"retail_friend_slot_occupied\":false,\"credential_material\":\"none\"}}",
                turns,
                shape.game_record_bytes,
                shape.game_attribute_keys,
                shape.player_attribute_keys,
                shape.add_player_bytes,
                shape.player_list_bytes,
                shape.ready_flag_bytes,
                shape.command_package_bytes,
                CheckSums::WIRE_LEN,
                host.checksum_hash,
            );
            let _ = (host.role, host.local_id, host.roster_ids, host.turns);
            let _ = (
                client.role,
                client.local_id,
                client.roster_ids,
                client.turns,
            );
            }
            Err(e) => fail(&e),
        },
        Mode::Retail(options) => match run_retail(&options) {
            Ok(report) => println!(
                "{{\"schema\":\"don.owned-peer.retail.v1\",\"status\":\"pass\",\"mode\":\"retail-connect\",\"transport\":\"replacement-crossplaynetlib-tcp\",\"peer_name\":\"Ai\",\"local_id\":{},\"host_id\":{},\"local_slot\":{},\"all_ready_observed\":{},\"packages_seen\":{},\"checksum_turns\":{},\"packages_sent\":{},\"orderly_disconnect_sent\":{},\"reply_policy\":\"{}\",\"compatible_game_key\":\"0x{:08x}\",\"transcript_hash\":\"{:016x}\",\"credential_material\":\"none\",\"simulation_equivalence_claimed\":false}}",
                report.local_id,
                report.host_id,
                report.local_slot,
                report.all_ready_observed,
                report.packages_seen,
                report.checksum_turns,
                report.packages_sent,
                report.orderly_disconnect_sent,
                if options.passive { "passive" } else { "mirror-retail-checksum" },
                report.game_key,
                report.transcript_hash,
            ),
            Err(e) => fail(&e),
        },
    }
}

fn fail(message: &str) -> ! {
    let escaped = message.replace('\\', "\\\\").replace('"', "\\\"");
    eprintln!(
        "{{\"schema\":\"don.owned-peer.v2\",\"status\":\"fail\",\"error\":\"{}\",\"credential_material\":\"none\"}}",
        escaped
    );
    std::process::exit(1)
}

fn parse_args<I>(mut args: I) -> Result<Option<Mode>, String>
where
    I: Iterator<Item = String>,
{
    let mut turns = DEFAULT_TURNS;
    let mut retail_addr = None;
    let mut id = CLIENT_ID;
    let mut timeout_secs = DEFAULT_RETAIL_TIMEOUT_SECS;
    let mut game_key = None;
    let mut passive = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--turns" => {
                let raw = args.next().ok_or("--turns requires a value")?;
                turns = raw
                    .parse::<u32>()
                    .map_err(|_| format!("invalid --turns value: {raw}"))?;
            }
            "--retail-connect" => {
                retail_addr = Some(args.next().ok_or("--retail-connect requires HOST:PORT")?);
            }
            "--id" => {
                let raw = args.next().ok_or("--id requires a value")?;
                id = raw
                    .parse::<i32>()
                    .map_err(|_| format!("invalid --id value: {raw}"))?;
            }
            "--timeout-secs" => {
                let raw = args.next().ok_or("--timeout-secs requires a value")?;
                timeout_secs = raw
                    .parse::<u64>()
                    .map_err(|_| format!("invalid --timeout-secs value: {raw}"))?;
            }
            "--game-key" => {
                let raw = args.next().ok_or("--game-key requires a value")?;
                game_key = Some(parse_u32(&raw)?);
            }
            "--passive" => passive = true,
            "-h" | "--help" => {
                println!(
                    "Usage:\n  don-owned-peer [--turns N]\n  don-owned-peer --retail-connect HOST:PORT [--id N] [--turns N] [--timeout-secs N] [--game-key 0xG] [--passive]\n\nWithout --retail-connect, runs the two-owned-peer TCP loopback acceptance. Retail mode directly joins only the supplied replacement-CrossplayNetLib TCP endpoint as Ai; it carries no authentication material. By default it recovers the multiplayer package key and returns a checksum-only package for each observed retail turn. --passive reports traffic without returning turn packages."
                );
                return Ok(None);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    if !(1..=MAX_TURNS).contains(&turns) {
        return Err(format!("--turns must be in 1..={MAX_TURNS}"));
    }
    if !(1..=MAX_RETAIL_TIMEOUT_SECS).contains(&timeout_secs) {
        return Err(format!(
            "--timeout-secs must be in 1..={MAX_RETAIL_TIMEOUT_SECS}"
        ));
    }
    match retail_addr {
        Some(addr) => {
            if addr.trim().is_empty() || !addr.contains(':') {
                return Err("--retail-connect must be HOST:PORT".into());
            }
            if id == 0 {
                return Err("--id must be non-zero".into());
            }
            Ok(Some(Mode::Retail(RetailOptions {
                addr,
                id,
                turns,
                timeout_secs,
                game_key,
                passive,
            })))
        }
        None => {
            if id != CLIENT_ID || game_key.is_some() || passive {
                return Err("--id, --game-key, and --passive require --retail-connect".into());
            }
            Ok(Some(Mode::Synthetic { turns }))
        }
    }
}

fn parse_u32(raw: &str) -> Result<u32, String> {
    let (digits, radix) = raw
        .strip_prefix("0x")
        .or_else(|| raw.strip_prefix("0X"))
        .map(|digits| (digits, 16))
        .unwrap_or((raw, 10));
    u32::from_str_radix(digits, radix).map_err(|_| format!("invalid u32 value: {raw}"))
}

fn run_retail(options: &RetailOptions) -> Result<RetailReport, String> {
    prove_shapes()?;
    let start = Instant::now();
    let deadline = Duration::from_secs(options.timeout_secs);
    let transport = bounded_join(options.id, options.addr.clone(), deadline)?;
    let mut session = Session::new(transport, Role::Client, PEER_NAME);
    let now = || start.elapsed().as_millis() as u64;
    let mut roster_announced = false;
    let mut ready_sent = false;
    let mut all_ready_observed = false;
    let mut host_id = 0;
    let mut local_slot = usize::MAX;
    let mut packages_seen = 0u32;
    let mut checksum_turns = 0u32;
    let mut packages_sent = 0u32;
    let mut transcript_hash = 0xcbf2_9ce4_8422_2325u64;
    let mut game_key = options.game_key;
    let mut key_samples = Vec::<Vec<u8>>::new();
    let mut seen_packages = BTreeSet::<(i32, u32, i8)>::new();

    println!(
        "{{\"schema\":\"don.owned-peer.retail.event.v1\",\"event\":\"connected\",\"peer_name\":\"Ai\",\"local_id\":{},\"endpoint\":\"{}\",\"credential_material\":\"none\"}}",
        options.id,
        json_escape(&options.addr),
    );

    loop {
        session
            .poll(now(), Duration::from_millis(10))
            .map_err(|e| format!("retail session poll: {e}"))?;

        if let Some((host, slot)) = authoritative_retail_roster(&session, options.id)? {
            host_id = host;
            local_slot = slot;
            if !roster_announced {
                println!(
                    "{{\"schema\":\"don.owned-peer.retail.event.v1\",\"event\":\"roster\",\"host_id\":{},\"host_slot\":0,\"local_id\":{},\"local_slot\":{},\"members\":2,\"peer_name\":\"Ai\"}}",
                    host_id, options.id, local_slot,
                );
                roster_announced = true;
            }
            if !ready_sent {
                session
                    .send_ready_flag(true)
                    .map_err(|e| format!("send retail ready flag: {e}"))?;
                ready_sent = true;
                println!(
                    "{{\"schema\":\"don.owned-peer.retail.event.v1\",\"event\":\"ready-sent\",\"local_id\":{},\"ready\":true}}",
                    options.id,
                );
            }
        }

        if session.all_ready() && roster_announced && !all_ready_observed {
            all_ready_observed = true;
            println!(
                "{{\"schema\":\"don.owned-peer.retail.event.v1\",\"event\":\"all-ready\",\"members\":2}}"
            );
        }

        let events = session.drain_events();
        for event in events {
            let Event::Game { from, msg } = event else {
                continue;
            };
            if !all_ready_observed {
                return Err(
                    "refusing game traffic before the authoritative all-ready transition".into(),
                );
            }
            if host_id == 0 || from != host_id {
                return Err(format!(
                    "refusing game traffic from non-host peer {from}; expected owned retail host {host_id}"
                ));
            }
            let framed = msg
                .decode()
                .map_err(|e| format!("decode retail game packet from {from}: {e}"))?;
            let NetMsg::CommandPackage {
                stamp,
                play,
                payload,
            } = framed.msg
            else {
                println!(
                    "{{\"schema\":\"don.owned-peer.retail.event.v1\",\"event\":\"game-message\",\"from\":{},\"id\":{},\"bytes\":{}}}",
                    from,
                    framed.ty.id,
                    msg.bytes.len(),
                );
                continue;
            };
            if play != 0 {
                return Err(format!(
                    "retail host {from} sent command package for unexpected slot {play}"
                ));
            }
            if !seen_packages.insert((from, stamp, play)) {
                continue;
            }
            packages_seen = packages_seen.saturating_add(1);
            key_samples.push(payload.to_vec());
            if key_samples.len() > 8 {
                key_samples.remove(0);
            }
            if game_key.is_none() {
                game_key = recover_game_key(&key_samples);
                if let Some(key) = game_key {
                    println!(
                        "{{\"schema\":\"don.owned-peer.retail.event.v1\",\"event\":\"game-key-recovered\",\"compatible_game_key\":\"0x{key:08x}\",\"xor_key\":\"0x{:04x}\"}}",
                        Obfuscation::xor_key(key),
                    );
                }
            }

            let Some(key) = game_key else {
                return Err(format!(
                    "could not recover the first retail command package key at stamp {stamp}; refusing to skip the authoritative first turn (supply --game-key)"
                ));
            };
            let traffic = decode_traffic(payload, key)
                .map_err(|e| format!("decode retail turn {stamp} with key 0x{key:08x}: {e}"))?;
            let Some(sums) = traffic.checksum else {
                return Err(format!(
                    "first retail command package at stamp {stamp} has no checksum command; refusing to synthesize or skip its slot-1 reply"
                ));
            };
            let checksum_bytes = traffic
                .checksum_bytes
                .as_deref()
                .ok_or("decoded checksum lost its command bytes")?;
            checksum_turns = checksum_turns.saturating_add(1);
            hash_bytes(&mut transcript_hash, &stamp.to_le_bytes());
            hash_bytes(&mut transcript_hash, &[play as u8]);
            hash_bytes(&mut transcript_hash, checksum_bytes);
            println!(
                "{{\"schema\":\"don.owned-peer.retail.event.v1\",\"event\":\"turn\",\"stamp\":{},\"play\":{},\"payload_bytes\":{},\"commands\":{},\"opcodes\":{},\"checksum_decoded\":true,\"checksums\":{}}}",
                stamp,
                play,
                payload.len(),
                traffic.command_count,
                json_u8_array(&traffic.opcodes),
                json_checksum_array(&sums),
            );

            if !options.passive {
                let reply = encode_checksum_only(checksum_bytes, key)?;
                session
                    .send_command_package(stamp, local_slot as i8, &reply)
                    .map_err(|e| format!("send checksum-only package for turn {stamp}: {e}"))?;
                packages_sent = packages_sent.saturating_add(1);
                println!(
                    "{{\"schema\":\"don.owned-peer.retail.event.v1\",\"event\":\"turn-sent\",\"stamp\":{},\"play\":{},\"payload_bytes\":{},\"policy\":\"mirror-retail-checksum\",\"simulation_equivalence_claimed\":false}}",
                    stamp,
                    local_slot,
                    reply.len(),
                );
            }
        }

        let enough =
            checksum_turns >= options.turns && (options.passive || packages_sent >= options.turns);
        if enough {
            let mut destroy = Vec::new();
            InternalPacket::DestroyPlayer {
                unique_id: options.id,
            }
            .encode(&mut destroy);
            session
                .transport
                .send(Dest::All, &destroy)
                .map_err(|e| format!("send orderly destroy-player: {e}"))?;
            println!(
                "{{\"schema\":\"don.owned-peer.retail.event.v1\",\"event\":\"disconnect-sent\",\"local_id\":{},\"packet\":\"IPT_DESTROYPLAYER\",\"bytes\":{}}}",
                options.id,
                destroy.len(),
            );
            return Ok(RetailReport {
                local_id: options.id,
                host_id,
                local_slot,
                all_ready_observed,
                packages_seen,
                checksum_turns,
                packages_sent,
                orderly_disconnect_sent: true,
                transcript_hash,
                game_key: game_key.expect("checksum traffic requires a key"),
            });
        }
        if start.elapsed() >= deadline {
            return Err(format!(
                "retail-connect timeout after {}s: roster={} ready_sent={} all_ready={} packages_seen={} checksum_turns={} packages_sent={} key={}",
                options.timeout_secs,
                roster_announced,
                ready_sent,
                all_ready_observed,
                packages_seen,
                checksum_turns,
                packages_sent,
                game_key
                    .map(|key| format!("0x{key:08x}"))
                    .unwrap_or_else(|| "unrecovered (supply --game-key)".into()),
            ));
        }
    }
}

fn bounded_join(id: i32, addr: String, timeout: Duration) -> Result<TcpTransport, String> {
    let rendered = addr.clone();
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = tx.send(TcpTransport::join(id, addr.as_str()));
    });
    rx.recv_timeout(timeout)
        .map_err(|_| format!("connect to replacement CrossplayNetLib at {rendered}: timeout"))?
        .map_err(|e| format!("connect to replacement CrossplayNetLib at {rendered}: {e}"))
}

fn authoritative_retail_roster(
    session: &Session<TcpTransport>,
    local_id: i32,
) -> Result<Option<(i32, usize)>, String> {
    let players = session.players();
    if players.len() > 2 {
        return Err(format!(
            "refusing roster with {} members; retail-connect is scoped to one owned host and this owned peer",
            players.len()
        ));
    }
    if players.len() != 2 {
        return Ok(None);
    }
    let Some(local) = players.iter().find(|player| player.is_local) else {
        return Err("authoritative roster has no local member".into());
    };
    let Some(host) = players
        .iter()
        .find(|player| player.is_host && !player.is_local)
    else {
        return Ok(None);
    };
    if local.unique_id != local_id || local.slot != 1 || host.slot != 0 {
        return Ok(None);
    }
    if local.name != PEER_NAME || host.name != PEER_NAME {
        return Err(format!(
            "retail roster name mismatch: host={:?} local={:?}; expected both Ai",
            host.name, local.name
        ));
    }
    if host.unique_id == 0 || host.unique_id == local_id {
        return Err(format!(
            "retail host id {} is invalid for local id {local_id}",
            host.unique_id
        ));
    }
    Ok(Some((host.unique_id, local.slot)))
}

fn decode_traffic(payload: &[u8], game_key: u32) -> Result<DecodedTraffic, String> {
    let mut plain = payload.to_vec();
    xor_payload(&mut plain, Obfuscation::xor_key(game_key));
    let mut obfuscation = Obfuscation::with_seed(game_key);
    let commands = decode_commands(&plain, &mut obfuscation).map_err(|e| e.to_string())?;
    let mut checksum = None;
    let mut checksum_bytes = None;
    let opcodes = commands.iter().map(|command| command.opcode).collect();
    for command in &commands {
        if command.opcode != CHECKSUM_OPCODE {
            continue;
        }
        validate_checksum_command(command.bytes)?;
        if checksum.is_some() {
            return Err("command package carries more than one checksum command".into());
        }
        checksum = CheckSums::decode(command);
        checksum_bytes = Some(command.bytes.to_vec());
    }
    Ok(DecodedTraffic {
        command_count: commands.len(),
        opcodes,
        checksum,
        checksum_bytes,
    })
}

fn recover_game_key(payloads: &[Vec<u8>]) -> Option<u32> {
    if payloads.is_empty() {
        return None;
    }
    let refs: Vec<&[u8]> = payloads.iter().map(Vec::as_slice).collect();
    let mut candidates = Vec::<u16>::new();
    let mut seen = BTreeSet::<u16>::new();
    let mut add = |key: u16| {
        if seen.insert(key) {
            candidates.push(key);
        }
    };

    // Zero-heavy command structs make the real XOR key a frequent ciphertext
    // word. Keep the frequency order because this normally succeeds first.
    for key in rank_xor_keys(refs.iter().copied(), usize::MAX) {
        add(key);
    }
    // A sparse checksum-only package need not contain a zero word. Its first
    // ciphertext word still reveals the key once we enumerate the first
    // command opcode and its adjacent byte.
    if let Some(first) = payloads.first().filter(|p| p.len() >= 2) {
        let cipher = u16::from_le_bytes([first[0], first[1]]);
        for opcode in [0x4a_u8, 0x48, 0x4f, CHECKSUM_OPCODE, 0x01, 0x00] {
            for adjacent in 0u16..=255 {
                add(cipher ^ ((adjacent << 8) | u16::from(opcode)));
            }
        }
    }

    for xor_key in candidates {
        for low in 0u32..=255 {
            let game_key = (u32::from(xor_key) << 8) | low;
            let decoded: Option<Vec<DecodedTraffic>> = payloads
                .iter()
                .map(|payload| decode_traffic(payload, game_key).ok())
                .collect();
            let Some(decoded) = decoded else { continue };
            if decoded.iter().any(|traffic| traffic.checksum.is_some()) {
                return Some(game_key);
            }
        }
    }
    None
}

fn encode_checksum_only(checksum_bytes: &[u8], game_key: u32) -> Result<Vec<u8>, String> {
    validate_checksum_command(checksum_bytes)?;
    let command = Command {
        opcode: CHECKSUM_OPCODE,
        bytes: checksum_bytes,
    };
    let mut payload = Vec::new();
    let mut obfuscation = Obfuscation::with_seed(game_key);
    encode_commands(&[command], &mut obfuscation, &mut payload);
    xor_payload(&mut payload, Obfuscation::xor_key(game_key));
    Ok(payload)
}

fn json_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn json_u8_array(values: &[u8]) -> String {
    let body = values
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    format!("[{body}]")
}

fn json_checksum_array(sums: &CheckSums) -> String {
    let body = sums
        .0
        .iter()
        .map(|value| format!("\"0x{value:08x}\""))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{body}]")
}

fn run(turns: u32) -> Result<(ShapeProof, PeerReport, PeerReport), String> {
    let shape = prove_shapes()?;
    let host_transport = TcpTransport::host(HOST_ID, "127.0.0.1:0")
        .map_err(|e| format!("bind loopback host: {e}"))?;
    let addr = host_transport
        .local_addr()
        .map_err(|e| format!("read loopback address: {e}"))?;
    if !addr.ip().is_loopback() {
        return Err(format!("refusing non-loopback bind: {addr}"));
    }

    let host = Session::new(host_transport, Role::Host, PEER_NAME);
    let (tx, rx) = mpsc::channel::<Result<PeerReport, String>>();
    let host_tx = tx.clone();
    let host_thread = std::thread::spawn(move || {
        let result = run_peer(host, 0, turns);
        let _ = host_tx.send(result);
    });
    let client_thread = std::thread::spawn(move || {
        let result = TcpTransport::join(CLIENT_ID, addr)
            .map_err(|e| format!("join loopback host: {e}"))
            .and_then(|transport| {
                run_peer(Session::new(transport, Role::Client, PEER_NAME), 1, turns)
            });
        let _ = tx.send(result);
    });

    let first = rx
        .recv_timeout(Duration::from_secs(30))
        .map_err(|e| format!("first peer result timeout: {e}"))??;
    let second = rx
        .recv_timeout(Duration::from_secs(30))
        .map_err(|e| format!("second peer result timeout: {e}"))??;
    host_thread
        .join()
        .map_err(|_| "host peer thread panicked".to_string())?;
    client_thread
        .join()
        .map_err(|_| "client peer thread panicked".to_string())?;

    let (host, client) = if first.role == Role::Host {
        (first, second)
    } else {
        (second, first)
    };
    if host.role != Role::Host || client.role != Role::Client {
        return Err("peer result roles are not one host and one client".into());
    }
    if host.local_id != HOST_ID || client.local_id != CLIENT_ID {
        return Err("peer result local ids changed".into());
    }
    if host.roster_ids != [HOST_ID, CLIENT_ID] || client.roster_ids != [HOST_ID, CLIENT_ID] {
        return Err(format!(
            "authoritative roster mismatch: host={:?} client={:?}",
            host.roster_ids, client.roster_ids
        ));
    }
    if host.checksum_hash == 0 || host.checksum_hash != client.checksum_hash {
        return Err(format!(
            "turn stream mismatch: host={:016x} client={:016x}",
            host.checksum_hash, client.checksum_hash
        ));
    }
    if host.turns != turns || client.turns != turns {
        return Err("a peer did not complete every requested turn".into());
    }
    Ok((shape, host, client))
}

fn prove_shapes() -> Result<ShapeProof, String> {
    let game = GameConnectionData {
        players: 2,
        max_observers: 0,
        game_speed: 2,
        seed: 0x0D0A_11CE,
        flags: 4,
        checksum_window_size: 64,
        checksum_deep: 1,
        checksum_failure_threshold: 1,
        ..Default::default()
    };
    let mut game_bytes = Vec::new();
    game.encode(&mut game_bytes);
    if game_bytes.len() != GameConnectionData::WIRE_LEN
        || GameConnectionData::decode(&game_bytes) != Some(game)
    {
        return Err("46-byte GameConnectionData did not round-trip".into());
    }

    // Retail setup in this build is carried by PlayFab lobby attributes, not
    // the dead NetMsg ids 1..4. Prove only the fields that schema transports.
    let game_attributes = game_to_attributes(&game);
    let mut game_back = GameConnectionData::default();
    attributes_to_game(&game_attributes, &mut game_back);
    if game_back.game_speed != game.game_speed
        || game_back.seed != game.seed
        || game_back.flags != game.flags
    {
        return Err("retail lobby game attributes did not round-trip".into());
    }
    let player = PlayerSlotPod {
        slot_type: 1,
        who: 1,
        team: 1,
        ready: 1,
        ..Default::default()
    };
    let player_attributes = player_to_attributes(1, &player);
    let mut player_back = PlayerSlotPod::default();
    attributes_to_player(&player_attributes, 1, &mut player_back);
    if player_back != player {
        return Err("retail lobby player attributes did not round-trip".into());
    }

    let add_player = InternalPacket::AddPlayer {
        player_name: PEER_NAME.into(),
        unique_id: CLIENT_ID,
        is_hosting: false,
    };
    let mut ids = [0i32; MAX_PLAYERS];
    ids[0] = HOST_ID;
    ids[1] = CLIENT_ID;
    let player_list = InternalPacket::PlayerList {
        num_players: 2,
        unique_ids: ids,
    };
    let ready = InternalPacket::ReadyFlag { ready: true };
    let add_player_bytes = round_trip_internal(&add_player)?;
    let player_list_bytes = round_trip_internal(&player_list)?;
    let ready_flag_bytes = round_trip_internal(&ready)?;
    if (add_player_bytes, player_list_bytes, ready_flag_bytes) != (70, 34, 2) {
        return Err("PDB-derived internal packet sizes changed".into());
    }

    let payload = checksum_command(0);
    validate_checksum_command(&payload)?;
    let msg = NetMsg::CommandPackage {
        stamp: 0,
        play: 0,
        payload: &payload,
    };
    let mut wire = Vec::new();
    msg.encode(&mut wire);
    let decoded = NetMsg::decode(&wire).map_err(|e| format!("command package decode: {e}"))?;
    match decoded.msg {
        NetMsg::CommandPackage {
            stamp,
            play,
            payload: decoded_payload,
        } if stamp == 0 && play == 0 && decoded_payload == payload => {}
        _ => return Err("command package shape did not round-trip".into()),
    }
    if wire.len() != COMMAND_PACKAGE_WIRE_LEN {
        return Err(format!(
            "command package length: expected {COMMAND_PACKAGE_WIRE_LEN}, got {}",
            wire.len()
        ));
    }

    Ok(ShapeProof {
        game_record_bytes: game_bytes.len(),
        game_attribute_keys: game_attributes.len(),
        player_attribute_keys: player_attributes.len(),
        add_player_bytes,
        player_list_bytes,
        ready_flag_bytes,
        command_package_bytes: wire.len(),
    })
}

fn round_trip_internal(packet: &InternalPacket) -> Result<usize, String> {
    let mut wire = Vec::new();
    packet.encode(&mut wire);
    if wire.len() != packet.wire_len() {
        return Err(format!(
            "internal packet {} encoded {} bytes, expected {}",
            packet.id(),
            wire.len(),
            packet.wire_len()
        ));
    }
    let back = InternalPacket::decode(&wire)
        .map_err(|e| format!("internal packet {} decode: {e}", packet.id()))?;
    if &back != packet {
        return Err(format!(
            "internal packet {} did not round-trip",
            packet.id()
        ));
    }
    Ok(wire.len())
}

fn run_peer(
    mut session: Session<TcpTransport>,
    slot: i8,
    turns: u32,
) -> Result<PeerReport, String> {
    let start = Instant::now();
    let now = || start.elapsed().as_millis() as u64;

    while !roster_is_authoritative(&session) {
        session
            .poll(now(), Duration::from_millis(5))
            .map_err(|e| format!("roster poll: {e}"))?;
        session.drain_events();
        if start.elapsed() > Duration::from_secs(10) {
            return Err(format!("roster timeout: {:?}", session.players()));
        }
    }
    validate_roster(&session)?;

    session
        .send_ready_flag(true)
        .map_err(|e| format!("send ready: {e}"))?;
    while !session.all_ready() {
        session
            .poll(now(), Duration::from_millis(5))
            .map_err(|e| format!("readiness poll: {e}"))?;
        session.drain_events();
        if start.elapsed() > Duration::from_secs(15) {
            return Err(format!("readiness timeout: {:?}", session.players()));
        }
    }

    let mut stream_hash = 0xcbf2_9ce4_8422_2325u64;
    for stamp in 0..turns {
        let payload = checksum_command(stamp);
        validate_checksum_command(&payload)?;
        session
            .send_command_package(stamp, slot, &payload)
            .map_err(|e| format!("send turn {stamp}: {e}"))?;
        while !session.turn_ready(stamp) {
            session
                .poll(now(), Duration::from_millis(5))
                .map_err(|e| format!("turn {stamp} poll: {e}"))?;
            session.drain_events();
            if start.elapsed() > Duration::from_secs(25) {
                return Err(format!("turn {stamp} timeout"));
            }
        }
        let packages = session.take_turn(stamp);
        if packages.len() != 2 {
            return Err(format!(
                "turn {stamp}: expected 2 packages, got {}",
                packages.len()
            ));
        }
        for (expected_slot, package) in packages.iter().enumerate() {
            if package.stamp != stamp || package.play != expected_slot as i8 {
                return Err(format!(
                    "turn {stamp}: package order/stamp mismatch: play={} stamp={}",
                    package.play, package.stamp
                ));
            }
            if package.payload != payload {
                return Err(format!("turn {stamp}: peer checksum payload differs"));
            }
            validate_checksum_command(&package.payload)?;
            hash_bytes(&mut stream_hash, &package.stamp.to_le_bytes());
            hash_bytes(&mut stream_hash, &[package.play as u8]);
            hash_bytes(&mut stream_hash, &package.payload);
        }
    }

    Ok(PeerReport {
        role: session.role,
        local_id: session.local_id(),
        roster_ids: session.players().iter().map(|p| p.unique_id).collect(),
        checksum_hash: stream_hash,
        turns,
    })
}

fn validate_roster(session: &Session<TcpTransport>) -> Result<(), String> {
    let players = session.players();
    if players.len() != 2 {
        return Err(format!("expected 2 roster members, got {}", players.len()));
    }
    if players.iter().map(|p| p.unique_id).collect::<Vec<_>>() != [HOST_ID, CLIENT_ID] {
        return Err(format!("unexpected authoritative roster: {players:?}"));
    }
    if players.iter().map(|p| p.slot).collect::<Vec<_>>() != [0, 1] {
        return Err(format!("unexpected authoritative slots: {players:?}"));
    }
    if players.iter().any(|p| p.name != PEER_NAME) {
        return Err(format!("peer label was not exactly Ai: {players:?}"));
    }
    if players.iter().filter(|p| p.is_host).count() != 1
        || players.iter().filter(|p| p.is_local).count() != 1
    {
        return Err(format!("host/local flags invalid: {players:?}"));
    }
    Ok(())
}

fn roster_is_authoritative(session: &Session<TcpTransport>) -> bool {
    let players = session.players();
    players.len() == 2
        && players[0].unique_id == HOST_ID
        && players[0].slot == 0
        && players[1].unique_id == CLIENT_ID
        && players[1].slot == 1
        && players.iter().all(|p| p.name == PEER_NAME)
}

fn checksum_command(stamp: u32) -> Vec<u8> {
    let mut words = [0u32; CHECKSUM_WORDS];
    for (channel, word) in words.iter_mut().take(CHECKSUM_CHANNELS).enumerate() {
        let mut preimage = [0u8; 12];
        preimage[0..4].copy_from_slice(&stamp.to_le_bytes());
        preimage[4..8].copy_from_slice(&(channel as u32).to_le_bytes());
        preimage[8..12].copy_from_slice(&(stamp ^ (channel as u32 * 0x0101_0101)).to_le_bytes());
        *word = adler32(&preimage);
    }
    words[CHECKSUM_CHANNELS] = words[..CHECKSUM_CHANNELS]
        .iter()
        .fold(0u32, |sum, value| sum.wrapping_add(*value));

    let mut bytes = Vec::with_capacity(CheckSums::WIRE_LEN);
    bytes.push(CHECKSUM_OPCODE);
    for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes
}

fn validate_checksum_command(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() != CheckSums::WIRE_LEN {
        return Err(format!(
            "checksum command is {} bytes, expected 65",
            bytes.len()
        ));
    }
    let command = Command {
        opcode: bytes[0],
        bytes,
    };
    let sums = CheckSums::decode(&command).ok_or("checksum command failed typed decode")?;
    if sums.0[..CHECKSUM_CHANNELS]
        .iter()
        .any(|value| (value & 0xffff) >= 65_521 || (value >> 16) >= 65_521)
    {
        return Err("checksum word is not Adler-32-shaped".into());
    }
    let expected_total = sums.0[..CHECKSUM_CHANNELS]
        .iter()
        .fold(0u32, |sum, value| sum.wrapping_add(*value));
    if sums.0[CHECKSUM_CHANNELS] != expected_total {
        return Err("checksum total is not wrapping sum of 15 channels".into());
    }
    Ok(())
}

fn adler32(bytes: &[u8]) -> u32 {
    const MOD_ADLER: u32 = 65_521;
    let mut a = 1u32;
    let mut b = 0u32;
    for byte in bytes {
        a = (a + u32::from(*byte)) % MOD_ADLER;
        b = (b + a) % MOD_ADLER;
    }
    (b << 16) | a
}

fn hash_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn retail_options(addr: String) -> RetailOptions {
        RetailOptions {
            addr,
            id: CLIENT_ID,
            turns: 1,
            timeout_secs: 5,
            game_key: None,
            passive: false,
        }
    }

    #[test]
    fn checksum_is_exact_shape_and_total() {
        let a = checksum_command(7);
        let b = checksum_command(8);
        assert_eq!(a.len(), 65);
        assert_ne!(a, b);
        validate_checksum_command(&a).unwrap();
        validate_checksum_command(&b).unwrap();
    }

    #[test]
    fn all_setup_and_packet_shapes_round_trip() {
        let proof = prove_shapes().unwrap();
        assert_eq!(proof.game_record_bytes, 46);
        assert_eq!(proof.add_player_bytes, 70);
        assert_eq!(proof.player_list_bytes, 34);
        assert_eq!(proof.ready_flag_bytes, 2);
        assert_eq!(proof.command_package_bytes, 73);
    }

    #[test]
    fn two_owned_peers_complete_lockstep() {
        let (_, host, client) = run(3).unwrap();
        assert_eq!(host.checksum_hash, client.checksum_hash);
        assert_ne!(host.checksum_hash, 0);
    }

    #[test]
    fn cli_preserves_synthetic_mode_and_adds_bounded_retail_mode() {
        assert_eq!(
            parse_args(["--turns", "3"].into_iter().map(str::to_owned)).unwrap(),
            Some(Mode::Synthetic { turns: 3 })
        );
        assert_eq!(
            parse_args(
                [
                    "--retail-connect",
                    "127.0.0.1:31337",
                    "--turns",
                    "2",
                    "--timeout-secs",
                    "9",
                    "--game-key",
                    "0x123456",
                ]
                .into_iter()
                .map(str::to_owned),
            )
            .unwrap(),
            Some(Mode::Retail(RetailOptions {
                addr: "127.0.0.1:31337".into(),
                id: CLIENT_ID,
                turns: 2,
                timeout_secs: 9,
                game_key: Some(0x123456),
                passive: false,
            }))
        );
    }

    #[test]
    fn multiplayer_key_is_recovered_and_checksum_reencoded() {
        let key = 0x00a1_b2c3;
        let checksum = checksum_command(17);
        let payload = encode_checksum_only(&checksum, key).unwrap();
        let recovered = recover_game_key(&[payload.clone()]).expect("recover package key");
        let decoded = decode_traffic(&payload, recovered).unwrap();
        assert_eq!(decoded.command_count, 1);
        assert_eq!(decoded.opcodes, [CHECKSUM_OPCODE]);
        assert_eq!(decoded.checksum_bytes.as_deref(), Some(checksum.as_slice()));
        assert_eq!(
            encode_checksum_only(&checksum, recovered).unwrap(),
            payload,
            "a compatible recovered key must reproduce the exact wire payload"
        );
    }

    #[test]
    fn retail_connect_replies_repeatedly_disconnects_and_rejoins_cleanly() {
        let host_transport = TcpTransport::host(HOST_ID, "127.0.0.1:0").unwrap();
        let addr = host_transport.local_addr().unwrap();
        let mut options = retail_options(addr.to_string());
        options.turns = 3;
        let peer = std::thread::spawn(move || run_retail(&options));
        let mut host = Session::new(host_transport, Role::Host, PEER_NAME);
        let start = Instant::now();
        let now = || start.elapsed().as_millis() as u64;
        let mut remote_join_sequence = None;
        let mut remote_ready_sequence = None;
        let mut transition_sequence = 0u32;

        while !roster_is_authoritative(&host) {
            host.poll(now(), Duration::from_millis(5)).unwrap();
            for event in host.drain_events() {
                transition_sequence += 1;
                match event {
                    Event::PlayerJoined(id) if id == CLIENT_ID => {
                        remote_join_sequence.get_or_insert(transition_sequence);
                    }
                    Event::ReadyChanged {
                        unique_id: CLIENT_ID,
                        ready: true,
                    } => {
                        remote_ready_sequence.get_or_insert(transition_sequence);
                    }
                    Event::Game { .. } => panic!("owned peer sent game traffic before roster"),
                    _ => {}
                }
            }
            assert!(start.elapsed() < Duration::from_secs(3));
        }
        host.send_ready_flag(true).unwrap();
        while !host.all_ready() {
            host.poll(now(), Duration::from_millis(5)).unwrap();
            for event in host.drain_events() {
                transition_sequence += 1;
                match event {
                    Event::PlayerJoined(id) if id == CLIENT_ID => {
                        remote_join_sequence.get_or_insert(transition_sequence);
                    }
                    Event::ReadyChanged {
                        unique_id: CLIENT_ID,
                        ready: true,
                    } => {
                        remote_ready_sequence.get_or_insert(transition_sequence);
                    }
                    Event::Game { .. } => panic!("owned peer sent game traffic before ready"),
                    _ => {}
                }
            }
            assert!(start.elapsed() < Duration::from_secs(3));
        }
        assert!(
            remote_join_sequence.is_some()
                && remote_ready_sequence.is_some()
                && remote_join_sequence < remote_ready_sequence,
            "remote transition must be PlayerJoined then ReadyChanged(true): join={remote_join_sequence:?} ready={remote_ready_sequence:?}"
        );

        // The owned client is reactive: readiness alone must never synthesize
        // a turn. Give it multiple poll cycles and fail on any game packet
        // before the host supplies the first authoritative stamp/checksum.
        let quiet_until = Instant::now() + Duration::from_millis(100);
        while Instant::now() < quiet_until {
            host.poll(now(), Duration::from_millis(5)).unwrap();
            assert!(
                host.drain_events()
                    .into_iter()
                    .all(|event| !matches!(event, Event::Game { .. })),
                "owned peer emitted a premature game packet after all-ready"
            );
        }

        let key = 0x005a_c33d;
        let mut first_peer_left = false;
        for stamp in 23..26 {
            let checksum = checksum_command(stamp);
            let host_payload = encode_checksum_only(&checksum, key).unwrap();
            host.send_command_package(stamp, 0, &host_payload).unwrap();
            while !host.turn_ready(stamp) {
                host.poll(now(), Duration::from_millis(5)).unwrap();
                first_peer_left |= host.drain_events().contains(&Event::PlayerLeft(CLIENT_ID));
                assert!(start.elapsed() < Duration::from_secs(5));
            }
            let packages = host.take_turn(stamp);
            assert_eq!(packages.len(), 2);
            let client = packages.iter().find(|package| package.play == 1).unwrap();
            assert_eq!(client.stamp, stamp);
            let decoded = decode_traffic(&client.payload, key).unwrap();
            assert_eq!(decoded.checksum_bytes.as_deref(), Some(checksum.as_slice()));
        }

        let report = peer.join().unwrap().unwrap();
        assert_eq!(report.host_id, HOST_ID);
        assert_eq!(report.local_slot, 1);
        assert!(report.all_ready_observed);
        assert_eq!(report.packages_seen, 3);
        assert_eq!(report.checksum_turns, 3);
        assert_eq!(report.packages_sent, 3);
        assert!(report.orderly_disconnect_sent);

        let mut left = first_peer_left;
        while !left {
            host.poll(now(), Duration::from_millis(5)).unwrap();
            left = host.drain_events().contains(&Event::PlayerLeft(CLIENT_ID));
            assert!(start.elapsed() < Duration::from_secs(6));
        }
        assert_eq!(host.players().len(), 1);

        // The exact IPT_DESTROYPLAYER transition permits the same owned ID to
        // reconnect. A socket drop without that packet is not promoted to a
        // protocol guarantee here; it remains timeout-driven in Session.
        let reconnect_options = retail_options(addr.to_string());
        let rejoined = std::thread::spawn(move || run_retail(&reconnect_options));
        let mut host_ready_republished = false;
        while !roster_is_authoritative(&host) || !host.all_ready() {
            host.poll(now(), Duration::from_millis(5)).unwrap();
            host.drain_events();
            if roster_is_authoritative(&host) && !host_ready_republished {
                // Reconnect is a new setup readiness epoch. Restate the host
                // flag after membership exists; a pre-roster READYFLAG is
                // deliberately dropped by the retail-compatible handler.
                host.send_ready_flag(true).unwrap();
                host_ready_republished = true;
            }
            assert!(start.elapsed() < Duration::from_secs(8));
        }
        let stamp = 26;
        let checksum = checksum_command(stamp);
        let host_payload = encode_checksum_only(&checksum, key).unwrap();
        host.send_command_package(stamp, 0, &host_payload).unwrap();
        while !host.turn_ready(stamp) {
            host.poll(now(), Duration::from_millis(5)).unwrap();
            host.drain_events();
            assert!(start.elapsed() < Duration::from_secs(9));
        }
        let packages = host.take_turn(stamp);
        assert_eq!(packages.len(), 2);
        let client = packages.iter().find(|package| package.play == 1).unwrap();
        let decoded = decode_traffic(&client.payload, key).unwrap();
        assert_eq!(decoded.checksum_bytes.as_deref(), Some(checksum.as_slice()));
        let rejoin_report = rejoined.join().unwrap().unwrap();
        assert_eq!(rejoin_report.local_slot, 1);
        assert_eq!(rejoin_report.packages_sent, 1);
        assert!(rejoin_report.orderly_disconnect_sent);
    }
}
