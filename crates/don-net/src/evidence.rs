//! Bounded persisted evidence for [`LockstepRunner`](crate::LockstepRunner).
//!
//! The binary stores inputs and outcomes. Reading it is therefore validation,
//! not deserialization alone: every setup epoch, exact package, deadline
//! observation, and commit is re-executed, and the regenerated canonical
//! outcome must match the stored JSON and hash byte-for-byte.

use crate::lockstep::{fnv1a64, EpochCause, LockstepError, LockstepEvidence, LockstepRunner};
use crate::session::TurnPackage;

pub const LOCKSTEP_EVIDENCE_MAGIC: [u8; 8] = *b"DONLSTP\0";
pub const LOCKSTEP_EVIDENCE_VERSION: u16 = 1;
pub const MAX_LOCKSTEP_EVIDENCE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_LOCKSTEP_ACTIONS: usize = 65_536;
pub const MAX_LOCKSTEP_OUTCOME_BYTES: usize = 4 * 1024 * 1024;

const HEADER_LEN: usize = 40;
const RECORD_INITIAL: u8 = 1;
const RECORD_EPOCH: u8 = 2;
const RECORD_PACKAGE: u8 = 3;
const RECORD_OBSERVE_DEADLINE: u8 = 4;
const RECORD_COMMIT: u8 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpochMember {
    pub play: i8,
    pub unique_id: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayAction {
    Initial {
        at_ms: u64,
        first_stamp: u32,
        members: Vec<EpochMember>,
    },
    Epoch {
        at_ms: u64,
        cause: EpochCause,
        members: Vec<EpochMember>,
    },
    Package {
        at_ms: u64,
        package: TurnPackage,
    },
    ObserveDeadline {
        at_ms: u64,
    },
    Commit {
        at_ms: u64,
    },
}

impl ReplayAction {
    fn at_ms(&self) -> u64 {
        match self {
            Self::Initial { at_ms, .. }
            | Self::Epoch { at_ms, .. }
            | Self::Package { at_ms, .. }
            | Self::ObserveDeadline { at_ms }
            | Self::Commit { at_ms } => *at_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayedLockstep {
    pub outcome_json: String,
    pub outcome_fnv1a64: u64,
    pub next_stamp: u32,
    pub final_epoch: u32,
    pub evidence: Vec<LockstepEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedLockstepTranscript {
    game_key: u32,
    timeout_ms: u64,
    actions: Vec<ReplayAction>,
    outcome_json: String,
    outcome_fnv1a64: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvidenceError {
    TranscriptTooLarge(usize),
    TooManyActions(usize),
    OutcomeTooLarge(usize),
    BadMagic,
    UnsupportedVersion(u16),
    NonzeroFlags(u16),
    Truncated {
        need: usize,
        have: usize,
    },
    Trailing(usize),
    UnknownRecord(u8),
    MalformedRecord {
        ty: u8,
        len: usize,
    },
    PackageTooLarge(usize),
    InvalidEpochCause(u8),
    InitialNotFirst,
    MissingInitial,
    InitialCauseAfterStart,
    NoncanonicalMembers,
    NonMonotonicTime {
        action: usize,
        previous_ms: u64,
        actual_ms: u64,
    },
    OutcomeNotUtf8,
    OutcomeHashMismatch {
        stored: u64,
        actual: u64,
    },
    ReplayOutcomeMismatch,
    Replay {
        action: usize,
        error: LockstepError,
    },
}

impl core::fmt::Display for EvidenceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TranscriptTooLarge(size) => {
                write!(
                    f,
                    "lockstep evidence is {size} bytes; bounded at {MAX_LOCKSTEP_EVIDENCE_BYTES}"
                )
            }
            Self::TooManyActions(count) => {
                write!(
                    f,
                    "lockstep evidence has {count} actions; bounded at {MAX_LOCKSTEP_ACTIONS}"
                )
            }
            Self::OutcomeTooLarge(size) => write!(
                f,
                "lockstep outcome is {size} bytes; bounded at {MAX_LOCKSTEP_OUTCOME_BYTES}"
            ),
            Self::BadMagic => write!(f, "not a DON lockstep evidence transcript"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported lockstep evidence version {version}")
            }
            Self::NonzeroFlags(flags) => write!(f, "unknown lockstep evidence flags {flags:#06x}"),
            Self::Truncated { need, have } => {
                write!(f, "truncated lockstep evidence: need {need}, have {have}")
            }
            Self::Trailing(bytes) => write!(f, "lockstep evidence has {bytes} trailing bytes"),
            Self::UnknownRecord(ty) => write!(f, "unknown lockstep evidence record {ty}"),
            Self::MalformedRecord { ty, len } => {
                write!(
                    f,
                    "malformed lockstep evidence record {ty} with {len} bytes"
                )
            }
            Self::PackageTooLarge(size) => {
                write!(
                    f,
                    "persisted command payload is {size} bytes; bounded at 512"
                )
            }
            Self::InvalidEpochCause(cause) => write!(f, "invalid persisted epoch cause {cause}"),
            Self::InitialNotFirst => write!(f, "initial setup record is not first"),
            Self::MissingInitial => write!(f, "lockstep evidence has no initial setup record"),
            Self::InitialCauseAfterStart => {
                write!(f, "initial epoch cause repeated after transcript start")
            }
            Self::NoncanonicalMembers => {
                write!(
                    f,
                    "persisted members require unique nonzero ids and sorted slots 0..7"
                )
            }
            Self::NonMonotonicTime {
                action,
                previous_ms,
                actual_ms,
            } => write!(f, "action {action} time {actual_ms} precedes {previous_ms}"),
            Self::OutcomeNotUtf8 => write!(f, "persisted lockstep outcome is not UTF-8"),
            Self::OutcomeHashMismatch { stored, actual } => write!(
                f,
                "persisted outcome hash {stored:016x} does not match bytes {actual:016x}"
            ),
            Self::ReplayOutcomeMismatch => {
                write!(
                    f,
                    "re-executed lockstep outcome differs from persisted outcome"
                )
            }
            Self::Replay { action, error } => {
                write!(f, "lockstep replay action {action} refused: {error}")
            }
        }
    }
}

impl std::error::Error for EvidenceError {}

impl PersistedLockstepTranscript {
    pub fn record(
        game_key: u32,
        timeout_ms: u64,
        actions: Vec<ReplayAction>,
    ) -> Result<Self, EvidenceError> {
        let replay = execute(game_key, timeout_ms, &actions)?;
        let transcript = Self {
            game_key,
            timeout_ms,
            actions,
            outcome_json: replay.outcome_json,
            outcome_fnv1a64: replay.outcome_fnv1a64,
        };
        transcript.validate_bounds()?;
        Ok(transcript)
    }

    pub fn game_key(&self) -> u32 {
        self.game_key
    }

    pub fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }

    pub fn actions(&self) -> &[ReplayAction] {
        &self.actions
    }

    pub fn outcome_json(&self) -> &str {
        &self.outcome_json
    }

    pub fn outcome_fnv1a64(&self) -> u64 {
        self.outcome_fnv1a64
    }

    pub fn replay(&self) -> Result<ReplayedLockstep, EvidenceError> {
        self.validate_bounds()?;
        let actual_stored_hash = fnv1a64(self.outcome_json.as_bytes());
        if actual_stored_hash != self.outcome_fnv1a64 {
            return Err(EvidenceError::OutcomeHashMismatch {
                stored: self.outcome_fnv1a64,
                actual: actual_stored_hash,
            });
        }
        let replay = execute(self.game_key, self.timeout_ms, &self.actions)?;
        if replay.outcome_fnv1a64 != self.outcome_fnv1a64
            || replay.outcome_json != self.outcome_json
        {
            return Err(EvidenceError::ReplayOutcomeMismatch);
        }
        Ok(replay)
    }

    pub fn encode(&self) -> Result<Vec<u8>, EvidenceError> {
        self.replay()?;
        let mut records = Vec::new();
        for action in &self.actions {
            encode_action(action, &mut records)?;
        }
        let total = HEADER_LEN
            .checked_add(records.len())
            .and_then(|size| size.checked_add(self.outcome_json.len()))
            .ok_or(EvidenceError::TranscriptTooLarge(usize::MAX))?;
        if total > MAX_LOCKSTEP_EVIDENCE_BYTES {
            return Err(EvidenceError::TranscriptTooLarge(total));
        }
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(&LOCKSTEP_EVIDENCE_MAGIC);
        out.extend_from_slice(&LOCKSTEP_EVIDENCE_VERSION.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(self.actions.len() as u32).to_le_bytes());
        out.extend_from_slice(&(self.outcome_json.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.outcome_fnv1a64.to_le_bytes());
        out.extend_from_slice(&self.game_key.to_le_bytes());
        out.extend_from_slice(&self.timeout_ms.to_le_bytes());
        debug_assert_eq!(out.len(), HEADER_LEN);
        out.extend_from_slice(&records);
        out.extend_from_slice(self.outcome_json.as_bytes());
        Ok(out)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, EvidenceError> {
        if bytes.len() > MAX_LOCKSTEP_EVIDENCE_BYTES {
            return Err(EvidenceError::TranscriptTooLarge(bytes.len()));
        }
        let mut cursor = Cursor::new(bytes);
        if cursor.take(8)? != LOCKSTEP_EVIDENCE_MAGIC {
            return Err(EvidenceError::BadMagic);
        }
        let version = cursor.u16()?;
        if version != LOCKSTEP_EVIDENCE_VERSION {
            return Err(EvidenceError::UnsupportedVersion(version));
        }
        let flags = cursor.u16()?;
        if flags != 0 {
            return Err(EvidenceError::NonzeroFlags(flags));
        }
        let action_count = cursor.u32()? as usize;
        if action_count > MAX_LOCKSTEP_ACTIONS {
            return Err(EvidenceError::TooManyActions(action_count));
        }
        let outcome_len = cursor.u32()? as usize;
        if outcome_len > MAX_LOCKSTEP_OUTCOME_BYTES {
            return Err(EvidenceError::OutcomeTooLarge(outcome_len));
        }
        let outcome_fnv1a64 = cursor.u64()?;
        let game_key = cursor.u32()?;
        let timeout_ms = cursor.u64()?;
        let mut actions = Vec::with_capacity(action_count);
        for _ in 0..action_count {
            let ty = cursor.u8()?;
            let len = cursor.u32()? as usize;
            let record = cursor.take(len)?;
            actions.push(decode_action(ty, record)?);
        }
        let outcome = cursor.take(outcome_len)?;
        if cursor.remaining() != 0 {
            return Err(EvidenceError::Trailing(cursor.remaining()));
        }
        let outcome_json = core::str::from_utf8(outcome)
            .map_err(|_| EvidenceError::OutcomeNotUtf8)?
            .to_owned();
        let transcript = Self {
            game_key,
            timeout_ms,
            actions,
            outcome_json,
            outcome_fnv1a64,
        };
        transcript.replay()?;
        Ok(transcript)
    }

    pub fn binary_fnv1a64(&self) -> Result<u64, EvidenceError> {
        Ok(fnv1a64(&self.encode()?))
    }

    fn validate_bounds(&self) -> Result<(), EvidenceError> {
        if self.actions.len() > MAX_LOCKSTEP_ACTIONS {
            return Err(EvidenceError::TooManyActions(self.actions.len()));
        }
        if self.outcome_json.len() > MAX_LOCKSTEP_OUTCOME_BYTES {
            return Err(EvidenceError::OutcomeTooLarge(self.outcome_json.len()));
        }
        for action in &self.actions {
            if let ReplayAction::Package { package, .. } = action {
                if package.payload.len() > 512 {
                    return Err(EvidenceError::PackageTooLarge(package.payload.len()));
                }
            }
        }
        Ok(())
    }
}

fn execute(
    game_key: u32,
    timeout_ms: u64,
    actions: &[ReplayAction],
) -> Result<ReplayedLockstep, EvidenceError> {
    if actions.len() > MAX_LOCKSTEP_ACTIONS {
        return Err(EvidenceError::TooManyActions(actions.len()));
    }
    let Some(ReplayAction::Initial {
        at_ms,
        first_stamp,
        members,
    }) = actions.first()
    else {
        return Err(EvidenceError::MissingInitial);
    };
    validate_members(members)?;
    let mut runner = LockstepRunner::new(
        game_key,
        members.iter().map(|member| member.play),
        *first_stamp,
        *at_ms,
        timeout_ms,
    )
    .map_err(|error| EvidenceError::Replay { action: 0, error })?;
    let mut previous_ms = *at_ms;
    for (index, action) in actions.iter().enumerate().skip(1) {
        let at_ms = action.at_ms();
        if at_ms < previous_ms {
            return Err(EvidenceError::NonMonotonicTime {
                action: index,
                previous_ms,
                actual_ms: at_ms,
            });
        }
        previous_ms = at_ms;
        match action {
            ReplayAction::Initial { .. } => return Err(EvidenceError::InitialNotFirst),
            ReplayAction::Epoch { cause, members, .. } => {
                if *cause == EpochCause::Initial {
                    return Err(EvidenceError::InitialCauseAfterStart);
                }
                validate_members(members)?;
                runner
                    .begin_epoch(members.iter().map(|member| member.play), *cause, at_ms)
                    .map_err(|error| EvidenceError::Replay {
                        action: index,
                        error,
                    })?;
            }
            ReplayAction::Package { package, .. } => {
                if package.payload.len() > 512 {
                    return Err(EvidenceError::PackageTooLarge(package.payload.len()));
                }
                runner
                    .submit(package.clone(), at_ms)
                    .map_err(|error| EvidenceError::Replay {
                        action: index,
                        error,
                    })?;
            }
            ReplayAction::ObserveDeadline { .. } => {
                let _ = runner.status(at_ms);
            }
            ReplayAction::Commit { .. } => {
                runner
                    .commit_ready(at_ms)
                    .map_err(|error| EvidenceError::Replay {
                        action: index,
                        error,
                    })?;
            }
        }
    }
    let outcome_json = runner.export_json();
    Ok(ReplayedLockstep {
        outcome_fnv1a64: fnv1a64(outcome_json.as_bytes()),
        outcome_json,
        next_stamp: runner.expected_stamp(),
        final_epoch: runner.epoch(),
        evidence: runner.evidence().to_vec(),
    })
}

fn validate_members(members: &[EpochMember]) -> Result<(), EvidenceError> {
    if members.is_empty()
        || members.len() > 8
        || members
            .iter()
            .any(|member| !(0..8).contains(&member.play) || member.unique_id == 0)
        || members.windows(2).any(|pair| pair[0].play >= pair[1].play)
        || members.iter().enumerate().any(|(index, member)| {
            members[..index]
                .iter()
                .any(|prior| prior.unique_id == member.unique_id)
        })
    {
        return Err(EvidenceError::NoncanonicalMembers);
    }
    Ok(())
}

fn encode_action(action: &ReplayAction, records: &mut Vec<u8>) -> Result<(), EvidenceError> {
    let (ty, payload) = match action {
        ReplayAction::Initial {
            at_ms,
            first_stamp,
            members,
        } => {
            let mut payload = Vec::with_capacity(13 + 5 * members.len());
            payload.extend_from_slice(&at_ms.to_le_bytes());
            payload.extend_from_slice(&first_stamp.to_le_bytes());
            encode_members(members, &mut payload)?;
            (RECORD_INITIAL, payload)
        }
        ReplayAction::Epoch {
            at_ms,
            cause,
            members,
        } => {
            if *cause == EpochCause::Initial {
                return Err(EvidenceError::InitialCauseAfterStart);
            }
            let mut payload = Vec::with_capacity(10 + 5 * members.len());
            payload.extend_from_slice(&at_ms.to_le_bytes());
            payload.push(encode_cause(*cause));
            encode_members(members, &mut payload)?;
            (RECORD_EPOCH, payload)
        }
        ReplayAction::Package { at_ms, package } => {
            if package.payload.len() > 512 {
                return Err(EvidenceError::PackageTooLarge(package.payload.len()));
            }
            let mut payload = Vec::with_capacity(15 + package.payload.len());
            payload.extend_from_slice(&at_ms.to_le_bytes());
            payload.extend_from_slice(&package.stamp.to_le_bytes());
            payload.push(package.play as u8);
            payload.extend_from_slice(&(package.payload.len() as u16).to_le_bytes());
            payload.extend_from_slice(&package.payload);
            (RECORD_PACKAGE, payload)
        }
        ReplayAction::ObserveDeadline { at_ms } => {
            (RECORD_OBSERVE_DEADLINE, at_ms.to_le_bytes().to_vec())
        }
        ReplayAction::Commit { at_ms } => (RECORD_COMMIT, at_ms.to_le_bytes().to_vec()),
    };
    records.push(ty);
    records.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    records.extend_from_slice(&payload);
    Ok(())
}

fn decode_action(ty: u8, bytes: &[u8]) -> Result<ReplayAction, EvidenceError> {
    let mut cursor = Cursor::new(bytes);
    let action = match ty {
        RECORD_INITIAL => {
            if bytes.len() < 13 {
                return Err(EvidenceError::MalformedRecord {
                    ty,
                    len: bytes.len(),
                });
            }
            ReplayAction::Initial {
                at_ms: cursor.u64()?,
                first_stamp: cursor.u32()?,
                members: decode_members(&mut cursor, ty)?,
            }
        }
        RECORD_EPOCH => {
            if bytes.len() < 10 {
                return Err(EvidenceError::MalformedRecord {
                    ty,
                    len: bytes.len(),
                });
            }
            ReplayAction::Epoch {
                at_ms: cursor.u64()?,
                cause: decode_cause(cursor.u8()?)?,
                members: decode_members(&mut cursor, ty)?,
            }
        }
        RECORD_PACKAGE => {
            if bytes.len() < 15 {
                return Err(EvidenceError::MalformedRecord {
                    ty,
                    len: bytes.len(),
                });
            }
            let at_ms = cursor.u64()?;
            let stamp = cursor.u32()?;
            let play = cursor.u8()? as i8;
            let payload_len = cursor.u16()? as usize;
            if payload_len > 512 {
                return Err(EvidenceError::PackageTooLarge(payload_len));
            }
            let payload = cursor.take(payload_len)?.to_vec();
            ReplayAction::Package {
                at_ms,
                package: TurnPackage {
                    stamp,
                    play,
                    payload,
                },
            }
        }
        RECORD_OBSERVE_DEADLINE => {
            if bytes.len() != 8 {
                return Err(EvidenceError::MalformedRecord {
                    ty,
                    len: bytes.len(),
                });
            }
            ReplayAction::ObserveDeadline {
                at_ms: cursor.u64()?,
            }
        }
        RECORD_COMMIT => {
            if bytes.len() != 8 {
                return Err(EvidenceError::MalformedRecord {
                    ty,
                    len: bytes.len(),
                });
            }
            ReplayAction::Commit {
                at_ms: cursor.u64()?,
            }
        }
        _ => return Err(EvidenceError::UnknownRecord(ty)),
    };
    if cursor.remaining() != 0 {
        return Err(EvidenceError::MalformedRecord {
            ty,
            len: bytes.len(),
        });
    }
    Ok(action)
}

fn encode_members(members: &[EpochMember], out: &mut Vec<u8>) -> Result<(), EvidenceError> {
    if members.len() > u8::MAX as usize {
        return Err(EvidenceError::MalformedRecord {
            ty: RECORD_EPOCH,
            len: members.len(),
        });
    }
    out.push(members.len() as u8);
    for member in members {
        out.push(member.play as u8);
        out.extend_from_slice(&member.unique_id.to_le_bytes());
    }
    Ok(())
}

fn decode_members(cursor: &mut Cursor<'_>, ty: u8) -> Result<Vec<EpochMember>, EvidenceError> {
    let count = cursor.u8()? as usize;
    let mut members = Vec::with_capacity(count);
    for _ in 0..count {
        members.push(EpochMember {
            play: cursor.u8()? as i8,
            unique_id: cursor.u32()? as i32,
        });
    }
    if cursor.remaining() != 0 {
        return Err(EvidenceError::MalformedRecord {
            ty,
            len: cursor.bytes.len(),
        });
    }
    Ok(members)
}

fn encode_cause(cause: EpochCause) -> u8 {
    match cause {
        EpochCause::Initial => 0,
        EpochCause::Drop => 1,
        EpochCause::Reconnect => 2,
        EpochCause::RosterChange => 3,
    }
}

fn decode_cause(cause: u8) -> Result<EpochCause, EvidenceError> {
    match cause {
        0 => Ok(EpochCause::Initial),
        1 => Ok(EpochCause::Drop),
        2 => Ok(EpochCause::Reconnect),
        3 => Ok(EpochCause::RosterChange),
        _ => Err(EvidenceError::InvalidEpochCause(cause)),
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.offset)
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], EvidenceError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or(EvidenceError::Truncated {
                need: usize::MAX,
                have: self.remaining(),
            })?;
        if end > self.bytes.len() {
            return Err(EvidenceError::Truncated {
                need: len,
                have: self.remaining(),
            });
        }
        let value = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, EvidenceError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, EvidenceError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, EvidenceError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64, EvidenceError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::obfuscate::xor_payload;
    use crate::{encode_commands, CheckSums, Command, LockstepEvidence, Obfuscation};

    const KEY: u32 = 0x005a_c33d;

    fn words(seed: u32) -> [u32; 16] {
        let mut words = [0u32; 16];
        for (index, word) in words[..15].iter_mut().enumerate() {
            *word = seed.wrapping_add(index as u32);
        }
        words[15] = words[..15]
            .iter()
            .fold(0u32, |sum, word| sum.wrapping_add(*word));
        words
    }

    fn package(stamp: u32, play: i8, checksums: [u32; 16]) -> TurnPackage {
        let mut bytes = vec![0x39];
        for checksum in checksums {
            bytes.extend_from_slice(&checksum.to_le_bytes());
        }
        let command = Command {
            opcode: 0x39,
            bytes: &bytes,
        };
        let mut payload = Vec::new();
        encode_commands(&[command], &mut Obfuscation::multiplayer(KEY), &mut payload);
        xor_payload(&mut payload, Obfuscation::xor_key(KEY));
        TurnPackage {
            stamp,
            play,
            payload,
        }
    }

    fn actions() -> Vec<ReplayAction> {
        vec![
            ReplayAction::Initial {
                at_ms: 1_000,
                first_stamp: 23,
                members: vec![
                    EpochMember {
                        play: 0,
                        unique_id: 1,
                    },
                    EpochMember {
                        play: 1,
                        unique_id: 2,
                    },
                ],
            },
            ReplayAction::Package {
                at_ms: 1_001,
                package: package(23, 0, words(10)),
            },
            ReplayAction::Package {
                at_ms: 1_002,
                package: package(23, 1, words(10)),
            },
            ReplayAction::Commit { at_ms: 1_003 },
            ReplayAction::Package {
                at_ms: 1_004,
                package: package(24, 0, words(20)),
            },
            ReplayAction::Package {
                at_ms: 1_005,
                package: package(24, 1, words(21)),
            },
            ReplayAction::Commit { at_ms: 1_006 },
            ReplayAction::Package {
                at_ms: 1_007,
                package: package(25, 0, words(30)),
            },
            ReplayAction::ObserveDeadline { at_ms: 1_106 },
            ReplayAction::Epoch {
                at_ms: 1_107,
                cause: EpochCause::Drop,
                members: vec![EpochMember {
                    play: 0,
                    unique_id: 1,
                }],
            },
            ReplayAction::Commit { at_ms: 1_108 },
            ReplayAction::Epoch {
                at_ms: 1_109,
                cause: EpochCause::Reconnect,
                members: vec![
                    EpochMember {
                        play: 0,
                        unique_id: 1,
                    },
                    EpochMember {
                        play: 1,
                        unique_id: 2,
                    },
                ],
            },
            ReplayAction::Package {
                at_ms: 1_110,
                package: package(26, 0, words(40)),
            },
            ReplayAction::Package {
                at_ms: 1_111,
                package: package(26, 1, words(40)),
            },
            ReplayAction::Commit { at_ms: 1_112 },
        ]
    }

    #[test]
    fn binary_roundtrip_reexecutes_exact_outcomes() {
        let transcript = PersistedLockstepTranscript::record(KEY, 100, actions()).unwrap();
        let first = transcript.replay().unwrap();
        assert_eq!(first.next_stamp, 27);
        assert_eq!(first.final_epoch, 2);
        let desync = first
            .evidence
            .iter()
            .find_map(|evidence| match evidence {
                LockstepEvidence::Turn(turn) if turn.stamp == 24 => Some(turn),
                _ => None,
            })
            .unwrap();
        assert_eq!(desync.differences.len(), CheckSums([0; 16]).0.len());

        let encoded = transcript.encode().unwrap();
        let decoded = PersistedLockstepTranscript::decode(&encoded).unwrap();
        let second = decoded.replay().unwrap();
        assert_eq!(decoded.encode().unwrap(), encoded);
        assert_eq!(second, first);
        assert_eq!(decoded.outcome_fnv1a64(), first.outcome_fnv1a64);
        assert_eq!(decoded.binary_fnv1a64().unwrap(), fnv1a64(&encoded));
        assert_eq!(decoded.binary_fnv1a64().unwrap(), 0x725c_fd99_ebe6_0b0c);
    }

    #[test]
    fn binary_reader_rejects_mutation_and_ambiguity() {
        let mut unsorted = actions();
        if let ReplayAction::Initial { members, .. } = &mut unsorted[0] {
            members.swap(0, 1);
        }
        assert_eq!(
            PersistedLockstepTranscript::record(KEY, 100, unsorted),
            Err(EvidenceError::NoncanonicalMembers)
        );

        let mut backwards = actions();
        if let ReplayAction::Package { at_ms, .. } = &mut backwards[1] {
            *at_ms = 999;
        }
        assert_eq!(
            PersistedLockstepTranscript::record(KEY, 100, backwards),
            Err(EvidenceError::NonMonotonicTime {
                action: 1,
                previous_ms: 1_000,
                actual_ms: 999,
            })
        );

        let transcript = PersistedLockstepTranscript::record(KEY, 100, actions()).unwrap();
        let encoded = transcript.encode().unwrap();

        let mut noncanonical_members = encoded.clone();
        // Header (40) + TLV header (5) + time/stamp/count/member0 (18)
        // reaches member1.play in the initial record.
        noncanonical_members[63] = 0;
        assert_eq!(
            PersistedLockstepTranscript::decode(&noncanonical_members),
            Err(EvidenceError::NoncanonicalMembers)
        );

        let mut wrong_version = encoded.clone();
        wrong_version[8..10].copy_from_slice(&2u16.to_le_bytes());
        assert_eq!(
            PersistedLockstepTranscript::decode(&wrong_version),
            Err(EvidenceError::UnsupportedVersion(2))
        );

        let mut unknown = encoded.clone();
        unknown[HEADER_LEN] = 99;
        assert_eq!(
            PersistedLockstepTranscript::decode(&unknown),
            Err(EvidenceError::UnknownRecord(99))
        );

        let mut trailing = encoded.clone();
        trailing.push(0);
        assert_eq!(
            PersistedLockstepTranscript::decode(&trailing),
            Err(EvidenceError::Trailing(1))
        );

        let mut changed_outcome = encoded.clone();
        let last = changed_outcome.len() - 1;
        changed_outcome[last] ^= 1;
        assert!(matches!(
            PersistedLockstepTranscript::decode(&changed_outcome),
            Err(EvidenceError::OutcomeHashMismatch { .. })
        ));

        let truncated = &encoded[..encoded.len() - 1];
        assert!(matches!(
            PersistedLockstepTranscript::decode(truncated),
            Err(EvidenceError::Truncated { .. })
        ));
    }
}
