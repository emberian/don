//! Same-frame replay adapter for the canonical Sim-owned Groups channel transition.
//!
//! [`crate::groups_channel`] owns the exact retail `CheckSums::check_groups` traversal,
//! while [`don_sim::tick::Sim::process_command_package`] owns the canonical atomic
//! `GroupCommand` -> `MoveToCommand` mutation.  This module joins those two existing
//! owners.  It does not create another Group pool and it never copies a recorded checksum
//! word into the producer.
//!
//! The replay boundary is intentionally strict.  A source recording must carry real
//! opcode-`0x39` checksum packets and the selected package must contain an immediately
//! adjacent `0x00,0x07` pair followed by its checksum boundary, with no intervening unowned
//! Sim command. The Sim frame must equal the package stamp. Package shell commands
//! such as Camera and PlayerSpeed, and all still-unowned Sim siblings in the same package,
//! are retained as provenance but are not sent through the bounded Sim host. The receipt is
//! therefore an exact *intra-package transition* over a caller-supplied pre-pair Sim state,
//! never a claim that the whole package was executed.
//!
//! This is an exact dynamic owner extension, not a whole-corpus channel installation.
//! Initial Unit/content reconstruction and the other Group action tails are still open,
//! so [`GroupChannelTransitionReceipt::installed_in_scoreboard`] stays false.

#![forbid(unsafe_code)]

use crate::groups_channel::{
    groups_checksum, GroupMembers, GroupRecord, GroupWindow, GroupsChannelError, GroupsChecksum,
    GROUP_SLOTS,
};
use crate::replay::Replay;
use crate::wire::{classify, CommandClass};
use don_net::retail::CHECKSUM_OPCODE;
use don_sim::systems::canonical_group_move_host::{
    GroupMovePackageReceipt, PackageError, GROUP_OPCODE, MOVE_TO_OPCODE,
};
use don_sim::systems::groups_guys::{Groups, GROUP_MAX_MEMBERS};
use don_sim::tick::Sim;
use std::path::PathBuf;

/// A checksum-bearing replay package reduced to the one dynamic Groups cohort owned by Sim.
///
/// Exact command bytes are retained rather than summarized. `inert_opcodes` records the
/// presentation/lockstep shell which is proven not to mutate Groups by command classification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayGroupMoveSource {
    pub replay_path: PathBuf,
    pub replay_payload_len: usize,
    pub replay_stream_start: usize,
    pub replay_xor_key: u16,
    pub turn_index: usize,
    pub player_index: usize,
    pub lockstep_serial: i32,
    pub play: usize,
    pub package_frame: i32,
    pub package_had_checksum: bool,
    pub package_command_count: usize,
    pub pair_command_index: usize,
    pub checksum_command_index: usize,
    pub inert_opcodes: Vec<u8>,
    /// Other Sim commands in this same package, partitioned around the admitted pair.
    /// Their presence is one reason this transition cannot enter the replay scoreboard.
    pub unowned_sim_prefix: Vec<u8>,
    pub unowned_sim_suffix: Vec<u8>,
    group_bytes: Vec<u8>,
    move_bytes: Vec<u8>,
}

impl ReplayGroupMoveSource {
    /// Admit one package directly by its decoded replay coordinates.
    ///
    /// Requiring the complete [`Replay`] prevents a caller from labelling arbitrary byte
    /// vectors "retail corpus".  A non-checksum recording is rejected even if its wire
    /// syntax happens to contain the same command pair.
    pub fn from_replay(
        replay: &Replay,
        turn_index: usize,
        player_index: usize,
    ) -> Result<Self, GroupSimChannelError> {
        if replay.checksum_packets == 0 {
            return Err(GroupSimChannelError::RecordingHasNoChecksums);
        }
        let turn = replay
            .turns
            .get(turn_index)
            .ok_or(GroupSimChannelError::TurnOutOfRange {
                turn_index,
                turns: replay.turns.len(),
            })?;
        let player =
            turn.players
                .get(player_index)
                .ok_or(GroupSimChannelError::PlayerOutOfRange {
                    player_index,
                    players: turn.players.len(),
                })?;
        if player.checksums.is_none() {
            return Err(GroupSimChannelError::PackageHasNoChecksum);
        }
        let play = usize::try_from(player.play)
            .map_err(|_| GroupSimChannelError::PlayOutOfRange { play: player.play })?;
        if play >= 8 {
            return Err(GroupSimChannelError::PlayOutOfRange { play: player.play });
        }
        let package_frame =
            i32::try_from(player.stamp).map_err(|_| GroupSimChannelError::FrameOutOfRange {
                frame: player.stamp,
            })?;

        let Some(pair_command_index) = player
            .commands
            .windows(2)
            .position(|pair| pair[0].opcode == GROUP_OPCODE && pair[1].opcode == MOVE_TO_OPCODE)
        else {
            return Err(GroupSimChannelError::NoGroupMovePair);
        };
        let group_bytes = player.commands[pair_command_index].bytes.clone();
        let move_bytes = player.commands[pair_command_index + 1].bytes.clone();
        let Some(checksum_command_index) = player
            .commands
            .iter()
            .enumerate()
            .skip(pair_command_index + 2)
            .find_map(|(index, command)| (command.opcode == CHECKSUM_OPCODE).then_some(index))
        else {
            return Err(GroupSimChannelError::ChecksumDoesNotFollowPair);
        };
        if let Some((index, command)) = player
            .commands
            .iter()
            .enumerate()
            .take(checksum_command_index)
            .skip(pair_command_index + 2)
            .find(|(_, command)| classify(command.opcode) == CommandClass::Sim)
        {
            return Err(GroupSimChannelError::UnownedSimBeforeChecksum {
                index,
                opcode: command.opcode,
            });
        }
        let mut inert_opcodes = Vec::new();
        let mut unowned_sim_prefix = Vec::new();
        let mut unowned_sim_suffix = Vec::new();
        for (index, command) in player.commands.iter().enumerate() {
            if index == pair_command_index || index == pair_command_index + 1 {
                continue;
            }
            if classify(command.opcode) == CommandClass::Sim {
                if index < pair_command_index {
                    unowned_sim_prefix.push(command.opcode);
                } else {
                    unowned_sim_suffix.push(command.opcode);
                }
            } else {
                inert_opcodes.push(command.opcode);
            }
        }
        if group_bytes.first().copied() != Some(GROUP_OPCODE)
            || move_bytes.first().copied() != Some(MOVE_TO_OPCODE)
        {
            return Err(GroupSimChannelError::DecodedOpcodeByteMismatch);
        }

        Ok(Self {
            replay_path: replay.path.clone(),
            replay_payload_len: replay.payload_len,
            replay_stream_start: replay.stream_start,
            replay_xor_key: replay.xor_key,
            turn_index,
            player_index,
            lockstep_serial: turn.turn,
            play,
            package_frame,
            package_had_checksum: true,
            package_command_count: player.commands.len(),
            pair_command_index,
            checksum_command_index,
            inert_opcodes,
            unowned_sim_prefix,
            unowned_sim_suffix,
            group_bytes,
            move_bytes,
        })
    }

    /// Exact plaintext bytes accepted by the bounded canonical host.
    pub fn command_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.group_bytes.len() + self.move_bytes.len());
        bytes.extend_from_slice(&self.group_bytes);
        bytes.extend_from_slice(&self.move_bytes);
        bytes
    }

    pub fn group_bytes(&self) -> &[u8] {
        &self.group_bytes
    }

    pub fn move_bytes(&self) -> &[u8] {
        &self.move_bytes
    }
}

/// Independently project the canonical Sim pool through the exact don-replay traversal.
///
/// The Sim host's receipt is produced by `groups_guys::Groups::check_groups`; this adapter
/// deliberately uses the separate instruction-derived [`groups_checksum`] implementation.
/// Equality between them therefore catches field-order, member-plane, tail, and byte-count drift.
pub fn sim_groups_checksum(groups: &Groups) -> Result<GroupsChecksum, GroupSimChannelError> {
    if groups.list.len() != GROUP_SLOTS {
        return Err(GroupSimChannelError::WrongGroupSlotCount {
            got: groups.list.len(),
        });
    }
    let records: Vec<GroupRecord<'_>> = groups
        .list
        .iter()
        .map(|group| {
            let n = if (0..=GROUP_MAX_MEMBERS as i32).contains(&group.num) {
                group.num as usize
            } else {
                // Preserve the invalid `num`; the exact walker returns the typed refusal.
                0
            };
            GroupRecord {
                window: GroupWindow {
                    id: group.id,
                    army: group.army,
                    num: group.num,
                    form: group.form,
                    stamp: group.stamp,
                    ox: group.ox,
                    oy: group.oy,
                    o_dist: group.o_dist,
                    o_angle: group.o_angle,
                    disband: group.disband,
                    order_num: group.order_num,
                    priority: group.priority,
                    role: group.role,
                    think_frame: group.think_frame,
                    new_speed: group.new_speed,
                    speed: group.speed,
                    form_num: group.form_num,
                    facing: group.facing,
                    buildings: group.buildings,
                    who: group.who,
                    march: group.march,
                },
                members: GroupMembers {
                    list: &group.list[..n],
                    off_x: &group.off_x[..n],
                    off_y: &group.off_y[..n],
                    curr_x: &group.curr_x[..n],
                    curr_y: &group.curr_y[..n],
                    angles: &group.angles[..n],
                },
            }
        })
        .collect();
    groups_checksum(&records, &groups.last_group).map_err(GroupSimChannelError::Walk)
}

/// Exact before/after evidence for one canonical replay package mutation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupChannelTransitionReceipt {
    pub source: ReplayGroupMoveSource,
    pub before: GroupsChecksum,
    pub after: GroupsChecksum,
    pub host: GroupMovePackageReceipt,
}

impl GroupChannelTransitionReceipt {
    pub fn channel_changed(&self) -> bool {
        self.before.checksum != self.after.checksum
    }

    /// This receipt is an exact owned transition but not a proof that replay setup has
    /// reconstructed every byte which preceded it.
    pub const fn installed_in_scoreboard(&self) -> bool {
        false
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GroupSimChannelError {
    RecordingHasNoChecksums,
    PackageHasNoChecksum,
    TurnOutOfRange {
        turn_index: usize,
        turns: usize,
    },
    PlayerOutOfRange {
        player_index: usize,
        players: usize,
    },
    PlayOutOfRange {
        play: i32,
    },
    FrameOutOfRange {
        frame: u32,
    },
    NoGroupMovePair,
    ChecksumDoesNotFollowPair,
    UnownedSimBeforeChecksum {
        index: usize,
        opcode: u8,
    },
    DecodedOpcodeByteMismatch,
    WrongGroupSlotCount {
        got: usize,
    },
    FrameMismatch {
        sim_frame: i32,
        package_frame: i32,
    },
    Walk(GroupsChannelError),
    Host(PackageError),
    PostCommitReceiptMismatch {
        receipt_checksum: u32,
        projected_checksum: u32,
    },
    PostCommitFrameMismatch {
        receipt_frame: i32,
        package_frame: i32,
    },
    PostCommitSerialMismatch {
        receipt_serial: i32,
        replay_serial: i32,
    },
    PostCommitPlayMismatch {
        receipt_play: usize,
        replay_play: usize,
    },
    PostCommitRandomAdvanced {
        before: i32,
        after: i32,
    },
}

impl std::fmt::Display for GroupSimChannelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Groups Sim-channel transition refused: {self:?}")
    }
}

impl std::error::Error for GroupSimChannelError {}

/// Execute one replay-derived Group -> Move package at its exact package frame.
pub fn issue_replay_group_move(
    sim: &mut Sim,
    source: &ReplayGroupMoveSource,
) -> Result<GroupChannelTransitionReceipt, GroupSimChannelError> {
    if sim.world.frame != source.package_frame {
        return Err(GroupSimChannelError::FrameMismatch {
            sim_frame: sim.world.frame,
            package_frame: source.package_frame,
        });
    }
    let before = sim_groups_checksum(&sim.groups)?;
    let host = sim
        .process_command_package(source.play, source.lockstep_serial, &source.command_bytes())
        .map_err(GroupSimChannelError::Host)?;
    let after = sim_groups_checksum(&sim.groups)?;
    if host.groups_checksum != after.checksum {
        return Err(GroupSimChannelError::PostCommitReceiptMismatch {
            receipt_checksum: host.groups_checksum,
            projected_checksum: after.checksum,
        });
    }
    if host.frame != source.package_frame {
        return Err(GroupSimChannelError::PostCommitFrameMismatch {
            receipt_frame: host.frame,
            package_frame: source.package_frame,
        });
    }
    if host.lockstep_serial != source.lockstep_serial {
        return Err(GroupSimChannelError::PostCommitSerialMismatch {
            receipt_serial: host.lockstep_serial,
            replay_serial: source.lockstep_serial,
        });
    }
    if host.play != source.play {
        return Err(GroupSimChannelError::PostCommitPlayMismatch {
            receipt_play: host.play,
            replay_play: source.play,
        });
    }
    if host.random_state_before != host.random_state_after {
        return Err(GroupSimChannelError::PostCommitRandomAdvanced {
            before: host.random_state_before,
            after: host.random_state_after,
        });
    }
    Ok(GroupChannelTransitionReceipt {
        source: source.clone(),
        before,
        after,
        host,
    })
}
