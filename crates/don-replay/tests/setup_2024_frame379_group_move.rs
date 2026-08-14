use std::path::{Path, PathBuf};

use don_replay::replay::Replay;
use don_replay::setup_2024_frame379::{GROUP_MOVE_FRAME, GROUP_MOVE_SERIAL};
use don_replay::setup_2024_frame379_group_move::{
    discover_frame379_group_move_source, DESTINATION, PAIR_INDEX, PAIR_PLAY, SELECTED_CITIZENS,
};
use don_sim::systems::canonical_group_move_host::decode_group_move_package;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn real_2024_source_is_unique_and_has_exact_execution_frame_and_shell() {
    let path = repo_root().join("ron-data/replays/multi/Playback___2024.02.23_20_49_35__Fri_.rcx");
    let Ok(replay) = Replay::open(&path) else {
        eprintln!("SKIPPED -- NOT A PASS: missing {}", path.display());
        return;
    };
    let source = discover_frame379_group_move_source(&replay).unwrap();
    assert_eq!(
        (
            source.lockstep_serial,
            source.package_frame,
            source.play,
            source.pair_command_index,
        ),
        (GROUP_MOVE_SERIAL, GROUP_MOVE_FRAME, PAIR_PLAY, PAIR_INDEX)
    );
    assert_eq!(source.inert_opcodes, [0x4f, 0x39, 0x4a, 0x48]);
    assert!(source.unowned_sim_prefix.is_empty());
    assert!(source.unowned_sim_suffix.is_empty());
    let wire = decode_group_move_package(&source.command_bytes()).unwrap();
    assert_eq!(wire.who, 0);
    assert_eq!(wire.objects, SELECTED_CITIZENS);
    assert_eq!((wire.movement.x, wire.movement.y), DESTINATION);
}
