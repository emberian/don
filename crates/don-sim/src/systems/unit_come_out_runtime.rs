//! Canonical Sim host for opcode 49's `Unit::action_come_out` transaction.
//!
//! The admitted cohort is deliberately narrow but real: one active ordinary-land captain,
//! with one live Guy, leaves an active Build owned by the same player.  Its empty gather list
//! drives all four recovered `Unit::come_out` tranches (prefix, common release, gather fallback,
//! and release tail).  Missing type, containment, Group, Order, path, collision, graphics, or
//! saved payload facts fail before the first mutation.

use crate::command::direct_entity_command_integration::plans::{
    DirectEntityCommandRequest, DirectEntityKind, DirectEntityTargetFacts,
};
use crate::command::direct_entity_command_integration::unit_action_come_out::{
    preflight_unit_action_come_out, InsideLookupFacts, ObjectIdentity as WrapperIdentity,
    UnitActionComeOutFacts, UnitActionComeOutPreflight,
};
use crate::command::direct_entity_command_integration::{
    classify_direct_entity_command, complete_unit_action_come_out_command,
    DirectEntityCommandTransactionReceipt, DirectEntityFleetReceipt, DirectEntityFleetRequest,
    DirectEntityTransactionStatus, DirectEntityTypeFacts,
};
use crate::objects::{Band, BUILD_BAND_BASE, OWNER_SLOTS};
use crate::order::OrderIndex;
use crate::systems::groups_guys::MemberState;
use crate::systems::movement_live::{
    ComeOutRelocationReceipt, ComeOutRelocationState, LiveCollisionGuy, MovementSourceState,
};
use crate::systems::unit_come_out_body_map::{
    self as body_map, CanonicalObjectIdentity, CanonicalPoint, CanonicalRngStamp,
};
use crate::systems::unit_come_out_common_release_frontier as common;
use crate::systems::unit_come_out_full_frontier as prefix;
use crate::systems::unit_come_out_gather_selection_frontier as gather;
use crate::systems::unit_come_out_release_tail_frontier as tail;
use crate::tick::Sim;
use crate::world::OBJ_FLAG_ACTIVE;

const PAYLOAD_MAGIC: [u8; 4] = *b"CO49";
const PAYLOAD_VERSION: u16 = 1;
const MAX_HINTS: usize = 128;

/// Exact non-column facts persisted beside one executable opcode-49 receiver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitComeOutResumePayload {
    pub actor: CanonicalObjectIdentity,
    pub uid: u16,
    pub type_index: i32,
    pub container: CanonicalObjectIdentity,
    pub container_uid: u16,
    pub container_type_index: i32,
    pub object_epoch: u64,
    pub order_epoch: u64,
    pub containment_epoch: u64,
    pub guy_epoch: u64,
    pub leader_epoch: u64,
    pub actor_obj_masks: u32,
    pub actor_block_radius: i32,
    pub actor_big_radius: i32,
    pub actor_unit_type_flags: u32,
    pub actor_x_size: i32,
    pub actor_y_size: i32,
    pub actor_uber_size: i32,
    pub actor_unit_flags2: u32,
    pub actor_attack: i32,
    pub actor_where_type: i32,
    pub actor_domain_query: i32,
    pub actor_type_movement_query: i32,
    pub container_angle: i32,
    pub container_block_radius: i32,
    pub container_x_size: i32,
    pub container_y_size: i32,
    pub container_inside_down: i16,
    pub container_matches_university: bool,
    pub container_matches_oil_platform: bool,
    pub container_gather_inside: bool,
    pub constants: prefix::ExitConstants,
    pub release_point: CanonicalPoint,
    pub terrain_z: i32,
    pub rng: CanonicalRngStamp,
    pub guy_hint_lengths: Vec<i32>,
    pub options_rebuild: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitComeOutPayloadError {
    Truncated,
    Magic,
    Version(u16),
    InvalidIdentity,
    InvalidBool(u8),
    TooManyHints(usize),
    TrailingBytes,
}

impl UnitComeOutResumePayload {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(192 + self.guy_hint_lengths.len() * 4);
        out.extend_from_slice(&PAYLOAD_MAGIC);
        out.extend_from_slice(&PAYLOAD_VERSION.to_le_bytes());
        put_identity(&mut out, self.actor);
        out.extend_from_slice(&self.uid.to_le_bytes());
        out.extend_from_slice(&self.type_index.to_le_bytes());
        put_identity(&mut out, self.container);
        out.extend_from_slice(&self.container_uid.to_le_bytes());
        out.extend_from_slice(&self.container_type_index.to_le_bytes());
        for epoch in [
            self.object_epoch,
            self.order_epoch,
            self.containment_epoch,
            self.guy_epoch,
            self.leader_epoch,
        ] {
            out.extend_from_slice(&epoch.to_le_bytes());
        }
        out.extend_from_slice(&self.actor_obj_masks.to_le_bytes());
        for value in [
            self.actor_block_radius,
            self.actor_big_radius,
            self.actor_x_size,
            self.actor_y_size,
            self.actor_uber_size,
            self.actor_attack,
            self.actor_where_type,
            self.actor_domain_query,
            self.actor_type_movement_query,
            self.container_angle,
            self.container_block_radius,
            self.container_x_size,
            self.container_y_size,
        ] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&self.actor_unit_type_flags.to_le_bytes());
        out.extend_from_slice(&self.actor_unit_flags2.to_le_bytes());
        out.extend_from_slice(&self.container_inside_down.to_le_bytes());
        put_bool(&mut out, self.container_matches_university);
        put_bool(&mut out, self.container_matches_oil_platform);
        put_bool(&mut out, self.container_gather_inside);
        for value in [
            self.constants.land_min,
            self.constants.land_max,
            self.constants.water_min,
            self.constants.water_max,
            self.constants.ordinary_padding,
            self.release_point.x,
            self.release_point.y,
            self.terrain_z,
        ] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&self.rng.seed.to_le_bytes());
        out.extend_from_slice(&self.rng.draws.to_le_bytes());
        out.extend_from_slice(&(self.guy_hint_lengths.len() as u16).to_le_bytes());
        for hint in &self.guy_hint_lengths {
            out.extend_from_slice(&hint.to_le_bytes());
        }
        put_bool(&mut out, self.options_rebuild);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, UnitComeOutPayloadError> {
        let mut cursor = PayloadCursor::new(bytes);
        if cursor.take(4)? != PAYLOAD_MAGIC {
            return Err(UnitComeOutPayloadError::Magic);
        }
        let version = cursor.u16()?;
        if version != PAYLOAD_VERSION {
            return Err(UnitComeOutPayloadError::Version(version));
        }
        let actor = cursor.identity()?;
        let uid = cursor.u16()?;
        let type_index = cursor.i32()?;
        let container = cursor.identity()?;
        let container_uid = cursor.u16()?;
        let container_type_index = cursor.i32()?;
        let object_epoch = cursor.u64()?;
        let order_epoch = cursor.u64()?;
        let containment_epoch = cursor.u64()?;
        let guy_epoch = cursor.u64()?;
        let leader_epoch = cursor.u64()?;
        let actor_obj_masks = cursor.u32()?;
        let actor_block_radius = cursor.i32()?;
        let actor_big_radius = cursor.i32()?;
        let actor_x_size = cursor.i32()?;
        let actor_y_size = cursor.i32()?;
        let actor_uber_size = cursor.i32()?;
        let actor_attack = cursor.i32()?;
        let actor_where_type = cursor.i32()?;
        let actor_domain_query = cursor.i32()?;
        let actor_type_movement_query = cursor.i32()?;
        let container_angle = cursor.i32()?;
        let container_block_radius = cursor.i32()?;
        let container_x_size = cursor.i32()?;
        let container_y_size = cursor.i32()?;
        let actor_unit_type_flags = cursor.u32()?;
        let actor_unit_flags2 = cursor.u32()?;
        let container_inside_down = cursor.i16()?;
        let container_matches_university = cursor.bool()?;
        let container_matches_oil_platform = cursor.bool()?;
        let container_gather_inside = cursor.bool()?;
        let constants = prefix::ExitConstants {
            land_min: cursor.i32()?,
            land_max: cursor.i32()?,
            water_min: cursor.i32()?,
            water_max: cursor.i32()?,
            ordinary_padding: cursor.i32()?,
        };
        let release_point = CanonicalPoint {
            x: cursor.i32()?,
            y: cursor.i32()?,
        };
        let terrain_z = cursor.i32()?;
        let rng = CanonicalRngStamp {
            seed: cursor.u32()?,
            draws: cursor.u64()?,
        };
        let hint_count = cursor.u16()? as usize;
        if hint_count > MAX_HINTS {
            return Err(UnitComeOutPayloadError::TooManyHints(hint_count));
        }
        let mut guy_hint_lengths = Vec::with_capacity(hint_count);
        for _ in 0..hint_count {
            guy_hint_lengths.push(cursor.i32()?);
        }
        let options_rebuild = cursor.bool()?;
        if !cursor.done() {
            return Err(UnitComeOutPayloadError::TrailingBytes);
        }
        if !actor.valid() || !container.valid() {
            return Err(UnitComeOutPayloadError::InvalidIdentity);
        }
        Ok(Self {
            actor,
            uid,
            type_index,
            container,
            container_uid,
            container_type_index,
            object_epoch,
            order_epoch,
            containment_epoch,
            guy_epoch,
            leader_epoch,
            actor_obj_masks,
            actor_block_radius,
            actor_big_radius,
            actor_unit_type_flags,
            actor_x_size,
            actor_y_size,
            actor_uber_size,
            actor_unit_flags2,
            actor_attack,
            actor_where_type,
            actor_domain_query,
            actor_type_movement_query,
            container_angle,
            container_block_radius,
            container_x_size,
            container_y_size,
            container_inside_down,
            container_matches_university,
            container_matches_oil_platform,
            container_gather_inside,
            constants,
            release_point,
            terrain_z,
            rng,
            guy_hint_lengths,
            options_rebuild,
        })
    }
}

fn put_identity(out: &mut Vec<u8>, value: CanonicalObjectIdentity) {
    out.push(value.owner as u8);
    out.extend_from_slice(&value.object.to_le_bytes());
}

fn put_bool(out: &mut Vec<u8>, value: bool) {
    out.push(u8::from(value));
}

struct PayloadCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> PayloadCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], UnitComeOutPayloadError> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or(UnitComeOutPayloadError::Truncated)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(UnitComeOutPayloadError::Truncated)?;
        self.offset = end;
        Ok(value)
    }

    fn u16(&mut self) -> Result<u16, UnitComeOutPayloadError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn i16(&mut self) -> Result<i16, UnitComeOutPayloadError> {
        Ok(i16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, UnitComeOutPayloadError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> Result<i32, UnitComeOutPayloadError> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64, UnitComeOutPayloadError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn bool(&mut self) -> Result<bool, UnitComeOutPayloadError> {
        match self.take(1)?[0] {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(UnitComeOutPayloadError::InvalidBool(value)),
        }
    }

    fn identity(&mut self) -> Result<CanonicalObjectIdentity, UnitComeOutPayloadError> {
        let owner = self.take(1)?[0] as i8;
        let object = self.i16()?;
        Ok(CanonicalObjectIdentity::new(owner, object))
    }

    fn done(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InstalledPayload {
    payload: UnitComeOutResumePayload,
    hints: Vec<i32>,
    consumed: bool,
}

/// Saved non-column owner for the admitted opcode-49 cohort.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UnitComeOutRuntime {
    installed: Vec<InstalledPayload>,
    pub options_rebuild: bool,
}

impl UnitComeOutRuntime {
    pub fn install(&mut self, payload: UnitComeOutResumePayload) -> bool {
        if !payload.actor.valid()
            || !payload.container.valid()
            || payload.guy_hint_lengths.len() > MAX_HINTS
            || self
                .installed
                .iter()
                .any(|entry| entry.payload.actor == payload.actor)
        {
            return false;
        }
        self.options_rebuild = payload.options_rebuild;
        self.installed.push(InstalledPayload {
            hints: payload.guy_hint_lengths.clone(),
            payload,
            consumed: false,
        });
        true
    }

    pub fn resume(&mut self, bytes: &[u8]) -> Result<(), UnitComeOutPayloadError> {
        let payload = UnitComeOutResumePayload::decode(bytes)?;
        if self.install(payload) {
            Ok(())
        } else {
            Err(UnitComeOutPayloadError::InvalidIdentity)
        }
    }

    pub fn save(&self, actor: CanonicalObjectIdentity) -> Option<Vec<u8>> {
        let entry = self
            .installed
            .iter()
            .find(|entry| entry.payload.actor == actor && !entry.consumed)?;
        let mut payload = entry.payload.clone();
        payload.guy_hint_lengths.clone_from(&entry.hints);
        payload.options_rebuild = self.options_rebuild;
        Some(payload.encode())
    }

    fn entry(&self, actor: CanonicalObjectIdentity) -> Option<&InstalledPayload> {
        self.installed
            .iter()
            .find(|entry| entry.payload.actor == actor && !entry.consumed)
    }

    fn entry_mut(&mut self, actor: CanonicalObjectIdentity) -> Option<&mut InstalledPayload> {
        self.installed
            .iter_mut()
            .find(|entry| entry.payload.actor == actor && !entry.consumed)
    }
}

/// Group owner image coupled to `UnitData::group`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComeOutGroupState {
    pub slot: i16,
    pub id: i32,
    pub priority: i32,
    pub members: i32,
    pub contains_actor: bool,
}

/// Graphics fields written by the admitted one-Guy `Unit::set_new_location` cohort.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComeOutGraphicsState {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub angle: i32,
    pub last_x: i32,
    pub last_y: i32,
    pub last_z: i32,
    pub last_angle: i32,
    pub des_x: i32,
    pub des_y: i32,
    pub des_angle: i32,
}

/// Recomputable canonical state image surrounding one opcode-49 transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitComeOutCanonicalState {
    pub actor: CanonicalObjectIdentity,
    pub uid: u16,
    pub type_index: i32,
    pub point: CanonicalPoint,
    pub z: i32,
    pub angle: i32,
    pub unit_masks: u32,
    pub unit_group: i16,
    pub group: Option<ComeOutGroupState>,
    pub order_count: usize,
    pub path_count: i32,
    pub inside: (i8, i16),
    pub movement: MovementSourceState,
    pub collision_guy: LiveCollisionGuy,
    pub graphics: ComeOutGraphicsState,
    pub guy_hint_lengths: Vec<i32>,
    pub options_rebuild: bool,
}

/// Full four-tranche proof retained on the command receipt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitComeOutTransactionReceipt {
    pub payload: UnitComeOutResumePayload,
    pub wrapper: UnitActionComeOutPreflight,
    pub prefix_facts: prefix::UnitComeOutPrefixFacts,
    pub prefix_plan: prefix::UnitComeOutPrefixPlan,
    pub common_facts: common::CommonReleaseFacts,
    pub common_plan: common::CommonReleasePlan,
    pub gather_facts: gather::GatherSelectionFacts,
    pub gather_plan: gather::GatherSelectionPlan,
    pub tail_facts: tail::ReleaseTailFacts,
    pub tail_receipts: Vec<tail::HostCallReceipt>,
    pub tail_plan: tail::ReleaseTailPlan,
    pub relocation: ComeOutRelocationReceipt,
    pub before: UnitComeOutCanonicalState,
    pub after: UnitComeOutCanonicalState,
}

impl UnitComeOutTransactionReceipt {
    /// Re-run the wrapper and every reached tranche, then verify every typed seam and the
    /// canonical before/after transition.
    pub fn validates(&self) -> bool {
        if body_map::check_body_map().is_err()
            || !self.payload.actor.valid()
            || !self.payload.container.valid()
            || self.payload.actor != self.before.actor
            || self.payload.uid != self.before.uid
            || self.payload.type_index != self.before.type_index
        {
            return false;
        }
        let Ok(wrapper) = preflight_unit_action_come_out(
            self.wrapper.object_epoch,
            self.wrapper.order_epoch,
            self.wrapper.containment_epoch,
            self.wrapper.guy_epoch,
            self.wrapper.leader_epoch,
            self.wrapper.facts.clone(),
        ) else {
            return false;
        };
        if wrapper != self.wrapper {
            return false;
        }
        let Ok(prefix_plan) = prefix::plan_unit_come_out_prefix(&self.prefix_facts) else {
            return false;
        };
        if prefix_plan != self.prefix_plan {
            return false;
        }
        let prefix::UnitComeOutPrefixExit::ContinueAtCommonRelease(prefix_continuation) =
            prefix_plan.exit
        else {
            return false;
        };
        if body_map::common_continuation(prefix_continuation) != self.common_facts.prefix {
            return false;
        }
        let Ok(common_plan) = common::plan_unit_come_out_common_release(&self.common_facts) else {
            return false;
        };
        if common_plan != self.common_plan {
            return false;
        }
        let common::CommonReleaseExit::GatherList(common_continuation) = common_plan.exit else {
            return false;
        };
        if body_map::gather_input(common_continuation).as_ref() != Some(&self.gather_facts.input) {
            return false;
        }
        let Ok(gather_plan) = gather::plan_unit_come_out_gather_selection(&self.gather_facts)
        else {
            return false;
        };
        if gather_plan != self.gather_plan
            || gather_plan.continuation.resume_va != tail::RELEASE_TAIL_START_VA
        {
            return false;
        }
        let Ok(tail_plan) = tail::plan_unit_come_out_release_tail(
            &self.tail_facts,
            &self.tail_receipts,
            CanonicalRngStamp::from(gather_plan.continuation.rng).into(),
        ) else {
            return false;
        };
        if tail_plan != self.tail_plan
            || !tail_plan.boundaries.is_empty()
            || tail_plan.exit != tail::ReleaseTailExit::Returned(0)
            || !tail_plan
                .steps
                .contains(&tail::ReleaseTailStep::SetOptionsRebuild)
        {
            return false;
        }

        let expected_masks = self.before.unit_masks
            & !crate::command::direct_entity_command_integration::unit_action_come_out::LAUNCHING_UNIT_MASK;
        let expected_group = self.before.group.map(|mut group| {
            group.members -= 1;
            group.contains_actor = false;
            group
        });
        let expected_graphics = ComeOutGraphicsState {
            x: self.payload.release_point.x,
            y: self.payload.release_point.y,
            z: self.payload.terrain_z,
            angle: self.payload.container_angle,
            last_x: self.payload.release_point.x,
            last_y: self.payload.release_point.y,
            last_z: self.payload.terrain_z,
            last_angle: self.payload.container_angle,
            des_x: self.payload.release_point.x,
            des_y: self.payload.release_point.y,
            des_angle: self.payload.container_angle,
        };
        let expected_after = UnitComeOutCanonicalState {
            actor: self.before.actor,
            uid: self.before.uid,
            type_index: self.before.type_index,
            point: self.payload.release_point,
            z: self.payload.terrain_z,
            angle: self.payload.container_angle,
            unit_masks: expected_masks,
            unit_group: -1,
            group: expected_group,
            order_count: 0,
            path_count: 0,
            inside: (-1, -1),
            movement: self.relocation.after.source,
            collision_guy: self.relocation.after.guy,
            graphics: expected_graphics,
            guy_hint_lengths: vec![0; self.before.guy_hint_lengths.len()],
            options_rebuild: true,
        };
        self.relocation.before.source == self.before.movement
            && self.relocation.before.point == (self.before.point.x, self.before.point.y)
            && self.relocation.before.angle == self.before.angle
            && self.relocation.before.inside == self.before.inside
            && self.relocation.before.guy == self.before.collision_guy
            && self.relocation.after.point
                == (self.payload.release_point.x, self.payload.release_point.y)
            && self.relocation.after.angle == self.payload.container_angle
            && self.relocation.after.inside == (-1, -1)
            && self.relocation.after.source.revision
                == self.relocation.before.source.revision.wrapping_add(1)
            && !self.relocation.after.source.moving
            && self.relocation.after.source.action == OrderIndex::None as i32
            && self.after == expected_after
    }
}

fn graphics_state(guy: &crate::systems::groups_guys::GuyData) -> ComeOutGraphicsState {
    ComeOutGraphicsState {
        x: guy.x,
        y: guy.y,
        z: guy.z,
        angle: guy.angle,
        last_x: guy.last_x,
        last_y: guy.last_y,
        last_z: guy.last_z,
        last_angle: guy.last_angle,
        des_x: guy.des_x,
        des_y: guy.des_y,
        des_angle: guy.des_angle,
    }
}

fn group_state(sim: &Sim, owner: usize, actor: i16, slot: i16) -> Option<ComeOutGroupState> {
    if slot < 0 {
        return None;
    }
    let slot_index = usize::try_from(slot).ok()?;
    if slot_index >= crate::systems::groups_guys::GROUPS_PER_PLAYER {
        return None;
    }
    let group = sim.groups.get(owner, slot_index);
    Some(ComeOutGroupState {
        slot,
        id: group.id,
        priority: group.priority,
        members: group.num,
        contains_actor: group.member(actor),
    })
}

fn snapshot_state(
    sim: &Sim,
    runtime: &UnitComeOutRuntime,
    row: usize,
    payload: &UnitComeOutResumePayload,
) -> Option<UnitComeOutCanonicalState> {
    let handle = sim.world.handle_at_row(row)?;
    let movement = sim.movement_source_state(handle).ok()?;
    let collision_guy = *sim.movement_collision.source(row)?.guys.first()?;
    let crash = sim.crash_units.get(row)?.as_ref()?;
    if crash.guys.guy_mark != 1 || crash.guys.guys.len() != 1 {
        return None;
    }
    let graphics = graphics_state(crash.guys.guys[0].as_ref()?);
    let entry = runtime.entry(payload.actor)?;
    let group_slot = sim.world.units.group()[row];
    let group = group_state(
        sim,
        payload.actor.owner as usize,
        payload.actor.object,
        group_slot,
    );
    if group_slot >= 0 && group.is_none() {
        return None;
    }
    Some(UnitComeOutCanonicalState {
        actor: payload.actor,
        uid: sim.world.units.get_uid(row),
        type_index: *sim.unit_type.get(row)?,
        point: CanonicalPoint {
            x: sim.world.units.x_internal()[row],
            y: sim.world.units.y_internal()[row],
        },
        z: sim.world.units.z_internal()[row],
        angle: sim.world.units.angle()[row],
        unit_masks: sim.world.units.get_unit_masks(row),
        unit_group: group_slot,
        group,
        order_count: sim.world.orders(row).len(),
        path_count: sim.paths.get(row)?.len(),
        inside: (
            sim.world.units.inside_up_who()[row],
            sim.world.units.inside_up()[row],
        ),
        movement,
        collision_guy,
        graphics,
        guy_hint_lengths: entry.hints.clone(),
        options_rebuild: runtime.options_rebuild,
    })
}

/// Execute one decoded opcode-49 request against the canonical Sim owners.
pub fn process_sim_unit_come_out_command(
    sim: &mut Sim,
    runtime: &mut UnitComeOutRuntime,
    request: DirectEntityCommandRequest,
    frame: i32,
) -> DirectEntityCommandTransactionReceipt {
    let DirectEntityCommandRequest::ComeOut {
        who, object_index, ..
    } = request
    else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };
    let Some(owner) = usize::try_from(who)
        .ok()
        .filter(|owner| *owner < OWNER_SLOTS)
    else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };
    let Some(object) = usize::try_from(object_index).ok() else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };
    let Some(row) = sim
        .world
        .objects
        .slot(owner)
        .band(Band::Unit)
        .get(object)
        .copied()
        .map(|row| row as usize)
    else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };
    if row >= sim.world.live_count() as usize
        || sim.world.units.get_who(row) as usize != owner
        || usize::try_from(sim.world.units.o()[row]).ok() != Some(object)
    {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    }

    let target = DirectEntityTargetFacts {
        kind: DirectEntityKind::Unit,
        active: sim.world.units.get_flags(row) & OBJ_FLAG_ACTIVE != 0,
        uid: sim.world.units.get_uid(row),
    };
    let prefix_without_type = classify_direct_entity_command(request, frame, Some(target), None);
    if prefix_without_type.status == DirectEntityTransactionStatus::Complete {
        return prefix_without_type;
    }
    let Some(type_index) = sim
        .unit_type
        .get(row)
        .copied()
        .filter(|type_index| *type_index >= 0)
    else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };
    let type_facts = DirectEntityTypeFacts {
        kind: DirectEntityKind::Unit,
        type_index,
    };
    let command_prefix =
        classify_direct_entity_command(request, frame, Some(target), Some(type_facts));
    if command_prefix.status != DirectEntityTransactionStatus::OpenTail {
        return command_prefix;
    }

    let actor = CanonicalObjectIdentity::new(who as i8, object_index as i16);
    let Some(entry) = runtime.entry(actor) else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };
    let payload = entry.payload.clone();
    if payload.actor != actor
        || payload.uid != target.uid
        || payload.type_index != type_index
        || payload.rng.seed != sim.world.random.state() as u32
        || payload.guy_hint_lengths != entry.hints
        || payload.options_rebuild != runtime.options_rebuild
        || payload.actor.owner < 0
        || payload.actor.owner as usize != owner
        || payload.actor.object != object_index as i16
        || payload.container.owner != payload.actor.owner
        || payload.container.object < BUILD_BAND_BASE as i16
        || payload.container_matches_university
        || payload.container_matches_oil_platform
        || payload.actor_obj_masks & prefix::DIRECT_CONTAINER_LOCATION_MASK != 0
        || matches!(type_index, 0x34 | 0x35 | 0x13b)
        || payload.actor_uber_size != 1
        || payload.actor_block_radius <= 0
        || payload.container_block_radius < 0
        || payload.guy_hint_lengths.len() != 1
        || sim.step8.leaders[owner].flags & common::LEADER_COMMAND_FLAG == 0
    {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    }

    let container_offset = i32::from(payload.container.object)
        .checked_sub(BUILD_BAND_BASE as i32)
        .and_then(|offset| usize::try_from(offset).ok());
    let Some(container_row) = container_offset
        .and_then(|offset| sim.world.objects.slot(owner).band(Band::Build).get(offset))
        .copied()
        .map(|row| row as usize)
    else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };
    let Some(build) = sim.builds.get(container_row) else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };
    if build.who as usize != owner
        || build.object_id() != payload.container.object
        || build.uid != payload.container_uid
        || build.orig_type != payload.container_type_index
        || !build.is_valid()
        || !build.is_active()
        || build.flags & common::ACTIVE_BUILD_FLAG != 0
        || !build.gather.is_empty()
        || payload.container_inside_down >= 0
        || sim.world.units.inside_up()[row] != payload.container.object
        || sim.world.units.inside_up_who()[row] != payload.container.owner
    {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    }
    let build_point = CanonicalPoint {
        x: build.position().0,
        y: build.position().1,
    };

    let Some(handle) = sim.world.handle_at_row(row) else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };
    let Some(source) = sim.movement_collision.source(row) else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };
    if source.domain != 0
        || !source.ordinary_land()
        || !source.captain
        || source.block_radius != payload.actor_block_radius
        || source.big_radius != payload.actor_big_radius
        || source.unit_flags != payload.actor_unit_type_flags
        || source.unit_flags2 != payload.actor_unit_flags2
        || source.attack_value != payload.actor_attack
        || source.guys.len() != 1
    {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    }
    let Some(crash) = sim.crash_units.get(row).and_then(Option::as_ref) else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };
    let Some(graphics_guy) = crash.guys.guys.first().and_then(Option::as_ref) else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };
    if crash.guys.guy_mark != 1
        || crash.guys.guys.len() != 1
        || graphics_guy.ty != type_index
        || graphics_guy.who != payload.actor.owner
        || graphics_guy.o != payload.actor.object
        || (graphics_guy.x, graphics_guy.y) != (source.guys[0].x, source.guys[0].y)
        || graphics_guy.angle != source.guys[0].angle
    {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    }
    let Some(before) = snapshot_state(sim, runtime, row, &payload) else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };
    if before.inside != (payload.container.owner, payload.container.object)
        || before.group.is_some_and(|group| {
            group.id != i32::from(group.slot)
                || group.priority != 0
                || !group.contains_actor
                || group.members <= 0
        })
    {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    }

    let wrapper_facts = UnitActionComeOutFacts {
        actor: WrapperIdentity::new(payload.actor.owner as u8, payload.actor.object),
        actor_type: payload.type_index,
        unit_masks: before.unit_masks,
        inside: InsideLookupFacts {
            container: Some(WrapperIdentity::new(
                payload.container.owner as u8,
                payload.container.object,
            )),
            container_is_build: None,
            container_is_university: None,
        },
        actor_first_guy: None,
        actor_inside_down: None,
        inside_chain: Vec::new(),
        leader_flags: None,
    };
    let Ok(wrapper) = preflight_unit_action_come_out(
        payload.object_epoch,
        payload.order_epoch,
        payload.containment_epoch,
        payload.guy_epoch,
        payload.leader_epoch,
        wrapper_facts,
    ) else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };

    let prefix_actor: prefix::ObjectIdentity = payload.actor.into();
    let prefix_container: prefix::ObjectIdentity = payload.container.into();
    let prefix_point: prefix::Point = before.point.into();
    let build_prefix_point: prefix::Point = build_point.into();
    let release_prefix_point: prefix::Point = payload.release_point.into();
    let search_request = prefix::NearbyRequest {
        receiver_type: payload.type_index,
        centre: build_prefix_point,
        min_radius: payload.container_block_radius,
        max_radius: payload
            .constants
            .ordinary_padding
            .wrapping_add(payload.container_block_radius),
        radial_step: 0,
        base_angle: payload.container_angle as u32,
        filter: 3,
        actor: prefix_actor,
        accept_without_collision: 0,
        expanded: 0,
        overlap_object: -1,
        overlap_owner: 0,
        required_region: -1,
    };
    let container_facts = prefix::ContainerFacts {
        identity: prefix_container,
        point: build_prefix_point,
        angle: payload.container_angle as u32,
        matches_university: false,
        matches_oil_platform: false,
        is_build: true,
        is_wallbuild: false,
        gpiece: build.gpiece,
        object_flags_low: build.flags,
        type_block_radius: payload.container_block_radius,
        type_x_size: payload.container_x_size,
        type_y_size: payload.container_y_size,
        gather: Some(prefix::GatherGateFacts {
            list_non_empty: false,
            gather_inside: payload.container_gather_inside,
            direction: None,
        }),
    };
    let prefix_facts = prefix::UnitComeOutPrefixFacts {
        actor: prefix_actor,
        argument: 0,
        point: prefix_point,
        actor_type: payload.type_index,
        actor_domain: source.domain,
        actor_obj_masks: payload.actor_obj_masks,
        actor_block_radius: payload.actor_block_radius,
        actor_big_radius: payload.actor_big_radius,
        actor_is_captain: true,
        captain_object: None,
        guy_hint_lengths: before.guy_hint_lengths.clone(),
        first_guy_turret_inc_bits: None,
        constants: payload.constants,
        initial_rng: payload.rng.into(),
        captain_recursive: None,
        inside: Some(prefix::InsideFacts {
            direct_container: container_facts,
            placement_container: Some(container_facts),
            leader_flags_before: None,
            oil_bridge: None,
            placement_searches: vec![prefix::NearbyObservation {
                request: search_request,
                outcome: prefix::NearbyOutcome::Found(release_prefix_point),
            }],
            terrain_z: Some(payload.terrain_z),
        }),
        uncontained_searches: Vec::new(),
    };
    let Ok(prefix_plan) = prefix::plan_unit_come_out_prefix(&prefix_facts) else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };
    let prefix::UnitComeOutPrefixExit::ContinueAtCommonRelease(prefix_continuation) =
        prefix_plan.exit
    else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };

    let common_prefix = body_map::common_continuation(prefix_continuation);
    let common_rng = common_prefix.rng;
    let common_facts = common::CommonReleaseFacts {
        prefix: common_prefix,
        actor: common::ActorReleaseFacts {
            unit_masks: before.unit_masks
                & !crate::command::direct_entity_command_integration::unit_action_come_out::LAUNCHING_UNIT_MASK,
            unit_masks2: payload.actor_unit_flags2,
            domain: source.domain,
            unit_type_flags: payload.actor_unit_type_flags,
            type_index: payload.type_index,
            is_plane_base_slot: true,
            is_plane: false,
            is_captain: true,
            matches_nuke_family: false,
            uber_size: payload.actor_uber_size,
            path_length: 0,
            leader_flags: sim.step8.leaders[owner].flags,
            nukes_launched_before: 0,
        },
        first_guy: None,
        down_unit: None,
        order_head: None,
        captain_container: Some(common::CaptainContainerFacts {
            identity: payload.container.into(),
            kind: common::CaptainContainerKind::Build(common::BuildExitFacts {
                flags_low: build.flags,
                inside_down: payload.container_inside_down,
                city_index: build.city,
                city_flags_before: None,
                owner_object_slots: 0,
                worker_scan: Vec::new(),
            }),
        }),
        host_receipts: vec![common::HostCallReceipt {
            kind: common::HostCallKind::SetNewLocation,
            result: common::HostReturn::I32(1),
            before: common_rng,
            after: common_rng,
        }],
    };
    let Ok(common_plan) = common::plan_unit_come_out_common_release(&common_facts) else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };
    let common::CommonReleaseExit::GatherList(common_continuation) = common_plan.exit else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };
    let Some(gather_input) = body_map::gather_input(common_continuation) else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };
    let gather_facts = gather::GatherSelectionFacts {
        input: gather_input,
        actor: gather::ActorSelectionFacts {
            attack: payload.actor_attack,
            uber_size: payload.actor_uber_size,
            is_captain_base_slot: true,
            captain_bit: true,
        },
        has_gather_head: false,
        points: Vec::new(),
        host_receipts: Vec::new(),
    };
    let Ok(gather_plan) = gather::plan_unit_come_out_gather_selection(&gather_facts) else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };

    let tail_facts = tail::ReleaseTailFacts {
        actor: tail::ActorFacts {
            identity: payload.actor.into(),
            type_index: payload.type_index,
            where_type: payload.actor_where_type,
            attack: payload.actor_attack,
            x_size: payload.actor_x_size,
            y_size: payload.actor_y_size,
            uber_size: payload.actor_uber_size,
            unit_flags2: payload.actor_unit_flags2,
            unit_masks: before.unit_masks
                & !crate::command::direct_entity_command_integration::unit_action_come_out::LAUNCHING_UNIT_MASK,
            on_map: true,
            domain_query: payload.actor_domain_query,
            type_movement_query: payload.actor_type_movement_query,
            o_down_chain: Vec::new(),
        },
        container: Some(tail::ContainerFacts {
            identity: payload.container.into(),
            active: build.is_active(),
            angle: payload.container_angle,
            position: build_point.into(),
        }),
        selection: tail::SelectionFacts {
            selected: gather_plan.continuation.terminal_selection,
            point: CanonicalPoint::from(gather_plan.continuation.selected).into(),
            action: gather_plan.continuation.action,
            scratch_group: gather_plan
                .continuation
                .input
                .scratch_group
                .unwrap_or(-1),
            container_gpiece: gather_plan.continuation.input.container_gpiece,
            spec_anim: 0,
        },
        constants: tail::ConstantsFacts {
            unit_train_distance: payload.constants.land_min,
            unit_train_max_distance: payload.constants.land_max,
        },
        game_frame: frame as u32,
        where_probes: None,
        army_probes: None,
    };
    let tail_receipts = Vec::new();
    let Ok(tail_plan) = tail::plan_unit_come_out_release_tail(
        &tail_facts,
        &tail_receipts,
        CanonicalRngStamp::from(gather_plan.continuation.rng).into(),
    ) else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };
    if !tail_plan.boundaries.is_empty() {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    }

    let relocation_before = ComeOutRelocationState {
        source: before.movement,
        point: (before.point.x, before.point.y),
        angle: before.angle,
        inside: before.inside,
        guy: before.collision_guy,
        linked: false,
    };
    let collision_guy_after = LiveCollisionGuy {
        x: payload.release_point.x,
        y: payload.release_point.y,
        angle: payload.container_angle,
        ..before.collision_guy
    };
    let movement_after = MovementSourceState {
        revision: before.movement.revision.wrapping_add(1),
        moving: false,
        action: OrderIndex::None as i32,
        ..before.movement
    };
    let relocation = ComeOutRelocationReceipt {
        before: relocation_before,
        after: ComeOutRelocationState {
            source: movement_after,
            point: (payload.release_point.x, payload.release_point.y),
            angle: payload.container_angle,
            inside: (-1, -1),
            guy: collision_guy_after,
            linked: true,
        },
    };
    let mut after = before.clone();
    after.point = payload.release_point;
    after.z = payload.terrain_z;
    after.angle = payload.container_angle;
    after.unit_masks &= !crate::command::direct_entity_command_integration::unit_action_come_out::LAUNCHING_UNIT_MASK;
    after.unit_group = -1;
    if let Some(group) = after.group.as_mut() {
        group.members -= 1;
        group.contains_actor = false;
    }
    after.order_count = 0;
    after.path_count = 0;
    after.inside = (-1, -1);
    after.movement = movement_after;
    after.collision_guy = collision_guy_after;
    after.graphics = ComeOutGraphicsState {
        x: payload.release_point.x,
        y: payload.release_point.y,
        z: payload.terrain_z,
        angle: payload.container_angle,
        last_x: payload.release_point.x,
        last_y: payload.release_point.y,
        last_z: payload.terrain_z,
        last_angle: payload.container_angle,
        des_x: payload.release_point.x,
        des_y: payload.release_point.y,
        des_angle: payload.container_angle,
    };
    after.guy_hint_lengths.fill(0);
    after.options_rebuild = true;

    let receiver = UnitComeOutTransactionReceipt {
        payload: payload.clone(),
        wrapper,
        prefix_facts,
        prefix_plan,
        common_facts,
        common_plan,
        gather_facts,
        gather_plan,
        tail_facts,
        tail_receipts,
        tail_plan,
        relocation,
        before: before.clone(),
        after,
    };
    if !receiver.validates() {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    }
    let completed = complete_unit_action_come_out_command(
        request,
        frame,
        Some(target),
        Some(type_facts),
        receiver.clone(),
    );
    if completed.status != DirectEntityTransactionStatus::Complete || !completed.validates(request)
    {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    }

    let Ok(actual_relocation) = sim.movement_collision.release_contained_for_come_out(
        &mut sim.world,
        &mut sim.map.world,
        handle,
        before.movement.revision,
        (payload.container.owner, payload.container.object),
        (payload.release_point.x, payload.release_point.y),
        payload.container_angle,
    ) else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };
    if actual_relocation != receiver.relocation {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    }

    sim.world.units.unit_masks_mut()[row] = receiver.after.unit_masks as i32;
    sim.world.units.z_internal_mut()[row] = payload.terrain_z;
    sim.world.units.group_mut()[row] = -1;
    sim.world.orders_mut(row).clear();
    sim.paths[row].clear();
    if let Some(group) = before.group {
        let actor_object = payload.actor.object;
        sim.groups
            .get_mut(owner, group.slot as usize)
            .normalize(&|member| {
                if member == actor_object {
                    MemberState::NotOurUnit
                } else {
                    MemberState::Keep
                }
            });
    }
    let crash = sim.crash_units[row]
        .as_mut()
        .expect("graphics source was preflighted");
    let guy = crash.guys.guys[0]
        .as_mut()
        .expect("one live graphics Guy was preflighted");
    guy.x = payload.release_point.x;
    guy.y = payload.release_point.y;
    guy.z = payload.terrain_z;
    guy.angle = payload.container_angle;
    guy.last_x = payload.release_point.x;
    guy.last_y = payload.release_point.y;
    guy.last_z = payload.terrain_z;
    guy.last_angle = payload.container_angle;
    guy.des_x = payload.release_point.x;
    guy.des_y = payload.release_point.y;
    guy.des_angle = payload.container_angle;
    let entry = runtime
        .entry_mut(payload.actor)
        .expect("resume payload was preflighted");
    entry.hints.fill(0);
    entry.consumed = true;
    runtime.options_rebuild = true;
    completed
}

/// Fleet envelope adapter used by real Bridge hosts. Other entity/market rows stay unavailable.
pub fn apply_sim_come_out_fleet_transaction(
    sim: &mut Sim,
    runtime: &mut UnitComeOutRuntime,
    envelope: DirectEntityFleetRequest,
) -> DirectEntityFleetReceipt {
    match envelope {
        DirectEntityFleetRequest::Entity { request, frame } => DirectEntityFleetReceipt::Entity(
            process_sim_unit_come_out_command(sim, runtime, request, frame),
        ),
        DirectEntityFleetRequest::Market { .. } => DirectEntityFleetReceipt::unavailable(envelope),
    }
}
