// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact body of `Map::place_player_resource` (`0x00691f70`).
//!
//! This continuation consumes the registered player prefix, composes the exact
//! `ResourceDivvyPool::get_early` / `get_late` owners for selector rows, walks the
//! complete native spiral for every player and request, applies every candidate
//! filter in executable order, and validates two-phase `Objects::init_good` /
//! `Objects::init_item` receipts.  RNG, pool, World, Good, and Item projections are
//! advanced only after the complete function transaction validates.

use std::collections::BTreeMap;

use don_sim::rng::Random;
use don_sim::systems::map_terrain::{WorldChecksum, WorldSection};

use crate::place_region_resource_world_body_frontier::{
    retail_approx_distance, ExistingResourceFact, ExistingResourceKind, RegionWorldCellFact,
    TerrainRareFact, CORNER_MULTIPLIERS, NEIGHBOR_OFFSETS,
};
use crate::place_resources_category_frontier::{
    execute_player_placement_prefix, DeterministicResourceState, ExactRandomDraw,
    PlacementPrefixError, PlayerPlacementPrefixFacts, PlayerPlacementPrefixReceipt,
    PlayerPlacementPrefixRequest, PlayerPrefixDisposition, MAP_PLACE_PLAYER_RESOURCE_VA,
    PLAYER_POOL_SELECTION_VA, PLAYER_RANDOM_GET_CALL_VA, RANDOM_GET_VA, SHIPPED_EXE_SHA256,
    SHIPPED_PDB_SHA256,
};
use crate::place_resources_pool_frontier::{resource_divvy_pool_digest, ResourceDivvyPoolState};
use crate::resource_divvy_pool_selection_frontier::{
    execute_resource_pool_selection, ResourcePoolLane, ResourcePoolSelectionError,
    ResourcePoolSelectionReceipt,
};

pub const ITEM_GOOD_ID: i32 = 0x21f;
pub const PLAYER_GET_EARLY_CALL_VA: u32 = 0x0069_20b5;
pub const PLAYER_GET_LATE_CALL_VA: u32 = 0x0069_20bc;
pub const RESOURCE_POOL_GET_EARLY_VA: u32 = 0x0068_a640;
pub const RESOURCE_POOL_GET_LATE_VA: u32 = 0x0068_a570;

pub const BOUNDS_FILTER_VA: u32 = 0x0069_2187;
pub const START_BIT_FILTER_VA: u32 = 0x0069_21e4;
pub const PRIMARY_START_OCCUPIED_FILTER_VA: u32 = 0x0069_2214;
pub const SHADOW_CELL_FILTER_VA: u32 = 0x0069_223e;
pub const PLAYER_KEEP_AWAY_FILTER_VA: u32 = 0x0069_22a0;
pub const PLAYER_STAY_NEAR_FILTER_VA: u32 = 0x0069_234f;
pub const NEAREST_PLAYER_FILTER_VA: u32 = 0x0069_2460;
pub const CENTER_KEEP_AWAY_FILTER_VA: u32 = 0x0069_2518;
pub const CENTER_STAY_NEAR_FILTER_VA: u32 = 0x0069_25b8;
pub const EDGE_KEEP_AWAY_FILTER_VA: u32 = 0x0069_2658;
pub const EDGE_STAY_NEAR_FILTER_VA: u32 = 0x0069_268d;
pub const CORNER_KEEP_AWAY_FILTER_VA: u32 = 0x0069_26bb;
pub const CORNER_STAY_NEAR_FILTER_VA: u32 = 0x0069_2781;
pub const HAS_SOUTH_FOREST_CALL_VA: u32 = 0x0069_285c;
pub const TERRAIN_HAS_SOUTH_FOREST_VA: u32 = 0x0085_0060;
pub const PLACED_RESOURCE_SPACING_FILTER_VA: u32 = 0x0069_2883;
pub const BORDER_FILTER_VA: u32 = 0x0069_2918;
pub const EXISTING_GOOD_SPACING_FILTER_VA: u32 = 0x0069_2970;
pub const EXISTING_ITEM_SPACING_FILTER_VA: u32 = 0x0069_2a90;
pub const CARDINAL_NEIGHBOR_FILTER_VA: u32 = 0x0069_2b96;
pub const HAS_MOUNTAIN_CALL_VA: u32 = 0x0069_2c06;
pub const WORLD_DATA_HAS_MOUNTAIN_TCOORDS_VA: u32 = 0x006b_3050;
pub const TERRAIN_RARE_FILTER_VA: u32 = 0x0069_2c20;
pub const ITEM_TERRAIN_FILTER_VA: u32 = 0x0069_2cc0;
pub const DOWN_CHAIN_FILTER_VA: u32 = 0x0069_2cf1;

pub const PLAYER_INIT_GOOD_CALL_VA: u32 = 0x0069_2d90;
pub const PLAYER_INIT_ITEM_CALL_VA: u32 = 0x0069_2db5;
pub const OBJECTS_INIT_GOOD_VA: u32 = 0x0065_3f30;
pub const OBJECTS_INIT_ITEM_VA: u32 = 0x0065_3e00;
pub const GOOD_WDATA_DOWN_WRITE_VA: u32 = 0x0065_403e;
pub const GOOD_WDATA_DOWN_WHO_WRITE_VA: u32 = 0x0065_4043;
pub const ITEM_WDATA_DOWN_WRITE_VA: u32 = 0x0065_3f05;
pub const ITEM_WDATA_DOWN_WHO_WRITE_VA: u32 = 0x0065_3f0c;
pub const GOOD_WDATA_DOWN_MARKER: i16 = -2;
pub const ITEM_WDATA_DOWN_MARKER: i16 = -3;
pub const ATTEMPT_TAIL_VA: u32 = 0x0069_2df1;
pub const OUTER_PLAYER_TAIL_VA: u32 = 0x0069_2e0d;
pub const FUNCTION_RETURN_VALUE_VA: u32 = 0x0069_2e40;
pub const CALLER_RETURN_ACCUMULATE_VA: u32 = 0x0069_00e1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerResourceBodyState {
    pub deterministic: DeterministicResourceState,
    pub resource_pool: ResourceDivvyPoolState,
    pub good_state_digest: u64,
    pub item_state_digest: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerResourceBodyParameters {
    pub player_count: i32,
    /// Entry-time `param_2`. Selector rows replace it per attempt, but the shipped
    /// water/land class boolean is deliberately computed once from this value.
    pub good_id: i32,
    pub num_rare: i32,
    /// Native `param_4`: spiral inner radius and distance from every player start.
    pub player_keep_away: i32,
    /// Native `param_5`: spiral outer radius and maximum distance from this start.
    pub player_stay_near: i32,
    /// XML `spacing`, checked against already-active Goods or Items.
    pub existing_resource_spacing: i32,
    /// XML `group_spacing`, checked against placements made by this invocation.
    pub placed_resource_spacing: i32,
    pub center_keep_away: i32,
    pub center_stay_near: i32,
    pub edge_keep_away: i32,
    pub edge_stay_near: i32,
    pub corner_keep_away: i32,
    pub corner_stay_near: i32,
    /// Zero is concrete, two selects Early, every other nonzero selector selects Late.
    pub selector: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoodPlacementFact {
    pub good_id: i32,
    /// Good type `+0x234 & 1`: true uses cell offset `0x120`, false uses `0x180`.
    pub uses_offset_0x120: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerResourceBodyFacts {
    /// Native signed-byte tables at `0x00cb7e90` / `0x00cbb0e0`.
    pub spiral_x_offsets: Vec<i8>,
    pub spiral_y_offsets: Vec<i8>,
    /// Exactly `world_width * world_height`, row-major with x changing fastest.
    pub cells: Vec<RegionWorldCellFact>,
    /// Native start-occupancy bitset projected as one boolean per World cell.
    pub start_occupied: Vec<bool>,
    pub existing_resources: Vec<ExistingResourceFact>,
    /// Exactly terrain classes 0..=7.
    pub terrain_rare: Vec<TerrainRareFact>,
    pub good_catalog: Vec<GoodPlacementFact>,
    pub placement_evidence: PlayerBodyPlacementEvidence,
    pub evidence: PlayerResourceBodyEvidence,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlayerBodyPlacementEvidence {
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

impl PlayerBodyPlacementEvidence {
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
pub enum PlayerResourceBodyEvidence {
    RetailCapture {
        executable_sha256: String,
        pdb_sha256: String,
        capture_sha256: [u8; 32],
        entry_va: u32,
        random_state: i32,
        world_checksum: WorldChecksum,
        sourced_walked_bytes: u64,
        resource_pool_digest: u64,
        good_state_digest: u64,
        item_state_digest: u64,
        facts_digest: u64,
        prefix_digest: u64,
    },
    ExactPort {
        implementation_sha256: [u8; 32],
        proof_document: String,
        entry_va: u32,
        random_state: i32,
        world_checksum: WorldChecksum,
        sourced_walked_bytes: u64,
        resource_pool_digest: u64,
        good_state_digest: u64,
        item_state_digest: u64,
        facts_digest: u64,
        prefix_digest: u64,
    },
    SyntheticFixture {
        fixture: String,
        entry_va: u32,
        random_state: i32,
        world_checksum: WorldChecksum,
        sourced_walked_bytes: u64,
        resource_pool_digest: u64,
        good_state_digest: u64,
        item_state_digest: u64,
        facts_digest: u64,
        prefix_digest: u64,
    },
}

impl PlayerResourceBodyEvidence {
    fn admissible(
        &self,
        state: &PlayerResourceBodyState,
        facts_digest: u64,
        prefix_digest: u64,
    ) -> bool {
        let fields = |entry_va: u32,
                      random_state: i32,
                      world_checksum: &WorldChecksum,
                      sourced_walked_bytes: u64,
                      resource_pool_digest: u64,
                      good_state_digest: u64,
                      item_state_digest: u64,
                      actual_facts_digest: u64,
                      actual_prefix_digest: u64| {
            entry_va == MAP_PLACE_PLAYER_RESOURCE_VA
                && random_state == state.deterministic.random_state
                && world_checksum == &state.deterministic.world_checksum
                && sourced_walked_bytes == state.deterministic.sourced_walked_bytes
                && resource_pool_digest == state.deterministic.resource_pool_digest
                && good_state_digest == state.good_state_digest
                && item_state_digest == state.item_state_digest
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
                sourced_walked_bytes,
                resource_pool_digest,
                good_state_digest,
                item_state_digest,
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
                        *sourced_walked_bytes,
                        *resource_pool_digest,
                        *good_state_digest,
                        *item_state_digest,
                        *facts_digest,
                        *prefix_digest,
                    )
            }
            Self::ExactPort {
                implementation_sha256,
                proof_document,
                entry_va,
                random_state,
                world_checksum,
                sourced_walked_bytes,
                resource_pool_digest,
                good_state_digest,
                item_state_digest,
                facts_digest,
                prefix_digest,
            } => {
                implementation_sha256.iter().any(|byte| *byte != 0)
                    && !proof_document.is_empty()
                    && fields(
                        *entry_va,
                        *random_state,
                        world_checksum,
                        *sourced_walked_bytes,
                        *resource_pool_digest,
                        *good_state_digest,
                        *item_state_digest,
                        *facts_digest,
                        *prefix_digest,
                    )
            }
            Self::SyntheticFixture {
                fixture,
                entry_va,
                random_state,
                world_checksum,
                sourced_walked_bytes,
                resource_pool_digest,
                good_state_digest,
                item_state_digest,
                facts_digest,
                prefix_digest,
            } => {
                cfg!(test)
                    && !fixture.is_empty()
                    && fields(
                        *entry_va,
                        *random_state,
                        world_checksum,
                        *sourced_walked_bytes,
                        *resource_pool_digest,
                        *good_state_digest,
                        *item_state_digest,
                        *facts_digest,
                        *prefix_digest,
                    )
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerFilterKind {
    Bounds,
    StartBit,
    PrimaryStartOccupied,
    ShadowFlags10,
    ShadowFlags20,
    ShadowFlags08,
    ShadowFlags800,
    ShadowFlags1000,
    ShadowFlags40,
    ShadowOccupant,
    PlayerKeepAway,
    PlayerStayNear,
    NearestPlayer,
    CenterKeepAway,
    CenterStayNear,
    EdgeKeepAway,
    EdgeStayNear,
    CornerKeepAway,
    CornerStayNear,
    HasSouthForest,
    PlacedResourceSpacing,
    Border,
    ExistingResourceSpacing,
    CardinalNeighbor,
    HasMountainTcoords,
    TerrainRare,
    ItemTerrain,
    DownChain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerFilterObservation {
    pub stage_va: u32,
    pub kind: PlayerFilterKind,
    pub subject_index: Option<usize>,
    pub observed: i32,
    pub threshold_or_mask: i32,
    pub passed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerCandidateEvaluation {
    pub spiral_table_index: i32,
    pub x: i32,
    pub y: i32,
    pub observations: Vec<PlayerFilterObservation>,
    pub rejection: Option<PlayerFilterKind>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerBodyRandomDraw {
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

impl From<ExactRandomDraw> for PlayerBodyRandomDraw {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerAllocationKind {
    Good { good_id: i32 },
    Item,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerWorldOccupancyWrite {
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
pub struct PlayerResourceAllocation {
    pub call_va: u32,
    pub callee_va: u32,
    pub kind: PlayerAllocationKind,
    pub coord_x: i32,
    pub coord_y: i32,
    pub slot: i32,
    pub occupancy_write: Option<PlayerWorldOccupancyWrite>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerAllocationRequest {
    pub call_va: u32,
    pub callee_va: u32,
    pub kind: PlayerAllocationKind,
    pub world_x: i32,
    pub world_y: i32,
    pub coord_x: i32,
    pub coord_y: i32,
    pub old_down: i16,
    pub old_down_who: i16,
    pub world_checksum_before: WorldChecksum,
    pub good_state_digest_before: u64,
    pub item_state_digest_before: u64,
    pub sourced_walked_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerAllocationReceipt {
    pub request: PlayerAllocationRequest,
    pub allocation: PlayerResourceAllocation,
    pub world_checksum_after: WorldChecksum,
    pub good_state_digest_after: u64,
    pub item_state_digest_after: u64,
    pub sourced_walked_bytes_after: u64,
}

/// Observational/two-phase host. Implementations must not commit external state
/// before the complete owner receipt has been accepted.
pub trait PlayerAllocationHost {
    fn propose_allocation(
        &mut self,
        request: &PlayerAllocationRequest,
    ) -> Option<PlayerAllocationReceipt>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerAttemptReceipt {
    pub player_index: usize,
    pub attempt_index: i32,
    pub resolved_good_id: i32,
    /// Outer call in `Map::place_player_resource`; the nested selection receipt owns
    /// the corresponding `ResourceDivvyPool` entry and its transitive RNG calls.
    pub selector_call_va: Option<u32>,
    pub pool_selection: Option<ResourcePoolSelectionReceipt>,
    pub point_draw: Option<PlayerBodyRandomDraw>,
    pub candidate_span: i32,
    pub scan_start_remainder: i32,
    pub candidates: Vec<PlayerCandidateEvaluation>,
    pub allocation_index: Option<usize>,
    pub attempt_tail_va: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerResourceBodyReceipt {
    pub entry_va: u32,
    pub outer_player_tail_va: u32,
    pub function_return_value_va: u32,
    pub caller_return_accumulate_va: u32,
    pub parameters: PlayerResourceBodyParameters,
    pub water_good: bool,
    pub spacing_after_clamp: i32,
    pub player_stay_near_after_clamp: i32,
    pub attempts: Vec<PlayerAttemptReceipt>,
    pub pool_selections: Vec<ResourcePoolSelectionReceipt>,
    pub random_draws: Vec<PlayerBodyRandomDraw>,
    pub allocations: Vec<PlayerResourceAllocation>,
    pub return_count: i32,
    pub deterministic_before: DeterministicResourceState,
    pub deterministic_after: DeterministicResourceState,
    pub resource_pool_before: ResourceDivvyPoolState,
    pub resource_pool_after: ResourceDivvyPoolState,
    pub good_state_digest_before: u64,
    pub good_state_digest_after: u64,
    pub item_state_digest_before: u64,
    pub item_state_digest_after: u64,
    pub placement_evidence: PlayerBodyPlacementEvidence,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlayerResourceBodyError {
    WrongPrefix,
    Prefix(PlacementPrefixError),
    StaleEvidence,
    InvalidPlacementEvidence,
    InvalidWorldShape,
    InvalidCellCatalog,
    InvalidStartCatalog,
    InvalidSpiralCatalog,
    InvalidTerrainCatalog,
    InvalidGoodCatalog,
    InvalidDownChain { x: i32, y: i32 },
    InvalidPoolDigest,
    PoolSelection(ResourcePoolSelectionError),
    AllocationUnavailable { player_index: usize, x: i32, y: i32 },
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

fn digest_world(hash: &mut u64, world: &WorldChecksum) {
    digest_i32(hash, world.full as i32);
    digest_u64(hash, world.bytes);
    for section in &world.per_section {
        digest_i32(hash, section.adler as i32);
        digest_u64(hash, section.bytes);
    }
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

/// Stable binding for the prefix transaction consumed by this body.
pub fn player_placement_prefix_receipt_digest(prefix: &PlayerPlacementPrefixReceipt) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for value in [
        prefix.entry_va as i32,
        prefix.water_good as i32,
        prefix.spacing_after_clamp,
        prefix.group_spacing_after_clamp,
        prefix.candidate_span,
        prefix.random_state_before,
        prefix.random_state_after,
    ] {
        digest_i32(&mut hash, value);
    }
    digest_bytes(&mut hash, &[prefix.point_draw.is_some() as u8]);
    if let Some(draw) = &prefix.point_draw {
        digest_random_draw(&mut hash, draw);
    }
    match prefix.disposition {
        PlayerPrefixDisposition::InvalidFirstStart { path_va } => {
            digest_bytes(&mut hash, &[0]);
            digest_i32(&mut hash, path_va as i32);
        }
        PlayerPrefixDisposition::NoRequestedResources { path_va } => {
            digest_bytes(&mut hash, &[1]);
            digest_i32(&mut hash, path_va as i32);
        }
        PlayerPrefixDisposition::PoolSelectionBoundary { call_va } => {
            digest_bytes(&mut hash, &[2]);
            digest_i32(&mut hash, call_va as i32);
        }
        PlayerPrefixDisposition::EmptyCandidateSpan { residual_va } => {
            digest_bytes(&mut hash, &[3]);
            digest_i32(&mut hash, residual_va as i32);
        }
        PlayerPrefixDisposition::CandidateScan {
            residual_va,
            first_candidate_table_index,
        } => {
            digest_bytes(&mut hash, &[4]);
            digest_i32(&mut hash, residual_va as i32);
            digest_i32(&mut hash, first_candidate_table_index);
        }
    }
    digest_world(&mut hash, &prefix.world_checksum);
    hash
}

/// Stable digest of every behavior-driving body fact, excluding the capture binding.
pub fn player_resource_body_facts_digest(
    parameters: PlayerResourceBodyParameters,
    prefix_facts: &PlayerPlacementPrefixFacts,
    facts: &PlayerResourceBodyFacts,
) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for value in [
        parameters.player_count,
        parameters.good_id,
        parameters.num_rare,
        parameters.player_keep_away,
        parameters.player_stay_near,
        parameters.existing_resource_spacing,
        parameters.placed_resource_spacing,
        parameters.center_keep_away,
        parameters.center_stay_near,
        parameters.edge_keep_away,
        parameters.edge_stay_near,
        parameters.corner_keep_away,
        parameters.corner_stay_near,
        parameters.selector,
        prefix_facts.world_width,
        prefix_facts.world_height,
    ] {
        digest_i32(&mut hash, value);
    }
    digest_u64(&mut hash, prefix_facts.starts.len() as u64);
    for start in &prefix_facts.starts {
        digest_i32(&mut hash, start.x);
        digest_i32(&mut hash, start.y);
    }
    digest_u64(&mut hash, prefix_facts.spiral_ring_ends.len() as u64);
    for value in &prefix_facts.spiral_ring_ends {
        digest_i32(&mut hash, *value);
    }
    digest_u64(&mut hash, facts.spiral_x_offsets.len() as u64);
    for value in &facts.spiral_x_offsets {
        digest_bytes(&mut hash, &value.to_le_bytes());
    }
    digest_u64(&mut hash, facts.spiral_y_offsets.len() as u64);
    for value in &facts.spiral_y_offsets {
        digest_bytes(&mut hash, &value.to_le_bytes());
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
    digest_u64(&mut hash, facts.start_occupied.len() as u64);
    for value in &facts.start_occupied {
        digest_bytes(&mut hash, &[*value as u8]);
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
    digest_u64(&mut hash, facts.terrain_rare.len() as u64);
    for terrain in &facts.terrain_rare {
        digest_bytes(&mut hash, &[terrain.terrain_class]);
        digest_u64(&mut hash, terrain.good_ids.len() as u64);
        for good_id in &terrain.good_ids {
            digest_i32(&mut hash, *good_id);
        }
    }
    digest_u64(&mut hash, facts.good_catalog.len() as u64);
    for good in &facts.good_catalog {
        digest_i32(&mut hash, good.good_id);
        digest_bytes(&mut hash, &[good.uses_offset_0x120 as u8]);
    }
    match &facts.placement_evidence {
        PlayerBodyPlacementEvidence::RetailCapture {
            executable_sha256,
            capture_sha256,
        } => {
            digest_bytes(&mut hash, &[0]);
            digest_bytes(&mut hash, executable_sha256.as_bytes());
            digest_bytes(&mut hash, capture_sha256);
        }
        PlayerBodyPlacementEvidence::ExactPort {
            implementation_sha256,
            proof_document,
        } => {
            digest_bytes(&mut hash, &[1]);
            digest_bytes(&mut hash, implementation_sha256);
            digest_bytes(&mut hash, proof_document.as_bytes());
        }
        PlayerBodyPlacementEvidence::SyntheticFixture { fixture } => {
            digest_bytes(&mut hash, &[2]);
            digest_bytes(&mut hash, fixture.as_bytes());
        }
    }
    hash
}

fn cell_at<'a>(
    prefix_facts: &PlayerPlacementPrefixFacts,
    facts: &'a PlayerResourceBodyFacts,
    x: i32,
    y: i32,
) -> &'a RegionWorldCellFact {
    &facts.cells[(y * prefix_facts.world_width + x) as usize]
}

fn down_terminal(cell: &RegionWorldCellFact) -> Option<i16> {
    let mut index = cell.down_chain.initial_index;
    let mut who = cell.down_chain.initial_who;
    let mut cursor = 0;
    while index >= 0 {
        let link = cell.down_chain.links.get(cursor)?;
        if link.index != index || link.who != who {
            return None;
        }
        index = link.next_index;
        who = link.next_who;
        cursor += 1;
    }
    (cursor == cell.down_chain.links.len()).then_some(index)
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

fn observe(
    observations: &mut Vec<PlayerFilterObservation>,
    stage_va: u32,
    kind: PlayerFilterKind,
    subject_index: Option<usize>,
    observed: i32,
    threshold_or_mask: i32,
    passed: bool,
) {
    observations.push(PlayerFilterObservation {
        stage_va,
        kind,
        subject_index,
        observed,
        threshold_or_mask,
        passed,
    });
}

fn rejected(
    spiral_table_index: i32,
    x: i32,
    y: i32,
    observations: Vec<PlayerFilterObservation>,
    rejection: PlayerFilterKind,
) -> PlayerCandidateEvaluation {
    PlayerCandidateEvaluation {
        spiral_table_index,
        x,
        y,
        observations,
        rejection: Some(rejection),
    }
}

fn matching_existing_resource(
    resource: &ExistingResourceFact,
    item: bool,
    water_good: bool,
) -> bool {
    if !resource.active {
        return false;
    }
    match (item, resource.kind) {
        (true, ExistingResourceKind::Item) => true,
        (true, ExistingResourceKind::Good { .. }) => false,
        (false, ExistingResourceKind::Item) => false,
        (false, ExistingResourceKind::Good { good_id: 5 }) => false,
        (false, ExistingResourceKind::Good { good_id: 6 | 31 }) => water_good,
        (false, ExistingResourceKind::Good { .. }) => !water_good,
    }
}

#[allow(clippy::too_many_arguments)]
fn evaluate_candidate(
    spiral_table_index: i32,
    x: i32,
    y: i32,
    player_index: usize,
    resolved_good_id: i32,
    water_good: bool,
    spacing: i32,
    stay_near: i32,
    parameters: PlayerResourceBodyParameters,
    prefix_facts: &PlayerPlacementPrefixFacts,
    facts: &PlayerResourceBodyFacts,
    placed_points: &[(i32, i32)],
    existing_resources: &[ExistingResourceFact],
    down_overlay: &BTreeMap<(i32, i32), (i16, i16)>,
) -> PlayerCandidateEvaluation {
    let mut observations = Vec::new();
    let in_bounds =
        x >= 0 && y >= 0 && x < prefix_facts.world_width && y < prefix_facts.world_height;
    observe(
        &mut observations,
        BOUNDS_FILTER_VA,
        PlayerFilterKind::Bounds,
        None,
        (x >= 0 && y >= 0) as i32,
        (x < prefix_facts.world_width && y < prefix_facts.world_height) as i32,
        in_bounds,
    );
    if !in_bounds {
        return rejected(
            spiral_table_index,
            x,
            y,
            observations,
            PlayerFilterKind::Bounds,
        );
    }
    let cell_index = (y * prefix_facts.world_width + x) as usize;
    let cell = &facts.cells[cell_index];
    let start_free = !facts.start_occupied[cell_index];
    observe(
        &mut observations,
        START_BIT_FILTER_VA,
        PlayerFilterKind::StartBit,
        None,
        facts.start_occupied[cell_index] as i32,
        0,
        start_free,
    );
    if !start_free {
        return rejected(
            spiral_table_index,
            x,
            y,
            observations,
            PlayerFilterKind::StartBit,
        );
    }
    let primary_free = cell.primary_flags & 0x1000 == 0;
    observe(
        &mut observations,
        PRIMARY_START_OCCUPIED_FILTER_VA,
        PlayerFilterKind::PrimaryStartOccupied,
        None,
        i32::from(cell.primary_flags),
        0x1000,
        primary_free,
    );
    if !primary_free {
        return rejected(
            spiral_table_index,
            x,
            y,
            observations,
            PlayerFilterKind::PrimaryStartOccupied,
        );
    }
    let mut shadow_rejection = None;
    for (kind, mask) in [
        (PlayerFilterKind::ShadowFlags10, 0x10),
        (PlayerFilterKind::ShadowFlags20, 0x20),
        (PlayerFilterKind::ShadowFlags08, 0x08),
        (PlayerFilterKind::ShadowFlags800, 0x800),
        (PlayerFilterKind::ShadowFlags1000, 0x1000),
        (PlayerFilterKind::ShadowFlags40, 0x40),
    ] {
        let passed = cell.shadow_flags & mask == 0;
        observe(
            &mut observations,
            SHADOW_CELL_FILTER_VA,
            kind,
            None,
            i32::from(cell.shadow_flags),
            i32::from(mask),
            passed,
        );
        if !passed {
            shadow_rejection = Some(kind);
            break;
        }
    }
    if shadow_rejection.is_none() {
        let shadow_empty = cell.shadow_occupant == 0;
        observe(
            &mut observations,
            SHADOW_CELL_FILTER_VA,
            PlayerFilterKind::ShadowOccupant,
            None,
            i32::from(cell.shadow_occupant),
            0,
            shadow_empty,
        );
        if !shadow_empty {
            shadow_rejection = Some(PlayerFilterKind::ShadowOccupant);
        }
    }

    if spacing > 0 {
        for (index, start) in prefix_facts
            .starts
            .iter()
            .take(parameters.player_count as usize)
            .enumerate()
        {
            let distance = retail_approx_distance(x, y, start.x, start.y);
            let passed = distance >= spacing;
            observe(
                &mut observations,
                PLAYER_KEEP_AWAY_FILTER_VA,
                PlayerFilterKind::PlayerKeepAway,
                Some(index),
                distance,
                spacing,
                passed,
            );
            if !passed {
                return rejected(
                    spiral_table_index,
                    x,
                    y,
                    observations,
                    PlayerFilterKind::PlayerKeepAway,
                );
            }
        }
    }
    if let Some(kind) = shadow_rejection {
        return rejected(spiral_table_index, x, y, observations, kind);
    }

    let own = prefix_facts.starts[player_index];
    let own_distance = retail_approx_distance(x, y, own.x, own.y);
    if stay_near > 0 {
        let passed = own_distance <= stay_near;
        observe(
            &mut observations,
            PLAYER_STAY_NEAR_FILTER_VA,
            PlayerFilterKind::PlayerStayNear,
            Some(player_index),
            own_distance,
            stay_near,
            passed,
        );
        if !passed {
            return rejected(
                spiral_table_index,
                x,
                y,
                observations,
                PlayerFilterKind::PlayerStayNear,
            );
        }
    }
    for (index, start) in prefix_facts
        .starts
        .iter()
        .take(parameters.player_count as usize)
        .enumerate()
    {
        if index == player_index {
            continue;
        }
        let other_distance = retail_approx_distance(x, y, start.x, start.y);
        let passed = other_distance > own_distance;
        observe(
            &mut observations,
            NEAREST_PLAYER_FILTER_VA,
            PlayerFilterKind::NearestPlayer,
            Some(index),
            other_distance,
            own_distance,
            passed,
        );
        if !passed {
            return rejected(
                spiral_table_index,
                x,
                y,
                observations,
                PlayerFilterKind::NearestPlayer,
            );
        }
    }

    let center_distance = retail_approx_distance(
        x,
        y,
        prefix_facts.world_width / 2,
        prefix_facts.world_height / 2,
    );
    if parameters.center_keep_away != 0 {
        let passed = center_distance >= parameters.center_keep_away;
        observe(
            &mut observations,
            CENTER_KEEP_AWAY_FILTER_VA,
            PlayerFilterKind::CenterKeepAway,
            None,
            center_distance,
            parameters.center_keep_away,
            passed,
        );
        if !passed {
            return rejected(
                spiral_table_index,
                x,
                y,
                observations,
                PlayerFilterKind::CenterKeepAway,
            );
        }
    }
    if parameters.center_stay_near > 0 {
        let passed = center_distance <= parameters.center_stay_near;
        observe(
            &mut observations,
            CENTER_STAY_NEAR_FILTER_VA,
            PlayerFilterKind::CenterStayNear,
            None,
            center_distance,
            parameters.center_stay_near,
            passed,
        );
        if !passed {
            return rejected(
                spiral_table_index,
                x,
                y,
                observations,
                PlayerFilterKind::CenterStayNear,
            );
        }
    }

    // Retail uses width-x / height-y, not the more usual width-1-x / height-1-y.
    let edge_distance = x
        .min(y)
        .min(prefix_facts.world_width - x)
        .min(prefix_facts.world_height - y);
    if parameters.edge_keep_away > 0 {
        let passed = edge_distance >= parameters.edge_keep_away;
        observe(
            &mut observations,
            EDGE_KEEP_AWAY_FILTER_VA,
            PlayerFilterKind::EdgeKeepAway,
            None,
            edge_distance,
            parameters.edge_keep_away,
            passed,
        );
        if !passed {
            return rejected(
                spiral_table_index,
                x,
                y,
                observations,
                PlayerFilterKind::EdgeKeepAway,
            );
        }
    }
    if parameters.edge_stay_near > 0 {
        let passed = edge_distance <= parameters.edge_stay_near;
        observe(
            &mut observations,
            EDGE_STAY_NEAR_FILTER_VA,
            PlayerFilterKind::EdgeStayNear,
            None,
            edge_distance,
            parameters.edge_stay_near,
            passed,
        );
        if !passed {
            return rejected(
                spiral_table_index,
                x,
                y,
                observations,
                PlayerFilterKind::EdgeStayNear,
            );
        }
    }

    let corner_distance = CORNER_MULTIPLIERS
        .iter()
        .map(|&(mx, my)| {
            retail_approx_distance(
                x,
                y,
                mx * prefix_facts.world_width,
                my * prefix_facts.world_height,
            )
        })
        .min()
        .unwrap_or(9999);
    if parameters.corner_keep_away > 0 {
        let passed = corner_distance >= parameters.corner_keep_away;
        observe(
            &mut observations,
            CORNER_KEEP_AWAY_FILTER_VA,
            PlayerFilterKind::CornerKeepAway,
            None,
            corner_distance,
            parameters.corner_keep_away,
            passed,
        );
        if !passed {
            return rejected(
                spiral_table_index,
                x,
                y,
                observations,
                PlayerFilterKind::CornerKeepAway,
            );
        }
    }
    if parameters.corner_stay_near > 0 {
        let passed = corner_distance <= parameters.corner_stay_near;
        observe(
            &mut observations,
            CORNER_STAY_NEAR_FILTER_VA,
            PlayerFilterKind::CornerStayNear,
            None,
            corner_distance,
            parameters.corner_stay_near,
            passed,
        );
        if !passed {
            return rejected(
                spiral_table_index,
                x,
                y,
                observations,
                PlayerFilterKind::CornerStayNear,
            );
        }
    }

    observe(
        &mut observations,
        HAS_SOUTH_FOREST_CALL_VA,
        PlayerFilterKind::HasSouthForest,
        None,
        cell.has_south_forest as i32,
        TERRAIN_HAS_SOUTH_FOREST_VA as i32,
        !cell.has_south_forest,
    );
    if cell.has_south_forest {
        return rejected(
            spiral_table_index,
            x,
            y,
            observations,
            PlayerFilterKind::HasSouthForest,
        );
    }
    if parameters.placed_resource_spacing > 0 {
        for (index, &(placed_x, placed_y)) in placed_points.iter().enumerate() {
            let distance = retail_approx_distance(x, y, placed_x, placed_y);
            let passed = distance >= parameters.placed_resource_spacing;
            observe(
                &mut observations,
                PLACED_RESOURCE_SPACING_FILTER_VA,
                PlayerFilterKind::PlacedResourceSpacing,
                Some(index),
                distance,
                parameters.placed_resource_spacing,
                passed,
            );
            if !passed {
                return rejected(
                    spiral_table_index,
                    x,
                    y,
                    observations,
                    PlayerFilterKind::PlacedResourceSpacing,
                );
            }
        }
    }
    // The shipped y comparison uses width-1 as well as the x comparison.
    let not_border =
        x != 0 && y != 0 && x != prefix_facts.world_width - 1 && y != prefix_facts.world_width - 1;
    observe(
        &mut observations,
        BORDER_FILTER_VA,
        PlayerFilterKind::Border,
        None,
        (x == 0 || y == 0) as i32,
        prefix_facts.world_width - 1,
        not_border,
    );
    if !not_border {
        return rejected(
            spiral_table_index,
            x,
            y,
            observations,
            PlayerFilterKind::Border,
        );
    }

    let item = resolved_good_id == ITEM_GOOD_ID;
    if parameters.existing_resource_spacing > 0 {
        for (index, resource) in existing_resources.iter().enumerate() {
            if !matching_existing_resource(resource, item, water_good) {
                continue;
            }
            let distance = retail_approx_distance(x, y, resource.cell_x, resource.cell_y);
            let passed = distance >= parameters.existing_resource_spacing;
            observe(
                &mut observations,
                if item {
                    EXISTING_ITEM_SPACING_FILTER_VA
                } else {
                    EXISTING_GOOD_SPACING_FILTER_VA
                },
                PlayerFilterKind::ExistingResourceSpacing,
                Some(index),
                distance,
                parameters.existing_resource_spacing,
                passed,
            );
            if !passed {
                return rejected(
                    spiral_table_index,
                    x,
                    y,
                    observations,
                    PlayerFilterKind::ExistingResourceSpacing,
                );
            }
        }
    }

    // The native land-ID equality at `0x00692b57` is guarded by `param_5 == 0`.
    // Entry zero is clamped to 64 and every accepted negative domain is rejected
    // above, so the comparison is unreachable for this exact continuation.

    if !item {
        for (index, &(dx, dy)) in NEIGHBOR_OFFSETS.iter().take(8).enumerate() {
            if dx != 0 && dy != 0 {
                continue;
            }
            let nx = x + dx;
            let ny = y + dy;
            if nx < 0 || ny < 0 || nx >= prefix_facts.world_width || ny >= prefix_facts.world_height
            {
                continue;
            }
            let neighbor = cell_at(prefix_facts, facts, nx, ny);
            let passed = neighbor.primary_flags & 0x4 == 0;
            observe(
                &mut observations,
                CARDINAL_NEIGHBOR_FILTER_VA,
                PlayerFilterKind::CardinalNeighbor,
                Some(index),
                i32::from(neighbor.primary_flags),
                0x4,
                passed,
            );
            if !passed {
                return rejected(
                    spiral_table_index,
                    x,
                    y,
                    observations,
                    PlayerFilterKind::CardinalNeighbor,
                );
            }
        }
    }

    observe(
        &mut observations,
        HAS_MOUNTAIN_CALL_VA,
        PlayerFilterKind::HasMountainTcoords,
        None,
        cell.has_mountain_tcoords as i32,
        WORLD_DATA_HAS_MOUNTAIN_TCOORDS_VA as i32,
        !cell.has_mountain_tcoords,
    );
    if cell.has_mountain_tcoords {
        return rejected(
            spiral_table_index,
            x,
            y,
            observations,
            PlayerFilterKind::HasMountainTcoords,
        );
    }

    if item {
        let bad_land =
            cell.primary_flags & 0x100 == 0 && (cell.primary_land == 1 || cell.primary_land == 2);
        let passed = !bad_land && cell.primary_flags & 0x70 == 0;
        observe(
            &mut observations,
            ITEM_TERRAIN_FILTER_VA,
            PlayerFilterKind::ItemTerrain,
            None,
            i32::from(cell.primary_flags),
            i32::from(cell.primary_land),
            passed,
        );
        if !passed {
            return rejected(
                spiral_table_index,
                x,
                y,
                observations,
                PlayerFilterKind::ItemTerrain,
            );
        }
    } else {
        let class = terrain_class(cell);
        let passed = facts.terrain_rare[class as usize]
            .good_ids
            .contains(&resolved_good_id);
        observe(
            &mut observations,
            TERRAIN_RARE_FILTER_VA,
            PlayerFilterKind::TerrainRare,
            Some(class as usize),
            resolved_good_id,
            class,
            passed,
        );
        if !passed {
            return rejected(
                spiral_table_index,
                x,
                y,
                observations,
                PlayerFilterKind::TerrainRare,
            );
        }
    }

    let terminal = down_overlay
        .get(&(x, y))
        .map(|(down, _)| *down)
        .unwrap_or_else(|| down_terminal(cell).expect("down chains validated before execution"));
    let passed = terminal >= -1;
    observe(
        &mut observations,
        DOWN_CHAIN_FILTER_VA,
        PlayerFilterKind::DownChain,
        None,
        i32::from(terminal),
        -1,
        passed,
    );
    if !passed {
        return rejected(
            spiral_table_index,
            x,
            y,
            observations,
            PlayerFilterKind::DownChain,
        );
    }

    PlayerCandidateEvaluation {
        spiral_table_index,
        x,
        y,
        observations,
        rejection: None,
    }
}

fn validate_facts(
    parameters: PlayerResourceBodyParameters,
    prefix_facts: &PlayerPlacementPrefixFacts,
    facts: &PlayerResourceBodyFacts,
) -> Result<(), PlayerResourceBodyError> {
    if prefix_facts.world_width <= 1 || prefix_facts.world_height <= 1 {
        return Err(PlayerResourceBodyError::InvalidWorldShape);
    }
    let Some(cell_count) = prefix_facts
        .world_width
        .checked_mul(prefix_facts.world_height)
        .and_then(|value| usize::try_from(value).ok())
    else {
        return Err(PlayerResourceBodyError::InvalidWorldShape);
    };
    let maximum_coord = prefix_facts.world_width.max(prefix_facts.world_height) - 1;
    if maximum_coord
        .checked_mul(0x300)
        .and_then(|coord| coord.checked_add(0x180))
        .is_none()
    {
        return Err(PlayerResourceBodyError::InvalidWorldShape);
    }
    if facts.cells.len() != cell_count
        || facts.start_occupied.len() != cell_count
        || facts.cells.iter().enumerate().any(|(index, cell)| {
            cell.x != index as i32 % prefix_facts.world_width
                || cell.y != index as i32 / prefix_facts.world_width
                || !(0..=7).contains(&i32::from(cell.primary_land))
        })
    {
        return Err(PlayerResourceBodyError::InvalidCellCatalog);
    }
    for cell in &facts.cells {
        if down_terminal(cell).is_none() {
            return Err(PlayerResourceBodyError::InvalidDownChain {
                x: cell.x,
                y: cell.y,
            });
        }
    }
    if parameters.player_count > 0 && prefix_facts.starts.len() < parameters.player_count as usize {
        return Err(PlayerResourceBodyError::InvalidStartCatalog);
    }
    if prefix_facts.spiral_ring_ends.len() != 65
        || prefix_facts
            .spiral_ring_ends
            .windows(2)
            .any(|pair| pair[0] > pair[1])
    {
        return Err(PlayerResourceBodyError::InvalidSpiralCatalog);
    }
    let offset_count = prefix_facts.spiral_ring_ends[64];
    if offset_count < 0
        || facts.spiral_x_offsets.len() != offset_count as usize
        || facts.spiral_y_offsets.len() != offset_count as usize
    {
        return Err(PlayerResourceBodyError::InvalidSpiralCatalog);
    }
    if facts.terrain_rare.len() != 8
        || facts
            .terrain_rare
            .iter()
            .enumerate()
            .any(|(index, row)| row.terrain_class as usize != index)
    {
        return Err(PlayerResourceBodyError::InvalidTerrainCatalog);
    }
    if facts.good_catalog.iter().any(|good| good.good_id < 0)
        || facts.good_catalog.iter().enumerate().any(|(index, good)| {
            facts.good_catalog[..index]
                .iter()
                .any(|previous| previous.good_id == good.good_id)
        })
        || (parameters.selector == 0
            && parameters.good_id != ITEM_GOOD_ID
            && !facts
                .good_catalog
                .iter()
                .any(|good| good.good_id == parameters.good_id))
    {
        return Err(PlayerResourceBodyError::InvalidGoodCatalog);
    }
    Ok(())
}

fn good_offset(facts: &PlayerResourceBodyFacts, good_id: i32) -> Option<i32> {
    facts
        .good_catalog
        .iter()
        .find(|fact| fact.good_id == good_id)
        .map(|fact| if fact.uses_offset_0x120 { 0x120 } else { 0x180 })
}

fn validate_prefix_draw(random: &mut Random, draw: ExactRandomDraw, modulus: i32) -> bool {
    if draw.call_va != PLAYER_RANDOM_GET_CALL_VA
        || draw.random_get_va != RANDOM_GET_VA
        || draw.low != 0
        || draw.high != 0xffff
        || draw.modulus != modulus
        || draw.state_before != random.state()
    {
        return false;
    }
    let raw = random.get(0, 0xffff);
    draw.raw == raw && draw.remainder == raw % modulus && draw.state_after == random.state()
}

fn draw_point(random: &mut Random, span: i32) -> PlayerBodyRandomDraw {
    let state_before = random.state();
    let raw = random.get(0, 0xffff);
    PlayerBodyRandomDraw {
        call_va: PLAYER_RANDOM_GET_CALL_VA,
        random_get_va: RANDOM_GET_VA,
        low: 0,
        high: 0xffff,
        state_before,
        raw,
        modulus: span + 1,
        remainder: raw % (span + 1),
        state_after: random.state(),
    }
}

fn validate_allocation_receipt(
    index: usize,
    receipt: &PlayerAllocationReceipt,
) -> Result<(), PlayerResourceBodyError> {
    let request = &receipt.request;
    let allocation = &receipt.allocation;
    if allocation.call_va != request.call_va
        || allocation.callee_va != request.callee_va
        || allocation.kind != request.kind
        || allocation.coord_x != request.coord_x
        || allocation.coord_y != request.coord_y
        || allocation.slot < 0
        || allocation.slot > i32::from(i16::MAX)
        || receipt.sourced_walked_bytes_after != request.sourced_walked_bytes
    {
        return Err(PlayerResourceBodyError::InvalidAllocationReceipt { index });
    }
    match request.kind {
        PlayerAllocationKind::Good { good_id } => {
            if request.call_va != PLAYER_INIT_GOOD_CALL_VA
                || request.callee_va != OBJECTS_INIT_GOOD_VA
                || receipt.good_state_digest_after == request.good_state_digest_before
                || receipt.item_state_digest_after != request.item_state_digest_before
            {
                return Err(PlayerResourceBodyError::InvalidAllocationReceipt { index });
            }
            if good_id == 5 {
                if allocation.occupancy_write.is_some()
                    || receipt.world_checksum_after != request.world_checksum_before
                {
                    return Err(PlayerResourceBodyError::InvalidAllocationReceipt { index });
                }
                return Ok(());
            }
        }
        PlayerAllocationKind::Item => {
            if request.call_va != PLAYER_INIT_ITEM_CALL_VA
                || request.callee_va != OBJECTS_INIT_ITEM_VA
                || receipt.item_state_digest_after == request.item_state_digest_before
                || receipt.good_state_digest_after != request.good_state_digest_before
            {
                return Err(PlayerResourceBodyError::InvalidAllocationReceipt { index });
            }
        }
    }
    let Some(write) = &allocation.occupancy_write else {
        return Err(PlayerResourceBodyError::InvalidAllocationReceipt { index });
    };
    let (down_write_va, down_who_write_va, marker) = match request.kind {
        PlayerAllocationKind::Good { .. } => (
            GOOD_WDATA_DOWN_WRITE_VA,
            GOOD_WDATA_DOWN_WHO_WRITE_VA,
            GOOD_WDATA_DOWN_MARKER,
        ),
        PlayerAllocationKind::Item => (
            ITEM_WDATA_DOWN_WRITE_VA,
            ITEM_WDATA_DOWN_WHO_WRITE_VA,
            ITEM_WDATA_DOWN_MARKER,
        ),
    };
    if write.world_x != request.world_x
        || write.world_y != request.world_y
        || write.down_write_va != down_write_va
        || write.down_who_write_va != down_who_write_va
        || write.old_down != request.old_down
        || write.old_down_who != request.old_down_who
        || write.new_down != marker
        || write.new_down_who != allocation.slot as i16
        || request
            .world_checksum_before
            .differing_sections(&receipt.world_checksum_after)
            != [WorldSection::WData]
    {
        return Err(PlayerResourceBodyError::InvalidAllocationReceipt { index });
    }
    Ok(())
}

/// Execute the complete shipped player-placement function atomically.
pub fn execute_player_resource_body<H: PlayerAllocationHost>(
    state: &mut PlayerResourceBodyState,
    parameters: PlayerResourceBodyParameters,
    prefix_facts: &PlayerPlacementPrefixFacts,
    prefix: &PlayerPlacementPrefixReceipt,
    facts: &PlayerResourceBodyFacts,
    host: &mut H,
) -> Result<PlayerResourceBodyReceipt, PlayerResourceBodyError> {
    let prefix_request = PlayerPlacementPrefixRequest {
        player_count: parameters.player_count,
        good_id: parameters.good_id,
        num_rare: parameters.num_rare,
        spacing: parameters.player_keep_away,
        group_spacing: parameters.player_stay_near,
        selector: parameters.selector,
    };
    let expected_prefix =
        execute_player_placement_prefix(&state.deterministic, prefix_request, prefix_facts)
            .map_err(PlayerResourceBodyError::Prefix)?;
    if expected_prefix != *prefix
        || prefix.entry_va != MAP_PLACE_PLAYER_RESOURCE_VA
        || prefix.random_state_before != state.deterministic.random_state
        || prefix.world_checksum != state.deterministic.world_checksum
    {
        return Err(PlayerResourceBodyError::WrongPrefix);
    }
    if resource_divvy_pool_digest(&state.resource_pool) != state.deterministic.resource_pool_digest
    {
        return Err(PlayerResourceBodyError::InvalidPoolDigest);
    }
    validate_facts(parameters, prefix_facts, facts)?;
    if !facts.placement_evidence.admissible() {
        return Err(PlayerResourceBodyError::InvalidPlacementEvidence);
    }
    let facts_digest = player_resource_body_facts_digest(parameters, prefix_facts, facts);
    let prefix_digest = player_placement_prefix_receipt_digest(prefix);
    if !facts
        .evidence
        .admissible(state, facts_digest, prefix_digest)
    {
        return Err(PlayerResourceBodyError::StaleEvidence);
    }
    let valid_start = |start: &crate::place_resources_category_frontier::PlayerStartFact| {
        start.x >= 0
            && start.y >= 0
            && start.x < prefix_facts.world_width
            && start.y < prefix_facts.world_height
    };
    let has_valid_player = parameters.player_count > 0
        && prefix_facts
            .starts
            .iter()
            .take(parameters.player_count as usize)
            .any(valid_start);
    let body_active = has_valid_player && parameters.num_rare > 0;

    if parameters.selector != 0 {
        let goods = if parameters.selector == 2 {
            &state.resource_pool.early_goods
        } else {
            &state.resource_pool.late_goods
        };
        if body_active
            && goods.iter().filter(|good| **good != -1).any(|good| {
                !facts
                    .good_catalog
                    .iter()
                    .any(|catalog| catalog.good_id == *good)
            })
        {
            return Err(PlayerResourceBodyError::InvalidGoodCatalog);
        }
        let first_start_is_valid = prefix_facts.starts.first().is_some_and(valid_start);
        if parameters.player_count > 0
            && parameters.num_rare > 0
            && first_start_is_valid
            && !matches!(
                prefix.disposition,
                PlayerPrefixDisposition::PoolSelectionBoundary {
                    call_va: PLAYER_POOL_SELECTION_VA
                }
            )
        {
            return Err(PlayerResourceBodyError::WrongPrefix);
        }
    } else if matches!(
        prefix.disposition,
        PlayerPrefixDisposition::PoolSelectionBoundary { .. }
    ) {
        return Err(PlayerResourceBodyError::WrongPrefix);
    }

    let stay_near =
        if body_active && (parameters.player_stay_near > 64 || parameters.player_stay_near == 0) {
            64
        } else {
            parameters.player_stay_near
        };
    let spacing = if body_active {
        parameters.player_keep_away.min(stay_near)
    } else {
        parameters.player_keep_away
    };
    if body_active && (stay_near < 0 || spacing < 0) {
        return Err(PlayerResourceBodyError::InvalidSpiralCatalog);
    }
    let span = if body_active {
        prefix_facts.spiral_ring_ends[stay_near as usize]
            - prefix_facts.spiral_ring_ends[spacing as usize]
    } else {
        0
    };
    if span < 0 {
        return Err(PlayerResourceBodyError::InvalidSpiralCatalog);
    }

    let deterministic_before = state.deterministic.clone();
    let resource_pool_before = state.resource_pool.clone();
    let good_state_digest_before = state.good_state_digest;
    let item_state_digest_before = state.item_state_digest;
    let mut random = Random::new(state.deterministic.random_state);
    let mut pool = state.resource_pool.clone();
    let mut current_world_checksum = state.deterministic.world_checksum.clone();
    let mut current_good_digest = state.good_state_digest;
    let mut current_item_digest = state.item_state_digest;
    let mut existing_resources = facts.existing_resources.clone();
    let mut down_overlay: BTreeMap<(i32, i32), (i16, i16)> = BTreeMap::new();
    let mut placed_points = Vec::new();
    let mut attempts = Vec::new();
    let mut pool_selections = Vec::new();
    let mut random_draws = Vec::new();
    let mut allocations = Vec::new();
    let water_good = parameters.good_id == 6 || parameters.good_id == 31;

    for player_index in 0..parameters.player_count.max(0) as usize {
        let start = prefix_facts.starts[player_index];
        // `validate_facts` pins the normal shipped domain. Keep the outer-loop
        // condition visible in the receipt owner because retail tests it per player.
        if start.x < 0
            || start.y < 0
            || start.x >= prefix_facts.world_width
            || start.y >= prefix_facts.world_height
        {
            continue;
        }
        for attempt_index in 0..parameters.num_rare.max(0) {
            let (resolved_good_id, selector_call_va, pool_selection) = if parameters.selector == 0 {
                (parameters.good_id, None, None)
            } else {
                let lane = if parameters.selector == 2 {
                    ResourcePoolLane::Early
                } else {
                    ResourcePoolLane::Late
                };
                let selection = execute_resource_pool_selection(&mut pool, lane, &mut random)
                    .map_err(PlayerResourceBodyError::PoolSelection)?;
                let expected_entry = if lane == ResourcePoolLane::Early {
                    RESOURCE_POOL_GET_EARLY_VA
                } else {
                    RESOURCE_POOL_GET_LATE_VA
                };
                if selection.entry_va != expected_entry {
                    return Err(PlayerResourceBodyError::WrongPrefix);
                }
                let selected = selection.selected_good;
                pool_selections.push(selection.clone());
                (
                    selected,
                    Some(if lane == ResourcePoolLane::Early {
                        PLAYER_GET_EARLY_CALL_VA
                    } else {
                        PLAYER_GET_LATE_CALL_VA
                    }),
                    Some(selection),
                )
            };
            if resolved_good_id != ITEM_GOOD_ID && good_offset(facts, resolved_good_id).is_none() {
                return Err(PlayerResourceBodyError::InvalidGoodCatalog);
            }

            let point_draw = if span > 0 {
                if parameters.selector == 0 && player_index == 0 && attempt_index == 0 {
                    let Some(draw) = prefix.point_draw else {
                        return Err(PlayerResourceBodyError::WrongPrefix);
                    };
                    if !validate_prefix_draw(&mut random, draw, span + 1) {
                        return Err(PlayerResourceBodyError::WrongPrefix);
                    }
                    let converted: PlayerBodyRandomDraw = draw.into();
                    random_draws.push(converted);
                    Some(converted)
                } else {
                    let draw = draw_point(&mut random, span);
                    random_draws.push(draw);
                    Some(draw)
                }
            } else {
                None
            };
            let scan_start_remainder = point_draw.map(|draw| draw.remainder).unwrap_or(0);
            let mut candidate_receipts = Vec::new();
            let mut allocation_index = None;

            for scan_offset in 0..span {
                let table_index = (scan_start_remainder + scan_offset) % span
                    + prefix_facts.spiral_ring_ends[spacing as usize];
                let x = start.x + i32::from(facts.spiral_x_offsets[table_index as usize]);
                let y = start.y + i32::from(facts.spiral_y_offsets[table_index as usize]);
                let evaluation = evaluate_candidate(
                    table_index,
                    x,
                    y,
                    player_index,
                    resolved_good_id,
                    water_good,
                    spacing,
                    stay_near,
                    parameters,
                    prefix_facts,
                    facts,
                    &placed_points,
                    &existing_resources,
                    &down_overlay,
                );
                let accepted = evaluation.rejection.is_none();
                candidate_receipts.push(evaluation);
                if !accepted {
                    continue;
                }

                let cell = cell_at(prefix_facts, facts, x, y);
                let (old_down, old_down_who) = down_overlay
                    .get(&(x, y))
                    .copied()
                    .unwrap_or((cell.down_chain.initial_index, cell.down_chain.initial_who));
                let (kind, call_va, callee_va, offset) = if resolved_good_id == ITEM_GOOD_ID {
                    (
                        PlayerAllocationKind::Item,
                        PLAYER_INIT_ITEM_CALL_VA,
                        OBJECTS_INIT_ITEM_VA,
                        0x180,
                    )
                } else {
                    (
                        PlayerAllocationKind::Good {
                            good_id: resolved_good_id,
                        },
                        PLAYER_INIT_GOOD_CALL_VA,
                        OBJECTS_INIT_GOOD_VA,
                        good_offset(facts, resolved_good_id)
                            .expect("catalog checked before candidate scan"),
                    )
                };
                let request = PlayerAllocationRequest {
                    call_va,
                    callee_va,
                    kind,
                    world_x: x,
                    world_y: y,
                    coord_x: x * 0x300 + offset,
                    coord_y: y * 0x300 + offset,
                    old_down,
                    old_down_who,
                    world_checksum_before: current_world_checksum.clone(),
                    good_state_digest_before: current_good_digest,
                    item_state_digest_before: current_item_digest,
                    sourced_walked_bytes: state.deterministic.sourced_walked_bytes,
                };
                let index = allocations.len();
                let Some(proposed) = host.propose_allocation(&request) else {
                    return Err(PlayerResourceBodyError::AllocationUnavailable {
                        player_index,
                        x,
                        y,
                    });
                };
                if proposed.request != request {
                    return Err(PlayerResourceBodyError::InvalidAllocationReceipt { index });
                }
                validate_allocation_receipt(index, &proposed)?;
                current_world_checksum = proposed.world_checksum_after.clone();
                current_good_digest = proposed.good_state_digest_after;
                current_item_digest = proposed.item_state_digest_after;
                let marker = match kind {
                    PlayerAllocationKind::Good { good_id: 5 } => None,
                    PlayerAllocationKind::Good { .. } => Some(GOOD_WDATA_DOWN_MARKER),
                    PlayerAllocationKind::Item => Some(ITEM_WDATA_DOWN_MARKER),
                };
                if let Some(marker) = marker {
                    down_overlay.insert((x, y), (marker, proposed.allocation.slot as i16));
                }
                existing_resources.push(ExistingResourceFact {
                    active: true,
                    kind: match kind {
                        PlayerAllocationKind::Good { good_id } => {
                            ExistingResourceKind::Good { good_id }
                        }
                        PlayerAllocationKind::Item => ExistingResourceKind::Item,
                    },
                    cell_x: x,
                    cell_y: y,
                });
                placed_points.push((x, y));
                allocations.push(proposed.allocation);
                allocation_index = Some(index);
                break;
            }
            attempts.push(PlayerAttemptReceipt {
                player_index,
                attempt_index,
                resolved_good_id,
                selector_call_va,
                pool_selection,
                point_draw,
                candidate_span: span,
                scan_start_remainder,
                candidates: candidate_receipts,
                allocation_index,
                attempt_tail_va: ATTEMPT_TAIL_VA,
            });
        }
    }

    if parameters.selector == 0 && span > 0 && prefix.point_draw.is_some() {
        if random_draws.first().copied() != prefix.point_draw.map(Into::into) {
            return Err(PlayerResourceBodyError::WrongPrefix);
        }
    } else if parameters.selector == 0 && prefix.point_draw.is_some() {
        return Err(PlayerResourceBodyError::WrongPrefix);
    }
    let return_count = allocations.len() as i32;
    let mut deterministic_after = state.deterministic.clone();
    deterministic_after.random_state = random.state();
    deterministic_after.world_checksum = current_world_checksum;
    deterministic_after.resource_pool_digest = resource_divvy_pool_digest(&pool);
    deterministic_after.allocated_resources = deterministic_after
        .allocated_resources
        .wrapping_add(return_count);
    // Requested count and all category chance locals belong to the caller and carry unchanged.
    let receipt = PlayerResourceBodyReceipt {
        entry_va: MAP_PLACE_PLAYER_RESOURCE_VA,
        outer_player_tail_va: OUTER_PLAYER_TAIL_VA,
        function_return_value_va: FUNCTION_RETURN_VALUE_VA,
        caller_return_accumulate_va: CALLER_RETURN_ACCUMULATE_VA,
        parameters,
        water_good,
        spacing_after_clamp: spacing,
        player_stay_near_after_clamp: stay_near,
        attempts,
        pool_selections,
        random_draws,
        allocations,
        return_count,
        deterministic_before,
        deterministic_after: deterministic_after.clone(),
        resource_pool_before,
        resource_pool_after: pool.clone(),
        good_state_digest_before,
        good_state_digest_after: current_good_digest,
        item_state_digest_before,
        item_state_digest_after: current_item_digest,
        placement_evidence: facts.placement_evidence.clone(),
    };
    state.deterministic = deterministic_after;
    state.resource_pool = pool;
    state.good_state_digest = current_good_digest;
    state.item_state_digest = current_item_digest;
    Ok(receipt)
}
