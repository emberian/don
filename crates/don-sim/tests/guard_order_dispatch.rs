// SPDX-License-Identifier: GPL-3.0-or-later

//! GUARD (arm 12) integration pins.
//!
//! Every test here drives the real `Unit::do_job` `0x00617A10` jump table through
//! [`don_sim::systems::order_dispatch::do_job`] rather than calling the planner directly, so
//! a green result is evidence that the *dispatcher* reaches the arm — which is exactly what
//! the closure row was red for.
//!
//! The two claims worth stating plainly, because the old inventory note (`"calls through to
//! do_move"`) implied neither:
//!
//! * GUARD consumes a canonical `Random::get(0, 0xffff)` draw on its path-backoff branch, and
//! * GUARD can insert a `CAST_SPELL` order through `Unit::add_cast_order` `0x005E4A60`.

use don_sim::order::{ArmStatus, OrderIndex, EXECUTORS, ORDER_GROUP};
use don_sim::systems::guard_dispatch::{
    do_guard_observed, guard_order_rec, order_queue_digest, path_stack_digest,
    preflight_guard_executor, validate_guard_receipt, GuardDispatchReceipt, GuardHostError,
    GuardHostSnapshot, GuardReceiptError, GUARD_DISPATCH_OPEN_TAILS,
};
use don_sim::systems::guard_order::{
    GuardActorFacts, GuardExecutorBranch, GuardExecutorRequest, GuardHostStep, GuardIdentity,
    GuardOrderState, GuardPostMoveFacts, GuardSpatialFacts, GuardTargetFacts,
};
use don_sim::systems::movement::{PathFinder, UnitWorld};
use don_sim::systems::order_dispatch::{
    do_job, AirPatrolSearch, ArmResult, AttackOutcome, DispatchCoverage, GatherOutcome, KillReason,
    OrderRec, TargetState, UnitWork, WorkWorld, ARMS,
};
use don_sim::systems::patrol::{AirPatrolOrder, AirPatrolTarget, GroupMoveRequest};

// ---------------------------------------------------------------------------
// A GUARD host that serves exactly one prepared receipt
// ---------------------------------------------------------------------------

#[derive(Default)]
struct GuardWorld {
    frame: i32,
    receipt: Option<GuardDispatchReceipt>,
    effects: Vec<GuardHostStep>,
    /// Every `DrawGameRandom` step the arm forwarded; the canonical stream would advance once
    /// per entry.
    rng_draws: Vec<i32>,
}

impl UnitWorld for GuardWorld {
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

impl WorkWorld for GuardWorld {
    fn frame(&self) -> i32 {
        self.frame
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
        unreachable!("GUARD tests do not dispatch patrol")
    }
    fn air_patrol_physics(
        &mut self,
        _: &mut UnitWork,
        _: &mut AirPatrolOrder,
        _: i32,
        _: i32,
    ) -> bool {
        unreachable!("GUARD tests do not dispatch patrol")
    }
    fn air_patrol_unit_target(
        &mut self,
        _: &UnitWork,
        _: &AirPatrolOrder,
        _: i32,
        _: i32,
        _: AirPatrolSearch,
    ) -> Option<AirPatrolTarget> {
        unreachable!("GUARD tests do not dispatch patrol")
    }
    fn air_patrol_building_target(
        &mut self,
        _: &UnitWork,
        _: &AirPatrolOrder,
        _: i32,
        _: i32,
    ) -> Option<AirPatrolTarget> {
        unreachable!("GUARD tests do not dispatch patrol")
    }
    fn group_patrol_move(&mut self, _: &mut UnitWork, _: GroupMoveRequest) {
        unreachable!("GUARD tests do not dispatch patrol")
    }
    fn patrol_actor_is_type(&self, _: &UnitWork, _: i32, _: bool) -> bool {
        unreachable!("GUARD tests do not dispatch patrol")
    }
    fn patrol_inside_is_scramblable(&self, _: u8, _: i16) -> bool {
        unreachable!("GUARD tests do not dispatch patrol")
    }
    fn patrol_scramble_inside(&mut self, _: &mut UnitWork, _: i16) {
        unreachable!("GUARD tests do not dispatch patrol")
    }

    fn guard_preflight(
        &mut self,
        _: &UnitWork,
        _: &OrderRec,
    ) -> Result<GuardDispatchReceipt, GuardHostError> {
        self.receipt.clone().ok_or(GuardHostError::Unavailable)
    }

    fn guard_effect(&mut self, _: &mut UnitWork, _: &OrderRec, step: GuardHostStep) {
        if let GuardHostStep::DrawGameRandom { result, .. } = step {
            self.rng_draws.push(result);
        }
        self.effects.push(step);
    }
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const ACTOR: GuardIdentity = GuardIdentity {
    o: 7,
    who: 2,
    uid: 0x3311,
};
const TARGET: GuardIdentity = GuardIdentity {
    o: 19,
    who: 3,
    uid: 0x5544,
};

/// `o = 7`, `frame = 0`: `phase = 7`, so neither `phase % 16 == 0` (idle pulse) nor
/// `(phase + 8) % 16 == 0` (melee scan) fires. Both gates are exercised separately.
const FRAME: i32 = 0;

fn actor_at(x: i32, y: i32) -> UnitWork {
    let mut actor = UnitWork::at(ACTOR.who as u8, ACTOR.o as i16, x, y);
    actor.uid = ACTOR.uid;
    actor
}

fn actor_facts(actor: &UnitWork) -> GuardActorFacts {
    GuardActorFacts {
        identity: ACTOR,
        x: actor.body.x,
        y: actor.body.y,
        angle: actor.body.angle,
        frame: FRAME,
        unit_masks: actor.unit_masks,
        type_flags_2b4: 0,
        type_flags_2b8: 0,
        type_slot_10c: false,
        actor_slot_cc: false,
        actor_slot_c4: false,
        actor_slot_100: false,
        is_unpacking: false,
        is_type_7b: false,
        is_captain: false,
    }
}

fn target_facts() -> GuardTargetFacts {
    GuardTargetFacts {
        identity: TARGET,
        active: true,
        initial_valid_unit: true,
        initial_on_map: Some(true),
        valid_unit: true,
        on_map: Some(true),
        is_moving: false,
        is_wallbuild: false,
        x: 0x1800,
        y: 0x1800,
        angle: 0,
        unit_masks: 0,
        attack_dist: 0,
        x_size: 1,
        y_size: 1,
    }
}

/// Spatial facts whose `chosen_div3_*` place the guard post at `(cell * 0x30 + 0x18)`.
fn spatial_at(
    chosen_cell: (i32, i32),
    actor_cell: (i32, i32),
    guard_cell: (i32, i32),
) -> GuardSpatialFacts {
    GuardSpatialFacts {
        sin_dy: 0,
        cos_dx: 0,
        cos_dy: 0,
        sin_dx: 0,
        projected_div3_x: chosen_cell.0,
        projected_div3_y: chosen_cell.1,
        invalid_loc: false,
        terrain_flags: 0,
        building_spot: None,
        primary_spot: None,
        secondary_spot: None,
        chosen_div3_x: chosen_cell.0,
        chosen_div3_y: chosen_cell.1,
        actor_cell_x: actor_cell.0,
        actor_cell_y: actor_cell.1,
        guard_cell_x: guard_cell.0,
        guard_cell_y: guard_cell.1,
        find_angle: Some(0x2000_0000),
    }
}

fn post(x: i32) -> i32 {
    x * 0x30 + 0x18
}

/// Install a GUARD head order plus one queued follower, and seed a matching receipt.
fn fixture(
    order: GuardOrderState,
    actor_pos: (i32, i32),
    target: Option<GuardTargetFacts>,
    spatial: Option<GuardSpatialFacts>,
    post_move: Option<GuardPostMoveFacts>,
) -> (UnitWork, GuardWorld) {
    fixture_from(
        actor_at(actor_pos.0, actor_pos.1),
        order,
        target,
        spatial,
        post_move,
    )
}

fn fixture_from(
    mut actor: UnitWork,
    order: GuardOrderState,
    target: Option<GuardTargetFacts>,
    spatial: Option<GuardSpatialFacts>,
    post_move: Option<GuardPostMoveFacts>,
) -> (UnitWork, GuardWorld) {
    actor.orders.push_back(guard_order_rec(order, ORDER_GROUP));
    actor.orders.push_back(OrderRec::of_kind(OrderIndex::Think));

    let request = GuardExecutorRequest {
        actor: actor_facts(&actor),
        order,
        target,
        spatial,
        post_move,
    };
    let snapshot = GuardHostSnapshot {
        actor: ACTOR,
        target: order.target,
        frame: FRAME,
        queue_digest: order_queue_digest(&actor.orders),
        path_digest: path_stack_digest(&actor.path),
        external_epoch: 41,
        rng_epoch: 97,
    };
    let receipt = preflight_guard_executor(snapshot, request).expect("fixture plans");
    let world = GuardWorld {
        frame: FRAME,
        receipt: Some(receipt),
        ..GuardWorld::default()
    };
    (actor, world)
}

fn guarding(target: GuardIdentity) -> GuardOrderState {
    GuardOrderState {
        target,
        ..GuardOrderState::default()
    }
}

/// Every checksum-visible field this arm can touch, so "zero mutation" is a real assertion
/// rather than a spot check. `UnitWork` has no `PartialEq`, hence the explicit projection.
fn image(actor: &UnitWork) -> (Vec<u8>, i32, i32, i32, u32, u32, u8, u64, i32) {
    (
        actor.orders.debug_image(),
        actor.body.x,
        actor.body.y,
        actor.body.angle,
        actor.unit_masks,
        actor.unit_masks2,
        actor.idle,
        path_stack_digest(&actor.path),
        actor.dest_angle,
    )
}

fn run(actor: &mut UnitWork, world: &mut GuardWorld) -> ArmResult {
    let mut pf = PathFinder::new();
    let mut cov = DispatchCoverage::default();
    do_job(actor, world, &mut pf, &mut cov, OrderIndex::Guard)
}

// ---------------------------------------------------------------------------
// The arm is reached, and reached fail-closed
// ---------------------------------------------------------------------------

#[test]
fn the_jump_table_row_and_the_dispatcher_row_both_report_guard_implemented() {
    assert_eq!(
        EXECUTORS[OrderIndex::Guard.index()].status,
        ArmStatus::Implemented
    );
    assert_eq!(ARMS[OrderIndex::Guard.index()], ArmStatus::Implemented);
    assert_eq!(EXECUTORS[OrderIndex::Guard.index()].va, Some("0x005E5C70"));
    // Seven surfaces are still host-owned. If that list ever empties without the row's note
    // changing, the note has started overclaiming.
    assert_eq!(GUARD_DISPATCH_OPEN_TAILS.len(), 7);
}

#[test]
fn a_host_without_a_receipt_refuses_and_mutates_nothing() {
    let (mut actor, _) = fixture(
        guarding(TARGET),
        (0x1000, 0x1000),
        Some(target_facts()),
        Some(spatial_at((0x40, 0x40), (1, 1), (4, 4))),
        Some(GuardPostMoveFacts {
            coarse_manhattan: 3,
            head_is_guard: false,
            random_draw: None,
        }),
    );
    let before = image(&actor);
    let mut world = GuardWorld {
        frame: FRAME,
        ..GuardWorld::default()
    };
    assert_eq!(run(&mut actor, &mut world), ArmResult::HostUnavailable);
    assert_eq!(
        image(&actor),
        before,
        "a refused GUARD frame must be zero-mutation"
    );
    assert!(world.effects.is_empty());
}

#[test]
fn a_guard_order_without_its_concrete_payload_is_malformed() {
    let mut actor = actor_at(0x1000, 0x1000);
    actor.orders.push_back(OrderRec::of_kind(OrderIndex::Guard));
    let mut world = GuardWorld {
        frame: FRAME,
        ..GuardWorld::default()
    };
    assert_eq!(run(&mut actor, &mut world), ArmResult::MalformedOrder);
}

// ---------------------------------------------------------------------------
// The branches
// ---------------------------------------------------------------------------

#[test]
fn an_unaddressed_target_retires_the_order_after_the_idle_animation() {
    let order = guarding(GuardIdentity {
        o: -1,
        who: -1,
        uid: u16::MAX,
    });
    let (mut actor, mut world) = fixture(order, (0x1000, 0x1000), None, None, None);
    assert_eq!(
        run(&mut actor, &mut world),
        ArmResult::Retired(KillReason::Completed)
    );
    // `set_anim(0, 0, 1)` runs *before* the bare kill [measured order in `guard_order`].
    assert_eq!(
        world.effects,
        vec![GuardHostStep::SetAnimation {
            animation: 0,
            arg0: 0,
            arg1: 1
        }]
    );
    assert_eq!(
        actor.orders.front().map(|o| o.kind),
        Some(OrderIndex::Think)
    );
}

#[test]
fn a_nonzero_retry_decrements_the_payload_and_does_nothing_else() {
    let mut order = guarding(TARGET);
    order.retry = 7;
    let (mut actor, mut world) = fixture(order, (0x1000, 0x1000), Some(target_facts()), None, None);
    assert_eq!(run(&mut actor, &mut world), ArmResult::Working);
    assert!(world.effects.is_empty());
    let head = actor.orders.front().expect("GUARD stays current");
    assert_eq!(head.kind, OrderIndex::Guard);
    assert_eq!(head.guard.expect("payload survives").retry, 6);
    assert_eq!(head.guard.expect("payload survives").idle, 0);
}

#[test]
fn the_periodic_melee_scan_fires_eight_frames_out_of_phase_with_the_idle_pulse() {
    // `phase = o + frame`; the melee gate is `(phase + 8) % 16 == 0`, so `o = 7` needs
    // `frame = 1`.
    let order = guarding(TARGET);
    let mut actor = actor_at(0x1000, 0x1000);
    actor.orders.push_back(guard_order_rec(order, ORDER_GROUP));
    let mut facts = actor_facts(&actor);
    facts.frame = 1;
    let request = GuardExecutorRequest {
        actor: facts,
        order,
        target: Some(target_facts()),
        spatial: None,
        post_move: None,
    };
    let snapshot = GuardHostSnapshot {
        actor: ACTOR,
        target: TARGET,
        frame: 1,
        queue_digest: order_queue_digest(&actor.orders),
        path_digest: path_stack_digest(&actor.path),
        external_epoch: 1,
        rng_epoch: 1,
    };
    let mut world = GuardWorld {
        frame: 1,
        receipt: Some(preflight_guard_executor(snapshot, request).expect("plans")),
        ..GuardWorld::default()
    };
    let (result, app) = {
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        do_guard_observed(&mut actor, &mut world, &mut pf, &mut cov)
    };
    assert_eq!(result, ArmResult::Working);
    assert_eq!(app.unwrap().branch, GuardExecutorBranch::PeriodicMeleeScan);
    assert_eq!(
        world.effects,
        vec![GuardHostStep::FindMeleeTarget {
            args: [-1, 0, 0, 1, 0]
        }]
    );
}

#[test]
fn standing_on_the_guard_post_increments_idle_and_writes_the_snapped_post() {
    let cell = (0x20, 0x21);
    let (mut actor, mut world) = fixture(
        guarding(TARGET),
        (post(cell.0), post(cell.1)),
        Some(target_facts()),
        Some(spatial_at(cell, (9, 9), (9, 9))),
        None,
    );
    assert_eq!(run(&mut actor, &mut world), ArmResult::Working);
    let head = actor.orders.front().expect("GUARD stays current");
    let state = head.guard.expect("payload survives");
    assert_eq!(state.guard_x, post(cell.0));
    assert_eq!(state.guard_y, post(cell.1));
    assert_eq!(state.idle, 1);
    assert_eq!(actor.orders.len(), 2, "no movement leg is inserted");
    // A stationary target and a facing mismatch turn the post into `Unit::set_angle`
    // `0x00605400` first, then the idle animation.
    assert_eq!(
        world.effects,
        vec![
            GuardHostStep::SetAngle {
                angle: 0x2000_0000,
                update_position: 0
            },
            GuardHostStep::SetAnimation {
                animation: 0,
                arg0: 0,
                arg1: 1
            }
        ]
    );
}

/// The claim the old inventory note made impossible to anticipate: a GUARD that has been
/// standing still long enough issues `Unit::add_cast_order` `0x005E4A60` with spell `0x28C`.
#[test]
fn a_long_idle_auto_caster_inserts_a_cast_spell_order() {
    let cell = (0x20, 0x20);
    let mut order = guarding(TARGET);
    order.idle = 69; // one short of the 70-tick slow threshold
    let mut actor = actor_at(post(cell.0), post(cell.1));
    // `type_flags_2b8 & 4`, `unit_masks & 0x80000`, not unpacking, actor slot 0x100 clear.
    actor.unit_masks = 0x0008_0000;
    actor.type_snap_arm = true;
    actor.orders.push_back(guard_order_rec(order, ORDER_GROUP));

    let mut facts = actor_facts(&actor);
    facts.type_flags_2b8 = 4;
    let request = GuardExecutorRequest {
        actor: facts,
        order,
        target: Some(target_facts()),
        spatial: Some(spatial_at(cell, (9, 9), (9, 9))),
        post_move: None,
    };
    let snapshot = GuardHostSnapshot {
        actor: ACTOR,
        target: TARGET,
        frame: FRAME,
        queue_digest: order_queue_digest(&actor.orders),
        path_digest: path_stack_digest(&actor.path),
        external_epoch: 5,
        rng_epoch: 5,
    };
    let mut world = GuardWorld {
        frame: FRAME,
        receipt: Some(preflight_guard_executor(snapshot, request).expect("plans")),
        ..GuardWorld::default()
    };
    let (result, app) = {
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        do_guard_observed(&mut actor, &mut world, &mut pf, &mut cov)
    };
    assert_eq!(result, ArmResult::Working);
    let app = app.expect("published");
    assert_eq!(app.branch, GuardExecutorBranch::AutoCast);
    assert!(app.inserted_cast, "GUARD reaches Unit::add_cast_order");
    assert_eq!(
        world.effects,
        vec![
            GuardHostStep::SetAngle {
                angle: 0x2000_0000,
                update_position: 0
            },
            GuardHostStep::AddCastOrder {
                target_o: -1,
                target_who: -1,
                x: -1,
                y: -1,
                cast_type: 0x28c,
                queue_pos: 0,
                arg7: 0,
            }
        ]
    );
    assert_eq!(actor.orders.front().unwrap().guard.unwrap().idle, 70);
}

/// The insertion tail, verbatim: a movement leg goes in at `QueuePos::First`, takes the
/// `max(1, coarse manhattan) * 30` pause, and `do_move` runs in the same tick.
#[test]
fn leaving_the_guard_cell_inserts_a_paused_move_and_drives_it_the_same_tick() {
    let cell = (0x20, 0x20);
    // 100 world units is further than the unit's default 48-unit tolerance, so `do_move`
    // does not complete the leg on the insertion tick; `masks::PATH_EXHAUSTED` keeps the
    // search out of it so the frame ends on retail's `MoveOrder::pause` arm at `0x005F8C88`.
    let mut actor = actor_at(post(cell.0) - 100, post(cell.1));
    actor.unit_masks |= 0x0000_0008;
    let (mut actor, mut world) = fixture_from(
        actor,
        guarding(TARGET),
        Some(target_facts()),
        Some(spatial_at(cell, (8, 8), (9, 9))),
        Some(GuardPostMoveFacts {
            coarse_manhattan: 4,
            head_is_guard: false,
            random_draw: None,
        }),
    );
    let (result, app) = {
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        do_guard_observed(&mut actor, &mut world, &mut pf, &mut cov)
    };
    assert_eq!(result, ArmResult::Working);
    let app = app.expect("published");
    assert_eq!(app.branch, GuardExecutorBranch::MoveStillCurrent);
    assert!(app.inserted_move);
    assert_eq!(app.post_move_head_is_guard, Some(false));
    assert!(!app.drew_random, "the no-backoff branch draws no random");

    let head = actor.orders.front().expect("the inserted leg is current");
    assert_eq!(head.kind, OrderIndex::MoveTo);
    assert_eq!((head.x, head.y), (post(cell.0), post(cell.1)));
    // 4 * 30 = 120, then `do_move`'s pause arm decrements it once in the same tick.
    assert_eq!(head.pause, 119);
    assert_eq!(actor.orders.len(), 3);
    let guard = actor
        .orders
        .iter()
        .find(|o| o.kind == OrderIndex::Guard)
        .and_then(|o| o.guard)
        .expect("the GUARD order is still queued behind the move");
    assert_eq!(guard.idle, 0, "leaving the post clears idle");
    assert_eq!(guard.guard_x, post(cell.0));
}

/// The path-backoff tail: when the same-tick `do_move` retires the inserted leg, GUARD is the
/// head order again and installs a 6-to-8 tick retry from a canonical RNG draw.
#[test]
fn a_move_that_retires_in_the_same_tick_installs_a_random_backoff() {
    let cell = (0x20, 0x20);
    let mut actor = actor_at(post(cell.0) - 30, post(cell.1));
    // A tolerance wider than the leg makes `do_move`'s arrival arm retire it immediately,
    // which is the state retail reaches through its own path-failure cascade.
    actor.tolerance = 0x100;
    let order = guarding(TARGET);
    actor.orders.push_back(guard_order_rec(order, ORDER_GROUP));

    let request = GuardExecutorRequest {
        actor: actor_facts(&actor),
        order,
        target: Some(target_facts()),
        spatial: Some(spatial_at(cell, (8, 8), (9, 9))),
        post_move: Some(GuardPostMoveFacts {
            coarse_manhattan: 4,
            head_is_guard: true,
            random_draw: Some(40_000),
        }),
    };
    let snapshot = GuardHostSnapshot {
        actor: ACTOR,
        target: TARGET,
        frame: FRAME,
        queue_digest: order_queue_digest(&actor.orders),
        path_digest: path_stack_digest(&actor.path),
        external_epoch: 11,
        rng_epoch: 13,
    };
    let mut world = GuardWorld {
        frame: FRAME,
        receipt: Some(preflight_guard_executor(snapshot, request).expect("plans")),
        ..GuardWorld::default()
    };
    let (result, app) = {
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        do_guard_observed(&mut actor, &mut world, &mut pf, &mut cov)
    };
    assert_eq!(result, ArmResult::Working);
    let app = app.expect("published");
    assert_eq!(app.branch, GuardExecutorBranch::RetryAfterMove);
    assert_eq!(app.post_move_head_is_guard, Some(true));
    assert!(app.drew_random, "GUARD consumes the canonical RNG here");
    assert_eq!(world.rng_draws, vec![40_000]);

    let head = actor.orders.front().expect("GUARD is current again");
    assert_eq!(head.kind, OrderIndex::Guard);
    // `retry = draw % 3 + 6` [measured, `0x005E6566..0x005E657A`].
    assert_eq!(head.guard.unwrap().retry, 40_000 % 3 + 6);
    assert_eq!(head.guard.unwrap().retry, 7);
    // The animation write precedes the draw.
    assert_eq!(
        world.effects,
        vec![
            GuardHostStep::SetAnimation {
                animation: 0,
                arg0: 0,
                arg1: 1
            },
            GuardHostStep::DrawGameRandom {
                low: 0,
                high: 0xffff,
                result: 40_000
            },
        ]
    );
}

// ---------------------------------------------------------------------------
// Receipt binding — the checks that make a green run mean something
// ---------------------------------------------------------------------------

#[test]
fn a_receipt_taken_against_a_different_actor_position_is_refused() {
    let cell = (0x20, 0x20);
    let (mut actor, mut world) = fixture(
        guarding(TARGET),
        (post(cell.0) - 30, post(cell.1)),
        Some(target_facts()),
        Some(spatial_at(cell, (8, 8), (9, 9))),
        Some(GuardPostMoveFacts {
            coarse_manhattan: 4,
            head_is_guard: false,
            random_draw: None,
        }),
    );
    // Move the unit after the host looked.
    actor.body.x += 1;
    let before = image(&actor);
    assert_eq!(run(&mut actor, &mut world), ArmResult::HostUnavailable);
    assert_eq!(image(&actor), before);
    assert!(world.effects.is_empty());
}

#[test]
fn a_receipt_taken_against_a_different_queue_is_refused() {
    let cell = (0x20, 0x20);
    let (mut actor, mut world) = fixture(
        guarding(TARGET),
        (post(cell.0) - 30, post(cell.1)),
        Some(target_facts()),
        Some(spatial_at(cell, (8, 8), (9, 9))),
        Some(GuardPostMoveFacts {
            coarse_manhattan: 4,
            head_is_guard: false,
            random_draw: None,
        }),
    );
    // A queued order appeared between the host lookup and the commit.
    actor
        .orders
        .push_back(OrderRec::of_kind(OrderIndex::Attack));
    let before = image(&actor);
    assert_eq!(run(&mut actor, &mut world), ArmResult::HostUnavailable);
    assert_eq!(image(&actor), before);
    assert!(world.effects.is_empty());
}

#[test]
fn a_host_supplied_plan_that_is_not_the_recomputed_plan_is_refused() {
    let cell = (0x20, 0x20);
    let (actor, world) = fixture(
        guarding(TARGET),
        (post(cell.0), post(cell.1)),
        Some(target_facts()),
        Some(spatial_at(cell, (9, 9), (9, 9))),
        None,
    );
    let mut receipt = world.receipt.expect("fixture seeded a receipt");
    // Swap in a plan the request does not produce.
    receipt.plan.branch = GuardExecutorBranch::HoldNearWallBuild;
    assert_eq!(
        validate_guard_receipt(&receipt, receipt.snapshot),
        Err(GuardReceiptError::PlanMismatch)
    );

    let mut actor = actor;
    let mut world = GuardWorld {
        frame: FRAME,
        receipt: Some(receipt),
        ..GuardWorld::default()
    };
    let before = image(&actor);
    assert_eq!(run(&mut actor, &mut world), ArmResult::HostUnavailable);
    assert_eq!(image(&actor), before);
}

#[test]
fn a_snapshot_naming_another_actor_never_becomes_a_receipt() {
    let actor = actor_at(0x1000, 0x1000);
    let order = guarding(TARGET);
    let request = GuardExecutorRequest {
        actor: actor_facts(&actor),
        order,
        target: Some(target_facts()),
        spatial: None,
        post_move: None,
    };
    let snapshot = GuardHostSnapshot {
        actor: GuardIdentity {
            o: 8,
            who: 2,
            uid: 0x3311,
        },
        target: TARGET,
        frame: FRAME,
        queue_digest: 0,
        path_digest: 0,
        external_epoch: 0,
        rng_epoch: 0,
    };
    assert_eq!(
        preflight_guard_executor(snapshot, request).map(|_| ()),
        Err(GuardReceiptError::IdentityMismatch)
    );
}

#[test]
fn the_queue_digest_actually_separates_two_different_queues() {
    let mut a = actor_at(0, 0);
    a.orders
        .push_back(guard_order_rec(guarding(TARGET), ORDER_GROUP));
    let mut b = actor_at(0, 0);
    b.orders
        .push_back(guard_order_rec(guarding(TARGET), ORDER_GROUP));
    assert_eq!(
        order_queue_digest(&a.orders),
        order_queue_digest(&b.orders),
        "identical queues must agree"
    );
    b.orders.push_back(OrderRec::of_kind(OrderIndex::Think));
    assert_ne!(order_queue_digest(&a.orders), order_queue_digest(&b.orders));
}
