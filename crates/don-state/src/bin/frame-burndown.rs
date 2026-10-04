//! `frame-burndown <capture-dir>...` — the per-frame oracle for `do_frame`.
//!
//! For every consecutive stride-1 pair (N, N+1) in each capture manifest:
//! load retail N, run `don_state::tick::do_frame`, re-emit through `save()`,
//! and compare three ways against retail N+1 using the `spandiff` path
//! alignment (raw file offsets are never compared directly — variable-length
//! collections shift them):
//!
//!   retail_changed — bytes differing between retail N and retail N+1
//!   explained      — of those, bytes where ours == retail N+1
//!   unexplained    — retail-changed bytes where ours != retail N+1
//!   introduced     — bytes we changed that retail did NOT change
//!                    (must be 0 for a correctly ported system)
//!
//! Also measures the `game_random` LCG (`FUN_00a39d70`,
//! `s = s*1664525 + 1013904223`) draw count between the two saves, and
//! reports which `check_all` channels our post-tick state matches against
//! retail N+1's manifest words. Writes `<capture-dir>/frame-burndown.json`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use don_state::spandiff::{self, AlignEvent};
use don_state::tick;
use don_state::{container, load, save, CheckSums, CHANNEL_NAMES};

struct Step {
    frame: i64,
    save: String,
    words: Vec<u64>,
}

fn manifest_steps(dir: &Path) -> Vec<Step> {
    let text = std::fs::read_to_string(dir.join("manifest.json")).expect("manifest.json");
    let mut out = Vec::new();
    for seg in text.split("\"frame\":").skip(1) {
        let Some(frame) = seg
            .trim_start()
            .split(|c: char| !c.is_ascii_digit())
            .next()
            .and_then(|t| t.parse::<i64>().ok())
        else {
            continue;
        };
        let save = seg
            .split("\"save_name\":")
            .nth(1)
            .and_then(|s| s.split('"').nth(1))
            .unwrap_or_default()
            .to_string();
        let body = seg.split("\"check_words\":").nth(1).and_then(|s| s.split('}').next()).unwrap_or("");
        let words: Vec<u64> = CHANNEL_NAMES
            .iter()
            .map(|c| {
                body.split(&format!("\"{c}\":"))
                    .nth(1)
                    .and_then(|s| s.trim_start().split(|ch: char| !ch.is_ascii_digit()).next())
                    .and_then(|t| t.parse().ok())
                    .unwrap_or(0)
            })
            .collect();
        out.push(Step { frame, save, words });
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: frame-burndown <capture-dir>...");
        std::process::exit(2);
    }
    for arg in &args[1..] {
        run_dir(PathBuf::from(arg));
    }
}

fn run_dir(dir: PathBuf) {
    let steps = manifest_steps(&dir);
    if steps.len() < 2 {
        eprintln!("{}: <2 steps, skipping", dir.display());
        return;
    }
    println!("\n==== {}", dir.display());

    let mut agg: BTreeMap<String, usize> = BTreeMap::new(); // unexplained bytes by class.field
    let mut agg_spans: BTreeMap<String, usize> = BTreeMap::new();
    let mut introduced_failures: Vec<String> = Vec::new();
    let mut rng_series: Vec<(i64, Option<u32>)> = Vec::new();
    let mut pair_rows: Vec<PairRow> = Vec::new();
    let mut rng_offsets: Vec<usize> = Vec::new(); // span-relative seed offsets that resolve
    let mut rng_scanned = false;

    for k in 0..steps.len() - 1 {
        let (sa, sb) = (&steps[k], &steps[k + 1]);
        if sb.frame - sa.frame != 1 {
            continue; // stride != 1: not a consecutive pair
        }
        let raw_a = container::load_svx(&dir.join(format!("{}.svx", sa.save))).expect("svx");
        let img_a = load(&raw_a).expect("load A");
        let raw_b = container::load_svx(&dir.join(format!("{}.svx", sb.save))).expect("svx");
        let img_b = load(&raw_b).expect("load B");

        // Tick retail N forward and re-emit.
        let mut ours = img_a.state.clone();
        let report = tick::do_frame(&mut ours);
        let ours_raw = save(&mut ours).expect("save");
        let ours_img = load(&ours_raw).expect("reload ours");
        // Paths are not unique (e.g. GameInfo appears at top level and
        // inside `game`), so map retail-N+1 span index -> ours span index
        // via the same path alignment rather than a path-keyed map.
        let mut ours_for_b: BTreeMap<usize, usize> = BTreeMap::new();
        for ev in spandiff::align(&ours_img.spans, &img_b.spans) {
            if let AlignEvent::Aligned(o, j) = ev {
                ours_for_b.insert(j, o);
            }
        }

        let mut row = PairRow::default();
        let align_events = spandiff::align(&img_a.spans, &img_b.spans);
        for ev in &align_events {
            match ev {
                AlignEvent::Aligned(i, j) => {
                    let a = &img_a.spans[*i];
                    let b = &img_b.spans[*j];
                    let excl = tick::is_nondeterministic(&a.path);
                    let ours_bytes = ours_for_b.get(j).map(|&o| {
                        let os = &ours_img.spans[o];
                        &ours_raw[os.offset..os.offset + os.len]
                    });
                    let burn = spandiff::burn_span(a, b, &raw_a, &raw_b, ours_bytes, excl);
                    row.retail_changed += burn.retail_changed;
                    row.explained += burn.explained;
                    row.unexplained += burn.unexplained;
                    row.introduced += burn.introduced;
                    if burn.introduced > 0 {
                        for (s, e) in &burn.introduced_ranges {
                            introduced_failures.push(format!(
                                "f{}->f{} {} +{s:#x}..+{e:#x} ({}B)",
                                sa.frame,
                                sb.frame,
                                burn.path,
                                e - s
                            ));
                        }
                    }
                    for (s, e) in &burn.unexplained_ranges {
                        let key = match spandiff::span_class(&a.path) {
                            Some(l) => match spandiff::fields_covering(&a.path, l, a.len, *s, *e) {
                                Some(f) if !f.is_empty() => format!("{}.{}", l.name, f.join("+")),
                                _ => format!("{}.?", l.name),
                            },
                            None => a.path.clone(),
                        };
                        *agg.entry(key).or_default() += e - s;
                        *agg_spans.entry(burn.path.clone()).or_default() += e - s;
                    }
                }
                AlignEvent::Added(idxs) => {
                    // Retail gained spans we do not produce: all changed,
                    // all unexplained.
                    for &j in idxs {
                        let s = &img_b.spans[j];
                        row.retail_changed += s.len;
                        row.unexplained += s.len;
                        row.structural += 1;
                        *agg_spans.entry(s.path.clone()).or_default() += s.len;
                    }
                }
                AlignEvent::Removed(idxs) => {
                    row.structural += idxs.len();
                }
                AlignEvent::Unresolved(i, j) => {
                    row.structural += 2;
                    row.retail_changed += img_b.spans[*j].len;
                    row.unexplained += img_b.spans[*j].len;
                    let _ = i;
                }
                AlignEvent::Tail { new, .. } => {
                    row.structural += 1;
                    row.unexplained += new;
                }
            }
        }

        // RNG measurement: game_random is the 4-byte Random seed walked in
        // the post-World block (WalkDataGame walks *(void**)0x00c06184).
        // Scan span offsets once to find which 4 bytes chain by the LCG.
        let pw_a = img_a.spans.iter().find(|s| s.path.ends_with("post_world"));
        let pw_b = img_b.spans.iter().find(|s| s.path.ends_with("post_world"));
        let draws = if let (Some(pa), Some(pb)) = (pw_a, pw_b) {
            if !rng_scanned {
                rng_scanned = true;
                for off in 0..pa.len.saturating_sub(3) {
                    let s0 = u32::from_le_bytes(raw_a[pa.offset + off..pa.offset + off + 4].try_into().unwrap());
                    let s1 = u32::from_le_bytes(raw_b[pb.offset + off..pb.offset + off + 4].try_into().unwrap());
                    if matches!(tick::rng_draws(s0, s1), Some(d) if d > 0) {
                        rng_offsets.push(off);
                    }
                }
                if rng_offsets.is_empty() {
                    eprintln!("  [rng] no 4-byte window in post_world follows the LCG on this pair");
                } else {
                    eprintln!("  [rng] LCG-following post_world offsets: {rng_offsets:?}");
                }
            }
            rng_offsets
                .iter()
                .find_map(|&off| {
                    let s0 = u32::from_le_bytes(raw_a[pa.offset + off..pa.offset + off + 4].try_into().unwrap());
                    let s1 = u32::from_le_bytes(raw_b[pb.offset + off..pb.offset + off + 4].try_into().unwrap());
                    tick::rng_draws(s0, s1)
                })
        } else {
            None
        };
        rng_series.push((sa.frame, draws));

        // Channel check on our post-tick state vs retail N+1 words.
        let sums = CheckSums::check_all(&mut ours).expect("check_all");
        let matched: Vec<String> = (0..15)
            .filter(|&c| sums.word[c] as u64 == sb.words[c])
            .map(|c| CHANNEL_NAMES[c].to_string())
            .collect();
        row.matched_channels = matched.len();
        row.frame_from = sa.frame;
        row.frame_to = sb.frame;
        row.rng_draws = draws;
        row.ported_steps = report.steps.iter().filter(|s| s.status == tick::StepStatus::Ported).count();

        println!(
            "pair f{0}->f{1}: retail_changed={2} explained={3} unexplained={4} introduced={5} structural={6} rng_draws={7} channels={8}/15",
            sa.frame,
            sb.frame,
            row.retail_changed,
            row.explained,
            row.unexplained,
            row.introduced,
            row.structural,
            draws.map(|d| d.to_string()).unwrap_or_else(|| "?".into()),
            row.matched_channels,
        );

        pair_rows.push(row);
    }

    // Ranked unexplained table.
    println!("\n== unexplained bytes by class.field (all pairs) ==");
    let mut rows: Vec<(String, usize)> = agg.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1));
    for (k, (key, bytes)) in rows.iter().take(20).enumerate() {
        println!("  {k:>2}. {bytes:>9}  {key}");
    }
    if !introduced_failures.is_empty() {
        println!("\n== INTRODUCED (FAILURE — we changed bytes retail kept) ==");
        for f in introduced_failures.iter().take(40) {
            println!("  {f}");
        }
    }

    // Step table.
    println!("\n== STEPS ==");
    for s in &tick::STEPS {
        println!(
            "  {:>2} {}{:<42} {}",
            s.idx,
            s.va.map(|v| format!("{v:#010x} ")).unwrap_or_else(|| "          ".into()),
            s.name,
            format!("{:?}", tick::step_status(s.idx)),
        );
    }

    write_json(&dir, &pair_rows, &rng_series, &rows);
    println!("wrote {}", dir.join("frame-burndown.json").display());
}

#[derive(Default)]
struct PairRow {
    frame_from: i64,
    frame_to: i64,
    retail_changed: usize,
    explained: usize,
    unexplained: usize,
    introduced: usize,
    structural: usize,
    rng_draws: Option<u32>,
    matched_channels: usize,
    ported_steps: usize,
}

fn write_json(dir: &Path, pairs: &[PairRow], rng: &[(i64, Option<u32>)], top: &[(String, usize)]) {
    let mut j = String::new();
    let _ = writeln!(j, "{{");
    let _ = writeln!(j, "  \"steps\": [");
    for (i, s) in tick::STEPS.iter().enumerate() {
        let _ = writeln!(
            j,
            "    {{\"idx\":{},\"name\":\"{}\",\"va\":{},\"status\":\"{:?}\"}}{}",
            s.idx,
            s.name,
            s.va.map(|v| format!("\"{v:#010x}\"")).unwrap_or("null".into()),
            tick::step_status(s.idx),
            if i + 1 == tick::STEPS.len() { "" } else { "," }
        );
    }
    let _ = writeln!(j, "  ],");
    let _ = writeln!(j, "  \"nondeterministic_exclusions\": [");
    for (i, e) in tick::NONDETERMINISTIC_EXCLUSIONS.iter().enumerate() {
        let _ = writeln!(
            j,
            "    {{\"path\":\"{}\",\"writer_va\":\"{:#010x}\"}}{}",
            e.path,
            e.writer_va,
            if i + 1 == tick::NONDETERMINISTIC_EXCLUSIONS.len() { "" } else { "," }
        );
    }
    let _ = writeln!(j, "  ],");
    let _ = writeln!(j, "  \"rng_draws\": [");
    for (i, (f, d)) in rng.iter().enumerate() {
        let _ = writeln!(
            j,
            "    {{\"frame\":{f},\"draws\":{}}}{}",
            d.map(|x| x.to_string()).unwrap_or("null".into()),
            if i + 1 == rng.len() { "" } else { "," }
        );
    }
    let _ = writeln!(j, "  ],");
    let _ = writeln!(j, "  \"pairs\": [");
    for (i, p) in pairs.iter().enumerate() {
        let _ = writeln!(
            j,
            "    {{\"from\":{},\"to\":{},\"retail_changed\":{},\"explained\":{},\"unexplained\":{},\"introduced\":{},\"structural\":{},\"rng_draws\":{},\"channels_matched\":{}}}{}",
            p.frame_from,
            p.frame_to,
            p.retail_changed,
            p.explained,
            p.unexplained,
            p.introduced,
            p.structural,
            p.rng_draws.map(|x| x.to_string()).unwrap_or("null".into()),
            p.matched_channels,
            if i + 1 == pairs.len() { "" } else { "," }
        );
    }
    let _ = writeln!(j, "  ],");
    let _ = writeln!(j, "  \"unexplained_top\": [");
    for (i, (k, v)) in top.iter().take(40).enumerate() {
        let _ = writeln!(j, "    [\"{}\",{}]{}", k, v, if i + 1 == top.len().min(40) { "" } else { "," });
    }
    let _ = writeln!(j, "  ]");
    let _ = writeln!(j, "}}");
    std::fs::write(dir.join("frame-burndown.json"), j).expect("write json");
}
