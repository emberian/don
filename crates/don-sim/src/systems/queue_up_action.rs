//! Executable ordinary-Unit cohort of `Group::action_queue_up` (`0x006FDBB0`).
//!
//! The group receiver copies and stably sorts the selected Build objects by
//! `BuildData::queued`, executes `Group::action_begin`, then calls
//! `Build::queue_up(type, 1)` in that fixed order for each requested repetition.  This
//! planner owns that deterministic body and the reached `can_pay -> could_queue -> queue`
//! decisions.  Research/build requests, scenario ignore-orders pruning, Library routing,
//! and producer/type pairs without an exact installed compatibility projection fail closed.

use crate::systems::groups_guys::{GroupData, GROUP_MAX_MEMBERS};

pub const GROUP_ACTION_QUEUE_UP_ADDRESS: u32 = 0x006f_dbb0;
pub const GROUP_ACTION_QUEUE_UP_SIZE: u32 = 1_516;
pub const BUILD_QUEUE_UP_ADDRESS: u32 = 0x0062_0f40;
pub const BUILD_QUEUE_UP_SIZE: u32 = 4_322;
pub const QUEUE_GOODS: usize = 6;

/// Fail-closed product guard for a corrupt replay count. Retail has no comparable guard.
pub const MAX_QUEUE_UP_ATTEMPTS: usize = 1_000_000;

#[derive(Clone, Debug, PartialEq)]
pub struct QueueUpActionRequest {
    pub group: GroupData,
    pub type_index: i32,
    pub num: i32,
    pub ignore_orders: bool,
    pub ignore_orders_prune_committed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueUpTrainingFamily {
    Barracks,
    Stable,
    Factory,
    Dock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueUpTypeFacts {
    pub is_unit_type: bool,
    pub available: bool,
    /// Exact six-good result charged by `TypeData`'s `+0xD4` virtual call.
    pub cost: [i32; QUEUE_GOODS],
    /// Reached armed-unit queued-family counter.
    pub training_family: QueueUpTrainingFamily,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueUpProducerFacts {
    pub object_index: i16,
    pub build_row: usize,
    /// Conjunction of the three Group-side receiver gates before `get_build()`.
    pub receiver_reached: bool,
    /// Exact `ObjectType::can_queue(type, 1)` answer inside `BuildData::could_queue`.
    pub can_queue_requested: bool,
    pub queued: u8,
    pub allocated: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueUpFacts {
    /// Lazy: absent for the exact non-building early return.
    pub type_facts: Option<QueueUpTypeFacts>,
    pub resources_before: [i32; QUEUE_GOODS],
    /// One row per live group member, in the original group-list order.
    pub producers: Vec<QueueUpProducerFacts>,
}

impl Default for QueueUpFacts {
    fn default() -> Self {
        Self {
            type_facts: None,
            resources_before: [0; QUEUE_GOODS],
            producers: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueUpAttemptDisposition {
    ReceiverRejected,
    CannotPay,
    CouldNotQueue,
    Enqueued { slot: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueUpAttempt {
    pub repetition: i32,
    pub object_index: i16,
    pub build_row: usize,
    pub disposition: QueueUpAttemptDisposition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueUpProducerAfter {
    pub object_index: i16,
    pub build_row: usize,
    pub queued: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QueueUpPlan {
    pub group: GroupData,
    pub sorted_members: Vec<i16>,
    pub attempts: Vec<QueueUpAttempt>,
    pub producers_after: Vec<QueueUpProducerAfter>,
    pub resources_after: [i32; QUEUE_GOODS],
    pub enqueued: u32,
}

/// Recompute the admitted queue-up body without mutating a world owner.
pub fn plan_queue_up_action(
    request: &QueueUpActionRequest,
    facts: &QueueUpFacts,
) -> Option<QueueUpPlan> {
    if request.ignore_orders || request.ignore_orders_prune_committed {
        return None;
    }
    if request.group.buildings == 0 {
        return (*facts == QueueUpFacts::default()).then(|| QueueUpPlan {
            group: request.group.clone(),
            sorted_members: Vec::new(),
            attempts: Vec::new(),
            producers_after: Vec::new(),
            resources_after: facts.resources_before,
            enqueued: 0,
        });
    }

    let n = usize::try_from(request.group.num).ok()?;
    if n > GROUP_MAX_MEMBERS || facts.producers.len() != n {
        return None;
    }
    let members = request.group.list.get(..n)?;
    if members
        .iter()
        .zip(&facts.producers)
        .any(|(&member, producer)| member != producer.object_index)
    {
        return None;
    }
    let type_facts = facts.type_facts?;
    if !type_facts.is_unit_type
        || !type_facts.available
        || type_facts.cost.iter().any(|&amount| amount < 0)
    {
        return None;
    }
    if facts
        .producers
        .iter()
        .any(|producer| producer.allocated > usize::from(u8::MAX))
    {
        return None;
    }

    let repetitions = request.num.max(0) as usize;
    let attempt_count = repetitions.checked_mul(n)?;
    if attempt_count > MAX_QUEUE_UP_ATTEMPTS {
        return None;
    }

    let mut sorted_indices: Vec<usize> = (0..n).collect();
    // Stable sort is load-bearing: retail swaps only when the left queued byte is greater.
    sorted_indices.sort_by_key(|&index| facts.producers[index].queued);
    let sorted_members = sorted_indices
        .iter()
        .map(|&index| facts.producers[index].object_index)
        .collect();
    let mut queued: Vec<u8> = facts
        .producers
        .iter()
        .map(|producer| producer.queued)
        .collect();
    let mut resources = facts.resources_before;
    let mut attempts = Vec::with_capacity(attempt_count);
    let mut enqueued = 0u32;

    for repetition in 0..request.num.max(0) {
        for &index in &sorted_indices {
            let producer = facts.producers[index];
            let disposition = if !producer.receiver_reached {
                QueueUpAttemptDisposition::ReceiverRejected
            } else if type_facts
                .cost
                .iter()
                .enumerate()
                .any(|(good, &amount)| resources[good] < amount)
            {
                QueueUpAttemptDisposition::CannotPay
            } else if !producer.can_queue_requested
                || usize::from(queued[index]) >= producer.allocated
            {
                QueueUpAttemptDisposition::CouldNotQueue
            } else {
                let slot = queued[index];
                queued[index] = queued[index].checked_add(1)?;
                for (good, amount) in type_facts.cost.into_iter().enumerate() {
                    resources[good] = resources[good].wrapping_sub(amount);
                }
                enqueued = enqueued.wrapping_add(1);
                QueueUpAttemptDisposition::Enqueued { slot }
            };
            attempts.push(QueueUpAttempt {
                repetition,
                object_index: producer.object_index,
                build_row: producer.build_row,
                disposition,
            });
        }
    }

    let mut group = request.group.clone();
    group.disband = 0;
    Some(QueueUpPlan {
        group,
        sorted_members,
        attempts,
        producers_after: facts
            .producers
            .iter()
            .zip(queued)
            .map(|(producer, queued)| QueueUpProducerAfter {
                object_index: producer.object_index,
                build_row: producer.build_row,
                queued,
            })
            .collect(),
        resources_after: resources,
        enqueued,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueUpTransactionStatus {
    Applied,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QueueUpActionReceipt {
    pub request: QueueUpActionRequest,
    pub status: QueueUpTransactionStatus,
    pub facts: Option<QueueUpFacts>,
    pub plan: Option<QueueUpPlan>,
}

impl QueueUpActionReceipt {
    pub fn unavailable(request: QueueUpActionRequest) -> Self {
        Self {
            request,
            status: QueueUpTransactionStatus::Unavailable,
            facts: None,
            plan: None,
        }
    }

    pub fn validates(&self, expected: &QueueUpActionRequest) -> bool {
        if &self.request != expected {
            return false;
        }
        match self.status {
            QueueUpTransactionStatus::Unavailable => self.facts.is_none() && self.plan.is_none(),
            QueueUpTransactionStatus::Applied => {
                let (Some(facts), Some(plan)) = (&self.facts, &self.plan) else {
                    return false;
                };
                plan_queue_up_action(expected, facts).is_some_and(|expected| expected == *plan)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_sort_and_fixed_repetition_order_are_recomputable() {
        let mut group = GroupData {
            num: 3,
            buildings: 1,
            who: 2,
            disband: 99,
            ..GroupData::default()
        };
        group.list[..3].copy_from_slice(&[2000, 2001, 2002]);
        let request = QueueUpActionRequest {
            group,
            type_index: 51,
            num: 2,
            ignore_orders: false,
            ignore_orders_prune_committed: false,
        };
        let facts = QueueUpFacts {
            type_facts: Some(QueueUpTypeFacts {
                is_unit_type: true,
                available: true,
                cost: [10, 0, 0, 0, 0, 0],
                training_family: QueueUpTrainingFamily::Factory,
            }),
            resources_before: [35, 0, 0, 0, 0, 0],
            producers: vec![
                QueueUpProducerFacts {
                    object_index: 2000,
                    build_row: 0,
                    receiver_reached: true,
                    can_queue_requested: true,
                    queued: 2,
                    allocated: 4,
                },
                QueueUpProducerFacts {
                    object_index: 2001,
                    build_row: 1,
                    receiver_reached: true,
                    can_queue_requested: true,
                    queued: 0,
                    allocated: 4,
                },
                QueueUpProducerFacts {
                    object_index: 2002,
                    build_row: 2,
                    receiver_reached: true,
                    can_queue_requested: true,
                    queued: 0,
                    allocated: 4,
                },
            ],
        };
        let plan = plan_queue_up_action(&request, &facts).unwrap();
        assert_eq!(plan.sorted_members, [2001, 2002, 2000]);
        assert_eq!(plan.enqueued, 3);
        assert_eq!(plan.resources_after[0], 5);
        assert_eq!(plan.producers_after[0].queued, 3);
        assert_eq!(plan.producers_after[1].queued, 1);
        assert_eq!(plan.producers_after[2].queued, 1);
        assert_eq!(plan.group.disband, 0);
    }
}
