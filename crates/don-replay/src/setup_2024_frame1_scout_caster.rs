//! Source-bound frame-one Scout `Caster::process_spells` child.
//!
//! The post-command frame-one image is an oracle authority, not evidence that frame zero was
//! executed source-exactly by the port.  This adapter uses that image only for the live Scout
//! identity, dispatch row, and call arguments.  The active-spell length has a separate source
//! derivation: the complete setup receiver reaches `Specials::init_special`, `Special::Special`
//! calls `Caster::Caster`, and the Caster constructor writes the array length to zero.  The
//! reached frame-zero `Unit::think_spellcaster` body has no access to that owner; its successful
//! write is a distinct Unit CastOrder.  An empty frame-one Caster body then jumps directly to its
//! return epilogue.
//!
//! The result is deliberately one child-local receipt.  It does not claim completion of frame
//! zero, the frame-one Unit body, or the frame-one tick.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::systems::frame0_scout_spellcaster::{
    validate_frame0_scout_caster_invariant, Frame0ScoutCasterInvariantAuthority,
    Frame0ScoutCasterInvariantBranch, NoCastReason,
};
use don_sim::systems::frame1_caster_process::{
    prove_source_empty_caster_process, CasterProcessSpellsRequest,
    Frame1CasterSourceEmptyNoopReceipt, Frame1ScoutTypeJoinAuthority, PROCESS_SPELLS_NORMAL_MODE,
    SETUP_SCOUT_BASE_TYPE, UNIT_FLAGS2_CASTER, UNIT_FLAGS2_HERO, UNIT_FLAGS2_SPECIAL,
};
use don_sim::systems::objects_init_unit_authority_frontier::{
    OBJECTS_INIT_UNIT_BYTES, OBJECTS_INIT_UNIT_VA,
};
use don_sim::systems::unit_inctime::SUPPORTED_RETAIL_EXE_SHA256;
use don_sim::tick::Sim;
use don_sim::world::{Handle, OBJ_FLAG_ACTIVE};

use crate::groups_pre_pair_unit_authority::{
    replay_unit_type_facts, PrePairUnitAuthorityError, ReplayUnitTypeSpans,
};
use crate::replay::{load_payload, Replay};
use crate::setup_2024_frame379::{
    Frame379SetupEntryReceipt, Frame379SetupReceipt, REPLAY_FILE_SHA256,
};
use crate::setup_2024_golden_capture::{
    validate_frame1_post_command_authority, Frame1GoldenBindError, Frame1PostCommandAuthority,
};
use crate::setup_units_producer::{StableUnitIdentityReceipt, StartingUnitPhase};
use crate::world_owner_frontier::sha256;

pub const UNIT_INIT_VA: u32 = 0x0061_2100;
pub const UNIT_INIT_BYTES: u32 = 3_732;
pub const UNIT_INIT_SHA256: [u8; 32] = [
    0x1a, 0xf7, 0xb1, 0x2a, 0x26, 0xe8, 0xdc, 0xf2, 0x9f, 0x74, 0x6b, 0x0c, 0xcd, 0x21, 0x79, 0x3e,
    0xe9, 0x68, 0x04, 0x28, 0x4f, 0x21, 0xf7, 0x91, 0xbc, 0x8e, 0x57, 0xa6, 0x67, 0x2c, 0x48, 0x67,
];
pub const UNIT_INIT_SPECIAL_CALL_VA: u32 = 0x0061_2e27;
pub const SPECIALS_INIT_SPECIAL_VA: u32 = 0x0074_01c0;
pub const SPECIALS_INIT_SPECIAL_BYTES: u32 = 333;
pub const SPECIALS_INIT_SPECIAL_SHA256: [u8; 32] = [
    0xe1, 0xfc, 0x83, 0xed, 0xa4, 0x7d, 0xfa, 0x23, 0xd7, 0x87, 0x28, 0xc3, 0xb7, 0x18, 0x64, 0x44,
    0x2a, 0xf9, 0x27, 0xef, 0xe0, 0xcd, 0xb8, 0xfc, 0x28, 0xff, 0x79, 0xa0, 0xc8, 0xd5, 0xda, 0xa6,
];
pub const SPECIAL_CONSTRUCTOR_VA: u32 = 0x0073_ffb0;
pub const SPECIAL_CONSTRUCTOR_BYTES: u32 = 126;
pub const SPECIAL_CONSTRUCTOR_SHA256: [u8; 32] = [
    0x53, 0xd5, 0x69, 0x23, 0x6f, 0x85, 0xb5, 0xea, 0xb9, 0x38, 0xe7, 0xeb, 0x41, 0x02, 0x8c, 0x2d,
    0x3d, 0x9c, 0x8e, 0xd9, 0x56, 0x9a, 0x34, 0x35, 0x68, 0x5e, 0xf8, 0xee, 0xcb, 0xcf, 0xac, 0xbc,
];
pub const SPECIAL_CONSTRUCTOR_CASTER_CALL_VA: u32 = 0x0073_ffdc;
pub const CASTER_CONSTRUCTOR_VA: u32 = 0x0073_9a20;
pub const CASTER_CONSTRUCTOR_BYTES: u32 = 141;
pub const CASTER_CONSTRUCTOR_SHA256: [u8; 32] = [
    0x3f, 0x15, 0xd1, 0xee, 0x4b, 0x98, 0x7f, 0xdf, 0xeb, 0x1c, 0x89, 0xff, 0x8b, 0x59, 0x82, 0x07,
    0x59, 0x92, 0x4e, 0xc0, 0xe6, 0x03, 0x1f, 0x47, 0x71, 0x67, 0xb9, 0x81, 0x19, 0x92, 0x49, 0x8e,
];
pub const CASTER_CONSTRUCTOR_LENGTH_ZERO_STORE_VA: u32 = 0x0073_9a49;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame1ScoutCasterChildSource {
    /// Canonical setup Caster construction and its narrow frame-zero no-writer source cone,
    /// joined to the independently captured post-command frame-one image.
    SetupCasterSourceConeAndPostCommandImage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame1ScoutCasterStage {
    PostCommandImageBound,
    StableSetupScoutBound,
    ReplayDispatchTypeBound,
    SetupCasterLengthZeroDerived,
    FrameZeroScoutNoWriterBound,
    FrameOneEmptyChildReturned,
}

pub const FRAME1_SCOUT_CASTER_STAGE_ORDER: [Frame1ScoutCasterStage; 6] = [
    Frame1ScoutCasterStage::PostCommandImageBound,
    Frame1ScoutCasterStage::StableSetupScoutBound,
    Frame1ScoutCasterStage::ReplayDispatchTypeBound,
    Frame1ScoutCasterStage::SetupCasterLengthZeroDerived,
    Frame1ScoutCasterStage::FrameZeroScoutNoWriterBound,
    Frame1ScoutCasterStage::FrameOneEmptyChildReturned,
];

/// Exact setup call chain which establishes the Caster array's length word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame1ScoutCasterSetupEmptyReceipt {
    pub completed_init_revision: u64,
    pub completed_init_digest: [u8; 32],
    pub objects_init_unit_va: u32,
    pub objects_init_unit_bytes: u32,
    pub unit_init_va: u32,
    pub unit_init_bytes: u32,
    pub unit_init_sha256: [u8; 32],
    pub unit_init_special_call_va: u32,
    pub specials_init_special_va: u32,
    pub specials_init_special_bytes: u32,
    pub specials_init_special_sha256: [u8; 32],
    pub special_constructor_va: u32,
    pub special_constructor_bytes: u32,
    pub special_constructor_sha256: [u8; 32],
    pub special_constructor_caster_call_va: u32,
    pub caster_constructor_va: u32,
    pub caster_constructor_bytes: u32,
    pub caster_constructor_sha256: [u8; 32],
    pub length_zero_store_va: u32,
    pub initialized_active_spells_length: i32,
}

/// Replay-carried type row which takes the exact Special-Caster dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame1ScoutCasterDispatchReceipt {
    pub spans: ReplayUnitTypeSpans,
    pub source_bytes_sha256: [u8; 32],
    pub setup_final_type: i32,
    pub current_type: i32,
    pub unit_flags2: u32,
}

/// One source-bound adjacent child receipt.  The entry/exit hashes cover only the projection
/// this empty child can observe: request, stable receiver/type join, Caster index, length, and
/// main RNG state.  They are not whole-Sim or whole-tick hashes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1ScoutCasterChildReceipt {
    pub source: Frame1ScoutCasterChildSource,
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub post_command_sim_sha256: [u8; 32],
    pub request: CasterProcessSpellsRequest,
    pub setup_member: StableUnitIdentityReceipt,
    pub unit_row: usize,
    pub uid: u16,
    pub object_header_flags: u8,
    pub unit_masks: u32,
    pub unit_masks2: u32,
    pub dispatch: Frame1ScoutCasterDispatchReceipt,
    pub scout_type: Frame1ScoutTypeJoinAuthority,
    pub setup_empty: Frame1ScoutCasterSetupEmptyReceipt,
    pub frame0_invariant: Frame0ScoutCasterInvariantAuthority,
    pub child: Frame1CasterSourceEmptyNoopReceipt,
    pub call_entry_source_projection_sha256: [u8; 32],
    pub call_exit_source_projection_sha256: [u8; 32],
    pub stage_order: [Frame1ScoutCasterStage; 6],
    pub removed_spells: u32,
    pub rng_draws: u32,
    pub simulation_mutations: u32,
    pub composition_digest: [u8; 32],
}

#[derive(Debug)]
pub enum Frame1ScoutCasterChildError {
    Golden(Frame1GoldenBindError),
    PayloadRead(String),
    PayloadMismatch,
    MissingRules,
    TypeFacts(PrePairUnitAuthorityError),
    SetupAuthorityMismatch,
    MissingSetupScout,
    SetupScoutSourceMismatch,
    SetupScoutCallMismatch,
    SetupScoutMemberMismatch,
    MissingLiveScout,
    LiveScoutIdentityMismatch,
    InactiveLiveScout,
    CurrentTypeMismatch { setup: i32, current: i32 },
    WrongDispatchFlags(u32),
    InvalidCasterIndex(i16),
    SourceInvariantMismatch,
    ChildResidual { entry_length: i32 },
    ReceiptMismatch,
}

impl fmt::Display for Frame1ScoutCasterChildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "2024 frame-1 Scout Caster child refused: {self:?}")
    }
}

impl std::error::Error for Frame1ScoutCasterChildError {}

impl From<Frame1GoldenBindError> for Frame1ScoutCasterChildError {
    fn from(value: Frame1GoldenBindError) -> Self {
        Self::Golden(value)
    }
}

impl From<PrePairUnitAuthorityError> for Frame1ScoutCasterChildError {
    fn from(value: PrePairUnitAuthorityError) -> Self {
        Self::TypeFacts(value)
    }
}

fn append_span(image: &mut Vec<u8>, span: crate::initial::ReplayByteSpan) {
    image.extend_from_slice(&(span.offset as u64).to_le_bytes());
    image.extend_from_slice(&(span.bytes as u64).to_le_bytes());
}

fn dispatch_source_digest(payload: &[u8], spans: ReplayUnitTypeSpans) -> [u8; 32] {
    let mut image = b"don-frame1-scout-caster-dispatch-source-v1".to_vec();
    for (tag, span) in [(1_u8, spans.type_base), (2, spans.object), (3, spans.unit)] {
        image.push(tag);
        append_span(&mut image, span);
        image.extend_from_slice(&payload[span.offset..span.end()]);
    }
    sha256(&image)
}

fn append_identity(image: &mut Vec<u8>, identity: StableUnitIdentityReceipt) {
    image.extend_from_slice(&identity.id.to_le_bytes());
    image.extend_from_slice(&identity.generation.to_le_bytes());
    image.extend_from_slice(&identity.owner.to_le_bytes());
    image.extend_from_slice(&identity.o.to_le_bytes());
}

fn append_request(image: &mut Vec<u8>, request: CasterProcessSpellsRequest) {
    image.extend_from_slice(&request.authority_revision.to_le_bytes());
    image.extend_from_slice(&request.authority_digest);
    image.extend_from_slice(&request.frame.to_le_bytes());
    image.extend_from_slice(&request.unit.id.to_le_bytes());
    image.extend_from_slice(&request.unit.generation.to_le_bytes());
    image.push(request.who);
    image.extend_from_slice(&request.o.to_le_bytes());
    image.extend_from_slice(&request.caster_index.to_le_bytes());
    image.extend_from_slice(&request.mode.to_le_bytes());
}

fn source_projection_digest(
    request: CasterProcessSpellsRequest,
    identity: StableUnitIdentityReceipt,
    uid: u16,
    dispatch: Frame1ScoutCasterDispatchReceipt,
    active_spells_length: i32,
    random_state: i32,
) -> [u8; 32] {
    let mut image = b"don-frame1-scout-caster-child-local-source-projection-v1".to_vec();
    append_request(&mut image, request);
    append_identity(&mut image, identity);
    image.extend_from_slice(&uid.to_le_bytes());
    image.extend_from_slice(&dispatch.setup_final_type.to_le_bytes());
    image.extend_from_slice(&dispatch.current_type.to_le_bytes());
    image.extend_from_slice(&dispatch.unit_flags2.to_le_bytes());
    image.extend_from_slice(&dispatch.source_bytes_sha256);
    image.extend_from_slice(&active_spells_length.to_le_bytes());
    image.extend_from_slice(&random_state.to_le_bytes());
    sha256(&image)
}

/// Stable digest over every public claim in the child receipt other than this digest itself.
pub fn frame1_scout_caster_child_composition_digest(
    receipt: &Frame1ScoutCasterChildReceipt,
) -> [u8; 32] {
    let mut image = b"don-frame1-scout-caster-child-v1".to_vec();
    image.push(match receipt.source {
        Frame1ScoutCasterChildSource::SetupCasterSourceConeAndPostCommandImage => 1,
    });
    image.extend_from_slice(&receipt.authority_revision.to_le_bytes());
    image.extend_from_slice(&receipt.authority_digest);
    image.extend_from_slice(&receipt.post_command_sim_sha256);
    append_request(&mut image, receipt.request);
    append_identity(&mut image, receipt.setup_member);
    image.extend_from_slice(&(receipt.unit_row as u64).to_le_bytes());
    image.extend_from_slice(&receipt.uid.to_le_bytes());
    image.push(receipt.object_header_flags);
    image.extend_from_slice(&receipt.unit_masks.to_le_bytes());
    image.extend_from_slice(&receipt.unit_masks2.to_le_bytes());
    append_span(&mut image, receipt.dispatch.spans.type_base);
    append_span(&mut image, receipt.dispatch.spans.object);
    append_span(&mut image, receipt.dispatch.spans.unit);
    image.extend_from_slice(&receipt.dispatch.source_bytes_sha256);
    image.extend_from_slice(&receipt.dispatch.setup_final_type.to_le_bytes());
    image.extend_from_slice(&receipt.dispatch.current_type.to_le_bytes());
    image.extend_from_slice(&receipt.dispatch.unit_flags2.to_le_bytes());
    image.extend_from_slice(&receipt.scout_type.revision.to_le_bytes());
    image.extend_from_slice(&receipt.scout_type.composition_digest);
    image.extend_from_slice(&receipt.scout_type.base_setup_type.to_le_bytes());
    image.extend_from_slice(&receipt.scout_type.effective_type_index.to_le_bytes());
    image.extend_from_slice(&receipt.scout_type.effective_type_unit_flags2.to_le_bytes());
    let setup = receipt.setup_empty;
    image.extend_from_slice(&setup.completed_init_revision.to_le_bytes());
    image.extend_from_slice(&setup.completed_init_digest);
    for value in [
        setup.objects_init_unit_va,
        setup.objects_init_unit_bytes,
        setup.unit_init_va,
        setup.unit_init_bytes,
        setup.unit_init_special_call_va,
        setup.specials_init_special_va,
        setup.specials_init_special_bytes,
        setup.special_constructor_va,
        setup.special_constructor_bytes,
        setup.special_constructor_caster_call_va,
        setup.caster_constructor_va,
        setup.caster_constructor_bytes,
        setup.length_zero_store_va,
    ] {
        image.extend_from_slice(&value.to_le_bytes());
    }
    image.extend_from_slice(&setup.unit_init_sha256);
    image.extend_from_slice(&setup.specials_init_special_sha256);
    image.extend_from_slice(&setup.special_constructor_sha256);
    image.extend_from_slice(&setup.caster_constructor_sha256);
    image.extend_from_slice(&setup.initialized_active_spells_length.to_le_bytes());
    let frame0 = receipt.frame0_invariant;
    image.extend_from_slice(&frame0.setup.completed_init_revision.to_le_bytes());
    image.extend_from_slice(&frame0.setup.completed_init_digest);
    image.extend_from_slice(&frame0.call_entry_revision.to_le_bytes());
    image.extend_from_slice(&frame0.call_entry_composition_digest);
    image.extend_from_slice(&frame0.executable_sha256);
    image.push(frame0.who);
    image.extend_from_slice(&frame0.o.to_le_bytes());
    image.extend_from_slice(&frame0.type_index.to_le_bytes());
    match frame0.branch {
        Frame0ScoutCasterInvariantBranch::ReturnedNoCast(reason) => {
            image.push(1);
            image.push(match reason {
                NoCastReason::NotSpecial => 1,
                NoCastReason::CounterintelNotCastable => 2,
                NoCastReason::InsufficientMana => 3,
                NoCastReason::NoTarget => 4,
            });
        }
        Frame0ScoutCasterInvariantBranch::AddCastOrderOwnerResidual => image.push(2),
    }
    image.extend_from_slice(&(frame0.consumed_child_receipts as u64).to_le_bytes());
    image.extend_from_slice(&frame0.before_revision.to_le_bytes());
    image.extend_from_slice(&frame0.before_length.to_le_bytes());
    image.extend_from_slice(&frame0.after_revision.to_le_bytes());
    image.extend_from_slice(&frame0.after_length.to_le_bytes());
    image.extend_from_slice(&frame0.write_set.executable_sha256);
    image.extend_from_slice(&frame0.write_set.body_va.to_le_bytes());
    image.extend_from_slice(&frame0.write_set.body_bytes.to_le_bytes());
    image.extend_from_slice(&frame0.write_set.body_sha256);
    image.extend_from_slice(&frame0.write_set.caster_active_spell_reads.to_le_bytes());
    image.extend_from_slice(&frame0.write_set.caster_active_spell_writes.to_le_bytes());
    image.extend_from_slice(&frame0.write_set.add_cast_order_va.to_le_bytes());
    image.extend_from_slice(&frame0.write_set.add_cast_order_bytes.to_le_bytes());
    image.extend_from_slice(&frame0.write_set.add_cast_order_sha256);
    image.extend_from_slice(
        &frame0
            .write_set
            .add_cast_order_caster_active_spell_writes
            .to_le_bytes(),
    );
    image.extend_from_slice(&frame0.write_set.successful_write_owner_offset.to_le_bytes());
    image.extend_from_slice(&frame0.write_set.successful_write_order_index.to_le_bytes());
    image.push(u8::from(frame0.caster_owner_transaction_complete));
    image.push(u8::from(
        frame0.unit_order_and_search_scratch_invariance_claimed,
    ));
    let child = receipt.child;
    image.extend_from_slice(&child.executable_sha256);
    image.extend_from_slice(&child.body_va.to_le_bytes());
    image.extend_from_slice(&child.body_bytes.to_le_bytes());
    image.extend_from_slice(&child.body_sha256);
    image.extend_from_slice(&child.length_load_va.to_le_bytes());
    image.extend_from_slice(&child.empty_branch_va.to_le_bytes());
    image.extend_from_slice(&child.return_va.to_le_bytes());
    image.extend_from_slice(&child.entry_length.to_le_bytes());
    image.extend_from_slice(&child.exit_length.to_le_bytes());
    image.extend_from_slice(&child.active_spell_entries_read.to_le_bytes());
    image.extend_from_slice(&child.stores.to_le_bytes());
    image.extend_from_slice(&child.rng_draws.to_le_bytes());
    image.extend_from_slice(&receipt.call_entry_source_projection_sha256);
    image.extend_from_slice(&receipt.call_exit_source_projection_sha256);
    for stage in receipt.stage_order {
        image.push(stage as u8);
    }
    image.extend_from_slice(&receipt.removed_spells.to_le_bytes());
    image.extend_from_slice(&receipt.rng_draws.to_le_bytes());
    image.extend_from_slice(&receipt.simulation_mutations.to_le_bytes());
    sha256(&image)
}

fn setup_empty_receipt(
    setup: &Frame379SetupReceipt,
) -> Result<Frame1ScoutCasterSetupEmptyReceipt, Frame1ScoutCasterChildError> {
    let call = setup
        .calls
        .first()
        .ok_or(Frame1ScoutCasterChildError::MissingSetupScout)?;
    if call.completed_init_revision == 0 || call.completed_init_digest == [0; 32] {
        return Err(Frame1ScoutCasterChildError::SetupScoutSourceMismatch);
    }
    Ok(Frame1ScoutCasterSetupEmptyReceipt {
        completed_init_revision: call.completed_init_revision,
        completed_init_digest: call.completed_init_digest,
        objects_init_unit_va: OBJECTS_INIT_UNIT_VA,
        objects_init_unit_bytes: OBJECTS_INIT_UNIT_BYTES,
        unit_init_va: UNIT_INIT_VA,
        unit_init_bytes: UNIT_INIT_BYTES,
        unit_init_sha256: UNIT_INIT_SHA256,
        unit_init_special_call_va: UNIT_INIT_SPECIAL_CALL_VA,
        specials_init_special_va: SPECIALS_INIT_SPECIAL_VA,
        specials_init_special_bytes: SPECIALS_INIT_SPECIAL_BYTES,
        specials_init_special_sha256: SPECIALS_INIT_SPECIAL_SHA256,
        special_constructor_va: SPECIAL_CONSTRUCTOR_VA,
        special_constructor_bytes: SPECIAL_CONSTRUCTOR_BYTES,
        special_constructor_sha256: SPECIAL_CONSTRUCTOR_SHA256,
        special_constructor_caster_call_va: SPECIAL_CONSTRUCTOR_CASTER_CALL_VA,
        caster_constructor_va: CASTER_CONSTRUCTOR_VA,
        caster_constructor_bytes: CASTER_CONSTRUCTOR_BYTES,
        caster_constructor_sha256: CASTER_CONSTRUCTOR_SHA256,
        length_zero_store_va: CASTER_CONSTRUCTOR_LENGTH_ZERO_STORE_VA,
        initialized_active_spells_length: 0,
    })
}

/// Bind the exact first frame-one Unit child to the post-command image and setup source cone.
pub fn bind_frame1_scout_caster_child(
    replay: &Replay,
    setup_entry: &Frame379SetupEntryReceipt,
    setup: &Frame379SetupReceipt,
    frame0_caster: &Frame0ScoutCasterInvariantAuthority,
    authority: &Frame1PostCommandAuthority,
    candidate: &Sim,
) -> Result<Frame1ScoutCasterChildReceipt, Frame1ScoutCasterChildError> {
    validate_frame1_post_command_authority(setup_entry, authority, candidate)?;
    if setup.replay_file_sha256 != REPLAY_FILE_SHA256
        || authority.replay_file_sha256 != REPLAY_FILE_SHA256
        || authority.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256
        || authority.setup_composition_digest != setup.canonical_composition_digest
        || setup.authority_revision == 0
        || setup.canonical_composition_digest == [0; 32]
        || setup.canonical_frame != 0
    {
        return Err(Frame1ScoutCasterChildError::SetupAuthorityMismatch);
    }

    let setup_call = setup
        .plan
        .calls
        .first()
        .ok_or(Frame1ScoutCasterChildError::MissingSetupScout)?;
    let setup_receiver = setup
        .calls
        .first()
        .ok_or(Frame1ScoutCasterChildError::MissingSetupScout)?;
    if setup_call.phase != StartingUnitPhase::BaseScout
        || setup_call.ordinal != 0
        || setup_call.owner != 0
        || setup_call.base_type != SETUP_SCOUT_BASE_TYPE
        || setup_receiver.setup_ordinal != 0
        || setup_receiver.init.validated_body_va != OBJECTS_INIT_UNIT_VA
        || setup_receiver.init.validated_body_bytes != OBJECTS_INIT_UNIT_BYTES
        || setup_receiver.init.members.len() != 1
    {
        return Err(Frame1ScoutCasterChildError::SetupScoutCallMismatch);
    }
    // The final receipt can only originate from the complete retail receiver source. Its
    // revision/digest were admitted by `produce_frame379_setup`; retain that edge explicitly.
    if setup_receiver.completed_init_revision == 0
        || setup_receiver.completed_init_digest == [0; 32]
        || setup_receiver.canonical_after.type_index != setup_call.place_unit_upgrade
    {
        return Err(Frame1ScoutCasterChildError::SetupScoutSourceMismatch);
    }
    let setup_member = authority.setup_members[0];
    if setup_receiver.init.members[0].identity != setup_member
        || setup_member.owner != 0
        || setup_member.o != 0
    {
        return Err(Frame1ScoutCasterChildError::SetupScoutMemberMismatch);
    }

    let handle = Handle {
        id: setup_member.id,
        generation: setup_member.generation,
    };
    let row = candidate
        .world
        .row_of(handle)
        .ok_or(Frame1ScoutCasterChildError::MissingLiveScout)?;
    if candidate.world.unit_row_at(0, 0) != Some(row)
        || candidate.world.units.get_who(row) != 0
        || candidate.world.units.o().get(row).copied() != Some(0)
    {
        return Err(Frame1ScoutCasterChildError::LiveScoutIdentityMismatch);
    }
    let object_header_flags = candidate.world.units.get_flags(row);
    if object_header_flags & OBJ_FLAG_ACTIVE == 0 {
        return Err(Frame1ScoutCasterChildError::InactiveLiveScout);
    }
    let current_type = candidate
        .unit_type
        .get(row)
        .copied()
        .filter(|&type_index| candidate.world.unit_type_id(row) == Some(type_index))
        .ok_or(Frame1ScoutCasterChildError::LiveScoutIdentityMismatch)?;
    if current_type != setup_call.place_unit_upgrade {
        return Err(Frame1ScoutCasterChildError::CurrentTypeMismatch {
            setup: setup_call.place_unit_upgrade,
            current: current_type,
        });
    }

    let payload = load_payload(&replay.path)
        .map_err(|error| Frame1ScoutCasterChildError::PayloadRead(error.to_string()))?;
    if sha256(&payload) != replay.initial.payload_sha256 {
        return Err(Frame1ScoutCasterChildError::PayloadMismatch);
    }
    let rules = replay
        .initial
        .rules
        .ok_or(Frame1ScoutCasterChildError::MissingRules)?;
    let type_facts = replay_unit_type_facts(&payload, &rules, current_type)?;
    if type_facts.unit_flags2 & UNIT_FLAGS2_CASTER == 0
        || type_facts.unit_flags2 & UNIT_FLAGS2_SPECIAL == 0
        || type_facts.unit_flags2 & UNIT_FLAGS2_HERO != 0
    {
        return Err(Frame1ScoutCasterChildError::WrongDispatchFlags(
            type_facts.unit_flags2,
        ));
    }
    let dispatch = Frame1ScoutCasterDispatchReceipt {
        spans: type_facts.spans,
        source_bytes_sha256: dispatch_source_digest(&payload, type_facts.spans),
        setup_final_type: setup_call.place_unit_upgrade,
        current_type,
        unit_flags2: type_facts.unit_flags2,
    };
    let mut type_digest_image = b"don-frame1-scout-caster-type-join-v1".to_vec();
    type_digest_image.extend_from_slice(&authority.setup_composition_digest);
    append_identity(&mut type_digest_image, setup_member);
    type_digest_image.extend_from_slice(&dispatch.source_bytes_sha256);
    type_digest_image.extend_from_slice(&dispatch.setup_final_type.to_le_bytes());
    type_digest_image.extend_from_slice(&dispatch.current_type.to_le_bytes());
    type_digest_image.extend_from_slice(&dispatch.unit_flags2.to_le_bytes());
    let scout_type = Frame1ScoutTypeJoinAuthority {
        revision: authority.revision,
        composition_digest: sha256(&type_digest_image),
        base_setup_type: setup_call.base_type,
        effective_type_index: current_type,
        effective_type_unit_flags2: type_facts.unit_flags2,
    };

    let caster_index = candidate
        .world
        .units
        .special()
        .get(row)
        .copied()
        .ok_or(Frame1ScoutCasterChildError::LiveScoutIdentityMismatch)?;
    if caster_index < 0 {
        return Err(Frame1ScoutCasterChildError::InvalidCasterIndex(
            caster_index,
        ));
    }
    let request = CasterProcessSpellsRequest {
        authority_revision: authority.revision,
        authority_digest: authority.composition_digest,
        frame: authority.command_frame,
        unit: handle,
        who: 0,
        o: 0,
        caster_index,
        mode: PROCESS_SPELLS_NORMAL_MODE,
    };

    let setup_empty = setup_empty_receipt(setup)?;
    if !validate_frame0_scout_caster_invariant(frame0_caster)
        || frame0_caster.setup.completed_init_revision != setup_empty.completed_init_revision
        || frame0_caster.setup.completed_init_digest != setup_empty.completed_init_digest
        || frame0_caster.executable_sha256 != authority.executable_sha256
        || frame0_caster.type_index != setup_call.place_unit_upgrade
        || frame0_caster.before_length as i32 != setup_empty.initialized_active_spells_length
    {
        return Err(Frame1ScoutCasterChildError::SourceInvariantMismatch);
    }
    let child = prove_source_empty_caster_process(frame0_caster.after_length as i32).map_err(
        |residual| Frame1ScoutCasterChildError::ChildResidual {
            entry_length: residual.entry_length,
        },
    )?;
    let projection_digest = source_projection_digest(
        request,
        setup_member,
        candidate.world.units.get_uid(row),
        dispatch,
        child.entry_length,
        candidate.world.random.state(),
    );
    let mut receipt = Frame1ScoutCasterChildReceipt {
        source: Frame1ScoutCasterChildSource::SetupCasterSourceConeAndPostCommandImage,
        authority_revision: authority.revision,
        authority_digest: authority.composition_digest,
        post_command_sim_sha256: authority.post_command_sim_sha256,
        request,
        setup_member,
        unit_row: row,
        uid: candidate.world.units.get_uid(row),
        object_header_flags,
        unit_masks: candidate.world.units.get_unit_masks(row),
        unit_masks2: candidate.world.units.get_unit_masks2(row),
        dispatch,
        scout_type,
        setup_empty,
        frame0_invariant: *frame0_caster,
        child,
        call_entry_source_projection_sha256: projection_digest,
        call_exit_source_projection_sha256: projection_digest,
        stage_order: FRAME1_SCOUT_CASTER_STAGE_ORDER,
        removed_spells: 0,
        rng_draws: 0,
        simulation_mutations: 0,
        composition_digest: [0; 32],
    };
    receipt.composition_digest = frame1_scout_caster_child_composition_digest(&receipt);
    Ok(receipt)
}

/// Recompute the source join and reject a stale or edited child receipt.
pub fn validate_frame1_scout_caster_child(
    replay: &Replay,
    setup_entry: &Frame379SetupEntryReceipt,
    setup: &Frame379SetupReceipt,
    frame0_caster: &Frame0ScoutCasterInvariantAuthority,
    authority: &Frame1PostCommandAuthority,
    candidate: &Sim,
    receipt: &Frame1ScoutCasterChildReceipt,
) -> Result<(), Frame1ScoutCasterChildError> {
    if receipt.composition_digest == [0; 32]
        || receipt.composition_digest != frame1_scout_caster_child_composition_digest(receipt)
    {
        return Err(Frame1ScoutCasterChildError::ReceiptMismatch);
    }
    let expected = bind_frame1_scout_caster_child(
        replay,
        setup_entry,
        setup,
        frame0_caster,
        authority,
        candidate,
    )?;
    if expected != *receipt {
        return Err(Frame1ScoutCasterChildError::ReceiptMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_source_chain_reaches_the_zero_length_store_and_empty_return() {
        assert_eq!(UNIT_INIT_SPECIAL_CALL_VA, 0x0061_2e27);
        assert_eq!(SPECIAL_CONSTRUCTOR_CASTER_CALL_VA, 0x0073_ffdc);
        assert_eq!(CASTER_CONSTRUCTOR_LENGTH_ZERO_STORE_VA, 0x0073_9a49);
        let child = prove_source_empty_caster_process(0).unwrap();
        assert_eq!(child.stores, 0);
        assert_eq!(child.entry_length, child.exit_length);
    }
}
