//! A checked, portable artifact for DoN's field-level rule overlay.
//!
//! Retail has no equivalent: its content layer replaces whole files. The format therefore
//! says `DON_OVERLAY` rather than masquerading as retail XML, and it is always an improved-
//! mode artifact. Text values go through [`crate::overlay::Patch::from_text`], so fractions,
//! scales, names, and array bounds use the recovered `don-rules` bindings.
//!
//! ```xml
//! <DON_OVERLAY version="1">
//!   <PATCH field="peasant_rate" index="0" value="1.25"
//!          note="Independent-edition balance pass"/>
//! </DON_OVERLAY>
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::overlay::{Attribution, Layer, Mode, OverlayError, Patch, RuleStack};

#[derive(Clone, Debug)]
pub struct OverlayFile {
    pub version: u32,
    pub patches: Vec<Patch>,
    pub attributions: Vec<Attribution>,
}

#[derive(Debug)]
pub enum OverlayFileError {
    Io(std::io::Error),
    Xml(String),
    MissingRoot,
    UnsupportedVersion(String),
    UnexpectedElement(String),
    UnknownAttribute {
        element: String,
        attribute: String,
    },
    MissingAttribute {
        element: String,
        attribute: &'static str,
    },
    InvalidIndex(String),
    DuplicateTarget {
        field: String,
        index: u16,
    },
    Patch(OverlayError),
    Validation(Vec<OverlayError>),
    Empty,
}

impl fmt::Display for OverlayFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "cannot read overlay: {e}"),
            Self::Xml(e) => write!(f, "malformed overlay XML: {e}"),
            Self::MissingRoot => write!(f, "overlay has no DON_OVERLAY root"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported DON_OVERLAY version {v:?}"),
            Self::UnexpectedElement(e) => write!(f, "unexpected overlay element {e:?}"),
            Self::UnknownAttribute { element, attribute } => {
                write!(f, "unknown {element} attribute {attribute:?}")
            }
            Self::MissingAttribute { element, attribute } => {
                write!(f, "{element} requires attribute {attribute:?}")
            }
            Self::InvalidIndex(v) => write!(
                f,
                "PATCH index must be an unsigned 16-bit integer, got {v:?}"
            ),
            Self::DuplicateTarget { field, index } => {
                write!(f, "overlay writes {field}[{index}] more than once")
            }
            Self::Patch(e) => write!(f, "invalid PATCH: {e}"),
            Self::Validation(es) => {
                write!(f, "overlay validation failed")?;
                for e in es {
                    write!(f, "; {e}")?;
                }
                Ok(())
            }
            Self::Empty => write!(f, "overlay contains no PATCH elements"),
        }
    }
}

impl std::error::Error for OverlayFileError {}

pub fn read_overlay(path: &Path) -> Result<OverlayFile, OverlayFileError> {
    let text = std::fs::read_to_string(path).map_err(OverlayFileError::Io)?;
    parse_overlay(&text)
}

pub fn parse_overlay(text: &str) -> Result<OverlayFile, OverlayFileError> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut stack: Vec<Vec<u8>> = Vec::new();
    let mut root_version = None;
    let mut patches = Vec::new();
    let mut targets = BTreeSet::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                visit(
                    &reader,
                    &stack,
                    &e,
                    &mut root_version,
                    &mut patches,
                    &mut targets,
                )?;
                stack.push(e.name().as_ref().to_vec());
            }
            Ok(Event::Empty(e)) => {
                visit(
                    &reader,
                    &stack,
                    &e,
                    &mut root_version,
                    &mut patches,
                    &mut targets,
                )?;
            }
            Ok(Event::End(_)) => {
                stack.pop();
            }
            Ok(Event::Eof) => break,
            Ok(Event::Text(t)) if !stack.is_empty() => {
                let s = t
                    .decode()
                    .map_err(|e| OverlayFileError::Xml(e.to_string()))?;
                if !s.trim().is_empty() {
                    return Err(OverlayFileError::UnexpectedElement(
                        "text content".to_string(),
                    ));
                }
            }
            Ok(_) => {}
            Err(e) => return Err(OverlayFileError::Xml(e.to_string())),
        }
    }

    let version = root_version.ok_or(OverlayFileError::MissingRoot)?;
    if patches.is_empty() {
        return Err(OverlayFileError::Empty);
    }
    let mut rules = RuleStack::shipped(Mode::Improved);
    rules.extend(Layer::Overlay, patches.iter().cloned());
    let (_, attributions) = rules.compose().map_err(OverlayFileError::Validation)?;
    Ok(OverlayFile {
        version,
        patches,
        attributions,
    })
}

fn visit(
    reader: &Reader<&[u8]>,
    stack: &[Vec<u8>],
    e: &BytesStart<'_>,
    root_version: &mut Option<u32>,
    patches: &mut Vec<Patch>,
    targets: &mut BTreeSet<(String, u16)>,
) -> Result<(), OverlayFileError> {
    let tag = std::str::from_utf8(e.name().as_ref())
        .map_err(|e| OverlayFileError::Xml(e.to_string()))?
        .to_string();
    let attrs = attributes(reader, e)?;
    if stack.is_empty() {
        if tag != "DON_OVERLAY" {
            return Err(OverlayFileError::MissingRoot);
        }
        reject_unknown(&tag, &attrs, &["version"])?;
        let version = attrs
            .get("version")
            .ok_or(OverlayFileError::MissingAttribute {
                element: tag,
                attribute: "version",
            })?;
        if version != "1" {
            return Err(OverlayFileError::UnsupportedVersion(version.clone()));
        }
        *root_version = Some(1);
        return Ok(());
    }
    if stack.len() != 1 || stack[0].as_slice() != b"DON_OVERLAY" || tag != "PATCH" {
        return Err(OverlayFileError::UnexpectedElement(tag));
    }
    reject_unknown(&tag, &attrs, &["field", "index", "value", "note"])?;
    let field = required(&attrs, "PATCH", "field")?;
    let value = required(&attrs, "PATCH", "value")?;
    let index = match attrs.get("index") {
        Some(v) => v
            .parse::<u16>()
            .map_err(|_| OverlayFileError::InvalidIndex(v.clone()))?,
        None => 0,
    };
    let mut patch = Patch::from_text(field, index, value).map_err(OverlayFileError::Patch)?;
    if let Some(note) = attrs.get("note") {
        patch = patch.why(note);
    }
    if !targets.insert((patch.field.clone(), patch.index)) {
        return Err(OverlayFileError::DuplicateTarget {
            field: patch.field,
            index: patch.index,
        });
    }
    patches.push(patch);
    Ok(())
}

fn attributes(
    reader: &Reader<&[u8]>,
    e: &BytesStart<'_>,
) -> Result<BTreeMap<String, String>, OverlayFileError> {
    let mut out = BTreeMap::new();
    for attr in e.attributes() {
        let attr = attr.map_err(|e| OverlayFileError::Xml(e.to_string()))?;
        let key = std::str::from_utf8(attr.key.as_ref())
            .map_err(|e| OverlayFileError::Xml(e.to_string()))?
            .to_string();
        let value = attr
            .decode_and_unescape_value(reader.decoder())
            .map_err(|e| OverlayFileError::Xml(e.to_string()))?
            .into_owned();
        if out.insert(key.clone(), value).is_some() {
            return Err(OverlayFileError::Xml(format!(
                "duplicate attribute {key:?}"
            )));
        }
    }
    Ok(out)
}

fn reject_unknown(
    element: &str,
    attrs: &BTreeMap<String, String>,
    allowed: &[&str],
) -> Result<(), OverlayFileError> {
    if let Some(k) = attrs.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(OverlayFileError::UnknownAttribute {
            element: element.to_string(),
            attribute: k.clone(),
        });
    }
    Ok(())
}

fn required<'a>(
    attrs: &'a BTreeMap<String, String>,
    element: &str,
    attribute: &'static str,
) -> Result<&'a str, OverlayFileError> {
    attrs
        .get(attribute)
        .map(String::as_str)
        .ok_or_else(|| OverlayFileError::MissingAttribute {
            element: element.to_string(),
            attribute,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_values_use_the_recovered_field_parser() {
        let f = parse_overlay(
            r#"<DON_OVERLAY version="1">
              <PATCH field="peasant_rate" value="1.25" note="test"/>
            </DON_OVERLAY>"#,
        )
        .unwrap();
        assert_eq!(f.patches.len(), 1);
        assert_eq!(f.patches[0].note, "test");
        assert_eq!(f.attributions.len(), 1);
    }

    #[test]
    fn typo_unknown_attribute_and_duplicate_target_fail_closed() {
        assert!(matches!(
            parse_overlay(
                r#"<DON_OVERLAY version="1"><PATCH field="not_a_rule" value="2"/></DON_OVERLAY>"#
            ),
            Err(OverlayFileError::Patch(OverlayError::UnknownField(_)))
        ));
        assert!(matches!(
            parse_overlay(
                r#"<DON_OVERLAY version="1"><PATCH field="peasant_rate" val="2"/></DON_OVERLAY>"#
            ),
            Err(OverlayFileError::UnknownAttribute { .. })
        ));
        assert!(matches!(
            parse_overlay(
                r#"<DON_OVERLAY version="1">
                <PATCH field="peasant_rate" value="2"/>
                <PATCH field="peasant_rate" value="3"/>
                </DON_OVERLAY>"#
            ),
            Err(OverlayFileError::DuplicateTarget { .. })
        ));
    }

    #[test]
    fn an_overlay_cannot_claim_fidelity_or_a_new_schema_version() {
        assert!(matches!(
            parse_overlay(
                r#"<DON_OVERLAY version="2"><PATCH field="peasant_rate" value="2"/></DON_OVERLAY>"#
            ),
            Err(OverlayFileError::UnsupportedVersion(_))
        ));
    }
}
