// SPDX-License-Identifier: GPL-3.0-or-later
//! Source-only reconstruction frontier for `Unit::do_cast`.
//!
//! This file is deliberately not registered in `systems/mod.rs`. It freezes the shipped
//! `CastOrder` payload, the executor's branch-sensitive effects, and the host observations
//! required for an eventual atomic adapter without claiming live dispatcher ownership.

pub const CAST_ORDER_INDEX: i32 = 14;
pub const UNIT_ADD_CAST_ORDER_VA: u32 = 0x005e_4a60;
pub const UNIT_DO_CAST_VA: u32 = 0x005e_bfe0;
pub const UNIT_DO_CAST_BYTES: usize = 4_191;
pub const UNIT_DO_CAST_END_VA: u32 = 0x005e_d03f;
pub const CAST_ORDER_SIZE: usize = 48;
pub const CAST_ORDER_WALKED_BYTES: usize = 27;
pub const SPELL_TYPE_FIRST: i32 = 0x275;
pub const SPELL_TYPE_END_EXCLUSIVE: i32 = 0x2ac;
pub const NON_SPELL_SENTINEL: i32 = 0x296;
pub const SPELL_TRANSPORT: i32 = 0x28a;
pub const SPELL_PACK_BASE: i32 = 0x28b;
pub const SPELL_DEPLOY_BASE: i32 = 0x28c;
pub const SPELL_BRIBE: i32 = 0x275;
pub const SPELL_RALLY: i32 = 0x279;
pub const SPELL_AMBUSH: i32 = 0x27b;
pub const SPELL_FORCED_MARCH: i32 = 0x27c;
pub const SPELL_INFORMER: i32 = 0x27f;
pub const SPELL_SABOTAGE: i32 = 0x280;
pub const SPELL_SNIPER: i32 = 0x281;
pub const SPECIAL_ANIM_GENERAL: i32 = 0x162;

pub mod offsets {
    pub const OX: usize = 0x08;
    pub const WHOM: usize = 0x0c;
    pub const UID: usize = 0x10;
    pub const X: usize = 0x14;
    pub const Y: usize = 0x18;
    pub const PAID: usize = 0x1c;
    pub const SPELL: usize = 0x20;
    pub const UNIT_ORDER_VBASE: usize = 0x28;
    pub const FLAGS: usize = 0x2c;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CfgBlock {
    pub start: u32,
    pub role: &'static str,
}

/// Semantic block heads covering every state-writing cone in the 4,191-byte PDB procedure.
/// Leaf blocks inside compiler-generated virtual-call fast paths share their enclosing role.
pub const CAST_REACHABLE_CFG: &[CfgBlock] = &[
    CfgBlock {
        start: 0x005e_bfe0,
        role: "entry/get-cast-order",
    },
    CfgBlock {
        start: 0x005e_c03c,
        role: "derive-effective-spell",
    },
    CfgBlock {
        start: 0x005e_c07a,
        role: "paid-cost-gate",
    },
    CfgBlock {
        start: 0x005e_c0a2,
        role: "cost-failure-feedback",
    },
    CfgBlock {
        start: 0x005e_c0fd,
        role: "set-paid",
    },
    CfgBlock {
        start: 0x005e_c107,
        role: "target-domain-split",
    },
    CfgBlock {
        start: 0x005e_c135,
        role: "resolve-target-or-inside",
    },
    CfgBlock {
        start: 0x005e_c1a9,
        role: "repair-target-identity",
    },
    CfgBlock {
        start: 0x005e_c20d,
        role: "validate-target",
    },
    CfgBlock {
        start: 0x005e_c23b,
        role: "bribe-alliance-gate",
    },
    CfgBlock {
        start: 0x005e_c324,
        role: "publish-held-target",
    },
    CfgBlock {
        start: 0x005e_c363,
        role: "select-coordinate-domain",
    },
    CfgBlock {
        start: 0x005e_c418,
        role: "range-query",
    },
    CfgBlock {
        start: 0x005e_c442,
        role: "out-of-range-test",
    },
    CfgBlock {
        start: 0x005e_c47d,
        role: "nearby-spot-request",
    },
    CfgBlock {
        start: 0x005e_c50e,
        role: "install-approach-move",
    },
    CfgBlock {
        start: 0x005e_c546,
        role: "cross-owner-visibility",
    },
    CfgBlock {
        start: 0x005e_c611,
        role: "face-target",
    },
    CfgBlock {
        start: 0x005e_c659,
        role: "select-targeted-animation",
    },
    CfgBlock {
        start: 0x005e_c75e,
        role: "one-shot-presentation",
    },
    CfgBlock {
        start: 0x005e_c8db,
        role: "cloak-and-spell-flags",
    },
    CfgBlock {
        start: 0x005e_c949,
        role: "targeted-job-timer",
    },
    CfgBlock {
        start: 0x005e_c9b7,
        role: "targeted-cast",
    },
    CfgBlock {
        start: 0x005e_c9f5,
        role: "untargeted-first-frame",
    },
    CfgBlock {
        start: 0x005e_ca62,
        role: "rare-collector-reposition",
    },
    CfgBlock {
        start: 0x005e_cb3a,
        role: "pack-deploy-animation",
    },
    CfgBlock {
        start: 0x005e_cbfd,
        role: "ordinary-untargeted-animation",
    },
    CfgBlock {
        start: 0x005e_cc69,
        role: "transport-nearby-spot",
    },
    CfgBlock {
        start: 0x005e_ccde,
        role: "general-timer-coupling",
    },
    CfgBlock {
        start: 0x005e_cd91,
        role: "untargeted-job-timer",
    },
    CfgBlock {
        start: 0x005e_cdc7,
        role: "transport-completion-hold",
    },
    CfgBlock {
        start: 0x005e_ce37,
        role: "untargeted-cast",
    },
    CfgBlock {
        start: 0x005e_ce93,
        role: "clear-paid",
    },
    CfgBlock {
        start: 0x005e_ce9d,
        role: "kill-current",
    },
    CfgBlock {
        start: 0x005e_cebf,
        role: "non-spell-completion-effects",
    },
    CfgBlock {
        start: 0x005e_cf7d,
        role: "transport-garrison-transfer",
    },
    CfgBlock {
        start: 0x005e_d03a,
        role: "common-return",
    },
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ObjectIdentity {
    pub o: i32,
    pub who: i32,
    pub uid: u16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CastOrderState {
    pub target: ObjectIdentity,
    pub x: i32,
    pub y: i32,
    pub paid: i32,
    pub spell: i32,
    pub flags: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorSnapshot {
    pub identity: ObjectIdentity,
    pub x: i32,
    pub y: i32,
    pub spell_time: i16,
    pub held_o: i16,
    pub held_who: i8,
    pub held_uid: u16,
    pub state_68: u32,
    pub state_6c: u32,
    pub seen_players: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PayResult {
    Accepted,
    Rejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PayObservation {
    AlreadyPaid,
    Attempted {
        result: PayResult,
        /// Retail emits cost feedback only for the locally controlled leader.
        local_feedback: bool,
        mutation_digest: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetResolution {
    Missing,
    Original(ObjectIdentity),
    /// `ObjectData::get_inside` selected another live object and all three target identity
    /// fields are rewritten before validation.
    Repaired {
        from: ObjectIdentity,
        to: ObjectIdentity,
    },
}

impl TargetResolution {
    fn resolved(self) -> Option<ObjectIdentity> {
        match self {
            Self::Missing => None,
            Self::Original(v) => Some(v),
            Self::Repaired { to, .. } => Some(to),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoordinateDomain {
    LiveTarget { x: i32, y: i32 },
    OrderPoint { x: i32, y: i32 },
}

impl CoordinateDomain {
    fn point(self) -> (i32, i32) {
        match self {
            Self::LiveTarget { x, y } | Self::OrderPoint { x, y } => (x, y),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NearbySpotObservation {
    NotNeeded,
    /// Nonzero return from `UnitType::find_nearby_spot`; retail returns immediately.
    Failed,
    /// Zero return wrote a candidate. Retail installs MOVE only when the candidate is now
    /// within the same `range + margin` boundary.
    Found {
        x: i32,
        y: i32,
        distance_to_target: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RangeCone {
    /// `SpellTypeData::get_range` returned nonzero. A zero return skips the whole
    /// distance/nearby-spot cone even when the actor is not at the target point.
    pub enabled: bool,
    pub range: i32,
    pub margin: i32,
    pub actor_distance: i32,
    pub nearby: NearbySpotObservation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentationCone {
    AlreadyLatched,
    GenericEvent,
    LocalTargetMessage { sound: i32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelEffects {
    pub anim: i32,
    pub anim_loop: i32,
    pub anim_force: i32,
    pub face_angle: Option<u32>,
    pub set_seen_player_bit: Option<u8>,
    pub mark_visible_state: bool,
    pub presentation: PresentationCone,
    pub set_cloak_bits: bool,
    pub set_bribe_bits: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobClock {
    pub before: i16,
    pub required: i16,
    /// Some untargeted general/pack arms increment once before the common increment.
    pub general_extra_increment: bool,
}

impl JobClock {
    fn after(self) -> i16 {
        self.before
            .wrapping_add(1)
            .wrapping_add(self.general_extra_increment as i16)
    }

    fn complete(self) -> bool {
        self.after() >= self.required
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetChannel {
    pub resolution: TargetResolution,
    pub valid_target: bool,
    pub bribe_alliance_ok: bool,
    pub coordinates: CoordinateDomain,
    pub range: RangeCone,
    pub effects: ChannelEffects,
    pub clock: JobClock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UntargetedOpening {
    AlreadyOpened,
    /// Rare-collector or merchant validation failed on the first frame. Retail may publish
    /// local feedback, then kills the order without clearing its paid latch.
    Rejected {
        local_feedback: bool,
    },
    /// Complete first-frame state-writing cone after the type-specific predicates have passed.
    Prepare {
        set_rare_collector_global: bool,
        set_merchant_latch: bool,
        update_group_piece: bool,
        reposition: Option<(i32, i32)>,
        face_angle: Option<u32>,
        anim: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportCone {
    NotTransport,
    /// A failed transport nearby-spot call kills the order while retaining the paid latch.
    SearchFailed,
    SearchFound {
        x: i32,
        y: i32,
    },
    /// A transport frame after the opening search but before job-time.
    Waiting,
    /// At job-time, a still-active CAST order decrements the timer and returns.
    CompletionHold,
    /// Job-time has passed the order-type hold and calls the real transport spell. Retail
    /// then returns without the ordinary clear-paid/kill tail.
    CompletionCast,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NonSpellCone {
    NotNonSpell,
    NoTransfer,
    /// The non-SpellType tail owns `find_building_at`, `go_inside`, and `come_out` before
    /// reaching the ordinary clear-paid/kill tail.
    Transfer {
        building: ObjectIdentity,
        accepts_transport: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UntargetedChannel {
    pub opening: UntargetedOpening,
    pub transport: TransportCone,
    pub non_spell: NonSpellCone,
    pub clock: JobClock,
    pub cast_is_real_spell: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastCone {
    Targeted(TargetChannel),
    Untargeted(UntargetedChannel),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CastFrameFacts {
    pub actor: ActorSnapshot,
    pub order: CastOrderState,
    pub spell_is_real: bool,
    /// Exact `SpellType+0x1C8` value used by the target/coordinate split.
    pub spell_flags: u32,
    pub pay: PayObservation,
    /// Absent exactly when cost payment failed and retail returned before the domain split.
    pub cone: Option<CastCone>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastStep {
    PayCosts { spell: i32, mutation_digest: u64 },
    CostFeedback,
    OpeningFeedback,
    StorePaid(i32),
    StoreOrderTarget(ObjectIdentity),
    ValidateTarget(ObjectIdentity),
    StoreHeldTarget(ObjectIdentity),
    FindNearbySpot { target_x: i32, target_y: i32 },
    AddMoveOrder { x: i32, y: i32 },
    Reposition { x: i32, y: i32 },
    SetRareCollectorGlobal,
    SetMerchantLatch,
    UpdateGroupPiece,
    SetSeenPlayerBit(u8),
    MarkVisibleState,
    SetAngle(u32),
    SetAnim { anim: i32, looped: i32, force: i32 },
    Presentation(PresentationCone),
    SetCloakBits,
    SetBribeBits,
    IncrementContainedTimers,
    StoreSpellTime(i16),
    CastTarget { spell: i32, target: ObjectIdentity },
    CastPoint { spell: i32, x: i32, y: i32 },
    NonSpellTransfer { building: ObjectIdentity },
    StorePaidZero,
    KillCurrent,
    Hold,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CastPlan {
    pub effective_spell: i32,
    pub steps: Vec<CastStep>,
    pub terminal: CastTerminal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastTerminal {
    Returned,
    Waiting,
    MoveInstalled,
    CastAndRetired,
    TransportRetained,
    KilledPaidRetained,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastPlanError {
    PaymentObservationMismatch,
    MissingCone,
    UnexpectedConeAfterRejectedPayment,
    DomainMismatch,
    ResolutionMismatch,
    InvalidRangeReceipt,
    ClockMismatch,
    OpeningMismatch,
    TransportMismatch,
    NonSpellMismatch,
    CoordinateDomainMismatch,
    RealSpellMismatch,
}

pub fn effective_spell(order_spell: i32, spell_is_real: bool) -> i32 {
    if spell_is_real {
        order_spell
    } else {
        NON_SPELL_SENTINEL
    }
}

pub fn is_targeted_domain(effective: i32, spell_flags: u32) -> bool {
    effective != NON_SPELL_SENTINEL && spell_flags & 0x0e != 0
}

/// Exact `Unit::add_cast_order` canonicalization for the two generic pack/deploy IDs.
pub fn normalize_pack_deploy(
    spell: i32,
    has_attr_0x7b: bool,
    object_type: i32,
    has_attr_0x13d: bool,
) -> i32 {
    let special_object = matches!(object_type, 0x3d | 0x3e | 0x190);
    match spell {
        SPELL_PACK_BASE if has_attr_0x7b => 0x28d,
        SPELL_PACK_BASE if special_object => 0x28f,
        SPELL_PACK_BASE if has_attr_0x13d => 0x291,
        SPELL_DEPLOY_BASE if has_attr_0x7b => 0x28e,
        SPELL_DEPLOY_BASE if special_object => 0x290,
        SPELL_DEPLOY_BASE if has_attr_0x13d => 0x292,
        _ => spell,
    }
}

fn push_effects(steps: &mut Vec<CastStep>, effects: ChannelEffects) {
    if let Some(bit) = effects.set_seen_player_bit {
        steps.push(CastStep::SetSeenPlayerBit(bit));
    }
    if effects.mark_visible_state {
        steps.push(CastStep::MarkVisibleState);
    }
    if let Some(angle) = effects.face_angle {
        steps.push(CastStep::SetAngle(angle));
    }
    steps.push(CastStep::SetAnim {
        anim: effects.anim,
        looped: effects.anim_loop,
        force: effects.anim_force,
    });
    if effects.presentation != PresentationCone::AlreadyLatched {
        steps.push(CastStep::Presentation(effects.presentation));
    }
    if effects.set_cloak_bits {
        steps.push(CastStep::SetCloakBits);
    }
    if effects.set_bribe_bits {
        steps.push(CastStep::SetBribeBits);
    }
}

fn plan_targeted(
    facts: &CastFrameFacts,
    effective: i32,
    channel: TargetChannel,
    mut steps: Vec<CastStep>,
) -> Result<CastPlan, CastPlanError> {
    let Some(target) = channel.resolution.resolved() else {
        steps.push(CastStep::KillCurrent);
        return Ok(CastPlan {
            effective_spell: effective,
            steps,
            terminal: CastTerminal::Returned,
        });
    };
    match channel.resolution {
        TargetResolution::Original(v) if v != facts.order.target => {
            return Err(CastPlanError::ResolutionMismatch);
        }
        TargetResolution::Repaired { from, to } if from != facts.order.target || from == to => {
            return Err(CastPlanError::ResolutionMismatch);
        }
        TargetResolution::Repaired { to, .. } => steps.push(CastStep::StoreOrderTarget(to)),
        _ => {}
    }
    steps.push(CastStep::ValidateTarget(target));
    if !channel.valid_target || (effective == SPELL_BRIBE && !channel.bribe_alliance_ok) {
        steps.push(CastStep::KillCurrent);
        return Ok(CastPlan {
            effective_spell: effective,
            steps,
            terminal: CastTerminal::Returned,
        });
    }
    steps.push(CastStep::StoreHeldTarget(target));

    let (target_x, target_y) = channel.coordinates.point();
    match channel.coordinates {
        CoordinateDomain::LiveTarget { .. } if facts.spell_flags & 8 != 0 => {
            return Err(CastPlanError::CoordinateDomainMismatch);
        }
        CoordinateDomain::OrderPoint { x, y }
            if facts.spell_flags & 8 == 0 || x != facts.order.x || y != facts.order.y =>
        {
            return Err(CastPlanError::CoordinateDomainMismatch);
        }
        _ => {}
    }
    let reach = channel.range.range.saturating_add(channel.range.margin);
    if channel.range.range < 0 || channel.range.margin < 0 {
        return Err(CastPlanError::InvalidRangeReceipt);
    }
    if !channel.range.enabled {
        if channel.range.nearby != NearbySpotObservation::NotNeeded {
            return Err(CastPlanError::InvalidRangeReceipt);
        }
    } else if channel.range.actor_distance > reach {
        steps.push(CastStep::FindNearbySpot { target_x, target_y });
        match channel.range.nearby {
            NearbySpotObservation::Failed => {
                steps.push(CastStep::Hold);
                return Ok(CastPlan {
                    effective_spell: effective,
                    steps,
                    terminal: CastTerminal::Returned,
                });
            }
            NearbySpotObservation::Found {
                x,
                y,
                distance_to_target,
            } if distance_to_target <= reach => {
                steps.push(CastStep::AddMoveOrder { x, y });
                return Ok(CastPlan {
                    effective_spell: effective,
                    steps,
                    terminal: CastTerminal::MoveInstalled,
                });
            }
            NearbySpotObservation::Found { .. } => {
                steps.push(CastStep::Hold);
                return Ok(CastPlan {
                    effective_spell: effective,
                    steps,
                    terminal: CastTerminal::Returned,
                });
            }
            NearbySpotObservation::NotNeeded => return Err(CastPlanError::InvalidRangeReceipt),
        }
    } else if channel.range.nearby != NearbySpotObservation::NotNeeded {
        return Err(CastPlanError::InvalidRangeReceipt);
    }

    if channel.clock.before != facts.actor.spell_time {
        return Err(CastPlanError::ClockMismatch);
    }
    push_effects(&mut steps, channel.effects);
    let after = channel.clock.after();
    steps.push(CastStep::StoreSpellTime(after));
    if !channel.clock.complete() {
        steps.push(CastStep::Hold);
        return Ok(CastPlan {
            effective_spell: effective,
            steps,
            terminal: CastTerminal::Waiting,
        });
    }
    steps.push(CastStep::StoreSpellTime(0));
    match channel.coordinates {
        CoordinateDomain::LiveTarget { .. } => {
            steps.push(CastStep::CastTarget {
                spell: effective,
                target,
            });
        }
        CoordinateDomain::OrderPoint { x, y } => {
            steps.push(CastStep::CastPoint {
                spell: effective,
                x,
                y,
            });
        }
    }
    steps.push(CastStep::StorePaidZero);
    steps.push(CastStep::KillCurrent);
    Ok(CastPlan {
        effective_spell: effective,
        steps,
        terminal: CastTerminal::CastAndRetired,
    })
}

fn plan_untargeted(
    facts: &CastFrameFacts,
    effective: i32,
    channel: UntargetedChannel,
    mut steps: Vec<CastStep>,
) -> Result<CastPlan, CastPlanError> {
    if channel.clock.before != facts.actor.spell_time {
        return Err(CastPlanError::ClockMismatch);
    }
    if channel.cast_is_real_spell != facts.spell_is_real {
        return Err(CastPlanError::RealSpellMismatch);
    }
    if (channel.cast_is_real_spell && channel.non_spell != NonSpellCone::NotNonSpell)
        || (!channel.cast_is_real_spell && channel.non_spell == NonSpellCone::NotNonSpell)
    {
        return Err(CastPlanError::NonSpellMismatch);
    }
    if (channel.clock.before == 0) == matches!(channel.opening, UntargetedOpening::AlreadyOpened) {
        return Err(CastPlanError::OpeningMismatch);
    }
    match channel.opening {
        UntargetedOpening::AlreadyOpened => {}
        UntargetedOpening::Rejected { local_feedback } => {
            if local_feedback {
                steps.push(CastStep::OpeningFeedback);
            }
            steps.push(CastStep::KillCurrent);
            return Ok(CastPlan {
                effective_spell: effective,
                steps,
                terminal: CastTerminal::KilledPaidRetained,
            });
        }
        UntargetedOpening::Prepare {
            set_rare_collector_global,
            set_merchant_latch,
            update_group_piece,
            reposition,
            face_angle,
            anim,
        } => {
            if set_rare_collector_global {
                steps.push(CastStep::SetRareCollectorGlobal);
            }
            if set_merchant_latch {
                steps.push(CastStep::SetMerchantLatch);
            }
            if update_group_piece {
                steps.push(CastStep::UpdateGroupPiece);
            }
            if let Some((x, y)) = reposition {
                steps.push(CastStep::Reposition { x, y });
            }
            if let Some(angle) = face_angle {
                steps.push(CastStep::SetAngle(angle));
            }
            steps.push(CastStep::SetAnim {
                anim,
                looped: 0,
                force: 1,
            });
        }
    }

    let transport_spell = facts.order.spell == SPELL_TRANSPORT && facts.spell_is_real;
    match channel.transport {
        TransportCone::NotTransport if transport_spell => {
            return Err(CastPlanError::TransportMismatch)
        }
        TransportCone::SearchFailed
            if effective == SPELL_TRANSPORT && channel.clock.before == 0 =>
        {
            steps.push(CastStep::FindNearbySpot {
                target_x: facts.order.x,
                target_y: facts.order.y,
            });
            steps.push(CastStep::KillCurrent);
            return Ok(CastPlan {
                effective_spell: effective,
                steps,
                terminal: CastTerminal::KilledPaidRetained,
            });
        }
        TransportCone::SearchFound { x, y }
            if effective == SPELL_TRANSPORT && channel.clock.before == 0 =>
        {
            steps.push(CastStep::FindNearbySpot {
                target_x: facts.order.x,
                target_y: facts.order.y,
            });
            let _ = (x, y);
        }
        TransportCone::Waiting
            if transport_spell && channel.clock.before != 0 && !channel.clock.complete() => {}
        TransportCone::CompletionHold
            if effective == SPELL_TRANSPORT && channel.clock.complete() => {}
        TransportCone::CompletionCast if transport_spell && channel.clock.complete() => {}
        TransportCone::NotTransport => {}
        _ => return Err(CastPlanError::TransportMismatch),
    }

    if channel.clock.general_extra_increment {
        steps.push(CastStep::IncrementContainedTimers);
    }
    let after = channel.clock.after();
    steps.push(CastStep::StoreSpellTime(after));
    if !channel.clock.complete() {
        steps.push(CastStep::Hold);
        return Ok(CastPlan {
            effective_spell: effective,
            steps,
            terminal: CastTerminal::Waiting,
        });
    }
    if channel.transport == TransportCone::CompletionHold {
        steps.push(CastStep::StoreSpellTime(after.wrapping_sub(1)));
        steps.push(CastStep::Hold);
        return Ok(CastPlan {
            effective_spell: effective,
            steps,
            terminal: CastTerminal::TransportRetained,
        });
    }
    steps.push(CastStep::StoreSpellTime(0));
    if channel.cast_is_real_spell {
        steps.push(CastStep::CastPoint {
            spell: facts.order.spell,
            x: 0,
            y: 0,
        });
    }
    if channel.transport == TransportCone::CompletionCast {
        steps.push(CastStep::Hold);
        return Ok(CastPlan {
            effective_spell: effective,
            steps,
            terminal: CastTerminal::TransportRetained,
        });
    }
    match channel.non_spell {
        NonSpellCone::NotNonSpell if !channel.cast_is_real_spell => {
            return Err(CastPlanError::NonSpellMismatch);
        }
        NonSpellCone::NoTransfer if channel.cast_is_real_spell => {
            return Err(CastPlanError::NonSpellMismatch);
        }
        NonSpellCone::Transfer { .. } if channel.cast_is_real_spell => {
            return Err(CastPlanError::NonSpellMismatch);
        }
        NonSpellCone::Transfer {
            building,
            accepts_transport,
        } => {
            if accepts_transport {
                steps.push(CastStep::NonSpellTransfer { building });
            }
        }
        NonSpellCone::NotNonSpell | NonSpellCone::NoTransfer => {}
    }
    steps.push(CastStep::StorePaidZero);
    steps.push(CastStep::KillCurrent);
    Ok(CastPlan {
        effective_spell: effective,
        steps,
        terminal: CastTerminal::CastAndRetired,
    })
}

pub fn plan_cast_frame(facts: &CastFrameFacts) -> Result<CastPlan, CastPlanError> {
    let effective = effective_spell(facts.order.spell, facts.spell_is_real);
    let mut steps = Vec::new();
    match facts.pay {
        PayObservation::AlreadyPaid if facts.order.paid == 0 => {
            return Err(CastPlanError::PaymentObservationMismatch);
        }
        PayObservation::AlreadyPaid => {}
        PayObservation::Attempted { .. } if facts.order.paid != 0 => {
            return Err(CastPlanError::PaymentObservationMismatch);
        }
        PayObservation::Attempted {
            result,
            local_feedback,
            mutation_digest,
        } => {
            steps.push(CastStep::PayCosts {
                spell: facts.order.spell,
                mutation_digest,
            });
            if result == PayResult::Rejected {
                if local_feedback {
                    steps.push(CastStep::CostFeedback);
                }
                steps.push(CastStep::KillCurrent);
                if facts.cone.is_some() {
                    return Err(CastPlanError::UnexpectedConeAfterRejectedPayment);
                }
                return Ok(CastPlan {
                    effective_spell: effective,
                    steps,
                    terminal: CastTerminal::Returned,
                });
            }
            steps.push(CastStep::StorePaid(1));
        }
    }
    let cone = facts.cone.ok_or(CastPlanError::MissingCone)?;
    if is_targeted_domain(effective, facts.spell_flags) != matches!(cone, CastCone::Targeted(_)) {
        return Err(CastPlanError::DomainMismatch);
    }
    match cone {
        CastCone::Targeted(v) => plan_targeted(facts, effective, v, steps),
        CastCone::Untargeted(v) => plan_untargeted(facts, effective, v, steps),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CastPreflight {
    pub actor_version: u64,
    pub order_version: u64,
    pub object_epoch: u64,
    pub leader_epoch: u64,
    pub spell_epoch: u64,
    pub world_epoch: u64,
    pub queue_digest: u64,
    pub effect_epoch: u64,
    pub facts: CastFrameFacts,
    pub plan: CastPlan,
}

pub fn preflight_cast(
    actor_version: u64,
    order_version: u64,
    object_epoch: u64,
    leader_epoch: u64,
    spell_epoch: u64,
    world_epoch: u64,
    queue_digest: u64,
    effect_epoch: u64,
    facts: CastFrameFacts,
) -> Result<CastPreflight, CastPlanError> {
    let plan = plan_cast_frame(&facts)?;
    Ok(CastPreflight {
        actor_version,
        order_version,
        object_epoch,
        leader_epoch,
        spell_epoch,
        world_epoch,
        queue_digest,
        effect_epoch,
        facts,
        plan,
    })
}

pub fn preflight_still_valid(before: &CastPreflight, after: &CastPreflight) -> bool {
    before == after && plan_cast_frame(&after.facts).ok().as_ref() == Some(&after.plan)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastOpenTail {
    ConcretePayloadSaveResume,
    SpellTypeAndFlagAdapter,
    CostTransaction,
    TargetAndInsideResolution,
    TargetValidityAndAlliance,
    RangeAndNearbySpot,
    VisibilityFacingAndAnimation,
    PresentationEffects,
    CloakAndActorFlags,
    TimerAndContainedObjectCoupling,
    CastEffectTransaction,
    NonSpellBuildingTransfer,
    DispatcherAtomicCommit,
}

pub const CAST_OPEN_TAILS: &[CastOpenTail] = &[
    CastOpenTail::ConcretePayloadSaveResume,
    CastOpenTail::SpellTypeAndFlagAdapter,
    CastOpenTail::CostTransaction,
    CastOpenTail::TargetAndInsideResolution,
    CastOpenTail::TargetValidityAndAlliance,
    CastOpenTail::RangeAndNearbySpot,
    CastOpenTail::VisibilityFacingAndAnimation,
    CastOpenTail::PresentationEffects,
    CastOpenTail::CloakAndActorFlags,
    CastOpenTail::TimerAndContainedObjectCoupling,
    CastOpenTail::CastEffectTransaction,
    CastOpenTail::NonSpellBuildingTransfer,
    CastOpenTail::DispatcherAtomicCommit,
];
