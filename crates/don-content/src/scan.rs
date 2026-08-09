//! Turning a directory on disk into a [`ModPackage`], the way retail does.
//!
//! `ModManager::buildModPackages` `0x00A221F0` enumerates the *subdirectories* of the
//! `MYMODS` storage point — `TFileSystem::FindAllMatchingFiles(L"*", MYMODS,
//! FindFile::MatchDirectories)` — and hands each one to `ModPackage::buildPackage`
//! `0x00A358B0`, which calls `ModPackage::buildCategory` `0x00A34DB0` for all twelve
//! categories and then keeps the package only if it found at least one file.
//!
//! `buildCategory(cat)` searches `<installDir>/<relativeDirectory(cat)>*` with flags
//! `MatchFiles | MatchAnyAttributes | SkipForbiddenFiles | ReturnPathSpec`, plus
//! `RecurseSubDirs` when `ModCategoryInfo[cat].recursive` — which is why `art`, `conquest`,
//! `scenario`, `sounds` and `terrain art` pick up subdirectories and `ai/scripts`, `data`,
//! `mapstyles`, `tribes`, `replays`, `saves` and the root do not. It stores each result
//! **relative to the category directory** and lowercased
//! (`SkyString::ConvertToLowerCase` `0x00A1F540`).
//!
//! One consequence that surprises modders: because `CAT_ROOT`'s relative directory is empty
//! and it is *not* recursive, a non-recursive category's own directory shows up as a root
//! entry only if it contains loose files, and files one level down in `data/` are invisible.

use std::collections::BTreeSet;
use std::io;
use std::path::Path;

use crate::vfs::{ModCategory, ModPackage, StorageLocation, ALL_CATEGORIES};

/// Scan a mod directory. `install_dir` is what goes in the package (the directory *name* for
/// a `MyMods` mod, the absolute path for a Workshop one); `root` is where to actually look.
pub fn scan_mod_dir(
    root: &Path,
    name: &str,
    install_dir: &str,
    location: StorageLocation,
) -> io::Result<ModPackage> {
    let mut m = ModPackage::new(name, install_dir);
    m.location = Some(location);
    for cat in ALL_CATEGORIES {
        let dir = if cat.relative_dir().is_empty() {
            root.to_path_buf()
        } else {
            root.join(cat.relative_dir().trim_end_matches('/'))
        };
        let mut found = BTreeSet::new();
        collect(&dir, "", cat.recursive(), &mut found)?;
        for f in found {
            m.declare(cat, &f);
        }
    }
    Ok(m)
}

fn collect(
    dir: &Path,
    prefix: &str,
    recursive: bool,
    out: &mut BTreeSet<String>,
) -> io::Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        let ty = entry.file_type()?;
        if ty.is_dir() {
            if recursive {
                collect(&entry.path(), &format!("{prefix}{name}/"), true, out)?;
            }
        } else if ty.is_file() {
            out.insert(format!("{prefix}{name}").to_ascii_lowercase());
        }
    }
    Ok(())
}

/// Scan every subdirectory of a `mods/` folder in host enumeration order. Retail likewise
/// keeps the order returned by `FindAllMatchingFiles`; neither API promises lexical order.
/// On a non-Windows host this default discovery order is not evidence of retail's order, so
/// load `mod-status.txt` and call [`crate::status::apply`] before treating precedence as fixed.
/// Packages with zero files are dropped, exactly as `buildPackage` does.
pub fn scan_mods_root(mods_root: &Path) -> io::Result<Vec<ModPackage>> {
    let mut names: Vec<String> = Vec::new();
    let entries = match std::fs::read_dir(mods_root) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    for e in entries {
        let e = e?;
        if e.file_type()?.is_dir() {
            names.push(e.file_name().to_string_lossy().to_string());
        }
    }
    let mut out = Vec::new();
    for n in names {
        let m = scan_mod_dir(&mods_root.join(&n), &n, &n, StorageLocation::MyMods)?;
        if m.file_count() > 0 {
            out.push(m);
        }
    }
    Ok(out)
}

/// The categories a scan actually found something in.
pub fn populated_categories(m: &ModPackage) -> Vec<ModCategory> {
    ALL_CATEGORIES
        .into_iter()
        .filter(|c| !m.files[c.index()].is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("don-content-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn recursion_follows_the_engines_per_category_flag() {
        let root = tmpdir("recursion");
        // data/ is NOT recursive; art/ IS.
        fs::create_dir_all(root.join("data/deeper")).unwrap();
        fs::write(root.join("data/rules.xml"), b"x").unwrap();
        fs::write(root.join("data/deeper/hidden.xml"), b"x").unwrap();
        fs::create_dir_all(root.join("art/units")).unwrap();
        fs::write(root.join("art/units/Guy.BH3"), b"x").unwrap();

        let m = scan_mod_dir(&root, "T", "T", StorageLocation::MyMods).unwrap();
        assert!(m.has_asset(ModCategory::Data, "rules.xml"));
        assert!(!m.has_asset(ModCategory::Data, "deeper/hidden.xml"));
        // recursive, and lowercased on the way in
        assert!(m.has_asset(ModCategory::Art, "units/guy.bh3"));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_scanned_mod_resolves_and_reports() {
        let root = tmpdir("resolve");
        fs::create_dir_all(root.join("data")).unwrap();
        fs::write(root.join("data/rules.xml"), b"x").unwrap();
        fs::write(root.join("info.xml"), b"x").unwrap();

        let m = scan_mod_dir(&root, "Demo", "Demo", StorageLocation::MyMods).unwrap();
        assert!(
            m.is_dropdown_mod(),
            "info.xml at root makes it a dropdown mod"
        );
        assert!(m.is_data_mod());
        assert_eq!(
            populated_categories(&m),
            vec![ModCategory::Data, ModCategory::Root]
        );

        let mut stack = crate::vfs::ContentStack::new();
        let mut m2 = m.clone();
        m2.dropdown_active = true;
        stack.push(m2);
        assert_eq!(
            stack.resolve("data/rules.xml").path,
            "mods/Demo/data/rules.xml"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_empty_directory_is_not_a_mod() {
        let root = tmpdir("empty");
        fs::create_dir_all(root.join("mods/Nothing")).unwrap();
        assert!(scan_mods_root(&root.join("mods")).unwrap().is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_missing_mods_root_is_not_an_error() {
        assert!(scan_mods_root(Path::new("/nonexistent/mods"))
            .unwrap()
            .is_empty());
    }
}
