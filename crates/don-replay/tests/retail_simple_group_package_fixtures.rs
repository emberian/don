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
const HALT_REPLAY_RELATIVE_PATH: &str =
    "ron-data/replays/multi/Playback___2024.03.23_21_16_13__Sat_.rcx";
const HALT_REPLAY_SHA256: &str = "82bfb0898209d40afd887b1a487f565446cdb154d1192435e41a43137809355a";
const SET_TRANSPORT_REPLAY_RELATIVE_PATH: &str =
    "ron-data/replays/multi/Playback___2024.02.23_20_49_35__Fri_.rcx";
const SET_TRANSPORT_REPLAY_SHA256: &str =
    "1690431a5ef19b38a3425d3dd7311e8e83ca0d27c56fabe49d776a9f1421b251";
const FOLLOW_REPLAY_RELATIVE_PATH: &str =
    "ron-data/replays/multi/Playback___2024.03.18_18_18_49__Mon_.rcx";
const FOLLOW_REPLAY_SHA256: &str =
    "d27e34aa6ac40fbab3948a3f0f2bfbf35058fe463e42604e1e375b26467360f9";
const FOLLOW_CACHE_REPLAY_RELATIVE_PATH: &str =
    "ron-data/replays/multi/Playback___2017.07.15_00_10_31__Sat_.rcx";
const FOLLOW_CACHE_REPLAY_SHA256: &str =
    "c9180d5f82666dd6fad304527f65a6a39a97f2e59c9a562940d526c66f6d7f57";
const AIR_LAUNCH_REPLAY_RELATIVE_PATH: &str =
    "ron-data/replays/multi/Playback___2017.07.20_20_24_10__Thu_.rcx";
const AIR_LAUNCH_REPLAY_SHA256: &str =
    "e8c0103f21dbdb97ecd083c1899065209bdb055581daaceff0c3ee547100ef8d";
const AIR_FORCE_ALL_REPLAY_RELATIVE_PATH: &str =
    "ron-data/replays/multi/Playback___2019.03.24_11_56_19__Sun_.rcx";
const AIR_FORCE_ALL_REPLAY_SHA256: &str =
    "dab1c282556642300a5bc153f1f432f417fa039b265b4d72cb5876dd643ec055";

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
fn retail_replay_binds_build_launch_patrol_single_best_wire_and_shell_chronology() {
    let path = root().join(AIR_LAUNCH_REPLAY_RELATIVE_PATH);
    if !path.exists() {
        eprintln!("SKIPPED — NOT A PASS. {} is absent", path.display());
        return;
    }
    assert_eq!(
        hex(&sha256(&std::fs::read(&path).unwrap())),
        AIR_LAUNCH_REPLAY_SHA256,
    );
    let replay = Replay::open(&path).unwrap();
    let turn = &replay.turns[2_304];
    let player = turn.players.iter().find(|player| player.play == 1).unwrap();
    assert_eq!(turn.turn, 2_305);
    assert_eq!(player.stamp, 66_809);
    assert_eq!(
        player
            .commands
            .iter()
            .map(|command| command.opcode)
            .collect::<Vec<_>>(),
        [79, 0, 11, 58, 74, 72],
    );
    assert_eq!(
        player
            .commands
            .iter()
            .map(|command| hex(&command.bytes))
            .collect::<Vec<_>>(),
        [
            "4f0008000000000000",
            "0004022d082e082f083008",
            "0bebb20000a77d000002000000000000000000000000000000",
            "3a03ce217829",
            "4a48001000000000000000",
            "4804f7af00005c6f0000",
        ],
    );
}

#[test]
fn retail_replay_binds_the_unique_cached_force_all_launch_and_its_explicit_cache_origin() {
    let path = root().join(AIR_FORCE_ALL_REPLAY_RELATIVE_PATH);
    if !path.exists() {
        eprintln!("SKIPPED — NOT A PASS. {} is absent", path.display());
        return;
    }
    assert_eq!(
        hex(&sha256(&std::fs::read(&path).unwrap())),
        AIR_FORCE_ALL_REPLAY_SHA256,
    );
    let replay = Replay::open(&path).unwrap();

    let origin = &replay.turns[10_981];
    let origin_player = origin
        .players
        .iter()
        .find(|player| player.play == 2)
        .unwrap();
    assert_eq!((origin.turn, origin_player.stamp), (10_982, 65_887));
    assert_eq!(
        origin_player
            .commands
            .iter()
            .map(|command| command.opcode)
            .collect::<Vec<_>>(),
        [0, 28, 57, 74, 72],
    );
    assert_eq!(
        hex(&origin_player.commands[0].bytes),
        "0004052f08300831083208"
    );

    let turn = &replay.turns[10_988];
    let player = turn.players.iter().find(|player| player.play == 2).unwrap();
    assert_eq!((turn.turn, player.stamp), (10_989, 65_929));
    assert_eq!(
        player
            .commands
            .iter()
            .map(|command| command.opcode)
            .collect::<Vec<_>>(),
        [0, 11, 79, 57, 74, 72],
    );
    assert_eq!(
        player
            .commands
            .iter()
            .map(|command| hex(&command.bytes))
            .collect::<Vec<_>>(),
        [
            "000005",
            "0be90d01007523000001000000010000000000000000000000",
            "4f0008010000010000",
            "390a68becf7d2f714a01000000ca9d89bd7479fcd0d348ea71dd7f2b0b2c1d3cbd092a209101000000e5e3811761ae81b40431ba12e01deeb601000400d79fd709",
            "4a48001000000000000000",
            "4804460e01000d130000",
        ],
    );
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
fn retail_replay_binds_halt_explicit_and_persistent_cache_wires() {
    let path = root().join(HALT_REPLAY_RELATIVE_PATH);
    if !path.exists() {
        eprintln!("SKIPPED — NOT A PASS. {} is absent", path.display());
        return;
    }
    assert_eq!(
        hex(&sha256(&std::fs::read(&path).unwrap())),
        HALT_REPLAY_SHA256
    );
    let replay = Replay::open(&path).unwrap();
    let wanted = [8_864_usize, 9_574];
    let mut fixtures = Vec::new();
    for (package_index0, turn) in replay.turns.iter().enumerate() {
        if !wanted.contains(&package_index0) {
            continue;
        }
        for player in &turn.players {
            for pair in player.commands.windows(2) {
                if pair[0].opcode == 0 && pair[1].opcode == 12 {
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
                package_index0: 8_864,
                turn: 8_865,
                play: 1,
                frame: 35_465,
                opcodes: vec![79, 0, 12, 57, 74, 72],
                group_hex: "001800550074004200440045005e006900710075007d007c00800040004b005d0001000300050006000a000c005c006e008700".into(),
                action_hex: "0c".into(),
            },
            Fixture {
                package_index0: 9_574,
                turn: 9_575,
                play: 1,
                frame: 38_305,
                opcodes: vec![79, 0, 12, 57, 74, 72],
                group_hex: "000000".into(),
                action_hex: "0c".into(),
            },
        ]
    );
}

#[test]
fn retail_replay_binds_set_transport_explicit_wire() {
    let path = root().join(SET_TRANSPORT_REPLAY_RELATIVE_PATH);
    if !path.exists() {
        eprintln!("SKIPPED — NOT A PASS. {} is absent", path.display());
        return;
    }
    assert_eq!(
        hex(&sha256(&std::fs::read(&path).unwrap())),
        SET_TRANSPORT_REPLAY_SHA256
    );
    let replay = Replay::open(&path).unwrap();
    let mut fixtures = Vec::new();
    for (package_index0, turn) in replay.turns.iter().enumerate() {
        for player in &turn.players {
            for pair in player.commands.windows(2) {
                if pair[0].opcode == 0 && pair[1].opcode == 14 {
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
        [Fixture {
            package_index0: 118,
            turn: 119,
            play: 1,
            frame: 709,
            opcodes: vec![79, 0, 14, 57, 74, 72],
            group_hex: "0001010000".into(),
            action_hex: "0e01000000".into(),
        }]
    );
}

#[test]
fn retail_replay_binds_buildmask_explicit_and_persistent_cache_wires() {
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
    let wanted = [5_297_usize, 5_331];
    let mut fixtures = Vec::new();
    for (package_index0, turn) in replay.turns.iter().enumerate() {
        if !wanted.contains(&package_index0) {
            continue;
        }
        for player in &turn.players {
            for pair in player.commands.windows(2) {
                if pair[0].opcode == 0 && pair[1].opcode == 33 {
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
                package_index0: 5_297,
                turn: 5_298,
                play: 1,
                frame: 105_512,
                opcodes: vec![79, 0, 33, 0, 24, 58, 74, 72],
                group_hex: "0001023708".into(),
                action_hex: "214000000001000000".into(),
            },
            Fixture {
                package_index0: 5_331,
                turn: 5_332,
                play: 1,
                frame: 105_732,
                opcodes: vec![79, 0, 33, 58, 74, 72],
                group_hex: "000002".into(),
                action_hex: "214000000001000000".into(),
            },
        ]
    );
}

#[test]
fn retail_replays_bind_follow_queue_new_explicit_and_cached_wires() {
    let path = root().join(FOLLOW_REPLAY_RELATIVE_PATH);
    if !path.exists() {
        eprintln!("SKIPPED — NOT A PASS. {} is absent", path.display());
        return;
    }
    assert_eq!(
        hex(&sha256(&std::fs::read(&path).unwrap())),
        FOLLOW_REPLAY_SHA256
    );
    let replay = Replay::open(&path).unwrap();
    let turn = &replay.turns[7_353];
    let player = turn.players.iter().find(|player| player.play == 1).unwrap();
    let pair = player
        .commands
        .windows(2)
        .find(|pair| pair[0].opcode == 0 && pair[1].opcode == 30)
        .unwrap();
    assert_eq!(
        Fixture {
            package_index0: 7_353,
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
        },
        Fixture {
            package_index0: 7_353,
            turn: 7_354,
            play: 1,
            frame: 29_421,
            opcodes: vec![0, 30, 57, 74, 72],
            group_hex: "0001023e00".into(),
            action_hex: "1e0a0000000700000002000000".into(),
        }
    );

    let path = root().join(FOLLOW_CACHE_REPLAY_RELATIVE_PATH);
    if !path.exists() {
        eprintln!("SKIPPED — NOT A PASS. {} is absent", path.display());
        return;
    }
    assert_eq!(
        hex(&sha256(&std::fs::read(&path).unwrap())),
        FOLLOW_CACHE_REPLAY_SHA256
    );
    let replay = Replay::open(&path).unwrap();
    let turn = &replay.turns[9_656];
    let player = turn.players.iter().find(|player| player.play == 0).unwrap();
    let pair = player
        .commands
        .windows(2)
        .find(|pair| pair[0].opcode == 0 && pair[1].opcode == 30)
        .unwrap();
    assert_eq!(
        Fixture {
            package_index0: 9_656,
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
        },
        Fixture {
            package_index0: 9_656,
            turn: 9_657,
            play: 0,
            frame: 55_825,
            opcodes: vec![79, 0, 30, 58, 74, 72],
            group_hex: "000001".into(),
            action_hex: "1e2a0000000200000002000000".into(),
        }
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

#[test]
#[ignore = "full retail replay corpus"]
fn census_strict_group_halt_packets() {
    let mut found = Vec::new();
    for path in corpus(&root()) {
        let Ok(replay) = Replay::open(&path) else {
            continue;
        };
        for (turn_index, turn) in replay.turns.iter().enumerate() {
            for player in &turn.players {
                for pair in player.commands.windows(2) {
                    if pair[0].opcode == 0 && pair[1].opcode == 12 {
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
    eprintln!("strict Group+HALT occurrences={}", found.len());
    for fixture in found.iter().take(40) {
        eprintln!("fixture={fixture:?}");
    }
}

#[test]
#[ignore = "full retail replay corpus"]
fn census_strict_group_set_transport_packets() {
    let mut found = Vec::new();
    for path in corpus(&root()) {
        let Ok(replay) = Replay::open(&path) else {
            continue;
        };
        for (turn_index, turn) in replay.turns.iter().enumerate() {
            for player in &turn.players {
                for pair in player.commands.windows(2) {
                    if pair[0].opcode == 0 && pair[1].opcode == 14 {
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
    assert_eq!(found.len(), 5);
    eprintln!("strict Group+SET_TRANSPORT occurrences={}", found.len());
    for fixture in &found {
        eprintln!("fixture={fixture:?}");
    }
}

#[test]
#[ignore = "full retail replay corpus"]
fn census_strict_group_buildmask_packets() {
    let mut found = Vec::new();
    for path in corpus(&root()) {
        let Ok(replay) = Replay::open(&path) else {
            continue;
        };
        for (turn_index, turn) in replay.turns.iter().enumerate() {
            for player in &turn.players {
                for pair in player.commands.windows(2) {
                    if pair[0].opcode == 0 && pair[1].opcode == 33 {
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
    assert_eq!(found.len(), 329);
    eprintln!("strict Group+BUILDMASK occurrences={}", found.len());
    for fixture in found.iter().take(24) {
        eprintln!("fixture={fixture:?}");
    }
}

#[test]
#[ignore = "full retail replay corpus"]
fn census_strict_group_follow_packets() {
    let mut found = Vec::new();
    for path in corpus(&root()) {
        let Ok(replay) = Replay::open(&path) else {
            continue;
        };
        for (turn_index, turn) in replay.turns.iter().enumerate() {
            for player in &turn.players {
                for pair in player.commands.windows(2) {
                    if pair[0].opcode == 0 && pair[1].opcode == 30 {
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
    assert_eq!(found.len(), 7);
    eprintln!("strict Group+FOLLOW occurrences={}", found.len());
    for fixture in &found {
        eprintln!("fixture={fixture:?}");
    }
}
