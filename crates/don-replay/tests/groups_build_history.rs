use don_replay::groups_build_history::{
    decode_queue_up_build, strict_group_build_history_before, GroupBuildHistoryError,
};
use don_replay::replay::{corpus, Replay};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf()
}

#[test]
fn strict_2018_pre_pair_history_is_eight_group_build_transactions() {
    let path = root().join("ron-data/replays/multi/Playback___2018.11.17_13_21_42__Sat_.rcx");
    let Ok(replay) = Replay::open(&path) else {
        eprintln!("SKIPPED -- NOT A PASS: missing {}", path.display());
        return;
    };
    let history = strict_group_build_history_before(&replay, 48).unwrap();
    let chronology: Vec<_> = history
        .iter()
        .map(|package| {
            (
                package.lockstep_serial,
                package.package_frame,
                package.play,
                package.command_index,
                package.who,
                package.objects.clone(),
                (
                    package.action.x,
                    package.action.y,
                    package.action.x2,
                    package.action.y2,
                ),
                package.action.type_index,
                package.action.queued,
                package.groups_checksum,
                package.shell_prefix.clone(),
                package.shell_suffix.clone(),
            )
        })
        .collect();
    assert_eq!(
        chronology,
        [
            (
                14,
                79,
                1,
                0,
                0,
                vec![4],
                (22286, 66184, 22286, 66184),
                0x1a1,
                1,
                0x1c78_f3f5,
                vec![],
                vec![0x4a, 0x48]
            ),
            (
                17,
                97,
                1,
                0,
                0,
                vec![],
                (22302, 66836, 22302, 66836),
                0x1a1,
                1,
                0x1118_f44b,
                vec![],
                vec![0x4f, 0x4a, 0x48]
            ),
            (
                20,
                115,
                1,
                1,
                0,
                vec![],
                (23056, 66959, 23056, 66959),
                0x1a1,
                1,
                0x119c_f484,
                vec![0x4f],
                vec![0x4a, 0x48]
            ),
            (
                23,
                133,
                0,
                0,
                1,
                vec![1, 2, 3, 4],
                (55125, 13459, 55125, 13459),
                0x1a1,
                1,
                0x119c_f484,
                vec![],
                vec![0x4a, 0x48]
            ),
            (
                23,
                133,
                1,
                1,
                0,
                vec![],
                (23071, 66166, 23071, 66166),
                0x1a1,
                1,
                0x119c_f484,
                vec![0x4f],
                vec![0x4a, 0x48]
            ),
            (
                26,
                151,
                1,
                0,
                0,
                vec![],
                (23078, 65389, 23078, 65389),
                0x1a1,
                1,
                0x48d2_f51a,
                vec![],
                vec![0x4a, 0x48]
            ),
            (
                27,
                157,
                0,
                0,
                1,
                vec![],
                (55173, 12720, 55173, 12720),
                0x1a1,
                1,
                0x48d2_f51a,
                vec![],
                vec![0x4a, 0x48]
            ),
            (
                32,
                187,
                0,
                1,
                1,
                vec![],
                (54388, 12788, 54347, 12786),
                0x1a1,
                1,
                0x53cb_f553,
                vec![0x4f],
                vec![0x4a, 0x48]
            ),
        ]
    );
    assert!(history.iter().all(|package| {
        package.command_bytes().len() == package.group_bytes().len() + 25
            && decode_queue_up_build(package.action_bytes()).unwrap() == package.action
    }));
}

#[test]
fn action_decoder_and_history_refuse_shape_or_unowned_sim_siblings() {
    assert_eq!(
        decode_queue_up_build(&[25; 24]),
        Err(GroupBuildHistoryError::WrongActionSize { got: 24 })
    );
    let path = root().join("ron-data/replays/multi/Playback___2018.11.17_13_21_42__Sat_.rcx");
    let Ok(mut replay) = Replay::open(&path) else {
        return;
    };
    let package = replay
        .turns
        .iter_mut()
        .find(|turn| turn.turn == 14)
        .unwrap()
        .players
        .iter_mut()
        .find(|player| player.play == 1)
        .unwrap();
    package.commands.push(don_replay::replay::OwnedCommand {
        opcode: 32,
        bytes: vec![32, 0, 0, 0, 0, 0, 0, 0, 0],
    });
    assert_eq!(
        strict_group_build_history_before(&replay, 48),
        Err(GroupBuildHistoryError::UnownedSimCommand { opcode: 32 })
    );
}

#[test]
fn local_checksum_corpus_contains_the_strict_history_without_widening_complex_packages() {
    let mut recordings = 0usize;
    let mut candidate_pairs = 0usize;
    let mut strict_pairs = 0usize;
    for path in corpus(&root()) {
        let Ok(replay) = Replay::open(&path) else {
            continue;
        };
        if replay.checksum_packets == 0 {
            continue;
        }
        recordings += 1;
        candidate_pairs += replay
            .turns
            .iter()
            .flat_map(|turn| &turn.players)
            .flat_map(|player| player.commands.windows(2))
            .filter(|pair| pair[0].opcode == 0 && pair[1].opcode == 25)
            .count();
        if path.file_name().and_then(|name| name.to_str())
            == Some("Playback___2018.11.17_13_21_42__Sat_.rcx")
        {
            strict_pairs = strict_group_build_history_before(&replay, 48)
                .expect("the pinned pre-pair history has no extra Sim siblings")
                .len();
        }
    }
    if recordings == 0 {
        eprintln!("SKIPPED -- NOT A PASS: no checksum-bearing local retail corpus");
        return;
    }
    assert!(
        candidate_pairs >= 8,
        "the checksum corpus lost the eight strict 2018 pre-pair transactions"
    );
    assert_eq!(strict_pairs, 8);
}
