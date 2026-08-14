//! Atomic continuation of the golden frame-one Citizen idle prefix through SetAnim.
//!
//! The prefix and SetAnim capture are both detached authorities. This module composes them,
//! installs only the receipt-owned Guys/RNG into the detached receiver, executes the two exact
//! local `do_idle -> check_idle` writes reached by the golden `idle == 1` case, and stops before
//! `Unit::think`. It never mutates a canonical [`don_sim::tick::Sim`].

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::systems::groups_guys::UnitGuys;
use don_sim::systems::setup_idle_prefix::{
    plan_frame1_citizen_idle_prefix, GoldenFrame1EntryAuthority, IdleCheckIdleBoundary,
    IdleCheckIdleFactsRequest, IdleCitizenPrefixError, IdleCitizenPrefixPlan, IdleCitizenPreimage,
    IdleCitizenThinkRequest, IdleOpenRequest,
};

use crate::setup_2024_frame1_set_anim_capture::{
    validate_frame1_idle_set_anim_receipt, Frame1IdleSetAnimError, Frame1IdleSetAnimReceipt,
};
use crate::world_owner_frontier::sha256;

pub const UNIT_DO_IDLE_COLLIDE_CLEAR_VA: u32 = 0x0060_dd51;
pub const UNIT_CHECK_IDLE_ENTRY_VA: u32 = 0x0060_32c0;
pub const UNIT_CHECK_IDLE_IDLE_INCREMENT_VA: u32 = 0x0060_32f8;
pub const UNIT_THINK_CALL_VA: u32 = 0x0060_dd5f;
pub const PROOF_DOCUMENT: &str = "docs/assembly/replay-2024-frame1-idle-set-anim-continuation.md";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame1CitizenIdleContinuationWrite {
    AcceptSetAnimReceipt {
        composition_digest: [u8; 32],
        before_sim_sha256: [u8; 32],
        after_sim_sha256: [u8; 32],
    },
    ClearCollide {
        before: i16,
        after: i16,
    },
    IncrementIdle {
        before: u8,
        after: u8,
    },
}

/// Detached state after consuming one exact SetAnim receipt.
///
/// `restore_mask2_bit8000` must remain true and the bit remains cleared in `after_local`.
/// The next continuation may restore it only after the complete Think child returns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1CitizenIdleContinuationPlan {
    pub composition_digest: [u8; 32],
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub prefix: IdleCitizenPrefixPlan,
    pub set_anim_receipt: Frame1IdleSetAnimReceipt,
    pub after_local: IdleCitizenPreimage,
    pub journal: Vec<Frame1CitizenIdleContinuationWrite>,
    pub restore_mask2_bit8000: bool,
    pub open: IdleOpenRequest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame1CitizenIdleContinuationError {
    Prefix(IdleCitizenPrefixError),
    StalePrefixPlan,
    NotSetAnim,
    AuthorityMismatch,
    SetAnim(Frame1IdleSetAnimError),
    ReceiverMismatch,
    RestoreNotArmed,
    StaleContinuationPlan,
}

impl fmt::Display for Frame1CitizenIdleContinuationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "2024 frame-1 Citizen idle continuation refused: {self:?}"
        )
    }
}

impl std::error::Error for Frame1CitizenIdleContinuationError {}

impl From<IdleCitizenPrefixError> for Frame1CitizenIdleContinuationError {
    fn from(value: IdleCitizenPrefixError) -> Self {
        Self::Prefix(value)
    }
}

impl From<Frame1IdleSetAnimError> for Frame1CitizenIdleContinuationError {
    fn from(value: Frame1IdleSetAnimError) -> Self {
        Self::SetAnim(value)
    }
}

fn append_guys(image: &mut Vec<u8>, guys: &UnitGuys) {
    image.extend_from_slice(&(guys.guys.len() as u64).to_le_bytes());
    image.extend_from_slice(&guys.size.to_le_bytes());
    image.extend_from_slice(&guys.increment.to_le_bytes());
    image.push(guys.flags);
    image.push(guys.guy_mark as u8);
    for guy in &guys.guys {
        image.push(u8::from(guy.is_some()));
    }
    for guy in guys.guys.iter().flatten() {
        image.extend_from_slice(&guy.walk_bytes());
    }
}

fn append_preimage(image: &mut Vec<u8>, preimage: &IdleCitizenPreimage) {
    image.extend_from_slice(&preimage.frame.to_le_bytes());
    image.extend_from_slice(&preimage.unit.handle.id.to_le_bytes());
    image.extend_from_slice(&preimage.unit.handle.generation.to_le_bytes());
    image.push(preimage.unit.who);
    image.extend_from_slice(&preimage.unit.o.to_le_bytes());
    image.extend_from_slice(&preimage.unit.uid.to_le_bytes());
    image.extend_from_slice(&preimage.unit.type_index.to_le_bytes());
    image.extend_from_slice(&preimage.type_facts.type_index.to_le_bytes());
    image.extend_from_slice(&preimage.type_facts.domain.to_le_bytes());
    image.extend_from_slice(&preimage.type_facts.unit_flags.to_le_bytes());
    image.extend_from_slice(&preimage.type_facts.unit_flags2.to_le_bytes());
    image.extend_from_slice(&preimage.type_facts.role.to_le_bytes());
    image.push(preimage.object_flags);
    image.extend_from_slice(&preimage.inside_up.to_le_bytes());
    image.push(u8::from(preimage.orders_empty));
    image.push(u8::from(preimage.path_empty));
    image.push(preimage.recharging);
    image.push(preimage.full);
    image.push(preimage.waiting);
    image.extend_from_slice(&preimage.healing.to_le_bytes());
    image.extend_from_slice(&preimage.mana_burn.to_le_bytes());
    image.extend_from_slice(&preimage.num_queued.to_le_bytes());
    image.extend_from_slice(&preimage.attrition.to_le_bytes());
    image.push(u8::from(preimage.healing_has_damage));
    image.extend_from_slice(&preimage.unit_masks.to_le_bytes());
    image.extend_from_slice(&preimage.unit_masks2.to_le_bytes());
    image.extend_from_slice(&preimage.spell_time.to_le_bytes());
    image.push(preimage.safe as u8);
    image.extend_from_slice(&preimage.collide_frame.to_le_bytes());
    image.extend_from_slice(&preimage.collide.to_le_bytes());
    image.push(preimage.idle);
    image.push(preimage.worker_stance as u8);
    append_guys(image, &preimage.guys);
    image.extend_from_slice(&preimage.random_state.to_le_bytes());
}

fn continuation_digest(
    prefix: &IdleCitizenPrefixPlan,
    receipt: &Frame1IdleSetAnimReceipt,
    after_local: &IdleCitizenPreimage,
    journal: &[Frame1CitizenIdleContinuationWrite],
    open: &IdleOpenRequest,
) -> [u8; 32] {
    let mut image = b"don-frame1-citizen-idle-after-set-anim-v1".to_vec();
    image.extend_from_slice(&prefix.authority_revision.to_le_bytes());
    image.extend_from_slice(&prefix.authority_digest);
    image.extend_from_slice(&receipt.composition_digest);
    image.push(u8::from(prefix.restore_mask2_bit8000));
    append_preimage(&mut image, &prefix.before);
    append_preimage(&mut image, &prefix.after_local);
    append_preimage(&mut image, after_local);
    image.extend_from_slice(&(journal.len() as u64).to_le_bytes());
    for write in journal {
        match write {
            Frame1CitizenIdleContinuationWrite::AcceptSetAnimReceipt {
                composition_digest,
                before_sim_sha256,
                after_sim_sha256,
            } => {
                image.push(0);
                image.extend_from_slice(composition_digest);
                image.extend_from_slice(before_sim_sha256);
                image.extend_from_slice(after_sim_sha256);
            }
            Frame1CitizenIdleContinuationWrite::ClearCollide { before, after } => {
                image.push(1);
                image.extend_from_slice(&before.to_le_bytes());
                image.extend_from_slice(&after.to_le_bytes());
            }
            Frame1CitizenIdleContinuationWrite::IncrementIdle { before, after } => {
                image.push(2);
                image.push(*before);
                image.push(*after);
            }
        }
    }
    match open {
        IdleOpenRequest::CheckIdleFacts(request) => {
            image.push(0);
            image.extend_from_slice(&request.authority_revision.to_le_bytes());
            image.extend_from_slice(&request.authority_digest);
            image.extend_from_slice(&request.set_anim_receipt_digest);
            image.extend_from_slice(&request.frame.to_le_bytes());
            image.extend_from_slice(&request.unit.id.to_le_bytes());
            image.extend_from_slice(&request.unit.generation.to_le_bytes());
            image.push(request.who);
            image.extend_from_slice(&request.o.to_le_bytes());
            image.push(match request.boundary {
                IdleCheckIdleBoundary::NonGoldenIdle => 0,
                IdleCheckIdleBoundary::EntrenchFacts => 1,
                IdleCheckIdleBoundary::ObjectIdleTypeQuery => 2,
            });
            append_preimage(&mut image, &request.check_idle_entry);
            image.push(u8::from(request.restore_mask2_bit8000));
        }
        IdleOpenRequest::Think(request) => {
            image.push(1);
            image.extend_from_slice(&request.authority_revision.to_le_bytes());
            image.extend_from_slice(&request.authority_digest);
            image.extend_from_slice(&request.set_anim_receipt_digest);
            image.extend_from_slice(&request.frame.to_le_bytes());
            image.extend_from_slice(&request.unit.id.to_le_bytes());
            image.extend_from_slice(&request.unit.generation.to_le_bytes());
            image.push(request.who);
            image.extend_from_slice(&request.o.to_le_bytes());
            append_preimage(&mut image, &request.after_check_idle);
            image.push(u8::from(request.restore_mask2_bit8000));
        }
        _ => image.push(0xff),
    }
    sha256(&image)
}

/// Consume the exact adjacent SetAnim receipt and continue the detached golden Citizen path.
///
/// The original prefix is recomputed from its complete before-image and authority before its
/// public fields are trusted. No write reaches a Sim: every refusal discards the merged Guys,
/// RNG, collide and idle values together. The exact golden path has idle 1, no entrench bit and
/// object idle bit 8 already installed, so `check_idle` performs only idle `1 -> 2` and reaches
/// the typed Think boundary. Every other shape stops at `CheckIdleFacts` before check_idle writes.
pub fn continue_frame1_citizen_idle_after_set_anim(
    authority: &GoldenFrame1EntryAuthority,
    prefix: &IdleCitizenPrefixPlan,
    receipt: &Frame1IdleSetAnimReceipt,
) -> Result<Frame1CitizenIdleContinuationPlan, Frame1CitizenIdleContinuationError> {
    let expected_prefix = plan_frame1_citizen_idle_prefix(authority, &prefix.before)?;
    if &expected_prefix != prefix {
        return Err(Frame1CitizenIdleContinuationError::StalePrefixPlan);
    }
    let IdleOpenRequest::SetAnim(request) = &prefix.open else {
        return Err(Frame1CitizenIdleContinuationError::NotSetAnim);
    };
    if prefix.authority_revision != authority.revision
        || prefix.authority_digest != authority.composition_digest
        || request.authority_revision != authority.revision
        || request.authority_digest != authority.composition_digest
    {
        return Err(Frame1CitizenIdleContinuationError::AuthorityMismatch);
    }
    validate_frame1_idle_set_anim_receipt(request, receipt)?;
    if prefix.after_local.unit.handle != request.unit
        || prefix.after_local.unit.who != request.who
        || prefix.after_local.unit.o != request.o
        || prefix.after_local.unit.type_index != 50
        || prefix.after_local.guys != receipt.guys_before
        || prefix.after_local.random_state != receipt.random_before
    {
        return Err(Frame1CitizenIdleContinuationError::ReceiverMismatch);
    }
    if !prefix.restore_mask2_bit8000 || prefix.after_local.unit_masks2 & 0x8000 != 0 {
        return Err(Frame1CitizenIdleContinuationError::RestoreNotArmed);
    }

    let mut after = prefix.after_local.clone();
    after.guys = receipt.guys_after.clone();
    after.random_state = receipt.random_after;
    let mut journal = vec![Frame1CitizenIdleContinuationWrite::AcceptSetAnimReceipt {
        composition_digest: receipt.composition_digest,
        before_sim_sha256: receipt.before_sim_sha256,
        after_sim_sha256: receipt.after_sim_sha256,
    }];
    let collide_before = after.collide;
    after.collide = 0;
    journal.push(Frame1CitizenIdleContinuationWrite::ClearCollide {
        before: collide_before,
        after: 0,
    });

    let boundary = if after.idle != 1 {
        Some(IdleCheckIdleBoundary::NonGoldenIdle)
    } else if after.unit_masks2 & 0x800 != 0 {
        Some(IdleCheckIdleBoundary::EntrenchFacts)
    } else if after.object_flags & 8 == 0 {
        Some(IdleCheckIdleBoundary::ObjectIdleTypeQuery)
    } else {
        None
    };
    let open = if let Some(boundary) = boundary {
        IdleOpenRequest::CheckIdleFacts(IdleCheckIdleFactsRequest {
            authority_revision: authority.revision,
            authority_digest: authority.composition_digest,
            set_anim_receipt_digest: receipt.composition_digest,
            frame: after.frame,
            unit: after.unit.handle,
            who: after.unit.who,
            o: after.unit.o,
            boundary,
            check_idle_entry: after.clone(),
            restore_mask2_bit8000: true,
        })
    } else {
        let idle_before = after.idle;
        after.idle = after.idle.wrapping_add(1);
        debug_assert_eq!(after.idle, 2);
        journal.push(Frame1CitizenIdleContinuationWrite::IncrementIdle {
            before: idle_before,
            after: after.idle,
        });
        IdleOpenRequest::Think(IdleCitizenThinkRequest {
            authority_revision: authority.revision,
            authority_digest: authority.composition_digest,
            set_anim_receipt_digest: receipt.composition_digest,
            frame: after.frame,
            unit: after.unit.handle,
            who: after.unit.who,
            o: after.unit.o,
            after_check_idle: after.clone(),
            restore_mask2_bit8000: true,
        })
    };
    let composition_digest = continuation_digest(prefix, receipt, &after, &journal, &open);
    Ok(Frame1CitizenIdleContinuationPlan {
        composition_digest,
        authority_revision: authority.revision,
        authority_digest: authority.composition_digest,
        prefix: prefix.clone(),
        set_anim_receipt: receipt.clone(),
        after_local: after,
        journal,
        restore_mask2_bit8000: true,
        open,
    })
}

/// Recompute and validate a detached continuation before a later Think binder consumes it.
///
/// This revalidates both source authorities and compares every published field, including the
/// ordered write journal and typed open request. A caller can therefore reject a mutated plan
/// without applying any part of it to canonical state.
pub fn validate_frame1_citizen_idle_continuation(
    authority: &GoldenFrame1EntryAuthority,
    plan: &Frame1CitizenIdleContinuationPlan,
) -> Result<(), Frame1CitizenIdleContinuationError> {
    let expected = continue_frame1_citizen_idle_after_set_anim(
        authority,
        &plan.prefix,
        &plan.set_anim_receipt,
    )?;
    if &expected != plan {
        return Err(Frame1CitizenIdleContinuationError::StaleContinuationPlan);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use don_sim::systems::groups_guys::{GuyData, UnitGuys};
    use don_sim::systems::setup_idle_prefix::{
        GoldenFrame1BuildIdentity, GoldenFrame1EntrySource, GoldenFrame1UnitIdentity,
        IdleCitizenTypeFacts,
    };
    use don_sim::systems::unit_inctime::SUPPORTED_RETAIL_EXE_SHA256;
    use don_sim::world::Handle;

    use crate::setup_2024_frame1_set_anim_capture::{
        receipt_composition_digest, Frame1CapturedGuySetAnimCall, Frame1IdleSetAnimCaptureSource,
    };
    use crate::setup_2024_frame379::REPLAY_FILE_SHA256;

    fn authority() -> GoldenFrame1EntryAuthority {
        GoldenFrame1EntryAuthority {
            revision: 9,
            composition_digest: [9; 32],
            source: GoldenFrame1EntrySource::SupportedRetailPostCommandAdjacentCapture,
            executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
            replay_file_sha256: REPLAY_FILE_SHA256,
            post_command_sim_sha256: [7; 32],
            frame: 1,
            random_state: 123,
            center: GoldenFrame1BuildIdentity {
                who: 0,
                o: 2000,
                uid: 1,
                type_index: 414,
            },
            market: GoldenFrame1BuildIdentity {
                who: 0,
                o: 2001,
                uid: 1,
                type_index: 436,
            },
            units: std::array::from_fn(|ordinal| GoldenFrame1UnitIdentity {
                handle: Handle {
                    id: ordinal as u32,
                    generation: 4,
                },
                who: 0,
                o: ordinal as i16,
                uid: (ordinal + 1) as u16,
                type_index: match ordinal {
                    0 => 69,
                    1 | 2 => 62,
                    _ => 50,
                },
            }),
        }
    }

    fn citizen(idle: u8, masks2: u32, object_flags: u8) -> IdleCitizenPreimage {
        let authority = authority();
        let guy = GuyData {
            ty: 50,
            who: 0,
            o: 3,
            guy_num: 0,
            hold_attack: 7,
            ..GuyData::default()
        };
        IdleCitizenPreimage {
            frame: 1,
            unit: authority.units[3],
            type_facts: IdleCitizenTypeFacts {
                type_index: 50,
                domain: 0,
                unit_flags: 0x1881,
                unit_flags2: 2,
                role: 0x40300,
            },
            object_flags,
            inside_up: -1,
            orders_empty: true,
            path_empty: true,
            recharging: 0,
            full: 0,
            waiting: 0,
            healing: 0,
            mana_burn: 0,
            num_queued: 0,
            attrition: 0,
            healing_has_damage: false,
            unit_masks: 0x1234_5678,
            unit_masks2: masks2,
            spell_time: 4,
            safe: 2,
            collide_frame: -100,
            collide: 7,
            idle,
            worker_stance: 1,
            guys: UnitGuys {
                guys: vec![Some(guy)],
                size: 1,
                increment: 1,
                flags: 0,
                guy_mark: 1,
            },
            random_state: 123,
        }
    }

    fn receipt(prefix: &IdleCitizenPrefixPlan) -> Frame1IdleSetAnimReceipt {
        let IdleOpenRequest::SetAnim(request) = &prefix.open else {
            panic!("prefix did not reach SetAnim")
        };
        let mut entry = request.guys_before.clone();
        entry.guys[0].as_mut().unwrap().hold_attack = 0;
        let mut returned = entry.clone();
        returned.guys[0].as_mut().unwrap().cur_time = 1;
        let mut receipt = Frame1IdleSetAnimReceipt {
            composition_digest: [0; 32],
            capture_revision: 10,
            source: Frame1IdleSetAnimCaptureSource::SupportedRetailUnitAndGuyCallTrace,
            replay_file_sha256: REPLAY_FILE_SHA256,
            executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
            request: request.clone(),
            setup_ordinal: 3,
            row: 3,
            type_index: 50,
            before_sim_sha256: [1; 32],
            after_sim_sha256: [2; 32],
            guys_before: request.guys_before.clone(),
            guys_after: returned.clone(),
            random_before: request.random_before,
            random_after: request.random_before,
            calls: vec![Frame1CapturedGuySetAnimCall {
                guy_index: 0,
                guys_at_child_entry: entry,
                guys_at_child_return: returned,
                random_before: request.random_before,
                random_after: request.random_before,
                random_draws: 0,
            }],
        };
        receipt.composition_digest = receipt_composition_digest(&receipt);
        receipt
    }

    fn plan_for(before: IdleCitizenPreimage) -> IdleCitizenPrefixPlan {
        plan_frame1_citizen_idle_prefix(&authority(), &before).unwrap()
    }

    #[test]
    fn golden_receipt_reaches_think_with_restore_still_armed() {
        let prefix = plan_for(citizen(1, 0x8010, 0x99));
        let receipt = receipt(&prefix);
        let plan =
            continue_frame1_citizen_idle_after_set_anim(&authority(), &prefix, &receipt).unwrap();
        let IdleOpenRequest::Think(request) = &plan.open else {
            panic!("expected Think, got {:?}", plan.open)
        };
        assert_eq!(plan.after_local.guys, receipt.guys_after);
        assert_eq!(plan.after_local.random_state, receipt.random_after);
        assert_eq!(plan.after_local.collide, 0);
        assert_eq!(plan.after_local.idle, 2);
        assert_eq!(plan.after_local.unit_masks2, 0);
        assert!(plan.restore_mask2_bit8000);
        assert!(request.restore_mask2_bit8000);
        assert_eq!(request.after_check_idle, plan.after_local);
        assert_eq!(plan.journal.len(), 3);
    }

    #[test]
    fn non_golden_check_idle_shapes_stop_before_any_check_idle_write() {
        for (before, expected) in [
            (
                citizen(2, 0x8010, 0x99),
                IdleCheckIdleBoundary::NonGoldenIdle,
            ),
            (
                citizen(1, 0x8810, 0x99),
                IdleCheckIdleBoundary::EntrenchFacts,
            ),
            (
                citizen(1, 0x8010, 0x81),
                IdleCheckIdleBoundary::ObjectIdleTypeQuery,
            ),
        ] {
            let prefix = plan_for(before);
            let receipt = receipt(&prefix);
            let plan = continue_frame1_citizen_idle_after_set_anim(&authority(), &prefix, &receipt)
                .unwrap();
            let IdleOpenRequest::CheckIdleFacts(request) = &plan.open else {
                panic!("expected CheckIdleFacts, got {:?}", plan.open)
            };
            assert_eq!(request.boundary, expected);
            assert_eq!(request.check_idle_entry.idle, prefix.after_local.idle);
            assert_eq!(request.check_idle_entry.collide, 0);
            assert_eq!(plan.journal.len(), 2);
        }
    }

    #[test]
    fn stale_prefix_receipt_and_restore_refuse_atomically() {
        let prefix = plan_for(citizen(1, 0x8010, 0x99));
        let bound_receipt = receipt(&prefix);

        let mut stale_prefix = prefix.clone();
        stale_prefix.after_local.collide = 12;
        assert_eq!(
            continue_frame1_citizen_idle_after_set_anim(
                &authority(),
                &stale_prefix,
                &bound_receipt,
            ),
            Err(Frame1CitizenIdleContinuationError::StalePrefixPlan)
        );

        let mut stale_receipt = bound_receipt.clone();
        stale_receipt.random_after += 1;
        assert!(matches!(
            continue_frame1_citizen_idle_after_set_anim(&authority(), &prefix, &stale_receipt,),
            Err(Frame1CitizenIdleContinuationError::SetAnim(_))
        ));

        let no_restore = plan_for(citizen(1, 0x10, 0x99));
        let no_restore_receipt = receipt(&no_restore);
        assert_eq!(
            continue_frame1_citizen_idle_after_set_anim(
                &authority(),
                &no_restore,
                &no_restore_receipt,
            ),
            Err(Frame1CitizenIdleContinuationError::RestoreNotArmed)
        );
    }

    #[test]
    fn continuation_validator_rejects_any_published_mutation() {
        let prefix = plan_for(citizen(1, 0x8010, 0x99));
        let receipt = receipt(&prefix);
        let plan =
            continue_frame1_citizen_idle_after_set_anim(&authority(), &prefix, &receipt).unwrap();
        validate_frame1_citizen_idle_continuation(&authority(), &plan).unwrap();

        let mut stale = plan;
        stale.after_local.idle = 3;
        assert_eq!(
            validate_frame1_citizen_idle_continuation(&authority(), &stale),
            Err(Frame1CitizenIdleContinuationError::StaleContinuationPlan)
        );
    }
}
