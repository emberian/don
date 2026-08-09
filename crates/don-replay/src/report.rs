//! JSON summary, written to `schema/replay-validation.json`.
//!
//! Hand-rolled rather than serde: this crate has no dependencies so a green
//! `cargo test` never touches the registry, matching `don-net`'s choice. The
//! shape is stable so runs are comparable over time — that is the whole point
//! of writing it to `schema/` instead of printing it.

use crate::checksum::{CHANNEL_NAMES, NUM_CHANNELS, NUM_WALKED};
use crate::harness::RunResult;

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

/// Corpus-level totals, computed from the per-file runs so the two can never
/// disagree.
pub struct Totals {
    pub files: usize,
    pub files_with_checksums: usize,
    pub turns: usize,
    pub checksum_packets: usize,
    pub checksum_total_ok: usize,
    pub checksum_shape_ok: usize,
    pub crossplay_comparisons: usize,
    pub crossplay_identical: usize,
    pub crossplay_per_channel: [usize; NUM_CHANNELS],
    pub crossplay_stamp_comparisons: usize,
    pub crossplay_stamp_identical: usize,
    pub crossplay_stamp_wrong_turn: usize,
    /// Per channel: total compares, total matches, and the best single-file
    /// consecutive survival.
    pub compares: [u64; NUM_CHANNELS],
    pub matches: [u64; NUM_CHANNELS],
    pub trivial: [u64; NUM_CHANNELS],
    pub best_survived: [u32; NUM_CHANNELS],
    pub sim_commands: usize,
    pub lockstep_commands: usize,
    pub presentation_commands: usize,
    pub opcode_counts: std::collections::BTreeMap<u8, usize>,
}

impl Totals {
    pub fn of(runs: &[RunResult]) -> Totals {
        let mut t = Totals {
            files: runs.len(),
            files_with_checksums: 0,
            turns: 0,
            checksum_packets: 0,
            checksum_total_ok: 0,
            checksum_shape_ok: 0,
            crossplay_comparisons: 0,
            crossplay_identical: 0,
            crossplay_per_channel: [0; NUM_CHANNELS],
            crossplay_stamp_comparisons: 0,
            crossplay_stamp_identical: 0,
            crossplay_stamp_wrong_turn: 0,
            compares: [0; NUM_CHANNELS],
            matches: [0; NUM_CHANNELS],
            trivial: [0; NUM_CHANNELS],
            best_survived: [0; NUM_CHANNELS],
            sim_commands: 0,
            lockstep_commands: 0,
            presentation_commands: 0,
            opcode_counts: Default::default(),
        };
        for r in runs {
            t.turns += r.turns_total;
            t.checksum_packets += r.checksum_packets;
            t.checksum_total_ok += r.checksum_total_ok;
            t.checksum_shape_ok += r.checksum_shape_ok;
            t.crossplay_comparisons += r.crossplay_comparisons;
            t.crossplay_identical += r.crossplay_identical;
            t.crossplay_stamp_comparisons += r.crossplay_stamp_comparisons;
            t.crossplay_stamp_identical += r.crossplay_stamp_identical;
            t.crossplay_stamp_wrong_turn += r.crossplay_stamp_wrong_turn;
            t.sim_commands += r.sim_commands;
            t.lockstep_commands += r.lockstep_commands;
            t.presentation_commands += r.presentation_commands;
            if r.checksum_packets > 0 {
                t.files_with_checksums += 1;
            }
            for (op, n) in &r.opcode_counts {
                *t.opcode_counts.entry(*op).or_insert(0) += n;
            }
            for c in 0..NUM_CHANNELS {
                t.crossplay_per_channel[c] += r.crossplay_per_channel[c];
                t.compares[c] += r.channels[c].compares as u64;
                t.matches[c] += r.channels[c].matches as u64;
                t.trivial[c] += r.channels[c].trivial_matches as u64;
                t.best_survived[c] = t.best_survived[c].max(r.channels[c].survived);
            }
        }
        t
    }
}

pub fn to_json(runs: &[RunResult], generated_by: &str) -> String {
    let t = Totals::of(runs);
    let mut s = String::new();
    s.push_str("{\n");
    s.push_str(&format!("  \"generated_by\": \"{}\",\n", esc(generated_by)));
    s.push_str("  \"what\": \"Replay-driven validation: a real .rcx lockstep command stream is stepped turn by turn and our 15 component DataWalk checksum channels plus aggregate `all` are compared against the recorded CheckSumsCommand (opcode 0x39). `survived` is consecutive agreeing turns from the recording's first checksummed turn. `trivial` counts agreements where our walker touched zero bytes.\",\n");
    s.push_str("  \"checksum_source\": \"CheckSums::check_all 0x00936560; packet builder 0x00940770; adler32 0x00a46830 (Tier B, 500k differential calls, 0 mismatches)\",\n");
    s.push_str("  \"walk_source\": \"schema/state-schema.json -> crates/don-replay/src/walk_gen.rs (generated)\",\n");
    s.push_str(&format!(
        "  \"totals\": {{\n    \"files\": {}, \"files_with_checksums\": {}, \"turns\": {},\n",
        t.files, t.files_with_checksums, t.turns
    ));
    s.push_str(&format!(
        "    \"checksum_packets\": {}, \"checksum_total_consistent\": {}, \"checksum_adler_shaped\": {},\n",
        t.checksum_packets, t.checksum_total_ok, t.checksum_shape_ok
    ));
    s.push_str(&format!(
        "    \"crossplay_by_group\": {{ \"comparisons\": {}, \"identical\": {} }},\n",
        t.crossplay_comparisons, t.crossplay_identical
    ));
    s.push_str(&format!(
        "    \"crossplay_by_stamp\": {{ \"comparisons\": {}, \"identical\": {}, \"disagreements_comparing_different_turns\": {} }},\n",
        t.crossplay_stamp_comparisons,
        t.crossplay_stamp_identical,
        t.crossplay_stamp_wrong_turn
    ));
    s.push_str(&format!(
        "    \"commands\": {{ \"sim\": {}, \"lockstep\": {}, \"presentation\": {} }},\n",
        t.sim_commands, t.lockstep_commands, t.presentation_commands
    ));
    s.push_str("    \"per_channel\": {\n");
    for (i, name) in CHANNEL_NAMES.iter().enumerate() {
        s.push_str(&format!(
            "      \"{name}\": {{ \"compares\": {}, \"matches\": {}, \"trivial\": {}, \"best_survived_turns\": {}, \"crossplay_disagreements\": {}, \"mutable\": {} }}{}\n",
            t.compares[i],
            t.matches[i],
            t.trivial[i],
            t.best_survived[i],
            t.crossplay_per_channel[i],
            crate::checksum::CHANNELS[i].is_mutable(),
            if i + 1 == NUM_CHANNELS { "" } else { "," }
        ));
    }
    s.push_str("    },\n");
    s.push_str("    \"opcode_counts\": {\n");
    let n = t.opcode_counts.len();
    for (k, (op, cnt)) in t.opcode_counts.iter().enumerate() {
        s.push_str(&format!(
            "      \"0x{op:02x}\": {{ \"struct\": \"{}\", \"class\": \"{:?}\", \"count\": {cnt} }}{}\n",
            crate::wire::COMMAND_STRUCT.get(*op as usize).copied().unwrap_or("?"),
            crate::wire::classify(*op),
            if k + 1 == n { "" } else { "," }
        ));
    }
    s.push_str("    }\n  },\n");

    s.push_str("  \"files\": [\n");
    for (k, r) in runs.iter().enumerate() {
        s.push_str("    {\n");
        s.push_str(&format!("      \"file\": \"{}\",\n", esc(&r.file)));
        s.push_str(&format!(
            "      \"version\": {},\n",
            r.version
                .as_ref()
                .map(|v| format!("\"{}\"", esc(v)))
                .unwrap_or_else(|| "null".into())
        ));
        s.push_str(&format!(
            "      \"phase\": \"{}\", \"latency_turns\": {},\n",
            r.phase.name(),
            r.latency
        ));
        s.push_str(&format!(
            "      \"turns\": {}, \"turns_checksummed\": {}, \"first_turn\": {}, \"last_turn\": {},\n",
            r.turns_total, r.turns_checksummed, r.first_turn, r.last_turn
        ));
        s.push_str(&format!(
            "      \"players\": {:?}, \"frames_per_turn\": {},\n",
            r.players,
            r.frames_per_turn
                .map(|f| format!("{f:.3}"))
                .unwrap_or_else(|| "null".into())
        ));
        s.push_str(&format!(
            "      \"packages\": {}, \"packages_decoded\": {},\n",
            r.packages, r.packages_decoded
        ));
        s.push_str(&format!(
            "      \"checksum_packets\": {}, \"checksum_total_consistent\": {}, \"checksum_adler_shaped\": {},\n",
            r.checksum_packets, r.checksum_total_ok, r.checksum_shape_ok
        ));
        s.push_str(&format!(
            "      \"rules_channel_constant\": {},\n",
            r.rules_constant
                .map(|v| format!("\"0x{v:08x}\""))
                .unwrap_or_else(|| "null".into())
        ));
        s.push_str(&format!(
            "      \"crossplay\": {{ \"by_group\": {{ \"comparisons\": {}, \"identical\": {} }}, \"by_stamp\": {{ \"comparisons\": {}, \"identical\": {}, \"disagreements_comparing_different_turns\": {} }} }},\n",
            r.crossplay_comparisons,
            r.crossplay_identical,
            r.crossplay_stamp_comparisons,
            r.crossplay_stamp_identical,
            r.crossplay_stamp_wrong_turn
        ));
        s.push_str(&format!(
            "      \"orders\": {{ \"sim\": {}, \"typed\": {}, \"applied\": {} }},\n",
            r.sim_commands, r.typed_orders, r.orders_applied
        ));
        s.push_str("      \"channels\": {\n");
        for (i, name) in CHANNEL_NAMES.iter().enumerate() {
            let c = &r.channels[i];
            s.push_str(&format!(
                "        \"{name}\": {{ \"survived\": {}, \"first_divergence_turn\": {}, \"expected\": \"0x{:08x}\", \"got\": \"0x{:08x}\", \"compares\": {}, \"matches\": {}, \"trivial\": {}, \"retail_disagreed\": {} }}{}\n",
                c.survived,
                c.first_divergence_turn.map(|t| t.to_string()).unwrap_or_else(|| "null".into()),
                c.expected,
                c.got,
                c.compares,
                c.matches,
                c.trivial_matches,
                c.retail_disagreed,
                if i + 1 == NUM_CHANNELS { "" } else { "," }
            ));
        }
        s.push_str("      }\n");
        s.push_str(if k + 1 == runs.len() {
            "    }\n"
        } else {
            "    },\n"
        });
    }
    s.push_str("  ]\n}\n");
    let _ = NUM_WALKED;
    s
}
