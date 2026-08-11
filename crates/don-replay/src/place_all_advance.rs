// SPDX-License-Identifier: GPL-3.0-or-later
//! Executed sub-boundary inside `TerrainGroups::place_all` `0x006a70d0`.
//!
//! `crate::place_all_boundary` crosses the whole call in one transaction once a
//! live capture supplies every absent producer. Ordinary `.rcx` reconstruction
//! has no such capture, so before this module the recorded boundary for every
//! checksum-bearing recording was the call's own entry address: the report could
//! not say whether the first missing thing was one instruction in or eight
//! thousand bytes in.
//!
//! This module runs the shipped `don-sim` transaction from the replay-owned
//! World/RNG and reports the **first retail primitive the reconstruction cannot
//! execute**, by exact VA. It never commits: `TerrainGroups::place_all` is
//! fail-closed in `don-sim`, so a stop leaves World, terrain groups, mountains
//! and RNG byte-for-byte unchanged. The receipt is evidence about control flow
//! and data dependencies, not about generated terrain.
//!
//! Addresses, all from the shipped PDB unless noted:
//!
//! | VA | symbol |
//! |---|---|
//! | `0x006a70d0` | `TerrainGroups::place_all` |
//! | `0x0089ca70` | `Mountains::randomize_mountains`, called at `0x006a7330` |
//! | `0x006a4190` | `TerrainGroup::place_player_group` |
//! | `0x006a2f60` | `TerrainGroup::place_region_group` |
//! | `0x006a2ab0` | `TerrainGroup::drop_tile` |
//! | `0x0089c2e0` | `Mountains::add_mountain` |
//! | `0x008a8680` | `Cliffs::verify_defensive_position` |
//! | `0x008a8bc0` | `Cliffs::position_cliff` |
//! | `0x006b2a10` | `World::set_oil_at` |
//! | `0x006a1540` | `TerrainGroups::add_doobers` |
//! | `0x006a1cc0` | `TerrainGroups::treeify_mountains` |
//! | `0x006a8f12` | the localized reporting tail inside `place_all` |
//! | `0x006a937d` | `place_all`'s `return 1` |

use crate::continent::{ContinentReceipt, ContinentStop};
use crate::fractal_boundary::TERRAIN_GROUPS_PLACE_ALL_VA;
use crate::initial::{InitialItemBoundary, InitialItemReconstruction, InitialWorld};
use crate::place_all_boundary::TERRAIN_GROUPS_PLACE_ALL_RETURN_VA;
use crate::post_continent::REGIONS_CLEAR_ALL_VA;
use don_sim::rng::Random;
use don_sim::systems::mountains::{Mountains, MountainRangeEntry, MountainRangeList};
use don_sim::systems::terrain_doobers::DooberTilesetRules;
use don_sim::systems::terrain_drop_tile::{DropTileExternalRequest, DropTileExternalResolution};
use don_sim::systems::terrain_groups::{
    PlaceAllError, PlaceAllGroupInput, PlaceAllHostEvent, PlacementReportingInputs, TerrainGroups,
    TerrainPlacementBoundary,
};
use don_sim::systems::terrain_player_group::{
    PlayerGroupExternalRequest, PlayerGroupExternalResolution,
};
use don_sim::systems::terrain_region_placement::RegionHelpingState;

pub const MOUNTAINS_RANDOMIZE_MOUNTAINS_VA: u32 = 0x0089_ca70;
pub const MOUNTAINS_RANDOMIZE_CALL_VA: u32 = 0x006a_7330;
pub const MOUNTAINS_ADD_MOUNTAIN_VA: u32 = 0x0089_c2e0;
pub const CLIFFS_VERIFY_DEFENSIVE_POSITION_VA: u32 = 0x008a_8680;
pub const CLIFFS_POSITION_CLIFF_VA: u32 = 0x008a_8bc0;
pub const WORLD_SET_OIL_AT_VA: u32 = 0x006b_2a10;
pub const TERRAIN_GROUP_PLACE_PLAYER_GROUP_VA: u32 = 0x006a_4190;
pub const TERRAIN_GROUP_PLACE_REGION_GROUP_VA: u32 = 0x006a_2f60;
pub const TERRAIN_GROUPS_ADD_DOOBERS_VA: u32 = 0x006a_1540;
pub const TERRAIN_GROUPS_TREEIFY_MOUNTAINS_VA: u32 = 0x006a_1cc0;
pub const TERRAIN_GROUPS_REPORTING_TAIL_VA: u32 = 0x006a_8f12;
/// `is_helping` `0x00cae708`, `lowest_player[5]` `0x00cbe440`,
/// `player_scores[8][5]` `0x00cbe480` — read by `place_region_group`.
pub const REGION_HELPING_IS_HELPING_VA: u32 = 0x00ca_e708;
/// `Mountains::init` `0x0089ad70`, whose `Mountains::add_range` `0x008992b0`
/// calls build the three range lists that `randomize_mountains` draws against.
pub const MOUNTAINS_INIT_VA: u32 = 0x0089_ad70;
pub const MOUNTAINS_ADD_RANGE_VA: u32 = 0x0089_92b0;
/// `World::wipe` `0x006b2c00` calls `Mountains::clear` `0x0089bec0` here, on the
/// straight-line path of all 21 `make_continents` overrides.
pub const WORLD_WIPE_MOUNTAINS_CLEAR_CALL_VA: u32 = 0x006b_2d97;

/// `place_all`'s first argument in a normal multiplayer or skirmish start.
///
/// Derived, not chosen. `Map::make` `0x0068bc90` forwards its third parameter to
/// `place_all` with `push esi` at `0x0068c009` (the literal `1` for
/// `place_players` is pushed at `0x0068c007`). `Setup::build_game` `0x005ac190`
/// dispatches `Map::make` at `0x005ac657` passing its own first argument, which
/// `Game::init` computes at `0x0058cab9`--`0x0058cac2` as
/// `sete al` on `param_3 == 0` and pushes at `0x0058d1b0`. Both `Game::run`
/// dispatches (`0x00584b52`, `0x00584db6`) pass `param_3 = 0`, so `progress` is
/// `1`. The only shipped path producing `0` is the scenario editor
/// (`TerrainWin::reinit_world` `0x008aa327`, `ScenarioEditor::generate_map`
/// `0x009a0f57`), which does not produce `.rcx` recordings.
pub const RETAIL_GAME_START_PROGRESS: i32 = 1;

/// `Mountains::init` `0x0089ad70` is reached from `Map::make` before `place_all`.
///
/// Verified call chain, each edge disassembled: `Map::make` `0x0068bc90` calls
/// `Map::load_map_data` `0x0069dad0` at `0x0068bd52`; `load_map_data` calls
/// `TileSet::load_tileset` `0x0087b020` at `0x0069dda0` after `push 0` for its
/// second argument at `0x0069dd9d`; `load_tileset` calls `Mountains::init` at
/// `0x0087b24e`. `place_all` follows at `0x0068c010`. The `jne 0x0069dda5`
/// short-circuit at `0x0069dd87` skips a reload when the tileset name already
/// matches; that is determinism-neutral here because the `MOUNTAINS` section is
/// a single tileset-independent block.
pub const MAP_LOAD_MAP_DATA_TILESET_CALL_VA: u32 = 0x0069_dda0;
pub const TILESET_LOAD_MOUNTAINS_INIT_CALL_VA: u32 = 0x0087_b24e;

/// Shipped file `Mountains::init` parses. Its name is string-table ordinal
/// 2768, not a `.text` literal, so it is named here from the parse it performs.
pub const MOUNTAIN_RANGE_SOURCE_FILE: &str = "effects_graphics.xml";
/// The 16-range cap in `Mountains::add_range` `0x008992b0`: the free-slot scan
/// at `0x008992f0`--`0x008992f9` compares against `0x10`.
pub const MOUNTAIN_RANGE_CAPACITY: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MountainRangeSourceError {
    Read { path: String, message: String },
    MissingMountainsSection,
    UnterminatedMountainsSection,
    MissingAreaAttribute { element: usize },
    UnsupportedArea { element: usize, area: String },
    /// More `MOUNTAIN` elements than `Mountains::add_range` can accept. Retail
    /// returns `-1` past the cap; refusing is better than silently modelling a
    /// truncation this shipped data never exercises.
    TooManyRanges { elements: usize },
}

/// Reconstruct the three `MountainsData` range lists from shipped data.
///
/// **[measured, this lane]** `ron-data/effects_graphics.xml` holds exactly one
/// `<MOUNTAINS>` block containing 16 `<MOUNTAIN>` elements: 7 `area="lg"`
/// (document positions 0..6), 8 `area="med"` (7..14) and 1 `area="sm"` (15).
///
/// **[measured, this lane]** `Mountains::randomize_mountains` `0x0089ca70` draws
/// only when `length - 1 > 0` — `lea eax,[esi-1]; test eax,eax; jg` at
/// `0x0089ca80`/`0x0089cac4`/`0x0089cb08` — and each draw is
/// `Random::get(0, 0xffff)` (`call 0x00a39d70`) on `ecx = [0x00c06184]`, i.e.
/// `GameAccess::game_random`, the main simulation LCG, followed by `cdq; idiv`
/// (signed remainder). So the shipped lengths `1 / 8 / 7` make the call consume
/// **exactly two** main-stream words, not three.
///
/// **[measured, this lane]** `LinkListBase<int,unsigned char>::add` `0x004a4af0`
/// writes `metric = 0` unconditionally (`0x004a4b00`), stores its argument as
/// the node payload (`0x004a4b04`), and makes the new node the head
/// (`0x004a4b37` / `0x004a4b4a`). `Mountains::add_range` `0x008992b0` returns
/// the free `ranges` slot index it filled (`mov eax, esi` `0x00899363`, store at
/// `0x0089936e`). Head-first order is therefore reverse insertion order with
/// every metric zero.
///
/// **[reported, not verified here]** That `Mountains::init` walks the
/// `<MOUNTAIN>` elements in document order and passes each `add_range` return
/// into the list its `area` selects. Only the payload *values and order* depend
/// on this; the list lengths and the two-draw RNG cost do not, and the payload
/// is first consumed by `Mountains::get_range` at `0x006a82c5`, at or after the
/// `Mountains::add_mountain` boundary.
pub fn resolve_mountain_ranges(
    effects_graphics_xml: &std::path::Path,
) -> Result<Mountains, MountainRangeSourceError> {
    let text = std::fs::read_to_string(effects_graphics_xml).map_err(|error| {
        MountainRangeSourceError::Read {
            path: effects_graphics_xml.display().to_string(),
            message: error.to_string(),
        }
    })?;
    resolve_mountain_ranges_xml(&text)
}

/// Same derivation over an already-read document.
pub fn resolve_mountain_ranges_xml(text: &str) -> Result<Mountains, MountainRangeSourceError> {
    let stripped = strip_xml_comments(text);
    let open = find_tag(&stripped, "MOUNTAINS", 0)
        .ok_or(MountainRangeSourceError::MissingMountainsSection)?;
    let body_start = stripped[open..]
        .find('>')
        .map(|offset| open + offset + 1)
        .ok_or(MountainRangeSourceError::UnterminatedMountainsSection)?;
    let body_end = stripped[body_start..]
        .find("</MOUNTAINS>")
        .map(|offset| body_start + offset)
        .ok_or(MountainRangeSourceError::UnterminatedMountainsSection)?;
    let body = &stripped[body_start..body_end];

    // Insertion order is document order; the list each range joins is chosen by
    // its `area` attribute. `sml` maps to the small list alongside `sm`.
    let mut per_list: [Vec<i32>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    let mut element = 0usize;
    let mut search = 0usize;
    while let Some(open) = find_tag(body, "MOUNTAIN", search) {
        let end = body[open..]
            .find('>')
            .map(|offset| open + offset)
            .ok_or(MountainRangeSourceError::UnterminatedMountainsSection)?;
        let tag = &body[open..end];
        search = end + 1;
        let area = attribute(tag, "area")
            .ok_or(MountainRangeSourceError::MissingAreaAttribute { element })?;
        let slot = match area {
            "sm" | "sml" => 0usize,
            "med" => 1,
            "lg" => 2,
            other => {
                return Err(MountainRangeSourceError::UnsupportedArea {
                    element,
                    area: other.to_owned(),
                })
            }
        };
        if element >= MOUNTAIN_RANGE_CAPACITY {
            return Err(MountainRangeSourceError::TooManyRanges {
                elements: element + 1,
            });
        }
        per_list[slot].push(element as i32);
        element += 1;
    }

    let mut lists = per_list.map(|mut indices| {
        indices.reverse();
        MountainRangeList::new(
            indices
                .into_iter()
                .map(|data| MountainRangeEntry::new(data, 0))
                .collect(),
        )
    });
    let large_ranges = lists[2].clone();
    let medium_ranges = lists[1].clone();
    let small_ranges = std::mem::take(&mut lists[0]);
    Ok(Mountains {
        small_ranges,
        medium_ranges,
        large_ranges,
    })
}

fn strip_xml_comments(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut at = 0usize;
    while let Some(relative) = text[at..].find("<!--") {
        let start = at + relative;
        output.push_str(&text[at..start]);
        match text[start + 4..].find("-->") {
            Some(offset) => at = start + 4 + offset + 3,
            None => return output,
        }
    }
    output.push_str(&text[at..]);
    output
}

/// Find `<NAME` where the next byte ends the tag name, so `<MOUNTAINS` is never
/// mistaken for `<MOUNTAIN`.
fn find_tag(text: &str, name: &str, from: usize) -> Option<usize> {
    let needle = format!("<{name}");
    let mut search = from;
    while let Some(relative) = text[search..].find(&needle) {
        let start = search + relative;
        let after = start + needle.len();
        match text.as_bytes().get(after) {
            Some(byte) if byte.is_ascii_whitespace() || *byte == b'>' || *byte == b'/' => {
                return Some(start)
            }
            None => return None,
            _ => search = after,
        }
    }
    None
}

fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=\"");
    let start = tag.find(&needle)? + needle.len();
    let end = start + tag[start..].find('"')?;
    Some(&tag[start..end])
}

/// The helping globals exactly as `place_all` initialises them, before its own
/// first read of any of them.
///
/// These are **not** absent producers. Instruction evidence, in execution order:
///
/// - `num_players` `0x00cae70c` is `world.start_x.length`, written at
///   `0x006a72eb`--`0x006a72fb` from `[GameAccess::world + 0x84]`;
/// - `lowest_player[5]` `0x00cbe440` is zeroed at `0x006a75ab` (`movaps`,
///   indices 0..3) and `0x006a75b4` (index 4), and `place_all` never reads it —
///   it is produced and consumed inside `place_region_group` `0x006a2f60`;
/// - `player_scores[8][5]` `0x00cbe480` has rows `[0, num_players)` zeroed at
///   `0x006a75ee`--`0x006a760c`. Rows at or above `num_players` keep values from
///   a previous game and are never read, so zero is correct over the whole read
///   domain and the residue is unobservable;
/// - `is_helping` `0x00cae708` is set to `0` at `0x006a7618`, before its first
///   read at `0x006a82d0`. It is a placement-loop flag recomputed from
///   `help_lowest_index[5]` `0x00cbe460`, never a game-setup option.
pub fn initial_region_helping_state(
    world: &don_sim::systems::map_terrain::World,
) -> RegionHelpingState {
    RegionHelpingState {
        is_helping: false,
        num_players: world.start_x.items.len(),
        lowest_player: [0; 5],
        scores: [[0; 5]; 8],
    }
}

/// How the driver treats `World::set_oil_at`'s Good-object side effect.
///
/// `TerrainGroup::drop_tile` and the player growth tail call
/// `World::set_oil_at` `0x006b2a10`, which sets or clears `WData::OIL` — a
/// `world`-channel byte the shipped `don-sim` setter already writes — and
/// additionally closes or creates a `Good` object, which belongs to the
/// unmodelled `goods` channel.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OilGoodPolicy {
    /// Treat the Good effect as an absent producer and stop there. This is the
    /// default because it assumes nothing at all.
    Stop,
    /// Continue past the call, recording every request.
    ///
    /// This supplies no value: the `don-sim` resolution for this branch carries
    /// only an echo of the request, is void, and consumes no RNG
    /// (`crates/don-sim/src/systems/terrain_drop_tile.rs`, the
    /// `OilGoodsApplied` and `OilGoodMutation` doc comments). Choosing it
    /// asserts exactly one thing: that the `Good` create/close is invisible to
    /// the `world` channel and to the main RNG. It never creates the `Good`, so
    /// the `goods` channel stays unproduced either way.
    ContinueRecordingGoodEffects,
}

/// Producers `.rcx` does not carry, supplied by the caller when it has them.
///
/// `None` means the reconstruction has no lawful source, and the driver stops
/// at the exact primitive that first needs it. It never substitutes a default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceAllAdvanceFacts {
    /// The `Mountains` singleton (`0x00e85f60`) at `place_all` entry. Its three
    /// range lists decide how many draws `Mountains::randomize_mountains`
    /// `0x0089ca70` takes from the main stream, so a guess would corrupt every
    /// later draw in the call.
    pub mountains: Option<Mountains>,
    /// `is_helping` `0x00cae708` plus `lowest_player`/`player_scores`, read by
    /// `TerrainGroup::place_region_group` `0x006a2f60`.
    pub helping: Option<RegionHelpingState>,
    /// `TileSetGroupData` fields consumed by `TerrainGroups::add_doobers`.
    pub doober_rules: Option<DooberTilesetRules>,
    /// The localized reporting tail's `num_players` / `player_scores` inputs.
    pub reporting: Option<PlacementReportingInputs>,
    /// `place_all`'s first argument. It gates only a progress-display host
    /// event and reaches neither World nor RNG; `progress_is_presentation_only`
    /// in `crates/don-replay/tests/place_all_advance.rs` pins that.
    pub progress: i32,
    pub oil_good_policy: OilGoodPolicy,
}

impl Default for OilGoodPolicy {
    fn default() -> Self {
        Self::Stop
    }
}

impl Default for PlaceAllAdvanceFacts {
    /// Every producer absent, `progress` at its derived retail-start value, and
    /// the oil policy that assumes nothing.
    fn default() -> Self {
        Self {
            mountains: None,
            helping: None,
            doober_rules: None,
            reporting: None,
            progress: RETAIL_GAME_START_PROGRESS,
            oil_good_policy: OilGoodPolicy::Stop,
        }
    }
}

/// First retail primitive the reconstruction could not execute, or completion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaceAllStop {
    /// `Mountains::randomize_mountains` `0x0089ca70`, called at `0x006a7330`,
    /// the first call in `place_all`. It draws once per range list of length
    /// greater than one, so the lists are an RNG-order dependency, not decoration.
    MountainRangeLists,
    /// `TerrainGroup::place_region_group` `0x006a2f60` reads `is_helping` and
    /// the helping score table before it selects a candidate.
    RegionHelpingGlobals { group_index: usize },
    /// `Mountains::add_mountain` `0x0089c2e0` returns a `Liberr` which decides
    /// whether the candidate is accepted.
    MountainsAddMountain {
        group_index: usize,
        world_x: i32,
        world_y: i32,
        template: i32,
    },
    /// `Cliffs::verify_defensive_position` `0x008a8680`.
    CliffsVerifyDefensivePosition {
        group_index: usize,
        world_x: i32,
        world_y: i32,
    },
    /// `Cliffs::position_cliff` `0x008a8bc0`. It returns a placement result and
    /// can itself draw from the main stream.
    CliffsPositionCliff {
        group_index: usize,
        world_x: i32,
        world_y: i32,
    },
    /// `World::set_oil_at` `0x006b2a10` under [`OilGoodPolicy::Stop`].
    OilGoodMutation {
        group_index: usize,
        world_x: i32,
        world_y: i32,
        enabled: bool,
    },
    /// The player growth block at `0x006a4cf0` returned a state the composed
    /// `don-sim` transaction does not continue from.
    PlayerGroupGrowthKernel {
        group_index: usize,
        clump_index: usize,
        player_index: usize,
    },
    /// `place_region_group` returned exactly and the enclosing per-clump
    /// continuation is the next unrecovered row.
    RegionGroupReturnControl {
        group_index: usize,
        return_value: i32,
    },
    /// Every selected group completed; `TerrainGroups::add_doobers` `0x006a1540`
    /// needs the selected tileset's `TileSetGroupData`.
    DooberTilesetRules,
    /// Both doober passes ran; `TerrainGroups::treeify_mountains` `0x006a1cc0`
    /// needs the map-style gate.
    TreeifyMountains,
    /// Treeification finished; the reporting tail at `0x006a8f12` needs the
    /// player-score table.
    PostPlacementReporting,
    /// `place_all` returned at `0x006a937d`.
    Completed { return_value: i32 },
}

impl PlaceAllStop {
    /// Stable report vocabulary, in the same style as the continent boundaries.
    pub fn name(&self) -> &'static str {
        match self {
            Self::MountainRangeLists => "place_all_mountain_range_lists",
            Self::RegionHelpingGlobals { .. } => "place_all_region_helping_globals",
            Self::MountainsAddMountain { .. } => "place_all_mountains_add_mountain",
            Self::CliffsVerifyDefensivePosition { .. } => "place_all_cliffs_verify_defensive",
            Self::CliffsPositionCliff { .. } => "place_all_cliffs_position_cliff",
            Self::OilGoodMutation { .. } => "place_all_world_set_oil_at",
            Self::PlayerGroupGrowthKernel { .. } => "place_all_player_growth_kernel",
            Self::RegionGroupReturnControl { .. } => "place_all_region_return_control",
            Self::DooberTilesetRules => "place_all_add_doobers",
            Self::TreeifyMountains => "place_all_treeify_mountains",
            Self::PostPlacementReporting => "place_all_reporting_tail",
            Self::Completed { .. } => "place_all_complete",
        }
    }

    /// Exact retail address of the primitive named by this stop.
    pub fn primitive_va(&self) -> u32 {
        match self {
            Self::MountainRangeLists => MOUNTAINS_RANDOMIZE_MOUNTAINS_VA,
            Self::RegionHelpingGlobals { .. } => REGION_HELPING_IS_HELPING_VA,
            Self::MountainsAddMountain { .. } => MOUNTAINS_ADD_MOUNTAIN_VA,
            Self::CliffsVerifyDefensivePosition { .. } => CLIFFS_VERIFY_DEFENSIVE_POSITION_VA,
            Self::CliffsPositionCliff { .. } => CLIFFS_POSITION_CLIFF_VA,
            Self::OilGoodMutation { .. } => WORLD_SET_OIL_AT_VA,
            Self::PlayerGroupGrowthKernel { .. } => TERRAIN_GROUP_PLACE_PLAYER_GROUP_VA,
            Self::RegionGroupReturnControl { .. } => TERRAIN_GROUP_PLACE_REGION_GROUP_VA,
            Self::DooberTilesetRules => TERRAIN_GROUPS_ADD_DOOBERS_VA,
            Self::TreeifyMountains => TERRAIN_GROUPS_TREEIFY_MOUNTAINS_VA,
            Self::PostPlacementReporting => TERRAIN_GROUPS_REPORTING_TAIL_VA,
            Self::Completed { .. } => TERRAIN_GROUPS_PLACE_ALL_RETURN_VA,
        }
    }

    /// The group whose arm reached the stop, when the stop is inside one.
    pub const fn group_index(&self) -> Option<usize> {
        match self {
            Self::RegionHelpingGlobals { group_index }
            | Self::MountainsAddMountain { group_index, .. }
            | Self::CliffsVerifyDefensivePosition { group_index, .. }
            | Self::CliffsPositionCliff { group_index, .. }
            | Self::OilGoodMutation { group_index, .. }
            | Self::PlayerGroupGrowthKernel { group_index, .. }
            | Self::RegionGroupReturnControl { group_index, .. } => Some(*group_index),
            _ => None,
        }
    }
}

/// One selected group row as `place_all`'s own selection pass produced it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SelectedGroupRow {
    pub group_index: usize,
    pub group_type: i32,
    pub pattern: i32,
    pub clumps: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceAllAdvanceReceipt {
    pub entry_va: u32,
    pub map_style: u8,
    /// `place_all`'s first argument as supplied. Recorded because it is an
    /// input, not because it can change the result: it gates one progress
    /// host event and reaches neither World nor RNG.
    pub progress: i32,
    /// Main-RNG word handed to `place_all`; unchanged by a stop.
    pub random_state_at_entry: i32,
    /// Lengths of the three derived `MountainsData` range lists, small/medium/
    /// large, and the number of main-stream words `randomize_mountains` took.
    /// `None` when the shipped `MOUNTAINS` section was unavailable.
    pub mountain_range_lengths: Option<[usize; 3]>,
    pub mountain_randomize_draws: Option<u8>,
    pub catalog_groups: usize,
    pub selected_groups: Vec<SelectedGroupRow>,
    /// Group indices whose complete placement arm reached the common dispatcher
    /// edge at `0x006a8ee5` during this attempt.
    pub completed_groups: Vec<usize>,
    /// Ordered `World::set_oil_at` calls crossed under
    /// [`OilGoodPolicy::ContinueRecordingGoodEffects`]; always empty otherwise.
    pub crossed_oil_good_effects: Vec<DropTileExternalRequest>,
    pub oil_good_policy: OilGoodPolicy,
    pub stop: PlaceAllStop,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaceAllAdvanceError {
    Blocked {
        boundary: InitialItemBoundary,
    },
    MissingFillFertileReceipt,
    MissingFertilityEvidence,
    MissingStaticStyle,
    ContinentDidNotReachCommonTail {
        next_va: Option<u32>,
    },
    Catalog(crate::place_all_facts::TerrainGroupCatalogError),
    /// The shipped transaction refused for a reason other than a missing
    /// caller-supplied row. This is a real defect, not a boundary.
    PlaceAll(Box<PlaceAllError>),
    /// Each attempt supplies exactly one more caller row than the last, so the
    /// driver must converge in at most one row per group plus one per crossed
    /// external. Exceeding the cap means the shipped dispatcher repeated a
    /// boundary without consuming the row that answers it.
    DidNotConverge { attempts: usize },
}

/// One row per selected group, plus one per crossed `World::set_oil_at`, with
/// generous slack. `OilGoodPolicy::Stop` converges in `groups.len() + 1`.
const MAX_ATTEMPTS: usize = 200_000;

/// Run `TerrainGroups::place_all` from the replay-owned World and RNG and
/// report the first primitive that cannot be executed.
///
/// Nothing is committed. `map` is read-only here on purpose: the transaction is
/// fail-closed in `don-sim`, and a partially generated world would be worse than
/// none, because the `world` channel would then hash bytes that no source backs.
pub fn advance_place_all_boundary(
    plan: &InitialItemReconstruction,
    map: &InitialWorld,
    continent: &ContinentReceipt,
    facts: &PlaceAllAdvanceFacts,
) -> Result<PlaceAllAdvanceReceipt, PlaceAllAdvanceError> {
    match plan.boundary {
        InitialItemBoundary::MapTerrainGroupsPlaceAllUnavailable { next_va }
            if next_va == TERRAIN_GROUPS_PLACE_ALL_VA => {}
        InitialItemBoundary::MapTerrainGroupsPlaceAllPrimitiveUnavailable { .. } => {}
        boundary => return Err(PlaceAllAdvanceError::Blocked { boundary }),
    }
    if plan.fill_fertile.is_none() {
        return Err(PlaceAllAdvanceError::MissingFillFertileReceipt);
    }
    let fertility = plan
        .fertility
        .as_ref()
        .ok_or(PlaceAllAdvanceError::MissingFertilityEvidence)?;
    let style = plan
        .style
        .as_ref()
        .ok_or(PlaceAllAdvanceError::MissingStaticStyle)?;
    let next_va = match &continent.stop {
        ContinentStop::HookComplete { next_va } => Some(*next_va),
        _ => None,
    };
    if next_va != Some(REGIONS_CLEAR_ALL_VA) {
        return Err(PlaceAllAdvanceError::ContinentDidNotReachCommonTail { next_va });
    }

    let catalog = crate::place_all_facts::resolve_terrain_group_catalog(style, &map.world)
        .map_err(PlaceAllAdvanceError::Catalog)?;
    let mut base = fertility.terrain_groups_input();
    base.groups = catalog.groups.clone();

    let mut receipt = PlaceAllAdvanceReceipt {
        entry_va: TERRAIN_GROUPS_PLACE_ALL_VA,
        map_style: plan.inputs.map_style,
        progress: facts.progress,
        random_state_at_entry: continent.rng_final,
        mountain_range_lengths: None,
        mountain_randomize_draws: None,
        catalog_groups: catalog.groups.len(),
        selected_groups: Vec::new(),
        completed_groups: Vec::new(),
        crossed_oil_good_effects: Vec::new(),
        oil_good_policy: facts.oil_good_policy,
        stop: PlaceAllStop::MountainRangeLists,
    };

    // `Mountains::randomize_mountains` is the first call in the body, so an
    // unavailable range list is the boundary and nothing after it is executed.
    let Some(mountains) = facts.mountains.as_ref() else {
        return Ok(receipt);
    };
    receipt.mountain_range_lengths = Some([
        mountains.small_ranges.len(),
        mountains.medium_ranges.len(),
        mountains.large_ranges.len(),
    ]);
    receipt.mountain_randomize_draws = Some(
        mountains
            .clone()
            .randomize_mountains(&mut Random::new(continent.rng_final))
            .draws,
    );

    let mut inputs: Vec<PlaceAllGroupInput> = Vec::new();
    // Each iteration re-runs the deterministic transaction from entry with one
    // more caller-supplied row than the last. A row asserts nothing on its own:
    // the shipped kernel decides whether it needs an external resolution.
    for attempt in 0..=MAX_ATTEMPTS {
        if attempt == MAX_ATTEMPTS {
            return Err(PlaceAllAdvanceError::DidNotConverge { attempts: attempt });
        }
        let mut groups: TerrainGroups = base.clone();
        let mut world = map.world.clone();
        let mut random = Random::new(continent.rng_final);
        let mut mountains = mountains.clone();
        // `Map::make` pushes literal one for `place_players` immediately before
        // the call, at 0x0068c007--0x0068c010. The wrapper is chosen by which
        // late-stage facts exist, so an absent one becomes its own stop rather
        // than a substituted default.
        let host = |_: PlaceAllHostEvent| {};
        let result = match (facts.doober_rules, facts.reporting) {
            (Some(rules), Some(reporting)) => groups.place_all_with_group_reporting_inputs(
                &mut world,
                &map.generation_regions,
                &mut random,
                &mut mountains,
                facts.progress,
                1,
                facts.helping,
                &inputs,
                rules,
                plan.inputs.map_style,
                reporting,
                host,
            ),
            (Some(rules), None) => groups.place_all_with_group_treeify_inputs(
                &mut world,
                &map.generation_regions,
                &mut random,
                &mut mountains,
                facts.progress,
                1,
                facts.helping,
                &inputs,
                rules,
                plan.inputs.map_style,
                host,
            ),
            (None, _) => groups.place_all_with_group_inputs(
                &mut world,
                &map.generation_regions,
                &mut random,
                &mut mountains,
                facts.progress,
                1,
                facts.helping,
                &inputs,
                host,
            ),
        };
        let (preview, boundary) = match result {
            Ok(return_value) => {
                receipt.stop = PlaceAllStop::Completed { return_value };
                return Ok(receipt);
            }
            Err(PlaceAllError::GameplayPlacementUnavailable { preview, boundary }) => {
                (preview, boundary)
            }
            Err(PlaceAllError::InvalidRegionPattern(
                don_sim::systems::terrain_region_patterns::RegionPatternError::MissingHelpingState {
                    ..
                },
            )) => {
                receipt.stop = PlaceAllStop::RegionHelpingGlobals {
                    group_index: inputs.last().map_or(0, PlaceAllGroupInput::group_index),
                };
                return Ok(receipt);
            }
            Err(other) => return Err(PlaceAllAdvanceError::PlaceAll(Box::new(other))),
        };

        receipt.selected_groups = preview
            .group_selection
            .groups
            .iter()
            .enumerate()
            .filter(|(_, row)| row.selected)
            .map(|(group_index, row)| SelectedGroupRow {
                group_index,
                group_type: catalog.groups[group_index].group_type,
                pattern: catalog.groups[group_index].pattern,
                clumps: row.clumps,
            })
            .collect();
        receipt
            .completed_groups
            .clone_from(&preview.completed_placement_groups);

        match boundary {
            TerrainPlacementBoundary::PlayerRosterAndPlacementKernel { group_index } => {
                inputs.push(PlaceAllGroupInput::Player {
                    group_index,
                    externals: Vec::new(),
                });
            }
            TerrainPlacementBoundary::UnitTypeCatalogAndRegionPlacementKernel {
                group_index,
                ..
            } => {
                inputs.push(PlaceAllGroupInput::Region {
                    group_index,
                    externals: Vec::new(),
                });
            }
            TerrainPlacementBoundary::PlayerGroupExternalSubsystem {
                group_index,
                request,
                ..
            } => {
                let Some(next) =
                    player_continuation(group_index, request, facts.oil_good_policy, &mut receipt)
                else {
                    return Ok(receipt);
                };
                let Some(PlaceAllGroupInput::Player { externals, .. }) = inputs
                    .iter_mut()
                    .find(|input| input.group_index() == group_index)
                else {
                    receipt.stop = PlaceAllStop::PlayerGroupGrowthKernel {
                        group_index,
                        clump_index: 0,
                        player_index: 0,
                    };
                    return Ok(receipt);
                };
                externals.push(next);
            }
            TerrainPlacementBoundary::RegionGroupDropTileExternalSubsystem { request } => {
                let group_index = inputs.last().map_or(0, PlaceAllGroupInput::group_index);
                let Some(next) =
                    region_continuation(group_index, request, facts.oil_good_policy, &mut receipt)
                else {
                    return Ok(receipt);
                };
                let Some(PlaceAllGroupInput::Region { externals, .. }) = inputs.last_mut() else {
                    return Ok(receipt);
                };
                externals.push(next);
            }
            TerrainPlacementBoundary::PlayerGroupGrowthKernel {
                group_index,
                clump_index,
                player_index,
            } => {
                receipt.stop = PlaceAllStop::PlayerGroupGrowthKernel {
                    group_index,
                    clump_index,
                    player_index,
                };
                return Ok(receipt);
            }
            TerrainPlacementBoundary::RegionGroupReturnControl {
                group_index,
                return_value,
            } => {
                receipt.stop = PlaceAllStop::RegionGroupReturnControl {
                    group_index,
                    return_value,
                };
                return Ok(receipt);
            }
            TerrainPlacementBoundary::AddDoobers => {
                receipt.stop = PlaceAllStop::DooberTilesetRules;
                return Ok(receipt);
            }
            TerrainPlacementBoundary::TreeifyMountainsMapStyle => {
                receipt.stop = PlaceAllStop::TreeifyMountains;
                return Ok(receipt);
            }
            TerrainPlacementBoundary::PostPlacementReporting => {
                receipt.stop = PlaceAllStop::PostPlacementReporting;
                return Ok(receipt);
            }
        }
    }
    Err(PlaceAllAdvanceError::DidNotConverge {
        attempts: MAX_ATTEMPTS,
    })
}

/// Decide whether a player-arm external can be crossed without inventing a
/// value, recording the stop when it cannot.
fn player_continuation(
    group_index: usize,
    request: PlayerGroupExternalRequest,
    policy: OilGoodPolicy,
    receipt: &mut PlaceAllAdvanceReceipt,
) -> Option<PlayerGroupExternalResolution> {
    match request {
        PlayerGroupExternalRequest::MountainsAddMountain {
            template,
            world_x,
            world_y,
            ..
        } => {
            receipt.stop = PlaceAllStop::MountainsAddMountain {
                group_index,
                world_x,
                world_y,
                template,
            };
            None
        }
        PlayerGroupExternalRequest::CliffsVerifyDefensivePosition {
            world_x, world_y, ..
        } => {
            receipt.stop = PlaceAllStop::CliffsVerifyDefensivePosition {
                group_index,
                world_x,
                world_y,
            };
            None
        }
        PlayerGroupExternalRequest::CliffsPositionCliff {
            world_x, world_y, ..
        } => {
            receipt.stop = PlaceAllStop::CliffsPositionCliff {
                group_index,
                world_x,
                world_y,
            };
            None
        }
        PlayerGroupExternalRequest::OilGoodMutation {
            world_x,
            world_y,
            enabled,
            good_type,
            coord_x,
            coord_y,
        } => match policy {
            OilGoodPolicy::Stop => {
                receipt.stop = PlaceAllStop::OilGoodMutation {
                    group_index,
                    world_x,
                    world_y,
                    enabled,
                };
                None
            }
            OilGoodPolicy::ContinueRecordingGoodEffects => {
                receipt
                    .crossed_oil_good_effects
                    .push(DropTileExternalRequest::OilGoodMutation {
                        world_x,
                        world_y,
                        enabled,
                        good_type,
                        coord_x,
                        coord_y,
                    });
                Some(PlayerGroupExternalResolution::OilGoodsApplied { request })
            }
        },
    }
}

fn region_continuation(
    group_index: usize,
    request: DropTileExternalRequest,
    policy: OilGoodPolicy,
    receipt: &mut PlaceAllAdvanceReceipt,
) -> Option<DropTileExternalResolution> {
    match request {
        DropTileExternalRequest::MountainsAddMountain {
            template,
            world_x,
            world_y,
            ..
        } => {
            receipt.stop = PlaceAllStop::MountainsAddMountain {
                group_index,
                world_x,
                world_y,
                template,
            };
            None
        }
        DropTileExternalRequest::CliffsPositionCliff {
            world_x, world_y, ..
        } => {
            receipt.stop = PlaceAllStop::CliffsPositionCliff {
                group_index,
                world_x,
                world_y,
            };
            None
        }
        DropTileExternalRequest::OilGoodMutation {
            world_x,
            world_y,
            enabled,
            ..
        } => match policy {
            OilGoodPolicy::Stop => {
                receipt.stop = PlaceAllStop::OilGoodMutation {
                    group_index,
                    world_x,
                    world_y,
                    enabled,
                };
                None
            }
            OilGoodPolicy::ContinueRecordingGoodEffects => {
                receipt.crossed_oil_good_effects.push(request);
                Some(DropTileExternalResolution::OilGoodsApplied { request })
            }
        },
    }
}
