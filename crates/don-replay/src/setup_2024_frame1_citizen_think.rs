//! Source-bound continuation of the frame-one Citizen idle transaction into `Unit::think`.
//!
//! The SetAnim continuation deliberately stops with a complete detached receiver. This module
//! joins that receiver to the exact post-command Sim for the reads which do not live in it:
//! both Leader flag mirrors, the owner's `LeaderOptions::peasants_wait`, encoded actor position,
//! and replay-carried `Constants::unit_build_respond_range`. It executes only the instruction-
//! closed `Unit::think -> Unit::think_peasant(0)` prefix. A wait-gate miss returns locally from
//! `think_peasant` and leaves a typed Unit-think suffix request; a reached build search leaves the
//! exact `Objects::find_builds` arguments. Neither outcome publishes canonical state.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::systems::leader_unit_think_pending::{
    PreparedUnitThinkLeaderPending, UnitThinkLeaderPendingError, UnitThinkLeaderPendingSource,
};
use don_sim::systems::save_load::{save_sim, SaveError};
use don_sim::systems::setup_idle_prefix::{
    GoldenFrame1BuildIdentity, GoldenFrame1EntryAuthority, GoldenFrame1EntrySource,
    GoldenFrame1UnitIdentity, IdleCitizenPreimage, IdleFindBuildsRequest, IdleOpenRequest,
};
use don_sim::tick::Sim;

use crate::replay::{load_payload, Replay};
use crate::setup_2024_frame1_set_anim_continuation::{
    validate_frame1_citizen_idle_continuation, Frame1CitizenIdleContinuationError,
    Frame1CitizenIdleContinuationPlan,
};
use crate::setup_2024_frame379::{Frame379SetupEntryReceipt, REPLAY_FILE_SHA256};
use crate::setup_2024_golden_capture::{
    validate_frame1_post_command_authority, Frame1GoldenBindError, Frame1PostCommandAuthority,
};
use crate::world_owner_frontier::sha256;

pub const UNIT_THINK_ENTRY_VA: u32 = 0x005f_6e40;
pub const UNIT_THINK_CITIZEN_MASK_CLEAR_VA: u32 = 0x005f_6f91;
pub const UNIT_THINK_FLAGS2_GATE_VA: u32 = 0x005f_6fde;
pub const UNIT_THINK_WORKER_GATE_VA: u32 = 0x005f_7192;
pub const UNIT_THINK_PEASANT_CALL_VA: u32 = 0x005f_719d;
pub const UNIT_THINK_PEASANT_WAIT_READ_VA: u32 = 0x005f_5783;
pub const UNIT_THINK_PEASANT_WAIT_COMPARE_VA: u32 = 0x005f_57ca;
pub const UNIT_FIND_BUILD_SPOT_FIND_BUILDS_CALL_VA: u32 = 0x0060_3f0c;
pub const UNIT_DO_IDLE_MASK2_RESTORE_VA: u32 = 0x0060_dd68;
pub const COORD_XOR: i32 = 0x0006_3637;
pub const UNIT_BUILD_RESPOND_RANGE_CONSTANT_OFFSET: usize = 0x24;
pub const PROOF_DOCUMENT: &str = "docs/assembly/replay-2024-frame1-citizen-think.md";

const SETUP_TYPES: [i32; 7] = [69, 62, 62, 50, 50, 50, 50];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1CitizenThinkAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub frame1_post_command_digest: [u8; 32],
    pub frame1_entry_digest: [u8; 32],
    pub continuation_digest: [u8; 32],
    pub post_command_sim_sha256: [u8; 32],
    pub set_anim_return_sim_sha256: [u8; 32],
    pub row: usize,
    pub unit: GoldenFrame1UnitIdentity,
    /// Stored `SubObjectData +0x10/+0x14` words, before retail's XOR decode.
    pub stored_x: i32,
    pub stored_y: i32,
    pub decoded_x: i32,
    pub decoded_y: i32,
    pub actor_unit_masks: u32,
    pub actor_unit_masks2: u32,
    pub actor_idle: u8,
    pub actor_stance: i8,
    pub victory_flags: i32,
    pub step8_flags: u32,
    pub victory_flags2: i32,
    pub step8_flags2: u32,
    pub peasants_wait: i32,
    pub unit_build_respond_range: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame1CitizenThinkWrite {
    ClearCitizenMasks {
        before: u32,
        after: u32,
    },
    StageLeaderPending {
        flags_before: u32,
        flags_after: u32,
    },
    PeasantWaitLocalReturn {
        idle: u8,
        peasants_wait: i32,
        threshold: u8,
    },
    ReachFindBuilds {
        idle: u8,
        peasants_wait: i32,
        threshold: u8,
    },
    RestoreTemporaryMask2Bit8000 {
        before: u32,
        after: u32,
    },
}

/// First boundary after the instruction-closed Think/think-peasant prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame1CitizenThinkOpenRequest {
    /// `Unit::think` returned through an exact local gate. The outer idle restore is discharged.
    Complete,
    /// `think_peasant(0)` returned zero. The enclosing `Unit::think` suffix has not run.
    ThinkSuffix(Frame1CitizenThinkSuffixRequest),
    /// `find_build_spot` reached the stateful `Objects::find_builds` child.
    FindBuilds(IdleFindBuildsRequest),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1CitizenThinkSuffixRequest {
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub continuation_digest: [u8; 32],
    pub frame: i32,
    pub unit: don_sim::world::Handle,
    pub after_think_peasant: IdleCitizenPreimage,
    pub staged_leader_pending: PreparedUnitThinkLeaderPending,
    pub restore_mask2_bit8000: bool,
}

/// A detached transaction. The staged Leader OR and Unit writes have not reached a Sim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1CitizenThinkPlan {
    pub composition_digest: [u8; 32],
    pub authority: Frame1CitizenThinkAuthority,
    pub continuation: Frame1CitizenIdleContinuationPlan,
    pub staged_leader_pending: PreparedUnitThinkLeaderPending,
    pub after_local: IdleCitizenPreimage,
    pub journal: Vec<Frame1CitizenThinkWrite>,
    pub restore_mask2_bit8000: bool,
    pub open: Frame1CitizenThinkOpenRequest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame1CitizenThinkError {
    Golden(Frame1GoldenBindError),
    Continuation(Frame1CitizenIdleContinuationError),
    LeaderPending(UnitThinkLeaderPendingError),
    ReplayRead(String),
    PayloadRead(String),
    WrongReplayFile,
    MissingRules,
    MissingConstants,
    WrongBuildRespondRange { actual: i32 },
    EntryAuthorityMismatch,
    NotThink,
    ThinkRequestMismatch,
    StaleActor,
    ActorBeforeImageMismatch,
    LeaderOptionsMismatch,
    RestoreNotArmed,
    UnsupportedThinkSchedule,
    UnsupportedWorkerStance { stance: i8 },
    PriorityPeasantBoundary,
    StalePlan,
    Snapshot(SaveError),
    SetAnimReturnSnapshotMismatch,
}

impl fmt::Display for Frame1CitizenThinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "2024 frame-1 Citizen Think continuation refused: {self:?}"
        )
    }
}

impl std::error::Error for Frame1CitizenThinkError {}

impl From<Frame1GoldenBindError> for Frame1CitizenThinkError {
    fn from(value: Frame1GoldenBindError) -> Self {
        Self::Golden(value)
    }
}

impl From<Frame1CitizenIdleContinuationError> for Frame1CitizenThinkError {
    fn from(value: Frame1CitizenIdleContinuationError) -> Self {
        Self::Continuation(value)
    }
}

impl From<UnitThinkLeaderPendingError> for Frame1CitizenThinkError {
    fn from(value: UnitThinkLeaderPendingError) -> Self {
        Self::LeaderPending(value)
    }
}

/// Materialize the exact idle-prefix entry authority from the already-bound post-command Sim.
///
/// Callers which have not yet validated `post` against `sim` should use
/// [`plan_frame1_citizen_think`], which performs that validation before calling this helper.
pub fn bind_frame1_golden_entry_authority(
    post: &Frame1PostCommandAuthority,
    sim: &Sim,
) -> Result<GoldenFrame1EntryAuthority, Frame1CitizenThinkError> {
    let mut units = Vec::with_capacity(7);
    for (ordinal, (stable, type_index)) in post.setup_members.iter().zip(SETUP_TYPES).enumerate() {
        if stable.owner != 0 || stable.o != ordinal as i32 {
            return Err(Frame1CitizenThinkError::EntryAuthorityMismatch);
        }
        let handle = don_sim::world::Handle {
            id: stable.id,
            generation: stable.generation,
        };
        let row = sim
            .world
            .row_of(handle)
            .ok_or(Frame1CitizenThinkError::EntryAuthorityMismatch)?;
        if sim.world.units.get_who(row) != 0
            || sim.world.units.o()[row] != ordinal as i16
            || sim.world.unit_type_id(row) != Some(type_index)
        {
            return Err(Frame1CitizenThinkError::EntryAuthorityMismatch);
        }
        units.push(GoldenFrame1UnitIdentity {
            handle,
            who: 0,
            o: ordinal as i16,
            uid: sim.world.units.get_uid(row),
            type_index,
        });
    }
    let units: [GoldenFrame1UnitIdentity; 7] = units
        .try_into()
        .map_err(|_| Frame1CitizenThinkError::EntryAuthorityMismatch)?;
    Ok(GoldenFrame1EntryAuthority {
        revision: post.revision,
        composition_digest: post.composition_digest,
        source: GoldenFrame1EntrySource::SupportedRetailPostCommandAdjacentCapture,
        executable_sha256: post.executable_sha256,
        replay_file_sha256: post.replay_file_sha256,
        post_command_sim_sha256: post.post_command_sim_sha256,
        frame: post.command_frame,
        random_state: post.random_state,
        center: GoldenFrame1BuildIdentity {
            who: 0,
            o: post.starting_builds.center_build_o as i16,
            uid: post.starting_builds.center_uid,
            type_index: post.starting_builds.center_type,
        },
        market: GoldenFrame1BuildIdentity {
            who: 0,
            o: post.starting_builds.market_build_o as i16,
            uid: post.starting_builds.market_uid,
            type_index: post.starting_builds.market_type,
        },
        units,
    })
}

fn replay_build_respond_range(replay: &Replay) -> Result<i32, Frame1CitizenThinkError> {
    let raw = std::fs::read(&replay.path)
        .map_err(|error| Frame1CitizenThinkError::ReplayRead(error.to_string()))?;
    if sha256(&raw) != REPLAY_FILE_SHA256 {
        return Err(Frame1CitizenThinkError::WrongReplayFile);
    }
    let payload = load_payload(&replay.path)
        .map_err(|error| Frame1CitizenThinkError::PayloadRead(error.to_string()))?;
    if sha256(&payload) != replay.initial.payload_sha256 {
        return Err(Frame1CitizenThinkError::WrongReplayFile);
    }
    let rules = replay
        .initial
        .rules
        .ok_or(Frame1CitizenThinkError::MissingRules)?;
    let constants = rules.serialized_offset + 1 + crate::initial::SHIPPED_TYPES_SERIALIZED_BYTES;
    let bytes = payload
        .get(
            constants + UNIT_BUILD_RESPOND_RANGE_CONSTANT_OFFSET
                ..constants + UNIT_BUILD_RESPOND_RANGE_CONSTANT_OFFSET + 4,
        )
        .ok_or(Frame1CitizenThinkError::MissingConstants)?;
    let value = i32::from_le_bytes(bytes.try_into().expect("four-byte constant"));
    if value != 12 {
        return Err(Frame1CitizenThinkError::WrongBuildRespondRange { actual: value });
    }
    Ok(value)
}

fn wait_threshold(peasants_wait: i32) -> u8 {
    match peasants_wait {
        1 => 7,
        2 => 12,
        3 => 17,
        4 => 32,
        5 => 62,
        _ => 2,
    }
}

fn wait_gate_reaches_body(idle: u8, threshold: u8) -> bool {
    idle >= threshold && (idle == threshold || idle.wrapping_sub(2) % 5 == 0)
}

fn authority_digest(authority: &Frame1CitizenThinkAuthority) -> [u8; 32] {
    let mut image = b"don-frame1-citizen-think-authority-v1".to_vec();
    image.extend_from_slice(&authority.revision.to_le_bytes());
    image.extend_from_slice(&authority.frame1_post_command_digest);
    image.extend_from_slice(&authority.frame1_entry_digest);
    image.extend_from_slice(&authority.continuation_digest);
    image.extend_from_slice(&authority.post_command_sim_sha256);
    image.extend_from_slice(&authority.set_anim_return_sim_sha256);
    image.extend_from_slice(&(authority.row as u64).to_le_bytes());
    image.extend_from_slice(&authority.unit.handle.id.to_le_bytes());
    image.extend_from_slice(&authority.unit.handle.generation.to_le_bytes());
    image.extend_from_slice(&authority.stored_x.to_le_bytes());
    image.extend_from_slice(&authority.stored_y.to_le_bytes());
    image.extend_from_slice(&authority.decoded_x.to_le_bytes());
    image.extend_from_slice(&authority.decoded_y.to_le_bytes());
    image.extend_from_slice(&authority.actor_unit_masks.to_le_bytes());
    image.extend_from_slice(&authority.actor_unit_masks2.to_le_bytes());
    image.push(authority.actor_idle);
    image.push(authority.actor_stance as u8);
    image.extend_from_slice(&authority.victory_flags.to_le_bytes());
    image.extend_from_slice(&authority.step8_flags.to_le_bytes());
    image.extend_from_slice(&authority.victory_flags2.to_le_bytes());
    image.extend_from_slice(&authority.step8_flags2.to_le_bytes());
    image.extend_from_slice(&authority.peasants_wait.to_le_bytes());
    image.extend_from_slice(&authority.unit_build_respond_range.to_le_bytes());
    sha256(&image)
}

fn plan_digest(
    authority: &Frame1CitizenThinkAuthority,
    pending: PreparedUnitThinkLeaderPending,
    after: &IdleCitizenPreimage,
    journal: &[Frame1CitizenThinkWrite],
    open: &Frame1CitizenThinkOpenRequest,
) -> [u8; 32] {
    let mut image = b"don-frame1-citizen-think-plan-v1".to_vec();
    image.extend_from_slice(&authority.composition_digest);
    image.extend_from_slice(&pending.source.revision.to_le_bytes());
    image.extend_from_slice(&pending.source.composition_digest);
    image.extend_from_slice(&pending.flags_after.to_le_bytes());
    image.extend_from_slice(&after.unit_masks.to_le_bytes());
    image.extend_from_slice(&after.unit_masks2.to_le_bytes());
    image.push(after.idle);
    image.push(after.object_flags);
    image.extend_from_slice(&(journal.len() as u64).to_le_bytes());
    for write in journal {
        match write {
            Frame1CitizenThinkWrite::ClearCitizenMasks { before, after } => {
                image.push(0);
                image.extend_from_slice(&before.to_le_bytes());
                image.extend_from_slice(&after.to_le_bytes());
            }
            Frame1CitizenThinkWrite::StageLeaderPending {
                flags_before,
                flags_after,
            } => {
                image.push(1);
                image.extend_from_slice(&flags_before.to_le_bytes());
                image.extend_from_slice(&flags_after.to_le_bytes());
            }
            Frame1CitizenThinkWrite::PeasantWaitLocalReturn {
                idle,
                peasants_wait,
                threshold,
            } => {
                image.push(2);
                image.push(*idle);
                image.extend_from_slice(&peasants_wait.to_le_bytes());
                image.push(*threshold);
            }
            Frame1CitizenThinkWrite::ReachFindBuilds {
                idle,
                peasants_wait,
                threshold,
            } => {
                image.push(3);
                image.push(*idle);
                image.extend_from_slice(&peasants_wait.to_le_bytes());
                image.push(*threshold);
            }
            Frame1CitizenThinkWrite::RestoreTemporaryMask2Bit8000 { before, after } => {
                image.push(4);
                image.extend_from_slice(&before.to_le_bytes());
                image.extend_from_slice(&after.to_le_bytes());
            }
        }
    }
    match open {
        Frame1CitizenThinkOpenRequest::Complete => image.push(0),
        Frame1CitizenThinkOpenRequest::ThinkSuffix(request) => {
            image.push(1);
            image.extend_from_slice(&request.authority_revision.to_le_bytes());
            image.extend_from_slice(&request.authority_digest);
            image.extend_from_slice(&request.continuation_digest);
            image.extend_from_slice(&request.frame.to_le_bytes());
            image.extend_from_slice(&request.unit.id.to_le_bytes());
            image.extend_from_slice(&request.unit.generation.to_le_bytes());
            image.extend_from_slice(&request.after_think_peasant.unit_masks.to_le_bytes());
            image.extend_from_slice(&request.after_think_peasant.unit_masks2.to_le_bytes());
            image.push(request.after_think_peasant.idle);
            image.extend_from_slice(&request.staged_leader_pending.flags_after.to_le_bytes());
            image.push(u8::from(request.restore_mask2_bit8000));
        }
        Frame1CitizenThinkOpenRequest::FindBuilds(request) => {
            image.push(2);
            image.extend_from_slice(&request.authority_revision.to_le_bytes());
            image.extend_from_slice(&request.authority_digest);
            image.extend_from_slice(&request.frame.to_le_bytes());
            image.extend_from_slice(&request.unit.id.to_le_bytes());
            image.extend_from_slice(&request.unit.generation.to_le_bytes());
            image.extend_from_slice(&request.x.to_le_bytes());
            image.extend_from_slice(&request.y.to_le_bytes());
            image.extend_from_slice(&request.search.to_le_bytes());
            image.push(request.who);
            image.extend_from_slice(&request.radius.to_le_bytes());
            image.extend_from_slice(&request.relation_mask.to_le_bytes());
            image.extend_from_slice(&request.filter.to_le_bytes());
            image.extend_from_slice(&request.arg8.to_le_bytes());
            image.extend_from_slice(&request.origin_x.to_le_bytes());
            image.extend_from_slice(&request.arg10.to_le_bytes());
        }
    }
    sha256(&image)
}

/// Bind the exact post-command source and execute the closed Think/think-peasant prefix.
///
/// `post_command` stays immutable. In both published outcomes the Leader OR, Unit mask clear,
/// and all earlier idle writes remain detached, and the `0x8000` restoration obligation remains
/// armed. A later suffix/FindBuilds receipt must complete before one atomic mount may publish.
pub fn plan_frame1_citizen_think(
    replay: &Replay,
    setup_entry: &Frame379SetupEntryReceipt,
    post_authority: &Frame1PostCommandAuthority,
    post_command: &Sim,
    set_anim_return: &Sim,
    entry_authority: &GoldenFrame1EntryAuthority,
    continuation: &Frame1CitizenIdleContinuationPlan,
) -> Result<Frame1CitizenThinkPlan, Frame1CitizenThinkError> {
    validate_frame1_post_command_authority(setup_entry, post_authority, post_command)?;
    let expected_entry = bind_frame1_golden_entry_authority(post_authority, post_command)?;
    if &expected_entry != entry_authority {
        return Err(Frame1CitizenThinkError::EntryAuthorityMismatch);
    }
    validate_frame1_citizen_idle_continuation(entry_authority, continuation)?;
    let IdleOpenRequest::Think(request) = &continuation.open else {
        return Err(Frame1CitizenThinkError::NotThink);
    };
    if request.authority_revision != entry_authority.revision
        || request.authority_digest != entry_authority.composition_digest
        || request.set_anim_receipt_digest != continuation.set_anim_receipt.composition_digest
        || request.frame != continuation.after_local.frame
        || request.unit != continuation.after_local.unit.handle
        || request.who != continuation.after_local.unit.who
        || request.o != continuation.after_local.unit.o
        || request.after_check_idle != continuation.after_local
    {
        return Err(Frame1CitizenThinkError::ThinkRequestMismatch);
    }
    if !continuation.restore_mask2_bit8000
        || !request.restore_mask2_bit8000
        || continuation.after_local.unit_masks2 & 0x8000 != 0
    {
        return Err(Frame1CitizenThinkError::RestoreNotArmed);
    }

    let set_anim_return_bytes =
        save_sim(set_anim_return).map_err(Frame1CitizenThinkError::Snapshot)?;
    if sha256(&set_anim_return_bytes) != continuation.set_anim_receipt.after_sim_sha256
        || set_anim_return.world.frame != request.frame
    {
        return Err(Frame1CitizenThinkError::SetAnimReturnSnapshotMismatch);
    }

    let before = &continuation.prefix.before;
    let set_anim_entry = &continuation.prefix.after_local;
    let row = set_anim_return
        .world
        .row_of(request.unit)
        .ok_or(Frame1CitizenThinkError::StaleActor)?;
    let units = &set_anim_return.world.units;
    if row != continuation.set_anim_receipt.row
        || units.get_who(row) != set_anim_entry.unit.who
        || units.o()[row] != set_anim_entry.unit.o
        || units.get_uid(row) != set_anim_entry.unit.uid
        || set_anim_return.world.unit_type_id(row) != Some(set_anim_entry.unit.type_index)
        || units.get_flags(row) != set_anim_entry.object_flags
        || units.inside_up()[row] != set_anim_entry.inside_up
        || units.get_unit_masks(row) != set_anim_entry.unit_masks
        || units.get_unit_masks2(row) != set_anim_entry.unit_masks2
        || units.get_idle(row) != set_anim_entry.idle
        || units.stance()[row] != set_anim_entry.worker_stance
        || units.collide()[row] != set_anim_entry.collide
        || units.collide_frame()[row] != set_anim_entry.collide_frame
        || set_anim_return.unit_guys.get(row).and_then(Option::as_ref)
            != Some(&continuation.set_anim_receipt.guys_after)
        || set_anim_return.world.random.state() != continuation.set_anim_receipt.random_after
    {
        return Err(Frame1CitizenThinkError::ActorBeforeImageMismatch);
    }

    let owner = usize::from(request.who);
    let option = post_authority
        .mount
        .rows_after
        .get(owner)
        .ok_or(Frame1CitizenThinkError::LeaderOptionsMismatch)?;
    if option.who != owner as i32 || option.peasants_wait != 2 {
        return Err(Frame1CitizenThinkError::LeaderOptionsMismatch);
    }
    let unit_build_respond_range = replay_build_respond_range(replay)?;
    let stored_x = units.x_internal()[row];
    let stored_y = units.y_internal()[row];
    let victory_flags = set_anim_return.vic_leaders.slots[owner].leader_flags;
    let step8_flags = set_anim_return.step8.leaders[owner].flags;
    let victory_flags2 = set_anim_return.vic_leaders.slots[owner].leader_flags2;
    let step8_flags2 = set_anim_return.step8.leaders[owner].ai.flags2;
    let mut authority = Frame1CitizenThinkAuthority {
        revision: post_authority.revision,
        composition_digest: [0; 32],
        frame1_post_command_digest: post_authority.composition_digest,
        frame1_entry_digest: entry_authority.composition_digest,
        continuation_digest: continuation.composition_digest,
        post_command_sim_sha256: post_authority.post_command_sim_sha256,
        set_anim_return_sim_sha256: continuation.set_anim_receipt.after_sim_sha256,
        row,
        unit: before.unit.clone(),
        stored_x,
        stored_y,
        decoded_x: stored_x ^ COORD_XOR,
        decoded_y: stored_y ^ COORD_XOR,
        actor_unit_masks: request.after_check_idle.unit_masks,
        actor_unit_masks2: request.after_check_idle.unit_masks2,
        actor_idle: request.after_check_idle.idle,
        actor_stance: request.after_check_idle.worker_stance,
        victory_flags,
        step8_flags,
        victory_flags2,
        step8_flags2,
        peasants_wait: option.peasants_wait,
        unit_build_respond_range,
    };
    authority.composition_digest = authority_digest(&authority);

    let pending_source = UnitThinkLeaderPendingSource {
        revision: authority.revision,
        composition_digest: authority.composition_digest,
        owner,
        unit_o: i32::from(request.o),
        unit_type: before.unit.type_index,
    };
    let pending = set_anim_return.prepare_unit_think_leader_pending(pending_source)?;
    let mut after = request.after_check_idle.clone();
    let masks_before = after.unit_masks;
    after.unit_masks &= 0x87ff_ffff;
    let mut journal = vec![
        Frame1CitizenThinkWrite::ClearCitizenMasks {
            before: masks_before,
            after: after.unit_masks,
        },
        Frame1CitizenThinkWrite::StageLeaderPending {
            flags_before: pending.step8_flags_before,
            flags_after: pending.flags_after,
        },
    ];

    // The captured golden receiver has object bit 0x10 set at this point, so the frame/idle
    // scheduling probe at 0x005F6FB5 is not reached. Refuse any other source shape instead of
    // importing an unbound `Game+0x550` scalar.
    if after.object_flags & 0x10 == 0 && after.idle > 2 {
        return Err(Frame1CitizenThinkError::UnsupportedThinkSchedule);
    }
    // Both gates are exact local Unit::think returns, but neither is reached by the supported
    // owner. Their eventual atomic publisher may restore 0x8000 immediately after this return.
    if pending.step8_flags2_before & 2 != 0 || after.unit_masks & 0x0100_0000 != 0 {
        let restore_before = after.unit_masks2;
        after.unit_masks2 |= 0x8000;
        journal.push(Frame1CitizenThinkWrite::RestoreTemporaryMask2Bit8000 {
            before: restore_before,
            after: after.unit_masks2,
        });
        let open = Frame1CitizenThinkOpenRequest::Complete;
        let composition_digest = plan_digest(&authority, pending, &after, &journal, &open);
        return Ok(Frame1CitizenThinkPlan {
            composition_digest,
            authority,
            continuation: continuation.clone(),
            staged_leader_pending: pending,
            after_local: after,
            journal,
            restore_mask2_bit8000: false,
            open,
        });
    }
    if after.unit_masks & 0x100 == 0 {
        return Err(Frame1CitizenThinkError::UnsupportedThinkSchedule);
    }
    if after.unit_masks & 0x40000 != 0 {
        return Err(Frame1CitizenThinkError::PriorityPeasantBoundary);
    }

    let threshold = wait_threshold(option.peasants_wait);
    let open = if !wait_gate_reaches_body(after.idle, threshold) {
        journal.push(Frame1CitizenThinkWrite::PeasantWaitLocalReturn {
            idle: after.idle,
            peasants_wait: option.peasants_wait,
            threshold,
        });
        Frame1CitizenThinkOpenRequest::ThinkSuffix(Frame1CitizenThinkSuffixRequest {
            authority_revision: authority.revision,
            authority_digest: authority.composition_digest,
            continuation_digest: continuation.composition_digest,
            frame: after.frame,
            unit: after.unit.handle,
            after_think_peasant: after.clone(),
            staged_leader_pending: pending,
            restore_mask2_bit8000: true,
        })
    } else {
        if !matches!(after.worker_stance, 1 | 2) {
            return Err(Frame1CitizenThinkError::UnsupportedWorkerStance {
                stance: after.worker_stance,
            });
        }
        journal.push(Frame1CitizenThinkWrite::ReachFindBuilds {
            idle: after.idle,
            peasants_wait: option.peasants_wait,
            threshold,
        });
        let mut radius = unit_build_respond_range.wrapping_mul(192);
        if matches!(after.worker_stance, 1 | 2) {
            radius = radius.wrapping_mul(2);
        }
        Frame1CitizenThinkOpenRequest::FindBuilds(IdleFindBuildsRequest {
            authority_revision: authority.revision,
            authority_digest: authority.composition_digest,
            frame: after.frame,
            unit: after.unit.handle,
            x: authority.decoded_x,
            y: authority.decoded_y,
            search: 1,
            who: after.unit.who,
            radius,
            relation_mask: 0x200,
            filter: 6,
            arg8: 0,
            origin_x: authority.decoded_x,
            arg10: 0,
        })
    };
    let composition_digest = plan_digest(&authority, pending, &after, &journal, &open);
    Ok(Frame1CitizenThinkPlan {
        composition_digest,
        authority,
        continuation: continuation.clone(),
        staged_leader_pending: pending,
        after_local: after,
        journal,
        restore_mask2_bit8000: true,
        open,
    })
}

/// Recompute every source join and detached write before a later continuation consumes a plan.
pub fn validate_frame1_citizen_think_plan(
    replay: &Replay,
    setup_entry: &Frame379SetupEntryReceipt,
    post_authority: &Frame1PostCommandAuthority,
    post_command: &Sim,
    set_anim_return: &Sim,
    entry_authority: &GoldenFrame1EntryAuthority,
    plan: &Frame1CitizenThinkPlan,
) -> Result<(), Frame1CitizenThinkError> {
    let expected = plan_frame1_citizen_think(
        replay,
        setup_entry,
        post_authority,
        post_command,
        set_anim_return,
        entry_authority,
        &plan.continuation,
    )?;
    if &expected != plan {
        return Err(Frame1CitizenThinkError::StalePlan);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retail_peasant_wait_switch_and_period_are_exact() {
        assert_eq!(wait_threshold(0), 2);
        assert_eq!(wait_threshold(1), 7);
        assert_eq!(wait_threshold(2), 12);
        assert_eq!(wait_threshold(3), 17);
        assert_eq!(wait_threshold(4), 32);
        assert_eq!(wait_threshold(5), 62);
        assert_eq!(wait_threshold(6), 2);

        assert!(!wait_gate_reaches_body(2, 12));
        assert!(wait_gate_reaches_body(12, 12));
        assert!(!wait_gate_reaches_body(13, 12));
        assert!(wait_gate_reaches_body(17, 12));
    }

    #[test]
    fn stored_positions_and_build_radius_match_the_call_site_arithmetic() {
        let x = 12_345;
        let y = 54_321;
        assert_eq!((x ^ COORD_XOR) ^ COORD_XOR, x);
        assert_eq!((y ^ COORD_XOR) ^ COORD_XOR, y);
        assert_eq!(12i32.wrapping_mul(192), 2_304);
        assert_eq!(12i32.wrapping_mul(192).wrapping_mul(2), 4_608);
    }
}
