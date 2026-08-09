//! Exact static-input and byte-plane boundary for `TerrainGroups::fill_fertile`.
//!
//! A replay does not serialize this plane.  Retail reconstructs it from the
//! replay seed plus two pieces of installed static content: the selected
//! tileset's `TileSetGroupData::clump_factor`, and the effective map-style
//! `TILESET_DATA/LANDKEY[name=baseland]` frequency row.  This module keeps
//! those sources explicit and fails closed when either lawful local file is
//! absent.

use don_sim::rng::Random;
use don_sim::systems::terrain_groups::{FertilityFractal, TerrainGroups};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

pub const MAP_INIT_MAP_DATA_VA: u32 = 0x0069_f250;
pub const MAP_TILESET_SELECTION_RNG_VA: u32 = 0x0069_f97e;
pub const MAP_LOAD_MAP_DATA_VA: u32 = 0x0069_dad0;
pub const TERRAIN_GROUPS_INIT_TILESET_DATA_VA: u32 = 0x006a_61f0;
pub const FRACTAL_INIT_CALL_VA: u32 = 0x006a_6453;
pub const FRACTAL_INIT_VA: u32 = 0x006a_a2d0;
pub const TERRAIN_GROUPS_FILL_FERTILE_VA: u32 = 0x006a_6f90;
pub const FRACTAL_RANDOM_SITES: [u32; 4] = [0x006a_a5eb, 0x006a_a67f, 0x006a_a717, 0x006a_a7a3];
pub const TERRAIN_GROUPS_PLACE_ALL_VA: u32 = 0x006a_70d0;

/// Replay-carried values which are sufficient to regenerate the fertility
/// plane once the three static XML inputs have been admitted.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FractalReplayInputs {
    pub seed: u32,
    pub map_style: u8,
    pub scenario_type: u8,
    pub world_edge: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FractalBoundarySources {
    pub default_map_style_xml: PathBuf,
    pub selected_map_style_xml: PathBuf,
    /// Shipped `Data/tilesets.xml`.  It is copyrighted install data and is an
    /// explicit caller-owned input, just like the gitignored map-style XML.
    pub tilesets_xml: PathBuf,
}

/// The exact `Fractal::frac : ObjectArray<Array<unsigned char>>` allocation.
/// Both guard edges are retained: there are `xs + 1` columns and `ys + 1`
/// bytes per column, while `fill_fertile` reads only `[0,xs) x [0,ys)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetailFractalPlane {
    pub xs: i32,
    pub ys: i32,
    pub smooth: i32,
    pub seed: u32,
    pub columns: Vec<Vec<u8>>,
    pub random_draws: u32,
    pub random_state_after: i32,
}

impl RetailFractalPlane {
    pub fn flat_x_major(&self) -> Vec<u8> {
        let xs = self.xs.max(0) as usize;
        let ys = self.ys.max(0) as usize;
        self.columns[..xs]
            .iter()
            .flat_map(|column| column[..ys].iter().copied())
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FertilityBoundary {
    pub map_style: u8,
    pub tileset: String,
    pub tile_chances: Vec<TileChance>,
    /// The final applied pass's value from `game_random.get(0, 0xffff)` before
    /// `% total`. `None` means that pass took the total-chance no-draw path;
    /// earlier draws remain visible in `tile_selection_passes`.
    pub tile_selection_draw: Option<i32>,
    pub tile_selection_bucket: i32,
    pub main_random_state_after_tileset: i32,
    pub tile_selection_passes: Vec<TileSelectionPass>,
    pub baseland_frequencies: Vec<i32>,
    pub partitions: Vec<u8>,
    pub clump_factor: i32,
    pub plane: RetailFractalPlane,
}

impl FertilityBoundary {
    pub fn tile_selection(&self) -> TileSelectionBoundary {
        TileSelectionBoundary {
            tileset: self.tileset.clone(),
            tile_chances: self.tile_chances.clone(),
            draw: self.tile_selection_draw,
            bucket: self.tile_selection_bucket,
            main_random_state_after: self.main_random_state_after_tileset,
            passes: self.tile_selection_passes.clone(),
        }
    }

    /// Construct the exact deterministic inputs already consumed by the
    /// instruction-complete `TerrainGroups::fill_fertile` port.
    pub fn terrain_groups_input(&self) -> TerrainGroups {
        TerrainGroups {
            fractal: FertilityFractal {
                columns: self.plane.columns.clone(),
            },
            partitions: self.partitions.clone(),
            ..TerrainGroups::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TileChance {
    pub tileset: String,
    pub chance: i32,
}

/// Complete main-RNG effect of `Map::init_map_data`'s tileset table. This
/// precedes Map's orientation draw and does not require `Data/tilesets.xml`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TileSelectionBoundary {
    pub tileset: String,
    pub tile_chances: Vec<TileChance>,
    pub draw: Option<i32>,
    pub bucket: i32,
    pub main_random_state_after: i32,
    /// `load_map_data` applies default MAP first, then selected MAP. A selected
    /// TILESET subtree therefore overrides the result and consumes a second
    /// chance-table draw.
    pub passes: Vec<TileSelectionPass>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TileSelectionSource {
    DefaultMapStyle,
    SelectedMapStyle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TileSelectionPass {
    pub source: TileSelectionSource,
    pub tileset: String,
    pub tile_chances: Vec<TileChance>,
    pub draw: Option<i32>,
    pub bucket: i32,
    pub main_random_state_after: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FractalBoundaryError {
    MissingInstalledTilesetsSource,
    Read {
        path: PathBuf,
        message: String,
    },
    CustomScenario {
        scenario_type: u8,
    },
    PriorSeedState {
        seed: u32,
    },
    UnknownWorldEdge,
    InvalidWorldDimensions {
        xs: i32,
        ys: i32,
    },
    InvalidSmooth {
        smooth: i32,
        ys: i32,
    },
    MalformedXml {
        context: &'static str,
        offset: usize,
        message: String,
    },
    MissingElement {
        context: &'static str,
        element: String,
    },
    DuplicateElement {
        context: &'static str,
        element: String,
    },
    EmptyTileChanceTable,
    InvalidInteger {
        context: &'static str,
        attribute: String,
        value: String,
    },
    InvalidTileChance {
        tileset: String,
        chance: i32,
    },
    TileChanceTotalOverflow,
    TilesetNotInstalled {
        tileset: String,
    },
    MissingBaselandFrequency {
        tileset: String,
        index: usize,
    },
    NoBaselandTextures {
        tileset: String,
    },
}

impl fmt::Display for FractalBoundaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingInstalledTilesetsSource => {
                write!(f, "installed Data/tilesets.xml source was not supplied")
            }
            Self::Read { path, message } => write!(f, "{}: {message}", path.display()),
            Self::CustomScenario { scenario_type } => {
                write!(
                    f,
                    "scenario type {scenario_type} does not prove procedural dimensions"
                )
            }
            Self::PriorSeedState { seed } => write!(
                f,
                "seed {seed:#010x} is signed-negative; Map::make preserves prior RNG state"
            ),
            Self::UnknownWorldEdge => write!(f, "map-size selector has no shipped world edge"),
            Self::InvalidWorldDimensions { xs, ys } => {
                write!(
                    f,
                    "Fractal::init requires dimensions above one, got {xs}x{ys}"
                )
            }
            Self::InvalidSmooth { smooth, ys } => {
                write!(f, "Fractal::init rejects smooth {smooth} for height {ys}")
            }
            Self::MalformedXml {
                context,
                offset,
                message,
            } => {
                write!(f, "{context} XML at {offset:#x}: {message}")
            }
            Self::MissingElement { context, element } => {
                write!(f, "{context} XML has no <{element}>")
            }
            Self::DuplicateElement { context, element } => {
                write!(f, "{context} XML has duplicate <{element}>")
            }
            Self::EmptyTileChanceTable => write!(f, "effective MAP/TILESET has no TILECHANCE"),
            Self::InvalidInteger {
                context,
                attribute,
                value,
            } => {
                write!(
                    f,
                    "{context} attribute {attribute} is not an i32: {value:?}"
                )
            }
            Self::InvalidTileChance { tileset, chance } => {
                write!(f, "tileset {tileset:?} has non-positive chance {chance}")
            }
            Self::TileChanceTotalOverflow => write!(f, "TILECHANCE sum overflows positive i32"),
            Self::TilesetNotInstalled { tileset } => {
                write!(f, "Data/tilesets.xml has no TILESET named {tileset:?}")
            }
            Self::MissingBaselandFrequency { tileset, index } => write!(
                f,
                "TILESET_DATA/{tileset}/LANDKEY[baseland] lacks frequency_{index}"
            ),
            Self::NoBaselandTextures { tileset } => {
                write!(f, "installed tileset {tileset:?} has no BASELAND/BASE rows")
            }
        }
    }
}

impl std::error::Error for FractalBoundaryError {}

pub fn resolve_fertility_boundary(
    inputs: FractalReplayInputs,
    sources: &FractalBoundarySources,
) -> Result<FertilityBoundary, FractalBoundaryError> {
    let default = read_xml(&sources.default_map_style_xml)?;
    let selected = read_xml(&sources.selected_map_style_xml)?;
    let tilesets = read_xml(&sources.tilesets_xml)?;
    resolve_fertility_boundary_xml(inputs, &default, &selected, &tilesets)
}

/// Resolve the tileset-selection prefix from admitted map-style evidence.
/// The returned state is the correct main-RNG handoff to Map's orientation
/// branch even when installed tileset data is unavailable.
pub fn resolve_tile_selection(
    seed: u32,
    default_map_style_xml: &Path,
    selected_map_style_xml: &Path,
) -> Result<TileSelectionBoundary, FractalBoundaryError> {
    let default = read_xml(default_map_style_xml)?;
    let selected = read_xml(selected_map_style_xml)?;
    resolve_tile_selection_xml(seed, &default, &selected)
}

pub fn resolve_tile_selection_xml(
    seed: u32,
    default_map_style_xml: &str,
    selected_map_style_xml: &str,
) -> Result<TileSelectionBoundary, FractalBoundaryError> {
    let default = strip_comments(default_map_style_xml, "default map-style")?;
    let selected = strip_comments(selected_map_style_xml, "selected map-style")?;
    let default_chances = tile_chances_in(&default, "default map-style")?.ok_or_else(|| {
        FractalBoundaryError::MissingElement {
            context: "default map-style",
            element: "MAP/TILESET".into(),
        }
    })?;
    let mut passes = vec![select_tileset_pass(
        seed,
        TileSelectionSource::DefaultMapStyle,
        default_chances,
    )?];
    if let Some(selected_chances) = tile_chances_in(&selected, "selected map-style")? {
        let state = passes
            .last()
            .expect("default pass was just installed")
            .main_random_state_after;
        passes.push(select_tileset_pass(
            state as u32,
            TileSelectionSource::SelectedMapStyle,
            selected_chances,
        )?);
    }
    let final_pass = passes.last().expect("default pass is mandatory");
    Ok(TileSelectionBoundary {
        tileset: final_pass.tileset.clone(),
        tile_chances: final_pass.tile_chances.clone(),
        draw: final_pass.draw,
        bucket: final_pass.bucket,
        main_random_state_after: final_pass.main_random_state_after,
        passes,
    })
}

/// Pure form used by mutation-sensitive tests and callers which already own
/// the admitted file bytes.
pub fn resolve_fertility_boundary_xml(
    inputs: FractalReplayInputs,
    default_map_style_xml: &str,
    selected_map_style_xml: &str,
    tilesets_xml: &str,
) -> Result<FertilityBoundary, FractalBoundaryError> {
    if inputs.scenario_type != 0 {
        return Err(FractalBoundaryError::CustomScenario {
            scenario_type: inputs.scenario_type,
        });
    }
    if (inputs.seed as i32) < 0 {
        return Err(FractalBoundaryError::PriorSeedState { seed: inputs.seed });
    }
    let edge = inputs
        .world_edge
        .ok_or(FractalBoundaryError::UnknownWorldEdge)?;
    if edge <= 1 {
        return Err(FractalBoundaryError::InvalidWorldDimensions { xs: edge, ys: edge });
    }

    let selection =
        resolve_tile_selection_xml(inputs.seed, default_map_style_xml, selected_map_style_xml)?;
    let default = strip_comments(default_map_style_xml, "default map-style")?;
    let selected = strip_comments(selected_map_style_xml, "selected map-style")?;
    let tilesets = strip_comments(tilesets_xml, "tilesets")?;

    let TileSelectionBoundary {
        tileset,
        tile_chances,
        draw: selection_draw,
        bucket: selection_bucket,
        main_random_state_after: main_random_state_after_tileset,
        passes: tile_selection_passes,
    } = selection;
    let installed = installed_tileset(&tilesets, &tileset)?;
    let baseland = unique_element(&installed.body, "BASELAND", "tilesets")?.ok_or_else(|| {
        FractalBoundaryError::MissingElement {
            context: "tilesets",
            element: format!("TILESET[{tileset}]/BASELAND"),
        }
    })?;
    let base_count = count_open_tags(&baseland.body, "BASE")?;
    if base_count == 0 {
        return Err(FractalBoundaryError::NoBaselandTextures {
            tileset: tileset.clone(),
        });
    }
    let terrain_group =
        unique_element(&installed.body, "TERRAINGROUP", "tilesets")?.ok_or_else(|| {
            FractalBoundaryError::MissingElement {
                context: "tilesets",
                element: format!("TILESET[{tileset}]/TERRAINGROUP"),
            }
        })?;
    let clump =
        unique_element(&terrain_group.body, "CLUMP_FACTOR", "tilesets")?.ok_or_else(|| {
            FractalBoundaryError::MissingElement {
                context: "tilesets",
                element: format!("TILESET[{tileset}]/TERRAINGROUP/CLUMP_FACTOR"),
            }
        })?;
    let clump_factor = element_i32(&clump, "value", "tilesets")?;

    let baseland_frequencies =
        effective_baseland_frequencies(&default, &selected, &tileset, base_count)?;
    let partitions = build_partitions(&baseland_frequencies);
    let plane = generate_retail_fractal(edge, edge, clump_factor, inputs.seed)?;

    Ok(FertilityBoundary {
        map_style: inputs.map_style,
        tileset,
        tile_chances,
        tile_selection_draw: selection_draw,
        tile_selection_bucket: selection_bucket,
        main_random_state_after_tileset,
        tile_selection_passes,
        baseland_frequencies,
        partitions,
        clump_factor,
        plane,
    })
}

/// `TerrainGroups::init_tileset_data` `0x006a6474..0x006a64c6`.
/// Each percentage is converted through binary32, truncated, narrowed to its
/// low byte, and accumulated.  The last frequency has no partition entry.
pub fn build_partitions(frequencies: &[i32]) -> Vec<u8> {
    let mut cumulative = 0i32;
    frequencies
        .iter()
        .take(frequencies.len().saturating_sub(1))
        .map(|frequency| {
            let contribution = ((*frequency as f32 / 100.0_f32) * 255.0_f32) as i32;
            cumulative = cumulative.wrapping_add(i32::from(contribution as u8));
            cumulative as u8
        })
        .collect()
}

/// Instruction-faithful `Fractal::init` for the non-wrapping flags used by
/// `TerrainGroups`.  The internal `Fractal::random` is seeded from
/// `WorldData::seed`; it does not consume `game_random`.
pub fn generate_retail_fractal(
    xs: i32,
    ys: i32,
    smooth: i32,
    seed: u32,
) -> Result<RetailFractalPlane, FractalBoundaryError> {
    if xs <= 1 || ys <= 1 {
        return Err(FractalBoundaryError::InvalidWorldDimensions { xs, ys });
    }
    if smooth > 30 || (smooth > 1 && ys < (1i32 << smooth)) {
        return Err(FractalBoundaryError::InvalidSmooth { smooth, ys });
    }

    let smooth = smooth.clamp(0, 5);
    let guarded_xs = xs
        .checked_add(1)
        .ok_or(FractalBoundaryError::InvalidWorldDimensions { xs, ys })?;
    let guarded_ys = ys
        .checked_add(1)
        .ok_or(FractalBoundaryError::InvalidWorldDimensions { xs, ys })?;
    let width = usize::try_from(guarded_xs)
        .map_err(|_| FractalBoundaryError::InvalidWorldDimensions { xs, ys })?;
    let height = usize::try_from(guarded_ys)
        .map_err(|_| FractalBoundaryError::InvalidWorldDimensions { xs, ys })?;
    let mut columns = vec![vec![0u8; height]; width];
    let mut random = Random::new(seed as i32);
    let mut random_draws = 0u32;
    let base_amplitude_shift = 7 - smooth;

    for level in (0..=smooth).rev() {
        let step = 1usize << level;
        let mask = (1usize << (level + 1)) - 1;

        // flags & 1 is zero at the only TerrainGroups call site.
        columns[xs as usize] = columns[0].clone();
        let x_count = (xs >> level) as usize;
        let y_count = (ys >> level) as usize;

        for x_index in 0..x_count {
            let x = x_index << level;
            for y_index in 0..y_count {
                let y = y_index << level;
                if level == smooth {
                    columns[x][y] = random.get(0, 0xff) as u8;
                    random_draws += 1;
                    continue;
                }

                let average = if x & mask == 0 {
                    if y & mask == 0 {
                        // Coarse lattice point written by an earlier pass.
                        continue;
                    }
                    (u32::from(columns[x][y + step]) + u32::from(columns[x][y - step]) + 1) >> 1
                } else if y & mask == 0 {
                    (u32::from(columns[x + step][y]) + u32::from(columns[x - step][y]) + 1) >> 1
                } else {
                    (u32::from(columns[x - step][y - step])
                        + u32::from(columns[x + step][y + step])
                        + u32::from(columns[x + step][y - step])
                        + u32::from(columns[x - step][y + step])
                        + 2)
                        >> 2
                };

                let amplitude_shift = base_amplitude_shift + level;
                let upper = (1i32 << (amplitude_shift + 1)) - 1;
                let noise = random.get(0, upper) - (1i32 << amplitude_shift);
                random_draws += 1;
                columns[x][y] = (average as i32 + noise).clamp(0, 0xff) as u8;
            }
        }
    }
    columns[xs as usize] = columns[0].clone();

    Ok(RetailFractalPlane {
        xs,
        ys,
        smooth,
        seed,
        columns,
        random_draws,
        random_state_after: random.state(),
    })
}

fn tile_chances_in(
    text: &str,
    context: &'static str,
) -> Result<Option<Vec<TileChance>>, FractalBoundaryError> {
    let Some(table) = unique_element(text, "TILESET", context)? else {
        return Ok(None);
    };
    let mut chances = Vec::new();
    for element in open_tags(&table.body, "TILECHANCE", context)? {
        let tileset = required_attribute(&element, "type", context)?.to_owned();
        let chance = element_i32(&element, "chance", context)?;
        if chance <= 0 {
            return Err(FractalBoundaryError::InvalidTileChance { tileset, chance });
        }
        chances.push(TileChance { tileset, chance });
    }
    if chances.is_empty() {
        return Err(FractalBoundaryError::EmptyTileChanceTable);
    }
    Ok(Some(chances))
}

fn select_tileset_pass(
    seed: u32,
    source: TileSelectionSource,
    tile_chances: Vec<TileChance>,
) -> Result<TileSelectionPass, FractalBoundaryError> {
    let (tileset, draw, bucket, main_random_state_after) = select_tileset(seed, &tile_chances)?;
    Ok(TileSelectionPass {
        source,
        tileset,
        tile_chances,
        draw,
        bucket,
        main_random_state_after,
    })
}

fn select_tileset(
    seed: u32,
    chances: &[TileChance],
) -> Result<(String, Option<i32>, i32, i32), FractalBoundaryError> {
    let total = chances.iter().try_fold(0i32, |sum, row| {
        sum.checked_add(row.chance)
            .ok_or(FractalBoundaryError::TileChanceTotalOverflow)
    })?;
    if total <= 0 {
        return Err(FractalBoundaryError::TileChanceTotalOverflow);
    }
    let mut random = Random::new(seed as i32);
    let selection_draw = (total >= 2).then(|| random.get(0, 0xffff));
    let bucket = selection_draw.map_or(0, |draw| draw % total);
    let mut cumulative = 0i32;
    for row in chances {
        cumulative += row.chance;
        // Retail uses `jle`: equality stays in the earlier bucket.
        if bucket <= cumulative {
            return Ok((row.tileset.clone(), selection_draw, bucket, random.state()));
        }
    }
    Err(FractalBoundaryError::TileChanceTotalOverflow)
}

fn effective_baseland_frequencies(
    default: &str,
    selected: &str,
    tileset: &str,
    count: usize,
) -> Result<Vec<i32>, FractalBoundaryError> {
    let key = tileset.to_ascii_uppercase();
    let default_row = baseland_row(default, &key, "default map-style")?;
    let selected_row = baseland_row(selected, &key, "selected map-style")?;
    let mut values = Vec::with_capacity(count);
    for index in 0..count {
        let attribute = format!("frequency_{index}");
        let value = selected_row
            .as_ref()
            .and_then(|row| row.attributes.get(&attribute))
            .or_else(|| {
                default_row
                    .as_ref()
                    .and_then(|row| row.attributes.get(&attribute))
            })
            .ok_or_else(|| FractalBoundaryError::MissingBaselandFrequency {
                tileset: tileset.to_owned(),
                index,
            })?;
        values.push(parse_i32(value, "map-style", &attribute)?);
    }
    Ok(values)
}

fn baseland_row(
    text: &str,
    key: &str,
    context: &'static str,
) -> Result<Option<XmlElement>, FractalBoundaryError> {
    let Some(data) = unique_element(text, "TILESET_DATA", context)? else {
        return Ok(None);
    };
    let Some(tileset) = unique_element(&data.body, key, context)? else {
        return Ok(None);
    };
    let rows = open_tags(&tileset.body, "LANDKEY", context)?;
    let mut matches = rows.into_iter().filter(|row| {
        row.attributes
            .get("name")
            .is_some_and(|name| name.eq_ignore_ascii_case("baseland"))
    });
    let first = matches.next();
    if matches.next().is_some() {
        return Err(FractalBoundaryError::DuplicateElement {
            context,
            element: format!("TILESET_DATA/{key}/LANDKEY[baseland]"),
        });
    }
    Ok(first)
}

fn installed_tileset(text: &str, name: &str) -> Result<XmlElement, FractalBoundaryError> {
    let rows = elements(text, "TILESET", "tilesets")?;
    let mut matches = rows.into_iter().filter(|row| {
        row.attributes
            .get("name")
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
    });
    let first = matches
        .next()
        .ok_or_else(|| FractalBoundaryError::TilesetNotInstalled {
            tileset: name.to_owned(),
        })?;
    if matches.next().is_some() {
        return Err(FractalBoundaryError::DuplicateElement {
            context: "tilesets",
            element: format!("TILESET[name={name}]"),
        });
    }
    Ok(first)
}

#[derive(Clone, Debug)]
struct XmlElement {
    attributes: BTreeMap<String, String>,
    body: String,
}

fn read_xml(path: &Path) -> Result<String, FractalBoundaryError> {
    fs::read_to_string(path).map_err(|error| FractalBoundaryError::Read {
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

fn strip_comments(text: &str, context: &'static str) -> Result<String, FractalBoundaryError> {
    let mut output = String::with_capacity(text.len());
    let mut at = 0usize;
    while let Some(relative) = text[at..].find("<!--") {
        let start = at + relative;
        output.push_str(&text[at..start]);
        let body = start + 4;
        let end = text[body..]
            .find("-->")
            .map(|relative| body + relative)
            .ok_or_else(|| FractalBoundaryError::MalformedXml {
                context,
                offset: start,
                message: "unclosed comment".into(),
            })?;
        at = end + 3;
    }
    output.push_str(&text[at..]);
    Ok(output)
}

fn unique_element(
    text: &str,
    name: &str,
    context: &'static str,
) -> Result<Option<XmlElement>, FractalBoundaryError> {
    let mut found = elements(text, name, context)?;
    if found.len() > 1 {
        return Err(FractalBoundaryError::DuplicateElement {
            context,
            element: name.to_owned(),
        });
    }
    Ok(found.pop())
}

fn elements(
    text: &str,
    name: &str,
    context: &'static str,
) -> Result<Vec<XmlElement>, FractalBoundaryError> {
    let mut output = Vec::new();
    let mut search = 0usize;
    while let Some(start) = find_open_tag(text, name, search) {
        let tag_end = text[start..]
            .find('>')
            .map(|relative| start + relative)
            .ok_or_else(|| FractalBoundaryError::MalformedXml {
                context,
                offset: start,
                message: format!("unclosed <{name}> tag"),
            })?;
        let raw = text[start + 1..tag_end].trim();
        let (tag, attributes) = parse_open_tag(raw, context, start)?;
        debug_assert_eq!(tag, name);
        if raw.ends_with('/') {
            output.push(XmlElement {
                attributes,
                body: String::new(),
            });
            search = tag_end + 1;
            continue;
        }
        let close = format!("</{name}>");
        let body_start = tag_end + 1;
        let close_start = text[body_start..]
            .find(&close)
            .map(|relative| body_start + relative)
            .ok_or_else(|| FractalBoundaryError::MalformedXml {
                context,
                offset: body_start,
                message: format!("unclosed <{name}> element"),
            })?;
        let body = text[body_start..close_start].to_owned();
        output.push(XmlElement { attributes, body });
        search = close_start + close.len();
    }
    Ok(output)
}

fn open_tags(
    text: &str,
    name: &str,
    context: &'static str,
) -> Result<Vec<XmlElement>, FractalBoundaryError> {
    let mut output = Vec::new();
    let mut search = 0usize;
    while let Some(start) = find_open_tag(text, name, search) {
        let end = text[start..]
            .find('>')
            .map(|relative| start + relative)
            .ok_or_else(|| FractalBoundaryError::MalformedXml {
                context,
                offset: start,
                message: format!("unclosed <{name}> tag"),
            })?;
        let (_, attributes) = parse_open_tag(text[start + 1..end].trim(), context, start)?;
        output.push(XmlElement {
            attributes,
            body: String::new(),
        });
        search = end + 1;
    }
    Ok(output)
}

fn count_open_tags(text: &str, name: &str) -> Result<usize, FractalBoundaryError> {
    Ok(open_tags(text, name, "tilesets")?.len())
}

fn find_open_tag(text: &str, name: &str, mut search: usize) -> Option<usize> {
    let needle = format!("<{name}");
    while let Some(relative) = text[search..].find(&needle) {
        let start = search + relative;
        let after = text.as_bytes().get(start + needle.len()).copied();
        if after.is_some_and(|byte| byte == b'>' || byte == b'/' || byte.is_ascii_whitespace()) {
            return Some(start);
        }
        search = start + needle.len();
    }
    None
}

fn parse_open_tag(
    raw: &str,
    context: &'static str,
    offset: usize,
) -> Result<(String, BTreeMap<String, String>), FractalBoundaryError> {
    let raw = raw.strip_suffix('/').unwrap_or(raw).trim_end();
    let bytes = raw.as_bytes();
    let mut at = 0usize;
    while at < bytes.len() && !bytes[at].is_ascii_whitespace() {
        at += 1;
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
        let start = at;
        while at < bytes.len() && bytes[at] != b'=' && !bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        let name = &raw[start..at];
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if bytes.get(at) != Some(&b'=') {
            return Err(FractalBoundaryError::MalformedXml {
                context,
                offset: offset + at,
                message: format!("attribute {name:?} has no '='"),
            });
        }
        at += 1;
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if bytes.get(at) != Some(&b'\"') {
            return Err(FractalBoundaryError::MalformedXml {
                context,
                offset: offset + at,
                message: format!("attribute {name:?} is not double quoted"),
            });
        }
        at += 1;
        let value_start = at;
        while at < bytes.len() && bytes[at] != b'\"' {
            at += 1;
        }
        if at == bytes.len() {
            return Err(FractalBoundaryError::MalformedXml {
                context,
                offset: offset + value_start,
                message: format!("attribute {name:?} has no closing quote"),
            });
        }
        let value = raw[value_start..at].to_owned();
        at += 1;
        if !seen.insert(name.to_owned()) {
            return Err(FractalBoundaryError::MalformedXml {
                context,
                offset: offset + start,
                message: format!("duplicate attribute {name:?}"),
            });
        }
        attributes.insert(name.to_owned(), value);
    }
    Ok((tag, attributes))
}

fn required_attribute<'a>(
    element: &'a XmlElement,
    name: &str,
    context: &'static str,
) -> Result<&'a str, FractalBoundaryError> {
    element
        .attributes
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| FractalBoundaryError::MissingElement {
            context,
            element: format!("attribute {name}"),
        })
}

fn element_i32(
    element: &XmlElement,
    name: &str,
    context: &'static str,
) -> Result<i32, FractalBoundaryError> {
    parse_i32(required_attribute(element, name, context)?, context, name)
}

fn parse_i32(
    value: &str,
    context: &'static str,
    attribute: &str,
) -> Result<i32, FractalBoundaryError> {
    value
        .parse::<i32>()
        .map_err(|_| FractalBoundaryError::InvalidInteger {
            context,
            attribute: attribute.to_owned(),
            value: value.to_owned(),
        })
}
