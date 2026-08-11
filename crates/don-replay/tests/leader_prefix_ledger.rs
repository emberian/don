//! Coverage gates for the sparse Leader-prefix ledger and its report hook.

use don_replay::checksum::Channel;
use don_replay::harness::{self, NullSim, Phase};
use don_replay::initial::parse_initial_state;
use don_replay::leader_initial_prefix::{CHECKSUM_LEADER_SLOTS, FIXED_WALK_END};
use don_replay::leader_prefix_ledger::{
    LeaderPrefixCorpusCoverage, LeaderPrefixSpanLedger, LEADER_HEADER_BYTES,
};
use don_replay::replay::{corpus, load_payload, Replay};
use std::path::{Path, PathBuf};

const CHECKSUM_BEARING_BASELINE: [&str; 21] = [
    "Playback___2018.11.17_13_21_42__Sat_.rcx",
    "Playback___2018.12.01_18_33_16__Sat_.rcx",
    "Playback___2019.03.24_11_56_19__Sun_.rcx",
    "Playback___2020.02.08_10_49_15__Sat_.rcx",
    "Playback___2020.02.21_09_48_48__Fri_.rcx",
    "Playback___2020.07.25_19_30_12__Sat_.rcx",
    "Playback___2020.07.25_19_32_40__Sat_.rcx",
    "Playback___2020.07.25_19_42_43__Sat_.rcx",
    "Playback___2024.02.23_20_49_35__Fri_.rcx",
    "Playback___2024.02.23_21_38_38__Fri_.rcx",
    "Playback___2024.02.24_21_25_53__Sat_.rcx",
    "Playback___2024.03.10_20_54_34__Sun_.rcx",
    "Playback___2024.03.10_20_56_31__Sun_.rcx",
    "Playback___2024.03.17_19_58_17__Sun_.rcx",
    "Playback___2024.03.18_18_18_49__Mon_.rcx",
    "Playback___2024.03.20_17_28_53__Wed_.rcx",
    "Playback___2024.03.23_21_16_13__Sat_.rcx",
    "Playback___2024.03.29_21_52_57__Fri_.rcx",
    "Playback___2024.03.29_22_00_58__Fri_.rcx",
    "Playback___2024.04.10_17_05_19__Wed_.rcx",
    "Playback___2025.02.10_21_26_50__Mon_.rcx",
];

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

fn baseline_path(name: &str) -> PathBuf {
    repo_root().join("ron-data/replays/multi").join(name)
}

#[test]
fn the_ledger_preserves_only_issued_nonoverlapping_content_addressed_spans() {
    let files = corpus(&repo_root());
    let Some(path) = files.first() else {
        skip("ron-data/replays contains no .rcx files");
        return;
    };
    let replay = Replay::open(path).expect("corpus replay decodes");
    let ledger = LeaderPrefixSpanLedger::derive(&replay.initial).expect("prefix ledger derives");
    let coverage = ledger.coverage();

    assert_eq!(coverage.rows, CHECKSUM_LEADER_SLOTS);
    assert_eq!(coverage.active_rows + coverage.inactive_rows, coverage.rows);
    assert_eq!(
        coverage.exact_owned_checksum_bytes,
        8 + 704 * coverage.active_rows
    );
    assert_eq!(
        coverage.fixed_traversal_bytes,
        coverage.active_rows * FIXED_WALK_END + coverage.inactive_rows * LEADER_HEADER_BYTES
    );
    assert_eq!(
        coverage.unknown_fixed_traversal_bytes,
        coverage.fixed_traversal_bytes - coverage.exact_owned_checksum_bytes
    );
    assert_eq!(coverage.spans, 8 + 6 * coverage.active_rows);
    assert_eq!(coverage.provenance.init_rules_and_teams_flags, 8);
    assert_eq!(
        coverage.provenance.leader_init_identity,
        16 * coverage.active_rows
    );
    assert_eq!(
        coverage.provenance.leader_init_self_diplomacy,
        12 * coverage.active_rows
    );
    assert_eq!(
        coverage.provenance.leader_init_diplomacy_reset,
        676 * coverage.active_rows
    );
    assert_eq!(
        coverage.provenance.total(),
        coverage.exact_owned_checksum_bytes
    );
    assert_eq!(coverage.dynamic_children_owned_bytes, 0);
    assert!(!coverage.walk_complete);
    assert!(!coverage.exact_channel_producer);
    assert!(!coverage.substantive_scoreboard_eligible);

    for span in ledger.spans().iter().copied() {
        let bytes = ledger
            .owned_slice(span)
            .expect("ledger span recovers bytes");
        assert_eq!(bytes.len(), span.bytes());
        assert_eq!(
            ledger.owner_at(usize::from(span.slot), span.begin),
            Some(&span)
        );
    }
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        assert!(ledger.owner_at(slot, 0).is_some());
        assert!(ledger.owner_at(slot, 1).is_none());
    }

    let mut forged = ledger.spans()[0];
    forged.value_sha256[0] ^= 1;
    assert!(
        ledger.owned_slice(forged).is_none(),
        "a caller-forged content identity must not expose bytes"
    );
}

#[test]
fn the_62824_player_row_projection_collapses_to_61416_owned_bytes() {
    if !baseline_path(CHECKSUM_BEARING_BASELINE[0]).is_file() {
        skip("the 21-file checksum-bearing replay baseline is not installed");
        return;
    }

    let mut aggregate = LeaderPrefixCorpusCoverage::default();
    for name in CHECKSUM_BEARING_BASELINE {
        let path = baseline_path(name);
        let payload = load_payload(&path)
            .unwrap_or_else(|error| panic!("{} did not decompress: {error}", path.display()));
        let initial = parse_initial_state(&payload)
            .unwrap_or_else(|error| panic!("{} did not parse: {error}", path.display()));
        let coverage = LeaderPrefixSpanLedger::derive(&initial)
            .unwrap_or_else(|error| panic!("{} refused its prefix: {error}", path.display()))
            .coverage();
        aggregate.observe(true, initial.active_players().count(), coverage);
    }

    assert_eq!(aggregate.files, 21);
    assert_eq!(aggregate.prefix_contributed_substantive_leaders_compares, 0);
    assert_eq!(aggregate.prefix_contributed_substantive_leaders_matches, 0);
    assert_eq!(aggregate.checksum_bearing_files, 21);
    assert_eq!(aggregate.checksum_bearing_present_player_rows, 89);
    assert_eq!(
        aggregate.checksum_bearing_player_row_projection_bytes,
        62_824
    );
    assert_eq!(aggregate.checksum_bearing_active_rows, 87);
    assert_eq!(
        aggregate.checksum_bearing_exact_owned_checksum_bytes,
        61_416
    );
    assert_eq!(aggregate.checksum_bearing_duplicate_who_collapsed_rows, 2);
    assert_eq!(
        aggregate.checksum_bearing_player_row_projection_overcount_bytes,
        1_408
    );
    assert_eq!(aggregate.checksum_bearing_human_only_candidates, 7);
    assert_eq!(aggregate.checksum_bearing_expired_by_nonhuman, 14);
}

#[test]
fn reporting_the_prefix_does_not_install_or_promote_the_leaders_channel() {
    let path = baseline_path(CHECKSUM_BEARING_BASELINE[0]);
    if !path.is_file() {
        skip("the checksum-bearing replay baseline is not installed");
        return;
    }
    let mut replay = Replay::open(&path).expect("baseline checksum replay decodes");
    let first = replay
        .turns
        .iter()
        .position(|turn| turn.any_checksums().is_some())
        .expect("checksum packet count has a checksum turn");
    replay.turns.truncate(first + 1);

    let mut sim = NullSim::new();
    let run = harness::run(&replay, &mut sim, Phase::BeforeCommands, 0);
    let coverage = run
        .initial_leader_prefix
        .expect("report carries sparse Leader coverage");
    assert!(coverage.exact_owned_checksum_bytes > 0);
    assert!(!coverage.substantive_scoreboard_eligible);

    let leaders = &run.channels[Channel::Leaders as usize];
    assert!(!leaders.our_installed);
    assert!(!leaders.our_exact_producer);
    assert_eq!(leaders.substantive_compares, 0);
    assert_eq!(leaders.substantive_matches, 0);

    let json = don_replay::report::to_json(&[run], "leader-prefix-ledger-test");
    assert!(json.contains("\"status\": \"sparse_exact_prefix\""));
    assert!(json.contains("\"substantive_scoreboard_eligible\": false"));
    assert!(json.contains("\"prefix_contributed_substantive_leaders_compares\": 0"));
    assert!(json.contains("\"prefix_contributed_substantive_leaders_matches\": 0"));
}
