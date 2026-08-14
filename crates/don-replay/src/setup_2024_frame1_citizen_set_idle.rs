//! Exact entry prefix of the golden frame-one Citizen `Unit::set_idle(0)` child.
//!
//! `Unit::think` reaches SetIdle with the complete receiver, staged Leader pending OR, and
//! outer `unit_masks2 |= 0x8000` obligation still detached.  SetIdle's first executable
//! branch reads `UnitTypeData::domain`; type 50 is land, so the very next operation is
//! `Unit::find_goody_box`.  That 593-byte child reads the item registry, WData, fog history,
//! current visibility, and can issue a movement order.  This module exposes that exact child
//! without inventing any animation, Guy, order, path, RNG, or canonical Sim effect.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::systems::leader_unit_think_pending::PreparedUnitThinkLeaderPending;
use don_sim::systems::setup_idle_prefix::{GoldenFrame1EntryAuthority, IdleCitizenPreimage};
use don_sim::tick::Sim;
use don_sim::world::Handle;

use crate::replay::Replay;
use crate::setup_2024_frame1_citizen_think_suffix::{
    validate_frame1_citizen_think_suffix_plan, Frame1CitizenSetIdleRequest,
    Frame1CitizenThinkSuffixError, Frame1CitizenThinkSuffixOpenRequest,
    Frame1CitizenThinkSuffixPlan,
};
use crate::setup_2024_frame379::Frame379SetupEntryReceipt;
use crate::setup_2024_golden_capture::Frame1PostCommandAuthority;
use crate::world_owner_frontier::sha256;

pub const UNIT_SET_IDLE_VA: u32 = 0x005f_6010;
pub const UNIT_SET_IDLE_DOMAIN_READ_VA: u32 = 0x005f_6028;
pub const UNIT_SET_IDLE_FIND_GOODY_CALL_VA: u32 = 0x005f_6034;
pub const UNIT_FIND_GOODY_BOX_VA: u32 = 0x005f_2540;
pub const UNIT_FIND_GOODY_BOX_SIZE: u32 = 593;
pub const PROOF_DOCUMENT: &str = "docs/assembly/replay-2024-frame1-citizen-set-idle.md";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame1CitizenSetIdleRead {
    LandDomain { type_index: i32, domain: i32 },
}

/// Exact first child of the supported golden `Unit::set_idle(0)` invocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1CitizenFindGoodyBoxRequest {
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub think_suffix_digest: [u8; 32],
    pub call_va: u32,
    pub body_va: u32,
    pub frame: i32,
    pub unit: Handle,
    pub who: u8,
    pub o: i16,
    pub type_index: i32,
    /// Stored `SubObjectData +0x10/+0x14` coordinate words.
    pub stored_x: i32,
    pub stored_y: i32,
    pub decoded_x: i32,
    pub decoded_y: i32,
    pub after_set_idle_entry: IdleCitizenPreimage,
    pub staged_leader_pending: PreparedUnitThinkLeaderPending,
    pub restore_mask2_bit8000: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame1CitizenSetIdleOpenRequest {
    FindGoodyBox(Frame1CitizenFindGoodyBoxRequest),
}

/// Detached SetIdle transaction.  No receiver or cross-owner field has been written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1CitizenSetIdlePlan {
    pub composition_digest: [u8; 32],
    pub think_suffix: Frame1CitizenThinkSuffixPlan,
    pub after_local: IdleCitizenPreimage,
    pub journal: Vec<Frame1CitizenSetIdleRead>,
    pub restore_mask2_bit8000: bool,
    pub open: Frame1CitizenSetIdleOpenRequest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame1CitizenSetIdleError {
    ThinkSuffix(Frame1CitizenThinkSuffixError),
    NotSetIdle,
    SetIdleRequestMismatch,
    UnsupportedArgument { idle: i32 },
    UnsupportedDomain { domain: i32 },
    RestoreNotArmed,
    StalePlan,
}

impl fmt::Display for Frame1CitizenSetIdleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "2024 frame-1 Citizen SetIdle refused: {self:?}")
    }
}

impl std::error::Error for Frame1CitizenSetIdleError {}

impl From<Frame1CitizenThinkSuffixError> for Frame1CitizenSetIdleError {
    fn from(value: Frame1CitizenThinkSuffixError) -> Self {
        Self::ThinkSuffix(value)
    }
}

fn set_idle_request_matches(
    suffix: &Frame1CitizenThinkSuffixPlan,
    request: &Frame1CitizenSetIdleRequest,
) -> bool {
    request.authority_revision == suffix.think.authority.revision
        && request.authority_digest == suffix.think.authority.composition_digest
        && request.think_plan_digest == suffix.think.composition_digest
        && request.frame == suffix.after_local.frame
        && request.unit == suffix.after_local.unit.handle
        && request.who == suffix.after_local.unit.who
        && request.o == suffix.after_local.unit.o
        && request.after_suffix_local == suffix.after_local
        && request.staged_leader_pending == suffix.think.staged_leader_pending
        && request.restore_mask2_bit8000
        && suffix.restore_mask2_bit8000
        && suffix.after_local.unit_masks2 & 0x8000 == 0
}

fn digest(plan: &Frame1CitizenSetIdlePlan) -> [u8; 32] {
    let mut image = b"don-frame1-citizen-set-idle-v1".to_vec();
    image.extend_from_slice(&plan.think_suffix.composition_digest);
    image.extend_from_slice(&plan.after_local.unit.handle.id.to_le_bytes());
    image.extend_from_slice(&plan.after_local.unit.handle.generation.to_le_bytes());
    image.extend_from_slice(&plan.after_local.unit_masks.to_le_bytes());
    image.extend_from_slice(&plan.after_local.unit_masks2.to_le_bytes());
    image.push(plan.after_local.idle);
    image.push(plan.after_local.object_flags);
    image.extend_from_slice(&(plan.journal.len() as u64).to_le_bytes());
    for read in &plan.journal {
        match read {
            Frame1CitizenSetIdleRead::LandDomain { type_index, domain } => {
                image.push(0);
                image.extend_from_slice(&type_index.to_le_bytes());
                image.extend_from_slice(&domain.to_le_bytes());
            }
        }
    }
    image.push(u8::from(plan.restore_mask2_bit8000));
    match &plan.open {
        Frame1CitizenSetIdleOpenRequest::FindGoodyBox(request) => {
            image.push(0);
            image.extend_from_slice(&request.authority_revision.to_le_bytes());
            image.extend_from_slice(&request.authority_digest);
            image.extend_from_slice(&request.think_suffix_digest);
            image.extend_from_slice(&request.call_va.to_le_bytes());
            image.extend_from_slice(&request.body_va.to_le_bytes());
            image.extend_from_slice(&request.frame.to_le_bytes());
            image.extend_from_slice(&request.unit.id.to_le_bytes());
            image.extend_from_slice(&request.unit.generation.to_le_bytes());
            image.push(request.who);
            image.extend_from_slice(&request.o.to_le_bytes());
            image.extend_from_slice(&request.type_index.to_le_bytes());
            image.extend_from_slice(&request.stored_x.to_le_bytes());
            image.extend_from_slice(&request.stored_y.to_le_bytes());
            image.extend_from_slice(&request.decoded_x.to_le_bytes());
            image.extend_from_slice(&request.decoded_y.to_le_bytes());
            image.extend_from_slice(&request.after_set_idle_entry.unit_masks.to_le_bytes());
            image.extend_from_slice(&request.after_set_idle_entry.unit_masks2.to_le_bytes());
            image.push(request.after_set_idle_entry.idle);
            image.extend_from_slice(&request.staged_leader_pending.flags_after.to_le_bytes());
            image.push(u8::from(request.restore_mask2_bit8000));
        }
    }
    sha256(&image)
}

/// Consume the exact SetIdle request and execute its closed entry prefix.
///
/// Retail performs no receiver write before `Unit::find_goody_box`.  The returned plan is
/// therefore bit-identical to the suffix's detached receiver, including complete Guys,
/// empty-order/path attestations, unchanged RNG, staged Leader mirrors, and armed restore.
#[allow(clippy::too_many_arguments)]
pub fn plan_frame1_citizen_set_idle(
    replay: &Replay,
    setup_entry: &Frame379SetupEntryReceipt,
    post_authority: &Frame1PostCommandAuthority,
    post_command: &Sim,
    set_anim_return: &Sim,
    entry_authority: &GoldenFrame1EntryAuthority,
    suffix: &Frame1CitizenThinkSuffixPlan,
) -> Result<Frame1CitizenSetIdlePlan, Frame1CitizenSetIdleError> {
    validate_frame1_citizen_think_suffix_plan(
        replay,
        setup_entry,
        post_authority,
        post_command,
        set_anim_return,
        entry_authority,
        suffix,
    )?;
    let Frame1CitizenThinkSuffixOpenRequest::SetIdle(request) = &suffix.open else {
        return Err(Frame1CitizenSetIdleError::NotSetIdle);
    };
    if !set_idle_request_matches(suffix, request) {
        return Err(Frame1CitizenSetIdleError::SetIdleRequestMismatch);
    }
    if request.idle != 0 {
        return Err(Frame1CitizenSetIdleError::UnsupportedArgument { idle: request.idle });
    }
    if !suffix.restore_mask2_bit8000 || suffix.after_local.unit_masks2 & 0x8000 != 0 {
        return Err(Frame1CitizenSetIdleError::RestoreNotArmed);
    }

    let after = request.after_suffix_local.clone();
    let domain = after.type_facts.domain;
    if domain != 0 {
        return Err(Frame1CitizenSetIdleError::UnsupportedDomain { domain });
    }
    let journal = vec![Frame1CitizenSetIdleRead::LandDomain {
        type_index: after.unit.type_index,
        domain,
    }];
    let open = Frame1CitizenSetIdleOpenRequest::FindGoodyBox(Frame1CitizenFindGoodyBoxRequest {
        authority_revision: request.authority_revision,
        authority_digest: request.authority_digest,
        think_suffix_digest: suffix.composition_digest,
        call_va: UNIT_SET_IDLE_FIND_GOODY_CALL_VA,
        body_va: UNIT_FIND_GOODY_BOX_VA,
        frame: request.frame,
        unit: request.unit,
        who: request.who,
        o: request.o,
        type_index: after.unit.type_index,
        stored_x: suffix.think.authority.stored_x,
        stored_y: suffix.think.authority.stored_y,
        decoded_x: suffix.think.authority.decoded_x,
        decoded_y: suffix.think.authority.decoded_y,
        after_set_idle_entry: after.clone(),
        staged_leader_pending: request.staged_leader_pending,
        restore_mask2_bit8000: true,
    });
    let mut plan = Frame1CitizenSetIdlePlan {
        composition_digest: [0; 32],
        think_suffix: suffix.clone(),
        after_local: after,
        journal,
        restore_mask2_bit8000: true,
        open,
    };
    plan.composition_digest = digest(&plan);
    Ok(plan)
}

/// Recompute the complete source chain before a FindGoody continuation consumes this plan.
#[allow(clippy::too_many_arguments)]
pub fn validate_frame1_citizen_set_idle_plan(
    replay: &Replay,
    setup_entry: &Frame379SetupEntryReceipt,
    post_authority: &Frame1PostCommandAuthority,
    post_command: &Sim,
    set_anim_return: &Sim,
    entry_authority: &GoldenFrame1EntryAuthority,
    plan: &Frame1CitizenSetIdlePlan,
) -> Result<(), Frame1CitizenSetIdleError> {
    let expected = plan_frame1_citizen_set_idle(
        replay,
        setup_entry,
        post_authority,
        post_command,
        set_anim_return,
        entry_authority,
        &plan.think_suffix,
    )?;
    if &expected != plan {
        return Err(Frame1CitizenSetIdleError::StalePlan);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_entry_reaches_find_goody_before_any_receiver_write() {
        assert_eq!(UNIT_SET_IDLE_VA, 0x005f_6010);
        assert_eq!(UNIT_SET_IDLE_DOMAIN_READ_VA, 0x005f_6028);
        assert_eq!(UNIT_SET_IDLE_FIND_GOODY_CALL_VA, 0x005f_6034);
        assert_eq!(UNIT_FIND_GOODY_BOX_VA, 0x005f_2540);
        assert_eq!(UNIT_FIND_GOODY_BOX_SIZE, 593);
    }
}
