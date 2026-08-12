// SPDX-License-Identifier: GPL-3.0-or-later
//! BHS scripts as *content*: the bridge between [`crate::vfs::ContentStack`] and the
//! compiler's file resolution.
//!
//! Every script open in the engine goes through the same funnel as every other content
//! open. `Compiler::compile` `0x009bf160` calls `String::prepend_content_dir` `0x00A1D690`
//! on the script path (`0x009bf1f5..0x009bf206`), and `Lexer::open_file` `0x009bff30`
//! calls it again on each of its three include candidates (`0x009c0026`, `0x009c00ae`,
//! `0x009c018a`). `prepend_content_dir` is exactly [`ContentStack::resolve`]. So a mod
//! that declares `scenario/scriptlibrary/ctw_lib.bhs` replaces the shipped one for every
//! script that includes it, with the same first-match-wins precedence as `data/rules.xml`,
//! and this module is what makes that true for Descent of Nations rather than only for
//! retail.
//!
//! It is deliberately thin. The resolution rule lives in `don_bhs_cc::sema::IncludePath`,
//! derived from `Lexer::open_file`; the mod precedence lives in [`ContentStack`], derived
//! from `ModManager::calcFilePath` `0x00A22910`. This file joins them and adds nothing.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use don_bhs_cc::sema::{ContentProbe, IncludePath};

use crate::vfs::ContentStack;

/// `String::prepend_content_dir` `0x00A1D690` over a real install directory.
///
/// `install` is the directory retail runs in — the one every path returned by
/// `calcFilePath` is relative to. Resolution is: classify and pick the owning mod exactly
/// as retail does, then probe the resulting install-relative path on disk.
///
/// The disk probe is not decoration. `calcFilePath` never checks that the file exists; it
/// returns a path and the caller's `_wfsopen` decides. `Lexer::open_file` relies on that:
/// a candidate whose resolved path does not open falls through to the next candidate. A
/// probe that reported success for a nonexistent mod path would make the include search
/// stop early and diverge from retail on exactly the mods this crate exists to support.
#[derive(Debug, Clone)]
pub struct ContentScriptSource {
    install: PathBuf,
    stack: ContentStack,
}

impl ContentScriptSource {
    pub fn new(install: impl Into<PathBuf>, stack: ContentStack) -> Self {
        ContentScriptSource {
            install: install.into(),
            stack,
        }
    }

    /// The shipped tree with no mods installed.
    pub fn shipped(install: impl Into<PathBuf>) -> Self {
        ContentScriptSource::new(install, ContentStack::new())
    }

    pub fn install_dir(&self) -> &Path {
        &self.install
    }

    pub fn stack(&self) -> &ContentStack {
        &self.stack
    }

    /// The install-relative path retail would hand to `_wfsopen`, and the 1-based mod
    /// index that owns it (`0` = shipped). Exposed separately from [`ContentProbe::probe`]
    /// so a caller can report *which* mod supplied a script.
    pub fn resolved(&self, game_relative: &str) -> (String, usize) {
        let r = self.stack.resolve(&normalise(game_relative));
        (r.path, r.mod_index)
    }

    /// An [`IncludePath`] whose content boundary is this mod stack, carrying retail's
    /// default `ScriptIncludePath`.
    pub fn include_path(self) -> IncludePath {
        IncludePath::with_content(Arc::new(self))
    }
}

impl ContentProbe for ContentScriptSource {
    fn probe(&self, game_relative: &str) -> Option<PathBuf> {
        let (rel, _) = self.resolved(game_relative);
        probe_ci(&self.install, &rel)
    }
}

/// Retail paths arrive with `\` separators and often a leading `.\`;
/// [`crate::vfs::classify`] already normalises both, but the probe path is built here too.
fn normalise(p: &str) -> String {
    let p = p.replace('\\', "/");
    p.strip_prefix("./").unwrap_or(&p).to_string()
}

/// Walk `root` one path segment at a time, matching case-insensitively.
///
/// The shipped tree really does mix case (`Cliffs.xml`, `IME.xml`, `_SBLchangelog.xml`,
/// `scenario/Chess Exercise/`) and retail opens it through the Win32 case-insensitive
/// filesystem. Lower-casing the whole path — which is what `ContentStack` does to its
/// *lookup keys* — would produce a path that does not exist on a case-sensitive host, so
/// the probe folds per segment instead.
fn probe_ci(root: &Path, rel: &str) -> Option<PathBuf> {
    let segs: Vec<&str> = rel
        .split('/')
        .filter(|s| !s.is_empty() && *s != ".")
        .collect();
    if segs.is_empty() {
        return None;
    }
    let mut at = root.to_path_buf();
    for (i, seg) in segs.iter().enumerate() {
        let last = i + 1 == segs.len();
        let direct = at.join(seg);
        if (last && direct.is_file()) || (!last && direct.is_dir()) {
            at = direct;
            continue;
        }
        let mut found = None;
        for e in std::fs::read_dir(&at).ok()?.flatten() {
            let name = e.file_name();
            let Some(name) = name.to_str() else { continue };
            if !name.eq_ignore_ascii_case(seg) {
                continue;
            }
            let p = e.path();
            if (last && p.is_file()) || (!last && p.is_dir()) {
                found = Some(p);
                break;
            }
        }
        at = found?;
    }
    Some(at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vfs::{ModCategory, ModPackage};

    fn write(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "don-content-script-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_mod_that_declares_a_script_library_file_replaces_the_shipped_one() {
        // Catches: routing script includes around the mod stack, or resolving them by
        // scanning the install tree for a matching basename. Either bug reads the shipped
        // library here even though a mod owns the file.
        let root = tmp("mod-override");
        write(
            &root,
            "scenario/scriptlibrary/lib.bhs",
            "scenario shipped_marker () { }\n",
        );
        write(
            &root,
            "mods/better/scenario/scriptlibrary/lib.bhs",
            "scenario mod_marker () { }\n",
        );

        let shipped = ContentScriptSource::shipped(&root);
        assert_eq!(
            shipped.resolved("scenario/scriptlibrary/lib.bhs"),
            ("scenario/scriptlibrary/lib.bhs".to_string(), 0)
        );

        let mut m = ModPackage::new("better", "better");
        m.declare_path("scenario/scriptlibrary/lib.bhs");
        let mut stack = ContentStack::new();
        stack.push(m);
        let modded = ContentScriptSource::new(&root, stack);
        let (path, index) = modded.resolved("scenario/scriptlibrary/lib.bhs");
        assert_eq!(path, "mods/better/scenario/scriptlibrary/lib.bhs");
        assert_eq!(index, 1);
        assert_eq!(
            modded.probe("scenario/scriptlibrary/lib.bhs"),
            Some(root.join("mods/better/scenario/scriptlibrary/lib.bhs"))
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_declared_but_absent_mod_file_does_not_open() {
        // `calcFilePath` never stats. If this returned Some, `Lexer::open_file` would stop
        // at candidate 2 instead of falling through to the include-path candidate, which
        // is a silent divergence rather than a visible failure.
        let root = tmp("declared-absent");
        write(
            &root,
            "scenario/scriptlibrary/lib.bhs",
            "scenario s () { }\n",
        );
        let mut m = ModPackage::new("broken", "broken");
        m.declare_path("scenario/scriptlibrary/lib.bhs");
        let mut stack = ContentStack::new();
        stack.push(m);
        let src = ContentScriptSource::new(&root, stack);
        assert_eq!(src.resolved("scenario/scriptlibrary/lib.bhs").1, 1);
        assert_eq!(src.probe("scenario/scriptlibrary/lib.bhs"), None);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn probe_folds_case_per_segment_not_by_lowercasing_the_path() {
        // The shipped tree has mixed-case names; a lower-cased whole path does not exist
        // on a case-sensitive host.
        // On a case-insensitive host the direct join already opens the file; on a
        // case-sensitive one only the per-segment fold does. Assert on what was opened,
        // not on the spelling that came back, so the test is meaningful on both.
        let root = tmp("mixed-case");
        write(
            &root,
            "Scenario/ScriptLibrary/Lib.BHS",
            "scenario mixed_case_marker () { }\n",
        );
        let src = ContentScriptSource::shipped(&root);
        let opened = src
            .probe(".\\scenario\\scriptlibrary\\lib.bhs")
            .expect("mixed-case shipped path must open");
        assert_eq!(
            std::fs::read_to_string(&opened).unwrap(),
            "scenario mixed_case_marker () { }\n"
        );
        assert_eq!(src.probe("scenario/scriptlibrary/absent.bhs"), None);
        assert_eq!(
            crate::vfs::classify("./scenario/scriptlibrary/lib.bhs").0,
            ModCategory::Scenario
        );
        std::fs::remove_dir_all(&root).ok();
    }
}
