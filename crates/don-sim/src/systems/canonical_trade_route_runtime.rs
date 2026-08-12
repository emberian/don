// SPDX-License-Identifier: GPL-3.0-or-later
//! Production `Unit::work` adapter for one exact, substantive `TRADE_ROUTE` branch.
//!
//! The complete 4,519-byte executor remains larger than this adapter. This module admits only
//! retail's establishment-refused branch when a queued successor already exists: the exact
//! `TradeOrder` is validated against a revision/digest-bound PE-derived fact snapshot, the
//! frontier proves that the only simulation mutation is `kill_current_order(0)`, and one CAS
//! publishes that removal. Route establishment, destination search, roads, city/caravan pools,
//! contact effects, and `think_caravan` continue to fail closed.

use crate::order::{OrderIndex, OrderList};
use crate::systems::canonical_group_move_host::{
    capture_unit, unit_still_current, PackageError, UnitImage,
};
use crate::systems::economy_order_payload_authority::EconomyOrderPayload;
use crate::systems::movement::PathStack;
use crate::systems::trade_order_frontier::{
    TradeAtomicSnapshot, TradeExecutorBranch, TradeExecutorPlan, TradeExecutorReceipt,
    TradeHostStep, TradeIdentity, TradeOrderState, TradePlanError,
};
use crate::world::World;
use crate::Handle;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TradeRouteActorAuthority {
    pub actor: Handle,
    pub snapshot: TradeAtomicSnapshot,
}

/// Facts absent from generated World columns. This adapter is deliberately not serialized;
/// load restores canonical orders and the caller must reinstall the same revision/digest-bound
/// observation before `Unit::work` can execute the branch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TradeRouteRuntimeAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub actors: Vec<TradeRouteActorAuthority>,
}

impl TradeRouteRuntimeAuthority {
    fn actor(&self, handle: Handle) -> Option<&TradeRouteActorAuthority> {
        self.actors.iter().find(|entry| entry.actor == handle)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TradeRouteRuntimeError {
    MissingAuthority,
    MissingCompositionDigest,
    DuplicateActorAuthority(Handle),
    Unit(PackageError),
    NotTradeRoute,
    MissingTypedPayload,
    ActorSnapshotMismatch,
    Frontier(TradePlanError),
    UnsupportedBranch(TradeExecutorBranch),
    UnsupportedEffect,
    MissingQueuedSuccessor,
    StaleAuthority,
    StaleUnit,
    StaleRandomState,
}

impl From<PackageError> for TradeRouteRuntimeError {
    fn from(value: PackageError) -> Self {
        Self::Unit(value)
    }
}

impl From<TradePlanError> for TradeRouteRuntimeError {
    fn from(value: TradePlanError) -> Self {
        Self::Frontier(value)
    }
}

#[derive(Clone, Debug)]
pub struct PreparedTradeRouteActivation {
    pub row: usize,
    pub random_state: i32,
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub authority_actor: TradeRouteActorAuthority,
    pub before: UnitImage,
    pub orders_after: OrderList,
    pub frontier: TradeExecutorReceipt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TradeRouteActivationReceipt {
    pub actor: Handle,
    pub frame: i32,
    pub branch: TradeExecutorBranch,
    pub retired_order: TradeOrderState,
    pub next_order: OrderIndex,
    pub random_state_before: i32,
    pub random_state_after: i32,
}

fn order_state(image: &UnitImage) -> Result<TradeOrderState, TradeRouteRuntimeError> {
    let order = image
        .orders
        .current()
        .ok_or(TradeRouteRuntimeError::NotTradeRoute)?;
    if order.kind != OrderIndex::TradeRoute {
        return Err(TradeRouteRuntimeError::NotTradeRoute);
    }
    let Some(EconomyOrderPayload::TradeRoute(payload)) = order.economy else {
        return Err(TradeRouteRuntimeError::MissingTypedPayload);
    };
    Ok(TradeOrderState {
        first: TradeIdentity {
            o: i32::from(order.target_o),
            who: i32::from(order.target_who),
            uid: order.target_uid,
        },
        second: TradeIdentity {
            o: payload.second.o,
            who: payload.second.who,
            uid: payload.second.uid,
        },
        started: payload.started,
        loaded: payload.loaded,
        flags: order.flags,
    })
}

fn snapshot_matches_unit(snapshot: &TradeAtomicSnapshot, image: &UnitImage) -> bool {
    let actor = snapshot.facts.actor;
    actor.identity
        == (TradeIdentity {
            o: i32::from(image.identity.o),
            who: i32::from(image.identity.who),
            uid: image.identity.uid,
        })
        && actor.x == image.x
        && actor.y == image.y
        && actor.flags == image.unit_masks
        && actor.inside == (image.unit_masks & 1 != 0)
        && actor.has_order_after_kill == (image.orders.len() > 1)
        && order_state(image).is_ok_and(|state| state == snapshot.facts.order)
}

fn admitted_refusal_plan(plan: &TradeExecutorPlan) -> bool {
    plan.branch == TradeExecutorBranch::EstablishmentRejected
        && matches!(
            plan.steps.as_slice(),
            [
                TradeHostStep::SetAnimation {
                    animation: 0,
                    arg0: 0,
                    arg1: 1
                },
                TradeHostStep::KillCurrentOrder { arg: 0 }
            ]
        )
}

/// Prepare the one currently executable `Unit::do_trade` branch from canonical World owners.
pub fn prepare_trade_route_activation(
    world: &World,
    paths: &[PathStack],
    authority: &TradeRouteRuntimeAuthority,
    row: usize,
) -> Result<PreparedTradeRouteActivation, TradeRouteRuntimeError> {
    if authority.composition_digest == [0; 32] {
        return Err(TradeRouteRuntimeError::MissingCompositionDigest);
    }
    if let Some(duplicate) = authority
        .actors
        .iter()
        .enumerate()
        .find_map(|(index, entry)| {
            authority.actors[..index]
                .iter()
                .any(|old| old.actor == entry.actor)
                .then_some(entry.actor)
        })
    {
        return Err(TradeRouteRuntimeError::DuplicateActorAuthority(duplicate));
    }
    let handle = world
        .handle_at_row(row)
        .ok_or(TradeRouteRuntimeError::MissingAuthority)?;
    let installed = authority
        .actor(handle)
        .ok_or(TradeRouteRuntimeError::MissingAuthority)?;
    let (resolved_row, before) =
        capture_unit(world, paths, world.units.get_who(row), world.units.o()[row])?;
    if resolved_row != row || !snapshot_matches_unit(&installed.snapshot, &before) {
        return Err(TradeRouteRuntimeError::ActorSnapshotMismatch);
    }
    if before.orders.len() < 2 {
        return Err(TradeRouteRuntimeError::MissingQueuedSuccessor);
    }
    let frontier = TradeExecutorReceipt::preflight(installed.snapshot.clone())?;
    if !admitted_refusal_plan(&frontier.plan) {
        return Err(
            if frontier.plan.branch != TradeExecutorBranch::EstablishmentRejected {
                TradeRouteRuntimeError::UnsupportedBranch(frontier.plan.branch)
            } else {
                TradeRouteRuntimeError::UnsupportedEffect
            },
        );
    }
    let mut orders_after = before.orders.clone();
    let retired = orders_after
        .kill_current()
        .ok_or(TradeRouteRuntimeError::NotTradeRoute)?;
    if retired.kind != OrderIndex::TradeRoute || orders_after.is_empty() {
        return Err(TradeRouteRuntimeError::MissingQueuedSuccessor);
    }
    Ok(PreparedTradeRouteActivation {
        row,
        random_state: world.random.state(),
        authority_revision: authority.revision,
        authority_digest: authority.composition_digest,
        authority_actor: installed.clone(),
        before,
        orders_after,
        frontier,
    })
}

/// Revalidate the complete actor/order/path and external fact snapshots, then retire the rejected
/// route by assignment. Animation is a renderer effect and is intentionally absent from World.
pub fn commit_trade_route_activation(
    world: &mut World,
    paths: &[PathStack],
    authority: &TradeRouteRuntimeAuthority,
    prepared: PreparedTradeRouteActivation,
) -> Result<TradeRouteActivationReceipt, TradeRouteRuntimeError> {
    if authority.revision != prepared.authority_revision
        || authority.composition_digest != prepared.authority_digest
        || authority.actor(prepared.before.identity.handle) != Some(&prepared.authority_actor)
    {
        return Err(TradeRouteRuntimeError::StaleAuthority);
    }
    if world.random.state() != prepared.random_state {
        return Err(TradeRouteRuntimeError::StaleRandomState);
    }
    if !unit_still_current(world, paths, &prepared.before)
        || !snapshot_matches_unit(&prepared.authority_actor.snapshot, &prepared.before)
        || !prepared
            .frontier
            .validates(&prepared.authority_actor.snapshot)
    {
        return Err(TradeRouteRuntimeError::StaleUnit);
    }
    let retired_order = prepared.frontier.plan.order_after;
    let next_order = prepared.orders_after.order_type();
    *world.orders_mut(prepared.row) = prepared.orders_after;
    Ok(TradeRouteActivationReceipt {
        actor: prepared.before.identity.handle,
        frame: world.frame,
        branch: prepared.frontier.plan.branch,
        retired_order,
        next_order,
        random_state_before: prepared.random_state,
        random_state_after: prepared.random_state,
    })
}
