//! Immutable same-group peer evidence for one retail World checkpoint.
//!
//! This frontier is intentionally not exported by `lib.rs`.  The standalone
//! `don-world-checkpoint` binary and focused integration test path-include it,
//! leaving the replay schedule and shared library surface untouched.

#![forbid(unsafe_code)]

use don_replay::checksum::{Channel, Channels, NUM_CHANNELS};
use don_replay::replay::{load_payload, Replay};
use std::path::Path;

pub const CHECKSUM_OPCODE: u8 = 0x39;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerPacketEvidence {
    pub play: i32,
    pub stamp: u32,
    pub packet_sha256: [u8; 32],
    pub packet_evidence_sha256: [u8; 32],
    pub channels: Channels,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldCheckpointEvidence {
    pub replay_name: String,
    pub replay_bytes: u64,
    pub replay_sha256: [u8; 32],
    pub payload_bytes: u64,
    pub payload_sha256: [u8; 32],
    pub version: String,
    pub group: i32,
    pub world_checksum: u32,
    pub peers: Vec<PeerPacketEvidence>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckpointError {
    ReadReplay(String),
    ReadPayload(String),
    ReplayIdentityDrift,
    MissingTurn(i32),
    MissingPeerPackets,
    DuplicatePeerPacket(i32),
    MalformedPacket { play: i32, bytes: usize },
    DecodedTupleDrift(i32),
    InvalidTuple(i32),
    PeerDisagreement { first: i32, other: i32 },
}

impl std::fmt::Display for CheckpointError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadReplay(error) => write!(f, "cannot read replay: {error}"),
            Self::ReadPayload(error) => write!(f, "cannot decode replay payload: {error}"),
            Self::ReplayIdentityDrift => write!(f, "Replay::open source identity drift"),
            Self::MissingTurn(turn) => write!(f, "replay has no lockstep group {turn}"),
            Self::MissingPeerPackets => write!(f, "group lacks two distinct checksum reporters"),
            Self::DuplicatePeerPacket(play) => {
                write!(
                    f,
                    "player {play} has more than one checksum packet in the group"
                )
            }
            Self::MalformedPacket { play, bytes } => {
                write!(
                    f,
                    "player {play} checksum packet has {bytes} bytes, expected 65"
                )
            }
            Self::DecodedTupleDrift(play) => {
                write!(f, "player {play} packet bytes disagree with Replay::open")
            }
            Self::InvalidTuple(play) => {
                write!(f, "player {play} checksum tuple fails retail self-checks")
            }
            Self::PeerDisagreement { first, other } => {
                write!(f, "same-group players {first} and {other} disagree")
            }
        }
    }
}

pub fn extract_world_checkpoint(
    replay_path: &Path,
    replay: &Replay,
    group: i32,
) -> Result<WorldCheckpointEvidence, CheckpointError> {
    let raw = std::fs::read(replay_path)
        .map_err(|error| CheckpointError::ReadReplay(error.to_string()))?;
    let payload = load_payload(replay_path)
        .map_err(|error| CheckpointError::ReadPayload(error.to_string()))?;
    if replay.path.as_path() != replay_path || replay.payload_len != payload.len() {
        return Err(CheckpointError::ReplayIdentityDrift);
    }
    let turn = replay
        .turns
        .iter()
        .find(|turn| turn.turn == group)
        .ok_or(CheckpointError::MissingTurn(group))?;
    let mut peers = Vec::new();
    for player in &turn.players {
        let packets: Vec<&[u8]> = player
            .commands
            .iter()
            .filter(|command| command.opcode == CHECKSUM_OPCODE)
            .map(|command| command.bytes.as_slice())
            .collect();
        if packets.is_empty() {
            continue;
        }
        if packets.len() != 1 {
            return Err(CheckpointError::DuplicatePeerPacket(player.play));
        }
        let packet = packets[0];
        let channels = decode_checksum_packet(packet).ok_or(CheckpointError::MalformedPacket {
            play: player.play,
            bytes: packet.len(),
        })?;
        if player.checksums != Some(channels) {
            return Err(CheckpointError::DecodedTupleDrift(player.play));
        }
        if !channels.total_is_consistent() || !channels.adler_shaped() {
            return Err(CheckpointError::InvalidTuple(player.play));
        }
        let mut packet_evidence = Vec::with_capacity(12 + packet.len());
        packet_evidence.extend_from_slice(&group.to_le_bytes());
        packet_evidence.extend_from_slice(&player.play.to_le_bytes());
        packet_evidence.extend_from_slice(&player.stamp.to_le_bytes());
        packet_evidence.extend_from_slice(packet);
        if peers
            .iter()
            .any(|peer: &PeerPacketEvidence| peer.play == player.play)
        {
            return Err(CheckpointError::DuplicatePeerPacket(player.play));
        }
        peers.push(PeerPacketEvidence {
            play: player.play,
            stamp: player.stamp,
            packet_sha256: sha256(packet),
            packet_evidence_sha256: sha256(&packet_evidence),
            channels,
        });
    }
    peers.sort_by_key(|peer| peer.play);
    if peers.len() < 2 {
        return Err(CheckpointError::MissingPeerPackets);
    }
    let first = &peers[0];
    for peer in &peers[1..] {
        if peer.channels != first.channels {
            return Err(CheckpointError::PeerDisagreement {
                first: first.play,
                other: peer.play,
            });
        }
    }
    let world_checksum = peers[0].channels.get(Channel::World);
    Ok(WorldCheckpointEvidence {
        replay_name: replay_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("<non-utf8>")
            .to_owned(),
        replay_bytes: raw.len() as u64,
        replay_sha256: sha256(&raw),
        payload_bytes: payload.len() as u64,
        payload_sha256: sha256(&payload),
        version: replay.version.clone().unwrap_or_default(),
        group,
        world_checksum,
        peers,
    })
}

pub fn decode_checksum_packet(packet: &[u8]) -> Option<Channels> {
    if packet.len() != 1 + NUM_CHANNELS * 4 || packet[0] != CHECKSUM_OPCODE {
        return None;
    }
    let mut words = [0u32; NUM_CHANNELS];
    for (index, word) in words.iter_mut().enumerate() {
        let offset = 1 + index * 4;
        *word = u32::from_le_bytes(packet[offset..offset + 4].try_into().ok()?);
    }
    Some(Channels::from_recorded(words))
}

pub fn checkpoint_json(evidence: &WorldCheckpointEvidence) -> String {
    let mut out = String::with_capacity(4096);
    out.push_str("{\n  \"schema\": \"don.retail-world-checkpoint.v1\",");
    out.push_str(&format!(
        "\n  \"replay\": {{\"name\": \"{}\", \"bytes\": {}, \"sha256\": \"{}\", \"payload_bytes\": {}, \"payload_sha256\": \"{}\", \"version\": \"{}\"}},",
        json_escape(&evidence.replay_name),
        evidence.replay_bytes,
        hex(&evidence.replay_sha256),
        evidence.payload_bytes,
        hex(&evidence.payload_sha256),
        json_escape(&evidence.version),
    ));
    out.push_str(&format!(
        "\n  \"join\": {{\"key\": \"CommandPackage::group\", \"group\": {}, \"reporters\": {}, \"all_16_channels_identical\": true}},",
        evidence.group,
        evidence.peers.len(),
    ));
    out.push_str(&format!(
        "\n  \"world_checksum\": \"0x{:08x}\",\n  \"peers\": [",
        evidence.world_checksum,
    ));
    for (index, peer) in evidence.peers.iter().enumerate() {
        if index != 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "\n    {{\"play\": {}, \"stamp\": {}, \"packet_bytes\": 65, \"packet_sha256\": \"{}\", \"packet_evidence_sha256\": \"{}\", \"channels\": [",
            peer.play,
            peer.stamp,
            hex(&peer.packet_sha256),
            hex(&peer.packet_evidence_sha256),
        ));
        for (word_index, word) in peer.channels.0.iter().enumerate() {
            if word_index != 0 {
                out.push_str(", ");
            }
            out.push_str(&format!("\"0x{word:08x}\""));
        }
        out.push_str("]}");
    }
    out.push_str("\n  ],\n  \"byte_agreement_claimed\": false\n}\n");
    out
}

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

fn json_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if character < ' ' => {
                out.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => out.push(character),
        }
    }
    out
}

/// Dependency-free SHA-256.  This keeps packet/replay identity in the extractor
/// instead of trusting filenames or an adjacent checksum-only summary.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut state = [
        0x6a09e667u32,
        0xbb67ae85,
        0x3c6ef372,
        0xa54ff53a,
        0x510e527f,
        0x9b05688c,
        0x1f83d9ab,
        0x5be0cd19,
    ];
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = Vec::with_capacity((data.len() + 72) & !63);
    padded.extend_from_slice(data);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in padded.chunks_exact(64) {
        let mut words = [0u32; 64];
        for (index, word) in words[..16].iter_mut().enumerate() {
            let offset = index * 4;
            *word = u32::from_be_bytes(chunk[offset..offset + 4].try_into().unwrap());
        }
        for index in 16..64 {
            let s0 = words[index - 15].rotate_right(7)
                ^ words[index - 15].rotate_right(18)
                ^ (words[index - 15] >> 3);
            let s1 = words[index - 2].rotate_right(17)
                ^ words[index - 2].rotate_right(19)
                ^ (words[index - 2] >> 10);
            words[index] = words[index - 16]
                .wrapping_add(s0)
                .wrapping_add(words[index - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for index in 0..64 {
            let sum1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(sum1)
                .wrapping_add(choose)
                .wrapping_add(K[index])
                .wrapping_add(words[index]);
            let sum0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = sum0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }
    let mut out = [0u8; 32];
    for (index, value) in state.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    out
}
