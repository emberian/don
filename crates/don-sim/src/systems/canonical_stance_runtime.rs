//! Canonical bounded opcode-2 `STANCE` receiver.
//!
//! This host admits ordinary, on-map, non-aircraft Units whose effective Object and Unit
//! stance types agree in the retail 0..=3 cycles.  In the nonzero cycles
//! `Group::action_stance` is a complete direct mutation. Type zero is admitted only when the
//! owning Leader's flags do not contain bit 4. Resolved options 0, 3, and 4 additionally walk
//! each Unit's whole order queue and clear the concrete `AttackOrder::mandatory` byte exactly as
//! retail does. The leader-bit-4 update/repath tail, Build groups, aircraft, and mixed
//! stance-type groups remain explicit refusals.

use crate::systems::canonical_group_move_host::{
    groups_equal, prepare_group_selection, unit_still_current, CommandPackageState,
    GroupMoveAuthority, GroupSelectionUse, PackageError, UnitIdentity, UnitMutation,
    NETWORK_PLAYERS, RECEIVED_SELECTION_CAPACITY,
};
use crate::systems::groups_guys::{
    plan_action_stance, CheckSum, Groups, StanceMemberFacts, StanceStep,
};
use crate::systems::movement::PathStack;
use crate::world::{Handle, World};

pub const STANCE_OPCODE: u8 = 2;
pub const STANCE_WIRE_BYTES: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalStanceBinding {
    pub actor: Handle,
    /// Exact result of the ObjectData virtual used by the group type scan.
    pub object_stance_type: i32,
    /// Exact result of the UnitTypeData stance-type query used by the mutation loop.
    pub unit_stance_type: i32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CanonicalStanceAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub bindings: Vec<CanonicalStanceBinding>,
}

impl CanonicalStanceAuthority {
    fn binding(&self, actor: Handle) -> Option<CanonicalStanceBinding> {
        self.bindings
            .iter()
            .copied()
            .find(|entry| entry.actor == actor)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StanceWire {
    pub who: u8,
    pub objects: Vec<i16>,
    pub requested_stance: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalStanceError {
    Selection(PackageError),
    Truncated,
    TrailingBytes { expected: usize, actual: usize },
    WrongOpcode { got: u8 },
    MissingAuthority,
    MissingBinding { actor: Handle },
    UnsupportedCone(&'static str),
    BrokenPlan,
    StalePlayerMap,
    StaleFrame,
    StaleRandom,
    StaleCommandState,
    StaleGroups,
    StaleSelectionAuthority,
    StaleStanceAuthority,
    StaleLeaderFlags,
    StaleUnit(Handle),
    StaleUnitFlags(Handle),
    StaleUnitStance(Handle),
    StaleUnitOrders(Handle),
}

impl From<PackageError> for CanonicalStanceError {
    fn from(value: PackageError) -> Self {
        Self::Selection(value)
    }
}

pub fn decode_stance_package(bytes: &[u8]) -> Result<StanceWire, CanonicalStanceError> {
    if bytes.len() < 4 || bytes[0] != 0 {
        return Err(CanonicalStanceError::WrongOpcode {
            got: bytes.first().copied().unwrap_or(u8::MAX),
        });
    }
    let count = usize::from(bytes[1]);
    if count > RECEIVED_SELECTION_CAPACITY {
        return Err(PackageError::ExplicitSelectionTooLong { len: count }.into());
    }
    let group_len = 3usize
        .checked_add(count.saturating_mul(2))
        .ok_or(CanonicalStanceError::Truncated)?;
    let expected = group_len + STANCE_WIRE_BYTES;
    if bytes.len() < expected {
        return Err(CanonicalStanceError::Truncated);
    }
    if bytes.len() != expected {
        return Err(CanonicalStanceError::TrailingBytes {
            expected,
            actual: bytes.len(),
        });
    }
    if bytes[group_len] != STANCE_OPCODE {
        return Err(CanonicalStanceError::WrongOpcode {
            got: bytes[group_len],
        });
    }
    let mut objects = Vec::with_capacity(count);
    for index in 0..count {
        objects.push(i16::from_le_bytes(
            bytes[3 + index * 2..5 + index * 2]
                .try_into()
                .expect("validated Group member range"),
        ));
    }
    Ok(StanceWire {
        who: bytes[2],
        objects,
        requested_stance: i32::from_le_bytes(
            bytes[group_len + 1..group_len + 5]
                .try_into()
                .expect("validated STANCE payload range"),
        ),
    })
}

#[derive(Clone, Debug)]
struct StanceMutation {
    identity: UnitIdentity,
    flags_before: u8,
    flags_after: u8,
    stance_before: i8,
    stance_after: i8,
    clear_mandatory: bool,
    orders_before: crate::order::OrderList,
    orders_after: crate::order::OrderList,
}

#[derive(Clone, Debug)]
pub struct PreparedStancePackage {
    pub play: usize,
    pub serial: i32,
    pub frame: i32,
    pub wire: StanceWire,
    pub group_slot: usize,
    pub selected: Vec<UnitIdentity>,
    pub stance_type: i32,
    pub current_option: i32,
    pub resolved_stance: i32,
    player_who_before: [Option<u8>; NETWORK_PLAYERS],
    command_before: CommandPackageState,
    command_after: CommandPackageState,
    groups_before: Groups,
    groups_after: Groups,
    selection_revision: u64,
    selection_digest: [u8; 32],
    selection_members: Vec<crate::systems::canonical_group_move_host::MoveMemberAuthority>,
    authority_before: CanonicalStanceAuthority,
    leader_flags_owner: usize,
    leader_flags_before: u32,
    random_state: i32,
    units: Vec<UnitMutation>,
    stances: Vec<StanceMutation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StancePackageReceipt {
    pub play: usize,
    pub serial: i32,
    pub frame: i32,
    pub group_slot: usize,
    pub selected: Vec<UnitIdentity>,
    pub stance_type: i32,
    pub current_option: i32,
    pub resolved_stance: i32,
    pub attack_orders_cleared: usize,
    pub groups_checksum: u32,
    pub random_state_before: i32,
    pub random_state_after: i32,
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn prepare_stance_package(
    world: &World,
    groups: &Groups,
    paths: &[PathStack],
    command: &CommandPackageState,
    selection_authority: &GroupMoveAuthority,
    authority: &CanonicalStanceAuthority,
    leader_flags: &[u32; crate::systems::groups_guys::NUM_LEADERS],
    player_who: &[Option<u8>; NETWORK_PLAYERS],
    frame: i32,
    play: usize,
    serial: i32,
    bytes: &[u8],
) -> Result<PreparedStancePackage, CanonicalStanceError> {
    let wire = decode_stance_package(bytes)?;
    if play >= NETWORK_PLAYERS {
        return Err(PackageError::PlayOutOfRange { play }.into());
    }
    let expected = player_who[play].ok_or(PackageError::MissingPlayerMap { play })?;
    if expected != wire.who {
        return Err(PackageError::PlayerOwnerMismatch {
            play,
            expected,
            got: wire.who,
        }
        .into());
    }
    if !matches!(wire.requested_stance, -1 | -2) {
        return Err(CanonicalStanceError::UnsupportedCone(
            "bounded STANCE admits the two retail-observed cycle requests",
        ));
    }
    if authority.revision == 0 || authority.composition_digest == [0; 32] {
        return Err(CanonicalStanceError::MissingAuthority);
    }
    let selection = prepare_group_selection(
        world,
        groups,
        paths,
        command,
        selection_authority,
        frame,
        play,
        wire.who,
        &wire.objects,
        GroupSelectionUse::SimpleUnitState,
    )?;
    if selection.members.is_empty() {
        return Err(CanonicalStanceError::UnsupportedCone(
            "bounded STANCE requires an effective Unit selection",
        ));
    }

    // Retail's `get_unit(0)` representative is the first captain with the smallest land
    // formation category. Stable `min_by_key` retains packet/Group order for ties.
    let representative = selection
        .members
        .iter()
        .filter(|member| member.authority.is_captain && member.authority.on_map)
        .min_by_key(|member| member.authority.land_formation.category)
        .ok_or(CanonicalStanceError::UnsupportedCone(
            "bounded STANCE requires an on-map formation representative",
        ))?;
    let preferred = authority
        .binding(representative.identity.handle)
        .ok_or(CanonicalStanceError::MissingBinding {
            actor: representative.identity.handle,
        })?
        .object_stance_type;
    if !(0..=3).contains(&preferred) {
        return Err(CanonicalStanceError::UnsupportedCone(
            "unsupported virtual stance type retains residual tails",
        ));
    }

    let mut facts = Vec::with_capacity(selection.members.len());
    let mut stances = Vec::with_capacity(selection.members.len());
    for member in &selection.members {
        let binding = authority.binding(member.identity.handle).ok_or(
            CanonicalStanceError::MissingBinding {
                actor: member.identity.handle,
            },
        )?;
        if !member.authority.on_map
            || member.authority.is_plane
            || binding.object_stance_type != preferred
            || binding.unit_stance_type != preferred
        {
            return Err(CanonicalStanceError::UnsupportedCone(
                "bounded STANCE requires one ordinary on-map Unit stance type",
            ));
        }
        let row = world
            .row_of(member.identity.handle)
            .ok_or(CanonicalStanceError::StaleUnit(member.identity.handle))?;
        let current_stance = world.units.stance()[row];
        let flags = world.units.get_flags(row);
        facts.push(StanceMemberFacts {
            o: member.identity.o,
            active: true,
            valid_unit: true,
            is_captain: member.authority.is_captain,
            on_map: member.authority.on_map,
            object_stance_type: binding.object_stance_type,
            current_stance: i32::from(current_stance),
            is_build: false,
            build_stance_type: -1,
            is_unit: true,
            unit_stance_type: binding.unit_stance_type,
            is_plane: member.authority.is_plane,
            update_order_first_present: false,
            update_order_second_mandatory: false,
            update_action_first_present: false,
            update_action_second_mandatory: false,
        });
        stances.push(StanceMutation {
            identity: member.identity.clone(),
            flags_before: flags,
            flags_after: flags,
            stance_before: current_stance,
            stance_after: current_stance,
            clear_mandatory: false,
            orders_before: world.orders(row).clone(),
            orders_after: world.orders(row).clone(),
        });
    }

    let group = selection.groups_after.list[selection.group_slot].clone();
    let leader_flags_owner = usize::from(wire.who);
    let owner_flags = leader_flags[leader_flags_owner];
    let plan = plan_action_stance(
        &group,
        wire.requested_stance,
        preferred,
        owner_flags,
        &facts,
    )
    .map_err(|_| CanonicalStanceError::BrokenPlan)?;
    if !plan.group_on_map || plan.stance_type != preferred {
        return Err(CanonicalStanceError::BrokenPlan);
    }
    for step in plan.steps {
        match step {
            StanceStep::WriteUnitStance { who, o, value } => {
                let mutation = stances
                    .iter_mut()
                    .find(|entry| entry.identity.who == who && entry.identity.o == o)
                    .ok_or(CanonicalStanceError::BrokenPlan)?;
                mutation.stance_after = value;
                continue;
            }
            StanceStep::SetObjectFlag { who, o, mask } if mask == 0x10 => {
                let mutation = stances
                    .iter_mut()
                    .find(|entry| entry.identity.who == who && entry.identity.o == o)
                    .ok_or(CanonicalStanceError::BrokenPlan)?;
                mutation.flags_after |= mask;
            }
            StanceStep::ClearMandatory { who, o } => {
                let mutation = stances
                    .iter_mut()
                    .find(|entry| entry.identity.who == who && entry.identity.o == o)
                    .ok_or(CanonicalStanceError::BrokenPlan)?;
                mutation.clear_mandatory = true;
                mutation
                    .orders_after
                    .clear_attack_mandatory()
                    .map_err(|()| {
                        CanonicalStanceError::UnsupportedCone(
                            "type-zero mandatory tail requires concrete ATTACK payloads",
                        )
                    })?;
            }
            StanceStep::SetObjectFlag { .. }
            | StanceStep::WriteBuildStance { .. }
            | StanceStep::UpdateOrder { .. }
            | StanceStep::UpdateAction { .. }
            | StanceStep::Repath { .. }
            | StanceStep::KillCurrentOrder { .. }
            | StanceStep::ClearOrders { .. } => {
                return Err(CanonicalStanceError::UnsupportedCone(
                    "STANCE plan reached an order, Build, or unsupported flag tail",
                ));
            }
        }
    }
    if stances
        .iter()
        .any(|mutation| mutation.stance_after as i32 != plan.resolved_stance)
    {
        return Err(CanonicalStanceError::BrokenPlan);
    }

    let mut groups_after = selection.groups_after.clone();
    groups_after.list[selection.group_slot] = plan.group;
    Ok(PreparedStancePackage {
        play,
        serial,
        frame,
        wire,
        group_slot: selection.group_slot,
        selected: selection
            .members
            .iter()
            .map(|member| member.identity.clone())
            .collect(),
        stance_type: plan.stance_type,
        current_option: plan.current_option,
        resolved_stance: plan.resolved_stance,
        player_who_before: *player_who,
        command_before: selection.command_state_before,
        command_after: selection.command_state_after,
        groups_before: selection.groups_before,
        groups_after,
        selection_revision: selection.authority_revision,
        selection_digest: selection.authority_digest,
        selection_members: selection.authority_members,
        authority_before: authority.clone(),
        leader_flags_owner,
        leader_flags_before: owner_flags,
        random_state: world.random.state(),
        units: selection.units,
        stances,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn commit_stance_package(
    world: &mut World,
    groups: &mut Groups,
    paths: &mut [PathStack],
    command: &mut CommandPackageState,
    selection_authority: &GroupMoveAuthority,
    authority: &CanonicalStanceAuthority,
    leader_flags: &[u32; crate::systems::groups_guys::NUM_LEADERS],
    player_who: &[Option<u8>; NETWORK_PLAYERS],
    prepared: PreparedStancePackage,
) -> Result<StancePackageReceipt, CanonicalStanceError> {
    if player_who != &prepared.player_who_before {
        return Err(CanonicalStanceError::StalePlayerMap);
    }
    if world.frame != prepared.frame {
        return Err(CanonicalStanceError::StaleFrame);
    }
    if world.random.state() != prepared.random_state {
        return Err(CanonicalStanceError::StaleRandom);
    }
    if command != &prepared.command_before {
        return Err(CanonicalStanceError::StaleCommandState);
    }
    if !groups_equal(groups, &prepared.groups_before) {
        return Err(CanonicalStanceError::StaleGroups);
    }
    if selection_authority.revision != prepared.selection_revision
        || selection_authority.composition_digest != prepared.selection_digest
        || selection_authority.members != prepared.selection_members
    {
        return Err(CanonicalStanceError::StaleSelectionAuthority);
    }
    if authority != &prepared.authority_before {
        return Err(CanonicalStanceError::StaleStanceAuthority);
    }
    if leader_flags[prepared.leader_flags_owner] != prepared.leader_flags_before {
        return Err(CanonicalStanceError::StaleLeaderFlags);
    }
    for mutation in &prepared.stances {
        let row = world
            .row_of(mutation.identity.handle)
            .ok_or(CanonicalStanceError::StaleUnit(mutation.identity.handle))?;
        if world.units.get_flags(row) != mutation.flags_before {
            return Err(CanonicalStanceError::StaleUnitFlags(
                mutation.identity.handle,
            ));
        }
        if world.units.stance()[row] != mutation.stance_before {
            return Err(CanonicalStanceError::StaleUnitStance(
                mutation.identity.handle,
            ));
        }
        if world.orders(row) != &mutation.orders_before {
            return Err(CanonicalStanceError::StaleUnitOrders(
                mutation.identity.handle,
            ));
        }
    }
    for mutation in &prepared.units {
        if !unit_still_current(world, paths, &mutation.before) {
            return Err(CanonicalStanceError::StaleUnit(
                mutation.before.identity.handle,
            ));
        }
    }

    let mut checksum = CheckSum::default();
    prepared.groups_after.check_groups(&mut checksum);
    let receipt = StancePackageReceipt {
        play: prepared.play,
        serial: prepared.serial,
        frame: prepared.frame,
        group_slot: prepared.group_slot,
        selected: prepared.selected.clone(),
        stance_type: prepared.stance_type,
        current_option: prepared.current_option,
        resolved_stance: prepared.resolved_stance,
        attack_orders_cleared: prepared
            .stances
            .iter()
            .map(|mutation| {
                if mutation.clear_mandatory {
                    mutation
                        .orders_before
                        .iter()
                        .filter(|order| order.kind == crate::order::OrderIndex::Attack)
                        .count()
                } else {
                    0
                }
            })
            .sum(),
        groups_checksum: checksum.value,
        random_state_before: prepared.random_state,
        random_state_after: prepared.random_state,
    };
    *groups = prepared.groups_after;
    *command = prepared.command_after;
    for mutation in prepared.units {
        let row = world
            .row_of(mutation.before.identity.handle)
            .expect("STANCE Unit identities were revalidated");
        world.units.group_mut()[row] = mutation.after.group;
    }
    for mutation in prepared.stances {
        let row = world
            .row_of(mutation.identity.handle)
            .expect("STANCE mutation identities were revalidated");
        world.units.set_flags(row, mutation.flags_after);
        world.units.stance_mut()[row] = mutation.stance_after;
        *world.orders_mut(row) = mutation.orders_after;
    }
    Ok(receipt)
}
