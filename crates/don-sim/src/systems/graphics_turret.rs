//! Retail graphics-turret materialization and aiming.
//!
//! This module owns the simulation-visible boundary between installed graphics data and
//! [`GuyData`](super::groups_guys::GuyData). It does not infer a turret from a unit role or
//! a numeric UnitType flag. `Guy::init_real` `0x005DB6B0` asks
//! `GraphicPieces::get_unit_gpiece` for the selected `RData`, initializes that piece's
//! pivot state, and sets `GuyData +0x9A & 0x0100` only when the selected unit graph has a
//! non-empty pivot-restriction list.
//!
//! The restriction lists are recoverable exactly from the supported installed
//! `Data/unit_graphics.xml`. Initial pivot angles and the node offsets used by
//! `Guy::set_all_pivots` `0x005D8BC0` are not: they come from the loaded `.bh3` hierarchy
//! through `GraphicPieces::get_position`. Accordingly, the APIs below admit only:
//!
//! * the hash-gated shipped XML catalog;
//! * a coherent retail/exact-hierarchy extractor result for initial Guy fields; and
//! * a fail-closed position provider for each live aim evaluation.
//!
//! Missing hierarchy data is an error, never a body-tracking or zero-offset fallback.

use std::collections::{BTreeMap, BTreeSet};

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use super::groups_guys::{GuyData, UnitGuys, GUY_FLAG_TURRETS, TURRET_STEP};
use super::unit_inctime::{
    angle_to_degrees, SUPPORTED_RETAIL_EXE_SHA256, SUPPORTED_UNIT_GRAPHICS_SHA256,
};
use crate::trig::find_angle;

/// Exact byte length of the supported retail `Data/unit_graphics.xml`.
pub const SUPPORTED_UNIT_GRAPHICS_LEN: usize = 3_440_809;
/// Number of `<UNIT>` records measured in the supported file.
pub const SUPPORTED_UNIT_GRAPHICS_UNIT_ROWS: usize = 1_435;
/// Number of `<RESTRICTION>` records measured in the supported file.
pub const SUPPORTED_UNIT_GRAPHICS_RESTRICTION_ROWS: usize = 81;

/// Identity supplied by the installed-data or live-process graphics resolver.
///
/// The producer must compute the two hashes over the executable and the installed XML.
/// `coherent_capture` means all resolved gpieces and hierarchy-derived fields in one
/// extraction came from one stopped/main-thread transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphicsProvenance {
    pub executable_sha256: [u8; 32],
    pub installed_unit_graphics_sha256: [u8; 32],
    pub coherent_capture: bool,
}

/// One `GraphicPieces::pivot_restrictions` list entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PivotRestriction {
    /// Hierarchy node metric. Shipped unit pivots are the sequential nodes `4..=7`.
    pub node: u8,
    /// `int(Vector<float>.x)` returned by `GraphicPieces::get_restrictions`.
    pub min_degrees: i32,
    /// `int(Vector<float>.y)` returned by `GraphicPieces::get_restrictions`.
    pub max_degrees: i32,
}

/// Fail-closed installed graphics/catalog/materialization errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GraphicsResourceError {
    UnsupportedExecutable,
    MissingInstalledDataIdentity,
    UnsupportedInstalledUnitGraphics,
    WrongInstalledUnitGraphicsLength {
        expected: usize,
        actual: usize,
    },
    IncoherentCapture,
    Xml(String),
    MissingAttribute {
        element: &'static str,
        attribute: &'static str,
    },
    DuplicateAttribute {
        element: &'static str,
        attribute: &'static str,
    },
    NonAsciiGraphName(String),
    InvalidRestrictionNumber {
        graph: String,
        attribute: &'static str,
        value: String,
    },
    InvalidRestrictionNode {
        graph: String,
        node: i32,
    },
    UnknownRestrictionGraph(String),
    NonSequentialRestrictionNode {
        graph: String,
        expected: u8,
        actual: u8,
    },
    WrongSupportedUnitRowCount {
        expected: usize,
        actual: usize,
    },
    WrongSupportedRestrictionRowCount {
        expected: usize,
        actual: usize,
    },
    UnknownUnitGraph(String),
    WrongExtractedGraph {
        requested: String,
        extracted: String,
    },
    WrongSlotCount {
        expected: usize,
        actual: usize,
    },
    WrongSlotPresence {
        slot: usize,
    },
    WrongGuyNumber {
        slot: usize,
        expected: i8,
        actual: i8,
    },
    MissingGpiece {
        slot: usize,
    },
    InvalidNodeFlags {
        slot: usize,
        flags: u16,
        allowed: u16,
    },
    NonTurretPivotState {
        slot: usize,
    },
    ExtractorFailure(String),
}

/// Ordered pivot restrictions keyed by retail's case-insensitive graph name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitGraphicsCatalog {
    provenance: GraphicsProvenance,
    unit_graphs: BTreeSet<String>,
    restrictions: BTreeMap<String, Vec<PivotRestriction>>,
}

impl UnitGraphicsCatalog {
    /// Parse the one supported installed `unit_graphics.xml`.
    ///
    /// The hash is deliberately producer-supplied just as it is for the existing graphics
    /// event extractor. The exact byte length and measured record counts are independently
    /// checked before the catalog is admitted, and malformed or structurally different XML
    /// is rejected.
    pub fn from_installed_xml(
        xml: &[u8],
        provenance: GraphicsProvenance,
    ) -> Result<Self, GraphicsResourceError> {
        validate_provenance(provenance)?;
        if xml.len() != SUPPORTED_UNIT_GRAPHICS_LEN {
            return Err(GraphicsResourceError::WrongInstalledUnitGraphicsLength {
                expected: SUPPORTED_UNIT_GRAPHICS_LEN,
                actual: xml.len(),
            });
        }
        Self::parse(xml, provenance, true)
    }

    #[cfg(test)]
    fn from_fixture(xml: &[u8]) -> Result<Self, GraphicsResourceError> {
        Self::parse(xml, supported_provenance(), false)
    }

    fn parse(
        xml: &[u8],
        provenance: GraphicsProvenance,
        enforce_supported_counts: bool,
    ) -> Result<Self, GraphicsResourceError> {
        let mut reader = Reader::from_reader(xml);
        reader.config_mut().trim_text(true);
        let mut known_graphs = BTreeSet::new();
        let mut pending = Vec::<(String, PivotRestriction)>::new();
        let mut unit_rows = 0usize;
        let mut restriction_rows = 0usize;

        loop {
            let event = reader
                .read_event()
                .map_err(|e| GraphicsResourceError::Xml(e.to_string()))?;
            let element = match event {
                Event::Start(ref e) | Event::Empty(ref e) => Some(e),
                Event::Eof => break,
                _ => None,
            };
            let Some(element) = element else { continue };
            match element.name().as_ref() {
                b"UNIT" => {
                    unit_rows += 1;
                    let name = required_attr(element, "UNIT", b"name", "name")?;
                    let graph = name.split('-').next().unwrap_or(&name);
                    known_graphs.insert(normalize_graph(graph)?);
                }
                b"RESTRICTION" => {
                    restriction_rows += 1;
                    let graph = required_attr(element, "RESTRICTION", b"name", "name")?;
                    let graph_key = normalize_graph(&graph)?;
                    let node = restriction_i32(element, &graph, b"node", "node")?;
                    if !(4..=7).contains(&node) {
                        return Err(GraphicsResourceError::InvalidRestrictionNode { graph, node });
                    }
                    let min_degrees = restriction_i32(element, &graph, b"minangle", "minangle")?;
                    let max_degrees = restriction_i32(element, &graph, b"maxangle", "maxangle")?;
                    pending.push((
                        graph_key,
                        PivotRestriction {
                            node: node as u8,
                            min_degrees,
                            max_degrees,
                        },
                    ));
                }
                _ => {}
            }
        }

        if enforce_supported_counts && unit_rows != SUPPORTED_UNIT_GRAPHICS_UNIT_ROWS {
            return Err(GraphicsResourceError::WrongSupportedUnitRowCount {
                expected: SUPPORTED_UNIT_GRAPHICS_UNIT_ROWS,
                actual: unit_rows,
            });
        }
        if enforce_supported_counts && restriction_rows != SUPPORTED_UNIT_GRAPHICS_RESTRICTION_ROWS
        {
            return Err(GraphicsResourceError::WrongSupportedRestrictionRowCount {
                expected: SUPPORTED_UNIT_GRAPHICS_RESTRICTION_ROWS,
                actual: restriction_rows,
            });
        }

        let mut restrictions = BTreeMap::<String, Vec<PivotRestriction>>::new();
        for (graph, restriction) in pending {
            // This name is the graph type returned by `get_type(gpiece)`, not the
            // UnitType `<GRAPH>` / `<UNIT name>` prefix. For example, BATTLESHIP's
            // selected model resolves the restriction graph `SuperBattleship`.
            let list = restrictions.entry(graph.clone()).or_default();
            let expected = 4u8.wrapping_add(list.len() as u8);
            if restriction.node != expected {
                return Err(GraphicsResourceError::NonSequentialRestrictionNode {
                    graph,
                    expected,
                    actual: restriction.node,
                });
            }
            list.push(restriction);
        }

        Ok(Self {
            provenance,
            unit_graphs: known_graphs,
            restrictions,
        })
    }

    /// Validate a UnitType `<GRAPH>` against the shipped `<UNIT name>` prefixes.
    pub fn validate_unit_graph(&self, graph_name: &str) -> Result<(), GraphicsResourceError> {
        let key = normalize_graph(graph_name)?;
        if self.unit_graphs.contains(&key) {
            Ok(())
        } else {
            Err(GraphicsResourceError::UnknownUnitGraph(
                graph_name.to_owned(),
            ))
        }
    }

    /// Exact restriction list for the graph type returned by `get_type(gpiece)`.
    pub fn pivot_restrictions(
        &self,
        pivot_graph_name: &str,
    ) -> Result<&[PivotRestriction], GraphicsResourceError> {
        let key = normalize_graph(pivot_graph_name)?;
        self.restrictions
            .get(&key)
            .map(Vec::as_slice)
            .ok_or_else(|| {
                GraphicsResourceError::UnknownRestrictionGraph(pivot_graph_name.to_owned())
            })
    }

    /// Whether this resolved gpiece graph makes `Guy::init_real` set `GUY_FLAG_TURRETS`.
    pub fn has_pivot_restrictions(
        &self,
        pivot_graph_name: &str,
    ) -> Result<bool, GraphicsResourceError> {
        Ok(!self.pivot_restrictions(pivot_graph_name)?.is_empty())
    }

    pub fn provenance(&self) -> GraphicsProvenance {
        self.provenance
    }
}

fn validate_provenance(provenance: GraphicsProvenance) -> Result<(), GraphicsResourceError> {
    if provenance.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256 {
        return Err(GraphicsResourceError::UnsupportedExecutable);
    }
    if provenance.installed_unit_graphics_sha256 == [0; 32] {
        return Err(GraphicsResourceError::MissingInstalledDataIdentity);
    }
    if provenance.installed_unit_graphics_sha256 != SUPPORTED_UNIT_GRAPHICS_SHA256 {
        return Err(GraphicsResourceError::UnsupportedInstalledUnitGraphics);
    }
    if !provenance.coherent_capture {
        return Err(GraphicsResourceError::IncoherentCapture);
    }
    Ok(())
}

fn normalize_graph(graph: &str) -> Result<String, GraphicsResourceError> {
    if !graph.is_ascii() {
        return Err(GraphicsResourceError::NonAsciiGraphName(graph.to_owned()));
    }
    Ok(graph.to_ascii_uppercase())
}

fn required_attr(
    element: &BytesStart<'_>,
    element_name: &'static str,
    key: &[u8],
    attribute_name: &'static str,
) -> Result<String, GraphicsResourceError> {
    let mut found = None;
    for attr in element.attributes() {
        let attr = attr.map_err(|e| GraphicsResourceError::Xml(e.to_string()))?;
        if attr.key.as_ref() != key {
            continue;
        }
        if found.is_some() {
            return Err(GraphicsResourceError::DuplicateAttribute {
                element: element_name,
                attribute: attribute_name,
            });
        }
        found = Some(
            std::str::from_utf8(attr.value.as_ref())
                .map_err(|e| GraphicsResourceError::Xml(e.to_string()))?
                .to_owned(),
        );
    }
    found.ok_or(GraphicsResourceError::MissingAttribute {
        element: element_name,
        attribute: attribute_name,
    })
}

fn restriction_i32(
    element: &BytesStart<'_>,
    graph: &str,
    key: &[u8],
    attribute_name: &'static str,
) -> Result<i32, GraphicsResourceError> {
    let value = required_attr(element, "RESTRICTION", key, attribute_name)?;
    let parsed =
        value
            .parse::<f32>()
            .map_err(|_| GraphicsResourceError::InvalidRestrictionNumber {
                graph: graph.to_owned(),
                attribute: attribute_name,
                value: value.clone(),
            })?;
    if !parsed.is_finite() || parsed < i32::MIN as f32 || parsed >= 2_147_483_648.0 {
        return Err(GraphicsResourceError::InvalidRestrictionNumber {
            graph: graph.to_owned(),
            attribute: attribute_name,
            value,
        });
    }
    // `GraphicPieces::get_restrictions` converts the stored f32 with `cvttss2si`.
    Ok(parsed.trunc() as i32)
}

/// All graphics-owned Guy fields resolved by the retail hierarchy for one pointer slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractedGuyGraphics {
    pub guy_num: i8,
    pub gpiece: i32,
    /// Exact name installed by `GraphicPieces::set_unit_graph_name(get_type(gpiece))`.
    /// `None` means the coherent resolver found a zero-length restriction list.
    pub pivot_graph_name: Option<String>,
    pub track_dx: i32,
    pub track_dy: i32,
    pub turret_angles: [i32; 4],
    pub des_turret_angles: [i32; 4],
    pub node_flags: i16,
    pub des_node_flags: i16,
}

/// Transactional result of the retail `get_unit_gpiece` / `.bh3` initialization path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractedUnitGraphics {
    pub graph_name: String,
    /// Exact `PtrArray<Guy>` slot order; `None` corresponds to a null Guy pointer.
    pub slots: Vec<Option<ExtractedGuyGraphics>>,
}

/// Exact-hierarchy provider for initial graphics state.
pub trait GuyGraphicsExtractor {
    fn provenance(&self) -> GraphicsProvenance;
    fn extract_unit_graphics(
        &mut self,
        graph_name: &str,
        guy_numbers: &[Option<i8>],
    ) -> Result<ExtractedUnitGraphics, GraphicsResourceError>;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GraphicsMaterializationStats {
    pub live_guys: usize,
    pub turret_guys: usize,
}

/// Install exact graphics-derived state into a fully allocated [`UnitGuys`].
///
/// Validation completes against a clone before any caller-visible write. This matters for
/// arena/replay hosts: a missing gpiece, wrong null slot, stale guy number, unsupported
/// capture, or impossible pivot bits leaves the whole unit byte-for-byte unchanged.
pub fn materialize_unit_graphics<E: GuyGraphicsExtractor>(
    catalog: &UnitGraphicsCatalog,
    graph_name: &str,
    guys: &mut UnitGuys,
    extractor: &mut E,
) -> Result<GraphicsMaterializationStats, GraphicsResourceError> {
    validate_provenance(extractor.provenance())?;
    if extractor.provenance() != catalog.provenance {
        return Err(GraphicsResourceError::UnsupportedInstalledUnitGraphics);
    }
    catalog.validate_unit_graph(graph_name)?;
    let guy_numbers = guys
        .guys
        .iter()
        .map(|slot| slot.as_ref().map(|guy| guy.guy_num))
        .collect::<Vec<_>>();
    let extracted = extractor.extract_unit_graphics(graph_name, &guy_numbers)?;
    if normalize_graph(&extracted.graph_name)? != normalize_graph(graph_name)? {
        return Err(GraphicsResourceError::WrongExtractedGraph {
            requested: graph_name.to_owned(),
            extracted: extracted.graph_name,
        });
    }
    if extracted.slots.len() != guys.guys.len() {
        return Err(GraphicsResourceError::WrongSlotCount {
            expected: guys.guys.len(),
            actual: extracted.slots.len(),
        });
    }

    let mut staged = guys.clone();
    let mut stats = GraphicsMaterializationStats::default();
    for (slot, (guy, profile)) in staged
        .guys
        .iter_mut()
        .zip(extracted.slots.iter())
        .enumerate()
    {
        let (Some(guy), Some(profile)) = (guy.as_mut(), profile.as_ref()) else {
            if guy.is_some() != profile.is_some() {
                return Err(GraphicsResourceError::WrongSlotPresence { slot });
            }
            continue;
        };
        if profile.guy_num != guy.guy_num {
            return Err(GraphicsResourceError::WrongGuyNumber {
                slot,
                expected: guy.guy_num,
                actual: profile.guy_num,
            });
        }
        if profile.gpiece < 0 {
            return Err(GraphicsResourceError::MissingGpiece { slot });
        }
        let restriction_count = match profile.pivot_graph_name.as_deref() {
            Some(pivot_graph) => catalog.pivot_restrictions(pivot_graph)?.len(),
            None => 0,
        };
        let turret = restriction_count != 0;
        let allowed = (1u16 << restriction_count) - 1;
        let flags = (profile.node_flags as u16) | (profile.des_node_flags as u16);
        if flags & !allowed != 0 {
            return Err(GraphicsResourceError::InvalidNodeFlags {
                slot,
                flags,
                allowed,
            });
        }
        if !turret
            && (profile.turret_angles != [0; 4]
                || profile.des_turret_angles != [0; 4]
                || flags != 0)
        {
            return Err(GraphicsResourceError::NonTurretPivotState { slot });
        }

        guy.gpiece = profile.gpiece;
        guy.track_dx = profile.track_dx;
        guy.track_dy = profile.track_dy;
        guy.turret_angles = profile.turret_angles;
        guy.des_turret_angles = profile.des_turret_angles;
        guy.node_flags = profile.node_flags;
        guy.des_node_flags = profile.des_node_flags;
        if turret {
            guy.guy_flags |= GUY_FLAG_TURRETS;
            stats.turret_guys += 1;
        } else {
            guy.guy_flags &= !GUY_FLAG_TURRETS;
        }
        stats.live_guys += 1;
    }

    *guys = staged;
    Ok(stats)
}

/// Exact arguments needed by `GraphicPieces::get_position` for one pivot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PivotOffsetRequest {
    pub gpiece: i32,
    pub node: u8,
    /// Retail passes `float(angle_to_degrees(guy.angle - 0x80000000))`.
    pub body_angle_degrees: f32,
    /// Current graphics-node rotations at `GuyData +0x20`.
    pub current_turret_angles: [i32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PivotOffset {
    pub x: f32,
    pub y: f32,
}

/// Fail-closed `.bh3` hierarchy position evaluator.
pub trait PivotOffsetProvider {
    fn pivot_offset(&mut self, request: PivotOffsetRequest) -> Result<PivotOffset, String>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TurretAimError {
    Resource(GraphicsResourceError),
    TurretFlagMismatch {
        graph_has_turrets: bool,
        guy_has_turrets: bool,
    },
    MissingGpiece,
    InvalidPivotOffset {
        node: u8,
    },
    ProviderFailure {
        node: u8,
        message: String,
    },
}

impl From<GraphicsResourceError> for TurretAimError {
    fn from(value: GraphicsResourceError) -> Self {
        Self::Resource(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TurretAimResolution {
    /// Exact return value of the recovered graphics-pivot arm.
    pub aligned: bool,
    pub evaluated_nodes: usize,
}

/// Evaluate the graphics-pivot arm of `Guy::set_all_pivots` `0x005D8BC0`.
///
/// `unit_x/unit_y` and the already-resolved target coordinates are explicit because the
/// retail function obtains them through object tables before entering its pivot loop. The
/// provider is called for every node before mutation, so a missing hierarchy cannot leave
/// a partially updated Guy.
pub fn resolve_turret_aim<P: PivotOffsetProvider>(
    catalog: &UnitGraphicsCatalog,
    pivot_graph_name: Option<&str>,
    guy: &mut GuyData,
    unit_x: i32,
    unit_y: i32,
    target_x: i32,
    target_y: i32,
    provider: &mut P,
) -> Result<TurretAimResolution, TurretAimError> {
    let restrictions = match pivot_graph_name {
        Some(name) => catalog.pivot_restrictions(name)?,
        None => &[],
    };
    let graph_has_turrets = !restrictions.is_empty();
    let guy_has_turrets = guy.guy_flags & GUY_FLAG_TURRETS != 0;
    if graph_has_turrets != guy_has_turrets {
        return Err(TurretAimError::TurretFlagMismatch {
            graph_has_turrets,
            guy_has_turrets,
        });
    }
    if !guy_has_turrets {
        return Ok(TurretAimResolution {
            aligned: false,
            evaluated_nodes: 0,
        });
    }
    if guy.gpiece < 0 {
        return Err(TurretAimError::MissingGpiece);
    }

    let body_angle_degrees = angle_to_degrees(guy.angle.wrapping_sub(i32::MIN)) as f32;
    let mut offsets = Vec::with_capacity(restrictions.len());
    for restriction in restrictions {
        let offset = provider
            .pivot_offset(PivotOffsetRequest {
                gpiece: guy.gpiece,
                node: restriction.node,
                body_angle_degrees,
                current_turret_angles: guy.turret_angles,
            })
            .map_err(|message| TurretAimError::ProviderFailure {
                node: restriction.node,
                message,
            })?;
        let x = cvttss2si_checked(offset.x).ok_or(TurretAimError::InvalidPivotOffset {
            node: restriction.node,
        })?;
        let y = cvttss2si_checked(offset.y).ok_or(TurretAimError::InvalidPivotOffset {
            node: restriction.node,
        })?;
        offsets.push((x, y));
    }

    let mut aligned = true;
    let mut next_des = guy.des_turret_angles;
    let mut next_node_flags = 0u16;
    let mut next_des_node_flags = 0u16;
    for (index, (restriction, &(offset_x, offset_y))) in
        restrictions.iter().zip(offsets.iter()).enumerate()
    {
        let dx = target_x.wrapping_sub(unit_x).wrapping_sub(offset_x);
        let dy = target_y.wrapping_sub(unit_y).wrapping_sub(offset_y);
        let target_angle = find_angle(dx, dy);
        let desired_relative = target_angle.wrapping_sub(guy.angle);
        let degrees = angle_to_degrees(desired_relative);
        let signed_degrees = if degrees > 180 {
            degrees - 360
        } else {
            degrees
        };

        if !(-45..=45).contains(&signed_degrees) {
            aligned = false;
        }
        let inside = if restriction.min_degrees < restriction.max_degrees {
            signed_degrees >= restriction.min_degrees && signed_degrees <= restriction.max_degrees
        } else {
            signed_degrees >= restriction.min_degrees || signed_degrees <= restriction.max_degrees
        };
        if !inside {
            aligned = false;
            continue;
        }

        next_des[index] = desired_relative;
        next_des_node_flags |= 1 << index;
        let mut delta = (desired_relative as u32).wrapping_sub(guy.turret_angles[index] as u32);
        if delta > 0x8000_0000 {
            delta = !delta;
        }
        if delta < TURRET_STEP {
            next_node_flags |= 1 << index;
        }
    }

    guy.des_turret_angles = next_des;
    guy.node_flags = next_node_flags as i16;
    guy.des_node_flags = next_des_node_flags as i16;
    Ok(TurretAimResolution {
        aligned,
        evaluated_nodes: restrictions.len(),
    })
}

fn cvttss2si_checked(value: f32) -> Option<i32> {
    if !value.is_finite() || value < -2_147_483_648.0 || value >= 2_147_483_648.0 {
        None
    } else {
        Some(value.trunc() as i32)
    }
}

#[cfg(test)]
fn supported_provenance() -> GraphicsProvenance {
    GraphicsProvenance {
        executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
        installed_unit_graphics_sha256: SUPPORTED_UNIT_GRAPHICS_SHA256,
        coherent_capture: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &[u8] = br#"<ROOT><UNITS>
      <UNIT name="HeavyTank-DEFAULT-AGE0"/>
      <UNIT name="SCOUT-DEFAULT-AGE0"/>
      <UNIT name="Cruiser-DEFAULT-AGE0"/>
      <RESTRICTION name="HEAVYTANK" node="4" minangle="-180" maxangle="180"/>
      <RESTRICTION name="Cruiser" node="4" minangle="-135" maxangle="135"/>
      <RESTRICTION name="cruiser" node="5" minangle="50" maxangle="-50"/>
    </UNITS></ROOT>"#;

    fn catalog() -> UnitGraphicsCatalog {
        UnitGraphicsCatalog::from_fixture(XML).unwrap()
    }

    fn one_guy() -> UnitGuys {
        UnitGuys {
            guys: vec![Some(GuyData {
                guy_num: 0,
                ..GuyData::default()
            })],
            size: 1,
            increment: 0,
            flags: 0,
            guy_mark: 1,
        }
    }

    #[test]
    fn catalog_matches_retail_case_insensitively_and_preserves_order() {
        let catalog = catalog();
        catalog.validate_unit_graph("heavytank").unwrap();
        catalog.validate_unit_graph("Scout").unwrap();
        assert!(catalog.has_pivot_restrictions("heavytank").unwrap());
        assert_eq!(
            catalog.pivot_restrictions("CRUISER").unwrap(),
            &[
                PivotRestriction {
                    node: 4,
                    min_degrees: -135,
                    max_degrees: 135
                },
                PivotRestriction {
                    node: 5,
                    min_degrees: 50,
                    max_degrees: -50
                },
            ]
        );
    }

    #[test]
    #[ignore = "requires the user's supported installed unit_graphics.xml"]
    fn supported_installed_catalog_matches_measured_shape() {
        let path = std::env::var("RON_UNIT_GRAPHICS_XML")
            .expect("set RON_UNIT_GRAPHICS_XML to the installed supported XML");
        let xml = std::fs::read(path).unwrap();
        let catalog = UnitGraphicsCatalog::from_installed_xml(&xml, supported_provenance())
            .expect("supported installed XML must parse without structural drift");
        assert_eq!(catalog.unit_graphs.len(), 355);
        assert_eq!(catalog.restrictions.len(), 66);
        assert_eq!(
            catalog.restrictions.values().map(Vec::len).sum::<usize>(),
            SUPPORTED_UNIT_GRAPHICS_RESTRICTION_ROWS
        );
        catalog.validate_unit_graph("BATTLESHIP").unwrap();
        assert!(catalog.has_pivot_restrictions("SuperBattleship").unwrap());
    }

    #[test]
    fn public_catalog_rejects_declared_supported_identity_with_wrong_bytes() {
        assert_eq!(
            UnitGraphicsCatalog::from_installed_xml(XML, supported_provenance()),
            Err(GraphicsResourceError::WrongInstalledUnitGraphicsLength {
                expected: SUPPORTED_UNIT_GRAPHICS_LEN,
                actual: XML.len(),
            })
        );
        let mut bad = supported_provenance();
        bad.coherent_capture = false;
        assert_eq!(
            UnitGraphicsCatalog::from_installed_xml(XML, bad),
            Err(GraphicsResourceError::IncoherentCapture)
        );
    }

    #[test]
    fn catalog_rejects_nonsequential_pivot_nodes() {
        let xml = br#"<ROOT><UNIT name="Tank-X"/><RESTRICTION name="Tank" node="5" minangle="-1" maxangle="1"/></ROOT>"#;
        assert!(matches!(
            UnitGraphicsCatalog::from_fixture(xml),
            Err(GraphicsResourceError::NonSequentialRestrictionNode {
                expected: 4,
                actual: 5,
                ..
            })
        ));
    }

    struct Extractor {
        provenance: GraphicsProvenance,
        result: Result<ExtractedUnitGraphics, GraphicsResourceError>,
    }

    impl GuyGraphicsExtractor for Extractor {
        fn provenance(&self) -> GraphicsProvenance {
            self.provenance
        }

        fn extract_unit_graphics(
            &mut self,
            _graph_name: &str,
            _guy_numbers: &[Option<i8>],
        ) -> Result<ExtractedUnitGraphics, GraphicsResourceError> {
            self.result.clone()
        }
    }

    fn extracted(graph: &str, profile: ExtractedGuyGraphics) -> Extractor {
        Extractor {
            provenance: supported_provenance(),
            result: Ok(ExtractedUnitGraphics {
                graph_name: graph.to_owned(),
                slots: vec![Some(profile)],
            }),
        }
    }

    #[test]
    fn materializer_stamps_exact_graphics_fields_and_turret_capability() {
        let catalog = catalog();
        let mut guys = one_guy();
        guys.guys[0].as_mut().unwrap().guy_flags = 0x0040;
        let profile = ExtractedGuyGraphics {
            guy_num: 0,
            gpiece: 77,
            pivot_graph_name: Some("HeavyTank".into()),
            track_dx: -12,
            track_dy: 4,
            turret_angles: [11, 0, 0, 0],
            des_turret_angles: [12, 0, 0, 0],
            node_flags: 1,
            des_node_flags: 1,
        };
        let stats = materialize_unit_graphics(
            &catalog,
            "HeavyTank",
            &mut guys,
            &mut extracted("HEAVYTANK", profile),
        )
        .unwrap();
        let guy = guys.guys[0].as_ref().unwrap();
        assert_eq!(
            stats,
            GraphicsMaterializationStats {
                live_guys: 1,
                turret_guys: 1
            }
        );
        assert_eq!(guy.gpiece, 77);
        assert_eq!((guy.track_dx, guy.track_dy), (-12, 4));
        assert_eq!(guy.turret_angles, [11, 0, 0, 0]);
        assert_eq!(guy.guy_flags, 0x0040 | GUY_FLAG_TURRETS);
    }

    #[test]
    fn materializer_is_transactional_on_slot_mismatch() {
        let catalog = catalog();
        let mut guys = one_guy();
        let before = guys.clone();
        let mut extractor = Extractor {
            provenance: supported_provenance(),
            result: Ok(ExtractedUnitGraphics {
                graph_name: "HeavyTank".into(),
                slots: vec![None],
            }),
        };
        assert_eq!(
            materialize_unit_graphics(&catalog, "HeavyTank", &mut guys, &mut extractor),
            Err(GraphicsResourceError::WrongSlotPresence { slot: 0 })
        );
        assert_eq!(guys, before);
    }

    #[test]
    fn non_turret_graph_rejects_invented_pivot_state() {
        let catalog = catalog();
        let mut guys = one_guy();
        let before = guys.clone();
        let profile = ExtractedGuyGraphics {
            guy_num: 0,
            gpiece: 5,
            pivot_graph_name: None,
            track_dx: 0,
            track_dy: 0,
            turret_angles: [1, 0, 0, 0],
            des_turret_angles: [0; 4],
            node_flags: 0,
            des_node_flags: 0,
        };
        assert_eq!(
            materialize_unit_graphics(
                &catalog,
                "Scout",
                &mut guys,
                &mut extracted("scout", profile),
            ),
            Err(GraphicsResourceError::NonTurretPivotState { slot: 0 })
        );
        assert_eq!(guys, before);
    }

    #[derive(Default)]
    struct Offsets {
        values: Vec<PivotOffset>,
        fail: bool,
        seen: Vec<PivotOffsetRequest>,
    }

    impl PivotOffsetProvider for Offsets {
        fn pivot_offset(&mut self, request: PivotOffsetRequest) -> Result<PivotOffset, String> {
            self.seen.push(request);
            if self.fail {
                return Err("hierarchy missing".into());
            }
            Ok(self
                .values
                .get(self.seen.len() - 1)
                .copied()
                .unwrap_or(PivotOffset { x: 0.0, y: 0.0 }))
        }
    }

    fn turret_guy() -> GuyData {
        GuyData {
            angle: 0,
            gpiece: 77,
            guy_flags: GUY_FLAG_TURRETS,
            ..GuyData::default()
        }
    }

    #[test]
    fn resolver_updates_desired_angle_and_strict_settle_bits() {
        let catalog = catalog();
        let mut guy = turret_guy();
        // Target due north gives desired_relative 0, inside [-180,180] and within 45°.
        let result = resolve_turret_aim(
            &catalog,
            Some("HeavyTank"),
            &mut guy,
            100,
            100,
            100,
            0,
            &mut Offsets::default(),
        )
        .unwrap();
        assert!(result.aligned);
        assert_eq!(guy.des_turret_angles[0], 0);
        assert_eq!(guy.des_node_flags, 1);
        assert_eq!(guy.node_flags, 1);

        // Exactly one slew step is not settled: comparison at 0x005D8F5D is strict.
        // Use the positive-delta arm. The negative arm uses bitwise complement rather
        // than two's-complement negation, so its exact-step boundary folds to STEP-1.
        guy.turret_angles[0] = -(TURRET_STEP as i32);
        resolve_turret_aim(
            &catalog,
            Some("HeavyTank"),
            &mut guy,
            100,
            100,
            100,
            0,
            &mut Offsets::default(),
        )
        .unwrap();
        assert_eq!(guy.node_flags, 0);
    }

    #[test]
    fn resolver_applies_circular_restrictions_and_45_degree_return_gate() {
        let catalog = catalog();
        let mut guy = turret_guy();
        guy.turret_angles = [0; 4];
        let result = resolve_turret_aim(
            &catalog,
            Some("Cruiser"),
            &mut guy,
            0,
            0,
            0,
            100,
            &mut Offsets::default(),
        )
        .unwrap();
        // South is 180°: in node 4's [-135,135] it is outside, while node 5's
        // [50,-50] wrap interval admits it. The global return still fails the ±45° gate.
        assert!(!result.aligned);
        assert_eq!(guy.des_node_flags, 0b10);
        assert_eq!(guy.des_turret_angles[1], i32::MIN);
    }

    #[test]
    fn hierarchy_failure_is_fail_closed_and_non_mutating() {
        let catalog = catalog();
        let mut guy = turret_guy();
        guy.des_turret_angles = [1, 2, 3, 4];
        guy.node_flags = 3;
        let before = guy;
        let error = resolve_turret_aim(
            &catalog,
            Some("HeavyTank"),
            &mut guy,
            0,
            0,
            0,
            -1,
            &mut Offsets {
                fail: true,
                ..Offsets::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            error,
            TurretAimError::ProviderFailure { node: 4, .. }
        ));
        assert_eq!(guy, before);
    }

    #[test]
    fn provider_receives_exact_body_angle_and_current_pivots() {
        let catalog = catalog();
        let mut guy = turret_guy();
        guy.angle = 0x4000_0000;
        guy.turret_angles = [7, 8, 9, 10];
        let mut provider = Offsets::default();
        resolve_turret_aim(
            &catalog,
            Some("HeavyTank"),
            &mut guy,
            0,
            0,
            1,
            0,
            &mut provider,
        )
        .unwrap();
        assert_eq!(provider.seen.len(), 1);
        assert_eq!(provider.seen[0].gpiece, 77);
        assert_eq!(provider.seen[0].node, 4);
        assert_eq!(provider.seen[0].body_angle_degrees, 270.0);
        assert_eq!(provider.seen[0].current_turret_angles, [7, 8, 9, 10]);
    }
}
