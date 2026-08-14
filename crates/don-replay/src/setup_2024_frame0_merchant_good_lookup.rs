//! Exact read-only `ObjectsData::find_good_at(WCoord,WCoord,who,0,0)` continuation for
//! the golden frame-zero Merchants.
//!
//! The emitted call has its fifth argument clear. Retail therefore does not scan
//! `0..ObjectsData::good_mark` and does not compare Good coordinates. It reads the requested
//! WData cell, follows its nonnegative `(down_who, down)` Object addresses in order, and
//! accepts only the terminal `down == -2` base-Good marker. The terminal Good must be active,
//! must have a live type pointer, and must not be TypeIndex 5 (Oil). With the fourth argument
//! clear and the Merchant owner nonnegative, the first remaining child is
//! `LeaderData::type_avail(good_type, 1)` (`0x006e33a0`).
//!
//! Live Unit, Build, and Wall links are resolved from the canonical `Sim` sparse object
//! registry and their owning rows. The terminal base Good is read from `OilGoodRuntime`, whose
//! name is historical: it retains every base-Good slot, not only Oil. No state or RNG is
//! mutated. A typed, digest-bound type-availability request is emitted rather than guessing
//! the Leader/type graph.
//!
//! Completed lookup receipts are also consumed in retail order. A `-1` answer resumes the
//! cached/full `calc_gather` circle after the exact probe that failed; a found Good enters the
//! 49-position `find_merchant_spot` loop. The four `good_merchant_spot` terrain reads and the
//! land-Merchant `invalid_loc(...,0,0,0,0,0)` return are source-owned. Golden Type 62 is
//! non-siege, non-hero, and non-supply, so `detect_unit_collision(...,1,1,0,0,0)` bypasses its
//! early-zero flag arm and reaches the still-unowned spatial detector. That exact call is the
//! new typed frontier. No empty-map collision answer or whole-search checksum is synthesized.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::fmt;

use don_sim::systems::bhs_type_table::TypeBuiltinState;
use don_sim::systems::sparse_object_bands_authority_frontier::{RetailBand, RetailObjectAddress};
use don_sim::systems::world_oil_goods::{OilGoodRuntime, OIL_GOOD_TYPE};
use don_sim::tick::Sim;
use don_sim::world::{Handle, WorldObjectIdentity};

use crate::groups_pre_pair_unit_authority::{
    replay_good_type_facts, PrePairUnitAuthorityError, ReplayGoodTypeFacts,
};
use crate::initial::ReplayByteSpan;
use crate::replay::{load_payload, Replay};
use crate::setup_2024_frame0_merchant_search::{
    advance_frame0_merchant_search, frame0_merchant_candidate_tiles,
    good_merchant_spot_terrain_prefix, next_calc_gather_good_lookup,
    validate_frame0_merchant_good_lookup_request, Frame0MerchantCalcGatherSite,
    Frame0MerchantGoodLookupRequest, Frame0MerchantSearchError, Frame0MerchantSearchFrontier,
    OBJECTS_FIND_GOOD_AT_WCOORD_VA, UNIT_DETECT_COLLISION_VA, UNIT_GOOD_MERCHANT_SPOT_VA,
    UNIT_INVALID_LOC_VA,
};
use crate::setup_2024_frame0_merchant_unpack::{
    Frame0MerchantSpotRequest, MerchantSpotOutcome, FAILED_UNPACK_TAIL_VA, MERCHANT_RADIUS,
};
use crate::setup_2024_frame379::REPLAY_FILE_SHA256;
use crate::setup_unit_member_authority::CanonicalSetupUnitMemberReceipt;
use crate::world_owner_frontier::sha256;

pub const LEADER_TYPE_AVAIL_VA: u32 = 0x006e_33a0;
pub const GOOD_TERMINAL: i16 = -2;
pub const NOT_FOUND: i32 = -1;

/// One ordered `find_good_at` answer already consumed by `calc_gather`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0MerchantSearchGoodStep {
    pub request: Frame0MerchantGoodLookupRequest,
    pub receipt: Frame0MerchantGoodLookupReceipt,
}

/// Exact source-owned reads and return from `invalid_loc(tx,ty,0,0,0,0,0)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame0MerchantInvalidLocReceipt {
    pub receipt_sha256: [u8; 32],
    pub call_va: u32,
    pub candidate_index: usize,
    pub tile_x: i32,
    pub tile_y: i32,
    pub orders_empty: bool,
    pub domain: i32,
    pub world_x: i32,
    pub world_y: i32,
    pub world_flags: u16,
    pub tile_mask: u16,
    pub cliff_predicate_read: Option<bool>,
    pub unit_masks: u32,
    pub return_value: i32,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub rng_draws: u32,
}

/// First exact residual after a source-complete `good_merchant_spot` and `invalid_loc == 0`.
///
/// Golden Type 62 is land, non-siege, non-hero, and non-supply. Retail consequently jumps
/// around the flag-based early-zero arm and enters the spatial collision detector. That
/// detector is deliberately left as a typed child; no empty-map result is guessed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0MerchantCollisionRequest {
    pub request_sha256: [u8; 32],
    pub parent_request_sha256: [u8; 32],
    pub search_trace_sha256: [u8; 32],
    pub setup_composition_digest: [u8; 32],
    pub setup_authority_revision: u64,
    pub invocation_ordinal: usize,
    pub actor_who: u8,
    pub actor_o: i16,
    pub actor_uid: u16,
    pub actor_type: i32,
    pub candidate_index: usize,
    pub tile_x: i32,
    pub tile_y: i32,
    pub good_merchant_spot_call_va: u32,
    pub gather_receipt_sha256: [u8; 32],
    pub invalid_loc: Frame0MerchantInvalidLocReceipt,
    pub call_va: u32,
    pub coord_x: i32,
    pub coord_y: i32,
    pub footprint_x: i32,
    pub footprint_y: i32,
    pub arg5: i32,
    pub arg6: i32,
    pub arg7: i32,
    pub type_domain: i32,
    pub type_unit_flags: u32,
    pub type_unit_flags2: u32,
    pub type_is_siege: bool,
    pub unit_is_hero: bool,
    pub unit_is_supply: bool,
    pub random_state: i32,
}

/// Source-complete `find_merchant_spot == 0` continuation into the unported Good-object tail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0MerchantCompletedSearchTailRequest {
    pub parent: Frame0MerchantSpotRequest,
    pub search_proof_sha256: [u8; 32],
    pub completed_good_steps: usize,
    pub head_calc_gather_returned_zero: bool,
    pub find_merchant_spot_outcome: MerchantSpotOutcome,
    pub next_va: u32,
    pub reason: &'static str,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub rng_draws: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame0MerchantSearchContinuationFrontier {
    NeedsGoodLookup(Frame0MerchantGoodLookupRequest),
    NeedsCollision(Frame0MerchantCollisionRequest),
    FailedUnpackTail(Frame0MerchantCompletedSearchTailRequest),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame0MerchantObjectIdentity {
    Unit {
        handle: Handle,
        row: usize,
        uid: u16,
    },
    Build {
        row: usize,
        uid: u16,
    },
    Wall {
        row: usize,
        uid: u16,
    },
}

/// One exact `ObjectData::{down,down_who}` read in retail chain order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame0MerchantObjectLinkRead {
    pub ordinal: usize,
    pub owner: i16,
    pub object: i16,
    pub identity: Frame0MerchantObjectIdentity,
    pub next_object: i16,
    pub next_owner: i16,
}

/// The terminal Good reads actually reached by the fifth-argument-zero arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame0MerchantGoodSlotRead {
    pub logical_length: usize,
    pub slot: i16,
    pub flags: u8,
    /// `None` when the active-bit filter returned before reading `ptype`.
    pub ptype_present: Option<bool>,
    /// `None` when the active-bit filter returned before dereferencing `ptype + 4`.
    pub type_index: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame0MerchantGoodNotFoundReason {
    TerminalWasNotGood,
    InactiveGood,
    OilGood,
    TypeUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame0MerchantGoodLookupOutcome {
    FoundType(i32),
    NotFound(Frame0MerchantGoodNotFoundReason),
}

/// Complete read-only result of the emitted `find_good_at` child.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0MerchantGoodLookupReceipt {
    pub receipt_sha256: [u8; 32],
    pub lookup_request_sha256: [u8; 32],
    pub call_va: u32,
    pub world_x: i32,
    pub world_y: i32,
    pub world_index: usize,
    pub initial_object: i16,
    pub initial_owner: i16,
    pub object_links: Vec<Frame0MerchantObjectLinkRead>,
    pub terminal_object: i16,
    pub terminal_owner: i16,
    pub good: Option<Frame0MerchantGoodSlotRead>,
    pub type_avail_checked: Option<bool>,
    pub type_avail_capture: Option<Frame0MerchantTypeAvailCapture>,
    pub outcome: Frame0MerchantGoodLookupOutcome,
    pub return_value: i32,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub rng_draws: u32,
}

/// Exact first child after a valid, active, non-Oil terminal Good.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0MerchantTypeAvailRequest {
    pub request_sha256: [u8; 32],
    pub lookup_request_sha256: [u8; 32],
    pub traversal_prefix_sha256: [u8; 32],
    pub call_va: u32,
    pub leader: i32,
    pub type_index: i32,
    pub strict: i32,
    pub good_slot: i16,
    pub random_state: i32,
}

/// Detached authority answer for one exact `LeaderData::type_avail(type, 1)` request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame0MerchantTypeAvailCapture {
    pub capture_sha256: [u8; 32],
    pub request_sha256: [u8; 32],
    pub installed_query_input_sha256: [u8; 32],
    pub available: bool,
}

/// One exact `LeaderData::has_preq` prerequisite read on the Good path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame0MerchantTypeAvailPreqRead {
    pub ordinal: u8,
    pub resolved_type: i32,
    pub has_tech: bool,
}

/// Source-owned answer for the golden strict Good `type_avail` child.
///
/// The `leader_tech_bit_read` field is intentionally `None`: GoodType returns through the
/// non-Unit/non-Build/non-government arm before retail reaches `LeaderData + 0x6c18`. Keeping
/// that negative read fact in the receipt prevents a later adapter from silently treating the
/// leader's availability mask as an input to this path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0MerchantTypeAvailReceipt {
    pub receipt_sha256: [u8; 32],
    pub request_sha256: [u8; 32],
    pub replay_file_sha256: [u8; 32],
    pub replay_payload_sha256: [u8; 32],
    pub serialized_rules_sha256: [u8; 32],
    pub type_base_span: ReplayByteSpan,
    pub object_span: ReplayByteSpan,
    pub good_span: ReplayByteSpan,
    pub leader: i32,
    pub leader_tribe: i32,
    pub type_index: i32,
    pub tribe_mask: u32,
    pub prerequisite_reads: [Frame0MerchantTypeAvailPreqRead; 2],
    pub obs_type: i32,
    pub strict_obs_has_tech: bool,
    pub has_preq_raw: i32,
    pub tribe_can_type_raw: i32,
    pub type_eligible_raw: i32,
    pub leader_tech_bit_read: Option<bool>,
    pub raw_availability: i32,
    pub available: bool,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub rng_draws: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame0MerchantGoodLookupFrontier {
    Resolved(Frame0MerchantGoodLookupReceipt),
    NeedsTypeAvail(Frame0MerchantTypeAvailRequest),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame0MerchantTypeAvailError {
    Lookup(Frame0MerchantGoodLookupError),
    ReplayRead(String),
    WrongGoldenReplayFile,
    ReplayPayloadDisagreement,
    MissingReplayRules,
    Rules(PrePairUnitAuthorityError),
    StaleRequest,
    MissingReplayLeader { leader: i32 },
    InvalidLeader { leader: i32 },
    LeaderIdentityDisagreement,
    LeaderTribeDisagreement { replay: i32, owner: i32 },
    TypeOwnerDisagreement,
    WrongGoldenGoodFacts,
    InvalidReceiptDigest,
    StaleReceipt,
}

impl fmt::Display for Frame0MerchantTypeAvailError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "2024 frame-zero Merchant type availability refused: {self:?}"
        )
    }
}

impl std::error::Error for Frame0MerchantTypeAvailError {}

impl From<Frame0MerchantGoodLookupError> for Frame0MerchantTypeAvailError {
    fn from(value: Frame0MerchantGoodLookupError) -> Self {
        Self::Lookup(value)
    }
}

impl From<PrePairUnitAuthorityError> for Frame0MerchantTypeAvailError {
    fn from(value: PrePairUnitAuthorityError) -> Self {
        Self::Rules(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame0MerchantGoodLookupError {
    Parent(Frame0MerchantSearchError),
    UnsupportedCallShape,
    WorldCellOutOfBounds { world_x: i32, world_y: i32 },
    ObjectChainCycle { owner: i16, object: i16 },
    UnsupportedObjectAddress { owner: i16, object: i16 },
    StaleObjectRegistry { owner: i16, object: i16 },
    MissingGoodSlot { slot: i16, logical_length: usize },
    NullGoodTypePointer { slot: i16 },
    InvalidTypeAvailRequestDigest,
    StaleTypeAvailRequest,
    MissingTypeAvailAuthorityDigest,
    InvalidTypeAvailCaptureDigest,
    InvalidSearchReceiptDigest,
    StaleSearchReceipt,
    UnexpectedSearchStep,
    UnsupportedMerchantDomain { domain: i32 },
    WrongGoldenCollisionPredicates,
}

impl fmt::Display for Frame0MerchantGoodLookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "2024 frame-zero Merchant Good lookup refused: {self:?}")
    }
}

impl std::error::Error for Frame0MerchantGoodLookupError {}

impl From<Frame0MerchantSearchError> for Frame0MerchantGoodLookupError {
    fn from(value: Frame0MerchantSearchError) -> Self {
        Self::Parent(value)
    }
}

fn append_identity(image: &mut Vec<u8>, identity: Frame0MerchantObjectIdentity) {
    match identity {
        Frame0MerchantObjectIdentity::Unit { handle, row, uid } => {
            image.push(0);
            image.extend_from_slice(&handle.id.to_le_bytes());
            image.extend_from_slice(&handle.generation.to_le_bytes());
            image.extend_from_slice(&(row as u64).to_le_bytes());
            image.extend_from_slice(&uid.to_le_bytes());
        }
        Frame0MerchantObjectIdentity::Build { row, uid } => {
            image.push(1);
            image.extend_from_slice(&(row as u64).to_le_bytes());
            image.extend_from_slice(&uid.to_le_bytes());
        }
        Frame0MerchantObjectIdentity::Wall { row, uid } => {
            image.push(2);
            image.extend_from_slice(&(row as u64).to_le_bytes());
            image.extend_from_slice(&uid.to_le_bytes());
        }
    }
}

fn append_link(image: &mut Vec<u8>, link: &Frame0MerchantObjectLinkRead) {
    image.extend_from_slice(&(link.ordinal as u64).to_le_bytes());
    image.extend_from_slice(&link.owner.to_le_bytes());
    image.extend_from_slice(&link.object.to_le_bytes());
    append_identity(image, link.identity);
    image.extend_from_slice(&link.next_object.to_le_bytes());
    image.extend_from_slice(&link.next_owner.to_le_bytes());
}

fn append_good(image: &mut Vec<u8>, good: Option<Frame0MerchantGoodSlotRead>) {
    let Some(good) = good else {
        image.push(0);
        return;
    };
    image.push(1);
    image.extend_from_slice(&(good.logical_length as u64).to_le_bytes());
    image.extend_from_slice(&good.slot.to_le_bytes());
    image.push(good.flags);
    match good.ptype_present {
        None => image.push(0),
        Some(value) => {
            image.push(1);
            image.push(u8::from(value));
        }
    }
    match good.type_index {
        None => image.push(0),
        Some(value) => {
            image.push(1);
            image.extend_from_slice(&value.to_le_bytes());
        }
    }
}

fn append_prefix(
    image: &mut Vec<u8>,
    child: &Frame0MerchantGoodLookupRequest,
    world_index: usize,
    initial_object: i16,
    initial_owner: i16,
    links: &[Frame0MerchantObjectLinkRead],
    terminal_object: i16,
    terminal_owner: i16,
    good: Option<Frame0MerchantGoodSlotRead>,
    random_state: i32,
) {
    image.extend_from_slice(&child.request_sha256);
    image.extend_from_slice(&OBJECTS_FIND_GOOD_AT_WCOORD_VA.to_le_bytes());
    image.extend_from_slice(&child.world_x.to_le_bytes());
    image.extend_from_slice(&child.world_y.to_le_bytes());
    image.extend_from_slice(&(world_index as u64).to_le_bytes());
    image.extend_from_slice(&initial_object.to_le_bytes());
    image.extend_from_slice(&initial_owner.to_le_bytes());
    image.extend_from_slice(&(links.len() as u64).to_le_bytes());
    for link in links {
        append_link(image, link);
    }
    image.extend_from_slice(&terminal_object.to_le_bytes());
    image.extend_from_slice(&terminal_owner.to_le_bytes());
    append_good(image, good);
    image.extend_from_slice(&random_state.to_le_bytes());
}

#[allow(clippy::too_many_arguments)]
fn traversal_prefix_digest(
    child: &Frame0MerchantGoodLookupRequest,
    world_index: usize,
    initial_object: i16,
    initial_owner: i16,
    links: &[Frame0MerchantObjectLinkRead],
    terminal_object: i16,
    terminal_owner: i16,
    good: Option<Frame0MerchantGoodSlotRead>,
    random_state: i32,
) -> [u8; 32] {
    let mut image = Vec::with_capacity(192 + links.len() * 48);
    append_prefix(
        &mut image,
        child,
        world_index,
        initial_object,
        initial_owner,
        links,
        terminal_object,
        terminal_owner,
        good,
        random_state,
    );
    sha256(&image)
}

pub fn frame0_merchant_type_avail_request_digest(
    request: &Frame0MerchantTypeAvailRequest,
) -> [u8; 32] {
    let mut image = Vec::with_capacity(128);
    image.extend_from_slice(&request.lookup_request_sha256);
    image.extend_from_slice(&request.traversal_prefix_sha256);
    image.extend_from_slice(&request.call_va.to_le_bytes());
    image.extend_from_slice(&request.leader.to_le_bytes());
    image.extend_from_slice(&request.type_index.to_le_bytes());
    image.extend_from_slice(&request.strict.to_le_bytes());
    image.extend_from_slice(&request.good_slot.to_le_bytes());
    image.extend_from_slice(&request.random_state.to_le_bytes());
    sha256(&image)
}

pub fn frame0_merchant_type_avail_capture_digest(
    capture: &Frame0MerchantTypeAvailCapture,
) -> [u8; 32] {
    let mut image = Vec::with_capacity(96);
    image.extend_from_slice(&capture.request_sha256);
    image.extend_from_slice(&capture.installed_query_input_sha256);
    image.push(u8::from(capture.available));
    sha256(&image)
}

fn append_span(image: &mut Vec<u8>, span: ReplayByteSpan) {
    image.extend_from_slice(&(span.offset as u64).to_le_bytes());
    image.extend_from_slice(&(span.bytes as u64).to_le_bytes());
}

pub fn frame0_merchant_type_avail_receipt_digest(
    receipt: &Frame0MerchantTypeAvailReceipt,
) -> [u8; 32] {
    let mut image = Vec::with_capacity(320);
    image.extend_from_slice(&receipt.request_sha256);
    image.extend_from_slice(&receipt.replay_file_sha256);
    image.extend_from_slice(&receipt.replay_payload_sha256);
    image.extend_from_slice(&receipt.serialized_rules_sha256);
    append_span(&mut image, receipt.type_base_span);
    append_span(&mut image, receipt.object_span);
    append_span(&mut image, receipt.good_span);
    image.extend_from_slice(&receipt.leader.to_le_bytes());
    image.extend_from_slice(&receipt.leader_tribe.to_le_bytes());
    image.extend_from_slice(&receipt.type_index.to_le_bytes());
    image.extend_from_slice(&receipt.tribe_mask.to_le_bytes());
    for read in receipt.prerequisite_reads {
        image.push(read.ordinal);
        image.extend_from_slice(&read.resolved_type.to_le_bytes());
        image.push(u8::from(read.has_tech));
    }
    image.extend_from_slice(&receipt.obs_type.to_le_bytes());
    image.push(u8::from(receipt.strict_obs_has_tech));
    image.extend_from_slice(&receipt.has_preq_raw.to_le_bytes());
    image.extend_from_slice(&receipt.tribe_can_type_raw.to_le_bytes());
    image.extend_from_slice(&receipt.type_eligible_raw.to_le_bytes());
    match receipt.leader_tech_bit_read {
        None => image.push(0),
        Some(value) => {
            image.push(1);
            image.push(u8::from(value));
        }
    }
    image.extend_from_slice(&receipt.raw_availability.to_le_bytes());
    image.push(u8::from(receipt.available));
    image.extend_from_slice(&receipt.random_state_before.to_le_bytes());
    image.extend_from_slice(&receipt.random_state_after.to_le_bytes());
    image.extend_from_slice(&receipt.rng_draws.to_le_bytes());
    sha256(&image)
}

pub fn frame0_merchant_good_lookup_receipt_digest(
    receipt: &Frame0MerchantGoodLookupReceipt,
) -> [u8; 32] {
    let mut image = Vec::with_capacity(224 + receipt.object_links.len() * 48);
    image.extend_from_slice(&receipt.lookup_request_sha256);
    image.extend_from_slice(&receipt.call_va.to_le_bytes());
    image.extend_from_slice(&receipt.world_x.to_le_bytes());
    image.extend_from_slice(&receipt.world_y.to_le_bytes());
    image.extend_from_slice(&(receipt.world_index as u64).to_le_bytes());
    image.extend_from_slice(&receipt.initial_object.to_le_bytes());
    image.extend_from_slice(&receipt.initial_owner.to_le_bytes());
    image.extend_from_slice(&(receipt.object_links.len() as u64).to_le_bytes());
    for link in &receipt.object_links {
        append_link(&mut image, link);
    }
    image.extend_from_slice(&receipt.terminal_object.to_le_bytes());
    image.extend_from_slice(&receipt.terminal_owner.to_le_bytes());
    append_good(&mut image, receipt.good);
    match receipt.type_avail_checked {
        None => image.push(0),
        Some(value) => {
            image.push(1);
            image.push(u8::from(value));
        }
    }
    match receipt.type_avail_capture {
        None => image.push(0),
        Some(capture) => {
            image.push(1);
            image.extend_from_slice(&capture.capture_sha256);
            image.extend_from_slice(&capture.request_sha256);
            image.extend_from_slice(&capture.installed_query_input_sha256);
            image.push(u8::from(capture.available));
        }
    }
    match receipt.outcome {
        Frame0MerchantGoodLookupOutcome::FoundType(value) => {
            image.push(0);
            image.extend_from_slice(&value.to_le_bytes());
        }
        Frame0MerchantGoodLookupOutcome::NotFound(reason) => {
            image.push(1);
            image.push(match reason {
                Frame0MerchantGoodNotFoundReason::TerminalWasNotGood => 0,
                Frame0MerchantGoodNotFoundReason::InactiveGood => 1,
                Frame0MerchantGoodNotFoundReason::OilGood => 2,
                Frame0MerchantGoodNotFoundReason::TypeUnavailable => 3,
            });
        }
    }
    image.extend_from_slice(&receipt.return_value.to_le_bytes());
    image.extend_from_slice(&receipt.random_state_before.to_le_bytes());
    image.extend_from_slice(&receipt.random_state_after.to_le_bytes());
    image.extend_from_slice(&receipt.rng_draws.to_le_bytes());
    sha256(&image)
}

fn band_for(object: i16) -> Option<RetailBand> {
    let object = i32::from(object);
    RetailBand::ALL
        .into_iter()
        .find(|band| band.contains(object))
}

fn read_i16(image: &[u8], offset: usize) -> i16 {
    i16::from_le_bytes(
        image[offset..offset + 2]
            .try_into()
            .expect("fixed ObjectData i16 window"),
    )
}

fn canonical_object_link(
    sim: &Sim,
    ordinal: usize,
    owner: i16,
    object: i16,
) -> Result<Frame0MerchantObjectLinkRead, Frame0MerchantGoodLookupError> {
    let owner_u8 = u8::try_from(owner)
        .map_err(|_| Frame0MerchantGoodLookupError::UnsupportedObjectAddress { owner, object })?;
    let band = band_for(object)
        .ok_or(Frame0MerchantGoodLookupError::UnsupportedObjectAddress { owner, object })?;
    let address = RetailObjectAddress::new(owner_u8, band, i32::from(object));
    if !address.is_well_formed() {
        return Err(Frame0MerchantGoodLookupError::UnsupportedObjectAddress { owner, object });
    }
    let identity = sim
        .world
        .object_bands()
        .live_identity(address)
        .ok_or(Frame0MerchantGoodLookupError::StaleObjectRegistry { owner, object })?;
    let (identity, next_object, next_owner) = match identity {
        WorldObjectIdentity::Unit { id, generation } if band == RetailBand::Unit => {
            let handle = Handle { id, generation };
            let row = sim
                .world
                .row_of(handle)
                .ok_or(Frame0MerchantGoodLookupError::StaleObjectRegistry { owner, object })?;
            if sim.world.units.get_who(row) != owner_u8 || sim.world.units.o()[row] != object {
                return Err(Frame0MerchantGoodLookupError::StaleObjectRegistry { owner, object });
            }
            (
                Frame0MerchantObjectIdentity::Unit {
                    handle,
                    row,
                    uid: sim.world.units.get_uid(row),
                },
                sim.world.units.down()[row],
                sim.world.units.down_who()[row],
            )
        }
        WorldObjectIdentity::BuildRow(row) if band == RetailBand::Build => {
            let row = row as usize;
            let build = sim
                .builds
                .get(row)
                .ok_or(Frame0MerchantGoodLookupError::StaleObjectRegistry { owner, object })?;
            if build.who != owner_u8 || build.object_id() != object {
                return Err(Frame0MerchantGoodLookupError::StaleObjectRegistry { owner, object });
            }
            (
                Frame0MerchantObjectIdentity::Build {
                    row,
                    uid: build.uid,
                },
                read_i16(&build.other, 0x2c),
                read_i16(&build.other, 0x2e),
            )
        }
        WorldObjectIdentity::WallRow(row) if band == RetailBand::Wall => {
            let row = row as usize;
            let wall = sim
                .walls
                .get(row)
                .ok_or(Frame0MerchantGoodLookupError::StaleObjectRegistry { owner, object })?;
            if wall.who != owner_u8 {
                return Err(Frame0MerchantGoodLookupError::StaleObjectRegistry { owner, object });
            }
            (
                Frame0MerchantObjectIdentity::Wall { row, uid: wall.uid },
                wall.down,
                wall.down_who,
            )
        }
        _ => {
            return Err(Frame0MerchantGoodLookupError::StaleObjectRegistry { owner, object });
        }
    };
    Ok(Frame0MerchantObjectLinkRead {
        ordinal,
        owner,
        object,
        identity,
        next_object,
        next_owner,
    })
}

#[allow(clippy::too_many_arguments)]
fn resolved_receipt(
    child: &Frame0MerchantGoodLookupRequest,
    world_index: usize,
    initial_object: i16,
    initial_owner: i16,
    object_links: Vec<Frame0MerchantObjectLinkRead>,
    terminal_object: i16,
    terminal_owner: i16,
    good: Option<Frame0MerchantGoodSlotRead>,
    type_avail_checked: Option<bool>,
    type_avail_capture: Option<Frame0MerchantTypeAvailCapture>,
    outcome: Frame0MerchantGoodLookupOutcome,
    random_state: i32,
) -> Frame0MerchantGoodLookupReceipt {
    let return_value = match outcome {
        Frame0MerchantGoodLookupOutcome::FoundType(value) => value,
        Frame0MerchantGoodLookupOutcome::NotFound(_) => NOT_FOUND,
    };
    let mut receipt = Frame0MerchantGoodLookupReceipt {
        receipt_sha256: [0; 32],
        lookup_request_sha256: child.request_sha256,
        call_va: OBJECTS_FIND_GOOD_AT_WCOORD_VA,
        world_x: child.world_x,
        world_y: child.world_y,
        world_index,
        initial_object,
        initial_owner,
        object_links,
        terminal_object,
        terminal_owner,
        good,
        type_avail_checked,
        type_avail_capture,
        outcome,
        return_value,
        random_state_before: random_state,
        random_state_after: random_state,
        rng_draws: 0,
    };
    receipt.receipt_sha256 = frame0_merchant_good_lookup_receipt_digest(&receipt);
    receipt
}

fn evaluate_prevalidated(
    sim: &Sim,
    child: &Frame0MerchantGoodLookupRequest,
    goods: &OilGoodRuntime,
) -> Result<Frame0MerchantGoodLookupFrontier, Frame0MerchantGoodLookupError> {
    if child.find_good_at_call_va != OBJECTS_FIND_GOOD_AT_WCOORD_VA
        || child.lookup_arg4 != 0
        || child.lookup_arg5 != 0
        || child.lookup_owner < 0
    {
        return Err(Frame0MerchantGoodLookupError::UnsupportedCallShape);
    }
    let world = &sim.map.world;
    if !world.valid_w(child.world_x, child.world_y) {
        return Err(Frame0MerchantGoodLookupError::WorldCellOutOfBounds {
            world_x: child.world_x,
            world_y: child.world_y,
        });
    }
    let random_state = sim.world.random.state();
    let world_index = world.w_index(child.world_x, child.world_y);
    let cell = &world.wdata[world_index];
    let initial_object = cell.down;
    let initial_owner = cell.down_who;
    let mut terminal_object = initial_object;
    let mut terminal_owner = initial_owner;
    let mut links = Vec::new();
    let mut seen = BTreeSet::new();
    while terminal_object >= 0 {
        if !seen.insert((terminal_owner, terminal_object)) {
            return Err(Frame0MerchantGoodLookupError::ObjectChainCycle {
                owner: terminal_owner,
                object: terminal_object,
            });
        }
        let link = canonical_object_link(sim, links.len(), terminal_owner, terminal_object)?;
        terminal_object = link.next_object;
        terminal_owner = link.next_owner;
        links.push(link);
    }

    if terminal_object != GOOD_TERMINAL {
        return Ok(Frame0MerchantGoodLookupFrontier::Resolved(
            resolved_receipt(
                child,
                world_index,
                initial_object,
                initial_owner,
                links,
                terminal_object,
                terminal_owner,
                None,
                None,
                None,
                Frame0MerchantGoodLookupOutcome::NotFound(
                    Frame0MerchantGoodNotFoundReason::TerminalWasNotGood,
                ),
                random_state,
            ),
        ));
    }

    let slot = usize::try_from(terminal_owner).map_err(|_| {
        Frame0MerchantGoodLookupError::MissingGoodSlot {
            slot: terminal_owner,
            logical_length: goods.slots.len(),
        }
    })?;
    let terminal_good =
        goods
            .slots
            .get(slot)
            .ok_or(Frame0MerchantGoodLookupError::MissingGoodSlot {
                slot: terminal_owner,
                logical_length: goods.slots.len(),
            })?;
    if !terminal_good.active() {
        let good = Frame0MerchantGoodSlotRead {
            logical_length: goods.slots.len(),
            slot: terminal_owner,
            flags: terminal_good.node.flags,
            ptype_present: None,
            type_index: None,
        };
        return Ok(Frame0MerchantGoodLookupFrontier::Resolved(
            resolved_receipt(
                child,
                world_index,
                initial_object,
                initial_owner,
                links,
                terminal_object,
                terminal_owner,
                Some(good),
                None,
                None,
                Frame0MerchantGoodLookupOutcome::NotFound(
                    Frame0MerchantGoodNotFoundReason::InactiveGood,
                ),
                random_state,
            ),
        ));
    }
    if !terminal_good.ptype_present {
        return Err(Frame0MerchantGoodLookupError::NullGoodTypePointer {
            slot: terminal_owner,
        });
    }
    let type_index = terminal_good.node.type_index;
    let good = Frame0MerchantGoodSlotRead {
        logical_length: goods.slots.len(),
        slot: terminal_owner,
        flags: terminal_good.node.flags,
        ptype_present: Some(true),
        type_index: Some(type_index),
    };
    if type_index == OIL_GOOD_TYPE {
        return Ok(Frame0MerchantGoodLookupFrontier::Resolved(
            resolved_receipt(
                child,
                world_index,
                initial_object,
                initial_owner,
                links,
                terminal_object,
                terminal_owner,
                Some(good),
                None,
                None,
                Frame0MerchantGoodLookupOutcome::NotFound(
                    Frame0MerchantGoodNotFoundReason::OilGood,
                ),
                random_state,
            ),
        ));
    }

    let prefix = traversal_prefix_digest(
        child,
        world_index,
        initial_object,
        initial_owner,
        &links,
        terminal_object,
        terminal_owner,
        Some(good),
        random_state,
    );
    let mut request = Frame0MerchantTypeAvailRequest {
        request_sha256: [0; 32],
        lookup_request_sha256: child.request_sha256,
        traversal_prefix_sha256: prefix,
        call_va: LEADER_TYPE_AVAIL_VA,
        leader: child.lookup_owner,
        type_index,
        strict: 1,
        good_slot: terminal_owner,
        random_state,
    };
    request.request_sha256 = frame0_merchant_type_avail_request_digest(&request);
    Ok(Frame0MerchantGoodLookupFrontier::NeedsTypeAvail(request))
}

/// Advance one exact Merchant Good lookup against canonical owners without mutation.
pub fn advance_frame0_merchant_good_lookup(
    sim: &Sim,
    parent: &Frame0MerchantSpotRequest,
    member: &CanonicalSetupUnitMemberReceipt,
    child: &Frame0MerchantGoodLookupRequest,
    goods: &OilGoodRuntime,
) -> Result<Frame0MerchantGoodLookupFrontier, Frame0MerchantGoodLookupError> {
    validate_frame0_merchant_good_lookup_request(sim, parent, member, child)?;
    evaluate_prevalidated(sim, child, goods)
}

fn live_type_row_agrees(facts: ReplayGoodTypeFacts, types: &TypeBuiltinState) -> bool {
    let Ok(index) = usize::try_from(facts.type_index) else {
        return false;
    };
    let Some(row) = types.types.rows().get(index) else {
        return false;
    };
    row.index == facts.type_index
        && row.common.tribe_mask == facts.tribe_mask
        && row.common.preq == facts.preq
        && row.from == facts.from_type
        && row.where_type == facts.where_type
}

fn source_owned_type_avail_receipt(
    request: &Frame0MerchantTypeAvailRequest,
    replay_file_sha256: [u8; 32],
    replay_payload_sha256: [u8; 32],
    serialized_rules_sha256: [u8; 32],
    facts: ReplayGoodTypeFacts,
    leader_tribe: i32,
) -> Result<Frame0MerchantTypeAvailReceipt, Frame0MerchantTypeAvailError> {
    // GoodTypeData::num_preq is the constant two. In the admitted golden Rules every Good
    // has the two identity prerequisites and the strict observation sentinel. A different
    // row is not this golden cone and must not be approximated with the same proof.
    if facts.preq != [-1, -1, -1] || facts.obs != -2 {
        return Err(Frame0MerchantTypeAvailError::WrongGoldenGoodFacts);
    }
    let prerequisite_reads = [
        Frame0MerchantTypeAvailPreqRead {
            ordinal: 0,
            resolved_type: -1,
            has_tech: true,
        },
        Frame0MerchantTypeAvailPreqRead {
            ordinal: 1,
            resolved_type: -1,
            has_tech: true,
        },
    ];
    let tribe_bit = u32::try_from(leader_tribe)
        .ok()
        .and_then(|shift| 1u32.checked_shl(shift))
        .unwrap_or(0);
    let tribe_can_type_raw = if facts.tribe_mask & tribe_bit != 0 {
        4
    } else {
        0
    };
    // `type_eligible(type,1)` returns the tribe result before its strict Good arm. When the
    // tribe admits the row, `has_tech(obs=-2)` is false and that arm returns 4.
    let type_eligible_raw = if tribe_can_type_raw == 4 { 4 } else { 0 };
    // GoodType is neither Unit, Build, nor government. `type_avail` therefore returns 4 at
    // 0x006e3458 before the Unit-only availability-mask block at 0x006e34ab.
    let raw_availability = type_eligible_raw;
    let random_state = request.random_state;
    let mut receipt = Frame0MerchantTypeAvailReceipt {
        receipt_sha256: [0; 32],
        request_sha256: request.request_sha256,
        replay_file_sha256,
        replay_payload_sha256,
        serialized_rules_sha256,
        type_base_span: facts.spans.type_base,
        object_span: facts.spans.object,
        good_span: facts.spans.good,
        leader: request.leader,
        leader_tribe,
        type_index: facts.type_index,
        tribe_mask: facts.tribe_mask,
        prerequisite_reads,
        obs_type: facts.obs,
        strict_obs_has_tech: false,
        has_preq_raw: 1,
        tribe_can_type_raw,
        type_eligible_raw,
        leader_tech_bit_read: None,
        raw_availability,
        available: raw_availability != 0,
        random_state_before: random_state,
        random_state_after: random_state,
        rng_draws: 0,
    };
    receipt.receipt_sha256 = frame0_merchant_type_avail_receipt_digest(&receipt);
    Ok(receipt)
}

/// Resolve the golden Good `LeaderData::type_avail(type,1)` child from replay-carried Rules
/// and the canonical live Leader/type owner.
///
/// The World/Object/Good prefix is re-evaluated first, so the request cannot be transplanted
/// across a changed chain. No replay checksum or post-state is accepted as an input.
#[allow(clippy::too_many_arguments)]
pub fn resolve_frame0_merchant_type_avail(
    replay: &Replay,
    sim: &Sim,
    parent: &Frame0MerchantSpotRequest,
    member: &CanonicalSetupUnitMemberReceipt,
    child: &Frame0MerchantGoodLookupRequest,
    goods: &OilGoodRuntime,
    types: &TypeBuiltinState,
    request: &Frame0MerchantTypeAvailRequest,
) -> Result<Frame0MerchantTypeAvailReceipt, Frame0MerchantTypeAvailError> {
    let current = advance_frame0_merchant_good_lookup(sim, parent, member, child, goods)?;
    let Frame0MerchantGoodLookupFrontier::NeedsTypeAvail(current) = current else {
        return Err(Frame0MerchantTypeAvailError::StaleRequest);
    };
    if current != *request {
        return Err(Frame0MerchantTypeAvailError::StaleRequest);
    }

    let replay_bytes = std::fs::read(&replay.path)
        .map_err(|error| Frame0MerchantTypeAvailError::ReplayRead(error.to_string()))?;
    let replay_file_sha256 = sha256(&replay_bytes);
    if replay_file_sha256 != REPLAY_FILE_SHA256 {
        return Err(Frame0MerchantTypeAvailError::WrongGoldenReplayFile);
    }
    let payload = load_payload(&replay.path)
        .map_err(|error| Frame0MerchantTypeAvailError::ReplayRead(error.to_string()))?;
    let replay_payload_sha256 = sha256(&payload);
    if replay_payload_sha256 != replay.initial.payload_sha256 {
        return Err(Frame0MerchantTypeAvailError::ReplayPayloadDisagreement);
    }
    let rules = replay
        .initial
        .rules
        .ok_or(Frame0MerchantTypeAvailError::MissingReplayRules)?;
    let facts = replay_good_type_facts(&payload, &rules, request.type_index)?;
    if !live_type_row_agrees(facts, types) {
        return Err(Frame0MerchantTypeAvailError::TypeOwnerDisagreement);
    }

    let leader = usize::try_from(request.leader)
        .ok()
        .filter(|&leader| leader < types.leaders.len())
        .ok_or(Frame0MerchantTypeAvailError::InvalidLeader {
            leader: request.leader,
        })?;
    let live_leader = &types.leaders[leader];
    let sim_leader =
        sim.vic_leaders
            .slots
            .get(leader)
            .ok_or(Frame0MerchantTypeAvailError::InvalidLeader {
                leader: request.leader,
            })?;
    if sim_leader.who != request.leader {
        return Err(Frame0MerchantTypeAvailError::LeaderIdentityDisagreement);
    }
    let replay_leader = replay
        .initial
        .info
        .players
        .iter()
        .find(|player| player.present && i32::from(player.who) == request.leader)
        .ok_or(Frame0MerchantTypeAvailError::MissingReplayLeader {
            leader: request.leader,
        })?;
    let replay_tribe = i32::from(replay_leader.tribe);
    if live_leader.tribe != replay_tribe {
        return Err(Frame0MerchantTypeAvailError::LeaderTribeDisagreement {
            replay: replay_tribe,
            owner: live_leader.tribe,
        });
    }

    source_owned_type_avail_receipt(
        request,
        replay_file_sha256,
        replay_payload_sha256,
        rules.serialized_sha256,
        facts,
        live_leader.tribe,
    )
}

fn complete_prevalidated(
    sim: &Sim,
    child: &Frame0MerchantGoodLookupRequest,
    goods: &OilGoodRuntime,
    request: &Frame0MerchantTypeAvailRequest,
    capture: &Frame0MerchantTypeAvailCapture,
) -> Result<Frame0MerchantGoodLookupReceipt, Frame0MerchantGoodLookupError> {
    if request.request_sha256 != frame0_merchant_type_avail_request_digest(request) {
        return Err(Frame0MerchantGoodLookupError::InvalidTypeAvailRequestDigest);
    }
    let Frame0MerchantGoodLookupFrontier::NeedsTypeAvail(current) =
        evaluate_prevalidated(sim, child, goods)?
    else {
        return Err(Frame0MerchantGoodLookupError::StaleTypeAvailRequest);
    };
    if current != *request {
        return Err(Frame0MerchantGoodLookupError::StaleTypeAvailRequest);
    }
    if capture.installed_query_input_sha256 == [0; 32] {
        return Err(Frame0MerchantGoodLookupError::MissingTypeAvailAuthorityDigest);
    }
    if capture.request_sha256 != request.request_sha256
        || capture.capture_sha256 != frame0_merchant_type_avail_capture_digest(capture)
    {
        return Err(Frame0MerchantGoodLookupError::InvalidTypeAvailCaptureDigest);
    }

    let world = &sim.map.world;
    let world_index = world.w_index(child.world_x, child.world_y);
    let cell = &world.wdata[world_index];
    let mut terminal_object = cell.down;
    let mut terminal_owner = cell.down_who;
    let mut links = Vec::new();
    while terminal_object >= 0 {
        let link = canonical_object_link(sim, links.len(), terminal_owner, terminal_object)?;
        terminal_object = link.next_object;
        terminal_owner = link.next_owner;
        links.push(link);
    }
    let terminal_good = &goods.slots[terminal_owner as usize];
    let good = Frame0MerchantGoodSlotRead {
        logical_length: goods.slots.len(),
        slot: terminal_owner,
        flags: terminal_good.node.flags,
        ptype_present: Some(true),
        type_index: Some(terminal_good.node.type_index),
    };
    let outcome = if capture.available {
        Frame0MerchantGoodLookupOutcome::FoundType(request.type_index)
    } else {
        Frame0MerchantGoodLookupOutcome::NotFound(Frame0MerchantGoodNotFoundReason::TypeUnavailable)
    };
    Ok(resolved_receipt(
        child,
        world_index,
        cell.down,
        cell.down_who,
        links,
        terminal_object,
        terminal_owner,
        Some(good),
        Some(capture.available),
        Some(*capture),
        outcome,
        sim.world.random.state(),
    ))
}

/// Complete the typed Leader child against an installed, digest-bound answer.
///
/// The entire World/Object/Good prefix is rerun first. A mutation to any reached input makes
/// the supplied request stale before its answer is observed.
#[allow(clippy::too_many_arguments)]
pub fn complete_frame0_merchant_good_lookup(
    sim: &Sim,
    parent: &Frame0MerchantSpotRequest,
    member: &CanonicalSetupUnitMemberReceipt,
    child: &Frame0MerchantGoodLookupRequest,
    goods: &OilGoodRuntime,
    request: &Frame0MerchantTypeAvailRequest,
    capture: &Frame0MerchantTypeAvailCapture,
) -> Result<Frame0MerchantGoodLookupReceipt, Frame0MerchantGoodLookupError> {
    validate_frame0_merchant_good_lookup_request(sim, parent, member, child)?;
    complete_prevalidated(sim, child, goods, request, capture)
}

/// Compose one source-owned Good availability receipt into the existing atomic lookup
/// transaction.
///
/// The generated detached capture is only a transport wrapper: its installed-input digest is
/// the source receipt digest, and the receipt is recomputed against every current owner before
/// the wrapper is accepted.
#[allow(clippy::too_many_arguments)]
pub fn complete_frame0_merchant_good_lookup_from_rules(
    replay: &Replay,
    sim: &Sim,
    parent: &Frame0MerchantSpotRequest,
    member: &CanonicalSetupUnitMemberReceipt,
    child: &Frame0MerchantGoodLookupRequest,
    goods: &OilGoodRuntime,
    types: &TypeBuiltinState,
    request: &Frame0MerchantTypeAvailRequest,
    receipt: &Frame0MerchantTypeAvailReceipt,
) -> Result<Frame0MerchantGoodLookupReceipt, Frame0MerchantTypeAvailError> {
    if receipt.receipt_sha256 != frame0_merchant_type_avail_receipt_digest(receipt) {
        return Err(Frame0MerchantTypeAvailError::InvalidReceiptDigest);
    }
    let current = resolve_frame0_merchant_type_avail(
        replay, sim, parent, member, child, goods, types, request,
    )?;
    if current != *receipt {
        return Err(Frame0MerchantTypeAvailError::StaleReceipt);
    }
    let mut capture = Frame0MerchantTypeAvailCapture {
        capture_sha256: [0; 32],
        request_sha256: request.request_sha256,
        installed_query_input_sha256: receipt.receipt_sha256,
        available: receipt.available,
    };
    capture.capture_sha256 = frame0_merchant_type_avail_capture_digest(&capture);
    complete_frame0_merchant_good_lookup(sim, parent, member, child, goods, request, &capture)
        .map_err(Frame0MerchantTypeAvailError::Lookup)
}

/// Stable digest over the exact `invalid_loc` read receipt.
pub fn frame0_merchant_invalid_loc_receipt_digest(
    receipt: &Frame0MerchantInvalidLocReceipt,
) -> [u8; 32] {
    let mut image = b"don-2024-frame0-merchant-invalid-loc-v1".to_vec();
    image.extend_from_slice(&receipt.call_va.to_le_bytes());
    image.extend_from_slice(&(receipt.candidate_index as u64).to_le_bytes());
    for value in [
        receipt.tile_x,
        receipt.tile_y,
        receipt.domain,
        receipt.world_x,
        receipt.world_y,
        i32::from(receipt.world_flags),
        i32::from(receipt.tile_mask),
        receipt.unit_masks as i32,
        receipt.return_value,
        receipt.random_state_before,
        receipt.random_state_after,
        receipt.rng_draws as i32,
    ] {
        image.extend_from_slice(&value.to_le_bytes());
    }
    image.push(u8::from(receipt.orders_empty));
    match receipt.cliff_predicate_read {
        None => image.push(0),
        Some(value) => {
            image.push(1);
            image.push(u8::from(value));
        }
    }
    sha256(&image)
}

fn append_invalid_loc(image: &mut Vec<u8>, receipt: &Frame0MerchantInvalidLocReceipt) {
    image.extend_from_slice(&receipt.receipt_sha256);
}

/// Stable digest over the first unsourced spatial-collision child.
pub fn frame0_merchant_collision_request_digest(
    request: &Frame0MerchantCollisionRequest,
) -> [u8; 32] {
    let mut image = b"don-2024-frame0-merchant-collision-request-v1".to_vec();
    image.extend_from_slice(&request.parent_request_sha256);
    image.extend_from_slice(&request.search_trace_sha256);
    image.extend_from_slice(&request.setup_composition_digest);
    image.extend_from_slice(&request.setup_authority_revision.to_le_bytes());
    image.extend_from_slice(&(request.invocation_ordinal as u64).to_le_bytes());
    image.push(request.actor_who);
    image.extend_from_slice(&request.actor_o.to_le_bytes());
    image.extend_from_slice(&request.actor_uid.to_le_bytes());
    for value in [
        request.actor_type,
        request.candidate_index as i32,
        request.tile_x,
        request.tile_y,
        request.good_merchant_spot_call_va as i32,
        request.call_va as i32,
        request.coord_x,
        request.coord_y,
        request.footprint_x,
        request.footprint_y,
        request.arg5,
        request.arg6,
        request.arg7,
        request.type_domain,
        request.type_unit_flags as i32,
        request.type_unit_flags2 as i32,
        request.random_state,
    ] {
        image.extend_from_slice(&value.to_le_bytes());
    }
    image.extend_from_slice(&request.gather_receipt_sha256);
    append_invalid_loc(&mut image, &request.invalid_loc);
    image.push(u8::from(request.type_is_siege));
    image.push(u8::from(request.unit_is_hero));
    image.push(u8::from(request.unit_is_supply));
    sha256(&image)
}

/// Stable digest over the ordered Good-call/answer chronology consumed so far.
pub fn frame0_merchant_search_trace_digest(steps: &[Frame0MerchantSearchGoodStep]) -> [u8; 32] {
    let mut image = b"don-2024-frame0-merchant-good-search-trace-v1".to_vec();
    image.extend_from_slice(&(steps.len() as u64).to_le_bytes());
    for step in steps {
        image.extend_from_slice(&step.request.request_sha256);
        image.extend_from_slice(&step.receipt.receipt_sha256);
    }
    sha256(&image)
}

fn continuation_failed_tail(
    parent: &Frame0MerchantSpotRequest,
    steps: &[Frame0MerchantSearchGoodStep],
    head_calc_gather_returned_zero: bool,
    reason: &'static str,
) -> Frame0MerchantSearchContinuationFrontier {
    Frame0MerchantSearchContinuationFrontier::FailedUnpackTail(
        Frame0MerchantCompletedSearchTailRequest {
            parent: parent.clone(),
            search_proof_sha256: frame0_merchant_search_trace_digest(steps),
            completed_good_steps: steps.len(),
            head_calc_gather_returned_zero,
            find_merchant_spot_outcome: MerchantSpotOutcome::NotFound,
            next_va: FAILED_UNPACK_TAIL_VA,
            reason,
            random_state_before: parent.random_state,
            random_state_after: parent.random_state,
            rng_draws: 0,
        },
    )
}

fn validate_current_search_receipt(
    sim: &Sim,
    goods: &OilGoodRuntime,
    step: &Frame0MerchantSearchGoodStep,
) -> Result<(), Frame0MerchantGoodLookupError> {
    if step.receipt.receipt_sha256 != frame0_merchant_good_lookup_receipt_digest(&step.receipt) {
        return Err(Frame0MerchantGoodLookupError::InvalidSearchReceiptDigest);
    }
    if step.receipt.lookup_request_sha256 != step.request.request_sha256 {
        return Err(Frame0MerchantGoodLookupError::StaleSearchReceipt);
    }
    let current = match evaluate_prevalidated(sim, &step.request, goods)? {
        Frame0MerchantGoodLookupFrontier::Resolved(receipt) => receipt,
        Frame0MerchantGoodLookupFrontier::NeedsTypeAvail(request) => {
            let capture = step
                .receipt
                .type_avail_capture
                .as_ref()
                .ok_or(Frame0MerchantGoodLookupError::StaleSearchReceipt)?;
            complete_prevalidated(sim, &step.request, goods, &request, capture)?
        }
    };
    if current != step.receipt {
        return Err(Frame0MerchantGoodLookupError::StaleSearchReceipt);
    }
    Ok(())
}

fn golden_land_invalid_loc_result(tile_mask: u16, unit_masks: u32) -> (Option<bool>, i32) {
    let surface = tile_mask & 0x30;
    let low = tile_mask & 3;
    // Retail short-circuits the cliff virtual when surface is 0x30 or low is 2.
    let cliff_predicate_read = (surface != 0x30 && low != 2).then_some(low == 1);
    let blocked_surface = if surface == 0x30 || low == 2 || cliff_predicate_read == Some(true) {
        surface != 0x30 || unit_masks & 0x4000 == 0
    } else {
        false
    };
    let return_value = if blocked_surface || surface == 0x20 {
        2
    } else {
        0
    };
    (cliff_predicate_read, return_value)
}

fn source_invalid_loc(
    sim: &Sim,
    parent: &Frame0MerchantSpotRequest,
    member: &CanonicalSetupUnitMemberReceipt,
    candidate_index: usize,
    tile_x: i32,
    tile_y: i32,
) -> Result<Frame0MerchantInvalidLocReceipt, Frame0MerchantGoodLookupError> {
    let domain = member.type_facts.domain;
    if domain != 0 {
        return Err(Frame0MerchantGoodLookupError::UnsupportedMerchantDomain { domain });
    }
    let world = &sim.map.world;
    let world_x = tile_x >> 2;
    let world_y = tile_y >> 2;
    let world_flags = world.wdata(world_x, world_y).flags;
    let tile_mask = world.tmask(tile_x, tile_y);
    let (cliff_predicate_read, return_value) =
        golden_land_invalid_loc_result(tile_mask, parent.actor.unit_masks);
    let random_state = sim.world.random.state();
    let mut receipt = Frame0MerchantInvalidLocReceipt {
        receipt_sha256: [0; 32],
        call_va: UNIT_INVALID_LOC_VA,
        candidate_index,
        tile_x,
        tile_y,
        orders_empty: parent.actor.orders.is_empty(),
        domain,
        world_x,
        world_y,
        world_flags,
        tile_mask,
        cliff_predicate_read,
        unit_masks: parent.actor.unit_masks,
        return_value,
        random_state_before: random_state,
        random_state_after: random_state,
        rng_draws: 0,
    };
    receipt.receipt_sha256 = frame0_merchant_invalid_loc_receipt_digest(&receipt);
    Ok(receipt)
}

fn continue_outer_candidates(
    sim: &Sim,
    parent: &Frame0MerchantSpotRequest,
    member: &CanonicalSetupUnitMemberReceipt,
    cached_good_obj: i16,
    steps: &[Frame0MerchantSearchGoodStep],
    start: usize,
) -> Result<Frame0MerchantSearchContinuationFrontier, Frame0MerchantGoodLookupError> {
    let candidates =
        frame0_merchant_candidate_tiles(parent.actor.x, parent.actor.y, MERCHANT_RADIUS)
            .ok_or(Frame0MerchantGoodLookupError::UnexpectedSearchStep)?;
    for (candidate_index, &(tile_x, tile_y)) in candidates.iter().enumerate().skip(start) {
        if !good_merchant_spot_terrain_prefix(&sim.map.world, tile_x, tile_y) {
            continue;
        }
        let calc_site = Frame0MerchantCalcGatherSite::GoodMerchantSpot {
            candidate_index,
            tile_x,
            tile_y,
        };
        if let Some(child) = next_calc_gather_good_lookup(
            &sim.map.world,
            parent,
            member.authority_revision,
            cached_good_obj,
            calc_site,
            tile_x.wrapping_mul(192),
            tile_y.wrapping_mul(192),
            None,
        ) {
            return Ok(Frame0MerchantSearchContinuationFrontier::NeedsGoodLookup(
                child,
            ));
        }
    }
    Ok(continuation_failed_tail(
        parent,
        steps,
        false,
        "all 49 ordered Merchant candidates failed good_merchant_spot; find_merchant_spot returned 0 and think_merchant continues with its Good-object scan",
    ))
}

fn continue_after_good_step(
    sim: &Sim,
    parent: &Frame0MerchantSpotRequest,
    member: &CanonicalSetupUnitMemberReceipt,
    cached_good_obj: i16,
    steps: &[Frame0MerchantSearchGoodStep],
) -> Result<Frame0MerchantSearchContinuationFrontier, Frame0MerchantGoodLookupError> {
    let step = steps
        .last()
        .ok_or(Frame0MerchantGoodLookupError::UnexpectedSearchStep)?;
    if matches!(
        step.receipt.outcome,
        Frame0MerchantGoodLookupOutcome::NotFound(_)
    ) {
        if let Some(child) = next_calc_gather_good_lookup(
            &sim.map.world,
            parent,
            member.authority_revision,
            cached_good_obj,
            step.request.calc_site,
            step.request.calc_coord_x,
            step.request.calc_coord_y,
            Some(&step.request),
        ) {
            return Ok(Frame0MerchantSearchContinuationFrontier::NeedsGoodLookup(
                child,
            ));
        }
        return match step.request.calc_site {
            Frame0MerchantCalcGatherSite::FindMerchantSpotHead => Ok(continuation_failed_tail(
                parent,
                steps,
                true,
                "the complete ordered head calc_gather scan returned 0; find_merchant_spot returned 0 and think_merchant continues with its Good-object scan",
            )),
            Frame0MerchantCalcGatherSite::GoodMerchantSpot {
                candidate_index, ..
            } => continue_outer_candidates(
                sim,
                parent,
                member,
                cached_good_obj,
                steps,
                candidate_index.saturating_add(1),
            ),
        };
    }

    match step.request.calc_site {
        Frame0MerchantCalcGatherSite::FindMerchantSpotHead => {
            continue_outer_candidates(sim, parent, member, cached_good_obj, steps, 0)
        }
        Frame0MerchantCalcGatherSite::GoodMerchantSpot {
            candidate_index,
            tile_x,
            tile_y,
        } => {
            if !good_merchant_spot_terrain_prefix(&sim.map.world, tile_x, tile_y) {
                return Err(Frame0MerchantGoodLookupError::StaleSearchReceipt);
            }
            let invalid_loc =
                source_invalid_loc(sim, parent, member, candidate_index, tile_x, tile_y)?;
            if invalid_loc.return_value != 0 {
                return continue_outer_candidates(
                    sim,
                    parent,
                    member,
                    cached_good_obj,
                    steps,
                    candidate_index.saturating_add(1),
                );
            }
            let type_is_siege = member.type_facts.unit_flags & 0x0002_0000 != 0;
            let unit_is_hero = member.type_facts.unit_flags2 & 0x20 != 0;
            let unit_is_supply = member.type_facts.unit_flags2 & 0x40 != 0;
            if type_is_siege || unit_is_hero || unit_is_supply {
                return Err(Frame0MerchantGoodLookupError::WrongGoldenCollisionPredicates);
            }
            let mut request = Frame0MerchantCollisionRequest {
                request_sha256: [0; 32],
                parent_request_sha256: parent.request_sha256,
                search_trace_sha256: frame0_merchant_search_trace_digest(steps),
                setup_composition_digest: parent.setup_composition_digest,
                setup_authority_revision: member.authority_revision,
                invocation_ordinal: parent.invocation_ordinal,
                actor_who: parent.actor.who,
                actor_o: parent.actor.o,
                actor_uid: parent.actor.uid,
                actor_type: parent.actor.type_index,
                candidate_index,
                tile_x,
                tile_y,
                good_merchant_spot_call_va: UNIT_GOOD_MERCHANT_SPOT_VA,
                gather_receipt_sha256: step.receipt.receipt_sha256,
                invalid_loc,
                call_va: UNIT_DETECT_COLLISION_VA,
                coord_x: tile_x.wrapping_mul(192),
                coord_y: tile_y.wrapping_mul(192),
                footprint_x: 1,
                footprint_y: 1,
                arg5: 0,
                arg6: 0,
                arg7: 0,
                type_domain: member.type_facts.domain,
                type_unit_flags: member.type_facts.unit_flags,
                type_unit_flags2: member.type_facts.unit_flags2,
                type_is_siege,
                unit_is_hero,
                unit_is_supply,
                random_state: sim.world.random.state(),
            };
            request.request_sha256 = frame0_merchant_collision_request_digest(&request);
            Ok(Frame0MerchantSearchContinuationFrontier::NeedsCollision(
                request,
            ))
        }
    }
}

/// Resume the exact `find_merchant_spot` transaction through an ordered sequence of completed
/// Good lookups.
///
/// Every prefix step is rerun against the current World/Object/Good owners. The returned
/// frontier is either the next retail `find_good_at`, the first spatial collision child, or
/// the actual failed-unpack `Unit::think_merchant` tail. This function is read-only and every
/// admitted branch preserves the RNG state.
pub fn advance_frame0_merchant_search_after_good_steps(
    sim: &Sim,
    parent: &Frame0MerchantSpotRequest,
    member: &CanonicalSetupUnitMemberReceipt,
    goods: &OilGoodRuntime,
    steps: &[Frame0MerchantSearchGoodStep],
) -> Result<Frame0MerchantSearchContinuationFrontier, Frame0MerchantGoodLookupError> {
    let cached_good_obj = sim
        .world
        .units
        .good_obj()
        .get(parent.actor.row)
        .copied()
        .ok_or(Frame0MerchantGoodLookupError::StaleSearchReceipt)?;
    let initial = advance_frame0_merchant_search(sim, parent, member)?;
    let mut frontier = match initial {
        Frame0MerchantSearchFrontier::NeedsGoodLookup(child) => {
            Frame0MerchantSearchContinuationFrontier::NeedsGoodLookup(child)
        }
        Frame0MerchantSearchFrontier::FailedUnpackTail(tail) => {
            if steps.is_empty() {
                return Ok(Frame0MerchantSearchContinuationFrontier::FailedUnpackTail(
                    Frame0MerchantCompletedSearchTailRequest {
                        parent: tail.parent,
                        search_proof_sha256: tail.local_search_proof_sha256,
                        completed_good_steps: 0,
                        head_calc_gather_returned_zero: tail.calc_gather_returned_zero,
                        find_merchant_spot_outcome: tail.find_merchant_spot_outcome,
                        next_va: tail.next_va,
                        reason: tail.reason,
                        random_state_before: parent.random_state,
                        random_state_after: parent.random_state,
                        rng_draws: 0,
                    },
                ));
            }
            return Err(Frame0MerchantGoodLookupError::UnexpectedSearchStep);
        }
    };

    for (index, step) in steps.iter().enumerate() {
        let Frame0MerchantSearchContinuationFrontier::NeedsGoodLookup(expected) = frontier else {
            return Err(Frame0MerchantGoodLookupError::UnexpectedSearchStep);
        };
        if expected != step.request {
            return Err(Frame0MerchantGoodLookupError::UnexpectedSearchStep);
        }
        validate_current_search_receipt(sim, goods, step)?;
        frontier =
            continue_after_good_step(sim, parent, member, cached_good_obj, &steps[..=index])?;
    }
    Ok(frontier)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup_2024_frame0_merchant_search::{
        frame0_merchant_good_lookup_digest, Frame0MerchantCalcGatherScan,
        Frame0MerchantCalcGatherSite,
    };
    use don_sim::systems::economy::GoodNode;
    use don_sim::systems::world_oil_goods::{OilGoodSlot, CLOSED_COORD_INTERNAL};

    fn type_avail_request(type_index: i32) -> Frame0MerchantTypeAvailRequest {
        let mut request = Frame0MerchantTypeAvailRequest {
            request_sha256: [0; 32],
            lookup_request_sha256: [1; 32],
            traversal_prefix_sha256: [2; 32],
            call_va: LEADER_TYPE_AVAIL_VA,
            leader: 0,
            type_index,
            strict: 1,
            good_slot: 0,
            random_state: 0x1234_5678,
        };
        request.request_sha256 = frame0_merchant_type_avail_request_digest(&request);
        request
    }

    fn good_type_facts(type_index: i32) -> ReplayGoodTypeFacts {
        ReplayGoodTypeFacts {
            spans: crate::groups_pre_pair_unit_authority::ReplayGoodTypeSpans {
                type_base: ReplayByteSpan {
                    offset: 0x100,
                    bytes: 90,
                },
                object: ReplayByteSpan {
                    offset: 0x200,
                    bytes: 152,
                },
                good: ReplayByteSpan {
                    offset: 0x300,
                    bytes: 68,
                },
            },
            type_index,
            tribe_mask: u32::MAX,
            preq: [-1, -1, -1],
            from_type: -1,
            where_type: -1,
            upgrade: -1,
            jump: -2,
            obs: -2,
        }
    }

    fn child() -> Frame0MerchantGoodLookupRequest {
        let mut request = Frame0MerchantGoodLookupRequest {
            request_sha256: [0; 32],
            parent_request_sha256: [1; 32],
            setup_composition_digest: [2; 32],
            setup_authority_revision: 7,
            invocation_ordinal: 1,
            actor_who: 0,
            actor_o: 1,
            actor_uid: 3,
            actor_type: 62,
            calc_gather_call_va: 0x0060_9180,
            calc_site: Frame0MerchantCalcGatherSite::FindMerchantSpotHead,
            calc_coord_x: 384,
            calc_coord_y: 384,
            upgrade_level: 0,
            gather_radius: 4,
            cached_good_obj: -1,
            scan: Frame0MerchantCalcGatherScan::OrderedCircle,
            circle_index: 0,
            tile_x: 2,
            tile_y: 2,
            tile_mask: 0x200,
            find_good_at_call_va: OBJECTS_FIND_GOOD_AT_WCOORD_VA,
            world_x: 0,
            world_y: 0,
            lookup_owner: 0,
            lookup_arg4: 0,
            lookup_arg5: 0,
        };
        request.request_sha256 = frame0_merchant_good_lookup_digest(&request);
        request
    }

    fn active_good(type_index: i32) -> OilGoodSlot {
        OilGoodSlot {
            node: GoodNode {
                flags: 1,
                who: u8::MAX,
                o: 0,
                z: CLOSED_COORD_INTERNAL,
                x: CLOSED_COORD_INTERNAL,
                y: CLOSED_COORD_INTERNAL,
                type_index,
                ever_seen: 0,
            },
            ptype_present: true,
            cur_time: 0,
        }
    }

    #[test]
    fn non_good_terminal_resolves_not_found_without_rng() {
        let sim = Sim::new(0x1234_5678, 8);
        let request = child();
        let before = sim.world.random.state();
        let out = evaluate_prevalidated(&sim, &request, &OilGoodRuntime::default()).unwrap();
        let Frame0MerchantGoodLookupFrontier::Resolved(receipt) = out else {
            panic!("empty WData must resolve locally");
        };
        assert_eq!(receipt.terminal_object, -1);
        assert_eq!(receipt.return_value, NOT_FOUND);
        assert_eq!(receipt.rng_draws, 0);
        assert_eq!(receipt.random_state_before, before);
        assert_eq!(receipt.random_state_after, before);
    }

    #[test]
    fn direct_inactive_and_oil_good_apply_filters_in_read_order() {
        let mut sim = Sim::new(9, 8);
        let request = child();
        sim.map.world.wdata[0].down = GOOD_TERMINAL;
        sim.map.world.wdata[0].down_who = 0;
        let mut goods = OilGoodRuntime::default();
        goods.slots.push(OilGoodSlot::default());
        let Frame0MerchantGoodLookupFrontier::Resolved(inactive) =
            evaluate_prevalidated(&sim, &request, &goods).unwrap()
        else {
            panic!("inactive Good must resolve locally");
        };
        assert_eq!(
            inactive.outcome,
            Frame0MerchantGoodLookupOutcome::NotFound(
                Frame0MerchantGoodNotFoundReason::InactiveGood
            )
        );
        assert_eq!(inactive.good.unwrap().ptype_present, None);

        goods.slots[0] = active_good(OIL_GOOD_TYPE);
        let Frame0MerchantGoodLookupFrontier::Resolved(oil) =
            evaluate_prevalidated(&sim, &request, &goods).unwrap()
        else {
            panic!("Oil Good must resolve locally");
        };
        assert_eq!(
            oil.outcome,
            Frame0MerchantGoodLookupOutcome::NotFound(Frame0MerchantGoodNotFoundReason::OilGood)
        );
    }

    #[test]
    fn live_unit_link_is_followed_before_terminal_good() {
        let mut sim = Sim::new(11, 8);
        sim.activate(0);
        let unit = sim.spawn_unit(0, 69, 384, 384, 1).unwrap();
        let row = sim.world.row_of(unit).unwrap();
        let object = sim.world.units.o()[row];
        sim.world.units.down_mut()[row] = GOOD_TERMINAL;
        sim.world.units.down_who_mut()[row] = 0;
        sim.map.world.wdata[0].down = object;
        sim.map.world.wdata[0].down_who = 0;
        let mut goods = OilGoodRuntime::default();
        goods.slots.push(active_good(17));
        let Frame0MerchantGoodLookupFrontier::NeedsTypeAvail(child) =
            evaluate_prevalidated(&sim, &child(), &goods).unwrap()
        else {
            panic!("rare Good must reach type_avail");
        };
        assert_eq!(child.leader, 0);
        assert_eq!(child.type_index, 17);
        assert_eq!(child.strict, 1);
        assert_eq!(child.good_slot, 0);
    }

    #[test]
    fn unit_build_wall_links_use_canonical_registry_identity_in_order() {
        let mut sim = Sim::new(12, 8);
        sim.activate(0);
        let unit = sim.spawn_unit(0, 69, 384, 384, 1).unwrap();
        let unit_row = sim.world.row_of(unit).unwrap();
        let unit_object = sim.world.units.o()[unit_row];

        let mut build = don_sim::systems::production::BuildData::default();
        build.other[0x2c..0x2e].copy_from_slice(&3_000i16.to_le_bytes());
        build.other[0x2e..0x30].copy_from_slice(&0i16.to_le_bytes());
        let build_row = sim.spawn_build(0, build);

        let wall = don_sim::systems::walls::WallState {
            down: GOOD_TERMINAL,
            down_who: 0,
            ..Default::default()
        };
        let wall_row = sim.spawn_wall(0, wall);

        sim.world.units.down_mut()[unit_row] = 2_000;
        sim.world.units.down_who_mut()[unit_row] = 0;
        sim.map.world.wdata[0].down = unit_object;
        sim.map.world.wdata[0].down_who = 0;

        let mut goods = OilGoodRuntime::default();
        goods.slots.push(active_good(17));
        let Frame0MerchantGoodLookupFrontier::NeedsTypeAvail(_) =
            evaluate_prevalidated(&sim, &child(), &goods).unwrap()
        else {
            panic!("rare Good must reach type_avail");
        };

        let mut object = unit_object;
        let mut owner = 0;
        let mut observed = Vec::new();
        while object >= 0 {
            let link = canonical_object_link(&sim, observed.len(), owner, object).unwrap();
            object = link.next_object;
            owner = link.next_owner;
            observed.push(link);
        }
        assert_eq!(observed.len(), 3);
        assert!(matches!(
            observed[0].identity,
            Frame0MerchantObjectIdentity::Unit { row, .. } if row == unit_row
        ));
        assert_eq!(
            observed[1].identity,
            Frame0MerchantObjectIdentity::Build {
                row: build_row,
                uid: sim.builds[build_row].uid,
            }
        );
        assert_eq!(
            observed[2].identity,
            Frame0MerchantObjectIdentity::Wall {
                row: wall_row,
                uid: sim.walls[wall_row].uid,
            }
        );
        assert_eq!((object, owner), (GOOD_TERMINAL, 0));
    }

    #[test]
    fn linked_object_mutation_changes_the_typed_child() {
        let mut sim = Sim::new(13, 8);
        sim.activate(0);
        let unit = sim.spawn_unit(0, 69, 384, 384, 1).unwrap();
        let row = sim.world.row_of(unit).unwrap();
        sim.map.world.wdata[0].down = sim.world.units.o()[row];
        sim.map.world.wdata[0].down_who = 0;
        sim.world.units.down_mut()[row] = GOOD_TERMINAL;
        sim.world.units.down_who_mut()[row] = 0;
        let mut goods = OilGoodRuntime::default();
        goods.slots.push(active_good(17));
        let Frame0MerchantGoodLookupFrontier::NeedsTypeAvail(first) =
            evaluate_prevalidated(&sim, &child(), &goods).unwrap()
        else {
            panic!("rare Good must reach type_avail");
        };
        sim.world.units.down_who_mut()[row] = 1;
        goods.slots.push(active_good(17));
        let Frame0MerchantGoodLookupFrontier::NeedsTypeAvail(second) =
            evaluate_prevalidated(&sim, &child(), &goods).unwrap()
        else {
            panic!("mutated rare Good must still reach type_avail");
        };
        assert_ne!(first.request_sha256, second.request_sha256);
        assert_ne!(
            first.traversal_prefix_sha256,
            second.traversal_prefix_sha256
        );
    }

    #[test]
    fn cycle_is_refused_instead_of_hanging() {
        let mut sim = Sim::new(15, 8);
        sim.activate(0);
        let unit = sim.spawn_unit(0, 69, 384, 384, 1).unwrap();
        let row = sim.world.row_of(unit).unwrap();
        let object = sim.world.units.o()[row];
        sim.map.world.wdata[0].down = object;
        sim.map.world.wdata[0].down_who = 0;
        sim.world.units.down_mut()[row] = object;
        sim.world.units.down_who_mut()[row] = 0;
        assert_eq!(
            evaluate_prevalidated(&sim, &child(), &OilGoodRuntime::default()),
            Err(Frame0MerchantGoodLookupError::ObjectChainCycle { owner: 0, object })
        );
    }

    #[test]
    fn active_good_with_null_type_pointer_fails_closed() {
        let mut sim = Sim::new(16, 8);
        sim.map.world.wdata[0].down = GOOD_TERMINAL;
        sim.map.world.wdata[0].down_who = 0;
        let mut goods = OilGoodRuntime::default();
        let mut good = active_good(17);
        good.ptype_present = false;
        goods.slots.push(good);
        assert_eq!(
            evaluate_prevalidated(&sim, &child(), &goods),
            Err(Frame0MerchantGoodLookupError::NullGoodTypePointer { slot: 0 })
        );
    }

    #[test]
    fn typed_capture_digest_binds_available_bit() {
        let mut capture = Frame0MerchantTypeAvailCapture {
            capture_sha256: [0; 32],
            request_sha256: [3; 32],
            installed_query_input_sha256: [4; 32],
            available: true,
        };
        capture.capture_sha256 = frame0_merchant_type_avail_capture_digest(&capture);
        let valid = capture.capture_sha256;
        capture.available = false;
        assert_ne!(valid, frame0_merchant_type_avail_capture_digest(&capture));
    }

    #[test]
    fn typed_capture_resolves_found_or_not_found_and_stales_on_good_mutation() {
        let mut sim = Sim::new(17, 8);
        sim.map.world.wdata[0].down = GOOD_TERMINAL;
        sim.map.world.wdata[0].down_who = 0;
        let mut goods = OilGoodRuntime::default();
        goods.slots.push(active_good(17));
        let lookup = child();
        let Frame0MerchantGoodLookupFrontier::NeedsTypeAvail(request) =
            evaluate_prevalidated(&sim, &lookup, &goods).unwrap()
        else {
            panic!("rare Good must reach type_avail");
        };

        let mut capture = Frame0MerchantTypeAvailCapture {
            capture_sha256: [0; 32],
            request_sha256: request.request_sha256,
            installed_query_input_sha256: [0x91; 32],
            available: true,
        };
        capture.capture_sha256 = frame0_merchant_type_avail_capture_digest(&capture);
        let found = complete_prevalidated(&sim, &lookup, &goods, &request, &capture).unwrap();
        assert_eq!(
            found.outcome,
            Frame0MerchantGoodLookupOutcome::FoundType(17)
        );
        assert_eq!(found.return_value, 17);
        assert_eq!(found.type_avail_capture, Some(capture));

        capture.available = false;
        capture.capture_sha256 = frame0_merchant_type_avail_capture_digest(&capture);
        let absent = complete_prevalidated(&sim, &lookup, &goods, &request, &capture).unwrap();
        assert_eq!(
            absent.outcome,
            Frame0MerchantGoodLookupOutcome::NotFound(
                Frame0MerchantGoodNotFoundReason::TypeUnavailable
            )
        );
        assert_eq!(absent.return_value, NOT_FOUND);

        goods.slots[0].node.type_index = 18;
        assert_eq!(
            complete_prevalidated(&sim, &lookup, &goods, &request, &capture),
            Err(Frame0MerchantGoodLookupError::StaleTypeAvailRequest)
        );
    }

    #[test]
    fn setup_2024_frame0_merchant_type_avail_returns_four_before_leader_tech_mask() {
        let request = type_avail_request(17);
        let receipt = source_owned_type_avail_receipt(
            &request,
            [3; 32],
            [4; 32],
            [5; 32],
            good_type_facts(17),
            22,
        )
        .unwrap();
        assert_eq!(receipt.prerequisite_reads[0].resolved_type, -1);
        assert_eq!(receipt.prerequisite_reads[1].resolved_type, -1);
        assert_eq!(receipt.has_preq_raw, 1);
        assert!(!receipt.strict_obs_has_tech);
        assert_eq!(receipt.tribe_can_type_raw, 4);
        assert_eq!(receipt.type_eligible_raw, 4);
        assert_eq!(receipt.leader_tech_bit_read, None);
        assert_eq!(receipt.raw_availability, 4);
        assert!(receipt.available);
        assert_eq!(receipt.rng_draws, 0);
        assert_eq!(receipt.random_state_before, request.random_state);
        assert_eq!(receipt.random_state_after, request.random_state);
    }

    #[test]
    fn setup_2024_frame0_merchant_type_avail_resolves_unavailable_without_tech_read() {
        let request = type_avail_request(17);
        let mut facts = good_type_facts(17);
        facts.tribe_mask = 1 << 3;
        let receipt =
            source_owned_type_avail_receipt(&request, [3; 32], [4; 32], [5; 32], facts, 22)
                .unwrap();
        assert_eq!(receipt.tribe_can_type_raw, 0);
        assert_eq!(receipt.type_eligible_raw, 0);
        assert_eq!(receipt.leader_tech_bit_read, None);
        assert_eq!(receipt.raw_availability, 0);
        assert!(!receipt.available);
        assert_eq!(receipt.rng_draws, 0);
    }

    #[test]
    fn setup_2024_frame0_merchant_type_avail_stops_at_non_golden_prerequisite() {
        let request = type_avail_request(17);
        let mut facts = good_type_facts(17);
        facts.preq[1] = 544;
        assert_eq!(
            source_owned_type_avail_receipt(&request, [3; 32], [4; 32], [5; 32], facts, 22,),
            Err(Frame0MerchantTypeAvailError::WrongGoldenGoodFacts)
        );
    }

    #[test]
    fn setup_2024_frame0_merchant_type_avail_digest_binds_every_source() {
        let request = type_avail_request(17);
        let receipt = source_owned_type_avail_receipt(
            &request,
            [3; 32],
            [4; 32],
            [5; 32],
            good_type_facts(17),
            22,
        )
        .unwrap();
        let digest = receipt.receipt_sha256;
        let mut mutant = receipt.clone();
        mutant.leader_tribe = 21;
        assert_ne!(digest, frame0_merchant_type_avail_receipt_digest(&mutant));
        mutant = receipt.clone();
        mutant.available = false;
        assert_ne!(digest, frame0_merchant_type_avail_receipt_digest(&mutant));
        mutant = receipt;
        mutant.serialized_rules_sha256[0] ^= 1;
        assert_ne!(digest, frame0_merchant_type_avail_receipt_digest(&mutant));
    }

    #[test]
    fn golden_land_invalid_loc_preserves_retail_short_circuit_order() {
        assert_eq!(
            golden_land_invalid_loc_result(0x0000, 0x0008_0000),
            (Some(false), 0)
        );
        assert_eq!(
            golden_land_invalid_loc_result(0x0010, 0x0008_0000),
            (Some(false), 0)
        );
        assert_eq!(
            golden_land_invalid_loc_result(0x0001, 0x0008_0000),
            (Some(true), 2)
        );
        assert_eq!(
            golden_land_invalid_loc_result(0x0002, 0x0008_0000),
            (None, 2)
        );
        assert_eq!(
            golden_land_invalid_loc_result(0x0020, 0x0008_0000),
            (Some(false), 2)
        );
        assert_eq!(
            golden_land_invalid_loc_result(0x0030, 0x0008_0000),
            (None, 2)
        );
        assert_eq!(
            golden_land_invalid_loc_result(0x0030, 0x0008_4000),
            (None, 0)
        );
    }

    #[test]
    fn collision_child_digest_binds_trace_and_spatial_call_shape() {
        let mut invalid_loc = Frame0MerchantInvalidLocReceipt {
            receipt_sha256: [0; 32],
            call_va: UNIT_INVALID_LOC_VA,
            candidate_index: 3,
            tile_x: 11,
            tile_y: 17,
            orders_empty: true,
            domain: 0,
            world_x: 2,
            world_y: 4,
            world_flags: 0,
            tile_mask: 0x200,
            cliff_predicate_read: Some(false),
            unit_masks: 0x0008_0000,
            return_value: 0,
            random_state_before: 19,
            random_state_after: 19,
            rng_draws: 0,
        };
        invalid_loc.receipt_sha256 = frame0_merchant_invalid_loc_receipt_digest(&invalid_loc);
        let mut request = Frame0MerchantCollisionRequest {
            request_sha256: [0; 32],
            parent_request_sha256: [1; 32],
            search_trace_sha256: [2; 32],
            setup_composition_digest: [3; 32],
            setup_authority_revision: 4,
            invocation_ordinal: 1,
            actor_who: 0,
            actor_o: 2,
            actor_uid: 9,
            actor_type: 62,
            candidate_index: 3,
            tile_x: 11,
            tile_y: 17,
            good_merchant_spot_call_va: UNIT_GOOD_MERCHANT_SPOT_VA,
            gather_receipt_sha256: [4; 32],
            invalid_loc,
            call_va: UNIT_DETECT_COLLISION_VA,
            coord_x: 11 * 192,
            coord_y: 17 * 192,
            footprint_x: 1,
            footprint_y: 1,
            arg5: 0,
            arg6: 0,
            arg7: 0,
            type_domain: 0,
            type_unit_flags: 0x0120_1882,
            type_unit_flags2: 6,
            type_is_siege: false,
            unit_is_hero: false,
            unit_is_supply: false,
            random_state: 19,
        };
        request.request_sha256 = frame0_merchant_collision_request_digest(&request);
        let digest = request.request_sha256;
        request.search_trace_sha256[0] ^= 1;
        assert_ne!(digest, frame0_merchant_collision_request_digest(&request));
        request.search_trace_sha256[0] ^= 1;
        request.footprint_x = 2;
        assert_ne!(digest, frame0_merchant_collision_request_digest(&request));
    }
}
