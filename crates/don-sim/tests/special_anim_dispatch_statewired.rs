// SPDX-License-Identifier: GPL-3.0-or-later

//! StateWired integration pins for SPECIAL_ANIM.
//!
//! The typed dispatcher adapter is executable behind one atomic host transaction. The real
//! `Sim::do_frame` path reaches it for the host-free UNIT arm and the object-free EXIT arm;
//! external ENTER/Airbase-EXIT tails remain typed-unavailable, so the strict row stays red.

use don_sim::order::{
    ArmStatus, Order, OrderIndex, SpecialAnimOrderState, SpecialAnimType, EXECUTORS,
};
use don_sim::systems::movement::{PathFinder, UnitWorld};
use don_sim::systems::order_dispatch::{
    do_job, kill_current_order, AirPatrolSearch, ArmResult, AttackOutcome, DispatchCoverage,
    GatherOutcome, KillReason, OrderRec, SpecialAnimCommitReceipt, SpecialAnimHostError,
    TargetState, UnitWork, WorkWorld, ARMS,
};
use don_sim::systems::patrol::{AirPatrolOrder, AirPatrolTarget, GroupMoveRequest};
use don_sim::systems::special_anim_executor::{
    preflight_special_anim_executor, ActorFacts, EnterTargetFacts, ObjectIdentity, ObjectSnapshot,
    SpecialAnimExecutorReceipt, SpecialAnimExecutorRequest, SpecialAnimHostSnapshot,
    SpecialAnimHostStep, SpecialAnimKind, SpecialAnimState,
};
use don_sim::tick::{Sim, StepRun};

#[derive(Default)]
struct AtomicSpecialWorld {
    preflight: Option<SpecialAnimExecutorReceipt>,
    committed: Vec<SpecialAnimHostStep>,
    corrupt_commit_receipt: bool,
}

impl UnitWorld for AtomicSpecialWorld {
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

impl WorkWorld for AtomicSpecialWorld {
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
        unreachable!("SPECIAL_ANIM tests do not dispatch patrol")
    }

    fn air_patrol_physics(
        &mut self,
        _: &mut UnitWork,
        _: &mut AirPatrolOrder,
        _: i32,
        _: i32,
    ) -> bool {
        unreachable!("SPECIAL_ANIM tests do not dispatch patrol")
    }

    fn air_patrol_unit_target(
        &mut self,
        _: &UnitWork,
        _: &AirPatrolOrder,
        _: i32,
        _: i32,
        _: AirPatrolSearch,
    ) -> Option<AirPatrolTarget> {
        unreachable!("SPECIAL_ANIM tests do not dispatch patrol")
    }

    fn air_patrol_building_target(
        &mut self,
        _: &UnitWork,
        _: &AirPatrolOrder,
        _: i32,
        _: i32,
    ) -> Option<AirPatrolTarget> {
        unreachable!("SPECIAL_ANIM tests do not dispatch patrol")
    }

    fn group_patrol_move(&mut self, _: &mut UnitWork, _: GroupMoveRequest) {
        unreachable!("SPECIAL_ANIM tests do not dispatch patrol")
    }

    fn patrol_actor_is_type(&self, _: &UnitWork, _: i32, _: bool) -> bool {
        unreachable!("SPECIAL_ANIM tests do not dispatch patrol")
    }

    fn patrol_inside_is_scramblable(&self, _: u8, _: i16) -> bool {
        unreachable!("SPECIAL_ANIM tests do not dispatch patrol")
    }

    fn patrol_scramble_inside(&mut self, _: &mut UnitWork, _: i16) {
        unreachable!("SPECIAL_ANIM tests do not dispatch patrol")
    }

    fn special_anim_preflight(
        &mut self,
        _: &UnitWork,
        _: &OrderRec,
    ) -> Result<SpecialAnimExecutorReceipt, SpecialAnimHostError> {
        self.preflight
            .clone()
            .ok_or(SpecialAnimHostError::Unavailable)
    }

    fn special_anim_commit(
        &mut self,
        actor: &mut UnitWork,
        _: &OrderRec,
        preflight: &SpecialAnimExecutorReceipt,
    ) -> Result<SpecialAnimCommitReceipt, SpecialAnimHostError> {
        if self.corrupt_commit_receipt {
            let mut receipt = SpecialAnimCommitReceipt::applied(preflight);
            receipt.committed_steps = receipt.committed_steps.saturating_sub(1);
            return Ok(receipt);
        }
        for step in &preflight.plan.steps {
            self.committed.push(*step);
            match *step {
                SpecialAnimHostStep::StoreFrames(value) => {
                    actor
                        .orders
                        .front_mut()
                        .unwrap()
                        .special_anim
                        .as_mut()
                        .unwrap()
                        .frames = value;
                }
                SpecialAnimHostStep::StoreStarted(value) => {
                    actor
                        .orders
                        .front_mut()
                        .unwrap()
                        .special_anim
                        .as_mut()
                        .unwrap()
                        .started = value;
                }
                SpecialAnimHostStep::KillCurrentOrder(reason) => {
                    assert_eq!(reason, 0);
                    kill_current_order(actor, KillReason::Completed);
                }
                _ => {}
            }
        }
        Ok(SpecialAnimCommitReceipt::applied(preflight))
    }
}

fn enter_fixture() -> (UnitWork, SpecialAnimExecutorReceipt) {
    let mut actor = UnitWork::at(2, 7, 100, 200);
    actor.uid = 0x7172;
    let walked = SpecialAnimOrderState {
        special_type: SpecialAnimType::Enter,
        data1: 81,
        data2: 1,
        data3: 41,
        data4: 3,
        ..SpecialAnimOrderState::default()
    };
    actor.orders.push_back(OrderRec::special_anim(walked));
    actor.orders.push_back(OrderRec::of_kind(OrderIndex::Guard));

    let order = SpecialAnimState {
        special_type: SpecialAnimKind::Enter,
        data1: 81,
        data2: 1,
        data3: 41,
        data4: 3,
        ..SpecialAnimState::default()
    };
    let actor_id = ObjectIdentity {
        o: 7,
        who: 2,
        uid: 0x7172,
    };
    let target_id = ObjectIdentity {
        o: 41,
        who: 3,
        uid: 0x4142,
    };
    let request = SpecialAnimExecutorRequest {
        order,
        actor: ActorFacts { identity: actor_id },
        enter_target: Some(EnterTargetFacts {
            identity: target_id,
            is_valid_build: true,
            is_aircraft_carrier: None,
        }),
        exit_target: None,
        random_draws: None,
        helicopter_samples: None,
        terrain_z: None,
    };
    let snapshot = SpecialAnimHostSnapshot {
        actor: ObjectSnapshot {
            identity: actor_id,
            version: 11,
        },
        target: Some(ObjectSnapshot {
            identity: target_id,
            version: 19,
        }),
        current_order: order,
        current_order_digest: 0x101,
        queue_digest: 0x202,
        path_digest: 0x303,
        primary_guy_digest: 0x404,
        object_epoch: 5,
        terrain_epoch: 6,
        external_epoch: 7,
        rng_epoch: 8,
    };
    let receipt = preflight_special_anim_executor(snapshot, request).unwrap();
    (actor, receipt)
}

fn dispatch(actor: &mut UnitWork, world: &mut AtomicSpecialWorld) -> (ArmResult, DispatchCoverage) {
    let mut pathfinder = PathFinder::new();
    let mut coverage = DispatchCoverage::default();
    let result = do_job(
        actor,
        world,
        &mut pathfinder,
        &mut coverage,
        OrderIndex::SpecialAnim,
    );
    (result, coverage)
}

#[test]
fn dispatcher_publishes_the_whole_preflight_plan_in_one_commit() {
    let (mut actor, receipt) = enter_fixture();
    let expected = receipt.plan.steps.clone();
    let mut world = AtomicSpecialWorld {
        preflight: Some(receipt),
        ..AtomicSpecialWorld::default()
    };

    let (result, coverage) = dispatch(&mut actor, &mut world);

    assert_eq!(result, ArmResult::Retired(KillReason::Completed));
    assert_eq!(actor.order_type(), OrderIndex::Guard);
    assert_eq!(world.committed, expected);
    assert_eq!(coverage.completed, 1);
    assert_eq!(
        coverage.unimplemented, 1,
        "StateWired is not closure-complete"
    );
}

#[test]
fn special_unit_is_the_exact_host_free_no_op() {
    let mut actor = UnitWork::at(2, 7, 100, 200);
    actor
        .orders
        .push_back(OrderRec::special_anim(SpecialAnimOrderState {
            special_type: SpecialAnimType::Unit,
            data1: 19,
            data2: 23,
            ..SpecialAnimOrderState::default()
        }));
    let before = actor.orders.clone();

    let (result, coverage) = dispatch(&mut actor, &mut AtomicSpecialWorld::default());

    assert_eq!(result, ArmResult::Working);
    assert_eq!(actor.orders, before);
    assert_eq!(
        coverage.unimplemented, 1,
        "StateWired is not closure-complete"
    );
}

#[test]
fn unavailable_or_malformed_publication_restores_the_local_before_image() {
    let (mut unavailable_actor, _) = enter_fixture();
    let unavailable_before = unavailable_actor.clone();
    let (result, _) = dispatch(&mut unavailable_actor, &mut AtomicSpecialWorld::default());
    assert_eq!(result, ArmResult::HostUnavailable);
    assert_eq!(unavailable_actor.orders, unavailable_before.orders);

    let (mut malformed_actor, receipt) = enter_fixture();
    let malformed_before = malformed_actor.clone();
    let mut malformed_world = AtomicSpecialWorld {
        preflight: Some(receipt),
        corrupt_commit_receipt: true,
        ..AtomicSpecialWorld::default()
    };
    let (result, _) = dispatch(&mut malformed_actor, &mut malformed_world);
    assert_eq!(result, ArmResult::MalformedOrder);
    assert_eq!(malformed_actor.orders, malformed_before.orders);
    assert!(malformed_world.committed.is_empty());
}

#[test]
fn real_sim_frame_reaches_the_host_free_special_unit_adapter() {
    let mut sim = Sim::new(0x25_5880, 16);
    sim.activate(0);
    let actor = sim.spawn_unit(0, 1, 192, 192, 1).unwrap();
    assert!(sim.issue(actor, Order::special_anim(SpecialAnimType::Unit, 9, 10),));
    let row = sim.world.row_of(actor).unwrap();
    let before = *sim.world.orders(row).current().unwrap();

    let trace = sim.do_frame();

    assert_eq!(trace.steps[14], StepRun::Executed);
    assert_eq!(
        sim.cover.unit_process, 1,
        "the real object pass visited the actor"
    );
    assert_eq!(sim.cover.special_anim_working, 1);
    assert_eq!(sim.world.orders(row).current(), Some(&before));
    assert_eq!(
        ARMS[OrderIndex::SpecialAnim.index()],
        ArmStatus::Unimplemented
    );
    assert_eq!(
        EXECUTORS[OrderIndex::SpecialAnim.index()].status,
        ArmStatus::Unimplemented
    );
}
