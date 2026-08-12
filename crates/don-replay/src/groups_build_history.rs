//! Strict retail history required before the 2018 same-frame Groups checksum witness.
//!
//! This source owns chronology and provenance only.  It extracts exact adjacent
//! `GroupCommand`/`QueueUpBuildCommand` pairs from a checksum-bearing [`Replay`], retains
//! the package checksum, and refuses every unowned simulation command.  It does not
//! execute `Group::action_build`, construct a Build, or install a checksum producer.

#![forbid(unsafe_code)]

use crate::checksum::Channel;
use crate::replay::Replay;
use crate::wire::{classify, CommandClass};
use don_net::retail::CHECKSUM_OPCODE;
use don_sim::systems::canonical_group_move_host::GROUP_OPCODE;
use std::path::PathBuf;

pub const QUEUE_UP_BUILD_OPCODE: u8 = 25;
pub const QUEUE_UP_BUILD_WIRE_SIZE: usize = 25;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueUpBuildWire {
    pub x: i32,
    pub y: i32,
    pub x2: i32,
    pub y2: i32,
    pub type_index: i32,
    pub queued: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayGroupBuildPackage {
    pub replay_path: PathBuf,
    pub lockstep_serial: i32,
    pub package_frame: i32,
    pub play: usize,
    pub command_index: usize,
    pub who: u8,
    pub objects: Vec<i16>,
    pub action: QueueUpBuildWire,
    pub groups_checksum: u32,
    pub units_checksum: u32,
    /// Exact package shell before the pair. These commands are retained for the outer package
    /// router; in particular PlayerSpeed is not declared inert by this owner.
    pub shell_prefix: Vec<u8>,
    /// Exact package shell after the pair, excluding the checksum command itself.
    pub shell_suffix: Vec<u8>,
    group_bytes: Vec<u8>,
    action_bytes: Vec<u8>,
}

impl ReplayGroupBuildPackage {
    pub fn command_bytes(&self) -> Vec<u8> {
        let mut bytes = self.group_bytes.clone();
        bytes.extend_from_slice(&self.action_bytes);
        bytes
    }

    pub fn group_bytes(&self) -> &[u8] {
        &self.group_bytes
    }

    pub fn action_bytes(&self) -> &[u8] {
        &self.action_bytes
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GroupBuildHistoryError {
    RecordingHasNoChecksums,
    FrameOutOfRange { frame: u32 },
    PlayOutOfRange { play: i32 },
    TruncatedGroup,
    NegativeObject { o: i16 },
    WrongActionSize { got: usize },
    MissingPackageChecksum,
    UnownedSimCommand { opcode: u8 },
}

const PLAYER_SPEED_OPCODE: u8 = 0x4f;
const TURN_DATA_OPCODE: u8 = 0x4a;
const CAMERA_OPCODE: u8 = 0x48;

fn retained_package_shell(opcode: u8) -> bool {
    matches!(
        opcode,
        PLAYER_SPEED_OPCODE | TURN_DATA_OPCODE | CAMERA_OPCODE
    )
}

fn i32_at(bytes: &[u8], at: usize) -> Option<i32> {
    Some(i32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn i16_at(bytes: &[u8], at: usize) -> Option<i16> {
    Some(i16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

pub fn decode_queue_up_build(bytes: &[u8]) -> Result<QueueUpBuildWire, GroupBuildHistoryError> {
    if bytes.len() != QUEUE_UP_BUILD_WIRE_SIZE {
        return Err(GroupBuildHistoryError::WrongActionSize { got: bytes.len() });
    }
    if bytes[0] != QUEUE_UP_BUILD_OPCODE {
        return Err(GroupBuildHistoryError::UnownedSimCommand { opcode: bytes[0] });
    }
    Ok(QueueUpBuildWire {
        x: i32_at(bytes, 1).expect("fixed QueueUpBuild x"),
        y: i32_at(bytes, 5).expect("fixed QueueUpBuild y"),
        x2: i32_at(bytes, 9).expect("fixed QueueUpBuild x2"),
        y2: i32_at(bytes, 13).expect("fixed QueueUpBuild y2"),
        type_index: i32_at(bytes, 17).expect("fixed QueueUpBuild type"),
        queued: i32_at(bytes, 21).expect("fixed QueueUpBuild queue"),
    })
}

/// Extract every exact Group+Build package strictly before `before_serial`.
///
/// Presentation/admin commands may surround the pair, but a second simulation command is
/// refused instead of discarded.  The checksum is read from the same package.
pub fn strict_group_build_history_before(
    replay: &Replay,
    before_serial: i32,
) -> Result<Vec<ReplayGroupBuildPackage>, GroupBuildHistoryError> {
    if replay.checksum_packets == 0 {
        return Err(GroupBuildHistoryError::RecordingHasNoChecksums);
    }
    let mut history = Vec::new();
    for turn in replay.turns.iter().filter(|turn| turn.turn < before_serial) {
        for player in &turn.players {
            let frame = i32::try_from(player.stamp).map_err(|_| {
                GroupBuildHistoryError::FrameOutOfRange {
                    frame: player.stamp,
                }
            })?;
            let play = usize::try_from(player.play)
                .ok()
                .filter(|play| *play < 8)
                .ok_or(GroupBuildHistoryError::PlayOutOfRange { play: player.play })?;
            for (command_index, pair) in player.commands.windows(2).enumerate() {
                if pair[0].opcode != GROUP_OPCODE || pair[1].opcode != QUEUE_UP_BUILD_OPCODE {
                    continue;
                }
                let group = &pair[0].bytes;
                let count =
                    usize::from(*group.get(1).ok_or(GroupBuildHistoryError::TruncatedGroup)?);
                let who = *group.get(2).ok_or(GroupBuildHistoryError::TruncatedGroup)?;
                if group.len() != 3 + count * 2 {
                    return Err(GroupBuildHistoryError::TruncatedGroup);
                }
                let mut objects = Vec::with_capacity(count);
                for index in 0..count {
                    let o = i16_at(group, 3 + index * 2)
                        .ok_or(GroupBuildHistoryError::TruncatedGroup)?;
                    if o < 0 {
                        return Err(GroupBuildHistoryError::NegativeObject { o });
                    }
                    objects.push(o);
                }
                let action = decode_queue_up_build(&pair[1].bytes)?;
                let checksums = player
                    .checksums
                    .ok_or(GroupBuildHistoryError::MissingPackageChecksum)?;
                let mut shell_prefix = Vec::new();
                let mut shell_suffix = Vec::new();
                for (index, command) in player.commands.iter().enumerate() {
                    if index == command_index
                        || index == command_index + 1
                        || command.opcode == CHECKSUM_OPCODE
                    {
                        continue;
                    }
                    // TurnData and PlayerSpeed are conservatively classified Sim by the generic
                    // wire layer. They are explicit, retained package-shell responsibilities
                    // here, not discarded inert commands. Every other Sim sibling is refused.
                    if classify(command.opcode) == CommandClass::Sim
                        && !retained_package_shell(command.opcode)
                    {
                        return Err(GroupBuildHistoryError::UnownedSimCommand {
                            opcode: command.opcode,
                        });
                    }
                    if index < command_index {
                        shell_prefix.push(command.opcode);
                    } else {
                        shell_suffix.push(command.opcode);
                    }
                }
                history.push(ReplayGroupBuildPackage {
                    replay_path: replay.path.clone(),
                    lockstep_serial: turn.turn,
                    package_frame: frame,
                    play,
                    command_index,
                    who,
                    objects,
                    action,
                    groups_checksum: checksums.get(Channel::Groups),
                    units_checksum: checksums.get(Channel::Units),
                    shell_prefix,
                    shell_suffix,
                    group_bytes: pair[0].bytes.clone(),
                    action_bytes: pair[1].bytes.clone(),
                });
            }
        }
    }
    Ok(history)
}
