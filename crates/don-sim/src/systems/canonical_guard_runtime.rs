//! Canonical bounded GUARD packet and frame owner.
//!
//! This host intentionally admits only the dominant retail subdomain: one ordinary Unit
//! selected by `[Group][Guard]`, `QueuePos::New`, a same-owner ordinary Unit target, and an
//! authority-attested singleton formation offset.  The saved executor is likewise bounded to
//! the nonmoving periodic idle pulse, which mutates only `GuardOrder::idle`.  Spatial search,
//! movement, casting, RNG, queue-last and the general multi-member receiver remain refused.

use crate::order::Order;
use crate::systems::air_runtime_authority::ScenarioIgnoreOrdersAuthority;
use crate::systems::canonical_group_move_host::{
    groups_equal, prepare_group_selection, unit_still_current, CommandPackageState,
    GroupMoveAuthority, GroupSelectionUse, PackageError, UnitIdentity, UnitMutation,
};
use crate::systems::groups_guys::{CheckSum, Groups};
use crate::systems::guard_order::{
    plan_guard_executor, plan_guard_install, GuardActorFacts, GuardExecutorBranch,
    GuardExecutorRequest, GuardIdentity, GuardInstallRequest, GuardInstallTarget, GuardOrderState,
    GuardPlanError, GuardTargetFacts, QUEUE_NEW,
};
use crate::systems::movement::PathStack;
use crate::world::{Handle, World, OBJ_FLAG_ACTIVE};

pub const GUARD_OPCODE: u8 = 31;
pub const GUARD_WIRE_BYTES: usize = 13;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalGuardBinding {
    pub actor: Handle,
    pub target: Handle,
    /// Exact `FormData.off_x/off_y` output consumed by the singleton receiver.
    pub dx: i32,
    pub dy: i32,
    pub target_is_on_map: bool,
    pub target_is_valid_unit: bool,
    pub target_is_moving: bool,
    /// Attests that the retail receiver's scenario/leader/formation side-effect tails are
    /// empty for this exact actor/target pair. Those tails are deliberately not synthesized by
    /// this bounded host.
    pub receiver_external_effects_empty: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CanonicalGuardAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub bindings: Vec<CanonicalGuardBinding>,
}

impl CanonicalGuardAuthority {
    fn binding(&self, actor: Handle, target: Handle) -> Option<CanonicalGuardBinding> {
        self.bindings
            .iter()
            .copied()
            .find(|entry| entry.actor == actor && entry.target == target)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuardWire {
    pub who: u8,
    pub objects: Vec<i16>,
    pub target_o: i32,
    pub target_who: i32,
    pub queue: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalGuardError {
    Selection(PackageError),
    Truncated,
    TrailingBytes { expected: usize, actual: usize },
    WrongOpcode { got: u8 },
    MissingPlayerMap { play: usize },
    PlayerOwnerMismatch { expected: u8, got: u8 },
    UnsupportedCone(&'static str),
    MissingAuthority,
    MissingBinding,
    MissingTarget,
    StaleTarget,
    Planner(GuardPlanError),
    StaleCommandState,
    StaleGroups,
    StaleSelectionAuthority,
    StaleGuardAuthority,
    StaleScenarioAuthority,
    StaleLeaderFlags,
    StaleUnit(Handle),
    StaleRandom,
}

impl From<PackageError> for CanonicalGuardError {
    fn from(value: PackageError) -> Self {
        Self::Selection(value)
    }
}

impl From<GuardPlanError> for CanonicalGuardError {
    fn from(value: GuardPlanError) -> Self {
        Self::Planner(value)
    }
}

fn i32_at(bytes: &[u8], at: usize) -> Result<i32, CanonicalGuardError> {
    Ok(i32::from_le_bytes(
        bytes
            .get(at..at + 4)
            .ok_or(CanonicalGuardError::Truncated)?
            .try_into()
            .expect("four-byte range"),
    ))
}

pub fn decode_guard_package(bytes: &[u8]) -> Result<GuardWire, CanonicalGuardError> {
    if bytes.len() < 4 || bytes[0] != 0 {
        return Err(CanonicalGuardError::WrongOpcode {
            got: bytes.first().copied().unwrap_or(u8::MAX),
        });
    }
    let count = usize::from(bytes[1]);
    let group_len = 3usize
        .checked_add(count.saturating_mul(2))
        .ok_or(CanonicalGuardError::Truncated)?;
    let expected = group_len + GUARD_WIRE_BYTES;
    if bytes.len() < expected {
        return Err(CanonicalGuardError::Truncated);
    }
    if bytes.len() != expected {
        return Err(CanonicalGuardError::TrailingBytes {
            expected,
            actual: bytes.len(),
        });
    }
    if bytes[group_len] != GUARD_OPCODE {
        return Err(CanonicalGuardError::WrongOpcode {
            got: bytes[group_len],
        });
    }
    let mut objects = Vec::with_capacity(count);
    for index in 0..count {
        objects.push(i16::from_le_bytes(
            bytes[3 + index * 2..5 + index * 2]
                .try_into()
                .expect("validated group range"),
        ));
    }
    Ok(GuardWire {
        who: bytes[2],
        objects,
        target_o: i32_at(bytes, group_len + 1)?,
        target_who: i32_at(bytes, group_len + 5)?,
        queue: i32_at(bytes, group_len + 9)?,
    })
}

#[derive(Clone, Debug)]
pub struct PreparedGuardPackage {
    pub play: usize,
    pub serial: i32,
    pub frame: i32,
    pub wire: GuardWire,
    pub group_slot: usize,
    pub selected: UnitIdentity,
    command_before: CommandPackageState,
    command_after: CommandPackageState,
    groups_before: Groups,
    groups_after: Groups,
    selection_revision: u64,
    selection_digest: [u8; 32],
    selection_members: Vec<crate::systems::canonical_group_move_host::MoveMemberAuthority>,
    guard_before: CanonicalGuardAuthority,
    scenario_before: ScenarioIgnoreOrdersAuthority,
    leader_flags_before: [i32; 8],
    target_before: GuardTargetImage,
    random_state: i32,
    units: Vec<UnitMutation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GuardTargetImage {
    handle: Handle,
    identity: GuardIdentity,
    flags: u8,
    x: i32,
    y: i32,
}

fn target_still_current(world: &World, image: GuardTargetImage) -> bool {
    let Some(row) = world.unit_row_at(image.identity.who, image.identity.o) else {
        return false;
    };
    world.row_of(image.handle) == Some(row)
        && world.handle_at_row(row) == Some(image.handle)
        && world.units.get_uid(row) == image.identity.uid
        && world.units.get_flags(row) == image.flags
        && world.units.x_internal()[row] == image.x
        && world.units.y_internal()[row] == image.y
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuardPackageReceipt {
    pub play: usize,
    pub serial: i32,
    pub frame: i32,
    pub group_slot: usize,
    pub selected: UnitIdentity,
    pub target: GuardIdentity,
    pub groups_checksum: u32,
    pub random_state_before: i32,
    pub random_state_after: i32,
}

#[allow(clippy::too_many_arguments)]
pub fn prepare_guard_package(
    world: &World,
    groups: &Groups,
    paths: &[PathStack],
    command: &CommandPackageState,
    selection_authority: &GroupMoveAuthority,
    guard_authority: &CanonicalGuardAuthority,
    scenario: &ScenarioIgnoreOrdersAuthority,
    leader_flags: &[i32; 8],
    player_who: &[Option<u8>; 8],
    frame: i32,
    play: usize,
    serial: i32,
    bytes: &[u8],
) -> Result<PreparedGuardPackage, CanonicalGuardError> {
    let wire = decode_guard_package(bytes)?;
    let expected = player_who
        .get(play)
        .copied()
        .flatten()
        .ok_or(CanonicalGuardError::MissingPlayerMap { play })?;
    if expected != wire.who {
        return Err(CanonicalGuardError::PlayerOwnerMismatch {
            expected,
            got: wire.who,
        });
    }
    if wire.queue != QUEUE_NEW {
        return Err(CanonicalGuardError::UnsupportedCone(
            "GUARD queue is not QueuePos::New",
        ));
    }
    if guard_authority.revision == 0 || guard_authority.composition_digest == [0; 32] {
        return Err(CanonicalGuardError::MissingAuthority);
    }
    if scenario.ignore_orders
        || scenario
            .ignored_by_owner
            .get(usize::from(wire.who))
            .is_none_or(|ignored| !ignored.is_empty())
        || leader_flags[usize::from(wire.who)] & 4 != 0
    {
        return Err(CanonicalGuardError::UnsupportedCone(
            "bounded GUARD requires empty scenario and leader side-effect tails",
        ));
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
        GroupSelectionUse::EconomyOrderInstall,
    )?;
    if selection.members.len() != 1 {
        return Err(CanonicalGuardError::UnsupportedCone(
            "bounded GUARD requires one effective Unit member",
        ));
    }
    let member = &selection.members[0];
    if !member.authority.on_map || !member.authority.is_captain {
        return Err(CanonicalGuardError::UnsupportedCone(
            "bounded GUARD requires an on-map captain",
        ));
    }
    let target_row = world
        .unit_row_at(wire.target_who, wire.target_o)
        .ok_or(CanonicalGuardError::MissingTarget)?;
    if world.units.get_flags(target_row) & OBJ_FLAG_ACTIVE == 0
        || i32::from(world.units.get_who(target_row)) != wire.target_who
        || i32::from(world.units.o()[target_row]) != wire.target_o
    {
        return Err(CanonicalGuardError::StaleTarget);
    }
    let target_handle = world
        .handle_at_row(target_row)
        .ok_or(CanonicalGuardError::MissingTarget)?;
    let binding = guard_authority
        .binding(member.identity.handle, target_handle)
        .ok_or(CanonicalGuardError::MissingBinding)?;
    if wire.target_who != i32::from(wire.who)
        || member.identity.handle == target_handle
        || !binding.target_is_on_map
        || !binding.target_is_valid_unit
        || !binding.receiver_external_effects_empty
    {
        return Err(CanonicalGuardError::UnsupportedCone(
            "bounded GUARD target admission is not satisfied",
        ));
    }

    let target = GuardInstallTarget {
        identity: GuardIdentity {
            o: wire.target_o,
            who: wire.target_who,
            uid: world.units.get_uid(target_row),
        },
        x: world.units.x_internal()[target_row],
        y: world.units.y_internal()[target_row],
    };
    let target_before = GuardTargetImage {
        handle: target_handle,
        identity: target.identity,
        flags: world.units.get_flags(target_row),
        x: target.x,
        y: target.y,
    };
    let install = plan_guard_install(
        GuardInstallRequest {
            actor: GuardIdentity {
                o: i32::from(member.identity.o),
                who: i32::from(member.identity.who),
                uid: member.identity.uid,
            },
            actor_x: world.units.x_internal()[member.row],
            actor_y: world.units.y_internal()[member.row],
            target_o: wire.target_o,
            target_who: wire.target_who,
            dx: binding.dx,
            dy: binding.dy,
            queue_pos: wire.queue,
            unused_arg6: 0,
        },
        Some(target),
    )?;
    let order = Order::guard(install.order).map_err(|_| {
        CanonicalGuardError::UnsupportedCone("GUARD target exceeds canonical header")
    })?;
    let mut units = selection.units.clone();
    let mutation = units
        .iter_mut()
        .find(|entry| entry.before.identity.handle == member.identity.handle)
        .ok_or(CanonicalGuardError::StaleUnit(member.identity.handle))?;
    mutation.after.orders.replace(order);
    mutation.after.unit_masks &= !0x0400_0000;
    mutation.after.path = PathStack::default();

    let mut groups_after = selection.groups_after.clone();
    groups_after.list[selection.group_slot].disband = 0;
    groups_after.list[selection.group_slot].form = -1;

    Ok(PreparedGuardPackage {
        play,
        serial,
        frame,
        wire,
        group_slot: selection.group_slot,
        selected: member.identity.clone(),
        command_before: selection.command_state_before,
        command_after: selection.command_state_after,
        groups_before: selection.groups_before,
        groups_after,
        selection_revision: selection.authority_revision,
        selection_digest: selection.authority_digest,
        selection_members: selection.authority_members,
        guard_before: guard_authority.clone(),
        scenario_before: scenario.clone(),
        leader_flags_before: *leader_flags,
        target_before,
        random_state: world.random.state(),
        units,
    })
}

pub fn commit_guard_package(
    world: &mut World,
    groups: &mut Groups,
    paths: &mut [PathStack],
    command: &mut CommandPackageState,
    selection_authority: &GroupMoveAuthority,
    guard_authority: &CanonicalGuardAuthority,
    scenario: &ScenarioIgnoreOrdersAuthority,
    leader_flags: &[i32; 8],
    prepared: PreparedGuardPackage,
) -> Result<GuardPackageReceipt, CanonicalGuardError> {
    if command != &prepared.command_before {
        return Err(CanonicalGuardError::StaleCommandState);
    }
    if !groups_equal(groups, &prepared.groups_before) {
        return Err(CanonicalGuardError::StaleGroups);
    }
    if selection_authority.revision != prepared.selection_revision
        || selection_authority.composition_digest != prepared.selection_digest
        || selection_authority.members != prepared.selection_members
    {
        return Err(CanonicalGuardError::StaleSelectionAuthority);
    }
    if guard_authority != &prepared.guard_before {
        return Err(CanonicalGuardError::StaleGuardAuthority);
    }
    if scenario != &prepared.scenario_before {
        return Err(CanonicalGuardError::StaleScenarioAuthority);
    }
    if leader_flags != &prepared.leader_flags_before {
        return Err(CanonicalGuardError::StaleLeaderFlags);
    }
    if world.random.state() != prepared.random_state {
        return Err(CanonicalGuardError::StaleRandom);
    }
    for mutation in &prepared.units {
        if !unit_still_current(world, paths, &mutation.before) {
            return Err(CanonicalGuardError::StaleUnit(
                mutation.before.identity.handle,
            ));
        }
    }
    if !target_still_current(world, prepared.target_before) {
        return Err(CanonicalGuardError::StaleTarget);
    }
    let current = prepared.units[0]
        .after
        .orders
        .current()
        .and_then(|order| order.guard)
        .expect("prepared GUARD owns its payload");
    let mut checksum = CheckSum::default();
    prepared.groups_after.check_groups(&mut checksum);
    let receipt = GuardPackageReceipt {
        play: prepared.play,
        serial: prepared.serial,
        frame: prepared.frame,
        group_slot: prepared.group_slot,
        selected: prepared.selected.clone(),
        target: current.target,
        groups_checksum: checksum.value,
        random_state_before: prepared.random_state,
        random_state_after: prepared.random_state,
    };
    *groups = prepared.groups_after;
    *command = prepared.command_after;
    for mutation in prepared.units {
        let row = world
            .row_of(mutation.before.identity.handle)
            .expect("GUARD identities were revalidated");
        world.units.group_mut()[row] = mutation.after.group;
        world.units.set_unit_masks(row, mutation.after.unit_masks);
        *world.orders_mut(row) = mutation.after.orders;
        paths[row] = mutation.after.path;
    }
    Ok(receipt)
}

#[derive(Clone, Debug)]
pub struct PreparedGuardIdleActivation {
    row: usize,
    actor: Handle,
    target: Handle,
    before: Order,
    after: Order,
    authority: CanonicalGuardAuthority,
    random_state: i32,
}

pub fn prepare_guard_idle_activation(
    world: &World,
    authority: &CanonicalGuardAuthority,
    row: usize,
) -> Result<PreparedGuardIdleActivation, CanonicalGuardError> {
    let actor = world
        .handle_at_row(row)
        .ok_or(CanonicalGuardError::MissingTarget)?;
    let before = world
        .orders(row)
        .current()
        .cloned()
        .ok_or(CanonicalGuardError::UnsupportedCone("GUARD queue is empty"))?;
    let guard = before.guard.ok_or(CanonicalGuardError::UnsupportedCone(
        "missing GUARD payload",
    ))?;
    if before.kind != crate::order::OrderIndex::Guard
        || (
            i32::from(before.target_o),
            i32::from(before.target_who),
            before.target_uid,
        ) != (guard.target.o, guard.target.who, guard.target.uid)
    {
        return Err(CanonicalGuardError::UnsupportedCone(
            "malformed GUARD payload",
        ));
    }
    let target_row = world
        .unit_row_at(guard.target.who, guard.target.o)
        .ok_or(CanonicalGuardError::MissingTarget)?;
    let target = world
        .handle_at_row(target_row)
        .ok_or(CanonicalGuardError::MissingTarget)?;
    let binding = authority
        .binding(actor, target)
        .ok_or(CanonicalGuardError::MissingBinding)?;
    if world.units.get_uid(target_row) != guard.target.uid
        || world.units.get_flags(target_row) & OBJ_FLAG_ACTIVE == 0
    {
        return Err(CanonicalGuardError::StaleTarget);
    }
    let request = GuardExecutorRequest {
        actor: GuardActorFacts {
            identity: GuardIdentity {
                o: i32::from(world.units.o()[row]),
                who: i32::from(world.units.get_who(row)),
                uid: world.units.get_uid(row),
            },
            x: world.units.x_internal()[row],
            y: world.units.y_internal()[row],
            angle: world.units.angle()[row],
            frame: world.frame,
            unit_masks: world.units.get_unit_masks(row),
            type_flags_2b4: 0,
            type_flags_2b8: 0,
            type_slot_10c: false,
            actor_slot_cc: false,
            actor_slot_c4: false,
            actor_slot_100: false,
            is_unpacking: false,
            is_type_7b: false,
            is_captain: true,
        },
        order: guard,
        target: Some(GuardTargetFacts {
            identity: guard.target,
            active: true,
            initial_valid_unit: binding.target_is_valid_unit,
            initial_on_map: binding
                .target_is_valid_unit
                .then_some(binding.target_is_on_map),
            valid_unit: binding.target_is_valid_unit,
            on_map: binding
                .target_is_valid_unit
                .then_some(binding.target_is_on_map),
            is_moving: binding.target_is_moving,
            is_wallbuild: false,
            x: world.units.x_internal()[target_row],
            y: world.units.y_internal()[target_row],
            angle: world.units.angle()[target_row],
            unit_masks: world.units.get_unit_masks(target_row),
            attack_dist: 0,
            x_size: 0,
            y_size: 0,
        }),
        spatial: None,
        post_move: None,
    };
    let plan = plan_guard_executor(request)?;
    if plan.branch != GuardExecutorBranch::PeriodicIdlePulse || !plan.steps.is_empty() {
        return Err(CanonicalGuardError::UnsupportedCone(
            "GUARD frame is outside the periodic idle pulse",
        ));
    }
    let mut after = before.clone();
    after.guard = Some(plan.order_after);
    Ok(PreparedGuardIdleActivation {
        row,
        actor,
        target,
        before,
        after,
        authority: authority.clone(),
        random_state: world.random.state(),
    })
}

pub fn commit_guard_idle_activation(
    world: &mut World,
    authority: &CanonicalGuardAuthority,
    prepared: PreparedGuardIdleActivation,
) -> Result<GuardOrderState, CanonicalGuardError> {
    if authority != &prepared.authority {
        return Err(CanonicalGuardError::StaleGuardAuthority);
    }
    if world.random.state() != prepared.random_state
        || world.row_of(prepared.actor) != Some(prepared.row)
        || world.row_of(prepared.target).is_none()
        || world.orders(prepared.row).current() != Some(&prepared.before)
    {
        return Err(CanonicalGuardError::StaleUnit(prepared.actor));
    }
    *world
        .orders_mut(prepared.row)
        .current_mut()
        .expect("GUARD head was revalidated") = prepared.after;
    Ok(world.orders(prepared.row).current().unwrap().guard.unwrap())
}
