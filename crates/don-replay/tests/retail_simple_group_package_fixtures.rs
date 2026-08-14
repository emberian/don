//! Retail-backed packet fixtures for the strict `[Group][one simple action]` Sim host.

use don_replay::replay::{corpus, Replay};
use don_replay::world_owner_frontier::sha256;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

const REPLAY_RELATIVE_PATH: &str = "ron-data/replays/Playback - 2026.08.11 11'44'38 (Tue).rcx";
const REPLAY_FILE_SHA256: &str = "558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54";
const SHIFT_FLIGHT_REPLAY_RELATIVE_PATH: &str =
    "ron-data/replays/multi/Playback___2018.11.17_13_21_42__Sat_.rcx";
const SHIFT_FLIGHT_REPLAY_SHA256: &str =
    "c006ecb860273605d2b48bf69f5dcb048596de5fc748aa664fa0a04452df2da0";
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
const AIR_TRIPLE_REPLAY_RELATIVE_PATH: &str =
    "ron-data/replays/multi/Playback___2018.12.01_18_33_16__Sat_.rcx";
const AIR_TRIPLE_REPLAY_SHA256: &str =
    "bc2c1f1a8bfb4b7e0d83a3f2ff69fb18ead1f2041507f6b3e864a1a069ee6089";

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
fn retail_replay_binds_explicit_then_cached_shift_flight_airbase_packet() {
    let path = root().join(SHIFT_FLIGHT_REPLAY_RELATIVE_PATH);
    if !path.exists() {
        eprintln!("SKIPPED — NOT A PASS. {} is absent", path.display());
        return;
    }
    assert_eq!(
        hex(&sha256(&std::fs::read(&path).unwrap())),
        SHIFT_FLIGHT_REPLAY_SHA256,
    );
    let replay = Replay::open(&path).unwrap();
    let turn = &replay.turns[17_886];
    let player = turn.players.iter().find(|player| player.play == 1).unwrap();
    assert_eq!((turn.turn, player.stamp), (17_887, 107_059));
    assert_eq!(
        player
            .commands
            .iter()
            .map(|command| command.opcode)
            .collect::<Vec<_>>(),
        [0, 28, 79, 0, 28, 57, 74, 72],
    );
    assert_eq!(
        player
            .commands
            .iter()
            .map(|command| hex(&command.bytes))
            .collect::<Vec<_>>(),
        [
            "00020043083c08",
            "1cfb070000010000000100000000000000000000000a000000",
            "4f0008010000010000",
            "000000",
            "1cfb070000010000000100000000000000000000000a000000",
            "39a02747dfb9fb6d7901000000c74931791cbec031840db3d7c82d9b844e918b93cec7ca859603722f895396350edc7fd30431ba1280259ff701000400574931bc",
            "4a48000e00000000000000",
            "48040001010050650000",
        ],
    );
}

#[test]
fn retail_replay_binds_explicit_unit_flight_to_unit_current_strafe_packet() {
    let path = root().join(FOLLOW_CACHE_REPLAY_RELATIVE_PATH);
    if !path.exists() {
        eprintln!("SKIPPED — NOT A PASS. {} is absent", path.display());
        return;
    }
    assert_eq!(
        hex(&sha256(&std::fs::read(&path).unwrap())),
        FOLLOW_CACHE_REPLAY_SHA256,
    );
    let replay = Replay::open(&path).unwrap();
    let turn = &replay.turns[15_605];
    let player = turn.players.iter().find(|player| player.play == 0).unwrap();
    assert_eq!((turn.turn, player.stamp), (15_606, 90_353));
    assert_eq!(
        player
            .commands
            .iter()
            .map(|command| command.opcode)
            .collect::<Vec<_>>(),
        [79, 0, 28, 58, 74, 72],
    );
    assert_eq!(
        player
            .commands
            .iter()
            .map(|command| hex(&command.bytes))
            .collect::<Vec<_>>(),
        [
            "4f0008000000000000",
            "000d01bb00c200f3004d005300c400c800ca00ff000001a0007c002f00",
            "1cb6000000030000000000000000000000000000000a000000",
            "3a0eec210adc",
            "4a48001100000000000000",
            "4804c4c800005d060100",
        ],
    );
}

#[test]
fn retail_replay_binds_cached_queue_new_patrol_then_flight_to_unit_packet() {
    let path = root().join(STOP_SPELL_REPLAY_RELATIVE_PATH);
    if !path.exists() {
        eprintln!("SKIPPED — NOT A PASS. {} is absent", path.display());
        return;
    }
    assert_eq!(
        hex(&sha256(&std::fs::read(&path).unwrap())),
        STOP_SPELL_REPLAY_SHA256,
    );
    let replay = Replay::open(&path).unwrap();
    let turn = &replay.turns[6_089];
    let player = turn.players.iter().find(|player| player.play == 0).unwrap();
    assert_eq!((turn.turn, player.stamp), (6_090, 111_040));
    assert_eq!(
        player
            .commands
            .iter()
            .map(|command| command.opcode)
            .collect::<Vec<_>>(),
        [79, 0, 10, 0, 28, 58, 74, 72],
    );
    assert_eq!(
        player
            .commands
            .iter()
            .map(|command| hex(&command.bytes))
            .collect::<Vec<_>>(),
        [
            "4f0008000000000000",
            "000001",
            "0a665500003ae4000002",
            "000001",
            "1c12000000040000000000000000000000000000000a000000",
            "3a06e30e93d2",
            "4a48001000000000000000",
            "48043459000029d70000",
        ],
    );
}

#[test]
fn retail_replay_binds_explicit_airbase_flight_to_unit_no_action_packet() {
    let path = root().join(REPLAY_RELATIVE_PATH);
    if !path.exists() {
        eprintln!("SKIPPED — NOT A PASS. {} is absent", path.display());
        return;
    }
    assert_eq!(
        hex(&sha256(&std::fs::read(&path).unwrap())),
        REPLAY_FILE_SHA256,
    );
    let replay = Replay::open(&path).unwrap();
    let turn = &replay.turns[39_549];
    let player = turn.players.iter().find(|player| player.play == 0).unwrap();
    assert_eq!((turn.turn, player.stamp), (39_550, 39_338));
    assert_eq!(
        player
            .commands
            .iter()
            .map(|command| command.opcode)
            .collect::<Vec<_>>(),
        [0, 28],
    );
    assert_eq!(
        player
            .commands
            .iter()
            .map(|command| hex(&command.bytes))
            .collect::<Vec<_>>(),
        [
            "0001001008",
            "1c9f000000030000000000000000000000000000000a000000",
        ],
    );
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
fn retail_replay_binds_the_unique_cached_patrol_then_flight_unit_to_build_package() {
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
    let turn = &replay.turns[10_869];
    let player = turn.players.iter().find(|player| player.play == 2).unwrap();
    assert_eq!((turn.turn, player.stamp), (10_870, 65_215));
    assert_eq!(
        player
            .commands
            .iter()
            .map(|command| command.opcode)
            .collect::<Vec<_>>(),
        [0, 10, 0, 28, 57, 74, 72],
    );
    assert_eq!(
        player
            .commands
            .iter()
            .map(|command| hex(&command.bytes))
            .collect::<Vec<_>>(),
        [
            "000005",
            "0acc0c01004534000001",
            "000005",
            "1c2c080000010000000000000000000000000000000a000000",
            "391e8b520b61cba8e901000000a457ee4a2db56e168965b7cce3503e37273aecb6cb22493301000000c5e3f22ab4d137630431ba12cf1dbf9601000400fd7a2b7c",
            "4a48001000000000000000",
            "4804cb0901009d280000",
        ],
    );
}

#[test]
fn retail_replay_binds_three_cached_launch_patrol_pairs_in_one_package() {
    let path = root().join(AIR_TRIPLE_REPLAY_RELATIVE_PATH);
    if !path.exists() {
        eprintln!("SKIPPED — NOT A PASS. {} is absent", path.display());
        return;
    }
    assert_eq!(
        hex(&sha256(&std::fs::read(&path).unwrap())),
        AIR_TRIPLE_REPLAY_SHA256,
    );
    let replay = Replay::open(&path).unwrap();
    let turn = &replay.turns[8_852];
    let player = turn.players.iter().find(|player| player.play == 1).unwrap();
    assert_eq!((turn.turn, player.stamp), (8_853, 53_053));
    assert_eq!(
        player
            .commands
            .iter()
            .map(|command| command.opcode)
            .collect::<Vec<_>>(),
        [79, 0, 11, 0, 11, 0, 11, 57, 74, 72],
    );
    assert_eq!(
        player
            .commands
            .iter()
            .map(|command| hex(&command.bytes))
            .collect::<Vec<_>>(),
        [
            "4f0008030000030000",
            "000002",
            "0b3d9700003293000002000000000000000000000000000000",
            "000002",
            "0b3d9700003293000002000000000000000000000000000000",
            "000002",
            "0b3d9700003293000002000000000000000000000000000000",
            "3973a3bbc3e077860b01000000d57d28869bce92f336b1e1442dcfc45b842dde4b68127735010000009ce18c5f3887b6a70431ba12db1c2db801000400c8de273d",
            "4a48001000000000000000",
            "48049a940000398a0000",
        ],
    );
}

#[test]
fn finished_replay_binds_cached_airbase_flight_to_build_and_explicit_origin() {
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

    let origin = &replay.turns[54_537];
    let origin_player = origin
        .players
        .iter()
        .find(|player| player.play == 0)
        .unwrap();
    assert_eq!((origin.turn, origin_player.stamp), (54_538, 54_133));
    assert_eq!(
        origin_player
            .commands
            .iter()
            .map(|command| command.opcode)
            .collect::<Vec<_>>(),
        [0, 24],
    );
    assert_eq!(hex(&origin_player.commands[0].bytes), "0001001008");

    let turn = &replay.turns[54_616];
    let player = turn.players.iter().find(|player| player.play == 0).unwrap();
    assert_eq!((turn.turn, player.stamp), (54_617, 54_211));
    assert_eq!(
        player
            .commands
            .iter()
            .map(|command| command.opcode)
            .collect::<Vec<_>>(),
        [0, 28],
    );
    assert_eq!(
        player
            .commands
            .iter()
            .map(|command| hex(&command.bytes))
            .collect::<Vec<_>>(),
        [
            "000000",
            "1c21080000030000000000000000000000000000000a000000",
        ],
    );
}

#[test]
fn finished_replay_binds_unit_group_current_strafe_flight_fixture() {
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
    let turn = &replay.turns[44_294];
    let player = turn.players.iter().find(|player| player.play == 0).unwrap();
    assert_eq!((turn.turn, player.stamp), (44_295, 43_963));
    assert_eq!(
        player
            .commands
            .iter()
            .map(|command| command.opcode)
            .collect::<Vec<_>>(),
        [0, 28],
    );
    assert_eq!(
        player
            .commands
            .iter()
            .map(|command| hex(&command.bytes))
            .collect::<Vec<_>>(),
        [
            "0016000e001500400061009700bd0055007800840088009100a900b600bf0090008300ca00cb00cd00ce00cf00d000",
            "1c20080000000000000000000000000000000000000a000000",
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
fn retail_replay_binds_stance_explicit_origin_and_immediate_cache_reuse() {
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
    for (index, turn_number, frame, group_hex) in [
        (2_818usize, 2_819, 11_281, "0005011b00200027002a002d00"),
        (2_820usize, 2_821, 11_289, "000001"),
    ] {
        let turn = &replay.turns[index];
        let player = turn.players.iter().find(|player| player.play == 0).unwrap();
        let pair = player
            .commands
            .windows(2)
            .find(|pair| pair[0].opcode == 0 && pair[1].opcode == 2)
            .unwrap();
        assert_eq!((turn.turn, player.stamp), (turn_number, frame));
        assert_eq!(hex(&pair[0].bytes), group_hex);
        assert_eq!(hex(&pair[1].bytes), "02ffffffff");
    }
}

#[test]
#[ignore = "full retail replay corpus"]
fn census_flight_build_to_build_no_action_shell() {
    let mut count = 0usize;
    let mut explicit = 0usize;
    let mut cached = 0usize;
    let mut sizes = BTreeMap::new();
    let mut files = BTreeSet::new();
    for path in corpus(&root()) {
        let Ok(replay) = Replay::open(&path) else {
            continue;
        };
        let mut selections = HashMap::<i32, Vec<i16>>::new();
        for turn in &replay.turns {
            for player in &turn.players {
                let selection = selections.entry(player.play).or_default();
                for (index, group) in player.commands.iter().enumerate() {
                    if group.opcode != 0 || group.bytes.len() < 3 {
                        continue;
                    }
                    let members = usize::from(group.bytes[1]);
                    if group.bytes.len() != 3 + members * 2 {
                        continue;
                    }
                    let is_explicit = members != 0;
                    if is_explicit {
                        *selection = group.bytes[3..]
                            .chunks_exact(2)
                            .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
                            .collect();
                    }
                    let Some(flight) = player.commands.get(index + 1) else {
                        continue;
                    };
                    if flight.opcode != 28
                        || flight.bytes.len() != 25
                        || !selection
                            .iter()
                            .all(|&o| (2_000..3_000).contains(&i32::from(o)))
                        || selection.is_empty()
                        || !(2_000..3_000)
                            .contains(&i32::from_le_bytes(flight.bytes[1..5].try_into().unwrap()))
                        || [9, 13, 17].into_iter().any(|offset| {
                            i32::from_le_bytes(flight.bytes[offset..offset + 4].try_into().unwrap())
                                != 0
                        })
                        || i32::from_le_bytes(flight.bytes[21..25].try_into().unwrap()) != 10
                    {
                        continue;
                    }
                    let mut package_index = 0usize;
                    while package_index < player.commands.len() {
                        let opcode = player.commands[package_index].opcode;
                        if opcode == 0 {
                            assert!(player
                                .commands
                                .get(package_index + 1)
                                .is_some_and(|action| { matches!(action.opcode, 11 | 28 | 36) }));
                            package_index += 2;
                        } else {
                            assert!(matches!(opcode, 57 | 58 | 72 | 74 | 79));
                            package_index += 1;
                        }
                    }
                    assert!(!player
                        .commands
                        .iter()
                        .any(|command| matches!(command.opcode, 11 | 36)));
                    count += 1;
                    explicit += usize::from(is_explicit);
                    cached += usize::from(!is_explicit);
                    *sizes.entry(selection.len()).or_insert(0usize) += 1;
                    files.insert(path.clone());
                }
            }
        }
    }
    assert_eq!(count, 658);
    assert_eq!((explicit, cached, files.len()), (92, 566, 21));
    assert_eq!(
        sizes,
        BTreeMap::from([
            (1, 336),
            (2, 80),
            (3, 64),
            (4, 92),
            (5, 21),
            (6, 56),
            (7, 9)
        ])
    );
}

#[test]
#[ignore = "full retail replay corpus"]
fn census_flight_unit_to_build_current_strafe_wire_shell() {
    let mut count = 0usize;
    let mut explicit = 0usize;
    let mut cached = 0usize;
    let mut shell_admissible = 0usize;
    let mut air_bearing = 0usize;
    let mut sizes = BTreeMap::new();
    let mut files = BTreeSet::new();
    for path in corpus(&root()) {
        let Ok(replay) = Replay::open(&path) else {
            continue;
        };
        let mut selections = HashMap::<i32, Vec<i16>>::new();
        for turn in &replay.turns {
            for player in &turn.players {
                let selection = selections.entry(player.play).or_default();
                for (index, group) in player.commands.iter().enumerate() {
                    if group.opcode != 0 || group.bytes.len() < 3 {
                        continue;
                    }
                    let members = usize::from(group.bytes[1]);
                    if group.bytes.len() != 3 + members * 2 {
                        continue;
                    }
                    let is_explicit = members != 0;
                    if is_explicit {
                        *selection = group.bytes[3..]
                            .chunks_exact(2)
                            .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
                            .collect();
                    }
                    let Some(flight) = player.commands.get(index + 1) else {
                        continue;
                    };
                    if flight.opcode != 28
                        || flight.bytes.len() != 25
                        || selection.is_empty()
                        || !selection
                            .iter()
                            .all(|&o| (0..2_000).contains(&i32::from(o)))
                        || !(2_000..3_000)
                            .contains(&i32::from_le_bytes(flight.bytes[1..5].try_into().unwrap()))
                        || [9, 13, 17].into_iter().any(|offset| {
                            i32::from_le_bytes(flight.bytes[offset..offset + 4].try_into().unwrap())
                                != 0
                        })
                        || i32::from_le_bytes(flight.bytes[21..25].try_into().unwrap()) != 10
                    {
                        continue;
                    }
                    let mut package_index = 0usize;
                    let mut supported_shell = true;
                    while package_index < player.commands.len() {
                        let opcode = player.commands[package_index].opcode;
                        if opcode == 0 {
                            let action = player.commands.get(package_index + 1);
                            let ordinary_air =
                                action.is_some_and(|action| matches!(action.opcode, 11 | 28 | 36));
                            let bounded_patrol = action.is_some_and(|action| action.opcode == 10)
                                && player
                                    .commands
                                    .get(package_index + 2)
                                    .is_some_and(|group| group.opcode == 0)
                                && player
                                    .commands
                                    .get(package_index + 3)
                                    .is_some_and(|flight| flight.opcode == 28);
                            if !ordinary_air && !bounded_patrol {
                                supported_shell = false;
                                break;
                            }
                            package_index += 2;
                        } else if matches!(opcode, 57 | 58 | 72 | 74 | 79) {
                            package_index += 1;
                        } else {
                            supported_shell = false;
                            break;
                        }
                    }
                    count += 1;
                    explicit += usize::from(is_explicit);
                    cached += usize::from(!is_explicit);
                    shell_admissible += usize::from(supported_shell);
                    air_bearing += usize::from(
                        player
                            .commands
                            .iter()
                            .any(|command| matches!(command.opcode, 11 | 36)),
                    );
                    *sizes.entry(selection.len()).or_insert(0usize) += 1;
                    files.insert(path.clone());
                }
            }
        }
    }
    assert_eq!(count, 942);
    assert_eq!(
        (explicit, cached, shell_admissible, air_bearing, files.len()),
        (603, 339, 942, 0, 28)
    );
    assert_eq!(sizes.values().sum::<usize>(), 942);
    assert_eq!(
        (sizes.first_key_value(), sizes.last_key_value()),
        (Some((&1, &18)), Some((&128, &52)))
    );
}

#[test]
#[ignore = "full retail replay corpus"]
fn census_flight_build_to_unit_no_action_wire_shell() {
    let mut count = 0usize;
    let mut explicit = 0usize;
    let mut cached = 0usize;
    let mut shell_admissible = 0usize;
    let mut sizes = BTreeMap::new();
    let mut files = BTreeSet::new();
    for path in corpus(&root()) {
        let Ok(replay) = Replay::open(&path) else {
            continue;
        };
        let mut selections = HashMap::<i32, Vec<i16>>::new();
        for turn in &replay.turns {
            for player in &turn.players {
                let selection = selections.entry(player.play).or_default();
                for (index, group) in player.commands.iter().enumerate() {
                    if group.opcode != 0 || group.bytes.len() < 3 {
                        continue;
                    }
                    let members = usize::from(group.bytes[1]);
                    if group.bytes.len() != 3 + members * 2 {
                        continue;
                    }
                    let is_explicit = members != 0;
                    if is_explicit {
                        *selection = group.bytes[3..]
                            .chunks_exact(2)
                            .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
                            .collect();
                    }
                    let Some(flight) = player.commands.get(index + 1) else {
                        continue;
                    };
                    if flight.opcode != 28
                        || flight.bytes.len() != 25
                        || selection.is_empty()
                        || !selection
                            .iter()
                            .all(|&o| (2_000..3_000).contains(&i32::from(o)))
                        || !(0..2_000)
                            .contains(&i32::from_le_bytes(flight.bytes[1..5].try_into().unwrap()))
                        || [9, 13, 17].into_iter().any(|offset| {
                            i32::from_le_bytes(flight.bytes[offset..offset + 4].try_into().unwrap())
                                != 0
                        })
                        || i32::from_le_bytes(flight.bytes[21..25].try_into().unwrap()) != 10
                    {
                        continue;
                    }
                    let mut package_index = 0usize;
                    let mut supported_shell = true;
                    while package_index < player.commands.len() {
                        let opcode = player.commands[package_index].opcode;
                        if opcode == 0 {
                            if !player
                                .commands
                                .get(package_index + 1)
                                .is_some_and(|action| matches!(action.opcode, 11 | 28 | 36))
                            {
                                supported_shell = false;
                                break;
                            }
                            package_index += 2;
                        } else if matches!(opcode, 57 | 58 | 72 | 74 | 79) {
                            package_index += 1;
                        } else {
                            supported_shell = false;
                            break;
                        }
                    }
                    count += 1;
                    explicit += usize::from(is_explicit);
                    cached += usize::from(!is_explicit);
                    shell_admissible += usize::from(supported_shell);
                    *sizes.entry(selection.len()).or_insert(0usize) += 1;
                    files.insert(path.clone());
                }
            }
        }
    }
    assert_eq!(count, 301);
    assert_eq!(
        (explicit, cached, shell_admissible, files.len()),
        (29, 272, 301, 17)
    );
    assert_eq!(
        sizes,
        BTreeMap::from([(1, 41), (2, 21), (3, 48), (4, 147), (6, 37), (7, 6), (8, 1)])
    );
}

#[test]
#[ignore = "full retail replay corpus"]
fn census_flight_unit_to_unit_exact_wire_shell() {
    let mut count = 0usize;
    let mut explicit = 0usize;
    let mut cached = 0usize;
    let mut shell_admissible = 0usize;
    let mut patrol_flight = 0usize;
    let mut sizes = BTreeMap::new();
    let mut files = BTreeSet::new();
    for path in corpus(&root()) {
        let Ok(replay) = Replay::open(&path) else {
            continue;
        };
        let mut selections = HashMap::<i32, Vec<i16>>::new();
        for turn in &replay.turns {
            for player in &turn.players {
                let selection = selections.entry(player.play).or_default();
                for (index, group) in player.commands.iter().enumerate() {
                    if group.opcode != 0 || group.bytes.len() < 3 {
                        continue;
                    }
                    let members = usize::from(group.bytes[1]);
                    if group.bytes.len() != 3 + members * 2 {
                        continue;
                    }
                    let is_explicit = members != 0;
                    if is_explicit {
                        *selection = group.bytes[3..]
                            .chunks_exact(2)
                            .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
                            .collect();
                    }
                    let Some(flight) = player.commands.get(index + 1) else {
                        continue;
                    };
                    if flight.opcode != 28
                        || flight.bytes.len() != 25
                        || selection.is_empty()
                        || !selection
                            .iter()
                            .all(|&o| (0..2_000).contains(&i32::from(o)))
                        || !(0..2_000)
                            .contains(&i32::from_le_bytes(flight.bytes[1..5].try_into().unwrap()))
                        || [9, 13, 17].into_iter().any(|offset| {
                            i32::from_le_bytes(flight.bytes[offset..offset + 4].try_into().unwrap())
                                != 0
                        })
                        || i32::from_le_bytes(flight.bytes[21..25].try_into().unwrap()) != 10
                    {
                        continue;
                    }
                    let mut package_index = 0usize;
                    let mut supported_shell = true;
                    while package_index < player.commands.len() {
                        let opcode = player.commands[package_index].opcode;
                        if opcode == 0 {
                            let action = player.commands.get(package_index + 1);
                            let ordinary_air =
                                action.is_some_and(|action| matches!(action.opcode, 11 | 28 | 36));
                            let bounded_patrol = action.is_some_and(|action| action.opcode == 10)
                                && player
                                    .commands
                                    .get(package_index + 2)
                                    .is_some_and(|group| group.opcode == 0)
                                && player
                                    .commands
                                    .get(package_index + 3)
                                    .is_some_and(|flight| flight.opcode == 28);
                            if !ordinary_air && !bounded_patrol {
                                supported_shell = false;
                                break;
                            }
                            package_index += 2;
                        } else if matches!(opcode, 57 | 58 | 72 | 74 | 79) {
                            package_index += 1;
                        } else {
                            supported_shell = false;
                            break;
                        }
                    }
                    count += 1;
                    explicit += usize::from(is_explicit);
                    cached += usize::from(!is_explicit);
                    shell_admissible += usize::from(supported_shell);
                    patrol_flight += usize::from(
                        index >= 2
                            && player.commands[index - 2].opcode == 0
                            && player.commands[index - 1].opcode == 10,
                    );
                    *sizes.entry(selection.len()).or_insert(0usize) += 1;
                    files.insert(path.clone());
                }
            }
        }
    }
    assert_eq!(count, 263);
    assert_eq!(
        (
            explicit,
            cached,
            shell_admissible,
            patrol_flight,
            files.len()
        ),
        (155, 108, 263, 1, 25)
    );
    assert_eq!(sizes.values().sum::<usize>(), 263);
    assert_eq!(
        (sizes.first_key_value(), sizes.last_key_value()),
        (Some((&1, &6)), Some((&128, &15)))
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
fn census_residual_flight_shapes_and_shift_airbase_shell() {
    let mut residual = BTreeMap::<(i32, i32, i32, i32, &'static str, &'static str), usize>::new();
    let mut shift_airbase = 0usize;
    let mut explicit = 0usize;
    let mut cached = 0usize;
    let mut shell_admissible = 0usize;
    let mut sizes = BTreeMap::new();
    let mut files = BTreeSet::new();
    for path in corpus(&root()) {
        let Ok(replay) = Replay::open(&path) else {
            continue;
        };
        let mut selections = HashMap::<i32, Vec<i16>>::new();
        for turn in &replay.turns {
            for player in &turn.players {
                let selection = selections.entry(player.play).or_default();
                for (index, group) in player.commands.iter().enumerate() {
                    if group.opcode != 0 || group.bytes.len() < 3 {
                        continue;
                    }
                    let members = usize::from(group.bytes[1]);
                    if group.bytes.len() != 3 + members * 2 {
                        continue;
                    }
                    let is_explicit = members != 0;
                    if is_explicit {
                        *selection = group.bytes[3..]
                            .chunks_exact(2)
                            .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
                            .collect();
                    }
                    let Some(flight) = player.commands.get(index + 1) else {
                        continue;
                    };
                    if flight.opcode != 28 || flight.bytes.len() != 25 {
                        continue;
                    }
                    let field = |offset| {
                        i32::from_le_bytes(flight.bytes[offset..offset + 4].try_into().unwrap())
                    };
                    let (shift, ctrl, alt, orders) = (field(9), field(13), field(17), field(21));
                    if orders == 10 && shift == 0 && ctrl == 0 && alt == 0 {
                        continue;
                    }
                    let selection_band = if selection.is_empty() {
                        "Empty"
                    } else if selection
                        .iter()
                        .all(|&o| (0..2_000).contains(&i32::from(o)))
                    {
                        "Unit"
                    } else if selection
                        .iter()
                        .all(|&o| (2_000..3_000).contains(&i32::from(o)))
                    {
                        "Build"
                    } else {
                        "Mixed"
                    };
                    let target_o = field(1);
                    let target_band = if (0..2_000).contains(&target_o) {
                        "Unit"
                    } else if (2_000..3_000).contains(&target_o) {
                        "Build"
                    } else {
                        "Other"
                    };
                    *residual
                        .entry((orders, shift, ctrl, alt, selection_band, target_band))
                        .or_default() += 1;
                    if (orders, shift, ctrl, alt, selection_band, target_band)
                        != (10, 1, 0, 0, "Build", "Build")
                    {
                        continue;
                    }
                    let mut package_index = 0usize;
                    let mut supported_shell = true;
                    while package_index < player.commands.len() {
                        let opcode = player.commands[package_index].opcode;
                        if opcode == 0 {
                            let action = player.commands.get(package_index + 1);
                            if !action.is_some_and(|action| matches!(action.opcode, 11 | 28 | 36)) {
                                supported_shell = false;
                                break;
                            }
                            package_index += 2;
                        } else if matches!(opcode, 57 | 58 | 72 | 74 | 79) {
                            package_index += 1;
                        } else {
                            supported_shell = false;
                            break;
                        }
                    }
                    shift_airbase += 1;
                    explicit += usize::from(is_explicit);
                    cached += usize::from(!is_explicit);
                    shell_admissible += usize::from(supported_shell);
                    *sizes.entry(selection.len()).or_insert(0usize) += 1;
                    files.insert(path.clone());
                }
            }
        }
    }
    assert_eq!(residual.values().sum::<usize>(), 41);
    assert_eq!(
        residual,
        BTreeMap::from([
            ((1, 0, 0, 0, "Build", "Build"), 5),
            ((1, 0, 0, 0, "Unit", "Build"), 13),
            ((10, 1, 0, 0, "Build", "Build"), 22),
            ((10, 1, 0, 0, "Unit", "Build"), 1),
        ]),
    );
    assert_eq!(
        (
            shift_airbase,
            explicit,
            cached,
            shell_admissible,
            files.len()
        ),
        (22, 2, 20, 22, 3),
    );
    assert_eq!(sizes, BTreeMap::from([(1, 1), (2, 10), (4, 11)]));
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
