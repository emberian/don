// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact decoded-package shells around canonical LaunchPatrol/Scramble pairs.
//!
//! Replay delivery has already removed command padding and exposes one byte-exact slice per
//! command. This adapter retains the complete chronology, decodes the bounded retail shell
//! commands surrounding the air pair, and commits only the landed canonical Group/air
//! transaction. Shell state remains an explicit receipt rather than being copied into the
//! legacy `command::Bridge` shadow owners.

use crate::systems::air_group_action_transaction::{
    AirGroupActionReceipt, AirTransactionStatus, CommandPackagePosition, LAUNCH_PATROL_OPCODE,
    SCRAMBLE_OPCODE,
};
use crate::systems::air_runtime_authority::ScenarioIgnoreOrdersAuthority;
use crate::systems::canonical_air_group_host::{
    commit_canonical_air_package, prepare_canonical_air_packet_pair, AirGroupRuntimeAuthority,
    CanonicalAirPackageError, PreparedCanonicalAirPackage,
};
use crate::systems::canonical_group_move_host::{
    build_still_current, groups_equal, prepare_air_group_selection, unit_still_current,
    BuildSelectionIdentity, CommandPackageState, GroupMoveAuthority, PackageError,
    PreparedGroupSelection, PreparedSelectionObject, NETWORK_PLAYERS,
};
use crate::systems::groups_guys::Groups;
use crate::systems::movement::PathStack;
use crate::systems::production::BuildData;
use crate::world::{World, OBJ_FLAG_ACTIVE};

pub const CHECK_SUMS_OPCODE: u8 = 57;
pub const NEXT_CHECK_SUM_OPCODE: u8 = 58;
pub const CAMERA_OPCODE: u8 = 72;
pub const TURN_DATA_OPCODE: u8 = 74;
pub const PLAYER_SPEED_OPCODE: u8 = 79;
pub const FLIGHT_OPCODE: u8 = 28;
const ATTACK_ORDER_INDEX: i32 = 10;
pub const AIRBASE_TYPE_INDEX: i32 = 447;
pub const NUCLEAR_MISSILE_TYPE_INDEX: i32 = 315;

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
    Flight(CanonicalFlightNoActionError),
    StaleCommandImage,
    NoAirPairs,
    AirTransactionNotApplied {
        group_command_index: u16,
    },
    StaleCanonicalState,
}

impl From<CanonicalAirPackageError> for CanonicalAirPackageShellError {
    fn from(error: CanonicalAirPackageError) -> Self {
        Self::Air(error)
    }
}

impl From<CanonicalFlightNoActionError> for CanonicalAirPackageShellError {
    fn from(error: CanonicalFlightNoActionError) -> Self {
        Self::Flight(error)
    }
}

/// The exact retail Flight subdomain which precedes the sole otherwise-supported AIR package:
/// cached Airbases, ATTACK, no modifier, and a complete containment walk with no Nuclear Missile.
/// Retail filters every child at the type-315 test before reading busy/mana/range/order state, so
/// this arm mutates only the opcode-0 Group/cache selection and has no Flight order tail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlightAttackNoActionRequest {
    pub target_o: i32,
    pub target_who: i32,
    pub shift: i32,
    pub ctrl: i32,
    pub alt: i32,
    pub orders: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalFlightNoActionError {
    WrongWireSize { expected: usize, actual: usize },
    WrongOpcode(u8),
    UnsupportedRequest(FlightAttackNoActionRequest),
    GroupWire(crate::systems::air_group_action_transaction::GroupSelectionWireError),
    PositionFrameMismatch { world: i32, packet: i32 },
    PositionPlayOutOfRange(i32),
    MissingPlayerMap(usize),
    PlayerOwnerMismatch { expected: u8, got: u8 },
    Selection(PackageError),
    ScenarioIgnoreOrdersArmed,
    EmptySelection,
    NonBuildingSelection,
    NonAirbaseSelection(BuildSelectionIdentity),
    InvalidTarget { who: i32, o: i32 },
    InvalidContainment { who: i8, o: i16 },
    ContainmentCycle { who: u8, o: i16 },
    MissingAirAuthority(crate::world::Handle),
    NuclearMissileTail { who: u8, o: i16 },
    StaleSelection,
}

impl From<PackageError> for CanonicalFlightNoActionError {
    fn from(error: PackageError) -> Self {
        Self::Selection(error)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalFlightNoActionReceipt {
    pub position: CommandPackagePosition,
    pub group_packet: Vec<u8>,
    pub flight_packet: Vec<u8>,
    pub request: FlightAttackNoActionRequest,
    pub target: crate::systems::air_group_action_transaction::CanonicalObjectIdentity,
    pub selected_airbases: Vec<BuildSelectionIdentity>,
    pub contained_non_missiles:
        Vec<crate::systems::air_group_action_transaction::CanonicalObjectIdentity>,
    pub command_state_revision_before: u64,
    pub command_state_revision_after: u64,
}

#[derive(Clone, Debug)]
struct PreparedCanonicalFlightNoAction {
    selection: PreparedGroupSelection,
    authority_before: AirGroupRuntimeAuthority,
    scenario_before: ScenarioIgnoreOrdersAuthority,
    receipt: CanonicalFlightNoActionReceipt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReplayPairPosition {
    Air(CommandPackagePosition),
    FlightNoAction(CommandPackagePosition),
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

/// Package-wide identity shared by every command in one decoded retail contribution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AirReplayPackageIdentity {
    pub game_frame: i32,
    pub package_serial: u32,
    pub play: i32,
}

impl AirReplayPackageIdentity {
    fn position(self, group_command_index: u16) -> CommandPackagePosition {
        CommandPackagePosition {
            game_frame: self.game_frame,
            package_serial: self.package_serial,
            play: self.play,
            group_command_index,
            action_command_index: group_command_index + 1,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PreparedCanonicalAirReplayBatch {
    identity: AirReplayPackageIdentity,
    commands_before: Vec<Vec<u8>>,
    shell: Vec<AirReplayShellCommand>,
    positions: Vec<ReplayPairPosition>,
    expected_air: Vec<AirGroupActionReceipt>,
    expected_flight_no_action: Vec<CanonicalFlightNoActionReceipt>,
    world_digest_before: u64,
    selection_authority_before: GroupMoveAuthority,
    authority_before: AirGroupRuntimeAuthority,
    scenario_before: ScenarioIgnoreOrdersAuthority,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalAirReplayBatchReceipt {
    pub identity: AirReplayPackageIdentity,
    pub command_image: Vec<Vec<u8>>,
    pub shell: Vec<AirReplayShellCommand>,
    pub air: Vec<AirGroupActionReceipt>,
    pub flight_no_action: Vec<CanonicalFlightNoActionReceipt>,
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

fn read_i32(command: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(
        command[offset..offset + 4]
            .try_into()
            .expect("validated fixed Flight packet"),
    )
}

fn decode_flight_attack_no_action(
    command: &[u8],
) -> Result<FlightAttackNoActionRequest, CanonicalFlightNoActionError> {
    if command.len() != 25 {
        return Err(CanonicalFlightNoActionError::WrongWireSize {
            expected: 25,
            actual: command.len(),
        });
    }
    if command[0] != FLIGHT_OPCODE {
        return Err(CanonicalFlightNoActionError::WrongOpcode(command[0]));
    }
    let request = FlightAttackNoActionRequest {
        target_o: read_i32(command, 1),
        target_who: read_i32(command, 5),
        shift: read_i32(command, 9),
        ctrl: read_i32(command, 13),
        alt: read_i32(command, 17),
        orders: read_i32(command, 21),
    };
    if request.orders != ATTACK_ORDER_INDEX
        || request.shift != 0
        || request.ctrl != 0
        || request.alt != 0
    {
        return Err(CanonicalFlightNoActionError::UnsupportedRequest(request));
    }
    Ok(request)
}

fn canonical_unit_identity(
    world: &World,
    who: u8,
    o: i16,
) -> Result<
    crate::systems::air_group_action_transaction::CanonicalObjectIdentity,
    CanonicalFlightNoActionError,
> {
    use crate::systems::air_group_action_transaction::{
        CanonicalObjectBand, CanonicalObjectGeneration, CanonicalObjectIdentity,
    };
    let row = world
        .unit_row_at(i32::from(who), i32::from(o))
        .ok_or(CanonicalFlightNoActionError::InvalidContainment { who: who as i8, o })?;
    if world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
        return Err(CanonicalFlightNoActionError::InvalidContainment { who: who as i8, o });
    }
    let handle = world
        .handle_at_row(row)
        .ok_or(CanonicalFlightNoActionError::InvalidContainment { who: who as i8, o })?;
    Ok(CanonicalObjectIdentity {
        owner: who,
        band: CanonicalObjectBand::Unit,
        o: i32::from(o),
        generation: CanonicalObjectGeneration::Unit {
            id: handle.id,
            generation: handle.generation,
        },
    })
}

#[allow(clippy::too_many_arguments)]
fn prepare_flight_no_action(
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
    group_packet: &[u8],
    flight_packet: &[u8],
) -> Result<PreparedCanonicalFlightNoAction, CanonicalFlightNoActionError> {
    use crate::systems::air_group_action_transaction::{
        decode_group_selection_packet, CanonicalObjectBand, CanonicalObjectGeneration,
    };

    if position.game_frame != world.frame {
        return Err(CanonicalFlightNoActionError::PositionFrameMismatch {
            world: world.frame,
            packet: position.game_frame,
        });
    }
    let play = usize::try_from(position.play)
        .map_err(|_| CanonicalFlightNoActionError::PositionPlayOutOfRange(position.play))?;
    if play >= NETWORK_PLAYERS {
        return Err(CanonicalFlightNoActionError::PositionPlayOutOfRange(
            position.play,
        ));
    }
    let group = decode_group_selection_packet(group_packet)
        .map_err(CanonicalFlightNoActionError::GroupWire)?;
    let expected = player_who[play].ok_or(CanonicalFlightNoActionError::MissingPlayerMap(play))?;
    if expected != group.owner {
        return Err(CanonicalFlightNoActionError::PlayerOwnerMismatch {
            expected,
            got: group.owner,
        });
    }
    if scenario.ignore_orders {
        return Err(CanonicalFlightNoActionError::ScenarioIgnoreOrdersArmed);
    }
    let request = decode_flight_attack_no_action(flight_packet)?;
    let target_who = u8::try_from(request.target_who).map_err(|_| {
        CanonicalFlightNoActionError::InvalidTarget {
            who: request.target_who,
            o: request.target_o,
        }
    })?;
    let target_o = i16::try_from(request.target_o).map_err(|_| {
        CanonicalFlightNoActionError::InvalidTarget {
            who: request.target_who,
            o: request.target_o,
        }
    })?;
    let target = canonical_unit_identity(world, target_who, target_o).map_err(|_| {
        CanonicalFlightNoActionError::InvalidTarget {
            who: request.target_who,
            o: request.target_o,
        }
    })?;

    let selection = prepare_air_group_selection(
        world,
        builds,
        groups,
        paths,
        command_state,
        selection_authority,
        &authority.builds,
        position.game_frame,
        play,
        group.owner,
        &group.requested,
    )?;
    if selection.selected_objects.is_empty() {
        return Err(CanonicalFlightNoActionError::EmptySelection);
    }
    let selected_group = &selection.groups_after.list[selection.group_slot];
    if selected_group.buildings == 0 || selected_group.disband != 0 {
        return Err(CanonicalFlightNoActionError::NonBuildingSelection);
    }

    let mut selected_airbases = Vec::with_capacity(selection.selected_objects.len());
    let mut contained_non_missiles = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for selected in &selection.selected_objects {
        let PreparedSelectionObject::Build(member) = selected else {
            return Err(CanonicalFlightNoActionError::NonBuildingSelection);
        };
        if !member.authority.is_airbase {
            return Err(CanonicalFlightNoActionError::NonAirbaseSelection(
                member.authority.identity,
            ));
        }
        selected_airbases.push(member.authority.identity);
        let mut next_o = member.image.inside_down;
        let mut next_who = member.image.inside_down_who;
        while next_o >= 0 {
            let who = u8::try_from(next_who).map_err(|_| {
                CanonicalFlightNoActionError::InvalidContainment {
                    who: next_who,
                    o: next_o,
                }
            })?;
            if !seen.insert((who, next_o)) {
                return Err(CanonicalFlightNoActionError::ContainmentCycle { who, o: next_o });
            }
            let identity = canonical_unit_identity(world, who, next_o)?;
            let CanonicalObjectGeneration::Unit { id, generation } = identity.generation else {
                unreachable!("canonical_unit_identity always returns a Unit generation")
            };
            let handle = crate::world::Handle { id, generation };
            let unit_authority = authority
                .unit(handle)
                .ok_or(CanonicalFlightNoActionError::MissingAirAuthority(handle))?;
            if unit_authority.is_nuclear_missile {
                return Err(CanonicalFlightNoActionError::NuclearMissileTail { who, o: next_o });
            }
            debug_assert_eq!(identity.band, CanonicalObjectBand::Unit);
            contained_non_missiles.push(identity);
            let row = world
                .unit_row_at(i32::from(who), i32::from(next_o))
                .expect("identity was resolved above");
            next_o = world.units.inside_down()[row];
            next_who = world.units.inside_down_who()[row];
        }
    }

    let receipt = CanonicalFlightNoActionReceipt {
        position,
        group_packet: group_packet.to_vec(),
        flight_packet: flight_packet.to_vec(),
        request,
        target,
        selected_airbases,
        contained_non_missiles,
        command_state_revision_before: selection.command_state_before.revision(),
        command_state_revision_after: selection.command_state_after.revision(),
    };
    Ok(PreparedCanonicalFlightNoAction {
        selection,
        authority_before: authority.clone(),
        scenario_before: scenario.clone(),
        receipt,
    })
}

fn commit_flight_no_action(
    world: &mut World,
    builds: &[BuildData],
    groups: &mut Groups,
    paths: &mut [PathStack],
    command_state: &mut CommandPackageState,
    selection_authority: &GroupMoveAuthority,
    authority: &AirGroupRuntimeAuthority,
    scenario: &ScenarioIgnoreOrdersAuthority,
    prepared: PreparedCanonicalFlightNoAction,
) -> Result<CanonicalFlightNoActionReceipt, CanonicalFlightNoActionError> {
    let selection = &prepared.selection;
    if command_state != &selection.command_state_before
        || !groups_equal(groups, &selection.groups_before)
        || selection_authority.revision != selection.authority_revision
        || selection_authority.composition_digest != selection.authority_digest
        || selection_authority.members != selection.authority_members
        || authority != &prepared.authority_before
        || scenario != &prepared.scenario_before
        || selection
            .units
            .iter()
            .any(|mutation| !unit_still_current(world, paths, &mutation.before))
        || selection
            .selected_objects
            .iter()
            .any(|member| match member {
                PreparedSelectionObject::Unit(_) => true,
                PreparedSelectionObject::Build(member) => {
                    !build_still_current(world, builds, &member.image)
                }
            })
    {
        return Err(CanonicalFlightNoActionError::StaleSelection);
    }
    *groups = selection.groups_after.clone();
    *command_state = selection.command_state_after.clone();
    for mutation in &selection.units {
        let row = world
            .row_of(mutation.before.identity.handle)
            .expect("every selection identity was revalidated");
        world.units.group_mut()[row] = mutation.after.group;
    }
    Ok(prepared.receipt)
}

impl CanonicalFlightNoActionReceipt {
    pub fn validates(&self) -> bool {
        let Ok(group) = crate::systems::air_group_action_transaction::decode_group_selection_packet(
            &self.group_packet,
        ) else {
            return false;
        };
        let Ok(request) = decode_flight_attack_no_action(&self.flight_packet) else {
            return false;
        };
        self.position.action_command_index == self.position.group_command_index + 1
            && request == self.request
            && !self.selected_airbases.is_empty()
            && group.owner == self.selected_airbases[0].who
            && self.target.is_well_formed()
            && self.target.address()
                == (self.request.target_who as u8, self.request.target_o as i16)
            && self.selected_airbases.iter().all(|identity| {
                identity.who == group.owner
                    && crate::systems::air_group_action_transaction::CanonicalObjectBand::Build
                        .contains(i32::from(identity.o))
            })
            && self
                .contained_non_missiles
                .iter()
                .all(|identity| identity.is_well_formed())
            && self.command_state_revision_after
                == self.command_state_revision_before.wrapping_add(1)
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

fn decode_batch_shell(
    identity: AirReplayPackageIdentity,
    commands: &[Vec<u8>],
) -> Result<(Vec<ReplayPairPosition>, Vec<AirReplayShellCommand>), CanonicalAirPackageShellError> {
    if commands.len() > usize::from(u16::MAX) + 1 {
        return Err(CanonicalAirPackageShellError::TooManyCommands(
            commands.len(),
        ));
    }
    let mut positions = Vec::new();
    let mut shell = Vec::new();
    let mut index = 0usize;
    while index < commands.len() {
        let command = &commands[index];
        let Some(&opcode) = command.first() else {
            return Err(CanonicalAirPackageShellError::EmptyCommand {
                index: u16::try_from(index).expect("command count checked"),
            });
        };
        if opcode == 0 {
            let Some(action) = commands.get(index + 1) else {
                return Err(CanonicalAirPackageShellError::UnexpectedAirCommand {
                    index: u16::try_from(index).expect("command count checked"),
                    opcode,
                });
            };
            let Some(&action_opcode) = action.first() else {
                return Err(CanonicalAirPackageShellError::EmptyCommand {
                    index: u16::try_from(index + 1).expect("command count checked"),
                });
            };
            if !matches!(
                action_opcode,
                LAUNCH_PATROL_OPCODE | SCRAMBLE_OPCODE | FLIGHT_OPCODE
            ) {
                return Err(CanonicalAirPackageShellError::UnexpectedAirCommand {
                    index: u16::try_from(index).expect("command count checked"),
                    opcode,
                });
            }
            let position = identity.position(
                u16::try_from(index).expect("paired group cannot occupy the final u16 index"),
            );
            positions.push(if action_opcode == FLIGHT_OPCODE {
                ReplayPairPosition::FlightNoAction(position)
            } else {
                ReplayPairPosition::Air(position)
            });
            index += 2;
            continue;
        }
        if matches!(
            opcode,
            LAUNCH_PATROL_OPCODE | SCRAMBLE_OPCODE | FLIGHT_OPCODE
        ) {
            return Err(CanonicalAirPackageShellError::UnexpectedAirCommand {
                index: u16::try_from(index).expect("command count checked"),
                opcode,
            });
        }
        shell.push(parse_shell(
            u16::try_from(index).expect("command count checked"),
            command,
        )?);
        index += 1;
    }
    if positions.is_empty() {
        return Err(CanonicalAirPackageShellError::NoAirPairs);
    }
    Ok((positions, shell))
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

/// Preflight every adjacent AIR pair in one decoded retail package on detached canonical
/// owners. The shadow execution is important: a later cached Group observes the selection
/// revision and order image published by every earlier pair, exactly as the retail command
/// loop does, while the live Sim remains untouched until the package-wide commit.
#[allow(clippy::too_many_arguments)]
pub fn prepare_canonical_air_replay_batch(
    world: &World,
    builds: &[BuildData],
    groups: &Groups,
    paths: &[PathStack],
    command_state: &CommandPackageState,
    selection_authority: &GroupMoveAuthority,
    authority: &AirGroupRuntimeAuthority,
    scenario: &ScenarioIgnoreOrdersAuthority,
    player_who: &[Option<u8>; NETWORK_PLAYERS],
    identity: AirReplayPackageIdentity,
    commands: &[Vec<u8>],
) -> Result<PreparedCanonicalAirReplayBatch, CanonicalAirPackageShellError> {
    let (positions, shell) = decode_batch_shell(identity, commands)?;
    let mut shadow_world = world.clone();
    let mut shadow_groups = groups.clone();
    let mut shadow_paths = paths.to_vec();
    let mut shadow_command_state = command_state.clone();
    let mut expected_air = Vec::with_capacity(positions.len());
    let mut expected_flight_no_action = Vec::new();

    for pair in &positions {
        let position = match pair {
            ReplayPairPosition::Air(position) | ReplayPairPosition::FlightNoAction(position) => {
                *position
            }
        };
        let group = &commands[usize::from(position.group_command_index)];
        let action = &commands[usize::from(position.action_command_index)];
        match pair {
            ReplayPairPosition::Air(_) => {
                let prepared = prepare_canonical_air_packet_pair(
                    &shadow_world,
                    builds,
                    &shadow_groups,
                    &shadow_paths,
                    &shadow_command_state,
                    selection_authority,
                    authority,
                    scenario,
                    player_who,
                    position,
                    group,
                    action,
                )?;
                let receipt = commit_canonical_air_package(
                    &mut shadow_world,
                    builds,
                    &mut shadow_groups,
                    &mut shadow_paths,
                    &mut shadow_command_state,
                    selection_authority,
                    authority,
                    scenario,
                    prepared,
                );
                if !matches!(receipt.status, AirTransactionStatus::Applied(_)) {
                    return Err(CanonicalAirPackageShellError::AirTransactionNotApplied {
                        group_command_index: position.group_command_index,
                    });
                }
                expected_air.push(receipt);
            }
            ReplayPairPosition::FlightNoAction(_) => {
                let prepared = prepare_flight_no_action(
                    &shadow_world,
                    builds,
                    &shadow_groups,
                    &shadow_paths,
                    &shadow_command_state,
                    selection_authority,
                    authority,
                    scenario,
                    player_who,
                    position,
                    group,
                    action,
                )?;
                let receipt = commit_flight_no_action(
                    &mut shadow_world,
                    builds,
                    &mut shadow_groups,
                    &mut shadow_paths,
                    &mut shadow_command_state,
                    selection_authority,
                    authority,
                    scenario,
                    prepared,
                )?;
                expected_flight_no_action.push(receipt);
            }
        }
    }

    Ok(PreparedCanonicalAirReplayBatch {
        identity,
        commands_before: commands.to_vec(),
        shell,
        positions,
        expected_air,
        expected_flight_no_action,
        world_digest_before: world.digest(),
        selection_authority_before: selection_authority.clone(),
        authority_before: authority.clone(),
        scenario_before: scenario.clone(),
    })
}

/// Re-run the detached package plan against live owners and publish all AIR pairs or none.
/// Any stale command, authority, selected Group/cache, target order/path, Build image, or World
/// input makes a recomputed receipt differ and restores the complete checkpoint.
#[allow(clippy::too_many_arguments)]
pub fn commit_canonical_air_replay_batch(
    world: &mut World,
    builds: &[BuildData],
    groups: &mut Groups,
    paths: &mut [PathStack],
    command_state: &mut CommandPackageState,
    selection_authority: &GroupMoveAuthority,
    authority: &AirGroupRuntimeAuthority,
    scenario: &ScenarioIgnoreOrdersAuthority,
    player_who: &[Option<u8>; NETWORK_PLAYERS],
    commands: &[Vec<u8>],
    prepared: PreparedCanonicalAirReplayBatch,
) -> Result<CanonicalAirReplayBatchReceipt, CanonicalAirPackageShellError> {
    if commands != prepared.commands_before {
        return Err(CanonicalAirPackageShellError::StaleCommandImage);
    }
    if world.digest() != prepared.world_digest_before
        || selection_authority != &prepared.selection_authority_before
        || authority != &prepared.authority_before
        || scenario != &prepared.scenario_before
    {
        return Err(CanonicalAirPackageShellError::StaleCanonicalState);
    }

    let checkpoint = (
        world.clone(),
        groups.clone(),
        paths.to_vec(),
        command_state.clone(),
    );
    let mut air = Vec::with_capacity(prepared.expected_air.len());
    let mut flight_no_action = Vec::with_capacity(prepared.expected_flight_no_action.len());
    let mut expected_air = prepared.expected_air.iter();
    let mut expected_flight = prepared.expected_flight_no_action.iter();
    for pair in &prepared.positions {
        let position = match pair {
            ReplayPairPosition::Air(position) | ReplayPairPosition::FlightNoAction(position) => {
                *position
            }
        };
        let group = &commands[usize::from(position.group_command_index)];
        let action = &commands[usize::from(position.action_command_index)];
        let result = match pair {
            ReplayPairPosition::Air(_) => {
                let expected = expected_air
                    .next()
                    .expect("prepared AIR receipt cardinality");
                let pair = prepare_canonical_air_packet_pair(
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
                )
                .map_err(CanonicalAirPackageShellError::Air);
                pair.map(|pair| {
                    let receipt = commit_canonical_air_package(
                        world,
                        builds,
                        groups,
                        paths,
                        command_state,
                        selection_authority,
                        authority,
                        scenario,
                        pair,
                    );
                    if &receipt != expected {
                        return Err(CanonicalAirPackageShellError::StaleCanonicalState);
                    }
                    air.push(receipt);
                    Ok(())
                })
                .and_then(|result| result)
            }
            ReplayPairPosition::FlightNoAction(_) => {
                let expected = expected_flight
                    .next()
                    .expect("prepared Flight receipt cardinality");
                prepare_flight_no_action(
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
                )
                .map_err(CanonicalAirPackageShellError::Flight)
                .and_then(|flight| {
                    commit_flight_no_action(
                        world,
                        builds,
                        groups,
                        paths,
                        command_state,
                        selection_authority,
                        authority,
                        scenario,
                        flight,
                    )
                    .map_err(CanonicalAirPackageShellError::Flight)
                })
                .and_then(|receipt| {
                    if &receipt != expected {
                        return Err(CanonicalAirPackageShellError::StaleCanonicalState);
                    }
                    flight_no_action.push(receipt);
                    Ok(())
                })
            }
        };
        if let Err(error) = result {
            *world = checkpoint.0;
            *groups = checkpoint.1;
            paths.clone_from_slice(&checkpoint.2);
            *command_state = checkpoint.3;
            return Err(error);
        }
    }

    Ok(CanonicalAirReplayBatchReceipt {
        identity: prepared.identity,
        command_image: prepared.commands_before,
        shell: prepared.shell,
        air,
        flight_no_action,
    })
}

impl CanonicalAirReplayBatchReceipt {
    pub fn validates(&self) -> bool {
        let Ok((positions, shell)) = decode_batch_shell(self.identity, &self.command_image) else {
            return false;
        };
        if shell != self.shell {
            return false;
        }
        let mut air = self.air.iter();
        let mut flights = self.flight_no_action.iter();
        for pair in positions {
            let (position, valid) = match pair {
                ReplayPairPosition::Air(position) => {
                    let Some(receipt) = air.next() else {
                        return false;
                    };
                    (
                        position,
                        receipt.validates()
                            && receipt.request.position == position
                            && receipt.request.group_packet
                                == self.command_image[usize::from(position.group_command_index)]
                            && receipt.request.packet
                                == self.command_image[usize::from(position.action_command_index)],
                    )
                }
                ReplayPairPosition::FlightNoAction(position) => {
                    let Some(receipt) = flights.next() else {
                        return false;
                    };
                    (
                        position,
                        receipt.validates()
                            && receipt.position == position
                            && receipt.group_packet
                                == self.command_image[usize::from(position.group_command_index)]
                            && receipt.flight_packet
                                == self.command_image[usize::from(position.action_command_index)],
                    )
                }
            };
            let _ = position;
            if !valid {
                return false;
            }
        }
        air.next().is_none() && flights.next().is_none()
    }
}
