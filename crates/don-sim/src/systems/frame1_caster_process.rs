// SPDX-License-Identifier: GPL-3.0-or-later
//! Adjacent-call authority for the golden replay's frame-one Scout caster pass.
//!
//! The supported Scout reaches `Caster::process_spells` from `Unit::process` at
//! `0x00610DD7`.  This module does not infer a fresh caster from static type data and it does
//! not treat [`Vec`] allocation history as retail state.  Instead, a retail call-boundary
//! capture owns the complete logical image of `Array<ActiveSpell>` on both sides of the call:
//! length, allocated size, increment, flags, cursor, and every ordered 12-byte value.
//!
//! Only one body is admitted here: a captured empty array whose adjacent entry/exit envelope
//! is unchanged.  A nonempty array is a typed child residual.  The mount validates everything
//! before publication and leaves its caller-owned projection byte-for-byte unchanged on every
//! error or residual.

use crate::systems::casters_animals::ActiveSpell;
use crate::systems::unit_inctime::SUPPORTED_RETAIL_EXE_SHA256;
use crate::world::Handle;

pub const UNIT_PROCESS_VA: u32 = 0x0061_0bc0;
pub const CASTER_DISPATCH_VA: u32 = 0x0061_0dd7;
pub const CASTER_PROCESS_SPELLS_VA: u32 = 0x0073_9ad0;
pub const CASTER_PROCESS_SPELLS_BYTES: usize = 481;
pub const FRAME_ONE: i32 = 1;
pub const SETUP_SCOUT_WHO: u8 = 0;
pub const SETUP_SCOUT_O: i16 = 0;
/// Base type selected by the supported setup call. The live effective type remains an explicit
/// source join because `current_upgrade` can replace this row before frame one.
pub const SETUP_SCOUT_BASE_TYPE: i32 = 69;
pub const SETUP_SCOUT_BASE_UNIT_FLAGS2: u32 = 18;
pub const UNIT_FLAGS2_CASTER: u32 = 0x02;
pub const UNIT_FLAGS2_SPECIAL: u32 = 0x10;
pub const UNIT_FLAGS2_HERO: u32 = 0x20;
pub const PROCESS_SPELLS_NORMAL_MODE: i32 = 0;

/// SHA-256 of the selected 2024 Great Lakes replay file.
pub const SUPPORTED_GOLDEN_REPLAY_SHA256: [u8; 32] = [
    0x16, 0x90, 0x43, 0x1a, 0x5e, 0xf1, 0x9b, 0x38, 0xa3, 0x42, 0x5d, 0x3d, 0xd7, 0x31, 0x1e, 0x8e,
    0x83, 0xca, 0x0d, 0x27, 0xc5, 0x6f, 0xab, 0xe4, 0x9d, 0x77, 0x6a, 0x9f, 0x14, 0x21, 0xb2, 0x51,
];

/// Exact request opened by the frame-one `Unit::process` prefix.
///
/// `authority_revision` and `authority_digest` bind this request to the independently
/// captured post-command frame-one chronology.  A setup-frame receipt is not admissible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CasterProcessSpellsRequest {
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub frame: i32,
    pub unit: Handle,
    pub who: u8,
    pub o: i16,
    pub caster_index: i16,
    pub mode: i32,
}

/// PDB-exact logical fields of `Array<ActiveSpell>` (28 bytes in the retail process).
///
/// The process pointer at array `+0x10` is deliberately not persisted.  Its pointee values,
/// in order, are persisted in `entries`.  `length`, `size`, `increment`, `flags`, and
/// `cur_index` are the fields at `+0x04/+0x08/+0x0c/+0x14/+0x18` respectively.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveSpellArrayImage {
    pub length: i32,
    pub size: i32,
    pub increment: i16,
    pub flags: u8,
    pub cur_index: i32,
    pub entries: Vec<ActiveSpell>,
}

impl ActiveSpellArrayImage {
    fn validate(&self) -> Result<(), ActiveSpellArrayShapeError> {
        if self.length < 0 {
            return Err(ActiveSpellArrayShapeError::NegativeLength(self.length));
        }
        if self.size < 0 {
            return Err(ActiveSpellArrayShapeError::NegativeSize(self.size));
        }
        if self.length > self.size {
            return Err(ActiveSpellArrayShapeError::LengthExceedsSize {
                length: self.length,
                size: self.size,
            });
        }
        if usize::try_from(self.length).ok() != Some(self.entries.len()) {
            return Err(ActiveSpellArrayShapeError::EntryCountMismatch {
                length: self.length,
                entries: self.entries.len(),
            });
        }
        Ok(())
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveSpellArrayShapeError {
    NegativeLength(i32),
    NegativeSize(i32),
    LengthExceedsSize { length: i32, size: i32 },
    EntryCountMismatch { length: i32, entries: usize },
}

/// Canonical child-local projection at one side of the adjacent retail call.
///
/// `unit_masks` is `UnitData+0x68`, the word Ambush and Forced March expiry can clear.
/// `spell_visibility_dirty` is `[0x00C06204]+0x90`, set after retail verifies spell flags.
/// The main RNG is included to prove that the admitted empty call consumed no draws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1CasterLocalEnvelope {
    pub frame: i32,
    pub unit: Handle,
    pub who: u8,
    pub o: i16,
    pub uid: u16,
    pub type_index: i32,
    pub type_unit_flags2: u32,
    pub caster_index: i16,
    pub object_header_flags: u8,
    pub unit_masks: u32,
    pub unit_masks2: u32,
    pub spell_visibility_dirty: i32,
    pub random_state: i32,
    pub active_spells: ActiveSpellArrayImage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame1CasterCaptureSource {
    /// Instrumentation sampled the supported retail process immediately before and after the
    /// single `call 0x00739AD0` at `0x00610DD7`.
    SupportedRetailAdjacentUnitProcessCall,
}

/// Exact setup-member/type join used by the adjacent call capture.
///
/// The replay proves base Scout 69. It does not by itself prove the live effective row after
/// `current_upgrade`, so `effective_type_index` and its complete dispatch word are retained
/// separately and bound to the canonical setup member authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame1ScoutTypeJoinAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub base_setup_type: i32,
    pub effective_type_index: i32,
    pub effective_type_unit_flags2: u32,
}

/// Revision-bound pre/post authority for the exact child call.
///
/// The two envelope digests are hashes of the child-local projection above.  They are named
/// narrowly on purpose: a whole-frame hash would include sibling writes and cannot prove this
/// child's no-action result.  A producer may additionally bind them into a whole-Sim chronology
/// digest through `request.authority_digest`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1CasterProcessAuthority {
    pub capture_revision: u64,
    pub capture_composition_digest: [u8; 32],
    pub source: Frame1CasterCaptureSource,
    pub executable_sha256: [u8; 32],
    pub replay_file_sha256: [u8; 32],
    pub request: CasterProcessSpellsRequest,
    pub scout_type: Frame1ScoutTypeJoinAuthority,
    pub call_entry_envelope_sha256: [u8; 32],
    pub call_exit_envelope_sha256: [u8; 32],
    pub before: Frame1CasterLocalEnvelope,
    pub after: Frame1CasterLocalEnvelope,
}

/// Caller-owned mount projection.  It is deliberately the complete child-local envelope.
/// Successful empty execution republishes the captured identical after-image; every other
/// outcome leaves this value untouched.
pub type Frame1CasterMountState = Frame1CasterLocalEnvelope;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame1CasterProcessBranch {
    CapturedEmptyArrayNoAction,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1CasterProcessReceipt {
    pub capture_revision: u64,
    pub capture_composition_digest: [u8; 32],
    pub source: Frame1CasterCaptureSource,
    pub executable_sha256: [u8; 32],
    pub replay_file_sha256: [u8; 32],
    pub request: CasterProcessSpellsRequest,
    pub scout_type: Frame1ScoutTypeJoinAuthority,
    pub call_entry_envelope_sha256: [u8; 32],
    pub call_exit_envelope_sha256: [u8; 32],
    pub before: Frame1CasterLocalEnvelope,
    pub after: Frame1CasterLocalEnvelope,
    pub branch: Frame1CasterProcessBranch,
    pub removed_spells: u32,
    pub jam_radar_pulses: u32,
    pub rng_draws: u32,
    pub simulation_mutations: u32,
}

/// A valid, reached retail child whose body is not yet owned by this adapter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame1CasterChildResidual {
    NonEmptyActiveSpellQueue {
        before_length: i32,
        after_length: i32,
        /// Ordered entry images are retained by the enclosing authority; this digest binds the
        /// residual to that exact captured transaction.
        capture_composition_digest: [u8; 32],
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame1CasterProcessOutcome {
    Complete(Frame1CasterProcessReceipt),
    OpenResidual(Frame1CasterChildResidual),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvelopeSide {
    Before,
    After,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame1CasterProcessError {
    MissingRequestAuthorityRevision,
    MissingRequestAuthorityDigest,
    MissingCaptureRevision,
    MissingCaptureCompositionDigest,
    MissingEntryEnvelopeDigest,
    MissingExitEnvelopeDigest,
    UnsupportedExecutable,
    ReplayMismatch,
    RequestMismatch,
    WrongFrame(i32),
    WrongMode(i32),
    WrongScoutIdentity,
    MissingScoutTypeRevision,
    MissingScoutTypeDigest,
    WrongScoutBaseType(i32),
    InvalidEffectiveScoutType(i32),
    ScoutEffectiveTypeMismatch {
        expected: i32,
        actual: i32,
    },
    WrongScoutTypeFlags(u32),
    InvalidCasterIndex(i16),
    WrongCasterIndex,
    InvalidArrayImage {
        side: EnvelopeSide,
        error: ActiveSpellArrayShapeError,
    },
    StaleMountState,
    CapturedIdentityChanged,
    EmptyCallMutatedState,
    EmptyCallDigestMismatch,
}

fn validate_request(request: &CasterProcessSpellsRequest) -> Result<(), Frame1CasterProcessError> {
    if request.authority_revision == 0 {
        return Err(Frame1CasterProcessError::MissingRequestAuthorityRevision);
    }
    if request.authority_digest == [0; 32] {
        return Err(Frame1CasterProcessError::MissingRequestAuthorityDigest);
    }
    if request.frame != FRAME_ONE {
        return Err(Frame1CasterProcessError::WrongFrame(request.frame));
    }
    if request.mode != PROCESS_SPELLS_NORMAL_MODE {
        return Err(Frame1CasterProcessError::WrongMode(request.mode));
    }
    if request.who != SETUP_SCOUT_WHO || request.o != SETUP_SCOUT_O {
        return Err(Frame1CasterProcessError::WrongScoutIdentity);
    }
    if request.caster_index < 0 {
        return Err(Frame1CasterProcessError::InvalidCasterIndex(
            request.caster_index,
        ));
    }
    Ok(())
}

fn validate_envelope(
    request: &CasterProcessSpellsRequest,
    scout_type: &Frame1ScoutTypeJoinAuthority,
    envelope: &Frame1CasterLocalEnvelope,
    side: EnvelopeSide,
) -> Result<(), Frame1CasterProcessError> {
    if envelope.frame != request.frame {
        return Err(Frame1CasterProcessError::WrongFrame(envelope.frame));
    }
    if envelope.unit != request.unit || envelope.who != request.who || envelope.o != request.o {
        return Err(Frame1CasterProcessError::WrongScoutIdentity);
    }
    if envelope.type_index != scout_type.effective_type_index {
        return Err(Frame1CasterProcessError::ScoutEffectiveTypeMismatch {
            expected: scout_type.effective_type_index,
            actual: envelope.type_index,
        });
    }
    if envelope.type_unit_flags2 != scout_type.effective_type_unit_flags2
        || envelope.type_unit_flags2 & UNIT_FLAGS2_CASTER == 0
        || envelope.type_unit_flags2 & UNIT_FLAGS2_SPECIAL == 0
        || envelope.type_unit_flags2 & UNIT_FLAGS2_HERO != 0
    {
        return Err(Frame1CasterProcessError::WrongScoutTypeFlags(
            envelope.type_unit_flags2,
        ));
    }
    if envelope.caster_index != request.caster_index {
        return Err(Frame1CasterProcessError::WrongCasterIndex);
    }
    envelope
        .active_spells
        .validate()
        .map_err(|error| Frame1CasterProcessError::InvalidArrayImage { side, error })
}

fn same_stable_identity(
    before: &Frame1CasterLocalEnvelope,
    after: &Frame1CasterLocalEnvelope,
) -> bool {
    before.frame == after.frame
        && before.unit == after.unit
        && before.who == after.who
        && before.o == after.o
        && before.uid == after.uid
        && before.type_index == after.type_index
        && before.type_unit_flags2 == after.type_unit_flags2
        && before.caster_index == after.caster_index
}

/// Mount the captured frame-one Scout child atomically.
///
/// The capture is first validated without touching `state`.  A valid nonempty call returns a
/// typed residual, also without touching `state`.  The only complete branch requires the exact
/// empty before/after images and identical adjacent envelope digests, then republishes the
/// captured after-image (which is equal to the before-image).
pub fn mount_captured_frame1_scout_caster_process(
    state: &mut Frame1CasterMountState,
    request: CasterProcessSpellsRequest,
    authority: &Frame1CasterProcessAuthority,
) -> Result<Frame1CasterProcessOutcome, Frame1CasterProcessError> {
    validate_request(&request)?;
    if authority.capture_revision == 0 {
        return Err(Frame1CasterProcessError::MissingCaptureRevision);
    }
    if authority.capture_composition_digest == [0; 32] {
        return Err(Frame1CasterProcessError::MissingCaptureCompositionDigest);
    }
    if authority.call_entry_envelope_sha256 == [0; 32] {
        return Err(Frame1CasterProcessError::MissingEntryEnvelopeDigest);
    }
    if authority.call_exit_envelope_sha256 == [0; 32] {
        return Err(Frame1CasterProcessError::MissingExitEnvelopeDigest);
    }
    if authority.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256 {
        return Err(Frame1CasterProcessError::UnsupportedExecutable);
    }
    if authority.replay_file_sha256 != SUPPORTED_GOLDEN_REPLAY_SHA256 {
        return Err(Frame1CasterProcessError::ReplayMismatch);
    }
    if authority.request != request {
        return Err(Frame1CasterProcessError::RequestMismatch);
    }
    if authority.scout_type.revision == 0 {
        return Err(Frame1CasterProcessError::MissingScoutTypeRevision);
    }
    if authority.scout_type.composition_digest == [0; 32] {
        return Err(Frame1CasterProcessError::MissingScoutTypeDigest);
    }
    if authority.scout_type.base_setup_type != SETUP_SCOUT_BASE_TYPE {
        return Err(Frame1CasterProcessError::WrongScoutBaseType(
            authority.scout_type.base_setup_type,
        ));
    }
    if authority.scout_type.effective_type_index < 0 {
        return Err(Frame1CasterProcessError::InvalidEffectiveScoutType(
            authority.scout_type.effective_type_index,
        ));
    }
    if authority.scout_type.effective_type_unit_flags2 & UNIT_FLAGS2_CASTER == 0
        || authority.scout_type.effective_type_unit_flags2 & UNIT_FLAGS2_SPECIAL == 0
        || authority.scout_type.effective_type_unit_flags2 & UNIT_FLAGS2_HERO != 0
    {
        return Err(Frame1CasterProcessError::WrongScoutTypeFlags(
            authority.scout_type.effective_type_unit_flags2,
        ));
    }
    validate_envelope(
        &request,
        &authority.scout_type,
        &authority.before,
        EnvelopeSide::Before,
    )?;
    validate_envelope(
        &request,
        &authority.scout_type,
        &authority.after,
        EnvelopeSide::After,
    )?;
    if *state != authority.before {
        return Err(Frame1CasterProcessError::StaleMountState);
    }
    if !same_stable_identity(&authority.before, &authority.after) {
        return Err(Frame1CasterProcessError::CapturedIdentityChanged);
    }

    if !authority.before.active_spells.is_empty() {
        return Ok(Frame1CasterProcessOutcome::OpenResidual(
            Frame1CasterChildResidual::NonEmptyActiveSpellQueue {
                before_length: authority.before.active_spells.length,
                after_length: authority.after.active_spells.length,
                capture_composition_digest: authority.capture_composition_digest,
            },
        ));
    }

    if !authority.after.active_spells.is_empty() || authority.before != authority.after {
        return Err(Frame1CasterProcessError::EmptyCallMutatedState);
    }
    if authority.call_entry_envelope_sha256 != authority.call_exit_envelope_sha256 {
        return Err(Frame1CasterProcessError::EmptyCallDigestMismatch);
    }

    // Publication occurs only after every check above.  It is a no-op today by construction,
    // but retaining the assignment makes the transaction boundary explicit to its caller.
    *state = authority.after.clone();
    Ok(Frame1CasterProcessOutcome::Complete(
        Frame1CasterProcessReceipt {
            capture_revision: authority.capture_revision,
            capture_composition_digest: authority.capture_composition_digest,
            source: authority.source,
            executable_sha256: authority.executable_sha256,
            replay_file_sha256: authority.replay_file_sha256,
            request,
            scout_type: authority.scout_type,
            call_entry_envelope_sha256: authority.call_entry_envelope_sha256,
            call_exit_envelope_sha256: authority.call_exit_envelope_sha256,
            before: authority.before.clone(),
            after: authority.after.clone(),
            branch: Frame1CasterProcessBranch::CapturedEmptyArrayNoAction,
            removed_spells: 0,
            jam_radar_pulses: 0,
            rng_draws: 0,
            simulation_mutations: 0,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> CasterProcessSpellsRequest {
        CasterProcessSpellsRequest {
            authority_revision: 7,
            authority_digest: [0x41; 32],
            frame: FRAME_ONE,
            unit: Handle {
                id: 1,
                generation: 2,
            },
            who: SETUP_SCOUT_WHO,
            o: SETUP_SCOUT_O,
            caster_index: 3,
            mode: PROCESS_SPELLS_NORMAL_MODE,
        }
    }

    fn empty_array() -> ActiveSpellArrayImage {
        ActiveSpellArrayImage {
            length: 0,
            // Allocation history is captured, not normalized away.
            size: 4,
            increment: 4,
            flags: 0x12,
            cur_index: -1,
            entries: Vec::new(),
        }
    }

    fn envelope() -> Frame1CasterLocalEnvelope {
        let request = request();
        Frame1CasterLocalEnvelope {
            frame: request.frame,
            unit: request.unit,
            who: request.who,
            o: request.o,
            uid: 11,
            type_index: SETUP_SCOUT_BASE_TYPE,
            type_unit_flags2: SETUP_SCOUT_BASE_UNIT_FLAGS2,
            caster_index: request.caster_index,
            object_header_flags: 1,
            unit_masks: 0x0004_0000,
            unit_masks2: 0,
            spell_visibility_dirty: 0,
            random_state: 0x1234_5678,
            active_spells: empty_array(),
        }
    }

    fn authority() -> Frame1CasterProcessAuthority {
        let image = envelope();
        Frame1CasterProcessAuthority {
            capture_revision: 9,
            capture_composition_digest: [0x82; 32],
            source: Frame1CasterCaptureSource::SupportedRetailAdjacentUnitProcessCall,
            executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
            replay_file_sha256: SUPPORTED_GOLDEN_REPLAY_SHA256,
            request: request(),
            scout_type: Frame1ScoutTypeJoinAuthority {
                revision: 8,
                composition_digest: [0x52; 32],
                base_setup_type: SETUP_SCOUT_BASE_TYPE,
                effective_type_index: SETUP_SCOUT_BASE_TYPE,
                effective_type_unit_flags2: SETUP_SCOUT_BASE_UNIT_FLAGS2,
            },
            call_entry_envelope_sha256: [0x33; 32],
            call_exit_envelope_sha256: [0x33; 32],
            before: image.clone(),
            after: image,
        }
    }

    #[test]
    fn exact_empty_adjacent_capture_admits_no_action() {
        let authority = authority();
        let mut state = authority.before.clone();
        let outcome =
            mount_captured_frame1_scout_caster_process(&mut state, authority.request, &authority)
                .unwrap();
        let Frame1CasterProcessOutcome::Complete(receipt) = outcome else {
            panic!("empty capture must complete")
        };
        assert_eq!(state, authority.before);
        assert_eq!(receipt.before.active_spells.size, 4);
        assert_eq!(receipt.before.active_spells.flags, 0x12);
        assert_eq!(
            receipt.branch,
            Frame1CasterProcessBranch::CapturedEmptyArrayNoAction
        );
        assert_eq!(
            (
                receipt.removed_spells,
                receipt.jam_radar_pulses,
                receipt.rng_draws,
                receipt.simulation_mutations,
            ),
            (0, 0, 0, 0)
        );
    }

    #[test]
    fn nonempty_capture_is_a_typed_residual_and_is_atomic() {
        let mut authority = authority();
        let spell = ActiveSpell {
            type_id: 635,
            start_frame: 0,
            end_frame: 0,
        };
        authority.before.active_spells.length = 1;
        authority.before.active_spells.entries.push(spell);
        authority.after.active_spells.length = 0;
        authority.after.active_spells.entries.clear();
        authority.after.unit_masks &= !0x2800;
        authority.after.spell_visibility_dirty = 1;
        let mut state = authority.before.clone();
        let saved = state.clone();
        assert_eq!(
            mount_captured_frame1_scout_caster_process(&mut state, authority.request, &authority,)
                .unwrap(),
            Frame1CasterProcessOutcome::OpenResidual(
                Frame1CasterChildResidual::NonEmptyActiveSpellQueue {
                    before_length: 1,
                    after_length: 0,
                    capture_composition_digest: [0x82; 32],
                }
            )
        );
        assert_eq!(state, saved);
    }

    #[test]
    fn empty_capture_cannot_hide_any_child_local_mutation() {
        for damage in 0..4 {
            let mut authority = authority();
            match damage {
                0 => authority.after.active_spells.size += 1,
                1 => authority.after.unit_masks ^= 0x8000,
                2 => authority.after.spell_visibility_dirty = 1,
                3 => authority.after.random_state ^= 1,
                _ => unreachable!(),
            }
            let mut state = authority.before.clone();
            let saved = state.clone();
            assert_eq!(
                mount_captured_frame1_scout_caster_process(
                    &mut state,
                    authority.request,
                    &authority,
                ),
                Err(Frame1CasterProcessError::EmptyCallMutatedState)
            );
            assert_eq!(state, saved);
        }
    }

    #[test]
    fn empty_capture_requires_equal_adjacent_envelope_digests() {
        let mut authority = authority();
        authority.call_exit_envelope_sha256[0] ^= 1;
        let mut state = authority.before.clone();
        let saved = state.clone();
        assert_eq!(
            mount_captured_frame1_scout_caster_process(&mut state, authority.request, &authority,),
            Err(Frame1CasterProcessError::EmptyCallDigestMismatch)
        );
        assert_eq!(state, saved);
    }

    #[test]
    fn stale_state_and_bad_revision_bindings_refuse_atomically() {
        let authority = authority();
        let mut stale = authority.before.clone();
        stale.unit_masks ^= 1;
        let saved = stale.clone();
        assert_eq!(
            mount_captured_frame1_scout_caster_process(&mut stale, authority.request, &authority,),
            Err(Frame1CasterProcessError::StaleMountState)
        );
        assert_eq!(stale, saved);

        let mut mismatched = authority.clone();
        mismatched.request.authority_revision += 1;
        let mut state = authority.before.clone();
        let saved = state.clone();
        assert_eq!(
            mount_captured_frame1_scout_caster_process(&mut state, authority.request, &mismatched,),
            Err(Frame1CasterProcessError::RequestMismatch)
        );
        assert_eq!(state, saved);
    }

    #[test]
    fn malformed_engine_array_image_is_not_an_empty_queue() {
        let mut authority = authority();
        authority.before.active_spells.length = 1;
        let mut state = authority.before.clone();
        let saved = state.clone();
        assert_eq!(
            mount_captured_frame1_scout_caster_process(&mut state, authority.request, &authority,),
            Err(Frame1CasterProcessError::InvalidArrayImage {
                side: EnvelopeSide::Before,
                error: ActiveSpellArrayShapeError::EntryCountMismatch {
                    length: 1,
                    entries: 0,
                },
            })
        );
        assert_eq!(state, saved);
    }

    #[test]
    fn scout_dispatch_requires_special_not_hero_type_flags() {
        for bad in [0, UNIT_FLAGS2_CASTER, UNIT_FLAGS2_CASTER | UNIT_FLAGS2_HERO] {
            let mut authority = authority();
            authority.scout_type.effective_type_unit_flags2 = bad;
            authority.before.type_unit_flags2 = bad;
            authority.after.type_unit_flags2 = bad;
            let mut state = authority.before.clone();
            let saved = state.clone();
            assert_eq!(
                mount_captured_frame1_scout_caster_process(
                    &mut state,
                    authority.request,
                    &authority,
                ),
                Err(Frame1CasterProcessError::WrongScoutTypeFlags(bad))
            );
            assert_eq!(state, saved);
        }
    }

    #[test]
    fn source_join_not_base_name_decides_the_effective_scout_type() {
        let mut authority = authority();
        authority.scout_type.effective_type_index = 71;
        authority.before.type_index = 71;
        authority.after.type_index = 71;
        let mut state = authority.before.clone();
        assert!(matches!(
            mount_captured_frame1_scout_caster_process(&mut state, authority.request, &authority,),
            Ok(Frame1CasterProcessOutcome::Complete(_))
        ));

        let mut missing = authority.clone();
        missing.scout_type.composition_digest = [0; 32];
        let mut state = authority.before.clone();
        let saved = state.clone();
        assert_eq!(
            mount_captured_frame1_scout_caster_process(&mut state, authority.request, &missing,),
            Err(Frame1CasterProcessError::MissingScoutTypeDigest)
        );
        assert_eq!(state, saved);
    }

    #[test]
    fn executable_replay_and_call_shape_are_revision_closed() {
        let baseline = authority();
        for damage in 0..4 {
            let mut authority = baseline.clone();
            let expected = match damage {
                0 => {
                    authority.executable_sha256[0] ^= 1;
                    Frame1CasterProcessError::UnsupportedExecutable
                }
                1 => {
                    authority.replay_file_sha256[0] ^= 1;
                    Frame1CasterProcessError::ReplayMismatch
                }
                2 => {
                    authority.request.mode = 1;
                    Frame1CasterProcessError::RequestMismatch
                }
                3 => {
                    authority.request.caster_index += 1;
                    Frame1CasterProcessError::RequestMismatch
                }
                _ => unreachable!(),
            };
            let mut state = baseline.before.clone();
            let saved = state.clone();
            assert_eq!(
                mount_captured_frame1_scout_caster_process(
                    &mut state,
                    baseline.request,
                    &authority,
                ),
                Err(expected)
            );
            assert_eq!(state, saved);
        }

        let mut bad_request = baseline.request;
        bad_request.caster_index = -1;
        let mut state = baseline.before.clone();
        let saved = state.clone();
        assert_eq!(
            mount_captured_frame1_scout_caster_process(&mut state, bad_request, &baseline,),
            Err(Frame1CasterProcessError::InvalidCasterIndex(-1))
        );
        assert_eq!(state, saved);
    }
}
