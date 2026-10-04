//! Gate 2: the fifteen `CheckSums::check_*` channels over the typed `Save`
//! state vs the live manifest (`check_words` / `check_bytes`) for
//! donf2/donf3/donf4. Skips when the proprietary captures are absent.
//!
//! Channels in `OPEN` are known-incomplete: they must still MISMATCH (the
//! assertion guards against accidental "fixes"); closing one means removing
//! it from the list so it becomes an exact-equality gate.

use std::path::{Path, PathBuf};

/// Channel indices still open (see crate README):
///   12 rules           — tail boundary unresolved (runtime/script region)
///   13 scenario_data   — retail hashes Game::init scenario state, not in .svx
///   14 script_run_time — RunTimeEnv records (initialized script state)
const OPEN: &[usize] = &[12, 13, 14];

fn live_dir() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().ok()?;
    let d = root.join("schema/live/frame-pairs/20261004-044959");
    d.is_dir().then_some(d)
}

struct Frame {
    save: String,
    words: Vec<u64>,
    bytes: Vec<u64>,
}

/// Minimal manifest extraction (no serde): per `steps[]` entry, pull
/// `save_name`, `frame`, and the `check_words`/`check_bytes` objects keyed by
/// channel name.
fn manifest_frames(dir: &Path, channels: &[&str]) -> Vec<(i64, Frame)> {
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
        let chan = |obj: &str| -> Vec<u64> {
            let body = seg
                .split(&format!("\"{obj}\":"))
                .nth(1)
                .and_then(|s| s.split('}').next())
                .unwrap_or_default();
            channels
                .iter()
                .map(|c| {
                    body.split(&format!("\"{c}\":"))
                        .nth(1)
                        .and_then(|s| {
                            s.trim_start()
                                .split(|ch: char| !ch.is_ascii_digit())
                                .next()
                                .and_then(|t| t.parse().ok())
                        })
                        .unwrap_or(0)
                })
                .collect()
        };
        out.push((
            frame,
            Frame {
                save,
                words: chan("check_words"),
                bytes: chan("check_bytes"),
            },
        ));
    }
    out
}

#[test]
fn check_all_matches_manifest() {
    let Some(dir) = live_dir() else {
        eprintln!("SKIP: proprietary live captures absent");
        return;
    };
    let frames = manifest_frames(&dir, &don_state::CHANNEL_NAMES);
    assert!(!frames.is_empty(), "manifest has no steps");
    let mut failures = Vec::new();
    let mut open_flips = Vec::new();
    let mut tested = 0;
    for (frame, f) in &frames {
        if !(2..=4).contains(frame) {
            continue; // f0/f1 were probes; the gate covers f2..f4
        }
        let name = format!("{}.svx", f.save);
        let raw = don_state::container::load_svx(&dir.join(&name)).expect(&name);
        let (mut save, _) = don_state::sections::load_save(&raw).expect(&name);
        let sums = don_state::CheckSums::check_all(&mut save).expect(&name);
        tested += 1;
        eprintln!("== {name} (frame {frame})");
        eprintln!(
            "{:>16} {:>12} {:>12} {:>12} {:>12} {}",
            "channel", "ours_word", "retail_word", "ours_bytes", "retail_bytes", "ok"
        );
        for c in 0..15 {
            let ok = sums.word[c] as u64 == f.words[c] && sums.bytes[c] == f.bytes[c];
            let open = OPEN.contains(&c);
            eprintln!(
                "{:>16} {:>12} {:>12} {:>12} {:>12} {}",
                don_state::CHANNEL_NAMES[c],
                sums.word[c],
                f.words[c],
                sums.bytes[c],
                f.bytes[c],
                if ok {
                    "MATCH"
                } else if open {
                    "OPEN"
                } else {
                    "MISMATCH"
                },
            );
            if open && ok {
                open_flips.push((name.clone(), c));
            } else if !open && !ok {
                failures.push((name.clone(), c));
            }
        }
    }
    assert_eq!(tested, 3, "expected donf2/3/4 in manifest");
    assert!(
        failures.is_empty(),
        "closed-channel mismatches: {failures:?}"
    );
    assert!(
        open_flips.is_empty(),
        "OPEN channel now matches — remove it from OPEN: {open_flips:?}"
    );
}
