// SPDX-License-Identifier: GPL-3.0-or-later
//! Mutation pins for the isolated Carrier implicit-queue unqueue transaction.

#[path = "../src/systems/carrier_implicit_unqueue_frontier.rs"]
mod subject;

use subject::*;

const NO_REFUND: CarrierImplicitUnqueueRequest =
    CarrierImplicitUnqueueRequest { refund_cost: false };
const REFUND: CarrierImplicitUnqueueRequest = CarrierImplicitUnqueueRequest { refund_cost: true };

fn encode(value: i32) -> u32 {
    value as u32 ^ RESOURCE_XOR_KEY
}

fn state(num_queued: i16) -> CarrierImplicitQueueState {
    CarrierImplicitQueueState {
        owner: 2,
        carrier: CarrierQueueRow {
            num_queued,
            queue_time: 91,
        },
        aggregate: Some(TypeQueuedCounter {
            type_index: 310,
            value: 4,
        }),
        training: TrainingQueueCounters {
            barracks: 7,
            stable: 8,
            factory: 9,
            combat: 10,
            dock: 11,
            air: 12,
        },
        encoded_resources: [
            encode(100),
            encode(200),
            encode(300),
            encode(400),
            encode(500),
            encode(600),
        ],
        resource_scratch: -77,
    }
}

fn armed(training_site: i32, domain: Option<i32>) -> CarrierImplicitQueueFacts {
    CarrierImplicitQueueFacts {
        current_upgrade: Some(310),
        queue_type: Some(QueueTypeFacts {
            type_index: 310,
            is_unit_type: true,
            object: Some(ObjectQueueFacts {
                attack: 3,
                armed: Some(ArmedUnitQueueFacts {
                    training_site,
                    domain,
                }),
            }),
        }),
        refund: None,
    }
}

fn non_unit() -> CarrierImplicitQueueFacts {
    CarrierImplicitQueueFacts {
        current_upgrade: Some(310),
        queue_type: Some(QueueTypeFacts {
            type_index: 310,
            is_unit_type: false,
            object: None,
        }),
        refund: None,
    }
}

fn refund_goods() -> [RefundGoodFacts; RETAIL_GOODS] {
    [
        RefundGoodFacts {
            available: Some(true),
            cost: Some(7),
        },
        RefundGoodFacts {
            available: Some(false),
            cost: None,
        },
        RefundGoodFacts {
            available: Some(true),
            cost: Some(-5),
        },
        RefundGoodFacts {
            available: Some(false),
            cost: None,
        },
        RefundGoodFacts {
            available: Some(false),
            cost: None,
        },
        RefundGoodFacts {
            available: Some(true),
            cost: Some(i32::MAX),
        },
    ]
}

#[test]
fn retail_symbols_extents_offsets_and_constants_are_pinned() {
    assert_eq!(UNIT_ACTION_UNQUEUE_VA, 0x005e_1f20);
    assert_eq!(UNIT_ACTION_UNQUEUE_BYTES, 389);
    assert_eq!(TYPE_UNPAY_COST_VA, 0x0066_82f0);
    assert_eq!(TYPE_UNPAY_COST_BYTES, 136);
    assert_eq!(LEADER_CURRENT_UPGRADE_VA, 0x006e_3140);
    assert_eq!(HELICOPTER_UPGRADE_BASE, 0x134);
    assert_eq!(UNIT_WHO_OFFSET, 9);
    assert_eq!(UNIT_QUEUE_TIME_OFFSET, 0x64);
    assert_eq!(UNIT_NUM_QUEUED_OFFSET, 0xa0);
    assert_eq!(LEADER_STRIDE, 0x6eec);
    assert_eq!(LEADER_NUM_QUEUED_OFFSET, 0x5a22);
    assert_eq!(OBJECT_TYPE_ATTACK_OFFSET, 0x1e8);
    assert_eq!(OBJECT_TYPE_DOMAIN_OFFSET, 0x218);
}

#[test]
fn empty_queue_returns_before_owner_upgrade_or_refund_reads() {
    let mut before = state(0);
    before.owner = u8::MAX;
    let plan =
        plan_carrier_implicit_unqueue(REFUND, &before, &CarrierImplicitQueueFacts::default())
            .unwrap();
    assert_eq!(plan.boundary, CarrierUnqueueBoundary::EmptyQueue);
    assert_eq!(plan.after, before);
    assert_eq!(
        plan.steps,
        vec![UnqueueStep::ReadCarrierNumQueued { value: 0 }]
    );
}

#[test]
fn last_entry_clears_queue_time_before_current_upgrade_read() {
    let mut before = state(1);
    before.aggregate = None;
    let facts = CarrierImplicitQueueFacts {
        current_upgrade: Some(-1),
        queue_type: None,
        refund: None,
    };
    let plan = plan_carrier_implicit_unqueue(NO_REFUND, &before, &facts).unwrap();
    assert_eq!(plan.after.carrier.num_queued, 0);
    assert_eq!(plan.after.carrier.queue_time, 0);
    assert_eq!(
        plan.steps,
        vec![
            UnqueueStep::ReadCarrierNumQueued { value: 1 },
            UnqueueStep::WriteCarrierNumQueued {
                before: 1,
                after: 0,
            },
            UnqueueStep::WriteQueueTime {
                before: 91,
                after: 0,
            },
            UnqueueStep::ReadCurrentUpgrade {
                base: HELICOPTER_UPGRADE_BASE,
                value: -1,
            },
        ]
    );
}

#[test]
fn signed_carrier_queue_decrement_wraps_and_does_not_clear_time() {
    let mut before = state(i16::MIN);
    before.aggregate = None;
    let facts = CarrierImplicitQueueFacts {
        current_upgrade: Some(-1),
        ..CarrierImplicitQueueFacts::default()
    };
    let plan = plan_carrier_implicit_unqueue(NO_REFUND, &before, &facts).unwrap();
    assert_eq!(plan.after.carrier.num_queued, i16::MAX);
    assert_eq!(plan.after.carrier.queue_time, 91);
}

#[test]
fn barracks_and_stable_each_decrement_the_shared_combat_counter_after_specific_counter() {
    for (site, specific) in [
        (TRAIN_AT_BARRACKS, TrainingQueueKind::Barracks),
        (TRAIN_AT_STABLE, TrainingQueueKind::Stable),
    ] {
        let plan = plan_carrier_implicit_unqueue(NO_REFUND, &state(2), &armed(site, None)).unwrap();
        let writes: Vec<_> = plan
            .steps
            .iter()
            .filter_map(|step| match step {
                UnqueueStep::WriteTrainingQueue { kind, .. } => Some(*kind),
                _ => None,
            })
            .collect();
        assert_eq!(writes, vec![specific, TrainingQueueKind::Combat]);
        assert_eq!(plan.after.training.combat, 9);
    }
}

#[test]
fn factory_dock_and_default_air_are_distinct_single_counter_arms() {
    for (site, domain, kind) in [
        (TRAIN_AT_FACTORY, None, TrainingQueueKind::Factory),
        (TRAIN_AT_DOCK, None, TrainingQueueKind::Dock),
        (0x777, Some(AIR_DOMAIN), TrainingQueueKind::Air),
    ] {
        let plan =
            plan_carrier_implicit_unqueue(NO_REFUND, &state(2), &armed(site, domain)).unwrap();
        let writes: Vec<_> = plan
            .steps
            .iter()
            .filter_map(|step| match step {
                UnqueueStep::WriteTrainingQueue { kind, .. } => Some(*kind),
                _ => None,
            })
            .collect();
        assert_eq!(writes, vec![kind]);
    }

    let plan =
        plan_carrier_implicit_unqueue(NO_REFUND, &state(2), &armed(0x777, Some(AIR_DOMAIN - 1)))
            .unwrap();
    assert!(!plan
        .steps
        .iter()
        .any(|step| matches!(step, UnqueueStep::ReadTrainingQueue { .. })));
}

#[test]
fn non_unit_and_unarmed_unit_stop_before_object_family_reads() {
    let plan = plan_carrier_implicit_unqueue(NO_REFUND, &state(2), &non_unit()).unwrap();
    assert!(!plan
        .steps
        .iter()
        .any(|step| matches!(step, UnqueueStep::ReadAttack { .. })));

    let facts = CarrierImplicitQueueFacts {
        current_upgrade: Some(310),
        queue_type: Some(QueueTypeFacts {
            type_index: 310,
            is_unit_type: true,
            object: Some(ObjectQueueFacts {
                attack: 0,
                armed: None,
            }),
        }),
        refund: None,
    };
    let plan = plan_carrier_implicit_unqueue(NO_REFUND, &state(2), &facts).unwrap();
    assert!(plan
        .steps
        .iter()
        .any(|step| matches!(step, UnqueueStep::ReadAttack { value: 0, .. })));
    assert!(!plan
        .steps
        .iter()
        .any(|step| matches!(step, UnqueueStep::ReadTrainingSite { .. })));
}

#[test]
fn aggregate_and_family_counters_are_saturating_not_wrapping() {
    let mut before = state(2);
    before.aggregate.as_mut().unwrap().value = 0;
    before.training.factory = 0;
    let plan =
        plan_carrier_implicit_unqueue(NO_REFUND, &before, &armed(TRAIN_AT_FACTORY, None)).unwrap();
    assert_eq!(plan.after.aggregate.unwrap().value, 0);
    assert_eq!(plan.after.training.factory, 0);
    assert!(!plan.steps.iter().any(|step| matches!(
        step,
        UnqueueStep::WriteLeaderTypeQueued { .. }
            | UnqueueStep::WriteTrainingQueue {
                kind: TrainingQueueKind::Factory,
                ..
            }
    )));
}

#[test]
fn refund_walks_all_goods_and_publishes_scratch_before_each_encoded_resource() {
    let mut facts = non_unit();
    facts.refund = Some(RefundFacts {
        type_index: 310,
        no_costs_mode: false,
        goods: refund_goods(),
    });
    let before = state(2);
    let plan = plan_carrier_implicit_unqueue(REFUND, &before, &facts).unwrap();
    assert_eq!(plan.boundary, CarrierUnqueueBoundary::AppliedRefund);
    assert_eq!(plan.after.encoded_resources[0], encode(107));
    assert_eq!(plan.after.encoded_resources[1], encode(200));
    assert_eq!(plan.after.encoded_resources[2], encode(295));
    assert_eq!(
        plan.after.encoded_resources[5],
        encode(600i32.wrapping_add(i32::MAX))
    );
    assert_eq!(plan.after.resource_scratch, 600i32.wrapping_add(i32::MAX));

    for good in [0, 2, 5] {
        let scratch = plan.steps.iter().position(
            |step| matches!(step, UnqueueStep::WriteResourceScratch { good: g, .. } if *g == good),
        );
        let resource = plan.steps.iter().position(
            |step| matches!(step, UnqueueStep::WriteEncodedResource { good: g, .. } if *g == good),
        );
        assert!(scratch.unwrap() < resource.unwrap());
    }
    assert_eq!(plan.direct_rng_draws, 0);
}

#[test]
fn no_costs_mode_returns_before_availability_and_cost_facts() {
    let mut facts = non_unit();
    facts.refund = Some(RefundFacts {
        type_index: 310,
        no_costs_mode: true,
        goods: [RefundGoodFacts::default(); RETAIL_GOODS],
    });
    let before = state(2);
    let plan = plan_carrier_implicit_unqueue(REFUND, &before, &facts).unwrap();
    assert_eq!(plan.after.encoded_resources, before.encoded_resources);
    assert_eq!(plan.after.resource_scratch, before.resource_scratch);
    assert!(plan
        .steps
        .contains(&UnqueueStep::ReadNoCostsMode { enabled: true }));
    assert!(!plan
        .steps
        .iter()
        .any(|step| matches!(step, UnqueueStep::ReadGoodAvailability { .. })));
}

#[test]
fn typed_identity_and_lazy_projection_errors_fail_before_a_plan_exists() {
    let mut bad = armed(TRAIN_AT_FACTORY, None);
    bad.queue_type.as_mut().unwrap().type_index = 309;
    assert_eq!(
        plan_carrier_implicit_unqueue(NO_REFUND, &state(2), &bad),
        Err(CarrierUnqueueError::TypeIdentity)
    );

    let mut bad = armed(TRAIN_AT_FACTORY, Some(AIR_DOMAIN));
    assert_eq!(
        plan_carrier_implicit_unqueue(NO_REFUND, &state(2), &bad),
        Err(CarrierUnqueueError::UnexpectedDomainProjection)
    );

    bad = armed(0x777, None);
    assert_eq!(
        plan_carrier_implicit_unqueue(NO_REFUND, &state(2), &bad),
        Err(CarrierUnqueueError::MissingDomainProjection)
    );

    let mut bad = non_unit();
    let mut goods = refund_goods();
    goods[0].cost = None;
    bad.refund = Some(RefundFacts {
        type_index: 310,
        no_costs_mode: false,
        goods,
    });
    assert_eq!(
        plan_carrier_implicit_unqueue(REFUND, &state(2), &bad),
        Err(CarrierUnqueueError::MissingRefundCost)
    );
}

#[test]
fn refund_rejects_negative_upgrade_that_retail_invariant_forbids() {
    let mut before = state(1);
    before.aggregate = None;
    let facts = CarrierImplicitQueueFacts {
        current_upgrade: Some(-1),
        ..CarrierImplicitQueueFacts::default()
    };
    assert_eq!(
        plan_carrier_implicit_unqueue(REFUND, &before, &facts),
        Err(CarrierUnqueueError::UnsafeRefundType)
    );
}

#[test]
fn recomputable_receipt_rejects_request_state_fact_and_trace_mutations() {
    let before = state(2);
    let facts = armed(TRAIN_AT_FACTORY, None);
    let plan = plan_carrier_implicit_unqueue(NO_REFUND, &before, &facts).unwrap();
    let receipt = CarrierImplicitUnqueueReceipt {
        request: NO_REFUND,
        status: CarrierImplicitUnqueueStatus::Complete,
        before: Some(before),
        facts: Some(facts),
        plan: Some(plan),
    };
    assert!(receipt.validates(NO_REFUND));
    assert!(!receipt.validates(REFUND));

    let mut changed = receipt.clone();
    changed.before.as_mut().unwrap().carrier.queue_time ^= 1;
    assert!(!changed.validates(NO_REFUND));
    changed = receipt.clone();
    changed
        .facts
        .as_mut()
        .unwrap()
        .queue_type
        .as_mut()
        .unwrap()
        .object
        .as_mut()
        .unwrap()
        .attack ^= 1;
    assert!(!changed.validates(NO_REFUND));
    changed = receipt.clone();
    changed.plan.as_mut().unwrap().steps.swap(0, 1);
    assert!(!changed.validates(NO_REFUND));

    assert!(CarrierImplicitUnqueueReceipt::unavailable(NO_REFUND).validates(NO_REFUND));
}
