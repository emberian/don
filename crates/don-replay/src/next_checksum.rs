// SPDX-License-Identifier: GPL-3.0-or-later
//! `NextCheckSumCommand` `0x3a` — the *other* lockstep checksum stream.
//!
//! # Why this module exists
//!
//! The corpus has two checksum streams, and until this module only one of them
//! was read. `CheckSumsCommand` `0x39` carries the sixteen-word tuple that
//! [`crate::check_all`] models; it appears in **21** of the 61 recordings.
//! `NextCheckSumCommand` `0x3a` carries **one** subsystem's checksum plus the
//! subsystem's index, and it appears in **39** — and in *no* recording that
//! carries `0x39` [measured, this lane, whole corpus]. The two are disjoint by
//! engine build: the `0x3a` files are the `03.02.03.2905`, `00.2014.07.1000`
//! and `00.2014.10.0200` builds, the `0x39` files are `00.2017.11.2900` and
//! `00.2024.06.2000`. One recording (`today.rcx`) carries neither.
//!
//! So 39 recordings that scored **zero** comparisons were carrying 796,957
//! per-subsystem checksum records.
//!
//! # The record
//!
//! Six bytes: opcode, `checksum_type` (`u8`, `+1`), `checksum` (`u32`, `+2`);
//! `schema/command-wire.json` already had the layout. `CommandPackage::
//! process_next_check_sum` `0x00945e20` reads exactly those two fields, logs
//! them through `commandpackage.cpp`'s reporter, and stores the value into
//! `[0x00cbee90]` indexed by the **package's** player — not by the type. The
//! shipped `riseofnations.exe` therefore still *receives* `0x3a`, but it has no
//! six-byte `CommandPackage` append anywhere in `.text` (`0x0094bae0`'s 102
//! call sites carry lengths 1, 2, 5, 7, 9, 10, 0xb, 0xd, 0xf, 0x11, 0x15, 0x19,
//! 0x35, 0x41, 0x209 — never 6), so it never *issues* one. The emitter is in
//! the older builds only, which is why the two streams never coexist.
//!
//! # `checksum_type` is `CheckSumTypes`
//!
//! `schema/types.json` carries the PDB enum with all sixteen enumerators, and
//! the observed values are exactly `0..=14` (`CHECKSUM_NUM = 15` never appears).
//! Note the enum is **not** the wire word order of `CheckSumsCommand`: it leads
//! with `CHECKSUM_ALL`, puts `CHECKSUM_RULES` second, and has no member for
//! `scenario_data` at all. [`CHECK_SUM_TYPES`] is the binding.
//!
//! # The sweep, measured
//!
//! Every `0x3a` recording emits exactly one record **per player per turn**,
//! every turn, starting at turn 2 — 0 non-contiguous turn steps corpus-wide.
//! The type is non-decreasing in 38 of the 39 files (the exception is the one
//! recording that desyncs on turn 3 and restarts its sweep), so the stream is a
//! single sweep `CHECKSUM_ALL → … → CHECKSUM_SCRIPT` that clamps on the last
//! type and stays there for the rest of the recording.
//!
//! Each type occupies a contiguous **run** of turns whose length is
//! `elements + 1`:
//!
//! * `all` 1 turn in 38/38 files, `rules` 1 in 38/38, `walls` 1 in 30/30,
//!   `groups` 1 in 27/27, `world` 1 in 25/25 — all channels with no per-element
//!   sub-check, plus `walls`, which the `0x39` corpus proves is empty in every
//!   recorded game (222,938 of 222,938 comparisons retail-empty);
//! * `leaders` **9 turns in 25 of 25 files**, and `CheckSums::check_leaders`
//!   `0x009375a0` iterates `[0x00e3a390, 0x00e71af0)` in `0x6eec` strides,
//!   i.e. exactly **8** leaders. 8 + 1;
//! * `ammo` and `deaths` are 1 turn in the 23 recordings where they are empty
//!   and 10–8,944 turns where they are not;
//! * `units`, `builds`, `guys`, `cities`, `items`, `goods` run for tens to
//!   thousands of turns.
//!
//! **The consequence this module depends on, and the only one it uses:** a run
//! of length 1 has no per-element record in it, so its single record is the
//! *whole channel*. That is confirmed twice — `walls`' single record is `1`,
//! the adler of nothing, in all 30 files; and `rules`' single record is
//! `0x12ba3104`, which [`crate::rules_channel`] independently produces by
//! walking 997,846 bytes of the recording's own carried Rules section.
//!
//! Whether the *last* record of a longer run is likewise the whole channel is
//! **not** established here, and nothing in this module assumes it. Runs longer
//! than one turn are counted and not compared.
//!
//! # What it does not license
//!
//! A `0x3a` agreement is exactly as substantive as the bytes our walker handed
//! the visitor for that channel, and no more. The scorer keeps its counts in
//! their own section so they can never be added to the `0x39` scoreboard by
//! accident, and it refuses to compare a channel whose producer is not
//! installed for that recording rather than scoring a vacuous `1 == 1`.

use crate::checksum::{Channels, CHANNEL_NAMES, NUM_CHANNELS, NUM_WALKED};
use crate::replay::Replay;
use crate::wire::CommandView;

/// `NextCheckSumCommand`.
pub const NEXT_CHECK_SUM_OPCODE: u8 = 0x3a;
/// `1 + 1 + 4`.
pub const NEXT_CHECK_SUM_WIRE_LEN: usize = 6;
/// `CommandPackage::process_next_check_sum`.
pub const PROCESS_NEXT_CHECK_SUM_VA: u32 = 0x0094_5e20;
/// The per-player table `process_next_check_sum` writes the value into.
pub const NEXT_CHECK_SUM_TABLE_VA: u32 = 0x00cb_ee90;
/// `CheckSums::check_leaders`, whose 8-slot loop pins the `leaders` run length.
pub const CHECK_LEADERS_VA: u32 = 0x0093_75a0;
/// The first type a sweep emits, and the turn it emits it on, in all 39 files.
pub const SWEEP_FIRST_TURN: i32 = 2;

/// One `CheckSumTypes` enumerator: its name and the `check_all` wire word it
/// names, where there is one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckSumTypeBinding {
    /// The PDB enumerator, verbatim.
    pub enumerator: &'static str,
    /// Index into [`crate::checksum::CHANNEL_NAMES`], or `None` where the
    /// enumerator names no single walked channel.
    pub channel: Option<usize>,
}

/// `CheckSumTypes` `0..=14`, from `schema/types.json`.
///
/// `CHECKSUM_ALL` maps to word 15 (`all`) because that is the only word of the
/// `CheckSumsCommand` tuple that is not a single channel; whether the older
/// builds' `all` is `check_all`'s **return** (which
/// [`crate::check_all`] measured to be channel 15, `script_run_time`) or the
/// wrapping **sum** of the fifteen is not established here, so nothing compares
/// against it.
pub const CHECK_SUM_TYPES: [CheckSumTypeBinding; 15] = [
    CheckSumTypeBinding {
        enumerator: "CHECKSUM_ALL",
        channel: Some(15),
    },
    CheckSumTypeBinding {
        enumerator: "CHECKSUM_RULES",
        channel: Some(12),
    },
    CheckSumTypeBinding {
        enumerator: "CHECKSUM_UNITS",
        channel: Some(0),
    },
    CheckSumTypeBinding {
        enumerator: "CHECKSUM_BUILDS",
        channel: Some(1),
    },
    CheckSumTypeBinding {
        enumerator: "CHECKSUM_WALLS",
        channel: Some(2),
    },
    CheckSumTypeBinding {
        enumerator: "CHECKSUM_AMMO",
        channel: Some(3),
    },
    CheckSumTypeBinding {
        enumerator: "CHECKSUM_DEATHS",
        channel: Some(4),
    },
    CheckSumTypeBinding {
        enumerator: "CHECKSUM_GROUPS",
        channel: Some(5),
    },
    CheckSumTypeBinding {
        enumerator: "CHECKSUM_GUYS",
        channel: Some(6),
    },
    CheckSumTypeBinding {
        enumerator: "CHECKSUM_LEADERS",
        channel: Some(7),
    },
    CheckSumTypeBinding {
        enumerator: "CHECKSUM_WORLD",
        channel: Some(11),
    },
    CheckSumTypeBinding {
        enumerator: "CHECKSUM_CITIES",
        channel: Some(8),
    },
    CheckSumTypeBinding {
        enumerator: "CHECKSUM_ITEMS",
        channel: Some(9),
    },
    CheckSumTypeBinding {
        enumerator: "CHECKSUM_GOODS",
        channel: Some(10),
    },
    CheckSumTypeBinding {
        enumerator: "CHECKSUM_SCRIPT",
        channel: Some(14),
    },
];
/// `CHECKSUM_NUM`, the terminator. Never observed on the wire.
pub const CHECK_SUM_NUM: u8 = 15;

/// Name of the channel a `checksum_type` selects, for reports.
pub fn type_channel_name(ty: u8) -> &'static str {
    match CHECK_SUM_TYPES.get(ty as usize).and_then(|b| b.channel) {
        Some(i) => CHANNEL_NAMES[i],
        None => "?",
    }
}

/// One decoded `0x3a` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NextCheckSumRecord {
    pub turn: i32,
    pub play: i32,
    pub ty: u8,
    pub value: u32,
}

/// A contiguous run of turns spent on one `checksum_type`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepRun {
    pub ty: u8,
    pub first_turn: i32,
    pub last_turn: i32,
    /// Turns in the run. `elements + 1` where the type has per-element checks.
    pub turns: usize,
    /// True when the recording ends inside this run, so its length is a lower
    /// bound rather than the sweep's own.
    pub truncated: bool,
}

impl SweepRun {
    /// A one-turn run carries the whole-channel checksum: there is no
    /// per-element record in it. See the module docs for the two confirmations.
    pub fn is_whole_channel(&self) -> bool {
        self.turns == 1 && !self.truncated
    }
}

/// Every `0x3a` record in a recording, in turn then player order.
pub fn records(rep: &Replay) -> Vec<NextCheckSumRecord> {
    let mut out = Vec::new();
    for t in &rep.turns {
        for p in &t.players {
            for c in &p.commands {
                if c.opcode != NEXT_CHECK_SUM_OPCODE {
                    continue;
                }
                let v = CommandView::new(c.opcode, &c.bytes);
                let (Some(ty), Some(value)) = (v.get("checksum_type"), v.get("checksum")) else {
                    continue;
                };
                out.push(NextCheckSumRecord {
                    turn: t.turn,
                    play: p.play,
                    ty: ty as u8,
                    value: value as u32,
                });
            }
        }
    }
    out
}

/// Reconstruct the sweep's runs from a recording's records.
pub fn sweep_runs(records: &[NextCheckSumRecord]) -> Vec<SweepRun> {
    let mut runs: Vec<SweepRun> = Vec::new();
    for r in records {
        match runs.last_mut() {
            Some(run)
                if run.ty == r.ty && (r.turn == run.last_turn || r.turn == run.last_turn + 1) =>
            {
                if r.turn == run.last_turn + 1 {
                    run.last_turn = r.turn;
                    run.turns += 1;
                }
            }
            _ => runs.push(SweepRun {
                ty: r.ty,
                first_turn: r.turn,
                last_turn: r.turn,
                turns: 1,
                truncated: false,
            }),
        }
    }
    if let Some(last) = runs.last_mut() {
        last.truncated = true;
    }
    runs
}

/// The `0x3a` cross-player control experiment.
///
/// Same shape as the `0x39` one in [`crate::replay`]: every turn on which two
/// or more players emitted a record for the same type is one comparison per
/// adjacent pair. This is the experiment that found the corpus's desyncs.
#[derive(Debug, Clone, Default)]
pub struct NextCheckSumCrossplay {
    pub comparisons: usize,
    pub identical: usize,
    pub disagreements: Vec<NextCheckSumDisagreement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextCheckSumDisagreement {
    pub turn: i32,
    pub ty: u8,
    pub a_play: i32,
    pub a_value: u32,
    pub b_play: i32,
    pub b_value: u32,
    /// `last_turn - turn`: how close to the end of the recording it is. Every
    /// disagreement in the corpus is within two turns of it.
    pub turns_before_end: i32,
}

/// Compare the players' records turn by turn, type by type.
pub fn crossplay(rep: &Replay, records: &[NextCheckSumRecord]) -> NextCheckSumCrossplay {
    let last_turn = rep.turns.last().map(|t| t.turn).unwrap_or(0);
    let mut out = NextCheckSumCrossplay::default();
    let mut i = 0;
    while i < records.len() {
        let mut j = i;
        while j < records.len()
            && records[j].turn == records[i].turn
            && records[j].ty == records[i].ty
        {
            j += 1;
        }
        for w in records[i..j].windows(2) {
            out.comparisons += 1;
            if w[0].value == w[1].value {
                out.identical += 1;
            } else {
                out.disagreements.push(NextCheckSumDisagreement {
                    turn: w[0].turn,
                    ty: w[0].ty,
                    a_play: w[0].play,
                    a_value: w[0].value,
                    b_play: w[1].play,
                    b_value: w[1].value,
                    turns_before_end: last_turn - w[0].turn,
                });
            }
        }
        i = j;
    }
    out
}

/// One channel's `0x3a` score for one recording.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NextCheckSumChannel {
    /// Records carrying this channel, of any run length.
    pub records: u64,
    /// Records inside a one-turn run, i.e. whole-channel values.
    pub whole_channel_turns: u64,
    /// Whole-channel turns where every reporting player agreed **and** our
    /// producer for the channel was installed. Only these are compared.
    pub compares: u64,
    pub matches: u64,
    /// Compares where our walker handed the visitor at least one byte.
    pub nontrivial_compares: u64,
    /// Compares where retail's own value was `1`.
    pub retail_empty_compares: u64,
    /// Whole-channel turns skipped because we hold no producer for the channel
    /// in this recording. Not compares — a vacuous `1 == 1` is not evidence.
    pub no_producer: u64,
    /// Whole-channel turns skipped because retail's own clients disagreed.
    pub retail_disagreed: u64,
    /// Bytes our walker handed the visitor on the last compare.
    pub our_bytes_walked: u64,
    /// Records inside runs longer than one turn, which this module does not
    /// interpret.
    pub per_element_records: u64,
}

/// A recording's whole `0x3a` result.
#[derive(Debug, Clone, Default)]
pub struct NextCheckSumResult {
    pub records: u64,
    pub runs: usize,
    pub sweep_monotone: bool,
    pub contiguous: bool,
    pub crossplay_comparisons: usize,
    pub crossplay_identical: usize,
    pub crossplay_disagreements: Vec<NextCheckSumDisagreement>,
    pub per_channel: [NextCheckSumChannel; NUM_CHANNELS],
    /// Run lengths in sweep order, for the record.
    pub run_turns: Vec<(u8, usize, bool)>,
}

impl NextCheckSumResult {
    pub fn present(&self) -> bool {
        self.records > 0
    }
}

/// Build everything but the per-turn comparisons, which need the simulation.
pub fn prepare(rep: &Replay) -> (Vec<NextCheckSumRecord>, Vec<SweepRun>, NextCheckSumResult) {
    let recs = records(rep);
    let runs = sweep_runs(&recs);
    let xp = crossplay(rep, &recs);
    let mut monotone = true;
    for w in runs.windows(2) {
        if w[1].ty < w[0].ty {
            monotone = false;
        }
    }
    let mut contiguous = true;
    for w in runs.windows(2) {
        if w[1].first_turn != w[0].last_turn + 1 {
            contiguous = false;
        }
    }
    let mut res = NextCheckSumResult {
        records: recs.len() as u64,
        runs: runs.len(),
        sweep_monotone: monotone,
        contiguous,
        crossplay_comparisons: xp.comparisons,
        crossplay_identical: xp.identical,
        crossplay_disagreements: xp.disagreements,
        per_channel: [NextCheckSumChannel::default(); NUM_CHANNELS],
        run_turns: runs.iter().map(|r| (r.ty, r.turns, r.truncated)).collect(),
    };
    for r in &recs {
        let Some(ch) = CHECK_SUM_TYPES.get(r.ty as usize).and_then(|b| b.channel) else {
            continue;
        };
        res.per_channel[ch].records += 1;
    }
    (recs, runs, res)
}

/// The turns on which a whole-channel comparison is available, with the value
/// every reporting player agreed on (`None` when they disagreed).
pub fn whole_channel_turns(
    recs: &[NextCheckSumRecord],
    runs: &[SweepRun],
) -> Vec<(i32, u8, Option<u32>)> {
    let mut out = Vec::new();
    for run in runs.iter().filter(|r| r.is_whole_channel()) {
        let mut agreed: Option<u32> = None;
        let mut contested = false;
        for r in recs
            .iter()
            .filter(|r| r.turn == run.first_turn && r.ty == run.ty)
        {
            match agreed {
                None => agreed = Some(r.value),
                Some(v) if v != r.value => contested = true,
                Some(_) => {}
            }
        }
        out.push((
            run.first_turn,
            run.ty,
            if contested { None } else { agreed },
        ));
    }
    out
}

/// Score one whole-channel turn against our state.
///
/// `ours`/`bytes`/`installed` come straight from `Simulation::check_all` and
/// `Simulation::installed_channels` on the turn in question, so this cannot see
/// anything the `0x39` path does not.
pub fn score_turn(
    res: &mut NextCheckSumResult,
    ty: u8,
    recorded: Option<u32>,
    ours: &Channels,
    bytes: &[u64; NUM_WALKED],
    installed: &[bool; NUM_WALKED],
) {
    let Some(ch) = CHECK_SUM_TYPES.get(ty as usize).and_then(|b| b.channel) else {
        return;
    };
    let c = &mut res.per_channel[ch];
    c.whole_channel_turns += 1;
    let Some(rec) = recorded else {
        c.retail_disagreed += 1;
        return;
    };
    // `all` is word 15 and no producer claims it; the walked channels gate on
    // their own installation bit.
    if ch >= NUM_WALKED || !installed[ch] {
        c.no_producer += 1;
        return;
    }
    c.compares += 1;
    c.our_bytes_walked = bytes[ch];
    if bytes[ch] > 0 {
        c.nontrivial_compares += 1;
    }
    if rec == 1 {
        c.retail_empty_compares += 1;
    }
    if ours.0[ch] == rec {
        c.matches += 1;
    }
}

/// Fill in `per_element_records` once every run is known.
pub fn finish(res: &mut NextCheckSumResult, recs: &[NextCheckSumRecord], runs: &[SweepRun]) {
    for run in runs.iter().filter(|r| !r.is_whole_channel()) {
        let Some(ch) = CHECK_SUM_TYPES.get(run.ty as usize).and_then(|b| b.channel) else {
            continue;
        };
        let n = recs
            .iter()
            .filter(|r| r.ty == run.ty && r.turn >= run.first_turn && r.turn <= run.last_turn)
            .count() as u64;
        res.per_channel[ch].per_element_records += n;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(turn: i32, play: i32, ty: u8, value: u32) -> NextCheckSumRecord {
        NextCheckSumRecord {
            turn,
            play,
            ty,
            value,
        }
    }

    #[test]
    fn the_binding_covers_every_pdb_enumerator_and_no_more() {
        assert_eq!(CHECK_SUM_TYPES.len(), CHECK_SUM_NUM as usize);
        assert_eq!(CHECK_SUM_TYPES[0].enumerator, "CHECKSUM_ALL");
        assert_eq!(CHECK_SUM_TYPES[14].enumerator, "CHECKSUM_SCRIPT");
        // The enum is not the wire word order: rules is word 12 but type 1.
        assert_eq!(CHECK_SUM_TYPES[1].channel, Some(12));
        assert_eq!(CHANNEL_NAMES[12], "rules");
        assert_eq!(type_channel_name(4), "walls");
        assert_eq!(type_channel_name(10), "world");
        // No enumerator names `scenario_data`; it is word 13.
        assert!(CHECK_SUM_TYPES.iter().all(|b| b.channel != Some(13)));
    }

    #[test]
    fn runs_are_contiguous_and_the_terminal_one_is_truncated() {
        let recs = vec![
            rec(2, 0, 0, 7),
            rec(2, 1, 0, 7),
            rec(3, 0, 1, 9),
            rec(3, 1, 1, 9),
            rec(4, 0, 2, 11),
            rec(4, 1, 2, 11),
            rec(5, 0, 2, 12),
            rec(5, 1, 2, 12),
        ];
        let runs = sweep_runs(&recs);
        assert_eq!(runs.len(), 3);
        assert_eq!(
            (runs[0].ty, runs[0].turns, runs[0].truncated),
            (0, 1, false)
        );
        assert_eq!(
            (runs[1].ty, runs[1].turns, runs[1].truncated),
            (1, 1, false)
        );
        assert_eq!((runs[2].ty, runs[2].turns, runs[2].truncated), (2, 2, true));
        assert!(runs[0].is_whole_channel());
        assert!(runs[1].is_whole_channel());
        assert!(!runs[2].is_whole_channel());
    }

    #[test]
    fn a_one_turn_terminal_run_is_not_treated_as_a_whole_channel_value() {
        // The recording stops the instant the sweep advances; we cannot know
        // the run was going to be one turn long, so it is refused.
        let recs = vec![rec(2, 0, 0, 7), rec(3, 0, 1, 9)];
        let runs = sweep_runs(&recs);
        assert!(runs[0].is_whole_channel());
        assert!(!runs[1].is_whole_channel());
    }

    #[test]
    fn contested_turns_are_never_compared() {
        let recs = vec![rec(2, 0, 1, 5), rec(2, 1, 1, 6), rec(3, 0, 2, 8)];
        let runs = sweep_runs(&recs);
        let turns = whole_channel_turns(&recs, &runs);
        assert_eq!(turns, vec![(2, 1, None)]);

        let mut res = NextCheckSumResult::default();
        let ours = Channels([0; NUM_CHANNELS]);
        let bytes = [0u64; NUM_WALKED];
        let installed = [true; NUM_WALKED];
        score_turn(&mut res, 1, None, &ours, &bytes, &installed);
        assert_eq!(res.per_channel[12].retail_disagreed, 1);
        assert_eq!(res.per_channel[12].compares, 0);
    }

    #[test]
    fn an_uninstalled_producer_is_not_a_compare() {
        let mut res = NextCheckSumResult::default();
        let mut ours = Channels([1; NUM_CHANNELS]);
        ours.0[2] = 1;
        let bytes = [0u64; NUM_WALKED];
        let mut installed = [false; NUM_WALKED];
        // walls, retail-empty, our value coincidentally equal: still not scored.
        score_turn(&mut res, 4, Some(1), &ours, &bytes, &installed);
        assert_eq!(res.per_channel[2].no_producer, 1);
        assert_eq!(res.per_channel[2].compares, 0);
        assert_eq!(res.per_channel[2].matches, 0);
        // With a producer it is a compare, and an honestly *trivial* one.
        installed[2] = true;
        score_turn(&mut res, 4, Some(1), &ours, &bytes, &installed);
        assert_eq!(res.per_channel[2].compares, 1);
        assert_eq!(res.per_channel[2].matches, 1);
        assert_eq!(res.per_channel[2].nontrivial_compares, 0);
        assert_eq!(res.per_channel[2].retail_empty_compares, 1);
    }

    #[test]
    fn all_is_word_fifteen_and_has_no_producer_to_compare() {
        let mut res = NextCheckSumResult::default();
        let ours = Channels([1; NUM_CHANNELS]);
        let bytes = [0u64; NUM_WALKED];
        let installed = [true; NUM_WALKED];
        score_turn(&mut res, 0, Some(1), &ours, &bytes, &installed);
        assert_eq!(res.per_channel[15].no_producer, 1);
        assert_eq!(res.per_channel[15].compares, 0);
    }
}
