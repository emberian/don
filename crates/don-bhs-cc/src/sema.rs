//! Semantic analysis: `include` resolution, struct layout, `labels` evaluation and the
//! script table.
//!
//! # How much type checking does BHS actually do?
//!
//! Very little, and the engine is the reason. A runtime value is a `ScriptType*` whose
//! `get_int` / `get_float` / `get_string` are *virtual coercions* and whose every
//! operator funnels through one virtual `do_operator(OpCodeTypes, ScriptType*)`. A
//! `ScriptString` handed to an `int` parameter converts; it does not fault. So declared
//! types are **allocation** information (they pick the `OP_CREATE_SIMPLE` type tag and
//! therefore the initial value and the coercion behaviour), not a checked contract.
//!
//! The corpus agrees, loudly: 51 parameters carry no type at all, `static
//! have_objective = false;` declares a variable with no type, and shipped scripts assign
//! strings and ints to the same untyped names. A compiler that rejected type mismatches
//! would reject the shipped campaign.
//!
//! What this module therefore checks is what the language really constrains:
//!
//! * every `include` resolves to a file on disk;
//! * every called name is either a registered engine builtin or a script declared in
//!   this compilation unit;
//! * a builtin call's **arity** matches one of that name's registrations (36 of the 873
//!   registrations are overloads distinguished by arity and parameter type);
//! * a `labels` entry does not collide with another in the same scope;
//! * a struct field name is unique within its struct.
//!
//! Everything else is reported as a *note*, never an error.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use don_bhs::builtin_table::{BuiltinDecl, BUILTINS};
use don_bhs::value::ScriptTy;

use crate::ast::*;
use crate::lex::Pos;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Note,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct Diag {
    pub severity: Severity,
    pub file: String,
    pub pos: Pos,
    pub msg: String,
}

impl std::fmt::Display for Diag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self.severity {
            Severity::Note => "note",
            Severity::Warning => "warning",
            Severity::Error => "error",
        };
        write!(f, "{}:{}: {}: {}", self.file, self.pos, s, self.msg)
    }
}

/// A BHS type after name resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    /// One of the engine's `SymType` scalars.
    Scalar(ScriptTy),
    /// A user `struct`, by index into [`Unit::structs`].
    Struct(usize),
    /// `T[]`.
    Array(Box<Ty>),
    /// A written type name that did not resolve. An omitted type is not represented by
    /// this variant: retail's grammar selects root `int` for that production.
    Untyped,
}

impl Ty {
    /// The `SymType` tag `OP_CREATE_SIMPLE` / `OP_CAST` take as their operand.
    pub fn tag(&self) -> u32 {
        match self {
            Ty::Scalar(s) => s.tag(),
            Ty::Array(inner) => match **inner {
                Ty::Scalar(ScriptTy::Str) => ScriptTy::StringArray.tag(),
                _ => ScriptTy::Array.tag(),
            },
            // A struct instance is an object to the engine.
            Ty::Struct(_) => ScriptTy::Any.tag(),
            Ty::Untyped => ScriptTy::Any.tag(),
        }
    }

    pub fn is_array(&self) -> bool {
        matches!(self, Ty::Array(_))
    }
}

/// Resolve a written type name. Case-insensitive: `String` and `string` both occur in
/// the corpus, as do `Int`/`int`. `real` and `float` are the same engine type
/// (`0x0012f35f`); `float` is what the corpus writes, `real` is what the builtin table
/// calls it.
pub fn scalar_from_name(name: &str) -> Option<ScriptTy> {
    Some(match name.to_ascii_lowercase().as_str() {
        "int" => ScriptTy::Int,
        // `SymTable::init_root` registers exactly FIVE built-in data types —
        // int, float, string, void, bool — in the block at `[0x00EBE408]`. `bool` has no
        // distinct `SymType` tag among the ten the builtin registrations use, so it maps
        // onto `int` here. The corpus never writes it. [measured that it exists]
        "bool" => ScriptTy::Int,
        "float" | "real" => ScriptTy::Real,
        "string" => ScriptTy::Str,
        "void" => ScriptTy::Void,
        "group" => ScriptTy::Group,
        "anytype" | "any" => ScriptTy::Any,
        "offer" => ScriptTy::Offer,
        _ => return None,
    })
}

#[derive(Debug, Clone)]
pub struct StructInfo {
    pub name: String,
    /// Field order **is** the layout: `OP_PUSH_STRUCT_FIELD` takes an index.
    pub fields: Vec<StructField>,
    pub declared_in: usize,
}

impl StructInfo {
    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields
            .iter()
            .position(|f| f.name.eq_ignore_ascii_case(name))
    }
}

#[derive(Debug, Clone)]
pub struct StructField {
    pub name: String,
    pub ty: Ty,
    /// `int grid[8];` — a fixed count. `None` means scalar or dynamic array.
    pub fixed_len: Option<i64>,
}

/// A script declared or defined somewhere in the compilation unit.
#[derive(Debug, Clone)]
pub struct ScriptInfo {
    pub name: String,
    /// Index into [`Unit::files`] of the file that *defines* it (or declares it, if no
    /// definition exists anywhere in the unit).
    pub file: usize,
    /// Index within that file's script list — the operand of `OP_CALL`.
    pub index_in_file: usize,
    pub arity: usize,
    pub ret: Ty,
    pub by_ref: Vec<bool>,
    pub script_type: Option<String>,
    pub has_body: bool,
}

/// One source file in the compilation unit.
#[derive(Debug)]
pub struct FileUnit {
    pub path: PathBuf,
    pub ast: SourceFile,
    /// Indices into [`Unit::files`], in `include` order. This is
    /// `ScriptFile::linked_files`, and the position here is the `OP_CALL_INCLUDE` file
    /// operand.
    pub includes: Vec<usize>,
    /// Names of this file's scripts in declaration order — the `OP_CALL` operand space.
    pub script_names: Vec<String>,
}

/// A whole compilation unit: a root `.bhs` plus everything it transitively includes.
#[derive(Debug)]
pub struct Unit {
    pub files: Vec<FileUnit>,
    pub structs: Vec<StructInfo>,
    /// File-scope `labels` constants, merged across the include graph.
    pub labels: HashMap<String, i64>,
    pub scripts: Vec<ScriptInfo>,
    pub diags: Vec<Diag>,
}

impl Unit {
    pub fn errors(&self) -> impl Iterator<Item = &Diag> {
        self.diags.iter().filter(|d| d.severity == Severity::Error)
    }

    pub fn struct_by_name(&self, name: &str) -> Option<usize> {
        self.structs
            .iter()
            .position(|s| s.name.eq_ignore_ascii_case(name))
    }

    pub fn script_by_name(&self, name: &str) -> Option<&ScriptInfo> {
        self.scripts
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
    }

    pub fn resolve_type(&self, t: Option<&TypeRef>) -> Ty {
        // The grammar's default-type reduction loads root[0] (`int`) before it creates
        // an untyped VarType or LocalScriptType. This covers omitted local/parameter
        // types and omitted script returns. [measured: yyparse 0x009bafcb-0x009bafe3]
        let Some(t) = t else {
            return Ty::Scalar(ScriptTy::Int);
        };
        let base = match scalar_from_name(&t.name) {
            Some(s) => Ty::Scalar(s),
            None => match self.struct_by_name(&t.name) {
                Some(i) => Ty::Struct(i),
                None => Ty::Untyped,
            },
        };
        let mut ty = base;
        for _ in 0..t.array_depth {
            ty = Ty::Array(Box::new(ty));
        }
        ty
    }
}

/// Retail's default `Lexer::include_paths` entry.
///
/// `Lexer::init_once` `0x009c0f90` reads
/// `prefs_get(int_str[7181], int_str[7182], 1, 0)` — `0x009c106a..0x009c1088`, with
/// `int_str_array` at `0x00c06378` and the 20-byte stride giving ordinals
/// `0x23104/0x14 = 7181` and `0x23118/0x14 = 7182`. Those ordinals in the shipped
/// `internal_strings.xml` are the preference key `ScriptIncludePath` and this default
/// value. The result is tokenised on the separator set `",;"` (wide literal at
/// `0xb04b1c`) by `TokenString::next` `0x00a44000` and each token is appended to
/// `Lexer::include_paths` (`Lexer+0xd0`, PDB member name) at `0x009c1160..0x009c11ae`.
/// [measured]
pub const DEFAULT_SCRIPT_INCLUDE_PATH: &str = ".\\scenario\\scriptlibrary\\";

/// The separator set `Lexer::init_once` splits `ScriptIncludePath` on. [measured,
/// wide literal `0xb04b1c`]
pub const SCRIPT_INCLUDE_PATH_SEPARATORS: &[char] = &[',', ';'];

/// `Lexer::start_include_file` `0x009c1960` refuses at `Lexer::file_stack` depth 16
/// (`cmp dword ptr [esi + 0x124], 0x10` at `0x009c19a6`; `file_stack` is PDB offset 284
/// and `+0x124` is its count word). Retail reports the nesting error and does not open
/// the file. [measured]
pub const MAX_INCLUDE_DEPTH: usize = 16;

/// The `String::prepend_content_dir` `0x00A1D690` boundary, as seen by the lexer.
///
/// Retail turns every candidate path into a real file by prepending the content
/// directory and running it through `ModManager::calcFilePath` `0x00A22910`, then
/// calling `_wfsopen(path, L"rb", _SH_DENYNO)`. This trait is that one step: given a
/// game-relative, `\`- or `/`-separated path, produce the file that would actually be
/// opened, or `None` when nothing opens.
///
/// `don-content`'s `ContentStack` is the derived implementation of the mod-stack half;
/// [`InstallRoot`] is the no-mods case.
pub trait ContentProbe: std::fmt::Debug + Send + Sync {
    fn probe(&self, game_relative: &str) -> Option<PathBuf>;
}

/// A plain install tree with no mods installed: the shipped-data branch of
/// `calcFilePath`, where `out_index` is 0 and the returned path is the request itself.
///
/// The segment walk is case-insensitive because the shipped tree genuinely mixes case
/// (`Cliffs.xml`, `IME.xml`, `scenario/Chess Exercise/`) and retail opens through the
/// Win32 case-insensitive filesystem; a case-sensitive host must fold explicitly rather
/// than lower-casing the path, which would not exist on disk.
#[derive(Debug, Clone)]
pub struct InstallRoot(pub PathBuf);

impl ContentProbe for InstallRoot {
    fn probe(&self, game_relative: &str) -> Option<PathBuf> {
        probe_under(&self.0, game_relative)
    }
}

/// Several install roots tried in order. Retail has exactly one content directory; this
/// is the harness/testing generalisation and is *not* a retail behaviour.
#[derive(Debug, Clone, Default)]
pub struct InstallRoots(pub Vec<PathBuf>);

impl ContentProbe for InstallRoots {
    fn probe(&self, game_relative: &str) -> Option<PathBuf> {
        self.0.iter().find_map(|r| probe_under(r, game_relative))
    }
}

/// Split a retail game-relative path into segments, dropping `.` and empty segments.
/// Both separators are accepted because the shipped preference value uses `\` while
/// every other path in this crate uses `/`.
fn path_segments(game_relative: &str) -> Vec<&str> {
    game_relative
        .split(['\\', '/'])
        .filter(|s| !s.is_empty() && *s != ".")
        .collect()
}

/// Walk `root` segment by segment, matching each segment case-insensitively. Returns the
/// concrete file only when the whole path lands on a regular file.
fn probe_under(root: &Path, game_relative: &str) -> Option<PathBuf> {
    let segs = path_segments(game_relative);
    if segs.is_empty() {
        return None;
    }
    let mut at = root.to_path_buf();
    for (i, seg) in segs.iter().enumerate() {
        let last = i + 1 == segs.len();
        let direct = at.join(seg);
        let exists = if last {
            direct.is_file()
        } else {
            direct.is_dir()
        };
        if exists {
            at = direct;
            continue;
        }
        // Case-insensitive fallback for hosts whose filesystem is case-sensitive.
        let mut found = None;
        let entries = std::fs::read_dir(&at).ok()?;
        for e in entries.flatten() {
            let name = e.file_name();
            let Some(name) = name.to_str() else { continue };
            if !name.eq_ignore_ascii_case(seg) {
                continue;
            }
            let p = e.path();
            let ok = if last { p.is_file() } else { p.is_dir() };
            if ok {
                found = Some(p);
                break;
            }
        }
        at = found?;
    }
    Some(at)
}

/// Where to look for a root script and for an `include`d file.
///
/// This is `Lexer::open_file` `0x009bff30`, read at the instruction level. Given the
/// requested name it tries, in this order, and takes the first that opens:
///
/// 1. **the including file's own directory** — only when there *is* a current file
///    (`Lexer+0x14`, the `yyFlexLexer` current `LexerFileEntry`), i.e. for `include`
///    and not for the root: `current_entry->path.get_directory()`
///    (`String::get_directory` `0x00a1dcd0`) `+= name`, then `prepend_content_dir`
///    (`0x009bffcf..0x009c0079`);
/// 2. **the bare name against the content directory** (`0x009c0090..0x009c00f7`);
/// 3. **each `Lexer::include_paths` entry** `+ name`, in array order
///    (`0x009c010a..0x009c01d3`).
///
/// Nothing else is searched. In particular retail does **not** scan the install tree for
/// a matching basename; a name that none of the three candidates resolves is a compile
/// error (`Lexer::open_file` returns 1, `Lexer::start_include_file` `0x009c1960` reports
/// it through `Compiler::comp_error`). An earlier version of this crate did scan by
/// basename, which accepts programs retail rejects and can pick a different file than
/// retail when two trees hold the same name.
#[derive(Debug, Clone)]
pub struct IncludePath {
    /// The `prepend_content_dir` boundary.
    pub content: std::sync::Arc<dyn ContentProbe>,
    /// `Lexer::include_paths`, in array order, game-relative.
    pub include_paths: Vec<String>,
    /// `Lexer::file_stack` depth cap.
    pub max_depth: usize,
}

impl Default for IncludePath {
    fn default() -> Self {
        IncludePath {
            content: std::sync::Arc::new(InstallRoots::default()),
            include_paths: vec![DEFAULT_SCRIPT_INCLUDE_PATH.to_string()],
            max_depth: MAX_INCLUDE_DEPTH,
        }
    }
}

impl IncludePath {
    /// One or more install roots with retail's default `ScriptIncludePath`.
    pub fn with_roots<I: IntoIterator<Item = PathBuf>>(roots: I) -> Self {
        IncludePath {
            content: std::sync::Arc::new(InstallRoots(roots.into_iter().collect())),
            ..IncludePath::default()
        }
    }

    /// Resolve against an arbitrary content probe — the `don-content` mod stack, for
    /// instance.
    pub fn with_content(content: std::sync::Arc<dyn ContentProbe>) -> Self {
        IncludePath {
            content,
            ..IncludePath::default()
        }
    }

    /// Replace `Lexer::include_paths`, e.g. from a `ScriptIncludePath` preference value.
    /// The value is tokenised exactly as `Lexer::init_once` tokenises it.
    pub fn with_include_path_pref(mut self, pref: &str) -> Self {
        self.include_paths = pref
            .split(SCRIPT_INCLUDE_PATH_SEPARATORS)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        self
    }

    /// Candidate 2 and 3 only: the root-file case, where `Lexer+0x14` is null because
    /// `Compiler::compile` `0x009bf160` calls `open_file` before any file is current.
    pub fn resolve_root(&self, name: &str) -> Option<PathBuf> {
        self.resolve_from(None, name)
    }

    /// The full three-candidate search. `from` is the *resolved* path of the including
    /// file, which is what retail stores in the `LexerFileEntry` and what
    /// `String::get_directory` is applied to.
    pub fn resolve_from(&self, from: Option<&Path>, name: &str) -> Option<PathBuf> {
        // 1. the including file's directory. Retail builds this candidate as a plain
        //    string join and *then* prepends the content dir; because the entry's stored
        //    path is already content-resolved, the join is done here on the real path.
        if let Some(dir) = from.and_then(Path::parent) {
            if let Some(p) = probe_under(dir, name) {
                return Some(p);
            }
        }
        // 2. the bare name against the content directory.
        if let Some(p) = self.content.probe(name) {
            return Some(p);
        }
        // 3. each include path entry, in array order.
        for d in &self.include_paths {
            let joined = format!("{}/{}", d.trim_end_matches(['\\', '/']), name);
            if let Some(p) = self.content.probe(&joined) {
                return Some(p);
            }
        }
        None
    }
}

/// Build a compilation unit from a root file.
pub fn analyze(root: &Path, inc: &IncludePath) -> Result<Unit, crate::CompileError> {
    let mut u = Unit {
        files: Vec::new(),
        structs: Vec::new(),
        labels: HashMap::new(),
        scripts: Vec::new(),
        diags: Vec::new(),
    };
    let mut seen: HashMap<PathBuf, usize> = HashMap::new();
    load(root, inc, 0, &mut u, &mut seen)?;

    collect_structs(&mut u);
    collect_labels(&mut u);
    collect_scripts(&mut u);
    Ok(u)
}

fn canon(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

fn load(
    path: &Path,
    inc: &IncludePath,
    depth: usize,
    u: &mut Unit,
    seen: &mut HashMap<PathBuf, usize>,
) -> Result<usize, crate::CompileError> {
    let key = canon(path);
    if let Some(i) = seen.get(&key) {
        return Ok(*i);
    }
    let ast = crate::parse_path(path)?;
    let idx = u.files.len();
    // Insert *before* recursing so an include cycle terminates.
    seen.insert(key, idx);
    u.files.push(FileUnit {
        path: path.to_path_buf(),
        ast,
        includes: Vec::new(),
        script_names: Vec::new(),
    });

    let includes: Vec<(String, Pos)> = u.files[idx]
        .ast
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Include { path, pos } => Some((path.clone(), *pos)),
            _ => None,
        })
        .collect();

    for (name, pos) in includes {
        // `Lexer::start_include_file` `0x009c1960` checks `Lexer::file_stack`'s count
        // *before* calling `open_file`, so an over-deep include is never opened.
        //
        // The count is the number of *suspended* files, not including the current one:
        // `Lexer::set_curr_buffer` `0x009bfeb0` pushes the outgoing `Lexer+0x14` and only
        // then installs the new entry, and it pushes nothing when there is no outgoing
        // file — so the root sits at count 0. `file_stack` is `{data +0x11c, cap +0x120,
        // count +0x124, grow +0x128}`, read off the `cmp eax, [edi+0x120]` / `inc dword
        // [esi+8]` pair at `0x009bfec2..0x009bfee8`. `depth` here is that same count.
        if depth >= inc.max_depth {
            u.diags.push(Diag {
                severity: Severity::Error,
                file: path.display().to_string(),
                pos,
                msg: format!(
                    "`include \"{name}\"` exceeds the retail nesting limit of {}",
                    inc.max_depth
                ),
            });
            continue;
        }
        match inc.resolve_from(Some(path), &name) {
            Some(p) => {
                // Retail's re-inclusion behaviour is an open question, recorded rather
                // than assumed. `Lexer::open_file`'s `included_files` guard
                // (`0x009bffa0..0x009bffb4`) compares each already-open entry's path
                // against a *freshly zero-initialised* local `String`, so as emitted it
                // can only fire for an entry whose stored path is empty — which cannot
                // happen, since the entry's path is assigned from the resolved candidate
                // at `0x009c020f`. Retail therefore appears to lex a diamond-included
                // file twice. This compiler processes it once; the divergence is real for
                // five shipped roots (all of them reaching `game_structs.bhs` twice
                // through `ctw_lib.bhs`) and is settled by one reference image from the
                // retail compiler, not by choosing here.
                let already = seen.contains_key(&canon(&p));
                let child = load(&p, inc, depth + 1, u, seen)?;
                if already {
                    u.diags.push(Diag {
                        severity: Severity::Note,
                        file: path.display().to_string(),
                        pos,
                        msg: format!(
                            "`include \"{name}\"` is already in this unit; retail's \
                             `included_files` guard cannot fire, so retail may lex it again"
                        ),
                    });
                }
                u.files[idx].includes.push(child);
            }
            None => u.diags.push(Diag {
                severity: Severity::Error,
                file: path.display().to_string(),
                pos,
                msg: format!(
                    "cannot resolve `include \"{name}\"`: no candidate of \
                     `Lexer::open_file` 0x009bff30 opens"
                ),
            }),
        }
    }
    Ok(idx)
}

fn collect_structs(u: &mut Unit) {
    let mut defs: Vec<(usize, StructDef)> = Vec::new();
    for (fi, f) in u.files.iter().enumerate() {
        for it in &f.ast.items {
            match it {
                Item::Struct(s) => defs.push((fi, s.clone())),
                Item::Script(ScriptDef { body: Some(b), .. }) => {
                    collect_nested_structs(fi, &b.stmts, &mut defs)
                }
                Item::Main(m) => collect_nested_structs(fi, &m.body.stmts, &mut defs),
                _ => {}
            }
        }
    }
    // Two passes: register names first so a struct may reference another declared later.
    for (fi, d) in &defs {
        if u.structs
            .iter()
            .any(|s| s.name.eq_ignore_ascii_case(&d.name))
        {
            continue;
        }
        u.structs.push(StructInfo {
            name: d.name.clone(),
            fields: Vec::new(),
            declared_in: *fi,
        });
    }
    for (fi, d) in &defs {
        let Some(si) = u.struct_by_name(&d.name) else {
            continue;
        };
        if !u.structs[si].fields.is_empty() {
            continue;
        }
        let mut fields: Vec<StructField> = Vec::new();
        for f in &d.fields {
            if fields.iter().any(|x| x.name.eq_ignore_ascii_case(&f.name)) {
                u.diags.push(Diag {
                    severity: Severity::Error,
                    file: u.files[*fi].path.display().to_string(),
                    pos: f.pos,
                    msg: format!("duplicate field `{}` in struct `{}`", f.name, d.name),
                });
                continue;
            }
            let ty = u.resolve_type(Some(&f.ty));
            let fixed_len = match &f.fixed_len {
                Some(Expr::Int(n, _)) => Some(*n),
                Some(e) => {
                    u.diags.push(Diag {
                        severity: Severity::Error,
                        file: u.files[*fi].path.display().to_string(),
                        pos: e.pos(),
                        msg: "struct field array length is not a literal; dynamic substitution is not recovered"
                            .into(),
                    });
                    None
                }
                None => None,
            };
            fields.push(StructField {
                name: f.name.clone(),
                ty,
                fixed_len,
            });
        }
        u.structs[si].fields = fields;
    }
}

fn collect_nested_structs(fi: usize, stmts: &[Stmt], out: &mut Vec<(usize, StructDef)>) {
    for s in stmts {
        if let Stmt::Struct(d) = s {
            out.push((fi, d.clone()));
        }
    }
}

/// `labels { A = 1, B, C }`.
///
/// **Auto-numbering starts at 1 and each unvalued entry is the previous plus one.**
/// This is not a guess. Ten shipped blocks mix implicit and explicit entries and every
/// one reads as a player-number table whose first entries are the playable sides:
/// `AMERICANS, COLOMBIANS, REBELS = 7` (`colombiaruntime.bhs`), `MACEDONIANS, PERSIANS,
/// BARON1 = 8, …` (`sogdiana.bhs`), `PLAYER, PORTUGUESE, FRENCH = 8` (`portsetup.bhs`).
/// In `conquest/ColdWar/skirmishsetup.bhs` the block `labels { ATTACKER, DEFENDER }` is
/// immediately followed by `for (i = 1; i < 3; i++) gain_tech(i, …)` granting the same
/// two sides the same techs that lines 65-68 grant to `ATTACKER` and `DEFENDER` — so
/// `ATTACKER == 1`. Zero-based numbering would make every one of those tables address
/// RoN player 0, which is Gaia.
/// [inferred, from the corpus; not yet read out of the retail compiler]
pub fn eval_labels(
    defs: &[LabelDef],
    into: &mut HashMap<String, i64>,
    diags: &mut Vec<Diag>,
    file: &str,
) {
    let mut next = 1i64;
    for d in defs {
        let v = match &d.value {
            Some(e) => match const_int(e, into) {
                Some(v) => v,
                None => {
                    diags.push(Diag {
                        severity: Severity::Error,
                        file: file.to_string(),
                        pos: d.pos,
                        msg: format!("`labels` entry `{}` is not a constant expression", d.name),
                    });
                    next
                }
            },
            None => next,
        };
        if let Some(prev) = into.insert(d.name.clone(), v) {
            if prev != v {
                diags.push(Diag {
                    severity: Severity::Error,
                    file: file.to_string(),
                    pos: d.pos,
                    msg: format!("label `{}` redefined ({prev} -> {v})", d.name),
                });
            }
        }
        next = v + 1;
    }
}

/// Constant folding, limited to what a `labels` entry may contain.
pub fn const_int(e: &Expr, env: &HashMap<String, i64>) -> Option<i64> {
    use crate::lex::P;
    Some(match e {
        Expr::Int(v, _) => *v,
        Expr::Name(n, _) => *env.get(n)?,
        Expr::Unary {
            op: P::Minus, expr, ..
        } => -const_int(expr, env)?,
        Expr::Unary {
            op: P::Not, expr, ..
        } => i64::from(const_int(expr, env)? == 0),
        Expr::Unary {
            op: P::Tilde, expr, ..
        } => !const_int(expr, env)?,
        Expr::Binary { op, lhs, rhs, .. } => {
            let a = const_int(lhs, env)?;
            let b = const_int(rhs, env)?;
            match op {
                P::Plus => a + b,
                P::Minus => a - b,
                P::Star => a * b,
                P::Slash if b != 0 => a / b,
                P::Percent if b != 0 => a % b,
                P::Shl => a << b,
                P::Shr => a >> b,
                P::Amp => a & b,
                P::Pipe => a | b,
                P::Caret => a ^ b,
                _ => return None,
            }
        }
        _ => return None,
    })
}

fn collect_labels(u: &mut Unit) {
    let mut labels = std::mem::take(&mut u.labels);
    let mut diags = std::mem::take(&mut u.diags);
    for f in &u.files {
        let path = f.path.display().to_string();
        for it in &f.ast.items {
            if let Item::Labels { defs, .. } = it {
                eval_labels(defs, &mut labels, &mut diags, &path);
            }
        }
    }
    u.labels = labels;
    u.diags = diags;
}

fn collect_scripts(u: &mut Unit) {
    // Pass 1: index every named script per file. This ordering is the `OP_CALL` operand
    // space, so it must be stable and per-file.
    for fi in 0..u.files.len() {
        let items = std::mem::take(&mut u.files[fi].ast.items);
        let mut names: Vec<String> = Vec::new();
        for it in &items {
            match it {
                Item::Script(s) => {
                    if !names.iter().any(|n| n.eq_ignore_ascii_case(&s.sig.name)) {
                        names.push(s.sig.name.clone());
                    }
                }
                Item::Main(_) => {
                    // The anonymous per-frame entry script. `Game::do_frame` calls
                    // `run_script(<name>, 0)` and attachment is per file, so we key it on
                    // the file stem. [inferred — the retail name is unread]
                    let n = entry_name(&u.files[fi].path);
                    if !names.iter().any(|x| x.eq_ignore_ascii_case(&n)) {
                        names.push(n);
                    }
                }
                _ => {}
            }
        }
        u.files[fi].ast.items = items;
        u.files[fi].script_names = names;
    }

    // Pass 2: build the global table, definitions winning over forward declarations.
    let mut scripts: Vec<ScriptInfo> = Vec::new();
    let mut diags = std::mem::take(&mut u.diags);
    for fi in 0..u.files.len() {
        let path = u.files[fi].path.display().to_string();
        let items: Vec<Item> = u.files[fi].ast.items.clone();
        for it in &items {
            let (name, params, ret, script_type, has_body, pos) = match it {
                Item::Script(s) => (
                    s.sig.name.clone(),
                    s.sig.params.clone(),
                    u.resolve_type(s.sig.ret.as_ref()),
                    s.sig.script_type.clone(),
                    s.body.is_some(),
                    s.sig.pos,
                ),
                Item::Main(m) => (
                    entry_name(&u.files[fi].path),
                    Vec::new(),
                    u.resolve_type(m.ret.as_ref()),
                    Some(m.script_type.clone()),
                    true,
                    m.pos,
                ),
                _ => continue,
            };
            let index_in_file = u.files[fi]
                .script_names
                .iter()
                .position(|n| n.eq_ignore_ascii_case(&name))
                .unwrap_or(0);
            let info = ScriptInfo {
                name: name.clone(),
                file: fi,
                index_in_file,
                arity: params.len(),
                ret,
                by_ref: params.iter().map(|p| p.by_ref).collect(),
                script_type,
                has_body,
            };
            match scripts
                .iter_mut()
                .find(|s| s.name.eq_ignore_ascii_case(&name))
            {
                Some(prev) => {
                    if prev.has_body && has_body {
                        diags.push(Diag {
                            severity: Severity::Error,
                            file: path.clone(),
                            pos,
                            msg: format!("script `{name}` defined more than once"),
                        });
                    } else if !prev.has_body {
                        *prev = info;
                    }
                }
                None => scripts.push(info),
            }
        }
    }
    u.scripts = scripts;
    u.diags = diags;
}

/// The name we give a file's anonymous entry script.
pub fn entry_name(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "main".into())
}

/// Every registration of a builtin name, in index order.
pub fn builtin_overloads(name: &str) -> Vec<&'static BuiltinDecl> {
    BUILTINS
        .iter()
        .filter(|b| b.name.eq_ignore_ascii_case(name))
        .collect()
}

/// Pick the registration a call resolves to.
///
/// Resolution is by **arity first**. `ScriptFuncSet::call_func` stops parameter
/// validation at a `params` (varargs) tag, so a declaration whose last parameter carries
/// that tag accepts any count at or above its fixed prefix — `parse(fmt, …)` is the only
/// such registration among the 873.
pub fn resolve_builtin(name: &str, argc: usize) -> Option<&'static BuiltinDecl> {
    let cands = builtin_overloads(name);
    if cands.is_empty() {
        return None;
    }
    if let Some(d) = cands.iter().find(|d| d.arity as usize == argc) {
        return Some(d);
    }
    if let Some(d) = cands
        .iter()
        .find(|d| d.params.last() == Some(&ScriptTy::Params) && argc + 1 >= d.arity as usize)
    {
        return Some(d);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_number_from_one() {
        let defs = crate::parse_file("<t>", "labels { A, B, C = 7, D }")
            .unwrap()
            .items
            .into_iter()
            .find_map(|i| match i {
                Item::Labels { defs, .. } => Some(defs),
                _ => None,
            })
            .unwrap();
        let mut env = HashMap::new();
        let mut d = Vec::new();
        eval_labels(&defs, &mut env, &mut d, "<t>");
        assert_eq!(env["A"], 1);
        assert_eq!(env["B"], 2);
        assert_eq!(env["C"], 7);
        assert_eq!(env["D"], 8);
        assert!(d.is_empty());
    }

    #[test]
    fn builtin_resolution_is_by_arity() {
        // `add` is registered twice: (array) and (array, anytype).
        assert_eq!(resolve_builtin("add", 1).unwrap().arity, 1);
        assert_eq!(resolve_builtin("add", 2).unwrap().arity, 2);
        assert!(resolve_builtin("add", 9).is_none());
    }

    #[test]
    fn parse_builtin_is_varargs() {
        // `parse(String fmt, params)` — arity 2, but shipped calls pass 1..6 arguments.
        for n in 1..7 {
            assert_eq!(resolve_builtin("parse", n).unwrap().name, "parse");
        }
    }
}
