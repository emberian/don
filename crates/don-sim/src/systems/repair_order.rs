//! Deterministic `Unit::do_repair` (`0x005EE420`) planner.
//!
//! This module owns no world storage.  It reproduces retail's branch and arithmetic
//! decisions from a frozen host snapshot and returns the ordered mutations which an
//! atomic host receipt must commit.  See `docs/assembly/repair-order.md`.

pub const UNIT_DO_REPAIR_VA: u32 = 0x005e_e420;
pub const REPAIR_ANIM: i32 = 0x22;
pub const REPAIR_ORDER_INDEX: i32 = 13;
pub const KOREAN_REPAIR_BONUS: i32 = 0x10;
pub const UNIVERSITY_TYPE_INDEX: i32 = 0x1a4;
pub const RESOURCE_COUNT: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectId {
    pub o: i32,
    pub who: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RepairResourceFacts {
    /// Result of `LeaderData::type_avail(resource, 1)`.
    pub type_available: bool,
    /// Result of the target type's vslot `+0x78`; zero suppresses charging.
    pub cost_basis: i32,
    /// Decoded (`^ 0x8221`) authoritative resource stock.
    pub stock: i32,
    /// The parallel `0x00e3a7f8` resource accumulator.
    pub secondary_stock: i32,
}

impl Default for RepairResourceFacts {
    fn default() -> Self {
        Self {
            type_available: false,
            cost_basis: 0,
            stock: 0,
            secondary_stock: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LostTargetFallbackFacts {
    pub repairer_order_type: i32,
    /// Result of the repairer's vslot `+0xf8`.
    pub repairer_state_f8: i32,
    /// Target still answers object vslot `+0x1c` after the current order is killed.
    pub target_has_object_interface: bool,
    /// Target type vslot `+0x90` is nonzero.
    pub target_type_allows_gather: bool,
    /// Result of `TypeData::is(UNIVERSITY, 0)`.
    pub target_is_university: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepairFacts {
    pub repairer: ObjectId,
    pub target: ObjectId,
    /// RepairOrder flag byte bit `0x04` (also passed as `more_work`).
    pub more_work: bool,
    pub repairer_unit_masks: u32,

    pub target_damage: i32,
    pub target_is_repairer_team: bool,
    pub repairer_relation_to_target_is_two: bool,
    pub team_relation_to_target_is_two: bool,
    pub target_has_object_interface: bool,
    pub target_build_active: bool,
    pub target_under_attack: bool,
    /// Signed terrain-owner byte; `None` is retail's negative/no-owner case.
    pub territory_owner: Option<i32>,
    pub target_owner_allied_with_territory: bool,
    pub repairer_in_range: bool,

    /// The second target vslot `+0x1c` query, after range admission.
    pub target_has_repair_interface_after_range: bool,
    /// Target build vslot `+0x18c(1)`.
    pub repair_numerator: i32,
    /// Target build vslot `+0x11c(0)`.
    pub target_hit_capacity: i32,
    /// Constants field `+0x22c`.
    pub repair_scale_constant: i32,
    pub target_helpers: u8,
    /// BuildData byte `+8`; bit `0x20` enables the city penalty branch.
    pub target_build_flags: u8,
    /// City bytes `+0x5f` and `+0x5e` differ.
    pub city_repair_state_mismatch: bool,
    pub leader_has_korean_repair_bonus: bool,
    /// Constants field `+0x7e0`.
    pub korean_repair_percent: i32,
    /// Constants field `+0x7dc` is nonzero.
    pub korean_skips_damage_penalties: bool,
    /// BuildData byte `+0x60`; bit `0x10` applies a second x4 penalty.
    pub target_build_masks: u8,

    pub frame: i32,
    /// Target object vslot `+0x114`, used by repair-cost bucket crossing.
    pub target_repair_state: i32,
    pub resources: [RepairResourceFacts; RESOURCE_COUNT],
    pub leader_repair_stamp: i32,
    pub repairer_owner_is_local_player: bool,
    pub lost_target_fallback: LostTargetFallbackFacts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepairStopReason {
    LostTarget,
    TargetHasNoObjectInterface,
    TargetBuildInactive,
    TargetUnderAttackWithoutMoreWork,
    HostileTerritory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepairOutcome {
    Stopped(RepairStopReason),
    RequeuedAroundTarget,
    WaitingOnUnitMask,
    NoRepairQuantum,
    InsufficientResource { resource: u8 },
    Repaired { amount: i32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepairEffect {
    SetAnimation {
        animation: i32,
        arg0: i32,
        arg1: i32,
    },
    KillCurrentOrder {
        arg: i32,
    },
    FindRepairSpot,
    AddGatherOrder {
        target: ObjectId,
        queue_pos: i32,
        arg: i32,
    },
    SwarmAround {
        target: ObjectId,
        queue_pos: i32,
        order_index: i32,
        more_work: i32,
    },
    SetTargetHelpers {
        value: u8,
    },
    SetLeaderRepairStamp {
        frame: i32,
    },
    LocalInsufficientResourceFeedback,
    DebitResource {
        resource: u8,
        cost: i32,
        stock_after: i32,
        secondary_after: i32,
    },
    RepairDamage {
        target: ObjectId,
        amount: i32,
        arg0: i32,
        arg1: i32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepairPlan {
    pub outcome: RepairOutcome,
    pub divisor: Option<i32>,
    pub resource_costs: [i32; RESOURCE_COUNT],
    pub effects: Vec<RepairEffect>,
}

/// Hardware faults which valid retail data is expected not to provoke.  Refusing the
/// snapshot is more faithful than inventing a result or panicking inside a batch tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepairPlanError {
    RetailUnsignedDivideByZero,
}

fn base_effects() -> Vec<RepairEffect> {
    vec![RepairEffect::SetAnimation {
        animation: REPAIR_ANIM,
        arg0: 0,
        arg1: 1,
    }]
}

fn stopped(reason: RepairStopReason, effects: Vec<RepairEffect>) -> RepairPlan {
    RepairPlan {
        outcome: RepairOutcome::Stopped(reason),
        divisor: None,
        resource_costs: [0; RESOURCE_COUNT],
        effects,
    }
}

fn append_lost_target_fallback(facts: &RepairFacts, effects: &mut Vec<RepairEffect>) {
    effects.push(RepairEffect::KillCurrentOrder { arg: 0 });
    if facts.repairer_unit_masks & 0x40000 != 0 {
        effects.push(RepairEffect::FindRepairSpot);
        return;
    }
    let fallback = facts.lost_target_fallback;
    if facts.target.who == facts.repairer.who
        && fallback.repairer_order_type == 0
        && fallback.repairer_state_f8 < 2
        && fallback.target_has_object_interface
        && fallback.target_type_allows_gather
        && !fallback.target_is_university
    {
        effects.push(RepairEffect::AddGatherOrder {
            target: facts.target,
            queue_pos: 2,
            arg: 0,
        });
    }
}

fn cvttss2si(value: f32) -> i32 {
    // SSE's indefinite integer for NaN, infinity, and an out-of-range conversion.
    if !value.is_finite() || value < -2_147_483_648.0_f32 || value >= 2_147_483_648.0_f32 {
        i32::MIN
    } else {
        value.trunc() as i32
    }
}

fn resource_cost(state: i32, hit_capacity: i32, amount: i32, basis: i32) -> i32 {
    if basis == 0 {
        return 0;
    }
    let scale = hit_capacity as f32 / basis as f32;
    let before = cvttss2si(state as f32 / scale);
    let after = cvttss2si(state.wrapping_add(amount) as f32 / scale);
    i32::from(before < after)
}

fn calculate_divisor(facts: &RepairFacts) -> Result<i32, RepairPlanError> {
    if !facts.target_has_repair_interface_after_range {
        return Ok(0x100);
    }

    let numerator = (facts.repair_numerator as u32).wrapping_shl(9);
    let denominator =
        (facts.target_hit_capacity as u32).wrapping_mul(facts.repair_scale_constant as u32);
    if denominator == 0 {
        return Err(RepairPlanError::RetailUnsignedDivideByZero);
    }
    let quotient = numerator / denominator;
    let mut divisor = quotient.wrapping_mul(u32::from(facts.target_helpers) + 1) as i32;

    if facts.target_build_flags & 0x20 != 0 {
        divisor = divisor.wrapping_mul(2);
        if facts.city_repair_state_mismatch {
            divisor = divisor.wrapping_mul(2);
        }
    }

    if facts.leader_has_korean_repair_bonus && facts.korean_repair_percent != 0 {
        divisor = 100_i32
            .wrapping_sub(facts.korean_repair_percent)
            .wrapping_mul(divisor)
            / 100;
    }

    if !facts.leader_has_korean_repair_bonus || !facts.korean_skips_damage_penalties {
        if facts.target_under_attack {
            divisor = divisor.wrapping_shl(2);
        }
        if facts.target_build_masks & 0x10 != 0 {
            divisor = divisor.wrapping_shl(2);
        }
    }
    Ok(divisor)
}

/// Reproduce one retail `Unit::do_repair` call from a complete, internally consistent
/// snapshot.  `effects` are ordered and must be committed atomically by the caller.
pub fn plan_repair(facts: &RepairFacts) -> Result<RepairPlan, RepairPlanError> {
    let mut effects = base_effects();

    let diplomacy_ok = facts.target_is_repairer_team
        || (facts.repairer_relation_to_target_is_two && facts.team_relation_to_target_is_two);
    if facts.target_damage == 0 || !diplomacy_ok {
        append_lost_target_fallback(facts, &mut effects);
        return Ok(stopped(RepairStopReason::LostTarget, effects));
    }
    if !facts.target_has_object_interface {
        effects.push(RepairEffect::KillCurrentOrder { arg: 0 });
        return Ok(stopped(
            RepairStopReason::TargetHasNoObjectInterface,
            effects,
        ));
    }
    if !facts.target_build_active {
        effects.push(RepairEffect::KillCurrentOrder { arg: 0 });
        return Ok(stopped(RepairStopReason::TargetBuildInactive, effects));
    }
    if facts.target_under_attack && !facts.more_work {
        effects.push(RepairEffect::KillCurrentOrder { arg: 0 });
        return Ok(stopped(
            RepairStopReason::TargetUnderAttackWithoutMoreWork,
            effects,
        ));
    }
    if let Some(owner) = facts.territory_owner {
        if owner >= 0 && owner != facts.target.who && !facts.target_owner_allied_with_territory {
            effects.push(RepairEffect::KillCurrentOrder { arg: 0 });
            return Ok(stopped(RepairStopReason::HostileTerritory, effects));
        }
    }
    if !facts.repairer_in_range {
        effects.push(RepairEffect::KillCurrentOrder { arg: 0 });
        effects.push(RepairEffect::SwarmAround {
            target: facts.target,
            queue_pos: 0,
            order_index: REPAIR_ORDER_INDEX,
            // Retail passes the masked byte itself, not a normalized boolean.
            more_work: i32::from(facts.more_work) * 4,
        });
        return Ok(RepairPlan {
            outcome: RepairOutcome::RequeuedAroundTarget,
            divisor: None,
            resource_costs: [0; RESOURCE_COUNT],
            effects,
        });
    }
    if facts.repairer_unit_masks & 1 != 0 {
        return Ok(RepairPlan {
            outcome: RepairOutcome::WaitingOnUnitMask,
            divisor: None,
            resource_costs: [0; RESOURCE_COUNT],
            effects,
        });
    }

    let divisor = calculate_divisor(facts)?;
    if facts.target_has_repair_interface_after_range {
        effects.push(RepairEffect::SetTargetHelpers {
            value: facts.target_helpers.wrapping_add(1),
        });
    }
    let amount = if divisor < 1 {
        facts.target_damage
    } else {
        let frame256 = facts.frame.wrapping_shl(8);
        frame256 / divisor - frame256.wrapping_sub(0x100) / divisor
    };
    if amount == 0 {
        return Ok(RepairPlan {
            outcome: RepairOutcome::NoRepairQuantum,
            divisor: Some(divisor),
            resource_costs: [0; RESOURCE_COUNT],
            effects,
        });
    }

    let mut costs = [0; RESOURCE_COUNT];
    for (resource, entry) in facts.resources.iter().enumerate() {
        if entry.type_available {
            costs[resource] = resource_cost(
                facts.target_repair_state,
                facts.target_hit_capacity,
                amount,
                entry.cost_basis,
            );
            if entry.stock < costs[resource] {
                effects.push(RepairEffect::KillCurrentOrder { arg: 0 });
                if facts.frame.wrapping_sub(facts.leader_repair_stamp) >= 0x97 {
                    effects.push(RepairEffect::SetLeaderRepairStamp { frame: facts.frame });
                    if facts.repairer_owner_is_local_player {
                        effects.push(RepairEffect::LocalInsufficientResourceFeedback);
                    }
                }
                return Ok(RepairPlan {
                    outcome: RepairOutcome::InsufficientResource {
                        resource: resource as u8,
                    },
                    divisor: Some(divisor),
                    resource_costs: costs,
                    effects,
                });
            }
        }
    }

    for (resource, entry) in facts.resources.iter().enumerate() {
        if entry.type_available {
            let cost = costs[resource];
            effects.push(RepairEffect::DebitResource {
                resource: resource as u8,
                cost,
                stock_after: entry.stock.wrapping_sub(cost),
                secondary_after: entry.secondary_stock.saturating_sub(cost).max(0),
            });
        }
    }
    effects.push(RepairEffect::RepairDamage {
        target: facts.target,
        amount,
        arg0: 0,
        arg1: 1,
    });
    Ok(RepairPlan {
        outcome: RepairOutcome::Repaired { amount },
        divisor: Some(divisor),
        resource_costs: costs,
        effects,
    })
}

/// The live adapter may claim REPAIR complete only when one callback validates the same
/// entity versions used to build `RepairFacts` and applies the full effect slice without
/// interleaving.  This marker gives dispatcher integration a typed receipt boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtomicRepairReceipt {
    pub actor: ObjectId,
    pub target: ObjectId,
    pub committed_effects: usize,
}

pub trait AtomicRepairHost {
    type Error;

    fn commit_repair(
        &mut self,
        facts: &RepairFacts,
        effects: &[RepairEffect],
    ) -> Result<AtomicRepairReceipt, Self::Error>;
}

pub fn execute_repair<H: AtomicRepairHost>(
    host: &mut H,
    facts: &RepairFacts,
) -> Result<(RepairPlan, AtomicRepairReceipt), RepairExecutionError<H::Error>> {
    let plan = plan_repair(facts).map_err(RepairExecutionError::Plan)?;
    let receipt = host
        .commit_repair(facts, &plan.effects)
        .map_err(RepairExecutionError::Host)?;
    Ok((plan, receipt))
}

#[derive(Debug, PartialEq, Eq)]
pub enum RepairExecutionError<E> {
    Plan(RepairPlanError),
    Host(E),
}
