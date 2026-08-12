//! Retail-backed packet fixtures for the strict `[Group][one simple action]` Sim host.

use don_replay::replay::{corpus, Replay};
use don_replay::world_owner_frontier::sha256;
use std::path::{Path, PathBuf};

const REPLAY_RELATIVE_PATH: &str = "ron-data/replays/Playback - 2026.08.11 11'44'38 (Tue).rcx";
const REPLAY_FILE_SHA256: &str = "558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54";
const STOP_SPELL_REPLAY_RELATIVE_PATH: &str =
    "ron-data/replays/multi/Playback___2017.07.20_20_46_23__Thu_.rcx";
const STOP_SPELL_REPLAY_SHA256: &str =
    "2c962b3607348784caec4ca0f95a1e6a43b28b2175c2db0dce8afb48ae425741";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Debug, PartialEq, Eq)]
struct Fixture {
    package_index0: usize,
    turn: i32,
    play: i32,
    frame: u32,
    opcodes: Vec<u8>,
    group_hex: String,
    action_hex: String,
}

#[test]
fn finished_replay_binds_five_group_unitmask_packets_and_one_strict_fixture() {
    let path = root().join(REPLAY_RELATIVE_PATH);
    if !path.exists() {
        eprintln!("SKIPPED — NOT A PASS. {} is absent", path.display());
        return;
    }
    assert_eq!(
        hex(&sha256(&std::fs::read(&path).unwrap())),
        REPLAY_FILE_SHA256
    );
    let replay = Replay::open(&path).unwrap();
    let mut fixtures = Vec::new();
    for (package_index0, turn) in replay.turns.iter().enumerate() {
        for player in &turn.players {
            for pair in player.commands.windows(2) {
                if pair[0].opcode == 0 && pair[1].opcode == 32 {
                    fixtures.push(Fixture {
                        package_index0,
                        turn: turn.turn,
                        play: player.play,
                        frame: player.stamp,
                        opcodes: player
                            .commands
                            .iter()
                            .map(|command| command.opcode)
                            .collect(),
                        group_hex: hex(&pair[0].bytes),
                        action_hex: hex(&pair[1].bytes),
                    });
                }
            }
        }
    }
    assert_eq!(
        fixtures,
        [
            Fixture {
                package_index0: 26,
                turn: 27,
                play: 0,
                frame: 25,
                opcodes: vec![72, 79, 0, 32],
                group_hex: "0001000200".into(),
                action_hex: "2000010000ffffffff".into(),
            },
            Fixture {
                package_index0: 52,
                turn: 53,
                play: 0,
                frame: 50,
                opcodes: vec![0, 32],
                group_hex: "0001000100".into(),
                action_hex: "2000010000ffffffff".into(),
            },
            Fixture {
                package_index0: 75,
                turn: 76,
                play: 0,
                frame: 72,
                opcodes: vec![0, 32],
                group_hex: "0001000000".into(),
                action_hex: "2000010000ffffffff".into(),
            },
            Fixture {
                package_index0: 5_913,
                turn: 5_914,
                play: 0,
                frame: 5_898,
                opcodes: vec![0, 32],
                group_hex: "0001000e00".into(),
                action_hex: "2000010000ffffffff".into(),
            },
            Fixture {
                package_index0: 7_057,
                turn: 7_058,
                play: 0,
                frame: 7_020,
                opcodes: vec![0, 32],
                group_hex: "0001000100".into(),
                action_hex: "2000010000ffffffff".into(),
            },
        ]
    );
}

#[test]
fn retail_replay_binds_stop_spell_explicit_and_cached_wires() {
    let path = root().join(STOP_SPELL_REPLAY_RELATIVE_PATH);
    if !path.exists() {
        eprintln!("SKIPPED — NOT A PASS. {} is absent", path.display());
        return;
    }
    assert_eq!(
        hex(&sha256(&std::fs::read(&path).unwrap())),
        STOP_SPELL_REPLAY_SHA256
    );
    let replay = Replay::open(&path).unwrap();
    let mut fixtures = Vec::new();
    for (package_index0, turn) in replay.turns.iter().enumerate() {
        for player in &turn.players {
            for pair in player.commands.windows(2) {
                if pair[0].opcode == 0 && pair[1].opcode == 29 {
                    fixtures.push(Fixture {
                        package_index0,
                        turn: turn.turn,
                        play: player.play,
                        frame: player.stamp,
                        opcodes: player
                            .commands
                            .iter()
                            .map(|command| command.opcode)
                            .collect(),
                        group_hex: hex(&pair[0].bytes),
                        action_hex: hex(&pair[1].bytes),
                    });
                }
            }
        }
    }
    assert_eq!(
        fixtures,
        [
            Fixture {
                package_index0: 9_162,
                turn: 9_163,
                play: 1,
                frame: 135_624,
                opcodes: vec![79, 0, 29, 0, 29, 0, 29, 58, 74, 72],
                group_hex: "00030207003800e300".into(),
                action_hex: "1d".into(),
            },
            Fixture {
                package_index0: 9_162,
                turn: 9_163,
                play: 1,
                frame: 135_624,
                opcodes: vec![79, 0, 29, 0, 29, 0, 29, 58, 74, 72],
                group_hex: "000002".into(),
                action_hex: "1d".into(),
            },
            Fixture {
                package_index0: 9_162,
                turn: 9_163,
                play: 1,
                frame: 135_624,
                opcodes: vec![79, 0, 29, 0, 29, 0, 29, 58, 74, 72],
                group_hex: "000002".into(),
                action_hex: "1d".into(),
            },
        ]
    );
}

#[test]
#[ignore = "full retail replay corpus"]
fn census_strict_group_unitmask_packets() {
    let mut found = Vec::new();
    for path in corpus(&root()) {
        let Ok(replay) = Replay::open(&path) else {
            continue;
        };
        for (turn_index, turn) in replay.turns.iter().enumerate() {
            for player in &turn.players {
                for pair in player.commands.windows(2) {
                    if pair[0].opcode == 0 && pair[1].opcode == 32 {
                        found.push((
                            path.clone(),
                            turn_index,
                            turn.turn,
                            player.play,
                            player.stamp,
                            player
                                .commands
                                .iter()
                                .map(|command| command.opcode)
                                .collect::<Vec<_>>(),
                            hex(&pair[0].bytes),
                            hex(&pair[1].bytes),
                        ));
                    }
                }
            }
        }
    }
    assert!(!found.is_empty());
    eprintln!("strict Group+UNITMASK occurrences={}", found.len());
    for fixture in found.iter().take(24) {
        eprintln!("fixture={fixture:?}");
    }
}

#[test]
#[ignore = "full retail replay corpus"]
fn census_strict_group_stop_spell_packets() {
    let mut found = Vec::new();
    for path in corpus(&root()) {
        let Ok(replay) = Replay::open(&path) else {
            continue;
        };
        for (turn_index, turn) in replay.turns.iter().enumerate() {
            for player in &turn.players {
                for pair in player.commands.windows(2) {
                    if pair[0].opcode == 0 && pair[1].opcode == 29 {
                        found.push((
                            path.clone(),
                            turn_index,
                            turn.turn,
                            player.play,
                            player.stamp,
                            player
                                .commands
                                .iter()
                                .map(|command| command.opcode)
                                .collect::<Vec<_>>(),
                            hex(&pair[0].bytes),
                            hex(&pair[1].bytes),
                        ));
                    }
                }
            }
        }
    }
    assert!(!found.is_empty());
    eprintln!("strict Group+STOP_SPELL occurrences={}", found.len());
    for fixture in found.iter().take(24) {
        eprintln!("fixture={fixture:?}");
    }
}
