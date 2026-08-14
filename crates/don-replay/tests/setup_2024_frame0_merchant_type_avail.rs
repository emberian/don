//! Exact replay-Rules evidence for the golden frame-zero Merchant Good availability leaf.

use std::path::{Path, PathBuf};

use don_replay::groups_pre_pair_unit_authority::replay_good_type_facts;
use don_replay::replay::{load_payload, Replay};

fn repo_root() -> PathBuf {
    if let Some(root) = std::env::var_os("DON_ROOT") {
        return PathBuf::from(root);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn golden_rare_goods_share_the_exact_early_available_rule_shape() {
    let path = repo_root().join("ron-data/replays/multi/Playback___2024.02.23_20_49_35__Fri_.rcx");
    if !path.is_file() {
        eprintln!("SKIPPED -- NOT A PASS: missing {}", path.display());
        return;
    }
    let replay = Replay::open(&path).expect("installed 2024 witness must decode");
    let payload = load_payload(&path).expect("installed 2024 witness payload");
    let rules = replay
        .initial
        .rules
        .expect("the golden replay carries admitted Rules");

    for type_index in 6..=49 {
        let facts = replay_good_type_facts(&payload, &rules, type_index).unwrap();
        assert_eq!(facts.type_index, type_index);
        assert_eq!(facts.tribe_mask, u32::MAX);
        assert_eq!(facts.preq, [-1, -1, -1]);
        assert_eq!(facts.obs, -2);
        assert_eq!(facts.from_type, -1);
        assert_eq!(facts.where_type, -1);
        assert_eq!(facts.upgrade, -1);
        assert_eq!(facts.jump, -2);
        assert_eq!(facts.spans.type_base.bytes, 90);
        assert_eq!(facts.spans.object.bytes, 152);
        assert_eq!(facts.spans.good.bytes, 68);
    }
}
