//! Retail `StringTable` loading and localized XML ownership.
//!
//! This module is intentionally narrower than "numeric-extension support". `XML::init`
//! (`0x00A279E0`) applies localization to every XML consumer, while `StringTable::init`
//! (`0x00A28520`) owns only `internal_strings.xml` and `translated_strings.xml`. The path
//! order is load-bearing: resolve the unsuffixed file first, append the current language
//! suffix to that physical winner, and fall back to that same winner. A suffix is never
//! resolved independently through the mod stack.
//!
//! Retail startup loads both tables before `readModStatus` enables packages, so both startup
//! owners are shipped. `OptionsWinPlayer::change_language` (`0x00832A30`) later reloads only
//! `translated_strings.xml`; the internal table remains pinned. [`StringTableRegistry`]
//! preserves that lifecycle while improving the mutation boundary: decoding, structural
//! validation, ordinal-identity validation, and I/O all happen in `prepare_*`, followed by
//! one generation-checked `Arc` replacement in [`StringTableRegistry::commit`].
//!
//! The exact five-value language table comes from `LocalizationManager::GetLanguageSuffix`
//! (`0x00A42830`, pointer table `0x00B1CA54`) and its language-code table at `0x00B1CA80`.
//! The install also contains `.4`, `.9`, `.17`, and `.18` siblings, but this binary's enum
//! cannot select them. They are not silently assigned invented language names here.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use quick_xml::escape::unescape;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::manifest::retail_file_checksum;
use crate::scan::{find_windows_dir, find_windows_file};
use crate::workflow::{ActivationPlan, ResolutionOutcome};

pub const INTERNAL_STRINGS: &str = "internal_strings.xml";
pub const TRANSLATED_STRINGS: &str = "translated_strings.xml";

/// `LocalizationManager::Language` in retail discriminant order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RetailLanguage {
    German = 0,
    English = 1,
    Spanish = 2,
    French = 3,
    Italian = 4,
}

impl RetailLanguage {
    pub const ALL: [Self; 5] = [
        Self::German,
        Self::English,
        Self::Spanish,
        Self::French,
        Self::Italian,
    ];

    pub const fn code(self) -> &'static str {
        match self {
            Self::German => "DE",
            Self::English => "EN",
            Self::Spanish => "ES",
            Self::French => "FR",
            Self::Italian => "IT",
        }
    }

    pub const fn suffix(self) -> &'static str {
        match self {
            Self::German => ".7",
            Self::English => "",
            Self::Spanish => ".10",
            Self::French => ".12",
            Self::Italian => ".16",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|language| language.code().eq_ignore_ascii_case(code))
    }
}

impl fmt::Display for RetailLanguage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// One ordinal entry. Retail identity is the array position (`ordinal * 0x14`), not `hash`.
/// The declared hash is retained so a translated replacement can prove that every ordinal
/// still names the same resource before it is registered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StringRecord {
    pub text: String,
    pub declared_hash: i32,
    pub needed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetailStringTable {
    internal: bool,
    records: Box<[StringRecord]>,
}

impl RetailStringTable {
    pub fn is_internal(&self) -> bool {
        self.internal
    }

    pub fn records(&self) -> &[StringRecord] {
        &self.records
    }

    pub fn get(&self, ordinal: usize) -> Option<&str> {
        self.records.get(ordinal).map(|record| record.text.as_str())
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StringXmlError {
    Xml(String),
    MissingRoot,
    DuplicateRoot,
    UnexpectedElement(String),
    UnexpectedText(String),
    MissingAttribute {
        element: String,
        attribute: String,
    },
    UnexpectedAttribute {
        element: String,
        attribute: String,
    },
    InvalidAttribute {
        element: String,
        attribute: String,
        value: String,
    },
    NestedStringElement(String),
    UnsupportedXmlConstruct(String),
    EmptyTable,
}

impl fmt::Display for StringXmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Xml(error) => write!(f, "XML: {error}"),
            Self::MissingRoot => write!(f, "missing ROOT element"),
            Self::DuplicateRoot => write!(f, "more than one ROOT element"),
            Self::UnexpectedElement(element) => write!(f, "unexpected element `{element}`"),
            Self::UnexpectedText(text) => write!(f, "text outside STRING: {text:?}"),
            Self::MissingAttribute { element, attribute } => {
                write!(f, "`{element}` is missing attribute `{attribute}`")
            }
            Self::UnexpectedAttribute { element, attribute } => {
                write!(f, "`{element}` has unexpected attribute `{attribute}`")
            }
            Self::InvalidAttribute {
                element,
                attribute,
                value,
            } => write!(f, "`{element}.{attribute}` has invalid value {value:?}"),
            Self::NestedStringElement(element) => {
                write!(f, "STRING contains nested element `{element}`")
            }
            Self::UnsupportedXmlConstruct(kind) => {
                write!(f, "unsupported XML construct `{kind}`")
            }
            Self::EmptyTable => write!(f, "StringTable contains no STRING entries"),
        }
    }
}

impl std::error::Error for StringXmlError {}

/// Parse the measured StringTable shape without borrowing `rules.xml` semantics.
///
/// Retail ignores `hash` and `needed`; the strict activation boundary requires their shipped
/// forms because an ordinal-compatible language replacement cannot otherwise prove identity.
/// Entity references are decoded, whitespace inside `STRING` is preserved, and empty strings
/// remain real entries. Unknown attributes/elements and nested markup fail closed.
pub fn parse_string_table_xml(text: &str) -> Result<RetailStringTable, StringXmlError> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(false);
    let mut stack: Vec<Vec<u8>> = Vec::new();
    let mut saw_root = false;
    let mut internal = None;
    let mut current_text = None::<String>;
    let mut current_record = None::<(i32, bool)>;
    let mut records = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                visit_element(
                    &reader,
                    &stack,
                    &element,
                    &mut saw_root,
                    &mut internal,
                    &mut current_text,
                    &mut current_record,
                    &mut records,
                    false,
                )?;
                stack.push(element.name().as_ref().to_vec());
            }
            Ok(Event::Empty(element)) => visit_element(
                &reader,
                &stack,
                &element,
                &mut saw_root,
                &mut internal,
                &mut current_text,
                &mut current_record,
                &mut records,
                true,
            )?,
            Ok(Event::End(element)) => {
                if element.name().as_ref() == b"STRING" {
                    finish_record(&mut current_text, &mut current_record, &mut records)?;
                }
                stack.pop();
            }
            Ok(Event::Text(value)) => {
                let decoded = value
                    .xml10_content()
                    .map_err(|error| StringXmlError::Xml(error.to_string()))?;
                let decoded =
                    unescape(&decoded).map_err(|error| StringXmlError::Xml(error.to_string()))?;
                if stack.as_slice() == [b"ROOT".as_slice(), b"STRING".as_slice()] {
                    current_text
                        .as_mut()
                        .expect("STRING start created text")
                        .push_str(&decoded);
                } else if !decoded.trim().is_empty() {
                    return Err(StringXmlError::UnexpectedText(decoded.into_owned()));
                }
            }
            Ok(Event::Decl(_)) | Ok(Event::Comment(_)) => {}
            Ok(Event::CData(_)) => {
                return Err(StringXmlError::UnsupportedXmlConstruct("CDATA".to_string()))
            }
            Ok(Event::PI(_)) => {
                return Err(StringXmlError::UnsupportedXmlConstruct(
                    "processing instruction".to_string(),
                ))
            }
            Ok(Event::DocType(_)) => {
                return Err(StringXmlError::UnsupportedXmlConstruct(
                    "DOCTYPE".to_string(),
                ))
            }
            Ok(Event::GeneralRef(reference)) => {
                let reference = reference
                    .decode()
                    .map_err(|error| StringXmlError::Xml(error.to_string()))?;
                let escaped = format!("&{reference};");
                let decoded =
                    unescape(&escaped).map_err(|error| StringXmlError::Xml(error.to_string()))?;
                if stack.as_slice() == [b"ROOT".as_slice(), b"STRING".as_slice()] {
                    current_text
                        .as_mut()
                        .expect("STRING start created text")
                        .push_str(&decoded);
                } else if !decoded.trim().is_empty() {
                    return Err(StringXmlError::UnexpectedText(decoded.into_owned()));
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => return Err(StringXmlError::Xml(error.to_string())),
        }
    }

    if !saw_root {
        return Err(StringXmlError::MissingRoot);
    }
    if records.is_empty() {
        return Err(StringXmlError::EmptyTable);
    }
    Ok(RetailStringTable {
        internal: internal.expect("ROOT validation sets internal"),
        records: records.into_boxed_slice(),
    })
}

#[allow(clippy::too_many_arguments)]
fn visit_element(
    reader: &Reader<&[u8]>,
    stack: &[Vec<u8>],
    element: &BytesStart<'_>,
    saw_root: &mut bool,
    internal: &mut Option<bool>,
    current_text: &mut Option<String>,
    current_record: &mut Option<(i32, bool)>,
    records: &mut Vec<StringRecord>,
    empty: bool,
) -> Result<(), StringXmlError> {
    let tag = std::str::from_utf8(element.name().as_ref())
        .map_err(|error| StringXmlError::Xml(error.to_string()))?
        .to_string();
    if stack.is_empty() {
        if *saw_root {
            return Err(StringXmlError::DuplicateRoot);
        }
        if tag != "ROOT" {
            return Err(StringXmlError::UnexpectedElement(tag));
        }
        let attrs = attributes(reader, element, &tag)?;
        reject_unknown(&tag, &attrs, &["internal", "xml:space"])?;
        let raw_internal = required(&tag, &attrs, "internal")?;
        *internal = Some(match raw_internal {
            "0" => false,
            "1" => true,
            _ => {
                return Err(StringXmlError::InvalidAttribute {
                    element: tag,
                    attribute: "internal".to_string(),
                    value: raw_internal.to_string(),
                })
            }
        });
        if let Some(space) = attrs.get("xml:space") {
            if space != "preserve" {
                return Err(StringXmlError::InvalidAttribute {
                    element: "ROOT".to_string(),
                    attribute: "xml:space".to_string(),
                    value: space.clone(),
                });
            }
        }
        *saw_root = true;
        return Ok(());
    }

    if stack.len() == 1 && stack[0].as_slice() == b"ROOT" {
        if tag != "STRING" {
            return Err(StringXmlError::UnexpectedElement(tag));
        }
        let attrs = attributes(reader, element, &tag)?;
        reject_unknown(&tag, &attrs, &["hash", "needed"])?;
        let hash_raw = required(&tag, &attrs, "hash")?;
        let declared_hash =
            hash_raw
                .parse::<i32>()
                .map_err(|_| StringXmlError::InvalidAttribute {
                    element: tag.clone(),
                    attribute: "hash".to_string(),
                    value: hash_raw.to_string(),
                })?;
        let needed_raw = required(&tag, &attrs, "needed")?;
        let needed = match needed_raw {
            "0" => false,
            "1" => true,
            _ => {
                return Err(StringXmlError::InvalidAttribute {
                    element: tag,
                    attribute: "needed".to_string(),
                    value: needed_raw.to_string(),
                })
            }
        };
        if empty {
            records.push(StringRecord {
                text: String::new(),
                declared_hash,
                needed,
            });
        } else {
            *current_text = Some(String::new());
            *current_record = Some((declared_hash, needed));
        }
        return Ok(());
    }

    if stack.len() == 2 && stack[0].as_slice() == b"ROOT" && stack[1].as_slice() == b"STRING" {
        return Err(StringXmlError::NestedStringElement(tag));
    }
    Err(StringXmlError::UnexpectedElement(tag))
}

fn finish_record(
    text: &mut Option<String>,
    record: &mut Option<(i32, bool)>,
    records: &mut Vec<StringRecord>,
) -> Result<(), StringXmlError> {
    let Some((declared_hash, needed)) = record.take() else {
        return Err(StringXmlError::Xml(
            "STRING end without matching start".to_string(),
        ));
    };
    records.push(StringRecord {
        text: text.take().unwrap_or_default(),
        declared_hash,
        needed,
    });
    Ok(())
}

fn attributes(
    reader: &Reader<&[u8]>,
    element: &BytesStart<'_>,
    name: &str,
) -> Result<BTreeMap<String, String>, StringXmlError> {
    let mut out = BTreeMap::new();
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|error| StringXmlError::Xml(error.to_string()))?;
        let key = std::str::from_utf8(attribute.key.as_ref())
            .map_err(|error| StringXmlError::Xml(error.to_string()))?
            .to_string();
        let value = attribute
            .decode_and_unescape_value(reader.decoder())
            .map_err(|error| StringXmlError::Xml(error.to_string()))?
            .into_owned();
        if out.insert(key.clone(), value).is_some() {
            return Err(StringXmlError::Xml(format!(
                "`{name}` repeats attribute `{key}`"
            )));
        }
    }
    Ok(out)
}

fn reject_unknown(
    element: &str,
    attrs: &BTreeMap<String, String>,
    expected: &[&str],
) -> Result<(), StringXmlError> {
    if let Some(attribute) = attrs
        .keys()
        .find(|attribute| !expected.contains(&attribute.as_str()))
    {
        return Err(StringXmlError::UnexpectedAttribute {
            element: element.to_string(),
            attribute: attribute.clone(),
        });
    }
    Ok(())
}

fn required<'a>(
    element: &str,
    attrs: &'a BTreeMap<String, String>,
    attribute: &str,
) -> Result<&'a str, StringXmlError> {
    attrs
        .get(attribute)
        .map(String::as_str)
        .ok_or_else(|| StringXmlError::MissingAttribute {
            element: element.to_string(),
            attribute: attribute.to_string(),
        })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StringTableOwner {
    Shipped,
    Mod { package: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StringTableSource {
    pub logical_path: String,
    pub base_path: PathBuf,
    pub selected_path: PathBuf,
    pub owner: StringTableOwner,
    pub localized: bool,
    /// Exact `GameMod::compute_checksum` four-byte stream over the selected file bytes.
    pub retail_checksum: u32,
}

#[derive(Clone, Debug)]
pub struct StringTablesSnapshot {
    generation: u64,
    language: RetailLanguage,
    translated: Arc<RetailStringTable>,
    internal: Arc<RetailStringTable>,
    translated_source: StringTableSource,
    internal_source: StringTableSource,
}

impl StringTablesSnapshot {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn language(&self) -> RetailLanguage {
        self.language
    }

    pub fn translated(&self) -> &Arc<RetailStringTable> {
        &self.translated
    }

    pub fn internal(&self) -> &Arc<RetailStringTable> {
        &self.internal
    }

    pub fn translated_source(&self) -> &StringTableSource {
        &self.translated_source
    }

    pub fn internal_source(&self) -> &StringTableSource {
        &self.internal_source
    }
}

#[derive(Clone, Debug)]
pub struct PreparedStringTables {
    based_on_generation: u64,
    snapshot: StringTablesSnapshot,
}

impl PreparedStringTables {
    pub fn candidate(&self) -> &StringTablesSnapshot {
        &self.snapshot
    }
}

#[derive(Clone, Debug, Default)]
pub struct StringTableRegistry {
    active: Option<Arc<StringTablesSnapshot>>,
}

impl StringTableRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn active(&self) -> Option<Arc<StringTablesSnapshot>> {
        self.active.as_ref().map(Arc::clone)
    }

    fn generation(&self) -> u64 {
        self.active
            .as_ref()
            .map_or(0, |snapshot| snapshot.generation)
    }

    /// Prepare retail startup: both owners are shipped because packages are still disabled.
    pub fn prepare_startup(
        &self,
        shipped_data_dir: &Path,
        language: RetailLanguage,
    ) -> Result<PreparedStringTables, StringPrepareError> {
        let translated = load_shipped(shipped_data_dir, TRANSLATED_STRINGS, language, false);
        let internal = load_shipped(shipped_data_dir, INTERNAL_STRINGS, language, true);
        let mut diagnostics = Vec::new();
        if let Err(error) = &translated {
            diagnostics.push(error.clone());
        }
        if let Err(error) = &internal {
            diagnostics.push(error.clone());
        }
        if !diagnostics.is_empty() {
            return Err(StringPrepareError { diagnostics });
        }
        let (translated, translated_source) = translated.expect("checked");
        let (internal, internal_source) = internal.expect("checked");
        Ok(PreparedStringTables {
            based_on_generation: self.generation(),
            snapshot: StringTablesSnapshot {
                generation: self.generation().wrapping_add(1),
                language,
                translated: Arc::new(translated),
                internal: Arc::new(internal),
                translated_source,
                internal_source,
            },
        })
    }

    /// Prepare the exact later UI lifecycle: reload translated strings through active mod
    /// precedence and retain the existing internal table and its original source.
    pub fn prepare_language_change(
        &self,
        plan: &ActivationPlan,
        shipped_data_dir: &Path,
        language: RetailLanguage,
    ) -> Result<PreparedStringTables, StringPrepareError> {
        let Some(active) = &self.active else {
            return Err(StringPrepareError {
                diagnostics: vec![
                    "language change requires a committed startup StringTable generation"
                        .to_string(),
                ],
            });
        };
        let (translated, translated_source) =
            load_resolved(plan, shipped_data_dir, TRANSLATED_STRINGS, language, false).map_err(
                |diagnostic| StringPrepareError {
                    diagnostics: vec![diagnostic],
                },
            )?;
        validate_translated_identity(&active.translated, &translated).map_err(|diagnostic| {
            StringPrepareError {
                diagnostics: vec![diagnostic],
            }
        })?;
        Ok(PreparedStringTables {
            based_on_generation: active.generation,
            snapshot: StringTablesSnapshot {
                generation: active.generation.wrapping_add(1),
                language,
                translated: Arc::new(translated),
                internal: Arc::clone(&active.internal),
                translated_source,
                internal_source: active.internal_source.clone(),
            },
        })
    }

    pub fn commit(
        &mut self,
        prepared: PreparedStringTables,
    ) -> Result<Arc<StringTablesSnapshot>, StringCommitError> {
        let active = self.generation();
        if prepared.based_on_generation != active {
            return Err(StringCommitError::Stale {
                prepared: prepared.based_on_generation,
                active,
            });
        }
        let committed = Arc::new(prepared.snapshot);
        self.active = Some(Arc::clone(&committed));
        Ok(committed)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StringPrepareError {
    pub diagnostics: Vec<String>,
}

impl fmt::Display for StringPrepareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "StringTable preparation failed with {} diagnostic(s)",
            self.diagnostics.len()
        )
    }
}

impl std::error::Error for StringPrepareError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StringCommitError {
    Stale { prepared: u64, active: u64 },
}

impl fmt::Display for StringCommitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stale { prepared, active } => write!(
                f,
                "prepared StringTable generation {prepared} is stale; active generation is {active}"
            ),
        }
    }
}

impl std::error::Error for StringCommitError {}

fn load_shipped(
    shipped_data_dir: &Path,
    filename: &str,
    language: RetailLanguage,
    expected_internal: bool,
) -> Result<(RetailStringTable, StringTableSource), String> {
    let base_path = require_windows_file(shipped_data_dir, filename)?;
    load_selected(
        filename,
        base_path,
        StringTableOwner::Shipped,
        language,
        expected_internal,
    )
}

fn load_resolved(
    plan: &ActivationPlan,
    shipped_data_dir: &Path,
    filename: &str,
    language: RetailLanguage,
    expected_internal: bool,
) -> Result<(RetailStringTable, StringTableSource), String> {
    let logical = format!("data/{filename}");
    let resolution = plan.explain(&logical);
    let (base_path, owner) = match resolution.outcome {
        ResolutionOutcome::Shipped { .. } => (
            require_windows_file(shipped_data_dir, filename)?,
            StringTableOwner::Shipped,
        ),
        ResolutionOutcome::Unresolved { eligible } => {
            return Err(format!(
                "{logical} owner is unresolved among {}",
                eligible.join(", ")
            ))
        }
        ResolutionOutcome::Mod { package, .. } => {
            let Some(index) = plan
                .stack
                .mods()
                .iter()
                .position(|candidate| candidate.name == package)
            else {
                return Err(format!("resolved package {package:?} is absent from plan"));
            };
            let root = &plan.packages[index].root;
            let data_dir = find_windows_dir(root, "data")
                .map_err(|error| format!("{}: {error}", root.display()))?
                .ok_or_else(|| format!("{} has no data directory", root.display()))?;
            (
                require_windows_file(&data_dir, filename)?,
                StringTableOwner::Mod { package },
            )
        }
    };
    load_selected(filename, base_path, owner, language, expected_internal)
}

fn load_selected(
    filename: &str,
    base_path: PathBuf,
    owner: StringTableOwner,
    language: RetailLanguage,
    expected_internal: bool,
) -> Result<(RetailStringTable, StringTableSource), String> {
    let parent = base_path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", base_path.display()))?;
    let selected_path = if language.suffix().is_empty() {
        base_path.clone()
    } else {
        let localized_name = format!("{filename}{}", language.suffix());
        find_windows_file(parent, &localized_name)
            .map_err(|error| format!("{}: {error}", parent.display()))?
            .unwrap_or_else(|| base_path.clone())
    };
    let bytes = fs::read(&selected_path)
        .map_err(|error| format!("{}: {error}", selected_path.display()))?;
    let checksum = retail_file_checksum(&bytes).ok_or_else(|| {
        format!(
            "{} is 1-3 bytes; retail checksum depends on uninitialized stack bytes",
            selected_path.display()
        )
    })?;
    let text = decode_string_xml(&bytes)
        .map_err(|error| format!("{}: {error}", selected_path.display()))?;
    let table = parse_string_table_xml(&text)
        .map_err(|error| format!("{}: {error}", selected_path.display()))?;
    if table.is_internal() != expected_internal {
        return Err(format!(
            "{} ROOT.internal is {}; expected {} for {filename}",
            selected_path.display(),
            i32::from(table.is_internal()),
            i32::from(expected_internal)
        ));
    }
    Ok((
        table,
        StringTableSource {
            logical_path: format!("data/{filename}"),
            localized: selected_path != base_path,
            base_path,
            selected_path,
            owner,
            retail_checksum: checksum,
        },
    ))
}

fn require_windows_file(root: &Path, filename: &str) -> Result<PathBuf, String> {
    find_windows_file(root, filename)
        .map_err(|error| format!("{}: {error}", root.display()))?
        .ok_or_else(|| format!("{} has no {filename}", root.display()))
}

fn decode_string_xml(bytes: &[u8]) -> Result<String, String> {
    if let Some(bytes) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        return std::str::from_utf8(bytes)
            .map(str::to_string)
            .map_err(|error| format!("invalid UTF-8 after BOM: {error}"));
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
        if bytes.len() % 2 != 0 {
            return Err("odd byte count after UTF-16LE BOM".to_string());
        }
        let words: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        return String::from_utf16(&words).map_err(|error| format!("invalid UTF-16LE: {error}"));
    }
    if bytes.starts_with(&[0xfe, 0xff]) {
        return Err("UTF-16BE is not present in the measured retail corpus".to_string());
    }
    std::str::from_utf8(bytes)
        .map(str::to_string)
        .map_err(|error| format!("invalid unmarked UTF-8: {error}"))
}

fn validate_translated_identity(
    active: &RetailStringTable,
    candidate: &RetailStringTable,
) -> Result<(), String> {
    if active.len() != candidate.len() {
        return Err(format!(
            "translated StringTable has {} entries; active ordinal ABI has {}",
            candidate.len(),
            active.len()
        ));
    }
    if let Some((ordinal, (before, after))) = active
        .records()
        .iter()
        .map(|record| record.declared_hash)
        .zip(
            candidate
                .records()
                .iter()
                .map(|record| record.declared_hash),
        )
        .enumerate()
        .find(|(_, (before, after))| before != after)
    {
        return Err(format!(
            "translated StringTable ordinal {ordinal} changes declared hash {before} -> {after}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::{build_plan, ActivationRequest};

    fn temp_dir(tag: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "don-content-strings-{tag}-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn xml(internal: bool, records: &[(i32, &str)]) -> String {
        let mut out = format!(
            "<?xml version=\"1.0\"?><ROOT internal=\"{}\" xml:space=\"preserve\">",
            i32::from(internal)
        );
        for (hash, text) in records {
            out.push_str(&format!(
                "<STRING hash=\"{hash}\" needed=\"1\">{text}</STRING>"
            ));
        }
        out.push_str("</ROOT>");
        out
    }

    fn write_shipped(data: &Path) {
        fs::create_dir_all(data).unwrap();
        fs::write(
            data.join(TRANSLATED_STRINGS),
            xml(false, &[(10, "English"), (20, " second ")]),
        )
        .unwrap();
        fs::write(
            data.join(INTERNAL_STRINGS),
            xml(true, &[(30, "unit"), (40, "")]),
        )
        .unwrap();
    }

    fn plan(mods: &Path) -> ActivationPlan {
        build_plan(&ActivationRequest::new(mods)).unwrap()
    }

    #[test]
    fn language_table_is_the_exact_five_value_runtime_enum() {
        let actual: Vec<(&str, &str)> = RetailLanguage::ALL
            .into_iter()
            .map(|language| (language.code(), language.suffix()))
            .collect();
        assert_eq!(
            actual,
            vec![
                ("DE", ".7"),
                ("EN", ""),
                ("ES", ".10"),
                ("FR", ".12"),
                ("IT", ".16")
            ]
        );
        assert_eq!(
            RetailLanguage::from_code("fr"),
            Some(RetailLanguage::French)
        );
        assert_eq!(RetailLanguage::from_code("RU"), None);
    }

    #[test]
    fn parser_preserves_ordinal_text_entities_whitespace_and_empty_values() {
        let table = parse_string_table_xml(
            "<ROOT internal=\"0\" xml:space=\"preserve\">\n\
             <STRING hash=\"-4\" needed=\"1\"> A &amp; B </STRING>\n\
             <STRING hash=\"7\" needed=\"0\"/>\n\
             </ROOT>",
        )
        .unwrap();
        assert!(!table.is_internal());
        assert_eq!(table.get(0), Some(" A & B "));
        assert_eq!(table.get(1), Some(""));
        assert_eq!(table.records()[0].declared_hash, -4);
        assert!(!table.records()[1].needed);
    }

    #[test]
    fn parser_fails_closed_on_unknown_or_nested_structure() {
        for bad in [
            "<ROOT internal=\"0\"><OTHER/></ROOT>",
            "<ROOT internal=\"0\"><STRING hash=\"1\" needed=\"1\"><B/></STRING></ROOT>",
            "<ROOT internal=\"0\"><STRING hash=\"1\" needed=\"1\" mystery=\"x\">x</STRING></ROOT>",
            "<ROOT internal=\"0\"><STRING hash=\"x\" needed=\"1\">x</STRING></ROOT>",
            "<ROOT internal=\"0\"><STRING hash=\"1\">x</STRING></ROOT>",
        ] {
            assert!(parse_string_table_xml(bad).is_err(), "accepted {bad}");
        }
    }

    #[test]
    fn decoder_accepts_measured_utf8_boms_and_utf16le() {
        let source = xml(false, &[(1, "Grüße")]);
        let mut utf8_bom = vec![0xef, 0xbb, 0xbf];
        utf8_bom.extend_from_slice(source.as_bytes());
        assert_eq!(decode_string_xml(&utf8_bom).unwrap(), source);

        let mut utf16 = vec![0xff, 0xfe];
        for word in source.encode_utf16() {
            utf16.extend_from_slice(&word.to_le_bytes());
        }
        assert_eq!(decode_string_xml(&utf16).unwrap(), source);
        assert!(decode_string_xml(&[0xfe, 0xff, 0, 1]).is_err());
    }

    #[test]
    fn startup_is_shipped_even_when_a_mod_declares_both_tables() {
        let root = temp_dir("startup-shipped");
        let shipped = root.join("Data");
        let mods = root.join("mods");
        write_shipped(&shipped);
        fs::create_dir_all(mods.join("Words/data")).unwrap();
        fs::write(
            mods.join("Words/data/translated_strings.xml"),
            xml(false, &[(10, "Mod"), (20, "words")]),
        )
        .unwrap();
        fs::write(
            mods.join("Words/data/internal_strings.xml"),
            xml(true, &[(30, "mod"), (40, "internal")]),
        )
        .unwrap();
        let mut registry = StringTableRegistry::new();
        let snapshot = registry
            .commit(
                registry
                    .prepare_startup(&shipped, RetailLanguage::English)
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(snapshot.translated().get(0), Some("English"));
        assert_eq!(snapshot.internal().get(0), Some("unit"));
        assert_eq!(
            snapshot.translated_source().owner,
            StringTableOwner::Shipped
        );
        assert_eq!(snapshot.internal_source().owner, StringTableOwner::Shipped);
        drop(plan(&mods));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn language_change_uses_suffix_only_beside_the_unsuffixed_winner() {
        let root = temp_dir("owner-first");
        let shipped = root.join("Data");
        let mods = root.join("mods");
        write_shipped(&shipped);
        fs::write(
            shipped.join("translated_strings.xml.7"),
            xml(false, &[(10, "Shipped DE"), (20, "Shipped zwei")]),
        )
        .unwrap();
        fs::create_dir_all(mods.join("Words/data")).unwrap();
        fs::write(
            mods.join("Words/data/translated_strings.xml"),
            xml(false, &[(10, "Mod base"), (20, "Mod second")]),
        )
        .unwrap();
        fs::write(
            mods.join("Words/data/translated_strings.xml.7"),
            xml(false, &[(10, "Mod DE"), (20, "Mod zwei")]),
        )
        .unwrap();
        let plan = plan(&mods);
        let mut registry = StringTableRegistry::new();
        let startup = registry
            .prepare_startup(&shipped, RetailLanguage::English)
            .unwrap();
        let startup = registry.commit(startup).unwrap();
        let internal = Arc::clone(startup.internal());
        let changed = registry
            .prepare_language_change(&plan, &shipped, RetailLanguage::German)
            .unwrap();
        assert_eq!(changed.candidate().translated().get(0), Some("Mod DE"));
        assert!(Arc::ptr_eq(&internal, changed.candidate().internal()));
        assert!(changed.candidate().translated_source().localized);
        let selected = &changed.candidate().translated_source().selected_path;
        assert_eq!(
            changed.candidate().translated_source().retail_checksum,
            retail_file_checksum(&fs::read(selected).unwrap()).unwrap()
        );
        assert!(matches!(
            changed.candidate().translated_source().owner,
            StringTableOwner::Mod { ref package } if package == "Words"
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn suffix_without_base_cannot_steal_shipped_ownership() {
        let root = temp_dir("suffix-alone");
        let shipped = root.join("Data");
        let mods = root.join("mods");
        write_shipped(&shipped);
        fs::write(
            shipped.join("translated_strings.xml.7"),
            xml(false, &[(10, "Shipped DE"), (20, "Shipped zwei")]),
        )
        .unwrap();
        fs::create_dir_all(mods.join("SuffixOnly/data")).unwrap();
        fs::write(
            mods.join("SuffixOnly/data/translated_strings.xml.7"),
            xml(false, &[(10, "Wrong"), (20, "owner")]),
        )
        .unwrap();
        let plan = plan(&mods);
        let mut registry = StringTableRegistry::new();
        let startup = registry
            .prepare_startup(&shipped, RetailLanguage::English)
            .unwrap();
        registry.commit(startup).unwrap();
        let changed = registry
            .prepare_language_change(&plan, &shipped, RetailLanguage::German)
            .unwrap();
        assert_eq!(changed.candidate().translated().get(0), Some("Shipped DE"));
        assert_eq!(
            changed.candidate().translated_source().owner,
            StringTableOwner::Shipped
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn winning_owner_without_suffix_falls_back_to_its_own_base() {
        let root = temp_dir("owner-fallback");
        let shipped = root.join("Data");
        let mods = root.join("mods");
        write_shipped(&shipped);
        fs::write(
            shipped.join("translated_strings.xml.7"),
            xml(false, &[(10, "Shipped DE"), (20, "Shipped zwei")]),
        )
        .unwrap();
        fs::create_dir_all(mods.join("BaseOnly/data")).unwrap();
        fs::write(
            mods.join("BaseOnly/data/translated_strings.xml"),
            xml(false, &[(10, "Mod fallback"), (20, "Mod second")]),
        )
        .unwrap();
        let plan = plan(&mods);
        let mut registry = StringTableRegistry::new();
        let startup = registry
            .prepare_startup(&shipped, RetailLanguage::English)
            .unwrap();
        registry.commit(startup).unwrap();
        let changed = registry
            .prepare_language_change(&plan, &shipped, RetailLanguage::German)
            .unwrap();
        assert_eq!(
            changed.candidate().translated().get(0),
            Some("Mod fallback")
        );
        assert!(!changed.candidate().translated_source().localized);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_and_stale_preparations_leave_active_arc_untouched() {
        let root = temp_dir("rollback");
        let shipped = root.join("Data");
        let mods = root.join("mods");
        write_shipped(&shipped);
        fs::create_dir_all(mods.join("Bad/data")).unwrap();
        fs::write(
            mods.join("Bad/data/translated_strings.xml"),
            xml(false, &[(999, "wrong identity"), (20, "same length")]),
        )
        .unwrap();
        let plan = plan(&mods);
        let mut registry = StringTableRegistry::new();
        let stale = registry
            .prepare_startup(&shipped, RetailLanguage::English)
            .unwrap();
        let startup = registry
            .prepare_startup(&shipped, RetailLanguage::English)
            .unwrap();
        let active = registry.commit(startup).unwrap();
        let failed = registry
            .prepare_language_change(&plan, &shipped, RetailLanguage::German)
            .unwrap_err();
        assert!(failed.diagnostics[0].contains("ordinal 0"));
        assert!(Arc::ptr_eq(&active, &registry.active().unwrap()));
        assert!(matches!(
            registry.commit(stale),
            Err(StringCommitError::Stale { .. })
        ));
        assert!(Arc::ptr_eq(&active, &registry.active().unwrap()));
        fs::remove_dir_all(root).unwrap();
    }
}
