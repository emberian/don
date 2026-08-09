//! Authoritative turn progression above [`Session`](crate::session::Session).
//!
//! `Session` owns packet delivery and stores packages by `(stamp, slot)`. This
//! module owns the next boundary: one expected stamp, one package per current
//! participant, retail checksum comparison, bounded deadline evidence, and
//! explicit membership epochs. It never runs simulation commands.

use crate::internal::MAX_PLAYERS;
use crate::retail::{decode_retail_checksum_package, RetailChecksumError};
use crate::session::TurnPackage;
use crate::{CheckSums, CHECKSUM_CHANNELS};
use std::collections::BTreeMap;
use std::fmt::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpochCause {
    Initial,
    Drop,
    Reconnect,
    RosterChange,
}

impl EpochCause {
    fn name(self) -> &'static str {
        match self {
            Self::Initial => "initial",
            Self::Drop => "drop",
            Self::Reconnect => "reconnect",
            Self::RosterChange => "roster-change",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChecksumDifference {
    pub channel: usize,
    pub reference_play: i8,
    pub reference: u32,
    pub divergent_play: i8,
    pub divergent: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageEvidence {
    pub play: i8,
    pub payload_bytes: usize,
    pub payload_fnv1a64: u64,
    pub checksums: CheckSums,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnEvidence {
    pub epoch: u32,
    pub stamp: u32,
    pub packages: Vec<PackageEvidence>,
    pub differences: Vec<ChecksumDifference>,
}

impl TurnEvidence {
    pub fn checksums_agree(&self) -> bool {
        self.differences.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnTimeoutEvidence {
    pub epoch: u32,
    pub stamp: u32,
    pub deadline_ms: u64,
    pub observed_ms: u64,
    pub received: Vec<i8>,
    pub missing: Vec<i8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockstepEvidence {
    Epoch {
        epoch: u32,
        cause: EpochCause,
        stamp: u32,
        participants: Vec<i8>,
    },
    Timeout(TurnTimeoutEvidence),
    Turn(TurnEvidence),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockstepStatus {
    Waiting {
        epoch: u32,
        stamp: u32,
        deadline_ms: u64,
        missing: Vec<i8>,
    },
    Ready {
        epoch: u32,
        stamp: u32,
        plays: Vec<i8>,
    },
    TimedOut(TurnTimeoutEvidence),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitOutcome {
    Accepted { missing: Vec<i8> },
    Duplicate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockstepError {
    NoParticipants,
    InvalidParticipant(i8),
    DuplicateParticipant(i8),
    ZeroTimeout,
    UnexpectedStamp { expected: u32, actual: u32 },
    UnexpectedPlay(i8),
    DuplicateConflict { stamp: u32, play: i8 },
    Checksum(RetailChecksumError),
    TurnTimedOut(TurnTimeoutEvidence),
    TurnNotReady { stamp: u32, missing: Vec<i8> },
    EpochOverflow,
    StampOverflow,
}

impl core::fmt::Display for LockstepError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoParticipants => write!(f, "lockstep roster has no participants"),
            Self::InvalidParticipant(play) => write!(f, "invalid lockstep slot {play}"),
            Self::DuplicateParticipant(play) => write!(f, "duplicate lockstep slot {play}"),
            Self::ZeroTimeout => write!(f, "lockstep timeout must be nonzero"),
            Self::UnexpectedStamp { expected, actual } => {
                write!(f, "expected stamp {expected}, received {actual}")
            }
            Self::UnexpectedPlay(play) => write!(f, "slot {play} is not in the current epoch"),
            Self::DuplicateConflict { stamp, play } => {
                write!(f, "conflicting duplicate for stamp {stamp}, slot {play}")
            }
            Self::Checksum(error) => write!(f, "checksum package refused: {error}"),
            Self::TurnTimedOut(timeout) => write!(
                f,
                "stamp {} timed out at {} ms with slots {:?} missing",
                timeout.stamp, timeout.deadline_ms, timeout.missing
            ),
            Self::TurnNotReady { stamp, missing } => {
                write!(f, "stamp {stamp} is missing slots {missing:?}")
            }
            Self::EpochOverflow => write!(f, "membership epoch overflow"),
            Self::StampOverflow => write!(f, "authoritative stamp overflow"),
        }
    }
}

impl std::error::Error for LockstepError {}

#[derive(Debug, Clone)]
struct PendingPackage {
    package: TurnPackage,
    checksums: CheckSums,
}

/// One deterministic lockstep clock. Membership changes are explicit and do
/// not reset `expected_stamp`, so reconnects continue the authoritative clock.
#[derive(Debug, Clone)]
pub struct LockstepRunner {
    game_key: u32,
    timeout_ms: u64,
    epoch: u32,
    expected_stamp: u32,
    deadline_ms: u64,
    participants: Vec<i8>,
    pending: BTreeMap<i8, PendingPackage>,
    timed_out: Option<TurnTimeoutEvidence>,
    evidence: Vec<LockstepEvidence>,
}

impl LockstepRunner {
    pub fn new(
        game_key: u32,
        participants: impl IntoIterator<Item = i8>,
        first_stamp: u32,
        now_ms: u64,
        timeout_ms: u64,
    ) -> Result<Self, LockstepError> {
        if timeout_ms == 0 {
            return Err(LockstepError::ZeroTimeout);
        }
        let participants = normalize_participants(participants)?;
        let mut runner = Self {
            game_key,
            timeout_ms,
            epoch: 0,
            expected_stamp: first_stamp,
            deadline_ms: now_ms.saturating_add(timeout_ms),
            participants: participants.clone(),
            pending: BTreeMap::new(),
            timed_out: None,
            evidence: Vec::new(),
        };
        runner.evidence.push(LockstepEvidence::Epoch {
            epoch: 0,
            cause: EpochCause::Initial,
            stamp: first_stamp,
            participants,
        });
        Ok(runner)
    }

    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    pub fn expected_stamp(&self) -> u32 {
        self.expected_stamp
    }

    pub fn participants(&self) -> &[i8] {
        &self.participants
    }

    pub fn evidence(&self) -> &[LockstepEvidence] {
        &self.evidence
    }

    pub fn begin_epoch(
        &mut self,
        participants: impl IntoIterator<Item = i8>,
        cause: EpochCause,
        now_ms: u64,
    ) -> Result<u32, LockstepError> {
        let participants = normalize_participants(participants)?;
        self.epoch = self
            .epoch
            .checked_add(1)
            .ok_or(LockstepError::EpochOverflow)?;
        self.pending
            .retain(|play, _| participants.binary_search(play).is_ok());
        self.participants = participants.clone();
        self.timed_out = None;
        self.deadline_ms = now_ms.saturating_add(self.timeout_ms);
        self.evidence.push(LockstepEvidence::Epoch {
            epoch: self.epoch,
            cause,
            stamp: self.expected_stamp,
            participants,
        });
        Ok(self.epoch)
    }

    pub fn submit(
        &mut self,
        package: TurnPackage,
        now_ms: u64,
    ) -> Result<SubmitOutcome, LockstepError> {
        self.observe_timeout(now_ms);
        if let Some(timeout) = &self.timed_out {
            return Err(LockstepError::TurnTimedOut(timeout.clone()));
        }
        if package.stamp != self.expected_stamp {
            return Err(LockstepError::UnexpectedStamp {
                expected: self.expected_stamp,
                actual: package.stamp,
            });
        }
        if self.participants.binary_search(&package.play).is_err() {
            return Err(LockstepError::UnexpectedPlay(package.play));
        }
        if let Some(existing) = self.pending.get(&package.play) {
            return if existing.package == package {
                Ok(SubmitOutcome::Duplicate)
            } else {
                Err(LockstepError::DuplicateConflict {
                    stamp: package.stamp,
                    play: package.play,
                })
            };
        }
        let decoded = decode_retail_checksum_package(&package, self.game_key)
            .map_err(LockstepError::Checksum)?;
        self.pending.insert(
            package.play,
            PendingPackage {
                package,
                checksums: decoded.checksums,
            },
        );
        Ok(SubmitOutcome::Accepted {
            missing: self.missing(),
        })
    }

    pub fn status(&mut self, now_ms: u64) -> LockstepStatus {
        if self.pending.len() == self.participants.len() {
            return LockstepStatus::Ready {
                epoch: self.epoch,
                stamp: self.expected_stamp,
                plays: self.pending.keys().copied().collect(),
            };
        }
        self.observe_timeout(now_ms);
        if let Some(timeout) = &self.timed_out {
            LockstepStatus::TimedOut(timeout.clone())
        } else {
            LockstepStatus::Waiting {
                epoch: self.epoch,
                stamp: self.expected_stamp,
                deadline_ms: self.deadline_ms,
                missing: self.missing(),
            }
        }
    }

    pub fn commit_ready(&mut self, now_ms: u64) -> Result<TurnEvidence, LockstepError> {
        self.observe_timeout(now_ms);
        if let Some(timeout) = &self.timed_out {
            return Err(LockstepError::TurnTimedOut(timeout.clone()));
        }
        let missing = self.missing();
        if !missing.is_empty() {
            return Err(LockstepError::TurnNotReady {
                stamp: self.expected_stamp,
                missing,
            });
        }
        let next_stamp = self
            .expected_stamp
            .checked_add(1)
            .ok_or(LockstepError::StampOverflow)?;
        let reference = *self.pending.keys().next().expect("nonempty participants");
        let reference_checksums = self.pending[&reference].checksums;
        let mut differences = Vec::new();
        for (&play, pending) in &self.pending {
            if play == reference {
                continue;
            }
            for channel in 0..CHECKSUM_CHANNELS.len() {
                let expected = reference_checksums.0[channel];
                let actual = pending.checksums.0[channel];
                if expected != actual {
                    differences.push(ChecksumDifference {
                        channel,
                        reference_play: reference,
                        reference: expected,
                        divergent_play: play,
                        divergent: actual,
                    });
                }
            }
        }
        let packages = self
            .pending
            .values()
            .map(|pending| PackageEvidence {
                play: pending.package.play,
                payload_bytes: pending.package.payload.len(),
                payload_fnv1a64: fnv1a64(&pending.package.payload),
                checksums: pending.checksums,
            })
            .collect();
        let turn = TurnEvidence {
            epoch: self.epoch,
            stamp: self.expected_stamp,
            packages,
            differences,
        };
        self.evidence.push(LockstepEvidence::Turn(turn.clone()));
        self.pending.clear();
        self.timed_out = None;
        self.expected_stamp = next_stamp;
        self.deadline_ms = now_ms.saturating_add(self.timeout_ms);
        Ok(turn)
    }

    /// Canonical JSON with fixed field order and sorted slot/channel arrays.
    pub fn export_json(&self) -> String {
        let mut out = String::new();
        write!(
            out,
            "{{\"schema\":\"don.lockstep-transcript.v1\",\"timeout_ms\":{},\"next_stamp\":{},\"events\":[",
            self.timeout_ms, self.expected_stamp
        )
        .unwrap();
        for (event_index, event) in self.evidence.iter().enumerate() {
            if event_index != 0 {
                out.push(',');
            }
            match event {
                LockstepEvidence::Epoch {
                    epoch,
                    cause,
                    stamp,
                    participants,
                } => {
                    write!(
                        out,
                        "{{\"type\":\"epoch\",\"epoch\":{epoch},\"cause\":\"{}\",\"stamp\":{stamp},\"participants\":",
                        cause.name()
                    )
                    .unwrap();
                    write_i8_array(&mut out, participants);
                    out.push('}');
                }
                LockstepEvidence::Timeout(timeout) => {
                    write!(
                        out,
                        "{{\"type\":\"timeout\",\"epoch\":{},\"stamp\":{},\"deadline_ms\":{},\"observed_ms\":{},\"received\":",
                        timeout.epoch,
                        timeout.stamp,
                        timeout.deadline_ms,
                        timeout.observed_ms
                    )
                    .unwrap();
                    write_i8_array(&mut out, &timeout.received);
                    out.push_str(",\"missing\":");
                    write_i8_array(&mut out, &timeout.missing);
                    out.push('}');
                }
                LockstepEvidence::Turn(turn) => write_turn_json(&mut out, turn),
            }
        }
        out.push_str("]}");
        out
    }

    pub fn transcript_fnv1a64(&self) -> u64 {
        fnv1a64(self.export_json().as_bytes())
    }

    fn missing(&self) -> Vec<i8> {
        self.participants
            .iter()
            .copied()
            .filter(|play| !self.pending.contains_key(play))
            .collect()
    }

    fn observe_timeout(&mut self, now_ms: u64) {
        if self.timed_out.is_some()
            || self.pending.len() == self.participants.len()
            || now_ms < self.deadline_ms
        {
            return;
        }
        let timeout = TurnTimeoutEvidence {
            epoch: self.epoch,
            stamp: self.expected_stamp,
            deadline_ms: self.deadline_ms,
            observed_ms: now_ms,
            received: self.pending.keys().copied().collect(),
            missing: self.missing(),
        };
        self.evidence
            .push(LockstepEvidence::Timeout(timeout.clone()));
        self.timed_out = Some(timeout);
    }
}

fn normalize_participants(
    participants: impl IntoIterator<Item = i8>,
) -> Result<Vec<i8>, LockstepError> {
    let mut participants: Vec<i8> = participants.into_iter().collect();
    if participants.is_empty() {
        return Err(LockstepError::NoParticipants);
    }
    participants.sort_unstable();
    for (index, &play) in participants.iter().enumerate() {
        if play < 0 || play as usize >= MAX_PLAYERS {
            return Err(LockstepError::InvalidParticipant(play));
        }
        if index != 0 && participants[index - 1] == play {
            return Err(LockstepError::DuplicateParticipant(play));
        }
    }
    Ok(participants)
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

fn write_i8_array(out: &mut String, values: &[i8]) {
    out.push('[');
    for (index, value) in values.iter().enumerate() {
        if index != 0 {
            out.push(',');
        }
        write!(out, "{value}").unwrap();
    }
    out.push(']');
}

fn write_turn_json(out: &mut String, turn: &TurnEvidence) {
    write!(
        out,
        "{{\"type\":\"turn\",\"epoch\":{},\"stamp\":{},\"checksum_agreement\":{},\"packages\":[",
        turn.epoch,
        turn.stamp,
        turn.checksums_agree()
    )
    .unwrap();
    for (package_index, package) in turn.packages.iter().enumerate() {
        if package_index != 0 {
            out.push(',');
        }
        write!(
            out,
            "{{\"play\":{},\"payload_bytes\":{},\"payload_fnv1a64\":\"{:016x}\",\"checksums\":[",
            package.play, package.payload_bytes, package.payload_fnv1a64
        )
        .unwrap();
        for (channel, checksum) in package.checksums.0.iter().enumerate() {
            if channel != 0 {
                out.push(',');
            }
            write!(out, "\"{checksum:08x}\"").unwrap();
        }
        out.push_str("]}");
    }
    out.push_str("],\"desync\":[");
    for (difference_index, difference) in turn.differences.iter().enumerate() {
        if difference_index != 0 {
            out.push(',');
        }
        write!(
            out,
            "{{\"channel\":\"{}\",\"index\":{},\"reference_play\":{},\"reference\":\"{:08x}\",\"divergent_play\":{},\"divergent\":\"{:08x}\"}}",
            CHECKSUM_CHANNELS[difference.channel],
            difference.channel,
            difference.reference_play,
            difference.reference,
            difference.divergent_play,
            difference.divergent
        )
        .unwrap();
    }
    out.push_str("]}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::obfuscate::xor_payload;
    use crate::{Command, Obfuscation};

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
        crate::encode_commands(&[command], &mut Obfuscation::multiplayer(KEY), &mut payload);
        xor_payload(&mut payload, Obfuscation::xor_key(KEY));
        TurnPackage {
            stamp,
            play,
            payload,
        }
    }

    #[test]
    fn authority_desync_timeout_drop_and_reconnect_are_evidentiary() {
        let mut runner = LockstepRunner::new(KEY, [1, 0], 23, 1_000, 100).unwrap();
        assert_eq!(runner.participants(), [0, 1]);
        assert_eq!(
            runner.status(1_000),
            LockstepStatus::Waiting {
                epoch: 0,
                stamp: 23,
                deadline_ms: 1_100,
                missing: vec![0, 1],
            }
        );

        let first = package(23, 0, words(10));
        assert_eq!(
            runner.submit(first.clone(), 1_050).unwrap(),
            SubmitOutcome::Accepted { missing: vec![1] }
        );
        assert_eq!(
            runner.submit(first.clone(), 1_051).unwrap(),
            SubmitOutcome::Duplicate
        );
        let mut conflict = first;
        conflict.payload[0] ^= 1;
        assert_eq!(
            runner.submit(conflict, 1_052),
            Err(LockstepError::DuplicateConflict { stamp: 23, play: 0 })
        );
        assert_eq!(
            runner.submit(package(24, 1, words(10)), 1_053),
            Err(LockstepError::UnexpectedStamp {
                expected: 23,
                actual: 24,
            })
        );
        let mut malformed = package(23, 1, words(10));
        malformed.payload[0] ^= 0xff;
        assert!(matches!(
            runner.submit(malformed, 1_053),
            Err(LockstepError::Checksum(_))
        ));
        runner.submit(package(23, 1, words(10)), 1_054).unwrap();
        assert!(matches!(runner.status(1_054), LockstepStatus::Ready { .. }));
        let agreed = runner.commit_ready(1_054).unwrap();
        assert!(agreed.checksums_agree());
        assert_eq!(runner.expected_stamp(), 24);
        assert_eq!(
            runner.submit(package(23, 0, words(10)), 1_055),
            Err(LockstepError::UnexpectedStamp {
                expected: 24,
                actual: 23,
            })
        );

        runner.submit(package(24, 0, words(20)), 1_060).unwrap();
        runner.submit(package(24, 1, words(21)), 1_061).unwrap();
        let desync = runner.commit_ready(1_061).unwrap();
        assert!(!desync.checksums_agree());
        assert_eq!(desync.differences.len(), 16);
        assert_eq!(desync.differences[0].channel, 0);
        assert_eq!(desync.differences[15].channel, 15);

        runner.submit(package(25, 0, words(30)), 1_070).unwrap();
        let timeout = match runner.status(1_162) {
            LockstepStatus::TimedOut(timeout) => timeout,
            other => panic!("expected timeout, got {other:?}"),
        };
        assert_eq!(timeout.received, [0]);
        assert_eq!(timeout.missing, [1]);
        assert_eq!(
            runner.submit(package(25, 1, words(30)), 1_163),
            Err(LockstepError::TurnTimedOut(timeout.clone()))
        );
        assert_eq!(
            runner.commit_ready(1_163),
            Err(LockstepError::TurnTimedOut(timeout))
        );

        runner.begin_epoch([0], EpochCause::Drop, 1_164).unwrap();
        assert!(matches!(runner.status(1_164), LockstepStatus::Ready { .. }));
        runner.commit_ready(1_164).unwrap();
        assert_eq!(runner.expected_stamp(), 26);

        runner
            .begin_epoch([0, 1], EpochCause::Reconnect, 1_165)
            .unwrap();
        runner.submit(package(26, 0, words(40)), 1_166).unwrap();
        runner.submit(package(26, 1, words(40)), 1_167).unwrap();
        assert!(runner.commit_ready(1_167).unwrap().checksums_agree());
        assert_eq!(runner.expected_stamp(), 27);

        let json = runner.export_json();
        assert!(json.contains("\"cause\":\"drop\""));
        assert!(json.contains("\"cause\":\"reconnect\""));
        assert!(json.contains("\"type\":\"timeout\""));
        assert!(json.contains("\"channel\":\"units\""));
        assert_eq!(runner.transcript_fnv1a64(), fnv1a64(json.as_bytes()));
        assert_eq!(runner.transcript_fnv1a64(), 0xf4cb_bb49_3b3a_6d3e);
    }
}
