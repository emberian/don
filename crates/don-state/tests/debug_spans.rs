//! Diagnostic: dump spans around the post-Scene tail for boundary checking.
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
    for s in &l.spans {
        if s.offset >= 0x1677c9 && (s.len > 20 || s.path.contains("tag") || s.path.contains("count") || s.path.contains("len")) {
            eprintln!("  {:#x} +{:#x} {}", s.offset, s.len, s.path);
        }
    }
}
