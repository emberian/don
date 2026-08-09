//! `info.xml`, the metadata file that makes a package a dropdown mod.
//!
//! Presence remains the exact retail classifier: [`crate::vfs::ModPackage::is_dropdown_mod`]
//! only checks that the root file snapshot contains `info.xml`. This module is the separate
//! preflight a human needs before attempting to activate that package.
//!
//! The schema below is not community lore. `GameMod::init` `0x005A9AD0` looks up `INFO`,
//! `FILES`, `FILE`, `complete`, `checksum`, `name`, `version`, `size`, and `description`.
//! Those spellings were recovered from the live retail `StringTable` on 2026-08-09 and the
//! control flow is pinned in `re/decomp-all/005a9ad0.c`. Missing `FILES` calls
//! `GameMod::generate_file_list` `0x005A9030`; `complete == 0`, a missing `INFO`, or a missing
//! direct metadata `FILE` returns retail error `0x20`.
//!
//! We deliberately do not write or repair the file. Retail's manifest generator walks the
//! host filesystem and writes checksums whose exact per-file function is still unidentified.
//! Mutating a user's mod with a lookalike generator would be worse than a clear diagnostic.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

/// What the measured gates in `GameMod::init` would do with this structure.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RetailInfoGate {
    /// `INFO`, a direct `FILE`, and a non-zero-or-missing `FILES.complete` are present.
    #[default]
    Accepts,
    /// `FILES` is absent. Retail calls `generate_file_list`; this scanner refuses to mutate.
    WouldGenerateManifest,
    /// `FILES complete="0"` takes the `0x20` return.
    RejectsIncompleteManifest,
}

impl RetailInfoGate {
    pub fn label(self) -> &'static str {
        match self {
            Self::Accepts => "retail structural gate accepts",
            Self::WouldGenerateManifest => "retail would generate FILES (not reproduced)",
            Self::RejectsIncompleteManifest => "retail rejects: FILES complete=0",
        }
    }
}

/// One file generated beneath `INFO/FILES`. The recovered generator writes `path`, `size`,
/// and `directory`; the checksum is on the `FILES` node, not on each generated entry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ManifestEntry {
    pub path: String,
    pub size: Option<i64>,
    pub directory: Option<bool>,
}

/// Parsed dropdown metadata and the generated-file snapshot, when supplied.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DropdownInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub size: Option<i64>,
    pub checksum: Option<i64>,
    pub files_complete: Option<i64>,
    pub files_checksum: Option<i64>,
    /// `GameMod::generate_file_list` stores the wrapping sum of entry sizes on `FILES`.
    pub files_size: Option<i64>,
    pub manifest: Vec<ManifestEntry>,
    pub gate: RetailInfoGate,
    /// Non-fatal quality findings. Retail defaults missing strings to empty and integers to
    /// `-1`; DoN shows that instead of pretending the metadata is complete.
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub enum InfoError {
    Io(std::io::Error),
    Xml(String),
    MissingInfoRoot,
    MissingMetadataFile,
    DuplicateElement(&'static str),
    InvalidInteger { field: String, value: String },
    InvalidBoolean { field: String, value: String },
}

impl fmt::Display for InfoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "cannot read info.xml: {e}"),
            Self::Xml(e) => write!(f, "malformed info.xml: {e}"),
            Self::MissingInfoRoot => write!(f, "info.xml has no INFO root element"),
            Self::MissingMetadataFile => {
                write!(
                    f,
                    "INFO has no direct FILE metadata element (retail returns 0x20)"
                )
            }
            Self::DuplicateElement(e) => {
                write!(f, "info.xml contains more than one direct {e} element")
            }
            Self::InvalidInteger { field, value } => {
                write!(f, "{field} must be an integer, got {value:?}")
            }
            Self::InvalidBoolean { field, value } => {
                write!(f, "{field} must be 0 or 1, got {value:?}")
            }
        }
    }
}

impl std::error::Error for InfoError {}

pub fn read_info(path: &Path) -> Result<DropdownInfo, InfoError> {
    let text = std::fs::read_to_string(path).map_err(InfoError::Io)?;
    parse_info(&text)
}

pub fn parse_info(text: &str) -> Result<DropdownInfo, InfoError> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut stack: Vec<Vec<u8>> = Vec::new();
    let mut saw_info = false;
    let mut saw_metadata = false;
    let mut saw_files = false;
    let mut out = DropdownInfo::default();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                visit(
                    &reader,
                    &stack,
                    &e,
                    &mut out,
                    &mut saw_info,
                    &mut saw_metadata,
                    &mut saw_files,
                )?;
                stack.push(e.name().as_ref().to_vec());
            }
            Ok(Event::Empty(e)) => {
                visit(
                    &reader,
                    &stack,
                    &e,
                    &mut out,
                    &mut saw_info,
                    &mut saw_metadata,
                    &mut saw_files,
                )?;
            }
            Ok(Event::End(_)) => {
                stack.pop();
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(InfoError::Xml(e.to_string())),
        }
    }

    if !saw_info {
        return Err(InfoError::MissingInfoRoot);
    }
    if !saw_metadata {
        return Err(InfoError::MissingMetadataFile);
    }
    out.gate = if !saw_files {
        RetailInfoGate::WouldGenerateManifest
    } else if out.files_complete == Some(0) {
        RetailInfoGate::RejectsIncompleteManifest
    } else {
        RetailInfoGate::Accepts
    };
    for (label, missing) in [
        ("FILE.name", out.name.is_empty()),
        ("FILE.version", out.version.is_empty()),
        ("FILE.description", out.description.is_empty()),
        ("FILE.size", out.size.is_none()),
        ("FILE.checksum", out.checksum.is_none()),
    ] {
        if missing {
            out.warnings.push(format!(
                "{label} is missing; retail uses its empty/-1 default"
            ));
        }
    }
    Ok(out)
}

fn visit(
    reader: &Reader<&[u8]>,
    stack: &[Vec<u8>],
    e: &BytesStart<'_>,
    out: &mut DropdownInfo,
    saw_info: &mut bool,
    saw_metadata: &mut bool,
    saw_files: &mut bool,
) -> Result<(), InfoError> {
    let name = e.name();
    let tag = name.as_ref();
    if stack.is_empty() {
        if tag == b"INFO" {
            *saw_info = true;
        }
        return Ok(());
    }
    if stack.first().map(Vec::as_slice) != Some(b"INFO") {
        return Ok(());
    }
    let attrs = attributes(reader, e)?;
    if stack.len() == 1 && tag == b"FILE" {
        if *saw_metadata {
            return Err(InfoError::DuplicateElement("INFO/FILE"));
        }
        *saw_metadata = true;
        out.name = attrs.get("name").cloned().unwrap_or_default();
        out.version = attrs.get("version").cloned().unwrap_or_default();
        out.description = attrs.get("description").cloned().unwrap_or_default();
        out.size = optional_i64(&attrs, "size", "FILE.size")?;
        out.checksum = optional_i64(&attrs, "checksum", "FILE.checksum")?;
    } else if stack.len() == 1 && tag == b"FILES" {
        if *saw_files {
            return Err(InfoError::DuplicateElement("INFO/FILES"));
        }
        *saw_files = true;
        out.files_complete = optional_i64(&attrs, "complete", "FILES.complete")?;
        out.files_checksum = optional_i64(&attrs, "checksum", "FILES.checksum")?;
        out.files_size = optional_i64(&attrs, "size", "FILES.size")?;
    } else if stack.len() == 2 && stack[1].as_slice() == b"FILES" && tag == b"FILE" {
        out.manifest.push(ManifestEntry {
            path: attrs.get("path").cloned().unwrap_or_default(),
            size: optional_i64(&attrs, "size", "FILES/FILE.size")?,
            directory: optional_bool(&attrs, "directory", "FILES/FILE.directory")?,
        });
    }
    Ok(())
}

fn attributes(
    reader: &Reader<&[u8]>,
    e: &BytesStart<'_>,
) -> Result<BTreeMap<String, String>, InfoError> {
    let mut out = BTreeMap::new();
    for attr in e.attributes() {
        let attr = attr.map_err(|e| InfoError::Xml(e.to_string()))?;
        let key = std::str::from_utf8(attr.key.as_ref())
            .map_err(|e| InfoError::Xml(e.to_string()))?
            .to_string();
        let value = attr
            .decode_and_unescape_value(reader.decoder())
            .map_err(|e| InfoError::Xml(e.to_string()))?
            .into_owned();
        out.insert(key, value);
    }
    Ok(out)
}

fn optional_i64(
    attrs: &BTreeMap<String, String>,
    key: &str,
    field: &str,
) -> Result<Option<i64>, InfoError> {
    attrs
        .get(key)
        .map(|v| {
            v.parse().map_err(|_| InfoError::InvalidInteger {
                field: field.to_string(),
                value: v.clone(),
            })
        })
        .transpose()
}

fn optional_bool(
    attrs: &BTreeMap<String, String>,
    key: &str,
    field: &str,
) -> Result<Option<bool>, InfoError> {
    attrs
        .get(key)
        .map(|v| match v.as_str() {
            "0" => Ok(false),
            "1" => Ok(true),
            _ => Err(InfoError::InvalidBoolean {
                field: field.to_string(),
                value: v.clone(),
            }),
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_measured_info_and_files_shape() {
        let x = r#"<?xml version="1.0"?>
<INFO>
  <FILE name="Clockwork" version="2" description="Measured metadata" size="4096" checksum="17"/>
  <FILES complete="1" checksum="99" size="12">
    <FILE path="data/rules.xml" size="12" directory="0"/>
  </FILES>
</INFO>"#;
        let i = parse_info(x).unwrap();
        assert_eq!(i.gate, RetailInfoGate::Accepts);
        assert_eq!(i.name, "Clockwork");
        assert_eq!(i.files_checksum, Some(99));
        assert_eq!(i.files_size, Some(12));
        assert_eq!(i.manifest[0].path, "data/rules.xml");
        assert!(i.warnings.is_empty());
    }

    #[test]
    fn files_absence_is_not_silently_treated_as_complete() {
        let i = parse_info(r#"<INFO><FILE name="M"/></INFO>"#).unwrap();
        assert_eq!(i.gate, RetailInfoGate::WouldGenerateManifest);
        assert!(i.warnings.iter().any(|w| w.contains("version")));
    }

    #[test]
    fn incomplete_and_missing_metadata_follow_the_retail_gates() {
        let i = parse_info(r#"<INFO><FILE name="M"/><FILES complete="0"/></INFO>"#).unwrap();
        assert_eq!(i.gate, RetailInfoGate::RejectsIncompleteManifest);
        assert!(matches!(
            parse_info(r#"<INFO><FILES complete="1"/></INFO>"#),
            Err(InfoError::MissingMetadataFile)
        ));
    }
}
