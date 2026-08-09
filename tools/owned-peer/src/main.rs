use don_net::internal::{InternalPacket, MAX_PLAYERS};
use don_net::lobby::{
    attributes_to_game, attributes_to_player, game_to_attributes, player_to_attributes,
};
use don_net::msg::NetMsg;
use don_net::session::{Role, Session};
use don_net::setup::{GameConnectionData, PlayerSlotPod};
use don_net::transport::TcpTransport;
use don_net::{CheckSums, Command};
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
const BLOCKER: &str = "a real retail second member requires a distinct authenticated PlayFab entity backed by a distinct Steam auth ticket/account; CrossPlayService::P2PStartConnection (0x1001dc60) and CreateLocalPlayerLoopback (0x1001e350) are shipped no-ops";

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

fn main() {
    let turns = match parse_turns(env::args().skip(1)) {
        Ok(Some(turns)) => turns,
        Ok(None) => return,
        Err(e) => fail(&e),
    };

    match run(turns) {
        Ok((shape, host, client)) => {
            println!(
                "{{\"schema\":\"don.owned-peer.v1\",\"status\":\"pass\",\"transport\":\"tcp-loopback\",\"peers\":2,\"peer_name\":\"Ai\",\"turns\":{},\"setup\":{{\"transport\":\"offline PlayFab lobby-attribute roundtrip\",\"game_record_bytes\":{},\"game_attribute_keys\":{},\"player_attribute_keys\":{}}},\"packet_shapes\":{{\"add_player_bytes\":{},\"player_list_bytes\":{},\"ready_flag_bytes\":{},\"command_package_bytes\":{},\"checksum_command_bytes\":{}}},\"checks\":{{\"authoritative_roster\":true,\"all_ready_both_peers\":true,\"one_package_per_peer_per_turn\":true,\"checksum_opcode\":\"0x39\",\"checksum_total_relation\":true,\"checksum_adler32_values\":true,\"peer_turn_hash_equal\":true}},\"turn_hash\":\"{:016x}\",\"retail_friend_slot_occupied\":false,\"credential_material\":\"none\",\"direct_join_blocker\":\"{}\"}}",
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
                BLOCKER
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
    }
}

fn fail(message: &str) -> ! {
    let escaped = message.replace('\\', "\\\\").replace('"', "\\\"");
    eprintln!(
        "{{\"schema\":\"don.owned-peer.v1\",\"status\":\"fail\",\"error\":\"{}\",\"retail_friend_slot_occupied\":false,\"credential_material\":\"none\"}}",
        escaped
    );
    std::process::exit(1)
}

fn parse_turns<I>(mut args: I) -> Result<Option<u32>, String>
where
    I: Iterator<Item = String>,
{
    let mut turns = DEFAULT_TURNS;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--turns" => {
                let raw = args.next().ok_or("--turns requires a value")?;
                turns = raw
                    .parse::<u32>()
                    .map_err(|_| format!("invalid --turns value: {raw}"))?;
            }
            "-h" | "--help" => {
                println!("Usage: don-owned-peer [--turns N]\n\nRuns two owned Ai peers over TCP loopback without contacting or modifying retail.");
                return Ok(None);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    if !(1..=MAX_TURNS).contains(&turns) {
        return Err(format!("--turns must be in 1..={MAX_TURNS}"));
    }
    Ok(Some(turns))
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
}
