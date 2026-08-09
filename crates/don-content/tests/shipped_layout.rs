//! Cross-checks against content that actually ships, not against our own model.
//!
//! Two corpora:
//!
//! * `ron-data/` on this machine — the shipped `Data\` tree, gitignored as copyrighted game
//!   content, so these tests **skip** rather than fail when it is absent.
//! * A capture of the retail install's `mapstyles\` directory, taken from the Parallels guest
//!   on 2026-08-08 with
//!   `prlctl exec "Windows 11" cmd.exe /c dir /b "...\Rise of Nations\mapstyles"`.
//!   Written down, not computed.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use don_content::compat::Support;
use don_content::generated::FORBIDDEN_MAPSTYLES;
use don_content::vfs::{ALL_CATEGORIES, CATEGORY_MAX};
use don_content::{classify, ContentStack, ModCategory, ModPackage};

fn repo_root() -> PathBuf {
    // crates/don-content/tests -> repo root
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// `dir /b "C:\Program Files (x86)\Steam\steamapps\common\Rise of Nations\mapstyles"`,
/// captured 2026-08-08 from the retail install [measured].
const SHIPPED_MAPSTYLES: [&str; 22] = [
    "AFRICA.xml",
    "AMAZON.xml",
    "BRITISHISLES.xml",
    "COLONIALPOWERS.xml",
    "CONQUESTMAP.xml",
    "CONQUESTMAPSEA.xml",
    "default.xml",
    "EASTINDIES.xml",
    "EASTWEST.xml",
    "EDITORMAP.xml",
    "greatlakes_trial.xml",
    "GREATLAKES.xml",
    "HIMALAYAS.xml",
    "MEDITERRANEAN.xml",
    "NEWWORLD.xml",
    "NILEDELTA.xml",
    "OLDWORLD.xml",
    "OUTBACK.xml",
    "SAHARA.xml",
    "SEAPOWER.xml",
    "SOUTHWESTMESA.xml",
    "WARRINGSTATES.xml",
];

/// The captured protection list and the captured install listing **do not agree**, and the
/// disagreement is a shipped defect rather than a capture error. Pinning it here means it
/// cannot be quietly "fixed" by editing the table, and it is the exact statement the
/// improved-mode lane needs.
///
/// `ModManager::isMapForbidden` `0x00A21140` holds 21 names; `mapstyles\` holds 22 files.
/// Two of the 22 are unprotected:
///
/// * `GREATLAKES.xml` — simply absent from the list, while `greatlakes_trial.xml` is present.
/// * `SOUTHWESTMESA.xml` — the list spells it `soutwestmesa.xml`, missing the `h`, so the
///   comparison never matches and the entry protects a file that does not exist.
#[test]
fn the_forbidden_map_list_does_not_cover_the_shipped_map_styles() {
    let shipped: BTreeSet<String> = SHIPPED_MAPSTYLES
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();
    let forbidden: BTreeSet<String> = FORBIDDEN_MAPSTYLES.iter().map(|s| s.to_string()).collect();

    assert_eq!(forbidden.len(), 21, "isMapForbidden holds 21 literals");
    assert_eq!(shipped.len(), 22, "the install holds 22 map styles");

    let unprotected: BTreeSet<_> = shipped.difference(&forbidden).cloned().collect();
    assert_eq!(
        unprotected,
        BTreeSet::from([
            "greatlakes.xml".to_string(),
            "southwestmesa.xml".to_string()
        ]),
        "exactly these shipped map styles are overridable"
    );

    let protects_nothing: BTreeSet<_> = forbidden.difference(&shipped).cloned().collect();
    assert_eq!(
        protects_nothing,
        BTreeSet::from(["soutwestmesa.xml".to_string()]),
        "and this entry is a typo that protects no file"
    );

    // And the consequence, through our resolver: a mod CAN replace the two.
    let mut stack = ContentStack::new();
    let mut m = ModPackage::new("MapPack", "MapPack");
    for f in SHIPPED_MAPSTYLES {
        m.declare_path(&format!("mapstyles/{f}"));
    }
    stack.push(m);
    let overridable: Vec<&str> = SHIPPED_MAPSTYLES
        .iter()
        .filter(|f| stack.resolve(&format!("mapstyles/{f}")).mod_index != 0)
        .copied()
        .collect();
    assert_eq!(overridable, vec!["GREATLAKES.xml", "SOUTHWESTMESA.xml"]);
}

/// Every category prefix must be unambiguous under the engine's first-hit scan: no prefix may
/// itself be a prefix of a *later* category's prefix, or that later category would be
/// unreachable. This is a property of the captured `s_ModCategoryInfo`, and it could have been
/// false.
#[test]
fn category_prefixes_do_not_shadow_each_other() {
    for (i, a) in ALL_CATEGORIES.iter().enumerate() {
        for b in ALL_CATEGORIES.iter().skip(i + 1) {
            let (pa, pb) = (a.relative_dir(), b.relative_dir());
            if pa.is_empty() {
                panic!("only CAT_ROOT may have an empty prefix, and it must be last");
            }
            assert!(!pb.starts_with(pa), "{:?} ({pa}) shadows {:?} ({pb})", a, b);
        }
    }
    assert_eq!(ALL_CATEGORIES[CATEGORY_MAX - 1], ModCategory::Root);
    assert!(ModCategory::Root.relative_dir().is_empty());
}

/// Round-trip the shipped `Data\` tree through classify/resolve. Skips when `ron-data/` is
/// not present (it is gitignored).
#[test]
fn the_shipped_data_tree_round_trips() {
    let dir = repo_root().join("ron-data");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("skip: {} not present", dir.display());
        return;
    };

    let mut names: Vec<String> = Vec::new();
    for e in entries.flatten() {
        if e.file_type().map(|t| t.is_file()).unwrap_or(false) {
            let n = e.file_name().to_string_lossy().to_string();
            if n.ends_with(".xml") || n.ends_with(".dtd") || n.ends_with(".sps") {
                names.push(n);
            }
        }
    }
    assert!(
        names.len() > 20,
        "expected the shipped Data tree, found {}",
        names.len()
    );

    let mut m = ModPackage::new("Everything", "Everything");
    for n in &names {
        let path = format!("data/{n}");
        let (cat, file) = classify(&path);
        assert_eq!(cat, ModCategory::Data, "{path}");
        assert_eq!(&file, n, "{path}");
        // and the shipped path is reconstructible from (category, filename)
        assert_eq!(
            format!("{}{}", cat.relative_dir(), file),
            path.to_ascii_lowercase()
        );
        m.declare_path(&path);
    }

    assert!(m.is_data_mod());
    assert!(!m.is_dropdown_mod());
    assert_eq!(m.file_count(), names.len());

    // With that mod installed, every one of those files resolves into it and nowhere else.
    let mut stack = ContentStack::new();
    stack.push(m);
    for n in &names {
        let r = stack.resolve(&format!("data/{n}"));
        assert_eq!(r.mod_index, 1, "data/{n}");
        assert_eq!(
            r.path,
            format!("mods/Everything/data/{}", n.to_ascii_lowercase())
        );
    }
    // and a file it does not declare still falls through to shipped
    assert_eq!(stack.resolve("data/not-a-real-file.xml").mod_index, 0);
}

/// The shipped AI scripts are the other half of the ruleset a data mod can touch, and they
/// live under a *two-segment* category prefix that a naive `first path segment` classifier
/// would get wrong.
#[test]
fn the_shipped_ai_scripts_classify_as_cat_ai() {
    let dir = repo_root().join("ron-data").join("ai-scripts");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("skip: {} not present", dir.display());
        return;
    };
    let mut n = 0;
    let mut m = ModPackage::new("AI", "AI");
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.ends_with(".bhs") {
            continue;
        }
        let path = format!("ai/scripts/{name}");
        assert_eq!(classify(&path), (ModCategory::Ai, name.clone()));
        m.declare_path(&path);
        n += 1;
    }
    assert!(n > 0, "expected shipped .bhs scripts");
    assert!(m.steam_tags().contains(&don_content::WorkshopTag::Ai));

    // Every one of them is `Parsed`, not `Consumed` — the BHS interpreter is a live lane.
    let r = don_content::compat_report(&m);
    assert_eq!(r.count(Support::Parsed), n);
    assert_eq!(r.count(Support::Consumed), 0);
}
