//! `do_frame` vs the live stride-1 captures: for every consecutive
//! (frame N, frame N+1) pair, the Ported systems must reproduce N+1
//! (`Game::frame`, `Game::tick`, `Groups::proc_group`) and must not
//! introduce a single byte retail did not change. `Game::graphic_tick`
//! is wall-clock derived (`FUN_00591570`) and excluded.
//!
//! Skips when the proprietary captures are absent.

use std::path::{Path, PathBuf};

use don_state::spandiff::{self, AlignEvent};
use don_state::tick;

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

fn frames(dir: &Path) -> Vec<(i64, String)> {
    let text = std::fs::read_to_string(dir.join("manifest.json")).expect("manifest.json");
    let mut out = Vec::new();
    for seg in text.split("\"frame\":").skip(1) {
        let frame = seg
            .trim_start()
            .split(|c: char| !c.is_ascii_digit())
            .next()
            .and_then(|t| t.parse::<i64>().ok());
        let save = seg
            .split("\"save_name\":")
            .nth(1)
            .and_then(|s| s.split('"').nth(1))
            .unwrap_or_default()
            .to_string();
        if let Some(f) = frame {
            out.push((f, save));
        }
    }
    out
}

fn scalars_i32(save: &don_state::Save, off: usize) -> i32 {
    i32::from_le_bytes(save.game.scalars[off..off + 4].try_into().unwrap())
}

#[test]
fn do_frame_ported_fields_match_next_frame() {
    let dirs = capture_dirs();
    if dirs.is_empty() {
        eprintln!("SKIP: proprietary live captures absent (schema/live/frame-pairs/*)");
        return;
    }
    let mut pairs_tested = 0;
    let mut field_failures: Vec<String> = Vec::new();
    let mut introduced: Vec<String> = Vec::new();
    for dir in &dirs {
        let dname = dir.file_name().unwrap().to_string_lossy().to_string();
        let steps = frames(dir);
        for k in 0..steps.len().saturating_sub(1) {
            let ((fa, na), (fb, nb)) = (steps[k].clone(), steps[k + 1].clone());
            if fb - fa != 1 {
                continue; // stride-15 dir: not consecutive
            }
            pairs_tested += 1;
            let tag = format!("{dname} f{fa}->f{fb}");
            let raw_a = don_state::container::load_svx(&dir.join(format!("{na}.svx"))).expect(&tag);
            let raw_b = don_state::container::load_svx(&dir.join(format!("{nb}.svx"))).expect(&tag);
            let img_a = don_state::load(&raw_a).expect(&tag);
            let img_b = don_state::load(&raw_b).expect(&tag);
            let mut ours = img_a.state.clone();
            let report = tick::do_frame(&mut ours);
            let ours_raw = don_state::save(&mut ours).expect(&tag);
            let ours_img = don_state::load(&ours_raw).expect(&tag);
            // Paths are not unique; map retail-N+1 span index -> ours.
            let mut ours_for_b: std::collections::BTreeMap<usize, usize> =
                std::collections::BTreeMap::new();
            for ev in spandiff::align(&ours_img.spans, &img_b.spans) {
                if let AlignEvent::Aligned(o, j) = ev {
                    ours_for_b.insert(j, o);
                }
            }

            // Ported fields must equal retail N+1 state.
            let want_frame = scalars_i32(&img_b.state, tick::FRAME);
            let want_tick = scalars_i32(&img_b.state, tick::TICK);
            if report.frame != want_frame || scalars_i32(&ours, tick::FRAME) != want_frame {
                field_failures.push(format!(
                    "{tag}: Game.frame ours={} want={want_frame}",
                    report.frame
                ));
            }
            if report.tick != want_tick {
                field_failures.push(format!("{tag}: Game.tick ours={} want={want_tick}", report.tick));
            }
            if report.proc_group != img_b.state.groups.proc_group {
                field_failures.push(format!(
                    "{tag}: Groups.proc_group ours={} want={}",
                    report.proc_group, img_b.state.groups.proc_group
                ));
            }

            // No introduced bytes anywhere in the emitted stream.
            for ev in spandiff::align(&img_a.spans, &img_b.spans) {
                if let AlignEvent::Aligned(i, j) = ev {
                    let (a, b) = (&img_a.spans[i], &img_b.spans[j]);
                    let ours_bytes = ours_for_b.get(&j).map(|&o| {
                        let os = &ours_img.spans[o];
                        &ours_raw[os.offset..os.offset + os.len]
                    });
                    let burn = spandiff::burn_span(
                        a,
                        b,
                        &raw_a,
                        &raw_b,
                        ours_bytes,
                        tick::is_nondeterministic(&a.path),
                    );
                    for (s, e) in &burn.introduced_ranges {
                        introduced.push(format!("{tag}: {} +{s:#x}..+{e:#x}", a.path));
                    }
                }
            }
        }
    }
    if pairs_tested == 0 {
        eprintln!("SKIP: no stride-1 capture pairs present");
        return;
    }
    assert!(field_failures.is_empty(), "ported-field failures:\n{}", field_failures.join("\n"));
    assert!(introduced.is_empty(), "introduced bytes (must be 0):\n{}", introduced.join("\n"));
}
