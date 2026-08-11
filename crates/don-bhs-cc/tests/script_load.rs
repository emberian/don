// SPDX-License-Identifier: GPL-3.0-or-later
//! The shipped-script load path: `Compiler::compile` `0x009bf160` + `Lexer::open_file`
//! `0x009bff30`, end to end.
//!
//! These tests answer one question: can a *shipped* `.bhs` become a `don_bhs::Program`
//! bound to retail's tick step 4, without anyone hand-writing the source? Before this
//! module the answer was no — every consumer in the tree built a `Program` from a source
//! string written inside a test.
//!
//! `ron-data/bhs-corpus/` and `ron-data/internal_strings.xml` are gitignored game content.
//! When they are absent the corpus-backed tests print a loud SKIPPED line and pass; a
//! skipped case is not evidence of anything.

use std::path::{Path, PathBuf};

use don_bhs_cc::load::{self, LoadError};
use don_bhs_cc::sema::{self, Severity};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn corpus_root() -> Option<PathBuf> {
    let root = repo().join("ron-data/bhs-corpus");
    root.is_dir().then_some(root)
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "don-bhs-cc-load-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

/// The shipped tree really does hold the path `ScenarioFuncSet::init` reads.
///
/// Catches: someone editing [`load::GENERAL_POWERS_SCRIPT_FILE`] to a plausible-looking
/// path. The expectation is read out of the shipped string table at the ordinal the
/// instruction stream names, not transcribed here.
#[test]
fn the_general_powers_path_is_the_shipped_string_table_ordinal_5958() {
    let strings = repo().join("ron-data/internal_strings.xml");
    if !strings.is_file() {
        eprintln!("SKIPPED: ron-data/internal_strings.xml is absent (gitignored game content)");
        return;
    }
    // Document order, `<STRING>` elements, ordinal 5958 — the `0x1d178 / 0x14` in
    // `ScenarioFuncSet::init` at `0x00a0401c`. Parse as XML: a `<STRING hash=` regex
    // drops eight elements before ordinal 5950 and would select the wrong entry.
    let text = std::fs::read_to_string(&strings).unwrap();
    let mut values: Vec<String> = Vec::new();
    let mut rest = text.as_str();
    while let Some(i) = rest.find("<STRING") {
        rest = &rest[i + "<STRING".len()..];
        let Some(gt) = rest.find('>') else { break };
        // Skip a self-closing element, which carries no text.
        if rest.as_bytes().get(gt.wrapping_sub(1)) == Some(&b'/') {
            values.push(String::new());
            rest = &rest[gt + 1..];
            continue;
        }
        rest = &rest[gt + 1..];
        let end = rest.find("</STRING>").unwrap_or(0);
        values.push(rest[..end].to_string());
        rest = &rest[end..];
    }
    assert_eq!(
        values.len(),
        7630,
        "internal_strings.xml element count changed; the ordinal binder is positional"
    );
    assert_eq!(values[5958], load::GENERAL_POWERS_SCRIPT_FILE);
}

/// The headline: the second script retail runs at tick step 4 loads from the shipped tree
/// and produces a legal zero-argument binding.
#[test]
fn shipped_general_powers_loads_as_a_zero_argument_step_four_entry() {
    let Some(root) = corpus_root() else {
        eprintln!("SKIPPED: ron-data/bhs-corpus is absent (gitignored game content)");
        return;
    };
    let inc = load::install_include_path(&root);
    let loaded = load::load_script(&inc, load::GENERAL_POWERS_SCRIPT_FILE)
        .expect("shipped general_powers.bhs must load through the retail candidate order");

    assert_eq!(loaded.entry, "general_powers");
    assert_eq!(
        loaded.resolved_name,
        "./scenario/scriptlibrary/general_powers.bhs",
        "check_ext must leave an already-.bhs path alone"
    );
    let idx = loaded
        .tick_entry_index()
        .expect("the entry script must exist with arity 0 to be a step-4 binding");
    let script = &loaded.program.files[0].scripts[idx];
    assert_eq!(script.arity, 0);
    // A `Program` with no code would satisfy every assertion above and run nothing.
    assert!(
        !loaded.program.files[0].code.is_empty(),
        "the compiled image must carry bytecode"
    );
    // The shipped body reads `static int alexander = 0;` and friends: cross-frame state is
    // what lands in checksum channel 15, so a load path that dropped statics would be
    // useless even though it compiled. The expectation is the 14 `static` declarations in
    // the shipped file, in declaration order.
    assert_eq!(
        script.static_var_names,
        vec![
            "counter",
            "alexander",
            "napoleon",
            "parmenio",
            "antipater",
            "ptolemy",
            "darius",
            "memnon",
            "spitamenes",
            "maurya",
            "porus",
            "fouche",
            "schwarzenberg",
            "paoli",
        ]
    );
    // The one `run_once` block lowers to an unnamed trigger, not a static.
    assert_eq!(script.trigger_count, 1, "the shipped run_once block");
}

/// The same script, loaded without its extension, exactly as a setup screen would name it.
#[test]
fn a_bare_script_name_gets_the_retail_extension_and_resolves() {
    let Some(root) = corpus_root() else {
        eprintln!("SKIPPED: ron-data/bhs-corpus is absent (gitignored game content)");
        return;
    };
    let inc = load::install_include_path(&root);
    // No directory at all: this can only resolve through candidate 3, the
    // `ScriptIncludePath` entry `.\scenario\scriptlibrary\`.
    let loaded = load::load_script(&inc, "general_powers").expect("include-path candidate");
    assert_eq!(loaded.resolved_name, "general_powers.bhs");
    assert!(loaded.root_path.ends_with("general_powers.bhs"));
}

/// Retail's three candidates resolve every `include` in the shipped corpus, and they are
/// the *only* thing that resolves them.
#[test]
fn every_shipped_include_resolves_under_the_retail_candidate_order() {
    let Some(root) = corpus_root() else {
        eprintln!("SKIPPED: ron-data/bhs-corpus is absent (gitignored game content)");
        return;
    };
    let inc = load::install_include_path(&root);
    let mut files = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p
                .extension()
                .map(|x| x.eq_ignore_ascii_case("bhs"))
                .unwrap_or(false)
            {
                files.push(p);
            }
        }
    }
    files.sort();
    assert_eq!(files.len(), 363, "corpus size changed");

    let mut unresolved: Vec<String> = Vec::new();
    let mut reinclusion_roots: Vec<String> = Vec::new();
    for f in &files {
        let unit = sema::analyze(f, &inc).expect("parse");
        for d in &unit.diags {
            if d.severity == Severity::Error && d.msg.contains("cannot resolve `include") {
                unresolved.push(format!("{}: {}", f.display(), d.msg));
            }
            if d.msg.contains("already in this unit") {
                reinclusion_roots.push(f.display().to_string());
            }
        }
    }
    assert!(
        unresolved.is_empty(),
        "{} shipped includes do not resolve under Lexer::open_file's candidates:\n{}",
        unresolved.len(),
        unresolved.join("\n")
    );
    // Measured, not chosen: five roots reach `game_structs.bhs` twice, through their own
    // `include` and again through `ctw_lib.bhs`. Retail's `included_files` guard cannot
    // fire, so retail may lex it twice where this compiler lexes it once. The count is
    // pinned so the divergence stays visible until reference bytecode settles it.
    reinclusion_roots.sort();
    reinclusion_roots.dedup();
    assert_eq!(
        reinclusion_roots.len(),
        5,
        "shipped re-inclusion count changed: {reinclusion_roots:#?}"
    );
}

/// The bug the previous resolver had: a recursive basename scan of the install tree.
///
/// `decoy/lib.bhs` sorts before `scenario/scriptlibrary/lib.bhs` and would win a sorted
/// directory walk. Retail never looks there, and neither may we.
#[test]
fn a_same_named_file_outside_the_candidates_is_never_used() {
    let root = tmp("decoy");
    write(
        &root,
        "aaa_decoy/lib.bhs",
        "scenario decoy_marker () { }\nscenario lib () { }\n",
    );
    write(
        &root,
        "scenario/scriptlibrary/lib.bhs",
        "scenario library_marker () { }\nscenario lib () { }\n",
    );
    write(
        &root,
        "scenario/Custom/root.bhs",
        "include \"lib.bhs\"\nscenario root () { }\n",
    );

    let inc = load::install_include_path(&root);
    let loaded = load::load_script_file(&inc, &root.join("scenario/Custom/root.bhs"))
        .expect("the scriptlibrary candidate must resolve");
    let names: Vec<&str> = loaded.program.files[1]
        .scripts
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert!(
        names.iter().any(|n| *n == "library_marker"),
        "expected the scriptlibrary file, got {names:?}"
    );
    assert!(
        !names.iter().any(|n| *n == "decoy_marker"),
        "the basename walk picked {names:?} from outside retail's candidate set"
    );

    // And a name that exists *only* outside the candidates must fail to resolve. Retail
    // reports it through `Compiler::comp_error`; accepting it would compile programs the
    // shipped engine rejects.
    write(&root, "aaa_decoy/orphan.bhs", "scenario orphan () { }\n");
    write(
        &root,
        "scenario/Custom/root2.bhs",
        "include \"orphan.bhs\"\nscenario root2 () { }\n",
    );
    let err = load::load_script_file(&inc, &root.join("scenario/Custom/root2.bhs"));
    match err {
        Err(LoadError::Diagnostics(d)) => assert!(
            d.iter().any(|x| x.msg.contains("cannot resolve `include")),
            "{d:#?}"
        ),
        other => panic!("expected an unresolved include, got {other:?}"),
    }

    std::fs::remove_dir_all(&root).ok();
}

/// Candidate 1 beats candidates 2 and 3: the including file's own directory wins.
#[test]
fn the_including_files_own_directory_is_tried_first() {
    let root = tmp("candidate-order");
    write(
        &root,
        "scenario/scriptlibrary/lib.bhs",
        "scenario library_marker () { }\n",
    );
    write(&root, "lib.bhs", "scenario content_root_marker () { }\n");
    write(
        &root,
        "scenario/Custom/lib.bhs",
        "scenario sibling_marker () { }\n",
    );
    write(
        &root,
        "scenario/Custom/root.bhs",
        "include \"lib.bhs\"\nscenario root () { }\n",
    );

    let inc = load::install_include_path(&root);
    let loaded = load::load_script_file(&inc, &root.join("scenario/Custom/root.bhs")).unwrap();
    let names: Vec<&str> = loaded.program.files[1]
        .scripts
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(names, vec!["sibling_marker"], "candidate 1 must win");

    // Remove the sibling and candidate 2 (the bare name against the content dir) takes
    // over, ahead of the include-path entry.
    std::fs::remove_file(root.join("scenario/Custom/lib.bhs")).unwrap();
    let loaded = load::load_script_file(&inc, &root.join("scenario/Custom/root.bhs")).unwrap();
    let names: Vec<&str> = loaded.program.files[1]
        .scripts
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(names, vec!["content_root_marker"], "candidate 2 must win");

    // Remove that too and only the `ScriptIncludePath` entry is left.
    std::fs::remove_file(root.join("lib.bhs")).unwrap();
    let loaded = load::load_script_file(&inc, &root.join("scenario/Custom/root.bhs")).unwrap();
    let names: Vec<&str> = loaded.program.files[1]
        .scripts
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(names, vec!["library_marker"], "candidate 3 must win");

    std::fs::remove_dir_all(&root).ok();
}

/// `Lexer::start_include_file` refuses at file-stack depth 16 before opening anything.
#[test]
fn include_nesting_stops_at_the_retail_limit_of_sixteen() {
    let root = tmp("depth");
    // A chain of 24 files, each including the next.
    for i in 0..24 {
        let body = if i == 23 {
            format!("scenario f{i} () {{ }}\n")
        } else {
            format!("include \"f{}.bhs\"\nscenario f{i} () {{ }}\n", i + 1)
        };
        write(&root, &format!("f{i}.bhs"), &body);
    }
    let inc = load::install_include_path(&root);
    let unit = sema::analyze(&root.join("f0.bhs"), &inc).unwrap();
    let over: Vec<&sema::Diag> = unit
        .diags
        .iter()
        .filter(|d| d.msg.contains("exceeds the retail nesting limit"))
        .collect();
    assert!(!over.is_empty(), "a 24-deep chain must hit the limit");
    // `file_stack` counts *suspended* files, so the root is at 0 and f16 is the first file
    // whose own include sees a full stack. Pin the file and the opened count so an
    // off-by-one in either direction is visible.
    assert!(
        over[0].file.ends_with("f16.bhs"),
        "refusal landed in {}",
        over[0].file
    );
    assert_eq!(unit.files.len(), 17, "nothing past the limit may be opened");

    // A chain that ends before the cap must load cleanly: f8..f23 is 16 files, whose
    // deepest include is written in f22 at stack depth 14.
    let inc16 = load::install_include_path(&root);
    let unit = sema::analyze(&root.join("f8.bhs"), &inc16).unwrap();
    assert!(
        !unit
            .diags
            .iter()
            .any(|d| d.msg.contains("exceeds the retail nesting limit")),
        "a 16-file chain is legal"
    );
    assert_eq!(unit.files.len(), 16);

    std::fs::remove_dir_all(&root).ok();
}
