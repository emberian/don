//! Diagnostic: dump the tail of the span trace when the loader stops, to
//! attribute the stopping offset to a field path. Skips when the proprietary
//! live captures are absent.
use std::path::Path;

#[test]
fn dump_tail() {
    let Ok(root) = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize() else {
        return;
    };
    let p = root.join("schema/live/frame-pairs/20261004-044959/donf2.svx");
    if !p.is_file() {
        eprintln!("SKIP: proprietary live captures absent");
        return;
    }
    let bytes = don_state::container::load_svx(&p).unwrap();
    let (l, e) = don_state::sections::load_save_dbg(&bytes);
    eprintln!("err: {e:?}\npos={:#x}", l.pos);
    let start = l.spans.len().saturating_sub(40);
    for s in &l.spans[start..] {
        eprintln!("  {:#x} +{:#x} {}", s.offset, s.len, s.path);
    }
}
