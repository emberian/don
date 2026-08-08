//! `don-replay` — the repeatable replay-validation command.
//!
//! ```text
//! don-replay validate [--corpus | FILE...] [--phase before|after] [--latency N]
//!                     [--json PATH] [--quiet]
//! don-replay scan     [--corpus | FILE...]      # decode only: framing, keys, checksum shape
//! don-replay crossplay [--corpus | FILE...]     # retail-vs-retail control experiment
//! don-replay walkers                            # coverage of the generated DataWalk table
//! ```

use don_replay::checksum::{CHANNEL_NAMES, NUM_CHANNELS, NUM_WALKED};
use don_replay::harness::{self, NullSim, Phase};
use don_replay::replay::{corpus, Replay};
use don_replay::report;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    // The binary lives at <root>/target/<profile>/don-replay, but it may also be
    // invoked from anywhere, so prefer the compile-time manifest path.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

struct Args {
    cmd: String,
    files: Vec<PathBuf>,
    phase: Phase,
    latency: u32,
    json: Option<PathBuf>,
    quiet: bool,
    limit: Option<usize>,
}

fn parse() -> Result<Args, String> {
    let mut a = Args {
        cmd: String::new(),
        files: Vec::new(),
        phase: Phase::BeforeCommands,
        latency: 0,
        json: None,
        quiet: false,
        limit: None,
    };
    let mut it = std::env::args().skip(1);
    let mut use_corpus = false;
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--corpus" => use_corpus = true,
            "--phase" => {
                a.phase = match it.next().as_deref() {
                    Some("before") | Some("before_commands") => Phase::BeforeCommands,
                    Some("after") | Some("after_step") => Phase::AfterStep,
                    other => return Err(format!("--phase before|after, got {other:?}")),
                }
            }
            "--latency" => {
                a.latency = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or("--latency wants an integer")?
            }
            "--limit" => {
                a.limit = Some(it.next().and_then(|v| v.parse().ok()).ok_or("--limit wants an integer")?)
            }
            "--json" => a.json = Some(PathBuf::from(it.next().ok_or("--json wants a path")?)),
            "--quiet" => a.quiet = true,
            s if s.starts_with("--") => return Err(format!("unknown flag {s}")),
            s if a.cmd.is_empty() => a.cmd = s.to_string(),
            s => a.files.push(PathBuf::from(s)),
        }
    }
    if a.cmd.is_empty() {
        a.cmd = "validate".into();
    }
    if use_corpus || a.files.is_empty() {
        a.files = corpus(&repo_root());
    }
    if let Some(n) = a.limit {
        a.files.truncate(n);
    }
    Ok(a)
}

fn main() {
    let args = match parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("don-replay: {e}");
            std::process::exit(2);
        }
    };
    if args.files.is_empty() {
        eprintln!(
            "\n  NO CORPUS — NOT A PASS. No .rcx under ron-data/replays/.\n  \
             ron-data/ is gitignored copyrighted game content; without it this\n  \
             command establishes nothing at all.\n"
        );
        std::process::exit(2);
    }
    match args.cmd.as_str() {
        "validate" => validate(&args),
        "scan" => scan(&args),
        "crossplay" => crossplay(&args),
        "walkers" => walkers(),
        other => {
            eprintln!("don-replay: unknown command {other}");
            std::process::exit(2);
        }
    }
}

fn open_all(files: &[PathBuf], quiet: bool) -> Vec<Replay> {
    let mut out = Vec::new();
    for f in files {
        match Replay::open(f) {
            Ok(r) => out.push(r),
            Err(e) => {
                if !quiet {
                    eprintln!("  skip {}: {e}", f.file_name().unwrap().to_string_lossy());
                }
            }
        }
    }
    out
}

fn validate(args: &Args) {
    let reps = open_all(&args.files, args.quiet);
    let mut runs = Vec::new();
    for r in &reps {
        let mut sim = NullSim::new();
        let run = harness::run(r, &mut sim, args.phase, args.latency);
        if !args.quiet && run.checksum_packets > 0 {
            println!("{}", harness::format_table(&run));
        }
        runs.push(run);
    }

    // ---- headline ----
    let with_cs: Vec<&harness::RunResult> = runs.iter().filter(|r| r.checksum_packets > 0).collect();
    println!("=== replay validation ===");
    println!(
        "files {} ({} carry checksums), turns {}, checksum packets {}",
        runs.len(),
        with_cs.len(),
        runs.iter().map(|r| r.turns_total).sum::<usize>(),
        runs.iter().map(|r| r.checksum_packets).sum::<usize>()
    );
    let t = report::Totals::of(&runs);
    println!(
        "checksum packets structurally sound: total==sum(15) {}/{}, adler-shaped {}/{}",
        t.checksum_total_ok, t.checksum_packets, t.checksum_shape_ok, t.checksum_packets
    );
    println!(
        "cross-player control: {}/{} identical tuples",
        t.crossplay_identical, t.crossplay_comparisons
    );
    println!("\nper-channel survival (consecutive agreeing turns; best over the corpus)");
    println!("  channel           best   compares    matches    trivial   xplay-diff");
    for i in 0..NUM_CHANNELS {
        println!(
            "  {:<16} {:>5}  {:>9}  {:>9}  {:>9}   {:>9}",
            CHANNEL_NAMES[i], t.best_survived[i], t.compares[i], t.matches[i], t.trivial[i],
            t.crossplay_per_channel[i]
        );
    }
    let (bi, bv) = (0..NUM_WALKED)
        .max_by_key(|&i| t.best_survived[i])
        .map(|i| (i, t.best_survived[i]))
        .unwrap();
    println!("\nHEADLINE: {} turns survived on channel `{}`", bv, CHANNEL_NAMES[bi]);

    if let Some(p) = &args.json {
        let js = report::to_json(&runs, "crates/don-replay/src/bin/don-replay.rs validate");
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        match std::fs::write(p, js) {
            Ok(()) => println!("wrote {}", p.display()),
            Err(e) => {
                eprintln!("could not write {}: {e}", p.display());
                std::process::exit(3);
            }
        }
    }
}

fn scan(args: &Args) {
    let reps = open_all(&args.files, args.quiet);
    println!(
        "{:<46} {:>8} {:>8} {:>7} {:>6} {:>9} {:>7}",
        "file", "pkgs", "decoded", "turns", "plys", "cs-pkts", "key"
    );
    let (mut pk, mut dec, mut cs, mut ok) = (0usize, 0usize, 0usize, 0usize);
    for r in &reps {
        let name: String = r
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .chars()
            .take(46)
            .collect();
        println!(
            "{:<46} {:>8} {:>8} {:>7} {:>6} {:>9} {:>7}",
            name,
            r.packages,
            r.packages_decoded,
            r.turns.len(),
            r.players.len(),
            r.checksum_packets,
            format!("{:04x}", r.xor_key)
        );
        pk += r.packages;
        dec += r.packages_decoded;
        cs += r.checksum_packets;
        ok += r.checksum_total_ok;
    }
    println!("\n{dec}/{pk} packages decoded, {cs} checksum packets, {ok} with total==sum(15)");
}

fn crossplay(args: &Args) {
    let reps = open_all(&args.files, args.quiet);
    let (mut c, mut i) = (0usize, 0usize);
    let (mut cs, mut is_) = (0usize, 0usize);
    let mut per = [0usize; NUM_CHANNELS];
    let mut per_s = [0usize; NUM_CHANNELS];
    let (mut diff_turn, mut mixed) = (0usize, 0usize);
    for r in &reps {
        let (rc, ri, rp) = r.crossplay();
        let (sc, si, sp, dt, mb) = r.crossplay_by_stamp_diag();
        diff_turn += dt;
        mixed += mb;
        if (rc > 0 && ri != rc) || (sc > 0 && si != sc) {
            println!(
                "{}: by-group {}/{} identical, by-stamp {}/{} identical \
                 ({} of the {} by-stamp disagreements compare different turns)",
                r.path.file_name().unwrap().to_string_lossy(),
                ri,
                rc,
                si,
                sc,
                dt,
                sc - si
            );
        }
        c += rc;
        i += ri;
        cs += sc;
        is_ += si;
        for k in 0..NUM_CHANNELS {
            per[k] += rp[k];
            per_s[k] += sp[k];
        }
    }
    println!("\naligned by CommandPackage::group (turn serial): {i}/{c} identical");
    println!("aligned by CommandPackage::stamp (sim frame):    {is_}/{cs} identical");
    println!(
        "  of the {} by-stamp disagreements, {diff_turn} compare packages from \
         DIFFERENT turns; {mixed} stamp buckets mix turns",
        cs - is_
    );
    println!("\ndisagreements by channel      by-group  by-stamp");
    for k in 0..NUM_CHANNELS {
        if per[k] > 0 || per_s[k] > 0 {
            println!("  {:<26} {:>8}  {:>8}", CHANNEL_NAMES[k], per[k], per_s[k]);
        }
    }
    let never: Vec<&str> = (0..NUM_CHANNELS)
        .filter(|&k| per[k] == 0 && per_s[k] == 0)
        .map(|k| CHANNEL_NAMES[k])
        .collect();
    println!("never disagreed under either alignment: {never:?}");
}

fn walkers() {
    let (bytes, tag, sub, unres, other) = don_replay::walk::table_coverage();
    println!("generated DataWalk table (schema/state-schema.json)");
    println!("  classes            {}", don_replay::SPECS.len());
    println!("  byte-range ops     {bytes}");
    println!("  tag ops            {tag}");
    println!("  sub-object ops     {sub}");
    println!("  unresolved ops     {unres}");
    println!("  virtual / unknown  {other}");
    println!("\nchannel element walkers:");
    for i in 0..NUM_WALKED {
        let cls = don_replay::state::CHANNEL_ELEMENT_CLASS[i];
        let idx = cls.and_then(don_replay::class_index);
        println!(
            "  {:<16} {:<28} {}",
            CHANNEL_NAMES[i],
            don_replay::state::CHANNEL_WALKER_SYMBOL[i],
            match idx {
                Some(k) => format!(
                    "{} (sizeof {}, walked {})",
                    don_replay::SPECS[k].name,
                    don_replay::SPECS[k].sizeof,
                    don_replay::SPECS[k].walked_bytes
                ),
                None => "NO DERIVED WALKER".to_string(),
            }
        );
    }
}
