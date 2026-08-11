// SPDX-License-Identifier: GPL-3.0-or-later

//! GARRISON (arm 26) integration pins.
//!
//! Like the GUARD file, every test drives the real `Unit::do_job` `0x00617A10` table so a
//! green result is evidence about the *dispatcher*, not about a planner called directly.
//!
//! GARRISON's split is the inverse of GUARD's: the dispatcher owns exactly one local effect,
//! the bare `Unit::kill_current_order(0)` `0x005E2CB0`, and the retirement performed by
//! `Unit::kill_garrison_order` `0x005E2BD0` walking the containment chain is **observed**
//! after the host steps rather than predicted.

use don_sim::order::{ArmStatus, OrderIndex, EXECUTORS, ORDER_GROUP};
use don_sim::systems::garrison_dispatch::{
    do_garrison_observed, garrison_order_rec, host_snapshot, GarrisonHostError,
    GARRISON_DISPATCH_OPEN_TAILS,
};
use don_sim::systems::garrison_order::{
    preflight_garrison_executor, validate_garrison_receipt, GarrisonActorFacts,
    GarrisonExecutorBranch, GarrisonExecutorReceipt, GarrisonExecutorRequest, GarrisonFeedback,
    GarrisonHostStep, GarrisonIdentity, GarrisonOrderState, GarrisonPlanError, GarrisonTargetFacts,
};
use don_sim::systems::guard_dispatch::path_stack_digest;
use don_sim::systems::movement::{PathFinder, UnitWorld};
use don_sim::systems::order_dispatch::{
    do_job, kill_current_order, AirPatrolSearch, ArmResult, AttackOutcome, DispatchCoverage,
    GatherOutcome, KillReason, OrderRec, TargetState, UnitWork, WorkWorld, ARMS,
};
use don_sim::systems::patrol::{AirPatrolOrder, AirPatrolTarget, GroupMoveRequest};

const ACTOR: GarrisonIdentity = GarrisonIdentity {
    o: 5,
    who: 1,
    uid: 0x1234,
};
const TARGET: GarrisonIdentity = GarrisonIdentity {
    o: 40,
    who: 1,
    uid: 0x9911,
};

// ---------------------------------------------------------------------------
// A GARRISON host that serves one prepared receipt
// ---------------------------------------------------------------------------

#[derive(Default)]
struct GarrisonWorld {
    receipt: Option<GarrisonExecutorReceipt>,
    effects: Vec<GarrisonHostStep>,
    target_uid: Option<u16>,
    /// When set, `KillCaptainGarrisonOrder` retires the actor's own head order, which is what
    /// the containment-chain walk does when the actor is its own captain.
    chain_retires_actor: bool,
}

impl UnitWorld for GarrisonWorld {
    fn tiles_w(&self) -> i32 {
        64
    }
    fn tiles_h(&self) -> i32 {
        64
    }
    fn wcells_w(&self) -> i32 {
        16
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

impl WorkWorld for GarrisonWorld {
    fn frame(&self) -> i32 {
        0
    }
    fn target(&self, who: i32, o: i32) -> Option<TargetState> {
        let uid = self.target_uid?;
        if (who, o) != (TARGET.who, TARGET.o) {
            return None;
        }
        Some(TargetState {
            x: 0x2000,
            y: 0x2000,
            uid,
            active: true,
            seen: true,
        })
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
        unreachable!("GARRISON tests do not dispatch patrol")
    }
    fn air_patrol_physics(
        &mut self,
        _: &mut UnitWork,
        _: &mut AirPatrolOrder,
        _: i32,
        _: i32,
    ) -> bool {
        unreachable!("GARRISON tests do not dispatch patrol")
    }
    fn air_patrol_unit_target(
        &mut self,
        _: &UnitWork,
        _: &AirPatrolOrder,
        _: i32,
        _: i32,
        _: AirPatrolSearch,
    ) -> Option<AirPatrolTarget> {
        unreachable!("GARRISON tests do not dispatch patrol")
    }
    fn air_patrol_building_target(
        &mut self,
        _: &UnitWork,
        _: &AirPatrolOrder,
        _: i32,
        _: i32,
    ) -> Option<AirPatrolTarget> {
        unreachable!("GARRISON tests do not dispatch patrol")
    }
    fn group_patrol_move(&mut self, _: &mut UnitWork, _: GroupMoveRequest) {
        unreachable!("GARRISON tests do not dispatch patrol")
    }
    fn patrol_actor_is_type(&self, _: &UnitWork, _: i32, _: bool) -> bool {
        unreachable!("GARRISON tests do not dispatch patrol")
    }
    fn patrol_inside_is_scramblable(&self, _: u8, _: i16) -> bool {
        unreachable!("GARRISON tests do not dispatch patrol")
    }
    fn patrol_scramble_inside(&mut self, _: &mut UnitWork, _: i16) {
        unreachable!("GARRISON tests do not dispatch patrol")
    }

    fn garrison_preflight(
        &mut self,
        _: &UnitWork,
        _: &OrderRec,
    ) -> Result<GarrisonExecutorReceipt, GarrisonHostError> {
        self.receipt.clone().ok_or(GarrisonHostError::Unavailable)
    }

    fn garrison_effect(&mut self, actor: &mut UnitWork, _: &OrderRec, step: GarrisonHostStep) {
        self.effects.push(step);
        if matches!(step, GarrisonHostStep::KillCaptainGarrisonOrder { .. })
            && self.chain_retires_actor
        {
            // `Unit::kill_garrison_order` reaches this actor through `UnitData::get_action`
            // and issues the failure pair; the bare kill is the reachable half here.
            kill_current_order(actor, KillReason::Failed);
        }
    }
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn actor_with(order: GarrisonOrderState, flags: u8) -> UnitWork {
    let mut actor = UnitWork::at(ACTOR.who as u8, ACTOR.o as i16, 0x1000, 0x1000);
    actor.uid = ACTOR.uid;
    actor.orders.push_back(garrison_order_rec(order, flags));
    actor.orders.push_back(OrderRec::of_kind(OrderIndex::Think));
    actor
}

fn actor_facts(actor: &UnitWork) -> GarrisonActorFacts {
    GarrisonActorFacts {
        identity: ACTOR,
        x: actor.body.x,
        y: actor.body.y,
        unit_masks: actor.unit_masks,
        type_flags_2b4: 0,
        inside_cost: 1,
        local_player: true,
        reaches_target: Some(true),
        captain_o: Some(ACTOR.o),
    }
}

fn target_facts() -> GarrisonTargetFacts {
    GarrisonTargetFacts {
        identity: TARGET,
        raw_address: 0,
        x: 0x2000,
        y: 0x2000,
        owner: TARGET.who,
        is_valid_wall: Some(true),
        is_active: Some(true),
        diplomacy_allows: Some(true),
        capacity_limit: Some(4),
        actor_type_can_garrison: Some(true),
        is_airbase: None,
        can_carry_actor: None,
        footprint_x: 2,
        footprint_y: 2,
        is_dock: None,
        build_flags: 0,
        city_index: -1,
        city_race: None,
        hits: None,
        hits_left: None,
        occupied: Some(0),
        terrain_owner: Some(-1),
        target_owner_allied_with_terrain: None,
        is_build: Some(true),
    }
}

fn seeded(
    actor: &UnitWork,
    order: GarrisonOrderState,
    flags: u8,
    target: Option<GarrisonTargetFacts>,
) -> GarrisonExecutorReceipt {
    let request = GarrisonExecutorRequest {
        actor: actor_facts(actor),
        order,
        order_flags: flags,
        outer_uid_validated: true,
        target,
        approach: None,
        alternate: None,
    };
    preflight_garrison_executor(host_snapshot(actor, order.target, 3, 5, 7, 11), request)
        .expect("fixture plans")
}

fn world_for(receipt: GarrisonExecutorReceipt) -> GarrisonWorld {
    GarrisonWorld {
        receipt: Some(receipt),
        target_uid: Some(TARGET.uid),
        ..GarrisonWorld::default()
    }
}

fn garrisoning() -> GarrisonOrderState {
    GarrisonOrderState {
        target: TARGET,
        search: 0,
    }
}

fn image(actor: &UnitWork) -> (Vec<u8>, i32, i32, u32, u64) {
    (
        actor.orders.debug_image(),
        actor.body.x,
        actor.body.y,
        actor.unit_masks,
        path_stack_digest(&actor.path),
    )
}

fn run(actor: &mut UnitWork, world: &mut GarrisonWorld) -> ArmResult {
    let mut pf = PathFinder::new();
    let mut cov = DispatchCoverage::default();
    do_job(actor, world, &mut pf, &mut cov, OrderIndex::Garrison)
}

// ---------------------------------------------------------------------------
// The arm is reached, and reached fail-closed
// ---------------------------------------------------------------------------

#[test]
fn both_status_tables_report_garrison_implemented() {
    assert_eq!(
        EXECUTORS[OrderIndex::Garrison.index()].status,
        ArmStatus::Implemented
    );
    assert_eq!(ARMS[OrderIndex::Garrison.index()], ArmStatus::Implemented);
    assert_eq!(
        EXECUTORS[OrderIndex::Garrison.index()].va,
        Some("0x005E6B80")
    );
    assert_eq!(GARRISON_DISPATCH_OPEN_TAILS.len(), 9);
}

#[test]
fn a_host_without_a_receipt_refuses_and_mutates_nothing() {
    let mut actor = actor_with(garrisoning(), ORDER_GROUP);
    let before = image(&actor);
    let mut world = GarrisonWorld {
        target_uid: Some(TARGET.uid),
        ..GarrisonWorld::default()
    };
    assert_eq!(run(&mut actor, &mut world), ArmResult::HostUnavailable);
    assert_eq!(image(&actor), before);
    assert!(world.effects.is_empty());
}

/// `Unit::work` block H owns the stale-UID gate. A receipt that claims the gate ran while the
/// world disagrees is refused, so the arm cannot act on a recycled owner slot.
#[test]
fn a_recycled_target_slot_refuses_before_any_effect() {
    let mut actor = actor_with(garrisoning(), ORDER_GROUP);
    let receipt = seeded(&actor, garrisoning(), ORDER_GROUP, Some(target_facts()));
    let before = image(&actor);
    let mut world = GarrisonWorld {
        receipt: Some(receipt),
        // The slot now holds a different incarnation.
        target_uid: Some(TARGET.uid ^ 1),
        ..GarrisonWorld::default()
    };
    assert_eq!(run(&mut actor, &mut world), ArmResult::HostUnavailable);
    assert_eq!(image(&actor), before);
    assert!(world.effects.is_empty());
}

#[test]
fn a_garrison_order_without_its_concrete_payload_is_malformed() {
    let mut actor = UnitWork::at(ACTOR.who as u8, ACTOR.o as i16, 0, 0);
    actor
        .orders
        .push_back(OrderRec::of_kind(OrderIndex::Garrison));
    let mut world = GarrisonWorld::default();
    assert_eq!(run(&mut actor, &mut world), ArmResult::MalformedOrder);
}

// ---------------------------------------------------------------------------
// The branches
// ---------------------------------------------------------------------------

#[test]
fn an_inactive_target_retires_the_order_with_the_bare_kill_and_no_feedback() {
    let mut target = target_facts();
    target.is_active = Some(false);
    let mut actor = actor_with(garrisoning(), ORDER_GROUP);
    let receipt = seeded(&actor, garrisoning(), ORDER_GROUP, Some(target));
    let mut world = world_for(receipt);

    let (result, app) = {
        let mut cov = DispatchCoverage::default();
        do_garrison_observed(&mut actor, &mut world, &mut cov)
    };
    assert_eq!(result, ArmResult::Retired(KillReason::Completed));
    let app = app.expect("published");
    assert_eq!(app.branch, GarrisonExecutorBranch::InvalidAdmission);
    assert!(app.killed_locally);
    assert!(app.head_retired);
    assert!(
        world.effects.is_empty(),
        "InvalidAdmission emits no host effect at all"
    );
    assert_eq!(
        actor.orders.front().map(|o| o.kind),
        Some(OrderIndex::Think)
    );
}

/// The order of retirement and feedback is not cosmetic: `HostileTerritory` kills *first*,
/// every other terminal branch kills *last*. A loop that returned on the first kill would
/// silently drop the feedback.
#[test]
fn hostile_territory_kills_first_and_then_emits_feedback_and_sound() {
    let mut target = target_facts();
    target.terrain_owner = Some(6);
    target.target_owner_allied_with_terrain = Some(false);
    let mut actor = actor_with(garrisoning(), ORDER_GROUP);
    let receipt = seeded(&actor, garrisoning(), ORDER_GROUP, Some(target));
    let mut world = world_for(receipt);

    let (result, app) = {
        let mut cov = DispatchCoverage::default();
        do_garrison_observed(&mut actor, &mut world, &mut cov)
    };
    assert_eq!(result, ArmResult::Retired(KillReason::Completed));
    assert_eq!(
        app.expect("published").branch,
        GarrisonExecutorBranch::HostileTerritory
    );
    assert_eq!(
        world.effects,
        vec![
            GarrisonHostStep::LocalFeedback(GarrisonFeedback::HostileTerritory),
            GarrisonHostStep::PlaySound { category: 0x40 },
        ],
        "the kill is local; the two presentation steps must still run after it"
    );
    assert_eq!(
        actor.orders.front().map(|o| o.kind),
        Some(OrderIndex::Think)
    );
}

#[test]
fn a_full_building_with_no_search_word_reports_garrison_full_after_the_kill() {
    let mut target = target_facts();
    target.occupied = Some(4); // limit is 4, actor costs 1
    let mut actor = actor_with(garrisoning(), ORDER_GROUP);
    let receipt = seeded(&actor, garrisoning(), ORDER_GROUP, Some(target));
    let mut world = world_for(receipt);

    let (result, app) = {
        let mut cov = DispatchCoverage::default();
        do_garrison_observed(&mut actor, &mut world, &mut cov)
    };
    assert_eq!(result, ArmResult::Retired(KillReason::Completed));
    assert_eq!(
        app.expect("published").branch,
        GarrisonExecutorBranch::CapacityFull
    );
    assert_eq!(
        world.effects,
        vec![
            GarrisonHostStep::LocalFeedback(GarrisonFeedback::GarrisonFull),
            GarrisonHostStep::PlaySound { category: 0x40 },
        ]
    );
}

/// The `Entered` branch issues no `kill_current_order` of its own. Whether the actor's order
/// leaves the queue is decided by the containment-chain walk, so the dispatcher reports what
/// it observes — both ways round.
#[test]
fn entering_retires_only_when_the_containment_chain_walk_reaches_this_actor() {
    for chain_retires in [false, true] {
        let mut actor = actor_with(garrisoning(), ORDER_GROUP);
        let receipt = seeded(&actor, garrisoning(), ORDER_GROUP, Some(target_facts()));
        let mut world = GarrisonWorld {
            chain_retires_actor: chain_retires,
            ..world_for(receipt)
        };

        let (result, app) = {
            let mut cov = DispatchCoverage::default();
            do_garrison_observed(&mut actor, &mut world, &mut cov)
        };
        let app = app.expect("published");
        assert_eq!(app.branch, GarrisonExecutorBranch::Entered);
        assert!(app.entered, "Unit::go_inside ran");
        assert!(
            !app.killed_locally,
            "the Entered branch never issues kill_current_order itself"
        );
        assert_eq!(app.head_retired, chain_retires);
        assert_eq!(
            result,
            if chain_retires {
                ArmResult::Retired(KillReason::Completed)
            } else {
                ArmResult::Working
            }
        );
        assert_eq!(
            world.effects,
            vec![
                GarrisonHostStep::GoInside {
                    captain_o: ACTOR.o,
                    target_o: TARGET.o,
                    target_who: TARGET.who,
                    arg3: 0,
                },
                GarrisonHostStep::ReadTargetIsBuild { value: true },
                GarrisonHostStep::SetOptionsRebuild { value: 1 },
                GarrisonHostStep::KillCaptainGarrisonOrder {
                    captain_o: ACTOR.o,
                    arg: 0,
                },
            ]
        );
    }
}

// ---------------------------------------------------------------------------
// Receipt binding
// ---------------------------------------------------------------------------

#[test]
fn a_receipt_taken_against_a_different_queue_is_refused() {
    let mut actor = actor_with(garrisoning(), ORDER_GROUP);
    let receipt = seeded(&actor, garrisoning(), ORDER_GROUP, Some(target_facts()));
    actor
        .orders
        .push_back(OrderRec::of_kind(OrderIndex::Attack));
    let before = image(&actor);
    let mut world = world_for(receipt);
    assert_eq!(run(&mut actor, &mut world), ArmResult::HostUnavailable);
    assert_eq!(image(&actor), before);
    assert!(world.effects.is_empty());
}

#[test]
fn a_receipt_whose_order_flags_disagree_with_the_live_node_is_refused() {
    let mut actor = actor_with(garrisoning(), ORDER_GROUP);
    // The host looked at a non-group order; the live node carries ORDER_GROUP.
    let receipt = seeded(&actor, garrisoning(), 0, Some(target_facts()));
    let before = image(&actor);
    let mut world = world_for(receipt);
    assert_eq!(run(&mut actor, &mut world), ArmResult::HostUnavailable);
    assert_eq!(image(&actor), before);
}

#[test]
fn a_host_supplied_plan_that_is_not_the_recomputed_plan_is_refused() {
    let actor = actor_with(garrisoning(), ORDER_GROUP);
    let mut receipt = seeded(&actor, garrisoning(), ORDER_GROUP, Some(target_facts()));
    receipt.plan.branch = GarrisonExecutorBranch::CapacityFull;
    assert_eq!(
        validate_garrison_receipt(&receipt, receipt.snapshot).map(|_| ()),
        Err(GarrisonPlanError::ReceiptPlanMismatch)
    );

    let mut actor = actor;
    let before = image(&actor);
    let mut world = world_for(receipt);
    assert_eq!(run(&mut actor, &mut world), ArmResult::HostUnavailable);
    assert_eq!(image(&actor), before);
}

#[test]
fn a_snapshot_naming_another_target_never_becomes_a_receipt() {
    let actor = actor_with(garrisoning(), ORDER_GROUP);
    let order = garrisoning();
    let request = GarrisonExecutorRequest {
        actor: actor_facts(&actor),
        order,
        order_flags: ORDER_GROUP,
        outer_uid_validated: true,
        target: Some(target_facts()),
        approach: None,
        alternate: None,
    };
    let wrong = GarrisonIdentity { o: 41, ..TARGET };
    assert_eq!(
        preflight_garrison_executor(host_snapshot(&actor, wrong, 0, 0, 0, 0), request).map(|_| ()),
        Err(GarrisonPlanError::ReceiptIdentityMismatch)
    );
}
