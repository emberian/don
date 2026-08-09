#[path = "../src/world_walk_checkpoint_frontier.rs"]
mod world_walk_checkpoint_frontier;

use don_replay::checksum::{Channel, Channels, NUM_CHANNELS, NUM_WALKED};
use world_walk_checkpoint_frontier::{
    checkpoint_json, decode_checksum_packet, hex, sha256, PeerPacketEvidence,
    WorldCheckpointEvidence,
};

fn packet(world: u32) -> Vec<u8> {
    let mut channels = Channels([1; NUM_CHANNELS]);
    channels.set(Channel::World, world);
    channels.set(Channel::All, channels.computed_total());
    assert_eq!(
        channels.computed_total(),
        channels.0[..NUM_WALKED].iter().copied().sum()
    );
    let mut packet = vec![0x39];
    for word in channels.0 {
        packet.extend_from_slice(&word.to_le_bytes());
    }
    packet
}

#[test]
fn exact_65_byte_packet_is_the_only_admitted_shape() {
    let packet = packet(0xd63a_3a53);
    let channels = decode_checksum_packet(&packet).expect("retail packet");
    assert_eq!(channels.get(Channel::World), 0xd63a_3a53);
    assert!(channels.total_is_consistent());
    assert!(decode_checksum_packet(&packet[..64]).is_none());
    let mut wrong_opcode = packet;
    wrong_opcode[0] = 0x38;
    assert!(decode_checksum_packet(&wrong_opcode).is_none());
}

#[test]
fn sha256_is_content_identity_not_a_filename_token() {
    assert_eq!(
        hex(&sha256(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_ne!(sha256(b"peer-0"), sha256(b"peer-1"));
}

#[test]
fn checkpoint_json_keeps_peer_packets_and_denies_byte_agreement() {
    let channels = decode_checksum_packet(&packet(0xd63a_3a53)).unwrap();
    let evidence = WorldCheckpointEvidence {
        replay_name: "Playback.rcx".to_owned(),
        replay_bytes: 42,
        replay_sha256: sha256(b"raw"),
        payload_bytes: 84,
        payload_sha256: sha256(b"payload"),
        version: "00.2024.06.20".to_owned(),
        group: 2,
        world_checksum: channels.get(Channel::World),
        peers: vec![
            PeerPacketEvidence {
                play: 0,
                stamp: 12,
                packet_sha256: sha256(b"packet-0"),
                packet_evidence_sha256: sha256(b"group-play-stamp-packet-0"),
                channels,
            },
            PeerPacketEvidence {
                play: 1,
                stamp: 13,
                packet_sha256: sha256(b"packet-1"),
                packet_evidence_sha256: sha256(b"group-play-stamp-packet-1"),
                channels,
            },
        ],
    };
    let json = checkpoint_json(&evidence);
    assert!(json.contains("\"key\": \"CommandPackage::group\""));
    assert!(json.contains("\"world_checksum\": \"0xd63a3a53\""));
    assert!(json.contains("\"reporters\": 2"));
    assert!(json.contains("\"byte_agreement_claimed\": false"));
}
