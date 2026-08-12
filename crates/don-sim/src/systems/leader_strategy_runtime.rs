// SPDX-License-Identifier: GPL-3.0-or-later

//! Atomic Sim transaction for tick step 11, `Leaders::strategy_all` `0x006ED430`.
//!
//! The dispatcher and `Leader::production_ai` step machine are recovered in
//! [`crate::systems::leaders`] and the parent module. This adapter closes their largest child
//! whose complete inputs already have canonical Sim owners: `Leader::queued_units`
//! `0x006CE000` (394 bytes). It walks the authoritative per-owner Build band, reads live
//! `BuildData` queues, and joins their TypeIndex rows to the installed production type table.
//!
//! `production_ai_setup` `0x006C83E0` is larger but is not in this cohort. Its complete body
//! writes `LeaderDataEncrypt::rate[6]` at `+0xAC`, which `LeaderEcon` does not yet own, and its
//! tail enters `Leader::market_speculation` `0x006C8110`. Treating the existing displayed
//! income or stockpile arrays as that missing rate block would alias distinct PDB fields.

use crate::checksum::adler32;
use crate::objects::{Band, ObjectRegistry};
use crate::systems::leader_production_ai::{flags, flags2};
use crate::systems::leaders::{self, Leaders, StrategyInputs, StrategyTrace, NUM_LEADER_SLOTS};
use crate::systems::production::runtime::{LiveProductionRuntime, LiveTypeClass};
use crate::systems::production::BuildData;
use std::fmt;

pub const QUEUED_UNITS_VA: u32 = 0x006c_e000;
pub const STRATEGY_ALL_VA: u32 = 0x006e_d430;
pub const BUILD_BAND_BASE: usize = 2000;
pub const SNAPSHOT_MAGIC: &[u8; 8] = b"DoNAI11\0";
pub const SNAPSHOT_VERSION: u32 = 1;
const SNAPSHOT_VALUES_PER_LEADER: usize = 9;
const SNAPSHOT_BYTES: usize = 8 + 4 + NUM_LEADER_SLOTS * SNAPSHOT_VALUES_PER_LEADER * 4;
const OWNED_IMAGE_LEN: usize = 0x9e4;

/// One admitted queued Unit row from an active production building.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueuedUnitEntry {
    pub build_object: i32,
    pub queue_slot: usize,
    pub type_index: i32,
    pub control_cost: i32,
}

/// Exact input-side receipt for `Leader::queued_units`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueuedUnitsReceipt {
    pub owner: usize,
    /// Retail-visible object ids, in the Build-band order the canonical registry stores.
    pub builds_visited: Vec<i32>,
    pub active_builds: usize,
    pub logical_entries: usize,
    pub admitted: Vec<QueuedUnitEntry>,
    pub total_control_cost: i32,
}

/// A structural invariant retail guarantees but a detached or corrupt Sim adapter may violate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StrategyRuntimeError {
    OwnerOutsideLeaderTable {
        owner: usize,
    },
    LeaderWhoOutsideTable {
        slot: usize,
        who: i32,
    },
    MissingBuildRow {
        owner: usize,
        row: usize,
    },
    BuildOwnerMismatch {
        owner: usize,
        row: usize,
        actual: u8,
    },
    QueueLengthExceedsStorage {
        owner: usize,
        build_object: i32,
        queued: usize,
        stored: usize,
    },
    MissingType {
        owner: usize,
        build_object: i32,
        type_index: i32,
    },
}

impl fmt::Display for StrategyRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OwnerOutsideLeaderTable { owner } => {
                write!(f, "owner {owner} is outside the eight retail Leader slots")
            }
            Self::LeaderWhoOutsideTable { slot, who } => {
                write!(f, "Leader slot {slot} carries out-of-range who {who}")
            }
            Self::MissingBuildRow { owner, row } => {
                write!(f, "owner {owner} Build band references absent row {row}")
            }
            Self::BuildOwnerMismatch { owner, row, actual } => write!(
                f,
                "owner {owner} Build band row {row} belongs to owner {actual}"
            ),
            Self::QueueLengthExceedsStorage {
                owner,
                build_object,
                queued,
                stored,
            } => write!(
                f,
                "owner {owner} build {build_object} has logical queue {queued} over {stored} stored rows"
            ),
            Self::MissingType {
                owner,
                build_object,
                type_index,
            } => write!(
                f,
                "owner {owner} build {build_object} queues missing TypeIndex {type_index}"
            ),
        }
    }
}

impl std::error::Error for StrategyRuntimeError {}

/// Execute `Leader::queued_units` over the Sim's authoritative sparse Build band.
///
/// Retail's loop begins at object index 2000, requires `is_valid()` and `is_active()`, then
/// examines every logical queue entry. Only a Unit TypeData row with
/// `LeaderData::type_avail(type, 1) > 3` contributes `UnitTypeData::control` (`+0x2F0`). An
/// installed [`LiveProductionType`](crate::systems::production::runtime::LiveProductionType)
/// plus its owner's canonical TechState represents that admitted availability with
/// `class == Unit && can_make && prerequisites held`.
pub fn queued_units(
    registry: &ObjectRegistry,
    builds: &[BuildData],
    runtime: &LiveProductionRuntime,
    owner: usize,
) -> Result<QueuedUnitsReceipt, StrategyRuntimeError> {
    if owner >= NUM_LEADER_SLOTS {
        return Err(StrategyRuntimeError::OwnerOutsideLeaderTable { owner });
    }
    let mut receipt = QueuedUnitsReceipt {
        owner,
        builds_visited: Vec::new(),
        active_builds: 0,
        logical_entries: 0,
        admitted: Vec::new(),
        total_control_cost: 0,
    };

    for (band_index, &row) in registry.slot(owner).band(Band::Build).iter().enumerate() {
        let row = row as usize;
        let build = builds
            .get(row)
            .ok_or(StrategyRuntimeError::MissingBuildRow { owner, row })?;
        if build.who as usize != owner {
            return Err(StrategyRuntimeError::BuildOwnerMismatch {
                owner,
                row,
                actual: build.who,
            });
        }
        let build_object = BUILD_BAND_BASE.wrapping_add(band_index) as i32;
        receipt.builds_visited.push(build_object);
        if !build.is_valid() || !build.is_active() {
            continue;
        }
        receipt.active_builds += 1;

        let logical = build.queue.queued as usize;
        if logical > build.queue.entries.len() {
            return Err(StrategyRuntimeError::QueueLengthExceedsStorage {
                owner,
                build_object,
                queued: logical,
                stored: build.queue.entries.len(),
            });
        }
        receipt.logical_entries = receipt.logical_entries.wrapping_add(logical);
        for (queue_slot, entry) in build.queue.entries.iter().take(logical).enumerate() {
            let type_index = entry.type_index as i32;
            let facts = usize::try_from(type_index)
                .ok()
                .and_then(|index| runtime.types.get(index))
                .and_then(Option::as_ref)
                .ok_or(StrategyRuntimeError::MissingType {
                    owner,
                    build_object,
                    type_index,
                })?;
            // The installed type's static `can_make` plus the canonical leader TechState is
            // the same per-leader projection the live production owner uses for its
            // `type_avail(type, 1) == 4` Unit classification.
            if facts.class != LiveTypeClass::Unit
                || !facts.can_make
                || !runtime.leader_has_prerequisites(owner, type_index)
            {
                continue;
            }
            receipt.total_control_cost =
                receipt.total_control_cost.wrapping_add(facts.control_cost);
            receipt.admitted.push(QueuedUnitEntry {
                build_object,
                queue_slot,
                type_index,
                control_cost: facts.control_cost,
            });
        }
    }
    Ok(receipt)
}

/// The retail-owned step-11 fields needed for replay-visible before/after and save/resume.
/// Host answers (`queued_units`, script return, MakeList head) are deliberately absent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StrategyLeaderState {
    pub leader_flags: i32,
    pub leader_flags2: i32,
    pub who: i32,
    pub production_step: i32,
    pub prod_script_run: i32,
    pub script_step: i32,
    pub control: i32,
    pub explored: i32,
    pub effective_pop: i32,
}

impl StrategyLeaderState {
    fn capture(leaders: &Leaders, slot: usize) -> Self {
        let leader = &leaders.leaders[slot];
        Self {
            leader_flags: leader.flags as i32,
            leader_flags2: leader.ai.flags2 as i32,
            who: leader.slot,
            production_step: leader.ai.production_step,
            prod_script_run: leader.ai.prod_script_run,
            script_step: leader.ai.script_step,
            control: leader.ai.control,
            explored: leader.explored,
            effective_pop: leader.ai.effective_pop,
        }
    }

    fn restore(self, leaders: &mut Leaders, slot: usize) {
        let leader = &mut leaders.leaders[slot];
        leader.flags = self.leader_flags as u32;
        leader.ai.flags2 = self.leader_flags2 as u32;
        leader.slot = self.who;
        leader.ai.production_step = self.production_step;
        leader.ai.prod_script_run = self.prod_script_run;
        leader.ai.script_step = self.script_step;
        leader.ai.control = self.control;
        leader.explored = self.explored;
        leader.ai.effective_pop = self.effective_pop;
    }

    fn values(self) -> [i32; SNAPSHOT_VALUES_PER_LEADER] {
        [
            self.leader_flags,
            self.leader_flags2,
            self.who,
            self.production_step,
            self.prod_script_run,
            self.script_step,
            self.control,
            self.explored,
            self.effective_pop,
        ]
    }

    fn from_values(v: [i32; SNAPSHOT_VALUES_PER_LEADER]) -> Self {
        Self {
            leader_flags: v[0],
            leader_flags2: v[1],
            who: v[2],
            production_step: v[3],
            prod_script_run: v[4],
            script_step: v[5],
            control: v[6],
            explored: v[7],
            effective_pop: v[8],
        }
    }

    /// Sparse owned projection at the real PDB offsets, suitable for before/after receipts.
    fn owned_image(self) -> [u8; OWNED_IMAGE_LEN] {
        let mut image = [0u8; OWNED_IMAGE_LEN];
        for (offset, value) in [
            (0x000, self.leader_flags),
            (0x004, self.leader_flags2),
            (0x008, self.who),
            (0x788, self.production_step),
            (0x78c, self.prod_script_run),
            (0x790, self.script_step),
            (0x940, self.control),
            (0x9d4, self.explored),
            (0x9e0, self.effective_pop),
        ] {
            image[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        image
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StrategySnapshot {
    pub leaders: [StrategyLeaderState; NUM_LEADER_SLOTS],
}

impl StrategySnapshot {
    pub fn capture(leaders: &Leaders) -> Self {
        Self {
            leaders: std::array::from_fn(|slot| StrategyLeaderState::capture(leaders, slot)),
        }
    }

    pub fn restore(&self, leaders: &mut Leaders) {
        for (slot, state) in self.leaders.iter().copied().enumerate() {
            state.restore(leaders, slot);
        }
    }

    /// Adler-32 over each sparse retail-offset image in slot order. This is an owned-field
    /// receipt, not a claim to the complete retail Leader checksum channel.
    pub fn owned_adler32(&self) -> u32 {
        self.leaders
            .iter()
            .fold(1, |sum, state| adler32(sum, &state.owned_image()))
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(SNAPSHOT_BYTES);
        out.extend_from_slice(SNAPSHOT_MAGIC);
        out.extend_from_slice(&SNAPSHOT_VERSION.to_le_bytes());
        for state in self.leaders {
            for value in state.values() {
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, StrategySnapshotError> {
        if bytes.len() != SNAPSHOT_BYTES {
            return Err(StrategySnapshotError::Length {
                expected: SNAPSHOT_BYTES,
                actual: bytes.len(),
            });
        }
        if &bytes[..8] != SNAPSHOT_MAGIC {
            return Err(StrategySnapshotError::Magic);
        }
        let version = u32::from_le_bytes(bytes[8..12].try_into().expect("fixed version"));
        if version != SNAPSHOT_VERSION {
            return Err(StrategySnapshotError::Version(version));
        }
        let mut cursor = 12;
        let mut leaders = [StrategyLeaderState::default(); NUM_LEADER_SLOTS];
        for leader in &mut leaders {
            let mut values = [0; SNAPSHOT_VALUES_PER_LEADER];
            for value in &mut values {
                *value = i32::from_le_bytes(
                    bytes[cursor..cursor + 4]
                        .try_into()
                        .expect("length checked above"),
                );
                cursor += 4;
            }
            *leader = StrategyLeaderState::from_values(values);
        }
        Ok(Self { leaders })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrategySnapshotError {
    Length { expected: usize, actual: usize },
    Magic,
    Version(u32),
}

/// Canonical environment fields that presently live outside the step-8 adapter.
#[derive(Clone, Copy, Debug)]
pub struct StrategyCanonicalInputs<'a> {
    pub dispatcher: StrategyInputs<'a>,
    pub ai_off: bool,
    pub starting_resources: u8,
}

/// Atomic before/after proof for one complete dispatcher invocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StrategyTransactionReceipt {
    pub dispatcher_va: u32,
    pub before: StrategySnapshot,
    pub after: StrategySnapshot,
    pub before_adler32: u32,
    pub after_adler32: u32,
    pub queued_units: [Option<QueuedUnitsReceipt>; NUM_LEADER_SLOTS],
    pub trace: StrategyTrace,
}

fn production_will_read_queued_units(leaders: &Leaders, slot: usize, ai_off: bool) -> bool {
    let leader = &leaders.leaders[slot];
    leader.flags & 3 == 3
        && leader.ai.production_step != 0
        && !(leader.flags & flags::HUMAN != 0
            && leader.flags & flags::PRODUCTION_DESPITE_HUMAN == 0)
        && !ai_off
        && leader.ai.flags2 & flags2::PRODUCTION_AI_DISABLED == 0
}

/// Preflight and execute the whole recovered step-11 dispatcher as one atomic transaction.
///
/// Queue/type failures are found before any Leader byte changes. Canonical `ai_off` and
/// `starting_resources`, plus each derived `queued_units()` answer, are installed only for the
/// duration of the call and restored afterward; they are host projections, not duplicate save
/// owners. The returned snapshot contains only retail-owned fields.
pub fn execute_strategy_all(
    leaders: &mut Leaders,
    registry: &ObjectRegistry,
    builds: &[BuildData],
    runtime: &LiveProductionRuntime,
    input: StrategyCanonicalInputs<'_>,
) -> Result<StrategyTransactionReceipt, StrategyRuntimeError> {
    let mut queue_receipts: [Option<QueuedUnitsReceipt>; NUM_LEADER_SLOTS] =
        std::array::from_fn(|_| None);
    for slot in 0..NUM_LEADER_SLOTS {
        if !production_will_read_queued_units(leaders, slot, input.ai_off) {
            continue;
        }
        let leader_who = leaders.leaders[slot].slot;
        let who = usize::try_from(leader_who)
            .ok()
            .filter(|who| *who < NUM_LEADER_SLOTS)
            .ok_or(StrategyRuntimeError::LeaderWhoOutsideTable {
                slot,
                who: leader_who,
            })?;
        queue_receipts[slot] = Some(queued_units(registry, builds, runtime, who)?);
    }

    let before = StrategySnapshot::capture(leaders);
    let old_ai_off = leaders.ai_off;
    let old_starting_resources = leaders.starting_resources;
    let old_queued: [Option<i32>; NUM_LEADER_SLOTS] =
        std::array::from_fn(|slot| leaders.leaders[slot].ai.queued_units);
    leaders.ai_off = input.ai_off;
    leaders.starting_resources = Some(input.starting_resources);
    for slot in 0..NUM_LEADER_SLOTS {
        if let Some(queue) = &queue_receipts[slot] {
            leaders.leaders[slot].ai.queued_units = Some(queue.total_control_cost);
        }
    }

    let trace = leaders::strategy_all(leaders, input.dispatcher);

    leaders.ai_off = old_ai_off;
    leaders.starting_resources = old_starting_resources;
    for (slot, answer) in old_queued.into_iter().enumerate() {
        leaders.leaders[slot].ai.queued_units = answer;
    }
    let after = StrategySnapshot::capture(leaders);
    Ok(StrategyTransactionReceipt {
        dispatcher_va: STRATEGY_ALL_VA,
        before_adler32: before.owned_adler32(),
        after_adler32: after.owned_adler32(),
        before,
        after,
        queued_units: queue_receipts,
        trace,
    })
}

/// Stable count of every child reached by the dispatcher, including a `check_explore` child
/// whose own cadence returned `NotDue` without mutation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StrategyStageMap {
    pub check_explore: usize,
    pub plan_strategy: usize,
    pub compute_score: usize,
    pub diplomacy: usize,
    pub check_victory: bool,
}

pub fn call_stage_map(trace: &StrategyTrace) -> StrategyStageMap {
    let reached = trace.processed.iter().filter(|ran| **ran).count();
    StrategyStageMap {
        check_explore: reached,
        plan_strategy: trace.plan.iter().flatten().count(),
        compute_score: reached,
        diplomacy: trace.diplomacy.iter().flatten().count(),
        check_victory: trace
            .calls
            .iter()
            .any(|call| matches!(call, leaders::StrategyCall::CheckVictory)),
    }
}
