//! Mutation-sensitive gates for substantive replay-checksum accounting.

use don_replay::checksum::{Channel, Channels, NUM_WALKED};
use don_replay::harness::{self, Phase, Simulation, SimulationChannelEvidence, WorldSim};
use don_replay::replay::{corpus, Replay};
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn every_substantive_gate_is_load_bearing() {
    let admitted = SimulationChannelEvidence {
        installed: true,
        bytes_walked: 1,
        unsourced_walked: 0,
        walk_complete: true,
        exact_producer: true,
    };
    assert!(admitted.substantive());

    let mutations = [
        SimulationChannelEvidence {
            installed: false,
            ..admitted
        },
        SimulationChannelEvidence {
            bytes_walked: 0,
            ..admitted
        },
        SimulationChannelEvidence {
            unsourced_walked: 1,
            ..admitted
        },
        SimulationChannelEvidence {
            walk_complete: false,
            ..admitted
        },
        SimulationChannelEvidence {
            exact_producer: false,
            ..admitted
        },
    ];
    for mutation in mutations {
        assert!(
            !mutation.substantive(),
            "gate mutation was admitted: {mutation:?}"
        );
    }
}

/// Wrap the real generic bridge but mutate its checksum words to an exact retail tuple.
/// Raw equality must observe that mutation; substantive accounting must still reject the
/// partial Unit walk using the unmodified bridge evidence.
struct ForcedChecksum {
    inner: WorldSim,
    forced: Channels,
}

impl Simulation for ForcedChecksum {
    fn step_turn(&mut self, frames: u32) {
        self.inner.step_turn(frames);
    }

    fn check_all(&self) -> (Channels, [u64; NUM_WALKED]) {
        let (_, bytes) = self.inner.check_all();
        (self.forced, bytes)
    }

    fn unsourced_walked(&self) -> [u64; NUM_WALKED] {
        self.inner.unsourced_walked()
    }

    fn installed_channels(&self) -> [bool; NUM_WALKED] {
        self.inner.installed_channels()
    }

    fn check_all_with_evidence(&self) -> (Channels, [SimulationChannelEvidence; NUM_WALKED]) {
        let (_, evidence) = self.inner.check_all_with_evidence();
        (self.forced, evidence)
    }
}

#[test]
fn a_forced_match_on_the_partial_generic_unit_walk_cannot_count() {
    let mut files = corpus(&repo_root());
    files.sort();
    let Some(mut replay) = files
        .iter()
        .find_map(|path| Replay::open(path).ok().filter(|r| r.checksum_packets > 0))
    else {
        eprintln!(
            "\n  SKIPPED — NOT A PASS. No checksum-bearing replay corpus is installed; \
             the mutation-sensitive harness comparison was not exercised.\n"
        );
        return;
    };
    let first = replay
        .turns
        .iter()
        .position(|turn| turn.any_checksums().is_some())
        .expect("checksum_packets implies a checksum turn");
    let forced = replay.turns[first].any_checksums().unwrap().1;
    replay.turns.truncate(first + 1);

    let mut sim = ForcedChecksum {
        inner: WorldSim::seeded(1, 1),
        forced,
    };
    let run = harness::run(&replay, &mut sim, Phase::BeforeCommands, 0);

    let units = &run.channels[Channel::Units as usize];
    assert_eq!(
        units.matches, 1,
        "the checksum mutation must create raw equality"
    );
    assert_eq!(
        units.nontrivial_compares, 1,
        "the generic walk touched bytes"
    );
    assert!(units.our_installed);
    assert!(units.our_bytes_walked > 0);
    assert!(units.our_unsourced_walked > 0);
    assert!(!units.our_walk_complete);
    assert!(!units.our_exact_producer);
    assert_eq!(units.substantive_compares, 0);
    assert_eq!(units.substantive_matches, 0);

    // The same forced tuple must count on the independently derived, four-byte
    // empty-script owner. This makes the test sensitive to accidentally disabling
    // substantive accounting wholesale instead of enforcing the authority predicate.
    let script = &run.channels[Channel::ScriptRunTime as usize];
    assert_eq!(script.matches, 1);
    assert_eq!(script.substantive_compares, 1);
    assert_eq!(script.substantive_matches, 1);
    assert!(script.our_installed);
    assert!(script.our_walk_complete);
    assert!(script.our_exact_producer);
}
