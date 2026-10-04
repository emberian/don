// quick debug binary
fn main() {
    let raw = don_state::container::load_svx(std::path::Path::new(&std::env::args().nth(1).unwrap())).unwrap();
    let img = don_state::load(&raw).unwrap();
    let mut first = usize::MAX; let mut last = 0; let mut total = 0usize;
    for s in &img.spans {
        if s.path.starts_with("ScenarioData") || s.path.starts_with("RunTimeEnv") || s.path.starts_with("WalkDataGame.final") || s.path.starts_with("rules") {
            if s.offset < first { first = s.offset; }
            if s.offset + s.len > last { last = s.offset + s.len; }
            total += s.len;
            println!("{:#x} +{:#x} {}", s.offset, s.len, s.path);
        }
    }
    println!("range {first:#x}..{last:#x} total {total:#x}");
}
