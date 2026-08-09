// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../src/systems/special_anim_executor.rs"]
mod subject;

use subject::*;

fn identity(o: i32, who: i32, uid: u16) -> ObjectIdentity {
    ObjectIdentity { o, who, uid }
}

fn actor() -> ActorFacts {
    ActorFacts {
        identity: identity(7, 2, 91),
    }
}

fn order(kind: SpecialAnimKind) -> SpecialAnimState {
    SpecialAnimState {
        special_type: kind,
        started: 8,
        frames: 9,
        data1: 11,
        data2: 12,
        data3: 1_000,
        data4: 2_000,
        ox: 31,
        whom: 4,
    }
}

fn request(kind: SpecialAnimKind) -> SpecialAnimExecutorRequest {
    SpecialAnimExecutorRequest {
        order: order(kind),
        actor: actor(),
        enter_target: None,
        exit_target: None,
        random_draws: None,
        helicopter_samples: None,
        terrain_z: None,
    }
}

#[test]
fn concrete_layout_defaults_and_walk_span_are_exact() {
    assert_eq!(SPECIAL_ANIM_ORDER_INDEX, 25);
    assert_eq!(SPECIAL_ANIM_ORDER_SIZE, 44);
    assert_eq!(SPECIAL_ANIM_WALKED_BYTES, 37);
    assert_eq!(SPECIAL_ANIM_WALK_PAYLOAD_BYTES, 36);
    assert_eq!(offsets::FLAGS, 0x04);
    assert_eq!(offsets::SPECIAL_TYPE, 0x08);
    assert_eq!(offsets::STARTED, 0x0c);
    assert_eq!(offsets::FRAMES, 0x10);
    assert_eq!(offsets::DATA1, 0x14);
    assert_eq!(offsets::DATA2, 0x18);
    assert_eq!(offsets::DATA3, 0x1c);
    assert_eq!(offsets::DATA4, 0x20);
    assert_eq!(offsets::OX, 0x24);
    assert_eq!(offsets::WHOM, 0x28);
    assert_eq!(
        SpecialAnimState::default(),
        SpecialAnimState {
            special_type: SpecialAnimKind::Unit,
            started: 0,
            frames: 0,
            data1: -1,
            data2: -1,
            data3: -1,
            data4: -1,
            ox: -1,
            whom: -1,
        }
    );
    assert_eq!(SpecialAnimKind::from_raw(0), Some(SpecialAnimKind::Enter));
    assert_eq!(SpecialAnimKind::from_raw(1), Some(SpecialAnimKind::Exit));
    assert_eq!(SpecialAnimKind::from_raw(2), Some(SpecialAnimKind::Unit));
    assert_eq!(SpecialAnimKind::from_raw(3), None);
}

#[test]
fn installer_ignores_queue_pos_and_always_rotates_to_first() {
    let make = |queue_pos| {
        plan_special_anim_install(SpecialAnimInstallRequest {
            special_type: SpecialAnimKind::Exit,
            data1: 17,
            data2: 19,
            queue_pos,
        })
    };
    let first = make(0);
    assert_eq!(first, make(1));
    assert_eq!(first, make(2));
    assert_eq!(first, make(i32::MIN));
    assert_eq!(first.flags, ORDER_GROUP);
    assert_eq!(first.order.data1, 17);
    assert_eq!(first.order.data2, 19);
    assert_eq!(first.order.data3, -1);
    assert_eq!(
        first.steps,
        vec![
            SpecialAnimInstallStep::AllocateOrder(25),
            SpecialAnimInstallStep::AddOrder,
            SpecialAnimInstallStep::ClearPartialPath,
            SpecialAnimInstallStep::RotateInsertedOrderToHead,
            SpecialAnimInstallStep::UpdateAction,
        ]
    );
}

#[test]
fn land_plane_is_the_direct_installer_and_patches_the_enter_target_slot() {
    let fixed = plan_land_plane_install(LandPlaneInstallRequest {
        target_o: 41,
        target_who: 5,
        target_gpiece: 77,
        actor_helicopter: false,
    });
    assert_eq!(fixed.order.special_type, SpecialAnimKind::Enter);
    assert_eq!(fixed.order.data1, 77);
    assert_eq!(fixed.order.data2, 1);
    assert_eq!(fixed.order.data3, 41);
    assert_eq!(fixed.order.data4, 5);
    assert_eq!(fixed.order.ox, -1);
    assert_eq!(fixed.flags, ORDER_GROUP);
    assert_eq!(
        fixed.steps,
        vec![
            LandPlaneInstallStep::ReadTargetGpiece { result: 77 },
            LandPlaneInstallStep::AddSpecialAnimEnter {
                data1: 77,
                data2: 1,
            },
            LandPlaneInstallStep::PatchData3TargetO(41),
            LandPlaneInstallStep::PatchData4TargetWho(5),
        ]
    );

    let helicopter = plan_land_plane_install(LandPlaneInstallRequest {
        actor_helicopter: true,
        ..LandPlaneInstallRequest {
            target_o: 41,
            target_who: 5,
            target_gpiece: 77,
            actor_helicopter: false,
        }
    });
    assert_eq!(helicopter.order.data2, 3);
    assert_eq!(
        helicopter.steps[0],
        LandPlaneInstallStep::ReadTargetGpiece { result: 77 }
    );
}

#[test]
fn unit_type_is_a_true_no_op_and_rejects_unqueried_facts() {
    let request = request(SpecialAnimKind::Unit);
    let plan = plan_special_anim_executor(request).unwrap();
    assert_eq!(plan.branch, SpecialAnimBranch::UnitNoOp);
    assert_eq!(plan.after_order, request.order);
    assert!(plan.steps.is_empty());

    let mut malformed_actor = request;
    malformed_actor.actor.identity.o = -1;
    assert_eq!(
        plan_special_anim_executor(malformed_actor).unwrap().branch,
        SpecialAnimBranch::UnitNoOp
    );

    let mut with_rng = request;
    with_rng.random_draws = Some([0, 0]);
    assert_eq!(
        plan_special_anim_executor(with_rng),
        Err(SpecialAnimPlanError::UnexpectedRandomDraws)
    );
}

#[test]
fn executor_never_reads_data1_or_data2_but_preserves_both_walked_words() {
    let mut left = request(SpecialAnimKind::Exit);
    left.order.ox = -1;
    left.order.data1 = i32::MIN;
    left.order.data2 = 17;
    let mut right = left;
    right.order.data1 = i32::MAX;
    right.order.data2 = -29;
    let left_plan = plan_special_anim_executor(left).unwrap();
    let right_plan = plan_special_anim_executor(right).unwrap();
    assert_eq!(left_plan.steps, right_plan.steps);
    assert_eq!(left_plan.after_order.data1, i32::MIN);
    assert_eq!(left_plan.after_order.data2, 17);
    assert_eq!(right_plan.after_order.data1, i32::MAX);
    assert_eq!(right_plan.after_order.data2, -29);
}

#[test]
fn enter_valid_build_short_circuits_carrier_then_goes_inside_before_kill() {
    let mut request = request(SpecialAnimKind::Enter);
    request.order.data3 = 55;
    request.order.data4 = 6;
    request.enter_target = Some(EnterTargetFacts {
        identity: identity(55, 6, 201),
        is_valid_build: true,
        is_aircraft_carrier: None,
    });
    let plan = plan_special_anim_executor(request).unwrap();
    assert_eq!(plan.branch, SpecialAnimBranch::EnterGoInside);
    assert_eq!(plan.after_order.started, 1);
    assert_eq!(plan.after_order.frames, 10);
    assert_eq!(
        plan.steps,
        vec![
            SpecialAnimHostStep::StoreFrames(10),
            SpecialAnimHostStep::StoreStarted(1),
            SpecialAnimHostStep::QueryIsValidBuild {
                target_o: 55,
                target_who: 6,
                result: true,
            },
            SpecialAnimHostStep::GoInside {
                target_o: 55,
                target_who: 6,
                arg3: 0,
            },
            SpecialAnimHostStep::KillCurrentOrder(0),
        ]
    );

    request.enter_target.as_mut().unwrap().is_aircraft_carrier = Some(false);
    assert_eq!(
        plan_special_anim_executor(request),
        Err(SpecialAnimPlanError::UnexpectedAircraftCarrierPredicate)
    );
}

#[test]
fn enter_invalid_build_queries_carrier_and_the_rejected_target_dies_after_kill() {
    let mut carrier = request(SpecialAnimKind::Enter);
    carrier.order.data3 = 55;
    carrier.order.data4 = 6;
    carrier.enter_target = Some(EnterTargetFacts {
        identity: identity(55, 6, 201),
        is_valid_build: false,
        is_aircraft_carrier: Some(true),
    });
    assert_eq!(
        plan_special_anim_executor(carrier).unwrap().branch,
        SpecialAnimBranch::EnterGoInside
    );

    carrier.enter_target.as_mut().unwrap().is_aircraft_carrier = Some(false);
    let plan = plan_special_anim_executor(carrier).unwrap();
    assert_eq!(plan.branch, SpecialAnimBranch::EnterDie);
    assert_eq!(
        &plan.steps[2..],
        &[
            SpecialAnimHostStep::QueryIsValidBuild {
                target_o: 55,
                target_who: 6,
                result: false,
            },
            SpecialAnimHostStep::QueryIsAircraftCarrier {
                target_o: 55,
                target_who: 6,
                type_index: AIRCRAFT_CARRIER_TYPE,
                strict: 0,
                result: false,
            },
            SpecialAnimHostStep::KillCurrentOrder(0),
            SpecialAnimHostStep::Die {
                arg1: 0,
                arg2: -1,
                arg3_bits: 0,
            },
        ]
    );
}

#[test]
fn exit_without_target_slot_does_not_lookup_and_only_retires_after_entry_writes() {
    let mut request = request(SpecialAnimKind::Exit);
    request.order.ox = -1;
    request.order.whom = -999;
    let plan = plan_special_anim_executor(request).unwrap();
    assert_eq!(plan.branch, SpecialAnimBranch::ExitWithoutAirbase);
    assert_eq!(
        plan.steps,
        vec![
            SpecialAnimHostStep::StoreFrames(10),
            SpecialAnimHostStep::StoreStarted(1),
            SpecialAnimHostStep::KillCurrentOrder(0),
        ]
    );
}

#[test]
fn exit_non_airbase_queries_then_retires_without_rng_or_terrain() {
    let mut request = request(SpecialAnimKind::Exit);
    request.exit_target = Some(ExitTargetFacts {
        identity: identity(31, 4, 71),
        is_airbase: false,
    });
    let plan = plan_special_anim_executor(request).unwrap();
    assert_eq!(plan.branch, SpecialAnimBranch::ExitWithoutAirbase);
    assert_eq!(
        &plan.steps[2..],
        &[
            SpecialAnimHostStep::QueryIsAirbase {
                target_o: 31,
                target_who: 4,
                type_index: AIRBASE_TYPE,
                strict: 0,
                result: false,
            },
            SpecialAnimHostStep::KillCurrentOrder(0),
        ]
    );
}

#[test]
fn nonhelicopter_airbase_exit_uses_fixed_offset_and_no_rng() {
    let mut request = request(SpecialAnimKind::Exit);
    request.exit_target = Some(ExitTargetFacts {
        identity: identity(31, 4, 71),
        is_airbase: true,
    });
    request.helicopter_samples = Some([false, false]);
    request.terrain_z = Some(333);
    let plan = plan_special_anim_executor(request).unwrap();
    assert_eq!(plan.branch, SpecialAnimBranch::ExitAirbase);
    assert!(!plan
        .steps
        .iter()
        .any(|step| matches!(step, SpecialAnimHostStep::RandomGet { .. })));
    assert!(plan.steps.contains(&SpecialAnimHostStep::SetNewLocation {
        x: 808,
        y: 2_000,
        arg3: 1,
        arg4: 1,
    }));
    assert!(plan.steps.contains(&SpecialAnimHostStep::FindTerrainZ {
        x: 1_000,
        y: 2_000,
        arg3: 0,
        result: 333,
    }));
    assert_eq!(
        plan.steps.last(),
        Some(&SpecialAnimHostStep::SameTickUnitWork)
    );
}

#[test]
fn helicopter_airbase_exit_draws_twice_before_mutation_and_raises_primary_guy() {
    let mut request = request(SpecialAnimKind::Exit);
    request.exit_target = Some(ExitTargetFacts {
        identity: identity(31, 4, 71),
        is_airbase: true,
    });
    request.helicopter_samples = Some([true, true]);
    request.random_draws = Some([10, 6]);
    request.terrain_z = Some(333);
    let plan = plan_special_anim_executor(request).unwrap();
    assert_eq!(
        &plan.steps[4..8],
        &[
            SpecialAnimHostStep::RandomGet {
                min: 0,
                max: 0xffff,
                result: 10,
            },
            SpecialAnimHostStep::RandomGet {
                min: 0,
                max: 0xffff,
                result: 6,
            },
            SpecialAnimHostStep::SetAngle { angle: 0, arg3: 1 },
            SpecialAnimHostStep::SetNewLocation {
                x: 813,
                y: 2_001,
                arg3: 1,
                arg4: 1,
            },
        ]
    );
    assert!(plan
        .steps
        .contains(&SpecialAnimHostStep::SetPrimaryGuyZ { z: 333, arg2: 1 }));
    assert!(plan.steps.contains(&SpecialAnimHostStep::RaisePrimaryGuyZ {
        delta: 200,
        arg2: 1,
    }));
    assert_eq!(
        &plan.steps[plan.steps.len() - 4..],
        &[
            SpecialAnimHostStep::ShiftPitchToLastAndZero,
            SpecialAnimHostStep::ShiftBankToLastAndZero,
            SpecialAnimHostStep::KillCurrentOrder(0),
            SpecialAnimHostStep::SameTickUnitWork,
        ]
    );
}

#[test]
fn conditional_host_facts_are_missing_or_unexpected_not_default_false() {
    let mut enter = request(SpecialAnimKind::Enter);
    assert_eq!(
        plan_special_anim_executor(enter),
        Err(SpecialAnimPlanError::MissingEnterTarget)
    );
    enter.enter_target = Some(EnterTargetFacts {
        identity: identity(1_000, 2_000, 1),
        is_valid_build: false,
        is_aircraft_carrier: None,
    });
    assert_eq!(
        plan_special_anim_executor(enter),
        Err(SpecialAnimPlanError::MissingAircraftCarrierPredicate)
    );

    let mut exit = request(SpecialAnimKind::Exit);
    exit.exit_target = Some(ExitTargetFacts {
        identity: identity(31, 4, 2),
        is_airbase: true,
    });
    assert_eq!(
        plan_special_anim_executor(exit),
        Err(SpecialAnimPlanError::MissingTerrainHeight)
    );
    exit.terrain_z = Some(0);
    assert_eq!(
        plan_special_anim_executor(exit),
        Err(SpecialAnimPlanError::MissingHelicopterSamples)
    );
    exit.helicopter_samples = Some([true, true]);
    assert_eq!(
        plan_special_anim_executor(exit),
        Err(SpecialAnimPlanError::MissingRandomDraws)
    );
    exit.random_draws = Some([-1, 0]);
    assert_eq!(
        plan_special_anim_executor(exit),
        Err(SpecialAnimPlanError::RandomDrawOutOfRange)
    );
    exit.random_draws = Some([RANDOM_RESULT_MAX + 1, 0]);
    assert_eq!(
        plan_special_anim_executor(exit),
        Err(SpecialAnimPlanError::RandomDrawOutOfRange)
    );
}

#[test]
fn the_two_helicopter_observations_are_not_collapsed() {
    let mut request = request(SpecialAnimKind::Exit);
    request.exit_target = Some(ExitTargetFacts {
        identity: identity(31, 4, 71),
        is_airbase: true,
    });
    request.helicopter_samples = Some([false, true]);
    request.terrain_z = Some(333);
    let plan = plan_special_anim_executor(request).unwrap();
    assert!(!plan
        .steps
        .iter()
        .any(|step| matches!(step, SpecialAnimHostStep::RandomGet { .. })));
    assert!(plan.steps.contains(&SpecialAnimHostStep::SetNewLocation {
        x: 808,
        y: 2_000,
        arg3: 1,
        arg4: 1,
    }));
    let first_z = plan
        .steps
        .iter()
        .position(|step| matches!(step, SpecialAnimHostStep::SetPrimaryGuyZ { .. }))
        .unwrap();
    assert_eq!(
        plan.steps[first_z + 1],
        SpecialAnimHostStep::ReadActorHelicopterFlag {
            sample: 2,
            result: true,
        }
    );
    assert_eq!(
        plan.steps[first_z + 2],
        SpecialAnimHostStep::RaisePrimaryGuyZ {
            delta: 200,
            arg2: 1,
        }
    );
}

#[test]
fn compiled_continuation_tail_is_unreachable_for_every_valid_discriminator() {
    for kind in [
        SpecialAnimKind::Enter,
        SpecialAnimKind::Exit,
        SpecialAnimKind::Unit,
    ] {
        for ox_nonnegative in [false, true] {
            for is_airbase in [false, true] {
                assert!(!compiled_continuation_reachable(
                    kind,
                    ox_nonnegative,
                    is_airbase
                ));
            }
        }
    }
    assert_eq!(
        compiled_continuation_threshold(SpecialAnimKind::Enter, true, true),
        Some(0)
    );
    assert_eq!(
        compiled_continuation_threshold(SpecialAnimKind::Exit, true, true),
        Some(1)
    );
    assert_eq!(
        compiled_continuation_threshold(SpecialAnimKind::Unit, true, true),
        None
    );
    assert_eq!(
        COMPILED_EXIT_PROGRESS_TAIL,
        &[
            CompiledDeadTailStep::SetLocationFromExitBase,
            CompiledDeadTailStep::FindTerrainZFromExitBase,
            CompiledDeadTailStep::ResolvePrimaryGuyUnchecked,
            CompiledDeadTailStep::SetGuyZFromTerrainIfStartedZeroElseShiftCurrentToLast,
            CompiledDeadTailStep::SetAngleZeroWithDeadMiddleAbiWord,
            CompiledDeadTailStep::ShiftBankToLastAndZero,
            CompiledDeadTailStep::IncrementStarted,
        ]
    );
    assert_eq!(
        COMPILED_NON_EXIT_PROGRESS_TAIL,
        &[
            CompiledDeadTailStep::QueryEnterTargetIsValidBuildWithoutAirbaseFallback,
            CompiledDeadTailStep::InvalidEnterKillThenDie,
            CompiledDeadTailStep::DecodeTargetXyzWithXorKey(0x63637),
            CompiledDeadTailStep::SetLocationFromDecodedTarget,
            CompiledDeadTailStep::ResolvePrimaryGuyUnchecked,
            CompiledDeadTailStep::ShiftGuyZToDecodedTargetZ,
            CompiledDeadTailStep::SetAngleZeroWithDeadMiddleAbiWord,
            CompiledDeadTailStep::ShiftBankToLastAndZero,
            CompiledDeadTailStep::IncrementStarted,
        ]
    );
}

fn snapshot(request: &SpecialAnimExecutorRequest) -> SpecialAnimHostSnapshot {
    let target = match request.order.special_type {
        SpecialAnimKind::Enter => request.enter_target.map(|facts| facts.identity),
        SpecialAnimKind::Exit => request.exit_target.map(|facts| facts.identity),
        SpecialAnimKind::Unit => None,
    };
    SpecialAnimHostSnapshot {
        actor: ObjectSnapshot {
            identity: request.actor.identity,
            version: 10,
        },
        target: target.map(|identity| ObjectSnapshot {
            identity,
            version: 20,
        }),
        current_order: request.order,
        current_order_digest: 30,
        queue_digest: 40,
        path_digest: 50,
        primary_guy_digest: 60,
        object_epoch: 70,
        terrain_epoch: 80,
        external_epoch: 90,
        rng_epoch: 100,
    }
}

#[test]
fn receipt_rejects_stale_host_state_and_plan_splicing() {
    let mut request = request(SpecialAnimKind::Exit);
    request.exit_target = Some(ExitTargetFacts {
        identity: identity(31, 4, 71),
        is_airbase: true,
    });
    request.helicopter_samples = Some([false, false]);
    request.terrain_z = Some(333);
    let host = snapshot(&request);
    let receipt = preflight_special_anim_executor(host, request).unwrap();
    assert_eq!(
        validate_special_anim_receipt(&receipt, host).unwrap(),
        &receipt.plan
    );

    let mut stale = host;
    stale.rng_epoch += 1;
    assert_eq!(
        validate_special_anim_receipt(&receipt, stale),
        Err(SpecialAnimPlanError::ReceiptSnapshotMismatch)
    );

    let mut wrong_order_host = host;
    wrong_order_host.current_order.data1 ^= 1;
    assert_eq!(
        preflight_special_anim_executor(wrong_order_host, request),
        Err(SpecialAnimPlanError::ReceiptOrderMismatch)
    );

    let mut spliced = receipt;
    spliced.plan.branch = SpecialAnimBranch::EnterDie;
    assert_eq!(
        validate_special_anim_receipt(&spliced, host),
        Err(SpecialAnimPlanError::ReceiptPlanMismatch)
    );
}

#[test]
fn closure_inventory_keeps_every_unadapted_host_tail_explicit() {
    assert_eq!(SPECIAL_ANIM_OPEN_TAILS.len(), 8);
    assert!(SPECIAL_ANIM_OPEN_TAILS.contains(&SpecialAnimOpenTail::InternalLandPlaneInstaller));
    assert!(
        SPECIAL_ANIM_OPEN_TAILS.contains(&SpecialAnimOpenTail::ObjectLookupAndVirtualPredicates)
    );
    assert!(SPECIAL_ANIM_OPEN_TAILS.contains(&SpecialAnimOpenTail::CanonicalGameRandom));
    assert!(SPECIAL_ANIM_OPEN_TAILS.contains(&SpecialAnimOpenTail::DispatcherAndLiveTickAdapter));
}
