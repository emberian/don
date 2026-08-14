// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact current-STRAFE retarget cone of `Group::action_flight`.
//!
//! This is deliberately not a general Flight implementation. It accepts only ATTACK with no
//! modifiers, an all-Unit selection, and every actor which reaches order dispatch already in
//! STRAFE. Retail's direct arm rewrites that existing payload, including its exact missile-mask
//! skip and fuel-exhausted target-only tail; every fresh-install branch remains typed red.

use crate::command::air_launch_receivers::MISSILE_OBJECT_MASK;
use crate::order::{Order, OrderIndex, ORDER_GROUP};
use crate::systems::air_group_action_transaction::{
    decode_group_selection_packet, CanonicalObjectBand, CanonicalObjectGeneration,
    CanonicalObjectIdentity, CommandPackagePosition, GroupSelectionWireError,
};
use crate::systems::air_runtime_authority::ScenarioIgnoreOrdersAuthority;
use crate::systems::canonical_air_group_host::{AirGroupRuntimeAuthority, AirGroupUnitAuthority};
use crate::systems::canonical_group_move_host::{
    groups_equal, prepare_air_group_selection, unit_still_current, CommandPackageState,
    GroupMoveAuthority, PackageError, PreparedGroupSelection, PreparedSelectionObject,
    UnitIdentity, NETWORK_PLAYERS,
};
use crate::systems::groups_guys::Groups;
use crate::systems::movement::PathStack;
use crate::systems::production::BuildData;
use crate::systems::sparse_object_bands_authority_frontier::{RetailBand, RetailObjectAddress};
use crate::systems::strafe_runtime_authority::validate_strafe_order;
use crate::world::{Handle, World, WorldObjectIdentity, OBJ_FLAG_ACTIVE};

pub const FLIGHT_OPCODE: u8 = 28;
pub const ATTACK_ORDER_INDEX: i32 = 10;
pub const FLIGHT_WIRE_SIZE: usize = 25;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlightStrafeRequest {
    pub target_o: i32,
    pub target_who: i32,
    pub shift: i32,
    pub ctrl: i32,
    pub alt: i32,
    pub orders: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalFlightStrafeError {
    WrongWireSize { expected: usize, actual: usize },
    WrongOpcode(u8),
    UnsupportedRequest(FlightStrafeRequest),
    GroupWire(GroupSelectionWireError),
    PositionFrameMismatch { world: i32, packet: i32 },
    PositionPlayOutOfRange(i32),
    MissingPlayerMap(usize),
    PlayerOwnerMismatch { expected: u8, got: u8 },
    ScenarioIgnoreOrdersArmed,
    Selection(PackageError),
    EmptySelection,
    NonUnitSelection,
    BuildingGroup,
    InvalidTarget { who: i32, o: i32 },
    MissingAirAuthority(Handle),
    CurrentOrderNotStrafe(UnitIdentity),
    MalformedStrafe(UnitIdentity),
    StaleCanonicalState,
}

impl From<PackageError> for CanonicalFlightStrafeError {
    fn from(error: PackageError) -> Self {
        Self::Selection(error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlightStrafeActorEffect {
    RetargetedAndArmed,
    RetargetedFuelExhausted,
    SkippedMissileMask,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlightStrafeActorReceipt {
    pub identity: UnitIdentity,
    pub before: Order,
    pub after: Order,
    pub effect: FlightStrafeActorEffect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlightStrafeSkipReason {
    NonNuclearActorInNuclearGroup,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlightStrafeSkippedActorReceipt {
    pub identity: UnitIdentity,
    pub reason: FlightStrafeSkipReason,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalFlightStrafeReceipt {
    pub position: CommandPackagePosition,
    pub group_packet: Vec<u8>,
    pub flight_packet: Vec<u8>,
    pub request: FlightStrafeRequest,
    pub target: CanonicalObjectIdentity,
    pub target_uid: u16,
    pub target_position: (i32, i32),
    pub actors: Vec<FlightStrafeActorReceipt>,
    pub skipped_actors: Vec<FlightStrafeSkippedActorReceipt>,
    pub command_state_revision_before: u64,
    pub command_state_revision_after: u64,
}

#[derive(Clone, Debug)]
pub struct PreparedCanonicalFlightStrafe {
    selection: PreparedGroupSelection,
    target: FlightTargetSnapshot,
    authority_before: AirGroupRuntimeAuthority,
    scenario_before: ScenarioIgnoreOrdersAuthority,
    receipt: CanonicalFlightStrafeReceipt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FlightTargetSnapshot {
    pub(crate) identity: CanonicalObjectIdentity,
    pub(crate) uid: u16,
    pub(crate) position: (i32, i32),
    backing: TargetBacking,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TargetBacking {
    Unit { handle: Handle, row: usize },
    Build { row: u32 },
}

fn read_i32(packet: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(
        packet[offset..offset + 4]
            .try_into()
            .expect("fixed Flight field"),
    )
}

pub fn decode_flight_strafe_request(
    packet: &[u8],
) -> Result<FlightStrafeRequest, CanonicalFlightStrafeError> {
    if packet.len() != FLIGHT_WIRE_SIZE {
        return Err(CanonicalFlightStrafeError::WrongWireSize {
            expected: FLIGHT_WIRE_SIZE,
            actual: packet.len(),
        });
    }
    if packet[0] != FLIGHT_OPCODE {
        return Err(CanonicalFlightStrafeError::WrongOpcode(packet[0]));
    }
    let request = FlightStrafeRequest {
        target_o: read_i32(packet, 1),
        target_who: read_i32(packet, 5),
        shift: read_i32(packet, 9),
        ctrl: read_i32(packet, 13),
        alt: read_i32(packet, 17),
        orders: read_i32(packet, 21),
    };
    if request.orders != ATTACK_ORDER_INDEX
        || request.shift != 0
        || request.ctrl != 0
        || request.alt != 0
    {
        return Err(CanonicalFlightStrafeError::UnsupportedRequest(request));
    }
    Ok(request)
}

pub(crate) fn flight_target_snapshot(
    world: &World,
    builds: &[BuildData],
    target_who: i32,
    target_o: i32,
) -> Result<FlightTargetSnapshot, CanonicalFlightStrafeError> {
    let who = u8::try_from(target_who).map_err(|_| CanonicalFlightStrafeError::InvalidTarget {
        who: target_who,
        o: target_o,
    })?;
    i16::try_from(target_o).map_err(|_| CanonicalFlightStrafeError::InvalidTarget {
        who: target_who,
        o: target_o,
    })?;
    if RetailBand::Unit.contains(target_o) {
        let row = world.unit_row_at(target_who, target_o).ok_or(
            CanonicalFlightStrafeError::InvalidTarget {
                who: target_who,
                o: target_o,
            },
        )?;
        if world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
            return Err(CanonicalFlightStrafeError::InvalidTarget {
                who: target_who,
                o: target_o,
            });
        }
        let handle = world
            .handle_at_row(row)
            .ok_or(CanonicalFlightStrafeError::InvalidTarget {
                who: target_who,
                o: target_o,
            })?;
        return Ok(FlightTargetSnapshot {
            identity: CanonicalObjectIdentity {
                owner: who,
                band: CanonicalObjectBand::Unit,
                o: target_o,
                generation: CanonicalObjectGeneration::Unit {
                    id: handle.id,
                    generation: handle.generation,
                },
            },
            uid: world.units.get_uid(row),
            position: (world.units.x_internal()[row], world.units.y_internal()[row]),
            backing: TargetBacking::Unit { handle, row },
        });
    }
    if RetailBand::Build.contains(target_o) {
        let address = RetailObjectAddress::new(who, RetailBand::Build, target_o);
        let WorldObjectIdentity::BuildRow(row) = world
            .object_bands()
            .live_identity(address)
            .ok_or(CanonicalFlightStrafeError::InvalidTarget {
                who: target_who,
                o: target_o,
            })?
        else {
            return Err(CanonicalFlightStrafeError::InvalidTarget {
                who: target_who,
                o: target_o,
            });
        };
        let build = builds
            .get(row as usize)
            .filter(|build| {
                build.who == who && i32::from(build.object_id()) == target_o && build.is_valid()
            })
            .ok_or(CanonicalFlightStrafeError::InvalidTarget {
                who: target_who,
                o: target_o,
            })?;
        return Ok(FlightTargetSnapshot {
            identity: CanonicalObjectIdentity {
                owner: who,
                band: CanonicalObjectBand::Build,
                o: target_o,
                generation: CanonicalObjectGeneration::BuildRow(row),
            },
            uid: build.uid,
            position: build.position(),
            backing: TargetBacking::Build { row },
        });
    }
    Err(CanonicalFlightStrafeError::InvalidTarget {
        who: target_who,
        o: target_o,
    })
}

fn air_authority(
    authority: &AirGroupRuntimeAuthority,
    handle: Handle,
) -> Result<AirGroupUnitAuthority, CanonicalFlightStrafeError> {
    authority
        .units
        .iter()
        .copied()
        .find(|entry| entry.handle == handle)
        .ok_or(CanonicalFlightStrafeError::MissingAirAuthority(handle))
}

fn rewrite_current_strafe(
    order: &mut Order,
    target: CanonicalObjectIdentity,
    target_uid: u16,
    target_position: (i32, i32),
    arm: bool,
) -> Result<(), ()> {
    if order.kind != OrderIndex::Strafe {
        return Err(());
    }
    let payload = order.strafe.as_mut().ok_or(())?;
    if i32::from(order.target_o) != payload.target_o
        || i32::from(order.target_who) != payload.target_who
        || order.target_uid != payload.target_uid
        || validate_strafe_order(payload).is_err()
    {
        return Err(());
    }
    payload.target_o = target.o;
    payload.target_who = i32::from(target.owner);
    payload.target_uid = target_uid;
    payload.xx = target_position.0;
    payload.yy = target_position.1;
    order.target_who = target.owner as i8;
    order.target_o = target.o as i16;
    order.target_uid = target_uid;
    if arm {
        payload.mandatory = 1;
        payload.air.returning = 0;
        order.flags |= ORDER_GROUP;
    }
    Ok(())
}

fn current_strafe_is_coherent(order: &Order) -> bool {
    let Some(payload) = order.strafe.as_ref() else {
        return false;
    };
    order.kind == OrderIndex::Strafe
        && i32::from(order.target_o) == payload.target_o
        && i32::from(order.target_who) == payload.target_who
        && order.target_uid == payload.target_uid
        && validate_strafe_order(payload).is_ok()
}

pub(crate) fn flight_target_still_current(
    world: &World,
    builds: &[BuildData],
    target: FlightTargetSnapshot,
) -> bool {
    match target.backing {
        TargetBacking::Unit { handle, row } => {
            world.row_of(handle) == Some(row)
                && world.handle_at_row(row) == Some(handle)
                && world.units.get_flags(row) & OBJ_FLAG_ACTIVE != 0
                && world.units.get_who(row) == target.identity.owner
                && i32::from(world.units.o()[row]) == target.identity.o
                && world.units.get_uid(row) == target.uid
                && (world.units.x_internal()[row], world.units.y_internal()[row]) == target.position
        }
        TargetBacking::Build { row } => builds.get(row as usize).is_some_and(|build| {
            build.who == target.identity.owner
                && i32::from(build.object_id()) == target.identity.o
                && build.uid == target.uid
                && build.is_valid()
                && build.position() == target.position
                && world.object_bands().live_identity(RetailObjectAddress::new(
                    target.identity.owner,
                    RetailBand::Build,
                    target.identity.o,
                )) == Some(WorldObjectIdentity::BuildRow(row))
        }),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn prepare_canonical_flight_strafe(
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
) -> Result<PreparedCanonicalFlightStrafe, CanonicalFlightStrafeError> {
    if position.game_frame != world.frame {
        return Err(CanonicalFlightStrafeError::PositionFrameMismatch {
            world: world.frame,
            packet: position.game_frame,
        });
    }
    let play = usize::try_from(position.play)
        .map_err(|_| CanonicalFlightStrafeError::PositionPlayOutOfRange(position.play))?;
    if play >= NETWORK_PLAYERS {
        return Err(CanonicalFlightStrafeError::PositionPlayOutOfRange(
            position.play,
        ));
    }
    let group = decode_group_selection_packet(group_packet)
        .map_err(CanonicalFlightStrafeError::GroupWire)?;
    let expected = player_who[play].ok_or(CanonicalFlightStrafeError::MissingPlayerMap(play))?;
    if expected != group.owner {
        return Err(CanonicalFlightStrafeError::PlayerOwnerMismatch {
            expected,
            got: group.owner,
        });
    }
    if scenario.ignore_orders {
        return Err(CanonicalFlightStrafeError::ScenarioIgnoreOrdersArmed);
    }
    let request = decode_flight_strafe_request(flight_packet)?;
    let target = flight_target_snapshot(world, builds, request.target_who, request.target_o)?;
    let mut selection = prepare_air_group_selection(
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
        return Err(CanonicalFlightStrafeError::EmptySelection);
    }
    let selected_group = &selection.groups_after.list[selection.group_slot];
    if selected_group.buildings != 0 || selected_group.disband != 0 {
        return Err(CanonicalFlightStrafeError::BuildingGroup);
    }

    let selected_units = selection
        .selected_objects
        .iter()
        .map(|selected| {
            let PreparedSelectionObject::Unit(member) = selected else {
                return Err(CanonicalFlightStrafeError::NonUnitSelection);
            };
            Ok((
                member.clone(),
                air_authority(authority, member.identity.handle)?,
            ))
        })
        .collect::<Result<Vec<_>, CanonicalFlightStrafeError>>()?;
    let nuclear_group = selected_units.iter().any(|(_, air)| air.is_nuclear_missile);

    let mut actors = Vec::with_capacity(selected_units.len());
    let mut skipped_actors = Vec::new();
    for (member, air) in selected_units {
        if nuclear_group && !air.is_nuclear_missile {
            skipped_actors.push(FlightStrafeSkippedActorReceipt {
                identity: member.identity,
                reason: FlightStrafeSkipReason::NonNuclearActorInNuclearGroup,
            });
            continue;
        }
        let mutation = selection
            .units
            .iter_mut()
            .find(|mutation| mutation.before.identity.handle == member.identity.handle)
            .expect("Unit selection always stages its group backlink");
        let current = mutation.after.orders.current_mut().ok_or_else(|| {
            CanonicalFlightStrafeError::CurrentOrderNotStrafe(member.identity.clone())
        })?;
        let before = current.clone();
        if !current_strafe_is_coherent(&before) {
            return Err(if before.kind == OrderIndex::Strafe {
                CanonicalFlightStrafeError::MalformedStrafe(member.identity.clone())
            } else {
                CanonicalFlightStrafeError::CurrentOrderNotStrafe(member.identity.clone())
            });
        }
        let effect = if air.object_masks & MISSILE_OBJECT_MASK != 0 {
            FlightStrafeActorEffect::SkippedMissileMask
        } else {
            let fueled =
                crate::systems::air::mana_left(air.mana_cap, world.units.mana_burn()[member.row])
                    != 0;
            rewrite_current_strafe(
                current,
                target.identity,
                target.uid,
                target.position,
                fueled,
            )
            .expect("current STRAFE coherence was validated above");
            if fueled {
                FlightStrafeActorEffect::RetargetedAndArmed
            } else {
                FlightStrafeActorEffect::RetargetedFuelExhausted
            }
        };
        actors.push(FlightStrafeActorReceipt {
            identity: member.identity,
            before,
            after: current.clone(),
            effect,
        });
    }

    let receipt = CanonicalFlightStrafeReceipt {
        position,
        group_packet: group_packet.to_vec(),
        flight_packet: flight_packet.to_vec(),
        request,
        target: target.identity,
        target_uid: target.uid,
        target_position: target.position,
        actors,
        skipped_actors,
        command_state_revision_before: selection.command_state_before.revision(),
        command_state_revision_after: selection.command_state_after.revision(),
    };
    Ok(PreparedCanonicalFlightStrafe {
        selection,
        target,
        authority_before: authority.clone(),
        scenario_before: scenario.clone(),
        receipt,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn commit_canonical_flight_strafe(
    world: &mut World,
    builds: &[BuildData],
    groups: &mut Groups,
    paths: &mut [PathStack],
    command_state: &mut CommandPackageState,
    selection_authority: &GroupMoveAuthority,
    authority: &AirGroupRuntimeAuthority,
    scenario: &ScenarioIgnoreOrdersAuthority,
    prepared: PreparedCanonicalFlightStrafe,
) -> Result<CanonicalFlightStrafeReceipt, CanonicalFlightStrafeError> {
    let selection = &prepared.selection;
    if command_state != &selection.command_state_before
        || !groups_equal(groups, &selection.groups_before)
        || selection_authority.revision != selection.authority_revision
        || selection_authority.composition_digest != selection.authority_digest
        || selection_authority.members != selection.authority_members
        || authority != &prepared.authority_before
        || scenario != &prepared.scenario_before
        || !flight_target_still_current(world, builds, prepared.target)
        || selection
            .units
            .iter()
            .any(|mutation| !unit_still_current(world, paths, &mutation.before))
    {
        return Err(CanonicalFlightStrafeError::StaleCanonicalState);
    }
    *groups = selection.groups_after.clone();
    *command_state = selection.command_state_after.clone();
    for mutation in &selection.units {
        let row = world
            .row_of(mutation.before.identity.handle)
            .expect("all Flight identities were revalidated");
        world.units.group_mut()[row] = mutation.after.group;
        *world.orders_mut(row) = mutation.after.orders.clone();
    }
    Ok(prepared.receipt)
}

impl CanonicalFlightStrafeReceipt {
    pub fn validates(&self) -> bool {
        let Ok(group) = decode_group_selection_packet(&self.group_packet) else {
            return false;
        };
        let Ok(request) = decode_flight_strafe_request(&self.flight_packet) else {
            return false;
        };
        self.position.action_command_index == self.position.group_command_index + 1
            && request == self.request
            && self.target.is_well_formed()
            && i32::from(self.target.owner) == self.request.target_who
            && self.target.o == self.request.target_o
            && !self.actors.is_empty()
            && self.actors.iter().all(|actor| {
                if actor.identity.who != group.owner || !current_strafe_is_coherent(&actor.before) {
                    return false;
                }
                let mut expected = actor.before.clone();
                let effect_valid = match actor.effect {
                    FlightStrafeActorEffect::RetargetedAndArmed => rewrite_current_strafe(
                        &mut expected,
                        self.target,
                        self.target_uid,
                        self.target_position,
                        true,
                    )
                    .is_ok(),
                    FlightStrafeActorEffect::RetargetedFuelExhausted => rewrite_current_strafe(
                        &mut expected,
                        self.target,
                        self.target_uid,
                        self.target_position,
                        false,
                    )
                    .is_ok(),
                    FlightStrafeActorEffect::SkippedMissileMask => true,
                };
                effect_valid && actor.after == expected
            })
            && self
                .skipped_actors
                .iter()
                .all(|actor| actor.identity.who == group.owner)
            && {
                let mut identities = std::collections::HashSet::new();
                self.actors
                    .iter()
                    .map(|actor| actor.identity.handle)
                    .chain(
                        self.skipped_actors
                            .iter()
                            .map(|actor| actor.identity.handle),
                    )
                    .all(|handle| identities.insert(handle))
            }
            && self.command_state_revision_after
                == self.command_state_revision_before.wrapping_add(1)
    }
}
