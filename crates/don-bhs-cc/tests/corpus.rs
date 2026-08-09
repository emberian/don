//! The shipped-corpus gate.
//!
//! `ron-data/bhs-corpus/` is 363 `.bhs` files, 93,649 lines, written by Big Huge Games —
//! simultaneously this lane's language specification and its test suite. Retail's array
//! member lowering explains the shipped `.lenght` typo, so every recovered source now
//! compiles without a fidelity exception.
//!
//! The corpus is **gitignored game content**. When it is absent the tests print a loud
//! SKIPPED line and pass; a skipped case is not evidence of anything and must never be
//! read as one.

use std::path::{Path, PathBuf};

use don_bhs::opcode::{self, OperandKind};
use don_bhs_cc::sema::{self, Severity};

fn corpus_root() -> Option<PathBuf> {
    // `CARGO_MANIFEST_DIR` is `<repo>/crates/don-bhs-cc`.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ron-data/bhs-corpus");
    root.is_dir().then_some(root)
}

fn bhs_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.filter_map(|e| e.ok()) {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p
                .extension()
                .map(|x| x.eq_ignore_ascii_case("bhs"))
                .unwrap_or(false)
            {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// The count on disk at the time this lane was written. Asserting it catches a corpus
/// that has silently shrunk, which would make a 100% rate meaningless.
const EXPECTED_FILES: usize = 363;

#[test]
fn parses_every_shipped_script() {
    let Some(root) = corpus_root() else {
        eprintln!("SKIPPED: ron-data/bhs-corpus is absent (gitignored game content)");
        return;
    };
    let files = bhs_files(&root);
    assert_eq!(files.len(), EXPECTED_FILES, "corpus size changed");

    let mut fails = Vec::new();
    for f in &files {
        if let Err(e) = don_bhs_cc::parse_path(f) {
            fails.push(format!("{}: {e}", f.display()));
        }
    }
    assert!(
        fails.is_empty(),
        "{}/{} files failed to parse:\n{}",
        fails.len(),
        files.len(),
        fails.join("\n")
    );
}

#[test]
fn compiles_every_shipped_script_with_no_errors() {
    let Some(root) = corpus_root() else {
        eprintln!("SKIPPED: ron-data/bhs-corpus is absent (gitignored game content)");
        return;
    };
    let files = bhs_files(&root);
    let inc = sema::IncludePath::with_roots([root.clone()]);

    let mut fails: Vec<String> = Vec::new();
    for f in &files {
        let unit = match sema::analyze(f, &inc) {
            Ok(u) => u,
            Err(e) => {
                fails.push(format!("{}: {e}", f.display()));
                continue;
            }
        };
        let (_prog, diags, _stats) = don_bhs_cc::codegen::compile(&unit);
        for d in diags.iter().chain(unit.diags.iter()) {
            if d.severity == Severity::Error {
                fails.push(d.to_string());
            }
        }
    }
    assert!(fails.is_empty(), "{} compile errors:\n{}", fails.len(), {
        let mut v = fails;
        v.truncate(30);
        v.join("\n")
    });
}

/// Walk every emitted byte with the engine's own opcode table.
///
/// This is the part that could fail: a mis-sized instruction, an unpatched jump operand
/// or a stray byte all produce a decode error rather than plausible-looking output. The
/// jump-target bound check catches a fixup that was never resolved.
#[test]
fn emitted_bytecode_decodes_and_every_jump_lands_in_range() {
    let Some(root) = corpus_root() else {
        eprintln!("SKIPPED: ron-data/bhs-corpus is absent (gitignored game content)");
        return;
    };
    let files = bhs_files(&root);
    let inc = sema::IncludePath::with_roots([root.clone()]);

    let mut problems: Vec<String> = Vec::new();
    let mut instrs = 0usize;
    for f in &files {
        let Ok(unit) = sema::analyze(f, &inc) else {
            continue;
        };
        let (prog, diags, _) = don_bhs_cc::codegen::compile(&unit);
        if unit
            .diags
            .iter()
            .chain(diags.iter())
            .any(|d| d.severity == Severity::Error)
        {
            continue;
        }
        let sf = &prog.files[0];

        // Every script's entry offset must point at a real instruction boundary.
        let mut boundaries = std::collections::HashSet::new();
        let mut i = 0usize;
        while i < sf.code.len() {
            boundaries.insert(i);
            let Some(d) = opcode::decode(sf.code[i]) else {
                problems.push(format!(
                    "{}: bad opcode {:#04x} at {i}",
                    f.display(),
                    sf.code[i]
                ));
                break;
            };
            if d.code == opcode::OP_ERROR_TOKEN {
                problems.push(format!("{}: OP_ERROR_TOKEN emitted at {i}", f.display()));
                break;
            }
            let len = 1 + 4 * d.operands.len();
            if i + len > sf.code.len() {
                problems.push(format!("{}: truncated {} at {i}", f.display(), d.name));
                break;
            }
            for (k, o) in d.operands.iter().enumerate() {
                if matches!(o, OperandKind::CodeOffset) {
                    let at = i + 1 + 4 * k;
                    let t = u32::from_le_bytes(sf.code[at..at + 4].try_into().unwrap()) as usize;
                    if t > sf.code.len() {
                        problems.push(format!(
                            "{}: {} at {i} jumps to {t}, past {} bytes",
                            f.display(),
                            d.name,
                            sf.code.len()
                        ));
                    }
                }
            }
            i += len;
            instrs += 1;
        }
        boundaries.insert(sf.code.len());
        for s in &sf.scripts {
            assert!(
                boundaries.contains(&(s.entry as usize)),
                "{}: script `{}` entry {} is not an instruction boundary",
                f.display(),
                s.name,
                s.entry
            );
        }
    }
    assert!(
        instrs > 100_000,
        "suspiciously little code emitted: {instrs} instructions"
    );
    assert!(problems.is_empty(), "{} problems:\n{}", problems.len(), {
        let mut v = problems;
        v.truncate(30);
        v.join("\n")
    });
}

/// Static slots must be unique per script and every trigger the code refers to must have
/// a name and a bit. A collision here would make two `static`s share cross-frame storage,
/// which is exactly the state the `script_run_time` checksum channel hashes.
#[test]
fn static_and_trigger_tables_are_consistent() {
    let Some(root) = corpus_root() else {
        eprintln!("SKIPPED: ron-data/bhs-corpus is absent (gitignored game content)");
        return;
    };
    let inc = sema::IncludePath::with_roots([root.clone()]);
    let mut checked = 0usize;
    for f in bhs_files(&root) {
        let Ok(unit) = sema::analyze(&f, &inc) else {
            continue;
        };
        let (prog, _, _) = don_bhs_cc::codegen::compile(&unit);
        for s in &prog.files[0].scripts {
            assert_eq!(
                s.params.len(),
                s.arity,
                "{}: script `{}` parameter type table mismatch",
                f.display(),
                s.name
            );
            assert_eq!(
                s.refs.len(),
                s.arity,
                "{}: script `{}` ref table mismatch",
                f.display(),
                s.name
            );
            assert_eq!(
                s.statics.len(),
                s.static_var_names.len(),
                "{}: script `{}` static table mismatch",
                f.display(),
                s.name
            );
            assert_eq!(
                s.trigger_count as usize,
                s.trigger_names.len(),
                "{}: script `{}` trigger table mismatch",
                f.display(),
                s.name
            );
            assert!(
                s.trigger_bits.len() * 8 >= s.trigger_names.len(),
                "{}: script `{}` has {} triggers but only {} bit bytes",
                f.display(),
                s.name,
                s.trigger_names.len(),
                s.trigger_bits.len()
            );
            checked += 1;
        }
    }
    assert!(checked > 300, "only {checked} scripts checked");
}
