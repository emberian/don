// SPDX-License-Identifier: GPL-3.0-or-later
//! Canonical `GroupCommand` plus one simple Group action transaction.
//!
//! The initial production arm is opcode 32, `UNITMASK`. It reuses the fixed-`Groups`
//! selector/cache/allocator from [`canonical_group_move_host`], plans the exact recovered
//! `Group::action_unitmask` body, and publishes every reached Group, backlink, Unit, order,
//! path, player-map, clock, and RNG surface at one stale-checked boundary. No
//! `command::Bridge` or shadow `command::Groups` state is observed.

use crate::systems::canonical_group_move_host::{
    groups_equal, prepare_group_selection, unit_still_current, CommandPackageState,
    GroupMoveAuthority, GroupSelectionUse, PackageError, UnitIdentity, UnitMutation,
    NETWORK_PLAYERS, RECEIVED_SELECTION_CAPACITY,
};
use crate::systems::groups_guys::{
    plan_action_unitmask, CheckSum, Groups, UnitMaskMemberFacts, UnitMaskStep,
};
use crate::systems::movement::PathStack;
use crate::systems::sparse_object_bands_authority_frontier::UNIT_BAND_LIMIT;
use crate::world::{Handle, World};

pub const GROUP_OPCODE: u8 = 0;
pub const UNITMASK_OPCODE: u8 = 32;
pub const UNITMASK_WIRE_SIZE: usize = 9;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimpleGroupActionWire {
    UnitMask { mask: u32, set: i32 },
}

impl SimpleGroupActionWire {
    pub const fn opcode(self) -> u8 {
        match self {
            Self::UnitMask { .. } => UNITMASK_OPCODE,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimpleGroupWire {
    pub who: u8,
    pub objects: Vec<i16>,
    pub action: SimpleGroupActionWire,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SimpleGroupPackageError {
    Truncated,
    WrongFirstOpcode { got: u8 },
    UnsupportedActionOpcode { got: u8 },
    TrailingBytes { expected: usize, got: usize },
    Selection(PackageError),
    UnsupportedSelectionBand { o: i16 },
    BrokenPlanIdentity { who: u8, o: i16 },
    StalePlayerMap,
    StaleFrame,
    StaleRng,
    StaleUnitFlags { handle: Handle },
    StaleUnitDestination { handle: Handle },
}

impl From<PackageError> for SimpleGroupPackageError {
    fn from(error: PackageError) -> Self {
        Self::Selection(error)
    }
}

#[inline]
fn read_i16(bytes: &[u8], at: usize) -> Option<i16> {
    Some(i16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

#[inline]
fn read_i32(bytes: &[u8], at: usize) -> Option<i32> {
    Some(i32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

#[inline]
fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// Decode exactly `[Group][Unitmask]`. Prefixes, suffixes, and a second action are refused.
pub fn decode_simple_group_package(
    bytes: &[u8],
) -> Result<SimpleGroupWire, SimpleGroupPackageError> {
    let Some(&first) = bytes.first() else {
        return Err(SimpleGroupPackageError::Truncated);
    };
    if first != GROUP_OPCODE {
        return Err(SimpleGroupPackageError::WrongFirstOpcode { got: first });
    }
    let Some(&count) = bytes.get(1) else {
        return Err(SimpleGroupPackageError::Truncated);
    };
    if usize::from(count) > RECEIVED_SELECTION_CAPACITY {
        return Err(PackageError::ExplicitSelectionTooLong {
            len: usize::from(count),
        }
        .into());
    }
    let Some(&who) = bytes.get(2) else {
        return Err(SimpleGroupPackageError::Truncated);
    };
    let group_len = 3usize + usize::from(count) * 2;
    let Some(&opcode) = bytes.get(group_len) else {
        return Err(SimpleGroupPackageError::Truncated);
    };
    if opcode != UNITMASK_OPCODE {
        return Err(SimpleGroupPackageError::UnsupportedActionOpcode { got: opcode });
    }
    let expected = group_len + UNITMASK_WIRE_SIZE;
    if bytes.len() < expected {
        return Err(SimpleGroupPackageError::Truncated);
    }
    if bytes.len() != expected {
        return Err(SimpleGroupPackageError::TrailingBytes {
            expected,
            got: bytes.len(),
        });
    }
    let mut objects = Vec::with_capacity(usize::from(count));
    for index in 0..usize::from(count) {
        let o = read_i16(bytes, 3 + index * 2).ok_or(SimpleGroupPackageError::Truncated)?;
        if o < 0 {
            return Err(PackageError::NegativeObject { o }.into());
        }
        objects.push(o);
    }
    Ok(SimpleGroupWire {
        who,
        objects,
        action: SimpleGroupActionWire::UnitMask {
            mask: read_u32(bytes, group_len + 1).ok_or(SimpleGroupPackageError::Truncated)?,
            set: read_i32(bytes, group_len + 5).ok_or(SimpleGroupPackageError::Truncated)?,
        },
    })
}

#[derive(Clone, Debug)]
pub struct SimpleUnitMutation {
    pub unit: UnitMutation,
    pub flags_before: u8,
    pub flags_after: u8,
    pub dest_angle_before: i32,
    pub dest_angle_after: i32,
}

#[derive(Clone, Debug)]
pub struct PreparedSimpleGroupPackage {
    pub play: usize,
    pub lockstep_serial: i32,
    pub frame: i32,
    pub random_state: i32,
    pub player_who_before: [Option<u8>; NETWORK_PLAYERS],
    pub wire: SimpleGroupWire,
    pub group_slot: usize,
    pub selected: Vec<UnitIdentity>,
    pub command_state_before: CommandPackageState,
    pub command_state_after: CommandPackageState,
    pub groups_before: Groups,
    pub groups_after: Groups,
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub authority_members: Vec<crate::systems::canonical_group_move_host::MoveMemberAuthority>,
    pub units: Vec<SimpleUnitMutation>,
    pub final_set: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimpleGroupPackageReceipt {
    pub play: usize,
    pub lockstep_serial: i32,
    pub frame: i32,
    pub who: u8,
    pub opcode: u8,
    pub group_slot: usize,
    pub selected: Vec<UnitIdentity>,
    pub command_state_revision: u64,
    pub groups_checksum: u32,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub final_set: bool,
}

fn mutation_for(
    mutations: &mut [SimpleUnitMutation],
    who: u8,
    o: i16,
) -> Result<&mut SimpleUnitMutation, SimpleGroupPackageError> {
    mutations
        .iter_mut()
        .find(|mutation| {
            mutation.unit.after.identity.who == who && mutation.unit.after.identity.o == o
        })
        .ok_or(SimpleGroupPackageError::BrokenPlanIdentity { who, o })
}

/// Prepare opcode-0 selection and the exact UNITMASK body against detached after-images.
#[allow(clippy::too_many_arguments)]
pub fn prepare_simple_group_package(
    world: &World,
    groups: &Groups,
    paths: &[PathStack],
    command_state: &CommandPackageState,
    authority: &GroupMoveAuthority,
    player_who: &[Option<u8>; NETWORK_PLAYERS],
    frame: i32,
    play: usize,
    lockstep_serial: i32,
    bytes: &[u8],
) -> Result<PreparedSimpleGroupPackage, SimpleGroupPackageError> {
    let wire = decode_simple_group_package(bytes)?;
    if play >= NETWORK_PLAYERS {
        return Err(PackageError::PlayOutOfRange { play }.into());
    }
    let expected_who = player_who[play].ok_or(PackageError::MissingPlayerMap { play })?;
    if expected_who != wire.who {
        return Err(PackageError::PlayerOwnerMismatch {
            play,
            expected: expected_who,
            got: wire.who,
        }
        .into());
    }
    let admitted_objects = if wire.objects.is_empty() {
        command_state.selection(play).unwrap_or_default()
    } else {
        &[]
    };
    if let Some(o) = wire
        .objects
        .iter()
        .copied()
        .chain(admitted_objects.iter().map(|entry| entry.o))
        .find(|&o| i32::from(o) >= UNIT_BAND_LIMIT)
    {
        return Err(SimpleGroupPackageError::UnsupportedSelectionBand { o });
    }
    let selection = prepare_group_selection(
        world,
        groups,
        paths,
        command_state,
        authority,
        frame,
        play,
        wire.who,
        &wire.objects,
        GroupSelectionUse::SimpleUnitState,
    )?;
    let group_slot = selection.group_slot;
    let mut units = Vec::with_capacity(selection.units.len());
    for unit in selection.units {
        let row = world
            .row_of(unit.before.identity.handle)
            .ok_or(PackageError::StaleUnit {
                handle: unit.before.identity.handle,
            })?;
        let flags = world.units.get_flags(row);
        let dest_angle = world.units.dest_angle()[row];
        units.push(SimpleUnitMutation {
            unit,
            flags_before: flags,
            flags_after: flags,
            dest_angle_before: dest_angle,
            dest_angle_after: dest_angle,
        });
    }

    let mut groups_after = selection.groups_after;
    let group = groups_after
        .list
        .get(group_slot)
        .ok_or(PackageError::InvalidGroupPool)?
        .clone();
    let SimpleGroupActionWire::UnitMask { mask, set } = wire.action;
    let mut facts = Vec::with_capacity(selection.members.len());
    for member in &selection.members {
        let mutation = mutation_for(&mut units, member.identity.who, member.identity.o)?;
        facts.push(UnitMaskMemberFacts {
            o: member.identity.o,
            valid_unit: true,
            is_plane: member.authority.is_plane,
            unit_masks: mutation.unit.after.unit_masks,
        });
    }
    let plan = plan_action_unitmask(&group, mask, set, &facts).map_err(|_| {
        SimpleGroupPackageError::BrokenPlanIdentity {
            who: wire.who,
            o: -1,
        }
    })?;
    groups_after.list[group_slot] = plan.group;
    for step in plan.steps {
        let (who, o) = match step {
            UnitMaskStep::WriteUnitMasks { who, o, .. }
            | UnitMaskStep::SetObjectFlag { who, o, .. }
            | UnitMaskStep::ClearUnitMasks { who, o, .. }
            | UnitMaskStep::ClearPathAnchor { who, o }
            | UnitMaskStep::CloseOrders { who, o, .. }
            | UnitMaskStep::ClearPartialPath { who, o }
            | UnitMaskStep::UpdateAction { who, o } => (who, o),
        };
        let mutation = mutation_for(&mut units, who, o)?;
        match step {
            UnitMaskStep::WriteUnitMasks { value, .. } => mutation.unit.after.unit_masks = value,
            UnitMaskStep::SetObjectFlag { mask, .. } => mutation.flags_after |= mask,
            UnitMaskStep::ClearUnitMasks { mask, .. } => mutation.unit.after.unit_masks &= !mask,
            UnitMaskStep::ClearPathAnchor { .. } | UnitMaskStep::ClearPartialPath { .. } => {
                mutation.unit.after.path.clear()
            }
            UnitMaskStep::CloseOrders { .. } => mutation.unit.after.orders.clear(),
            UnitMaskStep::UpdateAction { .. } => {
                mutation.unit.after.orders_x = mutation.unit.after.x;
                mutation.unit.after.orders_y = mutation.unit.after.y;
                mutation.dest_angle_after = mutation.unit.after.angle;
            }
        }
    }

    Ok(PreparedSimpleGroupPackage {
        play,
        lockstep_serial,
        frame,
        random_state: world.random.state(),
        player_who_before: *player_who,
        wire,
        group_slot,
        selected: selection
            .members
            .iter()
            .map(|member| member.identity.clone())
            .collect(),
        command_state_before: selection.command_state_before,
        command_state_after: selection.command_state_after,
        groups_before: selection.groups_before,
        groups_after,
        authority_revision: selection.authority_revision,
        authority_digest: selection.authority_digest,
        authority_members: selection.authority_members,
        units,
        final_set: plan.final_set,
    })
}

/// Revalidate every reached owner and then publish only assignments.
pub fn commit_simple_group_package(
    world: &mut World,
    groups: &mut Groups,
    paths: &mut [PathStack],
    command_state: &mut CommandPackageState,
    authority: &GroupMoveAuthority,
    player_who: &[Option<u8>; NETWORK_PLAYERS],
    prepared: PreparedSimpleGroupPackage,
) -> Result<SimpleGroupPackageReceipt, SimpleGroupPackageError> {
    if player_who != &prepared.player_who_before {
        return Err(SimpleGroupPackageError::StalePlayerMap);
    }
    if world.frame != prepared.frame {
        return Err(SimpleGroupPackageError::StaleFrame);
    }
    if world.random.state() != prepared.random_state {
        return Err(SimpleGroupPackageError::StaleRng);
    }
    if command_state != &prepared.command_state_before {
        return Err(PackageError::StaleCommandState.into());
    }
    if !groups_equal(groups, &prepared.groups_before) {
        return Err(PackageError::StaleGroups.into());
    }
    if authority.revision != prepared.authority_revision
        || authority.composition_digest != prepared.authority_digest
        || authority.members != prepared.authority_members
    {
        return Err(PackageError::StaleAuthority.into());
    }
    for mutation in &prepared.units {
        if !unit_still_current(world, paths, &mutation.unit.before) {
            return Err(PackageError::StaleUnit {
                handle: mutation.unit.before.identity.handle,
            }
            .into());
        }
        let row = world
            .row_of(mutation.unit.before.identity.handle)
            .expect("unit_still_current resolved this handle");
        if world.units.get_flags(row) != mutation.flags_before {
            return Err(SimpleGroupPackageError::StaleUnitFlags {
                handle: mutation.unit.before.identity.handle,
            });
        }
        if world.units.dest_angle()[row] != mutation.dest_angle_before {
            return Err(SimpleGroupPackageError::StaleUnitDestination {
                handle: mutation.unit.before.identity.handle,
            });
        }
    }

    let mut checksum = CheckSum::default();
    prepared.groups_after.check_groups(&mut checksum);
    let receipt = SimpleGroupPackageReceipt {
        play: prepared.play,
        lockstep_serial: prepared.lockstep_serial,
        frame: prepared.frame,
        who: prepared.wire.who,
        opcode: prepared.wire.action.opcode(),
        group_slot: prepared.group_slot,
        selected: prepared.selected.clone(),
        command_state_revision: prepared.command_state_after.revision(),
        groups_checksum: checksum.value,
        random_state_before: prepared.random_state,
        random_state_after: prepared.random_state,
        final_set: prepared.final_set,
    };
    *groups = prepared.groups_after;
    *command_state = prepared.command_state_after;
    for mutation in prepared.units {
        let row = world
            .row_of(mutation.unit.before.identity.handle)
            .expect("all handles were revalidated before publication");
        world.units.group_mut()[row] = mutation.unit.after.group;
        world
            .units
            .set_unit_masks(row, mutation.unit.after.unit_masks);
        world.units.set_flags(row, mutation.flags_after);
        world.units.orders_x_mut()[row] = mutation.unit.after.orders_x;
        world.units.orders_y_mut()[row] = mutation.unit.after.orders_y;
        world.units.dest_angle_mut()[row] = mutation.dest_angle_after;
        *world.orders_mut(row) = mutation.unit.after.orders;
        paths[row] = mutation.unit.after.path;
    }
    Ok(receipt)
}
