//! Source-bound continuation of the frame-one Citizen `Unit::think` suffix.
//!
//! The adjacent Think planner stops after `think_peasant(0)` returns zero.  This module
//! resumes at `0x005F71AA`, proves the two intervening type predicates from the replay's
//! immutable Rules image, and stops at the first reached stateful child.  For the supported
//! owner-zero Citizen that child is `Unit::set_idle(0)` at `0x005F7344`.  The Leader pending
//! OR, all Unit writes, and the outer `unit_masks2 |= 0x8000` restore remain detached.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::fmt;

use don_sim::systems::land_speed_authority::LandSpeedTypeFacts;
use don_sim::systems::leader_unit_think_pending::PreparedUnitThinkLeaderPending;
use don_sim::systems::setup_idle_prefix::{GoldenFrame1EntryAuthority, IdleCitizenPreimage};
use don_sim::tick::Sim;
use don_sim::world::Handle;

use crate::replay::Replay;
use crate::replay_land_speed_content::{
    produce_replay_land_speed_content, ReplayLandSpeedContent, ReplayLandSpeedContentError,
};
use crate::setup_2024_frame1_citizen_think::{
    validate_frame1_citizen_think_plan, Frame1CitizenThinkError, Frame1CitizenThinkOpenRequest,
    Frame1CitizenThinkPlan, Frame1CitizenThinkSuffixRequest,
};
use crate::setup_2024_frame379::{Frame379SetupEntryReceipt, REPLAY_FILE_SHA256};
use crate::setup_2024_golden_capture::Frame1PostCommandAuthority;
use crate::world_owner_frontier::sha256;

pub const UNIT_THINK_SUFFIX_ENTRY_VA: u32 = 0x005f_71aa;
pub const UNIT_IS_CARAVAN_TYPE_CALL_VA: u32 = 0x005f_71c2;
pub const UNIT_IS_RARE_COLLECTOR_CALL_VA: u32 = 0x005f_71df;
pub const OBJECT_TYPE_IS_RARE_COLLECTOR_CALL_VA: u32 = 0x0046_fb00;
pub const UNIT_THINK_HUMAN_READ_VA: u32 = 0x005f_7317;
pub const UNIT_THINK_LOCAL_PLAYER_READ_VA: u32 = 0x005f_7322;
pub const UNIT_THINK_SET_IDLE_CALL_VA: u32 = 0x005f_7344;
pub const UNIT_SET_IDLE_VA: u32 = 0x005f_6010;
pub const RARE_COLLECTOR_TYPE: i32 = 0x13d;
pub const LEADER_HUMAN: u32 = 0x4;
pub const PROOF_DOCUMENT: &str = "docs/assembly/replay-2024-frame1-citizen-think-suffix.md";

const DIRECT_RARE_COLLECTOR_TYPES: [i32; 3] = [61, 62, 400];

/// Complete replay-carried non-strict `ObjectTypeData::is(type, 0)` proof.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1CitizenTypeRelationProof {
    pub replay_file_sha256: [u8; 32],
    pub replay_payload_sha256: [u8; 32],
    pub rules_serialized_sha256: [u8; 32],
    pub source_type: i32,
    pub target_type: i32,
    /// Source row followed by each exact `from` ancestor inspected by retail.
    pub walked_rows: Vec<LandSpeedTypeFacts>,
    pub result: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame1CitizenThinkSuffixRead {
    IsCaravanFalse {
        unit_flags2: u32,
    },
    IsRareCollectorFalse {
        type_index: i32,
        target_type: i32,
    },
    ReachHumanSetIdle {
        leader_flags: u32,
        unit_masks: u32,
        idle_argument: i32,
    },
}

/// First stateful child reached by the supported golden suffix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1CitizenSetIdleRequest {
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub think_plan_digest: [u8; 32],
    pub frame: i32,
    pub unit: Handle,
    pub who: u8,
    pub o: i16,
    pub idle: i32,
    pub after_suffix_local: IdleCitizenPreimage,
    pub staged_leader_pending: PreparedUnitThinkLeaderPending,
    pub restore_mask2_bit8000: bool,
}

/// First source-dependent read on the non-human branch.  The supported golden owner does not
/// reach it, but exposing it keeps the planner fail-closed if its bound Leader image changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1CitizenLocalPlayerRequest {
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub think_plan_digest: [u8; 32],
    pub frame: i32,
    pub unit: Handle,
    pub after_suffix_local: IdleCitizenPreimage,
    pub staged_leader_pending: PreparedUnitThinkLeaderPending,
    pub restore_mask2_bit8000: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame1CitizenThinkSuffixOpenRequest {
    SetIdle(Frame1CitizenSetIdleRequest),
    LocalPlayer(Frame1CitizenLocalPlayerRequest),
}

/// Detached suffix transaction.  No member of this plan has been written to a `Sim`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1CitizenThinkSuffixPlan {
    pub composition_digest: [u8; 32],
    pub think: Frame1CitizenThinkPlan,
    pub type_relation: Frame1CitizenTypeRelationProof,
    pub after_local: IdleCitizenPreimage,
    pub journal: Vec<Frame1CitizenThinkSuffixRead>,
    pub restore_mask2_bit8000: bool,
    pub open: Frame1CitizenThinkSuffixOpenRequest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame1CitizenThinkSuffixError {
    Think(Frame1CitizenThinkError),
    TypeContent(ReplayLandSpeedContentError),
    NotThinkSuffix,
    ThinkSuffixRequestMismatch,
    WrongReplayFile,
    TypeBeforeImageMismatch,
    MissingTypeRelationRow { type_index: i32 },
    TypeRelationCycle { type_index: i32 },
    ReachedCaravan,
    ReachedRareCollector,
    UnsupportedHumanSchedule { unit_masks: u32 },
    RestoreNotArmed,
    StalePlan,
}

impl fmt::Display for Frame1CitizenThinkSuffixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "2024 frame-1 Citizen Think suffix refused: {self:?}")
    }
}

impl std::error::Error for Frame1CitizenThinkSuffixError {}

impl From<Frame1CitizenThinkError> for Frame1CitizenThinkSuffixError {
    fn from(value: Frame1CitizenThinkError) -> Self {
        Self::Think(value)
    }
}

impl From<ReplayLandSpeedContentError> for Frame1CitizenThinkSuffixError {
    fn from(value: ReplayLandSpeedContentError) -> Self {
        Self::TypeContent(value)
    }
}

fn relation_proof(
    content: &ReplayLandSpeedContent,
    source_type: i32,
    target_type: i32,
) -> Result<Frame1CitizenTypeRelationProof, Frame1CitizenThinkSuffixError> {
    let mut walked_rows = Vec::new();
    let mut seen = BTreeSet::new();
    let mut type_index = source_type;
    let result = loop {
        if !seen.insert(type_index) {
            return Err(Frame1CitizenThinkSuffixError::TypeRelationCycle { type_index });
        }
        let row = content
            .type_fact(type_index)
            .ok_or(Frame1CitizenThinkSuffixError::MissingTypeRelationRow { type_index })?;
        if row.type_id != type_index {
            return Err(Frame1CitizenThinkSuffixError::TypeBeforeImageMismatch);
        }
        walked_rows.push(row);
        if row.type_id == target_type || row.graft == target_type {
            break true;
        }
        if row.from < 0 {
            break false;
        }
        type_index = row.from;
    };
    Ok(Frame1CitizenTypeRelationProof {
        replay_file_sha256: content.replay_file_sha256(),
        replay_payload_sha256: content.replay_payload_sha256(),
        rules_serialized_sha256: content.rules_serialized_sha256(),
        source_type,
        target_type,
        walked_rows,
        result,
    })
}

fn suffix_request_matches(
    think: &Frame1CitizenThinkPlan,
    request: &Frame1CitizenThinkSuffixRequest,
) -> bool {
    request.authority_revision == think.authority.revision
        && request.authority_digest == think.authority.composition_digest
        && request.continuation_digest == think.continuation.composition_digest
        && request.frame == think.after_local.frame
        && request.unit == think.after_local.unit.handle
        && request.after_think_peasant == think.after_local
        && request.staged_leader_pending == think.staged_leader_pending
        && request.restore_mask2_bit8000
        && think.restore_mask2_bit8000
        && think.after_local.unit_masks2 & 0x8000 == 0
}

fn digest(plan: &Frame1CitizenThinkSuffixPlan) -> [u8; 32] {
    let mut image = b"don-frame1-citizen-think-suffix-v1".to_vec();
    image.extend_from_slice(&plan.think.composition_digest);
    image.extend_from_slice(&plan.type_relation.replay_file_sha256);
    image.extend_from_slice(&plan.type_relation.replay_payload_sha256);
    image.extend_from_slice(&plan.type_relation.rules_serialized_sha256);
    image.extend_from_slice(&plan.type_relation.source_type.to_le_bytes());
    image.extend_from_slice(&plan.type_relation.target_type.to_le_bytes());
    image.extend_from_slice(&(plan.type_relation.walked_rows.len() as u64).to_le_bytes());
    for row in &plan.type_relation.walked_rows {
        image.extend_from_slice(&row.type_id.to_le_bytes());
        image.extend_from_slice(&row.from.to_le_bytes());
        image.extend_from_slice(&row.graft.to_le_bytes());
        image.extend_from_slice(&row.domain.to_le_bytes());
        image.extend_from_slice(&row.unit_flags.to_le_bytes());
        image.extend_from_slice(&row.unit_flags2.to_le_bytes());
    }
    image.push(u8::from(plan.type_relation.result));
    image.extend_from_slice(&plan.after_local.unit_masks.to_le_bytes());
    image.extend_from_slice(&plan.after_local.unit_masks2.to_le_bytes());
    image.push(plan.after_local.idle);
    image.push(plan.after_local.object_flags);
    image.extend_from_slice(&(plan.journal.len() as u64).to_le_bytes());
    for read in &plan.journal {
        match read {
            Frame1CitizenThinkSuffixRead::IsCaravanFalse { unit_flags2 } => {
                image.push(0);
                image.extend_from_slice(&unit_flags2.to_le_bytes());
            }
            Frame1CitizenThinkSuffixRead::IsRareCollectorFalse {
                type_index,
                target_type,
            } => {
                image.push(1);
                image.extend_from_slice(&type_index.to_le_bytes());
                image.extend_from_slice(&target_type.to_le_bytes());
            }
            Frame1CitizenThinkSuffixRead::ReachHumanSetIdle {
                leader_flags,
                unit_masks,
                idle_argument,
            } => {
                image.push(2);
                image.extend_from_slice(&leader_flags.to_le_bytes());
                image.extend_from_slice(&unit_masks.to_le_bytes());
                image.extend_from_slice(&idle_argument.to_le_bytes());
            }
        }
    }
    image.push(u8::from(plan.restore_mask2_bit8000));
    match &plan.open {
        Frame1CitizenThinkSuffixOpenRequest::SetIdle(request) => {
            image.push(0);
            image.extend_from_slice(&request.authority_revision.to_le_bytes());
            image.extend_from_slice(&request.authority_digest);
            image.extend_from_slice(&request.think_plan_digest);
            image.extend_from_slice(&request.frame.to_le_bytes());
            image.extend_from_slice(&request.unit.id.to_le_bytes());
            image.extend_from_slice(&request.unit.generation.to_le_bytes());
            image.push(request.who);
            image.extend_from_slice(&request.o.to_le_bytes());
            image.extend_from_slice(&request.idle.to_le_bytes());
            image.extend_from_slice(&request.after_suffix_local.unit_masks.to_le_bytes());
            image.extend_from_slice(&request.after_suffix_local.unit_masks2.to_le_bytes());
            image.push(request.after_suffix_local.idle);
            image.extend_from_slice(&request.staged_leader_pending.flags_after.to_le_bytes());
            image.push(u8::from(request.restore_mask2_bit8000));
        }
        Frame1CitizenThinkSuffixOpenRequest::LocalPlayer(request) => {
            image.push(1);
            image.extend_from_slice(&request.authority_revision.to_le_bytes());
            image.extend_from_slice(&request.authority_digest);
            image.extend_from_slice(&request.think_plan_digest);
            image.extend_from_slice(&request.frame.to_le_bytes());
            image.extend_from_slice(&request.unit.id.to_le_bytes());
            image.extend_from_slice(&request.unit.generation.to_le_bytes());
            image.extend_from_slice(&request.after_suffix_local.unit_masks.to_le_bytes());
            image.extend_from_slice(&request.after_suffix_local.unit_masks2.to_le_bytes());
            image.extend_from_slice(&request.staged_leader_pending.flags_after.to_le_bytes());
            image.push(u8::from(request.restore_mask2_bit8000));
        }
    }
    sha256(&image)
}

/// Continue the exact Think suffix until its first unowned child.
///
/// The supported golden receiver reaches `Unit::set_idle(0)`.  This function therefore never
/// mutates `set_anim_return`, never commits the staged Leader mirrors, and never discharges the
/// outer `0x8000` restore.  A later SetIdle receipt must be composed before suffix execution may
/// continue or publish atomically.
#[allow(clippy::too_many_arguments)]
pub fn plan_frame1_citizen_think_suffix(
    replay: &Replay,
    setup_entry: &Frame379SetupEntryReceipt,
    post_authority: &Frame1PostCommandAuthority,
    post_command: &Sim,
    set_anim_return: &Sim,
    entry_authority: &GoldenFrame1EntryAuthority,
    think: &Frame1CitizenThinkPlan,
) -> Result<Frame1CitizenThinkSuffixPlan, Frame1CitizenThinkSuffixError> {
    validate_frame1_citizen_think_plan(
        replay,
        setup_entry,
        post_authority,
        post_command,
        set_anim_return,
        entry_authority,
        think,
    )?;
    let Frame1CitizenThinkOpenRequest::ThinkSuffix(request) = &think.open else {
        return Err(Frame1CitizenThinkSuffixError::NotThinkSuffix);
    };
    if !suffix_request_matches(think, request) {
        return Err(Frame1CitizenThinkSuffixError::ThinkSuffixRequestMismatch);
    }
    if !think.restore_mask2_bit8000 || think.after_local.unit_masks2 & 0x8000 != 0 {
        return Err(Frame1CitizenThinkSuffixError::RestoreNotArmed);
    }

    let content = produce_replay_land_speed_content(replay)?;
    if content.replay_file_sha256() != REPLAY_FILE_SHA256 {
        return Err(Frame1CitizenThinkSuffixError::WrongReplayFile);
    }
    let type_index = think.after_local.unit.type_index;
    let type_row = content
        .type_fact(type_index)
        .ok_or(Frame1CitizenThinkSuffixError::MissingTypeRelationRow { type_index })?;
    let live_type = think.after_local.type_facts;
    if type_row.type_id != live_type.type_index
        || type_row.domain != live_type.domain
        || type_row.unit_flags != live_type.unit_flags
        || type_row.unit_flags2 != live_type.unit_flags2
    {
        return Err(Frame1CitizenThinkSuffixError::TypeBeforeImageMismatch);
    }
    if type_row.unit_flags2 & 8 != 0 {
        return Err(Frame1CitizenThinkSuffixError::ReachedCaravan);
    }

    let relation = relation_proof(&content, type_index, RARE_COLLECTOR_TYPE)?;
    let rare_collector = DIRECT_RARE_COLLECTOR_TYPES.contains(&type_index) || relation.result;
    if rare_collector {
        return Err(Frame1CitizenThinkSuffixError::ReachedRareCollector);
    }
    let mut journal = vec![
        Frame1CitizenThinkSuffixRead::IsCaravanFalse {
            unit_flags2: type_row.unit_flags2,
        },
        Frame1CitizenThinkSuffixRead::IsRareCollectorFalse {
            type_index,
            target_type: RARE_COLLECTOR_TYPE,
        },
    ];

    let after = request.after_think_peasant.clone();
    let leader_flags = request.staged_leader_pending.flags_after;
    let open = if leader_flags & LEADER_HUMAN == 0 {
        Frame1CitizenThinkSuffixOpenRequest::LocalPlayer(Frame1CitizenLocalPlayerRequest {
            authority_revision: request.authority_revision,
            authority_digest: request.authority_digest,
            think_plan_digest: think.composition_digest,
            frame: request.frame,
            unit: request.unit,
            after_suffix_local: after.clone(),
            staged_leader_pending: request.staged_leader_pending,
            restore_mask2_bit8000: true,
        })
    } else if after.unit_masks & 0x100 != 0 {
        journal.push(Frame1CitizenThinkSuffixRead::ReachHumanSetIdle {
            leader_flags,
            unit_masks: after.unit_masks,
            idle_argument: 0,
        });
        Frame1CitizenThinkSuffixOpenRequest::SetIdle(Frame1CitizenSetIdleRequest {
            authority_revision: request.authority_revision,
            authority_digest: request.authority_digest,
            think_plan_digest: think.composition_digest,
            frame: request.frame,
            unit: request.unit,
            who: after.unit.who,
            o: after.unit.o,
            idle: 0,
            after_suffix_local: after.clone(),
            staged_leader_pending: request.staged_leader_pending,
            restore_mask2_bit8000: true,
        })
    } else {
        // The parent Think plan admits only the type-50 worker arm, which requires bit 0x100.
        // Keep this explicit in case a stale or independently constructed plan reaches here.
        return Err(Frame1CitizenThinkSuffixError::UnsupportedHumanSchedule {
            unit_masks: after.unit_masks,
        });
    };

    let mut plan = Frame1CitizenThinkSuffixPlan {
        composition_digest: [0; 32],
        think: think.clone(),
        type_relation: relation,
        after_local: after,
        journal,
        restore_mask2_bit8000: true,
        open,
    };
    plan.composition_digest = digest(&plan);
    Ok(plan)
}

/// Recompute the complete source join before a SetIdle continuation consumes this plan.
#[allow(clippy::too_many_arguments)]
pub fn validate_frame1_citizen_think_suffix_plan(
    replay: &Replay,
    setup_entry: &Frame379SetupEntryReceipt,
    post_authority: &Frame1PostCommandAuthority,
    post_command: &Sim,
    set_anim_return: &Sim,
    entry_authority: &GoldenFrame1EntryAuthority,
    plan: &Frame1CitizenThinkSuffixPlan,
) -> Result<(), Frame1CitizenThinkSuffixError> {
    let expected = plan_frame1_citizen_think_suffix(
        replay,
        setup_entry,
        post_authority,
        post_command,
        set_anim_return,
        entry_authority,
        &plan.think,
    )?;
    if &expected != plan {
        return Err(Frame1CitizenThinkSuffixError::StalePlan);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn content_row(type_id: i32, from: i32, graft: i32) -> LandSpeedTypeFacts {
        LandSpeedTypeFacts {
            type_id,
            from,
            graft,
            ..LandSpeedTypeFacts::default()
        }
    }

    #[test]
    fn direct_rare_collector_types_match_the_exact_concrete_fast_path() {
        assert_eq!(DIRECT_RARE_COLLECTOR_TYPES, [61, 62, 400]);
        assert!(!DIRECT_RARE_COLLECTOR_TYPES.contains(&50));
    }

    #[test]
    fn relation_walk_shape_is_identity_graft_then_from() {
        let rows = [
            content_row(50, 51, -1),
            content_row(51, -1, RARE_COLLECTOR_TYPE),
        ];
        let mut index = 50;
        let mut walked = Vec::new();
        let result = loop {
            let row = rows.iter().find(|row| row.type_id == index).unwrap();
            walked.push(row.type_id);
            if row.type_id == RARE_COLLECTOR_TYPE || row.graft == RARE_COLLECTOR_TYPE {
                break true;
            }
            if row.from < 0 {
                break false;
            }
            index = row.from;
        };
        assert!(result);
        assert_eq!(walked, [50, 51]);

        let terminal = content_row(50, -1, -1);
        assert!(terminal.type_id != RARE_COLLECTOR_TYPE);
        assert!(terminal.graft != RARE_COLLECTOR_TYPE);
        assert!(terminal.from < 0);
    }

    #[test]
    fn installed_golden_type50_closes_both_suffix_type_gates() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../ron-data/replays/multi/Playback___2024.02.23_20_49_35__Fri_.rcx");
        if !path.exists() {
            eprintln!("SKIPPED -- NOT A PASS: missing {}", path.display());
            return;
        }
        let replay = Replay::open(&path).expect("installed 2024 witness must decode");
        let content = produce_replay_land_speed_content(&replay).unwrap();
        assert_eq!(content.replay_file_sha256(), REPLAY_FILE_SHA256);
        let type50 = content.type_fact(50).unwrap();
        assert_eq!(type50.unit_flags2, 2);
        assert_eq!(type50.unit_flags2 & 8, 0);
        let rare = relation_proof(&content, 50, RARE_COLLECTOR_TYPE).unwrap();
        assert!(!rare.result);
        assert_eq!(rare.walked_rows.first().unwrap().type_id, 50);
    }
}
