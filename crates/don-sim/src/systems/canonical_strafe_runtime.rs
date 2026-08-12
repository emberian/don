// SPDX-License-Identifier: GPL-3.0-or-later
//! Canonical live-`Sim` adapter for one recovered `Unit::do_strafe` activation.
//!
//! This adapter deliberately admits only the fully owned active-Unit target cone.  Building
//! targets, captain repair, missing-target fallbacks, helicopter projection, and a due target
//! search without an installed revision-bound observation fail before mutation.  The admitted
//! cone still crosses the real canonical order queue, path stack, Unit columns, air-physics
//! planner, animation latch, simulation RNG, and (when requested) the Sim ammo callback.

use std::collections::BTreeMap;

use crate::checksum::adler32;
use crate::order::OrderIndex;
use crate::rng::Random;
use crate::systems::air::AirOrderWalk;
use crate::systems::air_physics_frontier as air_physics;
use crate::systems::groups_guys::{angle_diff, cosx, sinx};
use crate::systems::movement::{PathData, PathStack};
use crate::systems::order_dispatch::{self, OrderQueue, PatrolPayload};
use crate::systems::strafe_executor_transaction as transaction;
use crate::systems::strafe_order_frontier as frontier;
use crate::trig::find_angle;
use crate::world::{World, OBJ_FLAG_ACTIVE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrafeFireMode {
    Hold,
    FireAmmo,
    AttackAnimation,
}

/// Content/type answers that `World`'s generated columns do not carry yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StrafeTypeFacts {
    pub animal: bool,
    pub missile: bool,
    pub helicopter: bool,
    pub bomber: bool,
    pub strafes: bool,
    pub speed: i32,
    pub min_range: i32,
    pub max_range: i32,
    pub fire_mode: StrafeFireMode,
    pub attack_call_al: u8,
    pub bomber_spell_delta: Option<i16>,
}

/// One exact host search observation, bound to actor identity, frame, and search kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StrafeSearchObservation {
    pub actor: frontier::ObjectIdentity,
    pub frame: i32,
    pub kind: frontier::AirTargetSearchKind,
    pub result: Option<frontier::ObjectIdentity>,
}

/// Reinstalled, non-persistent projection over content and finder policy.  Epochs are receipt
/// tokens, not gameplay state; load may reinstall a fresh authority over the loaded canonical
/// World/order/RNG image.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StrafeRuntimeAuthority {
    pub revision: u64,
    pub type_facts: BTreeMap<i32, StrafeTypeFacts>,
    pub searches: Vec<StrafeSearchObservation>,
    pub rng_epoch: u64,
    pub external_effect_epoch: u64,
}

impl StrafeRuntimeAuthority {
    pub fn insert_type(&mut self, type_index: i32, facts: StrafeTypeFacts) {
        self.type_facts.insert(type_index, facts);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrafeRuntimeEffect {
    SetAnimation(i32),
    FireAmmo(frontier::ObjectIdentity),
}

#[derive(Clone, Debug)]
pub struct PreparedCanonicalStrafe {
    row: usize,
    authority_revision: u64,
    transaction: transaction::PreparedStrafeFrame,
    snapshot: frontier::StrafeHostSnapshot,
    before_rng_state: i32,
    rng_state_after_physics: i32,
    expected_rng_state_after_effects: i32,
    effects: Vec<StrafeRuntimeEffect>,
}

impl PreparedCanonicalStrafe {
    pub fn effects(&self) -> &[StrafeRuntimeEffect] {
        &self.effects
    }

    pub fn expected_rng_state_after_effects(&self) -> i32 {
        self.expected_rng_state_after_effects
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalStrafeCommitReceipt {
    pub transaction: transaction::StrafeFrameCommitReceipt,
    pub effects: Vec<StrafeRuntimeEffect>,
    pub rng_state_before: i32,
    pub rng_state_after_physics: i32,
    pub expected_rng_state_after_effects: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalStrafeRuntimeError {
    RowOutOfRange,
    PathOwnerMissing,
    MissingStrafeOrder,
    MalformedStrafeOrder,
    MissingTypeFacts(i32),
    MissingTarget,
    StaleTarget,
    UnsupportedTargetBand,
    MissingSearchObservation,
    StaleSearchObservation,
    UnsupportedCone(&'static str),
    AirPhysics(air_physics::AirPhysicsPlanError),
    Transaction(transaction::StrafeTransactionError),
    StaleAuthority,
    StaleCanonicalState,
}

impl From<air_physics::AirPhysicsPlanError> for CanonicalStrafeRuntimeError {
    fn from(error: air_physics::AirPhysicsPlanError) -> Self {
        Self::AirPhysics(error)
    }
}

impl From<transaction::StrafeTransactionError> for CanonicalStrafeRuntimeError {
    fn from(error: transaction::StrafeTransactionError) -> Self {
        Self::Transaction(error)
    }
}

fn digest(value: &impl std::fmt::Debug) -> u64 {
    u64::from(adler32(1, format!("{value:?}").as_bytes()))
}

fn actor_identity(world: &World, row: usize) -> frontier::ObjectIdentity {
    frontier::ObjectIdentity {
        o: i32::from(world.units.o()[row]),
        who: i32::from(world.units.get_who(row)),
        uid: world.units.get_uid(row),
    }
}

fn canonical_image(
    world: &World,
    paths: &[PathStack],
    authority: &StrafeRuntimeAuthority,
    row: usize,
) -> Result<transaction::CanonicalStrafeImage, CanonicalStrafeRuntimeError> {
    let path = paths
        .get(row)
        .cloned()
        .ok_or(CanonicalStrafeRuntimeError::PathOwnerMissing)?;
    Ok(transaction::CanonicalStrafeImage {
        actor: transaction::CanonicalStrafeActor {
            identity: actor_identity(world, row),
            x: world.units.x_internal()[row],
            y: world.units.y_internal()[row],
            angle: world.units.angle()[row] as u32,
            active: world.units.get_flags(row) & OBJ_FLAG_ACTIVE != 0,
            // Retail STRAFE writes the attack return byte at UnitData+0xAE, the canonical
            // generated `recharging` column.
            attack_latch: world.units.get_recharging(row),
            spell_time: world.units.spell_time()[row],
        },
        orders: order_dispatch::adopt(world.orders(row)),
        path,
        rng_epoch: authority.rng_epoch,
        external_effect_epoch: authority.external_effect_epoch,
    })
}

fn snapshot(
    world: &World,
    image: &transaction::CanonicalStrafeImage,
) -> frontier::StrafeHostSnapshot {
    frontier::StrafeHostSnapshot {
        actor: image.actor.identity,
        actor_version: digest(&image.actor),
        order_version: digest(image.orders.front().expect("validated STRAFE head")),
        target_pool_epoch: world.frame as u32 as u64,
        queue_digest: digest(&image.orders),
        path_digest: u64::from(adler32(1, &image.path.walk_bytes())),
        external_effect_epoch: image.external_effect_epoch,
        rng_epoch: image.rng_epoch,
    }
}

fn current_strafe(
    orders: &OrderQueue,
) -> Result<crate::systems::patrol::StrafeOrder, CanonicalStrafeRuntimeError> {
    let current = orders
        .front()
        .ok_or(CanonicalStrafeRuntimeError::MissingStrafeOrder)?;
    if current.kind != OrderIndex::Strafe {
        return Err(CanonicalStrafeRuntimeError::MissingStrafeOrder);
    }
    let PatrolPayload::Strafe(strafe) = &current.patrol_payload else {
        return Err(CanonicalStrafeRuntimeError::MissingStrafeOrder);
    };
    if (current.target_o, current.target_who, current.target_uid)
        != (strafe.target_o, strafe.target_who, strafe.target_uid)
    {
        return Err(CanonicalStrafeRuntimeError::MalformedStrafeOrder);
    }
    crate::systems::strafe_runtime_authority::validate_strafe_order(strafe)
        .map_err(|_| CanonicalStrafeRuntimeError::MalformedStrafeOrder)?;
    Ok(strafe.clone())
}

fn target_snapshot(
    world: &World,
    identity: frontier::ObjectIdentity,
) -> Result<frontier::TargetSnapshot, CanonicalStrafeRuntimeError> {
    let row = world
        .unit_row_at(identity.who, identity.o)
        .ok_or(CanonicalStrafeRuntimeError::MissingTarget)?;
    if row >= world.live_count() as usize {
        return Err(CanonicalStrafeRuntimeError::MissingTarget);
    }
    if world.units.get_uid(row) != identity.uid {
        return Err(CanonicalStrafeRuntimeError::StaleTarget);
    }
    let active = world.units.get_flags(row) & OBJ_FLAG_ACTIVE != 0;
    if !active {
        return Err(CanonicalStrafeRuntimeError::StaleTarget);
    }
    Ok(frontier::TargetSnapshot {
        identity,
        x: world.units.x_internal()[row],
        y: world.units.y_internal()[row],
        active,
    })
}

fn search_observation(
    world: &World,
    authority: &StrafeRuntimeAuthority,
    actor: frontier::ObjectIdentity,
    frame: i32,
    kind: frontier::AirTargetSearchKind,
) -> Result<frontier::SearchObservation, CanonicalStrafeRuntimeError> {
    let observation = authority
        .searches
        .iter()
        .find(|entry| entry.actor == actor && entry.frame == frame && entry.kind == kind)
        .ok_or(CanonicalStrafeRuntimeError::MissingSearchObservation)?;
    match observation.result {
        None => Ok(frontier::SearchObservation::Miss),
        Some(identity) => target_snapshot(world, identity)
            .map(frontier::SearchObservation::Hit)
            .map_err(|_| CanonicalStrafeRuntimeError::StaleSearchObservation),
    }
}

pub(crate) struct PreparedAirPhysics {
    pub(crate) plan: air_physics::AirPhysicsPlan,
    pub(crate) rng_state_after: i32,
    pub(crate) final_x: i32,
    pub(crate) final_y: i32,
    pub(crate) final_angle: i32,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare_canonical_air_physics(
    transaction_id: u64,
    frame: i32,
    map_tiles: (i32, i32),
    before_rng: Random,
    rng_epoch: u64,
    actor: frontier::ActorSnapshot,
    order: &AirOrderWalk,
    aim: (i32, i32),
    type_index: i32,
    type_facts: StrafeTypeFacts,
    queue_digest: u64,
    path_digest: u64,
    external_effect_epoch: u64,
) -> Result<PreparedAirPhysics, CanonicalStrafeRuntimeError> {
    if type_facts.animal || type_facts.helicopter || type_facts.speed <= 0 {
        return Err(CanonicalStrafeRuntimeError::UnsupportedCone(
            "animal/helicopter/nonmoving air physics",
        ));
    }
    let physics_identity = air_physics::ObjectIdentity {
        who: u8::try_from(actor.identity.who)
            .map_err(|_| CanonicalStrafeRuntimeError::MalformedStrafeOrder)?,
        o: i16::try_from(actor.identity.o)
            .map_err(|_| CanonicalStrafeRuntimeError::MalformedStrafeOrder)?,
        uid: actor.identity.uid,
    };
    let physics_actor = air_physics::ActorState {
        identity: physics_identity,
        x: actor.x,
        y: actor.y,
        angle: actor.angle as i32,
        recharging: actor.attack_latch,
        queue_len: actor.queue_len as i32,
        first_guy_z: 0,
    };
    let physics_order = air_physics::AirOrderState {
        home_o: order.oxx,
        home_who: order.whose,
        cruising_alt: order.cruising_alt,
        sharp_turn: order.sharp_turn,
        old: order.old,
        returning: order.returning,
    };
    let mut rng = before_rng;
    let cruise_draw =
        air_physics::cruise_altitude_due(physics_identity.o, frame).then(|| air_physics::RngDraw {
            lo: 0,
            hi_exclusive: 0xffff,
            value: rng.get(0, 0xffff),
        });
    let desired_angle = find_angle(aim.0.wrapping_sub(actor.x), aim.1.wrapping_sub(actor.y));
    let final_x = actor.x.wrapping_add(sinx(desired_angle, type_facts.speed));
    let final_y = actor.y.wrapping_sub(cosx(desired_angle, type_facts.speed));
    let mutation = |salt: u64| air_physics::MutationReceipt {
        transaction: transaction_id,
        mutation_digest: digest(&(transaction_id, salt)),
    };
    let snapshot = air_physics::AirPhysicsSnapshot {
        transaction: transaction_id,
        actor: physics_identity,
        actor_version: digest(&physics_actor),
        order_version: digest(&physics_order),
        queue_digest,
        path_digest,
        object_pool_epoch: frame as u32 as u64,
        terrain_epoch: 0,
        animation_epoch: external_effect_epoch,
        external_effect_epoch,
        rng_epoch,
    };
    let facts = air_physics::AirPhysicsFacts {
        snapshot,
        frame,
        map_width_tiles: map_tiles.0,
        map_height_tiles: map_tiles.1,
        ai_speed: 1,
        actor_type: type_index,
        actor_is_bomber: air_physics::HostFact::Known(type_facts.bomber),
        actor_is_animal: air_physics::HostFact::Known(false),
        actor_is_helicopter: air_physics::HostFact::Known(false),
        actor_speed: air_physics::HostFact::Known(type_facts.speed),
        actor_min_range: air_physics::HostFact::Known(type_facts.min_range),
        actor: physics_actor,
        order: physics_order,
        aim_x: aim.0,
        aim_y: aim.1,
        cruise_draw,
        fuel: Some(air_physics::FuelReceipt::Continue {
            mutation: mutation(1),
            actor: physics_actor,
            order: physics_order,
            aim_x: aim.0,
            aim_y: aim.1,
            altitude_goal: 0,
        }),
        terrain_altitude: air_physics::HostFact::Missing("unreached terrain altitude"),
        home_is_active: air_physics::HostFact::Missing("unreached home target"),
        current_air_target: air_physics::HostFact::Known(air_physics::CurrentAirTarget::Accepted {
            x: aim.0,
            y: aim.1,
        }),
        bank: air_physics::HostFact::Known(air_physics::BankReceipt {
            mutation: mutation(2),
            actor_angle_after: desired_angle,
            speed_after: type_facts.speed,
        }),
        pitch: air_physics::HostFact::Known(air_physics::PitchReceipt {
            mutation: mutation(3),
            speed_before: type_facts.speed,
            speed_after: type_facts.speed,
        }),
        guy_set_angle: air_physics::HostFact::Known(mutation(4)),
        collision: air_physics::HostFact::Known(air_physics::CollisionReceipt {
            invalid_location: false,
            restricted: None,
            turn_draw: None,
            set_new_location: mutation(5),
            actor_x_after: final_x,
            actor_y_after: final_y,
        }),
        land_plane: air_physics::HostFact::Missing("unreached landing"),
        kill_current_order: air_physics::HostFact::Missing("unreached helicopter queue tail"),
        set_idle_animation: air_physics::HostFact::Known(mutation(6)),
    };
    let plan = air_physics::plan_air_physics(&facts)?;
    if plan.returned != air_physics::AirPhysicsReturn::Continue {
        return Err(CanonicalStrafeRuntimeError::UnsupportedCone(
            "air physics did not continue",
        ));
    }
    Ok(PreparedAirPhysics {
        plan,
        rng_state_after: rng.state(),
        final_x,
        final_y,
        final_angle: desired_angle,
    })
}

fn apply_local_for_receipts(
    image: &mut transaction::CanonicalStrafeImage,
    step: frontier::StrafeStep,
) -> Result<(), CanonicalStrafeRuntimeError> {
    match step {
        frontier::StrafeStep::StoreTargetPosition { x, y } => {
            let current = image
                .orders
                .front_mut()
                .ok_or(CanonicalStrafeRuntimeError::MissingStrafeOrder)?;
            let PatrolPayload::Strafe(strafe) = &mut current.patrol_payload else {
                return Err(CanonicalStrafeRuntimeError::MissingStrafeOrder);
            };
            strafe.xx = x;
            strafe.yy = y;
        }
        frontier::StrafeStep::StoreAttackLatch(value) => image.actor.attack_latch = value,
        frontier::StrafeStep::AddSpellTime(delta) => {
            image.actor.spell_time = image.actor.spell_time.wrapping_add(delta)
        }
        frontier::StrafeStep::Hold => {}
        _ => {
            return Err(CanonicalStrafeRuntimeError::UnsupportedCone(
                "local STRAFE mutation outside active target cone",
            ))
        }
    }
    Ok(())
}

fn apply_physics_image(
    image: &mut transaction::CanonicalStrafeImage,
    physics: &PreparedAirPhysics,
    aim: (i32, i32),
) -> Result<(), CanonicalStrafeRuntimeError> {
    image.path.clear();
    image.path.push(PathData {
        to_x: aim.0,
        to_y: aim.1,
        tolerance: air_physics::PATH_TARGET_TOLERANCE,
        flags: air_physics::PATH_TARGET_FLAGS,
    });
    let current = image
        .orders
        .front_mut()
        .ok_or(CanonicalStrafeRuntimeError::MissingStrafeOrder)?;
    let PatrolPayload::Strafe(strafe) = &mut current.patrol_payload else {
        return Err(CanonicalStrafeRuntimeError::MissingStrafeOrder);
    };
    strafe.air.cruising_alt = physics.plan.final_order.cruising_alt;
    strafe.air.sharp_turn = physics.plan.final_order.sharp_turn;
    strafe.air.old = physics.plan.final_order.old;
    strafe.air.returning = physics.plan.final_order.returning;
    image.actor.x = physics.final_x;
    image.actor.y = physics.final_y;
    image.actor.angle = physics.final_angle as u32;
    image.rng_epoch = physics.plan.rng_epoch_after;
    Ok(())
}

/// Preflight one active-Unit STRAFE activation entirely against detached canonical images.
pub fn prepare_strafe_activation(
    world: &World,
    paths: &[PathStack],
    unit_types: &[i32],
    authority: &StrafeRuntimeAuthority,
    row: usize,
    map_tiles: (i32, i32),
) -> Result<PreparedCanonicalStrafe, CanonicalStrafeRuntimeError> {
    if row >= world.live_count() as usize || row >= unit_types.len() {
        return Err(CanonicalStrafeRuntimeError::RowOutOfRange);
    }
    let before = canonical_image(world, paths, authority, row)?;
    let strafe = current_strafe(&before.orders)?;
    let actor_type = unit_types[row];
    let type_facts = *authority
        .type_facts
        .get(&actor_type)
        .ok_or(CanonicalStrafeRuntimeError::MissingTypeFacts(actor_type))?;
    let actor_identity = before.actor.identity;
    let actor = frontier::ActorSnapshot {
        identity: actor_identity,
        x: before.actor.x,
        y: before.actor.y,
        angle: before.actor.angle,
        active: before.actor.active,
        animal: type_facts.animal,
        missile: type_facts.missile,
        helicopter: type_facts.helicopter,
        bomber: type_facts.bomber,
        strafes: type_facts.strafes,
        queue_len: before.orders.len() as u32,
        attack_latch: before.actor.attack_latch,
        spell_time: before.actor.spell_time,
    };
    let target_identity = frontier::ObjectIdentity {
        o: strafe.target_o,
        who: strafe.target_who,
        uid: strafe.target_uid,
    };
    let target = target_snapshot(world, target_identity)?;
    let search_kind = if type_facts.bomber {
        frontier::AirTargetSearchKind::BomberFirst
    } else {
        frontier::AirTargetSearchKind::AirFirst
    };
    let scan = if frontier::scan_due(actor.identity.o, world.frame) {
        search_observation(world, authority, actor.identity, world.frame, search_kind)?
    } else {
        frontier::SearchObservation::NotDue
    };
    let transaction_id = authority.revision.rotate_left(17)
        ^ (world.frame as u32 as u64).rotate_left(7)
        ^ (actor.identity.o as u32 as u64)
        ^ authority.external_effect_epoch;
    let host_snapshot = snapshot(world, &before);
    let before_rng = world.random;
    let physics = prepare_canonical_air_physics(
        transaction_id,
        world.frame,
        map_tiles,
        before_rng,
        before.rng_epoch,
        actor,
        &strafe.air,
        (target.x, target.y),
        actor_type,
        type_facts,
        host_snapshot.queue_digest,
        host_snapshot.path_digest,
        before.external_effect_epoch,
    )?;
    let distance = crate::systems::combat::vector_dist(
        target.x.wrapping_sub(actor.x),
        target.y.wrapping_sub(actor.y),
    );
    let bearing = find_angle(
        target.x.wrapping_sub(actor.x),
        target.y.wrapping_sub(actor.y),
    ) as u32;
    let aligned = angle_diff(bearing, actor.angle) < frontier::TURN_15_DEGREES;
    let can_fire = aligned && distance <= type_facts.max_range;
    let fire = if !can_fire || type_facts.fire_mode == StrafeFireMode::Hold {
        frontier::FireCone::Hold
    } else {
        let tail = frontier::AttackTail {
            attack_call_al: type_facts.attack_call_al,
            bomber_spell_delta: type_facts.bomber_spell_delta,
        };
        match type_facts.fire_mode {
            StrafeFireMode::Hold => unreachable!(),
            StrafeFireMode::FireAmmo => frontier::FireCone::FireAmmo(tail),
            StrafeFireMode::AttackAnimation => frontier::FireCone::AttackAnimation(tail),
        }
    };
    let reacquire = if actor.queue_len <= 1 {
        frontier::ReacquireCone::NotEligible
    } else if frontier::reacquire_due(actor.identity.o, world.frame) {
        return Err(CanonicalStrafeRuntimeError::MissingSearchObservation);
    } else {
        frontier::ReacquireCone::NotDue
    };
    let physics_digest = digest(&physics.plan);
    let facts = frontier::StrafeFrameFacts {
        frame: world.frame,
        actor,
        order: strafe,
        front: frontier::FrontCone::Active {
            target,
            repaired_from: None,
            aim: frontier::AimCone::Live {
                x: target.x,
                y: target.y,
            },
            search_kind,
            scan,
        },
        physics: Some(frontier::AirPhysicsReceipt {
            completed: true,
            mutation_digest: physics_digest,
            rng_epoch_before: before.rng_epoch,
            rng_epoch_after: physics.plan.rng_epoch_after,
            draws: physics
                .plan
                .rng_draws
                .iter()
                .map(|draw| frontier::RngDraw {
                    lo: draw.lo,
                    hi_exclusive: draw.hi_exclusive,
                    value: draw.value,
                })
                .collect(),
        }),
        post: frontier::PostPhysicsCone::Combat { fire, reacquire },
    };
    let plan = frontier::plan_strafe_frame(&facts)
        .map_err(transaction::StrafeTransactionError::Planner)?;
    let mut state = before.clone();
    let mut receipts = Vec::new();
    let mut effects = Vec::new();
    let mut predicted_rng = Random::new(physics.rng_state_after);
    for step in plan.steps.iter().cloned() {
        match step {
            frontier::StrafeStep::ThinkBird { .. } => {
                let before_step = state.clone();
                state.external_effect_epoch = state.external_effect_epoch.wrapping_add(1);
                receipts.push(transaction::StrafeExternalReceipt {
                    transaction: transaction_id,
                    step,
                    before: before_step,
                    after: state.clone(),
                    air_physics: None,
                });
            }
            frontier::StrafeStep::AirPhysics { x, y, .. } => {
                let before_step = state.clone();
                apply_physics_image(&mut state, &physics, (x, y))?;
                state.external_effect_epoch = state.external_effect_epoch.wrapping_add(1);
                effects.extend(physics.plan.steps.iter().filter_map(|step| match step {
                    air_physics::AirPhysicsStep::SetAnimation { animation, .. } => {
                        Some(StrafeRuntimeEffect::SetAnimation(*animation))
                    }
                    _ => None,
                }));
                receipts.push(transaction::StrafeExternalReceipt {
                    transaction: transaction_id,
                    step,
                    before: before_step,
                    after: state.clone(),
                    air_physics: Some(air_physics::AirPhysicsCommitReceipt::applied(&physics.plan)),
                });
            }
            frontier::StrafeStep::SetAnimation { animation } => {
                let before_step = state.clone();
                state.external_effect_epoch = state.external_effect_epoch.wrapping_add(1);
                effects.push(StrafeRuntimeEffect::SetAnimation(animation));
                receipts.push(transaction::StrafeExternalReceipt {
                    transaction: transaction_id,
                    step,
                    before: before_step,
                    after: state.clone(),
                    air_physics: None,
                });
            }
            frontier::StrafeStep::FireAmmo(target) => {
                let before_step = state.clone();
                // The current Sim ordinary targeted-arc adapter consumes two scatter draws.
                predicted_rng.get(0, 0xffff);
                predicted_rng.get(0, 0xffff);
                state.rng_epoch = state.rng_epoch.wrapping_add(2);
                state.external_effect_epoch = state.external_effect_epoch.wrapping_add(1);
                effects.push(StrafeRuntimeEffect::FireAmmo(target));
                receipts.push(transaction::StrafeExternalReceipt {
                    transaction: transaction_id,
                    step,
                    before: before_step,
                    after: state.clone(),
                    air_physics: None,
                });
            }
            local => apply_local_for_receipts(&mut state, local)?,
        }
    }
    let prepared =
        transaction::prepare_strafe_frame(transaction_id, host_snapshot, facts, before, receipts)?;
    if prepared.after != state {
        return Err(CanonicalStrafeRuntimeError::StaleCanonicalState);
    }
    Ok(PreparedCanonicalStrafe {
        row,
        authority_revision: authority.revision,
        transaction: prepared,
        snapshot: host_snapshot,
        before_rng_state: before_rng.state(),
        rng_state_after_physics: physics.rng_state_after,
        expected_rng_state_after_effects: predicted_rng.state(),
        effects,
    })
}

/// Commit the detached order/path/Unit-column/RNG image. Ammo effects are returned to the Sim
/// caller, which owns the pool and must apply them synchronously before accepting the expected
/// final RNG-state receipt.
pub fn commit_strafe_activation(
    world: &mut World,
    paths: &mut [PathStack],
    unit_types: &[i32],
    authority: &mut StrafeRuntimeAuthority,
    prepared: PreparedCanonicalStrafe,
) -> Result<CanonicalStrafeCommitReceipt, CanonicalStrafeRuntimeError> {
    if authority.revision != prepared.authority_revision {
        return Err(CanonicalStrafeRuntimeError::StaleAuthority);
    }
    if prepared.row >= world.live_count() as usize || prepared.row >= unit_types.len() {
        return Err(CanonicalStrafeRuntimeError::RowOutOfRange);
    }
    let mut current = canonical_image(world, paths, authority, prepared.row)?;
    let current_snapshot = snapshot(world, &current);
    if current_snapshot != prepared.snapshot || world.random.state() != prepared.before_rng_state {
        return Err(CanonicalStrafeRuntimeError::StaleCanonicalState);
    }
    let transaction =
        transaction::commit_strafe_frame(current_snapshot, &mut current, prepared.transaction)?;
    order_dispatch::publish(&current.orders, world.orders_mut(prepared.row));
    paths[prepared.row] = current.path;
    world.units.x_internal_mut()[prepared.row] = current.actor.x;
    world.units.y_internal_mut()[prepared.row] = current.actor.y;
    world.units.angle_mut()[prepared.row] = current.actor.angle as i32;
    world
        .units
        .set_recharging(prepared.row, current.actor.attack_latch);
    world.units.spell_time_mut()[prepared.row] = current.actor.spell_time;
    world.random.reseed(prepared.rng_state_after_physics);
    authority.rng_epoch = current.rng_epoch;
    authority.external_effect_epoch = current.external_effect_epoch;
    Ok(CanonicalStrafeCommitReceipt {
        transaction,
        effects: prepared.effects,
        rng_state_before: prepared.before_rng_state,
        rng_state_after_physics: prepared.rng_state_after_physics,
        expected_rng_state_after_effects: prepared.expected_rng_state_after_effects,
    })
}
