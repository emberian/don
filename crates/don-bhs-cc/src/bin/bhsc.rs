//! `bhsc` — the BHS compiler driver.
//!
//! `bhsc parse <dir-or-file>...` parses every `.bhs` under the given paths and reports
//! the rate. This is the lane's primary correctness signal: the shipped corpus at
//! `ron-data/bhs-corpus/` is 363 files of real programs by the original developers, and
//! "parses all 363" is the strongest claim available.

use std::path::{Path, PathBuf};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: bhsc parse <path>...");
        std::process::exit(2);
    }
    match args[0].as_str() {
        "parse" => cmd_parse(&args[1..]),
        "stats" => cmd_stats(&args[1..]),
        "compile" => cmd_compile(&args[1..]),
        other => {
            eprintln!("unknown subcommand `{other}`");
            std::process::exit(2);
        }
    }
}

fn collect(paths: &[String]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in paths {
        walk(Path::new(p), &mut out);
    }
    out.sort();
    out
}

fn walk(p: &Path, out: &mut Vec<PathBuf>) {
    if p.is_dir() {
        let mut entries: Vec<_> = match std::fs::read_dir(p) {
            Ok(d) => d.filter_map(|e| e.ok()).map(|e| e.path()).collect(),
            Err(_) => return,
        };
        entries.sort();
        for e in entries {
            walk(&e, out);
        }
    } else if p
        .extension()
        .map(|e| e.eq_ignore_ascii_case("bhs"))
        .unwrap_or(false)
    {
        out.push(p.to_path_buf());
    }
}

fn cmd_parse(paths: &[String]) {
    let files = collect(paths);
    let mut ok = 0usize;
    let mut fails: Vec<(PathBuf, String)> = Vec::new();
    for f in &files {
        match don_bhs_cc::parse_path(f) {
            Ok(_) => ok += 1,
            Err(e) => fails.push((f.clone(), e.to_string())),
        }
    }
    for (f, e) in &fails {
        println!("FAIL {}: {}", f.display(), e);
    }
    println!("\nparsed {}/{} files", ok, files.len());
    if !fails.is_empty() {
        std::process::exit(1);
    }
}

/// AST census. Its point is falsifiability: if the parser were quietly swallowing
/// constructs (an over-permissive `at_decl`, a runaway expression statement), the shape
/// counts here would not match a `grep` of the same corpus.
fn cmd_stats(paths: &[String]) {
    use don_bhs_cc::ast::*;
    #[derive(Default)]
    struct C {
        files: usize,
        include: usize,
        labels_item: usize,
        labels_stmt: usize,
        structs: usize,
        script_decl: usize,
        script_def: usize,
        main: usize,
        file_var: usize,
        stmts: usize,
        ifs: usize,
        elses: usize,
        fors: usize,
        whiles: usize,
        dowhiles: usize,
        switches: usize,
        cases: usize,
        defaults: usize,
        breaks: usize,
        continues: usize,
        returns: usize,
        decls: usize,
        statics: usize,
        triggers: usize,
        run_once: usize,
        calls: usize,
        method_calls: usize,
        casts: usize,
        array_lits: usize,
        locstr: usize,
        ref_params: usize,
        untyped_params: usize,
    }
    fn expr(e: &Expr, c: &mut C) {
        match e {
            Expr::Call { args, .. } => {
                c.calls += 1;
                args.iter().for_each(|a| expr(a, c));
            }
            Expr::MethodCall { recv, args, .. } => {
                c.method_calls += 1;
                expr(recv, c);
                args.iter().for_each(|a| expr(a, c));
            }
            Expr::Cast { expr: e2, .. } => {
                c.casts += 1;
                expr(e2, c);
            }
            Expr::ArrayLit(items, _) => {
                c.array_lits += 1;
                items.iter().for_each(|a| expr(a, c));
            }
            Expr::LocStr(..) => c.locstr += 1,
            Expr::Unary { expr: e2, .. }
            | Expr::PreIncDec { expr: e2, .. }
            | Expr::PostIncDec { expr: e2, .. } => expr(e2, c),
            Expr::Binary { lhs, rhs, .. } => {
                expr(lhs, c);
                expr(rhs, c);
            }
            Expr::Assign { target, value, .. } => {
                expr(target, c);
                expr(value, c);
            }
            Expr::Index { base, index, .. } => {
                expr(base, c);
                expr(index, c);
            }
            Expr::Member { base, .. } => expr(base, c),
            _ => {}
        }
    }
    fn decl(v: &VarDecl, c: &mut C) {
        c.decls += 1;
        if v.is_static {
            c.statics += 1;
        }
        for d in &v.decls {
            if let Some(ArraySuffix::Sized(e)) = &d.array {
                expr(e, c);
            }
            if let Some(e) = &d.init {
                expr(e, c);
            }
        }
    }
    fn stmt(s: &Stmt, c: &mut C) {
        c.stmts += 1;
        match s {
            Stmt::Expr(e) => expr(e, c),
            Stmt::Decl(v) => decl(v, c),
            Stmt::Block(b) => b.stmts.iter().for_each(|s| stmt(s, c)),
            Stmt::If {
                cond, then, els, ..
            } => {
                c.ifs += 1;
                expr(cond, c);
                stmt(then, c);
                if let Some(e) = els {
                    c.elses += 1;
                    stmt(e, c);
                }
            }
            Stmt::While { cond, body, .. } => {
                c.whiles += 1;
                expr(cond, c);
                stmt(body, c);
            }
            Stmt::DoWhile { body, cond, .. } => {
                c.dowhiles += 1;
                stmt(body, c);
                expr(cond, c);
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
                ..
            } => {
                c.fors += 1;
                if let Some(i) = init {
                    stmt(i, c);
                }
                if let Some(e) = cond {
                    expr(e, c);
                }
                if let Some(e) = step {
                    expr(e, c);
                }
                stmt(body, c);
            }
            Stmt::Switch { subject, arms, .. } => {
                c.switches += 1;
                expr(subject, c);
                for a in arms {
                    for l in &a.labels {
                        match &l.value {
                            Some(e) => {
                                c.cases += 1;
                                expr(e, c);
                            }
                            None => c.defaults += 1,
                        }
                    }
                    a.body.iter().for_each(|s| stmt(s, c));
                }
            }
            Stmt::Break(_) => c.breaks += 1,
            Stmt::Continue(_) => c.continues += 1,
            Stmt::Return { value, .. } => {
                c.returns += 1;
                if let Some(e) = value {
                    expr(e, c);
                }
            }
            Stmt::Labels { .. } => c.labels_stmt += 1,
            Stmt::Trigger { cond, body, .. } => {
                c.triggers += 1;
                if let Some(e) = cond {
                    expr(e, c);
                }
                stmt(body, c);
            }
            Stmt::RunOnce { body, .. } => {
                c.run_once += 1;
                body.stmts.iter().for_each(|s| stmt(s, c));
            }
            Stmt::Struct(_) => c.structs += 1,
            Stmt::Empty(_) => {}
        }
    }

    let mut c = C::default();
    for f in collect(paths) {
        let Ok(sf) = don_bhs_cc::parse_path(&f) else {
            continue;
        };
        c.files += 1;
        for it in &sf.items {
            match it {
                Item::Include { .. } => c.include += 1,
                Item::Labels { .. } => c.labels_item += 1,
                Item::Struct(_) => c.structs += 1,
                Item::Var(v) => {
                    c.file_var += 1;
                    decl(v, &mut c);
                }
                Item::Main(m) => {
                    c.main += 1;
                    m.body.stmts.iter().for_each(|s| stmt(s, &mut c));
                }
                Item::Script(s) => {
                    for p in &s.sig.params {
                        if p.by_ref {
                            c.ref_params += 1;
                        }
                        if p.ty.is_none() {
                            c.untyped_params += 1;
                        }
                    }
                    match &s.body {
                        None => c.script_decl += 1,
                        Some(b) => {
                            c.script_def += 1;
                            b.stmts.iter().for_each(|s| stmt(s, &mut c));
                        }
                    }
                }
            }
        }
    }
    println!("files                {}", c.files);
    println!("include              {}", c.include);
    println!("labels (file/stmt)   {} / {}", c.labels_item, c.labels_stmt);
    println!("struct               {}", c.structs);
    println!("script decl/def      {} / {}", c.script_decl, c.script_def);
    println!("anonymous entry      {}", c.main);
    println!("file-scope var       {}", c.file_var);
    println!("statements           {}", c.stmts);
    println!("  if / else          {} / {}", c.ifs, c.elses);
    println!(
        "  for / while / do   {} / {} / {}",
        c.fors, c.whiles, c.dowhiles
    );
    println!(
        "  switch/case/default {} / {} / {}",
        c.switches, c.cases, c.defaults
    );
    println!("  break / continue   {} / {}", c.breaks, c.continues);
    println!("  return             {}", c.returns);
    println!("  decl (of which static) {} ({})", c.decls, c.statics);
    println!("  trigger / run_once {} / {}", c.triggers, c.run_once);
    println!("calls / method calls {} / {}", c.calls, c.method_calls);
    println!("casts                {}", c.casts);
    println!("array literals       {}", c.array_lits);
    println!("$S literals          {}", c.locstr);
    println!("ref params           {}", c.ref_params);
    println!("untyped params       {}", c.untyped_params);
}

/// Compile every given root `.bhs` (each with its own include closure) and report.
///
/// The decode pass at the end is the falsifiable part: every emitted byte is walked with
/// `don_bhs::opcode`'s table, so a mis-sized instruction, an unpatched jump or a stray
/// operand fails loudly rather than producing plausible-looking bytes.
fn cmd_compile(paths: &[String]) {
    use don_bhs::opcode::{self, OperandKind};
    use don_bhs_cc::sema::{self, Severity};

    let roots: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
    let files = collect(paths);
    let inc = sema::IncludePath::with_roots(roots);

    let mut clean = 0usize;
    let mut with_errors = 0usize;
    let mut total_bytes = 0usize;
    let mut total_instrs = 0usize;
    let mut total_scripts = 0usize;
    let mut decode_failures: Vec<String> = Vec::new();
    let mut error_kinds: std::collections::BTreeMap<String, usize> = Default::default();
    let mut note_kinds: std::collections::BTreeMap<String, usize> = Default::default();
    let mut implicit_decls = 0usize;
    let mut auto_casts = 0usize;
    let mut warning_examples: Vec<String> = Vec::new();
    let mut histo = [0usize; 73];
    let mut unknown_names: std::collections::BTreeMap<String, usize> = Default::default();

    for f in &files {
        let unit = match sema::analyze(f, &inc) {
            Ok(u) => u,
            Err(e) => {
                with_errors += 1;
                *error_kinds.entry(format!("{e}")).or_default() += 1;
                continue;
            }
        };
        let (prog, mut diags, st) = don_bhs_cc::codegen::compile(&unit);
        auto_casts += st.auto_casts;
        diags.extend(unit.diags.iter().cloned());

        let mut file_errors = 0usize;
        for d in &diags {
            match d.severity {
                Severity::Error => {
                    file_errors += 1;
                    *error_kinds.entry(kind_of(&d.msg)).or_default() += 1;
                    if let Some(n) = d.msg.strip_prefix("unknown function `") {
                        *unknown_names
                            .entry(n.trim_end_matches('`').to_string())
                            .or_default() += 1;
                    }
                }
                Severity::Warning => {
                    *note_kinds.entry(kind_of(&d.msg)).or_default() += 1;
                    if warning_examples.len() < 10 {
                        warning_examples.push(d.to_string());
                    }
                }
                Severity::Note => {
                    implicit_decls += 1;
                }
            }
        }
        if file_errors == 0 {
            clean += 1;
        } else {
            with_errors += 1;
        }

        // Only the root file's own code is attributable to this compilation; included
        // files are compiled again as their own roots.
        let sf = &prog.files[0];
        total_bytes += sf.code.len();
        total_scripts += sf.scripts.len();
        let mut i = 0usize;
        let mut ok = true;
        while i < sf.code.len() {
            let Some(d) = opcode::decode(sf.code[i]) else {
                decode_failures.push(format!(
                    "{}: bad opcode {:#04x} at {i}",
                    f.display(),
                    sf.code[i]
                ));
                ok = false;
                break;
            };
            let len = 1 + 4 * d.operands.len();
            if i + len > sf.code.len() {
                decode_failures.push(format!("{}: truncated {} at {i}", f.display(), d.name));
                ok = false;
                break;
            }
            for (k, o) in d.operands.iter().enumerate() {
                if matches!(o, OperandKind::CodeOffset) {
                    let at = i + 1 + 4 * k;
                    let t = u32::from_le_bytes(sf.code[at..at + 4].try_into().unwrap()) as usize;
                    if t > sf.code.len() {
                        decode_failures.push(format!(
                            "{}: {} at {i} jumps to {t}, past {} bytes",
                            f.display(),
                            d.name,
                            sf.code.len()
                        ));
                        ok = false;
                    }
                }
            }
            histo[sf.code[i] as usize] += 1;
            i += len;
            total_instrs += 1;
        }
        let _ = ok;
    }

    println!("compiled cleanly      {}/{}", clean, files.len());
    println!("with errors           {}", with_errors);
    println!("scripts emitted       {}", total_scripts);
    println!(
        "bytecode              {} bytes / {} instructions",
        total_bytes, total_instrs
    );
    println!("decode failures       {}", decode_failures.len());
    for d in decode_failures.iter().take(20) {
        println!("  {d}");
    }
    println!("implicit declarations {}", implicit_decls);
    println!("auto-casts inserted   {}", auto_casts);
    let used = histo.iter().filter(|&&n| n > 0).count();
    println!("opcodes used          {}/73", used);
    let mut order: Vec<usize> = (0..73).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(histo[i]));
    println!("\nopcode histogram (emitted):");
    for i in order {
        if histo[i] == 0 {
            continue;
        }
        println!("  {:8}  {:#04x} {}", histo[i], i, opcode::OPCODES[i].name);
    }
    let never: Vec<&str> = (0..73)
        .filter(|&i| histo[i] == 0)
        .map(|i| opcode::OPCODES[i].name)
        .collect();
    println!("\nnever emitted ({}): {}", never.len(), never.join(" "));
    if !unknown_names.is_empty() {
        let mut v: Vec<_> = unknown_names.iter().collect();
        v.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
        println!("\ndistinct unknown functions: {}", v.len());
        for (k, n) in v.iter().take(40) {
            println!("  {n:6}  {k}");
        }
    }
    if !error_kinds.is_empty() {
        println!("\nerrors by kind:");
        let mut v: Vec<_> = error_kinds.into_iter().collect();
        v.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        for (k, n) in v.iter().take(30) {
            println!("  {n:6}  {k}");
        }
    }
    if !note_kinds.is_empty() {
        println!("\nwarnings by kind:");
        let mut v: Vec<_> = note_kinds.into_iter().collect();
        v.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        for (k, n) in v.iter().take(20) {
            println!("  {n:6}  {k}");
        }
        for w in &warning_examples {
            println!("    {w}");
        }
    }
}

/// Collapse a diagnostic into a countable kind by stripping the quoted specifics.
fn kind_of(msg: &str) -> String {
    let mut out = String::new();
    let mut in_q = false;
    for c in msg.chars() {
        if c == '`' {
            if !in_q {
                out.push_str("`…`");
            }
            in_q = !in_q;
        } else if !in_q {
            out.push(c);
        }
    }
    out
}
