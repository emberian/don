//! Executable pins for the complete `Build::action_unqueue` transaction.

use don_sim::command::direct_entity_command_integration::build_action_unqueue::{
    plan_build_action_unqueue, BuildActionObjectFacts, BuildActionObjectState, BuildActionQueue,
    BuildActionStep, BuildActionUnqueueFacts, BuildActionUnqueueReceipt, BuildActionUnqueueRequest,
    BuildActionUnqueueState, BuildActionUnqueueStatus, BuildQueuedTypeFacts, LibraryScanOutcome,
    RAZING_TYPE, REPEAT_DISABLED_SOUND,
};
use don_sim::command::direct_entity_command_integration::carrier_implicit_unqueue::{
    ArmedUnitQueueFacts, ObjectQueueFacts, TrainingQueueCounters, TrainingQueueKind,
    TRAIN_AT_FACTORY,
};
use don_sim::objects::BUILD_BAND_BASE;
use don_sim::systems::production::{flag, mask, BuildQueueEntry};

const OWNER: u8 = 2;
const UNIT_TYPE: i32 = 50;
const RESEARCH_TYPE: i32 = 0x220;

fn entry(type_index: i32, elapsed: i32, costs: &[(i16, i16)]) -> BuildQueueEntry {
    let mut entry = BuildQueueEntry {
        elapsed,
        type_index: type_index as i16,
        res: [-1; 3],
        ..BuildQueueEntry::default()
    };
    for (slot, &(good, amount)) in costs.iter().enumerate() {
        entry.res[slot] = good;
        entry.amt[slot] = amount;
    }
    entry
}

fn object(types: &[i32]) -> BuildActionObjectState {
    BuildActionObjectState {
        flags: flag::VALID | flag::ACTIVE,
        city: 0,
        build_masks: 0,
        queue: BuildActionQueue {
            queued: types.len() as u8,
            entries: types
                .iter()
                .enumerate()
                .map(|(i, &ty)| entry(ty, 100 + i as i32, &[(0, 10 + i as i16)]))
                .collect(),
        },
    }
}

fn unit_type() -> BuildQueuedTypeFacts {
    BuildQueuedTypeFacts {
        type_index: UNIT_TYPE,
        is_unit_type: true,
        object: Some(ObjectQueueFacts {
            attack: 1,
            armed: Some(ArmedUnitQueueFacts {
                training_site: TRAIN_AT_FACTORY,
                domain: None,
            }),
        }),
    }
}

fn research_type() -> BuildQueuedTypeFacts {
    BuildQueuedTypeFacts {
        type_index: RESEARCH_TYPE,
        is_unit_type: false,
        object: None,
    }
}

fn fixture() -> (BuildActionUnqueueState, BuildActionUnqueueFacts) {
    let mut types = vec![None; 806];
    types[UNIT_TYPE as usize] = Some(unit_type());
    types[RESEARCH_TYPE as usize] = Some(research_type());
    let state = BuildActionUnqueueState {
        owner: OWNER,
        objects: vec![Some(object(&[UNIT_TYPE, UNIT_TYPE, RESEARCH_TYPE]))],
        queued_counts: {
            let mut counters = vec![0; 806];
            counters[UNIT_TYPE as usize] = 7;
            counters[RESEARCH_TYPE as usize] = 1;
            counters
        },
        training: TrainingQueueCounters {
            factory: 4,
            ..TrainingQueueCounters::default()
        },
        ages_queued: 1,
        epochs_queued: 2,
        resources: [100, 200, 300, 400, 500, 600],
        resource_scratch: -77,
        queue_dirty: false,
    };
    let facts = BuildActionUnqueueFacts {
        local_player: OWNER,
        objects: vec![BuildActionObjectFacts {
            unassimilated: Some(false),
            is_library: Some(false),
        }],
        types,
    };
    (state, facts)
}

#[test]
fn one_cancel_selects_the_last_adjacent_match_and_commits_every_owner() {
    let (before, facts) = fixture();
    let request = BuildActionUnqueueRequest {
        object_index: BUILD_BAND_BASE as i16,
        selector: 0,
    };
    let plan = plan_build_action_unqueue(request, &before, &facts).unwrap();
    let queue = &plan.after.objects[0].as_ref().unwrap().queue;

    assert_eq!(queue.queued, 2);
    assert_eq!(
        queue.entries.len(),
        3,
        "allocation remains checksum-visible"
    );
    assert_eq!(queue.entries[0].type_index, UNIT_TYPE as i16);
    assert_eq!(queue.entries[0].elapsed, 100);
    assert_eq!(queue.entries[1].type_index, RESEARCH_TYPE as i16);
    assert_eq!(queue.entries[2].type_index, RESEARCH_TYPE as i16);
    assert_eq!(plan.after.queued_counts[UNIT_TYPE as usize], 6);
    assert_eq!(plan.after.training.factory, 3);
    assert_eq!(plan.after.resources[0], 111);
    assert_eq!(plan.after.resource_scratch, 111);
    assert!(plan.after.queue_dirty);
    assert!(plan.steps.iter().any(|step| matches!(
        step,
        BuildActionStep::CompactQueue {
            removed_slot: 1,
            ..
        }
    )));
    assert!(plan.steps.iter().any(|step| matches!(
        step,
        BuildActionStep::WriteTrainingQueued {
            kind: TrainingQueueKind::Factory,
            before: 4,
            after: 3,
        }
    )));
    assert_eq!(plan.direct_rng_draws, 0);
}

#[test]
fn selector_bands_and_repeat_latch_match_the_instruction_branches() {
    let (mut before, facts) = fixture();
    before.objects[0].as_mut().unwrap().build_masks |= mask::REPEAT_QUEUE;

    let only_disable = plan_build_action_unqueue(
        BuildActionUnqueueRequest {
            object_index: BUILD_BAND_BASE as i16,
            selector: 0,
        },
        &before,
        &facts,
    )
    .unwrap();
    assert_eq!(
        only_disable.after.objects[0].as_ref().unwrap().queue.queued,
        3
    );
    assert_eq!(
        only_disable.after.objects[0].as_ref().unwrap().build_masks & mask::REPEAT_QUEUE,
        0
    );
    assert!(only_disable.steps.iter().any(|step| matches!(
        step,
        BuildActionStep::LocalRepeatDisabledUi { owner: OWNER }
    )));
    assert!(only_disable.steps.iter().any(|step| matches!(
        step,
        BuildActionStep::Sound {
            category: REPEAT_DISABLED_SOUND
        }
    )));

    let last_after_disable = plan_build_action_unqueue(
        BuildActionUnqueueRequest {
            object_index: BUILD_BAND_BASE as i16,
            selector: -2,
        },
        &before,
        &facts,
    )
    .unwrap();
    assert_eq!(
        last_after_disable.after.objects[0]
            .as_ref()
            .unwrap()
            .queue
            .queued,
        2
    );
    assert_eq!(last_after_disable.after.ages_queued, 0);

    let five = plan_build_action_unqueue(
        BuildActionUnqueueRequest {
            object_index: BUILD_BAND_BASE as i16,
            selector: -5,
        },
        &before,
        &facts,
    )
    .unwrap();
    assert_eq!(five.after.objects[0].as_ref().unwrap().queue.queued, 0);

    let all = plan_build_action_unqueue(
        BuildActionUnqueueRequest {
            object_index: BUILD_BAND_BASE as i16,
            selector: -10,
        },
        &before,
        &facts,
    )
    .unwrap();
    assert_eq!(all.after.objects[0].as_ref().unwrap().queue.queued, 0);
}

#[test]
fn ordinary_library_action_routes_to_the_first_assimilated_live_library() {
    let (mut before, mut facts) = fixture();
    before.objects = vec![
        Some(object(&[RESEARCH_TYPE, UNIT_TYPE])),
        Some(object(&[UNIT_TYPE])),
    ];
    facts.objects = vec![
        BuildActionObjectFacts {
            unassimilated: Some(false),
            is_library: Some(true),
        },
        BuildActionObjectFacts {
            unassimilated: Some(false),
            is_library: Some(true),
        },
    ];
    let second = BUILD_BAND_BASE as i16 + 1;
    let plan = plan_build_action_unqueue(
        BuildActionUnqueueRequest {
            object_index: second,
            selector: 0,
        },
        &before,
        &facts,
    )
    .unwrap();

    assert_eq!(plan.after.objects[0].as_ref().unwrap().queue.queued, 1);
    assert_eq!(plan.after.objects[1].as_ref().unwrap().queue.queued, 1);
    assert!(plan.steps.iter().any(|step| matches!(
        step,
        BuildActionStep::RouteToFirstLibrary { from, to }
            if *from == second && *to == BUILD_BAND_BASE as i16
    )));
    assert!(plan.steps.iter().any(|step| matches!(
        step,
        BuildActionStep::ScanFirstLibrary {
            object_index,
            outcome: LibraryScanOutcome::Selected,
        } if *object_index == BUILD_BAND_BASE as i16
    )));
}

#[test]
fn razing_queue_entry_bypasses_the_shared_library_route() {
    let (mut before, mut facts) = fixture();
    before.objects = vec![Some(object(&[UNIT_TYPE])), Some(object(&[RAZING_TYPE]))];
    facts.objects = vec![
        BuildActionObjectFacts {
            unassimilated: Some(false),
            is_library: Some(true),
        },
        BuildActionObjectFacts {
            unassimilated: None,
            is_library: None,
        },
    ];
    facts.types[RAZING_TYPE as usize] = Some(BuildQueuedTypeFacts {
        type_index: RAZING_TYPE,
        is_unit_type: false,
        object: None,
    });
    before.queued_counts[RAZING_TYPE as usize] = 1;
    let second = BUILD_BAND_BASE as i16 + 1;
    let plan = plan_build_action_unqueue(
        BuildActionUnqueueRequest {
            object_index: second,
            selector: 0,
        },
        &before,
        &facts,
    )
    .unwrap();

    assert_eq!(plan.after.objects[0].as_ref().unwrap().queue.queued, 1);
    assert_eq!(plan.after.objects[1].as_ref().unwrap().queue.queued, 0);
    assert_eq!(plan.after.epochs_queued, 1);
    assert!(!plan
        .steps
        .iter()
        .any(|step| matches!(step, BuildActionStep::RouteToFirstLibrary { .. })));
}

#[test]
fn receipt_recomputation_rejects_a_mutated_counter_and_missing_lazy_fact() {
    let (before, facts) = fixture();
    let request = BuildActionUnqueueRequest {
        object_index: BUILD_BAND_BASE as i16,
        selector: -2,
    };
    let plan = plan_build_action_unqueue(request, &before, &facts).unwrap();
    let receipt = BuildActionUnqueueReceipt {
        request,
        status: BuildActionUnqueueStatus::Complete,
        before: Some(before.clone()),
        facts: Some(facts.clone()),
        plan: Some(plan),
    };
    assert!(receipt.validates(request));

    let mut changed = receipt.clone();
    changed.plan.as_mut().unwrap().after.ages_queued = 9;
    assert!(!changed.validates(request));

    let mut missing = facts;
    missing.objects[0].is_library = None;
    assert!(plan_build_action_unqueue(request, &before, &missing).is_err());
    assert_eq!(before.objects[0].as_ref().unwrap().queue.queued, 3);
}
