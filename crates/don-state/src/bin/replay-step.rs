//! `replay-step` — apply the command packages a `.rcx` recorded for one
//! frame onto that frame's `.svx`, tick once, and diff against the next
//! retail save.
//!
//!     replay-step <N.svx> <rcx> [N+1.svx] [--play P] [--frame F] [--json OUT]
//!     replay-step <N.svx> --cmd <hex> [N+1.svx] [--play P] [--json OUT]
//!     replay-step --list <rcx>
//!
//! The `.rcx` command stream is located structurally (the unique longest
//! chain of 18-byte `CommandPackage` records tiling to EOF — the method in
//! `docs/derivation/replay-stream.md` §2, same as `don-net::find_stream`;
//! copied here because `don-replay` would pull `don-sim` into this crate).
//! Solo recordings carry no XOR/pad obfuscation, which is all the captures
//! are; a multiplayer `.rcx` is refused rather than mis-decoded.
//!
//! Package selection: every record whose `stamp` equals the loaded save's
//! `Game::frame` (override with `--frame`). `CommandManager::process_turn`
//! applies the packages stamped for the current frame before
//! `Game::do_frame`, so the N+1 save contains both the command's writes and
//! the tick's.
//!
//! Output mirrors `frame-burndown`: per aligned span `retail_changed /
//! explained / unexplained / introduced`, with the Group / Unit spans the
//! package touched listed first. `introduced` outside the fields a command
//! owns is the failure signal.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use don_state::spandiff::{self, AlignEvent};
use don_state::systems::commands::{self, CommandContext};
use don_state::tick;
use don_state::{container, load, save};

// ---------------------------------------------------------------------------
// .rcx package stream
// ---------------------------------------------------------------------------

const PKG_HDR: usize = 18;

#[derive(Clone, Debug)]
struct PackageRec {
    stamp: u32,
    play: i32,
    valid: i32,
    group: i32,
    payload: Vec<u8>,
}

fn load_rcx(p: &Path) -> Result<Vec<u8>, String> {
    let raw = std::fs::read(p).map_err(|e| e.to_string())?;
    if raw.len() >= 2 && raw[0] == 0x1F && raw[1] == 0x8B {
        let out = std::process::Command::new("gzip").arg("-dc").arg(p).output().map_err(|e| e.to_string())?;
        if out.stdout.is_empty() {
            return Err("gzip produced nothing".into());
        }
        Ok(out.stdout)
    } else {
        Ok(raw)
    }
}

/// `don-net::find_stream`: backward DP over "record at o is good iff
/// o+18+size is good", `play < 8`, non-decreasing `stamp` (leap <= 200).
fn find_stream(buf: &[u8]) -> Option<usize> {
    let n = buf.len();
    if n < PKG_HDR {
        return None;
    }
    let u16at = |o: usize| u16::from_le_bytes([buf[o], buf[o + 1]]) as usize;
    let u32at = |o: usize| u32::from_le_bytes(buf[o..o + 4].try_into().unwrap());
    let mut count = vec![0u32; n + 1];
    let mut best = (0u32, usize::MAX);
    let mut o = n - PKG_HDR;
    loop {
        'body: {
            if u32at(o + 4) >= 8 {
                break 'body;
            }
            let ln = u16at(o + 16);
            if ln > 4000 {
                break 'body;
            }
            let next = o + PKG_HDR + ln;
            if next > n {
                break 'body;
            }
            let c = if next == n {
                1
            } else {
                if count[next] == 0 {
                    break 'body;
                }
                let (a, b) = (u32at(o), u32at(next));
                if b < a || b - a > 200 {
                    break 'body;
                }
                count[next] + 1
            };
            count[o] = c;
            if c > best.0 {
                best = (c, o);
            }
        }
        if o == 0 {
            break;
        }
        o -= 1;
    }
    (best.1 != usize::MAX).then_some(best.1)
}

fn packages(buf: &[u8]) -> Result<Vec<PackageRec>, String> {
    let start = find_stream(buf).ok_or("no command-package chain in .rcx")?;
    let mut out = Vec::new();
    let mut o = start;
    while o + PKG_HDR <= buf.len() {
        let g = |k: usize| u32::from_le_bytes(buf[o + k..o + k + 4].try_into().unwrap());
        let size = u16::from_le_bytes([buf[o + 16], buf[o + 17]]) as usize;
        if o + PKG_HDR + size > buf.len() {
            return Err(format!("framing residue at +{o:#x}"));
        }
        out.push(PackageRec {
            stamp: g(0),
            play: g(4) as i32,
            valid: g(8) as i32,
            group: g(12) as i32,
            payload: buf[o + PKG_HDR..o + PKG_HDR + size].to_vec(),
        });
        o += PKG_HDR + size;
    }
    if o != buf.len() {
        return Err(format!("framing residue: {} bytes", buf.len() - o));
    }
    Ok(out)
}

fn parse_hex(s: &str) -> Result<Vec<u8>, String> {
    let clean: String = s.chars().filter(|c| !c.is_whitespace() && *c != ':').collect();
    if clean.len() % 2 != 0 {
        return Err("odd hex length".into());
    }
    (0..clean.len() / 2)
        .map(|i| u8::from_str_radix(&clean[i * 2..i * 2 + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

struct Args {
    save_n: Option<PathBuf>,
    rcx: Option<PathBuf>,
    save_n1: Option<PathBuf>,
    cmd: Option<Vec<u8>>,
    play: Option<usize>,
    frame: Option<u32>,
    json: Option<PathBuf>,
    list: Option<PathBuf>,
    no_tick: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        save_n: None,
        rcx: None,
        save_n1: None,
        cmd: None,
        play: None,
        frame: None,
        json: None,
        list: None,
        no_tick: false,
    };
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    let mut positional: Vec<PathBuf> = Vec::new();
    while i < argv.len() {
        match argv[i].as_str() {
            "--play" => {
                a.play = Some(argv.get(i + 1).ok_or("--play needs a value")?.parse().map_err(|_| "bad --play")?);
                i += 1;
            }
            "--frame" => {
                a.frame = Some(argv.get(i + 1).ok_or("--frame needs a value")?.parse().map_err(|_| "bad --frame")?);
                i += 1;
            }
            "--json" => {
                a.json = Some(PathBuf::from(argv.get(i + 1).ok_or("--json needs a path")?));
                i += 1;
            }
            "--cmd" => {
                a.cmd = Some(parse_hex(argv.get(i + 1).ok_or("--cmd needs hex")?)?);
                i += 1;
            }
            "--list" => {
                a.list = Some(PathBuf::from(argv.get(i + 1).ok_or("--list needs an .rcx")?));
                i += 1;
            }
            "--no-tick" => a.no_tick = true,
            other => positional.push(PathBuf::from(other)),
        }
        i += 1;
    }
    if a.list.is_some() {
        return Ok(a);
    }
    let mut it = positional.into_iter();
    a.save_n = it.next();
    if a.cmd.is_none() {
        a.rcx = it.next();
    }
    a.save_n1 = it.next();
    if a.save_n.is_none() || (a.rcx.is_none() && a.cmd.is_none()) {
        return Err("usage: replay-step <N.svx> (<rcx> | --cmd <hex>) [N+1.svx] [--play P] [--frame F] [--no-tick] [--json OUT]\n       replay-step --list <rcx>".into());
    }
    Ok(a)
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    if let Some(rcx) = &args.list {
        let buf = load_rcx(rcx).unwrap_or_else(|e| fatal(&e));
        let pk = packages(&buf).unwrap_or_else(|e| fatal(&e));
        println!("{} packages", pk.len());
        for p in &pk {
            if p.payload.is_empty() {
                continue;
            }
            let ops: Vec<String> = match commands::split_commands(&p.payload) {
                Ok(cs) => cs
                    .iter()
                    .map(|c| format!("{}({})", commands::COMMAND_NAMES.get(c[0] as usize).copied().unwrap_or("?"), c.len()))
                    .collect(),
                Err(e) => vec![format!("<{e}>")],
            };
            println!(
                "stamp={:<7} play={} valid={} group={:<6} size={:<4} {}",
                p.stamp, p.play, p.valid, p.group, p.payload.len(), ops.join(" ")
            );
        }
        return;
    }

    let save_n = args.save_n.clone().unwrap();
    let raw_a = container::load_svx(&save_n).unwrap_or_else(|e| fatal(&e.to_string()));
    let img_a = load(&raw_a).unwrap_or_else(|e| fatal(&format!("load {}: {e}", save_n.display())));
    let mut ours = img_a.state.clone();
    let frame_n = i32::from_le_bytes(ours.game.scalars[0..4].try_into().unwrap());
    println!("loaded {} (Game::frame = {frame_n})", save_n.display());

    // Packages to apply.
    let mut todo: Vec<PackageRec> = Vec::new();
    if let Some(cmd) = &args.cmd {
        todo.push(PackageRec { stamp: frame_n as u32, play: args.play.unwrap_or(0) as i32, valid: 0, group: -1, payload: cmd.clone() });
    } else {
        let rcx = args.rcx.clone().unwrap();
        let buf = load_rcx(&rcx).unwrap_or_else(|e| fatal(&e));
        let pk = packages(&buf).unwrap_or_else(|e| fatal(&e));
        let want = args.frame.unwrap_or(frame_n as u32);
        for p in pk {
            if p.stamp == want && !p.payload.is_empty() && args.play.map(|pl| pl as i32 == p.play).unwrap_or(true) {
                todo.push(p);
            }
        }
        println!("{} package(s) in {} stamped {want}", todo.len(), rcx.display());
        if todo.iter().any(|p| commands::split_commands(&p.payload).is_err()) {
            eprintln!("a package does not tile as solo commands: multiplayer (XOR/pad) recording? refusing");
            std::process::exit(3);
        }
    }

    // Apply.
    let mut ctx = CommandContext::default();
    let mut applied: Vec<String> = Vec::new();
    let mut touched_groups: Vec<i32> = Vec::new();
    for p in &todo {
        let play = usize::try_from(p.play).unwrap_or(0);
        match commands::apply_package_with(&mut ours, &mut ctx, play, &p.payload) {
            Ok(rep) => {
                for c in &rep.commands {
                    println!("  [play {play}] {:<14} {:>3}B {:?}", c.name, c.len, c.status);
                    for e in &c.effects {
                        println!("        {e}");
                    }
                    applied.push(format!("{}:{:?}", c.name, c.status));
                }
                if rep.group >= 0 {
                    touched_groups.push(rep.group);
                }
            }
            Err(e) => {
                eprintln!("apply_package failed: {e}");
                std::process::exit(4);
            }
        }
    }

    // Tick.
    if !args.no_tick {
        let rep = tick::do_frame(&mut ours);
        println!("do_frame -> frame {} tick {} proc_group {}", rep.frame, rep.tick, rep.proc_group);
    }

    let Some(save_n1) = args.save_n1.clone() else {
        println!("no N+1 save given; done.");
        return;
    };
    let raw_b = container::load_svx(&save_n1).unwrap_or_else(|e| fatal(&e.to_string()));
    let img_b = load(&raw_b).unwrap_or_else(|e| fatal(&format!("load {}: {e}", save_n1.display())));
    let ours_raw = save(&mut ours).unwrap_or_else(|e| fatal(&format!("save: {e}")));
    let ours_img = load(&ours_raw).unwrap_or_else(|e| fatal(&format!("reload ours: {e}")));

    let mut ours_for_b: BTreeMap<usize, usize> = BTreeMap::new();
    for ev in spandiff::align(&ours_img.spans, &img_b.spans) {
        if let AlignEvent::Aligned(o, j) = ev {
            ours_for_b.insert(j, o);
        }
    }

    let mut tot = spandiff::PairBurn::default();
    let mut rows: Vec<(String, usize, spandiff::PairBurn)> = Vec::new();
    let mut structural = 0usize;
    for ev in spandiff::align(&img_a.spans, &img_b.spans) {
        match ev {
            AlignEvent::Aligned(i, j) => {
                let a = &img_a.spans[i];
                let b = &img_b.spans[j];
                let excl = tick::is_nondeterministic(&a.path);
                let ob = ours_for_b.get(&j).map(|&o| {
                    let s = &ours_img.spans[o];
                    &ours_raw[s.offset..s.offset + s.len]
                });
                let burn = spandiff::burn_span(a, b, &raw_a, &raw_b, ob, excl);
                tot.retail_changed += burn.retail_changed;
                tot.explained += burn.explained;
                tot.unexplained += burn.unexplained;
                tot.introduced += burn.introduced;
                if burn.retail_changed > 0 || burn.introduced > 0 {
                    rows.push((a.path.clone(), a.len, burn));
                }
            }
            AlignEvent::Added(idxs) => {
                for j in idxs {
                    tot.retail_changed += img_b.spans[j].len;
                    tot.unexplained += img_b.spans[j].len;
                    structural += 1;
                }
            }
            AlignEvent::Removed(idxs) => structural += idxs.len(),
            AlignEvent::Unresolved(_, j) => {
                structural += 2;
                tot.retail_changed += img_b.spans[j].len;
                tot.unexplained += img_b.spans[j].len;
            }
            AlignEvent::Tail { new, .. } => {
                structural += 1;
                tot.unexplained += new;
            }
        }
    }

    // Command-owned spans first: Groups.list[g] for touched groups, then
    // Objects (Unit) spans, then everything else.
    let owned = |p: &str| -> u8 {
        if touched_groups.iter().any(|g| p.starts_with(&format!("Groups.list[{g}]"))) {
            0
        } else if p.starts_with("Groups.") {
            1
        } else if p.starts_with("Objects.lists") {
            2
        } else {
            3
        }
    };
    rows.sort_by_key(|(p, _, _)| owned(p));

    println!(
        "\npair {} -> {}: retail_changed={} explained={} unexplained={} introduced={} structural={structural}",
        save_n.file_name().unwrap().to_string_lossy(),
        save_n1.file_name().unwrap().to_string_lossy(),
        tot.retail_changed,
        tot.explained,
        tot.unexplained,
        tot.introduced
    );
    println!("\n== spans (command-owned first) ==");
    for (p, span_len, b) in rows.iter().take(60) {
        let fields = |ranges: &[(usize, usize)]| -> String {
            ranges
                .iter()
                .map(|(s, e)| match spandiff::span_class(p) {
                    Some(l) => match spandiff::fields_covering(p, l, *span_len, *s, *e) {
                        Some(f) if !f.is_empty() => format!("+{s:#x}..+{e:#x}({})", f.join("+")),
                        _ => format!("+{s:#x}..+{e:#x}"),
                    },
                    None => format!("+{s:#x}..+{e:#x}"),
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        println!(
            "  {:<48} changed={:<4} explained={:<4} unexplained={:<4} introduced={:<4} unx[{}] intro[{}]",
            p,
            b.retail_changed,
            b.explained,
            b.unexplained,
            b.introduced,
            fields(&b.unexplained_ranges),
            fields(&b.introduced_ranges)
        );
    }
    if rows.len() > 60 {
        println!("  ... {} more spans", rows.len() - 60);
    }

    // Group-owned verdict: for every touched Groups.list[g] span, bytes must
    // equal retail N+1 exactly.
    let mut group_exact = true;
    for g in &touched_groups {
        let prefix = format!("Groups.list[{g}]");
        for (p, _, b) in &rows {
            if p.starts_with(&prefix) && (b.unexplained > 0 || b.introduced > 0) {
                group_exact = false;
            }
        }
    }
    if !touched_groups.is_empty() {
        println!(
            "\nGroups.list{:?} vs retail N+1: {}",
            touched_groups,
            if group_exact { "EXACT" } else { "DIFFERS (see spans above)" }
        );
    }

    if let Some(out) = &args.json {
        let mut s = String::new();
        s.push_str("{\n");
        s.push_str(&format!("  \"save_n\": \"{}\",\n  \"save_n1\": \"{}\",\n", save_n.display(), save_n1.display()));
        s.push_str(&format!("  \"frame\": {frame_n},\n  \"packages\": {},\n", todo.len()));
        s.push_str(&format!("  \"applied\": [{}],\n", applied.iter().map(|a| format!("\"{a}\"")).collect::<Vec<_>>().join(", ")));
        s.push_str(&format!("  \"touched_groups\": {touched_groups:?},\n  \"group_exact\": {group_exact},\n"));
        s.push_str(&format!(
            "  \"retail_changed\": {},\n  \"explained\": {},\n  \"unexplained\": {},\n  \"introduced\": {},\n  \"structural\": {structural},\n",
            tot.retail_changed, tot.explained, tot.unexplained, tot.introduced
        ));
        s.push_str("  \"spans\": [\n");
        for (k, (p, _, b)) in rows.iter().enumerate() {
            s.push_str(&format!(
                "    {{\"path\": \"{p}\", \"retail_changed\": {}, \"explained\": {}, \"unexplained\": {}, \"introduced\": {}}}{}\n",
                b.retail_changed,
                b.explained,
                b.unexplained,
                b.introduced,
                if k + 1 < rows.len() { "," } else { "" }
            ));
        }
        s.push_str("  ]\n}\n");
        std::fs::write(out, s).unwrap_or_else(|e| fatal(&format!("write {}: {e}", out.display())));
        println!("wrote {}", out.display());
    }
}

fn fatal(msg: &str) -> ! {
    eprintln!("replay-step: {msg}");
    std::process::exit(1)
}
