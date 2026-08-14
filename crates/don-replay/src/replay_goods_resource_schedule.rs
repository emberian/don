// SPDX-License-Identifier: GPL-3.0-or-later
//! Goods-channel splice from the initial oil owner through admitted resource placements.
//!
//! `Map::place_resources` owns the later initial `Good` producers.  The replay schedule
//! already retains each admitted `Objects::init_good` call as a typed `ResourceAllocation`.
//! This module consumes those receipts in native order and applies the exact sparse-array
//! and `Good::init` projection to the same `OilGoodRuntime` used by the initial/oil owner.
//!
//! The splice is intentionally bounded.  It does not port either placement body and it
//! does not cross the open `GOODIES`/`FISH` category tail.  Consequently it can own the
//! 22 checksum-walked bytes of every allocation present in an admitted schedule receipt,
//! but it never claims the pending first replay checkpoint.

use crate::map_make_resource_caller_gap_frontier::{
    MAP_PLACE_RESOURCES_ENTRY_VA, MAP_POST_RESOURCES_CHECKPOINT_CALL_VA,
    MAP_POST_RESOURCES_SOURCE_TOKEN,
};
use crate::map_make_resource_schedule_integration::{
    MapMakeResourcePlacementReceipt, MapMakeResourceScheduleReceipt,
};
use crate::place_resources_bonus_mutation_frontier::{
    AllocationKind, FirstBonusDisposition, PlacementEvidence, PlacementPath, PlacementReceipt,
    ResourceAllocation, WorldOccupancyWrite, BONUS_CATEGORY_TAIL_VA, GOOD_WDATA_DOWN_MARKER,
    GOOD_WDATA_DOWN_WHO_WRITE_VA, GOOD_WDATA_DOWN_WRITE_VA, ITEM_GOOD_ID, OBJECTS_INIT_GOOD_VA,
    OBJECTS_INIT_ITEM_VA, PLAYER_INIT_GOOD_CALL_VA, PLAYER_INIT_ITEM_CALL_VA,
    REGION_INIT_GOOD_CALL_VA, REGION_INIT_ITEM_CALL_VA, SHIPPED_EXE_SHA256,
};
use crate::place_resources_bonus_rows_mutation_frontier::LaterBonusDisposition;
use crate::place_resources_canonical_transaction::CanonicalRowMutationReceipt;
use crate::place_resources_category_frontier::ResourceCategory;
use crate::place_resources_pool_frontier::MAP_PLACE_RESOURCES_PREFIX_RESIDUAL_VA;
use don_sim::container::increase_by;
use don_sim::systems::economy::GoodNode;
use don_sim::systems::map_terrain::div_3;
use don_sim::systems::world_oil_goods::{
    OilGoodRuntime, OilGoodSlot, OilGoodStateSummary, CLOSED_COORD_INTERNAL, GOOD_WALKED_BYTES,
    OIL_GOOD_TYPE, SUBOBJECT_COORD_XOR,
};

/// `SubObject::init`, called by `Good::init`.
pub const SUBOBJECT_INIT_VA: u32 = 0x0066_2300;
/// `GoodTypeData::is_flat`, whose result contributes bit `0x20` to walked flags.
pub const GOOD_TYPE_IS_FLAT_VA: u32 = 0x0047_80c0;
/// `Good::init`.
pub const GOOD_INIT_VA: u32 = 0x0066_da20;
/// Flat-object flag installed by `SubObject::init`.
pub const SUBOBJECT_FLAT_FLAG: u8 = 0x20;

/// Exact source of the behavior-driving `GoodTypeData::is_flat` result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoodTypeFlatEvidence {
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

impl GoodTypeFlatEvidence {
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

/// One synchronized Good-type fact needed by `SubObject::init`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceGoodTypeFact {
    pub type_index: i32,
    pub is_flat: bool,
    pub evidence: GoodTypeFlatEvidence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceScheduleGoodsBoundary {
    /// The caller lawfully skipped `Map::place_resources`.
    PlaceResourcesSkipped,
    /// The caller admitted the call, but the function body remains open at entry.
    AtPlaceResourcesEntry,
    /// The pool prefix completed, but XML/category setup remains open.
    AfterPoolPrefix,
    /// XML/category setup completed; no BONUS mutation row is admitted yet.
    BeforeBonusRows,
    /// Exactly the first BONUS row is represented.
    AfterFirstBonusRow,
    /// One or more later BONUS rows are represented; another row remains.
    BeforeNextBonusRow,
    /// The final BONUS recurrence reached the still-unowned category cleanup.
    BeforeBonusCategoryCleanup,
    /// BONUSES cleanup completed and a nonempty FISH section is at row zero.
    BeforeFirstFishRow,
    /// BONUSES cleanup completed into an empty FISH section.
    BeforeFishCategoryCleanup,
    /// Exactly the first FISH row is represented; recurrence remains open.
    AfterFirstFishRow,
    /// Empty FISH cleanup completed and a nonempty GOODIES section is at row zero.
    BeforeFirstGoodiesRow,
    /// Empty FISH cleanup completed into an empty GOODIES section.
    BeforeGoodiesCategoryCleanup,
}

impl ResourceScheduleGoodsBoundary {
    pub const fn first_remaining_schedule_va(self) -> Option<u32> {
        match self {
            Self::PlaceResourcesSkipped => None,
            Self::AtPlaceResourcesEntry => Some(MAP_PLACE_RESOURCES_ENTRY_VA),
            Self::AfterPoolPrefix => Some(MAP_PLACE_RESOURCES_PREFIX_RESIDUAL_VA),
            Self::BeforeBonusRows | Self::AfterFirstBonusRow | Self::BeforeNextBonusRow => {
                Some(crate::place_resources_bonus_mutation_frontier::FIRST_BONUS_ROW_BODY_VA)
            }
            Self::BeforeBonusCategoryCleanup => Some(BONUS_CATEGORY_TAIL_VA),
            Self::BeforeFirstFishRow => Some(crate::place_resources_category_frontier::ROW_BODY_VA),
            Self::BeforeFishCategoryCleanup => {
                Some(crate::place_resources_category_frontier::CATEGORY_TAIL_VA)
            }
            Self::AfterFirstFishRow => {
                Some(crate::place_resources_bonus_mutation_frontier::FIRST_BONUS_ROW_RESIDUAL_VA)
            }
            Self::BeforeFirstGoodiesRow => {
                Some(crate::place_resources_category_frontier::ROW_BODY_VA)
            }
            Self::BeforeGoodiesCategoryCleanup => {
                Some(crate::place_resources_category_frontier::CATEGORY_TAIL_VA)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceGoodAllocationKind {
    ReusedInactive,
    Appended,
}

/// Exact Goods-side result of one admitted `Objects::init_good` call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceGoodAllocationReceipt {
    pub placement_ordinal: usize,
    pub allocation_ordinal: usize,
    pub source: ResourceAllocation,
    pub storage_kind: ResourceGoodAllocationKind,
    pub capacity_before: i32,
    pub capacity_after: i32,
    pub walked_bytes_added: usize,
    pub checksum_before: u32,
    pub checksum_after: u32,
}

/// Bounded schedule-to-Goods continuation receipt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceScheduleGoodsReceipt {
    pub boundary: ResourceScheduleGoodsBoundary,
    pub placements_seen: usize,
    pub allocations_seen: usize,
    pub item_allocations_ignored: usize,
    pub goods_allocations: Vec<ResourceGoodAllocationReceipt>,
    pub before: OilGoodStateSummary,
    pub after: OilGoodStateSummary,
    pub goods_after: OilGoodRuntime,
    pub newly_owned_walked_bytes: usize,
    /// Always false until category cleanup, `GOODIES`, `FISH`, and the caller return run.
    pub complete_for_first_checkpoint: bool,
    pub pending_checkpoint_call_va: u32,
    pub pending_source_token: u32,
    /// Exact next schedule instruction; `None` when the caller lawfully skipped the call.
    pub first_remaining_schedule_va: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceScheduleGoodsError {
    InvalidInitialRuntime {
        reason: &'static str,
    },
    InvalidScheduleContinuity {
        reason: &'static str,
    },
    InvalidPlacement {
        placement: usize,
        reason: &'static str,
    },
    InvalidAllocation {
        placement: usize,
        allocation: usize,
        reason: &'static str,
    },
    MissingGoodTypeFact {
        type_index: i32,
    },
    DuplicateGoodTypeFact {
        type_index: i32,
    },
    InvalidGoodTypeFact {
        type_index: i32,
    },
    SlotMismatch {
        expected: usize,
        actual: i32,
    },
    ArrayCannotGrow {
        capacity: i32,
        increment: i16,
    },
    CapacityOverflow {
        capacity: i32,
        increment: i16,
    },
}

impl std::fmt::Display for ResourceScheduleGoodsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "resource-schedule Goods splice refused: {self:?}")
    }
}

impl std::error::Error for ResourceScheduleGoodsError {}

/// Extract the admitted placement history from the public schedule receipt and continue
/// the supplied initial/oil Goods runtime through it.
pub fn continue_goods_through_resource_schedule(
    initial: &OilGoodRuntime,
    schedule: &MapMakeResourceScheduleReceipt,
    catalog: &[ResourceGoodTypeFact],
) -> Result<ResourceScheduleGoodsReceipt, ResourceScheduleGoodsError> {
    let (boundary, placements) = placements_from_schedule(schedule)?;
    apply_admitted_resource_placements(initial, boundary, &placements, catalog)
}

/// Apply a previously admitted placement transcript.  This split keeps the object owner
/// independently mutation-testable without manufacturing the large upstream Map receipt.
pub fn apply_admitted_resource_placements(
    initial: &OilGoodRuntime,
    boundary: ResourceScheduleGoodsBoundary,
    placements: &[&PlacementReceipt],
    catalog: &[ResourceGoodTypeFact],
) -> Result<ResourceScheduleGoodsReceipt, ResourceScheduleGoodsError> {
    validate_runtime(initial)?;
    validate_catalog(catalog)?;

    let before = initial.summary();
    let mut staged = initial.clone();
    let mut goods_allocations = Vec::new();
    let mut allocations_seen = 0;
    let mut item_allocations_ignored = 0;

    for (placement_ordinal, placement) in placements.iter().enumerate() {
        validate_placement_header(placement_ordinal, placement)?;
        allocations_seen += placement.allocations.len();
        for (allocation_ordinal, allocation) in placement.allocations.iter().enumerate() {
            validate_allocation_shape(
                placement_ordinal,
                allocation_ordinal,
                placement,
                allocation,
            )?;
            if allocation.kind == AllocationKind::Item {
                item_allocations_ignored += 1;
                continue;
            }
            let is_flat = flat_fact(allocation.type_id, catalog)?;
            let checksum_before = staged.goods_checksum();
            let (storage_kind, capacity_before, capacity_after) =
                allocate_exact_slot(&mut staged, allocation.slot)?;
            init_resource_good(&mut staged, allocation, is_flat);
            validate_runtime(&staged)?;
            goods_allocations.push(ResourceGoodAllocationReceipt {
                placement_ordinal,
                allocation_ordinal,
                source: allocation.clone(),
                storage_kind,
                capacity_before,
                capacity_after,
                walked_bytes_added: GOOD_WALKED_BYTES,
                checksum_before,
                checksum_after: staged.goods_checksum(),
            });
        }
    }

    let after = staged.summary();
    let newly_owned_walked_bytes = goods_allocations.len() * GOOD_WALKED_BYTES;
    if after.goods_walked_bytes != before.goods_walked_bytes + newly_owned_walked_bytes {
        return Err(ResourceScheduleGoodsError::InvalidScheduleContinuity {
            reason: "resource allocation did not add exactly one active walked row",
        });
    }

    Ok(ResourceScheduleGoodsReceipt {
        boundary,
        placements_seen: placements.len(),
        allocations_seen,
        item_allocations_ignored,
        goods_allocations,
        before,
        after,
        goods_after: staged,
        newly_owned_walked_bytes,
        complete_for_first_checkpoint: false,
        pending_checkpoint_call_va: MAP_POST_RESOURCES_CHECKPOINT_CALL_VA,
        pending_source_token: MAP_POST_RESOURCES_SOURCE_TOKEN,
        first_remaining_schedule_va: boundary.first_remaining_schedule_va(),
    })
}

fn placements_from_schedule(
    schedule: &MapMakeResourceScheduleReceipt,
) -> Result<(ResourceScheduleGoodsBoundary, Vec<&PlacementReceipt>), ResourceScheduleGoodsError> {
    let mut placements = Vec::new();
    let boundary = match &schedule.placement {
        MapMakeResourcePlacementReceipt::Skipped(_) => {
            ResourceScheduleGoodsBoundary::PlaceResourcesSkipped
        }
        MapMakeResourcePlacementReceipt::EntryOpen(_) => {
            ResourceScheduleGoodsBoundary::AtPlaceResourcesEntry
        }
        MapMakeResourcePlacementReceipt::BodyOpen(_) => {
            ResourceScheduleGoodsBoundary::AfterPoolPrefix
        }
        MapMakeResourcePlacementReceipt::XmlRowsOpen(_) => {
            ResourceScheduleGoodsBoundary::BeforeBonusRows
        }
        MapMakeResourcePlacementReceipt::FirstBonusRowOpen(first) => {
            if first.pending_checkpoint_call_va != MAP_POST_RESOURCES_CHECKPOINT_CALL_VA
                || first.pending_source_token != MAP_POST_RESOURCES_SOURCE_TOKEN
            {
                return Err(ResourceScheduleGoodsError::InvalidScheduleContinuity {
                    reason: "first BONUS row changed the pending caller checkpoint",
                });
            }
            push_first_placement(
                &first.first_bonus.disposition,
                &first.first_bonus.placement,
                &mut placements,
            )?;
            ResourceScheduleGoodsBoundary::AfterFirstBonusRow
        }
        MapMakeResourcePlacementReceipt::BonusRowsOpen(rows) => {
            if rows.pending_checkpoint_call_va != MAP_POST_RESOURCES_CHECKPOINT_CALL_VA
                || rows.pending_source_token != MAP_POST_RESOURCES_SOURCE_TOKEN
                || rows.remaining_state.next_row_index != rows.steps.len() + 1
            {
                return Err(ResourceScheduleGoodsError::InvalidScheduleContinuity {
                    reason: "later BONUS history does not match its row cursor/checkpoint",
                });
            }
            push_first_placement(
                &rows.first.first_bonus.disposition,
                &rows.first.first_bonus.placement,
                &mut placements,
            )?;
            for step in &rows.steps {
                let placed = matches!(step.receipt.disposition, LaterBonusDisposition::Placed(_));
                if placed != step.receipt.placement.is_some() {
                    return Err(ResourceScheduleGoodsError::InvalidScheduleContinuity {
                        reason: "later BONUS disposition/placement mismatch",
                    });
                }
                if let Some(placement) = &step.receipt.placement {
                    placements.push(placement);
                }
            }
            if rows.category_tail.is_some() {
                ResourceScheduleGoodsBoundary::BeforeBonusCategoryCleanup
            } else {
                ResourceScheduleGoodsBoundary::BeforeNextBonusRow
            }
        }
        MapMakeResourcePlacementReceipt::FishCategoryOpen(fish) => {
            let rows = &fish.bonus_rows;
            if fish.pending_checkpoint_call_va != MAP_POST_RESOURCES_CHECKPOINT_CALL_VA
                || fish.pending_source_token != MAP_POST_RESOURCES_SOURCE_TOKEN
                || rows.remaining_state.next_row_index != rows.steps.len() + 1
                || rows.category_tail.is_none()
            {
                return Err(ResourceScheduleGoodsError::InvalidScheduleContinuity {
                    reason: "FISH boundary does not retain completed BONUS history",
                });
            }
            push_first_placement(
                &rows.first.first_bonus.disposition,
                &rows.first.first_bonus.placement,
                &mut placements,
            )?;
            for step in &rows.steps {
                let placed = matches!(step.receipt.disposition, LaterBonusDisposition::Placed(_));
                if placed != step.receipt.placement.is_some() {
                    return Err(ResourceScheduleGoodsError::InvalidScheduleContinuity {
                        reason: "later BONUS disposition/placement mismatch",
                    });
                }
                if let Some(placement) = &step.receipt.placement {
                    placements.push(placement);
                }
            }
            if fish.residual_va == crate::place_resources_category_frontier::ROW_BODY_VA {
                ResourceScheduleGoodsBoundary::BeforeFirstFishRow
            } else if fish.residual_va == crate::place_resources_category_frontier::CATEGORY_TAIL_VA
            {
                ResourceScheduleGoodsBoundary::BeforeFishCategoryCleanup
            } else {
                return Err(ResourceScheduleGoodsError::InvalidScheduleContinuity {
                    reason: "FISH boundary has an unknown residual",
                });
            }
        }
        MapMakeResourcePlacementReceipt::FirstFishRowOpen(first_fish) => {
            let rows = &first_fish.fish_category.bonus_rows;
            if first_fish.pending_checkpoint_call_va != MAP_POST_RESOURCES_CHECKPOINT_CALL_VA
                || first_fish.pending_source_token != MAP_POST_RESOURCES_SOURCE_TOKEN
                || first_fish.residual_va
                    != crate::place_resources_bonus_mutation_frontier::FIRST_BONUS_ROW_RESIDUAL_VA
                || rows.remaining_state.next_row_index != rows.steps.len() + 1
                || rows.category_tail.is_none()
                || first_fish.remaining_state.next_row_index != 1
                || first_fish.canonical_state_after.mutation != first_fish.remaining_state.mutation
                || first_fish.row_receipt.category != ResourceCategory::Fish
                || first_fish.row_receipt.row_index != 0
                || first_fish.remaining_state.mutation.resource_pool.as_ref()
                    != Some(&first_fish.resource_pool_after)
            {
                return Err(ResourceScheduleGoodsError::InvalidScheduleContinuity {
                    reason: "first FISH row does not retain exact history/checkpoint",
                });
            }
            push_first_placement(
                &rows.first.first_bonus.disposition,
                &rows.first.first_bonus.placement,
                &mut placements,
            )?;
            for step in &rows.steps {
                let placed = matches!(step.receipt.disposition, LaterBonusDisposition::Placed(_));
                if placed != step.receipt.placement.is_some() {
                    return Err(ResourceScheduleGoodsError::InvalidScheduleContinuity {
                        reason: "later BONUS disposition/placement mismatch",
                    });
                }
                if let Some(placement) = &step.receipt.placement {
                    placements.push(placement);
                }
            }
            let CanonicalRowMutationReceipt::CarriedFirst(receipt) =
                &first_fish.row_receipt.mutation
            else {
                return Err(ResourceScheduleGoodsError::InvalidScheduleContinuity {
                    reason: "first FISH row has the wrong mutation receipt",
                });
            };
            if receipt.category != ResourceCategory::Fish
                || receipt.residual_va
                    != crate::place_resources_bonus_mutation_frontier::FIRST_BONUS_ROW_RESIDUAL_VA
            {
                return Err(ResourceScheduleGoodsError::InvalidScheduleContinuity {
                    reason: "first FISH mutation receipt has the wrong category/residual",
                });
            }
            let placed = matches!(receipt.disposition, LaterBonusDisposition::Placed(_));
            if placed != receipt.placement.is_some() {
                return Err(ResourceScheduleGoodsError::InvalidScheduleContinuity {
                    reason: "first FISH disposition/placement mismatch",
                });
            }
            if let Some(placement) = &receipt.placement {
                placements.push(placement);
            }
            ResourceScheduleGoodsBoundary::AfterFirstFishRow
        }
        MapMakeResourcePlacementReceipt::GoodiesCategoryOpen(goodies) => {
            let rows = &goodies.fish_category.bonus_rows;
            if goodies.pending_checkpoint_call_va != MAP_POST_RESOURCES_CHECKPOINT_CALL_VA
                || goodies.pending_source_token != MAP_POST_RESOURCES_SOURCE_TOKEN
                || rows.remaining_state.next_row_index != rows.steps.len() + 1
                || rows.category_tail.is_none()
                || !goodies.fish_category.fish_handoff.rows.is_empty()
                || goodies.category_state_after.category != ResourceCategory::Goodies
                || goodies.category_state_after.rows_remaining != goodies.goodies_handoff.rows.len()
                || goodies.category_receipt.completed_category != ResourceCategory::Fish
                || goodies.resource_pool_after != goodies.fish_category.resource_pool_after
            {
                return Err(ResourceScheduleGoodsError::InvalidScheduleContinuity {
                    reason: "GOODIES boundary does not retain empty-FISH/BONUSES history",
                });
            }
            push_first_placement(
                &rows.first.first_bonus.disposition,
                &rows.first.first_bonus.placement,
                &mut placements,
            )?;
            for step in &rows.steps {
                let placed = matches!(step.receipt.disposition, LaterBonusDisposition::Placed(_));
                if placed != step.receipt.placement.is_some() {
                    return Err(ResourceScheduleGoodsError::InvalidScheduleContinuity {
                        reason: "later BONUS disposition/placement mismatch",
                    });
                }
                if let Some(placement) = &step.receipt.placement {
                    placements.push(placement);
                }
            }
            if goodies.residual_va == crate::place_resources_category_frontier::ROW_BODY_VA {
                ResourceScheduleGoodsBoundary::BeforeFirstGoodiesRow
            } else if goodies.residual_va
                == crate::place_resources_category_frontier::CATEGORY_TAIL_VA
            {
                ResourceScheduleGoodsBoundary::BeforeGoodiesCategoryCleanup
            } else {
                return Err(ResourceScheduleGoodsError::InvalidScheduleContinuity {
                    reason: "GOODIES boundary has an unknown residual",
                });
            }
        }
    };
    Ok((boundary, placements))
}

fn push_first_placement<'a>(
    disposition: &FirstBonusDisposition,
    placement: &'a Option<PlacementReceipt>,
    out: &mut Vec<&'a PlacementReceipt>,
) -> Result<(), ResourceScheduleGoodsError> {
    let placed = matches!(disposition, FirstBonusDisposition::Placed(_));
    if placed != placement.is_some() {
        return Err(ResourceScheduleGoodsError::InvalidScheduleContinuity {
            reason: "first BONUS disposition/placement mismatch",
        });
    }
    if let Some(placement) = placement {
        out.push(placement);
    }
    Ok(())
}

fn validate_runtime(goods: &OilGoodRuntime) -> Result<(), ResourceScheduleGoodsError> {
    if goods.capacity < 0 || goods.slots.len() as i64 > i64::from(goods.capacity) {
        return Err(ResourceScheduleGoodsError::InvalidInitialRuntime {
            reason: "logical length exceeds nonnegative engine capacity",
        });
    }
    if goods.array_flags != 0 {
        return Err(ResourceScheduleGoodsError::InvalidInitialRuntime {
            reason: "unsupported PtrArray flags",
        });
    }
    if goods.good_mark < 0 || goods.good_mark as usize > goods.slots.len() {
        return Err(ResourceScheduleGoodsError::InvalidInitialRuntime {
            reason: "good_mark is outside logical length",
        });
    }
    for (index, slot) in goods.slots.iter().enumerate() {
        if slot.active()
            && (index >= goods.good_mark as usize
                || !slot.ptype_present
                || slot.node.who != u8::MAX
                || slot.node.o != index as i16)
        {
            return Err(ResourceScheduleGoodsError::InvalidInitialRuntime {
                reason: "active Good violates slot/type/owner/mark invariants",
            });
        }
    }
    Ok(())
}

fn validate_catalog(catalog: &[ResourceGoodTypeFact]) -> Result<(), ResourceScheduleGoodsError> {
    for (index, fact) in catalog.iter().enumerate() {
        if fact.type_index < 0 || !fact.evidence.admissible() {
            return Err(ResourceScheduleGoodsError::InvalidGoodTypeFact {
                type_index: fact.type_index,
            });
        }
        if catalog[..index]
            .iter()
            .any(|prior| prior.type_index == fact.type_index)
        {
            return Err(ResourceScheduleGoodsError::DuplicateGoodTypeFact {
                type_index: fact.type_index,
            });
        }
    }
    Ok(())
}

fn flat_fact(
    type_index: i32,
    catalog: &[ResourceGoodTypeFact],
) -> Result<bool, ResourceScheduleGoodsError> {
    if type_index == OIL_GOOD_TYPE {
        return Ok(false);
    }
    catalog
        .iter()
        .find(|fact| fact.type_index == type_index)
        .map(|fact| fact.is_flat)
        .ok_or(ResourceScheduleGoodsError::MissingGoodTypeFact { type_index })
}

fn validate_placement_header(
    placement_ordinal: usize,
    placement: &PlacementReceipt,
) -> Result<(), ResourceScheduleGoodsError> {
    if placement.allocated_count < 0
        || placement.allocated_count as usize != placement.allocations.len()
    {
        return Err(ResourceScheduleGoodsError::InvalidPlacement {
            placement: placement_ordinal,
            reason: "allocated_count does not match allocation transcript",
        });
    }
    let evidence_ok = match &placement.evidence {
        PlacementEvidence::RetailCapture {
            executable_sha256,
            capture_sha256,
        } => executable_sha256 == SHIPPED_EXE_SHA256 && capture_sha256.iter().any(|b| *b != 0),
        PlacementEvidence::ExactPort {
            implementation_sha256,
            proof_document,
        } => implementation_sha256.iter().any(|b| *b != 0) && !proof_document.is_empty(),
        PlacementEvidence::SyntheticFixture { fixture } => cfg!(test) && !fixture.is_empty(),
    };
    if !evidence_ok {
        return Err(ResourceScheduleGoodsError::InvalidPlacement {
            placement: placement_ordinal,
            reason: "placement evidence is not admissible",
        });
    }
    Ok(())
}

fn validate_allocation_shape(
    placement_ordinal: usize,
    allocation_ordinal: usize,
    placement: &PlacementReceipt,
    allocation: &ResourceAllocation,
) -> Result<(), ResourceScheduleGoodsError> {
    let request = &placement.request;
    let (good_call, item_call) = match request.path {
        PlacementPath::Player => (PLAYER_INIT_GOOD_CALL_VA, PLAYER_INIT_ITEM_CALL_VA),
        PlacementPath::Region => (REGION_INIT_GOOD_CALL_VA, REGION_INIT_ITEM_CALL_VA),
    };
    let expected_kind = if request.params.good_id == ITEM_GOOD_ID {
        AllocationKind::Item
    } else {
        AllocationKind::Good
    };
    let (expected_call, expected_callee) = match expected_kind {
        AllocationKind::Good => (good_call, OBJECTS_INIT_GOOD_VA),
        AllocationKind::Item => (item_call, OBJECTS_INIT_ITEM_VA),
    };
    let type_matches = if request.params.selector == 0 {
        allocation.type_id == request.params.good_id
    } else {
        request.params.good_id == -1 && allocation.type_id >= 0
    };
    let fail = |reason| ResourceScheduleGoodsError::InvalidAllocation {
        placement: placement_ordinal,
        allocation: allocation_ordinal,
        reason,
    };
    if allocation.call_va != expected_call
        || allocation.callee_va != expected_callee
        || allocation.kind != expected_kind
        || !type_matches
        || allocation.slot < 0
        || allocation.slot > i32::from(i16::MAX)
    {
        return Err(fail("callsite, kind, type, or slot does not match request"));
    }
    if allocation.kind == AllocationKind::Good {
        validate_good_occupancy(allocation).map_err(fail)?;
    }
    Ok(())
}

fn validate_good_occupancy(allocation: &ResourceAllocation) -> Result<(), &'static str> {
    if allocation.type_id == OIL_GOOD_TYPE {
        return if allocation.occupancy_write.is_none() {
            Ok(())
        } else {
            Err("type-5 Good must not write ordinary occupancy")
        };
    }
    let Some(WorldOccupancyWrite {
        world_x,
        world_y,
        down_write_va,
        down_who_write_va,
        new_down,
        new_down_who,
        ..
    }) = allocation.occupancy_write.as_ref()
    else {
        return Err("ordinary Good is missing its WData occupancy receipt");
    };
    if *world_x != div_3(allocation.coord_x >> 8)
        || *world_y != div_3(allocation.coord_y >> 8)
        || *down_write_va != GOOD_WDATA_DOWN_WRITE_VA
        || *down_who_write_va != GOOD_WDATA_DOWN_WHO_WRITE_VA
        || *new_down != GOOD_WDATA_DOWN_MARKER
        || *new_down_who != allocation.slot as i16
    {
        return Err("ordinary Good occupancy receipt does not match allocation");
    }
    Ok(())
}

fn allocate_exact_slot(
    goods: &mut OilGoodRuntime,
    claimed_slot: i32,
) -> Result<(ResourceGoodAllocationKind, i32, i32), ResourceScheduleGoodsError> {
    let expected = goods
        .slots
        .iter()
        .position(|slot| !slot.active())
        .unwrap_or(goods.slots.len());
    if claimed_slot != expected as i32 {
        return Err(ResourceScheduleGoodsError::SlotMismatch {
            expected,
            actual: claimed_slot,
        });
    }
    let capacity_before = goods.capacity;
    if expected < goods.slots.len() {
        return Ok((
            ResourceGoodAllocationKind::ReusedInactive,
            capacity_before,
            goods.capacity,
        ));
    }
    if goods.slots.len() as i32 >= goods.capacity {
        let growth = increase_by(goods.increment, goods.capacity);
        if growth == 0 {
            return Err(ResourceScheduleGoodsError::ArrayCannotGrow {
                capacity: goods.capacity,
                increment: goods.increment,
            });
        }
        let Some(next_capacity) = goods.capacity.checked_add(growth) else {
            return Err(ResourceScheduleGoodsError::CapacityOverflow {
                capacity: goods.capacity,
                increment: goods.increment,
            });
        };
        if next_capacity <= goods.capacity || next_capacity <= goods.slots.len() as i32 {
            return Err(ResourceScheduleGoodsError::CapacityOverflow {
                capacity: goods.capacity,
                increment: goods.increment,
            });
        }
        goods.capacity = next_capacity;
    }
    goods.slots.push(OilGoodSlot::default());
    Ok((
        ResourceGoodAllocationKind::Appended,
        capacity_before,
        goods.capacity,
    ))
}

fn init_resource_good(goods: &mut OilGoodRuntime, allocation: &ResourceAllocation, is_flat: bool) {
    let index = allocation.slot as usize;
    let flags = 1 | if is_flat { SUBOBJECT_FLAT_FLAG } else { 0 };
    goods.slots[index] = OilGoodSlot {
        node: GoodNode {
            flags,
            who: u8::MAX,
            o: allocation.slot as i16,
            // Map::place_resources still runs inside Map::make, before TerrainOut::init;
            // TerrainOut::find_tcoord_z therefore returns the zero fallback.
            z: 0 ^ SUBOBJECT_COORD_XOR,
            x: allocation.coord_x ^ SUBOBJECT_COORD_XOR,
            y: allocation.coord_y ^ SUBOBJECT_COORD_XOR,
            type_index: allocation.type_id,
            ever_seen: 0,
        },
        ptype_present: true,
        cur_time: 0,
    };
    goods.good_mark = goods.good_mark.max(allocation.slot + 1);
    debug_assert_ne!(goods.slots[index].node.z, CLOSED_COORD_INTERNAL);
}
