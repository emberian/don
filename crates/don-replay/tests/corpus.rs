//! The harness, against real recordings.
//!
//! `ron-data/` is gitignored copyrighted game content. Without it these tests
//! skip **loudly** — a skip is not a pass, and the banner says so, because the
//! whole value of this crate is that it compares against bytes retail wrote.
//!
//! Run with output:
//!   cargo test -p don-replay --release -- --nocapture

use don_replay::checksum::{Channel, NUM_CHANNELS, SHIPPED_RULES_CHANNEL};
use don_replay::harness::{self, NullSim, Phase};
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

/// Recordings that actually carry checksums, opened once for the whole test
/// binary.
///
/// Selection is by evidence, not by filename: the corpus mixes engine eras and
/// only multiplayer games emit `CheckSumsCommand`, so a name-based guess picks
/// pre-Extended-Edition solo files and every test then skips on nothing. Scans
/// newest-first and stops at `SCAN_BUDGET` files so the test stays under a
/// minute.
fn mp_replays() -> &'static [Replay] {
    static CACHE: std::sync::OnceLock<Vec<Replay>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| {
        // A debug build pays roughly 10x for the obfuscation-key search, and
        // this crate sits in a workspace other lanes run `cargo test` on, so
        // the debug default is a small spread and the thorough sweep is one
        // flag away: `cargo test -p don-replay --release`.
        const SCAN_BUDGET: usize = 14;
        let want: usize = if cfg!(debug_assertions) { 2 } else { 6 };
        let mut files: Vec<PathBuf> = corpus(&repo_root());
        // Ordering heuristic, for test *selection* only: the Extended Edition
        // recorder capitalises `Playback`, the 2003-era one does not, so
        // capital-P files first puts the checksum-bearing era at the front and
        // keeps this test off 30 pre-EE files it would only reject. Whether a
        // file counts is still decided by evidence (`checksum_packets > 0`),
        // never by its name.
        files.sort_by_key(|p| {
            let n = p.file_name().unwrap().to_string_lossy().to_string();
            (!n.starts_with("Playback"), std::cmp::Reverse(n))
        });
        let mut out = Vec::new();
        for f in files.iter().take(SCAN_BUDGET) {
            if out.len() >= want.min(SCAN_BUDGET) {
                break;
            }
            if let Ok(r) = Replay::open(f) {
                if r.checksum_packets > 0 {
                    out.push(r);
                }
            }
        }
        out
    })
}

/// True when the corpus is present at all — used to tell "no game data" apart
/// from "game data but no multiplayer recording".
fn corpus_present() -> bool {
    !corpus(&repo_root()).is_empty()
}

fn skip_banner() {
    if corpus_present() {
        eprintln!(
            "\n  SKIPPED — NOT A PASS. The corpus is present but no recording \n  \
             carries a CheckSumsCommand (solo and pre-Extended-Edition files do \n  \
             not emit one). Nothing was established.\n"
        );
    } else {
        eprintln!(
            "\n  SKIPPED — NOT A PASS. No .rcx under ron-data/replays/.\n  \
             This crate's only evidence is the recorded checksums; without the \n  \
             corpus it establishes nothing.\n"
        );
    }
}

/// Every `CheckSumsCommand` we decode must satisfy the engine's own internal
/// relation: the sixteenth word is the wrapping sum of the fifteen channels,
/// and every channel is adler-32-shaped. False-positive rate ≈ 2⁻³², so this
/// is a real test of the decode, not a formality.
#[test]
fn recorded_checksum_packets_are_structurally_sound() {
    let reps = mp_replays();
    if reps.is_empty() {
        skip_banner();
        return;
    }
    let (mut pk, mut tot, mut shape) = (0usize, 0usize, 0usize);
    for r in reps {
        pk += r.checksum_packets;
        tot += r.checksum_total_ok;
        shape += r.checksum_shape_ok;
    }
    eprintln!("  {pk} checksum packets: {tot} with total == sum(15), {shape} adler-shaped");
    assert!(
        pk > 0,
        "no checksum packets found in {} recordings",
        reps.len()
    );
    assert_eq!(tot, pk, "total != sum(15) in {} packets", pk - tot);
    assert_eq!(
        shape,
        pk,
        "non-adler-shaped channel in {} packets",
        pk - shape
    );
}

/// The `rules` channel is loaded-once static data. It must be constant within a
/// recording, and equal to the value the shipped rule set produces.
#[test]
fn rules_channel_is_the_shipped_constant() {
    let reps = mp_replays();
    if reps.is_empty() {
        skip_banner();
        return;
    }
    let mut checked = 0usize;
    for r in reps {
        let vals: Vec<u32> = r
            .turns
            .iter()
            .filter_map(|t| t.any_checksums().map(|(_, c)| c.get(Channel::Rules)))
            .collect();
        if vals.is_empty() {
            continue;
        }
        assert!(
            vals.iter().all(|&v| v == vals[0]),
            "rules channel moved within a game"
        );
        assert_eq!(
            vals[0],
            SHIPPED_RULES_CHANNEL,
            "{}: rules = {:08x}",
            r.path.file_name().unwrap().to_string_lossy(),
            vals[0]
        );
        checked += 1;
    }
    eprintln!("  rules == {SHIPPED_RULES_CHANNEL:08x} in {checked} recordings");
    assert!(checked > 0);
}

/// The control experiment, and the reason the join key matters.
///
/// Two clients running the same lockstep game emit their own tuple for each
/// turn. Joined on `CommandPackage::group` they must be identical; joined on
/// `stamp` they are not, and every disagreement compares two *different* turns.
/// A comparator that cannot tell those apart cannot be trusted to attribute our
/// own divergences either.
#[test]
fn cross_player_tuples_agree_when_joined_on_the_turn_serial() {
    let reps = mp_replays();
    if reps.is_empty() {
        skip_banner();
        return;
    }
    let (mut gc, mut gi, mut sc, mut si, mut wrong) = (0usize, 0usize, 0usize, 0usize, 0usize);
    for r in reps {
        let (a, b, _) = r.crossplay();
        let (c, d, _, e, _) = r.crossplay_by_stamp_diag();
        gc += a;
        gi += b;
        sc += c;
        si += d;
        wrong += e;
    }
    eprintln!("  by group: {gi}/{gc} identical;  by stamp: {si}/{sc} identical, {wrong} of the {} stamp disagreements compare different turns", sc - si);
    assert!(gc > 0, "no cross-player comparisons available");
    assert_eq!(gi, gc, "{} genuine cross-player disagreements", gc - gi);
    assert_eq!(
        wrong,
        sc - si,
        "a by-stamp disagreement that is NOT a turn mismatch"
    );
}

/// The loop itself: run a recording end to end and produce a divergence
/// profile. The assertions are about the *harness*, not about our fidelity —
/// the fidelity number is allowed to be bad and is reported, not asserted.
#[test]
fn the_harness_produces_a_divergence_profile() {
    let reps = mp_replays();
    if reps.is_empty() {
        skip_banner();
        return;
    }
    let mut ran = 0usize;
    for rep in reps.iter().take(3) {
        let mut sim = NullSim::new();
        let run = harness::run(rep, &mut sim, Phase::BeforeCommands, 0);
        eprintln!("{}", harness::format_table(&run));

        // The comparison actually happened on every channel.
        for c in 0..NUM_CHANNELS {
            assert_eq!(
                run.channels[c].compares + run.channels[c].retail_disagreed,
                run.turns_checksummed as u32,
                "channel {} compared {} of {} checksummed turns",
                don_replay::CHANNEL_NAMES[c],
                run.channels[c].compares,
                run.turns_checksummed
            );
        }
        // A channel that never diverged must have matched every compare.
        for c in 0..NUM_CHANNELS {
            let r = &run.channels[c];
            if r.first_divergence_turn.is_none() {
                assert_eq!(r.matches, r.compares, "{}", don_replay::CHANNEL_NAMES[c]);
            } else {
                assert!(r.matches < r.compares);
            }
        }
        // The empty model is the right model for `walls` in a wall-less game
        // and the wrong model for `units`. If that ever inverts, something is
        // very wrong with the walk, not with the mechanics.
        assert!(
            run.channels[Channel::Units as usize]
                .first_divergence_turn
                .is_some(),
            "an empty world cannot match retail's units channel"
        );
        assert_eq!(
            run.channels[Channel::Units as usize].survived,
            0,
            "units must diverge on the very first checksummed turn"
        );
        ran += 1;
    }
    assert!(ran > 0, "no multiplayer recording ran");
}

/// The initial-state vertical slice is measured against retail, not just a
/// synthetic parser fixture: the complete Game/GameInfo prefix is before the
/// package stream, and its map setup drives a non-empty world checksum which
/// records the expected first-turn divergence and its unsourced ceiling.
#[test]
fn replay_setup_drives_a_real_nonempty_world_channel() {
    let reps = mp_replays();
    if reps.is_empty() {
        skip_banner();
        return;
    }
    let rep = &reps[0];
    assert!(rep.initial.bytes_walked < rep.stream_start);
    assert_eq!(rep.initial.info.players.len(), 8);
    assert!(rep.initial.active_players().count() > 0);
    let first = rep
        .turns
        .iter()
        .find(|t| t.any_checksums().is_some())
        .unwrap()
        .turn;

    let mut sim = harness::WorldSim::from_replay(rep);
    let initial = sim
        .initial_world
        .as_ref()
        .expect("checksummed corpus recording has a procedural map setup");
    assert_eq!(initial.world.seed as u32, rep.initial.info.seed);
    let run = harness::run(rep, &mut sim, Phase::BeforeCommands, 0);
    let w = &run.channels[Channel::World as usize];
    assert_eq!(w.first_divergence_turn, Some(first));
    assert_eq!(w.nontrivial_compares, w.compares);
    assert!(w.our_bytes_walked > 0);
    assert!(w.our_unsourced_walked > 0);
    assert!(w.our_unsourced_walked < w.our_bytes_walked);
    assert_eq!(w.matches, 0);
}

/// The bridge, against a real recording: a `don_sim::World` with rows in it
/// produces engine-layout `Unit` records, and the `units` channel compares
/// **bytes against bytes** instead of nothing against something.
///
/// The population is seeded rather than derived — `don-sim` has no command
/// producer yet — so this asserts about the plumbing, never about agreement. It
/// asserts the opposite of agreement, in fact: a declared population must *not*
/// match retail, and if it ever did, the comparator would be broken.
#[test]
fn the_bridge_makes_the_units_channel_non_trivial_against_a_recording() {
    let reps = mp_replays();
    if reps.is_empty() {
        skip_banner();
        return;
    }
    let rep = &reps[0];
    let u = Channel::Units as usize;

    let mut empty = NullSim::new();
    let a = harness::run(rep, &mut empty, Phase::BeforeCommands, 0);
    assert_eq!(
        a.channels[u].nontrivial_compares, 0,
        "an empty world walks no bytes"
    );

    let mut seeded = harness::WorldSim::seeded(2, 3);
    assert_eq!(seeded.seed_units, 6);
    let b = harness::run(rep, &mut seeded, Phase::BeforeCommands, 0);
    eprintln!(
        "  units: {} non-trivial compares, {} bytes/compare, {} matches",
        b.channels[u].nontrivial_compares, b.channels[u].our_bytes_walked, b.channels[u].matches
    );
    assert_eq!(
        b.channels[u].nontrivial_compares, b.channels[u].compares,
        "every compare walked bytes"
    );
    assert!(
        b.channels[u].our_bytes_walked >= 6 * 145,
        "six units of Unit+Object bytes: {}",
        b.channels[u].our_bytes_walked
    );
    assert_eq!(b.channels[u].matches, 0, "a seeded world must not match");
    assert!(seeded.frames > 0, "the world was actually stepped");
}

/// Every agreement in the corpus on a channel `don-sim` has no producer for must be
/// *counted* as producerless, and the only agreements that are not are the ones a
/// producer really earned. Today exactly one channel earns any: `script_run_time`,
/// where retail's own value is `0x00040001` — the four-byte empty
/// `ScriptFile::script_files` count, not the adler-of-nothing 1 — so an agreement there
/// is a real comparison of four bytes against four bytes. Asserted rather than written
/// in prose so it stops being true the moment another producer lands.
#[test]
fn agreements_are_unmodelled_except_the_empty_script_file_count() {
    let reps = mp_replays();
    if reps.is_empty() {
        skip_banner();
        return;
    }
    let script = Channel::ScriptRunTime as usize;
    let mut modelled_matches = 0u32;
    let mut script_matches = 0u32;
    for rep in reps.iter().take(3) {
        let mut sim = NullSim::new();
        let run = harness::run(rep, &mut sim, Phase::BeforeCommands, 0);
        for c in 0..don_replay::checksum::NUM_WALKED {
            let r = &run.channels[c];
            assert!(r.unmodelled_matches <= r.trivial_matches);
            assert!(r.trivial_matches <= r.matches);
            if don_replay::check_all::CHANNEL_SOURCE[c]
                == don_replay::check_all::ChannelSource::Modelled
            {
                if c == script {
                    // Never trivial: our walker hands the visitor four bytes on every
                    // compare, so an agreement cannot be the empty-vs-empty coincidence.
                    assert_eq!(r.nontrivial_compares, r.compares);
                    assert_eq!(r.trivial_matches, 0);
                    assert_eq!(r.retail_empty_compares, 0, "retail never read 1 here");
                    script_matches += r.matches;
                } else {
                    modelled_matches += r.matches;
                }
            } else {
                assert_eq!(
                    r.matches,
                    r.unmodelled_matches,
                    "{} agreed without a producer and it was not counted as such",
                    don_replay::CHANNEL_NAMES[c]
                );
            }
        }
    }
    eprintln!(
        "  matches on channels we model, excluding script_run_time: {modelled_matches}; \
         script_run_time: {script_matches}"
    );
    assert_eq!(
        modelled_matches, 0,
        "the units or world channel started matching retail — update this test, it is good news"
    );
}

/// A mutation test on the harness: if the comparator cannot see a wrong value,
/// none of the numbers above mean anything.
#[test]
fn the_comparator_bites() {
    let reps = mp_replays();
    if reps.is_empty() {
        skip_banner();
        return;
    }
    let rep = &reps[0];

    // A simulation that reports the correct empty tuple: `walls` survives.
    let mut good = NullSim::new();
    let a = harness::run(rep, &mut good, Phase::BeforeCommands, 0);
    assert!(a.channels[Channel::Walls as usize].survived > 0);

    // The same simulation with one object pushed onto `walls`: it must not.
    struct Mutant(NullSim);
    impl harness::Simulation for Mutant {
        fn check_all(&self) -> (don_replay::Channels, [u64; 15]) {
            self.0.check_all()
        }
    }
    let mut bad = NullSim::new();
    assert!(bad.state.push_default_object(Channel::Walls));
    let b = harness::run(rep, &mut Mutant(bad), Phase::BeforeCommands, 0);
    assert_eq!(
        b.channels[Channel::Walls as usize].survived,
        0,
        "the comparator did not notice a wrong walls channel"
    );
}
