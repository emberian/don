#[path = "../src/systems/unit_come_out_common_release_frontier.rs"]
mod subject;

use subject::*;

fn stamp(seed: u32, draws: u64) -> RngStamp {
    RngStamp { seed, draws }
}

fn receipt(
    kind: HostCallKind,
    result: HostReturn,
    before: RngStamp,
    after: RngStamp,
) -> HostCallReceipt {
    HostCallReceipt {
        kind,
        result,
        before,
        after,
    }
}

fn prefix() -> PrefixContinuation {
    PrefixContinuation {
        resume_va: COMMON_RELEASE_START_VA,
        actor: ObjectIdentity::new(2, 41),
        point: Point {
            x: 0x1a40,
            y: 0x2080,
        },
        z: None,
        direct_container: None,
        placement_container: None,
        container_gpiece: 0,
        rng: stamp(7, 10),
    }
}

fn actor() -> ActorReleaseFacts {
    ActorReleaseFacts {
        unit_masks: PATHING_UNIT_MASK | 0x123,
        unit_masks2: 0,
        domain: 0,
        unit_type_flags: 0,
        type_index: 77,
        is_plane_base_slot: true,
        is_plane: false,
        is_captain: false,
        matches_nuke_family: false,
        uber_size: 1,
        path_length: 4,
        leader_flags: LEADER_COMMAND_FLAG,
        nukes_launched_before: 6,
    }
}

fn base() -> CommonReleaseFacts {
    CommonReleaseFacts {
        prefix: prefix(),
        actor: actor(),
        first_guy: None,
        down_unit: None,
        order_head: None,
        captain_container: None,
        host_receipts: vec![receipt(
            HostCallKind::SetNewLocation,
            HostReturn::I32(1),
            stamp(7, 10),
            stamp(7, 10),
        )],
    }
}

#[test]
fn constants_measure_the_disjoint_second_tranche() {
    assert_eq!(SEQUENTIAL_BYTES, 1_134);
    assert_eq!(OUTLINED_BYTES, 35);
    assert_eq!(LOGICAL_TRANCHE_BYTES, 1_169);
    assert_eq!(RESIDUAL_BYTES_AFTER_TRANCHE, 6_032);
    assert_eq!(OUTLINED_VIRTUAL_ISLANDS[0], (0x0061_a24e, 0x0061_a25f));
    assert_eq!(NUKE_FAMILY_TYPE, 0x13b);
}

#[test]
fn ordinary_non_captain_reaches_fallback_with_one_atomic_host_receipt() {
    let facts = base();
    let plan = plan_unit_come_out_common_release(&facts).unwrap();
    assert_eq!(plan.steps.len(), 1);
    assert!(matches!(
        plan.exit,
        CommonReleaseExit::FallbackSetup(CommonReleaseContinuation {
            resume_va: FALLBACK_SETUP_RESUME_VA,
            scratch_group: None,
            rng: RngStamp { seed: 7, draws: 10 },
            ..
        })
    ));
}

#[test]
fn overridden_plane_slot_uses_only_the_override_result_for_cleanup() {
    let mut facts = base();
    facts.actor.leader_flags = 0;
    facts.actor.is_plane_base_slot = false;
    facts.actor.is_plane = true;
    facts.actor.domain = 0;
    facts.actor.unit_type_flags = PLANE_EXIT_FLAG;
    let plan = plan_unit_come_out_common_release(&facts).unwrap();
    assert!(!plan
        .steps
        .iter()
        .any(|step| matches!(step, CommonReleaseStep::ClearPathingMask { .. })));
}

#[test]
fn null_current_order_data_keeps_duplicate_cursor_stores_without_strafe_call() {
    let mut facts = base();
    facts.actor.matches_nuke_family = true;
    facts.order_head = Some(OrderHeadFacts {
        metric: 4,
        current_data_present: false,
    });
    let plan = plan_unit_come_out_common_release(&facts).unwrap();
    assert!(plan.steps.iter().any(|step| matches!(
        step,
        CommonReleaseStep::SelectOrderHead {
            first_node_store_va: 0x0061_887f,
            repeat_node_store_va: 0x0061_88ba,
            metric: 4,
            ..
        }
    )));
    assert!(!plan
        .steps
        .iter()
        .any(|step| matches!(step, CommonReleaseStep::IncrementNukesLaunched { .. })));
}

#[test]
fn captain_inside_non_build_uses_the_same_fallback_seam() {
    let mut facts = base();
    facts.actor.is_captain = true;
    facts.prefix.direct_container = Some(ObjectIdentity::new(2, 12));
    facts.captain_container = Some(CaptainContainerFacts {
        identity: ObjectIdentity::new(2, 12),
        kind: CaptainContainerKind::Other,
    });
    let plan = plan_unit_come_out_common_release(&facts).unwrap();
    assert!(matches!(
        plan.exit,
        CommonReleaseExit::FallbackSetup(CommonReleaseContinuation {
            resume_va: FALLBACK_SETUP_RESUME_VA,
            ..
        })
    ));
}

#[test]
fn rich_captain_build_path_preserves_host_and_rng_chronology() {
    let mut facts = base();
    facts.prefix.direct_container = Some(ObjectIdentity::new(2, 9));
    facts.actor = ActorReleaseFacts {
        unit_masks2: CEO_POSITION_MASK,
        domain: 2,
        unit_type_flags: PLANE_EXIT_FLAG,
        type_index: 0x34,
        is_plane: true,
        is_captain: true,
        matches_nuke_family: true,
        uber_size: 3,
        leader_flags: 0,
        ..actor()
    };
    facts.first_guy = Some(FirstGuyFacts {
        z_after_update: 900,
        last_z_before: 875,
    });
    facts.down_unit = Some(DownUnitFacts {
        identity: ObjectIdentity::new(2, 15),
        flags_low: ACTIVE_OBJECT_FLAG,
    });
    facts.order_head = Some(OrderHeadFacts {
        metric: 11,
        current_data_present: true,
    });
    facts.captain_container = Some(CaptainContainerFacts {
        identity: ObjectIdentity::new(2, 9),
        kind: CaptainContainerKind::Build(BuildExitFacts {
            flags_low: ACTIVE_BUILD_FLAG,
            inside_down: -1,
            city_index: 4,
            city_flags_before: Some(CITY_WORKER_FLAG | 3),
            owner_object_slots: 8,
            worker_scan: vec![
                WorkerSlotFacts {
                    active: false,
                    is_worker: false,
                    action_type: None,
                },
                WorkerSlotFacts {
                    active: true,
                    is_worker: true,
                    action_type: Some(WORKER_EXIT_ACTION),
                },
            ],
        }),
    });

    let kinds = [
        (HostCallKind::SetNewLocation, HostReturn::I32(1)),
        (HostCallKind::UpdateCeoPosition, HostReturn::VoidOrIgnored),
        (HostCallKind::UpdateFirstGuyZ, HostReturn::VoidOrIgnored),
        (HostCallKind::DownUnitComeOut, HostReturn::I32(0)),
        (HostCallKind::SetExitAnimation, HostReturn::VoidOrIgnored),
        (HostCallKind::CloseOrders, HostReturn::VoidOrIgnored),
        (HostCallKind::ClearPartialPath, HostReturn::VoidOrIgnored),
        (HostCallKind::ActorUpdateAction, HostReturn::VoidOrIgnored),
        (
            HostCallKind::ActorReadStrafeTarget,
            HostReturn::Target(WideObjectIdentity::new(5, 88)),
        ),
        (HostCallKind::ScratchGroupAdd, HostReturn::VoidOrIgnored),
        (HostCallKind::ScratchGroupPush, HostReturn::I32(17)),
        (
            HostCallKind::WorkerUpdateAction { slot: 1 },
            HostReturn::VoidOrIgnored,
        ),
        (
            HostCallKind::WorkerReadGarrisonTarget { slot: 1 },
            HostReturn::Target(WideObjectIdentity::new(2, 9)),
        ),
    ];
    facts.host_receipts = kinds
        .into_iter()
        .enumerate()
        .map(|(index, (kind, result))| {
            receipt(
                kind,
                result,
                stamp(7 + index as u32, 10 + index as u64),
                stamp(8 + index as u32, 11 + index as u64),
            )
        })
        .collect();

    let plan = plan_unit_come_out_common_release(&facts).unwrap();
    assert!(plan.steps.iter().any(|step| matches!(
        step,
        CommonReleaseStep::RaiseFirstGuyAfterPlaneExit { after: 1_400, .. }
    )));
    assert!(plan.steps.iter().any(|step| matches!(
        step,
        CommonReleaseStep::ClearPathingMask { after: 0x123, .. }
    )));
    assert!(plan.steps.iter().any(|step| matches!(
        step,
        CommonReleaseStep::IncrementNukesLaunched {
            before: 6,
            after: 7,
            ..
        }
    )));
    assert!(!plan
        .steps
        .iter()
        .any(|step| matches!(step, CommonReleaseStep::ClearCityWorkerFlag { .. })));
    assert!(matches!(
        plan.exit,
        CommonReleaseExit::GatherList(CommonReleaseContinuation {
            resume_va: GATHER_LIST_RESUME_VA,
            scratch_group: Some(17),
            rng: RngStamp {
                seed: 20,
                draws: 23
            },
            ..
        })
    ));
}

#[test]
fn exhausted_city_scan_clears_only_the_retail_worker_flag() {
    let mut facts = base();
    facts.prefix.direct_container = Some(ObjectIdentity::new(2, 9));
    facts.actor.is_captain = true;
    facts.captain_container = Some(CaptainContainerFacts {
        identity: ObjectIdentity::new(2, 9),
        kind: CaptainContainerKind::Build(BuildExitFacts {
            flags_low: ACTIVE_BUILD_FLAG,
            inside_down: -1,
            city_index: 4,
            city_flags_before: Some(CITY_WORKER_FLAG | 5),
            owner_object_slots: 2,
            worker_scan: vec![
                WorkerSlotFacts {
                    active: false,
                    is_worker: false,
                    action_type: None,
                },
                WorkerSlotFacts {
                    active: true,
                    is_worker: false,
                    action_type: None,
                },
            ],
        }),
    });
    let plan = plan_unit_come_out_common_release(&facts).unwrap();
    assert!(plan
        .steps
        .contains(&CommonReleaseStep::ClearCityWorkerFlag {
            store_va: 0x0061_8b1d,
            city_index: 4,
            before: 0x45,
            after: 5,
        }));
}

#[test]
fn unmatched_partial_worker_scan_fails_closed() {
    let mut facts = base();
    facts.prefix.direct_container = Some(ObjectIdentity::new(2, 9));
    facts.actor.is_captain = true;
    facts.captain_container = Some(CaptainContainerFacts {
        identity: ObjectIdentity::new(2, 9),
        kind: CaptainContainerKind::Build(BuildExitFacts {
            flags_low: ACTIVE_BUILD_FLAG,
            inside_down: -1,
            city_index: 4,
            city_flags_before: Some(CITY_WORKER_FLAG),
            owner_object_slots: 3,
            worker_scan: vec![WorkerSlotFacts {
                active: false,
                is_worker: false,
                action_type: None,
            }],
        }),
    });
    assert_eq!(
        plan_unit_come_out_common_release(&facts),
        Err(CommonReleaseError::IncompleteWorkerScan {
            expected_slots: 3,
            observed: 1,
        })
    );
}

#[test]
fn receipt_order_and_rng_continuity_are_mandatory() {
    let mut wrong_kind = base();
    wrong_kind.host_receipts[0].kind = HostCallKind::CloseOrders;
    assert!(matches!(
        plan_unit_come_out_common_release(&wrong_kind),
        Err(CommonReleaseError::ReceiptKind { .. })
    ));

    let mut wrong_rng = base();
    wrong_rng.host_receipts[0].before = stamp(99, 10);
    assert_eq!(
        plan_unit_come_out_common_release(&wrong_rng),
        Err(CommonReleaseError::ReceiptContinuity)
    );
}

#[test]
fn unreachable_payloads_and_bad_recursive_results_are_rejected() {
    let mut non_plane = base();
    non_plane.first_guy = Some(FirstGuyFacts {
        z_after_update: 1,
        last_z_before: 2,
    });
    assert!(matches!(
        plan_unit_come_out_common_release(&non_plane),
        Err(CommonReleaseError::Unexpected(_))
    ));

    let mut bad_child = base();
    bad_child.down_unit = Some(DownUnitFacts {
        identity: ObjectIdentity::new(2, 8),
        flags_low: ACTIVE_OBJECT_FLAG,
    });
    bad_child.host_receipts.push(receipt(
        HostCallKind::DownUnitComeOut,
        HostReturn::I32(4),
        stamp(7, 10),
        stamp(7, 10),
    ));
    assert_eq!(
        plan_unit_come_out_common_release(&bad_child),
        Err(CommonReleaseError::InvalidComeOutResult(4))
    );
}

#[test]
fn extra_receipts_do_not_leak_past_the_atomic_plan() {
    let mut facts = base();
    facts.host_receipts.push(receipt(
        HostCallKind::UpdateCeoPosition,
        HostReturn::VoidOrIgnored,
        stamp(7, 10),
        stamp(7, 10),
    ));
    assert_eq!(
        plan_unit_come_out_common_release(&facts),
        Err(CommonReleaseError::ReceiptCount {
            consumed: 1,
            observed: 2,
        })
    );
}
