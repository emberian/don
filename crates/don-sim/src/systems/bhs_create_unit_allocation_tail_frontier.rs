// SPDX-License-Identifier: GPL-3.0-or-later
//! Source-only owner boundary for the direct positive-count tail of BHS registrations
//! 508--510.
//!
//! `ScenarioFuncSet::add_unit` does not treat nearby-placement failure as a rejection.  On
//! every direct Ground, Sea, or Air attempt it calls `UnitType::find_nearby_spot`, ignores the
//! integer return, and passes the two output cells to the complete 1,603-byte
//! `Objects::init_unit` receiver.  The latter can allocate several `uber_size` members and can
//! return a negative `Objects::find_free` failure after earlier members became live.  This
//! boundary preserves both layers of partial effects.  It deliberately does not register the
//! BHS builtins or claim that `World::allocate_typed_at` implements the native receiver.

use std::collections::BTreeSet;

pub const ADD_UNIT_VA: u32 = 0x009e_2220;
pub const ADD_UNIT_NEARBY_CALL_VA: u32 = 0x009e_23f4;
pub const ADD_UNIT_INIT_UNIT_CALL_VA: u32 = 0x009e_2412;
pub const UNIT_TYPE_FIND_NEARBY_SPOT_VA: u32 = 0x0061_de70;
pub const UNIT_TYPE_FIND_NEARBY_SPOT_BYTES: u32 = 1_433;
pub const OBJECTS_INIT_UNIT_VA: u32 = 0x0065_e0c0;
pub const OBJECTS_INIT_UNIT_BYTES: u32 = 1_603;
pub const OBJECTS_FIND_FREE_VA: u32 = 0x0065_ad60;
pub const OBJECTS_FIND_FREE_CALL_VA: u32 = 0x0065_e137;
pub const OBJECT_INIT_VTABLE_OFFSET: u32 = 0x8c;
pub const UNIT_TYPE_UBER_SIZE_OFFSET: u32 = 0x308;
pub const UNIT_LINK_PREVIOUS_OFFSET: u32 = 0x8e;
pub const UNIT_LINK_NEXT_OFFSET: u32 = 0x90;

/// The twelve arguments after `(origin_x, origin_y, &out_x, &out_y)` at `0x009E23C0..23F4`.
pub const ADD_UNIT_NEARBY_TAIL: [i32; 12] = [0, 0x0c00, 0, 0x5555_5555, 3, -1, -1, 0, 0, -1, 0, -1];

pub const ADD_UNIT_INIT_TAIL: [i32; 3] = [-1; 3];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BhsDirectAllocationRequest {
    /// Local orchestration identity. Native does not pass this value to either callee.
    pub attempt: u32,
    /// Exact zero-based Leader/object-band owner (`who - 1`).
    pub owner: i32,
    /// `LeaderData::get_graft(effective_type)`, not the effective/original type.
    pub graft_type: i32,
    /// Fine coordinates `script_coord * 192 + 96`, with x86 wrapping already applied.
    pub origin_x: i32,
    pub origin_y: i32,
}

impl BhsDirectAllocationRequest {
    pub const fn nearby(self) -> BhsNearbyRequest {
        BhsNearbyRequest {
            attempt: self.attempt,
            type_index: self.graft_type,
            origin_x: self.origin_x,
            origin_y: self.origin_y,
            tail: ADD_UNIT_NEARBY_TAIL,
        }
    }

    pub const fn init_unit(self, x: i32, y: i32) -> BhsObjectsInitUnitRequest {
        BhsObjectsInitUnitRequest {
            attempt: self.attempt,
            owner: self.owner,
            type_index: self.graft_type,
            x,
            y,
            tail: ADD_UNIT_INIT_TAIL,
        }
    }
}

/// Exact `UnitType::find_nearby_spot` call. The receiver is the graft type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BhsNearbyRequest {
    pub attempt: u32,
    pub type_index: i32,
    pub origin_x: i32,
    pub origin_y: i32,
    pub tail: [i32; 12],
}

/// The integer result is retained as evidence but never gates allocation in this caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BhsNearbyReceipt {
    pub request: BhsNearbyRequest,
    pub returned: i32,
    pub output_x: i32,
    pub output_y: i32,
}

/// Exact seven-argument `Objects::init_unit` call made by the direct BHS loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BhsObjectsInitUnitRequest {
    pub attempt: u32,
    pub owner: i32,
    pub type_index: i32,
    pub x: i32,
    pub y: i32,
    pub tail: [i32; 3],
}

/// An explicit attestation that the complete native receiver ran. A single-row world insert
/// cannot honestly produce this extent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectsInitUnitExtent {
    CompleteRetail1603ByteBody,
}

/// Observable allocation shape inside `Objects::init_unit` for the all-`-1` BHS tail.
///
/// Native reads `UnitTypeData::uber_size +0x308` and calls `Objects::find_free` once per
/// member. A negative allocator result returns immediately without rolling back any earlier
/// initialized members. On full success the public return is the final member's resolved
/// captain, which is one of the initialized owner-local object indices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectsInitUnitEffects {
    pub required_members: u32,
    pub initialized_members: Vec<i32>,
    pub terminal_find_free_failure: Option<i32>,
    pub returned_captain_or_failure: i32,
}

impl ObjectsInitUnitEffects {
    pub fn validates_complete_receiver(&self) -> bool {
        if self.required_members == 0
            || self.initialized_members.len() > self.required_members as usize
            || self.initialized_members.iter().any(|&member| member < 0)
            || self
                .initialized_members
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                .len()
                != self.initialized_members.len()
        {
            return false;
        }

        match self.terminal_find_free_failure {
            Some(failure) => {
                failure < 0
                    && self.returned_captain_or_failure == failure
                    && self.initialized_members.len() < self.required_members as usize
            }
            None => {
                self.initialized_members.len() == self.required_members as usize
                    && self.returned_captain_or_failure >= 0
                    && self
                        .initialized_members
                        .contains(&self.returned_captain_or_failure)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BhsObjectsInitUnitReceipt {
    pub request: BhsObjectsInitUnitRequest,
    pub extent: ObjectsInitUnitExtent,
    pub effects: ObjectsInitUnitEffects,
}

impl BhsObjectsInitUnitReceipt {
    pub fn validates(&self, expected: BhsObjectsInitUnitRequest) -> bool {
        self.request == expected
            && self.extent == ObjectsInitUnitExtent::CompleteRetail1603ByteBody
            && self.effects.validates_complete_receiver()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BhsDirectAllocationStep {
    Nearby(BhsNearbyReceipt),
    InitUnit(BhsObjectsInitUnitReceipt),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BhsDirectAllocationStatus {
    Applied,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BhsDirectAllocationReceipt {
    pub request: BhsDirectAllocationRequest,
    pub status: BhsDirectAllocationStatus,
    pub steps: Vec<BhsDirectAllocationStep>,
}

impl BhsDirectAllocationReceipt {
    pub fn unavailable(request: BhsDirectAllocationRequest) -> Self {
        Self {
            request,
            status: BhsDirectAllocationStatus::Unavailable,
            steps: Vec::new(),
        }
    }

    pub fn validate(
        &self,
        expected: BhsDirectAllocationRequest,
    ) -> Result<i32, BhsDirectAllocationError> {
        if self.request != expected {
            return Err(BhsDirectAllocationError::ReceiptRequestMismatch);
        }
        if self.status == BhsDirectAllocationStatus::Unavailable {
            return if self.steps.is_empty() {
                Err(BhsDirectAllocationError::Unavailable)
            } else {
                Err(BhsDirectAllocationError::UnavailableWithSteps)
            };
        }

        let [BhsDirectAllocationStep::Nearby(nearby), BhsDirectAllocationStep::InitUnit(init)] =
            self.steps.as_slice()
        else {
            return Err(BhsDirectAllocationError::WrongStepShape);
        };
        if nearby.request != expected.nearby() {
            return Err(BhsDirectAllocationError::NearbyRequestMismatch);
        }

        // Load-bearing retail quirk: no predicate on `nearby.returned` belongs here.
        let init_request = expected.init_unit(nearby.output_x, nearby.output_y);
        if init.request != init_request {
            return Err(BhsDirectAllocationError::InitRequestMismatch);
        }
        if !init.validates(init_request) {
            return Err(BhsDirectAllocationError::InvalidInitExtent);
        }
        Ok(init.effects.returned_captain_or_failure)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BhsDirectAllocationError {
    ReceiptRequestMismatch,
    Unavailable,
    UnavailableWithSteps,
    WrongStepShape,
    NearbyRequestMismatch,
    InitRequestMismatch,
    InvalidInitExtent,
}

/// The smallest honest mutation owner for one direct BHS attempt. Implementations own the
/// complete nearby call, complete `Objects::init_unit`, its RNG consumption, owner bands,
/// Leader counters, Unit columns, linkage, collision/location updates, and partial failures.
pub trait BhsDirectAllocationAuthority {
    fn execute_bhs_direct_allocation(
        &mut self,
        request: BhsDirectAllocationRequest,
    ) -> BhsDirectAllocationReceipt;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BhsDirectBatchStatus {
    Completed,
    AuthorityFault {
        attempt: u32,
        error: BhsDirectAllocationError,
    },
}

/// Evidence retained by the future runtime adapter. `retail_return` exists only when every
/// requested attempt crossed the authority boundary. Valid negative results remain ordinary
/// native results and do not stop the loop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BhsDirectBatchExecution {
    pub status: BhsDirectBatchStatus,
    pub validated_receipts: Vec<BhsDirectAllocationReceipt>,
    pub observed_fault: Option<BhsDirectAllocationReceipt>,
    pub init_results: Vec<i32>,
    pub published_captains: Vec<i32>,
    pub retail_return: Option<i32>,
}

/// Execute only the repeated direct allocation/publish spine.
///
/// `publish_success` is the immediate local-Group plus persistent numeric-group publication
/// seam. Air/Carrier suffixes and the final Groups push/form path remain outside this owner.
pub fn execute_bhs_direct_batch(
    authority: &mut impl BhsDirectAllocationAuthority,
    count: u32,
    owner: i32,
    graft_type: i32,
    origin_x: i32,
    origin_y: i32,
    mut publish_success: impl FnMut(i32),
) -> BhsDirectBatchExecution {
    let mut validated_receipts = Vec::with_capacity(count as usize);
    let mut init_results = Vec::with_capacity(count as usize);
    let mut published_captains = Vec::new();

    for attempt in 0..count {
        let request = BhsDirectAllocationRequest {
            attempt,
            owner,
            graft_type,
            origin_x,
            origin_y,
        };
        let receipt = authority.execute_bhs_direct_allocation(request);
        let result = match receipt.validate(request) {
            Ok(result) => result,
            Err(error) => {
                return BhsDirectBatchExecution {
                    status: BhsDirectBatchStatus::AuthorityFault { attempt, error },
                    validated_receipts,
                    observed_fault: Some(receipt),
                    init_results,
                    published_captains,
                    retail_return: None,
                };
            }
        };

        if result >= 0 {
            publish_success(result);
            published_captains.push(result);
        }
        init_results.push(result);
        validated_receipts.push(receipt);
    }

    BhsDirectBatchExecution {
        status: BhsDirectBatchStatus::Completed,
        validated_receipts,
        observed_fault: None,
        retail_return: Some(init_results.last().copied().unwrap_or(-1)),
        init_results,
        published_captains,
    }
}
