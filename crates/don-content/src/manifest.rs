//! Retail `GameMod` manifests and XML-content checksums.
//!
//! This is the recovered implementation boundary, including its awkward edges:
//!
//! * `GameMod::generate_file_list` (`0x005A9030`) changes to the package directory and walks
//!   `*.*` recursively. It lowercases each basename only for the exclusion test, skips `.`,
//!   `..`, and `info.xml`, and records `path`, 32-bit `size`, and `directory` on every other
//!   file-system entry. `FILES.size` is the wrapping sum of the recorded sizes.
//! * `GameMod::compute_checksum` (`0x005A94A0`) independently walks `\*.xml` recursively,
//!   raw-opens every match, and wrapping-adds `File::get_checksum` (`0x00A2DF30`). File order
//!   therefore does not affect the checksum.
//! * `File::get_checksum` calls the shipped zlib `adler32` (`0x005089D0`) with seed zero in
//!   four-byte reads. It passes a length of four even for the last short read, retaining bytes
//!   from the preceding word. For a whole file shorter than four bytes those retained bytes
//!   are uninitialised stack data. The safe implementation refuses that unknowable case.
//!
//! `_wfindfirst` order is not a portable contract. [`generate`] emits the same entry set in a
//! stable case-folded path order and labels that order as the independent edition's canonical
//! serialisation; it does not claim to reproduce a particular NTFS enumeration.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::info::DropdownInfo;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestEntry {
    /// Package-relative path using the backslashes `RecursiveFileSearch::get_name` emits.
    pub path: String,
    pub size: i32,
    pub directory: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChecksummedFile {
    pub path: String,
    pub size: u64,
    pub checksum: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetailManifest {
    /// Canonically ordered independent-edition serialisation of the retail entry set.
    pub entries: Vec<ManifestEntry>,
    /// Retail's signed 32-bit wrapping accumulation over entry sizes.
    pub total_size: i32,
    /// Retail's unsigned 32-bit wrapping sum over the XML file checksums.
    pub checksum: u32,
    /// Canonically ordered evidence behind `checksum`.
    pub checksummed_files: Vec<ChecksummedFile>,
}

impl RetailManifest {
    /// Stable `FILES` fragment for review/tooling. Retail writes through MSXML, so whitespace
    /// and attribute formatting are not asserted byte-identical; values and entry membership
    /// are the recovered semantics.
    pub fn canonical_files_xml(&self) -> String {
        let mut out = format!(
            "<FILES complete=\"1\" checksum=\"{}\" size=\"{}\">\n",
            self.checksum, self.total_size
        );
        for entry in &self.entries {
            out.push_str("  <FILE path=\"");
            push_xml_escaped(&mut out, &entry.path);
            out.push_str(&format!(
                "\" size=\"{}\" directory=\"{}\"/>\n",
                entry.size,
                i32::from(entry.directory)
            ));
        }
        out.push_str("</FILES>\n");
        out
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestMismatch {
    MissingFilesSize,
    FilesSize {
        declared: i64,
        current: i32,
    },
    DuplicateDeclaredPath(String),
    MissingEntry(String),
    UnexpectedEntry(String),
    EntrySize {
        path: String,
        declared: Option<i64>,
        current: i32,
    },
    EntryDirectory {
        path: String,
        declared: Option<bool>,
        current: bool,
    },
}

impl fmt::Display for ManifestMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingFilesSize => write!(f, "FILES.size is missing"),
            Self::FilesSize { declared, current } => {
                write!(
                    f,
                    "FILES.size is {declared}, current entry sum is {current}"
                )
            }
            Self::DuplicateDeclaredPath(path) => {
                write!(f, "FILES repeats Windows-equivalent path `{path}`")
            }
            Self::MissingEntry(path) => write!(f, "FILES is missing current entry `{path}`"),
            Self::UnexpectedEntry(path) => {
                write!(f, "FILES declares absent entry `{path}`")
            }
            Self::EntrySize {
                path,
                declared,
                current,
            } => write!(
                f,
                "FILES entry `{path}` size is {declared:?}, current size is {current}"
            ),
            Self::EntryDirectory {
                path,
                declared,
                current,
            } => write!(
                f,
                "FILES entry `{path}` directory is {declared:?}, current value is {current}"
            ),
        }
    }
}

/// Compare the reproducible entry set with a stored `INFO/FILES` snapshot. The stored
/// checksum is intentionally not compared to the current checksum: retail computes it before
/// adding the generated `FILES` node and then saves the changed `info.xml`, so recomputing over
/// the saved tree is not a valid equality check.
pub fn compare_declared(info: &DropdownInfo, current: &RetailManifest) -> Vec<ManifestMismatch> {
    let mut out = Vec::new();
    match info.files_size {
        None => out.push(ManifestMismatch::MissingFilesSize),
        Some(size) if size != i64::from(current.total_size) => {
            out.push(ManifestMismatch::FilesSize {
                declared: size,
                current: current.total_size,
            })
        }
        Some(_) => {}
    }
    let mut declared = BTreeMap::<String, &crate::info::ManifestEntry>::new();
    for entry in &info.manifest {
        let key = entry.path.replace('\\', "/").to_ascii_lowercase();
        if declared.insert(key, entry).is_some() {
            out.push(ManifestMismatch::DuplicateDeclaredPath(entry.path.clone()));
        }
    }
    let current_by_path: BTreeMap<String, &ManifestEntry> = current
        .entries
        .iter()
        .map(|entry| (entry.path.replace('\\', "/").to_ascii_lowercase(), entry))
        .collect();
    for (key, entry) in &current_by_path {
        let Some(stored) = declared.get(key) else {
            out.push(ManifestMismatch::MissingEntry(entry.path.clone()));
            continue;
        };
        if stored.size != Some(i64::from(entry.size)) {
            out.push(ManifestMismatch::EntrySize {
                path: entry.path.clone(),
                declared: stored.size,
                current: entry.size,
            });
        }
        if stored.directory != Some(entry.directory) {
            out.push(ManifestMismatch::EntryDirectory {
                path: entry.path.clone(),
                declared: stored.directory,
                current: entry.directory,
            });
        }
    }
    for (key, entry) in declared {
        if !current_by_path.contains_key(&key) {
            out.push(ManifestMismatch::UnexpectedEntry(entry.path.clone()));
        }
    }
    out
}

#[derive(Debug)]
pub enum ManifestError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    NonUnicodePath(PathBuf),
    Symlink(PathBuf),
    CaseCollision {
        first: PathBuf,
        second: PathBuf,
    },
    EntryTooLarge {
        path: PathBuf,
        size: u64,
    },
    /// Retail hashes four bytes after a one-to-three-byte first read. Those extra stack bytes
    /// have no file-derived value and must not be replaced with guessed zero padding.
    IndeterminateShortXml {
        path: PathBuf,
        size: u64,
    },
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::NonUnicodePath(path) => write!(
                f,
                "{} is not Unicode; no evidenced SkyString conversion is available",
                path.display()
            ),
            Self::Symlink(path) => write!(
                f,
                "{} is a symlink; Windows RecursiveFileSearch junction semantics are not reproduced",
                path.display()
            ),
            Self::CaseCollision { first, second } => write!(
                f,
                "Windows-case-equivalent paths collide: {} and {}",
                first.display(),
                second.display()
            ),
            Self::EntryTooLarge { path, size } => write!(
                f,
                "{} is {size} bytes; retail stores manifest sizes through a signed 32-bit XML attribute",
                path.display()
            ),
            Self::IndeterminateShortXml { path, size } => write!(
                f,
                "{} is {size} byte(s): retail File::get_checksum hashes uninitialised tail bytes for an XML file shorter than four bytes",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ManifestError {}

/// Generate the recovered entry set and XML checksum using a canonical, reproducible order.
pub fn generate(root: &Path) -> Result<RetailManifest, ManifestError> {
    let mut paths = Vec::new();
    collect(root, root, &mut paths)?;
    paths.sort_by(|a, b| {
        canonical_key(&a.0)
            .cmp(&canonical_key(&b.0))
            .then(a.0.cmp(&b.0))
    });

    let mut seen = BTreeMap::<String, PathBuf>::new();
    for (rel, absolute, _) in &paths {
        let key = canonical_key(rel);
        if let Some(first) = seen.insert(key, absolute.clone()) {
            return Err(ManifestError::CaseCollision {
                first,
                second: absolute.clone(),
            });
        }
    }

    let mut entries = Vec::new();
    let mut checksummed_files = Vec::new();
    let mut total_size = 0i32;
    let mut checksum = 0u32;
    for (rel, absolute, meta) in paths {
        let path = retail_path(&rel)?;
        let basename = rel
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| ManifestError::NonUnicodePath(absolute.clone()))?;
        let excluded = matches!(
            basename.to_ascii_lowercase().as_str(),
            "." | ".." | "info.xml"
        );
        // `_wfinddata64i32_t.size` is zero for directory records; host directory metadata
        // sizes are filesystem bookkeeping and must not leak into a retail manifest.
        let size = if meta.is_dir() { 0 } else { meta.len() };
        if !excluded {
            let size_i32 = i32::try_from(size).map_err(|_| ManifestError::EntryTooLarge {
                path: absolute.clone(),
                size,
            })?;
            total_size = total_size.wrapping_add(size_i32);
            entries.push(ManifestEntry {
                path: path.clone(),
                size: size_i32,
                directory: meta.is_dir(),
            });
        }
        if meta.is_file() && has_xml_extension(&rel) {
            let bytes = fs::read(&absolute).map_err(|source| ManifestError::Io {
                path: absolute.clone(),
                source,
            })?;
            let file_checksum = retail_file_checksum(&bytes).ok_or_else(|| {
                ManifestError::IndeterminateShortXml {
                    path: absolute.clone(),
                    size,
                }
            })?;
            checksum = checksum.wrapping_add(file_checksum);
            checksummed_files.push(ChecksummedFile {
                path,
                size,
                checksum: file_checksum,
            });
        }
    }
    Ok(RetailManifest {
        entries,
        total_size,
        checksum,
        checksummed_files,
    })
}

/// Exact for empty files and files of at least four bytes. Returns `None` for a one-to-three
/// byte whole file because retail consumes uninitialised stack bytes in that case.
pub fn retail_file_checksum(bytes: &[u8]) -> Option<u32> {
    if (1..4).contains(&bytes.len()) {
        return None;
    }
    if bytes.is_empty() {
        return Some(0);
    }
    let mut checksum = 0u32;
    let mut word = [0u8; 4];
    for chunk in bytes.chunks(4) {
        word[..chunk.len()].copy_from_slice(chunk);
        checksum = adler32(checksum, &word);
    }
    Some(checksum)
}

fn adler32(adler: u32, bytes: &[u8]) -> u32 {
    const BASE: u32 = 65_521;
    let mut s1 = adler & 0xffff;
    let mut s2 = adler >> 16;
    for &byte in bytes {
        s1 = s1.wrapping_add(u32::from(byte));
        s2 = s2.wrapping_add(s1);
    }
    ((s2 % BASE) << 16) | (s1 % BASE)
}

fn collect(
    root: &Path,
    directory: &Path,
    out: &mut Vec<(PathBuf, PathBuf, fs::Metadata)>,
) -> Result<(), ManifestError> {
    let iter = fs::read_dir(directory).map_err(|source| ManifestError::Io {
        path: directory.to_path_buf(),
        source,
    })?;
    for item in iter {
        let item = item.map_err(|source| ManifestError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
        let absolute = item.path();
        let file_type = item.file_type().map_err(|source| ManifestError::Io {
            path: absolute.clone(),
            source,
        })?;
        if file_type.is_symlink() {
            return Err(ManifestError::Symlink(absolute));
        }
        let metadata = item.metadata().map_err(|source| ManifestError::Io {
            path: absolute.clone(),
            source,
        })?;
        let rel = absolute
            .strip_prefix(root)
            .expect("recursive child remains beneath root")
            .to_path_buf();
        out.push((rel, absolute.clone(), metadata));
        if file_type.is_dir() {
            collect(root, &absolute, out)?;
        }
    }
    Ok(())
}

fn has_xml_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("xml"))
}

fn retail_path(path: &Path) -> Result<String, ManifestError> {
    path.to_str()
        .map(|p| p.replace('/', "\\"))
        .ok_or_else(|| ManifestError::NonUnicodePath(path.to_path_buf()))
}

fn canonical_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase()
}

fn push_xml_escaped(out: &mut String, value: &str) {
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "don-content-manifest-{name}-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn file_checksum_is_seed_zero_and_retains_the_previous_short_tail() {
        assert_eq!(retail_file_checksum(b""), Some(0));
        assert_eq!(retail_file_checksum(b"abcd"), Some(0x03d4_018a));
        assert_eq!(retail_file_checksum(b"abcde"), Some(0x0de0_0318));
        assert_eq!(retail_file_checksum(b"abcdef"), Some(0x0dec_031c));
        assert_eq!(retail_file_checksum(b"abcdefg"), Some(0x0df4_0320));
        assert_eq!(retail_file_checksum(b"abcdefgh"), Some(0x0df8_0324));
        assert_eq!(retail_file_checksum(b"x"), None);
    }

    #[test]
    fn canonical_manifest_has_the_retail_entry_set_and_xml_only_checksum() {
        let root = temp_dir("tree");
        fs::create_dir(root.join("Data")).unwrap();
        fs::write(root.join("info.xml"), b"<INFO/>\n").unwrap();
        fs::write(root.join("Data/rules.XML"), b"abcdefgh").unwrap();
        fs::write(root.join("Data/readme.txt"), b"not checksummed").unwrap();

        let first = generate(&root).unwrap();
        let second = generate(&root).unwrap();
        assert_eq!(first, second);
        assert!(first.entries.iter().all(|e| e.path != "info.xml"));
        assert!(first
            .entries
            .iter()
            .any(|e| e.path == "Data" && e.directory));
        assert!(first
            .entries
            .iter()
            .any(|e| e.path == "Data\\rules.XML" && !e.directory));
        assert_eq!(first.checksummed_files.len(), 2);
        assert_eq!(
            first.checksum,
            first
                .checksummed_files
                .iter()
                .fold(0u32, |sum, file| sum.wrapping_add(file.checksum))
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn one_to_three_byte_xml_is_a_reported_retail_indeterminacy() {
        let root = temp_dir("short");
        let mut file = fs::File::create(root.join("tiny.xml")).unwrap();
        file.write_all(b"x").unwrap();
        assert!(matches!(
            generate(&root),
            Err(ManifestError::IndeterminateShortXml { size: 1, .. })
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn declared_snapshot_comparison_reports_every_membership_and_metadata_difference() {
        let info = crate::info::parse_info(
            r#"<INFO><FILE name="x"/><FILES complete="1" checksum="9" size="7"><FILE path="data\rules.xml" size="6" directory="1"/><FILE path="gone.xml" size="1" directory="0"/></FILES></INFO>"#,
        )
        .unwrap();
        let current = RetailManifest {
            entries: vec![ManifestEntry {
                path: "Data\\rules.xml".into(),
                size: 8,
                directory: false,
            }],
            total_size: 8,
            checksum: 123,
            checksummed_files: Vec::new(),
        };
        let differences = compare_declared(&info, &current);
        assert_eq!(differences.len(), 4);
        assert!(differences
            .iter()
            .any(|difference| matches!(difference, ManifestMismatch::FilesSize { .. })));
        assert!(differences
            .iter()
            .any(|difference| matches!(difference, ManifestMismatch::EntrySize { .. })));
        assert!(differences
            .iter()
            .any(|difference| matches!(difference, ManifestMismatch::EntryDirectory { .. })));
        assert!(differences
            .iter()
            .any(|difference| matches!(difference, ManifestMismatch::UnexpectedEntry(_))));
    }
}
