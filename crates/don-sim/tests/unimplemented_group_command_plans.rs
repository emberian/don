//! Path-import pins for the maximal still-unimplemented group-opcode dispatcher cohort.
//!
//! The subject remains deliberately unexported: these tests freeze the later integration
//! contract without changing the shared command dispatcher or generated closure tables.

#[path = "../src/systems/unimplemented_group_command_plans.rs"]
mod subject;

use subject::*;

fn wire(opcode: u8, fields: &[i32]) -> Vec<u8> {
    let mut bytes = vec![opcode];
    for field in fields {
        bytes.extend_from_slice(&field.to_le_bytes());
    }
    bytes
}

fn facts(group_index: i32) -> UnimplementedGroupCommandFacts {
    UnimplementedGroupCommandFacts {
        group_index,
        addressed_object_flag_1: None,
    }
}

#[test]
fn mutation_exact_cohort_and_wire_advances_are_frozen() {
    assert_eq!(FRONTIER_OPCODES, [5, 6, 23, 24, 25, 28, 35]);

    let cases = [
        (wire(5, &[1, 2, 3]), 13),
        (wire(6, &[1, 2, 3, 4]), 17),
        (wire(23, &[1, 2, 3, 4, 5]), 21),
        (wire(24, &[1, 2]), 9),
        (wire(25, &[1, 2, 3, 4, 5, 6]), 25),
        (wire(28, &[1, 2, 3, 4, 5, 6]), 25),
        (wire(35, &[]), 1),
    ];
    for (bytes, expected_advance) in cases {
        let request = decode_unimplemented_group_command(&bytes).unwrap();
        assert_eq!(request.opcode(), bytes[0]);
        assert_eq!(request.wire_bytes(), expected_advance);

        let mut short = bytes.clone();
        short.pop();
        assert!(decode_unimplemented_group_command(&short).is_none());
        let mut long = bytes;
        long.push(0);
        assert!(decode_unimplemented_group_command(&long).is_none());
    }
    assert!(decode_unimplemented_group_command(&[0]).is_none());
    assert!(decode_unimplemented_group_command(&[]).is_none());
}

#[test]
fn mutation_decode_preserves_raw_signed_domains() {
    assert_eq!(
        decode_unimplemented_group_command(&wire(5, &[i32::MIN, -1, i32::MAX])),
        Some(UnimplementedGroupCommandRequest::SiegeAttack {
            ox: i32::MIN,
            whom: -1,
            queued: i32::MAX,
        })
    );
    assert_eq!(
        decode_unimplemented_group_command(&wire(25, &[i32::MIN, -2, -1, 0, 1, i32::MAX],)),
        Some(UnimplementedGroupCommandRequest::Build {
            x: i32::MIN,
            y: -2,
            x2: -1,
            y2: 0,
            type_index: 1,
            queued: i32::MAX,
        })
    );
}

#[test]
fn mutation_action_abi_reorders_spell_flight_and_swarm_fields() {
    let mut target_facts = facts(17);
    target_facts.addressed_object_flag_1 = Some(true);

    let spell = decode_unimplemented_group_command(&wire(23, &[10, 20, 30, 40, 50])).unwrap();
    assert_eq!(
        plan_unimplemented_group_command(spell, &target_facts)
            .unwrap()
            .delegate
            .unwrap()
            .call,
        GroupActionCall::Spell {
            type_index: 30,
            ox: 10,
            whom: 20,
            x: 40,
            y: 50,
        }
    );

    let flight = decode_unimplemented_group_command(&wire(28, &[1, 2, 3, 4, 5, 6])).unwrap();
    assert_eq!(
        plan_unimplemented_group_command(flight, &facts(17))
            .unwrap()
            .delegate
            .unwrap()
            .call,
        GroupActionCall::Flight {
            ox: 1,
            whom: 2,
            orders: 6,
            shift: 3,
            ctrl: 4,
            alt: 5,
        }
    );

    let swarm = decode_unimplemented_group_command(&wire(6, &[1, 2, 3, 4])).unwrap();
    assert_eq!(
        plan_unimplemented_group_command(swarm, &target_facts)
            .unwrap()
            .delegate
            .unwrap()
            .call,
        GroupActionCall::SwarmAround {
            ox: 1,
            whom: 2,
            queued: 3,
            orders: 4,
            retail_mode: SWARM_AROUND_RETAIL_MODE,
        }
    );
}

#[test]
fn mutation_direct_action_abi_preserves_siege_queue_build_and_recall() {
    let mut target_facts = facts(9);
    target_facts.addressed_object_flag_1 = Some(true);
    let cases = [
        (
            wire(5, &[1, 2, 3]),
            GroupActionCall::SiegeAttack {
                ox: 1,
                whom: 2,
                queued: 3,
            },
        ),
        (
            wire(24, &[4, 5]),
            GroupActionCall::QueueUp {
                type_index: 4,
                num: 5,
            },
        ),
        (
            wire(25, &[6, 7, 8, 9, 10, 11]),
            GroupActionCall::Build {
                x: 6,
                y: 7,
                x2: 8,
                y2: 9,
                type_index: 10,
                queued: 11,
            },
        ),
        (wire(35, &[]), GroupActionCall::Recall),
    ];
    for (bytes, expected) in cases {
        let request = decode_unimplemented_group_command(&bytes).unwrap();
        assert_eq!(
            plan_unimplemented_group_command(request, &target_facts)
                .unwrap()
                .delegate
                .unwrap()
                .call,
            expected,
        );
    }
}

#[test]
fn mutation_signed_group_gate_skips_every_delegate() {
    let requests = [
        wire(5, &[-1, -1, 0]),
        wire(6, &[-1, -1, 0, 0]),
        wire(23, &[-1, -1, 0, 0, 0]),
        wire(24, &[0, 0]),
        wire(25, &[0, 0, 0, 0, 0, 0]),
        wire(28, &[0, 0, 0, 0, 0, 0]),
        wire(35, &[]),
    ];
    for bytes in requests {
        let request = decode_unimplemented_group_command(&bytes).unwrap();
        let plan = plan_unimplemented_group_command(request, &facts(-1)).unwrap();
        assert_eq!(plan.delegate, None);
        assert!(!plan.downstream_required);
    }
}

#[test]
fn mutation_target_object_gate_is_branch_sensitive_and_fail_closed() {
    let positive_cases = [
        wire(5, &[4, 5, 6]),
        wire(6, &[4, 5, 6, 7]),
        wire(23, &[4, 5, 6, 7, 8]),
    ];
    for bytes in positive_cases {
        let request = decode_unimplemented_group_command(&bytes).unwrap();
        assert!(plan_unimplemented_group_command(request, &facts(0)).is_none());

        let mut target_facts = facts(0);
        target_facts.addressed_object_flag_1 = Some(false);
        let blocked = plan_unimplemented_group_command(request, &target_facts).unwrap();
        assert_eq!(blocked.delegate, None);
        assert!(!blocked.downstream_required);

        target_facts.addressed_object_flag_1 = Some(true);
        let reached = plan_unimplemented_group_command(request, &target_facts).unwrap();
        assert!(reached.delegate.is_some());
        assert!(reached.downstream_required);
    }

    let sentinel_cases = [
        wire(5, &[-1, 5, 6]),
        wire(5, &[4, -1, 6]),
        wire(6, &[-1, 5, 6, 7]),
        wire(6, &[4, -1, 6, 7]),
        wire(23, &[-1, 5, 6, 7, 8]),
        wire(23, &[4, -1, 6, 7, 8]),
    ];
    for bytes in sentinel_cases {
        let request = decode_unimplemented_group_command(&bytes).unwrap();
        let sentinel_plan = plan_unimplemented_group_command(request, &facts(0)).unwrap();
        assert!(sentinel_plan.delegate.is_some());
    }

    let non_target = decode_unimplemented_group_command(&wire(24, &[4, 5])).unwrap();
    let non_target_plan = plan_unimplemented_group_command(non_target, &facts(0)).unwrap();
    assert!(non_target_plan.delegate.is_some());
}

#[test]
fn mutation_every_reached_row_retains_an_open_typed_tail() {
    let cases = [
        wire(5, &[-1, -1, 0]),
        wire(6, &[-1, -1, 0, 0]),
        wire(23, &[-1, -1, 0, 0, 0]),
        wire(24, &[0, 0]),
        wire(25, &[0, 0, 0, 0, 0, 0]),
        wire(28, &[0, 0, 0, 0, 0, 0]),
        wire(35, &[]),
    ];
    for bytes in cases {
        let request = decode_unimplemented_group_command(&bytes).unwrap();
        let plan = plan_unimplemented_group_command(request, &facts(0)).unwrap();
        assert!(plan.delegate.is_some());
        assert!(plan.downstream_required);
    }
}

#[test]
fn mutation_receipt_recomputes_and_rejects_changed_delegate() {
    let request = decode_unimplemented_group_command(&wire(24, &[7, 11])).unwrap();
    let facts = facts(3);
    let plan = plan_unimplemented_group_command(request, &facts).unwrap();
    let receipt = UnimplementedGroupCommandReceipt {
        request,
        facts: Some(facts),
        status: PlanStatus::Planned,
        plan: Some(plan),
    };
    assert!(receipt.validates(request));

    let mut mutated = receipt;
    mutated
        .plan
        .as_mut()
        .unwrap()
        .delegate
        .as_mut()
        .unwrap()
        .group_index ^= 1;
    assert!(!mutated.validates(request));

    assert!(UnimplementedGroupCommandReceipt::unavailable(request).validates(request));
    assert!(!UnimplementedGroupCommandReceipt::unavailable(request)
        .validates(UnimplementedGroupCommandRequest::Recall,));
}
