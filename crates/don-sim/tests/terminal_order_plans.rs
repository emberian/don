// SPDX-License-Identifier: GPL-3.0-or-later

//! Deterministic planner pins for `CHANGE_FORM` and `THINK`.

use don_sim::order::OrderIndex;
use don_sim::systems::terminal_order_plans::{
    plan_terminal_order, think_peasant_type, ChangeFormOrderFacts, TerminalOrderActor,
    TerminalOrderEffect, TerminalOrderReceipt, TerminalOrderRequest, TerminalOrderStatus,
};

fn actor() -> TerminalOrderActor {
    TerminalOrderActor {
        who: 3,
        object: 27,
        uid: 0x8182,
    }
}

#[test]
fn change_form_single_order_sets_form_and_angle_before_retirement() {
    let request = TerminalOrderRequest::ChangeForm {
        actor: actor(),
        order: ChangeFormOrderFacts {
            angle: 0x2345_6789,
            new_form: 0x102,
        },
        queue_before: vec![OrderIndex::ChangeForm],
    };
    let plan = plan_terminal_order(&request).unwrap();
    assert_eq!(plan.arm, OrderIndex::ChangeForm);
    assert_eq!(
        plan.effects,
        vec![
            TerminalOrderEffect::StoreForm(2),
            TerminalOrderEffect::SetAngle {
                angle: 0x2345_6789,
                update_position: false,
            },
            TerminalOrderEffect::KillCurrentOrder {
                suppress_arrival: false,
            },
        ]
    );
}

#[test]
fn change_form_with_successor_retires_then_idles_without_setting_angle() {
    let request = TerminalOrderRequest::ChangeForm {
        actor: actor(),
        order: ChangeFormOrderFacts {
            angle: -17,
            new_form: -1,
        },
        queue_before: vec![OrderIndex::ChangeForm, OrderIndex::MoveTo],
    };
    assert_eq!(
        plan_terminal_order(&request).unwrap().effects,
        vec![
            TerminalOrderEffect::StoreForm(-1),
            TerminalOrderEffect::KillCurrentOrder {
                suppress_arrival: false,
            },
            TerminalOrderEffect::DoIdle,
        ]
    );
}

#[test]
fn think_with_real_successor_only_retires_itself() {
    let request = TerminalOrderRequest::Think {
        actor: actor(),
        unit_type: 50,
        queue_before: vec![OrderIndex::Think, OrderIndex::Gather],
    };
    assert_eq!(
        plan_terminal_order(&request).unwrap().effects,
        vec![TerminalOrderEffect::KillCurrentOrder {
            suppress_arrival: false,
        }]
    );
}

#[test]
fn think_peasant_runs_for_empty_or_none_successor() {
    for queue in [
        vec![OrderIndex::Think],
        vec![OrderIndex::Think, OrderIndex::None],
    ] {
        let request = TerminalOrderRequest::Think {
            actor: actor(),
            unit_type: 53,
            queue_before: queue,
        };
        assert_eq!(
            plan_terminal_order(&request).unwrap().effects,
            vec![
                TerminalOrderEffect::KillCurrentOrder {
                    suppress_arrival: false,
                },
                TerminalOrderEffect::ThinkPeasant { forced: true },
            ]
        );
    }
}

#[test]
fn think_type_gate_is_exactly_the_four_retail_ids() {
    for id in 48..=55 {
        assert_eq!(think_peasant_type(id), (50..=53).contains(&id));
    }
}

#[test]
fn stale_heads_fail_closed() {
    let change = TerminalOrderRequest::ChangeForm {
        actor: actor(),
        order: ChangeFormOrderFacts {
            angle: 0,
            new_form: 0,
        },
        queue_before: vec![OrderIndex::Think],
    };
    let think = TerminalOrderRequest::Think {
        actor: actor(),
        unit_type: 50,
        queue_before: vec![],
    };
    assert!(plan_terminal_order(&change).is_none());
    assert!(plan_terminal_order(&think).is_none());
    assert!(TerminalOrderReceipt::applied(change).is_none());
    assert!(TerminalOrderReceipt::applied(think).is_none());
}

#[test]
fn receipt_binds_actor_queue_and_exact_effect_order() {
    let request = TerminalOrderRequest::Think {
        actor: actor(),
        unit_type: 50,
        queue_before: vec![OrderIndex::Think],
    };
    let receipt = TerminalOrderReceipt::applied(request.clone()).unwrap();
    assert!(receipt.validates(&request));

    let mut wrong_actor = request.clone();
    if let TerminalOrderRequest::Think { actor, .. } = &mut wrong_actor {
        actor.uid ^= 1;
    }
    assert!(!receipt.validates(&wrong_actor));

    let mut reordered = receipt.clone();
    reordered.plan.as_mut().unwrap().effects.reverse();
    assert!(!reordered.validates(&request));

    let unavailable = TerminalOrderReceipt::unavailable(request.clone());
    assert_eq!(unavailable.status, TerminalOrderStatus::Unavailable);
    assert!(unavailable.validates(&request));
}
