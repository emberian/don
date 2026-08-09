// SPDX-License-Identifier: GPL-3.0-or-later
//! Atomic planner for `Unit::do_follow` `0x005E65D0`.
//!
//! The retail executor does more than chase one target identity.  A `FollowOrder` owns a
//! primary identity and a secondary containment/fallback identity; both can change before
//! the distance test.  This module keeps that complete order state, the exact target-query
//! results, and every non-local mutation in one receipt-recomputable transaction.

use crate::systems::groups_guys::formation_order_coord;
use crate::systems::movement::{cosx, find_angle, sinx, vector_dist};

pub const FOLLOW_EXECUTOR_VA: u32 = 0x005E_65D0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FollowIdentity {
    pub o: i32,
    pub who: i32,
    pub uid: u16,
}

impl FollowIdentity {
    #[inline]
    pub fn same_slot(self, other: Self) -> bool {
        self.o == other.o && self.who == other.who
    }
}

/// Concrete `FollowOrder` fields: `TargetOrder` at `+8..+10`, followed by the secondary
/// identity at `+14..+1C`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FollowOrderState {
    pub primary: FollowIdentity,
    pub fallback: FollowIdentity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FollowActorFacts {
    pub o: i16,
    pub who: u8,
    pub x: i32,
    pub y: i32,
    /// `UnitData::speed()` `0x0060AAE0`.
    pub speed: i32,
    /// Actor virtual `los()` at vtable `+0x128`.
    pub los: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FollowExecutorRequest {
    pub actor: FollowActorFacts,
    pub order: FollowOrderState,
}

/// One coherent observation of an object slot.  The planner deliberately retains every
/// virtual queried by the executor rather than deriving `is_on_map` or `is_moving` from a
/// compact substitute.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FollowObjectFacts {
    pub identity: FollowIdentity,
    pub valid_unit: bool,
    pub on_map: bool,
    /// `SubObjectData::flags & 1`.
    pub active: bool,
    /// `UnitData::inside_up`; non-negative permits `ObjectData::get_inside`.
    pub inside_up: i16,
    pub seen_by_actor: bool,
    pub x: i32,
    pub y: i32,
    pub angle: i32,
    pub speed: i32,
    pub is_moving: bool,
    /// Result of virtual `get_captain()` at vtable `+0xE4`.
    pub captain_o: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NearbySpotResult {
    /// Retail `UnitType::find_nearby_spot` returned zero and wrote these coordinates.
    Found { x: i32, y: i32 },
    /// Any non-zero return.
    Failed,
}

/// Exact sixteen-argument `UnitType::find_nearby_spot` request, excluding its receiver and
/// the two output pointers.  Field names stop where shipped names stop; the last five
/// arguments are retained positionally rather than assigned speculative semantics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NearbySpotRequest {
    pub centre_x: i32,
    pub centre_y: i32,
    pub min_radius: i32,
    pub max_radius: i32,
    pub step: i32,
    pub angle: i32,
    pub filter: i32,
    pub actor_o: i32,
    pub actor_who: i32,
    pub tail_12: i32,
    pub tail_13: i32,
    pub tail_14: i32,
    pub tail_15: i32,
    pub tail_16: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NearbySpotRecord {
    pub request: NearbySpotRequest,
    pub result: NearbySpotResult,
}

/// Coherent external facts for one executor activation.  Optional object observations are
/// demanded only on their retail branch.  `nearby_results` is consumed in call order; too
/// few or too many results make the plan unavailable instead of changing control flow.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FollowExecutorFacts {
    pub primary: Option<FollowObjectFacts>,
    pub containment_root: Option<FollowObjectFacts>,
    pub fallback: Option<FollowObjectFacts>,
    pub fallback_captain: Option<FollowObjectFacts>,
    pub nearby_results: Vec<NearbySpotResult>,
}

/// Exact fixed arguments after `(x, y, angle)` in
/// `Unit::add_move_facing_order(UCoord,UCoord,int,int,int,QueuePos,int,int,Coord,Coord,int)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FollowMoveFacingTail {
    pub arg4: i32,
    pub arg5: i32,
    pub queued: i32,
    pub arg7: i32,
    pub arg8: i32,
    pub coord9: i32,
    pub coord10: i32,
    pub arg11: i32,
}

pub const FOLLOW_MOVE_FACING_TAIL: FollowMoveFacingTail = FollowMoveFacingTail {
    arg4: 1,
    arg5: 0,
    queued: 0,
    arg7: 0,
    arg8: -1,
    coord9: -1,
    coord10: -1,
    arg11: 0,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FollowExecutorEffect {
    /// Bare `Unit::kill_current_order(0)`.
    KillCurrentOrder { flags: i32 },
    /// Actor virtual `work()` at vtable `+0x188`, after fallback was copied to primary.
    ReenterWork,
    /// `Unit::set_anim(0,0,1)`.
    SetAnim { anim: i32, mode: i32, choose: i32 },
    /// `Unit::add_move_facing_order` with the exact retail tuple.
    AddMoveFacingOrder {
        x: i32,
        y: i32,
        angle: i32,
        tail: FollowMoveFacingTail,
    },
    /// `Unit::update_order()` followed immediately by `Unit::do_move(current)` in the same
    /// activation.  This is one indivisible host capability: deferring it changes the tick.
    UpdateOrderThenDoMove,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FollowExecutorBranch {
    KillInvalidTarget,
    PromoteFallbackAndReenterWork,
    KillUnseenTarget,
    HoldNearTarget,
    MoveTowardTarget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FollowExecutorPlan {
    pub order: FollowOrderState,
    pub radius: Option<i32>,
    pub cutoff: Option<i32>,
    pub searches: Vec<NearbySpotRecord>,
    pub effects: Vec<FollowExecutorEffect>,
    pub branch: FollowExecutorBranch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FollowExecutorPlanError {
    MissingPrimary,
    PrimaryIdentity,
    MissingContainmentRoot,
    ContainmentRootIdentity,
    MissingFallback,
    FallbackIdentity,
    MissingFallbackCaptain,
    FallbackCaptainIdentity,
    MissingNearbyResult { index: usize },
    UnexpectedNearbyResults { expected: usize, got: usize },
}

#[inline]
fn same_observed_slot(observed: FollowObjectFacts, expected: FollowIdentity) -> bool {
    observed.identity.o == expected.o && observed.identity.who == expected.who
}

#[inline]
fn project(x: i32, y: i32, angle: i32, distance: i32) -> (i32, i32) {
    (
        x.wrapping_add(sinx(angle, distance)),
        y.wrapping_sub(cosx(angle, distance)),
    )
}

#[inline]
fn search_request(
    actor: FollowActorFacts,
    centre: (i32, i32),
    min_radius: i32,
    max_radius: i32,
    step: i32,
    angle: i32,
) -> NearbySpotRequest {
    NearbySpotRequest {
        centre_x: centre.0,
        centre_y: centre.1,
        min_radius,
        max_radius,
        step,
        angle,
        filter: 3,
        actor_o: i32::from(actor.o),
        actor_who: i32::from(actor.who),
        tail_12: 0,
        tail_13: 0,
        tail_14: -1,
        tail_15: 0,
        tail_16: -1,
    }
}

fn take_search(
    facts: &FollowExecutorFacts,
    searches: &mut Vec<NearbySpotRecord>,
    request: NearbySpotRequest,
) -> Result<NearbySpotResult, FollowExecutorPlanError> {
    let index = searches.len();
    let result = facts
        .nearby_results
        .get(index)
        .copied()
        .ok_or(FollowExecutorPlanError::MissingNearbyResult { index })?;
    searches.push(NearbySpotRecord { request, result });
    Ok(result)
}

fn finish(
    facts: &FollowExecutorFacts,
    plan: FollowExecutorPlan,
) -> Result<FollowExecutorPlan, FollowExecutorPlanError> {
    if facts.nearby_results.len() != plan.searches.len() {
        return Err(FollowExecutorPlanError::UnexpectedNearbyResults {
            expected: plan.searches.len(),
            got: facts.nearby_results.len(),
        });
    }
    Ok(plan)
}

/// Plan one complete `Unit::do_follow` activation in instruction order.
///
/// The host must acquire all reached facts and every emitted effect capability before it
/// publishes the returned order after-image.  In particular, `ReenterWork` is the full
/// actor virtual and `UpdateOrderThenDoMove` includes the same-tick movement activation.
pub fn plan_follow_executor(
    request: &FollowExecutorRequest,
    facts: &FollowExecutorFacts,
) -> Result<FollowExecutorPlan, FollowExecutorPlanError> {
    let mut order = request.order;

    let mut primary = if order.primary.o >= 0 {
        let observed = facts
            .primary
            .ok_or(FollowExecutorPlanError::MissingPrimary)?;
        if !same_observed_slot(observed, order.primary) {
            return Err(FollowExecutorPlanError::PrimaryIdentity);
        }
        Some(observed)
    } else {
        None
    };

    if let Some(observed) = primary {
        if (!observed.valid_unit || !observed.on_map) && observed.active && observed.inside_up >= 0
        {
            let root = facts
                .containment_root
                .ok_or(FollowExecutorPlanError::MissingContainmentRoot)?;
            if root.identity.o < 0 || root.identity.who < 0 {
                return Err(FollowExecutorPlanError::ContainmentRootIdentity);
            }
            if root.valid_unit && root.on_map {
                order.fallback = order.primary;
                order.primary = root.identity;
                primary = Some(root);
            }
        }
    }

    let primary_valid = primary.is_some_and(|p| p.valid_unit && p.on_map);
    if order.primary.o < 0 || !primary_valid {
        if !order.fallback.same_slot(order.primary) {
            order.primary = order.fallback;
            return finish(
                facts,
                FollowExecutorPlan {
                    order,
                    radius: None,
                    cutoff: None,
                    searches: Vec::new(),
                    effects: vec![FollowExecutorEffect::ReenterWork],
                    branch: FollowExecutorBranch::PromoteFallbackAndReenterWork,
                },
            );
        }
        return finish(
            facts,
            FollowExecutorPlan {
                order,
                radius: None,
                cutoff: None,
                searches: Vec::new(),
                effects: vec![FollowExecutorEffect::KillCurrentOrder { flags: 0 }],
                branch: FollowExecutorBranch::KillInvalidTarget,
            },
        );
    }
    let primary = primary.expect("primary_valid proves an observation");

    if !order.fallback.same_slot(order.primary) {
        let fallback = facts
            .fallback
            .ok_or(FollowExecutorPlanError::MissingFallback)?;
        if !same_observed_slot(fallback, order.fallback) {
            return Err(FollowExecutorPlanError::FallbackIdentity);
        }
        if fallback.identity.uid == order.fallback.uid {
            let expected = FollowIdentity {
                o: fallback.captain_o,
                who: order.fallback.who,
                uid: 0,
            };
            let captain = facts
                .fallback_captain
                .ok_or(FollowExecutorPlanError::MissingFallbackCaptain)?;
            if captain.identity.o != expected.o || captain.identity.who != expected.who {
                return Err(FollowExecutorPlanError::FallbackCaptainIdentity);
            }
            order.fallback.o = captain.identity.o;
            order.fallback.uid = captain.identity.uid;
        } else {
            order.fallback = order.primary;
        }
    }

    if !primary.seen_by_actor {
        return finish(
            facts,
            FollowExecutorPlan {
                order,
                radius: None,
                cutoff: None,
                searches: Vec::new(),
                effects: vec![FollowExecutorEffect::KillCurrentOrder { flags: 0 }],
                branch: FollowExecutorBranch::KillUnseenTarget,
            },
        );
    }

    let actor = request.actor;
    let distance = vector_dist(
        primary.x.wrapping_sub(actor.x),
        primary.y.wrapping_sub(actor.y),
    );
    let mut margin = if actor.speed > primary.speed {
        actor.los.wrapping_mul(0x60)
    } else {
        actor.los.wrapping_mul(0x300) / 5
    };
    if primary.is_moving {
        margin = margin.wrapping_mul(2);
    }
    let radius = actor
        .los
        .wrapping_mul(0x180)
        .wrapping_sub(margin)
        .max(0x180)
        .min(0x600);
    let cutoff = radius.wrapping_add(0xC0);

    if distance <= cutoff {
        return finish(
            facts,
            FollowExecutorPlan {
                order,
                radius: Some(radius),
                cutoff: Some(cutoff),
                searches: Vec::new(),
                effects: vec![FollowExecutorEffect::SetAnim {
                    anim: 0,
                    mode: 0,
                    choose: 1,
                }],
                branch: FollowExecutorBranch::HoldNearTarget,
            },
        );
    }

    let mut searches = Vec::new();
    let approach_angle = find_angle(
        actor.x.wrapping_sub(primary.x),
        actor.y.wrapping_sub(primary.y),
    );
    let first_centre = project(primary.x, primary.y, approach_angle, radius);
    let first = take_search(
        facts,
        &mut searches,
        search_request(actor, first_centre, 0, -1, 0, 0x5555_5555),
    )?;
    let destination = match first {
        NearbySpotResult::Found { x, y } => (x, y),
        NearbySpotResult::Failed => {
            let rear_angle = primary.angle.wrapping_sub(0x8000_0000u32 as i32);
            let second_centre = project(primary.x, primary.y, rear_angle, radius);
            let second = take_search(
                facts,
                &mut searches,
                search_request(actor, second_centre, 0, -1, 0, 0x5555_5555),
            )?;
            match second {
                NearbySpotResult::Found { x, y } => (x, y),
                NearbySpotResult::Failed => {
                    let third = take_search(
                        facts,
                        &mut searches,
                        search_request(
                            actor,
                            (primary.x, primary.y),
                            radius,
                            cutoff,
                            0x30,
                            primary.angle,
                        ),
                    )?;
                    match third {
                        NearbySpotResult::Found { x, y } => (x, y),
                        NearbySpotResult::Failed => (primary.x, primary.y),
                    }
                }
            }
        }
    };

    finish(
        facts,
        FollowExecutorPlan {
            order,
            radius: Some(radius),
            cutoff: Some(cutoff),
            searches,
            effects: vec![
                FollowExecutorEffect::AddMoveFacingOrder {
                    x: formation_order_coord(destination.0),
                    y: formation_order_coord(destination.1),
                    angle: primary.angle,
                    tail: FOLLOW_MOVE_FACING_TAIL,
                },
                FollowExecutorEffect::UpdateOrderThenDoMove,
            ],
            branch: FollowExecutorBranch::MoveTowardTarget,
        },
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FollowExecutorTransactionStatus {
    Applied,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FollowExecutorReceipt {
    pub request: FollowExecutorRequest,
    pub status: FollowExecutorTransactionStatus,
    pub facts: Option<FollowExecutorFacts>,
    pub plan: Option<FollowExecutorPlan>,
}

impl FollowExecutorReceipt {
    pub fn unavailable(request: FollowExecutorRequest) -> Self {
        Self {
            request,
            status: FollowExecutorTransactionStatus::Unavailable,
            facts: None,
            plan: None,
        }
    }

    pub fn validates(&self, expected: &FollowExecutorRequest) -> bool {
        if &self.request != expected {
            return false;
        }
        match self.status {
            FollowExecutorTransactionStatus::Unavailable => {
                self.facts.is_none() && self.plan.is_none()
            }
            FollowExecutorTransactionStatus::Applied => {
                let (Some(facts), Some(observed)) = (&self.facts, &self.plan) else {
                    return false;
                };
                plan_follow_executor(expected, facts)
                    .is_ok_and(|recomputed| recomputed == *observed)
            }
        }
    }
}
