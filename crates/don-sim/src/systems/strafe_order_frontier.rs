// SPDX-License-Identifier: GPL-3.0-or-later
//! Source-only reconstruction frontier for `Unit::do_strafe`.
//!
//! This module is intentionally absent from `systems/mod.rs`.  It freezes the shipped
//! executor's payload, reachable control-flow cones, external air-physics/RNG boundary, and
//! mutation receipts without claiming that the live dispatcher owns those host surfaces yet.

pub const STRAFE_ORDER_INDEX: i32 = 16;
pub const UNIT_DO_STRAFE_VA: u32 = 0x005e_ab00;
pub const UNIT_DO_STRAFE_BYTES: usize = 3_676;
pub const UNIT_ADD_STRAFE_ORDER_VA: u32 = 0x005e_48c0;
pub const UNIT_DO_AIR_PHYSICS_VA: u32 = 0x005e_86d0;
pub const FIND_NEW_BOMBER_TARGET_VA: u32 = 0x005e_b960;
pub const FIND_NEW_AIR_TARGET_VA: u32 = 0x005e_bc70;
pub const STRAFE_ORDER_SIZE: usize = 84;
pub const STRAFE_WALKED_BYTES: usize = 57;
pub const STRAFE_SCAN_PERIOD: i32 = 16;
pub const STRAFE_REACQUIRE_PERIOD: i32 = 32;
pub const AIR_CRUISING_ALTITUDE: i32 = 0x640;
pub const ATTACK_ANIM: i32 = 0x0c;
pub const IDLE_ANIM: i32 = 8;
pub const TURN_15_DEGREES: u32 = 0x0aaa_aaaa;
pub const TURN_60_DEGREES: u32 = 0x2aaa_aaaa;
pub const TURN_90_DEGREES: u32 = 0x4000_0000;

/// Concrete offsets in the shipped `StrafeOrder` object.
pub mod offsets {
    pub const TARGET_O: usize = 0x08;
    pub const TARGET_WHO: usize = 0x0c;
    pub const TARGET_UID: usize = 0x10;
    pub const DEF_X: usize = 0x14;
    pub const DEF_Y: usize = 0x18;
    pub const MANDATORY: usize = 0x1c;
    pub const DEFENSIVE: usize = 0x1d;
    pub const IN_RANGE: usize = 0x1e;
    pub const EVER_IN_RANGE: usize = 0x1f;
    pub const NEW_ORD: usize = 0x20;
    pub const AIR_OXX: usize = 0x28;
    pub const AIR_WHOSE: usize = 0x2c;
    pub const AIR_CRUISING_ALT: usize = 0x30;
    pub const AIR_SHARP_TURN: usize = 0x34;
    pub const AIR_OLD: usize = 0x38;
    pub const AIR_RETURNING: usize = 0x3c;
    pub const XX: usize = 0x40;
    pub const YY: usize = 0x44;
    pub const UNIT_ORDER_VBASE: usize = 0x4c;
    pub const FLAGS: usize = 0x50;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ObjectIdentity {
    pub o: i32,
    pub who: i32,
    pub uid: u16,
}

/// Consume the already-landed, checksum-complete payload rather than introducing another
/// STRAFE representation.  Common `UnitOrder::flags` stays on the surrounding `OrderRec`.
pub use crate::systems::air::AirOrderWalk as AirOrderState;
pub use crate::systems::patrol::StrafeOrder as StrafeOrderState;

fn order_target(order: &StrafeOrderState) -> ObjectIdentity {
    ObjectIdentity {
        o: order.target_o,
        who: order.target_who,
        uid: order.target_uid,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetSnapshot {
    pub identity: ObjectIdentity,
    pub x: i32,
    pub y: i32,
    pub active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorSnapshot {
    pub identity: ObjectIdentity,
    pub x: i32,
    pub y: i32,
    pub angle: u32,
    pub active: bool,
    pub animal: bool,
    pub missile: bool,
    pub helicopter: bool,
    pub bomber: bool,
    pub strafes: bool,
    pub queue_len: u32,
    pub attack_latch: u8,
    pub spell_time: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirTargetSearchKind {
    AirFirst,
    BomberFirst,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchObservation {
    NotDue,
    Miss,
    Hit(TargetSnapshot),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AimCone {
    /// Use the target's live position without a lead projection.
    Live { x: i32, y: i32 },
    /// Helicopter `is_in_range` succeeded; project 0x30 along the target bearing, then
    /// apply `WorldData::restrict`.
    HelicopterLead { projected_x: i32, projected_y: i32 },
    /// A live building target whose action domain is 2, whose type is not helicopter, and
    /// whose range exceeds 0xC00 may be pulled toward its action point by `(dist-0xC00)/3`.
    BuildingStandOff {
        distance: i32,
        projected_x: i32,
        projected_y: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingTargetCone {
    /// Animal override: call `think_bird(order,1)` and return; an actor killed by the call
    /// skips the following `work()` call.
    Animal {
        active_after_think: bool,
    },
    MissileDie,
    /// Helicopter: kill the order, then either work immediately when `order_type()!=NONE`,
    /// or install a patrol at the actor position with the preserved home identity.
    Helicopter {
        post_kill_order_type: i32,
    },
    QueueFallback,
    SavedPatrol {
        group_flag: i32,
    },
    LatchReturning,
}

/// The entire pre-physics CFG cone.  Target/captain lookup and target-search selection are
/// authoritative host reads; this enum makes every reachable retail result explicit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrontCone {
    AlreadyReturning,
    Missing(MissingTargetCone),
    Active {
        target: TargetSnapshot,
        /// `Some` only when the inactive original target was repaired through
        /// `ObjectData::get_captain()`.
        repaired_from: Option<ObjectIdentity>,
        aim: AimCone,
        search_kind: AirTargetSearchKind,
        scan: SearchObservation,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RngDraw {
    pub lo: i32,
    pub hi_exclusive: i32,
    pub value: i32,
}

impl RngDraw {
    pub fn is_retail_air_draw(self) -> bool {
        self.lo == 0 && self.hi_exclusive == 0xffff && (0..0xffff).contains(&self.value)
    }
}

/// Attested result of `Unit::do_air_physics`.  That callee owns movement/path mutations and
/// conditionally consumes `Random::get(0,0xFFFF)`; STRAFE itself draws no random number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AirPhysicsReceipt {
    pub completed: bool,
    pub mutation_digest: u64,
    pub rng_epoch_before: u64,
    pub rng_epoch_after: u64,
    pub draws: Vec<RngDraw>,
}

impl AirPhysicsReceipt {
    fn valid(&self) -> bool {
        self.draws.iter().copied().all(RngDraw::is_retail_air_draw)
            && self.rng_epoch_after == self.rng_epoch_before.wrapping_add(self.draws.len() as u64)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttackTail {
    /// Return byte from virtual `+0x134(0)`; retail stores its wrapping `+1` at actor `+0xAE`.
    pub attack_call_al: u8,
    /// Exact `Constants+0xC14` word, present only on the bomber arm.
    pub bomber_spell_delta: Option<i16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FireCone {
    /// Target/range/turn gates hold the frame without an attack mutation.
    Hold,
    /// `fire_ammo(target_o,target_who)`, followed by the common attack tail.
    FireAmmo(AttackTail),
    /// `set_anim(ATTACK,0,1)`, followed by the common attack tail.
    AttackAnimation(AttackTail),
    /// Missile self-destruction through virtual `die(0,-1,0)`.
    MissileDie,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueuedSearchOrigin {
    AirPatrol { x: i32, y: i32 },
    StrafeTarget(TargetSnapshot),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReacquireCone {
    NotEligible,
    NotDue,
    Search {
        origin: QueuedSearchOrigin,
        kind: AirTargetSearchKind,
        result: SearchObservation,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostPhysicsCone {
    /// Used exactly when the air-physics receipt returns zero.
    PhysicsStopped,
    /// Animal success tail: idle animation, then the shared queued-order cone.
    Animal { reacquire: ReacquireCone },
    /// Retail's common kill-current + idle-animation failure tail.
    KillAndIdle,
    /// Normal target/range/munition cone followed by queued reacquisition.
    Combat {
        fire: FireCone,
        reacquire: ReacquireCone,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StrafeFrameFacts {
    pub frame: i32,
    pub actor: ActorSnapshot,
    pub order: StrafeOrderState,
    pub front: FrontCone,
    pub physics: Option<AirPhysicsReceipt>,
    pub post: PostPhysicsCone,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrafeStep {
    ThinkBird {
        mode: i32,
    },
    StoreTargetPair {
        o: i32,
        who: i32,
    },
    StoreTargetIdentity(ObjectIdentity),
    StoreTargetPosition {
        x: i32,
        y: i32,
    },
    StoreReturning(i32),
    AirPhysics {
        x: i32,
        y: i32,
        digest: u64,
    },
    KillCurrent,
    AddAirPatrol {
        x: i32,
        y: i32,
        home_o: i32,
        home_who: i32,
        group_flag: i32,
    },
    InsertStrafeFirst {
        target: ObjectIdentity,
        home_o: i32,
        home_who: i32,
    },
    UpdateWork,
    Die,
    SetAnimation {
        animation: i32,
    },
    FireAmmo(ObjectIdentity),
    StoreAttackLatch(u8),
    AddSpellTime(i16),
    Hold,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StrafeFramePlan {
    pub steps: Vec<StrafeStep>,
    /// STRAFE direct draws are always zero.  These are only the verified draws performed
    /// inside the nested air-physics receipt.
    pub physics_draws: Vec<RngDraw>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrafePlanError {
    TargetIdentityMismatch,
    RepairedTargetWasNotReplaced,
    ScanCadenceMismatch,
    SearchHitInactive,
    MissingPhysicsReceipt,
    UnexpectedPhysicsReceipt,
    InvalidPhysicsRngReceipt,
    PostPhysicsMismatch,
    AnimalConeMismatch,
    MissileConeMismatch,
    HelicopterConeMismatch,
    QueueFallbackMismatch,
    SavedPatrolUnavailable,
    AimConeMismatch,
    SearchKindMismatch,
    ReacquireCadenceMismatch,
    ReacquireResultMismatch,
    FireConeMismatch,
    ReceiptSnapshotMismatch,
    ReceiptFactsMismatch,
    ReceiptPlanMismatch,
}

#[inline]
pub fn scan_due(actor_o: i32, frame: i32) -> bool {
    actor_o.wrapping_add(frame) % STRAFE_SCAN_PERIOD == 0
}

#[inline]
pub fn reacquire_due(actor_o: i32, frame: i32) -> bool {
    actor_o.wrapping_mul(2).wrapping_add(frame) % STRAFE_REACQUIRE_PERIOD == 0
}

fn scan_matches(due: bool, scan: SearchObservation) -> bool {
    due != matches!(scan, SearchObservation::NotDue)
}

fn append_reacquire(
    facts: &StrafeFrameFacts,
    reacquire: ReacquireCone,
    steps: &mut Vec<StrafeStep>,
) -> Result<(), StrafePlanError> {
    match reacquire {
        ReacquireCone::NotEligible => Ok(()),
        ReacquireCone::NotDue => {
            if reacquire_due(facts.actor.identity.o, facts.frame) {
                Err(StrafePlanError::ReacquireCadenceMismatch)
            } else {
                Ok(())
            }
        }
        ReacquireCone::Search { result, .. } => {
            if !reacquire_due(facts.actor.identity.o, facts.frame) {
                return Err(StrafePlanError::ReacquireCadenceMismatch);
            }
            let ReacquireCone::Search { kind, .. } = reacquire else {
                unreachable!()
            };
            let expected = if facts.actor.bomber {
                AirTargetSearchKind::BomberFirst
            } else {
                AirTargetSearchKind::AirFirst
            };
            if kind != expected {
                return Err(StrafePlanError::SearchKindMismatch);
            }
            match result {
                SearchObservation::NotDue => Err(StrafePlanError::ReacquireResultMismatch),
                SearchObservation::Miss => {
                    steps.push(StrafeStep::KillCurrent);
                    steps.push(StrafeStep::SetAnimation {
                        animation: IDLE_ANIM,
                    });
                    Ok(())
                }
                SearchObservation::Hit(target) => {
                    if !target.active {
                        return Err(StrafePlanError::SearchHitInactive);
                    }
                    steps.push(StrafeStep::StoreTargetIdentity(target.identity));
                    steps.push(StrafeStep::StoreReturning(0));
                    Ok(())
                }
            }
        }
    }
}

fn append_attack_tail(
    facts: &StrafeFrameFacts,
    tail: AttackTail,
    steps: &mut Vec<StrafeStep>,
) -> Result<(), StrafePlanError> {
    steps.push(StrafeStep::StoreAttackLatch(
        tail.attack_call_al.wrapping_add(1),
    ));
    match tail.bomber_spell_delta {
        Some(delta) if facts.actor.bomber => steps.push(StrafeStep::AddSpellTime(delta)),
        Some(_) => return Err(StrafePlanError::FireConeMismatch),
        None => {}
    }
    Ok(())
}

/// Build one ordered, fail-closed retail frame plan.
pub fn plan_strafe_frame(facts: &StrafeFrameFacts) -> Result<StrafeFramePlan, StrafePlanError> {
    let mut steps = vec![StrafeStep::ThinkBird { mode: 0 }];
    let mut aim = (-1, -1);

    match facts.front {
        FrontCone::AlreadyReturning => {}
        FrontCone::Missing(cone) => match cone {
            MissingTargetCone::Animal { active_after_think } => {
                if !facts.actor.animal {
                    return Err(StrafePlanError::AnimalConeMismatch);
                }
                steps.push(StrafeStep::ThinkBird { mode: 1 });
                if active_after_think {
                    steps.push(StrafeStep::UpdateWork);
                }
                if facts.physics.is_some() {
                    return Err(StrafePlanError::UnexpectedPhysicsReceipt);
                }
                return Ok(StrafeFramePlan {
                    steps,
                    physics_draws: Vec::new(),
                });
            }
            MissingTargetCone::MissileDie => {
                if !facts.actor.missile {
                    return Err(StrafePlanError::MissileConeMismatch);
                }
                steps.push(StrafeStep::Die);
                return Ok(StrafeFramePlan {
                    steps,
                    physics_draws: Vec::new(),
                });
            }
            MissingTargetCone::Helicopter {
                post_kill_order_type,
            } => {
                if !facts.actor.helicopter {
                    return Err(StrafePlanError::HelicopterConeMismatch);
                }
                steps.push(StrafeStep::KillCurrent);
                if post_kill_order_type == 0 {
                    steps.push(StrafeStep::AddAirPatrol {
                        x: facts.actor.x,
                        y: facts.actor.y,
                        home_o: facts.order.air.oxx,
                        home_who: facts.order.air.whose,
                        group_flag: 0,
                    });
                }
                steps.push(StrafeStep::UpdateWork);
                return Ok(StrafeFramePlan {
                    steps,
                    physics_draws: Vec::new(),
                });
            }
            MissingTargetCone::QueueFallback => {
                if facts.actor.queue_len <= 1 {
                    return Err(StrafePlanError::QueueFallbackMismatch);
                }
                steps.extend([
                    StrafeStep::KillCurrent,
                    StrafeStep::SetAnimation {
                        animation: IDLE_ANIM,
                    },
                    StrafeStep::UpdateWork,
                ]);
                return Ok(StrafeFramePlan {
                    steps,
                    physics_draws: Vec::new(),
                });
            }
            MissingTargetCone::SavedPatrol { group_flag } => {
                if facts.order.xx < 0 || facts.order.yy < 0 {
                    return Err(StrafePlanError::SavedPatrolUnavailable);
                }
                steps.extend([
                    StrafeStep::KillCurrent,
                    StrafeStep::AddAirPatrol {
                        x: facts.order.xx,
                        y: facts.order.yy,
                        home_o: facts.order.air.oxx,
                        home_who: facts.order.air.whose,
                        group_flag,
                    },
                    StrafeStep::UpdateWork,
                ]);
                return Ok(StrafeFramePlan {
                    steps,
                    physics_draws: Vec::new(),
                });
            }
            MissingTargetCone::LatchReturning => steps.extend([
                StrafeStep::StoreReturning(1),
                StrafeStep::StoreTargetPair { o: -1, who: -1 },
            ]),
        },
        FrontCone::Active {
            target,
            repaired_from,
            aim: target_aim,
            search_kind,
            scan,
        } => {
            if !target.active {
                return Err(StrafePlanError::TargetIdentityMismatch);
            }
            if let Some(old) = repaired_from {
                if old == target.identity {
                    return Err(StrafePlanError::RepairedTargetWasNotReplaced);
                }
                steps.push(StrafeStep::StoreTargetPair {
                    o: target.identity.o,
                    who: target.identity.who,
                });
            } else if target.identity.o != facts.order.target_o
                || target.identity.who != facts.order.target_who
            {
                return Err(StrafePlanError::TargetIdentityMismatch);
            }
            steps.push(StrafeStep::StoreTargetPosition {
                x: target.x,
                y: target.y,
            });
            aim = match target_aim {
                AimCone::Live { x, y } => {
                    if x != target.x || y != target.y {
                        return Err(StrafePlanError::AimConeMismatch);
                    }
                    (x, y)
                }
                AimCone::HelicopterLead {
                    projected_x,
                    projected_y,
                } => {
                    if !facts.actor.helicopter {
                        return Err(StrafePlanError::AimConeMismatch);
                    }
                    (projected_x, projected_y)
                }
                AimCone::BuildingStandOff {
                    distance,
                    projected_x,
                    projected_y,
                } => {
                    if distance <= 0xc00 {
                        return Err(StrafePlanError::AimConeMismatch);
                    }
                    (projected_x, projected_y)
                }
            };
            if !scan_matches(scan_due(facts.actor.identity.o, facts.frame), scan) {
                return Err(StrafePlanError::ScanCadenceMismatch);
            }
            if !matches!(scan, SearchObservation::NotDue) {
                let expected = if facts.actor.bomber {
                    AirTargetSearchKind::BomberFirst
                } else {
                    AirTargetSearchKind::AirFirst
                };
                if search_kind != expected {
                    return Err(StrafePlanError::SearchKindMismatch);
                }
            }
            if let SearchObservation::Hit(found) = scan {
                if !found.active {
                    return Err(StrafePlanError::SearchHitInactive);
                }
                steps.push(StrafeStep::InsertStrafeFirst {
                    target: found.identity,
                    home_o: facts.order.air.oxx,
                    home_who: facts.order.air.whose,
                });
                steps.push(StrafeStep::UpdateWork);
                if facts.physics.is_some() {
                    return Err(StrafePlanError::UnexpectedPhysicsReceipt);
                }
                return Ok(StrafeFramePlan {
                    steps,
                    physics_draws: Vec::new(),
                });
            }
        }
    }

    let physics = facts
        .physics
        .as_ref()
        .ok_or(StrafePlanError::MissingPhysicsReceipt)?;
    if !physics.valid() {
        return Err(StrafePlanError::InvalidPhysicsRngReceipt);
    }
    steps.push(StrafeStep::AirPhysics {
        x: aim.0,
        y: aim.1,
        digest: physics.mutation_digest,
    });
    if !physics.completed {
        if facts.post != PostPhysicsCone::PhysicsStopped {
            return Err(StrafePlanError::PostPhysicsMismatch);
        }
        return Ok(StrafeFramePlan {
            steps,
            physics_draws: physics.draws.clone(),
        });
    }
    if facts.post == PostPhysicsCone::PhysicsStopped {
        return Err(StrafePlanError::PostPhysicsMismatch);
    }

    match facts.post {
        PostPhysicsCone::PhysicsStopped => unreachable!(),
        PostPhysicsCone::Animal { reacquire } => {
            if !facts.actor.animal {
                return Err(StrafePlanError::AnimalConeMismatch);
            }
            steps.push(StrafeStep::SetAnimation {
                animation: IDLE_ANIM,
            });
            append_reacquire(facts, reacquire, &mut steps)?;
        }
        PostPhysicsCone::KillAndIdle => steps.extend([
            StrafeStep::KillCurrent,
            StrafeStep::SetAnimation {
                animation: IDLE_ANIM,
            },
        ]),
        PostPhysicsCone::Combat { fire, reacquire } => {
            if facts.actor.animal {
                return Err(StrafePlanError::AnimalConeMismatch);
            }
            match fire {
                FireCone::Hold => steps.push(StrafeStep::Hold),
                FireCone::FireAmmo(tail) => {
                    if facts.actor.missile || facts.actor.strafes {
                        return Err(StrafePlanError::FireConeMismatch);
                    }
                    steps.push(StrafeStep::FireAmmo(order_target(&facts.order)));
                    append_attack_tail(facts, tail, &mut steps)?;
                }
                FireCone::AttackAnimation(tail) => {
                    steps.push(StrafeStep::SetAnimation {
                        animation: ATTACK_ANIM,
                    });
                    append_attack_tail(facts, tail, &mut steps)?;
                }
                FireCone::MissileDie => {
                    if !facts.actor.missile {
                        return Err(StrafePlanError::MissileConeMismatch);
                    }
                    steps.push(StrafeStep::Die);
                    return Ok(StrafeFramePlan {
                        steps,
                        physics_draws: physics.draws.clone(),
                    });
                }
            }
            append_reacquire(facts, reacquire, &mut steps)?;
        }
    }

    Ok(StrafeFramePlan {
        steps,
        physics_draws: physics.draws.clone(),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StrafeHostSnapshot {
    pub actor: ObjectIdentity,
    pub actor_version: u64,
    pub order_version: u64,
    pub target_pool_epoch: u64,
    pub queue_digest: u64,
    pub path_digest: u64,
    pub external_effect_epoch: u64,
    pub rng_epoch: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StrafeExecutorReceipt {
    pub snapshot: StrafeHostSnapshot,
    pub facts: StrafeFrameFacts,
    pub plan: StrafeFramePlan,
}

impl StrafeExecutorReceipt {
    pub fn preflight(
        snapshot: StrafeHostSnapshot,
        facts: StrafeFrameFacts,
    ) -> Result<Self, StrafePlanError> {
        if snapshot.actor != facts.actor.identity {
            return Err(StrafePlanError::ReceiptSnapshotMismatch);
        }
        if let Some(physics) = &facts.physics {
            if snapshot.rng_epoch != physics.rng_epoch_before {
                return Err(StrafePlanError::ReceiptSnapshotMismatch);
            }
        }
        let plan = plan_strafe_frame(&facts)?;
        Ok(Self {
            snapshot,
            facts,
            plan,
        })
    }

    pub fn validates(
        &self,
        snapshot: StrafeHostSnapshot,
        facts: &StrafeFrameFacts,
    ) -> Result<(), StrafePlanError> {
        if self.snapshot != snapshot {
            return Err(StrafePlanError::ReceiptSnapshotMismatch);
        }
        if &self.facts != facts {
            return Err(StrafePlanError::ReceiptFactsMismatch);
        }
        let recomputed = plan_strafe_frame(facts)?;
        if self.plan != recomputed {
            return Err(StrafePlanError::ReceiptPlanMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CfgBlock {
    pub start: u32,
    pub label: &'static str,
}

/// Reachable semantic block heads recovered from the shipped function.  These are evidence
/// anchors for the high-level cones above, not a replacement disassembly.
pub const STRAFE_REACHABLE_CFG: &[CfgBlock] = &[
    CfgBlock {
        start: 0x005e_ab00,
        label: "entry/update_strafe/think_bird_0",
    },
    CfgBlock {
        start: 0x005e_ab39,
        label: "returning gate",
    },
    CfgBlock {
        start: 0x005e_ab52,
        label: "allied target gate",
    },
    CfgBlock {
        start: 0x005e_aba1,
        label: "captain repair",
    },
    CfgBlock {
        start: 0x005e_abd3,
        label: "saved-position fallback",
    },
    CfgBlock {
        start: 0x005e_ac2c,
        label: "live target refresh",
    },
    CfgBlock {
        start: 0x005e_ac8a,
        label: "mod-16 target scan",
    },
    CfgBlock {
        start: 0x005e_acf5,
        label: "front strafe insertion",
    },
    CfgBlock {
        start: 0x005e_ad3c,
        label: "invalid target cone",
    },
    CfgBlock {
        start: 0x005e_ad5f,
        label: "animal invalid-target tail",
    },
    CfgBlock {
        start: 0x005e_ad8b,
        label: "missile death tail",
    },
    CfgBlock {
        start: 0x005e_adc9,
        label: "helicopter no-target tail",
    },
    CfgBlock {
        start: 0x005e_ae10,
        label: "queued-order fallback",
    },
    CfgBlock {
        start: 0x005e_ae42,
        label: "saved-position patrol",
    },
    CfgBlock {
        start: 0x005e_aedb,
        label: "helicopter lead projection",
    },
    CfgBlock {
        start: 0x005e_af94,
        label: "building stand-off projection",
    },
    CfgBlock {
        start: 0x005e_b0ae,
        label: "air physics boundary",
    },
    CfgBlock {
        start: 0x005e_b0c2,
        label: "post-physics animal gate",
    },
    CfgBlock {
        start: 0x005e_b0df,
        label: "queued air-order leash",
    },
    CfgBlock {
        start: 0x005e_b26c,
        label: "kill-current terminal",
    },
    CfgBlock {
        start: 0x005e_b395,
        label: "kill-and-idle terminal",
    },
    CfgBlock {
        start: 0x005e_b3b7,
        label: "bearing/range fire gate",
    },
    CfgBlock {
        start: 0x005e_b478,
        label: "ammo-or-animation",
    },
    CfgBlock {
        start: 0x005e_b4d0,
        label: "post-shot missile/latch",
    },
    CfgBlock {
        start: 0x005e_b562,
        label: "returning bearing gate",
    },
    CfgBlock {
        start: 0x005e_b62b,
        label: "mod-32 queued reacquire gate",
    },
    CfgBlock {
        start: 0x005e_b6ad,
        label: "air-patrol search origin",
    },
    CfgBlock {
        start: 0x005e_b7f6,
        label: "strafe search origin",
    },
    CfgBlock {
        start: 0x005e_b7c0,
        label: "retarget from air-patrol",
    },
    CfgBlock {
        start: 0x005e_b8c1,
        label: "retarget from strafe",
    },
];

/// Host-owned closures which must land before strict row 16 can become executable.
pub const STRAFE_OPEN_TAILS: &[&str] = &[
    "ObjectCaptainAndIdentityAdapter",
    "LeaderAllianceAndTargetValidity",
    "AirTargetSearchCone",
    "WorldProjectionAndRestriction",
    "AirPhysicsAtomicRngAdapter",
    "QueuedAirOrderLeash",
    "RangeBearingAndAmmoAdapter",
    "MissileDeathAndAnimationEffects",
    "ConcreteStrafePayloadSaveResume",
    "LiveTickAtomicCommit",
];
