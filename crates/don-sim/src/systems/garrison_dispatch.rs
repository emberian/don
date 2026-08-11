// SPDX-License-Identifier: GPL-3.0-or-later
//! `systems::garrison_dispatch` — the `do_job` arm 26 adapter over the `Unit::do_garrison`
//! `0x005E6B80` planner in [`crate::systems::garrison_order`].
//!
//! # Shape of the arm, and why almost everything here is a host effect
//!
//! `Unit::do_garrison` is 2,387 bytes [PDB `S_GPROC32` size] and its fifteen reachable
//! branches are already transcribed by [`crate::systems::garrison_order`]. What it *does*,
//! however, is overwhelmingly external: `Unit::go_inside` `0x0061A2E0` moves the actor into
//! another object's containment list, `Unit::find_garrison_build` `0x00605040` searches a
//! city, `BuildTypeData::get_garrison_limit` `0x00633F50` and `ObjectData::num_inside`
//! `0x00646D50` read the target, `LeaderData::is_ally` `0x006EDB50` reads diplomacy, and
//! `Unit::kill_garrison_order` `0x005E2BD0` walks the *containment chain* retiring GARRISON
//! orders on units this dispatcher does not hold.
//!
//! So the split is the opposite of `GUARD`'s. The dispatcher owns exactly one local effect —
//! the bare `Unit::kill_current_order(0)` — and forwards the rest to
//! [`WorkWorld::garrison_effect`] in retail's order, behind a mandatory receipt. That is the
//! same contract `do_build_at` and `do_group_move` already run under, and it is stated here
//! rather than left implicit.
//!
//! Fidelity tier is **C**: derived from the instruction stream plus the shipped PDB, and not
//! differentially tested against retail.
//!
//! # The containment-chain retirement is observed, never predicted
//!
//! `Unit::kill_garrison_order(int)` `0x005E2BD0` reaches the actor's own queue through
//! `UnitData::get_action` `0x00608450`, and when the action is `GARRISON` (`0x1a`) it issues
//! the **failure pair** `Unit::repath` `0x005E29B0` then `Unit::kill_current_order(0)`
//! `0x005E2CB0`, then recurses into the container. Whether that reaches *this* actor depends
//! on a containment chain the dispatcher does not own, so [`do_garrison`] does not predict it:
//! it re-reads its own head order after the host steps and reports what actually happened.

use crate::order::OrderIndex;
use crate::systems::garrison_order::{
    plan_garrison_executor, validate_garrison_receipt, GarrisonExecutorBranch,
    GarrisonExecutorPlan, GarrisonExecutorReceipt, GarrisonHostSnapshot, GarrisonHostStep,
    GarrisonIdentity, GarrisonOrderState, GarrisonPlanError,
};
use crate::systems::guard_dispatch::{order_queue_digest, path_stack_digest};
use crate::systems::order_dispatch::{
    kill_current_order, update_order, ArmResult, DispatchCoverage, KillReason, OrderRec, UnitWork,
    WorkWorld,
};

/// `Unit::do_garrison` `0x005E6B80`, `S_GPROC32` size from `ron-bin/sbl/rise.pdb`.
pub const UNIT_DO_GARRISON_VA: u32 = 0x005e_6b80;
pub const UNIT_DO_GARRISON_BYTES: usize = 2_387;
/// `Unit::go_inside` `0x0061A2E0`, the containment transfer on the `Entered` branch.
pub const UNIT_GO_INSIDE_VA: u32 = 0x0061_a2e0;
/// `Unit::kill_garrison_order` `0x005E2BD0`, the containment-chain retirement walk.
pub const UNIT_KILL_GARRISON_ORDER_VA: u32 = 0x005e_2bd0;
/// `OrderIndex::GARRISON` as `Unit::kill_garrison_order` compares it [measured, `cmp eax, 0x1a`].
pub const GARRISON_ORDER_TYPE_BYTE: i32 = 0x1a;

/// A typed refusal from the GARRISON host. `Unavailable` authorizes no mutation at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GarrisonHostError {
    /// A reached external surface cannot be served. Zero mutation.
    Unavailable,
    /// The host observed state the retail executor cannot construct.
    InvalidState(&'static str),
}

/// Read the live GARRISON payload and prove the flattened `TargetOrder` view agrees with it.
pub fn live_garrison_state(order: &OrderRec) -> Option<GarrisonOrderState> {
    let state = order.garrison?;
    if order.kind != OrderIndex::Garrison
        || order.target_o != state.target.o
        || order.target_who != state.target.who
        || order.target_uid != state.target.uid
        || order.follow.is_some()
        || order.special_anim.is_some()
        || order.form_order.is_some()
        || order.guard.is_some()
    {
        return None;
    }
    Some(state)
}

/// Construct one GARRISON order node with a complete concrete payload.
pub fn garrison_order_rec(state: GarrisonOrderState, flags: u8) -> OrderRec {
    OrderRec {
        kind: OrderIndex::Garrison,
        flags,
        target_o: state.target.o,
        target_who: state.target.who,
        target_uid: state.target.uid,
        garrison: Some(state),
        ..OrderRec::default()
    }
}

/// Which of the fifteen planner branches issue the bare `kill_current_order(0)` themselves.
///
/// `Entered` is deliberately absent: it retires through the containment-chain walk instead,
/// which this dispatcher observes rather than predicts.
#[inline]
pub const fn branch_kills_locally(branch: GarrisonExecutorBranch) -> bool {
    matches!(
        branch,
        GarrisonExecutorBranch::HelicopterAirbaseStrafe
            | GarrisonExecutorBranch::HelicopterAirbaseMove
            | GarrisonExecutorBranch::InvalidAdmission
            | GarrisonExecutorBranch::DiplomacyRejected
            | GarrisonExecutorBranch::NoGarrisonCapacity
            | GarrisonExecutorBranch::IncompatibleActor
            | GarrisonExecutorBranch::ApproachFailed
            | GarrisonExecutorBranch::CityOwnershipRejected
            | GarrisonExecutorBranch::CityMetricRejected
            | GarrisonExecutorBranch::CapacityFullAlternate
            | GarrisonExecutorBranch::CapacityFull
            | GarrisonExecutorBranch::HostileTerritory
    )
}

/// What one published GARRISON frame actually did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GarrisonApplication {
    pub branch: GarrisonExecutorBranch,
    /// `GarrisonHostStep`s forwarded to [`WorkWorld::garrison_effect`].
    pub host_steps: usize,
    /// The dispatcher issued the bare `kill_current_order(0)` itself.
    pub killed_locally: bool,
    /// The actor's own GARRISON head order was gone once every step had run — whether the
    /// dispatcher retired it or the containment-chain walk did.
    pub head_retired: bool,
    /// `Unit::go_inside` ran, i.e. the actor entered the target.
    pub entered: bool,
}

/// `Unit::do_garrison(GarrisonOrder*)` `0x005E6B80` (2,387 B), arm 26.
pub fn do_garrison<W: WorkWorld>(
    actor: &mut UnitWork,
    world: &mut W,
    cov: &mut DispatchCoverage,
) -> ArmResult {
    do_garrison_observed(actor, world, cov).0
}

/// [`do_garrison`] plus the published-frame observation.
pub fn do_garrison_observed<W: WorkWorld>(
    actor: &mut UnitWork,
    world: &mut W,
    cov: &mut DispatchCoverage,
) -> (ArmResult, Option<GarrisonApplication>) {
    let Some(order_before) = update_order(actor) else {
        return (ArmResult::NoOrder, None);
    };
    if order_before.kind != OrderIndex::Garrison {
        return (ArmResult::MalformedOrder, None);
    }
    let Some(payload_before) = live_garrison_state(&order_before) else {
        return (ArmResult::MalformedOrder, None);
    };

    // `Unit::work` block H owns the stale-UID gate; the request's assertion that it ran is
    // checked here against the same `UnitWorld::target` lookup rather than believed.
    let uid_agrees = match world.target(payload_before.target.who, payload_before.target.o) {
        Some(t) => t.uid == payload_before.target.uid,
        None => false,
    };

    let receipt = match world.garrison_preflight(&*actor, &order_before) {
        Ok(receipt) => receipt,
        Err(GarrisonHostError::Unavailable) => return (ArmResult::HostUnavailable, None),
        Err(GarrisonHostError::InvalidState(_)) => return (ArmResult::MalformedOrder, None),
    };

    let actor_identity = GarrisonIdentity {
        o: i32::from(actor.o),
        who: i32::from(actor.who),
        uid: actor.uid,
    };
    let observed = GarrisonHostSnapshot {
        actor: actor_identity,
        target: payload_before.target,
        queue_digest: order_queue_digest(&actor.orders),
        path_digest: path_stack_digest(&actor.path),
        // Host-owned and therefore host-attested, not verified here.
        actor_version: receipt.snapshot.actor_version,
        target_version: receipt.snapshot.target_version,
        external_epoch: receipt.snapshot.external_epoch,
        rng_epoch: receipt.snapshot.rng_epoch,
    };
    let facts = receipt.request.actor;
    if receipt.request.order != payload_before
        || receipt.request.order_flags != order_before.flags
        || receipt.request.outer_uid_validated != uid_agrees
        || !uid_agrees
        || facts.identity != actor_identity
        || facts.x != actor.body.x
        || facts.y != actor.body.y
        || facts.unit_masks != actor.unit_masks
        // `UnitWork` already projects `UnitType[+0x2B4] & 0x20`; the two views must agree.
        || ((facts.type_flags_2b4 & 0x20) != 0) != actor.type_moves_while_turning
    {
        return (ArmResult::HostUnavailable, None);
    }
    let plan = match validate_garrison_receipt(&receipt, observed) {
        Ok(plan) => plan.clone(),
        Err(_) => return (ArmResult::HostUnavailable, None),
    };

    apply_garrison_plan(actor, world, cov, &order_before, &plan)
}

/// Publish one validated plan in retail's order.
fn apply_garrison_plan<W: WorkWorld>(
    actor: &mut UnitWork,
    world: &mut W,
    cov: &mut DispatchCoverage,
    order_before: &OrderRec,
    plan: &GarrisonExecutorPlan,
) -> (ArmResult, Option<GarrisonApplication>) {
    assert_eq!(
        plan.direct_rng_draws, 0,
        "Unit::do_garrison has no direct call into the canonical game RNG"
    );

    let mut app = GarrisonApplication {
        branch: plan.branch,
        host_steps: 0,
        killed_locally: false,
        head_retired: false,
        entered: false,
    };

    for step in &plan.steps {
        match *step {
            GarrisonHostStep::KillCurrentOrder { arg } => {
                assert_eq!(
                    arg, 0,
                    "GARRISON only ever issues the bare kill_current_order(0)"
                );
                // Retail's terminal branches kill first and *then* emit local feedback, so
                // the loop must keep running after this rather than returning.
                kill_current_order(actor, KillReason::Completed);
                app.killed_locally = true;
            }
            GarrisonHostStep::GoInside { .. } => {
                world.garrison_effect(actor, order_before, *step);
                app.entered = true;
                app.host_steps += 1;
            }
            _ => {
                world.garrison_effect(actor, order_before, *step);
                app.host_steps += 1;
            }
        }
    }

    app.head_retired = actor.orders.front() != Some(order_before);
    debug_assert_eq!(
        app.killed_locally,
        branch_kills_locally(plan.branch),
        "local-kill classification disagrees with the plan it was derived from"
    );

    if app.head_retired {
        cov.completed += 1;
        (ArmResult::Retired(KillReason::Completed), Some(app))
    } else {
        (ArmResult::Working, Some(app))
    }
}

/// Build a receipt with the dispatcher-observable half of the snapshot already filled in.
///
/// A host implementation should call this rather than assembling a [`GarrisonHostSnapshot`]
/// by hand, so the digests it publishes are the ones [`do_garrison`] will recompute.
pub fn host_snapshot(
    actor: &UnitWork,
    target: GarrisonIdentity,
    actor_version: u64,
    target_version: u64,
    external_epoch: u64,
    rng_epoch: u64,
) -> GarrisonHostSnapshot {
    GarrisonHostSnapshot {
        actor: GarrisonIdentity {
            o: i32::from(actor.o),
            who: i32::from(actor.who),
            uid: actor.uid,
        },
        target,
        actor_version,
        target_version,
        queue_digest: order_queue_digest(&actor.orders),
        path_digest: path_stack_digest(&actor.path),
        external_epoch,
        rng_epoch,
    }
}

/// Recompute the plan a request produces, for a host assembling its own receipt.
pub fn plan_for(
    request: crate::systems::garrison_order::GarrisonExecutorRequest,
) -> Result<GarrisonExecutorPlan, GarrisonPlanError> {
    plan_garrison_executor(request)
}

/// Assemble a receipt from a snapshot and request without going through the host trait.
pub fn receipt_for(
    snapshot: GarrisonHostSnapshot,
    request: crate::systems::garrison_order::GarrisonExecutorRequest,
) -> Result<GarrisonExecutorReceipt, GarrisonPlanError> {
    crate::systems::garrison_order::preflight_garrison_executor(snapshot, request)
}

/// Runtime capabilities this adapter routes to a host rather than owning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GarrisonDispatchOpenTail {
    /// `Unit::go_inside` `0x0061A2E0` and the containment lists it rewrites.
    ContainmentTransfer,
    /// `Unit::kill_garrison_order` `0x005E2BD0` walking the container chain.
    ContainmentChainRetirement,
    /// `UnitType::find_nearby_spot` `0x0061DE70` and the opaque post-call `ECX` argument the
    /// approach move forwards to `Unit::add_move_order` `0x00616ED0`.
    ApproachSearchAndOpaqueMoveArgument,
    /// `Unit::add_strafe_order` `0x005E48C0` and the `+0x3c` store on the inserted order.
    HelicopterStrafeInsertion,
    /// `LeaderData::is_ally` `0x006EDB50` and the diplomacy tables behind it.
    DiplomacyTables,
    /// `BuildTypeData::get_garrison_limit` `0x00633F50`, `ObjectData::num_inside`
    /// `0x00646D50`, city race/hits and the terrain owner byte.
    TargetCapacityCityAndTerrain,
    /// `Unit::find_garrison_build` `0x00605040` and the alternate-building install.
    AlternateBuildingSearch,
    /// Local-player feedback strings and the `0x40` sound category.
    LocalProductFeedback,
    /// `Group::action_garrison` `0x00700490` installation and the live `Sim::do_frame` bridge.
    GroupActionAndLiveTickAdapter,
}

pub const GARRISON_DISPATCH_OPEN_TAILS: &[GarrisonDispatchOpenTail] = &[
    GarrisonDispatchOpenTail::ContainmentTransfer,
    GarrisonDispatchOpenTail::ContainmentChainRetirement,
    GarrisonDispatchOpenTail::ApproachSearchAndOpaqueMoveArgument,
    GarrisonDispatchOpenTail::HelicopterStrafeInsertion,
    GarrisonDispatchOpenTail::DiplomacyTables,
    GarrisonDispatchOpenTail::TargetCapacityCityAndTerrain,
    GarrisonDispatchOpenTail::AlternateBuildingSearch,
    GarrisonDispatchOpenTail::LocalProductFeedback,
    GarrisonDispatchOpenTail::GroupActionAndLiveTickAdapter,
];
