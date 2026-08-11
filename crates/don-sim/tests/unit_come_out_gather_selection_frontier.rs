// Registered by lane `come-out` on 2026-08-11: this test used to `#[path]`-include the
// module source, which compiled it standalone and proved nothing about the library. It
// now drives the module through `don_sim::systems`, so it fails if the `pub mod` line
// is ever removed again.
use don_sim::systems::unit_come_out_gather_selection_frontier::*;

fn stamp(seed: u32, draws: u64) -> RngStamp {
    RngStamp { seed, draws }
}

fn input() -> GatherSelectionInput {
    GatherSelectionInput {
        resume_va: GATHER_SELECTION_START_VA,
        actor: ObjectIdentity::new(2, 41),
        actor_point: Point { x: 100, y: 200 },
        direct_container: ObjectIdentity::new(2, 9),
        container_gpiece: 77,
        scratch_group: None,
        rng: stamp(11, 20),
    }
}

fn actor() -> ActorSelectionFacts {
    ActorSelectionFacts {
        attack: 0,
        uber_size: 1,
        is_captain_base_slot: true,
        captain_bit: true,
    }
}

fn point(x: i32, y: i32, action: u8) -> GatherPointFacts {
    GatherPointFacts {
        point: Point { x, y },
        action,
        building: None,
        advance_head_present: false,
    }
}

fn receipt(kind: HostCallKind, result: HostReturn) -> HostCallReceipt {
    HostCallReceipt {
        kind,
        result,
        before: stamp(11, 20),
        after: stamp(11, 20),
    }
}

fn initial(count: i32) -> Vec<HostCallReceipt> {
    vec![
        receipt(HostCallKind::GatherInside, HostReturn::I32(0)),
        receipt(HostCallKind::ActorOrderType, HostReturn::I32(0)),
        receipt(HostCallKind::NumGather, HostReturn::I32(count)),
        receipt(HostCallKind::GatherHead, HostReturn::VoidOrIgnored),
    ]
}

fn facts(points: Vec<GatherPointFacts>, receipts: Vec<HostCallReceipt>) -> GatherSelectionFacts {
    GatherSelectionFacts {
        input: input(),
        actor: actor(),
        has_gather_head: true,
        points,
        host_receipts: receipts,
    }
}

#[test]
fn constants_measure_the_complete_selection_machine() {
    assert_eq!(SEQUENTIAL_BYTES, 1_667);
    assert_eq!(OUTLINED_BYTES, 16);
    assert_eq!(LOGICAL_TRANCHE_BYTES, 1_683);
    assert_eq!(RESIDUAL_BYTES_AFTER_TRANCHE, 4_349);
    assert_eq!(OUTLINED_VIRTUAL_ISLANDS[1], (0x0061_a278, 0x0061_a281));
    assert_eq!(GATHER_PROBE_RADIUS, 0x600);
    assert_eq!(GATHER_PROBE_ANGLE, 0x5555_5555);
    assert_eq!(UNIVERSITY_TYPE, 0x1a4);
}

#[test]
fn empty_head_uses_the_shipped_gpiece_square_fallback() {
    let mut value = facts(Vec::new(), Vec::new());
    value.has_gather_head = false;
    let plan = plan_unit_come_out_gather_selection(&value).unwrap();
    assert_eq!(plan.continuation.selected, Point { x: 77, y: 77 });
    assert!(!plan.continuation.terminal_selection);
    assert_eq!(plan.continuation.action, 0);
    assert!(matches!(
        plan.steps.as_slice(),
        [GatherSelectionStep::SetExhaustedFallback {
            flag_store_va: 0x0061_9195,
            ..
        }]
    ));
}

#[test]
fn one_entry_is_terminal_before_action_or_building_interpretation() {
    let only = point(300, 400, ATTACK_ACTION);
    let value = facts(vec![only], initial(1));
    let plan = plan_unit_come_out_gather_selection(&value).unwrap();
    assert_eq!(plan.continuation.selected, only.point);
    assert_eq!(plan.continuation.action, ATTACK_ACTION);
    assert!(plan.continuation.terminal_selection);
    assert_eq!(plan.steps.len(), 5);
}

#[test]
fn individual_move_then_valid_garrison_then_last_fallback_preserves_order() {
    let mut p0 = point(300, 400, 0);
    p0.advance_head_present = true;
    let mut p1 = point(500, 600, GARRISON_ACTION);
    p1.advance_head_present = true;
    p1.building = Some(BuildingDirectFacts {
        identity: WideObjectIdentity {
            owner: 2,
            object: 12,
        },
        myhits: 0,
        type_index: 99,
        gather_max: 4,
        university_is_base_slot: true,
    });
    let p2 = point(700, 800, ATTACK_ACTION);
    let mut receipts = initial(3);
    receipts.extend([
        receipt(
            HostCallKind::FindMoveAngle {
                point: 0,
                group: false,
            },
            HostReturn::I32(1),
        ),
        receipt(
            HostCallKind::ActorAddMoveFacingOrder { point: 0 },
            HostReturn::VoidOrIgnored,
        ),
        receipt(
            HostCallKind::TypeFindNearbySpot { point: 1 },
            HostReturn::Search {
                code: 0,
                point: Point { x: 510, y: 610 },
            },
        ),
        receipt(
            HostCallKind::FindAnyBuildingAt { point: 1 },
            HostReturn::Building(WideObjectIdentity {
                owner: 2,
                object: 12,
            }),
        ),
        receipt(
            HostCallKind::ActorIsPeasant { point: 1 },
            HostReturn::I32(0),
        ),
        receipt(
            HostCallKind::CandidateIsActiveBuild { point: 1 },
            HostReturn::I32(1),
        ),
        receipt(
            HostCallKind::CandidateOwnerIsAlly { point: 1 },
            HostReturn::I32(1),
        ),
        receipt(
            HostCallKind::CandidateGarrisonLimitGate { point: 1 },
            HostReturn::I32(5),
        ),
        receipt(
            HostCallKind::ActorCanGarrison {
                point: 1,
                type_index: 99,
            },
            HostReturn::I32(1),
        ),
        receipt(
            HostCallKind::CandidateGarrisonLimitCapacity { point: 1 },
            HostReturn::I32(5),
        ),
        receipt(
            HostCallKind::CandidateNumInside { point: 1 },
            HostReturn::I32(2),
        ),
        receipt(
            HostCallKind::ActorControlCost { point: 1 },
            HostReturn::I32(2),
        ),
        receipt(
            HostCallKind::TypeFindNearbySpot { point: 2 },
            HostReturn::Search {
                code: 0,
                point: Point { x: 710, y: 810 },
            },
        ),
    ]);
    let value = facts(vec![p0, p1, p2], receipts);
    let plan = plan_unit_come_out_gather_selection(&value).unwrap();
    assert_eq!(plan.continuation.selected, Point { x: 700, y: 800 });
    assert!(plan.steps.iter().any(|step| matches!(
        step,
        GatherSelectionStep::SetMovementBaseline {
            point: 0,
            before: Point { x: 100, y: 200 },
            after: Point { x: 300, y: 400 },
            ..
        }
    )));
    assert_eq!(
        plan.steps
            .iter()
            .filter(|step| matches!(step, GatherSelectionStep::AdvanceGatherCursor { .. }))
            .count(),
        2
    );
}

#[test]
fn full_allied_garrison_selects_the_raw_point() {
    let mut p0 = point(500, 600, GARRISON_ACTION);
    p0.building = Some(BuildingDirectFacts {
        identity: WideObjectIdentity {
            owner: 2,
            object: 12,
        },
        myhits: 0,
        type_index: 99,
        gather_max: 4,
        university_is_base_slot: true,
    });
    let p1 = point(700, 800, 0);
    let mut receipts = initial(2);
    receipts.extend([
        receipt(
            HostCallKind::FindAnyBuildingAt { point: 0 },
            HostReturn::Building(WideObjectIdentity {
                owner: 2,
                object: 12,
            }),
        ),
        receipt(
            HostCallKind::ActorIsPeasant { point: 0 },
            HostReturn::I32(0),
        ),
        receipt(
            HostCallKind::CandidateIsActiveBuild { point: 0 },
            HostReturn::I32(1),
        ),
        receipt(
            HostCallKind::CandidateOwnerIsAlly { point: 0 },
            HostReturn::I32(1),
        ),
        receipt(
            HostCallKind::CandidateGarrisonLimitGate { point: 0 },
            HostReturn::I32(3),
        ),
        receipt(
            HostCallKind::ActorCanGarrison {
                point: 0,
                type_index: 99,
            },
            HostReturn::I32(1),
        ),
        receipt(
            HostCallKind::CandidateGarrisonLimitCapacity { point: 0 },
            HostReturn::I32(3),
        ),
        receipt(
            HostCallKind::CandidateNumInside { point: 0 },
            HostReturn::I32(3),
        ),
        receipt(
            HostCallKind::ActorControlCost { point: 0 },
            HostReturn::I32(1),
        ),
    ]);
    let value = facts(vec![p0, p1], receipts);
    let plan = plan_unit_come_out_gather_selection(&value).unwrap();
    assert_eq!(plan.continuation.selected, p0.point);
    assert_eq!(plan.continuation.action, GARRISON_ACTION);
}

#[test]
fn university_exclusion_uses_the_outlined_is_dispatch_and_selects() {
    let mut p0 = point(500, 600, GARRISON_ACTION);
    p0.building = Some(BuildingDirectFacts {
        identity: WideObjectIdentity {
            owner: 2,
            object: 12,
        },
        myhits: 0,
        type_index: UNIVERSITY_TYPE,
        gather_max: 4,
        university_is_base_slot: false,
    });
    let p1 = point(700, 800, 0);
    let mut receipts = initial(2);
    receipts.extend([
        receipt(
            HostCallKind::FindAnyBuildingAt { point: 0 },
            HostReturn::Building(WideObjectIdentity {
                owner: 2,
                object: 12,
            }),
        ),
        receipt(
            HostCallKind::ActorIsPeasant { point: 0 },
            HostReturn::I32(1),
        ),
        receipt(
            HostCallKind::CandidateIsWallBuild { point: 0 },
            HostReturn::I32(1),
        ),
        receipt(
            HostCallKind::CandidateIsActive { point: 0 },
            HostReturn::I32(1),
        ),
        receipt(
            HostCallKind::CandidateIsValidWall { point: 0 },
            HostReturn::I32(1),
        ),
        receipt(
            HostCallKind::CandidateWallTypeIsGatherType { point: 0 },
            HostReturn::I32(0),
        ),
        receipt(
            HostCallKind::CandidateMatchesUniversity {
                point: 0,
                base_slot: false,
            },
            HostReturn::I32(1),
        ),
        receipt(
            HostCallKind::CandidateIsActiveBuild { point: 0 },
            HostReturn::I32(0),
        ),
    ]);
    let value = facts(vec![p0, p1], receipts);
    let plan = plan_unit_come_out_gather_selection(&value).unwrap();
    assert_eq!(plan.continuation.selected, p0.point);
    assert!(plan.steps.iter().any(|step| matches!(
        step,
        GatherSelectionStep::HostCall {
            call_va: 0x0061_a271,
            ..
        }
    )));
}

#[test]
fn overridden_non_captain_recheck_advances_without_emitting_a_move_order() {
    let mut p0 = point(300, 400, 0);
    p0.advance_head_present = true;
    let p1 = point(700, 800, 0);
    let mut value = facts(vec![p0, p1], {
        let mut receipts = initial(2);
        receipts.extend([
            receipt(
                HostCallKind::ActorIsCaptain { point: 0 },
                HostReturn::I32(0),
            ),
            receipt(
                HostCallKind::TypeFindNearbySpot { point: 1 },
                HostReturn::Search {
                    code: 0,
                    point: p1.point,
                },
            ),
        ]);
        receipts
    });
    value.actor.is_captain_base_slot = false;
    let plan = plan_unit_come_out_gather_selection(&value).unwrap();
    assert!(plan.steps.iter().any(|step| matches!(
        step,
        GatherSelectionStep::HostCall {
            call_va: 0x0061_a27a,
            ..
        }
    )));
    assert!(!plan.steps.iter().any(|step| matches!(
        step,
        GatherSelectionStep::HostCall {
            receipt: HostCallReceipt {
                kind: HostCallKind::ActorAddMoveFacingOrder { .. },
                ..
            },
            ..
        }
    )));
}

#[test]
fn attack_point_without_building_skips_for_non_worker_attacker() {
    let mut p0 = point(500, 600, ATTACK_ACTION);
    p0.advance_head_present = true;
    let p1 = point(700, 800, 0);
    let mut value = facts(vec![p0, p1], {
        let mut receipts = initial(2);
        receipts.extend([
            receipt(
                HostCallKind::FindAnyBuildingAt { point: 0 },
                HostReturn::Building(WideObjectIdentity {
                    owner: 0,
                    object: -1,
                }),
            ),
            receipt(
                HostCallKind::ActorIsWorker {
                    point: 0,
                    building_found: false,
                },
                HostReturn::I32(0),
            ),
            receipt(
                HostCallKind::TypeFindNearbySpot { point: 1 },
                HostReturn::Search {
                    code: 0,
                    point: p1.point,
                },
            ),
        ]);
        receipts
    });
    value.actor.attack = 8;
    let plan = plan_unit_come_out_gather_selection(&value).unwrap();
    assert_eq!(plan.continuation.selected, p1.point);
}

#[test]
fn scratch_group_movement_uses_authoritative_search_output() {
    let mut p0 = point(300, 400, 0);
    p0.advance_head_present = true;
    let p1 = point(700, 800, 0);
    let mut value = facts(vec![p0, p1], {
        let mut receipts = initial(2);
        receipts.extend([
            receipt(
                HostCallKind::ActorFindNearbySpot { point: 0 },
                HostReturn::Search {
                    code: 1,
                    point: Point { x: 333, y: 444 },
                },
            ),
            receipt(
                HostCallKind::FindMoveAngle {
                    point: 0,
                    group: true,
                },
                HostReturn::I32(12),
            ),
            receipt(
                HostCallKind::ScratchGroupMove { point: 0 },
                HostReturn::VoidOrIgnored,
            ),
            receipt(
                HostCallKind::TypeFindNearbySpot { point: 1 },
                HostReturn::Search {
                    code: 1,
                    point: p1.point,
                },
            ),
        ]);
        receipts
    });
    value.actor.uber_size = 3;
    value.input.scratch_group = Some(17);
    let plan = plan_unit_come_out_gather_selection(&value).unwrap();
    assert!(plan.steps.iter().any(|step| matches!(
        step,
        GatherSelectionStep::SetMovementBaseline {
            after: Point { x: 333, y: 444 },
            ..
        }
    )));
    assert_eq!(plan.continuation.selected, Point { x: 77, y: 77 });
}

#[test]
fn wrong_receipt_and_unreachable_payload_fail_closed() {
    let only = point(1, 2, 0);
    let mut wrong = facts(vec![only], initial(1));
    wrong.host_receipts[0].kind = HostCallKind::ActorOrderType;
    assert!(matches!(
        plan_unit_come_out_gather_selection(&wrong),
        Err(GatherSelectionError::ReceiptKind { .. })
    ));

    let mut discontinuous = facts(vec![only], initial(1));
    discontinuous.host_receipts[0].before = stamp(12, 20);
    assert_eq!(
        plan_unit_come_out_gather_selection(&discontinuous),
        Err(GatherSelectionError::ReceiptContinuity)
    );

    let mut payload = point(1, 2, 0);
    payload.building = Some(BuildingDirectFacts {
        identity: WideObjectIdentity {
            owner: 2,
            object: 3,
        },
        myhits: 0,
        type_index: 1,
        gather_max: 1,
        university_is_base_slot: true,
    });
    let value = facts(vec![payload], initial(1));
    assert!(matches!(
        plan_unit_come_out_gather_selection(&value),
        Err(GatherSelectionError::Unexpected(_))
    ));
}
