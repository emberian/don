//! Transactional ruleset preparation and registration.
//!
//! Retail hot-swaps through `GameMod::closeExistingData` (`0x005AA170`) followed by
//! `GameMod::initExistingData` (`0x005AA3E0`). Those routines mutate global arrays in place;
//! reproducing that ownership graph incrementally would expose half-closed state. DoN instead
//! prepares an immutable [`RuntimeSnapshot`] completely off to the side. [`RuleRegistry::commit`]
//! is one generation-checked pointer replacement and has no parsing, I/O, or fallible consumer
//! callback inside it. A `don-sim` world can retain the returned `Arc` for a match, while a UI
//! or future match reads the newly committed generation.
//!
//! Content `rules.xml` remains retail-style whole-file replacement. The parser uses the
//! extractor-backed `don_rules::FIELDS` table and [`don_rules::apply_parser`] for every scalar
//! and array entry. Unknown, duplicate, mistyped, or normally-present missing constants are
//! errors collected in one report; no partial [`don_rules::Rules`] escapes. The few dead tags,
//! duplicate CTW tags, and one surplus Korean array attribute present in the shipped file are
//! pinned by name and surfaced as diagnostics instead of being silently generalised.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use don_rules::{Rules, FIELDS, RULES_DWORDS, SLOTS};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::manifest::RetailManifest;
use crate::overlay::{Attribution, Layer, Mode, RuleStack};
use crate::workflow::{ActivationPlan, Artifact, ResolutionOutcome};
use crate::ModCategory;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleXmlError {
    Xml(String),
    MissingConstants,
    DuplicateConstants,
    UnknownConstant(String),
    WrongConstantCase { found: String, expected: String },
    DuplicateConstant(String),
    DuplicateAttribute { constant: String, attribute: String },
    MissingAttribute { constant: String, attribute: String },
    UnexpectedAttribute { constant: String, attribute: String },
    UnclassifiedParser(String),
    MissingNormallyPresent(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleXmlDiagnostic {
    /// Present in the shipped XML but absent from every recovered `Constants::init` binder.
    IgnoredUnboundElement(String),
    /// The shipped Korean table has `entry8`; the recovered binder consumes only 0 through 7.
    IgnoredUnboundAttribute { constant: String, attribute: String },
    /// The shipped file repeats five CTW elements with byte-identical attributes.
    RepeatedIdenticalElement(String),
}

impl fmt::Display for RuleXmlDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IgnoredUnboundElement(name) => write!(
                f,
                "ignored shipped legacy element `{name}`: no recovered Constants::init binder"
            ),
            Self::IgnoredUnboundAttribute {
                constant,
                attribute,
            } => write!(
                f,
                "ignored shipped legacy attribute `{constant}.{attribute}`: binder arity is smaller"
            ),
            Self::RepeatedIdenticalElement(name) => {
                write!(f, "accepted byte-identical shipped duplicate `{name}`")
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct ParsedRules {
    rules: Rules,
    diagnostics: Vec<RuleXmlDiagnostic>,
}

impl ParsedRules {
    pub fn rules(&self) -> &Rules {
        &self.rules
    }

    pub fn diagnostics(&self) -> &[RuleXmlDiagnostic] {
        &self.diagnostics
    }

    pub fn into_parts(self) -> (Rules, Vec<RuleXmlDiagnostic>) {
        (self.rules, self.diagnostics)
    }
}

impl fmt::Display for RuleXmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Xml(e) => write!(f, "XML: {e}"),
            Self::MissingConstants => write!(f, "no ROOT/CONSTANTS section"),
            Self::DuplicateConstants => write!(f, "more than one CONSTANTS section"),
            Self::UnknownConstant(name) => write!(f, "unknown CONSTANTS element `{name}`"),
            Self::WrongConstantCase { found, expected } => write!(
                f,
                "CONSTANTS element `{found}` has the wrong case; retail key is `{expected}`"
            ),
            Self::DuplicateConstant(name) => {
                write!(f, "CONSTANTS element `{name}` occurs more than once")
            }
            Self::DuplicateAttribute {
                constant,
                attribute,
            } => write!(f, "`{constant}` repeats attribute `{attribute}`"),
            Self::MissingAttribute {
                constant,
                attribute,
            } => write!(f, "`{constant}` is missing attribute `{attribute}`"),
            Self::UnexpectedAttribute {
                constant,
                attribute,
            } => write!(f, "`{constant}` has unexpected attribute `{attribute}`"),
            Self::UnclassifiedParser(name) => {
                write!(f, "`{name}` has no recovered retail value transform")
            }
            Self::MissingNormallyPresent(name) => write!(
                f,
                "normally-present retail constant `{name}` is absent from this whole-file replacement"
            ),
        }
    }
}

/// Parse a complete retail `rules.xml` value block. All errors are returned together and no
/// partially populated `Rules` value is returned.
pub fn parse_rules_xml(text: &str) -> Result<ParsedRules, Vec<RuleXmlError>> {
    let fields: BTreeMap<&str, &don_rules::RuleField> =
        FIELDS.iter().map(|field| (field.name, field)).collect();
    let required: BTreeSet<&str> = SLOTS.iter().map(|slot| slot.name).collect();
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(false);
    let mut stack: Vec<Vec<u8>> = Vec::new();
    let mut state = RuleParseState {
        saw_constants: false,
        seen: BTreeMap::new(),
        errors: Vec::new(),
        diagnostics: Vec::new(),
        rules: Rules {
            raw: [0; RULES_DWORDS],
        },
    };

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                visit_rule_element(&reader, &stack, &e, &fields, &mut state);
                stack.push(e.name().as_ref().to_vec());
            }
            Ok(Event::Empty(e)) => visit_rule_element(&reader, &stack, &e, &fields, &mut state),
            Ok(Event::End(_)) => {
                stack.pop();
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                state.errors.push(RuleXmlError::Xml(e.to_string()));
                break;
            }
        }
    }
    if !state.saw_constants {
        state.errors.push(RuleXmlError::MissingConstants);
    }
    for name in required {
        if !state.seen.contains_key(name) {
            state
                .errors
                .push(RuleXmlError::MissingNormallyPresent(name.to_string()));
        }
    }
    if state.errors.is_empty() {
        Ok(ParsedRules {
            rules: state.rules,
            diagnostics: state.diagnostics,
        })
    } else {
        Err(state.errors)
    }
}

pub fn read_rules_xml(path: &Path) -> Result<ParsedRules, RuleReadError> {
    let text = fs::read_to_string(path).map_err(|source| RuleReadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    parse_rules_xml(&text).map_err(|errors| RuleReadError::Invalid {
        path: path.to_path_buf(),
        errors,
    })
}

struct RuleParseState {
    saw_constants: bool,
    seen: BTreeMap<String, BTreeMap<String, String>>,
    rules: Rules,
    errors: Vec<RuleXmlError>,
    diagnostics: Vec<RuleXmlDiagnostic>,
}

fn visit_rule_element(
    reader: &Reader<&[u8]>,
    stack: &[Vec<u8>],
    element: &BytesStart<'_>,
    fields: &BTreeMap<&str, &don_rules::RuleField>,
    state: &mut RuleParseState,
) {
    let tag = match std::str::from_utf8(element.name().as_ref()) {
        Ok(tag) => tag.to_string(),
        Err(e) => {
            state.errors.push(RuleXmlError::Xml(e.to_string()));
            return;
        }
    };
    if tag == "CONSTANTS" {
        if state.saw_constants {
            state.errors.push(RuleXmlError::DuplicateConstants);
        }
        state.saw_constants = true;
        return;
    }
    if stack.last().map(Vec::as_slice) != Some(b"CONSTANTS") {
        return;
    }

    let lowercase = tag.to_ascii_lowercase();
    let Some(field) = fields.get(lowercase.as_str()).copied() else {
        if is_shipped_unbound_element(&tag) {
            state
                .diagnostics
                .push(RuleXmlDiagnostic::IgnoredUnboundElement(tag));
        } else {
            state.errors.push(RuleXmlError::UnknownConstant(tag));
        }
        return;
    };
    let expected_tag = field.name.to_ascii_uppercase();
    if tag != expected_tag {
        state.errors.push(RuleXmlError::WrongConstantCase {
            found: tag,
            expected: expected_tag,
        });
        return;
    }
    let attrs = match rule_attributes(reader, element, &expected_tag, &mut state.errors) {
        Some(attrs) => attrs,
        None => return,
    };
    if let Some(first) = state.seen.get(field.name) {
        if is_shipped_repeated_element(&tag) && first == &attrs {
            state
                .diagnostics
                .push(RuleXmlDiagnostic::RepeatedIdenticalElement(tag));
        } else {
            state
                .errors
                .push(RuleXmlError::DuplicateConstant(expected_tag));
        }
        return;
    }
    state.seen.insert(field.name.to_string(), attrs.clone());

    let expected: Vec<String> = if field.count == 1 {
        vec!["value".to_string()]
    } else {
        (0..field.count).map(|i| format!("entry{i}")).collect()
    };
    let expected_set: BTreeSet<&str> = expected.iter().map(String::as_str).collect();
    for attribute in attrs.keys() {
        if !expected_set.contains(attribute.as_str()) {
            if field.name == "korean_citizens" && attribute == "entry8" {
                state
                    .diagnostics
                    .push(RuleXmlDiagnostic::IgnoredUnboundAttribute {
                        constant: expected_tag.clone(),
                        attribute: attribute.clone(),
                    });
            } else {
                state.errors.push(RuleXmlError::UnexpectedAttribute {
                    constant: expected_tag.clone(),
                    attribute: attribute.clone(),
                });
            }
        }
    }
    for (index, attribute) in expected.iter().enumerate() {
        let Some(raw) = attrs.get(attribute) else {
            state.errors.push(RuleXmlError::MissingAttribute {
                constant: expected_tag.clone(),
                attribute: attribute.clone(),
            });
            continue;
        };
        let Some(value) = don_rules::apply_parser(raw, field.parser) else {
            state
                .errors
                .push(RuleXmlError::UnclassifiedParser(field.name.to_string()));
            continue;
        };
        let slot = field.offset as usize / 4 + index;
        state.rules.raw[slot] = value;
    }
}

fn is_shipped_unbound_element(tag: &str) -> bool {
    matches!(
        tag,
        "CITY_UPGRADE_TERR"
            | "TAJ_CARAVAN_LIMIT"
            | "LIBERTY_FREE_UPGRADES"
            | "EIFFEL_SIEGE_RANGE"
            | "SPANISH_EXTRA_SCOUT"
            | "JAPANESE_AIRCRAFT_CARRIERS_SPEED"
            | "KOREAN_START_CITIZEN"
            | "KOREAN_FREE_CITIZEN"
    )
}

fn is_shipped_repeated_element(tag: &str) -> bool {
    matches!(
        tag,
        "CTW_STARTING_TRIBUTE"
            | "CTW_NO_ATTACK_BONUS"
            | "CTW_TRIB_NO_ATTACK"
            | "CTW_CONTINENT_BONUS"
            | "CTW_ATTRITION"
    )
}

fn rule_attributes(
    reader: &Reader<&[u8]>,
    element: &BytesStart<'_>,
    constant: &str,
    errors: &mut Vec<RuleXmlError>,
) -> Option<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for attr in element.attributes() {
        let attr = match attr {
            Ok(attr) => attr,
            Err(e) => {
                errors.push(RuleXmlError::Xml(e.to_string()));
                return None;
            }
        };
        let key = match std::str::from_utf8(attr.key.as_ref()) {
            Ok(key) => key.to_string(),
            Err(e) => {
                errors.push(RuleXmlError::Xml(e.to_string()));
                return None;
            }
        };
        let value = match attr.decode_and_unescape_value(reader.decoder()) {
            Ok(value) => value.into_owned(),
            Err(e) => {
                errors.push(RuleXmlError::Xml(e.to_string()));
                return None;
            }
        };
        if out.insert(key.clone(), value).is_some() {
            errors.push(RuleXmlError::DuplicateAttribute {
                constant: constant.to_string(),
                attribute: key,
            });
        }
    }
    Some(out)
}

#[derive(Debug)]
pub enum RuleReadError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Invalid {
        path: PathBuf,
        errors: Vec<RuleXmlError>,
    },
}

impl fmt::Display for RuleReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Invalid { path, errors } => write!(
                f,
                "{}: {} rule-loader error(s); first: {}",
                path.display(),
                errors.len(),
                errors
                    .first()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "unknown".to_string())
            ),
        }
    }
}

impl std::error::Error for RuleReadError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleSource {
    Shipped,
    Content {
        package: String,
        path: PathBuf,
        /// Exact current `GameMod::compute_checksum` result for the package's XML tree.
        retail_xml_checksum: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageIdentity {
    pub package: String,
    pub priority: i32,
    pub manifest: RetailManifest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverlayWriter {
    pub package: String,
    pub value: i32,
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverlayConflict {
    pub field: String,
    pub index: u16,
    /// Application order; the last writer wins. Packages are applied in reverse retail
    /// priority so lower numeric priority wins, matching file-replacement precedence.
    pub writers: Vec<OverlayWriter>,
    pub winner: String,
}

#[derive(Clone, Debug)]
pub struct RuntimeSnapshot {
    generation: u64,
    mode: Mode,
    rules: Rules,
    source: RuleSource,
    packages: Vec<PackageIdentity>,
    audit: Vec<Attribution>,
    conflicts: Vec<OverlayConflict>,
    rule_diagnostics: Vec<RuleXmlDiagnostic>,
}

impl RuntimeSnapshot {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn rules(&self) -> &Rules {
        &self.rules
    }

    pub fn source(&self) -> &RuleSource {
        &self.source
    }

    pub fn packages(&self) -> &[PackageIdentity] {
        &self.packages
    }

    pub fn audit(&self) -> &[Attribution] {
        &self.audit
    }

    pub fn conflicts(&self) -> &[OverlayConflict] {
        &self.conflicts
    }

    pub fn rule_diagnostics(&self) -> &[RuleXmlDiagnostic] {
        &self.rule_diagnostics
    }
}

#[derive(Clone, Debug)]
pub struct PreparedReload {
    based_on_generation: u64,
    snapshot: RuntimeSnapshot,
}

impl PreparedReload {
    pub fn candidate(&self) -> &RuntimeSnapshot {
        &self.snapshot
    }
}

#[derive(Clone, Debug)]
pub struct RuleRegistry {
    active: Arc<RuntimeSnapshot>,
}

impl RuleRegistry {
    pub fn new(mode: Mode) -> Self {
        Self {
            active: Arc::new(RuntimeSnapshot {
                generation: 0,
                mode,
                rules: Rules::shipped(),
                source: RuleSource::Shipped,
                packages: Vec::new(),
                audit: Vec::new(),
                conflicts: Vec::new(),
                rule_diagnostics: Vec::new(),
            }),
        }
    }

    /// Clone the immutable generation a new sim/world should retain.
    pub fn active(&self) -> Arc<RuntimeSnapshot> {
        Arc::clone(&self.active)
    }

    /// Perform every fallible operation without touching the registered generation.
    pub fn prepare(
        &self,
        plan: &ActivationPlan,
        mode: Mode,
    ) -> Result<PreparedReload, ReloadError> {
        let mut diagnostics = plan.activation_blockers();
        let mut packages = Vec::new();
        for (index, package) in plan.stack.mods().iter().enumerate() {
            if !package.enabled || (package.is_dropdown_mod() && !package.dropdown_active) {
                continue;
            }
            match &plan.packages[index].manifest {
                Artifact::Valid(manifest) => packages.push(PackageIdentity {
                    package: package.name.clone(),
                    priority: package.priority,
                    manifest: manifest.clone(),
                }),
                Artifact::Invalid(_) => {}
                Artifact::Absent => diagnostics.push(format!(
                    "{} has no prepared manifest/checksum artifact",
                    package.name
                )),
            }
        }

        let resolution = plan.explain("data/rules.xml");
        let mut rule_diagnostics = Vec::new();
        let (base, source, digest) = match resolution.outcome {
            ResolutionOutcome::Shipped { .. } => (Rules::shipped(), RuleSource::Shipped, 0),
            ResolutionOutcome::Unresolved { eligible } => {
                diagnostics.push(format!(
                    "data/rules.xml winner unresolved among {}",
                    eligible.join(", ")
                ));
                (Rules::shipped(), RuleSource::Shipped, 0)
            }
            ResolutionOutcome::Mod {
                ref package,
                path: _,
            } => {
                let Some(index) = plan
                    .stack
                    .mods()
                    .iter()
                    .position(|candidate| candidate.name == *package)
                else {
                    diagnostics.push(format!("resolved rules package {package:?} is absent"));
                    return Err(ReloadError { diagnostics });
                };
                let physical = plan.packages[index]
                    .root
                    .join(ModCategory::Data.relative_dir())
                    .join(&resolution.filename);
                let rules = match read_rules_xml(&physical) {
                    Ok(parsed) => {
                        let (rules, parsed_diagnostics) = parsed.into_parts();
                        rule_diagnostics = parsed_diagnostics;
                        rules
                    }
                    Err(RuleReadError::Invalid { errors, .. }) => {
                        diagnostics.extend(
                            errors
                                .into_iter()
                                .map(|error| format!("{}: {error}", physical.display())),
                        );
                        Rules::shipped()
                    }
                    Err(error) => {
                        diagnostics.push(error.to_string());
                        Rules::shipped()
                    }
                };
                let checksum = packages
                    .iter()
                    .find(|identity| identity.package == *package)
                    .map(|identity| identity.manifest.checksum)
                    .unwrap_or_else(|| {
                        diagnostics.push(format!(
                            "rules package {package:?} has no valid XML checksum"
                        ));
                        0
                    });
                (
                    rules,
                    RuleSource::Content {
                        package: package.clone(),
                        path: physical,
                        retail_xml_checksum: checksum,
                    },
                    u64::from(checksum),
                )
            }
        };

        if !diagnostics.is_empty() {
            return Err(ReloadError { diagnostics });
        }

        let mut stack = match source {
            RuleSource::Shipped => RuleStack::shipped(mode),
            RuleSource::Content { .. } => RuleStack::from_content(mode, base, digest),
        };
        let mut writers = BTreeMap::<(String, u16), Vec<OverlayWriter>>::new();
        let active_indices: Vec<usize> = plan
            .stack
            .mods()
            .iter()
            .enumerate()
            .filter(|(_, package)| {
                package.enabled && (!package.is_dropdown_mod() || package.dropdown_active)
            })
            .map(|(index, _)| index)
            .collect();
        for index in active_indices.into_iter().rev() {
            let package = &plan.stack.mods()[index];
            if let Artifact::Valid(overlay) = &plan.packages[index].overlay {
                for patch in &overlay.patches {
                    writers
                        .entry((patch.field.clone(), patch.index))
                        .or_default()
                        .push(OverlayWriter {
                            package: package.name.clone(),
                            value: patch.value,
                            note: patch.note.clone(),
                        });
                }
                stack.extend(Layer::Overlay, overlay.patches.clone());
            }
        }
        let conflicts = writers
            .into_iter()
            .filter_map(|((field, index), writers)| {
                (writers.len() > 1).then(|| OverlayConflict {
                    field,
                    index,
                    winner: writers.last().expect("two writers").package.clone(),
                    writers,
                })
            })
            .collect();
        let (rules, audit) = stack.compose().map_err(|errors| ReloadError {
            diagnostics: errors.into_iter().map(|error| error.to_string()).collect(),
        })?;
        Ok(PreparedReload {
            based_on_generation: self.active.generation,
            snapshot: RuntimeSnapshot {
                generation: self.active.generation.wrapping_add(1),
                mode,
                rules,
                source,
                packages,
                audit,
                conflicts,
                rule_diagnostics,
            },
        })
    }

    /// Atomically replace the active immutable snapshot. A candidate prepared against an old
    /// generation is rejected; it is never silently rebased over a newer activation.
    pub fn commit(
        &mut self,
        prepared: PreparedReload,
    ) -> Result<Arc<RuntimeSnapshot>, CommitError> {
        if prepared.based_on_generation != self.active.generation {
            return Err(CommitError::Stale {
                prepared: prepared.based_on_generation,
                active: self.active.generation,
            });
        }
        let committed = Arc::new(prepared.snapshot);
        self.active = Arc::clone(&committed);
        Ok(committed)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReloadError {
    pub diagnostics: Vec<String>,
}

impl fmt::Display for ReloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "reload preparation failed with {} diagnostic(s)",
            self.diagnostics.len()
        )
    }
}

impl std::error::Error for ReloadError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommitError {
    Stale { prepared: u64, active: u64 },
}

impl fmt::Display for CommitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stale { prepared, active } => write!(
                f,
                "prepared generation {prepared} is stale; active generation is {active}"
            ),
        }
    }
}

impl std::error::Error for CommitError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn extracted_rules_xml() -> String {
        fn escape(value: &str) -> String {
            value
                .replace('&', "&amp;")
                .replace('"', "&quot;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
        }

        let mut grouped = BTreeMap::<&str, BTreeMap<u16, &str>>::new();
        for slot in SLOTS {
            grouped
                .entry(slot.name)
                .or_default()
                .insert(slot.index, slot.xml_value);
        }
        let mut xml = String::from("<ROOT><CONSTANTS>");
        for field in FIELDS {
            let Some(values) = grouped.get(field.name) else {
                continue;
            };
            xml.push('<');
            xml.push_str(&field.name.to_ascii_uppercase());
            if field.count == 1 {
                xml.push_str(" value=\"");
                xml.push_str(&escape(values[&0]));
                xml.push('"');
            } else {
                for index in 0..field.count {
                    xml.push_str(&format!(" entry{index}=\"{}\"", escape(values[&index])));
                }
            }
            xml.push_str("/>");
        }
        xml.push_str("</CONSTANTS></ROOT>");
        xml
    }

    #[test]
    fn extractor_corpus_builds_the_recovered_rule_block() {
        let parsed = parse_rules_xml(&extracted_rules_xml()).unwrap();
        assert!(parsed.diagnostics().is_empty());
        for slot in SLOTS {
            assert_eq!(
                parsed.rules().raw[slot.offset as usize / 4],
                slot.stored,
                "{}",
                slot.name
            );
        }
    }

    #[test]
    fn extracted_shipped_file_matches_every_recovered_slot_when_available() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ron-data/rules.xml");
        let Ok(text) = fs::read_to_string(&path) else {
            eprintln!("skipping: {} is an extracted local asset", path.display());
            return;
        };
        let parsed = parse_rules_xml(&text).unwrap();
        assert_eq!(parsed.diagnostics().len(), 14);
        assert!(parsed.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic,
            RuleXmlDiagnostic::IgnoredUnboundElement(name) if name == "CITY_UPGRADE_TERR"
        )));
        assert!(parsed.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic,
            RuleXmlDiagnostic::IgnoredUnboundAttribute { constant, attribute }
                if constant == "KOREAN_CITIZENS" && attribute == "entry8"
        )));
        assert!(parsed.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic,
            RuleXmlDiagnostic::RepeatedIdenticalElement(name)
                if name == "CTW_STARTING_TRIBUTE"
        )));
        for slot in SLOTS {
            assert_eq!(
                parsed.rules().raw[slot.offset as usize / 4],
                slot.stored,
                "{}[{}]",
                slot.name,
                slot.index
            );
        }
    }

    #[test]
    fn no_partial_rules_escape_a_multi_error_parse() {
        let errors = parse_rules_xml(
            r#"<ROOT><CONSTANTS><UNIT_MOVE_SPEED value="1"/><UNIT_MOVE_SPEED value="2"/><TYPO value="3"/></CONSTANTS></ROOT>"#,
        )
        .unwrap_err();
        assert!(errors
            .iter()
            .any(|error| matches!(error, RuleXmlError::DuplicateConstant(_))));
        assert!(errors
            .iter()
            .any(|error| matches!(error, RuleXmlError::UnknownConstant(_))));
        assert!(errors
            .iter()
            .any(|error| matches!(error, RuleXmlError::MissingNormallyPresent(_))));
    }

    #[test]
    fn failed_and_stale_transactions_leave_the_active_arc_unchanged() {
        let mut registry = RuleRegistry::new(Mode::Improved);
        let original = registry.active();
        assert_eq!(original.generation(), 0);

        let bad_root =
            std::env::temp_dir().join(format!("don-content-runtime-bad-{}", std::process::id()));
        let _ = fs::remove_dir_all(&bad_root);
        fs::create_dir_all(bad_root.join("Bad/data")).unwrap();
        fs::write(bad_root.join("Bad/data/rules.xml"), b"<ROOT/>").unwrap();
        let bad_plan =
            crate::workflow::build_plan(&crate::workflow::ActivationRequest::new(&bad_root))
                .unwrap();
        assert!(registry.prepare(&bad_plan, Mode::Improved).is_err());
        assert!(Arc::ptr_eq(&original, &registry.active()));
        fs::remove_dir_all(bad_root).unwrap();

        // Build two valid no-mod plans against the same generation.
        let root =
            std::env::temp_dir().join(format!("don-content-runtime-empty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let plan =
            crate::workflow::build_plan(&crate::workflow::ActivationRequest::new(&root)).unwrap();
        let first = registry.prepare(&plan, Mode::Improved).unwrap();
        let stale = registry.prepare(&plan, Mode::Improved).unwrap();
        let committed = registry.commit(first).unwrap();
        assert_eq!(committed.generation(), 1);
        assert!(matches!(
            registry.commit(stale),
            Err(CommitError::Stale {
                prepared: 0,
                active: 1
            })
        ));
        assert_eq!(registry.active().generation(), 1);
        assert!(Arc::ptr_eq(&committed, &registry.active()));
        assert_eq!(
            original.generation(),
            0,
            "retained sim snapshot is immutable"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn overlay_conflicts_are_explicit_and_low_priority_number_wins() {
        let root = std::env::temp_dir().join(format!(
            "don-content-runtime-conflict-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        for (name, value) in [("First", "2"), ("Second", "3")] {
            fs::create_dir_all(root.join(name)).unwrap();
            fs::write(
                root.join(name).join("don-overlay.xml"),
                format!(
                    "<DON_OVERLAY version=\"1\"><PATCH field=\"unit_pack_turn_bonus\" index=\"0\" value=\"{value}\"/></DON_OVERLAY>"
                ),
            )
            .unwrap();
        }
        let mut request = crate::workflow::ActivationRequest::new(&root);
        request.explicit_order = vec!["First".into(), "Second".into()];
        let plan = crate::workflow::build_plan(&request).unwrap();
        let registry = RuleRegistry::new(Mode::Improved);
        let prepared = registry.prepare(&plan, Mode::Improved).unwrap();
        let conflict = &prepared.candidate().conflicts()[0];
        assert_eq!(conflict.winner, "First");
        assert_eq!(conflict.writers.len(), 2);
        let field = FIELDS
            .iter()
            .find(|field| field.name == "unit_pack_turn_bonus")
            .unwrap();
        assert_eq!(
            prepared.candidate().rules().raw[field.offset as usize / 4],
            2
        );
        fs::remove_dir_all(root).unwrap();
    }
}
