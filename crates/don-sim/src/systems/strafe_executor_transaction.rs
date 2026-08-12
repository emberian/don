// SPDX-License-Identifier: GPL-3.0-or-later
//! Exclusive, source-only atomic transaction body for one `Unit::do_strafe` activation.
//!
//! This module is intentionally not registered in `systems/mod.rs`. It consumes the recovered
//! STRAFE planner and the registered air-physics proof, applies every locally-owned payload/
//! queue/scalar mutation to detached after-images, and requires full before/after receipts for
//! nested host effects. Nothing is published until [`commit_strafe_frame`] revalidates the exact
//! snapshot and complete canonical image.

use crate::systems::air_physics_frontier::{
    AirOrderState as PhysicsAirOrderState, AirPhysicsCommitReceipt, AirPhysicsReturn,
};
use crate::systems::movement::PathStack;
use crate::systems::order_dispatch::{OrderQueue, OrderRec, PatrolPayload};
use crate::systems::patrol::{self, AirPatrolTarget};
use crate::systems::strafe_order_frontier::{
    ObjectIdentity, StrafeExecutorReceipt, StrafeFrameFacts, StrafeFramePlan, StrafeHostSnapshot,
    StrafePlanError, StrafeStep,
};
use crate::systems::strafe_runtime_authority::{validate_strafe_order, StrafeAuthorityError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalStrafeActor {
    pub identity: ObjectIdentity,
    pub x: i32,
    pub y: i32,
    pub angle: u32,
    pub active: bool,
    /// Retail `UnitData+0xAE`, used by STRAFE as the attack-result latch.
    pub attack_latch: u8,
    pub spell_time: i16,
}

/// Detached image of every directly reached canonical actor surface. World-wide mutations are
/// represented by the two epochs and may change only through a transaction-bound external
/// receipt. This is a prepare/commit image, never a second persistent gameplay owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalStrafeImage {
    pub actor: CanonicalStrafeActor,
    pub orders: OrderQueue,
    pub path: PathStack,
    pub rng_epoch: u64,
    pub external_effect_epoch: u64,
}

/// Full-image receipt for a nested mutation which cannot be replayed by the STRAFE body alone.
/// The `step` must equal the next exact planner step. An air-physics step additionally carries the
/// registered top-level air-physics commit receipt; all other steps must leave that field empty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StrafeExternalReceipt {
    pub transaction: u64,
    pub step: StrafeStep,
    pub before: CanonicalStrafeImage,
    pub after: CanonicalStrafeImage,
    pub air_physics: Option<AirPhysicsCommitReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedStrafeFrame {
    pub transaction: u64,
    pub snapshot: StrafeHostSnapshot,
    pub planner: StrafeExecutorReceipt,
    pub before: CanonicalStrafeImage,
    pub after: CanonicalStrafeImage,
    pub external_receipts: Vec<StrafeExternalReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StrafeFrameCommitReceipt {
    pub transaction: u64,
    pub snapshot: StrafeHostSnapshot,
    pub plan: StrafeFramePlan,
    pub external_steps: usize,
    pub rng_epoch_before: u64,
    pub rng_epoch_after: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StrafeTransactionError {
    Planner(StrafePlanError),
    SnapshotActorMismatch,
    SnapshotEpochMismatch,
    ActorFactsMismatch,
    MissingStrafeOrder,
    StrafePayloadMismatch,
    StrafeTargetHeaderMismatch,
    StrafeAuthority(StrafeAuthorityError),
    MissingExternalReceipt { step: StrafeStep },
    UnexpectedExternalReceipt { step: StrafeStep },
    ExternalTransactionMismatch,
    ExternalStepMismatch,
    ExternalBeforeMismatch,
    ExternalActorIdentityChanged,
    MissingAirPhysicsCommit,
    UnexpectedAirPhysicsCommit,
    InvalidAirPhysicsCommit,
    AirPhysicsTransactionMismatch,
    AirPhysicsActorMismatch,
    AirPhysicsRngMismatch,
    AirPhysicsReturnMismatch,
    StaleSnapshot,
    StaleImage,
}

impl From<StrafePlanError> for StrafeTransactionError {
    fn from(error: StrafePlanError) -> Self {
        Self::Planner(error)
    }
}

impl From<StrafeAuthorityError> for StrafeTransactionError {
    fn from(error: StrafeAuthorityError) -> Self {
        Self::StrafeAuthority(error)
    }
}

fn current_strafe(orders: &OrderQueue) -> Result<&patrol::StrafeOrder, StrafeTransactionError> {
    let Some(order) = orders.front() else {
        return Err(StrafeTransactionError::MissingStrafeOrder);
    };
    let PatrolPayload::Strafe(strafe) = &order.patrol_payload else {
        return Err(StrafeTransactionError::MissingStrafeOrder);
    };
    if order.kind as i32 != 16 {
        return Err(StrafeTransactionError::MissingStrafeOrder);
    }
    if (order.target_o, order.target_who, order.target_uid)
        != (strafe.target_o, strafe.target_who, strafe.target_uid)
    {
        return Err(StrafeTransactionError::StrafeTargetHeaderMismatch);
    }
    validate_strafe_order(strafe)?;
    Ok(strafe)
}

fn current_strafe_node_mut(
    orders: &mut OrderQueue,
) -> Result<&mut OrderRec, StrafeTransactionError> {
    orders.reset();
    let Some(order) = orders.current_mut() else {
        return Err(StrafeTransactionError::MissingStrafeOrder);
    };
    if order.kind as i32 != 16 {
        return Err(StrafeTransactionError::MissingStrafeOrder);
    }
    let PatrolPayload::Strafe(strafe) = &mut order.patrol_payload else {
        return Err(StrafeTransactionError::MissingStrafeOrder);
    };
    validate_strafe_order(strafe)?;
    Ok(order)
}

fn validate_initial(
    snapshot: StrafeHostSnapshot,
    facts: &StrafeFrameFacts,
    image: &CanonicalStrafeImage,
) -> Result<(), StrafeTransactionError> {
    if snapshot.actor != image.actor.identity || snapshot.actor != facts.actor.identity {
        return Err(StrafeTransactionError::SnapshotActorMismatch);
    }
    if snapshot.rng_epoch != image.rng_epoch
        || snapshot.external_effect_epoch != image.external_effect_epoch
    {
        return Err(StrafeTransactionError::SnapshotEpochMismatch);
    }
    if image.actor.x != facts.actor.x
        || image.actor.y != facts.actor.y
        || image.actor.angle != facts.actor.angle
        || image.actor.active != facts.actor.active
        || image.actor.attack_latch != facts.actor.attack_latch
        || image.actor.spell_time != facts.actor.spell_time
        || image.orders.len() as u32 != facts.actor.queue_len
    {
        return Err(StrafeTransactionError::ActorFactsMismatch);
    }
    if current_strafe(&image.orders)? != &facts.order {
        return Err(StrafeTransactionError::StrafePayloadMismatch);
    }
    Ok(())
}

fn is_external(step: &StrafeStep) -> bool {
    matches!(
        step,
        StrafeStep::ThinkBird { .. }
            | StrafeStep::AirPhysics { .. }
            | StrafeStep::AddAirPatrol { .. }
            | StrafeStep::UpdateWork
            | StrafeStep::Die
            | StrafeStep::SetAnimation { .. }
            | StrafeStep::FireAmmo(_)
    )
}

fn bind_air_physics(
    transaction: u64,
    facts: &StrafeFrameFacts,
    external: &StrafeExternalReceipt,
) -> Result<(), StrafeTransactionError> {
    let Some(nested) = external.air_physics.as_ref() else {
        return Err(StrafeTransactionError::MissingAirPhysicsCommit);
    };
    if !nested.validates(&nested.plan) {
        return Err(StrafeTransactionError::InvalidAirPhysicsCommit);
    }
    if nested.snapshot.transaction != transaction {
        return Err(StrafeTransactionError::AirPhysicsTransactionMismatch);
    }
    if i32::from(nested.snapshot.actor.o) != facts.actor.identity.o
        || i32::from(nested.snapshot.actor.who) != facts.actor.identity.who
        || nested.snapshot.actor.uid != facts.actor.identity.uid
    {
        return Err(StrafeTransactionError::AirPhysicsActorMismatch);
    }
    let Some(strafe_physics) = facts.physics.as_ref() else {
        return Err(StrafeTransactionError::MissingAirPhysicsCommit);
    };
    if nested.snapshot.rng_epoch != strafe_physics.rng_epoch_before
        || nested.plan.rng_epoch_after != strafe_physics.rng_epoch_after
        || external.before.rng_epoch != strafe_physics.rng_epoch_before
        || external.after.rng_epoch != strafe_physics.rng_epoch_after
    {
        return Err(StrafeTransactionError::AirPhysicsRngMismatch);
    }
    let continued = nested.plan.returned == AirPhysicsReturn::Continue;
    if continued != strafe_physics.completed {
        return Err(StrafeTransactionError::AirPhysicsReturnMismatch);
    }
    if let Ok(strafe_after) = current_strafe(&external.after.orders) {
        let air = PhysicsAirOrderState {
            home_o: strafe_after.air.oxx,
            home_who: strafe_after.air.whose,
            cruising_alt: strafe_after.air.cruising_alt,
            sharp_turn: strafe_after.air.sharp_turn,
            old: strafe_after.air.old,
            returning: strafe_after.air.returning,
        };
        if air != nested.plan.final_order {
            return Err(StrafeTransactionError::InvalidAirPhysicsCommit);
        }
    }
    Ok(())
}

fn apply_external(
    transaction: u64,
    facts: &StrafeFrameFacts,
    step: &StrafeStep,
    state: &mut CanonicalStrafeImage,
    receipt: &StrafeExternalReceipt,
) -> Result<(), StrafeTransactionError> {
    if receipt.transaction != transaction {
        return Err(StrafeTransactionError::ExternalTransactionMismatch);
    }
    if &receipt.step != step {
        return Err(StrafeTransactionError::ExternalStepMismatch);
    }
    if &receipt.before != state {
        return Err(StrafeTransactionError::ExternalBeforeMismatch);
    }
    if receipt.after.actor.identity != state.actor.identity {
        return Err(StrafeTransactionError::ExternalActorIdentityChanged);
    }
    if matches!(step, StrafeStep::AirPhysics { .. }) {
        bind_air_physics(transaction, facts, receipt)?;
    } else if receipt.air_physics.is_some() {
        return Err(StrafeTransactionError::UnexpectedAirPhysicsCommit);
    }
    *state = receipt.after.clone();
    Ok(())
}

fn apply_local(
    state: &mut CanonicalStrafeImage,
    step: StrafeStep,
) -> Result<(), StrafeTransactionError> {
    match step {
        StrafeStep::StoreTargetPair { o, who } => {
            let header = current_strafe_node_mut(&mut state.orders)?;
            header.target_o = o;
            header.target_who = who;
            let PatrolPayload::Strafe(strafe) = &mut header.patrol_payload else {
                unreachable!()
            };
            strafe.target_o = o;
            strafe.target_who = who;
        }
        StrafeStep::StoreTargetIdentity(target) => {
            let header = current_strafe_node_mut(&mut state.orders)?;
            header.target_o = target.o;
            header.target_who = target.who;
            header.target_uid = target.uid;
            let PatrolPayload::Strafe(strafe) = &mut header.patrol_payload else {
                unreachable!()
            };
            strafe.target_o = target.o;
            strafe.target_who = target.who;
            strafe.target_uid = target.uid;
        }
        StrafeStep::StoreTargetPosition { x, y } => {
            let header = current_strafe_node_mut(&mut state.orders)?;
            let PatrolPayload::Strafe(strafe) = &mut header.patrol_payload else {
                unreachable!()
            };
            strafe.xx = x;
            strafe.yy = y;
        }
        StrafeStep::StoreReturning(returning) => {
            let header = current_strafe_node_mut(&mut state.orders)?;
            let PatrolPayload::Strafe(strafe) = &mut header.patrol_payload else {
                unreachable!()
            };
            strafe.air.returning = returning;
        }
        StrafeStep::KillCurrent => {
            state.orders.reset();
            state.orders.remove_current();
        }
        StrafeStep::InsertStrafeFirst {
            target,
            target_x,
            target_y,
            home_o,
            home_who,
        } => {
            let body = patrol::patrol_strafe_order(
                AirPatrolTarget {
                    o: target.o,
                    who: target.who,
                    uid: target.uid,
                    x: target_x,
                    y: target_y,
                    domain: 2,
                    ever_seen_by_actor: true,
                },
                home_o,
                home_who,
                0,
            );
            state.orders.push_front(OrderRec::strafe(body));
        }
        StrafeStep::StoreAttackLatch(value) => state.actor.attack_latch = value,
        StrafeStep::AddSpellTime(delta) => {
            state.actor.spell_time = state.actor.spell_time.wrapping_add(delta)
        }
        StrafeStep::Hold => {}
        external if is_external(&external) => {
            return Err(StrafeTransactionError::MissingExternalReceipt { step: external })
        }
        other => return Err(StrafeTransactionError::UnexpectedExternalReceipt { step: other }),
    }
    Ok(())
}

/// Prepare the complete frame against detached state. Any missing/forged nested receipt returns
/// an error before the caller's canonical image is borrowed mutably.
pub fn prepare_strafe_frame(
    transaction: u64,
    snapshot: StrafeHostSnapshot,
    facts: StrafeFrameFacts,
    before: CanonicalStrafeImage,
    external_receipts: Vec<StrafeExternalReceipt>,
) -> Result<PreparedStrafeFrame, StrafeTransactionError> {
    validate_initial(snapshot, &facts, &before)?;
    let planner = StrafeExecutorReceipt::preflight(snapshot, facts.clone())?;
    planner.validates(snapshot, &facts)?;

    let mut after = before.clone();
    let mut external = external_receipts.iter();
    for step in planner.plan.steps.iter().cloned() {
        if is_external(&step) {
            let Some(receipt) = external.next() else {
                return Err(StrafeTransactionError::MissingExternalReceipt { step });
            };
            apply_external(transaction, &facts, &step, &mut after, receipt)?;
        } else {
            apply_local(&mut after, step)?;
        }
    }
    if let Some(receipt) = external.next() {
        return Err(StrafeTransactionError::UnexpectedExternalReceipt { step: receipt.step });
    }

    Ok(PreparedStrafeFrame {
        transaction,
        snapshot,
        planner,
        before,
        after,
        external_receipts,
    })
}

/// Revalidate the exact snapshot and full canonical image, then publish one assignment-only
/// after-image. There is no fallible mutation after the two stale checks.
pub fn commit_strafe_frame(
    current_snapshot: StrafeHostSnapshot,
    current: &mut CanonicalStrafeImage,
    prepared: PreparedStrafeFrame,
) -> Result<StrafeFrameCommitReceipt, StrafeTransactionError> {
    if current_snapshot != prepared.snapshot {
        return Err(StrafeTransactionError::StaleSnapshot);
    }
    if current != &prepared.before {
        return Err(StrafeTransactionError::StaleImage);
    }
    let receipt = StrafeFrameCommitReceipt {
        transaction: prepared.transaction,
        snapshot: prepared.snapshot,
        plan: prepared.planner.plan,
        external_steps: prepared.external_receipts.len(),
        rng_epoch_before: prepared.before.rng_epoch,
        rng_epoch_after: prepared.after.rng_epoch,
    };
    *current = prepared.after;
    Ok(receipt)
}
