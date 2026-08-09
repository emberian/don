// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../src/systems/guard_order.rs"]
mod subject;

use subject::*;

fn identity(o: i32, who: i32, uid: u16) -> GuardIdentity {
    GuardIdentity { o, who, uid }
}

fn order() -> GuardOrderState {
    GuardOrderState {
        target: identity(7, 2, 0x7788),
        dx: 30,
        dy: 40,
        guard_x: 333,
        guard_y: 444,
        idle: 9,
        retry: 0,
    }
}

fn actor() -> GuardActorFacts {
    GuardActorFacts {
        identity: identity(5, 1, 0x1122),
        x: 100,
        y: 200,
        angle: 10,
        frame: 2,
        unit_masks: 0,
        type_flags_2b4: 0,
        type_flags_2b8: 0,
        type_slot_10c: false,
        actor_slot_cc: false,
        actor_slot_c4: false,
        actor_slot_100: false,
        is_unpacking: false,
        is_type_7b: false,
        is_captain: true,
    }
}

fn target() -> GuardTargetFacts {
    GuardTargetFacts {
        identity: identity(7, 2, 0xaabb),
        active: true,
        initial_valid_unit: true,
        initial_on_map: Some(true),
        valid_unit: true,
        on_map: Some(true),
        is_moving: true,
        is_wallbuild: false,
        x: 1_000,
        y: 2_000,
        angle: 0x1234_5678,
        unit_masks: 0,
        attack_dist: 0x601,
        x_size: 3,
        y_size: 5,
    }
}

fn spatial() -> GuardSpatialFacts {
    GuardSpatialFacts {
        sin_dy: 11,
        cos_dx: 13,
        cos_dy: 17,
        sin_dx: 19,
        projected_div3_x: 4,
        projected_div3_y: 6,
        invalid_loc: false,
        terrain_flags: 0,
        building_spot: None,
        primary_spot: None,
        secondary_spot: None,
        chosen_div3_x: 10,
        chosen_div3_y: 20,
        actor_cell_x: 1,
        actor_cell_y: 2,
        guard_cell_x: 10,
        guard_cell_y: 20,
        find_angle: None,
    }
}

fn request() -> GuardExecutorRequest {
    GuardExecutorRequest {
        actor: actor(),
        order: order(),
        target: Some(target()),
        spatial: Some(spatial()),
        post_move: Some(GuardPostMoveFacts {
            coarse_manhattan: 3,
            head_is_guard: true,
            random_draw: Some(5),
        }),
    }
}

#[test]
fn pdb_layout_and_clear_after_image_are_exact() {
    assert_eq!(GUARD_ORDER_SIZE, 56);
    assert_eq!(GUARD_WALKED_BYTES, 35);
    assert_eq!(
        [
            offsets::OX,
            offsets::WHOM,
            offsets::UID,
            offsets::DX,
            offsets::DY,
            offsets::GUARD_X,
            offsets::GUARD_Y,
            offsets::IDLE,
            offsets::RETRY,
            offsets::UNIT_ORDER_VBASE,
            offsets::FLAGS,
        ],
        [0x08, 0x0c, 0x10, 0x14, 0x18, 0x1c, 0x20, 0x24, 0x28, 0x30, 0x34]
    );
    assert_eq!(
        GuardOrderState::default(),
        GuardOrderState {
            target: identity(-1, -1, u16::MAX),
            dx: 0,
            dy: 0,
            guard_x: 0,
            guard_y: 0,
            idle: 0,
            retry: 0,
        }
    );
}

#[test]
fn valid_target_install_captures_full_width_identity_position_and_group_flag() {
    let request = GuardInstallRequest {
        actor: identity(70_000, 300, 0x1234),
        actor_x: -10,
        actor_y: -20,
        target_o: 90_000,
        target_who: 400,
        dx: i32::MIN,
        dy: i32::MAX,
        queue_pos: QUEUE_LAST,
        unused_arg6: 0x5566_7788,
    };
    let plan = plan_guard_install(
        request,
        Some(GuardInstallTarget {
            identity: identity(90_000, 400, 0xbeef),
            x: 1_234_567,
            y: -7_654_321,
        }),
    )
    .unwrap();
    assert_eq!(plan.flags, ORDER_GROUP);
    assert_eq!(plan.order.target, identity(90_000, 400, 0xbeef));
    assert_eq!((plan.order.dx, plan.order.dy), (i32::MIN, i32::MAX));
    assert_eq!(
        (plan.order.guard_x, plan.order.guard_y),
        (1_234_567, -7_654_321)
    );
    assert_eq!((plan.order.idle, plan.order.retry), (0, 0));
    assert_eq!(
        plan.steps,
        [
            GuardInstallStep::AllocateOrder(GUARD_ORDER_INDEX),
            GuardInstallStep::AddOrder,
            GuardInstallStep::UpdateAction,
        ]
    );
}

#[test]
fn invalid_target_install_uses_actor_position_and_ffff_uid() {
    let request = GuardInstallRequest {
        actor: identity(4, 1, 9),
        actor_x: 333,
        actor_y: 777,
        target_o: -1,
        target_who: 2,
        dx: -1,
        dy: -1,
        queue_pos: QUEUE_FIRST,
        unused_arg6: -99,
    };
    let plan = plan_guard_install(request, None).unwrap();
    assert_eq!(plan.order.target, identity(-1, 2, u16::MAX));
    assert_eq!((plan.order.guard_x, plan.order.guard_y), (333, 777));
    assert!(plan.steps.ends_with(&[
        GuardInstallStep::ClearPartialPath,
        GuardInstallStep::RotateFirstOrderToHead,
        GuardInstallStep::UpdateAction,
    ]));
}

#[test]
fn queue_new_preflight_precedes_allocation_and_unused_arg_is_dead() {
    let mut request = GuardInstallRequest {
        actor: identity(4, 1, 9),
        actor_x: 10,
        actor_y: 20,
        target_o: -1,
        target_who: -1,
        dx: -1,
        dy: -1,
        queue_pos: QUEUE_NEW,
        unused_arg6: 1,
    };
    let first = plan_guard_install(request, None).unwrap();
    request.unused_arg6 = i32::MIN;
    let second = plan_guard_install(request, None).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        &first.steps[..5],
        &[
            GuardInstallStep::ClearActorMask(0x0400_0000),
            GuardInstallStep::StoreActorC0(0),
            GuardInstallStep::CloseOrders(0),
            GuardInstallStep::ClearPartialPath,
            GuardInstallStep::UpdateAction,
        ]
    );
}

#[test]
fn matching_guard_reissue_changes_only_formation_offsets() {
    let before = order();
    let after = update_existing_guard_offsets(before, i32::MIN, i32::MAX);
    assert_eq!((after.dx, after.dy), (i32::MIN, i32::MAX));
    assert_eq!(after.target, before.target);
    assert_eq!(
        (after.guard_x, after.guard_y),
        (before.guard_x, before.guard_y)
    );
    assert_eq!((after.idle, after.retry), (before.idle, before.retry));
}

#[test]
fn negative_target_order_animates_then_bare_kills() {
    let mut request = request();
    request.order.target.o = -1;
    request.target = None;
    request.spatial = None;
    request.post_move = None;
    let plan = plan_guard_executor(request).unwrap();
    assert_eq!(plan.branch, GuardExecutorBranch::TerminalInvalidTarget);
    assert_eq!(
        plan.steps,
        [
            GuardHostStep::SetAnimation {
                animation: 0,
                arg0: 0,
                arg1: 1
            },
            GuardHostStep::KillCurrentOrder { arg: 0 },
        ]
    );
}

#[test]
fn retry_decrements_only_retry_after_the_first_predicate_pair() {
    let mut request = request();
    request.order.retry = -3;
    request.spatial = None;
    request.post_move = None;
    // These second-pass facts are unreachable while retry is nonzero.
    request.target.as_mut().unwrap().valid_unit = false;
    request.target.as_mut().unwrap().on_map = Some(true);
    let plan = plan_guard_executor(request).unwrap();
    assert_eq!(plan.branch, GuardExecutorBranch::RetryDelay);
    assert_eq!(plan.order_after.retry, -4);
    assert_eq!(plan.order_after.idle, 9);
    assert!(plan.steps.is_empty());
}

#[test]
fn phase_eight_scan_precedes_phase_zero_idle_increment() {
    let mut melee = request();
    melee.target.as_mut().unwrap().is_moving = false;
    melee.actor.frame = 3; // actor.o 5 + frame 3 + 8 == 16
    melee.spatial = None;
    melee.post_move = None;
    let melee_plan = plan_guard_executor(melee).unwrap();
    assert_eq!(melee_plan.branch, GuardExecutorBranch::PeriodicMeleeScan);
    assert_eq!(melee_plan.order_after.idle, 9);
    assert_eq!(
        melee_plan.steps,
        [GuardHostStep::FindMeleeTarget {
            args: [-1, 0, 0, 1, 0],
        }]
    );

    let mut idle = melee;
    idle.actor.frame = 11; // actor.o 5 + frame 11 == 16
    let idle_plan = plan_guard_executor(idle).unwrap();
    assert_eq!(idle_plan.branch, GuardExecutorBranch::PeriodicIdlePulse);
    assert_eq!(idle_plan.order_after.idle, 10);
    assert!(idle_plan.steps.is_empty());
}

#[test]
fn wallbuild_branch_uses_exact_search_and_replacing_group_move_tuple() {
    let mut request = request();
    request.target.as_mut().unwrap().is_wallbuild = true;
    request.spatial.as_mut().unwrap().building_spot = Some(GuardSpotOutcome::Failed);
    request.post_move = None;
    let plan = plan_guard_executor(request).unwrap();
    assert_eq!(plan.branch, GuardExecutorBranch::ReplaceWithWallBuildMove);
    let GuardHostStep::FindNearbySpot {
        request: find,
        outcome,
    } = plan.steps[0]
    else {
        panic!("expected wallbuild search");
    };
    assert_eq!(outcome, GuardSpotOutcome::Failed);
    assert_eq!(
        (find.min_radius, find.max_radius, find.step),
        (8 * 48, 40 * 48, 0)
    );
    assert_eq!(
        (
            find.tail_12,
            find.tail_13,
            find.tail_14,
            find.tail_15,
            find.tail_16
        ),
        (0, 1, -1, 0, -1)
    );
    assert_eq!(
        plan.steps[1],
        GuardHostStep::GroupActionMoveNear(GroupMoveNearRequest {
            x: 1_000,
            y: 2_000,
            tail: [0, QUEUE_NEW, 0, 0, 1, 0, -1, -1, 0],
        })
    );
}

#[test]
fn invalid_projection_requires_both_search_results_and_pins_secondary_radius() {
    let mut request = request();
    request.target.as_mut().unwrap().is_moving = false;
    let spatial = request.spatial.as_mut().unwrap();
    spatial.invalid_loc = true;
    spatial.primary_spot = Some(GuardSpotOutcome::Failed);
    spatial.secondary_spot = Some(GuardSpotOutcome::Found { x: 700, y: 800 });
    spatial.find_angle = Some(0x77);
    let plan = plan_guard_executor(request).unwrap();
    let searches: Vec<_> = plan
        .steps
        .iter()
        .filter_map(|step| match step {
            GuardHostStep::FindNearbySpot { request, .. } => Some(*request),
            _ => None,
        })
        .collect();
    assert_eq!(searches.len(), 2);
    assert_eq!((searches[0].centre_x, searches[0].centre_y), (216, 312));
    assert_eq!(
        (
            searches[0].min_radius,
            searches[0].max_radius,
            searches[0].step
        ),
        (0xc0, 0x180, 0x60)
    );
    assert_eq!(
        (
            searches[1].min_radius,
            searches[1].max_radius,
            searches[1].step
        ),
        (51, 51 + 0x180, 0xc0)
    );

    let mut missing = request;
    missing.spatial.as_mut().unwrap().secondary_spot = None;
    assert_eq!(
        plan_guard_executor(missing),
        Err(GuardPlanError::MissingSecondarySpot)
    );
}

#[test]
fn off_cell_path_snaps_resets_idle_moves_same_tick_and_draws_retry() {
    let plan = plan_guard_executor(request()).unwrap();
    assert_eq!(plan.branch, GuardExecutorBranch::RetryAfterMove);
    assert_eq!(
        (plan.order_after.guard_x, plan.order_after.guard_y),
        (504, 984)
    );
    assert_eq!((plan.order_after.idle, plan.order_after.retry), (0, 8));
    assert_eq!(
        &plan.steps[0..3],
        &[
            GuardHostStep::AddMoveFacingOrder(AddMoveFacingRequest {
                x: 504,
                y: 984,
                angle: 0x1234_5678,
                tail: [2, 0, QUEUE_FIRST, 0, -1, -1, -1, 0],
            }),
            GuardHostStep::SetInsertedMovePause(90),
            GuardHostStep::UpdateOrderThenDoMove,
        ]
    );
    assert_eq!(
        plan.steps.last(),
        Some(&GuardHostStep::DrawGameRandom {
            low: 0,
            high: 0xffff,
            result: 5,
        })
    );
}

#[test]
fn random_is_not_consumed_until_same_tick_move_exposes_guard() {
    let mut request = request();
    request.post_move.as_mut().unwrap().head_is_guard = false;
    request.post_move.as_mut().unwrap().random_draw = None;
    let plan = plan_guard_executor(request).unwrap();
    assert_eq!(plan.branch, GuardExecutorBranch::MoveStillCurrent);
    assert!(!plan
        .steps
        .iter()
        .any(|step| matches!(step, GuardHostStep::DrawGameRandom { .. })));

    let mut spurious = request;
    spurious.post_move.as_mut().unwrap().random_draw = Some(1);
    assert_eq!(
        plan_guard_executor(spurious),
        Err(GuardPlanError::UnexpectedRandomDraw)
    );
}

#[test]
fn same_cell_turns_then_idles_and_fast_threshold_casts() {
    let mut request = request();
    request.target.as_mut().unwrap().is_moving = false;
    let spatial = request.spatial.as_mut().unwrap();
    spatial.actor_cell_x = 10;
    spatial.actor_cell_y = 20;
    spatial.guard_cell_x = 10;
    spatial.guard_cell_y = 20;
    spatial.find_angle = Some(0x5566);
    request.post_move = None;
    let idle = plan_guard_executor(request).unwrap();
    assert_eq!(idle.branch, GuardExecutorBranch::IdleAtGuardPoint);
    assert_eq!(idle.order_after.idle, 10);
    assert_eq!(
        idle.steps,
        [
            GuardHostStep::SetAngle {
                angle: 0x5566,
                update_position: 0
            },
            GuardHostStep::SetAnimation {
                animation: 0,
                arg0: 0,
                arg1: 1
            },
        ]
    );

    let mut cast = request;
    cast.order.idle = 29;
    cast.actor.type_flags_2b8 = 4;
    cast.actor.unit_masks = 0x0008_0000;
    cast.actor.is_type_7b = true;
    let cast = plan_guard_executor(cast).unwrap();
    assert_eq!(cast.branch, GuardExecutorBranch::AutoCast);
    assert_eq!(cast.order_after.idle, 30);
    assert_eq!(
        cast.steps.last(),
        Some(&GuardHostStep::AddCastOrder {
            target_o: -1,
            target_who: -1,
            x: -1,
            y: -1,
            cast_type: GUARD_CAST_TYPE,
            queue_pos: QUEUE_FIRST,
            arg7: 0,
        })
    );
}

#[test]
fn missing_and_unexpected_conditional_facts_fail_closed() {
    let mut missing = request();
    missing.target.as_mut().unwrap().initial_on_map = None;
    assert_eq!(
        plan_guard_executor(missing),
        Err(GuardPlanError::MissingInitialOnMap)
    );

    let mut unexpected = request();
    unexpected.target.as_mut().unwrap().valid_unit = false;
    assert_eq!(
        plan_guard_executor(unexpected),
        Err(GuardPlanError::UnexpectedSecondOnMap)
    );

    let mut range = request();
    range.post_move.as_mut().unwrap().random_draw = Some(0x1_0000);
    assert_eq!(
        plan_guard_executor(range),
        Err(GuardPlanError::RandomDrawOutOfRange)
    );
}

#[test]
fn receipt_recomputes_and_binds_every_fact_including_ignored_uid() {
    let request = request();
    let receipt = GuardExecutorReceipt::applied(request).unwrap();
    assert!(receipt.validates(request));

    let mut changed = request;
    changed.target.as_mut().unwrap().identity.uid ^= 1;
    assert!(!receipt.validates(changed));

    let mut spliced = receipt.clone();
    spliced.plan.as_mut().unwrap().steps.swap(0, 1);
    assert!(!spliced.validates(request));

    let unavailable = GuardExecutorReceipt::unavailable(request);
    assert!(unavailable.validates(request));
    assert!(unavailable.plan.is_none());
}

#[test]
fn every_unresolved_runtime_and_integration_tail_is_explicit() {
    assert_eq!(
        GUARD_OPEN_TAILS,
        [
            GuardOpenTail::GroupActionGuardFormationAndUpdate,
            GuardOpenTail::ObjectVirtualPredicates,
            GuardOpenTail::TrigDiv3TerrainAndInvalidLoc,
            GuardOpenTail::FindMeleeTarget,
            GuardOpenTail::FindNearbySpot,
            GuardOpenTail::TemporaryGroupActionMoveNear,
            GuardOpenTail::AddMoveFacingAndSameTickDoMove,
            GuardOpenTail::AutoCastInsertion,
            GuardOpenTail::CanonicalGameRandom,
            GuardOpenTail::ConcretePayloadSaveAndLiveTickAdapter,
        ]
    );
}
