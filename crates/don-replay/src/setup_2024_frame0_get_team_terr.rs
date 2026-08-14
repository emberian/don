// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact frame-zero `LeaderData::get_team_terr` child for the golden strategy pass.
//!
//! `get_team_terr` (`0x006d62e0`) has two materially different relation paths. Once
//! `Game::frame` is nonzero it can call `LeaderData::is_ally`; at exact frame zero it instead
//! calls `LeaderData::get_player` and compares `GameInfo::Player::team`. The existing simulator
//! helper represents the later ally path and is therefore not evidence for this call.
//!
//! This module binds the complete frame-zero input surface: `GameInfo::team_style`, all eight
//! Player flag/who/team rows, and all eight current Leader flag/who/territory rows. The Player
//! projection may originate in the exact post-`Game::init_teams` setup owner, but the Leader
//! projection must be captured again at this call boundary; setup-time Leader rows and missing
//! final-territory production are not accepted as substitutes. Resolution is detached and has
//! no canonical writes.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::systems::unit_inctime::SUPPORTED_RETAIL_EXE_SHA256;

use crate::setup_2024_frame0_plan_strategy::{
    validate_frame0_get_team_terr_request, Frame0GetTeamTerrInputSurface, Frame0GetTeamTerrRequest,
    GET_TEAM_TERR_CALL_VA, GET_TEAM_TERR_VA, GOLDEN_FRAME,
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
pub enum Frame0GetTeamTerrGameSource {
    /// Exact supported-replay GameInfo projection produced after `Game::init_teams` and retained
    /// unchanged through this frame-zero call entry.
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0GetTeamTerrCapture {
    pub revision: u64,
    pub native_trace_sha256: [u8; 32],
    pub replay_file_sha256: [u8; 32],
    pub executable_sha256: [u8; 32],
    pub request_sha256: [u8; 32],
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
    pub game_projection_digest: [u8; 32],
    pub leader_projection_digest: [u8; 32],
    pub receiver_leader_slot: u8,
    pub get_player_calls: Vec<Frame0GetPlayerReceipt>,
    pub visits: Vec<Frame0GetTeamTerrVisit>,
    pub result: i32,
    pub return_va: u32,
    pub is_ally_reached: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame0GetTeamTerrError {
    InvalidParentRequest,
    MissingCaptureRevision,
    MissingNativeTrace,
    ReplayMismatch,
    UnsupportedExecutable,
    CaptureRequestMismatch,
    MissingGameProjectionRevision,
    MissingGameProjectionDigest,
    InvalidGameProjectionDigest,
    WrongGameProjectionSource,
    WrongFrame { expected: i32, actual: i32 },
    MissingLeaderProjectionRevision,
    MissingLeaderProjectionDigest,
    InvalidLeaderProjectionDigest,
    WrongLeaderProjectionSource,
    MissingCallEntrySnapshot,
    CallEntrySnapshotMismatch,
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
    image.extend_from_slice(&receipt.game_projection_digest);
    image.extend_from_slice(&receipt.leader_projection_digest);
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
    capture: &Frame0GetTeamTerrCapture,
) -> Result<Frame0GetTeamTerrReceipt, Frame0GetTeamTerrError> {
    if !validate_frame0_get_team_terr_request(request)
        || request.callsite_va != GET_TEAM_TERR_CALL_VA
        || request.callee_va != GET_TEAM_TERR_VA
        || request.input_surface
            != Frame0GetTeamTerrInputSurface::CompleteLeaderGameTeamAndPlayerProjection
    {
        return Err(Frame0GetTeamTerrError::InvalidParentRequest);
    }
    if capture.revision == 0 {
        return Err(Frame0GetTeamTerrError::MissingCaptureRevision);
    }
    if capture.native_trace_sha256 == [0; 32] {
        return Err(Frame0GetTeamTerrError::MissingNativeTrace);
    }
    if capture.replay_file_sha256 != REPLAY_FILE_SHA256 {
        return Err(Frame0GetTeamTerrError::ReplayMismatch);
    }
    if capture.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256 {
        return Err(Frame0GetTeamTerrError::UnsupportedExecutable);
    }
    if capture.request_sha256 != request.request_sha256 {
        return Err(Frame0GetTeamTerrError::CaptureRequestMismatch);
    }
    if capture.game.revision == 0 {
        return Err(Frame0GetTeamTerrError::MissingGameProjectionRevision);
    }
    if capture.game.composition_digest == [0; 32] {
        return Err(Frame0GetTeamTerrError::MissingGameProjectionDigest);
    }
    if capture.game.composition_digest != frame0_get_team_terr_game_projection_digest(&capture.game)
    {
        return Err(Frame0GetTeamTerrError::InvalidGameProjectionDigest);
    }
    if capture.game.source != Frame0GetTeamTerrGameSource::SourceBackedGameInfoPlayerTeamProjection
    {
        return Err(Frame0GetTeamTerrError::WrongGameProjectionSource);
    }
    if capture.game.frame != GOLDEN_FRAME {
        return Err(Frame0GetTeamTerrError::WrongFrame {
            expected: GOLDEN_FRAME,
            actual: capture.game.frame,
        });
    }
    if capture.leader.revision == 0 {
        return Err(Frame0GetTeamTerrError::MissingLeaderProjectionRevision);
    }
    if capture.leader.composition_digest == [0; 32] {
        return Err(Frame0GetTeamTerrError::MissingLeaderProjectionDigest);
    }
    if capture.leader.composition_digest
        != frame0_get_team_terr_leader_projection_digest(&capture.leader)
    {
        return Err(Frame0GetTeamTerrError::InvalidLeaderProjectionDigest);
    }
    if capture.leader.source != Frame0GetTeamTerrLeaderSource::CompleteRetailGetTeamTerrCallEntry {
        return Err(Frame0GetTeamTerrError::WrongLeaderProjectionSource);
    }
    if capture.leader.call_entry_sim_sha256 == [0; 32] {
        return Err(Frame0GetTeamTerrError::MissingCallEntrySnapshot);
    }
    if capture.leader.call_entry_sim_sha256 != request.call_entry_sim_sha256 {
        return Err(Frame0GetTeamTerrError::CallEntrySnapshotMismatch);
    }
    let receiver = usize::from(request.receiver_owner);
    if receiver >= LEADER_SLOTS {
        return Err(Frame0GetTeamTerrError::ReceiverOutsideLeaderTable);
    }
    if capture.leader.leaders[receiver].who != receiver as i32 {
        return Err(Frame0GetTeamTerrError::ReceiverWhoMismatch);
    }

    let mut sum = 0i32;
    let mut get_player_calls = Vec::new();
    let mut visits = Vec::with_capacity(LEADER_SLOTS);
    for slot in 0..LEADER_SLOTS {
        let leader = capture.leader.leaders[slot];
        let sum_before = sum;
        let disposition;
        if slot == receiver {
            sum = sum.wrapping_add(leader.territory);
            disposition = Frame0GetTeamTerrDisposition::ReceiverSelf;
        } else if leader.leader_flags & LEADER_VALID == 0 {
            disposition = Frame0GetTeamTerrDisposition::InvalidLeader;
        } else {
            if capture.game.team_style == TEAM_STYLE_SPECIAL {
                let receiver_player = get_player(
                    &capture.leader.leaders,
                    &capture.game.players,
                    receiver,
                    &mut get_player_calls,
                );
                let row = capture.game.players[receiver_player];
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
                    &capture.leader.leaders,
                    &capture.game.players,
                    slot,
                    &mut get_player_calls,
                );
                let row = capture.game.players[candidate_player];
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
                &capture.leader.leaders,
                &capture.game.players,
                receiver,
                &mut get_player_calls,
            );
            let receiver_team = capture.game.players[receiver_player].team;
            if !(FIRST_NORMAL_TEAM..FIRST_NORMAL_TEAM + NORMAL_TEAM_COUNT).contains(&receiver_team)
            {
                disposition = Frame0GetTeamTerrDisposition::ReceiverTeamOutsideNormalRange {
                    team: receiver_team,
                };
            } else {
                let candidate_player = get_player(
                    &capture.leader.leaders,
                    &capture.game.players,
                    slot,
                    &mut get_player_calls,
                );
                let candidate_team = capture.game.players[candidate_player].team;
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
        revision: capture.revision,
        composition_digest: [0; 32],
        replay_file_sha256: capture.replay_file_sha256,
        executable_sha256: capture.executable_sha256,
        request: request.clone(),
        native_trace_sha256: capture.native_trace_sha256,
        game_projection_digest: capture.game.composition_digest,
        leader_projection_digest: capture.leader.composition_digest,
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
    use crate::setup_2024_frame0_plan_strategy::{
        Frame0GetTeamTerrInputSurface, Frame0GetTeamTerrRequest,
    };

    fn hash(byte: u8) -> [u8; 32] {
        [byte; 32]
    }

    fn fixture_request() -> Frame0GetTeamTerrRequest {
        let mut request = Frame0GetTeamTerrRequest {
            request_sha256: [0; 32],
            parent_authority_digest: hash(1),
            local_prefix_digest: hash(2),
            call_entry_sim_sha256: hash(3),
            receiver_owner: 0,
            callsite_va: GET_TEAM_TERR_CALL_VA,
            callee_va: GET_TEAM_TERR_VA,
            input_surface: Frame0GetTeamTerrInputSurface::CompleteLeaderGameTeamAndPlayerProjection,
        };
        let mut image = b"don-2024-frame0-get-team-terr-request-v1".to_vec();
        image.extend_from_slice(&request.parent_authority_digest);
        image.extend_from_slice(&request.local_prefix_digest);
        image.extend_from_slice(&request.call_entry_sim_sha256);
        image.push(request.receiver_owner);
        image.extend_from_slice(&request.callsite_va.to_le_bytes());
        image.extend_from_slice(&request.callee_va.to_le_bytes());
        image.push(request.input_surface as u8);
        request.request_sha256 = sha256(&image);
        request
    }

    fn fixture_capture() -> Frame0GetTeamTerrCapture {
        let players = std::array::from_fn(|slot| Frame0GetTeamTerrPlayerRow {
            flags: PLAYER_VALID,
            who: slot as u8,
            team: slot as i8,
        });
        let leaders = std::array::from_fn(|slot| Frame0GetTeamTerrLeaderRow {
            leader_flags: LEADER_VALID,
            who: slot as i32,
            territory: (slot as i32 + 1) * 10,
        });
        let mut capture = Frame0GetTeamTerrCapture {
            revision: 1,
            native_trace_sha256: hash(4),
            replay_file_sha256: REPLAY_FILE_SHA256,
            executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
            request_sha256: fixture_request().request_sha256,
            game: Frame0GetTeamTerrGameProjection {
                revision: 2,
                composition_digest: [0; 32],
                source: Frame0GetTeamTerrGameSource::SourceBackedGameInfoPlayerTeamProjection,
                frame: GOLDEN_FRAME,
                team_style: 0,
                players,
            },
            leader: Frame0GetTeamTerrLeaderProjection {
                revision: 3,
                composition_digest: [0; 32],
                source: Frame0GetTeamTerrLeaderSource::CompleteRetailGetTeamTerrCallEntry,
                call_entry_sim_sha256: fixture_request().call_entry_sim_sha256,
                leaders,
            },
        };
        capture.game.composition_digest =
            frame0_get_team_terr_game_projection_digest(&capture.game);
        capture.leader.composition_digest =
            frame0_get_team_terr_leader_projection_digest(&capture.leader);
        capture
    }

    fn refresh_projection_digests(capture: &mut Frame0GetTeamTerrCapture) {
        capture.game.composition_digest =
            frame0_get_team_terr_game_projection_digest(&capture.game);
        capture.leader.composition_digest =
            frame0_get_team_terr_leader_projection_digest(&capture.leader);
    }

    #[test]
    fn frame_zero_uses_player_teams_not_current_diplomacy() {
        let request = fixture_request();
        let mut capture = fixture_capture();
        capture.game.players[1].team = 0;
        refresh_projection_digests(&mut capture);
        let receipt = resolve_captured_frame0_get_team_terr(&request, &capture).unwrap();

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
    }

    #[test]
    fn team_style_seven_excludes_observer_rows_before_team_compare() {
        let request = fixture_request();
        let mut capture = fixture_capture();
        capture.game.team_style = TEAM_STYLE_SPECIAL;
        capture.game.players[1].team = TEAM_OBSERVER;
        refresh_projection_digests(&mut capture);
        let receipt = resolve_captured_frame0_get_team_terr(&request, &capture).unwrap();

        assert_eq!(receipt.result, 10);
        assert_eq!(
            receipt.visits[1].disposition,
            Frame0GetTeamTerrDisposition::CandidatePlayerObserver
        );
    }

    #[test]
    fn get_player_preserves_deferred_special_match_and_default_zero() {
        let request = fixture_request();
        let mut capture = fixture_capture();
        capture.game.players[0].flags = 0;
        capture.game.players[2] = Frame0GetTeamTerrPlayerRow {
            flags: PLAYER_VALID | PLAYER_DEFERRED_MATCH_MASK,
            who: 0,
            team: 1,
        };
        capture.game.players[5] = Frame0GetTeamTerrPlayerRow {
            flags: PLAYER_VALID | PLAYER_DEFERRED_MATCH_MASK,
            who: 0,
            team: 2,
        };
        for player in &mut capture.game.players {
            if player.who == 7 {
                player.flags = 0;
            }
        }
        refresh_projection_digests(&mut capture);
        let receipt = resolve_captured_frame0_get_team_terr(&request, &capture).unwrap();

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
        let request = fixture_request();
        let mut capture = fixture_capture();
        capture.game.players[1].team = 0;
        capture.leader.leaders[0].territory = i32::MAX;
        capture.leader.leaders[1].territory = 2;
        refresh_projection_digests(&mut capture);
        let receipt = resolve_captured_frame0_get_team_terr(&request, &capture).unwrap();
        assert_eq!(receipt.result, i32::MIN + 1);
    }

    #[test]
    fn missing_runtime_leader_join_and_nonzero_frame_refuse() {
        let request = fixture_request();
        let mut capture = fixture_capture();
        capture.leader.composition_digest = [0; 32];
        assert_eq!(
            resolve_captured_frame0_get_team_terr(&request, &capture).unwrap_err(),
            Frame0GetTeamTerrError::MissingLeaderProjectionDigest
        );

        let mut capture = fixture_capture();
        capture.game.frame = 1;
        refresh_projection_digests(&mut capture);
        assert_eq!(
            resolve_captured_frame0_get_team_terr(&request, &capture).unwrap_err(),
            Frame0GetTeamTerrError::WrongFrame {
                expected: 0,
                actual: 1,
            }
        );
    }

    #[test]
    fn stale_parent_and_call_entry_links_bite() {
        let mut request = fixture_request();
        request.local_prefix_digest[0] ^= 1;
        assert_eq!(
            resolve_captured_frame0_get_team_terr(&request, &fixture_capture()).unwrap_err(),
            Frame0GetTeamTerrError::InvalidParentRequest
        );

        let request = fixture_request();
        let mut capture = fixture_capture();
        capture.leader.call_entry_sim_sha256[0] ^= 1;
        refresh_projection_digests(&mut capture);
        assert_eq!(
            resolve_captured_frame0_get_team_terr(&request, &capture).unwrap_err(),
            Frame0GetTeamTerrError::CallEntrySnapshotMismatch
        );

        let mut capture = fixture_capture();
        capture.game.players[1].team ^= 1;
        assert_eq!(
            resolve_captured_frame0_get_team_terr(&request, &capture).unwrap_err(),
            Frame0GetTeamTerrError::InvalidGameProjectionDigest
        );

        let mut capture = fixture_capture();
        capture.leader.leaders[1].territory ^= 1;
        assert_eq!(
            resolve_captured_frame0_get_team_terr(&request, &capture).unwrap_err(),
            Frame0GetTeamTerrError::InvalidLeaderProjectionDigest
        );
    }
}
