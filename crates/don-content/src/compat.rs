//! Can a retail mod load into our engine unmodified?
//!
//! The honest answer, in one sentence: **the core path resolver is reproduced, but no retail
//! gameplay mod is accepted until every file it supplies has an end-to-end consumer.**
//!
//! Two halves have to work, and they fail for different reasons:
//!
//! 1. **Path classification and precedence.** Retail's core rule is [`crate::vfs`], which is
//!    small and reproduced with the engine's own tables. Cross-platform directory discovery
//!    is not certified because enumeration order and `SkipForbiddenFiles` still need a live
//!    retail corpus. Nothing about resolving an already-installed package needs Steam: the
//!    Workshop is a *delivery* mechanism that ends with a directory on disk, and
//!    `ModManager::buildModPackages` `0x00A221F0` then treats that directory exactly like a
//!    local one. Subscribing needs Steam; loading does not.
//! 2. **Consumption.** A mod ships bytes for a subsystem. We have a model of the shipped
//!    `rules.xml` result, but no external XML loader; `art/foo.bh3` likewise has no renderer.
//!
//! So this module does not answer yes/no. It takes a mod's declared file list and reports,
//! per file, which of those two halves it clears. That is the number worth quoting — "N of M
//! files consumed" — because it is measurable and it moves as lanes land.
//!
//! # The two things that would actually block compatibility, and neither does
//!
//! * *Encryption / packing.* There is none. `ModPackage::buildCategory` `0x00A34DB0`
//!   enumerates plain files with `TFileSystem::FindAllMatchingFiles` `0x00A323E0` and every
//!   consumer opens them through `String::prepend_content_dir` `0x00A1D690`. A mod is a
//!   directory tree.
//! * *A manifest format we would have to guess.* There is no manifest for data mods at all —
//!   the file list *is* the manifest. Only dropdown mods have `info.xml`
//!   (`ModPackage::isDropdownMod` `0x00A35C20`), read by `GameMod::init` `0x005A9AD0`, and
//!   that is metadata (name, description) plus a reload trigger, not a schema.

use std::collections::BTreeMap;

use crate::vfs::{ModCategory, ModPackage};

/// How far a given file gets in *our* engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Support {
    /// Resolved and consumed end-to-end for the file's advertised purpose.
    Consumed,
    /// Resolved and parsed, but nothing downstream acts on it yet.
    Parsed,
    /// Resolves to the right path; no end-to-end consumer exists. Inert and rejected by the
    /// gameplay-readiness gate.
    ResolvedOnly,
    /// Needs a subsystem that is not implemented (renderer, audio). Rejected.
    OutOfScope,
}

impl Support {
    pub fn label(self) -> &'static str {
        match self {
            Support::Consumed => "consumed",
            Support::Parsed => "parsed",
            Support::ResolvedOnly => "resolved-only",
            Support::OutOfScope => "out-of-scope",
        }
    }
}

/// One rule of the support table, with the reason it says what it says.
pub struct SupportRule {
    pub category: Option<ModCategory>,
    /// Lowercase extension including the dot, an exact basename, or `""` to match every file
    /// in the category.
    pub ext: &'static str,
    pub support: Support,
    pub reason: &'static str,
}

/// The support table, as of this lane. Each row cites the crate or the gap.
///
/// Rows are scanned in order and the first match wins, so put specific rows above the
/// category catch-alls.
pub static SUPPORT: &[SupportRule] = &[
    SupportRule {
        category: Some(ModCategory::Data),
        ext: "rules.xml",
        support: Support::ResolvedOnly,
        reason: "don-rules models the shipped Constants block, but no external XML-to-Rules loader is wired",
    },
    SupportRule {
        category: Some(ModCategory::Data),
        ext: ".xml",
        support: Support::ResolvedOnly,
        reason: "no end-to-end loader applies arbitrary data XML; unit/type/tech/building binders are not implemented",
    },
    SupportRule {
        category: Some(ModCategory::Data),
        ext: ".dtd",
        support: Support::ResolvedOnly,
        reason: "schema only; the engine's XML reader does not validate against it",
    },
    SupportRule {
        category: Some(ModCategory::Data),
        ext: ".sps",
        support: Support::ResolvedOnly,
        reason: "XMLSpy project file, shipped but unread by the engine",
    },
    SupportRule {
        category: Some(ModCategory::Data),
        ext: ".xsd",
        support: Support::ResolvedOnly,
        reason: "schema only",
    },
    SupportRule {
        category: Some(ModCategory::Data),
        ext: ".bhs",
        support: Support::Parsed,
        reason: "don-bhs / don-bhs-cc front end exists; RunTimeEnv::run_script 0x0043D0E0 is not driven by our tick",
    },
    SupportRule {
        category: Some(ModCategory::Ai),
        ext: ".bhs",
        support: Support::Parsed,
        reason: "same: the AI scripts parse, the interpreter is a live lane",
    },
    SupportRule {
        category: Some(ModCategory::Ai),
        ext: ".bho",
        support: Support::ResolvedOnly,
        reason: "precompiled BHS object; our front end compiles from source instead",
    },
    SupportRule {
        category: Some(ModCategory::MapStyles),
        ext: ".xml",
        support: Support::ResolvedOnly,
        reason: "no map-style XML parser or map-generation consumer is wired",
    },
    SupportRule {
        category: Some(ModCategory::Tribes),
        ext: "",
        support: Support::ResolvedOnly,
        reason: "localised nation text (.4/.7/.9/.10/.12/.16/.17/.18); needs the StringTable at [0x00C06378]",
    },
    SupportRule {
        category: Some(ModCategory::Scenario),
        ext: "",
        support: Support::ResolvedOnly,
        reason: "ScenarioData::walk_data 0x00997AD0 is checksum channel 14 and has no runtime producer",
    },
    SupportRule {
        category: Some(ModCategory::Conquest),
        ext: "",
        support: Support::OutOfScope,
        reason: "Conquer the World campaign layer; explicitly out of scope in COVERAGE.md",
    },
    SupportRule {
        category: Some(ModCategory::Art),
        ext: "",
        support: Support::OutOfScope,
        reason: "no renderer consumes .bh3/.bha/.mot/.tga/.wmv/.cur",
    },
    SupportRule {
        category: Some(ModCategory::Terrain),
        ext: "",
        support: Support::OutOfScope,
        reason: "no renderer",
    },
    SupportRule {
        category: Some(ModCategory::Sounds),
        ext: "",
        support: Support::OutOfScope,
        reason: "no audio",
    },
    SupportRule {
        category: Some(ModCategory::Replays),
        ext: ".rcx",
        support: Support::Consumed,
        reason: "don-replay decodes .rcx and drives the validation scoreboard",
    },
    SupportRule {
        category: Some(ModCategory::Root),
        ext: "info.xml",
        support: Support::Parsed,
        reason: "dropdown metadata and measured structural gates are parsed; retail checksum generation and runtime reload are not reproduced",
    },
    SupportRule {
        category: Some(ModCategory::Root),
        ext: "don-overlay.xml",
        support: Support::Parsed,
        reason: "DoN overlay schema is parsed and composed against don-rules; the composed block is not yet registered into the sim bootstrap",
    },
    SupportRule {
        category: Some(ModCategory::Root),
        ext: ".txt",
        support: Support::ResolvedOnly,
        reason: "root text data (balancerules.txt, counterchart.txt); read by Text::open, unported",
    },
    SupportRule {
        category: None,
        ext: "",
        support: Support::ResolvedOnly,
        reason: "resolves correctly; no consumer classified",
    },
];

pub fn classify_support(cat: ModCategory, filename: &str) -> &'static SupportRule {
    let f = filename.to_ascii_lowercase();
    let base = f.rsplit('/').next().unwrap_or(&f).to_string();
    let ext = base
        .rfind('.')
        .map(|i| base[i..].to_string())
        .unwrap_or_default();
    SUPPORT
        .iter()
        .find(|r| {
            r.category.is_none_or(|c| c == cat)
                && (r.ext.is_empty() || r.ext == ext || r.ext == base)
        })
        .expect("the table ends with a catch-all")
}

/// One line of a compatibility report.
#[derive(Clone, Debug)]
pub struct FileVerdict {
    pub category: ModCategory,
    pub filename: String,
    pub support: Support,
    pub reason: &'static str,
}

/// What a mod would do if it were installed today.
#[derive(Clone, Debug)]
pub struct CompatReport {
    pub mod_name: String,
    pub files: Vec<FileVerdict>,
    pub by_support: BTreeMap<Support, usize>,
    /// Files this mod declares that retail itself would refuse to load
    /// (`ModManager::isMapForbidden` `0x00A21140`).
    pub vetoed_by_retail: Vec<String>,
}

impl CompatReport {
    pub fn count(&self, s: Support) -> usize {
        self.by_support.get(&s).copied().unwrap_or(0)
    }

    /// The number worth quoting.
    pub fn consumed_fraction(&self) -> (usize, usize) {
        (self.count(Support::Consumed), self.files.len())
    }

    /// True when nothing the package declares is inert, unsupported, or vetoed by retail.
    pub fn fully_consumed(&self) -> bool {
        !self.files.is_empty()
            && self.vetoed_by_retail.is_empty()
            && self.count(Support::Consumed) == self.files.len()
    }
}

pub fn report(m: &ModPackage) -> CompatReport {
    let mut files = Vec::new();
    let mut by_support: BTreeMap<Support, usize> = BTreeMap::new();
    let mut vetoed = Vec::new();
    for cat in crate::vfs::ALL_CATEGORIES {
        for name in &m.files[cat.index()] {
            if cat == ModCategory::MapStyles && crate::vfs::is_map_forbidden(name) {
                vetoed.push(name.clone());
            }
            let rule = classify_support(cat, name);
            *by_support.entry(rule.support).or_insert(0) += 1;
            files.push(FileVerdict {
                category: cat,
                filename: name.clone(),
                support: rule.support,
                reason: rule.reason,
            });
        }
    }
    CompatReport {
        mod_name: m.name.clone(),
        files,
        by_support,
        vetoed_by_retail: vetoed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_xml_is_rejected_until_an_external_loader_is_wired() {
        let mut m = ModPackage::new("Rebalance", "Rebalance");
        m.declare_path("data/rules.xml");
        m.declare_path("data/unitrules.xml");
        let r = report(&m);
        assert!(!r.fully_consumed());
        assert_eq!(r.consumed_fraction(), (0, 2));
        assert_eq!(r.count(Support::ResolvedOnly), 2);
    }

    #[test]
    fn an_art_mod_is_honestly_reported_as_out_of_scope() {
        let mut m = ModPackage::new("Skins", "Skins");
        m.declare_path("art/unit01.bh3");
        m.declare_path("art/unit01.tga");
        let r = report(&m);
        assert_eq!(r.count(Support::OutOfScope), 2);
        assert!(!r.fully_consumed());
    }

    #[test]
    fn a_mixed_mod_splits_by_file_not_by_mod() {
        let mut m = ModPackage::new("Total Conversion", "TC");
        m.declare_path("data/rules.xml");
        m.declare_path("ai/scripts/economic.bhs");
        m.declare_path("art/thing.bha");
        m.declare_path("sounds/boom.wav");
        let r = report(&m);
        assert_eq!(r.count(Support::Consumed), 0);
        assert_eq!(r.count(Support::Parsed), 1);
        assert_eq!(r.count(Support::ResolvedOnly), 1);
        assert_eq!(r.count(Support::OutOfScope), 2);
    }

    #[test]
    fn retails_own_veto_is_surfaced() {
        let mut m = ModPackage::new("Maps", "Maps");
        m.declare_path("mapstyles/default.xml");
        m.declare_path("mapstyles/brandnew.xml");
        let r = report(&m);
        assert_eq!(r.vetoed_by_retail, vec!["default.xml".to_string()]);
    }
}
