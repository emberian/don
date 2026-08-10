// SPDX-License-Identifier: GPL-3.0-or-later
//! Transaction frontier for `Unit::do_air_physics`.
//!
//! The registered module freezes the complete top-level control flow at
//! `0x005E86D0..0x005E8DD2`, while leaving the large nested mutation helpers as mandatory,
//! transaction-bound receipts. In particular, registration is not a substitute for
//! `check_fuel`, `bank_aircraft`, `pitch_aircraft`, collision, animation, or landing.

use crate::systems::groups_guys::{angle_diff, cosx, sinx, vector_dist};
use crate::trig::find_angle;

pub const UNIT_DO_AIR_PHYSICS_VA: u32 = 0x005e_86d0;
pub const UNIT_DO_AIR_PHYSICS_BYTES: usize = 1_794;
pub const UNIT_DO_AIR_PHYSICS_END: u32 = UNIT_DO_AIR_PHYSICS_VA + UNIT_DO_AIR_PHYSICS_BYTES as u32;

pub const BOMBER_TYPE: i32 = 0x130;
pub const BIRD_HIGH_TYPE: i32 = 0x192;
pub const BIRD_TERRAIN_TYPE: i32 = 0x193;
pub const UNIT_FLAG_HELICOPTER: u32 = 0x20;
pub const AIR_DOMAIN: i32 = 2;
pub const PATH_MARK_HOME_MASK: u32 = 0x0010_0000;
pub const CRUISE_BASE: i32 = 0x640;
pub const PATH_TARGET_TOLERANCE: i32 = 0;
pub const PATH_TARGET_FLAGS: i32 = 0;
pub const IDLE_ANIMATION: i32 = 8;
pub const COLLISION_DISTANCE: i32 = 0x300;
pub const HOME_MARK_DISTANCE: i32 = 0x0c00;
pub const HELICOPTER_OUTBOUND_DISTANCE: i32 = 0x00c0;
pub const HELICOPTER_RETURN_DISTANCE: i32 = 0x0030;
pub const LAND_ALTITUDE_DELTA: i32 = 0x0096;
pub const TURN_45_DEGREES: u32 = 0x2000_0000;
pub const TURN_90_DEGREES: i32 = 0x4000_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectIdentity {
    pub who: u8,
    pub o: i16,
    pub uid: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AirOrderState {
    pub home_o: i32,
    pub home_who: i32,
    pub cruising_alt: i32,
    pub sharp_turn: i32,
    pub old: i32,
    pub returning: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorState {
    pub identity: ObjectIdentity,
    pub x: i32,
    pub y: i32,
    pub angle: i32,
    pub recharging: u8,
    pub queue_len: i32,
    /// `GuyData::z` from the first pointer in `UnitData::guys`.
    pub first_guy_z: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AirPhysicsSnapshot {
    pub transaction: u64,
    pub actor: ObjectIdentity,
    pub actor_version: u64,
    pub order_version: u64,
    pub queue_digest: u64,
    pub path_digest: u64,
    pub object_pool_epoch: u64,
    pub terrain_epoch: u64,
    pub animation_epoch: u64,
    pub external_effect_epoch: u64,
    pub rng_epoch: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostFact<T> {
    Known(T),
    Missing(&'static str),
}

impl<T: Copy> HostFact<T> {
    fn require(&self) -> Result<T, AirPhysicsPlanError> {
        match self {
            Self::Known(value) => Ok(*value),
            Self::Missing(label) => Err(AirPhysicsPlanError::MissingHostFact(label)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MutationReceipt {
    /// Every nested result must belong to the same single-use transaction as the snapshot.
    pub transaction: u64,
    /// Host-defined digest over all state/effects changed by the nested helper.
    pub mutation_digest: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RngDraw {
    pub lo: i32,
    pub hi_exclusive: i32,
    pub value: i32,
}

impl RngDraw {
    fn valid(self) -> bool {
        self.lo == 0
            && self.hi_exclusive == 0xffff
            && (self.lo..self.hi_exclusive).contains(&self.value)
    }
}

/// `check_fuel` is itself 1,917 bytes and may change the home, returning state, aim point,
/// altitude goal, and external state.  The caller must observe its complete post-call image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FuelReceipt {
    Stopped {
        mutation: MutationReceipt,
        /// Complete post-call `AirOrder` image; the helper may mutate it before returning one.
        order: AirOrderState,
    },
    Continue {
        mutation: MutationReceipt,
        actor: ActorState,
        order: AirOrderState,
        aim_x: i32,
        aim_y: i32,
        altitude_goal: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurrentAirTarget {
    /// `UnitOrder::is_air()` returned zero.
    NotAirOrder,
    /// The order is air, but its target is inactive, outside AIR domain, or rejected by
    /// `UnitData::valid_target(o, who, 0)`.
    Rejected,
    Accepted {
        x: i32,
        y: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BankReceipt {
    pub mutation: MutationReceipt,
    /// `UnitData::angle` as observed after `bank_aircraft`.
    pub actor_angle_after: i32,
    /// The by-reference speed after `bank_aircraft`.
    pub speed_after: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PitchReceipt {
    pub mutation: MutationReceipt,
    /// The by-reference value observed on entry; must equal `bank_aircraft`'s output.
    pub speed_before: i32,
    /// The by-reference speed after `pitch_aircraft`.
    pub speed_after: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CollisionReceipt {
    pub invalid_location: bool,
    /// Required only when `invalid_location` is true; exact output of `WorldData::restrict`.
    pub restricted: Option<(i32, i32)>,
    /// Required exactly when the invalid-location arm sees `AirOrder::sharp_turn == 0`.
    pub turn_draw: Option<RngDraw>,
    pub set_new_location: MutationReceipt,
    /// Position observed after `Unit::set_new_location`; its integer return is ignored by
    /// the shipped caller, but its mutation is not.
    pub actor_x_after: i32,
    pub actor_y_after: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AirPhysicsFacts {
    pub snapshot: AirPhysicsSnapshot,
    pub frame: i32,
    pub map_width_tiles: i32,
    pub map_height_tiles: i32,
    pub ai_speed: i32,
    pub actor_type: i32,
    /// Virtual `ObjectData::is(BOMBER_TYPE, 0)`. Required only on the modulo-eight arm.
    pub actor_is_bomber: HostFact<bool>,
    /// Virtual `SubObjectData::is_animal()`.
    pub actor_is_animal: HostFact<bool>,
    /// `UnitTypeData::unit_flags & UNIT_FLAG_HELICOPTER`.
    pub actor_is_helicopter: HostFact<bool>,
    /// Virtual `UnitData::get_speed(actor.x, actor.y, 1)` before `ai_speed` scaling.
    pub actor_speed: HostFact<i32>,
    /// Virtual `ObjectData::min_range()`, needed only on reached non-returning turn/target arms.
    pub actor_min_range: HostFact<i32>,
    pub actor: ActorState,
    pub order: AirOrderState,
    pub aim_x: i32,
    pub aim_y: i32,
    pub cruise_draw: Option<RngDraw>,
    pub fuel: Option<FuelReceipt>,
    /// `TerrainOut::find_tcoord_z` at the actor's current cell. Required only for type 0x193
    /// with exactly one queued order.
    pub terrain_altitude: HostFact<i32>,
    /// Active state of `objects[home_who][home_o]`; required only for a returning order whose
    /// non-negative home pair reaches the marker arm.
    pub home_is_active: HostFact<bool>,
    /// Current-order target cone, required only for non-returning, non-helicopter aircraft.
    pub current_air_target: HostFact<CurrentAirTarget>,
    pub bank: HostFact<BankReceipt>,
    pub pitch: HostFact<PitchReceipt>,
    pub guy_set_angle: HostFact<MutationReceipt>,
    pub collision: HostFact<CollisionReceipt>,
    pub land_plane: HostFact<MutationReceipt>,
    pub kill_current_order: HostFact<MutationReceipt>,
    pub set_idle_animation: HostFact<MutationReceipt>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirPhysicsReturn {
    Continue,
    Stop,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AirPhysicsStep {
    DrawCruisingAltitude(RngDraw),
    StoreCruisingAltitude(i32),
    ClearPath,
    CheckFuel {
        mutation_digest: u64,
    },
    StoreRecharge(u8),
    StoreReturning(i32),
    ReadTerrainAltitude(i32),
    ClampAim {
        x: i32,
        y: i32,
    },
    LandPlane {
        mutation_digest: u64,
    },
    OrHomeUnitMasks {
        home_o: i32,
        home_who: i32,
        mask: u32,
    },
    PushPath {
        x: i32,
        y: i32,
        tolerance: i32,
        flags: i32,
    },
    BankAircraft {
        desired_angle: i32,
        aligned_with_air_target: bool,
        speed_in: i32,
        mutation_digest: u64,
    },
    PitchAircraft {
        dx: i32,
        dy: i32,
        altitude_goal: i32,
        speed_in: i32,
        mutation_digest: u64,
    },
    GuySetAngle {
        angle: i32,
        immediate: i32,
        mutation_digest: u64,
    },
    ProjectLocation {
        x: i32,
        y: i32,
    },
    TestInvalidLocation {
        invalid: bool,
    },
    RestrictLocation {
        x: i32,
        y: i32,
    },
    DrawCollisionTurn(RngDraw),
    StoreSharpTurn(i32),
    SetNewLocation {
        x: i32,
        y: i32,
        mutation_digest: u64,
    },
    SetAnimation {
        animation: i32,
        b: i32,
        c: i32,
        mutation_digest: u64,
    },
    KillCurrentOrder {
        mutation_digest: u64,
    },
    Return(AirPhysicsReturn),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AirPhysicsPlan {
    pub snapshot: AirPhysicsSnapshot,
    pub steps: Vec<AirPhysicsStep>,
    pub final_order: AirOrderState,
    pub rng_draws: Vec<RngDraw>,
    pub rng_epoch_after: u64,
    pub returned: AirPhysicsReturn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirPhysicsPlanError {
    MissingHostFact(&'static str),
    InvalidActorIdentity,
    InvalidMapExtent,
    InvalidMutationTransaction,
    MissingRngDraw(&'static str),
    UnexpectedRngDraw(&'static str),
    InvalidRngDraw,
    FuelReceiptOnAnimal,
    MissingFuelReceipt,
    InvalidFuelActorIdentity,
    InvalidHelperChain,
    CollisionReceiptMismatch,
}

fn checked_mutation(
    receipt: MutationReceipt,
    transaction: u64,
) -> Result<u64, AirPhysicsPlanError> {
    if receipt.transaction != transaction {
        return Err(AirPhysicsPlanError::InvalidMutationTransaction);
    }
    Ok(receipt.mutation_digest)
}

fn consume_draw(
    draw: Option<RngDraw>,
    label: &'static str,
    steps: &mut Vec<AirPhysicsStep>,
    draws: &mut Vec<RngDraw>,
    step: fn(RngDraw) -> AirPhysicsStep,
) -> Result<RngDraw, AirPhysicsPlanError> {
    let draw = draw.ok_or(AirPhysicsPlanError::MissingRngDraw(label))?;
    if !draw.valid() {
        return Err(AirPhysicsPlanError::InvalidRngDraw);
    }
    steps.push(step(draw));
    draws.push(draw);
    Ok(draw)
}

#[inline]
pub fn cruise_altitude_due(actor_o: i16, frame: i32) -> bool {
    i32::from(actor_o).wrapping_add(frame) % 8 == 0
}

#[inline]
fn manhattan(dx: i32, dy: i32) -> i32 {
    dx.wrapping_abs().wrapping_add(dy.wrapping_abs())
}

#[inline]
fn project(x: i32, y: i32, angle: i32, distance: i32) -> (i32, i32) {
    (
        x.wrapping_add(sinx(angle, distance)),
        y.wrapping_sub(cosx(angle, distance)),
    )
}

fn finish(
    snapshot: AirPhysicsSnapshot,
    steps: Vec<AirPhysicsStep>,
    order: AirOrderState,
    draws: Vec<RngDraw>,
    returned: AirPhysicsReturn,
) -> AirPhysicsPlan {
    AirPhysicsPlan {
        snapshot,
        rng_epoch_after: snapshot.rng_epoch.wrapping_add(draws.len() as u64),
        steps,
        final_order: order,
        rng_draws: draws,
        returned,
    }
}

/// Plan the shipped top-level transaction.  Every reached nested mutation is mandatory and
/// bound to `snapshot.transaction`; missing facts fail before a plan can authorize publication.
pub fn plan_air_physics(facts: &AirPhysicsFacts) -> Result<AirPhysicsPlan, AirPhysicsPlanError> {
    if facts.snapshot.actor != facts.actor.identity {
        return Err(AirPhysicsPlanError::InvalidActorIdentity);
    }
    if facts.map_width_tiles <= 0 || facts.map_height_tiles <= 0 {
        return Err(AirPhysicsPlanError::InvalidMapExtent);
    }

    let transaction = facts.snapshot.transaction;
    let animal = facts.actor_is_animal.require()?;
    let helicopter = facts.actor_is_helicopter.require()?;
    let mut actor = facts.actor;
    let mut order = facts.order;
    let mut aim_x = facts.aim_x;
    let mut aim_y = facts.aim_y;
    let mut altitude_goal = 0;
    let mut steps = Vec::new();
    let mut draws = Vec::new();

    if cruise_altitude_due(actor.identity.o, facts.frame) {
        let bomber = facts.actor_is_bomber.require()?;
        if !bomber && (!animal || facts.actor_type == BIRD_TERRAIN_TYPE) && !helicopter {
            let draw = consume_draw(
                facts.cruise_draw,
                "cruising-altitude draw",
                &mut steps,
                &mut draws,
                AirPhysicsStep::DrawCruisingAltitude,
            )?;
            order.cruising_alt = (draw.value % 7 + 0x0d).wrapping_mul(100);
        } else {
            if facts.cruise_draw.is_some() {
                return Err(AirPhysicsPlanError::UnexpectedRngDraw(
                    "cruising-altitude draw",
                ));
            }
            order.cruising_alt = CRUISE_BASE;
            if animal {
                order.cruising_alt = if facts.actor_type == BIRD_HIGH_TYPE {
                    order.cruising_alt.wrapping_add(200)
                } else {
                    order.cruising_alt.wrapping_sub(200)
                };
            }
        }
        steps.push(AirPhysicsStep::StoreCruisingAltitude(order.cruising_alt));
    } else if facts.cruise_draw.is_some() {
        return Err(AirPhysicsPlanError::UnexpectedRngDraw(
            "cruising-altitude draw",
        ));
    }

    // `UnitData::path.length = 0` precedes fuel, including fuel's early-stop arm.
    steps.push(AirPhysicsStep::ClearPath);
    if animal {
        if facts.fuel.is_some() {
            return Err(AirPhysicsPlanError::FuelReceiptOnAnimal);
        }
    } else {
        let fuel = facts.fuel.ok_or(AirPhysicsPlanError::MissingFuelReceipt)?;
        match fuel {
            FuelReceipt::Stopped {
                mutation,
                order: post_order,
            } => {
                let mutation_digest = checked_mutation(mutation, transaction)?;
                steps.push(AirPhysicsStep::CheckFuel { mutation_digest });
                steps.push(AirPhysicsStep::Return(AirPhysicsReturn::Stop));
                return Ok(finish(
                    facts.snapshot,
                    steps,
                    post_order,
                    draws,
                    AirPhysicsReturn::Stop,
                ));
            }
            FuelReceipt::Continue {
                mutation,
                actor: post_actor,
                order: post_order,
                aim_x: post_x,
                aim_y: post_y,
                altitude_goal: post_altitude,
            } => {
                let mutation_digest = checked_mutation(mutation, transaction)?;
                if post_actor.identity != actor.identity {
                    return Err(AirPhysicsPlanError::InvalidFuelActorIdentity);
                }
                steps.push(AirPhysicsStep::CheckFuel { mutation_digest });
                actor = post_actor;
                order = post_order;
                aim_x = post_x;
                aim_y = post_y;
                altitude_goal = post_altitude;
            }
        }
    }

    if facts.actor_type == BIRD_TERRAIN_TYPE {
        actor.recharging = 0;
        steps.push(AirPhysicsStep::StoreRecharge(0));
        if actor.queue_len == 1 {
            order.returning = 1;
            altitude_goal = facts.terrain_altitude.require()?;
            steps.push(AirPhysicsStep::StoreReturning(1));
            steps.push(AirPhysicsStep::ReadTerrainAltitude(altitude_goal));
        }
    }

    let max_x = facts.map_width_tiles.wrapping_mul(0x300).wrapping_sub(1);
    let max_y = facts.map_height_tiles.wrapping_mul(0x300).wrapping_sub(1);
    let clamped_x = aim_x.clamp(0, max_x);
    let clamped_y = aim_y.clamp(0, max_y);
    let clamped = clamped_x != aim_x || clamped_y != aim_y;
    aim_x = clamped_x;
    aim_y = clamped_y;
    steps.push(AirPhysicsStep::ClampAim { x: aim_x, y: aim_y });

    let dx = aim_x.wrapping_sub(actor.x);
    let dy = aim_y.wrapping_sub(actor.y);
    let mut speed = facts.actor_speed.require()?;
    if facts.ai_speed > 1 {
        speed = speed.wrapping_mul(facts.ai_speed);
    }

    if order.returning != 0 {
        let close_enough = manhattan(dx, dy) < speed.wrapping_mul(3) / 2;
        let altitude_ready =
            !helicopter || actor.first_guy_z.wrapping_sub(altitude_goal) < LAND_ALTITUDE_DELTA;
        if close_enough && !clamped && altitude_ready {
            let mutation_digest = checked_mutation(facts.land_plane.require()?, transaction)?;
            steps.push(AirPhysicsStep::LandPlane { mutation_digest });
            steps.push(AirPhysicsStep::Return(AirPhysicsReturn::Stop));
            return Ok(finish(
                facts.snapshot,
                steps,
                order,
                draws,
                AirPhysicsReturn::Stop,
            ));
        }

        if order.home_o >= 0 && order.home_who >= 0 {
            if facts.home_is_active.require()? && manhattan(dx, dy) < HOME_MARK_DISTANCE {
                steps.push(AirPhysicsStep::OrHomeUnitMasks {
                    home_o: order.home_o,
                    home_who: order.home_who,
                    mask: PATH_MARK_HOME_MASK,
                });
            }
        }
    }

    steps.push(AirPhysicsStep::PushPath {
        x: aim_x,
        y: aim_y,
        tolerance: PATH_TARGET_TOLERANCE,
        flags: PATH_TARGET_FLAGS,
    });

    let mut aligned_with_air_target = false;
    if order.returning == 0 && !helicopter {
        if let CurrentAirTarget::Accepted { x, y } = facts.current_air_target.require()? {
            let target_dx = x.wrapping_sub(actor.x);
            let target_dy = y.wrapping_sub(actor.y);
            let min_range = facts.actor_min_range.require()?.wrapping_mul(0x0c0);
            if vector_dist(target_dx, target_dy) < min_range {
                let target_angle = find_angle(target_dx, target_dy) as u32;
                aligned_with_air_target =
                    angle_diff(target_angle, actor.angle as u32) < TURN_90_DEGREES as u32;
            }
        }
    }

    let mut desired_angle = find_angle(dx, dy);
    if !helicopter && angle_diff(desired_angle as u32, actor.angle as u32) > TURN_45_DEGREES {
        let min_distance = if order.returning == 0 {
            facts.actor_min_range.require()?.wrapping_mul(0x0c0)
        } else {
            0
        };
        if vector_dist(dx, dy) < min_distance.wrapping_add(COLLISION_DISTANCE) {
            desired_angle = actor.angle;
        }
    }
    if order.sharp_turn != 0 {
        desired_angle = actor
            .angle
            .wrapping_add(order.sharp_turn.wrapping_mul(TURN_90_DEGREES));
    }

    let bank = facts.bank.require()?;
    let bank_digest = checked_mutation(bank.mutation, transaction)?;
    steps.push(AirPhysicsStep::BankAircraft {
        desired_angle,
        aligned_with_air_target,
        speed_in: speed,
        mutation_digest: bank_digest,
    });
    actor.angle = bank.actor_angle_after;
    speed = bank.speed_after;

    let pitch = facts.pitch.require()?;
    let pitch_digest = checked_mutation(pitch.mutation, transaction)?;
    if pitch.speed_before != speed {
        return Err(AirPhysicsPlanError::InvalidHelperChain);
    }
    steps.push(AirPhysicsStep::PitchAircraft {
        dx,
        dy,
        altitude_goal,
        speed_in: speed,
        mutation_digest: pitch_digest,
    });
    speed = pitch.speed_after;

    let guy_digest = checked_mutation(facts.guy_set_angle.require()?, transaction)?;
    steps.push(AirPhysicsStep::GuySetAngle {
        angle: actor.angle,
        immediate: 1,
        mutation_digest: guy_digest,
    });

    let distance = vector_dist(dx, dy);
    let collide = if !helicopter {
        true
    } else {
        let threshold = if order.returning == 0 {
            HELICOPTER_OUTBOUND_DISTANCE
        } else {
            HELICOPTER_RETURN_DISTANCE
        };
        if distance < threshold && actor.queue_len > 1 {
            let mutation_digest =
                checked_mutation(facts.kill_current_order.require()?, transaction)?;
            steps.push(AirPhysicsStep::KillCurrentOrder { mutation_digest });
            steps.push(AirPhysicsStep::Return(AirPhysicsReturn::Stop));
            return Ok(finish(
                facts.snapshot,
                steps,
                order,
                draws,
                AirPhysicsReturn::Stop,
            ));
        }
        distance < threshold
    };

    let mut final_x = actor.x;
    let mut final_y = actor.y;
    if collide {
        let collision = facts.collision.require()?;
        let (projected_x, projected_y) = project(actor.x, actor.y, actor.angle, speed);
        steps.push(AirPhysicsStep::ProjectLocation {
            x: projected_x,
            y: projected_y,
        });
        steps.push(AirPhysicsStep::TestInvalidLocation {
            invalid: collision.invalid_location,
        });

        let (next_x, next_y) = if collision.invalid_location {
            let restricted = collision
                .restricted
                .ok_or(AirPhysicsPlanError::CollisionReceiptMismatch)?;
            steps.push(AirPhysicsStep::RestrictLocation {
                x: restricted.0,
                y: restricted.1,
            });
            if order.sharp_turn == 0 {
                let draw = consume_draw(
                    collision.turn_draw,
                    "collision-turn draw",
                    &mut steps,
                    &mut draws,
                    AirPhysicsStep::DrawCollisionTurn,
                )?;
                order.sharp_turn = if draw.value as u32 & 0x8000_0001 != 0 {
                    1
                } else {
                    -1
                };
                steps.push(AirPhysicsStep::StoreSharpTurn(order.sharp_turn));
            } else if collision.turn_draw.is_some() {
                return Err(AirPhysicsPlanError::UnexpectedRngDraw(
                    "collision-turn draw",
                ));
            }
            restricted
        } else {
            if collision.restricted.is_some() || collision.turn_draw.is_some() {
                return Err(AirPhysicsPlanError::CollisionReceiptMismatch);
            }
            order.sharp_turn = 0;
            steps.push(AirPhysicsStep::StoreSharpTurn(0));
            (projected_x, projected_y)
        };

        let location_digest = checked_mutation(collision.set_new_location, transaction)?;
        steps.push(AirPhysicsStep::SetNewLocation {
            x: next_x,
            y: next_y,
            mutation_digest: location_digest,
        });
        final_x = collision.actor_x_after;
        final_y = collision.actor_y_after;
    }

    if actor.recharging == 0 {
        let animation_digest = checked_mutation(facts.set_idle_animation.require()?, transaction)?;
        steps.push(AirPhysicsStep::SetAnimation {
            animation: IDLE_ANIMATION,
            b: 0,
            c: 1,
            mutation_digest: animation_digest,
        });
    }

    if facts.actor_type == BIRD_TERRAIN_TYPE {
        order.returning = 0;
        actor.recharging = 1;
        steps.push(AirPhysicsStep::StoreReturning(0));
        steps.push(AirPhysicsStep::StoreRecharge(1));
        if manhattan(aim_x.wrapping_sub(final_x), aim_y.wrapping_sub(final_y))
            < HELICOPTER_RETURN_DISTANCE
            && actor.queue_len > 1
        {
            let mutation_digest =
                checked_mutation(facts.kill_current_order.require()?, transaction)?;
            steps.push(AirPhysicsStep::KillCurrentOrder { mutation_digest });
            steps.push(AirPhysicsStep::Return(AirPhysicsReturn::Stop));
            return Ok(finish(
                facts.snapshot,
                steps,
                order,
                draws,
                AirPhysicsReturn::Stop,
            ));
        }
    }

    steps.push(AirPhysicsStep::Return(AirPhysicsReturn::Continue));
    Ok(finish(
        facts.snapshot,
        steps,
        order,
        draws,
        AirPhysicsReturn::Continue,
    ))
}

/// Attestation returned only after an adapter atomically publishes the entire plan.  Validation
/// compares the full snapshot and plan, not just an effect count or the integer return value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AirPhysicsCommitReceipt {
    pub snapshot: AirPhysicsSnapshot,
    pub plan: AirPhysicsPlan,
    pub committed_steps: usize,
    pub rng_epoch_after: u64,
}

impl AirPhysicsCommitReceipt {
    pub fn applied(plan: &AirPhysicsPlan) -> Self {
        Self {
            snapshot: plan.snapshot,
            plan: plan.clone(),
            committed_steps: plan.steps.len(),
            rng_epoch_after: plan.rng_epoch_after,
        }
    }

    pub fn validates(&self, plan: &AirPhysicsPlan) -> bool {
        self.snapshot == plan.snapshot
            && self.plan == *plan
            && self.committed_steps == plan.steps.len()
            && self.rng_epoch_after == plan.rng_epoch_after
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CfgBlock {
    pub start: u32,
    pub role: &'static str,
}

/// Semantic heads in the reachable top-level body. Nested callees have their own CFGs and are
/// represented by receipts above, not silently folded into these blocks.
pub const AIR_PHYSICS_CFG: &[CfgBlock] = &[
    CfgBlock {
        start: 0x005e_86d0,
        role: "entry / AirOrder projection",
    },
    CfgBlock {
        start: 0x005e_8712,
        role: "modulo-eight cruise cadence",
    },
    CfgBlock {
        start: 0x005e_8778,
        role: "cruise RNG",
    },
    CfgBlock {
        start: 0x005e_87cb,
        role: "clear path",
    },
    CfgBlock {
        start: 0x005e_87e0,
        role: "check_fuel",
    },
    CfgBlock {
        start: 0x005e_8802,
        role: "type-0x193 altitude cone",
    },
    CfgBlock {
        start: 0x005e_885c,
        role: "world clamp / speed",
    },
    CfgBlock {
        start: 0x005e_88fa,
        role: "returning arrival",
    },
    CfgBlock {
        start: 0x005e_8964,
        role: "home target marker",
    },
    CfgBlock {
        start: 0x005e_89be,
        role: "path append",
    },
    CfgBlock {
        start: 0x005e_8a0c,
        role: "air-target alignment",
    },
    CfgBlock {
        start: 0x005e_8b6d,
        role: "desired angle",
    },
    CfgBlock {
        start: 0x005e_8c00,
        role: "bank / pitch / Guy angle",
    },
    CfgBlock {
        start: 0x005e_8c31,
        role: "helicopter close gate",
    },
    CfgBlock {
        start: 0x005e_8c6d,
        role: "kill-current stop",
    },
    CfgBlock {
        start: 0x005e_8c81,
        role: "project / collision",
    },
    CfgBlock {
        start: 0x005e_8cf7,
        role: "collision-turn RNG",
    },
    CfgBlock {
        start: 0x005e_8d2c,
        role: "set_new_location",
    },
    CfgBlock {
        start: 0x005e_8d3d,
        role: "idle animation",
    },
    CfgBlock {
        start: 0x005e_8d53,
        role: "type-0x193 tail",
    },
    CfgBlock {
        start: 0x005e_8da9,
        role: "success return",
    },
];

/// These are transaction capabilities, not optional improvements. An adapter that lacks any
/// reached row must reject before consuming RNG or publishing a partial mutation.
pub const AIR_PHYSICS_OPEN_HOSTS: &[&str] = &[
    "TypeIdentityAndVirtualPredicates",
    "CheckFuelAtomicMutation",
    "TerrainAltitude",
    "PathStackAndAllocator",
    "ObjectPoolAndHomeMask",
    "CurrentAirTargetAndValidTarget",
    "BankAircraft",
    "PitchAircraft",
    "GuyStateAndSetAngle",
    "InvalidLocationAndWorldRestrict",
    "SetNewLocation",
    "LandPlane",
    "SetAnimation",
    "KillCurrentOrder",
    "AtomicMainRng",
];
