// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact decoded-package shell around one canonical LaunchPatrol/Scramble pair.
//!
//! Replay delivery has already removed command padding and exposes one byte-exact slice per
//! command. This adapter retains the complete chronology, decodes the bounded retail shell
//! commands surrounding the air pair, and commits only the landed canonical Group/air
//! transaction. Shell state remains an explicit receipt rather than being copied into the
//! legacy `command::Bridge` shadow owners.

use crate::systems::air_group_action_transaction::{
    AirGroupActionReceipt, CommandPackagePosition, LAUNCH_PATROL_OPCODE, SCRAMBLE_OPCODE,
};
use crate::systems::air_runtime_authority::ScenarioIgnoreOrdersAuthority;
use crate::systems::canonical_air_group_host::{
    commit_canonical_air_package, prepare_canonical_air_packet_pair, AirGroupRuntimeAuthority,
    CanonicalAirPackageError, PreparedCanonicalAirPackage,
};
use crate::systems::canonical_group_move_host::{
    CommandPackageState, GroupMoveAuthority, NETWORK_PLAYERS,
};
use crate::systems::groups_guys::Groups;
use crate::systems::movement::PathStack;
use crate::systems::production::BuildData;
use crate::world::World;

pub const CHECK_SUMS_OPCODE: u8 = 57;
pub const NEXT_CHECK_SUM_OPCODE: u8 = 58;
pub const CAMERA_OPCODE: u8 = 72;
pub const TURN_DATA_OPCODE: u8 = 74;
pub const PLAYER_SPEED_OPCODE: u8 = 79;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AirReplayShellCommand {
    CheckSums {
        index: u16,
        words: [u32; 16],
    },
    NextCheckSum {
        index: u16,
        kind: u8,
        value: u32,
    },
    Camera {
        index: u16,
        zoom: u8,
        x: i32,
        y: i32,
    },
    /// Exact ten-byte TurnData payload. Its separate TurnControl owner is not duplicated here.
    TurnData {
        index: u16,
        payload: [u8; 10],
    },
    PlayerSpeed {
        index: u16,
        deltas: [u8; 8],
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalAirPackageShellError {
    TooManyCommands(usize),
    PositionOutsidePackage {
        index: u16,
        commands: usize,
    },
    NonAdjacentAirPair {
        group_command_index: u16,
        action_command_index: u16,
    },
    EmptyCommand {
        index: u16,
    },
    WrongWireSize {
        index: u16,
        opcode: u8,
        expected: usize,
        actual: usize,
    },
    UnsupportedShellOpcode {
        index: u16,
        opcode: u8,
    },
    UnexpectedAirCommand {
        index: u16,
        opcode: u8,
    },
    Air(CanonicalAirPackageError),
    StaleCommandImage,
}

impl From<CanonicalAirPackageError> for CanonicalAirPackageShellError {
    fn from(error: CanonicalAirPackageError) -> Self {
        Self::Air(error)
    }
}

#[derive(Clone, Debug)]
pub struct PreparedCanonicalAirReplayPackage {
    position: CommandPackagePosition,
    commands_before: Vec<Vec<u8>>,
    shell: Vec<AirReplayShellCommand>,
    air: PreparedCanonicalAirPackage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalAirReplayPackageReceipt {
    pub position: CommandPackagePosition,
    pub command_image: Vec<Vec<u8>>,
    pub shell: Vec<AirReplayShellCommand>,
    pub air: AirGroupActionReceipt,
}

fn require_size(
    index: u16,
    command: &[u8],
    expected: usize,
) -> Result<(), CanonicalAirPackageShellError> {
    if command.len() != expected {
        return Err(CanonicalAirPackageShellError::WrongWireSize {
            index,
            opcode: command[0],
            expected,
            actual: command.len(),
        });
    }
    Ok(())
}

fn parse_shell(
    index: u16,
    command: &[u8],
) -> Result<AirReplayShellCommand, CanonicalAirPackageShellError> {
    let Some(&opcode) = command.first() else {
        return Err(CanonicalAirPackageShellError::EmptyCommand { index });
    };
    if matches!(opcode, 0 | LAUNCH_PATROL_OPCODE | SCRAMBLE_OPCODE) {
        return Err(CanonicalAirPackageShellError::UnexpectedAirCommand { index, opcode });
    }
    match opcode {
        CHECK_SUMS_OPCODE => {
            require_size(index, command, 65)?;
            let mut words = [0; 16];
            for (word, bytes) in words.iter_mut().zip(command[1..].chunks_exact(4)) {
                *word = u32::from_le_bytes(bytes.try_into().expect("fixed checksum word"));
            }
            Ok(AirReplayShellCommand::CheckSums { index, words })
        }
        NEXT_CHECK_SUM_OPCODE => {
            require_size(index, command, 6)?;
            Ok(AirReplayShellCommand::NextCheckSum {
                index,
                kind: command[1],
                value: u32::from_le_bytes(command[2..6].try_into().expect("fixed checksum")),
            })
        }
        CAMERA_OPCODE => {
            require_size(index, command, 10)?;
            Ok(AirReplayShellCommand::Camera {
                index,
                zoom: command[1],
                x: i32::from_le_bytes(command[2..6].try_into().expect("fixed camera x")),
                y: i32::from_le_bytes(command[6..10].try_into().expect("fixed camera y")),
            })
        }
        TURN_DATA_OPCODE => {
            require_size(index, command, 11)?;
            Ok(AirReplayShellCommand::TurnData {
                index,
                payload: command[1..11].try_into().expect("fixed TurnData payload"),
            })
        }
        PLAYER_SPEED_OPCODE => {
            require_size(index, command, 9)?;
            Ok(AirReplayShellCommand::PlayerSpeed {
                index,
                deltas: command[1..9].try_into().expect("fixed PlayerSpeed payload"),
            })
        }
        _ => Err(CanonicalAirPackageShellError::UnsupportedShellOpcode { index, opcode }),
    }
}

fn decode_shell(
    position: CommandPackagePosition,
    commands: &[Vec<u8>],
) -> Result<Vec<AirReplayShellCommand>, CanonicalAirPackageShellError> {
    if commands.len() > usize::from(u16::MAX) + 1 {
        return Err(CanonicalAirPackageShellError::TooManyCommands(
            commands.len(),
        ));
    }
    let group = usize::from(position.group_command_index);
    let action = usize::from(position.action_command_index);
    for index in [position.group_command_index, position.action_command_index] {
        if usize::from(index) >= commands.len() {
            return Err(CanonicalAirPackageShellError::PositionOutsidePackage {
                index,
                commands: commands.len(),
            });
        }
    }
    if action != group + 1 {
        return Err(CanonicalAirPackageShellError::NonAdjacentAirPair {
            group_command_index: position.group_command_index,
            action_command_index: position.action_command_index,
        });
    }
    commands
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != group && *index != action)
        .map(|(index, command)| {
            parse_shell(
                u16::try_from(index).expect("command count validated for u16 indices"),
                command,
            )
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub fn prepare_canonical_air_replay_package(
    world: &World,
    builds: &[BuildData],
    groups: &Groups,
    paths: &[PathStack],
    command_state: &CommandPackageState,
    selection_authority: &GroupMoveAuthority,
    authority: &AirGroupRuntimeAuthority,
    scenario: &ScenarioIgnoreOrdersAuthority,
    player_who: &[Option<u8>; NETWORK_PLAYERS],
    position: CommandPackagePosition,
    commands: &[Vec<u8>],
) -> Result<PreparedCanonicalAirReplayPackage, CanonicalAirPackageShellError> {
    let shell = decode_shell(position, commands)?;
    let group = &commands[usize::from(position.group_command_index)];
    let action = &commands[usize::from(position.action_command_index)];
    let air = prepare_canonical_air_packet_pair(
        world,
        builds,
        groups,
        paths,
        command_state,
        selection_authority,
        authority,
        scenario,
        player_who,
        position,
        group,
        action,
    )?;
    Ok(PreparedCanonicalAirReplayPackage {
        position,
        commands_before: commands.to_vec(),
        shell,
        air,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn commit_canonical_air_replay_package(
    world: &mut World,
    builds: &[BuildData],
    groups: &mut Groups,
    paths: &mut [PathStack],
    command_state: &mut CommandPackageState,
    selection_authority: &GroupMoveAuthority,
    authority: &AirGroupRuntimeAuthority,
    scenario: &ScenarioIgnoreOrdersAuthority,
    commands: &[Vec<u8>],
    prepared: PreparedCanonicalAirReplayPackage,
) -> Result<CanonicalAirReplayPackageReceipt, CanonicalAirPackageShellError> {
    if commands != prepared.commands_before {
        return Err(CanonicalAirPackageShellError::StaleCommandImage);
    }
    let air = commit_canonical_air_package(
        world,
        builds,
        groups,
        paths,
        command_state,
        selection_authority,
        authority,
        scenario,
        prepared.air,
    );
    Ok(CanonicalAirReplayPackageReceipt {
        position: prepared.position,
        command_image: prepared.commands_before,
        shell: prepared.shell,
        air,
    })
}

impl CanonicalAirReplayPackageReceipt {
    pub fn validates(&self) -> bool {
        let Ok(shell) = decode_shell(self.position, &self.command_image) else {
            return false;
        };
        let group = usize::from(self.position.group_command_index);
        let action = usize::from(self.position.action_command_index);
        shell == self.shell
            && self.air.validates()
            && self.air.request.position == self.position
            && self.air.request.group_packet == self.command_image[group]
            && self.air.request.packet == self.command_image[action]
    }
}
