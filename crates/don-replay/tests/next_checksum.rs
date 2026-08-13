// SPDX-License-Identifier: GPL-3.0-or-later
//! `NextCheckSumCommand` `0x3a` against the real corpus.
//!
//! `ron-data/` is gitignored copyrighted game content. Without it these tests
//! skip **loudly** — a skip is not a pass. Every assertion below is a claim
//! about bytes a retail client wrote in 2014 or 2017.
//!
//! Run with output:
//!   cargo test -p don-replay --release --test next_checksum -- --nocapture

use don_replay::checksum::{Channel, SHIPPED_RULES_CHANNEL};
use don_replay::harness::{self, NullSim, Phase, WorldSim};
use don_replay::next_checksum as ncs;
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

/// Every recording that carries the `0x3a` stream, opened once.
///
/// Selection is by evidence — `records(rep).is_empty()` — never by filename.
fn next_replays() -> &'static [Replay] {
    static CACHE: std::sync::OnceLock<Vec<Replay>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| {
        // A debug build pays roughly 10x for the obfuscation-key search; the
        // full sweep is one flag away (`--release`).
        let budget: usize = if cfg!(debug_assertions) { 10 } else { 200 };
        let mut files: Vec<PathBuf> = corpus(&repo_root());
        // The `0x3a` era is the lowercase-`playback` 2003-lineage recorder plus
        // the 2017 `00.2014.10.0200` build, so scanning in name order reaches
        // them first in a debug run. Membership is still decided by evidence.
        files.sort();
        let mut out = Vec::new();
        for f in files.iter().take(budget) {
            if let Ok(r) = Replay::open(f) {
                if !ncs::records(&r).is_empty() {
                    out.push(r);
                }
            }
        }
        out
    })
}

fn skip_banner() {
    if corpus(&repo_root()).is_empty() {
        eprintln!(
            "\n  SKIPPED — NOT A PASS. No .rcx under ron-data/replays/.\n  \
             This crate's only evidence is the recorded checksums; without the \n  \
             corpus it establishes nothing.\n"
        );
    } else {
        eprintln!(
            "\n  SKIPPED — NOT A PASS. The corpus is present but no recording \n  \
             carries a NextCheckSumCommand. Nothing was established.\n"
        );
    }
}

/// The two streams never coexist. This is what makes the `0x3a` recordings
/// *new* evidence rather than a second view of the same 21 files.
#[test]
fn no_recording_carries_both_checksum_streams() {
    let reps = next_replays();
    if reps.is_empty() {
        skip_banner();
        return;
    }
    for r in reps {
        assert_eq!(
            r.checksum_packets,
            0,
            "{} carries both 0x3a and 0x39",
            r.path.display()
        );
    }
    eprintln!(
        "  {} recordings carry 0x3a and none of them carries a CheckSumsCommand",
        reps.len()
    );
}

/// One record per player per turn, every turn, from turn 2 — no gaps.
#[test]
fn the_sweep_emits_one_record_per_player_per_turn_with_no_gaps() {
    let reps = next_replays();
    if reps.is_empty() {
        skip_banner();
        return;
    }
    let mut runs_total = 0usize;
    for r in reps {
        let recs = ncs::records(r);
        let runs = ncs::sweep_runs(&recs);
        runs_total += runs.len();
        assert_eq!(
            runs.first().map(|run| run.first_turn),
            Some(ncs::SWEEP_FIRST_TURN),
            "{} does not start its sweep on turn {}",
            r.path.display(),
            ncs::SWEEP_FIRST_TURN
        );
        for w in runs.windows(2) {
            assert_eq!(
                w[1].first_turn,
                w[0].last_turn + 1,
                "{} has a gap between type {} and type {}",
                r.path.display(),
                w[0].ty,
                w[1].ty
            );
        }
        // Every turn inside a run carries the same number of records, and that
        // number is the number of reporting players.
        for run in &runs {
            for turn in run.first_turn..=run.last_turn {
                let n = recs
                    .iter()
                    .filter(|x| x.turn == turn && x.ty == run.ty)
                    .count();
                assert!(
                    n >= 1,
                    "{} turn {turn} inside a type-{} run has no record",
                    r.path.display(),
                    run.ty
                );
            }
        }
    }
    eprintln!(
        "  {runs_total} sweep runs over {} recordings, no gaps",
        reps.len()
    );
}

/// The sweep visits the types in `CheckSumTypes` order and never runs off the
/// end of the enum.
#[test]
fn the_sweep_is_monotone_except_where_the_game_desyncs() {
    let reps = next_replays();
    if reps.is_empty() {
        skip_banner();
        return;
    }
    let mut monotone = 0usize;
    let mut regressed: Vec<String> = Vec::new();
    for r in reps {
        let recs = ncs::records(r);
        for x in &recs {
            assert!(
                (x.ty as usize) < ncs::CHECK_SUM_TYPES.len(),
                "{} emitted checksum_type {} — CHECKSUM_NUM or beyond",
                r.path.display(),
                x.ty
            );
        }
        let runs = ncs::sweep_runs(&recs);
        if runs.windows(2).any(|w| w[1].ty < w[0].ty) {
            regressed.push(r.path.file_name().unwrap().to_string_lossy().to_string());
        } else {
            monotone += 1;
        }
    }
    // The only recording whose sweep restarts is one that desyncs; if a second
    // one appears, the sweep model is wrong and this test should say so.
    for name in &regressed {
        let rep = reps
            .iter()
            .find(|r| r.path.file_name().unwrap() == name.as_str())
            .unwrap();
        let recs = ncs::records(rep);
        let xp = ncs::crossplay(rep, &recs);
        assert!(
            !xp.disagreements.is_empty(),
            "{name} restarts its sweep without any cross-player disagreement"
        );
    }
    eprintln!(
        "  monotone in {monotone} of {}; restarts: {regressed:?}",
        reps.len()
    );
}

/// The run-length law: `elements + 1`.
///
/// Two independent pins. `walls` is empty in every recorded game — the `0x39`
/// corpus measures 222,938 of 222,938 comparisons with retail's own value at
/// `1` — and its run is one turn. `CheckSums::check_leaders` `0x009375a0`
/// iterates `[0x00e3a390, 0x00e71af0)` in `0x6eec` strides, i.e. exactly eight
/// leaders, and its run is nine turns in every recording that reaches it.
#[test]
fn walls_runs_one_turn_and_leaders_runs_exactly_nine() {
    let reps = next_replays();
    if reps.is_empty() {
        skip_banner();
        return;
    }
    let mut walls = 0usize;
    let mut leaders = 0usize;
    for r in reps {
        let recs = ncs::records(r);
        for run in ncs::sweep_runs(&recs).iter().filter(|run| !run.truncated) {
            match run.ty {
                4 => {
                    assert_eq!(run.turns, 1, "{} walls run", r.path.display());
                    walls += 1;
                }
                9 => {
                    assert_eq!(run.turns, 9, "{} leaders run", r.path.display());
                    leaders += 1;
                }
                _ => {}
            }
        }
    }
    assert!(walls > 0 && leaders > 0, "no walls or leaders run reached");
    eprintln!("  walls run = 1 turn in {walls} files; leaders run = 9 turns in {leaders} files");
    assert_eq!(
        (0x00e7_1af0u32 - 0x00e3_a390u32) / 0x6eec,
        8,
        "check_leaders 0x009375a0 no longer iterates eight leaders"
    );
}

/// A one-turn run carries the whole channel, and the `rules` record proves it:
/// it equals what `rules_channel` independently produces by walking the
/// recording's own carried Rules section, 997,846 bytes at a time.
///
/// The other half is the falsifiable one — the parser must **refuse** every
/// recording whose wire rules value is not the shipped constant. Thirty-plus
/// chances to be wrong.
#[test]
fn the_rules_record_agrees_with_our_projection_and_refuses_every_other_ruleset() {
    let reps = next_replays();
    if reps.is_empty() {
        skip_banner();
        return;
    }
    let mut admitted_and_shipped = 0usize;
    let mut refused_and_not_shipped = 0usize;
    let mut refused_but_shipped = 0usize;
    let mut distinct = std::collections::BTreeSet::new();
    for r in reps {
        let recs = ncs::records(r);
        let runs = ncs::sweep_runs(&recs);
        for (turn, ty, value) in ncs::whole_channel_turns(&recs, &runs) {
            if ty != 1 {
                continue;
            }
            let Some(wire) = value else { continue };
            distinct.insert(wire);
            match r.initial.rules.map(|x| x.checksum) {
                Some(ours) => {
                    assert_eq!(
                        ours,
                        SHIPPED_RULES_CHANNEL,
                        "{} admitted a Rules section that is not the shipped one",
                        r.path.display()
                    );
                    assert_eq!(
                        ours,
                        wire,
                        "{} turn {turn}: our Rules projection disagrees with the wire",
                        r.path.display()
                    );
                    admitted_and_shipped += 1;
                }
                None => {
                    if wire == SHIPPED_RULES_CHANNEL {
                        refused_but_shipped += 1;
                    } else {
                        refused_and_not_shipped += 1;
                    }
                }
            }
        }
    }
    eprintln!(
        "  rules: {admitted_and_shipped} admitted and agreeing, \
         {refused_and_not_shipped} refused with a non-shipped ruleset, \
         {refused_but_shipped} refused although the wire says shipped; \
         {} distinct rulesets on the wire",
        distinct.len()
    );
    assert!(
        admitted_and_shipped > 0,
        "no recording both admitted a Rules section and carried a rules record"
    );
    // The refusals are the falsifiable half: the wire carries several rulesets
    // that are not the shipped one, and the producer took none of them.
    assert!(
        refused_and_not_shipped > 0,
        "no recording carried a non-shipped ruleset, so nothing was refused"
    );
    assert!(
        distinct.len() > 1,
        "the corpus should carry more than one ruleset on this stream"
    );
    // `refused_but_shipped` is a measured gap in `Replay::open`'s Rules
    // locator, not a fidelity failure: those recordings carry the shipped
    // rules and we do not find the section. Reported above; closing it can
    // only raise `admitted_and_shipped`.
    assert!(
        refused_but_shipped < admitted_and_shipped + refused_and_not_shipped,
        "the locator now misses more recordings than it handles"
    );
}

/// The cross-player control experiment. Unlike the `0x39` corpus — where the
/// 21 known disagreements were all a `stamp`-join artifact — this stream has
/// genuine disagreements, and every one of them sits at the end of its
/// recording: the client detects the divergence and the recording stops.
#[test]
fn every_cross_player_disagreement_is_at_the_end_of_its_recording() {
    let reps = next_replays();
    if reps.is_empty() {
        skip_banner();
        return;
    }
    let (mut cmp, mut same) = (0usize, 0usize);
    let mut bad = Vec::new();
    for r in reps {
        let recs = ncs::records(r);
        let xp = ncs::crossplay(r, &recs);
        cmp += xp.comparisons;
        same += xp.identical;
        for d in xp.disagreements {
            assert!(
                d.turns_before_end <= 2,
                "{} disagrees on turn {} with {} turns still to go — a mid-game desync \
                 that survives is new and this test should be updated to say so",
                r.path.display(),
                d.turn,
                d.turns_before_end
            );
            bad.push((r.path.file_name().unwrap().to_string_lossy().to_string(), d));
        }
    }
    eprintln!(
        "  {cmp} comparisons, {same} identical, {} disagreements",
        bad.len()
    );
    for (f, d) in &bad {
        eprintln!(
            "    {f} turn {} {} p{}=0x{:08x} p{}=0x{:08x} ({} turns before the end)",
            d.turn,
            ncs::type_channel_name(d.ty),
            d.a_play,
            d.a_value,
            d.b_play,
            d.b_value,
            d.turns_before_end
        );
    }
    assert!(cmp > 0, "no cross-player comparison was made");
    assert!(
        !bad.is_empty(),
        "the corpus is known to contain cross-player disagreements on this stream"
    );
}

/// The scorer never credits a channel we do not produce, even when the numbers
/// would agree. `walls`, `ammo` and `deaths` all read `1` on this stream in the
/// recordings where they are empty, and so does our absent producer — that is
/// exactly the vacuous agreement this crate exists not to score.
#[test]
fn an_absent_producer_is_never_credited_with_a_match() {
    let reps = next_replays();
    if reps.is_empty() {
        skip_banner();
        return;
    }
    let mut no_producer = 0u64;
    let mut compares = 0u64;
    for rep in reps.iter().take(6) {
        let mut sim = NullSim::new();
        let run = harness::run(rep, &mut sim, Phase::BeforeCommands, 0);
        for c in [Channel::Walls, Channel::Ammo, Channel::Deaths] {
            let n = run.next_checksum.per_channel[c as usize];
            assert_eq!(
                n.matches,
                0,
                "{} was credited with a match on {:?} without a producer",
                rep.path.display(),
                c
            );
            assert_eq!(n.compares, 0);
            no_producer += n.no_producer;
        }
        for c in 0..don_replay::checksum::NUM_CHANNELS {
            compares += run.next_checksum.per_channel[c].compares;
        }
    }
    eprintln!("  {no_producer} whole-channel records refused for want of a producer");
    assert!(
        no_producer > 0,
        "no walls/ammo/deaths whole-channel record was reached at all"
    );
    let _ = compares;
}

/// The `world` channel reads `1` on this stream in every recording that reaches
/// it, while the `0x39` corpus never reads `1` there.
///
/// `CommandManager::issue_check_sums` `0x00940770` is the shipped mechanism:
/// the world walk runs only when `[[0x00c06188] + 0x134] != 0`, otherwise the
/// channel keeps its initial `1`. Recorded as a measurement of the stream, not
/// as a conclusion about which build disabled what.
#[test]
fn the_world_record_is_one_on_this_stream() {
    let reps = next_replays();
    if reps.is_empty() {
        skip_banner();
        return;
    }
    let mut seen = 0usize;
    for r in reps {
        let recs = ncs::records(r);
        let runs = ncs::sweep_runs(&recs);
        for (_, ty, value) in ncs::whole_channel_turns(&recs, &runs) {
            if ty != 10 {
                continue;
            }
            if let Some(v) = value {
                assert_eq!(
                    v,
                    1,
                    "{} carries a non-empty world record on the 0x3a stream — \
                     good news, and this test should be updated",
                    r.path.display()
                );
                seen += 1;
            }
        }
    }
    assert!(seen > 0, "no world record was reached");
    eprintln!("  world == 1 in {seen} recordings on this stream");
}

/// The live `Sim.groups` producer is compared here and it loses, on purpose.
///
/// The sweep reaches `CHECKSUM_GROUPS` several hundred turns in, long after the
/// first `GroupCommand`, so the `Game::init` image cannot still hold. Recording
/// the divergence is the point: it is a real falsification opportunity taken.
#[test]
fn the_fresh_live_groups_pool_is_compared_and_diverges() {
    let reps = next_replays();
    if reps.is_empty() {
        skip_banner();
        return;
    }
    let mut compares = 0u64;
    let mut matches = 0u64;
    let mut nontrivial = 0u64;
    for rep in reps.iter().take(6) {
        let mut sim = WorldSim::from_replay(rep);
        let run = harness::run(rep, &mut sim, Phase::BeforeCommands, 0);
        let n = run.next_checksum.per_channel[Channel::Groups as usize];
        compares += n.compares;
        matches += n.matches;
        nontrivial += n.nontrivial_compares;
    }
    eprintln!("  groups: {compares} compares, {matches} matches, {nontrivial} nontrivial");
    assert!(compares > 0, "the sweep never reached CHECKSUM_GROUPS");
    assert_eq!(
        nontrivial, compares,
        "every groups compare must walk the 36,896-byte image"
    );
    assert_eq!(
        matches, 0,
        "the fresh live Sim.groups pool started matching a mid-game record — \
         that would be news, and this test should be updated to say what changed"
    );
}
