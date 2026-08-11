// SPDX-License-Identifier: GPL-3.0-or-later
//! Source-only owner for the first `BONUS` row in `Map::place_resources`.
//!
//! The caller-owned span is `0x0068fb9d..0x00690215`: it selects the first XML row,
//! resolves its type and placement attributes, performs the direct chance draw at
//! `0x0068fd64`, and dispatches one of the two placement callees.  The placement bodies
//! remain deliberately external, but their shared-RNG, resource-pool, allocation, and
//! `World::WData` effects cross a typed, checksum-bound, two-phase receipt.

use crate::place_resources_pool_frontier::{resource_divvy_pool_digest, ResourceDivvyPoolState};
use crate::place_resources_xml_frontier::{
    BonusesSectionSource, PlaceResourcesBonusRowsHandoff, XmlHostHandles,
    PLACE_RESOURCES_XML_RESIDUAL_VA,
};
use crate::resource_divvy_pool_selection_frontier::{
    validate_resource_pool_selection, ResourcePoolLane, ResourcePoolSelectionReceipt,
};
use don_sim::rng::Random;
use don_sim::systems::map_terrain::{div_3, WorldChecksum, WorldSection};

pub const SHIPPED_EXE_SHA256: &str =
    "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079";
pub const SHIPPED_PDB_SHA256: &str =
    "334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5";

pub const FIRST_BONUS_ROW_LOOP_INIT_VA: u32 = 0x0068_fb9d;
pub const FIRST_BONUS_ROW_BODY_VA: u32 = 0x0068_fbb3;
pub const FIRST_BONUS_ROW_RESIDUAL_VA: u32 = 0x0069_0215;
pub const NEXT_BONUS_ROW_VA: u32 = 0x0068_fbb3;
pub const BONUS_CATEGORY_TAIL_VA: u32 = 0x0069_0225;

pub const ROW_OLD_TAIL_RELEASE_CALL_VA: u32 = 0x0068_fbc0;
pub const ROW_OLD_HEAD_RELEASE_CALL_VA: u32 = 0x0068_fbda;
pub const ROW_NEW_HEAD_ACQUIRE_CALL_VA: u32 = 0x0068_fbf7;
pub const ROW_NEW_TAIL_ACQUIRE_CALL_VA: u32 = 0x0068_fc0a;
pub const TYPE_GET_ATTRIB_CALL_VA: u32 = 0x0068_fc30;
pub const GOOD_KEY_CALL_VA: u32 = 0x0068_fc5a;
pub const SELECTOR_ONE_IGNORE_CALL_VA: u32 = 0x0068_fc81;
pub const ITEM_GOOD_IGNORE_CALL_VA: u32 = 0x0068_fcac;
pub const SELECTOR_TWO_IGNORE_CALL_VA: u32 = 0x0068_fcd3;
pub const SELECTOR_THREE_IGNORE_CALL_VA: u32 = 0x0068_fcfa;
pub const CHANCE_GET_ATTRIB_NUM_CALL_VA: u32 = 0x0068_fd27;
pub const CHANCE_GROUP_GET_ATTRIB_NUM_CALL_VA: u32 = 0x0068_fd40;
pub const CHANCE_RANDOM_GET_CALL_VA: u32 = 0x0068_fd64;
pub const NUM_RARE_SCALE_CALL_VA: u32 = 0x0068_fdcf;
pub const PATTERN_GET_ATTRIB_CALL_VA: u32 = 0x0068_fe35;
pub const SATURATE_GET_ATTRIB_NUM_CALL_VA: u32 = 0x0068_ff21;
pub const SPACING_GET_ATTRIB_NUM_CALL_VA: u32 = 0x0068_ff45;
pub const GROUP_SPACING_SCALE_CALL_VA: u32 = 0x0068_ff67;
pub const PLAYER_KEEP_AWAY_SCALE_CALL_VA: u32 = 0x0068_ff9a;
pub const PLAYER_STAY_NEAR_SCALE_CALL_VA: u32 = 0x0068_ffc7;
pub const CENTER_KEEP_AWAY_SCALE_CALL_VA: u32 = 0x0068_fff4;
pub const CENTER_STAY_NEAR_SCALE_CALL_VA: u32 = 0x0069_0017;
pub const CORNER_KEEP_AWAY_SCALE_CALL_VA: u32 = 0x0069_003a;
pub const CORNER_STAY_NEAR_SCALE_CALL_VA: u32 = 0x0069_005d;
pub const EDGE_KEEP_AWAY_SCALE_CALL_VA: u32 = 0x0069_0080;
pub const EDGE_STAY_NEAR_SCALE_CALL_VA: u32 = 0x0069_00a3;
pub const PLAYER_PLACEMENT_CALL_VA: u32 = 0x0069_00dc;
pub const REGION_PLACEMENT_CALL_VA: u32 = 0x0069_01ec;

pub const XMLELEMENT_GET_ATTRIB_VA: u32 = 0x00a2_7900;
pub const XMLELEMENT_GET_ATTRIB_NUM_VA: u32 = 0x00a2_7580;
pub const TYPES_GOOD_KEY_VA: u32 = 0x0066_91c0;
pub const STRING_IGNORE_VA: u32 = 0x00a1_ba80;
pub const MAP_SCALE_NUMBER_VA: u32 = 0x006a_0320;
pub const MAP_PLACE_REGION_RESOURCE_VA: u32 = 0x0069_0480;
pub const MAP_PLACE_PLAYER_RESOURCE_VA: u32 = 0x0069_1f70;
pub const RANDOM_GET_VA: u32 = 0x00a3_9d70;
pub const RESOURCE_POOL_GET_WATER_RANDOM_CALL_VA: u32 = 0x0068_a4cd;
pub const RESOURCE_POOL_GET_LATE_RANDOM_CALL_VA: u32 = 0x0068_a59d;
pub const RESOURCE_POOL_GET_EARLY_RANDOM_CALL_VA: u32 = 0x0068_a66c;
/// Transitive main-RNG call in `Map::find_avail_regions`, reached only from
/// `Map::place_region_resource` and chronologically before all region-body draws.
pub const REGION_FIND_AVAIL_RANDOM_CALL_VA: u32 = 0x0068_f498;
pub const REGION_PLACEMENT_RANDOM_CALL_VA: u32 = 0x0069_071a;
pub const PLAYER_PLACEMENT_RANDOM_CALL_VA: u32 = 0x0069_2114;

pub const REGION_INIT_GOOD_CALL_VA: u32 = 0x0069_157c;
pub const REGION_INIT_ITEM_CALL_VA: u32 = 0x0069_1596;
pub const PLAYER_INIT_GOOD_CALL_VA: u32 = 0x0069_2d90;
pub const PLAYER_INIT_ITEM_CALL_VA: u32 = 0x0069_2db5;
pub const OBJECTS_INIT_ITEM_VA: u32 = 0x0065_3e00;
pub const OBJECTS_INIT_GOOD_VA: u32 = 0x0065_3f30;
pub const ITEM_WDATA_DOWN_WRITE_VA: u32 = 0x0065_3f05;
pub const ITEM_WDATA_DOWN_WHO_WRITE_VA: u32 = 0x0065_3f0c;
pub const GOOD_WDATA_DOWN_WRITE_VA: u32 = 0x0065_403e;
pub const GOOD_WDATA_DOWN_WHO_WRITE_VA: u32 = 0x0065_4043;
pub const ITEM_GOOD_ID: i32 = 0x21f;
pub const GOOD_WDATA_DOWN_MARKER: i16 = -2;
pub const ITEM_WDATA_DOWN_MARKER: i16 = -3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceTypeResolution {
    CatalogGood { good_id: i32 },
    PoolSelector { selector: u8, matched_call_va: u32 },
    ItemGood,
    Unknown,
}

impl ResourceTypeResolution {
    pub(crate) fn good_and_selector(self) -> Option<(i32, i32)> {
        match self {
            Self::CatalogGood { good_id } if good_id >= 0 => Some((good_id, 0)),
            Self::PoolSelector {
                selector,
                matched_call_va,
            } if (selector == 1 && matched_call_va == SELECTOR_ONE_IGNORE_CALL_VA)
                || (selector == 2 && matched_call_va == SELECTOR_TWO_IGNORE_CALL_VA)
                || (selector == 3 && matched_call_va == SELECTOR_THREE_IGNORE_CALL_VA) =>
            {
                Some((-1, i32::from(selector)))
            }
            Self::ItemGood => Some((ITEM_GOOD_ID, 0)),
            Self::Unknown => None,
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum PlacementPattern {
    Player = 0,
    World = 1,
    NonPlayer = 2,
    Corner = 3,
    Edge = 4,
}

impl PlacementPattern {
    pub(crate) fn path(self) -> PlacementPath {
        if self == Self::Player {
            PlacementPath::Player
        } else {
            PlacementPath::Region
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaledAttribute {
    NumRare,
    GroupSpacing,
    PlayerKeepAway,
    PlayerStayNear,
    CenterKeepAway,
    CenterStayNear,
    CornerKeepAway,
    CornerStayNear,
    EdgeKeepAway,
    EdgeStayNear,
}

impl ScaledAttribute {
    const ORDERED: [(Self, u32); 10] = [
        (Self::NumRare, NUM_RARE_SCALE_CALL_VA),
        (Self::GroupSpacing, GROUP_SPACING_SCALE_CALL_VA),
        (Self::PlayerKeepAway, PLAYER_KEEP_AWAY_SCALE_CALL_VA),
        (Self::PlayerStayNear, PLAYER_STAY_NEAR_SCALE_CALL_VA),
        (Self::CenterKeepAway, CENTER_KEEP_AWAY_SCALE_CALL_VA),
        (Self::CenterStayNear, CENTER_STAY_NEAR_SCALE_CALL_VA),
        (Self::CornerKeepAway, CORNER_KEEP_AWAY_SCALE_CALL_VA),
        (Self::CornerStayNear, CORNER_STAY_NEAR_SCALE_CALL_VA),
        (Self::EdgeKeepAway, EDGE_KEEP_AWAY_SCALE_CALL_VA),
        (Self::EdgeStayNear, EDGE_STAY_NEAR_SCALE_CALL_VA),
    ];
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScaledAttributeFact {
    pub attribute: ScaledAttribute,
    pub call_va: u32,
    pub expression: String,
    /// Captured result of `Map::scale_number(player_count)` for `expression`.
    pub scaled: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BonusMutationEvidence {
    RetailCapture {
        executable_sha256: String,
        pdb_sha256: String,
        capture_sha256: [u8; 32],
        entry_va: u32,
        capture_ordinal: u32,
        random_state: i32,
        world_checksum: WorldChecksum,
        sourced_walked_bytes: u64,
        resource_pool_digest: u64,
    },
    SyntheticFixture {
        fixture: String,
        entry_va: u32,
        capture_ordinal: u32,
        random_state: i32,
        world_checksum: WorldChecksum,
        sourced_walked_bytes: u64,
        resource_pool_digest: u64,
    },
}

impl BonusMutationEvidence {
    pub(crate) fn admissible(&self, entry: &PlaceResourcesBonusRowsHandoff, ordinal: u32) -> bool {
        let fields_match = |entry_va: u32,
                            capture_ordinal: u32,
                            random_state: i32,
                            world_checksum: &WorldChecksum,
                            sourced_walked_bytes: u64,
                            resource_pool_digest: u64| {
            entry_va == entry.resume_va
                && capture_ordinal == ordinal
                && random_state == entry.random_state
                && world_checksum == &entry.world_checksum
                && sourced_walked_bytes == entry.sourced_walked_bytes
                && resource_pool_digest == entry.resource_pool_digest
        };
        match self {
            Self::RetailCapture {
                executable_sha256,
                pdb_sha256,
                capture_sha256,
                entry_va,
                capture_ordinal,
                random_state,
                world_checksum,
                sourced_walked_bytes,
                resource_pool_digest,
            } => {
                executable_sha256 == SHIPPED_EXE_SHA256
                    && pdb_sha256 == SHIPPED_PDB_SHA256
                    && capture_sha256.iter().any(|byte| *byte != 0)
                    && fields_match(
                        *entry_va,
                        *capture_ordinal,
                        *random_state,
                        world_checksum,
                        *sourced_walked_bytes,
                        *resource_pool_digest,
                    )
            }
            Self::SyntheticFixture {
                fixture,
                entry_va,
                capture_ordinal,
                random_state,
                world_checksum,
                sourced_walked_bytes,
                resource_pool_digest,
            } => {
                cfg!(test)
                    && !fixture.is_empty()
                    && fields_match(
                        *entry_va,
                        *capture_ordinal,
                        *random_state,
                        world_checksum,
                        *sourced_walked_bytes,
                        *resource_pool_digest,
                    )
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirstBonusMutationFacts {
    pub capture_ordinal: u32,
    pub type_name: String,
    pub type_resolution: ResourceTypeResolution,
    /// `chance`, default `-1` at call `0x0068fd27`.
    pub chance: i32,
    /// Native chance-bucket key, default `-1` at call `0x0068fd40`.
    pub chance_group: i32,
    pub pattern_name: String,
    pub pattern: PlacementPattern,
    pub saturate: i32,
    pub spacing: i32,
    /// Exactly ten `Map::scale_number` results in native call order.
    pub scaled: Vec<ScaledAttributeFact>,
    pub evidence: BonusMutationEvidence,
}

impl FirstBonusMutationFacts {
    fn scaled_value(&self, attribute: ScaledAttribute) -> Option<i32> {
        self.scaled
            .iter()
            .find(|fact| fact.attribute == attribute)
            .map(|fact| fact.scaled)
    }

    fn validate_scaled(&self) -> bool {
        self.scaled.len() == ScaledAttribute::ORDERED.len()
            && self.scaled.iter().zip(ScaledAttribute::ORDERED).all(
                |(fact, (attribute, call_va))| {
                    fact.attribute == attribute && fact.call_va == call_va
                },
            )
    }

    fn validate_num_rare(&self) -> bool {
        self.scaled.first().is_some_and(|fact| {
            fact.attribute == ScaledAttribute::NumRare && fact.call_va == NUM_RARE_SCALE_CALL_VA
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceResourcesBonusMutationState {
    pub random_state: i32,
    pub world_checksum: WorldChecksum,
    pub sourced_walked_bytes: u64,
    pub resource_pool_digest: u64,
    /// Concrete pool continuity is required before a pool-selector placement can execute.
    /// The digest-only XML handoff may leave this `None` for catalog/item rows.
    pub resource_pool: Option<ResourceDivvyPoolState>,
    pub allocated_resources: i32,
    pub requested_resources: i32,
}

impl PlaceResourcesBonusMutationState {
    pub fn from_handoff(handoff: &PlaceResourcesBonusRowsHandoff) -> Self {
        Self {
            random_state: handoff.random_state,
            world_checksum: handoff.world_checksum.clone(),
            sourced_walked_bytes: handoff.sourced_walked_bytes,
            resource_pool_digest: handoff.resource_pool_digest,
            resource_pool: None,
            allocated_resources: 0,
            requested_resources: 0,
        }
    }

    /// Bind the digest-only XML seam to the exact pool projection produced by the prefix owner.
    pub fn from_handoff_with_pool(
        handoff: &PlaceResourcesBonusRowsHandoff,
        pool: &ResourceDivvyPoolState,
    ) -> Option<Self> {
        (resource_divvy_pool_digest(pool) == handoff.resource_pool_digest).then(|| Self {
            random_state: handoff.random_state,
            world_checksum: handoff.world_checksum.clone(),
            sourced_walked_bytes: handoff.sourced_walked_bytes,
            resource_pool_digest: handoff.resource_pool_digest,
            resource_pool: Some(pool.clone()),
            allocated_resources: 0,
            requested_resources: 0,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallbackKind {
    GetTypeAttribute,
    ResolveGoodKey,
    MatchTypeFallback,
    GetChance,
    GetChanceGroup,
    Scale(ScaledAttribute),
    GetPatternAttribute,
    GetSaturate,
    GetSpacing,
    PlacePlayerResource,
    PlaceRegionResource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostCallback {
    pub call_va: u32,
    pub callee_va: u32,
    pub kind: CallbackKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChanceRandomDraw {
    pub call_va: u32,
    pub random_get_va: u32,
    pub low: i32,
    pub high: i32,
    pub state_before: i32,
    pub raw: i32,
    pub modulo_100: i32,
    pub state_after: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlacementPath {
    Player,
    Region,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlacementParameters {
    pub player_count: i32,
    pub good_id: i32,
    pub selector: i32,
    pub pattern: PlacementPattern,
    pub saturate: i32,
    pub num_rare: i32,
    pub spacing: i32,
    pub group_spacing: i32,
    pub player_keep_away: i32,
    pub player_stay_near: i32,
    pub center_keep_away: i32,
    pub center_stay_near: i32,
    pub corner_keep_away: i32,
    pub corner_stay_near: i32,
    pub edge_keep_away: i32,
    pub edge_stay_near: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlacementRequest {
    pub call_va: u32,
    pub callee_va: u32,
    pub path: PlacementPath,
    pub params: PlacementParameters,
    pub random_state_before: i32,
    pub world_checksum_before: WorldChecksum,
    pub sourced_walked_bytes: u64,
    pub resource_pool_digest_before: u64,
    /// Present when the caller owns the canonical six-field pool projection.
    pub resource_pool_before: Option<ResourceDivvyPoolState>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalleeRandomDraw {
    pub call_va: u32,
    pub random_get_va: u32,
    pub low: i32,
    pub high: i32,
    pub state_before: i32,
    pub raw: i32,
    pub state_after: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllocationKind {
    Good,
    Item,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldOccupancyWrite {
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
pub struct ResourceAllocation {
    pub call_va: u32,
    pub callee_va: u32,
    pub kind: AllocationKind,
    pub type_id: i32,
    pub coord_x: i32,
    pub coord_y: i32,
    pub slot: i32,
    /// `Objects::init_good(5, ...)` deliberately does not claim WData occupancy.
    pub occupancy_write: Option<WorldOccupancyWrite>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlacementEvidence {
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

impl PlacementEvidence {
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
pub struct PlacementReceipt {
    pub request: PlacementRequest,
    pub random_draws: Vec<CalleeRandomDraw>,
    pub random_state_after: i32,
    pub allocations: Vec<ResourceAllocation>,
    pub allocated_count: i32,
    pub world_checksum_after: WorldChecksum,
    pub sourced_walked_bytes_after: u64,
    pub resource_pool_digest_after: u64,
    /// Ordered exact selector transactions performed by the opaque placement body.
    pub resource_pool_selections: Vec<ResourcePoolSelectionReceipt>,
    /// Canonical post-call projection. Required for selector placements.
    pub resource_pool_after: Option<ResourceDivvyPoolState>,
    pub evidence: PlacementEvidence,
}

/// The external placement body must be observational/two-phase: this owner validates the
/// complete receipt before it commits its local RNG/checksum/pool state.
pub trait PlacementHost {
    fn place(&mut self, request: &PlacementRequest) -> Option<PlacementReceipt>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirstBonusDisposition {
    UnknownType,
    ChanceMiss,
    ZeroRequested,
    Placed(PlacementPath),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirstBonusMutationReceipt {
    pub entry_va: u32,
    pub row_body_va: u32,
    pub residual_va: u32,
    pub next_va: u32,
    pub section_source: BonusesSectionSource,
    pub capture_ordinal: u32,
    /// Exact behavior-driving fact projection consumed to produce this receipt.
    pub facts: FirstBonusMutationFacts,
    pub row_handles: XmlHostHandles,
    pub callbacks: Vec<HostCallback>,
    pub chance_draw: Option<ChanceRandomDraw>,
    pub chance_budget_before_subtract: i32,
    pub chance_budget_after_subtract: i32,
    pub chance_winner_seen: bool,
    pub disposition: FirstBonusDisposition,
    pub placement: Option<PlacementReceipt>,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub world_checksum_before: WorldChecksum,
    pub world_checksum_after: WorldChecksum,
    pub sourced_walked_bytes: u64,
    pub resource_pool_digest_before: u64,
    pub resource_pool_digest_after: u64,
    pub allocated_resources_after: i32,
    pub requested_resources_after: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FirstBonusMutationError {
    WrongHandoff,
    NoBonusRow,
    StateMismatch,
    StaleEvidence,
    InvalidTypeResolution,
    InvalidScaledAttributes,
    PlacementReceiptUnavailable { request: PlacementRequest },
    PlacementReceiptMismatch,
    ConcreteResourcePoolUnavailable,
    InvalidResourcePoolSelection { index: usize },
    InvalidPlacementEvidence,
    InvalidCalleeRandomDraw { index: usize },
    InvalidAllocation { index: usize },
    InvalidWorldChecksumDelta,
}

fn callback(call_va: u32, callee_va: u32, kind: CallbackKind) -> HostCallback {
    HostCallback {
        call_va,
        callee_va,
        kind,
    }
}

fn validate_random_draws(
    path: PlacementPath,
    selector: i32,
    state_before: i32,
    draws: &[CalleeRandomDraw],
    expected_after: i32,
) -> Result<(), usize> {
    let path_call = match path {
        PlacementPath::Player => PLAYER_PLACEMENT_RANDOM_CALL_VA,
        PlacementPath::Region => REGION_PLACEMENT_RANDOM_CALL_VA,
    };
    let mut random = Random::new(state_before);
    for (index, draw) in draws.iter().enumerate() {
        let region_find_avail_call =
            path == PlacementPath::Region && draw.call_va == REGION_FIND_AVAIL_RANDOM_CALL_VA;
        let pool_call = draw.call_va == RESOURCE_POOL_GET_WATER_RANDOM_CALL_VA
            || draw.call_va == RESOURCE_POOL_GET_LATE_RANDOM_CALL_VA
            || draw.call_va == RESOURCE_POOL_GET_EARLY_RANDOM_CALL_VA;
        let allowed_call =
            draw.call_va == path_call || region_find_avail_call || (selector != 0 && pool_call);
        if !allowed_call
            || (region_find_avail_call && index != 0)
            || draw.random_get_va != RANDOM_GET_VA
            || draw.low != 0
            || draw.high != 0xffff
            || draw.state_before != random.state()
        {
            return Err(index);
        }
        let raw = random.get(draw.low, draw.high);
        if draw.raw != raw || draw.state_after != random.state() {
            return Err(index);
        }
    }
    if random.state() != expected_after {
        return Err(draws.len());
    }
    Ok(())
}

fn validate_resource_pool_receipt(
    request: &PlacementRequest,
    receipt: &PlacementReceipt,
) -> Result<(), FirstBonusMutationError> {
    let expected_lane = if request.params.selector == 2 {
        ResourcePoolLane::Early
    } else {
        ResourcePoolLane::Late
    };
    if request.params.selector == 0 {
        if !receipt.resource_pool_selections.is_empty()
            || receipt.resource_pool_after != request.resource_pool_before
            || receipt.resource_pool_digest_after != request.resource_pool_digest_before
        {
            return Err(FirstBonusMutationError::PlacementReceiptMismatch);
        }
        return Ok(());
    }

    let Some(pool_before) = request.resource_pool_before.as_ref() else {
        return Err(FirstBonusMutationError::ConcreteResourcePoolUnavailable);
    };
    let Some(pool_after) = receipt.resource_pool_after.as_ref() else {
        return Err(FirstBonusMutationError::ConcreteResourcePoolUnavailable);
    };
    if resource_divvy_pool_digest(pool_before) != request.resource_pool_digest_before
        || resource_divvy_pool_digest(pool_after) != receipt.resource_pool_digest_after
    {
        return Err(FirstBonusMutationError::PlacementReceiptMismatch);
    }

    let mut expected_pool = pool_before;
    let mut transcript_cursor = 0;
    let mut selected_goods = Vec::new();
    for (index, selection) in receipt.resource_pool_selections.iter().enumerate() {
        if selection.lane != expected_lane
            || selection.pool_before != *expected_pool
            || !validate_resource_pool_selection(selection)
        {
            return Err(FirstBonusMutationError::InvalidResourcePoolSelection { index });
        }
        if selection.random_draws.is_empty() {
            let state_is_in_placement_chain = selection.random_state_before
                == request.random_state_before
                || receipt
                    .random_draws
                    .iter()
                    .any(|draw| draw.state_after == selection.random_state_before);
            if !state_is_in_placement_chain {
                return Err(FirstBonusMutationError::InvalidResourcePoolSelection { index });
            }
        }
        if !selection.random_draws.is_empty() {
            while transcript_cursor < receipt.random_draws.len()
                && receipt.random_draws[transcript_cursor].call_va
                    != RESOURCE_POOL_GET_WATER_RANDOM_CALL_VA
                && receipt.random_draws[transcript_cursor].call_va
                    != RESOURCE_POOL_GET_LATE_RANDOM_CALL_VA
                && receipt.random_draws[transcript_cursor].call_va
                    != RESOURCE_POOL_GET_EARLY_RANDOM_CALL_VA
            {
                transcript_cursor += 1;
            }
            for draw in &selection.random_draws {
                let Some(global) = receipt.random_draws.get(transcript_cursor) else {
                    return Err(FirstBonusMutationError::InvalidResourcePoolSelection { index });
                };
                if (
                    global.call_va,
                    global.random_get_va,
                    global.low,
                    global.high,
                    global.state_before,
                    global.raw,
                    global.state_after,
                ) != (
                    draw.call_va,
                    draw.random_get_va,
                    draw.low,
                    draw.high,
                    draw.state_before,
                    draw.raw,
                    draw.state_after,
                ) {
                    return Err(FirstBonusMutationError::InvalidResourcePoolSelection { index });
                }
                transcript_cursor += 1;
            }
        }
        selected_goods.push(selection.selected_good);
        expected_pool = &selection.pool_after;
    }
    let unmatched_pool_draw = receipt
        .random_draws
        .iter()
        .skip(transcript_cursor)
        .filter(|draw| {
            draw.call_va == RESOURCE_POOL_GET_WATER_RANDOM_CALL_VA
                || draw.call_va == RESOURCE_POOL_GET_LATE_RANDOM_CALL_VA
                || draw.call_va == RESOURCE_POOL_GET_EARLY_RANDOM_CALL_VA
        })
        .next()
        .is_some();
    if unmatched_pool_draw || expected_pool != pool_after {
        return Err(FirstBonusMutationError::PlacementReceiptMismatch);
    }
    if receipt
        .allocations
        .iter()
        .any(|allocation| !selected_goods.contains(&allocation.type_id))
    {
        return Err(FirstBonusMutationError::PlacementReceiptMismatch);
    }
    Ok(())
}

fn validate_allocation(request: &PlacementRequest, allocation: &ResourceAllocation) -> bool {
    let (good_call, item_call) = match request.path {
        PlacementPath::Player => (PLAYER_INIT_GOOD_CALL_VA, PLAYER_INIT_ITEM_CALL_VA),
        PlacementPath::Region => (REGION_INIT_GOOD_CALL_VA, REGION_INIT_ITEM_CALL_VA),
    };
    let (expected_call, expected_callee, expected_kind) = if request.params.good_id == ITEM_GOOD_ID
    {
        (item_call, OBJECTS_INIT_ITEM_VA, AllocationKind::Item)
    } else {
        (good_call, OBJECTS_INIT_GOOD_VA, AllocationKind::Good)
    };
    let expected_type = if request.params.selector == 0 {
        allocation.type_id == request.params.good_id
    } else {
        request.params.good_id == -1 && allocation.type_id >= 0
    };
    if allocation.call_va != expected_call
        || allocation.callee_va != expected_callee
        || allocation.kind != expected_kind
        || !expected_type
        || allocation.slot < 0
        || allocation.slot > i32::from(i16::MAX)
    {
        return false;
    }

    if allocation.kind == AllocationKind::Good && allocation.type_id == 5 {
        return allocation.occupancy_write.is_none();
    }
    let Some(write) = &allocation.occupancy_write else {
        return false;
    };
    let expected_marker = match allocation.kind {
        AllocationKind::Good => GOOD_WDATA_DOWN_MARKER,
        AllocationKind::Item => ITEM_WDATA_DOWN_MARKER,
    };
    let (down_write_va, down_who_write_va) = match allocation.kind {
        AllocationKind::Good => (GOOD_WDATA_DOWN_WRITE_VA, GOOD_WDATA_DOWN_WHO_WRITE_VA),
        AllocationKind::Item => (ITEM_WDATA_DOWN_WRITE_VA, ITEM_WDATA_DOWN_WHO_WRITE_VA),
    };
    write.world_x == div_3(allocation.coord_x >> 8)
        && write.world_y == div_3(allocation.coord_y >> 8)
        && write.down_write_va == down_write_va
        && write.down_who_write_va == down_who_write_va
        && write.new_down == expected_marker
        && write.new_down_who == allocation.slot as i16
        && (write.old_down != write.new_down || write.old_down_who != write.new_down_who)
}

pub(crate) fn validate_placement_receipt(
    request: &PlacementRequest,
    receipt: &PlacementReceipt,
) -> Result<(), FirstBonusMutationError> {
    if receipt.request != *request
        || receipt.allocated_count < 0
        || receipt.allocated_count as usize != receipt.allocations.len()
        || receipt.sourced_walked_bytes_after != request.sourced_walked_bytes
    {
        return Err(FirstBonusMutationError::PlacementReceiptMismatch);
    }
    validate_resource_pool_receipt(request, receipt)?;
    if !receipt.evidence.admissible() {
        return Err(FirstBonusMutationError::InvalidPlacementEvidence);
    }
    if let Err(index) = validate_random_draws(
        request.path,
        request.params.selector,
        request.random_state_before,
        &receipt.random_draws,
        receipt.random_state_after,
    ) {
        return Err(FirstBonusMutationError::InvalidCalleeRandomDraw { index });
    }
    for (index, allocation) in receipt.allocations.iter().enumerate() {
        if !validate_allocation(request, allocation) {
            return Err(FirstBonusMutationError::InvalidAllocation { index });
        }
    }

    let writes = receipt
        .allocations
        .iter()
        .filter(|allocation| allocation.occupancy_write.is_some())
        .count();
    let differing = request
        .world_checksum_before
        .differing_sections(&receipt.world_checksum_after);
    if (writes == 0 && !differing.is_empty()) || (writes != 0 && differing != [WorldSection::WData])
    {
        return Err(FirstBonusMutationError::InvalidWorldChecksumDelta);
    }
    Ok(())
}

fn placement_parameters(
    entry: &PlaceResourcesBonusRowsHandoff,
    facts: &FirstBonusMutationFacts,
    good_id: i32,
    selector: i32,
) -> PlacementParameters {
    let scaled = |attribute| {
        facts
            .scaled_value(attribute)
            .expect("scaled attributes validated before parameter projection")
    };
    let group_spacing = {
        let value = scaled(ScaledAttribute::GroupSpacing);
        if facts.pattern == PlacementPattern::Player && value < 0 {
            0
        } else {
            value
        }
    };
    PlacementParameters {
        player_count: entry.player_count_argument,
        good_id,
        selector,
        pattern: facts.pattern,
        saturate: facts.saturate.max(0),
        num_rare: scaled(ScaledAttribute::NumRare),
        spacing: facts.spacing,
        group_spacing,
        player_keep_away: scaled(ScaledAttribute::PlayerKeepAway).max(1),
        player_stay_near: scaled(ScaledAttribute::PlayerStayNear).max(1),
        center_keep_away: scaled(ScaledAttribute::CenterKeepAway),
        center_stay_near: scaled(ScaledAttribute::CenterStayNear),
        corner_keep_away: scaled(ScaledAttribute::CornerKeepAway),
        corner_stay_near: scaled(ScaledAttribute::CornerStayNear),
        edge_keep_away: scaled(ScaledAttribute::EdgeKeepAway),
        edge_stay_near: scaled(ScaledAttribute::EdgeStayNear),
    }
}

/// Execute exactly one first-category row from the XML handoff.
///
/// Local state changes only after every fact and placement receipt validates. The host itself
/// must be side-effect-free or two-phase; observations made while producing a rejected receipt
/// cannot be rolled back by this adapter.
pub fn execute_first_bonus_mutation<H: PlacementHost>(
    state: &mut PlaceResourcesBonusMutationState,
    entry: &PlaceResourcesBonusRowsHandoff,
    facts: &FirstBonusMutationFacts,
    host: &mut H,
) -> Result<FirstBonusMutationReceipt, FirstBonusMutationError> {
    if entry.resume_va != PLACE_RESOURCES_XML_RESIDUAL_VA
        || entry.first_row_body_va != FIRST_BONUS_ROW_BODY_VA
        || !entry.selected_document_live
        || !entry.default_document_live
    {
        return Err(FirstBonusMutationError::WrongHandoff);
    }
    let Some(row) = entry.rows.first() else {
        return Err(FirstBonusMutationError::NoBonusRow);
    };
    if state.random_state != entry.random_state
        || state.world_checksum != entry.world_checksum
        || state.sourced_walked_bytes != entry.sourced_walked_bytes
        || state.resource_pool_digest != entry.resource_pool_digest
        || state.allocated_resources != 0
        || state.requested_resources != 0
        || state
            .resource_pool
            .as_ref()
            .is_some_and(|pool| resource_divvy_pool_digest(pool) != state.resource_pool_digest)
    {
        return Err(FirstBonusMutationError::StateMismatch);
    }
    if facts.capture_ordinal != row.capture_ordinal
        || !facts.evidence.admissible(entry, row.capture_ordinal)
    {
        return Err(FirstBonusMutationError::StaleEvidence);
    }

    let type_projection = facts.type_resolution.good_and_selector();
    if type_projection.is_none() && facts.type_resolution != ResourceTypeResolution::Unknown {
        return Err(FirstBonusMutationError::InvalidTypeResolution);
    }

    let random_state_before = state.random_state;
    let world_checksum_before = state.world_checksum.clone();
    let resource_pool_digest_before = state.resource_pool_digest;
    let mut callbacks = vec![
        callback(
            TYPE_GET_ATTRIB_CALL_VA,
            XMLELEMENT_GET_ATTRIB_VA,
            CallbackKind::GetTypeAttribute,
        ),
        callback(
            GOOD_KEY_CALL_VA,
            TYPES_GOOD_KEY_VA,
            CallbackKind::ResolveGoodKey,
        ),
    ];
    let fallback_call_vas: &[u32] = match facts.type_resolution {
        ResourceTypeResolution::CatalogGood { .. } => &[],
        ResourceTypeResolution::PoolSelector { selector: 1, .. } => &[SELECTOR_ONE_IGNORE_CALL_VA],
        ResourceTypeResolution::ItemGood => {
            &[SELECTOR_ONE_IGNORE_CALL_VA, ITEM_GOOD_IGNORE_CALL_VA]
        }
        ResourceTypeResolution::PoolSelector { selector: 2, .. } => &[
            SELECTOR_ONE_IGNORE_CALL_VA,
            ITEM_GOOD_IGNORE_CALL_VA,
            SELECTOR_TWO_IGNORE_CALL_VA,
        ],
        ResourceTypeResolution::PoolSelector { selector: 3, .. }
        | ResourceTypeResolution::Unknown => &[
            SELECTOR_ONE_IGNORE_CALL_VA,
            ITEM_GOOD_IGNORE_CALL_VA,
            SELECTOR_TWO_IGNORE_CALL_VA,
            SELECTOR_THREE_IGNORE_CALL_VA,
        ],
        ResourceTypeResolution::PoolSelector { .. } => &[],
    };
    callbacks.extend(
        fallback_call_vas
            .iter()
            .map(|call_va| callback(*call_va, STRING_IGNORE_VA, CallbackKind::MatchTypeFallback)),
    );

    let next_va = if entry.rows.len() > 1 {
        NEXT_BONUS_ROW_VA
    } else {
        BONUS_CATEGORY_TAIL_VA
    };
    let finish_without_placement =
        |state: &mut PlaceResourcesBonusMutationState,
         callbacks: Vec<HostCallback>,
         chance_draw: Option<ChanceRandomDraw>,
         budget_before: i32,
         budget_after: i32,
         winner: bool,
         disposition: FirstBonusDisposition| {
            FirstBonusMutationReceipt {
                entry_va: FIRST_BONUS_ROW_LOOP_INIT_VA,
                row_body_va: FIRST_BONUS_ROW_BODY_VA,
                residual_va: FIRST_BONUS_ROW_RESIDUAL_VA,
                next_va,
                section_source: entry.section_source,
                capture_ordinal: row.capture_ordinal,
                facts: facts.clone(),
                row_handles: row.handles,
                callbacks,
                chance_draw,
                chance_budget_before_subtract: budget_before,
                chance_budget_after_subtract: budget_after,
                chance_winner_seen: winner,
                disposition,
                placement: None,
                random_state_before,
                random_state_after: state.random_state,
                world_checksum_before: world_checksum_before.clone(),
                world_checksum_after: state.world_checksum.clone(),
                sourced_walked_bytes: state.sourced_walked_bytes,
                resource_pool_digest_before,
                resource_pool_digest_after: state.resource_pool_digest,
                allocated_resources_after: state.allocated_resources,
                requested_resources_after: state.requested_resources,
            }
        };

    let Some((good_id, selector)) = type_projection else {
        return Ok(finish_without_placement(
            state,
            callbacks,
            None,
            0,
            0,
            false,
            FirstBonusDisposition::UnknownType,
        ));
    };

    callbacks.push(callback(
        CHANCE_GET_ATTRIB_NUM_CALL_VA,
        XMLELEMENT_GET_ATTRIB_NUM_VA,
        CallbackKind::GetChance,
    ));
    callbacks.push(callback(
        CHANCE_GROUP_GET_ATTRIB_NUM_CALL_VA,
        XMLELEMENT_GET_ATTRIB_NUM_VA,
        CallbackKind::GetChanceGroup,
    ));

    // `local_94 == -1`, `local_30 == 0`, `local_2c == 0` at this exact first-row seam.
    let mut random = Random::new(state.random_state);
    let mut budget = 0;
    let chance_draw = if facts.chance_group != -1 {
        let before = random.state();
        let raw = random.get(0, 0xffff);
        let modulo_100 = raw % 100;
        budget = modulo_100;
        Some(ChanceRandomDraw {
            call_va: CHANCE_RANDOM_GET_CALL_VA,
            random_get_va: RANDOM_GET_VA,
            low: 0,
            high: 0xffff,
            state_before: before,
            raw,
            modulo_100,
            state_after: random.state(),
        })
    } else {
        None
    };
    let budget_before = budget;
    budget = budget.wrapping_sub(facts.chance);
    state.random_state = random.state();
    if budget >= 0 {
        return Ok(finish_without_placement(
            state,
            callbacks,
            chance_draw,
            budget_before,
            budget,
            false,
            FirstBonusDisposition::ChanceMiss,
        ));
    }

    if !facts.validate_num_rare() {
        // Restore the direct draw too: validation is intentionally ahead of commit.
        state.random_state = random_state_before;
        return Err(FirstBonusMutationError::InvalidScaledAttributes);
    }
    callbacks.push(callback(
        NUM_RARE_SCALE_CALL_VA,
        MAP_SCALE_NUMBER_VA,
        CallbackKind::Scale(ScaledAttribute::NumRare),
    ));
    if facts
        .scaled_value(ScaledAttribute::NumRare)
        .expect("scaled attributes validated above")
        == 0
    {
        return Ok(finish_without_placement(
            state,
            callbacks,
            chance_draw,
            budget_before,
            budget,
            true,
            FirstBonusDisposition::ZeroRequested,
        ));
    }
    if !facts.validate_scaled() {
        // The nine remaining attributes are not read on the zero-`numrare` path.
        state.random_state = random_state_before;
        return Err(FirstBonusMutationError::InvalidScaledAttributes);
    }
    callbacks.push(callback(
        PATTERN_GET_ATTRIB_CALL_VA,
        XMLELEMENT_GET_ATTRIB_VA,
        CallbackKind::GetPatternAttribute,
    ));
    callbacks.push(callback(
        SATURATE_GET_ATTRIB_NUM_CALL_VA,
        XMLELEMENT_GET_ATTRIB_NUM_VA,
        CallbackKind::GetSaturate,
    ));
    callbacks.push(callback(
        SPACING_GET_ATTRIB_NUM_CALL_VA,
        XMLELEMENT_GET_ATTRIB_NUM_VA,
        CallbackKind::GetSpacing,
    ));
    callbacks.extend(facts.scaled.iter().skip(1).map(|fact| {
        callback(
            fact.call_va,
            MAP_SCALE_NUMBER_VA,
            CallbackKind::Scale(fact.attribute),
        )
    }));

    let params = placement_parameters(entry, facts, good_id, selector);
    let path = facts.pattern.path();
    let (call_va, callee_va, callback_kind) = match path {
        PlacementPath::Player => (
            PLAYER_PLACEMENT_CALL_VA,
            MAP_PLACE_PLAYER_RESOURCE_VA,
            CallbackKind::PlacePlayerResource,
        ),
        PlacementPath::Region => (
            REGION_PLACEMENT_CALL_VA,
            MAP_PLACE_REGION_RESOURCE_VA,
            CallbackKind::PlaceRegionResource,
        ),
    };
    callbacks.push(callback(call_va, callee_va, callback_kind));
    let request = PlacementRequest {
        call_va,
        callee_va,
        path,
        params,
        random_state_before: random.state(),
        world_checksum_before: state.world_checksum.clone(),
        sourced_walked_bytes: state.sourced_walked_bytes,
        resource_pool_digest_before: state.resource_pool_digest,
        resource_pool_before: state.resource_pool.clone(),
    };
    let Some(placement) = host.place(&request) else {
        state.random_state = random_state_before;
        return Err(FirstBonusMutationError::PlacementReceiptUnavailable { request });
    };
    if let Err(error) = validate_placement_receipt(&request, &placement) {
        state.random_state = random_state_before;
        return Err(error);
    }

    state.random_state = placement.random_state_after;
    state.world_checksum = placement.world_checksum_after.clone();
    state.resource_pool_digest = placement.resource_pool_digest_after;
    state.resource_pool = placement.resource_pool_after.clone();
    state.allocated_resources = state
        .allocated_resources
        .wrapping_add(placement.allocated_count);
    let requested = if path == PlacementPath::Player {
        request
            .params
            .num_rare
            .wrapping_mul(entry.player_count_argument)
    } else {
        request.params.num_rare
    };
    state.requested_resources = state.requested_resources.wrapping_add(requested);

    Ok(FirstBonusMutationReceipt {
        entry_va: FIRST_BONUS_ROW_LOOP_INIT_VA,
        row_body_va: FIRST_BONUS_ROW_BODY_VA,
        residual_va: FIRST_BONUS_ROW_RESIDUAL_VA,
        next_va,
        section_source: entry.section_source,
        capture_ordinal: row.capture_ordinal,
        facts: facts.clone(),
        row_handles: row.handles,
        callbacks,
        chance_draw,
        chance_budget_before_subtract: budget_before,
        chance_budget_after_subtract: budget,
        chance_winner_seen: true,
        disposition: FirstBonusDisposition::Placed(path),
        placement: Some(placement),
        random_state_before,
        random_state_after: state.random_state,
        world_checksum_before,
        world_checksum_after: state.world_checksum.clone(),
        sourced_walked_bytes: state.sourced_walked_bytes,
        resource_pool_digest_before,
        resource_pool_digest_after: state.resource_pool_digest,
        allocated_resources_after: state.allocated_resources,
        requested_resources_after: state.requested_resources,
    })
}
