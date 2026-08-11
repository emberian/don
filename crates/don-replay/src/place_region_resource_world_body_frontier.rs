// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact concrete-good `World`-pattern body of `Map::place_region_resource`.
//!
//! The registered category owner reaches `0x00690770` after choosing the first
//! available region and the first point.  This owner continues that exact seam
//! through every candidate filter, the native point permutation, all subsequent
//! point draws, `Objects::init_good`, the per-start saturation accumulator, the
//! circular region walk, and the returned placement count.  Pool selectors,
//! items, and patterns other than `World` remain typed boundaries.

use std::collections::BTreeMap;

use don_sim::rng::Random;
use don_sim::systems::map_terrain::{WorldChecksum, WorldSection};

use crate::place_resources_category_frontier::{
    DeterministicResourceState, ExactRandomDraw, RegionPlacementPrefixReceipt,
    RegionPrefixDisposition, RANDOM_GET_VA, REGION_CANDIDATE_SCAN_VA, REGION_RANDOM_GET_CALL_VA,
    SHIPPED_EXE_SHA256, SHIPPED_PDB_SHA256,
};

pub const WORLD_PATTERN: i32 = 1;
pub const CONCRETE_SELECTOR: i32 = 0;
pub const ITEM_GOOD_ID: i32 = 0x21f;

pub const PRIMARY_START_OCCUPIED_FILTER_VA: u32 = 0x0069_07a0;
pub const PLAYER_KEEP_AWAY_FILTER_VA: u32 = 0x0069_07b7;
pub const SHADOW_CELL_FILTER_VA: u32 = 0x0069_08b7;
pub const PLAYER_STAY_NEAR_FILTER_VA: u32 = 0x0069_08f3;
pub const CENTER_KEEP_AWAY_FILTER_VA: u32 = 0x0069_09c2;
pub const CENTER_STAY_NEAR_FILTER_VA: u32 = 0x0069_0a80;
pub const EDGE_KEEP_AWAY_FILTER_VA: u32 = 0x0069_0b31;
pub const EDGE_STAY_NEAR_FILTER_VA: u32 = 0x0069_0b78;
pub const CORNER_KEEP_AWAY_FILTER_VA: u32 = 0x0069_0bb9;
pub const CORNER_STAY_NEAR_FILTER_VA: u32 = 0x0069_0ca4;
pub const GROUP_SPACING_FILTER_VA: u32 = 0x0069_0d93;
pub const EXISTING_RESOURCE_SPACING_FILTER_VA: u32 = 0x0069_0e4c;
pub const LAND_NEIGHBOR_FILTER_VA: u32 = 0x0069_1098;
pub const WATER_NEIGHBOR_FILTER_VA: u32 = 0x0069_1158;
pub const HAS_MOUNTAIN_CALL_VA: u32 = 0x0069_11c7;
pub const WORLD_DATA_HAS_MOUNTAIN_TCOORDS_VA: u32 = 0x006b_3050;
pub const BORDER_FILTER_VA: u32 = 0x0069_11db;
pub const HAS_SOUTH_FOREST_CALL_VA: u32 = 0x0069_1207;
pub const TERRAIN_HAS_SOUTH_FOREST_VA: u32 = 0x0085_0060;
pub const TERRAIN_RARE_FILTER_VA: u32 = 0x0069_1214;
pub const DOWN_CHAIN_FILTER_VA: u32 = 0x0069_1334;
pub const SATURATION_OWNER_FILTER_VA: u32 = 0x0069_137f;
pub const REGION_INIT_GOOD_CALL_VA: u32 = 0x0069_157c;
pub const OBJECTS_INIT_GOOD_VA: u32 = 0x0065_3f30;
pub const GOOD_WDATA_DOWN_WRITE_VA: u32 = 0x0065_403e;
pub const GOOD_WDATA_DOWN_WHO_WRITE_VA: u32 = 0x0065_4043;
pub const GOOD_WDATA_DOWN_MARKER: i16 = -2;
pub const ATTEMPT_TAIL_VA: u32 = 0x0069_18b5;
pub const CALLER_RETURN_ACCUMULATE_VA: u32 = 0x0069_01f1;
pub const FUNCTION_RETURN_VALUE_VA: u32 = 0x0069_1f52;

/// The native neighbor tables at `0x00adc404` / `0x00adcaf4`.
pub const NEIGHBOR_OFFSETS: [(i32, i32); 24] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -2),
    (0, -2),
    (1, -2),
    (2, -1),
    (2, 0),
    (2, 1),
    (1, 2),
    (0, 2),
    (-1, 2),
    (-2, 1),
    (-2, 0),
    (-2, -1),
    (-2, -2),
    (2, -2),
    (2, 2),
    (-2, 2),
];

/// The native corner multipliers at `0x00add2a0` / `0x00add2b0`.
pub const CORNER_MULTIPLIERS: [(i32, i32); 4] = [(0, 0), (0, 1), (1, 0), (1, 1)];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionWorldBodyState {
    pub deterministic: DeterministicResourceState,
    /// Stable digest of the checksum-relevant `Good` projection owned by the host.
    pub good_state_digest: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionWorldBodyParameters {
    pub good_id: i32,
    pub pattern: i32,
    pub saturate: i32,
    pub num_rare: i32,
    pub selector: i32,
    pub player_keep_away: i32,
    pub player_stay_near: i32,
    pub existing_resource_spacing: i32,
    pub group_spacing: i32,
    pub center_keep_away: i32,
    pub center_stay_near: i32,
    pub edge_keep_away: i32,
    pub edge_stay_near: i32,
    pub corner_keep_away: i32,
    pub corner_stay_near: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionPlayerStartFact {
    /// Native `World::start_x/start_y` values used by the first two filters.
    pub cell_x: i32,
    pub cell_y: i32,
    /// Decoded values used by the post-limit nearest-owner accumulator.
    pub metric_x: i32,
    pub metric_y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExistingResourceKind {
    Good { good_id: i32 },
    Item,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExistingResourceFact {
    pub active: bool,
    pub kind: ExistingResourceKind,
    pub cell_x: i32,
    pub cell_y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DownLinkFact {
    pub index: i16,
    pub who: i16,
    pub next_index: i16,
    pub next_who: i16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownChainFact {
    pub initial_index: i16,
    pub initial_who: i16,
    /// Complete native chain in traversal order. The final `next_index` must be negative.
    pub links: Vec<DownLinkFact>,
}

impl DownChainFact {
    fn terminal_index(&self) -> Option<i16> {
        let mut index = self.initial_index;
        let mut who = self.initial_who;
        let mut cursor = 0;
        while index >= 0 {
            let link = self.links.get(cursor)?;
            if link.index != index || link.who != who {
                return None;
            }
            index = link.next_index;
            who = link.next_who;
            cursor += 1;
        }
        (cursor == self.links.len()).then_some(index)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionWorldCellFact {
    pub x: i32,
    pub y: i32,
    /// Primary `World::WData` word and signed land byte.
    pub primary_flags: u16,
    pub primary_land: i8,
    /// The same coordinate in the secondary `World` consulted at `0x006908b7`.
    pub shadow_flags: u16,
    pub shadow_occupant: u8,
    pub down_chain: DownChainFact,
    /// Captured return values of the two named retail predicates.
    pub has_mountain_tcoords: bool,
    pub has_south_forest: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionCandidateSetFact {
    pub region_id: i32,
    /// Native point-array order at region `+0x7c`; the LFSR uses one-based indices.
    pub points: Vec<(i32, i32)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerrainRareFact {
    pub terrain_class: u8,
    pub good_ids: Vec<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegionBodyPlacementEvidence {
    RetailCapture {
        executable_sha256: String,
        capture_sha256: [u8; 32],
    },
    ExactPort {
        implementation_sha256: [u8; 32],
        proof_document: String,
    },
    SyntheticFixture {
        fixture: String,
    },
}

impl RegionBodyPlacementEvidence {
    fn admissible(&self) -> bool {
        let nonzero = |digest: &[u8; 32]| digest.iter().any(|byte| *byte != 0);
        match self {
            Self::RetailCapture {
                executable_sha256,
                capture_sha256,
            } => executable_sha256 == SHIPPED_EXE_SHA256 && nonzero(capture_sha256),
            Self::ExactPort {
                implementation_sha256,
                proof_document,
            } => nonzero(implementation_sha256) && !proof_document.is_empty(),
            Self::SyntheticFixture { fixture } => cfg!(test) && !fixture.is_empty(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegionWorldBodyEvidence {
    RetailCapture {
        executable_sha256: String,
        pdb_sha256: String,
        capture_sha256: [u8; 32],
        entry_va: u32,
        random_state: i32,
        world_checksum: WorldChecksum,
        resource_pool_digest: u64,
        good_state_digest: u64,
        facts_digest: u64,
        prefix_digest: u64,
    },
    SyntheticFixture {
        fixture: String,
        entry_va: u32,
        random_state: i32,
        world_checksum: WorldChecksum,
        resource_pool_digest: u64,
        good_state_digest: u64,
        facts_digest: u64,
        prefix_digest: u64,
    },
}

impl RegionWorldBodyEvidence {
    fn admissible(
        &self,
        state: &RegionWorldBodyState,
        facts_digest: u64,
        prefix_digest: u64,
    ) -> bool {
        let fields = |entry_va: u32,
                      random_state: i32,
                      world_checksum: &WorldChecksum,
                      resource_pool_digest: u64,
                      good_state_digest: u64,
                      actual_facts_digest: u64,
                      actual_prefix_digest: u64| {
            entry_va == REGION_CANDIDATE_SCAN_VA
                && random_state == state.deterministic.random_state
                && world_checksum == &state.deterministic.world_checksum
                && resource_pool_digest == state.deterministic.resource_pool_digest
                && good_state_digest == state.good_state_digest
                && actual_facts_digest == facts_digest
                && actual_prefix_digest == prefix_digest
        };
        match self {
            Self::RetailCapture {
                executable_sha256,
                pdb_sha256,
                capture_sha256,
                entry_va,
                random_state,
                world_checksum,
                resource_pool_digest,
                good_state_digest,
                facts_digest,
                prefix_digest,
            } => {
                executable_sha256 == SHIPPED_EXE_SHA256
                    && pdb_sha256 == SHIPPED_PDB_SHA256
                    && capture_sha256.iter().any(|byte| *byte != 0)
                    && fields(
                        *entry_va,
                        *random_state,
                        world_checksum,
                        *resource_pool_digest,
                        *good_state_digest,
                        *facts_digest,
                        *prefix_digest,
                    )
            }
            Self::SyntheticFixture {
                fixture,
                entry_va,
                random_state,
                world_checksum,
                resource_pool_digest,
                good_state_digest,
                facts_digest,
                prefix_digest,
            } => {
                cfg!(test)
                    && !fixture.is_empty()
                    && fields(
                        *entry_va,
                        *random_state,
                        world_checksum,
                        *resource_pool_digest,
                        *good_state_digest,
                        *facts_digest,
                        *prefix_digest,
                    )
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionWorldBodyFacts {
    pub world_width: i32,
    pub world_height: i32,
    /// Exactly `width * height` cells, row-major with x changing fastest.
    pub cells: Vec<RegionWorldCellFact>,
    pub starts: Vec<RegionPlayerStartFact>,
    pub existing_resources: Vec<ExistingResourceFact>,
    /// One set for each ID in the prefix receipt's `available_regions` projection.
    pub regions: Vec<RegionCandidateSetFact>,
    /// Exactly classes 0..=7 in ascending order.
    pub terrain_rare: Vec<TerrainRareFact>,
    /// Captured `coord_lookup_array` projection. Allocation coordinate `>> 8` indexes it.
    pub coord_metric_table: Vec<i32>,
    /// Good type `+0x234 & 1`; false selects `+0x180`, true selects `+0x120`.
    pub good_uses_offset_0x120: bool,
    pub placement_evidence: RegionBodyPlacementEvidence,
    pub evidence: RegionWorldBodyEvidence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionFilterKind {
    PrimaryStartOccupied,
    PlayerKeepAway,
    ShadowFlags10,
    ShadowFlags68,
    ShadowFlags1800,
    ShadowOccupant,
    PlayerStayNear,
    CenterKeepAway,
    CenterStayNear,
    EdgeKeepAway,
    EdgeStayNear,
    CornerKeepAway,
    CornerStayNear,
    GroupSpacing,
    ExistingResourceSpacing,
    LandNeighbor,
    WaterNeighbor,
    HasMountainTcoords,
    Border,
    HasSouthForest,
    TerrainRare,
    DownChain,
    SaturationOwner,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionFilterObservation {
    pub stage_va: u32,
    pub kind: RegionFilterKind,
    /// Player/resource/corner/neighbor index when the stage iterates a collection.
    pub subject_index: Option<usize>,
    pub observed: i32,
    pub threshold_or_mask: i32,
    pub passed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionCandidateEvaluation {
    pub one_based_point_index: i32,
    pub x: i32,
    pub y: i32,
    pub observations: Vec<RegionFilterObservation>,
    pub rejection: Option<RegionFilterKind>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionBodyRandomDraw {
    pub call_va: u32,
    pub random_get_va: u32,
    pub low: i32,
    pub high: i32,
    pub state_before: i32,
    pub raw: i32,
    pub modulus: i32,
    pub remainder: i32,
    pub state_after: i32,
}

impl From<ExactRandomDraw> for RegionBodyRandomDraw {
    fn from(draw: ExactRandomDraw) -> Self {
        Self {
            call_va: draw.call_va,
            random_get_va: draw.random_get_va,
            low: draw.low,
            high: draw.high,
            state_before: draw.state_before,
            raw: draw.raw,
            modulus: draw.modulus,
            remainder: draw.remainder,
            state_after: draw.state_after,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionAttemptReceipt {
    pub region_pass: usize,
    pub region_id: i32,
    pub attempt_in_region: i32,
    pub point_draw: Option<RegionBodyRandomDraw>,
    pub starting_one_based_point_index: i32,
    pub candidates: Vec<RegionCandidateEvaluation>,
    pub allocation_index: Option<usize>,
    pub attempt_tail_va: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionWorldOccupancyWrite {
    pub world_x: i32,
    pub world_y: i32,
    pub down_write_va: u32,
    pub down_who_write_va: u32,
    pub old_down: i16,
    pub old_down_who: i16,
    pub new_down: i16,
    pub new_down_who: i16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionGoodAllocation {
    pub call_va: u32,
    pub callee_va: u32,
    pub good_id: i32,
    pub coord_x: i32,
    pub coord_y: i32,
    pub slot: i32,
    /// `Objects::init_good(5, ...)` deliberately performs no WData down write.
    pub occupancy_write: Option<RegionWorldOccupancyWrite>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionInitGoodRequest {
    pub call_va: u32,
    pub callee_va: u32,
    pub good_id: i32,
    pub world_x: i32,
    pub world_y: i32,
    pub coord_x: i32,
    pub coord_y: i32,
    pub old_down: i16,
    pub old_down_who: i16,
    pub world_checksum_before: WorldChecksum,
    pub good_state_digest_before: u64,
    pub sourced_walked_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionInitGoodReceipt {
    pub request: RegionInitGoodRequest,
    pub allocation: RegionGoodAllocation,
    pub world_checksum_after: WorldChecksum,
    pub good_state_digest_after: u64,
    pub sourced_walked_bytes_after: u64,
}

/// Observational/two-phase host. No external mutation may be committed before this
/// owner has accepted the complete function receipt.
pub trait RegionInitGoodHost {
    fn propose_init_good(
        &mut self,
        request: &RegionInitGoodRequest,
    ) -> Option<RegionInitGoodReceipt>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionWorldBodyReceipt {
    pub entry_va: u32,
    pub function_return_value_va: u32,
    pub caller_return_accumulate_va: u32,
    pub parameters: RegionWorldBodyParameters,
    pub water_good: bool,
    pub circular_region_order: Vec<i32>,
    pub random_draws: Vec<RegionBodyRandomDraw>,
    pub attempts: Vec<RegionAttemptReceipt>,
    pub allocations: Vec<RegionGoodAllocation>,
    pub start_distance_accumulator: Vec<i32>,
    pub preferred_start_index: usize,
    pub return_count: i32,
    pub deterministic_before: DeterministicResourceState,
    pub deterministic_after: DeterministicResourceState,
    pub good_state_digest_before: u64,
    pub good_state_digest_after: u64,
    pub placement_evidence: RegionBodyPlacementEvidence,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegionWorldBodyError {
    UnsupportedPattern,
    UnsupportedSelector,
    UnsupportedGood,
    WrongPrefix,
    StaleEvidence,
    InvalidWorldShape,
    InvalidCellCatalog,
    InvalidStartCatalog,
    InvalidRegionCatalog,
    InvalidTerrainCatalog,
    InvalidMetricTable,
    InvalidDownChain { x: i32, y: i32 },
    InvalidPlacementEvidence,
    AllocationUnavailable { region_id: i32, x: i32, y: i32 },
    InvalidAllocationReceipt { index: usize },
}

fn digest_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

fn digest_i32(hash: &mut u64, value: i32) {
    digest_bytes(hash, &value.to_le_bytes());
}

fn digest_u64(hash: &mut u64, value: u64) {
    digest_bytes(hash, &value.to_le_bytes());
}

fn digest_random_draw(hash: &mut u64, draw: &ExactRandomDraw) {
    for value in [
        draw.call_va as i32,
        draw.random_get_va as i32,
        draw.low,
        draw.high,
        draw.state_before,
        draw.raw,
        draw.modulus,
        draw.remainder,
        draw.state_after,
    ] {
        digest_i32(hash, value);
    }
}

/// Stable binding of the registered prefix receipt accepted by this continuation.
pub fn region_placement_prefix_receipt_digest(prefix: &RegionPlacementPrefixReceipt) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for value in [
        prefix.entry_va as i32,
        prefix.find_avail_regions_call_va as i32,
        prefix.find_avail_regions_va as i32,
        prefix.water_good as i32,
        prefix
            .first_region_index
            .map(|value| value as i32)
            .unwrap_or(-1),
        prefix.region_pass_count,
        prefix.per_region_limit,
        prefix.random_state_before,
        prefix.random_state_after,
    ] {
        digest_i32(&mut hash, value);
    }
    digest_u64(&mut hash, prefix.available_regions.len() as u64);
    for region_id in &prefix.available_regions {
        digest_i32(&mut hash, *region_id);
    }
    digest_bytes(&mut hash, &[prefix.find_avail_draw.is_some() as u8]);
    if let Some(draw) = &prefix.find_avail_draw {
        digest_random_draw(&mut hash, draw);
    }
    digest_bytes(&mut hash, &[prefix.point_draw.is_some() as u8]);
    if let Some(draw) = &prefix.point_draw {
        digest_random_draw(&mut hash, draw);
    }
    match prefix.disposition {
        RegionPrefixDisposition::NoAvailableRegions { return_path_va } => {
            digest_bytes(&mut hash, &[0]);
            digest_i32(&mut hash, return_path_va as i32);
        }
        RegionPrefixDisposition::NoRequestedResources { residual_va } => {
            digest_bytes(&mut hash, &[1]);
            digest_i32(&mut hash, residual_va as i32);
        }
        RegionPrefixDisposition::PoolSelectionBoundary {
            call_va,
            selected_region,
        } => {
            digest_bytes(&mut hash, &[2]);
            digest_i32(&mut hash, call_va as i32);
            digest_i32(&mut hash, selected_region);
        }
        RegionPrefixDisposition::CandidateScan {
            residual_va,
            selected_region,
            point_index,
        } => {
            digest_bytes(&mut hash, &[3]);
            digest_i32(&mut hash, residual_va as i32);
            digest_i32(&mut hash, selected_region);
            digest_i32(&mut hash, point_index);
        }
    }
    digest_i32(&mut hash, prefix.world_checksum.full as i32);
    digest_u64(&mut hash, prefix.world_checksum.bytes);
    for section in &prefix.world_checksum.per_section {
        digest_i32(&mut hash, section.adler as i32);
        digest_u64(&mut hash, section.bytes);
    }
    hash
}

/// Stable logical digest of every behavior-driving fact (excluding capture evidence).
pub fn region_world_body_facts_digest(
    parameters: RegionWorldBodyParameters,
    facts: &RegionWorldBodyFacts,
) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for value in [
        parameters.good_id,
        parameters.pattern,
        parameters.saturate,
        parameters.num_rare,
        parameters.selector,
        parameters.player_keep_away,
        parameters.player_stay_near,
        parameters.existing_resource_spacing,
        parameters.group_spacing,
        parameters.center_keep_away,
        parameters.center_stay_near,
        parameters.edge_keep_away,
        parameters.edge_stay_near,
        parameters.corner_keep_away,
        parameters.corner_stay_near,
        facts.world_width,
        facts.world_height,
    ] {
        digest_i32(&mut hash, value);
    }
    digest_u64(&mut hash, facts.cells.len() as u64);
    for cell in &facts.cells {
        digest_i32(&mut hash, cell.x);
        digest_i32(&mut hash, cell.y);
        digest_bytes(&mut hash, &cell.primary_flags.to_le_bytes());
        digest_bytes(&mut hash, &cell.primary_land.to_le_bytes());
        digest_bytes(&mut hash, &cell.shadow_flags.to_le_bytes());
        digest_bytes(&mut hash, &[cell.shadow_occupant]);
        digest_bytes(&mut hash, &cell.down_chain.initial_index.to_le_bytes());
        digest_bytes(&mut hash, &cell.down_chain.initial_who.to_le_bytes());
        digest_u64(&mut hash, cell.down_chain.links.len() as u64);
        for link in &cell.down_chain.links {
            digest_bytes(&mut hash, &link.index.to_le_bytes());
            digest_bytes(&mut hash, &link.who.to_le_bytes());
            digest_bytes(&mut hash, &link.next_index.to_le_bytes());
            digest_bytes(&mut hash, &link.next_who.to_le_bytes());
        }
        digest_bytes(
            &mut hash,
            &[cell.has_mountain_tcoords as u8, cell.has_south_forest as u8],
        );
    }
    digest_u64(&mut hash, facts.starts.len() as u64);
    for start in &facts.starts {
        for value in [start.cell_x, start.cell_y, start.metric_x, start.metric_y] {
            digest_i32(&mut hash, value);
        }
    }
    digest_u64(&mut hash, facts.existing_resources.len() as u64);
    for resource in &facts.existing_resources {
        digest_bytes(&mut hash, &[resource.active as u8]);
        match resource.kind {
            ExistingResourceKind::Good { good_id } => {
                digest_bytes(&mut hash, &[0]);
                digest_i32(&mut hash, good_id);
            }
            ExistingResourceKind::Item => digest_bytes(&mut hash, &[1]),
        }
        digest_i32(&mut hash, resource.cell_x);
        digest_i32(&mut hash, resource.cell_y);
    }
    digest_u64(&mut hash, facts.regions.len() as u64);
    for region in &facts.regions {
        digest_i32(&mut hash, region.region_id);
        digest_u64(&mut hash, region.points.len() as u64);
        for (x, y) in &region.points {
            digest_i32(&mut hash, *x);
            digest_i32(&mut hash, *y);
        }
    }
    digest_u64(&mut hash, facts.terrain_rare.len() as u64);
    for terrain in &facts.terrain_rare {
        digest_bytes(&mut hash, &[terrain.terrain_class]);
        digest_u64(&mut hash, terrain.good_ids.len() as u64);
        for good in &terrain.good_ids {
            digest_i32(&mut hash, *good);
        }
    }
    digest_u64(&mut hash, facts.coord_metric_table.len() as u64);
    for value in &facts.coord_metric_table {
        digest_i32(&mut hash, *value);
    }
    digest_bytes(&mut hash, &[facts.good_uses_offset_0x120 as u8]);
    match &facts.placement_evidence {
        RegionBodyPlacementEvidence::RetailCapture {
            executable_sha256,
            capture_sha256,
        } => {
            digest_bytes(&mut hash, &[0]);
            digest_bytes(&mut hash, executable_sha256.as_bytes());
            digest_bytes(&mut hash, capture_sha256);
        }
        RegionBodyPlacementEvidence::ExactPort {
            implementation_sha256,
            proof_document,
        } => {
            digest_bytes(&mut hash, &[1]);
            digest_bytes(&mut hash, implementation_sha256);
            digest_bytes(&mut hash, proof_document.as_bytes());
        }
        RegionBodyPlacementEvidence::SyntheticFixture { fixture } => {
            digest_bytes(&mut hash, &[2]);
            digest_bytes(&mut hash, fixture.as_bytes());
        }
    }
    hash
}

fn cell_at(facts: &RegionWorldBodyFacts, x: i32, y: i32) -> &RegionWorldCellFact {
    &facts.cells[(y * facts.world_width + x) as usize]
}

fn region_set(facts: &RegionWorldBodyFacts, region_id: i32) -> &RegionCandidateSetFact {
    facts
        .regions
        .iter()
        .find(|region| region.region_id == region_id)
        .expect("region catalog validated before execution")
}

/// Integer approximation emitted repeatedly in the retail body.
pub fn retail_approx_distance(x0: i32, y0: i32, x1: i32, y1: i32) -> i32 {
    let dx = x0.wrapping_sub(x1).unsigned_abs();
    let dy = y0.wrapping_sub(y1).unsigned_abs();
    let (min, max) = if dx < dy { (dx, dy) } else { (dy, dx) };
    if max == 0 {
        return 0;
    }
    let value = if min < 60_000 {
        max.wrapping_add(min.wrapping_mul(min) / max.wrapping_mul(2))
    } else {
        min.wrapping_add(max.wrapping_mul(2)) >> 1
    };
    value as i32
}

fn observation(
    observations: &mut Vec<RegionFilterObservation>,
    stage_va: u32,
    kind: RegionFilterKind,
    subject_index: Option<usize>,
    observed: i32,
    threshold_or_mask: i32,
    passed: bool,
) {
    observations.push(RegionFilterObservation {
        stage_va,
        kind,
        subject_index,
        observed,
        threshold_or_mask,
        passed,
    });
}

fn terrain_class(cell: &RegionWorldCellFact) -> i32 {
    let flags = cell.primary_flags;
    if flags & 0x4 != 0 {
        3
    } else if flags & 0x20 != 0 {
        4
    } else if flags & 0x50 != 0 {
        5
    } else if flags & 0x8 != 0 {
        i32::from(((flags & 0x800) | 0x3000) >> 11)
    } else if flags & 0x100 == 0
        && (cell.primary_land == 1 || cell.primary_land == 2)
        && flags & 0x800 != 0
    {
        7
    } else {
        i32::from(cell.primary_land)
    }
}

fn next_point_index(mut current: i32, start: i32, point_count: i32) -> Option<i32> {
    loop {
        if current & 1 != 0 {
            current ^= 0x16800;
        }
        current >>= 1;
        if current <= point_count {
            break;
        }
        if current == start {
            return None;
        }
    }
    (current != start).then_some(current)
}

fn matching_existing_resource(resource: &ExistingResourceFact, water_good: bool) -> bool {
    if !resource.active {
        return false;
    }
    match resource.kind {
        ExistingResourceKind::Item => false,
        ExistingResourceKind::Good { good_id: 5 } => false,
        ExistingResourceKind::Good { good_id: 6 | 31 } => water_good,
        ExistingResourceKind::Good { .. } => !water_good,
    }
}

#[allow(clippy::too_many_arguments)]
fn evaluate_candidate(
    one_based_point_index: i32,
    x: i32,
    y: i32,
    attempt_in_region: i32,
    parameters: RegionWorldBodyParameters,
    water_good: bool,
    per_region_limit: i32,
    facts: &RegionWorldBodyFacts,
    placed_points: &[(i32, i32)],
    existing_resources: &[ExistingResourceFact],
    down_overlay: &BTreeMap<(i32, i32), (i16, i16)>,
    preferred_start_index: usize,
) -> RegionCandidateEvaluation {
    let cell = cell_at(facts, x, y);
    let mut observations = Vec::new();
    let mut deferred_rejection = None;

    let start_free = cell.primary_flags & 0x1000 == 0;
    observation(
        &mut observations,
        PRIMARY_START_OCCUPIED_FILTER_VA,
        RegionFilterKind::PrimaryStartOccupied,
        None,
        i32::from(cell.primary_flags),
        0x1000,
        start_free,
    );
    if !start_free {
        deferred_rejection = Some(RegionFilterKind::PrimaryStartOccupied);
    } else if parameters.player_keep_away > 0 {
        for (index, start) in facts.starts.iter().enumerate() {
            let distance = retail_approx_distance(x, y, start.cell_x, start.cell_y);
            let passed = distance >= parameters.player_keep_away;
            observation(
                &mut observations,
                PLAYER_KEEP_AWAY_FILTER_VA,
                RegionFilterKind::PlayerKeepAway,
                Some(index),
                distance,
                parameters.player_keep_away,
                passed,
            );
            if !passed {
                deferred_rejection = Some(RegionFilterKind::PlayerKeepAway);
                break;
            }
        }
    }

    for (kind, mask) in [
        (RegionFilterKind::ShadowFlags10, 0x10),
        (RegionFilterKind::ShadowFlags68, 0x68),
        (RegionFilterKind::ShadowFlags1800, 0x1800),
    ] {
        let passed = cell.shadow_flags & mask == 0;
        observation(
            &mut observations,
            SHADOW_CELL_FILTER_VA,
            kind,
            None,
            i32::from(cell.shadow_flags),
            i32::from(mask),
            passed,
        );
        if !passed {
            return RegionCandidateEvaluation {
                one_based_point_index,
                x,
                y,
                observations,
                rejection: Some(kind),
            };
        }
    }
    let shadow_empty = cell.shadow_occupant == 0;
    observation(
        &mut observations,
        SHADOW_CELL_FILTER_VA,
        RegionFilterKind::ShadowOccupant,
        None,
        i32::from(cell.shadow_occupant),
        0,
        shadow_empty,
    );
    if !shadow_empty {
        return RegionCandidateEvaluation {
            one_based_point_index,
            x,
            y,
            observations,
            rejection: Some(RegionFilterKind::ShadowOccupant),
        };
    }
    if deferred_rejection.is_some() {
        return RegionCandidateEvaluation {
            one_based_point_index,
            x,
            y,
            observations,
            rejection: deferred_rejection,
        };
    }

    if parameters.player_stay_near > 0 {
        for (index, start) in facts.starts.iter().enumerate() {
            let distance = retail_approx_distance(x, y, start.cell_x, start.cell_y);
            let passed = distance <= parameters.player_stay_near;
            observation(
                &mut observations,
                PLAYER_STAY_NEAR_FILTER_VA,
                RegionFilterKind::PlayerStayNear,
                Some(index),
                distance,
                parameters.player_stay_near,
                passed,
            );
            if !passed {
                return RegionCandidateEvaluation {
                    one_based_point_index,
                    x,
                    y,
                    observations,
                    rejection: Some(RegionFilterKind::PlayerStayNear),
                };
            }
        }
    }

    let center_x = facts.world_width / 2;
    let center_y = facts.world_height / 2;
    let center_distance = retail_approx_distance(x, y, center_x, center_y);
    for (kind, stage_va, threshold, passed) in [
        (
            RegionFilterKind::CenterKeepAway,
            CENTER_KEEP_AWAY_FILTER_VA,
            parameters.center_keep_away,
            parameters.center_keep_away <= 0 || center_distance >= parameters.center_keep_away,
        ),
        (
            RegionFilterKind::CenterStayNear,
            CENTER_STAY_NEAR_FILTER_VA,
            parameters.center_stay_near,
            parameters.center_stay_near <= 0 || center_distance <= parameters.center_stay_near,
        ),
    ] {
        if threshold > 0 {
            observation(
                &mut observations,
                stage_va,
                kind,
                None,
                center_distance,
                threshold,
                passed,
            );
            if !passed {
                return RegionCandidateEvaluation {
                    one_based_point_index,
                    x,
                    y,
                    observations,
                    rejection: Some(kind),
                };
            }
        }
    }

    let edge_distance = x
        .min(y)
        .min(facts.world_width - x)
        .min(facts.world_height - y);
    for (kind, stage_va, threshold, passed) in [
        (
            RegionFilterKind::EdgeKeepAway,
            EDGE_KEEP_AWAY_FILTER_VA,
            parameters.edge_keep_away,
            parameters.edge_keep_away <= 0 || edge_distance >= parameters.edge_keep_away,
        ),
        (
            RegionFilterKind::EdgeStayNear,
            EDGE_STAY_NEAR_FILTER_VA,
            parameters.edge_stay_near,
            parameters.edge_stay_near <= 0 || edge_distance <= parameters.edge_stay_near,
        ),
    ] {
        if threshold > 0 {
            observation(
                &mut observations,
                stage_va,
                kind,
                None,
                edge_distance,
                threshold,
                passed,
            );
            if !passed {
                return RegionCandidateEvaluation {
                    one_based_point_index,
                    x,
                    y,
                    observations,
                    rejection: Some(kind),
                };
            }
        }
    }

    if parameters.corner_keep_away > 0 {
        let mut nearest_corner = 0x270f;
        for (index, (mx, my)) in CORNER_MULTIPLIERS.iter().copied().enumerate() {
            let distance =
                retail_approx_distance(x, y, mx * facts.world_width, my * facts.world_height);
            nearest_corner = nearest_corner.min(distance);
            observation(
                &mut observations,
                CORNER_KEEP_AWAY_FILTER_VA,
                RegionFilterKind::CornerKeepAway,
                Some(index),
                distance,
                0x270f,
                true,
            );
        }
        let passed = nearest_corner >= parameters.corner_keep_away;
        observation(
            &mut observations,
            CORNER_KEEP_AWAY_FILTER_VA,
            RegionFilterKind::CornerKeepAway,
            None,
            nearest_corner,
            parameters.corner_keep_away,
            passed,
        );
        if !passed {
            return RegionCandidateEvaluation {
                one_based_point_index,
                x,
                y,
                observations,
                rejection: Some(RegionFilterKind::CornerKeepAway),
            };
        }
    }
    if parameters.corner_stay_near > 0 {
        let mut nearest_corner = 0x270f;
        for (index, (mx, my)) in CORNER_MULTIPLIERS.iter().copied().enumerate() {
            let distance =
                retail_approx_distance(x, y, mx * facts.world_width, my * facts.world_height);
            nearest_corner = nearest_corner.min(distance);
            observation(
                &mut observations,
                CORNER_STAY_NEAR_FILTER_VA,
                RegionFilterKind::CornerStayNear,
                Some(index),
                distance,
                0x270f,
                true,
            );
        }
        let passed = nearest_corner <= parameters.corner_stay_near;
        observation(
            &mut observations,
            CORNER_STAY_NEAR_FILTER_VA,
            RegionFilterKind::CornerStayNear,
            None,
            nearest_corner,
            parameters.corner_stay_near,
            passed,
        );
        if !passed {
            return RegionCandidateEvaluation {
                one_based_point_index,
                x,
                y,
                observations,
                rejection: Some(RegionFilterKind::CornerStayNear),
            };
        }
    }

    if parameters.group_spacing > 0 {
        for (index, &(other_x, other_y)) in placed_points.iter().enumerate() {
            let distance = retail_approx_distance(x, y, other_x, other_y);
            let passed = distance >= parameters.group_spacing;
            observation(
                &mut observations,
                GROUP_SPACING_FILTER_VA,
                RegionFilterKind::GroupSpacing,
                Some(index),
                distance,
                parameters.group_spacing,
                passed,
            );
            if !passed {
                return RegionCandidateEvaluation {
                    one_based_point_index,
                    x,
                    y,
                    observations,
                    rejection: Some(RegionFilterKind::GroupSpacing),
                };
            }
        }
    }

    if parameters.existing_resource_spacing > 0 {
        for (index, resource) in existing_resources.iter().enumerate() {
            if !matching_existing_resource(resource, water_good) {
                continue;
            }
            let distance = retail_approx_distance(x, y, resource.cell_x, resource.cell_y);
            let passed = distance >= parameters.existing_resource_spacing;
            observation(
                &mut observations,
                EXISTING_RESOURCE_SPACING_FILTER_VA,
                RegionFilterKind::ExistingResourceSpacing,
                Some(index),
                distance,
                parameters.existing_resource_spacing,
                passed,
            );
            if !passed {
                return RegionCandidateEvaluation {
                    one_based_point_index,
                    x,
                    y,
                    observations,
                    rejection: Some(RegionFilterKind::ExistingResourceSpacing),
                };
            }
        }
    }

    let neighbor_count = if water_good { 24 } else { 8 };
    for (index, (dx, dy)) in NEIGHBOR_OFFSETS
        .iter()
        .copied()
        .take(neighbor_count)
        .enumerate()
    {
        let nx = x + dx;
        let ny = y + dy;
        let in_bounds = nx >= 0 && ny >= 0 && nx < facts.world_width && ny < facts.world_height;
        let passed = if !in_bounds {
            true
        } else {
            let neighbor = cell_at(facts, nx, ny);
            if water_good {
                neighbor.primary_land != 0
            } else {
                neighbor.primary_flags & 0x100 != 0
                    || (neighbor.primary_land != 1 && neighbor.primary_land != 2)
            }
        };
        observation(
            &mut observations,
            if water_good {
                WATER_NEIGHBOR_FILTER_VA
            } else {
                LAND_NEIGHBOR_FILTER_VA
            },
            if water_good {
                RegionFilterKind::WaterNeighbor
            } else {
                RegionFilterKind::LandNeighbor
            },
            Some(index),
            if in_bounds { 1 } else { 0 },
            1,
            passed,
        );
        if !passed {
            return RegionCandidateEvaluation {
                one_based_point_index,
                x,
                y,
                observations,
                rejection: Some(if water_good {
                    RegionFilterKind::WaterNeighbor
                } else {
                    RegionFilterKind::LandNeighbor
                }),
            };
        }
    }

    observation(
        &mut observations,
        HAS_MOUNTAIN_CALL_VA,
        RegionFilterKind::HasMountainTcoords,
        None,
        cell.has_mountain_tcoords as i32,
        WORLD_DATA_HAS_MOUNTAIN_TCOORDS_VA as i32,
        !cell.has_mountain_tcoords,
    );
    if cell.has_mountain_tcoords {
        return RegionCandidateEvaluation {
            one_based_point_index,
            x,
            y,
            observations,
            rejection: Some(RegionFilterKind::HasMountainTcoords),
        };
    }

    // Retail compares y against width-1 at 0x006911f7, not height-1.
    let interior = x != 0 && y != 0 && x != facts.world_width - 1 && y != facts.world_width - 1;
    observation(
        &mut observations,
        BORDER_FILTER_VA,
        RegionFilterKind::Border,
        None,
        (x == 0 || y == 0 || x == facts.world_width - 1 || y == facts.world_width - 1) as i32,
        0,
        interior,
    );
    if !interior {
        return RegionCandidateEvaluation {
            one_based_point_index,
            x,
            y,
            observations,
            rejection: Some(RegionFilterKind::Border),
        };
    }

    observation(
        &mut observations,
        HAS_SOUTH_FOREST_CALL_VA,
        RegionFilterKind::HasSouthForest,
        None,
        cell.has_south_forest as i32,
        TERRAIN_HAS_SOUTH_FOREST_VA as i32,
        !cell.has_south_forest,
    );
    if cell.has_south_forest {
        return RegionCandidateEvaluation {
            one_based_point_index,
            x,
            y,
            observations,
            rejection: Some(RegionFilterKind::HasSouthForest),
        };
    }

    let class = terrain_class(cell);
    let terrain_allowed = facts.terrain_rare[class as usize]
        .good_ids
        .contains(&parameters.good_id)
        && (!water_good || cell.primary_flags & 0x4 == 0);
    observation(
        &mut observations,
        TERRAIN_RARE_FILTER_VA,
        RegionFilterKind::TerrainRare,
        Some(class as usize),
        parameters.good_id,
        class,
        terrain_allowed,
    );
    if !terrain_allowed {
        return RegionCandidateEvaluation {
            one_based_point_index,
            x,
            y,
            observations,
            rejection: Some(RegionFilterKind::TerrainRare),
        };
    }

    let terminal = down_overlay
        .get(&(x, y))
        .map(|(down, _)| *down)
        .unwrap_or_else(|| {
            cell.down_chain
                .terminal_index()
                .expect("down chains validated before execution")
        });
    let down_allowed = terminal >= -1;
    observation(
        &mut observations,
        DOWN_CHAIN_FILTER_VA,
        RegionFilterKind::DownChain,
        None,
        i32::from(terminal),
        -1,
        down_allowed,
    );
    if !down_allowed {
        return RegionCandidateEvaluation {
            one_based_point_index,
            x,
            y,
            observations,
            rejection: Some(RegionFilterKind::DownChain),
        };
    }

    if attempt_in_region >= per_region_limit {
        let offset = if facts.good_uses_offset_0x120 {
            0x120
        } else {
            0x180
        };
        let coord_x = x * 0x300 + offset;
        let coord_y = y * 0x300 + offset;
        let metric_x = facts.coord_metric_table[(coord_x >> 8) as usize];
        let metric_y = facts.coord_metric_table[(coord_y >> 8) as usize];
        let mut nearest_index = i32::MAX;
        let mut nearest_distance = i32::MAX;
        for (index, start) in facts.starts.iter().enumerate() {
            let distance =
                retail_approx_distance(metric_x, metric_y, start.metric_x, start.metric_y);
            if distance < nearest_distance {
                nearest_distance = distance;
                nearest_index = index as i32;
            }
        }
        let passed = nearest_index == preferred_start_index as i32;
        observation(
            &mut observations,
            SATURATION_OWNER_FILTER_VA,
            RegionFilterKind::SaturationOwner,
            (!facts.starts.is_empty()).then_some(nearest_index as usize),
            preferred_start_index as i32,
            per_region_limit,
            passed,
        );
        if !passed {
            return RegionCandidateEvaluation {
                one_based_point_index,
                x,
                y,
                observations,
                rejection: Some(RegionFilterKind::SaturationOwner),
            };
        }
    }

    RegionCandidateEvaluation {
        one_based_point_index,
        x,
        y,
        observations,
        rejection: None,
    }
}

fn validate_prefix_draw(random: &mut Random, draw: ExactRandomDraw, call_va: u32) -> bool {
    if draw.call_va != call_va
        || draw.random_get_va != RANDOM_GET_VA
        || draw.low != 0
        || draw.high != 0xffff
        || draw.modulus <= 0
        || draw.state_before != random.state()
    {
        return false;
    }
    let raw = random.get(0, 0xffff);
    draw.raw == raw && draw.remainder == raw % draw.modulus && draw.state_after == random.state()
}

fn draw_point(random: &mut Random, modulus: i32) -> RegionBodyRandomDraw {
    let state_before = random.state();
    let raw = random.get(0, 0xffff);
    RegionBodyRandomDraw {
        call_va: REGION_RANDOM_GET_CALL_VA,
        random_get_va: RANDOM_GET_VA,
        low: 0,
        high: 0xffff,
        state_before,
        raw,
        modulus,
        remainder: raw % modulus,
        state_after: random.state(),
    }
}

fn validate_facts(
    prefix: &RegionPlacementPrefixReceipt,
    facts: &RegionWorldBodyFacts,
) -> Result<(), RegionWorldBodyError> {
    if facts.world_width <= 1 || facts.world_height <= 1 {
        return Err(RegionWorldBodyError::InvalidWorldShape);
    }
    let Some(cell_count) = facts
        .world_width
        .checked_mul(facts.world_height)
        .and_then(|count| usize::try_from(count).ok())
    else {
        return Err(RegionWorldBodyError::InvalidWorldShape);
    };
    let maximum_coord = facts.world_width.max(facts.world_height) - 1;
    if maximum_coord
        .checked_mul(0x300)
        .and_then(|coord| coord.checked_add(0x180))
        .is_none()
    {
        return Err(RegionWorldBodyError::InvalidWorldShape);
    }
    if facts.cells.len() != cell_count
        || facts.cells.iter().enumerate().any(|(index, cell)| {
            cell.x != index as i32 % facts.world_width
                || cell.y != index as i32 / facts.world_width
                || !(0..=7).contains(&i32::from(cell.primary_land))
        })
    {
        return Err(RegionWorldBodyError::InvalidCellCatalog);
    }
    for cell in &facts.cells {
        if cell.down_chain.terminal_index().is_none() {
            return Err(RegionWorldBodyError::InvalidDownChain {
                x: cell.x,
                y: cell.y,
            });
        }
    }
    if facts.starts.len() > 9
        || facts.starts.iter().any(|start| {
            start.cell_x < 0
                || start.cell_y < 0
                || start.cell_x >= facts.world_width
                || start.cell_y >= facts.world_height
        })
    {
        return Err(RegionWorldBodyError::InvalidStartCatalog);
    }
    if facts.regions.len() != prefix.available_regions.len()
        || prefix.available_regions.iter().any(|region_id| {
            facts
                .regions
                .iter()
                .filter(|region| region.region_id == *region_id)
                .count()
                != 1
        })
        || facts.regions.iter().any(|region| {
            region.points.is_empty()
                || region.points.iter().any(|&(x, y)| {
                    x < 0 || y < 0 || x >= facts.world_width || y >= facts.world_height
                })
        })
    {
        return Err(RegionWorldBodyError::InvalidRegionCatalog);
    }
    if facts.terrain_rare.len() != 8
        || facts
            .terrain_rare
            .iter()
            .enumerate()
            .any(|(index, row)| row.terrain_class as usize != index)
    {
        return Err(RegionWorldBodyError::InvalidTerrainCatalog);
    }
    let Some(required_metric_index) = maximum_coord
        .checked_mul(3)
        .and_then(|index| index.checked_add(1))
    else {
        return Err(RegionWorldBodyError::InvalidMetricTable);
    };
    if facts.coord_metric_table.len() <= required_metric_index as usize {
        return Err(RegionWorldBodyError::InvalidMetricTable);
    }
    Ok(())
}

fn validate_allocation_receipt(
    index: usize,
    receipt: &RegionInitGoodReceipt,
) -> Result<(), RegionWorldBodyError> {
    let request = &receipt.request;
    let allocation = &receipt.allocation;
    if allocation.call_va != REGION_INIT_GOOD_CALL_VA
        || allocation.callee_va != OBJECTS_INIT_GOOD_VA
        || allocation.good_id != request.good_id
        || allocation.coord_x != request.coord_x
        || allocation.coord_y != request.coord_y
        || allocation.slot < 0
        || allocation.slot > i32::from(i16::MAX)
        || receipt.sourced_walked_bytes_after != request.sourced_walked_bytes
        || receipt.good_state_digest_after == request.good_state_digest_before
    {
        return Err(RegionWorldBodyError::InvalidAllocationReceipt { index });
    }

    if request.good_id == 5 {
        if allocation.occupancy_write.is_some()
            || receipt.world_checksum_after != request.world_checksum_before
        {
            return Err(RegionWorldBodyError::InvalidAllocationReceipt { index });
        }
        return Ok(());
    }
    let Some(write) = &allocation.occupancy_write else {
        return Err(RegionWorldBodyError::InvalidAllocationReceipt { index });
    };
    if write.world_x != request.world_x
        || write.world_y != request.world_y
        || write.down_write_va != GOOD_WDATA_DOWN_WRITE_VA
        || write.down_who_write_va != GOOD_WDATA_DOWN_WHO_WRITE_VA
        || write.old_down != request.old_down
        || write.old_down_who != request.old_down_who
        || write.new_down != GOOD_WDATA_DOWN_MARKER
        || write.new_down_who != allocation.slot as i16
        || request
            .world_checksum_before
            .differing_sections(&receipt.world_checksum_after)
            != [WorldSection::WData]
    {
        return Err(RegionWorldBodyError::InvalidAllocationReceipt { index });
    }
    Ok(())
}

/// Execute the complete concrete-good `World`-pattern body atomically.
pub fn execute_region_world_body<H: RegionInitGoodHost>(
    state: &mut RegionWorldBodyState,
    parameters: RegionWorldBodyParameters,
    prefix: &RegionPlacementPrefixReceipt,
    facts: &RegionWorldBodyFacts,
    host: &mut H,
) -> Result<RegionWorldBodyReceipt, RegionWorldBodyError> {
    if parameters.pattern != WORLD_PATTERN {
        return Err(RegionWorldBodyError::UnsupportedPattern);
    }
    if parameters.selector != CONCRETE_SELECTOR {
        return Err(RegionWorldBodyError::UnsupportedSelector);
    }
    if parameters.good_id < 0 || parameters.good_id == ITEM_GOOD_ID {
        return Err(RegionWorldBodyError::UnsupportedGood);
    }
    if !facts.placement_evidence.admissible() {
        return Err(RegionWorldBodyError::InvalidPlacementEvidence);
    }
    let digest = region_world_body_facts_digest(parameters, facts);
    let prefix_digest = region_placement_prefix_receipt_digest(prefix);
    if !facts.evidence.admissible(state, digest, prefix_digest) {
        return Err(RegionWorldBodyError::StaleEvidence);
    }
    validate_facts(prefix, facts)?;

    let RegionPrefixDisposition::CandidateScan {
        residual_va,
        selected_region,
        point_index,
    } = prefix.disposition
    else {
        return Err(RegionWorldBodyError::WrongPrefix);
    };
    let Some(first_region_index) = prefix.first_region_index else {
        return Err(RegionWorldBodyError::WrongPrefix);
    };
    if residual_va != REGION_CANDIDATE_SCAN_VA
        || prefix.entry_va != crate::place_resources_category_frontier::MAP_PLACE_REGION_RESOURCE_VA
        || prefix.find_avail_regions_call_va
            != crate::place_resources_category_frontier::MAP_FIND_AVAIL_REGIONS_CALL_VA
        || prefix.find_avail_regions_va
            != crate::place_resources_category_frontier::MAP_FIND_AVAIL_REGIONS_VA
        || prefix.random_state_before != state.deterministic.random_state
        || prefix.world_checksum != state.deterministic.world_checksum
        || prefix.region_pass_count != prefix.available_regions.len() as i32
        || prefix.available_regions.get(first_region_index).copied() != Some(selected_region)
        || parameters.num_rare <= 0
    {
        return Err(RegionWorldBodyError::WrongPrefix);
    }

    let first_points = &region_set(facts, selected_region).points;
    if point_index < 0 || point_index as usize >= first_points.len() {
        return Err(RegionWorldBodyError::WrongPrefix);
    }
    let expected_point_draw = first_points.len() > 1;
    let expected_first_region_index = prefix
        .find_avail_draw
        .map(|draw| draw.remainder as usize)
        .unwrap_or(0);
    let expected_point_index = prefix.point_draw.map(|draw| draw.remainder).unwrap_or(0);
    let expected_per_region_limit = if parameters.num_rare > 2
        && parameters.saturate == 0
        && prefix.available_regions.len() == 1
    {
        parameters.num_rare >> 1
    } else {
        i32::MAX
    };
    if prefix.point_draw.is_some() != expected_point_draw
        || first_region_index != expected_first_region_index
        || point_index != expected_point_index
        || prefix.per_region_limit != expected_per_region_limit
        || prefix
            .point_draw
            .is_some_and(|draw| draw.modulus != first_points.len() as i32)
        || prefix.find_avail_draw.is_some() != (prefix.available_regions.len() > 1)
        || prefix
            .find_avail_draw
            .is_some_and(|draw| draw.modulus != prefix.available_regions.len() as i32)
    {
        return Err(RegionWorldBodyError::WrongPrefix);
    }

    let mut random = Random::new(state.deterministic.random_state);
    let mut random_draws = Vec::new();
    if let Some(draw) = prefix.find_avail_draw {
        if !validate_prefix_draw(
            &mut random,
            draw,
            crate::place_resources_category_frontier::FIND_AVAIL_RANDOM_GET_CALL_VA,
        ) {
            return Err(RegionWorldBodyError::WrongPrefix);
        }
        random_draws.push(draw.into());
    }
    if let Some(draw) = prefix.point_draw {
        if !validate_prefix_draw(&mut random, draw, REGION_RANDOM_GET_CALL_VA) {
            return Err(RegionWorldBodyError::WrongPrefix);
        }
        random_draws.push(draw.into());
    }
    if random.state() != prefix.random_state_after {
        return Err(RegionWorldBodyError::WrongPrefix);
    }

    let deterministic_before = state.deterministic.clone();
    let good_state_digest_before = state.good_state_digest;
    let mut current_world_checksum = state.deterministic.world_checksum.clone();
    let mut current_good_digest = state.good_state_digest;
    let mut existing_resources = facts.existing_resources.clone();
    let mut down_overlay = BTreeMap::new();
    let mut placed_points = Vec::new();
    let mut allocations = Vec::new();
    let mut attempts = Vec::new();
    let mut start_distance_accumulator = vec![0_i32; facts.starts.len()];
    let mut preferred_start_index = 0_usize;
    let mut first_attempt = true;

    let circular_region_order: Vec<i32> = (0..prefix.available_regions.len())
        .map(|pass| {
            prefix.available_regions[(first_region_index + pass) % prefix.available_regions.len()]
        })
        .collect();

    for (region_pass, region_id) in circular_region_order.iter().copied().enumerate() {
        let points = &region_set(facts, region_id).points;
        for attempt_in_region in 0..parameters.num_rare {
            let point_draw = if first_attempt {
                prefix.point_draw.map(Into::into)
            } else if points.len() > 1 {
                let draw = draw_point(&mut random, points.len() as i32);
                random_draws.push(draw);
                Some(draw)
            } else {
                None
            };
            let starting_one_based_point_index = if first_attempt {
                point_index + 1
            } else {
                point_draw.map(|draw| draw.remainder + 1).unwrap_or(1)
            };
            first_attempt = false;

            let mut current_point = starting_one_based_point_index;
            let mut candidate_receipts = Vec::new();
            let mut allocation_index = None;
            loop {
                let (x, y) = points[(current_point - 1) as usize];
                let evaluation = evaluate_candidate(
                    current_point,
                    x,
                    y,
                    attempt_in_region,
                    parameters,
                    prefix.water_good,
                    prefix.per_region_limit,
                    facts,
                    &placed_points,
                    &existing_resources,
                    &down_overlay,
                    preferred_start_index,
                );
                let accepted = evaluation.rejection.is_none();
                candidate_receipts.push(evaluation);
                if accepted {
                    let offset = if facts.good_uses_offset_0x120 {
                        0x120
                    } else {
                        0x180
                    };
                    let (old_down, old_down_who) = down_overlay.get(&(x, y)).copied().unwrap_or((
                        cell_at(facts, x, y).down_chain.initial_index,
                        cell_at(facts, x, y).down_chain.initial_who,
                    ));
                    let request = RegionInitGoodRequest {
                        call_va: REGION_INIT_GOOD_CALL_VA,
                        callee_va: OBJECTS_INIT_GOOD_VA,
                        good_id: parameters.good_id,
                        world_x: x,
                        world_y: y,
                        coord_x: x * 0x300 + offset,
                        coord_y: y * 0x300 + offset,
                        old_down,
                        old_down_who,
                        world_checksum_before: current_world_checksum.clone(),
                        good_state_digest_before: current_good_digest,
                        sourced_walked_bytes: state.deterministic.sourced_walked_bytes,
                    };
                    let Some(proposed) = host.propose_init_good(&request) else {
                        return Err(RegionWorldBodyError::AllocationUnavailable {
                            region_id,
                            x,
                            y,
                        });
                    };
                    let index = allocations.len();
                    if proposed.request != request {
                        return Err(RegionWorldBodyError::InvalidAllocationReceipt { index });
                    }
                    validate_allocation_receipt(index, &proposed)?;
                    current_world_checksum = proposed.world_checksum_after.clone();
                    current_good_digest = proposed.good_state_digest_after;
                    if parameters.good_id != 5 {
                        down_overlay.insert(
                            (x, y),
                            (GOOD_WDATA_DOWN_MARKER, proposed.allocation.slot as i16),
                        );
                    }
                    existing_resources.push(ExistingResourceFact {
                        active: true,
                        kind: ExistingResourceKind::Good {
                            good_id: parameters.good_id,
                        },
                        cell_x: x,
                        cell_y: y,
                    });
                    placed_points.push((x, y));
                    let coord_x_index = (request.coord_x >> 8) as usize;
                    let coord_y_index = (request.coord_y >> 8) as usize;
                    let metric_x = facts.coord_metric_table[coord_x_index];
                    let metric_y = facts.coord_metric_table[coord_y_index];
                    for (start_index, start) in facts.starts.iter().enumerate() {
                        let distance = retail_approx_distance(
                            metric_x,
                            metric_y,
                            start.metric_x,
                            start.metric_y,
                        );
                        start_distance_accumulator[start_index] =
                            start_distance_accumulator[start_index].wrapping_add(distance);
                    }
                    let mut maximum = 0;
                    preferred_start_index = 0;
                    for (start_index, distance) in
                        start_distance_accumulator.iter().copied().enumerate()
                    {
                        if distance > maximum {
                            maximum = distance;
                            preferred_start_index = start_index;
                        }
                    }
                    allocations.push(proposed.allocation);
                    allocation_index = Some(index);
                    break;
                }

                let Some(next) = next_point_index(
                    current_point,
                    starting_one_based_point_index,
                    points.len() as i32,
                ) else {
                    break;
                };
                current_point = next;
            }
            attempts.push(RegionAttemptReceipt {
                region_pass,
                region_id,
                attempt_in_region,
                point_draw,
                starting_one_based_point_index,
                candidates: candidate_receipts,
                allocation_index,
                attempt_tail_va: ATTEMPT_TAIL_VA,
            });
        }
    }

    let return_count = allocations.len() as i32;
    let mut deterministic_after = state.deterministic.clone();
    deterministic_after.random_state = random.state();
    deterministic_after.world_checksum = current_world_checksum;
    deterministic_after.allocated_resources = deterministic_after
        .allocated_resources
        .wrapping_add(return_count);
    // Pool, sourced bytes, requested count, and the three chance locals carry unchanged.
    let receipt = RegionWorldBodyReceipt {
        entry_va: REGION_CANDIDATE_SCAN_VA,
        function_return_value_va: FUNCTION_RETURN_VALUE_VA,
        caller_return_accumulate_va: CALLER_RETURN_ACCUMULATE_VA,
        parameters,
        water_good: prefix.water_good,
        circular_region_order,
        random_draws,
        attempts,
        allocations,
        start_distance_accumulator,
        preferred_start_index,
        return_count,
        deterministic_before,
        deterministic_after: deterministic_after.clone(),
        good_state_digest_before,
        good_state_digest_after: current_good_digest,
        placement_evidence: facts.placement_evidence.clone(),
    };
    state.deterministic = deterministic_after;
    state.good_state_digest = current_good_digest;
    Ok(receipt)
}
