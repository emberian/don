//! Gate 1: load each captured retail save, assert emit == input over the
//! consumed prefix, and report the consumed length. The live captures are
//! proprietary and gitignored; the test skips clearly when absent.

use std::path::{Path, PathBuf};

use don_state::container::load_svx;

fn live_dir() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().ok()?;
    let dir = root.join("schema/live/frame-pairs/20261004-044959");
    dir.is_dir().then_some(dir)
}

#[test]
fn roundtrip_consumed_prefix() {
    let Some(dir) = live_dir() else {
        eprintln!("SKIP: proprietary live captures absent (schema/live/frame-pairs/20261004-044959)");
        return;
    };
    for name in ["donf2", "donf3", "donf4"] {
        let path = dir.join(format!("{name}.svx"));
        let bytes = load_svx(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        match don_state::load(&bytes) {
            Ok(mut img) => {
                let out = don_state::save(&mut img.state)
                    .unwrap_or_else(|e| panic!("{name}: save failed: {e}"));
                assert_eq!(out, bytes[..img.consumed], "{name}: emit != input over consumed prefix");
                println!(
                    "{name}: consumed {:#x} of {:#x} bytes ({:.1}%), {} spans",
                    img.consumed,
                    bytes.len(),
                    100.0 * img.consumed as f64 / bytes.len() as f64,
                    img.spans.len()
                );
            }
            Err(e) => {
                println!("{name}: STOPPED — {e}");
            }
        }
    }
}
