use don_net::extension::game_keys_are_wire_equivalent;
use don_net::internal::{InternalPacket, MAX_PLAYERS};
use don_net::lobby::{
    attributes_to_game, attributes_to_player, game_to_attributes, player_to_attributes,
};
use don_net::msg::NetMsg;
use don_net::obfuscate::{rank_xor_keys, xor_payload};
use don_net::session::{Event, Role, Session};
use don_net::setup::{GameConnectionData, PlayerSlotPod};
use don_net::transport::{Dest, TcpTransport, Transport};
use don_net::{
    decode_commands, encode_commands, CheckSums, Command, EpochCause, EpochMember, LockstepRunner,
    LockstepStatus, Obfuscation, PersistedLockstepTranscript, ReplayAction, TurnPackage,
};
use std::collections::BTreeSet;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const DEFAULT_TURNS: u32 = 50;
const MAX_TURNS: u32 = 10_000;
const HOST_ID: i32 = 1;
const CLIENT_ID: i32 = 2;
const PEER_NAME: &str = "Ai";
/// Retail lobbies show every slot's label side by side, so the owned client must not reuse
/// the host's. Sharing `Ai` with a `DON_NET_NAME=Ai` host made retail warn about duplicate
/// names and made both drop-screen rows read `Ai`. Synthetic acceptance still uses
/// `PEER_NAME` for both of its own peers, where the shared label is the point.
const RETAIL_PEER_NAME: &str = "DoN";
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
    name: String,
    turns: u32,
    timeout_secs: u64,
    game_key: Option<u32>,
    passive: bool,
    evidence: Option<PathBuf>,
    reconnect_after: Option<u32>,
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
    /// Turns whose host package decoded exactly but carried no `0x39` command,
    /// answered with a zero-command package. `CommandManager::start`
    /// `0x00942e10` produces exactly one of these per game.
    empty_replies: u32,
    /// 5-byte `NETMSG_SYNCSIGNAL` records answered one-for-one. Retail's
    /// `SyncPoint::sync` `0x0093a2d0` blocks the whole process until every
    /// participating `NetPlayer` reports `get_sync_counter() >=
    /// SyncPoint::counter`, and only an inbound sync signal advances a remote
    /// peer's counter (`SyncPoint::process_sync_signal` `0x0093a150`).
    sync_signals_answered: u32,
    orderly_disconnect_sent: bool,
    transcript_hash: u64,
    game_key: u32,
    game_key_source: &'static str,
    reconnects: u32,
    evidence: Option<EvidenceReport>,
}

/// Where the key used to decode retail's packages came from. Reported so a run
/// record never conflates a value read out of the match with one inferred from
/// ciphertext.
const KEY_SOURCE_OPERATOR: &str = "operator-supplied";
const KEY_SOURCE_RANKING: &str = "ciphertext-ranking";
const KEY_SOURCE_UNKNOWN: &str = "none";

#[derive(Debug)]
struct EvidenceReport {
    path: PathBuf,
    bytes: usize,
    binary_fnv1a64: u64,
    outcome_fnv1a64: u64,
}

struct EvidenceCapture {
    runner: LockstepRunner,
    actions: Vec<ReplayAction>,
    game_key: u32,
    timeout_ms: u64,
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
                "{{\"schema\":\"don.owned-peer.retail.v3\",\"status\":\"pass\",\"mode\":\"retail-connect\",\"transport\":\"replacement-crossplaynetlib-tcp\",\"peer_name\":\"{}\",\"local_id\":{},\"host_id\":{},\"local_slot\":{},\"all_ready_observed\":{},\"packages_seen\":{},\"checksum_turns\":{},\"packages_sent\":{},\"empty_replies\":{},\"sync_signals_answered\":{},\"orderly_disconnect_sent\":{},\"reconnects\":{},\"reply_policy\":\"{}\",\"compatible_game_key\":\"0x{:08x}\",\"game_key_source\":\"{}\",\"transcript_hash\":\"{:016x}\",\"evidence\":{},\"credential_material\":\"none\",\"simulation_equivalence_claimed\":false}}",
                json_escape(&options.name),
                report.local_id,
                report.host_id,
                report.local_slot,
                report.all_ready_observed,
                report.packages_seen,
                report.checksum_turns,
                report.packages_sent,
                report.empty_replies,
                report.sync_signals_answered,
                report.orderly_disconnect_sent,
                report.reconnects,
                if options.passive { "passive" } else { "mirror-retail-checksum" },
                report.game_key,
                report.game_key_source,
                report.transcript_hash,
                json_evidence(report.evidence.as_ref()),
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
    let mut name = RETAIL_PEER_NAME.to_string();
    let mut passive = false;
    let mut evidence = None;
    let mut reconnect_after = None;
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
            "--name" => {
                name = args.next().ok_or("--name requires a value")?;
                if name.is_empty() {
                    return Err("--name must not be empty".into());
                }
            }
            "--game-key" => {
                let raw = args.next().ok_or("--game-key requires a value")?;
                game_key = Some(parse_u32(&raw)?);
            }
            "--passive" => passive = true,
            "--evidence" => {
                evidence = Some(PathBuf::from(
                    args.next().ok_or("--evidence requires a path")?,
                ));
            }
            "--reconnect-after" => {
                let raw = args.next().ok_or("--reconnect-after requires a value")?;
                reconnect_after = Some(
                    raw.parse::<u32>()
                        .map_err(|_| format!("invalid --reconnect-after value: {raw}"))?,
                );
            }
            "-h" | "--help" => {
                println!(
                    "Usage:\n  don-owned-peer [--turns N]\n  don-owned-peer --retail-connect HOST:PORT [--id N] [--name NAME] [--turns N] [--timeout-secs N] [--game-key 0xG] [--passive] [--evidence PATH] [--reconnect-after N]\n\nWithout --retail-connect, runs the two-owned-peer TCP loopback acceptance. Retail mode directly joins only the supplied replacement-CrossplayNetLib TCP endpoint under --name (default DoN, distinct from the host so retail does not warn about duplicate labels); it carries no authentication material. The multiplayer package key comes from the host's announced GameInfo::seed when available, else --game-key, else ciphertext ranking; ranking cannot settle the checksum-less first turn of a match. It returns one package per observed retail turn: a byte-identical copy of that turn's checksum command, or a package carrying no commands when the turn legitimately has none. --passive reports traffic without returning turn packages. --evidence atomically creates a bounded canonical DONLSTP file. --reconnect-after performs one orderly same-ID reconnect after N completed turns and requires N < --turns."
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
            if evidence
                .as_ref()
                .is_some_and(|path| path.as_os_str().is_empty())
            {
                return Err("--evidence path must not be empty".into());
            }
            if evidence.is_some() && passive && turns != 1 {
                return Err("passive --evidence requires --turns 1 because no slot-1 package is emitted to complete and advance a turn".into());
            }
            if passive && reconnect_after.is_some() {
                return Err("--reconnect-after requires active checksum replies".into());
            }
            if let Some(after) = reconnect_after {
                if after == 0 || after >= turns {
                    return Err("--reconnect-after must be in 1..--turns".into());
                }
            }
            Ok(Some(Mode::Retail(RetailOptions {
                addr,
                id,
                name,
                turns,
                timeout_secs,
                game_key,
                passive,
                evidence,
                reconnect_after,
            })))
        }
        None => {
            if id != CLIENT_ID
                || name != RETAIL_PEER_NAME
                || game_key.is_some()
                || passive
                || evidence.is_some()
                || reconnect_after.is_some()
            {
                return Err(
                    "--id, --name, --game-key, --passive, --evidence, and --reconnect-after require --retail-connect"
                        .into(),
                );
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

impl EvidenceCapture {
    fn new(
        game_key: u32,
        timeout_ms: u64,
        at_ms: u64,
        first_stamp: u32,
        host_id: i32,
        local_id: i32,
    ) -> Result<Self, String> {
        let members = retail_members(host_id, local_id);
        let runner = LockstepRunner::new(
            game_key,
            members.iter().map(|member| member.play),
            first_stamp,
            at_ms,
            timeout_ms,
        )
        .map_err(|error| format!("initialize canonical evidence: {error}"))?;
        Ok(Self {
            runner,
            actions: vec![ReplayAction::Initial {
                at_ms,
                first_stamp,
                members,
            }],
            game_key,
            timeout_ms,
        })
    }

    fn submit(&mut self, at_ms: u64, package: TurnPackage) -> Result<(), String> {
        self.runner
            .submit(package.clone(), at_ms)
            .map_err(|error| format!("record canonical package: {error}"))?;
        self.actions.push(ReplayAction::Package { at_ms, package });
        Ok(())
    }

    fn observe_and_commit(&mut self, at_ms: u64) -> Result<(), String> {
        let status = self.runner.status(at_ms);
        self.actions.push(ReplayAction::ObserveDeadline { at_ms });
        if !matches!(status, LockstepStatus::Ready { .. }) {
            return Err(format!(
                "canonical turn was not ready after the exact host and owned-peer packages: {status:?}"
            ));
        }
        self.runner
            .commit_ready(at_ms)
            .map_err(|error| format!("commit canonical turn: {error}"))?;
        self.actions.push(ReplayAction::Commit { at_ms });
        Ok(())
    }

    fn begin_epoch(
        &mut self,
        at_ms: u64,
        cause: EpochCause,
        members: Vec<EpochMember>,
    ) -> Result<(), String> {
        self.runner
            .begin_epoch(members.iter().map(|member| member.play), cause, at_ms)
            .map_err(|error| format!("record canonical {cause:?} epoch: {error}"))?;
        self.actions.push(ReplayAction::Epoch {
            at_ms,
            cause,
            members,
        });
        Ok(())
    }

    fn finish(self, path: &Path) -> Result<EvidenceReport, String> {
        let live_json = self.runner.export_json();
        let transcript =
            PersistedLockstepTranscript::record(self.game_key, self.timeout_ms, self.actions)
                .map_err(|error| format!("finalize canonical evidence: {error}"))?;
        if transcript.outcome_json() != live_json {
            return Err("canonical evidence replay diverged from the live recorder".into());
        }
        let bytes = transcript
            .encode()
            .map_err(|error| format!("encode canonical evidence: {error}"))?;
        let decoded = PersistedLockstepTranscript::decode(&bytes)
            .map_err(|error| format!("self-decode canonical evidence: {error}"))?;
        let replayed = decoded
            .replay()
            .map_err(|error| format!("self-replay canonical evidence: {error}"))?;
        let reencoded = decoded
            .encode()
            .map_err(|error| format!("re-encode canonical evidence: {error}"))?;
        if replayed.outcome_json != live_json || reencoded != bytes {
            return Err("canonical evidence failed deterministic decode/re-encode/replay".into());
        }
        atomic_create(path, &bytes)?;
        Ok(EvidenceReport {
            path: path.to_path_buf(),
            bytes: bytes.len(),
            binary_fnv1a64: decoded
                .binary_fnv1a64()
                .map_err(|error| format!("hash canonical evidence: {error}"))?,
            outcome_fnv1a64: decoded.outcome_fnv1a64(),
        })
    }
}

fn retail_members(host_id: i32, local_id: i32) -> Vec<EpochMember> {
    vec![
        EpochMember {
            play: 0,
            unique_id: host_id,
        },
        EpochMember {
            play: 1,
            unique_id: local_id,
        },
    ]
}

fn atomic_create(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if path.as_os_str().is_empty() {
        return Err("evidence path must not be empty".into());
    }
    if path.exists() {
        return Err(format!(
            "refusing to replace existing evidence file {}",
            path.display()
        ));
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("evidence path has no UTF-8 file name: {}", path.display()))?;
    let mut last_collision = None;
    for nonce in 0..100u32 {
        let temp = parent.join(format!(".{name}.{}.{}.tmp", std::process::id(), nonce));
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                last_collision = Some(error);
                continue;
            }
            Err(error) => {
                return Err(format!(
                    "create evidence temporary file {}: {error}",
                    temp.display()
                ));
            }
        };
        let write_result = file.write_all(bytes).and_then(|()| file.sync_all());
        drop(file);
        if let Err(error) = write_result {
            let _ = fs::remove_file(&temp);
            return Err(format!(
                "write evidence temporary file {}: {error}",
                temp.display()
            ));
        }
        if let Err(error) = fs::hard_link(&temp, path) {
            let _ = fs::remove_file(&temp);
            return Err(format!(
                "atomically publish evidence {} without replacing an existing file: {error}",
                path.display()
            ));
        }
        fs::remove_file(&temp).map_err(|error| {
            format!(
                "remove evidence temporary link {} after publication: {error}",
                temp.display()
            )
        })?;
        return Ok(());
    }
    Err(format!(
        "could not allocate evidence temporary file beside {}: {}",
        path.display(),
        last_collision
            .map(|error| error.to_string())
            .unwrap_or_else(|| "too many collisions".into())
    ))
}

fn run_retail(options: &RetailOptions) -> Result<RetailReport, String> {
    prove_shapes()?;
    let start = Instant::now();
    let deadline = Duration::from_secs(options.timeout_secs);
    let transport = bounded_join(options.id, options.addr.clone(), deadline)?;
    let mut session = Session::new(transport, Role::Client, &options.name);
    let now = || start.elapsed().as_millis() as u64;
    let mut roster_announced = false;
    let mut ready_sent = false;
    let mut all_ready_observed = false;
    let mut host_id = 0;
    let mut local_slot = usize::MAX;
    let mut packages_seen = 0u32;
    let mut checksum_turns = 0u32;
    let mut packages_sent = 0u32;
    let mut empty_replies = 0u32;
    let mut sync_signals_answered = 0u32;
    let mut transcript_hash = 0xcbf2_9ce4_8422_2325u64;
    let mut game_key = options.game_key;
    let mut game_key_source = if options.game_key.is_some() {
        KEY_SOURCE_OPERATOR
    } else {
        KEY_SOURCE_UNKNOWN
    };
    let mut key_samples = Vec::<Vec<u8>>::new();
    let mut seen_packages = BTreeSet::<(i32, u32, i8)>::new();
    let mut initial_roster_at_ms = None;
    let mut evidence_capture = None::<EvidenceCapture>;
    let mut reconnect_pending = false;
    let mut reconnects = 0u32;

    println!(
        "{{\"schema\":\"don.owned-peer.retail.event.v1\",\"event\":\"connected\",\"peer_name\":\"{}\",\"local_id\":{},\"endpoint\":\"{}\",\"credential_material\":\"none\"}}",
        json_escape(&options.name),
        options.id,
        json_escape(&options.addr),
    );

    loop {
        session
            .poll(now(), Duration::from_millis(10))
            .map_err(|e| format!("retail session poll: {e}"))?;

        // The host announces `GameInfo::seed` over the DoN transport extension
        // immediately before the command package that needs it, so this is
        // settled before any package is inspected below. `Session` has already
        // refused anything that did not come from the authoritative host.
        if let Some(announced) = session.announced_game_key() {
            if game_key != Some(announced.seed) {
                if let Some(active) = game_key {
                    if !game_keys_are_wire_equivalent(active, announced.seed) {
                        return Err(format!(
                            "retail host announced match key 0x{:08x} ({}), which is not the transform of the key 0x{active:08x} already in use ({game_key_source}); refusing to reinterpret this lockstep transcript",
                            announced.seed,
                            announced.source.as_str(),
                        ));
                    }
                }
                game_key = Some(announced.seed);
                game_key_source = announced.source.as_str();
                println!(
                    "{{\"schema\":\"don.owned-peer.retail.event.v1\",\"event\":\"game-key-announced\",\"from\":{},\"game_key\":\"0x{:08x}\",\"xor_key\":\"0x{:04x}\",\"source\":\"{}\"}}",
                    announced.from,
                    announced.seed,
                    Obfuscation::xor_key(announced.seed),
                    announced.source.as_str(),
                );
            }
        }

        if let Some((host, slot)) =
            authoritative_retail_roster_named(&session, options.id, &options.name)?
        {
            if host_id != 0 && host_id != host {
                return Err(format!(
                    "authoritative host identity changed across setup epochs: {host_id} -> {host}"
                ));
            }
            host_id = host;
            local_slot = slot;
            if !roster_announced {
                let roster_ms = now();
                initial_roster_at_ms.get_or_insert(roster_ms);
                if reconnect_pending {
                    if let Some(capture) = &mut evidence_capture {
                        capture.begin_epoch(
                            roster_ms,
                            EpochCause::Reconnect,
                            retail_members(host_id, options.id),
                        )?;
                    }
                    reconnect_pending = false;
                }
                println!(
                    "{{\"schema\":\"don.owned-peer.retail.event.v1\",\"event\":\"roster\",\"host_id\":{},\"host_slot\":0,\"local_id\":{},\"local_slot\":{},\"members\":2,\"peer_name\":\"{}\"}}",
                    host_id,
                    options.id,
                    local_slot,
                    json_escape(&options.name),
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
        let mut reconnect_requested = false;
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
            // `SyncPoint::sync` `0x0093a2d0` is a blocking rendezvous, and it is
            // the reason a retail host sits on "Starting Game" forever when the
            // only other participant is silent. It increments the global
            // `SyncPoint::counter` `0x00cbee7c`, bumps the *local* NetPlayer's
            // counter, broadcasts one 5-byte sync signal through
            // `NetDaemon::send_sync_signal` `0x00950c50`, then spins on
            // `NetDaemon::process_all` until **every** NetSys player bound to a
            // `GameInfo::player[j].net_player` reports
            // `NetPlayer::get_sync_counter() >= SyncPoint::counter`
            // (`0x0093a48d..0x0093a496`).
            //
            // A remote peer's counter only ever moves on the receiving side:
            // `NetDaemon::process` `0x00950f30` dispatches masked type 10 to
            // `SyncPoint::process_sync_signal` `0x0093a150`, which calls
            // `NetPlayer::inc_sync_counter` on the *sender's* NetPlayer. So the
            // peer does not account for anything; it puts one sync signal on the
            // wire per sync signal received and retail does its own arithmetic
            // against its own counter.
            if let NetMsg::SyncSignal { play } = framed.msg {
                // `0x0093a150` reads only ECX (the `NetPlayer*` the transport
                // resolved); it never dereferences the message. The `play` field
                // is therefore inert on the only receive path that exists in this
                // build, and echoing the observed word is the one choice that
                // puts no underived number on the wire. Retail fills it from
                // `Console::play` (`[[0x00c06210]+0x2a0]`, `0x00950c9c`), which a
                // peer outside the process cannot read.
                session
                    .send_to(from, &NetMsg::SyncSignal { play })
                    .map_err(|e| format!("answer retail sync signal from {from}: {e}"))?;
                sync_signals_answered = sync_signals_answered.saturating_add(1);
                println!(
                    "{{\"schema\":\"don.owned-peer.retail.event.v1\",\"event\":\"sync-signal\",\"from\":{},\"play\":{},\"answered\":true,\"answered_total\":{}}}",
                    from, play, sync_signals_answered,
                );
                continue;
            }
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
                    game_key_source = KEY_SOURCE_RANKING;
                    println!(
                        "{{\"schema\":\"don.owned-peer.retail.event.v1\",\"event\":\"game-key-recovered\",\"compatible_game_key\":\"0x{key:08x}\",\"xor_key\":\"0x{:04x}\",\"source\":\"{KEY_SOURCE_RANKING}\"}}",
                        Obfuscation::xor_key(key),
                    );
                }
            }

            let Some(key) = game_key else {
                // Ciphertext ranking anchors a candidate on the checksum
                // invariant, and `CommandManager::start` `0x00942e10` sends the
                // first package of every match with no `0x39` command in it —
                // so ranking is structurally unable to settle the key from that
                // package, however long it is. The key has to arrive from the
                // match itself.
                return Err(format!(
                    "no match key at stamp {stamp}: the host announced none and ciphertext ranking cannot settle one from a checksum-less package; refusing to answer a turn this peer cannot read (supply --game-key, or run a shim host that announces GameInfo::seed)"
                ));
            };
            let traffic = decode_traffic(payload, key)
                .map_err(|e| format!("decode retail turn {stamp} with key 0x{key:08x}: {e}"))?;
            let package_ms = now();
            let checksum_bytes = traffic.checksum_bytes.clone();
            println!(
                "{{\"schema\":\"don.owned-peer.retail.event.v1\",\"event\":\"turn\",\"stamp\":{},\"play\":{},\"payload_bytes\":{},\"commands\":{},\"opcodes\":{},\"checksum_decoded\":{},\"checksums\":{}}}",
                stamp,
                play,
                payload.len(),
                traffic.command_count,
                json_u8_array(&traffic.opcodes),
                traffic.checksum.is_some(),
                match &traffic.checksum {
                    Some(sums) => json_checksum_array(sums),
                    None => "null".into(),
                },
            );

            // Evidence is the checksum-agreement record, so it opens on the
            // first checksum-bearing stamp. A checksum-less start package is
            // reported above and answered below, but there is nothing for a
            // checksum transcript to hold about it.
            if checksum_bytes.is_some() {
                if options.evidence.is_some() && evidence_capture.is_none() {
                    evidence_capture = Some(EvidenceCapture::new(
                        key,
                        options.timeout_secs.saturating_mul(1_000),
                        initial_roster_at_ms.ok_or(
                            "authoritative package arrived before a recorded setup roster epoch",
                        )?,
                        stamp,
                        host_id,
                        options.id,
                    )?);
                }
                if let Some(capture) = &mut evidence_capture {
                    capture.submit(
                        package_ms,
                        TurnPackage {
                            stamp,
                            play,
                            payload: payload.to_vec(),
                        },
                    )?;
                }
                checksum_turns = checksum_turns.saturating_add(1);
            }
            hash_bytes(&mut transcript_hash, &stamp.to_le_bytes());
            hash_bytes(&mut transcript_hash, &[play as u8]);
            hash_bytes(&mut transcript_hash, &[u8::from(checksum_bytes.is_some())]);
            if let Some(bytes) = &checksum_bytes {
                hash_bytes(&mut transcript_hash, bytes);
            }

            if !options.passive {
                // Two reply shapes, neither of which invents simulation state:
                // mirror the stamp's own checksum when retail issued one, and
                // otherwise send a package carrying no commands at all — the
                // truthful "this peer issued nothing this turn", and the one
                // package a peer can always form because an empty payload has
                // no XOR words and no inter-command pad to get wrong.
                let (reply, policy) = match &checksum_bytes {
                    Some(bytes) => (encode_checksum_only(bytes, key)?, "mirror-retail-checksum"),
                    None => (Vec::new(), "empty-package"),
                };
                session
                    .send_command_package(stamp, local_slot as i8, &reply)
                    .map_err(|e| format!("send {policy} for turn {stamp}: {e}"))?;
                packages_sent = packages_sent.saturating_add(1);
                if checksum_bytes.is_none() {
                    empty_replies = empty_replies.saturating_add(1);
                }
                if checksum_bytes.is_some() {
                    if let Some(capture) = &mut evidence_capture {
                        let reply_ms = now();
                        capture.submit(
                            reply_ms,
                            TurnPackage {
                                stamp,
                                play: local_slot as i8,
                                payload: reply.clone(),
                            },
                        )?;
                        capture.observe_and_commit(reply_ms)?;
                    }
                }
                println!(
                    "{{\"schema\":\"don.owned-peer.retail.event.v1\",\"event\":\"turn-sent\",\"stamp\":{},\"play\":{},\"payload_bytes\":{},\"policy\":\"{policy}\",\"simulation_equivalence_claimed\":false}}",
                    stamp,
                    local_slot,
                    reply.len(),
                );
            }
            if options.reconnect_after == Some(checksum_turns) && reconnects == 0 {
                reconnect_requested = true;
                break;
            }
        }

        if reconnect_requested {
            send_orderly_destroy(&mut session, options.id)?;
            let drop_ms = now();
            if let Some(capture) = &mut evidence_capture {
                capture.begin_epoch(
                    drop_ms,
                    EpochCause::Drop,
                    vec![EpochMember {
                        play: 0,
                        unique_id: host_id,
                    }],
                )?;
            }
            println!(
                "{{\"schema\":\"don.owned-peer.retail.event.v1\",\"event\":\"disconnect-sent\",\"local_id\":{},\"packet\":\"IPT_DESTROYPLAYER\",\"bytes\":5,\"reason\":\"bounded-reconnect\"}}",
                options.id,
            );
            let remaining = deadline
                .checked_sub(start.elapsed())
                .ok_or("retail-connect deadline expired before bounded reconnect")?;
            let transport = bounded_join(options.id, options.addr.clone(), remaining)?;
            session = Session::new(transport, Role::Client, &options.name);
            roster_announced = false;
            ready_sent = false;
            all_ready_observed = false;
            local_slot = usize::MAX;
            reconnect_pending = true;
            reconnects += 1;
            println!(
                "{{\"schema\":\"don.owned-peer.retail.event.v1\",\"event\":\"reconnected\",\"peer_name\":\"{}\",\"local_id\":{},\"endpoint\":\"{}\",\"credential_material\":\"none\"}}",
                json_escape(&options.name),
                options.id,
                json_escape(&options.addr),
            );
            continue;
        }

        let enough =
            checksum_turns >= options.turns && (options.passive || packages_sent >= options.turns);
        if enough {
            send_orderly_destroy(&mut session, options.id)?;
            let drop_ms = now();
            if let Some(capture) = &mut evidence_capture {
                capture.begin_epoch(
                    drop_ms,
                    EpochCause::Drop,
                    vec![EpochMember {
                        play: 0,
                        unique_id: host_id,
                    }],
                )?;
            }
            println!(
                "{{\"schema\":\"don.owned-peer.retail.event.v1\",\"event\":\"disconnect-sent\",\"local_id\":{},\"packet\":\"IPT_DESTROYPLAYER\",\"bytes\":{}}}",
                options.id,
                5,
            );
            let evidence = match (&options.evidence, evidence_capture) {
                (Some(path), Some(capture)) => {
                    let report = capture.finish(path)?;
                    println!(
                        "{{\"schema\":\"don.owned-peer.retail.event.v1\",\"event\":\"evidence-written\",\"path\":\"{}\",\"bytes\":{},\"binary_fnv1a64\":\"{:016x}\",\"outcome_fnv1a64\":\"{:016x}\"}}",
                        json_escape(&report.path.display().to_string()),
                        report.bytes,
                        report.binary_fnv1a64,
                        report.outcome_fnv1a64,
                    );
                    Some(report)
                }
                (Some(_), None) => {
                    return Err(
                        "evidence was requested but no authoritative package was recorded".into(),
                    )
                }
                (None, _) => None,
            };
            return Ok(RetailReport {
                local_id: options.id,
                host_id,
                local_slot,
                all_ready_observed,
                packages_seen,
                checksum_turns,
                packages_sent,
                empty_replies,
                sync_signals_answered,
                orderly_disconnect_sent: true,
                transcript_hash,
                game_key: game_key.expect("checksum traffic requires a key"),
                game_key_source,
                reconnects,
                evidence,
            });
        }
        if start.elapsed() >= deadline {
            return Err(format!(
                "retail-connect timeout after {}s: roster={} ready_sent={} all_ready={} packages_seen={} checksum_turns={} packages_sent={} empty_replies={} sync_signals_answered={} key={} key_source={game_key_source}",
                options.timeout_secs,
                roster_announced,
                ready_sent,
                all_ready_observed,
                packages_seen,
                checksum_turns,
                packages_sent,
                empty_replies,
                sync_signals_answered,
                game_key
                    .map(|key| format!("0x{key:08x}"))
                    .unwrap_or_else(|| "unannounced (supply --game-key)".into()),
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

fn send_orderly_destroy(session: &mut Session<TcpTransport>, local_id: i32) -> Result<(), String> {
    let mut destroy = Vec::new();
    InternalPacket::DestroyPlayer {
        unique_id: local_id,
    }
    .encode(&mut destroy);
    if destroy.len() != 5 {
        return Err(format!(
            "orderly destroy-player encoded to {} bytes, expected 5",
            destroy.len()
        ));
    }
    session
        .transport
        .send(Dest::All, &destroy)
        .map_err(|error| format!("send orderly destroy-player: {error}"))
}

fn authoritative_retail_roster_named(
    session: &Session<TcpTransport>,
    local_id: i32,
    expected_local_name: &str,
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
    // Only our own slot's label is ours to assert. Requiring the HOST to also be called
    // `Ai` was a synthetic-two-peer assumption: a real retail host is a human whose slot
    // carries their own profile name, so that clause refused every genuine lobby. The
    // safety boundary here is the explicit `--retail-connect` address plus the
    // host-authoritative slot/id checks above, not the host's chosen display name.
    if local.name != expected_local_name {
        return Err(format!(
            "retail roster local name mismatch: local={:?}; expected {expected_local_name:?}",
            local.name
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

fn json_evidence(report: Option<&EvidenceReport>) -> String {
    match report {
        Some(report) => format!(
            "{{\"schema\":\"don.lockstep-evidence.v1\",\"path\":\"{}\",\"bytes\":{},\"binary_fnv1a64\":\"{:016x}\",\"outcome_fnv1a64\":\"{:016x}\"}}",
            json_escape(&report.path.display().to_string()),
            report.bytes,
            report.binary_fnv1a64,
            report.outcome_fnv1a64,
        ),
        None => "null".into(),
    }
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
    use don_net::extension::GameKeySource;

    fn retail_options(addr: String) -> RetailOptions {
        RetailOptions {
            addr,
            id: CLIENT_ID,
            name: RETAIL_PEER_NAME.to_string(),
            turns: 1,
            timeout_secs: 5,
            game_key: None,
            passive: false,
            evidence: None,
            reconnect_after: None,
        }
    }

    /// A retail host's roster as the owned peer requires it: the host in slot 0
    /// under its own profile label, this peer in slot 1 under `--name`.
    fn retail_roster_is_authoritative(session: &Session<TcpTransport>) -> bool {
        let players = session.players();
        players.len() == 2
            && players[0].unique_id == HOST_ID
            && players[0].slot == 0
            && players[1].unique_id == CLIENT_ID
            && players[1].slot == 1
            && players[1].name == RETAIL_PEER_NAME
    }

    /// The shape `CommandManager::start` `0x00942e10` puts on the wire: one
    /// `TurnDataCommand` (opcode 0x4a, 11 bytes) and no checksum command.
    fn turn_data_command() -> Vec<u8> {
        let mut bytes = vec![0x4a_u8];
        for word in [12u16, 3, 0, 1, 0] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        assert_eq!(bytes.len(), 11);
        bytes
    }

    fn encode_commands_with_key(commands: &[Command<'_>], game_key: u32) -> Vec<u8> {
        let mut payload = Vec::new();
        let mut obfuscation = Obfuscation::with_seed(game_key);
        encode_commands(commands, &mut obfuscation, &mut payload);
        xor_payload(&mut payload, Obfuscation::xor_key(game_key));
        payload
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
                name: RETAIL_PEER_NAME.to_string(),
                turns: 2,
                timeout_secs: 9,
                game_key: Some(0x123456),
                passive: false,
                evidence: None,
                reconnect_after: None,
            }))
        );
        assert_eq!(
            parse_args(
                [
                    "--retail-connect",
                    "127.0.0.1:31337",
                    "--turns",
                    "2",
                    "--evidence",
                    "run.donlstp",
                    "--reconnect-after",
                    "1",
                ]
                .into_iter()
                .map(str::to_owned),
            )
            .unwrap(),
            Some(Mode::Retail(RetailOptions {
                addr: "127.0.0.1:31337".into(),
                id: CLIENT_ID,
                name: RETAIL_PEER_NAME.to_string(),
                turns: 2,
                timeout_secs: DEFAULT_RETAIL_TIMEOUT_SECS,
                game_key: None,
                passive: false,
                evidence: Some(PathBuf::from("run.donlstp")),
                reconnect_after: Some(1),
            }))
        );
        assert!(parse_args(
            [
                "--retail-connect",
                "127.0.0.1:31337",
                "--turns",
                "2",
                "--passive",
                "--evidence",
                "run.donlstp",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .is_err());
    }

    #[test]
    fn atomic_evidence_publish_never_replaces_an_existing_file() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "don-owned-peer-no-clobber-{}-{nonce}.donlstp",
            std::process::id(),
        ));
        fs::write(&path, b"existing").unwrap();
        let error = atomic_create(&path, b"replacement").unwrap_err();
        assert!(error.contains("refusing to replace"));
        assert_eq!(fs::read(&path).unwrap(), b"existing");
        fs::remove_file(path).unwrap();
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

    /// Bring a mock retail host and the owned peer to the authoritative
    /// two-member all-ready state, then hand back the host session.
    fn mock_retail_host_ready(
        host_transport: TcpTransport,
        start: Instant,
    ) -> Session<TcpTransport> {
        let mut host = Session::new(host_transport, Role::Host, PEER_NAME);
        let now = || start.elapsed().as_millis() as u64;
        while !retail_roster_is_authoritative(&host) {
            host.poll(now(), Duration::from_millis(5)).unwrap();
            host.drain_events();
            assert!(start.elapsed() < Duration::from_secs(3));
        }
        host.send_ready_flag(true).unwrap();
        while !host.all_ready() {
            host.poll(now(), Duration::from_millis(5)).unwrap();
            host.drain_events();
            assert!(start.elapsed() < Duration::from_secs(3));
        }
        host
    }

    /// Retail's pre-match rendezvous, and why silence hangs "Starting Game".
    ///
    /// `SyncPoint::sync` `0x0093a2d0` increments the global `SyncPoint::counter`
    /// `0x00cbee7c`, bumps the local `NetPlayer`'s counter through vtable `+0x48`
    /// (`0x0093a3fa`), broadcasts one 5-byte record via
    /// `NetDaemon::send_sync_signal` `0x00950c50`, and then spins on
    /// `NetDaemon::process_all` `0x00951300` while any participating player still
    /// reports `NetPlayer::get_sync_counter() < SyncPoint::counter`
    /// (`0x0093a48d..0x0093a496`). A remote peer's counter is advanced only by
    /// `SyncPoint::process_sync_signal` `0x0093a150`, which retail runs on
    /// *receipt* of masked type 10 — so the owned peer's entire obligation is to
    /// return one sync signal per sync signal, and retail does its own counting.
    /// `Game::run` `0x00584590` issues four of these before
    /// `CommandManager::start` `0x00942e10`, which issues up to three more (two
    /// of them inside `TimeSync::sync` `0x00955060`), so the one-for-one
    /// accounting is what matters, not a single reply.
    #[test]
    fn the_retail_sync_barrier_is_answered_one_signal_per_signal() {
        let host_transport = TcpTransport::host(HOST_ID, "127.0.0.1:0").unwrap();
        let addr = host_transport.local_addr().unwrap();
        let mut options = retail_options(addr.to_string());
        options.turns = 1;
        options.timeout_secs = 20;
        let seed = 0x0055_1234u32;
        let start = Instant::now();
        let peer = std::thread::spawn(move || run_retail(&options));
        let mut host = mock_retail_host_ready(host_transport, start);
        let now = || start.elapsed().as_millis() as u64;

        let barriers = [0i32, 7];
        let mut echoed = Vec::new();
        for play in barriers {
            host.send_all(&NetMsg::SyncSignal { play }).unwrap();
            let want = echoed.len() + 1;
            while echoed.len() < want {
                host.poll(now(), Duration::from_millis(5)).unwrap();
                for event in host.drain_events() {
                    if let Event::Game { from, msg } = event {
                        assert_eq!(from, CLIENT_ID, "unexpected game traffic origin");
                        echoed.push(msg);
                    }
                }
                assert!(
                    start.elapsed() < Duration::from_secs(10),
                    "the owned peer never answered retail's sync signal (play={play}); \
                     a real host would sit on Starting Game forever"
                );
            }
        }
        assert_eq!(
            echoed.len(),
            barriers.len(),
            "one signal answers one signal: {echoed:?}"
        );
        for (msg, play) in echoed.iter().zip(barriers) {
            assert_eq!(msg.id, 10, "the reply must land on jump-table entry 10");
            assert_eq!(msg.bytes.len(), 5, "NetMsg_SyncSignal is exactly 5 bytes");
            assert_eq!(msg.decode().unwrap().msg, NetMsg::SyncSignal { play });
        }

        // The barrier is crossed; retail then reaches its ordinary turn cadence,
        // and the peer must still be in its normal reactive shape.
        host.announce_game_key(seed, GameKeySource::RetailGameInfoSeed)
            .unwrap();
        let checksum = checksum_command(1);
        host.send_command_package(1, 0, &encode_checksum_only(&checksum, seed).unwrap())
            .unwrap();
        while !host.turn_ready(1) {
            host.poll(now(), Duration::from_millis(5)).unwrap();
            host.drain_events();
            assert!(start.elapsed() < Duration::from_secs(14));
        }
        let report = peer.join().unwrap().unwrap();
        assert_eq!(report.sync_signals_answered, barriers.len() as u32);
        assert_eq!(report.checksum_turns, 1);
        assert_eq!(report.packages_sent, 1);
    }

    #[test]
    fn the_checksum_less_start_package_is_answered_from_the_announced_match_key() {
        // Retail's very first package of a match comes from
        // `CommandManager::start` `0x00942e10`, which issues turn data and sends
        // without ever calling `CommandManager::issue_check_sums` `0x00940770`.
        // Ciphertext ranking is anchored on the checksum invariant, so it cannot
        // settle a key from that package at all; the announced `GameInfo::seed`
        // is what makes turn one readable.
        let seed = 0x1234_5678u32;
        let host_transport = TcpTransport::host(HOST_ID, "127.0.0.1:0").unwrap();
        let addr = host_transport.local_addr().unwrap();
        let mut options = retail_options(addr.to_string());
        options.turns = 1;
        let start = Instant::now();
        let peer = std::thread::spawn(move || run_retail(&options));
        let mut host = mock_retail_host_ready(host_transport, start);
        let now = || start.elapsed().as_millis() as u64;

        host.announce_game_key(seed, GameKeySource::RetailGameInfoSeed)
            .unwrap();

        let start_payload = encode_commands_with_key(
            &[Command {
                opcode: 0x4a,
                bytes: &turn_data_command(),
            }],
            seed,
        );
        host.send_command_package(1, 0, &start_payload).unwrap();
        while !host.turn_ready(1) {
            host.poll(now(), Duration::from_millis(5)).unwrap();
            host.drain_events();
            assert!(start.elapsed() < Duration::from_secs(5));
        }
        let reply = host
            .take_turn(1)
            .into_iter()
            .find(|package| package.play == 1)
            .expect("owned peer answered the checksum-less start turn");
        assert!(
            reply.payload.is_empty(),
            "a peer with no commands must send a package with none, not an invented one: {reply:?}"
        );

        let checksum = checksum_command(2);
        let host_payload = encode_checksum_only(&checksum, seed).unwrap();
        host.send_command_package(2, 0, &host_payload).unwrap();
        while !host.turn_ready(2) {
            host.poll(now(), Duration::from_millis(5)).unwrap();
            host.drain_events();
            assert!(start.elapsed() < Duration::from_secs(6));
        }
        let mirrored = host
            .take_turn(2)
            .into_iter()
            .find(|package| package.play == 1)
            .expect("owned peer answered the checksum turn");
        assert_eq!(
            decode_traffic(&mirrored.payload, seed)
                .unwrap()
                .checksum_bytes
                .as_deref(),
            Some(checksum.as_slice())
        );

        let report = peer.join().unwrap().unwrap();
        assert_eq!(report.packages_seen, 2);
        assert_eq!(report.checksum_turns, 1);
        assert_eq!(report.packages_sent, 2);
        assert_eq!(report.empty_replies, 1);
        assert_eq!(report.game_key, seed, "the exact announced seed is used");
        assert_eq!(
            report.game_key_source,
            GameKeySource::RetailGameInfoSeed.as_str()
        );
    }

    #[test]
    fn an_announced_key_contradicting_the_operator_supplied_one_is_refused() {
        let host_transport = TcpTransport::host(HOST_ID, "127.0.0.1:0").unwrap();
        let addr = host_transport.local_addr().unwrap();
        let mut options = retail_options(addr.to_string());
        // Differs inside bits 0..24, which is exactly the part of the key the
        // package transform reads.
        options.game_key = Some(0x0012_3456);
        let start = Instant::now();
        let peer = std::thread::spawn(move || run_retail(&options));
        let mut host = mock_retail_host_ready(host_transport, start);
        let now = || start.elapsed().as_millis() as u64;

        host.announce_game_key(0x0065_4321, GameKeySource::RetailGameInfoSeed)
            .unwrap();
        while !peer.is_finished() {
            host.poll(now(), Duration::from_millis(5)).unwrap();
            host.drain_events();
            assert!(start.elapsed() < Duration::from_secs(6));
        }
        let error = peer.join().unwrap().unwrap_err();
        assert!(
            error.contains("0x00654321") && error.contains("0x00123456"),
            "both keys must be named in the refusal: {error}"
        );

        // A key differing only above bit 23 drives the identical transform and
        // must not be treated as a contradiction.
        assert!(game_keys_are_wire_equivalent(0x0012_3456, 0xff12_3456));
    }

    #[test]
    fn retail_connect_replies_repeatedly_disconnects_and_rejoins_cleanly() {
        let host_transport = TcpTransport::host(HOST_ID, "127.0.0.1:0").unwrap();
        let addr = host_transport.local_addr().unwrap();
        let mut options = retail_options(addr.to_string());
        options.turns = 4;
        options.reconnect_after = Some(3);
        let evidence_path = std::env::temp_dir().join(format!(
            "don-owned-peer-{}-{}.donlstp",
            std::process::id(),
            addr.port()
        ));
        assert!(!evidence_path.exists());
        options.evidence = Some(evidence_path.clone());
        let peer = std::thread::spawn(move || run_retail(&options));
        let mut host = Session::new(host_transport, Role::Host, PEER_NAME);
        let start = Instant::now();
        let now = || start.elapsed().as_millis() as u64;
        let mut remote_join_sequence = None;
        let mut remote_ready_sequence = None;
        let mut transition_sequence = 0u32;

        while !retail_roster_is_authoritative(&host) {
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
        let mut host_ready_republished = false;
        while !retail_roster_is_authoritative(&host) || !host.all_ready() {
            host.poll(now(), Duration::from_millis(5)).unwrap();
            host.drain_events();
            if retail_roster_is_authoritative(&host) && !host_ready_republished {
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
        let report = peer.join().unwrap().unwrap();
        assert_eq!(report.host_id, HOST_ID);
        assert_eq!(report.local_slot, 1);
        assert!(report.all_ready_observed);
        assert_eq!(report.packages_seen, 4);
        assert_eq!(report.checksum_turns, 4);
        assert_eq!(report.packages_sent, 4);
        assert_eq!(report.reconnects, 1);
        assert!(report.orderly_disconnect_sent);

        let evidence_report = report.evidence.unwrap();
        assert_eq!(evidence_report.path, evidence_path);
        let bytes = fs::read(&evidence_path).unwrap();
        assert_eq!(bytes.len(), evidence_report.bytes);
        let transcript = PersistedLockstepTranscript::decode(&bytes).unwrap();
        assert_eq!(transcript.encode().unwrap(), bytes);
        assert_eq!(
            transcript.binary_fnv1a64().unwrap(),
            evidence_report.binary_fnv1a64
        );
        assert_eq!(
            transcript.outcome_fnv1a64(),
            evidence_report.outcome_fnv1a64
        );
        assert_eq!(transcript.replay().unwrap().next_stamp, 27);
        assert_eq!(
            transcript
                .actions()
                .iter()
                .filter(|action| matches!(action, ReplayAction::Package { .. }))
                .count(),
            8
        );
        assert_eq!(
            transcript
                .actions()
                .iter()
                .filter(|action| matches!(action, ReplayAction::ObserveDeadline { .. }))
                .count(),
            4
        );
        assert_eq!(
            transcript
                .actions()
                .iter()
                .filter(|action| matches!(action, ReplayAction::Commit { .. }))
                .count(),
            4
        );
        assert!(transcript.actions().iter().any(|action| matches!(
            action,
            ReplayAction::Epoch {
                cause: EpochCause::Reconnect,
                members,
                ..
            } if members == &retail_members(HOST_ID, CLIENT_ID)
        )));
        assert_eq!(
            transcript
                .actions()
                .iter()
                .filter(|action| matches!(
                    action,
                    ReplayAction::Epoch {
                        cause: EpochCause::Drop,
                        ..
                    }
                ))
                .count(),
            2
        );
        fs::remove_file(&evidence_path).unwrap();
    }
}
