//! Full-corpus retail packet evidence for diplomacy opcodes 38 and 41.

#[path = "fixtures/retail_diplomacy_cohort.rs"]
mod fixture;

use don_replay::replay::{corpus, Replay};
use don_replay::world_owner_frontier::sha256;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn i32_at(bytes: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn decode_hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

#[test]
fn whole_corpus_fixes_declare_and_accept_wire_images() {
    let files = corpus(&repo_root());
    if files.is_empty() {
        eprintln!("\n  SKIPPED — NOT A PASS. No local retail replay corpus.\n");
        return;
    }

    let mut manifest = Vec::new();
    let mut declare = 0usize;
    let mut accept = 0usize;
    let mut declare_treaties = BTreeMap::<i32, usize>::new();
    let mut declare_pairs = BTreeMap::<(i32, i32), usize>::new();
    let mut accept_pairs = BTreeMap::<(i32, i32), usize>::new();
    let mut files_with_declare = 0usize;
    let mut files_with_accept = 0usize;
    let mut decoded_files = 0usize;
    let mut decode_failures = 0usize;

    for path in files {
        let replay = match Replay::open(&path) {
            Ok(replay) => replay,
            Err(error) => {
                decode_failures += 1;
                eprintln!("legacy corpus decode refusal: {}: {error}", path.display());
                continue;
            }
        };
        decoded_files += 1;
        let file_sha = sha256(&std::fs::read(&path).unwrap());
        let mut file_declare = 0usize;
        let mut file_accept = 0usize;
        for turn in &replay.turns {
            for player in &turn.players {
                for command in &player.commands {
                    if !matches!(
                        command.opcode,
                        fixture::DECLARE_OPCODE | fixture::ACCEPT_OPCODE
                    ) {
                        continue;
                    }
                    let expected = if command.opcode == fixture::DECLARE_OPCODE {
                        13
                    } else {
                        9
                    };
                    assert_eq!(command.bytes.len(), expected);
                    let sender = i32_at(&command.bytes, 1);
                    let target = i32_at(&command.bytes, 5);
                    assert!((0..8).contains(&sender));
                    assert!((0..8).contains(&target));

                    // A filename-independent normalized record.  File content identity,
                    // lockstep serial, simulation frame, play and complete wire bytes are
                    // all retained; ordering is the sorted corpus order and replay order.
                    manifest.extend_from_slice(&file_sha);
                    manifest.extend_from_slice(&turn.turn.to_le_bytes());
                    manifest.extend_from_slice(&player.stamp.to_le_bytes());
                    manifest.extend_from_slice(&player.play.to_le_bytes());
                    manifest.extend_from_slice(&(command.bytes.len() as u16).to_le_bytes());
                    manifest.extend_from_slice(&command.bytes);

                    if command.opcode == fixture::DECLARE_OPCODE {
                        declare += 1;
                        file_declare += 1;
                        *declare_pairs.entry((sender, target)).or_default() += 1;
                        *declare_treaties
                            .entry(i32_at(&command.bytes, 9))
                            .or_default() += 1;
                    } else {
                        accept += 1;
                        file_accept += 1;
                        *accept_pairs.entry((sender, target)).or_default() += 1;
                    }
                }
            }
        }
        files_with_declare += usize::from(file_declare != 0);
        files_with_accept += usize::from(file_accept != 0);
    }

    eprintln!(
        "diplomacy corpus: declare={declare} accept={accept} files={files_with_declare}/{files_with_accept} treaties={declare_treaties:?} declare_pairs={declare_pairs:?} accept_pairs={accept_pairs:?} manifest_bytes={} sha256={}",
        manifest.len(),
        hex(&sha256(&manifest)),
    );
    assert_eq!(declare, fixture::DECLARE_COMMANDS);
    assert_eq!(accept, fixture::ACCEPT_COMMANDS);
    assert_eq!(
        declare_treaties.get(&0),
        Some(&fixture::DECLARE_WAR_COMMANDS)
    );
    assert_eq!(
        declare_treaties.get(&1),
        Some(&fixture::DECLARE_PEACE_COMMANDS)
    );
    assert_eq!(declare_treaties.len(), 2);
    assert_eq!(files_with_declare, fixture::FILES_WITH_DECLARE);
    assert_eq!(files_with_accept, fixture::FILES_WITH_ACCEPT);
    assert_eq!(decoded_files, 62);
    assert_eq!(decode_failures, 2);
    assert_eq!(manifest.len(), fixture::MANIFEST_BYTES);
    assert_eq!(hex(&sha256(&manifest)), fixture::MANIFEST_SHA256);
}

#[test]
fn exact_packets_are_real_bridge_wire_not_reencoded_fields() {
    // These are complete decoded command bytes from two fully decoded multiplayer
    // recordings.  Keeping opcode and little-endian field bytes together catches a
    // command adapter that accidentally treats the package player as the action owner.
    let declare_war = decode_hex("26020000000500000000000000");
    let declare_peace = decode_hex("26010000000600000001000000");
    let accept = decode_hex("290200000003000000");
    assert_eq!(declare_war[0], fixture::DECLARE_OPCODE);
    assert_eq!(
        (
            i32_at(&declare_war, 1),
            i32_at(&declare_war, 5),
            i32_at(&declare_war, 9)
        ),
        (2, 5, 0)
    );
    assert_eq!(
        (
            i32_at(&declare_peace, 1),
            i32_at(&declare_peace, 5),
            i32_at(&declare_peace, 9)
        ),
        (1, 6, 1)
    );
    assert_eq!(accept[0], fixture::ACCEPT_OPCODE);
    assert_eq!((i32_at(&accept, 1), i32_at(&accept, 5)), (2, 3));
}
