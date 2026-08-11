//! Lawful prerequisite producer for the replay `place_all` adapter.
//!
//! Installed map-style XML is sufficient to reconstruct every deterministic
//! `TerrainGroup` scalar. The remaining inputs are runtime state and therefore
//! require a capture tied to the shipped executable and exact call anchors.

use crate::fractal_boundary::TERRAIN_GROUPS_PLACE_ALL_VA;
use crate::initial::{InitialItemBoundary, InitialItemReconstruction, InitialWorld};
use crate::map_style::{
    MapStyleStaticData, StaticFileEvidence, StaticXmlEntry, SHIPPED_EXE_SHA256,
};
use crate::place_all_boundary::{
    ReplayPlaceAllFacts, ReplayPlaceAllHostFacts, ReplayPlaceAllPlayerFacts, ReplayPlaceAllRuntime,
    ReplayPlaceAllTDataFacts,
};
use don_sim::systems::mountains::Mountains;
use don_sim::systems::terrain_doobers::DooberTilesetRules;
use don_sim::systems::terrain_groups::{PlacementReportingInputs, TerrainGroup};
use don_sim::systems::terrain_region_placement::RegionHelpingState;

pub const TERRAIN_GROUPS_INIT_TERRAIN_DATA_VA: u32 = 0x006a_6540;
pub const TERRAIN_GROUP_EXPRESSION_VA: u32 = 0x006a_0320;
pub const TERRAIN_GROUPS_REPORTING_VA: u32 = 0x006a_8f12;
pub const GAME_MAP_POINTER_VA: u32 = 0x00ca_a34c;
pub const MAP_TERRAIN_GROUPS_OFFSET: u32 = 0x140;
pub const TERRAIN_GROUPS_SUBTYPE_FREQS_OFFSET: u32 = 0x24;
pub const TERRAIN_GROUPS_CONSOLE_INFO_OFFSET: u32 = 0x78;
pub const MOUNTAINS_GLOBAL_VA: u32 = 0x00e8_5f60;
pub const WORLD_GLOBAL_VA: u32 = 0x00c0_97e8;
pub const WORLD_TDATA_OFFSET: u32 = 0x138;
pub const TILESETS_GLOBAL_VA: u32 = 0x00e8_85d0;
pub const TILESET_CURRENT_OFFSET: u32 = 0x20;
pub const TILESET_GROUP_DATA_OFFSET: u32 = 0x614;
pub const IS_HELPING_VA: u32 = 0x00ca_e708;
pub const NUM_PLAYERS_VA: u32 = 0x00ca_e70c;
pub const LOWEST_PLAYER_VA: u32 = 0x00cb_e440;
pub const PLAYER_SCORES_VA: u32 = 0x00cb_e480;

const STANDARD_MAP_EDGE: i32 = 70;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerrainGroupCatalogReceipt {
    pub groups: Vec<TerrainGroup>,
    pub raw_rows: Vec<StaticXmlEntry>,
    pub source: StaticFileEvidence,
    pub selected_section: bool,
    pub init_terrain_data_va: u32,
    pub expression_va: u32,
    pub world_xs: i32,
    pub world_ys: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerrainGroupCatalogError {
    InvalidWorldShape {
        xs: i32,
        ys: i32,
        size: i32,
    },
    UnexpectedTag {
        row: usize,
        tag: String,
    },
    MissingAttribute {
        row: usize,
        attribute: &'static str,
    },
    InvalidInteger {
        row: usize,
        attribute: &'static str,
        value: String,
    },
    UnsupportedExpressionSuffix {
        row: usize,
        attribute: &'static str,
        value: String,
        suffix: String,
    },
    UnsupportedGroupType {
        row: usize,
        value: String,
    },
    UnsupportedPattern {
        row: usize,
        value: String,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CapturedHelpingFacts {
    Disabled,
    Enabled(RegionHelpingState),
}

impl CapturedHelpingFacts {
    const fn into_sim(self) -> Option<RegionHelpingState> {
        match self {
            Self::Disabled => None,
            Self::Enabled(state) => Some(state),
        }
    }
}

/// One capture session spanning the entry and reporting anchors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceAllLiveCaptureEvidence {
    pub executable_sha256: String,
    pub entry_va: u32,
    pub reporting_va: u32,
    pub map_style: u8,
    pub tileset: String,
}

/// Runtime facts which cannot be reconstructed from `.rcx` or map-style XML.
///
/// `Option` means capture presence, not native optionality. In particular,
/// `Some(CapturedHelpingFacts::Disabled)` and `Some(empty host rows)` are
/// authoritative values, while `None` is unavailable evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceAllLiveFacts {
    pub evidence: PlaceAllLiveCaptureEvidence,
    pub subtype_freqs: Option<[Vec<i32>; 3]>,
    pub console_info: Option<i32>,
    pub mountains: Option<Mountains>,
    pub tdata: Option<ReplayPlaceAllTDataFacts>,
    pub doober_rules: Option<DooberTilesetRules>,
    pub progress: Option<i32>,
    pub helping: Option<CapturedHelpingFacts>,
    /// The table at the reporting anchor after group placement has updated it.
    pub reporting_scores: Option<[[i32; 5]; 8]>,
    pub host: Option<ReplayPlaceAllHostFacts>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PlaceAllFactKind {
    TerrainGroupCatalog,
    FertilityPlaneAndPartitions,
    TerrainSubtypeFrequencies,
    TerrainConsoleInfo,
    MountainRangeListsAndCursors,
    TDataPlane,
    DooberTilesetRules,
    ProgressArgument,
    PlayerCountAndPlacePlayers,
    HelpingGlobals,
    ReportingScores,
    HostGroupResolutions,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnavailablePlaceAllFact {
    pub kind: PlaceAllFactKind,
    pub required_source: &'static str,
    pub addresses: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedReplayPlaceAll {
    pub runtime: ReplayPlaceAllRuntime,
    pub facts: ReplayPlaceAllFacts,
    pub live_evidence: PlaceAllLiveCaptureEvidence,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceAllPrerequisiteResolution {
    pub catalog: TerrainGroupCatalogReceipt,
    pub available: Vec<PlaceAllFactKind>,
    pub unavailable: Vec<UnavailablePlaceAllFact>,
    pub ready: Option<PreparedReplayPlaceAll>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaceAllPrerequisiteError {
    Blocked {
        boundary: InitialItemBoundary,
    },
    MissingStaticStyle,
    MissingFertilityEvidence,
    StyleSelectorMismatch {
        replay: u8,
        installed: u8,
    },
    Catalog(TerrainGroupCatalogError),
    CaptureExecutableMismatch {
        expected: &'static str,
        actual: String,
    },
    CaptureAnchorMismatch {
        expected_entry: u32,
        actual_entry: u32,
        expected_reporting: u32,
        actual_reporting: u32,
    },
    CaptureMapStyleMismatch {
        replay: u8,
        captured: u8,
    },
    CaptureTilesetMismatch {
        selected: String,
        captured: String,
    },
    TDataShapeMismatch {
        world_tile_xs: i32,
        world_tile_ys: i32,
        world_tile_size: i32,
        captured_tile_xs: i32,
        captured_tile_ys: i32,
        captured_tile_size: i32,
        captured_cells: usize,
    },
    HelpingPlayerCountMismatch {
        replay: usize,
        captured: usize,
    },
}

/// Reconstruct the exact native TerrainGroup scalar rows admitted from the
/// catalog-validated effective map-style section.
pub fn resolve_terrain_group_catalog(
    style: &MapStyleStaticData,
    world: &don_sim::systems::map_terrain::World,
) -> Result<TerrainGroupCatalogReceipt, TerrainGroupCatalogError> {
    if world.xs < 0 || world.ys < 0 || world.xs.checked_mul(world.ys) != Some(world.size) {
        return Err(TerrainGroupCatalogError::InvalidWorldShape {
            xs: world.xs,
            ys: world.ys,
            size: world.size,
        });
    }
    let rows = style.effective_terrain_groups();
    let mut groups = Vec::with_capacity(rows.len());
    for (row_index, row) in rows.iter().enumerate() {
        groups.push(resolve_group(row, row_index, world)?);
    }
    Ok(TerrainGroupCatalogReceipt {
        groups,
        raw_rows: rows.to_vec(),
        source: if style.selected_terrain_groups_section_present {
            style.selected_source.clone()
        } else {
            style.default_source.clone()
        },
        selected_section: style.selected_terrain_groups_section_present,
        init_terrain_data_va: TERRAIN_GROUPS_INIT_TERRAIN_DATA_VA,
        expression_va: TERRAIN_GROUP_EXPRESSION_VA,
        world_xs: world.xs,
        world_ys: world.ys,
    })
}

/// Resolve all replay `place_all` prerequisites, retaining a typed unavailable
/// matrix until every live-only producer has supplied evidence.
pub fn resolve_place_all_prerequisites(
    plan: &InitialItemReconstruction,
    map: &InitialWorld,
    capture: Option<&PlaceAllLiveFacts>,
) -> Result<PlaceAllPrerequisiteResolution, PlaceAllPrerequisiteError> {
    match plan.boundary {
        InitialItemBoundary::MapTerrainGroupsPlaceAllUnavailable { next_va }
            if next_va == TERRAIN_GROUPS_PLACE_ALL_VA => {}
        boundary => return Err(PlaceAllPrerequisiteError::Blocked { boundary }),
    }
    let style = plan
        .style
        .as_ref()
        .ok_or(PlaceAllPrerequisiteError::MissingStaticStyle)?;
    if style.identity.ordinal != plan.inputs.map_style {
        return Err(PlaceAllPrerequisiteError::StyleSelectorMismatch {
            replay: plan.inputs.map_style,
            installed: style.identity.ordinal,
        });
    }
    let fertility = plan
        .fertility
        .as_ref()
        .ok_or(PlaceAllPrerequisiteError::MissingFertilityEvidence)?;
    let catalog = resolve_terrain_group_catalog(style, &map.world)
        .map_err(PlaceAllPrerequisiteError::Catalog)?;
    let mut available = vec![
        PlaceAllFactKind::TerrainGroupCatalog,
        PlaceAllFactKind::FertilityPlaneAndPartitions,
        PlaceAllFactKind::PlayerCountAndPlacePlayers,
    ];
    let mut unavailable = Vec::new();

    let Some(capture) = capture else {
        push_all_live_unavailable(&mut unavailable);
        return Ok(PlaceAllPrerequisiteResolution {
            catalog,
            available,
            unavailable,
            ready: None,
        });
    };
    validate_capture_evidence(plan, &fertility.tileset, &capture.evidence)?;

    record_presence(
        capture.subtype_freqs.is_some(),
        PlaceAllFactKind::TerrainSubtypeFrequencies,
        missing_subtype_freqs,
        &mut available,
        &mut unavailable,
    );
    record_presence(
        capture.console_info.is_some(),
        PlaceAllFactKind::TerrainConsoleInfo,
        missing_console_info,
        &mut available,
        &mut unavailable,
    );
    record_presence(
        capture.mountains.is_some(),
        PlaceAllFactKind::MountainRangeListsAndCursors,
        missing_mountains,
        &mut available,
        &mut unavailable,
    );
    record_presence(
        capture.tdata.is_some(),
        PlaceAllFactKind::TDataPlane,
        missing_tdata,
        &mut available,
        &mut unavailable,
    );
    record_presence(
        capture.doober_rules.is_some(),
        PlaceAllFactKind::DooberTilesetRules,
        missing_doober_rules,
        &mut available,
        &mut unavailable,
    );
    record_presence(
        capture.progress.is_some(),
        PlaceAllFactKind::ProgressArgument,
        missing_progress,
        &mut available,
        &mut unavailable,
    );
    record_presence(
        capture.helping.is_some(),
        PlaceAllFactKind::HelpingGlobals,
        missing_helping,
        &mut available,
        &mut unavailable,
    );
    record_presence(
        capture.reporting_scores.is_some(),
        PlaceAllFactKind::ReportingScores,
        missing_reporting_scores,
        &mut available,
        &mut unavailable,
    );
    record_presence(
        capture.host.is_some(),
        PlaceAllFactKind::HostGroupResolutions,
        missing_host,
        &mut available,
        &mut unavailable,
    );
    if !unavailable.is_empty() {
        return Ok(PlaceAllPrerequisiteResolution {
            catalog,
            available,
            unavailable,
            ready: None,
        });
    }

    let tdata = capture.tdata.clone().expect("presence checked above");
    if tdata.tile_xs != map.world.tile_xs
        || tdata.tile_ys != map.world.tile_ys
        || tdata.tile_size != map.world.tile_size
        || tdata.cells.len() != map.world.tdata.len()
    {
        return Err(PlaceAllPrerequisiteError::TDataShapeMismatch {
            world_tile_xs: map.world.tile_xs,
            world_tile_ys: map.world.tile_ys,
            world_tile_size: map.world.tile_size,
            captured_tile_xs: tdata.tile_xs,
            captured_tile_ys: tdata.tile_ys,
            captured_tile_size: tdata.tile_size,
            captured_cells: tdata.cells.len(),
        });
    }
    let helping = capture.helping.expect("presence checked above");
    if let CapturedHelpingFacts::Enabled(state) = helping {
        if state.num_players != usize::from(plan.inputs.players) {
            return Err(PlaceAllPrerequisiteError::HelpingPlayerCountMismatch {
                replay: usize::from(plan.inputs.players),
                captured: state.num_players,
            });
        }
    }

    let mut terrain_groups = fertility.terrain_groups_input();
    terrain_groups.groups = catalog.groups.clone();
    terrain_groups.subtype_freqs = capture
        .subtype_freqs
        .clone()
        .expect("presence checked above");
    terrain_groups.console_info = capture.console_info.expect("presence checked above");
    let runtime = ReplayPlaceAllRuntime {
        terrain_groups,
        mountains: capture.mountains.clone().expect("presence checked above"),
    };
    let facts = ReplayPlaceAllFacts {
        tdata,
        doober_rules: capture.doober_rules.expect("presence checked above"),
        players: ReplayPlaceAllPlayerFacts {
            progress: capture.progress.expect("presence checked above"),
            // Map::make pushes literal one immediately before the place_all call
            // at 0x0068c007--0x0068c010.
            place_players: 1,
            helping: helping.into_sim(),
            reporting: PlacementReportingInputs {
                num_players: i32::from(plan.inputs.players),
                player_scores: capture.reporting_scores.expect("presence checked above"),
            },
        },
        host: capture.host.clone().expect("presence checked above"),
    };
    Ok(PlaceAllPrerequisiteResolution {
        catalog,
        available,
        unavailable,
        ready: Some(PreparedReplayPlaceAll {
            runtime,
            facts,
            live_evidence: capture.evidence.clone(),
        }),
    })
}

fn resolve_group(
    row: &StaticXmlEntry,
    row_index: usize,
    world: &don_sim::systems::map_terrain::World,
) -> Result<TerrainGroup, TerrainGroupCatalogError> {
    if row.tag != "GROUPENTRY" {
        return Err(TerrainGroupCatalogError::UnexpectedTag {
            row: row_index,
            tag: row.tag.clone(),
        });
    }
    let group_type = match required(row, row_index, "type")? {
        "trees" => 4,
        "mountains" => 5,
        "rocks" => 6,
        "oil" => 7,
        "cliffs" => 8,
        value => {
            return Err(TerrainGroupCatalogError::UnsupportedGroupType {
                row: row_index,
                value: value.to_owned(),
            });
        }
    };
    let pattern = match required(row, row_index, "pattern")? {
        "player" => 0,
        "continent" => 1,
        "world" => 2,
        "nonplayer" => 3,
        "corner" => 4,
        value => {
            return Err(TerrainGroupCatalogError::UnsupportedPattern {
                row: row_index,
                value: value.to_owned(),
            });
        }
    };
    let min_size = if group_type == 5 {
        direct(row, row_index, "min_size")?
    } else {
        expression(row, row_index, "min_size", world)?
    };
    let max_size = if group_type == 5 {
        direct(row, row_index, "max_size")?
    } else {
        expression(row, row_index, "max_size", world)?
    };
    let mut group = TerrainGroup {
        group_type,
        chance: direct(row, row_index, "chance")?,
        grouping: direct(row, row_index, "grouping")?,
        min_clumps: expression(row, row_index, "min_clumps", world)?,
        max_clumps: expression(row, row_index, "max_clumps", world)?,
        pattern,
        min_size,
        max_size,
        start_min: expression(row, row_index, "city_keep_away", world)?,
        start_max: upper_64(expression(row, row_index, "city_stay_near", world)?),
        forest_space: spacing(expression(row, row_index, "forest_space", world)?),
        mount_space: spacing(expression(row, row_index, "mount_space", world)?),
        rock_space: spacing(expression(row, row_index, "rock_space", world)?),
        coast_space: spacing(expression(row, row_index, "coast_space", world)?),
        cent_min: expression(row, row_index, "cent_keep_away", world)?,
        cent_max: upper_64(expression(row, row_index, "cent_stay_near", world)?),
        edge_min: expression(row, row_index, "edge_keep_away", world)?,
        edge_max: upper_64(expression(row, row_index, "edge_stay_near", world)?),
        corner_min: expression(row, row_index, "corner_keep_away", world)?,
        corner_max: upper_64(expression(row, row_index, "corner_stay_near", world)?),
        touching_mountain: -1,
        ..TerrainGroup::default()
    };
    if group_type == 6 {
        group.min_oil = optional_direct(row, row_index, "min_oil", -1)?;
        group.max_oil = optional_direct(row, row_index, "max_oil", -1)?;
    }
    if group_type == 8 {
        let cliff_face = optional_direct(row, row_index, "cliff_face", -1)?;
        group.cliff_face = if cliff_face == -1 { 0 } else { cliff_face };
    }
    Ok(group)
}

fn required<'a>(
    row: &'a StaticXmlEntry,
    row_index: usize,
    attribute: &'static str,
) -> Result<&'a str, TerrainGroupCatalogError> {
    row.attribute(attribute)
        .ok_or(TerrainGroupCatalogError::MissingAttribute {
            row: row_index,
            attribute,
        })
}

fn direct(
    row: &StaticXmlEntry,
    row_index: usize,
    attribute: &'static str,
) -> Result<i32, TerrainGroupCatalogError> {
    let raw = required(row, row_index, attribute)?;
    raw.trim()
        .parse::<i32>()
        .map_err(|_| TerrainGroupCatalogError::InvalidInteger {
            row: row_index,
            attribute,
            value: raw.to_owned(),
        })
}

fn optional_direct(
    row: &StaticXmlEntry,
    row_index: usize,
    attribute: &'static str,
    default: i32,
) -> Result<i32, TerrainGroupCatalogError> {
    match row.attribute(attribute) {
        Some(_) => direct(row, row_index, attribute),
        None => Ok(default),
    }
}

fn expression(
    row: &StaticXmlEntry,
    row_index: usize,
    attribute: &'static str,
    world: &don_sim::systems::map_terrain::World,
) -> Result<i32, TerrainGroupCatalogError> {
    let raw = required(row, row_index, attribute)?;
    let fields = raw.split_whitespace().collect::<Vec<_>>();
    let Some(first) = fields.first() else {
        return Err(TerrainGroupCatalogError::InvalidInteger {
            row: row_index,
            attribute,
            value: raw.to_owned(),
        });
    };
    let value = first
        .parse::<i32>()
        .map_err(|_| TerrainGroupCatalogError::InvalidInteger {
            row: row_index,
            attribute,
            value: raw.to_owned(),
        })?;
    let Some(suffix) = fields.get(1) else {
        return Ok(value);
    };
    if fields.len() != 2 {
        return Err(TerrainGroupCatalogError::UnsupportedExpressionSuffix {
            row: row_index,
            attribute,
            value: raw.to_owned(),
            suffix: fields[1..].join(" "),
        });
    }
    match *suffix {
        "SCALE" => Ok(scale_by_axis(world.xs, value)),
        "AREA" => Ok(scale_by_area(world.size, value)),
        // These shipped suffixes are comments to the native parser. Its four
        // recognized scaling keywords do not match them, so the parsed base
        // integer is retained unchanged.
        "min" | "max" | "minsize" | "mindist" | "maxdist" | "mntspace" | "forestspace"
        | "rockspace" | "rockspacee" | "coastspace" | "space" | "spacing" => Ok(value),
        _ => Err(TerrainGroupCatalogError::UnsupportedExpressionSuffix {
            row: row_index,
            attribute,
            value: raw.to_owned(),
            suffix: (*suffix).to_owned(),
        }),
    }
}

fn scale_by_axis(xs: i32, value: i32) -> i32 {
    if value < 1 {
        0
    } else {
        ((STANDARD_MAP_EDGE / 2).wrapping_add(xs.wrapping_mul(value)) / STANDARD_MAP_EDGE).max(1)
    }
}

fn scale_by_area(size: i32, value: i32) -> i32 {
    if value < 1 {
        0
    } else {
        let standard_area = STANDARD_MAP_EDGE * STANDARD_MAP_EDGE;
        ((standard_area / 2).wrapping_add(size.wrapping_mul(value)) / standard_area).max(1)
    }
}

const fn spacing(value: i32) -> i32 {
    if value < 0 {
        0
    } else if value > 64 {
        64
    } else {
        value
    }
}

const fn upper_64(value: i32) -> i32 {
    if value > 64 {
        64
    } else {
        value
    }
}

fn validate_capture_evidence(
    plan: &InitialItemReconstruction,
    selected_tileset: &str,
    evidence: &PlaceAllLiveCaptureEvidence,
) -> Result<(), PlaceAllPrerequisiteError> {
    if evidence.executable_sha256 != SHIPPED_EXE_SHA256 {
        return Err(PlaceAllPrerequisiteError::CaptureExecutableMismatch {
            expected: SHIPPED_EXE_SHA256,
            actual: evidence.executable_sha256.clone(),
        });
    }
    if evidence.entry_va != TERRAIN_GROUPS_PLACE_ALL_VA
        || evidence.reporting_va != TERRAIN_GROUPS_REPORTING_VA
    {
        return Err(PlaceAllPrerequisiteError::CaptureAnchorMismatch {
            expected_entry: TERRAIN_GROUPS_PLACE_ALL_VA,
            actual_entry: evidence.entry_va,
            expected_reporting: TERRAIN_GROUPS_REPORTING_VA,
            actual_reporting: evidence.reporting_va,
        });
    }
    if evidence.map_style != plan.inputs.map_style {
        return Err(PlaceAllPrerequisiteError::CaptureMapStyleMismatch {
            replay: plan.inputs.map_style,
            captured: evidence.map_style,
        });
    }
    if evidence.tileset != selected_tileset {
        return Err(PlaceAllPrerequisiteError::CaptureTilesetMismatch {
            selected: selected_tileset.to_owned(),
            captured: evidence.tileset.clone(),
        });
    }
    Ok(())
}

fn record_presence(
    present: bool,
    kind: PlaceAllFactKind,
    missing: fn() -> UnavailablePlaceAllFact,
    available: &mut Vec<PlaceAllFactKind>,
    unavailable: &mut Vec<UnavailablePlaceAllFact>,
) {
    if present {
        available.push(kind);
    } else {
        unavailable.push(missing());
    }
}

fn push_all_live_unavailable(unavailable: &mut Vec<UnavailablePlaceAllFact>) {
    unavailable.extend([
        missing_subtype_freqs(),
        missing_console_info(),
        missing_mountains(),
        missing_tdata(),
        missing_doober_rules(),
        missing_progress(),
        missing_helping(),
        missing_reporting_scores(),
        missing_host(),
    ]);
}

fn missing_subtype_freqs() -> UnavailablePlaceAllFact {
    UnavailablePlaceAllFact {
        kind: PlaceAllFactKind::TerrainSubtypeFrequencies,
        // CORRECTION (place_all boundary lane): these rows are neither read nor
        // written anywhere in place_all. A full Capstone pass over
        // 0x006a70d0..0x006a93a3 finds every `this` dereference to be
        // [this+0x10] (the group pointer array), [this+0x04] (its count) or the
        // single [this+0x78] write; nothing touches +0x24/+0x40/+0x5c. Their
        // only consumer is the CDF at +0xf8 that fill_fertile 0x006a6f90 walks,
        // which Map::make runs at 0x0068bfb8, before place_all at 0x0068c010.
        // They are also pure shipped data: init_tileset_data 0x006a61f0 fills
        // row N (0 BASELAND, 1 SANDY, 2 OCEAN, per Lands::init 0x0067e730 and
        // the land_key literals at 0x00ecda20) from the map style's
        // TILESET_DATA/<tileset>/LANDKEY[name]/frequency_<i> attributes.
        required_source:
            "not read by place_all; otherwise mapstyle TILESET_DATA/LANDKEY frequency_N",
        addresses: vec![GAME_MAP_POINTER_VA, crate::fractal_boundary::TERRAIN_GROUPS_INIT_TILESET_DATA_VA],
    }
}

fn missing_console_info() -> UnavailablePlaceAllFact {
    UnavailablePlaceAllFact {
        kind: PlaceAllFactKind::TerrainConsoleInfo,
        // CORRECTION (place_all boundary lane): +0x78 has exactly one writer in
        // the whole image -- place_all at 0x006a91b5, which stores 0 -- and no
        // reader anywhere. It gates no branch, so it cannot reach World, the
        // main RNG, or even the reporting tail, which is unconditional. The
        // TerrainGroups constructor 0x0047a9c0 skips +0x78 and +0x7c entirely,
        // so before the first place_all it is allocator residue.
        required_source: "gates nothing; place_all 0x006a91b5 is its only writer and stores 0",
        addresses: vec![GAME_MAP_POINTER_VA, 0x006a_91b5],
    }
}

fn missing_mountains() -> UnavailablePlaceAllFact {
    UnavailablePlaceAllFact {
        kind: PlaceAllFactKind::MountainRangeListsAndCursors,
        // SHARPENED (place_all boundary lane): the four per-game placement
        // arrays are NOT part of this fact. All 21 map-style make_continents
        // overrides call World::wipe 0x006b2c00 on their straight-line path,
        // and World::wipe calls Mountains::clear 0x0089bec0 at 0x006b2d97,
        // which zeroes mountain_locs/mountain_types/mountain_loc_wcoords_x/_y.
        // Mountains::add_mountain 0x0089c2e0 is only reachable from inside
        // place_all, so they are provably empty at entry. What remains is the
        // three MountainsData LinkList<int,u8> range lists at 0x00e860d4 /
        // 0x00e860ec / 0x00e86104, which Mountains::clear does not touch and
        // which Mountains::init 0x0089ad70 / add_range 0x008992b0 build from
        // the loaded tileset. Their LENGTHS are the load-bearing part:
        // randomize_mountains draws one main-RNG word per list of length > 1,
        // so a wrong length corrupts every draw in the remaining call.
        // RESOLVED (place_all boundary lane): the lists are shipped data.
        // Mountains::init parses the single <MOUNTAINS> block of
        // Data/effects_graphics.xml -- 16 <MOUNTAIN> elements, area lg x7,
        // med x8, sm x1 -- so the lengths are 1/8/7 and randomize_mountains
        // consumes exactly two main-stream words. crate::place_all_advance::
        // resolve_mountain_ranges builds them.
        required_source:
            "shipped Data/effects_graphics.xml <MOUNTAINS>; lengths 1/8/7 set the randomize_mountains draw count",
        addresses: vec![MOUNTAINS_GLOBAL_VA, 0x0089_ca70, 0x0089_ad70, 0x0089_92b0],
    }
}

fn missing_tdata() -> UnavailablePlaceAllFact {
    UnavailablePlaceAllFact {
        kind: PlaceAllFactKind::TDataPlane,
        required_source:
            "World tile dimensions +0x18/+0x1c/+0x20 and TData* +0x138 at place_all entry",
        addresses: vec![WORLD_GLOBAL_VA, TERRAIN_GROUPS_PLACE_ALL_VA],
    }
}

fn missing_doober_rules() -> UnavailablePlaceAllFact {
    UnavailablePlaceAllFact {
        kind: PlaceAllFactKind::DooberTilesetRules,
        // CORRECTION (place_all boundary lane): shipped data. TileSetGroupData
        // is sixteen ints at offsets 0..60 in the PDB, and the selected
        // TILESET/TERRAINGROUP element in Data/tilesets.xml declares exactly
        // sixteen <NAME value="N"/> children matching those field names in
        // offset order. crate::fractal_boundary::FertilityBoundary::doober_rules
        // now reads them from the same element it already read CLUMP_FACTOR
        // (field 0) from.
        required_source: "shipped Data/tilesets.xml TILESET[selected]/TERRAINGROUP; TileSetData.group_data +0x614 fields +0x14 and +0x20..+0x3c",
        addresses: vec![TILESETS_GLOBAL_VA, TERRAIN_GROUPS_PLACE_ALL_VA],
    }
}

fn missing_progress() -> UnavailablePlaceAllFact {
    UnavailablePlaceAllFact {
        kind: PlaceAllFactKind::ProgressArgument,
        // CORRECTION (place_all boundary lane): derived, and irrelevant either
        // way. Map::make forwards its third parameter with `push esi` at
        // 0x0068c009; Setup::build_game dispatches Map::make at 0x005ac657 with
        // its own first argument, which Game::init computes as
        // `sete al` on (param_3 == 0) at 0x0058cab9..0x0058cac2 and pushes at
        // 0x0058d1b0. Both Game::run dispatches (0x00584b52, 0x00584db6) pass
        // param_3 = 0, so a normal game start is place_all(1, 1). The argument
        // is tested only at 0x006a79f1 / 0x006a86f1 / 0x006a8a61, each guarding
        // a splash-caption block with no RNG draw and no simulation write.
        required_source: "derived: 1 in a normal game start; gates only splash-caption blocks",
        addresses: vec![0x0068_c009, 0x0058_d1b0, 0x0058_cab9],
    }
}

fn missing_helping() -> UnavailablePlaceAllFact {
    UnavailablePlaceAllFact {
        kind: PlaceAllFactKind::HelpingGlobals,
        // CORRECTION (place_all boundary lane): place_all initialises all of
        // these itself, before its own first read. num_players 0x00cae70c is
        // world.start_x.length, written at 0x006a72eb..0x006a72fb;
        // lowest_player[5] is zeroed at 0x006a75ab/0x006a75b4 and never read by
        // place_all; player_scores rows [0, num_players) are zeroed at
        // 0x006a75ee..0x006a760c; is_helping is set to 0 at 0x006a7618, before
        // its first read at 0x006a82d0, and is thereafter recomputed from
        // help_lowest_index[5] 0x00cbe460, which place_all also builds. It is a
        // placement-loop flag, not a game-setup option.
        // crate::place_all_advance::initial_region_helping_state builds it.
        required_source: "derived: place_all initialises is_helping/lowest_player/player_scores before reading them",
        addresses: vec![0x006a_7618, 0x006a_75ab, 0x006a_75ee, 0x006a_72eb],
    }
}

fn missing_reporting_scores() -> UnavailablePlaceAllFact {
    UnavailablePlaceAllFact {
        kind: PlaceAllFactKind::ReportingScores,
        // CORRECTION (place_all boundary lane): this is a port gap, not a
        // capture requirement. player_scores[8][5] is zeroed by place_all and
        // then accumulated inside the same call by place_region_group
        // 0x006a2f60, which adds each successful tile's octagonal distance to
        // every participating player. The reporting tail at 0x006a8f12 reads
        // the result. Nothing outside place_all supplies it.
        required_source: "produced inside place_all by place_region_group 0x006a2f60; the port does not yet accumulate it",
        addresses: vec![NUM_PLAYERS_VA, PLAYER_SCORES_VA, TERRAIN_GROUPS_REPORTING_VA],
    }
}

fn missing_host() -> UnavailablePlaceAllFact {
    UnavailablePlaceAllFact {
        kind: PlaceAllFactKind::HostGroupResolutions,
        required_source: "ordered typed player/region external resolutions captured from the selected group calls",
        addresses: vec![TERRAIN_GROUPS_PLACE_ALL_VA, TERRAIN_GROUPS_REPORTING_VA],
    }
}
