//! Corpus and replay-source tests for the sparse setup-time Leader producer.
//!
//! The recorded channel supplies only the final Adler-32, so these tests do not
//! compare a partial byte image with that word. They instead prove every replay
//! source span, bite a source byte, and measure the human/AI expiration split.

mod initial {
    pub use don_replay::initial::{InitialState, ReplayByteSpan};
}

#[path = "../src/leader_initial_prefix.rs"]
mod leader_initial_prefix;

use don_replay::checksum::Channel;
use don_replay::groups_channel::InitialGroupsChannel;
use don_replay::initial::parse_initial_state;
use don_replay::replay::{corpus, load_payload, Replay};
use leader_initial_prefix::{
    derive, LeaderPrefixProvenance, ACTIVE_OWNED_BYTES, INACTIVE_OWNED_BYTES,
};
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn skip(reason: &str) {
    eprintln!("\n  SKIPPED — NOT A PASS. {reason}\n  Nothing was established.\n");
}

fn identity(row: &leader_initial_prefix::InitialLeaderRow) -> &[u8] {
    let span = row
        .owned_spans()
        .iter()
        .copied()
        .find(|span| span.provenance == LeaderPrefixProvenance::LeaderInitIdentity)
        .expect("active row owns its identity");
    row.owned_slice(span).unwrap()
}

fn assert_sources(rep: &Replay, payload: &[u8]) {
    let prefix = derive(&rep.initial).expect("supported replay setup derives a Leader prefix");
    assert_eq!(prefix.sources.payload_sha256, rep.initial.payload_sha256);
    assert_eq!(prefix.sources.semaphore, rep.initial.game.semaphore);
    assert_eq!(
        prefix.claimed_walked_bytes(),
        8 + 704 * prefix.active_mask.count_ones() as usize
    );

    for (slot, player) in rep.initial.info.players.iter().enumerate() {
        let source = prefix.sources.players[slot];
        let gate = &payload[source.flag_gate.offset..source.flag_gate.end()];
        assert_eq!(gate, player.flags.to_le_bytes());
        match source.body {
            Some(body) => {
                let bytes = &payload[body.offset..body.end()];
                assert_eq!(bytes.len(), 0x39);
                assert_eq!(&bytes[0x30..0x32], &player.flags.to_le_bytes());
                assert_eq!(bytes[0x32], player.tribe);
                assert_eq!(bytes[0x33], player.who);
                assert_eq!(bytes[0x34], player.team);
            }
            None => assert!(!player.present),
        }
        assert_eq!(
            prefix.rows[slot].claimed_walked_bytes(),
            if prefix.rows[slot].active {
                ACTIVE_OWNED_BYTES
            } else {
                INACTIVE_OWNED_BYTES
            }
        );
    }
}

#[test]
fn a_replay_owned_tribe_byte_changes_the_claimed_identity() {
    let files = corpus(&repo_root());
    if files.is_empty() {
        skip("ron-data/replays contains no .rcx files");
        return;
    }

    let mut selected = None;
    for path in &files {
        let payload = load_payload(path).expect("corpus replay decompresses");
        let initial = parse_initial_state(&payload).expect("corpus replay has a Game prefix");
        let prefix = match derive(&initial) {
            Ok(prefix) => prefix,
            Err(_) => continue,
        };
        if let Some(leader_slot) = prefix
            .rows
            .iter()
            .position(|row| row.setup_player_slot.is_some())
        {
            selected = Some((payload, initial, prefix, leader_slot));
            break;
        }
    }
    let Some((mut payload, initial, before, leader_slot)) = selected else {
        panic!("no corpus replay contains a derivable active Leader");
    };
    let player_slot = before.rows[leader_slot].setup_player_slot.unwrap() as usize;
    let body = initial.worldgen_sources.player_bodies[player_slot].unwrap();
    payload[body.offset + 0x32] ^= 1;

    let mutated = parse_initial_state(&payload).expect("tribe mutation preserves the wire layout");
    let after = derive(&mutated).expect("mutated replay setup still derives");
    assert_ne!(before.sources.payload_sha256, after.sources.payload_sha256);
    assert_ne!(
        &identity(&before.rows[leader_slot])[4..8],
        &identity(&after.rows[leader_slot])[4..8]
    );
    assert_eq!(
        identity(&before.rows[leader_slot])[0..4],
        identity(&after.rows[leader_slot])[0..4]
    );
}

#[test]
fn the_whole_local_corpus_proves_sources_and_the_first_turn_expiration_split() {
    let files = corpus(&repo_root());
    if files.is_empty() {
        skip("ron-data/replays contains no .rcx files");
        return;
    }

    let game_init_groups = InitialGroupsChannel::derive().checksum;
    let mut opened = 0usize;
    let mut checksummed = 0usize;
    let mut human_only = 0usize;
    let mut with_nonhuman = 0usize;
    for path in &files {
        let rep = Replay::open(path).unwrap_or_else(|error| {
            panic!("{} did not decode: {error}", path.display());
        });
        let payload = load_payload(path).expect("decoded replay decompresses twice identically");
        assert_sources(&rep, &payload);
        opened += 1;

        let Some((_, first)) = rep.turns.iter().find_map(|turn| turn.any_checksums()) else {
            continue;
        };
        checksummed += 1;
        let prefix = derive(&rep.initial).unwrap();
        if prefix.is_human_only_first_checksum_candidate() {
            human_only += 1;
            assert_eq!(
                first.get(Channel::Groups),
                game_init_groups,
                "{}: a human-only first-turn candidate had already left the independent Game::init Groups image",
                path.display()
            );
        } else {
            with_nonhuman += 1;
        }
    }

    eprintln!(
        "  replay Leader prefix: {opened} decoded, {checksummed} checksum-bearing, \
         {human_only} human-only candidates, {with_nonhuman} expired-by-AI"
    );
    assert_eq!(opened, files.len());
    assert!(checksummed > 0);
    assert!(
        human_only > 0,
        "the corpus did not exercise the candidate side"
    );
    assert!(
        with_nonhuman > 0,
        "the corpus did not exercise the AI-expiration side"
    );
    if files.len() == 61 {
        assert_eq!(checksummed, 21, "the full 61-recording corpus changed");
        assert_eq!(human_only, 7, "the measured human-only cohort changed");
        assert_eq!(with_nonhuman, 14, "the measured AI cohort changed");
    }
}
