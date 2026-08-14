//! Common terrain-group world-generation stages.
//!
//! The shipped PDB fixes `TerrainGroups` at 280 bytes, `TerrainGroup` at 156,
//! `Fractal` at 120, and identifies the two common calls made by `Map::make`:
//! `TerrainGroups::fill_fertile` (`0x006a6f90`) followed by
//! `TerrainGroups::place_all` (`0x006a70d0`).  Behavior here is checked against
//! Ghidra and the retail instruction stream, not inferred from terrain names.
//!
//! `fill_fertile` is complete for a supplied `Fractal::frac` byte plane and its
//! supplied partition thresholds.  The plane is produced upstream by
//! `TerrainGroups::init_tileset_data`; this module deliberately does not invent
//! a fractal generator or tileset frequencies.

use super::map_terrain::{land, tflag, wflag, World};
use super::mountain_add_runtime::{AddMountainCall, MountainAddRuntime};
use super::mountains::{MountainRandomizeReceipt, Mountains};
use super::regions::{Regions, WCoordList};
use super::terrain_doobers::{
    plan_bush_fringe_with_host, plan_mountain_rock_fringe_with_host, validate_bush_fringe_inputs,
    validate_mountain_rock_fringe_inputs, BushDooberPlacement, BushFringeError, BushFringeReceipt,
    DooberTilesetRules, MountainRockDooberPlacement, MountainRockFringeError,
    MountainRockFringeReceipt,
};
use super::terrain_drop_tile::{
    DropTileExternalRequest, DropTileExternalResolution, DropTileReceipt,
};
use super::terrain_player_group::{
    PlacePlayerGroupCall, PlacePlayerGroupError, PlacePlayerGroupOutcome, PlacePlayerGroupReceipt,
    PlayerGroupExternalRequest, PlayerGroupExternalResolution, PlayerMountainResolution,
    PlayerMountainResolver,
};
use super::terrain_player_mountain_retry::{
    PlayerMountainTemplateRetryError, PlayerMountainTemplateRetryReceipt,
};
use super::terrain_region_continuation::{
    build_world_receipt, MountainOwnerExecutionReceipt, PlaceRegionGroupError,
    PlaceRegionGroupOutcome, PlaceRegionGroupOwnerReceipt, PlaceRegionGroupOwners,
    PlaceRegionGroupReceipt,
};
use super::terrain_region_patterns::{
    RegionPatternError, RegionPatternOutcome, RegionPatternReceipt,
};
use super::terrain_region_placement::{
    PlaceRegionGroupCall, PlaceRegionGroupPrefixReceipt, RegionHelpingState,
};
use super::world_oil_goods::{apply_world_set_oil_at, OilGoodMutation, OilGoodMutationError};
use crate::rng::Random;

/// `TerrainGroup` (PDB size 156), excluding native vbase/pointer representation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TerrainGroup {
    /// PDB field `type` at `+0x04`.
    pub group_type: i32,
    pub chance: i32,
    pub grouping: i32,
    pub min_clumps: i32,
    pub max_clumps: i32,
    pub pattern: i32,
    pub min_size: i32,
    pub max_size: i32,
    pub start_min: i32,
    pub start_max: i32,
    pub forest_space: i32,
    pub mount_space: i32,
    pub rock_space: i32,
    pub coast_space: i32,
    pub cent_min: i32,
    pub cent_max: i32,
    pub edge_min: i32,
    pub edge_max: i32,
    pub corner_min: i32,
    pub corner_max: i32,
    pub min_oil: i32,
    pub max_oil: i32,
    pub cliff_face: i32,
    pub touching_mountain: i32,
    pub placed: Vec<i32>,
    pub tiles: WCoordList,
}

/// The `Fractal::frac : ObjectArray<Array<unsigned char>>` values consumed by
/// `fill_fertile`.  The outer index is X and each inner array is indexed by Y.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FertilityFractal {
    pub columns: Vec<Vec<u8>>,
}

/// Deterministic inputs owned by the retail `TerrainGroups` singleton.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TerrainGroups {
    pub groups: Vec<TerrainGroup>,
    pub subtype_freqs: [Vec<i32>; 3],
    pub console_info: i32,
    pub fractal: FertilityFractal,
    pub partitions: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FillFertileError {
    InvalidWorldShape {
        xs: i32,
        ys: i32,
        size: i32,
        wdata_len: usize,
    },
    MissingFractalColumn {
        x: i32,
        required_columns: i32,
        actual_columns: usize,
    },
    ShortFractalColumn {
        x: i32,
        required_rows: i32,
        actual_rows: usize,
    },
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct FillFertileReceipt {
    /// Number of `WData.land == 0` cells written and logged by retail.
    pub fertile_cells: i32,
}

/// One row of the two zero-filled native temporary arrays used by `place_all`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct TerrainGroupSelection {
    pub selected: bool,
    /// Zero for rejected groups; otherwise the fixed or randomly selected clumps.
    pub clumps: i32,
}

/// Deterministic result through `0x006a7615`, immediately before the placement
/// pass resets its group index and reaches `NetDaemon::process_all`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerrainGroupSelectionReceipt {
    pub groups: Vec<TerrainGroupSelection>,
    /// Accumulators at `0x00cbe460` before retail normalizes them in place.
    pub raw_clumps_by_type: [i32; 5],
    /// The same five globals after `0x006a75c0`--`0x006a75e9`.
    pub normalized_clumps_by_type: [i32; 5],
    pub chance_draws: u32,
    pub clump_draws: u32,
    pub rng_state_after: i32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TerrainGroupSelectionError {
    /// Native indexes a five-element table with `TerrainGroup::type - 4`.
    UnsupportedGroupType { group_index: usize, group_type: i32 },
    /// Native's inclusive `(max - min) + 1` divisor must fit positive `i32`.
    InvalidClumpSpan {
        group_index: usize,
        min_clumps: i32,
        max_clumps: i32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceAllPreviewReceipt {
    pub mountain_randomization: MountainRandomizeReceipt,
    pub group_selection: TerrainGroupSelectionReceipt,
    pub placement_preparation: TerrainPlacementPreparationReceipt,
    /// Present when the caller supplied the `TileSetGroupData` doober rules and
    /// placement reached the common post-group `add_doobers` stage.
    pub bush_fringe: Option<BushFringeReceipt>,
    pub mountain_rock_fringe: Option<MountainRockFringeReceipt>,
    pub treeify_mountains: Option<TreeifyMountainsGateReceipt>,
    /// Present when a resolved unit-catalog/region-selection receipt advances
    /// the selected pattern arm through `place_region_group`'s entry search.
    pub region_group_prefix: Option<PlaceRegionGroupPrefixReceipt>,
    /// Present after the selected candidate has executed the complete
    /// `TerrainGroup::drop_tile` branch on preview state.
    pub region_group_drop: Option<DropTileReceipt>,
    /// Complete retry/helping/growth/cleanup/oil continuation of the selected
    /// `place_region_group` call.
    pub region_group_continuation: Option<PlaceRegionGroupReceipt>,
    /// Exact pattern-1/2/3 eligible-region and clump loop.
    pub region_pattern: Option<RegionPatternReceipt>,
    /// One receipt per attempted pattern-1/2/3 group in native group order.
    pub region_pattern_dispatches: Vec<RegionPatternGroupReceipt>,
    /// Exact mountain/oil owner leaves executed on staged `place_all` state.
    /// The caller-owned transaction remains uncommitted on every boundary.
    pub owner_receipts: Vec<PlaceAllOwnerExecutionReceipt>,
    /// Exact pattern-0 player/start-ring calls, including locally closed growth
    /// and mountain-template retry continuations.
    pub player_group_prefix: Option<Vec<PlacePlayerGroupReceipt>>,
    /// Complete type-5 mountain-template cursor cycles reached after an initial
    /// player placement returned zero.
    pub player_group_mountain_retries: Vec<PlayerMountainTemplateRetryReceipt>,
    pub player_group_host_events: Vec<PlaceAllHostEvent>,
    pub player_group_placed_after: Vec<i32>,
    pub player_group_formation_x: Vec<i32>,
    pub player_group_formation_y: Vec<i32>,
    /// One receipt per fully attempted pattern-0 group in native group order.
    /// Unlike the legacy flattened fields above, group-local formation and
    /// `placed` arrays remain separated across the `0x006a8ee5` cleanup edge.
    pub player_group_dispatches: Vec<PlayerPatternGroupReceipt>,
    /// Selected placement arms that reached their native group-local cleanup
    /// and `group_index++` edge at `0x006a8ee5`.
    pub completed_placement_groups: Vec<usize>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PlayerPatternGroupOutcome {
    Complete,
    ExternalResolutionRequired {
        clump_index: usize,
        player_index: usize,
        request: PlayerGroupExternalRequest,
    },
    GrowthKernel {
        clump_index: usize,
        player_index: usize,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerPatternGroupReceipt {
    pub group_index: usize,
    pub calls: Vec<PlacePlayerGroupReceipt>,
    pub mountain_retries: Vec<PlayerMountainTemplateRetryReceipt>,
    pub host_events: Vec<PlaceAllHostEvent>,
    pub placed_after: Vec<i32>,
    pub formation_x_after: Vec<i32>,
    pub formation_y_after: Vec<i32>,
    pub external_resolutions_consumed: usize,
    pub outcome: PlayerPatternGroupOutcome,
    pub rng_state_after: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionPatternGroupReceipt {
    pub group_index: usize,
    pub pattern: RegionPatternReceipt,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PlaceAllOwnerSource {
    Player {
        clump_index: usize,
        player_index: usize,
    },
    Region {
        call_index: usize,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceAllOwnerExecutionReceipt {
    pub group_index: usize,
    pub source: PlaceAllOwnerSource,
    pub execution: PlaceRegionGroupOwnerReceipt,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TerrainGroupInputKind {
    Player,
    Region,
}

/// One selected-group input row for the heterogeneous `place_all` adapter.
/// Rows are ordered by native group index; each row owns only the external
/// effects that its selected placement arm can consume.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaceAllGroupInput {
    Player {
        group_index: usize,
        externals: Vec<PlayerGroupExternalResolution>,
    },
    Region {
        group_index: usize,
        externals: Vec<DropTileExternalResolution>,
    },
}

impl PlaceAllGroupInput {
    pub const fn group_index(&self) -> usize {
        match self {
            Self::Player { group_index, .. } | Self::Region { group_index, .. } => *group_index,
        }
    }

    pub const fn kind(&self) -> TerrainGroupInputKind {
        match self {
            Self::Player { .. } => TerrainGroupInputKind::Player,
            Self::Region { .. } => TerrainGroupInputKind::Region,
        }
    }
}

/// Outputs of the still-upstream unit-catalog and region-selection block in
/// `place_all`, sufficient to enter `TerrainGroup::place_region_group` exactly.
///
/// The block can consume RNG while selecting/rotating catalog entries. Supplying
/// its resulting state explicitly prevents the composed adapter from pretending
/// those draws do not exist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedRegionGroupPlacement {
    pub group_index: usize,
    pub clump_index: usize,
    pub region_id: usize,
    pub land_subtype: i32,
    pub rng_state_at_call: i32,
    pub helping: Option<RegionHelpingState>,
    /// Ordered Mountains/Cliffs/Good receipts consumed by candidate retries,
    /// cleanup, and the type-6 oil tail.
    pub drop_tile_externals: Vec<DropTileExternalResolution>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TreeifyOpenNeighbor {
    East,
    South,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TreeifyMutationKind {
    /// The cell contains mountain TCoords but lacks the WData mountain class.
    Forest,
    /// The cell has the WData mountain class and the named forward neighbor has
    /// no mountain TCoords.
    Trees { open_neighbor: TreeifyOpenNeighbor },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TreeifyMutation {
    pub world_x: i32,
    pub world_y: i32,
    pub kind: TreeifyMutationKind,
    pub flags_before: u16,
    pub flags_after: u16,
    pub land_before: i8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeifyMountainsReceipt {
    pub mountain_tcoord_queries: u32,
    pub chance_draws: u32,
    pub mutations: Vec<TreeifyMutation>,
    pub rng_state_after: i32,
}

/// Exact call gate at `TerrainGroups::place_all` `0x006a8ef7`--`0x006a8f12`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeifyMountainsGateReceipt {
    pub map_style: u8,
    /// Retail skips `treeify_mountains` only for map style 9.
    pub called: bool,
    pub treeify: Option<TreeifyMountainsReceipt>,
    pub rng_state_after: i32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TreeifyMountainsError {
    InvalidWorldShape {
        xs: i32,
        ys: i32,
        size: i32,
        wdata_len: usize,
        tile_xs: i32,
        tile_ys: i32,
        tile_size: i32,
        tdata_len: usize,
    },
}

/// Fixed global inputs read by the reporting tail. The PDB declares
/// `player_scores` as `int[5][8]`; native indexes it as
/// `player_scores[player][type_slot]`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct PlacementReportingInputs {
    pub num_players: i32,
    pub player_scores: [[i32; 5]; 8],
}

/// String-table rows selected by the retail instruction stream. The values are
/// byte offsets into `int_str_array`'s 20-byte `String` rows; text remains a
/// presentation concern and is not fabricated by the simulation.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum PlacementReportString {
    Header = 0x1fcc0,
    GroupPrefix = 0x1fcd4,
    GroupType4 = 0x1fce8,
    GroupType5 = 0x1fcfc,
    GroupType6 = 0x1fd10,
    GroupType7 = 0x1fd24,
    GroupType8 = 0x2878,
    PlayerPrefix = 0x1f98c,
    ScorePrefix = 0x1fd4c,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PlacementReportingError {
    UnsupportedPlayerCount { num_players: i32, supported: i32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlacementReportingReceipt {
    pub host_events: Vec<PlaceAllHostEvent>,
    pub console_info_before: i32,
    pub console_info_after: i32,
    pub return_value: i32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PlaceAllHostEvent {
    /// `NetDaemon::process_all` at `0x006a7645`, before inspecting each group.
    NetDaemonProcessAll { group_index: usize },
    /// The additional daemon pump immediately before a pattern-0 player call.
    NetDaemonProcessAllPlayer {
        group_index: usize,
        clump_index: usize,
        player_index: usize,
    },
    /// Progress-display calls within the selected pattern arm.  They are
    /// simulation-external but ordered before that arm's gameplay dependency.
    ProgressDisplay { group_index: usize, pattern: i32 },
    /// `Doober::add_doober("bush", ...)` calls in the first `add_doobers` pass.
    AddBushDoober { placement: BushDooberPlacement },
    /// The same shipped `"bush"` add call from the mountain-rock fringe pass.
    AddMountainRockDoober {
        placement: MountainRockDooberPlacement,
    },
    /// First `Log::say` call in the post-placement reporting tail.
    PlacementReportHeader { text: PlacementReportString },
    /// One of the five outer-loop terrain-type lines. Native appends the
    /// zero-based type slot after the localized prefix and label.
    PlacementReportGroup {
        prefix: PlacementReportString,
        label: PlacementReportString,
        type_slot: i32,
    },
    /// Inner-loop line for one `[player][type_slot]` score.
    PlacementReportPlayerScore {
        prefix: PlacementReportString,
        player_index: i32,
        score_prefix: PlacementReportString,
        type_slot: i32,
        score: i32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerrainGroupPlacementPreparation {
    pub group_index: usize,
    pub group_type: i32,
    pub pattern: i32,
    pub primary_min: i32,
    pub primary_max: i32,
    pub secondary_min: i32,
    pub secondary_max: i32,
    pub primary_sizes: Vec<i32>,
    pub secondary_sizes: Vec<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerrainPlacementPreparationReceipt {
    pub host_events: Vec<PlaceAllHostEvent>,
    /// Selected groups fully prepared before the first missing gameplay input.
    pub prepared_groups: Vec<TerrainGroupPlacementPreparation>,
    pub primary_size_draws: u32,
    pub secondary_size_draws: u32,
    pub rng_state_after: i32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TerrainPlacementPreparationError {
    SelectionLengthMismatch {
        groups: usize,
        selections: usize,
    },
    InvalidPrimarySizeSpan {
        group_index: usize,
        min_size: i32,
        max_size: i32,
    },
    InvalidSecondarySizeSpan {
        group_index: usize,
        min_size: i32,
        max_size: i32,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TerrainPlacementBoundary {
    /// Pattern 0 next enumerates players and calls `place_player_group`
    /// (`0x006a4190`).
    PlayerRosterAndPlacementKernel { group_index: usize },
    PlayerGroupExternalSubsystem {
        group_index: usize,
        clump_index: usize,
        player_index: usize,
        request: PlayerGroupExternalRequest,
    },
    PlayerGroupGrowthKernel {
        group_index: usize,
        clump_index: usize,
        player_index: usize,
    },
    /// Patterns 1--3 first inspect the unit-type catalog and then call
    /// `place_region_group` (`0x006a2f60`).
    UnitTypeCatalogAndRegionPlacementKernel { group_index: usize, pattern: i32 },
    /// The chosen `drop_tile` branch belongs to another gameplay subsystem and
    /// needs the exact typed resolution before local continuation can advance.
    RegionGroupDropTileExternalSubsystem { request: DropTileExternalRequest },
    /// The selected `place_region_group` call returned exactly; the enclosing
    /// catalog/per-clump continuation is the next unrecovered row.
    RegionGroupReturnControl {
        group_index: usize,
        return_value: i32,
    },
    /// Every group was visited and either branch-skipped or completed; retail
    /// next calls
    /// `TerrainGroups::add_doobers` (`0x006a1540`).
    AddDoobers,
    /// Both `add_doobers` passes completed. Retail next reads `GameInfo::map_style`
    /// and `TileSetGroupData::mnt_fringe_tree_prob` to gate `treeify_mountains`.
    TreeifyMountainsMapStyle,
    /// Treeification (or the exact map-style-9 skip) completed at `0x006a8f12`.
    /// Retail's remaining tail is localized reporting/formatting before returning
    /// one.
    PostPlacementReporting,
}

/// First unresolved dependency in `TerrainGroups::place_all`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaceAllError {
    InvalidTerrainGroupSelection(TerrainGroupSelectionError),
    InvalidTerrainPlacementPreparation(TerrainPlacementPreparationError),
    InvalidBushFringe(BushFringeError),
    InvalidMountainRockFringe(MountainRockFringeError),
    InvalidTreeifyMountains(TreeifyMountainsError),
    InvalidPlacementReporting(PlacementReportingError),
    InvalidRegionGroupContinuation(PlaceRegionGroupError),
    InvalidRegionPattern(RegionPatternError),
    InvalidOwnedOilGoodMutation(OilGoodMutationError),
    InvalidPlayerGroupPrefix(PlacePlayerGroupError),
    InvalidPlayerMountainTemplateRetry(PlayerMountainTemplateRetryError),
    InvalidMixedGroupInput {
        expected_group_index: usize,
        expected_kind: TerrainGroupInputKind,
        actual_group_index: usize,
        actual_kind: TerrainGroupInputKind,
    },
    InvalidPlayerGroupInputs {
        group_index: usize,
    },
    InvalidRegionPatternInputs {
        group_index: usize,
    },
    InvalidResolvedRegionGroupPlacement {
        group_index: usize,
        clump_index: usize,
    },
    /// The exact randomization, selection, host-event order, and clump-size
    /// preparation prefix completed.  `boundary` is the first missing gameplay
    /// input/kernel on the path selected by the group data and call flags.
    GameplayPlacementUnavailable {
        preview: PlaceAllPreviewReceipt,
        boundary: TerrainPlacementBoundary,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct PlacementBounds {
    primary_min: i32,
    primary_max: i32,
    secondary_min: i32,
    secondary_max: i32,
}

struct OwnedPlayerMountainResolver<'a> {
    runtime: &'a mut MountainAddRuntime,
    receipts: Vec<MountainOwnerExecutionReceipt>,
}

impl PlayerMountainResolver for OwnedPlayerMountainResolver<'_> {
    fn resolve(
        &mut self,
        request: PlayerGroupExternalRequest,
        world: &mut World,
    ) -> Result<Option<PlayerMountainResolution>, PlacePlayerGroupError> {
        let PlayerGroupExternalRequest::MountainsAddMountain {
            template,
            world_x,
            world_y,
            pattern,
            mountain_space,
            forest_space,
            rock_space,
            coast_space,
            start_min,
        } = request
        else {
            return Ok(None);
        };
        let world_before = world.clone();
        let walked_before = self.runtime.walked_bytes();
        let execution = self
            .runtime
            .apply_add_mountain(
                world,
                AddMountainCall {
                    template,
                    world_x,
                    world_y,
                    verification_mode: pattern,
                    mountain_space,
                    forest_space,
                    rock_space,
                    coast_space,
                    start_min,
                },
            )
            .map_err(PlacePlayerGroupError::InvalidMountainRuntime)?;
        let walked_after = self.runtime.walked_bytes();
        let liberr = execution.liberr;
        self.receipts.push(MountainOwnerExecutionReceipt {
            execution,
            world: build_world_receipt(&world_before, world),
            mountain_walk_adler_before: crate::checksum::adler32(1, &walked_before),
            mountain_walk_adler_after: crate::checksum::adler32(1, &walked_after),
            mountain_walk_bytes_before: walked_before.len(),
            mountain_walk_bytes_after: walked_after.len(),
        });
        Ok(Some(PlayerMountainResolution {
            liberr,
            recorded_external: false,
        }))
    }
}

impl TerrainGroups {
    /// Complete `TerrainGroups::fill_fertile` `0x006a6f90`--`0x006a70cd`.
    ///
    /// Retail scans X outside / Y inside.  For every cell whose `land` byte is
    /// exactly zero, it finds the first partition strictly greater than the
    /// fractal byte.  Equality advances to the next bucket.  It logs that integer,
    /// narrows it into `land_sub`, and redundantly writes `land = 0`; all other
    /// `WData` bytes are preserved.  Non-fertile land classes are skipped.
    pub fn fill_fertile(&self, world: &mut World) -> Result<FillFertileReceipt, FillFertileError> {
        self.fill_fertile_with_log(world, |_| {})
    }

    /// Same shipped body with the `Log::say` integer argument exposed so callers
    /// can preserve or test its observable X-major order.  Logging itself does not
    /// mutate deterministic simulation state.
    pub fn fill_fertile_with_log(
        &self,
        world: &mut World,
        mut log_bucket: impl FnMut(i32),
    ) -> Result<FillFertileReceipt, FillFertileError> {
        validate_world(world)?;
        self.validate_fractal_accesses(world)?;

        let mut receipt = FillFertileReceipt::default();
        for x in 0..world.xs {
            for y in 0..world.ys {
                if world.wdata(x, y).land != 0 {
                    continue;
                }

                let value = self.fractal.columns[x as usize][y as usize];
                let mut bucket = 0usize;
                while bucket < self.partitions.len() && value >= self.partitions[bucket] {
                    bucket += 1;
                }

                log_bucket(bucket as i32);
                let cell = world.wdata_mut(x, y);
                cell.land_sub = bucket as u8;
                cell.land = 0;
                receipt.fertile_cells += 1;
            }
        }
        Ok(receipt)
    }

    /// Fail-closed prefix of `TerrainGroups::place_all` `0x006a70d0`.
    ///
    /// Before the first terrain-group selection draw, retail randomizes mountains
    /// at `0x006a7330`, then executes [`Self::select_groups`] and normalizes five
    /// type accumulators.  The prefix is executed on preview clones, proving its
    /// composed RNG order while retaining the fail-closed contract: until the
    /// gameplay placement kernels are recovered, no partial world, group,
    /// mountain-list, or RNG mutation escapes.
    pub fn place_all(
        &mut self,
        _world: &mut World,
        random: &mut Random,
        mountains: &mut Mountains,
        _progress: i32,
        _place_players: i32,
    ) -> Result<i32, PlaceAllError> {
        self.place_all_with_host(_world, random, mountains, _progress, _place_players, |_| {})
    }

    /// Same fail-closed deterministic prefix with simulation-external host calls
    /// surfaced in their exact order.  The callback may pump sockets/UI; it must
    /// not mutate deterministic simulation state.
    pub fn place_all_with_host(
        &mut self,
        _world: &mut World,
        random: &mut Random,
        mountains: &mut Mountains,
        progress: i32,
        place_players: i32,
        mut host: impl FnMut(PlaceAllHostEvent),
    ) -> Result<i32, PlaceAllError> {
        self.place_all_preview(
            _world,
            random,
            mountains,
            progress,
            place_players,
            None,
            None,
            None,
            None,
            None,
            &mut host,
        )
    }

    /// Advances the common `place_all` path through both complete deterministic
    /// passes of `TerrainGroups::add_doobers`.
    ///
    /// `rules` are the recovered `TileSetGroupData` tree/doober fields owned by
    /// the active tileset.  Bush creation is simulation-external and is surfaced
    /// as ordered host events. The transaction remains fail-closed at the next
    /// stage, `TerrainGroups::treeify_mountains`.
    pub fn place_all_with_doober_rules(
        &mut self,
        world: &mut World,
        random: &mut Random,
        mountains: &mut Mountains,
        progress: i32,
        place_players: i32,
        rules: DooberTilesetRules,
        mut host: impl FnMut(PlaceAllHostEvent),
    ) -> Result<i32, PlaceAllError> {
        self.place_all_preview(
            world,
            random,
            mountains,
            progress,
            place_players,
            Some(rules),
            None,
            None,
            None,
            None,
            &mut host,
        )
    }

    /// Extends [`Self::place_all_with_doober_rules`] through the exact map-style
    /// gate and `TerrainGroups::treeify_mountains` body.
    #[allow(clippy::too_many_arguments)]
    pub fn place_all_with_treeify_inputs(
        &mut self,
        world: &mut World,
        random: &mut Random,
        mountains: &mut Mountains,
        progress: i32,
        place_players: i32,
        rules: DooberTilesetRules,
        map_style: u8,
        mut host: impl FnMut(PlaceAllHostEvent),
    ) -> Result<i32, PlaceAllError> {
        self.place_all_preview(
            world,
            random,
            mountains,
            progress,
            place_players,
            Some(rules),
            Some(map_style),
            None,
            None,
            None,
            &mut host,
        )
    }

    /// Composes `place_all` through the exact deterministic entry search of
    /// `TerrainGroup::place_region_group` (`0x006a2f60`).
    ///
    /// `resolved` is an explicit receipt from the immediate upstream
    /// unit-catalog/region-selection block, which is not yet implemented. Its RNG
    /// state is installed only on the transaction's preview RNG. Consequently no
    /// group, world, mountain-list, or caller RNG mutation escapes at the next
    /// `drop_tile` boundary.
    #[allow(clippy::too_many_arguments)]
    pub fn place_all_with_resolved_region_group(
        &mut self,
        world: &mut World,
        regions: &Regions,
        random: &mut Random,
        mountains: &mut Mountains,
        progress: i32,
        place_players: i32,
        resolved: ResolvedRegionGroupPlacement,
        mut host: impl FnMut(PlaceAllHostEvent),
    ) -> Result<i32, PlaceAllError> {
        self.place_all_preview(
            world,
            random,
            mountains,
            progress,
            place_players,
            None,
            None,
            None,
            None,
            Some((regions, resolved)),
            &mut host,
        )
    }

    /// Executes the exact pattern-1/2/3 region and clump loop without a
    /// synthetic per-call `ResolvedRegionGroupPlacement` handoff.
    #[allow(clippy::too_many_arguments)]
    pub fn place_all_with_regions(
        &mut self,
        world: &mut World,
        regions: &Regions,
        random: &mut Random,
        mountains: &mut Mountains,
        progress: i32,
        place_players: i32,
        helping: Option<RegionHelpingState>,
        externals: &[DropTileExternalResolution],
        mut host: impl FnMut(PlaceAllHostEvent),
    ) -> Result<i32, PlaceAllError> {
        self.place_all_preview(
            world,
            random,
            mountains,
            progress,
            place_players,
            None,
            None,
            None,
            Some((regions, helping, externals)),
            None,
            &mut host,
        )
    }

    /// Executes the exact selected pattern-0 clump/player loop, including
    /// type-4 strict retry, type-4/type-6 growth, and type-5 template fallback.
    /// Object-system effects remain ordered typed inputs; an absent input stops
    /// at the exact request without mutating caller-owned preview state.
    #[allow(clippy::too_many_arguments)]
    pub fn place_all_with_player_group_inputs(
        &mut self,
        world: &mut World,
        random: &mut Random,
        mountains: &mut Mountains,
        progress: i32,
        place_players: i32,
        externals: &[PlayerGroupExternalResolution],
        mut host: impl FnMut(PlaceAllHostEvent),
    ) -> Result<i32, PlaceAllError> {
        self.place_all_preview(
            world,
            random,
            mountains,
            progress,
            place_players,
            None,
            None,
            Some(externals),
            None,
            None,
            &mut host,
        )
    }

    /// Executes selected pattern-0 and pattern-1/2/3 groups in one native
    /// group-index transaction. Each input row must match the next selected
    /// group's index and arm kind; omission stops at that already-prepared
    /// kernel, while a conflicting row fails closed.
    #[allow(clippy::too_many_arguments)]
    pub fn place_all_with_group_inputs(
        &mut self,
        world: &mut World,
        regions: &Regions,
        random: &mut Random,
        mountains: &mut Mountains,
        progress: i32,
        place_players: i32,
        helping: Option<RegionHelpingState>,
        inputs: &[PlaceAllGroupInput],
        mut host: impl FnMut(PlaceAllHostEvent),
    ) -> Result<i32, PlaceAllError> {
        self.place_all_with_group_inputs_preview(
            world,
            regions,
            random,
            mountains,
            progress,
            place_players,
            helping,
            inputs,
            None,
            None,
            None,
            None,
            &mut host,
        )
    }

    /// Extends [`Self::place_all_with_group_inputs`] across the common
    /// `TerrainGroups::add_doobers` call at `0x006a8ef2` after every selected
    /// player/region arm completes.
    ///
    /// Both doober passes inspect the post-placement preview world. Their
    /// `Doober::add_doober` effects are surfaced as ordered host events, while
    /// caller-owned world, groups, mountains and RNG remain transactional at
    /// the following map-style/treeification boundary.
    #[allow(clippy::too_many_arguments)]
    pub fn place_all_with_group_and_doober_inputs(
        &mut self,
        world: &mut World,
        regions: &Regions,
        random: &mut Random,
        mountains: &mut Mountains,
        progress: i32,
        place_players: i32,
        helping: Option<RegionHelpingState>,
        inputs: &[PlaceAllGroupInput],
        rules: DooberTilesetRules,
        mut host: impl FnMut(PlaceAllHostEvent),
    ) -> Result<i32, PlaceAllError> {
        self.place_all_with_group_inputs_preview(
            world,
            regions,
            random,
            mountains,
            progress,
            place_players,
            helping,
            inputs,
            None,
            Some(rules),
            None,
            None,
            &mut host,
        )
    }

    /// Extends the heterogeneous group/doober transaction through the exact
    /// `GameInfo::map_style` gate and `TerrainGroups::treeify_mountains` body.
    /// The treeification scan consumes the RNG left by both doober passes and
    /// mutates their shared post-group preview world.
    #[allow(clippy::too_many_arguments)]
    pub fn place_all_with_group_treeify_inputs(
        &mut self,
        world: &mut World,
        regions: &Regions,
        random: &mut Random,
        mountains: &mut Mountains,
        progress: i32,
        place_players: i32,
        helping: Option<RegionHelpingState>,
        inputs: &[PlaceAllGroupInput],
        rules: DooberTilesetRules,
        map_style: u8,
        mut host: impl FnMut(PlaceAllHostEvent),
    ) -> Result<i32, PlaceAllError> {
        self.place_all_with_group_inputs_preview(
            world,
            regions,
            random,
            mountains,
            progress,
            place_players,
            helping,
            inputs,
            None,
            Some(rules),
            Some(map_style),
            None,
            &mut host,
        )
    }

    /// Complete heterogeneous `TerrainGroups::place_all` transaction through
    /// its presentation-only reporting tail and native return value `1`.
    /// Reporting inputs mirror the fixed `num_players` and `player_scores[8][5]`
    /// globals. On success, deterministic preview state is committed only after
    /// the final ordered log receipt and `console_info` clear.
    #[allow(clippy::too_many_arguments)]
    pub fn place_all_with_group_reporting_inputs(
        &mut self,
        world: &mut World,
        regions: &Regions,
        random: &mut Random,
        mountains: &mut Mountains,
        progress: i32,
        place_players: i32,
        helping: Option<RegionHelpingState>,
        inputs: &[PlaceAllGroupInput],
        rules: DooberTilesetRules,
        map_style: u8,
        reporting: PlacementReportingInputs,
        mut host: impl FnMut(PlaceAllHostEvent),
    ) -> Result<i32, PlaceAllError> {
        self.place_all_with_group_inputs_preview(
            world,
            regions,
            random,
            mountains,
            progress,
            place_players,
            helping,
            inputs,
            None,
            Some(rules),
            Some(map_style),
            Some(reporting),
            &mut host,
        )
    }

    /// Owner-aware replay adapter for the heterogeneous group transaction.
    ///
    /// The owner state is cloned beside the existing Group, World, RNG, and
    /// mountain-cursor previews. It is threaded through player oil calls and
    /// region-pattern mountain/oil calls, then committed only if `place_all`
    /// reaches its native return. The optional tail facts select the same
    /// doober/treeify/reporting continuations as the legacy wrappers.
    #[allow(clippy::too_many_arguments)]
    pub fn place_all_with_group_owned_inputs(
        &mut self,
        world: &mut World,
        regions: &Regions,
        random: &mut Random,
        mountains: &mut Mountains,
        owners: &mut PlaceRegionGroupOwners,
        progress: i32,
        place_players: i32,
        helping: Option<RegionHelpingState>,
        inputs: &[PlaceAllGroupInput],
        doober_rules: Option<DooberTilesetRules>,
        map_style: Option<u8>,
        reporting: Option<PlacementReportingInputs>,
        mut host: impl FnMut(PlaceAllHostEvent),
    ) -> Result<i32, PlaceAllError> {
        self.place_all_with_group_inputs_preview(
            world,
            regions,
            random,
            mountains,
            progress,
            place_players,
            helping,
            inputs,
            Some(owners),
            doober_rules,
            map_style,
            reporting,
            &mut host,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn place_all_with_group_inputs_preview(
        &mut self,
        world: &mut World,
        regions: &Regions,
        random: &mut Random,
        mountains: &mut Mountains,
        progress: i32,
        place_players: i32,
        helping: Option<RegionHelpingState>,
        inputs: &[PlaceAllGroupInput],
        mut owners: Option<&mut PlaceRegionGroupOwners>,
        doober_rules: Option<DooberTilesetRules>,
        map_style: Option<u8>,
        reporting: Option<PlacementReportingInputs>,
        host: &mut impl FnMut(PlaceAllHostEvent),
    ) -> Result<i32, PlaceAllError> {
        if let Some(reporting) = reporting {
            validate_placement_reporting_inputs(reporting)
                .map_err(PlaceAllError::InvalidPlacementReporting)?;
        }
        if let Some(rules) = doober_rules {
            validate_bush_fringe_inputs(world, rules).map_err(PlaceAllError::InvalidBushFringe)?;
            validate_mountain_rock_fringe_inputs(world, rules)
                .map_err(PlaceAllError::InvalidMountainRockFringe)?;
            if map_style.is_some_and(|style| style != 9) {
                validate_treeify_world(world).map_err(PlaceAllError::InvalidTreeifyMountains)?;
            }
        }
        let mut preview_random = *random;
        let mut preview_mountains = mountains.clone();
        let mountain_randomization = preview_mountains.randomize_mountains(&mut preview_random);
        let group_selection = self
            .select_groups(&mut preview_random)
            .map_err(PlaceAllError::InvalidTerrainGroupSelection)?;
        let (mut placement_preparation, boundary) = self
            .prepare_placement_prefix(
                &group_selection.groups,
                &mut preview_random,
                progress,
                place_players,
                &mut *host,
            )
            .map_err(PlaceAllError::InvalidTerrainPlacementPreparation)?;

        let mut preview_world = world.clone();
        let mut preview_groups = self.groups.clone();
        let mut preview_owners = owners.as_deref().cloned();
        let mut current_helping = helping;
        let mut next = boundary;
        let mut input_cursor = 0usize;
        let mut completed_placement_groups = Vec::new();
        let mut player_group_dispatches = Vec::new();
        let mut region_pattern_dispatches = Vec::new();
        let mut owner_receipts = Vec::new();
        let mut player_calls = Vec::new();
        let mut player_group_mountain_retries = Vec::new();
        let mut player_group_host_events = Vec::new();
        let mut player_group_placed_after = Vec::new();
        let mut player_group_formation_x = Vec::new();
        let mut player_group_formation_y = Vec::new();
        let mut region_group_prefix = None;
        let mut region_group_drop = None;
        let mut region_group_continuation = None;
        let mut region_pattern = None;
        let mut bush_fringe = None;
        let mut mountain_rock_fringe = None;
        let mut treeify_mountains = None;

        loop {
            let expected = match next {
                TerrainPlacementBoundary::PlayerRosterAndPlacementKernel { group_index } => {
                    Some((group_index, TerrainGroupInputKind::Player))
                }
                TerrainPlacementBoundary::UnitTypeCatalogAndRegionPlacementKernel {
                    group_index,
                    ..
                } => Some((group_index, TerrainGroupInputKind::Region)),
                _ => None,
            };
            let Some((group_index, expected_kind)) = expected else {
                break;
            };
            let Some(input) = inputs.get(input_cursor) else {
                break;
            };
            if input.group_index() != group_index || input.kind() != expected_kind {
                return Err(PlaceAllError::InvalidMixedGroupInput {
                    expected_group_index: group_index,
                    expected_kind,
                    actual_group_index: input.group_index(),
                    actual_kind: input.kind(),
                });
            }

            let Some(prepared) = placement_preparation
                .prepared_groups
                .iter()
                .find(|prepared| prepared.group_index == group_index)
            else {
                return Err(match expected_kind {
                    TerrainGroupInputKind::Player => {
                        PlaceAllError::InvalidPlayerGroupInputs { group_index }
                    }
                    TerrainGroupInputKind::Region => {
                        PlaceAllError::InvalidRegionPatternInputs { group_index }
                    }
                });
            };

            match input {
                PlaceAllGroupInput::Player { externals, .. } => {
                    let (execution, executed_owners) =
                        if let Some(staged_owners) = preview_owners.as_mut() {
                            Self::execute_player_pattern_group_owned(
                                &mut preview_groups[group_index],
                                &mut preview_world,
                                &mut preview_random,
                                &mut preview_mountains,
                                prepared,
                                externals,
                                staged_owners,
                                &mut *host,
                            )?
                        } else {
                            (
                                Self::execute_player_pattern_group(
                                    &mut preview_groups[group_index],
                                    &mut preview_world,
                                    &mut preview_random,
                                    &mut preview_mountains,
                                    prepared,
                                    externals,
                                    &mut *host,
                                )?,
                                Vec::new(),
                            )
                        };
                    owner_receipts.extend(executed_owners);
                    player_calls.extend(execution.calls.iter().cloned());
                    player_group_mountain_retries
                        .extend(execution.mountain_retries.iter().cloned());
                    player_group_host_events.extend(execution.host_events.iter().copied());
                    player_group_placed_after = execution.placed_after.clone();
                    player_group_formation_x = execution.formation_x_after.clone();
                    player_group_formation_y = execution.formation_y_after.clone();
                    next = match execution.outcome {
                        PlayerPatternGroupOutcome::Complete => {
                            completed_placement_groups.push(group_index);
                            self.prepare_placement_continuation(
                                &group_selection.groups,
                                &mut preview_random,
                                progress,
                                place_players,
                                group_index + 1,
                                &mut placement_preparation,
                                &mut *host,
                            )
                            .map_err(PlaceAllError::InvalidTerrainPlacementPreparation)?
                        }
                        PlayerPatternGroupOutcome::ExternalResolutionRequired {
                            clump_index,
                            player_index,
                            request,
                        } => TerrainPlacementBoundary::PlayerGroupExternalSubsystem {
                            group_index,
                            clump_index,
                            player_index,
                            request,
                        },
                        PlayerPatternGroupOutcome::GrowthKernel {
                            clump_index,
                            player_index,
                        } => TerrainPlacementBoundary::PlayerGroupGrowthKernel {
                            group_index,
                            clump_index,
                            player_index,
                        },
                    };
                    player_group_dispatches.push(execution);
                }
                PlaceAllGroupInput::Region { externals, .. } => {
                    let group_type = self.groups[group_index].group_type;
                    let type_slot = usize::try_from(group_type - 4)
                        .ok()
                        .filter(|&slot| slot < 5)
                        .ok_or(PlaceAllError::InvalidRegionPatternInputs { group_index })?;
                    let receipt = if let Some(staged_owners) = preview_owners.as_mut() {
                        preview_groups[group_index].apply_region_pattern_owned_audited(
                            &mut preview_world,
                            regions,
                            &mut preview_random,
                            &mut preview_mountains,
                            prepared.pattern,
                            &prepared.primary_sizes,
                            &prepared.secondary_sizes,
                            group_selection.normalized_clumps_by_type[type_slot],
                            place_players,
                            group_index,
                            current_helping,
                            staged_owners,
                        )
                    } else {
                        preview_groups[group_index].apply_region_pattern(
                            &mut preview_world,
                            regions,
                            &mut preview_random,
                            &mut preview_mountains,
                            prepared.pattern,
                            &prepared.primary_sizes,
                            &prepared.secondary_sizes,
                            group_selection.normalized_clumps_by_type[type_slot],
                            place_players,
                            group_index,
                            current_helping,
                            externals,
                        )
                    }
                    .map_err(PlaceAllError::InvalidRegionPattern)?;
                    for (call_index, call) in receipt.calls.iter().enumerate() {
                        owner_receipts.extend(call.owners.iter().cloned().map(|execution| {
                            PlaceAllOwnerExecutionReceipt {
                                group_index,
                                source: PlaceAllOwnerSource::Region { call_index },
                                execution,
                            }
                        }));
                    }
                    current_helping = receipt.helping_after;
                    if let Some(first) = receipt.calls.first() {
                        region_group_prefix = Some(first.placement.prefix.clone());
                        region_group_drop = first.placement.drops.first().cloned();
                        region_group_continuation = Some(first.placement.clone());
                    }
                    next = match receipt.outcome {
                        RegionPatternOutcome::ExternalResolutionRequired { request } => {
                            TerrainPlacementBoundary::RegionGroupDropTileExternalSubsystem {
                                request,
                            }
                        }
                        RegionPatternOutcome::Complete => {
                            completed_placement_groups.push(group_index);
                            self.prepare_placement_continuation(
                                &group_selection.groups,
                                &mut preview_random,
                                progress,
                                place_players,
                                group_index + 1,
                                &mut placement_preparation,
                                &mut *host,
                            )
                            .map_err(PlaceAllError::InvalidTerrainPlacementPreparation)?
                        }
                    };
                    region_pattern = Some(receipt.clone());
                    region_pattern_dispatches.push(RegionPatternGroupReceipt {
                        group_index,
                        pattern: receipt,
                    });
                }
            }
            input_cursor += 1;
        }

        if next == TerrainPlacementBoundary::AddDoobers {
            if let Some(rules) = doober_rules {
                let receipt = plan_bush_fringe_with_host(
                    &preview_world,
                    rules,
                    &mut preview_random,
                    |placement| host(PlaceAllHostEvent::AddBushDoober { placement }),
                )
                .map_err(PlaceAllError::InvalidBushFringe)?;
                bush_fringe = Some(receipt);
                let receipt = plan_mountain_rock_fringe_with_host(
                    &preview_world,
                    rules,
                    &mut preview_random,
                    |placement| host(PlaceAllHostEvent::AddMountainRockDoober { placement }),
                )
                .map_err(PlaceAllError::InvalidMountainRockFringe)?;
                mountain_rock_fringe = Some(receipt);
                next = TerrainPlacementBoundary::TreeifyMountainsMapStyle;
            }
        }
        if next == TerrainPlacementBoundary::TreeifyMountainsMapStyle {
            if let (Some(rules), Some(map_style)) = (doober_rules, map_style) {
                treeify_mountains = Some(
                    Self::treeify_mountains_for_map_style(
                        &mut preview_world,
                        &mut preview_random,
                        map_style,
                        rules.mountain_fringe_tree_prob,
                    )
                    .map_err(PlaceAllError::InvalidTreeifyMountains)?,
                );
                next = TerrainPlacementBoundary::PostPlacementReporting;
            }
        }
        if next == TerrainPlacementBoundary::PostPlacementReporting {
            if let Some(reporting) = reporting {
                let receipt =
                    Self::plan_placement_reporting(self.console_info, reporting, &mut *host)
                        .map_err(PlaceAllError::InvalidPlacementReporting)?;
                self.groups = preview_groups;
                self.console_info = receipt.console_info_after;
                *world = preview_world;
                *random = preview_random;
                *mountains = preview_mountains;
                if let (Some(target), Some(staged)) = (owners.as_deref_mut(), preview_owners) {
                    *target = staged;
                }
                return Ok(receipt.return_value);
            }
        }

        Err(PlaceAllError::GameplayPlacementUnavailable {
            preview: PlaceAllPreviewReceipt {
                mountain_randomization,
                group_selection,
                placement_preparation,
                bush_fringe,
                mountain_rock_fringe,
                treeify_mountains,
                region_group_prefix,
                region_group_drop,
                region_group_continuation,
                region_pattern,
                region_pattern_dispatches,
                owner_receipts,
                player_group_prefix: if player_group_dispatches.is_empty() {
                    None
                } else {
                    Some(player_calls)
                },
                player_group_mountain_retries,
                player_group_host_events,
                player_group_placed_after,
                player_group_formation_x,
                player_group_formation_y,
                player_group_dispatches,
                completed_placement_groups,
            },
            boundary: next,
        })
    }

    /// Complete post-placement reporting tail (`0x006a8f12`--`0x006a937d`).
    ///
    /// String construction and `Log::say` are presentation-only, so each line
    /// is surfaced as a typed host event carrying the exact string-table rows
    /// and appended integers. The deterministic tail clears `console_info` only
    /// after all lines and returns one.
    pub fn plan_placement_reporting(
        console_info_before: i32,
        inputs: PlacementReportingInputs,
        mut host: impl FnMut(PlaceAllHostEvent),
    ) -> Result<PlacementReportingReceipt, PlacementReportingError> {
        validate_placement_reporting_inputs(inputs)?;
        let mut host_events = Vec::new();

        let header = PlaceAllHostEvent::PlacementReportHeader {
            text: PlacementReportString::Header,
        };
        host(header);
        host_events.push(header);

        const LABELS: [PlacementReportString; 5] = [
            PlacementReportString::GroupType4,
            PlacementReportString::GroupType5,
            PlacementReportString::GroupType6,
            PlacementReportString::GroupType7,
            PlacementReportString::GroupType8,
        ];
        for (type_slot, label) in LABELS.into_iter().enumerate() {
            let group = PlaceAllHostEvent::PlacementReportGroup {
                prefix: PlacementReportString::GroupPrefix,
                label,
                type_slot: type_slot as i32,
            };
            host(group);
            host_events.push(group);

            for player_index in 0..inputs.num_players.max(0) as usize {
                let score = inputs.player_scores[player_index][type_slot];
                let player = PlaceAllHostEvent::PlacementReportPlayerScore {
                    prefix: PlacementReportString::PlayerPrefix,
                    player_index: player_index as i32,
                    score_prefix: PlacementReportString::ScorePrefix,
                    type_slot: type_slot as i32,
                    score,
                };
                host(player);
                host_events.push(player);
            }
        }

        Ok(PlacementReportingReceipt {
            host_events,
            console_info_before,
            console_info_after: 0,
            return_value: 1,
        })
    }

    /// Exact `GameInfo::map_style` gate immediately preceding
    /// `TerrainGroups::treeify_mountains` in `place_all`.
    ///
    /// Retail skips the call only for map style 9. Because that path neither
    /// reads the world nor advances the RNG, malformed world storage is also
    /// unobserved on the skip path.
    pub fn treeify_mountains_for_map_style(
        world: &mut World,
        random: &mut Random,
        map_style: u8,
        chance: i32,
    ) -> Result<TreeifyMountainsGateReceipt, TreeifyMountainsError> {
        if map_style == 9 {
            return Ok(TreeifyMountainsGateReceipt {
                map_style,
                called: false,
                treeify: None,
                rng_state_after: random.state(),
            });
        }

        let treeify = Self::treeify_mountains(world, random, chance)?;
        Ok(TreeifyMountainsGateReceipt {
            map_style,
            called: true,
            rng_state_after: treeify.rng_state_after,
            treeify: Some(treeify),
        })
    }

    /// Complete deterministic body of `TerrainGroups::treeify_mountains`
    /// (`0x006a1cc0`--`0x006a1eb5`).
    ///
    /// The scan is X-major. Every world cell first calls
    /// `WorldData::has_mountain_tcoords` (`0x006b3050`) over its 4x4 tile block.
    /// Mountain-tile fringe cells become forest after SOUTH-then-EAST WData
    /// exclusions; WData mountain cells become the `0x30` class when the first
    /// open tile neighbor is EAST or SOUTH. Candidate cells always consume one
    /// percentile draw, including when `chance` is zero.
    pub fn treeify_mountains(
        world: &mut World,
        random: &mut Random,
        chance: i32,
    ) -> Result<TreeifyMountainsReceipt, TreeifyMountainsError> {
        validate_treeify_world(world)?;

        let mut receipt = TreeifyMountainsReceipt {
            mountain_tcoord_queries: 0,
            chance_draws: 0,
            mutations: Vec::new(),
            rng_state_after: random.state(),
        };

        for x in 0..world.xs {
            for y in 0..world.ys {
                let has_mountain_tiles =
                    has_mountain_tcoords(world, x, y, &mut receipt.mountain_tcoord_queries);
                let flags = world.wdata(x, y).flags;

                if has_mountain_tiles && flags & wflag::MOUNTAINS == 0 {
                    // The native branch checks SOUTH before EAST and skips on a
                    // neighboring WData mountain class without examining TData.
                    if y + 1 < world.ys && world.wdata(x, y + 1).flags & wflag::MOUNTAINS != 0 {
                        continue;
                    }
                    if x + 1 < world.xs && world.wdata(x + 1, y).flags & wflag::MOUNTAINS != 0 {
                        continue;
                    }
                    if flags & (wflag::COAST | wflag::ORIG_COAST) != 0 {
                        continue;
                    }

                    receipt.chance_draws += 1;
                    if random.get(0, 0xffff) % 100 < chance {
                        apply_treeify_mutation(
                            world,
                            x,
                            y,
                            TreeifyMutationKind::Forest,
                            wflag::FOREST,
                            &mut receipt,
                        );
                    }
                    continue;
                }

                if flags & wflag::MOUNTAINS == 0 {
                    continue;
                }

                // PDB globals `orthog_x`/`orthog_y` are indexed 1..4 here,
                // but retail explicitly skips indices 1 (NORTH) and 4 (WEST).
                let mut open_neighbor = None;
                if x + 1 < world.xs
                    && !has_mountain_tcoords(world, x + 1, y, &mut receipt.mountain_tcoord_queries)
                {
                    open_neighbor = Some(TreeifyOpenNeighbor::East);
                } else if y + 1 < world.ys
                    && !has_mountain_tcoords(world, x, y + 1, &mut receipt.mountain_tcoord_queries)
                {
                    open_neighbor = Some(TreeifyOpenNeighbor::South);
                }
                let Some(open_neighbor) = open_neighbor else {
                    continue;
                };
                if flags & (wflag::COAST | wflag::ORIG_COAST) != 0 {
                    continue;
                }

                receipt.chance_draws += 1;
                if random.get(0, 0xffff) % 100 < chance {
                    apply_treeify_mutation(
                        world,
                        x,
                        y,
                        TreeifyMutationKind::Trees { open_neighbor },
                        wflag::MOUNTAINS | wflag::FOREST,
                        &mut receipt,
                    );
                }
            }
        }

        receipt.rng_state_after = random.state();
        Ok(receipt)
    }

    #[allow(clippy::too_many_arguments)]
    fn place_all_preview(
        &mut self,
        world: &mut World,
        random: &mut Random,
        mountains: &mut Mountains,
        progress: i32,
        place_players: i32,
        doober_rules: Option<DooberTilesetRules>,
        map_style: Option<u8>,
        player_group_externals: Option<&[PlayerGroupExternalResolution]>,
        region_pattern_inputs: Option<(
            &Regions,
            Option<RegionHelpingState>,
            &[DropTileExternalResolution],
        )>,
        resolved_region_group: Option<(&Regions, ResolvedRegionGroupPlacement)>,
        host: &mut impl FnMut(PlaceAllHostEvent),
    ) -> Result<i32, PlaceAllError> {
        if let Some(rules) = doober_rules {
            validate_bush_fringe_inputs(world, rules).map_err(PlaceAllError::InvalidBushFringe)?;
            validate_mountain_rock_fringe_inputs(world, rules)
                .map_err(PlaceAllError::InvalidMountainRockFringe)?;
            if map_style.is_some_and(|style| style != 9) {
                validate_treeify_world(world).map_err(PlaceAllError::InvalidTreeifyMountains)?;
            }
        }
        let mut preview_random = *random;
        let mut preview_mountains = mountains.clone();
        let mountain_randomization = preview_mountains.randomize_mountains(&mut preview_random);
        let group_selection = self
            .select_groups(&mut preview_random)
            .map_err(PlaceAllError::InvalidTerrainGroupSelection)?;
        let (mut placement_preparation, boundary) = self
            .prepare_placement_prefix(
                &group_selection.groups,
                &mut preview_random,
                progress,
                place_players,
                &mut *host,
            )
            .map_err(PlaceAllError::InvalidTerrainPlacementPreparation)?;
        let mut bush_fringe = None;
        let mut mountain_rock_fringe = None;
        let mut treeify_mountains = None;
        let mut region_group_prefix = None;
        let mut region_group_drop = None;
        let mut region_group_continuation = None;
        let mut region_pattern = None;
        let mut region_pattern_dispatches = Vec::new();
        let mut player_group_prefix = None;
        let mut player_group_mountain_retries = Vec::new();
        let mut player_group_host_events = Vec::new();
        let mut player_group_placed_after = Vec::new();
        let mut player_group_formation_x = Vec::new();
        let mut player_group_formation_y = Vec::new();
        let mut player_group_dispatches = Vec::new();
        let mut completed_placement_groups = Vec::new();
        let boundary = if let Some(externals) = player_group_externals {
            if !matches!(
                boundary,
                TerrainPlacementBoundary::PlayerRosterAndPlacementKernel { .. }
            ) {
                return Err(PlaceAllError::InvalidPlayerGroupInputs { group_index: 0 });
            }
            let mut preview_world = world.clone();
            let mut preview_groups = self.groups.clone();
            let mut consumed = 0usize;
            let mut calls = Vec::new();
            let mut next = boundary;
            loop {
                let TerrainPlacementBoundary::PlayerRosterAndPlacementKernel { group_index } = next
                else {
                    break;
                };
                let Some(prepared) = placement_preparation
                    .prepared_groups
                    .iter()
                    .find(|prepared| prepared.group_index == group_index)
                else {
                    return Err(PlaceAllError::InvalidPlayerGroupInputs { group_index });
                };
                let execution = Self::execute_player_pattern_group(
                    &mut preview_groups[group_index],
                    &mut preview_world,
                    &mut preview_random,
                    &mut preview_mountains,
                    prepared,
                    &externals[consumed..],
                    &mut *host,
                )?;
                consumed += execution.external_resolutions_consumed;
                calls.extend(execution.calls.iter().cloned());
                player_group_mountain_retries.extend(execution.mountain_retries.iter().cloned());
                player_group_host_events.extend(execution.host_events.iter().copied());
                player_group_placed_after = execution.placed_after.clone();
                player_group_formation_x = execution.formation_x_after.clone();
                player_group_formation_y = execution.formation_y_after.clone();

                next = match execution.outcome {
                    PlayerPatternGroupOutcome::Complete => {
                        completed_placement_groups.push(group_index);
                        self.prepare_placement_continuation(
                            &group_selection.groups,
                            &mut preview_random,
                            progress,
                            place_players,
                            group_index + 1,
                            &mut placement_preparation,
                            &mut *host,
                        )
                        .map_err(PlaceAllError::InvalidTerrainPlacementPreparation)?
                    }
                    PlayerPatternGroupOutcome::ExternalResolutionRequired {
                        clump_index,
                        player_index,
                        request,
                    } => TerrainPlacementBoundary::PlayerGroupExternalSubsystem {
                        group_index,
                        clump_index,
                        player_index,
                        request,
                    },
                    PlayerPatternGroupOutcome::GrowthKernel {
                        clump_index,
                        player_index,
                    } => TerrainPlacementBoundary::PlayerGroupGrowthKernel {
                        group_index,
                        clump_index,
                        player_index,
                    },
                };
                player_group_dispatches.push(execution);
            }
            player_group_prefix = Some(calls);
            next
        } else if let Some((regions, helping, externals)) = region_pattern_inputs {
            if !matches!(
                boundary,
                TerrainPlacementBoundary::UnitTypeCatalogAndRegionPlacementKernel { .. }
            ) {
                return Err(PlaceAllError::InvalidRegionPatternInputs { group_index: 0 });
            }
            let mut preview_world = world.clone();
            let mut preview_groups = self.groups.clone();
            let mut current_helping = helping;
            let mut consumed = 0usize;
            let mut next = boundary;
            loop {
                let TerrainPlacementBoundary::UnitTypeCatalogAndRegionPlacementKernel {
                    group_index,
                    ..
                } = next
                else {
                    break;
                };
                let Some(prepared) = placement_preparation
                    .prepared_groups
                    .iter()
                    .find(|prepared| prepared.group_index == group_index)
                else {
                    return Err(PlaceAllError::InvalidRegionPatternInputs { group_index });
                };
                let group_type = self.groups[group_index].group_type;
                let type_slot = usize::try_from(group_type - 4)
                    .ok()
                    .filter(|&slot| slot < 5)
                    .ok_or(PlaceAllError::InvalidRegionPatternInputs { group_index })?;
                let receipt = preview_groups[group_index]
                    .apply_region_pattern(
                        &mut preview_world,
                        regions,
                        &mut preview_random,
                        &mut preview_mountains,
                        prepared.pattern,
                        &prepared.primary_sizes,
                        &prepared.secondary_sizes,
                        group_selection.normalized_clumps_by_type[type_slot],
                        place_players,
                        group_index,
                        current_helping,
                        &externals[consumed..],
                    )
                    .map_err(PlaceAllError::InvalidRegionPattern)?;
                consumed += receipt.external_resolutions_consumed;
                current_helping = receipt.helping_after;
                if let Some(first) = receipt.calls.first() {
                    region_group_prefix = Some(first.placement.prefix.clone());
                    region_group_drop = first.placement.drops.first().cloned();
                    region_group_continuation = Some(first.placement.clone());
                }
                next = match receipt.outcome {
                    RegionPatternOutcome::ExternalResolutionRequired { request } => {
                        TerrainPlacementBoundary::RegionGroupDropTileExternalSubsystem { request }
                    }
                    RegionPatternOutcome::Complete => {
                        completed_placement_groups.push(group_index);
                        self.prepare_placement_continuation(
                            &group_selection.groups,
                            &mut preview_random,
                            progress,
                            place_players,
                            group_index + 1,
                            &mut placement_preparation,
                            &mut *host,
                        )
                        .map_err(PlaceAllError::InvalidTerrainPlacementPreparation)?
                    }
                };
                region_pattern = Some(receipt.clone());
                region_pattern_dispatches.push(RegionPatternGroupReceipt {
                    group_index,
                    pattern: receipt,
                });
            }
            next
        } else if let Some((regions, resolved)) = resolved_region_group {
            let TerrainPlacementBoundary::UnitTypeCatalogAndRegionPlacementKernel {
                group_index,
                ..
            } = boundary
            else {
                return Err(PlaceAllError::InvalidResolvedRegionGroupPlacement {
                    group_index: resolved.group_index,
                    clump_index: resolved.clump_index,
                });
            };
            let Some(prepared) = placement_preparation
                .prepared_groups
                .iter()
                .find(|prepared| prepared.group_index == group_index)
            else {
                return Err(PlaceAllError::InvalidResolvedRegionGroupPlacement {
                    group_index: resolved.group_index,
                    clump_index: resolved.clump_index,
                });
            };
            if resolved.group_index != group_index
                || resolved.clump_index >= prepared.primary_sizes.len()
            {
                return Err(PlaceAllError::InvalidResolvedRegionGroupPlacement {
                    group_index: resolved.group_index,
                    clump_index: resolved.clump_index,
                });
            }

            // The unresolved catalog block owns every intervening draw. Resume
            // from its explicit call-site receipt rather than the pre-catalog RNG.
            preview_random.reseed(resolved.rng_state_at_call);
            let call = PlaceRegionGroupCall {
                target_tiles: prepared.primary_sizes[resolved.clump_index],
                region_id: resolved.region_id,
                land_subtype: resolved.land_subtype,
                oil_deposits: prepared.secondary_sizes[resolved.clump_index],
                place_players,
                group_index,
            };
            let mut preview_world = world.clone();
            let mut preview_group = self.groups[group_index].clone();
            let receipt = preview_group
                .apply_place_region_group(
                    &mut preview_world,
                    regions,
                    &mut preview_random,
                    call,
                    resolved.helping,
                    &resolved.drop_tile_externals,
                )
                .map_err(PlaceAllError::InvalidRegionGroupContinuation)?;
            let next = match receipt.outcome {
                PlaceRegionGroupOutcome::ExternalResolutionRequired { request } => {
                    TerrainPlacementBoundary::RegionGroupDropTileExternalSubsystem { request }
                }
                PlaceRegionGroupOutcome::Returned(return_value) => {
                    TerrainPlacementBoundary::RegionGroupReturnControl {
                        group_index,
                        return_value,
                    }
                }
            };
            region_group_prefix = Some(receipt.prefix.clone());
            region_group_drop = receipt.drops.first().cloned();
            region_group_continuation = Some(receipt);
            next
        } else if boundary == TerrainPlacementBoundary::AddDoobers {
            if let Some(rules) = doober_rules {
                let receipt =
                    plan_bush_fringe_with_host(world, rules, &mut preview_random, |placement| {
                        host(PlaceAllHostEvent::AddBushDoober { placement })
                    })
                    .map_err(PlaceAllError::InvalidBushFringe)?;
                bush_fringe = Some(receipt);
                let receipt = plan_mountain_rock_fringe_with_host(
                    world,
                    rules,
                    &mut preview_random,
                    |placement| host(PlaceAllHostEvent::AddMountainRockDoober { placement }),
                )
                .map_err(PlaceAllError::InvalidMountainRockFringe)?;
                mountain_rock_fringe = Some(receipt);
                if let Some(map_style) = map_style {
                    let mut preview_world = world.clone();
                    treeify_mountains = Some(
                        Self::treeify_mountains_for_map_style(
                            &mut preview_world,
                            &mut preview_random,
                            map_style,
                            rules.mountain_fringe_tree_prob,
                        )
                        .map_err(PlaceAllError::InvalidTreeifyMountains)?,
                    );
                    TerrainPlacementBoundary::PostPlacementReporting
                } else {
                    TerrainPlacementBoundary::TreeifyMountainsMapStyle
                }
            } else {
                boundary
            }
        } else {
            boundary
        };
        Err(PlaceAllError::GameplayPlacementUnavailable {
            preview: PlaceAllPreviewReceipt {
                mountain_randomization,
                group_selection,
                placement_preparation,
                bush_fringe,
                mountain_rock_fringe,
                treeify_mountains,
                region_group_prefix,
                region_group_drop,
                region_group_continuation,
                region_pattern,
                region_pattern_dispatches,
                owner_receipts: Vec::new(),
                player_group_prefix,
                player_group_mountain_retries,
                player_group_host_events,
                player_group_placed_after,
                player_group_formation_x,
                player_group_formation_y,
                player_group_dispatches,
                completed_placement_groups,
            },
            boundary,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_player_pattern_group_owned(
        group: &mut TerrainGroup,
        world: &mut World,
        random: &mut Random,
        mountains: &mut Mountains,
        prepared: &TerrainGroupPlacementPreparation,
        externals: &[PlayerGroupExternalResolution],
        owners: &mut PlaceRegionGroupOwners,
        host: &mut impl FnMut(PlaceAllHostEvent),
    ) -> Result<
        (
            PlayerPatternGroupReceipt,
            Vec<PlaceAllOwnerExecutionReceipt>,
        ),
        PlaceAllError,
    > {
        if group.group_type == 5 {
            if owners.mountains.is_none() {
                return Self::execute_player_pattern_group(
                    group, world, random, mountains, prepared, externals, host,
                )
                .map(|receipt| (receipt, Vec::new()));
            }

            let mut staged_group = group.clone();
            let mut staged_world = world.clone();
            let mut staged_random = *random;
            let mut staged_mountains = mountains.clone();
            let mut staged_owners = owners.clone();
            let runtime = staged_owners
                .mountains
                .as_mut()
                .expect("checked before staging the player mountain owner");
            let (execution, owner_receipts) = Self::execute_player_mountain_pattern_group_owned(
                &mut staged_group,
                &mut staged_world,
                &mut staged_random,
                &mut staged_mountains,
                prepared,
                runtime,
                host,
            )?;
            if execution.outcome == PlayerPatternGroupOutcome::Complete {
                *group = staged_group;
                *world = staged_world;
                *random = staged_random;
                *mountains = staged_mountains;
                *owners = staged_owners;
            }
            return Ok((execution, owner_receipts));
        }

        // Without the exact Good owner this is precisely the legacy red
        // boundary. Cliffs remain recorded-only red.
        if owners.oil_goods.is_none() {
            return Self::execute_player_pattern_group(
                group, world, random, mountains, prepared, externals, host,
            )
            .map(|receipt| (receipt, Vec::new()));
        }

        let group_before = group.clone();
        let world_before = world.clone();
        let random_before = *random;
        let mountains_before = mountains.clone();
        let mut staged_owners = owners.clone();
        let mut resolutions = externals.to_vec();
        let mut owner_receipts = Vec::new();
        let attempt_limit = world.wdata.len().saturating_mul(2).saturating_add(16);

        for _ in 0..attempt_limit {
            let mut attempt_group = group_before.clone();
            let mut attempt_world = world_before.clone();
            let mut attempt_random = random_before;
            let mut attempt_mountains = mountains_before.clone();
            let mut discard_host = |_| {};
            let execution = Self::execute_player_pattern_group(
                &mut attempt_group,
                &mut attempt_world,
                &mut attempt_random,
                &mut attempt_mountains,
                prepared,
                &resolutions,
                &mut discard_host,
            )?;

            let outcome = execution.outcome.clone();
            let PlayerPatternGroupOutcome::ExternalResolutionRequired {
                clump_index,
                player_index,
                request:
                    PlayerGroupExternalRequest::OilGoodMutation {
                        world_x,
                        world_y,
                        enabled,
                        good_type,
                        coord_x,
                        coord_y,
                    },
            } = outcome
            else {
                for &event in &execution.host_events {
                    host(event);
                }
                if execution.outcome == PlayerPatternGroupOutcome::Complete {
                    *group = attempt_group;
                    *world = attempt_world;
                    *random = attempt_random;
                    *mountains = attempt_mountains;
                    *owners = staged_owners;
                }
                return Ok((execution, owner_receipts));
            };

            let request = PlayerGroupExternalRequest::OilGoodMutation {
                world_x,
                world_y,
                enabled,
                good_type,
                coord_x,
                coord_y,
            };
            let goods = staged_owners
                .oil_goods
                .as_mut()
                .expect("checked before the owner loop");
            let execution = apply_world_set_oil_at(
                &mut attempt_world,
                goods,
                OilGoodMutation {
                    world_x,
                    world_y,
                    enabled,
                    good_type,
                    coord_x,
                    coord_y,
                },
            )
            .map_err(PlaceAllError::InvalidOwnedOilGoodMutation)?;
            owner_receipts.push(PlaceAllOwnerExecutionReceipt {
                group_index: prepared.group_index,
                source: PlaceAllOwnerSource::Player {
                    clump_index,
                    player_index,
                },
                execution: PlaceRegionGroupOwnerReceipt::OilGood(execution),
            });
            resolutions.push(PlayerGroupExternalResolution::OilGoodsApplied { request });
        }

        Err(PlaceAllError::InvalidPlayerGroupInputs {
            group_index: prepared.group_index,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_player_mountain_pattern_group_owned(
        group: &mut TerrainGroup,
        world: &mut World,
        random: &mut Random,
        mountains: &mut Mountains,
        prepared: &TerrainGroupPlacementPreparation,
        runtime: &mut MountainAddRuntime,
        host: &mut impl FnMut(PlaceAllHostEvent),
    ) -> Result<
        (
            PlayerPatternGroupReceipt,
            Vec<PlaceAllOwnerExecutionReceipt>,
        ),
        PlaceAllError,
    > {
        let group_index = prepared.group_index;
        if group.group_type != 5
            || prepared.primary_sizes.is_empty()
            || prepared.primary_sizes.len() != prepared.secondary_sizes.len()
        {
            return Err(PlaceAllError::InvalidPlayerGroupInputs { group_index });
        }

        let mut receipt = PlayerPatternGroupReceipt {
            group_index,
            calls: Vec::new(),
            mountain_retries: Vec::new(),
            host_events: Vec::new(),
            placed_after: group.placed.clone(),
            formation_x_after: Vec::new(),
            formation_y_after: Vec::new(),
            external_resolutions_consumed: 0,
            outcome: PlayerPatternGroupOutcome::Complete,
            rng_state_after: random.state(),
        };
        if world.start_x.items.is_empty() {
            return Ok((receipt, Vec::new()));
        }

        let mut resolver = OwnedPlayerMountainResolver {
            runtime,
            receipts: Vec::new(),
        };
        let mut owner_receipts = Vec::new();

        'clumps: for (clump_index, (&target_tiles, &oil_deposits)) in prepared
            .primary_sizes
            .iter()
            .zip(&prepared.secondary_sizes)
            .enumerate()
        {
            for player_index in 0..world.start_x.items.len() {
                let event = PlaceAllHostEvent::NetDaemonProcessAllPlayer {
                    group_index,
                    clump_index,
                    player_index,
                };
                host(event);
                receipt.host_events.push(event);

                let land_subtype = mountains.get_range_raw(target_tiles);
                let call = PlacePlayerGroupCall {
                    target_tiles,
                    player_index,
                    land_subtype,
                    oil_deposits,
                    group_index,
                    strict_type_four: false,
                };
                let first = group
                    .apply_place_player_group_with_mountain_resolver(
                        world,
                        random,
                        call,
                        &mut receipt.formation_x_after,
                        &mut receipt.formation_y_after,
                        &[],
                        &mut resolver,
                    )
                    .map_err(PlaceAllError::InvalidPlayerGroupPrefix)?;
                let mut outcome = first.outcome.clone();
                receipt.calls.push(first);

                if let PlacePlayerGroupOutcome::Returned(0) = outcome {
                    let retry = group
                        .apply_player_mountain_template_retry_with_resolver(
                            world,
                            random,
                            mountains,
                            call,
                            land_subtype,
                            &mut receipt.formation_x_after,
                            &mut receipt.formation_y_after,
                            &[],
                            &mut resolver,
                            &mut || {
                                let event = PlaceAllHostEvent::NetDaemonProcessAllPlayer {
                                    group_index,
                                    clump_index,
                                    player_index,
                                };
                                host(event);
                                receipt.host_events.push(event);
                            },
                        )
                        .map_err(PlaceAllError::InvalidPlayerMountainTemplateRetry)?;
                    receipt.calls.extend(
                        retry
                            .attempts
                            .iter()
                            .map(|attempt| attempt.placement.clone()),
                    );
                    outcome = retry.outcome.clone();
                    receipt.mountain_retries.push(retry);
                }

                owner_receipts.extend(resolver.receipts.drain(..).map(|execution| {
                    PlaceAllOwnerExecutionReceipt {
                        group_index,
                        source: PlaceAllOwnerSource::Player {
                            clump_index,
                            player_index,
                        },
                        execution: PlaceRegionGroupOwnerReceipt::Mountain(execution),
                    }
                }));

                match outcome {
                    PlacePlayerGroupOutcome::Returned(return_value) => {
                        group.placed.push(return_value);
                    }
                    PlacePlayerGroupOutcome::ExternalResolutionRequired { request } => {
                        receipt.outcome = PlayerPatternGroupOutcome::ExternalResolutionRequired {
                            clump_index,
                            player_index,
                            request,
                        };
                        break 'clumps;
                    }
                    PlacePlayerGroupOutcome::GrowthKernel { .. } => {
                        receipt.outcome = PlayerPatternGroupOutcome::GrowthKernel {
                            clump_index,
                            player_index,
                        };
                        break 'clumps;
                    }
                }
            }
        }

        receipt.placed_after = group.placed.clone();
        receipt.rng_state_after = random.state();
        Ok((receipt, owner_receipts))
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_player_pattern_group(
        group: &mut TerrainGroup,
        world: &mut World,
        random: &mut Random,
        mountains: &mut Mountains,
        prepared: &TerrainGroupPlacementPreparation,
        externals: &[PlayerGroupExternalResolution],
        host: &mut impl FnMut(PlaceAllHostEvent),
    ) -> Result<PlayerPatternGroupReceipt, PlaceAllError> {
        let group_index = prepared.group_index;
        if prepared.primary_sizes.is_empty()
            || prepared.primary_sizes.len() != prepared.secondary_sizes.len()
        {
            return Err(PlaceAllError::InvalidPlayerGroupInputs { group_index });
        }

        let mut receipt = PlayerPatternGroupReceipt {
            group_index,
            calls: Vec::new(),
            mountain_retries: Vec::new(),
            host_events: Vec::new(),
            placed_after: group.placed.clone(),
            formation_x_after: Vec::new(),
            formation_y_after: Vec::new(),
            external_resolutions_consumed: 0,
            outcome: PlayerPatternGroupOutcome::Complete,
            rng_state_after: random.state(),
        };
        if world.start_x.items.is_empty() {
            return Ok(receipt);
        }

        'clumps: for (clump_index, (&target_tiles, &oil_deposits)) in prepared
            .primary_sizes
            .iter()
            .zip(&prepared.secondary_sizes)
            .enumerate()
        {
            for player_index in 0..world.start_x.items.len() {
                let event = PlaceAllHostEvent::NetDaemonProcessAllPlayer {
                    group_index,
                    clump_index,
                    player_index,
                };
                host(event);
                receipt.host_events.push(event);

                let land_subtype = if group.group_type == 5 {
                    mountains.get_range_raw(target_tiles)
                } else {
                    target_tiles
                };
                let first = group
                    .apply_place_player_group(
                        world,
                        random,
                        PlacePlayerGroupCall {
                            target_tiles,
                            player_index,
                            land_subtype,
                            oil_deposits,
                            group_index,
                            strict_type_four: group.group_type == 4,
                        },
                        &mut receipt.formation_x_after,
                        &mut receipt.formation_y_after,
                        &externals[receipt.external_resolutions_consumed..],
                    )
                    .map_err(PlaceAllError::InvalidPlayerGroupPrefix)?;
                receipt.external_resolutions_consumed += first.external_resolutions_consumed;
                let mut outcome = first.outcome.clone();
                receipt.calls.push(first);

                if group.group_type == 4 && matches!(outcome, PlacePlayerGroupOutcome::Returned(0))
                {
                    let retry = group
                        .apply_place_player_group(
                            world,
                            random,
                            PlacePlayerGroupCall {
                                target_tiles,
                                player_index,
                                land_subtype,
                                oil_deposits,
                                group_index,
                                strict_type_four: false,
                            },
                            &mut receipt.formation_x_after,
                            &mut receipt.formation_y_after,
                            &externals[receipt.external_resolutions_consumed..],
                        )
                        .map_err(PlaceAllError::InvalidPlayerGroupPrefix)?;
                    receipt.external_resolutions_consumed += retry.external_resolutions_consumed;
                    outcome = retry.outcome.clone();
                    receipt.calls.push(retry);
                }

                match outcome {
                    PlacePlayerGroupOutcome::ExternalResolutionRequired { request } => {
                        receipt.outcome = PlayerPatternGroupOutcome::ExternalResolutionRequired {
                            clump_index,
                            player_index,
                            request,
                        };
                        break 'clumps;
                    }
                    PlacePlayerGroupOutcome::GrowthKernel { .. } => {
                        receipt.outcome = PlayerPatternGroupOutcome::GrowthKernel {
                            clump_index,
                            player_index,
                        };
                        break 'clumps;
                    }
                    PlacePlayerGroupOutcome::Returned(return_value) => {
                        if group.group_type == 5 && return_value == 0 {
                            let retry = group
                                .apply_player_mountain_template_retry(
                                    world,
                                    random,
                                    mountains,
                                    PlacePlayerGroupCall {
                                        target_tiles,
                                        player_index,
                                        land_subtype,
                                        oil_deposits,
                                        group_index,
                                        strict_type_four: false,
                                    },
                                    land_subtype,
                                    &mut receipt.formation_x_after,
                                    &mut receipt.formation_y_after,
                                    &externals[receipt.external_resolutions_consumed..],
                                    || {
                                        let event = PlaceAllHostEvent::NetDaemonProcessAllPlayer {
                                            group_index,
                                            clump_index,
                                            player_index,
                                        };
                                        host(event);
                                        receipt.host_events.push(event);
                                    },
                                )
                                .map_err(PlaceAllError::InvalidPlayerMountainTemplateRetry)?;
                            receipt.external_resolutions_consumed +=
                                retry.external_resolutions_consumed;
                            receipt.calls.extend(
                                retry
                                    .attempts
                                    .iter()
                                    .map(|attempt| attempt.placement.clone()),
                            );
                            outcome = retry.outcome.clone();
                            receipt.mountain_retries.push(retry);
                        }
                        match outcome {
                            PlacePlayerGroupOutcome::Returned(return_value) => {
                                group.placed.push(return_value);
                            }
                            PlacePlayerGroupOutcome::ExternalResolutionRequired { request } => {
                                receipt.outcome =
                                    PlayerPatternGroupOutcome::ExternalResolutionRequired {
                                        clump_index,
                                        player_index,
                                        request,
                                    };
                                break 'clumps;
                            }
                            PlacePlayerGroupOutcome::GrowthKernel { .. } => {
                                receipt.outcome = PlayerPatternGroupOutcome::GrowthKernel {
                                    clump_index,
                                    player_index,
                                };
                                break 'clumps;
                            }
                        }
                    }
                }
            }
        }

        receipt.placed_after = group.placed.clone();
        receipt.rng_state_after = random.state();
        Ok(receipt)
    }

    /// Exact placement preparation beginning with the former host boundary at
    /// `0x006a7645` and ending at the first path-specific gameplay dependency.
    ///
    /// Each visited group emits its daemon pump before any group read.  Selected
    /// groups generate one primary size per positive clump count; type 6 also
    /// generates its secondary size.  Pattern-0/type-4 primary sizes are rounded
    /// up to odd and capped at nine.  Unsupported pattern values are native no-ops
    /// and therefore do not stop later groups from being visited.
    pub fn prepare_placement_prefix(
        &self,
        selections: &[TerrainGroupSelection],
        random: &mut Random,
        progress: i32,
        place_players: i32,
        mut host: impl FnMut(PlaceAllHostEvent),
    ) -> Result<
        (TerrainPlacementPreparationReceipt, TerrainPlacementBoundary),
        TerrainPlacementPreparationError,
    > {
        let mut receipt = TerrainPlacementPreparationReceipt {
            host_events: Vec::new(),
            prepared_groups: Vec::new(),
            primary_size_draws: 0,
            secondary_size_draws: 0,
            rng_state_after: random.state(),
        };
        let boundary = self.prepare_placement_continuation(
            selections,
            random,
            progress,
            place_players,
            0,
            &mut receipt,
            &mut host,
        )?;
        Ok((receipt, boundary))
    }

    /// Common `place_all` group-index dispatcher at `0x006a8ee5` ->
    /// `0x006a7640`. The caller supplies the first group not yet visited and an
    /// existing receipt/RNG state from the completed placement arm.
    #[allow(clippy::too_many_arguments)]
    fn prepare_placement_continuation(
        &self,
        selections: &[TerrainGroupSelection],
        random: &mut Random,
        progress: i32,
        place_players: i32,
        first_group: usize,
        receipt: &mut TerrainPlacementPreparationReceipt,
        host: &mut impl FnMut(PlaceAllHostEvent),
    ) -> Result<TerrainPlacementBoundary, TerrainPlacementPreparationError> {
        let normalized =
            self.validate_and_normalize_placement_inputs(selections, place_players, first_group)?;

        for (group_index, ((group, selection), bounds)) in self
            .groups
            .iter()
            .zip(selections)
            .zip(normalized)
            .enumerate()
            .skip(first_group)
        {
            emit_host_event(
                receipt,
                host,
                PlaceAllHostEvent::NetDaemonProcessAll { group_index },
            );
            if !selection.selected {
                continue;
            }

            let count = selection.clumps.max(0) as usize;
            let mut primary_sizes = Vec::with_capacity(count);
            let mut secondary_sizes = Vec::with_capacity(count);
            for _ in 0..count {
                let mut primary = bounds.primary_min;
                if bounds.primary_min < bounds.primary_max {
                    let span = bounds
                        .primary_max
                        .wrapping_sub(bounds.primary_min)
                        .wrapping_add(1);
                    primary = (random.get(0, 0xffff) % span).wrapping_add(bounds.primary_min);
                    receipt.primary_size_draws += 1;
                }
                if group.pattern == 0 && group.group_type == 4 {
                    let rounded = if primary & 1 == 0 {
                        primary.wrapping_add(1)
                    } else {
                        primary
                    };
                    primary = rounded.min(9);
                }
                primary_sizes.push(primary);

                let secondary = if group.group_type == 6 {
                    let mut value = bounds.secondary_min;
                    if bounds.secondary_min < bounds.secondary_max {
                        let span = bounds
                            .secondary_max
                            .wrapping_sub(bounds.secondary_min)
                            .wrapping_add(1);
                        value = (random.get(0, 0xffff) % span).wrapping_add(bounds.secondary_min);
                        receipt.secondary_size_draws += 1;
                    }
                    value
                } else {
                    0
                };
                secondary_sizes.push(secondary);
            }

            receipt
                .prepared_groups
                .push(TerrainGroupPlacementPreparation {
                    group_index,
                    group_type: group.group_type,
                    pattern: group.pattern,
                    primary_min: bounds.primary_min,
                    primary_max: bounds.primary_max,
                    secondary_min: bounds.secondary_min,
                    secondary_max: bounds.secondary_max,
                    primary_sizes,
                    secondary_sizes,
                });
            receipt.rng_state_after = random.state();

            let boundary = match group.pattern {
                0 if group.group_type != 7 => {
                    if progress != 0 {
                        emit_host_event(
                            receipt,
                            host,
                            PlaceAllHostEvent::ProgressDisplay {
                                group_index,
                                pattern: group.pattern,
                            },
                        );
                    }
                    if place_players != 0 && selection.clumps > 0 {
                        Some(TerrainPlacementBoundary::PlayerRosterAndPlacementKernel {
                            group_index,
                        })
                    } else {
                        None
                    }
                }
                1..=3 => {
                    if progress != 0 {
                        emit_host_event(
                            receipt,
                            host,
                            PlaceAllHostEvent::ProgressDisplay {
                                group_index,
                                pattern: group.pattern,
                            },
                        );
                    }
                    Some(
                        TerrainPlacementBoundary::UnitTypeCatalogAndRegionPlacementKernel {
                            group_index,
                            pattern: group.pattern,
                        },
                    )
                }
                _ => None,
            };
            if let Some(boundary) = boundary {
                receipt.rng_state_after = random.state();
                return Ok(boundary);
            }
        }

        receipt.rng_state_after = random.state();
        Ok(TerrainPlacementBoundary::AddDoobers)
    }

    /// Exact terrain-group chance/clump-selection transaction from
    /// `0x006a7445` through the five-type normalization ending at `0x006a7615`.
    ///
    /// A zero `grouping` always starts a new percentile draw.  Adjacent groups
    /// with the same nonzero `grouping` carry the signed remainder and the prior
    /// selected flag.  The short-circuit when that remainder is already negative
    /// is observable: it can reject without subtracting the current chance.
    /// Selected groups draw an inclusive clump count only when `min < max`.
    /// Structural validation precedes the first draw, so errors are transactional.
    pub fn select_groups(
        &self,
        random: &mut Random,
    ) -> Result<TerrainGroupSelectionReceipt, TerrainGroupSelectionError> {
        self.validate_selection_inputs()?;

        let mut selections = vec![TerrainGroupSelection::default(); self.groups.len()];
        let mut raw_clumps_by_type = [0i32; 5];
        let mut last_grouping = -10i32;
        let mut remaining_percentile = 0i32;
        let mut previous_selected = false;
        let mut chance_draws = 0u32;
        let mut clump_draws = 0u32;

        for (index, group) in self.groups.iter().enumerate() {
            if group.grouping == 0 || group.grouping != last_grouping {
                remaining_percentile = random.get(0, 0xffff) % 100;
                chance_draws += 1;
                last_grouping = group.grouping;
                previous_selected = false;
            }

            let reject_without_subtraction =
                remaining_percentile < 0 && (group.chance != 0 || !previous_selected);
            let rejected = if reject_without_subtraction {
                true
            } else {
                remaining_percentile = remaining_percentile.wrapping_sub(group.chance);
                remaining_percentile >= 0
            };

            if rejected {
                previous_selected = false;
                continue;
            }

            previous_selected = true;
            let mut clumps = group.min_clumps;
            if group.min_clumps < group.max_clumps {
                let span = group
                    .max_clumps
                    .wrapping_sub(group.min_clumps)
                    .wrapping_add(1);
                clumps = (random.get(0, 0xffff) % span).wrapping_add(group.min_clumps);
                clump_draws += 1;
            }
            selections[index] = TerrainGroupSelection {
                selected: true,
                clumps,
            };
            let type_index = (group.group_type - 4) as usize;
            raw_clumps_by_type[type_index] = raw_clumps_by_type[type_index].wrapping_add(clumps);
        }

        let normalized_clumps_by_type = raw_clumps_by_type.map(|total| {
            if total < 2 {
                i32::MAX
            } else {
                let half = total >> 1;
                if half & 1 != 0 {
                    half - 1
                } else {
                    half
                }
            }
        });

        Ok(TerrainGroupSelectionReceipt {
            groups: selections,
            raw_clumps_by_type,
            normalized_clumps_by_type,
            chance_draws,
            clump_draws,
            rng_state_after: random.state(),
        })
    }

    fn validate_selection_inputs(&self) -> Result<(), TerrainGroupSelectionError> {
        for (group_index, group) in self.groups.iter().enumerate() {
            if !(4..=8).contains(&group.group_type) {
                return Err(TerrainGroupSelectionError::UnsupportedGroupType {
                    group_index,
                    group_type: group.group_type,
                });
            }
            if group.min_clumps < group.max_clumps {
                let span = i64::from(group.max_clumps) - i64::from(group.min_clumps) + 1;
                if span > i64::from(i32::MAX) {
                    return Err(TerrainGroupSelectionError::InvalidClumpSpan {
                        group_index,
                        min_clumps: group.min_clumps,
                        max_clumps: group.max_clumps,
                    });
                }
            }
        }
        Ok(())
    }

    fn validate_and_normalize_placement_inputs(
        &self,
        selections: &[TerrainGroupSelection],
        place_players: i32,
        first_group: usize,
    ) -> Result<Vec<PlacementBounds>, TerrainPlacementPreparationError> {
        if selections.len() != self.groups.len() {
            return Err(TerrainPlacementPreparationError::SelectionLengthMismatch {
                groups: self.groups.len(),
                selections: selections.len(),
            });
        }

        let mut reachable = true;
        self.groups
            .iter()
            .zip(selections)
            .enumerate()
            .map(|(group_index, (group, selection))| {
                let in_continuation = group_index >= first_group;
                let primary_min = group.min_size.min(group.max_size);
                let mut primary_max = group.min_size.max(group.max_size);
                if group.group_type == 5 {
                    primary_max = primary_max.min(3);
                } else if group.group_type == 8 {
                    primary_max = primary_max.min(5);
                }

                let mut secondary_min = group.min_oil.min(group.max_oil);
                let mut secondary_max = group.min_oil.max(group.max_oil);
                if group.group_type == 6 {
                    secondary_max = secondary_max.min(primary_max);
                    secondary_min = secondary_min.max(0);
                }

                if in_continuation
                    && reachable
                    && selection.selected
                    && !positive_i32_inclusive_span(primary_min, primary_max)
                {
                    return Err(TerrainPlacementPreparationError::InvalidPrimarySizeSpan {
                        group_index,
                        min_size: primary_min,
                        max_size: primary_max,
                    });
                }
                if in_continuation
                    && reachable
                    && selection.selected
                    && group.group_type == 6
                    && !positive_i32_inclusive_span(secondary_min, secondary_max)
                {
                    return Err(TerrainPlacementPreparationError::InvalidSecondarySizeSpan {
                        group_index,
                        min_size: secondary_min,
                        max_size: secondary_max,
                    });
                }

                let bounds = PlacementBounds {
                    primary_min,
                    primary_max,
                    secondary_min,
                    secondary_max,
                };
                if in_continuation
                    && selection.selected
                    && (matches!(group.pattern, 1..=3)
                        || (group.pattern == 0
                            && group.group_type != 7
                            && place_players != 0
                            && selection.clumps > 0))
                {
                    reachable = false;
                }
                Ok(bounds)
            })
            .collect()
    }

    fn validate_fractal_accesses(&self, world: &World) -> Result<(), FillFertileError> {
        for x in 0..world.xs {
            for y in 0..world.ys {
                if world.wdata(x, y).land != 0 {
                    continue;
                }
                let Some(column) = self.fractal.columns.get(x as usize) else {
                    return Err(FillFertileError::MissingFractalColumn {
                        x,
                        required_columns: x + 1,
                        actual_columns: self.fractal.columns.len(),
                    });
                };
                if column.len() <= y as usize {
                    return Err(FillFertileError::ShortFractalColumn {
                        x,
                        required_rows: y + 1,
                        actual_rows: column.len(),
                    });
                }
            }
        }
        Ok(())
    }
}

fn positive_i32_inclusive_span(minimum: i32, maximum: i32) -> bool {
    minimum >= maximum || i64::from(maximum) - i64::from(minimum) + 1 <= i64::from(i32::MAX)
}

fn emit_host_event(
    receipt: &mut TerrainPlacementPreparationReceipt,
    host: &mut impl FnMut(PlaceAllHostEvent),
    event: PlaceAllHostEvent,
) {
    receipt.host_events.push(event);
    host(event);
}

fn validate_world(world: &World) -> Result<(), FillFertileError> {
    let expected = world.xs.checked_mul(world.ys);
    if world.xs < 0
        || world.ys < 0
        || expected != Some(world.size)
        || usize::try_from(world.size).ok() != Some(world.wdata.len())
    {
        return Err(FillFertileError::InvalidWorldShape {
            xs: world.xs,
            ys: world.ys,
            size: world.size,
            wdata_len: world.wdata.len(),
        });
    }
    Ok(())
}

fn validate_placement_reporting_inputs(
    inputs: PlacementReportingInputs,
) -> Result<(), PlacementReportingError> {
    if inputs.num_players > 8 {
        return Err(PlacementReportingError::UnsupportedPlayerCount {
            num_players: inputs.num_players,
            supported: 8,
        });
    }
    Ok(())
}

fn validate_treeify_world(world: &World) -> Result<(), TreeifyMountainsError> {
    let world_size = world.xs.checked_mul(world.ys);
    let tile_xs = world.xs.checked_mul(4);
    let tile_ys = world.ys.checked_mul(4);
    let tile_size = world.tile_xs.checked_mul(world.tile_ys);
    if world.xs < 0
        || world.ys < 0
        || world_size != Some(world.size)
        || usize::try_from(world.size).ok() != Some(world.wdata.len())
        || tile_xs != Some(world.tile_xs)
        || tile_ys != Some(world.tile_ys)
        || tile_size != Some(world.tile_size)
        || usize::try_from(world.tile_size).ok() != Some(world.tdata.len())
    {
        return Err(TreeifyMountainsError::InvalidWorldShape {
            xs: world.xs,
            ys: world.ys,
            size: world.size,
            wdata_len: world.wdata.len(),
            tile_xs: world.tile_xs,
            tile_ys: world.tile_ys,
            tile_size: world.tile_size,
            tdata_len: world.tdata.len(),
        });
    }
    Ok(())
}

fn has_mountain_tcoords(world: &World, x: i32, y: i32, queries: &mut u32) -> bool {
    *queries += 1;
    let tile_x = x * 4;
    let tile_y = y * 4;
    for local_y in 0..4 {
        for local_x in 0..4 {
            let index = (tile_y + local_y) * world.tile_xs + tile_x + local_x;
            if world.tdata[index as usize] & tflag::BLOCKER_MASK == tflag::BLOCKER_MOUNTAIN {
                return true;
            }
        }
    }
    false
}

fn apply_treeify_mutation(
    world: &mut World,
    x: i32,
    y: i32,
    kind: TreeifyMutationKind,
    class_flags: u16,
    receipt: &mut TreeifyMountainsReceipt,
) {
    let cell = world.wdata_mut(x, y);
    let flags_before = cell.flags;
    let land_before = cell.land;
    cell.flags &= !wflag::LAND_CLASS_MASK;
    cell.land = land::FERTILE;
    cell.flags |= class_flags;
    receipt.mutations.push(TreeifyMutation {
        world_x: x,
        world_y: y,
        kind,
        flags_before,
        flags_after: cell.flags,
        land_before,
    });
}
