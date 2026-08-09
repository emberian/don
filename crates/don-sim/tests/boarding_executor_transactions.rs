// SPDX-License-Identifier: GPL-3.0-or-later

//! Mutation-order gates for `Unit::do_board` and `Unit::do_await_board`.
//!
//! These are finite instruction-transcription checks against the recovered callbacks, not
//! retail differential evidence. The event trace intentionally includes queue lengths so a
//! host cannot move capacity or containment ahead of the measured order retirement.

use std::collections::VecDeque;

use don_sim::order::OrderIndex;
use don_sim::systems::movement::{PathFinder, UnitWorld};
use don_sim::systems::naval::TargetRef;
use don_sim::systems::order_dispatch::{
    do_job, AirPatrolSearch, ArmResult, AttackOutcome, BoardingAction, DispatchCoverage,
    GatherOutcome, KillReason, OrderRec, TargetState, UnitWork, WorkWorld,
};
use don_sim::systems::patrol::{AirPatrolOrder, AirPatrolTarget, GroupMoveRequest};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Anim {
        orders: usize,
        args: (i32, i32, i32),
    },
    Meet {
        orders: usize,
        target: TargetRef,
    },
    Carry {
        orders: usize,
        carrier: TargetRef,
        passenger: TargetRef,
    },
    GoInside {
        orders: usize,
        carrier: TargetRef,
        mode: i32,
    },
    LiveUnit(TargetRef),
    Action(TargetRef),
    AbortPassenger {
        ship_orders: usize,
        passenger: TargetRef,
    },
}

struct BoardingWorld {
    events: Vec<Event>,
    meet_pending: bool,
    carry_answers: VecDeque<bool>,
    live_answers: VecDeque<bool>,
    action_answers: VecDeque<Option<BoardingAction>>,
}

impl BoardingWorld {
    fn new(meet_pending: bool) -> Self {
        Self {
            events: Vec::new(),
            meet_pending,
            carry_answers: VecDeque::new(),
            live_answers: VecDeque::new(),
            action_answers: VecDeque::new(),
        }
    }
}

impl UnitWorld for BoardingWorld {
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

impl WorkWorld for BoardingWorld {
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
        unreachable!("boarding tests do not dispatch patrol")
    }

    fn air_patrol_physics(
        &mut self,
        _: &mut UnitWork,
        _: &mut AirPatrolOrder,
        _: i32,
        _: i32,
    ) -> bool {
        unreachable!("boarding tests do not dispatch patrol")
    }

    fn air_patrol_unit_target(
        &mut self,
        _: &UnitWork,
        _: &AirPatrolOrder,
        _: i32,
        _: i32,
        _: AirPatrolSearch,
    ) -> Option<AirPatrolTarget> {
        unreachable!("boarding tests do not dispatch patrol")
    }

    fn air_patrol_building_target(
        &mut self,
        _: &UnitWork,
        _: &AirPatrolOrder,
        _: i32,
        _: i32,
    ) -> Option<AirPatrolTarget> {
        unreachable!("boarding tests do not dispatch patrol")
    }

    fn group_patrol_move(&mut self, _: &mut UnitWork, _: GroupMoveRequest) {
        unreachable!("boarding tests do not dispatch patrol")
    }

    fn patrol_actor_is_type(&self, _: &UnitWork, _: i32, _: bool) -> bool {
        unreachable!("boarding tests do not dispatch patrol")
    }

    fn patrol_inside_is_scramblable(&self, _: u8, _: i16) -> bool {
        unreachable!("boarding tests do not dispatch patrol")
    }

    fn patrol_scramble_inside(&mut self, _: &mut UnitWork, _: i16) {
        unreachable!("boarding tests do not dispatch patrol")
    }

    fn boarding_set_anim(&mut self, actor: &mut UnitWork, a: i32, b: i32, c: i32) {
        self.events.push(Event::Anim {
            orders: actor.orders.len(),
            args: (a, b, c),
        });
    }

    fn board_check_meet_ship(&mut self, actor: &mut UnitWork, target: TargetRef) -> bool {
        self.events.push(Event::Meet {
            orders: actor.orders.len(),
            target,
        });
        self.meet_pending
    }

    fn boarding_can_carry(
        &mut self,
        actor: &UnitWork,
        carrier: TargetRef,
        passenger: TargetRef,
    ) -> bool {
        self.events.push(Event::Carry {
            orders: actor.orders.len(),
            carrier,
            passenger,
        });
        self.carry_answers
            .pop_front()
            .expect("test must provide every can_carry result")
    }

    fn board_go_inside(&mut self, actor: &mut UnitWork, carrier: TargetRef, mode: i32) {
        self.events.push(Event::GoInside {
            orders: actor.orders.len(),
            carrier,
            mode,
        });
        actor.inside_up = carrier.ox as i16;
    }

    fn boarding_target_is_live_unit(&mut self, target: TargetRef) -> bool {
        self.events.push(Event::LiveUnit(target));
        self.live_answers
            .pop_front()
            .expect("test must provide every live-unit result")
    }

    fn boarding_target_action(&mut self, target: TargetRef) -> Option<BoardingAction> {
        self.events.push(Event::Action(target));
        self.action_answers
            .pop_front()
            .expect("test must provide every action result")
    }

    fn boarding_abort_passenger(&mut self, actor: &UnitWork, passenger: TargetRef) {
        self.events.push(Event::AbortPassenger {
            ship_orders: actor.orders.len(),
            passenger,
        });
    }
}

fn actor_with_target(kind: OrderIndex, me: TargetRef, target: TargetRef) -> UnitWork {
    let mut actor = UnitWork::at(me.whom as u8, me.ox as i16, 0, 0);
    actor.uid = me.uid;
    actor.orders.push_back(OrderRec {
        kind,
        target_o: target.ox,
        target_who: target.whom,
        target_uid: target.uid,
        ..OrderRec::default()
    });
    // A surviving tail makes the exact point of retirement visible to callbacks.
    actor.orders.push_back(OrderRec::of_kind(OrderIndex::Think));
    actor
}

fn dispatch(
    actor: &mut UnitWork,
    world: &mut BoardingWorld,
    kind: OrderIndex,
) -> (ArmResult, DispatchCoverage) {
    let mut pathfinder = PathFinder::new();
    let mut coverage = DispatchCoverage::default();
    let result = do_job(actor, world, &mut pathfinder, &mut coverage, kind);
    (result, coverage)
}

#[test]
fn board_ship_holds_during_rendezvous_then_retires_before_capacity_and_containment() {
    let passenger = TargetRef {
        ox: 3,
        whom: 1,
        uid: 11,
    };
    let carrier = TargetRef {
        ox: 8,
        whom: 1,
        uid: 22,
    };

    let mut waiting_actor = actor_with_target(OrderIndex::BoardShip, passenger, carrier);
    let mut waiting_world = BoardingWorld::new(true);
    let (result, coverage) = dispatch(
        &mut waiting_actor,
        &mut waiting_world,
        OrderIndex::BoardShip,
    );
    assert_eq!(result, ArmResult::Working);
    assert_eq!(coverage.dispatches[OrderIndex::BoardShip.index()], 1);
    assert_eq!(waiting_actor.orders.len(), 2);
    assert_eq!(
        waiting_world.events,
        vec![
            Event::Anim {
                orders: 2,
                args: (0, 0, 1),
            },
            Event::Meet {
                orders: 2,
                target: carrier,
            },
        ]
    );

    let mut loading_actor = actor_with_target(OrderIndex::BoardShip, passenger, carrier);
    let mut loading_world = BoardingWorld::new(false);
    loading_world.carry_answers.push_back(true);
    let (result, coverage) = dispatch(
        &mut loading_actor,
        &mut loading_world,
        OrderIndex::BoardShip,
    );
    assert_eq!(result, ArmResult::Retired(KillReason::Completed));
    assert_eq!(coverage.completed, 1);
    assert_eq!(loading_actor.orders.len(), 1);
    assert_eq!(
        loading_actor.orders.front().unwrap().kind,
        OrderIndex::Think
    );
    assert_eq!(loading_actor.inside_up, carrier.ox as i16);
    assert_eq!(
        loading_world.events,
        vec![
            Event::Anim {
                orders: 2,
                args: (0, 0, 1),
            },
            Event::Meet {
                orders: 2,
                target: carrier,
            },
            Event::Carry {
                orders: 1,
                carrier,
                passenger,
            },
            Event::GoInside {
                orders: 1,
                carrier,
                mode: 0,
            },
        ]
    );
}

#[test]
fn await_board_holds_only_the_same_owner_carriable_reverse_link() {
    let ship = TargetRef {
        ox: 8,
        whom: 1,
        uid: 22,
    };
    let passenger = TargetRef {
        ox: 3,
        whom: 1,
        uid: 11,
    };
    let mut actor = actor_with_target(OrderIndex::AwaitBoard, ship, passenger);
    let mut world = BoardingWorld::new(false);
    world.live_answers.push_back(true);
    world.carry_answers.push_back(true);
    world.action_answers.push_back(Some(BoardingAction {
        kind: OrderIndex::BoardShip,
        // Uid is deliberately different: the executor compares only o/who here.
        target: TargetRef { uid: 99, ..ship },
    }));

    let (result, coverage) = dispatch(&mut actor, &mut world, OrderIndex::AwaitBoard);
    assert_eq!(result, ArmResult::Working);
    assert_eq!(coverage.completed, 0);
    assert_eq!(actor.orders.len(), 2);
    assert_eq!(
        world.events,
        vec![
            Event::Anim {
                orders: 2,
                args: (0, 0, 1),
            },
            Event::LiveUnit(passenger),
            Event::Carry {
                orders: 2,
                carrier: ship,
                passenger,
            },
            Event::Action(passenger),
        ]
    );
}

#[test]
fn await_board_fallback_cancels_passenger_before_ship_but_fast_path_mismatch_does_not() {
    let ship = TargetRef {
        ox: 8,
        whom: 1,
        uid: 22,
    };
    let passenger = TargetRef {
        ox: 3,
        whom: 1,
        uid: 11,
    };

    // Same owner but no room: retail repeats the live-unit probe, then cancels the matching
    // passenger order before killing the ship's await order.
    let mut full_ship = actor_with_target(OrderIndex::AwaitBoard, ship, passenger);
    let mut fallback = BoardingWorld::new(false);
    fallback.live_answers.extend([true, true]);
    fallback.carry_answers.push_back(false);
    fallback.action_answers.push_back(Some(BoardingAction {
        kind: OrderIndex::BoardShip,
        target: ship,
    }));
    let (result, coverage) = dispatch(&mut full_ship, &mut fallback, OrderIndex::AwaitBoard);
    assert_eq!(result, ArmResult::Retired(KillReason::Completed));
    assert_eq!(coverage.completed, 1);
    assert_eq!(full_ship.orders.front().unwrap().kind, OrderIndex::Think);
    assert_eq!(
        fallback.events,
        vec![
            Event::Anim {
                orders: 2,
                args: (0, 0, 1),
            },
            Event::LiveUnit(passenger),
            Event::Carry {
                orders: 2,
                carrier: ship,
                passenger,
            },
            Event::LiveUnit(passenger),
            Event::Action(passenger),
            Event::AbortPassenger {
                // The ship order is still current while the passenger is cancelled.
                ship_orders: 2,
                passenger,
            },
        ]
    );

    // A fully probed same-owner passenger that is boarding another ship takes the direct
    // local kill at 0x005ED116. It must not enter the fallback cancellation pass.
    let mut wrong_ship = actor_with_target(OrderIndex::AwaitBoard, ship, passenger);
    let mut direct = BoardingWorld::new(false);
    direct.live_answers.push_back(true);
    direct.carry_answers.push_back(true);
    direct.action_answers.push_back(Some(BoardingAction {
        kind: OrderIndex::BoardShip,
        target: TargetRef { ox: 9, ..ship },
    }));
    let (result, _) = dispatch(&mut wrong_ship, &mut direct, OrderIndex::AwaitBoard);
    assert_eq!(result, ArmResult::Retired(KillReason::Completed));
    assert_eq!(
        direct.events,
        vec![
            Event::Anim {
                orders: 2,
                args: (0, 0, 1),
            },
            Event::LiveUnit(passenger),
            Event::Carry {
                orders: 2,
                carrier: ship,
                passenger,
            },
            Event::Action(passenger),
        ]
    );
}
