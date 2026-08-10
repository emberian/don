//! Shipped map-style identity and static placement inputs.
//!
//! A replay stores only the ordinal in `GameInfo::map_style`. Retail resolves
//! that ordinal through the ordered `mapstyles` category in `rules.xml`, then
//! loads `mapstyles/default.xml` and the selected style XML. Those XML files
//! are copyrighted install data and therefore remain under gitignored
//! `ron-data/`; this module consumes a lawful local extraction without copying
//! it into the crate.
//!
//! The parser is deliberately narrow and fail-closed. It accepts the exact
//! double-quoted attribute form used by the shipped files, preserves attribute
//! values (including `SCALE`, `AREA`, and spacing tokens), and rejects malformed
//! comments/tags/attributes rather than silently inventing placement values.

use crate::checksum::adler32;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

pub const SHIPPED_RULES_SHA256: &str =
    "2cad6156f257c2faf79c3fa2de293a249f61ae245160b92fb5a76d0dbf3a9988";
pub const SHIPPED_EXE_SHA256: &str =
    "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079";

/// `Map::make` seeds the main RNG, loads map data, selects orientation, calls
/// the style virtual, constructs/fixes regions, places terrain groups, crosses
/// the no-RNG resource caller gap, then enters `Map::place_resources`.
pub const MAP_MAKE_VA: u32 = 0x0068_bc90;
pub const MAP_MAKE_ORIENTATION_RNG_VA: u32 = 0x0068_bd6a;
pub const MAP_PLACE_RESOURCES_VA: u32 = 0x0068_f4f0;
pub const MAP_PLACE_RESOURCES_DIRECT_RNG_SITES: [u32; 1] = [0x0068_fd64];

/// Direct calls to `Random::get` in `MapMediterranean::make_continents`.
/// Calls inside `make_region`, `grow_region`, fairness, terrain groups and
/// resource placement remain separate dynamic boundaries; a static call-site
/// list is not misreported as a fixed draw count.
pub const MEDITERRANEAN_DIRECT_RNG_SITES: [u32; 6] = [
    0x0069_af23,
    0x0069_af3a,
    0x0069_afd9,
    0x0069_b00a,
    0x0069_b476,
    0x0069_b4a7,
];
pub const OLD_WORLD_DIRECT_RNG_SITES: [u32; 4] =
    [0x0069_c14f, 0x0069_c167, 0x0069_c17d, 0x0069_c1ae];
pub const HIMALAYAS_DIRECT_RNG_SITES: [u32; 4] =
    [0x0069_bcff, 0x0069_bd17, 0x0069_bd2d, 0x0069_bd5e];
pub const GREAT_LAKES_DIRECT_RNG_SITES: [u32; 6] = [
    0x0069_9f88,
    0x0069_9fa0,
    0x0069_a002,
    0x0069_a175,
    0x0069_a1a1,
    0x0069_a44a,
];
pub const EAST_INDIES_DIRECT_RNG_SITES: [u32; 6] = [
    0x0069_76dd,
    0x0069_7723,
    0x0069_7b72,
    0x0069_7c3f,
    0x0069_7c69,
    0x0069_7d6a,
];
pub const EAST_MEETS_WEST_DIRECT_RNG_SITES: [u32; 3] = [0x0069_689f, 0x0069_68b7, 0x0069_6f82];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapStyleIdentity {
    pub ordinal: u8,
    pub key: &'static str,
    /// Concrete selectors 6..=22 map to an install XML. Selectors 0..=5 are
    /// random/secret meta-selectors resolved by retail before construction.
    pub filename: Option<&'static str>,
    pub map_class: Option<&'static str>,
    pub make_continents_va: Option<u32>,
}

macro_rules! style {
    ($ordinal:expr, $key:expr) => {
        MapStyleIdentity {
            ordinal: $ordinal,
            key: $key,
            filename: None,
            map_class: None,
            make_continents_va: None,
        }
    };
    ($ordinal:expr, $key:expr, $filename:expr, $class:expr, $va:expr) => {
        MapStyleIdentity {
            ordinal: $ordinal,
            key: $key,
            filename: Some($filename),
            map_class: Some($class),
            make_continents_va: Some($va),
        }
    };
}

/// Ordered `<CATEGORIES id="mapstyles">` list from shipped `rules.xml`.
/// Concrete entries are cross-pinned to `Map::create` (`0x006a0950`).
pub const SHIPPED_MAP_STYLE_CATALOG: [MapStyleIdentity; 23] = [
    style!(0, "Random"),
    style!(1, "Random Land Map"),
    style!(2, "Random Sea Map"),
    style!(3, "Secret Random"),
    style!(4, "Secret Random Land"),
    style!(5, "Secret Random Sea"),
    style!(6, "Old World", "oldworld.xml", "MapOldWorld", 0x0069_c0e0),
    style!(
        7,
        "Great Sahara",
        "sahara.xml",
        "MapGreatSahara",
        0x0069_bf70
    ),
    style!(
        8,
        "Amazon Rainforest",
        "amazon.xml",
        "MapAmazonBasin",
        0x0069_be00
    ),
    style!(9, "Himalayas", "himalayas.xml", "MapHimalayas", 0x0069_bc90),
    style!(
        10,
        "Southwest Mesa",
        "southwestmesa.xml",
        "MapSouthwestMesa",
        0x0069_bb20
    ),
    style!(
        11,
        "African Watering Hole",
        "africa.xml",
        "MapAfricanWateringHole",
        0x0069_b650
    ),
    style!(
        12,
        "Mediterranean",
        "mediterranean.xml",
        "MapMediterranean",
        0x0069_add0
    ),
    style!(
        13,
        "Australian Outback",
        "outback.xml",
        "MapAustralianOutback",
        0x0069_a650
    ),
    style!(
        14,
        "Great Lakes",
        "greatlakes.xml",
        "MapGreatLakes",
        0x0069_9e40
    ),
    style!(
        15,
        "Warring States",
        "warringstates.xml",
        "MapWarringStates",
        0x0069_8b60
    ),
    style!(16, "New World", "newworld.xml", "MapNewWorld", 0x0069_84d0),
    style!(
        17,
        "Colonial Powers",
        "colonialpowers.xml",
        "MapColonialPowers",
        0x0069_7dd0
    ),
    style!(
        18,
        "East Indies",
        "eastindies.xml",
        "MapEastIndies",
        0x0069_7540
    ),
    style!(
        19,
        "East Meets West",
        "eastwest.xml",
        "MapEastMeetsWest",
        0x0069_6640
    ),
    style!(
        20,
        "Atlantic Sea Power",
        "seapower.xml",
        "MapSeaPower",
        0x0069_6080
    ),
    style!(
        21,
        "Nile Delta",
        "niledelta.xml",
        "MapNileDelta",
        0x0069_5bf0
    ),
    style!(
        22,
        "British Isles",
        "britishisles.xml",
        "MapBritishIsles",
        0x0069_5130
    ),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapGenerationCheckpoint {
    pub call_va: u32,
    pub source_token: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapGenerationStage {
    pub name: &'static str,
    /// Function entry or `Map::make` call site which pins this stage in order.
    pub evidence_va: Option<u32>,
    pub rng: &'static str,
    /// The checksum-log deadline immediately after this stage, when one is
    /// proven. Tokens are caller instrumentation, not replay-source coverage.
    pub checkpoint: Option<MapGenerationCheckpoint>,
}

pub const MAP_POST_PLACE_ALL_CHECKPOINT_CALL_VA: u32 = 0x0068_c039;
pub const MAP_POST_PLACE_ALL_SOURCE_TOKEN: u32 = 0x1eb3;
pub const MAP_POST_CHECK_PLAYER_FOREST_CHECKPOINT_CALL_VA: u32 = 0x0068_c076;
pub const MAP_POST_CHECK_PLAYER_FOREST_SOURCE_TOKEN: u32 = 0x1eb5;
pub const MAP_POST_NUBIFY_FOREST_CHECKPOINT_CALL_VA: u32 = 0x0068_c0b4;
pub const MAP_POST_NUBIFY_FOREST_SOURCE_TOKEN: u32 = 0x1eb9;
pub const MAP_POST_TERRAIN_TRANSITIONS_CHECKPOINT_CALL_VA: u32 = 0x0068_c12a;
pub const MAP_POST_TERRAIN_TRANSITIONS_SOURCE_TOKEN: u32 = 0x1ebe;
pub const MAP_RESOURCE_CALLER_GAP_RESUME_VA: u32 = 0x0068_c12f;

/// Proven common ordering in `Map::make`. `rng` distinguishes a known direct
/// call from stages whose callees consume a branch-dependent number of draws.
/// The four post-placement checkpoints pin the exact receipt chain without
/// pretending their source tokens are bytes serialized by a replay.
pub const MAP_MAKE_SCHEDULE: [MapGenerationStage; 15] = [
    MapGenerationStage {
        name: "load_map_data",
        evidence_va: None,
        rng: "XML only",
        checkpoint: None,
    },
    MapGenerationStage {
        name: "select_orientation",
        evidence_va: Some(MAP_MAKE_ORIENTATION_RNG_VA),
        rng: "one direct Random::get when orientation < 0",
        checkpoint: None,
    },
    MapGenerationStage {
        name: "make_continents",
        evidence_va: None,
        rng: "style virtual; branch-dependent",
        checkpoint: None,
    },
    MapGenerationStage {
        name: "make_regions",
        evidence_va: Some(0x0068_0060),
        rng: "none; clear_all/find_all runs before and after coastlines",
        checkpoint: None,
    },
    MapGenerationStage {
        name: "fix_diag_land",
        evidence_va: Some(0x0069_c250),
        rng: "none",
        checkpoint: None,
    },
    MapGenerationStage {
        name: "make_coastlines",
        evidence_va: Some(0x0069_47a0),
        rng: "none",
        checkpoint: None,
    },
    MapGenerationStage {
        name: "fill_fertile",
        evidence_va: Some(0x006a_6f90),
        rng: "none; requires unreconstructed Fractal::frac and partitions",
        checkpoint: None,
    },
    MapGenerationStage {
        name: "terrain_groups_place_all",
        evidence_va: Some(0x0068_c010),
        rng: "branch-dependent; skipped when caller semaphore bit 0x02 is set",
        checkpoint: Some(MapGenerationCheckpoint {
            call_va: MAP_POST_PLACE_ALL_CHECKPOINT_CALL_VA,
            source_token: MAP_POST_PLACE_ALL_SOURCE_TOKEN,
        }),
    },
    MapGenerationStage {
        name: "check_player_forest",
        evidence_va: Some(0x0068_c04d),
        rng: "none; shares the place_all caller gate",
        checkpoint: Some(MapGenerationCheckpoint {
            call_va: MAP_POST_CHECK_PLAYER_FOREST_CHECKPOINT_CALL_VA,
            source_token: MAP_POST_CHECK_PLAYER_FOREST_SOURCE_TOKEN,
        }),
    },
    MapGenerationStage {
        name: "nubify_forest",
        evidence_va: Some(0x0068_c08b),
        rng: "branch-dependent; exact draws are receipted; shares the caller gate",
        checkpoint: Some(MapGenerationCheckpoint {
            call_va: MAP_POST_NUBIFY_FOREST_CHECKPOINT_CALL_VA,
            source_token: MAP_POST_NUBIFY_FOREST_SOURCE_TOKEN,
        }),
    },
    MapGenerationStage {
        name: "post_nubify_transitions",
        evidence_va: Some(0x0068_c101),
        rng: "three signed base gates, then exact transition draws",
        checkpoint: Some(MapGenerationCheckpoint {
            call_va: MAP_POST_TERRAIN_TRANSITIONS_CHECKPOINT_CALL_VA,
            source_token: MAP_POST_TERRAIN_TRANSITIONS_SOURCE_TOKEN,
        }),
    },
    MapGenerationStage {
        name: "resource_caller_gap",
        evidence_va: Some(MAP_RESOURCE_CALLER_GAP_RESUME_VA),
        rng: "none; exact internal checkpoints and lazy placement gate are receipted",
        checkpoint: None,
    },
    MapGenerationStage {
        name: "place_resources",
        evidence_va: Some(MAP_PLACE_RESOURCES_VA),
        rng: "pool prefix none; body open at 0x0068f597 before known direct/callee draws",
        checkpoint: None,
    },
    MapGenerationStage {
        name: "adjust_blocking",
        evidence_va: None,
        rng: "unresolved",
        checkpoint: None,
    },
    MapGenerationStage {
        name: "compute_values",
        evidence_va: None,
        rng: "unresolved",
        checkpoint: None,
    },
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaticXmlEntry {
    pub tag: String,
    pub attributes: BTreeMap<String, String>,
}

impl StaticXmlEntry {
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(String::as_str)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaticFileEvidence {
    pub path: PathBuf,
    pub bytes: usize,
    pub adler32: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapStyleStaticData {
    pub identity: MapStyleIdentity,
    pub catalog_source: StaticFileEvidence,
    pub default_source: StaticFileEvidence,
    pub selected_source: StaticFileEvidence,
    pub default_map_entries: Vec<StaticXmlEntry>,
    pub selected_map_entries: Vec<StaticXmlEntry>,
    pub selected_map_section_present: bool,
    pub default_terrain_groups: Vec<StaticXmlEntry>,
    pub selected_terrain_groups: Vec<StaticXmlEntry>,
    pub selected_terrain_groups_section_present: bool,
    pub default_goodies: Vec<StaticXmlEntry>,
    pub selected_goodies: Vec<StaticXmlEntry>,
    pub selected_goodies_section_present: bool,
    pub make_continents_direct_rng_sites: Vec<u32>,
}

impl MapStyleStaticData {
    /// Load and independently validate the replay selector against local
    /// shipped content. The catalog order must exactly match all 23 known keys;
    /// matching XML filenames alone is insufficient evidence for an ordinal.
    pub fn load_from_ron_data(root: &Path, ordinal: u8) -> Result<Self, MapStyleLoadError> {
        let identity = SHIPPED_MAP_STYLE_CATALOG
            .get(ordinal as usize)
            .copied()
            .ok_or(MapStyleLoadError::UnknownOrdinal(ordinal))?;
        let filename = identity
            .filename
            .ok_or(MapStyleLoadError::RandomSelectorRequiresResolution(ordinal))?;

        let rules_path = root.join("rules.xml");
        let default_path = root.join("mapstyles/default.xml");
        let selected_path = root.join("mapstyles").join(filename);
        let rules = read(&rules_path)?;
        let catalog = parse_map_style_catalog(&rules)?;
        let expected: Vec<&str> = SHIPPED_MAP_STYLE_CATALOG.iter().map(|s| s.key).collect();
        if catalog != expected {
            let mismatch = catalog
                .iter()
                .zip(expected.iter())
                .position(|(got, want)| got != want)
                .unwrap_or(catalog.len().min(expected.len()));
            return Err(MapStyleLoadError::CatalogMismatch {
                index: mismatch,
                expected: expected.get(mismatch).copied().map(str::to_owned),
                actual: catalog.get(mismatch).cloned(),
                expected_len: expected.len(),
                actual_len: catalog.len(),
            });
        }

        // Only an admitted catalog is allowed to select a filename. This keeps
        // the first reported boundary stable when both a catalog and an XML
        // extraction are incomplete.
        let default = read(&default_path)?;
        let selected = read(&selected_path)?;

        let default_parsed = ParsedMapStyle::parse(&default)?;
        let selected_parsed = ParsedMapStyle::parse(&selected)?;
        let default_map_entries = default_parsed
            .map
            .ok_or(MapStyleLoadError::MissingSection("MAP"))?;
        let default_terrain_groups = default_parsed
            .terrain_groups
            .ok_or(MapStyleLoadError::MissingSection("TERRAIN_GROUPS"))?;
        let default_goodies = default_parsed
            .goodies
            .ok_or(MapStyleLoadError::MissingSection("GOODIES"))?;
        let selected_map_section_present = selected_parsed.map.is_some();
        let selected_terrain_groups_section_present = selected_parsed.terrain_groups.is_some();
        let selected_goodies_section_present = selected_parsed.goodies.is_some();
        Ok(Self {
            identity,
            catalog_source: evidence(rules_path, &rules),
            default_source: evidence(default_path, &default),
            selected_source: evidence(selected_path, &selected),
            default_map_entries,
            selected_map_entries: selected_parsed.map.unwrap_or_default(),
            selected_map_section_present,
            default_terrain_groups,
            selected_terrain_groups: selected_parsed.terrain_groups.unwrap_or_default(),
            selected_terrain_groups_section_present,
            default_goodies,
            selected_goodies: selected_parsed.goodies.unwrap_or_default(),
            selected_goodies_section_present,
            make_continents_direct_rng_sites: direct_rng_sites(identity).to_vec(),
        })
    }

    /// All direct RNG sites currently pinned for the top-level placement path.
    /// This is explicitly not the dynamic draw count: called helpers and retry
    /// loops consume further draws.
    pub fn known_direct_rng_sites(&self) -> Vec<u32> {
        let mut sites = Vec::with_capacity(2 + self.make_continents_direct_rng_sites.len());
        sites.push(MAP_MAKE_ORIENTATION_RNG_VA);
        sites.extend_from_slice(&self.make_continents_direct_rng_sites);
        sites.extend_from_slice(&MAP_PLACE_RESOURCES_DIRECT_RNG_SITES);
        sites
    }

    /// Retail opens both the selected XML and `default.xml`; an absent selected
    /// section falls back to the default section. A present-but-empty section
    /// remains empty, so section presence is kept separately from row count.
    pub fn effective_map_entries(&self) -> &[StaticXmlEntry] {
        if self.selected_map_section_present {
            &self.selected_map_entries
        } else {
            &self.default_map_entries
        }
    }

    pub fn effective_terrain_groups(&self) -> &[StaticXmlEntry] {
        if self.selected_terrain_groups_section_present {
            &self.selected_terrain_groups
        } else {
            &self.default_terrain_groups
        }
    }

    pub fn effective_goodies(&self) -> &[StaticXmlEntry] {
        if self.selected_goodies_section_present {
            &self.selected_goodies
        } else {
            &self.default_goodies
        }
    }
}

fn direct_rng_sites(identity: MapStyleIdentity) -> &'static [u32] {
    match identity.ordinal {
        6 => &OLD_WORLD_DIRECT_RNG_SITES,
        9 => &HIMALAYAS_DIRECT_RNG_SITES,
        12 => &MEDITERRANEAN_DIRECT_RNG_SITES,
        14 => &GREAT_LAKES_DIRECT_RNG_SITES,
        18 => &EAST_INDIES_DIRECT_RNG_SITES,
        19 => &EAST_MEETS_WEST_DIRECT_RNG_SITES,
        _ => &[],
    }
}

fn evidence(path: PathBuf, bytes: &[u8]) -> StaticFileEvidence {
    StaticFileEvidence {
        path,
        bytes: bytes.len(),
        adler32: adler32(1, bytes),
    }
}

fn read(path: &Path) -> Result<Vec<u8>, MapStyleLoadError> {
    fs::read(path).map_err(|e| MapStyleLoadError::Read {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapStyleLoadError {
    Read {
        path: PathBuf,
        message: String,
    },
    Utf8 {
        context: &'static str,
    },
    Malformed {
        context: &'static str,
        offset: usize,
        message: String,
    },
    MissingSection(&'static str),
    DuplicateSection(&'static str),
    UnknownOrdinal(u8),
    RandomSelectorRequiresResolution(u8),
    CatalogMismatch {
        index: usize,
        expected: Option<String>,
        actual: Option<String>,
        expected_len: usize,
        actual_len: usize,
    },
}

impl fmt::Display for MapStyleLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, message } => write!(f, "{}: {message}", path.display()),
            Self::Utf8 { context } => write!(f, "{context}: input is not UTF-8"),
            Self::Malformed { context, offset, message } => {
                write!(f, "{context} at {offset:#x}: {message}")
            }
            Self::MissingSection(name) => write!(f, "missing <{name}> section"),
            Self::DuplicateSection(name) => write!(f, "duplicate <{name}> section"),
            Self::UnknownOrdinal(n) => write!(f, "map-style ordinal {n} is outside 0..23"),
            Self::RandomSelectorRequiresResolution(n) => {
                write!(f, "map-style ordinal {n} is a random meta-selector")
            }
            Self::CatalogMismatch { index, expected, actual, expected_len, actual_len } => write!(
                f,
                "mapstyles catalog mismatch at {index}: expected {expected:?} ({expected_len} entries), got {actual:?} ({actual_len} entries)"
            ),
        }
    }
}

impl std::error::Error for MapStyleLoadError {}

struct ParsedMapStyle {
    map: Option<Vec<StaticXmlEntry>>,
    terrain_groups: Option<Vec<StaticXmlEntry>>,
    goodies: Option<Vec<StaticXmlEntry>>,
}

impl ParsedMapStyle {
    fn parse(bytes: &[u8]) -> Result<Self, MapStyleLoadError> {
        let text = std::str::from_utf8(bytes).map_err(|_| MapStyleLoadError::Utf8 {
            context: "map-style XML",
        })?;
        let text = strip_comments(text, "map-style XML")?;
        Ok(Self {
            map: parse_section_entries_optional(&text, "MAP")?,
            terrain_groups: parse_section_entries_optional(&text, "TERRAIN_GROUPS")?,
            goodies: parse_section_entries_optional(&text, "GOODIES")?,
        })
    }
}

fn strip_comments(text: &str, context: &'static str) -> Result<String, MapStyleLoadError> {
    let mut out = String::with_capacity(text.len());
    let mut at = 0usize;
    while let Some(rel) = text[at..].find("<!--") {
        let start = at + rel;
        out.push_str(&text[at..start]);
        let body = start + 4;
        let Some(end_rel) = text[body..].find("-->") else {
            return Err(MapStyleLoadError::Malformed {
                context,
                offset: start,
                message: "unclosed XML comment".into(),
            });
        };
        at = body + end_rel + 3;
    }
    out.push_str(&text[at..]);
    Ok(out)
}

fn parse_map_style_catalog(bytes: &[u8]) -> Result<Vec<String>, MapStyleLoadError> {
    let text = std::str::from_utf8(bytes).map_err(|_| MapStyleLoadError::Utf8 {
        context: "rules.xml",
    })?;
    let text = strip_comments(text, "rules.xml")?;
    let (start, body) = find_section_by_attribute(&text, "CATEGORIES", "id", "mapstyles")?;
    let entries = scan_entries(body, "rules.xml", start)?;
    let mut keys = Vec::new();
    for entry in entries {
        if entry.tag == "CATEGORY" {
            let Some(key) = entry.attributes.get("key") else {
                return Err(MapStyleLoadError::Malformed {
                    context: "rules.xml",
                    offset: start,
                    message: "mapstyles CATEGORY has no key".into(),
                });
            };
            keys.push(key.clone());
        }
    }
    Ok(keys)
}

fn parse_section_entries_optional(
    text: &str,
    name: &'static str,
) -> Result<Option<Vec<StaticXmlEntry>>, MapStyleLoadError> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let Some(start) = text.find(&open) else {
        return Ok(None);
    };
    if text[start + open.len()..].contains(&open) {
        return Err(MapStyleLoadError::DuplicateSection(name));
    }
    let body_start = start + open.len();
    let Some(end_rel) = text[body_start..].find(&close) else {
        return Err(MapStyleLoadError::Malformed {
            context: "map-style XML",
            offset: body_start,
            message: format!("unclosed <{name}> section"),
        });
    };
    let end = body_start + end_rel;
    if text[end + close.len()..].contains(&open) {
        return Err(MapStyleLoadError::DuplicateSection(name));
    }
    scan_entries(&text[body_start..end], "map-style XML", body_start).map(Some)
}

fn find_section_by_attribute<'a>(
    text: &'a str,
    tag: &'static str,
    attr: &str,
    value: &str,
) -> Result<(usize, &'a str), MapStyleLoadError> {
    let needle = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut search = 0usize;
    let mut found: Option<(usize, usize)> = None;
    while let Some(rel) = text[search..].find(&needle) {
        let start = search + rel;
        let Some(end_rel) = text[start..].find('>') else {
            return Err(MapStyleLoadError::Malformed {
                context: "rules.xml",
                offset: start,
                message: format!("unclosed <{tag}> tag"),
            });
        };
        let tag_end = start + end_rel + 1;
        let entry = parse_open_tag(&text[start + 1..tag_end - 1], "rules.xml", start)?;
        if entry.attribute(attr) == Some(value) {
            if found.is_some() {
                return Err(MapStyleLoadError::DuplicateSection(
                    "CATEGORIES[id=mapstyles]",
                ));
            }
            found = Some((start, tag_end));
        }
        search = tag_end;
    }
    let Some((start, body_start)) = found else {
        return Err(MapStyleLoadError::MissingSection(
            "CATEGORIES[id=mapstyles]",
        ));
    };
    let Some(end_rel) = text[body_start..].find(&close) else {
        return Err(MapStyleLoadError::Malformed {
            context: "rules.xml",
            offset: body_start,
            message: "unclosed mapstyles CATEGORIES section".into(),
        });
    };
    Ok((start, &text[body_start..body_start + end_rel]))
}

fn scan_entries(
    body: &str,
    context: &'static str,
    base: usize,
) -> Result<Vec<StaticXmlEntry>, MapStyleLoadError> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while let Some(rel) = body[at..].find('<') {
        let start = at + rel;
        let Some(end_rel) = body[start..].find('>') else {
            return Err(MapStyleLoadError::Malformed {
                context,
                offset: base + start,
                message: "unclosed tag".into(),
            });
        };
        let end = start + end_rel;
        let raw = body[start + 1..end].trim();
        at = end + 1;
        if raw.is_empty() || raw.starts_with('/') || raw.starts_with('?') || raw.starts_with('!') {
            continue;
        }
        let raw = raw.strip_suffix('/').unwrap_or(raw).trim_end();
        out.push(parse_open_tag(raw, context, base + start)?);
    }
    Ok(out)
}

fn parse_open_tag(
    raw: &str,
    context: &'static str,
    offset: usize,
) -> Result<StaticXmlEntry, MapStyleLoadError> {
    let bytes = raw.as_bytes();
    let mut at = 0usize;
    while at < bytes.len() && !bytes[at].is_ascii_whitespace() {
        at += 1;
    }
    if at == 0 {
        return Err(MapStyleLoadError::Malformed {
            context,
            offset,
            message: "empty tag name".into(),
        });
    }
    let tag = raw[..at].to_owned();
    let mut attributes = BTreeMap::new();
    let mut seen = BTreeSet::new();
    while at < bytes.len() {
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if at == bytes.len() {
            break;
        }
        let name_start = at;
        while at < bytes.len() && !bytes[at].is_ascii_whitespace() && bytes[at] != b'=' {
            at += 1;
        }
        let name = &raw[name_start..at];
        if name.is_empty() {
            return Err(MapStyleLoadError::Malformed {
                context,
                offset: offset + at,
                message: "empty attribute name".into(),
            });
        }
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if bytes.get(at) != Some(&b'=') {
            return Err(MapStyleLoadError::Malformed {
                context,
                offset: offset + at,
                message: format!("attribute {name} has no '='"),
            });
        }
        at += 1;
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if bytes.get(at) != Some(&b'\"') {
            return Err(MapStyleLoadError::Malformed {
                context,
                offset: offset + at,
                message: format!("attribute {name} is not double-quoted"),
            });
        }
        at += 1;
        let value_start = at;
        while at < bytes.len() && bytes[at] != b'\"' {
            at += 1;
        }
        if at == bytes.len() {
            return Err(MapStyleLoadError::Malformed {
                context,
                offset: offset + value_start,
                message: format!("attribute {name} has no closing quote"),
            });
        }
        let value = raw[value_start..at].to_owned();
        at += 1;
        if !seen.insert(name.to_owned()) {
            return Err(MapStyleLoadError::Malformed {
                context,
                offset: offset + name_start,
                message: format!("duplicate attribute {name}"),
            });
        }
        attributes.insert(name.to_owned(), value);
    }
    Ok(StaticXmlEntry { tag, attributes })
}

/// Locate the gitignored `ron-data` ancestor for a replay path. This does not
/// guess a global install location: only the content root which owns the replay
/// is admitted.
pub fn ron_data_root_for_replay(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|p| p.file_name().is_some_and(|n| n == "ron-data") && p.join("rules.xml").is_file())
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_identity_order_is_self_indexing() {
        assert_eq!(SHIPPED_MAP_STYLE_CATALOG.len(), 23);
        for (ordinal, style) in SHIPPED_MAP_STYLE_CATALOG.iter().enumerate() {
            assert_eq!(style.ordinal as usize, ordinal);
        }
        let med = SHIPPED_MAP_STYLE_CATALOG[12];
        assert_eq!(med.key, "Mediterranean");
        assert_eq!(med.filename, Some("mediterranean.xml"));
        assert_eq!(med.make_continents_va, Some(0x0069_add0));
    }

    #[test]
    fn attributes_preserve_placement_expressions_and_reject_duplicates() {
        let e = parse_open_tag(
            r#"BONUS numrare="4 SCALE" spacing="12  grspace""#,
            "test",
            0,
        )
        .unwrap();
        assert_eq!(e.attribute("numrare"), Some("4 SCALE"));
        assert_eq!(e.attribute("spacing"), Some("12  grspace"));
        assert!(parse_open_tag(r#"BONUS numrare="1" numrare="2""#, "test", 0).is_err());
    }

    #[test]
    fn unclosed_comments_fail_closed() {
        assert!(strip_comments("<ROOT><!-- no end", "test").is_err());
    }
}
