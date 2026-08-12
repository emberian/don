//! Canonical bounded opcode-9 receiver and ATTACK_GROUND frame adapter.
//!
//! The admitted retail cone is deliberately small: an ordinary on-map Unit group selected by
//! `[Group][ATTACK_GROUND]`, QueuePos::New, unowned terrain, and the ground-order constructor.
//! The resumed executor is the reached recharge-hold branch, where the target is in range and
//! the current animation already lies in the retail 0..=3 hold set. Foreign terrain/diplomacy,
//! aircraft/containment delegation, repositioning, firing and every multi-member tail remain
//! refused rather than approximated.

use crate::order::{Order, ORDER_GROUP};
use crate::systems::air_runtime_authority::ScenarioIgnoreOrdersAuthority;
use crate::systems::canonical_group_move_host::{
    groups_equal, prepare_group_selection, unit_still_current, CommandPackageState,
    GroupMoveAuthority, GroupSelectionUse, PackageError, UnitIdentity, UnitMutation,
};
use crate::systems::groups_guys::{CheckSum, Groups};
use crate::systems::map_terrain::{World as TerrainWorld, COORD_PER_WCELL};
use crate::systems::movement::PathStack;
use crate::systems::targeted_order_plans::{
    plan_attack_ground, AttackGroundFacts, AttackGroundOrderState, HostFact,
};
use crate::world::{Handle, World};

pub const ATTACK_GROUND_OPCODE: u8 = 9;
pub const ATTACK_GROUND_WIRE_BYTES: usize = 10;
const QUEUE_NEW: i32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalAttackGroundBinding {
    pub actor: Handle,
    /// Exact receiver virtual/type result: this member stays in the ordinary ground branch.
    pub routes_to_ground_order: bool,
    pub can_attack_ground: bool,
    /// Attests that the receiver's presentation/scenario tail is empty for this actor.
    pub receiver_external_effects_empty: bool,
    /// Frame-only facts for the bounded recharge-hold executor branch.
    pub target_is_in_range: bool,
    pub raw_target_angle: u32,
    pub side_firing: bool,
    pub current_animation: i8,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CanonicalAttackGroundAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub bindings: Vec<CanonicalAttackGroundBinding>,
}

impl CanonicalAttackGroundAuthority {
    fn binding(&self, actor: Handle) -> Option<CanonicalAttackGroundBinding> {
        self.bindings
            .iter()
            .copied()
            .find(|entry| entry.actor == actor)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttackGroundWire {
    pub who: u8,
    pub objects: Vec<i16>,
    pub x: i32,
    pub y: i32,
    pub queue: i8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalAttackGroundError {
    Selection(PackageError),
    Truncated,
    TrailingBytes { expected: usize, actual: usize },
    WrongOpcode { got: u8 },
    MissingPlayerMap { play: usize },
    PlayerOwnerMismatch { expected: u8, got: u8 },
    UnsupportedCone(&'static str),
    MissingAuthority,
    MissingBinding,
    StaleCommandState,
    StaleGroups,
    StaleSelectionAuthority,
    StaleAttackGroundAuthority,
    StaleScenarioAuthority,
    StaleLeaderFlags,
    StaleTerrain,
    StaleUnit(Handle),
    StaleRandom,
}

impl From<PackageError> for CanonicalAttackGroundError {
    fn from(value: PackageError) -> Self {
        Self::Selection(value)
    }
}

fn i32_at(bytes: &[u8], at: usize) -> Result<i32, CanonicalAttackGroundError> {
    Ok(i32::from_le_bytes(
        bytes
            .get(at..at + 4)
            .ok_or(CanonicalAttackGroundError::Truncated)?
            .try_into()
            .expect("four-byte range"),
    ))
}

pub fn decode_attack_ground_package(
    bytes: &[u8],
) -> Result<AttackGroundWire, CanonicalAttackGroundError> {
    if bytes.len() < 4 || bytes[0] != 0 {
        return Err(CanonicalAttackGroundError::WrongOpcode {
            got: bytes.first().copied().unwrap_or(u8::MAX),
        });
    }
    let count = usize::from(bytes[1]);
    let group_len = 3usize
        .checked_add(count.saturating_mul(2))
        .ok_or(CanonicalAttackGroundError::Truncated)?;
    let expected = group_len + ATTACK_GROUND_WIRE_BYTES;
    if bytes.len() < expected {
        return Err(CanonicalAttackGroundError::Truncated);
    }
    if bytes.len() != expected {
        return Err(CanonicalAttackGroundError::TrailingBytes {
            expected,
            actual: bytes.len(),
        });
    }
    if bytes[group_len] != ATTACK_GROUND_OPCODE {
        return Err(CanonicalAttackGroundError::WrongOpcode {
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
    Ok(AttackGroundWire {
        who: bytes[2],
        objects,
        x: i32_at(bytes, group_len + 1)?,
        y: i32_at(bytes, group_len + 5)?,
        queue: bytes[group_len + 9] as i8,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TerrainImage {
    xs: i32,
    ys: i32,
    wx: i32,
    wy: i32,
    owner: i8,
}

fn clamped_target(terrain: &TerrainWorld, x: i32, y: i32) -> Option<(i32, i32)> {
    let max_x = terrain.xs.checked_mul(COORD_PER_WCELL)?.checked_sub(1)?;
    let max_y = terrain.ys.checked_mul(COORD_PER_WCELL)?.checked_sub(1)?;
    (max_x >= 0 && max_y >= 0).then_some((x.clamp(0, max_x), y.clamp(0, max_y)))
}

fn terrain_image(terrain: &TerrainWorld, x: i32, y: i32) -> Option<TerrainImage> {
    let (x, y) = clamped_target(terrain, x, y)?;
    let wx = x / COORD_PER_WCELL;
    let wy = y / COORD_PER_WCELL;
    Some(TerrainImage {
        xs: terrain.xs,
        ys: terrain.ys,
        wx,
        wy,
        owner: terrain.wdata(wx, wy).who,
    })
}

fn terrain_still_current(terrain: &TerrainWorld, image: TerrainImage) -> bool {
    terrain.xs == image.xs
        && terrain.ys == image.ys
        && image.wx >= 0
        && image.wy >= 0
        && image.wx < terrain.xs
        && image.wy < terrain.ys
        && terrain.wdata(image.wx, image.wy).who == image.owner
}

#[derive(Clone, Debug)]
pub struct PreparedAttackGroundPackage {
    pub play: usize,
    pub serial: i32,
    pub frame: i32,
    pub wire: AttackGroundWire,
    pub group_slot: usize,
    pub selected: Vec<UnitIdentity>,
    pub installed: AttackGroundOrderState,
    command_before: CommandPackageState,
    command_after: CommandPackageState,
    groups_before: Groups,
    groups_after: Groups,
    selection_revision: u64,
    selection_digest: [u8; 32],
    selection_members: Vec<crate::systems::canonical_group_move_host::MoveMemberAuthority>,
    authority_before: CanonicalAttackGroundAuthority,
    scenario_before: ScenarioIgnoreOrdersAuthority,
    leader_flags_before: [i32; 8],
    terrain_before: TerrainImage,
    random_state: i32,
    units: Vec<UnitMutation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttackGroundPackageReceipt {
    pub play: usize,
    pub serial: i32,
    pub frame: i32,
    pub group_slot: usize,
    pub selected: Vec<UnitIdentity>,
    pub installed: AttackGroundOrderState,
    pub groups_checksum: u32,
    pub random_state_before: i32,
    pub random_state_after: i32,
}

#[allow(clippy::too_many_arguments)]
pub fn prepare_attack_ground_package(
    world: &World,
    terrain: &TerrainWorld,
    groups: &Groups,
    paths: &[PathStack],
    command: &CommandPackageState,
    selection_authority: &GroupMoveAuthority,
    authority: &CanonicalAttackGroundAuthority,
    scenario: &ScenarioIgnoreOrdersAuthority,
    leader_flags: &[i32; 8],
    player_who: &[Option<u8>; 8],
    frame: i32,
    play: usize,
    serial: i32,
    bytes: &[u8],
) -> Result<PreparedAttackGroundPackage, CanonicalAttackGroundError> {
    let wire = decode_attack_ground_package(bytes)?;
    let expected = player_who
        .get(play)
        .copied()
        .flatten()
        .ok_or(CanonicalAttackGroundError::MissingPlayerMap { play })?;
    if expected != wire.who {
        return Err(CanonicalAttackGroundError::PlayerOwnerMismatch {
            expected,
            got: wire.who,
        });
    }
    if i32::from(wire.queue) != QUEUE_NEW {
        return Err(CanonicalAttackGroundError::UnsupportedCone(
            "ATTACK_GROUND queue is not QueuePos::New",
        ));
    }
    if authority.revision == 0 || authority.composition_digest == [0; 32] {
        return Err(CanonicalAttackGroundError::MissingAuthority);
    }
    if scenario.ignore_orders
        || scenario
            .ignored_by_owner
            .get(usize::from(wire.who))
            .is_none_or(|ignored| !ignored.is_empty())
        || leader_flags[usize::from(wire.who)] & 4 != 0
    {
        return Err(CanonicalAttackGroundError::UnsupportedCone(
            "bounded ATTACK_GROUND requires empty scenario and leader side-effect tails",
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
    if selection.members.is_empty() {
        return Err(CanonicalAttackGroundError::UnsupportedCone(
            "bounded ATTACK_GROUND requires an effective Unit selection",
        ));
    }
    if !selection
        .members
        .iter()
        .any(|member| member.authority.is_captain)
    {
        return Err(CanonicalAttackGroundError::UnsupportedCone(
            "ordinary ground receiver has no formation leader",
        ));
    }
    for member in &selection.members {
        let binding = authority
            .binding(member.identity.handle)
            .ok_or(CanonicalAttackGroundError::MissingBinding)?;
        if !member.authority.on_map
            || member.authority.is_plane
            || !binding.routes_to_ground_order
            || !binding.can_attack_ground
            || !binding.receiver_external_effects_empty
        {
            return Err(CanonicalAttackGroundError::UnsupportedCone(
                "ordinary ground receiver admission is not satisfied",
            ));
        }
    }
    let (x, y) = clamped_target(terrain, wire.x, wire.y).ok_or(
        CanonicalAttackGroundError::UnsupportedCone("terrain dimensions cannot clamp target"),
    )?;
    let terrain_before = terrain_image(terrain, x, y).ok_or(
        CanonicalAttackGroundError::UnsupportedCone("ATTACK_GROUND target is outside terrain"),
    )?;
    if terrain_before.owner >= 0 {
        return Err(CanonicalAttackGroundError::UnsupportedCone(
            "owned terrain requires the residual diplomacy branch",
        ));
    }
    let installed = AttackGroundOrderState {
        att_x: x,
        att_y: y,
        accuracy: 0,
        attack_unit: 0,
    };
    let mut units = selection.units.clone();
    for member in &selection.members {
        let mutation = units
            .iter_mut()
            .find(|entry| entry.before.identity.handle == member.identity.handle)
            .ok_or(CanonicalAttackGroundError::StaleUnit(
                member.identity.handle,
            ))?;
        if !mutation.before.orders.is_empty() {
            return Err(CanonicalAttackGroundError::UnsupportedCone(
                "nonempty queue requires close_orders epilogue authority",
            ));
        }
        mutation
            .after
            .orders
            .replace(Order::attack_ground(installed));
        mutation.after.unit_masks &= !0x0400_0000;
        mutation.after.path = PathStack::default();
        mutation.after.orders_x = mutation.before.x;
        mutation.after.orders_y = mutation.before.y;
        mutation.after.dest_angle = mutation.before.angle;
    }
    let mut groups_after = selection.groups_after.clone();
    groups_after.list[selection.group_slot].disband = 0;

    Ok(PreparedAttackGroundPackage {
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
        installed,
        command_before: selection.command_state_before,
        command_after: selection.command_state_after,
        groups_before: selection.groups_before,
        groups_after,
        selection_revision: selection.authority_revision,
        selection_digest: selection.authority_digest,
        selection_members: selection.authority_members,
        authority_before: authority.clone(),
        scenario_before: scenario.clone(),
        leader_flags_before: *leader_flags,
        terrain_before,
        random_state: world.random.state(),
        units,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn commit_attack_ground_package(
    world: &mut World,
    terrain: &TerrainWorld,
    groups: &mut Groups,
    paths: &mut [PathStack],
    command: &mut CommandPackageState,
    selection_authority: &GroupMoveAuthority,
    authority: &CanonicalAttackGroundAuthority,
    scenario: &ScenarioIgnoreOrdersAuthority,
    leader_flags: &[i32; 8],
    prepared: PreparedAttackGroundPackage,
) -> Result<AttackGroundPackageReceipt, CanonicalAttackGroundError> {
    if command != &prepared.command_before {
        return Err(CanonicalAttackGroundError::StaleCommandState);
    }
    if !groups_equal(groups, &prepared.groups_before) {
        return Err(CanonicalAttackGroundError::StaleGroups);
    }
    if selection_authority.revision != prepared.selection_revision
        || selection_authority.composition_digest != prepared.selection_digest
        || selection_authority.members != prepared.selection_members
    {
        return Err(CanonicalAttackGroundError::StaleSelectionAuthority);
    }
    if authority != &prepared.authority_before {
        return Err(CanonicalAttackGroundError::StaleAttackGroundAuthority);
    }
    if scenario != &prepared.scenario_before {
        return Err(CanonicalAttackGroundError::StaleScenarioAuthority);
    }
    if leader_flags != &prepared.leader_flags_before {
        return Err(CanonicalAttackGroundError::StaleLeaderFlags);
    }
    if !terrain_still_current(terrain, prepared.terrain_before) {
        return Err(CanonicalAttackGroundError::StaleTerrain);
    }
    if world.random.state() != prepared.random_state {
        return Err(CanonicalAttackGroundError::StaleRandom);
    }
    for mutation in &prepared.units {
        if !unit_still_current(world, paths, &mutation.before) {
            return Err(CanonicalAttackGroundError::StaleUnit(
                mutation.before.identity.handle,
            ));
        }
    }
    let mut checksum = CheckSum::default();
    prepared.groups_after.check_groups(&mut checksum);
    let receipt = AttackGroundPackageReceipt {
        play: prepared.play,
        serial: prepared.serial,
        frame: prepared.frame,
        group_slot: prepared.group_slot,
        selected: prepared.selected.clone(),
        installed: prepared.installed,
        groups_checksum: checksum.value,
        random_state_before: prepared.random_state,
        random_state_after: prepared.random_state,
    };
    *groups = prepared.groups_after;
    *command = prepared.command_after;
    for mutation in prepared.units {
        let row = world
            .row_of(mutation.before.identity.handle)
            .expect("ATTACK_GROUND identities were revalidated");
        world.units.group_mut()[row] = mutation.after.group;
        world.units.set_unit_masks(row, mutation.after.unit_masks);
        world.units.orders_x_mut()[row] = mutation.after.orders_x;
        world.units.orders_y_mut()[row] = mutation.after.orders_y;
        world.units.dest_angle_mut()[row] = mutation.after.dest_angle;
        *world.orders_mut(row) = mutation.after.orders;
        paths[row] = mutation.after.path;
    }
    Ok(receipt)
}

#[derive(Clone, Debug)]
pub struct PreparedAttackGroundHold {
    row: usize,
    actor: Handle,
    order: Order,
    authority: CanonicalAttackGroundAuthority,
    terrain: TerrainImage,
    actor_angle: i32,
    recharge: u8,
    random_state: i32,
}

pub fn prepare_attack_ground_hold(
    world: &World,
    terrain: &TerrainWorld,
    authority: &CanonicalAttackGroundAuthority,
    row: usize,
) -> Result<PreparedAttackGroundHold, CanonicalAttackGroundError> {
    let actor = world
        .handle_at_row(row)
        .ok_or(CanonicalAttackGroundError::UnsupportedCone(
            "missing ATTACK_GROUND actor",
        ))?;
    let order =
        world
            .orders(row)
            .current()
            .cloned()
            .ok_or(CanonicalAttackGroundError::UnsupportedCone(
                "ATTACK_GROUND queue is empty",
            ))?;
    let payload = order
        .attack_ground
        .ok_or(CanonicalAttackGroundError::UnsupportedCone(
            "missing ATTACK_GROUND payload",
        ))?;
    if order.kind != crate::order::OrderIndex::AttackGround
        || order.flags & ORDER_GROUP == 0
        || (order.x, order.y) != (payload.att_x, payload.att_y)
        || payload.accuracy != 0
        || payload.attack_unit != 0
    {
        return Err(CanonicalAttackGroundError::UnsupportedCone(
            "malformed or non-retail ATTACK_GROUND payload",
        ));
    }
    let binding = authority
        .binding(actor)
        .ok_or(CanonicalAttackGroundError::MissingBinding)?;
    if !binding.routes_to_ground_order
        || !binding.can_attack_ground
        || !binding.receiver_external_effects_empty
        || !binding.target_is_in_range
        || !(0..=3).contains(&binding.current_animation)
    {
        return Err(CanonicalAttackGroundError::UnsupportedCone(
            "ATTACK_GROUND frame is outside the recharge-hold branch",
        ));
    }
    let terrain_before = terrain_image(terrain, payload.att_x, payload.att_y).ok_or(
        CanonicalAttackGroundError::UnsupportedCone("ATTACK_GROUND target is outside terrain"),
    )?;
    if terrain_before.owner >= 0 {
        return Err(CanonicalAttackGroundError::UnsupportedCone(
            "owned terrain requires the residual diplomacy branch",
        ));
    }
    let recharge = world.units.get_recharging(row);
    if recharge == 0 {
        return Err(CanonicalAttackGroundError::UnsupportedCone(
            "ATTACK_GROUND actor is not in the recharge-hold branch",
        ));
    }
    let actor_angle = world.units.angle()[row];
    let effects = plan_attack_ground(AttackGroundFacts {
        actor_owner: i32::from(world.units.get_who(row)),
        actor_object: i32::from(world.units.o()[row]),
        actor_angle: actor_angle as u32,
        target_x: payload.att_x,
        target_y: payload.att_y,
        raw_target_angle: binding.raw_target_angle,
        side_firing: binding.side_firing,
        can_attack_ground: HostFact::known(true),
        attack_unit: payload.attack_unit,
        terrain_owner: HostFact::known(i32::from(terrain_before.owner)),
        at_peace_with_terrain_owner: HostFact::missing("foreign terrain diplomacy"),
        target_is_in_range: HostFact::known(true),
        recharge,
        order_facing_target: HostFact::known(order.flags & 0x80 != 0),
        current_animation: HostFact::known(binding.current_animation),
        redirects_to_special_cast: false,
        directional_attack_animation: false,
        has_ammo: false,
        recharge_delay: 0,
        vector_distance: 0,
        range_bias: 0,
        near_range: 0,
        far_range: 0,
        nearby_spot: HostFact::missing("attack-ground reposition search"),
        is_local_player: HostFact::missing("local-player presentation"),
    })
    .map_err(|_| {
        CanonicalAttackGroundError::UnsupportedCone("ATTACK_GROUND frame needs residual facts")
    })?;
    if !effects.is_empty() {
        return Err(CanonicalAttackGroundError::UnsupportedCone(
            "ATTACK_GROUND frame reaches a mutating residual tail",
        ));
    }
    Ok(PreparedAttackGroundHold {
        row,
        actor,
        order,
        authority: authority.clone(),
        terrain: terrain_before,
        actor_angle,
        recharge,
        random_state: world.random.state(),
    })
}

pub fn commit_attack_ground_hold(
    world: &World,
    terrain: &TerrainWorld,
    authority: &CanonicalAttackGroundAuthority,
    prepared: PreparedAttackGroundHold,
) -> Result<AttackGroundOrderState, CanonicalAttackGroundError> {
    if authority != &prepared.authority {
        return Err(CanonicalAttackGroundError::StaleAttackGroundAuthority);
    }
    if world.row_of(prepared.actor) != Some(prepared.row)
        || world.orders(prepared.row).current() != Some(&prepared.order)
        || world.units.angle()[prepared.row] != prepared.actor_angle
        || world.units.get_recharging(prepared.row) != prepared.recharge
    {
        return Err(CanonicalAttackGroundError::StaleUnit(prepared.actor));
    }
    if !terrain_still_current(terrain, prepared.terrain) {
        return Err(CanonicalAttackGroundError::StaleTerrain);
    }
    if world.random.state() != prepared.random_state {
        return Err(CanonicalAttackGroundError::StaleRandom);
    }
    Ok(prepared
        .order
        .attack_ground
        .expect("prepared typed payload"))
}
