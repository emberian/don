//! Gate 1: for EVERY `schema/live/frame-pairs/*/manifest.json`, load each
//! captured retail save, assert emit == input over the consumed prefix, and
//! report the consumed length. The live captures are proprietary and
//! gitignored; the test skips clearly when the directory is absent.
//! A loader stop or emit divergence on any frame is a gate failure.

use std::path::{Path, PathBuf};

use don_state::container::load_svx;

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

/// `save_name` for every step in a manifest (no serde).
fn manifest_saves(dir: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(dir.join("manifest.json")).expect("manifest.json");
    text.split("\"save_name\":")
        .skip(1)
        .filter_map(|s| s.split('"').nth(1).map(str::to_string))
        .collect()
}

#[test]
fn roundtrip_consumed_prefix() {
    let dirs = capture_dirs();
    if dirs.is_empty() {
        eprintln!("SKIP: proprietary live captures absent (schema/live/frame-pairs/*)");
        return;
    }
    let mut failures = Vec::new();
    let mut tested = 0usize;
    for dir in &dirs {
        let dname = dir.file_name().unwrap().to_string_lossy();
        for name in manifest_saves(dir) {
            let path = dir.join(format!("{name}.svx"));
            let bytes = match load_svx(&path) {
                Ok(b) => b,
                Err(e) => {
                    failures.push(format!("{dname}/{name}: container: {e}"));
                    continue;
                }
            };
            tested += 1;
            match don_state::load(&bytes) {
                Ok(mut img) => {
                    match don_state::save(&mut img.state) {
                        Ok(out) => {
                            if out != bytes[..img.consumed] {
                                failures.push(format!(
                                    "{dname}/{name}: emit != input over consumed prefix {:#x}",
                                    img.consumed
                                ));
                            }
                        }
                        Err(e) => failures.push(format!("{dname}/{name}: save: {e}")),
                    }
                    println!(
                        "{dname}/{name}: consumed {:#x} of {:#x} bytes ({:.1}%), {} spans",
                        img.consumed,
                        bytes.len(),
                        100.0 * img.consumed as f64 / bytes.len() as f64,
                        img.spans.len()
                    );
                }
                Err(e) => {
                    println!("{dname}/{name}: STOPPED — {e}");
                    failures.push(format!("{dname}/{name}: load stopped: {e}"));
                }
            }
        }
    }
    assert!(tested > 0, "no manifest steps gated");
    assert!(failures.is_empty(), "Gate 1 failures:\n{}", failures.join("\n"));
}
