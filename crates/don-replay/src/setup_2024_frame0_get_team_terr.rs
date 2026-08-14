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
    Frame0PlanStrategyError, GET_TEAM_TERR_CALL_VA, GET_TEAM_TERR_VA,
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
    }
}
