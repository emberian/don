//! Corpus boundary for replay-selected shipped BHS state.
//!
//! `ron-data` is gitignored copyrighted content.  These tests skip loudly when either
//! the recordings or the extracted source corpus is absent; a skip is not a pass.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// Keep this adapter testable as an exclusive overlay on a clean remote baseline: the
// production module uses crate-relative paths, so these two shims provide the same names
// when the source file is compiled directly by this integration test.
mod initial {
    pub use don_replay::initial::*;
}
mod script_channel {
    pub use don_replay::script_channel::*;
}
#[path = "../src/replay_bhs_runtime.rs"]
mod replay_bhs_runtime;

use don_replay::checksum::Channel;
use don_replay::replay::Replay;
use don_replay::script_channel::{checksum_program, EMPTY_RUNTIME_CHANNEL};
use replay_bhs_runtime::{load_replay_bhs_program, ReplayBhsSelection, LEADER_FLAG_HUMAN};

const LOADED_IMAGE_CHANNEL: u32 = 0x6a25_ab2b;
const LOADED_IMAGE_BYTES: u64 = 42_177;

/// Frozen checksum-bearing subset from `schema/replay-validation.json` (61 files total).
/// Selecting these by independently recorded packet presence avoids spending the test on
/// the 40 recordings which cannot falsify a checksum producer.
const CHECKSUM_RECORDINGS: [&str; 21] = [
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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn shipped_bhs_root() -> Option<PathBuf> {
    let root = repo_root().join("ron-data/bhs-corpus");
    root.is_dir().then_some(root)
}

fn skip(reason: &str) {
    eprintln!("\n  SKIPPED — NOT A PASS. {reason}\n  Nothing was established.\n");
}

fn first_recorded(rep: &Replay, channel: Channel) -> Option<u32> {
    rep.turns
        .iter()
        .find_map(|turn| turn.any_checksums())
        .map(|(_, checksums)| checksums.get(channel))
}

/// The complete corpus measurement.  This intentionally compares the loaded image with
/// retail rather than blessing it: source compilation installs real program state, but
/// runtime statics and compiler byte identity remain falsifiable discrepancies.
#[test]
fn stock_replays_load_the_observed_registry_without_fitting_checksum_values() {
    let Some(content_root) = shipped_bhs_root() else {
        skip("ron-data/bhs-corpus is absent.");
        return;
    };
    let replay_root = repo_root().join("ron-data/replays/multi");
    if !replay_root.is_dir() {
        skip("the replay corpus is absent.");
        return;
    }

    let mut checked = 0usize;
    let mut no_ai = 0usize;
    let mut with_ai = 0usize;
    let mut loaded_matches = 0usize;
    let mut targets = BTreeMap::<u32, usize>::new();

    for name in CHECKSUM_RECORDINGS {
        let path = replay_root.join(name);
        let rep = Replay::open(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let expected = first_recorded(&rep, Channel::ScriptRunTime).unwrap_or_else(|| {
            panic!(
                "{}: frozen checksum recording has no checksum",
                path.display()
            )
        });
        checked += 1;
        *targets.entry(expected).or_default() += 1;

        let image = load_replay_bhs_program(&rep.initial, &content_root)
            .unwrap_or_else(|error| panic!("{}: {error}", rep.path.display()));
        let ai_slots = rep
            .initial
            .active_players()
            .filter(|player| player.flags & LEADER_FLAG_HUMAN == 0)
            .map(|player| player.slot)
            .collect::<Vec<_>>();

        if ai_slots.is_empty() {
            no_ai += 1;
            assert_eq!(image.selection, ReplayBhsSelection::EmptyNoAi);
            assert_eq!(image.checksum.script_files, 0);
            assert_eq!(image.checksum.bytes_walked, 4);
            assert_eq!(image.checksum.checksum, EMPTY_RUNTIME_CHANNEL);
            assert_eq!(expected, EMPTY_RUNTIME_CHANNEL, "{}", rep.path.display());
        } else {
            with_ai += 1;
            assert_eq!(
                image.selection,
                ReplayBhsSelection::StandardAiEconomic {
                    ai_slots: ai_slots.clone()
                }
            );
            assert_eq!(image.program.files.len(), 3);
            assert_eq!(image.checksum.script_files, 3);
            assert_eq!(image.checksum.bytes_walked, LOADED_IMAGE_BYTES);
            assert_eq!(image.checksum.checksum, LOADED_IMAGE_CHANNEL);
            assert_eq!(
                image.production.as_ref().map(|b| (b.file, b.name.as_str())),
                Some((0, "economic"))
            );
            assert_eq!(
                image
                    .general_powers
                    .as_ref()
                    .map(|b| (b.file, b.name.as_str())),
                Some((2, "general_powers"))
            );
            assert!(image.program.find_script("economic").is_some());
            assert!(image.program.files[1]
                .source_file
                .to_ascii_lowercase()
                .ends_with("aibestbuildlibrary.bhs"));
            assert!(image.program.find_script("general_powers").is_some());
            loaded_matches += usize::from(image.checksum.checksum == expected);
            let (runtime, production) = image
                .into_script_runtime()
                .expect("derived step-4 entries bind in the persistent runtime");
            assert_eq!(
                production.map(|binding| (binding.file, binding.name)),
                Some((0, "economic".to_string()))
            );
            assert_eq!(
                checksum_program(runtime.program())
                    .expect("runtime retained the checksum sidecar")
                    .checksum,
                LOADED_IMAGE_CHANNEL
            );
        }
    }

    eprintln!(
        "  checksum recordings={checked}; empty/no-AI={no_ai}; loaded/AI={with_ai}; \
         loaded image=3 files/{LOADED_IMAGE_BYTES} bytes/0x{LOADED_IMAGE_CHANNEL:08x}; \
         loaded first-turn matches={loaded_matches}; retail targets={targets:08x?}"
    );

    // These are corpus-shape assertions, not values used by the producer.  They make a
    // partial scan or accidental selection-regression conspicuous.
    assert_eq!(checked, 21);
    assert_eq!(no_ai, 7);
    assert_eq!(with_ai, 14);
    assert_eq!(loaded_matches, 0);
}
