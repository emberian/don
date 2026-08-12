// SPDX-License-Identifier: GPL-3.0-or-later
//! Canonical `[Group][LaunchPatrol|Scramble]` package transaction.
//!
//! Opcode 0 is prepared by the fixed-`Groups` selector. The resulting group image, exact
//! containment chain and Handle-bound type facts are then fed through the landed
//! `air_group_action_transaction`. Its atomic host publishes the selection cache, fixed Group,
//! backlinks, order queues and paths together; the shadow command bridge is never observed.

use crate::command::air_launch_receivers::{
    plan_launch_patrol, plan_scramble, AirLaunchFacts, AirLaunchInstall, ContainedFacts,
    MemberFacts,
};
use crate::order::{OrderIndex, ORDER_GROUP};
use crate::systems::air_group_action_transaction as transaction;
use crate::systems::canonical_group_move_host::{
    build_still_current, ensure_mutation_for_row, groups_equal, prepare_air_group_selection,
    unit_still_current, BuildSelectionAuthority, CommandPackageState, GroupMoveAuthority,
    PackageError, PreparedGroupSelection, PreparedSelectionObject, UnitImage, NETWORK_PLAYERS,
};
use crate::systems::groups_guys::{
    CheckSum, GroupData, Groups, GROUPS_PER_PLAYER, GROUP_MAX_MEMBERS,
};
use crate::systems::movement::PathStack;
use crate::systems::order_dispatch::{
    self, clear_partial_path, install_air_patrol, update_action, OrderRec, UnitWork,
};
use crate::systems::production::BuildData;
use crate::systems::sparse_object_bands_authority_frontier::{RetailBand, RetailObjectAddress};
use crate::world::{Handle, World, WorldObjectIdentity, OBJ_FLAG_ACTIVE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AirGroupUnitAuthority {
    pub handle: Handle,
    pub object_masks: u32,
    pub busy: bool,
    pub is_biplane: bool,
    pub is_bomber: bool,
    pub is_helicopter: bool,
}

/// Reinstalled facts not carried by generated Unit columns or `GroupMoveAuthority`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AirGroupRuntimeAuthority {
    pub revision: u64,
    pub scenario_revision: u64,
    pub composition_digest: [u8; 32],
    pub ignore_orders: bool,
    pub units: Vec<AirGroupUnitAuthority>,
    pub builds: Vec<BuildSelectionAuthority>,
}

impl AirGroupRuntimeAuthority {
    fn unit(&self, handle: Handle) -> Option<AirGroupUnitAuthority> {
        self.units
            .iter()
            .copied()
            .find(|entry| entry.handle == handle)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalAirPackageError {
    Truncated,
    TrailingBytes { expected: usize, actual: usize },
    Pair(transaction::AirGroupPairError),
    Selection(PackageError),
    PlayOutOfRange { play: usize },
    MissingPlayerMap { play: usize },
    PlayerOwnerMismatch { play: usize, expected: u8, got: u8 },
    MissingCompositionDigest,
    DuplicateAuthorityUnit(Handle),
    MissingAirAuthority(Handle),
    MissingMoveAuthority(Handle),
    StaleAirAuthority(Handle),
    InvalidContainment { who: i8, o: i16 },
    ContainmentCycle { who: u8, o: i16 },
    NestedContainerUnsupported { who: i8, o: i16 },
    NoInstalls,
    Planner(crate::command::air_launch_receivers::AirLaunchBoundary),
}

impl From<PackageError> for CanonicalAirPackageError {
    fn from(error: PackageError) -> Self {
        Self::Selection(error)
    }
}

#[derive(Clone, Debug)]
pub struct PreparedCanonicalAirPackage {
    pub play: usize,
    pub lockstep_serial: i32,
    pub frame: i32,
    pub selection: PreparedGroupSelection,
    pub request: transaction::AirGroupActionRequest,
    pub snapshot: transaction::AirGroupActionSnapshot,
    pub authority_snapshot: AirGroupRuntimeAuthority,
}

fn decode_package_parts(bytes: &[u8]) -> Result<(&[u8], &[u8]), CanonicalAirPackageError> {
    if bytes.len() < 3 {
        return Err(CanonicalAirPackageError::Truncated);
    }
    let group_len = 3usize
        .checked_add(usize::from(bytes[1]).saturating_mul(2))
        .ok_or(CanonicalAirPackageError::Truncated)?;
    if group_len >= bytes.len() {
        return Err(CanonicalAirPackageError::Truncated);
    }
    let action_len = match bytes[group_len] {
        transaction::LAUNCH_PATROL_OPCODE => {
            crate::command::air_launch_receivers::LAUNCH_PATROL_WIRE_SIZE
        }
        transaction::SCRAMBLE_OPCODE => crate::command::air_launch_receivers::SCRAMBLE_WIRE_SIZE,
        _ => bytes.len() - group_len,
    };
    let expected = group_len.saturating_add(action_len);
    if bytes.len() != expected {
        return Err(CanonicalAirPackageError::TrailingBytes {
            expected,
            actual: bytes.len(),
        });
    }
    Ok(bytes.split_at(group_len))
}

pub fn decode_canonical_air_package(
    frame: i32,
    play: usize,
    lockstep_serial: i32,
    bytes: &[u8],
) -> Result<transaction::AirGroupPacketPair, CanonicalAirPackageError> {
    let (group, action) = decode_package_parts(bytes)?;
    transaction::decode_air_group_packet_pair(
        transaction::CommandPackagePosition {
            game_frame: frame,
            package_serial: lockstep_serial as u32,
            play: play as i32,
            group_command_index: 0,
            action_command_index: 1,
        },
        group,
        action,
    )
    .map_err(CanonicalAirPackageError::Pair)
}

fn unit_identity(
    world: &World,
    row: usize,
) -> Result<transaction::CanonicalObjectIdentity, CanonicalAirPackageError> {
    let handle = world
        .handle_at_row(row)
        .ok_or(CanonicalAirPackageError::InvalidContainment {
            who: world.units.get_who(row) as i8,
            o: world.units.o()[row],
        })?;
    Ok(transaction::CanonicalObjectIdentity {
        owner: world.units.get_who(row),
        band: transaction::CanonicalObjectBand::Unit,
        o: i32::from(world.units.o()[row]),
        generation: transaction::CanonicalObjectGeneration::Unit {
            id: handle.id,
            generation: handle.generation,
        },
    })
}

fn group_walk_image(group: &GroupData) -> Vec<u8> {
    let n = group.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
    let mut image = group.header_bytes().to_vec();
    for value in &group.list[..n] {
        image.extend_from_slice(&value.to_le_bytes());
    }
    for values in [&group.off_x, &group.off_y, &group.curr_x, &group.curr_y] {
        for value in &values[..n] {
            image.extend_from_slice(&value.to_le_bytes());
        }
    }
    image.extend(group.angles[..n].iter().map(|value| *value as u8));
    image
}

fn group_before_image(
    world: &World,
    selection: &PreparedGroupSelection,
) -> Result<transaction::GroupBeforeImage, CanonicalAirPackageError> {
    let group = &selection.groups_after.list[selection.group_slot];
    let n = group.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
    let mut members = Vec::with_capacity(n);
    for member in &selection.selected_objects {
        members.push(match member {
            PreparedSelectionObject::Unit(member) => unit_identity(world, member.row)?,
            PreparedSelectionObject::Build(member) => transaction::CanonicalObjectIdentity {
                owner: member.image.identity.who,
                band: transaction::CanonicalObjectBand::Build,
                o: i32::from(member.image.identity.o),
                generation: transaction::CanonicalObjectGeneration::BuildRow(
                    member.image.identity.row,
                ),
            },
        });
    }
    Ok(transaction::GroupBeforeImage {
        key: transaction::CanonicalGroupKey {
            owner: group.who,
            // The transaction key is owner-local, while `Groups::list` uses a global slot.
            slot: (selection.group_slot % GROUPS_PER_PLAYER) as u8,
            group_id: group.id,
        },
        revision: selection.command_state_after.revision(),
        walk_image: group_walk_image(group),
        members,
    })
}

fn move_authority_for(
    authority: &GroupMoveAuthority,
    handle: Handle,
) -> Option<&crate::systems::canonical_group_move_host::MoveMemberAuthority> {
    authority
        .members
        .iter()
        .find(|entry| entry.handle == handle)
}

fn outer_container(
    world: &World,
    builds: &[BuildData],
    row: usize,
) -> Result<Option<(i16, u8)>, CanonicalAirPackageError> {
    let o = world.units.inside_up()[row];
    if o < 0 {
        return Ok(None);
    }
    let who = world.units.inside_up_who()[row];
    let owner =
        u8::try_from(who).map_err(|_| CanonicalAirPackageError::InvalidContainment { who, o })?;
    if RetailBand::Unit.contains(i32::from(o)) {
        let parent = world
            .unit_row_at(i32::from(owner), i32::from(o))
            .ok_or(CanonicalAirPackageError::InvalidContainment { who, o })?;
        if world.units.inside_up()[parent] >= 0 {
            return Err(CanonicalAirPackageError::NestedContainerUnsupported { who, o });
        }
    } else if RetailBand::Build.contains(i32::from(o)) {
        let address = RetailObjectAddress::new(owner, RetailBand::Build, i32::from(o));
        let WorldObjectIdentity::BuildRow(build_row) = world
            .object_bands()
            .live_identity(address)
            .ok_or(CanonicalAirPackageError::InvalidContainment { who, o })?
        else {
            return Err(CanonicalAirPackageError::InvalidContainment { who, o });
        };
        builds
            .get(build_row as usize)
            .filter(|build| build.who == owner && build.object_id() == o && build.is_valid())
            .ok_or(CanonicalAirPackageError::InvalidContainment { who, o })?;
    } else {
        return Err(CanonicalAirPackageError::InvalidContainment { who, o });
    }
    Ok(Some((o, owner)))
}

fn capture_contained(
    world: &World,
    builds: &[BuildData],
    selection_authority: &GroupMoveAuthority,
    authority: &AirGroupRuntimeAuthority,
    mut next_o: i16,
    mut next_who: i8,
) -> Result<
    (
        Vec<ContainedFacts>,
        Vec<transaction::CanonicalObjectIdentity>,
    ),
    CanonicalAirPackageError,
> {
    let mut facts = Vec::new();
    let mut identities = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    while next_o >= 0 {
        let who =
            u8::try_from(next_who).map_err(|_| CanonicalAirPackageError::InvalidContainment {
                who: next_who,
                o: next_o,
            })?;
        if !seen.insert((who, next_o)) || seen.len() > transaction::MAX_GROUP_MEMBERS {
            return Err(CanonicalAirPackageError::ContainmentCycle { who, o: next_o });
        }
        let row = world.unit_row_at(i32::from(who), i32::from(next_o)).ok_or(
            CanonicalAirPackageError::InvalidContainment {
                who: next_who,
                o: next_o,
            },
        )?;
        if world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
            return Err(CanonicalAirPackageError::InvalidContainment {
                who: next_who,
                o: next_o,
            });
        }
        let handle =
            world
                .handle_at_row(row)
                .ok_or(CanonicalAirPackageError::InvalidContainment {
                    who: next_who,
                    o: next_o,
                })?;
        let air = authority
            .unit(handle)
            .ok_or(CanonicalAirPackageError::MissingAirAuthority(handle))?;
        let movement = move_authority_for(selection_authority, handle)
            .ok_or(CanonicalAirPackageError::MissingMoveAuthority(handle))?;
        let movement_is_helicopter =
            movement.unit_flags & crate::command::air_launch_receivers::HELICOPTER_TYPE_FLAG != 0;
        if movement_is_helicopter != air.is_helicopter {
            return Err(CanonicalAirPackageError::StaleAirAuthority(handle));
        }
        facts.push(ContainedFacts {
            object: (who, next_o),
            is_unit: true,
            domain: movement.domain,
            busy: Some(air.busy),
            object_masks: Some(air.object_masks),
            unit_flags: movement.unit_flags,
            mana_burn: Some(world.units.mana_burn()[row]),
            inside: Some(outer_container(world, builds, row)?),
            is_biplane: Some(air.is_biplane),
            is_bomber: Some(air.is_bomber),
            is_helicopter: Some(air.is_helicopter),
            pos: (world.units.x_internal()[row], world.units.y_internal()[row]),
            busy_order: world
                .orders(row)
                .current()
                .is_some_and(|order| order.kind != OrderIndex::None),
        });
        identities.push(unit_identity(world, row)?);
        next_o = world.units.inside_down()[row];
        next_who = world.units.inside_down_who()[row];
    }
    Ok((facts, identities))
}

fn capture_facts(
    world: &World,
    builds: &[BuildData],
    selection: &PreparedGroupSelection,
    selection_authority: &GroupMoveAuthority,
    authority: &AirGroupRuntimeAuthority,
) -> Result<
    (
        AirLaunchFacts,
        Vec<Vec<transaction::CanonicalObjectIdentity>>,
    ),
    CanonicalAirPackageError,
> {
    let group = &selection.groups_after.list[selection.group_slot];
    let n = group.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
    let mut members = Vec::with_capacity(n);
    let mut identities = Vec::with_capacity(n);
    for selected in &selection.selected_objects {
        let (object, pos, inside_down, inside_down_who) = match selected {
            PreparedSelectionObject::Unit(member) => (
                (member.identity.who, member.identity.o),
                (
                    world.units.x_internal()[member.row],
                    world.units.y_internal()[member.row],
                ),
                world.units.inside_down()[member.row],
                world.units.inside_down_who()[member.row],
            ),
            PreparedSelectionObject::Build(member) => (
                (member.image.identity.who, member.image.identity.o),
                member.image.position,
                member.image.inside_down,
                member.image.inside_down_who,
            ),
        };
        let (contained, contained_identities) = capture_contained(
            world,
            builds,
            selection_authority,
            authority,
            inside_down,
            inside_down_who,
        )?;
        members.push(MemberFacts {
            object,
            pos,
            contained,
            containment_answered: true,
        });
        identities.push(contained_identities);
    }
    Ok((
        AirLaunchFacts {
            who: group.who,
            members,
        },
        identities,
    ))
}

fn order_token(world: &World, row: usize) -> u64 {
    u64::from(crate::checksum::adler32(
        1,
        format!("{:?}", world.orders(row)).as_bytes(),
    ))
}

fn path_token(paths: &[PathStack], row: usize) -> Option<u64> {
    paths
        .get(row)
        .map(|path| u64::from(crate::checksum::adler32(1, &path.walk_bytes())))
}

fn object_position(world: &World, builds: &[BuildData], who: u8, o: i16) -> Option<(i32, i32)> {
    if RetailBand::Unit.contains(i32::from(o)) {
        let row = world.unit_row_at(i32::from(who), i32::from(o))?;
        return Some((world.units.x_internal()[row], world.units.y_internal()[row]));
    }
    if !RetailBand::Build.contains(i32::from(o)) {
        return None;
    }
    let address = RetailObjectAddress::new(who, RetailBand::Build, i32::from(o));
    let WorldObjectIdentity::BuildRow(row) = world.object_bands().live_identity(address)? else {
        return None;
    };
    builds
        .get(row as usize)
        .filter(|build| build.who == who && build.object_id() == o && build.is_valid())
        .map(BuildData::position)
}

fn action_token(world: &World, row: usize) -> u64 {
    let units = &world.units;
    let bytes = format!(
        "{}:{}:{}:{}",
        units.orders_x()[row],
        units.orders_y()[row],
        units.dest_angle()[row],
        units.group()[row]
    );
    u64::from(crate::checksum::adler32(1, bytes.as_bytes()))
}

fn target_before(
    world: &World,
    paths: &[PathStack],
    install: AirLaunchInstall,
) -> Option<transaction::AirOrderTargetBefore> {
    let (who, o) = install.plane();
    let row = world.unit_row_at(i32::from(who), i32::from(o))?;
    Some(transaction::AirOrderTargetBefore {
        identity: unit_identity(world, row).ok()?,
        order_revision: order_token(world, row),
        order_digest: order_token(world, row),
        unit_mask: world.units.get_unit_masks(row),
        action_revision: action_token(world, row),
        path_revision: path_token(paths, row)?,
    })
}

/// Prepare selection plus the landed packet transaction without publishing either half.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn prepare_canonical_air_package(
    world: &World,
    builds: &[BuildData],
    groups: &Groups,
    paths: &[PathStack],
    command_state: &CommandPackageState,
    selection_authority: &GroupMoveAuthority,
    authority: &AirGroupRuntimeAuthority,
    player_who: &[Option<u8>; NETWORK_PLAYERS],
    frame: i32,
    play: usize,
    lockstep_serial: i32,
    bytes: &[u8],
) -> Result<PreparedCanonicalAirPackage, CanonicalAirPackageError> {
    if authority.composition_digest == [0; 32] {
        return Err(CanonicalAirPackageError::MissingCompositionDigest);
    }
    if let Some(duplicate) = authority
        .units
        .iter()
        .enumerate()
        .find_map(|(index, entry)| {
            authority.units[..index]
                .iter()
                .any(|old| old.handle == entry.handle)
                .then_some(entry.handle)
        })
    {
        return Err(CanonicalAirPackageError::DuplicateAuthorityUnit(duplicate));
    }
    let pair = decode_canonical_air_package(frame, play, lockstep_serial, bytes)?;
    if play >= NETWORK_PLAYERS {
        return Err(CanonicalAirPackageError::PlayOutOfRange { play });
    }
    let expected = player_who[play].ok_or(CanonicalAirPackageError::MissingPlayerMap { play })?;
    if expected != pair.group.owner {
        return Err(CanonicalAirPackageError::PlayerOwnerMismatch {
            play,
            expected,
            got: pair.group.owner,
        });
    }
    let selection = prepare_air_group_selection(
        world,
        builds,
        groups,
        paths,
        command_state,
        selection_authority,
        &authority.builds,
        frame,
        play,
        pair.group.owner,
        &pair.group.requested,
    )?;
    let group_before = group_before_image(world, &selection)?;
    let (group_packet, action_packet) = decode_package_parts(bytes)?;
    let revisions = transaction::AuthorityRevisions {
        scenario: authority.scenario_revision,
        types: authority.revision,
        world_orders: world.digest(),
    };
    let request = transaction::AirGroupActionRequest::from_paired_packets(
        pair.position,
        group_packet.to_vec(),
        action_packet.to_vec(),
        group_before.clone(),
        revisions,
    )
    .map_err(CanonicalAirPackageError::Pair)?;
    let (facts, contained_identities) =
        capture_facts(world, builds, &selection, selection_authority, authority)?;
    let installs = match request.command {
        transaction::AirGroupCommand::Scramble => {
            plan_scramble(&facts)
                .map_err(CanonicalAirPackageError::Planner)?
                .installs
        }
        transaction::AirGroupCommand::LaunchPatrol(launch) => {
            plan_launch_patrol(&launch, &facts)
                .map_err(CanonicalAirPackageError::Planner)?
                .installs
        }
    };
    if installs.is_empty() {
        return Err(CanonicalAirPackageError::NoInstalls);
    }
    let target_orders = installs
        .iter()
        .copied()
        .map(|install| {
            target_before(world, paths, install).ok_or_else(|| {
                let (who, o) = install.plane();
                CanonicalAirPackageError::InvalidContainment { who: who as i8, o }
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let cache_revision = request
        .selection
        .is_cached_reselection()
        .then_some(selection.command_state_before.revision());
    let snapshot = transaction::AirGroupActionSnapshot {
        group_before,
        authority: revisions,
        canonical_group_packet: Some(transaction::CanonicalGroupPacketReceipt::for_request(
            &request,
            cache_revision,
        )),
        ignore_orders: if authority.ignore_orders {
            transaction::IgnoreOrdersSnapshot::Armed {
                revision: authority.scenario_revision,
            }
        } else {
            transaction::IgnoreOrdersSnapshot::Clear {
                revision: authority.scenario_revision,
            }
        },
        type_authority: transaction::AirTypeAuthoritySnapshot {
            revision: authority.revision,
            domain_column: true,
            object_masks_column: true,
            unit_flags_column: true,
            nonstrict_type_relations: true,
        },
        integration: transaction::AirIntegrationCapabilities {
            packet_to_sim_route: true,
            move_order_tag_v1: true,
            air_patrol_order_tag_v1: true,
            dynamic_patrol_arrays: true,
            move_unit_work: true,
            air_patrol_unit_work: true,
            move_save_reload_resume: true,
            air_patrol_save_reload_resume: true,
        },
        facts,
        contained_identities,
        target_orders,
    };
    Ok(PreparedCanonicalAirPackage {
        play,
        lockstep_serial,
        frame,
        selection,
        request,
        snapshot,
        authority_snapshot: authority.clone(),
    })
}

fn actor_from_image(image: &UnitImage) -> UnitWork {
    let mut actor = UnitWork::at(image.identity.who, image.identity.o, image.x, image.y);
    actor.uid = image.identity.uid;
    actor.group = image.group;
    actor.unit_masks = image.unit_masks;
    actor.form = image.form;
    actor.body.angle = image.angle;
    actor.orders_x = image.orders_x;
    actor.orders_y = image.orders_y;
    actor.dest_angle = image.dest_angle;
    actor.orders = order_dispatch::adopt(&image.orders);
    actor.path = image.path.clone();
    actor
}

fn image_from_actor(image: &mut UnitImage, actor: &UnitWork) {
    image.group = actor.group;
    image.unit_masks = actor.unit_masks;
    image.form = actor.form;
    image.angle = actor.body.angle;
    image.orders_x = actor.orders_x;
    image.orders_y = actor.orders_y;
    image.dest_angle = actor.dest_angle;
    order_dispatch::publish(&actor.orders, &mut image.orders);
    image.path = actor.path.clone();
}

struct CanonicalAirCommitHost<'a> {
    world: &'a mut World,
    builds: &'a [BuildData],
    groups: &'a mut Groups,
    paths: &'a mut [PathStack],
    command_state: &'a mut CommandPackageState,
    selection_authority: &'a GroupMoveAuthority,
    authority: &'a AirGroupRuntimeAuthority,
    prepared: &'a PreparedCanonicalAirPackage,
}

type AirCheckpoint = (World, Groups, Vec<PathStack>, CommandPackageState);

impl CanonicalAirCommitHost<'_> {
    fn current_target(&self, before: &transaction::AirOrderTargetBefore) -> bool {
        let (who, o) = before.identity.address();
        let Some(row) = self.world.unit_row_at(i32::from(who), i32::from(o)) else {
            return false;
        };
        unit_identity(self.world, row).ok() == Some(before.identity)
            && order_token(self.world, row) == before.order_revision
            && self.world.units.get_unit_masks(row) == before.unit_mask
            && action_token(self.world, row) == before.action_revision
            && path_token(self.paths, row) == Some(before.path_revision)
    }
}

impl transaction::AtomicAirGroupActionHost for CanonicalAirCommitHost<'_> {
    type Checkpoint = AirCheckpoint;

    fn checkpoint(&self) -> Self::Checkpoint {
        (
            self.world.clone(),
            self.groups.clone(),
            self.paths.to_vec(),
            self.command_state.clone(),
        )
    }

    fn state_digest(&self) -> u64 {
        let mut checksum = CheckSum::default();
        self.groups.check_groups(&mut checksum);
        let mut state = self.world.digest()
            ^ u64::from(checksum.value).rotate_left(17)
            ^ self.command_state.revision().rotate_left(31);
        for path in self.paths.iter() {
            state ^= u64::from(crate::checksum::adler32(1, &path.walk_bytes())).rotate_left(7);
            state = state.rotate_left(9).wrapping_mul(0x0000_0100_0000_01b3);
        }
        state
    }

    fn revalidate(
        &self,
        prepared: &transaction::PreparedAirGroupAction,
    ) -> Result<(), transaction::AirCommitFailure> {
        let selection = &self.prepared.selection;
        if self.command_state != &selection.command_state_before
            || !groups_equal(self.groups, &selection.groups_before)
        {
            return Err(transaction::AirCommitFailure::StaleGroup);
        }
        if self.selection_authority.revision != selection.authority_revision
            || self.selection_authority.composition_digest != selection.authority_digest
            || self.selection_authority.members != selection.authority_members
        {
            return Err(transaction::AirCommitFailure::StaleTypes);
        }
        if self.authority != &self.prepared.authority_snapshot
            || prepared.request.authority.world_orders != self.world.digest()
        {
            return Err(transaction::AirCommitFailure::StaleWorldOrders);
        }
        if selection
            .units
            .iter()
            .any(|mutation| !unit_still_current(self.world, self.paths, &mutation.before))
        {
            return Err(transaction::AirCommitFailure::StaleGroup);
        }
        if selection
            .selected_objects
            .iter()
            .any(|member| match member {
                PreparedSelectionObject::Unit(_) => false,
                PreparedSelectionObject::Build(member) => {
                    !build_still_current(self.world, self.builds, &member.image)
                }
            })
        {
            return Err(transaction::AirCommitFailure::StaleGroup);
        }
        if let Some(stale) = prepared
            .snapshot
            .target_orders
            .iter()
            .find(|before| !self.current_target(before))
        {
            return Err(transaction::AirCommitFailure::StaleTarget(stale.identity));
        }
        Ok(())
    }

    fn commit(
        &mut self,
        prepared: &transaction::PreparedAirGroupAction,
    ) -> Result<Vec<transaction::CommittedAirInstall>, transaction::AirCommitFailure> {
        let mut selection = self.prepared.selection.clone();
        let mut committed = Vec::with_capacity(prepared.plan.installs().len());
        for (install, before) in prepared
            .plan
            .installs()
            .iter()
            .copied()
            .zip(&prepared.snapshot.target_orders)
        {
            let (who, o) = install.plane();
            let row = self
                .world
                .unit_row_at(i32::from(who), i32::from(o))
                .ok_or(transaction::AirCommitFailure::StaleTarget(before.identity))?;
            let mutation_index =
                ensure_mutation_for_row(&mut selection.units, self.world, self.paths, row)
                    .map_err(|_| transaction::AirCommitFailure::HostRejected("target image"))?;
            let mutation = &mut selection.units[mutation_index];
            let mut actor = actor_from_image(&mutation.after);
            match install {
                AirLaunchInstall::AirPatrol {
                    x,
                    y,
                    home,
                    group_flag,
                    ..
                } => {
                    let (home_o, home_who, home_pos) = match home {
                        None => (-1, -1, None),
                        Some((home_o, home_who)) => (
                            i32::from(home_o),
                            i32::from(home_who),
                            Some(
                                object_position(self.world, self.builds, home_who, home_o).ok_or(
                                    transaction::AirCommitFailure::HostRejected("home target"),
                                )?,
                            ),
                        ),
                    };
                    install_air_patrol(
                        &mut actor,
                        x,
                        y,
                        home_o,
                        home_who,
                        home_pos,
                        group_flag,
                        crate::command::QueuePos::New,
                    );
                }
                AirLaunchInstall::MoveFacing {
                    stored,
                    angle,
                    clear_unit_mask,
                    ..
                } => {
                    actor.unit_masks &= !clear_unit_mask;
                    let tile = crate::systems::movement::TILE;
                    actor.orders.replace(OrderRec {
                        kind: OrderIndex::MoveTo,
                        flags: ORDER_GROUP,
                        x: stored.0,
                        y: stored.1,
                        angle,
                        dest_x: stored.0,
                        dest_y: stored.1,
                        last_x: -1,
                        last_y: -1,
                        facing: -1,
                        orig_x: -1,
                        orig_y: -1,
                        off_x: (stored.0 % tile) as i16,
                        off_y: (stored.1 % tile) as i16,
                        ..OrderRec::default()
                    });
                    clear_partial_path(&mut actor);
                    update_action(&mut actor);
                }
            }
            image_from_actor(&mut mutation.after, &actor);
            let after_order = u64::from(crate::checksum::adler32(
                1,
                format!("{:?}", mutation.after.orders).as_bytes(),
            ));
            committed.push(transaction::CommittedAirInstall {
                identity: before.identity,
                kind: install.kind(),
                order_revision_after: if after_order == before.order_revision {
                    after_order.wrapping_add(1)
                } else {
                    after_order
                },
                order_digest_after: after_order,
                unit_mask_after: mutation.after.unit_masks,
                action_revision_after: before.action_revision.wrapping_add(1),
                path_revision_after: before.path_revision.wrapping_add(1),
            });
        }

        *self.groups = selection.groups_after;
        *self.command_state = selection.command_state_after;
        for mutation in selection.units {
            let row = self
                .world
                .row_of(mutation.before.identity.handle)
                .expect("every air package identity was revalidated");
            self.world.units.group_mut()[row] = mutation.after.group;
            self.world
                .units
                .set_unit_masks(row, mutation.after.unit_masks);
            self.world.units.orders_x_mut()[row] = mutation.after.orders_x;
            self.world.units.orders_y_mut()[row] = mutation.after.orders_y;
            self.world.units.dest_angle_mut()[row] = mutation.after.dest_angle;
            *self.world.orders_mut(row) = mutation.after.orders;
            self.paths[row] = mutation.after.path;
        }
        Ok(committed)
    }

    fn restore(&mut self, checkpoint: Self::Checkpoint) {
        *self.world = checkpoint.0;
        *self.groups = checkpoint.1;
        self.paths.clone_from_slice(&checkpoint.2);
        *self.command_state = checkpoint.3;
    }
}

/// Execute the already-prepared landed transaction against canonical owners.
pub fn commit_canonical_air_package(
    world: &mut World,
    builds: &[BuildData],
    groups: &mut Groups,
    paths: &mut [PathStack],
    command_state: &mut CommandPackageState,
    selection_authority: &GroupMoveAuthority,
    authority: &AirGroupRuntimeAuthority,
    prepared: PreparedCanonicalAirPackage,
) -> transaction::AirGroupActionReceipt {
    let request = prepared.request.clone();
    let snapshot = prepared.snapshot.clone();
    let mut host = CanonicalAirCommitHost {
        world,
        builds,
        groups,
        paths,
        command_state,
        selection_authority,
        authority,
        prepared: &prepared,
    };
    transaction::execute_air_group_action(&mut host, &request, &snapshot)
}
