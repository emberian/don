// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact combined `[Group][Patrol][Group][Flight]` aircraft cone.
//!
//! Retail `Group::action_patrol` delegates an all-plane Group to `action_air_patrol`, then
//! the immediately following ATTACK Flight replaces that newly installed AIR_PATROL with a
//! fresh STRAFE order.  Neither command is admitted independently here: the package shell
//! proves adjacency and passes the Patrol receipt into Flight preparation.

use crate::command::air_launch_receivers::MISSILE_OBJECT_MASK;
use crate::order::{Order, OrderIndex};
use crate::systems::air_busy_authority::{exact_air_busy, AirBusyAuthorityError};
use crate::systems::air_runtime_authority::{AirPatrolOrderPayload, AirRuntimeAuthorityError};
use crate::systems::canonical_air_group_host::{AirGroupRuntimeAuthority, AirGroupUnitAuthority};
use crate::systems::canonical_flight_strafe_host::{
    current_strafe_is_coherent, decode_flight_strafe_request, flight_target_snapshot,
    flight_target_still_current, CanonicalFlightStrafeError, FlightStrafeRequest,
    FlightTargetSnapshot,
};
use crate::systems::canonical_group_move_host::{
    groups_equal, prepare_air_group_selection, unit_still_current, CommandPackageState,
    GroupMoveAuthority, PackageError, PreparedGroupSelection, PreparedSelectionObject,
    UnitIdentity, NETWORK_PLAYERS,
};
use crate::systems::groups_guys::Groups;
use crate::systems::movement::PathStack;
use crate::systems::patrol::StrafeOrder;
use crate::systems::production::BuildData;
use crate::world::{Handle, World};

pub const PATROL_OPCODE: u8 = 10;
pub const PATROL_WIRE_SIZE: usize = 10;
pub const QUEUE_LAST: u8 = 1;

fn clamp_axis(value: i32, tiles: i32) -> i32 {
    let value = value.max(0);
    let limit = tiles.wrapping_mul(0x300);
    if value >= limit {
        limit.wrapping_sub(1)
    } else {
        value
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatrolRequest {
    pub x: i32,
    pub y: i32,
    pub queue: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalPatrolFlightError {
    WrongPatrolWireSize { expected: usize, actual: usize },
    WrongPatrolOpcode(u8),
    UnsupportedPatrolQueue(u8),
    InvalidMapDimensions((i32, i32)),
    GroupWire(crate::systems::air_group_action_transaction::GroupSelectionWireError),
    ExplicitSelection,
    PositionFrameMismatch { world: i32, packet: i32 },
    PositionPlayOutOfRange(i32),
    MissingPlayerMap(usize),
    PlayerOwnerMismatch { expected: u8, got: u8 },
    ScenarioIgnoreOrdersArmed,
    Selection(PackageError),
    EmptySelection,
    NonUnitSelection,
    BuildingGroup,
    MissingAirAuthority(Handle),
    NonAircraft(UnitIdentity),
    Busy(UnitIdentity),
    BusyAuthority(AirBusyAuthorityError),
    MissileMask(UnitIdentity),
    FuelExhausted(UnitIdentity),
    CurrentOrderNotStrafe(UnitIdentity),
    MalformedStrafe(UnitIdentity),
    IncoherentHome(UnitIdentity),
    InvalidHome(UnitIdentity),
    OrderPayload(AirRuntimeAuthorityError),
    Flight(CanonicalFlightStrafeError),
    FlightNotAdjacent,
    FlightActorMismatch,
    CurrentOrderNotPatrol(UnitIdentity),
    FreshFlightRangeUnavailable(UnitIdentity),
    NuclearMissile(UnitIdentity),
    StaleCanonicalState,
}

impl From<PackageError> for CanonicalPatrolFlightError {
    fn from(error: PackageError) -> Self {
        Self::Selection(error)
    }
}

impl From<AirBusyAuthorityError> for CanonicalPatrolFlightError {
    fn from(error: AirBusyAuthorityError) -> Self {
        Self::BusyAuthority(error)
    }
}

impl From<CanonicalFlightStrafeError> for CanonicalPatrolFlightError {
    fn from(error: CanonicalFlightStrafeError) -> Self {
        Self::Flight(error)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatrolActorReceipt {
    pub identity: UnitIdentity,
    pub before: Order,
    pub after: Order,
    pub patrol_point: (i32, i32),
    pub before_path: PathStack,
    pub after_path: PathStack,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalPatrolReceipt {
    pub position: crate::systems::air_group_action_transaction::CommandPackagePosition,
    pub group_packet: Vec<u8>,
    pub patrol_packet: Vec<u8>,
    pub request: PatrolRequest,
    pub destination: (i32, i32),
    pub map_tiles: (i32, i32),
    pub actors: Vec<PatrolActorReceipt>,
    pub command_state_revision_before: u64,
    pub command_state_revision_after: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FreshFlightActorReceipt {
    pub identity: UnitIdentity,
    pub before: Order,
    pub after: Order,
    pub before_path: PathStack,
    pub after_path: PathStack,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalFreshFlightReceipt {
    pub position: crate::systems::air_group_action_transaction::CommandPackagePosition,
    pub group_packet: Vec<u8>,
    pub flight_packet: Vec<u8>,
    pub request: FlightStrafeRequest,
    pub target: crate::systems::air_group_action_transaction::CanonicalObjectIdentity,
    pub target_uid: u16,
    pub target_position: (i32, i32),
    pub actors: Vec<FreshFlightActorReceipt>,
    pub patrol_position: crate::systems::air_group_action_transaction::CommandPackagePosition,
    pub command_state_revision_before: u64,
    pub command_state_revision_after: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedCanonicalPatrol {
    selection: PreparedGroupSelection,
    homes: Vec<Option<FlightTargetSnapshot>>,
    authority_before: AirGroupRuntimeAuthority,
    receipt: CanonicalPatrolReceipt,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedCanonicalFreshFlight {
    selection: PreparedGroupSelection,
    target: FlightTargetSnapshot,
    authority_before: AirGroupRuntimeAuthority,
    receipt: CanonicalFreshFlightReceipt,
}

fn read_i32(packet: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(
        packet[offset..offset + 4]
            .try_into()
            .expect("fixed Patrol field"),
    )
}

pub fn decode_patrol_request(packet: &[u8]) -> Result<PatrolRequest, CanonicalPatrolFlightError> {
    if packet.len() != PATROL_WIRE_SIZE {
        return Err(CanonicalPatrolFlightError::WrongPatrolWireSize {
            expected: PATROL_WIRE_SIZE,
            actual: packet.len(),
        });
    }
    if packet[0] != PATROL_OPCODE {
        return Err(CanonicalPatrolFlightError::WrongPatrolOpcode(packet[0]));
    }
    let request = PatrolRequest {
        x: read_i32(packet, 1),
        y: read_i32(packet, 5),
        queue: packet[9],
    };
    if request.queue != QUEUE_LAST {
        return Err(CanonicalPatrolFlightError::UnsupportedPatrolQueue(
            request.queue,
        ));
    }
    Ok(request)
}

fn air_authority(
    authority: &AirGroupRuntimeAuthority,
    handle: Handle,
) -> Result<AirGroupUnitAuthority, CanonicalPatrolFlightError> {
    authority
        .unit(handle)
        .ok_or(CanonicalPatrolFlightError::MissingAirAuthority(handle))
}

fn validate_cached_group(
    packet: &[u8],
    play: usize,
    player_who: &[Option<u8>; NETWORK_PLAYERS],
) -> Result<u8, CanonicalPatrolFlightError> {
    let group = crate::systems::air_group_action_transaction::decode_group_selection_packet(packet)
        .map_err(CanonicalPatrolFlightError::GroupWire)?;
    if !group.requested.is_empty() {
        return Err(CanonicalPatrolFlightError::ExplicitSelection);
    }
    let expected = player_who[play].ok_or(CanonicalPatrolFlightError::MissingPlayerMap(play))?;
    if group.owner != expected {
        return Err(CanonicalPatrolFlightError::PlayerOwnerMismatch {
            expected,
            got: group.owner,
        });
    }
    Ok(group.owner)
}

fn common_position(
    world: &World,
    position: crate::systems::air_group_action_transaction::CommandPackagePosition,
) -> Result<usize, CanonicalPatrolFlightError> {
    if position.game_frame != world.frame {
        return Err(CanonicalPatrolFlightError::PositionFrameMismatch {
            world: world.frame,
            packet: position.game_frame,
        });
    }
    let play = usize::try_from(position.play)
        .map_err(|_| CanonicalPatrolFlightError::PositionPlayOutOfRange(position.play))?;
    if play >= NETWORK_PLAYERS {
        return Err(CanonicalPatrolFlightError::PositionPlayOutOfRange(
            position.play,
        ));
    }
    Ok(play)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare_canonical_patrol(
    world: &World,
    builds: &[BuildData],
    groups: &Groups,
    paths: &[PathStack],
    command_state: &CommandPackageState,
    selection_authority: &GroupMoveAuthority,
    authority: &AirGroupRuntimeAuthority,
    scenario_ignore_orders: bool,
    player_who: &[Option<u8>; NETWORK_PLAYERS],
    map_tiles: (i32, i32),
    position: crate::systems::air_group_action_transaction::CommandPackagePosition,
    group_packet: &[u8],
    patrol_packet: &[u8],
) -> Result<PreparedCanonicalPatrol, CanonicalPatrolFlightError> {
    let play = common_position(world, position)?;
    let owner = validate_cached_group(group_packet, play, player_who)?;
    if scenario_ignore_orders {
        return Err(CanonicalPatrolFlightError::ScenarioIgnoreOrdersArmed);
    }
    if map_tiles.0 <= 0 || map_tiles.1 <= 0 {
        return Err(CanonicalPatrolFlightError::InvalidMapDimensions(map_tiles));
    }
    let request = decode_patrol_request(patrol_packet)?;
    let destination = (
        clamp_axis(request.x, map_tiles.0),
        clamp_axis(request.y, map_tiles.1),
    );
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
        owner,
        &[],
    )?;
    if selection.selected_objects.is_empty() {
        return Err(CanonicalPatrolFlightError::EmptySelection);
    }
    let selected_group = &mut selection.groups_after.list[selection.group_slot];
    if selected_group.buildings != 0 {
        return Err(CanonicalPatrolFlightError::BuildingGroup);
    }
    selected_group.disband = 0;
    selected_group.form = -1;

    let mut actors = Vec::with_capacity(selection.selected_objects.len());
    let mut homes = Vec::with_capacity(selection.selected_objects.len());
    for selected in selection.selected_objects.clone() {
        let PreparedSelectionObject::Unit(member) = selected else {
            return Err(CanonicalPatrolFlightError::NonUnitSelection);
        };
        let air = air_authority(authority, member.identity.handle)?;
        if !member.authority.can_move
            || !member.authority.can_install_order
            || !member.authority.is_plane
            || member.authority.domain != 2
            || air.is_helicopter
        {
            return Err(CanonicalPatrolFlightError::NonAircraft(member.identity));
        }
        let mutation = selection
            .units
            .iter_mut()
            .find(|mutation| mutation.before.identity.handle == member.identity.handle)
            .expect("selected Unit has staged backlink");
        let current = mutation.after.orders.current().ok_or_else(|| {
            CanonicalPatrolFlightError::CurrentOrderNotStrafe(member.identity.clone())
        })?;
        if !current_strafe_is_coherent(current) {
            return Err(if current.kind == OrderIndex::Strafe {
                CanonicalPatrolFlightError::MalformedStrafe(member.identity)
            } else {
                CanonicalPatrolFlightError::CurrentOrderNotStrafe(member.identity)
            });
        }
        if exact_air_busy(Some(current), &authority.busy_spells)? {
            return Err(CanonicalPatrolFlightError::Busy(member.identity));
        }
        if air.object_masks & MISSILE_OBJECT_MASK != 0 {
            return Err(CanonicalPatrolFlightError::MissileMask(member.identity));
        }
        if crate::systems::air::mana_left(air.mana_cap, world.units.mana_burn()[member.row]) <= 0 {
            return Err(CanonicalPatrolFlightError::FuelExhausted(member.identity));
        }
        let before = current.clone();
        let strafe = current
            .strafe
            .as_ref()
            .expect("coherent STRAFE has payload");
        let (home, patrol_xy) = match (strafe.air.oxx >= 0, strafe.air.whose >= 0) {
            (false, false) => (None, destination),
            (true, true) => {
                let home = flight_target_snapshot(world, builds, strafe.air.whose, strafe.air.oxx)
                    .map_err(|_| {
                        CanonicalPatrolFlightError::InvalidHome(member.identity.clone())
                    })?;
                (
                    Some(home),
                    (
                        destination.0.wrapping_sub(home.position.0),
                        destination.1.wrapping_sub(home.position.1),
                    ),
                )
            }
            _ => return Err(CanonicalPatrolFlightError::IncoherentHome(member.identity)),
        };
        let payload =
            AirPatrolOrderPayload::one(patrol_xy.0, patrol_xy.1, strafe.air.oxx, strafe.air.whose);
        let after =
            Order::air_patrol(payload, true).map_err(CanonicalPatrolFlightError::OrderPayload)?;
        let before_path = mutation.after.path.clone();
        mutation.after.orders.replace(after.clone());
        mutation.after.path = PathStack::default();
        actors.push(PatrolActorReceipt {
            identity: member.identity,
            before,
            after,
            patrol_point: patrol_xy,
            before_path,
            after_path: mutation.after.path.clone(),
        });
        homes.push(home);
    }
    let receipt = CanonicalPatrolReceipt {
        position,
        group_packet: group_packet.to_vec(),
        patrol_packet: patrol_packet.to_vec(),
        request,
        destination,
        map_tiles,
        actors,
        command_state_revision_before: selection.command_state_before.revision(),
        command_state_revision_after: selection.command_state_after.revision(),
    };
    Ok(PreparedCanonicalPatrol {
        selection,
        homes,
        authority_before: authority.clone(),
        receipt,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn commit_canonical_patrol(
    world: &mut World,
    builds: &[BuildData],
    groups: &mut Groups,
    paths: &mut [PathStack],
    command_state: &mut CommandPackageState,
    selection_authority: &GroupMoveAuthority,
    authority: &AirGroupRuntimeAuthority,
    prepared: PreparedCanonicalPatrol,
) -> Result<CanonicalPatrolReceipt, CanonicalPatrolFlightError> {
    let selection = &prepared.selection;
    if command_state != &selection.command_state_before
        || !groups_equal(groups, &selection.groups_before)
        || selection_authority.revision != selection.authority_revision
        || selection_authority.composition_digest != selection.authority_digest
        || selection_authority.members != selection.authority_members
        || authority != &prepared.authority_before
        || selection
            .units
            .iter()
            .any(|m| !unit_still_current(world, paths, &m.before))
        || prepared
            .homes
            .iter()
            .flatten()
            .any(|home| !flight_target_still_current(world, builds, *home))
    {
        return Err(CanonicalPatrolFlightError::StaleCanonicalState);
    }
    *groups = selection.groups_after.clone();
    *command_state = selection.command_state_after.clone();
    for mutation in &selection.units {
        let row = world
            .row_of(mutation.before.identity.handle)
            .expect("revalidated actor");
        world.units.group_mut()[row] = mutation.after.group;
        *world.orders_mut(row) = mutation.after.orders.clone();
        paths[row] = mutation.after.path.clone();
    }
    Ok(prepared.receipt)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare_canonical_fresh_flight(
    world: &World,
    builds: &[BuildData],
    groups: &Groups,
    paths: &[PathStack],
    command_state: &CommandPackageState,
    selection_authority: &GroupMoveAuthority,
    authority: &AirGroupRuntimeAuthority,
    scenario_ignore_orders: bool,
    player_who: &[Option<u8>; NETWORK_PLAYERS],
    position: crate::systems::air_group_action_transaction::CommandPackagePosition,
    group_packet: &[u8],
    flight_packet: &[u8],
    patrol: &CanonicalPatrolReceipt,
) -> Result<PreparedCanonicalFreshFlight, CanonicalPatrolFlightError> {
    if position.group_command_index != patrol.position.action_command_index + 1 {
        return Err(CanonicalPatrolFlightError::FlightNotAdjacent);
    }
    let play = common_position(world, position)?;
    let owner = validate_cached_group(group_packet, play, player_who)?;
    if scenario_ignore_orders {
        return Err(CanonicalPatrolFlightError::ScenarioIgnoreOrdersArmed);
    }
    let request = decode_flight_strafe_request(flight_packet)?;
    let target = flight_target_snapshot(world, builds, request.target_who, request.target_o)?;
    if target.identity.band
        != crate::systems::air_group_action_transaction::CanonicalObjectBand::Build
    {
        return Err(CanonicalPatrolFlightError::Flight(
            CanonicalFlightStrafeError::InvalidTarget {
                who: request.target_who,
                o: request.target_o,
            },
        ));
    }
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
        owner,
        &[],
    )?;
    if selection.selected_objects.is_empty() {
        return Err(CanonicalPatrolFlightError::EmptySelection);
    }
    if selection.groups_after.list[selection.group_slot].buildings != 0 {
        return Err(CanonicalPatrolFlightError::BuildingGroup);
    }
    let mut actors = Vec::with_capacity(selection.selected_objects.len());
    for selected in selection.selected_objects.clone() {
        let PreparedSelectionObject::Unit(member) = selected else {
            return Err(CanonicalPatrolFlightError::NonUnitSelection);
        };
        let Some(patrol_actor) = patrol
            .actors
            .iter()
            .find(|actor| actor.identity == member.identity)
        else {
            return Err(CanonicalPatrolFlightError::FlightActorMismatch);
        };
        let air = air_authority(authority, member.identity.handle)?;
        if air.is_nuclear_missile {
            return Err(CanonicalPatrolFlightError::NuclearMissile(member.identity));
        }
        if air.fresh_flight_target != Some((request.target_who, request.target_o)) {
            return Err(CanonicalPatrolFlightError::FreshFlightRangeUnavailable(
                member.identity,
            ));
        }
        if air.object_masks & MISSILE_OBJECT_MASK != 0 {
            return Err(CanonicalPatrolFlightError::MissileMask(member.identity));
        }
        if crate::systems::air::mana_left(air.mana_cap, world.units.mana_burn()[member.row]) <= 0 {
            return Err(CanonicalPatrolFlightError::FuelExhausted(member.identity));
        }
        let mutation = selection
            .units
            .iter_mut()
            .find(|mutation| mutation.before.identity.handle == member.identity.handle)
            .expect("selected Unit has staged backlink");
        let current = mutation.after.orders.current().ok_or_else(|| {
            CanonicalPatrolFlightError::CurrentOrderNotPatrol(member.identity.clone())
        })?;
        if current != &patrol_actor.after || current.kind != OrderIndex::AirPatrol {
            return Err(CanonicalPatrolFlightError::CurrentOrderNotPatrol(
                member.identity,
            ));
        }
        if exact_air_busy(Some(current), &authority.busy_spells)? {
            return Err(CanonicalPatrolFlightError::Busy(member.identity));
        }
        let patrol_payload = current.air_patrol.as_ref().ok_or_else(|| {
            CanonicalPatrolFlightError::CurrentOrderNotPatrol(member.identity.clone())
        })?;
        let payload = StrafeOrder {
            target_o: request.target_o,
            target_who: request.target_who,
            target_uid: target.uid,
            def_x: 0,
            def_y: 0,
            mandatory: 1,
            defensive: 0,
            in_range: 0,
            ever_in_range: 0,
            new_ord: 0,
            air: crate::systems::air::AirOrderWalk {
                oxx: patrol_payload.air.home_o,
                whose: patrol_payload.air.home_who,
                cruising_alt: 0x640,
                sharp_turn: 0,
                old: 0,
                returning: 0,
            },
            xx: target.position.0,
            yy: target.position.1,
        };
        let after = Order::strafe(payload, true)
            .map_err(|_| CanonicalPatrolFlightError::FlightActorMismatch)?;
        let before = current.clone();
        let before_path = mutation.after.path.clone();
        mutation.after.orders.replace(after.clone());
        mutation.after.path = PathStack::default();
        actors.push(FreshFlightActorReceipt {
            identity: member.identity,
            before,
            after,
            before_path,
            after_path: mutation.after.path.clone(),
        });
    }
    if actors.len() != patrol.actors.len() {
        return Err(CanonicalPatrolFlightError::FlightActorMismatch);
    }
    let receipt = CanonicalFreshFlightReceipt {
        position,
        group_packet: group_packet.to_vec(),
        flight_packet: flight_packet.to_vec(),
        request,
        target: target.identity,
        target_uid: target.uid,
        target_position: target.position,
        actors,
        patrol_position: patrol.position,
        command_state_revision_before: selection.command_state_before.revision(),
        command_state_revision_after: selection.command_state_after.revision(),
    };
    Ok(PreparedCanonicalFreshFlight {
        selection,
        target,
        authority_before: authority.clone(),
        receipt,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn commit_canonical_fresh_flight(
    world: &mut World,
    builds: &[BuildData],
    groups: &mut Groups,
    paths: &mut [PathStack],
    command_state: &mut CommandPackageState,
    selection_authority: &GroupMoveAuthority,
    authority: &AirGroupRuntimeAuthority,
    prepared: PreparedCanonicalFreshFlight,
) -> Result<CanonicalFreshFlightReceipt, CanonicalPatrolFlightError> {
    let selection = &prepared.selection;
    if command_state != &selection.command_state_before
        || !groups_equal(groups, &selection.groups_before)
        || selection_authority.revision != selection.authority_revision
        || selection_authority.composition_digest != selection.authority_digest
        || selection_authority.members != selection.authority_members
        || authority != &prepared.authority_before
        || !flight_target_still_current(world, builds, prepared.target)
        || selection
            .units
            .iter()
            .any(|m| !unit_still_current(world, paths, &m.before))
    {
        return Err(CanonicalPatrolFlightError::StaleCanonicalState);
    }
    *groups = selection.groups_after.clone();
    *command_state = selection.command_state_after.clone();
    for mutation in &selection.units {
        let row = world
            .row_of(mutation.before.identity.handle)
            .expect("revalidated actor");
        world.units.group_mut()[row] = mutation.after.group;
        *world.orders_mut(row) = mutation.after.orders.clone();
        paths[row] = mutation.after.path.clone();
    }
    Ok(prepared.receipt)
}

impl CanonicalPatrolReceipt {
    pub fn validates(&self) -> bool {
        let Ok(group) = crate::systems::air_group_action_transaction::decode_group_selection_packet(
            &self.group_packet,
        ) else {
            return false;
        };
        decode_patrol_request(&self.patrol_packet) == Ok(self.request)
            && self.position.action_command_index == self.position.group_command_index + 1
            && group.requested.is_empty()
            && self.map_tiles.0 > 0
            && self.map_tiles.1 > 0
            && self.destination
                == (
                    clamp_axis(self.request.x, self.map_tiles.0),
                    clamp_axis(self.request.y, self.map_tiles.1),
                )
            && !self.actors.is_empty()
            && self.actors.iter().all(|actor| {
                let Some(strafe) = actor.before.strafe.as_ref() else {
                    return false;
                };
                let expected = Order::air_patrol(
                    AirPatrolOrderPayload::one(
                        actor.patrol_point.0,
                        actor.patrol_point.1,
                        strafe.air.oxx,
                        strafe.air.whose,
                    ),
                    true,
                );
                actor.identity.who == group.owner
                    && current_strafe_is_coherent(&actor.before)
                    && expected == Ok(actor.after.clone())
                    && actor.after_path == PathStack::default()
            })
            && {
                let mut handles = std::collections::HashSet::new();
                self.actors
                    .iter()
                    .all(|actor| handles.insert(actor.identity.handle))
            }
            && self.command_state_revision_after
                == self.command_state_revision_before.wrapping_add(1)
    }
}

impl CanonicalFreshFlightReceipt {
    pub fn validates(&self) -> bool {
        let Ok(group) = crate::systems::air_group_action_transaction::decode_group_selection_packet(
            &self.group_packet,
        ) else {
            return false;
        };
        decode_flight_strafe_request(&self.flight_packet) == Ok(self.request)
            && self.position.group_command_index == self.patrol_position.action_command_index + 1
            && self.position.game_frame == self.patrol_position.game_frame
            && self.position.package_serial == self.patrol_position.package_serial
            && self.position.play == self.patrol_position.play
            && group.requested.is_empty()
            && self.target.is_well_formed()
            && (i32::from(self.target.owner), self.target.o)
                == (self.request.target_who, self.request.target_o)
            && !self.actors.is_empty()
            && self.actors.iter().all(|actor| {
                let Some(patrol) = actor.before.air_patrol.as_ref() else {
                    return false;
                };
                let expected = Order::strafe(
                    StrafeOrder {
                        target_o: self.target.o,
                        target_who: i32::from(self.target.owner),
                        target_uid: self.target_uid,
                        def_x: 0,
                        def_y: 0,
                        mandatory: 1,
                        defensive: 0,
                        in_range: 0,
                        ever_in_range: 0,
                        new_ord: 0,
                        air: crate::systems::air::AirOrderWalk {
                            oxx: patrol.air.home_o,
                            whose: patrol.air.home_who,
                            cruising_alt: 0x640,
                            sharp_turn: 0,
                            old: 0,
                            returning: 0,
                        },
                        xx: self.target_position.0,
                        yy: self.target_position.1,
                    },
                    true,
                );
                actor.identity.who == group.owner
                    && actor.before.kind == OrderIndex::AirPatrol
                    && expected == Ok(actor.after.clone())
                    && actor.before_path == PathStack::default()
                    && actor.after_path == PathStack::default()
            })
            && {
                let mut handles = std::collections::HashSet::new();
                self.actors
                    .iter()
                    .all(|actor| handles.insert(actor.identity.handle))
            }
            && self.command_state_revision_after
                == self.command_state_revision_before.wrapping_add(1)
    }
}
