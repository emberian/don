// SPDX-License-Identifier: GPL-3.0-or-later
#[path = "../src/systems/trade_order_frontier.rs"]
mod trade;

use trade::*;

fn id(o: i32, who: i32, uid: u16) -> TradeIdentity {
    TradeIdentity { o, who, uid }
}

fn road(status: RoadBuildStatus) -> RoadBuildReceipt {
    RoadBuildReceipt {
        status,
        search_before: 10,
        search_after: 11,
        road_before: 20,
        road_after: if status == RoadBuildStatus::Built {
            21
        } else {
            20
        },
        terrain_epoch_before: 30,
        terrain_epoch_after: 31,
        rng_epoch_before: 40,
        rng_epoch_after: 43,
        draw_count: 3,
    }
}

fn endpoint(identity: TradeIdentity, city: i32, x: i32) -> TradeEndpointFacts {
    TradeEndpointFacts {
        identity,
        live: true,
        is_build: true,
        city,
        canonical_o: identity.o,
        city_live: true,
        unloaded_departure_gate: true,
        x,
        y: 200,
        city_x: x + 11,
        city_y: 211,
        x_size: 2,
        y_size: 3,
        empty_route: true,
    }
}

fn base() -> TradeFrameFacts {
    let first = id(10, 0, 100);
    let second = id(20, 0, 200);
    TradeFrameFacts {
        actor: TradeActorFacts {
            identity: id(7, 0, 70),
            x: 4_000,
            y: 4_000,
            flags: 0,
            caravan_slot: 2,
            inside: false,
            has_order_after_kill: false,
        },
        order: TradeOrderState {
            first,
            second,
            started: 1,
            loaded: 0,
            flags: 0,
        },
        caravan: Some(TradeCaravanFacts {
            owner: 0,
            slot: 2,
            flags: 3,
            making_road: 0,
            route_digest: 99,
        }),
        first: Some(endpoint(first, 3, 1_000)),
        second: Some(endpoint(second, 4, 2_000)),
        candidates: None,
        source_transport_compatible: Some(true),
        transport_compatible: Some(true),
        route_admission: None,
        movement: Some(RoadMovementFacts::Nearby {
            return_nonzero: false,
            x: 2_100,
            y: 200,
            actor_distance: 100,
        }),
        recovery: None,
    }
}

#[test]
fn payload_layout_defaults_and_walk_ranges_are_exact() {
    assert_eq!(TRADE_ORDER_INDEX, 15);
    assert_eq!(UNIT_DO_TRADE_VA, 0x005e_d270);
    assert_eq!(UNIT_DO_TRADE_BYTES, 4_519);
    assert_eq!(UNIT_DO_TRADE_END_VA, 0x005e_e417);
    assert_eq!(TRADE_ORDER_SIZE, 52);
    assert_eq!(TRADE_ORDER_WALKED_BYTES, 29);
    assert_eq!(TRADE_LIST_NODE_WALKED_BYTES, 34);
    assert_eq!(TradeOrderState::default().first, TradeIdentity::NONE);
    assert_eq!(TradeOrderState::default().second, TradeIdentity::NONE);
    assert_eq!(
        TRADE_WALK_RANGES,
        [(0x30, 0x31), (0x08, 0x12), (0x14, 0x26)]
    );
    assert_eq!(
        TRADE_WALK_RANGES.iter().map(|(a, b)| b - a).sum::<usize>(),
        29
    );
    assert_eq!(offsets::VTORDISP, 0x28);
    assert_eq!(offsets::UNIT_ORDER_VBASE, 0x2c);
    assert!(!GROUP_QUEUE_FIRST_COPIES_TRADE_ORDER);
}

#[test]
fn installer_clears_route_before_allocating_and_preserves_both_uids() {
    let steps = plan_trade_install(TradeInstallRequest {
        first: id(10, 1, 100),
        second: id(20, 2, 200),
        queue: QUEUE_NEW,
        group: true,
        mark_transport_route: true,
    });
    assert_eq!(steps[0], TradeInstallStep::ClearNewOrderState);
    assert_eq!(steps[1], TradeInstallStep::ClearUnitTradeRouteFlag);
    assert_eq!(steps[2], TradeInstallStep::AllocateOrder(15));
    assert_eq!(steps[3], TradeInstallStep::StoreFirst(id(10, 1, 100)));
    assert_eq!(steps[4], TradeInstallStep::StoreLoaded(0));
    assert_eq!(steps[5], TradeInstallStep::StoreSecond(id(20, 2, 200)));
    assert_eq!(steps[6], TradeInstallStep::StoreStarted(0));
    assert!(steps.contains(&TradeInstallStep::SetTransportRouteFlag));
    assert_eq!(installed_primary_uid(10, -1, 100), u16::MAX);
    assert_eq!(installed_second_uid(20, 1, -1, 200), 200);
    assert_eq!(installed_second_uid(20, -1, 2, 200), u16::MAX);
}

fn candidate(owner: i32, city: i32, score: i32) -> TradeCandidate {
    TradeCandidate {
        owner,
        city,
        endpoint: id(city + 100, owner, city as u16),
        leader_live: true,
        allied: true,
        actor_has_foreign_prereq: true,
        city_live: true,
        seen: true,
        candidate_empty_route_result: 1,
        source_empty_route_result: 1,
        transport_compatible: true,
        score,
    }
}

#[test]
fn destination_fold_is_strict_greater_and_rejects_foreign_without_source_authority() {
    let context = TradeSelectionContext {
        actor_owner: 0,
        source_owner: 0,
        source_city: 1,
    };
    let candidates = [candidate(0, 1, 999), candidate(0, 2, 7), candidate(0, 3, 7)];
    let selected = select_trade_destination(context, &candidates).unwrap();
    assert_eq!(
        selected.scan_index, 1,
        "source is excluded and a tie keeps the first"
    );

    let mut foreign = candidate(2, 0, 100);
    foreign.actor_has_foreign_prereq = false;
    assert!(select_trade_destination(context, &[foreign]).is_none());

    let mut duplicate_quirk = candidate(0, 4, 3);
    duplicate_quirk.candidate_empty_route_result = -1;
    assert!(select_trade_destination(context, &[duplicate_quirk]).is_some());
}

#[test]
fn missing_destination_retires_with_idle_99_and_transport_recovery() {
    let mut request = base();
    request.order.second = TradeIdentity::NONE;
    request.second = None;
    request.candidates = Some(vec![]);
    request.actor.flags = UNIT_TRANSPORT_RECOVERY;
    request.recovery = Some(TradeRecoveryFacts {
        local_feedback: true,
        transport_next_city: Some(id(8, 0, 80)),
    });
    let plan = plan_trade_executor(&request).unwrap();
    assert_eq!(plan.branch, TradeExecutorBranch::NoDestination);
    assert!(plan.steps.contains(&TradeHostStep::SetIdle(99)));
    assert!(plan
        .steps
        .contains(&TradeHostStep::KillCurrentOrder { arg: 0 }));
    assert!(plan.steps.contains(&TradeHostStep::GoToCity {
        city: 8,
        owner: 0,
        queue: QUEUE_LAST,
    }));
    assert!(!plan
        .steps
        .iter()
        .any(|step| matches!(step, TradeHostStep::ThinkCaravan { .. })));
}

#[test]
fn establishment_orients_near_endpoint_first_then_publishes_links_flags_and_income() {
    let mut request = base();
    request.order.started = 0;
    request.actor.x = 900;
    request.actor.y = 200;
    request.route_admission = Some(RouteAdmissionFacts {
        actor_has_foreign_prereq: false,
        source_empty_route_result: 1,
        destination_empty_route_result: 1,
        actor_distance_to_source: 100,
        actor_distance_to_destination: 1_100,
        road: road(RoadBuildStatus::Built),
    });
    request.movement = Some(RoadMovementFacts::Stack {
        road_before: 5,
        road_after: 6,
        unit_path_before: 7,
        unit_path_after: 8,
        add_move_x: 1_500,
        add_move_y: 200,
        smoothed_vertices: 2,
        pushed_join: true,
    });
    let plan = plan_trade_executor(&request).unwrap();
    assert_eq!(plan.order_after.started, 1);
    let endpoint_step = plan.steps.iter().find_map(|step| match step {
        TradeHostStep::StoreCaravanEndpoints { first, second } => Some((*first, *second)),
        _ => None,
    });
    assert_eq!(
        endpoint_step,
        Some((
            TradeCityEndpoint { city: 3, owner: 0 },
            TradeCityEndpoint { city: 4, owner: 0 },
        ))
    );
    assert!(plan.steps.contains(&TradeHostStep::SetCaravanFlags(3)));
    assert!(plan
        .steps
        .contains(&TradeHostStep::BuildRoad(road(RoadBuildStatus::Built))));
}

#[test]
fn parked_road_search_inserts_direct_move_and_returns_before_arrival() {
    let mut request = base();
    request.order.started = 0;
    request.route_admission = Some(RouteAdmissionFacts {
        actor_has_foreign_prereq: false,
        source_empty_route_result: 1,
        destination_empty_route_result: 1,
        actor_distance_to_source: 500,
        actor_distance_to_destination: 100,
        road: road(RoadBuildStatus::SearchPending),
    });
    let plan = plan_trade_executor(&request).unwrap();
    assert_eq!(plan.branch, TradeExecutorBranch::RoadSearchPending);
    assert!(plan
        .steps
        .contains(&TradeHostStep::AddMoveOrder { x: 2_011, y: 211 }));
    assert!(!plan
        .steps
        .iter()
        .any(|step| matches!(step, TradeHostStep::FindNearbySpot { .. })));
}

#[test]
fn loaded_arrival_activates_income_before_advancing_the_route() {
    let mut request = base();
    request.order.loaded = 1;
    request.actor.x = 1_000;
    request.actor.y = 200;
    let plan = plan_trade_executor(&request).unwrap();
    assert_eq!(plan.branch, TradeExecutorBranch::FirstEndpointTransition);
    assert_eq!(plan.order_after.loaded, 0);
    let loaded = plan
        .steps
        .iter()
        .position(|step| *step == TradeHostStep::SetOrderLoaded(0))
        .unwrap();
    let active = plan
        .steps
        .iter()
        .position(|step| *step == TradeHostStep::SetCaravanFlags(7))
        .unwrap();
    let movement = plan
        .steps
        .iter()
        .position(|step| matches!(step, TradeHostStep::FindNearbySpot { .. }))
        .unwrap();
    assert!(loaded < active && active < movement);
    assert!(plan.steps.contains(&TradeHostStep::SetUnitTradeRouteFlag));
}

#[test]
fn nearby_result_beyond_exact_pad_holds_without_fake_move() {
    let mut request = base();
    let second = request.second.unwrap();
    let base_radius = second.x_size.max(second.y_size) * 0x60;
    request.movement = Some(RoadMovementFacts::Nearby {
        return_nonzero: true,
        x: 2_500,
        y: 200,
        actor_distance: base_radius + TRADE_NEARBY_ACCEPT_PAD + 1,
    });
    let plan = plan_trade_executor(&request).unwrap();
    assert_eq!(plan.branch, TradeExecutorBranch::HoldOutsideNearbyRadius);
    assert!(!plan
        .steps
        .iter()
        .any(|step| matches!(step, TradeHostStep::AddMoveOrder { .. })));
}

#[test]
fn nearby_return_value_is_ignored_when_written_point_is_in_range() {
    let mut request = base();
    request.movement = Some(RoadMovementFacts::Nearby {
        return_nonzero: true,
        x: 1_050,
        y: 200,
        actor_distance: 50,
    });
    let plan = plan_trade_executor(&request).unwrap();
    assert_eq!(plan.branch, TradeExecutorBranch::Moving);
    assert!(plan
        .steps
        .contains(&TradeHostStep::AddMoveOrder { x: 1_050, y: 200 }));
    assert_eq!(
        plan.steps[0],
        TradeHostStep::SetAnimation {
            animation: 0,
            arg0: 0,
            arg1: 1,
        }
    );
}

#[test]
fn road_receipt_and_whole_snapshot_are_revalidated() {
    let mut bad = road(RoadBuildStatus::Built);
    bad.rng_epoch_after += 1;
    assert!(!bad.valid());

    let snapshot = TradeAtomicSnapshot {
        actor_version: 1,
        order_version: 2,
        object_epoch: 3,
        leader_epoch: 4,
        city_epoch: 5,
        caravan_epoch: 6,
        path_epoch: 7,
        terrain_epoch: 8,
        rng_epoch: 9,
        effect_epoch: 10,
        facts: base(),
    };
    let receipt = TradeExecutorReceipt::preflight(snapshot.clone()).unwrap();
    assert!(receipt.validates(&snapshot));
    let mut changed = snapshot;
    changed.path_epoch += 1;
    assert!(!receipt.validates(&changed));
}

#[test]
fn closure_stays_red_until_every_runtime_tail_is_owned() {
    assert_eq!(TRADE_OPEN_TAILS.len(), 11);
    assert!(TRADE_OPEN_TAILS.contains(&TradeOpenTail::QueueFirstCopyQuirk));
    assert!(TRADE_OPEN_TAILS.contains(&TradeOpenTail::CaravanRoadAStarParkedStateAndRng));
    assert!(TRADE_OPEN_TAILS.contains(&TradeOpenTail::LiveTickAtomicCommit));
}
