// SPDX-License-Identifier: GPL-3.0-or-later
//! Focused retail-corpus evidence for replay-carried static Rules state.

use don_replay::checksum::Channel;
use don_replay::harness::{self, Phase, WorldSim};
use don_replay::initial::{
    parse_serialized_rules_at, SHIPPED_RULES_SERIALIZED_BYTES, SHIPPED_TYPES_SERIALIZED_BYTES,
};
use don_replay::replay::{load_payload, Replay};
use don_replay::rules_channel::{
    BALANCE_BYTES, RETAIL_AFTER_BALANCE, RETAIL_AFTER_CONSTANTS, RETAIL_AFTER_TYPES,
    RETAIL_WALKED_BYTES, RULES_BLOCK_BYTES, SHIPPED_RULES_CHANNEL,
};
use std::path::{Path, PathBuf};

fn supported_replay() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("ron-data/replays/multi/Playback___2025.02.10_21_26_50__Mon_.rcx")
}

fn open_supported() -> Option<Replay> {
    let path = supported_replay();
    if !path.is_file() {
        eprintln!(
            "\n  SKIPPED — NOT A PASS. The exact 00.2024.06.20 retail replay is absent; \
             replay-carried Rules agreement was not exercised.\n"
        );
        return None;
    }
    Some(Replay::open(&path).expect("supported retail replay must decode"))
}

#[test]
fn replay_rules_are_independently_projected_and_survive_the_whole_recording() {
    let Some(rep) = open_supported() else {
        return;
    };
    let rules = rep
        .initial
        .rules
        .expect("supported ordinary recording must carry its Rules section");

    assert_eq!(rules.serialized_bytes, SHIPPED_RULES_SERIALIZED_BYTES);
    assert_eq!(
        rules.serialized_offset + rules.serialized_bytes,
        rep.stream_start
    );
    assert_eq!(rules.walked_bytes, RETAIL_WALKED_BYTES);
    assert_eq!(rules.after_types, RETAIL_AFTER_TYPES);
    assert_eq!(rules.after_constants, RETAIL_AFTER_CONSTANTS);
    assert_eq!(rules.after_balance, RETAIL_AFTER_BALANCE);
    assert_eq!(rules.checksum, SHIPPED_RULES_CHANNEL);

    let mut sim = WorldSim::from_replay(&rep);
    let run = harness::run(&rep, &mut sim, Phase::BeforeCommands, 0);
    let channel = &run.channels[Channel::Rules as usize];
    assert!(channel.compares > 10_000, "real whole-game comparison ran");
    assert_eq!(channel.survived, channel.compares);
    assert_eq!(channel.matches, channel.compares);
    assert_eq!(channel.nontrivial_compares, channel.compares);
    assert_eq!(channel.trivial_matches, 0);
    assert_eq!(channel.unmodelled_matches, 0);
    assert_eq!(channel.our_bytes_walked, RETAIL_WALKED_BYTES);
    assert_eq!(channel.our_unsourced_walked, 0);
    assert_eq!(channel.first_divergence_turn, None);
}

#[test]
fn every_rules_projection_checkpoint_is_an_admission_gate() {
    let Some(rep) = open_supported() else {
        return;
    };
    let rules = rep.initial.rules.unwrap();
    let payload = load_payload(&rep.path).unwrap();

    // The command stream (and therefore every recorded wire tuple) remains
    // unchanged. A mutation in each independently checkpointed root must still
    // make the projection refuse admission.
    let types_at = rules.serialized_offset + 1;
    let constants_at = types_at + SHIPPED_TYPES_SERIALIZED_BYTES;
    let balance_at = constants_at + RULES_BLOCK_BYTES + 4;
    let tribes_at = balance_at + BALANCE_BYTES;
    for (at, want) in [
        (types_at, "Types checkpoint"),
        (constants_at, "Constants checkpoint"),
        (balance_at, "Balance checkpoint"),
        (tribes_at + 1, "final checkpoint"),
    ] {
        let mut mutant = payload.clone();
        mutant[at] ^= 1;
        let err = parse_serialized_rules_at(&mutant, rules.serialized_offset).unwrap_err();
        assert!(err.message.contains(want), "unexpected refusal: {err}");
    }

    let mut wrong_tag = payload;
    wrong_tag[rules.serialized_offset] ^= 1;
    let err = parse_serialized_rules_at(&wrong_tag, rules.serialized_offset).unwrap_err();
    assert!(
        err.message.contains("Rules tag"),
        "unexpected refusal: {err}"
    );
}
