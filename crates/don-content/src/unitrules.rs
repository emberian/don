//! Exact structural boundary for retail `unitrules.xml`.
//!
//! Retail's `UnitType` registry contains 364 consecutive records at global `TypeIndex`
//! 50 through 413. The shipped file has one `UNIT` per record, in that exact order, and
//! every record has the same closed 55-field schema. [`UnitRuleCatalog`] retains that
//! positional identity and every decoded field as text; it deliberately does not guess the
//! field-specific transforms performed by `UnitType::init` (`0x0061_AB50`).
//!
//! `NAME` is not unique: the shipped 364 rows contain only 300 distinct names. `TYPENAME`
//! and `GRAPH` are also non-unique. A row's stable content identity is therefore its
//! [`UnitCanonicalKey`] — the exact `NAME` character data plus its zero-based occurrence
//! among earlier rows with that same exact name — while its runtime identity remains the
//! positional `TypeIndex`. No map in this module silently overwrites a same-name row.
//!
//! This boundary is intentionally fail-closed. Unknown, missing, reordered, attributed, or
//! nested fields are errors, as is any record count other than 364. Runtime graft inheritance
//! is a later loader pass: live evidence shows that the formerly described "same-NAME
//! canonicalisation" is actually driven by the `GRAFT` column. This parser preserves `GRAFT`
//! but does not manufacture the still-underived copied-field set.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use quick_xml::escape::unescape;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

pub const UNIT_TYPE_BASE: u16 = 50;
pub const UNIT_TYPE_END_EXCLUSIVE: u16 = 414;
pub const UNIT_RULE_ROWS: usize = 364;
pub const UNIT_RULE_FIELD_COUNT: usize = 55;

/// Byte identity of the extracted shipped file used to recover and audit this schema.
///
/// The structural parser does not pretend that a byte hash is a semantic binder. Consumers
/// that need shipped-only admission can compare their source artifact against these values
/// before calling [`parse_unitrules_xml`].
pub const SHIPPED_UNITRULES_BYTES: usize = 629_942;
pub const SHIPPED_UNITRULES_SHA256: &str =
    "09e0b35c20d149083fafac12c5d2182951d1f1878bfc372ad99d6004e2aceabb";

/// The closed child-element schema of a retail `UNIT`, in file/binder order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum UnitRuleField {
    Name,
    Graph,
    ObjMask,
    Flags,
    Where,
    Attack,
    Hits,
    Moves,
    Support,
    Cost,
    JobTime,
    Preq0,
    Preq1,
    From,
    Jump,
    Graft,
    Range,
    Los,
    ScienceLos,
    FlyHigh,
    FlyLow,
    Recharge,
    Armor,
    Domain,
    ToHit,
    Attenuate,
    Cat,
    Progression,
    Splash,
    SplashPercent,
    AmmoPerAtt,
    TurnSpeed,
    ProjSpeed,
    CarrySize,
    Pop,
    ResearchPremiumTime,
    ResearchPremiumCost,
    JobExtraTime,
    Mana,
    TribeMask,
    Carry,
    GuySpacing,
    XSpacing,
    YSpacing,
    CircleRadius,
    BlockRadius,
    TargetSize,
    UberSize,
    CrewSize,
    GridX,
    GridY,
    Upgrade,
    PushSize,
    PushCircles,
    TypeName,
}

impl UnitRuleField {
    pub const ALL: [Self; UNIT_RULE_FIELD_COUNT] = [
        Self::Name,
        Self::Graph,
        Self::ObjMask,
        Self::Flags,
        Self::Where,
        Self::Attack,
        Self::Hits,
        Self::Moves,
        Self::Support,
        Self::Cost,
        Self::JobTime,
        Self::Preq0,
        Self::Preq1,
        Self::From,
        Self::Jump,
        Self::Graft,
        Self::Range,
        Self::Los,
        Self::ScienceLos,
        Self::FlyHigh,
        Self::FlyLow,
        Self::Recharge,
        Self::Armor,
        Self::Domain,
        Self::ToHit,
        Self::Attenuate,
        Self::Cat,
        Self::Progression,
        Self::Splash,
        Self::SplashPercent,
        Self::AmmoPerAtt,
        Self::TurnSpeed,
        Self::ProjSpeed,
        Self::CarrySize,
        Self::Pop,
        Self::ResearchPremiumTime,
        Self::ResearchPremiumCost,
        Self::JobExtraTime,
        Self::Mana,
        Self::TribeMask,
        Self::Carry,
        Self::GuySpacing,
        Self::XSpacing,
        Self::YSpacing,
        Self::CircleRadius,
        Self::BlockRadius,
        Self::TargetSize,
        Self::UberSize,
        Self::CrewSize,
        Self::GridX,
        Self::GridY,
        Self::Upgrade,
        Self::PushSize,
        Self::PushCircles,
        Self::TypeName,
    ];

    pub const fn tag(self) -> &'static str {
        const TAGS: [&str; UNIT_RULE_FIELD_COUNT] = [
            "NAME",
            "GRAPH",
            "OBJ_MASK",
            "FLAGS",
            "WHERE",
            "ATTACK",
            "HITS",
            "MOVES",
            "SUPPORT",
            "COST",
            "JOB_TIME",
            "PREQ0",
            "PREQ1",
            "FROM",
            "JUMP",
            "GRAFT",
            "RANGE",
            "LOS",
            "SCIENCE_LOS",
            "FLY_HIGH",
            "FLY_LOW",
            "RECHARGE",
            "ARMOR",
            "DOMAIN",
            "TO_HIT",
            "ATTENUATE",
            "CAT",
            "PROGRESSION",
            "SPLASH",
            "SPLASH_PERCENT",
            "AMMO_PER_ATT",
            "TURN_SPEED",
            "PROJ_SPEED",
            "CARRY_SIZE",
            "POP",
            "RESEARCH_PREMIUM_TIME",
            "RESEARCH_PREMIUM_COST",
            "JOB_EXTRA_TIME",
            "MANA",
            "TRIBE_MASK",
            "CARRY",
            "GUY_SPACING",
            "X_SPACING",
            "Y_SPACING",
            "CIRCLE_RADIUS",
            "BLOCK_RADIUS",
            "TARGET_SIZE",
            "UBER_SIZE",
            "CREW_SIZE",
            "GRID_X",
            "GRID_Y",
            "UPGRADE",
            "PUSH_SIZE",
            "PUSH_CIRCLES",
            "TYPENAME",
        ];
        TAGS[self as usize]
    }

    const fn may_be_empty(self) -> bool {
        matches!(self, Self::Flags | Self::Upgrade)
    }
}

/// Exact source-level identity for one of the intentionally repeated `NAME` values.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnitCanonicalKey {
    pub name: String,
    pub occurrence: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitRuleRow {
    row_index: u16,
    type_index: u16,
    canonical_key: UnitCanonicalKey,
    values: [String; UNIT_RULE_FIELD_COUNT],
}

impl UnitRuleRow {
    pub fn row_index(&self) -> u16 {
        self.row_index
    }

    pub fn type_index(&self) -> u16 {
        self.type_index
    }

    pub fn canonical_key(&self) -> &UnitCanonicalKey {
        &self.canonical_key
    }

    pub fn get(&self, field: UnitRuleField) -> &str {
        &self.values[field as usize]
    }

    pub fn values(&self) -> &[String; UNIT_RULE_FIELD_COUNT] {
        &self.values
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitRuleCatalog {
    rows: Box<[UnitRuleRow]>,
    canonical: BTreeMap<UnitCanonicalKey, usize>,
}

impl UnitRuleCatalog {
    pub fn rows(&self) -> &[UnitRuleRow] {
        &self.rows
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn by_type_index(&self, type_index: u16) -> Option<&UnitRuleRow> {
        let row = type_index.checked_sub(UNIT_TYPE_BASE)? as usize;
        self.rows.get(row)
    }

    /// Exact lookup. Name case and whitespace are not folded or trimmed.
    pub fn by_name_occurrence(&self, name: &str, occurrence: u16) -> Option<&UnitRuleRow> {
        let key = UnitCanonicalKey {
            name: name.to_string(),
            occurrence,
        };
        self.canonical.get(&key).map(|&row| &self.rows[row])
    }

    /// Every exact-name match in source order, without collapsing national variants.
    pub fn same_name<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a UnitRuleRow> + 'a {
        self.rows
            .iter()
            .filter(move |row| row.get(UnitRuleField::Name) == name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnitRulesError {
    Xml(String),
    MissingRoot,
    DuplicateRoot,
    UnclosedElement(String),
    UnexpectedElement {
        path: String,
        element: String,
    },
    UnexpectedAttribute {
        element: String,
        attribute: String,
    },
    UnexpectedText(String),
    UnsupportedXmlConstruct(String),
    WrongField {
        row: usize,
        position: usize,
        expected: &'static str,
        found: String,
    },
    MissingFields {
        row: usize,
        found: usize,
    },
    EmptyField {
        row: usize,
        field: &'static str,
    },
    EmptyName {
        row: usize,
    },
    WrongRowCount {
        expected: usize,
        found: usize,
    },
    IndexOverflow,
}

impl fmt::Display for UnitRulesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Xml(error) => write!(f, "XML: {error}"),
            Self::MissingRoot => write!(f, "missing ROOT element"),
            Self::DuplicateRoot => write!(f, "more than one ROOT element"),
            Self::UnclosedElement(element) => write!(f, "unclosed element `{element}`"),
            Self::UnexpectedElement { path, element } => {
                write!(f, "unexpected element `{element}` under {path}")
            }
            Self::UnexpectedAttribute { element, attribute } => {
                write!(f, "`{element}` has unexpected attribute `{attribute}`")
            }
            Self::UnexpectedText(text) => write!(f, "unexpected text {text:?}"),
            Self::UnsupportedXmlConstruct(kind) => {
                write!(f, "unsupported XML construct `{kind}`")
            }
            Self::WrongField {
                row,
                position,
                expected,
                found,
            } => write!(
                f,
                "UNIT row {row} field {position} is `{found}`; expected `{expected}`"
            ),
            Self::MissingFields { row, found } => write!(
                f,
                "UNIT row {row} has {found} fields; expected {UNIT_RULE_FIELD_COUNT}"
            ),
            Self::EmptyField { row, field } => {
                write!(f, "UNIT row {row} field `{field}` is empty")
            }
            Self::EmptyName { row } => write!(f, "UNIT row {row} has an empty NAME"),
            Self::WrongRowCount { expected, found } => {
                write!(f, "unitrules has {found} UNIT rows; expected {expected}")
            }
            Self::IndexOverflow => write!(f, "unitrules row cannot fit the retail TypeIndex range"),
        }
    }
}

impl std::error::Error for UnitRulesError {}

#[derive(Debug)]
pub enum UnitRulesReadError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Invalid {
        path: PathBuf,
        source: UnitRulesError,
    },
}

impl fmt::Display for UnitRulesReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "cannot read {}: {source}", path.display()),
            Self::Invalid { path, source } => {
                write!(f, "invalid {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for UnitRulesReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Invalid { source, .. } => Some(source),
        }
    }
}

pub fn read_unitrules_xml(path: &Path) -> Result<UnitRuleCatalog, UnitRulesReadError> {
    let text = fs::read_to_string(path).map_err(|source| UnitRulesReadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    parse_unitrules_xml(&text).map_err(|source| UnitRulesReadError::Invalid {
        path: path.to_path_buf(),
        source,
    })
}

/// Parse the closed retail registry shape. Values stay as exact decoded XML character data.
pub fn parse_unitrules_xml(text: &str) -> Result<UnitRuleCatalog, UnitRulesError> {
    parse_with_expected_rows(text, UNIT_RULE_ROWS)
}

#[derive(Default)]
struct UnitBuilder {
    values: Vec<String>,
    field_text: Option<String>,
}

fn parse_with_expected_rows(
    text: &str,
    expected_rows: usize,
) -> Result<UnitRuleCatalog, UnitRulesError> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(false);
    let mut stack = Vec::<String>::new();
    let mut saw_root = false;
    let mut closed_root = false;
    let mut saw_comments = false;
    let mut builder = None::<UnitBuilder>;
    let mut rows = Vec::with_capacity(expected_rows);
    let mut occurrences = BTreeMap::<String, u16>::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                let tag = element_name(&element)?;
                reject_attributes(&element)?;
                match stack.as_slice() {
                    [] => {
                        if saw_root {
                            return Err(UnitRulesError::DuplicateRoot);
                        }
                        if tag != "ROOT" {
                            return Err(unexpected(&stack, tag));
                        }
                        saw_root = true;
                    }
                    [root] if root == "ROOT" && tag == "COMMENTS" => {
                        if saw_comments || !rows.is_empty() {
                            return Err(unexpected(&stack, tag));
                        }
                        saw_comments = true;
                    }
                    [root] if root == "ROOT" && tag == "UNIT" => {
                        builder = Some(UnitBuilder::default());
                    }
                    [root, unit] if root == "ROOT" && unit == "UNIT" => {
                        begin_field(
                            builder.as_mut().expect("UNIT has builder"),
                            rows.len(),
                            &tag,
                        )?;
                    }
                    _ => return Err(unexpected(&stack, tag)),
                }
                stack.push(tag);
            }
            Ok(Event::Empty(element)) => {
                let tag = element_name(&element)?;
                reject_attributes(&element)?;
                match stack.as_slice() {
                    [root, unit] if root == "ROOT" && unit == "UNIT" => {
                        let builder = builder.as_mut().expect("UNIT has builder");
                        begin_field(builder, rows.len(), &tag)?;
                        finish_field(builder, rows.len())?;
                    }
                    _ => return Err(unexpected(&stack, tag)),
                }
            }
            Ok(Event::End(element)) => {
                let tag = std::str::from_utf8(element.name().as_ref())
                    .map_err(|error| UnitRulesError::Xml(error.to_string()))?
                    .to_string();
                if stack.last() != Some(&tag) {
                    return Err(UnitRulesError::Xml(format!(
                        "closing `{tag}` does not match open element {:?}",
                        stack.last()
                    )));
                }
                match stack.as_slice() {
                    [root, unit, _field] if root == "ROOT" && unit == "UNIT" => {
                        finish_field(builder.as_mut().expect("UNIT has builder"), rows.len())?;
                    }
                    [root, unit] if root == "ROOT" && unit == "UNIT" => {
                        finish_unit(
                            builder.take().expect("UNIT has builder"),
                            &mut rows,
                            &mut occurrences,
                        )?;
                    }
                    [root] if root == "ROOT" => closed_root = true,
                    [root, comments] if root == "ROOT" && comments == "COMMENTS" => {}
                    _ => return Err(unexpected(&stack[..stack.len().saturating_sub(1)], tag)),
                }
                stack.pop();
            }
            Ok(Event::Text(value)) => {
                let decoded = value
                    .xml10_content()
                    .map_err(|error| UnitRulesError::Xml(error.to_string()))?;
                let decoded =
                    unescape(&decoded).map_err(|error| UnitRulesError::Xml(error.to_string()))?;
                append_text(&stack, builder.as_mut(), &decoded)?;
            }
            Ok(Event::GeneralRef(reference)) => {
                let reference = reference
                    .decode()
                    .map_err(|error| UnitRulesError::Xml(error.to_string()))?;
                let escaped = format!("&{reference};");
                let decoded =
                    unescape(&escaped).map_err(|error| UnitRulesError::Xml(error.to_string()))?;
                append_text(&stack, builder.as_mut(), &decoded)?;
            }
            Ok(Event::Decl(_)) if !saw_root => {}
            Ok(Event::Comment(_)) if stack.len() < 3 => {}
            Ok(Event::Decl(_)) => {
                return Err(UnitRulesError::UnsupportedXmlConstruct(
                    "XML declaration after document start".to_string(),
                ))
            }
            Ok(Event::Comment(_)) => {
                return Err(UnitRulesError::UnsupportedXmlConstruct(
                    "comment inside UNIT field".to_string(),
                ))
            }
            Ok(Event::CData(_)) => {
                return Err(UnitRulesError::UnsupportedXmlConstruct("CDATA".to_string()))
            }
            Ok(Event::PI(_)) => {
                return Err(UnitRulesError::UnsupportedXmlConstruct(
                    "processing instruction".to_string(),
                ))
            }
            Ok(Event::DocType(_)) => {
                return Err(UnitRulesError::UnsupportedXmlConstruct(
                    "DOCTYPE".to_string(),
                ))
            }
            Ok(Event::Eof) => break,
            Err(error) => return Err(UnitRulesError::Xml(error.to_string())),
        }
    }

    if !saw_root {
        return Err(UnitRulesError::MissingRoot);
    }
    if !stack.is_empty() || !closed_root {
        return Err(UnitRulesError::UnclosedElement(
            stack.last().cloned().unwrap_or_else(|| "ROOT".to_string()),
        ));
    }
    if rows.len() != expected_rows {
        return Err(UnitRulesError::WrongRowCount {
            expected: expected_rows,
            found: rows.len(),
        });
    }
    if expected_rows == UNIT_RULE_ROWS
        && rows.last().map(UnitRuleRow::type_index) != Some(UNIT_TYPE_END_EXCLUSIVE - 1)
    {
        return Err(UnitRulesError::IndexOverflow);
    }

    let canonical = rows
        .iter()
        .enumerate()
        .map(|(row, value)| (value.canonical_key.clone(), row))
        .collect();
    Ok(UnitRuleCatalog {
        rows: rows.into_boxed_slice(),
        canonical,
    })
}

fn begin_field(builder: &mut UnitBuilder, row: usize, found: &str) -> Result<(), UnitRulesError> {
    let position = builder.values.len();
    let Some(expected) = UnitRuleField::ALL.get(position).copied() else {
        return Err(UnitRulesError::WrongField {
            row,
            position,
            expected: "end of UNIT",
            found: found.to_string(),
        });
    };
    if found != expected.tag() {
        return Err(UnitRulesError::WrongField {
            row,
            position,
            expected: expected.tag(),
            found: found.to_string(),
        });
    }
    builder.field_text = Some(String::new());
    Ok(())
}

fn finish_field(builder: &mut UnitBuilder, row: usize) -> Result<(), UnitRulesError> {
    let position = builder.values.len();
    let field = UnitRuleField::ALL[position];
    let value = builder.field_text.take().expect("field start created text");
    if value.is_empty() && field == UnitRuleField::Name {
        return Err(UnitRulesError::EmptyName { row });
    }
    if value.is_empty() && !field.may_be_empty() {
        return Err(UnitRulesError::EmptyField {
            row,
            field: field.tag(),
        });
    }
    builder.values.push(value);
    Ok(())
}

fn finish_unit(
    builder: UnitBuilder,
    rows: &mut Vec<UnitRuleRow>,
    occurrences: &mut BTreeMap<String, u16>,
) -> Result<(), UnitRulesError> {
    let row = rows.len();
    if builder.values.len() != UNIT_RULE_FIELD_COUNT {
        return Err(UnitRulesError::MissingFields {
            row,
            found: builder.values.len(),
        });
    }
    let values: [String; UNIT_RULE_FIELD_COUNT] = builder
        .values
        .try_into()
        .map_err(|_| UnitRulesError::MissingFields { row, found: 0 })?;
    let name = values[UnitRuleField::Name as usize].clone();
    if name.is_empty() {
        return Err(UnitRulesError::EmptyName { row });
    }
    let occurrence = occurrences.entry(name.clone()).or_default();
    let canonical_key = UnitCanonicalKey {
        name,
        occurrence: *occurrence,
    };
    *occurrence = occurrence
        .checked_add(1)
        .ok_or(UnitRulesError::IndexOverflow)?;
    let row_index = u16::try_from(row).map_err(|_| UnitRulesError::IndexOverflow)?;
    let type_index = UNIT_TYPE_BASE
        .checked_add(row_index)
        .ok_or(UnitRulesError::IndexOverflow)?;
    rows.push(UnitRuleRow {
        row_index,
        type_index,
        canonical_key,
        values,
    });
    Ok(())
}

fn append_text(
    stack: &[String],
    builder: Option<&mut UnitBuilder>,
    text: &str,
) -> Result<(), UnitRulesError> {
    if matches!(stack, [root, unit, _field] if root == "ROOT" && unit == "UNIT") {
        builder
            .and_then(|builder| builder.field_text.as_mut())
            .expect("UNIT field has text buffer")
            .push_str(text);
    } else if !text.trim().is_empty() {
        return Err(UnitRulesError::UnexpectedText(text.to_string()));
    }
    Ok(())
}

fn element_name(element: &BytesStart<'_>) -> Result<String, UnitRulesError> {
    std::str::from_utf8(element.name().as_ref())
        .map(str::to_string)
        .map_err(|error| UnitRulesError::Xml(error.to_string()))
}

fn reject_attributes(element: &BytesStart<'_>) -> Result<(), UnitRulesError> {
    if let Some(attribute) = element.attributes().next() {
        let attribute = attribute.map_err(|error| UnitRulesError::Xml(error.to_string()))?;
        let key = std::str::from_utf8(attribute.key.as_ref())
            .map_err(|error| UnitRulesError::Xml(error.to_string()))?;
        return Err(UnitRulesError::UnexpectedAttribute {
            element: element_name(element)?,
            attribute: key.to_string(),
        });
    }
    Ok(())
}

fn unexpected(stack: &[String], element: String) -> UnitRulesError {
    UnitRulesError::UnexpectedElement {
        path: if stack.is_empty() {
            "document root".to_string()
        } else {
            stack.join("/")
        },
        element,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn fixture_rows(names: &[&str]) -> String {
        let mut out = String::from("<?xml version=\"1.0\"?><ROOT><COMMENTS></COMMENTS>");
        for name in names {
            out.push_str("<UNIT>");
            for field in UnitRuleField::ALL {
                if field == UnitRuleField::Name {
                    out.push_str("<NAME>");
                    out.push_str(name);
                    out.push_str("</NAME>");
                } else if field.may_be_empty() {
                    out.push('<');
                    out.push_str(field.tag());
                    out.push_str("/>");
                } else {
                    out.push('<');
                    out.push_str(field.tag());
                    out.push_str(">v</");
                    out.push_str(field.tag());
                    out.push('>');
                }
            }
            out.push_str("</UNIT>");
        }
        out.push_str("</ROOT>");
        out
    }

    #[test]
    fn same_name_rows_are_not_overwritten() {
        let parsed = parse_with_expected_rows(&fixture_rows(&["Citizen", "Citizen"]), 2).unwrap();
        assert_eq!(
            parsed.by_type_index(50).unwrap().canonical_key().occurrence,
            0
        );
        assert_eq!(
            parsed.by_type_index(51).unwrap().canonical_key().occurrence,
            1
        );
        assert_eq!(parsed.same_name("Citizen").count(), 2);
        assert_eq!(
            parsed
                .by_name_occurrence("Citizen", 1)
                .unwrap()
                .type_index(),
            51
        );
        assert!(parsed.by_name_occurrence("citizen", 0).is_none());
    }

    #[test]
    fn field_order_and_closed_schema_are_enforced() {
        let valid = fixture_rows(&["Citizen"]);
        let swapped = valid.replacen(
            "<GRAPH>v</GRAPH><OBJ_MASK>v</OBJ_MASK>",
            "<OBJ_MASK>v</OBJ_MASK><GRAPH>v</GRAPH>",
            1,
        );
        assert!(matches!(
            parse_with_expected_rows(&swapped, 1),
            Err(UnitRulesError::WrongField {
                position: 1,
                expected: "GRAPH",
                ..
            })
        ));

        let unknown = valid.replacen("<GRAPH>v</GRAPH>", "<UNKNOWN>v</UNKNOWN>", 1);
        assert!(matches!(
            parse_with_expected_rows(&unknown, 1),
            Err(UnitRulesError::WrongField { found, .. }) if found == "UNKNOWN"
        ));

        let attributed = valid.replacen("<NAME>", "<NAME x=\"1\">", 1);
        assert!(matches!(
            parse_with_expected_rows(&attributed, 1),
            Err(UnitRulesError::UnexpectedAttribute { .. })
        ));

        let nested = valid.replacen("<GRAPH>v</GRAPH>", "<GRAPH><X>v</X></GRAPH>", 1);
        assert!(matches!(
            parse_with_expected_rows(&nested, 1),
            Err(UnitRulesError::UnexpectedElement { element, .. }) if element == "X"
        ));

        assert!(matches!(
            parse_with_expected_rows(&fixture_rows(&[""]), 1),
            Err(UnitRulesError::EmptyName { row: 0 })
        ));
    }

    #[test]
    fn missing_rows_and_fields_fail_closed() {
        assert!(matches!(
            parse_unitrules_xml(&fixture_rows(&["Citizen"])),
            Err(UnitRulesError::WrongRowCount {
                expected: UNIT_RULE_ROWS,
                found: 1
            })
        ));
        let missing = fixture_rows(&["Citizen"]).replacen("<TYPENAME>v</TYPENAME>", "", 1);
        assert!(matches!(
            parse_with_expected_rows(&missing, 1),
            Err(UnitRulesError::MissingFields { found: 54, .. })
        ));
    }

    #[test]
    fn extracted_shipped_table_has_the_measured_364_by_55_boundary() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ron-data/unitrules.xml");
        let Ok(text) = fs::read_to_string(&path) else {
            eprintln!("skipping: {} is an extracted local asset", path.display());
            return;
        };
        assert_eq!(text.len(), SHIPPED_UNITRULES_BYTES);
        let parsed = parse_unitrules_xml(&text).unwrap();
        assert_eq!(parsed.len(), 364);
        assert_eq!(
            parsed.by_type_index(50).unwrap().get(UnitRuleField::Name),
            "Citizen"
        );
        assert_eq!(
            parsed.by_type_index(51).unwrap().get(UnitRuleField::Name),
            "Citizen"
        );
        assert_eq!(
            parsed.by_type_index(413).unwrap().get(UnitRuleField::Name),
            "Herd Peacock"
        );

        let distinct_names: BTreeSet<_> = parsed
            .rows()
            .iter()
            .map(|row| row.get(UnitRuleField::Name))
            .collect();
        let repeated_groups = distinct_names
            .iter()
            .filter(|name| parsed.same_name(name).count() > 1)
            .count();
        assert_eq!(distinct_names.len(), 300);
        assert_eq!(repeated_groups, 33);
        assert_eq!(364 - distinct_names.len(), 64);
        assert_eq!(parsed.same_name("General").count(), 4);
        assert_eq!(parsed.same_name("Assault Infantry").count(), 3);

        let distinct_type_names: BTreeSet<_> = parsed
            .rows()
            .iter()
            .map(|row| row.get(UnitRuleField::TypeName))
            .collect();
        let distinct_graphs: BTreeSet<_> = parsed
            .rows()
            .iter()
            .map(|row| row.get(UnitRuleField::Graph))
            .collect();
        assert_eq!(distinct_type_names.len(), 352);
        assert_eq!(distinct_graphs.len(), 351);
    }
}
