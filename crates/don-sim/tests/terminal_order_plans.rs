// SPDX-License-Identifier: GPL-3.0-or-later

//! Deterministic planner pins for `CHANGE_FORM` and `THINK`.

use don_sim::order::OrderIndex;
use don_sim::systems::movement::{PathFinder, UnitWorld};
use don_sim::systems::order_dispatch::{
    do_job, kill_current_order, AirPatrolSearch, ArmResult, AttackOutcome, DispatchCoverage,
    GatherOutcome, KillReason, OrderRec, TargetState, UnitWork, WorkWorld,
};
use don_sim::systems::patrol::{AirPatrolOrder, AirPatrolTarget, GroupMoveRequest};
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

#[derive(Default)]
struct AtomicTerminalWorld {
    available: bool,
    effects: Vec<TerminalOrderEffect>,
    idle_calls: u32,
    think_calls: u32,
}

impl UnitWorld for AtomicTerminalWorld {
    fn tiles_w(&self) -> i32 {
        16
    }

    fn tiles_h(&self) -> i32 {
        16
    }

    fn wcells_w(&self) -> i32 {
        4
    }

    fn invalid_loc(&self, _: i32, _: i32) -> bool {
        false
    }

    fn unit_collides(&self, _: i32, _: i32) -> bool {
        false
    }

    fn needs_transport(&self, _: i32, _: i32, _: i32, _: i32) -> i32 {
        0
    }

    fn tregion(&self, _: i32, _: i32) -> i32 {
        0
    }
}

impl WorkWorld for AtomicTerminalWorld {
    fn frame(&self) -> i32 {
        0
    }

    fn target(&self, _: i32, _: i32) -> Option<TargetState> {
        None
    }

    fn attack(&mut self, _: &UnitWork, _: &OrderRec) -> AttackOutcome {
        AttackOutcome::Impossible
    }

    fn gather(&mut self, _: &UnitWork, _: &OrderRec) -> GatherOutcome {
        GatherOutcome::Exhausted
    }

    fn draw_path_retry_delay(&mut self) -> i32 {
        6
    }

    fn patrol_think_bird(&mut self, _: &mut UnitWork, _: &mut AirPatrolOrder) {
        unreachable!("terminal-order tests do not dispatch patrol")
    }

    fn air_patrol_physics(
        &mut self,
        _: &mut UnitWork,
        _: &mut AirPatrolOrder,
        _: i32,
        _: i32,
    ) -> bool {
        unreachable!("terminal-order tests do not dispatch patrol")
    }

    fn air_patrol_unit_target(
        &mut self,
        _: &UnitWork,
        _: &AirPatrolOrder,
        _: i32,
        _: i32,
        _: AirPatrolSearch,
    ) -> Option<AirPatrolTarget> {
        unreachable!("terminal-order tests do not dispatch patrol")
    }

    fn air_patrol_building_target(
        &mut self,
        _: &UnitWork,
        _: &AirPatrolOrder,
        _: i32,
        _: i32,
    ) -> Option<AirPatrolTarget> {
        unreachable!("terminal-order tests do not dispatch patrol")
    }

    fn group_patrol_move(&mut self, _: &mut UnitWork, _: GroupMoveRequest) {
        unreachable!("terminal-order tests do not dispatch patrol")
    }

    fn patrol_actor_is_type(&self, _: &UnitWork, _: i32, _: bool) -> bool {
        unreachable!("terminal-order tests do not dispatch patrol")
    }

    fn patrol_inside_is_scramblable(&self, _: u8, _: i16) -> bool {
        unreachable!("terminal-order tests do not dispatch patrol")
    }

    fn patrol_scramble_inside(&mut self, _: &mut UnitWork, _: i16) {
        unreachable!("terminal-order tests do not dispatch patrol")
    }

    fn apply_terminal_order_transaction(
        &mut self,
        actor: &mut UnitWork,
        request: TerminalOrderRequest,
    ) -> TerminalOrderReceipt {
        if !self.available {
            return TerminalOrderReceipt::unavailable(request);
        }
        let receipt = TerminalOrderReceipt::applied(request)
            .expect("dispatcher preflight supplies the expected terminal head");
        for effect in &receipt.plan.as_ref().unwrap().effects {
            self.effects.push(*effect);
            match *effect {
                TerminalOrderEffect::StoreForm(value) => actor.form = value,
                TerminalOrderEffect::SetAngle { angle, .. } => {
                    actor.body.angle = angle;
                    actor.lead_guy.angle = angle;
                }
                TerminalOrderEffect::KillCurrentOrder { .. } => {
                    kill_current_order(actor, KillReason::Completed);
                }
                TerminalOrderEffect::DoIdle => {
                    actor.idle = 1;
                    self.idle_calls += 1;
                }
                TerminalOrderEffect::ThinkPeasant { forced } => {
                    assert!(forced);
                    self.think_calls += 1;
                }
            }
        }
        receipt
    }
}

fn dispatch(
    actor: &mut UnitWork,
    world: &mut AtomicTerminalWorld,
    kind: OrderIndex,
) -> (ArmResult, DispatchCoverage) {
    let mut pathfinder = PathFinder::new();
    let mut coverage = DispatchCoverage::default();
    let result = do_job(actor, world, &mut pathfinder, &mut coverage, kind);
    (result, coverage)
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

#[test]
fn live_change_form_single_order_commits_form_angle_and_retirement_atomically() {
    let mut actor = UnitWork::at(3, 27, 100, 200);
    actor.uid = 0x8182;
    actor.form = 7;
    actor.body.angle = 99;
    actor
        .orders
        .push_back(OrderRec::change_form(0x2345_6789, 0x102, 0x5566_7788));
    let mut world = AtomicTerminalWorld {
        available: true,
        ..AtomicTerminalWorld::default()
    };

    let (result, coverage) = dispatch(&mut actor, &mut world, OrderIndex::ChangeForm);

    assert_eq!(result, ArmResult::Retired(KillReason::Completed));
    assert_eq!(actor.form, 2);
    assert_eq!(
        (actor.body.angle, actor.lead_guy.angle),
        (0x2345_6789, 0x2345_6789)
    );
    assert!(actor.orders.is_empty());
    assert_eq!(coverage.completed, 1);
    assert_eq!(coverage.unimplemented, 0);
    assert_eq!(
        world.effects,
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
fn live_change_form_with_successor_skips_angle_and_runs_idle_after_retirement() {
    let mut actor = UnitWork::at(3, 27, 100, 200);
    actor.uid = 0x8182;
    actor.form = 7;
    actor.body.angle = 99;
    actor.orders.push_back(OrderRec::change_form(-17, -1, 44));
    actor.orders.push_back(OrderRec::move_to(500, 600, 12));
    let mut world = AtomicTerminalWorld {
        available: true,
        ..AtomicTerminalWorld::default()
    };

    let (result, _) = dispatch(&mut actor, &mut world, OrderIndex::ChangeForm);

    assert_eq!(result, ArmResult::Retired(KillReason::Completed));
    assert_eq!(actor.form, -1);
    assert_eq!(actor.body.angle, 99);
    assert_eq!(actor.orders.front().unwrap().kind, OrderIndex::MoveTo);
    assert_eq!(world.idle_calls, 1);
    assert_eq!(
        world.effects,
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
fn live_think_uses_exact_type_and_successor_gates() {
    let mut peasant = UnitWork::at(3, 27, 100, 200);
    peasant.uid = 0x8182;
    peasant.ptype = 50;
    peasant
        .orders
        .push_back(OrderRec::of_kind(OrderIndex::Think));
    let mut peasant_world = AtomicTerminalWorld {
        available: true,
        ..AtomicTerminalWorld::default()
    };
    let (result, _) = dispatch(&mut peasant, &mut peasant_world, OrderIndex::Think);
    assert_eq!(result, ArmResult::Retired(KillReason::Completed));
    assert_eq!(peasant_world.think_calls, 1);

    let mut queued = UnitWork::at(3, 27, 100, 200);
    queued.uid = 0x8182;
    queued.ptype = 53;
    queued
        .orders
        .push_back(OrderRec::of_kind(OrderIndex::Think));
    queued
        .orders
        .push_back(OrderRec::of_kind(OrderIndex::Gather));
    let mut queued_world = AtomicTerminalWorld {
        available: true,
        ..AtomicTerminalWorld::default()
    };
    let (result, _) = dispatch(&mut queued, &mut queued_world, OrderIndex::Think);
    assert_eq!(result, ArmResult::Retired(KillReason::Completed));
    assert_eq!(queued.orders.front().unwrap().kind, OrderIndex::Gather);
    assert_eq!(queued_world.think_calls, 0);
}

#[test]
fn live_terminal_dispatch_fails_closed_for_missing_host_or_payload() {
    let mut unavailable = UnitWork::at(3, 27, 100, 200);
    unavailable.form = 7;
    unavailable
        .orders
        .push_back(OrderRec::change_form(88, 3, 4));
    let mut world = AtomicTerminalWorld::default();
    let before_queue = unavailable.orders.debug_image();
    let before_angle = unavailable.body.angle;
    let (result, coverage) = dispatch(&mut unavailable, &mut world, OrderIndex::ChangeForm);
    assert_eq!(result, ArmResult::HostUnavailable);
    assert_eq!(unavailable.form, 7);
    assert_eq!(unavailable.body.angle, before_angle);
    assert_eq!(unavailable.orders.debug_image(), before_queue);
    assert_eq!(coverage.completed, 0);

    let mut malformed = UnitWork::at(3, 27, 100, 200);
    malformed
        .orders
        .push_back(OrderRec::of_kind(OrderIndex::ChangeForm));
    let mut world = AtomicTerminalWorld {
        available: true,
        ..AtomicTerminalWorld::default()
    };
    let (result, _) = dispatch(&mut malformed, &mut world, OrderIndex::ChangeForm);
    assert_eq!(result, ArmResult::MalformedOrder);
    assert!(world.effects.is_empty());
}
