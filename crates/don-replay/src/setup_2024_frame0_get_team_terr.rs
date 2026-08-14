// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact frame-zero `LeaderData::get_team_terr` child for the golden strategy pass.
//!
//! `get_team_terr` (`0x006d62e0`) has two materially different relation paths. Once
//! `Game::frame` is nonzero it can call `LeaderData::is_ally`; at exact frame zero it instead
//! calls `LeaderData::get_player` and compares `GameInfo::Player::team`. The existing simulator
//! helper represents the later ally path and is therefore not evidence for this call.
//!
//! This module binds the complete frame-zero input surface: `GameInfo::team_style`, all eight
//! Player flag/who/team rows, and all eight current Leader flag/who/territory rows. Both are
//! extracted from one canonical whole-Sim call-entry capture; post-`Game::init_teams` rows,
//! setup-time Leader rows, and missing final-territory production are not accepted as
//! substitutes. Resolution is detached and has no canonical writes.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::systems::save_load::{save_sim, SaveError};
use don_sim::systems::unit_inctime::SUPPORTED_RETAIL_EXE_SHA256;
use don_sim::tick::Sim;

use crate::setup_2024_frame0_plan_strategy::{
    bind_golden_frame0_owner0_plan_strategy_entry, frame0_plan_strategy_entry_authority_digest,
    plan_golden_frame0_owner0_plan_strategy_prefix, validate_frame0_get_team_terr_request,
    Frame0GetTeamTerrInputSurface, Frame0GetTeamTerrRequest, Frame0PlanStrategyEntryAuthority,
    Frame0PlanStrategyError, Frame0PlanStrategyPrefixPlan, GET_TEAM_TERR_CALL_VA, GET_TEAM_TERR_VA,
    GOLDEN_FIRST_STRATEGY_ORDINAL, GOLDEN_FIRST_STRATEGY_OWNER, GOLDEN_FRAME, GOLDEN_STEP,
};
use crate::setup_2024_frame379::REPLAY_FILE_SHA256;
use crate::world_owner_frontier::sha256;

pub const GET_PLAYER_VA: u32 = 0x006e_c0f0;
pub const IS_ALLY_VA: u32 = 0x006e_db50;
pub const LEADER_SLOTS: usize = 8;
pub const LEADER_STRIDE: u32 = 0x6eec;
pub const LEADER_FLAGS_OFFSET: u32 = 0x0000;
pub const LEADER_WHO_OFFSET: u32 = 0x0008;
pub const LEADER_TERRITORY_OFFSET: u32 = 0x09d8;
pub const LEADER_OTHER_TEAM_TERR_OFFSET: u32 = 0x09e4;
pub const LEADER_MIN_OTHER_TEAM_TERR_OFFSET: u32 = 0x09e8;
pub const LEADER_MY_TEAM_TERR_OFFSET: u32 = 0x09ec;
pub const GAME_TEAM_STYLE_OFFSET: u32 = 0x0024;
pub const GAME_PLAYER_ZERO_OFFSET: u32 = 0x0074;
pub const GAME_PLAYER_STRIDE: u32 = 0x008c;
pub const GAME_FRAME_OFFSET: u32 = 0x0550;

const PLAYER_VALID: u16 = 0x0001;
const PLAYER_DEFERRED_MATCH_MASK: u16 = 0x0050;
const LEADER_VALID: i32 = 0x0001;
const TEAM_STYLE_SPECIAL: u8 = 7;
const TEAM_OBSERVER: i8 = 8;
const FIRST_NORMAL_TEAM: i8 = 0;
const NORMAL_TEAM_COUNT: i8 = 4;
const LEADER_ACTIVE: i32 = 0x0002;
const DIPLO_ALLY: i32 = 2;

const STORE_MY_TEAM_TERR_VA: u32 = 0x006b_98da;
const STORE_OTHER_TEAM_TERR_VA: u32 = 0x006b_98e5;
const STORE_MIN_OTHER_TEAM_TERR_VA: u32 = 0x006b_98ef;
const FIRST_DIRECTIONAL_DIPLO_READ_VA: u32 = 0x006b_9910;
const REVERSE_DIRECTIONAL_DIPLO_READ_VA: u32 = 0x006b_9925;
const OPPONENT_GET_TEAM_TERR_CALL_VA: u32 = 0x006b_992e;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Frame0GetTeamTerrCallEntrySource {
    /// One supported-retail whole-Sim capture at owner zero's frame-zero step-11
    /// `Leader::plan_strategy` entry. The parent prefix and this child projection name the same
    /// boundary; neither is reconstructed from the completed setup image.
    CompleteRetailOwnerZeroPlanStrategyEntry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Frame0GetTeamTerrGameSource {
    /// Exact live GameInfo projection extracted from the supported-retail whole-Sim call-entry
    /// capture. Setup-time team rows alone do not produce this variant.
    SourceBackedGameInfoPlayerTeamProjection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Frame0GetTeamTerrLeaderSource {
    /// Exact supported-retail eight-row Leader projection at the native child entry.
    CompleteRetailGetTeamTerrCallEntry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame0GetTeamTerrPlayerRow {
    pub flags: u16,
    pub who: u8,
    pub team: i8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame0GetTeamTerrLeaderRow {
    pub leader_flags: i32,
    pub who: i32,
    pub territory: i32,
}

/// Three native Leader scalars overwritten immediately after the first `get_team_terr`
/// return. Don's current canonical Sim does not serialize this strategy-only trio, so the
/// supported-retail call-entry capture retains their exact preimages explicitly instead of
/// silently substituting zeros.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame0PlanStrategyTeamTerritoryPreimage {
    pub other_team_terr: i32,
    pub min_other_team_terr: i32,
    pub my_team_terr: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0GetTeamTerrGameProjection {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub source: Frame0GetTeamTerrGameSource,
    pub frame: i32,
    pub team_style: u8,
    pub players: [Frame0GetTeamTerrPlayerRow; LEADER_SLOTS],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0GetTeamTerrLeaderProjection {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub source: Frame0GetTeamTerrLeaderSource,
    pub call_entry_sim_sha256: [u8; 32],
    pub leaders: [Frame0GetTeamTerrLeaderRow; LEADER_SLOTS],
}

/// Minimal source attestation for the complete whole-Sim input image. The projected rows are
/// deliberately not caller fields: the binder extracts them from `call_entry` only after its
/// canonical DoNSave bytes match this hash and the parent strategy authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0GetTeamTerrCallEntryCapture {
    pub revision: u64,
    pub source: Frame0GetTeamTerrCallEntrySource,
    pub native_trace_sha256: [u8; 32],
    pub replay_file_sha256: [u8; 32],
    pub executable_sha256: [u8; 32],
    pub plan_strategy_entry_authority_digest: [u8; 32],
    pub call_entry_sim_sha256: [u8; 32],
    pub frame: i32,
    pub step: u8,
    pub owner: u8,
    pub strategy_ordinal: u8,
    pub team_territory_preimage: Frame0PlanStrategyTeamTerritoryPreimage,
}

/// Complete immutable input authority for native `get_team_terr` at exact frame zero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0GetTeamTerrCallEntryAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub source: Frame0GetTeamTerrCallEntrySource,
    pub native_trace_sha256: [u8; 32],
    pub replay_file_sha256: [u8; 32],
    pub executable_sha256: [u8; 32],
    pub plan_strategy_entry_authority_digest: [u8; 32],
    pub call_entry_sim_sha256: [u8; 32],
    pub expected_request_sha256: [u8; 32],
    pub local_prefix_digest: [u8; 32],
    pub local_prefix_preserved_input_projection: bool,
    pub frame: i32,
    pub step: u8,
    pub owner: u8,
    pub strategy_ordinal: u8,
    pub team_territory_preimage: Frame0PlanStrategyTeamTerritoryPreimage,
    pub game: Frame0GetTeamTerrGameProjection,
    pub leader: Frame0GetTeamTerrLeaderProjection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0GetPlayerReceipt {
    pub call_ordinal: usize,
    pub receiver_leader_slot: u8,
    pub receiver_who: i32,
    pub visited_player_rows: Vec<u8>,
    pub deferred_special_matches: Vec<u8>,
    pub returned_early: bool,
    pub result_player_row: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame0GetTeamTerrDisposition {
    ReceiverSelf,
    InvalidLeader,
    ReceiverPlayerObserver,
    CandidatePlayerObserver,
    ReceiverTeamOutsideNormalRange {
        team: i8,
    },
    DifferentFrameZeroTeam {
        receiver_team: i8,
        candidate_team: i8,
    },
    SameFrameZeroTeam {
        team: i8,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame0GetTeamTerrVisit {
    pub leader_slot: u8,
    pub leader_who: i32,
    pub territory: i32,
    pub sum_before: i32,
    pub sum_after: i32,
    pub disposition: Frame0GetTeamTerrDisposition,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0GetTeamTerrReceipt {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub replay_file_sha256: [u8; 32],
    pub executable_sha256: [u8; 32],
    pub request: Frame0GetTeamTerrRequest,
    pub native_trace_sha256: [u8; 32],
    pub call_entry_authority_digest: [u8; 32],
    pub game_projection_digest: [u8; 32],
    pub leader_projection_digest: [u8; 32],
    /// The parent-produced local prefix writes only escrow, City scratch, and planning scratch;
    /// it cannot alter any Game/Player or flags/who/territory field projected here.
    pub local_prefix_preserved_input_projection: bool,
    pub receiver_leader_slot: u8,
    pub get_player_calls: Vec<Frame0GetPlayerReceipt>,
    pub visits: Vec<Frame0GetTeamTerrVisit>,
    pub result: i32,
    pub return_va: u32,
    pub is_ally_reached: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Frame0PlanStrategyTeamTerritoryField {
    MyTeamTerr,
    OtherTeamTerr,
    MinOtherTeamTerr,
}

/// An instruction-ordered native store after the first team-territory child returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame0PlanStrategyTeamTerritoryWrite {
    pub instruction_va: u32,
    pub field: Frame0PlanStrategyTeamTerritoryField,
    pub before: i32,
    pub after: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Frame0PlanStrategyDiplomacyReadDirection {
    CandidateTowardReceiver,
}

/// Exact first unowned input after the three post-child stores and the receiver-self loop row.
/// Retail short-circuits after this read when the value is not `2`; the reverse direction is
/// therefore deliberately not bundled into this request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0PlanStrategyDiplomacyReadRequest {
    pub request_sha256: [u8; 32],
    pub call_entry_authority_digest: [u8; 32],
    pub first_get_team_terr_receipt_digest: [u8; 32],
    pub staged_writes_digest: [u8; 32],
    pub call_entry_sim_sha256: [u8; 32],
    pub receiver_leader_slot: u8,
    pub receiver_who: i32,
    pub candidate_leader_slot: u8,
    pub candidate_who: i32,
    pub direction: Frame0PlanStrategyDiplomacyReadDirection,
    pub target_who_index: i32,
    pub read_va: u32,
    pub ally_value: i32,
    pub reverse_read_va_if_ally: u32,
    pub get_team_terr_call_va_if_not_ally: u32,
}

/// Detached atomic continuation. `prior_local_prefix` and `writes` are one staged journal;
/// neither may be installed until the diplomacy child and the rest of `plan_strategy` close.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0PlanStrategyPostTeamTerrPlan {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub call_entry_authority_digest: [u8; 32],
    pub prior_local_prefix: Frame0PlanStrategyPrefixPlan,
    pub first_get_team_terr: Frame0GetTeamTerrReceipt,
    pub before: Frame0PlanStrategyTeamTerritoryPreimage,
    pub after_local: Frame0PlanStrategyTeamTerritoryPreimage,
    pub writes: [Frame0PlanStrategyTeamTerritoryWrite; 3],
    pub receiver_self_slot_skipped: u8,
    pub open: Frame0PlanStrategyDiplomacyReadRequest,
    pub owner0_strategy_complete: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Frame0PlanStrategyDiplomacyReadSource {
    /// The exact directional relation extracted from the same canonical call-entry Sim whose
    /// DoNSave hash produced the strict team-territory authority.
    CompleteRetailPlanStrategyCallEntry,
}

/// Source-owned value for the first `candidate.diplos[receiver_who]` read. Only this one
/// directional cell is projected; the reverse relation remains a later boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0PlanStrategyDiplomacyReadAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub source: Frame0PlanStrategyDiplomacyReadSource,
    pub replay_file_sha256: [u8; 32],
    pub executable_sha256: [u8; 32],
    pub call_entry_authority_digest: [u8; 32],
    pub staged_plan_digest: [u8; 32],
    pub request_sha256: [u8; 32],
    pub call_entry_sim_sha256: [u8; 32],
    pub candidate_leader_slot: u8,
    pub candidate_who: i32,
    pub target_who_index: i32,
    pub read_va: u32,
    pub value: i32,
}

/// The second directional read reached only when the first relation equals retail ally `2`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0PlanStrategyReverseDiplomacyReadRequest {
    pub request_sha256: [u8; 32],
    pub first_read_authority_digest: [u8; 32],
    pub staged_plan_digest: [u8; 32],
    pub call_entry_sim_sha256: [u8; 32],
    pub receiver_leader_slot: u8,
    pub receiver_who: i32,
    pub candidate_leader_slot: u8,
    pub candidate_who: i32,
    pub target_who_index: i32,
    pub read_va: u32,
    pub ally_value: i32,
    pub get_team_terr_call_va_if_not_ally: u32,
}

/// Exact repeated child call reached when either directional relation is not ally `2`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0PlanStrategyOpponentGetTeamTerrRequest {
    pub request_sha256: [u8; 32],
    pub first_read_authority_digest: [u8; 32],
    pub staged_plan_digest: [u8; 32],
    pub call_entry_authority_digest: [u8; 32],
    pub call_entry_sim_sha256: [u8; 32],
    pub receiver_leader_slot: u8,
    pub receiver_who: i32,
    pub callsite_va: u32,
    pub callee_va: u32,
    pub input_surface: Frame0GetTeamTerrInputSurface,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame0PlanStrategyDiplomacyStepOpen {
    ReverseRead(Frame0PlanStrategyReverseDiplomacyReadRequest),
    OpponentGetTeamTerr(Frame0PlanStrategyOpponentGetTeamTerrRequest),
}

/// No Leader write occurs between `0x006B9910` and either successor. The complete staged prefix
/// is retained byte-for-byte and still cannot be installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0PlanStrategyDiplomacyStepPlan {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub staged: Frame0PlanStrategyPostTeamTerrPlan,
    pub first_read: Frame0PlanStrategyDiplomacyReadAuthority,
    pub first_relation_is_ally: bool,
    pub open: Frame0PlanStrategyDiplomacyStepOpen,
    pub owner0_strategy_complete: bool,
}

/// Source-owned value for `receiver.diplos[candidate_who]` at `0x006B9925`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0PlanStrategyReverseDiplomacyReadAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub source: Frame0PlanStrategyDiplomacyReadSource,
    pub replay_file_sha256: [u8; 32],
    pub executable_sha256: [u8; 32],
    pub call_entry_authority_digest: [u8; 32],
    pub diplomacy_step_digest: [u8; 32],
    pub request_sha256: [u8; 32],
    pub call_entry_sim_sha256: [u8; 32],
    pub receiver_leader_slot: u8,
    pub receiver_who: i32,
    pub candidate_leader_slot: u8,
    pub candidate_who: i32,
    pub target_who_index: i32,
    pub read_va: u32,
    pub value: i32,
}

/// Exact repeated `get_team_terr(candidate)` return at `0x006B9933`. The receipt is detached:
/// max/min opponent-territory stores and later loop rows remain unexecuted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0PlanStrategyOpponentGetTeamTerrReceipt {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub replay_file_sha256: [u8; 32],
    pub executable_sha256: [u8; 32],
    pub request: Frame0PlanStrategyOpponentGetTeamTerrRequest,
    pub call_entry_authority_digest: [u8; 32],
    pub diplomacy_step_digest: [u8; 32],
    pub game_projection_digest: [u8; 32],
    pub leader_projection_digest: [u8; 32],
    pub staged_writes_preserved_input_projection: bool,
    pub receiver_leader_slot: u8,
    pub get_player_calls: Vec<Frame0GetPlayerReceipt>,
    pub visits: Vec<Frame0GetTeamTerrVisit>,
    pub result: i32,
    pub return_va: u32,
    pub is_ally_reached: bool,
    pub owner0_strategy_complete: bool,
}

#[derive(Debug)]
pub enum Frame0GetTeamTerrCallEntryBindError {
    Parent(Frame0PlanStrategyError),
    MissingCaptureRevision,
    WrongCaptureSource,
    ReplayMismatch,
    UnsupportedExecutable,
    ParentAuthorityMismatch,
    CaptureBoundaryMismatch,
    MissingNativeTrace,
    NativeTraceMismatch,
    Snapshot(SaveError),
    CallEntrySnapshotMismatch,
    WrongWorldFrame { expected: i32, actual: i32 },
    WrongGameFrame { expected: i32, actual: i32 },
    MissingPlayerTable,
    OwnerLeaderProjectionMismatch,
}

impl fmt::Display for Frame0GetTeamTerrCallEntryBindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "2024 frame-zero get_team_terr entry refused: {self:?}")
    }
}

impl std::error::Error for Frame0GetTeamTerrCallEntryBindError {}

impl From<Frame0PlanStrategyError> for Frame0GetTeamTerrCallEntryBindError {
    fn from(value: Frame0PlanStrategyError) -> Self {
        Self::Parent(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame0GetTeamTerrError {
    InvalidParentRequest,
    InvalidCallEntryAuthority,
    ParentAuthorityMismatch,
    CallEntrySnapshotMismatch,
    InvalidGameProjectionDigest,
    InvalidLeaderProjectionDigest,
    ReceiverOutsideLeaderTable,
    ReceiverWhoMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame0PlanStrategyPostTeamTerrError {
    InvalidParentPrefix,
    InvalidCallEntryAuthority,
    InvalidFirstChildReceipt,
    NoQualifyingDiplomacyCandidate,
}

#[derive(Debug)]
pub enum Frame0PlanStrategyDiplomacyReadBindError {
    InvalidStagedPlan,
    InvalidCallEntryAuthority,
    Snapshot(SaveError),
    CallEntrySnapshotMismatch,
    ActorProjectionMismatch,
    TargetIndexOutsideLeaderTable,
}

impl fmt::Display for Frame0PlanStrategyDiplomacyReadBindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "2024 frame-zero strategy diplomacy read refused: {self:?}"
        )
    }
}

impl std::error::Error for Frame0PlanStrategyDiplomacyReadBindError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame0PlanStrategyDiplomacyStepError {
    InvalidStagedPlan,
    InvalidReadAuthority,
}

#[derive(Debug)]
pub enum Frame0PlanStrategyReverseDiplomacyReadBindError {
    InvalidStagedPlan,
    WrongBranch,
    InvalidCallEntryAuthority,
    Snapshot(SaveError),
    CallEntrySnapshotMismatch,
    ActorProjectionMismatch,
    TargetIndexOutsideLeaderTable,
}

impl fmt::Display for Frame0PlanStrategyReverseDiplomacyReadBindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "2024 frame-zero reverse diplomacy read refused: {self:?}"
        )
    }
}

impl std::error::Error for Frame0PlanStrategyReverseDiplomacyReadBindError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame0PlanStrategyOpponentGetTeamTerrError {
    InvalidStagedPlan,
    WrongBranch,
    InvalidCallEntryAuthority,
    InvalidRequest,
    ReceiverOutsideLeaderTable,
    ReceiverWhoMismatch,
}

impl fmt::Display for Frame0PlanStrategyOpponentGetTeamTerrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "2024 frame-zero opponent get_team_terr refused: {self:?}"
        )
    }
}

impl std::error::Error for Frame0PlanStrategyOpponentGetTeamTerrError {}

impl fmt::Display for Frame0PlanStrategyDiplomacyStepError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "2024 frame-zero strategy diplomacy step refused: {self:?}"
        )
    }
}

impl std::error::Error for Frame0PlanStrategyDiplomacyStepError {}

impl fmt::Display for Frame0PlanStrategyPostTeamTerrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "2024 frame-zero post-team-territory prefix refused: {self:?}"
        )
    }
}

impl std::error::Error for Frame0PlanStrategyPostTeamTerrError {}

impl fmt::Display for Frame0GetTeamTerrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "2024 frame-zero get_team_terr refused: {self:?}")
    }
}

impl std::error::Error for Frame0GetTeamTerrError {}

fn append_player(image: &mut Vec<u8>, row: Frame0GetTeamTerrPlayerRow) {
    image.extend_from_slice(&row.flags.to_le_bytes());
    image.push(row.who);
    image.push(row.team as u8);
}

fn append_leader(image: &mut Vec<u8>, row: Frame0GetTeamTerrLeaderRow) {
    image.extend_from_slice(&row.leader_flags.to_le_bytes());
    image.extend_from_slice(&row.who.to_le_bytes());
    image.extend_from_slice(&row.territory.to_le_bytes());
}

/// Stable digest of the exact Game/Player input projection, excluding the digest field itself.
pub fn frame0_get_team_terr_game_projection_digest(
    projection: &Frame0GetTeamTerrGameProjection,
) -> [u8; 32] {
    let mut image = b"don-2024-frame0-get-team-terr-game-projection-v1".to_vec();
    image.extend_from_slice(&projection.revision.to_le_bytes());
    image.push(projection.source as u8);
    image.extend_from_slice(&projection.frame.to_le_bytes());
    image.push(projection.team_style);
    for player in projection.players {
        append_player(&mut image, player);
    }
    sha256(&image)
}

/// Stable digest of the current eight-Leader input projection, excluding the digest field.
pub fn frame0_get_team_terr_leader_projection_digest(
    projection: &Frame0GetTeamTerrLeaderProjection,
) -> [u8; 32] {
    let mut image = b"don-2024-frame0-get-team-terr-leader-projection-v1".to_vec();
    image.extend_from_slice(&projection.revision.to_le_bytes());
    image.push(projection.source as u8);
    image.extend_from_slice(&projection.call_entry_sim_sha256);
    for leader in projection.leaders {
        append_leader(&mut image, leader);
    }
    sha256(&image)
}

/// Stable identity of the unified call-entry authority. Both projection digests are themselves
/// recomputable from the extracted rows, so this digest cannot retain a stale row silently.
pub fn frame0_get_team_terr_call_entry_authority_digest(
    authority: &Frame0GetTeamTerrCallEntryAuthority,
) -> [u8; 32] {
    let mut image = b"don-2024-frame0-get-team-terr-call-entry-v1".to_vec();
    image.extend_from_slice(&authority.revision.to_le_bytes());
    image.push(authority.source as u8);
    image.extend_from_slice(&authority.native_trace_sha256);
    image.extend_from_slice(&authority.replay_file_sha256);
    image.extend_from_slice(&authority.executable_sha256);
    image.extend_from_slice(&authority.plan_strategy_entry_authority_digest);
    image.extend_from_slice(&authority.call_entry_sim_sha256);
    image.extend_from_slice(&authority.expected_request_sha256);
    image.extend_from_slice(&authority.local_prefix_digest);
    image.push(u8::from(authority.local_prefix_preserved_input_projection));
    image.extend_from_slice(&authority.frame.to_le_bytes());
    image.extend_from_slice(&[authority.step, authority.owner, authority.strategy_ordinal]);
    image.extend_from_slice(
        &authority
            .team_territory_preimage
            .other_team_terr
            .to_le_bytes(),
    );
    image.extend_from_slice(
        &authority
            .team_territory_preimage
            .min_other_team_terr
            .to_le_bytes(),
    );
    image.extend_from_slice(&authority.team_territory_preimage.my_team_terr.to_le_bytes());
    image.extend_from_slice(&authority.game.composition_digest);
    image.extend_from_slice(&authority.leader.composition_digest);
    sha256(&image)
}

/// Bind the complete native child input directly from the independently captured call-entry
/// Sim. Setup-time Leader rows and caller-supplied projections are intentionally not accepted.
pub fn bind_captured_frame0_get_team_terr_call_entry(
    parent: &Frame0PlanStrategyEntryAuthority,
    call_entry: &Sim,
    capture: Frame0GetTeamTerrCallEntryCapture,
) -> Result<Frame0GetTeamTerrCallEntryAuthority, Frame0GetTeamTerrCallEntryBindError> {
    let rebound = bind_golden_frame0_owner0_plan_strategy_entry(parent.capture.clone())?;
    if &rebound != parent
        || parent.composition_digest != frame0_plan_strategy_entry_authority_digest(&parent.capture)
    {
        return Err(Frame0GetTeamTerrCallEntryBindError::ParentAuthorityMismatch);
    }
    let local_prefix = plan_golden_frame0_owner0_plan_strategy_prefix(parent)?;
    if capture.revision == 0 {
        return Err(Frame0GetTeamTerrCallEntryBindError::MissingCaptureRevision);
    }
    if capture.source != Frame0GetTeamTerrCallEntrySource::CompleteRetailOwnerZeroPlanStrategyEntry
    {
        return Err(Frame0GetTeamTerrCallEntryBindError::WrongCaptureSource);
    }
    if capture.replay_file_sha256 != REPLAY_FILE_SHA256
        || capture.replay_file_sha256 != parent.capture.replay_file_sha256
    {
        return Err(Frame0GetTeamTerrCallEntryBindError::ReplayMismatch);
    }
    if capture.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256
        || capture.executable_sha256 != parent.capture.executable_sha256
    {
        return Err(Frame0GetTeamTerrCallEntryBindError::UnsupportedExecutable);
    }
    if capture.plan_strategy_entry_authority_digest != parent.composition_digest
        || capture.revision != parent.revision
        || capture.call_entry_sim_sha256 != parent.capture.call_entry_sim_sha256
        || capture.frame != parent.capture.frame
        || capture.step != parent.capture.step
        || capture.owner != parent.capture.owner
        || capture.strategy_ordinal != parent.capture.strategy_ordinal
        || capture.frame != GOLDEN_FRAME
        || capture.step != GOLDEN_STEP
        || capture.owner != GOLDEN_FIRST_STRATEGY_OWNER
        || capture.strategy_ordinal != GOLDEN_FIRST_STRATEGY_ORDINAL
    {
        return Err(Frame0GetTeamTerrCallEntryBindError::CaptureBoundaryMismatch);
    }
    if capture.native_trace_sha256 == [0; 32] {
        return Err(Frame0GetTeamTerrCallEntryBindError::MissingNativeTrace);
    }
    if capture.native_trace_sha256 != parent.capture.native_trace_sha256 {
        return Err(Frame0GetTeamTerrCallEntryBindError::NativeTraceMismatch);
    }
    let snapshot = save_sim(call_entry).map_err(Frame0GetTeamTerrCallEntryBindError::Snapshot)?;
    if sha256(&snapshot) != capture.call_entry_sim_sha256 {
        return Err(Frame0GetTeamTerrCallEntryBindError::CallEntrySnapshotMismatch);
    }
    if call_entry.world.frame != GOLDEN_FRAME {
        return Err(Frame0GetTeamTerrCallEntryBindError::WrongWorldFrame {
            expected: GOLDEN_FRAME,
            actual: call_entry.world.frame,
        });
    }
    if call_entry.vic_match.frame != GOLDEN_FRAME {
        return Err(Frame0GetTeamTerrCallEntryBindError::WrongGameFrame {
            expected: GOLDEN_FRAME,
            actual: call_entry.vic_match.frame,
        });
    }
    let players = call_entry
        .players
        .as_ref()
        .ok_or(Frame0GetTeamTerrCallEntryBindError::MissingPlayerTable)?;
    let mut game = Frame0GetTeamTerrGameProjection {
        revision: capture.revision,
        composition_digest: [0; 32],
        source: Frame0GetTeamTerrGameSource::SourceBackedGameInfoPlayerTeamProjection,
        frame: call_entry.vic_match.frame,
        team_style: call_entry.vic_match.options.team_style,
        players: std::array::from_fn(|slot| Frame0GetTeamTerrPlayerRow {
            flags: players.players[slot].flags,
            who: players.players[slot].who,
            team: players.players[slot].team,
        }),
    };
    game.composition_digest = frame0_get_team_terr_game_projection_digest(&game);
    let mut leader = Frame0GetTeamTerrLeaderProjection {
        revision: capture.revision,
        composition_digest: [0; 32],
        source: Frame0GetTeamTerrLeaderSource::CompleteRetailGetTeamTerrCallEntry,
        call_entry_sim_sha256: capture.call_entry_sim_sha256,
        leaders: std::array::from_fn(|slot| Frame0GetTeamTerrLeaderRow {
            leader_flags: call_entry.vic_leaders.slots[slot].leader_flags,
            who: call_entry.vic_leaders.slots[slot].who,
            territory: call_entry.vic_leaders.slots[slot].territory,
        }),
    };
    leader.composition_digest = frame0_get_team_terr_leader_projection_digest(&leader);
    let owner = usize::from(capture.owner);
    if leader.leaders[owner].leader_flags != parent.capture.leader.leader_flags
        || leader.leaders[owner].who != parent.capture.leader.who
    {
        return Err(Frame0GetTeamTerrCallEntryBindError::OwnerLeaderProjectionMismatch);
    }

    let mut authority = Frame0GetTeamTerrCallEntryAuthority {
        revision: capture.revision,
        composition_digest: [0; 32],
        source: capture.source,
        native_trace_sha256: capture.native_trace_sha256,
        replay_file_sha256: capture.replay_file_sha256,
        executable_sha256: capture.executable_sha256,
        plan_strategy_entry_authority_digest: capture.plan_strategy_entry_authority_digest,
        call_entry_sim_sha256: capture.call_entry_sim_sha256,
        expected_request_sha256: local_prefix.open.request_sha256,
        local_prefix_digest: local_prefix.local_prefix_digest,
        local_prefix_preserved_input_projection: true,
        frame: capture.frame,
        step: capture.step,
        owner: capture.owner,
        strategy_ordinal: capture.strategy_ordinal,
        team_territory_preimage: capture.team_territory_preimage,
        game,
        leader,
    };
    authority.composition_digest = frame0_get_team_terr_call_entry_authority_digest(&authority);
    Ok(authority)
}

fn get_player(
    leaders: &[Frame0GetTeamTerrLeaderRow; LEADER_SLOTS],
    players: &[Frame0GetTeamTerrPlayerRow; LEADER_SLOTS],
    leader_slot: usize,
    calls: &mut Vec<Frame0GetPlayerReceipt>,
) -> usize {
    let receiver_who = leaders[leader_slot].who;
    let mut result = 0usize;
    let mut visited = Vec::with_capacity(LEADER_SLOTS);
    let mut deferred = Vec::new();
    for (index, player) in players.iter().enumerate() {
        visited.push(index as u8);
        if player.flags & PLAYER_VALID == 0 || i32::from(player.who) != receiver_who {
            continue;
        }
        if player.flags & PLAYER_DEFERRED_MATCH_MASK == 0 {
            calls.push(Frame0GetPlayerReceipt {
                call_ordinal: calls.len(),
                receiver_leader_slot: leader_slot as u8,
                receiver_who,
                visited_player_rows: visited,
                deferred_special_matches: deferred,
                returned_early: true,
                result_player_row: index as u8,
            });
            return index;
        }
        result = index;
        deferred.push(index as u8);
    }
    calls.push(Frame0GetPlayerReceipt {
        call_ordinal: calls.len(),
        receiver_leader_slot: leader_slot as u8,
        receiver_who,
        visited_player_rows: visited,
        deferred_special_matches: deferred,
        returned_early: false,
        result_player_row: result as u8,
    });
    result
}

struct Frame0GetTeamTerrCore {
    get_player_calls: Vec<Frame0GetPlayerReceipt>,
    visits: Vec<Frame0GetTeamTerrVisit>,
    result: i32,
}

fn compute_frame0_get_team_terr(
    receiver: usize,
    authority: &Frame0GetTeamTerrCallEntryAuthority,
) -> Frame0GetTeamTerrCore {
    let mut sum = 0i32;
    let mut get_player_calls = Vec::new();
    let mut visits = Vec::with_capacity(LEADER_SLOTS);
    for slot in 0..LEADER_SLOTS {
        let leader = authority.leader.leaders[slot];
        let sum_before = sum;
        let disposition;
        if slot == receiver {
            sum = sum.wrapping_add(leader.territory);
            disposition = Frame0GetTeamTerrDisposition::ReceiverSelf;
        } else if leader.leader_flags & LEADER_VALID == 0 {
            disposition = Frame0GetTeamTerrDisposition::InvalidLeader;
        } else {
            if authority.game.team_style == TEAM_STYLE_SPECIAL {
                let receiver_player = get_player(
                    &authority.leader.leaders,
                    &authority.game.players,
                    receiver,
                    &mut get_player_calls,
                );
                let row = authority.game.players[receiver_player];
                if row.flags & PLAYER_VALID != 0 && row.team == TEAM_OBSERVER {
                    visits.push(Frame0GetTeamTerrVisit {
                        leader_slot: slot as u8,
                        leader_who: leader.who,
                        territory: leader.territory,
                        sum_before,
                        sum_after: sum,
                        disposition: Frame0GetTeamTerrDisposition::ReceiverPlayerObserver,
                    });
                    continue;
                }
                let candidate_player = get_player(
                    &authority.leader.leaders,
                    &authority.game.players,
                    slot,
                    &mut get_player_calls,
                );
                let row = authority.game.players[candidate_player];
                if row.flags & PLAYER_VALID != 0 && row.team == TEAM_OBSERVER {
                    visits.push(Frame0GetTeamTerrVisit {
                        leader_slot: slot as u8,
                        leader_who: leader.who,
                        territory: leader.territory,
                        sum_before,
                        sum_after: sum,
                        disposition: Frame0GetTeamTerrDisposition::CandidatePlayerObserver,
                    });
                    continue;
                }
            }

            let receiver_player = get_player(
                &authority.leader.leaders,
                &authority.game.players,
                receiver,
                &mut get_player_calls,
            );
            let receiver_team = authority.game.players[receiver_player].team;
            if !(FIRST_NORMAL_TEAM..FIRST_NORMAL_TEAM + NORMAL_TEAM_COUNT).contains(&receiver_team)
            {
                disposition = Frame0GetTeamTerrDisposition::ReceiverTeamOutsideNormalRange {
                    team: receiver_team,
                };
            } else {
                let candidate_player = get_player(
                    &authority.leader.leaders,
                    &authority.game.players,
                    slot,
                    &mut get_player_calls,
                );
                let candidate_team = authority.game.players[candidate_player].team;
                if candidate_team == receiver_team {
                    sum = sum.wrapping_add(leader.territory);
                    disposition = Frame0GetTeamTerrDisposition::SameFrameZeroTeam {
                        team: receiver_team,
                    };
                } else {
                    disposition = Frame0GetTeamTerrDisposition::DifferentFrameZeroTeam {
                        receiver_team,
                        candidate_team,
                    };
                }
            }
        }
        visits.push(Frame0GetTeamTerrVisit {
            leader_slot: slot as u8,
            leader_who: leader.who,
            territory: leader.territory,
            sum_before,
            sum_after: sum,
            disposition,
        });
    }
    Frame0GetTeamTerrCore {
        get_player_calls,
        visits,
        result: sum,
    }
}

pub fn frame0_get_team_terr_receipt_digest(receipt: &Frame0GetTeamTerrReceipt) -> [u8; 32] {
    let mut image = b"don-2024-frame0-get-team-terr-receipt-v1".to_vec();
    image.extend_from_slice(&receipt.revision.to_le_bytes());
    image.extend_from_slice(&receipt.replay_file_sha256);
    image.extend_from_slice(&receipt.executable_sha256);
    image.extend_from_slice(&receipt.request.request_sha256);
    image.extend_from_slice(&receipt.native_trace_sha256);
    image.extend_from_slice(&receipt.call_entry_authority_digest);
    image.extend_from_slice(&receipt.game_projection_digest);
    image.extend_from_slice(&receipt.leader_projection_digest);
    image.push(u8::from(receipt.local_prefix_preserved_input_projection));
    image.push(receipt.receiver_leader_slot);
    image.extend_from_slice(&(receipt.get_player_calls.len() as u64).to_le_bytes());
    for call in &receipt.get_player_calls {
        image.extend_from_slice(&(call.call_ordinal as u64).to_le_bytes());
        image.push(call.receiver_leader_slot);
        image.extend_from_slice(&call.receiver_who.to_le_bytes());
        image.extend_from_slice(&(call.visited_player_rows.len() as u64).to_le_bytes());
        image.extend_from_slice(&call.visited_player_rows);
        image.extend_from_slice(&(call.deferred_special_matches.len() as u64).to_le_bytes());
        image.extend_from_slice(&call.deferred_special_matches);
        image.push(call.returned_early as u8);
        image.push(call.result_player_row);
    }
    image.extend_from_slice(&(receipt.visits.len() as u64).to_le_bytes());
    for visit in &receipt.visits {
        image.push(visit.leader_slot);
        image.extend_from_slice(&visit.leader_who.to_le_bytes());
        image.extend_from_slice(&visit.territory.to_le_bytes());
        image.extend_from_slice(&visit.sum_before.to_le_bytes());
        image.extend_from_slice(&visit.sum_after.to_le_bytes());
        match visit.disposition {
            Frame0GetTeamTerrDisposition::ReceiverSelf => image.push(0),
            Frame0GetTeamTerrDisposition::InvalidLeader => image.push(1),
            Frame0GetTeamTerrDisposition::ReceiverPlayerObserver => image.push(2),
            Frame0GetTeamTerrDisposition::CandidatePlayerObserver => image.push(3),
            Frame0GetTeamTerrDisposition::ReceiverTeamOutsideNormalRange { team } => {
                image.extend_from_slice(&[4, team as u8]);
            }
            Frame0GetTeamTerrDisposition::DifferentFrameZeroTeam {
                receiver_team,
                candidate_team,
            } => image.extend_from_slice(&[5, receiver_team as u8, candidate_team as u8]),
            Frame0GetTeamTerrDisposition::SameFrameZeroTeam { team } => {
                image.extend_from_slice(&[6, team as u8]);
            }
        }
    }
    image.extend_from_slice(&receipt.result.to_le_bytes());
    image.extend_from_slice(&receipt.return_va.to_le_bytes());
    image.push(receipt.is_ally_reached as u8);
    sha256(&image)
}

/// Resolve the exact frame-zero child without publishing any parent `plan_strategy` writes.
pub fn resolve_captured_frame0_get_team_terr(
    request: &Frame0GetTeamTerrRequest,
    authority: &Frame0GetTeamTerrCallEntryAuthority,
) -> Result<Frame0GetTeamTerrReceipt, Frame0GetTeamTerrError> {
    if !validate_frame0_get_team_terr_request(request)
        || request.callsite_va != GET_TEAM_TERR_CALL_VA
        || request.callee_va != GET_TEAM_TERR_VA
        || request.input_surface
            != Frame0GetTeamTerrInputSurface::CompleteLeaderGameTeamAndPlayerProjection
    {
        return Err(Frame0GetTeamTerrError::InvalidParentRequest);
    }
    if authority.revision == 0
        || authority.composition_digest == [0; 32]
        || authority.source
            != Frame0GetTeamTerrCallEntrySource::CompleteRetailOwnerZeroPlanStrategyEntry
        || authority.native_trace_sha256 == [0; 32]
        || authority.replay_file_sha256 != REPLAY_FILE_SHA256
        || authority.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256
        || authority.frame != GOLDEN_FRAME
        || authority.step != GOLDEN_STEP
        || authority.owner != GOLDEN_FIRST_STRATEGY_OWNER
        || authority.strategy_ordinal != GOLDEN_FIRST_STRATEGY_ORDINAL
        || authority.game.revision != authority.revision
        || authority.leader.revision != authority.revision
        || authority.game.source
            != Frame0GetTeamTerrGameSource::SourceBackedGameInfoPlayerTeamProjection
        || authority.leader.source
            != Frame0GetTeamTerrLeaderSource::CompleteRetailGetTeamTerrCallEntry
        || authority.game.frame != GOLDEN_FRAME
        || authority.leader.call_entry_sim_sha256 != authority.call_entry_sim_sha256
        || authority.expected_request_sha256 == [0; 32]
        || authority.local_prefix_digest == [0; 32]
        || !authority.local_prefix_preserved_input_projection
        || authority.composition_digest
            != frame0_get_team_terr_call_entry_authority_digest(authority)
    {
        return Err(Frame0GetTeamTerrError::InvalidCallEntryAuthority);
    }
    if request.parent_authority_digest != authority.plan_strategy_entry_authority_digest {
        return Err(Frame0GetTeamTerrError::ParentAuthorityMismatch);
    }
    if request.request_sha256 != authority.expected_request_sha256
        || request.local_prefix_digest != authority.local_prefix_digest
    {
        return Err(Frame0GetTeamTerrError::InvalidParentRequest);
    }
    if request.call_entry_sim_sha256 != authority.call_entry_sim_sha256 {
        return Err(Frame0GetTeamTerrError::CallEntrySnapshotMismatch);
    }
    if authority.game.composition_digest
        != frame0_get_team_terr_game_projection_digest(&authority.game)
    {
        return Err(Frame0GetTeamTerrError::InvalidGameProjectionDigest);
    }
    if authority.leader.composition_digest
        != frame0_get_team_terr_leader_projection_digest(&authority.leader)
    {
        return Err(Frame0GetTeamTerrError::InvalidLeaderProjectionDigest);
    }
    let receiver = usize::from(request.receiver_owner);
    if receiver >= LEADER_SLOTS {
        return Err(Frame0GetTeamTerrError::ReceiverOutsideLeaderTable);
    }
    if authority.leader.leaders[receiver].who != receiver as i32 {
        return Err(Frame0GetTeamTerrError::ReceiverWhoMismatch);
    }

    let mut sum = 0i32;
    let mut get_player_calls = Vec::new();
    let mut visits = Vec::with_capacity(LEADER_SLOTS);
    for slot in 0..LEADER_SLOTS {
        let leader = authority.leader.leaders[slot];
        let sum_before = sum;
        let disposition;
        if slot == receiver {
            sum = sum.wrapping_add(leader.territory);
            disposition = Frame0GetTeamTerrDisposition::ReceiverSelf;
        } else if leader.leader_flags & LEADER_VALID == 0 {
            disposition = Frame0GetTeamTerrDisposition::InvalidLeader;
        } else {
            if authority.game.team_style == TEAM_STYLE_SPECIAL {
                let receiver_player = get_player(
                    &authority.leader.leaders,
                    &authority.game.players,
                    receiver,
                    &mut get_player_calls,
                );
                let row = authority.game.players[receiver_player];
                if row.flags & PLAYER_VALID != 0 && row.team == TEAM_OBSERVER {
                    visits.push(Frame0GetTeamTerrVisit {
                        leader_slot: slot as u8,
                        leader_who: leader.who,
                        territory: leader.territory,
                        sum_before,
                        sum_after: sum,
                        disposition: Frame0GetTeamTerrDisposition::ReceiverPlayerObserver,
                    });
                    continue;
                }
                let candidate_player = get_player(
                    &authority.leader.leaders,
                    &authority.game.players,
                    slot,
                    &mut get_player_calls,
                );
                let row = authority.game.players[candidate_player];
                if row.flags & PLAYER_VALID != 0 && row.team == TEAM_OBSERVER {
                    visits.push(Frame0GetTeamTerrVisit {
                        leader_slot: slot as u8,
                        leader_who: leader.who,
                        territory: leader.territory,
                        sum_before,
                        sum_after: sum,
                        disposition: Frame0GetTeamTerrDisposition::CandidatePlayerObserver,
                    });
                    continue;
                }
            }

            let receiver_player = get_player(
                &authority.leader.leaders,
                &authority.game.players,
                receiver,
                &mut get_player_calls,
            );
            let receiver_team = authority.game.players[receiver_player].team;
            if !(FIRST_NORMAL_TEAM..FIRST_NORMAL_TEAM + NORMAL_TEAM_COUNT).contains(&receiver_team)
            {
                disposition = Frame0GetTeamTerrDisposition::ReceiverTeamOutsideNormalRange {
                    team: receiver_team,
                };
            } else {
                let candidate_player = get_player(
                    &authority.leader.leaders,
                    &authority.game.players,
                    slot,
                    &mut get_player_calls,
                );
                let candidate_team = authority.game.players[candidate_player].team;
                if candidate_team == receiver_team {
                    sum = sum.wrapping_add(leader.territory);
                    disposition = Frame0GetTeamTerrDisposition::SameFrameZeroTeam {
                        team: receiver_team,
                    };
                } else {
                    disposition = Frame0GetTeamTerrDisposition::DifferentFrameZeroTeam {
                        receiver_team,
                        candidate_team,
                    };
                }
            }
        }
        visits.push(Frame0GetTeamTerrVisit {
            leader_slot: slot as u8,
            leader_who: leader.who,
            territory: leader.territory,
            sum_before,
            sum_after: sum,
            disposition,
        });
    }

    let mut receipt = Frame0GetTeamTerrReceipt {
        revision: authority.revision,
        composition_digest: [0; 32],
        replay_file_sha256: authority.replay_file_sha256,
        executable_sha256: authority.executable_sha256,
        request: request.clone(),
        native_trace_sha256: authority.native_trace_sha256,
        call_entry_authority_digest: authority.composition_digest,
        game_projection_digest: authority.game.composition_digest,
        leader_projection_digest: authority.leader.composition_digest,
        local_prefix_preserved_input_projection: authority.local_prefix_preserved_input_projection,
        receiver_leader_slot: request.receiver_owner,
        get_player_calls,
        visits,
        result: sum,
        return_va: GET_TEAM_TERR_CALL_VA + 5,
        is_ally_reached: false,
    };
    receipt.composition_digest = frame0_get_team_terr_receipt_digest(&receipt);
    Ok(receipt)
}

fn append_team_territory_preimage(
    image: &mut Vec<u8>,
    values: Frame0PlanStrategyTeamTerritoryPreimage,
) {
    image.extend_from_slice(&values.other_team_terr.to_le_bytes());
    image.extend_from_slice(&values.min_other_team_terr.to_le_bytes());
    image.extend_from_slice(&values.my_team_terr.to_le_bytes());
}

fn append_team_territory_write(image: &mut Vec<u8>, write: Frame0PlanStrategyTeamTerritoryWrite) {
    image.extend_from_slice(&write.instruction_va.to_le_bytes());
    image.push(write.field as u8);
    image.extend_from_slice(&write.before.to_le_bytes());
    image.extend_from_slice(&write.after.to_le_bytes());
}

fn post_team_territory_writes_digest(
    authority: &Frame0GetTeamTerrCallEntryAuthority,
    prefix: &Frame0PlanStrategyPrefixPlan,
    first_child: &Frame0GetTeamTerrReceipt,
    before: Frame0PlanStrategyTeamTerritoryPreimage,
    after: Frame0PlanStrategyTeamTerritoryPreimage,
    writes: &[Frame0PlanStrategyTeamTerritoryWrite; 3],
) -> [u8; 32] {
    let mut image = b"don-2024-frame0-plan-strategy-post-team-terr-writes-v1".to_vec();
    image.extend_from_slice(&authority.composition_digest);
    image.extend_from_slice(&prefix.local_prefix_digest);
    image.extend_from_slice(&first_child.composition_digest);
    append_team_territory_preimage(&mut image, before);
    append_team_territory_preimage(&mut image, after);
    for write in writes {
        append_team_territory_write(&mut image, *write);
    }
    sha256(&image)
}

fn diplomacy_read_request_digest(request: &Frame0PlanStrategyDiplomacyReadRequest) -> [u8; 32] {
    let mut image = b"don-2024-frame0-plan-strategy-diplo-read-request-v1".to_vec();
    image.extend_from_slice(&request.call_entry_authority_digest);
    image.extend_from_slice(&request.first_get_team_terr_receipt_digest);
    image.extend_from_slice(&request.staged_writes_digest);
    image.extend_from_slice(&request.call_entry_sim_sha256);
    image.extend_from_slice(&[
        request.receiver_leader_slot,
        request.candidate_leader_slot,
        request.direction as u8,
    ]);
    image.extend_from_slice(&request.receiver_who.to_le_bytes());
    image.extend_from_slice(&request.candidate_who.to_le_bytes());
    image.extend_from_slice(&request.target_who_index.to_le_bytes());
    image.extend_from_slice(&request.read_va.to_le_bytes());
    image.extend_from_slice(&request.ally_value.to_le_bytes());
    image.extend_from_slice(&request.reverse_read_va_if_ally.to_le_bytes());
    image.extend_from_slice(&request.get_team_terr_call_va_if_not_ally.to_le_bytes());
    sha256(&image)
}

pub fn validate_frame0_plan_strategy_diplomacy_read_request(
    request: &Frame0PlanStrategyDiplomacyReadRequest,
) -> bool {
    request.request_sha256 != [0; 32]
        && request.call_entry_authority_digest != [0; 32]
        && request.first_get_team_terr_receipt_digest != [0; 32]
        && request.staged_writes_digest != [0; 32]
        && request.call_entry_sim_sha256 != [0; 32]
        && request.receiver_leader_slot == GOLDEN_FIRST_STRATEGY_OWNER
        && request.receiver_who == i32::from(GOLDEN_FIRST_STRATEGY_OWNER)
        && usize::from(request.candidate_leader_slot) < LEADER_SLOTS
        && request.candidate_leader_slot != request.receiver_leader_slot
        && request.direction == Frame0PlanStrategyDiplomacyReadDirection::CandidateTowardReceiver
        && request.target_who_index == request.receiver_who
        && request.read_va == FIRST_DIRECTIONAL_DIPLO_READ_VA
        && request.ally_value == DIPLO_ALLY
        && request.reverse_read_va_if_ally == REVERSE_DIRECTIONAL_DIPLO_READ_VA
        && request.get_team_terr_call_va_if_not_ally == OPPONENT_GET_TEAM_TERR_CALL_VA
        && request.request_sha256 == diplomacy_read_request_digest(request)
}

pub fn frame0_plan_strategy_post_team_terr_digest(
    plan: &Frame0PlanStrategyPostTeamTerrPlan,
) -> [u8; 32] {
    let mut image = b"don-2024-frame0-plan-strategy-post-team-terr-plan-v1".to_vec();
    image.extend_from_slice(&plan.revision.to_le_bytes());
    image.extend_from_slice(&plan.call_entry_authority_digest);
    image.extend_from_slice(&plan.prior_local_prefix.local_prefix_digest);
    image.extend_from_slice(&plan.first_get_team_terr.composition_digest);
    append_team_territory_preimage(&mut image, plan.before);
    append_team_territory_preimage(&mut image, plan.after_local);
    for write in plan.writes {
        append_team_territory_write(&mut image, write);
    }
    image.push(plan.receiver_self_slot_skipped);
    image.extend_from_slice(&plan.open.request_sha256);
    image.push(u8::from(plan.owner0_strategy_complete));
    sha256(&image)
}

/// Continue owner zero from the exact return at `0x006B98D5` through the three Leader scalar
/// stores and the receiver-self loop row. The next native input is the first directional
/// diplomacy read for the first active nonself Leader. It remains typed and unexecuted.
pub fn plan_frame0_owner0_after_get_team_terr(
    parent: &Frame0PlanStrategyEntryAuthority,
    authority: &Frame0GetTeamTerrCallEntryAuthority,
    first_child: &Frame0GetTeamTerrReceipt,
) -> Result<Frame0PlanStrategyPostTeamTerrPlan, Frame0PlanStrategyPostTeamTerrError> {
    let prefix = plan_golden_frame0_owner0_plan_strategy_prefix(parent)
        .map_err(|_| Frame0PlanStrategyPostTeamTerrError::InvalidParentPrefix)?;
    if prefix.authority_digest != authority.plan_strategy_entry_authority_digest
        || prefix.open.request_sha256 != authority.expected_request_sha256
        || prefix.local_prefix_digest != authority.local_prefix_digest
        || prefix.call_entry_sim_sha256 != authority.call_entry_sim_sha256
    {
        return Err(Frame0PlanStrategyPostTeamTerrError::InvalidCallEntryAuthority);
    }
    let expected = resolve_captured_frame0_get_team_terr(&prefix.open, authority)
        .map_err(|_| Frame0PlanStrategyPostTeamTerrError::InvalidCallEntryAuthority)?;
    if &expected != first_child
        || first_child.composition_digest != frame0_get_team_terr_receipt_digest(first_child)
        || first_child.return_va != GET_TEAM_TERR_CALL_VA + 5
        || first_child.receiver_leader_slot != GOLDEN_FIRST_STRATEGY_OWNER
    {
        return Err(Frame0PlanStrategyPostTeamTerrError::InvalidFirstChildReceipt);
    }

    let before = authority.team_territory_preimage;
    let after_local = Frame0PlanStrategyTeamTerritoryPreimage {
        other_team_terr: 0,
        min_other_team_terr: 0,
        my_team_terr: first_child.result,
    };
    let writes = [
        Frame0PlanStrategyTeamTerritoryWrite {
            instruction_va: STORE_MY_TEAM_TERR_VA,
            field: Frame0PlanStrategyTeamTerritoryField::MyTeamTerr,
            before: before.my_team_terr,
            after: after_local.my_team_terr,
        },
        Frame0PlanStrategyTeamTerritoryWrite {
            instruction_va: STORE_OTHER_TEAM_TERR_VA,
            field: Frame0PlanStrategyTeamTerritoryField::OtherTeamTerr,
            before: before.other_team_terr,
            after: after_local.other_team_terr,
        },
        Frame0PlanStrategyTeamTerritoryWrite {
            instruction_va: STORE_MIN_OTHER_TEAM_TERR_VA,
            field: Frame0PlanStrategyTeamTerritoryField::MinOtherTeamTerr,
            before: before.min_other_team_terr,
            after: after_local.min_other_team_terr,
        },
    ];
    let staged_writes_digest = post_team_territory_writes_digest(
        authority,
        &prefix,
        first_child,
        before,
        after_local,
        &writes,
    );

    let receiver_slot = usize::from(first_child.receiver_leader_slot);
    let receiver = authority.leader.leaders[receiver_slot];
    let (candidate_slot, candidate) = authority
        .leader
        .leaders
        .iter()
        .copied()
        .enumerate()
        .find(|(slot, candidate)| {
            *slot as i32 != receiver.who
                && candidate.leader_flags & LEADER_ACTIVE != 0
                && receiver.who != candidate.who
        })
        .ok_or(Frame0PlanStrategyPostTeamTerrError::NoQualifyingDiplomacyCandidate)?;
    let mut open = Frame0PlanStrategyDiplomacyReadRequest {
        request_sha256: [0; 32],
        call_entry_authority_digest: authority.composition_digest,
        first_get_team_terr_receipt_digest: first_child.composition_digest,
        staged_writes_digest,
        call_entry_sim_sha256: authority.call_entry_sim_sha256,
        receiver_leader_slot: first_child.receiver_leader_slot,
        receiver_who: receiver.who,
        candidate_leader_slot: candidate_slot as u8,
        candidate_who: candidate.who,
        direction: Frame0PlanStrategyDiplomacyReadDirection::CandidateTowardReceiver,
        target_who_index: receiver.who,
        read_va: FIRST_DIRECTIONAL_DIPLO_READ_VA,
        ally_value: DIPLO_ALLY,
        reverse_read_va_if_ally: REVERSE_DIRECTIONAL_DIPLO_READ_VA,
        get_team_terr_call_va_if_not_ally: OPPONENT_GET_TEAM_TERR_CALL_VA,
    };
    open.request_sha256 = diplomacy_read_request_digest(&open);

    let mut plan = Frame0PlanStrategyPostTeamTerrPlan {
        revision: authority.revision,
        composition_digest: [0; 32],
        call_entry_authority_digest: authority.composition_digest,
        prior_local_prefix: prefix,
        first_get_team_terr: first_child.clone(),
        before,
        after_local,
        writes,
        receiver_self_slot_skipped: first_child.receiver_leader_slot,
        open,
        owner0_strategy_complete: false,
    };
    plan.composition_digest = frame0_plan_strategy_post_team_terr_digest(&plan);
    Ok(plan)
}

pub fn frame0_plan_strategy_diplomacy_read_authority_digest(
    authority: &Frame0PlanStrategyDiplomacyReadAuthority,
) -> [u8; 32] {
    let mut image = b"don-2024-frame0-plan-strategy-diplo-read-authority-v1".to_vec();
    image.extend_from_slice(&authority.revision.to_le_bytes());
    image.push(authority.source as u8);
    image.extend_from_slice(&authority.replay_file_sha256);
    image.extend_from_slice(&authority.executable_sha256);
    image.extend_from_slice(&authority.call_entry_authority_digest);
    image.extend_from_slice(&authority.staged_plan_digest);
    image.extend_from_slice(&authority.request_sha256);
    image.extend_from_slice(&authority.call_entry_sim_sha256);
    image.push(authority.candidate_leader_slot);
    image.extend_from_slice(&authority.candidate_who.to_le_bytes());
    image.extend_from_slice(&authority.target_who_index.to_le_bytes());
    image.extend_from_slice(&authority.read_va.to_le_bytes());
    image.extend_from_slice(&authority.value.to_le_bytes());
    sha256(&image)
}

/// Project exactly one directional diplomacy cell from the canonical call-entry Sim. Replanning
/// the complete detached prefix proves the request is reached before the read is admitted.
pub fn bind_frame0_plan_strategy_first_diplomacy_read(
    parent: &Frame0PlanStrategyEntryAuthority,
    call_entry_authority: &Frame0GetTeamTerrCallEntryAuthority,
    first_child: &Frame0GetTeamTerrReceipt,
    staged: &Frame0PlanStrategyPostTeamTerrPlan,
    call_entry: &Sim,
) -> Result<Frame0PlanStrategyDiplomacyReadAuthority, Frame0PlanStrategyDiplomacyReadBindError> {
    let expected =
        plan_frame0_owner0_after_get_team_terr(parent, call_entry_authority, first_child)
            .map_err(|_| Frame0PlanStrategyDiplomacyReadBindError::InvalidStagedPlan)?;
    if &expected != staged
        || staged.composition_digest != frame0_plan_strategy_post_team_terr_digest(staged)
        || !validate_frame0_plan_strategy_diplomacy_read_request(&staged.open)
    {
        return Err(Frame0PlanStrategyDiplomacyReadBindError::InvalidStagedPlan);
    }
    if staged.call_entry_authority_digest != call_entry_authority.composition_digest
        || staged.open.call_entry_authority_digest != call_entry_authority.composition_digest
        || staged.open.call_entry_sim_sha256 != call_entry_authority.call_entry_sim_sha256
    {
        return Err(Frame0PlanStrategyDiplomacyReadBindError::InvalidCallEntryAuthority);
    }
    let snapshot =
        save_sim(call_entry).map_err(Frame0PlanStrategyDiplomacyReadBindError::Snapshot)?;
    if sha256(&snapshot) != call_entry_authority.call_entry_sim_sha256 {
        return Err(Frame0PlanStrategyDiplomacyReadBindError::CallEntrySnapshotMismatch);
    }

    let candidate_slot = usize::from(staged.open.candidate_leader_slot);
    let target_index = usize::try_from(staged.open.target_who_index)
        .ok()
        .filter(|index| *index < LEADER_SLOTS)
        .ok_or(Frame0PlanStrategyDiplomacyReadBindError::TargetIndexOutsideLeaderTable)?;
    let live_candidate = &call_entry.vic_leaders.slots[candidate_slot];
    let projected_candidate = call_entry_authority.leader.leaders[candidate_slot];
    if live_candidate.leader_flags != projected_candidate.leader_flags
        || live_candidate.who != projected_candidate.who
        || live_candidate.territory != projected_candidate.territory
        || live_candidate.who != staged.open.candidate_who
        || staged.open.target_who_index != staged.open.receiver_who
        || staged.open.read_va != FIRST_DIRECTIONAL_DIPLO_READ_VA
    {
        return Err(Frame0PlanStrategyDiplomacyReadBindError::ActorProjectionMismatch);
    }

    let mut authority = Frame0PlanStrategyDiplomacyReadAuthority {
        revision: call_entry_authority.revision,
        composition_digest: [0; 32],
        source: Frame0PlanStrategyDiplomacyReadSource::CompleteRetailPlanStrategyCallEntry,
        replay_file_sha256: call_entry_authority.replay_file_sha256,
        executable_sha256: call_entry_authority.executable_sha256,
        call_entry_authority_digest: call_entry_authority.composition_digest,
        staged_plan_digest: staged.composition_digest,
        request_sha256: staged.open.request_sha256,
        call_entry_sim_sha256: call_entry_authority.call_entry_sim_sha256,
        candidate_leader_slot: staged.open.candidate_leader_slot,
        candidate_who: staged.open.candidate_who,
        target_who_index: staged.open.target_who_index,
        read_va: staged.open.read_va,
        value: live_candidate.diplos[target_index],
    };
    authority.composition_digest = frame0_plan_strategy_diplomacy_read_authority_digest(&authority);
    Ok(authority)
}

fn reverse_diplomacy_read_request_digest(
    request: &Frame0PlanStrategyReverseDiplomacyReadRequest,
) -> [u8; 32] {
    let mut image = b"don-2024-frame0-plan-strategy-reverse-diplo-read-request-v1".to_vec();
    image.extend_from_slice(&request.first_read_authority_digest);
    image.extend_from_slice(&request.staged_plan_digest);
    image.extend_from_slice(&request.call_entry_sim_sha256);
    image.extend_from_slice(&[request.receiver_leader_slot, request.candidate_leader_slot]);
    image.extend_from_slice(&request.receiver_who.to_le_bytes());
    image.extend_from_slice(&request.candidate_who.to_le_bytes());
    image.extend_from_slice(&request.target_who_index.to_le_bytes());
    image.extend_from_slice(&request.read_va.to_le_bytes());
    image.extend_from_slice(&request.ally_value.to_le_bytes());
    image.extend_from_slice(&request.get_team_terr_call_va_if_not_ally.to_le_bytes());
    sha256(&image)
}

pub fn validate_frame0_plan_strategy_reverse_diplomacy_read_request(
    request: &Frame0PlanStrategyReverseDiplomacyReadRequest,
) -> bool {
    request.request_sha256 != [0; 32]
        && request.first_read_authority_digest != [0; 32]
        && request.staged_plan_digest != [0; 32]
        && request.call_entry_sim_sha256 != [0; 32]
        && request.receiver_leader_slot == GOLDEN_FIRST_STRATEGY_OWNER
        && request.receiver_who == i32::from(GOLDEN_FIRST_STRATEGY_OWNER)
        && usize::from(request.candidate_leader_slot) < LEADER_SLOTS
        && request.candidate_leader_slot != request.receiver_leader_slot
        && request.target_who_index == request.candidate_who
        && request.read_va == REVERSE_DIRECTIONAL_DIPLO_READ_VA
        && request.ally_value == DIPLO_ALLY
        && request.get_team_terr_call_va_if_not_ally == OPPONENT_GET_TEAM_TERR_CALL_VA
        && request.request_sha256 == reverse_diplomacy_read_request_digest(request)
}

fn opponent_get_team_terr_request_digest(
    request: &Frame0PlanStrategyOpponentGetTeamTerrRequest,
) -> [u8; 32] {
    let mut image = b"don-2024-frame0-plan-strategy-opponent-get-team-terr-request-v1".to_vec();
    image.extend_from_slice(&request.first_read_authority_digest);
    image.extend_from_slice(&request.staged_plan_digest);
    image.extend_from_slice(&request.call_entry_authority_digest);
    image.extend_from_slice(&request.call_entry_sim_sha256);
    image.push(request.receiver_leader_slot);
    image.extend_from_slice(&request.receiver_who.to_le_bytes());
    image.extend_from_slice(&request.callsite_va.to_le_bytes());
    image.extend_from_slice(&request.callee_va.to_le_bytes());
    image.push(request.input_surface as u8);
    sha256(&image)
}

pub fn validate_frame0_plan_strategy_opponent_get_team_terr_request(
    request: &Frame0PlanStrategyOpponentGetTeamTerrRequest,
) -> bool {
    request.request_sha256 != [0; 32]
        && request.first_read_authority_digest != [0; 32]
        && request.staged_plan_digest != [0; 32]
        && request.call_entry_authority_digest != [0; 32]
        && request.call_entry_sim_sha256 != [0; 32]
        && usize::from(request.receiver_leader_slot) < LEADER_SLOTS
        && request.receiver_leader_slot != GOLDEN_FIRST_STRATEGY_OWNER
        && request.callsite_va == OPPONENT_GET_TEAM_TERR_CALL_VA
        && request.callee_va == GET_TEAM_TERR_VA
        && request.input_surface
            == Frame0GetTeamTerrInputSurface::CompleteLeaderGameTeamAndPlayerProjection
        && request.request_sha256 == opponent_get_team_terr_request_digest(request)
}

pub fn frame0_plan_strategy_diplomacy_step_digest(
    plan: &Frame0PlanStrategyDiplomacyStepPlan,
) -> [u8; 32] {
    let mut image = b"don-2024-frame0-plan-strategy-diplo-step-plan-v1".to_vec();
    image.extend_from_slice(&plan.revision.to_le_bytes());
    image.extend_from_slice(&plan.staged.composition_digest);
    image.extend_from_slice(&plan.first_read.composition_digest);
    image.push(u8::from(plan.first_relation_is_ally));
    match &plan.open {
        Frame0PlanStrategyDiplomacyStepOpen::ReverseRead(request) => {
            image.push(0);
            image.extend_from_slice(&request.request_sha256);
        }
        Frame0PlanStrategyDiplomacyStepOpen::OpponentGetTeamTerr(request) => {
            image.push(1);
            image.extend_from_slice(&request.request_sha256);
        }
    }
    image.push(u8::from(plan.owner0_strategy_complete));
    sha256(&image)
}

/// Execute only the first directional comparison. No staged Leader value is published and no
/// second relation or repeated child result is guessed.
pub fn advance_frame0_plan_strategy_first_diplomacy_read(
    staged: &Frame0PlanStrategyPostTeamTerrPlan,
    first_read: &Frame0PlanStrategyDiplomacyReadAuthority,
) -> Result<Frame0PlanStrategyDiplomacyStepPlan, Frame0PlanStrategyDiplomacyStepError> {
    if staged.composition_digest != frame0_plan_strategy_post_team_terr_digest(staged)
        || !validate_frame0_plan_strategy_diplomacy_read_request(&staged.open)
    {
        return Err(Frame0PlanStrategyDiplomacyStepError::InvalidStagedPlan);
    }
    if first_read.revision != staged.revision
        || first_read.composition_digest == [0; 32]
        || first_read.source
            != Frame0PlanStrategyDiplomacyReadSource::CompleteRetailPlanStrategyCallEntry
        || first_read.replay_file_sha256 != REPLAY_FILE_SHA256
        || first_read.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256
        || first_read.call_entry_authority_digest != staged.call_entry_authority_digest
        || first_read.staged_plan_digest != staged.composition_digest
        || first_read.request_sha256 != staged.open.request_sha256
        || first_read.call_entry_sim_sha256 != staged.open.call_entry_sim_sha256
        || first_read.candidate_leader_slot != staged.open.candidate_leader_slot
        || first_read.candidate_who != staged.open.candidate_who
        || first_read.target_who_index != staged.open.target_who_index
        || first_read.read_va != staged.open.read_va
        || first_read.composition_digest
            != frame0_plan_strategy_diplomacy_read_authority_digest(first_read)
    {
        return Err(Frame0PlanStrategyDiplomacyStepError::InvalidReadAuthority);
    }

    let first_relation_is_ally = first_read.value == DIPLO_ALLY;
    let open = if first_relation_is_ally {
        let mut request = Frame0PlanStrategyReverseDiplomacyReadRequest {
            request_sha256: [0; 32],
            first_read_authority_digest: first_read.composition_digest,
            staged_plan_digest: staged.composition_digest,
            call_entry_sim_sha256: first_read.call_entry_sim_sha256,
            receiver_leader_slot: staged.open.receiver_leader_slot,
            receiver_who: staged.open.receiver_who,
            candidate_leader_slot: first_read.candidate_leader_slot,
            candidate_who: first_read.candidate_who,
            target_who_index: first_read.candidate_who,
            read_va: REVERSE_DIRECTIONAL_DIPLO_READ_VA,
            ally_value: DIPLO_ALLY,
            get_team_terr_call_va_if_not_ally: OPPONENT_GET_TEAM_TERR_CALL_VA,
        };
        request.request_sha256 = reverse_diplomacy_read_request_digest(&request);
        Frame0PlanStrategyDiplomacyStepOpen::ReverseRead(request)
    } else {
        let mut request = Frame0PlanStrategyOpponentGetTeamTerrRequest {
            request_sha256: [0; 32],
            first_read_authority_digest: first_read.composition_digest,
            staged_plan_digest: staged.composition_digest,
            call_entry_authority_digest: staged.call_entry_authority_digest,
            call_entry_sim_sha256: first_read.call_entry_sim_sha256,
            receiver_leader_slot: first_read.candidate_leader_slot,
            receiver_who: first_read.candidate_who,
            callsite_va: OPPONENT_GET_TEAM_TERR_CALL_VA,
            callee_va: GET_TEAM_TERR_VA,
            input_surface: Frame0GetTeamTerrInputSurface::CompleteLeaderGameTeamAndPlayerProjection,
        };
        request.request_sha256 = opponent_get_team_terr_request_digest(&request);
        Frame0PlanStrategyDiplomacyStepOpen::OpponentGetTeamTerr(request)
    };

    let mut plan = Frame0PlanStrategyDiplomacyStepPlan {
        revision: staged.revision,
        composition_digest: [0; 32],
        staged: staged.clone(),
        first_read: first_read.clone(),
        first_relation_is_ally,
        open,
        owner0_strategy_complete: false,
    };
    plan.composition_digest = frame0_plan_strategy_diplomacy_step_digest(&plan);
    Ok(plan)
}

fn validate_exact_diplomacy_step(
    parent: &Frame0PlanStrategyEntryAuthority,
    call_entry_authority: &Frame0GetTeamTerrCallEntryAuthority,
    first_child: &Frame0GetTeamTerrReceipt,
    step: &Frame0PlanStrategyDiplomacyStepPlan,
) -> bool {
    let Ok(staged) =
        plan_frame0_owner0_after_get_team_terr(parent, call_entry_authority, first_child)
    else {
        return false;
    };
    let Ok(expected) = advance_frame0_plan_strategy_first_diplomacy_read(&staged, &step.first_read)
    else {
        return false;
    };
    expected == *step
        && step.composition_digest == frame0_plan_strategy_diplomacy_step_digest(step)
        && !step.owner0_strategy_complete
}

pub fn frame0_plan_strategy_reverse_diplomacy_read_authority_digest(
    authority: &Frame0PlanStrategyReverseDiplomacyReadAuthority,
) -> [u8; 32] {
    let mut image = b"don-2024-frame0-plan-strategy-reverse-diplo-authority-v1".to_vec();
    image.extend_from_slice(&authority.revision.to_le_bytes());
    image.push(authority.source as u8);
    image.extend_from_slice(&authority.replay_file_sha256);
    image.extend_from_slice(&authority.executable_sha256);
    image.extend_from_slice(&authority.call_entry_authority_digest);
    image.extend_from_slice(&authority.diplomacy_step_digest);
    image.extend_from_slice(&authority.request_sha256);
    image.extend_from_slice(&authority.call_entry_sim_sha256);
    image.extend_from_slice(&[
        authority.receiver_leader_slot,
        authority.candidate_leader_slot,
    ]);
    image.extend_from_slice(&authority.receiver_who.to_le_bytes());
    image.extend_from_slice(&authority.candidate_who.to_le_bytes());
    image.extend_from_slice(&authority.target_who_index.to_le_bytes());
    image.extend_from_slice(&authority.read_va.to_le_bytes());
    image.extend_from_slice(&authority.value.to_le_bytes());
    sha256(&image)
}

/// Bind only the reverse directional relation from the exact call-entry Sim. The comparison and
/// any repeated child remain outside this authority.
pub fn bind_frame0_plan_strategy_reverse_diplomacy_read(
    parent: &Frame0PlanStrategyEntryAuthority,
    call_entry_authority: &Frame0GetTeamTerrCallEntryAuthority,
    first_child: &Frame0GetTeamTerrReceipt,
    step: &Frame0PlanStrategyDiplomacyStepPlan,
    call_entry: &Sim,
) -> Result<
    Frame0PlanStrategyReverseDiplomacyReadAuthority,
    Frame0PlanStrategyReverseDiplomacyReadBindError,
> {
    if !validate_exact_diplomacy_step(parent, call_entry_authority, first_child, step) {
        return Err(Frame0PlanStrategyReverseDiplomacyReadBindError::InvalidStagedPlan);
    }
    let Frame0PlanStrategyDiplomacyStepOpen::ReverseRead(request) = &step.open else {
        return Err(Frame0PlanStrategyReverseDiplomacyReadBindError::WrongBranch);
    };
    if !validate_frame0_plan_strategy_reverse_diplomacy_read_request(request)
        || request.first_read_authority_digest != step.first_read.composition_digest
        || request.staged_plan_digest != step.staged.composition_digest
        || request.call_entry_sim_sha256 != call_entry_authority.call_entry_sim_sha256
        || step.staged.call_entry_authority_digest != call_entry_authority.composition_digest
    {
        return Err(Frame0PlanStrategyReverseDiplomacyReadBindError::InvalidCallEntryAuthority);
    }
    let snapshot =
        save_sim(call_entry).map_err(Frame0PlanStrategyReverseDiplomacyReadBindError::Snapshot)?;
    if sha256(&snapshot) != call_entry_authority.call_entry_sim_sha256 {
        return Err(Frame0PlanStrategyReverseDiplomacyReadBindError::CallEntrySnapshotMismatch);
    }
    let receiver_slot = usize::from(request.receiver_leader_slot);
    let candidate_slot = usize::from(request.candidate_leader_slot);
    let target_index = usize::try_from(request.target_who_index)
        .ok()
        .filter(|index| *index < LEADER_SLOTS)
        .ok_or(Frame0PlanStrategyReverseDiplomacyReadBindError::TargetIndexOutsideLeaderTable)?;
    let live_receiver = &call_entry.vic_leaders.slots[receiver_slot];
    let projected_receiver = call_entry_authority.leader.leaders[receiver_slot];
    let projected_candidate = call_entry_authority.leader.leaders[candidate_slot];
    if live_receiver.leader_flags != projected_receiver.leader_flags
        || live_receiver.who != projected_receiver.who
        || live_receiver.territory != projected_receiver.territory
        || request.receiver_who != projected_receiver.who
        || request.candidate_who != projected_candidate.who
        || request.target_who_index != projected_candidate.who
        || request.read_va != REVERSE_DIRECTIONAL_DIPLO_READ_VA
    {
        return Err(Frame0PlanStrategyReverseDiplomacyReadBindError::ActorProjectionMismatch);
    }

    let mut authority = Frame0PlanStrategyReverseDiplomacyReadAuthority {
        revision: step.revision,
        composition_digest: [0; 32],
        source: Frame0PlanStrategyDiplomacyReadSource::CompleteRetailPlanStrategyCallEntry,
        replay_file_sha256: call_entry_authority.replay_file_sha256,
        executable_sha256: call_entry_authority.executable_sha256,
        call_entry_authority_digest: call_entry_authority.composition_digest,
        diplomacy_step_digest: step.composition_digest,
        request_sha256: request.request_sha256,
        call_entry_sim_sha256: call_entry_authority.call_entry_sim_sha256,
        receiver_leader_slot: request.receiver_leader_slot,
        receiver_who: request.receiver_who,
        candidate_leader_slot: request.candidate_leader_slot,
        candidate_who: request.candidate_who,
        target_who_index: request.target_who_index,
        read_va: request.read_va,
        value: live_receiver.diplos[target_index],
    };
    authority.composition_digest =
        frame0_plan_strategy_reverse_diplomacy_read_authority_digest(&authority);
    Ok(authority)
}

pub fn frame0_plan_strategy_opponent_get_team_terr_receipt_digest(
    receipt: &Frame0PlanStrategyOpponentGetTeamTerrReceipt,
) -> [u8; 32] {
    let mut image = b"don-2024-frame0-plan-strategy-opponent-get-team-terr-receipt-v1".to_vec();
    image.extend_from_slice(&receipt.revision.to_le_bytes());
    image.extend_from_slice(&receipt.replay_file_sha256);
    image.extend_from_slice(&receipt.executable_sha256);
    image.extend_from_slice(&receipt.request.request_sha256);
    image.extend_from_slice(&receipt.call_entry_authority_digest);
    image.extend_from_slice(&receipt.diplomacy_step_digest);
    image.extend_from_slice(&receipt.game_projection_digest);
    image.extend_from_slice(&receipt.leader_projection_digest);
    image.push(u8::from(receipt.staged_writes_preserved_input_projection));
    image.push(receipt.receiver_leader_slot);
    image.extend_from_slice(&(receipt.get_player_calls.len() as u64).to_le_bytes());
    for call in &receipt.get_player_calls {
        image.extend_from_slice(&(call.call_ordinal as u64).to_le_bytes());
        image.push(call.receiver_leader_slot);
        image.extend_from_slice(&call.receiver_who.to_le_bytes());
        image.extend_from_slice(&(call.visited_player_rows.len() as u64).to_le_bytes());
        image.extend_from_slice(&call.visited_player_rows);
        image.extend_from_slice(&(call.deferred_special_matches.len() as u64).to_le_bytes());
        image.extend_from_slice(&call.deferred_special_matches);
        image.push(u8::from(call.returned_early));
        image.push(call.result_player_row);
    }
    image.extend_from_slice(&(receipt.visits.len() as u64).to_le_bytes());
    for visit in &receipt.visits {
        image.extend_from_slice(&visit.leader_slot.to_le_bytes());
        image.extend_from_slice(&visit.leader_who.to_le_bytes());
        image.extend_from_slice(&visit.territory.to_le_bytes());
        image.extend_from_slice(&visit.sum_before.to_le_bytes());
        image.extend_from_slice(&visit.sum_after.to_le_bytes());
        match visit.disposition {
            Frame0GetTeamTerrDisposition::ReceiverSelf => image.push(0),
            Frame0GetTeamTerrDisposition::InvalidLeader => image.push(1),
            Frame0GetTeamTerrDisposition::ReceiverPlayerObserver => image.push(2),
            Frame0GetTeamTerrDisposition::CandidatePlayerObserver => image.push(3),
            Frame0GetTeamTerrDisposition::ReceiverTeamOutsideNormalRange { team } => {
                image.extend_from_slice(&[4, team as u8]);
            }
            Frame0GetTeamTerrDisposition::DifferentFrameZeroTeam {
                receiver_team,
                candidate_team,
            } => image.extend_from_slice(&[5, receiver_team as u8, candidate_team as u8]),
            Frame0GetTeamTerrDisposition::SameFrameZeroTeam { team } => {
                image.extend_from_slice(&[6, team as u8]);
            }
        }
    }
    image.extend_from_slice(&receipt.result.to_le_bytes());
    image.extend_from_slice(&receipt.return_va.to_le_bytes());
    image.extend_from_slice(&[
        u8::from(receipt.is_ally_reached),
        u8::from(receipt.owner0_strategy_complete),
    ]);
    sha256(&image)
}

/// Execute the repeated candidate team-territory call against the unchanged call-entry
/// projection. The prior strategy writes touch only escrow, City scratch, planning counters,
/// and the three team-territory output scalars; none is read by `get_team_terr`.
pub fn resolve_frame0_plan_strategy_opponent_get_team_terr(
    parent: &Frame0PlanStrategyEntryAuthority,
    call_entry_authority: &Frame0GetTeamTerrCallEntryAuthority,
    first_child: &Frame0GetTeamTerrReceipt,
    step: &Frame0PlanStrategyDiplomacyStepPlan,
) -> Result<Frame0PlanStrategyOpponentGetTeamTerrReceipt, Frame0PlanStrategyOpponentGetTeamTerrError>
{
    if !validate_exact_diplomacy_step(parent, call_entry_authority, first_child, step) {
        return Err(Frame0PlanStrategyOpponentGetTeamTerrError::InvalidStagedPlan);
    }
    let Frame0PlanStrategyDiplomacyStepOpen::OpponentGetTeamTerr(request) = &step.open else {
        return Err(Frame0PlanStrategyOpponentGetTeamTerrError::WrongBranch);
    };
    if !validate_frame0_plan_strategy_opponent_get_team_terr_request(request)
        || request.first_read_authority_digest != step.first_read.composition_digest
        || request.staged_plan_digest != step.staged.composition_digest
        || request.call_entry_authority_digest != call_entry_authority.composition_digest
        || request.call_entry_sim_sha256 != call_entry_authority.call_entry_sim_sha256
    {
        return Err(Frame0PlanStrategyOpponentGetTeamTerrError::InvalidRequest);
    }
    let expected_first = resolve_captured_frame0_get_team_terr(
        &step.staged.prior_local_prefix.open,
        call_entry_authority,
    )
    .map_err(|_| Frame0PlanStrategyOpponentGetTeamTerrError::InvalidCallEntryAuthority)?;
    if &expected_first != first_child
        || !call_entry_authority.local_prefix_preserved_input_projection
    {
        return Err(Frame0PlanStrategyOpponentGetTeamTerrError::InvalidCallEntryAuthority);
    }
    let receiver = usize::from(request.receiver_leader_slot);
    if receiver >= LEADER_SLOTS {
        return Err(Frame0PlanStrategyOpponentGetTeamTerrError::ReceiverOutsideLeaderTable);
    }
    if call_entry_authority.leader.leaders[receiver].who != request.receiver_who {
        return Err(Frame0PlanStrategyOpponentGetTeamTerrError::ReceiverWhoMismatch);
    }
    let core = compute_frame0_get_team_terr(receiver, call_entry_authority);
    let mut receipt = Frame0PlanStrategyOpponentGetTeamTerrReceipt {
        revision: step.revision,
        composition_digest: [0; 32],
        replay_file_sha256: call_entry_authority.replay_file_sha256,
        executable_sha256: call_entry_authority.executable_sha256,
        request: request.clone(),
        call_entry_authority_digest: call_entry_authority.composition_digest,
        diplomacy_step_digest: step.composition_digest,
        game_projection_digest: call_entry_authority.game.composition_digest,
        leader_projection_digest: call_entry_authority.leader.composition_digest,
        staged_writes_preserved_input_projection: true,
        receiver_leader_slot: request.receiver_leader_slot,
        get_player_calls: core.get_player_calls,
        visits: core.visits,
        result: core.result,
        return_va: OPPONENT_GET_TEAM_TERR_CALL_VA + 5,
        is_ally_reached: false,
        owner0_strategy_complete: false,
    };
    receipt.composition_digest =
        frame0_plan_strategy_opponent_get_team_terr_receipt_digest(&receipt);
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use don_sim::tick::lifecycle_host::PlayerTable;

    use crate::setup_2024_frame0_plan_strategy::{
        bind_golden_frame0_owner0_plan_strategy_entry,
        plan_golden_frame0_owner0_plan_strategy_prefix, Frame0PlanStrategyCityImage,
        Frame0PlanStrategyEntryCapture, Frame0PlanStrategyEntrySource,
        Frame0PlanStrategyLeaderImage, GOLDEN_CENTER_BUILD_O, GOLDEN_CENTER_CITY_SLOT,
        PLAN_ENTRY_SCRATCH_DWORDS,
    };

    fn hash(byte: u8) -> [u8; 32] {
        [byte; 32]
    }

    fn call_entry() -> Sim {
        let mut sim = Sim::new(7, 1);
        for (slot, leader) in sim.vic_leaders.slots.iter_mut().enumerate() {
            leader.leader_flags = LEADER_VALID;
            leader.who = slot as i32;
            leader.territory = (slot as i32 + 1) * 10;
        }
        sim.vic_leaders.slots[0].leader_flags = 0x7;
        let mut players = PlayerTable::new();
        for slot in 0..LEADER_SLOTS {
            players.seat(slot, PLAYER_VALID, slot as u8, slot as i8);
        }
        sim.players = Some(players);
        sim
    }

    fn parent(sim: &Sim) -> Frame0PlanStrategyEntryAuthority {
        let call_entry_sim_sha256 = sha256(&save_sim(sim).unwrap());
        bind_golden_frame0_owner0_plan_strategy_entry(Frame0PlanStrategyEntryCapture {
            revision: 3,
            source: Frame0PlanStrategyEntrySource::CompleteRetailOwnerZeroPlanStrategyEntry,
            replay_file_sha256: REPLAY_FILE_SHA256,
            executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
            completed_setup_sim_sha256: hash(1),
            setup_composition_digest: hash(2),
            preceding_chronology_digest: hash(3),
            call_entry_sim_sha256,
            native_trace_sha256: hash(4),
            frame: GOLDEN_FRAME,
            step: GOLDEN_STEP,
            owner: GOLDEN_FIRST_STRATEGY_OWNER,
            strategy_ordinal: GOLDEN_FIRST_STRATEGY_ORDINAL,
            leader: Frame0PlanStrategyLeaderImage {
                leader_flags: sim.vic_leaders.slots[0].leader_flags,
                who: sim.vic_leaders.slots[0].who,
                city_num: 1,
                village_num: 1,
                city_mark: 1,
                escrow_rate: [0; 6],
                scratch: [1; PLAN_ENTRY_SCRATCH_DWORDS],
            },
            cities: vec![Frame0PlanStrategyCityImage {
                slot: GOLDEN_CENTER_CITY_SLOT,
                city_flags: 1,
                city: GOLDEN_CENTER_CITY_SLOT,
                center_o: GOLDEN_CENTER_BUILD_O,
                who: GOLDEN_FIRST_STRATEGY_OWNER as i8,
                peasant_dist: -1,
                free: 1,
                busy: 2,
                gatherers: 3,
            }],
        })
        .unwrap()
    }

    fn source_capture(
        parent: &Frame0PlanStrategyEntryAuthority,
    ) -> Frame0GetTeamTerrCallEntryCapture {
        Frame0GetTeamTerrCallEntryCapture {
            revision: parent.revision,
            source: Frame0GetTeamTerrCallEntrySource::CompleteRetailOwnerZeroPlanStrategyEntry,
            native_trace_sha256: parent.capture.native_trace_sha256,
            replay_file_sha256: parent.capture.replay_file_sha256,
            executable_sha256: parent.capture.executable_sha256,
            plan_strategy_entry_authority_digest: parent.composition_digest,
            call_entry_sim_sha256: parent.capture.call_entry_sim_sha256,
            frame: parent.capture.frame,
            step: parent.capture.step,
            owner: parent.capture.owner,
            strategy_ordinal: parent.capture.strategy_ordinal,
            team_territory_preimage: Frame0PlanStrategyTeamTerritoryPreimage {
                other_team_terr: 91,
                min_other_team_terr: 92,
                my_team_terr: 93,
            },
        }
    }

    fn bind(
        sim: &Sim,
    ) -> (
        Frame0PlanStrategyEntryAuthority,
        Frame0GetTeamTerrCallEntryAuthority,
        Frame0GetTeamTerrRequest,
    ) {
        let parent = parent(sim);
        let authority =
            bind_captured_frame0_get_team_terr_call_entry(&parent, sim, source_capture(&parent))
                .unwrap();
        let request = plan_golden_frame0_owner0_plan_strategy_prefix(&parent)
            .unwrap()
            .open;
        (parent, authority, request)
    }

    fn refresh_authority_digests(authority: &mut Frame0GetTeamTerrCallEntryAuthority) {
        authority.game.composition_digest =
            frame0_get_team_terr_game_projection_digest(&authority.game);
        authority.leader.composition_digest =
            frame0_get_team_terr_leader_projection_digest(&authority.leader);
        authority.composition_digest = frame0_get_team_terr_call_entry_authority_digest(authority);
    }

    #[test]
    fn frame_zero_uses_player_teams_not_current_diplomacy() {
        let mut sim = call_entry();
        sim.players.as_mut().unwrap().players[1].team = 0;
        let (_, authority, request) = bind(&sim);
        let receipt = resolve_captured_frame0_get_team_terr(&request, &authority).unwrap();

        assert_eq!(receipt.result, 30);
        assert!(!receipt.is_ally_reached);
        assert_eq!(receipt.return_va, 0x006b_98d5);
        assert_eq!(receipt.get_player_calls.len(), 14);
        assert_eq!(
            receipt.visits[1].disposition,
            Frame0GetTeamTerrDisposition::SameFrameZeroTeam { team: 0 }
        );
        assert_eq!(
            receipt.visits[2].disposition,
            Frame0GetTeamTerrDisposition::DifferentFrameZeroTeam {
                receiver_team: 0,
                candidate_team: 2,
            }
        );
        assert_ne!(receipt.composition_digest, [0; 32]);
        assert_eq!(
            receipt.call_entry_authority_digest,
            authority.composition_digest
        );
        assert!(receipt.local_prefix_preserved_input_projection);
    }

    #[test]
    fn team_style_seven_excludes_observer_rows_before_team_compare() {
        let mut sim = call_entry();
        sim.vic_match.options.team_style = TEAM_STYLE_SPECIAL;
        sim.players.as_mut().unwrap().players[1].team = TEAM_OBSERVER;
        let (_, authority, request) = bind(&sim);
        let receipt = resolve_captured_frame0_get_team_terr(&request, &authority).unwrap();

        assert_eq!(receipt.result, 10);
        assert_eq!(
            receipt.visits[1].disposition,
            Frame0GetTeamTerrDisposition::CandidatePlayerObserver
        );
    }

    #[test]
    fn get_player_preserves_deferred_special_match_and_default_zero() {
        let mut sim = call_entry();
        let players = &mut sim.players.as_mut().unwrap().players;
        players[0].flags = 0;
        players[2].flags = PLAYER_VALID | PLAYER_DEFERRED_MATCH_MASK;
        players[2].who = 0;
        players[2].team = 1;
        players[5].flags = PLAYER_VALID | PLAYER_DEFERRED_MATCH_MASK;
        players[5].who = 0;
        players[5].team = 2;
        for player in players {
            if player.who == 7 {
                player.flags = 0;
            }
        }
        let (_, authority, request) = bind(&sim);
        let receipt = resolve_captured_frame0_get_team_terr(&request, &authority).unwrap();

        assert_eq!(receipt.get_player_calls[0].result_player_row, 5);
        assert_eq!(
            receipt.get_player_calls[0].deferred_special_matches,
            vec![2, 5]
        );
        assert!(!receipt.get_player_calls[0].returned_early);
        let slot_seven = receipt
            .get_player_calls
            .iter()
            .find(|call| call.receiver_leader_slot == 7)
            .unwrap();
        assert_eq!(slot_seven.result_player_row, 0);
    }

    #[test]
    fn result_addition_wraps_like_x86() {
        let mut sim = call_entry();
        sim.players.as_mut().unwrap().players[1].team = 0;
        sim.vic_leaders.slots[0].territory = i32::MAX;
        sim.vic_leaders.slots[1].territory = 2;
        let (_, authority, request) = bind(&sim);
        let receipt = resolve_captured_frame0_get_team_terr(&request, &authority).unwrap();
        assert_eq!(receipt.result, i32::MIN + 1);
    }

    #[test]
    fn binder_requires_exact_whole_sim_and_live_player_table() {
        let mut sim = call_entry();
        sim.players = None;
        let entry = parent(&sim);
        assert!(matches!(
            bind_captured_frame0_get_team_terr_call_entry(&entry, &sim, source_capture(&entry)),
            Err(Frame0GetTeamTerrCallEntryBindError::MissingPlayerTable)
        ));

        let mut sim = call_entry();
        let entry = parent(&sim);
        sim.vic_leaders.slots[1].territory ^= 1;
        assert!(matches!(
            bind_captured_frame0_get_team_terr_call_entry(&entry, &sim, source_capture(&entry)),
            Err(Frame0GetTeamTerrCallEntryBindError::CallEntrySnapshotMismatch)
        ));
    }

    #[test]
    fn stale_parent_and_call_entry_links_bite() {
        let sim = call_entry();
        let (_, authority, mut request) = bind(&sim);
        request.local_prefix_digest[0] ^= 1;
        assert_eq!(
            resolve_captured_frame0_get_team_terr(&request, &authority).unwrap_err(),
            Frame0GetTeamTerrError::InvalidParentRequest
        );

        let (_, mut authority, request) = bind(&sim);
        authority.call_entry_sim_sha256[0] ^= 1;
        authority.leader.call_entry_sim_sha256 = authority.call_entry_sim_sha256;
        refresh_authority_digests(&mut authority);
        assert_eq!(
            resolve_captured_frame0_get_team_terr(&request, &authority).unwrap_err(),
            Frame0GetTeamTerrError::CallEntrySnapshotMismatch
        );

        let (_, mut authority, request) = bind(&sim);
        authority.game.players[1].team ^= 1;
        authority.composition_digest = frame0_get_team_terr_call_entry_authority_digest(&authority);
        assert_eq!(
            resolve_captured_frame0_get_team_terr(&request, &authority).unwrap_err(),
            Frame0GetTeamTerrError::InvalidGameProjectionDigest
        );

        let (_, mut authority, request) = bind(&sim);
        authority.leader.leaders[1].territory ^= 1;
        authority.composition_digest = frame0_get_team_terr_call_entry_authority_digest(&authority);
        assert_eq!(
            resolve_captured_frame0_get_team_terr(&request, &authority).unwrap_err(),
            Frame0GetTeamTerrError::InvalidLeaderProjectionDigest
        );

        let (_, mut authority, request) = bind(&sim);
        authority.team_territory_preimage.my_team_terr ^= 1;
        assert_eq!(
            resolve_captured_frame0_get_team_terr(&request, &authority).unwrap_err(),
            Frame0GetTeamTerrError::InvalidCallEntryAuthority
        );
    }

    #[test]
    fn post_return_stages_native_stores_then_stops_at_first_directional_diplo_read() {
        let mut sim = call_entry();
        sim.vic_leaders.slots[1].leader_flags |= LEADER_ACTIVE;
        let (parent, authority, request) = bind(&sim);
        let receipt = resolve_captured_frame0_get_team_terr(&request, &authority).unwrap();
        let plan = plan_frame0_owner0_after_get_team_terr(&parent, &authority, &receipt).unwrap();

        assert_eq!(plan.prior_local_prefix.open, request);
        assert_eq!(plan.before, authority.team_territory_preimage);
        assert_eq!(
            plan.after_local,
            Frame0PlanStrategyTeamTerritoryPreimage {
                other_team_terr: 0,
                min_other_team_terr: 0,
                my_team_terr: receipt.result,
            }
        );
        assert_eq!(
            plan.writes,
            [
                Frame0PlanStrategyTeamTerritoryWrite {
                    instruction_va: 0x006b_98da,
                    field: Frame0PlanStrategyTeamTerritoryField::MyTeamTerr,
                    before: 93,
                    after: receipt.result,
                },
                Frame0PlanStrategyTeamTerritoryWrite {
                    instruction_va: 0x006b_98e5,
                    field: Frame0PlanStrategyTeamTerritoryField::OtherTeamTerr,
                    before: 91,
                    after: 0,
                },
                Frame0PlanStrategyTeamTerritoryWrite {
                    instruction_va: 0x006b_98ef,
                    field: Frame0PlanStrategyTeamTerritoryField::MinOtherTeamTerr,
                    before: 92,
                    after: 0,
                },
            ]
        );
        assert_eq!(plan.receiver_self_slot_skipped, 0);
        assert_eq!(plan.open.candidate_leader_slot, 1);
        assert_eq!(plan.open.candidate_who, 1);
        assert_eq!(plan.open.target_who_index, 0);
        assert!(validate_frame0_plan_strategy_diplomacy_read_request(
            &plan.open
        ));
        assert!(!plan.owner0_strategy_complete);
        assert_eq!(
            plan.composition_digest,
            frame0_plan_strategy_post_team_terr_digest(&plan)
        );
    }

    #[test]
    fn post_return_skips_locally_rejected_rows_without_reading_diplomacy() {
        let mut sim = call_entry();
        sim.vic_leaders.slots[1].leader_flags &= !LEADER_ACTIVE;
        sim.vic_leaders.slots[2].leader_flags |= LEADER_ACTIVE;
        let (parent, authority, request) = bind(&sim);
        let receipt = resolve_captured_frame0_get_team_terr(&request, &authority).unwrap();
        let plan = plan_frame0_owner0_after_get_team_terr(&parent, &authority, &receipt).unwrap();
        assert_eq!(plan.open.candidate_leader_slot, 2);

        let mut request = plan.open;
        request.candidate_leader_slot = 1;
        assert!(!validate_frame0_plan_strategy_diplomacy_read_request(
            &request
        ));
    }

    #[test]
    fn post_return_refuses_stale_child_and_missing_candidate() {
        let mut sim = call_entry();
        sim.vic_leaders.slots[1].leader_flags |= LEADER_ACTIVE;
        let (parent, authority, request) = bind(&sim);
        let mut receipt = resolve_captured_frame0_get_team_terr(&request, &authority).unwrap();
        receipt.result ^= 1;
        receipt.composition_digest = frame0_get_team_terr_receipt_digest(&receipt);
        assert_eq!(
            plan_frame0_owner0_after_get_team_terr(&parent, &authority, &receipt).unwrap_err(),
            Frame0PlanStrategyPostTeamTerrError::InvalidFirstChildReceipt
        );

        let sim = call_entry();
        let (parent, authority, request) = bind(&sim);
        let receipt = resolve_captured_frame0_get_team_terr(&request, &authority).unwrap();
        assert_eq!(
            plan_frame0_owner0_after_get_team_terr(&parent, &authority, &receipt).unwrap_err(),
            Frame0PlanStrategyPostTeamTerrError::NoQualifyingDiplomacyCandidate
        );
    }

    fn diplomacy_frontier(
        sim: &Sim,
    ) -> (
        Frame0PlanStrategyEntryAuthority,
        Frame0GetTeamTerrCallEntryAuthority,
        Frame0GetTeamTerrReceipt,
        Frame0PlanStrategyPostTeamTerrPlan,
    ) {
        let (parent, authority, request) = bind(sim);
        let receipt = resolve_captured_frame0_get_team_terr(&request, &authority).unwrap();
        let staged = plan_frame0_owner0_after_get_team_terr(&parent, &authority, &receipt).unwrap();
        (parent, authority, receipt, staged)
    }

    #[test]
    fn first_nonally_direction_reaches_repeated_team_territory_child_without_writes() {
        let mut sim = call_entry();
        sim.vic_leaders.slots[1].leader_flags |= LEADER_ACTIVE;
        sim.vic_leaders.slots[1].diplos[0] = 0;
        let (parent, authority, receipt, staged) = diplomacy_frontier(&sim);
        let read = bind_frame0_plan_strategy_first_diplomacy_read(
            &parent, &authority, &receipt, &staged, &sim,
        )
        .unwrap();
        assert_eq!(read.value, 0);
        assert_eq!(read.read_va, 0x006b_9910);

        let next = advance_frame0_plan_strategy_first_diplomacy_read(&staged, &read).unwrap();
        assert_eq!(next.staged, staged);
        assert!(!next.first_relation_is_ally);
        let Frame0PlanStrategyDiplomacyStepOpen::OpponentGetTeamTerr(ref request) = next.open
        else {
            panic!("nonally first direction must call get_team_terr");
        };
        assert_eq!(request.receiver_leader_slot, 1);
        assert_eq!(request.receiver_who, 1);
        assert_eq!(request.callsite_va, 0x006b_992e);
        assert_eq!(request.callee_va, 0x006d_62e0);
        assert!(validate_frame0_plan_strategy_opponent_get_team_terr_request(&request));
        let opponent = resolve_frame0_plan_strategy_opponent_get_team_terr(
            &parent, &authority, &receipt, &next,
        )
        .unwrap();
        assert_eq!(opponent.receiver_leader_slot, 1);
        assert_eq!(opponent.result, 20);
        assert_eq!(opponent.return_va, 0x006b_9933);
        assert!(opponent.staged_writes_preserved_input_projection);
        assert!(!opponent.is_ally_reached);
        assert!(!opponent.owner0_strategy_complete);
        assert_eq!(
            opponent.composition_digest,
            frame0_plan_strategy_opponent_get_team_terr_receipt_digest(&opponent)
        );
        assert!(!next.owner0_strategy_complete);
        assert_eq!(
            next.composition_digest,
            frame0_plan_strategy_diplomacy_step_digest(&next)
        );
    }

    #[test]
    fn first_ally_direction_stops_before_reverse_relation_read() {
        let mut sim = call_entry();
        sim.vic_leaders.slots[1].leader_flags |= LEADER_ACTIVE;
        sim.vic_leaders.slots[1].diplos[0] = DIPLO_ALLY;
        let (parent, authority, receipt, staged) = diplomacy_frontier(&sim);
        let read = bind_frame0_plan_strategy_first_diplomacy_read(
            &parent, &authority, &receipt, &staged, &sim,
        )
        .unwrap();
        let next = advance_frame0_plan_strategy_first_diplomacy_read(&staged, &read).unwrap();
        assert!(next.first_relation_is_ally);
        let Frame0PlanStrategyDiplomacyStepOpen::ReverseRead(ref request) = next.open else {
            panic!("ally first direction must read the reverse relation");
        };
        assert_eq!(request.receiver_leader_slot, 0);
        assert_eq!(request.candidate_leader_slot, 1);
        assert_eq!(request.target_who_index, 1);
        assert_eq!(request.read_va, 0x006b_9925);
        assert!(validate_frame0_plan_strategy_reverse_diplomacy_read_request(&request));
        let reverse = bind_frame0_plan_strategy_reverse_diplomacy_read(
            &parent, &authority, &receipt, &next, &sim,
        )
        .unwrap();
        assert_eq!(reverse.receiver_leader_slot, 0);
        assert_eq!(reverse.target_who_index, 1);
        assert_eq!(reverse.read_va, 0x006b_9925);
        assert_eq!(reverse.value, sim.vic_leaders.slots[0].diplos[1]);
        assert_eq!(
            reverse.composition_digest,
            frame0_plan_strategy_reverse_diplomacy_read_authority_digest(&reverse)
        );
        assert!(!next.owner0_strategy_complete);
    }

    #[test]
    fn diplomacy_read_binder_rejects_stale_sim_and_step_rejects_stale_authority() {
        let mut sim = call_entry();
        sim.vic_leaders.slots[1].leader_flags |= LEADER_ACTIVE;
        let (parent, authority, receipt, staged) = diplomacy_frontier(&sim);

        sim.vic_leaders.slots[1].diplos[0] ^= 1;
        assert!(matches!(
            bind_frame0_plan_strategy_first_diplomacy_read(
                &parent, &authority, &receipt, &staged, &sim,
            ),
            Err(Frame0PlanStrategyDiplomacyReadBindError::CallEntrySnapshotMismatch)
        ));
        sim.vic_leaders.slots[1].diplos[0] ^= 1;

        let mut read = bind_frame0_plan_strategy_first_diplomacy_read(
            &parent, &authority, &receipt, &staged, &sim,
        )
        .unwrap();
        read.value ^= 1;
        assert_eq!(
            advance_frame0_plan_strategy_first_diplomacy_read(&staged, &read).unwrap_err(),
            Frame0PlanStrategyDiplomacyStepError::InvalidReadAuthority
        );
    }

    #[test]
    fn reverse_read_binds_only_live_reverse_cell_and_repeated_child_refuses_wrong_branch() {
        let mut sim = call_entry();
        sim.vic_leaders.slots[1].leader_flags |= LEADER_ACTIVE;
        sim.vic_leaders.slots[1].diplos[0] = DIPLO_ALLY;
        sim.vic_leaders.slots[0].diplos[1] = DIPLO_ALLY;
        let (parent, authority, receipt, staged) = diplomacy_frontier(&sim);
        let first = bind_frame0_plan_strategy_first_diplomacy_read(
            &parent, &authority, &receipt, &staged, &sim,
        )
        .unwrap();
        let step = advance_frame0_plan_strategy_first_diplomacy_read(&staged, &first).unwrap();
        let reverse = bind_frame0_plan_strategy_reverse_diplomacy_read(
            &parent, &authority, &receipt, &step, &sim,
        )
        .unwrap();
        assert_eq!(reverse.value, DIPLO_ALLY);
        assert_eq!(
            reverse.call_entry_sim_sha256,
            authority.call_entry_sim_sha256
        );
        assert_eq!(
            resolve_frame0_plan_strategy_opponent_get_team_terr(
                &parent, &authority, &receipt, &step,
            )
            .unwrap_err(),
            Frame0PlanStrategyOpponentGetTeamTerrError::WrongBranch
        );

        sim.vic_leaders.slots[0].diplos[1] = 0;
        assert!(matches!(
            bind_frame0_plan_strategy_reverse_diplomacy_read(
                &parent, &authority, &receipt, &step, &sim,
            ),
            Err(Frame0PlanStrategyReverseDiplomacyReadBindError::CallEntrySnapshotMismatch)
        ));
    }
}
