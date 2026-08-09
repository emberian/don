//! `don-deviations` — print the deviation registry, and gate fidelity measurements.
//!
//! ```text
//! don-deviations                     # the whole registry, grouped
//! don-deviations --mode improved     # what our edition turns on, and what it does not
//! don-deviations --show <slug>       # one entry in full, with its derivation
//! don-deviations --markdown          # the table in docs/tracks/deviations.md
//! don-deviations --assert-fidelity   # exit 0 only if nothing is active (exit 3 if it is)
//! don-deviations --assert-ready playable|rl-env|product|replay
//! ```
//!
//! The mode comes from `DON_MODE` / `DON_DEVIATIONS` unless `--mode` overrides it, so
//! `--assert-fidelity` is a real statement about the environment a harness is running in
//! and not about a compiled-in constant. `tools/replay-validate.sh` runs it before it is
//! allowed to write a scoreboard.

use don_sim::deviations::{Deviation, Entry, Kind, Mode, ModeConfig, Surface};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut mode_override: Option<Mode> = None;
    let mut action = Action::List;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--assert-fidelity" => action = Action::AssertFidelity,
            "--assert-ready" => {
                i += 1;
                match args.get(i).map(|s| Surface::from_slug(s)) {
                    Some(Ok(surface)) => action = Action::AssertReady(surface),
                    Some(Err(e)) => return fail(&e.to_string()),
                    None => return fail("--assert-ready needs a surface"),
                }
            }
            "--markdown" | "--md" => action = Action::Markdown,
            "--list" => action = Action::List,
            "--show" => {
                i += 1;
                match args.get(i) {
                    Some(s) => action = Action::Show(s.clone()),
                    None => return fail("--show needs a slug"),
                }
            }
            "--mode" => {
                i += 1;
                match args.get(i).map(|s| Mode::from_slug(s)) {
                    Some(Ok(m)) => mode_override = Some(m),
                    Some(Err(e)) => return fail(&e.to_string()),
                    None => return fail("--mode needs `fidelity` or `improved`"),
                }
            }
            "-h" | "--help" => {
                print_help();
                return ExitCode::SUCCESS;
            }
            other => return fail(&format!("unknown argument `{other}` (try --help)")),
        }
        i += 1;
    }

    let cfg = match (mode_override, ModeConfig::from_env()) {
        (Some(Mode::Fidelity), _) => ModeConfig::fidelity(),
        (Some(Mode::Improved), _) => ModeConfig::improved(),
        (None, Ok(c)) => c,
        (None, Err(e)) => return fail(&e.to_string()),
    };

    match action {
        Action::AssertFidelity => match cfg.assert_fidelity() {
            Ok(()) => {
                println!("fidelity mode, 0 deviations active — measurements may be reported");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("\n  REFUSING TO MEASURE: {e}\n");
                eprintln!("{}", cfg.describe());
                eprintln!(
                    "\n  A fidelity number produced with a deviation active is not a\n  \
                     fidelity number. Unset DON_MODE / DON_DEVIATIONS and re-run.\n"
                );
                ExitCode::from(3)
            }
        },
        Action::AssertReady(surface) => match cfg.assert_ready(surface) {
            Ok(()) => {
                println!(
                    "{surface} ready under {} mode — no reachable known gaps",
                    cfg.mode()
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("\n  REFUSING {surface} CLAIM: {e}\n");
                eprintln!("{}", cfg.describe());
                let blockers: Vec<_> = cfg.readiness_blockers(surface).collect();
                if !blockers.is_empty() {
                    eprintln!("\n  reachable blockers:");
                    for blocker in blockers {
                        eprintln!("    - {blocker}");
                    }
                }
                eprintln!(
                    "\n  Research-only gaps do not block product surfaces. A blocker is listed\n  \
                     only when this entrypoint can execute it.\n"
                );
                ExitCode::from(3)
            }
        },
        Action::List => {
            list(&cfg);
            ExitCode::SUCCESS
        }
        Action::Markdown => {
            markdown();
            ExitCode::SUCCESS
        }
        Action::Show(slug) => match Deviation::from_slug(&slug) {
            Ok(d) => {
                show(&cfg, d);
                ExitCode::SUCCESS
            }
            Err(e) => fail(&e.to_string()),
        },
    }
}

enum Action {
    List,
    Markdown,
    Show(String),
    AssertFidelity,
    AssertReady(Surface),
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("don-deviations: {msg}");
    ExitCode::from(2)
}

fn print_help() {
    println!(
        "don-deviations — the Descent of Nations deviation registry\n\n\
         USAGE\n  \
           don-deviations [--mode fidelity|improved] [--list|--show <slug>|--markdown]\n  \
           don-deviations --assert-fidelity\n  \
           don-deviations [--mode fidelity|improved] --assert-ready \
             replay|playable|rl-env|product\n\n\
         ENVIRONMENT\n  \
           DON_MODE=fidelity|improved      (default: fidelity)\n  \
           DON_DEVIATIONS=+slug,-slug,...  applied on top of the mode's defaults\n"
    );
}

fn list(cfg: &ModeConfig) {
    println!("Descent of Nations — deviation registry");
    println!("mode: {}   active: {}\n", cfg.mode(), cfg.active_count());

    for kind in [Kind::Fix, Kind::Drift, Kind::Rejected] {
        let heading = match kind {
            Kind::Fix => "FIXES — deliberate, individually toggleable, improved mode only",
            Kind::Drift => "DRIFT — divergence nobody chose. Scoped readiness blocker",
            Kind::Rejected => "REJECTED — investigated, not a deviation. Do not resurrect",
        };
        let members: Vec<Deviation> = Deviation::ALL
            .into_iter()
            .filter(|d| d.kind() == kind)
            .collect();
        println!("{heading}");
        for d in members {
            let e = d.entry();
            let mark = match (
                kind,
                cfg.is_fidelity(),
                cfg.is_active(d),
                e.default_in_improved,
            ) {
                (Kind::Fix, true, _, _) => "[   ]",
                (Kind::Fix, false, true, _) => "[on ]",
                (Kind::Fix, false, false, true) => "[off]",
                (Kind::Fix, false, false, false) => "[opt]",
                _ => "[ - ]",
            };
            println!("  {mark} {:<28} {}", e.slug, e.title);
        }
        println!();
    }
    if cfg.is_fidelity() {
        println!(
            "  fidelity mode: every fix is off by construction, not by configuration.\n  \
             `--mode improved` to see what our edition changes.\n"
        );
    } else {
        println!(
            "  [on ] active now   [off] a default this run turned off   \
             [opt] available, not on by default\n"
        );
    }
    println!("`--show <slug>` for the derivation. Prose: docs/tracks/deviations.md");
}

fn show(cfg: &ModeConfig, d: Deviation) {
    let e: &Entry = d.entry();
    println!("{}  ({})", e.slug, e.kind.slug());
    println!("{}\n", e.title);
    println!(
        "ACTIVE NOW   {}",
        if cfg.is_active(d) { "yes" } else { "no" }
    );
    println!(
        "IN IMPROVED  {}",
        match (e.kind.toggleable(), e.default_in_improved) {
            (false, _) => "n/a — not toggleable",
            (true, true) => "on by default",
            (true, false) => "available, off by default",
        }
    );
    println!(
        "CHECKSUM     {}",
        if e.affects_checksum {
            "can diverge a replay"
        } else {
            "no channel effect"
        }
    );
    println!("\nRETAIL\n  {}", wrap(e.retail));
    println!("\nOURS\n  {}", wrap(e.ours));
    println!("\nWHY\n  {}", wrap(e.why));
    println!("\nDERIVED FROM");
    for a in e.derived_from {
        println!("  {a}");
    }
    println!("\nEVIDENCE\n  {}", wrap(e.evidence));
    if !e.seam.is_empty() {
        println!("\nSEAM\n  {}", e.seam);
    }
}

/// Re-wrap a doc string (which arrives with source-indentation collapsed by the compiler's
/// line continuations) to 88 columns with a two-space hanging indent.
fn wrap(s: &str) -> String {
    let mut out = String::new();
    let mut col = 2usize;
    for word in s.split_whitespace() {
        if col + word.len() + 1 > 88 && col > 2 {
            out.push_str("\n  ");
            col = 2;
        } else if col > 2 {
            out.push(' ');
            col += 1;
        }
        out.push_str(word);
        col += word.len();
    }
    out
}

fn markdown() {
    println!("| # | deviation | kind | what retail does | what we do | improved default |");
    println!("|--:|---|---|---|---|---|");
    for (i, d) in Deviation::ALL.into_iter().enumerate() {
        let e = d.entry();
        let dflt = match (e.kind.toggleable(), e.default_in_improved) {
            (false, _) => "—",
            (true, true) => "**on**",
            (true, false) => "off",
        };
        println!(
            "| {} | `{}` | {} | {} | {} | {} |",
            i + 1,
            e.slug,
            e.kind.slug(),
            one_line(e.retail),
            one_line(e.ours),
            dflt
        );
    }
}

/// First sentence only, with table-hostile characters neutralised.
fn one_line(s: &str) -> String {
    let words: Vec<&str> = s.split_whitespace().collect();
    let joined = words.join(" ");
    let cut = joined.find(". ").map(|i| i + 1).unwrap_or(joined.len());
    joined[..cut].replace('|', "\\|")
}
