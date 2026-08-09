//! The builtin census over `ron-data/bhs-corpus` — 363 shipped `.bhs` scripts.
//!
//! The corpus is gitignored copyrighted game content. When it is absent these tests
//! **skip loudly** rather than passing vacuously; a green run on a machine without
//! the data is not evidence of anything and must not look like it is.

use don_bhs::builtins;
use don_bhs::corpus;

macro_rules! corpus_or_skip {
    () => {
        match corpus::default_corpus() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: ron-data/bhs-corpus is not present");
                return;
            }
        }
    };
}

#[test]
fn the_corpus_scans_and_the_headline_numbers_hold() {
    let root = corpus_or_skip!();
    let c = corpus::scan_dir(&root).unwrap();
    // Measured on 2026-08-08 against the shipped corpus. These are lower bounds so
    // that adding scripts does not break the test, but a *drop* means the scanner
    // regressed.
    assert!(c.files.len() >= 363, "files {}", c.files.len());
    assert!(c.lines >= 93_000, "lines {}", c.lines);
    assert!(
        c.distinct_called() >= 549,
        "distinct builtins {}",
        c.distinct_called()
    );
    assert!(c.total_calls() >= 39_000, "calls {}", c.total_calls());
}

#[test]
fn every_builtin_the_corpus_calls_resolves_against_the_binarys_table() {
    let root = corpus_or_skip!();
    let c = corpus::scan_dir(&root).unwrap();
    for r in &c.builtins {
        let d = corpus::decl_of(r).expect("row index is a real registration");
        assert_eq!(d.name, r.name);
        assert!(d.index < 873);
    }
}

/// The most-called builtin in the whole corpus is `create_unit_upgrade`, and the
/// most-called one that needs no simulation at all is `rand_int` — which draws from
/// `GameAccess::game_random`, the main lockstep stream. That single fact is why the
/// `Host::game_random` routing exists.
#[test]
fn the_ranking_is_stable_at_the_top() {
    let root = corpus_or_skip!();
    let c = corpus::scan_dir(&root).unwrap();
    assert_eq!(c.builtins[0].name, "create_unit_upgrade");
    let rand = c
        .builtins
        .iter()
        .position(|r| r.name == "rand_int")
        .expect("rand_int is called");
    assert!(rand < 10, "rand_int ranked {rand}");
    let util_calls: u64 = c
        .builtins
        .iter()
        .filter(|r| r.index <= builtins::UTIL_MAX_INDEX)
        .map(|r| r.calls)
        .sum();
    // The four utility FuncSets are a small slice of the workload; the 842
    // `ScenarioFuncSet` entries are where the real debt is, and pretending
    // otherwise would be the easy lie here.
    assert!(util_calls > 1_000 && util_calls < c.total_calls() / 10);
}

/// The measured coverage number this crate is allowed to claim. It is small, and
/// the point of the test is that it stays honest: the assertion is an equality band,
/// so implementing a builtin *or* silently dropping one both show up.
#[test]
fn implemented_call_share_is_measured_not_claimed() {
    let root = corpus_or_skip!();
    let c = corpus::scan_dir(&root).unwrap();
    let done = builtins::implemented_indices();
    let frac = c.coverage_fraction(&done);
    assert!(
        (0.015..0.030).contains(&frac),
        "implemented call share {frac:.4} left its measured band; \
         update the band deliberately when you implement more"
    );
    // The top of the debt list is a ScenarioFuncSet entry, i.e. `don-sim` work.
    let owed: Vec<_> = c.owed(&done).take(3).map(|r| r.name).collect();
    assert_eq!(
        owed,
        vec!["create_unit_upgrade", "create_unit_in_group", "set_timer"]
    );
}

/// `game_structs.bhs` is the shipped declaration of the standard structs, and the
/// aggregate opcodes exist for it. Its presence is what makes the struct work in
/// this crate testable against something real rather than invented.
#[test]
fn the_standard_struct_library_is_in_the_corpus() {
    let root = corpus_or_skip!();
    let p = root.join("scenario/scriptlibrary/game_structs.bhs");
    if !p.exists() {
        eprintln!("SKIP: game_structs.bhs not present");
        return;
    }
    let src = std::fs::read_to_string(&p).unwrap();
    for s in ["struct Vector", "struct UnitGroup", "int[] units"] {
        assert!(src.contains(s), "missing {s}");
    }
}

/// The scanner's own error bars, printed rather than assumed.
///
/// `defined` are the names the corpus declares itself — script functions and named
/// `trigger` blocks — which are excluded from the builtin counts. `unresolved` are
/// call-shaped identifiers that are neither: the language's own statement forms
/// (`enable_trigger`, `disable_trigger`), trigger references, and cross-file script
/// calls. A builtin count can only be *inflated* by a corpus-defined name that
/// collides with a registered builtin name and escapes `defined`, so this is the
/// number to watch if the ranking ever looks wrong.
#[test]
fn scanner_exclusion_counts_are_reported() {
    let root = corpus_or_skip!();
    let c = corpus::scan_dir(&root).unwrap();
    eprintln!(
        "corpus defines {} names; {} call-shaped identifiers unresolved",
        c.defined.len(),
        c.unresolved.len()
    );
    assert!(!c.defined.is_empty());
    // `enable_trigger` / `disable_trigger` are NOT registered builtins in this build:
    // they are language statements that compile to OP_BIT_UNSET / OP_BIT_SET, which
    // is the independent corroboration that those two opcode names are inverted
    // relative to the machine (0x3d is `bts`, 0x3c is `btr`).
    assert!(don_bhs::find_builtin("enable_trigger").is_none());
    assert!(don_bhs::find_builtin("disable_trigger").is_none());
    assert!(c.unresolved.contains_key("enable_trigger"));
}
