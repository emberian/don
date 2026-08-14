// SPDX-License-Identifier: GPL-3.0-or-later
//! `systems::guard_dispatch` — the `do_job` arm 12 adapter over the `Unit::do_guard`
//! `0x005E5C70` planner in [`crate::systems::guard_order`].
//!
//! # What this module adds, and what it deliberately does not claim
//!
//! [`crate::systems::guard_order`] is a **decision transcription**: it fixes every branch,
//! every conditionally consumed host fact, and the exact ordered host-call sequence of the
//! 2,392-byte retail executor. It was landed unregistered, so `Unit::do_job` still fell to
//! its default arm for `GUARD` and mutated nothing.
//!
//! This module supplies the missing half: a snapshot-bound atomic host receipt, the *local*
//! effects the dispatcher owns (concrete payload writes, `QueuePos::First` movement
//! insertion, the inserted leg's `pause`, the same-tick `Unit::do_move`, and the bare
//! `Unit::kill_current_order(0)`), and the `ArmResult` the driver reports.
//!
//! Fidelity tier is **C**: derived from the instruction stream and the shipped PDB, exercised
//! by tests against this port, and **not** differentially tested against retail. Nothing here
//! is verified in the proof-assistant sense.
//!
//! # The one measured claim that contradicts the inventory note
//!
//! `schema/simulation-closure.json` carried `GUARD` with the note *"calls through to
//! do_move"*. That is false as a characterization of the arm. [measured, `r2 -c "s
//! 0x005E5C70; pD 2392"` over `ron-bin/riseofnations.exe`] the body issues **30 direct
//! `call rel32`s**, of which **exactly one** targets `Unit::do_move` `0x005F7B30`, at
//! `0x005E6532`, and it is reachable only after the arm has
//!
//! 1. recomputed the guard post and stored it into `GuardOrder::guard_x/guard_y`,
//! 2. inserted a movement node with `Unit::add_move_facing_order` `0x005E55C0` at
//!    `0x005E64AA`, and
//! 3. re-read the head through `Unit::update_order` `0x006179D0` at `0x005E64B1`.
//!
//! So `GUARD` *installs* a move and drives it in the same tick; it does not delegate. The
//! remaining 29 calls are `ObjectData::attack_dist` `0x0064C880`, `Unit::find_melee_target`
//! `0x005FF9C0`, `UnitType::find_nearby_spot` `0x0061DE70` (three sites),
//! `Group::clear` `0x00713E80`, `Group::add` `0x00714350`, `Groups::push_group` `0x0070F9E0`,
//! `Group::action_move_near` `0x00704990`, `sinx` `0x0092D100` and `cosx` `0x0092D0C0` (two
//! sites each), `UnitData::invalid_loc` `0x00607C30`, `vector_dist` `0x0046CFF0`,
//! `find_angle` `0x0092D130`, `Unit::set_angle` `0x00605400`, `UnitData::is_unpacking`
//! `0x0060A4B0`, `Unit::add_cast_order` `0x005E4A60`, `Unit::set_anim` `0x00616F40` (three
//! sites), `UnitData::order_type` `0x00616E80`, `Random::get` `0x00A39D70` and
//! `Unit::kill_current_order` `0x005E2CB0`.
//!
//! Two consequences are load-bearing and are asserted by this module's tests: `GUARD` can
//! **consume a canonical `game_random` draw**, and it can **insert a `CAST_SPELL` order**.
//! An arm sized off the old note would have budgeted for neither.
//!
//! # The retail tail this reproduces verbatim
//!
//! ```text
//!   005e64aa  call 0x5e55c0   ; Unit::add_move_facing_order(guard_x, guard_y, angle, ...)
//!   005e64b1  call 0x6179d0   ; Unit::update_order()  -> the inserted MoveOrder
//!             ...             ; pause = max(1, coarse manhattan) * 30   (MoveOrder+0x24)
//!   005e652c  call 0x6179d0   ; Unit::update_order()
//!   005e6532  call 0x5f7b30   ; Unit::do_move(head)
//!   005e6539  call 0x616e80   ; UnitData::order_type()
//!             cmp eax, 0xc    ; != GUARD -> return, the move is still current
//!   005e654b  call 0x616f40   ; Unit::set_anim(0, 0, 1)
//!   005e6552  call 0x6179d0   ; Unit::update_order()  -> the GUARD order again
//!   005e6566  call 0xa39d70   ; Random::get(0, 0xffff)
//!             ...             ; retry = draw % 3 + 6                    (GuardOrder+0x28)
//! ```
//!
//! `retry` is therefore written **after** the same-tick `do_move`, not with the rest of the
//! payload. [`do_guard`] splits the write for that reason rather than publishing the
//! planner's whole after-image up front.

use crate::order::OrderIndex;
use crate::systems::guard_order::{
    plan_guard_executor, GuardExecutorBranch, GuardExecutorPlan, GuardExecutorRequest,
    GuardHostStep, GuardIdentity, GuardOrderState, GuardPlanError, QUEUE_FIRST,
};
use crate::systems::movement::{PathFinder, PathStack, WCELL};
use crate::systems::order_dispatch::{
    clear_partial_path, do_move, kill_current_order, update_action, update_order, ArmResult,
    DispatchCoverage, KillReason, OrderQueue, OrderRec, UnitWork, WorkWorld,
};

/// `Unit::do_guard` `0x005E5C70`, `S_GPROC32` size from `ron-bin/sbl/rise.pdb`.
pub const UNIT_DO_GUARD_VA: u32 = 0x005e_5c70;
pub const UNIT_DO_GUARD_BYTES: usize = 2_392;
/// The single `call 0x5f7b30` site inside the body [measured].
pub const UNIT_DO_GUARD_DO_MOVE_CALL_VA: u32 = 0x005e_6532;
/// Direct `call rel32` sites in the body [measured].
pub const UNIT_DO_GUARD_DIRECT_CALLS: usize = 30;
/// `Unit::add_move_facing_order` call site, immediately before the `do_move` tail [measured].
pub const UNIT_DO_GUARD_ADD_MOVE_CALL_VA: u32 = 0x005e_64aa;
/// `Random::get(0, 0xffff)` call site [measured].
pub const UNIT_DO_GUARD_RANDOM_CALL_VA: u32 = 0x005e_6566;

// ---------------------------------------------------------------------------
// Dispatcher-observable state fingerprints
// ---------------------------------------------------------------------------

/// FNV-1a over the queue's `debug_image`, so a receipt cannot be replayed against a queue
/// that changed shape between the host lookup and the commit.
///
/// This is **our** freshness device, not a retail field. It is deliberately computed by the
/// dispatcher rather than accepted from the host: a host-supplied digest would prove nothing.
pub fn order_queue_digest(queue: &OrderQueue) -> u64 {
    fnv1a(&queue.debug_image())
}

/// FNV-1a over the unit's `Stack<PathData>` at `UnitData+0xB8`, in stack order.
///
/// Shared with [`crate::systems::garrison_dispatch`]; both arm adapters need the same
/// dispatcher-side fingerprint and duplicating it would let the two drift.
pub fn path_stack_digest(path: &PathStack) -> u64 {
    fnv1a(&path.walk_bytes())
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// ---------------------------------------------------------------------------
// The host boundary
// ---------------------------------------------------------------------------

/// Everything a GUARD receipt is bound to.
///
/// The first five fields are computed by the dispatcher from live state and must be *carried*
/// by the receipt; the last two are host-owned epochs the dispatcher cannot observe and which
/// are therefore host-attested, not verified. That split is the honest boundary: it is what
/// lets a caller say exactly which part of the freshness claim is checked here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuardHostSnapshot {
    pub actor: GuardIdentity,
    pub target: GuardIdentity,
    /// `Game::frame`, the phase input of both periodic gates.
    pub frame: i32,
    pub queue_digest: u64,
    pub path_digest: u64,
    /// Host-attested version of every external object/type/terrain surface the plan reads.
    pub external_epoch: u64,
    /// Host-attested position of the canonical `game_random` stream.
    pub rng_epoch: u64,
}

/// A typed refusal from the GUARD host. `Unavailable` authorizes no mutation at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardHostError {
    /// A reached external surface cannot be served. Zero mutation, zero RNG consumption.
    Unavailable,
    /// The host observed state the retail executor cannot construct.
    InvalidState(&'static str),
}

/// Why a GUARD receipt was rejected. Every variant is zero-mutation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardReceiptError {
    /// The snapshot names an actor or target the request does not.
    IdentityMismatch,
    /// The receipt was taken against different dispatcher state.
    SnapshotMismatch,
    /// The host's plan is not the plan its own facts produce.
    PlanMismatch,
    /// The facts themselves are not a state the retail executor can reach.
    Plan(GuardPlanError),
}

/// An immutable GUARD receipt: the snapshot it was taken against, the complete request, and
/// the plan that request produces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuardDispatchReceipt {
    pub snapshot: GuardHostSnapshot,
    pub request: GuardExecutorRequest,
    pub plan: GuardExecutorPlan,
}

/// Build a receipt after the host's lookups and **before** any mutation is published.
pub fn preflight_guard_executor(
    snapshot: GuardHostSnapshot,
    request: GuardExecutorRequest,
) -> Result<GuardDispatchReceipt, GuardReceiptError> {
    if snapshot.actor != request.actor.identity || snapshot.target != request.order.target {
        return Err(GuardReceiptError::IdentityMismatch);
    }
    if snapshot.frame != request.actor.frame {
        return Err(GuardReceiptError::IdentityMismatch);
    }
    let plan = plan_guard_executor(request).map_err(GuardReceiptError::Plan)?;
    Ok(GuardDispatchReceipt {
        snapshot,
        request,
        plan,
    })
}

/// Revalidate a receipt against a freshly observed snapshot and **recompute** its plan.
///
/// Recomputation is the point: the dispatcher never trusts the host's plan, only its facts.
pub fn validate_guard_receipt(
    receipt: &GuardDispatchReceipt,
    current: GuardHostSnapshot,
) -> Result<&GuardExecutorPlan, GuardReceiptError> {
    if receipt.snapshot != current {
        return Err(GuardReceiptError::SnapshotMismatch);
    }
    let recomputed = plan_guard_executor(receipt.request).map_err(GuardReceiptError::Plan)?;
    if recomputed != receipt.plan {
        return Err(GuardReceiptError::PlanMismatch);
    }
    Ok(&receipt.plan)
}

// ---------------------------------------------------------------------------
// Local effects the dispatcher owns
// ---------------------------------------------------------------------------

/// Which of the eleven planner branches retire the GUARD order.
#[inline]
pub const fn branch_retires_guard(branch: GuardExecutorBranch) -> bool {
    matches!(branch, GuardExecutorBranch::TerminalInvalidTarget)
}

/// Signed remainder of a destination modulo one WCoord cell, as retail stores in
/// `MoveOrder::off_x/off_y` at `+76/+78`.
#[inline]
fn move_remainder(coord: i32) -> i16 {
    let low_coord = i32::from(coord as i16);
    let low_cells = i32::from((coord / WCELL) as i16);
    low_coord.wrapping_sub(low_cells.wrapping_mul(WCELL)) as i16
}

/// `Unit::add_move_facing_order(UCoord, UCoord, int, ...)` `0x005E55C0` as GUARD calls it.
///
/// GUARD passes the already-world-scaled `guard_x/guard_y` it just stored, so unlike
/// `Unit::do_follow`'s call there is no cell-to-world conversion here. The tail tuple is
/// pinned by the planner and re-checked, because calling this with an unmodelled arm would
/// silently install a different order shape.
fn install_guard_move(actor: &mut UnitWork, x: i32, y: i32, angle: i32, tail: [i32; 8]) {
    assert_eq!(tail[0], 2, "GUARD add_move_facing_order arg4");
    assert_eq!(tail[1], 0, "GUARD add_move_facing_order arg5");
    assert_eq!(tail[2], QUEUE_FIRST, "GUARD add_move_facing_order queue");
    assert_eq!(tail[3], 0, "GUARD add_move_facing_order arg7");
    assert_eq!(tail[4], -1, "GUARD add_move_facing_order arg8");
    assert_eq!(tail[5], -1, "GUARD add_move_facing_order coord9");
    assert_eq!(tail[6], -1, "GUARD add_move_facing_order coord10");
    assert_eq!(tail[7], 0, "GUARD add_move_facing_order arg11");
    actor.orders.push_front(OrderRec {
        kind: OrderIndex::MoveTo,
        x,
        y,
        angle,
        dest_x: x,
        dest_y: y,
        facing: tail[4],
        orig_x: tail[5],
        orig_y: tail[6],
        off_x: move_remainder(x),
        off_y: move_remainder(y),
        ..OrderRec::default()
    });
    clear_partial_path(actor);
    update_action(actor);
}

/// What one published GUARD frame actually did.
///
/// Exposed so a test can assert the RNG draw and the CAST insertion really happened rather
/// than inferring them from the returned [`ArmResult`], which cannot distinguish them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuardApplication {
    pub branch: GuardExecutorBranch,
    /// `GuardHostStep`s forwarded to [`WorkWorld::guard_effect`].
    pub host_steps: usize,
    /// The arm consumed the canonical RNG this frame.
    pub drew_random: bool,
    /// The arm inserted a `CAST_SPELL` order through `Unit::add_cast_order`.
    pub inserted_cast: bool,
    /// The arm inserted a `QueuePos::First` movement leg.
    pub inserted_move: bool,
    /// `Some(head_is_guard)` once the same-tick `do_move` has run.
    pub post_move_head_is_guard: Option<bool>,
}

// ---------------------------------------------------------------------------
// The arm
// ---------------------------------------------------------------------------

/// Read the live GUARD payload and prove the flattened `TargetOrder` view agrees with it.
///
/// Retail has one copy; this port keeps `target_o/target_who/target_uid` beside the concrete
/// payload because generic consumers (`Unit::work`'s staleness gate, `update_action`) read
/// the flattened view. A disagreement is state retail cannot construct.
pub fn live_guard_state(order: &OrderRec) -> Option<GuardOrderState> {
    let state = order.guard?;
    if order.kind != OrderIndex::Guard
        || order.target_o != state.target.o
        || order.target_who != state.target.who
        || order.target_uid != state.target.uid
        || order.follow.is_some()
        || order.special_anim.is_some()
        || order.form_order.is_some()
    {
        return None;
    }
    Some(state)
}

/// Construct one GUARD order node with a complete concrete payload.
pub fn guard_order_rec(state: GuardOrderState, flags: u8) -> OrderRec {
    OrderRec {
        kind: OrderIndex::Guard,
        flags,
        target_o: state.target.o,
        target_who: state.target.who,
        target_uid: state.target.uid,
        guard: Some(state),
        ..OrderRec::default()
    }
}

/// `Unit::do_guard(GuardOrder*)` `0x005E5C70` (2,392 B), arm 12.
///
/// The dispatcher acquires one snapshot-bound receipt, **recomputes** the plan from the
/// receipt's facts, verifies every fact it can observe against live state, and only then
/// publishes. A refusal before that point is zero-mutation.
pub fn do_guard<W: WorkWorld>(
    actor: &mut UnitWork,
    world: &mut W,
    pathfinder: &mut PathFinder,
    cov: &mut DispatchCoverage,
) -> ArmResult {
    do_guard_observed(actor, world, pathfinder, cov).0
}

/// [`do_guard`] plus the published-frame observation, for callers that must check *which*
/// effects ran.
pub fn do_guard_observed<W: WorkWorld>(
    actor: &mut UnitWork,
    world: &mut W,
    pathfinder: &mut PathFinder,
    cov: &mut DispatchCoverage,
) -> (ArmResult, Option<GuardApplication>) {
    let Some(order_before) = update_order(actor) else {
        return (ArmResult::NoOrder, None);
    };
    if order_before.kind != OrderIndex::Guard {
        return (ArmResult::MalformedOrder, None);
    }
    let Some(payload_before) = live_guard_state(&order_before) else {
        return (ArmResult::MalformedOrder, None);
    };

    let receipt = match world.guard_preflight(&*actor, &order_before) {
        Ok(receipt) => receipt,
        Err(GuardHostError::Unavailable) => return (ArmResult::HostUnavailable, None),
        Err(GuardHostError::InvalidState(_)) => return (ArmResult::MalformedOrder, None),
    };

    // Everything the dispatcher can see for itself. A host that disagrees with live state on
    // any of these is refused before a single write.
    let observed = GuardHostSnapshot {
        actor: GuardIdentity {
            o: i32::from(actor.o),
            who: i32::from(actor.who),
            uid: actor.uid,
        },
        target: payload_before.target,
        frame: world.frame(),
        queue_digest: order_queue_digest(&actor.orders),
        path_digest: path_stack_digest(&actor.path),
        external_epoch: receipt.snapshot.external_epoch,
        rng_epoch: receipt.snapshot.rng_epoch,
    };
    let facts = receipt.request.actor;
    if receipt.request.order != payload_before
        || facts.identity != observed.actor
        || facts.x != actor.body.x
        || facts.y != actor.body.y
        || facts.angle != actor.body.angle
        || facts.frame != observed.frame
        || facts.unit_masks != actor.unit_masks
        // `UnitWork` already projects two of the type words; the two views must agree.
        || ((facts.type_flags_2b8 & 4) != 0) != actor.type_snap_arm
        || ((facts.type_flags_2b4 & 0x20) != 0) != actor.type_moves_while_turning
    {
        return (ArmResult::HostUnavailable, None);
    }
    let plan = match validate_guard_receipt(&receipt, observed) {
        Ok(plan) => plan.clone(),
        Err(_) => return (ArmResult::HostUnavailable, None),
    };

    apply_guard_plan(actor, world, pathfinder, cov, &order_before, &plan)
}

/// Publish one validated plan in retail's order.
///
/// Local writes (payload, queue, inserted `pause`, same-tick `do_move`) are the dispatcher's;
/// every other step is forwarded to [`WorkWorld::guard_effect`], which an applied receipt has
/// already proven available.
fn apply_guard_plan<W: WorkWorld>(
    actor: &mut UnitWork,
    world: &mut W,
    pathfinder: &mut PathFinder,
    cov: &mut DispatchCoverage,
    order_before: &OrderRec,
    plan: &GuardExecutorPlan,
) -> (ArmResult, Option<GuardApplication>) {
    // `retry` is the one payload word retail writes *after* the same-tick `do_move`
    // (`0x005E6566..`), so it is held back rather than published with the rest.
    let post_move_retry = match plan.branch {
        GuardExecutorBranch::RetryAfterMove => Some(plan.order_after.retry),
        _ => None,
    };
    let mut pre_move = plan.order_after;
    if post_move_retry.is_some() {
        let Some(before) = order_before.guard else {
            return (ArmResult::MalformedOrder, None);
        };
        pre_move.retry = before.retry;
    }

    {
        let Some(current) = actor.orders.front_mut() else {
            return (ArmResult::HostUnavailable, None);
        };
        if current != order_before {
            return (ArmResult::HostUnavailable, None);
        }
        current.guard = Some(pre_move);
        current.target_o = pre_move.target.o;
        current.target_who = pre_move.target.who;
        current.target_uid = pre_move.target.uid;
    }

    let mut app = GuardApplication {
        branch: plan.branch,
        host_steps: 0,
        drew_random: false,
        inserted_cast: false,
        inserted_move: false,
        post_move_head_is_guard: None,
    };

    for step in &plan.steps {
        match *step {
            GuardHostStep::KillCurrentOrder { arg } => {
                assert_eq!(
                    arg, 0,
                    "GUARD only ever issues the bare kill_current_order(0)"
                );
                kill_current_order(actor, KillReason::Completed);
                cov.completed += 1;
                return (ArmResult::Retired(KillReason::Completed), Some(app));
            }
            GuardHostStep::AddMoveFacingOrder(request) => {
                install_guard_move(actor, request.x, request.y, request.angle, request.tail);
                app.inserted_move = true;
            }
            GuardHostStep::SetInsertedMovePause(pause) => {
                assert!(
                    app.inserted_move,
                    "GUARD pause write requires the inserted movement leg"
                );
                let Some(head) = actor.orders.front_mut() else {
                    return (ArmResult::HostUnavailable, Some(app));
                };
                assert_eq!(head.kind, OrderIndex::MoveTo);
                head.pause = pause;
            }
            GuardHostStep::UpdateOrderThenDoMove => {
                assert!(
                    app.inserted_move,
                    "GUARD same-tick do_move requires the inserted movement leg"
                );
                do_move(actor, world, pathfinder, cov);
                let head_is_guard = actor.order_type() == OrderIndex::Guard;
                app.post_move_head_is_guard = Some(head_is_guard);
                assert_eq!(
                    head_is_guard,
                    matches!(plan.branch, GuardExecutorBranch::RetryAfterMove),
                    "GUARD host mispredicted the post-do_move head order",
                );
            }
            GuardHostStep::DrawGameRandom { low, high, .. } => {
                assert_eq!(
                    (low, high),
                    (0, 0xffff),
                    "GUARD draws Random::get(0,0xffff)"
                );
                world.guard_effect(actor, order_before, *step);
                app.drew_random = true;
                app.host_steps += 1;
            }
            GuardHostStep::AddCastOrder { .. } => {
                world.guard_effect(actor, order_before, *step);
                app.inserted_cast = true;
                app.host_steps += 1;
            }
            _ => {
                world.guard_effect(actor, order_before, *step);
                app.host_steps += 1;
            }
        }
    }

    if let Some(retry) = post_move_retry {
        // The GUARD order is the head again; retail reaches it through the second
        // `update_order()` at `0x005E6552` and writes `GuardOrder+0x28`.
        let Some(head) = actor.orders.front_mut() else {
            return (ArmResult::HostUnavailable, Some(app));
        };
        if head.kind != OrderIndex::Guard {
            return (ArmResult::HostUnavailable, Some(app));
        }
        let Some(mut state) = head.guard else {
            return (ArmResult::MalformedOrder, Some(app));
        };
        state.retry = retry;
        head.guard = Some(state);
    }

    debug_assert!(!branch_retires_guard(plan.branch));
    (ArmResult::Working, Some(app))
}

/// Runtime capabilities this adapter still routes to a host rather than owning.
///
/// This is the honest residue: the arm is wired and drives its own queue, but the listed
/// surfaces are host observations or host effects, not port-owned behaviour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardDispatchOpenTail {
    /// `UnitType::find_nearby_spot` `0x0061DE70`, `UnitData::invalid_loc` `0x00607C30`,
    /// `sinx`/`cosx`, `find_angle`, `div_3_table` and the terrain flag word.
    SpatialSearchAndTrig,
    /// `Unit::find_melee_target` `0x005FF9C0`.
    MeleeTargetSearch,
    /// The temporary `Group` + `Group::action_move_near` `0x00704990` wall-build approach.
    TemporaryGroupMoveNear,
    /// `Unit::set_anim` `0x00616F40` and `Unit::set_angle` `0x00605400`.
    AnimationAndFacing,
    /// `Unit::add_cast_order` `0x005E4A60`; CAST_SPELL has no concrete payload here yet.
    AutoCastInsertion,
    /// `Random::get(0, 0xffff)` `0x00A39D70` against the canonical `game_random` stream.
    CanonicalGameRandom,
    /// `Group::action_guard` `0x006FCD30` installation and the live `Sim::do_frame` bridge.
    GroupActionAndLiveTickAdapter,
}

pub const GUARD_DISPATCH_OPEN_TAILS: &[GuardDispatchOpenTail] = &[
    GuardDispatchOpenTail::SpatialSearchAndTrig,
    GuardDispatchOpenTail::MeleeTargetSearch,
    GuardDispatchOpenTail::TemporaryGroupMoveNear,
    GuardDispatchOpenTail::AnimationAndFacing,
    GuardDispatchOpenTail::AutoCastInsertion,
    GuardDispatchOpenTail::CanonicalGameRandom,
    GuardDispatchOpenTail::GroupActionAndLiveTickAdapter,
];
