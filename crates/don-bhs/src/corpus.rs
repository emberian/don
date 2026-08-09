//! A **lexical** census of builtin calls across a corpus of `.bhs` source.
//!
//! `ron-data/bhs-corpus/` holds 363 shipped scripts — 93,649 lines of the campaign,
//! Conquer-the-World, and scenario-library code that Big Huge Games actually wrote.
//! It is the only workload we have that tells us *which* of the 873 registered
//! builtins matter, and in what proportion.
//!
//! # This is deliberately not a parser
//!
//! A sibling lane owns the BHS compiler front end. This module tokenises just far
//! enough to count call sites: it strips `/* */` and `//` comments and string
//! literals, then matches `identifier (`. That is enough to rank the surface and
//! nothing more, and the numbers it produces are labelled as what they are.
//!
//! Two known and bounded inaccuracies, both stated rather than hidden:
//!
//! - A name that is *both* a builtin and a script-local function is attributed to
//!   whichever the corpus defines. [`Census::defined`] holds every name the corpus
//!   itself defines and those are excluded, so the count is a lower bound on
//!   builtin calls.
//! - Control-flow keywords (`if (`, `while (`, `switch (`, …) look like calls and
//!   are filtered by an explicit keyword list.
//!
//! Neither affects the ordering of the top of the list, which is what the debt list
//! is for.

use crate::builtin_table::{find_builtin, BuiltinDecl};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Reserved words that are followed by `(` and are not calls.
const KEYWORDS: &[&str] = &[
    "if", "while", "for", "switch", "return", "else", "do", "case", "sizeof", "trigger", "struct",
    "static", "int", "real", "float", "string", "void", "group", "array", "anytype", "and", "or",
    "not", "include", "ref", "const", "break", "continue", "default", "true", "false",
];

/// Which `FuncSet` a registration index belongs to.
///
/// `ScriptGameInterface::init` (`0x009e1a20`) constructs the five sets in a fixed
/// order and each records `begin_func = funcs.count` before registering, so
/// registration order *is* the index space and these boundaries are exact.
/// [measured]
pub fn func_set(index: u32) -> &'static str {
    match index {
        0..=14 => "MathUtilFuncSet",
        15..=17 => "TriggerUtilFuncSet",
        18..=23 => "StringUtilFuncSet",
        24..=30 => "ArrayUtilFuncSet",
        _ => "ScenarioFuncSet",
    }
}

/// One builtin's measured share of the corpus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub index: u32,
    pub name: &'static str,
    pub calls: u64,
    /// How many distinct files call it.
    pub files: u32,
}

impl Row {
    pub fn func_set(&self) -> &'static str {
        func_set(self.index)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Census {
    /// Files scanned.
    pub files: Vec<PathBuf>,
    pub lines: u64,
    /// `ref` parameter tokens in shipped declarations. `ref` is only grammatical
    /// in a parameter list, so the lexical count over comment/string-stripped
    /// source is an exact reachability count rather than a call-shape estimate.
    pub ref_parameters: u64,
    /// Registered builtins, by call count descending then index ascending.
    pub builtins: Vec<Row>,
    /// Names the corpus defines itself (script functions), excluded from the counts.
    pub defined: BTreeSet<String>,
    /// Call-shaped identifiers that are neither a registered builtin nor defined in
    /// the corpus. These are the language's own statement forms plus any name this
    /// build does not register — worth eyeballing, not worth trusting.
    pub unresolved: BTreeMap<String, u64>,
}

impl Census {
    pub fn total_calls(&self) -> u64 {
        self.builtins.iter().map(|r| r.calls).sum()
    }

    pub fn distinct_called(&self) -> usize {
        self.builtins.len()
    }

    /// The debt list: everything called that `implemented` does not contain, in
    /// descending call order. This is what drives the order builtins get written in.
    pub fn owed<'a>(&'a self, implemented: &'a BTreeSet<u32>) -> impl Iterator<Item = &'a Row> {
        self.builtins
            .iter()
            .filter(move |r| !implemented.contains(&r.index))
    }

    /// Share of all measured builtin calls covered by `implemented`.
    pub fn coverage_fraction(&self, implemented: &BTreeSet<u32>) -> f64 {
        let total = self.total_calls();
        if total == 0 {
            return 1.0;
        }
        let hit: u64 = self
            .builtins
            .iter()
            .filter(|r| implemented.contains(&r.index))
            .map(|r| r.calls)
            .sum();
        hit as f64 / total as f64
    }

    pub fn report(&self, top: usize) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let _ = writeln!(
            s,
            "{} files, {} lines, {} distinct registered builtins called, {} calls",
            self.files.len(),
            self.lines,
            self.distinct_called(),
            self.total_calls()
        );
        for r in self.builtins.iter().take(top) {
            let _ = writeln!(
                s,
                "  {:>6}  {:<32} #{:<4} {:<20} in {} files",
                r.calls,
                r.name,
                r.index,
                r.func_set(),
                r.files
            );
        }
        s
    }
}

/// Strip `/* */` and `//` comments and the contents of string literals.
fn strip(src: &str) -> String {
    let b: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == '/' && i + 1 < b.len() && b[i + 1] == '*' {
            i += 2;
            while i + 1 < b.len() && !(b[i] == '*' && b[i + 1] == '/') {
                if b[i] == '\n' {
                    out.push('\n');
                }
                i += 1;
            }
            i = (i + 2).min(b.len());
        } else if c == '/' && i + 1 < b.len() && b[i + 1] == '/' {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
        } else if c == '"' {
            i += 1;
            while i < b.len() && b[i] != '"' {
                if b[i] == '\\' {
                    i += 1;
                }
                if i < b.len() && b[i] == '\n' {
                    out.push('\n');
                }
                i += 1;
            }
            i += 1;
            out.push_str("\"\"");
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}
fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn ident_count(src: &str, needle: &str) -> u64 {
    let b: Vec<char> = src.chars().collect();
    let mut count = 0;
    let mut i = 0;
    while i < b.len() {
        if is_ident_start(b[i]) && (i == 0 || !is_ident(b[i - 1])) {
            let start = i;
            while i < b.len() && is_ident(b[i]) {
                i += 1;
            }
            if b[start..i].iter().copied().eq(needle.chars()) {
                count += 1;
            }
        } else {
            i += 1;
        }
    }
    count
}

/// Every `identifier (` in `src`.
fn call_sites(src: &str) -> Vec<String> {
    let b: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if is_ident_start(b[i]) && (i == 0 || !is_ident(b[i - 1])) {
            let s = i;
            while i < b.len() && is_ident(b[i]) {
                i += 1;
            }
            let name: String = b[s..i].iter().collect();
            let mut j = i;
            while j < b.len() && (b[j] == ' ' || b[j] == '\t' || b[j] == '\n' || b[j] == '\r') {
                j += 1;
            }
            if j < b.len() && b[j] == '(' {
                out.push(name);
            }
        } else {
            i += 1;
        }
    }
    out
}

/// Names this source file *defines* as script functions: a return type, a name and
/// an open paren at the start of a line. Only used to exclude them from the counts.
fn definitions(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in src.lines() {
        let t = line.trim_start();
        // `trigger <name> ( ... ) { ... }` — a named trigger block. Its declaration
        // looks exactly like a call site, and `enable_trigger(name)` references it
        // by name, so both must be excluded.
        if let Some(rest) = t.strip_prefix("trigger ") {
            let rest = rest.trim_start();
            let name: String = rest.chars().take_while(|c| is_ident(*c)).collect();
            if !name.is_empty() {
                out.push(name);
                continue;
            }
        }
        let t = t.strip_prefix("static ").unwrap_or(t);
        for ty in [
            "int", "real", "float", "string", "String", "void", "group", "anytype",
        ] {
            if let Some(rest) = t.strip_prefix(ty) {
                let rest = rest.trim_start();
                // `int[] foo(` is a script too.
                let rest = rest
                    .strip_prefix("[]")
                    .map(|r| r.trim_start())
                    .unwrap_or(rest);
                let mut it = rest.chars();
                if let Some(c) = it.next() {
                    if is_ident_start(c) {
                        let name: String = rest.chars().take_while(|c| is_ident(*c)).collect();
                        let after = rest[name.len()..].trim_start();
                        if after.starts_with('(') {
                            out.push(name);
                        }
                    }
                }
                break;
            }
        }
    }
    out
}

/// Scan every `.bhs` file under `root`.
pub fn scan_dir(root: &Path) -> std::io::Result<Census> {
    let mut files = Vec::new();
    collect(root, &mut files)?;
    files.sort();
    let mut c = Census::default();
    let mut per_name: BTreeMap<String, (u64, u32)> = BTreeMap::new();
    let mut texts = Vec::with_capacity(files.len());
    for p in &files {
        let raw = std::fs::read(p)?;
        let src = String::from_utf8_lossy(&raw).into_owned();
        c.lines += src.lines().count() as u64;
        let s = strip(&src);
        c.ref_parameters += ident_count(&s, "ref");
        for d in definitions(&s) {
            c.defined.insert(d.to_ascii_lowercase());
        }
        texts.push(s);
    }
    for s in &texts {
        let mut seen_here: BTreeSet<String> = BTreeSet::new();
        for name in call_sites(s) {
            let lower = name.to_ascii_lowercase();
            if KEYWORDS.contains(&lower.as_str()) {
                continue;
            }
            let e = per_name.entry(lower.clone()).or_insert((0, 0));
            e.0 += 1;
            if seen_here.insert(lower) {
                e.1 += 1;
            }
        }
    }
    for (name, (calls, nfiles)) in per_name {
        if c.defined.contains(&name) {
            continue;
        }
        match find_builtin(&name) {
            Some(d) => c.builtins.push(Row {
                index: d.index,
                name: d.name,
                calls,
                files: nfiles,
            }),
            None => {
                c.unresolved.insert(name, calls);
            }
        }
    }
    c.builtins
        .sort_by(|a, b| b.calls.cmp(&a.calls).then(a.index.cmp(&b.index)));
    c.files = files;
    Ok(c)
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.is_dir() {
            collect(&p, out)?;
        } else if p
            .extension()
            .map(|x| x.eq_ignore_ascii_case("bhs"))
            .unwrap_or(false)
        {
            out.push(p);
        }
    }
    Ok(())
}

/// Locate `ron-data/bhs-corpus` from the crate directory. Returns `None` when the
/// (gitignored, copyrighted) corpus is not present, so tests skip rather than lie.
pub fn default_corpus() -> Option<PathBuf> {
    let mut d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for _ in 0..4 {
        let c = d.join("ron-data/bhs-corpus");
        if c.is_dir() {
            return Some(c);
        }
        d = d.parent()?.to_path_buf();
    }
    None
}

/// Convenience: the declaration behind a census row.
pub fn decl_of(r: &Row) -> Option<&'static BuiltinDecl> {
    crate::builtin_table::builtin(r.index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_comments_and_strings() {
        let s = strip("a(); /* b() */ // c()\n d(\"e()\");");
        assert!(s.contains("a("));
        assert!(!s.contains("b("));
        assert!(!s.contains("c("));
        assert!(s.contains("d("));
        assert!(!s.contains("e("));
    }

    #[test]
    fn finds_definitions_and_calls() {
        let src = "int my_script(int who)\n{\n  num_cities(who);\n  return my_script(1);\n}\n";
        let s = strip(src);
        assert_eq!(definitions(&s), vec!["my_script".to_string()]);
        let calls = call_sites(&s);
        assert!(calls.contains(&"num_cities".to_string()));
        assert!(calls.contains(&"my_script".to_string()));
    }
}
