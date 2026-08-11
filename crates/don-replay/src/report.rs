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
    /// Matches on channels with no producer at all. See
    /// `ChannelResult::unmodelled_matches` — this is the column that says how
    /// much of the scoreboard is vacuous.
    pub unmodelled: [u64; NUM_CHANNELS],
    /// Compares where our walker touched bytes.
    pub nontrivial: [u64; NUM_CHANNELS],
    /// Comparisons and equalities admitted by the complete exact-owner gate.
    /// These are separate from the legacy raw/nontrivial counters so historical
    /// reports remain comparable.
    pub substantive_compares: [u64; NUM_CHANNELS],
    pub substantive_matches: [u64; NUM_CHANNELS],
    /// Compares where retail's own value was 1.
    pub retail_empty: [u64; NUM_CHANNELS],
    pub best_survived: [u32; NUM_CHANNELS],
    pub sim_commands: usize,
    pub lockstep_commands: usize,
    pub presentation_commands: usize,
    pub opcode_counts: std::collections::BTreeMap<u8, usize>,
    /// `NextCheckSumCommand` `0x3a`: the second, per-subsystem checksum stream,
    /// carried by the recordings that carry no `0x39` tuple at all.
    pub next_files: usize,
    pub next_records: u64,
    pub next_crossplay_comparisons: usize,
    pub next_crossplay_identical: usize,
    pub next_crossplay_disagreements: usize,
    /// Disagreements at or within two turns of the recording's last turn.
    pub next_crossplay_disagreements_at_end: usize,
    pub next_sweep_monotone_files: usize,
    pub next_records_per_channel: [u64; NUM_CHANNELS],
    pub next_whole_channel_turns: [u64; NUM_CHANNELS],
    pub next_compares: [u64; NUM_CHANNELS],
    pub next_matches: [u64; NUM_CHANNELS],
    pub next_nontrivial: [u64; NUM_CHANNELS],
    pub next_retail_empty: [u64; NUM_CHANNELS],
    pub next_no_producer: [u64; NUM_CHANNELS],
    pub next_retail_disagreed: [u64; NUM_CHANNELS],
    pub next_per_element: [u64; NUM_CHANNELS],
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
            unmodelled: [0; NUM_CHANNELS],
            nontrivial: [0; NUM_CHANNELS],
            substantive_compares: [0; NUM_CHANNELS],
            substantive_matches: [0; NUM_CHANNELS],
            retail_empty: [0; NUM_CHANNELS],
            best_survived: [0; NUM_CHANNELS],
            sim_commands: 0,
            lockstep_commands: 0,
            presentation_commands: 0,
            opcode_counts: Default::default(),
            next_files: 0,
            next_records: 0,
            next_crossplay_comparisons: 0,
            next_crossplay_identical: 0,
            next_crossplay_disagreements: 0,
            next_crossplay_disagreements_at_end: 0,
            next_sweep_monotone_files: 0,
            next_records_per_channel: [0; NUM_CHANNELS],
            next_whole_channel_turns: [0; NUM_CHANNELS],
            next_compares: [0; NUM_CHANNELS],
            next_matches: [0; NUM_CHANNELS],
            next_nontrivial: [0; NUM_CHANNELS],
            next_retail_empty: [0; NUM_CHANNELS],
            next_no_producer: [0; NUM_CHANNELS],
            next_retail_disagreed: [0; NUM_CHANNELS],
            next_per_element: [0; NUM_CHANNELS],
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
            let n = &r.next_checksum;
            if n.present() {
                t.next_files += 1;
                t.next_records += n.records;
                t.next_crossplay_comparisons += n.crossplay_comparisons;
                t.next_crossplay_identical += n.crossplay_identical;
                t.next_crossplay_disagreements += n.crossplay_disagreements.len();
                t.next_crossplay_disagreements_at_end += n
                    .crossplay_disagreements
                    .iter()
                    .filter(|d| d.turns_before_end <= 2)
                    .count();
                if n.sweep_monotone {
                    t.next_sweep_monotone_files += 1;
                }
            }
            for c in 0..NUM_CHANNELS {
                t.next_records_per_channel[c] += n.per_channel[c].records;
                t.next_whole_channel_turns[c] += n.per_channel[c].whole_channel_turns;
                t.next_compares[c] += n.per_channel[c].compares;
                t.next_matches[c] += n.per_channel[c].matches;
                t.next_nontrivial[c] += n.per_channel[c].nontrivial_compares;
                t.next_retail_empty[c] += n.per_channel[c].retail_empty_compares;
                t.next_no_producer[c] += n.per_channel[c].no_producer;
                t.next_retail_disagreed[c] += n.per_channel[c].retail_disagreed;
                t.next_per_element[c] += n.per_channel[c].per_element_records;
            }
            for c in 0..NUM_CHANNELS {
                t.crossplay_per_channel[c] += r.crossplay_per_channel[c];
                t.compares[c] += r.channels[c].compares as u64;
                t.matches[c] += r.channels[c].matches as u64;
                t.trivial[c] += r.channels[c].trivial_matches as u64;
                t.unmodelled[c] += r.channels[c].unmodelled_matches as u64;
                t.nontrivial[c] += r.channels[c].nontrivial_compares as u64;
                t.substantive_compares[c] += r.channels[c].substantive_compares as u64;
                t.substantive_matches[c] += r.channels[c].substantive_matches as u64;
                t.retail_empty[c] += r.channels[c].retail_empty_compares as u64;
                t.best_survived[c] = t.best_survived[c].max(r.channels[c].survived);
            }
        }
        t
    }
}

pub fn to_json(runs: &[RunResult], generated_by: &str) -> String {
    let t = Totals::of(runs);
    let mut leader_prefix_corpus =
        crate::leader_prefix_ledger::LeaderPrefixCorpusCoverage::default();
    for run in runs {
        if let Some(coverage) = run.initial_leader_prefix {
            leader_prefix_corpus.observe(
                run.checksum_packets > 0,
                run.initial_active_players,
                coverage,
            );
        }
    }
    let mut s = String::new();
    s.push_str("{\n");
    s.push_str(&format!("  \"generated_by\": \"{}\",\n", esc(generated_by)));
    s.push_str("  \"what\": \"Replay-driven validation: a real .rcx lockstep command stream is stepped turn by turn and our 15 component DataWalk checksum channels plus aggregate `all` are compared against the recorded CheckSumsCommand (opcode 0x39). `survived`, `matches`, and `nontrivial_compares` are retained as historical raw counters. Only `substantive_matches` establishes compatibility: its compare must have an installed exact producer, walk at least one byte, report zero unsourced bytes, and complete with no missed traversal operation. `trivial` counts agreements where our walker touched zero bytes; `unmodelled` is the subset of those on channels don-sim has no producer for at all, which are not evidence about anything. The static Rules producer independently projects the replay-carried SaveGame section through the checksum-only traversal and admits it only when all four retail checkpoints match; it never copies the recorded wire checksum. The script_run_time producer walks the four-byte ScriptFile::script_files count that RunTimeEnv::walk_data 0x009c41a0 always hashes; agreement there means only that the recording loaded no BHS program (true of 7 of the 21 checksum-bearing files) and is NOT evidence about script semantics. The scenario_data producer walks the complete 8,453-byte ScenarioData image that ScenarioFuncSet::init 0x00a03c30 leaves behind, with its two shipped internal_strings.xml ordinals (5958, 5959) bound positionally; it is FROZEN there because no don-sim path writes units_killed / builds_destroyed / city_lost_to, so its agreement means `no scenario counter has moved yet` and expires at the recording's first kill or city capture. It is not empty-state agreement: the walker touches 8,453 real bytes on every compare. The groups producer walks the 36,896-byte state Groups::clear 0x00713f20 leaves at Game::init -- 512 slots cleared by Group::clear 0x00713e80 plus the 32-byte last_group tail check_groups hashes through GroupsData::const_last_group -- and is FROZEN there for the same reason: nothing in don-sim drives Groups::push_group or any Group::action_*, so its agreement means `no group slot has been touched yet` and expires at the recording's first group command. `retail_empty_compares` counts compares where the ENGINE's own value was 1, and `retail_first_nonempty_turn` is the turn it stopped being 1 -- the deadline a producer has to meet.\",\n");
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
    s.push_str(&format!(
        "    \"sparse_leaders_prefix\": {{ \"what\": \"exact setup-owned checksum spans only; never installed as a complete Leaders channel producer; the player-row projection is not ownership because duplicate Player::who rows collapse\", \"derived_files\": {}, \"refused_files\": {}, \"all_exact_owned_checksum_bytes\": {}, \"checksum_bearing_files\": {}, \"checksum_bearing_exact_owned_checksum_bytes\": {}, \"checksum_bearing_active_leader_rows\": {}, \"checksum_bearing_present_player_rows\": {}, \"checksum_bearing_player_row_projection_bytes\": {}, \"checksum_bearing_duplicate_who_collapsed_rows\": {}, \"checksum_bearing_player_row_projection_overcount_bytes\": {}, \"checksum_bearing_human_only_candidates\": {}, \"checksum_bearing_expired_by_nonhuman\": {}, \"prefix_contributed_substantive_leaders_compares\": {}, \"prefix_contributed_substantive_leaders_matches\": {} }},\n",
        leader_prefix_corpus.files,
        runs.len().saturating_sub(leader_prefix_corpus.files),
        leader_prefix_corpus.all_exact_owned_checksum_bytes,
        leader_prefix_corpus.checksum_bearing_files,
        leader_prefix_corpus.checksum_bearing_exact_owned_checksum_bytes,
        leader_prefix_corpus.checksum_bearing_active_rows,
        leader_prefix_corpus.checksum_bearing_present_player_rows,
        leader_prefix_corpus.checksum_bearing_player_row_projection_bytes,
        leader_prefix_corpus.checksum_bearing_duplicate_who_collapsed_rows,
        leader_prefix_corpus.checksum_bearing_player_row_projection_overcount_bytes,
        leader_prefix_corpus.checksum_bearing_human_only_candidates,
        leader_prefix_corpus.checksum_bearing_expired_by_nonhuman,
        leader_prefix_corpus.prefix_contributed_substantive_leaders_compares,
        leader_prefix_corpus.prefix_contributed_substantive_leaders_matches,
    ));
    s.push_str("    \"per_channel\": {\n");
    for (i, name) in CHANNEL_NAMES.iter().enumerate() {
        s.push_str(&format!(
            "      \"{name}\": {{ \"compares\": {}, \"matches\": {}, \"trivial\": {}, \"unmodelled\": {}, \"nontrivial_compares\": {}, \"substantive_compares\": {}, \"substantive_matches\": {}, \"retail_empty_compares\": {}, \"best_survived_turns\": {}, \"crossplay_disagreements\": {}, \"mutable\": {} }}{}\n",
            t.compares[i],
            t.matches[i],
            t.trivial[i],
            t.unmodelled[i],
            t.nontrivial[i],
            t.substantive_compares[i],
            t.substantive_matches[i],
            t.retail_empty[i],
            t.best_survived[i],
            t.crossplay_per_channel[i],
            crate::checksum::CHANNELS[i].is_mutable(),
            if i + 1 == NUM_CHANNELS { "" } else { "," }
        ));
    }
    s.push_str("    },\n");
    s.push_str(&format!(
        "    \"next_checksum\": {{\n      \"what\": \"NextCheckSumCommand 0x3a, the second lockstep checksum stream. Six bytes: CheckSumTypes index + one subsystem checksum. Emitted only by the pre-2018 engine builds; the shipped riseofnations.exe still processes it (CommandPackage::process_next_check_sum 0x00945e20) but has no six-byte package append, so it never issues one. NO recording carries both streams. Each recording emits one record per player per turn from turn 2, sweeping the types in order; a type's run of turns is `elements + 1`, so a ONE-TURN run carries the whole-channel checksum and a longer run contains per-element records this module does not interpret. Only one-turn runs are compared, and only when our producer for that channel is installed for that recording -- a vacuous 1 == 1 against an absent producer is counted as no_producer, not as a match. These numbers are NOT part of the 0x39 scoreboard above.\",\n      \"files\": {}, \"records\": {}, \"sweep_monotone_files\": {},\n      \"crossplay\": {{ \"comparisons\": {}, \"identical\": {}, \"disagreements\": {}, \"disagreements_within_two_turns_of_the_last_turn\": {} }},\n      \"per_channel\": {{\n",
        t.next_files,
        t.next_records,
        t.next_sweep_monotone_files,
        t.next_crossplay_comparisons,
        t.next_crossplay_identical,
        t.next_crossplay_disagreements,
        t.next_crossplay_disagreements_at_end
    ));
    for (i, name) in CHANNEL_NAMES.iter().enumerate() {
        s.push_str(&format!(
            "        \"{name}\": {{ \"records\": {}, \"whole_channel_turns\": {}, \"compares\": {}, \"matches\": {}, \"nontrivial_compares\": {}, \"retail_empty_compares\": {}, \"no_producer\": {}, \"retail_disagreed\": {}, \"per_element_records\": {} }}{}\n",
            t.next_records_per_channel[i],
            t.next_whole_channel_turns[i],
            t.next_compares[i],
            t.next_matches[i],
            t.next_nontrivial[i],
            t.next_retail_empty[i],
            t.next_no_producer[i],
            t.next_retail_disagreed[i],
            t.next_per_element[i],
            if i + 1 == NUM_CHANNELS { "" } else { "," }
        ));
    }
    s.push_str("      }\n    },\n");
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
        let style_json = if let Some(key) = &r.initial_item_style_key {
            let sites = r
                .initial_item_known_direct_rng_sites
                .iter()
                .map(|va| format!("\"0x{va:08x}\""))
                .collect::<Vec<_>>()
                .join(", ");
            let executed_sites = r
                .initial_continent
                .as_ref()
                .map(|receipt| {
                    receipt
                        .direct_rng_sites
                        .iter()
                        .map(|va| format!("\"0x{va:08x}\""))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let dynamic_draw_count = r
                .initial_continent
                .as_ref()
                .map(|receipt| receipt.direct_rng_sites.len().to_string())
                .unwrap_or_else(|| "null".into());
            let rng_state = r
                .initial_continent
                .as_ref()
                .map(|receipt| {
                    format!(
                        "{{ \"initial\": \"0x{:08x}\", \"orientation\": {}, \"at_boundary\": \"0x{:08x}\", \"retry_attempt\": {}, \"regions_cleared\": {}, \"region_seeds\": {}, \"region_growths\": {}, \"starts_added\": {} }}",
                        receipt.rng_initial as u32,
                        receipt.orientation,
                        receipt.rng_final as u32,
                        receipt.retry_attempt,
                        receipt.regions_cleared,
                        receipt.region_seeds.len(),
                        receipt.region_growths.len(),
                        receipt.starts_added,
                    )
                })
                .unwrap_or_else(|| "null".into());
            format!(
                "{{ \"key\": \"{}\", \"filename\": {}, \"terrain_groups\": {{ \"default\": {}, \"selected\": {}, \"selected_section_present\": {}, \"effective\": {} }}, \"goodies\": {{ \"default\": {}, \"selected\": {}, \"selected_section_present\": {}, \"effective\": {} }}, \"known_direct_rng_sites\": [{}], \"executed_direct_rng_sites\": [{}], \"dynamic_draw_count\": {}, \"continent_rng\": {} }}",
                esc(key),
                r.initial_item_style_filename
                    .as_ref()
                    .map(|v| format!("\"{}\"", esc(v)))
                    .unwrap_or_else(|| "null".into()),
                r.initial_item_default_terrain_groups,
                r.initial_item_selected_terrain_groups,
                r.initial_item_selected_terrain_groups_present,
                r.initial_item_effective_terrain_groups,
                r.initial_item_default_goodies,
                r.initial_item_selected_goodies,
                r.initial_item_selected_goodies_present,
                r.initial_item_effective_goodies,
                sites,
                executed_sites,
                dynamic_draw_count,
                rng_state,
            )
        } else {
            "null".into()
        };
        let style_error = r
            .initial_item_style_error
            .as_ref()
            .map(|v| format!("\"{}\"", esc(v)))
            .unwrap_or_else(|| "null".into());
        let tile_selection = r
            .initial_tile_selection
            .as_ref()
            .map(|selection| {
                format!(
                    "{{ \"tileset\": \"{}\", \"draw\": {}, \"bucket\": {}, \"passes\": {}, \"main_rng_after\": \"0x{:08x}\" }}",
                    esc(&selection.tileset),
                    selection
                        .draw
                        .map(|draw| draw.to_string())
                        .unwrap_or_else(|| "null".into()),
                    selection.bucket,
                    selection.passes.len(),
                    selection.main_random_state_after as u32,
                )
            })
            .unwrap_or_else(|| "null".into());
        let fertility_error = r
            .initial_fertility_error
            .as_ref()
            .map(|v| format!("\"{}\"", esc(v)))
            .unwrap_or_else(|| "null".into());
        let fill_fertile_cells = r
            .initial_fill_fertile_cells
            .map(|cells| cells.to_string())
            .unwrap_or_else(|| "null".into());
        let place_all = r
            .initial_place_all
            .as_ref()
            .map(|advance| {
                let arms = advance
                    .selected_groups
                    .iter()
                    .map(|row| {
                        format!(
                            "{{ \"group\": {}, \"type\": {}, \"pattern\": {}, \"clumps\": {} }}",
                            row.group_index, row.group_type, row.pattern, row.clumps
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "{{ \"entry\": \"0x{:08x}\", \"stop\": \"{}\", \"primitive\": \"0x{:08x}\", \"stop_group\": {}, \"completed_groups\": {}, \"catalog_groups\": {}, \"rng_at_entry\": \"0x{:08x}\", \"oil_good_policy\": \"{:?}\", \"mountain_range_lengths\": {}, \"mountain_randomize_draws\": {}, \"selected_groups\": [{}], \"committed_world_bytes\": 0 }}",
                    advance.entry_va,
                    advance.stop.name(),
                    advance.stop.primitive_va(),
                    advance
                        .stop
                        .group_index()
                        .map(|index| index.to_string())
                        .unwrap_or_else(|| "null".into()),
                    advance.completed_groups.len(),
                    advance.catalog_groups,
                    advance.random_state_at_entry as u32,
                    advance.oil_good_policy,
                    advance
                        .mountain_range_lengths
                        .map(|lengths| format!("[{}, {}, {}]", lengths[0], lengths[1], lengths[2]))
                        .unwrap_or_else(|| "null".into()),
                    advance
                        .mountain_randomize_draws
                        .map(|draws| draws.to_string())
                        .unwrap_or_else(|| "null".into()),
                    arms,
                )
            })
            .unwrap_or_else(|| {
                r.initial_place_all_error
                    .as_ref()
                    .map(|error| format!("{{ \"error\": \"{}\" }}", esc(error)))
                    .unwrap_or_else(|| "null".into())
            });
        let leader_prefix = r
            .initial_leader_prefix
            .map(|coverage| {
                format!(
                    "{{ \"status\": \"sparse_exact_prefix\", \"rows\": {}, \"active_rows\": {}, \"inactive_rows\": {}, \"present_player_rows\": {}, \"duplicate_who_collapsed_rows\": {}, \"player_row_projection_bytes\": {}, \"player_row_projection_overcount_bytes\": {}, \"human_rows\": {}, \"nonhuman_rows\": {}, \"spans\": {}, \"fixed_traversal_bytes\": {}, \"exact_owned_checksum_bytes\": {}, \"unknown_fixed_traversal_bytes\": {}, \"dynamic_children_owned_bytes\": {}, \"human_only_first_checksum_candidate\": {}, \"walk_complete\": {}, \"exact_channel_producer\": {}, \"substantive_scoreboard_eligible\": {}, \"provenance_bytes\": {{ \"init_rules_and_teams_flags\": {}, \"leader_init_identity\": {}, \"leader_init_self_diplomacy\": {}, \"leader_init_diplomacy_reset\": {} }} }}",
                    coverage.rows,
                    coverage.active_rows,
                    coverage.inactive_rows,
                    r.initial_active_players,
                    r.initial_active_players.saturating_sub(coverage.active_rows),
                    8 + 704 * r.initial_active_players,
                    704 * r.initial_active_players.saturating_sub(coverage.active_rows),
                    coverage.human_rows,
                    coverage.nonhuman_rows,
                    coverage.spans,
                    coverage.fixed_traversal_bytes,
                    coverage.exact_owned_checksum_bytes,
                    coverage.unknown_fixed_traversal_bytes,
                    coverage.dynamic_children_owned_bytes,
                    coverage.human_only_first_checksum_candidate,
                    coverage.walk_complete,
                    coverage.exact_channel_producer,
                    coverage.substantive_scoreboard_eligible,
                    coverage.provenance.init_rules_and_teams_flags,
                    coverage.provenance.leader_init_identity,
                    coverage.provenance.leader_init_self_diplomacy,
                    coverage.provenance.leader_init_diplomacy_reset,
                )
            })
            .unwrap_or_else(|| {
                r.initial_leader_prefix_error
                    .as_ref()
                    .map(|error| format!("{{ \"status\": \"refused\", \"error\": \"{}\" }}", esc(error)))
                    .unwrap_or_else(|| "null".into())
            });
        s.push_str(&format!(
            "      \"initial\": {{ \"prefix_bytes_walked\": {}, \"seed\": \"0x{:08x}\", \"map_style\": {}, \"map_size\": {}, \"map_edge_world_cells\": {}, \"active_players\": {}, \"teams\": {:?}, \"items\": {{ \"status\": \"blocked\", \"boundary\": \"{}\", \"scalar_source_bytes\": {}, \"static_style\": {}, \"static_style_error\": {}, \"tile_selection\": {}, \"fertility\": {{ \"fill_fertile_cells\": {}, \"error\": {} }}, \"place_all\": {}, \"absent_replay_fields\": {{ \"selected_map_style\": 0, \"terrain_group_tables\": 0, \"generated_item_candidates\": 0, \"post_worldgen_rng\": 0 }} }}, \"rules\": {{ \"serialized_offset\": {}, \"serialized_bytes\": {}, \"checksum_walked_bytes\": {}, \"checksum\": {} }}, \"scenario\": {{ \"source\": \"ScenarioFuncSet::init 0x00a03c30 + internal_strings.xml ordinals 5958/5959\", \"checksum_walked_bytes\": {}, \"checksum\": {}, \"error\": {} }}, \"groups\": {{ \"source\": \"Groups::clear 0x00713f20 + Group::clear 0x00713e80\", \"slots\": {}, \"checksum_walked_bytes\": {}, \"checksum\": \"0x{:08x}\" }}, \"leaders_prefix\": {} }},\n",
            r.initial_prefix_bytes,
            r.initial_seed,
            r.initial_map_style,
            r.initial_map_size,
            r.initial_map_edge.map(|v| v.to_string()).unwrap_or_else(|| "null".into()),
            r.initial_active_players,
            r.initial_teams,
            r.initial_item_boundary.name(),
            r.initial_item_scalar_source_bytes,
            style_json,
            style_error,
            tile_selection,
            fill_fertile_cells,
            fertility_error,
            place_all,
            r.initial_rules_offset.map(|v| v.to_string()).unwrap_or_else(|| "null".into()),
            r.initial_rules_serialized_bytes,
            r.initial_rules_walked_bytes,
            r.initial_rules_checksum.map(|v| format!("\"0x{v:08x}\"")).unwrap_or_else(|| "null".into()),
            r.initial_scenario_walked_bytes,
            r.initial_scenario_checksum.map(|v| format!("\"0x{v:08x}\"")).unwrap_or_else(|| "null".into()),
            r.initial_scenario_error.as_deref().map(|e| format!("\"{}\"", esc(e))).unwrap_or_else(|| "null".into()),
            r.initial_groups_slots,
            r.initial_groups_walked_bytes,
            r.initial_groups_checksum,
            leader_prefix,
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
        let n = &r.next_checksum;
        if n.present() {
            let runs: Vec<String> = n
                .run_turns
                .iter()
                .map(|(ty, turns, truncated)| {
                    format!(
                        "{{ \"type\": {ty}, \"channel\": \"{}\", \"turns\": {turns}, \"truncated\": {truncated} }}",
                        crate::next_checksum::type_channel_name(*ty)
                    )
                })
                .collect();
            let dis: Vec<String> = n
                .crossplay_disagreements
                .iter()
                .map(|d| {
                    format!(
                        "{{ \"turn\": {}, \"type\": {}, \"channel\": \"{}\", \"play_a\": {}, \"value_a\": \"0x{:08x}\", \"play_b\": {}, \"value_b\": \"0x{:08x}\", \"turns_before_end\": {} }}",
                        d.turn,
                        d.ty,
                        crate::next_checksum::type_channel_name(d.ty),
                        d.a_play,
                        d.a_value,
                        d.b_play,
                        d.b_value,
                        d.turns_before_end
                    )
                })
                .collect();
            let per: Vec<String> = CHANNEL_NAMES
                .iter()
                .enumerate()
                .filter(|(i, _)| n.per_channel[*i].records > 0)
                .map(|(i, name)| {
                    let c = &n.per_channel[i];
                    format!(
                        "\"{name}\": {{ \"records\": {}, \"whole_channel_turns\": {}, \"compares\": {}, \"matches\": {}, \"nontrivial_compares\": {}, \"retail_empty_compares\": {}, \"no_producer\": {}, \"retail_disagreed\": {}, \"our_bytes_walked\": {}, \"per_element_records\": {} }}",
                        c.records,
                        c.whole_channel_turns,
                        c.compares,
                        c.matches,
                        c.nontrivial_compares,
                        c.retail_empty_compares,
                        c.no_producer,
                        c.retail_disagreed,
                        c.our_bytes_walked,
                        c.per_element_records
                    )
                })
                .collect();
            s.push_str(&format!(
                "      \"next_checksum\": {{ \"records\": {}, \"runs\": {}, \"sweep_monotone\": {}, \"turns_contiguous\": {}, \"crossplay\": {{ \"comparisons\": {}, \"identical\": {}, \"disagreements\": [{}] }}, \"sweep\": [{}], \"per_channel\": {{ {} }} }},\n",
                n.records,
                n.runs,
                n.sweep_monotone,
                n.contiguous,
                n.crossplay_comparisons,
                n.crossplay_identical,
                dis.join(", "),
                runs.join(", "),
                per.join(", ")
            ));
        }
        s.push_str("      \"channels\": {\n");
        for (i, name) in CHANNEL_NAMES.iter().enumerate() {
            let c = &r.channels[i];
            s.push_str(&format!(
                "        \"{name}\": {{ \"survived\": {}, \"first_divergence_turn\": {}, \"expected\": \"0x{:08x}\", \"got\": \"0x{:08x}\", \"compares\": {}, \"matches\": {}, \"trivial\": {}, \"unmodelled\": {}, \"nontrivial_compares\": {}, \"substantive_compares\": {}, \"substantive_matches\": {}, \"our_bytes_walked\": {}, \"our_unsourced_walked\": {}, \"our_installed\": {}, \"our_walk_complete\": {}, \"our_exact_producer\": {}, \"retail_empty_compares\": {}, \"retail_first_nonempty_turn\": {}, \"retail_disagreed\": {} }}{}\n",
                c.survived,
                c.first_divergence_turn.map(|t| t.to_string()).unwrap_or_else(|| "null".into()),
                c.expected,
                c.got,
                c.compares,
                c.matches,
                c.trivial_matches,
                c.unmodelled_matches,
                c.nontrivial_compares,
                c.substantive_compares,
                c.substantive_matches,
                c.our_bytes_walked,
                c.our_unsourced_walked,
                c.our_installed,
                c.our_walk_complete,
                c.our_exact_producer,
                c.retail_empty_compares,
                c.retail_first_nonempty_turn.map(|t| t.to_string()).unwrap_or_else(|| "null".into()),
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
