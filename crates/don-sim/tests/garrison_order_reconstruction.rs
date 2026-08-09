// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../src/systems/garrison_order.rs"]
mod subject;

use subject::*;

fn id(o: i32, who: i32, uid: u16) -> GarrisonIdentity {
    GarrisonIdentity { o, who, uid }
}

fn actor() -> GarrisonActorFacts {
    GarrisonActorFacts {
        identity: id(7, 2, 0x1234),
        x: 100,
        y: 200,
        unit_masks: 0,
        type_flags_2b4: 0,
        inside_cost: 3,
        local_player: true,
        reaches_target: Some(true),
        captain_o: Some(70),
    }
}

fn target() -> GarrisonTargetFacts {
    GarrisonTargetFacts {
        identity: id(11, 2, 0xabcd),
        raw_address: 0x1122_3344,
        x: 5_000,
        y: 7_000,
        owner: 2,
        is_valid_wall: Some(true),
        is_active: Some(true),
        diplomacy_allows: Some(true),
        capacity_limit: Some(20),
        actor_type_can_garrison: Some(true),
        is_airbase: None,
        can_carry_actor: None,
        footprint_x: 4,
        footprint_y: 6,
        is_dock: None,
        build_flags: 0,
        city_index: 9,
        city_race: None,
        hits: None,
        hits_left: None,
        occupied: Some(4),
        terrain_owner: Some(-1),
        target_owner_allied_with_terrain: None,
        is_build: Some(false),
    }
}

fn order() -> GarrisonOrderState {
    GarrisonOrderState {
        target: id(11, 2, 0xabcd),
        search: 0,
    }
}

fn request() -> GarrisonExecutorRequest {
    GarrisonExecutorRequest {
        actor: actor(),
        order: order(),
        order_flags: ORDER_GROUP,
        outer_uid_validated: true,
        target: Some(target()),
        approach: None,
        alternate: None,
    }
}

#[test]
fn pdb_layout_clear_and_walk_ranges_are_exact() {
    assert_eq!(GARRISON_ORDER_SIZE, 36);
    assert_eq!(GARRISON_WALKED_BYTES, 15);
    assert_eq!((AIRBASE_TYPE, DOCK_TYPE), (0x1bf, 0x1b0));
    assert_eq!(
        [
            offsets::OX,
            offsets::WHOM,
            offsets::UID,
            offsets::SEARCH,
            offsets::UNIT_ORDER_VBASE,
            offsets::FLAGS,
        ],
        [0x08, 0x0c, 0x10, 0x14, 0x1c, 0x20]
    );
    assert_eq!(
        GarrisonOrderState::default(),
        GarrisonOrderState {
            target: id(-1, -1, u16::MAX),
            search: 0,
        }
    );
}

#[test]
fn installer_preserves_full_width_identity_search_and_group_choice() {
    let plan = plan_garrison_install(
        GarrisonInstallRequest {
            target_o: 100_000,
            target_who: 500,
            search: i32::MIN,
            queue_pos: QUEUE_LAST,
            group_flag: -3,
        },
        Some(id(100_000, 500, 0xbeef)),
    )
    .unwrap();
    assert_eq!(plan.order.target, id(100_000, 500, 0xbeef));
    assert_eq!(plan.order.search, i32::MIN);
    assert_eq!(plan.flags, ORDER_GROUP);
    assert_eq!(
        plan.steps,
        [
            GarrisonInstallStep::AllocateOrder(GARRISON_ORDER_INDEX),
            GarrisonInstallStep::AddOrder,
            GarrisonInstallStep::UpdateAction,
        ]
    );

    let ungrouped = plan_garrison_install(
        GarrisonInstallRequest {
            group_flag: 0,
            ..GarrisonInstallRequest {
                target_o: 100_000,
                target_who: 500,
                search: i32::MIN,
                queue_pos: QUEUE_LAST,
                group_flag: -3,
            }
        },
        Some(id(100_000, 500, 0xbeef)),
    )
    .unwrap();
    assert_eq!(ungrouped.flags, 0);
}

#[test]
fn new_and_first_queue_mutations_are_ordered_around_allocation() {
    let new = plan_garrison_install(
        GarrisonInstallRequest {
            target_o: -1,
            target_who: 2,
            search: 0,
            queue_pos: QUEUE_NEW,
            group_flag: 1,
        },
        None,
    )
    .unwrap();
    assert_eq!(new.order.target, id(-1, 2, u16::MAX));
    assert_eq!(
        &new.steps[..5],
        &[
            GarrisonInstallStep::ClearActorMask(0x0400_0000),
            GarrisonInstallStep::StoreActorC0(0),
            GarrisonInstallStep::CloseOrders(0),
            GarrisonInstallStep::ClearPartialPath,
            GarrisonInstallStep::UpdateAction,
        ]
    );
    assert_eq!(new.steps[5], GarrisonInstallStep::AllocateOrder(26));

    let first = plan_garrison_install(
        GarrisonInstallRequest {
            target_o: 11,
            target_who: 2,
            search: 1,
            queue_pos: QUEUE_FIRST,
            group_flag: 1,
        },
        Some(id(11, 2, 99)),
    )
    .unwrap();
    assert!(first.steps.ends_with(&[
        GarrisonInstallStep::ClearPartialPath,
        GarrisonInstallStep::RotateFirstOrderToHead,
        GarrisonInstallStep::UpdateAction,
    ]));
}

#[test]
fn wire_decoder_is_13_bytes_and_negative_target_short_circuits_active_lookup() {
    let mut bytes = [0u8; GARRISON_WIRE_BYTES];
    bytes[0] = 0x14;
    bytes[1..5].copy_from_slice(&(-1i32).to_le_bytes());
    bytes[5..9].copy_from_slice(&(300i32).to_le_bytes());
    bytes[9..13].copy_from_slice(&QUEUE_NEW.to_le_bytes());
    let command = GarrisonWireCommand::decode(bytes);
    let plan = plan_garrison_wire(
        command,
        GarrisonWireFacts {
            package_has_group: true,
            target_active: None,
        },
    )
    .unwrap();
    assert_eq!(plan.consumed, 13);
    assert_eq!(
        plan.steps,
        [
            GarrisonWireStep::LogDecodedCommand,
            GarrisonWireStep::RecordReplayTrace,
            GarrisonWireStep::GroupActionGarrison {
                target_o: -1,
                target_who: 300,
                queue_pos: QUEUE_NEW,
                search_for_alternate: 0,
            },
        ]
    );
    assert_eq!(
        plan_garrison_wire(
            command,
            GarrisonWireFacts {
                package_has_group: true,
                target_active: Some(false),
            },
        ),
        Err(GarrisonPlanError::WireUnexpectedTargetActive)
    );
}

#[test]
fn wire_without_a_package_group_does_not_touch_target_activity() {
    let command = GarrisonWireCommand {
        target_o: 1,
        target_who: 2,
        queue_pos: QUEUE_LAST,
    };
    let plan = plan_garrison_wire(
        command,
        GarrisonWireFacts {
            package_has_group: false,
            target_active: None,
        },
    )
    .unwrap();
    assert_eq!(
        plan.steps,
        [
            GarrisonWireStep::LogDecodedCommand,
            GarrisonWireStep::RecordReplayTrace,
        ]
    );
    assert_eq!(
        plan_garrison_wire(
            command,
            GarrisonWireFacts {
                package_has_group: false,
                target_active: Some(true),
            },
        ),
        Err(GarrisonPlanError::WireUnexpectedTargetActive)
    );
}

#[test]
fn group_member_installer_preserves_all_four_retail_call_tuples() {
    assert_eq!(
        plan_group_entry(QUEUE_FIRST, 99),
        GarrisonGroupEntryPlan {
            opens_first_transaction: true,
            effective_queue_pos: QUEUE_NEW,
            effective_search: 0,
        }
    );
    assert_eq!(
        plan_group_entry(QUEUE_LAST, 99),
        GarrisonGroupEntryPlan {
            opens_first_transaction: false,
            effective_queue_pos: QUEUE_LAST,
            effective_search: 99,
        }
    );
    let base = GarrisonGroupInstallRequest {
        chosen_target_o: 77,
        target_who: 4,
        caller_search: 9,
        caller_queue_pos: QUEUE_NEW,
        path: GarrisonGroupInstallPath::Normal,
    };
    assert_eq!(
        plan_group_member_install(base),
        GarrisonGroupInstallCall {
            target_o: 77,
            target_who: 4,
            search: 9,
            queue_pos: QUEUE_NEW,
            group_flag: 1,
        }
    );
    assert_eq!(
        plan_group_member_install(GarrisonGroupInstallRequest {
            path: GarrisonGroupInstallPath::NewWorker {
                post_is_worker_edx: 0x1234_5678,
            },
            ..base
        }),
        GarrisonGroupInstallCall {
            target_o: 77,
            target_who: 4,
            search: 0x1234_5678,
            queue_pos: QUEUE_FIRST,
            group_flag: 1,
        }
    );
    assert_eq!(
        plan_group_member_install(GarrisonGroupInstallRequest {
            path: GarrisonGroupInstallPath::NewUnpacking,
            ..base
        })
        .queue_pos,
        QUEUE_NEW
    );
    assert_eq!(
        plan_group_member_install(GarrisonGroupInstallRequest {
            path: GarrisonGroupInstallPath::NewPacking,
            ..base
        })
        .queue_pos,
        QUEUE_LAST
    );
}

#[test]
fn helicopter_airbase_success_replaces_garrison_with_returning_strafe() {
    let mut request = request();
    request.actor.type_flags_2b4 = 0x20;
    let target = request.target.as_mut().unwrap();
    target.is_airbase = Some(true);
    target.can_carry_actor = Some(true);
    let plan = plan_garrison_executor(request).unwrap();
    assert_eq!(plan.branch, GarrisonExecutorBranch::HelicopterAirbaseStrafe);
    assert_eq!(plan.direct_rng_draws, 0);
    assert_eq!(
        plan.steps,
        [
            GarrisonHostStep::KillCurrentOrder { arg: 0 },
            GarrisonHostStep::AddStrafeOrder {
                x: -1,
                y: -1,
                target_o: 11,
                target_who: 2,
                arg5: 1,
                queue_pos: QUEUE_FIRST,
                arg7: 1,
            },
            GarrisonHostStep::UpdateOrder,
            GarrisonHostStep::StoreStrafeI32 {
                offset: 0x3c,
                value: 1,
            },
        ]
    );
}

#[test]
fn full_airbase_kills_then_moves_then_emits_feedback() {
    let mut request = request();
    request.actor.type_flags_2b4 = 0x20;
    let target = request.target.as_mut().unwrap();
    target.is_airbase = Some(true);
    target.can_carry_actor = Some(false);
    let plan = plan_garrison_executor(request).unwrap();
    assert_eq!(plan.branch, GarrisonExecutorBranch::HelicopterAirbaseMove);
    assert_eq!(
        plan.steps,
        [
            GarrisonHostStep::KillCurrentOrder { arg: 0 },
            GarrisonHostStep::AddMoveOrder {
                x: 5_000,
                y: 7_000,
                arg3: 1,
                arg4: 0,
                queue_pos: QUEUE_FIRST,
                arg6: 0,
                opaque_arg7: 0x1122_3344,
                tail_x: -1,
                tail_y: -1,
            },
            GarrisonHostStep::LocalFeedback(GarrisonFeedback::AirbaseFull),
        ]
    );

    request.actor.local_player = false;
    assert!(plan_garrison_executor(request).unwrap().steps.contains(
        &GarrisonHostStep::LocalFeedback(GarrisonFeedback::AirbaseFull)
    ));
}

#[test]
fn every_normal_admission_gate_is_fail_closed() {
    let mut missing = request();
    missing.target.as_mut().unwrap().is_valid_wall = None;
    assert_eq!(
        plan_garrison_executor(missing),
        Err(GarrisonPlanError::MissingAdmissionPredicate)
    );

    let mut known_false = request();
    known_false.target.as_mut().unwrap().is_valid_wall = Some(false);
    known_false.target.as_mut().unwrap().is_active = None;
    let plan = plan_garrison_executor(known_false).unwrap();
    assert_eq!(plan.branch, GarrisonExecutorBranch::InvalidAdmission);
    assert_eq!(plan.steps, [GarrisonHostStep::KillCurrentOrder { arg: 0 }]);

    let mut stale = request();
    stale.outer_uid_validated = false;
    assert_eq!(
        plan_garrison_executor(stale),
        Err(GarrisonPlanError::TargetUidNotPrevalidated)
    );
}

#[test]
fn approach_search_pins_radii_angle_retry_tail_and_opaque_ecx() {
    let mut request = request();
    request.actor.reaches_target = Some(false);
    let target = request.target.as_mut().unwrap();
    target.is_dock = Some(true);
    request.approach = Some(GarrisonApproachFacts {
        angle: 0x1020_3040,
        first: Some(GarrisonSpotResult::Failed {
            post_call_ecx: 0xaaaa_0001,
        }),
        second: Some(GarrisonSpotResult::Found {
            x: 9_001,
            y: 9_002,
            post_call_ecx: 0xbbbb_0002,
        }),
    });
    let plan = plan_garrison_executor(request).unwrap();
    assert_eq!(plan.branch, GarrisonExecutorBranch::ApproachSecond);
    let base = 4 * 0x60 + 0x30 + 0x180;
    assert_eq!(
        plan.steps,
        [
            GarrisonHostStep::FindNearbySpot {
                request: GarrisonSpotRequest {
                    center_x: 5_000,
                    center_y: 7_000,
                    min_radius: base,
                    max_radius: base + 0x180,
                    step: 0,
                    angle: 0x1020_3040,
                    filter: 3,
                    actor_o: 7,
                    actor_who: 2,
                    tail_0: 0,
                    tail_1: 0,
                    tail_2: -1,
                    tail_3: 0,
                    tail_4: -1,
                },
                result: GarrisonSpotResult::Failed {
                    post_call_ecx: 0xaaaa_0001,
                },
            },
            GarrisonHostStep::FindNearbySpot {
                request: GarrisonSpotRequest {
                    tail_1: 1,
                    ..GarrisonSpotRequest {
                        center_x: 5_000,
                        center_y: 7_000,
                        min_radius: base,
                        max_radius: base + 0x180,
                        step: 0,
                        angle: 0x1020_3040,
                        filter: 3,
                        actor_o: 7,
                        actor_who: 2,
                        tail_0: 0,
                        tail_1: 0,
                        tail_2: -1,
                        tail_3: 0,
                        tail_4: -1,
                    }
                },
                result: GarrisonSpotResult::Found {
                    x: 9_001,
                    y: 9_002,
                    post_call_ecx: 0xbbbb_0002,
                },
            },
            GarrisonHostStep::AddMoveOrder {
                x: 9_001,
                y: 9_002,
                arg3: 1,
                arg4: 0,
                queue_pos: QUEUE_FIRST,
                arg6: 0,
                opaque_arg7: 0xbbbb_0002,
                tail_x: -1,
                tail_y: -1,
            },
        ]
    );
}

#[test]
fn second_search_is_forbidden_after_first_success_and_required_after_failure() {
    let mut request = request();
    request.actor.reaches_target = Some(false);
    request.target.as_mut().unwrap().is_dock = Some(false);
    request.approach = Some(GarrisonApproachFacts {
        angle: 7,
        first: Some(GarrisonSpotResult::Found {
            x: 1,
            y: 2,
            post_call_ecx: 3,
        }),
        second: Some(GarrisonSpotResult::Failed { post_call_ecx: 4 }),
    });
    assert_eq!(
        plan_garrison_executor(request),
        Err(GarrisonPlanError::UnexpectedSecondSpot)
    );

    request.approach.as_mut().unwrap().first =
        Some(GarrisonSpotResult::Failed { post_call_ecx: 3 });
    request.approach.as_mut().unwrap().second = None;
    assert_eq!(
        plan_garrison_executor(request),
        Err(GarrisonPlanError::MissingSecondSpot)
    );
}

#[test]
fn city_rejection_feedback_precedes_common_kill() {
    let mut request = request();
    let target = request.target.as_mut().unwrap();
    target.build_flags = 0x20;
    target.city_race = Some(9);
    let plan = plan_garrison_executor(request).unwrap();
    assert_eq!(plan.branch, GarrisonExecutorBranch::CityOwnershipRejected);
    assert_eq!(
        plan.steps,
        [
            GarrisonHostStep::LocalFeedback(GarrisonFeedback::CityOwnershipOrMetric),
            GarrisonHostStep::KillCurrentOrder { arg: 0 },
        ]
    );
}

#[test]
fn city_hits_gate_uses_signed_truncating_one_tenth_boundary() {
    let mut rejected = request();
    let target = rejected.target.as_mut().unwrap();
    target.build_flags = 0x20;
    target.city_race = Some(2);
    target.hits = Some(99);
    target.hits_left = Some(8);
    assert_eq!(
        plan_garrison_executor(rejected).unwrap().branch,
        GarrisonExecutorBranch::CityMetricRejected
    );

    let mut admitted = rejected;
    admitted.target.as_mut().unwrap().hits_left = Some(9);
    assert_eq!(
        plan_garrison_executor(admitted).unwrap().branch,
        GarrisonExecutorBranch::Entered
    );
}

#[test]
fn capacity_full_searches_alternate_then_preserves_order_group_flag() {
    let mut request = request();
    request.order.search = -5;
    let target = request.target.as_mut().unwrap();
    target.capacity_limit = Some(6);
    target.occupied = Some(4);
    request.alternate = Some(GarrisonAlternateFacts {
        city_object_active: Some(true),
        alternate_o: Some(88),
    });
    let plan = plan_garrison_executor(request).unwrap();
    assert_eq!(plan.branch, GarrisonExecutorBranch::CapacityFullAlternate);
    assert_eq!(
        plan.steps,
        [
            GarrisonHostStep::FindGarrisonBuild {
                city_index: 9,
                target_owner: 2,
                result_o: 88,
            },
            GarrisonHostStep::KillCurrentOrder { arg: 0 },
            GarrisonHostStep::AddGarrisonOrder {
                target_o: 88,
                target_who: 2,
                search: 1,
                queue_pos: QUEUE_FIRST,
                group_flag: 1,
            },
        ]
    );
}

#[test]
fn capacity_full_feedback_and_sound_precede_common_kill() {
    let mut request = request();
    let target = request.target.as_mut().unwrap();
    target.capacity_limit = Some(6);
    target.occupied = Some(4);
    let plan = plan_garrison_executor(request).unwrap();
    assert_eq!(plan.branch, GarrisonExecutorBranch::CapacityFull);
    assert_eq!(
        plan.steps,
        [
            GarrisonHostStep::LocalFeedback(GarrisonFeedback::GarrisonFull),
            GarrisonHostStep::PlaySound { category: 0x40 },
            GarrisonHostStep::KillCurrentOrder { arg: 0 },
        ]
    );
}

#[test]
fn hostile_terrain_kills_before_feedback_but_success_uses_captain() {
    let mut hostile = request();
    let target = hostile.target.as_mut().unwrap();
    target.terrain_owner = Some(5);
    target.target_owner_allied_with_terrain = Some(false);
    let plan = plan_garrison_executor(hostile).unwrap();
    assert_eq!(plan.branch, GarrisonExecutorBranch::HostileTerritory);
    assert_eq!(
        plan.steps,
        [
            GarrisonHostStep::KillCurrentOrder { arg: 0 },
            GarrisonHostStep::LocalFeedback(GarrisonFeedback::HostileTerritory),
            GarrisonHostStep::PlaySound { category: 0x40 },
        ]
    );

    let mut success = request();
    success.target.as_mut().unwrap().is_build = Some(true);
    let plan = plan_garrison_executor(success).unwrap();
    assert_eq!(plan.branch, GarrisonExecutorBranch::Entered);
    assert_eq!(plan.direct_rng_draws, 0);
    assert_eq!(
        plan.steps,
        [
            GarrisonHostStep::GoInside {
                captain_o: 70,
                target_o: 11,
                target_who: 2,
                arg3: 0,
            },
            GarrisonHostStep::ReadTargetIsBuild { value: true },
            GarrisonHostStep::SetOptionsRebuild { value: 1 },
            GarrisonHostStep::KillCaptainGarrisonOrder {
                captain_o: 70,
                arg: 0,
            },
        ]
    );
}

#[test]
fn receipt_rejects_stale_snapshot_and_any_plan_splice() {
    let snapshot = GarrisonHostSnapshot {
        actor: id(7, 2, 0x1234),
        target: id(11, 2, 0xabcd),
        actor_version: 10,
        target_version: 20,
        queue_digest: 30,
        path_digest: 40,
        external_epoch: 50,
        rng_epoch: 60,
    };
    let mut receipt = preflight_garrison_executor(snapshot, request()).unwrap();
    assert_eq!(
        validate_garrison_receipt(&receipt, snapshot).unwrap(),
        &receipt.plan
    );

    let mut stale = snapshot;
    stale.queue_digest ^= 1;
    assert_eq!(
        validate_garrison_receipt(&receipt, stale),
        Err(GarrisonPlanError::ReceiptSnapshotMismatch)
    );

    receipt
        .plan
        .steps
        .push(GarrisonHostStep::KillCurrentOrder { arg: 99 });
    assert_eq!(
        validate_garrison_receipt(&receipt, snapshot),
        Err(GarrisonPlanError::ReceiptPlanMismatch)
    );
}

#[test]
fn open_tail_inventory_keeps_live_order_closure_red() {
    assert_eq!(GARRISON_OPEN_TAILS.len(), 10);
    assert!(GARRISON_OPEN_TAILS.contains(&GarrisonOpenTail::ConcretePayloadSaveAndLiveTickAdapter));
    assert!(GARRISON_OPEN_TAILS.contains(&GarrisonOpenTail::GroupActionGarrisonHostAdapter));
}
