// SPDX-License-Identifier: GPL-3.0-or-later
//! Loading a **shipped** script: from a game-relative path to a runnable
//! [`don_bhs::Program`] plus the entry binding the retail tick would call.
//!
//! Everything before this module operated on a `.bhs` file the caller had already found.
//! Retail does not work that way: a script name arrives from the setup screen, a scenario
//! file, or `ScenarioData::general_powers_script_file`, and the engine turns it into a
//! compiled `ScriptFile` through a fixed sequence that this module reproduces.
//!
//! # The sequence, as read
//!
//! `Compiler::compile(const String& path, ScriptReloadType reload)` `0x009bf160`:
//!
//! 1. `if (path == EMPTY_STRING) return -1;` (`0x009bf186..0x009bf199`). An empty script
//!    name is not "no script"; it is a failed compile.
//! 2. `path.prepend_content_dir(0, "script\\compiler.cpp", 100)` `0x00A1D690`
//!    (`0x009bf1f5..0x009bf206`) — the mod-stack resolution reproduced by
//!    `don_content::ContentStack`.
//! 3. `path.check_ext(L"bhs", 1)` `0x00a1bce0` (`0x009bf248..0x009bf263`) — see
//!    [`check_ext`].
//! 4. `ScriptFile::find_script_file(path)` `0x009c6a10`; if the file is already loaded and
//!    `ScriptFile::has_changed()` `0x009c4a70` is 0, **it is not recompiled** and
//!    `compile` returns 0 (`0x009bf2cb..0x009bf308`).
//! 5. otherwise `Lexer::open_file(path)` `0x009bff30` on the global `Lexer` at
//!    `0x00ebe380` (`0x009bf4d0..0x009bf4e3`), then `yyparse` `0x009ba430`.
//!
//! # What the tick actually calls
//!
//! `Game::do_frame` step 4 calls the varargs thunk `RunTimeEnv::run_script` `0x0043d0e0`,
//! which is 32 bytes of forwarding into `RunTimeEnv::run_script(const String& script_name,
//! const String& file_name, ScriptRunMode, int nargs, char* va)` `0x009c4460` with
//! `file_name = EMPTY_STRING`. Read at `0x009c4460`:
//!
//! * `if (file_name.curr_len != 0) Compiler::compile(file_name, 0);` — so a *named-file*
//!   call compiles on demand, and the step-4 thunk, which passes the empty string, **does
//!   not**. The program must already be loaded when the tick runs.
//! * `ScriptFile::find_script(script_name, &file_index)` `0x009c6c80` searches **every
//!   loaded `ScriptFile`** for a script of that name and yields the owning file index.
//!   Binding is by script name across the whole loaded set, not by (file, name).
//! * a missing script is not silent: it calls `RunTimeEnv::run_time_error` `0x009c31e0`
//!   and returns 3, which is the same `script_status = 3` an in-script runtime error
//!   sets.
//!
//! # Tier
//!
//! Tier C. Every address above was read off `ron-bin/riseofnations.exe` (PE32 i386, image
//! base `0x00400000`) with Capstone, and the names come from the matching private PDB. No
//! byte of this module's output has been compared against the retail compiler, and no part
//! of it has been executed against retail machine code.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use don_bhs::Program;

use crate::sema::{self, IncludePath, Severity};

/// `ScenarioData::general_powers_script_file`, the second script retail runs at tick step
/// 4 (and only when `Game::frame > 0`).
///
/// `ScenarioFuncSet::init` `0x00a03c30` assigns it from the runtime string table:
/// `mov eax, [0x00c06378]; mov eax, [eax+0x10]; add eax, 0x1d178` at
/// `0x00a04014..0x00a04021`, and `0x1d178 / 0x14` is `internal_strings.xml` ordinal 5958,
/// whose value in the shipped file is exactly this string. [measured; the ordinal
/// derivation is `docs/assembly/scenario-initial-state.md` §7]
pub const GENERAL_POWERS_SCRIPT_FILE: &str = "./scenario/scriptlibrary/general_powers.bhs";

/// The extension `Compiler::compile` forces onto every script path. [measured, wide
/// literal `0xb04758`, pushed at `0x009bf248`]
pub const SCRIPT_EXT: &str = "bhs";

/// `String::check_ext(const String& exts, int force)` `0x00a1bce0`, specialised to the
/// single-token call `Compiler::compile` makes.
///
/// Read at the instruction level, the shape that matters here is:
///
/// * the "already has it" test only runs when `curr_len >= 4` and the character at
///   `curr_len - 4` is `'.'` (`0x00a1be54..0x00a1beab`). The `4` is a literal, so the test
///   is hard-wired to **three-character** extensions;
/// * if that three-character extension equals the token (case-insensitively, through
///   `String::operator==` `0x00a1f140`, whose tail is `_wcsicmp`), the string is returned
///   unchanged (`0x00a1bf17..0x00a1bf1e`, then `esi = 1`);
/// * otherwise, with `force != 0`, the existing three-character extension is **truncated**
///   (`0x00a1c12e: push ebx; call String::truncate 0x00a1afb0`, where `ebx = curr_len - 4`)
///   and `"." + token` is appended (`0x00a1c156..0x00a1c168`);
/// * a name with no three-character extension simply gets `"." + token` appended.
///
/// So `general_powers` becomes `general_powers.bhs`, `editor_scratch.svx` becomes
/// `editor_scratch.bhs`, and `a.jpeg` becomes `a.jpeg.bhs` — the last because `'.'` is not
/// at `curr_len - 4`. That last case is retail behaviour, not an oversight here.
///
/// **Not reproduced:** the general multi-token form. `check_ext` tokenises `exts` on `' '`
/// (`TokenString` over the wide literal at `0xb13f18`) and loops; the two extra literals it
/// can append (`0xb13f98`, `0xb13f9c`) were not decoded. Every call this crate makes is the
/// single-token `L"bhs"` form, and [`check_ext`] panics rather than guessing if handed a
/// token containing a space.
pub fn check_ext(path: &str, ext: &str) -> String {
    assert!(
        !ext.contains(' ') && !ext.is_empty(),
        "only the single-token form of String::check_ext 0x00a1bce0 is recovered"
    );
    let chars: Vec<char> = path.chars().collect();
    if chars.len() >= 4 && chars[chars.len() - 4] == '.' {
        let have: String = chars[chars.len() - 3..].iter().collect();
        if have.eq_ignore_ascii_case(ext) {
            return path.to_string();
        }
        let stem: String = chars[..chars.len() - 4].iter().collect();
        return format!("{stem}.{ext}");
    }
    format!("{path}.{ext}")
}

/// A shipped script compiled and ready for the persistent runtime.
#[derive(Debug)]
pub struct LoadedScript {
    /// The concrete file the content stack opened for the root script.
    pub root_path: PathBuf,
    /// The path after step 1-3 of `Compiler::compile`, i.e. what retail would have stored
    /// as `ScriptFile::source_file`.
    pub resolved_name: String,
    /// The compiled image. One `ScriptFile` per compilation unit, as retail produces.
    pub program: Program,
    /// The zero-argument entry script `Game::do_frame` would name for this file.
    ///
    /// `SetupWin::set_script_path` `0x005b7aa0` stores the selected file's base name with
    /// the extension stripped in `Game+0x500`, and `Game::do_frame` `0x00591ef0` passes
    /// that same string to `run_script`. So the entry name is the file stem.
    pub entry: String,
    /// Non-error diagnostics worth surfacing; errors never reach here.
    pub diags: Vec<sema::Diag>,
}

impl LoadedScript {
    /// Index of [`Self::entry`] in `program.files[0].scripts`, if it exists with arity 0.
    ///
    /// A step-4 binding must be zero-argument: `RunTimeEnv::run_script` `0x0043d0e0` is
    /// called with `nargs = 0` from `Game::do_frame`, and
    /// `RunTimeEnv::setup_params` rejects a mismatch with status 4.
    pub fn tick_entry_index(&self) -> Option<usize> {
        let f = self.program.files.first()?;
        f.scripts
            .iter()
            .position(|s| s.name.eq_ignore_ascii_case(&self.entry) && s.arity == 0)
    }
}

/// Why a shipped script did not become a runnable program.
#[derive(Debug)]
pub enum LoadError {
    /// `Compiler::compile` `0x009bf160` returns -1 for an empty path before doing anything
    /// else.
    EmptyPath,
    /// No candidate of `Lexer::open_file` `0x009bff30` opened.
    NotFound { resolved_name: String },
    /// Lex/parse failure, or an I/O failure reading a file the probe said exists.
    Compile(crate::CompileError),
    /// Semantic or codegen errors. Retail's `Compiler::comp_error` `0x009bed80` counts
    /// these and `compile` reports the total; a nonzero count means no usable image.
    Diagnostics(Vec<sema::Diag>),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::EmptyPath => write!(f, "empty script path"),
            LoadError::NotFound { resolved_name } => {
                write!(f, "no content path opens for `{resolved_name}`")
            }
            LoadError::Compile(e) => write!(f, "{e}"),
            LoadError::Diagnostics(d) => {
                for x in d {
                    writeln!(f, "{x}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for LoadError {}

/// Load a script the way retail loads one: game-relative path in, compiled image out.
///
/// `path` is what the engine would have in the `String` it passes to `Compiler::compile` —
/// the scenario's script name, `Game+0x500`, or [`GENERAL_POWERS_SCRIPT_FILE`]. It may or
/// may not carry the `.bhs` extension; [`check_ext`] settles that exactly as retail does.
pub fn load_script(inc: &IncludePath, path: &str) -> Result<LoadedScript, LoadError> {
    if path.is_empty() {
        return Err(LoadError::EmptyPath);
    }
    let resolved_name = check_ext(path, SCRIPT_EXT);
    // The root goes through `open_file` with no current file, so candidate 1 is skipped.
    let root_path = inc
        .resolve_root(&resolved_name)
        .ok_or_else(|| LoadError::NotFound {
            resolved_name: resolved_name.clone(),
        })?;
    compile_resolved(inc, &root_path, resolved_name)
}

/// The same, for a caller that already holds the concrete file — a scenario editor, a test
/// fixture, or a mod tool. `resolved_name` is recorded verbatim.
pub fn load_script_file(inc: &IncludePath, root_path: &Path) -> Result<LoadedScript, LoadError> {
    let name = root_path.display().to_string();
    compile_resolved(inc, root_path, name)
}

fn compile_resolved(
    inc: &IncludePath,
    root_path: &Path,
    resolved_name: String,
) -> Result<LoadedScript, LoadError> {
    let unit = sema::analyze(root_path, inc).map_err(LoadError::Compile)?;
    let errors: Vec<sema::Diag> = unit
        .diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .cloned()
        .collect();
    if !errors.is_empty() {
        return Err(LoadError::Diagnostics(errors));
    }
    let entry = sema::entry_name(root_path);
    let (program, codegen_diags, _) = crate::codegen::compile(&unit);
    let cg_errors: Vec<sema::Diag> = codegen_diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .cloned()
        .collect();
    if !cg_errors.is_empty() {
        return Err(LoadError::Diagnostics(cg_errors));
    }
    let mut diags = unit.diags.clone();
    diags.extend(codegen_diags);
    diags.retain(|d| d.severity != Severity::Error);
    Ok(LoadedScript {
        root_path: root_path.to_path_buf(),
        resolved_name,
        program,
        entry,
        diags,
    })
}

/// Convenience for the no-mods case: one install root, retail's default
/// `ScriptIncludePath`.
pub fn install_include_path(root: impl Into<PathBuf>) -> IncludePath {
    IncludePath::with_content(Arc::new(sema::InstallRoot(root.into())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_ext_matches_the_three_measured_shapes() {
        // No extension: appended. This is how the general-powers *name* and a setup-screen
        // stem both become a file.
        assert_eq!(check_ext("general_powers", "bhs"), "general_powers.bhs");
        // Already `.bhs`, case-insensitively: untouched.
        assert_eq!(check_ext("a/b.bhs", "bhs"), "a/b.bhs");
        assert_eq!(check_ext("a/b.BHS", "bhs"), "a/b.BHS");
        // A different three-character extension is replaced, not appended.
        assert_eq!(check_ext("editor_scratch.svx", "bhs"), "editor_scratch.bhs");
        // A four-character extension is NOT at curr_len-4, so retail appends. A test that
        // expected `a.bhs` here would be asserting a tidier rule than the machine has.
        assert_eq!(check_ext("a.jpeg", "bhs"), "a.jpeg.bhs");
        // Shorter than four characters never enters the replace branch.
        assert_eq!(check_ext("x", "bhs"), "x.bhs");
        assert_eq!(check_ext(".bh", "bhs"), ".bh.bhs");
    }

    #[test]
    fn an_empty_path_is_a_failed_compile_not_an_absent_script() {
        let inc = install_include_path("/nonexistent");
        assert!(matches!(load_script(&inc, ""), Err(LoadError::EmptyPath)));
    }
}
