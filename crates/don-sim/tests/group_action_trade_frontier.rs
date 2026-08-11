// SPDX-License-Identifier: GPL-3.0-or-later
#[path = "../src/systems/group_action_trade_frontier.rs"]
mod trade;

use trade::*;

fn key(o: i32, who: i32) -> ObjectKey {
    ObjectKey { o, who }
}

fn id(o: i32, who: i32, uid: u16) -> ObjectIdentity {
    ObjectIdentity {
        key: key(o, who),
        uid,
    }
}

fn region(object: ObjectIdentity, tregion: i32, digest: u64) -> TerrainRegionReceipt {
    TerrainRegionReceipt {
        object,
        x: object.key.o * 100,
        y: object.key.o * 100 + 10,
        tile_x: object.key.o * 3,
        tile_y: object.key.o * 3 + 1,
        tregion,
        world_before_digest: digest,
        world_after_digest: digest,
        complete: true,
    }
}

fn group() -> TradeGroupSnapshot {
    TradeGroupSnapshot {
        group_slot: 4,
        id: 77,
        owner: 2,
        num: 2,
        form: 8,
        disband: 9,
        members: vec![10, 11],
    }
}

fn scenario(group: TradeGroupSnapshot) -> ScenarioPruneFacts {
    ScenarioPruneFacts {
        ignore_orders: false,
        ignored_objects: Vec::new(),
        calls: Vec::new(),
        group_after: group,
        state_before_digest: 10,
        state_after_digest: 11,
        complete: true,
    }
}

fn target() -> TradeTargetGateFacts {
    TradeTargetGateFacts {
        identity: id(30, 3, 300),
        live_building: true,
        active: Some(true),
        entry_receiver_is_trade: Some(true),
        entry_receiver_digest: Some(0xabc),
    }
}

fn skipped(o: i32) -> TradeMemberFacts {
    TradeMemberFacts {
        identity: id(o, 2, o as u16 + 1_000),
        live_unit: false,
        on_map: None,
        region: None,
        target_is_trade: None,
        is_caravan: None,
        target_is_sea_trade: None,
        is_sea_trade_member: None,
        regions_touch: None,
        install: None,
    }
}

fn caravan(o: i32) -> TradeMemberFacts {
    let actor = id(o, 2, o as u16 + 1_000);
    TradeMemberFacts {
        identity: actor,
        live_unit: true,
        on_map: Some(true),
        region: Some(region(actor, 5, 50)),
        target_is_trade: Some(true),
        is_caravan: Some(true),
        target_is_sea_trade: None,
        is_sea_trade_member: None,
        regions_touch: None,
        install: Some(TradeInstallFacts {
            actor,
            unit_flags_before: UNIT_ACTIVE_ORDER_MASK
                | UNIT_HAS_TRADE_ROUTE_MASK
                | UNIT_TRANSPORT_ROUTE_MASK,
            primary_lookup: Some(id(30, 3, 301)),
            secondary_lookup: Some(id(40, 4, 401)),
            primary_region: region(id(30, 3, 301), 9, 60),
            actor_region: region(actor, 5, 61),
            leader_flags: Some(0x100),
            transport_type: Some(3),
            can_ever_transport: Some(true),
            orders_before_digest: 101,
            partial_path_before_digest: 102,
            action_before_digest: 103,
        }),
    }
}

fn direct_invocation(entry: TradeGroupSnapshot) -> TradeInvocationFacts {
    TradeInvocationFacts {
        scenario: scenario(entry),
        target: Some(target()),
        count_land: Some(1),
        count_sea: None,
        direct: Some(DirectTradeFacts {
            target_region: region(id(30, 3, 300), 9, 49),
            members: vec![caravan(10), skipped(11)],
        }),
        queue_first: None,
    }
}

fn action(queued: i32) -> RawTradeAction {
    RawTradeAction {
        ox: 30,
        whom: 3,
        oxx: 40,
        whose: 4,
        queued,
    }
}

fn request(queued: i32) -> GroupActionTradeRequest {
    GroupActionTradeRequest {
        group: group(),
        action: action(queued),
    }
}

fn facts(invocation: TradeInvocationFacts) -> GroupActionTradeFacts {
    GroupActionTradeFacts {
        evidence: RetailTradeEvidence::SHIPPED,
        invocation,
    }
}

#[test]
fn actual_pe_pdb_capstone_identity_and_layout_are_frozen() {
    assert_eq!(GROUP_ACTION_TRADE_VA, 0x0070_1cc0);
    assert_eq!(GROUP_ACTION_TRADE_BYTES, 1_022);
    assert_eq!(GROUP_ACTION_TRADE_END_VA, 0x0070_20be);
    assert_eq!(GROUP_ACTION_TRADE_INSTRUCTIONS, 299);
    assert_eq!(TRADE_ROUTE_ORDER_SIZE, 52);
    assert_eq!(TRADE_ROUTE_ORDER_INDEX, 15);
    assert_eq!(
        [
            GROUP_ACTION_BEGIN_VA,
            GROUPDATA_COUNT_VA,
            WORLD_GET_TREGION_VA,
            REGION_IS_COAST_VA,
            UNIT_ADD_TRADE_ORDER_VA,
            UNIT_CLOSE_ORDERS_VA,
            UNIT_CLEAR_PARTIAL_PATH_VA,
            UNIT_UPDATE_ACTION_VA,
            ORDERS_GET_OBJ_VA,
            ORDER_LIST_ADD_VA,
            UNIT_TRANSPORT_TYPE_VA,
            UNIT_CAN_EVER_TRANSPORT_VA,
        ],
        [
            0x0071_4100,
            0x0071_1720,
            0x006b_52e0,
            0x0068_0f90,
            0x005e_4dc0,
            0x005e_37f0,
            0x005e_3920,
            0x0060_a870,
            0x0073_0ac0,
            0x0046_d5a0,
            0x0046_f790,
            0x0046_f290,
        ]
    );
    assert_eq!(RetailTradeEvidence::SHIPPED.executable_sha256.len(), 64);
    assert_eq!(RetailTradeEvidence::SHIPPED.pdb_sha256.len(), 64);
    assert!(!ACTION_TRADE_READS_CITY);
    assert!(!ACTION_TRADE_READS_GOOD);
    assert!(!ACTION_TRADE_READS_RNG);
    assert!(!QUEUE_FIRST_COPIES_TRADE_ROUTE);
}

#[test]
fn direct_new_installs_full_two_endpoint_payload_in_retail_order() {
    let tx = plan_group_action_trade(request(QUEUE_NEW), facts(direct_invocation(group())))
        .expect("coherent capture");

    assert_eq!(tx.outcome, TradeOutcome::Completed { installed: 1 });
    assert_eq!(tx.atomic_residual_va, 0x0070_1e8c);
    assert_eq!(tx.group_after.authority, GroupAfterAuthority::Exact);
    assert_eq!(tx.group_after.last_known.disband, 0);
    assert_eq!(tx.group_after.last_known.form, -1);

    let install = tx
        .events
        .iter()
        .find_map(|event| match event {
            TradeActionEvent::AddTradeOrder(plan) => Some(plan),
            _ => None,
        })
        .expect("one accepted caravan");
    assert_eq!(install.actor, id(10, 2, 1_010));
    assert_eq!(install.queue, QUEUE_NEW);
    assert_eq!(
        install.payload,
        TradeRouteOrderPayload {
            first: id(30, 3, 301),
            second: id(40, 4, 401),
            started: 0,
            loaded: 0,
            flags: TRADE_GROUP_FLAG,
        }
    );
    assert_eq!(
        install.steps,
        vec![
            TradeInstallStep::ClearActiveOrderMask,
            TradeInstallStep::ClearPathAnchor,
            TradeInstallStep::CloseOrders { arg: 0 },
            TradeInstallStep::ClearPartialPath,
            TradeInstallStep::UpdateActionBeforeInstall,
            TradeInstallStep::ClearTradeRouteMask,
            TradeInstallStep::AllocateOrder {
                index: 15,
                size: 52,
            },
            TradeInstallStep::StoreFirst(id(30, 3, 301)),
            TradeInstallStep::StoreLoaded(0),
            TradeInstallStep::StoreSecond(id(40, 4, 401)),
            TradeInstallStep::StoreStarted(0),
            TradeInstallStep::SetGroupFlag,
            TradeInstallStep::ResolvePrimaryRegion(region(id(30, 3, 301), 9, 60)),
            TradeInstallStep::ResolveActorRegion(region(id(10, 2, 1_010), 5, 61)),
            TradeInstallStep::ReadTransportCapability {
                leader_flags: 0x100,
                capability: 3,
            },
            TradeInstallStep::ReadTransportType(3),
            TradeInstallStep::ReadCanEverTransport(true),
            TradeInstallStep::SetTransportRouteMask,
            TradeInstallStep::AppendOrder,
            TradeInstallStep::UpdateActionAfterInstall,
        ]
    );
    assert_eq!(
        install.unit_flags_after_projection,
        UNIT_TRANSPORT_ROUTE_MASK
    );

    let labels: Vec<_> = tx
        .events
        .iter()
        .map(|event| match event {
            TradeActionEvent::ScenarioPrune(_) => "scenario",
            TradeActionEvent::ActionBegin { .. } => "begin",
            TradeActionEvent::ResolvePrimary(_) => "target",
            TradeActionEvent::TargetBuildingGate(_) => "build",
            TradeActionEvent::TargetActiveGate(_) => "active",
            TradeActionEvent::TargetEntryTradeGate { .. } => "entry-trade",
            TradeActionEvent::CountGroup { .. } => "count",
            TradeActionEvent::SetGroupForm { .. } => "form",
            TradeActionEvent::ResolveTargetRegion(_) => "target-region",
            TradeActionEvent::ResolveMember(_) => "member",
            TradeActionEvent::MemberUnitGate { .. } => "unit",
            TradeActionEvent::MemberOnMapGate { .. } => "on-map",
            TradeActionEvent::ResolveMemberRegion { .. } => "member-region",
            TradeActionEvent::TargetTradeGate { .. } => "target-kind",
            TradeActionEvent::MemberCaravanGate { .. } => "caravan",
            TradeActionEvent::AddTradeOrder(_) => "install",
            TradeActionEvent::SkipMember { .. } => "skip",
            _ => "unexpected",
        })
        .collect();
    assert_eq!(
        &labels[..16],
        &[
            "scenario",
            "begin",
            "target",
            "build",
            "active",
            "entry-trade",
            "count",
            "form",
            "target-region",
            "member",
            "unit",
            "on-map",
            "member-region",
            "target-kind",
            "caravan",
            "install",
        ]
    );
}

#[test]
fn sea_and_unclassified_destinations_preserve_the_distinct_member_cones() {
    let mut sea_member = caravan(10);
    sea_member.target_is_trade = Some(false);
    sea_member.is_caravan = None;
    sea_member.target_is_sea_trade = Some(true);
    sea_member.is_sea_trade_member = Some(true);
    sea_member.regions_touch = Some(false);
    sea_member.install = None;
    let mut invocation = direct_invocation(group());
    invocation.direct.as_mut().unwrap().members = vec![sea_member, skipped(11)];
    let tx = plan_group_action_trade(request(QUEUE_LAST), facts(invocation)).unwrap();
    assert_eq!(tx.outcome, TradeOutcome::Completed { installed: 0 });
    assert!(tx.events.iter().any(|event| matches!(
        event,
        TradeActionEvent::SkipMember {
            reason: TradeMemberGate::RegionsDoNotTouch,
            ..
        }
    )));

    let mut free_member = caravan(10);
    free_member.target_is_trade = Some(false);
    free_member.is_caravan = None;
    free_member.target_is_sea_trade = Some(false);
    free_member.is_sea_trade_member = None;
    free_member.regions_touch = None;
    free_member.install.as_mut().unwrap().leader_flags = Some(0);
    free_member.install.as_mut().unwrap().transport_type = Some(1);
    free_member.install.as_mut().unwrap().can_ever_transport = None;
    let mut invocation = direct_invocation(group());
    invocation.direct.as_mut().unwrap().members = vec![free_member, skipped(11)];
    let tx = plan_group_action_trade(request(QUEUE_LAST), facts(invocation)).unwrap();
    assert_eq!(tx.outcome, TradeOutcome::Completed { installed: 1 });
    let install = tx
        .events
        .iter()
        .find_map(|event| match event {
            TradeActionEvent::AddTradeOrder(plan) => Some(plan),
            _ => None,
        })
        .unwrap();
    assert_eq!(install.queue, QUEUE_LAST);
    assert!(!install
        .steps
        .contains(&TradeInstallStep::ClearActiveOrderMask));
    assert!(!install
        .steps
        .contains(&TradeInstallStep::SetTransportRouteMask));
}

#[test]
fn every_entry_refusal_keeps_form_and_still_commits_action_begin() {
    let mut invocation = direct_invocation(group());
    invocation.target = Some(TradeTargetGateFacts {
        identity: id(30, 3, 300),
        live_building: false,
        active: None,
        entry_receiver_is_trade: None,
        entry_receiver_digest: None,
    });
    invocation.count_land = None;
    invocation.direct = None;
    let tx = plan_group_action_trade(request(QUEUE_NEW), facts(invocation)).unwrap();
    assert_eq!(
        tx.outcome,
        TradeOutcome::Refused(TradeRefusal::TargetNotBuilding)
    );
    assert_eq!(tx.group_after.last_known.form, 8);
    assert_eq!(tx.group_after.last_known.disband, 0);
    assert_eq!(
        tx.events
            .iter()
            .filter(|event| matches!(event, TradeActionEvent::ActionBegin { .. }))
            .count(),
        1
    );
    assert!(!tx
        .events
        .iter()
        .any(|event| matches!(event, TradeActionEvent::SetGroupForm { .. })));
}

fn boundary(
    kind: QueueBoundaryKind,
    entry_va: u32,
    residual_va: u32,
    before: u64,
    after: u64,
) -> QueueBoundaryReceipt {
    QueueBoundaryReceipt {
        kind,
        entry_va,
        residual_va,
        state_before_digest: before,
        state_after_digest: after,
        complete: true,
    }
}

fn queue_first_invocation(saved_order_indices: Vec<i32>) -> TradeInvocationFacts {
    let outer_group = group();
    let mut halted = group();
    halted.disband = 0;
    halted.form = -1;
    let recursive = direct_invocation(halted.clone());
    TradeInvocationFacts {
        scenario: scenario(outer_group),
        target: Some(target()),
        count_land: Some(0),
        count_sea: Some(2),
        direct: None,
        queue_first: Some(QueueFirstFacts {
            saved_order_indices,
            set_up_insert: boundary(
                QueueBoundaryKind::SetUpInsert,
                GROUP_SET_UP_INSERT_VA,
                GROUP_SET_UP_INSERT_VA + 256,
                100,
                101,
            ),
            action_halt: boundary(
                QueueBoundaryKind::ActionHalt,
                GROUP_ACTION_HALT_VA,
                GROUP_ACTION_HALT_VA + 685,
                101,
                102,
            ),
            group_after_halt: halted,
            recursive_trade: boundary(
                QueueBoundaryKind::RecursiveTrade,
                GROUP_ACTION_TRADE_VA,
                GROUP_ACTION_TRADE_END_VA,
                102,
                103,
            ),
            recursive: Box::new(recursive),
            finish_insert: boundary(
                QueueBoundaryKind::FinishInsert,
                GROUP_FINISH_INSERT_VA,
                GROUP_FINISH_INSERT_VA + 1_104,
                103,
                104,
            ),
            release_saved_list: boundary(
                QueueBoundaryKind::ReleaseSavedList,
                ORDER_LIST_CLEAR_VA,
                ORDER_LIST_CLEAR_VA + 236,
                104,
                105,
            ),
        }),
    }
}

#[test]
fn queue_first_is_save_halt_recursive_new_finish_release_and_never_copies_trade() {
    let invocation = queue_first_invocation(vec![0, 3, 16]);
    let tx = plan_group_action_trade(request(QUEUE_FIRST), facts(invocation)).unwrap();
    assert_eq!(tx.outcome, TradeOutcome::QueueFirst { installed: 1 });
    assert_eq!(tx.group_after.authority, GroupAfterAuthority::EconomyHost);

    let boundaries: Vec<_> = tx
        .events
        .iter()
        .filter_map(|event| match event {
            TradeActionEvent::QueueBoundary(receipt) => Some(receipt.kind),
            _ => None,
        })
        .collect();
    assert_eq!(
        boundaries,
        vec![
            QueueBoundaryKind::SetUpInsert,
            QueueBoundaryKind::ActionHalt,
            QueueBoundaryKind::RecursiveTrade,
            QueueBoundaryKind::FinishInsert,
            QueueBoundaryKind::ReleaseSavedList,
        ]
    );
    let install = tx
        .events
        .iter()
        .find_map(|event| match event {
            TradeActionEvent::AddTradeOrder(plan) => Some(plan),
            _ => None,
        })
        .unwrap();
    assert_eq!(install.queue, QUEUE_NEW);

    let error = plan_group_action_trade(
        request(QUEUE_FIRST),
        facts(queue_first_invocation(vec![0, TRADE_ROUTE_ORDER_INDEX, 16])),
    )
    .unwrap_err();
    assert_eq!(error, TradePlanError::QueueFirstCopiedTradeRoute);
}

#[test]
fn wrong_order_state_or_call_is_rejected_atomically() {
    let request = request(QUEUE_FIRST);
    let request_before = request.clone();
    let mut invocation = queue_first_invocation(vec![0, 16]);
    invocation
        .queue_first
        .as_mut()
        .unwrap()
        .finish_insert
        .state_before_digest = 999;
    assert_eq!(
        plan_group_action_trade(request.clone(), facts(invocation)).unwrap_err(),
        TradePlanError::QueueBoundaryChronology
    );
    assert_eq!(request, request_before);

    let mut invocation = queue_first_invocation(vec![0, 16]);
    invocation
        .queue_first
        .as_mut()
        .unwrap()
        .action_halt
        .entry_va = GROUP_ACTION_HALT_VA + 1;
    assert_eq!(
        plan_group_action_trade(request.clone(), facts(invocation)).unwrap_err(),
        TradePlanError::QueueBoundaryAddress
    );
    assert_eq!(request, request_before);

    let mut invocation = direct_invocation(group());
    invocation
        .direct
        .as_mut()
        .unwrap()
        .target_region
        .world_after_digest += 1;
    assert_eq!(
        plan_group_action_trade(
            GroupActionTradeRequest {
                action: action(QUEUE_NEW),
                ..request.clone()
            },
            facts(invocation)
        )
        .unwrap_err(),
        TradePlanError::IncompleteExternalBoundary
    );
    assert_eq!(request, request_before);

    let mut invocation = direct_invocation(group());
    let member = &mut invocation.direct.as_mut().unwrap().members[0];
    member.is_caravan = Some(false);
    assert_eq!(
        plan_group_action_trade(
            GroupActionTradeRequest {
                action: action(QUEUE_NEW),
                ..request.clone()
            },
            facts(invocation)
        )
        .unwrap_err(),
        TradePlanError::MemberFactShape { index: 0 }
    );
    assert_eq!(request, request_before);
}

#[test]
fn uid2_uses_primary_owner_gate_and_malformed_second_owner_is_a_typed_crash_boundary() {
    let mut invocation = direct_invocation(group());
    let member = &mut invocation.direct.as_mut().unwrap().members[0];
    member.install.as_mut().unwrap().secondary_lookup = None;
    let mut raw = action(QUEUE_NEW);
    raw.oxx = -1;
    let tx = plan_group_action_trade(
        GroupActionTradeRequest {
            group: group(),
            action: raw,
        },
        facts(invocation),
    )
    .unwrap();
    let payload = tx
        .events
        .iter()
        .find_map(|event| match event {
            TradeActionEvent::AddTradeOrder(plan) => Some(plan.payload),
            _ => None,
        })
        .unwrap();
    assert_eq!(payload.second.uid, u16::MAX);

    let mut invocation = direct_invocation(group());
    invocation.direct.as_mut().unwrap().members[0]
        .install
        .as_mut()
        .unwrap()
        .secondary_lookup = Some(id(40, -1, 401));
    let mut raw = action(QUEUE_NEW);
    raw.whose = -1;
    assert_eq!(
        plan_group_action_trade(
            GroupActionTradeRequest {
                group: group(),
                action: raw,
            },
            facts(invocation),
        )
        .unwrap_err(),
        TradePlanError::RetailCrashDomain
    );
}
