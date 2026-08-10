#[path = "../src/systems/bhs_create_unit_allocation_tail_frontier.rs"]
mod frontier;

use std::collections::VecDeque;

use frontier::*;

fn request(attempt: u32) -> BhsDirectAllocationRequest {
    BhsDirectAllocationRequest {
        attempt,
        owner: 2,
        graft_type: 150,
        origin_x: 3_936,
        origin_y: 2_400,
    }
}

fn applied(
    request: BhsDirectAllocationRequest,
    nearby_returned: i32,
    output_x: i32,
    output_y: i32,
    effects: ObjectsInitUnitEffects,
) -> BhsDirectAllocationReceipt {
    let nearby = BhsNearbyReceipt {
        request: request.nearby(),
        returned: nearby_returned,
        output_x,
        output_y,
    };
    let init_request = request.init_unit(output_x, output_y);
    BhsDirectAllocationReceipt {
        request,
        status: BhsDirectAllocationStatus::Applied,
        steps: vec![
            BhsDirectAllocationStep::Nearby(nearby),
            BhsDirectAllocationStep::InitUnit(BhsObjectsInitUnitReceipt {
                request: init_request,
                extent: ObjectsInitUnitExtent::CompleteRetail1603ByteBody,
                effects,
            }),
        ],
    }
}

fn success(captain: i32, members: &[i32]) -> ObjectsInitUnitEffects {
    ObjectsInitUnitEffects {
        required_members: members.len() as u32,
        initialized_members: members.to_vec(),
        terminal_find_free_failure: None,
        returned_captain_or_failure: captain,
    }
}

#[test]
fn native_calls_abi_and_offsets_are_frozen() {
    assert_eq!(ADD_UNIT_VA, 0x009e_2220);
    assert_eq!(ADD_UNIT_NEARBY_CALL_VA, 0x009e_23f4);
    assert_eq!(ADD_UNIT_INIT_UNIT_CALL_VA, 0x009e_2412);
    assert_eq!(UNIT_TYPE_FIND_NEARBY_SPOT_VA, 0x0061_de70);
    assert_eq!(UNIT_TYPE_FIND_NEARBY_SPOT_BYTES, 1_433);
    assert_eq!(OBJECTS_INIT_UNIT_VA, 0x0065_e0c0);
    assert_eq!(OBJECTS_INIT_UNIT_BYTES, 1_603);
    assert_eq!(OBJECTS_FIND_FREE_VA, 0x0065_ad60);
    assert_eq!(OBJECTS_FIND_FREE_CALL_VA, 0x0065_e137);
    assert_eq!(OBJECT_INIT_VTABLE_OFFSET, 0x8c);
    assert_eq!(UNIT_TYPE_UBER_SIZE_OFFSET, 0x308);
    assert_eq!(UNIT_LINK_PREVIOUS_OFFSET, 0x8e);
    assert_eq!(UNIT_LINK_NEXT_OFFSET, 0x90);
    assert_eq!(ADD_UNIT_INIT_TAIL, [-1; 3]);
}

#[test]
fn request_uses_zero_based_owner_graft_type_and_exact_nearby_tail() {
    let request = request(7);
    assert_eq!(
        request.nearby(),
        BhsNearbyRequest {
            attempt: 7,
            type_index: 150,
            origin_x: 3_936,
            origin_y: 2_400,
            tail: [0, 0xc00, 0, 0x5555_5555, 3, -1, -1, 0, 0, -1, 0, -1,],
        }
    );
    assert_eq!(
        request.init_unit(4_032, 2_496),
        BhsObjectsInitUnitRequest {
            attempt: 7,
            owner: 2,
            type_index: 150,
            x: 4_032,
            y: 2_496,
            tail: [-1; 3],
        }
    );
}

#[test]
fn nearby_failure_is_evidence_not_a_gate_and_outputs_feed_init() {
    let request = request(0);
    let receipt = applied(request, 1, -77, 88, success(41, &[40, 41]));
    assert_eq!(receipt.validate(request), Ok(41));

    let BhsDirectAllocationStep::Nearby(nearby) = &receipt.steps[0] else {
        panic!("nearby step");
    };
    assert_eq!(nearby.returned, 1);
    let BhsDirectAllocationStep::InitUnit(init) = &receipt.steps[1] else {
        panic!("init step");
    };
    assert_eq!((init.request.x, init.request.y), (-77, 88));
}

#[test]
fn receipt_rejects_type_position_extent_and_step_substitutions() {
    let request = request(0);
    assert_eq!(
        BhsDirectAllocationReceipt::unavailable(request).validate(request),
        Err(BhsDirectAllocationError::Unavailable)
    );

    let mut wrong_nearby = applied(request, 0, 4_032, 2_496, success(41, &[41]));
    let BhsDirectAllocationStep::Nearby(nearby) = &mut wrong_nearby.steps[0] else {
        unreachable!()
    };
    nearby.request.type_index = 149;
    assert_eq!(
        wrong_nearby.validate(request),
        Err(BhsDirectAllocationError::NearbyRequestMismatch)
    );

    let mut wrong_init = applied(request, 0, 4_032, 2_496, success(41, &[41]));
    let BhsDirectAllocationStep::InitUnit(init) = &mut wrong_init.steps[1] else {
        unreachable!()
    };
    init.request.x += 1;
    assert_eq!(
        wrong_init.validate(request),
        Err(BhsDirectAllocationError::InitRequestMismatch)
    );

    let bad_extent = applied(
        request,
        0,
        4_032,
        2_496,
        ObjectsInitUnitEffects {
            required_members: 2,
            initialized_members: vec![41],
            terminal_find_free_failure: None,
            returned_captain_or_failure: 41,
        },
    );
    assert_eq!(
        bad_extent.validate(request),
        Err(BhsDirectAllocationError::InvalidInitExtent)
    );

    let unavailable_with_steps = BhsDirectAllocationReceipt {
        status: BhsDirectAllocationStatus::Unavailable,
        ..applied(request, 0, 4_032, 2_496, success(41, &[41]))
    };
    assert_eq!(
        unavailable_with_steps.validate(request),
        Err(BhsDirectAllocationError::UnavailableWithSteps)
    );
}

#[test]
fn init_unit_partial_failure_is_valid_and_keeps_earlier_members_visible() {
    let effects = ObjectsInitUnitEffects {
        required_members: 3,
        initialized_members: vec![51, 52],
        terminal_find_free_failure: Some(-9),
        returned_captain_or_failure: -9,
    };
    assert!(effects.validates_complete_receiver());
    assert_eq!(
        applied(request(0), 0, 4_032, 2_496, effects).validate(request(0)),
        Ok(-9)
    );

    for invalid in [
        ObjectsInitUnitEffects {
            required_members: 3,
            initialized_members: vec![51, 51],
            terminal_find_free_failure: Some(-9),
            returned_captain_or_failure: -9,
        },
        ObjectsInitUnitEffects {
            required_members: 2,
            initialized_members: vec![51, 52],
            terminal_find_free_failure: Some(-9),
            returned_captain_or_failure: -9,
        },
        ObjectsInitUnitEffects {
            required_members: 2,
            initialized_members: vec![51, 52],
            terminal_find_free_failure: None,
            returned_captain_or_failure: 99,
        },
    ] {
        assert!(!invalid.validates_complete_receiver());
    }
}

struct QueueAuthority {
    replies: VecDeque<BhsDirectAllocationReceipt>,
    requests: Vec<BhsDirectAllocationRequest>,
}

impl BhsDirectAllocationAuthority for QueueAuthority {
    fn execute_bhs_direct_allocation(
        &mut self,
        request: BhsDirectAllocationRequest,
    ) -> BhsDirectAllocationReceipt {
        self.requests.push(request);
        self.replies.pop_front().expect("fixture reply")
    }
}

#[test]
fn batch_publishes_each_success_immediately_and_returns_the_last_attempt() {
    let partial_failure = ObjectsInitUnitEffects {
        required_members: 3,
        initialized_members: vec![61],
        terminal_find_free_failure: Some(-7),
        returned_captain_or_failure: -7,
    };
    let mut authority = QueueAuthority {
        replies: VecDeque::from([
            applied(request(0), 0, 4_032, 2_496, success(41, &[40, 41])),
            applied(request(1), 1, 4_128, 2_592, partial_failure),
        ]),
        requests: Vec::new(),
    };
    let mut published = Vec::new();
    let execution = execute_bhs_direct_batch(&mut authority, 2, 2, 150, 3_936, 2_400, |captain| {
        published.push(captain)
    });

    assert_eq!(execution.status, BhsDirectBatchStatus::Completed);
    assert_eq!(authority.requests, vec![request(0), request(1)]);
    assert_eq!(execution.init_results, vec![41, -7]);
    assert_eq!(execution.published_captains, vec![41]);
    assert_eq!(published, vec![41]);
    assert_eq!(execution.retail_return, Some(-7));
    assert_eq!(execution.validated_receipts.len(), 2);
}

#[test]
fn batch_fault_retains_prior_effect_evidence_and_never_invents_a_native_return() {
    let mut mismatched = request(1);
    mismatched.graft_type = 151;
    let mut authority = QueueAuthority {
        replies: VecDeque::from([
            applied(request(0), 0, 4_032, 2_496, success(41, &[41])),
            applied(mismatched, 0, 4_128, 2_592, success(42, &[42])),
        ]),
        requests: Vec::new(),
    };
    let mut published = Vec::new();
    let execution = execute_bhs_direct_batch(&mut authority, 2, 2, 150, 3_936, 2_400, |captain| {
        published.push(captain)
    });

    assert_eq!(
        execution.status,
        BhsDirectBatchStatus::AuthorityFault {
            attempt: 1,
            error: BhsDirectAllocationError::ReceiptRequestMismatch,
        }
    );
    assert_eq!(execution.init_results, vec![41]);
    assert_eq!(published, vec![41]);
    assert_eq!(execution.validated_receipts.len(), 1);
    assert!(execution.observed_fault.is_some());
    assert_eq!(execution.retail_return, None);
}

#[test]
fn zero_count_is_a_completed_empty_batch_with_native_minus_one_return() {
    let mut authority = QueueAuthority {
        replies: VecDeque::new(),
        requests: Vec::new(),
    };
    let execution = execute_bhs_direct_batch(&mut authority, 0, 2, 150, 3_936, 2_400, |_| {
        panic!("zero count cannot publish")
    });
    assert_eq!(execution.status, BhsDirectBatchStatus::Completed);
    assert_eq!(execution.retail_return, Some(-1));
    assert!(authority.requests.is_empty());
}
