//! **The regression suite.** One command that runs every case in `oracle::registry`.
//!
//! ```text
//! regress [--json PATH] [--only ID[,ID...]] [--scale F] [--seed HEX] [--list]
//! ```
//!
//! Exit codes, and they are the point:
//!
//! | code | meaning |
//! |---|---|
//! | 0 | every registered case ran **and** agreed with its Rust model |
//! | 1 | at least one case mismatched, crashed, or produced unreadable output |
//! | 2 | at least one case was SKIPPED — it produced no evidence |
//! | 3 | the harness could not start (image missing/unmappable, selftest failed, bad `--only`) |
//!
//! There is deliberately no exit code that means "mostly fine". A suite that cannot run a
//! case reports zero mismatches for that case, which is indistinguishable from agreement
//! unless the runner refuses to call it green. Every Tier-B claim in
//! `docs/provenance-ledger.md` was established by one manual run that never repeated; this
//! command exists so those numbers are measurements with a date on them rather than
//! recollections.
//!
//! Must run as a 32-bit x86 process (`i686-unknown-linux-musl`) with
//! `data/riseofnations.exe` present in the working directory.

use oracle::image::Mapped;
use oracle::registry::{KNOWN_GAPS, REGISTRY};
use oracle::run;

const IMAGE_PATH: &str = "data/riseofnations.exe";

fn usage() -> ! {
    eprintln!(
        "usage: regress [--json PATH] [--only ID[,ID..]] [--scale F] [--seed HEX] [--list]

  --json PATH   write the machine-readable record (schema don/oracle-regression v1)
  --only IDS    comma-separated case ids; everything else is reported SKIPPED, and the
                run therefore exits 2 -- a filtered run is not a green run
  --scale F     multiply randomised and swept phase sizes by F. Exhaustive phases are
                never scaled: a shrunk exhaustive phase is a different claim
  --seed HEX    seed for every randomised phase (default 0x2545f4914f6cdd1d)
  --list        print the registry and the known gaps, run nothing

exit: 0 all ran and passed | 1 mismatch/crash | 2 something skipped | 3 harness failure"
    );
    std::process::exit(3)
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let mut json: Option<String> = None;
    let mut only: Option<String> = None;
    let mut scale = 1.0f64;
    let mut seed: u64 = 0x2545_F491_4F6C_DD1D;

    let mut i = 1;
    while i < argv.len() {
        match argv[i].as_str() {
            "--json" => {
                i += 1;
                match argv.get(i) {
                    Some(v) => json = Some(v.clone()),
                    None => usage(),
                }
            }
            "--only" => {
                i += 1;
                match argv.get(i) {
                    Some(v) => only = Some(v.clone()),
                    None => usage(),
                }
            }
            "--scale" => {
                i += 1;
                scale = match argv.get(i).and_then(|s| s.parse::<f64>().ok()) {
                    Some(v) if v > 0.0 => v,
                    _ => usage(),
                };
            }
            "--seed" => {
                i += 1;
                seed = match argv
                    .get(i)
                    .map(|s| s.trim_start_matches("0x"))
                    .and_then(|s| u64::from_str_radix(s, 16).ok())
                {
                    Some(v) => v,
                    None => usage(),
                };
            }
            "--list" => {
                println!("{} registered differential cases\n", REGISTRY.len());
                for c in REGISTRY.iter() {
                    println!("{:<26} {:#010x}  {}", c.id, c.va, c.abi);
                    println!("  model        {}", c.model);
                    println!("  ledger       {}", c.ledger);
                    println!("  derivation   {}", c.derivation);
                    println!("  reachability {}", c.reachability);
                    println!("  caveat       {}", c.caveat);
                    println!();
                }
                println!("{} Tier-B claims this suite cannot re-run:\n", KNOWN_GAPS.len());
                for g in KNOWN_GAPS.iter() {
                    println!("GAP  {}\n     {}\n", g.claim, g.why);
                }
                return;
            }
            "-h" | "--help" => usage(),
            _ => usage(),
        }
        i += 1;
    }

    if let Some(f) = &only {
        for id in f.split(',') {
            if !REGISTRY.iter().any(|c| c.id == id) {
                eprintln!("regress: --only names no registered case: {id}");
                eprintln!(
                    "known ids: {}",
                    REGISTRY.iter().map(|c| c.id).collect::<Vec<_>>().join(", ")
                );
                std::process::exit(3);
            }
        }
    }

    // A missing or unmappable image is a harness failure, not a run in which nothing
    // happened to mismatch. Exit 3 and say so; never write a record that could be read as
    // evidence.
    let bytes = match std::fs::read(IMAGE_PATH) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("regress: cannot read {IMAGE_PATH}: {e}");
            eprintln!(
                "regress: the retail image is copyrighted and is not committed; see \
                 docs/binary-ground-truth.md for extraction. NOTHING WAS TESTED."
            );
            std::process::exit(3);
        }
    };
    let (m, pe) = match Mapped::load(&bytes) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("regress: cannot map {IMAGE_PATH}: {e}. NOTHING WAS TESTED.");
            std::process::exit(3);
        }
    };

    let rep = run::run_all(&m, &pe, IMAGE_PATH, &bytes, scale, seed, only.as_deref());
    run::print_summary(&rep);

    if let Some(p) = &json {
        match run::write_json(&rep, p) {
            Ok(()) => println!("wrote {p}"),
            Err(e) => {
                eprintln!("regress: could not write {p}: {e}");
                std::process::exit(1);
            }
        }
    }
    std::process::exit(rep.exit_code())
}
