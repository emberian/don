// SPDX-License-Identifier: GPL-3.0-or-later
//! Source-only transaction recovery for `Unit::action_unqueue` at `0x005E1F20`.
//!
//! Aircraft Carriers own a one-slot implicit queue separate from every `BuildQueue`.  The
//! recovered receiver decrements that queue, the owner's aggregate/type-family counters,
//! and optionally executes the complete `Type::unpay_cost` refund leaf.  This isolated file
//! owns a typed snapshot and a recomputable ordered receipt; it does not register itself in
//! the shared module graph or claim that opcode 48 reaches the transaction in production.

pub const UNIT_ACTION_UNQUEUE_VA: u32 = 0x005e_1f20;
pub const UNIT_ACTION_UNQUEUE_BYTES: usize = 389;
pub const TYPE_UNPAY_COST_VA: u32 = 0x0066_82f0;
pub const TYPE_UNPAY_COST_BYTES: usize = 136;
pub const LEADER_CURRENT_UPGRADE_VA: u32 = 0x006e_3140;

pub const HELICOPTER_UPGRADE_BASE: i32 = 0x134;
pub const RETAIL_LEADER_SLOTS: usize = 8;
pub const RETAIL_TYPE_SLOTS: i32 = 806;
pub const RETAIL_GOODS: usize = 6;
pub const RESOURCE_XOR_KEY: u32 = 0x8221;

pub const UNIT_WHO_OFFSET: u32 = 0x09;
pub const UNIT_QUEUE_TIME_OFFSET: u32 = 0x64;
pub const UNIT_NUM_QUEUED_OFFSET: u32 = 0xa0;
pub const LEADER_STRIDE: u32 = 0x6eec;
pub const LEADER_BARRACKS_QUEUED_OFFSET: u32 = 0x0a10;
pub const LEADER_STABLE_QUEUED_OFFSET: u32 = 0x0a14;
pub const LEADER_FACTORY_QUEUED_OFFSET: u32 = 0x0a18;
pub const LEADER_COMBAT_QUEUED_OFFSET: u32 = 0x0a1c;
pub const LEADER_DOCK_QUEUED_OFFSET: u32 = 0x0a20;
pub const LEADER_AIR_QUEUED_OFFSET: u32 = 0x0a24;
pub const LEADER_NUM_QUEUED_OFFSET: u32 = 0x5a22;
pub const OBJECT_TYPE_ATTACK_OFFSET: u32 = 0x01e8;
pub const OBJECT_TYPE_DOMAIN_OFFSET: u32 = 0x0218;

pub const TRAIN_AT_BARRACKS: i32 = 0x1ab;
pub const TRAIN_AT_STABLE: i32 = 0x1ac;
pub const TRAIN_AT_FACTORY: i32 = 0x1ae;
pub const TRAIN_AT_DOCK: i32 = 0x1b0;
pub const AIR_DOMAIN: i32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CarrierQueueRow {
    /// PDB `UnitData::num_queued`, a signed 16-bit field. Retail decrements it with `dec eax`
    /// followed by a 16-bit store, so the edge wraps exactly.
    pub num_queued: i16,
    pub queue_time: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeQueuedCounter {
    pub type_index: i32,
    pub value: u16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrainingQueueCounters {
    pub barracks: i32,
    pub stable: i32,
    pub factory: i32,
    pub combat: i32,
    pub dock: i32,
    pub air: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CarrierImplicitQueueState {
    pub owner: u8,
    pub carrier: CarrierQueueRow,
    /// Exact selected cell from `LeaderData::num_queued[806]`. It is absent only when
    /// `current_upgrade(HELICOPTER)` returns a negative sentinel.
    pub aggregate: Option<TypeQueuedCounter>,
    pub training: TrainingQueueCounters,
    /// The six encrypted `LeaderData::resources` cells used by `Type::unpay_cost`.
    pub encoded_resources: [u32; RETAIL_GOODS],
    /// Retail publishes each decoded post-refund value through `0x00CB195C` before storing
    /// its re-encoded resource cell. Only the final loop value remains observable.
    pub resource_scratch: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArmedUnitQueueFacts {
    /// PDB `TypeData::where` at `+0x40`.
    pub training_site: i32,
    /// `ObjectTypeData::domain` is read only for the default training-site arm.
    pub domain: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectQueueFacts {
    pub attack: i32,
    /// Retail does not read `where` or `domain` for an unarmed unit.
    pub armed: Option<ArmedUnitQueueFacts>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueTypeFacts {
    pub type_index: i32,
    pub is_unit_type: bool,
    /// The ObjectType projection is reached only after `is_unit_type()` succeeds.
    pub object: Option<ObjectQueueFacts>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RefundGoodFacts {
    /// `LeaderData::type_avail(good, 1)` is not reached in no-cost mode.
    pub available: Option<bool>,
    /// Result of virtual `Type::get_cost(good, who, -1, -1, 1, 1, -1)`.
    pub cost: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RefundFacts {
    pub type_index: i32,
    /// `Game +0x821 & 8`: when set, `Type::unpay_cost` returns before the goods loop.
    pub no_costs_mode: bool,
    pub goods: [RefundGoodFacts; RETAIL_GOODS],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CarrierImplicitQueueFacts {
    /// Result of `LeaderData::current_upgrade(HELICOPTER=0x134)`. It is lazy on the empty
    /// carrier-queue return.
    pub current_upgrade: Option<i32>,
    pub queue_type: Option<QueueTypeFacts>,
    pub refund: Option<RefundFacts>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrainingQueueKind {
    Barracks,
    Stable,
    Factory,
    Combat,
    Dock,
    Air,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnqueueStep {
    ReadCarrierNumQueued {
        value: i16,
    },
    WriteCarrierNumQueued {
        before: i16,
        after: i16,
    },
    WriteQueueTime {
        before: i32,
        after: i32,
    },
    ReadCurrentUpgrade {
        base: i32,
        value: i32,
    },
    ReadLeaderTypeQueued {
        type_index: i32,
        value: u16,
    },
    WriteLeaderTypeQueued {
        type_index: i32,
        before: u16,
        after: u16,
    },
    ReadIsUnitType {
        type_index: i32,
        value: bool,
    },
    ReadAttack {
        type_index: i32,
        value: i32,
    },
    ReadTrainingSite {
        type_index: i32,
        value: i32,
    },
    ReadDomain {
        type_index: i32,
        value: i32,
    },
    ReadTrainingQueue {
        kind: TrainingQueueKind,
        value: i32,
    },
    WriteTrainingQueue {
        kind: TrainingQueueKind,
        before: i32,
        after: i32,
    },
    ReadNoCostsMode {
        enabled: bool,
    },
    ReadGoodAvailability {
        good: usize,
        available: bool,
    },
    ReadRefundCost {
        good: usize,
        value: i32,
    },
    WriteResourceScratch {
        good: usize,
        before: i32,
        after: i32,
    },
    WriteEncodedResource {
        good: usize,
        before: u32,
        after: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CarrierUnqueueBoundary {
    EmptyQueue,
    AppliedNoRefund,
    AppliedRefund,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CarrierImplicitUnqueuePlan {
    pub boundary: CarrierUnqueueBoundary,
    pub after: CarrierImplicitQueueState,
    pub steps: Vec<UnqueueStep>,
    pub direct_rng_draws: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CarrierUnqueueError {
    OwnerOutOfRange,
    MissingCurrentUpgrade,
    UnsafeRefundType,
    TypeOutOfRange,
    UnexpectedTypeProjection,
    MissingTypeProjection,
    TypeIdentity,
    UnexpectedAggregate,
    MissingAggregate,
    AggregateIdentity,
    UnexpectedObjectProjection,
    MissingObjectProjection,
    UnexpectedArmedProjection,
    MissingArmedProjection,
    UnexpectedDomainProjection,
    MissingDomainProjection,
    UnexpectedRefundProjection,
    MissingRefundProjection,
    RefundIdentity,
    UnexpectedGoodProjection,
    MissingAvailability,
    MissingRefundCost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CarrierImplicitUnqueueRequest {
    /// The `int` argument is only tested for zero by retail.
    pub refund_cost: bool,
}

fn validate_projection(
    request: CarrierImplicitUnqueueRequest,
    before: &CarrierImplicitQueueState,
    facts: &CarrierImplicitQueueFacts,
) -> Result<i32, CarrierUnqueueError> {
    if usize::from(before.owner) >= RETAIL_LEADER_SLOTS {
        return Err(CarrierUnqueueError::OwnerOutOfRange);
    }

    let current = facts
        .current_upgrade
        .ok_or(CarrierUnqueueError::MissingCurrentUpgrade)?;
    if current < 0 {
        if request.refund_cost {
            // Retail would index `types[current]` when it reaches `unpay_cost`; the shipped
            // Carrier invariant is that this cannot happen. A safe host must reject it.
            return Err(CarrierUnqueueError::UnsafeRefundType);
        }
        if facts.queue_type.is_some() {
            return Err(CarrierUnqueueError::UnexpectedTypeProjection);
        }
        if before.aggregate.is_some() {
            return Err(CarrierUnqueueError::UnexpectedAggregate);
        }
    } else {
        if current >= RETAIL_TYPE_SLOTS {
            return Err(CarrierUnqueueError::TypeOutOfRange);
        }
        let type_facts = facts
            .queue_type
            .ok_or(CarrierUnqueueError::MissingTypeProjection)?;
        if type_facts.type_index != current {
            return Err(CarrierUnqueueError::TypeIdentity);
        }
        let aggregate = before
            .aggregate
            .ok_or(CarrierUnqueueError::MissingAggregate)?;
        if aggregate.type_index != current {
            return Err(CarrierUnqueueError::AggregateIdentity);
        }

        match (type_facts.is_unit_type, type_facts.object) {
            (false, Some(_)) => return Err(CarrierUnqueueError::UnexpectedObjectProjection),
            (true, None) => return Err(CarrierUnqueueError::MissingObjectProjection),
            (false, None) => {}
            (true, Some(object)) => {
                if object.attack == 0 {
                    if object.armed.is_some() {
                        return Err(CarrierUnqueueError::UnexpectedArmedProjection);
                    }
                } else {
                    let armed = object
                        .armed
                        .ok_or(CarrierUnqueueError::MissingArmedProjection)?;
                    let fixed_site = matches!(
                        armed.training_site,
                        TRAIN_AT_BARRACKS | TRAIN_AT_STABLE | TRAIN_AT_FACTORY | TRAIN_AT_DOCK
                    );
                    match (fixed_site, armed.domain) {
                        (true, Some(_)) => {
                            return Err(CarrierUnqueueError::UnexpectedDomainProjection)
                        }
                        (false, None) => return Err(CarrierUnqueueError::MissingDomainProjection),
                        _ => {}
                    }
                }
            }
        }
    }

    if request.refund_cost {
        let refund = facts
            .refund
            .ok_or(CarrierUnqueueError::MissingRefundProjection)?;
        if refund.type_index != current {
            return Err(CarrierUnqueueError::RefundIdentity);
        }
        for good in refund.goods {
            if refund.no_costs_mode {
                if good != RefundGoodFacts::default() {
                    return Err(CarrierUnqueueError::UnexpectedGoodProjection);
                }
                continue;
            }
            let available = good
                .available
                .ok_or(CarrierUnqueueError::MissingAvailability)?;
            if available {
                if good.cost.is_none() {
                    return Err(CarrierUnqueueError::MissingRefundCost);
                }
            } else if good.cost.is_some() {
                return Err(CarrierUnqueueError::UnexpectedGoodProjection);
            }
        }
    } else if facts.refund.is_some() {
        return Err(CarrierUnqueueError::UnexpectedRefundProjection);
    }

    Ok(current)
}

fn queue_value(training: &TrainingQueueCounters, kind: TrainingQueueKind) -> i32 {
    match kind {
        TrainingQueueKind::Barracks => training.barracks,
        TrainingQueueKind::Stable => training.stable,
        TrainingQueueKind::Factory => training.factory,
        TrainingQueueKind::Combat => training.combat,
        TrainingQueueKind::Dock => training.dock,
        TrainingQueueKind::Air => training.air,
    }
}

fn queue_value_mut(training: &mut TrainingQueueCounters, kind: TrainingQueueKind) -> &mut i32 {
    match kind {
        TrainingQueueKind::Barracks => &mut training.barracks,
        TrainingQueueKind::Stable => &mut training.stable,
        TrainingQueueKind::Factory => &mut training.factory,
        TrainingQueueKind::Combat => &mut training.combat,
        TrainingQueueKind::Dock => &mut training.dock,
        TrainingQueueKind::Air => &mut training.air,
    }
}

fn decrement_training(
    state: &mut CarrierImplicitQueueState,
    kind: TrainingQueueKind,
    steps: &mut Vec<UnqueueStep>,
) {
    let before = queue_value(&state.training, kind);
    steps.push(UnqueueStep::ReadTrainingQueue {
        kind,
        value: before,
    });
    if before != 0 {
        let after = before.wrapping_sub(1);
        *queue_value_mut(&mut state.training, kind) = after;
        steps.push(UnqueueStep::WriteTrainingQueue {
            kind,
            before,
            after,
        });
    }
}

pub fn plan_carrier_implicit_unqueue(
    request: CarrierImplicitUnqueueRequest,
    before: &CarrierImplicitQueueState,
    facts: &CarrierImplicitQueueFacts,
) -> Result<CarrierImplicitUnqueuePlan, CarrierUnqueueError> {
    let mut steps = vec![UnqueueStep::ReadCarrierNumQueued {
        value: before.carrier.num_queued,
    }];
    if before.carrier.num_queued == 0 {
        return Ok(CarrierImplicitUnqueuePlan {
            boundary: CarrierUnqueueBoundary::EmptyQueue,
            after: before.clone(),
            steps,
            direct_rng_draws: 0,
        });
    }

    // Validate every reached typed owner before publishing the first retail write.
    let current = validate_projection(request, before, facts)?;
    let mut after = before.clone();

    let queued_after = before.carrier.num_queued.wrapping_sub(1);
    after.carrier.num_queued = queued_after;
    steps.push(UnqueueStep::WriteCarrierNumQueued {
        before: before.carrier.num_queued,
        after: queued_after,
    });
    if queued_after == 0 {
        let queue_time_before = after.carrier.queue_time;
        after.carrier.queue_time = 0;
        steps.push(UnqueueStep::WriteQueueTime {
            before: queue_time_before,
            after: 0,
        });
    }

    steps.push(UnqueueStep::ReadCurrentUpgrade {
        base: HELICOPTER_UPGRADE_BASE,
        value: current,
    });

    if current >= 0 {
        let aggregate = after.aggregate.as_mut().expect("projection preflight");
        let aggregate_before = aggregate.value;
        steps.push(UnqueueStep::ReadLeaderTypeQueued {
            type_index: current,
            value: aggregate_before,
        });
        if aggregate_before != 0 {
            aggregate.value = aggregate_before - 1;
            steps.push(UnqueueStep::WriteLeaderTypeQueued {
                type_index: current,
                before: aggregate_before,
                after: aggregate.value,
            });
        }

        let queue_type = facts.queue_type.expect("projection preflight");
        steps.push(UnqueueStep::ReadIsUnitType {
            type_index: current,
            value: queue_type.is_unit_type,
        });
        if let Some(object) = queue_type.object {
            steps.push(UnqueueStep::ReadAttack {
                type_index: current,
                value: object.attack,
            });
            if let Some(armed) = object.armed {
                steps.push(UnqueueStep::ReadTrainingSite {
                    type_index: current,
                    value: armed.training_site,
                });
                match armed.training_site {
                    TRAIN_AT_BARRACKS => {
                        decrement_training(&mut after, TrainingQueueKind::Barracks, &mut steps);
                        decrement_training(&mut after, TrainingQueueKind::Combat, &mut steps);
                    }
                    TRAIN_AT_STABLE => {
                        decrement_training(&mut after, TrainingQueueKind::Stable, &mut steps);
                        decrement_training(&mut after, TrainingQueueKind::Combat, &mut steps);
                    }
                    TRAIN_AT_FACTORY => {
                        decrement_training(&mut after, TrainingQueueKind::Factory, &mut steps);
                    }
                    TRAIN_AT_DOCK => {
                        decrement_training(&mut after, TrainingQueueKind::Dock, &mut steps);
                    }
                    _ => {
                        let domain = armed.domain.expect("projection preflight");
                        steps.push(UnqueueStep::ReadDomain {
                            type_index: current,
                            value: domain,
                        });
                        if domain == AIR_DOMAIN {
                            decrement_training(&mut after, TrainingQueueKind::Air, &mut steps);
                        }
                    }
                }
            }
        }
    }

    if request.refund_cost {
        let refund = facts.refund.expect("projection preflight");
        steps.push(UnqueueStep::ReadNoCostsMode {
            enabled: refund.no_costs_mode,
        });
        if !refund.no_costs_mode {
            for (good, good_facts) in refund.goods.into_iter().enumerate() {
                let available = good_facts.available.expect("projection preflight");
                steps.push(UnqueueStep::ReadGoodAvailability { good, available });
                if !available {
                    continue;
                }
                let cost = good_facts.cost.expect("projection preflight");
                steps.push(UnqueueStep::ReadRefundCost { good, value: cost });
                let encoded_before = after.encoded_resources[good];
                let decoded_before = encoded_before ^ RESOURCE_XOR_KEY;
                let decoded_after = decoded_before.wrapping_add(cost as u32);
                let scratch_before = after.resource_scratch;
                after.resource_scratch = decoded_after as i32;
                steps.push(UnqueueStep::WriteResourceScratch {
                    good,
                    before: scratch_before,
                    after: after.resource_scratch,
                });
                let encoded_after = decoded_after ^ RESOURCE_XOR_KEY;
                after.encoded_resources[good] = encoded_after;
                steps.push(UnqueueStep::WriteEncodedResource {
                    good,
                    before: encoded_before,
                    after: encoded_after,
                });
            }
        }
    }

    Ok(CarrierImplicitUnqueuePlan {
        boundary: if request.refund_cost {
            CarrierUnqueueBoundary::AppliedRefund
        } else {
            CarrierUnqueueBoundary::AppliedNoRefund
        },
        after,
        steps,
        direct_rng_draws: 0,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CarrierImplicitUnqueueStatus {
    Complete,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CarrierImplicitUnqueueReceipt {
    pub request: CarrierImplicitUnqueueRequest,
    pub status: CarrierImplicitUnqueueStatus,
    pub before: Option<CarrierImplicitQueueState>,
    pub facts: Option<CarrierImplicitQueueFacts>,
    pub plan: Option<CarrierImplicitUnqueuePlan>,
}

impl CarrierImplicitUnqueueReceipt {
    pub fn unavailable(request: CarrierImplicitUnqueueRequest) -> Self {
        Self {
            request,
            status: CarrierImplicitUnqueueStatus::Unavailable,
            before: None,
            facts: None,
            plan: None,
        }
    }

    pub fn validates(&self, request: CarrierImplicitUnqueueRequest) -> bool {
        if self.request != request {
            return false;
        }
        match self.status {
            CarrierImplicitUnqueueStatus::Unavailable => {
                self.before.is_none() && self.facts.is_none() && self.plan.is_none()
            }
            CarrierImplicitUnqueueStatus::Complete => {
                let (Some(before), Some(facts), Some(plan)) =
                    (&self.before, &self.facts, &self.plan)
                else {
                    return false;
                };
                plan_carrier_implicit_unqueue(request, before, facts).as_ref() == Ok(plan)
            }
        }
    }
}
