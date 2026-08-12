// SPDX-License-Identifier: GPL-3.0-or-later
//! Atomic canonical adapter for the ordinary-aircraft cone of `Unit::do_air_patrol`.
//!
//! The admitted cone uses the same revisioned type/search/RNG authority and the same
//! instruction-derived air-physics host as canonical STRAFE. Animal flyers, helicopters,
//! stale homes, and a due building search without an authority observation fail before
//! mutation. An accepted Unit search inserts the real typed STRAFE node at queue front.

use crate::checksum::adler32;
use crate::order::OrderIndex;
use crate::systems::air_physics_frontier as air_physics;
use crate::systems::canonical_group_move_host::{build_still_current, BuildSelectionImage};
use crate::systems::canonical_strafe_runtime::{
    self, PreparedAirPhysics, StrafeRuntimeAuthority, StrafeRuntimeEffect, StrafeTypeFacts,
};
use crate::systems::movement::{PathData, PathStack};
use crate::systems::order_dispatch::{self, OrderQueue, PatrolPayload};
use crate::systems::patrol::{self, AirPatrolAction, AirPatrolAfterPhysics, AirPatrolTarget};
use crate::systems::production::{BuildData, BUILDDATA_SIZE};
use crate::systems::sparse_object_bands_authority_frontier::{RetailBand, RetailObjectAddress};
use crate::systems::strafe_order_frontier::{ActorSnapshot, AirTargetSearchKind, ObjectIdentity};
use crate::world::{World, WorldObjectIdentity, OBJ_FLAG_ACTIVE};

#[derive(Clone, Debug, PartialEq, Eq)]
struct AirPatrolImage {
    orders: OrderQueue,
    path: PathStack,
    x: i32,
    y: i32,
    angle: i32,
    spell_time: i16,
    orders_x: i32,
    orders_y: i32,
    dest_angle: i32,
    rng_epoch: u64,
    external_effect_epoch: u64,
}

#[derive(Clone, Debug)]
pub struct PreparedCanonicalAirPatrol {
    row: usize,
    actor_type: i32,
    world_digest_before: u64,
    authority_before: StrafeRuntimeAuthority,
    build_home_before: Option<BuildSelectionImage>,
    image_before: AirPatrolImage,
    image_after: AirPatrolImage,
    rng_state_before: i32,
    rng_state_after: i32,
    effects: Vec<StrafeRuntimeEffect>,
    inserted_strafe: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalAirPatrolCommitReceipt {
    pub row: usize,
    pub rng_epoch_before: u64,
    pub rng_epoch_after: u64,
    pub rng_state_before: i32,
    pub rng_state_after: i32,
    pub effects: Vec<StrafeRuntimeEffect>,
    pub inserted_strafe: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalAirPatrolRuntimeError {
    RowOutOfRange,
    PathOwnerMissing,
    MissingAirPatrolOrder,
    MalformedAirPatrolOrder,
    MissingTypeFacts(i32),
    UnsupportedCone(&'static str),
    MissingHome,
    StaleHome,
    MissingSearchObservation,
    StaleSearchObservation,
    MissingBuildingSearchObservation,
    AirPhysics(canonical_strafe_runtime::CanonicalStrafeRuntimeError),
    StaleAuthority,
    StaleCanonicalState,
}

fn digest(value: &impl std::fmt::Debug) -> u64 {
    u64::from(adler32(1, format!("{value:?}").as_bytes()))
}

fn identity(world: &World, row: usize) -> ObjectIdentity {
    ObjectIdentity {
        o: i32::from(world.units.o()[row]),
        who: i32::from(world.units.get_who(row)),
        uid: world.units.get_uid(row),
    }
}

fn image(
    world: &World,
    paths: &[PathStack],
    authority: &StrafeRuntimeAuthority,
    row: usize,
) -> Result<AirPatrolImage, CanonicalAirPatrolRuntimeError> {
    Ok(AirPatrolImage {
        orders: order_dispatch::adopt(world.orders(row)),
        path: paths
            .get(row)
            .cloned()
            .ok_or(CanonicalAirPatrolRuntimeError::PathOwnerMissing)?,
        x: world.units.x_internal()[row],
        y: world.units.y_internal()[row],
        angle: world.units.angle()[row],
        spell_time: world.units.spell_time()[row],
        orders_x: world.units.orders_x()[row],
        orders_y: world.units.orders_y()[row],
        dest_angle: world.units.dest_angle()[row],
        rng_epoch: authority.rng_epoch,
        external_effect_epoch: authority.external_effect_epoch,
    })
}

fn air_patrol(
    orders: &OrderQueue,
) -> Result<patrol::AirPatrolOrder, CanonicalAirPatrolRuntimeError> {
    let current = orders
        .front()
        .ok_or(CanonicalAirPatrolRuntimeError::MissingAirPatrolOrder)?;
    if current.kind != OrderIndex::AirPatrol {
        return Err(CanonicalAirPatrolRuntimeError::MissingAirPatrolOrder);
    }
    let PatrolPayload::Air(order) = &current.patrol_payload else {
        return Err(CanonicalAirPatrolRuntimeError::MalformedAirPatrolOrder);
    };
    order
        .points
        .validate()
        .map_err(|_| CanonicalAirPatrolRuntimeError::MalformedAirPatrolOrder)?;
    Ok(order.clone())
}

fn live_unit(
    world: &World,
    target: ObjectIdentity,
) -> Result<usize, CanonicalAirPatrolRuntimeError> {
    let row = world
        .unit_row_at(target.who, target.o)
        .ok_or(CanonicalAirPatrolRuntimeError::StaleSearchObservation)?;
    if row >= world.live_count() as usize
        || world.units.get_uid(row) != target.uid
        || world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0
    {
        return Err(CanonicalAirPatrolRuntimeError::StaleSearchObservation);
    }
    Ok(row)
}

fn build_home_image(
    world: &World,
    builds: &[BuildData],
    who: u8,
    o: i16,
) -> Result<BuildSelectionImage, CanonicalAirPatrolRuntimeError> {
    let address = RetailObjectAddress::new(who, RetailBand::Build, i32::from(o));
    let WorldObjectIdentity::BuildRow(row) = world
        .object_bands()
        .live_identity(address)
        .ok_or(CanonicalAirPatrolRuntimeError::MissingHome)?
    else {
        return Err(CanonicalAirPatrolRuntimeError::MissingHome);
    };
    let build = builds
        .get(row as usize)
        .ok_or(CanonicalAirPatrolRuntimeError::MissingHome)?;
    if build.who != who || build.object_id() != o || !build.is_valid() {
        return Err(CanonicalAirPatrolRuntimeError::StaleHome);
    }
    let bytes: [u8; BUILDDATA_SIZE] = build.image();
    Ok(BuildSelectionImage {
        identity: crate::systems::canonical_group_move_host::BuildSelectionIdentity {
            row,
            who,
            o,
            uid: build.uid,
        },
        bytes,
        position: build.position(),
        inside_down: i16::from_le_bytes([build.other[0x28], build.other[0x29]]),
        inside_down_who: build.other[0x3e] as i8,
    })
}

fn home_position(
    world: &World,
    builds: &[BuildData],
    order: &patrol::AirPatrolOrder,
) -> Result<(Option<(i32, i32)>, Option<BuildSelectionImage>), CanonicalAirPatrolRuntimeError> {
    match (order.air.whose >= 0, order.air.oxx >= 0) {
        (false, false) => Ok((None, None)),
        (true, true) => {
            if let Some(row) = world.unit_row_at(order.air.whose, order.air.oxx) {
                if world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
                    return Err(CanonicalAirPatrolRuntimeError::StaleHome);
                }
                return Ok((
                    Some((world.units.x_internal()[row], world.units.y_internal()[row])),
                    None,
                ));
            }
            let who = u8::try_from(order.air.whose)
                .map_err(|_| CanonicalAirPatrolRuntimeError::MissingHome)?;
            let o = i16::try_from(order.air.oxx)
                .map_err(|_| CanonicalAirPatrolRuntimeError::MissingHome)?;
            let build = build_home_image(world, builds, who, o)?;
            Ok((Some(build.position), Some(build)))
        }
        _ => Err(CanonicalAirPatrolRuntimeError::MalformedAirPatrolOrder),
    }
}

fn unit_search_target(
    world: &World,
    authority: &StrafeRuntimeAuthority,
    actor: ObjectIdentity,
    kind: AirTargetSearchKind,
) -> Result<Option<AirPatrolTarget>, CanonicalAirPatrolRuntimeError> {
    let observation = authority
        .searches
        .iter()
        .find(|entry| entry.actor == actor && entry.frame == world.frame && entry.kind == kind)
        .ok_or(CanonicalAirPatrolRuntimeError::MissingSearchObservation)?;
    let Some(target) = observation.result else {
        return Ok(None);
    };
    let row = live_unit(world, target)?;
    Ok(Some(AirPatrolTarget {
        o: target.o,
        who: target.who,
        uid: target.uid,
        x: world.units.x_internal()[row],
        y: world.units.y_internal()[row],
        // Both exact callbacks represented by this authority are air-target finders.
        domain: 2,
        ever_seen_by_actor: true,
    }))
}

fn store_air_patrol(
    image: &mut AirPatrolImage,
    order: patrol::AirPatrolOrder,
) -> Result<(), CanonicalAirPatrolRuntimeError> {
    let current = image
        .orders
        .front_mut()
        .ok_or(CanonicalAirPatrolRuntimeError::MissingAirPatrolOrder)?;
    if current.kind != OrderIndex::AirPatrol {
        return Err(CanonicalAirPatrolRuntimeError::MissingAirPatrolOrder);
    }
    current.patrol_payload = PatrolPayload::Air(order);
    Ok(())
}

fn update_action(image: &mut AirPatrolImage, actor: ObjectIdentity) {
    let mut unit = order_dispatch::UnitWork::at(actor.who as u8, actor.o as i16, image.x, image.y);
    unit.body.angle = image.angle;
    unit.orders = image.orders.clone();
    order_dispatch::update_action(&mut unit);
    image.orders_x = unit.orders_x;
    image.orders_y = unit.orders_y;
    image.dest_angle = unit.dest_angle;
}

/// Preflight one ordinary AIR_PATROL activation against detached order/path/Unit/RNG images.
pub fn prepare_air_patrol_activation(
    world: &World,
    builds: &[BuildData],
    paths: &[PathStack],
    unit_types: &[i32],
    authority: &StrafeRuntimeAuthority,
    row: usize,
    map_tiles: (i32, i32),
) -> Result<PreparedCanonicalAirPatrol, CanonicalAirPatrolRuntimeError> {
    if row >= world.live_count() as usize || row >= unit_types.len() {
        return Err(CanonicalAirPatrolRuntimeError::RowOutOfRange);
    }
    let before = image(world, paths, authority, row)?;
    let mut order = air_patrol(&before.orders)?;
    let actor_type = unit_types[row];
    let type_facts: StrafeTypeFacts = *authority
        .type_facts
        .get(&actor_type)
        .ok_or(CanonicalAirPatrolRuntimeError::MissingTypeFacts(actor_type))?;
    if type_facts.animal || type_facts.helicopter || type_facts.missile {
        return Err(CanonicalAirPatrolRuntimeError::UnsupportedCone(
            "animal/helicopter/missile AIR_PATROL",
        ));
    }
    let actor_identity = identity(world, row);
    let actor = ActorSnapshot {
        identity: actor_identity,
        x: before.x,
        y: before.y,
        angle: before.angle as u32,
        active: world.units.get_flags(row) & OBJ_FLAG_ACTIVE != 0,
        animal: false,
        missile: false,
        helicopter: false,
        bomber: type_facts.bomber,
        strafes: type_facts.strafes,
        queue_len: before.orders.len() as u32,
        attack_latch: world.units.get_recharging(row),
        spell_time: before.spell_time,
    };
    let (home, build_home_before) = home_position(world, builds, &order)?;
    let max_x = map_tiles.0.saturating_mul(crate::systems::movement::TILE);
    let max_y = map_tiles.1.saturating_mul(crate::systems::movement::TILE);
    let flight_target = patrol::air_patrol_target(&mut order, false, home, max_x, max_y);
    let transaction_id = authority.revision.rotate_left(23)
        ^ (world.frame as u32 as u64).rotate_left(11)
        ^ (actor_identity.o as u32 as u64)
        ^ authority.external_effect_epoch;
    let before_rng = world.random;
    let physics: PreparedAirPhysics = canonical_strafe_runtime::prepare_canonical_air_physics(
        transaction_id,
        world.frame,
        map_tiles,
        before_rng,
        before.rng_epoch,
        actor,
        &order.air,
        flight_target,
        actor_type,
        type_facts,
        digest(&before.orders),
        u64::from(adler32(1, &before.path.walk_bytes())),
        before.external_effect_epoch,
    )
    .map_err(CanonicalAirPatrolRuntimeError::AirPhysics)?;

    let mut after = before.clone();
    after.path.clear();
    after.path.push(PathData {
        to_x: flight_target.0,
        to_y: flight_target.1,
        tolerance: air_physics::PATH_TARGET_TOLERANCE,
        flags: air_physics::PATH_TARGET_FLAGS,
    });
    order.air.cruising_alt = physics.plan.final_order.cruising_alt;
    order.air.sharp_turn = physics.plan.final_order.sharp_turn;
    order.air.old = physics.plan.final_order.old;
    order.air.returning = physics.plan.final_order.returning;
    after.x = physics.final_x;
    after.y = physics.final_y;
    after.angle = physics.final_angle;
    after.rng_epoch = physics.plan.rng_epoch_after;
    after.external_effect_epoch = after.external_effect_epoch.wrapping_add(1);
    let mut effects: Vec<StrafeRuntimeEffect> = physics
        .plan
        .steps
        .iter()
        .filter_map(|step| match step {
            air_physics::AirPhysicsStep::SetAnimation { animation, .. } => {
                Some(StrafeRuntimeEffect::SetAnimation(*animation))
            }
            _ => None,
        })
        .collect();

    let mut input = AirPatrolAfterPhysics {
        actor_x: after.x,
        actor_y: after.y,
        actor_o: actor_identity.o as i16,
        frame: world.frame,
        is_animal: false,
        spell_time: after.spell_time,
        order_list_len: after.orders.len(),
        unit_target: None,
        building_target: None,
    };
    let waypoint_action =
        patrol::advance_air_patrol_waypoint_after_physics(&mut order, flight_target, &input);
    if waypoint_action == AirPatrolAction::KillCurrent {
        return Err(CanonicalAirPatrolRuntimeError::UnsupportedCone(
            "AIR_PATROL waypoint retirement",
        ));
    }
    let phase = actor_identity.o.wrapping_add(world.frame);
    if order.air.returning == 0 && phase % 16 == 0 {
        let kind = if type_facts.bomber {
            AirTargetSearchKind::BomberFirst
        } else {
            AirTargetSearchKind::AirFirst
        };
        input.unit_target = unit_search_target(world, authority, actor_identity, kind)?;
    }
    let action = patrol::step_air_patrol_after_unit_search(&order, &input);
    if action == AirPatrolAction::Continue && phase % 32 == 0 {
        return Err(CanonicalAirPatrolRuntimeError::MissingBuildingSearchObservation);
    }
    let mut inserted_strafe = false;
    match action {
        AirPatrolAction::InsertStrafe { target, mandatory } => {
            let home = (order.air.oxx, order.air.whose);
            store_air_patrol(&mut after, order)?;
            after.orders.push_front(order_dispatch::OrderRec::strafe(
                patrol::patrol_strafe_order(target, home.0, home.1, mandatory),
            ));
            after.path.clear();
            update_action(&mut after, actor_identity);
            inserted_strafe = true;
        }
        AirPatrolAction::Continue => store_air_patrol(&mut after, order)?,
        AirPatrolAction::KillCurrent | AirPatrolAction::PrimeAnimalSpellTime => {
            return Err(CanonicalAirPatrolRuntimeError::UnsupportedCone(
                "AIR_PATROL post-physics branch",
            ));
        }
    }
    effects.shrink_to_fit();
    Ok(PreparedCanonicalAirPatrol {
        row,
        actor_type,
        world_digest_before: world.digest(),
        authority_before: authority.clone(),
        build_home_before,
        image_before: before,
        image_after: after,
        rng_state_before: before_rng.state(),
        rng_state_after: physics.rng_state_after,
        effects,
        inserted_strafe,
    })
}

/// Revalidate and publish one detached AIR_PATROL after-image.
pub fn commit_air_patrol_activation(
    world: &mut World,
    builds: &[BuildData],
    paths: &mut [PathStack],
    unit_types: &[i32],
    authority: &mut StrafeRuntimeAuthority,
    prepared: PreparedCanonicalAirPatrol,
) -> Result<CanonicalAirPatrolCommitReceipt, CanonicalAirPatrolRuntimeError> {
    if authority != &prepared.authority_before {
        return Err(CanonicalAirPatrolRuntimeError::StaleAuthority);
    }
    if prepared.row >= world.live_count() as usize || prepared.row >= unit_types.len() {
        return Err(CanonicalAirPatrolRuntimeError::RowOutOfRange);
    }
    if unit_types[prepared.row] != prepared.actor_type
        || world.digest() != prepared.world_digest_before
    {
        return Err(CanonicalAirPatrolRuntimeError::StaleCanonicalState);
    }
    if prepared
        .build_home_before
        .as_ref()
        .is_some_and(|home| !build_still_current(world, builds, home))
    {
        return Err(CanonicalAirPatrolRuntimeError::StaleHome);
    }
    let current = image(world, paths, authority, prepared.row)?;
    if current != prepared.image_before || world.random.state() != prepared.rng_state_before {
        return Err(CanonicalAirPatrolRuntimeError::StaleCanonicalState);
    }
    let rng_epoch_before = current.rng_epoch;
    let after = prepared.image_after;
    order_dispatch::publish(&after.orders, world.orders_mut(prepared.row));
    paths[prepared.row] = after.path;
    world.units.x_internal_mut()[prepared.row] = after.x;
    world.units.y_internal_mut()[prepared.row] = after.y;
    world.units.angle_mut()[prepared.row] = after.angle;
    world.units.spell_time_mut()[prepared.row] = after.spell_time;
    world.units.orders_x_mut()[prepared.row] = after.orders_x;
    world.units.orders_y_mut()[prepared.row] = after.orders_y;
    world.units.dest_angle_mut()[prepared.row] = after.dest_angle;
    world.random.reseed(prepared.rng_state_after);
    authority.rng_epoch = after.rng_epoch;
    authority.external_effect_epoch = after.external_effect_epoch;
    Ok(CanonicalAirPatrolCommitReceipt {
        row: prepared.row,
        rng_epoch_before,
        rng_epoch_after: after.rng_epoch,
        rng_state_before: prepared.rng_state_before,
        rng_state_after: prepared.rng_state_after,
        effects: prepared.effects,
        inserted_strafe: prepared.inserted_strafe,
    })
}
