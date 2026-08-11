// SPDX-License-Identifier: GPL-3.0-or-later
//! Compiled owner for recurrence rows and carried first rows in resource categories.
//!
//! The first-row owner stops before `0x00690215`. This module owns recurrence
//! routing there and the mutation cone `0x0068fc0d..0x00690215`, one atomic
//! projection at a time, until the category-tail seam at `0x00690225`; the
//! canonical map-make schedule replays this owner before each continuation. It also
//! exposes a separately typed `0x0068fbb3` entry for row zero after BONUSES→FISH or
//! FISH→GOODIES; that entry carries the existing chance locals and deliberately
//! claims no recurrence prefix. The intervening native XML reference assignment
//! remains an explicit captured host boundary. This module does not own category
//! cleanup, the function return, or caller token `0x1ef7`.

use crate::place_resources_bonus_mutation_frontier::{
    validate_placement_receipt, CallbackKind, ChanceRandomDraw, FirstBonusMutationFacts,
    FirstBonusMutationReceipt, HostCallback, PlacementHost, PlacementParameters, PlacementPath,
    PlacementReceipt, ResourceTypeResolution, ScaledAttribute, ScaledAttributeFact,
    CHANCE_GET_ATTRIB_NUM_CALL_VA, CHANCE_GROUP_GET_ATTRIB_NUM_CALL_VA, CHANCE_RANDOM_GET_CALL_VA,
    FIRST_BONUS_ROW_BODY_VA, FIRST_BONUS_ROW_RESIDUAL_VA, GOOD_KEY_CALL_VA,
    ITEM_GOOD_IGNORE_CALL_VA, MAP_PLACE_PLAYER_RESOURCE_VA, MAP_PLACE_REGION_RESOURCE_VA,
    MAP_SCALE_NUMBER_VA, NEXT_BONUS_ROW_VA, NUM_RARE_SCALE_CALL_VA, PATTERN_GET_ATTRIB_CALL_VA,
    PLAYER_PLACEMENT_CALL_VA, RANDOM_GET_VA, REGION_PLACEMENT_CALL_VA,
    ROW_NEW_HEAD_ACQUIRE_CALL_VA, ROW_NEW_TAIL_ACQUIRE_CALL_VA, ROW_OLD_HEAD_RELEASE_CALL_VA,
    ROW_OLD_TAIL_RELEASE_CALL_VA, SATURATE_GET_ATTRIB_NUM_CALL_VA, SELECTOR_ONE_IGNORE_CALL_VA,
    SELECTOR_THREE_IGNORE_CALL_VA, SELECTOR_TWO_IGNORE_CALL_VA, SHIPPED_EXE_SHA256,
    SHIPPED_PDB_SHA256, SPACING_GET_ATTRIB_NUM_CALL_VA, STRING_IGNORE_VA, TYPES_GOOD_KEY_VA,
    TYPE_GET_ATTRIB_CALL_VA, XMLELEMENT_GET_ATTRIB_NUM_VA, XMLELEMENT_GET_ATTRIB_VA,
};
use crate::place_resources_bonus_mutation_frontier::{
    PlaceResourcesBonusMutationState, PlacementPattern,
};
use crate::place_resources_category_frontier::{
    ResourceCategory, ROW_COUNT_TEST_VA, ROW_ENUMERATE_CALL_VA,
};
use crate::place_resources_xml_frontier::{
    BonusXmlRowFact, BonusesSectionSource, PlaceResourcesBonusRowsHandoff, XmlHostHandles,
    PLACE_RESOURCES_XML_RESIDUAL_VA,
};
use don_sim::rng::Random;
use don_sim::systems::map_terrain::WorldChecksum;

pub const LATER_BONUS_ENTRY_VA: u32 = 0x0069_0215;
pub const ROW_POINTER_STRIDE: u32 = 0x28;
pub const ROW_COUNT_DECREMENT_VA: u32 = 0x0069_0218;
pub const ROW_RECURRENCE_BRANCH_VA: u32 = 0x0069_021c;
pub const BONUS_CATEGORY_TAIL_VA: u32 = 0x0069_0225;
pub const LATER_ROW_HOST_REF_ENTRY_VA: u32 = 0x0068_fbb3;
pub const LATER_ROW_MUTATION_ENTRY_VA: u32 = 0x0068_fc0d;
/// First row reached after the carried category dispatch (FISH or GOODIES).
pub const CARRIED_CATEGORY_FIRST_ENTRY_VA: u32 = 0x0068_fbb3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaterBonusMutationEvidence {
    RetailCapture {
        executable_sha256: String,
        pdb_sha256: String,
        capture_sha256: [u8; 32],
        entry_va: u32,
        row_body_va: u32,
        mutation_entry_va: u32,
        row_index: usize,
        capture_ordinal: u32,
        random_state: i32,
        world_checksum: WorldChecksum,
        sourced_walked_bytes: u64,
        resource_pool_digest: u64,
        last_chance_group: i32,
        signed_chance_budget: i32,
        winner_seen: bool,
        facts_digest: u64,
    },
    SyntheticFixture {
        fixture: String,
        entry_va: u32,
        row_body_va: u32,
        mutation_entry_va: u32,
        row_index: usize,
        capture_ordinal: u32,
        random_state: i32,
        world_checksum: WorldChecksum,
        sourced_walked_bytes: u64,
        resource_pool_digest: u64,
        last_chance_group: i32,
        signed_chance_budget: i32,
        winner_seen: bool,
        facts_digest: u64,
    },
    /// Same row mutation cone, entered directly from category enumeration rather
    /// than through the `0x00690215` recurrence tail.
    CarriedRetailCapture {
        executable_sha256: String,
        pdb_sha256: String,
        capture_sha256: [u8; 32],
        category: ResourceCategory,
        entry_va: u32,
        row_enumerate_call_va: u32,
        row_count_test_va: u32,
        mutation_entry_va: u32,
        row_index: usize,
        capture_ordinal: u32,
        random_state: i32,
        world_checksum: WorldChecksum,
        sourced_walked_bytes: u64,
        resource_pool_digest: u64,
        last_chance_group: i32,
        signed_chance_budget: i32,
        winner_seen: bool,
        facts_digest: u64,
    },
    CarriedSyntheticFixture {
        fixture: String,
        category: ResourceCategory,
        entry_va: u32,
        row_enumerate_call_va: u32,
        row_count_test_va: u32,
        mutation_entry_va: u32,
        row_index: usize,
        capture_ordinal: u32,
        random_state: i32,
        world_checksum: WorldChecksum,
        sourced_walked_bytes: u64,
        resource_pool_digest: u64,
        last_chance_group: i32,
        signed_chance_budget: i32,
        winner_seen: bool,
        facts_digest: u64,
    },
}

impl LaterBonusMutationEvidence {
    fn admissible(&self, state: &RemainingBonusRowsState, facts: &LaterBonusMutationFacts) -> bool {
        let fields_match = |entry_va: u32,
                            row_body_va: u32,
                            mutation_entry_va: u32,
                            row_index: usize,
                            ordinal: u32,
                            random_state: i32,
                            world_checksum: &WorldChecksum,
                            sourced_walked_bytes: u64,
                            resource_pool_digest: u64,
                            last_chance_group: i32,
                            signed_chance_budget: i32,
                            winner_seen: bool,
                            facts_digest: u64| {
            entry_va == LATER_BONUS_ENTRY_VA
                && row_body_va == FIRST_BONUS_ROW_BODY_VA
                && mutation_entry_va == LATER_ROW_MUTATION_ENTRY_VA
                && row_index == state.next_row_index
                && ordinal == facts.capture_ordinal
                && random_state == state.mutation.random_state
                && world_checksum == &state.mutation.world_checksum
                && sourced_walked_bytes == state.mutation.sourced_walked_bytes
                && resource_pool_digest == state.mutation.resource_pool_digest
                && last_chance_group == state.last_chance_group
                && signed_chance_budget == state.signed_chance_budget
                && winner_seen == state.winner_seen
                && facts_digest == later_bonus_facts_digest(facts)
        };
        match self {
            Self::RetailCapture {
                executable_sha256,
                pdb_sha256,
                capture_sha256,
                entry_va,
                row_body_va,
                mutation_entry_va,
                row_index,
                capture_ordinal: ordinal,
                random_state,
                world_checksum,
                sourced_walked_bytes,
                resource_pool_digest,
                last_chance_group,
                signed_chance_budget,
                winner_seen,
                facts_digest,
            } => {
                executable_sha256 == SHIPPED_EXE_SHA256
                    && pdb_sha256 == SHIPPED_PDB_SHA256
                    && capture_sha256.iter().any(|byte| *byte != 0)
                    && fields_match(
                        *entry_va,
                        *row_body_va,
                        *mutation_entry_va,
                        *row_index,
                        *ordinal,
                        *random_state,
                        world_checksum,
                        *sourced_walked_bytes,
                        *resource_pool_digest,
                        *last_chance_group,
                        *signed_chance_budget,
                        *winner_seen,
                        *facts_digest,
                    )
            }
            Self::SyntheticFixture {
                fixture,
                entry_va,
                row_body_va,
                mutation_entry_va,
                row_index,
                capture_ordinal: ordinal,
                random_state,
                world_checksum,
                sourced_walked_bytes,
                resource_pool_digest,
                last_chance_group,
                signed_chance_budget,
                winner_seen,
                facts_digest,
            } => {
                cfg!(test)
                    && !fixture.is_empty()
                    && fields_match(
                        *entry_va,
                        *row_body_va,
                        *mutation_entry_va,
                        *row_index,
                        *ordinal,
                        *random_state,
                        world_checksum,
                        *sourced_walked_bytes,
                        *resource_pool_digest,
                        *last_chance_group,
                        *signed_chance_budget,
                        *winner_seen,
                        *facts_digest,
                    )
            }
            Self::CarriedRetailCapture { .. } | Self::CarriedSyntheticFixture { .. } => false,
        }
    }

    fn admissible_carried(
        &self,
        category: ResourceCategory,
        state: &RemainingBonusRowsState,
        facts: &LaterBonusMutationFacts,
    ) -> bool {
        let fields_match = |actual_category: ResourceCategory,
                            entry_va: u32,
                            row_enumerate_call_va: u32,
                            row_count_test_va: u32,
                            mutation_entry_va: u32,
                            row_index: usize,
                            ordinal: u32,
                            random_state: i32,
                            world_checksum: &WorldChecksum,
                            sourced_walked_bytes: u64,
                            resource_pool_digest: u64,
                            last_chance_group: i32,
                            signed_chance_budget: i32,
                            winner_seen: bool,
                            facts_digest: u64| {
            category != ResourceCategory::Bonuses
                && actual_category == category
                && entry_va == CARRIED_CATEGORY_FIRST_ENTRY_VA
                && row_enumerate_call_va == ROW_ENUMERATE_CALL_VA
                && row_count_test_va == ROW_COUNT_TEST_VA
                && mutation_entry_va == LATER_ROW_MUTATION_ENTRY_VA
                && row_index == 0
                && state.next_row_index == 0
                && ordinal == facts.capture_ordinal
                && random_state == state.mutation.random_state
                && world_checksum == &state.mutation.world_checksum
                && sourced_walked_bytes == state.mutation.sourced_walked_bytes
                && resource_pool_digest == state.mutation.resource_pool_digest
                && last_chance_group == state.last_chance_group
                && signed_chance_budget == state.signed_chance_budget
                && winner_seen == state.winner_seen
                && facts_digest == later_bonus_facts_digest(facts)
        };
        match self {
            Self::CarriedRetailCapture {
                executable_sha256,
                pdb_sha256,
                capture_sha256,
                category,
                entry_va,
                row_enumerate_call_va,
                row_count_test_va,
                mutation_entry_va,
                row_index,
                capture_ordinal,
                random_state,
                world_checksum,
                sourced_walked_bytes,
                resource_pool_digest,
                last_chance_group,
                signed_chance_budget,
                winner_seen,
                facts_digest,
            } => {
                executable_sha256 == SHIPPED_EXE_SHA256
                    && pdb_sha256 == SHIPPED_PDB_SHA256
                    && capture_sha256.iter().any(|byte| *byte != 0)
                    && fields_match(
                        *category,
                        *entry_va,
                        *row_enumerate_call_va,
                        *row_count_test_va,
                        *mutation_entry_va,
                        *row_index,
                        *capture_ordinal,
                        *random_state,
                        world_checksum,
                        *sourced_walked_bytes,
                        *resource_pool_digest,
                        *last_chance_group,
                        *signed_chance_budget,
                        *winner_seen,
                        *facts_digest,
                    )
            }
            Self::CarriedSyntheticFixture {
                fixture,
                category,
                entry_va,
                row_enumerate_call_va,
                row_count_test_va,
                mutation_entry_va,
                row_index,
                capture_ordinal,
                random_state,
                world_checksum,
                sourced_walked_bytes,
                resource_pool_digest,
                last_chance_group,
                signed_chance_budget,
                winner_seen,
                facts_digest,
            } => {
                cfg!(test)
                    && !fixture.is_empty()
                    && fields_match(
                        *category,
                        *entry_va,
                        *row_enumerate_call_va,
                        *row_count_test_va,
                        *mutation_entry_va,
                        *row_index,
                        *capture_ordinal,
                        *random_state,
                        world_checksum,
                        *sourced_walked_bytes,
                        *resource_pool_digest,
                        *last_chance_group,
                        *signed_chance_budget,
                        *winner_seen,
                        *facts_digest,
                    )
            }
            Self::RetailCapture { .. } | Self::SyntheticFixture { .. } => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaterBonusMutationFacts {
    pub capture_ordinal: u32,
    pub type_name: String,
    pub type_resolution: ResourceTypeResolution,
    pub chance: i32,
    pub chance_group: i32,
    pub pattern_name: String,
    pub pattern: PlacementPattern,
    pub saturate: i32,
    pub spacing: i32,
    pub scaled: Vec<ScaledAttributeFact>,
    pub evidence: LaterBonusMutationEvidence,
}

/// Stable logical digest of every behavior-driving later-row fact.
pub fn later_bonus_facts_digest(facts: &LaterBonusMutationFacts) -> u64 {
    fn bytes(hash: &mut u64, value: &[u8]) {
        for byte in value {
            *hash ^= u64::from(*byte);
            *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    fn i32_value(hash: &mut u64, value: i32) {
        bytes(hash, &value.to_le_bytes());
    }
    fn u32_value(hash: &mut u64, value: u32) {
        bytes(hash, &value.to_le_bytes());
    }

    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    u32_value(&mut hash, facts.capture_ordinal);
    bytes(&mut hash, facts.type_name.as_bytes());
    match facts.type_resolution {
        ResourceTypeResolution::CatalogGood { good_id } => {
            bytes(&mut hash, &[0]);
            i32_value(&mut hash, good_id);
        }
        ResourceTypeResolution::PoolSelector {
            selector,
            matched_call_va,
        } => {
            bytes(&mut hash, &[1, selector]);
            u32_value(&mut hash, matched_call_va);
        }
        ResourceTypeResolution::ItemGood => bytes(&mut hash, &[2]),
        ResourceTypeResolution::Unknown => bytes(&mut hash, &[3]),
    }
    i32_value(&mut hash, facts.chance);
    i32_value(&mut hash, facts.chance_group);
    bytes(&mut hash, facts.pattern_name.as_bytes());
    i32_value(&mut hash, facts.pattern as i32);
    i32_value(&mut hash, facts.saturate);
    i32_value(&mut hash, facts.spacing);
    u32_value(&mut hash, facts.scaled.len() as u32);
    for fact in &facts.scaled {
        i32_value(&mut hash, fact.attribute as i32);
        u32_value(&mut hash, fact.call_va);
        bytes(&mut hash, fact.expression.as_bytes());
        i32_value(&mut hash, fact.scaled);
    }
    hash
}

impl LaterBonusMutationFacts {
    fn scaled_value(&self, attribute: ScaledAttribute) -> Option<i32> {
        self.scaled
            .iter()
            .find(|fact| fact.attribute == attribute)
            .map(|fact| fact.scaled)
    }

    fn validate_scaled(&self) -> bool {
        const ORDERED: [(ScaledAttribute, u32); 10] = [
            (ScaledAttribute::NumRare, 0x0068_fdcf),
            (ScaledAttribute::GroupSpacing, 0x0068_ff67),
            (ScaledAttribute::PlayerKeepAway, 0x0068_ff9a),
            (ScaledAttribute::PlayerStayNear, 0x0068_ffc7),
            (ScaledAttribute::CenterKeepAway, 0x0068_fff4),
            (ScaledAttribute::CenterStayNear, 0x0069_0017),
            (ScaledAttribute::CornerKeepAway, 0x0069_003a),
            (ScaledAttribute::CornerStayNear, 0x0069_005d),
            (ScaledAttribute::EdgeKeepAway, 0x0069_0080),
            (ScaledAttribute::EdgeStayNear, 0x0069_00a3),
        ];
        self.scaled.len() == ORDERED.len()
            && self
                .scaled
                .iter()
                .zip(ORDERED)
                .all(|(fact, (attribute, call_va))| {
                    fact.attribute == attribute && fact.call_va == call_va
                })
    }

    fn validate_num_rare(&self) -> bool {
        self.scaled.first().is_some_and(|fact| {
            fact.attribute == ScaledAttribute::NumRare && fact.call_va == NUM_RARE_SCALE_CALL_VA
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemainingBonusRowsState {
    pub next_row_index: usize,
    pub section_source: BonusesSectionSource,
    /// Logical row projection accepted by the preceding XML receipt. This detects
    /// substitution after construction; it does not authenticate the capture bytes.
    pub rows: Vec<BonusXmlRowFact>,
    pub last_chance_group: i32,
    pub signed_chance_budget: i32,
    pub winner_seen: bool,
    pub mutation: PlaceResourcesBonusMutationState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaterBonusDisposition {
    UnknownType,
    NegativeCarryBlocked,
    ChanceMiss,
    ZeroRequested,
    Placed(PlacementPath),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaterRowHostRefBoundary {
    pub entry_va: u32,
    pub residual_va: u32,
    pub release_call_vas: [u32; 2],
    pub acquire_call_vas: [u32; 2],
    pub previous_row_handles: XmlHostHandles,
    pub current_row_handles: XmlHostHandles,
    /// Native pointer identity and refcounts remain owned by the capture host.
    pub pointer_identity_external: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaterBonusMutationReceipt {
    pub entry_va: u32,
    pub row_pointer_stride: u32,
    pub row_count_decrement_va: u32,
    pub recurrence_branch_va: u32,
    pub row_body_va: u32,
    pub host_ref_boundary: LaterRowHostRefBoundary,
    pub mutation_entry_va: u32,
    pub residual_va: u32,
    pub next_va: u32,
    pub section_source: BonusesSectionSource,
    pub row_count: usize,
    pub row_index: usize,
    pub capture_ordinal: u32,
    pub row_handles: XmlHostHandles,
    pub callbacks: Vec<HostCallback>,
    pub chance_draw: Option<ChanceRandomDraw>,
    pub last_chance_group_before: i32,
    pub last_chance_group_after: i32,
    pub chance_budget_before: i32,
    pub chance_budget_after: i32,
    pub budget_subtracted: bool,
    pub winner_seen_before: bool,
    pub winner_seen_after: bool,
    pub disposition: LaterBonusDisposition,
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

/// Separately typed first-row receipt after BONUSES→FISH or FISH→GOODIES. The
/// chance locals are carried into this entry; they are not reinitialized as at the
/// first BONUSES row and no `0x00690215` recurrence prefix is claimed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CarriedCategoryFirstMutationReceipt {
    pub category: ResourceCategory,
    pub entry_va: u32,
    pub row_enumerate_call_va: u32,
    pub row_count_test_va: u32,
    pub row_body_va: u32,
    pub previous_row_handles: XmlHostHandles,
    pub current_row_handles: XmlHostHandles,
    pub mutation_entry_va: u32,
    pub residual_va: u32,
    pub next_va: u32,
    pub section_source: BonusesSectionSource,
    pub row_count: usize,
    pub capture_ordinal: u32,
    pub callbacks: Vec<HostCallback>,
    pub chance_draw: Option<ChanceRandomDraw>,
    pub last_chance_group_before: i32,
    pub last_chance_group_after: i32,
    pub chance_budget_before: i32,
    pub chance_budget_after: i32,
    pub budget_subtracted: bool,
    pub winner_seen_before: bool,
    pub winner_seen_after: bool,
    pub disposition: LaterBonusDisposition,
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
pub enum RemainingBonusRowsError {
    WrongFirstRowReceipt,
    WrongHandoff,
    StaleEvidence,
    InvalidTypeResolution,
    InvalidScaledAttributes,
    PlacementReceiptUnavailable,
    PlacementReceiptMismatch,
    ConcreteResourcePoolUnavailable,
    InvalidResourcePoolSelection { index: usize },
    InvalidPlacementEvidence,
    InvalidCalleeRandomDraw { index: usize },
    InvalidAllocation { index: usize },
    InvalidWorldChecksumDelta,
}

impl From<crate::place_resources_bonus_mutation_frontier::FirstBonusMutationError>
    for RemainingBonusRowsError
{
    fn from(
        error: crate::place_resources_bonus_mutation_frontier::FirstBonusMutationError,
    ) -> Self {
        use crate::place_resources_bonus_mutation_frontier::FirstBonusMutationError as First;
        match error {
            First::PlacementReceiptMismatch => Self::PlacementReceiptMismatch,
            First::ConcreteResourcePoolUnavailable => Self::ConcreteResourcePoolUnavailable,
            First::InvalidResourcePoolSelection { index } => {
                Self::InvalidResourcePoolSelection { index }
            }
            First::InvalidPlacementEvidence => Self::InvalidPlacementEvidence,
            First::InvalidCalleeRandomDraw { index } => Self::InvalidCalleeRandomDraw { index },
            First::InvalidAllocation { index } => Self::InvalidAllocation { index },
            First::InvalidWorldChecksumDelta => Self::InvalidWorldChecksumDelta,
            _ => Self::WrongHandoff,
        }
    }
}

impl RemainingBonusRowsState {
    /// Recover the native chance carry at the first-row recurrence seam.
    pub fn from_first_row(
        entry: &PlaceResourcesBonusRowsHandoff,
        facts: &FirstBonusMutationFacts,
        receipt: &FirstBonusMutationReceipt,
        mutation: &PlaceResourcesBonusMutationState,
    ) -> Result<Self, RemainingBonusRowsError> {
        let chance_was_read = receipt
            .callbacks
            .iter()
            .any(|callback| callback.kind == CallbackKind::GetChanceGroup);
        let expected_next_va = if entry.rows.len() > 1 {
            NEXT_BONUS_ROW_VA
        } else {
            BONUS_CATEGORY_TAIL_VA
        };
        if entry.rows.is_empty()
            || receipt.entry_va != 0x0068_fb9d
            || receipt.row_body_va != FIRST_BONUS_ROW_BODY_VA
            || receipt.residual_va != FIRST_BONUS_ROW_RESIDUAL_VA
            || receipt.next_va != expected_next_va
            || receipt.capture_ordinal != entry.rows[0].capture_ordinal
            || facts.capture_ordinal != entry.rows[0].capture_ordinal
            || receipt.facts != *facts
            || !facts
                .evidence
                .admissible(entry, entry.rows[0].capture_ordinal)
            || receipt.random_state_after != mutation.random_state
            || receipt.world_checksum_after != mutation.world_checksum
            || receipt.sourced_walked_bytes != mutation.sourced_walked_bytes
            || receipt.resource_pool_digest_after != mutation.resource_pool_digest
            || receipt.allocated_resources_after != mutation.allocated_resources
            || receipt.requested_resources_after != mutation.requested_resources
        {
            return Err(RemainingBonusRowsError::WrongFirstRowReceipt);
        }
        Ok(Self {
            next_row_index: 1,
            section_source: entry.section_source,
            rows: entry.rows.clone(),
            last_chance_group: if chance_was_read {
                facts.chance_group
            } else {
                -1
            },
            signed_chance_budget: receipt.chance_budget_after_subtract,
            winner_seen: receipt.chance_winner_seen,
            mutation: mutation.clone(),
        })
    }
}

fn callback(call_va: u32, callee_va: u32, kind: CallbackKind) -> HostCallback {
    HostCallback {
        call_va,
        callee_va,
        kind,
    }
}

fn placement_parameters(
    entry: &PlaceResourcesBonusRowsHandoff,
    facts: &LaterBonusMutationFacts,
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

fn finish_receipt(
    before: &RemainingBonusRowsState,
    after: &RemainingBonusRowsState,
    entry: &PlaceResourcesBonusRowsHandoff,
    callbacks: Vec<HostCallback>,
    chance_draw: Option<ChanceRandomDraw>,
    budget_subtracted: bool,
    disposition: LaterBonusDisposition,
    placement: Option<PlacementReceipt>,
) -> LaterBonusMutationReceipt {
    let row_index = before.next_row_index;
    LaterBonusMutationReceipt {
        entry_va: LATER_BONUS_ENTRY_VA,
        row_pointer_stride: ROW_POINTER_STRIDE,
        row_count_decrement_va: ROW_COUNT_DECREMENT_VA,
        recurrence_branch_va: ROW_RECURRENCE_BRANCH_VA,
        row_body_va: FIRST_BONUS_ROW_BODY_VA,
        host_ref_boundary: LaterRowHostRefBoundary {
            entry_va: LATER_ROW_HOST_REF_ENTRY_VA,
            residual_va: LATER_ROW_MUTATION_ENTRY_VA,
            release_call_vas: [ROW_OLD_TAIL_RELEASE_CALL_VA, ROW_OLD_HEAD_RELEASE_CALL_VA],
            acquire_call_vas: [ROW_NEW_HEAD_ACQUIRE_CALL_VA, ROW_NEW_TAIL_ACQUIRE_CALL_VA],
            previous_row_handles: before.rows[row_index - 1].handles,
            current_row_handles: before.rows[row_index].handles,
            pointer_identity_external: true,
        },
        mutation_entry_va: LATER_ROW_MUTATION_ENTRY_VA,
        residual_va: FIRST_BONUS_ROW_RESIDUAL_VA,
        next_va: if after.next_row_index < entry.rows.len() {
            FIRST_BONUS_ROW_BODY_VA
        } else {
            BONUS_CATEGORY_TAIL_VA
        },
        section_source: before.section_source,
        row_count: before.rows.len(),
        row_index,
        capture_ordinal: entry.rows[row_index].capture_ordinal,
        row_handles: entry.rows[row_index].handles,
        callbacks,
        chance_draw,
        last_chance_group_before: before.last_chance_group,
        last_chance_group_after: after.last_chance_group,
        chance_budget_before: before.signed_chance_budget,
        chance_budget_after: after.signed_chance_budget,
        budget_subtracted,
        winner_seen_before: before.winner_seen,
        winner_seen_after: after.winner_seen,
        disposition,
        placement,
        random_state_before: before.mutation.random_state,
        random_state_after: after.mutation.random_state,
        world_checksum_before: before.mutation.world_checksum.clone(),
        world_checksum_after: after.mutation.world_checksum.clone(),
        sourced_walked_bytes: after.mutation.sourced_walked_bytes,
        resource_pool_digest_before: before.mutation.resource_pool_digest,
        resource_pool_digest_after: after.mutation.resource_pool_digest,
        allocated_resources_after: after.mutation.allocated_resources,
        requested_resources_after: after.mutation.requested_resources,
    }
}

fn finish_carried_first_receipt(
    category: ResourceCategory,
    before: &RemainingBonusRowsState,
    after: &RemainingBonusRowsState,
    entry: &PlaceResourcesBonusRowsHandoff,
    callbacks: Vec<HostCallback>,
    chance_draw: Option<ChanceRandomDraw>,
    budget_subtracted: bool,
    disposition: LaterBonusDisposition,
    placement: Option<PlacementReceipt>,
) -> CarriedCategoryFirstMutationReceipt {
    CarriedCategoryFirstMutationReceipt {
        category,
        entry_va: CARRIED_CATEGORY_FIRST_ENTRY_VA,
        row_enumerate_call_va: ROW_ENUMERATE_CALL_VA,
        row_count_test_va: ROW_COUNT_TEST_VA,
        row_body_va: FIRST_BONUS_ROW_BODY_VA,
        previous_row_handles: XmlHostHandles::default(),
        current_row_handles: before.rows[0].handles,
        mutation_entry_va: LATER_ROW_MUTATION_ENTRY_VA,
        residual_va: FIRST_BONUS_ROW_RESIDUAL_VA,
        next_va: if after.next_row_index < entry.rows.len() {
            FIRST_BONUS_ROW_BODY_VA
        } else {
            BONUS_CATEGORY_TAIL_VA
        },
        section_source: before.section_source,
        row_count: before.rows.len(),
        capture_ordinal: entry.rows[0].capture_ordinal,
        callbacks,
        chance_draw,
        last_chance_group_before: before.last_chance_group,
        last_chance_group_after: after.last_chance_group,
        chance_budget_before: before.signed_chance_budget,
        chance_budget_after: after.signed_chance_budget,
        budget_subtracted,
        winner_seen_before: before.winner_seen,
        winner_seen_after: after.winner_seen,
        disposition,
        placement,
        random_state_before: before.mutation.random_state,
        random_state_after: after.mutation.random_state,
        world_checksum_before: before.mutation.world_checksum.clone(),
        world_checksum_after: after.mutation.world_checksum.clone(),
        sourced_walked_bytes: after.mutation.sourced_walked_bytes,
        resource_pool_digest_before: before.mutation.resource_pool_digest,
        resource_pool_digest_after: after.mutation.resource_pool_digest,
        allocated_resources_after: after.mutation.allocated_resources,
        requested_resources_after: after.mutation.requested_resources,
    }
}

/// Execute one later `BONUS` mutation projection atomically after the captured XML
/// host-reference boundary.
pub fn execute_next_bonus_mutation<H: PlacementHost>(
    state: &mut RemainingBonusRowsState,
    entry: &PlaceResourcesBonusRowsHandoff,
    facts: &LaterBonusMutationFacts,
    host: &mut H,
) -> Result<LaterBonusMutationReceipt, RemainingBonusRowsError> {
    if entry.resume_va != PLACE_RESOURCES_XML_RESIDUAL_VA
        || entry.first_row_body_va != FIRST_BONUS_ROW_BODY_VA
        || !entry.selected_document_live
        || !entry.default_document_live
        || state.next_row_index == 0
        || state.next_row_index >= entry.rows.len()
        || state.section_source != entry.section_source
        || state.rows != entry.rows
        || state.mutation.sourced_walked_bytes != entry.sourced_walked_bytes
        || state.mutation.resource_pool.as_ref().is_some_and(|pool| {
            crate::place_resources_pool_frontier::resource_divvy_pool_digest(pool)
                != state.mutation.resource_pool_digest
        })
    {
        return Err(RemainingBonusRowsError::WrongHandoff);
    }
    let row = &entry.rows[state.next_row_index];
    if facts.capture_ordinal != row.capture_ordinal || !facts.evidence.admissible(state, facts) {
        return Err(RemainingBonusRowsError::StaleEvidence);
    }
    let type_projection = facts.type_resolution.good_and_selector();
    if type_projection.is_none() && facts.type_resolution != ResourceTypeResolution::Unknown {
        return Err(RemainingBonusRowsError::InvalidTypeResolution);
    }

    let before = state.clone();
    let mut staged = state.clone();
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

    let Some((good_id, selector)) = type_projection else {
        staged.next_row_index += 1;
        let receipt = finish_receipt(
            &before,
            &staged,
            entry,
            callbacks,
            None,
            false,
            LaterBonusDisposition::UnknownType,
            None,
        );
        *state = staged;
        return Ok(receipt);
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

    let redraw = facts.chance_group != staged.last_chance_group || facts.chance_group == 0;
    let mut random = Random::new(staged.mutation.random_state);
    let chance_draw = if redraw {
        let state_before = random.state();
        let raw = random.get(0, 0xffff);
        let modulo_100 = raw % 100;
        staged.last_chance_group = facts.chance_group;
        staged.signed_chance_budget = modulo_100;
        staged.mutation.random_state = random.state();
        Some(ChanceRandomDraw {
            call_va: CHANCE_RANDOM_GET_CALL_VA,
            random_get_va: RANDOM_GET_VA,
            low: 0,
            high: 0xffff,
            state_before,
            raw,
            modulo_100,
            state_after: random.state(),
        })
    } else {
        None
    };
    let winner_register = !redraw && staged.winner_seen;
    if staged.signed_chance_budget < 0 && (facts.chance != 0 || !winner_register) {
        staged.winner_seen = false;
        staged.next_row_index += 1;
        let receipt = finish_receipt(
            &before,
            &staged,
            entry,
            callbacks,
            chance_draw,
            false,
            LaterBonusDisposition::NegativeCarryBlocked,
            None,
        );
        *state = staged;
        return Ok(receipt);
    }

    staged.signed_chance_budget = staged.signed_chance_budget.wrapping_sub(facts.chance);
    if staged.signed_chance_budget >= 0 {
        staged.winner_seen = false;
        staged.next_row_index += 1;
        let receipt = finish_receipt(
            &before,
            &staged,
            entry,
            callbacks,
            chance_draw,
            true,
            LaterBonusDisposition::ChanceMiss,
            None,
        );
        *state = staged;
        return Ok(receipt);
    }

    if !facts.validate_num_rare() {
        return Err(RemainingBonusRowsError::InvalidScaledAttributes);
    }
    staged.winner_seen = true;
    callbacks.push(callback(
        NUM_RARE_SCALE_CALL_VA,
        MAP_SCALE_NUMBER_VA,
        CallbackKind::Scale(ScaledAttribute::NumRare),
    ));
    if facts
        .scaled_value(ScaledAttribute::NumRare)
        .expect("scaled facts validated above")
        == 0
    {
        staged.next_row_index += 1;
        let receipt = finish_receipt(
            &before,
            &staged,
            entry,
            callbacks,
            chance_draw,
            true,
            LaterBonusDisposition::ZeroRequested,
            None,
        );
        *state = staged;
        return Ok(receipt);
    }
    if !facts.validate_scaled() {
        // Retail reads no later placement attributes when scaled `numrare` is zero.
        return Err(RemainingBonusRowsError::InvalidScaledAttributes);
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
    let request = crate::place_resources_bonus_mutation_frontier::PlacementRequest {
        call_va,
        callee_va,
        path,
        params,
        random_state_before: staged.mutation.random_state,
        world_checksum_before: staged.mutation.world_checksum.clone(),
        sourced_walked_bytes: staged.mutation.sourced_walked_bytes,
        resource_pool_digest_before: staged.mutation.resource_pool_digest,
        resource_pool_before: staged.mutation.resource_pool.clone(),
    };
    let Some(placement) = host.place(&request) else {
        return Err(RemainingBonusRowsError::PlacementReceiptUnavailable);
    };
    validate_placement_receipt(&request, &placement).map_err(RemainingBonusRowsError::from)?;

    staged.mutation.random_state = placement.random_state_after;
    staged.mutation.world_checksum = placement.world_checksum_after.clone();
    staged.mutation.resource_pool_digest = placement.resource_pool_digest_after;
    staged.mutation.resource_pool = placement.resource_pool_after.clone();
    staged.mutation.allocated_resources = staged
        .mutation
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
    staged.mutation.requested_resources =
        staged.mutation.requested_resources.wrapping_add(requested);
    staged.next_row_index += 1;
    let receipt = finish_receipt(
        &before,
        &staged,
        entry,
        callbacks,
        chance_draw,
        true,
        LaterBonusDisposition::Placed(path),
        Some(placement),
    );
    *state = staged;
    Ok(receipt)
}

/// Execute the first row of FISH or GOODIES with the exact chance carry produced
/// by the completed preceding category. This is the same mutation cone beginning
/// at `0x0068fc0d`, but its control-flow entry is category enumeration at
/// `0x0068fb98..0x0068fbb3`, not the later-row recurrence at `0x00690215`.
pub fn execute_carried_category_first_mutation<H: PlacementHost>(
    state: &mut RemainingBonusRowsState,
    entry: &PlaceResourcesBonusRowsHandoff,
    category: ResourceCategory,
    facts: &LaterBonusMutationFacts,
    host: &mut H,
) -> Result<CarriedCategoryFirstMutationReceipt, RemainingBonusRowsError> {
    if category == ResourceCategory::Bonuses
        || entry.resume_va != PLACE_RESOURCES_XML_RESIDUAL_VA
        || entry.first_row_body_va != FIRST_BONUS_ROW_BODY_VA
        || !entry.selected_document_live
        || !entry.default_document_live
        || state.next_row_index != 0
        || entry.rows.is_empty()
        || state.section_source != entry.section_source
        || state.rows != entry.rows
        || state.mutation.sourced_walked_bytes != entry.sourced_walked_bytes
        || state.mutation.random_state != entry.random_state
        || state.mutation.world_checksum != entry.world_checksum
        || state.mutation.resource_pool_digest != entry.resource_pool_digest
        || state.mutation.resource_pool.as_ref().is_some_and(|pool| {
            crate::place_resources_pool_frontier::resource_divvy_pool_digest(pool)
                != state.mutation.resource_pool_digest
        })
    {
        return Err(RemainingBonusRowsError::WrongHandoff);
    }
    let row = &entry.rows[0];
    if facts.capture_ordinal != row.capture_ordinal
        || !facts.evidence.admissible_carried(category, state, facts)
    {
        return Err(RemainingBonusRowsError::StaleEvidence);
    }
    let type_projection = facts.type_resolution.good_and_selector();
    if type_projection.is_none() && facts.type_resolution != ResourceTypeResolution::Unknown {
        return Err(RemainingBonusRowsError::InvalidTypeResolution);
    }

    let before = state.clone();
    let mut staged = state.clone();
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

    let Some((good_id, selector)) = type_projection else {
        staged.next_row_index += 1;
        let receipt = finish_carried_first_receipt(
            category,
            &before,
            &staged,
            entry,
            callbacks,
            None,
            false,
            LaterBonusDisposition::UnknownType,
            None,
        );
        *state = staged;
        return Ok(receipt);
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

    let redraw = facts.chance_group != staged.last_chance_group || facts.chance_group == 0;
    let mut random = Random::new(staged.mutation.random_state);
    let chance_draw = if redraw {
        let state_before = random.state();
        let raw = random.get(0, 0xffff);
        let modulo_100 = raw % 100;
        staged.last_chance_group = facts.chance_group;
        staged.signed_chance_budget = modulo_100;
        staged.mutation.random_state = random.state();
        Some(ChanceRandomDraw {
            call_va: CHANCE_RANDOM_GET_CALL_VA,
            random_get_va: RANDOM_GET_VA,
            low: 0,
            high: 0xffff,
            state_before,
            raw,
            modulo_100,
            state_after: random.state(),
        })
    } else {
        None
    };
    let winner_register = !redraw && staged.winner_seen;
    if staged.signed_chance_budget < 0 && (facts.chance != 0 || !winner_register) {
        staged.winner_seen = false;
        staged.next_row_index += 1;
        let receipt = finish_carried_first_receipt(
            category,
            &before,
            &staged,
            entry,
            callbacks,
            chance_draw,
            false,
            LaterBonusDisposition::NegativeCarryBlocked,
            None,
        );
        *state = staged;
        return Ok(receipt);
    }

    staged.signed_chance_budget = staged.signed_chance_budget.wrapping_sub(facts.chance);
    if staged.signed_chance_budget >= 0 {
        staged.winner_seen = false;
        staged.next_row_index += 1;
        let receipt = finish_carried_first_receipt(
            category,
            &before,
            &staged,
            entry,
            callbacks,
            chance_draw,
            true,
            LaterBonusDisposition::ChanceMiss,
            None,
        );
        *state = staged;
        return Ok(receipt);
    }

    if !facts.validate_num_rare() {
        return Err(RemainingBonusRowsError::InvalidScaledAttributes);
    }
    staged.winner_seen = true;
    callbacks.push(callback(
        NUM_RARE_SCALE_CALL_VA,
        MAP_SCALE_NUMBER_VA,
        CallbackKind::Scale(ScaledAttribute::NumRare),
    ));
    if facts
        .scaled_value(ScaledAttribute::NumRare)
        .expect("scaled facts validated above")
        == 0
    {
        staged.next_row_index += 1;
        let receipt = finish_carried_first_receipt(
            category,
            &before,
            &staged,
            entry,
            callbacks,
            chance_draw,
            true,
            LaterBonusDisposition::ZeroRequested,
            None,
        );
        *state = staged;
        return Ok(receipt);
    }
    if !facts.validate_scaled() {
        return Err(RemainingBonusRowsError::InvalidScaledAttributes);
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
    let request = crate::place_resources_bonus_mutation_frontier::PlacementRequest {
        call_va,
        callee_va,
        path,
        params,
        random_state_before: staged.mutation.random_state,
        world_checksum_before: staged.mutation.world_checksum.clone(),
        sourced_walked_bytes: staged.mutation.sourced_walked_bytes,
        resource_pool_digest_before: staged.mutation.resource_pool_digest,
        resource_pool_before: staged.mutation.resource_pool.clone(),
    };
    let Some(placement) = host.place(&request) else {
        return Err(RemainingBonusRowsError::PlacementReceiptUnavailable);
    };
    validate_placement_receipt(&request, &placement).map_err(RemainingBonusRowsError::from)?;

    staged.mutation.random_state = placement.random_state_after;
    staged.mutation.world_checksum = placement.world_checksum_after.clone();
    staged.mutation.resource_pool_digest = placement.resource_pool_digest_after;
    staged.mutation.resource_pool = placement.resource_pool_after.clone();
    staged.mutation.allocated_resources = staged
        .mutation
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
    staged.mutation.requested_resources =
        staged.mutation.requested_resources.wrapping_add(requested);
    staged.next_row_index += 1;
    let receipt = finish_carried_first_receipt(
        category,
        &before,
        &staged,
        entry,
        callbacks,
        chance_draw,
        true,
        LaterBonusDisposition::Placed(path),
        Some(placement),
    );
    *state = staged;
    Ok(receipt)
}
