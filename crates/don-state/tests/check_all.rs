//! Gate 2: the fifteen `CheckSums::check_*` channels over the typed `Save`
//! state vs the live manifest (`check_words` / `check_bytes`) for every
//! `schema/live/frame-pairs/*/manifest.json` step. Skips when the
//! proprietary captures are absent.
//!
//! Channels in `OPEN` are known-incomplete: they must still MISMATCH (the
//! assertion guards against accidental "fixes"); closing one means removing
//! it from the list so it becomes an exact-equality gate.

use std::path::{Path, PathBuf};

/// Channel indices still open (see crate README). Empty: all fifteen
/// channels are exact-equality gates (scenario_data and script_run_time
/// closed once the GraphicEvents slot plane, Camera, Terrain coord data,
/// Cliffs payloads, Doober and ScenarioData/ScriptFile grammars were typed).
const OPEN: &[usize] = &[];

/// Every `schema/live/frame-pairs/<ts>*/` containing a manifest.json.
fn capture_dirs() -> Vec<PathBuf> {
    let Ok(root) = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize() else {
        return Vec::new();
    };
    let pairs = root.join("schema/live/frame-pairs");
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(pairs) {
        for e in rd.flatten() {
            let d = e.path();
            if d.is_dir() && d.join("manifest.json").is_file() {
                out.push(d);
            }
        }
    }
    out.sort();
    out
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
    let dirs = capture_dirs();
    if dirs.is_empty() {
        eprintln!("SKIP: proprietary live captures absent (schema/live/frame-pairs/*)");
        return;
    }
    let mut failures = Vec::new();
    let mut open_flips = Vec::new();
    let mut tested = 0;
    for dir in &dirs {
        let dname = dir.file_name().unwrap().to_string_lossy().to_string();
        let frames = manifest_frames(dir, &don_state::CHANNEL_NAMES);
        assert!(!frames.is_empty(), "{dname}: manifest has no steps");
        for (frame, f) in &frames {
        let svx = format!("{}.svx", f.save);
        let name = format!("{dname}/{svx}");
        let raw = don_state::container::load_svx(&dir.join(&svx)).expect(&name);
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
    }
    assert!(tested > 0, "no manifest steps gated");
    assert!(
        failures.is_empty(),
        "closed-channel mismatches: {failures:?}"
    );
    assert!(
        open_flips.is_empty(),
        "OPEN channel now matches — remove it from OPEN: {open_flips:?}"
    );
}

/// `load(save(do_frame(load(x))))`: a save emitted from a *mutated* state
/// tree must load again, reproduce itself byte-for-byte on a second save,
/// and carry a `SaveGame::verify_save` trailer regenerated from the mutated
/// state (the loader recomputes and compares it, so a stale trailer fails
/// the load). Run on the first frame of every capture directory.
#[test]
fn mutated_state_save_roundtrips() {
    let dirs = capture_dirs();
    if dirs.is_empty() {
        eprintln!("SKIP: proprietary live captures absent (schema/live/frame-pairs/*)");
        return;
    }
    let mut tested = 0;
    for dir in &dirs {
        let dname = dir.file_name().unwrap().to_string_lossy().to_string();
        let frames = manifest_frames(dir, &don_state::CHANNEL_NAMES);
        let Some((frame, f)) = frames.first() else { continue };
        let name = format!("{dname}/{}.svx (frame {frame})", f.save);
        let raw = don_state::container::load_svx(&dir.join(format!("{}.svx", f.save))).expect(&name);
        let mut img = don_state::load(&raw).expect(&name);
        let trailer_before = img.state.rules_tail.verify_words.clone();
        let report = don_state::tick::do_frame(&mut img.state);
        let emitted = don_state::save(&mut img.state).unwrap_or_else(|e| panic!("{name}: save after do_frame: {e}"));
        assert_ne!(emitted, raw, "{name}: do_frame changed nothing (frame counter must advance)");
        let trailer_after = img.state.rules_tail.verify_words.clone();
        assert_ne!(trailer_before, trailer_after, "{name}: verify_save trailer did not follow the mutated state");
        let mut again = don_state::load(&emitted).unwrap_or_else(|e| panic!("{name}: reload of mutated save: {e}"));
        assert_eq!(again.consumed, emitted.len(), "{name}: reload left residue");
        let twice = don_state::save(&mut again.state).expect("second save");
        assert_eq!(twice, emitted, "{name}: mutated save is not a fixed point of load/save");
        let _ = report;
        tested += 1;
    }
    assert!(tested > 0);
}
