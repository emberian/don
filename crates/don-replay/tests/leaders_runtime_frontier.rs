//! Exactness, refusal, and corpus gates for the live Leaders checksum frontier.

mod leader_initial_prefix {
    pub use don_replay::leader_initial_prefix::*;
}

#[path = "../src/leaders_runtime_frontier.rs"]
mod leaders_runtime_frontier;

use don_replay::checksum::Channel;
use don_replay::leader_initial_prefix::{derive, InitialLeaderPrefix};
use don_replay::replay::{corpus, Replay};
use don_sim::systems::{leaders, victory_score};
use leaders_runtime_frontier::{
    bind_live, LeadersRuntimeError, LeadersWalkBoundary, RuntimeCoveredRange,
    LEADER_DATA_ENCRYPT_DWORDS, LEADER_WALK_DATA_VA,
};
use std::collections::BTreeSet;
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

fn first_prefix() -> Option<InitialLeaderPrefix> {
    for path in corpus(&repo_root()) {
        let Ok(replay) = Replay::open(&path) else {
            continue;
        };
        if let Ok(prefix) = derive(&replay.initial) {
            return Some(prefix);
        }
    }
    None
}

fn current_states(prefix: &InitialLeaderPrefix) -> (victory_score::Leaders, leaders::Leaders) {
    let types = victory_score::TypeTable::with_default_kinds(Default::default());
    let mut victory = victory_score::Leaders::new(types);
    let mut step8 = leaders::Leaders::new();
    for slot in 0..8 {
        let mut flags = 0i32;
        if prefix.rows[slot].active {
            flags |= victory_score::leader_flag::VALID | victory_score::leader_flag::ACTIVE;
        }
        if prefix.rows[slot].human {
            flags |= victory_score::leader_flag::HUMAN;
        }
        victory.slots[slot].leader_flags = flags;
        step8.leaders[slot].flags = flags as u32;
    }
    (victory, step8)
}

#[test]
fn live_projection_keeps_setup_and_runtime_ownership_separate() {
    let Some(prefix) = first_prefix() else {
        skip("ron-data/replays contains no derivable replay");
        return;
    };
    let (victory, step8) = current_states(&prefix);
    let runtime = bind_live(&prefix, &victory, &step8).expect("coherent duplicate owners bind");
    assert_eq!(LEADER_WALK_DATA_VA, 0x006d_6750);

    assert_eq!(
        runtime.setup_claimed_walked_bytes,
        prefix.claimed_walked_bytes()
    );
    assert!(runtime.runtime_claimed_walked_bytes() > runtime.setup_claimed_walked_bytes);

    let first_active = prefix
        .rows
        .iter()
        .position(|row| row.active)
        .expect("a recorded match has an active Leader");
    let row = &runtime.rows[first_active];
    let claim_bytes = row
        .claims()
        .iter()
        .map(|claim| claim.bytes())
        .sum::<usize>();
    let covered_bytes = row
        .covered_ranges()
        .iter()
        .map(|range| range.bytes())
        .sum::<usize>();
    eprintln!(
        "  first active row: {claim_bytes} claimed writes / {covered_bytes} unique image bytes"
    );
    assert!(claim_bytes > 4_000);
    assert!(covered_bytes > 4_000);
    assert_eq!(row.first_unowned_fixed_offset(), Some(0x0c));
    assert_eq!(
        row.owned_slice(RuntimeCoveredRange { begin: 0, end: 12 })
            .expect("current header and who are owned")
            .len(),
        12
    );
    assert!(row
        .owned_slice(RuntimeCoveredRange { begin: 12, end: 16 })
        .is_none());
    assert_eq!(
        row.econ_plaintext()
            .iter()
            .filter(|value| value.is_some())
            .count(),
        49
    );
    assert_eq!(row.econ_plaintext().len(), LEADER_DATA_ENCRYPT_DWORDS);
    eprintln!(
        "  current Leaders ownership: {} bytes across eight rows; first active row {} bytes",
        runtime.runtime_claimed_walked_bytes(),
        row.runtime_claimed_walked_bytes()
    );

    let frontier = runtime.walk_frontier();
    assert_eq!(
        frontier.boundary,
        LeadersWalkBoundary::FixedBody {
            slot: first_active,
            offset: 0x0c,
        }
    );
    assert_eq!(frontier.bytes_walked, (first_active * 8 + 12) as u64);
    assert_eq!(runtime.checksum(), Err(frontier));
}

#[test]
fn duplicate_runtime_owners_disagree_fail_closed_at_the_exact_byte() {
    let Some(prefix) = first_prefix() else {
        skip("ron-data/replays contains no derivable replay");
        return;
    };
    let (victory, mut step8) = current_states(&prefix);
    let slot = prefix
        .rows
        .iter()
        .position(|row| row.active)
        .expect("a recorded match has an active Leader");
    step8.leaders[slot].flags ^= leaders::flag::PENDING;

    let error = bind_live(&prefix, &victory, &step8).unwrap_err();
    assert!(matches!(
        error,
        LeadersRuntimeError::SourceDisagreement {
            slot: error_slot,
            offset: 2,
            ..
        } if error_slot == slot
    ));
}

#[test]
fn corpus_first_leaders_checkpoint_is_live_unique_and_never_empty() {
    let files = corpus(&repo_root());
    if files.is_empty() {
        skip("ron-data/replays contains no .rcx files");
        return;
    }

    let mut checksummed = 0usize;
    let mut unique = BTreeSet::new();
    let mut first_turns = BTreeSet::new();
    let mut setup_owned = 0usize;
    let mut active_rows = 0usize;
    let mut setup_derived = 0usize;
    let mut decoded = 0usize;
    let mut undecodable = 0usize;
    for path in &files {
        let replay = match Replay::open(path) {
            Ok(replay) => replay,
            Err(error) => {
                eprintln!("  excluded undecodable {}: {error}", path.display());
                undecodable += 1;
                continue;
            }
        };
        decoded += 1;
        if let Ok(prefix) = derive(&replay.initial) {
            setup_owned += prefix.claimed_walked_bytes();
            active_rows += prefix.active_mask.count_ones() as usize;
            setup_derived += 1;
        }

        let Some((turn, channels)) = replay.turns.iter().find_map(|turn| turn.any_checksums())
        else {
            continue;
        };
        let leaders = channels.get(Channel::Leaders);
        assert_ne!(
            leaders,
            1,
            "{} has an empty Leaders checkpoint",
            path.display()
        );
        checksummed += 1;
        unique.insert(leaders);
        first_turns.insert(turn);
    }

    eprintln!(
        "  live Leaders frontier: {decoded} decoded / {undecodable} excluded, \
         {checksummed} checksum-bearing recordings, {} unique first values, \
         first checkpoint turns {first_turns:?}; {setup_derived} setup snapshots derived, \
         {active_rows} setup-active rows, {setup_owned} setup-only bytes",
        unique.len(),
    );
    assert!(checksummed > 0);
    if checksummed == 21 {
        assert_eq!(checksummed, 21, "the full checksum-bearing cohort changed");
        assert_eq!(
            unique.len(),
            21,
            "first Leaders values stopped being replay-unique"
        );
    }
}
