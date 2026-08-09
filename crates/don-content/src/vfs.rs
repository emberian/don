//! The retail content virtual filesystem, reproduced.
//!
//! # What retail actually does
//!
//! Rise of Nations does **not** merge mods. It resolves *file paths*. Every content open in
//! the game — `File::open`, `XML::init`, `Text::open`, `Filemap::open`, `Lexer::open_file`,
//! `Compiler::compile`, and 55 other call sites — goes through
//! `String::prepend_content_dir` `0x00A1D690`, which calls
//! `ModManager::prepend_content_dir` `0x00A22800`, which is exactly two steps:
//!
//! 1. `ModManager::calcFileNameAndCategoryFromPath` `0x00A21E40` — classify a game-relative
//!    path into a [`ModCategory`] plus the filename *within* that category.
//! 2. `ModManager::calcFilePath` `0x00A22910` — walk the mod list and return the first
//!    installed path that owns that filename in that category, or the vanilla path.
//!
//! Consequence, and it is the single most important fact in this module: **a mod replaces
//! whole files, never fields.** A data mod that ships `data/rules.xml` supplies *all* of the
//! rules; the shipped file is not consulted at all. There is no retail mechanism for "change
//! one constant", which is why [`crate::overlay`] exists.
//!
//! # The precedence rule, stated exactly
//!
//! `calcFilePath(category, filename, &out_index)` [measured, `0x00A22910`]:
//!
//! ```text
//! for mod in mods:                              # std::list order == priority order
//!     if not mod.enabled:                       continue   # +0x150
//!     if mod.is_dropdown and not mod.dropdown_active: continue  # +0x151
//!     if mod.files[category] is empty:          continue
//!     if filename not in mod.files[category]:   continue   # SkyStringList::FindI, case-insensitive
//!     if category == MapStyles and is_map_forbidden(filename): continue
//!     out_index = 1 + position_of(mod)          # 1-based; 0 means "shipped"
//!     if mod.location == MYMODS:  return "mods/" + mod.install_dir + "/" + rel(category) + filename
//!     else:                       return          mod.install_dir + "/" + rel(category) + filename
//! out_index = 0
//! return rel(category) + filename
//! ```
//!
//! The list is kept sorted by `ModManager::sortViaPriority` `0x00A22CC0`, whose comparator
//! `0x00A21E20` is a bare `a->priority < b->priority` on the `int` at `+0x144`, after which
//! the priorities are *renumbered* `1..N` in list order. So priority is a total order with no
//! ties by construction, **lower number wins**, and it is stable only because the sort is
//! `std::list::sort` (a merge sort).
//!
//! Two details that a naive reimplementation gets wrong:
//!
//! * **First match wins, and the scan stops.** A second mod that also owns the file never
//!   sees it. There is no "later mod patches earlier mod".
//! * **The mod must *declare* the file.** `mod.files[category]` is not a live directory
//!   probe; it is the snapshot `ModPackage::buildCategory` `0x00A34DB0` took at scan time.
//!   A file dropped into a mod folder after the scan is invisible until the next scan.

use std::collections::BTreeSet;

use crate::generated::{CATEGORY_INFO, FORBIDDEN_MAPSTYLES, TAG_LINKS, TAG_NAMES};

/// `enum ModCategoryType` — PDB `LF_ENUM 0x58AB` [measured].
///
/// The discriminants are the engine's, and the *order* is load-bearing: classification scans
/// `0..=11` and returns the first prefix hit, so `Root` (empty prefix) must remain last.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(i32)]
pub enum ModCategory {
    Ai = 0,
    Art = 1,
    Conquest = 2,
    Data = 3,
    MapStyles = 4,
    Scenario = 5,
    Sounds = 6,
    Terrain = 7,
    Tribes = 8,
    Replays = 9,
    Saves = 10,
    Root = 11,
}

/// `CATEGORY_MAX` = 12 [measured].
pub const CATEGORY_MAX: usize = 12;

pub static ALL_CATEGORIES: [ModCategory; CATEGORY_MAX] = [
    ModCategory::Ai,
    ModCategory::Art,
    ModCategory::Conquest,
    ModCategory::Data,
    ModCategory::MapStyles,
    ModCategory::Scenario,
    ModCategory::Sounds,
    ModCategory::Terrain,
    ModCategory::Tribes,
    ModCategory::Replays,
    ModCategory::Saves,
    ModCategory::Root,
];

impl ModCategory {
    #[inline]
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(i: usize) -> Option<ModCategory> {
        ALL_CATEGORIES.get(i).copied()
    }

    /// `ModCategories::getRelativeDirectory` `0x00A491D0` — the `relativeDirectory` field of
    /// `s_ModCategoryInfo[cat]`, with `/` separators.
    pub fn relative_dir(self) -> &'static str {
        CATEGORY_INFO[self.index()].relative_dir
    }

    /// `ModCategoryInfo::name`, the engine's own label (`"AI"`, `"MAPSTYLES"`, ...).
    pub fn name(self) -> &'static str {
        CATEGORY_INFO[self.index()].name
    }

    /// `ModCategoryInfo::recursive` — whether `ModPackage::buildCategory` passes
    /// `FindFile::RecurseSubDirs` when it enumerates this category.
    pub fn recursive(self) -> bool {
        CATEGORY_INFO[self.index()].recursive
    }
}

/// `enum SteamWorkshopTags` — PDB `LF_ENUM 0x16B5` [measured].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(i32)]
pub enum WorkshopTag {
    Ai = 0,
    Art = 1,
    Conquest = 2,
    Cursors = 3,
    Data = 4,
    Mods = 5,
    MapStyles = 6,
    Replays = 7,
    Scenarios = 8,
    Sounds = 9,
    Terrain = 10,
    Tribe = 11,
    Other = 12,
}

impl WorkshopTag {
    /// `s_SteamWorkshopTagNames[tag].tagName` — the literal string Steam sees.
    pub fn name(self) -> &'static str {
        TAG_NAMES[self as usize]
    }
}

/// One row of `s_SteamWorkshopTagLinks` `0x00C068D0`.
pub struct TagLink {
    pub tag: WorkshopTag,
    pub category: ModCategory,
    /// Either `"*"` (any file), a `.ext` suffix, or an exact filename such as `info.xml`.
    /// A leading `!` negates the row [measured, `0x00A35300` tests for `'!'` = `0x21`];
    /// no shipped row uses it.
    pub pattern: &'static str,
    pub recursive: bool,
}

/// `enum StoragePoint::Location` — PDB `LF_ENUM` [measured]. Local packages use `MyMods`;
/// Workshop packages use `None` plus an absolute install directory. The intervening storage
/// points are named so status matching and the discriminants stay exact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(i32)]
pub enum StorageLocation {
    None = 0,
    ProgramPath = 1,
    LanguageData = 2,
    TempFiles = 3,
    GameRoot = 4,
    LogFiles = 5,
    SaveGames = 6,
    /// `MYMODS` = 7. Locally installed mods, under `<MyMods>/mods/<dir>/`. This is the value
    /// `ModManager::buildModPackages` `0x00A221F0` stamps on every directory it finds, and
    /// the one `calcFilePath` special-cases with the literal `"mods\"` prefix `0x00B14A5C`.
    MyMods = 7,
}

// -----------------------------------------------------------------------------------------
// classification
// -----------------------------------------------------------------------------------------

/// `ModManager::calcFileNameAndCategoryFromPath` `0x00A21E40` [measured].
///
/// Normalises separators, strips a leading `./`, then returns the first category whose
/// `relativeDirectory` is a prefix, together with the remainder. Because `Root`'s prefix is
/// empty and it is scanned last, this never fails for a non-empty path — every content path
/// is moddable as *something*.
///
/// Retail's prefix test is `SkyString::StartsWith` `0x00A207F0`, which is case-sensitive.
/// The shipped tables and engine callers use lowercase paths. This function keeps that
/// behavior exactly: `Data/rules.xml` is `CAT_ROOT`, not `CAT_DATA`. Callers that accept
/// human-entered paths should normalise them explicitly before entering the fidelity path.
pub fn classify(path: &str) -> (ModCategory, String) {
    let mut p = path.replace('\\', "/");
    if let Some(rest) = p.strip_prefix("./") {
        p = rest.to_string();
    }
    for cat in ALL_CATEGORIES {
        let rel = cat.relative_dir();
        if !rel.is_empty() && p.starts_with(rel) {
            return (cat, p[rel.len()..].to_string());
        }
    }
    (ModCategory::Root, p)
}

/// `ModManager::isMapForbidden` `0x00A21140` [measured]. Case-insensitive because the
/// filename it is handed has already been lowercased by `ModPackage::buildCategory`
/// (`SkyString::ConvertToLowerCase` `0x00A1F540`).
pub fn is_map_forbidden(filename: &str) -> bool {
    let f = filename.to_ascii_lowercase();
    FORBIDDEN_MAPSTYLES.iter().any(|m| *m == f)
}

// -----------------------------------------------------------------------------------------
// packages
// -----------------------------------------------------------------------------------------

/// One entry of `ModManager`'s `std::list<ModPackage*>` (`0x00ED6760`).
///
/// Field offsets in the retail 360-byte (`0x168`) object, all derived from code rather than
/// from PDB member records (the PDB emits only `ModPackage`'s ctor/dtor):
///
/// | offset | field | derived from |
/// |---|---|---|
/// | `+0x000` | `u64` owner SteamID | `readModStatus` `0x00A244E0` writes it from a Steam call |
/// | `+0x008` | `u64` publishedFileId | `addModPackage` `0x00A23E40` dedup key |
/// | `+0x010` | `SkyStringList` for `CAT_ROOT` | `getCategoryFileList` `0x00A35B40` |
/// | `+0x024 .. +0x0FC` | `SkyStringList` x 11, stride `0x14` | same |
/// | `+0x114` | `SkyString` display name | `addModPackage` arg 4 |
/// | `+0x124` | `SkyString` install directory | `calcFilePath` reads it |
/// | `+0x138` | `u64` total bytes | `buildCategory` accumulates |
/// | `+0x140` | `StoragePoint::Location` | `calcFilePath` compares to 7 |
/// | `+0x144` | `int` priority | `sortViaPriority` renumbers |
/// | `+0x148` | `int` publishable-asset count | `buildCategory` |
/// | `+0x14C` | `int` total file count | `buildPackage` requires non-zero |
/// | `+0x150` | `bool` enabled | `calcFilePath` gate 1 |
/// | `+0x151` | `bool` dropdown active | `calcFilePath` gate 2 |
/// | `+0x160` | `int` second timestamp column | `readModStatus` |
/// | `+0x164` | `int` first timestamp column | `readModStatus` |
#[derive(Clone, Debug, Default)]
pub struct ModPackage {
    /// `+0x114`. The name `mod-status.txt` keys on.
    pub name: String,
    /// `+0x124`. Directory *name* when `location == MyMods`; absolute path otherwise.
    pub install_dir: String,
    /// `+0x140`.
    pub location: Option<StorageLocation>,
    /// `+0x144`. Lower wins. Renumbered `1..N` after every sort.
    pub priority: i32,
    /// `+0x150`.
    pub enabled: bool,
    /// `+0x151`. Only consulted when the mod is a *dropdown* mod (has `info.xml` at root).
    pub dropdown_active: bool,
    /// `+0x008`.
    pub published_file_id: u64,
    /// `+0x000`.
    pub author_id: u64,
    /// First `TIMESTAMP` column in `mod-status.txt`, stored at retail offset `+0x164`.
    pub timestamp: i32,
    /// Second `TIMESTAMP2` column, stored at retail offset `+0x160`.
    pub timestamp2: i32,
    /// The 12 declared file lists, lowercased, `/`-separated, relative to the category dir.
    /// This is `ModPackage::buildCategory`'s snapshot, not a live directory.
    pub files: [BTreeSet<String>; CATEGORY_MAX],
}

impl ModPackage {
    pub fn new(name: impl Into<String>, install_dir: impl Into<String>) -> ModPackage {
        ModPackage {
            name: name.into(),
            install_dir: install_dir.into(),
            location: Some(StorageLocation::MyMods),
            // 0 = unset. `ContentStack::push` fills it with the discovery index, which is
            // what `ModManager::addModPackage` `0x00A23E40` does (`priority = list.size()`).
            priority: 0,
            enabled: true,
            dropdown_active: false,
            ..Default::default()
        }
    }

    /// Declare a file, exactly as `buildCategory` would after `ConvertToLowerCase`.
    pub fn declare(&mut self, cat: ModCategory, filename: &str) -> &mut Self {
        self.files[cat.index()].insert(filename.replace('\\', "/").to_ascii_lowercase());
        self
    }

    /// Declare a file given a *game-relative* path; classifies it first.
    pub fn declare_path(&mut self, path: &str) -> ModCategory {
        let (cat, name) = classify(path);
        self.declare(cat, &name);
        cat
    }

    /// `ModPackage::hasAsset` `0x00A35A60`.
    pub fn has_asset(&self, cat: ModCategory, filename: &str) -> bool {
        self.files[cat.index()].contains(&filename.to_ascii_lowercase())
    }

    /// `ModPackage::isDropdownMod` `0x00A35C20` — literally "does the root file list contain
    /// `info.xml`". A dropdown mod is one the player picks in a combo box for a single match;
    /// a data mod is one that is simply on or off.
    pub fn is_dropdown_mod(&self) -> bool {
        self.files[ModCategory::Root.index()].contains("info.xml")
    }

    /// `ModPackage::isDataMod` `0x00A35BF0` — any non-empty category other than `Replays`,
    /// `Saves`, `Scenario`, `Root`.
    pub fn is_data_mod(&self) -> bool {
        ALL_CATEGORIES.iter().any(|c| {
            !matches!(
                c,
                ModCategory::Replays
                    | ModCategory::Saves
                    | ModCategory::Scenario
                    | ModCategory::Root
            ) && !self.files[c.index()].is_empty()
        })
    }

    /// `+0x14C`: total declared files.
    pub fn file_count(&self) -> usize {
        self.files.iter().map(|s| s.len()).sum()
    }

    /// `ModPackage::calculateSteamTags` `0x00A35300`, positive rows only.
    ///
    /// For each [`TagLink`]: a file in `link.category` matches when the pattern is `*`, or
    /// the pattern starts with `.` and the filename ends with it, or the pattern equals the
    /// filename. When `link.recursive` is false, files in a subdirectory (containing a
    /// separator) are skipped — that is the `"\\"` test at `0x00B1966C`. An empty result
    /// becomes `TagOther`.
    pub fn steam_tags(&self) -> BTreeSet<WorkshopTag> {
        let mut out = BTreeSet::new();
        for link in TAG_LINKS.iter() {
            if link.pattern.starts_with('!') {
                continue; // no shipped row uses the negation form
            }
            let hit = self.files[link.category.index()].iter().any(|f| {
                if !link.recursive && f.contains('/') {
                    return false;
                }
                if link.pattern == "*" {
                    true
                } else if let Some(ext) = link.pattern.strip_prefix('.') {
                    f.rsplit('/')
                        .next()
                        .unwrap_or(f)
                        .ends_with(&format!(".{ext}"))
                } else {
                    f.rsplit('/').next().unwrap_or(f) == link.pattern
                }
            });
            if hit {
                out.insert(link.tag);
            }
        }
        if out.is_empty() {
            out.insert(WorkshopTag::Other);
        }
        out
    }
}

// -----------------------------------------------------------------------------------------
// the stack
// -----------------------------------------------------------------------------------------

/// Where a resolved content path came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    /// Retail's `out_index`: `0` = shipped data, `n` = the `n`-th mod in priority order.
    pub mod_index: usize,
    /// The path retail would hand to the storage layer, `/`-separated.
    pub path: String,
    /// Which category the request classified into.
    pub category: ModCategory,
}

/// `ModManager` — the ordered mod list plus the resolution rule.
#[derive(Clone, Debug, Default)]
pub struct ContentStack {
    mods: Vec<ModPackage>,
}

impl ContentStack {
    pub fn new() -> ContentStack {
        ContentStack::default()
    }

    /// Append, exactly as `ModManager::addModPackage` `0x00A23E40` does: the package goes on
    /// the end of the `std::list` and, if it carries no priority yet, takes the new list
    /// length as its priority. **This does not sort.** Retail sorts once, at the end of
    /// `ModManager::readModStatus` `0x00A244E0`, and never per insertion — so discovery order
    /// is the default order and the status file is what reshuffles it.
    pub fn push(&mut self, mut m: ModPackage) -> &mut Self {
        if m.priority <= 0 {
            m.priority = self.mods.len() as i32 + 1;
        }
        self.mods.push(m);
        self
    }

    pub fn mods(&self) -> &[ModPackage] {
        &self.mods
    }

    pub fn mods_mut(&mut self) -> &mut [ModPackage] {
        &mut self.mods
    }

    /// `ModManager::sortViaPriority` `0x00A22CC0`.
    pub fn sort_via_priority(&mut self) {
        self.mods.sort_by_key(|m| m.priority); // stable, like std::list::sort
        for (i, m) in self.mods.iter_mut().enumerate() {
            m.priority = i as i32 + 1;
        }
    }

    /// `ModManager::getNumActiveDataMods` `0x00A243D0`.
    pub fn active_data_mods(&self) -> usize {
        self.mods
            .iter()
            .filter(|m| m.enabled && m.is_data_mod())
            .count()
    }

    /// `ModManager::calcFilePath` `0x00A22910`, given a game-relative path.
    ///
    /// The returned path is what retail hands the storage layer: relative for shipped files
    /// and for `MyMods` mods (rooted at the MyMods storage point), absolute for Workshop
    /// mods whose `install_dir` is absolute.
    pub fn resolve(&self, path: &str) -> Resolved {
        let (cat, filename) = classify(path);
        self.resolve_in(cat, &filename)
    }

    /// The same, when the caller already knows the category.
    pub fn resolve_in(&self, cat: ModCategory, filename: &str) -> Resolved {
        let key = filename.to_ascii_lowercase();
        for (i, m) in self.mods.iter().enumerate() {
            if !m.enabled {
                continue;
            }
            if m.is_dropdown_mod() && !m.dropdown_active {
                continue;
            }
            if m.files[cat.index()].is_empty() {
                continue;
            }
            if !m.files[cat.index()].contains(&key) {
                continue;
            }
            if cat == ModCategory::MapStyles && is_map_forbidden(&key) {
                continue;
            }
            let prefix = if m.location == Some(StorageLocation::MyMods) {
                format!("mods/{}", m.install_dir)
            } else {
                m.install_dir.clone()
            };
            return Resolved {
                mod_index: i + 1,
                path: format!("{}/{}{}", prefix, cat.relative_dir(), filename),
                category: cat,
            };
        }
        Resolved {
            mod_index: 0,
            path: format!("{}{}", cat.relative_dir(), filename),
            category: cat,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_prefixes_are_the_engines() {
        assert_eq!(ModCategory::Ai.relative_dir(), "ai/scripts/");
        assert_eq!(ModCategory::Terrain.relative_dir(), "terrain art/");
        assert_eq!(ModCategory::Root.relative_dir(), "");
        assert_eq!(ModCategory::Art.name(), "ART");
        assert!(ModCategory::Scenario.recursive());
        assert!(!ModCategory::Data.recursive());
    }

    #[test]
    fn classify_matches_the_scan_order() {
        assert_eq!(
            classify("data/rules.xml"),
            (ModCategory::Data, "rules.xml".into())
        );
        assert_eq!(
            classify("Data\\rules.xml"),
            (ModCategory::Root, "Data/rules.xml".into()),
            "retail's StartsWith is case-sensitive"
        );
        assert_eq!(
            classify("./data/rules.xml"),
            (ModCategory::Data, "rules.xml".into())
        );
        assert_eq!(
            classify("ai/scripts/economic.bhs"),
            (ModCategory::Ai, "economic.bhs".into())
        );
        // "ai/" alone is not a category; only "ai/scripts/" is, so this falls to Root.
        assert_eq!(
            classify("ai/other.bhs"),
            (ModCategory::Root, "ai/other.bhs".into())
        );
        // Root's empty prefix catches everything else.
        assert_eq!(
            classify("bighuge.txt"),
            (ModCategory::Root, "bighuge.txt".into())
        );
    }

    #[test]
    fn shipped_path_when_no_mod_owns_the_file() {
        let stack = ContentStack::new();
        let r = stack.resolve("data/rules.xml");
        assert_eq!(r.mod_index, 0);
        assert_eq!(r.path, "data/rules.xml");
    }

    #[test]
    fn first_match_in_priority_order_wins_and_the_scan_stops() {
        let mut stack = ContentStack::new();
        let mut a = ModPackage::new("Alpha", "Alpha");
        a.priority = 2;
        a.declare_path("data/rules.xml");
        let mut b = ModPackage::new("Bravo", "Bravo");
        b.priority = 1;
        b.declare_path("data/rules.xml");
        stack.push(a).push(b);
        stack.sort_via_priority();

        // sortViaPriority renumbered them 1,2 in priority order.
        assert_eq!(stack.mods()[0].name, "Bravo");
        assert_eq!(stack.mods()[0].priority, 1);

        let r = stack.resolve("data/rules.xml");
        assert_eq!(r.mod_index, 1);
        assert_eq!(r.path, "mods/Bravo/data/rules.xml");
    }

    #[test]
    fn a_disabled_mod_is_skipped_entirely() {
        let mut stack = ContentStack::new();
        let mut a = ModPackage::new("Alpha", "Alpha");
        a.enabled = false;
        a.declare_path("data/rules.xml");
        stack.push(a);
        assert_eq!(stack.resolve("data/rules.xml").mod_index, 0);
    }

    #[test]
    fn a_dropdown_mod_only_applies_when_selected() {
        let mut stack = ContentStack::new();
        let mut a = ModPackage::new("Alpha", "Alpha");
        a.declare_path("info.xml");
        a.declare_path("data/rules.xml");
        assert!(a.is_dropdown_mod());
        stack.push(a);
        assert_eq!(stack.resolve("data/rules.xml").mod_index, 0);
        stack.mods_mut()[0].dropdown_active = true;
        assert_eq!(stack.resolve("data/rules.xml").mod_index, 1);
    }

    #[test]
    fn forbidden_map_styles_cannot_be_overridden_but_new_ones_can() {
        let mut stack = ContentStack::new();
        let mut a = ModPackage::new("Alpha", "Alpha");
        a.declare_path("mapstyles/default.xml");
        a.declare_path("mapstyles/mymap.xml");
        stack.push(a);
        assert_eq!(stack.resolve("mapstyles/default.xml").mod_index, 0);
        assert_eq!(stack.resolve("mapstyles/mymap.xml").mod_index, 1);
        // and the veto is map-styles-only
        let mut stack2 = ContentStack::new();
        let mut b = ModPackage::new("Bravo", "Bravo");
        b.declare_path("data/default.xml");
        stack2.push(b);
        assert_eq!(stack2.resolve("data/default.xml").mod_index, 1);
    }

    #[test]
    fn workshop_mods_resolve_to_their_absolute_install_path() {
        let mut stack = ContentStack::new();
        let mut a = ModPackage::new("Ws", "C:/steam/workshop/content/287450/1234");
        a.location = Some(StorageLocation::None);
        a.declare_path("data/rules.xml");
        stack.push(a);
        assert_eq!(
            stack.resolve("data/rules.xml").path,
            "C:/steam/workshop/content/287450/1234/data/rules.xml"
        );
    }

    #[test]
    fn steam_tags_follow_the_shipped_tag_link_table() {
        let mut m = ModPackage::new("M", "M");
        m.declare_path("data/rules.xml");
        let tags = m.steam_tags();
        assert!(tags.contains(&WorkshopTag::Data));
        assert!(!tags.contains(&WorkshopTag::Other));

        let mut ai = ModPackage::new("A", "A");
        ai.declare_path("ai/scripts/economic.bhs");
        assert!(ai.steam_tags().contains(&WorkshopTag::Ai));

        let empty = ModPackage::new("E", "E");
        assert_eq!(
            empty.steam_tags().into_iter().collect::<Vec<_>>(),
            vec![WorkshopTag::Other]
        );
    }
}
