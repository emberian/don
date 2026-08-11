// SPDX-License-Identifier: GPL-3.0-or-later
//! Complete state transaction for `Build::action_unqueue` at `0x00620280`.
//!
//! The command-facing action is more than a queue pop. It resolves the shared Library
//! queue, owns the repeat-queue latch and its local presentation, selects one/five/all
//! cancellation modes from the signed wire selector, and invokes the complete virtual
//! `Build::unqueue(slot, 1)` body for every selected record. The snapshot below keeps the
//! player-major Build band and every external counter/refund owner in one recomputable
//! transaction so a host can preflight before publishing the first write.

use super::carrier_implicit_unqueue::{
    ObjectQueueFacts, TrainingQueueCounters, TrainingQueueKind, AIR_DOMAIN, RETAIL_GOODS,
    RETAIL_LEADER_SLOTS, RETAIL_TYPE_SLOTS, TRAIN_AT_BARRACKS, TRAIN_AT_DOCK, TRAIN_AT_FACTORY,
    TRAIN_AT_STABLE,
};
use crate::objects::BUILD_BAND_BASE;
use crate::systems::production::{mask, BuildQueueEntry};

pub const BUILD_ACTION_UNQUEUE_VA: u32 = 0x0062_0280;
pub const BUILD_ACTION_UNQUEUE_BYTES: usize = 521;
pub const BUILD_UNQUEUE_VA: u32 = 0x0062_07c0;
pub const BUILD_UNQUEUE_BYTES: usize = 915;
pub const FIRST_LIBRARY_VA: u32 = 0x006d_b6c0;
pub const RAZING_TYPE: i32 = 0x29a;
pub const LIBRARY_TYPE_CLASS: i32 = 0x1b3;
pub const REPEAT_DISABLED_SOUND: i32 = 0x3d;
pub const FIRST_TECH_TYPE: i32 = 0x220;
pub const LAST_TECH_TYPE: i32 = 0x274;
pub const FIRST_SPELL_TYPE: i32 = 0x275;
pub const LAST_SPELL_TYPE: i32 = 0x2ab;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildActionQueue {
    pub queued: u8,
    /// Physical allocation. Retail compacts the logical prefix but never shrinks this.
    pub entries: Vec<BuildQueueEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildActionObjectState {
    pub flags: u8,
    pub city: i16,
    pub build_masks: u16,
    pub queue: BuildActionQueue,
}

impl BuildActionObjectState {
    fn valid(&self) -> bool {
        self.flags & 1 != 0
    }

    fn active(&self) -> bool {
        self.flags & 4 != 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildActionUnqueueState {
    pub owner: u8,
    /// Band-relative slots `0..objects.counts[owner]-2000`; holes remain `None`.
    pub objects: Vec<Option<BuildActionObjectState>>,
    /// Canonical `LeaderData::num_queued[806]` projection.
    pub queued_counts: Vec<i32>,
    pub training: TrainingQueueCounters,
    pub ages_queued: u8,
    pub epochs_queued: u8,
    pub resources: [i32; RETAIL_GOODS],
    pub resource_scratch: i32,
    pub queue_dirty: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BuildActionObjectFacts {
    /// Exact `BuildData::is_unassimilated` result. Read only for a valid, active object
    /// attached to a city while scanning `get_first_library`.
    pub unassimilated: Option<bool>,
    /// Exact non-strict `ObjectData::is(LIBRARY, 0)` result. Read for the receiver and for
    /// a scan candidate only after its assimilation gate succeeds.
    pub is_library: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildQueuedTypeFacts {
    pub type_index: i32,
    pub is_unit_type: bool,
    /// Reached only for Unit types. `attack == 0` suppresses training-site/domain reads.
    pub object: Option<ObjectQueueFacts>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildActionUnqueueFacts {
    pub local_player: u8,
    pub objects: Vec<BuildActionObjectFacts>,
    pub types: Vec<Option<BuildQueuedTypeFacts>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildActionUnqueueRequest {
    pub object_index: i16,
    pub selector: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LibraryScanOutcome {
    Missing,
    Invalid,
    Inactive,
    NoCity,
    Unassimilated,
    NotLibrary,
    Selected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildActionStep {
    InspectRazing {
        object_index: i16,
        selector: i32,
        value: bool,
    },
    ReadIsLibrary {
        object_index: i16,
        value: bool,
    },
    ScanFirstLibrary {
        object_index: i16,
        outcome: LibraryScanOutcome,
    },
    RouteToFirstLibrary {
        from: i16,
        to: i16,
    },
    ReadQueueLength {
        object_index: i16,
        value: u8,
    },
    ClearRepeatLatch {
        object_index: i16,
        before: u16,
        after: u16,
    },
    LocalRepeatDisabledUi {
        owner: u8,
    },
    Sound {
        category: i32,
    },
    CallUnqueue {
        source: i16,
        slot: i32,
        refund: bool,
    },
    RouteUnqueue {
        from: i16,
        to: i16,
        source_slot: i32,
        target_slot: i32,
    },
    WriteQueueDirty {
        before: bool,
        after: bool,
    },
    WriteElapsedZero {
        object_index: i16,
        slot: usize,
        before: i32,
    },
    WriteTypeQueued {
        type_index: i32,
        before: i32,
        after: i32,
    },
    WriteTrainingQueued {
        kind: TrainingQueueKind,
        before: i32,
        after: i32,
    },
    WriteAgesQueued {
        before: u8,
        after: u8,
    },
    WriteEpochsQueued {
        before: u8,
        after: u8,
    },
    WriteResource {
        queue_cost_slot: usize,
        good: usize,
        amount: i16,
        before: i32,
        after: i32,
    },
    WriteResourceScratch {
        before: i32,
        after: i32,
    },
    CompactQueue {
        object_index: i16,
        removed_slot: usize,
        queued_before: u8,
        queued_after: u8,
    },
    ClearEmptyRepeatLatch {
        object_index: i16,
        before: u16,
        after: u16,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildActionUnqueuePlan {
    pub after: BuildActionUnqueueState,
    pub steps: Vec<BuildActionStep>,
    pub direct_rng_draws: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildActionUnqueueError {
    OwnerOutOfRange,
    ObjectOutsideBand,
    MissingObject,
    ObjectFactShape,
    MissingLibraryFact(i16),
    MissingAssimilationFact(i16),
    LogicalLengthExceedsAllocation(i16),
    NegativeForwardedSlot,
    TypeOutOfRange(i32),
    MissingTypeFact(i32),
    TypeIdentity(i32),
    MissingObjectTypeFact(i32),
    UnexpectedObjectTypeFact(i32),
    MissingArmedTypeFact(i32),
    UnexpectedArmedTypeFact(i32),
    MissingDomainTypeFact(i32),
    UnexpectedDomainTypeFact(i32),
    CounterOutOfRange(i32),
    ResourceOutOfRange(i16),
    CancellationMadeNoProgress,
}

fn band_slot(object_index: i16) -> Result<usize, BuildActionUnqueueError> {
    let object_index =
        usize::try_from(object_index).map_err(|_| BuildActionUnqueueError::ObjectOutsideBand)?;
    object_index
        .checked_sub(BUILD_BAND_BASE as usize)
        .ok_or(BuildActionUnqueueError::ObjectOutsideBand)
}

fn object(
    state: &BuildActionUnqueueState,
    object_index: i16,
) -> Result<&BuildActionObjectState, BuildActionUnqueueError> {
    state
        .objects
        .get(band_slot(object_index)?)
        .and_then(Option::as_ref)
        .ok_or(BuildActionUnqueueError::MissingObject)
}

fn object_mut(
    state: &mut BuildActionUnqueueState,
    object_index: i16,
) -> Result<&mut BuildActionObjectState, BuildActionUnqueueError> {
    state
        .objects
        .get_mut(band_slot(object_index)?)
        .and_then(Option::as_mut)
        .ok_or(BuildActionUnqueueError::MissingObject)
}

fn object_facts(
    facts: &BuildActionUnqueueFacts,
    object_index: i16,
) -> Result<BuildActionObjectFacts, BuildActionUnqueueError> {
    facts
        .objects
        .get(band_slot(object_index)?)
        .copied()
        .ok_or(BuildActionUnqueueError::ObjectFactShape)
}

fn read_is_library(
    facts: &BuildActionUnqueueFacts,
    object_index: i16,
    steps: &mut Vec<BuildActionStep>,
) -> Result<bool, BuildActionUnqueueError> {
    let value = object_facts(facts, object_index)?
        .is_library
        .ok_or(BuildActionUnqueueError::MissingLibraryFact(object_index))?;
    steps.push(BuildActionStep::ReadIsLibrary {
        object_index,
        value,
    });
    Ok(value)
}

fn first_library(
    state: &BuildActionUnqueueState,
    facts: &BuildActionUnqueueFacts,
    steps: &mut Vec<BuildActionStep>,
) -> Result<Option<i16>, BuildActionUnqueueError> {
    for (slot, candidate) in state.objects.iter().enumerate() {
        let object_index = i16::try_from(BUILD_BAND_BASE as usize + slot)
            .map_err(|_| BuildActionUnqueueError::ObjectOutsideBand)?;
        let Some(candidate) = candidate else {
            steps.push(BuildActionStep::ScanFirstLibrary {
                object_index,
                outcome: LibraryScanOutcome::Missing,
            });
            continue;
        };
        let outcome = if !candidate.valid() {
            LibraryScanOutcome::Invalid
        } else if !candidate.active() {
            LibraryScanOutcome::Inactive
        } else if candidate.city < 0 {
            LibraryScanOutcome::NoCity
        } else {
            let candidate_facts = object_facts(facts, object_index)?;
            if candidate_facts.unassimilated.ok_or(
                BuildActionUnqueueError::MissingAssimilationFact(object_index),
            )? {
                LibraryScanOutcome::Unassimilated
            } else if !candidate_facts
                .is_library
                .ok_or(BuildActionUnqueueError::MissingLibraryFact(object_index))?
            {
                LibraryScanOutcome::NotLibrary
            } else {
                LibraryScanOutcome::Selected
            }
        };
        steps.push(BuildActionStep::ScanFirstLibrary {
            object_index,
            outcome,
        });
        if outcome == LibraryScanOutcome::Selected {
            return Ok(Some(object_index));
        }
    }
    Ok(None)
}

fn action_razing(object: &BuildActionObjectState, selector: i32) -> bool {
    if object.queue.queued == 0 || selector >= i32::from(object.queue.queued) {
        return false;
    }
    let slot = selector.max(0) as usize;
    slot < object.queue.entries.len()
        && i32::from(object.queue.entries[slot].type_index) == RAZING_TYPE
}

fn local_razing(object: &BuildActionObjectState, slot: i32) -> bool {
    let Ok(slot) = usize::try_from(slot) else {
        return false;
    };
    slot < object.queue.queued as usize
        && slot < object.queue.entries.len()
        && i32::from(object.queue.entries[slot].type_index) == RAZING_TYPE
}

fn type_facts(
    facts: &BuildActionUnqueueFacts,
    type_index: i32,
) -> Result<BuildQueuedTypeFacts, BuildActionUnqueueError> {
    let index = usize::try_from(type_index)
        .map_err(|_| BuildActionUnqueueError::TypeOutOfRange(type_index))?;
    if type_index >= RETAIL_TYPE_SLOTS {
        return Err(BuildActionUnqueueError::TypeOutOfRange(type_index));
    }
    let found = facts
        .types
        .get(index)
        .copied()
        .flatten()
        .ok_or(BuildActionUnqueueError::MissingTypeFact(type_index))?;
    if found.type_index != type_index {
        return Err(BuildActionUnqueueError::TypeIdentity(type_index));
    }
    match (found.is_unit_type, found.object) {
        (true, None) => return Err(BuildActionUnqueueError::MissingObjectTypeFact(type_index)),
        (false, Some(_)) => {
            return Err(BuildActionUnqueueError::UnexpectedObjectTypeFact(
                type_index,
            ))
        }
        (true, Some(object)) if object.attack == 0 && object.armed.is_some() => {
            return Err(BuildActionUnqueueError::UnexpectedArmedTypeFact(type_index))
        }
        (true, Some(object)) if object.attack != 0 && object.armed.is_none() => {
            return Err(BuildActionUnqueueError::MissingArmedTypeFact(type_index))
        }
        (true, Some(object)) if object.attack != 0 => {
            let armed = object.armed.expect("matched some");
            let fixed = matches!(
                armed.training_site,
                TRAIN_AT_BARRACKS | TRAIN_AT_STABLE | TRAIN_AT_FACTORY | TRAIN_AT_DOCK
            );
            match (fixed, armed.domain) {
                (true, Some(_)) => {
                    return Err(BuildActionUnqueueError::UnexpectedDomainTypeFact(
                        type_index,
                    ))
                }
                (false, None) => {
                    return Err(BuildActionUnqueueError::MissingDomainTypeFact(type_index))
                }
                _ => {}
            }
        }
        _ => {}
    }
    Ok(found)
}

fn training_value(training: &TrainingQueueCounters, kind: TrainingQueueKind) -> i32 {
    match kind {
        TrainingQueueKind::Barracks => training.barracks,
        TrainingQueueKind::Stable => training.stable,
        TrainingQueueKind::Factory => training.factory,
        TrainingQueueKind::Combat => training.combat,
        TrainingQueueKind::Dock => training.dock,
        TrainingQueueKind::Air => training.air,
    }
}

fn training_value_mut(training: &mut TrainingQueueCounters, kind: TrainingQueueKind) -> &mut i32 {
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
    state: &mut BuildActionUnqueueState,
    kind: TrainingQueueKind,
    steps: &mut Vec<BuildActionStep>,
) {
    let before = training_value(&state.training, kind);
    if before != 0 {
        let after = before.wrapping_sub(1);
        *training_value_mut(&mut state.training, kind) = after;
        steps.push(BuildActionStep::WriteTrainingQueued {
            kind,
            before,
            after,
        });
    }
}

fn apply_local_unqueue(
    state: &mut BuildActionUnqueueState,
    facts: &BuildActionUnqueueFacts,
    object_index: i16,
    requested_slot: i32,
    refund: bool,
    steps: &mut Vec<BuildActionStep>,
) -> Result<(), BuildActionUnqueueError> {
    let queued = object(state, object_index)?.queue.queued as usize;
    let allocated = object(state, object_index)?.queue.entries.len();
    if queued > allocated {
        return Err(BuildActionUnqueueError::LogicalLengthExceedsAllocation(
            object_index,
        ));
    }
    let Ok(mut slot) = usize::try_from(requested_slot) else {
        return Err(BuildActionUnqueueError::NegativeForwardedSlot);
    };
    if queued == 0 || slot >= queued {
        return Ok(());
    }
    if refund {
        while slot + 1 < queued
            && object(state, object_index)?.queue.entries[slot].type_index
                == object(state, object_index)?.queue.entries[slot + 1].type_index
        {
            slot += 1;
        }
    }

    let entry = object(state, object_index)?.queue.entries[slot];
    let type_index = i32::from(entry.type_index);
    let profile = type_facts(facts, type_index)?;
    let counter = state
        .queued_counts
        .get(type_index as usize)
        .copied()
        .ok_or(BuildActionUnqueueError::CounterOutOfRange(type_index))?;
    if !(0..=u16::MAX as i32).contains(&counter) {
        return Err(BuildActionUnqueueError::CounterOutOfRange(type_index));
    }
    if refund {
        for &good in &entry.res {
            if good >= 0
                && usize::try_from(good)
                    .ok()
                    .filter(|&g| g < RETAIL_GOODS)
                    .is_none()
            {
                return Err(BuildActionUnqueueError::ResourceOutOfRange(good));
            }
        }
    }

    let dirty_before = state.queue_dirty;
    state.queue_dirty = true;
    steps.push(BuildActionStep::WriteQueueDirty {
        before: dirty_before,
        after: true,
    });
    let elapsed_before = object(state, object_index)?.queue.entries[slot].elapsed;
    object_mut(state, object_index)?.queue.entries[slot].elapsed = 0;
    steps.push(BuildActionStep::WriteElapsedZero {
        object_index,
        slot,
        before: elapsed_before,
    });

    if counter != 0 {
        state.queued_counts[type_index as usize] = counter - 1;
        steps.push(BuildActionStep::WriteTypeQueued {
            type_index,
            before: counter,
            after: counter - 1,
        });
    }
    if profile.is_unit_type {
        let object = profile.object.expect("type projection preflight");
        if object.attack != 0 {
            let armed = object.armed.expect("type projection preflight");
            match armed.training_site {
                TRAIN_AT_BARRACKS => {
                    decrement_training(state, TrainingQueueKind::Barracks, steps);
                    decrement_training(state, TrainingQueueKind::Combat, steps);
                }
                TRAIN_AT_STABLE => {
                    decrement_training(state, TrainingQueueKind::Stable, steps);
                    decrement_training(state, TrainingQueueKind::Combat, steps);
                }
                TRAIN_AT_FACTORY => decrement_training(state, TrainingQueueKind::Factory, steps),
                TRAIN_AT_DOCK => decrement_training(state, TrainingQueueKind::Dock, steps),
                _ if armed.domain == Some(AIR_DOMAIN) => {
                    decrement_training(state, TrainingQueueKind::Air, steps)
                }
                _ => {}
            }
        }
    }
    // `TypeData::is_tech_type` and `TypeData::is_spell_type` are closed interval tests in
    // the shipped image; they do not dispatch through the runtime's broader completion
    // classification.
    if (FIRST_TECH_TYPE..=LAST_TECH_TYPE).contains(&type_index) {
        let before = state.ages_queued;
        state.ages_queued = before.wrapping_sub(1);
        steps.push(BuildActionStep::WriteAgesQueued {
            before,
            after: state.ages_queued,
        });
    }
    if (FIRST_SPELL_TYPE..=LAST_SPELL_TYPE).contains(&type_index) {
        let before = state.epochs_queued;
        state.epochs_queued = before.wrapping_sub(1);
        steps.push(BuildActionStep::WriteEpochsQueued {
            before,
            after: state.epochs_queued,
        });
    }
    if refund {
        for (queue_cost_slot, (&good, &amount)) in entry.res.iter().zip(&entry.amt).enumerate() {
            if good < 0 {
                continue;
            }
            let good = good as usize;
            let before = state.resources[good];
            let after = before.wrapping_add(i32::from(amount));
            state.resources[good] = after;
            steps.push(BuildActionStep::WriteResource {
                queue_cost_slot,
                good,
                amount,
                before,
                after,
            });
            let scratch_before = state.resource_scratch;
            state.resource_scratch = after;
            steps.push(BuildActionStep::WriteResourceScratch {
                before: scratch_before,
                after,
            });
        }
    }

    let queue = &mut object_mut(state, object_index)?.queue;
    if slot + 1 < queued {
        queue.entries.copy_within(slot + 1..queued, slot);
    }
    let queued_before = queue.queued;
    queue.queued -= 1;
    steps.push(BuildActionStep::CompactQueue {
        object_index,
        removed_slot: slot,
        queued_before,
        queued_after: queue.queued,
    });
    if queue.queued == 0 {
        let object = object_mut(state, object_index)?;
        let before = object.build_masks;
        object.build_masks &= !mask::REPEAT_QUEUE;
        steps.push(BuildActionStep::ClearEmptyRepeatLatch {
            object_index,
            before,
            after: object.build_masks,
        });
    }
    Ok(())
}

fn apply_unqueue(
    state: &mut BuildActionUnqueueState,
    facts: &BuildActionUnqueueFacts,
    source: i16,
    slot: i32,
    refund: bool,
    steps: &mut Vec<BuildActionStep>,
) -> Result<(), BuildActionUnqueueError> {
    steps.push(BuildActionStep::CallUnqueue {
        source,
        slot,
        refund,
    });
    let local_razing = local_razing(object(state, source)?, slot);
    if !local_razing && read_is_library(facts, source, steps)? {
        if let Some(first) = first_library(state, facts, steps)? {
            if first != source {
                let source_queued = i32::from(object(state, source)?.queue.queued);
                let target_slot = slot.wrapping_sub(source_queued);
                steps.push(BuildActionStep::RouteUnqueue {
                    from: source,
                    to: first,
                    source_slot: slot,
                    target_slot,
                });
                return apply_local_unqueue(state, facts, first, target_slot, refund, steps);
            }
        }
    }
    apply_local_unqueue(state, facts, source, slot, refund, steps)
}

pub fn plan_build_action_unqueue(
    request: BuildActionUnqueueRequest,
    before: &BuildActionUnqueueState,
    facts: &BuildActionUnqueueFacts,
) -> Result<BuildActionUnqueuePlan, BuildActionUnqueueError> {
    if usize::from(before.owner) >= RETAIL_LEADER_SLOTS {
        return Err(BuildActionUnqueueError::OwnerOutOfRange);
    }
    if facts.objects.len() != before.objects.len() {
        return Err(BuildActionUnqueueError::ObjectFactShape);
    }
    object(before, request.object_index)?;
    let mut after = before.clone();
    let mut steps = Vec::new();
    let mut receiver = request.object_index;

    // `action_unqueue` repeats the receiver prefix after routing to the first Library.
    // A well-formed owner graph converges in at most one hop because the selected object
    // is, by definition, the first Library. Refuse a malformed cycle rather than spin.
    for _ in 0..=1 {
        let razing = action_razing(object(&after, receiver)?, request.selector);
        steps.push(BuildActionStep::InspectRazing {
            object_index: receiver,
            selector: request.selector,
            value: razing,
        });
        if !razing && read_is_library(facts, receiver, &mut steps)? {
            if let Some(first) = first_library(&after, facts, &mut steps)? {
                if first != receiver {
                    steps.push(BuildActionStep::RouteToFirstLibrary {
                        from: receiver,
                        to: first,
                    });
                    receiver = first;
                    continue;
                }
            }
        }
        break;
    }

    let queued = object(&after, receiver)?.queue.queued;
    steps.push(BuildActionStep::ReadQueueLength {
        object_index: receiver,
        value: queued,
    });
    if queued == 0 {
        return Ok(BuildActionUnqueuePlan {
            after,
            steps,
            direct_rng_draws: 0,
        });
    }

    if object(&after, receiver)?.build_masks & mask::REPEAT_QUEUE != 0 {
        let object = object_mut(&mut after, receiver)?;
        let before_masks = object.build_masks;
        object.build_masks &= !mask::REPEAT_QUEUE;
        steps.push(BuildActionStep::ClearRepeatLatch {
            object_index: receiver,
            before: before_masks,
            after: object.build_masks,
        });
        if before.owner == facts.local_player {
            steps.push(BuildActionStep::LocalRepeatDisabledUi {
                owner: before.owner,
            });
            steps.push(BuildActionStep::Sound {
                category: REPEAT_DISABLED_SOUND,
            });
        }
        if request.selector >= -1 {
            return Ok(BuildActionUnqueuePlan {
                after,
                steps,
                direct_rng_draws: 0,
            });
        }
    }

    if request.selector <= -10 {
        while object(&after, receiver)?.queue.queued != 0 {
            let before_len = object(&after, receiver)?.queue.queued;
            apply_unqueue(
                &mut after,
                facts,
                receiver,
                i32::from(before_len) - 1,
                true,
                &mut steps,
            )?;
            if object(&after, receiver)?.queue.queued >= before_len {
                return Err(BuildActionUnqueueError::CancellationMadeNoProgress);
            }
        }
    } else if request.selector <= -5 {
        let count = usize::from(queued.min(5));
        for _ in 0..count {
            let slot = i32::from(object(&after, receiver)?.queue.queued) - 1;
            apply_unqueue(&mut after, facts, receiver, slot, true, &mut steps)?;
        }
    } else {
        let slot = if request.selector < 0 {
            i32::from(queued) - 1
        } else {
            request.selector
        };
        apply_unqueue(&mut after, facts, receiver, slot, true, &mut steps)?;
    }

    Ok(BuildActionUnqueuePlan {
        after,
        steps,
        direct_rng_draws: 0,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildActionUnqueueStatus {
    Complete,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildActionUnqueueReceipt {
    pub request: BuildActionUnqueueRequest,
    pub status: BuildActionUnqueueStatus,
    pub before: Option<BuildActionUnqueueState>,
    pub facts: Option<BuildActionUnqueueFacts>,
    pub plan: Option<BuildActionUnqueuePlan>,
}

impl BuildActionUnqueueReceipt {
    pub fn unavailable(request: BuildActionUnqueueRequest) -> Self {
        Self {
            request,
            status: BuildActionUnqueueStatus::Unavailable,
            before: None,
            facts: None,
            plan: None,
        }
    }

    pub fn validates(&self, request: BuildActionUnqueueRequest) -> bool {
        if self.request != request {
            return false;
        }
        match self.status {
            BuildActionUnqueueStatus::Unavailable => {
                self.before.is_none() && self.facts.is_none() && self.plan.is_none()
            }
            BuildActionUnqueueStatus::Complete => {
                let (Some(before), Some(facts), Some(plan)) =
                    (&self.before, &self.facts, &self.plan)
                else {
                    return false;
                };
                plan_build_action_unqueue(request, before, facts).as_ref() == Ok(plan)
            }
        }
    }
}
