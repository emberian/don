// SPDX-License-Identifier: GPL-3.0-or-later
//! Canonical `GroupCommand` plus one simple Group action transaction.
//!
//! The production arms are opcode 32 `UNITMASK`, opcode 29 `STOP_SPELL`, opcode 12
//! `HALT`, the ordinary-Unit arm of opcode 14 `SET_TRANSPORT`, and opcode 33 `BUILDMASK`.
//! They reuse the fixed-`Groups` selector/cache/allocator from [`canonical_group_move_host`], plan the exact recovered
//! `Group::action_unitmask` body, and publishes every reached Group, backlink, Unit, order,
//! path, player-map, clock, and RNG surface at one stale-checked boundary. No
//! `command::Bridge` or shadow `command::Groups` state is observed.

use crate::command::group_action_frontier::{plan_stop_spell, StopSpellMemberFacts, StopSpellStep};
use crate::objects::{Band, BUILD_BAND_BASE, WALL_BAND_BASE};
use crate::systems::canonical_group_move_host::{
    ensure_mutation_for_row, exact_immediate_allocator_slot, groups_equal, prepare_group_selection,
    unit_still_current, CachedSelection, CommandPackageState, GroupMoveAuthority,
    GroupSelectionUse, PackageError, UnitIdentity, UnitMutation, NETWORK_PLAYERS,
    RECEIVED_SELECTION_CAPACITY,
};
use crate::systems::groups_guys::{
    plan_action_buildmask, plan_action_halt, plan_action_set_transport, plan_action_unitmask,
    BuildMaskMemberFacts, BuildMaskStep, CheckSum, GroupData, Groups, HaltMemberFacts, HaltStep,
    SetTransportMemberFacts, SetTransportStep, UnitMaskMemberFacts, UnitMaskStep, NUM_GROUPS,
    NUM_LEADERS,
};
use crate::systems::movement::PathStack;
use crate::systems::production::BuildData;
use crate::systems::sparse_object_bands_authority_frontier::UNIT_BAND_LIMIT;
use crate::world::{Handle, World};

pub const GROUP_OPCODE: u8 = 0;
pub const UNITMASK_OPCODE: u8 = 32;
pub const UNITMASK_WIRE_SIZE: usize = 9;
pub const STOP_SPELL_OPCODE: u8 = 29;
pub const STOP_SPELL_WIRE_SIZE: usize = 1;
pub const HALT_OPCODE: u8 = 12;
pub const HALT_WIRE_SIZE: usize = 1;
pub const SET_TRANSPORT_OPCODE: u8 = 14;
pub const SET_TRANSPORT_WIRE_SIZE: usize = 5;
pub const BUILDMASK_OPCODE: u8 = 33;
pub const BUILDMASK_WIRE_SIZE: usize = 9;

/// Handle-bound capability facts read only by the SET_TRANSPORT arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimpleGroupActionMemberAuthority {
    pub handle: Handle,
    /// Exact `UnitData::can_ever_transport()` result. This cannot be inferred for sea Units
    /// from the movement projection because retail also reads carry capacity and ability 0x15f.
    pub can_ever_transport: bool,
}

/// Stable Build-band capability facts read by `WallData::valid_buildmask`.
///
/// The shipped 100-byte body tests only input bits `0x40` and `0x80`, in that order.
/// Their virtual/type cascades are reinstalled facts; the canonical mask itself remains in
/// [`BuildData`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimpleBuildMaskMemberAuthority {
    pub who: u8,
    pub o: i16,
    pub uid: u16,
    pub row: usize,
    /// Exact `ObjectTypeData::role` folded by `Group::add` into the checksum-visible Group.
    pub role: i32,
    pub admits_0x40: bool,
    pub admits_0x80: bool,
}

impl SimpleBuildMaskMemberAuthority {
    fn valid_buildmask(self, mask: u16) -> bool {
        (mask & 0x40 != 0 && self.admits_0x40) || (mask & 0x80 != 0 && self.admits_0x80)
    }
}

/// Reinstalled action facts which are neither gameplay state nor part of DoNSave.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SimpleGroupActionAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub members: Vec<SimpleGroupActionMemberAuthority>,
    pub builds: Vec<SimpleBuildMaskMemberAuthority>,
}

impl SimpleGroupActionAuthority {
    fn member(&self, handle: Handle) -> Option<&SimpleGroupActionMemberAuthority> {
        self.members.iter().find(|member| member.handle == handle)
    }

    fn build_member(
        &self,
        who: u8,
        o: i16,
        uid: u16,
        row: usize,
    ) -> Option<SimpleBuildMaskMemberAuthority> {
        self.builds
            .iter()
            .copied()
            .find(|member| (member.who, member.o, member.uid, member.row) == (who, o, uid, row))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimpleGroupActionWire {
    UnitMask { mask: u32, set: i32 },
    StopSpell,
    Halt,
    SetTransport { flag: i32 },
    BuildMask { mask: u16, set: i32 },
}

impl SimpleGroupActionWire {
    pub const fn opcode(self) -> u8 {
        match self {
            Self::UnitMask { .. } => UNITMASK_OPCODE,
            Self::StopSpell => STOP_SPELL_OPCODE,
            Self::Halt => HALT_OPCODE,
            Self::SetTransport { .. } => SET_TRANSPORT_OPCODE,
            Self::BuildMask { .. } => BUILDMASK_OPCODE,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimpleGroupActionResult {
    UnitMask {
        final_set: bool,
    },
    StopSpell {
        stopped_units: usize,
    },
    Halt {
        halted_units: usize,
    },
    SetTransport {
        enabled: bool,
        changed_units: usize,
    },
    BuildMask {
        final_set: bool,
        changed_builds: usize,
        feedback: bool,
    },
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
    MissingUnitType { handle: Handle },
    StaleUnitType { handle: Handle },
    StaleUnitSpellTime { handle: Handle },
    MissingStopSpellGpieceAuthority { handle: Handle, type_index: i32 },
    MissingSpecialAnimPayload { handle: Handle },
    MissingSetTransportAuthority { handle: Handle },
    StaleActionAuthority,
    StaleLeaderFlags { who: u8 },
    MissingBuild { who: u8, o: i16 },
    InactiveBuild { who: u8, o: i16 },
    StaleBuild { who: u8, o: i16 },
    MissingBuildMaskAuthority { who: u8, o: i16, uid: u16 },
    StaleBuildRegistry,
    StaleLocalWho,
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

/// Decode exactly `[Group]` plus UNITMASK, STOP_SPELL, HALT, SET_TRANSPORT, or BUILDMASK. Prefixes,
/// suffixes, and a
/// second action are refused.
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
    let action_size = match opcode {
        UNITMASK_OPCODE => UNITMASK_WIRE_SIZE,
        STOP_SPELL_OPCODE => STOP_SPELL_WIRE_SIZE,
        HALT_OPCODE => HALT_WIRE_SIZE,
        SET_TRANSPORT_OPCODE => SET_TRANSPORT_WIRE_SIZE,
        BUILDMASK_OPCODE => BUILDMASK_WIRE_SIZE,
        _ => return Err(SimpleGroupPackageError::UnsupportedActionOpcode { got: opcode }),
    };
    let expected = group_len + action_size;
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
    let action = match opcode {
        UNITMASK_OPCODE => SimpleGroupActionWire::UnitMask {
            mask: read_u32(bytes, group_len + 1).ok_or(SimpleGroupPackageError::Truncated)?,
            set: read_i32(bytes, group_len + 5).ok_or(SimpleGroupPackageError::Truncated)?,
        },
        STOP_SPELL_OPCODE => SimpleGroupActionWire::StopSpell,
        HALT_OPCODE => SimpleGroupActionWire::Halt,
        SET_TRANSPORT_OPCODE => SimpleGroupActionWire::SetTransport {
            flag: read_i32(bytes, group_len + 1).ok_or(SimpleGroupPackageError::Truncated)?,
        },
        BUILDMASK_OPCODE => SimpleGroupActionWire::BuildMask {
            mask: read_u32(bytes, group_len + 1).ok_or(SimpleGroupPackageError::Truncated)? as u16,
            set: read_i32(bytes, group_len + 5).ok_or(SimpleGroupPackageError::Truncated)?,
        },
        _ => unreachable!("action size match admitted this opcode"),
    };
    Ok(SimpleGroupWire {
        who,
        objects,
        action,
    })
}

#[derive(Clone, Debug)]
pub struct SimpleUnitMutation {
    pub unit: UnitMutation,
    pub flags_before: u8,
    pub flags_after: u8,
    pub dest_angle_before: i32,
    pub dest_angle_after: i32,
    pub spell_time_before: Option<i16>,
    pub spell_time_after: Option<i16>,
    pub type_index_before: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimpleBuildIdentity {
    pub who: u8,
    pub o: i16,
    pub uid: u16,
    pub row: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimpleBuildMutation {
    pub identity: SimpleBuildIdentity,
    pub flags_before: u8,
    pub build_masks_before: u16,
    pub build_masks_after: u16,
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
    pub selected_builds: Vec<SimpleBuildIdentity>,
    pub command_state_before: CommandPackageState,
    pub command_state_after: CommandPackageState,
    pub groups_before: Groups,
    pub groups_after: Groups,
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub authority_members: Vec<crate::systems::canonical_group_move_host::MoveMemberAuthority>,
    pub action_authority_before: Option<SimpleGroupActionAuthority>,
    pub leader_flags_before: Option<i32>,
    pub local_who_before: Option<u8>,
    pub units: Vec<SimpleUnitMutation>,
    pub builds: Vec<SimpleBuildMutation>,
    pub action_result: SimpleGroupActionResult,
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
    pub selected_builds: Vec<SimpleBuildIdentity>,
    pub command_state_revision: u64,
    pub groups_checksum: u32,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub action_result: SimpleGroupActionResult,
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

fn build_identity(
    world: &World,
    builds: &[BuildData],
    who: u8,
    o: i16,
) -> Result<SimpleBuildIdentity, SimpleGroupPackageError> {
    if i32::from(o) < BUILD_BAND_BASE as i32 || i32::from(o) >= WALL_BAND_BASE as i32 {
        return Err(SimpleGroupPackageError::MissingBuild { who, o });
    }
    let offset = usize::try_from(i32::from(o) - BUILD_BAND_BASE as i32)
        .expect("Build-band range was checked");
    let row = world
        .objects
        .slot(usize::from(who))
        .band(Band::Build)
        .get(offset)
        .copied()
        .map(|row| row as usize)
        .ok_or(SimpleGroupPackageError::MissingBuild { who, o })?;
    let build = builds
        .get(row)
        .ok_or(SimpleGroupPackageError::MissingBuild { who, o })?;
    if build.who != who || build.object_id() != o {
        return Err(SimpleGroupPackageError::MissingBuild { who, o });
    }
    if build.flags & crate::world::OBJ_FLAG_ACTIVE == 0 {
        return Err(SimpleGroupPackageError::InactiveBuild { who, o });
    }
    Ok(SimpleBuildIdentity {
        who,
        o,
        uid: build.uid,
        row,
    })
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn prepare_buildmask_package(
    world: &World,
    builds: &[BuildData],
    groups: &Groups,
    paths: &[PathStack],
    command_state: &CommandPackageState,
    authority: &GroupMoveAuthority,
    action_authority: &SimpleGroupActionAuthority,
    local_who: Option<u8>,
    player_who: &[Option<u8>; NETWORK_PLAYERS],
    frame: i32,
    play: usize,
    lockstep_serial: i32,
    wire: SimpleGroupWire,
) -> Result<PreparedSimpleGroupPackage, SimpleGroupPackageError> {
    if !world.object_bands_are_dense_equivalent() {
        return Err(SimpleGroupPackageError::StaleBuildRegistry);
    }
    if groups.list.len() != NUM_GROUPS
        || groups
            .list
            .iter()
            .enumerate()
            .any(|(slot, group)| group.id != slot as i32)
    {
        return Err(PackageError::InvalidGroupPool.into());
    }

    let received = if wire.objects.is_empty() {
        command_state.selection(play).unwrap_or_default().to_vec()
    } else {
        let cache = wire
            .objects
            .iter()
            .filter_map(|&o| match build_identity(world, builds, wire.who, o) {
                Ok(identity) => Some(CachedSelection {
                    o,
                    uid: identity.uid,
                }),
                Err(
                    SimpleGroupPackageError::MissingBuild { .. }
                    | SimpleGroupPackageError::InactiveBuild { .. },
                ) => None,
                Err(_) => unreachable!("Build identity has only typed lookup refusals"),
            })
            .collect::<Vec<_>>();
        cache
    };
    let command_state_after = command_state.staged_selection(play, received.clone())?;

    let mut selected = Vec::new();
    for entry in received {
        let identity = match build_identity(world, builds, wire.who, entry.o) {
            Ok(identity) if identity.uid == entry.uid => identity,
            Ok(_)
            | Err(SimpleGroupPackageError::MissingBuild { .. })
            | Err(SimpleGroupPackageError::InactiveBuild { .. }) => continue,
            Err(_) => unreachable!("Build identity has only typed lookup refusals"),
        };
        if selected
            .iter()
            .any(|member: &SimpleBuildIdentity| member.o == identity.o)
        {
            continue;
        }
        if action_authority
            .build_member(identity.who, identity.o, identity.uid, identity.row)
            .is_none()
        {
            return Err(SimpleGroupPackageError::MissingBuildMaskAuthority {
                who: identity.who,
                o: identity.o,
                uid: identity.uid,
            });
        }
        selected.push(identity);
    }
    if selected.is_empty() {
        return Err(PackageError::EmptyEffectiveSelection.into());
    }

    let mut transient = GroupData {
        id: -1,
        army: -1,
        form: -1,
        stamp: frame,
        ..GroupData::default()
    };
    for member in &selected {
        let role = action_authority
            .build_member(member.who, member.o, member.uid, member.row)
            .expect("selection preflight bound every Build authority member")
            .role;
        transient.add(member.o, wire.who, true, role, frame);
    }

    let mut groups_after = groups.clone();
    let current = groups_after.last_group[usize::from(wire.who)];
    let n = transient.num as usize;
    let reuse = usize::try_from(current).ok().is_some_and(|slot| {
        groups_after.list.get(slot).is_some_and(|candidate| {
            candidate.num == transient.num && candidate.list[..n] == transient.list[..n]
        })
    });
    let group_slot = if reuse {
        current as usize
    } else {
        let slot = exact_immediate_allocator_slot(&mut groups_after, wire.who, world, authority)?;
        let id = groups_after.list[slot].id;
        groups_after.list[slot] = transient;
        groups_after.list[slot].id = id;
        groups_after.list[slot].stamp = frame;
        groups_after.last_group[usize::from(wire.who)] = slot as i32;
        slot
    };

    // Replacing an ordinary-Unit group must detach its old canonical backlinks. Builds have
    // no UnitData::group column and therefore require no invented shadow backlink.
    let mut unit_mutations = Vec::new();
    if !reuse && groups.list[group_slot].buildings == 0 {
        for row in 0..world.live_count() as usize {
            if world.units.get_who(row) == wire.who && world.units.group()[row] == group_slot as i16
            {
                let index = ensure_mutation_for_row(&mut unit_mutations, world, paths, row)?;
                unit_mutations[index].after.group = -1;
            }
        }
    }
    let units = unit_mutations
        .into_iter()
        .map(|unit| {
            let row = world
                .row_of(unit.before.identity.handle)
                .expect("selection cleanup resolved this Unit");
            let flags = world.units.get_flags(row);
            let dest_angle = world.units.dest_angle()[row];
            SimpleUnitMutation {
                unit,
                flags_before: flags,
                flags_after: flags,
                dest_angle_before: dest_angle,
                dest_angle_after: dest_angle,
                spell_time_before: None,
                spell_time_after: None,
                type_index_before: None,
            }
        })
        .collect::<Vec<_>>();

    let SimpleGroupActionWire::BuildMask { mask, set } = wire.action else {
        unreachable!("Build-band selector is exclusive to BUILDMASK")
    };
    let mut facts = Vec::with_capacity(selected.len());
    let mut build_mutations = Vec::with_capacity(selected.len());
    for identity in &selected {
        let build = &builds[identity.row];
        let capability = action_authority
            .build_member(identity.who, identity.o, identity.uid, identity.row)
            .expect("selection preflight bound every Build authority member");
        facts.push(BuildMaskMemberFacts {
            o: identity.o,
            valid_build: true,
            valid_buildmask: capability.valid_buildmask(mask),
            build_masks: build.build_masks,
        });
        build_mutations.push(SimpleBuildMutation {
            identity: *identity,
            flags_before: build.flags,
            build_masks_before: build.build_masks,
            build_masks_after: build.build_masks,
        });
    }
    let group = groups_after.list[group_slot].clone();
    let plan = plan_action_buildmask(&group, mask, set, local_who == Some(wire.who), &facts)
        .map_err(|_| SimpleGroupPackageError::BrokenPlanIdentity {
            who: wire.who,
            o: -1,
        })?;
    groups_after.list[group_slot] = plan.group;
    let mut feedback = false;
    let mut changed_builds = 0usize;
    for step in plan.steps {
        match step {
            BuildMaskStep::WriteBuildMasks { who, o, value } => {
                let mutation = build_mutations
                    .iter_mut()
                    .find(|mutation| (mutation.identity.who, mutation.identity.o) == (who, o))
                    .ok_or(SimpleGroupPackageError::BrokenPlanIdentity { who, o })?;
                mutation.build_masks_after = value;
                changed_builds += 1;
            }
            BuildMaskStep::Feedback { .. } => feedback = true,
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
        selected: Vec::new(),
        selected_builds: selected,
        command_state_before: command_state.clone(),
        command_state_after,
        groups_before: groups.clone(),
        groups_after,
        authority_revision: authority.revision,
        authority_digest: authority.composition_digest,
        authority_members: authority.members.clone(),
        action_authority_before: Some(action_authority.clone()),
        leader_flags_before: None,
        local_who_before: local_who,
        units,
        builds: build_mutations,
        action_result: SimpleGroupActionResult::BuildMask {
            final_set: plan.final_set,
            changed_builds,
            feedback,
        },
    })
}

/// Prepare opcode-0 selection and one admitted simple action against detached after-images.
#[allow(clippy::too_many_arguments)]
pub fn prepare_simple_group_package(
    world: &World,
    unit_types: &[i32],
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
    prepare_simple_group_package_with_action_authority(
        world,
        unit_types,
        groups,
        paths,
        command_state,
        authority,
        &SimpleGroupActionAuthority::default(),
        &[0; NUM_LEADERS],
        player_who,
        frame,
        play,
        lockstep_serial,
        bytes,
    )
}

/// Extended preparation entry used by production Sim for actions with extra Handle/type facts.
#[allow(clippy::too_many_arguments)]
pub fn prepare_simple_group_package_with_action_authority(
    world: &World,
    unit_types: &[i32],
    groups: &Groups,
    paths: &[PathStack],
    command_state: &CommandPackageState,
    authority: &GroupMoveAuthority,
    action_authority: &SimpleGroupActionAuthority,
    leader_flags: &[i32; NUM_LEADERS],
    player_who: &[Option<u8>; NETWORK_PLAYERS],
    frame: i32,
    play: usize,
    lockstep_serial: i32,
    bytes: &[u8],
) -> Result<PreparedSimpleGroupPackage, SimpleGroupPackageError> {
    prepare_simple_group_package_with_builds(
        world,
        unit_types,
        &[],
        groups,
        paths,
        command_state,
        authority,
        action_authority,
        leader_flags,
        None,
        player_who,
        frame,
        play,
        lockstep_serial,
        bytes,
    )
}

/// Production preparation entry with canonical Build-band state and local presentation owner.
#[allow(clippy::too_many_arguments)]
pub fn prepare_simple_group_package_with_builds(
    world: &World,
    unit_types: &[i32],
    builds: &[BuildData],
    groups: &Groups,
    paths: &[PathStack],
    command_state: &CommandPackageState,
    authority: &GroupMoveAuthority,
    action_authority: &SimpleGroupActionAuthority,
    leader_flags: &[i32; NUM_LEADERS],
    local_who: Option<u8>,
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
    if usize::from(wire.who) >= NUM_LEADERS {
        return Err(PackageError::OwnerOutOfRange {
            who: wire.who as i8,
        }
        .into());
    }
    let admitted_objects = if wire.objects.is_empty() {
        command_state.selection(play).unwrap_or_default()
    } else {
        &[]
    };
    let addresses = wire
        .objects
        .iter()
        .copied()
        .chain(admitted_objects.iter().map(|entry| entry.o))
        .collect::<Vec<_>>();
    let build_selection = matches!(wire.action, SimpleGroupActionWire::BuildMask { .. })
        && !addresses.is_empty()
        && addresses
            .iter()
            .all(|&o| (BUILD_BAND_BASE as i32..WALL_BAND_BASE as i32).contains(&i32::from(o)));
    if build_selection {
        return prepare_buildmask_package(
            world,
            builds,
            groups,
            paths,
            command_state,
            authority,
            action_authority,
            local_who,
            player_who,
            frame,
            play,
            lockstep_serial,
            wire,
        );
    }
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
            spell_time_before: None,
            spell_time_after: None,
            type_index_before: None,
        });
    }

    let mut groups_after = selection.groups_after;
    let group = groups_after
        .list
        .get(group_slot)
        .ok_or(PackageError::InvalidGroupPool)?
        .clone();
    let mut action_authority_before = None;
    let mut leader_flags_before = None;
    let action_result = match wire.action {
        SimpleGroupActionWire::UnitMask { mask, set } => {
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
                    UnitMaskStep::WriteUnitMasks { value, .. } => {
                        mutation.unit.after.unit_masks = value
                    }
                    UnitMaskStep::SetObjectFlag { mask, .. } => mutation.flags_after |= mask,
                    UnitMaskStep::ClearUnitMasks { mask, .. } => {
                        mutation.unit.after.unit_masks &= !mask
                    }
                    UnitMaskStep::ClearPathAnchor { .. }
                    | UnitMaskStep::ClearPartialPath { .. } => mutation.unit.after.path.clear(),
                    UnitMaskStep::CloseOrders { .. } => mutation.unit.after.orders.clear(),
                    UnitMaskStep::UpdateAction { .. } => {
                        mutation.unit.after.orders_x = mutation.unit.after.x;
                        mutation.unit.after.orders_y = mutation.unit.after.y;
                        mutation.dest_angle_after = mutation.unit.after.angle;
                    }
                }
            }
            SimpleGroupActionResult::UnitMask {
                final_set: plan.final_set,
            }
        }
        SimpleGroupActionWire::StopSpell => {
            let mut facts = Vec::with_capacity(selection.members.len());
            for member in &selection.members {
                let mutation = mutation_for(&mut units, member.identity.who, member.identity.o)?;
                let current_order = mutation.unit.after.orders.current().map(|order| order.kind);
                let reached = member.authority.on_map
                    && current_order == Some(crate::order::OrderIndex::CastSpell);
                let type_index = if reached {
                    let row = world
                        .row_of(mutation.unit.before.identity.handle)
                        .expect("selection preparation resolved this handle");
                    let type_index = unit_types.get(row).copied().ok_or(
                        SimpleGroupPackageError::MissingUnitType {
                            handle: mutation.unit.before.identity.handle,
                        },
                    )?;
                    let spell_time = world.units.spell_time()[row];
                    mutation.spell_time_before = Some(spell_time);
                    mutation.spell_time_after = Some(spell_time);
                    mutation.type_index_before = Some(type_index);
                    type_index
                } else {
                    -1
                };
                facts.push(StopSpellMemberFacts {
                    o: member.identity.o,
                    valid_unit: true,
                    on_map: member.authority.on_map,
                    current_order,
                    unit_masks: mutation.unit.after.unit_masks,
                    type_index,
                });
            }
            if let Some(facts) = facts.iter().find(|facts| {
                facts.on_map
                    && facts.current_order == Some(crate::order::OrderIndex::CastSpell)
                    && matches!(facts.type_index, 61 | 62 | 400)
            }) {
                let mutation = mutation_for(&mut units, wire.who, facts.o)?;
                return Err(SimpleGroupPackageError::MissingStopSpellGpieceAuthority {
                    handle: mutation.unit.before.identity.handle,
                    type_index: mutation
                        .type_index_before
                        .expect("STOP_SPELL preparation captured the type owner"),
                });
            }
            let plan = plan_stop_spell(&group, &facts).map_err(|_| {
                SimpleGroupPackageError::BrokenPlanIdentity {
                    who: wire.who,
                    o: -1,
                }
            })?;
            groups_after.list[group_slot] = plan.group;
            let stopped_units = facts
                .iter()
                .filter(|facts| {
                    facts.on_map && facts.current_order == Some(crate::order::OrderIndex::CastSpell)
                })
                .count();
            for step in plan.steps {
                match step {
                    StopSpellStep::SetUnitMasks { o, value } => {
                        mutation_for(&mut units, wire.who, o)?.unit.after.unit_masks = value;
                    }
                    StopSpellStep::ClearPathAnchor { o }
                    | StopSpellStep::ClearPartialPath { o } => {
                        mutation_for(&mut units, wire.who, o)?
                            .unit
                            .after
                            .path
                            .clear();
                    }
                    StopSpellStep::CloseOrders { o, .. } => {
                        mutation_for(&mut units, wire.who, o)?
                            .unit
                            .after
                            .orders
                            .clear();
                    }
                    StopSpellStep::UpdateAction { o } => {
                        let mutation = mutation_for(&mut units, wire.who, o)?;
                        mutation.unit.after.orders_x = mutation.unit.after.x;
                        mutation.unit.after.orders_y = mutation.unit.after.y;
                        mutation.dest_angle_after = mutation.unit.after.angle;
                    }
                    StopSpellStep::ClearSpellWord98 { o } => {
                        mutation_for(&mut units, wire.who, o)?.spell_time_after = Some(0);
                    }
                    StopSpellStep::SetObjectsFlag22c | StopSpellStep::UpdateGpiece => {
                        unreachable!("special-type graphics tails were refused before planning")
                    }
                }
            }
            SimpleGroupActionResult::StopSpell { stopped_units }
        }
        SimpleGroupActionWire::Halt => {
            let mut facts = Vec::with_capacity(selection.members.len());
            for member in &selection.members {
                let mutation = mutation_for(&mut units, member.identity.who, member.identity.o)?;
                let airborne_plane_veto = member.authority.is_plane
                    && member.authority.domain == 2
                    && member.authority.unit_flags & 0x20 == 0;
                let entering_or_exiting = if member.authority.on_map && !airborne_plane_veto {
                    mutation
                        .unit
                        .after
                        .orders
                        .current()
                        .map_or(Some(false), crate::order::Order::is_entering_or_exiting)
                        .ok_or(SimpleGroupPackageError::MissingSpecialAnimPayload {
                            handle: mutation.unit.before.identity.handle,
                        })?
                } else {
                    false
                };
                facts.push(HaltMemberFacts {
                    o: member.identity.o,
                    valid_unit: true,
                    on_map: member.authority.on_map,
                    is_plane: member.authority.is_plane,
                    domain: member.authority.domain,
                    unit_flags: member.authority.unit_flags,
                    entering_or_exiting,
                    // Wire opcode 12 always calls `action_halt(0)`, so these predicates are
                    // not read by retail and are intentionally absent from the authority.
                    flag_4_veto: false,
                    special: false,
                    spy: false,
                });
            }
            let plan = plan_action_halt(&group, 0, &facts).map_err(|_| {
                SimpleGroupPackageError::BrokenPlanIdentity {
                    who: wire.who,
                    o: -1,
                }
            })?;
            groups_after.list[group_slot] = plan.group;
            let mut halted_units = 0usize;
            for step in plan.steps {
                let (who, o) = match step {
                    HaltStep::ClearUnitMask { who, o, .. }
                    | HaltStep::ClearPathAnchor { who, o }
                    | HaltStep::CloseOrders { who, o, .. }
                    | HaltStep::ClearPartialPath { who, o }
                    | HaltStep::UpdateAction { who, o } => (who, o),
                };
                let mutation = mutation_for(&mut units, who, o)?;
                match step {
                    HaltStep::ClearUnitMask { mask, .. } => mutation.unit.after.unit_masks &= !mask,
                    HaltStep::ClearPathAnchor { .. } | HaltStep::ClearPartialPath { .. } => {
                        mutation.unit.after.path.clear()
                    }
                    HaltStep::CloseOrders { .. } => mutation.unit.after.orders.clear(),
                    HaltStep::UpdateAction { .. } => {
                        mutation.unit.after.orders_x = mutation.unit.after.x;
                        mutation.unit.after.orders_y = mutation.unit.after.y;
                        mutation.dest_angle_after = mutation.unit.after.angle;
                        halted_units += 1;
                    }
                }
            }
            SimpleGroupActionResult::Halt { halted_units }
        }
        SimpleGroupActionWire::SetTransport { flag } => {
            let owner_flags =
                *leader_flags
                    .get(usize::from(wire.who))
                    .ok_or(PackageError::OwnerOutOfRange {
                        who: wire.who as i8,
                    })?;
            let transport_level = if owner_flags & 0x100 != 0 {
                3
            } else if owner_flags & 0x200 != 0 {
                2
            } else {
                ((owner_flags >> 10) & 1) as u8
            };
            let mut facts = Vec::with_capacity(selection.members.len());
            for member in &selection.members {
                let capability = action_authority.member(member.identity.handle).ok_or(
                    SimpleGroupPackageError::MissingSetTransportAuthority {
                        handle: member.identity.handle,
                    },
                )?;
                let mutation = mutation_for(&mut units, member.identity.who, member.identity.o)?;
                facts.push(SetTransportMemberFacts {
                    o: member.identity.o,
                    valid_unit: true,
                    can_ever_transport: capability.can_ever_transport,
                    unit_masks: mutation.unit.after.unit_masks,
                });
            }
            let plan =
                plan_action_set_transport(&group, flag, transport_level, &facts).map_err(|_| {
                    SimpleGroupPackageError::BrokenPlanIdentity {
                        who: wire.who,
                        o: -1,
                    }
                })?;
            groups_after.list[group_slot] = plan.group;
            let changed_units = plan.steps.len();
            for step in plan.steps {
                let SetTransportStep::WriteUnitMasks { who, o, value } = step;
                mutation_for(&mut units, who, o)?.unit.after.unit_masks = value;
            }
            action_authority_before = Some(action_authority.clone());
            leader_flags_before = Some(owner_flags);
            SimpleGroupActionResult::SetTransport {
                enabled: plan.enabled,
                changed_units,
            }
        }
        SimpleGroupActionWire::BuildMask { mask, set } => {
            // A Unit-band selection is an exact early no-op: action_buildmask tests the
            // Group building discriminator before walking any Build virtual or mask column.
            let plan = plan_action_buildmask(&group, mask, set, false, &[]).map_err(|_| {
                SimpleGroupPackageError::BrokenPlanIdentity {
                    who: wire.who,
                    o: -1,
                }
            })?;
            groups_after.list[group_slot] = plan.group;
            SimpleGroupActionResult::BuildMask {
                final_set: plan.final_set,
                changed_builds: 0,
                feedback: false,
            }
        }
    };

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
        selected_builds: Vec::new(),
        command_state_before: selection.command_state_before,
        command_state_after: selection.command_state_after,
        groups_before: selection.groups_before,
        groups_after,
        authority_revision: selection.authority_revision,
        authority_digest: selection.authority_digest,
        authority_members: selection.authority_members,
        action_authority_before,
        leader_flags_before,
        local_who_before: local_who,
        units,
        builds: Vec::new(),
        action_result,
    })
}

/// Revalidate every reached owner and then publish only assignments.
pub fn commit_simple_group_package(
    world: &mut World,
    unit_types: &[i32],
    groups: &mut Groups,
    paths: &mut [PathStack],
    command_state: &mut CommandPackageState,
    authority: &GroupMoveAuthority,
    player_who: &[Option<u8>; NETWORK_PLAYERS],
    prepared: PreparedSimpleGroupPackage,
) -> Result<SimpleGroupPackageReceipt, SimpleGroupPackageError> {
    commit_simple_group_package_with_action_authority(
        world,
        unit_types,
        groups,
        paths,
        command_state,
        authority,
        &SimpleGroupActionAuthority::default(),
        &[0; NUM_LEADERS],
        player_who,
        prepared,
    )
}

/// Extended commit entry which revalidates action-specific facts before any publication.
#[allow(clippy::too_many_arguments)]
pub fn commit_simple_group_package_with_action_authority(
    world: &mut World,
    unit_types: &[i32],
    groups: &mut Groups,
    paths: &mut [PathStack],
    command_state: &mut CommandPackageState,
    authority: &GroupMoveAuthority,
    action_authority: &SimpleGroupActionAuthority,
    leader_flags: &[i32; NUM_LEADERS],
    player_who: &[Option<u8>; NETWORK_PLAYERS],
    prepared: PreparedSimpleGroupPackage,
) -> Result<SimpleGroupPackageReceipt, SimpleGroupPackageError> {
    commit_simple_group_package_with_builds(
        world,
        unit_types,
        &mut [],
        groups,
        paths,
        command_state,
        authority,
        action_authority,
        leader_flags,
        None,
        player_who,
        prepared,
    )
}

/// Production commit entry which includes canonical Build-band before-images.
#[allow(clippy::too_many_arguments)]
pub fn commit_simple_group_package_with_builds(
    world: &mut World,
    unit_types: &[i32],
    builds: &mut [BuildData],
    groups: &mut Groups,
    paths: &mut [PathStack],
    command_state: &mut CommandPackageState,
    authority: &GroupMoveAuthority,
    action_authority: &SimpleGroupActionAuthority,
    leader_flags: &[i32; NUM_LEADERS],
    local_who: Option<u8>,
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
    if let Some(action_authority_before) = &prepared.action_authority_before {
        if action_authority != action_authority_before {
            return Err(SimpleGroupPackageError::StaleActionAuthority);
        }
    }
    if let Some(leader_flags_before) = prepared.leader_flags_before {
        if leader_flags.get(usize::from(prepared.wire.who)).copied() != Some(leader_flags_before) {
            return Err(SimpleGroupPackageError::StaleLeaderFlags {
                who: prepared.wire.who,
            });
        }
    }
    if local_who != prepared.local_who_before {
        return Err(SimpleGroupPackageError::StaleLocalWho);
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
        if let Some(spell_time_before) = mutation.spell_time_before {
            if world.units.spell_time()[row] != spell_time_before {
                return Err(SimpleGroupPackageError::StaleUnitSpellTime {
                    handle: mutation.unit.before.identity.handle,
                });
            }
        }
        if let Some(type_index_before) = mutation.type_index_before {
            if unit_types.get(row).copied() != Some(type_index_before) {
                return Err(SimpleGroupPackageError::StaleUnitType {
                    handle: mutation.unit.before.identity.handle,
                });
            }
        }
    }
    if !prepared.builds.is_empty() && !world.object_bands_are_dense_equivalent() {
        return Err(SimpleGroupPackageError::StaleBuildRegistry);
    }
    for mutation in &prepared.builds {
        let current = build_identity(world, builds, mutation.identity.who, mutation.identity.o)
            .map_err(|_| SimpleGroupPackageError::StaleBuild {
                who: mutation.identity.who,
                o: mutation.identity.o,
            })?;
        let build = builds
            .get(current.row)
            .ok_or(SimpleGroupPackageError::StaleBuild {
                who: mutation.identity.who,
                o: mutation.identity.o,
            })?;
        if current != mutation.identity
            || build.flags != mutation.flags_before
            || build.build_masks != mutation.build_masks_before
        {
            return Err(SimpleGroupPackageError::StaleBuild {
                who: mutation.identity.who,
                o: mutation.identity.o,
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
        selected_builds: prepared.selected_builds.clone(),
        command_state_revision: prepared.command_state_after.revision(),
        groups_checksum: checksum.value,
        random_state_before: prepared.random_state,
        random_state_after: prepared.random_state,
        action_result: prepared.action_result,
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
        if let Some(spell_time_after) = mutation.spell_time_after {
            world.units.spell_time_mut()[row] = spell_time_after;
        }
        *world.orders_mut(row) = mutation.unit.after.orders;
        paths[row] = mutation.unit.after.path;
    }
    for mutation in prepared.builds {
        builds[mutation.identity.row].build_masks = mutation.build_masks_after;
    }
    Ok(receipt)
}
