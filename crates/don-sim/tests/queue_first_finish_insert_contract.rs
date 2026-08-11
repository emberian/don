#[path = "../src/systems/queue_first_finish_insert_contract.rs"]
mod contract;

use contract::*;

fn gather(token: u32, group: bool) -> LeaderOrderSnapshot {
    LeaderOrderSnapshot {
        token,
        kind: OrderKind::Gather,
        flags: if group { ORDER_GROUP } else { 0 },
        clone_image_complete: true,
        finish_payload: Some(FinishPayload::Gather {
            target_o: token as i32 + 100,
            target_who: 8,
            target_uid: token as u16 + 500,
            history: GatherHistory {
                tx: token as i32,
                ty: token as i32 + 1,
                build_type: 0x1a4,
                wait: -7,
                goto_build: 0,
                non_flat_gather: 1,
                dist_mod: 10,
                been_there: 1,
            },
        }),
    }
}

fn cast(token: u32, paid: i32) -> LeaderOrderSnapshot {
    LeaderOrderSnapshot {
        token,
        kind: OrderKind::CastSpell,
        flags: ORDER_GROUP,
        clone_image_complete: true,
        finish_payload: Some(FinishPayload::Cast {
            spell: 0x293,
            target_o: 44,
            target_who: 2,
            x: 0x1200,
            y: 0x2400,
            paid,
        }),
    }
}

fn request(orders: Vec<LeaderOrderSnapshot>) -> QueueFirstRequest {
    QueueFirstRequest {
        leader_found: true,
        leader_queue_complete: true,
        physical_links_complete: true,
        new_order_token: 99,
        leader_execution_order: orders,
    }
}

#[test]
fn pe_addresses_and_both_jump_tables_pin_the_instruction_contract() {
    assert_eq!(
        GROUP_SET_UP_INSERT_VA + GROUP_SET_UP_INSERT_BYTES as u32,
        GROUP_FINISH_INSERT_VA
    );
    assert_eq!(
        GROUP_FINISH_INSERT_VA + GROUP_FINISH_INSERT_BYTES as u32,
        0x0070_ea70
    );
    assert_eq!(COPY_ORDER_VA + COPY_ORDER_BYTES as u32, 0x0072_fd1c);
    assert_eq!(GROUP_FLAG_TEST_VA, 0x0070_e5a3);
    assert_eq!(COPY_ORDER_CALL_VA, 0x0070_e5a9);
    assert_eq!(SAVED_LIST_ADD_CALL_VA, 0x0070_e5b2);
    assert_eq!(FINISH_REMOVE_BEFORE_DISPATCH_VA, 0x0070_e657);
    assert_eq!(
        COPY_ORDER_CASE_VA[OrderKind::Gather.index()],
        COPY_GATHER_ARM_VA
    );
    assert_eq!(
        COPY_ORDER_CASE_VA[OrderKind::CastSpell.index()],
        COPY_CAST_ARM_VA
    );
    assert_eq!(
        COPY_ORDER_CASE_VA[OrderKind::TradeRoute.index()],
        COPY_NULL_ARM_VA
    );
    assert_eq!(
        FINISH_INSERT_CASE_VA[OrderKind::Gather.index()],
        FINISH_GATHER_ARM_VA
    );
    assert_eq!(
        FINISH_INSERT_CASE_VA[OrderKind::CastSpell.index()],
        FINISH_CAST_ARM_VA
    );
    assert_eq!(
        FINISH_INSERT_CASE_VA[OrderKind::TradeRoute.index()],
        FINISH_TRADE_ARM_VA
    );
    assert!(!copy_order_returns_clone(OrderKind::TradeRoute));
    assert!(finish_insert_has_action(OrderKind::TradeRoute));
}

#[test]
fn reverse_physical_reissue_keeps_old_group_orders_ahead_of_the_new_order() {
    let plan = plan_queue_first(&request(vec![
        gather(1, true),
        gather(2, true),
        gather(3, true),
    ]))
    .unwrap();

    assert_eq!(plan.set_up_visit_tokens, [3, 2, 1]);
    assert_eq!(plan.finish_slots, [Some(3), Some(2), Some(1)]);
    assert_eq!(
        plan.reissued_actions
            .iter()
            .map(|action| action.source_token())
            .collect::<Vec<_>>(),
        [3, 2, 1]
    );
    assert_eq!(plan.final_execution_tokens, [1, 2, 3, 99]);
}

#[test]
fn leader_only_group_filter_trade_null_clone_and_history_reset_are_explicit() {
    let trade = LeaderOrderSnapshot {
        token: 30,
        kind: OrderKind::TradeRoute,
        flags: ORDER_GROUP,
        // Native copy_order returns null before any concrete assignment is required.
        clone_image_complete: false,
        finish_payload: None,
    };
    let plan = plan_queue_first(&request(vec![
        gather(10, true),
        gather(20, false),
        trade,
        cast(40, 1),
    ]))
    .unwrap();

    assert_eq!(plan.set_up_visit_tokens, [40, 30, 20, 10]);
    assert_eq!(plan.finish_slots, [Some(40), None, Some(10)]);
    assert_eq!(plan.dropped_non_group_tokens, [20]);
    assert_eq!(plan.dropped_noncopyable_tokens, [30]);
    assert_eq!(plan.final_execution_tokens, [10, 40, 99]);

    let ReissuedAction::Cast {
        discarded_paid,
        spell,
        ..
    } = plan.reissued_actions[0]
    else {
        panic!("tail Cast must reissue first")
    };
    assert_eq!((spell, discarded_paid), (0x293, 1));

    let ReissuedAction::Gather {
        queue_raw,
        discarded_target_who,
        discarded_history,
        ..
    } = plan.reissued_actions[1]
    else {
        panic!("front Gather must reissue last")
    };
    assert_eq!(queue_raw, QUEUE_LAST);
    assert_eq!(discarded_target_who, 8);
    assert_eq!(discarded_history.wait, -7);
    assert_eq!(discarded_history.been_there, 1);
}

#[test]
fn incomplete_leader_links_clone_or_typed_history_refuses_before_a_plan_exists() {
    let mut r = request(vec![gather(1, true)]);
    r.leader_found = false;
    assert_eq!(
        plan_queue_first(&r),
        Err(QueueFirstContractError::MissingLeader)
    );

    r.leader_found = true;
    r.leader_queue_complete = false;
    assert_eq!(
        plan_queue_first(&r),
        Err(QueueFirstContractError::IncompleteLeaderQueue)
    );

    r.leader_queue_complete = true;
    r.physical_links_complete = false;
    assert_eq!(
        plan_queue_first(&r),
        Err(QueueFirstContractError::IncompletePhysicalLinks)
    );

    let mut missing_clone = gather(7, true);
    missing_clone.clone_image_complete = false;
    assert_eq!(
        plan_queue_first(&request(vec![missing_clone])),
        Err(QueueFirstContractError::IncompleteCloneImage {
            token: 7,
            kind: OrderKind::Gather,
        })
    );

    let mut missing_history = cast(8, 1);
    missing_history.finish_payload = None;
    assert_eq!(
        plan_queue_first(&request(vec![missing_history])),
        Err(QueueFirstContractError::MissingFinishPayload {
            token: 8,
            kind: OrderKind::CastSpell,
        })
    );
}

#[test]
fn unrelated_finish_actions_stay_at_a_source_owned_typed_boundary() {
    let move_order = LeaderOrderSnapshot {
        token: 5,
        kind: OrderKind::MoveTo,
        flags: ORDER_GROUP,
        clone_image_complete: true,
        finish_payload: Some(FinishPayload::OtherSourceOwned),
    };
    assert_eq!(
        plan_queue_first(&request(vec![move_order])),
        Err(QueueFirstContractError::SourceOwnedFinishPayloadRequired {
            token: 5,
            kind: OrderKind::MoveTo,
        })
    );
}
